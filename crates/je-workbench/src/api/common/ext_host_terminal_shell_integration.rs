//! Porte de `src/vs/workbench/api/common/extHostTerminalShellIntegration.ts`: o `InternalTerminalShellIntegration` (o estado
//! de execuções de comando de um terminal: execuções pendentes, a corrente, linhas de comando multilinha, os fluxos de dados
//! `read()`), o serviço `ExtHostTerminalShellIntegration` (liga a thread principal a um `InternalTerminalShellIntegration`
//! por terminal e implementa o `ExtHostTerminalShellIntegrationShape`) e os tipos `vscode.*` que eles produzem.
//!
//! Divergências:
//! - `ExtHostTerminalShellIntegration` é genérico sobre a estratégia de criação do [`BaseExtHostTerminalService`] (a
//!   `IExtHostTerminalService` do original); `IExtHostRpcService` é o [`IMainContext`]. `_register(toDisposable(...))` que
//!   limpa as integrações é o `dispose`. `terminal.onWillDispose(...)` descarta a integração sem tirá-la do mapa, como o
//!   original (quem tira é `$closeTerminal`). O `shellEnvValues` com `undefined` é `Option<String>`.
//! - `vscode.Terminal` é opaco para este arquivo (só viaja nos eventos e é comparado por identidade): [`Terminal`] é um
//!   `Rc<dyn Any>`.
//! - `InternalTerminalShellExecution` e o `value: vscode.TerminalShellExecution` que ele expõe são um tipo só,
//!   [`TerminalShellExecution`] (em `Rc`, a identidade é a de `value`). `read()` devolve o `AsyncIterable<string>`
//!   como [`AsyncIterable`]. O `ShellExecutionDataStream` guarda os emissores num `Rc<RefCell<..>>` porque o executor do
//!   [`AsyncIterableObject`] sobrevive ao `createIterable`.
//! - `InternalTerminalShellIntegration#value` (`vscode.TerminalShellIntegration`, com getters que fecham sobre
//!   `that`) é o [`TerminalShellIntegration`], com um [`Weak`] de volta para o `InternalTerminalShellIntegration` (que
//!   nasce em `Rc::new_cyclic`). `executeCommand(commandLine)` e `executeCommand(executable, args)` são
//!   [`TerminalShellIntegration::execute_command`] com `args` opcional; o lançamento do original é o `Err`.
//! - `Object.freeze({...})` do `env` é um clone de [`TerminalShellIntegrationEnvironment`]. `env: { [key]: string |
//!   undefined }` é uma lista de pares que preserva a ordem de inserção (uma chave repetida sobrescreve a anterior).
//! - `/\s/` do `executeCommand` é `char::is_whitespace`.
//! - `flush()` (`async`) devolve [`Promise`]; as chamadas que não esperam o resultado (`this._currentExecution.flush()`)
//!   descartam a promessa.
//! - `exitCode: number | undefined` é `Option<i32>`; `cwd: URI | undefined` é `Option<URI>`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{AsyncIterable, AsyncIterableEmitter, AsyncIterableObject, Barrier, async_fn, async_iterable_to_promise};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_platform::resources::host_platform;

use super::ext_host_protocol::{
    ExtHostTerminalShellIntegrationShape, IMainContext, MainContext, MainThreadTerminalShellIntegrationShape,
};
use super::ext_host_terminal_service::{BaseExtHostTerminalService, ExtHostTerminalCreation};
use super::ext_host_types::TerminalShellExecutionCommandLineConfidence;

/// `vscode.Terminal`, opaco (ver as divergências no topo do módulo).
pub type Terminal = Rc<dyn Any>;

/// `vscode.TerminalShellExecutionCommandLine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalShellExecutionCommandLine {
    pub value: String,
    pub confidence: TerminalShellExecutionCommandLineConfidence,
    pub is_trusted: bool,
}

/// `vscode.TerminalShellIntegrationEnvironment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalShellIntegrationEnvironment {
    pub is_trusted: bool,
    pub value: Vec<(String, Option<String>)>,
}

/// `vscode.TerminalShellExecutionStartEvent`.
#[derive(Clone)]
pub struct TerminalShellExecutionStartEvent {
    pub terminal: Terminal,
    pub shell_integration: Rc<TerminalShellIntegration>,
    pub execution: Rc<TerminalShellExecution>,
}

/// `vscode.TerminalShellExecutionEndEvent`.
#[derive(Clone)]
pub struct TerminalShellExecutionEndEvent {
    pub terminal: Terminal,
    pub shell_integration: Rc<TerminalShellIntegration>,
    pub execution: Rc<TerminalShellExecution>,
    pub exit_code: Option<i32>,
}

/// `vscode.TerminalShellIntegrationChangeEvent`.
#[derive(Clone)]
pub struct TerminalShellIntegrationChangeEvent {
    pub terminal: Terminal,
    pub shell_integration: Rc<TerminalShellIntegration>,
}

struct ExecutionProperties {
    is_multi_line: bool,
    unresolved_command_lines: Option<Vec<String>>,
}

// ---- o fluxo de dados de uma execução

/// `ShellExecutionDataStream`.
struct ShellExecutionDataStream {
    store: Rc<DisposableStore>,
    barrier: RefCell<Option<Barrier>>,
    iterables: RefCell<Vec<Rc<AsyncIterableObject<String>>>>,
    emitters: Rc<RefCell<Vec<AsyncIterableEmitter<String>>>>,
}

impl ShellExecutionDataStream {
    fn new() -> Rc<Self> {
        Rc::new(Self { store: DisposableStore::new(), barrier: RefCell::new(None), iterables: RefCell::new(Vec::new()), emitters: Rc::new(RefCell::new(Vec::new())) })
    }

    fn create_iterable(&self) -> Rc<AsyncIterableObject<String>> {
        let barrier = self.barrier.borrow_mut().get_or_insert_with(Barrier::new).clone();
        let emitters = self.emitters.clone();
        let iterable = AsyncIterableObject::new(
            move |emitter| {
                emitters.borrow_mut().push(emitter);
                MaybePromise::Promise(barrier.wait().then(|_| ()))
            },
            None,
        );
        self.iterables.borrow_mut().push(iterable.clone());
        iterable
    }

    fn emit_data(&self, data: &str) {
        let emitters = self.emitters.borrow().clone();
        for emitter in emitters {
            emitter.emit_one(data.to_owned());
        }
    }

    fn end_execution(&self) {
        if let Some(barrier) = &*self.barrier.borrow() {
            barrier.open();
        }
    }

    fn flush(&self) -> Promise<()> {
        let iterables = self.iterables.borrow().clone();
        async_fn(async move {
            let promises: Vec<Promise<()>> = iterables
                .into_iter()
                .map(|iterable| {
                    let iterable: Rc<dyn AsyncIterable<String>> = iterable;
                    async_iterable_to_promise(iterable).then(|_| ())
                })
                .collect();
            Promise::all(promises).await?;
            Ok(())
        })
    }
}

impl IDisposable for ShellExecutionDataStream {
    fn dispose(&self) {
        self.store.dispose();
    }
}

// ---- a execução

/// `InternalTerminalShellExecution` e o seu `value` (`vscode.TerminalShellExecution`): ver as divergências no topo.
pub struct TerminalShellExecution {
    command_line: RefCell<TerminalShellExecutionCommandLine>,
    cwd: Option<URI>,
    data_stream: RefCell<Option<Rc<ShellExecutionDataStream>>>,
    is_ended: Cell<bool>,
}

impl TerminalShellExecution {
    fn new(command_line: TerminalShellExecutionCommandLine, cwd: Option<URI>) -> Rc<Self> {
        Rc::new(Self { command_line: RefCell::new(command_line), cwd, data_stream: RefCell::new(None), is_ended: Cell::new(false) })
    }

    /// `commandLine`.
    pub fn command_line(&self) -> TerminalShellExecutionCommandLine {
        self.command_line.borrow().clone()
    }

    /// `cwd`.
    pub fn cwd(&self) -> Option<URI> {
        self.cwd.clone()
    }

    /// `read()`.
    pub fn read(&self) -> Rc<dyn AsyncIterable<String>> {
        if self.data_stream.borrow().is_none() {
            if self.is_ended.get() {
                return AsyncIterableObject::empty();
            }
            *self.data_stream.borrow_mut() = Some(ShellExecutionDataStream::new());
        }
        let stream = self.data_stream.borrow().clone().expect("o fluxo acabou de ser criado");
        stream.create_iterable()
    }

    fn emit_data(&self, data: &str) {
        if !self.is_ended.get() {
            let stream = self.data_stream.borrow().clone();
            if let Some(stream) = stream {
                stream.emit_data(data);
            }
        }
    }

    fn end_execution(&self, command_line: Option<TerminalShellExecutionCommandLine>) {
        if let Some(command_line) = command_line {
            *self.command_line.borrow_mut() = command_line;
        }
        let stream = self.data_stream.borrow().clone();
        if let Some(stream) = stream {
            stream.end_execution();
        }
        self.is_ended.set(true);
    }

    fn flush(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let stream = this.data_stream.borrow().clone();
            if let Some(stream) = stream {
                stream.flush().await?;
                stream.dispose();
                *this.data_stream.borrow_mut() = None;
            }
            Ok(())
        })
    }
}

// ---- o `value` do `InternalTerminalShellIntegration`

/// `vscode.TerminalShellIntegration`.
pub struct TerminalShellIntegration {
    owner: Weak<InternalTerminalShellIntegration>,
    supports_execute_command_api: bool,
}

impl TerminalShellIntegration {
    /// `cwd`.
    pub fn cwd(&self) -> Option<URI> {
        self.owner.upgrade().and_then(|owner| owner.cwd.borrow().clone())
    }

    /// `env`.
    pub fn env(&self) -> Option<TerminalShellIntegrationEnvironment> {
        self.owner.upgrade().and_then(|owner| owner.env.borrow().clone())
    }

    /// `executeCommand(commandLine)` e `executeCommand(executable, args)`.
    pub fn execute_command(&self, command_line_or_executable: &str, args: Option<&[String]>) -> Result<Rc<TerminalShellExecution>, Error> {
        if !self.supports_execute_command_api {
            return Err(Error::new("This terminal does not support the executeCommand API."));
        }
        let owner = self.owner.upgrade().ok_or_else(|| Error::new("The terminal shell integration was disposed."))?;
        let mut command_line_value = command_line_or_executable.to_owned();
        if let Some(args) = args {
            for arg in args {
                let wrap_in_quotes = !arg.contains(['"', '\'', '`']) && arg.chars().any(char::is_whitespace);
                if wrap_in_quotes {
                    command_line_value.push_str(&format!(" \"{arg}\""));
                } else {
                    command_line_value.push_str(&format!(" {arg}"));
                }
            }
        }

        owner.on_did_request_shell_execution_emitter.fire(command_line_value.clone());
        // Fire the event in a microtask to allow the extension to use the execution before
        // the start event fires
        let command_line = TerminalShellExecutionCommandLine {
            value: command_line_value,
            confidence: TerminalShellExecutionCommandLineConfidence::High,
            is_trusted: true,
        };
        let cwd = owner.cwd.borrow().clone();
        Ok(owner.request_new_shell_execution(command_line, cwd))
    }
}

// ---- o `InternalTerminalShellIntegration`

/// `InternalTerminalShellIntegration`.
pub struct InternalTerminalShellIntegration {
    base: Rc<DisposableStore>,
    terminal: Terminal,
    on_did_start_terminal_shell_execution: Rc<Emitter<TerminalShellExecutionStartEvent>>,

    pending_executions: RefCell<Vec<Rc<TerminalShellExecution>>>,
    pending_ending_execution: RefCell<Option<Rc<TerminalShellExecution>>>,
    current_execution_properties: RefCell<Option<ExecutionProperties>>,
    current_execution: RefCell<Option<Rc<TerminalShellExecution>>>,

    env: RefCell<Option<TerminalShellIntegrationEnvironment>>,
    cwd: RefCell<Option<URI>>,

    /// `store`.
    pub store: Rc<DisposableStore>,
    value: Rc<TerminalShellIntegration>,

    on_did_request_change_shell_integration_emitter: Rc<Emitter<TerminalShellIntegrationChangeEvent>>,
    on_did_request_shell_execution_emitter: Rc<Emitter<String>>,
    on_did_request_end_execution_emitter: Rc<Emitter<TerminalShellExecutionEndEvent>>,
    on_did_request_new_execution_emitter: Rc<Emitter<String>>,
}

impl InternalTerminalShellIntegration {
    pub fn new(terminal: Terminal, supports_execute_command_api: bool, on_did_start_terminal_shell_execution: Rc<Emitter<TerminalShellExecutionStartEvent>>) -> Rc<Self> {
        let base = DisposableStore::new();
        let store = base.add_rc(DisposableStore::new());
        let on_did_request_change_shell_integration_emitter = base.add_rc(Emitter::new());
        let on_did_request_shell_execution_emitter = base.add_rc(Emitter::new());
        let on_did_request_end_execution_emitter = base.add_rc(Emitter::new());
        let on_did_request_new_execution_emitter = base.add_rc(Emitter::new());
        Rc::new_cyclic(|this| Self {
            base,
            terminal,
            on_did_start_terminal_shell_execution,
            pending_executions: RefCell::new(Vec::new()),
            pending_ending_execution: RefCell::new(None),
            current_execution_properties: RefCell::new(None),
            current_execution: RefCell::new(None),
            env: RefCell::new(None),
            cwd: RefCell::new(None),
            store,
            value: Rc::new(TerminalShellIntegration { owner: this.clone(), supports_execute_command_api }),
            on_did_request_change_shell_integration_emitter,
            on_did_request_shell_execution_emitter,
            on_did_request_end_execution_emitter,
            on_did_request_new_execution_emitter,
        })
    }

    /// `currentExecution`.
    pub fn current_execution(&self) -> Option<Rc<TerminalShellExecution>> {
        self.current_execution.borrow().clone()
    }

    /// `value`.
    pub fn value(&self) -> Rc<TerminalShellIntegration> {
        self.value.clone()
    }

    /// `onDidRequestChangeShellIntegration`.
    pub fn on_did_request_change_shell_integration(&self) -> Event<TerminalShellIntegrationChangeEvent> {
        self.on_did_request_change_shell_integration_emitter.event()
    }

    /// `onDidRequestShellExecution`.
    pub fn on_did_request_shell_execution(&self) -> Event<String> {
        self.on_did_request_shell_execution_emitter.event()
    }

    /// `onDidRequestEndExecution`.
    pub fn on_did_request_end_execution(&self) -> Event<TerminalShellExecutionEndEvent> {
        self.on_did_request_end_execution_emitter.event()
    }

    /// `onDidRequestNewExecution`.
    pub fn on_did_request_new_execution(&self) -> Event<String> {
        self.on_did_request_new_execution_emitter.event()
    }

    fn fire_end_event(&self, execution: Rc<TerminalShellExecution>, exit_code: Option<i32>) {
        self.on_did_request_end_execution_emitter.fire(TerminalShellExecutionEndEvent {
            terminal: self.terminal.clone(),
            shell_integration: self.value.clone(),
            execution,
            exit_code,
        });
    }

    /// `requestNewShellExecution(commandLine, cwd)`: devolve a `InternalTerminalShellExecution`, cujo `value` é a própria execução.
    pub fn request_new_shell_execution(&self, command_line: TerminalShellExecutionCommandLine, cwd: Option<URI>) -> Rc<TerminalShellExecution> {
        let cwd = cwd.or_else(|| self.cwd.borrow().clone());
        let execution = TerminalShellExecution::new(command_line.clone(), cwd);
        let unresolved_command_lines = split_and_sanitize_command_line(&command_line.value);
        if unresolved_command_lines.len() > 1 {
            *self.current_execution_properties.borrow_mut() =
                Some(ExecutionProperties { is_multi_line: true, unresolved_command_lines: Some(split_and_sanitize_command_line(&command_line.value)) });
        }
        self.pending_executions.borrow_mut().push(execution.clone());
        self.on_did_request_new_execution_emitter.fire(command_line.value);
        execution
    }

    /// `startShellExecution(commandLine, cwd)`.
    pub fn start_shell_execution(&self, command_line: TerminalShellExecutionCommandLine, cwd: Option<URI>) {
        // Since an execution is starting, fire the end event for any execution that is awaiting to
        // end. When this happens it means that the data stream may not be flushed and therefore may
        // fire events after the end event.
        let pending_ending = self.pending_ending_execution.borrow_mut().take();
        if let Some(execution) = pending_ending {
            self.fire_end_event(execution, None);
        }

        let current = self.current_execution.borrow().clone();
        if let Some(current) = current {
            // If the current execution is multi-line, check if this command line is part of it.
            let sub_execution_result = match &*self.current_execution_properties.borrow() {
                Some(ExecutionProperties { is_multi_line: true, unresolved_command_lines: Some(unresolved) }) => is_sub_execution(unresolved, &command_line),
                _ => None,
            };
            if let Some(unresolved_command_lines) = sub_execution_result {
                if let Some(properties) = &mut *self.current_execution_properties.borrow_mut() {
                    properties.unresolved_command_lines = Some(unresolved_command_lines);
                }
                return;
            }
            current.end_execution(None);
            drop(current.flush());
            self.fire_end_event(current, None);
        }

        // Get the matching pending execution, how strict this is depends on the confidence of the
        // command line
        let mut current_execution: Option<Rc<TerminalShellExecution>> = None;
        if command_line.confidence == TerminalShellExecutionCommandLineConfidence::High {
            let pending = self.pending_executions.borrow().clone();
            for (i, execution) in pending.iter().enumerate() {
                if execution.command_line().value == command_line.value {
                    *self.current_execution_properties.borrow_mut() = Some(ExecutionProperties { is_multi_line: false, unresolved_command_lines: None });
                    current_execution = Some(execution.clone());
                    self.pending_executions.borrow_mut().remove(i);
                    break;
                }
                if let Some(unresolved_command_lines) = is_sub_execution(&split_and_sanitize_command_line(&execution.command_line().value), &command_line) {
                    *self.current_execution_properties.borrow_mut() = Some(ExecutionProperties { is_multi_line: true, unresolved_command_lines: Some(unresolved_command_lines) });
                    current_execution = Some(execution.clone());
                    self.pending_executions.borrow_mut().remove(i);
                    break;
                }
            }
        } else {
            let mut pending = self.pending_executions.borrow_mut();
            if !pending.is_empty() {
                current_execution = Some(pending.remove(0));
            }
        }

        // If there is no execution, create a new one
        let current_execution = current_execution.unwrap_or_else(|| {
            // Fallback to the shell integration's cwd as the cwd may not have been restored after a reload
            let cwd = cwd.or_else(|| self.cwd.borrow().clone());
            TerminalShellExecution::new(command_line.clone(), cwd)
        });

        *self.current_execution.borrow_mut() = Some(current_execution.clone());
        self.on_did_start_terminal_shell_execution.fire(TerminalShellExecutionStartEvent {
            terminal: self.terminal.clone(),
            shell_integration: self.value.clone(),
            execution: current_execution,
        });
    }

    /// `emitData(data)`.
    pub fn emit_data(&self, data: &str) {
        let current = self.current_execution.borrow().clone();
        if let Some(current) = current {
            current.emit_data(data);
        }
    }

    /// `endShellExecution(commandLine, exitCode)`.
    pub fn end_shell_execution(self: &Rc<Self>, command_line: Option<TerminalShellExecutionCommandLine>, exit_code: Option<i32>) {
        // If the current execution is multi-line, don't end it until the next command line is
        // confirmed to not be a part of it.
        let is_multi_line = self.current_execution_properties.borrow().as_ref().is_some_and(|properties| properties.is_multi_line);
        if is_multi_line
            && self.current_execution_properties.borrow().as_ref().and_then(|properties| properties.unresolved_command_lines.as_ref()).is_some_and(|lines| !lines.is_empty())
        {
            return;
        }

        let current = self.current_execution.borrow().clone();
        if let Some(current) = current {
            let command_line_for_event = if is_multi_line { Some(current.command_line()) } else { command_line };
            current.end_execution(command_line_for_event);
            *self.pending_ending_execution.borrow_mut() = Some(current.clone());
            *self.current_execution.borrow_mut() = None;
            // IMPORTANT: Ensure the current execution's data events are flushed in order to
            // prevent data events firing after the end event fires.
            let this = self.clone();
            let ending = current.clone();
            drop(current.flush().then(move |_| {
                // Only fire if it's still the same execution, if it's changed it would have already
                // been fired.
                let is_same = this.pending_ending_execution.borrow().as_ref().is_some_and(|pending| Rc::ptr_eq(pending, &ending));
                if is_same {
                    this.fire_end_event(ending, exit_code);
                    *this.pending_ending_execution.borrow_mut() = None;
                }
            }));
        }
    }

    /// `setEnv(keys, values, isTrusted)`.
    pub fn set_env(&self, keys: &[String], values: &[Option<String>], is_trusted: bool) {
        let mut env: Vec<(String, Option<String>)> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let value = values.get(i).cloned().flatten();
            match env.iter_mut().find(|(existing, _)| existing == key) {
                Some(entry) => entry.1 = value,
                None => env.push((key.clone(), value)),
            }
        }
        *self.env.borrow_mut() = Some(TerminalShellIntegrationEnvironment { value: env, is_trusted });
        self.fire_change_event();
    }

    /// `setCwd(cwd)`.
    pub fn set_cwd(&self, cwd: Option<URI>) {
        let was_changed = match (&*self.cwd.borrow(), &cwd) {
            (Some(current), Some(new)) => current.to_string(false) != new.to_string(false),
            (None, None) => false,
            _ => true,
        };
        if was_changed {
            *self.cwd.borrow_mut() = cwd;
            self.fire_change_event();
        }
    }

    fn fire_change_event(&self) {
        self.on_did_request_change_shell_integration_emitter
            .fire(TerminalShellIntegrationChangeEvent { terminal: self.terminal.clone(), shell_integration: self.value.clone() });
    }
}

impl IDisposable for InternalTerminalShellIntegration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---- o serviço

/// `ExtHostTerminalShellIntegration` (e a sua `IExtHostTerminalShellIntegration`).
pub struct ExtHostTerminalShellIntegration<S: ExtHostTerminalCreation> {
    this: Weak<Self>,
    base: Rc<DisposableStore>,
    proxy: Rc<dyn MainThreadTerminalShellIntegrationShape>,
    ext_host_terminal_service: Rc<BaseExtHostTerminalService<S>>,
    /// `_activeShellIntegrations`, por id da instância.
    active_shell_integrations: RefCell<HashMap<u32, Rc<InternalTerminalShellIntegration>>>,
    on_did_change_terminal_shell_integration_emitter: Rc<Emitter<TerminalShellIntegrationChangeEvent>>,
    on_did_start_terminal_shell_execution_emitter: Rc<Emitter<TerminalShellExecutionStartEvent>>,
    on_did_end_terminal_shell_execution_emitter: Rc<Emitter<TerminalShellExecutionEndEvent>>,
}

impl<S: ExtHostTerminalCreation> ExtHostTerminalShellIntegration<S> {
    pub fn new(ext_host_rpc: &dyn IMainContext, ext_host_terminal_service: Rc<BaseExtHostTerminalService<S>>) -> Rc<Self> {
        let proxy = ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_TERMINAL_SHELL_INTEGRATION);
        let base = DisposableStore::new();
        let on_did_change_terminal_shell_integration_emitter = base.add_rc(Emitter::new());
        let on_did_start_terminal_shell_execution_emitter = base.add_rc(Emitter::new());
        let on_did_end_terminal_shell_execution_emitter = base.add_rc(Emitter::new());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            proxy,
            ext_host_terminal_service,
            active_shell_integrations: RefCell::new(HashMap::new()),
            on_did_change_terminal_shell_integration_emitter,
            on_did_start_terminal_shell_execution_emitter,
            on_did_end_terminal_shell_execution_emitter,
        })
    }

    /// `onDidChangeTerminalShellIntegration`.
    pub fn on_did_change_terminal_shell_integration(&self) -> Event<TerminalShellIntegrationChangeEvent> {
        self.on_did_change_terminal_shell_integration_emitter.event()
    }

    /// `onDidStartTerminalShellExecution`.
    pub fn on_did_start_terminal_shell_execution(&self) -> Event<TerminalShellExecutionStartEvent> {
        self.on_did_start_terminal_shell_execution_emitter.event()
    }

    /// `onDidEndTerminalShellExecution`.
    pub fn on_did_end_terminal_shell_execution(&self) -> Event<TerminalShellExecutionEndEvent> {
        self.on_did_end_terminal_shell_execution_emitter.event()
    }

    fn integration_of(&self, instance_id: u32) -> Option<Rc<InternalTerminalShellIntegration>> {
        self.active_shell_integrations.borrow().get(&instance_id).cloned()
    }

    /// `_convertCwdToUri(cwd)`: o `cwd` chega do renderer como texto e só vira URI aqui, na máquina em que o pty vive.
    fn convert_cwd_to_uri(cwd: Option<&str>) -> Option<URI> {
        cwd.filter(|cwd| !cwd.is_empty()).map(|cwd| URI::file(cwd, host_platform()))
    }
}

impl<S: ExtHostTerminalCreation> IDisposable for ExtHostTerminalShellIntegration<S> {
    fn dispose(&self) {
        let integrations: Vec<_> = self.active_shell_integrations.borrow_mut().drain().map(|(_, integration)| integration).collect();
        for integration in integrations {
            integration.dispose();
        }
        self.base.dispose();
    }
}

impl<S: ExtHostTerminalCreation> ExtHostTerminalShellIntegrationShape for ExtHostTerminalShellIntegration<S> {
    fn rpc_shell_integration_change(&self, instance_id: u32, supports_execute_command_api: bool) {
        let Some(terminal) = self.ext_host_terminal_service.get_terminal_by_id(instance_id) else { return };

        let api_terminal: Terminal = terminal.value();
        let shell_integration = match self.integration_of(instance_id) {
            Some(shell_integration) => shell_integration,
            None => {
                let shell_integration =
                    InternalTerminalShellIntegration::new(api_terminal.clone(), supports_execute_command_api, self.on_did_start_terminal_shell_execution_emitter.clone());
                self.active_shell_integrations.borrow_mut().insert(instance_id, shell_integration.clone());
                let weak = self.this.clone();
                shell_integration.store.add(terminal.on_will_dispose().listen(move |_| {
                    let Some(this) = weak.upgrade() else { return };
                    let integration = this.integration_of(instance_id);
                    if let Some(integration) = integration {
                        integration.dispose();
                    }
                }));
                let proxy = self.proxy.clone();
                shell_integration.store.add(shell_integration.on_did_request_shell_execution().listen(move |command_line| proxy.rpc_execute_command(instance_id, command_line)));
                let end_emitter = self.on_did_end_terminal_shell_execution_emitter.clone();
                shell_integration.store.add(shell_integration.on_did_request_end_execution().listen(move |e| end_emitter.fire(e.clone())));
                let change_emitter = self.on_did_change_terminal_shell_integration_emitter.clone();
                shell_integration.store.add(shell_integration.on_did_request_change_shell_integration().listen(move |e| change_emitter.fire(e.clone())));
                terminal.set_shell_integration(Some(shell_integration.value()));
                shell_integration
            }
        };
        self.on_did_change_terminal_shell_integration_emitter
            .fire(TerminalShellIntegrationChangeEvent { terminal: api_terminal, shell_integration: shell_integration.value() });
    }

    fn rpc_shell_execution_start(
        &self,
        instance_id: u32,
        supports_execute_command_api: bool,
        command_line_value: &str,
        command_line_confidence: TerminalShellExecutionCommandLineConfidence,
        is_trusted: bool,
        cwd: Option<&str>,
    ) {
        // Force shellIntegration creation if it hasn't been created yet, this could when events
        // don't come through on startup
        let exists = self.active_shell_integrations.borrow().contains_key(&instance_id);
        if !exists {
            self.rpc_shell_integration_change(instance_id, supports_execute_command_api);
        }
        let command_line = TerminalShellExecutionCommandLine { value: command_line_value.to_owned(), confidence: command_line_confidence, is_trusted };
        if let Some(integration) = self.integration_of(instance_id) {
            integration.start_shell_execution(command_line, Self::convert_cwd_to_uri(cwd));
        }
    }

    fn rpc_shell_execution_end(
        &self,
        instance_id: u32,
        command_line_value: &str,
        command_line_confidence: TerminalShellExecutionCommandLineConfidence,
        is_trusted: bool,
        exit_code: Option<i32>,
    ) {
        let command_line = TerminalShellExecutionCommandLine { value: command_line_value.to_owned(), confidence: command_line_confidence, is_trusted };
        if let Some(integration) = self.integration_of(instance_id) {
            integration.end_shell_execution(Some(command_line), exit_code);
        }
    }

    fn rpc_shell_execution_data(&self, instance_id: u32, data: &str) {
        if let Some(integration) = self.integration_of(instance_id) {
            integration.emit_data(data);
        }
    }

    fn rpc_shell_env_change(&self, instance_id: u32, shell_env_keys: &[String], shell_env_values: &[Option<String>], is_trusted: bool) {
        if let Some(integration) = self.integration_of(instance_id) {
            integration.set_env(shell_env_keys, shell_env_values, is_trusted);
        }
    }

    fn rpc_cwd_change(&self, instance_id: u32, cwd: Option<&str>) {
        if let Some(integration) = self.integration_of(instance_id) {
            integration.set_cwd(Self::convert_cwd_to_uri(cwd));
        }
    }

    fn rpc_close_terminal(&self, instance_id: u32) {
        let integration = self.active_shell_integrations.borrow_mut().remove(&instance_id);
        if let Some(integration) = integration {
            integration.dispose();
        }
    }
}

/// `splitAndSanitizeCommandLine(commandLine)`.
fn split_and_sanitize_command_line(command_line: &str) -> Vec<String> {
    command_line.split('\n').map(|line| line.trim().to_owned()).filter(|line| !line.is_empty()).collect()
}

/// `isSubExecution(unresolvedCommandLines, commandLine)`: quando se executa algo que o shell considera vários comandos,
/// como um comentário seguido de um comando, tudo precisa ser rastreado sob uma única execução. Devolve as linhas ainda não
/// resolvidas (`{ unresolvedCommandLines }`) ou `None` (`false`).
fn is_sub_execution(unresolved_command_lines: &[String], command_line: &TerminalShellExecutionCommandLine) -> Option<Vec<String>> {
    if unresolved_command_lines.is_empty() {
        return None;
    }
    let mut new_unresolved_command_lines = unresolved_command_lines.to_vec();
    let mut sub_execution_lines = split_and_sanitize_command_line(&command_line.value);
    // If all sub-execution lines are in the command line, this is part of the
    // multi-line execution.
    while !new_unresolved_command_lines.is_empty() {
        if Some(&new_unresolved_command_lines[0]) != sub_execution_lines.first() {
            break;
        }
        new_unresolved_command_lines.remove(0);
        sub_execution_lines.remove(0);
    }

    if sub_execution_lines.is_empty() {
        return Some(new_unresolved_command_lines);
    }
    None
}
