//! Porte parcial de `src/vs/workbench/test/browser/workbenchTestServices.ts`: os dublês de teste do navegador que os
//! testes de caminho e de rótulo usam: `TestPathService`, `TestEnvironmentService` e `TestRemoteAgentService`.
//!
//! Notas de tradução:
//! - **`TestEnvironmentService`** é uma constante do original (`new TestEnvironmentServiceWithArgs('', URI.file('tests')
//!   .with({ scheme: 'vscode-tests' }), Object.create(null), TestProductService)`, uma subclasse do
//!   `BrowserWorkbenchEnvironmentService` que só acrescenta `args = []`). O `BrowserWorkbenchEnvironmentService` não
//!   foi portado, então esta struct implementa os contratos [`EnvironmentService`] e [`IWorkbenchEnvironmentService`]
//!   com os valores que a classe do navegador calcula para essas opções: sem `payload`, sem `developmentOptions`, sem
//!   `remoteAuthority` e sem `enableWorkspaceTrust`. `@memoize` não existe aqui, os valores são puros.
//! - **`TestRemoteAgentService`** implementa o [`IRemoteAgentService`] reduzido do porte (`getEnvironment` e
//!   `getRawEnvironment`, que devolvem `null`). Os demais métodos da classe (`getConnection`,
//!   `getExtensionHostExitInfo`, `getDiagnosticInfo`, `updateTelemetryLevel`, `logTelemetry`, `flushTelemetry`,
//!   `getRoundTripTime` e `endConnection`) não fazem parte do contrato reduzido e entram junto com os tipos de que
//!   dependem.
//! - **`TestPathService.defaultUriScheme`** é uma propriedade pública atribuível, aqui o campo com
//!   [`TestPathService::set_default_uri_scheme`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::extpath::is_valid_basename;
use je_base::lifecycle::{self, DisposableRef};
use je_base::network::schemas;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_base::JsString;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode};
use je_platform::product::ProductService;
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;
use je_platform::resources::{ext_uri, host_platform, join_path, uri_file};

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::path::path_service::{IPathService, IResourcePathProvider, PathFlavor};
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use crate::test::common::workbench_test_services::vscode_tests_resource;

/// `TestPathService`.
pub struct TestPathService {
    fallback_user_home: URI,
    default_uri_scheme: RefCell<String>,
}

impl TestPathService {
    /// `constructor(fallbackUserHome = URI.from({ scheme: Schemas.file, path: '/' }), defaultUriScheme = Schemas.file)`.
    pub fn new(fallback_user_home: Option<URI>, default_uri_scheme: Option<&str>) -> Rc<Self> {
        let fallback_user_home = fallback_user_home
            .unwrap_or_else(|| URI::from(&UriComponents { scheme: Some(JsString::from(schemas::FILE)), path: Some(JsString::from("/")), ..UriComponents::default() }, false));
        Rc::new(Self { fallback_user_home, default_uri_scheme: RefCell::new(default_uri_scheme.unwrap_or(schemas::FILE).to_string()) })
    }

    /// `testPathService.defaultUriScheme = scheme`.
    pub fn set_default_uri_scheme(&self, scheme: &str) {
        *self.default_uri_scheme.borrow_mut() = scheme.to_string();
    }

    /// `isValidBasename(name ?? basename(resource))`, com o `isWindowsOS` padrão (o do processo).
    fn valid_basename(resource: &URI, name: Option<&str>) -> bool {
        let basename = ext_uri().basename(resource, None);
        is_valid_basename(Some(name.unwrap_or(&basename)), host_platform() == PathFlavor::Win32)
    }
}

impl IPathService for TestPathService {
    fn path(&self) -> Promise<PathFlavor> {
        Promise::resolved(host_platform())
    }

    fn default_uri_scheme(&self) -> String {
        self.default_uri_scheme.borrow().clone()
    }

    fn file_uri(&self, path: &str) -> Promise<URI> {
        Promise::resolved(uri_file(path))
    }

    fn user_home(&self) -> Promise<URI> {
        Promise::resolved(self.fallback_user_home.clone())
    }

    fn local_user_home(&self) -> URI {
        self.fallback_user_home.clone()
    }

    fn has_valid_basename(&self, resource: &URI, basename: Option<&str>) -> Promise<bool> {
        Promise::resolved(Self::valid_basename(resource, basename))
    }

    fn has_valid_basename_for_os(&self, resource: &URI, _os: OperatingSystem, basename: Option<&str>) -> bool {
        Self::valid_basename(resource, basename)
    }

    fn get_operating_system(&self, _resource: &URI) -> Promise<Option<OperatingSystem>> {
        Promise::resolved(Some(if host_platform() == PathFlavor::Win32 { OperatingSystem::Windows } else { OperatingSystem::Linux }))
    }

    fn get_path(&self, _resource: &URI) -> Promise<Option<PathFlavor>> {
        Promise::resolved(Some(host_platform()))
    }

    fn register_path_provider(&self, _scheme: &str, _provider: Rc<dyn IResourcePathProvider>) -> DisposableRef {
        lifecycle::none()
    }

    fn resolved_user_home(&self) -> Option<URI> {
        Some(self.fallback_user_home.clone())
    }
}

/// `TestRemoteAgentService`.
pub struct TestRemoteAgentService;

impl IRemoteAgentService for TestRemoteAgentService {
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        Promise::resolved(None)
    }

    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        Promise::resolved(None)
    }
}

/// `TestEnvironmentService`.
pub struct TestEnvironmentService {
    workspace_id: String,
    logs_home: URI,
    is_built: bool,
}

impl TestEnvironmentService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { workspace_id: String::new(), logs_home: vscode_tests_resource(), is_built: ProductService::default().commit.is_some_and(|commit| !commit.is_empty()) })
    }

    /// `joinPath(this.userRoamingDataHome, ...fragments)`.
    fn under_user_roaming_data_home(&self, fragments: &[&str]) -> URI {
        join_path(&self.user_roaming_data_home(), fragments)
    }
}

impl EnvironmentService for TestEnvironmentService {
    fn state_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["State", "storage.json"])
    }

    fn user_roaming_data_home(&self) -> URI {
        uri_file("/User").with(&UriChange { scheme: Field::from(schemas::VSCODE_USER_DATA), ..UriChange::default() })
    }

    fn keyboard_layout_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["keyboardLayout.json"])
    }

    fn argv_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["argv.json"])
    }

    fn untitled_workspaces_home(&self) -> URI {
        self.under_user_roaming_data_home(&["Workspaces"])
    }

    fn workspace_storage_home(&self) -> URI {
        self.under_user_roaming_data_home(&["workspaceStorage"])
    }

    fn local_history_home(&self) -> URI {
        self.under_user_roaming_data_home(&["History"])
    }

    fn cache_home(&self) -> URI {
        self.under_user_roaming_data_home(&["caches"])
    }

    fn app_shared_data_home(&self) -> URI {
        self.under_user_roaming_data_home(&["sharedData"])
    }

    /// In Web every workspace can potentially have scoped user-data and/or extensions and if Sync state is shared
    /// then it can make Sync error prone - say removing extensions from another workspace. Hence scope Sync state per
    /// workspace. Sync scoped to a workspace is capable of handling opening same workspace in multiple windows.
    fn user_data_sync_home(&self) -> URI {
        self.under_user_roaming_data_home(&["sync", &self.workspace_id])
    }

    fn sync(&self) -> Option<SyncMode> {
        None
    }

    fn continue_on(&self) -> Option<String> {
        None
    }

    fn edit_session_id(&self) -> Option<String> {
        None
    }

    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        ExtensionHostDebugParams { port: None, break_: false, debug_id: None, env: None }
    }

    fn is_extension_development(&self) -> bool {
        false
    }

    fn disable_extensions(&self) -> DisableExtensions {
        DisableExtensions::All(false)
    }

    fn skip_builtin_extensions(&self) -> Vec<String> {
        Vec::new()
    }

    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        None
    }

    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        None
    }

    fn extension_tests_location_uri(&self) -> Option<URI> {
        None
    }

    fn logs_home(&self) -> URI {
        self.logs_home.clone()
    }

    fn log_level(&self) -> Option<String> {
        None
    }

    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        None
    }

    fn verbose(&self) -> bool {
        false
    }

    fn is_built(&self) -> bool {
        self.is_built
    }

    fn disable_telemetry(&self) -> bool {
        false
    }

    fn disable_experiments(&self) -> bool {
        false
    }

    fn service_machine_id_resource(&self) -> URI {
        self.under_user_roaming_data_home(&["machineid"])
    }

    fn agent_sessions_workspace(&self) -> URI {
        self.under_user_roaming_data_home(&["agent-sessions.code-workspace"])
    }

    fn policy_file(&self) -> Option<URI> {
        None
    }
}

impl IWorkbenchEnvironmentService for TestEnvironmentService {
    fn log_file(&self) -> URI {
        join_path(&self.window_logs_path(), &["window.log"])
    }

    fn window_logs_path(&self) -> URI {
        self.logs_home()
    }

    fn ext_host_logs_path(&self) -> URI {
        join_path(&self.logs_home(), &["exthost"])
    }

    fn extension_enabled_proposed_api(&self) -> Option<Vec<String>> {
        None
    }

    fn remote_authority(&self) -> Option<String> {
        None
    }

    fn skip_release_notes(&self) -> bool {
        false
    }

    fn skip_welcome(&self) -> bool {
        false
    }

    /// `!this.options.enableWorkspaceTrust`.
    fn disable_workspace_trust(&self) -> bool {
        true
    }

    fn is_sessions_window(&self) -> bool {
        false
    }

    fn debug_renderer(&self) -> bool {
        false
    }

    fn log_extension_host_communication(&self) -> Option<bool> {
        Some(false)
    }

    fn enable_smoke_test_driver(&self) -> Option<bool> {
        None
    }

    fn prof_duration_markers(&self) -> Option<Vec<String>> {
        None
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `TestEditorService`, `TestEditorInput` e `TestFileEditorInput`: os dublês que os testes do modelo de editores usam.
//
// - `TestEditorService` é o `IEditorService` reduzido do porte (`visibleEditorPanes`, que o original deixa `[]`).
// - `TestEditorInput` e `TestFileEditorInput` herdam de `EditorInput`: cada um guarda o [`EditorInputBase`], como as
//   demais entradas do porte. O `super.matches(other)` do `TestFileEditorInput` é [`matches_by_default`]. As propriedades
//   atribuíveis do original (`dirty`, `capabilities`, `movedEditor`...) são campos com `Cell`/`RefCell`.
// - `IFileEditorInput` (que o `TestFileEditorInput` implementa) ainda não foi portado: os setters dele
//   (`setPreferredName`, `setEncoding`...) não fazem nada, como no original, e entram como métodos da struct.
// - `registerTestEditor`, `registerTestFileEditor` e os demais `registerTest*` dependem do `Registry` de plataforma e do
//   `EditorPane` (interface), e não são portados aqui.
// ---------------------------------------------------------------------------------------------------------------------

pub use editor_inputs::{TestEditorInput, TestEditorService, TestFileEditorInput, TestForceRevealFileEditorInput};

mod editor_inputs {
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use je_base::errors::Error;
    use je_base::lifecycle::IDisposable;
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_platform::opener::editor_types::EditorOverride;
    use je_platform::resources::ext_uri;

    use crate::common::editor::editor_input::{CanMove, EditorInput, EditorInputBase, matches_by_default};
    use crate::common::editor::editor_model::IEditorModel;
    use crate::common::editor::{
        EditorInputCapabilities, EditorInputOrUntyped, GroupIdentifier, IEditorPane, MoveResult, RevertOptions, SaveOptions, UntypedEditorInput,
    };
    use crate::services::editor::editor_service::{ActiveTextEditorControl, IEditorService};

    /// `TestEditorService`.
    pub struct TestEditorService;

    impl IEditorService for TestEditorService {
        fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
            Vec::new()
        }

        /// `activeEditor` nasce `undefined`.
        fn active_editor(&self) -> Option<EditorInputOrUntyped> {
            None
        }

        /// `activeTextEditorControl` nasce `undefined`.
        fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
            None
        }
    }

    /// `TestEditorInput`.
    pub struct TestEditorInput {
        base: EditorInputBase,
        /// `resource`.
        pub resource: URI,
        type_id: String,
    }

    impl TestEditorInput {
        /// `constructor(resource, _typeId)`.
        pub fn new(resource: URI, type_id: &str) -> Rc<Self> {
            Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource, type_id: type_id.to_string() })
        }
    }

    impl IDisposable for TestEditorInput {
        fn dispose(&self) {
            self.base.dispose();
        }
    }

    impl EditorInput for TestEditorInput {
        fn base(&self) -> &EditorInputBase {
            &self.base
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn editor_type_id(&self) -> String {
            self.type_id.clone()
        }

        fn editor_id(&self) -> Option<String> {
            Some(self.type_id.clone())
        }

        fn resource(&self) -> Option<URI> {
            Some(self.resource.clone())
        }

        fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
            Promise::resolved(None)
        }
    }

    /// `TestFileEditorInput`.
    pub struct TestFileEditorInput {
        base: EditorInputBase,
        /// `resource`.
        pub resource: URI,
        preferred_resource: URI,
        type_id: String,
        force_reveal: bool,

        /// `gotDisposed`.
        pub got_disposed: Cell<bool>,
        /// `gotSaved`.
        pub got_saved: Cell<bool>,
        /// `gotSavedAs`.
        pub got_saved_as: Cell<bool>,
        /// `gotReverted`.
        pub got_reverted: Cell<bool>,
        /// `dirty`.
        pub dirty: Cell<bool>,
        /// `modified`.
        pub modified: Cell<Option<bool>>,
        fails: Cell<bool>,

        /// `disableToUntyped`.
        pub disable_to_untyped: Cell<bool>,

        capabilities: Cell<EditorInputCapabilities>,

        /// `movedEditor`.
        pub moved_editor: RefCell<Option<MoveResult>>,
        move_disabled_reason: RefCell<Option<String>>,
    }

    impl TestFileEditorInput {
        /// `constructor(resource, _typeId)`.
        pub fn new(resource: URI, type_id: &str) -> Rc<Self> {
            Self::create(resource, type_id, false)
        }

        /// `new TestForceRevealFileEditorInput(resource, _typeId)`: `capabilities` é sempre `ForceReveal`.
        pub fn new_force_reveal(resource: URI, type_id: &str) -> Rc<Self> {
            Self::create(resource, type_id, true)
        }

        fn create(resource: URI, type_id: &str, force_reveal: bool) -> Rc<Self> {
            Rc::new_cyclic(|this| Self {
                base: EditorInputBase::new(this.clone()),
                preferred_resource: resource.clone(),
                resource,
                type_id: type_id.to_string(),
                force_reveal,
                got_disposed: Cell::new(false),
                got_saved: Cell::new(false),
                got_saved_as: Cell::new(false),
                got_reverted: Cell::new(false),
                dirty: Cell::new(false),
                modified: Cell::new(None),
                fails: Cell::new(false),
                disable_to_untyped: Cell::new(false),
                capabilities: Cell::new(EditorInputCapabilities::NONE),
                moved_editor: RefCell::new(None),
                move_disabled_reason: RefCell::new(None),
            })
        }

        /// `set capabilities(capabilities)`.
        pub fn set_capabilities(&self, capabilities: EditorInputCapabilities) {
            if self.capabilities.get() != capabilities {
                self.capabilities.set(capabilities);
                self.base.fire_did_change_capabilities();
            }
        }

        pub fn set_preferred_resource(&self, _resource: &URI) {}

        pub fn set_encoding(&self, _encoding: &str) {}

        pub fn get_encoding(&self) -> Option<String> {
            None
        }

        pub fn set_preferred_name(&self, _name: &str) {}

        pub fn set_preferred_description(&self, _description: &str) {}

        pub fn set_preferred_encoding(&self, _encoding: &str) {}

        pub fn set_preferred_contents(&self, _contents: &str) {}

        pub fn set_language_id(&self, _language_id: &str, _source: Option<&str>) {}

        pub fn set_preferred_language_id(&self, _language_id: &str) {}

        pub fn set_force_open_as_binary(&self) {}

        pub fn set_fail_to_open(&self) {
            self.fails.set(true);
        }

        pub fn set_modified(&self) {
            self.modified.set(Some(true));
        }

        pub fn set_dirty(&self) {
            self.dirty.set(true);
        }

        pub fn is_resolved(&self) -> bool {
            false
        }

        pub fn set_move_disabled(&self, reason: &str) {
            *self.move_disabled_reason.borrow_mut() = Some(reason.to_string());
        }
    }

    impl IDisposable for TestFileEditorInput {
        fn dispose(&self) {
            self.base.dispose();
            self.got_disposed.set(true);
        }
    }

    impl EditorInput for TestFileEditorInput {
        fn base(&self) -> &EditorInputBase {
            &self.base
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn editor_type_id(&self) -> String {
            self.type_id.clone()
        }

        fn editor_id(&self) -> Option<String> {
            Some(self.type_id.clone())
        }

        fn resource(&self) -> Option<URI> {
            Some(self.resource.clone())
        }

        fn preferred_resource(&self) -> Option<URI> {
            Some(self.preferred_resource.clone())
        }

        fn capabilities(&self) -> EditorInputCapabilities {
            if self.force_reveal { EditorInputCapabilities::FORCE_REVEAL } else { self.capabilities.get() }
        }

        fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
            if self.fails.get() { Promise::rejected(Error::new("fails")) } else { Promise::resolved(None) }
        }

        fn matches(&self, other: &EditorInputOrUntyped) -> bool {
            if matches_by_default(self, other) {
                return true;
            }

            match other {
                EditorInputOrUntyped::Typed(other) => other.resource().is_some_and(|other_resource| {
                    self.resource.to_string(false).to_string_lossy() == other_resource.to_string(false).to_string_lossy()
                        && other.as_any().downcast_ref::<TestFileEditorInput>().is_some_and(|other| other.type_id == self.type_id)
                }),
                EditorInputOrUntyped::Untyped(untyped) => {
                    let override_ = untyped.options.as_ref().and_then(|options| options.base.override_.as_ref());

                    ext_uri().is_equal(Some(&self.resource), untyped.resource.as_ref(), false)
                        && (override_.is_none() || matches!(override_, Some(EditorOverride::Id(id)) if id.to_string_lossy() == self.type_id))
                }
            }
        }

        fn save(&self, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
            self.got_saved.set(true);
            self.dirty.set(false);
            Promise::resolved(Some(EditorInputOrUntyped::Typed(self.base.this())))
        }

        fn save_as(&self, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
            self.got_saved_as.set(true);
            Promise::resolved(Some(EditorInputOrUntyped::Typed(self.base.this())))
        }

        fn revert(&self, _group: GroupIdentifier, _options: Option<RevertOptions>) -> Promise<()> {
            self.got_reverted.set(true);
            self.got_saved.set(false);
            self.got_saved_as.set(false);
            self.dirty.set(false);
            Promise::resolved(())
        }

        fn to_untyped(&self, _options: Option<&crate::common::editor::editor_input::UntypedEditorOptions>) -> Option<UntypedEditorInput> {
            if self.disable_to_untyped.get() {
                return None;
            }

            Some(UntypedEditorInput::from_resource(self.resource.clone()))
        }

        fn is_modified(&self) -> bool {
            self.modified.get().unwrap_or_else(|| self.dirty.get())
        }

        fn is_dirty(&self) -> bool {
            self.dirty.get()
        }

        fn rename(&self, _group: GroupIdentifier, _target: &URI) -> Promise<Option<MoveResult>> {
            Promise::resolved(self.moved_editor.borrow().clone())
        }

        fn can_move(&self, _source_group: GroupIdentifier, _target_group: GroupIdentifier) -> CanMove {
            match &*self.move_disabled_reason.borrow() {
                Some(reason) => CanMove::No(reason.clone()),
                None => CanMove::Yes,
            }
        }
    }

    /// `TestForceRevealFileEditorInput`: o `TestFileEditorInput` com `capabilities` sempre `ForceReveal`
    /// ([`TestFileEditorInput::new_force_reveal`]).
    pub type TestForceRevealFileEditorInput = TestFileEditorInput;
}

// `TestEditorGroupsService`, reduzido ao contrato `IEditorGroupsService` do porte (só os `groups`). A classe do original
// recebe os `IEditorGroupView[]` e acrescenta dezenas de membros de layout, que entram junto com o contrato.
// ---------------------------------------------------------------------------------------------------------------------

use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};

/// `TestEditorGroupsService`.
pub struct TestEditorGroupsService {
    groups: Vec<Rc<dyn IEditorGroup>>,
}

impl TestEditorGroupsService {
    /// `constructor(public groups: IEditorGroupView[] = [])`.
    pub fn new(groups: Option<Vec<Rc<dyn IEditorGroup>>>) -> Rc<Self> {
        Rc::new(Self { groups: groups.unwrap_or_default() })
    }
}

impl IEditorGroupsService for TestEditorGroupsService {
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        self.groups.clone()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `TestQuickInputService` e `TestExtensionService`: os dublês que o teste do resolvedor de variáveis de configuração usa.
//
// - `TestQuickInputService` implementa o `IQuickInputService` reduzido (`input` e `pick`, os dois métodos que os serviços
//   portados chamam): os demais (`createQuickPick`, `focus`, `navigate`...) lançam `not implemented` no original e entram com
//   o contrato inteiro. `input` devolve `'resolved' + options.prompt` e `pick` devolve o item fixo
//   `{ label: 'selectedPick', description: 'pick description', value: 'selectedPick' }`, que aqui leva o `value` em `extra`.
//   `sinon.stub(quickInputService, 'input').resolves(value)` é [`TestQuickInputService::stub_input_resolves`].
// - `TestExtensionService` é o `IExtensionService` reduzido (`getExtension`), que devolve `undefined` (o `NullExtensionService`).
//   `sinon.stub(service, 'getExtension').withArgs(id).returns(Promise.resolve(description))` é
//   [`TestExtensionService::stub_get_extension`].
// ---------------------------------------------------------------------------------------------------------------------

pub use quick_input_and_extension::{TestExtensionService, TestQuickInputService};

mod quick_input_and_extension {
    use std::cell::RefCell;
    use std::rc::Rc;

    use je_base::JsString;
    use je_base::dynamic::Dynamic;
    use je_base::promise::Promise;
    use je_platform::quickinput::quick_input::{IInputOptions, IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem};

    use crate::services::extensions::extensions::{IExtensionDescription, IExtensionService};

    /// `TestQuickInputService`.
    pub struct TestQuickInputService {
        /// O que `sinon.stub(service, 'input').resolves(...)` fixa: `Some(valor)` troca o retorno de `input`.
        input_stub: RefCell<Option<Option<String>>>,
    }

    impl TestQuickInputService {
        pub fn new() -> Rc<Self> {
            Rc::new(Self { input_stub: RefCell::new(None) })
        }

        /// `sinon.stub(quickInputService, 'input').resolves(value)`.
        pub fn stub_input_resolves(&self, value: Option<String>) {
            *self.input_stub.borrow_mut() = Some(value);
        }
    }

    impl IQuickInputService for TestQuickInputService {
        /// `options ? 'resolved' + options.prompt : 'resolved'`: `prompt` ausente é `undefined`, que o `+` escreve como texto.
        fn input(&self, options: Option<&IInputOptions>) -> Promise<Option<String>> {
            if let Some(stub) = &*self.input_stub.borrow() {
                return Promise::resolved(stub.clone());
            }
            Promise::resolved(Some(match options {
                Some(options) => format!("resolved{}", options.prompt.as_deref().unwrap_or("undefined")),
                None => "resolved".to_string(),
            }))
        }

        /// `Array.isArray(picks)` é sempre verdadeiro aqui: o item fixo.
        fn pick(&self, _picks: Vec<IQuickPickItem>, _options: Option<&IPickOptions>) -> Promise<Option<IQuickPickItem>> {
            Promise::resolved(Some(IQuickPickItem {
                base: IQuickItem { label: JsString::from("selectedPick"), description: Some(JsString::from("pick description")), ..IQuickItem::default() },
                extra: Dynamic::object([("value", Dynamic::string("selectedPick"))]),
            }))
        }
    }

    /// `TestExtensionService`.
    pub struct TestExtensionService {
        extensions: RefCell<Vec<(String, IExtensionDescription)>>,
    }

    impl TestExtensionService {
        pub fn new() -> Rc<Self> {
            Rc::new(Self { extensions: RefCell::new(Vec::new()) })
        }

        /// `sinon.stub(service, 'getExtension').withArgs(id).returns(Promise.resolve(description))`.
        pub fn stub_get_extension(&self, id: &str, description: IExtensionDescription) {
            self.extensions.borrow_mut().push((id.to_string(), description));
        }
    }

    impl IExtensionService for TestExtensionService {
        fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>> {
            let stubbed = self.extensions.borrow().iter().rfind(|(stubbed_id, _)| stubbed_id == id).map(|(_, description)| description.clone());
            Promise::resolved(stubbed)
        }
    }
}
