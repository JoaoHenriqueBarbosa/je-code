//! Porte de `src/vs/workbench/contrib/files/test/browser/textFileEditorTracker.test.ts` (suíte `Files - TextFileEditorTracker`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado antes do corpo (o
//!   `disposables` do original é o store dele) e descartado depois dele (`drop(guard)` em [`run_case`]), que é o
//!   `disposables.clear()` do `teardown` seguido da conferência de vazamentos. O `setup` registra os painéis
//!   `TestTextFileEditor` e `TestResourceEditor`. O original não usa timers falsos: os casos rodam com `run_with_real_timers`.
//! - **`createTracker`.** O [`create_tracker`] usa o [`create_accessor`] (ver `support/editor_auto_save_support.rs`) com
//!   `files.autoSave` igual a `afterDelay` ou `off` e `autoSaveDelay: 1`. A subclasse de teste do rastreador (que devolve `5`
//!   em `getDirtyTextFileTrackerDelay`) é o [`TestTextFileEditorTrackerHooks`], e o `cleanup` é [`cleanup`]: o
//!   `workbenchTeardown` do acessor. O `part.dispose()` do original não entra: a parte já é descartada pelo store do acessor.
//! - **`TestHostService`.** É o [`FocusTestHostService`] (`setFocus`, `onDidChangeFocus`), passado ao rastreador no lugar do
//!   serviço de host do acessor.
//! - **`ICodeEditorService` e o `TestTextFileEditor`.** O painel `TestTextFileEditor` do original cria um `TestCodeEditor` sobre
//!   o modelo de texto da entrada e o registra no `ICodeEditorService` (o `TestCodeEditorService` do `je-editor/tests/support`,
//!   cujo registro o rastreador recebe como [`CodeEditorRegistry`]). O painel lógico do porte não tem controle de editor, então
//!   o caso `non-dirty files reload on window focus` cria o `TestCodeEditor` na hora, depois de abrir a entrada, sobre o modelo
//!   que a abertura resolveu, e o descarta antes do `cleanup` (o painel o descartaria ao fechar os editores).
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`as IResolvedTextFileEditorModel`.** O `textEditorModel` e o `createSnapshot()` do modelo resolvido são os do contrato
//!   `ITextFileEditorModel` (`Option`), lidos com `expect`.
//! - **`Event.toPromise(Event.once(...))`.** É `Event::to_promise(None)` (que já dispara uma vez só). O `awaitModelResolveEvent`
//!   (ouvinte que se descarta no primeiro evento do recurso) é `once_if` com `to_promise`.
//! - **`untitledTextEditor.resolve()`.** É `resolve_untitled()` da entrada sem título (devolve o modelo sem título).
//! - **`isOpened(untitledTextEditor)`.** O identificador é o recurso, o `typeId` e o `editorId` da entrada.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

/// O suporte de editor do `je-editor/tests/support` que o `createTestCodeEditor` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use editor_support::test_code_editor::{I_CODE_EDITOR_SERVICE, TestCodeEditorInstantiationOptions, create_test_code_editor};
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::path::PathModule;
use je_base::uri::URI;
use je_editor::services::abstract_code_editor_service::CodeEditorRegistry;
use je_editor::text_model::TextSource;
use je_instantiation::{Injectable, ServiceCollection};
use je_platform::files::files::{FileChange, FileChangeType, FileChangesEvent, FileOperationError, FileOperationResult};
use je_platform::resources::{ext_uri, host_environment, uri_file};
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::{DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputRef, TextEditorOptions, UntypedEditorInput};
use je_workbench::contrib::files::browser::editors::text_file_editor_tracker::{TextFileEditorTracker, TextFileEditorTrackerHooks};
use je_workbench::contrib::files::common_files::FILE_EDITOR_INPUT_ID;
use je_workbench::services::editor::editor_groups_service::IEditorPart;
use je_workbench::services::editor::editor_service::{IEditorService, ResourceEditorInputIdentifier};
use je_workbench::services::textfile::text_editor_service::ITextEditorService;
use je_workbench::services::textfile::textfiles::{ITextFileEditorModel, ITextFileEditorModelManager, snapshot_to_string};
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use support::editor_auto_save_support::{AutoSaveAccessor, FocusTestHostService, create_accessor};
use support::editor_service_support::{EditorServiceAccessor, register_test_file_editor, register_test_resource_editor};

/// O título da suíte, que o `this.test.fullTitle()` do `toResource` usa.
const SUITE_TITLE: &str = "Files - TextFileEditorTracker";

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `TestTextFileEditorTracker`: `getDirtyTextFileTrackerDelay` devolve `5`.
struct TestTextFileEditorTrackerHooks;

impl TextFileEditorTrackerHooks for TestTextFileEditorTrackerHooks {
    fn get_dirty_text_file_tracker_delay(&self) -> u64 {
        5 // encapsulated in a method for tests to override
    }
}

/// O caso: o guarda de vazamentos, o `setup` e o corpo, com o relógio real.
fn run_case<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = guard.store();

    // `setup`
    disposables.add(register_test_file_editor());
    disposables.add(register_test_resource_editor());

    let case = body(disposables);
    run_with_real_timers(async move {
        case.await;

        drop(guard);
    });
}

/// O que o `createTracker` devolve (o `cleanup` é [`cleanup`]).
struct TrackerFixture {
    accessor: EditorServiceAccessor,
    host_service: Rc<FocusTestHostService>,
    code_editor_service: Rc<TestCodeEditorService>,
}

/// `createTracker(autoSaveEnabled)`.
async fn create_tracker(disposables: &Rc<DisposableStore>, auto_save_enabled: bool) -> TrackerFixture {
    let files_configuration = if auto_save_enabled {
        Dynamic::object([("autoSave", Dynamic::from("afterDelay")), ("autoSaveDelay", Dynamic::from(1))])
    } else {
        Dynamic::object([("autoSave", Dynamic::from("off")), ("autoSaveDelay", Dynamic::from(1))])
    };
    let AutoSaveAccessor { accessor, files_configuration_service } = create_accessor(disposables, files_configuration);

    accessor.part.when_ready().await.unwrap();

    let host_service = disposables.add_rc(FocusTestHostService::new());
    let code_editor_service = Rc::new(TestCodeEditorService::new(TestThemeService::new_default()));
    let code_editor_registry: Rc<CodeEditorRegistry> = disposables.add_rc(code_editor_service.registry().clone());

    let hooks: Rc<dyn TextFileEditorTrackerHooks> = Rc::new(TestTextFileEditorTrackerHooks);
    disposables.add_rc(TextFileEditorTracker::create(
        Some(hooks),
        (
            accessor.editor_service.clone(),
            accessor.text_file_service.clone(),
            accessor.lifecycle_service.clone(),
            host_service.clone(),
            code_editor_registry,
            files_configuration_service,
            accessor.working_copy_editor_service.clone(),
        ),
    ));

    TrackerFixture { accessor, host_service, code_editor_service }
}

/// O `cleanup` do `createTracker`: `workbenchTeardown(instantiationService)`.
async fn cleanup(fixture: &TrackerFixture) {
    fixture.accessor.teardown().await.unwrap();
}

/// `model.textEditorModel.setValue('Super Good')`.
fn set_super_good(model: &Rc<dyn ITextFileEditorModel>) {
    model.text_editor_model().expect("o modelo de texto existe").set_value(TextSource::from("Super Good"));
}

/// `snapshotToString(model.createSnapshot()!)`.
fn model_snapshot(model: &Rc<dyn ITextFileEditorModel>) -> String {
    let mut snapshot = model.create_snapshot().expect("o modelo tem conteúdo");

    snapshot_to_string(&mut *snapshot)
}

/// `accessor.editorService.isOpened({ resource, typeId: FILE_EDITOR_INPUT_ID, editorId: DEFAULT_EDITOR_ASSOCIATION.id })`.
fn is_file_opened(accessor: &EditorServiceAccessor, resource: &URI) -> bool {
    IEditorService::is_opened(
        accessor.editor_service.as_ref(),
        &ResourceEditorInputIdentifier { resource: resource.clone(), type_id: FILE_EDITOR_INPUT_ID.to_string(), editor_id: Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string()) },
    )
}

/// `awaitEditorOpening(editorService)`: `Event.toPromise(Event.once(editorService.onDidActiveEditorChange))`.
async fn await_editor_opening(accessor: &EditorServiceAccessor) {
    IEditorService::on_did_active_editor_change(accessor.editor_service.as_ref()).to_promise(None).await.unwrap();
}

#[test]
fn file_change_event_updates_model() {
    run_case(|disposables| async move {
        let fixture = create_tracker(&disposables, false).await;
        let accessor = &fixture.accessor;

        let resource = to_resource("file change event updates model", "/path/index.txt");

        let model = accessor.files().resolve(&resource, None).await.unwrap();
        disposables.add(model.clone());

        set_super_good(&model);
        assert_eq!(model_snapshot(&model), "Super Good");

        IWorkingCopy::save(&*model, None).await.unwrap();

        // change event (watcher)
        accessor.file_service.fire_file_changes(Rc::new(FileChangesEvent::new(&[FileChange::new(FileChangeType::Updated, resource.clone())], false)));

        timeout(0).await.unwrap(); // due to event updating model async

        assert_eq!(model_snapshot(&model), "Hello Html");

        cleanup(&fixture).await;
    });
}

#[test]
fn dirty_text_file_model_opens_as_editor() {
    run_case(|disposables| async move {
        let resource = to_resource("dirty text file model opens as editor", "/path/index.txt");

        test_dirty_text_file_model_opens_editor_depending_on_auto_save_setting(&disposables, resource, false, false).await;
    });
}

#[test]
fn dirty_text_file_model_does_not_open_as_editor_if_autosave_is_on() {
    run_case(|disposables| async move {
        let resource = to_resource("dirty text file model does not open as editor if autosave is ON", "/path/index.txt");

        test_dirty_text_file_model_opens_editor_depending_on_auto_save_setting(&disposables, resource, true, false).await;
    });
}

#[test]
fn dirty_text_file_model_opens_as_editor_when_save_fails() {
    run_case(|disposables| async move {
        let resource = to_resource("dirty text file model opens as editor when save fails", "/path/index.txt");

        test_dirty_text_file_model_opens_editor_depending_on_auto_save_setting(&disposables, resource, false, true).await;
    });
}

#[test]
fn dirty_text_file_model_opens_as_editor_when_save_fails_if_autosave_is_on() {
    run_case(|disposables| async move {
        let resource = to_resource("dirty text file model opens as editor when save fails if autosave is ON", "/path/index.txt");

        test_dirty_text_file_model_opens_editor_depending_on_auto_save_setting(&disposables, resource, true, true).await;
    });
}

/// `testDirtyTextFileModelOpensEditorDependingOnAutoSaveSetting(resource, autoSave, error)`.
async fn test_dirty_text_file_model_opens_editor_depending_on_auto_save_setting(disposables: &Rc<DisposableStore>, resource: URI, auto_save: bool, error: bool) {
    let fixture = create_tracker(disposables, auto_save).await;
    let accessor = &fixture.accessor;

    assert!(!is_file_opened(accessor, &resource));

    if error {
        accessor.text_file_service.set_write_error_once(FileOperationError::new("fail to write", FileOperationResult::FileOtherError));
    }

    let model = accessor.files().resolve(&resource, None).await.unwrap();
    disposables.add(model.clone());

    set_super_good(&model);

    if auto_save {
        IWorkingCopy::save(&*model, None).await.unwrap();
        timeout(10).await.unwrap();
        if error {
            assert!(is_file_opened(accessor, &resource));
        } else {
            assert!(!is_file_opened(accessor, &resource));
        }
    } else {
        await_editor_opening(accessor).await;
        assert!(is_file_opened(accessor, &resource));
    }

    cleanup(&fixture).await;
}

#[test]
fn dirty_untitled_text_file_model_opens_as_editor() {
    run_case(|disposables| test_untitled_editor(disposables, false));
}

#[test]
fn dirty_untitled_text_file_model_opens_as_editor_autosave_on() {
    run_case(|disposables| test_untitled_editor(disposables, true));
}

/// `isOpened(untitledTextEditor)`: o recurso, o `typeId` e o `editorId` da entrada.
fn is_input_opened(accessor: &EditorServiceAccessor, input: &EditorInputRef) -> bool {
    IEditorService::is_opened(
        accessor.editor_service.as_ref(),
        &ResourceEditorInputIdentifier {
            resource: EditorInput::resource(input.as_ref()).expect("a entrada tem recurso"),
            type_id: EditorInput::editor_type_id(input.as_ref()),
            editor_id: EditorInput::editor_id(input.as_ref()),
        },
    )
}

/// `testUntitledEditor(autoSaveEnabled)`.
async fn test_untitled_editor(disposables: Rc<DisposableStore>, auto_save_enabled: bool) {
    let fixture = create_tracker(&disposables, auto_save_enabled).await;
    let accessor = &fixture.accessor;

    let input = ITextEditorService::resolve_text_editor(
        accessor.text_editor_service.as_ref(),
        UntypedEditorInput { resource: None, force_untitled: Some(true), ..UntypedEditorInput::default() },
    )
    .await
    .unwrap();
    let untitled_text_editor = input.as_any().downcast_ref::<UntitledTextEditorInput>().expect("a entrada é uma UntitledTextEditorInput");
    let model = untitled_text_editor.resolve_untitled().await.unwrap();
    disposables.add(model.clone());

    assert!(!is_input_opened(accessor, &input));

    if let Some(text_editor_model) = model.text_editor_model() {
        text_editor_model.set_value(TextSource::from("Super Good"));
    }

    await_editor_opening(accessor).await;
    assert!(is_input_opened(accessor, &input));

    cleanup(&fixture).await;
}

#[test]
fn non_dirty_files_reload_on_window_focus() {
    run_case(|disposables| async move {
        let fixture = create_tracker(&disposables, false).await;
        let accessor = &fixture.accessor;

        let resource = to_resource("non-dirty files reload on window focus", "/path/index.txt");

        let input = ITextEditorService::resolve_text_editor(
            accessor.text_editor_service.as_ref(),
            UntypedEditorInput {
                resource: Some(resource.clone()),
                options: Some(TextEditorOptions::with_override_id(DEFAULT_EDITOR_ASSOCIATION_ID)),
                ..UntypedEditorInput::default()
            },
        )
        .await
        .unwrap();
        IEditorService::open_editor_typed(accessor.editor_service.as_ref(), &input, None, None).await.unwrap();

        // o `TestCodeEditor` que o painel `TestTextFileEditor` cria sobre o modelo da entrada (ver as notas do módulo)
        let model = accessor.files().get(&resource).expect("a abertura resolveu o modelo do arquivo");
        let service_collection = ServiceCollection::new();
        service_collection.set(I_CODE_EDITOR_SERVICE, fixture.code_editor_service.clone());
        let code_editor = create_test_code_editor(
            model.text_editor_model(),
            TestCodeEditorInstantiationOptions { options: Dynamic::object([("hasTextFocus", Dynamic::Bool(false))]), service_collection: Some(service_collection) },
        );

        fixture.host_service.set_focus(false);
        fixture.host_service.set_focus(true);

        await_model_resolve_event(accessor, &resource).await;

        code_editor.dispose();
        cleanup(&fixture).await;
    });
}

/// `awaitModelResolveEvent(textFileService, resource)`: o primeiro `onDidResolve` do recurso.
async fn await_model_resolve_event(accessor: &EditorServiceAccessor, resource: &URI) {
    let resource = resource.clone();

    accessor
        .files()
        .on_did_resolve()
        .once_if(move |event| ext_uri().is_equal(Some(&IWorkingCopy::resource(&*event.model)), Some(&resource), false))
        .to_promise(None)
        .await
        .unwrap();
}
