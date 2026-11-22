//! Keeps the kernel from outliving the app.
//!
//! A proxy client can be killed in ways that skip every cleanup path:
//! `taskkill /f`, the task manager, a crash, a power-loss reboot. If the kernel
//! survives that, the machine is left with a process that still holds the
//! inbound port (so the next start fails with "address already in use") and an
//! open tunnel the user believes is closed.
//!
//! Windows solves this at the OS level: put the child into a *job object* with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. When the last handle to the job closes
//! — which the OS does when our process dies, however it dies — every process
//! inside the job is terminated. No polling, no watchdog, nothing to forget.
//!
//! Adoption is best effort: if it fails (an exotic parent job configuration),
//! the app still works and we only lose the guarantee, so the caller logs and
//! carries on instead of refusing to start the kernel.

/// Owns the job object for this process. Create it once and keep it alive:
/// dropping it kills the kernels that were adopted into it.
pub struct KillOnCloseJob {
    /// The job handle, stored as an integer so this type stays `Send + Sync`
    /// (a raw `HANDLE` is a `*mut c_void`, which is neither) and can live
    /// inside the shared runtime state.
    #[cfg(windows)]
    handle: usize,
}

impl KillOnCloseJob {
    /// Create a job that kills its members when this handle goes away.
    pub fn new() -> crate::error::AppResult<Self> {
        #[cfg(windows)]
        {
            windows_impl::create()
        }
        #[cfg(not(windows))]
        {
            Ok(Self {})
        }
    }

    /// Put an already-spawned process into the job.
    pub fn adopt(&self, pid: u32) -> crate::error::AppResult<()> {
        #[cfg(windows)]
        {
            windows_impl::adopt(self.handle, pid)
        }
        #[cfg(not(windows))]
        {
            let _ = pid;
            Ok(())
        }
    }
}

#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        // Closing the handle is what terminates the members; that is the point.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle as HANDLE);
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    use super::KillOnCloseJob;
    use crate::error::{AppError, AppResult};

    pub fn create() -> AppResult<KillOnCloseJob> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(AppError::Engine(format!(
                    "创建作业对象失败：{}",
                    std::io::Error::last_os_error()
                )));
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(AppError::Engine(format!("配置作业对象失败：{error}")));
            }

            Ok(KillOnCloseJob {
                handle: handle as usize,
            })
        }
    }

    pub fn adopt(job: usize, pid: u32) -> AppResult<()> {
        let job = job as HANDLE;
        unsafe {
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(AppError::Engine(format!(
                    "打开内核进程 {pid} 失败：{}",
                    std::io::Error::last_os_error()
                )));
            }
            let ok = AssignProcessToJobObject(job, process);
            let error = std::io::Error::last_os_error();
            CloseHandle(process);
            if ok == 0 {
                return Err(AppError::Engine(format!(
                    "把内核进程 {pid} 加入作业对象失败：{error}"
                )));
            }
            Ok(())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::KillOnCloseJob;

    #[test]
    fn the_job_object_can_be_created() {
        // Creating it is the whole contract on our side: Windows guarantees the
        // kill-on-close behaviour once the flag is set.
        let job = KillOnCloseJob::new().expect("job object");
        // A pid that cannot exist must fail loudly instead of pretending.
        assert!(job.adopt(u32::MAX).is_err());
    }
}
