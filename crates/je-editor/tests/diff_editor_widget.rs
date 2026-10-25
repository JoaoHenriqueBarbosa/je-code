//! Porte de `src/vs/editor/test/browser/widget/diffEditorWidget.test.ts` (suíte `DiffEditorWidget2`), caso a caso. Os dois casos de
//! abrir e fechar as regiões sem mudanças pelo teclado (`compact disclosures expand and collapse with a persistent keyboard target`,
//! um por valor de `renderSideBySide`), que no original são os primeiros da suíte, ficam no fim do arquivo: dependem do
//! `HideUnchangedRegionsFeature`, que veio na fatia 3a do editor de diff.
//!
//! Os casos `compact disclosures`: não há DOM. `container.querySelector('.editor.modified .disclosure-toggle')` é o primeiro
//! `HiddenLinesOverlay::Collapsed` do editor modificado da feature; `toggle.focus()` é `focus_toggle`; o `keydown` é
//! `on_toggle_key_down(key, false)`; `aria-expanded` é o `aria_expanded` da visão do controle; `document.activeElement === toggle` é
//! `is_toggle_focused`; `toggle.isConnected` é o controle continuar entre os controles atuais da feature (o mesmo `Rc`). O
//! `container` é o nó raiz de um `Ui`. Os serviços são os do `createCodeEditorServices` do porte mais os que o widget pede (chaves de
//! contexto e atalhos de teste, sinais de acessibilidade, progresso, provedor de diff). As features da fatia 3b (régua, zonas de
//! visão, visão acessível, calha, blocos movidos) ainda não existem: o teste as substitui por dublês inertes (`InertFeature`), e as
//! duas da fatia 3a são as reais.
//!
//! Notas de tradução:
//! - O `deepStrictEqual` sobre objetos vira a comparação de tuplas de campos, na ordem do objeto do original.
//! - `new TestAccessibilityService()` é o [`TestAccessibilityService`] daqui: sem leitor de tela, e nenhum evento dispara.
//! - `options.setWidth(width)` e `options.setWidth(width, smoothResizeStartWidth)` são `set_width(width, None)` e
//!   `set_width(width, Some(..))`; `options.resetWidthBasedLayout()` é `reset_width_based_layout(None)`.
//! - `serialize(regions)` usa o `Display` de `LineRange` (`[início,fim)`), que é o `toString()` do original.

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, RefCounted, to_disposable};
use je_base::observable::{IObservable, ITransaction, const_observable};
use je_base::promise::Thenable;
use je_editor::code_action::platform::{
    I_ACCESSIBILITY_SIGNAL_SERVICE, I_EDITOR_PROGRESS_SERVICE, IAccessibilitySignalService, IEditorProgressService, IProgressIndicator,
    IProgressRunner, ProgressTotal,
};
use je_editor::code_editor_widget::CodeEditorWidgetOptions;
use je_editor::config::editor_options::layout_info::EditorLayoutInfo;
use je_editor::core_2d::dimension::Dimension;
use je_editor::diff::range_mapping::DetailedLineRangeMapping;
use je_editor::diff_editor::components::diff_editor_editors::{CreateInnerEditor, DiffEditorSide};
use je_editor::diff_editor::components::diff_editor_sash::SashLayout;
use je_editor::diff_editor::delegating_editor_impl::DelegatingEditor;
use je_editor::diff_editor::diff_editor_widget::{DiffEditorWidget, DiffEditorWidgetArgs};
use je_editor::diff_editor::feature_contracts::{
    DiffEditorFeatureContext, DiffEditorFeatures, IAccessibleDiffViewer, IDiffEditorGutter, IDiffEditorViewZones, IHideUnchangedRegionsFeature,
    IMovedBlocksLinesFeature, IOverviewRulerFeature, IsUpdatingViewZones, ObservableViewZonePair, SetAccessibleDiffViewerVisible,
};
use je_editor::diff_editor::features::hide_unchanged_regions_feature::{HiddenLinesOverlay, HideUnchangedRegionsFeature};
use je_editor::diff_editor::features::revert_buttons_feature::RevertButtonsFeature;
use je_editor::diff_editor::{DiffCodeEditorWidgetOptions, ScrollAnchorRunner};
use je_editor::diff_editor_options::DiffEditorVariant;
use je_editor::diff_editor_utils::ViewZoneIdSet;
use je_editor::diff_editor_view_model::DiffEditorModel;
use je_editor::diff_provider_factory_service::{I_DIFF_PROVIDER_FACTORY_SERVICE, IDiffProviderFactoryService};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::multi_diff_editor::diff_editor_surface::IDiffEditorWidget;
use je_instantiation::ServiceCollection;
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::keybinding::{I_KEYBINDING_SERVICE, IKeybindingService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_ui::Ui;
use je_widgets::sash::BoundarySashes;
use support::inline_completions_test_utils::{MockKeybindingService, SilentAccessibilitySignalService};
use support::test_code_editor::{TestCodeEditor, create_code_editor_services, instantiate_test_code_editor_with_configuration};
use support::test_configuration::{StyledTestEnvironment, TestConfiguration};
use support::test_diff_provider_factory_service::TestDiffProviderFactoryService;
use support::test_text_model::instantiate_text_model;
use je_base::observable::Observable;
use je_base::promise::Promise;
use je_editor::diff::range_mapping::DetailedLineRangeMapping;
use je_editor::diff_editor_options::DiffEditorOptions;
use je_editor::diff_editor_view_model::UnchangedRegion;
use je_editor::line_range::LineRange;
use je_platform::accessibility::{AccessibilitySupport, I_ACCESSIBILITY_SERVICE, IAccessibilityService};

/// `TestAccessibilityService`: sem leitor de tela, e nenhum evento dispara.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// `new DiffEditorOptions({ renderSideBySide: true, renderSideBySideInlineBreakpoint: 900, useInlineViewWhenSpaceIsLimited: true },
/// new TestAccessibilityService())`.
fn width_based_options() -> DiffEditorOptions {
    let options = Dynamic::object([
        ("renderSideBySide", Dynamic::Bool(true)),
        ("renderSideBySideInlineBreakpoint", Dynamic::Number(900.0)),
        ("useInlineViewWhenSpaceIsLimited", Dynamic::Bool(true)),
    ]);
    DiffEditorOptions::new(&options, Rc::new(TestAccessibilityService))
}

// #region width based layout

#[test]
fn commits_temporary_inline_when_smoothly_enlarging_from_automatic_inline() {
    let options = width_based_options();

    options.set_width(1000.0, None);
    let initially_side_by_side = options.render_side_by_side.get();
    options.set_width(800.0, Some(1000.0));
    let inline_during_resize = options.render_side_by_side.get();
    let temporary_inline_after_shrinking = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(1000.0));
    let restored_during_resize = options.render_side_by_side.get();
    options.set_width(800.0, Some(1000.0));
    let temporary_inline_after_ending_narrow = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(800.0));
    let wide_after_inline_was_committed = options.render_side_by_side.get();
    let temporary_inline_mode = options.temporary_inline_mode.get();
    options.set_width(800.0, None);
    let temporary_inline_after_becoming_narrow = options.temporary_inline_mode.get();
    options.set_width(1000.0, Some(800.0));
    options.reset_width_based_layout(None);
    let wide_after_resetting_automatic = options.render_side_by_side.get();
    options.set_width(800.0, None);
    let automatic_inline_result = options.render_side_by_side_in_automatic_mode.get();
    options.set_width(1000.0, None);
    let automatic_side_by_side_result = options.render_side_by_side_in_automatic_mode.get();
    options.update_options(&Dynamic::object([("renderSideBySide", Dynamic::Bool(false))]));
    options.update_options(&Dynamic::object([("renderSideBySide", Dynamic::Bool(true))]));

    assert_eq!(
        (
            initially_side_by_side,
            inline_during_resize,
            temporary_inline_after_shrinking,
            restored_during_resize,
            temporary_inline_after_ending_narrow,
            wide_after_inline_was_committed,
            temporary_inline_mode,
            temporary_inline_after_becoming_narrow,
            wide_after_resetting_automatic,
            automatic_inline_result,
            automatic_side_by_side_result,
            options.render_side_by_side.get(),
        ),
        (true, false, false, true, false, false, true, false, true, false, true, true),
    );
}

#[test]
fn keeps_auto_layout_after_a_non_resize_layout_change() {
    let options = width_based_options();

    options.set_width(800.0, None);
    let narrow = options.render_side_by_side.get();
    options.set_width(1000.0, None);

    assert_eq!((narrow, options.render_side_by_side.get()), (false, true));
}

// #endregion

// #region UnchangedRegion

fn serialize(regions: &[Rc<UnchangedRegion>]) -> Vec<String> {
    regions.iter().map(|r| format!("{} - {}", r.original_unchanged_range(), r.modified_unchanged_range())).collect()
}

/// `new DetailedLineRangeMapping(new LineRange(a, b), new LineRange(c, d), [])`.
fn change(original: (i32, i32), modified: (i32, i32)) -> DetailedLineRangeMapping {
    DetailedLineRangeMapping::new(LineRange::new(original.0, original.1), LineRange::new(modified.0, modified.1), Some(Vec::new()))
}

#[test]
fn everything_changed() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[change((1, 10), (1, 10))], 10, 10, 3, 3)), Vec::<String>::new());
}

#[test]
fn nothing_changed() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[], 10, 10, 3, 3)), vec!["[1,11) - [1,11)"]);
}

#[test]
fn change_in_the_middle() {
    assert_eq!(
        serialize(&UnchangedRegion::from_diffs(&[change((50, 60), (50, 60))], 100, 100, 3, 3)),
        vec!["[1,47) - [1,47)", "[63,101) - [63,101)"],
    );
}

#[test]
fn change_at_the_end() {
    assert_eq!(serialize(&UnchangedRegion::from_diffs(&[change((99, 100), (100, 100))], 100, 100, 3, 3)), vec!["[1,96) - [1,96)"]);
}

// #endregion

// #region compact disclosures

/// O `IContextKeyServiceTarget` do contêiner (o `HTMLElement` do `createScoped`): sem pai nem atributos.
struct ContainerTarget;

impl IContextKeyServiceTarget for ContainerTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        None
    }
    fn set_attribute(&self, _attr: &str, _value: &str) {}
    fn remove_attribute(&self, _attr: &str) {}
    fn has_attribute(&self, _attr: &str) -> bool {
        false
    }
    fn get_attribute(&self, _attr: &str) -> Option<String> {
        None
    }
}

/// `emptyProgressRunner`.
struct EmptyProgressRunner;

impl IProgressRunner for EmptyProgressRunner {
    fn total(&self, _value: f64) {}
    fn worked(&self, _value: f64) {}
    fn done(&self) {}
}

/// O `IEditorProgressService` do teste: `show() { return emptyProgressRunner; }`.
struct TestEditorProgressService;

impl IProgressIndicator for TestEditorProgressService {
    fn show(&self, _total: ProgressTotal, _delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        Rc::new(EmptyProgressRunner)
    }
    fn show_while(&self, _promise: &dyn Thenable, _delay: Option<u64>) -> Promise<()> {
        unreachable!("o teste não chama showWhile")
    }
}

impl IEditorProgressService for TestEditorProgressService {}

/// O dublê inerte das features da fatia 3b (ver o topo do arquivo).
struct InertFeature;

impl IDisposable for InertFeature {
    fn dispose(&self) {}
}

impl IOverviewRulerFeature for InertFeature {
    fn width(&self) -> f64 {
        0.0
    }
}

impl IMovedBlocksLinesFeature for InertFeature {
    fn width(&self) -> IObservable<f64> {
        const_observable(0.0)
    }
}

impl IDiffEditorGutter for InertFeature {
    fn width(&self) -> IObservable<f64> {
        const_observable(0.0)
    }
    fn layout(&self, _left: f64) {}
}

impl IDiffEditorViewZones for InertFeature {
    fn view_zones(&self) -> IObservable<Rc<ObservableViewZonePair>> {
        const_observable(Rc::new(ObservableViewZonePair { orig: Vec::new(), modified: Vec::new() }))
    }
}

impl IAccessibleDiffViewer for InertFeature {
    fn next(&self) {}
    fn prev(&self) {}
    fn close(&self) {}
}

/// As features do widget: as duas da fatia 3a são as reais, e a feature de regiões sem mudanças fica guardada para o teste.
#[derive(Default)]
struct TestFeatures {
    hide_unchanged_regions: RefCell<Option<Rc<HideUnchangedRegionsFeature>>>,
}

impl DiffEditorFeatures for TestFeatures {
    fn create_overview_ruler(
        &self,
        _context: &DiffEditorFeatureContext,
        _root_width: IObservable<f64>,
        _root_height: IObservable<f64>,
        _modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IOverviewRulerFeature> {
        Rc::new(InertFeature)
    }

    fn create_hide_unchanged_regions(
        &self,
        context: &DiffEditorFeatureContext,
        run_with_original_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        run_with_modified_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        variant: DiffEditorVariant,
    ) -> Rc<dyn IHideUnchangedRegionsFeature> {
        let feature = HideUnchangedRegionsFeature::new(context, run_with_original_editor_scroll_anchor, run_with_modified_editor_scroll_anchor, variant);
        *self.hide_unchanged_regions.borrow_mut() = Some(feature.clone());
        feature
    }

    fn create_view_zones(
        &self,
        _context: &DiffEditorFeatureContext,
        _is_updating_view_zones: IsUpdatingViewZones,
        _orig_view_zone_ids_to_ignore: ViewZoneIdSet,
        _mod_view_zone_ids_to_ignore: ViewZoneIdSet,
    ) -> Rc<dyn IDiffEditorViewZones> {
        Rc::new(InertFeature)
    }

    fn create_accessible_diff_viewer(
        &self,
        _context: &DiffEditorFeatureContext,
        _visible: IObservable<bool>,
        _set_visible: SetAccessibleDiffViewerVisible,
        _can_close: IObservable<bool>,
        _width: IObservable<f64>,
        _height: IObservable<f64>,
        _diffs: IObservable<Option<Rc<Vec<DetailedLineRangeMapping>>>>,
    ) -> Rc<dyn IAccessibleDiffViewer> {
        Rc::new(InertFeature)
    }

    fn create_gutter(
        &self,
        _context: &DiffEditorFeatureContext,
        _sash_layout: Rc<SashLayout>,
        _boundary_sashes: IObservable<Option<Rc<BoundarySashes>>>,
    ) -> Rc<dyn IDiffEditorGutter> {
        Rc::new(InertFeature)
    }

    fn create_moved_blocks_lines(
        &self,
        _context: &DiffEditorFeatureContext,
        _original_editor_layout_info: IObservable<EditorLayoutInfo>,
        _modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<dyn IMovedBlocksLinesFeature> {
        Rc::new(InertFeature)
    }

    fn create_revert_buttons(&self, context: &DiffEditorFeatureContext) -> DisposableRef {
        RevertButtonsFeature::new(context)
    }
}

/// O corpo de `compact disclosures expand and collapse with a persistent keyboard target (side by side: ${renderSideBySide})`.
fn compact_disclosures_expand_and_collapse_with_a_persistent_keyboard_target(render_side_by_side: bool) {
    common::run_with_real_timers(async move {
        let disposables = common::ensure_no_disposables_are_leaked_in_test_suite();
        let ui = Ui::new();

        let collection = ServiceCollection::new();
        let signal_service: Rc<dyn IAccessibilitySignalService> = Rc::new(SilentAccessibilitySignalService);
        collection.set(I_ACCESSIBILITY_SIGNAL_SERVICE, signal_service);
        let progress_service: Rc<dyn IEditorProgressService> = Rc::new(TestEditorProgressService);
        collection.set(I_EDITOR_PROGRESS_SERVICE, progress_service);
        let diff_provider_factory_service: Rc<dyn IDiffProviderFactoryService> = Rc::new(TestDiffProviderFactoryService);
        collection.set(I_DIFF_PROVIDER_FACTORY_SERVICE, diff_provider_factory_service);
        let accessibility_service: Rc<dyn IAccessibilityService> = Rc::new(TestAccessibilityService);
        collection.set(I_ACCESSIBILITY_SERVICE, accessibility_service);
        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        collection.set(I_CONTEXT_KEY_SERVICE, context_key_service);
        let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(MockKeybindingService);
        collection.set(I_KEYBINDING_SERVICE, keybinding_service);
        let services = create_code_editor_services(&disposables.store(), Some(collection));

        let mut lines: Vec<String> = (0..40).map(|i| format!("const value{i} = {i};")).collect();
        let original = instantiate_text_model(services.undo_redo_service.clone(), lines.join("\n"), None, RelaxedTextModelCreationOptions::default(), None);
        {
            let original = original.clone();
            disposables.add(to_disposable(move || original.dispose()));
        }
        lines[20] = "const value20 = 100;".to_owned();
        let modified = instantiate_text_model(services.undo_redo_service.clone(), lines.join("\n"), None, RelaxedTextModelCreationOptions::default(), None);
        {
            let modified = modified.clone();
            disposables.add(to_disposable(move || modified.dispose()));
        }

        // Os editores internos são editores de teste; o guarda os descarta depois do widget.
        let created_editors: Rc<RefCell<Vec<Rc<TestCodeEditor>>>> = Rc::default();
        let create_inner_editor: CreateInnerEditor = {
            let (services, created_editors) = (services.clone(), created_editors.clone());
            Rc::new(move |_side: DiffEditorSide, options: &Dynamic, _widget_options: CodeEditorWidgetOptions| {
                let configuration = TestConfiguration::with_environment(options, Rc::new(StyledTestEnvironment));
                let editor = instantiate_test_code_editor_with_configuration(&services, None, options, configuration);
                let widget = editor.widget().clone();
                created_editors.borrow_mut().push(editor);
                widget
            })
        };

        let features = Rc::new(TestFeatures::default());
        let options = Dynamic::object([
            ("renderSideBySide", Dynamic::Bool(render_side_by_side)),
            ("renderGutterMenu", Dynamic::Bool(false)),
            ("useInlineViewWhenSpaceIsLimited", Dynamic::Bool(false)),
            (
                "hideUnchangedRegions",
                Dynamic::object([
                    ("enabled", Dynamic::Bool(true)),
                    ("contextLineCount", Dynamic::Number(2.0)),
                    ("minimumLineCount", Dynamic::Number(4.0)),
                ]),
            ),
        ]);
        let widget = services.create_instance::<DiffEditorWidget>(DiffEditorWidgetArgs {
            ui: ui.clone(),
            dom_node: ui.root(),
            context_key_target: Rc::new(ContainerTarget),
            reference_element: None,
            options,
            code_editor_widget_options: DiffCodeEditorWidgetOptions {
                variant: Some(DiffEditorVariant::Compact),
                original_editor: Some(CodeEditorWidgetOptions { contributions: Some(Vec::new()), telemetry_data: None }),
                modified_editor: Some(CodeEditorWidgetOptions { contributions: Some(Vec::new()), telemetry_data: None }),
                ..DiffCodeEditorWidgetOptions::default()
            },
            create_inner_editor,
            features: features.clone(),
            diff_editor_registry: None,
        });
        disposables.add_rc(widget.clone());
        {
            let created_editors = created_editors.clone();
            disposables.add(to_disposable(move || {
                for editor in created_editors.borrow().iter() {
                    editor.dispose();
                }
            }));
        }

        let model = RefCounted::create(widget.create_view_model(DiffEditorModel { original: original.clone(), modified: modified.clone() }));
        disposables.add_rc(model.clone());
        DelegatingEditor::layout(&*widget, Some(Dimension { width: 800.0, height: 500.0 }));
        IDiffEditorWidget::set_diff_model(&*widget, Some(model.clone()), None);

        widget.wait_for_diff().await.unwrap();
        let region = model.object().unchanged_regions.get()[0].clone();
        let feature = features.hide_unchanged_regions.borrow().clone().expect("a feature de regiões sem mudanças foi criada");
        let HiddenLinesOverlay::Collapsed(toggle) = feature.overlays().modified[0].clone() else {
            panic!("o controle do editor modificado é o de disclosure");
        };
        let initially_hidden = region.get_hidden_modified_range(None).length();
        assert!(toggle.focus_toggle());
        assert!(toggle.on_toggle_key_down("Enter", false));
        let expanded = (
            region.get_hidden_modified_range(None).length(),
            toggle.view().toggle.aria_expanded,
            toggle.is_toggle_focused(),
            feature.overlays().modified.iter().any(|overlay| matches!(overlay, HiddenLinesOverlay::Collapsed(w) if Rc::ptr_eq(w, &toggle))),
        );
        assert!(toggle.on_toggle_key_down(" ", false));
        let collapsed = (region.get_hidden_modified_range(None).length(), toggle.view().toggle.aria_expanded);

        IDiffEditorWidget::set_diff_model(&*widget, None, None);

        assert_eq!(
            (expanded, collapsed),
            ((0, Some(true), true, true), (initially_hidden, Some(false))),
        );
    });
}

#[test]
fn compact_disclosures_expand_and_collapse_with_a_persistent_keyboard_target_side_by_side_true() {
    compact_disclosures_expand_and_collapse_with_a_persistent_keyboard_target(true);
}

#[test]
fn compact_disclosures_expand_and_collapse_with_a_persistent_keyboard_target_side_by_side_false() {
    compact_disclosures_expand_and_collapse_with_a_persistent_keyboard_target(false);
}

// #endregion
