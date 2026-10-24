//! Os dublês de `src/vs/workbench/contrib/debug/test/common/mockDebug.ts` e `test/browser/mockDebugModel.ts`, mais o
//! `createTestSession` de `test/browser/callStack.test.ts`, que os testes de depuração compartilham: `MockSession`,
//! `MockRawSession`, `MockDebugAdapter`, `MockDebugStorage`, `createMockDebugModel` e `mockUriIdentityService`.
//!
//! Divergências:
//! - **`MockDebugService`** NÃO foi portado (só o teste `decorations` de `breakpoints.test.ts` o usa, e esse caso não foi
//!   portado: ver o topo de `debug_breakpoints.rs`).
//! - **`MockSession`** implementa o contrato completo [`IDebugSessionFull`]; o que o original lança com
//!   `throw new Error('not implemented')` é `panic!` (os getters síncronos) ou uma promessa rejeitada com a mesma mensagem
//!   (os pedidos assíncronos). O `mockObject<MockSession>()({ onDidInvalidateMemory })` do teste de memória é o
//!   [`MockSession`] com [`MockSession::set_memory_hooks`] (o `callsFake` de `readMemory` e `writeMemory` e o evento).
//! - **`MockRawSession`** implementa [`IRawDebugSession`]: `stackTrace` devolve a resposta fixa e conta as chamadas (o
//!   `sinon.spy(mockRawSession, 'stackTrace')`); o resto rejeita com `not implemented`.
//! - **`MockDebugAdapter`** é o de `abstractDebugAdapter.test.ts`; o `class extends MockDebugAdapter { override sendMessage }`
//!   do teste de `continued` é [`MockDebugAdapter::set_send_message_override`], que devolve `true` quando tratou a mensagem
//!   (o `super.sendMessage` roda quando devolve `false`).
//! - **`MockDebugStorage(storageService)`** passa ao `DebugStorage` os dublês de `ITextFileService` (`isDirty => false`) e
//!   de `IUriIdentityService`, que o original deixa `undefined`; o `ILogService` é o `NullLogService`.
//! - **Descarte.** Os serviços do `createTestSession` incluem um `ViewModel` real (o original usa o objeto `{ updateViews() }`);
//!   ele vai para o `store` recebido, como o resto dos descartáveis do teste.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_instantiation::Injectable;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::{LogService, NullLogService};
use je_platform::storage::storage::StorageService;
use je_storage::StorageValue;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::WorkspaceFolder;
use je_workbench::contrib::debug::abstract_debug_adapter::{AbstractDebugAdapter, DebugAdapterState};
use je_workbench::contrib::debug::breakpoints::{
    Breakpoint, DataBreakpoint, ExceptionBreakpoint, ExceptionBreakpointsFilter, FunctionBreakpoint, IDirtyChecker, InstructionBreakpoint,
};
use je_workbench::contrib::debug::debug::{DebugCompoundRoot, IConfig, IDebugSession};
use je_workbench::contrib::debug::debug_model::{DebugModel, DebugStorage, MemoryRegion, StackFrame, Thread};
use je_workbench::contrib::debug::debug_protocol::{Event as ProtocolEvent, ProtocolMessage, Request, Response, Source as ProtocolSource};
use je_workbench::contrib::debug::debug_session::{DebugConfiguration, DebugSession, DebugSessionOptions, FocusOptions, IDebugSessionServices};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::contrib::debug::debug_types::{
    AdapterEndEvent, DapResponse, IDataBreakpointInfoResponse, IDebugEvaluatePosition, IDebugLocationReferenced, IDebugger, IExceptionInfo, IRawModelUpdate,
    IRawStoppedDetails, ITelemetryEndpoint, LoadedSourceEvent, State, StoppedDetails,
};
use je_workbench::contrib::debug::debug_types::IDebugSessionFull;
use je_workbench::contrib::debug::debug_view_model::ViewModel;
use je_workbench::contrib::debug::raw_debug_session::IRawDebugSession;
use je_workbench::contrib::debug::repl_model::{DebugConsoleConfiguration, NewReplElementData, ReplElement, ReplModel};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestStorageService};
use serde_json::{Value, json};

thread_local! {
    // No original é uma constante do módulo (`export const mockUriIdentityService = new UriIdentityService(fileService)`), criada
    // antes do rastreador de vazamentos: aqui nasce no primeiro uso, fora do rastreador.
    static MOCK_URI_IDENTITY_SERVICE: Rc<UriIdentityService> =
        je_base::lifecycle::without_disposable_tracker(|| UriIdentityService::create((), (TestFileService::new(),)));
}

/// `mockUriIdentityService`: um `UriIdentityService` sobre o `TestFileService`.
pub fn mock_uri_identity_service() -> Rc<UriIdentityService> {
    MOCK_URI_IDENTITY_SERVICE.with(Rc::clone)
}

/// `upcastPartial<ITextFileService>({ isDirty: (e) => false })`.
pub struct NotDirty;

impl IDirtyChecker for NotDirty {
    fn is_dirty(&self, _resource: &URI) -> bool {
        false
    }
}

/// `new MockDebugStorage(storageService)`.
pub fn mock_debug_storage(storage_service: Rc<dyn StorageService>) -> Rc<DebugStorage> {
    DebugStorage::new(storage_service, Rc::new(NotDirty), mock_uri_identity_service(), NullLogService::shared())
}

/// O `TestStorageService` como `Rc<dyn StorageService>`.
pub fn storage_service_of(storage: &Rc<je_platform::storage::storage::AbstractStorageService>) -> Rc<dyn StorageService> {
    Rc::new(storage.clone())
}

/// `createMockDebugModel(disposable)`.
pub fn create_mock_debug_model(store: &DisposableStore) -> Rc<DebugModel> {
    let storage = TestStorageService::new();
    store.add_rc(storage.clone());
    let debug_storage = mock_debug_storage(storage_service_of(&storage));
    store.add_rc(debug_storage.clone());
    let model = DebugModel::new(debug_storage, Rc::new(NotDirty), Some(mock_uri_identity_service()), NullLogService::shared());
    store.add_rc(model.clone());
    model
}

/// Os serviços de `createTestSession`: o `IDebugService` com `getViewModel()` e a configuração
/// `{ debug: { console: { collapseIdenticalLines: true } } }`.
pub struct TestSessionServices {
    view_model: Rc<ViewModel>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl TestSessionServices {
    pub fn new(store: &DisposableStore) -> Rc<Self> {
        let view_model = ViewModel::new(MockContextKeyService::new());
        store.add_rc(view_model.clone());
        Rc::new(Self { view_model, uri_identity_service: mock_uri_identity_service() })
    }
}

impl IDebugSessionServices for TestSessionServices {
    fn view_model(&self) -> Rc<ViewModel> {
        self.view_model.clone()
    }

    fn focus_stack_frame(&self, _: Option<Rc<StackFrame>>, _: Option<Rc<Thread>>, _: Option<Rc<dyn IDebugSessionFull>>, _: FocusOptions) -> Promise<()> {
        panic!("this.debugService.focusStackFrame is not a function")
    }

    fn send_all_breakpoints(&self, _: Rc<dyn IDebugSessionFull>) -> Promise<()> {
        panic!("this.debugService.sendAllBreakpoints is not a function")
    }

    fn send_breakpoints(&self, _: URI, _: Option<bool>, _: Rc<dyn IDebugSessionFull>) -> Promise<()> {
        panic!("this.debugService.sendBreakpoints is not a function")
    }

    fn restart_session(&self, _: Rc<dyn IDebugSessionFull>, _: Value) -> Promise<()> {
        panic!("this.debugService.restartSession is not a function")
    }

    fn set_exception_breakpoints_for_session(&self, _: Rc<dyn IDebugSessionFull>, _: Vec<ExceptionBreakpointsFilter>) {
        panic!("this.debugService.setExceptionBreakpointsForSession is not a function")
    }

    fn debug_configuration(&self) -> DebugConfiguration {
        DebugConfiguration {
            auto_expand_lazy_variables: String::new(),
            focus_editor_on_break: false,
            open_debug: String::new(),
            focus_window_on_break: false,
            console: DebugConsoleConfiguration { collapse_identical_lines: true, maximum_lines: usize::MAX },
        }
    }

    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    /// `mockWorkspaceContextService.getWorkspace().folders` é a lista vazia.
    fn workspace_folder_count(&self) -> usize {
        0
    }

    fn product_name_long(&self) -> String {
        panic!("this.productService.nameLong")
    }

    fn locale(&self) -> String {
        "en".to_owned()
    }

    fn aria_status(&self, _: &str) {}

    fn notify_error(&self, _: &str) {
        panic!("this.notificationService.error is not a function")
    }

    fn notify(&self, _: &str, _: Severity, _: &str) {
        panic!("this.notificationService.notify is not a function")
    }

    fn open_debug_view(&self) -> Promise<()> {
        panic!("this.paneCompositeService.openPaneComposite is not a function")
    }

    fn extension_tests_location_uri_is_set(&self) -> bool {
        false
    }

    fn window_has_focus(&self) -> bool {
        true
    }

    fn focus_window(&self) -> Promise<()> {
        panic!("this.hostService.focus is not a function")
    }

    fn telemetry_enabled(&self) -> bool {
        false
    }

    fn publish_custom_endpoint_telemetry(&self, _: &ITelemetryEndpoint, _: &str, _: Option<Value>) {}

    fn on_will_shutdown(&self) -> Option<Event<()>> {
        None
    }

    fn uri_identity_service(&self) -> Rc<dyn IUriIdentityService> {
        self.uri_identity_service.clone()
    }

    fn log_service(&self) -> Rc<dyn LogService> {
        NullLogService::shared()
    }

    fn create_raw_session(&self, _: Rc<dyn je_workbench::contrib::debug::debug::IDebugAdapter>, _: Rc<dyn IDebugger>, _: &str, _: &str) -> Rc<dyn IRawDebugSession> {
        panic!("this.instantiationService.createInstance(RawDebugSession) não é usado nos testes")
    }
}

/// `createTestSession(model, name = 'mockSession', options)`.
pub fn create_test_session(model: &Rc<DebugModel>, name: &str, options: Option<DebugSessionOptions>, store: &DisposableStore) -> Rc<DebugSession> {
    DebugSession::new(
        generate_uuid(),
        json!({ "name": name, "type": "node", "request": "launch" }),
        None,
        None,
        model.clone(),
        options,
        TestSessionServices::new(store),
    )
}

// ---- MockDebugAdapter

type SendMessageOverride = Rc<dyn Fn(&Rc<MockDebugAdapter>, &ProtocolMessage) -> bool>;

/// `MockDebugAdapter`.
pub struct MockDebugAdapter {
    state: Rc<DebugAdapterState>,
    me: Weak<MockDebugAdapter>,
    seq: Cell<i64>,
    pending_responses: RefCell<HashMap<String, Rc<DeferredPromise<Response>>>>,
    send_message_override: RefCell<Option<SendMessageOverride>>,
}

impl MockDebugAdapter {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            state: DebugAdapterState::new(),
            me: me.clone(),
            seq: Cell::new(0),
            pending_responses: RefCell::new(HashMap::new()),
            send_message_override: RefCell::new(None),
        })
    }

    /// O `override sendMessage` de uma subclasse: devolve `true` quando tratou a mensagem.
    pub fn set_send_message_override(&self, handler: impl Fn(&Rc<MockDebugAdapter>, &ProtocolMessage) -> bool + 'static) {
        *self.send_message_override.borrow_mut() = Some(Rc::new(handler));
    }

    fn next_seq(&self) -> i64 {
        self.seq.set(self.seq.get() + 1);
        self.seq.get()
    }

    fn this(&self) -> Rc<MockDebugAdapter> {
        self.me.upgrade().expect("o adaptador está vivo")
    }

    pub fn send_response_body(&self, request: &Request, body: Value) {
        let response = Response {
            seq: self.next_seq(),
            request_seq: request.seq,
            command: request.command.clone(),
            success: true,
            message: None,
            body: Some(body),
        };
        self.this().accept_message(ProtocolMessage::Response(response));
    }

    pub fn send_event_body(&self, event: &str, body: Value) {
        let response = ProtocolEvent { seq: self.next_seq(), event: event.to_owned(), body: Some(body) };
        self.this().accept_message(ProtocolMessage::Event(response));
    }

    pub fn wait_for_response_from_client(&self, command: &str) -> Promise<Response> {
        let deferred = Rc::new(DeferredPromise::<Response>::default());
        if let Some(existing) = self.pending_responses.borrow().get(command) {
            return existing.promise.clone();
        }

        self.pending_responses.borrow_mut().insert(command.to_owned(), deferred.clone());
        deferred.promise.clone()
    }

    pub fn send_request_body(&self, command: &str, args: Value) {
        let request = Request { seq: self.next_seq(), command: command.to_owned(), arguments: Some(args) };
        self.this().accept_message(ProtocolMessage::Request(request));
    }

    pub fn evaluate(&self, request: &Request, args: &Value) {
        let expression = args["expression"].as_str().expect("expression").to_owned();
        if expression.starts_with("before.") {
            self.send_event_body("output", json!({ "output": expression }));
        }

        self.send_response_body(request, json!({ "result": format!("={expression}"), "variablesReference": 0 }));

        if expression.starts_with("after.") {
            self.send_event_body("output", json!({ "output": expression }));
        }
    }
}

impl IDisposable for MockDebugAdapter {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for MockDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn stop_session(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn send_message(&self, message: &ProtocolMessage) {
        let handler = self.send_message_override.borrow().clone();
        if let Some(handler) = handler
            && handler(&self.this(), message)
        {
            return;
        }
        match message {
            ProtocolMessage::Request(request) => {
                let request = request.clone();
                let this = self.this();
                runtime::set_timeout(
                    move || {
                        if request.command == "evaluate" {
                            this.evaluate(&request, request.arguments.as_ref().expect("arguments"));
                            return;
                        }
                        this.send_response_body(&request, json!({}));
                    },
                    0,
                );
            }
            ProtocolMessage::Response(response) => {
                if let Some(pending) = self.pending_responses.borrow().get(&response.command) {
                    drop(pending.complete(response.clone()));
                }
            }
            _ => {}
        }
    }
}

// ---- MockSession

fn not_implemented<T: Clone + 'static>() -> Promise<T> {
    Promise::rejected(je_base::errors::Error::new("Method not implemented."))
}

type ReadMemoryHook = Rc<dyn Fn(&str, i64, i64) -> Promise<DapResponse>>;
type WriteMemoryHook = Rc<dyn Fn(&str, i64, &str) -> Promise<DapResponse>>;

/// `MockSession`.
pub struct MockSession {
    configuration: IConfig,
    memory_hooks: RefCell<Option<(ReadMemoryHook, WriteMemoryHook, Event<ProtocolEvent>)>>,
    repl_model: Rc<ReplModel>,
    me: Weak<MockSession>,
}

impl MockSession {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            configuration: json!({ "type": "mock", "name": "mock", "request": "launch" }),
            memory_hooks: RefCell::new(None),
            repl_model: ReplModel::new(Rc::new(|| DebugConsoleConfiguration { collapse_identical_lines: false, maximum_lines: usize::MAX })),
            me: me.clone(),
        })
    }

    /// O `mockObject<MockSession>()({ onDidInvalidateMemory })` com o `callsFake` de `readMemory` e `writeMemory`.
    pub fn set_memory_hooks(
        &self,
        read_memory: impl Fn(&str, i64, i64) -> Promise<DapResponse> + 'static,
        write_memory: impl Fn(&str, i64, &str) -> Promise<DapResponse> + 'static,
        on_did_invalidate_memory: Event<ProtocolEvent>,
    ) {
        *self.memory_hooks.borrow_mut() = Some((Rc::new(read_memory), Rc::new(write_memory), on_did_invalidate_memory));
    }

    fn this(&self) -> Rc<MockSession> {
        self.me.upgrade().expect("a sessão está viva")
    }
}

impl IDisposable for MockSession {
    fn dispose(&self) {}
}

impl IDebugSession for MockSession {
    fn get_id(&self) -> String {
        "mock".to_owned()
    }

    fn configuration(&self) -> IConfig {
        self.configuration.clone()
    }

    fn name(&self) -> String {
        "mockname".to_owned()
    }

    fn set_name(&self, _name: &str) {
        panic!("not implemented")
    }

    fn root(&self) -> Option<WorkspaceFolder> {
        None
    }

    fn parent_session(&self) -> Option<Rc<dyn IDebugSession>> {
        None
    }

    fn compound_root(&self) -> Option<Rc<DebugCompoundRoot>> {
        None
    }

    fn on_did_change_name(&self) -> Event<String> {
        panic!("not implemented")
    }

    fn on_did_custom_event(&self) -> Event<ProtocolEvent> {
        panic!("not implemented")
    }

    fn append_to_repl(&self, _data: je_workbench::contrib::debug::debug::INewReplElementData) {}

    fn custom_request(&self, _request: &str, _args: Option<Value>) -> Promise<Option<Response>> {
        not_implemented()
    }

    fn get_debug_protocol_breakpoint(&self, _breakpoint_id: &str) -> Option<Value> {
        panic!("Method not implemented.")
    }
}

impl IDebugSessionFull for MockSession {
    fn state(&self) -> State {
        State::Stopped
    }

    fn unresolved_configuration(&self) -> Option<IConfig> {
        Some(self.configuration.clone())
    }

    fn sub_id(&self) -> Option<String> {
        None
    }

    fn set_sub_id(&self, _sub_id: Option<String>) {
        panic!("Method not implemented.")
    }

    fn compact(&self) -> bool {
        false
    }

    fn save_before_restart(&self) -> bool {
        true
    }

    fn auto_expand_lazy_variables(&self) -> bool {
        false
    }

    fn suppress_debug_toolbar(&self) -> bool {
        false
    }

    fn suppress_debug_statusbar(&self) -> bool {
        false
    }

    fn suppress_debug_view(&self) -> bool {
        false
    }

    fn lifecycle_managed_by_parent(&self) -> bool {
        false
    }

    fn get_label(&self) -> String {
        "mockname".to_owned()
    }

    fn get_memory(&self, _memory_reference: &str) -> Rc<MemoryRegion> {
        panic!("Method not implemented.")
    }

    fn get_source_for_uri(&self, _model_uri: &URI) -> Option<Rc<Source>> {
        panic!("not implemented")
    }

    fn get_source(&self, _raw: Option<ProtocolSource>) -> Rc<Source> {
        panic!("not implemented")
    }

    fn set_configuration(&self, _resolved: IConfig, _unresolved: Option<IConfig>) {}

    fn raw_update(&self, _data: IRawModelUpdate) {}

    fn get_thread(&self, _thread_id: i64) -> Option<Rc<Thread>> {
        panic!("not implemented")
    }

    fn get_all_threads(&self) -> Vec<Rc<Thread>> {
        Vec::new()
    }

    fn clear_threads(&self, _remove_threads: bool, _reference: Option<i64>) {}

    fn get_stopped_details(&self) -> Option<StoppedDetails> {
        panic!("not implemented")
    }

    fn repl_model(&self) -> Rc<ReplModel> {
        self.repl_model.clone()
    }

    fn get_repl_elements(&self) -> Vec<ReplElement> {
        Vec::new()
    }

    fn has_separate_repl(&self) -> bool {
        true
    }

    fn remove_repl_expressions(&self) {}

    fn add_repl_expression(&self, _stack_frame: Option<Rc<StackFrame>>, _name: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn append_to_repl_data(&self, _data: NewReplElementData, _is_important: bool) {}

    fn cancel_correlated_test_run(&self) {}

    fn on_did_end_adapter(&self) -> Event<Option<AdapterEndEvent>> {
        panic!("not implemented")
    }

    fn on_did_change_state(&self) -> Event<()> {
        panic!("not implemented")
    }

    fn on_did_change_repl_elements(&self) -> Event<Option<ReplElement>> {
        panic!("not implemented")
    }

    fn capabilities(&self) -> Value {
        json!({})
    }

    fn remembered_capabilities(&self) -> Option<Value> {
        None
    }

    fn on_did_loaded_source(&self) -> Event<LoadedSourceEvent> {
        panic!("not implemented")
    }

    fn on_did_progress_start(&self) -> Event<ProtocolEvent> {
        panic!("not implemented")
    }

    fn on_did_progress_update(&self) -> Event<ProtocolEvent> {
        panic!("not implemented")
    }

    fn on_did_progress_end(&self) -> Event<ProtocolEvent> {
        panic!("not implemented")
    }

    fn on_did_invalidate_memory(&self) -> Event<ProtocolEvent> {
        match self.memory_hooks.borrow().as_ref() {
            Some((_, _, event)) => event.clone(),
            None => panic!("Not implemented"),
        }
    }

    fn initialize(&self, _dbgr: Rc<dyn IDebugger>) -> Promise<()> {
        not_implemented()
    }

    fn launch_or_attach(&self, _config: IConfig) -> Promise<()> {
        not_implemented()
    }

    fn restart(&self) -> Promise<()> {
        not_implemented()
    }

    fn terminate(&self, _restart: bool) -> Promise<()> {
        not_implemented()
    }

    fn disconnect(&self, _restart: bool, _suspend: bool) -> Promise<()> {
        not_implemented()
    }

    fn send_breakpoints(&self, _model_uri: URI, _breakpoints: Vec<Rc<Breakpoint>>, _source_modified: bool) -> Promise<()> {
        not_implemented()
    }

    fn send_function_breakpoints(&self, _fbps: Vec<Rc<FunctionBreakpoint>>) -> Promise<()> {
        not_implemented()
    }

    fn data_breakpoint_info(&self, _name: &str, _variables_reference: Option<i64>, _frame_id: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>> {
        not_implemented()
    }

    fn data_bytes_breakpoint_info(&self, _address: &str, _bytes: Option<i64>) -> Promise<Option<IDataBreakpointInfoResponse>> {
        not_implemented()
    }

    fn send_data_breakpoints(&self, _dbps: Vec<Rc<DataBreakpoint>>) -> Promise<()> {
        not_implemented()
    }

    fn send_instruction_breakpoints(&self, _ibps: Vec<Rc<InstructionBreakpoint>>) -> Promise<()> {
        not_implemented()
    }

    fn send_exception_breakpoints(&self, _exbpts: Vec<Rc<ExceptionBreakpoint>>) -> Promise<()> {
        not_implemented()
    }

    fn breakpoints_locations(&self, _uri: &URI, _line_number: i64) -> Promise<Vec<(i64, i64)>> {
        not_implemented()
    }

    fn resolve_location_reference(&self, _location_reference: i64) -> Promise<IDebugLocationReferenced> {
        not_implemented()
    }

    fn stack_trace(&self, _thread_id: i64, _start_frame: i64, _levels: i64, _token: CancellationToken) -> Promise<DapResponse> {
        not_implemented()
    }

    fn exception_info(&self, _thread_id: i64) -> Promise<Option<IExceptionInfo>> {
        not_implemented()
    }

    fn scopes(&self, _frame_id: i64, _thread_id: i64) -> Promise<DapResponse> {
        not_implemented()
    }

    fn variables(&self, _variables_reference: i64, _thread_id: Option<i64>, _filter: Option<&str>, _start: Option<i64>, _count: Option<i64>) -> Promise<DapResponse> {
        not_implemented()
    }

    fn evaluate(&self, _expression: &str, _frame_id: Option<i64>, _context: Option<&str>, _location: Option<IDebugEvaluatePosition>) -> Promise<DapResponse> {
        not_implemented()
    }

    fn restart_frame(&self, _frame_id: i64, _thread_id: i64) -> Promise<()> {
        not_implemented()
    }

    fn next(&self, _thread_id: i64, _granularity: Option<String>) -> Promise<()> {
        not_implemented()
    }

    fn step_in(&self, _thread_id: i64, _target_id: Option<i64>, _granularity: Option<String>) -> Promise<()> {
        not_implemented()
    }

    fn step_out(&self, _thread_id: i64, _granularity: Option<String>) -> Promise<()> {
        not_implemented()
    }

    fn step_back(&self, _thread_id: i64, _granularity: Option<String>) -> Promise<()> {
        not_implemented()
    }

    fn continue_(&self, _thread_id: i64) -> Promise<()> {
        not_implemented()
    }

    fn reverse_continue(&self, _thread_id: i64) -> Promise<()> {
        not_implemented()
    }

    fn pause(&self, _thread_id: i64) -> Promise<()> {
        not_implemented()
    }

    fn terminate_threads(&self, _thread_ids: Option<Vec<i64>>) -> Promise<()> {
        not_implemented()
    }

    fn set_variable(&self, _variables_reference: i64, _name: &str, _value: &str) -> Promise<DapResponse> {
        not_implemented()
    }

    fn set_expression(&self, _frame_id: i64, _expression: &str, _value: &str) -> Promise<DapResponse> {
        not_implemented()
    }

    fn goto_targets(&self, _source: ProtocolSource, _line: i64, _column: Option<i64>) -> Promise<DapResponse> {
        not_implemented()
    }

    fn goto(&self, _thread_id: i64, _target_id: i64) -> Promise<DapResponse> {
        not_implemented()
    }

    fn load_source(&self, _resource: &URI) -> Promise<DapResponse> {
        not_implemented()
    }

    fn get_loaded_sources(&self) -> Promise<Vec<Rc<Source>>> {
        Promise::resolved(Vec::new())
    }

    fn completions(&self, _frame_id: Option<i64>, _thread_id: i64, _text: &str, _line_number: i64, _column: i64, _token: CancellationToken) -> Promise<DapResponse> {
        not_implemented()
    }

    fn step_in_targets(&self, _frame_id: i64) -> Promise<Option<Value>> {
        not_implemented()
    }

    fn cancel(&self, _progress_id: &str) -> Promise<DapResponse> {
        not_implemented()
    }

    fn disassemble(&self, _memory_reference: &str, _offset: i64, _instruction_offset: i64, _instruction_count: i64) -> Promise<Option<Value>> {
        not_implemented()
    }

    fn read_memory(&self, memory_reference: &str, offset: i64, count: i64) -> Promise<DapResponse> {
        let hook = self.memory_hooks.borrow().as_ref().map(|(read, _, _)| read.clone());
        match hook {
            Some(read) => read(memory_reference, offset, count),
            None => not_implemented(),
        }
    }

    fn write_memory(&self, memory_reference: &str, offset: i64, data: &str, _allow_partial: Option<bool>) -> Promise<DapResponse> {
        let hook = self.memory_hooks.borrow().as_ref().map(|(_, write, _)| write.clone());
        match hook {
            Some(write) => write(memory_reference, offset, data),
            None => not_implemented(),
        }
    }
}

// ---- MockRawSession

/// `MockRawSession`.
pub struct MockRawSession {
    capabilities: RefCell<Value>,
    stack_trace_calls: Cell<usize>,
}

impl MockRawSession {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { capabilities: RefCell::new(json!({})), stack_trace_calls: Cell::new(0) })
    }

    /// `sessionStub.callCount` (o `sinon.spy(mockRawSession, 'stackTrace')`).
    pub fn stack_trace_call_count(&self) -> usize {
        self.stack_trace_calls.get()
    }

    /// `capabilities = {...}`.
    pub fn set_capabilities(&self, capabilities: Value) {
        *self.capabilities.borrow_mut() = capabilities;
    }
}

macro_rules! raw_not_implemented {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            fn $name(&self $(, $arg: $ty)*) -> Promise<DapResponse> {
                not_implemented()
            }
        )*
    };
}

impl IDisposable for MockRawSession {
    fn dispose(&self) {}
}

macro_rules! raw_no_events {
    ($($name:ident;)*) => {
        $(
            fn $name(&self) -> Event<ProtocolEvent> {
                Event::none()
            }
        )*
    };
}

impl IRawDebugSession for MockRawSession {
    fn is_in_shutdown(&self) -> bool {
        false
    }

    fn dbgr(&self) -> Option<Rc<dyn IDebugger>> {
        None
    }

    fn capabilities(&self) -> Value {
        self.capabilities.borrow().clone()
    }

    fn ready_for_breakpoints(&self) -> bool {
        true
    }

    fn on_did_exit_adapter(&self) -> Event<AdapterEndEvent> {
        Event::none()
    }

    raw_no_events! {
        on_did_initialize;
        on_did_stop;
        on_did_continued;
        on_did_terminate_debugee;
        on_did_exit_debugee;
        on_did_thread;
        on_did_output;
        on_did_breakpoint;
        on_did_loaded_source;
        on_did_custom_event;
        on_did_progress_start;
        on_did_progress_update;
        on_did_progress_end;
        on_did_invalidated;
        on_did_invalidate_memory;
        on_did_event;
    }

    fn start(&self) -> Promise<()> {
        not_implemented()
    }

    fn disconnect(&self, _args: Value) -> Promise<()> {
        not_implemented()
    }

    fn stack_trace(&self, _args: Value, _token: CancellationToken) -> Promise<DapResponse> {
        self.stack_trace_calls.set(self.stack_trace_calls.get() + 1);
        Promise::resolved(Some(Response {
            seq: 1,
            request_seq: 1,
            success: true,
            command: "stackTrace".to_owned(),
            message: None,
            body: Some(json!({ "stackFrames": [{ "id": 1, "name": "mock", "line": 5, "column": 6 }] })),
        }))
    }

    fn evaluate(&self, _args: Value) -> Promise<DapResponse> {
        Promise::resolved(None)
    }

    fn threads(&self) -> Promise<DapResponse> {
        not_implemented()
    }

    fn configuration_done(&self) -> Promise<DapResponse> {
        not_implemented()
    }

    raw_not_implemented! {
        initialize(args: Value);
        launch_or_attach(config: IConfig);
        terminate(restart: bool);
        restart(args: Value);
        next(args: Value);
        step_in(args: Value);
        step_out(args: Value);
        continue_(args: Value);
        pause(args: Value);
        terminate_threads(args: Value);
        set_variable(args: Value);
        set_expression(args: Value);
        restart_frame(args: Value, thread_id: i64);
        step_in_targets(args: Value);
        completions(args: Value, token: CancellationToken);
        set_breakpoints(args: Value);
        set_function_breakpoints(args: Value);
        data_breakpoint_info(args: Value);
        set_data_breakpoints(args: Value);
        set_exception_breakpoints(args: Value);
        breakpoint_locations(args: Value);
        exception_info(args: Value);
        scopes(args: Value, token: CancellationToken);
        variables(args: Value, token: Option<CancellationToken>);
        source(args: Value);
        locations(args: Value);
        loaded_sources(args: Value);
        step_back(args: Value);
        reverse_continue(args: Value);
        goto_targets(args: Value);
        goto(args: Value);
        set_instruction_breakpoints(args: Value);
        disassemble(args: Value);
        read_memory(args: Value);
        write_memory(args: Value);
        cancel(args: Value);
        custom(request: &str, args: Value);
    }
}

/// `DebugProtocol.StoppedEvent.body` (`IRawStoppedDetails`) a partir dos campos que os testes preenchem.
pub fn stopped_details(reason: Option<&str>, thread_id: Option<i64>, all_threads_stopped: Option<bool>) -> IRawStoppedDetails {
    IRawStoppedDetails {
        reason: reason.map(str::to_owned),
        thread_id,
        all_threads_stopped,
        ..IRawStoppedDetails::default()
    }
}

/// `storage.store(key, value, scope, target, external)` do `TestStorageService` (o `store_with` do serviço).
pub fn store_external(storage: &Rc<je_platform::storage::storage::AbstractStorageService>, key: &str, value: &str, external: bool) {
    storage.store_with(
        key,
        StorageValue::String(value.to_owned()),
        je_platform::storage::storage::StorageScope::Workspace,
        je_platform::storage::storage::StorageTarget::User,
        external,
    );
}
