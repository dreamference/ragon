use ragon_node_monitor::{
    Config, collect, host::RealHost, sampler::Sampler, status_json, status_text, store::Store,
};
use std::path::PathBuf;

const USAGE: &str = "usage: ragon-node-monitor status [--json] [--port N] [--engine-port N] [--root DIR]
       ragon-node-monitor history [--db FILE] [--hours N]
       ragon-node-monitor history [--window 30m|6h|2d] [--item NAME] [--db FILE]
       ragon-node-monitor run [--db FILE] [--port N] [--engine-port N] [--root DIR]";

fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

fn run(args: Vec<String>) -> i32 {
    match args.first().map(String::as_str) {
        Some("status") => {}
        Some("run") => return run_sampler(args),
        Some("history") if args.iter().any(|a| a == "--window" || a == "--item") => {
            return run_history(args);
        }
        Some("history") => return history(args),
        _ => return usage(),
    }
    let mut config = Config::default();
    let mut engine_given = false;
    let mut json = false;
    let mut root = PathBuf::from("/");
    let mut rest = args.into_iter().skip(1);
    while let Some(arg) = rest.next() {
        let number = |value: Option<String>| value.and_then(|v| v.parse::<u16>().ok());
        match arg.as_str() {
            "--json" => json = true,
            "--port" => match number(rest.next()) {
                Some(port) => config.public_port = port,
                None => return usage(),
            },
            "--engine-port" => match number(rest.next()) {
                Some(port) => {
                    config.engine_port = port;
                    engine_given = true;
                }
                None => return usage(),
            },
            "--root" => match rest.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    if !engine_given {
        config.engine_port = Config::engine_port_for(config.public_port);
    }
    let items = collect(&RealHost { root }, &config);
    if json {
        println!("{}", status_json(&items));
    } else {
        print!("{}", status_text(&items));
    }
    0
}

/// `history`: the last hours of the store as JSON strip-chart series, at most 120 points each.
/// No store yet is an empty list, not an error.
fn history(args: Vec<String>) -> i32 {
    let mut db: Option<PathBuf> = None;
    let mut hours: i64 = 6;
    let mut rest = args.into_iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--db" => match rest.next() {
                Some(file) => db = Some(PathBuf::from(file)),
                None => return usage(),
            },
            "--hours" => match rest.next().and_then(|v| v.parse::<i64>().ok()) {
                Some(n) if (1..=48).contains(&n) => hours = n,
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    let mut series = Vec::new();
    if let Some(db) = db.or_else(default_db).filter(|p| p.exists())
        && let Ok(store) = Store::open(&db)
    {
        let to = now() + 1;
        for (name, points) in store.series(to - hours * 3600, to, hours * 30) {
            series.push(serde_json::json!({ "name": name, "points": points }));
        }
    }
    println!("{}", serde_json::json!({ "series": series }));
    0
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

/// `run`: samples at the intervals of section 3 into the store until killed.
fn run_sampler(args: Vec<String>) -> i32 {
    let mut config = Config::default();
    let mut engine_given = false;
    let mut root = PathBuf::from("/");
    let mut db: Option<PathBuf> = None;
    let mut rest = args.into_iter().skip(1);
    while let Some(arg) = rest.next() {
        let number = |value: Option<String>| value.and_then(|v| v.parse::<u16>().ok());
        match arg.as_str() {
            "--port" => match number(rest.next()) {
                Some(port) => config.public_port = port,
                None => return usage(),
            },
            "--engine-port" => match number(rest.next()) {
                Some(port) => {
                    config.engine_port = port;
                    engine_given = true;
                }
                None => return usage(),
            },
            "--root" => match rest.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return usage(),
            },
            "--db" => match rest.next() {
                Some(file) => db = Some(PathBuf::from(file)),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    if !engine_given {
        config.engine_port = Config::engine_port_for(config.public_port);
    }
    let Some(db) = db.or_else(default_db) else {
        eprintln!("no --db and no home directory");
        return 2;
    };
    if let Some(dir) = db.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut store = match Store::open(&db) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("cannot open {}: {e}", db.display());
            return 1;
        }
    };
    let host = RealHost { root };
    let mut sampler = Sampler::new(now());
    loop {
        if let Err(e) = sampler.tick(now(), &host, &config, &mut store) {
            eprintln!("sample not stored: {e}");
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// `history`: prints the stored series of a window, oldest first.
fn run_history(args: Vec<String>) -> i32 {
    let mut window = 3600;
    let mut item: Option<String> = None;
    let mut db: Option<PathBuf> = None;
    let mut rest = args.into_iter().skip(1);
    while let Some(arg) = rest.next() {
        match (arg.as_str(), rest.next()) {
            ("--window", Some(v)) => match parse_window(&v) {
                Some(secs) => window = secs,
                None => return usage(),
            },
            ("--item", Some(v)) => item = Some(v),
            ("--db", Some(v)) => db = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let Some(db) = db.or_else(default_db) else {
        eprintln!("no --db and no home directory");
        return 2;
    };
    if !db.is_file() {
        eprintln!("no history at {}", db.display());
        return 1;
    }
    let store = match Store::open(&db) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("cannot open {}: {e}", db.display());
            return 1;
        }
    };
    let end = now();
    match store.stored_series(item.as_deref(), end - window, end + 1, end) {
        Ok(rows) => {
            for (ts, name, state) in rows {
                println!("{ts} {name} {state}");
            }
            0
        }
        Err(e) => {
            eprintln!("cannot read history: {e}");
            1
        }
    }
}

/// `30m`, `6h`, `2d` or plain seconds.
fn parse_window(text: &str) -> Option<i64> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, c @ ('m' | 'h' | 'd' | 's')) => (&text[..i], c),
        _ => (text, 's'),
    };
    let n: i64 = digits.parse().ok().filter(|n| *n > 0)?;
    Some(
        n * match unit {
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => 1,
        },
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `~/.local/share/dreamference/monitor/node/monitor.sqlite` (section 5.2).
fn default_db() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/share/dreamference/monitor/node/monitor.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::parse_window;

    #[test]
    fn windows_parse_with_units() {
        assert_eq!(parse_window("30m"), Some(1800));
        assert_eq!(parse_window("2d"), Some(172800));
        assert_eq!(parse_window("90"), Some(90));
        assert_eq!(parse_window("0h"), None);
        assert_eq!(parse_window("x"), None);
    }
}
