//! The Mac node's power settings (specs/DREAMFERENCE_MAC_NODE.md §4.2): on mains no system sleep,
//! start after a power failure, wake for network access (`pmset -c sleep 0 autorestart 1 womp 1`);
//! display sleep is left alone. Set once at install, restored at uninstall from the values saved
//! before the change. On battery (a laptop, MN6) the supervisor does not fight sleep: it reports
//! `state: battery` instead of `healthy` (the node no longer advertises ready) and closes admission
//! for new sessions below [`MIN_CHARGE_PCT`]; `server status` says `power: battery`.
//!
//! Everything here is pure (argument lists and parsers) and is tested on every platform; running
//! `pmset` (root for a change, any user for a reading) is in src/cli.rs, macOS only.

/// The settings the node changes on mains, in `pmset` order.
pub const NODE_SETTINGS: [(&str, &str); 3] = [("sleep", "0"), ("autorestart", "1"), ("womp", "1")];

/// The file in the state directory that keeps the values from before the install.
pub const SAVED_FILE: &str = "pmset-before.txt";

/// `pmset` arguments that apply the node's settings to the charger (mains) profile.
pub fn set_args() -> Vec<String> {
    let mut args = vec!["-c".to_string()];
    for (key, value) in NODE_SETTINGS {
        args.push(key.to_string());
        args.push(value.to_string());
    }
    args
}

/// The `AC Power:` section of `pmset -g custom`, restricted to the keys in [`NODE_SETTINGS`], in
/// that order. A key the machine does not list (a laptop without `autorestart`) is absent.
pub fn parse_custom_ac(text: &str) -> Vec<(String, String)> {
    let mut in_ac = false;
    let mut found: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            in_ac = line.trim_end().trim_end_matches(':') == "AC Power";
            continue;
        }
        if !in_ac {
            continue;
        }
        // `Sleep On Power Button 1` has spaces in its name: the value is the last word.
        let Some((key, value)) = line.trim().rsplit_once(char::is_whitespace) else { continue };
        let key = key.trim();
        if NODE_SETTINGS.iter().any(|(k, _)| *k == key) {
            found.push((key.to_string(), value.to_string()));
        }
    }
    NODE_SETTINGS
        .iter()
        .filter_map(|(k, _)| found.iter().find(|(f, _)| f == k).cloned())
        .collect()
}

/// The saved file's text: one `key value` line per setting.
pub fn saved_text(values: &[(String, String)]) -> String {
    values.iter().map(|(k, v)| format!("{k} {v}\n")).collect()
}

/// Reads [`saved_text`] back; lines for keys the node does not manage, or with a value that is not
/// a number, are dropped, so a damaged file never feeds `pmset` anything else.
pub fn parse_saved(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, v)| {
            NODE_SETTINGS.iter().any(|(n, _)| n == k) && !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())
        })
        .collect()
}

/// `pmset` arguments that put the saved values back, or `None` when nothing was saved.
pub fn restore_args(saved: &[(String, String)]) -> Option<Vec<String>> {
    if saved.is_empty() {
        return None;
    }
    let mut args = vec!["-c".to_string()];
    for (key, value) in saved {
        args.push(key.clone());
        args.push(value.clone());
    }
    Some(args)
}

/// Where the Mac draws power from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSource {
    Mains,
    Battery,
    Unknown,
}

impl PowerSource {
    /// The value of the `power:` line of `server status`.
    pub fn as_str(self) -> &'static str {
        match self {
            PowerSource::Mains => "mains",
            PowerSource::Battery => "battery",
            PowerSource::Unknown => "unknown",
        }
    }
}

/// `pmset -g batt`'s first line: `Now drawing from 'AC Power'` (or `'Battery Power'`, `'UPS Power'`).
pub fn parse_batt(text: &str) -> PowerSource {
    let Some(line) = text.lines().find(|l| l.contains("drawing from")) else { return PowerSource::Unknown };
    if line.contains("'AC Power'") {
        PowerSource::Mains
    } else if line.contains("'Battery Power'") || line.contains("'UPS Power'") {
        PowerSource::Battery
    } else {
        PowerSource::Unknown
    }
}

/// Below this charge (percent) a node on battery admits no new sessions [E: spec §4.2].
pub const MIN_CHARGE_PCT: u8 = 20;
/// How often the supervisor reads `pmset -g batt` (any user may).
pub const POLL_S: u64 = 60;

/// The internal battery's charge from `pmset -g batt`'s second line
/// (` -InternalBattery-0 (id=1)\t81%; discharging; ...`); `None` on a desktop or an unreadable line.
pub fn parse_charge(text: &str) -> Option<u8> {
    let line = text.lines().find(|l| l.contains("InternalBattery"))?;
    line.split(|c: char| c.is_whitespace() || c == ';')
        .find_map(|word| word.strip_suffix('%')?.parse::<u8>().ok())
        .filter(|pct| *pct <= 100)
}

/// The node on battery: its charge, when the Mac says, and whether it admits new sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub charge: Option<u8>,
    pub admitting: bool,
}

/// `None` on mains or when the source is unknown (nothing changes); on battery or UPS power the
/// charge and admission. An unknown charge (a desktop on a UPS) never closes admission.
pub fn battery(batt: &str, min_charge: u8) -> Option<Battery> {
    if parse_batt(batt) != PowerSource::Battery {
        return None;
    }
    let charge = parse_charge(batt);
    Some(Battery { charge, admitting: charge.is_none_or(|pct| pct >= min_charge) })
}

/// The status file on battery: `state: healthy` becomes `state: battery` (the engine still serves,
/// the node no longer says ready); `charge` and `admission` follow the shared keys. Any other state
/// (starting, backoff, refused) is left as it is: it says more than the power source.
pub fn status_on_battery(status: &str, battery: &Battery) -> String {
    let mut out: String = status
        .lines()
        .map(|line| if line == "state: healthy" { "state: battery".to_string() } else { line.to_string() })
        .map(|line| line + "\n")
        .collect();
    out.push_str(&format!("charge: {}\n", battery.charge.map(|c| c.to_string()).unwrap_or_else(|| "none".into())));
    out.push_str(&format!("admission: {}\n", if battery.admitting { "open" } else { "closed" }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // `pmset -g custom` on a Mac mini (macOS 27), verbatim.
    const MINI: &str = "AC Power:\n Sleep On Power Button 1\n autorestartatconnect 0\n powermode            0\n \
standby              0\n ttyskeepawake        1\n powernap             1\n displaysleep         10\n \
womp                 1\n networkoversleep     0\n sleep                0\n tcpkeepalive         1\n \
autorestart          1\n disksleep            0\n";

    // A laptop: two sections, the battery one first, no `autorestart`.
    const LAPTOP: &str = "Battery Power:\n lowpowermode         0\n sleep                1\n womp                 0\n \
displaysleep         2\nAC Power:\n Sleep On Power Button 1\n sleep                10\n womp                 0\n \
displaysleep         10\n";

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_set_arguments_touch_mains_only_and_leave_display_sleep() {
        assert_eq!(set_args(), ["-c", "sleep", "0", "autorestart", "1", "womp", "1"]);
        assert!(!set_args().iter().any(|a| a == "displaysleep" || a == "-b" || a == "-a"));
    }

    #[test]
    fn the_ac_section_is_read_in_the_node_order() {
        assert_eq!(parse_custom_ac(MINI), pairs(&[("sleep", "0"), ("autorestart", "1"), ("womp", "1")]));
        assert_eq!(parse_custom_ac(LAPTOP), pairs(&[("sleep", "10"), ("womp", "0")]));
        assert_eq!(parse_custom_ac("Battery Power:\n sleep 1\n"), Vec::new());
        assert_eq!(parse_custom_ac(""), Vec::new());
    }

    #[test]
    fn saved_values_round_trip_to_the_restore_arguments() {
        let before = parse_custom_ac(LAPTOP);
        let text = saved_text(&before);
        assert_eq!(text, "sleep 10\nwomp 0\n");
        assert_eq!(parse_saved(&text), before);
        assert_eq!(restore_args(&parse_saved(&text)).unwrap(), ["-c", "sleep", "10", "womp", "0"]);
        assert_eq!(restore_args(&[]), None);
    }

    #[test]
    fn a_damaged_saved_file_feeds_pmset_nothing_else() {
        let saved = parse_saved("sleep 5\nhibernatemode 25\nwomp ; rm -rf /\nautorestart\ngarbage\n");
        assert_eq!(saved, pairs(&[("sleep", "5")]));
    }

    #[test]
    fn the_power_source() {
        assert_eq!(parse_batt("Now drawing from 'AC Power'\n"), PowerSource::Mains);
        assert_eq!(
            parse_batt("Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t81%; discharging;\n"),
            PowerSource::Battery
        );
        assert_eq!(parse_batt("Now drawing from 'UPS Power'\n"), PowerSource::Battery);
        assert_eq!(parse_batt(""), PowerSource::Unknown);
        assert_eq!(PowerSource::Battery.as_str(), "battery");
    }

    const ON_BATTERY: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=21823587)\t81%; discharging; 4:02 remaining present: true\n";
    const LOW: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=21823587)\t12%; discharging; 0:41 remaining present: true\n";
    const CHARGING: &str = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=21823587)\t9%; charging; 2:10 remaining present: true\n";
    const UPS: &str = "Now drawing from 'UPS Power'\n";

    #[test]
    fn the_charge_is_read_from_the_internal_battery_line() {
        assert_eq!(parse_charge(ON_BATTERY), Some(81));
        assert_eq!(parse_charge(LOW), Some(12));
        assert_eq!(parse_charge(UPS), None, "a desktop has no internal battery");
        assert_eq!(parse_charge(" -InternalBattery-0 (id=1)\t250%; x\n"), None);
        assert_eq!(parse_charge(""), None);
    }

    #[test]
    fn admission_closes_only_on_battery_below_the_threshold() {
        assert_eq!(battery(ON_BATTERY, MIN_CHARGE_PCT), Some(Battery { charge: Some(81), admitting: true }));
        assert_eq!(battery(LOW, MIN_CHARGE_PCT), Some(Battery { charge: Some(12), admitting: false }));
        assert_eq!(battery(CHARGING, MIN_CHARGE_PCT), None, "on mains a low charge changes nothing");
        assert_eq!(battery(UPS, MIN_CHARGE_PCT), Some(Battery { charge: None, admitting: true }));
        assert_eq!(battery("", MIN_CHARGE_PCT), None, "an unknown source changes nothing");
        assert_eq!(battery(LOW, 12).map(|b| b.admitting), Some(true), "the threshold itself admits");
    }

    #[test]
    fn the_status_on_battery() {
        let status = "name: ragon-node\nstate: healthy\nengine_pid: 4\n";
        let low = Battery { charge: Some(12), admitting: false };
        assert_eq!(
            status_on_battery(status, &low),
            "name: ragon-node\nstate: battery\nengine_pid: 4\ncharge: 12\nadmission: closed\n"
        );
        let ups = Battery { charge: None, admitting: true };
        let starting = "name: ragon-node\nstate: starting\n";
        assert_eq!(status_on_battery(starting, &ups), "name: ragon-node\nstate: starting\ncharge: none\nadmission: open\n");
    }
}
