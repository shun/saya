use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use vim_core_rs::{
    CoreJobStartRequest, CoreVfsError, CoreVfsErrorKind, CoreVfsRequest, CoreVfsResponse, JobStatus,
};

use crate::core::bridge::CoreBridge;

#[derive(Debug)]
pub enum HostActionError {
    VfsResponseRejected(String),
    JobStatusRejected(String),
    VfdInjectionRejected(String),
}

#[derive(Debug, Default)]
pub struct CoreHostActionRuntime {
    vfs: LocalVfsHost,
    jobs: JobHost,
}

pub type HostActionRuntime = CoreHostActionRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostVfsRequestEffect {
    pub load_failed: bool,
}

impl CoreHostActionRuntime {
    pub fn handle_vfs_request(
        &mut self,
        core_bridge: &mut CoreBridge,
        request: CoreVfsRequest,
    ) -> Result<HostVfsRequestEffect, HostActionError> {
        let is_load_request = matches!(&request, CoreVfsRequest::Load { .. });
        let response = self.vfs.response_for(request);
        let effect = HostVfsRequestEffect {
            load_failed: is_load_request && matches!(&response, CoreVfsResponse::Failed { .. }),
        };
        core_bridge
            .submit_vfs_response(response)
            .map_err(|error| HostActionError::VfsResponseRejected(format!("{error:?}")))?;
        Ok(effect)
    }

    pub fn start_job(
        &mut self,
        core_bridge: &mut CoreBridge,
        request: CoreJobStartRequest,
    ) -> Result<(), HostActionError> {
        self.jobs.start(core_bridge, request)
    }

    pub fn write_job(&mut self, vfd: i32, data: Vec<u8>) {
        self.jobs.write(vfd, &data);
    }

    pub fn stop_job(
        &mut self,
        core_bridge: &mut CoreBridge,
        job_id: i32,
    ) -> Result<(), HostActionError> {
        self.jobs.stop(core_bridge, job_id)
    }

    pub fn drain_job_events(
        &mut self,
        core_bridge: &mut CoreBridge,
    ) -> Result<(), HostActionError> {
        self.jobs.drain_events(core_bridge)
    }
}

#[derive(Debug, Default)]
struct LocalVfsHost;

impl LocalVfsHost {
    fn response_for(&mut self, request: CoreVfsRequest) -> CoreVfsResponse {
        match request {
            CoreVfsRequest::Resolve {
                request_id,
                locator,
                ..
            } => {
                let path = locator_to_path(&locator);
                if path.exists() {
                    let document_id = document_id_for_path(&path);
                    CoreVfsResponse::Resolved {
                        request_id,
                        document_id,
                        display_name: locator,
                    }
                } else {
                    CoreVfsResponse::ResolvedMissing {
                        request_id,
                        locator,
                    }
                }
            }
            CoreVfsRequest::Exists {
                request_id,
                locator,
            } => CoreVfsResponse::ExistsResult {
                request_id,
                exists: locator_to_path(&locator).exists(),
            },
            CoreVfsRequest::Load {
                request_id,
                document_id,
                ..
            } => {
                match path_from_document_id(&document_id).and_then(|path| load_local_text(&path)) {
                    Some(text) => CoreVfsResponse::Loaded {
                        request_id,
                        document_id,
                        text,
                    },
                    None => CoreVfsResponse::Failed {
                        request_id,
                        error: vfs_error(
                            CoreVfsErrorKind::HostUnavailable,
                            "failed to load local file",
                        ),
                    },
                }
            }
            CoreVfsRequest::Save {
                request_id,
                document_id,
                target_locator,
                text,
                ..
            } => {
                let path = target_locator
                    .map(|locator| locator_to_path(&locator))
                    .or_else(|| path_from_document_id(&document_id));
                match path {
                    Some(path) if path.is_dir() => {
                        log::debug!(
                            "[core_host_actions] refusing to save directory listing as a regular file: path={}",
                            path.display()
                        );
                        CoreVfsResponse::Failed {
                            request_id,
                            error: vfs_error(
                                CoreVfsErrorKind::HostUnavailable,
                                "directory listings are not saved as regular files",
                            ),
                        }
                    }
                    Some(path) if fs::write(&path, text).is_ok() => CoreVfsResponse::Saved {
                        request_id,
                        document_id,
                    },
                    _ => CoreVfsResponse::Failed {
                        request_id,
                        error: vfs_error(
                            CoreVfsErrorKind::HostUnavailable,
                            "failed to save local file",
                        ),
                    },
                }
            }
        }
    }
}

fn document_id_for_path(path: &Path) -> String {
    let absolute = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    format!("file://{absolute}")
}

fn path_from_document_id(document_id: &str) -> Option<PathBuf> {
    document_id
        .strip_prefix("file://")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(document_id)).filter(|path| path.exists()))
}

fn load_local_text(path: &Path) -> Option<String> {
    if path.is_dir() {
        let started_at = Instant::now();
        log::debug!(
            "[core_host_actions] loading local directory as editable listing: path={}",
            path.display()
        );
        return match render_directory_listing(path) {
            Ok(listing) => {
                log::debug!(
                    "[core_host_actions] loaded local directory listing: path={}, entry_count={}, duration_ms={}",
                    path.display(),
                    listing.entry_count,
                    started_at.elapsed().as_millis()
                );
                Some(listing.text)
            }
            Err(error) => {
                log::debug!(
                    "[core_host_actions] failed to load local directory listing: path={}, duration_ms={}, error={}",
                    path.display(),
                    started_at.elapsed().as_millis(),
                    error
                );
                None
            }
        };
    }
    fs::read_to_string(path).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryListing {
    text: String,
    entry_count: usize,
}

fn render_directory_listing(path: &Path) -> std::io::Result<DirectoryListing> {
    let mut entries = fs::read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let is_directory = file_type.is_dir();
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if is_directory {
                name.push('/');
            }
            Ok((is_directory, name))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by(|left, right| {
        directory_group_rank(left.0)
            .cmp(&directory_group_rank(right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    let directory_count = entries.iter().filter(|entry| entry.0).count();
    let entry_count = entries.len();
    log::debug!(
        "[core_host_actions][dired] rendered directory listing with eza-style directory grouping: path={}, directories={}, non_directories={}",
        path.display(),
        directory_count,
        entry_count.saturating_sub(directory_count)
    );
    let text = if entries.is_empty() {
        String::new()
    } else {
        entries
            .iter()
            .map(|entry| entry.1.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    Ok(DirectoryListing { text, entry_count })
}

fn directory_group_rank(is_directory: bool) -> usize {
    if is_directory { 0 } else { 1 }
}

fn locator_to_path(locator: &str) -> PathBuf {
    locator
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(locator))
}

fn vfs_error(kind: CoreVfsErrorKind, message: &str) -> CoreVfsError {
    CoreVfsError {
        kind,
        message: Some(message.to_string()),
    }
}

#[derive(Debug)]
struct RunningJob {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
    vfd_in: i32,
}

#[derive(Debug)]
enum HostJobEvent {
    Output { vfd: i32, data: Vec<u8> },
    Exited { job_id: i32, exit_code: i32 },
    Failed { job_id: i32, message: String },
}

#[derive(Debug)]
struct JobHost {
    jobs: BTreeMap<i32, RunningJob>,
    sender: Sender<HostJobEvent>,
    receiver: Receiver<HostJobEvent>,
}

impl Default for JobHost {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            jobs: BTreeMap::new(),
            sender,
            receiver,
        }
    }
}

impl JobHost {
    fn start(
        &mut self,
        core_bridge: &mut CoreBridge,
        request: CoreJobStartRequest,
    ) -> Result<(), HostActionError> {
        let Some(program) = request.argv.first() else {
            return self.notify_status(core_bridge, request.job_id, JobStatus::Failed, -1);
        };

        let mut command = Command::new(program);
        command.args(request.argv.iter().skip(1));
        if let Some(cwd) = request.cwd.as_ref() {
            command.current_dir(cwd);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                log::debug!(
                    "[core_host_actions] failed to spawn job: job_id={}, argv={:?}, error={}",
                    request.job_id,
                    request.argv,
                    error
                );
                return self.notify_status(core_bridge, request.job_id, JobStatus::Failed, -1);
            }
        };

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));

        if let Some(stdout) = stdout {
            spawn_reader_thread(self.sender.clone(), request.job_id, request.vfd_out, stdout);
        }
        if let Some(stderr) = stderr {
            spawn_reader_thread(self.sender.clone(), request.job_id, request.vfd_err, stderr);
        }
        spawn_wait_thread(self.sender.clone(), request.job_id, child.clone());

        self.jobs.insert(
            request.job_id,
            RunningJob {
                child,
                stdin,
                vfd_in: request.vfd_in,
            },
        );
        self.notify_status(core_bridge, request.job_id, JobStatus::Running, 0)
    }

    fn write(&mut self, vfd: i32, data: &[u8]) {
        let Some(job) = self.jobs.values_mut().find(|job| job.vfd_in == vfd) else {
            log::debug!(
                "[core_host_actions] ignoring job write for unknown stdin vfd: vfd={}, bytes={}",
                vfd,
                data.len()
            );
            return;
        };
        if let Some(stdin) = job.stdin.as_mut()
            && let Err(error) = stdin.write_all(data)
        {
            log::debug!(
                "[core_host_actions] failed to write job stdin: vfd={}, bytes={}, error={}",
                vfd,
                data.len(),
                error
            );
        }
    }

    fn stop(&mut self, core_bridge: &mut CoreBridge, job_id: i32) -> Result<(), HostActionError> {
        let Some(job) = self.jobs.get(&job_id) else {
            log::debug!(
                "[core_host_actions] ignoring stop for unknown job: job_id={}",
                job_id
            );
            return self.notify_status(core_bridge, job_id, JobStatus::Failed, -1);
        };
        if let Ok(mut child) = job.child.lock()
            && let Err(error) = child.kill()
        {
            log::debug!(
                "[core_host_actions] failed to kill job: job_id={}, error={}",
                job_id,
                error
            );
        }
        self.notify_status(core_bridge, job_id, JobStatus::Finished, -1)
    }

    fn drain_events(&mut self, core_bridge: &mut CoreBridge) -> Result<(), HostActionError> {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                HostJobEvent::Output { vfd, data } => {
                    core_bridge.inject_vfd_data(vfd, &data).map_err(|error| {
                        HostActionError::VfdInjectionRejected(format!("{error:?}"))
                    })?;
                }
                HostJobEvent::Exited { job_id, exit_code } => {
                    self.jobs.remove(&job_id);
                    self.notify_status(core_bridge, job_id, JobStatus::Finished, exit_code)?;
                }
                HostJobEvent::Failed { job_id, message } => {
                    self.jobs.remove(&job_id);
                    log::debug!(
                        "[core_host_actions] job worker failed: job_id={}, error={}",
                        job_id,
                        message
                    );
                    self.notify_status(core_bridge, job_id, JobStatus::Failed, -1)?;
                }
            }
        }
        Ok(())
    }

    fn notify_status(
        &mut self,
        core_bridge: &mut CoreBridge,
        job_id: i32,
        status: JobStatus,
        exit_code: i32,
    ) -> Result<(), HostActionError> {
        core_bridge
            .notify_job_status(job_id, status, exit_code)
            .map_err(|error| HostActionError::JobStatusRejected(format!("{error:?}")))
    }
}

fn spawn_reader_thread<R>(sender: Sender<HostJobEvent>, job_id: i32, vfd: i32, mut reader: R)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender
                        .send(HostJobEvent::Output {
                            vfd,
                            data: buffer[..count].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(HostJobEvent::Failed {
                        job_id,
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
    });
}

fn spawn_wait_thread(sender: Sender<HostJobEvent>, job_id: i32, child: Arc<Mutex<Child>>) {
    thread::spawn(move || {
        loop {
            let status = {
                let mut child = match child.lock() {
                    Ok(child) => child,
                    Err(error) => {
                        let _ = sender.send(HostJobEvent::Failed {
                            job_id,
                            message: error.to_string(),
                        });
                        return;
                    }
                };
                child.try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    let _ = sender.send(HostJobEvent::Exited { job_id, exit_code });
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = sender.send(HostJobEvent::Failed {
                        job_id,
                        message: error.to_string(),
                    });
                    return;
                }
            }
        }
    });
}
