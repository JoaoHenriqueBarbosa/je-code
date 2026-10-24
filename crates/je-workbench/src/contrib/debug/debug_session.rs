//! Porte de `src/vs/workbench/contrib/debug/browser/debugSession.ts`: a sessão de depuração lógica (`DebugSession`), que
//! fala com o adaptador pelo `RawDebugSession` (o trait [`IRawDebugSession`]) e mantém threads, fontes, REPL e breakpoints,
//! e o `ThreadStatusScheduler`.
//!
//! Notas de tradução:
//! - **Injeção de serviços.** Os 20 serviços do construtor viram o trait [`IDebugSessionServices`], com só o que a sessão
//!   chama (o `IDebugService` que a sessão usa, a configuração `debug`, o aviso de tela, as notificações, o host, o painel
//!   de depuração, a telemetria do endpoint do adaptador, a fábrica da `RawDebugSession`). `debugService.getModel()` é o
//!   `model` do construtor (o mesmo objeto no original).
//! - **Opções.** As opções com `parentSession` usam a [`DebugSessionOptions`] (a `IDebugSessionOptions` de `debug.rs` guarda
//!   o `IDebugSession` mínimo; a sessão precisa do completo para `terminate`, `state`, `repl`).
//! - **`raw`.** O campo público `raw` (usado nos testes) é [`DebugSession::set_raw`].
//! - **Ficam de fora** o `correlatedTestRun` (`LiveTestResult`, `ITestService`, `ITestResultService`: o módulo de testes não
//!   foi portado), logo `cancelCorrelatedTestRun` é um no-op, `terminate` não desvia para a execução de teste e a opção
//!   `testRun` não existe.
//! - `mixin(source.raw, raw)` copia só os campos presentes (`Some`) de `raw`; `presentationHint` é sempre trocado.
//! - Métodos `async` são [`async_fn`] com a mesma ordem de `await`; os que o original chama sem `await` (`focus()`,
//!   `statusQueue.run(...)` do `handleStop`, `this.shutdown()`) rodam sem esperar o resultado.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{Queue, RunOnceScheduler, async_fn, race_timeout};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::log::LogService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::WorkspaceFolder;
use serde_json::{Value, json};

use super::breakpoints::{
    Breakpoint, DapBreakpoint, DataBreakpoint, ExceptionBreakpoint, ExceptionBreakpointsFilter, FunctionBreakpoint, InstructionBreakpoint,
    protocol_source_from_value, protocol_source_to_value,
};
use super::debug::{DebugCompoundRoot, IConfig, IDebugSession, IDebugSessionReplMode, INewReplElementData};
use super::debug_model::{BreakpointFilter, BreakpointMode, DebugModel, ExpressionContainer, MemoryRegion, StackFrame, Thread, is_frame_deemphasized};
use super::debug_protocol::{Event as ProtocolEvent, Response, Source as ProtocolSource};
use super::debug_source::Source;
use super::debug_types::{
    AdapterEndEvent, DapResponse, DapThread, IDataBreakpointInfoResponse, IDebugEvaluatePosition, IDebugLocationReferenced, IDebugSessionFull, IDebugger,
    IExceptionInfo, IRawModelUpdate, IRawStoppedDetails, ITelemetryEndpoint, LoadedSourceEvent, LoadedSourceReason, State, StoppedDetails, capability,
    new_stopped_details,
};
use super::debug_utils::filter_exceptions_from_telemetry;
use super::debug_view_model::ViewModel;
use super::raw_debug_session::IRawDebugSession;
use super::repl_model::{DebugConsoleConfiguration, IReplElementSource, NewReplElementData, ReplElement, ReplModel};

const TRIGGERED_BREAKPOINT_MAX_DELAY: u64 = 1500;

/// `IDebugConfiguration`, só o que a sessão lê.
#[derive(Clone, Debug)]
pub struct DebugConfiguration {
    /// `autoExpandLazyVariables`: `'auto' | 'off' | 'on'`.
    pub auto_expand_lazy_variables: String,
    pub focus_editor_on_break: bool,
    /// `openDebug`: `'neverOpen' | 'openOnSessionStart' | 'openOnFirstSessionStart' | 'openOnDebugBreak'`.
    pub open_debug: String,
    pub focus_window_on_break: bool,
    pub console: DebugConsoleConfiguration,
}

impl Default for DebugConfiguration {
    fn default() -> Self {
        Self {
            auto_expand_lazy_variables: "off".to_owned(),
            focus_editor_on_break: true,
            open_debug: "openOnFirstSessionStart".to_owned(),
            focus_window_on_break: true,
            console: DebugConsoleConfiguration { collapse_identical_lines: true, maximum_lines: 10000 },
        }
    }
}

/// O `{ explicit?, preserveFocus? }` de `focusStackFrame`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FocusOptions {
    pub explicit: Option<bool>,
    pub preserve_focus: Option<bool>,
}

/// Os serviços que a [`DebugSession`] usa (ver as notas do módulo).
pub trait IDebugSessionServices {
    // IDebugService
    fn view_model(&self) -> Rc<ViewModel>;
    fn focus_stack_frame(
        &self,
        stack_frame: Option<Rc<StackFrame>>,
        thread: Option<Rc<Thread>>,
        session: Option<Rc<dyn IDebugSessionFull>>,
        options: FocusOptions,
    ) -> Promise<()>;
    fn send_all_breakpoints(&self, session: Rc<dyn IDebugSessionFull>) -> Promise<()>;
    fn send_breakpoints(&self, uri: URI, source_modified: Option<bool>, session: Rc<dyn IDebugSessionFull>) -> Promise<()>;
    fn restart_session(&self, session: Rc<dyn IDebugSessionFull>, restart: Value) -> Promise<()>;
    fn set_exception_breakpoints_for_session(&self, session: Rc<dyn IDebugSessionFull>, filters: Vec<ExceptionBreakpointsFilter>);

    // configuração, ambiente e UI
    fn debug_configuration(&self) -> DebugConfiguration;
    fn is_screen_reader_optimized(&self) -> bool;
    fn workspace_folder_count(&self) -> usize;
    fn product_name_long(&self) -> String;
    fn locale(&self) -> String;
    fn aria_status(&self, message: &str);
    fn notify_error(&self, message: &str);
    fn notify(&self, message: &str, severity: Severity, source: &str);
    fn open_debug_view(&self) -> Promise<()>;
    fn extension_tests_location_uri_is_set(&self) -> bool;
    fn window_has_focus(&self) -> bool;
    fn focus_window(&self) -> Promise<()>;
    fn telemetry_enabled(&self) -> bool;
    fn publish_custom_endpoint_telemetry(&self, endpoint: &ITelemetryEndpoint, name: &str, data: Option<Value>);
    /// `onWillShutdown` do `ILifecycleService` (ausente quando o serviço não existe).
    fn on_will_shutdown(&self) -> Option<Event<()>>;
    fn uri_identity_service(&self) -> Rc<dyn IUriIdentityService>;
    fn log_service(&self) -> Rc<dyn LogService>;
    /// `instantiationService.createInstance(RawDebugSession, debugAdapter, dbgr, id, name)`.
    fn create_raw_session(
        &self,
        debug_adapter: Rc<dyn super::debug::IDebugAdapter>,
        dbgr: Rc<dyn IDebugger>,
        session_id: &str,
        name: &str,
    ) -> Rc<dyn IRawDebugSession>;
}

/// `IDebugSessionOptions` com o `parentSession` completo e sem o `testRun`.
#[derive(Clone, Default)]
pub struct DebugSessionOptions {
    pub no_debug: Option<bool>,
    pub parent_session: Option<Rc<dyn IDebugSessionFull>>,
    pub lifecycle_managed_by_parent: Option<bool>,
    pub repl: Option<IDebugSessionReplMode>,
    pub compound_root: Option<Rc<DebugCompoundRoot>>,
    pub compact: Option<bool>,
    pub started_by_user: Option<bool>,
    pub save_before_restart: Option<bool>,
    pub suppress_debug_toolbar: Option<bool>,
    pub suppress_debug_statusbar: Option<bool>,
    pub suppress_debug_view: Option<bool>,
}

fn is_same<T: ?Sized, U: ?Sized>(a: &Rc<T>, b: &Rc<U>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

fn no_debug_adapter(what: &str) -> Error {
    Error::new(localize("noDebugAdapter", "No debugger available, can not send '{0}'", &[what]))
}

fn body_of(event: &ProtocolEvent) -> Value {
    event.body.clone().unwrap_or(Value::Null)
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn number_of(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

type ThreadOperation = Rc<dyn Fn(i64, CancellationToken) -> Promise<()>>;

/// Keeps track of events for threads, and cancels any previous operations for a thread when the thread goes into a new
/// state. Currently, the operations a thread has are: started, stopped, continue, exited.
///
/// In each case, the new state preempts the old state, so we don't need to queue work, just cancel old work. It's up to
/// the caller to make sure that no UI effects happen at the point when the `token` is cancelled.
pub struct ThreadStatusScheduler {
    /// An array of set of thread IDs. When a 'stopped' event is encountered, the editor refreshes its thread IDs. In the
    /// meantime, the thread may change state it again. So the editor puts a Set into this array when it starts the refresh,
    /// and checks it after the refresh is finished, to see if any of the threads it looked up should now be invalidated.
    pending_cancellations: RefCell<Vec<Rc<RefCell<HashSet<Option<i64>>>>>>,

    /// Cancellation tokens for currently-running operations on threads.
    thread_ops: Rc<DisposableMap<i64, CancellationTokenSource>>,
}

impl ThreadStatusScheduler {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { pending_cancellations: RefCell::new(Vec::new()), thread_ops: DisposableMap::new() })
    }

    /// Runs the operation. If thread is undefined it affects all threads.
    pub fn run(self: &Rc<Self>, thread_ids_p: Promise<Vec<i64>>, operation: ThreadOperation) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let cancelled_while_looking_up_threads: Rc<RefCell<HashSet<Option<i64>>>> = Rc::default();
            this.pending_cancellations.borrow_mut().push(cancelled_while_looking_up_threads.clone());
            let thread_ids = thread_ids_p.await?;

            // Now that we got our threads,
            // 1. Remove our pending set, and
            // 2. Cancel any slower callers who might also have found this thread
            {
                let mut pending = this.pending_cancellations.borrow_mut();
                let mut i = 0;
                while i < pending.len() {
                    let s = pending[i].clone();
                    if Rc::ptr_eq(&s, &cancelled_while_looking_up_threads) {
                        pending.remove(i);
                        break;
                    } else {
                        for thread_id in &thread_ids {
                            s.borrow_mut().insert(Some(*thread_id));
                        }
                    }
                    i += 1;
                }
            }

            if cancelled_while_looking_up_threads.borrow().contains(&None) {
                return Ok(());
            }

            let mut operations = Vec::new();
            for thread_id in thread_ids {
                if cancelled_while_looking_up_threads.borrow().contains(&Some(thread_id)) {
                    operations.push(Promise::resolved(()));
                    continue;
                }
                if let Some(previous) = this.thread_ops.get(&thread_id) {
                    previous.cancel();
                }
                let cts = Rc::new(CancellationTokenSource::new(None));
                this.thread_ops.set(thread_id, cts.clone(), false);
                operations.push(operation(thread_id, cts.token()));
            }
            Promise::all(operations).await?;
            Ok(())
        })
    }

    /// Cancels all ongoing state operations on the given threads. If threads is undefined it cancel all threads.
    pub fn cancel(&self, thread_ids: Option<&[i64]>) {
        match thread_ids {
            None => {
                for key in self.thread_ops.keys() {
                    if let Some(op) = self.thread_ops.get(&key) {
                        op.cancel();
                    }
                }
                self.thread_ops.clear_and_dispose_all();
                for s in self.pending_cancellations.borrow().iter() {
                    s.borrow_mut().insert(None);
                }
            }
            Some(thread_ids) => {
                for thread_id in thread_ids {
                    if let Some(op) = self.thread_ops.get(thread_id) {
                        op.cancel();
                    }
                    self.thread_ops.delete_and_dispose(thread_id);
                    for s in self.pending_cancellations.borrow().iter() {
                        s.borrow_mut().insert(Some(*thread_id));
                    }
                }
            }
        }
    }
}

impl IDisposable for ThreadStatusScheduler {
    fn dispose(&self) {
        self.thread_ops.dispose();
    }
}

/// `DebugSession`.
pub struct DebugSession {
    id: String,
    configuration: RefCell<(IConfig, Option<IConfig>)>,
    pub root: Option<WorkspaceFolder>,
    model: Rc<DebugModel>,
    services: Rc<dyn IDebugSessionServices>,
    options: DebugSessionOptions,
    parent_session: RefCell<Option<Rc<dyn IDebugSessionFull>>>,
    remembered_capabilities: RefCell<Option<Value>>,
    sub_id: RefCell<Option<String>>,
    raw: RefCell<Option<Rc<dyn IRawDebugSession>>>,
    initialized: Cell<bool>,

    sources: RefCell<HashMap<String, Rc<Source>>>,
    threads: RefCell<IndexMap<i64, Rc<Thread>>>,
    thread_ids: RefCell<Vec<i64>>,
    cancellation_map: RefCell<HashMap<i64, Vec<CancellationTokenSource>>>,
    raw_listeners: Rc<DisposableStore>,
    global_disposables: Rc<DisposableStore>,
    fetch_threads_scheduler: RefCell<Option<Rc<RunOnceScheduler>>>,
    pass_focus_scheduler: RefCell<Option<Rc<RunOnceScheduler>>>,
    last_continued_thread_id: Cell<Option<i64>>,
    repl: RefCell<Rc<ReplModel>>,
    stopped_details: RefCell<Vec<StoppedDetails>>,
    status_queue: Rc<ThreadStatusScheduler>,

    on_did_change_state: Rc<Emitter<()>>,
    on_did_end_adapter: Rc<Emitter<Option<AdapterEndEvent>>>,
    on_did_loaded_source: Rc<Emitter<LoadedSourceEvent>>,
    on_did_custom_event: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_start: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_update: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_end: Rc<Emitter<ProtocolEvent>>,
    on_did_invalid_memory: Rc<Emitter<ProtocolEvent>>,
    on_did_change_repl_elements: Rc<Emitter<Option<ReplElement>>>,

    name: RefCell<Option<String>>,
    on_did_change_name: Rc<Emitter<String>>,
    repl_listener: RefCell<Option<DisposableRef>>,

    /// Promise set while enabling dependent breakpoints to block the debugger from continuing from a stopped state.
    wait_to_resume: RefCell<Option<Promise<()>>>,
    state_override: RefCell<Option<State>>,
    me: Weak<DebugSession>,
}

impl DebugSession {
    pub fn new(
        id: String,
        resolved: IConfig,
        unresolved: Option<IConfig>,
        root: Option<WorkspaceFolder>,
        model: Rc<DebugModel>,
        options: Option<DebugSessionOptions>,
        services: Rc<dyn IDebugSessionServices>,
    ) -> Rc<Self> {
        let options = options.unwrap_or_default();
        let parent_session = options.parent_session.clone();
        let raw_listeners = DisposableStore::new();
        let global_disposables = DisposableStore::new();
        let status_queue = ThreadStatusScheduler::new();
        raw_listeners.add(status_queue.clone());

        let has_separate_repl = parent_session.is_none() || options.repl != Some(IDebugSessionReplMode::MergeWithParent);
        let console = {
            let services = services.clone();
            Rc::new(move || services.debug_configuration().console) as super::repl_model::DebugConsoleConfigurationProvider
        };
        let repl = if has_separate_repl { ReplModel::new(console) } else { parent_session.as_ref().expect("a sessão pai").repl_model() };

        let emitter = |store: &Rc<DisposableStore>| -> Rc<Emitter<ProtocolEvent>> {
            let emitter = Emitter::new();
            store.add(emitter.clone());
            emitter
        };
        macro_rules! owned_emitter {
            () => {{
                let emitter = Emitter::new();
                global_disposables.add(emitter.clone());
                emitter
            }};
        }

        let session = Rc::new_cyclic(|me| Self {
            id,
            configuration: RefCell::new((resolved, unresolved)),
            root,
            model,
            services: services.clone(),
            options: options.clone(),
            parent_session: RefCell::new(parent_session.clone()),
            remembered_capabilities: RefCell::new(None),
            sub_id: RefCell::new(None),
            raw: RefCell::new(None),
            initialized: Cell::new(false),
            sources: RefCell::new(HashMap::new()),
            threads: RefCell::new(IndexMap::new()),
            thread_ids: RefCell::new(Vec::new()),
            cancellation_map: RefCell::new(HashMap::new()),
            raw_listeners: raw_listeners.clone(),
            global_disposables: global_disposables.clone(),
            fetch_threads_scheduler: RefCell::new(None),
            pass_focus_scheduler: RefCell::new(None),
            last_continued_thread_id: Cell::new(None),
            repl: RefCell::new(repl.clone()),
            stopped_details: RefCell::new(Vec::new()),
            status_queue,
            on_did_change_state: owned_emitter!(),
            on_did_end_adapter: owned_emitter!(),
            on_did_loaded_source: owned_emitter!(),
            on_did_custom_event: emitter(&global_disposables),
            on_did_progress_start: emitter(&global_disposables),
            on_did_progress_update: emitter(&global_disposables),
            on_did_progress_end: emitter(&global_disposables),
            on_did_invalid_memory: emitter(&global_disposables),
            on_did_change_repl_elements: owned_emitter!(),
            name: RefCell::new(None),
            on_did_change_name: owned_emitter!(),
            repl_listener: RefCell::new(None),
            wait_to_resume: RefCell::new(None),
            state_override: RefCell::new(None),
            me: me.clone(),
        });

        let weak = Rc::downgrade(&session);
        let subscribe_repl = |repl: &Rc<ReplModel>, session: &Rc<DebugSession>| {
            let weak = Rc::downgrade(session);
            repl.on_did_change_elements().listen(move |e| {
                if let Some(session) = weak.upgrade() {
                    session.on_did_change_repl_elements.fire(e.clone());
                }
            })
        };
        let listener = subscribe_repl(&repl, &session);
        *session.repl_listener.borrow_mut() = Some(listener);

        if let Some(on_will_shutdown) = services.on_will_shutdown() {
            let weak = weak.clone();
            global_disposables.add(on_will_shutdown.listen(move |_| {
                if let Some(session) = weak.upgrade() {
                    session.shutdown();
                    session.global_disposables.dispose();
                }
            }));
        }

        if let Some(compound_root) = options.compound_root.clone() {
            let weak = weak.clone();
            global_disposables.add(compound_root.on_did_session_stop().listen(move |_| {
                if let Some(session) = weak.upgrade() {
                    session.terminate_inner(false);
                }
            }));
        }

        let weak_pass = weak.clone();
        let pass_focus_scheduler = RunOnceScheduler::new(
            move || {
                let Some(this) = weak_pass.upgrade() else {
                    return;
                };
                // If there is some session or thread that is stopped pass focus to it
                if this.model.get_sessions(false).iter().any(|s| s.state() == State::Stopped) || this.threads.borrow().values().any(|t| t.stopped.get()) {
                    let services = this.services.clone();
                    if let Some(last_continued) = this.last_continued_thread_id.get() {
                        let thread = services.view_model().focused_thread();
                        if let Some(thread) = thread
                            && thread.thread_id == last_continued
                            && !thread.stopped.get()
                        {
                            let to_focus_thread_id = this.get_stopped_details().and_then(|d| d.borrow().thread_id);
                            let to_focus_thread = to_focus_thread_id.and_then(|id| this.get_thread(id));
                            services.focus_stack_frame(None, to_focus_thread, None, FocusOptions::default());
                        }
                    } else {
                        let focused = services.view_model().focused_session();
                        if let Some(focused) = focused
                            && focused.get_id() == this.get_id()
                            && focused.state() != State::Stopped
                        {
                            services.focus_stack_frame(None, None, None, FocusOptions::default());
                        }
                    }
                }
            },
            800,
        );
        *session.pass_focus_scheduler.borrow_mut() = Some(pass_focus_scheduler);

        if let Some(parent) = parent_session {
            let weak = weak.clone();
            global_disposables.add(parent.on_did_end_adapter().listen(move |_| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                // copy the parent repl and get a new detached repl for this child, and remove its parent, if it's still running
                let not_in_shutdown = this.raw.borrow().as_ref().is_some_and(|raw| !raw.is_in_shutdown());
                if !this.has_separate_repl() && not_in_shutdown {
                    let cloned = this.repl.borrow().clone_model();
                    *this.repl.borrow_mut() = cloned.clone();
                    let weak_inner = Rc::downgrade(&this);
                    let listener = cloned.on_did_change_elements().listen(move |e| {
                        if let Some(session) = weak_inner.upgrade() {
                            session.on_did_change_repl_elements.fire(e.clone());
                        }
                    });
                    if let Some(old) = this.repl_listener.borrow_mut().replace(listener) {
                        old.dispose();
                    }
                    *this.parent_session.borrow_mut() = None;
                }
            }));
        }

        session
    }

    fn this(&self) -> Rc<DebugSession> {
        self.me.upgrade().expect("a sessão está viva")
    }

    fn this_full(&self) -> Rc<dyn IDebugSessionFull> {
        self.this()
    }

    /// `session.raw = raw` (usado nos testes) e o `initializeForTest`.
    pub fn set_raw<R: IRawDebugSession + 'static>(&self, raw: Option<Rc<R>>) {
        *self.raw.borrow_mut() = raw.map(|raw| raw as Rc<dyn IRawDebugSession>);
    }

    pub fn raw(&self) -> Option<Rc<dyn IRawDebugSession>> {
        self.raw.borrow().clone()
    }

    /// `state` sobrescrito (o `override get state()` das subclasses de teste do original).
    pub fn override_state(&self, state: Option<State>) {
        *self.state_override.borrow_mut() = state;
    }

    fn raw_or_err(&self, what: &str) -> Result<Rc<dyn IRawDebugSession>, Error> {
        self.raw.borrow().clone().ok_or_else(|| no_debug_adapter(what))
    }

    fn fetch_threads_scheduler(&self) -> Rc<RunOnceScheduler> {
        if let Some(scheduler) = self.fetch_threads_scheduler.borrow().as_ref() {
            return scheduler.clone();
        }
        let weak = self.me.clone();
        let inst = RunOnceScheduler::new(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.fetch_threads(None);
                }
            },
            100,
        );
        self.fetch_threads_scheduler.borrow_mut().replace(inst.clone());
        let to_cancel = inst.clone();
        self.raw_listeners.add(je_base::lifecycle::to_disposable(move || to_cancel.cancel()));
        inst
    }

    fn pass_focus_scheduler(&self) -> Rc<RunOnceScheduler> {
        self.pass_focus_scheduler.borrow().clone().expect("pass focus scheduler")
    }

    fn view_model(&self) -> Rc<ViewModel> {
        self.services.view_model()
    }

    fn has_session_focus(&self, session: &Rc<dyn IDebugSessionFull>) -> bool {
        is_same(session, &self.this())
    }

    //---- DAP requests

    fn terminate_inner(&self, restart: bool) -> Promise<()> {
        self.terminate(restart)
    }

    fn get_raw_source(&self, uri: &URI) -> ProtocolSource {
        match self.get_source_for_uri(uri) {
            Some(source) => source.raw.borrow().clone(),
            None => {
                let data = Source::get_encoded_debug_data(uri);
                ProtocolSource { name: Some(data.name), path: Some(data.path), source_reference: data.source_reference, ..ProtocolSource::default() }
            }
        }
    }

    fn get_new_cancellation_token(&self, thread_id: i64, token: Option<&CancellationToken>) -> CancellationToken {
        let token_source = CancellationTokenSource::new(token);
        let result = token_source.token();
        self.cancellation_map.borrow_mut().entry(thread_id).or_default().push(token_source);
        result
    }

    fn cancel_all_requests(&self) {
        let map = std::mem::take(&mut *self.cancellation_map.borrow_mut());
        for tokens in map.into_values() {
            for t in tokens {
                t.dispose_with(true);
            }
        }
    }

    fn wait_for_triggered_breakpoints(&self) -> Promise<()> {
        let pending = self.wait_to_resume.borrow().clone();
        match pending {
            None => Promise::resolved(()),
            Some(pending) => race_timeout(&pending, TRIGGERED_BREAKPOINT_MAX_DELAY, None).then(|_| ()),
        }
    }

    fn set_last_stepping_granularity(&self, thread_id: i64, granularity: Option<String>) {
        if let Some(thread) = self.get_thread(thread_id) {
            *thread.last_stepping_granularity.borrow_mut() = granularity;
        }
    }

    fn step_request(
        &self,
        what: &'static str,
        thread_id: i64,
        granularity: Option<String>,
        track_granularity: bool,
        send: impl FnOnce(Rc<dyn IRawDebugSession>) -> Promise<DapResponse> + 'static,
    ) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.wait_for_triggered_breakpoints().await?;
            let raw = this.raw_or_err(what)?;
            if track_granularity {
                this.set_last_stepping_granularity(thread_id, granularity);
            }
            send(raw).await?;
            Ok(())
        })
    }

    fn fetch_threads(&self, stopped_details: Option<StoppedDetails>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let raw = this.raw.borrow().clone();
            if let Some(raw) = raw {
                let response = raw.threads().await?;
                if let Some(threads) = response.and_then(|r| r.body).and_then(|b| b.get("threads").cloned()).and_then(|t| t.as_array().cloned()) {
                    this.model.raw_update(IRawModelUpdate {
                        session_id: this.get_id(),
                        threads: threads
                            .iter()
                            .map(|t| DapThread { id: number_of(t, "id").unwrap_or(0), name: text_of(t, "name").unwrap_or_default() })
                            .collect(),
                        stopped_details,
                    });
                }
            }
            Ok(())
        })
    }

    /// `initializeForTest(raw)`.
    pub fn initialize_for_test(&self, raw: Rc<dyn IRawDebugSession>) {
        *self.raw.borrow_mut() = Some(raw);
        self.register_listeners();
    }

    //---- private

    fn register_listeners(&self) {
        let Some(raw) = self.raw.borrow().clone() else {
            return;
        };
        let weak = self.me.clone();
        macro_rules! this_or_return {
            ($weak:expr) => {
                match $weak.upgrade() {
                    Some(this) => this,
                    None => return,
                }
            };
        }

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_initialize().listen(move |_| {
            let this = this_or_return!(w);
            this.on_initialized();
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_stop().listen(move |event| {
            let this = this_or_return!(w);
            this.handle_stop(new_stopped_details(IRawStoppedDetails::from_value(&body_of(event))));
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_thread().listen(move |event| {
            let this = this_or_return!(w);
            let body = body_of(event);
            let thread_id = number_of(&body, "threadId").unwrap_or(0);
            this.status_queue.cancel(Some(&[thread_id]));
            match text_of(&body, "reason").as_deref() {
                Some("started") => {
                    if !this.fetch_threads_scheduler().is_scheduled() {
                        this.fetch_threads_scheduler().schedule(None);
                    }
                }
                Some("exited") => {
                    this.model.clear_threads(&this.get_id(), true, Some(thread_id));
                    let view_model = this.view_model();
                    let focused_thread = view_model.focused_thread();
                    this.pass_focus_scheduler().cancel();
                    if focused_thread.is_some_and(|t| thread_id == t.thread_id) {
                        // De-focus the thread in case it was focused
                        this.services.focus_stack_frame(None, None, view_model.focused_session(), FocusOptions { explicit: Some(false), preserve_focus: None });
                    }
                }
                _ => {}
            }
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_terminate_debugee().listen(move |event| {
            let this = this_or_return!(w);
            let body = event.body.clone();
            async_fn(async move {
                this.services.aria_status(&localize("debuggingStopped", "Debugging stopped.", &[]));
                let restart = body.as_ref().and_then(|b| b.get("restart")).cloned().filter(|r| !matches!(r, Value::Null | Value::Bool(false)));
                if let Some(restart) = restart {
                    this.services.restart_session(this.this_full(), restart).await?;
                } else {
                    let raw = this.raw.borrow().clone();
                    if let Some(raw) = raw {
                        raw.disconnect(json!({ "terminateDebuggee": false })).await?;
                    }
                }
                Ok(())
            });
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_continued().listen(move |event| {
            let this = this_or_return!(w);
            this.on_continued(body_of(event));
        }));

        let output_queue: Rc<Queue<()>> = Rc::new(Queue::new());
        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_output().listen(move |event| {
            let this = this_or_return!(w);
            this.on_output(body_of(event), output_queue.clone());
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_breakpoint().listen(move |event| {
            let this = this_or_return!(w);
            this.on_breakpoint(body_of(event));
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_loaded_source().listen(move |event| {
            let this = this_or_return!(w);
            let body = body_of(event);
            let reason = match text_of(&body, "reason").as_deref() {
                Some("changed") => LoadedSourceReason::Changed,
                Some("removed") => LoadedSourceReason::Removed,
                _ => LoadedSourceReason::New,
            };
            let source = this.get_source(body.get("source").filter(|s| s.is_object()).map(protocol_source_from_value));
            this.on_did_loaded_source.fire(LoadedSourceEvent { reason, source });
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_custom_event().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_custom_event.fire(event.clone());
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_progress_start().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_progress_start.fire(event.clone());
        }));
        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_progress_update().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_progress_update.fire(event.clone());
        }));
        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_progress_end().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_progress_end.fire(event.clone());
        }));
        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_invalidate_memory().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_invalid_memory.fire(event.clone());
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_invalidated().listen(move |event| {
            let this = this_or_return!(w);
            let body = body_of(event);
            async_fn(async move {
                let areas: Vec<String> = body
                    .get("areas")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_else(|| vec!["all".to_owned()]);
                // If invalidated event only requires to update variables or watch, do that, otherwise refetch threads https://github.com/microsoft/vscode/issues/106745
                if areas.iter().any(|a| a == "threads" || a == "stacks" || a == "all") {
                    this.cancel_all_requests();
                    this.model.clear_threads(&this.get_id(), true, None);

                    let details: Vec<StoppedDetails> = std::mem::take(&mut *this.stopped_details.borrow_mut());
                    if !details.is_empty() {
                        let handles: Vec<Promise<()>> = details.iter().map(|d| this.handle_stop(d.clone())).collect();
                        Promise::all(handles).await?;
                    } else if !this.fetch_threads_scheduler().is_scheduled() {
                        // threads are fetched as a side-effect of processing the stopped event(s), but if there are none, schedule a thread update manually (#282777)
                        this.fetch_threads_scheduler().schedule(None);
                    }
                }

                let view_model = this.view_model();
                if view_model.focused_session().is_some_and(|s| this.has_session_focus(&s)) {
                    view_model.update_views();
                }
                Ok(())
            });
        }));

        let w = weak.clone();
        self.raw_listeners.add(raw.on_did_exit_adapter().listen(move |event| {
            let this = this_or_return!(w);
            this.on_did_exit_adapter(Some(event.clone()));
        }));
    }

    fn on_initialized(&self) {
        let this = self.this();
        async_fn(async move {
            this.services.aria_status(&if this.configuration().get("noDebug").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false))) {
                localize("debuggingStartedNoDebug", "Started running without debugging.", &[])
            } else {
                localize("debuggingStarted", "Debugging started.", &[])
            });

            // Send all breakpoints
            let send_result = this.services.send_all_breakpoints(this.this_full()).await;
            // finally
            let configuration_done = {
                let raw = this.raw.borrow().clone();
                match raw {
                    Some(raw) if capability(&raw.capabilities(), "supportsConfigurationDoneRequest") => match raw.configuration_done().await {
                        Ok(_) => None,
                        Err(e) => {
                            // Disconnect the debug session on configuration done error #10596
                            this.services.notify_error(&e.message);
                            if let Some(raw) = this.raw.borrow().clone() {
                                raw.disconnect(json!({}));
                            }
                            None
                        }
                    },
                    _ => None::<Error>,
                }
            };
            let _ = configuration_done;
            let fetch = this.fetch_threads(None).await;
            send_result?;
            fetch
        });
    }

    fn on_continued(&self, body: Value) {
        let this = self.this();
        async_fn(async move {
            let thread_id = number_of(&body, "threadId").unwrap_or(0);
            let all_threads = body.get("allThreadsContinued").and_then(Value::as_bool) != Some(false);
            {
                let mut details = this.stopped_details.borrow_mut();
                if all_threads {
                    details.clear();
                } else {
                    details.retain(|sd| sd.borrow().thread_id != Some(thread_id));
                }
            }

            let affected_threads: Promise<Vec<i64>> = if !all_threads {
                if this.thread_ids.borrow().contains(&thread_id) {
                    Promise::resolved(vec![thread_id])
                } else {
                    if let Some(scheduler) = this.fetch_threads_scheduler.borrow().as_ref() {
                        scheduler.cancel();
                    }
                    this.fetch_threads(None).then(move |_| vec![thread_id])
                }
            } else if this.fetch_threads_scheduler().is_scheduled() {
                this.fetch_threads_scheduler().cancel();
                let inner = this.clone();
                this.fetch_threads(None).then(move |_| inner.thread_ids.borrow().clone())
            } else {
                Promise::resolved(this.thread_ids.borrow().clone())
            };

            this.status_queue.cancel(if all_threads { None } else { Some(&[thread_id][..]) });
            let operation_this = this.clone();
            this.status_queue
                .run(
                    affected_threads,
                    Rc::new(move |thread_id, _token| {
                        let tokens = operation_this.cancellation_map.borrow_mut().remove(&thread_id);
                        for t in tokens.into_iter().flatten() {
                            t.dispose_with(true);
                        }
                        operation_this.model.clear_threads(&operation_this.get_id(), false, Some(thread_id));
                        Promise::resolved(())
                    }),
                )
                .await?;

            // We need to pass focus to other sessions / threads with a timeout in case a quick stop event occurs #130321
            this.last_continued_thread_id.set(if all_threads { None } else { Some(thread_id) });
            this.pass_focus_scheduler().schedule(None);
            this.on_did_change_state.fire(());
            Ok(())
        });
    }

    fn on_output(&self, body: Value, output_queue: Rc<Queue<()>>) {
        let this = self.this();
        let category = text_of(&body, "category");
        let output_severity = match category.as_deref() {
            Some("stderr") => Severity::Error,
            Some("console") => Severity::Warning,
            _ => Severity::Info,
        };
        let is_important = category.as_deref() == Some("important");
        let source_of = |this: &Rc<DebugSession>, body: &Value| -> Option<IReplElementSource> {
            let source = body.get("source").filter(|s| s.is_object())?;
            let line = number_of(body, "line").filter(|l| *l != 0)?;
            Some(IReplElementSource {
                line_number: line,
                column: number_of(body, "column").filter(|c| *c != 0).unwrap_or(1),
                source: this.get_source(Some(protocol_source_from_value(source))),
            })
        };

        // When a variables event is received, execute immediately to obtain the variables value #126967
        if let Some(variables_reference) = number_of(&body, "variablesReference").filter(|r| *r != 0) {
            let source = source_of(&this, &body);
            let container = ExpressionContainer::with_defaults(Some(this.this_full()), None, Some(variables_reference), generate_uuid());
            let children = container.get_children();
            // we should put appendToRepl into queue to make sure the logs to be displayed in correct order
            // see https://github.com/microsoft/vscode/issues/126967#issuecomment-874954269
            let output = text_of(&body, "output").unwrap_or_default();
            output_queue.queue(move || {
                async_fn(async move {
                    let resolved = children.await?;
                    // For single logged variables, try to use the output if we can so present a better (i.e. ANSI-aware) representation of the output
                    if resolved.len() == 1 {
                        this.append_to_repl(
                            NewReplElementData { output, expression: Some(resolved[0].clone()), sev: output_severity, source },
                            is_important,
                        );
                        return Ok(());
                    }

                    for child in resolved {
                        // Since we can not display multiple trees in a row, we are displaying these variables one after the other (ignoring their names)
                        this.append_to_repl(
                            NewReplElementData { output: String::new(), expression: Some(child), sev: output_severity, source: source.clone() },
                            is_important,
                        );
                    }
                    Ok(())
                })
            });
            return;
        }

        output_queue.queue(move || {
            async_fn(async move {
                let Some(raw) = this.raw.borrow().clone() else {
                    return Ok(());
                };
                if body.is_null() {
                    return Ok(());
                }

                if category.as_deref() == Some("telemetry") {
                    // only log telemetry events from debug adapter if the debug extension provided the telemetry key
                    // and the user opted in telemetry
                    let telemetry_endpoint = raw.dbgr().and_then(|dbgr| dbgr.get_custom_telemetry_endpoint());
                    if let Some(telemetry_endpoint) = telemetry_endpoint
                        && this.services.telemetry_enabled()
                    {
                        let mut data = body.get("data").cloned();
                        if !telemetry_endpoint.send_error_telemetry
                            && let Some(Value::Object(object)) = &data
                        {
                            data = Some(Value::Object(filter_exceptions_from_telemetry(object)));
                        }

                        this.services.publish_custom_endpoint_telemetry(&telemetry_endpoint, &text_of(&body, "output").unwrap_or_default(), data);
                    }

                    return Ok(());
                }

                // Make sure to append output in the correct order by properly waiting on preivous promises #33822
                let source = source_of(&this, &body);

                let group = text_of(&body, "group");
                if group.as_deref() == Some("start") || group.as_deref() == Some("startCollapsed") {
                    let expanded = group.as_deref() == Some("start");
                    this.repl.borrow().start_group(&this.this_full(), &text_of(&body, "output").unwrap_or_default(), expanded, source);
                    return Ok(());
                }
                if group.as_deref() == Some("end") {
                    this.repl.borrow().end_group();
                    if text_of(&body, "output").is_none_or(|o| o.is_empty()) {
                        // Only return if the end event does not have additional output in it
                        return Ok(());
                    }
                }

                if let Some(output) = text_of(&body, "output") {
                    this.append_to_repl(NewReplElementData { output, expression: None, sev: output_severity, source }, is_important);
                }
                Ok(())
            })
        });
    }

    fn on_breakpoint(&self, body: Value) {
        let breakpoint_body = body.get("breakpoint").filter(|b| b.is_object()).map(DapBreakpoint::from_value);
        let id = breakpoint_body.as_ref().and_then(|b| b.id);
        let session_id = self.get_id();
        let matches = |adapter_id: Option<i64>| adapter_id == id;
        let breakpoint = self.model.get_breakpoints(None).into_iter().find(|bp| matches(bp.base.get_id_from_adapter(&session_id)));
        let function_breakpoint = self.model.get_function_breakpoints().into_iter().find(|bp| matches(bp.base.get_id_from_adapter(&session_id)));
        let data_breakpoint = self.model.get_data_breakpoints().into_iter().find(|bp| matches(bp.base.get_id_from_adapter(&session_id)));
        let exception_breakpoint = self.model.get_exception_breakpoints().into_iter().find(|bp| matches(bp.base.get_id_from_adapter(&session_id)));
        let capabilities = self.capabilities();
        let reason = text_of(&body, "reason");

        let set_data = |id: String, breakpoint: DapBreakpoint| {
            let data = HashMap::from([(id, breakpoint)]);
            self.model.set_breakpoint_session_data(&session_id, &capabilities, Some(&data));
        };

        if reason.as_deref() == Some("new")
            && let Some(breakpoint_body) = &breakpoint_body
            && let Some(source) = &breakpoint_body.source
            && breakpoint_body.line.is_some_and(|l| l != 0)
        {
            let source = self.get_source(Some(source.clone()));
            let bps = self.model.add_breakpoints(
                &source.uri,
                vec![super::debug_model::BreakpointData {
                    column: breakpoint_body.column,
                    enabled: Some(true),
                    line_number: breakpoint_body.line.unwrap_or(0),
                    ..Default::default()
                }],
                false,
            );
            if bps.len() == 1 {
                set_data(bps[0].get_id(), breakpoint_body.clone());
            }
        }

        if reason.as_deref() == Some("removed") {
            if let Some(breakpoint) = &breakpoint {
                self.model.remove_breakpoints(std::slice::from_ref(breakpoint));
            }
            if let Some(function_breakpoint) = &function_breakpoint {
                self.model.remove_function_breakpoints(Some(&function_breakpoint.get_id()));
            }
            if let Some(data_breakpoint) = &data_breakpoint {
                self.model.remove_data_breakpoints(Some(&data_breakpoint.get_id()));
            }
        }

        if reason.as_deref() == Some("changed") {
            if let Some(breakpoint) = &breakpoint {
                let mut changed = breakpoint_body.clone().unwrap_or_default();
                if breakpoint.column().is_none_or(|c| c == 0) {
                    changed.column = None;
                }
                set_data(breakpoint.get_id(), changed);
            }
            if let Some(function_breakpoint) = &function_breakpoint {
                set_data(function_breakpoint.get_id(), breakpoint_body.clone().unwrap_or_default());
            }
            if let Some(data_breakpoint) = &data_breakpoint {
                set_data(data_breakpoint.get_id(), breakpoint_body.clone().unwrap_or_default());
            }
            if let Some(exception_breakpoint) = &exception_breakpoint {
                set_data(exception_breakpoint.get_id(), breakpoint_body.clone().unwrap_or_default());
            }
        }
    }

    fn handle_stop(&self, shared_event: StoppedDetails) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.pass_focus_scheduler().cancel();
            let event = shared_event.borrow().clone();
            this.stopped_details.borrow_mut().push(shared_event.clone());

            // do this very eagerly if we have hitBreakpointIds, since it may take a moment for breakpoints to set and we want
            // to do our best to not miss anything
            if let Some(ids) = &event.hit_breakpoint_ids {
                *this.wait_to_resume.borrow_mut() = Some(this.enable_dependent_breakpoints_by_ids(ids.clone()));
            }

            let thread_ids = {
                let event = event.clone();
                let inner = this.clone();
                this.fetch_threads(Some(shared_event.clone())).then(move |_| match event.thread_id {
                    None => inner.thread_ids.borrow().clone(),
                    Some(id) => vec![id],
                })
            };
            let operation_this = this.clone();
            let operation_event = event.clone();
            this.status_queue.run(
                thread_ids,
                Rc::new(move |thread_id, token| {
                    let this = operation_this.clone();
                    let event = operation_event.clone();
                    async_fn(async move {
                        let has_lots_of_threads = event.thread_id.is_none() && this.thread_ids.borrow().len() > 10;

                        // If the focus for the current session is on a non-existent thread, clear the focus.
                        let focused_thread = this.view_model().focused_thread();
                        let focused_thread_does_not_exist = focused_thread
                            .as_ref()
                            .is_some_and(|t| this.has_session_focus(&t.session) && !this.threads.borrow().contains_key(&t.thread_id));
                        if focused_thread_does_not_exist {
                            this.services.focus_stack_frame(None, None, None, FocusOptions::default());
                        }

                        let thread = this.get_thread(thread_id);
                        if let Some(thread) = thread {
                            // Call fetch call stack twice, the first only return the top stack frame.
                            // Second retrieves the rest of the call stack. For performance reasons #25605
                            // Second call is only done if there's few threads that stopped in this event.
                            let promises = this.model.refresh_top_of_callstack(thread.clone(), !has_lots_of_threads);
                            let focus: Rc<dyn Fn()> = {
                                let this = this.clone();
                                let thread = thread.clone();
                                let event = event.clone();
                                let token = token.clone();
                                Rc::new(move || {
                                    let this = this.clone();
                                    let thread = thread.clone();
                                    let event = event.clone();
                                    let token = token.clone();
                                    async_fn(async move {
                                        if focused_thread_does_not_exist || (event.preserve_focus_hint != Some(true) && !thread.get_call_stack().is_empty()) {
                                            let focused_stack_frame = this.view_model().focused_stack_frame();
                                            if focused_stack_frame.as_ref().is_none_or(|f| this.has_session_focus(&f.thread.session)) {
                                                // Only take focus if nothing is focused, or if the focus is already on the current session
                                                let preserve_focus = !this.services.debug_configuration().focus_editor_on_break;
                                                this.services
                                                    .focus_stack_frame(None, Some(thread.clone()), None, FocusOptions { explicit: None, preserve_focus: Some(preserve_focus) })
                                                    .await?;
                                            }

                                            if let Some(details) = thread.stopped_details()
                                                && !token.is_cancellation_requested()
                                            {
                                                let configuration = this.services.debug_configuration();
                                                if details.reason.as_deref() == Some("breakpoint")
                                                    && configuration.open_debug == "openOnDebugBreak"
                                                    && !this.suppress_debug_view()
                                                {
                                                    this.services.open_debug_view().await?;
                                                }

                                                if configuration.focus_window_on_break && !this.services.extension_tests_location_uri_is_set() && !this.services.window_has_focus() {
                                                    this.services.focus_window().await?;
                                                }
                                            }
                                        }
                                        Ok(())
                                    });
                                })
                            };

                            promises.top_call_stack.await?;

                            if event.hit_breakpoint_ids.is_none() {
                                // if hitBreakpointIds are present, this is handled earlier on
                                *this.wait_to_resume.borrow_mut() = Some(this.enable_dependent_breakpoints_by_thread(thread.clone()));
                            }

                            if token.is_cancellation_requested() {
                                return Ok(());
                            }

                            focus();

                            promises.whole_call_stack.await?;
                            if token.is_cancellation_requested() {
                                return Ok(());
                            }

                            let focused_stack_frame = this.view_model().focused_stack_frame();
                            if focused_stack_frame.as_ref().is_none_or(|f| is_frame_deemphasized(f)) {
                                // The top stack frame can be deemphesized so try to focus again #68616
                                focus();
                            }
                        }
                        this.on_did_change_state.fire(());
                        Ok(())
                    })
                }),
            );
            Ok(())
        })
    }

    fn enable_dependent_breakpoints_by_ids(&self, hit_breakpoint_ids: Vec<i64>) -> Promise<()> {
        let session_id = self.id.clone();
        let breakpoints: Vec<Rc<Breakpoint>> =
            self.model.get_breakpoints(None).into_iter().filter(|bp| bp.base.get_id_from_adapter(&session_id).is_some_and(|id| hit_breakpoint_ids.contains(&id))).collect();
        self.enable_dependent_breakpoints(breakpoints)
    }

    fn enable_dependent_breakpoints_by_thread(&self, thread: Rc<Thread>) -> Promise<()> {
        let Some(frame) = thread.get_top_stack_frame() else {
            return Promise::resolved(());
        };

        if thread.stopped_details().is_some_and(|d| d.reason.as_deref() != Some("breakpoint")) {
            return Promise::resolved(());
        }

        let range = frame.range.expect("o quadro de pilha tem intervalo");
        let breakpoints = self.get_breakpoints_at_position(
            &frame.source.uri,
            range.start_line_number as i64,
            range.end_line_number as i64,
            range.start_column as i64,
            range.end_column as i64,
        );
        self.enable_dependent_breakpoints(breakpoints)
    }

    fn enable_dependent_breakpoints(&self, breakpoints: Vec<Rc<Breakpoint>>) -> Promise<()> {
        // find the current breakpoints

        // check if the current breakpoints are dependencies, and if so collect and send the dependents to DA
        let mut uris_to_resend: Vec<String> = Vec::new();
        for bp in self.model.get_breakpoints(Some(&BreakpointFilter { triggered_only: true, enabled_only: true, ..BreakpointFilter::default() })) {
            for cbp in &breakpoints {
                if bp.enabled() && bp.triggered_by.borrow().as_deref() == Some(cbp.get_id().as_str()) {
                    bp.set_session_did_trigger(&self.get_id(), true);
                    let uri = bp.uri().to_string(false).to_string_lossy();
                    if !uris_to_resend.contains(&uri) {
                        uris_to_resend.push(uri);
                    }
                }
            }
        }

        let results: Vec<Promise<()>> =
            uris_to_resend.iter().map(|uri| self.services.send_breakpoints(URI::parse(uri, false), None, self.this_full())).collect();
        Promise::all(results).then(|_| ())
    }

    fn get_breakpoints_at_position(&self, uri: &URI, start_line_number: i64, end_line_number: i64, start_column: i64, end_column: i64) -> Vec<Rc<Breakpoint>> {
        self.model
            .get_breakpoints(Some(&BreakpointFilter { uri: Some(uri.clone()), ..BreakpointFilter::default() }))
            .into_iter()
            .filter(|bp| {
                if bp.line_number() < start_line_number || bp.line_number() > end_line_number {
                    return false;
                }

                if bp.column().is_some_and(|c| c != 0 && (c < start_column || c > end_column)) {
                    return false;
                }
                true
            })
            .collect()
    }

    fn on_did_exit_adapter(&self, event: Option<AdapterEndEvent>) {
        self.initialized.set(true);
        self.model.set_breakpoint_session_data(&self.get_id(), &self.capabilities(), None);
        self.shutdown();
        self.on_did_end_adapter.fire(event);
    }

    // Disconnects and clears state. Session can be initialized again for a new connection.
    fn shutdown(&self) {
        self.raw_listeners.clear();
        let raw = self.raw.borrow_mut().take();
        if let Some(raw) = raw {
            // Send out disconnect and immediatly dispose (do not wait for response) #127418
            raw.disconnect(json!({}));
            raw.dispose();
        }
        self.pass_focus_scheduler().cancel();
        self.pass_focus_scheduler().cancel();
        self.model.clear_threads(&self.get_id(), true, None);
        self.sources.borrow_mut().clear();
        self.threads.borrow_mut().clear();
        self.thread_ids.borrow_mut().clear();
        self.stopped_details.borrow_mut().clear();
        self.on_did_change_state.fire(());
    }

    pub fn append_to_repl(&self, data: NewReplElementData, is_important: bool) {
        let output = data.output.clone();
        let sev = data.sev;
        self.repl.borrow().append_to_repl(&self.this_full(), data);
        if is_important {
            self.services.notify(&output, sev, &self.name());
        }
    }
}

impl IDebugSession for DebugSession {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn configuration(&self) -> IConfig {
        self.configuration.borrow().0.clone()
    }

    fn name(&self) -> String {
        self.name.borrow().clone().filter(|n| !n.is_empty()).unwrap_or_else(|| self.configuration().get("name").and_then(Value::as_str).unwrap_or_default().to_owned())
    }

    fn set_name(&self, name: &str) {
        *self.name.borrow_mut() = Some(name.to_owned());
        self.on_did_change_name.fire(name.to_owned());
    }

    fn root(&self) -> Option<WorkspaceFolder> {
        self.root.clone()
    }

    fn parent_session(&self) -> Option<Rc<dyn IDebugSession>> {
        self.parent_session.borrow().clone().map(|p| p as Rc<dyn IDebugSession>)
    }

    fn compound_root(&self) -> Option<Rc<DebugCompoundRoot>> {
        self.options.compound_root.clone()
    }

    fn on_did_change_name(&self) -> Event<String> {
        self.on_did_change_name.event()
    }

    fn on_did_custom_event(&self) -> Event<ProtocolEvent> {
        self.on_did_custom_event.event()
    }

    fn append_to_repl(&self, data: INewReplElementData) {
        DebugSession::append_to_repl(self, NewReplElementData { output: data.output, expression: None, sev: data.sev, source: None }, false);
    }

    fn custom_request(&self, request: &str, args: Option<Value>) -> Promise<Option<Response>> {
        match self.raw_or_err(request) {
            Ok(raw) => raw.custom(request, args.unwrap_or(Value::Null)),
            Err(error) => Promise::rejected(error),
        }
    }

    fn get_debug_protocol_breakpoint(&self, breakpoint_id: &str) -> Option<Value> {
        self.model.get_debug_protocol_breakpoint(breakpoint_id, &self.get_id()).map(|bp| {
            let mut object = serde_json::Map::new();
            if let Some(id) = bp.id {
                object.insert("id".into(), json!(id));
            }
            object.insert("verified".into(), json!(bp.verified));
            if let Some(message) = bp.message {
                object.insert("message".into(), json!(message));
            }
            if let Some(source) = &bp.source {
                object.insert("source".into(), protocol_source_to_value(source));
            }
            for (key, value) in [("line", bp.line), ("column", bp.column), ("endLine", bp.end_line), ("endColumn", bp.end_column), ("offset", bp.offset)] {
                if let Some(value) = value {
                    object.insert(key.into(), json!(value));
                }
            }
            if let Some(reference) = bp.instruction_reference {
                object.insert("instructionReference".into(), json!(reference));
            }
            Value::Object(object)
        })
    }
}

impl IDisposable for DebugSession {
    fn dispose(&self) {
        self.cancel_all_requests();
        self.raw_listeners.dispose();
        self.global_disposables.dispose();
        if let Some(listener) = self.repl_listener.borrow_mut().take() {
            listener.dispose();
        }
        if let Some(scheduler) = self.pass_focus_scheduler.borrow().as_ref() {
            scheduler.cancel();
        }
        *self.wait_to_resume.borrow_mut() = None;
    }
}

impl IDebugSessionFull for DebugSession {
    fn state(&self) -> State {
        if let Some(state) = *self.state_override.borrow() {
            return state;
        }
        if !self.initialized.get() {
            return State::Initializing;
        }
        if self.raw.borrow().is_none() {
            return State::Inactive;
        }

        let focused_thread = self.view_model().focused_thread();
        if let Some(focused_thread) = focused_thread
            && self.has_session_focus(&focused_thread.session)
        {
            return if focused_thread.stopped.get() { State::Stopped } else { State::Running };
        }
        if self.threads.borrow().values().any(|t| t.stopped.get()) {
            return State::Stopped;
        }

        State::Running
    }

    fn unresolved_configuration(&self) -> Option<IConfig> {
        self.configuration.borrow().1.clone()
    }

    fn sub_id(&self) -> Option<String> {
        self.sub_id.borrow().clone()
    }

    fn set_sub_id(&self, sub_id: Option<String>) {
        *self.sub_id.borrow_mut() = sub_id;
    }

    fn compact(&self) -> bool {
        self.options.compact.unwrap_or(false)
    }

    fn save_before_restart(&self) -> bool {
        self.options.save_before_restart.unwrap_or(self.options.parent_session.is_none())
    }

    fn auto_expand_lazy_variables(&self) -> bool {
        // This tiny helper avoids converting the entire debug model to use service injection
        let screen_reader_optimized = self.services.is_screen_reader_optimized();
        let value = self.services.debug_configuration().auto_expand_lazy_variables;
        (value == "auto" && screen_reader_optimized) || value == "on"
    }

    fn suppress_debug_toolbar(&self) -> bool {
        self.options.suppress_debug_toolbar.unwrap_or(false)
    }

    fn suppress_debug_statusbar(&self) -> bool {
        self.options.suppress_debug_statusbar.unwrap_or(false)
    }

    fn suppress_debug_view(&self) -> bool {
        self.options.suppress_debug_view.unwrap_or(false)
    }

    fn lifecycle_managed_by_parent(&self) -> bool {
        self.options.lifecycle_managed_by_parent.unwrap_or(false)
    }

    fn get_label(&self) -> String {
        let include_root = self.services.workspace_folder_count() > 1;
        match (&self.root, include_root) {
            (Some(root), true) => format!("{} ({})", self.name(), ext_uri().basename_or_authority(&root.uri)),
            _ => self.name(),
        }
    }

    fn get_memory(&self, memory_reference: &str) -> Rc<MemoryRegion> {
        MemoryRegion::new(memory_reference, self.this_full())
    }

    fn get_source_for_uri(&self, model_uri: &URI) -> Option<Rc<Source>> {
        let key = self.services.uri_identity_service().as_canonical_uri(model_uri).to_string(false).to_string_lossy();
        self.sources.borrow().get(&key).cloned()
    }

    fn get_source(&self, raw: Option<ProtocolSource>) -> Rc<Source> {
        let source = Rc::new(Source::new(raw.clone(), &self.get_id(), &*self.services.uri_identity_service(), &*self.services.log_service()));
        let uri_key = source.uri.to_string(false).to_string_lossy();
        let found = self.sources.borrow().get(&uri_key).cloned();
        match found {
            Some(found) => {
                // merge attributes of new into existing
                if let Some(raw) = raw {
                    let mut existing = found.raw.borrow_mut();
                    if raw.name.is_some() {
                        existing.name = raw.name.clone();
                    }
                    if raw.path.is_some() {
                        existing.path = raw.path.clone();
                    }
                    if raw.source_reference.is_some() {
                        existing.source_reference = raw.source_reference;
                    }
                    if raw.origin.is_some() {
                        existing.origin = raw.origin.clone();
                    }
                    if raw.sources.is_some() {
                        existing.sources = raw.sources.clone();
                    }
                    if raw.adapter_data.is_some() {
                        existing.adapter_data = raw.adapter_data.clone();
                    }
                    if raw.checksums.is_some() {
                        existing.checksums = raw.checksums.clone();
                    }
                    // Always take the latest presentation hint from adapter #42139
                    existing.presentation_hint = raw.presentation_hint;
                }
                found
            }
            None => {
                self.sources.borrow_mut().insert(uri_key, source.clone());
                source
            }
        }
    }

    fn set_configuration(&self, resolved: IConfig, unresolved: Option<IConfig>) {
        *self.configuration.borrow_mut() = (resolved, unresolved);
    }

    //---- threads

    fn raw_update(&self, data: IRawModelUpdate) {
        self.thread_ids.borrow_mut().clear();
        for thread in &data.threads {
            self.thread_ids.borrow_mut().push(thread.id);
            let existing = self.threads.borrow().get(&thread.id).cloned();
            match existing {
                None => {
                    // A new thread came in, initialize it.
                    self.threads.borrow_mut().insert(thread.id, Thread::new(self.this_full(), &thread.name, thread.id));
                }
                Some(old_thread) => {
                    if !thread.name.is_empty() {
                        // Just the thread name got updated #18244
                        *old_thread.name.borrow_mut() = thread.name.clone();
                    }
                }
            }
        }
        // Remove all old threads which are no longer part of the update #75980
        let ids = self.thread_ids.borrow().clone();
        self.threads.borrow_mut().retain(|id, _| ids.contains(id));

        if let Some(shared) = data.stopped_details {
            let stopped_details = shared.borrow().clone();
            // Set the availability of the threads' callstacks depending on whether the thread is stopped or not
            if stopped_details.all_threads_stopped == Some(true) {
                for thread in self.threads.borrow().values() {
                    if Some(thread.thread_id) == stopped_details.thread_id {
                        thread.set_stopped_details_ref(Some(shared.clone()));
                    } else {
                        let reason = thread.stopped_details().and_then(|d| d.reason);
                        thread.set_stopped_details(Some(IRawStoppedDetails { reason, ..IRawStoppedDetails::default() }));
                    }
                    thread.stopped.set(true);
                    thread.clear_call_stack();
                }
            } else {
                let thread = stopped_details.thread_id.and_then(|id| self.threads.borrow().get(&id).cloned());
                if let Some(thread) = thread {
                    // One thread is stopped, only update that thread.
                    thread.set_stopped_details_ref(Some(shared));
                    thread.clear_call_stack();
                    thread.stopped.set(true);
                }
            }
        }
    }

    fn get_thread(&self, thread_id: i64) -> Option<Rc<Thread>> {
        self.threads.borrow().get(&thread_id).cloned()
    }

    fn get_all_threads(&self) -> Vec<Rc<Thread>> {
        let threads = self.threads.borrow();
        self.thread_ids.borrow().iter().filter_map(|id| threads.get(id).cloned()).collect()
    }

    fn clear_threads(&self, remove_threads: bool, reference: Option<i64>) {
        match reference {
            Some(reference) => {
                let thread = self.threads.borrow().get(&reference).cloned();
                if let Some(thread) = thread {
                    thread.clear_call_stack();
                    thread.set_stopped_details(None);
                    thread.stopped.set(false);

                    if remove_threads {
                        self.threads.borrow_mut().shift_remove(&reference);
                    }
                }
            }
            None => {
                for thread in self.threads.borrow().values() {
                    thread.clear_call_stack();
                    thread.set_stopped_details(None);
                    thread.stopped.set(false);
                }

                if remove_threads {
                    self.threads.borrow_mut().clear();
                    self.thread_ids.borrow_mut().clear();
                    super::debug_model::clear_all_values();
                }
            }
        }
    }

    fn get_stopped_details(&self) -> Option<StoppedDetails> {
        self.stopped_details.borrow().first().cloned()
    }

    //---- REPL

    fn repl_model(&self) -> Rc<ReplModel> {
        self.repl.borrow().clone()
    }

    fn get_repl_elements(&self) -> Vec<ReplElement> {
        self.repl.borrow().get_repl_elements()
    }

    fn has_separate_repl(&self) -> bool {
        self.parent_session.borrow().is_none() || self.options.repl != Some(IDebugSessionReplMode::MergeWithParent)
    }

    fn remove_repl_expressions(&self) {
        self.repl.borrow().remove_repl_expressions();
    }

    fn add_repl_expression(&self, stack_frame: Option<Rc<StackFrame>>, name: &str) -> Promise<()> {
        let this = self.this();
        let name = name.to_owned();
        async_fn(async move {
            let repl = this.repl.borrow().clone();
            repl.add_repl_expression(this.this_full(), stack_frame, &name).await?;
            // Evaluate all watch expressions and fetch variables again since repl evaluation might have changed some.
            this.view_model().update_views();
            Ok(())
        })
    }

    fn append_to_repl_data(&self, data: NewReplElementData, is_important: bool) {
        DebugSession::append_to_repl(self, data, is_important);
    }

    /// Terminate any linked test run (sem execução de teste correlata: ver as notas do módulo).
    fn cancel_correlated_test_run(&self) {}

    fn on_did_end_adapter(&self) -> Event<Option<AdapterEndEvent>> {
        self.on_did_end_adapter.event()
    }

    fn on_did_change_state(&self) -> Event<()> {
        self.on_did_change_state.event()
    }

    fn on_did_change_repl_elements(&self) -> Event<Option<ReplElement>> {
        self.on_did_change_repl_elements.event()
    }

    fn capabilities(&self) -> Value {
        match self.raw.borrow().as_ref() {
            Some(raw) => raw.capabilities(),
            None => json!({}),
        }
    }

    fn remembered_capabilities(&self) -> Option<Value> {
        self.remembered_capabilities.borrow().clone()
    }

    fn on_did_loaded_source(&self) -> Event<LoadedSourceEvent> {
        self.on_did_loaded_source.event()
    }

    fn on_did_progress_start(&self) -> Event<ProtocolEvent> {
        self.on_did_progress_start.event()
    }

    fn on_did_progress_update(&self) -> Event<ProtocolEvent> {
        self.on_did_progress_update.event()
    }

    fn on_did_progress_end(&self) -> Event<ProtocolEvent> {
        self.on_did_progress_end.event()
    }

    fn on_did_invalidate_memory(&self) -> Event<ProtocolEvent> {
        self.on_did_invalid_memory.event()
    }

    //---- DAP requests

    /// create and initialize a new debug adapter for this session
    fn initialize(&self, dbgr: Rc<dyn IDebugger>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if this.raw.borrow().is_some() {
                // if there was already a connection make sure to remove old listeners
                this.shutdown();
                Promise::resolved(()).await?;
            }

            let outcome: Result<(), Error> = async {
                let debug_adapter = dbgr.create_debug_adapter(this.this_full()).await?;
                let configuration = this.configuration();
                let raw = this.services.create_raw_session(debug_adapter, dbgr.clone(), &this.id, configuration.get("name").and_then(Value::as_str).unwrap_or_default());
                *this.raw.borrow_mut() = Some(raw.clone());

                raw.start().await?;
                this.register_listeners();
                raw.initialize(json!({
                    "clientID": "vscode",
                    "clientName": this.services.product_name_long(),
                    "adapterID": configuration.get("type").cloned().unwrap_or(Value::Null),
                    "pathFormat": "path",
                    "linesStartAt1": true,
                    "columnsStartAt1": true,
                    "supportsVariableType": true, // #8858
                    "supportsVariablePaging": true, // #9537
                    "supportsRunInTerminalRequest": true, // #10574
                    "locale": this.services.locale(), // #169114
                    "supportsProgressReporting": true, // #92253
                    "supportsInvalidatedEvent": true, // #106745
                    "supportsMemoryReferences": true, //#129684
                    "supportsArgsCanBeInterpretedByShell": true, // #149910
                    "supportsMemoryEvent": true, // #133643
                    "supportsStartDebuggingRequest": true,
                    "supportsANSIStyling": true,
                }))
                .await?;

                this.initialized.set(true);
                this.on_did_change_state.fire(());
                let capabilities = raw.capabilities();
                *this.remembered_capabilities.borrow_mut() = Some(capabilities.clone());
                let filters: Vec<ExceptionBreakpointsFilter> = capabilities
                    .get("exceptionBreakpointFilters")
                    .and_then(Value::as_array)
                    .map(|filters| filters.iter().map(ExceptionBreakpointsFilter::from_value).collect())
                    .unwrap_or_default();
                this.services.set_exception_breakpoints_for_session(this.this_full(), filters);
                let modes: Vec<BreakpointMode> = capabilities
                    .get("breakpointModes")
                    .and_then(Value::as_array)
                    .map(|modes| {
                        modes
                            .iter()
                            .map(|m| BreakpointMode {
                                mode: text_of(m, "mode").unwrap_or_default(),
                                label: text_of(m, "label").unwrap_or_default(),
                                description: text_of(m, "description"),
                                applies_to: m
                                    .get("appliesTo")
                                    .and_then(Value::as_array)
                                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                                    .unwrap_or_default(),
                                first_from_debug_type: String::new(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                this.model.register_breakpoint_modes(configuration.get("type").and_then(Value::as_str).unwrap_or_default(), &modes);
                Ok(())
            }
            .await;

            if let Err(err) = outcome {
                this.initialized.set(true);
                this.on_did_change_state.fire(());
                this.shutdown();
                Promise::resolved(()).await?;
                return Err(err);
            }
            Ok(())
        })
    }

    /// launch or attach to the debuggee
    fn launch_or_attach(&self, mut config: IConfig) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("launch or attach"));
            };
            if let Some(parent) = this.parent_session.borrow().clone()
                && parent.state() == State::Inactive
            {
                return Err(Error::cancellation());
            }

            // __sessionID only used for EH debugging (but we add it always for now...)
            config["__sessionId"] = json!(this.get_id());
            if let Err(err) = raw.launch_or_attach(config).await {
                this.shutdown();
                return Err(err);
            }
            Ok(())
        })
    }

    /// terminate the current debug adapter session
    fn terminate(&self, restart: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if this.raw.borrow().is_none() {
                // Adapter went down but it did not send a 'terminated' event, simulate like the event has been sent
                this.on_did_exit_adapter(None);
            }

            this.cancel_all_requests();
            let parent = this.parent_session.borrow().clone();
            if this.options.lifecycle_managed_by_parent.unwrap_or(false)
                && let Some(parent) = parent
            {
                parent.terminate(restart).await?;
            } else if let Some(raw) = this.raw.borrow().clone() {
                if capability(&raw.capabilities(), "supportsTerminateRequest") && this.configuration.borrow().0.get("request").and_then(Value::as_str) == Some("launch") {
                    raw.terminate(restart).await?;
                } else {
                    raw.disconnect(json!({ "restart": restart, "terminateDebuggee": true })).await?;
                }
            }

            if !restart && let Some(compound_root) = &this.options.compound_root {
                compound_root.session_stopped();
            }
            Ok(())
        })
    }

    /// end the current debug adapter session
    fn disconnect(&self, restart: bool, suspend: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if this.raw.borrow().is_none() {
                // Adapter went down but it did not send a 'terminated' event, simulate like the event has been sent
                this.on_did_exit_adapter(None);
            }

            this.cancel_all_requests();
            let parent = this.parent_session.borrow().clone();
            if this.options.lifecycle_managed_by_parent.unwrap_or(false)
                && let Some(parent) = parent
            {
                parent.disconnect(restart, suspend).await?;
            } else if let Some(raw) = this.raw.borrow().clone() {
                raw.disconnect(json!({ "restart": restart, "terminateDebuggee": false, "suspendDebuggee": suspend })).await?;
            }

            if !restart && let Some(compound_root) = &this.options.compound_root {
                compound_root.session_stopped();
            }
            Ok(())
        })
    }

    /// restart debug adapter session
    fn restart(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("restart"));
            };

            this.cancel_all_requests();
            let parent = this.parent_session.borrow().clone();
            if this.options.lifecycle_managed_by_parent.unwrap_or(false)
                && let Some(parent) = parent
            {
                parent.restart().await?;
            } else {
                raw.restart(json!({ "arguments": this.configuration() })).await?;
            }
            Ok(())
        })
    }

    fn send_breakpoints(&self, model_uri: URI, breakpoints_to_send: Vec<Rc<Breakpoint>>, source_modified: bool) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("breakpoints"));
            };

            if !raw.ready_for_breakpoints() {
                return Ok(());
            }

            let mut raw_source = this.get_raw_source(&model_uri);
            if !breakpoints_to_send.is_empty() && raw_source.adapter_data.is_none() {
                raw_source.adapter_data = breakpoints_to_send[0].adapter_data();
            }
            // Normalize all drive letters going out from vscode to debug adapters so we are consistent with our resolving #43959
            if let Some(path) = raw_source.path.clone() {
                raw_source.path = Some(je_base::labels::normalize_drive_letter(&path, je_platform::resources::host_platform().is_windows));
            }

            let response = raw
                .set_breakpoints(json!({
                    "source": protocol_source_to_value(&raw_source),
                    "lines": breakpoints_to_send.iter().map(|bp| bp.session_agnostic_data().0).collect::<Vec<_>>(),
                    "breakpoints": breakpoints_to_send.iter().map(|bp| bp.to_dap()).collect::<Vec<_>>(),
                    "sourceModified": source_modified,
                }))
                .await?;
            if let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()) {
                let breakpoints = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
                let mut data = HashMap::new();
                for (i, bp) in breakpoints_to_send.iter().enumerate() {
                    data.insert(bp.get_id(), breakpoints.get(i).map(DapBreakpoint::from_value).unwrap_or_default());
                }

                this.model.set_breakpoint_session_data(&this.get_id(), &this.capabilities(), Some(&data));
            }
            Ok(())
        })
    }

    fn send_function_breakpoints(&self, fbpts: Vec<Rc<FunctionBreakpoint>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("function breakpoints"));
            };

            if raw.ready_for_breakpoints() {
                let response = raw.set_function_breakpoints(json!({ "breakpoints": fbpts.iter().map(|bp| bp.to_dap()).collect::<Vec<_>>() })).await?;
                if let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()) {
                    let breakpoints = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
                    let mut data = HashMap::new();
                    for (i, bp) in fbpts.iter().enumerate() {
                        data.insert(bp.get_id(), breakpoints.get(i).map(DapBreakpoint::from_value).unwrap_or_default());
                    }
                    this.model.set_breakpoint_session_data(&this.get_id(), &this.capabilities(), Some(&data));
                }
            }
            Ok(())
        })
    }

    fn send_exception_breakpoints(&self, exbpts: Vec<Rc<ExceptionBreakpoint>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("exception breakpoints"));
            };

            if raw.ready_for_breakpoints() {
                let args = if capability(&this.capabilities(), "supportsExceptionFilterOptions") {
                    json!({
                        "filters": [],
                        "filterOptions": exbpts.iter().map(|exb| {
                            match exb.base.condition.borrow().as_ref().filter(|c| !c.is_empty()) {
                                Some(condition) => json!({ "filterId": exb.filter, "condition": condition }),
                                None => json!({ "filterId": exb.filter }),
                            }
                        }).collect::<Vec<_>>(),
                    })
                } else {
                    json!({ "filters": exbpts.iter().map(|exb| exb.filter.clone()).collect::<Vec<_>>() })
                };

                let response = raw.set_exception_breakpoints(args).await?;
                if let Some(breakpoints) = response.and_then(|r| r.body).and_then(|b| b.get("breakpoints").cloned()).and_then(|b| b.as_array().cloned()) {
                    let mut data = HashMap::new();
                    for (i, bp) in exbpts.iter().enumerate() {
                        data.insert(bp.get_id(), breakpoints.get(i).map(DapBreakpoint::from_value).unwrap_or_default());
                    }

                    this.model.set_breakpoint_session_data(&this.get_id(), &this.capabilities(), Some(&data));
                }
            }
            Ok(())
        })
    }

    fn data_bytes_breakpoint_info(&self, address: &str, bytes: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>> {
        if self.raw.borrow().as_ref().is_some_and(|raw| raw.capabilities().get("supportsDataBreakpointBytes") == Some(&Value::Bool(false))) {
            return Promise::rejected(Error::new(localize("sessionDoesNotSupporBytesBreakpoints", "Session does not support breakpoints with bytes", &[])));
        }

        self.data_breakpoint_info_impl(json!({ "name": address, "bytes": bytes, "asAddress": true }))
    }

    fn data_breakpoint_info(&self, name: &str, variables_reference: Option<i64>, frame_id: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>> {
        self.data_breakpoint_info_impl(json!({ "name": name, "variablesReference": variables_reference, "frameId": frame_id }))
    }

    fn send_data_breakpoints(&self, data_breakpoints: Vec<Rc<DataBreakpoint>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("data breakpoints"));
            };

            if raw.ready_for_breakpoints() {
                let mut converted: Vec<(Option<Value>, Rc<DataBreakpoint>, Option<String>)> = Vec::new();
                let pending: Vec<Promise<Option<Value>>> = data_breakpoints.iter().map(|bp| bp.to_dap(this.this_full())).collect();
                let settled = Promise::all_settled(pending).await?;
                for (bp, result) in data_breakpoints.iter().zip(settled) {
                    match result {
                        Ok(dap) => converted.push((dap, bp.clone(), None)),
                        Err(e) => converted.push((None, bp.clone(), Some(e.message))),
                    }
                }
                let response = raw
                    .set_data_breakpoints(json!({ "breakpoints": converted.iter().filter_map(|(dap, _, _)| dap.clone()).collect::<Vec<_>>() }))
                    .await?;
                if let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()) {
                    let breakpoints = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
                    let mut data = HashMap::new();
                    let mut i = 0;
                    for (dap, bp, message) in &converted {
                        if dap.is_none() {
                            data.insert(bp.get_id(), DapBreakpoint { message: message.clone(), ..DapBreakpoint::default() });
                        } else if i < breakpoints.len() {
                            data.insert(bp.get_id(), DapBreakpoint::from_value(&breakpoints[i]));
                            i += 1;
                        }
                    }
                    this.model.set_breakpoint_session_data(&this.get_id(), &this.capabilities(), Some(&data));
                }
            }
            Ok(())
        })
    }

    fn send_instruction_breakpoints(&self, instruction_breakpoints: Vec<Rc<InstructionBreakpoint>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("instruction breakpoints"));
            };

            if raw.ready_for_breakpoints() {
                let response =
                    raw.set_instruction_breakpoints(json!({ "breakpoints": instruction_breakpoints.iter().map(|ib| ib.to_dap()).collect::<Vec<_>>() })).await?;
                if let Some(body) = response.and_then(|r| r.body).filter(|b| !b.is_null()) {
                    let breakpoints = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
                    let mut data = HashMap::new();
                    for (i, bp) in instruction_breakpoints.iter().enumerate() {
                        data.insert(bp.get_id(), breakpoints.get(i).map(DapBreakpoint::from_value).unwrap_or_default());
                    }
                    this.model.set_breakpoint_session_data(&this.get_id(), &this.capabilities(), Some(&data));
                }
            }
            Ok(())
        })
    }

    fn breakpoints_locations(&self, uri: &URI, line_number: i64) -> Promise<Vec<(i64, i64)>> {
        let this = self.this();
        let uri = uri.clone();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("breakpoints locations"));
            };

            let source = this.get_raw_source(&uri);
            let response = raw.breakpoint_locations(json!({ "source": protocol_source_to_value(&source), "line": line_number })).await?;
            let Some(breakpoints) = response.and_then(|r| r.body).and_then(|b| b.get("breakpoints").cloned()).and_then(|b| b.as_array().cloned()) else {
                return Ok(Vec::new());
            };

            let positions: Vec<(i64, i64)> = breakpoints
                .iter()
                .map(|bp| (number_of(bp, "line").unwrap_or(0), number_of(bp, "column").filter(|c| *c != 0).unwrap_or(1)))
                .collect();

            Ok(je_base::arrays::distinct(&positions, |p| format!("{}:{}", p.0, p.1)))
        })
    }

    fn resolve_location_reference(&self, location_reference: i64) -> Promise<IDebugLocationReferenced> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("locations"));
            };

            let location = raw.locations(json!({ "locationReference": location_reference })).await?;
            let Some(body) = location.and_then(|l| l.body).filter(|b| !b.is_null()) else {
                return Err(no_debug_adapter("locations"));
            };

            let source = this.get_source(body.get("source").filter(|s| s.is_object()).map(protocol_source_from_value));
            Ok(IDebugLocationReferenced {
                column: number_of(&body, "column").unwrap_or(1),
                line: number_of(&body, "line").unwrap_or(0),
                end_line: number_of(&body, "endLine"),
                end_column: number_of(&body, "endColumn"),
                source,
            })
        })
    }

    fn stack_trace(&self, thread_id: i64, start_frame: i64, levels: i64, token: CancellationToken) -> Promise<DapResponse> {
        match self.raw_or_err("stackTrace") {
            Ok(raw) => {
                let session_token = self.get_new_cancellation_token(thread_id, Some(&token));
                raw.stack_trace(json!({ "threadId": thread_id, "startFrame": start_frame, "levels": levels }), session_token)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn exception_info(&self, thread_id: i64) -> Promise<Option<IExceptionInfo>> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("exceptionInfo"));
            };

            let response = raw.exception_info(json!({ "threadId": thread_id })).await?;
            Ok(response.map(|response| {
                let body = response.body.unwrap_or(Value::Null);
                IExceptionInfo {
                    id: text_of(&body, "exceptionId"),
                    description: text_of(&body, "description"),
                    break_mode: text_of(&body, "breakMode"),
                    details: body.get("details").cloned(),
                }
            }))
        })
    }

    fn scopes(&self, frame_id: i64, thread_id: i64) -> Promise<DapResponse> {
        match self.raw_or_err("scopes") {
            Ok(raw) => {
                let token = self.get_new_cancellation_token(thread_id, None);
                raw.scopes(json!({ "frameId": frame_id }), token)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn variables(&self, variables_reference: i64, thread_id: Option<i64>, filter: Option<&str>, start: Option<i64>, count: Option<i64>) -> Promise<DapResponse> {
        match self.raw_or_err("variables") {
            Ok(raw) => {
                let token = thread_id.filter(|id| *id != 0).map(|id| self.get_new_cancellation_token(id, None));
                raw.variables(json!({ "variablesReference": variables_reference, "filter": filter, "start": start, "count": count }), token)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn evaluate(&self, expression: &str, frame_id: Option<i64>, context: Option<&str>, location: Option<IDebugEvaluatePosition>) -> Promise<DapResponse> {
        match self.raw_or_err("evaluate") {
            Ok(raw) => raw.evaluate(json!({
                "expression": expression,
                "frameId": frame_id,
                "context": context,
                "line": location.as_ref().map(|l| l.line),
                "column": location.as_ref().map(|l| l.column),
                "source": location.as_ref().map(|l| protocol_source_to_value(&l.source)),
            })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn restart_frame(&self, frame_id: i64, thread_id: i64) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.wait_for_triggered_breakpoints().await?;
            let raw = this.raw_or_err("restartFrame")?;
            raw.restart_frame(json!({ "frameId": frame_id }), thread_id).await?;
            Ok(())
        })
    }

    fn next(&self, thread_id: i64, granularity: Option<String>) -> Promise<()> {
        let args = json!({ "threadId": thread_id, "granularity": granularity });
        self.step_request("next", thread_id, granularity, true, move |raw| raw.next(args))
    }

    fn step_in(&self, thread_id: i64, target_id: Option<i64>, granularity: Option<String>) -> Promise<()> {
        let args = json!({ "threadId": thread_id, "targetId": target_id, "granularity": granularity });
        self.step_request("stepIn", thread_id, granularity, true, move |raw| raw.step_in(args))
    }

    fn step_out(&self, thread_id: i64, granularity: Option<String>) -> Promise<()> {
        let args = json!({ "threadId": thread_id, "granularity": granularity });
        self.step_request("stepOut", thread_id, granularity, true, move |raw| raw.step_out(args))
    }

    fn step_back(&self, thread_id: i64, granularity: Option<String>) -> Promise<()> {
        let args = json!({ "threadId": thread_id, "granularity": granularity });
        self.step_request("stepBack", thread_id, granularity, true, move |raw| raw.step_back(args))
    }

    fn continue_(&self, thread_id: i64) -> Promise<()> {
        self.step_request("continue", thread_id, None, false, move |raw| raw.continue_(json!({ "threadId": thread_id })))
    }

    fn reverse_continue(&self, thread_id: i64) -> Promise<()> {
        self.step_request("reverse continue", thread_id, None, false, move |raw| raw.reverse_continue(json!({ "threadId": thread_id })))
    }

    fn pause(&self, thread_id: i64) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let raw = this.raw_or_err("pause")?;
            raw.pause(json!({ "threadId": thread_id })).await?;
            Ok(())
        })
    }

    fn terminate_threads(&self, thread_ids: Option<Vec<i64>>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let raw = this.raw_or_err("terminateThreads")?;
            raw.terminate_threads(json!({ "threadIds": thread_ids })).await?;
            Ok(())
        })
    }

    fn set_variable(&self, variables_reference: i64, name: &str, value: &str) -> Promise<DapResponse> {
        match self.raw_or_err("setVariable") {
            Ok(raw) => raw.set_variable(json!({ "variablesReference": variables_reference, "name": name, "value": value })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn set_expression(&self, frame_id: i64, expression: &str, value: &str) -> Promise<DapResponse> {
        match self.raw_or_err("setExpression") {
            Ok(raw) => raw.set_expression(json!({ "expression": expression, "value": value, "frameId": frame_id })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn goto_targets(&self, source: ProtocolSource, line: i64, column: Option<i64>) -> Promise<DapResponse> {
        match self.raw_or_err("gotoTargets") {
            Ok(raw) => raw.goto_targets(json!({ "source": protocol_source_to_value(&source), "line": line, "column": column })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn goto(&self, thread_id: i64, target_id: i64) -> Promise<DapResponse> {
        match self.raw_or_err("goto") {
            Ok(raw) => raw.goto(json!({ "threadId": thread_id, "targetId": target_id })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn load_source(&self, resource: &URI) -> Promise<DapResponse> {
        let Ok(raw) = self.raw_or_err("loadSource") else {
            return Promise::rejected(no_debug_adapter("loadSource"));
        };

        let raw_source = match self.get_source_for_uri(resource) {
            Some(source) => source.raw.borrow().clone(),
            None => {
                // create a Source
                let data = Source::get_encoded_debug_data(resource);
                ProtocolSource { path: Some(data.path), source_reference: data.source_reference, ..ProtocolSource::default() }
            }
        };

        raw.source(json!({ "sourceReference": raw_source.source_reference.unwrap_or(0), "source": protocol_source_to_value(&raw_source) }))
    }

    fn get_loaded_sources(&self) -> Promise<Vec<Rc<Source>>> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("getLoadedSources"));
            };

            let response = raw.loaded_sources(json!({})).await?;
            match response.and_then(|r| r.body).and_then(|b| b.get("sources").cloned()).and_then(|s| s.as_array().cloned()) {
                Some(sources) => Ok(sources.iter().map(|src| this.get_source(Some(protocol_source_from_value(src)))).collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    fn completions(&self, frame_id: Option<i64>, thread_id: i64, text: &str, line_number: i64, column: i64, token: CancellationToken) -> Promise<DapResponse> {
        match self.raw_or_err("completions") {
            Ok(raw) => {
                let session_cancelation_token = self.get_new_cancellation_token(thread_id, Some(&token));
                raw.completions(json!({ "frameId": frame_id, "text": text, "column": column, "line": line_number }), session_cancelation_token)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn step_in_targets(&self, frame_id: i64) -> Promise<Option<Value>> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("stepInTargets"));
            };

            let response = raw.step_in_targets(json!({ "frameId": frame_id })).await?;
            Ok(response.and_then(|r| r.body).and_then(|b| b.get("targets").cloned()))
        })
    }

    fn cancel(&self, progress_id: &str) -> Promise<DapResponse> {
        match self.raw_or_err("cancel") {
            Ok(raw) => raw.cancel(json!({ "progressId": progress_id })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn disassemble(&self, memory_reference: &str, offset: i64, instruction_offset: i64, instruction_count: i64) -> Promise<Option<Value>> {
        let this = self.this();
        let memory_reference = memory_reference.to_owned();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("disassemble"));
            };

            let response = raw
                .disassemble(json!({
                    "memoryReference": memory_reference,
                    "offset": offset,
                    "instructionOffset": instruction_offset,
                    "instructionCount": instruction_count,
                    "resolveSymbols": true,
                }))
                .await?;
            Ok(response.and_then(|r| r.body).and_then(|b| b.get("instructions").cloned()))
        })
    }

    fn read_memory(&self, memory_reference: &str, offset: i64, count: i64) -> Promise<DapResponse> {
        match self.raw_or_err("readMemory") {
            Ok(raw) => raw.read_memory(json!({ "count": count, "memoryReference": memory_reference, "offset": offset })),
            Err(error) => Promise::rejected(error),
        }
    }

    fn write_memory(&self, memory_reference: &str, offset: i64, data: &str, allow_partial: Option<bool>) -> Promise<DapResponse> {
        // O original reporta `'disassemble'` na mensagem de erro do `writeMemory`.
        match self.raw_or_err("disassemble") {
            Ok(raw) => raw.write_memory(json!({ "memoryReference": memory_reference, "offset": offset, "allowPartial": allow_partial, "data": data })),
            Err(error) => Promise::rejected(error),
        }
    }
}

impl DebugSession {
    fn data_breakpoint_info_impl(&self, args: Value) -> Promise<Option<IDataBreakpointInfoResponse>> {
        let this = self.this();
        async_fn(async move {
            let Some(raw) = this.raw.borrow().clone() else {
                return Err(no_debug_adapter("data breakpoints info"));
            };
            if !raw.ready_for_breakpoints() {
                return Err(Error::new(localize("sessionNotReadyForBreakpoints", "Session is not ready for breakpoints", &[])));
            }

            let response = raw.data_breakpoint_info(args).await?;
            Ok(response.and_then(|r| r.body).filter(|b| !b.is_null()).map(|body| IDataBreakpointInfoResponse {
                data_id: text_of(&body, "dataId"),
                description: text_of(&body, "description").unwrap_or_default(),
                can_persist: body.get("canPersist").and_then(Value::as_bool),
                access_types: None,
            }))
        })
    }
}
