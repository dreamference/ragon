//! The engine as a child process: started from a program and its arguments, stdout and stderr
//! appended to `engine.log` in the state directory. On Windows it runs in the supervisor's Job
//! Object (src/windows_job.rs), so it dies with the supervisor.

use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::core::Engine;

pub struct ChildEngine {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub log_path: Option<PathBuf>,
    child: Option<Child>,
    #[cfg(windows)]
    job: crate::windows_job::Job,
}

impl ChildEngine {
    pub fn new(program: PathBuf, args: Vec<String>, log_path: Option<PathBuf>) -> io::Result<Self> {
        Ok(Self {
            program,
            args,
            log_path,
            child: None,
            #[cfg(windows)]
            job: crate::windows_job::Job::new()?,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    fn output(&self) -> io::Result<(Stdio, Stdio)> {
        match &self.log_path {
            Some(path) => {
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                Ok((Stdio::from(file.try_clone()?), Stdio::from(file)))
            }
            None => Ok((Stdio::null(), Stdio::null())),
        }
    }
}

impl Engine for ChildEngine {
    fn start(&mut self) -> io::Result<u32> {
        let (out, err) = self.output()?;
        let mut command = Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null()).stdout(out).stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(crate::windows_job::ENGINE_CREATION_FLAGS);
        }
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut child = command.spawn()?;
        #[cfg(windows)]
        if let Err(e) = self.job.adopt_suspended(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        let pid = child.id();
        self.child = Some(child);
        Ok(pid)
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        let Some(child) = self.child.as_mut() else { return Ok(Some(-1)) };
        match child.try_wait()? {
            Some(status) => {
                self.child = None;
                Ok(Some(status.code().unwrap_or(-1)))
            }
            None => Ok(None),
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        let Some(mut child) = self.child.take() else { return Ok(()) };
        #[cfg(windows)]
        let killed = self.job.terminate(1).or_else(|_| child.kill());
        #[cfg(not(windows))]
        let killed = child.kill();
        let waited = child.wait().map(|_| ());
        killed.or(waited)
    }
}

impl Drop for ChildEngine {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
