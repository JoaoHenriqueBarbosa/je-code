//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookEditorModel.test.ts` (suíte `NotebookFileWorkingCopyModel`), caso a caso e na mesma ordem.
//!
//! Os 5 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, e o `teardown`
//!   (`disposables.dispose()`) roda no fim dele ([`run_case`], que também é o `setup`: o `DisposableStore` e o
//!   `setupInstantiationService`). Os casos `async` rodam em `run_with_faked_timers`.
//! - O `TestConfigurationService` do escopo da suíte é criado por caso: o original o compartilha, mas só o caso do limite de
//!   saída (`outputBackupSizeLimit`) e o do delegate de salvamento (`remoteSaving`) o alteram, e nenhum lê o que o outro pôs.
//!   O `telemetryService` (`publicLogError2() { }`) é o `NullTelemetryService`, e o `logservice` (`mock<ILogService>()`, que o
//!   original passa no lugar do `INotebookLoggingService`) é o `NullNotebookLoggingService`.
//! - `instantiationService.createInstance(NotebookTextModel, ...)` é [`create_notebook`].
//! - `new class extends mock<INotebookSerializer>() { override options; override notebookToData }` é o [`TestSerializer`] (os
//!   membros ausentes lançam `TypeError`); `callCount` é um `Cell` compartilhado, e as asserções dentro de `notebookToData`
//!   reprovam o `snapshot` (o pânico vira promessa rejeitada na `async_fn`).
//! - `mockNotebookService(notebook, serializer, configurationService)` é o [`MockNotebookService`]: `withNotebookDataProvider`,
//!   `tryGetDataProviderSync` e `createNotebookTextDocumentSnapshot` respondem, e os demais membros lançam `TypeError`. O
//!   `outputSizeLimit` do `createNotebookTextDocumentSnapshot` do mock é o `getValue<number>(...) ?? 1024` sem multiplicar por
//!   1024; como o `output_size_limit` do `INotebookSnapshotOptions` do porte é inteiro, o limite fracionário do caso
//!   (`100 / 1024`) entra arredondado para cima (1). A saída do caso tem 101 bytes, e `101 > 100 / 1024` e `101 > 1` dão o
//!   mesmo resultado (o `snapshot` de backup lança).
//! - `{ name: 'savedFile' } as IFileStatWithMetadata` é um `FileStatWithMetadata` completo com `name = "savedFile"` (o porte
//!   exige todos os metadados); `{} as IFileStatWithMetadata` e `{} as CancellationToken` são `WriteFileOptions::default()` e
//!   `CancellationToken::None`.
//! - `try { await model.snapshot(Backup); assert.fail(...) } catch (e) { assert.notEqual(e.code, 'ERR_ASSERTION', e.message) }`
//!   é conferir que o `snapshot` de backup rejeita com o erro do modelo (a promessa rejeitada), e não com uma falha de asserção.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::{FileStat, FileStatWithMetadata, WriteFileOptions};
use je_platform::telemetry::null_telemetry_service;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, ICellDto2, INotebookContributionData, INotebookRendererInfo, INotebookSnapshotOptions, INotebookStaticPreloadInfo, IOutputDto, IOutputItemDto,
    NotebookData, NotebookDocumentMetadata, NotebookExtensionDescription, NotebookSetting, TransientOptions,
};
use je_workbench::contrib::notebook::common::notebook_editor_model::NotebookFileWorkingCopyModel;
use je_workbench::contrib::notebook::common::notebook_service::{INotebookSerializer, INotebookService, SimpleNotebookProviderInfo};
use je_workbench::services::working_copy::file_working_copy::{IFileWorkingCopyModel, SnapshotContext};
use je_workbench::services::working_copy::stored_file_working_copy::IStoredFileWorkingCopyModel;
use support::test_notebook_editor::{NotebookTestServices, NullNotebookLoggingService, setup_notebook_services};

fn absent(member: &str) -> ! {
    unreachable!("TypeError: {member} is not a function")
}

/// `new class extends mock<INotebookSerializer>() { override options; override notebookToData; override save }`.
struct TestSerializer {
    options: TransientOptions,
    notebook_to_data: Option<Box<dyn Fn(NotebookData) -> Promise<VSBuffer>>>,
    save: Option<Box<dyn Fn() -> Promise<FileStatWithMetadata>>>,
}

impl INotebookSerializer for TestSerializer {
    fn options(&self) -> TransientOptions {
        self.options.clone()
    }

    fn data_to_notebook(&self, _data: VSBuffer) -> Promise<NotebookData> {
        absent("dataToNotebook")
    }

    fn notebook_to_data(&self, data: NotebookData) -> Promise<VSBuffer> {
        match &self.notebook_to_data {
            Some(notebook_to_data) => notebook_to_data(data),
            None => absent("notebookToData"),
        }
    }

    fn save(&self, _uri: &URI, _version_id: i64, _options: WriteFileOptions, _token: CancellationToken) -> Promise<FileStatWithMetadata> {
        match &self.save {
            Some(save) => save(),
            None => absent("save"),
        }
    }
}

/// `mockNotebookService(notebook, notebookSerializer, configurationService)`.
struct MockNotebookService {
    this: Weak<MockNotebookService>,
    notebook: Rc<NotebookTextModel>,
    notebook_serializer: Promise<Rc<dyn INotebookSerializer>>,
    serializer: RefCell<Option<Rc<dyn INotebookSerializer>>>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl MockNotebookService {
    fn new(notebook: &Rc<NotebookTextModel>, notebook_serializer: Promise<Rc<dyn INotebookSerializer>>, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), notebook: notebook.clone(), notebook_serializer, serializer: RefCell::new(None), configuration_service })
    }

    fn provider_info(&self, serializer: Rc<dyn INotebookSerializer>) -> Rc<SimpleNotebookProviderInfo> {
        Rc::new(SimpleNotebookProviderInfo::new(
            &self.notebook.view_type(),
            serializer,
            NotebookExtensionDescription { id: ExtensionIdentifier::new("test"), location: None },
        ))
    }
}

impl INotebookService for MockNotebookService {
    fn can_resolve(&self, _view_type: &str) -> Promise<bool> {
        absent("canResolve")
    }
    fn on_add_view_type(&self) -> Event<String> {
        absent("onAddViewType")
    }
    fn on_will_remove_view_type(&self) -> Event<String> {
        absent("onWillRemoveViewType")
    }
    fn on_did_change_output_renderers(&self) -> Event<()> {
        absent("onDidChangeOutputRenderers")
    }
    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillAddNotebookDocument")
    }
    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidAddNotebookDocument")
    }
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillRemoveNotebookDocument")
    }
    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidRemoveNotebookDocument")
    }
    fn register_notebook_serializer(&self, _view_type: &str, _extension_data: NotebookExtensionDescription, _serializer: Rc<dyn INotebookSerializer>) -> DisposableRef {
        absent("registerNotebookSerializer")
    }
    fn with_notebook_data_provider(&self, _view_type: &str) -> Promise<Rc<SimpleNotebookProviderInfo>> {
        let this = self.this.upgrade().expect("MockNotebookService");
        async_fn(async move {
            let serializer = this.notebook_serializer.clone().await?;
            *this.serializer.borrow_mut() = Some(serializer.clone());
            Ok(this.provider_info(serializer))
        })
    }
    fn try_get_data_provider_sync(&self, _view_type: &str) -> Option<Rc<SimpleNotebookProviderInfo>> {
        let serializer = self.serializer.borrow().clone()?;
        Some(self.provider_info(serializer))
    }
    fn get_view_type_provider(&self, _view_type: &str) -> Option<String> {
        absent("getViewTypeProvider")
    }
    fn get_renderer_info(&self, _id: &str) -> Option<Rc<dyn INotebookRendererInfo>> {
        absent("getRendererInfo")
    }
    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>> {
        absent("getRenderers")
    }
    fn get_static_preloads(&self, _view_type: &str) -> Vec<INotebookStaticPreloadInfo> {
        absent("getStaticPreloads")
    }
    fn create_notebook_text_model(&self, _view_type: &str, _uri: &URI, _stream: Option<VSBufferReadableStream>) -> Promise<Rc<NotebookTextModel>> {
        absent("createNotebookTextModel")
    }
    fn get_notebook_text_model(&self, _uri: &URI) -> Option<Rc<NotebookTextModel>> {
        absent("getNotebookTextModel")
    }
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("getNotebookTextModels")
    }
    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("listNotebookDocuments")
    }
    fn create_notebook_text_document_snapshot(&self, _uri: &URI, context: SnapshotContext, _token: CancellationToken) -> Promise<VSBufferReadableStream> {
        let this = self.this.upgrade().expect("MockNotebookService");
        async_fn(async move {
            let info = this.with_notebook_data_provider(&this.notebook.view_type()).await?;
            let serializer = info.serializer.clone();
            let output_size_limit = match this.configuration_service.get_value(Some(NotebookSetting::OUTPUT_BACKUP_SIZE_LIMIT), None) {
                Dynamic::Number(limit) => limit,
                _ => 1024.0,
            };
            let data = this.notebook.create_snapshot(&INotebookSnapshotOptions {
                context,
                output_size_limit: output_size_limit.ceil() as u64,
                transient_options: Some(serializer.options()),
            });
            let bytes = serializer.notebook_to_data(data).await?;

            let stream: VSBufferReadableStream = buffer_to_stream(bytes);
            Ok(stream)
        })
    }
    fn register_contributed_notebook_type(&self, _view_type: &str, _data: INotebookContributionData) -> DisposableRef {
        absent("registerContributedNotebookType")
    }
    fn has_supported_notebooks(&self, _resource: &URI) -> bool {
        absent("hasSupportedNotebooks")
    }
    fn get_notebook_provider_resource_roots(&self) -> Vec<URI> {
        absent("getNotebookProviderResourceRoots")
    }
    fn clear_editor_cache(&self) {
        absent("clearEditorCache")
    }
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o corpo `async` e o `teardown` de um caso.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>, Rc<NotebookTestServices>) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let services = Rc::new(setup_notebook_services(&disposables));
    let run_disposables = disposables.clone();
    run_with_faked_timers(async move {
        body(run_disposables, services).await;
    });
    disposables.dispose();
    drop(guard);
}

/// `instantiationService.createInstance(NotebookTextModel, 'notebook', URI.file('test'), cells, metadata, transientOptions)`.
fn create_notebook(services: &NotebookTestServices, cells: Vec<ICellDto2>, metadata: NotebookDocumentMetadata) -> Rc<NotebookTextModel> {
    NotebookTextModel::new(
        "notebook",
        URI::file("test", Platform::Posix),
        cells,
        metadata,
        TransientOptions::default(),
        services.undo_redo_service.clone(),
        services.model_service.clone(),
        services.language_service(),
        services.language_detection_service(),
        services.notebook_execution_state_service(),
        services.notebook_logging_service(),
    )
}

/// `{ cellKind: CellKind.Code, language: 'foo', mime: 'foo', source: 'foo', outputs, metadata }`.
fn foo_cell(outputs: Vec<IOutputDto>, metadata: Option<Dynamic>) -> ICellDto2 {
    ICellDto2 {
        source: JsString::from("foo"),
        language: "foo".to_owned(),
        mime: Some("foo".to_owned()),
        cell_kind: CellKind::Code,
        outputs,
        metadata,
        internal_metadata: None,
        collapse_state: None,
    }
}

fn empty_record() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// `{ foo: 123, bar: 456 }`.
fn foo_bar_metadata() -> Dynamic {
    Dynamic::object([("foo", Dynamic::Number(123.0)), ("bar", Dynamic::Number(456.0))])
}

/// `{ transientOutputs, transientCellMetadata, transientDocumentMetadata, cellContentMetadata: {} }`.
fn transient_options(transient_outputs: bool, transient_cell_metadata: &[&str], transient_document_metadata: &[&str]) -> TransientOptions {
    let flags = |keys: &[&str]| keys.iter().map(|key| ((*key).to_owned(), true)).collect();
    TransientOptions {
        transient_outputs,
        transient_cell_metadata: flags(transient_cell_metadata),
        transient_document_metadata: flags(transient_document_metadata),
        cell_content_metadata: Default::default(),
    }
}

/// O `TestConfigurationService` e o `Emitter` dele (que o `LeakGuard` rastreia) no `DisposableStore` do caso.
fn new_configuration_service(disposables: &DisposableStore) -> Rc<TestConfigurationService> {
    let configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    configuration_service
}

/// Um serializador que só confere o que o `notebookToData` recebe, contando as chamadas.
fn counting_serializer(options: TransientOptions, check: impl Fn(&NotebookData) + 'static, call_count: &Rc<Cell<u32>>) -> Rc<dyn INotebookSerializer> {
    let call_count = call_count.clone();
    Rc::new(TestSerializer {
        options,
        notebook_to_data: Some(Box::new(move |data| {
            call_count.set(call_count.get() + 1);
            check(&data);
            Promise::resolved(VSBuffer::from_string(""))
        })),
        save: None,
    })
}

fn resolved_serializer(serializer: Rc<dyn INotebookSerializer>) -> Promise<Rc<dyn INotebookSerializer>> {
    Promise::resolved(serializer)
}

/// `disposables.add(new NotebookFileWorkingCopyModel(notebook, mockNotebookService(...), configurationService, telemetryService, logservice))`.
fn create_model(
    disposables: &DisposableStore,
    notebook: &Rc<NotebookTextModel>,
    notebook_service: Rc<dyn INotebookService>,
    configuration_service: Rc<dyn IConfigurationService>,
) -> Rc<NotebookFileWorkingCopyModel> {
    disposables.add_rc(NotebookFileWorkingCopyModel::new(notebook.clone(), notebook_service, configuration_service, null_telemetry_service(), Rc::new(NullNotebookLoggingService)))
}

/// `mockNotebookService(notebook, serializer)` com um `TestConfigurationService` novo.
fn mock_notebook_service(disposables: &DisposableStore, notebook: &Rc<NotebookTextModel>, serializer: Promise<Rc<dyn INotebookSerializer>>) -> Rc<dyn INotebookService> {
    let configuration_service: Rc<dyn IConfigurationService> = new_configuration_service(disposables);
    MockNotebookService::new(notebook, serializer, configuration_service)
}

/// `await model.snapshot(SnapshotContext.Save, CancellationToken.None)`.
async fn snapshot_save(model: &Rc<NotebookFileWorkingCopyModel>) {
    model.snapshot(SnapshotContext::Save, CancellationToken::None).await.expect("snapshot");
}

#[test]
fn no_transient_output_is_send_to_serializer() {
    run_case(|disposables, services| async move {
        let output = IOutputDto {
            output_id: "id".to_owned(),
            outputs: vec![IOutputItemDto { mime: mimes::TEXT.to_owned(), data: VSBuffer::from_string("Hello Out") }],
            metadata: None,
        };
        let notebook = create_notebook(&services, vec![foo_cell(vec![output], None)], empty_record());
        let configuration_service: Rc<dyn IConfigurationService> = new_configuration_service(&disposables);

        {
            // transient output
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(true, &[], &[]),
                |data| {
                    assert_eq!(data.cells.len(), 1);
                    assert_eq!(data.cells[0].outputs.len(), 0);
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());

            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }

        {
            // NOT transient output
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(false, &[], &[]),
                |data| {
                    assert_eq!(data.cells.len(), 1);
                    assert_eq!(data.cells[0].outputs.len(), 1);
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());
            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }
    });
}

#[test]
fn no_transient_metadata_is_send_to_serializer() {
    run_case(|disposables, services| async move {
        let notebook = create_notebook(&services, vec![foo_cell(Vec::new(), None)], foo_bar_metadata());
        disposables.add_rc(notebook.clone());
        let configuration_service: Rc<dyn IConfigurationService> = new_configuration_service(&disposables);

        {
            // transient
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(true, &[], &["bar"]),
                |data| {
                    assert!(data.metadata.get_str("foo").strict_equals(&Dynamic::Number(123.0)));
                    assert!(data.metadata.get_str("bar").is_undefined());
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());

            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }

        {
            // NOT transient
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(false, &[], &[]),
                |data| {
                    assert!(data.metadata.get_str("foo").strict_equals(&Dynamic::Number(123.0)));
                    assert!(data.metadata.get_str("bar").strict_equals(&Dynamic::Number(456.0)));
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());
            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }
    });
}

#[test]
fn no_transient_cell_metadata_is_send_to_serializer() {
    run_case(|disposables, services| async move {
        let notebook = create_notebook(&services, vec![foo_cell(Vec::new(), Some(foo_bar_metadata()))], empty_record());
        disposables.add_rc(notebook.clone());
        let configuration_service: Rc<dyn IConfigurationService> = new_configuration_service(&disposables);

        {
            // transient
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(true, &["bar"], &[]),
                |data| {
                    let metadata = data.cells[0].metadata.as_ref().expect("metadata");
                    assert!(metadata.get_str("foo").strict_equals(&Dynamic::Number(123.0)));
                    assert!(metadata.get_str("bar").is_undefined());
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());

            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }

        {
            // NOT transient
            let call_count = Rc::new(Cell::new(0));
            let serializer = counting_serializer(
                transient_options(false, &[], &[]),
                |data| {
                    let metadata = data.cells[0].metadata.as_ref().expect("metadata");
                    assert!(metadata.get_str("foo").strict_equals(&Dynamic::Number(123.0)));
                    assert!(metadata.get_str("bar").strict_equals(&Dynamic::Number(456.0)));
                },
                &call_count,
            );
            let model = create_model(&disposables, &notebook, mock_notebook_service(&disposables, &notebook, resolved_serializer(serializer)), configuration_service.clone());
            snapshot_save(&model).await;
            assert_eq!(call_count.get(), 1);
        }
    });
}

#[test]
fn notebooks_with_outputs_beyond_the_size_threshold_will_throw_for_backup_snapshots() {
    run_case(|disposables, services| async move {
        let output_limit = 100_usize;
        let configuration_service = new_configuration_service(&disposables);
        configuration_service
            .set_user_configuration(NotebookSetting::OUTPUT_BACKUP_SIZE_LIMIT, Dynamic::Number(output_limit as f64 * 1.0 / 1024.0), None)
            .await
            .expect("setUserConfiguration");
        let large_output = IOutputDto {
            output_id: "123".to_owned(),
            outputs: vec![IOutputItemDto { mime: mimes::TEXT.to_owned(), data: VSBuffer::from_string(&"a".repeat(output_limit + 1)) }],
            metadata: None,
        };
        let notebook = create_notebook(&services, vec![foo_cell(vec![large_output], Some(foo_bar_metadata()))], empty_record());
        disposables.add_rc(notebook.clone());

        let call_count = Rc::new(Cell::new(0));
        let serializer = counting_serializer(
            transient_options(true, &["bar"], &[]),
            |data| {
                let metadata = data.cells[0].metadata.as_ref().expect("metadata");
                assert!(metadata.get_str("foo").strict_equals(&Dynamic::Number(123.0)));
                assert!(metadata.get_str("bar").is_undefined());
            },
            &call_count,
        );
        let configuration_service_for_mock: Rc<dyn IConfigurationService> = configuration_service.clone();
        let notebook_service: Rc<dyn INotebookService> = MockNotebookService::new(&notebook, resolved_serializer(serializer), configuration_service_for_mock);
        let model = create_model(&disposables, &notebook, notebook_service, configuration_service);

        let backup = model.snapshot(SnapshotContext::Backup, CancellationToken::None).await;
        // `assert.fail` de dentro do `try` cairia no `catch`; o que o caso confere é que a rejeição não é uma falha de asserção.
        let error = backup.err().expect("Expected snapshot to throw an error for large output");
        assert_eq!(error.message, "Notebook too large to backup");

        snapshot_save(&model).await;
        assert_eq!(call_count.get(), 1);
    });
}

#[test]
fn notebook_model_will_not_return_a_save_delegate_if_the_serializer_has_not_been_retreived() {
    run_case(|disposables, services| async move {
        let notebook = create_notebook(&services, vec![foo_cell(Vec::new(), Some(foo_bar_metadata()))], empty_record());
        disposables.add_rc(notebook.clone());

        let serializer: Rc<dyn INotebookSerializer> = Rc::new(TestSerializer {
            options: TransientOptions::default(),
            notebook_to_data: None,
            save: Some(Box::new(|| {
                Promise::resolved(FileStatWithMetadata::new(FileStat {
                    resource: URI::file("savedFile", Platform::Posix),
                    name: "savedFile".to_owned(),
                    size: Some(0),
                    mtime: Some(0),
                    ctime: Some(0),
                    etag: Some(String::new()),
                    readonly: Some(false),
                    locked: Some(false),
                    executable: Some(false),
                    is_file: true,
                    is_directory: false,
                    is_symbolic_link: false,
                    children: None,
                }))
            })),
        });

        let (serializer_promise, resolve_serializer) = Promise::<Rc<dyn INotebookSerializer>>::new();
        let notebook_service = mock_notebook_service(&disposables, &notebook, serializer_promise);
        let configuration_service = new_configuration_service(&disposables);
        configuration_service.set_user_configuration(NotebookSetting::REMOTE_SAVING, Dynamic::Bool(true), None).await.expect("setUserConfiguration");

        let model = create_model(&disposables, &notebook, notebook_service, configuration_service);

        // the save method should not be set if the serializer is not yet resolved
        let not_exist = model.save(WriteFileOptions::default(), CancellationToken::None);
        assert!(not_exist.is_none());

        resolve_serializer.resolve(serializer);
        model.get_notebook_serializer().await.expect("getNotebookSerializer");
        let result = model.save(WriteFileOptions::default(), CancellationToken::None).expect("save").await.expect("save");

        assert_eq!(result.name, "savedFile");
    });
}
