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
#[path = "session_guard_test.rs"]
mod tests;
