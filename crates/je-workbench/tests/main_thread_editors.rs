//! Porte de `src/vs/workbench/api/test/browser/mainThreadEditors.test.ts` (suíte `MainThreadEditors`), caso a caso e na
//! mesma ordem, menos os quatro casos de fora abaixo. Entram os dois casos de `$tryApplyEdits`
//! (`applyWorkspaceEdit can control undo/redo stack 1` e `2`).
//!
//! Casos de fora (continuam no original e não viram `#[test]` aqui, para o placar não contar o que não foi porta fiel):
//! - `applyWorkspaceEdit returns false if model is changed by user`
//! - `issue #54773: applyWorkspaceEdit checks model version in race situation`
//! - `applyWorkspaceEdit: noop eol edit keeps undo stack clean`
//! - `applyWorkspaceEdit with only resource edit`
//!
//! Os quatro usam o `MainThreadBulkEdits` (`$tryApplyWorkspaceEdit`), que depende do `BulkEditService`, do `LabelService`
//! e do `IWorkingCopyFileService` completo, e não tem porte. Os dois casos portados não os tocam.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (ele faz também o papel do
//!   `disposables`); o `setup` é [`Fixture::new`].
//! - **Serviços.** O `InstantiationService` com o `ServiceCollection` do original (e os `createInstance`) é a passagem direta
//!   dos serviços aos construtores, como nas outras suítes de `MainThreadXxx`. O `IModelService` é o `ModelService` do
//!   porte (sem o `IInstantiationService`); o `IBulkEditService`, o `ILabelService`, o `ILogService`, o
//!   `IWorkspaceContextService`, o `IDialogService`, o `ILifecycleService`, o `IWorkingCopyService`, o
//!   `ITreeSitterLibraryService`, o `IEditorWorkerService` e o `ILanguageService` que o original registra só serviam ao
//!   `MainThreadBulkEdits` ou ao `ModelService` do original.
//! - **`ITextModelService`** do original devolve uma referência imortal com o `textEditorModel` do `ModelService`; nenhum
//!   caso portado o chama, e o dublê é o [`UnusedTextModelService`], que falha se usado.
//! - **`SingleProxyRPCProtocol(null)`** é o [`FixedProxyContext`] (ver `support/main_thread_support.rs`); `createTestCodeEditor(model)`
//!   usa o editor de teste padrão (foco no texto).
//! - **`editorLocator`** é o [`EditorLocator`]: `getEditor(id)` devolve o `testEditor` quando o id é `testEditorId`.
//! - `await model.undo()` é `model.undo()`: o `UndoRedoService` do porte é síncrono.
//! - Os `async` do original são [`run_with_faked_timers`] e `.await` das promessas do porte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

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

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use editor_support::test_clipboard_service::TestClipboardService;
use editor_support::test_code_editor::{create_test_code_editor, TestCodeEditorInstantiationOptions};
use editor_support::test_code_editor_service::TestCodeEditorService;
use editor_support::test_color_theme_service::TestThemeService;
use je_base::js_string::JsString;
use je_base::uri::URI;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model_service::ModelService;
use je_editor::services_model::IModelService;
use je_editor::range::Range;
use je_editor::services::abstract_code_editor_service::ICodeEditorRegistry;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::api::browser::main_thread_documents::MainThreadDocuments;
use je_workbench::api::browser::main_thread_editor::{IFocusTracker, MainThreadTextEditor};
use je_workbench::api::browser::main_thread_editors::{IMainThreadEditorLocator, MainThreadTextEditors};
use je_workbench::api::common::ext_host_protocol::{ApplyEditsOptions, MainThreadTextEditorsShape};
use je_workbench::common::editor::IEditorPane;
use je_workbench::services::editor::editor_service::IActiveCodeEditor;
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestFileService, TestTextResourcePropertiesService};
use main_thread_support::{
    FixedProxyContext, NoEventWorkingCopyFileService, RecordingProxy, StubQuickDiffModelService, StubTextFileService, UnusedTextModelService,
};

const EDITOR_ID: &str = "testEditorId";

/// `editorLocator`.
struct EditorLocator {
    test_editor: RefCell<Option<Rc<MainThreadTextEditor>>>,
}

impl IMainThreadEditorLocator for EditorLocator {
    fn get_editor(&self, id: &str) -> Option<Rc<MainThreadTextEditor>> {
        if id == EDITOR_ID { self.test_editor.borrow().clone() } else { None }
    }

    fn find_text_editor_id_for(&self, _editor_pane: &dyn IEditorPane) -> Option<String> {
        None
    }

    fn get_id_of_code_editor(&self, _code_editor: &dyn IActiveCodeEditor) -> Option<String> {
        None
    }
}

/// `{ onGainedFocus() { }, onLostFocus() { } }`.
struct NoopFocusTracker;

impl IFocusTracker for NoopFocusTracker {
    fn on_gained_focus(&self) {}

    fn on_lost_focus(&self) {}
}

/// O estado que o `setup` da suíte deixa nas variáveis do `suite`.
struct Fixture {
    model_service: Rc<ModelService>,
    editors: Rc<MainThreadTextEditors>,
    /// `editorLocator` (o `MainThreadTextEditors` só guarda um `Weak`).
    _editor_locator: Rc<EditorLocator>,
}

impl Fixture {
    /// `setup`.
    fn new(disposables: &LeakGuard) -> Self {
        let config_service = TestConfigurationService::new(None);
        let configuration_service: Rc<dyn IConfigurationService> = config_service.clone();

        let model_service = disposables.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service.clone()),
            UndoRedoService::for_tests(),
        ));
        let code_editor_service = Rc::new(TestCodeEditorService::new(TestThemeService::new_default()));
        let code_editor_registry = disposables.add_rc(code_editor_service.registry().clone());
        let code_editor_registry: Rc<dyn ICodeEditorRegistry> = code_editor_registry;

        let file_service = disposables.add_rc(TestFileService::new());
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract,)));

        let proxy = Rc::new(RecordingProxy::default());
        let ext_host_context = FixedProxyContext::new(proxy);

        let documents = disposables.add_rc(MainThreadDocuments::new(
            &ext_host_context,
            model_service.clone(),
            Rc::new(StubTextFileService),
            file_service.clone(),
            Rc::new(UnusedTextModelService),
            TestEnvironmentService::new(),
            uri_identity_service.clone(),
            Rc::new(NoEventWorkingCopyFileService),
            TestPathService::new(None, None),
        ));

        // Create editor locator
        let editor_locator = Rc::new(EditorLocator { test_editor: RefCell::new(None) });
        let locator: Weak<dyn IMainThreadEditorLocator> = Rc::downgrade(&editor_locator) as Weak<dyn IMainThreadEditorLocator>;

        let editors = MainThreadTextEditors::new(
            locator,
            &ext_host_context,
            code_editor_registry,
            Rc::new(TestEditorService),
            TestEditorGroupsService::new(None),
            configuration_service,
            Rc::new(StubQuickDiffModelService),
            uri_identity_service,
        );
        disposables.add_rc(editors.clone());

        // Create a test code editor using the helper
        let model = model_service.create_model("Hello world!", None, Some(&URI::parse("foo:existing", false)), false);
        // O `ModelService` do porte não é filho do `InstantiationService` do caso, então não descarta o modelo no teardown como o do original.
        disposables.add_rc(model.clone());
        let test_code_editor = disposables.add_rc(create_test_code_editor(Some(model.clone()), TestCodeEditorInstantiationOptions::default()));

        let clipboard_service = Rc::new(TestClipboardService::new());
        let test_editor = disposables.add_rc(MainThreadTextEditor::new(
            EDITOR_ID.to_string(),
            model,
            test_code_editor.widget().clone(),
            Rc::new(NoopFocusTracker),
            documents,
            model_service.clone(),
            clipboard_service,
        ));
        *editor_locator.test_editor.borrow_mut() = Some(test_editor);

        Self { model_service, editors, _editor_locator: editor_locator }
    }
}

/// `{ range, text, forceMoveMarkers: false }`.
fn edit(range: Range, text: &str) -> SingleEditOperation {
    SingleEditOperation::new(range, Some(JsString::from(text)), false)
}

/// `{ undoStopBefore, undoStopAfter }` de `$tryApplyEdits`.
fn undo_stops(undo_stop_before: bool, undo_stop_after: bool) -> ApplyEditsOptions {
    ApplyEditsOptions { set_end_of_line: None, undo_stop_before, undo_stop_after }
}

#[test]
fn apply_workspace_edit_can_control_undo_redo_stack_1() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    run_with_faked_timers(async move {
        let model = fixture.model_service.get_model(&URI::parse("foo:existing", false)).expect("modelo existente");

        let edit1 = edit(Range::new(1, 1, 1, 2), "h");

        let applied1 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit1], undo_stops(false, false)).await.expect("applied1");
        assert!(applied1);
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");

        let edit2 = edit(Range::new(1, 2, 1, 6), "ELLO");

        let applied2 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit2], undo_stops(false, false)).await.expect("applied2");
        assert!(applied2);
        assert_eq!(model.get_value().to_string_lossy(), "hELLO world!");

        model.undo();
        assert_eq!(model.get_value().to_string_lossy(), "Hello world!");
    });
}

#[test]
fn apply_workspace_edit_can_control_undo_redo_stack_2() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&disposables);
    run_with_faked_timers(async move {
        let model = fixture.model_service.get_model(&URI::parse("foo:existing", false)).expect("modelo existente");

        let edit1 = edit(Range::new(1, 1, 1, 2), "h");

        let applied1 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit1], undo_stops(false, false)).await.expect("applied1");
        assert!(applied1);
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");

        let edit2 = edit(Range::new(1, 2, 1, 6), "ELLO");

        let applied2 = fixture.editors.rpc_try_apply_edits(EDITOR_ID, model.get_version_id() as f64, vec![edit2], undo_stops(true, false)).await.expect("applied2");
        assert!(applied2);
        assert_eq!(model.get_value().to_string_lossy(), "hELLO world!");

        model.undo();
        assert_eq!(model.get_value().to_string_lossy(), "hello world!");
    });
}
