//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsWordReplacementView.ts`:
//! `InlineEditsWordReplacementView`, a view de uma palavra substituída (o texto novo numa caixa abaixo da palavra
//! original, com a seta que liga as duas e, quando há, o botão da ação alternativa).
//!
//! Notas de tradução:
//! - o `n.div` da raiz (a sobreposição, o fundo inferior, o `widget` com o texto e a ação alternativa, a moldura da palavra
//!   original e a seta em SVG) é o [`WordReplacementVisual`], lido pelo pintor nativo em
//!   [`InlineEditsWordReplacementView::visual`] (ver as notas do módulo pai). O `mapOutFalsy` do original só mantém a
//!   árvore DOM estável entre recálculos; aqui há um único dado calculado, então ele não aparece. O `widget` é um
//!   `display: flex` com `justifyContent: left` e sem largura própria (`width: undefined`): o pintor põe o texto e o botão
//!   lado a lado, e o botão leva a margem esquerda de [`RENAME_MARGIN_LEFT`];
//! - o `_line` (um `div` preenchido por `renderLines`) é a linha tokenizada em [`WordReplacementPart::line`]: o pintor a
//!   renderiza com as opções do editor (`RenderOptions.fromEditor(editor).withSetWidth(false).withScrollBeyondLastColumn(0)`)
//!   e dá a ela a largura que a renderização mede. O `_renderTextEffect` é o derivado dessa linha; como ele não lê
//!   nenhum observável (o modelo é lido sem rastrear), ele roda uma vez, como no original;
//! - `IUserInteractionService.createHoverTracker` e `readModifierKeyStatus` são eventos do nó DOM e do teclado: o pintor
//!   os entrega por [`InlineEditsWordReplacementView::notify_hover`] (a caixa do texto novo) e
//!   [`InlineEditsWordReplacementView::notify_shift_key`] (o estado do shift enquanto o editor tem o foco). O `isHovered`
//!   continua sendo o derivado do original: falso sem a caixa, senão o valor do pintor. A dica atrasada
//!   (`setupDelayedHoverAtMouse`) é o [`WordReplacementAlternativeAction::tooltip`]: o pintor a mostra junto do mouse;
//! - `KeybindingLabel` monta o atalho num elemento DOM; o dado é o [`ResolvedKeybinding`] em
//!   [`WordReplacementAlternativeAction::keybinding`], que o pintor desenha como o original (`unthemedKeybindingLabelOptions`,
//!   sem título). O `icon` da ação alternativa é sempre `undefined` no original (`Do not render icon fo the moment`), então
//!   não é um campo;
//! - `alternativeAction.active` é um observável no original, lido logo depois no mesmo derivado do layout; aqui é o
//!   booleano lido ali, e o `alternativeActionActive` (`active || secondaryElementHovered`, com o segundo fixo em falso)
//!   é o próprio `active`;
//! - `_mouseDown` sobe a árvore do alvo até um id conhecido: o pintor resolve o alvo no [`WordReplacementTarget`] e chama
//!   [`InlineEditsWordReplacementView::notify_mouse_down`] (o `preventDefault`, que impede o editor de perder o foco, é
//!   dele); `None` é nenhum id encontrado;
//! - `asCssVariable(id)` é o próprio identificador do tema; as cores calculadas (alto contraste, borda passiva) são o
//!   [`WordReplacementColor::Resolved`];
//! - `@ILanguageService` é o `Rc<dyn ILanguageService>` (só serve ao `languageIdCodec`), `@IThemeService` o
//!   `Rc<dyn IThemeService>` e `@IKeybindingService` o `Rc<dyn IKeybindingService>`;
//! - o `Math.round` da posição da seta é `floor(x + 0.5)`, que arredonda a metade para cima como o do JavaScript.

use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, Observable, ObservableExt, SettableExt, const_observable, derived, observable_from_event, observable_from_promise, observable_value,
};
use je_platform::keybinding::IKeybindingService;
use je_theme::ThemeType;
use je_theme::color::Color;

use crate::code_editor_widget::ICodeEditor;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::font_size::FONT_SIZE;
use crate::config::editor_options::font_weight::FONT_WEIGHT;
use crate::config::editor_options::registry::FONT_FAMILY;
use crate::core_2d::point::Point;
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::edits::edit::BaseReplacement;
use crate::edits::string_edit::{BaseStringReplacement, StringReplacement};
use crate::edits::text_edit::TextReplacement;
use crate::editor_browser::{MouseEvent, OverlayWidgetPlacement, OverlayWidgetPosition, OverlayWidgetPositionCoordinates};
use crate::inline_completions::controller::command_ids::INLINE_SUGGEST_COMMIT_ALTERNATIVE_ACTION_ID;
use crate::inline_completions::inline_suggest_alternative_action::InlineSuggestAlternativeAction;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::language_service::ILanguageService;
use crate::line_tokens::{LineTokens, TokenArray};
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable};
use crate::offset_range::OffsetRange;
use crate::services::theme_service::IThemeService;

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{
    INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_PRIMARY_BORDER, INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND,
    INLINE_EDITS_BORDER_RADIUS, MODIFIED_CHANGED_TEXT_OVERLAY_COLOR, ORIGINAL_CHANGED_TEXT_OVERLAY_COLOR, get_editor_background_color,
    get_modified_border_color, get_original_border_color, observe_color,
};
use super::super::utils::utils::{PathBuilder, PathCommand, get_editor_valid_overlay_rect};
use super::NonOverflowOverlayWidget;

/// `BORDER_WIDTH`.
pub const BORDER_WIDTH: f64 = 1.0;
/// O `marginLeft: '4px'` do botão da ação alternativa.
pub const RENAME_MARGIN_LEFT: f64 = 4.0;
/// O `padding: '0 4px 0 1px'` do botão da ação alternativa: direita e esquerda.
pub const RENAME_PADDING_RIGHT: f64 = 4.0;
pub const RENAME_PADDING_LEFT: f64 = 1.0;
/// O `outline: 2px solid` do `widget`.
pub const WIDGET_OUTLINE_WIDTH: f64 = 2.0;
/// A largura do SVG da seta.
pub const ARROW_WIDTH: f64 = 11.0;
/// A altura do SVG da seta.
pub const ARROW_HEIGHT: f64 = 14.0;

/// `editorHoverForeground` (`editorHoverWidget.foreground`).
const EDITOR_HOVER_FOREGROUND: &str = "editorHoverWidget.foreground";
/// `contrastBorder`.
const CONTRAST_BORDER: &str = "contrastBorder";

/// Os ids do DOM (`DOM_ID_OVERLAY`, `DOM_ID_WIDGET`, `DOM_ID_REPLACEMENT` e `DOM_ID_RENAME`) que o `_mouseDown` procura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordReplacementTarget {
    /// `word-replacement-view-overlay`.
    Overlay,
    /// `word-replacement-view-widget`.
    Widget,
    /// `word-replacement-view-replacement`.
    Replacement,
    /// `word-replacement-view-rename`.
    Rename,
}

/// `WordReplacementsViewData`.
#[derive(Clone)]
pub struct WordReplacementsViewData {
    pub edit: TextReplacement,
    pub editor_type: InlineCompletionEditorType,
    /// A identidade da ação (o `===` do original) é a do `Rc`.
    pub alternative_action: Option<Rc<InlineSuggestAlternativeAction>>,
}

impl WordReplacementsViewData {
    pub fn new(edit: TextReplacement, editor_type: InlineCompletionEditorType, alternative_action: Option<Rc<InlineSuggestAlternativeAction>>) -> Self {
        Self { edit, editor_type, alternative_action }
    }

    /// `equals(other)`.
    pub fn equals(&self, other: &WordReplacementsViewData) -> bool {
        let same_action = match (&self.alternative_action, &other.alternative_action) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.edit.equals(&other.edit) && same_action
    }
}

/// A ação alternativa do layout: `{ label, tooltip, icon, count, keybinding, active }`.
#[derive(Clone)]
pub struct WordReplacementAlternativeAction {
    pub label: String,
    pub tooltip: Option<String>,
    pub count: Option<f64>,
    pub keybinding: Option<Rc<dyn ResolvedKeybinding>>,
    pub active: bool,
}

/// O `_layout` do original.
#[derive(Clone)]
pub struct WordReplacementLayout {
    pub alternative_action: Option<WordReplacementAlternativeAction>,
    pub original_line: Rect,
    pub code_line: Rect,
    pub modified_line: Rect,
    pub lower_background: Rect,
    pub line_height: f64,
}

/// Uma cor do estilo: o identificador do tema (`asCssVariable`) ou a cor já calculada.
#[derive(Clone)]
pub enum WordReplacementColor {
    Theme(&'static str),
    Resolved(Rc<Color>),
}

/// `primaryActiveStyles`, `secondaryActiveStyles` e `passiveStyles`.
#[derive(Clone)]
pub struct WordReplacementStyles {
    pub border_color: WordReplacementColor,
    pub background_color: WordReplacementColor,
    /// `color` (vazio no original é `None`: a cor herdada).
    pub color: Option<WordReplacementColor>,
    pub opacity: f64,
}

/// O `div` `DOM_ID_REPLACEMENT`: a caixa do texto novo.
#[derive(Clone)]
pub struct WordReplacementPart {
    /// A largura (`codeLine.withMargin(BORDER_WIDTH, 2 * BORDER_WIDTH).width`).
    pub width: f64,
    /// `border`, `padding` (`BORDER_WIDTH`) e `box-sizing: border-box`.
    pub border_width: f64,
    pub border_radius: f64,
    pub styles: WordReplacementStyles,
    pub font_family: Rc<String>,
    pub font_size: f64,
    pub font_weight: Rc<String>,
    /// A linha tokenizada (`_line`); `None` sem modelo.
    pub line: Option<Rc<LineTokens>>,
    /// O `lineHeight` da linha: `modifiedLine.height + 2 * BORDER_WIDTH`.
    pub line_height: f64,
}

/// O `div` `DOM_ID_RENAME`: o botão da ação alternativa.
#[derive(Clone)]
pub struct WordReplacementRename {
    pub action: WordReplacementAlternativeAction,
    pub styles: WordReplacementStyles,
    pub border_width: f64,
    pub border_radius: f64,
}

/// A seta em SVG: `11x14`, sem preenchimento, com o contorno nas duas linhas.
#[derive(Clone)]
pub struct WordReplacementArrow {
    pub left: f64,
    pub top: f64,
    pub paths: [Vec<PathCommand>; 2],
    /// O `stroke` (`editorHoverForeground`), como identificador do tema.
    pub stroke_color: &'static str,
}

/// O que o pintor nativo lê da view. Os contêineres de sobreposição têm `pointer-events: none`, exceto o fundo inferior,
/// o `widget` e a caixa do texto novo (`auto`, com `cursor: pointer`).
#[derive(Clone)]
pub struct WordReplacementVisual {
    pub layout: Rc<WordReplacementLayout>,
    /// A caixa do contêiner (`getEditorValidOverlayRect`), com `overflow: hidden`.
    pub overlay_rect: Rect,
    /// O fundo inferior (`lowerBackground.withMargin(BORDER_WIDTH, 2 * BORDER_WIDTH, BORDER_WIDTH, 0)`).
    pub lower_background_rect: Rect,
    /// O `widget` (`modifiedLine.withMargin(BORDER_WIDTH, 2 * BORDER_WIDTH)`): sem largura própria, ver as notas do módulo.
    pub widget_rect: Rect,
    /// O fundo do editor, usado no fundo inferior e no `widget` (e no `outline` de [`WIDGET_OUTLINE_WIDTH`]).
    pub editor_background_color: &'static str,
    pub replacement: WordReplacementPart,
    pub rename: Option<WordReplacementRename>,
    /// A moldura da palavra original (`originalLine.withMargin(BORDER_WIDTH)`).
    pub original_rect: Rect,
    pub original_border_color: &'static str,
    pub original_background_color: &'static str,
    pub arrow: WordReplacementArrow,
}

/// `InlineEditsWordReplacementView`.
pub struct InlineEditsWordReplacementView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    is_hovered: IObservable<bool>,
    primary_hovered: Rc<ObservableValue<bool, ()>>,
    shift_pressed: Rc<ObservableValue<bool, ()>>,
    min_editor_scroll_height: IObservable<f64>,
    visual: IObservable<Option<Rc<WordReplacementVisual>>>,
}

/// `renderTextEffect`: a linha do texto novo tokenizada (a do modelo com a troca aplicada, só o trecho novo).
fn tokenize_replacement(
    editor: &Rc<ObservableCodeEditor>,
    language_service: &Rc<dyn ILanguageService>,
    view_data: &WordReplacementsViewData,
) -> Option<Rc<LineTokens>> {
    let tm = editor.model.get()?;
    let range = view_data.edit.range;
    let text = view_data.edit.text.clone();
    let orig_line = tm.get_line_content(range.start_line_number);

    let edit = StringReplacement::replace(OffsetRange::new(range.start_column - 1, range.end_column - 1), text.clone());
    let line_to_tokenize = BaseStringReplacement::replace(&edit, orig_line.units());
    let tokenized = tm.tokenization().tokenize_lines_at(range.start_line_number, std::slice::from_ref(&line_to_tokenize)).and_then(|lines| lines.into_iter().next());
    let language_id_codec = language_service.language_id_codec();
    let tokens = match tokenized {
        Some(t) => TokenArray::from_line_tokens(&t).slice(edit.get_range_after_replace()).to_line_tokens(&text, language_id_codec),
        None => LineTokens::create_empty(text, language_id_codec),
    };
    Some(Rc::new(tokens))
}

/// O texto do botão com as ocorrências (`labelOccurence` e `labelOccurences`).
fn occurrences_label(label: &str, count: Option<f64>) -> String {
    match count {
        Some(count) if count == 1.0 => localize("labelOccurence", "{0} 1 occurrence", &[label]),
        Some(count) => localize("labelOccurences", "{0} {1} occurrences", &[label, &count.to_string()]),
        None => label.to_owned(),
    }
}

/// A seta em SVG (`M1 0C...` e `M6 7.5L...`).
fn arrow_paths() -> [Vec<PathCommand>; 2] {
    let p = Point::new;
    let mut first = PathBuilder::new();
    first
        .move_to(p(1.0, 0.0))
        .curve_to2(p(1.0, 2.98966), p(1.0, 5.92087), p(1.0, 8.49952))
        .curve_to2(p(1.0, 9.60409), p(1.89543, 10.5), p(3.0, 10.5))
        .line_to(p(10.5, 10.5));
    let mut second = PathBuilder::new();
    second.move_to(p(6.0, 7.5)).line_to(p(9.99999, 10.49998)).line_to(p(6.0, 13.5));
    [first.build(), second.build()]
}

impl InlineEditsWordReplacementView {
    pub const MAX_LENGTH: usize = 100;

    pub fn new(
        editor: Rc<ObservableCodeEditor>,
        view_data: WordReplacementsViewData,
        tab_action: IObservable<InlineEditTabAction>,
        language_service: Rc<dyn ILanguageService>,
        theme_service: Rc<dyn IThemeService>,
        keybinding_service: Rc<dyn IKeybindingService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(on_did_click.clone());
        let view_data = Rc::new(view_data);

        let start = editor.observe_position(&const_observable(Some(view_data.edit.range.get_start_position())), base.store());
        let end = editor.observe_position(&const_observable(Some(view_data.edit.range.get_end_position())), base.store());

        let primary_hovered = observable_value::<bool, ()>("primaryHovered", false);
        let shift_pressed = observable_value::<bool, ()>("altModifierActive", false);

        // `_renderTextEffect`
        let render_text_effect: IObservable<Option<Rc<LineTokens>>> = {
            let (editor, language_service, view_data) = (Rc::clone(&editor), Rc::clone(&language_service), Rc::clone(&view_data));
            derived(move |_| tokenize_replacement(&editor, &language_service, &view_data))
        };

        let modified_line_height = editor.observe_line_height_for_position(ValueOrObservable::Value(view_data.edit.range.get_start_position()));
        let alt_count: IObservable<Rc<Option<f64>>> = match &view_data.alternative_action {
            Some(action) => observable_from_promise(action.count.clone()),
            None => const_observable(Rc::new(None)),
        };
        let alt_modifier_active = shift_pressed.to_observable();

        // `_layout`
        let layout: IObservable<Option<Rc<WordReplacementLayout>>> = {
            let (editor, view_data, render_text_effect) = (Rc::clone(&editor), Rc::clone(&view_data), Rc::clone(&render_text_effect));
            let font_info = editor.get_option(FONT_INFO);
            let scroll_left = Rc::clone(&editor.scroll_left);
            derived(move |reader| {
                render_text_effect.read(reader);
                let widget_start = start.read(reader);
                let widget_end = end.read(reader);

                let (Some(widget_start), Some(widget_end)) = (widget_start, widget_end) else {
                    return None;
                };
                if widget_start.x > widget_end.x || widget_start.y > widget_end.y {
                    return None;
                }

                let line_height = modified_line_height.read(reader);
                if line_height <= 0.0 {
                    return None;
                }
                let scroll_left = scroll_left.read(reader);
                let w = font_info.read(reader).typical_halfwidth_character_width;

                let modified_left_offset = 3.0 * w;
                let modified_top_offset = 4.0;
                let modified_offset = Point::new(modified_left_offset, modified_top_offset);

                let alternative_action = view_data.alternative_action.as_ref().map(|action| {
                    let label = action.label.as_str();
                    let count = *alt_count.read(reader);
                    let active = alt_modifier_active.read(reader);
                    let occurrences_label = occurrences_label(label, count);
                    let keybinding_tooltip = localize("shiftToSeeOccurences", "{0} show occurrences", &["[shift]"]);
                    WordReplacementAlternativeAction {
                        label: if count.is_some() && active { occurrences_label.clone() } else { label.to_owned() },
                        tooltip: if occurrences_label.is_empty() { None } else { Some(format!("{occurrences_label}\n{keybinding_tooltip}")) },
                        count,
                        keybinding: keybinding_service.lookup_keybinding(INLINE_SUGGEST_COMMIT_ALTERNATIVE_ACTION_ID, None, false),
                        active,
                    }
                });

                let original_line = Rect::from_points(widget_start, widget_end).with_height(line_height).translate_x(-scroll_left);
                let code_line = Rect::from_point_size(
                    original_line.get_left_bottom().add(modified_offset),
                    Point::new(view_data.edit.text.len() as f64 * w, original_line.height()),
                );
                let modified_line = code_line.with_width(
                    code_line.width()
                        + alternative_action.as_ref().map_or(0.0, |action| JsString::from(action.label.as_str()).len() as f64 * w + 8.0 + 4.0 + 12.0),
                );
                let lower_background = modified_line.with_left(original_line.left);

                Some(Rc::new(WordReplacementLayout { alternative_action, original_line, code_line, modified_line, lower_background, line_height }))
            })
        };

        let min_editor_scroll_height: IObservable<f64> = {
            let (layout, editor) = (Rc::clone(&layout), Rc::clone(&editor));
            derived(move |reader| match layout.read(reader) {
                Some(layout) => layout.modified_line.bottom + BORDER_WIDTH + editor.editor.get_scroll_top(),
                None => 0.0,
            })
        };

        // `_root`
        let visual: IObservable<Option<Rc<WordReplacementVisual>>> = {
            let (editor, view_data, layout, render_text_effect) = (Rc::clone(&editor), Rc::clone(&view_data), Rc::clone(&layout), Rc::clone(&render_text_effect));
            let original_border_color = get_original_border_color(&tab_action);
            let modified_border_color = get_modified_border_color(&tab_action);
            let is_high_contrast = {
                let theme_service = Rc::clone(&theme_service);
                observable_from_event(theme_service.on_did_color_theme_change(), move |_| {
                    matches!(theme_service.get_color_theme().theme_type(), ThemeType::HighContrastDark | ThemeType::HighContrastLight)
                })
            };
            let overlay_rect = get_editor_valid_overlay_rect(&editor);
            let (font_family, font_size, font_weight) = (editor.get_option(FONT_FAMILY), editor.get_option(FONT_SIZE), editor.get_option(FONT_WEIGHT));
            derived(move |reader| {
                let layout = layout.read(reader)?;

                let original_border_color = original_border_color.read(reader);
                let modified_border_color = modified_border_color.read(reader);

                let hc_border_color = if is_high_contrast.read(reader) {
                    Some(observe_color(CONTRAST_BORDER, &theme_service).read(reader))
                } else {
                    None
                };
                let hc_border = |fallback: WordReplacementColor| match &hc_border_color {
                    Some(color) => WordReplacementColor::Resolved(Rc::clone(color)),
                    None => fallback,
                };

                let editor_background = get_editor_background_color(view_data.editor_type);

                let primary_active_styles = WordReplacementStyles {
                    border_color: hc_border(WordReplacementColor::Theme(modified_border_color)),
                    background_color: WordReplacementColor::Theme(MODIFIED_CHANGED_TEXT_OVERLAY_COLOR),
                    color: None,
                    opacity: 1.0,
                };
                let secondary_active_styles = WordReplacementStyles {
                    border_color: hc_border(WordReplacementColor::Theme(INLINE_EDIT_INDICATOR_PRIMARY_BORDER)),
                    background_color: WordReplacementColor::Theme(INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND),
                    color: Some(WordReplacementColor::Theme(INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND)),
                    opacity: 1.0,
                };
                let passive_styles = WordReplacementStyles {
                    border_color: hc_border(WordReplacementColor::Resolved(Rc::new(
                        observe_color(EDITOR_HOVER_FOREGROUND, &theme_service).read(reader).transparent(0.2),
                    ))),
                    background_color: WordReplacementColor::Theme(editor_background),
                    color: None,
                    opacity: 0.7,
                };

                // `alternativeActionActive`: o `secondaryElementHovered` do original é fixo em falso.
                let alternative_action_active = layout.alternative_action.as_ref().is_some_and(|action| action.active);
                let secondary_styles = if alternative_action_active { secondary_active_styles } else { passive_styles };

                let replacement = WordReplacementPart {
                    width: layout.code_line.with_vertical_horizontal_margin(BORDER_WIDTH, 2.0 * BORDER_WIDTH).width(),
                    border_width: BORDER_WIDTH,
                    border_radius: INLINE_EDITS_BORDER_RADIUS,
                    styles: primary_active_styles,
                    font_family: font_family.read(reader),
                    font_size: *font_size.read(reader),
                    font_weight: font_weight.read(reader),
                    line: render_text_effect.read(reader),
                    line_height: layout.modified_line.height() + 2.0 * BORDER_WIDTH,
                };

                let rename = layout.alternative_action.clone().map(|action| WordReplacementRename {
                    action,
                    styles: secondary_styles,
                    border_width: BORDER_WIDTH,
                    border_radius: INLINE_EDITS_BORDER_RADIUS,
                });

                let arrow = WordReplacementArrow {
                    left: layout.modified_line.left - 16.0,
                    top: layout.modified_line.top + (((layout.line_height - 14.0 - 5.0) / 2.0) + 0.5).floor(),
                    paths: arrow_paths(),
                    stroke_color: EDITOR_HOVER_FOREGROUND,
                };

                Some(Rc::new(WordReplacementVisual {
                    overlay_rect: overlay_rect.read(reader),
                    lower_background_rect: layout.lower_background.with_margins(BORDER_WIDTH, 2.0 * BORDER_WIDTH, BORDER_WIDTH, 0.0),
                    widget_rect: layout.modified_line.with_vertical_horizontal_margin(BORDER_WIDTH, 2.0 * BORDER_WIDTH),
                    editor_background_color: editor_background,
                    replacement,
                    rename,
                    original_rect: layout.original_line.with_margin(BORDER_WIDTH),
                    original_border_color,
                    original_background_color: ORIGINAL_CHANGED_TEXT_OVERLAY_COLOR,
                    arrow,
                    layout,
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        // `isHovered`: sem a caixa do texto novo (a raiz sem layout) é falso.
        let is_hovered: IObservable<bool> = {
            let (layout, primary_hovered) = (Rc::clone(&layout), primary_hovered.to_observable());
            derived(move |reader| layout.read(reader).is_some() && primary_hovered.read(reader))
        };

        let position = const_observable(Some(OverlayWidgetPosition {
            preference: Some(OverlayWidgetPlacement::Coordinates(OverlayWidgetPositionCoordinates { top: 0.0, left: 0.0 })),
            stack_ordinal: None,
        }));
        base.register(editor.create_overlay_widget(NonOverflowOverlayWidget::new(position, const_observable(0.0))));

        Rc::new(Self { base, on_did_click, is_hovered, primary_hovered, shift_pressed, min_editor_scroll_height, visual })
    }

    /// O dado que o pintor nativo lê (`None` enquanto a posição não foi medida ou a altura da linha é zero).
    pub fn visual(&self) -> IObservable<Option<Rc<WordReplacementVisual>>> {
        Rc::clone(&self.visual)
    }

    /// O `createHoverTracker` da caixa do texto novo: o pintor avisa quando o ponteiro entra ou sai dela.
    pub fn notify_hover(&self, hovered: bool) {
        self.primary_hovered.set(hovered, None);
    }

    /// O `readModifierKeyStatus(...).shiftKey`: o pintor avisa o estado do shift enquanto o editor tem o foco.
    pub fn notify_shift_key(&self, pressed: bool) {
        self.shift_pressed.set(pressed, None);
    }

    /// O `onmousedown` do fundo inferior, do `widget` e da seta: `target` é o id mais próximo, subindo a árvore.
    pub fn notify_mouse_down(&self, event: MouseEvent, target: Option<WordReplacementTarget>) {
        let Some(target) = target else {
            return;
        };
        self.on_did_click.fire(InlineEditClickEvent::new(event, target == WordReplacementTarget::Rename));
    }
}

impl IInlineEditsView for InlineEditsWordReplacementView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn min_editor_scroll_height(&self) -> Option<IObservable<f64>> {
        Some(Rc::clone(&self.min_editor_scroll_height))
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for InlineEditsWordReplacementView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
