#[cfg(unix)]
pub use unix::block_until_signalled;

#[cfg(unix)]
mod unix {
    use std::{io, mem, ptr};

    type SignalCode = libc::c_int;

    pub fn block_until_signalled() -> io::Result<()> {
        let mut set = Sigset::empty()?;
        set.addsig(libc::SIGINT)?;
        set.addsig(libc::SIGTERM)?;
        set.addsig(libc::SIGHUP)?;

        set.setsigmask()?;

        // TODO: call this in the spawned thread
        set.wait()?;
        Ok(())
    }

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
        /// This should be called before wait() to avoid race conditions.
        ///
        /// For more information on the relevant libc function see:
        ///
        /// http://man7.org/linux/man-pages/man3/pthread_sigmask.3.html
        fn setsigmask(&self) -> io::Result<()> {
            let ret =
                unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.inner, ptr::null_mut()) };
            if ret < 0 {
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
