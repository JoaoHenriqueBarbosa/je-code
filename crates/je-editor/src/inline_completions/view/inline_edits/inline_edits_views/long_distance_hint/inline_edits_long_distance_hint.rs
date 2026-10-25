//! Porte de `.../inlineEditsViews/longDistanceHint/inlineEditsLongDistanceHint.ts`: `InlineEditsLongDistanceHint`, a dica
//! que aparece quando a próxima edição está longe do cursor (um widget com a prévia da linha de destino, a trilha de símbolos
//! ou o arquivo alvo e o rótulo "Tab to jump"), `ILongDistanceHint` e `ILongDistanceViewState` (em
//! `inline_completions::long_distance_hint`, junto do `scrollToReveal`).
//!
//! Notas de tradução:
//! - o `n.div` da raiz (o widget com a borda, o fundo, o contêiner do editor de prévia e a barra de baixo) é o
//!   [`LongDistanceHintVisual`], lido pelo pintor nativo em [`InlineEditsLongDistanceHint::visual`]. O editor de prévia é o
//!   de [`InlineEditsLongDistanceHint::preview_editor`], que o pintor desenha dentro de `editor_container`. O `display:
//!   block | none` é [`InlineEditsLongDistanceHint::is_displayed`]; a opacidade (`1` ou `0`, com a transição de
//!   [`OPACITY_TRANSITION_MS`]) vai no visual. Os estilos fixos do original (`position: absolute`, `overflow: hidden`,
//!   `cursor: pointer`, `display: flex` em coluna, a barra com `margin: 0 4px`) são do pintor;
//! - `renderIcon(icon)` é o [`ThemeIcon`] (`Codicon.arrowRight`, `arrowDown`, `arrowUp` e o ícone do tipo de símbolo); o espaço
//!   que o original põe depois do ícone (`' '`) e o `iconClasses` do arquivo alvo são do pintor, com o dado de
//!   [`LowerBarLeft`];
//! - `onmousedown` (com `preventDefault`, para o editor não perder o foco) é do pintor; o `onclick` é
//!   [`InlineEditsLongDistanceHint::notify_click`]; o `didMouseMoveDuringHover` do nó é o [`InlineEditsLongDistanceHint::notify_hover`],
//!   que o pintor chama quando o ponteiro entra ou sai do widget (`isHovered`);
//! - `_hintTextPosition` só servia para a posição existir quando o estado existe, então ele é o próprio estado;
//! - `border: borderColor.toString()` é a [`Color`] opaca (o pintor serializa); `getEditorBackgroundColor` e as cores do tema
//!   são identificadores do tema (`asCssVariable(id)` do original é só o `id`);
//! - o `OffsetRange` do crate é inteiro: os contornos que o `longDistnaceWidgetPlacement` devolve e a faixa a revelar
//!   arredondam as bordas para o pixel inteiro mais próximo (ver `long_distance_widget_placement`); o retângulo do widget
//!   (`lengthsToOffsetRanges`) é calculado em `f64` direto;
//! - `distributeFlexBoxLayout` devolve um mapa pela chave: as partes são o enum local [`FlexPart`];
//! - `showRects`, `showAvailableSpace`, `showRects2`, `showEditorRect` (constantes `false`), `debugView`, `stackSizesDown`
//!   (só usada sob `showRects`) e `drawEditorWidths` são depuração do original e não entram;
//! - `observableFromEvent(themeService.onDidColorThemeChange, ...)` do alto contraste é o `observable_from_event`; o
//!   `theme.type === 'hcDark' || 'hcLight'` é a conferência do [`ThemeType`];
//! - `derivedDisposable(...)` da fonte da trilha de símbolos é o `derived` que entrega o descarte da fonte ao `store` do
//!   leitor.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::utils::utils::{DebounceMs, debounced_observable2};
use je_base::observable::{
    IObservable, IReaderWithStore, Observable, ObservableExt, SettableExt, autorun, const_observable, derived, observable_from_event, observable_value,
};
use je_base::themables::ThemeIcon;
use je_theme::ThemeType;
use je_theme::color::Color;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::core_2d::dimension::Dimension;
use crate::core_2d::rect::Rect;
use crate::core_2d::size::Size2D;
use crate::cursor_common::CursorSimpleModel;
use crate::inline_completions::controller::command_ids::JUMP_TO_NEXT_INLINE_EDIT_ID;
use crate::inline_completions::inline_suggestion_item::InlineSuggestionAction;
use crate::inline_completions::long_distance_hint::{LongDistanceViewState, scroll_to_reveal};
use crate::inline_completions::long_distance_widget_placement::{
    ContinuousLineSizes, WidgetLayoutConstants, WidgetOutline, WidgetPlacementContext, split_into_continuous_line_ranges,
};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::inline_completions::utils::dispose_handle;
use crate::line_range::LineRange;
use crate::observable_code_editor::{ValueOrObservable, observable_code_editor};
use crate::offset_range::OffsetRange;
use crate::text_model::TextModel;

use super::super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::super::theme::{
    INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND, INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND,
    get_editor_background_color, get_editor_blended_color, observe_color,
};
use super::super::super::utils::flex_box_layout::{
    FlexBoxPart, FlexBoxPartExtensionRule, FlexBoxPartGrowthRule, distribute_flex_box_layout,
};
use super::super::super::utils::utils::get_content_size_of_lines;
use super::super::NonOverflowOverlayWidget;
use super::long_distance_preview_editor::{LongDistancePreviewEditor, LongDistancePreviewProps};
use super::LongDistanceHintServices;
use crate::diff_editor::features::hide_unchanged_regions_feature::{BreadcrumbItem, IDiffEditorBreadcrumbsSource};

const BORDER_RADIUS: f64 = 6.0;
const MAX_WIDGET_WIDTH_EMPTY_SPACE: f64 = 425.0;
const MAX_WIDGET_WIDTH_OVERLAY: f64 = 375.0;
const MIN_WIDGET_WIDTH: f64 = 250.0;

/// A duração da transição de opacidade do widget (`transition: 'opacity 200ms ease-in-out'`).
pub const OPACITY_TRANSITION_MS: f64 = 200.0;

/// `editorWidgetBackground` (`editorWidget.background`).
const EDITOR_WIDGET_BACKGROUND: &str = "editorWidget.background";
/// `editorWidgetBorder` (`editorWidget.border`).
const EDITOR_WIDGET_BORDER: &str = "editorWidget.border";
/// `descriptionForeground`.
const DESCRIPTION_FOREGROUND: &str = "descriptionForeground";

const DEFAULT_WIDGET_LAYOUT_CONSTANTS: WidgetLayoutConstants = WidgetLayoutConstants {
    preview_editor_margin: 2.0,
    widget_padding: 2.0,
    widget_border: 1.0,
    lower_bar_height: 20.0,
    min_widget_width: MIN_WIDGET_WIDTH,
};

/// As partes de `distributeFlexBoxLayout`: `spaceBefore`, `content` e `spaceAfter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FlexPart {
    SpaceBefore,
    Content,
    SpaceAfter,
}

/// `'overlay' | 'empty-space'`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WidgetPosition {
    Overlay,
    EmptySpace,
}

/// O `_styles` do original: `{ border, background }`.
struct HintStyles {
    border: Rc<Color>,
    background: &'static str,
}

/// O `_previewEditorLayoutInfo` do original.
pub struct LongDistanceLayoutInfo {
    pub code_editor_size: Size2D,
    pub code_scroll_left: f64,
    pub content_left: f64,

    pub widget_rect: Rect,

    pub preview_editor_margin: f64,
    pub widget_padding: f64,
    pub widget_border: f64,

    pub lower_bar_height: f64,

    pub desired_preview_editor_scroll_left: f64,
}

/// O lado esquerdo da barra de baixo: o arquivo alvo (edição entre arquivos) ou o símbolo que contém a linha de destino.
#[derive(Clone, Debug, PartialEq)]
pub enum LowerBarLeft {
    /// `div.target-file`: o ícone do arquivo (pelas classes) e o nome.
    TargetFile { file_name: String, icon_classes: Vec<String> },
    /// `div.outline-elements`: o primeiro símbolo (`slice(0, 1)`), com o nome e o tipo (o ícone é `SymbolKind::to_icon`) (`div.breadcrumb-item`).
    Outline { item: Option<BreadcrumbItem> },
}

/// O conteúdo da barra de baixo (`div.bar`): o lado esquerdo e o `div.go-to-label`.
#[derive(Clone, Debug, PartialEq)]
pub struct LowerBarContent {
    pub left: LowerBarLeft,
    /// `Go to file`, `Go to suggestion`, `Tab to open` ou `Tab to jump`.
    pub label: String,
    /// `arrowRight`, `arrowDown` ou `arrowUp`.
    pub arrow_icon: ThemeIcon,
}

/// O contêiner do editor de prévia (`div.editorContainer`: `overflow: hidden`, `pointer-events: none`).
#[derive(Clone, Debug, PartialEq)]
pub struct LongDistanceEditorContainer {
    pub padding: f64,
    /// O fundo, como identificador do tema.
    pub background_color: &'static str,
    /// O tamanho dado ao editor de prévia (`previewEditor.layout`).
    pub editor_size: (f64, f64),
}

/// O que o pintor nativo lê da view (o `n.div` do widget).
#[derive(Clone)]
pub struct LongDistanceHintVisual {
    pub widget_rect: Rect,
    pub padding: f64,
    pub border_width: f64,
    pub border_color: Rc<Color>,
    pub border_radius: f64,
    /// `asCssVariable(editorWidgetBackground)`.
    pub background_color: &'static str,
    /// `viewState.hint.isVisible ? '1' : '0'`, com a transição de [`OPACITY_TRANSITION_MS`].
    pub opacity: f64,
    pub editor_container: LongDistanceEditorContainer,
    /// A altura da barra de baixo e a cor do texto dela (`descriptionForeground`).
    pub lower_bar_height: f64,
    pub lower_bar_color: &'static str,
    /// `None` sem estado (a barra fica vazia).
    pub lower_bar: Option<Rc<LowerBarContent>>,
}

/// `InlineEditsLongDistanceHint`.
pub struct InlineEditsLongDistanceHint {
    base: Rc<Disposable>,
    view_state: IObservable<Option<Rc<LongDistanceViewState>>>,
    hovered: Rc<ObservableValue<bool, ()>>,
    display: IObservable<bool>,
    visual: IObservable<Option<Rc<LongDistanceHintVisual>>>,
    preview_editor: Rc<LongDistancePreviewEditor>,
}

impl InlineEditsLongDistanceHint {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        view_state: IObservable<Option<Rc<LongDistanceViewState>>>,
        preview_text_model: Rc<TextModel>,
        tab_action: IObservable<InlineEditTabAction>,
        services: LongDistanceHintServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);
        let hovered = observable_value::<bool, ()>("isHovered", false);

        let styles: IObservable<Rc<HintStyles>> = {
            let (tab_action, view_state, theme_service) = (Rc::clone(&tab_action), Rc::clone(&view_state), Rc::clone(&services.theme_service));
            derived(move |reader| {
                let v = tab_action.read(reader);

                // Check theme type by observing a color - this ensures we react to theme changes
                let widget_border_color = observe_color(EDITOR_WIDGET_BORDER, &theme_service).read(reader);
                let is_high_contrast = {
                    let themes = Rc::clone(&theme_service);
                    observable_from_event(theme_service.on_did_color_theme_change(), move |_| {
                        matches!(themes.get_color_theme().theme_type(), ThemeType::HighContrastDark | ThemeType::HighContrastLight)
                    })
                    .read(reader)
                };

                let border_color = if is_high_contrast {
                    // Use editorWidgetBorder in high contrast mode for better visibility
                    widget_border_color
                } else {
                    let border = match v {
                        InlineEditTabAction::Inactive => INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND,
                        InlineEditTabAction::Jump => INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND,
                        InlineEditTabAction::Accept => INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND,
                    };
                    get_editor_blended_color(ValueOrObservable::Value(border), &theme_service).read(reader)
                };

                let editor_type = view_state.read(reader).map_or(InlineCompletionEditorType::TextEditor, |state| state.editor_type);
                Rc::new(HintStyles { border: border_color, background: get_editor_background_color(editor_type) })
            })
        };

        let preview_editor = LongDistancePreviewEditor::new(
            preview_text_model,
            {
                let view_state = Rc::clone(&view_state);
                derived(move |reader| {
                    let view_state = view_state.read(reader)?;
                    Some(Rc::new(LongDistancePreviewProps {
                        diff: view_state.diff.clone(),
                        model: Rc::clone(&view_state.model),
                        inline_suggest_info: Rc::clone(&view_state.inline_suggest_info),
                        next_cursor_position: view_state.next_cursor_position,
                        target: Rc::clone(&view_state.target),
                    }))
                })
            },
            Rc::clone(&editor),
            Rc::clone(&tab_action),
            &services,
        );
        base.register(preview_editor.clone());

        let line_sizes_around_hint_position: IObservable<Rc<Vec<ContinuousLineSizes>>> = {
            let (view_state, editor, editor_obs) = (Rc::clone(&view_state), Rc::clone(&editor), Rc::clone(&editor_obs));
            derived(move |reader| {
                // O `_hintTextPosition` existe quando o estado existe.
                let Some(view_state) = view_state.read(reader) else {
                    return Rc::new(Vec::new());
                };

                let Some(model) = editor_obs.model.read(reader) else {
                    return Rc::new(Vec::new());
                };
                let line_count = CursorSimpleModel::get_line_count(&*model);
                let Some(range) = LineRange::of_length(view_state.hint.line_number, 1).add_margin(5, 5).intersect(&LineRange::of_length(1, line_count))
                else {
                    return Rc::new(Vec::new());
                };

                let sizes = get_content_size_of_lines(&editor_obs, range, Some(reader));
                let top = editor_obs.observe_top_for_line_number(range.start_line_number).read(reader);

                let result = split_into_continuous_line_ranges(range, &sizes, top, |line_number| editor.get_top_for_line_number(line_number, None));
                // Don't observe each line individually for performance reasons
                editor_obs.observe_top_for_line_number(range.end_line_number_exclusive - 1).read(reader);

                Rc::new(result)
            })
        };

        let is_visible_delayed: IObservable<Option<bool>> = debounced_observable2(
            {
                let view_state = Rc::clone(&view_state);
                derived(move |reader| view_state.read(reader).map(|state| state.hint.is_visible))
            },
            DebounceMs::Dynamic(Rc::new(|last_value: Option<&Option<bool>>, new_value: &Option<bool>| {
                if matches!(last_value, Some(Some(true))) && *new_value == Some(false) { 200 } else { 0 }
            })),
        )
        .recompute_initially_and_on_change(base.store(), None);

        let content_height = preview_editor.content_height();
        let horizontal_content_range = preview_editor.horizontal_content_range_in_preview_editor_to_show();

        let preview_editor_layout_info: IObservable<Option<Rc<LongDistanceLayoutInfo>>> = {
            let (view_state, editor_obs) = (Rc::clone(&view_state), Rc::clone(&editor_obs));
            derived(move |reader| {
                let view_state = view_state.read(reader)?;

                if !is_visible_delayed.read(reader).unwrap_or(false) {
                    return None;
                }

                let continuous_line_ranges = line_sizes_around_hint_position.read(reader);
                if continuous_line_ranges.is_empty() {
                    return None;
                }

                let editor_scroll_top = editor_obs.scroll_top.read(reader);
                let editor_scroll_left = editor_obs.scroll_left.read(reader);
                let editor_layout = editor_obs.layout_info.read(reader);

                let preview_content_height = content_height.read(reader);
                let preview_editor_content_layout = horizontal_content_range.read(reader);

                let (Some(preview_content_height), Some(preview_editor_content_layout)) =
                    (preview_content_height.filter(|height| *height != 0.0), preview_editor_content_layout)
                else {
                    return None;
                };

                let editor_true_content_width = editor_layout.content_width - editor_layout.vertical_scrollbar_width;
                let editor_true_content_right = editor_layout.content_left + editor_true_content_width;

                // `if (!c) return undefined`: sem cursor ou na linha zero.
                if editor_obs.cursor_line_number.read(reader).unwrap_or(0) == 0 {
                    return None;
                }

                let layout_constants = DEFAULT_WIDGET_LAYOUT_CONSTANTS;
                let extra_gutter_margin_to_avoid_scroll_bar = 2.0;
                let preview_editor_height = preview_content_height + extra_gutter_margin_to_avoid_scroll_bar;

                // Try to find widget placement in available empty space
                let mut possible_widget_outline: Option<WidgetOutline> = None;
                let mut last_placement_context: Option<WidgetPlacementContext> = None;

                let hint_line_number = view_state.hint.line_number;
                let end_of_line_padding = move |line_number: i32| if line_number == hint_line_number { 40.0 } else { 20.0 };

                for continuous_line_range in continuous_line_ranges.iter() {
                    let placement_context =
                        WidgetPlacementContext::new(continuous_line_range.clone(), editor_true_content_width, end_of_line_padding);

                    possible_widget_outline =
                        placement_context.try_find_widget_outline(hint_line_number, preview_editor_height, editor_true_content_right, layout_constants);
                    last_placement_context = Some(placement_context);

                    if possible_widget_outline.is_some() {
                        break;
                    }
                }

                // Fallback to overlay position if no empty space was found
                let mut position = WidgetPosition::EmptySpace;
                if possible_widget_outline.is_none() {
                    position = WidgetPosition::Overlay;
                    let max_available_width = (editor_layout.width - editor_layout.content_left).min(MAX_WIDGET_WIDTH_OVERLAY);

                    // Create a fallback placement context for computing overlay vertical position
                    let fallback_placement_context = last_placement_context
                        .unwrap_or_else(|| WidgetPlacementContext::new(continuous_line_ranges[0].clone(), editor_true_content_width, end_of_line_padding));

                    possible_widget_outline = fallback_placement_context
                        .get_widget_vertical_outline(hint_line_number + 2, preview_editor_height, layout_constants)
                        .map(|vertical_widget_range| WidgetOutline {
                            horizontal_widget_range: OffsetRange::of_start_and_length(
                                (editor_true_content_right - max_available_width).round() as i32,
                                max_available_width.round() as i32,
                            ),
                            vertical_widget_range: vertical_widget_range.delta(10),
                        });
                }

                let possible_widget_outline = possible_widget_outline?;

                let rect_available_space = Rect::from_ranges(possible_widget_outline.horizontal_widget_range, possible_widget_outline.vertical_widget_range)
                    .translate_x(-editor_scroll_left)
                    .translate_y(-editor_scroll_top);

                let WidgetLayoutConstants { preview_editor_margin, widget_padding, widget_border, lower_bar_height, .. } = layout_constants;
                let max_widget_width_upper_bound =
                    if position == WidgetPosition::Overlay { MAX_WIDGET_WIDTH_OVERLAY } else { MAX_WIDGET_WIDTH_EMPTY_SPACE };
                // Honor the same `minWidgetWidth` the placement logic guarantees, so a short/empty preview line
                // (e.g. a jump onto an empty line) can't collapse the widget below its reserved space.
                let content_based_widget_width = preview_editor_content_layout.max_editor_width + preview_editor_margin + widget_padding;
                let max_widget_width = max_widget_width_upper_bound.min(content_based_widget_width.max(layout_constants.min_widget_width));

                let layout: HashMap<FlexPart, f64> = distribute_flex_box_layout(
                    rect_available_space.width(),
                    &[
                        (
                            FlexPart::SpaceBefore,
                            FlexBoxPart::Growth(FlexBoxPartGrowthRule { min: Some(0.0), max: Some(10.0), priority: Some(1.0), ..Default::default() }),
                        ),
                        (
                            FlexPart::Content,
                            FlexBoxPart::Growth(FlexBoxPartGrowthRule {
                                min: Some(50.0),
                                rules: Some(vec![
                                    FlexBoxPartExtensionRule { max: Some(150.0), priority: Some(2.0), share: None },
                                    FlexBoxPartExtensionRule { max: Some(max_widget_width), priority: Some(1.0), share: None },
                                ]),
                                ..Default::default()
                            }),
                        ),
                        (FlexPart::SpaceAfter, FlexBoxPart::Growth(FlexBoxPartGrowthRule { min: Some(10.0), ..Default::default() })),
                    ],
                )?;

                // `lengthsToOffsetRanges([spaceBefore, content, spaceAfter], rectAvailableSpace.left)`: só a faixa do conteúdo é usada.
                let widget_left = rect_available_space.left + layout[&FlexPart::SpaceBefore];
                let widget_rect = Rect::new(widget_left, rect_available_space.top, widget_left + layout[&FlexPart::Content], rect_available_space.bottom);

                let preview_editor_rect =
                    widget_rect.with_margin(-widget_padding - widget_border - preview_editor_margin).with_margins(0.0, 0.0, -lower_bar_height, 0.0);

                let preview_editor_content_width = preview_editor_rect.width() - preview_editor_content_layout.non_content_width;
                let max_preferred_range_length = preview_editor_content_width * 0.8;
                let preferred = preview_editor_content_layout.preferred_range_to_reveal;
                let preferred_range_to_reveal = preferred
                    .intersect(OffsetRange::of_start_and_length(preferred.start, max_preferred_range_length.round() as i32))
                    .unwrap_or(preferred);
                let desired_preview_editor_scroll_left = scroll_to_reveal(
                    preview_editor_content_layout.indentation_end.round() as i32,
                    preview_editor_content_width.round() as i32,
                    preferred_range_to_reveal,
                );

                Some(Rc::new(LongDistanceLayoutInfo {
                    code_editor_size: preview_editor_rect.get_size(),
                    code_scroll_left: editor_scroll_left,
                    content_left: editor_layout.content_left,

                    widget_rect,

                    preview_editor_margin,
                    widget_padding,
                    widget_border,

                    lower_bar_height,

                    desired_preview_editor_scroll_left: f64::from(desired_preview_editor_scroll_left.new_scroll_position),
                }))
            })
        };

        let display: IObservable<bool> = preview_editor_layout_info.map(|layout_info, _| layout_info.is_some());

        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(const_observable(None), const_observable(0.0))));

        // Drives breadcrumbs and symbol icon
        let original_target_line_number: IObservable<i32> = {
            let view_state = Rc::clone(&view_state);
            derived(move |reader| {
                let Some(view_state) = view_state.read(reader) else {
                    return -1;
                };

                if let Some(InlineSuggestionAction::JumpTo(jump_to)) = &view_state.edit.action {
                    return jump_to.position.line_number;
                }

                view_state.diff.first().map_or(-1, |mapping| mapping.original.start_line_number)
            })
        };

        let original_outline_source: IObservable<Option<Rc<dyn IDiffEditorBreadcrumbsSource>>> = {
            let (editor_obs, host) = (Rc::clone(&editor_obs), Rc::clone(&services.host));
            derived(move |reader| {
                let model = editor_obs.model.read(reader)?;
                let source = host.outline_source(&model, reader)?;
                reader.store().add(dispose_handle(&source));
                Some(source)
            })
        };

        let lower_bar_content: IObservable<Option<Rc<LowerBarContent>>> = {
            let (view_state, editor_obs, host, keybinding_service) =
                (Rc::clone(&view_state), Rc::clone(&editor_obs), Rc::clone(&services.host), Rc::clone(&services.keybinding_service));
            derived(move |reader| {
                let view_state = view_state.read(reader)?;

                // Check if this is a cross-file edit
                let current_model = editor_obs.model.read(reader);
                let is_cross_file_edit = current_model.as_ref().is_none_or(|model| !view_state.target.targets(model));

                let original_target_line_number = original_target_line_number.read(reader);

                let left = if is_cross_file_edit {
                    // For cross-file edits, show target filename instead of outline
                    let info = host.target_file(&view_state.target.uri());
                    LowerBarLeft::TargetFile { file_name: info.file_name, icon_classes: info.icon_classes }
                } else {
                    // Outline Element
                    let source = original_outline_source.read(reader);
                    let item = source.and_then(|source| source.get_at(original_target_line_number, reader).into_iter().next());
                    LowerBarLeft::Outline { item }
                };

                // Show Edit Direction
                let arrow_icon = if is_cross_file_edit {
                    "arrow-right"
                } else if view_state.hint.line_number < original_target_line_number {
                    "arrow-down"
                } else {
                    "arrow-up"
                };
                let keybinding = keybinding_service.lookup_keybinding(JUMP_TO_NEXT_INLINE_EDIT_ID, None, false);
                let mut label = if is_cross_file_edit { "Go to file" } else { "Go to suggestion" };
                if keybinding.and_then(|keybinding| keybinding.get_label()).as_deref() == Some("Tab") {
                    label = if is_cross_file_edit { "Tab to open" } else { "Tab to jump" };
                }

                Some(Rc::new(LowerBarContent { left, label: label.to_owned(), arrow_icon: ThemeIcon::from_id(arrow_icon) }))
            })
        };

        let visual: IObservable<Option<Rc<LongDistanceHintVisual>>> = {
            let (layout_info, view_state) = (Rc::clone(&preview_editor_layout_info), Rc::clone(&view_state));
            derived(move |reader| {
                let layout_info = layout_info.read(reader)?;
                let styles = styles.read(reader);
                let is_visible = view_state.read(reader).is_some_and(|state| state.hint.is_visible);

                Some(Rc::new(LongDistanceHintVisual {
                    widget_rect: layout_info.widget_rect,
                    padding: layout_info.widget_padding,
                    border_width: layout_info.widget_border,
                    border_color: Rc::clone(&styles.border),
                    border_radius: BORDER_RADIUS,
                    background_color: EDITOR_WIDGET_BACKGROUND,
                    opacity: if is_visible { 1.0 } else { 0.0 },
                    editor_container: LongDistanceEditorContainer {
                        padding: layout_info.preview_editor_margin,
                        background_color: styles.background,
                        editor_size: (layout_info.code_editor_size.width, layout_info.code_editor_size.height),
                    },
                    lower_bar_height: layout_info.lower_bar_height,
                    lower_bar_color: DESCRIPTION_FOREGROUND,
                    lower_bar: lower_bar_content.read(reader),
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        base.register(autorun({
            let (preview_editor, layout_info) = (Rc::clone(&preview_editor), Rc::clone(&preview_editor_layout_info));
            move |reader| {
                let Some(layout_info) = layout_info.read(reader) else {
                    return;
                };
                preview_editor.layout(
                    Dimension { height: layout_info.code_editor_size.height, width: layout_info.code_editor_size.width },
                    layout_info.desired_preview_editor_scroll_left,
                );
            }
        }));

        Rc::new(Self { base, view_state, hovered, display, visual, preview_editor })
    }

    /// `_display`: o widget só aparece com o layout calculado (`display: block | none`).
    pub fn is_displayed(&self) -> IObservable<bool> {
        Rc::clone(&self.display)
    }

    /// O dado que o pintor nativo lê (`None` sem layout).
    pub fn visual(&self) -> IObservable<Option<Rc<LongDistanceHintVisual>>> {
        Rc::clone(&self.visual)
    }

    /// O editor de prévia que o pintor desenha dentro de `editor_container`.
    pub fn preview_editor(&self) -> &Rc<LongDistancePreviewEditor> {
        &self.preview_editor
    }

    /// O `didMouseMoveDuringHover` do nó do widget: o pintor avisa quando o ponteiro entra ou sai dele.
    pub fn notify_hover(&self, hovered: bool) {
        self.hovered.set(hovered, None);
    }

    /// O `onclick` do widget: salta para a sugestão.
    pub fn notify_click(&self) {
        if let Some(view_state) = self.view_state.get() {
            (view_state.model.jump)();
        }
    }
}

impl IInlineEditsView for InlineEditsLongDistanceHint {
    fn is_hovered(&self) -> IObservable<bool> {
        self.hovered.to_observable()
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        Event::none()
    }
}

impl IDisposable for InlineEditsLongDistanceHint {
    fn dispose(&self) {
        self.base.dispose();
    }
}
