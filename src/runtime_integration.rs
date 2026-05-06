use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use crate::callback_registry_seed::CallbackRegistrySeed;
use crate::presentation_effect::RuntimePresentationIntent;
use crate::runtime_message::runtime_callback_failure_message;
use crate::runtime_refresh::runtime_dispatch_requests_redraw;
use crate::saya_live_runtime::{
    HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeDispatchError, RuntimeDispatchReport, RuntimeEventPayload,
    RuntimeInitError, RuntimeMode, SayaLiveRuntime,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeDispatchOutcome {
    pub transient_message: Option<String>,
    pub requires_redraw: bool,
    pub shutdown_intent: Option<RuntimeShutdownIntent>,
    pub presentation_intents: Vec<RuntimePresentationIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeCommandEffect {
    pub transient_message: Option<String>,
    pub follow_up_events: Vec<RuntimeEventPayload>,
    pub shutdown_intent: Option<RuntimeShutdownIntent>,
    pub presentation_intents: Vec<RuntimePresentationIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeShutdownIntent {
    UserQuit,
    UserForceQuit,
}

pub trait RuntimeHostSession {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot;
    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot;
    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot;
    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError>;
}

pub struct RuntimeEventMapper;

impl RuntimeEventMapper {
    pub fn buffer_open(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferOpen(crate::saya_live_runtime::BufferEventPayload { buffer })
    }

    pub fn buffer_write_post(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferWritePost(crate::saya_live_runtime::BufferEventPayload {
            buffer,
        })
    }
}

pub struct RuntimeOutcomeProjector;

impl RuntimeOutcomeProjector {
    pub fn project_report(report: &RuntimeDispatchReport) -> RuntimeDispatchOutcome {
        RuntimeDispatchOutcome {
            transient_message: None,
            requires_redraw: runtime_dispatch_requests_redraw(report),
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    }

    pub fn project_error(error: &RuntimeDispatchError) -> RuntimeDispatchOutcome {
        let transient_message = runtime_callback_failure_message(error);
        RuntimeDispatchOutcome {
            requires_redraw: transient_message.is_some(),
            transient_message,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedRuntimeSnapshots {
    buffer: ReadonlyBufferSnapshot,
    window: ReadonlyWindowSnapshot,
    editor: ReadonlyEditorSnapshot,
}

impl Default for CachedRuntimeSnapshots {
    fn default() -> Self {
        Self {
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 1,
            },
            window: ReadonlyWindowSnapshot { id: 1 },
            editor: ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            },
        }
    }
}

struct RuntimeHostCommandRequest {
    name: String,
    reply: oneshot::Sender<Result<(), RuntimeCommandError>>,
}

struct ChannelBackedHostBridge {
    snapshots: Arc<Mutex<CachedRuntimeSnapshots>>,
    command_sender: mpsc::UnboundedSender<RuntimeHostCommandRequest>,
}

impl HostCapabilityBridge for ChannelBackedHostBridge {
    fn execute_host_command(
        &self,
        name: &str,
    ) -> crate::saya_live_runtime::BoxFuture<Result<(), RuntimeCommandError>> {
        let command_sender = self.command_sender.clone();
        let name = name.to_string();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            command_sender
                .send(RuntimeHostCommandRequest {
                    name: name.clone(),
                    reply,
                })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: name.clone(),
                    message: "host command channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name,
                    message: "host command reply channel closed".to_string(),
                })?
        })
    }

    fn current_buffer(&self) -> crate::saya_live_runtime::BoxFuture<ReadonlyBufferSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .buffer
                .clone()
        })
    }

    fn current_window(&self) -> crate::saya_live_runtime::BoxFuture<ReadonlyWindowSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .window
                .clone()
        })
    }

    fn current_editor(&self) -> crate::saya_live_runtime::BoxFuture<ReadonlyEditorSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .editor
                .clone()
        })
    }
}

pub struct RuntimeSessionOwner {
    runtime: SayaLiveRuntime,
    snapshots: Arc<Mutex<CachedRuntimeSnapshots>>,
    _command_sender: mpsc::UnboundedSender<RuntimeHostCommandRequest>,
    command_receiver: mpsc::UnboundedReceiver<RuntimeHostCommandRequest>,
}

impl RuntimeSessionOwner {
    pub fn spawn(seed: CallbackRegistrySeed) -> Result<Self, RuntimeInitError> {
        log::debug!(
            "[runtime_integration] spawning runtime session owner: commands={}, events={}",
            seed.commands().len(),
            seed.events().len()
        );
        let snapshots = Arc::new(Mutex::new(CachedRuntimeSnapshots::default()));
        let (command_sender, command_receiver) = mpsc::unbounded_channel();
        let bridge = Arc::new(ChannelBackedHostBridge {
            snapshots: snapshots.clone(),
            command_sender: command_sender.clone(),
        });
        let runtime = SayaLiveRuntime::spawn_from_seed(bridge, seed)?;
        Ok(Self {
            runtime,
            snapshots,
            _command_sender: command_sender,
            command_receiver,
        })
    }

    pub async fn dispatch<H: RuntimeHostSession>(
        &mut self,
        event: RuntimeEventPayload,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        let mut aggregate = RuntimeDispatchOutcome::default();
        let mut pending_events = VecDeque::from([event]);

        while let Some(next_event) = pending_events.pop_front() {
            let (projected, follow_up_events) = self.dispatch_once(next_event, host_session).await;
            merge_dispatch_outcome(&mut aggregate, projected);
            pending_events.extend(follow_up_events);
        }

        aggregate
    }

    pub async fn execute_command<H: RuntimeHostSession>(
        &mut self,
        name: &str,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        self.refresh_cached_snapshots(host_session);
        log::info!(
            "[runtime_integration][command] execute runtime command through session owner: command={}",
            name
        );

        let receipt = match self.runtime.execute_command(name) {
            Ok(receipt) => receipt,
            Err(error) => {
                log::debug!(
                    "[runtime_integration] failed to queue runtime command: command={}, error={:?}",
                    name,
                    error
                );
                return RuntimeDispatchOutcome {
                    transient_message: Some(format!("Runtime command failed: {:?}", error)),
                    requires_redraw: true,
                    shutdown_intent: None,
                    presentation_intents: Vec::new(),
                };
            }
        };

        let mut aggregate = self
            .await_runtime_command(name, receipt, host_session)
            .await;
        let follow_up_events = std::mem::take(&mut aggregate.follow_up_events);
        let mut dispatch_outcome = aggregate.outcome;
        for event in follow_up_events {
            merge_dispatch_outcome(
                &mut dispatch_outcome,
                self.dispatch(event, host_session).await,
            );
        }
        dispatch_outcome
    }

    async fn dispatch_once<H: RuntimeHostSession>(
        &mut self,
        event: RuntimeEventPayload,
        host_session: &mut H,
    ) -> (RuntimeDispatchOutcome, Vec<RuntimeEventPayload>) {
        self.refresh_cached_snapshots(host_session);
        log::debug!(
            "[runtime_integration] dispatch runtime event through session owner: event={:?}",
            event.event_name()
        );

        let receipt = match self.runtime.dispatch_event(event.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                log::debug!(
                    "[runtime_integration] failed to queue runtime event: event={:?}, error={:?}",
                    event.event_name(),
                    error
                );
                return (RuntimeOutcomeProjector::project_error(&error), Vec::new());
            }
        };

        let mut projected = RuntimeDispatchOutcome::default();
        let mut follow_up_events = Vec::new();
        let result_future = receipt.await_result();
        tokio::pin!(result_future);

        loop {
            tokio::select! {
                result = &mut result_future => {
                    match result {
                        Ok(report) => {
                            log::debug!(
                                "[runtime_integration] runtime dispatch completed: event={:?}, handler_count={}",
                                report.event,
                                report.handler_count
                            );
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeOutcomeProjector::project_report(&report),
                            );
                        }
                        Err(error) => {
                            log::debug!(
                                "[runtime_integration] runtime dispatch failed: event={:?}, error={:?}",
                                event.event_name(),
                                error
                            );
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeOutcomeProjector::project_error(&error),
                            );
                        }
                    }
                    break;
                }
                request = self.command_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] host command channel closed while dispatch was in flight");
                        break;
                    };
                    log::debug!(
                        "[runtime_integration] servicing runtime host command request: command={}",
                        request.name
                    );
                    match host_session.execute_host_command(&request.name) {
                        Ok(effect) => {
                            self.refresh_cached_snapshots(host_session);
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeDispatchOutcome {
                                    transient_message: effect.transient_message.clone(),
                                    requires_redraw: effect.transient_message.is_some()
                                        || !effect.presentation_intents.is_empty(),
                                    shutdown_intent: effect.shutdown_intent,
                                    presentation_intents: effect.presentation_intents.clone(),
                                },
                            );
                            follow_up_events.extend(effect.follow_up_events.clone());
                            let _ = request.reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
            }
        }

        (projected, follow_up_events)
    }

    fn refresh_cached_snapshots<H: RuntimeHostSession>(&self, host_session: &mut H) {
        let buffer = host_session.current_buffer_snapshot();
        let window = host_session.current_window_snapshot();
        let editor = host_session.current_editor_snapshot();
        log::debug!(
            "[runtime_integration] refreshing cached runtime snapshots: buffer_id={}, window_id={}, mode={:?}",
            buffer.id,
            window.id,
            editor.mode
        );
        *self
            .snapshots
            .lock()
            .expect("runtime snapshots mutex should not poison") = CachedRuntimeSnapshots {
            buffer,
            window,
            editor,
        };
    }

    async fn await_runtime_command<H: RuntimeHostSession>(
        &mut self,
        name: &str,
        receipt: crate::saya_live_runtime::RuntimeCommandReceipt,
        host_session: &mut H,
    ) -> RuntimeCommandDispatchAggregate {
        let mut projected = RuntimeDispatchOutcome::default();
        let mut follow_up_events = Vec::new();
        let result_future = receipt.await_result();
        tokio::pin!(result_future);

        loop {
            tokio::select! {
                result = &mut result_future => {
                    match result {
                        Ok(()) => {
                            log::debug!(
                                "[runtime_integration] runtime command completed: command={}",
                                name
                            );
                        }
                        Err(error) => {
                            log::debug!(
                                "[runtime_integration] runtime command failed: command={}, error={:?}",
                                name,
                                error
                            );
                            projected.transient_message = Some(format!("Runtime command failed: {:?}", error));
                            projected.requires_redraw = true;
                        }
                    }
                    break;
                }
                request = self.command_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] host command channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][host_command] servicing runtime host command request during command execution: command={}",
                        request.name
                    );
                    match host_session.execute_host_command(&request.name) {
                        Ok(effect) => {
                            self.refresh_cached_snapshots(host_session);
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeDispatchOutcome {
                                    transient_message: effect.transient_message.clone(),
                                    requires_redraw: effect.transient_message.is_some()
                                        || !effect.presentation_intents.is_empty(),
                                    shutdown_intent: effect.shutdown_intent,
                                    presentation_intents: effect.presentation_intents.clone(),
                                },
                            );
                            follow_up_events.extend(effect.follow_up_events.clone());
                            let _ = request.reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
            }
        }

        RuntimeCommandDispatchAggregate {
            outcome: projected,
            follow_up_events,
        }
    }
}

struct RuntimeCommandDispatchAggregate {
    outcome: RuntimeDispatchOutcome,
    follow_up_events: Vec<RuntimeEventPayload>,
}

fn merge_dispatch_outcome(target: &mut RuntimeDispatchOutcome, next: RuntimeDispatchOutcome) {
    if next.transient_message.is_some() {
        target.transient_message = next.transient_message;
    }
    target.requires_redraw |= next.requires_redraw;
    merge_shutdown_intent(&mut target.shutdown_intent, next.shutdown_intent);
    target
        .presentation_intents
        .extend(next.presentation_intents);
}

fn merge_shutdown_intent(
    target: &mut Option<RuntimeShutdownIntent>,
    next: Option<RuntimeShutdownIntent>,
) {
    match (target.as_ref().copied(), next) {
        (None, Some(intent)) => *target = Some(intent),
        (Some(RuntimeShutdownIntent::UserQuit), Some(RuntimeShutdownIntent::UserForceQuit)) => {
            *target = Some(RuntimeShutdownIntent::UserForceQuit);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saya_live_runtime::{RuntimeCallbackError, RuntimeEventName};

    #[test]
    fn runtime_outcome_projector_requests_redraw_for_callback_failure() {
        let projected =
            RuntimeOutcomeProjector::project_error(&RuntimeDispatchError::CallbackFailed {
                event: RuntimeEventName::BufferOpen,
                handler_index: 0,
                error: RuntimeCallbackError::ScriptFailed {
                    message: "boom".to_string(),
                },
            });

        assert_eq!(
            projected,
            RuntimeDispatchOutcome {
                transient_message: Some(
                    "Runtime callback failed on bufferOpen handler 0: script error: boom"
                        .to_string()
                ),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            }
        );
    }
}
