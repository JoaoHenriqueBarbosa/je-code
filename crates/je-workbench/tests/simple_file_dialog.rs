//! Porte de `src/vs/workbench/services/dialogs/test/browser/simpleFileDialog.test.ts` (suíte `SimpleFileDialog`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `disposables.add` vira `guard.add_rc`.
//! - **`Object.assign(Object.create(SimpleFileDialog.prototype), { ... })`.** Rust não tem protótipo nem campo atribuível de
//!   fora: o caso monta o [`SimpleFileDialog`] de verdade (`Injectable::create`, com os dublês do porte) e atribui os campos
//!   por [`DialogTestState`] (`configure_for_testing`), com o `filePickBox` sendo um quick pick do serviço de teste
//!   (`attach_pick_box_for_testing`). Os métodos que o original chama pelo protótipo (`validate`, `handleValueChange`,
//!   `onDidAccept`, `updateItems`, `resolvePathFormatting`, `pathFromUri`, `remoteUriFrom`) são `pub` e `#[doc(hidden)]`.
//! - **Métodos sobrescritos pelo objeto (`yesNoPrompt`, `insertText`, `tryUpdateItems`, `validate`, `createItems`,
//!   `setActiveItems`).** Não há como trocar um método de um struct. O caso roda o código de produção por inteiro e observa o
//!   mesmo efeito: o `yesNoPrompt` é o quick pick de confirmação real, aceito pelo caso (o `promptedUri` vira o valor do
//!   quick pick, que é o `pathFromUri` do recurso); o `insertText` é o valor final do campo; o `tryUpdateItems` é conferido
//!   pelas consultas ao serviço de arquivos (ou pela pasta atual, quando a pasta existe de verdade); o `createItems` é o real,
//!   sobre uma pasta sem filhos.
//! - **`{ ...fileService, stat, createFolder }` e `new class extends mock<IFileService>()`.** É o [`GuardedFileService`]: embrulha
//!   o `FileService` real e repassa cada método do `IFileService`, sobrescrevendo só o `stat` (que registra o caminho e lança o
//!   erro que o caso mandar), o `resolve` e o `exists`. No caso do `mock`, os métodos que o objeto não define não são chamados
//!   pelo código de produção, então o repasse não muda o resultado.
//! - **`createFileSystemProviderError('No permissions', NoPermissions)`.** É `create_file_system_provider_error_from_text`.
//! - **`for (const permitted of [true, false]) test(...)`.** Vira um teste por valor, na ordem do original.
//! - **`instantiationService.stub(IFileService, 'resolve', ...)` e a promessa lenta.** O `resolve` do
//!   [`GuardedFileService`] devolve a promessa pendente da pasta lenta, resolvida pelo caso; o `.catch(() => undefined)` é o
//!   `Promise::catch`.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_notification_service.rs"]
mod test_notification_service;
#[path = "../../je-platform/tests/common/keybinding.rs"]
mod keybinding;
#[path = "support/test_quick_input.rs"]
mod test_quick_input;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::network::schemas;
use je_base::path::Platform;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::Injectable;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{ConfirmResult, FileNameOrResource, IFileDialogService, OpenDialogOptions, PickAndOpenOptions, SaveDialogOptions};
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderErrorCode, FileSystemProviderRef,
    FileSystemProviderRegistrationEvent, PartialFileDeleteOptions, ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
    create_file_system_provider_error_from_text,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::log::NullLogService;
use je_platform::path::path_service::{IPathService, IResourcePathProvider};
use je_platform::quickinput::quick_input::IQuickPick;
use je_platform::resources::{ext_uri, join_path, uri_file};
use je_platform::storage::storage::{InMemoryStorageService, StorageService};
use je_workbench::services::dialogs::simple_file_dialog::{DialogTestState, SimpleFileDialog};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestTextResourcePropertiesService};
use keybinding::MockKeybindingService;
use test_notification_service::TestNotificationService;
use test_quick_input::TestQuickInputService;

/// `new FileService(...)` e o que o `IFileService` repassa: o `FileService` real com o `stat`, o `resolve` e o `exists`
/// sobrescritíveis, e o registro dos caminhos consultados no `stat`.
struct GuardedFileService {
    inner: Rc<FileService>,
    stat_requests: RefCell<Vec<String>>,
    resolve_requests: RefCell<Vec<String>>,
    stat_error: RefCell<Option<Rc<dyn Fn(&URI) -> Option<Error>>>>,
    resolve_override: RefCell<Option<Rc<dyn Fn(&URI) -> Promise<FileStat>>>>,
    exists_override: Cell<Option<bool>>,
}

impl GuardedFileService {
    fn new(inner: Rc<FileService>) -> Rc<Self> {
        Rc::new(Self {
            inner,
            stat_requests: RefCell::new(Vec::new()),
            resolve_requests: RefCell::new(Vec::new()),
            stat_error: RefCell::new(None),
            resolve_override: RefCell::new(None),
            exists_override: Cell::new(None),
        })
    }
}

impl IFileService for GuardedFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        IFileService::on_did_files_change(&*self.inner)
    }

    fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        IFileService::get_provider(&*self.inner, scheme)
    }

    fn resolve_with_metadata(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::resolve_with_metadata(&*self.inner, resource, options)
    }

    /// `stat: async (resource) => { ... }`.
    fn stat(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        self.stat_requests.borrow_mut().push(resource.path().to_string_lossy());
        let hook = self.stat_error.borrow().clone();
        if let Some(error) = hook.and_then(|hook| hook(resource)) {
            return Promise::rejected(error);
        }
        IFileService::stat(&*self.inner, resource)
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        match self.exists_override.get() {
            Some(exists) => Promise::resolved(exists),
            None => IFileService::exists(&*self.inner, resource),
        }
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        IFileService::on_did_change_file_system_provider_registrations(&*self.inner)
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        IFileService::on_did_change_file_system_provider_capabilities(&*self.inner)
    }

    fn has_provider(&self, resource: &URI) -> bool {
        IFileService::has_provider(&*self.inner, resource)
    }

    fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        IFileService::has_capability(&*self.inner, resource, capability)
    }

    fn read_file(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent> {
        IFileService::read_file(&*self.inner, resource, options, token)
    }

    /// `resolve: (resource) => ...` do `instantiationService.stub`.
    fn resolve(&self, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        self.resolve_requests.borrow_mut().push(resource.path().to_string_lossy());
        let hook = self.resolve_override.borrow().clone();
        match hook {
            Some(hook) => hook(resource),
            None => IFileService::resolve(&*self.inner, resource, options),
        }
    }

    fn create_folder(&self, resource: &URI) -> Promise<FileStatWithMetadata> {
        IFileService::create_folder(&*self.inner, resource)
    }

    fn watch(&self, resource: &URI, options: Option<WatchOptions>) -> DisposableRef {
        IFileService::watch(&*self.inner, resource, options)
    }

    fn read_file_stream(&self, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        IFileService::read_file_stream(&*self.inner, resource, options, token)
    }

    fn write_file(&self, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::write_file(&*self.inner, resource, content, options)
    }

    fn del(&self, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        IFileService::del(&*self.inner, resource, options)
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        IFileService::on_did_run_operation(&*self.inner)
    }

    fn create_file(&self, resource: &URI, content: Option<FileContentSource>, options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        IFileService::create_file(&*self.inner, resource, content, options)
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        IFileService::on_will_activate_file_system_provider(&*self.inner)
    }

    fn register_provider(&self, scheme: &str, provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        IFileService::register_provider(&*self.inner, scheme, provider)
    }
}

/// O `ILabelService` do `workbenchInstantiationService`, com o `labelService: { getSeparator: () => { throw ... } }` dos casos
/// com autoridade: nenhum caso lê o separador de exibição.
struct StubLabelService;

impl ILabelService for StubLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        unreachable!("ILabelService.getUriLabel não é usado por este teste")
    }

    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        unreachable!("ILabelService.getUriBasenameLabel não é usado por este teste")
    }

    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        unreachable!("ILabelService.getWorkspaceLabel não é usado por este teste")
    }

    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        unreachable!("ILabelService.getHostLabel não é usado por este teste")
    }

    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        unreachable!("ILabelService.getHostTooltip não é usado por este teste")
    }

    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        panic!("Scoped paths must not use display separators")
    }

    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        unreachable!("ILabelService.getUriHome não é usado por este teste")
    }

    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        unreachable!("ILabelService.registerFormatter não é usado por este teste")
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }

    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        unreachable!("ILabelService.registerCachedFormatter não é usado por este teste")
    }
}

/// O `IFileDialogService` do `workbenchInstantiationService`: nenhum caso abre diálogo.
struct UnusedFileDialogService;

impl IFileDialogService for UnusedFileDialogService {
    fn default_file_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultFilePath não é usado por este teste")
    }

    fn default_folder_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultFolderPath não é usado por este teste")
    }

    fn default_workspace_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.defaultWorkspacePath não é usado por este teste")
    }

    fn pick_file_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFileFolderAndOpen não é usado por este teste")
    }

    fn pick_file_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFileAndOpen não é usado por este teste")
    }

    fn pick_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickFolderAndOpen não é usado por este teste")
    }

    fn pick_workspace_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        unreachable!("IFileDialogService.pickWorkspaceAndOpen não é usado por este teste")
    }

    fn pick_file_to_save(&self, _default_uri: &URI, _available_file_systems: Option<Vec<String>>) -> Promise<Option<URI>> {
        unreachable!("IFileDialogService.pickFileToSave não é usado por este teste")
    }

    fn preferred_home(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        unreachable!("IFileDialogService.preferredHome não é usado por este teste")
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions) -> Promise<Option<URI>> {
        unreachable!("IFileDialogService.showSaveDialog não é usado por este teste")
    }

    fn show_save_confirm(&self, _file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult> {
        unreachable!("IFileDialogService.showSaveConfirm não é usado por este teste")
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions) -> Promise<Option<Vec<URI>>> {
        unreachable!("IFileDialogService.showOpenDialog não é usado por este teste")
    }
}

/// O `IAccessibilityService` do `workbenchInstantiationService`: sem leitor de tela.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }

    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    fn is_motion_reduced(&self) -> bool {
        false
    }

    fn is_transparency_reduced(&self) -> bool {
        false
    }

    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }

    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}

    fn alert(&self, _message: &str) {}

    fn status(&self, _message: &str) {}
}

/// O `pathService` dos casos de sistemas de arquivos com autoridade: devolve o sistema operacional e o módulo de caminho
/// configurados (ou `undefined`) e registra cada consulta (`semanticRequests`).
struct ScopedPathService {
    operating_system: Option<OperatingSystem>,
    path: Option<Platform>,
    requests: RefCell<Vec<String>>,
}

impl ScopedPathService {
    fn new(operating_system: Option<OperatingSystem>, path: Option<Platform>) -> Rc<Self> {
        Rc::new(Self { operating_system, path, requests: RefCell::new(Vec::new()) })
    }
}

impl IPathService for ScopedPathService {
    fn path(&self) -> Promise<Platform> {
        unreachable!("IPathService.path não é usado por este teste")
    }

    fn default_uri_scheme(&self) -> String {
        schemas::FILE.to_string()
    }

    fn file_uri(&self, path: &str) -> Promise<URI> {
        Promise::resolved(uri_file(path))
    }

    fn user_home(&self) -> Promise<URI> {
        unreachable!("IPathService.userHome não é usado por este teste")
    }

    fn local_user_home(&self) -> URI {
        unreachable!("IPathService.localUserHome não é usado por este teste")
    }

    fn has_valid_basename(&self, _resource: &URI, _basename: Option<&str>) -> Promise<bool> {
        unreachable!("IPathService.hasValidBasename não é usado por este teste")
    }

    fn has_valid_basename_for_os(&self, _resource: &URI, _os: OperatingSystem, _basename: Option<&str>) -> bool {
        unreachable!("IPathService.hasValidBasename não é usado por este teste")
    }

    /// `getOperatingSystem: async (uri) => { semanticRequests.push(`os:${uri.toString()}`); ... }`.
    fn get_operating_system(&self, resource: &URI) -> Promise<Option<OperatingSystem>> {
        self.requests.borrow_mut().push(format!("os:{}", uri_text(resource)));
        Promise::resolved(self.operating_system)
    }

    /// `getPath: async (uri) => { semanticRequests.push(`path:${uri.toString()}`); ... }`.
    fn get_path(&self, resource: &URI) -> Promise<Option<Platform>> {
        self.requests.borrow_mut().push(format!("path:{}", uri_text(resource)));
        Promise::resolved(self.path)
    }

    fn register_path_provider(&self, _scheme: &str, _provider: Rc<dyn IResourcePathProvider>) -> DisposableRef {
        unreachable!("IPathService.registerPathProvider não é usado por este teste")
    }

    fn resolved_user_home(&self) -> Option<URI> {
        None
    }
}

/// `resource.toString()`.
fn uri_text(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `URI.from({ scheme: Schemas.inMemory, path })`.
fn in_memory_uri(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::IN_MEMORY)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `{ scheme, authority, path }` de um recurso.
fn parts(resource: &URI) -> (String, String, String) {
    (resource.scheme().to_string_lossy(), resource.authority().to_string_lossy(), resource.path().to_string_lossy())
}

/// `resources.isEqual(a, b)`.
fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// `createFileSystemProviderError('No permissions', FileSystemProviderErrorCode.NoPermissions)`.
fn no_permissions() -> Error {
    create_file_system_provider_error_from_text("No permissions", FileSystemProviderErrorCode::NoPermissions)
}

/// O `setup` da suíte (`fileService` e `provider` registrado em `Schemas.inMemory`) e o diálogo montado sobre os dublês do
/// `workbenchInstantiationService`, com o `filePickBox` anexado.
struct Fixture {
    guard: LeakGuard,
    file_service: Rc<FileService>,
    provider: Rc<InMemoryFileSystemProvider>,
    guarded: Rc<GuardedFileService>,
    quick_input: Rc<TestQuickInputService>,
    dialog: Rc<SimpleFileDialog>,
    pick: Rc<dyn IQuickPick>,
}

impl Fixture {
    fn new(path_service: Rc<dyn IPathService>) -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service = guard.add_rc(FileService::new(NullLogService::shared()));
        let provider = guard.add_rc(InMemoryFileSystemProvider::new());
        let provider_ref: FileSystemProviderRef = provider.clone();
        guard.add(file_service.register_provider(schemas::IN_MEMORY, provider_ref).unwrap());

        let guarded = GuardedFileService::new(file_service.clone());
        let quick_input = TestQuickInputService::new();
        let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let model_service = guard.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service),
            UndoRedoService::for_tests(),
        ));
        let language_service = guard.add_rc(LanguageService::new(false));
        let context_key_service = guard.add_rc(MockContextKeyService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        let dialog = guard.add_rc(SimpleFileDialog::create(
            (),
            (
                guarded.clone(),
                quick_input.clone(),
                Rc::new(StubLabelService),
                TestContextService::new(None, None),
                Rc::new(TestNotificationService),
                Rc::new(UnusedFileDialogService),
                model_service,
                language_service,
                TestEnvironmentService::new(),
                Rc::new(TestRemoteAgentService),
                path_service,
                Rc::new(MockKeybindingService),
                context_key_service,
                Rc::new(TestAccessibilityService),
                storage_service,
            ),
        ));
        let pick = dialog.attach_pick_box_for_testing();
        Self { guard, file_service, provider, guarded, quick_input, dialog, pick }
    }

    /// `createFolderOnlyDialog(fileService, options)`.
    fn folder_only(is_windows: bool) -> Self {
        let fixture = Self::new(TestPathService::new(None, None));
        fixture.dialog.configure_for_testing(DialogTestState {
            requires_trailing: Some(false),
            allow_folder_selection: Some(true),
            allow_file_selection: Some(false),
            is_windows: Some(is_windows),
            ..DialogTestState::default()
        });
        fixture
    }

    /// `result.dialog.validate(resource)` e o `promptedUri`: o quick pick de confirmação que a validação abre (o segundo quick
    /// pick criado) é aceito, como o `yesNoPrompt: async () => true` do original, e o valor dele é devolvido.
    async fn validate(&self, resource: &URI) -> (bool, Option<String>) {
        let validation = self.dialog.validate(Some(resource.clone()));
        timeout(0).await.unwrap();
        let prompt = self.quick_input.created.borrow().get(1).cloned();
        let prompted = prompt.map(|prompt| {
            let value = prompt.value();
            prompt.accept(None);
            value
        });
        (validation.await.unwrap(), prompted)
    }

    /// `fileService.exists(resource)`.
    async fn exists(&self, resource: &URI) -> bool {
        self.file_service.exists(resource).await.unwrap()
    }
}

#[test]
fn creates_nested_missing_folders_from_a_folder_only_open_dialog() {
    run_with_real_timers(async {
        let fixture = Fixture::folder_only(false);
        let root = in_memory_uri("/root");
        let existing_folder = join_path(&root, &["folderA"]);
        let nested_folder = join_path(&existing_folder, &["newFolder1", "newFolder2"]);

        fixture.file_service.create_folder(&existing_folder).await.unwrap();

        let (accepted, prompted) = fixture.validate(&nested_folder).await;

        assert!(accepted);
        assert_eq!(prompted, Some(fixture.dialog.path_from_uri(&nested_folder, false)));
        assert!(fixture.exists(&nested_folder).await);
    });
}

#[test]
fn does_not_create_a_missing_folder_below_a_readonly_parent() {
    run_with_real_timers(async {
        let fixture = Fixture::folder_only(false);
        let root = in_memory_uri("/root");
        let existing_folder = join_path(&root, &["folderA"]);
        let nested_folder = join_path(&existing_folder, &["newFolder"]);

        fixture.file_service.create_folder(&existing_folder).await.unwrap();
        fixture.provider.set_read_only(true);

        let (accepted, prompted) = fixture.validate(&nested_folder).await;

        assert!(!accepted);
        assert_eq!(prompted, None);
        assert!(!fixture.exists(&nested_folder).await);
    });
}

#[test]
fn does_not_create_a_missing_folder_below_a_file() {
    run_with_real_timers(async {
        let fixture = Fixture::folder_only(false);
        let root = in_memory_uri("/root");
        let existing_file = join_path(&root, &["file.txt"]);
        let nested_folder = join_path(&existing_file, &["newFolder"]);

        fixture.file_service.create_file(&existing_file, Some("contents".into()), None).await.unwrap();

        let (accepted, prompted) = fixture.validate(&nested_folder).await;

        assert!(!accepted);
        assert_eq!(prompted, None);
        assert!(!fixture.exists(&nested_folder).await);
    });
}

#[test]
fn does_not_create_a_missing_folder_with_an_invalid_path_segment() {
    run_with_real_timers(async {
        let fixture = Fixture::folder_only(true);
        let root = in_memory_uri("/root");
        let existing_folder = join_path(&root, &["folderA"]);
        let nested_folder = join_path(&existing_folder, &["bad:name", "newFolder"]);

        fixture.file_service.create_folder(&existing_folder).await.unwrap();

        let (accepted, prompted) = fixture.validate(&nested_folder).await;

        assert!(!accepted);
        assert_eq!(prompted, None);
        assert!(!fixture.exists(&nested_folder).await);
    });
}

#[test]
fn does_not_create_a_missing_folder_when_parent_lookup_fails_for_reasons_other_than_missing_files() {
    run_with_real_timers(async {
        let fixture = Fixture::folder_only(false);
        let root = in_memory_uri("/root");
        let existing_folder = join_path(&root, &["folderA"]);
        let protected_folder = join_path(&existing_folder, &["protected"]);
        let nested_folder = join_path(&protected_folder, &["newFolder"]);

        fixture.file_service.create_folder(&existing_folder).await.unwrap();

        *fixture.guarded.stat_error.borrow_mut() = Some(Rc::new(move |resource: &URI| is_equal(resource, &protected_folder).then(no_permissions)));

        let (accepted, prompted) = fixture.validate(&nested_folder).await;

        assert!(!accepted);
        assert_eq!(prompted, None);
        assert!(!fixture.exists(&nested_folder).await);
    });
}

fn accepts_or_rejects_the_selected_folder_independently_of_denied_parent_access(permitted: bool) {
    run_with_real_timers(async move {
        let fixture = Fixture::folder_only(false);
        let root = in_memory_uri("/root");
        let folder = join_path(&root, &["granted"]);
        fixture.file_service.create_folder(&folder).await.unwrap();
        // `requests` registra só as consultas da validação.
        fixture.guarded.stat_requests.borrow_mut().clear();
        *fixture.guarded.stat_error.borrow_mut() =
            Some(Rc::new(move |resource: &URI| (is_equal(resource, &root) || !permitted).then(no_permissions)));

        let (accepted, prompted) = fixture.validate(&folder).await;
        let requests = fixture.guarded.stat_requests.borrow().clone();

        let expected_requests: Vec<String> = if permitted { vec!["/root", "/root/granted"] } else { vec!["/root", "/root/granted", "/root"] }
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!((accepted, prompted, requests), (permitted, None, expected_requests));
    });
}

#[test]
fn accepts_the_selected_folder_independently_of_denied_parent_access() {
    accepts_or_rejects_the_selected_folder_independently_of_denied_parent_access(true);
}

#[test]
fn rejects_the_selected_folder_independently_of_denied_parent_access() {
    accepts_or_rejects_the_selected_folder_independently_of_denied_parent_access(false);
}

#[test]
fn matches_a_direct_child_synchronously_before_accepting_a_folder() {
    run_with_real_timers(async {
        let fixture = Fixture::new(TestPathService::new(None, None));
        let folder = uri_file("/root/folder");
        fixture.dialog.configure_for_testing(DialogTestState {
            current_folder: Some(uri_file("/root")),
            user_entered_path_segment: Some(String::new()),
            auto_complete_path_segment: Some(String::new()),
            separator: Some("/".to_string()),
            is_windows: Some(false),
            scheme: Some(schemas::FILE.to_string()),
            ..DialogTestState::default()
        });
        fixture.dialog.set_items_for_testing(vec![("folder".to_string(), folder, true)]);
        fixture.pick.set_active_items(Vec::new());
        fixture.pick.set_selected_items(Vec::new());
        fixture.pick.set_value("/root/fol".to_string());

        let value_change = fixture.dialog.handle_value_change(fixture.pick.value());
        fixture.pick.set_selected_items(fixture.pick.active_items());
        fixture.dialog.on_did_accept().await.unwrap();
        value_change.await.unwrap();

        // `insertedValue` é o valor que o `insertText` deixa no campo, e `triedToUpdateItems` é a ausência de qualquer consulta ao
        // serviço de arquivos (o `tryUpdateItems` consulta o `stat`, e o `updateItems` o `resolve`).
        let tried_to_update_items = !fixture.guarded.stat_requests.borrow().is_empty() || !fixture.guarded.resolve_requests.borrow().is_empty();
        assert_eq!((fixture.pick.value(), tried_to_update_items), ("/root/folder".to_string(), false));
    });
}

#[test]
fn updates_items_when_navigating_from_root_into_a_child_folder() {
    run_with_real_timers(async {
        let fixture = Fixture::new(TestPathService::new(None, None));
        let root = in_memory_uri("/");
        let folder = in_memory_uri("/folder/");
        // O `tryUpdateItems` do original captura `{ value, folder }`; aqui a pasta existe de verdade, e a pasta atual depois da
        // mudança de valor é a que o `tryUpdateItems` recebeu.
        fixture.file_service.create_folder(&in_memory_uri("/folder")).await.unwrap();
        fixture.dialog.configure_for_testing(DialogTestState {
            current_folder: Some(root),
            user_entered_path_segment: Some(String::new()),
            auto_complete_path_segment: Some("folder".to_string()),
            separator: Some("/".to_string()),
            is_windows: Some(false),
            scheme: Some(schemas::IN_MEMORY.to_string()),
            ..DialogTestState::default()
        });
        fixture.pick.set_value("/folder/".to_string());

        fixture.dialog.handle_value_change(fixture.pick.value()).await.unwrap();

        assert_eq!((fixture.pick.value(), uri_text(&fixture.dialog.current_folder())), ("/folder/".to_string(), uri_text(&folder)));
    });
}

#[test]
fn uses_authority_specific_windows_semantics_for_scoped_file_systems() {
    run_with_real_timers(async {
        let resource = URI::parse("vscode-agent-host://windows-host/c:/Users/test/project", false);
        let path_service = ScopedPathService::new(Some(OperatingSystem::Windows), Some(Platform::Win32));
        let fixture = Fixture::new(path_service.clone());
        fixture.dialog.configure_for_testing(DialogTestState {
            scheme: Some("vscode-agent-host".to_string()),
            scoped_authority: Some("windows-host".to_string()),
            ..DialogTestState::default()
        });

        fixture.dialog.resolve_path_formatting().await.unwrap();
        let path = fixture.dialog.path_from_uri(&resource, true);
        let round_tripped = fixture.dialog.remote_uri_from(&path, Some(&resource));
        let another_drive = fixture.dialog.remote_uri_from("D:\\code\\project\\", Some(&resource));
        let semantic_requests = path_service.requests.borrow().clone();

        let host = |path: &str| ("vscode-agent-host".to_string(), "windows-host".to_string(), path.to_string());
        assert_eq!(
            (path, parts(&round_tripped), parts(&another_drive), fixture.dialog.separator(), fixture.dialog.is_windows(), semantic_requests),
            (
                "C:\\Users\\test\\project\\".to_string(),
                host("/C:/Users/test/project/"),
                host("/D:/code/project/"),
                "\\".to_string(),
                true,
                vec!["os:vscode-agent-host://windows-host/".to_string(), "path:vscode-agent-host://windows-host/".to_string()],
            )
        );
    });
}

#[test]
fn uses_posix_semantics_for_scoped_file_systems() {
    run_with_real_timers(async {
        let resource = URI::parse("vscode-agent-host://linux-host/Users/test/project", false);
        let fixture = Fixture::new(ScopedPathService::new(Some(OperatingSystem::Linux), Some(Platform::Posix)));
        fixture.dialog.configure_for_testing(DialogTestState {
            scheme: Some("vscode-agent-host".to_string()),
            scoped_authority: Some("linux-host".to_string()),
            ..DialogTestState::default()
        });

        fixture.dialog.resolve_path_formatting().await.unwrap();

        assert_eq!(
            (fixture.dialog.path_from_uri(&resource, true), fixture.dialog.separator(), fixture.dialog.is_windows()),
            ("/Users/test/project/".to_string(), "/".to_string(), false)
        );
    });
}

#[test]
fn falls_back_to_posix_formatting_when_scoped_semantics_are_unknown() {
    run_with_real_timers(async {
        let fixture = Fixture::new(ScopedPathService::new(None, None));
        fixture.dialog.configure_for_testing(DialogTestState {
            scheme: Some("vscode-agent-host".to_string()),
            scoped_authority: Some("unknown-host".to_string()),
            ..DialogTestState::default()
        });

        fixture.dialog.resolve_path_formatting().await.unwrap();

        assert_eq!((fixture.dialog.separator(), fixture.dialog.is_windows()), ("/".to_string(), false));
    });
}

#[test]
fn does_not_let_a_canceled_slow_folder_update_overwrite_a_newer_folder() {
    run_with_real_timers(async {
        let slow_folder = uri_file("/slow");
        let fast_folder = uri_file("/fast");

        let (slow_resolve, resolve_slow) = Promise::<FileStat>::new();

        fn folder_stat(resource: &URI) -> FileStat {
            FileStat {
                resource: resource.clone(),
                name: ext_uri().basename(resource, None),
                size: Some(0),
                mtime: Some(0),
                ctime: Some(0),
                etag: Some(String::new()),
                readonly: Some(false),
                locked: Some(false),
                executable: None,
                is_file: false,
                is_directory: true,
                is_symbolic_link: false,
                children: Some(Vec::new()),
            }
        }

        let fixture = Fixture::new(TestPathService::new(None, None));
        {
            let slow_folder = slow_folder.clone();
            *fixture.guarded.resolve_override.borrow_mut() = Some(Rc::new(move |resource: &URI| {
                if is_equal(resource, &slow_folder) { slow_resolve.clone() } else { Promise::resolved(folder_stat(resource)) }
            }));
        }
        // O `createItems` do original devolve `[]`: sem o item de volta, que o `exists` real acharia.
        fixture.guarded.exists_override.set(Some(false));
        fixture.dialog.configure_for_testing(DialogTestState { current_folder: Some(uri_file("/")), ..DialogTestState::default() });

        let slow_update = fixture.dialog.update_items(slow_folder.clone(), true, None).catch(|_| false);
        fixture.dialog.update_items(fast_folder.clone(), true, None).await.unwrap();

        let expected_folder = ext_uri().add_trailing_path_separator(&fast_folder, None);
        assert_eq!(uri_text(&fixture.dialog.current_folder()), uri_text(&expected_folder));
        assert_eq!(fixture.pick.value(), "/fast/");

        resolve_slow.resolve(folder_stat(&slow_folder));
        slow_update.await.unwrap();

        assert_eq!(uri_text(&fixture.dialog.current_folder()), uri_text(&expected_folder));
        assert_eq!(fixture.pick.value(), "/fast/");
    });
}
