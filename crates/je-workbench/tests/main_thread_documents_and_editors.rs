//! Porte de `src/vs/workbench/api/test/browser/mainThreadDocumentsAndEditors.test.ts` (suíte `MainThreadDocumentsAndEditors`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (ele também faz o papel do
//!   `disposables` do `setup` e do `teardown`: tudo o que o original registra em `disposables` entra no guarda). O
//!   `setup` é [`Fixture::new`].
//! - **`ModelService`.** O do porte não recebe o `IInstantiationService` (o `ILanguageService`, o
//!   `ILanguageConfigurationService` e o `ITreeSitterLibraryService` que o original registra nele): o modelo nasce com a
//!   linguagem `plaintext`, que é a do `createModel(..., null)`. `configService.setUserConfiguration('editor', {
//!   'detectIndentation': false })` é o mesmo.
//! - **`TestCodeEditorService(themeService)`** é o `TestCodeEditorService` do `je-editor/tests/support`, com o
//!   `TestThemeService` do tema de cores; o registro de editores dele é descartado no fim (o original o deixa sem
//!   descartar, fora da conferência de vazamentos por criá-lo antes de o rastreador ligar).
//! - **`myCreateTestCodeEditor`** passa o `ICodeEditorService` pela `serviceCollection`, como o original (identificador
//!   `I_CODE_EDITOR_SERVICE` do suporte).
//! - **`UriIdentityService(fileService)`** é o real, sobre o `TestFileService` (o original usa um `IFileService` com os três
//!   eventos). O `ITextModelService` `null!` é o [`UnusedTextModelService`], que falha se usado. Os demais dublês (`mock<T>()`)
//!   e o `SingleProxyRPCProtocol` estão em `support/main_thread_support.rs` (ver as divergências de lá).
//! - `TextModel._MODEL_SYNC_LIMIT` é lido e atribuído por `model_sync_limit` e `set_model_sync_limit`; o `try/finally` que o
//!   restaura é o guarda [`SyncLimit`].
//! - `this.timeout(1000 * 60)` do caso `ignore simple widget model` não tem equivalente (os testes não têm limite de tempo).
//! - `deltas.find(...)` e `delta.addedDocuments?.length === 1` são `iter().find(...)` sobre os campos `Option`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `createTestCodeEditor` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
mod editor_support {
    #[path = "../../../je-editor/tests/support/editor_test_services.rs"]
    pub mod editor_test_services;
    #[path = "../../../je-editor/tests/support/test_clipboard_service.rs"]
    pub mod test_clipboard_service;
    #[path = "../../../je-editor/tests/support/test_code_editor.rs"]
    pub mod test_code_editor;
    #[path = "../../../je-editor/tests/support/test_code_editor_service.rs"]
    pub mod test_code_editor_service;
    #[path = "../../../je-editor/tests/support/test_color_theme_service.rs"]
    pub mod test_color_theme_service;
    #[path = "../../../je-editor/tests/support/test_configuration.rs"]
    pub mod test_configuration;
    #[path = "../../../je-editor/tests/support/test_instantiation_service.rs"]
    pub mod test_instantiation_service;
    #[path = "../../../je-editor/tests/support/test_language_configuration_service.rs"]
    pub mod test_language_configuration_service;
    #[path = "../../../je-editor/tests/support/test_notification_service.rs"]
    pub mod test_notification_service;
    #[path = "../../../je-editor/tests/support/test_text_model.rs"]
    pub mod test_text_model;
    #[path = "../../../je-editor/tests/support/test_theme_service.rs"]
    pub mod test_theme_service;
}

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};
use editor_support::test_clipboard_service::TestClipboardService;
use editor_support::test_code_editor::{create_test_code_editor, TestCodeEditor, TestCodeEditorInstantiationOptions, I_CODE_EDITOR_SERVICE};
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::model_service::ModelService;
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::text_model::{model_sync_limit, set_model_sync_limit, TextModel};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{Injectable, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_documents_and_editors::MainThreadDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::DocumentsAndEditorsDelta;
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestTextResourcePropertiesService};
use main_thread_support::{
    FixedProxyContext, NoEventWorkingCopyFileService, RecordingProxy, StubPaneCompositePartService, StubQuickDiffModelService, StubTextFileService, UnusedTextModelService,
};

/// O estado que o `setup` da suíte deixa nas variáveis do `suite`.
struct Fixture {
    model_service: Rc<ModelService>,
    code_editor_service: Rc<TestCodeEditorService>,
    proxy: Rc<RecordingProxy>,
    main_thread_documents_and_editors: Rc<MainThreadDocumentsAndEditors>,
}

impl Fixture {
    /// `setup`.
    fn new(disposables: &LeakGuard) -> Self {
        let config_service = TestConfigurationService::new(None);
        let _ = config_service.set_user_configuration("editor", Dynamic::object([("detectIndentation", Dynamic::Bool(false))]), None);
        let configuration_service: Rc<dyn IConfigurationService> = config_service;
        let model_service = disposables.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service),
            UndoRedoService::for_tests(),
        ));
        let code_editor_service = Rc::new(TestCodeEditorService::new(TestThemeService::new_default()));
        let code_editor_registry = disposables.add_rc(code_editor_service.registry().clone());
        let workbench_editor_service = Rc::new(TestEditorService);
        let editor_group_service = TestEditorGroupsService::new(None);

        let file_service = disposables.add_rc(TestFileService::new());
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract,)));

        let clipboard_service = Rc::new(TestClipboardService::new());
        let _ = clipboard_service.write_text("clipboard_contents");

        let proxy = Rc::new(RecordingProxy::default());
        let main_thread_documents_and_editors = MainThreadDocumentsAndEditors::new(
            &FixedProxyContext::new(proxy.clone()),
            model_service.clone(),
            Rc::new(StubTextFileService),
            workbench_editor_service,
            code_editor_registry,
            file_service,
            Rc::new(UnusedTextModelService),
            editor_group_service,
            Rc::new(StubPaneCompositePartService),
            TestEnvironmentService::new(),
            Rc::new(NoEventWorkingCopyFileService),
            uri_identity_service,
            clipboard_service,
            TestPathService::new(None, None),
            TestConfigurationService::new(None),
            Rc::new(StubQuickDiffModelService),
        );
        disposables.add_rc(main_thread_documents_and_editors.clone());

        // O construtor emite o delta inicial (`newActiveEditor: null`): o `deltas.length = 0` do `setup` acontece antes
        // dele, então os casos que não zeram `deltas` o contam, como no original.
        Self { model_service, code_editor_service, proxy, main_thread_documents_and_editors }
    }

    /// `deltas`.
    fn deltas(&self) -> Vec<DocumentsAndEditorsDelta> {
        self.proxy.deltas.borrow().clone()
    }

    /// `deltas.length = 0`.
    fn clear_deltas(&self) {
        self.proxy.deltas.borrow_mut().clear();
    }

    /// `myCreateTestCodeEditor(model)`.
    fn my_create_test_code_editor(&self, model: Option<Rc<TextModel>>) -> Rc<TestCodeEditor> {
        let service_collection = ServiceCollection::new();
        service_collection.set(I_CODE_EDITOR_SERVICE, self.code_editor_service.clone());
        create_test_code_editor(
            model,
            TestCodeEditorInstantiationOptions {
                options: Dynamic::object([("hasTextFocus", Dynamic::Bool(false))]),
                service_collection: Some(service_collection),
            },
        )
    }
}

/// O `try/finally` que restaura `TextModel._MODEL_SYNC_LIMIT`.
struct SyncLimit(usize);

impl SyncLimit {
    /// `const oldLimit = TextModel._MODEL_SYNC_LIMIT; TextModel._MODEL_SYNC_LIMIT = limit`.
    fn set(limit: usize) -> Self {
        let old_limit = model_sync_limit();
        set_model_sync_limit(limit);
        Self(old_limit)
    }
}

impl Drop for SyncLimit {
    fn drop(&mut self) {
        set_model_sync_limit(self.0);
    }
}

#[test]
fn model_add() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    fixture.clear_deltas();

    disposables.add_rc(fixture.model_service.create_model("farboo", None, None, false));

    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 1);
    let delta = &deltas[0];

    assert_eq!(delta.added_documents.as_ref().unwrap().len(), 1);
    assert!(delta.removed_documents.is_none());
    assert!(delta.added_editors.is_none());
    assert!(delta.removed_editors.is_none());
    assert!(delta.new_active_editor.is_none());
}

#[test]
fn ignore_huge_model() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let large_model_string = "abc".repeat(1024);
    let _limit = SyncLimit::set(large_model_string.len() / 2);

    let model = fixture.model_service.create_model(large_model_string.as_str(), None, None, false);
    disposables.add_rc(model.clone());
    assert!(model.is_too_large_for_syncing());

    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 1);
    let delta = &deltas[0];
    assert_eq!(delta.new_active_editor, Some(None));
    assert!(delta.added_documents.is_none());
    assert!(delta.removed_documents.is_none());
    assert!(delta.added_editors.is_none());
    assert!(delta.removed_editors.is_none());
}

#[test]
fn ignore_huge_model_from_editor() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let large_model_string = "abc".repeat(1024);
    let _limit = SyncLimit::set(large_model_string.len() / 2);

    let model = fixture.model_service.create_model(large_model_string.as_str(), None, None, false);
    let editor = fixture.my_create_test_code_editor(Some(model.clone()));

    assert_eq!(fixture.deltas().len(), 1);
    fixture.clear_deltas();
    assert_eq!(fixture.deltas().len(), 0);
    editor.dispose();
    model.dispose();
}

#[test]
fn ignore_simple_widget_model() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let model = fixture.model_service.create_model("test", None, None, true);
    disposables.add_rc(model.clone());
    assert!(model.is_for_simple_widget());

    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 1);
    let delta = &deltas[0];
    assert_eq!(delta.new_active_editor, Some(None));
    assert!(delta.added_documents.is_none());
    assert!(delta.removed_documents.is_none());
    assert!(delta.added_editors.is_none());
    assert!(delta.removed_editors.is_none());
}

#[test]
fn ignore_editor_without_model() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let editor = fixture.my_create_test_code_editor(None);
    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 1);
    let delta = &deltas[0];
    assert_eq!(delta.new_active_editor, Some(None));
    assert!(delta.added_documents.is_none());
    assert!(delta.removed_documents.is_none());
    assert!(delta.added_editors.is_none());
    assert!(delta.removed_editors.is_none());

    editor.dispose();
}

#[test]
fn editor_with_model() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    fixture.clear_deltas();

    let model = fixture.model_service.create_model("farboo", None, None, false);
    let editor = fixture.my_create_test_code_editor(Some(model.clone()));

    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 2);
    let (first, second) = (&deltas[0], &deltas[1]);
    assert_eq!(first.added_documents.as_ref().unwrap().len(), 1);
    assert!(first.new_active_editor.is_none());
    assert!(first.removed_documents.is_none());
    assert!(first.added_editors.is_none());
    assert!(first.removed_editors.is_none());

    assert_eq!(second.added_editors.as_ref().unwrap().len(), 1);
    assert!(second.added_documents.is_none());
    assert!(second.removed_documents.is_none());
    assert!(second.removed_editors.is_none());
    assert!(second.new_active_editor.is_none());

    editor.dispose();
    model.dispose();
}

#[test]
fn fires_expected_add_remove_events_on_editor_lifecycle() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    fixture.clear_deltas();

    let removed_editor_events_from_service: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();
    let removed_via_editor_dispose: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();

    let model = fixture.model_service.create_model("farboo", None, None, false);
    let editor = fixture.my_create_test_code_editor(Some(model.clone()));
    let editor_id = format!("{},{}", editor.get_id(), model.id());

    let removed_from_service = removed_editor_events_from_service.clone();
    disposables.add(fixture.code_editor_service.registry().on_code_editor_remove().listen(move |editor_to_remove| {
        removed_from_service.borrow_mut().push(editor_to_remove.get_id());
    }));
    let removed_via_dispose = removed_via_editor_dispose.clone();
    let disposed_editor_id = editor.get_id();
    disposables.add(editor.on_did_dispose().listen(move |_| {
        removed_via_dispose.borrow_mut().push(disposed_editor_id.clone());
    }));

    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 2);

    let added_document_delta = deltas.iter().find(|delta| delta.added_documents.as_ref().is_some_and(|documents| documents.len() == 1)).expect("delta do documento");
    assert_eq!(
        added_document_delta.added_documents.as_ref().unwrap()[0].uri.to_string(false),
        model.uri().to_string(false)
    );
    assert!(added_document_delta.added_editors.is_none());
    assert!(added_document_delta.removed_editors.is_none());
    assert!(added_document_delta.removed_documents.is_none());

    let added_editor_delta = deltas
        .iter()
        .find(|delta| delta.added_editors.as_ref().is_some_and(|editors| editors.iter().any(|editor_dto| editor_dto.id == editor_id)))
        .expect("delta do editor");
    assert_eq!(fixture.main_thread_documents_and_editors.get_id_of_code_editor(&**editor.widget()), Some(editor_id.clone()));
    assert_eq!(added_editor_delta.added_editors.as_ref().unwrap().len(), 1);
    assert_eq!(added_editor_delta.added_editors.as_ref().unwrap()[0].id, editor_id);
    assert_eq!(added_editor_delta.added_editors.as_ref().unwrap()[0].document_uri.to_string(false), model.uri().to_string(false));

    editor.dispose();

    assert_eq!(*removed_via_editor_dispose.borrow(), vec![editor.get_id()]);
    assert_eq!(*removed_editor_events_from_service.borrow(), vec![editor.get_id()]);
    let deltas = fixture.deltas();
    let removed_editor_delta = deltas
        .iter()
        .find(|delta| delta.removed_editors.as_ref().is_some_and(|editors| editors.contains(&editor_id)))
        .expect("delta da remoção do editor");
    assert_eq!(removed_editor_delta.removed_editors, Some(vec![editor_id.clone()]));
    assert!(removed_editor_delta.removed_documents.is_none());

    assert_eq!(fixture.main_thread_documents_and_editors.get_id_of_code_editor(&**editor.widget()), None);
    assert!(fixture.main_thread_documents_and_editors.get_editor(&editor_id).is_none());

    model.dispose();

    let deltas = fixture.deltas();
    let removed_document_delta = deltas
        .iter()
        .find(|delta| delta.removed_documents.as_ref().is_some_and(|uris| uris.iter().any(|uri| uri.to_string(false) == model.uri().to_string(false))))
        .expect("delta da remoção do documento");
    assert_eq!(
        removed_document_delta.removed_documents.as_ref().unwrap().iter().map(|uri| uri.to_string(false)).collect::<Vec<_>>(),
        vec![model.uri().to_string(false)]
    );
}

#[test]
fn editor_with_disposed_disposing_model() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let model = fixture.model_service.create_model("farboo", None, None, false);
    let editor = fixture.my_create_test_code_editor(Some(model.clone()));

    // ignore things until now
    fixture.clear_deltas();

    fixture.model_service.destroy_model(&model.uri());
    let deltas = fixture.deltas();
    assert_eq!(deltas.len(), 1);
    let first = &deltas[0];

    assert!(first.new_active_editor.is_none());
    assert_eq!(first.removed_editors.as_ref().unwrap().len(), 1);
    assert_eq!(first.removed_documents.as_ref().unwrap().len(), 1);
    assert!(first.added_documents.is_none());
    assert!(first.added_editors.is_none());

    editor.dispose();
    model.dispose();
}

#[test]
fn dispose_removes_editor_listeners() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);

    let model = fixture.model_service.create_model("farboo", None, None, false);
    let editor = fixture.my_create_test_code_editor(Some(model.clone()));
    let main_thread_text_editor = fixture.main_thread_documents_and_editors.get_editor(&format!("{},{}", editor.get_id(), model.id())).expect("editor da API");

    let direct_property_changes = Rc::new(Cell::new(0usize));
    let counter = direct_property_changes.clone();
    disposables.add(main_thread_text_editor.on_properties_changed().listen(move |_| counter.set(counter.get() + 1)));
    let property_changes_before_update = fixture.proxy.property_changes.get();
    let direct_property_changes_before_update = direct_property_changes.get();
    editor.update_options(Some(&Dynamic::object([("lineNumbers", Dynamic::string("off"))])));
    assert!(fixture.proxy.property_changes.get() > property_changes_before_update);
    assert!(direct_property_changes.get() > direct_property_changes_before_update);
    let property_changes_after_update = fixture.proxy.property_changes.get();
    let direct_property_changes_after_update = direct_property_changes.get();

    fixture.main_thread_documents_and_editors.dispose();
    editor.update_options(Some(&Dynamic::object([("lineNumbers", Dynamic::string("on"))])));
    assert_eq!(fixture.proxy.property_changes.get(), property_changes_after_update);
    assert_eq!(direct_property_changes.get(), direct_property_changes_after_update);

    editor.dispose();
    model.dispose();
}
