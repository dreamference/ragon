use ragon_node_monitor::{Config, collect, host::RealHost, status_json, status_text};
use std::path::PathBuf;

const USAGE: &str =
    "usage: ragon-node-monitor status [--json] [--port N] [--engine-port N] [--root DIR]";

fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

fn run(args: Vec<String>) -> i32 {
    if args.first().map(String::as_str) != Some("status") {
        eprintln!("{USAGE}");
        return 2;
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

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}
