//! Porte de `src/vs/workbench/contrib/limitIndicator/test/browser/limitIndicator.test.ts` (suíte
//! `LimitIndicatorContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do [`Fixture`], e o `disposables` da suíte é o próprio
//!   guarda. O `setup` (`createCodeEditorServices`, `MarkerService`, `MarkerDecorationsService`, o editor, o `editorService`
//!   e o `languageStatusService` dublês e a contribuição) é [`Fixture::new`].
//! - `await changeMarkers(...)` (`Event.toPromise(markerService.onMarkerChanged)`) é esvaziar as microtarefas, porque o
//!   `MarkerService` dispara `onMarkerChanged` numa microtarefa.
//! - `statuses: Set<ILanguageStatus>` é uma lista de `Rc` que preserva a ordem de inserção; a identidade (`===`) é
//!   `Rc::ptr_eq`. Os `mock<IEditorService>()` e `mock<ILanguageStatusService>()` são tipos de teste com só os membros que o
//!   original sobrescreve (os demais lançam, como o `mock`).
//! - `mock<IDiffEditor>()` é o `ActiveTextEditorControl::Diff`.
//! - O `mock<ColorDetector>()` e o `mock<FoldingController>()` do penúltimo caso não existem em Rust (a busca é por tipo
//!   concreto): entram o `ColorDetector` e o `FoldingController` reais, sem provedores, e os relatores deles recebem o
//!   `update(3, 1)` e o `update(10, 5)` do original depois de o cálculo das dobras assentar. O `DecoratorLimitReporter` e o
//!   `RangesLimitReporter` avulsos do original são, portanto, os relatores das contribuições reais.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

/// O suporte de editor do `je-editor/tests/support` que o `createCodeEditorServices` precisa, no mesmo módulo pai (os
/// arquivos se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod mock_environment_service;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_text_resource_properties_service;
    pub mod test_theme_service;
}

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use editor_support::mock_environment_service::MockEnvironmentService;
use editor_support::test_code_editor::{
    CodeEditorServices, TestCodeEditor, TestCodeEditorInstantiationOptions, create_code_editor_services, instantiate_test_code_editor,
};
use editor_support::test_text_resource_properties_service::TestTextResourcePropertiesService;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime::run_microtasks;
use je_base::uri::URI;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::color_detector::{ColorDetector, ColorDetectorServices};
use je_editor::folding::FoldingLimitReporter;
use je_editor::folding::folding_controller::{FoldingController, FoldingControllerServices};
use je_editor::language_feature_debounce::{ILanguageFeatureDebounceService, LanguageFeatureDebounceService};
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::model_service::ModelService;
use je_editor::services::marker_decorations::IMarkerDecorationsService;
use je_editor::services::marker_decorations_service::MarkerDecorationsService;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::{IMarkerService, MarkerChangeOptions, MarkerData, MarkerSeverity};
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::contrib::limit_indicator::{DiagnosticDecorationLimitReporter, LimitIndicatorContribution};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IActiveCodeEditor, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::language_status::language_status_service::{ILanguageStatus, ILanguageStatusService};

/// `mock<IEditorService>()` com `onDidActiveEditorChange` e `activeTextEditorControl`.
struct FakeEditorService {
    active_editor: RefCell<Option<ActiveTextEditorControl>>,
    active_editor_changed: Rc<Emitter<()>>,
}

impl IEditorService for FakeEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        panic!("Method not implemented.")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        panic!("Method not implemented.")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        self.active_editor.borrow().clone()
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        panic!("Method not implemented.")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        panic!("Method not implemented.")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        panic!("Method not implemented.")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("Method not implemented.")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        panic!("Method not implemented.")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        panic!("Method not implemented.")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        panic!("Method not implemented.")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        panic!("Method not implemented.")
    }

    fn on_did_active_editor_change(&self) -> Event<()> {
        self.active_editor_changed.event()
    }
}

/// `mock<ILanguageStatusService>()` com `addStatus`.
struct FakeLanguageStatusService {
    statuses: Rc<RefCell<Vec<Rc<ILanguageStatus>>>>,
}

impl ILanguageStatusService for FakeLanguageStatusService {
    fn on_did_change(&self) -> Event<()> {
        panic!("Method not implemented.")
    }

    fn add_status(&self, status: ILanguageStatus) -> DisposableRef {
        let status = Rc::new(status);
        self.statuses.borrow_mut().push(Rc::clone(&status));
        let statuses = Rc::clone(&self.statuses);
        to_disposable(move || statuses.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &status)))
    }

    fn get_language_status(&self, _model: &TextModel) -> Vec<Rc<ILanguageStatus>> {
        panic!("Method not implemented.")
    }
}

struct Fixture {
    services: Rc<CodeEditorServices>,
    model_service: Rc<ModelService>,
    marker_service: Rc<MarkerService>,
    decoration_service: Rc<MarkerDecorationsService>,
    editor: Rc<TestCodeEditor>,
    editor_service: Rc<FakeEditorService>,
    language_status_service: Rc<FakeLanguageStatusService>,
    statuses: Rc<RefCell<Vec<Rc<ILanguageStatus>>>>,
    contribution: RefCell<Rc<LimitIndicatorContribution>>,
    /// O `disposables` da suíte; o último campo, para o descarte e a conferência de vazamentos virem depois do resto.
    ds: LeakGuard,
}

fn file_uri(path: &str) -> URI {
    URI::file(path, je_base::path::Environment::platform(&je_base::path::ProcessEnvironment))
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_code_editor_services(&ds.store(), None);
        let config_service = TestConfigurationService::new(None);
        let _ = config_service.set_user_configuration("files", Dynamic::object([("eol", Dynamic::string("\n"))]), None);
        let configuration_service: Rc<dyn IConfigurationService> = config_service;
        let model_service = ds.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service),
            UndoRedoService::for_tests(),
        ));
        let marker_service = ds.add_rc(MarkerService::new());
        let decoration_service = ds.add_rc(MarkerDecorationsService::new(model_service.clone(), marker_service.clone()));
        let editor = Self::create_editor_in(&ds, &services, &model_service, &decoration_service);
        let active_editor_changed: Rc<Emitter<()>> = Emitter::new();
        ds.add_rc(active_editor_changed.clone());
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let editor_service = Rc::new(FakeEditorService {
            active_editor: RefCell::new(Some(ActiveTextEditorControl::Code(editor.widget().clone()))),
            active_editor_changed,
        });
        let language_status_service = Rc::new(FakeLanguageStatusService { statuses: Rc::clone(&statuses) });
        let contribution = ds.add_rc(LimitIndicatorContribution::new(editor_service.clone(), language_status_service.clone()));
        Self {
            services,
            model_service,
            marker_service,
            decoration_service,
            editor,
            editor_service,
            language_status_service,
            statuses,
            contribution: RefCell::new(contribution),
            ds,
        }
    }

    fn create_model(&self) -> Rc<TextModel> {
        Self::create_model_in(&self.ds, &self.model_service, None)
    }

    fn create_model_in(ds: &LeakGuard, model_service: &Rc<ModelService>, resource: Option<&URI>) -> Rc<TextModel> {
        let text = vec!["value"; 5100].join("\n");
        ds.add_rc(model_service.create_model(text.as_str(), None, resource, false))
    }

    fn create_editor(&self) -> Rc<TestCodeEditor> {
        Self::create_editor_in(&self.ds, &self.services, &self.model_service, &self.decoration_service)
    }

    fn create_editor_in(
        ds: &LeakGuard,
        services: &Rc<CodeEditorServices>,
        model_service: &Rc<ModelService>,
        decoration_service: &Rc<MarkerDecorationsService>,
    ) -> Rc<TestCodeEditor> {
        let model = Self::create_model_in(ds, model_service, None);
        let editor = ds.add_rc(instantiate_test_code_editor(services, Some(model), &TestCodeEditorInstantiationOptions::default().options));
        let marker_decorations_service: Rc<dyn IMarkerDecorationsService> = decoration_service.clone();
        let reporter = editor
            .register_and_instantiate_contribution(DiagnosticDecorationLimitReporter::ID, |widget, _| DiagnosticDecorationLimitReporter::new(widget, marker_decorations_service));
        ds.add_rc(reporter);
        editor
    }

    fn set_active_editor(&self, control: Option<ActiveTextEditorControl>) {
        *self.editor_service.active_editor.borrow_mut() = control;
        self.editor_service.active_editor_changed.fire(());
    }

    fn change_markers(&self, model: &TextModel, count: usize, owner: &str, is_truncated: bool) {
        let markers = (0..count)
            .map(|index| MarkerData::new(MarkerSeverity::ERROR, format!("problem {}", index + 1), index as i64 + 1, 1, index as i64 + 1, 2))
            .collect();
        self.marker_service.change_one(owner, &model.uri(), markers, Some(MarkerChangeOptions { is_truncated }));
        run_microtasks();
    }

    fn model(editor: &TestCodeEditor) -> Rc<TextModel> {
        editor.get_model().expect("o editor tem modelo")
    }

    fn details(&self) -> Vec<String> {
        self.statuses.borrow().iter().map(|status| status.detail.clone()).collect()
    }

    fn count(&self) -> usize {
        self.statuses.borrow().len()
    }

    fn first_status(&self) -> Rc<ILanguageStatus> {
        Rc::clone(&self.statuses.borrow()[0])
    }

    fn update_options(&self, key: &str, value: Dynamic) {
        self.editor.update_options(Some(&Dynamic::object([(key, value)])));
    }

    fn code(editor: &Rc<TestCodeEditor>) -> ActiveTextEditorControl {
        ActiveTextEditorControl::Code(editor.widget().clone())
    }
}

#[test]
fn distinguishes_diagnostic_truncation_from_editor_highlight_limits() {
    let f = Fixture::new();
    let mut details: Vec<Vec<String>> = Vec::new();
    f.change_markers(&Fixture::model(&f.editor), 5000, "test", false);
    details.push(f.details());
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    details.push(f.details());
    f.change_markers(&Fixture::model(&f.editor), 1001, "test", true);
    details.push(f.details());
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", true);
    details.push(f.details());
    f.change_markers(&Fixture::model(&f.editor), 5000, "test", false);
    details.push(f.details());

    assert_eq!(
        details,
        vec![
            Vec::<String>::new(),
            vec!["Only 5000 diagnostics are highlighted in the editor for performance reasons.".to_string()],
            vec!["Some diagnostics are not shown for performance reasons.".to_string()],
            vec!["Some diagnostics are not shown, and only 5000 diagnostics are highlighted in the editor for performance reasons.".to_string()],
            Vec::<String>::new(),
        ]
    );
}

#[test]
fn shows_existing_overflow_when_the_contribution_is_initialized() {
    let f = Fixture::new();
    f.contribution.borrow().dispose();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    *f.contribution.borrow_mut() = f.ds.add_rc(LimitIndicatorContribution::new(f.editor_service.clone(), f.language_status_service.clone()));

    let ids: Vec<String> = f.statuses.borrow().iter().map(|status| status.id.clone()).collect();
    assert_eq!(ids, vec!["diagnosticsLimitInfo".to_string()]);
}

#[test]
fn follows_model_changes_in_the_same_editor_including_an_empty_editor() {
    let f = Fixture::new();
    let affected = Fixture::model(&f.editor);
    let unaffected = f.create_model();
    f.change_markers(&affected, 5001, "test", false);
    let mut counts = vec![f.count()];

    f.editor.set_model(Some(unaffected));
    counts.push(f.count());
    f.editor.set_model(Some(affected.clone()));
    counts.push(f.count());
    f.editor.set_model(None);
    counts.push(f.count());
    f.editor.set_model(Some(affected.clone()));
    counts.push(f.count());
    affected.dispose();
    counts.push(f.count());

    assert_eq!(counts, vec![1, 0, 1, 0, 1, 0]);
}

#[test]
fn follows_the_active_split_editor_and_ignores_background_diagnostics() {
    let f = Fixture::new();
    let affected = Fixture::model(&f.editor);
    let other_editor = f.create_editor();
    f.change_markers(&affected, 5001, "test", false);
    let mut counts = vec![f.count()];

    f.set_active_editor(Some(Fixture::code(&other_editor)));
    counts.push(f.count());
    f.change_markers(&affected, 5100, "test", false);
    counts.push(f.count());
    f.set_active_editor(Some(Fixture::code(&f.editor)));
    counts.push(f.count());
    other_editor.set_model(Some(affected.clone()));
    f.set_active_editor(Some(Fixture::code(&other_editor)));
    counts.push(f.count());
    f.change_markers(&affected, 5000, "test", false);
    counts.push(f.count());
    f.set_active_editor(Some(Fixture::code(&f.editor)));
    counts.push(f.count());
    f.change_markers(&affected, 5001, "test", false);
    counts.push(f.count());
    f.set_active_editor(None);
    counts.push(f.count());

    assert_eq!(counts, vec![1, 0, 0, 1, 1, 0, 0, 1, 0]);
}

#[test]
fn does_not_recreate_the_active_warning_for_another_resource() {
    let f = Fixture::new();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    let status = f.first_status();
    f.change_markers(&f.create_model(), 5100, "test", false);

    assert!(Rc::ptr_eq(&f.first_status(), &status));
}

#[test]
fn does_not_recreate_the_warning_while_diagnostics_remain_over_the_limit() {
    let f = Fixture::new();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    let status = f.first_status();
    f.change_markers(&Fixture::model(&f.editor), 5100, "test", false);

    assert!(Rc::ptr_eq(&f.first_status(), &status));
}

#[test]
fn uses_the_modified_editor_in_a_diff() {
    let f = Fixture::new();
    let modified_editor = f.create_editor();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    let original: Rc<dyn IActiveCodeEditor> = f.editor.widget().clone();
    let modified: Rc<dyn IActiveCodeEditor> = modified_editor.widget().clone();
    f.set_active_editor(Some(ActiveTextEditorControl::Diff { original, modified }));
    let mut counts = vec![f.count()];
    f.change_markers(&Fixture::model(&modified_editor), 5001, "test", false);
    counts.push(f.count());
    f.change_markers(&Fixture::model(&modified_editor), 5000, "test", false);
    counts.push(f.count());

    assert_eq!(counts, vec![0, 1, 0]);
}

#[test]
fn respects_validation_visibility_and_read_only_changes() {
    let f = Fixture::new();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    let mut counts = vec![f.count()];

    f.update_options("renderValidationDecorations", Dynamic::string("off"));
    counts.push(f.count());
    f.update_options("renderValidationDecorations", Dynamic::string("on"));
    counts.push(f.count());
    f.update_options("readOnly", Dynamic::Bool(true));
    counts.push(f.count());
    f.update_options("renderValidationDecorations", Dynamic::string("editable"));
    counts.push(f.count());
    f.update_options("readOnly", Dynamic::Bool(false));
    counts.push(f.count());

    assert_eq!(counts, vec![1, 0, 1, 1, 0, 1]);
}

#[test]
fn clears_and_restores_the_warning_when_diagnostics_are_filtered() {
    let f = Fixture::new();
    let model = Fixture::model(&f.editor);
    f.change_markers(&model, 5001, "test", false);
    let mut counts = vec![f.count()];

    let filter = f.ds.add(f.marker_service.install_resource_filter(&model.uri(), "test"));
    run_microtasks();
    counts.push(f.count());
    filter.dispose();
    run_microtasks();
    counts.push(f.count());

    assert_eq!(counts, vec![1, 0, 1]);
}

#[test]
fn keeps_configure_actions_for_folding_and_color_limits() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.contribution.borrow().dispose();

        let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let language_features_service = Rc::new(LanguageFeaturesService::new());
        let debounce_service: Rc<dyn ILanguageFeatureDebounceService> =
            Rc::new(LanguageFeatureDebounceService::new(NullLogService::shared(), &*MockEnvironmentService::new(true, false)));
        let color_detector = f.editor.register_and_instantiate_contribution(ColorDetector::ID, |widget, _| {
            ColorDetector::new(
                widget,
                ColorDetectorServices {
                    configuration_service,
                    language_features_service: Rc::clone(&language_features_service),
                    language_feature_debounce_service: Rc::clone(&debounce_service),
                },
            )
        });
        f.ds.add_rc(color_detector.clone());
        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        let folding_controller = f.editor.register_and_instantiate_contribution(FoldingController::ID, |widget, _| {
            FoldingController::new(
                widget,
                FoldingControllerServices {
                    context_key_service,
                    language_features_service: Rc::clone(&language_features_service),
                    language_feature_debounce_service: Rc::clone(&debounce_service),
                },
            )
        });
        f.ds.add_rc(folding_controller.clone());
        // o cálculo inicial das dobras assenta antes de o relator receber os valores do caso
        let _ = folding_controller.get_folding_model().expect("o controlador tem o modelo de dobras").await;

        color_detector.limit_reporter().update(3, Some(1));
        folding_controller.limit_reporter().update(10, Some(5));
        *f.contribution.borrow_mut() = f.ds.add_rc(LimitIndicatorContribution::new(f.editor_service.clone(), f.language_status_service.clone()));
        f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);

        let summary: Vec<(String, String, Option<(String, Vec<String>, String)>)> = f
            .statuses
            .borrow()
            .iter()
            .map(|status| {
                let command = status.command.as_ref().map(|command| {
                    let arguments = command
                        .arguments
                        .iter()
                        .flatten()
                        .map(|argument| match argument {
                            Dynamic::String(text) => text.to_string_lossy(),
                            other => panic!("argumento inesperado: {other:?}"),
                        })
                        .collect();
                    (command.id.clone(), arguments, command.title.clone())
                });
                (status.id.clone(), status.detail.clone(), command)
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "decoratorsLimitInfo".to_string(),
                    "only 1 shown for performance reasons".to_string(),
                    Some(("workbench.action.openSettings".to_string(), vec!["editor.colorDecoratorsLimit".to_string()], "Configure".to_string())),
                ),
                (
                    "foldingLimitInfo".to_string(),
                    "only 5 shown for performance reasons".to_string(),
                    Some(("workbench.action.openSettings".to_string(), vec!["editor.foldingMaximumRegions".to_string()], "Configure".to_string())),
                ),
                (
                    "diagnosticsLimitInfo".to_string(),
                    "Only 5000 diagnostics are highlighted in the editor for performance reasons.".to_string(),
                    None,
                ),
            ]
        );
    });
}

#[test]
fn disposes_its_warning_and_listeners() {
    let f = Fixture::new();
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);
    f.contribution.borrow().dispose();
    f.change_markers(&Fixture::model(&f.editor), 5100, "test", false);
    f.update_options("renderValidationDecorations", Dynamic::string("off"));
    f.update_options("renderValidationDecorations", Dynamic::string("on"));
    f.set_active_editor(Some(Fixture::code(&f.editor)));
    let other = Fixture::create_model_in(&f.ds, &f.model_service, Some(&file_uri("/other.txt")));
    f.editor.set_model(Some(other));
    f.change_markers(&Fixture::model(&f.editor), 5001, "test", false);

    assert_eq!(f.count(), 0);
}
