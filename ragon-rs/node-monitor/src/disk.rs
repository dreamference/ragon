//! Disk (§3.6): `statvfs` on each filesystem holding `/`, Docker's data root, the model cache,
//! `~/.local/share/dreamference` and `~/.ragon`, each filesystem counted once. "Free" is `f_bavail`.
//! Not in Phase 1: growth, NVMe health, the model files' headers.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::json;

const GIB: u64 = 1 << 30;

/// A watched path and whether its filesystem may make the node `down` (the model cache and `~`).
fn watched(home: &str) -> Vec<(String, bool)> {
    vec![
        ("/".into(), false),
        ("/var/lib/docker".into(), false),
        (format!("{home}/.cache/huggingface"), true),
        (format!("{home}/.local/share/dreamference"), true),
        (format!("{home}/.ragon"), false),
        (home.to_string(), true),
    ]
}

struct Volume {
    fs_id: u64,
    paths: Vec<String>,
    total: u64,
    avail: u64,
    holds_node: bool,
}

pub fn probe(host: &dyn Host) -> Vec<Item> {
    let Some(home) = host.home() else {
        return vec![Item::new(
            "disk",
            State::Unknown,
            Weight::Node,
            "no home directory",
        )];
    };
    let mut volumes: Vec<Volume> = Vec::new();
    let mut items = Vec::new();
    for (path, holds_node) in watched(&home) {
        match host.statvfs(&path) {
            Ok(stat) => match volumes.iter_mut().find(|v| v.fs_id == stat.fs_id) {
                Some(v) => {
                    v.paths.push(path);
                    v.holds_node |= holds_node;
                }
                None => volumes.push(Volume {
                    fs_id: stat.fs_id,
                    paths: vec![path],
                    total: stat.total,
                    avail: stat.avail,
                    holds_node,
                }),
            },
            // A path that is not there (no Docker, no model cache yet) has nothing to watch.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => items.push(Item::new(
                &format!("disk:{path}"),
                State::Unknown,
                if holds_node {
                    Weight::Node
                } else {
                    Weight::Item
                },
                format!("statvfs: {}", e.kind()),
            )),
        }
    }
    items.extend(volumes.into_iter().map(volume_item));
    items
}

fn volume_item(v: Volume) -> Item {
    let percent = if v.total == 0 {
        0.0
    } else {
        v.avail as f64 * 100.0 / v.total as f64
    };
    let state = if v.avail < 15 * GIB {
        State::Down
    } else if v.avail < 50 * GIB || percent < 10.0 {
        State::Degraded
    } else {
        State::Ok
    };
    let free_gib = v.avail / GIB;
    let detail = format!(
        "{free_gib} GiB free ({percent:.0}%) under {}",
        v.paths.join(", ")
    );
    Item::new(
        &format!("disk:{}", v.paths[0]),
        state,
        if v.holds_node {
            Weight::Node
        } else {
            Weight::Item
        },
        detail,
    )
    .with_values(json!({ "avail_bytes": v.avail, "total_bytes": v.total, "paths": v.paths }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    fn host() -> FakeHost {
        FakeHost {
            home: Some("/h".into()),
            ..Default::default()
        }
    }

    #[test]
    fn paths_on_one_filesystem_are_counted_once() {
        let h = host()
            .volume("/", 1, 900, 400)
            .volume("/h", 1, 900, 400)
            .volume("/h/.ragon", 1, 900, 400);
        let items = probe(&h);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "disk:/");
        assert_eq!(items[0].state, State::Ok);
        assert_eq!(items[0].weight, Weight::Node, "`~` is on it");
    }

    #[test]
    fn thresholds_are_50_gib_or_10_percent_and_15_gib() {
        let state = |total, avail| probe(&host().volume("/", 1, total, avail))[0].state;
        assert_eq!(state(400, 60), State::Ok);
        assert_eq!(state(900, 60), State::Degraded, "under 10 % free");
        assert_eq!(state(900, 49), State::Degraded);
        assert_eq!(state(100, 60), State::Ok);
        assert_eq!(state(1000, 90), State::Degraded, "under 10 % free");
        assert_eq!(state(900, 15), State::Degraded);
        assert_eq!(state(900, 14), State::Down);
    }

    #[test]
    fn a_missing_path_is_skipped() {
        assert!(probe(&host()).is_empty());
        let mut h = host().volume("/", 1, 900, 400);
        h.fs.clear();
        assert!(probe(&h).is_empty());
    }

    #[test]
    fn a_filesystem_without_the_model_cache_or_home_cannot_make_the_node_down() {
        let h = host().volume("/", 1, 900, 5).volume("/h", 2, 900, 800);
        let items = probe(&h);
        let root = items.iter().find(|i| i.name == "disk:/").unwrap();
        assert_eq!((root.state, root.weight), (State::Down, Weight::Item));
        assert_eq!(crate::node_state(&items), State::Degraded);
    }

    #[test]
    fn no_home_is_unknown() {
        let h = FakeHost::default();
        assert_eq!(probe(&h)[0].state, State::Unknown);
    }
}
