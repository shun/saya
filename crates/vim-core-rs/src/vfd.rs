use std::collections::{HashMap, VecDeque};
use std::ffi::{c_int, c_short, c_void};
use std::sync::{Mutex, OnceLock};

#[repr(C)]
pub struct pollfd {
    pub fd: c_int,
    pub events: c_short,
    pub revents: c_short,
}

pub const POLLIN: c_short = 0x0001;
pub const POLLOUT: c_short = 0x0004;

// Requirements: 2.1, 2.2, 2.3, 7.5
pub struct VfdState {
    pub read_queue: VecDeque<u8>,
    pub is_closed: bool,
}

// Requirements: 1.2, 3.1, 8.1
pub struct JobState {
    pub vfd_in: i32,
    pub vfd_out: i32,
    pub vfd_err: i32,
    pub is_closed: bool,
    pub status: i32,
    pub exit_code: i32,
    pub reaped: bool,
}

#[derive(Debug, Clone)]
pub struct PendingJobWrite {
    pub vfd: i32,
    pub data: Vec<u8>,
}

pub struct VfdManager {
    pub vfds: HashMap<c_int, VfdState>,
    pub jobs: HashMap<i32, JobState>,
    pending_job_writes: VecDeque<PendingJobWrite>,
    next_vfd: i32,
}

impl VfdManager {
    pub fn new() -> Self {
        Self {
            vfds: HashMap::new(),
            jobs: HashMap::new(),
            pending_job_writes: VecDeque::new(),
            next_vfd: 512,
        }
    }

    pub fn register_job(&mut self, job_id: i32, vfd_in: i32, vfd_out: i32, vfd_err: i32) {
        self.jobs.insert(
            job_id,
            JobState {
                vfd_in,
                vfd_out,
                vfd_err,
                is_closed: false,
                status: 0,
                exit_code: 0,
                reaped: false,
            },
        );
        self.ensure_vfd(vfd_in);
        self.ensure_vfd(vfd_out);
        self.ensure_vfd(vfd_err);
    }

    pub fn ensure_vfd(&mut self, fd: i32) {
        if fd >= 0 && !self.vfds.contains_key(&fd) {
            self.vfds.insert(
                fd,
                VfdState {
                    read_queue: VecDeque::new(),
                    is_closed: false,
                },
            );
        }
    }

    pub fn update_job_status(&mut self, job_id: i32, status: i32, exit_code: i32) -> bool {
        let (vfd_in, vfd_out, vfd_err) = if let Some(job) = self.jobs.get_mut(&job_id) {
            job.status = status;
            if status == 1 || status == 2 {
                job.is_closed = true;
                job.exit_code = exit_code;
                (job.vfd_in, job.vfd_out, job.vfd_err)
            } else {
                return true;
            }
        } else {
            return false;
        };

        // Close associated VFDs to signal EOF to Vim
        self.close_fd(vfd_in);
        self.close_fd(vfd_out);
        self.close_fd(vfd_err);
        true
    }

    pub fn is_job_input_fd(&self, fd: c_int) -> bool {
        self.jobs
            .values()
            .any(|job| job.vfd_in == fd && !job.is_closed)
    }

    pub fn enqueue_job_write(&mut self, fd: c_int, data: Vec<u8>) -> bool {
        if self.is_job_input_fd(fd) {
            self.pending_job_writes
                .push_back(PendingJobWrite { vfd: fd, data });
            true
        } else {
            false
        }
    }

    pub fn take_pending_job_write(&mut self) -> Option<PendingJobWrite> {
        self.pending_job_writes.pop_front()
    }

    pub fn inject_data(&mut self, fd: c_int, data: &[u8]) -> bool {
        if let Some(state) = self.vfds.get_mut(&fd)
            && !state.is_closed
        {
            state.read_queue.extend(data);
            return true;
        }
        false
    }

    pub fn read_data(&mut self, fd: c_int, buf: &mut [u8]) -> isize {
        if let Some(state) = self.vfds.get_mut(&fd) {
            if state.read_queue.is_empty() {
                if state.is_closed {
                    return 0; // EOF
                } else {
                    return -2; // EAGAIN
                }
            } else {
                let len = buf.len().min(state.read_queue.len());
                for (i, b) in state.read_queue.drain(..len).enumerate() {
                    buf[i] = b;
                }
                return len as isize;
            }
        }
        -1 // Error
    }

    pub fn close_fd(&mut self, fd: c_int) -> c_int {
        if let Some(state) = self.vfds.get_mut(&fd) {
            state.is_closed = true;
            return 0;
        }
        -1
    }

    pub fn poll_fds(&self, fds: &mut [pollfd]) -> c_int {
        let mut count = 0;
        for pfd in fds.iter_mut() {
            if let Some(state) = self.vfds.get(&pfd.fd) {
                pfd.revents = 0;
                if (pfd.events & POLLIN) != 0 && (!state.read_queue.is_empty() || state.is_closed) {
                    pfd.revents |= POLLIN;
                }
                // We assume we can always write for now
                if (pfd.events & POLLOUT) != 0 && !state.is_closed {
                    pfd.revents |= POLLOUT;
                }
                if pfd.revents != 0 {
                    count += 1;
                }
            }
        }
        count
    }
    pub fn clear_all(&mut self) {
        self.vfds.clear();
        self.jobs.clear();
        self.pending_job_writes.clear();
        self.next_vfd = 512;
    }
}

static MANAGER: OnceLock<Mutex<VfdManager>> = OnceLock::new();

pub fn get_manager() -> std::sync::MutexGuard<'static, VfdManager> {
    MANAGER
        .get_or_init(|| Mutex::new(VfdManager::new()))
        .lock()
        .unwrap()
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_vfd_read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
    let mut mgr = get_manager();
    let slice = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, count) };
    mgr.read_data(fd, slice)
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_vfd_write(_fd: c_int, _buf: *const c_void, count: usize) -> isize {
    if count == 0 {
        return 0;
    }

    let data = unsafe { std::slice::from_raw_parts(_buf as *const u8, count) }.to_vec();
    let mut mgr = get_manager();
    let _ = mgr.enqueue_job_write(_fd, data);
    count as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_vfd_close(fd: c_int) -> c_int {
    let mut mgr = get_manager();
    mgr.close_fd(fd)
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_vfd_poll(
    fds: *mut pollfd,
    nfds: std::ffi::c_ulong,
    _timeout: c_int,
) -> c_int {
    let mgr = get_manager();
    let slice = unsafe { std::slice::from_raw_parts_mut(fds, nfds as usize) };
    mgr.poll_fds(slice)
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_job_get_status(job_id: c_int, exit_code_out: *mut c_int) -> c_int {
    let mut mgr = get_manager();
    if let Some(job) = mgr.jobs.get_mut(&job_id) {
        if job.is_closed && !job.reaped {
            if !exit_code_out.is_null() {
                unsafe {
                    *exit_code_out = job.exit_code;
                }
            }
            job.reaped = true;
            return 1; // Ended
        } else if job.is_closed {
            return 2; // Dead
        } else {
            return 0; // Running
        }
    }
    -1 // Not found
}

#[unsafe(no_mangle)]
pub extern "C" fn vim_core_job_clear(job_id: c_int) {
    let mut mgr = get_manager();
    mgr.jobs.remove(&job_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    impl VfdManager {
        fn allocate_vfd(&mut self) -> i32 {
            let fd = self.next_vfd;
            self.next_vfd += 1;
            self.vfds.insert(
                fd,
                VfdState {
                    read_queue: VecDeque::new(),
                    is_closed: false,
                },
            );
            fd
        }
    }

    #[test]
    fn test_vfd_manager_read_write_poll() {
        let mut mgr = VfdManager::new();
        let fd = mgr.allocate_vfd();

        let mut pfd = [pollfd {
            fd,
            events: POLLIN,
            revents: 0,
        }];
        assert_eq!(mgr.poll_fds(&mut pfd), 0); // No data, not closed

        mgr.inject_data(fd, b"hello");
        assert_eq!(mgr.poll_fds(&mut pfd), 1); // Has data
        assert_eq!(pfd[0].revents, POLLIN);

        let mut buf = [0u8; 10];
        let n = mgr.read_data(fd, &mut buf);
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"hello");

        assert_eq!(mgr.poll_fds(&mut pfd), 0); // Data consumed

        mgr.close_fd(fd);
        assert_eq!(mgr.poll_fds(&mut pfd), 1); // Closed means it is readable (will return EOF)
        assert_eq!(pfd[0].revents, POLLIN);

        let n = mgr.read_data(fd, &mut buf);
        assert_eq!(n, 0); // EOF
    }

    #[test]
    fn test_vfd_queue_large_data() {
        let mut mgr = VfdManager::new();
        let fd = mgr.allocate_vfd();

        let large_data = vec![0x41; 1024 * 1024]; // 1MB
        mgr.inject_data(fd, &large_data);

        let mut pfd = [pollfd {
            fd,
            events: POLLIN,
            revents: 0,
        }];
        assert_eq!(mgr.poll_fds(&mut pfd), 1);

        let mut buf = vec![0u8; 1024 * 512]; // Read 512KB
        let n1 = mgr.read_data(fd, &mut buf);
        assert_eq!(n1, 1024 * 512);

        let n2 = mgr.read_data(fd, &mut buf);
        assert_eq!(n2, 1024 * 512);

        let n3 = mgr.read_data(fd, &mut buf);
        assert_eq!(n3, -2); // No more data (EAGAIN)
    }
}
