//! Porte de `src/vs/workbench/contrib/debug/browser/rawDebugSession.ts`: o `RawDebugSession`, que encapsula o ciclo de
//! vida do adaptador de depuração e as idiossincrasias do Debug Adapter Protocol, e o trait [`IRawDebugSession`], que é o
//! que a `DebugSession` lógica usa dele (os dublês dos testes, como o `MockRawSession`, implementam o trait).
//!
//! Notas de tradução:
//! - **Tipos do DAP.** Argumentos e capacidades são [`serde_json::Value`]; as respostas são `Option<Response>` (o `undefined`
//!   do original é `None`); os eventos são o [`Event`](super::debug_protocol::Event) cru.
//! - `send` do original rejeita com a *resposta* de erro e o `.then(undefined, err => Promise.reject(handleErrorResponse(...)))`
//!   a converte num `Error`; aqui a promessa interna resolve com `Result<resposta, resposta de erro>` e a externa faz a
//!   conversão na mesma rodada de microtarefas.
//! - Os serviços do construtor (`IExtensionHostDebugService`, `IOpenerService`, `INotificationService`, `IDialogService`)
//!   são o recorte [`IRawSessionServices`]; o `undefined!` dos testes é `None`.
//! - `createErrorWithActions` (o erro com a ação "More Info") não existe no porte: o erro com `url` é um `Error` que leva a
//!   mensagem e, nas `props`, `actionId`, `actionLabel` e `actionUrl` (a ação que abriria a URL fica a cargo de quem
//!   exibe o erro).
//! - `new Date().getTime()` é [`runtime::date_now`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{Promise, Resolver};
use je_base::runtime;
use je_base::uri::URI;
use serde_json::{Value, json};

use super::debug::{IConfig, IDebugAdapter};
use super::debug_protocol::{Event as ProtocolEvent, Request, Response};
use super::debug_types::{AdapterEndEvent, DapResponse, IDebugger, capability};
use super::debug_utils::{format_pii, is_uri_string};

/// O resultado de `openExtensionDevelopmentHostWindow`: `IOpenExtensionWindowResult`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IOpenExtensionWindowResult {
    pub success: bool,
    pub renderer_debug_addr: Option<String>,
}

/// Os serviços que o `RawDebugSession` usa (extension host de depuração, abridor, notificações e diálogos).
pub trait IRawSessionServices {
    /// `extensionHostDebugService.openExtensionDevelopmentHostWindow(args, debugRenderer)`.
    fn open_extension_development_host_window(&self, args: Vec<String>, debug_renderer: bool) -> Promise<IOpenExtensionWindowResult>;
    /// `dialogService.confirm(...)` do aviso de bloqueio do navegador: `confirmed`.
    fn confirm_browser_blocked(&self, message: &str, primary_button: &str) -> Promise<bool>;
    /// `notificationService.error(message)`.
    fn notify_error(&self, message: &str);
    /// `openerService.open(uri, { allowCommands: true })`.
    fn open(&self, uri: &URI) -> Promise<bool>;
}

/// O que a `DebugSession` usa do `RawDebugSession`.
pub trait IRawDebugSession: IDisposable {
    fn is_in_shutdown(&self) -> bool;
    fn dbgr(&self) -> Option<Rc<dyn IDebugger>>;
    fn capabilities(&self) -> Value;
    fn ready_for_breakpoints(&self) -> bool;
    fn on_did_exit_adapter(&self) -> Event<AdapterEndEvent>;
    fn on_did_initialize(&self) -> Event<ProtocolEvent>;
    fn on_did_stop(&self) -> Event<ProtocolEvent>;
    fn on_did_continued(&self) -> Event<ProtocolEvent>;
    fn on_did_terminate_debugee(&self) -> Event<ProtocolEvent>;
    fn on_did_exit_debugee(&self) -> Event<ProtocolEvent>;
    fn on_did_thread(&self) -> Event<ProtocolEvent>;
    fn on_did_output(&self) -> Event<ProtocolEvent>;
    fn on_did_breakpoint(&self) -> Event<ProtocolEvent>;
    fn on_did_loaded_source(&self) -> Event<ProtocolEvent>;
    fn on_did_custom_event(&self) -> Event<ProtocolEvent>;
    fn on_did_progress_start(&self) -> Event<ProtocolEvent>;
    fn on_did_progress_update(&self) -> Event<ProtocolEvent>;
    fn on_did_progress_end(&self) -> Event<ProtocolEvent>;
    fn on_did_invalidated(&self) -> Event<ProtocolEvent>;
    fn on_did_invalidate_memory(&self) -> Event<ProtocolEvent>;
    fn on_did_event(&self) -> Event<ProtocolEvent>;

    fn start(&self) -> Promise<()>;
    fn initialize(&self, args: Value) -> Promise<DapResponse>;
    fn disconnect(&self, args: Value) -> Promise<()>;
    fn launch_or_attach(&self, config: IConfig) -> Promise<DapResponse>;
    fn terminate(&self, restart: bool) -> Promise<DapResponse>;
    fn restart(&self, args: Value) -> Promise<DapResponse>;
    fn next(&self, args: Value) -> Promise<DapResponse>;
    fn step_in(&self, args: Value) -> Promise<DapResponse>;
    fn step_out(&self, args: Value) -> Promise<DapResponse>;
    fn continue_(&self, args: Value) -> Promise<DapResponse>;
    fn pause(&self, args: Value) -> Promise<DapResponse>;
    fn terminate_threads(&self, args: Value) -> Promise<DapResponse>;
    fn set_variable(&self, args: Value) -> Promise<DapResponse>;
    fn set_expression(&self, args: Value) -> Promise<DapResponse>;
    fn restart_frame(&self, args: Value, thread_id: i64) -> Promise<DapResponse>;
    fn step_in_targets(&self, args: Value) -> Promise<DapResponse>;
    fn completions(&self, args: Value, token: CancellationToken) -> Promise<DapResponse>;
    fn set_breakpoints(&self, args: Value) -> Promise<DapResponse>;
    fn set_function_breakpoints(&self, args: Value) -> Promise<DapResponse>;
    fn data_breakpoint_info(&self, args: Value) -> Promise<DapResponse>;
    fn set_data_breakpoints(&self, args: Value) -> Promise<DapResponse>;
    fn set_exception_breakpoints(&self, args: Value) -> Promise<DapResponse>;
    fn breakpoint_locations(&self, args: Value) -> Promise<DapResponse>;
    fn configuration_done(&self) -> Promise<DapResponse>;
    fn stack_trace(&self, args: Value, token: CancellationToken) -> Promise<DapResponse>;
    fn exception_info(&self, args: Value) -> Promise<DapResponse>;
    fn scopes(&self, args: Value, token: CancellationToken) -> Promise<DapResponse>;
    fn variables(&self, args: Value, token: Option<CancellationToken>) -> Promise<DapResponse>;
    fn source(&self, args: Value) -> Promise<DapResponse>;
    fn locations(&self, args: Value) -> Promise<DapResponse>;
    fn loaded_sources(&self, args: Value) -> Promise<DapResponse>;
    fn threads(&self) -> Promise<DapResponse>;
    fn evaluate(&self, args: Value) -> Promise<DapResponse>;
    fn step_back(&self, args: Value) -> Promise<DapResponse>;
    fn reverse_continue(&self, args: Value) -> Promise<DapResponse>;
    fn goto_targets(&self, args: Value) -> Promise<DapResponse>;
    fn goto(&self, args: Value) -> Promise<DapResponse>;
    fn set_instruction_breakpoints(&self, args: Value) -> Promise<DapResponse>;
    fn disassemble(&self, args: Value) -> Promise<DapResponse>;
    fn read_memory(&self, args: Value) -> Promise<DapResponse>;
    fn write_memory(&self, args: Value) -> Promise<DapResponse>;
    fn cancel(&self, args: Value) -> Promise<DapResponse>;
    fn custom(&self, request: &str, args: Value) -> Promise<DapResponse>;
}

/// Um `Result<resposta, resposta de erro>` que a promessa interna de `send` carrega.
type SendOutcome = Result<DapResponse, Response>;

/// `mixin(capabilities, new)`: os campos novos sobrescrevem os antigos.
fn merge_capabilities(current: &mut Value, new: &Value) {
    if let (Value::Object(current), Value::Object(new)) = (current, new) {
        for (key, value) in new {
            current.insert(key.clone(), value.clone());
        }
    }
}

/// Encapsulates the DebugAdapter lifecycle and some idiosyncrasies of the Debug Adapter Protocol.
pub struct RawDebugSession {
    all_threads_continued: Cell<bool>,
    ready_for_breakpoints: Cell<bool>,
    capabilities: RefCell<Value>,

    // shutdown
    debug_adapter_stopped: Cell<bool>,
    in_shutdown: Cell<bool>,
    terminated: Cell<bool>,
    fired_adapter_exit_event: Cell<bool>,

    // telemetry
    start_time: Cell<u64>,
    did_receive_stopped_event: Cell<bool>,

    to_dispose: Rc<DisposableStore>,

    // DAP events
    on_did_initialize: Rc<Emitter<ProtocolEvent>>,
    on_did_stop: Rc<Emitter<ProtocolEvent>>,
    on_did_continued: Rc<Emitter<ProtocolEvent>>,
    on_did_terminate_debugee: Rc<Emitter<ProtocolEvent>>,
    on_did_exit_debugee: Rc<Emitter<ProtocolEvent>>,
    on_did_thread: Rc<Emitter<ProtocolEvent>>,
    on_did_output: Rc<Emitter<ProtocolEvent>>,
    on_did_breakpoint: Rc<Emitter<ProtocolEvent>>,
    on_did_loaded_source: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_start: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_update: Rc<Emitter<ProtocolEvent>>,
    on_did_progress_end: Rc<Emitter<ProtocolEvent>>,
    on_did_invalidated: Rc<Emitter<ProtocolEvent>>,
    on_did_invalidate_memory: Rc<Emitter<ProtocolEvent>>,
    on_did_custom_event: Rc<Emitter<ProtocolEvent>>,
    on_did_event: Rc<Emitter<ProtocolEvent>>,

    // DA events
    on_did_exit_adapter: Rc<Emitter<AdapterEndEvent>>,

    debug_adapter: RefCell<Option<Rc<dyn IDebugAdapter>>>,
    stopped_since_last_step: Cell<bool>,

    dbgr: Option<Rc<dyn IDebugger>>,
    session_id: String,
    name: String,
    services: Option<Rc<dyn IRawSessionServices>>,
    me: std::rc::Weak<RawDebugSession>,
}

impl RawDebugSession {
    pub fn new(
        debug_adapter: Rc<dyn IDebugAdapter>,
        dbgr: Option<Rc<dyn IDebugger>>,
        session_id: &str,
        name: &str,
        services: Option<Rc<dyn IRawSessionServices>>,
    ) -> Rc<Self> {
        let to_dispose = DisposableStore::new();
        let emitter = |store: &Rc<DisposableStore>| -> Rc<Emitter<ProtocolEvent>> {
            let emitter = Emitter::new();
            store.add(emitter.clone());
            emitter
        };
        let on_did_exit_adapter = Emitter::new();
        to_dispose.add(on_did_exit_adapter.clone());

        let this = Rc::new_cyclic(|me| Self {
            all_threads_continued: Cell::new(true),
            ready_for_breakpoints: Cell::new(false),
            capabilities: RefCell::new(json!({})),
            debug_adapter_stopped: Cell::new(false),
            in_shutdown: Cell::new(false),
            terminated: Cell::new(false),
            fired_adapter_exit_event: Cell::new(false),
            start_time: Cell::new(0),
            did_receive_stopped_event: Cell::new(false),
            on_did_initialize: emitter(&to_dispose),
            on_did_stop: emitter(&to_dispose),
            on_did_continued: emitter(&to_dispose),
            on_did_terminate_debugee: emitter(&to_dispose),
            on_did_exit_debugee: emitter(&to_dispose),
            on_did_thread: emitter(&to_dispose),
            on_did_output: emitter(&to_dispose),
            on_did_breakpoint: emitter(&to_dispose),
            on_did_loaded_source: emitter(&to_dispose),
            on_did_progress_start: emitter(&to_dispose),
            on_did_progress_update: emitter(&to_dispose),
            on_did_progress_end: emitter(&to_dispose),
            on_did_invalidated: emitter(&to_dispose),
            on_did_invalidate_memory: emitter(&to_dispose),
            on_did_custom_event: emitter(&to_dispose),
            on_did_event: emitter(&to_dispose),
            on_did_exit_adapter,
            to_dispose: to_dispose.clone(),
            debug_adapter: RefCell::new(Some(debug_adapter.clone())),
            stopped_since_last_step: Cell::new(false),
            dbgr,
            session_id: session_id.to_owned(),
            name: name.to_owned(),
            services,
            me: me.clone(),
        });

        let weak = Rc::downgrade(&this);
        to_dispose.add(debug_adapter.on_error().listen(move |err| {
            if let Some(this) = weak.upgrade() {
                this.shutdown(Some(err.clone()), false, None, None);
            }
        }));
        let weak = Rc::downgrade(&this);
        to_dispose.add(debug_adapter.on_exit().listen(move |code| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if *code != Some(0) {
                let text = match code {
                    Some(code) => code.to_string(),
                    None => "undefined".to_owned(),
                };
                this.shutdown(Some(Error::new(format!("exit code: {text}"))), false, None, None);
            } else {
                // normal exit
                this.shutdown(None, false, None, None);
            }
        }));

        let weak = Rc::downgrade(&this);
        debug_adapter.on_event(Box::new(move |event| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            match event.event.as_str() {
                "initialized" => {
                    this.ready_for_breakpoints.set(true);
                    this.on_did_initialize.fire(event.clone());
                }
                "loadedSource" => this.on_did_loaded_source.fire(event.clone()),
                "capabilities" => {
                    if let Some(body) = &event.body {
                        let capabilities = body.get("capabilities").cloned().unwrap_or(Value::Null);
                        this.merge_capabilities(Some(&capabilities));
                    }
                }
                "stopped" => {
                    this.did_receive_stopped_event.set(true); // telemetry: remember that debugger stopped successfully
                    this.stopped_since_last_step.set(true);
                    this.on_did_stop.fire(event.clone());
                }
                "continued" => {
                    let all = event.body.as_ref().and_then(|b| b.get("allThreadsContinued")).and_then(Value::as_bool) != Some(false);
                    this.all_threads_continued.set(all);
                    this.on_did_continued.fire(event.clone());
                }
                "thread" => this.on_did_thread.fire(event.clone()),
                "output" => this.on_did_output.fire(event.clone()),
                "breakpoint" => this.on_did_breakpoint.fire(event.clone()),
                "terminated" => this.on_did_terminate_debugee.fire(event.clone()),
                "exited" => this.on_did_exit_debugee.fire(event.clone()),
                "progressStart" => this.on_did_progress_start.fire(event.clone()),
                "progressUpdate" => this.on_did_progress_update.fire(event.clone()),
                "progressEnd" => this.on_did_progress_end.fire(event.clone()),
                "invalidated" => this.on_did_invalidated.fire(event.clone()),
                "memory" => this.on_did_invalidate_memory.fire(event.clone()),
                "process" | "module" => {}
                _ => this.on_did_custom_event.fire(event.clone()),
            }
            this.on_did_event.fire(event.clone());
        }));

        let weak = Rc::downgrade(&this);
        debug_adapter.on_request(Box::new(move |request| {
            if let Some(this) = weak.upgrade() {
                this.dispatch_request(request.clone());
            }
        }));

        this
    }

    fn this(&self) -> Rc<RawDebugSession> {
        self.me.upgrade().expect("a sessão bruta está viva")
    }

    //---- DebugAdapter lifecycle

    /// `send(command, args, token, timeout, showErrors)`.
    fn send(&self, command: &str, args: Value, token: Option<CancellationToken>, timeout: Option<u64>, show_errors: bool) -> Promise<DapResponse> {
        let this = self.this();
        let command = command.to_owned();
        let inner = Promise::<SendOutcome>::with_executor({
            let this = this.clone();
            move |resolver: Resolver<SendOutcome>| {
                let adapter = this.debug_adapter.borrow().clone();
                let Some(adapter) = adapter else {
                    if this.in_shutdown.get() {
                        // We are in shutdown silently complete
                        resolver.resolve(Ok(None));
                    } else {
                        resolver.resolve(Err(Response {
                            success: false,
                            message: Some(localize("noDebugAdapter", "No debugger available found. Can not send '{0}'.", &[&command])),
                            ..Response::default()
                        }));
                    }
                    return;
                };

                let cancelation_listener: Rc<RefCell<Option<je_base::lifecycle::DisposableRef>>> = Rc::new(RefCell::new(None));
                let listener_for_callback = cancelation_listener.clone();
                let request_id = adapter.send_request(
                    &command,
                    if args.is_null() { None } else { Some(&args) },
                    Rc::new(move |response: &Response| {
                        if let Some(listener) = listener_for_callback.borrow_mut().take() {
                            listener.dispose();
                        }
                        if response.success {
                            resolver.resolve(Ok(Some(response.clone())));
                        } else {
                            resolver.resolve(Err(response.clone()));
                        }
                    }),
                    timeout,
                );

                if let Some(token) = token {
                    let listener_slot = cancelation_listener.clone();
                    let session = this.clone();
                    let subscription = token.on_cancellation_requested().listen(move |_| {
                        if let Some(listener) = listener_slot.borrow_mut().take() {
                            listener.dispose();
                        }
                        if capability(&session.capabilities.borrow(), "supportsCancelRequest") {
                            session.cancel(json!({ "requestId": request_id }));
                        }
                    });
                    *cancelation_listener.borrow_mut() = Some(subscription);
                }
            }
        });

        inner.then_result(move |outcome| match outcome {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error_response)) => Err(this.handle_error_response(&error_response, show_errors)),
            Err(error) => Err(error),
        })
    }

    fn handle_error_response(&self, error_response: &Response, show_errors: bool) -> Error {
        if error_response.command == "canceled" && error_response.message.as_deref() == Some("canceled") {
            return Error::cancellation();
        }

        let error: Option<Value> = error_response.body.as_ref().and_then(|b| b.get("error")).cloned().filter(|e| e.is_object());
        let error_message = error_response.message.clone().unwrap_or_default();
        let format = error.as_ref().and_then(|e| e.get("format")).and_then(Value::as_str).map(str::to_owned);
        let user_message = match (&error, &format) {
            (Some(error), Some(format)) => {
                let variables: Option<HashMap<String, String>> = error.get("variables").and_then(Value::as_object).map(|v| {
                    v.iter()
                        .map(|(key, value)| (key.clone(), value.as_str().map_or_else(|| value.to_string(), str::to_owned)))
                        .collect()
                });
                format_pii(format, false, variables.as_ref())
            }
            (Some(_), None) => format_pii("", false, None),
            _ => error_message,
        };
        let url = error.as_ref().and_then(|e| e.get("url")).and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_owned);
        if let (Some(_), Some(url)) = (&error, url) {
            let label = error
                .as_ref()
                .and_then(|e| e.get("urlLabel"))
                .and_then(Value::as_str)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| localize("moreInfo", "More Info", &[]));
            let uri = URI::parse(&url, false);
            // Use a suffixed id if uri invokes a command, so default 'Open launch.json' command is suppressed on dialog
            let action_id = if *uri.scheme() == je_base::network::schemas::COMMAND { "debug.moreInfo.command" } else { "debug.moreInfo" };
            let mut result = Error::new(user_message);
            result.props.push(("actionId".to_owned(), action_id.to_owned()));
            result.props.push(("actionLabel".to_owned(), label));
            result.props.push(("actionUrl".to_owned(), url));
            return result;
        }

        let show_user = error.as_ref().and_then(|e| e.get("showUser")).and_then(Value::as_bool);
        if show_errors && error.is_some() && format.as_deref().is_some_and(|f| !f.is_empty()) && show_user == Some(true)
            && let Some(services) = &self.services
        {
            services.notify_error(&user_message);
        }

        let mut result = Error::named("ErrorNoTelemetry", user_message);
        if let Some(show_user) = show_user {
            result.props.push(("showUser".to_owned(), show_user.to_string()));
        }
        result
    }

    fn merge_capabilities(&self, capabilities: Option<&Value>) {
        if let Some(capabilities) = capabilities.filter(|c| !c.is_null()) {
            merge_capabilities(&mut self.capabilities.borrow_mut(), capabilities);
        }
    }

    fn fire_simulated_continued_event(&self, thread_id: i64, all_threads_continued: bool) {
        self.on_did_continued.fire(ProtocolEvent {
            seq: 0,
            event: "continued".to_owned(),
            body: Some(json!({ "threadId": thread_id, "allThreadsContinued": all_threads_continued })),
        });
    }

    /// Requisição com checagem de capacidade (`if (this.capabilities.x) return this.send(...); return Promise.reject(new Error(msg))`).
    fn send_if(&self, capability_key: &str, command: &str, args: Value, token: Option<CancellationToken>, not_supported: &str) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), capability_key) {
            return self.send(command, args, token, None, true);
        }
        Promise::rejected(Error::new(not_supported))
    }

    /// A requisição de passo: zera `stoppedSinceLastStep`, envia e, se nenhum `stopped` chegou, simula o `continued`.
    fn send_step(&self, command: &str, args: Value, thread_id: i64, all_threads_continued: bool, from_continue: bool) -> Promise<DapResponse> {
        let this = self.this();
        let command = command.to_owned();
        async_fn(async move {
            this.stopped_since_last_step.set(false);
            let response = this.send(&command, args, None, None, true).await?;
            if from_continue
                && let Some(all) = response.as_ref().and_then(|r| r.body.as_ref()).and_then(|b| b.get("allThreadsContinued")).and_then(Value::as_bool)
            {
                this.all_threads_continued.set(all);
            }
            if !this.stopped_since_last_step.get() {
                let all = if from_continue { this.all_threads_continued.get() } else { all_threads_continued };
                this.fire_simulated_continued_event(thread_id, all);
            }
            Ok(response)
        })
    }

    fn shutdown(&self, error: Option<Error>, restart: bool, terminate_debuggee: Option<bool>, suspend_debuggee: Option<bool>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if !this.in_shutdown.get() {
                this.in_shutdown.set(true);
                if this.debug_adapter.borrow().is_some() {
                    let mut args = json!({ "restart": restart });
                    if let Some(terminate_debuggee) = terminate_debuggee {
                        args["terminateDebuggee"] = json!(terminate_debuggee);
                    }
                    if let Some(suspend_debuggee) = suspend_debuggee {
                        args["suspendDebuggee"] = json!(suspend_debuggee);
                    }

                    // if there's an error, the DA is probably already gone, so give it a much shorter timeout.
                    // Catch the potential 'disconnect' error - no need to show it to the user since the adapter is shutting down
                    let _ = this.send("disconnect", args, None, Some(if error.is_some() { 200 } else { 2000 }), true).await;
                    this.stop_adapter(error).await?;
                } else {
                    return this.stop_adapter(error).await;
                }
            }
            Ok(())
        })
    }

    fn stop_adapter(&self, error: Option<Error>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let da = this.debug_adapter.borrow_mut().take();
            let result = match da {
                Some(da) => {
                    let result = da.stop_session().await;
                    if result.is_ok() {
                        this.debug_adapter_stopped.set(true);
                    }
                    result
                }
                None => Ok(()),
            };
            this.fire_adapter_exit_event(error);
            result
        })
    }

    fn fire_adapter_exit_event(&self, error: Option<Error>) {
        if !self.fired_adapter_exit_event.get() {
            self.fired_adapter_exit_event.set(true);
            let mut e = AdapterEndEvent {
                emitted_stopped: self.did_receive_stopped_event.get(),
                session_length_in_seconds: (runtime::date_now() as f64 - self.start_time.get() as f64) / 1000.0,
                error: None,
            };
            if error.is_some() && !self.debug_adapter_stopped.get() {
                e.error = error;
            }
            self.on_did_exit_adapter.fire(e);
        }
    }

    fn dispatch_request(&self, request: Request) {
        let this = self.this();
        async_fn(async move {
            let mut response = Response { seq: 0, request_seq: request.seq, success: true, command: request.command.clone(), message: None, body: None };

            let safe_send_response = |this: &Rc<RawDebugSession>, response: &Response| {
                let adapter = this.debug_adapter.borrow().clone();
                if let Some(adapter) = adapter {
                    adapter.send_response(&mut response.clone());
                }
            };

            if request.command == "launchVSCode" {
                let outcome: Result<Response, Error> = async {
                    let arguments = request.arguments.clone().unwrap_or(Value::Null);
                    let mut result = this.launch_vs_code(&arguments).await?;
                    if !result.success {
                        let confirmed = match &this.services {
                            Some(services) => {
                                services
                                    .confirm_browser_blocked(
                                        &localize(
                                            "canNotStart",
                                            "The debugger needs to open a new tab or window for the debuggee but the browser prevented this. You must give permission to continue.",
                                            &[],
                                        ),
                                        &localize("continue", "&&Continue", &[]),
                                    )
                                    .await?
                            }
                            None => false,
                        };
                        if confirmed {
                            result = this.launch_vs_code(&arguments).await?;
                        } else {
                            response.success = false;
                            safe_send_response(&this, &response);
                            this.shutdown(None, false, None, None).await?;
                        }
                    }
                    response.body = Some(json!({ "rendererDebugAddr": result.renderer_debug_addr }));
                    Ok(response.clone())
                }
                .await;
                match outcome {
                    Ok(response) => safe_send_response(&this, &response),
                    Err(err) => {
                        response.success = false;
                        response.message = Some(err.message);
                        safe_send_response(&this, &response);
                    }
                }
            } else if request.command == "runInTerminal" {
                let outcome: Result<Option<i64>, Error> = match &this.dbgr {
                    Some(dbgr) => dbgr.run_in_terminal(request.arguments.clone().unwrap_or(Value::Null), &this.session_id).await,
                    None => Err(Error::new("Cannot read properties of undefined (reading 'runInTerminal')")),
                };
                match outcome {
                    Ok(shell_process_id) => {
                        response.body = Some(match shell_process_id {
                            Some(id) => json!({ "shellProcessId": id }),
                            None => json!({}),
                        });
                        safe_send_response(&this, &response);
                    }
                    Err(err) => {
                        response.success = false;
                        response.message = Some(err.message);
                        safe_send_response(&this, &response);
                    }
                }
            } else if request.command == "startDebugging" {
                let outcome: Result<bool, Error> = match &this.dbgr {
                    Some(dbgr) => {
                        let args = request.arguments.clone().unwrap_or(Value::Null);
                        let mut config = args.get("configuration").cloned().unwrap_or_else(|| json!({}));
                        let configuration_name = config.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_owned);
                        config["request"] = args.get("request").cloned().unwrap_or(Value::Null);
                        config["type"] = json!(dbgr.debug_type());
                        config["name"] = json!(configuration_name.unwrap_or_else(|| this.name.clone()));
                        dbgr.start_debugging(config, &this.session_id).await
                    }
                    None => Err(Error::new("Cannot read properties of undefined (reading 'startDebugging')")),
                };
                match outcome {
                    Ok(true) => safe_send_response(&this, &response),
                    Ok(false) => {
                        response.success = false;
                        response.message = Some("Failed to start debugging".to_owned());
                        safe_send_response(&this, &response);
                    }
                    Err(err) => {
                        response.success = false;
                        response.message = Some(err.message);
                        safe_send_response(&this, &response);
                    }
                }
            } else {
                response.success = false;
                response.message = Some(format!("unknown request '{}'", request.command));
                safe_send_response(&this, &response);
            }
            Ok(())
        });
    }

    fn launch_vs_code(&self, vscode_args: &Value) -> Promise<IOpenExtensionWindowResult> {
        let mut args: Vec<String> = Vec::new();
        for arg in vscode_args.get("args").and_then(Value::as_array).cloned().unwrap_or_default() {
            let prefix = arg.get("prefix").and_then(Value::as_str).unwrap_or_default();
            let path = arg.get("path").and_then(Value::as_str);
            let a2 = format!("{}{}", prefix, path.unwrap_or_default());
            // `/^--(.+)=(.+)$/`
            let matched = a2.strip_prefix("--").and_then(|rest| {
                let positions: Vec<usize> = rest.match_indices('=').map(|(i, _)| i).collect();
                positions.into_iter().rev().find(|i| *i >= 1 && *i + 1 < rest.len()).map(|i| (rest[..i].to_owned(), rest[i + 1..].to_owned()))
            });
            if let Some((key, mut value)) = matched {
                if (key == "file-uri" || key == "folder-uri") && !path.is_some_and(is_uri_string) {
                    value = if is_uri_string(&value) { value } else { URI::file(&value, je_platform::resources::host_platform()).to_string(false).to_string_lossy() };
                }
                args.push(format!("--{key}={value}"));
            } else {
                args.push(a2);
            }
        }

        if let Some(env) = vscode_args.get("env").filter(|e| !e.is_null()) {
            args.push(format!("--extensionEnvironment={env}"));
        }

        let debug_renderer = vscode_args.get("debugRenderer").and_then(Value::as_bool).unwrap_or(false);
        match &self.services {
            Some(services) => services.open_extension_development_host_window(args, debug_renderer),
            None => Promise::rejected(Error::new("Cannot read properties of undefined (reading 'openExtensionDevelopmentHostWindow')")),
        }
    }
}

impl IRawDebugSession for RawDebugSession {
    fn is_in_shutdown(&self) -> bool {
        self.in_shutdown.get()
    }

    fn dbgr(&self) -> Option<Rc<dyn IDebugger>> {
        self.dbgr.clone()
    }

    fn capabilities(&self) -> Value {
        self.capabilities.borrow().clone()
    }

    /// DA is ready to accepts setBreakpoint requests. Becomes true after "initialized" events has been received.
    fn ready_for_breakpoints(&self) -> bool {
        self.ready_for_breakpoints.get()
    }

    fn on_did_exit_adapter(&self) -> Event<AdapterEndEvent> {
        self.on_did_exit_adapter.event()
    }

    fn on_did_initialize(&self) -> Event<ProtocolEvent> {
        self.on_did_initialize.event()
    }

    fn on_did_stop(&self) -> Event<ProtocolEvent> {
        self.on_did_stop.event()
    }

    fn on_did_continued(&self) -> Event<ProtocolEvent> {
        self.on_did_continued.event()
    }

    fn on_did_terminate_debugee(&self) -> Event<ProtocolEvent> {
        self.on_did_terminate_debugee.event()
    }

    fn on_did_exit_debugee(&self) -> Event<ProtocolEvent> {
        self.on_did_exit_debugee.event()
    }

    fn on_did_thread(&self) -> Event<ProtocolEvent> {
        self.on_did_thread.event()
    }

    fn on_did_output(&self) -> Event<ProtocolEvent> {
        self.on_did_output.event()
    }

    fn on_did_breakpoint(&self) -> Event<ProtocolEvent> {
        self.on_did_breakpoint.event()
    }

    fn on_did_loaded_source(&self) -> Event<ProtocolEvent> {
        self.on_did_loaded_source.event()
    }

    fn on_did_custom_event(&self) -> Event<ProtocolEvent> {
        self.on_did_custom_event.event()
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

    fn on_did_invalidated(&self) -> Event<ProtocolEvent> {
        self.on_did_invalidated.event()
    }

    fn on_did_invalidate_memory(&self) -> Event<ProtocolEvent> {
        self.on_did_invalidate_memory.event()
    }

    fn on_did_event(&self) -> Event<ProtocolEvent> {
        self.on_did_event.event()
    }

    /// Starts the underlying debug adapter and tracks the session time for telemetry.
    fn start(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let adapter = this.debug_adapter.borrow().clone();
            let Some(adapter) = adapter else {
                return Err(Error::new(localize("noDebugAdapterStart", "No debug adapter, can not start debug session.", &[])));
            };
            adapter.start_session().await?;
            this.start_time.set(runtime::date_now());
            Ok(())
        })
    }

    /// Send client capabilities to the debug adapter and receive DA capabilities in return.
    fn initialize(&self, args: Value) -> Promise<DapResponse> {
        let this = self.this();
        async_fn(async move {
            let response = this.send("initialize", args, None, None, false).await?;
            if let Some(response) = &response {
                this.merge_capabilities(response.body.as_ref());
            }
            Ok(response)
        })
    }

    /// Terminate the debuggee and shutdown the adapter
    fn disconnect(&self, args: Value) -> Promise<()> {
        let capabilities = self.capabilities.borrow().clone();
        let terminate_debuggee =
            if capability(&capabilities, "supportTerminateDebuggee") { args.get("terminateDebuggee").and_then(Value::as_bool) } else { None };
        let suspend_debuggee = if capability(&capabilities, "supportTerminateDebuggee") && capability(&capabilities, "supportSuspendDebuggee") {
            args.get("suspendDebuggee").and_then(Value::as_bool)
        } else {
            None
        };
        self.shutdown(None, args.get("restart").and_then(Value::as_bool).unwrap_or(false), terminate_debuggee, suspend_debuggee)
    }

    fn launch_or_attach(&self, config: IConfig) -> Promise<DapResponse> {
        let this = self.this();
        async_fn(async move {
            let command = config.get("request").and_then(Value::as_str).unwrap_or_default().to_owned();
            let response = this.send(&command, config, None, None, false).await?;
            if let Some(response) = &response {
                this.merge_capabilities(response.body.as_ref());
            }
            Ok(response)
        })
    }

    /// Try killing the debuggee softly...
    fn terminate(&self, restart: bool) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), "supportsTerminateRequest") {
            if !self.terminated.get() {
                self.terminated.set(true);
                return self.send("terminate", json!({ "restart": restart }), None, None, true);
            }
            let this = self.this();
            let disconnect = self.disconnect(json!({ "terminateDebuggee": true, "restart": restart }));
            return async_fn(async move {
                let _ = &this;
                disconnect.await?;
                Ok(None)
            });
        }
        Promise::rejected(Error::new("terminated not supported"))
    }

    fn restart(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsRestartRequest", "restart", args, None, "restart not supported")
    }

    fn next(&self, args: Value) -> Promise<DapResponse> {
        let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
        self.send_step("next", args, thread_id, false, false)
    }

    fn step_in(&self, args: Value) -> Promise<DapResponse> {
        let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
        self.send_step("stepIn", args, thread_id, false, false)
    }

    fn step_out(&self, args: Value) -> Promise<DapResponse> {
        let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
        self.send_step("stepOut", args, thread_id, false, false)
    }

    fn continue_(&self, args: Value) -> Promise<DapResponse> {
        let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
        self.send_step("continue", args, thread_id, false, true)
    }

    fn pause(&self, args: Value) -> Promise<DapResponse> {
        self.send("pause", args, None, None, true)
    }

    fn terminate_threads(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsTerminateThreadsRequest", "terminateThreads", args, None, "terminateThreads not supported")
    }

    fn set_variable(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsSetVariable", "setVariable", args, None, "setVariable not supported")
    }

    fn set_expression(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsSetExpression", "setExpression", args, None, "setExpression not supported")
    }

    fn restart_frame(&self, args: Value, thread_id: i64) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), "supportsRestartFrame") {
            return self.send_step("restartFrame", args, thread_id, false, false);
        }
        Promise::rejected(Error::new("restartFrame not supported"))
    }

    fn step_in_targets(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsStepInTargetsRequest", "stepInTargets", args, None, "stepInTargets not supported")
    }

    fn completions(&self, args: Value, token: CancellationToken) -> Promise<DapResponse> {
        self.send_if("supportsCompletionsRequest", "completions", args, Some(token), "completions not supported")
    }

    fn set_breakpoints(&self, args: Value) -> Promise<DapResponse> {
        self.send("setBreakpoints", args, None, None, true)
    }

    fn set_function_breakpoints(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsFunctionBreakpoints", "setFunctionBreakpoints", args, None, "setFunctionBreakpoints not supported")
    }

    fn data_breakpoint_info(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsDataBreakpoints", "dataBreakpointInfo", args, None, "dataBreakpointInfo not supported")
    }

    fn set_data_breakpoints(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsDataBreakpoints", "setDataBreakpoints", args, None, "setDataBreakpoints not supported")
    }

    fn set_exception_breakpoints(&self, args: Value) -> Promise<DapResponse> {
        self.send("setExceptionBreakpoints", args, None, None, true)
    }

    fn breakpoint_locations(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsBreakpointLocationsRequest", "breakpointLocations", args, None, "breakpointLocations is not supported")
    }

    fn configuration_done(&self) -> Promise<DapResponse> {
        self.send_if("supportsConfigurationDoneRequest", "configurationDone", Value::Null, None, "configurationDone not supported")
    }

    fn stack_trace(&self, args: Value, token: CancellationToken) -> Promise<DapResponse> {
        self.send("stackTrace", args, Some(token), None, true)
    }

    fn exception_info(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsExceptionInfoRequest", "exceptionInfo", args, None, "exceptionInfo not supported")
    }

    fn scopes(&self, args: Value, token: CancellationToken) -> Promise<DapResponse> {
        self.send("scopes", args, Some(token), None, true)
    }

    fn variables(&self, args: Value, token: Option<CancellationToken>) -> Promise<DapResponse> {
        self.send("variables", args, token, None, true)
    }

    fn source(&self, args: Value) -> Promise<DapResponse> {
        self.send("source", args, None, None, true)
    }

    fn locations(&self, args: Value) -> Promise<DapResponse> {
        self.send("locations", args, None, None, true)
    }

    fn loaded_sources(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsLoadedSourcesRequest", "loadedSources", args, None, "loadedSources not supported")
    }

    fn threads(&self) -> Promise<DapResponse> {
        self.send("threads", Value::Null, None, None, true)
    }

    fn evaluate(&self, args: Value) -> Promise<DapResponse> {
        self.send("evaluate", args, None, None, true)
    }

    fn step_back(&self, args: Value) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), "supportsStepBack") {
            let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
            return self.send_step("stepBack", args, thread_id, false, false);
        }
        Promise::rejected(Error::new("stepBack not supported"))
    }

    fn reverse_continue(&self, args: Value) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), "supportsStepBack") {
            let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
            return self.send_step("reverseContinue", args, thread_id, false, false);
        }
        Promise::rejected(Error::new("reverseContinue not supported"))
    }

    fn goto_targets(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsGotoTargetsRequest", "gotoTargets", args, None, "gotoTargets is not supported")
    }

    fn goto(&self, args: Value) -> Promise<DapResponse> {
        if capability(&self.capabilities.borrow(), "supportsGotoTargetsRequest") {
            let thread_id = args.get("threadId").and_then(Value::as_i64).unwrap_or(0);
            return self.send_step("goto", args, thread_id, false, false);
        }
        Promise::rejected(Error::new("goto is not supported"))
    }

    fn set_instruction_breakpoints(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsInstructionBreakpoints", "setInstructionBreakpoints", args, None, "setInstructionBreakpoints is not supported")
    }

    fn disassemble(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsDisassembleRequest", "disassemble", args, None, "disassemble is not supported")
    }

    fn read_memory(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsReadMemoryRequest", "readMemory", args, None, "readMemory is not supported")
    }

    fn write_memory(&self, args: Value) -> Promise<DapResponse> {
        self.send_if("supportsWriteMemoryRequest", "writeMemory", args, None, "writeMemory is not supported")
    }

    fn cancel(&self, args: Value) -> Promise<DapResponse> {
        self.send("cancel", args, None, None, true)
    }

    fn custom(&self, request: &str, args: Value) -> Promise<DapResponse> {
        self.send(request, args, None, None, true)
    }
}

impl IDisposable for RawDebugSession {
    fn dispose(&self) {
        self.to_dispose.dispose();
    }
}
