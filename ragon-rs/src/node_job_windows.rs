//! `ragon server job-submit|job-list|job-logs|job-fetched|job-cancel|job-out ...` on the Windows
//! node: the job operations the sender (`NodeJobSender`) asks for over SSH, which the Linux node
//! answers with `ragon-admin node serve-job`. No Python runs on the Windows node: the operations
//! are the supervisor crate's (ragon-rs/supervisor/src/job_cli.rs `serve`), on the jobs folder of
//! the service (`%ProgramData%\Ragon\jobs`), and the service's runner thread
//! (`server install --jobs-dir D`) runs each job in a Job Object with its memory cap, its time
//! limit and the unelevated sandbox, one job per lane. A job outlives the SSH session that sent
//! it: the session only writes the record and follows the output.
//!
//! `ragon server job <verb>` is the same store under its own names (job_cli.rs `run`).

use std::path::Path;

use ragon_supervisor::job_cli;
use ragon_supervisor::jobs::Store;

/// Whether `ragon server <args>` is a job operation of the sender's.
pub fn is_job_operation(args: &[String]) -> bool {
    args.first().is_some_and(|op| op.starts_with("job-"))
}

/// Carries out a job operation on the jobs folder `jobs_dir`; refuses (exit 2) anything else.
pub fn serve(jobs_dir: &Path, args: &[String]) -> i32 {
    let Some((operation, rest)) = args.split_first() else {
        eprintln!("{}", job_cli::REFUSAL);
        return 2;
    };
    match job_cli::serve(&Store::new(jobs_dir), operation, rest) {
        Some(code) => code,
        None => {
            eprintln!("{}", job_cli::REFUSAL);
            2
        }
    }
}

/// Runs a job operation on the service's jobs folder.
pub fn run_cli(args: &[String]) -> i32 {
    serve(&job_cli::default_jobs_dir(), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_the_job_operations_are_served() {
        let dir = std::env::temp_dir().join(format!("ragon-node-job-{}", std::process::id()));
        assert!(is_job_operation(&args(&["job-list"])));
        assert!(!is_job_operation(&args(&["status"])));
        assert!(!is_job_operation(&[]));
        assert_eq!(serve(&dir, &args(&["job-list"])), 0);
        assert_eq!(serve(&dir, &args(&["job-cancel", "20261125-1000-abc"])), 2, "no such job");
        assert_eq!(serve(&dir, &args(&["set-model", "x"])), 2);
        assert_eq!(serve(&dir, &[]), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
