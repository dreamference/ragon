//! The sampling schedule (specs/DREAMFERENCE_MONITORING.md §3): each probe group runs at its own
//! interval, the fastest interval its items have in §3. `tick` is called about once a second with
//! the time; it runs the groups that are due and writes their items to the store. Time is a
//! parameter, so the tests need no clock.

use crate::host::Host;
use crate::store::Store;
use crate::{Config, Item, disk, memory, server, units};
use std::io;

/// A group of probes sharing an interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Engine health (15 s), the models list and the counters ride along.
    Server,
    /// PSI and MemAvailable (10 s), swap and OOM kills ride along.
    Memory,
    /// Filesystems (5 min).
    Disk,
    /// Units, sidecars, Docker (60 s).
    Units,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Memory, Group::Server, Group::Units, Group::Disk];

    /// The interval in seconds.
    pub fn interval(self) -> i64 {
        match self {
            Group::Memory => 10,
            Group::Server => 15,
            Group::Units => 60,
            Group::Disk => 300,
        }
    }

    fn probe(self, host: &dyn Host, config: &Config) -> Vec<Item> {
        match self {
            Group::Server => server::probe(host, config),
            Group::Memory => memory::probe(host),
            Group::Disk => disk::probe(host),
            Group::Units => units::probe(host),
        }
    }
}

/// When each group is next due, and when the store last had its hourly pass.
pub struct Sampler {
    due: [i64; 4],
    last_maintain: i64,
}

impl Sampler {
    /// Every group is due at `start`.
    pub fn new(start: i64) -> Sampler {
        Sampler {
            due: [start; 4],
            last_maintain: start,
        }
    }

    /// Runs what is due at `now`, records it, and does the hourly pass. Returns the items sampled.
    pub fn tick(
        &mut self,
        now: i64,
        host: &dyn Host,
        config: &Config,
        store: &mut Store,
    ) -> io::Result<Vec<Item>> {
        let mut items = Vec::new();
        for (slot, group) in Group::ALL.iter().enumerate() {
            if now >= self.due[slot] {
                // A late tick does not queue up a burst: the next one is an interval from now.
                self.due[slot] = now + group.interval();
                items.extend(group.probe(host, config));
            }
        }
        if !items.is_empty() {
            store.record(now, &items)?;
        }
        if now - self.last_maintain >= 3600 {
            self.last_maintain = now;
            store.maintain(now)?;
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    #[test]
    fn each_group_samples_at_its_own_interval() {
        let host = FakeHost::default();
        let config = Config::default();
        let mut store = Store::in_memory().unwrap();
        let mut s = Sampler::new(1000);
        let mut counts = [0usize; 4];
        for now in 1000..1000 + 600 {
            let before = store.sample_count();
            let items = s.tick(now, &host, &config, &mut store).unwrap();
            assert_eq!((store.sample_count() - before) as usize, items.len());
            for (i, g) in Group::ALL.iter().enumerate() {
                let due = (now - 1000) % g.interval() == 0;
                if due {
                    counts[i] += 1;
                }
            }
        }
        assert_eq!(counts, [60, 40, 10, 2]);
        // Memory's items appear 60 times in 600 s: its interval is 10 s.
        assert_eq!(store.samples_between("mem_available", 0, i64::MAX), 60);
    }

    #[test]
    fn the_hourly_pass_runs_once_an_hour() {
        let host = FakeHost::default();
        let config = Config::default();
        let mut store = Store::in_memory().unwrap();
        let mut s = Sampler::new(0);
        for now in (0..2 * 3600 + 1).step_by(10) {
            s.tick(now, &host, &config, &mut store).unwrap();
        }
        assert!(store.count("rollup_1m") > 0);
    }
}
