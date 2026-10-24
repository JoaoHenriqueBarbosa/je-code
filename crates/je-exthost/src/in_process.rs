//! A ponte em processo entre o host de extensões e os clientes `MainThread*` do workbench, para os namespaces da API `vscode` além
//! de `commands`.
//!
//! Os dois lados do RPC são Rust no mesmo processo (decisão de arquitetura de `wip/notes/exthost-plan.md`): para os serviços que esta
//! ponte atende, o "proxy" que um `ExtHost*` obtém de `MainThread*` é o próprio cliente do workbench, sem serialização, e o proxy que
//! o cliente obtém de `ExtHost*` é o serviço do host. Os demais serviços (comandos, extensões, telemetria) seguem pelo `RPCProtocol`.
//!
//! O ciclo que isso cria (o `ExtHostWorkspace` precisa do proxy de `MainThreadWorkspace` ao nascer, e o `MainThreadWorkspace` chama
//! `$initializeWorkspace` ao nascer) é quebrado pelos encaminhadores `Late*`: o cliente do workbench é criado primeiro, com um
//! `ExtHost*` que ainda não existe; as chamadas sem retorno feitas nesse intervalo (`$initializeWorkspace`,
//! `$initializeConfiguration`, `$acceptStaticEntries`) ficam numa fila e são entregues ao serviço real assim que ele nasce
//! ([`Late::bind`]). Uma chamada com retorno antes do `bind` é erro de programação (o host só as faz depois de montado).
//!
//! Divergências:
//! - O original cria cada cliente `MainThread*` pelo registro de `extHostNamedCustomer`, uma vez por host; aqui quem monta os clientes
//!   é o [`IMainThreadBinder`] que o dono do host entrega (o aplicativo ou o teste), porque os clientes dependem de serviços do
//!   workbench (busca, editores, confiança) que o `je-exthost` não conhece.
//! - Sem serialização, um valor que o RPC revive por `URI.revive` ou `cloneAndChange` chega como o tipo Rust, igual ao que o
//!   `RPCProtocol` entregaria depois de revivido.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_platform::configuration::ConfigurationChange;
use je_platform::edit_sessions::EditSessionIdentityMatch;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostConfigurationShape, ExtHostContext, ExtHostStatusBarShape, ExtHostWorkspaceShape, IConfigurationInitData, IMainContext, IRawFileMatch2, IWorkspaceData,
    StatusBarItemDto,
};

use crate::local_context::LocalContext;

/// Quem monta os clientes `MainThread*` em processo.
pub trait IMainThreadBinder {
    /// Cria os clientes que o dono do host quer ligar e os registra em `context` (`context.set(&MainContext::MAIN_THREAD_X, cliente)`).
    /// `context.get_proxy(&ExtHostContext::EXT_HOST_X)` já devolve o encaminhador do serviço do host. O contexto vem como `Rc`
    /// porque alguns clientes (o `MainThreadWorkspace`, `Injectable`) o recebem por valor. Devolve o que precisa ser descartado junto
    /// com o host.
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>>;
}

/// Um encaminhador preguiçoso para o `T` que ainda não existe (ver o cabeçalho do módulo).
pub struct Late<T: ?Sized> {
    target: RefCell<Option<Rc<T>>>,
    pending: RefCell<Vec<Box<dyn FnOnce(&T)>>>,
}

impl<T: ?Sized> Default for Late<T> {
    fn default() -> Self {
        Self { target: RefCell::new(None), pending: RefCell::new(Vec::new()) }
    }
}

impl<T: ?Sized> Late<T> {
    /// Liga o serviço real e entrega a ele, em ordem, as chamadas sem retorno que esperaram.
    pub fn bind(&self, target: Rc<T>) {
        *self.target.borrow_mut() = Some(target.clone());
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for call in pending {
            call(&*target);
        }
    }

    /// Uma chamada sem retorno: entrega já se o serviço existe, senão guarda para o `bind`.
    fn call_void(&self, call: impl FnOnce(&T) + 'static) {
        let target = self.target.borrow().clone();
        match target {
            Some(target) => call(&*target),
            None => self.pending.borrow_mut().push(Box::new(call)),
        }
    }

    /// O serviço real, para as chamadas com retorno.
    fn bound(&self) -> Rc<T> {
        self.target.borrow().clone().expect("o serviço do host ainda não foi ligado ao encaminhador")
    }
}

/// O `ExtHostWorkspaceShape` que ainda não existe quando `MainThreadWorkspace` nasce.
#[derive(Default)]
pub struct LateExtHostWorkspace(pub Late<dyn ExtHostWorkspaceShape>);

impl ExtHostWorkspaceShape for LateExtHostWorkspace {
    fn rpc_initialize_workspace(&self, workspace: Option<IWorkspaceData>, trusted: bool) {
        self.0.call_void(move |target| target.rpc_initialize_workspace(workspace, trusted));
    }

    fn rpc_accept_workspace_data(&self, workspace: Option<IWorkspaceData>) {
        self.0.call_void(move |target| target.rpc_accept_workspace_data(workspace));
    }

    fn rpc_handle_text_search_result(&self, result: IRawFileMatch2, request_id: u64) {
        self.0.call_void(move |target| target.rpc_handle_text_search_result(result, request_id));
    }

    fn rpc_on_did_grant_workspace_trust(&self) {
        self.0.call_void(|target| target.rpc_on_did_grant_workspace_trust());
    }

    fn rpc_on_did_change_workspace_trusted_folders(&self) {
        self.0.call_void(|target| target.rpc_on_did_change_workspace_trusted_folders());
    }

    fn rpc_get_edit_session_identifier(&self, folder: URI, token: CancellationToken) -> Promise<Option<String>> {
        self.0.bound().rpc_get_edit_session_identifier(folder, token)
    }

    fn rpc_provide_edit_session_identity_match(&self, folder: URI, identity1: &str, identity2: &str, token: CancellationToken) -> Promise<Option<EditSessionIdentityMatch>> {
        self.0.bound().rpc_provide_edit_session_identity_match(folder, identity1, identity2, token)
    }

    fn rpc_on_will_create_edit_session_identity(&self, folder: URI, token: CancellationToken, timeout: f64) -> Promise<()> {
        self.0.bound().rpc_on_will_create_edit_session_identity(folder, token, timeout)
    }

    fn rpc_provide_canonical_uri(&self, uri: URI, target_scheme: &str, token: CancellationToken) -> Promise<Option<URI>> {
        self.0.bound().rpc_provide_canonical_uri(uri, target_scheme, token)
    }
}

/// O `ExtHostConfigurationShape` que ainda não existe quando `MainThreadConfiguration` nasce.
#[derive(Default)]
pub struct LateExtHostConfiguration(pub Late<dyn ExtHostConfigurationShape>);

impl ExtHostConfigurationShape for LateExtHostConfiguration {
    fn rpc_initialize_configuration(&self, data: IConfigurationInitData) {
        self.0.call_void(move |target| target.rpc_initialize_configuration(data));
    }

    fn rpc_accept_configuration_changed(&self, data: IConfigurationInitData, change: ConfigurationChange) {
        self.0.call_void(move |target| target.rpc_accept_configuration_changed(data, change));
    }
}

/// O `ExtHostStatusBarShape` que ainda não existe quando `MainThreadStatusBar` nasce.
#[derive(Default)]
pub struct LateExtHostStatusBar(pub Late<dyn ExtHostStatusBarShape>);

impl ExtHostStatusBarShape for LateExtHostStatusBar {
    fn rpc_accept_static_entries(&self, added: Vec<StatusBarItemDto>) {
        self.0.call_void(move |target| target.rpc_accept_static_entries(added));
    }

    fn rpc_provide_tooltip(&self, entry_id: &str, cancellation: CancellationToken) -> Promise<Option<StringOrMarkdown>> {
        self.0.bound().rpc_provide_tooltip(entry_id, cancellation)
    }
}

/// Os encaminhadores `Late*` registrados no contexto em processo, para o host ligar os serviços reais depois.
pub struct LateForwarders {
    pub workspace: Rc<LateExtHostWorkspace>,
    pub configuration: Rc<LateExtHostConfiguration>,
    pub status_bar: Rc<LateExtHostStatusBar>,
}

impl LateForwarders {
    /// Cria os encaminhadores e os registra em `local` (onde os clientes do workbench os encontram com `get_proxy`).
    pub fn register(local: &LocalContext) -> LateForwarders {
        let workspace = Rc::new(LateExtHostWorkspace::default());
        let configuration = Rc::new(LateExtHostConfiguration::default());
        let status_bar = Rc::new(LateExtHostStatusBar::default());
        let as_workspace: Rc<dyn ExtHostWorkspaceShape> = workspace.clone();
        let as_configuration: Rc<dyn ExtHostConfigurationShape> = configuration.clone();
        let as_status_bar: Rc<dyn ExtHostStatusBarShape> = status_bar.clone();
        local.set_erased(ExtHostContext::EXT_HOST_WORKSPACE.sid, Rc::new(as_workspace));
        local.set_erased(ExtHostContext::EXT_HOST_CONFIGURATION.sid, Rc::new(as_configuration));
        local.set_erased(ExtHostContext::EXT_HOST_STATUS_BAR.sid, Rc::new(as_status_bar));
        LateForwarders { workspace, configuration, status_bar }
    }
}

/// O contexto que os serviços do host enxergam: o que o workbench registrou em processo ([`LocalContext`]) vem primeiro, o resto
/// (comandos, extensões, telemetria) é o RPC.
pub struct BridgedContext<'a> {
    rpc: &'a dyn IMainContext,
    local: Rc<LocalContext>,
}

impl<'a> BridgedContext<'a> {
    pub fn new(rpc: &'a dyn IMainContext, local: Rc<LocalContext>) -> Self {
        Self { rpc, local }
    }
}

impl IMainContext for BridgedContext<'_> {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        if self.local.contains(sid) { self.local.get_proxy_erased(sid) } else { self.rpc.get_proxy_erased(sid) }
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        if self.local.contains(sid) { self.local.set_erased(sid, value) } else { self.rpc.set_erased(sid, value) }
    }
}
