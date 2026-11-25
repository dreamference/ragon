//! A stand-in for a job's command and for a job process gone wrong, for the job drills
//! (tests/job_drills.rs). Arguments after the mode's own are ignored, so it can also stand where
//! the runner starts `server job exec ... <id>`.
//!
//! - `sleep MS`: sleeps, then exits 0;
//! - `print TEXT`: prints TEXT and exits 0;
//! - `write FILE TEXT`: writes FILE in the current folder (a change for the job's commit);
//! - `exit N`: exits with N;
//! - `alloc MB`: allocates and touches MB mebibytes 16 at a time, then holds them; an allocation
//!   the system refuses (a Job Object's cap) is reported and the process holds what it has;
//! - `hang`: sleeps for an hour and ignores the stop file (a job process that does not stop).

use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let number = |i: usize| arg(i).parse::<u64>().unwrap_or_else(|_| panic!("argument {i} needs a number"));
    match arg(0).as_str() {
        "sleep" => std::thread::sleep(Duration::from_millis(number(1))),
        "print" => println!("{}", arg(1)),
        "write" => std::fs::write(arg(1), arg(2)).expect("cannot write the file"),
        "exit" => std::process::exit(number(1) as i32),
        "alloc" => alloc(number(1)),
        "hang" => std::thread::sleep(Duration::from_secs(3600)),
        other => {
            eprintln!("unknown mode {other:?}");
            std::process::exit(2);
        }
    }
}

fn alloc(mebibytes: u64) {
    const CHUNK: usize = 16 << 20;
    let mut held: Vec<Vec<u8>> = Vec::new();
    let mut total = 0u64;
    while total < mebibytes << 20 {
        let mut chunk: Vec<u8> = Vec::new();
        if chunk.try_reserve_exact(CHUNK).is_err() {
            eprintln!("allocation refused after {} MiB", total >> 20);
            break;
        }
        chunk.resize(CHUNK, 1);
        // Touch every page so the memory is committed and resident.
        for i in (0..CHUNK).step_by(4096) {
            chunk[i] = (i & 0xff) as u8;
        }
        total += CHUNK as u64;
        held.push(chunk);
    }
    println!("holding {} MiB", total >> 20);
    std::thread::sleep(Duration::from_secs(3600));
    std::hint::black_box(&held);
}
