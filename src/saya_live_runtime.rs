use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

use deno_core::{JsRuntime, OpState, RuntimeOptions, op2};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::callback_registry_seed::CallbackRegistrySeed;
#[cfg(test)]
use crate::startup_runtime::{
    PreparedStartupModule, StartupModulePrepareResult, prepare_init_module,
};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

const RUNTIME_COMMAND_ERROR_PREFIX: &str = "__SAYA_RUNTIME_COMMAND_ERROR__";
const RUNTIME_CALLBACK_ERROR_PREFIX: &str = "__SAYA_RUNTIME_CALLBACK_ERROR__";
const RUNTIME_PUBLIC_SURFACE_PATHS: &[&str] = &[
    "saya.commands.execute",
    "saya.buffer.current",
    "saya.window.current",
    "saya.editor.current",
    "saya.editor.mode",
];

/// Formal runtime surface は read-only/command 実行に限定し、compat 文字列 DSL は含めない。
pub fn runtime_public_surface_paths() -> &'static [&'static str] {
    RUNTIME_PUBLIC_SURFACE_PATHS
}

const LIVE_RUNTIME_BOOTSTRAP: &str = r#"
const commandErrorPrefix = "__SAYA_RUNTIME_COMMAND_ERROR__";
const callbackErrorPrefix = "__SAYA_RUNTIME_CALLBACK_ERROR__";

function runtimeErrorMessage(error) {
    if (error instanceof Error) {
        return error.message;
    }
    return String(error);
}

globalThis.__sayaRuntime = {
    commands: new Map(),
    events: new Map(),
    commandStack: [],
    registerCommand(name, callback) {
        this.commands.set(String(name), callback);
    },
    registerEvent(name, callback) {
        const normalized = String(name);
        const handlers = this.events.get(normalized) ?? [];
        handlers.push(callback);
        this.events.set(normalized, handlers);
    },
    async executeCommand(name) {
        const normalized = String(name);
        if (this.commandStack.includes(normalized)) {
            throw `${commandErrorPrefix}${JSON.stringify({ "CircularCommand": { name: normalized } })}`;
        }

        this.commandStack.push(normalized);
        try {
            const callback = this.commands.get(normalized);
            if (callback) {
                return await callback();
            }
            return await Deno.core.ops.op_runtime_execute_host_command(normalized);
        } finally {
            this.commandStack.pop();
        }
    },
    async dispatchEvent(name, payload) {
        const handlers = this.events.get(String(name)) ?? [];
        for (let index = 0; index < handlers.length; index += 1) {
            try {
                await handlers[index](payload);
            } catch (error) {
                throw `${callbackErrorPrefix}${JSON.stringify({
                    handlerIndex: index,
                    error: runtimeErrorMessage(error),
                })}`;
            }
        }
    },
};

globalThis.saya = {
    commands: {
        execute(name) {
            return globalThis.__sayaRuntime.executeCommand(String(name));
        },
    },
    buffer: {
        current() {
            return Deno.core.ops.op_runtime_current_buffer();
        },
    },
    window: {
        current() {
            return Deno.core.ops.op_runtime_current_window();
        },
    },
    editor: {
        current() {
            return Deno.core.ops.op_runtime_current_editor();
        },
        async mode() {
            const editor = await Deno.core.ops.op_runtime_current_editor();
            return editor.mode;
        },
    },
};

Object.freeze(globalThis.saya.commands);
Object.freeze(globalThis.saya.buffer);
Object.freeze(globalThis.saya.window);
Object.freeze(globalThis.saya.editor);
Object.freeze(globalThis.saya);
"#;

const RUNTIME_PUBLIC_SURFACE_NAMES: &[&str] = &["commands", "buffer", "window", "editor"];
const RUNTIME_FORBIDDEN_SURFACE_NAMES: &[&str] = &["filesystem", "network"];

pub const RUNTIME_SAYA_TYPE_DECLARATION: &str = r#"
declare global {
    type SayaRuntimeMode = "Normal" | "Insert" | "Visual";

    interface SayaReadonlyBufferSnapshot {
        id: number;
        path: string | null;
        lineCount: number;
    }

    interface SayaReadonlyWindowSnapshot {
        id: number;
    }

    interface SayaReadonlyEditorSnapshot {
        mode: SayaRuntimeMode;
    }

    interface SayaRuntimeCommandsSurface {
        execute(name: string): Promise<unknown>;
    }

    interface SayaRuntimeBufferSurface {
        current(): Promise<SayaReadonlyBufferSnapshot>;
    }

    interface SayaRuntimeWindowSurface {
        current(): Promise<SayaReadonlyWindowSnapshot>;
    }

    interface SayaRuntimeEditorSurface {
        current(): Promise<SayaReadonlyEditorSnapshot>;
        mode(): Promise<SayaRuntimeMode>;
    }

    interface SayaRuntimeSurface {
        commands: SayaRuntimeCommandsSurface;
        buffer: SayaRuntimeBufferSurface;
        window: SayaRuntimeWindowSurface;
        editor: SayaRuntimeEditorSurface;
    }

    var saya: SayaRuntimeSurface;
}

export {};
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadonlyBufferSnapshot {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub line_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadonlyWindowSnapshot {
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadonlyEditorSnapshot {
    pub mode: RuntimeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    Normal,
    Insert,
    Visual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferEventPayload {
    pub buffer: ReadonlyBufferSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeEventName {
    BufferOpen,
    BufferWritePost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEventPayload {
    BufferOpen(BufferEventPayload),
    BufferWritePost(BufferEventPayload),
}

impl RuntimeEventPayload {
    pub fn event_name(&self) -> RuntimeEventName {
        match self {
            Self::BufferOpen(_) => RuntimeEventName::BufferOpen,
            Self::BufferWritePost(_) => RuntimeEventName::BufferWritePost,
        }
    }

    pub fn buffer_payload(&self) -> &BufferEventPayload {
        match self {
            Self::BufferOpen(payload) | Self::BufferWritePost(payload) => payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeCommandError {
    UnknownCommand { name: String },
    CommandFailed { name: String, message: String },
    CircularCommand { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCallbackError {
    Command(RuntimeCommandError),
    ScriptFailed { message: String },
}

impl From<RuntimeCommandError> for RuntimeCallbackError {
    fn from(value: RuntimeCommandError) -> Self {
        Self::Command(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDispatchError {
    QueueClosed,
    WorkerStopped,
    CallbackFailed {
        event: RuntimeEventName,
        handler_index: usize,
        error: RuntimeCallbackError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDispatchReport {
    pub event: RuntimeEventName,
    pub handler_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInitError {
    WorkerStartFailed { message: String },
    UnsupportedEvent { name: String },
    BootstrapFailed { message: String },
}

pub trait HostCapabilityBridge: Send + Sync + 'static {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>>;
    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot>;
    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot>;
    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot>;
}

/// runtime phase の正式な `saya` 公開面を返す。
pub fn runtime_public_surface_names() -> &'static [&'static str] {
    RUNTIME_PUBLIC_SURFACE_NAMES
}

/// MVP から除外する危険な capability 名を返す。
pub fn runtime_forbidden_surface_names() -> &'static [&'static str] {
    RUNTIME_FORBIDDEN_SURFACE_NAMES
}

type CommandCallback =
    Arc<dyn Fn(RuntimeContext) -> BoxFuture<Result<(), RuntimeCommandError>> + Send + Sync>;
type EventCallback = Arc<
    dyn Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
        + Send
        + Sync,
>;

#[derive(Clone, Default)]
pub struct CallbackRegistryBuilder {
    commands: Vec<(String, CommandCallback)>,
    buffer_open_handlers: Vec<EventCallback>,
    buffer_write_post_handlers: Vec<EventCallback>,
}

impl CallbackRegistryBuilder {
    pub fn register_command<F>(&mut self, name: &str, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext) -> BoxFuture<Result<(), RuntimeCommandError>> + Send + Sync + 'static,
    {
        log::debug!("[saya_live_runtime] register command: {}", name);
        self.commands.push((name.to_string(), Arc::new(callback)));
        self
    }

    pub fn on_buffer_open<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferOpen handler");
        self.buffer_open_handlers.push(Arc::new(callback));
        self
    }

    pub fn on_buffer_write_post<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferWritePost handler");
        self.buffer_write_post_handlers.push(Arc::new(callback));
        self
    }

    pub fn build(&self) -> CallbackRegistry {
        log::debug!(
            "[saya_live_runtime] build callback registry: commands={}, buffer_open_handlers={}, buffer_write_post_handlers={}",
            self.commands.len(),
            self.buffer_open_handlers.len(),
            self.buffer_write_post_handlers.len()
        );

        let mut commands = HashMap::new();
        for (name, callback) in &self.commands {
            commands.insert(name.clone(), callback.clone());
        }

        CallbackRegistry {
            commands,
            handlers: HashMap::from([
                (
                    RuntimeEventName::BufferOpen,
                    self.buffer_open_handlers.clone(),
                ),
                (
                    RuntimeEventName::BufferWritePost,
                    self.buffer_write_post_handlers.clone(),
                ),
            ]),
        }
    }
}

#[derive(Clone)]
pub struct CallbackRegistry {
    commands: HashMap<String, CommandCallback>,
    handlers: HashMap<RuntimeEventName, Vec<EventCallback>>,
}

impl CallbackRegistry {
    fn command(&self, name: &str) -> Option<CommandCallback> {
        self.commands.get(name).cloned()
    }

    fn handlers_for(&self, event: RuntimeEventName) -> Vec<EventCallback> {
        self.handlers.get(&event).cloned().unwrap_or_default()
    }
}

#[derive(Clone)]
struct LiveRuntimeOpState {
    bridge: Arc<dyn HostCapabilityBridge>,
}

#[derive(Debug, Default)]
struct SeedRuntimeMetadata {
    handler_counts: HashMap<RuntimeEventName, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncodedCallbackFailure {
    handler_index: usize,
    error: String,
}

#[op2(async(deferred), fast)]
async fn op_runtime_execute_host_command(
    state: Rc<RefCell<OpState>>,
    #[string] name: String,
) -> Result<(), JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime] runtime op execute host command: name={}",
        name
    );
    bridge
        .execute_host_command(&name)
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_buffer(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyBufferSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_buffer");
    Ok(bridge.current_buffer().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_window(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyWindowSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_window");
    Ok(bridge.current_window().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_editor(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyEditorSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_editor");
    Ok(bridge.current_editor().await)
}

deno_core::extension!(
    live_saya_extension,
    ops = [
        op_runtime_execute_host_command,
        op_runtime_current_buffer,
        op_runtime_current_window,
        op_runtime_current_editor
    ],
    options = {
        bridge: Arc<dyn HostCapabilityBridge>,
    },
    state = |state, options| {
        state.put(LiveRuntimeOpState {
            bridge: options.bridge,
        });
    }
);

fn runtime_command_error_to_js_error(error: RuntimeCommandError) -> JsErrorBox {
    let encoded = serde_json::to_string(&error).expect("runtime command error should serialize");
    JsErrorBox::generic(format!("{RUNTIME_COMMAND_ERROR_PREFIX}{encoded}"))
}

fn runtime_callback_error_from_script_message(message: &str) -> RuntimeCallbackError {
    if let Some(payload) = extract_prefixed_json_payload(message, RUNTIME_COMMAND_ERROR_PREFIX) {
        if let Ok(error) = serde_json::from_str::<RuntimeCommandError>(payload) {
            return RuntimeCallbackError::Command(error);
        }
    }

    RuntimeCallbackError::ScriptFailed {
        message: message.to_string(),
    }
}

fn runtime_event_name_from_seed(name: &str) -> Result<RuntimeEventName, RuntimeInitError> {
    match name {
        "bufferOpen" => Ok(RuntimeEventName::BufferOpen),
        "bufferWritePost" => Ok(RuntimeEventName::BufferWritePost),
        other => Err(RuntimeInitError::UnsupportedEvent {
            name: other.to_string(),
        }),
    }
}

fn runtime_event_name_to_script(event: RuntimeEventName) -> &'static str {
    match event {
        RuntimeEventName::BufferOpen => "bufferOpen",
        RuntimeEventName::BufferWritePost => "bufferWritePost",
    }
}

fn callback_expression(source: &str, default_params: &str) -> String {
    let trimmed = source.trim().trim_end_matches(';').trim();
    let looks_like_function = trimmed.contains("=>")
        || trimmed.starts_with("function")
        || trimmed.starts_with("async function")
        || trimmed.starts_with("async (")
        || trimmed.starts_with('(');

    if looks_like_function {
        format!("({trimmed})")
    } else if default_params.is_empty() {
        format!("(async () => {{ {trimmed} }})")
    } else {
        format!("(async ({default_params}) => {{ {trimmed} }})")
    }
}

fn build_seed_registration_script(
    seed: &CallbackRegistrySeed,
) -> Result<(String, SeedRuntimeMetadata), RuntimeInitError> {
    let mut script = String::from("\"use strict\";\n");
    let mut metadata = SeedRuntimeMetadata::default();

    for command in seed.commands() {
        let name = serde_json::to_string(command.name()).expect("command name should serialize");
        let callback = callback_expression(command.callback_source(), "");
        script.push_str(&format!(
            "globalThis.__sayaRuntime.registerCommand({name}, {callback});\n"
        ));
    }

    for event in seed.events() {
        let event_name = runtime_event_name_from_seed(event.name())?;
        let name = serde_json::to_string(event.name()).expect("event name should serialize");
        let callback = callback_expression(event.callback_source(), "payload");
        script.push_str(&format!(
            "globalThis.__sayaRuntime.registerEvent({name}, {callback});\n"
        ));
        *metadata.handler_counts.entry(event_name).or_insert(0) += 1;
    }

    Ok((script, metadata))
}

fn create_seed_runtime(
    bridge: Arc<dyn HostCapabilityBridge>,
    seed: &CallbackRegistrySeed,
) -> Result<(JsRuntime, SeedRuntimeMetadata), RuntimeInitError> {
    log::debug!(
        "[saya_live_runtime] create deno_core live runtime from seed: commands={}, events={}",
        seed.commands().len(),
        seed.events().len()
    );
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![live_saya_extension::init(bridge)],
        ..Default::default()
    });

    runtime
        .execute_script("<saya-live-runtime-bootstrap>", LIVE_RUNTIME_BOOTSTRAP)
        .map_err(|error| RuntimeInitError::BootstrapFailed {
            message: error.to_string(),
        })?;

    let (registration_script, metadata) = build_seed_registration_script(seed)?;
    runtime
        .execute_script("<saya-live-runtime-seed>", registration_script)
        .map_err(|error| RuntimeInitError::BootstrapFailed {
            message: error.to_string(),
        })?;

    Ok((runtime, metadata))
}

async fn dispatch_event_in_seed_runtime(
    runtime: &mut JsRuntime,
    event: RuntimeEventPayload,
) -> Result<(), RuntimeDispatchError> {
    let event_name = event.event_name();
    let payload_json = serde_json::to_string(event.buffer_payload())
        .expect("buffer event payload should serialize");
    let event_name_json = serde_json::to_string(runtime_event_name_to_script(event_name))
        .expect("runtime event name should serialize");
    let script = format!(
        "(async () => {{ await globalThis.__sayaRuntime.dispatchEvent({event_name_json}, {payload_json}); }})()"
    );

    log::debug!(
        "[saya_live_runtime] execute seed runtime dispatch script: event={:?}",
        event_name
    );
    let promise = runtime
        .execute_script("<saya-live-runtime-dispatch>", script)
        .map_err(|error| parse_runtime_dispatch_error(event_name, error.to_string()))?;
    #[allow(deprecated)]
    runtime
        .resolve_value(promise)
        .await
        .map_err(|error| parse_runtime_dispatch_error(event_name, error.to_string()))?;
    Ok(())
}

fn parse_runtime_dispatch_error(event: RuntimeEventName, message: String) -> RuntimeDispatchError {
    if let Some(payload) = extract_prefixed_json_payload(&message, RUNTIME_CALLBACK_ERROR_PREFIX) {
        if let Ok(encoded) = serde_json::from_str::<EncodedCallbackFailure>(payload) {
            return RuntimeDispatchError::CallbackFailed {
                event,
                handler_index: encoded.handler_index,
                error: runtime_callback_error_from_script_message(&encoded.error),
            };
        }
    }

    RuntimeDispatchError::CallbackFailed {
        event,
        handler_index: 0,
        error: RuntimeCallbackError::ScriptFailed { message },
    }
}

fn extract_prefixed_json_payload<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let payload = message.split_once(prefix).map(|(_, rhs)| rhs)?;
    let trimmed = payload.trim_start();
    let opening = trimmed.as_bytes().first().copied()?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;

    for (index, byte) in trimmed.bytes().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match byte {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            value if value == opening => depth += 1,
            value if value == closing => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&trimmed[..=index]);
                }
            }
            _ => {}
        }
    }

    None
}

pub trait SayaStartupPhaseEvaluator: Send + Sync + 'static {
    type Output: Send + 'static;
    type Error: Send + 'static;

    fn evaluate(&self) -> BoxFuture<Result<Self::Output, Self::Error>>;
}

#[cfg(test)]
mod startup_runtime_prepare_test_support {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum StartupRuntimePrepareError {
        ReadFailed { path: PathBuf, message: String },
        TranspileFailed { path: PathBuf, message: String },
    }

    pub(super) struct StartupRuntimePrepareEvaluator {
        config_path: PathBuf,
        current_dir: PathBuf,
    }

    impl SayaStartupPhaseEvaluator for StartupRuntimePrepareEvaluator {
        type Output = PreparedStartupModule;
        type Error = StartupRuntimePrepareError;

        fn evaluate(&self) -> BoxFuture<Result<Self::Output, Self::Error>> {
            let config_path = self.config_path.clone();
            let current_dir = self.current_dir.clone();
            Box::pin(async move {
                log::debug!(
                    "[saya_live_runtime] prepare startup runtime on worker: config_path={}, current_dir={}",
                    config_path.display(),
                    current_dir.display()
                );
                match prepare_init_module(&config_path, &current_dir) {
                    StartupModulePrepareResult::Success(module) => Ok(module),
                    StartupModulePrepareResult::ReadFailed { path, message } => {
                        Err(StartupRuntimePrepareError::ReadFailed { path, message })
                    }
                    StartupModulePrepareResult::TranspileFailed { path, message } => {
                        Err(StartupRuntimePrepareError::TranspileFailed { path, message })
                    }
                }
            })
        }
    }

    pub(super) fn spawn_startup_runtime_prepare_runner(
        config_path: PathBuf,
        current_dir: PathBuf,
    ) -> SayaStartupPhaseRunner<StartupRuntimePrepareEvaluator> {
        log::debug!(
            "[saya_live_runtime] spawn startup runtime prepare runner: config_path={}, current_dir={}",
            config_path.display(),
            current_dir.display()
        );
        SayaStartupPhaseRunner::new(Arc::new(StartupRuntimePrepareEvaluator {
            config_path,
            current_dir,
        }))
    }
}

#[cfg(test)]
use startup_runtime_prepare_test_support::spawn_startup_runtime_prepare_runner;

pub struct SayaStartupPhaseRunner<E>
where
    E: SayaStartupPhaseEvaluator,
{
    sender: mpsc::UnboundedSender<oneshot::Sender<Result<E::Output, E::Error>>>,
    _worker: JoinHandle<()>,
}

impl<E> SayaStartupPhaseRunner<E>
where
    E: SayaStartupPhaseEvaluator,
{
    pub fn new(evaluator: Arc<E>) -> Self {
        let (sender, mut receiver) =
            mpsc::unbounded_channel::<oneshot::Sender<Result<E::Output, E::Error>>>();

        let worker = tokio::spawn(async move {
            log::debug!("[saya_live_runtime] startup worker spawned");
            while let Some(reply) = receiver.recv().await {
                log::debug!("[saya_live_runtime] startup worker evaluating request");
                let result = evaluator.evaluate().await;
                let _ = reply.send(result);
            }
            log::debug!("[saya_live_runtime] startup worker stopped");
        });

        Self {
            sender,
            _worker: worker,
        }
    }

    pub fn begin(&self) -> Result<StartupPhaseReceipt<E::Output, E::Error>, StartupPhaseError> {
        log::debug!("[saya_live_runtime] queue startup evaluation");
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(sender)
            .map_err(|_| StartupPhaseError::QueueClosed)?;
        Ok(StartupPhaseReceipt { receiver })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPhaseError {
    QueueClosed,
    WorkerStopped,
}

pub struct StartupPhaseReceipt<T, E> {
    receiver: oneshot::Receiver<Result<T, E>>,
}

impl<T, E> StartupPhaseReceipt<T, E> {
    pub async fn await_result(self) -> Result<T, StartupPhaseError> {
        self.receiver
            .await
            .map_err(|_| StartupPhaseError::WorkerStopped)?
            .map_err(|_| StartupPhaseError::WorkerStopped)
    }
}

#[derive(Clone)]
pub struct RuntimeContext {
    shared: Arc<RuntimeSharedState>,
}

impl RuntimeContext {
    pub fn commands(&self) -> RuntimeCommandsApi {
        RuntimeCommandsApi {
            shared: self.shared.clone(),
        }
    }

    pub fn buffer(&self) -> RuntimeBufferApi {
        RuntimeBufferApi {
            bridge: self.shared.bridge.clone(),
        }
    }

    pub fn window(&self) -> RuntimeWindowApi {
        RuntimeWindowApi {
            bridge: self.shared.bridge.clone(),
        }
    }

    pub fn editor(&self) -> RuntimeEditorApi {
        RuntimeEditorApi {
            bridge: self.shared.bridge.clone(),
        }
    }
}

pub struct RuntimeCommandsApi {
    shared: Arc<RuntimeSharedState>,
}

impl RuntimeCommandsApi {
    pub async fn execute(&self, name: &str) -> Result<(), RuntimeCommandError> {
        log::debug!("[saya_live_runtime] execute command requested: {}", name);

        {
            let mut stack = self.shared.command_stack.lock().await;
            if stack.iter().any(|entry| entry == name) {
                log::debug!("[saya_live_runtime] circular command detected: {}", name);
                return Err(RuntimeCommandError::CircularCommand {
                    name: name.to_string(),
                });
            }
            stack.push(name.to_string());
        }

        let result = if let Some(callback) = self.shared.registry.command(name) {
            log::debug!("[saya_live_runtime] execute registered command: {}", name);
            callback(RuntimeContext {
                shared: self.shared.clone(),
            })
            .await
        } else {
            log::debug!(
                "[saya_live_runtime] execute host command fallback: {}",
                name
            );
            self.shared.bridge.execute_host_command(name).await
        };

        let mut stack = self.shared.command_stack.lock().await;
        if let Some(position) = stack.iter().rposition(|entry| entry == name) {
            stack.remove(position);
        }

        result
    }
}

pub struct RuntimeBufferApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeBufferApi {
    pub async fn current(&self) -> ReadonlyBufferSnapshot {
        self.bridge.current_buffer().await
    }
}

pub struct RuntimeWindowApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeWindowApi {
    pub async fn current(&self) -> ReadonlyWindowSnapshot {
        self.bridge.current_window().await
    }
}

pub struct RuntimeEditorApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeEditorApi {
    pub async fn current(&self) -> ReadonlyEditorSnapshot {
        self.bridge.current_editor().await
    }

    pub async fn mode(&self) -> RuntimeMode {
        self.current().await.mode
    }
}

struct RuntimeSharedState {
    bridge: Arc<dyn HostCapabilityBridge>,
    registry: CallbackRegistry,
    command_stack: Mutex<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeWorkerKind {
    Tokio,
    Thread,
}

pub struct SayaLiveRuntime {
    sender: mpsc::UnboundedSender<RuntimeMessage>,
    worker_kind: RuntimeWorkerKind,
}

impl Drop for SayaLiveRuntime {
    fn drop(&mut self) {
        match self.worker_kind {
            RuntimeWorkerKind::Tokio => {
                log::debug!("[saya_live_runtime] drop live runtime backed by tokio worker");
            }
            RuntimeWorkerKind::Thread => {
                log::debug!("[saya_live_runtime] drop live runtime backed by dedicated thread");
            }
        }
    }
}

impl SayaLiveRuntime {
    pub fn spawn(bridge: Arc<dyn HostCapabilityBridge>, registry: CallbackRegistry) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let shared = Arc::new(RuntimeSharedState {
            bridge,
            registry,
            command_stack: Mutex::new(Vec::new()),
        });

        let worker_shared = shared.clone();
        tokio::spawn(async move {
            log::debug!("[saya_live_runtime] live runtime worker spawned");
            while let Some(message) = receiver.recv().await {
                match message {
                    RuntimeMessage::Dispatch { event, reply } => {
                        let result = dispatch_event(worker_shared.clone(), event).await;
                        let _ = reply.send(result);
                    }
                }
            }
            log::debug!("[saya_live_runtime] live runtime worker stopped");
        });

        Self {
            sender,
            worker_kind: RuntimeWorkerKind::Tokio,
        }
    }

    pub fn spawn_from_seed(
        bridge: Arc<dyn HostCapabilityBridge>,
        seed: CallbackRegistrySeed,
    ) -> Result<Self, RuntimeInitError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (init_sender, init_receiver) = std::sync::mpsc::sync_channel(1);

        let worker = thread::Builder::new()
            .name("saya-live-runtime".to_string())
            .spawn(move || {
                log::debug!("[saya_live_runtime] spawn seed-backed live runtime thread");
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("seed runtime thread should create tokio runtime");

                runtime.block_on(async move {
                    let mut receiver = receiver;
                    let (mut js_runtime, metadata) = match create_seed_runtime(bridge, &seed) {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            let _ = init_sender.send(Err(error));
                            return;
                        }
                    };
                    let _ = init_sender.send(Ok(()));
                    log::debug!("[saya_live_runtime] seed-backed live runtime initialized");

                    while let Some(message) = receiver.recv().await {
                        match message {
                            RuntimeMessage::Dispatch { event, reply } => {
                                let event_name = event.event_name();
                                log::debug!(
                                    "[saya_live_runtime] seed runtime received dispatch: event={:?}",
                                    event_name
                                );
                                let result =
                                    dispatch_event_in_seed_runtime(&mut js_runtime, event).await;
                                let report = result.map(|()| RuntimeDispatchReport {
                                    event: event_name,
                                    handler_count: metadata
                                        .handler_counts
                                        .get(&event_name)
                                        .copied()
                                        .unwrap_or_default(),
                                });
                                let _ = reply.send(report);
                            }
                        }
                    }

                    log::debug!("[saya_live_runtime] seed-backed live runtime stopped");
                });
            })
            .map_err(|error| RuntimeInitError::WorkerStartFailed {
                message: error.to_string(),
            })?;

        match init_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender,
                worker_kind: RuntimeWorkerKind::Thread,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = worker.join();
                Err(RuntimeInitError::WorkerStartFailed {
                    message: error.to_string(),
                })
            }
        }
    }

    pub fn dispatch_event(
        &self,
        event: RuntimeEventPayload,
    ) -> Result<RuntimeDispatchReceipt, RuntimeDispatchError> {
        log::debug!(
            "[saya_live_runtime] queue dispatch: event={:?}",
            event.event_name()
        );
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(RuntimeMessage::Dispatch { event, reply })
            .map_err(|_| RuntimeDispatchError::QueueClosed)?;
        Ok(RuntimeDispatchReceipt { receiver })
    }
}

pub struct RuntimeDispatchReceipt {
    receiver: oneshot::Receiver<Result<RuntimeDispatchReport, RuntimeDispatchError>>,
}

impl RuntimeDispatchReceipt {
    pub async fn await_result(self) -> Result<RuntimeDispatchReport, RuntimeDispatchError> {
        self.receiver
            .await
            .map_err(|_| RuntimeDispatchError::WorkerStopped)?
    }
}

enum RuntimeMessage {
    Dispatch {
        event: RuntimeEventPayload,
        reply: oneshot::Sender<Result<RuntimeDispatchReport, RuntimeDispatchError>>,
    },
}

async fn dispatch_event(
    shared: Arc<RuntimeSharedState>,
    event: RuntimeEventPayload,
) -> Result<RuntimeDispatchReport, RuntimeDispatchError> {
    let event_name = event.event_name();
    let payload = event.buffer_payload().clone();
    let handlers = shared.registry.handlers_for(event_name);

    log::debug!(
        "[saya_live_runtime] dispatch start: event={:?}, handler_count={}",
        event_name,
        handlers.len()
    );

    for (index, handler) in handlers.iter().enumerate() {
        log::debug!(
            "[saya_live_runtime] dispatch handler: event={:?}, index={}",
            event_name,
            index
        );
        handler(
            RuntimeContext {
                shared: shared.clone(),
            },
            payload.clone(),
        )
        .await
        .map_err(|error| RuntimeDispatchError::CallbackFailed {
            event: event_name,
            handler_index: index,
            error,
        })?;
    }

    Ok(RuntimeDispatchReport {
        event: event_name,
        handler_count: handlers.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::callback_registry_seed::CallbackRegistrySeed;
    use crate::config_runtime::StartupRegistryEntry;
    use crate::startup_runtime::PreparedStartupModule;

    use tokio::sync::Mutex;

    use super::{
        BufferEventPayload, CallbackRegistryBuilder, HostCapabilityBridge, ReadonlyBufferSnapshot,
        ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
        RuntimeMode, SayaLiveRuntime, SayaStartupPhaseEvaluator, SayaStartupPhaseRunner,
        spawn_startup_runtime_prepare_runner,
    };

    fn unique_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-live-runtime-{name}-{nanos}"))
    }

    struct RecordingHostBridge {
        executed_commands: Arc<Mutex<Vec<String>>>,
        command_results: HashMap<String, RuntimeCommandError>,
    }

    impl RecordingHostBridge {
        fn new() -> Self {
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
                command_results: HashMap::new(),
            }
        }

        fn with_command_error(name: &str, error: RuntimeCommandError) -> Self {
            let mut command_results = HashMap::new();
            command_results.insert(name.to_string(), error);
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
                command_results,
            }
        }
    }

    impl HostCapabilityBridge for RecordingHostBridge {
        fn execute_host_command(
            &self,
            name: &str,
        ) -> super::BoxFuture<Result<(), RuntimeCommandError>> {
            let executed_commands = self.executed_commands.clone();
            let name = name.to_string();
            let result = self.command_results.get(&name).cloned();
            Box::pin(async move {
                if let Some(error) = result {
                    return Err(error);
                }
                executed_commands.lock().await.push(name);
                Ok(())
            })
        }

        fn current_buffer(&self) -> super::BoxFuture<ReadonlyBufferSnapshot> {
            Box::pin(async move {
                ReadonlyBufferSnapshot {
                    id: 7,
                    path: Some(PathBuf::from("notes.md")),
                    line_count: 3,
                }
            })
        }

        fn current_window(&self) -> super::BoxFuture<ReadonlyWindowSnapshot> {
            Box::pin(async move { ReadonlyWindowSnapshot { id: 9 } })
        }

        fn current_editor(&self) -> super::BoxFuture<ReadonlyEditorSnapshot> {
            Box::pin(async move {
                ReadonlyEditorSnapshot {
                    mode: RuntimeMode::Normal,
                }
            })
        }
    }

    struct SleepingStartupEvaluator;

    impl SayaStartupPhaseEvaluator for SleepingStartupEvaluator {
        type Output = &'static str;
        type Error = &'static str;

        fn evaluate(&self) -> super::BoxFuture<Result<Self::Output, Self::Error>> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                Ok("startup-ready")
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_phase_runs_on_worker_boundary_without_blocking_caller() {
        let runner = SayaStartupPhaseRunner::new(Arc::new(SleepingStartupEvaluator));

        let started_at = Instant::now();
        let receipt = runner.begin().expect("startup evaluation should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "begin should return quickly without waiting for startup evaluation"
        );

        let result = receipt
            .await_result()
            .await
            .expect("startup evaluation result");
        assert_eq!(result, "startup-ready");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_runtime_prepare_runs_on_worker_boundary_without_blocking_caller() {
        let current_dir = unique_path("startup-runtime-cwd");
        std::fs::create_dir_all(&current_dir).expect("current dir");
        let config_path = current_dir.join("init.ts");
        std::fs::write(
            &config_path,
            r#"
                const tabSize: number = 4;
                saya.options.tabSize = tabSize;
            "#,
        )
        .expect("config file");

        let runner = spawn_startup_runtime_prepare_runner(config_path.clone(), current_dir.clone());

        let started_at = Instant::now();
        let receipt = runner
            .begin()
            .expect("startup runtime evaluation should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "begin should return quickly without waiting for startup runtime preparation"
        );

        let result: PreparedStartupModule = receipt
            .await_result()
            .await
            .expect("startup runtime evaluation result");
        assert_eq!(result.path, config_path);
        assert_eq!(
            result.specifier.as_str(),
            format!("file://{}/init.ts", current_dir.to_string_lossy())
        );
        assert!(result.executable_source_text.contains("const tabSize = 4;"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_dispatch_preserves_registration_order_without_blocking_sender() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let first_trace = trace.clone();
        let second_trace = trace.clone();

        let registry = CallbackRegistryBuilder::default()
            .on_buffer_open(move |_, payload| {
                let first_trace = first_trace.clone();
                Box::pin(async move {
                    first_trace
                        .lock()
                        .await
                        .push(format!("first:{:?}", payload.buffer.path));
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    first_trace.lock().await.push("first:done".to_string());
                    Ok(())
                })
            })
            .on_buffer_open(move |_, payload| {
                let second_trace = second_trace.clone();
                Box::pin(async move {
                    second_trace
                        .lock()
                        .await
                        .push(format!("second:{:?}", payload.buffer.path));
                    Ok(())
                })
            })
            .build();

        let runtime = SayaLiveRuntime::spawn(Arc::new(RecordingHostBridge::new()), registry);
        let payload = RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 11,
                path: Some(PathBuf::from("article.md")),
                line_count: 8,
            },
        });

        let started_at = Instant::now();
        let receipt = runtime
            .dispatch_event(payload)
            .expect("dispatch should succeed");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "dispatch should queue work without waiting for handlers"
        );

        let report = receipt.await_result().await.expect("dispatch result");
        assert_eq!(report.handler_count, 2);

        let trace = trace.lock().await.clone();
        assert_eq!(
            trace,
            vec![
                "first:Some(\"article.md\")".to_string(),
                "first:done".to_string(),
                "second:Some(\"article.md\")".to_string(),
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_callback_can_execute_registered_command_and_read_typed_state() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_in_command = observed.clone();
        let observed_in_event = observed.clone();

        let registry = CallbackRegistryBuilder::default()
            .register_command("writeCurrent", move |ctx| {
                let observed_in_command = observed_in_command.clone();
                Box::pin(async move {
                    let buffer = ctx.buffer().current().await;
                    let editor_mode = ctx.editor().mode().await;
                    observed_in_command
                        .lock()
                        .await
                        .push(format!("command:{:?}:{:?}", buffer.path, editor_mode));
                    ctx.commands().execute("write").await
                })
            })
            .on_buffer_open(move |ctx, payload| {
                let observed_in_event = observed_in_event.clone();
                Box::pin(async move {
                    observed_in_event.lock().await.push(format!(
                        "event:{}:{:?}",
                        payload.buffer.id, payload.buffer.path
                    ));
                    ctx.commands().execute("writeCurrent").await?;
                    Ok(())
                })
            })
            .build();

        let runtime = SayaLiveRuntime::spawn(host_bridge.clone(), registry);
        let receipt = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 21,
                    path: Some(PathBuf::from("typed.md")),
                    line_count: 5,
                },
            }))
            .expect("dispatch should succeed");

        let report = receipt.await_result().await.expect("dispatch report");
        assert_eq!(report.handler_count, 1);

        assert_eq!(
            observed.lock().await.clone(),
            vec![
                "event:21:Some(\"typed.md\")".to_string(),
                "command:Some(\"notes.md\"):Normal".to_string(),
            ]
        );
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_can_read_buffer_window_editor_state_and_typed_payload() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: r#"
                async (payload) => {
                    const buffer = await saya.buffer.current();
                    const window = await saya.window.current();
                    const mode = await saya.editor.mode();
                    await saya.commands.execute(`state:${payload.buffer.id}:${buffer.id}:${window.id}:${mode}`);
                }
            "#
            .to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 51,
                    path: Some(PathBuf::from("typed-payload.md")),
                    line_count: 12,
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 1);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["state:51:7:9:Normal".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_can_dispatch_buffer_write_post_payload() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferWritePost".to_string(),
            callback_source:
                "(payload) => saya.commands.execute(`write-post:${payload.buffer.id}:${payload.buffer.lineCount}`)"
                    .to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 61,
                    path: Some(PathBuf::from("write-post.md")),
                    line_count: 14,
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 1);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write-post:61:14".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_reports_unknown_command_as_structured_error() {
        let host_bridge = Arc::new(RecordingHostBridge::with_command_error(
            "missing",
            RuntimeCommandError::UnknownCommand {
                name: "missing".to_string(),
            },
        ));
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"missing\")".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let error = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 71,
                    path: Some(PathBuf::from("unknown-command.md")),
                    line_count: 2,
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect_err("unknown command should be surfaced");

        assert_eq!(
            error,
            super::RuntimeDispatchError::CallbackFailed {
                event: super::RuntimeEventName::BufferOpen,
                handler_index: 0,
                error: super::RuntimeCallbackError::Command(RuntimeCommandError::UnknownCommand {
                    name: "missing".to_string(),
                },),
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_reports_script_failure_as_structured_error() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => { throw new Error(\"boom\"); }".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let error = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 81,
                    path: Some(PathBuf::from("script-failure.md")),
                    line_count: 6,
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect_err("script failure should be surfaced");

        assert!(
            matches!(
                error,
                super::RuntimeDispatchError::CallbackFailed {
                    event: super::RuntimeEventName::BufferOpen,
                    handler_index: 0,
                    error: super::RuntimeCallbackError::ScriptFailed { ref message },
                } if message.contains("boom")
            ),
            "script failure should stay structured: {error:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_dispatch_queues_without_blocking_sender() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"write\")".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let started_at = Instant::now();
        let receipt = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 31,
                    path: Some(PathBuf::from("seed.md")),
                    line_count: 4,
                },
            }))
            .expect("dispatch should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "dispatch should return quickly even for seed runtime"
        );

        let report = receipt.await_result().await.expect("dispatch report");
        assert_eq!(report.handler_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_dispatches_event_handlers_in_registration_order() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![
            StartupRegistryEntry::Command {
                name: "writeCurrent".to_string(),
                callback_source: "() => saya.commands.execute(\"write\")".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: "(payload) => saya.commands.execute(\"writeCurrent\")".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: "(payload) => saya.commands.execute(\"write!\")".to_string(),
            },
        ]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 41,
                    path: Some(PathBuf::from("ordered.md")),
                    line_count: 9,
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 2);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write".to_string(), "write!".to_string()]
        );
    }
}
