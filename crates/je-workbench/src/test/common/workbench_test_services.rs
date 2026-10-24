//! Porte de `src/vs/workbench/test/common/workbenchTestServices.ts`: os serviços de teste (dublês) que os testes de
//! `vs/workbench` e de todas as crates acima dele usam.
//!
//! Infraestrutura de teste compartilhada por dezenas de testes é produção de teste: fica aqui, num lugar só, e não
//! num `tests/support` repetido em cada crate.
//!
//! Notas de tradução:
//! - **Classes que estendem uma base** (`TestLoggerService`, `TestStorageService`, `TestProductService`,
//!   `TestWorkspaceTrustManagementService`...) não têm herança em Rust: quando a subclasse só dá padrões ao construtor
//!   ou só acrescenta um método que a base já expõe, é um alias ou uma struct de construtor sobre o tipo de produção.
//!   `TestStorageService` é o [`InMemoryStorageService`] (o `testEmitWillSaveState` é o
//!   `AbstractStorageService::emit_will_save_state`, já público) e `TestProductService`
//!   (`{ _serviceBrand: undefined, ...product }`) é o [`ProductService`] padrão do porte, que nos testes não traz
//!   `extensionKind`, `extensionPointExtensionKind`, `extensionUntrustedWorkspaceSupport` nem
//!   `extensionVirtualWorkspacesSupport`; quem precisa de `trustedExtensionAuthAccess` usa
//!   `ProductService::set_trusted_extension_auth_access`.
//! - **Parâmetros opcionais** do construtor (`constructor(isDirty = false)`) são `Option`, com o padrão do original.
//! - **Métodos "Method not implemented."**: nos métodos síncronos o `throw` do original é [`errors::throw`] (um pânico
//!   com o `Error`); nos `async` o `throw` vira uma promessa rejeitada, como num `async function`.
//! - **Propriedades** viram métodos (ver cada contrato), e os eventos são `Rc<Emitter<...>>` registrados no
//!   `Disposable` base quando a classe original estende `Disposable`.
//! - `mock<T>()` e `Ctor<T>` (uma classe que serve de base para um dublê parcial com os métodos sobrescritos) não têm
//!   equivalente em Rust: um dublê parcial é uma struct que implementa o trait inteiro, como as deste módulo.
//! - `createFileStat` recebe as opções posicionais do original na struct [`FileStatOptions`].
//! - `TestLifecycleService`: o `_phase` e o `startupKind` do original nascem `undefined`; aqui nascem em
//!   `Starting` e `NewWindow`, porque o contrato [`ILifecycleService`] devolve o valor, não um `Option`.
//!
//! Fora deste módulo (e por quê): `TestHistoryService` precisa do `EditorInput` (`IHistoryService.getHistory`),
//! `TestActivityService` do `ViewContainer` (`onDidChangeActivity: Event<string | ViewContainer>`),
//! `TestWorkingCopyFileService` do contrato `IWorkingCopyFileService`, `InMemoryTestFileService` do contrato
//! `IFileService` completo (o `TestFileService` deste módulo implementa o `IFileService` reduzido, porque o porte do
//! `FileService` é uma struct concreta, sem trait), `TestExtensionService` do `IExtensionService` e
//! `TestChatEntitlementService` do `IChatEntitlementService` (o chat está fora do escopo do porte). Cada um entra aqui
//! quando o contrato de que depende for portado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::map::{ResourceMap, ResourceSet};
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::{DeferredPromise, Promise};
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_editor::position::Position;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationChangeEvent, ITextResourceConfigurationService, ITextResourcePropertiesService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationValue};
use je_platform::files::files::{
    FileChangesEvent, FileStat, FileStatWithMetadata, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef,
    FileSystemProviderRegistrationEvent, ResolveFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{AbstractLoggerService, LogLevel, LoggerRef, NullLogger};
use je_platform::markers::{IMarkerService, Marker, MarkerChangeOptions, MarkerData, MarkerReadOptions, MarkerStatistics, ResourceMarker};
use je_platform::product::ProductService;
use je_platform::resources::{host_environment, uri_file};
use je_platform::storage::InMemoryStorageService;
use je_platform::user_data_profile::{UserDataProfile, to_user_data_profile};
use je_platform::workspace::{
    CurrentWorkspaceQuery, IWorkspaceContextService, WorkbenchState, Workspace, WorkspaceFolder, WorkspaceFoldersChangeEvent,
    WorkspaceFoldersWillChangeEvent, default_test_workspace,
};
use je_platform::workspace_trust::{
    IWorkspaceTrustEnablementService, IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, IWorkspaceTrustTransitionParticipant,
    ResourceTrustRequestOptions, WorkspaceTrustRequestOptions, WorkspaceTrustUriInfo, WorkspaceTrustUriResponse,
};

use crate::common::editor::{RevertOptions, SaveOptions, SaveReason};
use crate::services::files_configuration::files_configuration_service::{
    IAutoSaveConfiguration, IAutoSaveMode, IFilesConfigurationService, Readonly, ReadonlyUpdate, ResourceOrEditor, SingleReadonlyUpdate,
};
use crate::services::lifecycle::lifecycle::{
    BeforeShutdownErrorEvent, BeforeShutdownEvent, ILifecycleService, InternalBeforeShutdownEvent, LifecyclePhase, ShutdownReason, StartupKind,
    WillShutdownEvent, WillShutdownEventJoiner, WillShutdownJoin,
};
use crate::services::user_data_profile::user_data_profile::{DidChangeUserDataProfileEvent, IUserDataProfileService};
use crate::services::working_copy::stored_file_working_copy::StoredFileWorkingCopySaveEvent;
use crate::services::working_copy::working_copy::{
    IWorkingCopy, WorkingCopyBackup, WorkingCopyCapabilities, WorkingCopySaveEvent,
};

/// O `ExtUri` sem viés que o `resources.ts` exporta como `basename`, `isEqual`, `isEqualOrParent` e `isEqualAuthority`.
fn unbiased_ext_uri() -> ExtUri {
    ExtUri::unbiased(host_environment())
}

/// `URI.file('tests').with({ scheme: 'vscode-tests' })`.
pub(crate) fn vscode_tests_resource() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::from("vscode-tests"), ..Default::default() })
}

/// O `throw new Error('Method not implemented.')` dos métodos síncronos.
fn not_implemented() -> ! {
    errors::throw(Error::new("Method not implemented."))
}

/// O `throw new Error('Method not implemented.')` de um método `async`: uma promessa rejeitada.
fn not_implemented_async<T: Clone + 'static>() -> Promise<T> {
    Promise::rejected(Error::new("Method not implemented."))
}

/// `TestLoggerService`: um `AbstractLoggerService` cujos loggers descartam tudo.
pub struct TestLoggerService;

impl TestLoggerService {
    /// `constructor(logsHome?: URI)`.
    #[allow(clippy::new_ret_no_self)] // o construtor devolve o núcleo compartilhado, como `InMemoryStorageService::new`
    pub fn new(logs_home: Option<URI>) -> Rc<AbstractLoggerService> {
        AbstractLoggerService::new(
            LogLevel::Info,
            logs_home.unwrap_or_else(vscode_tests_resource),
            Vec::new(),
            Rc::new(|_resource, _level, _options| -> LoggerRef { Rc::new(NullLogger::new()) }),
        )
    }
}

/// `TestTextResourcePropertiesService`.
pub struct TestTextResourcePropertiesService {
    configuration_service: Rc<dyn IConfigurationService>,
}

impl TestTextResourcePropertiesService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new(Self { configuration_service })
    }
}

impl ITextResourcePropertiesService for TestTextResourcePropertiesService {
    fn get_eol(&self, resource: &URI, language: Option<&str>) -> String {
        let overrides = ConfigurationOverrides { resource: Some(resource.clone()), override_identifier: language.map(JsString::from) };
        if let Dynamic::String(eol) = self.configuration_service.get_value(Some("files.eol"), Some(&overrides)) {
            let eol = eol.to_string_lossy();
            if !eol.is_empty() && eol != "auto" {
                return eol;
            }
        }
        let platform = Platform::detect();
        let eol = if platform.is_linux || platform.is_macintosh { "\n" } else { "\r\n" };
        eol.to_string()
    }
}

/// `TestUserDataProfileService`.
pub struct TestUserDataProfileService {
    current_profile: UserDataProfile,
}

impl TestUserDataProfileService {
    pub fn new() -> Rc<Self> {
        let location = vscode_tests_resource();
        Rc::new(Self { current_profile: to_user_data_profile("test", "test", &location, &location, None, None, &unbiased_ext_uri()) })
    }
}

impl IUserDataProfileService for TestUserDataProfileService {
    fn current_profile(&self) -> UserDataProfile {
        self.current_profile.clone()
    }

    fn on_did_change_current_profile(&self) -> Event<DidChangeUserDataProfileEvent> {
        Event::none()
    }

    fn update_current_profile(&self, _current_profile: &UserDataProfile) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestContextService`.
pub struct TestContextService {
    workspace: RefCell<Rc<Workspace>>,
    options: Dynamic,
    on_did_change_workspace_name: Rc<Emitter<()>>,
    on_will_change_workspace_folders: Rc<Emitter<WorkspaceFoldersWillChangeEvent>>,
    on_did_change_workspace_folders: Rc<Emitter<WorkspaceFoldersChangeEvent>>,
    on_did_change_workbench_state: Rc<Emitter<WorkbenchState>>,
    /// O `instantiationService.stub(IWorkspaceContextService, 'getWorkbenchState', state)` dos testes: troca o resultado de
    /// `get_workbench_state` no serviço que já foi entregue a quem o usa.
    workbench_state_override: Cell<Option<WorkbenchState>>,
}

impl TestContextService {
    /// `constructor(workspace = TestWorkspace, options = null)`.
    pub fn new(workspace: Option<Rc<Workspace>>, options: Option<Dynamic>) -> Rc<Self> {
        Rc::new(Self {
            workspace: RefCell::new(workspace.unwrap_or_else(default_test_workspace)),
            options: options.unwrap_or_else(|| Dynamic::Object(ObjectRef::new())),
            on_did_change_workspace_name: Emitter::new(),
            on_will_change_workspace_folders: Emitter::new(),
            on_did_change_workspace_folders: Emitter::new(),
            on_did_change_workbench_state: Emitter::new(),
            workbench_state_override: Cell::new(None),
        })
    }

    /// `stub(IWorkspaceContextService, 'getWorkbenchState', state)`; `None` desfaz o stub.
    pub fn stub_workbench_state(&self, state: Option<WorkbenchState>) {
        self.workbench_state_override.set(state);
    }

    pub fn get_folders(&self) -> Vec<WorkspaceFolder> {
        self.workspace.borrow().folders()
    }

    pub fn set_workspace(&self, workspace: Rc<Workspace>) {
        *self.workspace.borrow_mut() = workspace;
    }

    /// `Reflect.get(service, '_onDidChangeWorkspaceFolders') as Emitter<IWorkspaceFoldersChangeEvent>`: o emissor privado
    /// que os testes disparam depois de `setWorkspace` para simular a mudança de pastas.
    pub fn on_did_change_workspace_folders_emitter(&self) -> Rc<Emitter<WorkspaceFoldersChangeEvent>> {
        self.on_did_change_workspace_folders.clone()
    }

    pub fn get_options(&self) -> Dynamic {
        self.options.clone()
    }

    pub fn update_options(&self) {}

    /// `toResource(workspaceRelativePath)`: `URI.file(join('C:\\', workspaceRelativePath))`.
    pub fn to_resource(&self, workspace_relative_path: &str) -> URI {
        let env = host_environment();
        uri_file(&PathModule::native(env.as_ref()).join(&["C:\\", workspace_relative_path]))
    }

    /// `this.workspace.folders[0].uri`: com o workspace sem pastas o original lança um `TypeError`, e aqui o índice entra em pânico.
    fn first_folder_uri(&self) -> URI {
        self.workspace.borrow().folders()[0].uri.clone()
    }
}

impl IWorkspaceContextService for TestContextService {
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        self.on_did_change_workbench_state.event()
    }

    fn on_did_change_workspace_name(&self) -> Event<()> {
        self.on_did_change_workspace_name.event()
    }

    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        self.on_will_change_workspace_folders.event()
    }

    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        self.on_did_change_workspace_folders.event()
    }

    fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        Promise::resolved(self.get_workspace())
    }

    fn get_workspace(&self) -> Rc<Workspace> {
        self.workspace.borrow().clone()
    }

    fn get_workbench_state(&self) -> WorkbenchState {
        if let Some(state) = self.workbench_state_override.get() {
            return state;
        }
        let workspace = self.workspace.borrow();
        if workspace.configuration().is_some() {
            return WorkbenchState::Workspace;
        }
        if !workspace.folders().is_empty() {
            return WorkbenchState::Folder;
        }
        WorkbenchState::Empty
    }

    fn get_workspace_folder(&self, resource: &URI) -> Option<WorkspaceFolder> {
        self.workspace.borrow().get_folder(resource)
    }

    fn is_current_workspace(&self, workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        match workspace_id_or_folder {
            CurrentWorkspaceQuery::Uri(uri) => unbiased_ext_uri().is_equal(Some(&self.first_folder_uri()), Some(uri), false),
            CurrentWorkspaceQuery::Workspace(_) | CurrentWorkspaceQuery::SingleFolder(_) => false,
        }
    }

    fn is_inside_workspace(&self, resource: &URI) -> bool {
        unbiased_ext_uri().is_equal_or_parent(resource, &self.first_folder_uri(), false, false)
    }

    fn has_workspace_data(&self) -> bool {
        self.get_workbench_state() != WorkbenchState::Empty
    }
}

/// `TestStorageService`: o `InMemoryStorageService` com o `testEmitWillSaveState`, que é o
/// `AbstractStorageService::emit_will_save_state` (já público). `TestStorageService::new()` devolve o serviço.
pub type TestStorageService = InMemoryStorageService;

/// `TestWorkingCopy`.
pub struct TestWorkingCopy {
    base: Rc<Disposable>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_change_content: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<StoredFileWorkingCopySaveEvent>>,
    name: String,
    resource: URI,
    type_id: String,
    dirty: Cell<bool>,
}

impl TestWorkingCopy {
    /// `constructor(resource, isDirty = false, typeId = 'testWorkingCopyType')`.
    pub fn new(resource: URI, is_dirty: Option<bool>, type_id: Option<&str>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_content = Emitter::new();
        let on_did_save = Emitter::new();
        base.register(on_did_change_dirty.clone());
        base.register(on_did_change_content.clone());
        base.register(on_did_save.clone());
        Rc::new(Self {
            base,
            on_did_change_dirty,
            on_did_change_content,
            on_did_save,
            name: unbiased_ext_uri().basename(&resource, None),
            resource,
            type_id: type_id.unwrap_or("testWorkingCopyType").to_string(),
            dirty: Cell::new(is_dirty.unwrap_or(false)),
        })
    }

    pub fn set_dirty(&self, dirty: bool) {
        if self.dirty.get() != dirty {
            self.dirty.set(dirty);
            self.on_did_change_dirty.fire(());
        }
    }

    pub fn set_content(&self, _content: &str) {
        self.on_did_change_content.fire(());
    }

    /// O `onDidSave` com o tipo que a classe dispara (`IStoredFileWorkingCopySaveEvent`, com o `stat`).
    pub fn on_did_save_stored(&self) -> Event<StoredFileWorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    /// `save(options?, stat?)`: a versão da `TestWorkingCopy`, que aceita o `stat` do evento.
    pub fn save_with_stat(&self, options: Option<SaveOptions>, stat: Option<FileStatWithMetadata>) -> Promise<bool> {
        let reason = options.as_ref().and_then(|o| o.reason).unwrap_or(SaveReason::Explicit);
        let source = options.and_then(|o| o.source);
        self.on_did_save.fire(StoredFileWorkingCopySaveEvent {
            event: WorkingCopySaveEvent { reason: Some(reason), source },
            stat: stat.unwrap_or_else(|| create_file_stat(&self.resource, &FileStatOptions::default())),
        });
        Promise::resolved(true)
    }
}

impl IDisposable for TestWorkingCopy {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopy for TestWorkingCopy {
    fn working_copy_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        WorkingCopyCapabilities::NONE
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event().map(|e| e.event.clone(), None)
    }

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    fn is_modified(&self) -> bool {
        self.is_dirty()
    }

    fn backup(&self, _token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        Promise::resolved(WorkingCopyBackup::default())
    }

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        self.save_with_stat(options, None)
    }

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        self.set_dirty(false);
        Promise::resolved(())
    }
}

/// Um item de `children` de `createFileStat`: `{ resource, isFile?, isDirectory?, isSymbolicLink?, executable? }`.
#[derive(Clone, Debug)]
pub struct FileStatChild {
    pub resource: URI,
    pub is_file: Option<bool>,
    pub is_directory: Option<bool>,
    pub is_symbolic_link: Option<bool>,
    pub executable: Option<bool>,
}

/// Os parâmetros posicionais de `createFileStat` depois do `resource`; o que o original deixa `undefined` é `None`.
#[derive(Clone, Debug, Default)]
pub struct FileStatOptions {
    pub readonly: bool,
    pub is_file: Option<bool>,
    pub is_directory: Option<bool>,
    pub is_symbolic_link: Option<bool>,
    pub children: Option<Vec<FileStatChild>>,
    pub executable: Option<bool>,
}

/// `createFileStat`.
pub fn create_file_stat(resource: &URI, options: &FileStatOptions) -> FileStatWithMetadata {
    let now = je_base::runtime::date_now() as i64;
    FileStatWithMetadata::new(FileStat {
        resource: resource.clone(),
        etag: Some(now.to_string()),
        mtime: Some(now),
        ctime: Some(now),
        size: Some(42),
        is_file: options.is_file.unwrap_or(true),
        is_directory: options.is_directory.unwrap_or(false),
        is_symbolic_link: options.is_symbolic_link.unwrap_or(false),
        readonly: Some(options.readonly),
        locked: Some(false),
        executable: Some(options.executable.unwrap_or(false)),
        name: unbiased_ext_uri().basename(resource, None),
        children: options.children.as_ref().map(|children| {
            children
                .iter()
                .map(|c| {
                    let child_options = FileStatOptions {
                        is_file: c.is_file,
                        is_directory: c.is_directory,
                        is_symbolic_link: c.is_symbolic_link,
                        executable: c.executable,
                        ..FileStatOptions::default()
                    };
                    create_file_stat(&c.resource, &child_options).into_stat()
                })
                .collect()
        }),
    })
}

/// `TestProductService`: o `ProductService` padrão do porte (`TestProductService::default()`).
pub type TestProductService = ProductService;

/// `TestWorkspaceTrustEnablementService`.
pub struct TestWorkspaceTrustEnablementService {
    is_enabled: bool,
}

impl TestWorkspaceTrustEnablementService {
    /// `constructor(isEnabled = true)`.
    pub fn new(is_enabled: bool) -> Rc<Self> {
        Rc::new(Self { is_enabled })
    }
}

impl IWorkspaceTrustEnablementService for TestWorkspaceTrustEnablementService {
    fn is_workspace_trust_enabled(&self) -> bool {
        self.is_enabled
    }
}

/// `{ readonly scheme: string; readonly authority: string }` de `trustedAuthorities`.
struct TrustedAuthority {
    scheme: String,
    authority: String,
}

/// `TestWorkspaceTrustManagementService`.
pub struct TestWorkspaceTrustManagementService {
    base: Rc<Disposable>,
    trusted_authorities: Rc<RefCell<Vec<Rc<TrustedAuthority>>>>,
    on_did_change_trust: Rc<Emitter<bool>>,
    on_did_change_trusted_folders: Rc<Emitter<()>>,
    on_did_initiate_workspace_trust_request_on_startup: Rc<Emitter<()>>,
    trusted: Cell<bool>,
    trusted_uris: RefCell<ResourceSet>,
}

impl TestWorkspaceTrustManagementService {
    /// `constructor(trusted = true, trustedUris = new ResourceSet())`.
    pub fn new(trusted: Option<bool>, trusted_uris: Option<ResourceSet>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_trust = Emitter::new();
        let on_did_change_trusted_folders = Emitter::new();
        let on_did_initiate_workspace_trust_request_on_startup = Emitter::new();
        base.register(on_did_change_trust.clone());
        base.register(on_did_change_trusted_folders.clone());
        base.register(on_did_initiate_workspace_trust_request_on_startup.clone());
        Rc::new(Self {
            base,
            trusted_authorities: Rc::new(RefCell::new(Vec::new())),
            on_did_change_trust,
            on_did_change_trusted_folders,
            on_did_initiate_workspace_trust_request_on_startup,
            trusted: Cell::new(trusted.unwrap_or(true)),
            trusted_uris: RefCell::new(trusted_uris.unwrap_or_else(|| ResourceSet::new(None))),
        })
    }

    /// `onDidInitiateWorkspaceTrustRequestOnStartup` (a classe de teste o expõe, o contrato não).
    pub fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()> {
        self.on_did_initiate_workspace_trust_request_on_startup.event()
    }
}

impl IDisposable for TestWorkspaceTrustManagementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspaceTrustManagementService for TestWorkspaceTrustManagementService {
    fn on_did_change_trust(&self) -> Event<bool> {
        self.on_did_change_trust.event()
    }

    fn on_did_change_trusted_folders(&self) -> Event<()> {
        self.on_did_change_trusted_folders.event()
    }

    fn workspace_resolved(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn workspace_trust_initialized(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn accepts_out_of_workspace_files(&self) -> bool {
        not_implemented()
    }

    fn set_accepts_out_of_workspace_files(&self, _value: bool) {
        not_implemented()
    }

    fn is_workspace_trusted(&self) -> bool {
        self.trusted.get()
    }

    fn is_workspace_trust_forced(&self) -> bool {
        false
    }

    fn can_set_parent_folder_trust(&self) -> bool {
        not_implemented()
    }

    fn set_parent_folder_trust(&self, _trusted: bool) -> Promise<()> {
        not_implemented()
    }

    fn can_set_workspace_trust(&self) -> bool {
        not_implemented()
    }

    fn set_workspace_trust(&self, trusted: bool) -> Promise<()> {
        if self.trusted.get() != trusted {
            self.trusted.set(trusted);
            self.on_did_change_trust.fire(trusted);
        }
        Promise::resolved(())
    }

    fn get_uri_trust_info(&self, uri: &URI) -> Promise<WorkspaceTrustUriInfo> {
        let ext_uri = unbiased_ext_uri();
        let authority_trusted = self
            .trusted_authorities
            .borrow()
            .iter()
            .any(|entry| entry.scheme == uri.scheme().to_string_lossy() && ext_uri.is_equal_authority(&JsString::from(entry.authority.as_str()), uri.authority()));
        Promise::resolved(WorkspaceTrustUriInfo { trusted: authority_trusted || self.trusted_uris.borrow().has(uri), uri: uri.clone() })
    }

    fn set_uris_trust(&self, _uris: &[URI], _trusted: bool) -> Promise<()> {
        not_implemented_async()
    }

    fn register_trusted_authority(&self, scheme: &str, authority: &str) -> DisposableRef {
        let entry = Rc::new(TrustedAuthority { scheme: scheme.to_string(), authority: authority.to_string() });
        self.trusted_authorities.borrow_mut().push(entry.clone());
        let trusted_authorities = self.trusted_authorities.clone();
        to_disposable(move || trusted_authorities.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &entry)))
    }

    fn get_trusted_uris(&self) -> Vec<URI> {
        not_implemented()
    }

    fn set_trusted_uris(&self, folders: &[URI]) -> Promise<()> {
        *self.trusted_uris.borrow_mut() = ResourceSet::from_entries(folders.iter().cloned(), None);
        Promise::resolved(())
    }

    fn add_workspace_trust_transition_participant(&self, _participant: Rc<dyn IWorkspaceTrustTransitionParticipant>) -> DisposableRef {
        not_implemented()
    }
}

/// O tipo de `requestOpenUrisHandler`: `(uris: URI[]) => Promise<WorkspaceTrustUriResponse>`.
pub type RequestOpenUrisHandler = Rc<dyn Fn(&[URI]) -> Promise<WorkspaceTrustUriResponse>>;

/// `TestWorkspaceTrustRequestService`.
pub struct TestWorkspaceTrustRequestService {
    base: Rc<Disposable>,
    on_did_initiate_open_files_trust_request: Rc<Emitter<()>>,
    on_did_initiate_resources_trust_request: Rc<Emitter<ResourceTrustRequestOptions>>,
    on_did_initiate_workspace_trust_request: Rc<Emitter<Option<WorkspaceTrustRequestOptions>>>,
    on_did_initiate_workspace_trust_request_on_startup: Rc<Emitter<()>>,
    trusted: bool,
    /// `requestOpenUrisHandler`: os testes trocam o tratador atribuindo a este campo.
    pub request_open_uris_handler: RefCell<RequestOpenUrisHandler>,
}

impl TestWorkspaceTrustRequestService {
    pub fn new(trusted: bool) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_initiate_open_files_trust_request = Emitter::new();
        let on_did_initiate_resources_trust_request = Emitter::new();
        let on_did_initiate_workspace_trust_request = Emitter::new();
        let on_did_initiate_workspace_trust_request_on_startup = Emitter::new();
        base.register(on_did_initiate_open_files_trust_request.clone());
        base.register(on_did_initiate_resources_trust_request.clone());
        base.register(on_did_initiate_workspace_trust_request.clone());
        base.register(on_did_initiate_workspace_trust_request_on_startup.clone());
        let default_handler: RequestOpenUrisHandler = Rc::new(|_uris: &[URI]| Promise::resolved(WorkspaceTrustUriResponse::Open));
        Rc::new(Self {
            base,
            on_did_initiate_open_files_trust_request,
            on_did_initiate_resources_trust_request,
            on_did_initiate_workspace_trust_request,
            on_did_initiate_workspace_trust_request_on_startup,
            trusted,
            request_open_uris_handler: RefCell::new(default_handler),
        })
    }
}

impl IDisposable for TestWorkspaceTrustRequestService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspaceTrustRequestService for TestWorkspaceTrustRequestService {
    fn on_did_initiate_open_files_trust_request(&self) -> Event<()> {
        self.on_did_initiate_open_files_trust_request.event()
    }

    fn on_did_initiate_workspace_trust_request(&self) -> Event<Option<WorkspaceTrustRequestOptions>> {
        self.on_did_initiate_workspace_trust_request.event()
    }

    fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()> {
        self.on_did_initiate_workspace_trust_request_on_startup.event()
    }

    fn on_did_initiate_resources_trust_request(&self) -> Event<ResourceTrustRequestOptions> {
        self.on_did_initiate_resources_trust_request.event()
    }

    fn complete_open_files_trust_request(&self, _result: WorkspaceTrustUriResponse, _save_response: Option<bool>) -> Promise<()> {
        not_implemented_async()
    }

    fn request_open_files_trust(&self, open_files: &[URI]) -> Promise<WorkspaceTrustUriResponse> {
        let handler = self.request_open_uris_handler.borrow().clone();
        handler(open_files)
    }

    fn complete_resources_trust_request(&self, _uri: &URI, _result: WorkspaceTrustUriResponse) -> Promise<()> {
        not_implemented_async()
    }

    fn request_resources_trust(&self, _options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        Promise::resolved(Some(self.trusted))
    }

    fn cancel_workspace_trust_request(&self) {
        not_implemented()
    }

    fn complete_workspace_trust_request(&self, _trusted: Option<bool>) -> Promise<()> {
        not_implemented_async()
    }

    fn request_workspace_trust(&self, _options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        Promise::resolved(Some(self.trusted))
    }

    fn request_workspace_trust_on_startup(&self) {
        not_implemented()
    }
}

/// `TestMarkerService`.
pub struct TestMarkerService;

impl IMarkerService for TestMarkerService {
    fn get_statistics(&self) -> MarkerStatistics {
        not_implemented()
    }

    fn is_source_truncated(&self, _resource: &URI) -> bool {
        false
    }

    fn change_one(&self, _owner: &str, _resource: &URI, _markers: Vec<MarkerData>, _options: Option<MarkerChangeOptions>) {}

    fn change_all(&self, _owner: &str, _data: Vec<ResourceMarker>) {}

    fn remove(&self, _owner: &str, _resources: &[URI]) {}

    fn read(&self, _filter: Option<&MarkerReadOptions>) -> Vec<Marker> {
        Vec::new()
    }

    fn install_resource_filter(&self, _resource: &URI, _reason: &str) -> DisposableRef {
        lifecycle::none()
    }

    fn on_marker_changed(&self) -> Event<Vec<URI>> {
        Event::none()
    }
}

/// `TestLifecycleService`.
pub struct TestLifecycleService {
    base: Rc<Disposable>,
    /// `usePhases`.
    pub use_phases: Cell<bool>,
    phase: Cell<LifecyclePhase>,
    /// `startupKind`.
    pub startup_kind: Cell<StartupKind>,
    /// `willShutdown`.
    pub will_shutdown: Cell<bool>,
    when_started: DeferredPromise<()>,
    when_ready: DeferredPromise<()>,
    when_restored: DeferredPromise<()>,
    when_eventually: DeferredPromise<()>,
    on_before_shutdown: Rc<Emitter<InternalBeforeShutdownEvent>>,
    on_before_shutdown_error: Rc<Emitter<BeforeShutdownErrorEvent>>,
    on_shutdown_veto: Rc<Emitter<()>>,
    on_will_shutdown: Rc<Emitter<WillShutdownEvent>>,
    on_did_shutdown: Rc<Emitter<()>>,
    /// `shutdownJoiners`.
    pub shutdown_joiners: Rc<RefCell<Vec<Promise<()>>>>,
}

impl TestLifecycleService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_before_shutdown = Emitter::new();
        let on_before_shutdown_error = Emitter::new();
        let on_shutdown_veto = Emitter::new();
        let on_will_shutdown = Emitter::new();
        let on_did_shutdown = Emitter::new();
        base.register(on_before_shutdown.clone());
        base.register(on_before_shutdown_error.clone());
        base.register(on_shutdown_veto.clone());
        base.register(on_will_shutdown.clone());
        base.register(on_did_shutdown.clone());
        Rc::new(Self {
            base,
            use_phases: Cell::new(false),
            phase: Cell::new(LifecyclePhase::Starting),
            startup_kind: Cell::new(StartupKind::NewWindow),
            will_shutdown: Cell::new(false),
            when_started: DeferredPromise::new(),
            when_ready: DeferredPromise::new(),
            when_restored: DeferredPromise::new(),
            when_eventually: DeferredPromise::new(),
            on_before_shutdown,
            on_before_shutdown_error,
            on_shutdown_veto,
            on_will_shutdown,
            on_did_shutdown,
            shutdown_joiners: Rc::new(RefCell::new(Vec::new())),
        })
    }

    /// `onBeforeShutdown` com o tipo que a classe de teste expõe (`Event<InternalBeforeShutdownEvent>`, com o `finalVeto`).
    pub fn on_internal_before_shutdown(&self) -> Event<InternalBeforeShutdownEvent> {
        self.on_before_shutdown.event()
    }

    /// `fireShutdown(reason = ShutdownReason.QUIT)`.
    pub fn fire_shutdown(&self, reason: Option<ShutdownReason>) {
        self.shutdown_joiners.borrow_mut().clear();
        let shutdown_joiners = self.shutdown_joiners.clone();
        self.on_will_shutdown.fire(WillShutdownEvent::new(
            reason.unwrap_or(ShutdownReason::Quit),
            CancellationToken::None,
            Rc::new(move |join: WillShutdownJoin, _joiner: WillShutdownEventJoiner| {
                let promise = match join {
                    WillShutdownJoin::Promise(promise) => promise,
                    WillShutdownJoin::Function(promise_fn) => promise_fn(),
                };
                shutdown_joiners.borrow_mut().push(promise);
            }),
            Rc::new(Vec::new),
            // No-Op in tests
            Rc::new(|| {}),
        ));
    }

    pub fn fire_before_shutdown(&self, event: InternalBeforeShutdownEvent) {
        self.on_before_shutdown.fire(event);
    }

    pub fn fire_will_shutdown(&self, event: WillShutdownEvent) {
        self.on_will_shutdown.fire(event);
    }
}

impl IDisposable for TestLifecycleService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ILifecycleService for TestLifecycleService {
    fn startup_kind(&self) -> StartupKind {
        self.startup_kind.get()
    }

    fn phase(&self) -> LifecyclePhase {
        self.phase.get()
    }

    fn set_phase(&self, phase: LifecyclePhase) {
        self.phase.set(phase);
        let phase_promise = match phase {
            LifecyclePhase::Starting => &self.when_started,
            LifecyclePhase::Ready => &self.when_ready,
            LifecyclePhase::Restored => &self.when_restored,
            LifecyclePhase::Eventually => &self.when_eventually,
        };
        let _ = phase_promise.complete(());
    }

    fn on_before_shutdown(&self) -> Event<BeforeShutdownEvent> {
        self.on_before_shutdown.event().map(|e| e.event.clone(), None)
    }

    fn on_shutdown_veto(&self) -> Event<()> {
        self.on_shutdown_veto.event()
    }

    fn on_before_shutdown_error(&self) -> Event<BeforeShutdownErrorEvent> {
        self.on_before_shutdown_error.event()
    }

    fn on_will_shutdown(&self) -> Event<WillShutdownEvent> {
        self.on_will_shutdown.event()
    }

    fn will_shutdown(&self) -> bool {
        self.will_shutdown.get()
    }

    fn on_did_shutdown(&self) -> Event<()> {
        self.on_did_shutdown.event()
    }

    fn when(&self, phase: LifecyclePhase) -> Promise<()> {
        if !self.use_phases.get() {
            return Promise::resolved(());
        }
        match phase {
            LifecyclePhase::Starting => self.when_started.promise.clone(),
            LifecyclePhase::Ready => self.when_ready.promise.clone(),
            LifecyclePhase::Restored => self.when_restored.promise.clone(),
            LifecyclePhase::Eventually => self.when_eventually.promise.clone(),
        }
    }

    fn shutdown(&self) -> Promise<()> {
        self.fire_shutdown(None);
        Promise::resolved(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `TestFileService`, `TestTextResourceConfigurationService` e `TestFilesConfigurationService`.
//
// - `TestFileService` implementa o `IFileService` reduzido de `je_platform::files::i_file_service` (o porte do serviço de
//   arquivos é uma struct concreta): os métodos que o contrato não tem (`readFile`, `writeFile`, `move`, `watch`...)
//   entram junto com o contrato. O `resolve` com metadados devolve o `createFileStat(resource, this.readonly)` do original.
//   O `TestFileService` do original não herda de `Disposable` mas é registrado por `disposables.add(...)`: aqui o
//   `IDisposable` descarta os emissores.
// - `TestTextResourceConfigurationService` é do `test/browser/workbenchTestServices.ts`; mora aqui porque os testes de
//   `common` e de `browser` o usam do mesmo jeito.
// - `TestFilesConfigurationService` só expõe o `onFilesConfigurationChange` protegido como `testOnFilesConfigurationChange`;
//   como o `onFilesConfigurationChange` já é público aqui, o tipo é o próprio serviço.
// ---------------------------------------------------------------------------------------------------------------------

/// `TestFilesConfigurationService`.
pub type TestFilesConfigurationService = crate::services::files_configuration::files_configuration_service::FilesConfigurationService;

/// `NullFilesConfigurationService`: o `IFilesConfigurationService` que não faz nada (os métodos que o original deixa com
/// `throw new Error('Method not implemented.')` lançam, e `isReadonly` e `updateReadonly` não fazem nada).
pub struct NullFilesConfigurationService;

impl IFilesConfigurationService for NullFilesConfigurationService {
    fn on_did_change_auto_save_configuration(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_auto_save_disabled(&self) -> Event<URI> {
        Event::none()
    }

    fn get_auto_save_configuration(&self, _resource_or_editor: Option<&ResourceOrEditor>) -> IAutoSaveConfiguration {
        not_implemented()
    }

    fn has_short_auto_save_delay(&self, _resource_or_editor: Option<&ResourceOrEditor>) -> bool {
        not_implemented()
    }

    fn get_auto_save_mode(&self, _resource_or_editor: Option<&ResourceOrEditor>, _save_reason: Option<SaveReason>) -> IAutoSaveMode {
        not_implemented()
    }

    fn toggle_auto_save(&self) -> Promise<()> {
        not_implemented()
    }

    fn enable_auto_save_after_short_delay(&self, _resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        not_implemented()
    }

    fn disable_auto_save(&self, _resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        not_implemented()
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        Event::none()
    }

    fn is_readonly(&self, _resource: &URI, _stat: Option<&FileStat>) -> Readonly {
        Readonly::Bool(false)
    }

    fn update_readonly(&self, _resource: &URI, _readonly: SingleReadonlyUpdate) -> Promise<()> {
        Promise::resolved(())
    }

    fn update_readonly_all(&self, _resources: &[URI], _readonly: ReadonlyUpdate) -> Promise<()> {
        Promise::resolved(())
    }

    fn on_did_change_files_association(&self) -> Event<()> {
        Event::none()
    }

    fn is_hot_exit_enabled(&self) -> bool {
        false
    }

    fn hot_exit_configuration(&self) -> Option<String> {
        None
    }

    fn prevent_save_conflicts(&self, _resource: &URI, _language: Option<&str>) -> bool {
        not_implemented()
    }
}

/// `TestTextResourceConfigurationService`.
pub struct TestTextResourceConfigurationService {
    configuration_service: Rc<dyn IConfigurationService>,
}

impl TestTextResourceConfigurationService {
    /// `constructor(private configurationService = new TestConfigurationService())`.
    pub fn new(configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new(Self { configuration_service })
    }

    /// `{ resource }`.
    fn overrides_of(resource: Option<&URI>) -> ConfigurationOverrides {
        ConfigurationOverrides { resource: resource.cloned(), override_identifier: None }
    }
}

impl ITextResourceConfigurationService for TestTextResourceConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<dyn ITextResourceConfigurationChangeEvent>> {
        Event::none()
    }

    fn get_value(&self, resource: Option<&URI>, section: Option<&str>) -> Dynamic {
        self.configuration_service.get_value(section, Some(&Self::overrides_of(resource)))
    }

    fn get_value_at(&self, resource: Option<&URI>, position: Option<&Position>, section: Option<&str>) -> Dynamic {
        // A seção só vale quando a posição vem junto: sem posição o `arg2` do original não é uma seção.
        let section = if position.is_some() { section } else { None };
        self.configuration_service.get_value(section, Some(&Self::overrides_of(resource)))
    }

    fn inspect(&self, resource: Option<&URI>, _position: Option<&Position>, section: &str) -> Rc<dyn ConfigurationValue> {
        self.configuration_service.inspect(section, &Self::overrides_of(resource))
    }

    fn update_value(&self, _resource: Option<&URI>, key: &str, value: Dynamic, _configuration_target: Option<ConfigurationTarget>) -> Promise<()> {
        self.configuration_service.update_value(key, value, None, None)
    }
}

/// `TestFileService`.
pub struct TestFileService {
    base: Rc<Disposable>,
    on_did_files_change: Rc<Emitter<Rc<FileChangesEvent>>>,
    on_did_change_file_system_provider_capabilities: Rc<Emitter<FileSystemProviderCapabilitiesChangeEvent>>,
    /// `readonly`.
    pub readonly: Cell<bool>,
    /// `notExistsSet`.
    pub not_exists_set: RefCell<ResourceMap<bool>>,
    providers: RefCell<HashMap<String, FileSystemProviderRef>>,
}

impl TestFileService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_files_change = Emitter::new();
        base.register(on_did_files_change.clone());
        let on_did_change_file_system_provider_capabilities = Emitter::new();
        base.register(on_did_change_file_system_provider_capabilities.clone());
        Rc::new(Self {
            base,
            on_did_files_change,
            on_did_change_file_system_provider_capabilities,
            readonly: Cell::new(false),
            not_exists_set: RefCell::new(ResourceMap::new()),
            providers: RefCell::new(HashMap::new()),
        })
    }

    /// `fireFileChanges(event)`.
    pub fn fire_file_changes(&self, event: Rc<FileChangesEvent>) {
        self.on_did_files_change.fire(event);
    }

    /// `fireFileSystemProviderCapabilitiesChangeEvent(event)`.
    pub fn fire_file_system_provider_capabilities_change_event(&self, event: FileSystemProviderCapabilitiesChangeEvent) {
        self.on_did_change_file_system_provider_capabilities.fire(event);
    }

    /// `registerProvider(scheme, provider)`.
    pub fn register_provider(self: &Rc<Self>, scheme: &str, provider: FileSystemProviderRef) -> DisposableRef {
        self.providers.borrow_mut().insert(scheme.to_string(), provider);

        let this = self.clone();
        let scheme = scheme.to_string();
        to_disposable(move || {
            this.providers.borrow_mut().remove(&scheme);
        })
    }
}

impl IDisposable for TestFileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IFileService for TestFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        self.on_did_files_change.event()
    }

    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        self.providers.borrow().get(scheme).cloned()
    }

    fn resolve_with_metadata(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        Promise::resolved(create_file_stat(resource, &FileStatOptions { readonly: self.readonly.get(), ..FileStatOptions::default() }))
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        Promise::resolved(!self.not_exists_set.borrow().has(resource))
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Event::none()
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        self.on_did_change_file_system_provider_capabilities.event()
    }

    fn has_provider(&self, resource: &URI) -> bool {
        resource.scheme() == schemas::FILE || self.providers.borrow().contains_key(&resource.scheme().to_string_lossy())
    }

    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        if capability == FileSystemProviderCapabilities::PATH_CASE_SENSITIVE && Platform::detect().is_linux {
            return true;
        }
        self.get_provider(&resource.scheme().to_string_lossy()).is_some_and(|provider| provider.capabilities().intersects(capability))
    }

    /// `readFile(resource, options?)` do original sem o `readShouldThrowError` e sem o rastreio de leituras (`content`,
    /// `lastReadFileUri`, `readOperations`), que o porte do `TestFileService` ainda não tem: devolve o stat do
    /// `createFileStat(resource, readonly)` e o conteúdo padrão `'Hello Html'` da classe.
    fn read_file(&self, resource: &URI, _options: Option<je_platform::files::files::ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<je_platform::files::files::FileContent> {
        Promise::resolved(je_platform::files::files::FileContent {
            stat: create_file_stat(resource, &FileStatOptions { readonly: self.readonly.get(), ..FileStatOptions::default() }),
            value: je_base::buffer::VSBuffer::from_string("Hello Html"),
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Dublês do teste do `ExtensionEnablementService`: `TestChatEntitlementService` (deste arquivo no original, sobre o
// recorte `IChatEntitlementService` de `services::extension_management::service_contracts`) e
// `TestNotificationService` (de `platform/notification/test/common/testNotificationService.ts`, que não mostra nada).
//
// - `TestChatEntitlementService.context` (um `Lazy<ChatEntitlementContext> | undefined` atribuível) é o
//   [`TestChatEntitlementService::set_context`]; os demais membros do original (cotas, sentimento, entitlement...)
//   pertencem ao contrato completo do chat, que está fora do escopo do porte.
// - No original `info`, `warn` e `error` do `TestNotificationService` chamam `notify` e descartam o resultado; o
//   [`INotificationService`] do porte os declara sem retorno e, como `notify` não tem efeito, eles não fazem nada.
// ---------------------------------------------------------------------------------------------------------------------

pub use extension_enablement_test_services::{TestChatEntitlementService, TestNotificationService};

mod extension_enablement_test_services {
    use std::cell::RefCell;
    use std::rc::Rc;

    use je_base::event::Event;
    use je_platform::notification::{
        INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
        NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
    };

    use crate::services::extension_management::service_contracts::{ChatEntitlementContext, IChatEntitlementService};

    /// `TestChatEntitlementService`.
    #[derive(Default)]
    pub struct TestChatEntitlementService {
        context: RefCell<Option<ChatEntitlementContext>>,
    }

    impl TestChatEntitlementService {
        pub fn new() -> Rc<Self> {
            Rc::new(Self::default())
        }

        /// `service.context = ...`.
        pub fn set_context(&self, context: Option<ChatEntitlementContext>) {
            *self.context.borrow_mut() = context;
        }
    }

    impl IChatEntitlementService for TestChatEntitlementService {
        fn context(&self) -> Option<ChatEntitlementContext> {
            *self.context.borrow()
        }
    }

    /// O handle de status que não faz nada (o `Disposable.None` do original).
    struct NoOpStatus;

    impl StatusHandle for NoOpStatus {
        fn close(&self) {}
    }

    /// `TestNotificationService`.
    #[derive(Default)]
    pub struct TestNotificationService;

    impl INotificationService for TestNotificationService {
        fn on_did_change_filter(&self) -> Event<()> {
            Event::none()
        }

        fn set_filter(&self, _filter: NotificationFilterSetting) {}

        fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
            NotificationsFilter::Off
        }

        fn get_filters(&self) -> Vec<NotificationSourceFilter> {
            Vec::new()
        }

        fn remove_filter(&self, _source_id: &str) {}

        fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
            Rc::new(NoOpNotification::new())
        }

        fn info(&self, _message: Vec<NotificationMessage>) {}

        fn warn(&self, _message: Vec<NotificationMessage>) {}

        fn error(&self, _message: Vec<NotificationMessage>) {}

        fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
            Rc::new(NoOpNotification::new())
        }

        fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
            Rc::new(NoOpStatus)
        }
    }
}
