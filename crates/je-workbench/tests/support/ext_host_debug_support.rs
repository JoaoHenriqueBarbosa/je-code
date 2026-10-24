//! Dublês e auxiliares dos testes do host de extensões de depuração (`extHostDebugAdapters.test.ts`,
//! `extHostDebugTreeSessions.test.ts` e `extHostDebugVisualization.test.ts`).
//!
//! Divergências:
//! - O `new class extends mock<MainThreadDebugServiceShape>() { override $x() {} }` é o [`MockMainThreadDebugService`]
//!   com a lista dos membros que o caso sobrescreve (`allowed`): chamar um membro fora da lista falha com `panic!`, como
//!   o membro ausente do `mock` lançaria `TypeError`.
//! - O `SingleProxyRPCProtocol(proxy)` devolve o mesmo objeto para qualquer identificador; o [`DebugRpcContext`] devolve o
//!   dublê pelo `sid` do `MainThreadDebugService` e um `MainThreadTelemetry` que falha se for usado (o `mock` não tem
//!   `$publicLog2`).
//! - `ExtensionDescriptionRegistry` mockado (`onDidChange = Event.None`, `getAllExtensionDescriptions() { return []; }`) é o
//!   registro real vazio, que se comporta igual (nunca dispara e não tem extensões); ele é entregue ao `store`.
//! - `mock<IExtHostWorkspace>()`, `mock<IExtHostVariableResolverProvider>()`, `mock<IExtHostCommands>()` e
//!   `mock<IExtHostTesting>()` são dublês que falham se forem usados.
//! - `IExtHostConfiguration` e `IExtHostEditorTabs` não são parâmetros do construtor no porte (ver
//!   `ext_host_debug_service.rs`).

use std::any::Any;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::request::request::{AuthInfo, Credentials};
use je_workbench::api::common::ext_host_debug_protocol::{
    IDebugSessionDto, IDebugSessionFullDto, IStartDebuggingOptions, MainThreadDebugServiceShape, NameOrConfig, RegisterBreakpointDto,
};
use je_workbench::api::common::ext_host_debug_service::{
    DefaultExtHostDebugServiceHooks, ExtHostDebugServiceBase, ExtHostDebugServiceDeps, IConfigurationResolver, IExtHostDebugCommands, IExtHostDebugExtensionService,
    IExtHostDebugTesting, IExtHostVariableResolverProvider,
};
use je_workbench::api::common::ext_host_debug_types::{DebugTreeItem, DebugVisualizationContext, DebugVisualizationTree, ProviderResult};
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainThreadTelemetryShape};
use je_workbench::api::common::ext_host_testing::TestRun;
use je_workbench::api::common::ext_host_workspace::{IExtHostWorkspaceProvider, WorkspaceFolder};
use je_workbench::contrib::debug::debug::{DebugConfigurationProviderTriggerKind, IConfig, IDebugVisualizationContext};
use je_workbench::contrib::debug::debug_protocol::{ProtocolMessage, Variable};
use je_workbench::services::extensions::extension_description_registry::{ExtensionDescriptionRegistry, IActivationEventsReader};
use je_workbench::services::extensions::extensions::null_extension_description;
use serde_json::Value;

/// `MainThreadDebugServiceShape` com os membros sobrescritos pelo caso.
pub struct MockMainThreadDebugService {
    allowed: Vec<&'static str>,
}

impl MockMainThreadDebugService {
    pub fn new(allowed: &[&'static str]) -> Rc<MockMainThreadDebugService> {
        Rc::new(MockMainThreadDebugService { allowed: allowed.to_vec() })
    }

    /// O membro que o `mock` não define falha ao ser chamado.
    fn check(&self, member: &str) {
        assert!(self.allowed.contains(&member), "TypeError: proxy.{member} is not a function");
    }
}

impl IDisposable for MockMainThreadDebugService {
    fn dispose(&self) {}
}

impl MainThreadDebugServiceShape for MockMainThreadDebugService {
    fn rpc_register_debug_types(&self, _debug_types: Vec<String>) {
        self.check("$registerDebugTypes");
    }

    fn rpc_session_cached(&self, _session_id: &str) {
        self.check("$sessionCached");
    }

    fn rpc_accept_da_message(&self, _handle: u32, _message: ProtocolMessage) {
        self.check("$acceptDAMessage");
    }

    fn rpc_accept_da_error(&self, _handle: u32, _name: &str, _message: &str, _stack: Option<&str>) {
        self.check("$acceptDAError");
    }

    fn rpc_accept_da_exit(&self, _handle: u32, _code: Option<i32>, _signal: Option<&str>) {
        self.check("$acceptDAExit");
    }

    fn rpc_register_debug_configuration_provider(
        &self,
        _type: &str,
        _trigger_kind: DebugConfigurationProviderTriggerKind,
        _has_provide_method: bool,
        _has_resolve_method: bool,
        _has_resolve2_method: bool,
        _handle: u32,
    ) -> Promise<()> {
        self.check("$registerDebugConfigurationProvider");
        Promise::resolved(())
    }

    fn rpc_register_debug_adapter_descriptor_factory(&self, _type: &str, _handle: u32) -> Promise<()> {
        self.check("$registerDebugAdapterDescriptorFactory");
        Promise::resolved(())
    }

    fn rpc_unregister_debug_configuration_provider(&self, _handle: u32) {
        self.check("$unregisterDebugConfigurationProvider");
    }

    fn rpc_unregister_debug_adapter_descriptor_factory(&self, _handle: u32) {
        self.check("$unregisterDebugAdapterDescriptorFactory");
    }

    fn rpc_start_debugging(&self, _folder: Option<URI>, _name_or_config: NameOrConfig, _options: IStartDebuggingOptions) -> Promise<bool> {
        self.check("$startDebugging");
        Promise::resolved(false)
    }

    fn rpc_stop_debugging(&self, _session_id: Option<&str>) -> Promise<()> {
        self.check("$stopDebugging");
        Promise::resolved(())
    }

    fn rpc_set_debug_session_name(&self, _id: &str, _name: &str) {
        self.check("$setDebugSessionName");
    }

    fn rpc_custom_debug_adapter_request(&self, _id: &str, _command: &str, _args: Option<Value>) -> Promise<Option<Value>> {
        self.check("$customDebugAdapterRequest");
        Promise::resolved(None)
    }

    fn rpc_get_debug_protocol_breakpoint(&self, _id: &str, _breakpoint_id: &str) -> Promise<Option<Value>> {
        self.check("$getDebugProtocolBreakpoint");
        Promise::resolved(None)
    }

    fn rpc_append_debug_console(&self, _value: &str) {
        self.check("$appendDebugConsole");
    }

    fn rpc_register_breakpoints(&self, _breakpoints: Vec<RegisterBreakpointDto>) -> Promise<()> {
        self.check("$registerBreakpoints");
        Promise::resolved(())
    }

    fn rpc_unregister_breakpoints(&self, _breakpoint_ids: Vec<String>, _function_breakpoint_ids: Vec<String>, _data_breakpoint_ids: Vec<String>) -> Promise<()> {
        self.check("$unregisterBreakpoints");
        Promise::resolved(())
    }

    fn rpc_register_debug_visualizer(&self, _extension_id: &str, _id: &str) {
        self.check("$registerDebugVisualizer");
    }

    fn rpc_unregister_debug_visualizer(&self, _extension_id: &str, _id: &str) {
        self.check("$unregisterDebugVisualizer");
    }

    fn rpc_register_debug_visualizer_tree(&self, _tree_id: &str, _can_edit: bool) {
        self.check("$registerDebugVisualizerTree");
    }

    fn rpc_unregister_debug_visualizer_tree(&self, _tree_id: &str) {
        self.check("$unregisterDebugVisualizerTree");
    }
}

/// O `MainThreadTelemetry` do `mock`: nenhum membro existe.
struct UnusedTelemetry;

impl IDisposable for UnusedTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for UnusedTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {
        panic!("TypeError: proxy.$publicLog is not a function");
    }

    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {
        panic!("TypeError: proxy.$publicLog2 is not a function");
    }
}

/// `SingleProxyRPCProtocol(proxy)` do `MainThreadDebugServiceShape` mockado.
struct DebugRpcContext {
    proxy: Rc<dyn MainThreadDebugServiceShape>,
}

impl IMainContext for DebugRpcContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadDebugService" => Rc::new(self.proxy.clone()),
            "MainThreadTelemetry" => {
                let telemetry: Rc<dyn MainThreadTelemetryShape> = Rc::new(UnusedTelemetry);
                Rc::new(telemetry)
            }
            other => panic!("TypeError: o proxy '{other}' não existe no dublê"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `SingleProxyRPCProtocol(proxy)`.
pub fn debug_rpc_context(proxy: Rc<dyn MainThreadDebugServiceShape>) -> Rc<dyn IMainContext> {
    Rc::new(DebugRpcContext { proxy })
}

/// `mock<IExtHostWorkspace>()`.
struct UnusedWorkspace;

impl IExtHostWorkspaceProvider for UnusedWorkspace {
    fn get_workspace_folder2(&self, _uri: &URI, _resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        panic!("TypeError: workspace.getWorkspaceFolder2 is not a function");
    }

    fn resolve_workspace_folder(&self, _uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>> {
        panic!("TypeError: workspace.resolveWorkspaceFolder is not a function");
    }

    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>> {
        panic!("TypeError: workspace.getWorkspaceFolders2 is not a function");
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        panic!("TypeError: workspace.resolveProxy is not a function");
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        panic!("TypeError: workspace.lookupAuthorization is not a function");
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        panic!("TypeError: workspace.lookupKerberosAuthorization is not a function");
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        panic!("TypeError: workspace.loadCertificates is not a function");
    }
}

struct BasicActivationEventsReader;

impl IActivationEventsReader for BasicActivationEventsReader {
    fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        extension_description.manifest.activation_events.clone().unwrap_or_default()
    }
}

/// O `upcastPartial<IExtHostExtensionService>({ getExtensionRegistry })`.
struct EmptyRegistryExtensionService {
    registry: Rc<ExtensionDescriptionRegistry>,
}

impl IExtHostDebugExtensionService for EmptyRegistryExtensionService {
    fn get_extension_registry(&self) -> Promise<Rc<ExtensionDescriptionRegistry>> {
        Promise::resolved(self.registry.clone())
    }
}

/// `mock<IExtHostVariableResolverProvider>()`.
struct UnusedVariableResolverProvider;

impl IExtHostVariableResolverProvider for UnusedVariableResolverProvider {
    fn get_resolver(&self) -> Promise<Rc<dyn IConfigurationResolver>> {
        panic!("TypeError: variableResolver.getResolver is not a function");
    }
}

/// `mock<IExtHostCommands>()`.
struct UnusedCommands;

impl IExtHostDebugCommands for UnusedCommands {
    fn execute_command(&self, _id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        panic!("TypeError: commands.executeCommand is not a function");
    }
}

/// `mock<IExtHostTesting>()`.
struct UnusedTesting;

impl IExtHostDebugTesting for UnusedTesting {
    fn get_metadata_for_run(&self, _run: &Rc<TestRun>) -> Option<(String, String)> {
        panic!("TypeError: testing.getMetadataForRun is not a function");
    }
}

/// O `new class extends ExtHostDebugServiceBase { }(SingleProxyRPCProtocol(proxy), mock, ..., mock)` dos três arquivos. O
/// serviço e o registro de extensões vazio entram no `store` (o `store.add(...)` do original).
pub fn create_debug_service(proxy: Rc<dyn MainThreadDebugServiceShape>, store: &DisposableStore) -> Rc<ExtHostDebugServiceBase> {
    let registry = store.add_rc(ExtensionDescriptionRegistry::new(Rc::new(BasicActivationEventsReader), Vec::new()));
    store.add_rc(ExtHostDebugServiceBase::new(
        ExtHostDebugServiceDeps {
            rpc_service: debug_rpc_context(proxy),
            workspace_service: Rc::new(UnusedWorkspace),
            extension_service: Rc::new(EmptyRegistryExtensionService { registry }),
            variable_resolver: Rc::new(UnusedVariableResolverProvider),
            commands: Rc::new(UnusedCommands),
            testing: Rc::new(UnusedTesting),
        },
        Rc::new(DefaultExtHostDebugServiceHooks),
    ))
}

/// Uma configuração de depuração `{ type, request, name }`.
pub fn debug_config(r#type: &str, request: &str, name: &str) -> IConfig {
    serde_json::json!({ "type": r#type, "request": request, "name": name })
}

/// `{ ...nullExtensionDescription, identifier: new ExtensionIdentifier('sample.visualizers') }`.
pub fn visualizer_extension() -> ExtensionDescription {
    ExtensionDescription { identifier: ExtensionIdentifier::new("sample.visualizers"), ..null_extension_description() }
}

/// `'sample.visualizers\0tree'`.
pub const TREE_ID: &str = "sample.visualizers\0tree";

/// `session(id)`: `{ id, type: 'test', name: id, parent: undefined, folderUri: undefined, configuration: { type: 'test', request: 'launch', name: id } }`.
pub fn session_dto(id: &str) -> IDebugSessionDto {
    IDebugSessionDto::Full(IDebugSessionFullDto {
        id: id.to_owned(),
        r#type: "test".to_owned(),
        name: id.to_owned(),
        parent: None,
        folder_uri: None,
        configuration: debug_config("test", "launch", id),
    })
}

/// `{ sessionId, threadId: 1, variable: { name: 'value', value: '1', variablesReference: 0 } }`.
pub fn visualization_context(session_id: &str) -> IDebugVisualizationContext {
    IDebugVisualizationContext {
        session_id: session_id.to_owned(),
        thread_id: 1,
        variable: Variable { name: "value".to_owned(), value: "1".to_owned(), variables_reference: 0, ..Variable::default() },
        container_id: None,
        frame_id: None,
    }
}

/// `{ label }`.
pub fn tree_item(label: &str) -> Rc<DebugTreeItem> {
    Rc::new(DebugTreeItem { label: label.to_owned(), ..DebugTreeItem::default() })
}

/// `getTreeItem: () => ...`.
pub type GetTreeItem = Box<dyn Fn() -> ProviderResult<Rc<DebugTreeItem>>>;
/// `getChildren: () => ...`.
pub type GetChildren = Box<dyn Fn() -> ProviderResult<Vec<Rc<DebugTreeItem>>>>;
/// `editItem: item => ...`.
pub type EditItem = Box<dyn Fn(Rc<DebugTreeItem>) -> ProviderResult<Rc<DebugTreeItem>>>;

/// O `vscode.DebugVisualizationTree` literal dos casos; `edit_item` ausente é um provedor sem `editItem`.
pub struct TestTree {
    pub get_tree_item: GetTreeItem,
    pub get_children: GetChildren,
    pub edit_item: Option<EditItem>,
}

impl DebugVisualizationTree for TestTree {
    fn get_tree_item(&self, _context: DebugVisualizationContext) -> ProviderResult<Rc<DebugTreeItem>> {
        (self.get_tree_item)()
    }

    fn get_children(&self, _element: Rc<DebugTreeItem>) -> ProviderResult<Vec<Rc<DebugTreeItem>>> {
        (self.get_children)()
    }

    fn has_edit_item(&self) -> bool {
        self.edit_item.is_some()
    }

    fn edit_item(&self, item: Rc<DebugTreeItem>, _value: &str) -> ProviderResult<Rc<DebugTreeItem>> {
        match &self.edit_item {
            Some(edit_item) => edit_item(item),
            None => MaybePromise::Value(None),
        }
    }
}
