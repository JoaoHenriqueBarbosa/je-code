//! Testes do `FoldingController`, do `RangesLimitReporter` e das ações `editor.fold*` (`folding.ts` não tem teste no
//! oráculo): o limite de regiões, o cálculo por indentação sem provedor, o provedor de sintaxe com o recuo de reserva,
//! as ações principais, a navegação entre dobras e o estado de visão.
//!
//! O texto de todos os casos tem três regiões de indentação: `[1, 5]`, `[2, 3]` e `[7, 8]` (o fim de uma linguagem sem
//! `offSide` deixa a linha de fechamento visível). O controlador é criado direto sobre o editor de teste e as ações
//! rodam por `FoldingAction::run_with_controller`, porque o editor de teste não registra contribuições.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::run_with_faked_timers;
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_editor::code_editor_widget::{ICodeEditor, SelectionOrRange};
use je_editor::folding::folding_controller::{
    FoldingAction, FoldingActionKind, FoldingController, FoldingControllerServices, FoldingStateMemento,
};
use je_editor::folding::folding_model::FoldingModel;
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange, FoldingRangeProvider};
use je_editor::language_feature_debounce::{ILanguageFeatureDebounceService, LanguageFeatureDebounceService};
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::LanguageSelector;
use je_editor::language_service::I_LANGUAGE_SERVICE;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::position::Position;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextModelServices};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use support::mock_environment_service::MockEnvironmentService;
use support::test_code_editor::{
    TestCodeEditor, TestCodeEditorInstantiationOptions, create_code_editor_services, instantiate_test_code_editor,
};
use support::test_text_model::resolve_options;

const TEXT: &str = "function a() {\n    if (x) {\n        y();\n    }\n    z();\n}\nfunction b() {\n    w();\n}";

/// Um provedor de sintaxe que devolve sempre o mesmo resultado (`None` é o `null` do provedor).
struct FixedProvider(Option<Vec<FoldingRange>>);

impl FoldingRangeProvider for FixedProvider {
    fn provide_folding_ranges(
        &self,
        _model: &Rc<TextModel>,
        _context: &FoldingContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<FoldingRange>> {
        Promise::resolved(self.0.clone())
    }
}

struct Fixture {
    disposables: Rc<DisposableStore>,
    editor: Rc<TestCodeEditor>,
    controller: Rc<FoldingController>,
    language_features_service: Rc<LanguageFeaturesService>,
}

impl Fixture {
    /// `maximum_regions` é o `foldingMaximumRegions` do editor; `strategy` é o `foldingStrategy`.
    fn new(maximum_regions: Option<f64>, strategy: Option<&str>) -> Self {
        Self::with_text(TEXT, maximum_regions, strategy)
    }

    fn with_text(text: &str, maximum_regions: Option<f64>, strategy: Option<&str>) -> Self {
        let disposables = DisposableStore::new();
        let services = create_code_editor_services(&disposables, None);
        let model = TextModel::new_with_language_service(
            text.into(),
            PLAINTEXT_LANGUAGE_ID,
            resolve_options(&RelaxedTextModelCreationOptions::default()),
            None,
            services.undo_redo_service.clone(),
            Some(TextModelServices {
                language_service: services.get(I_LANGUAGE_SERVICE),
                language_configuration_service: services.language_configuration_service.clone(),
            }),
        );
        let owned = model.clone();
        disposables.add(to_disposable(move || owned.dispose()));

        let options = TestCodeEditorInstantiationOptions::default().options;
        if let Dynamic::Object(object) = &options {
            if let Some(maximum_regions) = maximum_regions {
                object.set(&JsString::from("foldingMaximumRegions"), Dynamic::Number(maximum_regions));
            }
            if let Some(strategy) = strategy {
                object.set(&JsString::from("foldingStrategy"), Dynamic::from(strategy));
            }
        }
        let editor = disposables.add_rc(instantiate_test_code_editor(&services, Some(model), &options));

        let log_service = NullLogService::shared();
        let environment_service = MockEnvironmentService::new(true, false);
        let language_feature_debounce_service: Rc<dyn ILanguageFeatureDebounceService> =
            Rc::new(LanguageFeatureDebounceService::new(log_service, &*environment_service));
        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        let language_features_service = Rc::new(LanguageFeaturesService::new());
        let controller = FoldingController::new(
            editor.widget(),
            FoldingControllerServices {
                context_key_service,
                language_features_service: Rc::clone(&language_features_service),
                language_feature_debounce_service,
            },
        );
        disposables.add_rc(controller.clone());
        Self { disposables, editor, controller, language_features_service }
    }

    fn register_provider(&self, ranges: Option<Vec<FoldingRange>>) {
        let provider: Rc<dyn FoldingRangeProvider> = Rc::new(FixedProvider(ranges));
        self.disposables.add(self.language_features_service.folding_range_provider.register(LanguageSelector::from(PLAINTEXT_LANGUAGE_ID), provider));
    }

    /// Espera o cálculo assíncrono das regiões e devolve o modelo de dobras.
    async fn settle(&self) -> Rc<FoldingModel> {
        let promise = self.controller.get_folding_model().expect("o controlador tem o modelo de dobras");
        match promise.await {
            Ok(Some(folding_model)) => folding_model,
            _ => panic!("o cálculo das regiões de dobra não devolveu o modelo"),
        }
    }

    async fn run(&self, kind: FoldingActionKind, args: &Dynamic) {
        let action = FoldingAction::new(kind);
        let result = action.run_with_controller(&self.controller, self.editor.widget(), args).await;
        assert!(result.is_ok(), "a ação {:?} falhou", kind);
    }

    fn set_cursor(&self, line_number: i32) {
        self.editor.set_position(Position::new(line_number, 1), None);
    }

    fn collapsed(folding_model: &FoldingModel) -> Vec<bool> {
        let regions = folding_model.regions();
        (0..regions.length()).map(|index| regions.is_collapsed(index)).collect()
    }

    fn finish(self) {
        self.disposables.dispose();
    }
}

fn no_args() -> Dynamic {
    Dynamic::object(std::iter::empty::<(&str, Dynamic)>())
}

#[test]
fn indent_ranges_are_computed_without_providers() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;
        let regions = folding_model.regions();
        let lines: Vec<(u32, u32)> = (0..regions.length()).map(|i| (regions.get_start_line_number(i), regions.get_end_line_number(i))).collect();
        assert_eq!(lines, vec![(1, 5), (2, 3), (7, 8)]);
        let state = f.controller.save_folding_state().expect("o modelo de dobras existe");
        assert_eq!(state.provider.as_deref(), Some("indent"));
        assert_eq!(f.controller.limit_reporter().computed(), 3);
        assert_eq!(f.controller.limit_reporter().limited(), None);
        f.finish();
    });
}

#[test]
fn limit_reporter_reports_computed_and_limited_ranges() {
    run_with_faked_timers(async {
        // `foldingMaximumRegions` é cortado em no mínimo 10 (EditorIntOption 5000, 10, 65000): são 12 regiões para 10.
        let text = (0..12).map(|i| format!("function f{i}() {{\n    w();\n}}")).collect::<Vec<_>>().join("\n");
        let f = Fixture::with_text(&text, Some(10.0), None);
        let reporter = f.controller.limit_reporter().clone();
        let changes = Rc::new(Cell::new(0));
        let counter = Rc::clone(&changes);
        let _listener = reporter.on_did_change().listen(move |_| counter.set(counter.get() + 1));

        let folding_model = f.settle().await;
        assert_eq!(reporter.computed(), 12);
        assert_eq!(reporter.limited(), Some(10));
        assert_eq!(folding_model.regions().length(), 10);
        assert!(changes.get() >= 1, "o relator avisa quando o resultado muda");

        // o mesmo resultado de novo não dispara outro aviso
        let before = changes.get();
        f.controller.trigger_folding_model_changed();
        f.settle().await;
        assert_eq!(changes.get(), before);
        f.finish();
    });
}

#[test]
fn syntax_provider_wins_and_falls_back_to_indentation_on_null() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        f.register_provider(Some(vec![FoldingRange { start: 1, end: 2, kind: None }]));
        f.controller.trigger_folding_model_changed();
        let folding_model = f.settle().await;
        let regions = folding_model.regions();
        assert_eq!((regions.length(), regions.get_start_line_number(0), regions.get_end_line_number(0)), (1, 1, 2));
        assert_eq!(f.controller.save_folding_state().unwrap().provider.as_deref(), Some("syntax"));
        f.finish();

        let f = Fixture::new(None, None);
        f.register_provider(None);
        f.controller.trigger_folding_model_changed();
        let folding_model = f.settle().await;
        assert_eq!(folding_model.regions().length(), 3, "o provedor que devolve null cai na indentação");
        f.finish();
    });
}

#[test]
fn indentation_strategy_ignores_syntax_providers() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, Some("indentation"));
        f.register_provider(Some(vec![FoldingRange { start: 1, end: 2, kind: None }]));
        f.controller.trigger_folding_model_changed();
        let folding_model = f.settle().await;
        assert_eq!(folding_model.regions().length(), 3);
        assert_eq!(f.controller.save_folding_state().unwrap().provider.as_deref(), Some("indent"));
        f.finish();
    });
}

#[test]
fn fold_unfold_and_toggle_use_the_selected_lines() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;
        f.set_cursor(2);

        f.run(FoldingActionKind::Fold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, true, false]);
        // o mesmo lugar já dobrado: dobra o primeiro pai aberto
        f.run(FoldingActionKind::Fold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, false]);

        f.set_cursor(1);
        f.run(FoldingActionKind::Unfold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, true, false]);

        f.run(FoldingActionKind::ToggleFold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, false]);
        f.run(FoldingActionKind::ToggleFold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, true, false]);
        f.finish();
    });
}

#[test]
fn recursive_all_and_level_actions() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;

        f.set_cursor(1);
        f.run(FoldingActionKind::FoldRecursively, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, false]);
        f.run(FoldingActionKind::UnfoldRecursively, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, false]);

        f.run(FoldingActionKind::FoldAll, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, true]);
        f.run(FoldingActionKind::UnfoldAll, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, false]);

        // o nível 2 só tem a região [2, 3]; a seleção na linha 7 não a bloqueia
        f.set_cursor(7);
        f.run(FoldingActionKind::FoldLevel(2), &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, true, false]);

        // as dobras abertas fora da seleção
        f.run(FoldingActionKind::UnfoldAll, &no_args()).await;
        f.run(FoldingActionKind::FoldAllExcept, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, false]);
        f.finish();
    });
}

#[test]
fn fold_with_levels_and_direction_arguments() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;
        f.set_cursor(3);

        let up = Dynamic::object([("levels", Dynamic::Number(2.0)), ("direction", Dynamic::from("up"))]);
        f.run(FoldingActionKind::Fold, &up).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![true, true, false]);
        // a seleção foi movida para fora da região dobrada: o desdobrar nomeia a linha 3 (índice 2, de base 0)
        let unfold_up = Dynamic::object([
            ("levels", Dynamic::Number(2.0)),
            ("direction", Dynamic::from("up")),
            ("selectionLines", Dynamic::array([Dynamic::Number(2.0)])),
        ]);
        f.run(FoldingActionKind::Unfold, &unfold_up).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, false]);

        // `selectionLines` é de base 0: a linha 6 do documento é o índice 6 (a região [7, 8])
        let lines = Dynamic::object([("selectionLines", Dynamic::array([Dynamic::Number(6.0)]))]);
        f.run(FoldingActionKind::Fold, &lines).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, true]);
        f.finish();
    });
}

#[test]
fn goto_actions_move_the_cursor_to_fold_starts() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        f.settle().await;

        f.set_cursor(3);
        f.run(FoldingActionKind::GotoParentFold, &no_args()).await;
        assert_eq!(f.editor.get_selection(), Some(Selection::new(2, 1, 2, 1)));
        f.run(FoldingActionKind::GotoParentFold, &no_args()).await;
        assert_eq!(f.editor.get_selection(), Some(Selection::new(1, 1, 1, 1)));

        f.run(FoldingActionKind::GotoNextFold, &no_args()).await;
        assert_eq!(f.editor.get_selection(), Some(Selection::new(7, 1, 7, 1)), "o próximo irmão do mesmo nível");
        f.run(FoldingActionKind::GotoPreviousFold, &no_args()).await;
        assert_eq!(f.editor.get_selection(), Some(Selection::new(1, 1, 1, 1)));
        f.finish();
    });
}

#[test]
fn manual_ranges_are_created_from_the_selection_and_removed() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;

        // a seleção termina na coluna 1 da linha 6, então o fim da região é a linha 5
        f.editor.set_selection(SelectionOrRange::Selection(Selection::new(4, 1, 6, 1)), None);
        f.run(FoldingActionKind::CreateFoldingRangeFromSelection, &no_args()).await;
        let regions = folding_model.regions();
        assert_eq!(regions.length(), 4);
        let manual = folding_model.get_region_at_line(4).expect("a região manual existe");
        assert_eq!((manual.start_line_number(), manual.end_line_number(), manual.is_collapsed()), (4, 5, true));
        f.finish();
    });
}

#[test]
fn view_state_round_trips_the_collapsed_regions() {
    run_with_faked_timers(async {
        let f = Fixture::new(None, None);
        let folding_model = f.settle().await;
        f.set_cursor(7);
        f.run(FoldingActionKind::Fold, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, true]);

        let state = f.controller.save_folding_state().expect("o modelo de dobras existe");
        assert_eq!(state.line_count, Some(9));
        assert!(state.collapsed_regions.as_ref().is_some_and(|regions| !regions.is_empty()));
        let restored = FoldingStateMemento::from_dynamic(&state.to_dynamic()).expect("o estado é um objeto");
        assert_eq!(restored, state);

        f.run(FoldingActionKind::UnfoldAll, &no_args()).await;
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, false]);
        f.controller.restore_folding_state(&restored);
        assert_eq!(Fixture::collapsed(&folding_model), vec![false, false, true]);
        f.finish();
    });
}
