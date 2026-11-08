//! The node watcher (specs/DREAMFERENCE_MONITORING.md §3, §4, §11). `collect` runs every probe once;
//! `tracker` adds the hysteresis of §4.2 over a series of samples and `liveness` the §3.3 decision
//! (O3), both on a clock the caller supplies. `status --json` is still one sample, no history.

pub mod disk;
pub mod gate;
pub mod host;
pub mod liveness;
pub mod memory;
pub mod sampler;
pub mod server;
pub mod store;
pub mod tracker;
pub mod units;

#[cfg(test)]
mod fake;

use host::Host;
use serde_json::{Value, json};

/// An item's state (§4.1). The order is the order of badness; `Off` and `Unknown` are outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ok,
    Degraded,
    Down,
    /// The feature is not turned on; never an error.
    Off,
    /// The probe itself could not run.
    Unknown,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::Degraded => "degraded",
            State::Down => "down",
            State::Off => "off",
            State::Unknown => "unknown",
        }
    }
    fn rank(self) -> u8 {
        match self {
            State::Down => 2,
            State::Degraded => 1,
            _ => 0,
        }
    }
}

/// Whether an item's `down` may make the node `down` (§4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    /// The model server, memory, and the disk holding the model cache or `~`.
    Node,
    /// Any other item: its `down` makes the node `degraded`.
    Item,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub state: State,
    pub weight: Weight,
    /// One line for a person.
    pub detail: String,
    /// The sampled numbers, for the status page.
    pub values: Value,
}

impl Item {
    pub fn new(name: &str, state: State, weight: Weight, detail: impl Into<String>) -> Self {
        Item {
            name: name.into(),
            state,
            weight,
            detail: detail.into(),
            values: Value::Null,
        }
    }
    pub fn with_values(mut self, values: Value) -> Self {
        self.values = values;
        self
    }
}

/// The node's state (§4.3): the worst item, except that only `Weight::Node` items reach `down`.
/// `off` and `unknown` do not count (an `unknown` counts as `degraded` only after an hour, §4.1,
/// which needs history).
pub fn node_state(items: &[Item]) -> State {
    let mut worst = 0;
    for item in items {
        let rank = match (item.state, item.weight) {
            (State::Down, Weight::Item) => 1,
            (state, _) => state.rank(),
        };
        worst = worst.max(rank);
    }
    match worst {
        2 => State::Down,
        1 => State::Degraded,
        _ => State::Ok,
    }
}

/// Where to look; the defaults are the product's (public 8000, engine 18000).
#[derive(Clone, Debug)]
pub struct Config {
    pub public_port: u16,
    pub engine_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            public_port: 8000,
            engine_port: 18000,
        }
    }
}

impl Config {
    /// The engine's loopback port behind a public one (`ModelGate.engine_port`): +10000, or -10000
    /// where that passes 65535.
    pub fn engine_port_for(public: u16) -> u16 {
        public.checked_add(10000).unwrap_or_else(|| public - 10000)
    }
}

/// Runs every probe once.
pub fn collect(host: &dyn Host, config: &Config) -> Vec<Item> {
    let mut items = server::probe(host, config);
    items.extend(memory::probe(host));
    items.extend(disk::probe(host));
    items.extend(units::probe(host));
    items
}

/// The `status --json` document.
pub fn status_json(items: &[Item]) -> Value {
    json!({
        "node": { "state": node_state(items).as_str() },
        "items": items.iter().map(|item| {
            let mut entry = json!({
                "name": item.name,
                "state": item.state.as_str(),
                "detail": item.detail,
            });
            if !item.values.is_null() {
                entry["values"] = item.values.clone();
            }
            entry
        }).collect::<Vec<_>>(),
    })
}

/// The same, as lines for a terminal.
pub fn status_text(items: &[Item]) -> String {
    let mut out = format!("node: {}\n", node_state(items).as_str());
    for item in items {
        out.push_str(&format!(
            "{:<10} {:<28} {}\n",
            item.state.as_str(),
            item.name,
            item.detail
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(state: State, weight: Weight) -> Item {
        Item::new("x", state, weight, "")
    }

    #[test]
    fn only_node_items_make_the_node_down() {
        assert_eq!(node_state(&[item(State::Down, Weight::Node)]), State::Down);
        assert_eq!(
            node_state(&[item(State::Down, Weight::Item)]),
            State::Degraded
        );
        assert_eq!(
            node_state(&[item(State::Degraded, Weight::Node)]),
            State::Degraded
        );
    }

    #[test]
    fn the_engine_port_follows_the_gate_rule() {
        assert_eq!(Config::engine_port_for(8000), 18000);
        assert_eq!(Config::engine_port_for(60000), 50000);
    }

    #[test]
    fn off_and_unknown_do_not_count() {
        let items = [
            item(State::Off, Weight::Item),
            item(State::Unknown, Weight::Node),
        ];
        assert_eq!(node_state(&items), State::Ok);
        assert_eq!(node_state(&[]), State::Ok);
    }

    #[test]
    fn the_document_names_the_node_and_every_item() {
        let items = [Item::new("a", State::Ok, Weight::Item, "fine").with_values(json!({"n": 1}))];
        let doc = status_json(&items);
        assert_eq!(doc["node"]["state"], "ok");
        assert_eq!(doc["items"][0]["name"], "a");
        assert_eq!(doc["items"][0]["values"]["n"], 1);
        assert!(status_text(&items).contains("fine"));
    }
}
