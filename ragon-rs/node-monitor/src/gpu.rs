//! GPU errors (§3.7): the Xid records the driver writes to the kernel journal, read from its
//! structured fields. One table maps an Xid to what the driver vendor recommends; an Xid not in the
//! table is `degraded`. The monitor never resets the GPU or reboots for one: the item names the
//! recommendation. Without the journal group the item is `unknown` with that reason.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::{Value, json};
use std::time::Duration;

/// What the vendor's catalog recommends for an Xid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    RestartApplication,
    ResetOrReboot,
}

/// The one table: Xid and its recommended action.
const CATALOG: &[(u32, Action)] = &[
    (13, Action::RestartApplication),
    (31, Action::RestartApplication),
    (43, Action::RestartApplication),
    (45, Action::RestartApplication),
    (48, Action::ResetOrReboot),
    (62, Action::ResetOrReboot),
    (63, Action::ResetOrReboot),
    (64, Action::ResetOrReboot),
    (74, Action::ResetOrReboot),
    (79, Action::ResetOrReboot),
    (95, Action::ResetOrReboot),
    (119, Action::ResetOrReboot),
    (120, Action::ResetOrReboot),
    (140, Action::ResetOrReboot),
];

pub fn action_for(xid: u32) -> Option<Action> {
    CATALOG.iter().find(|(x, _)| *x == xid).map(|(_, a)| *a)
}

/// The Xid number in a kernel message such as `NVRM: Xid (PCI:0000:0f:00): 79, pid=…`.
pub fn parse_xid(message: &str) -> Option<u32> {
    let rest = &message[message.find("NVRM: Xid")? + "NVRM: Xid".len()..];
    let rest = &rest[rest.find("): ")? + 3..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Every Xid in `journalctl -o json` lines (one JSON object per line).
pub fn xids_in(journal: &str) -> Vec<u32> {
    journal
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| parse_xid(v["MESSAGE"].as_str()?))
        .collect()
}

/// Whether this process may read the kernel journal: root, or in `adm` or `systemd-journal`.
fn may_read_journal(host: &dyn Host) -> bool {
    let Ok(status) = host.read_file("/proc/self/status") else {
        return false;
    };
    let field = |key: &str| -> Vec<u32> {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| {
                v.split_whitespace()
                    .filter_map(|w| w.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    if field("Uid:").first() == Some(&0) {
        return true;
    }
    let groups = field("Groups:");
    let Ok(etc_group) = host.read_file("/etc/group") else {
        return false;
    };
    etc_group.lines().any(|l| {
        let mut p = l.split(':');
        let (name, gid) = (p.next(), p.nth(1).and_then(|g| g.parse::<u32>().ok()));
        matches!(name, Some("adm" | "systemd-journal")) && gid.is_some_and(|g| groups.contains(&g))
    })
}

pub fn probe(host: &dyn Host) -> Vec<Item> {
    vec![xid_item(host)]
}

fn xid_item(host: &dyn Host) -> Item {
    let name = "gpu_errors";
    if !may_read_journal(host) {
        return Item::new(
            name,
            State::Unknown,
            Weight::Item,
            "the user is not in the adm or systemd-journal group, so the kernel journal is unreadable",
        );
    }
    let out = host.run(
        "journalctl",
        &["-k", "-b", "-o", "json", "--no-pager", "-n", "5000"],
        Duration::from_secs(10),
    );
    let out = match out {
        Ok(o) if o.success => o.stdout,
        Ok(_) => return Item::new(name, State::Unknown, Weight::Item, "journalctl failed"),
        Err(e) => {
            return Item::new(
                name,
                State::Unknown,
                Weight::Item,
                format!("journalctl: {}", e.kind()),
            );
        }
    };
    let xids = xids_in(&out);
    let reset: Vec<u32> = xids
        .iter()
        .copied()
        .filter(|x| action_for(*x) == Some(Action::ResetOrReboot))
        .collect();
    let values = json!({ "xids": xids });
    if let Some(x) = reset.last() {
        return Item::new(
            name,
            State::Down,
            Weight::Item,
            format!("Xid {x}: the GPU needs a reset or a reboot"),
        )
        .with_values(values);
    }
    if let Some(x) = xids.last() {
        let why = match action_for(*x) {
            Some(_) => "restart the application",
            None => "not in the catalog",
        };
        return Item::new(
            name,
            State::Degraded,
            Weight::Item,
            format!("Xid {x}: {why}"),
        )
        .with_values(values);
    }
    Item::new(name, State::Ok, Weight::Item, "no Xid since boot").with_values(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    const CMD: &str = "journalctl -k -b -o json --no-pager -n 5000";

    fn line(xid: u32) -> String {
        format!(
            r#"{{"MESSAGE":"NVRM: Xid (PCI:0000:0f:00): {xid}, pid=1, name=x, GPU has fallen off the bus."}}"#
        )
    }

    fn host(journal: &str) -> FakeHost {
        FakeHost::default()
            .file("/proc/self/status", "Uid:\t1000\t1000\nGroups:\t4 1000\n")
            .file("/etc/group", "adm:x:4:me\nusers:x:100:\n")
            .command(CMD, journal)
    }

    #[test]
    fn parses_the_xid_number() {
        assert_eq!(parse_xid("NVRM: Xid (PCI:0000:0f:00): 79, pid=1"), Some(79));
        assert_eq!(parse_xid("usb 1-1: new device"), None);
        assert_eq!(parse_xid("NVRM: Xid (PCI:0): oops"), None);
    }

    #[test]
    fn classes_follow_the_catalog() {
        let state = |j: &str| xid_item(&host(j)).state;
        assert_eq!(state(""), State::Ok);
        assert_eq!(state(&line(13)), State::Degraded);
        assert_eq!(state(&line(9999)), State::Degraded);
        assert_eq!(state(&format!("{}\n{}", line(13), line(79))), State::Down);
        let down = xid_item(&host(&line(79)));
        assert!(down.detail.contains("reset or a reboot"));
    }

    #[test]
    fn no_journal_group_or_no_journalctl_is_unknown() {
        let no_group = FakeHost::default()
            .file("/proc/self/status", "Uid:\t1000\nGroups:\t1000\n")
            .file("/etc/group", "adm:x:4:me\n")
            .command(CMD, "");
        assert_eq!(xid_item(&no_group).state, State::Unknown);
        assert_eq!(xid_item(&FakeHost::default()).state, State::Unknown);
        let no_cmd = FakeHost::default()
            .file("/proc/self/status", "Uid:\t0\nGroups:\t0\n")
            .file("/etc/group", "");
        assert_eq!(xid_item(&no_cmd).state, State::Unknown);
    }
}
