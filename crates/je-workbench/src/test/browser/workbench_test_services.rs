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
    /// `policyFile`: no original é uma propriedade atribuível da constante compartilhada (os testes de configuração a
    /// ajustam antes de criar o `FilePolicyService`).
    policy_file: RefCell<Option<URI>>,
}

impl TestEnvironmentService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            workspace_id: String::new(),
            logs_home: vscode_tests_resource(),
            is_built: ProductService::default().commit.is_some_and(|commit| !commit.is_empty()),
            policy_file: RefCell::new(None),
        })
    }

    /// `environmentService.policyFile = policyFile`.
    pub fn set_policy_file(&self, policy_file: Option<URI>) {
        *self.policy_file.borrow_mut() = policy_file;
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
        self.policy_file.borrow().clone()
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
// Dublês de escrita de configuração: o recorte injetável dos serviços de texto que o `ConfigurationEditing` e o
// `JSONEditingService` usam.
//
// O teste de configuração monta, no `instantiationService`, o `TestTextFileService` (herdeiro do `TextFileService`
// completo) e o `TextModelResolverService` (que resolve o modelo de texto de um arquivo pelo `ModelService`), e deixa o
// `WorkspaceService` criar o `ConfigurationEditing` por ele. Nenhum dos dois (nem o `ITextModelService`, nem o
// `ITextFileService`) foi portado; a produção os lê pelo trait `ConfigurationEditingServices`, e este módulo o
// implementa sobre o `FileService` real, observando só o que o teste observa:
// - **Modelo de texto.** [`TestTextModel`] guarda o conteúdo lido do arquivo e aplica as edições em offsets UTF-16, como o
//   `pushEditOperations` do original. As opções de indentação são as do `detectIndentation` do editor (padrão do
//   `ModelService`: `insertSpaces` verdadeiro e `tabSize` 4, trocados pelo que o conteúdo usa quando há linhas
//   indentadas).
// - **Referência.** Cada `createModelReference` lê o arquivo de novo (o modelo do original é descartado quando a
//   última referência é liberada, e as escritas do teste passam pela fila, uma por vez).
// - **Salvar.** `save` grava o conteúdo do modelo com `FileService::write_file` (o `TextFileService.save` do original
//   grava o modelo; a conferência de `etag` fica com o serviço de arquivos).
// - **Notificações.** `notifyError` e `prompt` guardam as mensagens (`notifications`), `open*` guardam o que o usuário
//   abriria (`opened`), e `enableAutoSaveAfterShortDelay` não tem efeito (é só uma dica de interface).
// ---------------------------------------------------------------------------------------------------------------------

pub use configuration_editing_doubles::{TestConfigurationEditingServices, TestTextModel, TestWorkspaceInstantiation};

mod configuration_editing_doubles {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::{Rc, Weak};

    use je_base::async_::async_fn;
    use je_base::buffer::VSBuffer;
    use je_base::json_formatter::Edit;
    use je_base::lifecycle::{self, DisposableRef};
    use je_base::promise::Promise;
    use je_base::uri::URI;
    use je_platform::files::file_service::FileService;
    use je_platform::uri_identity::uri_identity::IUriIdentityService;
    use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;
    use je_platform::workspace::IWorkspaceContextService;

    use crate::services::configuration::browser::configuration_service::{WorkspaceInstantiation, WorkspaceService};
    use crate::services::configuration::common::configuration::IWorkbenchConfigurationService;
    use crate::services::configuration::common::configuration_editing::{
        ConfigurationEditing, ConfigurationEditingModel, ConfigurationEditingServices, PromptAction,
    };
    use crate::services::configuration::common::json_editing::IJsonEditingService;
    use crate::services::configuration::common::json_editing_service::JsonEditingService;
    use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

    /// A chave de um recurso no mapa de modelos vivos.
    fn key_of(resource: &URI) -> String {
        resource.to_string(false).to_string_lossy()
    }

    /// `detectIndentation` do editor, reduzido ao que muda `insertSpaces` e `tabSize`: o conteúdo indentado só com tabs
    /// usa tabs, o indentado com espaços usa o menor recuo como `tabSize`, e sem recuo valem os padrões (espaços, 4).
    fn detect_indentation(content: &str) -> (bool, usize) {
        let (mut tabs, mut spaces, mut smallest_indent) = (0usize, 0usize, usize::MAX);
        for line in content.lines() {
            if line.starts_with('\t') {
                tabs += 1;
            } else if line.starts_with(' ') {
                let indent = line.len() - line.trim_start_matches(' ').len();
                if !line.trim().is_empty() {
                    spaces += 1;
                    smallest_indent = smallest_indent.min(indent);
                }
            }
        }
        if tabs > spaces {
            (false, 4)
        } else if spaces > 0 && smallest_indent != usize::MAX {
            (true, smallest_indent)
        } else {
            (true, 4)
        }
    }

    /// O modelo de texto resolvido de um arquivo (`IResolvedTextEditorModel.textEditorModel`).
    pub struct TestTextModel {
        uri: URI,
        value: RefCell<String>,
        insert_spaces: bool,
        tab_size: usize,
        dirty: Cell<bool>,
        services: Weak<TestConfigurationEditingServices>,
    }

    impl ConfigurationEditingModel for TestTextModel {
        fn uri(&self) -> URI {
            self.uri.clone()
        }

        fn get_value(&self) -> String {
            self.value.borrow().clone()
        }

        fn insert_spaces(&self) -> bool {
            self.insert_spaces
        }

        fn tab_size(&self) -> usize {
            self.tab_size
        }

        fn get_eol(&self) -> String {
            "\n".to_owned()
        }

        fn apply_edit(&self, edit: &Edit) -> bool {
            let units: Vec<u16> = self.value.borrow().encode_utf16().collect();
            let start = edit.offset.min(units.len());
            let end = (edit.offset + edit.length).min(units.len());
            if edit.content.units() == &units[start..end] {
                return false;
            }
            let mut edited: Vec<u16> = units[..start].to_vec();
            edited.extend_from_slice(edit.content.units());
            edited.extend_from_slice(&units[end..]);
            *self.value.borrow_mut() = String::from_utf16_lossy(&edited);
            self.dirty.set(true);
            true
        }

        fn dispose(&self) {
            if let Some(services) = self.services.upgrade() {
                services.models.borrow_mut().remove(&key_of(&self.uri));
            }
        }
    }

    /// O `TestTextFileService` + `TextModelResolverService` + `INotificationService` + `IPreferencesService` +
    /// `IEditorService` + `IFilesConfigurationService` que a escrita de configuração usa (ver a nota do módulo).
    pub struct TestConfigurationEditingServices {
        this: Weak<Self>,
        file_service: Rc<FileService>,
        models: RefCell<HashMap<String, Rc<TestTextModel>>>,
        /// As mensagens de `notificationService.error` e `notificationService.prompt`.
        pub notifications: RefCell<Vec<String>>,
        /// O que o usuário abriria: `openUserSettings`, `openRemoteSettings`, `openWorkspaceSettings`, `openFolderSettings`
        /// (com a pasta) e `openEditor` (com o recurso).
        pub opened: RefCell<Vec<String>>,
    }

    impl TestConfigurationEditingServices {
        pub fn new(file_service: Rc<FileService>) -> Rc<Self> {
            Rc::new_cyclic(|this| Self {
                this: this.clone(),
                file_service,
                models: RefCell::new(HashMap::new()),
                notifications: RefCell::new(Vec::new()),
                opened: RefCell::new(Vec::new()),
            })
        }

        fn rc(&self) -> Rc<Self> {
            self.this.upgrade().expect("o serviço está vivo enquanto é usado")
        }
    }

    impl ConfigurationEditingServices for TestConfigurationEditingServices {
        fn create_model_reference(&self, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>> {
            let (this, resource) = (self.rc(), resource.clone());
            async_fn(async move {
                let content = this.file_service.read_file(&resource, None, None).await?.value.to_string();
                let (insert_spaces, tab_size) = detect_indentation(&content);
                let model = Rc::new(TestTextModel {
                    uri: resource.clone(),
                    value: RefCell::new(content),
                    insert_spaces,
                    tab_size,
                    dirty: Cell::new(false),
                    services: Rc::downgrade(&this),
                });
                this.models.borrow_mut().insert(key_of(&resource), model.clone());
                Ok(model as Rc<dyn ConfigurationEditingModel>)
            })
        }

        fn is_dirty(&self, resource: &URI) -> bool {
            self.models.borrow().get(&key_of(resource)).is_some_and(|model| model.dirty.get())
        }

        fn save(&self, resource: &URI) -> Promise<()> {
            let (this, resource) = (self.rc(), resource.clone());
            async_fn(async move {
                let model = this.models.borrow().get(&key_of(&resource)).cloned();
                if let Some(model) = model {
                    let content = model.get_value();
                    this.file_service.write_file(&resource, VSBuffer::from_string(&content).into(), None).await?;
                    model.dirty.set(false);
                }
                Ok(())
            })
        }

        fn revert(&self, resource: &URI) -> Promise<()> {
            let (this, resource) = (self.rc(), resource.clone());
            async_fn(async move {
                let model = this.models.borrow().get(&key_of(&resource)).cloned();
                if let Some(model) = model {
                    let content = this.file_service.read_file(&resource, None, None).await?.value.to_string();
                    *model.value.borrow_mut() = content;
                    model.dirty.set(false);
                }
                Ok(())
            })
        }

        fn write(&self, resource: &URI, content: &str) -> Promise<()> {
            let (this, resource, content) = (self.rc(), resource.clone(), content.to_owned());
            async_fn(async move {
                this.file_service.write_file(&resource, VSBuffer::from_string(&content).into(), None).await?;
                Ok(())
            })
        }

        fn notify_error(&self, message: &str) {
            self.notifications.borrow_mut().push(message.to_owned());
        }

        fn prompt_error(&self, message: &str, _actions: Vec<PromptAction>) {
            self.notifications.borrow_mut().push(message.to_owned());
        }

        fn open_user_settings(&self) {
            self.opened.borrow_mut().push("user".to_owned());
        }

        fn open_remote_settings(&self) {
            self.opened.borrow_mut().push("remote".to_owned());
        }

        fn open_workspace_settings(&self) {
            self.opened.borrow_mut().push("workspace".to_owned());
        }

        fn open_folder_settings(&self, folder_uri: &URI) {
            self.opened.borrow_mut().push(format!("folder:{}", key_of(folder_uri)));
        }

        fn open_file(&self, resource: &URI) {
            self.opened.borrow_mut().push(format!("file:{}", key_of(resource)));
        }

        fn enable_auto_save_after_short_delay(&self, _resource: &URI) -> DisposableRef {
            lifecycle::none()
        }
    }

    /// O `IInstantiationService` que o teste entrega ao `WorkspaceService.acquireInstantiationService`: cria o
    /// `ConfigurationEditing` e entrega o `IJSONEditingService`, ambos sobre os mesmos [`TestConfigurationEditingServices`].
    pub struct TestWorkspaceInstantiation {
        workspace_service: Weak<WorkspaceService>,
        user_data_profile_service: Rc<dyn IUserDataProfileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        file_service: Rc<FileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        services: Rc<TestConfigurationEditingServices>,
        json_editing_service: Rc<JsonEditingService>,
    }

    impl TestWorkspaceInstantiation {
        /// `testObject.acquireInstantiationService(instantiationService)`: monta os dublês de texto sobre o `file_service`,
        /// entrega-os ao serviço e devolve o [`TestConfigurationEditingServices`] (para o teste ler as notificações).
        pub fn acquire(
            workspace_service: &Rc<WorkspaceService>,
            user_data_profile_service: Rc<dyn IUserDataProfileService>,
            user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
            file_service: Rc<FileService>,
            uri_identity_service: Rc<dyn IUriIdentityService>,
        ) -> Rc<TestConfigurationEditingServices> {
            let services = TestConfigurationEditingServices::new(file_service.clone());
            let json_editing_service = JsonEditingService::new(file_service.clone(), services.clone());
            workspace_service.acquire_instantiation_service(Rc::new(Self {
                workspace_service: Rc::downgrade(workspace_service),
                user_data_profile_service,
                user_data_profiles_service,
                file_service,
                uri_identity_service,
                services: services.clone(),
                json_editing_service,
            }));
            services
        }
    }

    impl WorkspaceInstantiation for TestWorkspaceInstantiation {
        fn create_configuration_editing(&self, remote_settings_resource: Option<URI>) -> Rc<ConfigurationEditing> {
            let workspace_service = self.workspace_service.upgrade().expect("o serviço está vivo enquanto é usado");
            let configuration_service: Rc<dyn IWorkbenchConfigurationService> = workspace_service.clone();
            let context_service: Rc<dyn IWorkspaceContextService> = workspace_service;
            ConfigurationEditing::new(
                remote_settings_resource,
                configuration_service,
                context_service,
                self.user_data_profile_service.clone(),
                self.user_data_profiles_service.clone(),
                self.file_service.clone(),
                self.services.clone(),
                self.uri_identity_service.clone(),
            )
        }

        fn json_editing_service(&self) -> Rc<dyn IJsonEditingService> {
            self.json_editing_service.clone()
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `RemoteFileSystemProvider` e o `IRemoteAgentService` que os testes de configuração remota controlam.
//
// - **`RemoteFileSystemProvider`** (`workbenchTestServices.ts`) embrulha o provider em memória e troca o esquema e a
//   autoridade das URIs: `vscode-remote://<autoridade>/...` vira `file:///...` na ida, e as mudanças voltam como
//   `vscode-remote`. O trait [`FileSystemProvider`] do porte é um só (sem as interfaces de capacidade do original), então
//   os métodos que o original chama com `!` (`copy`, `readFile`, `writeFile`, `open`, `close`, `read`, `write`,
//   `readFileStream`) são repassados direto e o embrulhado decide se os suporta.
// - **`getEnvironment: () => remoteEnvironmentPromise`.** O teste entrega ao serviço um `IRemoteAgentService` parcial cujo
//   `getEnvironment` devolve uma promessa que ele resolve depois com `{ settingsPath }`. O [`TestDeferredRemoteAgentService`]
//   é essa promessa. O `Partial<IRemoteAgentEnvironment>` do original só tem `settingsPath`: os outros campos do
//   [`RemoteAgentEnvironment`] completo (que ninguém lê aqui) valem o `settingsPath` (os URIs) e valores neutros.
// ---------------------------------------------------------------------------------------------------------------------

pub use remote_doubles::{TestDeferredRemoteAgentService, TestRemoteFileSystemProvider};

mod remote_doubles {
    use std::rc::Rc;

    use je_base::JsString;
    use je_base::buffer::{VSBuffer, VSBufferReadableStream};
    use je_base::cancellation::CancellationToken;
    use je_base::event::Event;
    use je_base::lifecycle::DisposableRef;
    use je_base::network::schemas;
    use je_base::platform::OperatingSystem;
    use je_base::promise::{DeferredPromise, Promise};
    use je_base::uri::{Field, URI, UriChange};
    use je_platform::files::files::{
        FileChange, FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FileReadStreamOptions, FileSystemProvider, FileSystemProviderCapabilities,
        FileSystemProviderRef, FileType, FileWriteOptions, Stat, WatchOptions,
    };
    use je_platform::remote::remote_agent_environment::{RemoteAgentEnvironment, RemoteAgentProfiles};

    use crate::services::remote::remote_agent_service::IRemoteAgentService;

    /// `RemoteFileSystemProvider`.
    pub struct TestRemoteFileSystemProvider {
        wrapped_fsp: FileSystemProviderRef,
        on_did_change_file: Event<Vec<FileChange>>,
    }

    impl TestRemoteFileSystemProvider {
        /// `constructor(wrappedFsp, remoteAuthority)`.
        pub fn new(wrapped_fsp: FileSystemProviderRef, remote_authority: &str) -> Rc<Self> {
            let remote_authority = remote_authority.to_owned();
            let on_did_change_file = wrapped_fsp.on_did_change_file().map(
                move |changes| {
                    changes
                        .iter()
                        .map(|change| FileChange {
                            change_type: change.change_type,
                            resource: change.resource.with(&UriChange {
                                scheme: Field::Set(JsString::from(schemas::VSCODE_REMOTE)),
                                authority: Field::Set(JsString::from(remote_authority.as_str())),
                                ..UriChange::default()
                            }),
                            c_id: None,
                        })
                        .collect()
                },
                None,
            );
            Rc::new(Self { wrapped_fsp, on_did_change_file })
        }

        /// `resource.with({ scheme: Schemas.file, authority: '' })`.
        fn to_file_resource(resource: &URI) -> URI {
            resource.with(&UriChange { scheme: Field::Set(JsString::from(schemas::FILE)), authority: Field::Set(JsString::from("")), ..UriChange::default() })
        }
    }

    impl FileSystemProvider for TestRemoteFileSystemProvider {
        fn capabilities(&self) -> FileSystemProviderCapabilities {
            self.wrapped_fsp.capabilities()
        }

        fn on_did_change_capabilities(&self) -> Event<()> {
            self.wrapped_fsp.on_did_change_capabilities()
        }

        fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
            self.on_did_change_file.clone()
        }

        fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
            self.wrapped_fsp.watch(&Self::to_file_resource(resource), opts)
        }

        fn stat(&self, resource: &URI) -> Promise<Stat> {
            self.wrapped_fsp.stat(&Self::to_file_resource(resource))
        }

        fn mkdir(&self, resource: &URI) -> Promise<()> {
            self.wrapped_fsp.mkdir(&Self::to_file_resource(resource))
        }

        fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
            self.wrapped_fsp.readdir(&Self::to_file_resource(resource))
        }

        fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()> {
            self.wrapped_fsp.delete(&Self::to_file_resource(resource), opts)
        }

        fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
            self.wrapped_fsp.rename(&Self::to_file_resource(from), &Self::to_file_resource(to), opts)
        }

        fn copy(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
            self.wrapped_fsp.copy(&Self::to_file_resource(from), &Self::to_file_resource(to), opts)
        }

        fn read_file(&self, resource: &URI, atomic: bool) -> Promise<VSBuffer> {
            self.wrapped_fsp.read_file(&Self::to_file_resource(resource), atomic)
        }

        fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
            self.wrapped_fsp.write_file(&Self::to_file_resource(resource), content, opts)
        }

        fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
            self.wrapped_fsp.open(&Self::to_file_resource(resource), opts)
        }

        fn close(&self, fd: i64) -> Promise<()> {
            self.wrapped_fsp.close(fd)
        }

        fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
            self.wrapped_fsp.read(fd, pos, data, offset, length)
        }

        fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
            self.wrapped_fsp.write(fd, pos, data, offset, length)
        }

        fn read_file_stream(&self, resource: &URI, opts: &FileReadStreamOptions, token: &CancellationToken) -> VSBufferReadableStream {
            self.wrapped_fsp.read_file_stream(&Self::to_file_resource(resource), opts, token)
        }
    }

    /// O `IRemoteAgentService` parcial `{ getEnvironment: () => remoteEnvironmentPromise }`.
    pub struct TestDeferredRemoteAgentService {
        environment: DeferredPromise<Option<RemoteAgentEnvironment>>,
    }

    impl TestDeferredRemoteAgentService {
        pub fn new() -> Rc<Self> {
            Rc::new(Self { environment: DeferredPromise::new() })
        }

        /// `resolveRemoteEnvironment()`: resolve a promessa com `{ settingsPath }`.
        pub fn resolve(&self, settings_path: &URI) {
            let _ = self.environment.complete(Some(RemoteAgentEnvironment {
                pid: 0,
                connection_token: String::new(),
                app_root: settings_path.clone(),
                exec_path: String::new(),
                tmp_dir: settings_path.clone(),
                settings_path: settings_path.clone(),
                mcp_resource: settings_path.clone(),
                logs_path: settings_path.clone(),
                extension_host_logs_path: settings_path.clone(),
                global_storage_home: settings_path.clone(),
                workspace_storage_home: settings_path.clone(),
                local_history_home: settings_path.clone(),
                user_home: settings_path.clone(),
                os: OperatingSystem::Linux,
                arch: String::new(),
                marks: Vec::new(),
                use_host_proxy: false,
                profiles: RemoteAgentProfiles { all: Vec::new(), home: settings_path.clone() },
                is_unsupported_glibc: false,
                reconnection_grace_time: None,
            }));
        }
    }

    impl IRemoteAgentService for TestDeferredRemoteAgentService {
        fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
            self.environment.promise.clone()
        }

        fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
            self.environment.promise.clone()
        }
    }
}

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

// ---------------------------------------------------------------------------------------------------------------------
// `MockLabelService` (`src/vs/workbench/services/label/test/common/mockLabelService.ts`): o `ILabelService` dos testes
// das migalhas e de tudo que lê o `home` de um formatador de rótulo.
//
// - **`uriExt`** (`IExtUri = extUri`) é o `ExtUri` do construtor, e sem argumento o `ExtUri` sem viés.
// - **Formatadores por identidade.** `this.formatters.filter(candidate => candidate !== formatter)` compara os `Rc` do
//   [`LabelFormatter`] (dois formatadores iguais registrados em separado continuam distintos).
// - **`registerFormatter`** dispara `onDidChangeFormatters` ao registrar e ao descartar, e um formatador de modelo
//   (`ResourceLabelTemplateFormatter`) também dispara quando o seu `onDidChangeFormatting` dispara.
// - **`getUriLabel`/`getUriBasenameLabel`** usam o `fsPath` e o módulo `path` nativos do processo (`normalize` e `basename`).
// ---------------------------------------------------------------------------------------------------------------------

pub use mock_label_service::MockLabelService;

mod mock_label_service {
    use std::cell::RefCell;
    use std::rc::Rc;

    use je_base::event::{Emitter, Event};
    use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
    use je_base::path::PathModule;
    use je_base::resources::ExtUri;
    use je_base::uri::{Field, URI, UriChange};
    use je_base::JsString;
    use je_platform::label::label::{
        FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, ResourceLabelFormatting, ResourceLabelFormattingContext,
        UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
    };
    use je_platform::resources::{ext_uri, fs_path, host_environment};

    use crate::services::label::resource_label_template::{ResourceLabelTemplate, ResourceLabelTemplateMatch, with_path_only};

    /// O `{ home, formatting }` que `findHomeFormatter` devolve.
    struct HomeMatch {
        home: URI,
        formatting: ResourceLabelFormatting,
    }

    /// `candidate !== formatter`, por identidade do `Rc`.
    fn is_same_formatter(a: &LabelFormatter, b: &LabelFormatter) -> bool {
        match (a, b) {
            (LabelFormatter::Resource(a), LabelFormatter::Resource(b)) => Rc::ptr_eq(a, b),
            (LabelFormatter::Template(a), LabelFormatter::Template(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `isTemplateFormatter(formatter) ? formatter.home.scheme : formatter.scheme`.
    fn formatter_scheme(formatter: &LabelFormatter) -> String {
        match formatter {
            LabelFormatter::Resource(formatter) => formatter.scheme.clone(),
            LabelFormatter::Template(formatter) => formatter.home.scheme().to_string_lossy(),
        }
    }

    /// `MockLabelService`.
    pub struct MockLabelService {
        uri_ext: ExtUri,
        formatters: Rc<RefCell<Vec<LabelFormatter>>>,
        on_did_change_formatters: Rc<Emitter<FormatterChangeEvent>>,
    }

    impl MockLabelService {
        /// `constructor(uriExt: IExtUri = extUri)`.
        pub fn new(uri_ext: Option<ExtUri>) -> Rc<Self> {
            Rc::new(Self { uri_ext: uri_ext.unwrap_or_else(ext_uri), formatters: Rc::new(RefCell::new(Vec::new())), on_did_change_formatters: Emitter::new() })
        }

        /// `get formatterCount()`.
        pub fn formatter_count(&self) -> usize {
            self.formatters.borrow().len()
        }

        /// `findHomeFormatter(resource)`: o formatador cujo `home` contém o recurso, com o `home` mais longo.
        fn find_home_formatter(&self, resource: &URI) -> Option<HomeMatch> {
            let mut result: Option<HomeMatch> = None;
            for formatter in self.formatters.borrow().iter() {
                let candidate = match formatter {
                    LabelFormatter::Template(template) => {
                        let Some(template_match) = ResourceLabelTemplate::new(&template.home).match_resource(resource, &self.uri_ext) else {
                            continue;
                        };
                        let ResourceLabelTemplateMatch { home, parameters } = template_match;
                        let context = ResourceLabelFormattingContext { resource: resource.clone(), home: home.clone(), parameters };
                        (template.formatting)(&context).map(|formatting| HomeMatch { home, formatting })
                    }
                    LabelFormatter::Resource(formatter) => {
                        let Some(home) = formatter.home.as_ref().filter(|home| !home.is_empty()) else {
                            continue;
                        };
                        let same_authority = formatter.authority.as_ref().is_none_or(|authority| authority.is_empty() || *authority == resource.authority().to_string_lossy());
                        let home_resource = resource.with(&UriChange { path: Field::Set(JsString::from(home.as_str())), ..UriChange::default() });
                        (formatter.scheme == resource.scheme().to_string_lossy() && same_authority && self.uri_ext.is_equal_or_parent(resource, &home_resource, false, false))
                            .then(|| HomeMatch { home: with_path_only(resource, home.as_str()), formatting: formatter.formatting.clone() })
                    }
                };
                if let Some(candidate) = candidate {
                    let longer = result.as_ref().is_none_or(|current| candidate.home.path().units().len() > current.home.path().units().len());
                    if longer {
                        result = Some(candidate);
                    }
                }
            }
            result
        }
    }

    impl ILabelService for MockLabelService {
        fn get_uri_label(&self, resource: &URI, _options: &UriLabelOptions) -> String {
            if let Some(found) = self.find_home_formatter(resource) {
                let path = resource.path().units();
                let relative_path = String::from_utf16_lossy(&path[found.home.path().units().len().min(path.len())..]);
                let relative_path = relative_path.strip_prefix('/').unwrap_or(&relative_path);
                return if relative_path.is_empty() { found.formatting.label } else { format!("{}/{}", found.formatting.label, relative_path) };
            }
            let env = host_environment();
            PathModule::native(env.as_ref()).normalize(&fs_path(resource))
        }

        fn get_uri_basename_label(&self, resource: &URI) -> String {
            let env = host_environment();
            PathModule::native(env.as_ref()).basename(&fs_path(resource), None)
        }

        fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
            String::new()
        }

        fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
            String::new()
        }

        fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
            Some(String::new())
        }

        fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
            LabelSeparator::Slash
        }

        fn get_uri_home(&self, resource: &URI) -> Option<URI> {
            self.find_home_formatter(resource).map(|found| found.home)
        }

        fn register_formatter(&self, formatter: LabelFormatter) -> DisposableRef {
            self.formatters.borrow_mut().push(formatter.clone());
            let scheme = formatter_scheme(&formatter);
            self.on_did_change_formatters.fire(FormatterChangeEvent { scheme: scheme.clone() });
            let change_listener = match &formatter {
                LabelFormatter::Template(template) => {
                    let (emitter, scheme) = (self.on_did_change_formatters.clone(), scheme.clone());
                    Some(template.on_did_change_formatting.listen(move |_| emitter.fire(FormatterChangeEvent { scheme: scheme.clone() })))
                }
                LabelFormatter::Resource(_) => None,
            };

            let (formatters, emitter) = (self.formatters.clone(), self.on_did_change_formatters.clone());
            to_disposable(move || {
                if let Some(change_listener) = change_listener {
                    change_listener.dispose();
                }
                formatters.borrow_mut().retain(|candidate| !is_same_formatter(candidate, &formatter));
                emitter.fire(FormatterChangeEvent { scheme });
            })
        }

        fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
            self.on_did_change_formatters.event()
        }

        fn register_cached_formatter(&self, formatter: ResourceLabelFormatter) -> DisposableRef {
            self.register_formatter(LabelFormatter::from(formatter))
        }
    }
}
