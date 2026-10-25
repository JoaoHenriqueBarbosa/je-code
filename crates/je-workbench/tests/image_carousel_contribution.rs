//! Porte de `src/vs/workbench/contrib/imageCarousel/test/browser/imageCarousel.contribution.test.ts` (suíte
//! `OpenImagesInCarouselFromExplorerAction`), caso a caso e na mesma ordem (11 casos).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso. O `import` da contribuição
//!   é [`register_image_carousel_contribution`], chamado antes do guarda.
//! - **`workbenchInstantiationService(undefined, disposables)`.** É o [`TestInstantiationService`] com os cinco serviços que a ação
//!   lê: [`FakeFileService`] (o `IFileService` do workbench, com `resolve` e `readFile` sobrescritos por `stubFileService`),
//!   [`FakeExplorerService`], [`RecordingEditorService`], [`RecordingNotificationService`] e o `TestContextService`. O serviço de
//!   notificação grava sempre (no original só os casos que chamam `stubNotificationService` gravam, e os demais usam o do
//!   workbench, que nenhum deles consulta).
//! - **`createFileStat(uri, false, false, true, false, children)`.** É `create_file_stat` com `FileStatOptions` e os filhos como
//!   `FileStatChild`.
//! - **`openEditor(input, _options, group)`.** O `IEditorService` do porte abre entrada tipada por `open_editors`; o dublê guarda
//!   a coleção e o `startIndex` e registra a entrada no guarda (`disposables.add(input)`). O `group` não é conferido no original.
//! - **`CommandsRegistry.getCommand(...).handler` e `await invokeFunction`.** O handler da ação descarta a promessa do corpo; o
//!   `await` é `run_microtasks`, e todas as promessas dos dublês já nascem resolvidas.
//! - **`readFile` que lança.** O `read_file` do dublê conta a chamada e rejeita quando não há conteúdo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_platform::commands::commands::commands_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{
    CreateFileOptions, FileChangesEvent, FileContent, FileOperationEvent, FileStat, FileStatWithMetadata, FileStreamContent, FileSystemProviderActivationEvent,
    FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef, FileSystemProviderRegistrationEvent, PartialFileDeleteOptions,
    ReadFileOptions, ResolveFileOptions, WatchOptions, WriteFileOptions,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::notification::{
    I_NOTIFICATION_SERVICE, INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
    NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::resources::{join_path, uri_file};
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::contrib::files::browser_files::{EditableItem, ExplorerBulkEditOptions, ExplorerReveal, I_EXPLORER_SERVICE, IExplorerService};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::contrib::image_carousel::browser::image_carousel_contribution::{OPEN_IMAGES_IN_CAROUSEL_FROM_EXPLORER_ACTION_ID, register_image_carousel_contribution};
use je_workbench::contrib::image_carousel::browser::image_carousel_editor_input::ImageCarouselEditorInput;
use je_workbench::contrib::image_carousel::browser::image_carousel_types::IImageCarouselCollection;
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, I_EDITOR_SERVICE, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::test::common::workbench_test_services::{FileStatChild, FileStatOptions, NullFilesConfigurationService, TestContextService, create_file_stat};
use test_instantiation_service::TestInstantiationService;

fn not_mocked(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

// ---- dublês ----

/// O `IFileService` do workbench com `resolve` e `readFile` trocados por `stubFileService`.
#[derive(Default)]
struct FakeFileService {
    resolve_map: RefCell<HashMap<String, FileStat>>,
    file_contents: RefCell<HashMap<String, VSBuffer>>,
    read_file_calls: Cell<usize>,
}

impl IFileService for FakeFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        not_mocked("onDidChangeFileSystemProviderRegistrations")
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        not_mocked("onDidChangeFileSystemProviderCapabilities")
    }

    fn has_provider(&self, _resource: &URI) -> bool {
        not_mocked("hasProvider")
    }

    fn has_capability(&self, _resource: &URI, _capability: FileSystemProviderCapabilities) -> bool {
        not_mocked("hasCapability")
    }

    fn read_file(&self, resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        self.read_file_calls.set(self.read_file_calls.get() + 1);
        let path = resource.path().to_string_lossy();
        match self.file_contents.borrow().get(&path) {
            Some(value) => Promise::resolved(FileContent { stat: create_file_stat(resource, &FileStatOptions::default()), value: value.clone() }),
            None => Promise::rejected(Error::new(format!("Cannot read: {path}"))),
        }
    }

    fn resolve(&self, resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        let path = resource.path().to_string_lossy();
        match self.resolve_map.borrow().get(&path) {
            Some(stat) => Promise::resolved(stat.clone()),
            None => Promise::rejected(Error::new(format!("File not found: {path}"))),
        }
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        not_mocked("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<WatchOptions>) -> DisposableRef {
        not_mocked("watch")
    }

    fn read_file_stream(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        not_mocked("readFileStream")
    }

    fn write_file(&self, _resource: &URI, _content: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("writeFile")
    }

    fn del(&self, _resource: &URI, _options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        not_mocked("del")
    }

    fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        not_mocked("onDidRunOperation")
    }

    fn create_file(&self, _resource: &URI, _content: Option<FileContentSource>, _options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        not_mocked("createFile")
    }

    fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        not_mocked("onWillActivateFileSystemProvider")
    }

    fn register_provider(&self, _scheme: &str, _provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        not_mocked("registerProvider")
    }
}

/// `instantiationService.stub(IExplorerService, { getContext: () => items })`.
#[derive(Default)]
struct FakeExplorerService {
    items: RefCell<Vec<Rc<ExplorerItem>>>,
}

impl IExplorerService for FakeExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        not_mocked("roots")
    }

    fn get_context(&self, _respect_multi_selection: bool, _ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        self.items.borrow().clone()
    }

    fn get_editable(&self) -> Option<EditableItem> {
        not_mocked("getEditable")
    }

    fn find_closest(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        not_mocked("findClosest")
    }

    fn set_to_copy(&self, _stats: Vec<Rc<ExplorerItem>>, _cut: bool) -> Promise<()> {
        not_mocked("setToCopy")
    }

    fn apply_bulk_edit(&self, _edit: Vec<ResourceFileEdit>, _options: ExplorerBulkEditOptions) -> Promise<()> {
        not_mocked("applyBulkEdit")
    }

    fn select(&self, _resource: &URI, _reveal: Option<ExplorerReveal>) -> Promise<()> {
        not_mocked("select")
    }
}

/// O que `openedInputs` guarda de cada `ImageCarouselEditorInput` aberto.
struct OpenedInput {
    collection: IImageCarouselCollection,
    start_index: f64,
}

/// `stub(IEditorService, 'openEditor', ...)`.
struct RecordingEditorService {
    opened_inputs: Rc<RefCell<Vec<OpenedInput>>>,
    disposables: Rc<DisposableStore>,
}

impl IEditorService for RecordingEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        not_mocked("visibleEditorPanes")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        not_mocked("activeEditor")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        not_mocked("activeTextEditorControl")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        not_mocked("activeTextEditorLanguageId")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_mocked("openEditor")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        not_mocked("findEditors")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_mocked("save")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_mocked("saveAll")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        not_mocked("getEditors")
    }

    fn open_editors(&self, editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        for EditorInputWithOptions { editor, .. } in editors {
            if let Some(input) = editor.as_any().downcast_ref::<ImageCarouselEditorInput>() {
                self.opened_inputs.borrow_mut().push(OpenedInput { collection: input.collection.clone(), start_index: input.start_index });
                let editor: EditorInputRef = editor.clone();
                self.disposables.add(to_disposable(move || editor.dispose()));
            }
        }
        Promise::resolved(Vec::new())
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        not_mocked("isVisible")
    }
}

struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

/// `stub(INotificationService, 'info' | 'error', ...)`.
struct RecordingNotificationService {
    info_messages: Rc<RefCell<Vec<String>>>,
    error_messages: Rc<RefCell<Vec<String>>>,
}

fn message_text(message: &[NotificationMessage]) -> String {
    match message.first() {
        Some(NotificationMessage::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

impl INotificationService for RecordingNotificationService {
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

    fn info(&self, message: Vec<NotificationMessage>) {
        self.info_messages.borrow_mut().push(message_text(&message));
    }

    fn warn(&self, _message: Vec<NotificationMessage>) {}

    fn error(&self, message: Vec<NotificationMessage>) {
        self.error_messages.borrow_mut().push(message_text(&message));
    }

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}

// ---- suíte ----

struct Suite {
    _guard: LeakGuard,
    instantiation_service: Rc<TestInstantiationService>,
    config_service: Rc<TestConfigurationService>,
    file_service: Rc<FakeFileService>,
    explorer_service: Rc<FakeExplorerService>,
    opened_inputs: Rc<RefCell<Vec<OpenedInput>>>,
    info_messages: Rc<RefCell<Vec<String>>>,
    error_messages: Rc<RefCell<Vec<String>>>,
}

impl Suite {
    /// O `setup`.
    fn new() -> Self {
        register_image_carousel_contribution();
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let instantiation_service = guard.add_rc(TestInstantiationService::new());
        let file_service = Rc::new(FakeFileService::default());
        let explorer_service = Rc::new(FakeExplorerService::default());
        let info_messages: Rc<RefCell<Vec<String>>> = Rc::default();
        let error_messages: Rc<RefCell<Vec<String>>> = Rc::default();
        instantiation_service.stub(I_FILE_SERVICE, file_service.clone());
        instantiation_service.stub(I_EXPLORER_SERVICE, explorer_service.clone());
        instantiation_service.stub(I_WORKSPACE_CONTEXT_SERVICE, TestContextService::new(None, None));
        instantiation_service
            .stub(I_NOTIFICATION_SERVICE, Rc::new(RecordingNotificationService { info_messages: info_messages.clone(), error_messages: error_messages.clone() }));
        Self {
            _guard: guard,
            instantiation_service,
            config_service: TestConfigurationService::new(None),
            file_service,
            explorer_service,
            opened_inputs: Rc::default(),
            info_messages,
            error_messages,
        }
    }

    /// `stubFileService(resolveMap, fileContents)`.
    fn stub_file_service(&self, resolve_map: Vec<(URI, FileStat)>, file_contents: Vec<(URI, VSBuffer)>) {
        *self.file_service.resolve_map.borrow_mut() = resolve_map.into_iter().map(|(uri, stat)| (uri.path().to_string_lossy(), stat)).collect();
        *self.file_service.file_contents.borrow_mut() = file_contents.into_iter().map(|(uri, value)| (uri.path().to_string_lossy(), value)).collect();
    }

    /// `stubExplorerService(items)`.
    fn stub_explorer_service(&self, items: Vec<Rc<ExplorerItem>>) {
        *self.explorer_service.items.borrow_mut() = items;
    }

    /// `stubEditorService()`.
    fn stub_editor_service(&self) {
        let service = RecordingEditorService { opened_inputs: self.opened_inputs.clone(), disposables: self._guard.store() };
        self.instantiation_service.stub(I_EDITOR_SERVICE, Rc::new(service));
    }

    /// `createExplorerItem(path, isFolder, fileService, configService, parent?)`.
    fn explorer_item(&self, path: &str, is_folder: bool, parent: Option<&Rc<ExplorerItem>>) -> Rc<ExplorerItem> {
        let file_service: Rc<dyn IFileService> = self.instantiation_service.get(I_FILE_SERVICE);
        let config_service: Rc<dyn IConfigurationService> = self.config_service.clone();
        ExplorerItem::new(uri_file(path), file_service, config_service, Rc::new(NullFilesConfigurationService), parent, Some(is_folder), None, None, None, None, None, false)
    }

    /// `CommandsRegistry.getCommand('workbench.action.openImagesInCarousel')` e `invokeFunction(command.handler, ...args)`.
    fn run_command(&self, resource: Option<&URI>) {
        let handler = commands_registry().get_command(OPEN_IMAGES_IN_CAROUSEL_FROM_EXPLORER_ACTION_ID).expect("Command should be registered").handler;
        let args: Vec<Dynamic> = resource.map(|resource| vec![resource.to_dynamic()]).unwrap_or_default();
        self.instantiation_service.invoke_function(|accessor| handler(accessor, &args));
        runtime::run_microtasks();
    }

    /// `input.collection.sections[0].images` mapeado para `(name, data presente, uri presente)`.
    fn images(&self) -> Vec<(String, bool, bool)> {
        self.opened_inputs.borrow()[0].collection.sections[0].images.iter().map(|image| (image.name.clone(), image.data.is_some(), image.uri.is_some())).collect()
    }

    fn names(&self) -> Vec<String> {
        self.images().into_iter().map(|(name, ..)| name).collect()
    }
}

fn file(path: &str) -> FileStatChild {
    FileStatChild { resource: uri_file(path), is_file: Some(true), is_directory: None, is_symbolic_link: None, executable: None }
}

fn file_at(resource: URI) -> FileStatChild {
    FileStatChild { resource, is_file: Some(true), is_directory: None, is_symbolic_link: None, executable: None }
}

/// `createFileStat(uri, false, false, true, false, children)`.
fn folder_stat(resource: &URI, children: Vec<FileStatChild>) -> FileStat {
    let options = FileStatOptions {
        readonly: false,
        is_file: Some(false),
        is_directory: Some(true),
        is_symbolic_link: Some(false),
        children: Some(children),
        executable: None,
    };
    create_file_stat(resource, &options).into_stat()
}

fn data(text: &str) -> VSBuffer {
    VSBuffer::from_string(text)
}

#[test]
fn single_image_file_opens_carousel_with_sibling_images() {
    let t = Suite::new();
    let parent = t.explorer_item("/workspace/images", true, None);
    let image_item = t.explorer_item("/workspace/images/photo.png", false, Some(&parent));

    let folder = uri_file("/workspace/images");
    let subfolder = FileStatChild { resource: uri_file("/workspace/images/subfolder"), is_file: Some(false), is_directory: Some(true), is_symbolic_link: None, executable: None };
    let stat = folder_stat(
        &folder,
        vec![file("/workspace/images/photo.png"), file("/workspace/images/other.jpg"), file("/workspace/images/readme.txt"), subfolder],
    );
    t.stub_file_service(
        vec![(folder, stat)],
        vec![
            (uri_file("/workspace/images/photo.png"), data("fake-png")),
            (uri_file("/workspace/images/other.jpg"), data("fake-jpg")),
            (uri_file("/workspace/images/readme.txt"), data("text file")),
        ],
    );
    t.stub_explorer_service(vec![image_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1, "Should open one editor");
    assert_eq!(t.opened_inputs.borrow()[0].collection.sections.len(), 1);
    // Images are sorted by basename: other.jpg before photo.png
    assert_eq!(t.names(), ["other.jpg", "photo.png"], "Should include 2 image siblings (png + jpg), not txt");
    // Start index should be the selected image (photo.png = index 1 after sorting)
    assert_eq!(t.opened_inputs.borrow()[0].start_index, 1.0);
}

#[test]
fn folder_opens_carousel_with_all_contained_images() {
    let t = Suite::new();
    let folder_item = t.explorer_item("/workspace/images", true, None);

    let folder = uri_file("/workspace/images");
    let stat = folder_stat(&folder, vec![file("/workspace/images/anim.gif"), file("/workspace/images/photo.webp"), file("/workspace/images/script.js")]);
    t.stub_file_service(
        vec![(folder, stat)],
        vec![(uri_file("/workspace/images/anim.gif"), data("fake-gif")), (uri_file("/workspace/images/photo.webp"), data("fake-webp"))],
    );
    t.stub_explorer_service(vec![folder_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1);
    assert_eq!(t.names(), ["anim.gif", "photo.webp"], "Should include 2 images (gif + webp), not js");
}

#[test]
fn multiple_selected_images_open_in_carousel() {
    let t = Suite::new();
    let img1 = t.explorer_item("/workspace/a.png", false, None);
    let img2 = t.explorer_item("/workspace/b.svg", false, None);
    let txt_file = t.explorer_item("/workspace/notes.txt", false, None);

    t.stub_file_service(
        Vec::new(),
        vec![(uri_file("/workspace/a.png"), data("fake-png")), (uri_file("/workspace/b.svg"), data("<svg></svg>"))],
    );
    t.stub_explorer_service(vec![img1, img2, txt_file]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1);
    assert_eq!(t.names(), ["a.png", "b.svg"], "Should include only image files");
}

#[test]
fn empty_selection_with_resource_argument_opens_carousel_from_that_folder() {
    let t = Suite::new();
    let folder_uri = uri_file("/workspace/photos");
    let stat = folder_stat(&folder_uri, vec![file("/workspace/photos/sunset.png"), file("/workspace/photos/mountain.jpg"), file("/workspace/photos/notes.txt")]);
    t.stub_file_service(
        vec![(folder_uri.clone(), stat)],
        vec![(uri_file("/workspace/photos/sunset.png"), data("fake-png")), (uri_file("/workspace/photos/mountain.jpg"), data("fake-jpg"))],
    );
    t.stub_explorer_service(Vec::new());
    t.stub_editor_service();

    // Pass the folder URI as the resource argument (as explorer does for empty-space click)
    t.run_command(Some(&folder_uri));

    assert_eq!(t.opened_inputs.borrow().len(), 1, "Should open carousel using resource argument fallback");
    assert_eq!(t.images().len(), 2, "Should include 2 images from the folder");
}

#[test]
fn empty_selection_without_resource_falls_back_to_first_workspace_folder() {
    let t = Suite::new();
    // Derive the workspace root from IWorkspaceContextService so the test works on all platforms.
    let context_service = t.instantiation_service.get(I_WORKSPACE_CONTEXT_SERVICE);
    let ws_root = context_service.get_workspace().folders()[0].uri.clone();
    let logo_uri = join_path(&ws_root, &["logo.png"]);
    let readme_uri = join_path(&ws_root, &["readme.md"]);

    let stat = folder_stat(&ws_root, vec![file_at(logo_uri.clone()), file_at(readme_uri)]);
    t.stub_file_service(vec![(ws_root, stat)], vec![(logo_uri, data("fake-png"))]);
    t.stub_explorer_service(Vec::new());
    t.stub_editor_service();

    // No resource argument: should fall back to workspace root
    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1, "Should open carousel using workspace root fallback");
    assert_eq!(t.names(), ["logo.png"], "Should include image from workspace root");
}

#[test]
fn empty_selection_with_no_images_shows_notification() {
    let t = Suite::new();
    let folder_uri = uri_file("/workspace/docs");
    let stat = folder_stat(&folder_uri, vec![file("/workspace/docs/readme.md")]);
    t.stub_file_service(vec![(folder_uri.clone(), stat)], Vec::new());
    t.stub_explorer_service(Vec::new());
    t.stub_editor_service();

    t.run_command(Some(&folder_uri));

    assert_eq!(t.opened_inputs.borrow().len(), 0, "Should not open carousel when folder has no images");
    assert_eq!(t.info_messages.borrow().len(), 1, "Should show notification");
}

#[test]
fn folder_with_no_images_shows_notification() {
    let t = Suite::new();
    let folder_item = t.explorer_item("/workspace/docs", true, None);

    let folder = uri_file("/workspace/docs");
    let stat = folder_stat(&folder, vec![file("/workspace/docs/readme.md"), file("/workspace/docs/notes.txt")]);
    t.stub_file_service(vec![(folder, stat)], Vec::new());
    t.stub_explorer_service(vec![folder_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 0, "Should not open carousel when folder has no images");
    assert_eq!(t.info_messages.borrow().len(), 1, "Should show notification about no images");
}

#[test]
fn folder_read_error_shows_error_notification() {
    let t = Suite::new();
    let folder_item = t.explorer_item("/workspace/restricted", true, None);

    // resolve throws to simulate a permission error
    t.stub_file_service(Vec::new(), Vec::new());
    t.stub_explorer_service(vec![folder_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 0, "Should not open carousel on folder read error");
    assert_eq!(t.error_messages.borrow().len(), 1, "Should show error notification");
    assert_eq!(t.info_messages.borrow().len(), 0, "Should not show info notification");
}

#[test]
fn images_with_uris_are_passed_lazily_without_reading_file_contents() {
    let t = Suite::new();
    let folder_uri = uri_file("/workspace/broken");
    let stat = folder_stat(&folder_uri, vec![file("/workspace/broken/corrupt.png"), file("/workspace/broken/missing.jpg")]);

    // No file contents: with lazy loading, no readFile should be called at action time
    t.stub_file_service(vec![(folder_uri.clone(), stat)], Vec::new());
    t.stub_explorer_service(Vec::new());
    t.stub_editor_service();

    t.run_command(Some(&folder_uri));

    assert_eq!(t.file_service.read_file_calls.get(), 0, "readFile should not be called during action");
    assert_eq!(t.opened_inputs.borrow().len(), 1, "Should open carousel with lazy image entries");
    let images = t.images();
    assert_eq!(images.len(), 2, "Should include 2 lazy image entries");
    assert!(!images[0].1, "Image data should not be loaded eagerly");
    assert!(images[0].2, "Image should have a URI for lazy loading");
}

#[test]
fn folder_includes_video_files_alongside_images() {
    let t = Suite::new();
    let folder_item = t.explorer_item("/workspace/media", true, None);

    let folder = uri_file("/workspace/media");
    let stat = folder_stat(
        &folder,
        vec![
            file("/workspace/media/clip.mp4"),
            file("/workspace/media/photo.png"),
            file("/workspace/media/demo.webm"),
            file("/workspace/media/intro.mov"),
            file("/workspace/media/readme.txt"),
        ],
    );
    t.stub_file_service(vec![(folder, stat)], Vec::new());
    t.stub_explorer_service(vec![folder_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1);
    assert_eq!(t.names(), ["clip.mp4", "demo.webm", "intro.mov", "photo.png"], "Should include mp4 + webm + mov + png, not txt");
}

#[test]
fn single_video_file_opens_carousel_with_sibling_media() {
    let t = Suite::new();
    let parent = t.explorer_item("/workspace/media", true, None);
    let video_item = t.explorer_item("/workspace/media/clip.mp4", false, Some(&parent));

    let folder = uri_file("/workspace/media");
    let stat = folder_stat(&folder, vec![file("/workspace/media/clip.mp4"), file("/workspace/media/photo.png"), file("/workspace/media/notes.txt")]);
    t.stub_file_service(vec![(folder, stat)], Vec::new());
    t.stub_explorer_service(vec![video_item]);
    t.stub_editor_service();

    t.run_command(None);

    assert_eq!(t.opened_inputs.borrow().len(), 1);
    assert_eq!(t.names(), ["clip.mp4", "photo.png"], "Should include mp4 + png siblings");
    assert_eq!(t.opened_inputs.borrow()[0].start_index, 0.0, "Start index should point to the selected video");
}
