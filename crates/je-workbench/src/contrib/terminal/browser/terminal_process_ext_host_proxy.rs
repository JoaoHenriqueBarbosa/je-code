//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalProcessExtHostProxy.ts`: o processo de um terminal de extensão visto
//! da thread principal (`TerminalProcessExtHostProxy`): os dados e os eventos que o host de extensões envia viram eventos do
//! processo, e o que a instância faz (entrada, redimensionamento, desligamento, pedido de cwd) vira evento para o host.
//!
//! Divergências:
//! - O `Disposable` do original é um [`Disposable`] por composição; o objeto nasce em `Rc::new_cyclic` porque `start` passa o
//!   próprio proxy (como `Rc<dyn ITerminalProcessExtHostProxy>`) ao serviço de terminais.
//! - `onStart` (um emissor que nada dispara) fica de fora.
//! - `emitReady(pid, cwd, windowsPty)` do `ITerminalProcessExtHostProxy` ignora o `windowsPty` e dispara `windowsPty: undefined`,
//!   como a classe do original.
//! - `getInitialCwd()` e `getCwd()` guardam o [`Resolver`] da promessa; `emitInitialCwd` e `emitCwd` resolvem os pendentes do último
//!   para o primeiro (`pop`).
//! - `refreshProperty` lança (a promessa rejeitada, como a `async` do original). `acknowledgeDataEvent`, `setUnicodeVersion`,
//!   `clearBuffer` e `sendSignal` não fazem nada.
//! - `shouldPersist` é falso e `id` é 0.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{Promise, Resolver};
use je_platform::terminal::terminal::{
    IProcessProperty, IProcessReadyEvent, IProcessReadyWindowsPty, IShellLaunchConfig, ITerminalChildProcess, ITerminalDimensions, ITerminalDimensionsOverride,
    ITerminalLaunchOutcome, ProcessData, ProcessPropertyType, UnicodeVersion,
};

use super::terminal::ITerminalService;
use crate::contrib::terminal::common::terminal::ITerminalProcessExtHostProxy;

/// `TerminalProcessExtHostProxy`.
pub struct TerminalProcessExtHostProxy {
    this: Weak<Self>,
    base: Rc<Disposable>,
    instance_id: Cell<u32>,
    cols: i32,
    rows: i32,
    terminal_service: Rc<dyn ITerminalService>,

    on_process_data: Rc<Emitter<ProcessData>>,
    on_process_ready: Rc<Emitter<IProcessReadyEvent>>,
    on_input: Rc<Emitter<String>>,
    on_binary: Rc<Emitter<String>>,
    on_resize: Rc<Emitter<ITerminalDimensions>>,
    on_acknowledge_data_event: Rc<Emitter<usize>>,
    on_shutdown: Rc<Emitter<bool>>,
    on_request_initial_cwd: Rc<Emitter<()>>,
    on_request_cwd: Rc<Emitter<()>>,
    on_did_change_property: Rc<Emitter<IProcessProperty>>,
    on_process_exit: Rc<Emitter<Option<i32>>>,

    pending_initial_cwd_requests: RefCell<Vec<Resolver<String>>>,
    pending_cwd_requests: RefCell<Vec<Resolver<String>>>,
}

/// Cria um emissor registrado no `Disposable` (o `this._register(new Emitter())`).
fn registered<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

impl TerminalProcessExtHostProxy {
    pub fn new(instance_id: u32, cols: i32, rows: i32, terminal_service: Rc<dyn ITerminalService>) -> Rc<Self> {
        let base = Disposable::new();
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            instance_id: Cell::new(instance_id),
            cols,
            rows,
            terminal_service,
            on_process_data: registered(&base),
            on_process_ready: registered(&base),
            on_input: registered(&base),
            on_binary: registered(&base),
            on_resize: registered(&base),
            on_acknowledge_data_event: registered(&base),
            on_shutdown: registered(&base),
            on_request_initial_cwd: registered(&base),
            on_request_cwd: registered(&base),
            on_did_change_property: registered(&base),
            on_process_exit: registered(&base),
            base,
            pending_initial_cwd_requests: RefCell::new(Vec::new()),
            pending_cwd_requests: RefCell::new(Vec::new()),
        })
    }

    /// `emitTitle(title)`.
    pub fn emit_title(&self, title: &str) {
        self.on_did_change_property.fire(IProcessProperty::Title(title.to_owned()));
    }

    /// `emitOverrideDimensions(dimensions)`.
    pub fn emit_override_dimensions(&self, dimensions: Option<ITerminalDimensionsOverride>) {
        self.on_did_change_property.fire(IProcessProperty::OverrideDimensions(dimensions));
    }

    /// `emitResolvedShellLaunchConfig(shellLaunchConfig)`.
    pub fn emit_resolved_shell_launch_config(&self, shell_launch_config: IShellLaunchConfig) {
        self.on_did_change_property.fire(IProcessProperty::ResolvedShellLaunchConfig(shell_launch_config));
    }

    /// `emitInitialCwd(initialCwd)`.
    pub fn emit_initial_cwd(&self, initial_cwd: &str) {
        loop {
            let pending = self.pending_initial_cwd_requests.borrow_mut().pop();
            match pending {
                Some(resolve) => resolve.resolve(initial_cwd.to_owned()),
                None => break,
            }
        }
    }

    /// `emitCwd(cwd)`.
    pub fn emit_cwd(&self, cwd: &str) {
        loop {
            let pending = self.pending_cwd_requests.borrow_mut().pop();
            match pending {
                Some(resolve) => resolve.resolve(cwd.to_owned()),
                None => break,
            }
        }
    }
}

impl IDisposable for TerminalProcessExtHostProxy {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalProcessExtHostProxy for TerminalProcessExtHostProxy {
    fn instance_id(&self) -> u32 {
        self.instance_id.get()
    }

    fn emit_data(&self, data: &str) {
        self.on_process_data.fire(ProcessData::Text(data.to_owned()));
    }

    fn emit_process_property(&self, property: IProcessProperty) {
        match property {
            IProcessProperty::Cwd(cwd) => self.emit_cwd(&cwd),
            IProcessProperty::InitialCwd(initial_cwd) => self.emit_initial_cwd(&initial_cwd),
            IProcessProperty::Title(title) => self.emit_title(&title),
            IProcessProperty::OverrideDimensions(dimensions) => self.emit_override_dimensions(dimensions),
            IProcessProperty::ResolvedShellLaunchConfig(config) => self.emit_resolved_shell_launch_config(config),
            _ => {}
        }
    }

    fn emit_ready(&self, pid: i32, cwd: &str, _windows_pty: Option<IProcessReadyWindowsPty>) {
        self.on_process_ready.fire(IProcessReadyEvent { pid, cwd: cwd.to_owned(), windows_pty: None });
    }

    fn emit_exit(&self, exit_code: Option<i32>) {
        self.on_process_exit.fire(exit_code);
        self.dispose();
    }

    fn on_input(&self) -> Event<String> {
        self.on_input.event()
    }

    fn on_binary(&self) -> Event<String> {
        self.on_binary.event()
    }

    fn on_resize(&self) -> Event<ITerminalDimensions> {
        self.on_resize.event()
    }

    fn on_acknowledge_data_event(&self) -> Event<usize> {
        self.on_acknowledge_data_event.event()
    }

    fn on_shutdown(&self) -> Event<bool> {
        self.on_shutdown.event()
    }

    fn on_request_initial_cwd(&self) -> Event<()> {
        self.on_request_initial_cwd.event()
    }

    fn on_request_cwd(&self) -> Event<()> {
        self.on_request_cwd.event()
    }
}

impl ITerminalChildProcess for TerminalProcessExtHostProxy {
    fn id(&self) -> u32 {
        0
    }

    fn should_persist(&self) -> bool {
        false
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        self.on_process_data.event()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        self.on_process_ready.event()
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        self.on_did_change_property.event()
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        self.on_process_exit.event()
    }

    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>> {
        let proxy: Rc<dyn ITerminalProcessExtHostProxy> = self.this.upgrade().expect("o proxy está vivo enquanto um método dele roda");
        self.terminal_service.request_start_extension_terminal(proxy, self.cols, self.rows).then(|error| error.map(ITerminalLaunchOutcome::Error))
    }

    fn shutdown(&self, immediate: bool) {
        self.on_shutdown.fire(immediate);
    }

    fn input(&self, data: &str) {
        self.on_input.fire(data.to_owned());
    }

    fn send_signal(&self, _signal: &str) {
        // No-op - Extension terminals don't have direct process access
    }

    fn process_binary(&self, data: &str) -> Promise<()> {
        // Disabled for extension terminals
        self.on_binary.fire(data.to_owned());
        Promise::resolved(())
    }

    fn resize(&self, cols: i32, rows: i32, _pixel_width: Option<i32>, _pixel_height: Option<i32>) -> Result<(), Error> {
        self.on_resize.fire(ITerminalDimensions { cols, rows });
        Ok(())
    }

    fn clear_buffer(&self) -> Promise<()> {
        // no-op
        Promise::resolved(())
    }

    fn acknowledge_data_event(&self, _char_count: usize) {
        // Flow control is disabled for extension terminals
    }

    fn set_unicode_version(&self, _version: UnicodeVersion) -> Promise<()> {
        // No-op
        Promise::resolved(())
    }

    fn get_initial_cwd(&self) -> Promise<String> {
        Promise::with_executor(|resolve| {
            self.on_request_initial_cwd.fire(());
            self.pending_initial_cwd_requests.borrow_mut().push(resolve);
        })
    }

    fn get_cwd(&self) -> Promise<String> {
        Promise::with_executor(|resolve| {
            self.on_request_cwd.fire(());
            self.pending_cwd_requests.borrow_mut().push(resolve);
        })
    }

    fn refresh_property(&self, _property: ProcessPropertyType) -> Promise<IProcessProperty> {
        // throws if called in extHostTerminalService
        Promise::rejected(Error::new("refreshProperty not implemented on extension host"))
    }

    fn update_property(&self, _property: IProcessProperty) -> Promise<()> {
        // throws if called in extHostTerminalService
        Promise::resolved(())
    }
}
