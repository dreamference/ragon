//! `ragon-monitor …`: the same commands as `ragon monitor …`, for a machine whose installed `ragon`
//! predates the monitor. The timer's units name whichever of the two installed them.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "ragon-monitor".to_string());
    std::process::exit(ragon_monitor::cli::run(&args, &[exe]));
}
