//! Porte de `src/vs/workbench/services/textfile/test/browser/textFileService.test.ts` (suíte `Files - TextFileService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`disposables.clear()`) é o descarte do store do guarda, que roda quando o guarda sai de escopo e antes de ele conferir
//!   os vazamentos. Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** Estão em `tests/support/text_file_support.rs` (o
//!   `TestTextFileService` é montado sobre o `AbstractTextFileService` real). `accessor.textFileService.files as
//!   ITestTextFileEditorModelManager` é [`TextFileTestAccessor::files`] (o gerenciador concreto, que tem `add`), e o
//!   `disposables.add(files)` do `setup` é o descarte dele no store do guarda.
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`].
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte
//!   mais o título do caso) é o argumento de [`to_resource`].
//! - **`model.textEditorModel!.setValue('foo')`** é `text_editor_model().set_value(TextSource::from("foo"))`.
//! - **`accessor.textFileService.untitled.resolve()`** é o `resolve(None)` do gerenciador de modelos sem título; o
//!   `UntitledTextEditorModel` nasce por `createInstance` com os serviços que o [`TextFileTestAccessor`] registra na coleção.
//! - **Asserções dentro de ouvintes e participantes** (`create does not overwrite existing model`): o participante e o
//!   ouvinte guardam o que o original afirmava (a operação e o alvo) e o caso confere os registros no fim, junto com a
//!   contagem de eventos. O participante é o [`file_operation_participant`] do `TestWorkingCopyFileService` (o
//!   `addFileOperationParticipant` do `WorkingCopyFileService`), e o `WorkingCopyFileService.create` do teste grava o
//!   conteúdo no `TestFileService`.
//! - **`languageService.registerLanguage({ id, extensions, filenames })`** é o `register_language` com o
//!   `LanguageExtensionPoint`; `suggestFilename` é [`TestTextFileService::suggest_filename`] (método do
//!   `AbstractTextFileService`, que o contrato `ITextFileService` não tem).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::text_model::TextSource;
use je_platform::files::files::FileOperation;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::services::textfile::collaborators::IWorkingCopyFileService;
use je_workbench::services::textfile::textfiles::{EncodingMode, ITextFileEditorModel, ITextFileService, TextFileCreateOperation, TextOrSnapshot};
use je_workbench::services::textmodel_resolver::resolver_service::ITextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use support::stored_file_working_copy_support::file_operation_participant;
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Files - TextFileService";

/// O `setup`: `workbenchInstantiationService(undefined, disposables)`, o `TestServiceAccessor` e o gerenciador de modelos
/// como descartável (`disposables.add(<ITestTextFileEditorModelManager>accessor.textFileService.files)`).
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let accessor = TextFileTestAccessor::new(&disposables.store());
    disposables.add_rc(accessor.files());

    accessor
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

/// `languageService.registerLanguage({ id, extensions, filenames })`.
fn register_language(disposables: &LeakGuard, accessor: &TextFileTestAccessor, id: &str, extensions: Option<&[&str]>, filenames: Option<&[&str]>) {
    let to_strings = |values: &[&str]| values.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    disposables.add(accessor.language_service.register_language(LanguageExtensionPoint {
        extensions: extensions.map(to_strings),
        filenames: filenames.map(to_strings),
        ..LanguageExtensionPoint::new(id)
    }));
}

//#endregion

#[test]
fn is_dirty_get_dirty_files_and_untitled() {
    const TITLE: &str = "isDirty/getDirty - files and untitled";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);

        model.resolve().await.unwrap();

        assert!(!accessor.text_file_service.is_dirty(&model.resource()));
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));

        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        let untitled = accessor.text_file_service.untitled().resolve(None).await.unwrap();
        disposables.add(untitled.clone());

        assert!(!accessor.text_file_service.is_dirty(&untitled.resource()));
        untitled.text_editor_model().expect("o modelo de texto existe").set_value(TextSource::from("changed"));

        assert!(accessor.text_file_service.is_dirty(&untitled.resource()));
    });
}

#[test]
fn save_file() {
    const TITLE: &str = "save - file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);

        model.resolve().await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));
        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        let res = accessor.text_file_service.save(&model.resource(), None).await.unwrap();
        assert_eq!(res.map(|resource| uri_string(&resource)), Some(uri_string(&model.resource())));
        assert!(!accessor.text_file_service.is_dirty(&model.resource()));
    });
}

#[test]
fn save_all_file() {
    const TITLE: &str = "saveAll - file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);

        model.resolve().await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));
        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        let res = accessor.text_file_service.save(&model.resource(), None).await.unwrap();
        assert_eq!(res.map(|resource| uri_string(&resource)), Some(uri_string(&model.resource())));
        assert!(!accessor.text_file_service.is_dirty(&model.resource()));
    });
}

#[test]
fn save_as_file() {
    const TITLE: &str = "saveAs - file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);
        accessor.file_dialog_service.set_pick_file_to_save(model.resource());

        model.resolve().await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));
        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        let res = accessor.text_file_service.save_as(&model.resource(), None, None).await.unwrap();
        assert_eq!(uri_string(&res.expect("o salvar como devolve o recurso")), uri_string(&model.resource()));
        assert!(!accessor.text_file_service.is_dirty(&model.resource()));
    });
}

#[test]
fn revert_file() {
    const TITLE: &str = "revert - file";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);
        accessor.file_dialog_service.set_pick_file_to_save(model.resource());

        model.resolve().await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));
        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        accessor.text_file_service.revert(&model.resource(), None).await.unwrap();
        assert!(!accessor.text_file_service.is_dirty(&model.resource()));
    });
}

#[test]
fn create_does_not_overwrite_existing_model() {
    const TITLE: &str = "create does not overwrite existing model";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);

        model.resolve().await.unwrap();
        ITextFileEditorModel::text_editor_model(&*model).expect("o modelo de texto existe").set_value(TextSource::from("foo"));
        assert!(accessor.text_file_service.is_dirty(&model.resource()));

        let event_counter = Rc::new(Cell::new(0usize));
        // o que o original afirma dentro do participante e do ouvinte, conferido no fim do caso
        let participated_targets: Rc<RefCell<Vec<String>>> = Rc::default();
        let ran_events: Rc<RefCell<Vec<(bool, String)>>> = Rc::default();

        let (counter, targets) = (event_counter.clone(), participated_targets.clone());
        disposables.add(accessor.working_copy_file_service.add_file_operation_participant(file_operation_participant(move |files| {
            targets.borrow_mut().push(uri_string(&files[0].target));
            counter.set(counter.get() + 1);

            Promise::resolved(())
        })));

        let (counter, events) = (event_counter.clone(), ran_events.clone());
        disposables.add(accessor.working_copy_file_service.on_did_run_working_copy_file_operation().listen(move |event| {
            events.borrow_mut().push((event.operation == FileOperation::Create, uri_string(&event.files[0].target)));
            counter.set(counter.get() + 1);
        }));

        accessor
            .text_file_service
            .create(vec![TextFileCreateOperation { resource: model.resource(), value: Some(TextOrSnapshot::from("Foo")), overwrite: None }], None)
            .await
            .unwrap();
        assert!(!accessor.text_file_service.is_dirty(&model.resource()));

        assert_eq!(event_counter.get(), 2);
        assert_eq!(*participated_targets.borrow(), vec![uri_string(&model.resource())]);
        assert_eq!(*ran_events.borrow(), vec![(true, uri_string(&model.resource()))]);
    });
}

#[test]
fn filename_suggestion_suggest_prefix_only_when_there_are_no_relevant_extensions() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus0", Some(&[".one", ".two"]), None);

    let suggested = accessor.text_file_service.suggest_filename("shleem", "Untitled-1");
    assert_eq!(suggested, "Untitled-1");
}

#[test]
fn filename_suggestion_suggest_prefix_with_first_extension() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus1", Some(&[".shleem", ".gazorpazorp"]), Some(&["plumbus"]));

    let suggested = accessor.text_file_service.suggest_filename("plumbus1", "Untitled-1");
    assert_eq!(suggested, "Untitled-1.shleem");
}

#[test]
fn filename_suggestion_preserve_extension_if_it_matchers() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus2", Some(&[".shleem", ".gazorpazorp"]), None);

    let suggested = accessor.text_file_service.suggest_filename("plumbus2", "Untitled-1.gazorpazorp");
    assert_eq!(suggested, "Untitled-1.gazorpazorp");
}

#[test]
fn filename_suggestion_rewrite_extension_according_to_language() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus2", Some(&[".shleem", ".gazorpazorp"]), None);

    let suggested = accessor.text_file_service.suggest_filename("plumbus2", "Untitled-1.foobar");
    assert_eq!(suggested, "Untitled-1.shleem");
}

#[test]
fn filename_suggestion_suggest_filename_if_there_are_no_extensions() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus2", None, Some(&["plumbus", "shleem", "gazorpazorp"]));

    let suggested = accessor.text_file_service.suggest_filename("plumbus2", "Untitled-1");
    assert_eq!(suggested, "plumbus");
}

#[test]
fn filename_suggestion_preserve_filename_if_it_matches() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus2", None, Some(&["plumbus", "shleem", "gazorpazorp"]));

    let suggested = accessor.text_file_service.suggest_filename("plumbus2", "gazorpazorp");
    assert_eq!(suggested, "gazorpazorp");
}

#[test]
fn filename_suggestion_rewrites_filename_according_to_language() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    register_language(&disposables, &accessor, "plumbus2", None, Some(&["plumbus", "shleem", "gazorpazorp"]));

    let suggested = accessor.text_file_service.suggest_filename("plumbus2", "foobar");
    assert_eq!(suggested, "plumbus");
}

#[test]
fn get_encoding_files_and_untitled() {
    const TITLE: &str = "getEncoding() - files and untitled";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/file.txt"), Some("utf8"), None));
        accessor.files().add(&model.resource(), &model);

        model.resolve().await.unwrap();

        assert_eq!(accessor.text_file_service.get_encoding(&model.resource()), "utf8");
        model.set_encoding("utf16", EncodingMode::Encode).await.unwrap();
        assert_eq!(accessor.text_file_service.get_encoding(&model.resource()), "utf16");

        let untitled = accessor.text_file_service.untitled().resolve(None).await.unwrap();
        disposables.add(untitled.clone());

        assert_eq!(accessor.text_file_service.get_encoding(&untitled.resource()), "utf8");
        untitled.set_encoding("utf16").await.unwrap();
        assert_eq!(accessor.text_file_service.get_encoding(&untitled.resource()), "utf16");
    });
}
