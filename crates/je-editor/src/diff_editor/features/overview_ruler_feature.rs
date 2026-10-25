//! Porte de `src/vs/editor/browser/widget/diffEditor/features/overviewRulerFeature.ts`: a régua de visão geral do editor de diff, com as
//! zonas das diferenças dos dois editores (removidas à esquerda, inseridas à direita) e o `diffViewport`, o retângulo que mostra a
//! parte visível do editor modificado.
//!
//! Divergências:
//! - **DOM vira dado.** A classe `has-diff-overview` da raiz, o `div.diffOverview` (`left`, `height`, a largura fixa) e o
//!   `div.diffViewport` (`top`, `height`, a largura fixa) são o [`OverviewRulerView`] ([`OverviewRulerFeature::view`]). Os
//!   `pointerdown` e `wheel` do `diffOverview` vão direto ao editor modificado (`delegateVerticalScrollbarPointerDown` e
//!   `delegateScrollFromMouseWheelEvent`), que o `CodeEditorWidget` do porte não tem (não há `View`); quem tem a `View` do editor
//!   modificado faz o repasse quando o evento chega no `diffOverview`.
//! - **`createOverviewRuler`.** O `IOverviewRuler` do editor (que desenha num canvas) não existe no `CodeEditorWidget`: cada régua é o
//!   [`OverviewRulerSideView`] (as zonas `setZones`, a posição `setLayout` e a classe CSS dada), que o pintor lê. Como no original, só
//!   há régua com modelo (`createOverviewRuler` devolve `null` sem `View` real), e sem as duas o autorun sai ("probably no model").
//! - **Cores.** `defaultInsertColor` e `defaultRemoveColor` (de `colorRegistry`) são [`default_insert_color`] e
//!   [`default_remove_color`]; as cores do tema são pedidas pelo identificador. `IThemeService` vem do serviço de instanciação do
//!   contexto.
//! - **Layout sem `null`.** `IObservable<EditorLayoutInfo | null>` é `IObservable<EditorLayoutInfo>` (o contrato do widget): o ramo
//!   `else` de `layoutInfo` ausente não existe.
//! - **`editor._getViewModel()`** é `get_view_model()` do editor, e o `coordinatesConverter` é o `ICoordinatesConverter` dele.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::{DebugOwner, IObservable, IReader, Observable, autorun, autorun_with_store, derived, observable_signal_from_event};
use je_theme::color::{Color, Srgba};
use je_widgets::scrollbar::scrollbar_state::ScrollbarState;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::layout_info::{EditorLayoutInfo, OverviewRulerPosition};
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::feature_contracts::{DiffEditorFeatureContext, IOverviewRulerFeature};
pub use crate::diff_editor::{ENTIRE_DIFF_OVERVIEW_WIDTH, ONE_OVERVIEW_WIDTH};
use crate::line_range::LineRange;
use crate::overview_zone_manager::OverviewRulerZone;
use crate::position::Position;
use crate::services::theme_service::{I_THEME_SERVICE, IColorTheme};
use crate::view_model_types::IViewModel;

/// `diffOverviewRulerInserted`.
const DIFF_OVERVIEW_RULER_INSERTED: &str = "diffEditorOverview.insertedForeground";
/// `diffOverviewRulerRemoved`.
const DIFF_OVERVIEW_RULER_REMOVED: &str = "diffEditorOverview.removedForeground";
/// `diffInserted`.
const DIFF_INSERTED: &str = "diffEditor.insertedTextBackground";
/// `diffRemoved`.
const DIFF_REMOVED: &str = "diffEditor.removedTextBackground";

/// `defaultInsertColor`.
pub fn default_insert_color() -> Color {
    Color::from(Srgba::new(155, 185, 85, 0.2))
}

/// `defaultRemoveColor`.
pub fn default_remove_color() -> Color {
    Color::from(Srgba::new(255, 0, 0, 0.2))
}

/// O `div.diffOverview`: a posição absoluta (`top: 0`) e o que o layout grava.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiffOverviewView {
    /// `style.left`.
    pub left: f64,
    /// `style.height`.
    pub height: f64,
    /// `style.width` (`ENTIRE_DIFF_OVERVIEW_WIDTH`).
    pub width: f64,
}

/// O `div.diffViewport`: a posição absoluta e o que o layout grava.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiffViewportView {
    pub top: f64,
    pub height: f64,
    /// `setWidth(ENTIRE_DIFF_OVERVIEW_WIDTH)`.
    pub width: f64,
}

/// O que o `IOverviewRuler` de um dos editores guarda: a classe CSS, as zonas (`setZones`) e a posição (`setLayout`).
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewRulerSideView {
    pub css_class_name: &'static str,
    pub zones: Vec<OverviewRulerZone>,
    pub layout: OverviewRulerPosition,
}

/// O que o pintor lê da feature.
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewRulerView {
    /// A classe `has-diff-overview` da raiz.
    pub has_diff_overview: bool,
    pub diff_overview: DiffOverviewView,
    pub viewport: DiffViewportView,
    pub original: Option<OverviewRulerSideView>,
    pub modified: Option<OverviewRulerSideView>,
}

/// `{ insertColor, removeColor }`.
struct OverviewColors {
    insert_color: Color,
    remove_color: Color,
}

/// `theme.getColor(overview) || (theme.getColor(base) || default).transparent(2)`.
fn overview_color(theme: &dyn IColorTheme, overview: &str, base: &str, default: Color) -> Color {
    theme.get_color(overview, None).unwrap_or_else(|| theme.get_color(base, None).unwrap_or(default).transparent(2.0))
}

/// `createZones(ranges, color, editor)`.
fn create_zones(ranges: Vec<LineRange>, color: &Color, editor: &Rc<CodeEditorWidget>) -> Vec<OverviewRulerZone> {
    let Some(vm) = editor.get_view_model() else { return Vec::new() };
    let coordinates_converter = vm.coordinates_converter();
    ranges
        .into_iter()
        .filter(|d| d.length() > 0)
        .map(|r| {
            let start = coordinates_converter.convert_model_position_to_view_position(Position::new(r.start_line_number, 1), None, None, None);
            let end = coordinates_converter.convert_model_position_to_view_position(Position::new(r.end_line_number_exclusive, 1), None, None, None);
            // By computing the lineCount, we won't ask the view model later for the bottom vertical position.
            // (The view model will take into account the alignment viewzones, which will give
            // modifications and deletetions always the same height.)
            let line_count = end.line_number - start.line_number;
            OverviewRulerZone::new(start.line_number as u32, end.line_number as u32, f64::from(line_count), &color.to_string())
        })
        .collect()
}

/// `OverviewRulerFeature`.
pub struct OverviewRulerFeature {
    base: Rc<Disposable>,
    view: Rc<RefCell<OverviewRulerView>>,
}

impl OverviewRulerFeature {
    /// `new OverviewRulerFeature(editors, rootElement, diffModel, rootWidth, rootHeight, modifiedEditorLayoutInfo, themeService)`, com
    /// `editors`, `rootElement`, `diffModel` e o `IThemeService` (do serviço de instanciação) do contexto.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &DiffEditorFeatureContext,
        root_width: IObservable<f64>,
        root_height: IObservable<f64>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("OverviewRulerFeature");
        let base = Disposable::new();
        let editors: Rc<DiffEditorEditors> = context.editors.clone();
        let diff_model = context.diff_model.clone();
        let theme_service = context.instantiation_service.invoke_function(|accessor| accessor.get(I_THEME_SERVICE));

        let view = Rc::new(RefCell::new(OverviewRulerView {
            has_diff_overview: true,
            diff_overview: DiffOverviewView { left: 0.0, height: 0.0, width: ENTIRE_DIFF_OVERVIEW_WIDTH },
            viewport: DiffViewportView::default(),
            original: None,
            modified: None,
        }));
        {
            let view = view.clone();
            base.register(to_disposable(move || view.borrow_mut().has_diff_overview = false));
        }

        let current_color_theme = {
            let theme_service = theme_service.clone();
            observable_from_event_owned(owner.clone(), theme_service.on_did_color_theme_change(), move |_| theme_service.get_color_theme())
        };

        let current_colors = derived(move |reader| {
            // colors
            let theme = current_color_theme.read(reader);
            let insert_color = overview_color(&*theme, DIFF_OVERVIEW_RULER_INSERTED, DIFF_INSERTED, default_insert_color());
            let remove_color = overview_color(&*theme, DIFF_OVERVIEW_RULER_REMOVED, DIFF_REMOVED, default_remove_color());
            Rc::new(OverviewColors { insert_color, remove_color })
        });

        {
            // recreate overview rules when model changes
            let (view, editors) = (view.clone(), editors.clone());
            base.register(autorun_with_store(move |reader, store| {
                let m = diff_model.read(reader);

                let original_overview_ruler = editors.original.has_model();
                let modified_overview_ruler = editors.modified.has_model();

                {
                    let view = view.clone();
                    store.add(to_disposable(move || {
                        let mut view = view.borrow_mut();
                        view.original = None;
                        view.modified = None;
                    }));
                }

                if !original_overview_ruler || !modified_overview_ruler {
                    // probably no model
                    return;
                }

                let empty_layout = OverviewRulerPosition { width: 0.0, height: 0.0, top: 0.0, right: 0.0 };
                {
                    let mut view = view.borrow_mut();
                    view.original = Some(OverviewRulerSideView { css_class_name: "original diffOverviewRuler", zones: Vec::new(), layout: empty_layout });
                    view.modified = Some(OverviewRulerSideView { css_class_name: "modified diffOverviewRuler", zones: Vec::new(), layout: empty_layout });
                }

                let orig_view_zones_changed = observable_signal_from_event("viewZoneChanged", editors.original.on_did_change_view_zones());
                let mod_view_zones_changed = observable_signal_from_event("viewZoneChanged", editors.modified.on_did_change_view_zones());
                let orig_hidden_ranges_changed = observable_signal_from_event("hiddenRangesChanged", editors.original.on_did_change_hidden_areas());
                let mod_hidden_ranges_changed = observable_signal_from_event("hiddenRangesChanged", editors.modified.on_did_change_hidden_areas());

                {
                    let (view, editors, current_colors) = (view.clone(), editors.clone(), current_colors.clone());
                    store.add(autorun(move |reader| {
                        // set overview ruler zones
                        let reader: &dyn IReader = reader;
                        orig_view_zones_changed.read(reader);
                        mod_view_zones_changed.read(reader);
                        orig_hidden_ranges_changed.read(reader);
                        mod_hidden_ranges_changed.read(reader);

                        let colors = current_colors.read(reader);
                        let diff = m.as_ref().and_then(|m| m.diff.read(reader));
                        let mappings = diff.as_ref().map(|diff| diff.mappings.as_slice()).unwrap_or_default();

                        let original_zones = create_zones(mappings.iter().map(|d| d.line_range_mapping.original).collect(), &colors.remove_color, &editors.original);
                        let modified_zones = create_zones(mappings.iter().map(|d| d.line_range_mapping.modified).collect(), &colors.insert_color, &editors.modified);
                        let mut view = view.borrow_mut();
                        if let Some(side) = view.original.as_mut() {
                            side.zones = original_zones;
                        }
                        if let Some(side) = view.modified.as_mut() {
                            side.zones = modified_zones;
                        }
                    }));
                }

                {
                    let (view, editors) = (view.clone(), editors.clone());
                    let (root_height, root_width, modified_editor_layout_info) =
                        (root_height.clone(), root_width.clone(), modified_editor_layout_info.clone());
                    store.add(autorun(move |reader| {
                        // layout overview ruler
                        let reader: &dyn IReader = reader;
                        let height = root_height.read(reader);
                        let width = root_width.read(reader);
                        let layout_info = modified_editor_layout_info.read(reader);

                        let free_space = ENTIRE_DIFF_OVERVIEW_WIDTH - 2.0 * ONE_OVERVIEW_WIDTH;
                        let scroll_top = editors.modified_scroll_top.read(reader);
                        let scroll_height = editors.modified_scroll_height.read(reader);

                        let scroll_bar_options = editors.modified.get_option(SCROLLBAR);
                        let state = ScrollbarState::new(
                            if scroll_bar_options.vertical_has_arrows { scroll_bar_options.arrow_size } else { 0.0 },
                            scroll_bar_options.vertical_scrollbar_size,
                            0.0,
                            layout_info.height,
                            scroll_height,
                            scroll_top,
                        );

                        let mut view = view.borrow_mut();
                        if let Some(side) = view.original.as_mut() {
                            side.layout = OverviewRulerPosition { top: 0.0, height, right: free_space + ONE_OVERVIEW_WIDTH, width: ONE_OVERVIEW_WIDTH };
                        }
                        if let Some(side) = view.modified.as_mut() {
                            side.layout = OverviewRulerPosition { top: 0.0, height, right: 0.0, width: ONE_OVERVIEW_WIDTH };
                        }
                        view.viewport.top = state.get_slider_position();
                        view.viewport.height = state.get_slider_size();

                        view.diff_overview.height = height;
                        view.diff_overview.left = width - ENTIRE_DIFF_OVERVIEW_WIDTH;
                        view.viewport.width = ENTIRE_DIFF_OVERVIEW_WIDTH;
                    }));
                }
            }));
        }

        Rc::new(Self { base, view })
    }

    /// O que o pintor lê da régua.
    pub fn view(&self) -> OverviewRulerView {
        self.view.borrow().clone()
    }
}

impl IOverviewRulerFeature for OverviewRulerFeature {
    fn width(&self) -> f64 {
        ENTIRE_DIFF_OVERVIEW_WIDTH
    }
}

impl IDisposable for OverviewRulerFeature {
    fn dispose(&self) {
        self.base.dispose();
    }
}
