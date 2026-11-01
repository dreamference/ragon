//! `ragon-signal`, the small binary the bridge's system account runs from /usr/local/lib/ragon
//! (specs/DREAMFERENCE_RAGON_SIGNAL.md §2, §6). The commands are `cli.rs`'s, the same ones
//! `ragon signal …` runs; this binary also accepts the bridge's own (`serve`, `init`, …).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = ragon_signal::cli::Invocation::new(std::env::current_exe().ok(), true);
    std::process::exit(ragon_signal::cli::run(&args, &invocation));
}
