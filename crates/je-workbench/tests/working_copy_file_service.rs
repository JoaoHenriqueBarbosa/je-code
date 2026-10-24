//! Porte de `src/vs/workbench/services/workingCopy/test/browser/workingCopyFileService.test.ts` (suíte `WorkingCopyFileService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`disposables.clear()`) é o descarte do store do guarda. Os casos `async` rodam em `run_with_faked_timers`; `await` é
//!   `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** São o [`TextFileTestAccessor`] de
//!   `tests/support/text_file_support.rs`, criado por `with_working_copy_file_service` com o `WorkingCopyFileService` REAL
//!   (o `accessor.workingCopyFileService` do original, que o gerenciador de modelos e o serviço de arquivos de texto também
//!   leem) no lugar do dublê do acessor de `stored_file_working_copy_support`. A configuração do serviço leva o
//!   `files.participants.timeout` que o `workbenchInstantiationService` define (`60000`). `disposables.add(<TextFileEditorModelManager>
//!   accessor.textFileService.files)` é `add_rc` do gerenciador.
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`]; `(<ITestTextFileEditorModelManager>accessor.textFileService.files).add(...)` é
//!   [`TextFileTestAccessor::files`] (o gerenciador concreto, que tem `add`).
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título completo do caso
//!   (a suíte mais o título do caso) é o argumento de [`to_resource`].
//! - **Participantes e ouvintes.** Os objetos literais `{ participate: async (files, operation) => { ... } }` são
//!   [`participant`] (com todos os argumentos) e [`sync_participant`] (só `files` e `operation`, que devolve a promessa já
//!   cumprida). `assert.strictEqual(a.toString(), b.toString())` é [`uri_string`] nos dois lados.
//! - **`testMoveOrCopy`: `options: { overwrite: true }`.** O `.map(model => ({ file: {...}, options: { overwrite: true } }))`
//!   do original passa uma chave `options` que o `IMoveOperation` não lê (o `overwrite` fica indefinido); o porte mantém
//!   `overwrite: None` nesse caso e `overwrite: Some(true)` só em `testEventsMoveOrCopy`, que o declara de fato.
//! - **`let correlationId: number | undefined`** é um `Rc<Cell<Option<usize>>>`.
//! - **`cancellation of participants`.** `cts.cancel()` depois de chamar a operação e antes de esperá-la é
//!   [`assert_participants_see_cancellation`], que roda os cinco blocos do original com a mesma sequência.
//! - **`model1 === model2`** é a igualdade de endereço dos dois `Rc` ([`same_model`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model::TextSource;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::FileOperation;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::services::textfile::collaborators::{
    CreateFileOperation, CreateOperation, DeleteOperation, FileOperationUndoRedoInfo, IWorkingCopyFileOperationParticipant, IWorkingCopyFileService, MoveOperation,
    RequiredSourceTargetPair, WorkingCopyFileEvent, WorkingCopyFileEventFile,
};
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::textfiles::{EncodedContent, ITextFileEditorModel, ITextFileService};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::services::working_copy::working_copy_file_service::WorkingCopyFileService;
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use je_workbench::test::common::workbench_test_services::TestWorkingCopy;
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "WorkingCopyFileService";

/// O `setup`: `workbenchInstantiationService(undefined, disposables)`, o `TestServiceAccessor` e o gerenciador de modelos
/// como descartável.
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let accessor = TextFileTestAccessor::with_working_copy_file_service(&disposables.store(), |base| -> Rc<dyn IWorkingCopyFileService> {
        let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
            "files",
            Dynamic::object([("participants", Dynamic::object([("timeout", Dynamic::from(60000))]))]),
        )])));
        disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

        disposables.add_rc(WorkingCopyFileService::new(
            base.file_service.clone(),
            base.working_copy_service.clone(),
            base.uri_identity_service.clone(),
            base.log_service.clone(),
            configuration_service,
            base.progress_service.clone(),
        ))
    });

    disposables.add_rc(accessor.files());

    accessor
}

/// `accessor.workingCopyFileService`.
fn working_copy_file_service(accessor: &TextFileTestAccessor) -> Rc<dyn IWorkingCopyFileService> {
    accessor.working_copy_file_service_contract.clone()
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `model.textEditorModel!.setValue(value)`.
fn set_value(model: &dyn ITextFileEditorModel, value: &str) {
    model.text_editor_model().expect("o modelo de texto existe").set_value(TextSource::from(value));
}

/// `model.textEditorModel!.getValue()`.
fn editor_value(model: &dyn ITextFileEditorModel) -> String {
    model.text_editor_model().expect("o modelo de texto existe").get_value().to_string_lossy()
}

/// O objeto literal `{ participate: async (files, operation, info, t, token) => { ... } }`.
struct FnParticipant<F>(F);

impl<F> IWorkingCopyFileOperationParticipant for FnParticipant<F>
where
    F: Fn(Vec<WorkingCopyFileEventFile>, FileOperation, Option<FileOperationUndoRedoInfo>, f64, CancellationToken) -> Promise<()>,
{
    fn participate(&self, files: Vec<WorkingCopyFileEventFile>, operation: FileOperation, undo_info: Option<FileOperationUndoRedoInfo>, timeout: f64, token: CancellationToken) -> Promise<()> {
        (self.0)(files, operation, undo_info, timeout, token)
    }
}

/// `{ participate: async (files, operation, info, t, token) => { ... } }`.
fn participant(
    participate: impl Fn(Vec<WorkingCopyFileEventFile>, FileOperation, Option<FileOperationUndoRedoInfo>, f64, CancellationToken) -> Promise<()> + 'static,
) -> Rc<dyn IWorkingCopyFileOperationParticipant> {
    Rc::new(FnParticipant(participate))
}

/// `{ participate: async (files, operation) => { ... } }`: o corpo roda e a promessa devolvida já está cumprida.
fn sync_participant(participate: impl Fn(&[WorkingCopyFileEventFile], FileOperation) + 'static) -> Rc<dyn IWorkingCopyFileOperationParticipant> {
    participant(move |files, operation, _undo_info, _timeout, _token| {
        participate(&files, operation);
        Promise::resolved(())
    })
}

/// A operação de movimento ou cópia de `source` para `target`, com `overwrite`.
fn move_operation(source: &URI, target: &URI, overwrite: Option<bool>) -> MoveOperation {
    MoveOperation { file: RequiredSourceTargetPair { source: source.clone(), target: target.clone() }, overwrite }
}

/// `testEventsMoveOrCopy(files, move)`.
async fn test_events_move_or_copy(accessor: &TextFileTestAccessor, files: Vec<MoveOperation>, is_move: bool) -> usize {
    let working_copy_file_service = working_copy_file_service(accessor);
    let event_counter = Rc::new(Cell::new(0usize));

    let counter = event_counter.clone();
    let participant = working_copy_file_service.add_file_operation_participant(sync_participant(move |_files, _operation| {
        counter.set(counter.get() + 1);
    }));

    let counter = event_counter.clone();
    let listener1 = working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |_e| {
        counter.set(counter.get() + 1);
    });

    let counter = event_counter.clone();
    let listener2 = working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |_e| {
        counter.set(counter.get() + 1);
    });

    if is_move {
        working_copy_file_service.move_(files, CancellationToken::None, None).await.unwrap();
    } else {
        working_copy_file_service.copy(files, CancellationToken::None, None).await.unwrap();
    }

    participant.dispose();
    listener1.dispose();
    listener2.dispose();
    event_counter.get()
}

/// `testMoveOrCopy(files, move, targetDirty)`.
async fn test_move_or_copy(accessor: &TextFileTestAccessor, files: Vec<(URI, URI)>, is_move: bool, target_dirty: bool) {
    let working_copy_file_service = working_copy_file_service(accessor);
    let event_counter = Rc::new(Cell::new(0usize));

    let creations: Vec<Promise<(Rc<TextFileEditorModel>, Rc<TextFileEditorModel>)>> = files
        .iter()
        .enumerate()
        .map(|(i, (source, target))| {
            let source_model = accessor.create_model(source, Some("utf8"), None);
            let target_model = accessor.create_model(target, Some("utf8"), None);
            accessor.files().add(&source_model.resource(), &source_model);
            accessor.files().add(&target_model.resource(), &target_model);

            let text_file_service = accessor.text_file_service.clone();
            async_fn(async move {
                source_model.resolve().await?;
                set_value(&*source_model, &format!("foo{i}"));
                assert!(text_file_service.is_dirty(&source_model.resource()));
                if target_dirty {
                    target_model.resolve().await?;
                    set_value(&*target_model, &format!("bar{i}"));
                    assert!(text_file_service.is_dirty(&target_model.resource()));
                }

                Ok((source_model, target_model))
            })
        })
        .collect();
    let models = Promise::all(creations).await.unwrap();

    let file_operation = if is_move { FileOperation::Move } else { FileOperation::Copy };

    let (participant_models, counter) = (models.clone(), event_counter.clone());
    let participant = working_copy_file_service.add_file_operation_participant(sync_participant(move |files, operation| {
        for (i, file) in files.iter().enumerate() {
            let (source_model, target_model) = &participant_models[i];

            assert_eq!(uri_string(&file.target), uri_string(&target_model.resource()));
            assert_eq!(file.source.as_ref().map(uri_string), Some(uri_string(&source_model.resource())));
        }

        counter.set(counter.get() + 1);

        assert_eq!(operation, file_operation);
    }));

    let correlation_id: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    let (listener_files, listener_models, counter, correlation) = (files.clone(), models.clone(), event_counter.clone(), correlation_id.clone());
    let listener1 = working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        for i in 0..e.files.len() {
            let (source, target) = &listener_files[i];
            let (source_model, target_model) = &listener_models[i];

            assert_eq!(uri_string(target), uri_string(&target_model.resource()));
            assert_eq!(Some(uri_string(source)), Some(uri_string(&source_model.resource())));
        }

        counter.set(counter.get() + 1);

        correlation.set(Some(e.correlation_id));
        assert_eq!(e.operation, file_operation);
    });

    let (listener_files, listener_models, counter, correlation) = (files, models.clone(), event_counter.clone(), correlation_id);
    let listener2 = working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        for i in 0..e.files.len() {
            let (source, target) = &listener_files[i];
            let (source_model, target_model) = &listener_models[i];
            assert_eq!(uri_string(target), uri_string(&target_model.resource()));
            assert_eq!(Some(uri_string(source)), Some(uri_string(&source_model.resource())));
        }

        counter.set(counter.get() + 1);

        assert_eq!(e.operation, file_operation);
        assert_eq!(Some(e.correlation_id), correlation.get());
    });

    // `options: { overwrite: true }` não é lido pelo `IMoveOperation`: o `overwrite` fica indefinido.
    let operations: Vec<MoveOperation> = models.iter().map(|(source_model, target_model)| move_operation(&source_model.resource(), &target_model.resource(), None)).collect();
    if is_move {
        working_copy_file_service.move_(operations, CancellationToken::None, None).await.unwrap();
    } else {
        working_copy_file_service.copy(operations, CancellationToken::None, None).await.unwrap();
    }

    for (i, (source_model, target_model)) in models.iter().enumerate() {
        assert_eq!(editor_value(&**target_model), format!("foo{i}"));

        if is_move {
            assert!(!accessor.text_file_service.is_dirty(&source_model.resource()));
        } else {
            assert!(accessor.text_file_service.is_dirty(&source_model.resource()));
        }
        assert!(accessor.text_file_service.is_dirty(&target_model.resource()));

        source_model.dispose();
        target_model.dispose();
    }
    assert_eq!(event_counter.get(), 3);

    participant.dispose();
    listener1.dispose();
    listener2.dispose();
}

/// `testDelete(resources)`.
async fn test_delete(accessor: &TextFileTestAccessor, resources: Vec<URI>) {
    let working_copy_file_service = working_copy_file_service(accessor);

    let creations: Vec<Promise<Rc<TextFileEditorModel>>> = resources
        .iter()
        .map(|resource| {
            let model = accessor.create_model(resource, Some("utf8"), None);
            accessor.files().add(&model.resource(), &model);

            let working_copy_service = accessor.working_copy_service.clone();
            async_fn(async move {
                model.resolve().await?;
                set_value(&*model, "foo");
                assert!(working_copy_service.is_dirty(&model.resource(), None));

                Ok(model)
            })
        })
        .collect();
    let models = Promise::all(creations).await.unwrap();

    let event_counter = Rc::new(Cell::new(0usize));
    let correlation_id: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    let (participant_models, counter) = (models.clone(), event_counter.clone());
    let participant = working_copy_file_service.add_file_operation_participant(sync_participant(move |files, operation| {
        for (i, model) in participant_models.iter().enumerate() {
            let file = &files[i];
            assert_eq!(uri_string(&file.target), uri_string(&model.resource()));
        }
        assert_eq!(operation, FileOperation::Delete);
        counter.set(counter.get() + 1);
    }));

    let (listener_models, counter, correlation) = (models.clone(), event_counter.clone(), correlation_id.clone());
    let listener1 = working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        for (i, model) in listener_models.iter().enumerate() {
            let file = &e.files[i];
            assert_eq!(uri_string(&file.target), uri_string(&model.resource()));
        }
        assert_eq!(e.operation, FileOperation::Delete);
        correlation.set(Some(e.correlation_id));
        counter.set(counter.get() + 1);
    });

    let (listener_models, counter, correlation) = (models.clone(), event_counter.clone(), correlation_id);
    let listener2 = working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        for (i, model) in listener_models.iter().enumerate() {
            let file = &e.files[i];
            assert_eq!(uri_string(&file.target), uri_string(&model.resource()));
        }
        assert_eq!(e.operation, FileOperation::Delete);
        assert_eq!(Some(e.correlation_id), correlation.get());
        counter.set(counter.get() + 1);
    });

    let operations: Vec<DeleteOperation> = models.iter().map(|model| DeleteOperation { resource: model.resource(), use_trash: None, recursive: None }).collect();
    working_copy_file_service.delete(operations, CancellationToken::None, None).await.unwrap();
    for model in &models {
        assert!(!accessor.working_copy_service.is_dirty(&model.resource(), None));
        model.dispose();
    }

    assert_eq!(event_counter.get(), 3);

    participant.dispose();
    listener1.dispose();
    listener2.dispose();
}

/// `testCreate(resource, contents)`.
async fn test_create(accessor: &TextFileTestAccessor, disposables: &LeakGuard, resource: URI, contents: VSBuffer) {
    let working_copy_file_service = working_copy_file_service(accessor);

    let model = accessor.create_model(&resource, Some("utf8"), None);
    accessor.files().add(&model.resource(), &model);

    model.resolve().await.unwrap();
    set_value(&*model, "foo");
    assert!(accessor.working_copy_service.is_dirty(&model.resource(), None));

    let event_counter = Rc::new(Cell::new(0usize));
    let correlation_id: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    let (model_resource, counter) = (model.resource(), event_counter.clone());
    disposables.add(working_copy_file_service.add_file_operation_participant(sync_participant(move |files, operation| {
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(uri_string(&file.target), uri_string(&model_resource));
        assert_eq!(operation, FileOperation::Create);
        counter.set(counter.get() + 1);
    })));

    let (model_resource, counter, correlation) = (model.resource(), event_counter.clone(), correlation_id.clone());
    disposables.add(working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        assert_eq!(e.files.len(), 1);
        let file = &e.files[0];
        assert_eq!(uri_string(&file.target), uri_string(&model_resource));
        assert_eq!(e.operation, FileOperation::Create);
        correlation.set(Some(e.correlation_id));
        counter.set(counter.get() + 1);
    }));

    let (model_resource, counter, correlation) = (model.resource(), event_counter.clone(), correlation_id);
    disposables.add(working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
        assert_eq!(e.files.len(), 1);
        let file = &e.files[0];
        assert_eq!(uri_string(&file.target), uri_string(&model_resource));
        assert_eq!(e.operation, FileOperation::Create);
        assert_eq!(Some(e.correlation_id), correlation.get());
        counter.set(counter.get() + 1);
    }));

    let operation = CreateFileOperation { resource, contents: Some(EncodedContent::Buffer(contents)), overwrite: None };
    working_copy_file_service.create(vec![operation], CancellationToken::None, None).await.unwrap();
    assert!(!accessor.working_copy_service.is_dirty(&model.resource(), None));
    model.dispose();

    assert_eq!(event_counter.get(), 3);
}

/// Um bloco de `cancellation of participants`: inicia a operação com o token de um `CancellationTokenSource`, cancela
/// antes de esperá-la e confere que o participante viu o token cancelado (depois reinicia a bandeira).
async fn assert_participants_see_cancellation(canceled: &Rc<Cell<bool>>, start: impl FnOnce(CancellationToken) -> Promise<()>) {
    let cts = CancellationTokenSource::new(None);
    let promise = start(cts.token());
    cts.cancel();
    promise.await.unwrap();
    assert!(canceled.get());
    canceled.set(false);
}

//#endregion

#[test]
fn create_dirty_file() {
    const TITLE: &str = "create - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_create(&accessor, &disposables, to_resource(TITLE, "/path/file.txt"), VSBuffer::from_string("Hello World")).await;
    });
}

#[test]
fn delete_dirty_file() {
    const TITLE: &str = "delete - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_delete(&accessor, vec![to_resource(TITLE, "/path/file.txt")]).await;
    });
}

#[test]
fn delete_multiple_dirty_files() {
    const TITLE: &str = "delete multiple - dirty files";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_delete(
            &accessor,
            vec![
                to_resource(TITLE, "/path/file1.txt"),
                to_resource(TITLE, "/path/file2.txt"),
                to_resource(TITLE, "/path/file3.txt"),
                to_resource(TITLE, "/path/file4.txt"),
            ],
        )
        .await;
    });
}

#[test]
fn move_dirty_file() {
    const TITLE: &str = "move - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(&accessor, vec![(to_resource(TITLE, "/path/file.txt"), to_resource(TITLE, "/path/file_target.txt"))], true, false).await;
    });
}

#[test]
fn move_source_identical_to_target() {
    const TITLE: &str = "move - source identical to target";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let source_model = accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None);
        accessor.files().add(&source_model.resource(), &source_model);

        let event_counter = test_events_move_or_copy(&accessor, vec![move_operation(&source_model.resource(), &source_model.resource(), Some(true))], true).await;

        source_model.dispose();
        assert_eq!(event_counter, 3);
    });
}

#[test]
fn move_one_source_equals_target_and_another_source_differs_from_target() {
    const TITLE: &str = "move - one source == target and another source != target";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let source_model1 = accessor.create_model(&to_resource(TITLE, "/path/file1.txt"), Some("utf8"), None);
        let source_model2 = accessor.create_model(&to_resource(TITLE, "/path/file2.txt"), Some("utf8"), None);
        let target_model2 = accessor.create_model(&to_resource(TITLE, "/path/file_target2.txt"), Some("utf8"), None);
        accessor.files().add(&source_model1.resource(), &source_model1);
        accessor.files().add(&source_model2.resource(), &source_model2);
        accessor.files().add(&target_model2.resource(), &target_model2);

        let event_counter = test_events_move_or_copy(
            &accessor,
            vec![
                move_operation(&source_model1.resource(), &source_model1.resource(), Some(true)),
                move_operation(&source_model2.resource(), &target_model2.resource(), Some(true)),
            ],
            true,
        )
        .await;

        source_model1.dispose();
        source_model2.dispose();
        target_model2.dispose();
        assert_eq!(event_counter, 3);
    });
}

#[test]
fn move_multiple_dirty_file() {
    const TITLE: &str = "move multiple - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(
            &accessor,
            vec![
                (to_resource(TITLE, "/path/file1.txt"), to_resource(TITLE, "/path/file1_target.txt")),
                (to_resource(TITLE, "/path/file2.txt"), to_resource(TITLE, "/path/file2_target.txt")),
            ],
            true,
            false,
        )
        .await;
    });
}

#[test]
fn move_dirty_file_target_exists_and_is_dirty() {
    const TITLE: &str = "move - dirty file (target exists and is dirty)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(&accessor, vec![(to_resource(TITLE, "/path/file.txt"), to_resource(TITLE, "/path/file_target.txt"))], true, true).await;
    });
}

#[test]
fn copy_dirty_file() {
    const TITLE: &str = "copy - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(&accessor, vec![(to_resource(TITLE, "/path/file.txt"), to_resource(TITLE, "/path/file_target.txt"))], false, false).await;
    });
}

#[test]
fn copy_source_identical_to_target() {
    const TITLE: &str = "copy - source identical to target";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let source_model = accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None);
        accessor.files().add(&source_model.resource(), &source_model);

        let event_counter = test_events_move_or_copy(&accessor, vec![move_operation(&source_model.resource(), &source_model.resource(), Some(true))], false).await;

        source_model.dispose();
        assert_eq!(event_counter, 3);
    });
}

#[test]
fn copy_one_source_equals_target_and_another_source_differs_from_target() {
    const TITLE: &str = "copy - one source == target and another source != target";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let source_model1 = accessor.create_model(&to_resource(TITLE, "/path/file1.txt"), Some("utf8"), None);
        let source_model2 = accessor.create_model(&to_resource(TITLE, "/path/file2.txt"), Some("utf8"), None);
        let target_model2 = accessor.create_model(&to_resource(TITLE, "/path/file_target2.txt"), Some("utf8"), None);
        accessor.files().add(&source_model1.resource(), &source_model1);
        accessor.files().add(&source_model2.resource(), &source_model2);
        accessor.files().add(&target_model2.resource(), &target_model2);

        let event_counter = test_events_move_or_copy(
            &accessor,
            vec![
                move_operation(&source_model1.resource(), &source_model1.resource(), Some(true)),
                move_operation(&source_model2.resource(), &target_model2.resource(), Some(true)),
            ],
            false,
        )
        .await;

        source_model1.dispose();
        source_model2.dispose();
        target_model2.dispose();
        assert_eq!(event_counter, 3);
    });
}

#[test]
fn copy_multiple_dirty_file() {
    const TITLE: &str = "copy multiple - dirty file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(
            &accessor,
            vec![
                (to_resource(TITLE, "/path/file1.txt"), to_resource(TITLE, "/path/file_target1.txt")),
                (to_resource(TITLE, "/path/file2.txt"), to_resource(TITLE, "/path/file_target2.txt")),
                (to_resource(TITLE, "/path/file3.txt"), to_resource(TITLE, "/path/file_target3.txt")),
            ],
            false,
            false,
        )
        .await;
    });
}

#[test]
fn copy_dirty_file_target_exists_and_is_dirty() {
    const TITLE: &str = "copy - dirty file (target exists and is dirty)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        test_move_or_copy(&accessor, vec![(to_resource(TITLE, "/path/file.txt"), to_resource(TITLE, "/path/file_target.txt"))], false, true).await;
    });
}

#[test]
fn get_dirty() {
    const TITLE: &str = "getDirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let working_copy_file_service = working_copy_file_service(&accessor);

        let model1 = accessor.create_model(&to_resource(TITLE, "/path/file-1.txt"), Some("utf8"), None);
        accessor.files().add(&model1.resource(), &model1);

        let model2 = accessor.create_model(&to_resource(TITLE, "/path/file-2.txt"), Some("utf8"), None);
        accessor.files().add(&model2.resource(), &model2);

        let mut dirty = working_copy_file_service.get_dirty(&model1.resource());
        assert_eq!(dirty.len(), 0);

        model1.resolve().await.unwrap();
        set_value(&*model1, "foo");

        dirty = working_copy_file_service.get_dirty(&model1.resource());
        assert_eq!(dirty.len(), 1);
        assert!(same_model(&dirty[0], &model1));

        dirty = working_copy_file_service.get_dirty(&to_resource(TITLE, "/path"));
        assert_eq!(dirty.len(), 1);
        assert!(same_model(&dirty[0], &model1));

        model2.resolve().await.unwrap();
        set_value(&*model2, "bar");

        dirty = working_copy_file_service.get_dirty(&to_resource(TITLE, "/path"));
        assert_eq!(dirty.len(), 2);

        model1.dispose();
        model2.dispose();
    });
}

#[test]
fn register_working_copy_provider() {
    const TITLE: &str = "registerWorkingCopyProvider";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let working_copy_file_service = working_copy_file_service(&accessor);

        let model1 = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file-1.txt"), Some("utf8"), None));
        accessor.files().add(&model1.resource(), &model1);
        model1.resolve().await.unwrap();
        set_value(&*model1, "foo");

        let test_working_copy = disposables.add_rc(TestWorkingCopy::new(to_resource(TITLE, "/path/file-2.txt"), Some(true), None));
        let provided_model: Rc<dyn IWorkingCopy> = model1.clone();
        let provided_working_copy: Rc<dyn IWorkingCopy> = test_working_copy.clone();
        let registration = working_copy_file_service.register_working_copy_provider(Rc::new(move |_resource: &URI| vec![provided_model.clone(), provided_working_copy.clone()]));

        let mut dirty = working_copy_file_service.get_dirty(&model1.resource());
        assert_eq!(dirty.len(), 2, "Should return default working copy + working copy from provider");
        assert!(same_model(&dirty[0], &model1));
        assert!(same_model(&dirty[1], &test_working_copy));

        registration.dispose();

        dirty = working_copy_file_service.get_dirty(&model1.resource());
        assert_eq!(dirty.len(), 1, "Should have unregistered our provider");
        assert!(same_model(&dirty[0], &model1));
    });
}

#[test]
fn create_folder() {
    const TITLE: &str = "createFolder";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let working_copy_file_service = working_copy_file_service(&accessor);

        let event_counter = Rc::new(Cell::new(0usize));
        let correlation_id: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

        let resource = to_resource(TITLE, "/path/folder");

        let (expected, counter) = (resource.clone(), event_counter.clone());
        disposables.add(working_copy_file_service.add_file_operation_participant(sync_participant(move |files, operation| {
            assert_eq!(files.len(), 1);
            let file = &files[0];
            assert_eq!(uri_string(&file.target), uri_string(&expected));
            assert_eq!(operation, FileOperation::Create);
            counter.set(counter.get() + 1);
        })));

        let (expected, counter, correlation) = (resource.clone(), event_counter.clone(), correlation_id.clone());
        disposables.add(working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
            assert_eq!(e.files.len(), 1);
            let file = &e.files[0];
            assert_eq!(uri_string(&file.target), uri_string(&expected));
            assert_eq!(e.operation, FileOperation::Create);
            correlation.set(Some(e.correlation_id));
            counter.set(counter.get() + 1);
        }));

        let (expected, counter, correlation) = (resource.clone(), event_counter.clone(), correlation_id);
        disposables.add(working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |e: &WorkingCopyFileEvent| {
            assert_eq!(e.files.len(), 1);
            let file = &e.files[0];
            assert_eq!(uri_string(&file.target), uri_string(&expected));
            assert_eq!(e.operation, FileOperation::Create);
            assert_eq!(Some(e.correlation_id), correlation.get());
            counter.set(counter.get() + 1);
        }));

        working_copy_file_service.create_folder(vec![CreateOperation { resource, overwrite: None }], CancellationToken::None, None).await.unwrap();

        assert_eq!(event_counter.get(), 3);
    });
}

#[test]
fn cancellation_of_participants() {
    const TITLE: &str = "cancellation of participants";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let working_copy_file_service = working_copy_file_service(&accessor);

        let resource = to_resource(TITLE, "/path/folder");

        let canceled = Rc::new(Cell::new(false));
        let flag = canceled.clone();
        disposables.add(working_copy_file_service.add_file_operation_participant(participant(move |_files, _operation, _info, _t, token| {
            let flag = flag.clone();
            async_fn(async move {
                timeout(0).await?;
                flag.set(token.is_cancellation_requested());

                Ok(())
            })
        })));

        // Create
        let (service, target) = (working_copy_file_service.clone(), resource.clone());
        assert_participants_see_cancellation(&canceled, move |token| {
            service.create(vec![CreateFileOperation { resource: target, contents: None, overwrite: None }], token, None).then(|_| ())
        })
        .await;

        // Create Folder
        let (service, target) = (working_copy_file_service.clone(), resource.clone());
        assert_participants_see_cancellation(&canceled, move |token| service.create_folder(vec![CreateOperation { resource: target, overwrite: None }], token, None).then(|_| ())).await;

        // Move
        let (service, target) = (working_copy_file_service.clone(), resource.clone());
        assert_participants_see_cancellation(&canceled, move |token| service.move_(vec![move_operation(&target, &target, None)], token, None).then(|_| ())).await;

        // Copy
        let (service, target) = (working_copy_file_service.clone(), resource.clone());
        assert_participants_see_cancellation(&canceled, move |token| service.copy(vec![move_operation(&target, &target, None)], token, None).then(|_| ())).await;

        // Delete
        let (service, target) = (working_copy_file_service.clone(), resource);
        assert_participants_see_cancellation(&canceled, move |token| service.delete(vec![DeleteOperation { resource: target, use_trash: None, recursive: None }], token, None)).await;
    });
}
