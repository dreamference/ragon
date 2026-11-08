//! Hysteresis (§4.2): an item enters `degraded` or `down` after its threshold holds for N
//! consecutive samples and leaves after M good ones and at least 2 minutes; three state changes
//! within 30 minutes hold it at `degraded` ("flapping") until it has been stable for 30 minutes.
//! Time is seconds the caller supplies.

use crate::{Item, State};
use std::collections::HashMap;

pub const FLAP_CHANGES: usize = 3;
pub const FLAP_WINDOW_S: u64 = 30 * 60;
pub const MIN_HOLD_S: u64 = 120;

#[derive(Clone, Copy, Debug)]
pub struct Hysteresis {
    pub enter: u32,
    pub leave: u32,
}

impl Default for Hysteresis {
    fn default() -> Self {
        Hysteresis { enter: 3, leave: 3 }
    }
}

impl Hysteresis {
    /// The daily and hourly probes: one sample is enough.
    pub fn single() -> Self {
        Hysteresis { enter: 1, leave: 1 }
    }
}

#[derive(Debug)]
struct Entry {
    shown: State,
    since: u64,
    rising: Option<(State, u32)>,
    good: u32,
    changes: Vec<u64>,
    flapping: bool,
}

#[derive(Debug, Default)]
pub struct Tracker {
    entries: HashMap<String, Entry>,
}

fn rank(s: State) -> u8 {
    match s {
        State::Down => 2,
        State::Degraded => 1,
        _ => 0,
    }
}

impl Tracker {
    /// Feeds one sample of every item and returns the items with their hysteretic state.
    pub fn observe(&mut self, now: u64, items: &[Item], h: Hysteresis) -> Vec<Item> {
        items.iter().map(|i| self.observe_one(now, i, h)).collect()
    }

    fn observe_one(&mut self, now: u64, item: &Item, h: Hysteresis) -> Item {
        let e = self.entries.entry(item.name.clone()).or_insert(Entry {
            shown: State::Ok,
            since: now,
            rising: None,
            good: 0,
            changes: Vec::new(),
            flapping: false,
        });
        let raw = item.state;
        let before = e.shown;
        match raw {
            // A feature that is off, or a probe that could not run, is not a sample of health.
            State::Off => {
                *e = Entry {
                    shown: State::Off,
                    since: now,
                    rising: None,
                    good: 0,
                    changes: Vec::new(),
                    flapping: false,
                };
                return item.clone();
            }
            State::Unknown => return item.clone(),
            _ if rank(raw) > rank(e.shown) => {
                e.good = 0;
                let n = match e.rising {
                    Some((s, n)) if s == raw => n + 1,
                    _ => 1,
                };
                e.rising = Some((raw, n));
                if n >= h.enter {
                    e.shown = raw;
                    e.rising = None;
                }
            }
            _ if rank(raw) < rank(e.shown) => {
                e.rising = None;
                e.good += 1;
                if e.good >= h.leave && now.saturating_sub(e.since) >= MIN_HOLD_S {
                    e.shown = raw;
                    e.good = 0;
                }
            }
            _ => {
                e.rising = None;
                e.good = 0;
            }
        }
        if e.shown != before {
            e.since = now;
            e.changes.retain(|t| now.saturating_sub(*t) < FLAP_WINDOW_S);
            e.changes.push(now);
            if e.changes.len() >= FLAP_CHANGES {
                e.flapping = true;
            }
        }
        if e.flapping
            && e.changes
                .last()
                .is_some_and(|t| now.saturating_sub(*t) >= FLAP_WINDOW_S)
        {
            e.flapping = false;
            e.changes.clear();
        }
        let mut out = item.clone();
        out.state = e.shown;
        if e.flapping {
            out.state = State::Degraded;
            out.detail = format!("{} (flapping)", item.detail);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Weight;

    fn at(t: &mut Tracker, now: u64, s: State) -> Item {
        t.observe(
            now,
            &[Item::new("x", s, Weight::Item, "d")],
            Hysteresis::default(),
        )
        .remove(0)
    }

    #[test]
    fn a_threshold_must_hold_for_three_samples_to_enter() {
        let mut t = Tracker::default();
        assert_eq!(at(&mut t, 0, State::Down).state, State::Ok);
        assert_eq!(at(&mut t, 10, State::Down).state, State::Ok);
        assert_eq!(at(&mut t, 20, State::Down).state, State::Down);
    }

    #[test]
    fn a_good_sample_in_between_resets_the_count() {
        let mut t = Tracker::default();
        at(&mut t, 0, State::Down);
        at(&mut t, 10, State::Ok);
        at(&mut t, 20, State::Down);
        assert_eq!(at(&mut t, 30, State::Down).state, State::Ok);
    }

    #[test]
    fn leaving_needs_three_good_samples_and_two_minutes() {
        let mut t = Tracker::default();
        for s in 0..3 {
            at(&mut t, s * 10, State::Down);
        }
        for s in 3..6 {
            assert_eq!(at(&mut t, s * 10, State::Ok).state, State::Down);
        }
        assert_eq!(at(&mut t, 20 + 120, State::Ok).state, State::Ok);
    }

    #[test]
    fn three_changes_in_half_an_hour_hold_degraded_as_flapping_until_stable() {
        let mut t = Tracker::default();
        let h = Hysteresis::single();
        let mut run = |now: u64, s: State| {
            t.observe(now, &[Item::new("x", s, Weight::Item, "d")], h)
                .remove(0)
        };
        assert_eq!(run(0, State::Down).state, State::Down);
        assert_eq!(run(200, State::Ok).state, State::Ok);
        let third = run(400, State::Down);
        assert_eq!(third.state, State::Degraded);
        assert!(third.detail.contains("flapping"));
        // Still held while changes are recent, released after 30 quiet minutes.
        assert!(run(800, State::Ok).detail.contains("flapping"));
        let released = run(800 + FLAP_WINDOW_S + 1, State::Ok);
        assert_eq!(released.state, State::Ok);
        assert!(!released.detail.contains("flapping"));
    }

    #[test]
    fn unknown_and_off_pass_through() {
        let mut t = Tracker::default();
        assert_eq!(at(&mut t, 0, State::Unknown).state, State::Unknown);
        assert_eq!(at(&mut t, 10, State::Off).state, State::Off);
    }
}
