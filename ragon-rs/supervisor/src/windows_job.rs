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
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// `CREATE_SUSPENDED | CREATE_NO_WINDOW`, the creation flags of the engine.
pub const ENGINE_CREATION_FLAGS: u32 =
    windows_sys::Win32::System::Threading::CREATE_SUSPENDED | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub struct Job(HANDLE);

// The handle is a kernel object handle, usable from any thread.
unsafe impl Send for Job {}

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
fn resume_threads(pid: u32) -> io::Result<()> {
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
