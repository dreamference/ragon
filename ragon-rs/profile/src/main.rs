//! `ragon-profile …`: the same commands as `ragon profile …`, for a machine whose installed `ragon`
//! does not route them yet.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(ragon_profile::cli::run(&args));
}
