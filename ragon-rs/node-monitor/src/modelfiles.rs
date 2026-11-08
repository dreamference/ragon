//! Model files (§3.6): a safetensors file's header length and its last tensor's end against the
//! file's size. A file shorter than its header says is `down` (a truncated download or a disk
//! that lost its tail). Reads 8 bytes and the header, never the tensors.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::{Value, json};

/// A header larger than this is garbage, not a header.
const MAX_HEADER: u64 = 256 << 20;

/// What a file needs to be whole: `8 + header length + the largest tensor end`, or why the header
/// cannot be read.
pub fn required_size(host: &dyn Host, path: &str) -> Result<(u64, u64), String> {
    let (head, size) = host.read_head(path, 8).map_err(|e| e.kind().to_string())?;
    let Ok(len_bytes) = <[u8; 8]>::try_from(head.as_slice()) else {
        return Ok((8, size)); // shorter than the length field itself
    };
    let n = u64::from_le_bytes(len_bytes);
    if n > MAX_HEADER {
        return Err("header length is not plausible".into());
    }
    if 8 + n > size {
        return Ok((8 + n, size));
    }
    let (bytes, _) = host
        .read_head(path, (8 + n) as usize)
        .map_err(|e| e.kind().to_string())?;
    let json: Value = serde_json::from_slice(&bytes[8..]).map_err(|_| "header is not JSON")?;
    let end = json
        .as_object()
        .ok_or("header is not an object")?
        .iter()
        .filter(|(k, _)| k.as_str() != "__metadata__")
        .filter_map(|(_, t)| t["data_offsets"][1].as_u64())
        .max()
        .unwrap_or(0);
    Ok((8 + n + end, size))
}

/// Checks the `.safetensors` files directly in `dir`; one item per directory.
pub fn check_dir(host: &dyn Host, dir: &str) -> Item {
    let name = format!("model_files:{}", dir.rsplit('/').next().unwrap_or(dir));
    let names = match host.list_dir(dir) {
        Ok(n) => n,
        Err(e) => {
            return Item::new(
                &name,
                State::Unknown,
                Weight::Item,
                format!("{dir}: {}", e.kind()),
            );
        }
    };
    let (mut checked, mut short, mut unreadable) = (0u32, Vec::new(), Vec::new());
    for file in names.iter().filter(|n| n.ends_with(".safetensors")) {
        match required_size(host, &format!("{dir}/{file}")) {
            Ok((need, have)) if have < need => short.push(format!("{file} ({have} < {need})")),
            Ok(_) => checked += 1,
            Err(why) => unreadable.push(format!("{file}: {why}")),
        }
    }
    let values = json!({ "checked": checked, "short": short, "unreadable": unreadable });
    let (state, detail) = if !short.is_empty() {
        (
            State::Down,
            format!("shorter than its header says: {}", short.join(", ")),
        )
    } else if !unreadable.is_empty() {
        (
            State::Unknown,
            format!("header unreadable: {}", unreadable.join(", ")),
        )
    } else if checked == 0 {
        (State::Unknown, "no safetensors files".to_string())
    } else {
        (State::Ok, format!("{checked} files whole"))
    };
    Item::new(&name, state, Weight::Item, detail).with_values(values)
}

/// Every snapshot of every model in the Hugging Face cache.
pub fn probe(host: &dyn Host) -> Vec<Item> {
    let Some(home) = host.home() else {
        return vec![Item::new(
            "model_files",
            State::Unknown,
            Weight::Item,
            "no HOME",
        )];
    };
    let hub = format!("{home}/.cache/huggingface/hub");
    let Ok(models) = host.list_dir(&hub) else {
        return vec![Item::new(
            "model_files",
            State::Unknown,
            Weight::Item,
            format!("{hub}: not readable"),
        )];
    };
    let mut items = Vec::new();
    for model in models.iter().filter(|m| m.starts_with("models--")) {
        let snaps = format!("{hub}/{model}/snapshots");
        for snap in host.list_dir(&snaps).unwrap_or_default() {
            items.push(check_dir(host, &format!("{snaps}/{snap}")));
        }
    }
    if items.is_empty() {
        items.push(Item::new(
            "model_files",
            State::Unknown,
            Weight::Item,
            "no model in the cache",
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    fn file(header: &str, data_len: u64) -> (Vec<u8>, u64) {
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend(header.as_bytes());
        let size = bytes.len() as u64 + data_len;
        (bytes, size)
    }

    const H: &str = r#"{"__metadata__":{"x":"y"},"a":{"dtype":"F16","shape":[2],"data_offsets":[0,4]},"b":{"dtype":"F16","shape":[4],"data_offsets":[4,12]}}"#;

    fn host(data_len: u64) -> FakeHost {
        let (bytes, size) = file(H, data_len);
        FakeHost::default()
            .head("/m/a.safetensors", bytes, size)
            .dir("/m", &["a.safetensors", "config.json"])
    }

    #[test]
    fn a_whole_file_is_ok() {
        assert_eq!(check_dir(&host(12), "/m").state, State::Ok);
    }

    #[test]
    fn a_file_shorter_than_its_header_says_is_down() {
        let item = check_dir(&host(11), "/m");
        assert_eq!(item.state, State::Down);
        assert!(item.detail.contains("a.safetensors"));
    }

    #[test]
    fn a_header_longer_than_the_file_is_down() {
        let mut bytes = 1000u64.to_le_bytes().to_vec();
        bytes.extend(b"{}");
        let host = FakeHost::default()
            .head("/m/a.safetensors", bytes, 10)
            .dir("/m", &["a.safetensors"]);
        assert_eq!(check_dir(&host, "/m").state, State::Down);
    }

    #[test]
    fn missing_sources_are_unknown() {
        assert_eq!(check_dir(&FakeHost::default(), "/m").state, State::Unknown);
        let empty = FakeHost::default().dir("/m", &["config.json"]);
        assert_eq!(check_dir(&empty, "/m").state, State::Unknown);
        let junk = FakeHost::default()
            .head("/m/a.safetensors", vec![0xff; 16], 16)
            .dir("/m", &["a.safetensors"]);
        assert_eq!(check_dir(&junk, "/m").state, State::Unknown);
        assert_eq!(probe(&FakeHost::default())[0].state, State::Unknown);
        let no_cache = FakeHost {
            home: Some("/h".into()),
            ..Default::default()
        };
        assert_eq!(probe(&no_cache)[0].state, State::Unknown);
    }

    #[test]
    fn the_cache_is_walked_model_by_snapshot() {
        let (bytes, size) = file(H, 12);
        let snap = "/h/.cache/huggingface/hub/models--o--m/snapshots/abc";
        let host = FakeHost {
            home: Some("/h".into()),
            ..Default::default()
        }
        .dir("/h/.cache/huggingface/hub", &["models--o--m", ".locks"])
        .dir("/h/.cache/huggingface/hub/models--o--m/snapshots", &["abc"])
        .dir(snap, &["w.safetensors"])
        .head(&format!("{snap}/w.safetensors"), bytes, size);
        let items = probe(&host);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, State::Ok);
    }
}
