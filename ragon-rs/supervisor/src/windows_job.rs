//! The Job Object the engine runs in. It is created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and
//! only the supervisor holds its handle, so when the supervisor dies, however it dies, Windows
//! closes the handle and kills the engine and everything the engine started. The engine is created
//! suspended and resumed only after it is in the job, so nothing it starts can escape the job.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectAssociateCompletionPortInformation, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// `CREATE_SUSPENDED | CREATE_NO_WINDOW`, the creation flags of the engine.
pub const ENGINE_CREATION_FLAGS: u32 =
    windows_sys::Win32::System::Threading::CREATE_SUSPENDED | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub struct Job(HANDLE);

// The handle is a kernel object handle, usable from any thread.
unsafe impl Send for Job {}

/// `JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT` and `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT`: what the job's
/// completion port receives when an allocation is refused at the cap.
const MSG_PROCESS_MEMORY_LIMIT: u32 = 9;
const MSG_JOB_MEMORY_LIMIT: u32 = 10;

/// The limits of a node job's Job Object (src/job_runner.rs). Windows refuses the allocation
/// that would pass `memory_bytes` (it kills nothing); the completion port reports it and the
/// runner stops the job. Time is the runner's own wall clock, not `JOB_OBJECT_LIMIT_JOB_TIME`
/// (CPU time).
#[derive(Clone, Copy, Debug)]
pub struct JobLimits {
    pub memory_bytes: u64,
    pub max_processes: u32,
}

/// A Job Object with limits and a completion port that reports the memory cap.
pub struct LimitedJob {
    job: Job,
    port: HANDLE,
    tripped: bool,
}

unsafe impl Send for LimitedJob {}

impl LimitedJob {
    pub fn new(limits: JobLimits) -> io::Result<Self> {
        let job = Job::new()?;
        // SAFETY: plain Win32 calls on handles owned here; the port is closed in Drop.
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            info.BasicLimitInformation.ActiveProcessLimit = limits.max_processes;
            info.JobMemoryLimit = limits.memory_bytes as usize;
            let ok = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, 1);
            if port == 0 {
                return Err(io::Error::last_os_error());
            }
            let limited = LimitedJob { job, port, tripped: false };
            let assoc = JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey: std::ptr::null_mut(), CompletionPort: port };
            let ok = SetInformationJobObject(
                limited.job.0,
                JobObjectAssociateCompletionPortInformation,
                (&assoc as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                std::mem::size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(limited)
        }
    }

    pub fn adopt_suspended(&self, child: &Child) -> io::Result<()> {
        self.job.adopt_suspended(child)
    }

    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        self.job.terminate(exit_code)
    }

    /// True once an allocation in the job was refused at the cap (drains the port's messages).
    pub fn memory_tripped(&mut self) -> bool {
        loop {
            let mut message = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
            // SAFETY: valid port handle and out pointers; zero timeout.
            let ok = unsafe { GetQueuedCompletionStatus(self.port, &mut message, &mut key, &mut overlapped, 0) };
            if ok == 0 {
                break;
            }
            if message == MSG_JOB_MEMORY_LIMIT || message == MSG_PROCESS_MEMORY_LIMIT {
                self.tripped = true;
            }
        }
        self.tripped
    }

    /// The job's peak committed memory, in bytes.
    pub fn peak_memory(&self) -> Option<u64> {
        // SAFETY: valid job handle; the struct is plain data.
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                self.job.0,
                JobObjectExtendedLimitInformation,
                (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            );
            (ok != 0).then_some(info.PeakJobMemoryUsed as u64)
        }
    }
}

impl Drop for LimitedJob {
    fn drop(&mut self) {
        // SAFETY: we own the port handle; the job closes itself.
        unsafe { CloseHandle(self.port) };
    }
}

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: plain Win32 calls; the handle is owned by the returned value and closed in Drop.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle == 0 {
                return Err(io::Error::last_os_error());
            }
            let job = Job(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }

    /// Puts a child spawned with [`ENGINE_CREATION_FLAGS`] in the job, then resumes its threads.
    pub fn adopt_suspended(&self, child: &Child) -> io::Result<()> {
        // SAFETY: the child's handle is valid while `child` lives.
        let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        resume_threads(child.id())
    }

    /// Kills every process in the job (the job itself stays usable for the next engine).
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: valid job handle.
        if unsafe { TerminateJobObject(self.0, exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle. Closing it kills what is left in the job.
        unsafe { CloseHandle(self.0) };
    }
}

/// Resumes every thread of `pid` (a process created suspended has exactly one).
pub(crate) fn resume_threads(pid: u32) -> io::Result<()> {
    // SAFETY: snapshot and thread handles are closed before returning.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut more = Thread32First(snap, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread != 0 {
                    if ResumeThread(thread) != u32::MAX {
                        resumed += 1;
                    }
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        if resumed == 0 {
            return Err(io::Error::other(format!("could not resume any thread of pid {pid}")));
        }
    }
    Ok(())
}
