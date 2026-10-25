//! Porte de `src/vs/editor/browser/widget/diffEditor/features/hideUnchangedRegionsFeature.ts`: esconde as regiões sem mudanças dos dois
//! editores do diff, põe no lugar de cada uma o controle que a mostra de novo (`CollapsedCodeOverlayWidget`, ou o
//! `CompactCollapsedCodeOverlayWidget` do modo compacto) e as decorações e a calha de dobrar que acompanham.
//!
//! Divergências:
//! - **DOM vira dado.** Os nós de cada controle (`div.diff-hidden-lines`, o `disclosure-toggle`, os botões do topo e de baixo, o
//!   texto e a trilha de símbolos) são o [`CollapsedCodeOverlayView`] ([`CollapsedCodeOverlayWidget::view`]), que o pintor lê, e o
//!   [`CompactCollapsedCodeOverlayWidget::text`]. O `applyStyle` (largura do `first`, posição do `lineNumberControl`) grava nesse
//!   dado. As classes que o original põe no `div` do próprio editor (`draggingUnchangedRegion`, `canMoveTop`, `canMoveBottom`) são
//!   o [`EditorDomClasses`] de cada lado ([`HideUnchangedRegionsFeature::editor_dom_classes`]).
//! - **Ouvintes de DOM viram métodos.** `click`, `keydown`, `dblclick` e `mousedown` dos nós, e o `mousemove`/`mouseup` que o
//!   `mousedown` registra na janela, são `on_toggle_click`, `on_toggle_key_down`, `on_default_control_click`,
//!   `on_label_double_click`, `on_breadcrumb_click`, `on_breadcrumb_key_down`, `on_mouse_down`, `on_drag_move` e `on_drag_up`, que o
//!   dono da janela chama quando o evento nativo chega (o `move` e o `up` só valem enquanto há arrasto). Os que o original termina
//!   com `preventDefault()` e `stopPropagation()` devolvem se consumiram o evento. O foco do `disclosure-toggle` é
//!   [`CollapsedCodeOverlayWidget::focus_toggle`] e `is_toggle_focused`.
//! - **Hover.** O `setupDelayedHover` não vai para o serviço de hover (atrelado à árvore de widgets): o conteúdo que ele mostraria é
//!   dado da visão (`aria_label` do `toggle` e de cada item da trilha, `hover` do conteúdo do modo compacto).
//! - **Serviços.** O `createInstance(CollapsedCodeOverlayWidget, ...)` injetava só o `IHoverService`, que não é mais usado; os
//!   widgets nascem direto por [`CollapsedCodeOverlayWidget::new`].
//! - **`onMouseUp` com `fold-unchanged`.** `event.target.element?.className.includes('fold-unchanged')` pergunta se o alvo é o glifo
//!   de dobrar. O elemento do alvo é opaco; o glifo é a decoração da margem de glifos com essa classe, então o alvo vale quando é a
//!   margem de glifos de uma linha que tem a decoração. `stopPropagation()` e `preventDefault()` não têm o que agir: o evento chega
//!   por referência, e quem o disparou decide.
//! - **`_breadcrumbsSourceFactory`.** O `observableValue` estático é um `thread_local` com a fábrica padrão (uma fonte sem itens);
//!   [`HideUnchangedRegionsFeature::set_breadcrumbs_source_factory`] a troca. A [`IDiffEditorBreadcrumbsSource`] completa mora aqui, e
//!   o `SymbolKinds.toIcon(item.kind)` é [`SymbolKind::to_icon`].
//! - **`derived(...)` com `reader.store`.** O derivado das zonas de visão é um `derivedDisposable` ([`derived_disposable_with`]): os
//!   controles criados no recálculo vão para um `DisposableStore` descartado no recálculo seguinte, como o `reader.store`.
//! - **`IMarkdownString`.** A dica da calha de dobrar é o [`MarkdownString`] do porte, que só carrega o texto (sem `isTrusted` nem
//!   `supportThemeIcons`).
//! - `Math.round` do arrasto é o arredondamento do JavaScript (metade para cima).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::deps::try_finally;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{DebugOwner, IObservable, IReader, ITransaction, Observable, ObservableExt, SettableExt, autorun, derived, transaction};
use je_base::themables::ThemeIcon;
use je_instantiation::InstantiationService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_events::CursorChangeReason;
use crate::diff_editor::ScrollAnchorRunner;
use crate::diff_editor::components::diff_editor_editors::{DiffEditorEditors, DiffEditorSide};
use crate::diff_editor::feature_contracts::{DiffEditorFeatureContext, IHideUnchangedRegionsFeature, ObservableViewZonePair};
use crate::diff_editor_options::{DiffEditorOptions, DiffEditorVariant};
use crate::diff_editor_utils::{
    IObservableViewZone, PlaceholderViewZone, ViewZoneOverlayLayout, ViewZoneOverlayWidget, apply_observable_decorations, derived_disposable_with,
    derived_optional_disposable_of,
};
use crate::diff_editor_view_model::{DiffEditorViewModel, DraggedSide, RevealPreference, UnchangedRegion};
use crate::editor_browser::{EditorMouseEvent, MouseTarget, MouseTargetType};
use crate::language_providers::SymbolKind;
use crate::line_range::LineRange;
use crate::model::{MarkdownString, ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};

// #region IDiffEditorBreadcrumbsSource

/// `{ name: string; kind: SymbolKind; startLineNumber: number }`.
#[derive(Clone, Debug, PartialEq)]
pub struct BreadcrumbItem {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line_number: i32,
}

/// `IDiffEditorBreadcrumbsSource`.
pub trait IDiffEditorBreadcrumbsSource: IDisposable {
    /// `getBreadcrumbItems(startRange, reader)`.
    fn get_breadcrumb_items(&self, start_range: LineRange, reader: &dyn IReader) -> Vec<BreadcrumbItem>;

    /// `getAt(lineNumber, reader)`.
    fn get_at(&self, line_number: i32, reader: &dyn IReader) -> Vec<BreadcrumbItem>;
}

/// `(textModel: ITextModel, instantiationService: IInstantiationService) => IDiffEditorBreadcrumbsSource`.
pub type BreadcrumbsSourceFactory = Rc<dyn Fn(&Rc<TextModel>, &Rc<InstantiationService>) -> Rc<dyn IDiffEditorBreadcrumbsSource>>;

/// A fonte padrão da fábrica estática: `dispose() {}`, `getBreadcrumbItems: () => []` e `getAt: () => []`.
struct NoBreadcrumbsSource;

impl IDisposable for NoBreadcrumbsSource {
    fn dispose(&self) {}
}

impl IDiffEditorBreadcrumbsSource for NoBreadcrumbsSource {
    fn get_breadcrumb_items(&self, _start_range: LineRange, _reader: &dyn IReader) -> Vec<BreadcrumbItem> {
        Vec::new()
    }

    fn get_at(&self, _line_number: i32, _reader: &dyn IReader) -> Vec<BreadcrumbItem> {
        Vec::new()
    }
}

fn default_breadcrumbs_source_factory() -> BreadcrumbsSourceFactory {
    Rc::new(|_text_model: &Rc<TextModel>, _instantiation_service: &Rc<InstantiationService>| -> Rc<dyn IDiffEditorBreadcrumbsSource> {
        Rc::new(NoBreadcrumbsSource)
    })
}

thread_local! {
    /// `HideUnchangedRegionsFeature._breadcrumbsSourceFactory`.
    static BREADCRUMBS_SOURCE_FACTORY: Rc<ObservableValue<BreadcrumbsSourceFactory, ()>> =
        observable_value_owned(DebugOwner::new("HideUnchangedRegionsFeature"), default_breadcrumbs_source_factory());
}

// #endregion

// #region Estado de visão (o DOM como dado)

/// As classes que o original põe no `div` do próprio editor enquanto uma região é arrastada.
#[derive(Default)]
pub struct EditorDomClasses {
    dragging_unchanged_region: Cell<bool>,
    can_move_top: Cell<bool>,
    can_move_bottom: Cell<bool>,
}

impl EditorDomClasses {
    /// A classe `draggingUnchangedRegion`.
    pub fn dragging_unchanged_region(&self) -> bool {
        self.dragging_unchanged_region.get()
    }

    /// A classe `canMoveTop`.
    pub fn can_move_top(&self) -> bool {
        self.can_move_top.get()
    }

    /// A classe `canMoveBottom`.
    pub fn can_move_bottom(&self) -> bool {
        self.can_move_bottom.get()
    }
}

/// O `div.top` ou o `div.bottom` do controle: o título, o papel e as classes `canMoveTop`, `canMoveBottom` e `dragging`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiddenLinesHandleView {
    pub title: String,
    /// `role="button"`.
    pub role_button: bool,
    pub can_move_top: bool,
    pub can_move_bottom: bool,
    pub dragging: bool,
}

/// O `div.disclosure-toggle`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisclosureToggleView {
    /// `tabIndex`, só no modo compacto.
    pub tab_index: Option<i32>,
    /// `role="button"`.
    pub role_button: bool,
    /// `aria-expanded`.
    pub aria_expanded: Option<bool>,
    /// `aria-label`, que é também o conteúdo do hover.
    pub aria_label: Option<String>,
}

/// O `span.line-number-control`: a posição (`left` e `width`, no modo compacto) e o ícone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineNumberControlView {
    pub left: Option<f64>,
    pub width: Option<f64>,
    pub icon: Option<ThemeIcon>,
}

/// O `a.default-control`: o título e o ícone do botão que mostra a região de novo.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultControlView {
    pub title: String,
    pub icon: ThemeIcon,
}

/// O `div.first`.
#[derive(Clone, Debug, PartialEq)]
pub struct HiddenLinesFirstView {
    /// `style.width`, que acompanha `layoutInfo.contentLeft`.
    pub width: f64,
    pub line_number_control: LineNumberControlView,
    pub default_control: DefaultControlView,
}

/// Um item da trilha de símbolos (`div.breadcrumb-item`).
#[derive(Clone, Debug, PartialEq)]
pub struct BreadcrumbItemView {
    pub icon: ThemeIcon,
    pub name: String,
    /// O ícone `chevronRight` depois do nome (todos menos o último).
    pub chevron: bool,
    /// `tabIndex = 0` e `role="button"` (modo compacto).
    pub focusable: bool,
    /// `aria-label`, que é também o conteúdo do hover (modo compacto).
    pub aria_label: Option<String>,
}

/// Um filho do `div.content-row` (`others`).
#[derive(Clone, Debug, PartialEq)]
pub enum HiddenLinesItem {
    /// O `span` com o texto das linhas escondidas, com o `title` do duplo clique (fora do modo compacto).
    Label { text: String, title: Option<String> },
    /// O `span` do separador (` | ` ou ` · `).
    Separator(String),
    Breadcrumb(BreadcrumbItemView),
}

/// O conteúdo do `div.content-row`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiddenLinesOthers {
    /// No modo compacto com filhos, os filhos ficam dentro de `div.line-left`, `div.disclosure-content` e `div.line-right`.
    pub disclosure_content: bool,
    /// O conteúdo do hover do `div.disclosure-content` (o `textContent`).
    pub hover: Option<String>,
    pub items: Vec<HiddenLinesItem>,
}

/// O que o pintor lê do `CollapsedCodeOverlayWidget`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollapsedCodeOverlayView {
    /// A classe `diff-hidden-lines-disclosure` do `div.diff-hidden-lines`.
    pub disclosure: bool,
    /// A classe `dragging` do `div.diff-hidden-lines`.
    pub dragging: bool,
    pub top: HiddenLinesHandleView,
    pub bottom: HiddenLinesHandleView,
    pub toggle: DisclosureToggleView,
    /// `None` quando o controle está escondido (`reset(this._nodes.first)`).
    pub first: Option<HiddenLinesFirstView>,
    pub others: HiddenLinesOthers,
}

// #endregion

/// `Math.round`: a metade vai para cima.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `localize('hiddenLines', '{0} hidden lines', lineCount)`.
fn hidden_lines_text(line_count: i32) -> String {
    localize("hiddenLines", "{0} hidden lines", &[line_count.to_string().as_str()])
}

/// `visibleLineCountTop + visibleLineCountBottom === lineCount`.
fn is_fully_revealed(region: &UnchangedRegion, reader: Option<&dyn IReader>) -> bool {
    region.visible_line_count_top.read_opt(reader) + region.visible_line_count_bottom.read_opt(reader) == region.line_count
}

/// O `textContent` dos filhos do `div.disclosure-content`.
fn text_content(items: &[HiddenLinesItem]) -> String {
    items
        .iter()
        .map(|item| match item {
            HiddenLinesItem::Label { text, .. } => text.clone(),
            HiddenLinesItem::Separator(text) => text.clone(),
            HiddenLinesItem::Breadcrumb(item) => format!("\u{a0}{}", item.name),
        })
        .collect()
}

/// `CompactCollapsedCodeOverlayWidget`.
pub struct CompactCollapsedCodeOverlayWidget {
    base: ViewZoneOverlayWidget,
    hide: bool,
    text: Rc<RefCell<String>>,
}

impl CompactCollapsedCodeOverlayWidget {
    pub fn new(editor: &Rc<CodeEditorWidget>, view_zone: &PlaceholderViewZone, unchanged_region: Rc<UnchangedRegion>, hide: bool) -> Rc<Self> {
        let base = ViewZoneOverlayWidget::new(editor, view_zone);

        let text = Rc::new(RefCell::new(String::new()));
        {
            let text = text.clone();
            base.register(autorun(move |reader| {
                // update labels
                let reader: &dyn IReader = reader;
                if !hide {
                    let line_count = unchanged_region.get_hidden_modified_range(Some(reader)).length();
                    *text.borrow_mut() = hidden_lines_text(line_count);
                }
            }));
        }
        Rc::new(Self { base, hide, text })
    }

    /// O `div.text` (`innerText`); `None` quando o controle está escondido (`this._nodes.root.replaceChildren()`).
    pub fn text(&self) -> Option<String> {
        (!self.hide).then(|| self.text.borrow().clone())
    }

    /// O topo e a altura do overlay, como a zona de visão os deu.
    pub fn layout(&self) -> ViewZoneOverlayLayout {
        self.base.layout()
    }

    /// O id do overlay no editor.
    pub fn overlay_widget_id(&self) -> &str {
        self.base.overlay_widget_id()
    }
}

impl IDisposable for CompactCollapsedCodeOverlayWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Os argumentos do construtor de `CollapsedCodeOverlayWidget` (menos a zona de visão e o `IHoverService`).
pub struct CollapsedCodeOverlayArgs {
    pub editor: Rc<CodeEditorWidget>,
    pub unchanged_region: Rc<UnchangedRegion>,
    pub unchanged_region_range: LineRange,
    pub hide: bool,
    pub modified_outline_source: Rc<dyn IDiffEditorBreadcrumbsSource>,
    pub reveal_modified_hidden_line: Rc<dyn Fn(i32)>,
    pub options: Rc<DiffEditorOptions>,
    pub run_with_scroll_anchor: Option<ScrollAnchorRunner>,
    pub compact_control: bool,
    /// As classes do `div` do editor, compartilhadas pelos controles do mesmo editor.
    pub editor_classes: Rc<EditorDomClasses>,
}

/// O arrasto em andamento (as variáveis `startTop`, `didMove` e `cur` do `mousedown`).
#[derive(Clone, Copy)]
struct DragState {
    edge: DraggedSide,
    start_client_y: f64,
    did_move: bool,
    initial: i32,
}

/// `CollapsedCodeOverlayWidget`.
pub struct CollapsedCodeOverlayWidget {
    base: ViewZoneOverlayWidget,
    editor: Rc<CodeEditorWidget>,
    unchanged_region: Rc<UnchangedRegion>,
    unchanged_region_range: LineRange,
    hide: bool,
    reveal_modified_hidden_line: Rc<dyn Fn(i32)>,
    options: Rc<DiffEditorOptions>,
    run_with_scroll_anchor: Option<ScrollAnchorRunner>,
    compact_control: bool,
    view: Rc<RefCell<CollapsedCodeOverlayView>>,
    /// Os itens da trilha de símbolos que o último `update labels` mostrou, na ordem dos `HiddenLinesItem::Breadcrumb`.
    breadcrumb_items: Rc<RefCell<Vec<BreadcrumbItem>>>,
    drag: RefCell<Option<DragState>>,
    toggle_focused: Cell<bool>,
}

impl CollapsedCodeOverlayWidget {
    #[allow(clippy::too_many_lines)]
    pub fn new(view_zone: &PlaceholderViewZone, args: CollapsedCodeOverlayArgs) -> Rc<Self> {
        let CollapsedCodeOverlayArgs {
            editor,
            unchanged_region,
            unchanged_region_range,
            hide,
            modified_outline_source,
            reveal_modified_hidden_line,
            options,
            run_with_scroll_anchor,
            compact_control,
            editor_classes,
        } = args;
        let base = ViewZoneOverlayWidget::new(&editor, view_zone);

        let toggle_enabled = compact_control && !hide;
        let view = Rc::new(RefCell::new(CollapsedCodeOverlayView {
            disclosure: compact_control,
            dragging: false,
            top: HiddenLinesHandleView {
                title: localize("diff.hiddenLines.top", "Click or drag to show more above", &[]),
                ..HiddenLinesHandleView::default()
            },
            bottom: HiddenLinesHandleView {
                title: localize("diff.bottom", "Click or drag to show more below", &[]),
                role_button: true,
                ..HiddenLinesHandleView::default()
            },
            toggle: DisclosureToggleView {
                tab_index: toggle_enabled.then_some(0),
                role_button: toggle_enabled,
                aria_expanded: None,
                aria_label: None,
            },
            first: (!hide).then(|| HiddenLinesFirstView {
                width: 0.0,
                line_number_control: LineNumberControlView::default(),
                default_control: DefaultControlView {
                    title: localize("showUnchangedRegion", "Show Unchanged Region", &[]),
                    icon: ThemeIcon::from_id("unfold"),
                },
            }),
            others: HiddenLinesOthers::default(),
        }));

        if !hide {
            let editor_layout = crate::observable_code_editor::observable_code_editor(&editor);
            {
                // applyStyle(this._nodes.first, { width: editorLayout.layoutInfoContentLeft })
                let (view, content_left) = (view.clone(), editor_layout.layout_info_content_left.clone());
                base.register(autorun(move |reader| {
                    let reader: &dyn IReader = reader;
                    let width = content_left.read(reader);
                    if let Some(first) = view.borrow_mut().first.as_mut() {
                        first.width = width;
                    }
                }));
            }
            if compact_control {
                let (view, layout_info) = (view.clone(), editor_layout.layout_info.clone());
                base.register(autorun(move |reader| {
                    let reader: &dyn IReader = reader;
                    let layout_info = layout_info.read(reader);
                    if let Some(first) = view.borrow_mut().first.as_mut() {
                        first.line_number_control.left = Some(layout_info.line_numbers_left);
                        first.line_number_control.width = Some(layout_info.line_numbers_width);
                    }
                }));
            }
        }

        {
            // Update CollapsedCodeOverlayWidget canMove* css classes
            let (view, unchanged_region, editor_classes) = (view.clone(), unchanged_region.clone(), editor_classes.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let fully_revealed = is_fully_revealed(&unchanged_region, Some(reader));
                let visible_top = unchanged_region.visible_line_count_top.read(reader);
                let visible_bottom = unchanged_region.visible_line_count_bottom.read(reader);

                {
                    let mut view = view.borrow_mut();
                    if compact_control && !hide {
                        let action_label = if fully_revealed {
                            localize("diff.hiddenLines.collapse", "Collapse unchanged lines", &[])
                        } else {
                            let hidden_lines = unchanged_region.get_hidden_modified_range(Some(reader)).length();
                            localize("diff.hiddenLines.expand", "Show {0} hidden lines", &[hidden_lines.to_string().as_str()])
                        };
                        view.toggle.aria_expanded = Some(fully_revealed);
                        view.toggle.aria_label = Some(action_label);
                        if let Some(first) = view.first.as_mut() {
                            first.line_number_control.icon = Some(ThemeIcon::from_id(if fully_revealed { "fold" } else { "unfold" }));
                        }
                    }
                    view.bottom.can_move_top = !fully_revealed;
                    view.bottom.can_move_bottom = visible_bottom > 0;
                    view.top.can_move_top = visible_top > 0;
                    view.top.can_move_bottom = !fully_revealed;
                }

                let is_dragged = unchanged_region.is_dragged.read(reader);
                editor_classes.dragging_unchanged_region.set(is_dragged.is_some());
                match is_dragged {
                    Some(DraggedSide::Top) => {
                        editor_classes.can_move_top.set(visible_top > 0);
                        editor_classes.can_move_bottom.set(!fully_revealed);
                    }
                    Some(DraggedSide::Bottom) => {
                        editor_classes.can_move_top.set(!fully_revealed);
                        editor_classes.can_move_bottom.set(visible_bottom > 0);
                    }
                    None => {
                        editor_classes.can_move_top.set(false);
                        editor_classes.can_move_bottom.set(false);
                    }
                }
            }));
        }

        let breadcrumb_items: Rc<RefCell<Vec<BreadcrumbItem>>> = Rc::default();
        {
            // update labels
            let (view, unchanged_region, breadcrumb_items) = (view.clone(), unchanged_region.clone(), breadcrumb_items.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let mut children: Vec<HiddenLinesItem> = Vec::new();
                let mut breadcrumbs: Vec<BreadcrumbItem> = Vec::new();
                if !hide {
                    let fully_revealed = is_fully_revealed(&unchanged_region, Some(reader));
                    let line_count = unchanged_region.get_hidden_modified_range(Some(reader)).length();
                    let lines_hidden_text = hidden_lines_text(line_count);
                    children.push(HiddenLinesItem::Label {
                        text: if compact_control && fully_revealed {
                            localize("diff.hiddenLines.collapse", "Collapse unchanged lines", &[])
                        } else {
                            lines_hidden_text
                        },
                        title: (!compact_control).then(|| localize("diff.hiddenLines.expandAll", "Double click to unfold", &[])),
                    });

                    let range = unchanged_region.get_hidden_modified_range(Some(reader));
                    let items = if fully_revealed { Vec::new() } else { modified_outline_source.get_breadcrumb_items(range, reader) };

                    if !items.is_empty() {
                        let separator = if compact_control { "\u{a0}\u{b7}\u{a0}" } else { "\u{a0}\u{a0}|\u{a0}\u{a0}" };
                        children.push(HiddenLinesItem::Separator(separator.to_owned()));

                        for (i, item) in items.iter().enumerate() {
                            children.push(HiddenLinesItem::Breadcrumb(BreadcrumbItemView {
                                icon: item.kind.to_icon(),
                                name: item.name.clone(),
                                chevron: i != items.len() - 1,
                                focusable: compact_control,
                                aria_label: compact_control.then(|| localize("diff.hiddenLines.breadcrumb", "Go to {0}", &[item.name.as_str()])),
                            }));
                        }
                    }
                    breadcrumbs = items;
                }
                *breadcrumb_items.borrow_mut() = breadcrumbs;

                view.borrow_mut().others = if compact_control && !children.is_empty() {
                    HiddenLinesOthers { disclosure_content: true, hover: Some(text_content(&children)), items: children }
                } else {
                    HiddenLinesOthers { disclosure_content: false, hover: None, items: children }
                };
            }));
        }

        Rc::new(Self {
            base,
            editor,
            unchanged_region,
            unchanged_region_range,
            hide,
            reveal_modified_hidden_line,
            options,
            run_with_scroll_anchor,
            compact_control,
            view,
            breadcrumb_items,
            drag: RefCell::new(None),
            toggle_focused: Cell::new(false),
        })
    }

    /// O que o pintor lê do controle.
    pub fn view(&self) -> CollapsedCodeOverlayView {
        self.view.borrow().clone()
    }

    /// O topo e a altura do overlay, como a zona de visão os deu.
    pub fn layout(&self) -> ViewZoneOverlayLayout {
        self.base.layout()
    }

    /// O id do overlay no editor.
    pub fn overlay_widget_id(&self) -> &str {
        self.base.overlay_widget_id()
    }

    /// `toggle.focus()`: dá o foco ao `disclosure-toggle`, que só existe como alvo de foco no modo compacto com o controle à mostra.
    pub fn focus_toggle(&self) -> bool {
        let focusable = self.view.borrow().toggle.tab_index.is_some();
        if focusable {
            self.toggle_focused.set(true);
        }
        focusable
    }

    /// `document.activeElement === toggle`.
    pub fn is_toggle_focused(&self) -> bool {
        self.toggle_focused.get()
    }

    /// O `click` do `disclosure-toggle`.
    pub fn on_toggle_click(&self) {
        if self.compact_control && !self.hide {
            self.toggle_all();
        }
    }

    /// O `keydown` do `disclosure-toggle`: devolve se consumiu a tecla (`preventDefault()` e `stopPropagation()`).
    pub fn on_toggle_key_down(&self, key: &str, repeat: bool) -> bool {
        if !(self.compact_control && !self.hide) {
            return false;
        }
        if key == "Enter" || key == " " {
            if !repeat {
                self.toggle_all();
            }
            return true;
        }
        false
    }

    /// O `click` do `a.default-control`.
    pub fn on_default_control_click(&self) {
        if !self.hide {
            self.unchanged_region.show_all(None);
        }
    }

    /// O `dblclick` do `span` do texto (fora do modo compacto): devolve se consumiu o evento.
    pub fn on_label_double_click(&self, button: u8) -> bool {
        if self.hide || self.compact_control || button != 0 {
            return false;
        }
        self.unchanged_region.show_all(None);
        true
    }

    /// O `revealBreadcrumb(event)` do item `index` da trilha: o `click` (todos os modos).
    pub fn on_breadcrumb_click(&self, index: usize) {
        self.reveal_breadcrumb(index);
    }

    /// O `keydown` do item `index` da trilha (só existe no modo compacto): devolve se consumiu a tecla.
    pub fn on_breadcrumb_key_down(&self, index: usize, key: &str) -> bool {
        if !self.compact_control {
            return false;
        }
        if key == "Enter" || key == " " {
            self.reveal_breadcrumb(index);
            return true;
        }
        false
    }

    /// O `mousedown` do `div.top` ou do `div.bottom`; `button` é o botão do mouse e `client_y` o `clientY`.
    pub fn on_mouse_down(&self, edge: DraggedSide, button: u8, client_y: f64) {
        if button != 0 {
            return;
        }
        {
            let mut view = self.view.borrow_mut();
            match edge {
                DraggedSide::Top => view.top.dragging = true,
                DraggedSide::Bottom => view.bottom.dragging = true,
            }
            view.dragging = true;
        }
        let initial = match edge {
            DraggedSide::Top => self.unchanged_region.visible_line_count_top.get(),
            DraggedSide::Bottom => self.unchanged_region.visible_line_count_bottom.get(),
        };
        *self.drag.borrow_mut() = Some(DragState { edge, start_client_y: client_y, did_move: false, initial });
        self.unchanged_region.is_dragged.set(Some(edge), None);
    }

    /// Há um arrasto em andamento (os ouvintes de `mousemove` e `mouseup` da janela estão registrados).
    pub fn is_dragging(&self) -> bool {
        self.drag.borrow().is_some()
    }

    /// O `mousemove` da janela durante o arrasto.
    pub fn on_drag_move(&self, client_y: f64) {
        let Some(mut drag) = *self.drag.borrow() else { return };
        let delta = client_y - drag.start_client_y;
        drag.did_move = drag.did_move || delta.abs() > 2.0;
        *self.drag.borrow_mut() = Some(drag);

        let line_delta = js_round(delta / *self.editor.get_option(LINE_HEIGHT)) as i32;
        match drag.edge {
            DraggedSide::Top => {
                let new_val = (drag.initial + line_delta).min(self.unchanged_region.get_max_visible_line_count_top()).max(0);
                self.unchanged_region.visible_line_count_top.set(new_val, None);
            }
            DraggedSide::Bottom => {
                let new_val = (drag.initial - line_delta).min(self.unchanged_region.get_max_visible_line_count_bottom()).max(0);
                if new_val == self.unchanged_region.visible_line_count_bottom.get() {
                    return;
                }
                let region = self.unchanged_region.clone();
                self.run_with_lower_scroll_anchor(move || region.visible_line_count_bottom.set(new_val, None));
            }
        }
    }

    /// O `mouseup` da janela durante o arrasto.
    pub fn on_drag_up(&self) {
        let Some(drag) = self.drag.borrow_mut().take() else { return };
        let reveal_line_count = self.options.hide_unchanged_regions_reveal_line_count.get();
        match drag.edge {
            DraggedSide::Top => {
                if !drag.did_move {
                    if self.compact_control && self.is_fully_revealed() {
                        self.unchanged_region.collapse_all(None);
                    } else {
                        self.unchanged_region.show_more_above(Some(reveal_line_count), None);
                    }
                }
                self.set_dragging(DraggedSide::Top, false);
                self.unchanged_region.is_dragged.set(None, None);
            }
            DraggedSide::Bottom => {
                self.unchanged_region.is_dragged.set(None, None);

                if !drag.did_move {
                    if self.compact_control && self.is_fully_revealed() {
                        self.unchanged_region.collapse_all(None);
                    } else {
                        let region = self.unchanged_region.clone();
                        self.run_with_lower_scroll_anchor(move || region.show_more_below(Some(reveal_line_count), None));
                    }
                }
                self.set_dragging(DraggedSide::Bottom, false);
            }
        }
    }

    /// `this._nodes.<top | bottom>.classList.toggle('dragging', value)` e o mesmo na raiz.
    fn set_dragging(&self, edge: DraggedSide, value: bool) {
        let mut view = self.view.borrow_mut();
        match edge {
            DraggedSide::Top => view.top.dragging = value,
            DraggedSide::Bottom => view.bottom.dragging = value,
        }
        view.dragging = value;
    }

    fn reveal_breadcrumb(&self, index: usize) {
        let item = self.breadcrumb_items.borrow().get(index).cloned();
        if let Some(item) = item {
            (self.reveal_modified_hidden_line)(item.start_line_number);
        }
    }

    fn toggle_all(&self) {
        if self.is_fully_revealed() {
            self.unchanged_region.collapse_all(None);
        } else {
            self.unchanged_region.show_all(None);
        }
    }

    fn is_fully_revealed(&self) -> bool {
        is_fully_revealed(&self.unchanged_region, None)
    }

    fn run_with_lower_scroll_anchor(&self, update: impl FnOnce() + 'static) {
        let anchor_line_number = self.unchanged_region_range.end_line_number_exclusive;
        if let Some(run_with_scroll_anchor) = &self.run_with_scroll_anchor {
            run_with_scroll_anchor(anchor_line_number, Box::new(update));
            return;
        }

        let scroll_top = self.editor.get_scroll_top();
        let top = self.get_top_for_line_number(anchor_line_number);
        update();
        self.editor.set_scroll_top(scroll_top + self.get_top_for_line_number(anchor_line_number) - top, None);
    }

    fn get_top_for_line_number(&self, line_number: i32) -> f64 {
        if line_number > self.editor.get_model().expect("o editor do diff tem modelo").get_line_count() {
            self.editor.get_content_height()
        } else {
            self.editor.get_top_for_line_number(line_number, None)
        }
    }
}

impl IDisposable for CollapsedCodeOverlayWidget {
    fn dispose(&self) {
        // Os ouvintes da janela do arrasto vão embora com o controle.
        *self.drag.borrow_mut() = None;
        self.base.dispose();
    }
}

/// O controle que uma região sem mudanças tem num dos editores.
#[derive(Clone)]
pub enum HiddenLinesOverlay {
    Compact(Rc<CompactCollapsedCodeOverlayWidget>),
    Collapsed(Rc<CollapsedCodeOverlayWidget>),
}

/// Os controles de cada editor, na ordem das regiões.
#[derive(Clone, Default)]
pub struct HiddenLinesOverlays {
    pub original: Vec<HiddenLinesOverlay>,
    pub modified: Vec<HiddenLinesOverlay>,
}

/// Mantém um ouvinte de posição do cursor que torna visíveis as linhas dos cursores (`ensure*LineIsVisible`). `ensure_line_is_visible`
/// é `ensureOriginalLineIsVisible` ou `ensureModifiedLineIsVisible`.
fn listen_cursor_position(
    editor: &Rc<CodeEditorWidget>,
    diff_model: &IObservable<Option<Rc<DiffEditorViewModel>>>,
    ensure_line_is_visible: fn(&DiffEditorViewModel, i32, RevealPreference, Option<&dyn ITransaction>),
) -> DisposableRef {
    // O ouvinte vive no emissor do próprio editor: o `Weak` evita o ciclo.
    let weak_editor = Rc::downgrade(editor);
    let diff_model = diff_model.clone();
    editor.on_did_change_cursor_position().listen(move |e| {
        if e.reason == CursorChangeReason::ContentFlush {
            return;
        }
        let m = diff_model.get();
        let Some(editor) = weak_editor.upgrade() else { return };
        transaction(|tx| {
            for s in editor.get_selections().unwrap_or_default() {
                if let Some(m) = &m {
                    ensure_line_is_visible(m, s.get_start_position().line_number, RevealPreference::FromCloserSide, Some(tx));
                    ensure_line_is_visible(m, s.get_end_position().line_number, RevealPreference::FromCloserSide, Some(tx));
                }
            }
        });
    })
}

/// O alvo do mouse é o glifo de dobrar (`event.target.element?.className.includes('fold-unchanged')`, ver as divergências do módulo).
fn is_fold_unchanged_glyph(editor: &CodeEditorWidget, target: &MouseTarget) -> bool {
    let MouseTarget::Margin(margin) = target else { return false };
    if margin.target_type != MouseTargetType::GutterGlyphMargin {
        return false;
    }
    editor.get_line_decorations(margin.position.line_number).is_some_and(|decorations| {
        decorations.iter().any(|decoration| decoration.options.glyph_margin_class_name.as_deref().is_some_and(|class_name| class_name.contains("fold-unchanged")))
    })
}

/// Mantém o ouvinte de `onMouseUp` que dobra a região cujo glifo foi clicado. `unchanged_range_of` é `originalUnchangedRange` ou
/// `modifiedUnchangedRange`.
fn listen_fold_unchanged_click(
    editor: &Rc<CodeEditorWidget>,
    diff_model: &IObservable<Option<Rc<DiffEditorViewModel>>>,
    unchanged_range_of: fn(&UnchangedRegion) -> LineRange,
) -> DisposableRef {
    let weak_editor = Rc::downgrade(editor);
    let diff_model = diff_model.clone();
    editor.on_mouse_up().listen(move |event: &EditorMouseEvent| {
        let Some(editor) = weak_editor.upgrade() else { return };
        if event.event.right_button || !is_fold_unchanged_glyph(&editor, &event.target) {
            return;
        }
        let Some(position) = event.target.position() else { return };
        let line_number = position.line_number;
        let Some(model) = diff_model.get() else { return };
        let Some(region) = model.unchanged_regions.get().iter().find(|r| unchanged_range_of(r).contains(line_number)).cloned() else { return };
        region.collapse_all(None);
    })
}

/// `applyObservableDecorations` das linhas sem mudanças de um editor: a decoração do intervalo todo e, quando o controle está
/// escondido (fora da variante compacta), a do glifo de dobrar. `unchanged_range_of` e `fold_glyph_range_of` dão os intervalos do
/// lado do editor.
fn apply_unchanged_lines_decorations(
    editor: &Rc<CodeEditorWidget>,
    unchanged_regions: &IObservable<Rc<Vec<Rc<UnchangedRegion>>>>,
    variant: DiffEditorVariant,
    unchanged_lines_decoration: &Rc<ModelDecorationOptions>,
    unchanged_lines_decoration_show: &Rc<ModelDecorationOptions>,
    unchanged_range_of: fn(&UnchangedRegion) -> LineRange,
    fold_glyph_range_of: fn(&UnchangedRegion) -> Range,
) -> DisposableRef {
    let (unchanged_regions, unchanged_lines_decoration, unchanged_lines_decoration_show) =
        (unchanged_regions.clone(), unchanged_lines_decoration.clone(), unchanged_lines_decoration_show.clone());
    apply_observable_decorations(
        editor,
        derived(move |reader| {
            // decorations
            let reader: &dyn IReader = reader;
            let cur_unchanged_regions = unchanged_regions.read(reader);
            let mut result: Vec<ModelDeltaDecoration> = cur_unchanged_regions
                .iter()
                .map(|r| ModelDeltaDecoration {
                    range: unchanged_range_of(r).to_inclusive_range().expect("a região sem mudanças tem ao menos uma linha"),
                    options: unchanged_lines_decoration.clone(),
                })
                .collect();
            for r in cur_unchanged_regions.iter() {
                if variant != DiffEditorVariant::Compact && r.should_hide_controls(Some(reader)) {
                    result.push(ModelDeltaDecoration { range: fold_glyph_range_of(r), options: unchanged_lines_decoration_show.clone() });
                }
            }
            Rc::new(result)
        }),
    )
}

/// `HideUnchangedRegionsFeature`. Make sure to add the view zones to the editor!
pub struct HideUnchangedRegionsFeature {
    base: Rc<Disposable>,
    view_zones: IObservable<Rc<ObservableViewZonePair>>,
    is_updating_hidden_areas: Rc<Cell<bool>>,
    overlays: Rc<RefCell<HiddenLinesOverlays>>,
    original_editor_classes: Rc<EditorDomClasses>,
    modified_editor_classes: Rc<EditorDomClasses>,
}

impl HideUnchangedRegionsFeature {
    /// `HideUnchangedRegionsFeature._breadcrumbsSourceFactory`.
    pub fn breadcrumbs_source_factory() -> IObservable<BreadcrumbsSourceFactory> {
        BREADCRUMBS_SOURCE_FACTORY.with(|factory| factory.to_observable())
    }

    /// `HideUnchangedRegionsFeature.setBreadcrumbsSourceFactory(factory)`.
    pub fn set_breadcrumbs_source_factory(factory: BreadcrumbsSourceFactory) {
        BREADCRUMBS_SOURCE_FACTORY.with(|value| value.set(factory, None));
    }

    /// `new HideUnchangedRegionsFeature(editors, diffModel, options, runWithOriginalEditorScrollAnchor,
    /// runWithModifiedEditorScrollAnchor, variant, instantiationService)`, com `editors`, `diffModel`, `options` e
    /// `instantiationService` do contexto.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &DiffEditorFeatureContext,
        run_with_original_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        run_with_modified_editor_scroll_anchor: Option<ScrollAnchorRunner>,
        variant: DiffEditorVariant,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("HideUnchangedRegionsFeature");
        let base = Disposable::new();
        let editors: Rc<DiffEditorEditors> = context.editors.clone();
        let diff_model = context.diff_model.clone();
        let options = context.options.clone();
        let instantiation_service = context.instantiation_service.clone();

        let modified_outline_source = {
            let editors = editors.clone();
            derived_optional_disposable_of(Some(owner.clone()), move |reader| {
                let m = editors.modified_model.read(reader)?;
                let factory = BREADCRUMBS_SOURCE_FACTORY.with(|factory| factory.read(reader));
                Some(factory(&m, &instantiation_service))
            })
        };

        base.register(listen_cursor_position(&editors.original, &diff_model, DiffEditorViewModel::ensure_original_line_is_visible));
        base.register(listen_cursor_position(&editors.modified, &diff_model, DiffEditorViewModel::ensure_modified_line_is_visible));

        let unchanged_regions: IObservable<Rc<Vec<Rc<UnchangedRegion>>>> = {
            let editors = editors.clone();
            diff_model.map(move |m, reader| {
                let regions = m.map(|m| m.unchanged_regions.read(reader)).unwrap_or_default();
                if regions.len() == 1
                    && regions[0].modified_line_number == 1
                    && Some(regions[0].line_count) == editors.modified_model.read(reader).map(|model| model.get_line_count())
                {
                    return Rc::default();
                }
                regions
            })
        };

        let overlays: Rc<RefCell<HiddenLinesOverlays>> = Rc::default();
        let original_editor_classes: Rc<EditorDomClasses> = Rc::default();
        let modified_editor_classes: Rc<EditorDomClasses> = Rc::default();

        let view_zones = {
            let (editors, options, diff_model, unchanged_regions, overlays) =
                (editors.clone(), options.clone(), diff_model.clone(), unchanged_regions.clone(), overlays.clone());
            let (original_editor_classes, modified_editor_classes) = (original_editor_classes.clone(), modified_editor_classes.clone());
            derived_disposable_with(Some(owner.clone()), move |reader| {
                // view Zones
                let mut current_overlays = HiddenLinesOverlays::default();
                let Some(modified_outline_source) = modified_outline_source.read(reader) else {
                    *overlays.borrow_mut() = current_overlays;
                    return (Rc::new(ObservableViewZonePair { orig: Vec::new(), modified: Vec::new() }), None);
                };

                let store = DisposableStore::new();
                let mut orig_view_zones: Vec<Rc<dyn IObservableViewZone>> = Vec::new();
                let mut mod_view_zones: Vec<Rc<dyn IObservableViewZone>> = Vec::new();
                let side_by_side = options.render_side_by_side.read(reader);

                let compact_control = variant == DiffEditorVariant::Compact;
                let compact_mode = !compact_control && options.compact_mode.read(reader);

                let reveal_modified_hidden_line: Rc<dyn Fn(i32)> = {
                    let diff_model = diff_model.clone();
                    Rc::new(move |l: i32| {
                        diff_model.get().expect("o modelo do diff existe ao revelar uma linha").ensure_modified_line_is_visible(
                            l,
                            RevealPreference::FromBottom,
                            None,
                        );
                    })
                };

                let cur_unchanged_regions = unchanged_regions.read(reader);
                for (i, r) in cur_unchanged_regions.iter().enumerate() {
                    if !compact_control && r.should_hide_controls(Some(reader)) {
                        continue;
                    }

                    if compact_mode && (i == 0 || i == cur_unchanged_regions.len() - 1) {
                        continue;
                    }

                    let hidden_original_range_start = {
                        let r = r.clone();
                        derived(move |reader| {
                            // hiddenOriginalRangeStart
                            let reader: &dyn IReader = reader;
                            r.get_hidden_original_range(Some(reader)).start_line_number - 1
                        })
                    };
                    let hidden_modified_range_start = {
                        let r = r.clone();
                        derived(move |reader| {
                            // hiddenModifiedRangeStart
                            let reader: &dyn IReader = reader;
                            r.get_hidden_modified_range(Some(reader)).start_line_number - 1
                        })
                    };

                    if compact_mode {
                        let orig_vz = PlaceholderViewZone::new(hidden_original_range_start, 12.0);
                        orig_view_zones.push(orig_vz.clone());
                        let widget = CompactCollapsedCodeOverlayWidget::new(&editors.original, &orig_vz, r.clone(), !side_by_side);
                        store.add(widget.clone());
                        current_overlays.original.push(HiddenLinesOverlay::Compact(widget));

                        let mod_view_zone = PlaceholderViewZone::new(hidden_modified_range_start, 12.0);
                        mod_view_zones.push(mod_view_zone.clone());
                        let widget = CompactCollapsedCodeOverlayWidget::new(&editors.modified, &mod_view_zone, r.clone(), false);
                        store.add(widget.clone());
                        current_overlays.modified.push(HiddenLinesOverlay::Compact(widget));
                    } else {
                        let height_in_px = if compact_control { 40.0 } else { 24.0 };

                        let orig_vz = PlaceholderViewZone::new(hidden_original_range_start, height_in_px);
                        orig_view_zones.push(orig_vz.clone());
                        let widget = CollapsedCodeOverlayWidget::new(
                            &orig_vz,
                            CollapsedCodeOverlayArgs {
                                editor: editors.original.clone(),
                                unchanged_region: r.clone(),
                                unchanged_region_range: r.original_unchanged_range(),
                                hide: !side_by_side,
                                modified_outline_source: modified_outline_source.clone(),
                                reveal_modified_hidden_line: reveal_modified_hidden_line.clone(),
                                options: options.clone(),
                                run_with_scroll_anchor: run_with_original_editor_scroll_anchor.clone(),
                                compact_control,
                                editor_classes: original_editor_classes.clone(),
                            },
                        );
                        store.add(widget.clone());
                        current_overlays.original.push(HiddenLinesOverlay::Collapsed(widget));

                        let mod_view_zone = PlaceholderViewZone::new(hidden_modified_range_start, height_in_px);
                        mod_view_zones.push(mod_view_zone.clone());
                        let widget = CollapsedCodeOverlayWidget::new(
                            &mod_view_zone,
                            CollapsedCodeOverlayArgs {
                                editor: editors.modified.clone(),
                                unchanged_region: r.clone(),
                                unchanged_region_range: r.modified_unchanged_range(),
                                hide: false,
                                modified_outline_source: modified_outline_source.clone(),
                                reveal_modified_hidden_line: reveal_modified_hidden_line.clone(),
                                options: options.clone(),
                                run_with_scroll_anchor: run_with_modified_editor_scroll_anchor.clone(),
                                compact_control,
                                editor_classes: modified_editor_classes.clone(),
                            },
                        );
                        store.add(widget.clone());
                        current_overlays.modified.push(HiddenLinesOverlay::Collapsed(widget));
                    }
                }

                *overlays.borrow_mut() = current_overlays;
                let disposable: DisposableRef = store;
                (Rc::new(ObservableViewZonePair { orig: orig_view_zones, modified: mod_view_zones }), Some(disposable))
            })
        };

        let unchanged_lines_decoration = ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
            description: "unchanged lines".to_owned(),
            class_name: Some("diff-unchanged-lines".to_owned()),
            is_whole_line: true,
            ..ModelDecorationOptionsInit::default()
        });
        let unchanged_lines_decoration_show = ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
            description: "Fold Unchanged".to_owned(),
            glyph_margin_hover_message: Some(vec![MarkdownString { value: localize("foldUnchanged", "Fold Unchanged Region", &[]) }]),
            glyph_margin_class_name: Some(format!("fold-unchanged {}", ThemeIcon::from_id("fold").as_class_name())),
            z_index: 10001,
            ..ModelDecorationOptionsInit::default()
        });

        base.register(apply_unchanged_lines_decorations(
            &editors.original,
            &unchanged_regions,
            variant,
            &unchanged_lines_decoration,
            &unchanged_lines_decoration_show,
            UnchangedRegion::original_unchanged_range,
            |r| Range::from_positions(Position::new(r.original_line_number, 1), Position::new(r.original_line_number, 1)),
        ));
        base.register(apply_unchanged_lines_decorations(
            &editors.modified,
            &unchanged_regions,
            variant,
            &unchanged_lines_decoration,
            &unchanged_lines_decoration_show,
            UnchangedRegion::modified_unchanged_range,
            |r| LineRange::of_length(r.modified_line_number, 1).to_inclusive_range().expect("o intervalo de uma linha não está vazio"),
        ));

        let is_updating_hidden_areas = Rc::new(Cell::new(false));
        {
            // update folded unchanged regions
            let (editors, unchanged_regions, is_updating_hidden_areas) = (editors.clone(), unchanged_regions.clone(), is_updating_hidden_areas.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let cur_unchanged_regions = unchanged_regions.read(reader);
                is_updating_hidden_areas.set(true);
                try_finally(
                    || {
                        let original: Vec<Range> =
                            cur_unchanged_regions.iter().filter_map(|r| r.get_hidden_original_range(Some(reader)).to_inclusive_range()).collect();
                        editors.original.set_hidden_areas(&original, None, None);
                        let modified: Vec<Range> =
                            cur_unchanged_regions.iter().filter_map(|r| r.get_hidden_modified_range(Some(reader)).to_inclusive_range()).collect();
                        editors.modified.set_hidden_areas(&modified, None, None);
                    },
                    || is_updating_hidden_areas.set(false),
                );
            }));
        }

        base.register(listen_fold_unchanged_click(&editors.modified, &diff_model, UnchangedRegion::modified_unchanged_range));
        base.register(listen_fold_unchanged_click(&editors.original, &diff_model, UnchangedRegion::original_unchanged_range));

        Rc::new(Self { base, view_zones, is_updating_hidden_areas, overlays, original_editor_classes, modified_editor_classes })
    }

    /// Os controles de cada editor que o último cálculo das zonas de visão criou.
    pub fn overlays(&self) -> HiddenLinesOverlays {
        self.overlays.borrow().clone()
    }

    /// As classes do `div` do editor do lado dado.
    pub fn editor_dom_classes(&self, side: DiffEditorSide) -> &Rc<EditorDomClasses> {
        match side {
            DiffEditorSide::Original => &self.original_editor_classes,
            DiffEditorSide::Modified => &self.modified_editor_classes,
        }
    }
}

impl IHideUnchangedRegionsFeature for HideUnchangedRegionsFeature {
    fn view_zones(&self) -> IObservable<Rc<ObservableViewZonePair>> {
        self.view_zones.clone()
    }

    fn is_updating_hidden_areas(&self) -> bool {
        self.is_updating_hidden_areas.get()
    }
}

impl IDisposable for HideUnchangedRegionsFeature {
    fn dispose(&self) {
        self.base.dispose();
    }
}
