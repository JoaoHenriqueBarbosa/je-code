//! Infraestrutura de teste de `src/vs/workbench/services/workingCopy/test/browser/untitledFileWorkingCopy.test.ts` e de
//! `untitledScratchpadWorkingCopy.test.ts`: o modelo e a fábrica do teste (`TestUntitledFileWorkingCopyModel` e
//! `TestUntitledFileWorkingCopyModelFactory`), o `createWorkingCopy` que as duas suítes repetem e o acessor das suítes dos
//! gerenciadores (`workbenchInstantiationService` mais `TestServiceAccessor`, com os diálogos, o serviço de caminho, o
//! ambiente e as decorações).
//!
//! Divergências:
//! - **Modelo.** `versionId`, `pushedStackElement` e `pushStackElement()` do modelo do original não fazem parte do contrato
//!   `IUntitledFileWorkingCopyModel` e nenhuma suíte os lê, então não existem aqui. O resto é igual: o evento de mudança
//!   com `isInitial` (`updateContents` dispara `isInitial` quando o novo conteúdo é vazio), o `snapshot` que falha depois
//!   do `setThrowOnSnapshot()` e o `update` que lê o stream.
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** O contêiner de injeção não entra: o acessor das suítes
//!   das working copies é o [`TestServiceAccessor`] de `stored_file_working_copy_support` (que cria só os serviços que a
//!   working copy lê, com os mesmos dublês do original). O das suítes dos gerenciadores o compõe (por `Deref`) e acrescenta
//!   os serviços que o `FileWorkingCopyManager` lê, e troca o `IWorkingCopyFileService` pelo `WorkingCopyFileService` real,
//!   que é o `accessor.workingCopyFileService` do original (o dublê do acessor base não tem as operações de arquivo do
//!   contrato).
//! - **`TestDialogService`.** O [`TestDialogService`] guarda o resultado de `setConfirmResult` (usado uma vez, como no
//!   original; sem resultado devolve `{ confirmed: false }`); `prompt`, `input`, `info`, `warn`, `error` e `about` são os do
//!   dublê de `text_file_support` (o mesmo código do original).
//! - **`TestInMemoryFileSystemProvider`.** É o `InMemoryFileSystemProvider` do porte, registrado no `TestFileService` por
//!   [`FileWorkingCopyManagerAccessor::register_providers`] (esquemas `file` e `vscode-remote`) e descartado com o guarda de
//!   vazamentos, o que as duas suítes que o registram esperam.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::stream::WriteableStream;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream, new_writeable_buffer_stream, readable_to_buffer, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::files::files::FileSystemProviderRef;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::resources::ext_uri;
use je_workbench::common::editor::SaveOptions;
use je_workbench::services::working_copy::file_working_copy::{IFileWorkingCopyModel, IFileWorkingCopyModelFactory, SnapshotContext};
use je_workbench::services::working_copy::file_working_copy_manager::{FileWorkingCopyManager, FileWorkingCopyManagerServices};
use je_workbench::services::working_copy::untitled_file_working_copy::{
    IUntitledFileWorkingCopyModel, UntitledFileWorkingCopy, UntitledFileWorkingCopyInitialContents, UntitledFileWorkingCopyModelContentChangedEvent, UntitledFileWorkingCopySaveDelegate,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackupContent};
use je_workbench::services::working_copy::working_copy_file_service::WorkingCopyFileService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestPathService};

use super::stored_file_working_copy_support::{TestServiceAccessor, TestStoredFileWorkingCopyModel, TestStoredFileWorkingCopyModelFactory};
use super::text_file_support::{TestDecorationsService, TestDialogService as BaseTestDialogService, TestFileDialogService};

//#region TestUntitledFileWorkingCopyModel

/// `TestUntitledFileWorkingCopyModel`.
pub struct TestUntitledFileWorkingCopyModel {
    this: Weak<TestUntitledFileWorkingCopyModel>,
    base: Rc<Disposable>,
    on_did_change_untitled_content: Rc<Emitter<UntitledFileWorkingCopyModelContentChangedEvent>>,
    on_will_dispose: Rc<Emitter<()>>,
    resource: URI,
    contents: RefCell<String>,
    throw_on_snapshot: Cell<bool>,
}

impl TestUntitledFileWorkingCopyModel {
    /// `constructor(resource, contents)`.
    pub fn new(resource: URI, contents: String) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_untitled_content: Rc<Emitter<UntitledFileWorkingCopyModelContentChangedEvent>> = Emitter::new();
        let on_will_dispose: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_untitled_content.clone());
        base.register(on_will_dispose.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change_untitled_content,
            on_will_dispose,
            resource,
            contents: RefCell::new(contents),
            throw_on_snapshot: Cell::new(false),
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o modelo está vivo enquanto é usado")
    }

    /// `resource`.
    pub fn resource(&self) -> &URI {
        &self.resource
    }

    /// `contents`.
    pub fn contents(&self) -> String {
        self.contents.borrow().clone()
    }

    /// `fireContentChangeEvent(event)`.
    pub fn fire_content_change_event(&self, event: UntitledFileWorkingCopyModelContentChangedEvent) {
        self.on_did_change_untitled_content.fire(event);
    }

    /// `updateContents(newContents)`.
    pub fn update_contents(&self, new_contents: &str) {
        self.do_update(new_contents);
    }

    /// `setThrowOnSnapshot()`.
    pub fn set_throw_on_snapshot(&self) {
        self.throw_on_snapshot.set(true);
    }

    fn do_update(&self, new_contents: &str) {
        *self.contents.borrow_mut() = new_contents.to_string();

        self.on_did_change_untitled_content.fire(UntitledFileWorkingCopyModelContentChangedEvent { is_initial: new_contents.is_empty() });
    }
}

impl IDisposable for TestUntitledFileWorkingCopyModel {
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        self.base.dispose();
    }
}

impl IFileWorkingCopyModel for TestUntitledFileWorkingCopyModel {
    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_untitled_content.event().map(|_| (), None)
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn snapshot(&self, _context: SnapshotContext, _token: CancellationToken) -> Promise<VSBufferReadableStream> {
        if self.throw_on_snapshot.get() {
            return Promise::rejected(Error::new("Fail"));
        }

        let stream = new_writeable_buffer_stream(None);
        stream.end(Some(VSBuffer::from_string(&self.contents.borrow())));

        let stream: VSBufferReadableStream = stream;
        Promise::resolved(stream)
    }

    fn update(&self, contents: VSBufferReadableStream, _token: CancellationToken) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let buffer = stream_to_buffer(&*contents).await?;
            this.do_update(&buffer.to_string());

            Ok(())
        })
    }
}

impl IUntitledFileWorkingCopyModel for TestUntitledFileWorkingCopyModel {
    fn on_did_change_untitled_content(&self) -> Event<UntitledFileWorkingCopyModelContentChangedEvent> {
        self.on_did_change_untitled_content.event()
    }
}

/// `TestUntitledFileWorkingCopyModelFactory`.
pub struct TestUntitledFileWorkingCopyModelFactory;

impl TestUntitledFileWorkingCopyModelFactory {
    /// `new TestUntitledFileWorkingCopyModelFactory()`.
    pub fn new() -> Rc<Self> {
        Rc::new(Self)
    }
}

impl IFileWorkingCopyModelFactory<TestUntitledFileWorkingCopyModel> for TestUntitledFileWorkingCopyModelFactory {
    fn create_model(&self, resource: &URI, contents: VSBufferReadableStream, _token: CancellationToken) -> Promise<Rc<TestUntitledFileWorkingCopyModel>> {
        let resource = resource.clone();

        async_fn(async move {
            let contents = stream_to_buffer(&*contents).await?.to_string();

            Ok(TestUntitledFileWorkingCopyModel::new(resource, contents))
        })
    }
}

//#endregion

//#region createWorkingCopy

/// O `if (isReadableStream(backup.content)) { consumeStream(...) } else if (backup.content) { consumeReadable(...) }` que os
/// casos de backup repetem: o conteúdo do backup lido como texto (`None` quando não há conteúdo). `consumeStream` e
/// `consumeReadable` com `VSBuffer.concat` são `streamToBuffer` e `readableToBuffer`.
pub fn backup_contents(content: Option<WorkingCopyBackupContent>) -> Promise<Option<String>> {
    async_fn(async move {
        Ok(match content {
            Some(WorkingCopyBackupContent::Stream(stream)) => Some(stream_to_buffer(&*stream).await?.to_string()),
            Some(WorkingCopyBackupContent::Readable(readable)) => Some(readable_to_buffer(&mut **readable.borrow_mut()).to_string()),
            None => None,
        })
    })
}

/// A working copy do teste.
pub type TestUntitledFileWorkingCopy = UntitledFileWorkingCopy<TestUntitledFileWorkingCopyModel>;

/// O `typeId` que as suítes das working copies sem título dão às working copies.
pub const UNTITLED_WORKING_COPY_TYPE_ID: &str = "testUntitledWorkingCopyType";

/// `createWorkingCopy(uri, hasAssociatedFilePath, initialValue)` das duas suítes (a do scratchpad passa `isScratchpad`
/// verdadeiro): `disposables.add(new UntitledFileWorkingCopy(...))` com o delegado que reverte e devolve `true`.
pub fn create_working_copy(
    disposables: &Rc<DisposableStore>,
    accessor: &TestServiceAccessor,
    uri: &URI,
    has_associated_file_path: bool,
    initial_value: &str,
    is_scratchpad: bool,
) -> Rc<TestUntitledFileWorkingCopy> {
    let save_delegate: UntitledFileWorkingCopySaveDelegate<TestUntitledFileWorkingCopyModel> =
        Rc::new(|working_copy: Rc<TestUntitledFileWorkingCopy>, _options: Option<SaveOptions>| {
            async_fn(async move {
                working_copy.revert(None).await?;

                Ok(true)
            })
        });
    let initial_contents =
        (!initial_value.is_empty()).then(|| UntitledFileWorkingCopyInitialContents { value: buffer_to_stream(VSBuffer::from_string(initial_value)), mark_modified: None });

    disposables.add_rc(UntitledFileWorkingCopy::new(
        UNTITLED_WORKING_COPY_TYPE_ID.to_string(),
        uri.clone(),
        ext_uri().basename(uri, None),
        has_associated_file_path,
        is_scratchpad,
        initial_contents,
        TestUntitledFileWorkingCopyModelFactory::new(),
        save_delegate,
        accessor.working_copy_service.clone(),
        accessor.working_copy_backup_service.clone(),
        accessor.log_service.clone(),
    ))
}

/// `URI.from({ scheme: Schemas.untitled, path: 'Untitled-1' })`.
pub fn untitled_resource() -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), path: Some(JsString::from("Untitled-1")), ..UriComponents::default() }, false)
}

/// O `setup` das duas suítes: `workbenchInstantiationService`, o `TestServiceAccessor` e `workingCopy =
/// disposables.add(createWorkingCopy())` (a do scratchpad passa `isScratchpad` verdadeiro).
pub fn setup_working_copy(disposables: &Rc<DisposableStore>, is_scratchpad: bool) -> (TestServiceAccessor, Rc<TestUntitledFileWorkingCopy>) {
    let accessor = TestServiceAccessor::new(disposables);
    let working_copy = create_working_copy(disposables, &accessor, &untitled_resource(), false, "", is_scratchpad);

    (accessor, working_copy)
}

/// Conta os disparos de `event` (o `counter++` dos ouvintes do original).
pub fn count_events<T: 'static>(disposables: &Rc<DisposableStore>, event: Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// `{ isInitial }` do evento de mudança de conteúdo do modelo.
pub fn initial(is_initial: bool) -> UntitledFileWorkingCopyModelContentChangedEvent {
    UntitledFileWorkingCopyModelContentChangedEvent { is_initial }
}

//#endregion

//#region TestServiceAccessor dos gerenciadores

/// `TestDialogService` com o `setConfirmResult`.
pub struct TestDialogService {
    base: BaseTestDialogService,
    confirm_result: Cell<Option<ConfirmationResult>>,
}

impl TestDialogService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: BaseTestDialogService, confirm_result: Cell::new(None) })
    }

    /// `setConfirmResult(result)`.
    pub fn set_confirm_result(&self, result: ConfirmationResult) {
        self.confirm_result.set(Some(result));
    }
}

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        self.base.on_will_show_dialog()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        self.base.on_did_show_dialog()
    }

    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResult> {
        match self.confirm_result.take() {
            Some(confirm_result) => Promise::resolved(confirm_result),
            None => self.base.confirm(confirmation),
        }
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        self.base.prompt(prompt)
    }

    fn input(&self, input: Input) -> Promise<InputResult> {
        self.base.input(input)
    }

    fn info(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.base.info(message, detail)
    }

    fn warn(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.base.warn(message, detail)
    }

    fn error(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.base.error(message, detail)
    }

    fn about(&self) -> Promise<()> {
        self.base.about()
    }
}

/// O gerenciador das suítes.
pub type TestFileWorkingCopyManager = FileWorkingCopyManager<TestStoredFileWorkingCopyModel, TestUntitledFileWorkingCopyModel>;

/// `workbenchInstantiationService(undefined, disposables)` mais `TestServiceAccessor`, com os serviços que o
/// `FileWorkingCopyManager` lê. Os demais serviços (`fileService`, `workingCopyService`, `lifecycleService`, ...) vêm do
/// [`TestServiceAccessor`] por `Deref`.
pub struct FileWorkingCopyManagerAccessor {
    base: TestServiceAccessor,
    /// `workingCopyFileService`: o `WorkingCopyFileService` real (o do [`TestServiceAccessor`] é o dublê).
    pub working_copy_file_service: Rc<WorkingCopyFileService>,
    /// `fileDialogService`.
    pub file_dialog_service: Rc<TestFileDialogService>,
    /// `dialogService`.
    pub dialog_service: Rc<TestDialogService>,
    /// `pathService`.
    pub path_service: Rc<TestPathService>,
    /// `environmentService`.
    pub environment_service: Rc<TestEnvironmentService>,
    /// `decorationsService`.
    pub decorations_service: Rc<TestDecorationsService>,
}

impl Deref for FileWorkingCopyManagerAccessor {
    type Target = TestServiceAccessor;

    fn deref(&self) -> &TestServiceAccessor {
        &self.base
    }
}

impl FileWorkingCopyManagerAccessor {
    /// `workbenchInstantiationService(undefined, disposables)` e `instantiationService.createInstance(TestServiceAccessor)`.
    pub fn new(disposables: &Rc<DisposableStore>) -> Self {
        let base = TestServiceAccessor::new(disposables);

        let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
            "files",
            Dynamic::object([("participants", Dynamic::object([("timeout", Dynamic::from(60000))]))]),
        )])));
        disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

        let working_copy_file_service = disposables.add_rc(WorkingCopyFileService::new(
            base.file_service.clone(),
            base.working_copy_service.clone(),
            base.uri_identity_service.clone(),
            base.log_service.clone(),
            configuration_service,
            base.progress_service.clone(),
        ));
        let path_service = TestPathService::new(None, None);
        let file_dialog_service = TestFileDialogService::new(path_service.clone());

        Self {
            base,
            working_copy_file_service,
            file_dialog_service,
            dialog_service: TestDialogService::new(),
            path_service,
            environment_service: TestEnvironmentService::new(),
            decorations_service: Rc::new(TestDecorationsService),
        }
    }

    /// `accessor.fileService.registerProvider(Schemas.file, new TestInMemoryFileSystemProvider())` e o mesmo para
    /// `Schemas.vscodeRemote`.
    pub fn register_providers(&self, disposables: &Rc<DisposableStore>) {
        for scheme in [schemas::FILE, schemas::VSCODE_REMOTE] {
            let concrete_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
            let provider: FileSystemProviderRef = concrete_provider;
            let registration = self.base.file_service.register_provider(scheme, provider).expect("o dublê aceita qualquer esquema");
            disposables.add(registration);
        }
    }

    /// Os serviços do construtor do `FileWorkingCopyManager`, na ordem do original.
    pub fn services(&self) -> FileWorkingCopyManagerServices {
        let mut services = self.base.services();
        services.working_copy_file_service = self.working_copy_file_service.clone();

        FileWorkingCopyManagerServices {
            services,
            lifecycle_service: self.base.lifecycle_service.clone(),
            label_service: self.base.label_service.clone(),
            uri_identity_service: self.base.uri_identity_service.clone(),
            file_dialog_service: self.file_dialog_service.clone(),
            path_service: self.path_service.clone(),
            environment_service: self.environment_service.clone(),
            dialog_service: self.dialog_service.clone(),
            decorations_service: self.decorations_service.clone(),
        }
    }

    /// `new FileWorkingCopyManager(typeId, new TestStoredFileWorkingCopyModelFactory(), new
    /// TestUntitledFileWorkingCopyModelFactory(), ...services)`.
    pub fn new_manager(&self, type_id: &str) -> Rc<TestFileWorkingCopyManager> {
        FileWorkingCopyManager::new(type_id.to_string(), TestStoredFileWorkingCopyModelFactory::new(), TestUntitledFileWorkingCopyModelFactory::new(), self.services())
    }
}

//#endregion
