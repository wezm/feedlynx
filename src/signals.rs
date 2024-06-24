#[cfg(unix)]
pub use unix::SignalHandle;

#[cfg(unix)]
mod unix {
    use std::{io, mem, ptr};

    pub struct SignalHandle(Sigset);

    impl SignalHandle {
        pub fn new() -> io::Result<Self> {
            let mut set = Sigset::empty()?;
            set.addsig(libc::SIGINT)?;
            set.addsig(libc::SIGTERM)?;
            set.addsig(libc::SIGHUP)?;

            set.setsigmask()?;
            Ok(SignalHandle(set))
        }

        pub fn block_until_signalled(&self) -> io::Result<()> {
            self.0.wait()?;
            Ok(())
        }
    }

    type SignalCode = libc::c_int;

    // Derived from: https://github.com/habitat-sh/habitat/blob/631af77f7705fb4ea68a5464f269e0c0b9283a91/components/core/src/os/signals/unix.rs#L146
    // Licence: Apache-2.0

    /// Sigset is a wrapper for the underlying libc type.
    struct Sigset {
        inner: libc::sigset_t,
    }

    impl Sigset {
        /// empty returns an empty Sigset.
        ///
        /// For more information on the relevant libc function see:
        ///
        /// http://man7.org/linux/man-pages/man3/sigsetops.3.html
        fn empty() -> io::Result<Sigset> {
            let mut set: libc::sigset_t = unsafe { mem::zeroed() };
            let ret = unsafe { libc::sigemptyset(&mut set) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(Sigset { inner: set })
            }
        }

        /// addsig adds the given signal to the Sigset.
        ///
        /// For more information on the relevant libc function see:
        ///
        /// http://man7.org/linux/man-pages/man3/sigsetops.3.html
        fn addsig(&mut self, signal: SignalCode) -> io::Result<()> {
            let ret = unsafe { libc::sigaddset(&mut self.inner, signal) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        /// setsigmask sets the calling thread's signal mask to the current
        /// sigmask, blocking delivery of all signals in the sigmask.
        ///
        /// > A new thread inherits a copy of its creator's signal mask.
        ///
        /// This should be called before wait():
        ///
        /// > The sigwait() function suspends execution of the calling thread until one of the signals specified in the signal set set becomes pending. For a signal to become pending, it must first be blocked.
        ///
        /// For more information on the relevant libc function see:
        ///
        /// http://man7.org/linux/man-pages/man3/pthread_sigmask.3.html
        fn setsigmask(&self) -> io::Result<()> {
            let ret =
                unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.inner, ptr::null_mut()) };
            if ret != 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        /// wait blocks until a signal in the current sigset has been
        /// delivered to the thread.
        ///
        /// Callers should call setsigmask() before this function to avoid
        /// race conditions.
        ///
        /// For information on the relevant libc function see:
        ///
        /// http://man7.org/linux/man-pages/man3/sigwait.3.html
        ///
        /// The manual page on linux only lists a single failure case:
        ///
        /// > EINVAL set contains an invalid signal number.
        ///
        /// thus most callers should be able to expect success.
        fn wait(&self) -> io::Result<SignalCode> {
            let mut signal: libc::c_int = 0;
            let ret = unsafe { libc::sigwait(&self.inner, &mut signal) };
            if ret != 0 {
                Err(io::Error::from_raw_os_error(ret))
            } else {
                Ok(signal)
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    // https://github.com/Detegr/rust-ctrlc/blob/b543abe6c25bd54754bbbbcfcff566e046f8e609/src/platform/windows/mod.rs

    // Copyright (c) 2017 CtrlC developers
    // Licensed under the Apache License, Version 2.0
    // <LICENSE-APACHE or
    // http://www.apache.org/licenses/LICENSE-2.0> or the MIT
    // license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
    // at your option. All files in the project carrying such
    // notice may not be copied, modified, or distributed except
    // according to those terms.

    use std::io;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    use windows_sys::Win32::System::Threading::{
        CreateSemaphoreA, ReleaseSemaphore, WaitForSingleObject, INFINITE,
    };

    /// Platform specific error type
    pub type Error = io::Error;

    /// Platform specific signal type
    pub type Signal = u32;

    const MAX_SEM_COUNT: i32 = 255;
    static mut SEMAPHORE: HANDLE = 0 as HANDLE;
    const TRUE: BOOL = 1;
    const FALSE: BOOL = 0;

    unsafe extern "system" fn os_handler(_: u32) -> BOOL {
        // Assuming this always succeeds. Can't really handle errors in any meaningful way.
        ReleaseSemaphore(SEMAPHORE, 1, ptr::null_mut());
        TRUE
    }

    /// Register os signal handler.
    ///
    /// Must be called before calling [`block_ctrl_c()`](fn.block_ctrl_c.html)
    /// and should only be called once.
    ///
    /// # Errors
    /// Will return an error if a system error occurred.
    ///
    #[inline]
    pub unsafe fn init_os_handler(_overwrite: bool) -> Result<(), Error> {
        SEMAPHORE = CreateSemaphoreA(ptr::null_mut(), 0, MAX_SEM_COUNT, ptr::null());
        if SEMAPHORE == 0 {
            return Err(io::Error::last_os_error());
        }

        if SetConsoleCtrlHandler(Some(os_handler), TRUE) == FALSE {
            let e = io::Error::last_os_error();
            CloseHandle(SEMAPHORE);
            SEMAPHORE = 0 as HANDLE;
            return Err(e);
        }

        Ok(())
    }

    /// Blocks until a Ctrl-C signal is received.
    ///
    /// Must be called after calling [`init_os_handler()`](fn.init_os_handler.html).
    ///
    /// # Errors
    /// Will return an error if a system error occurred.
    ///
    #[inline]
    pub unsafe fn block_ctrl_c() -> Result<(), Error> {
        match WaitForSingleObject(SEMAPHORE, INFINITE) {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            ret => Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "WaitForSingleObject(), unexpected return value \"{:x}\"",
                    ret
                ),
            )),
        }
    }
}
