use std::sync::atomic::{AtomicBool, Ordering};

static SESSION_OWNED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGuardError {
    AlreadyInitialized,
}

#[derive(Debug)]
pub struct SessionGuard {
    released: bool,
}

impl SessionGuard {
    pub fn acquire() -> Result<Self, SessionGuardError> {
        log::debug!("[session_guard] session acquire requested");
        let already_owned = SESSION_OWNED.swap(true, Ordering::SeqCst);
        if already_owned {
            log::debug!("[session_guard] session acquire rejected: already initialized");
            return Err(SessionGuardError::AlreadyInitialized);
        }

        log::debug!("[session_guard] session acquire granted");
        Ok(Self { released: false })
    }

    pub fn release(mut self) {
        log::debug!("[session_guard] session release requested");
        if !self.released {
            SESSION_OWNED.store(false, Ordering::SeqCst);
            self.released = true;
            log::debug!("[session_guard] session released");
        }
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if !self.released {
            log::debug!("[session_guard] session dropped and released");
            SESSION_OWNED.store(false, Ordering::SeqCst);
            self.released = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn session_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn rejects_second_live_session_guard() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        SESSION_OWNED.store(false, Ordering::SeqCst);

        let first = SessionGuard::acquire().unwrap();
        let second = SessionGuard::acquire();

        assert!(matches!(second, Err(SessionGuardError::AlreadyInitialized)));

        drop(first);
        SESSION_OWNED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn allows_reacquiring_after_guard_is_released() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        SESSION_OWNED.store(false, Ordering::SeqCst);

        let first = SessionGuard::acquire().unwrap();
        first.release();

        let second = SessionGuard::acquire();

        assert!(second.is_ok());

        drop(second);
        SESSION_OWNED.store(false, Ordering::SeqCst);
    }
}
