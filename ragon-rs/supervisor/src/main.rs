//! `ragon-supervisor [server] <start|stop|status|run|install|uninstall> ...` and, on Unix,
//! `ragon-supervisor jobs ...` (src/jobs.rs): the supervisor without
//! the launcher, for the drills and for nodes that do not install the launcher. The launcher runs
//! the same code as `ragon server ...` on Windows.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(unix)]
    if args.first().map(String::as_str) == Some("jobs") {
        std::process::exit(ragon_supervisor::jobs::cli(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("server") {
        args.remove(0);
    }
    std::process::exit(ragon_supervisor::cli::run(&args));
}
