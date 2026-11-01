//! Standalone entry point for `ragon-delegate` when invoked directly rather than via `ragon delegate`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(ragon_delegate::cli::run(&args));
}
