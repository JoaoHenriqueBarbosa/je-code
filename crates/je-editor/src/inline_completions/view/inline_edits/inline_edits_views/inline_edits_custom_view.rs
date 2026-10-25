//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsCustomView.ts`:
//! `InlineEditsCustomView`, a caixa com o texto da dica (`InlineSuggestHint`) no fim da linha, abaixo ou acima dela.
//!
//! Notas de tradução:
//! - o `n.div` do contêiner (com o `collapsedView` dentro) é o [`CustomViewVisual`], lido pelo pintor nativo em
//!   [`InlineEditsCustomView::visual`]: o retângulo (já com a margem horizontal do `getRendering`), a linha tokenizada,
//!   a borda e o fundo. O contêiner de `overflow: visible` na origem não tem medida própria. O `renderLines` com
//!   `RenderOptions.fromEditor(editor).withSetWidth(false).withScrollBeyondLastColumn(0)` e o `line.style.width =
//!   minWidthInPx` são de quem pinta: ele renderiza a [`CustomViewVisual::line`] com as opções do editor
//!   ([`RENDER_SET_WIDTH`], [`RENDER_SCROLL_BEYOND_LAST_COLUMN`]) e dá à linha a largura que a renderização mede. A caixa
//!   é `display: flex`, centrada, sem quebra de linha e com `overflow: hidden`;
//! - `overlay.isHovered` e o `onclick` são eventos do nó DOM; o pintor os entrega por [`InlineEditsCustomView::notify_hover`]
//!   e [`InlineEditsCustomView::notify_click`] (o `onmousedown` com `preventDefault`, que impede o editor de perder o foco,
//!   também é dele). O `isHovered` da view continua sendo o `autorun` do original: falso sem renderização, senão o valor
//!   do pintor;
//! - `ICodeEditor` e o `observableCodeEditor(editor)` do construtor são o `Rc<CodeEditorWidget>` e o
//!   [`observable_code_editor`]; `@ILanguageService` é o `Rc<dyn ILanguageService>`, que só serve ao `languageIdCodec`.
//!   `@IThemeService` é o `Rc<dyn IThemeService>`. `getModel()!` é um `expect`: sem modelo não há dica a mostrar, e o
//!   original também lançaria;
//! - `InlineEditClickEvent.create(e)` é [`InlineEditClickEvent::new`] (ver `inline_edits_view_interface`);
//! - `getState` roda uma vez por dica, dentro do `map`, e a checagem `fitsInsideViewport` roda uma só vez, ao montar, como
//!   no original; a posição `end | below | above` é o [`CustomViewPosition`];
//! - o `recomputeInitiallyAndOnChange(reader.store)` da altura de linha usa o `store()` do leitor do derivado, que é
//!   esvaziado a cada recálculo, como `reader.store` do original;
//! - `InlineSuggestHint.content.length` é o comprimento UTF-16 (`JsString::len`).

use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, IReaderWithStore, Observable, ObservableExt, SettableExt, autorun, const_observable, derived, derived_observable_with_cache,
    observable_value,
};
use je_theme::color::Color;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{MouseEvent, OverlayWidgetPosition};
use crate::inline_completions::inline_suggestion_item::InlineSuggestHint;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::language_service::ILanguageService;
use crate::languages::InlineCompletionHintStyle;
use crate::line_range::LineRange;
use crate::line_tokens::{LineTokens, TokenArray};
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable, observable_code_editor};
use crate::services::theme_service::IThemeService;

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{
    INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND, INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND,
    INLINE_EDITS_BORDER_RADIUS, get_editor_background_color, get_editor_blended_color,
};
use super::super::utils::utils::{get_content_render_width, max_content_width_in_range};
use super::NonOverflowOverlayWidget;

const MIN_END_OF_LINE_PADDING: f64 = 14.0;
const PADDING_VERTICALLY: f64 = 0.0;
const PADDING_HORIZONTALLY: f64 = 4.0;
const HORIZONTAL_OFFSET_WHEN_ABOVE_BELOW: f64 = 4.0;
const VERTICAL_OFFSET_WHEN_ABOVE_BELOW: f64 = 2.0;
// !! minEndOfLinePadding should always be larger than paddingHorizontally + horizontalOffsetWhenAboveBelow

/// A largura da borda da caixa (`1px solid`).
pub const CUSTOM_VIEW_BORDER_WIDTH: f64 = 1.0;
/// O `borderRadius` da caixa.
pub const CUSTOM_VIEW_BORDER_RADIUS: f64 = INLINE_EDITS_BORDER_RADIUS;
/// `withSetWidth(false)` do `RenderOptions`.
pub const RENDER_SET_WIDTH: bool = false;
/// `withScrollBeyondLastColumn(0)` do `RenderOptions`.
pub const RENDER_SCROLL_BEYOND_LAST_COLUMN: i32 = 0;

/// Onde a caixa fica em relação à linha da dica (`'end' | 'below' | 'above'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomViewPosition {
    End,
    Below,
    Above,
}

/// O que o `contentState` do `getState` mede.
struct ContentState {
    line_width: f64,
    line_width_below: Option<f64>,
    line_width_above: Option<f64>,
    start_content_left_offset: f64,
    end_content_left_offset: f64,
}

/// O que o `getState` devolve: `{ rect, label, kind }`.
struct CustomViewState {
    rect: IObservable<Rect>,
    label: String,
    kind: InlineCompletionHintStyle,
}

/// O que o `getRendering` monta uma vez por dica: o retângulo da caixa e a linha tokenizada.
struct CustomViewRendering {
    rect: IObservable<Rect>,
    line: LineTokens,
}

/// Os `styles` do construtor: `{ border, background }`.
struct CustomViewStyles {
    border: Rc<Color>,
    /// Identificador do tema (`getEditorBackgroundColor`).
    background: &'static str,
}

/// O que o pintor nativo lê da view.
#[derive(Clone)]
pub struct CustomViewVisual {
    /// A caixa (`position: absolute`, `box-sizing: border-box`, `cursor: pointer`).
    pub rect: Rect,
    /// A linha com o texto da dica, tokenizada (com a sintaxe só no estilo `Code`).
    pub line: LineTokens,
    /// A cor da borda (`1px solid`), opaca sobre o fundo do editor.
    pub border_color: Rc<Color>,
    /// A cor do fundo, como identificador do tema.
    pub background_color: &'static str,
}

/// `fitsInsideViewport(range, displayLabel, reader)`: só é checado ao montar a view.
fn fits_inside_viewport(editor: &Rc<CodeEditorWidget>, editor_obs: &Rc<ObservableCodeEditor>, range: LineRange, display_label: &str) -> bool {
    let editor_width = editor_obs.layout_info_width.get();
    let editor_content_left = editor_obs.layout_info_content_left.get();
    let editor_vertical_scrollbar = editor.get_layout_info().vertical_scrollbar_width;
    let minimap = editor_obs.layout_info_minimap.get();
    let minimap_width = if minimap.minimap_left != 0.0 { minimap.minimap_width } else { 0.0 };

    let max_original_content = max_content_width_in_range(editor_obs, range, None);
    let model = editor.get_model().expect("a dica só é mostrada com modelo");
    let max_modified_content = get_content_render_width(&JsString::from(display_label), editor, &model);
    let padding = PADDING_HORIZONTALLY + MIN_END_OF_LINE_PADDING;

    max_original_content + max_modified_content + padding < editor_width - editor_content_left - editor_vertical_scrollbar - minimap_width
}

/// `getState(displayLocation)`.
fn get_state(editor: &Rc<CodeEditorWidget>, editor_obs: &Rc<ObservableCodeEditor>, display_location: &InlineSuggestHint) -> Rc<CustomViewState> {
    let range = display_location.range;
    let (start_line_number, end_line_number) = (range.start_line_number, range.end_line_number);
    let (start_column, end_column) = (range.start_column, range.end_column);

    let content_state: IObservable<Rc<ContentState>> = {
        let (editor, editor_obs) = (Rc::clone(editor), Rc::clone(editor_obs));
        derived(move |reader| {
            let line_count = editor.get_model().map(|model| CursorSimpleModel::get_line_count(&*model)).unwrap_or(0);

            let line_width = max_content_width_in_range(&editor_obs, LineRange::new(start_line_number, start_line_number + 1), Some(reader));
            let line_width_below = (start_line_number + 1 <= line_count)
                .then(|| max_content_width_in_range(&editor_obs, LineRange::new(start_line_number + 1, start_line_number + 2), Some(reader)));
            let line_width_above = (start_line_number - 1 >= 1)
                .then(|| max_content_width_in_range(&editor_obs, LineRange::new(start_line_number - 1, start_line_number), Some(reader)));
            let start_content_left_offset = editor.get_offset_for_column(start_line_number, start_column);
            let end_content_left_offset = editor.get_offset_for_column(end_line_number, end_column);

            Rc::new(ContentState { line_width, line_width_below, line_width_above, start_content_left_offset, end_content_left_offset })
        })
    };

    // only check viewport once in the beginning when rendering the view
    let fits_inside_viewport =
        fits_inside_viewport(editor, editor_obs, LineRange::new(start_line_number, end_line_number + 1), &display_location.content);

    let label_length = JsString::from(display_location.content.as_str()).len() as f64;
    let rect: IObservable<Rect> = {
        let (editor, editor_obs) = (Rc::clone(editor), Rc::clone(editor_obs));
        let font_info = editor_obs.get_option(FONT_INFO);
        derived(move |reader| {
            let w = font_info.read(reader).typical_halfwidth_character_width;

            let content = content_state.read(reader);

            let content_left = editor_obs.layout_info_content_left.read(reader);
            let line_height = editor_obs
                .observe_line_height_for_line(ValueOrObservable::Value(start_line_number))
                .recompute_initially_and_on_change(&reader.store(), None)
                .read(reader);
            let scroll_top = editor_obs.scroll_top.read(reader);
            let scroll_left = editor_obs.scroll_left.read(reader);

            let fits_below_or_above = |line_width: Option<f64>| {
                line_width.is_some_and(|width| {
                    width + MIN_END_OF_LINE_PADDING - HORIZONTAL_OFFSET_WHEN_ABOVE_BELOW - PADDING_HORIZONTALLY < content.start_content_left_offset
                })
            };
            let position = if start_line_number == end_line_number && content.end_content_left_offset + 5.0 * w >= content.line_width && fits_inside_viewport
            {
                CustomViewPosition::End // Render at the end of the line if the range ends almost at the end of the line
            } else if fits_below_or_above(content.line_width_below) {
                CustomViewPosition::Below // Render Below if possible
            } else if fits_below_or_above(content.line_width_above) {
                CustomViewPosition::Above // Render Above if possible
            } else {
                CustomViewPosition::End // Render at the end of the line otherwise
            };

            let (top_of_line, content_start_offset, delta_x, delta_y) = match position {
                CustomViewPosition::End => (
                    editor.get_top_for_line_number(start_line_number, None),
                    content.line_width,
                    PADDING_HORIZONTALLY + MIN_END_OF_LINE_PADDING,
                    0.0,
                ),
                CustomViewPosition::Below => (
                    editor.get_top_for_line_number(start_line_number + 1, None),
                    content.start_content_left_offset,
                    PADDING_HORIZONTALLY + HORIZONTAL_OFFSET_WHEN_ABOVE_BELOW,
                    PADDING_VERTICALLY + VERTICAL_OFFSET_WHEN_ABOVE_BELOW,
                ),
                CustomViewPosition::Above => (
                    editor.get_top_for_line_number(start_line_number - 1, None),
                    content.start_content_left_offset,
                    PADDING_HORIZONTALLY + HORIZONTAL_OFFSET_WHEN_ABOVE_BELOW,
                    -PADDING_VERTICALLY + VERTICAL_OFFSET_WHEN_ABOVE_BELOW,
                ),
            };

            let text_rect = Rect::from_left_top_width_height(
                content_left + content_start_offset - scroll_left,
                top_of_line - scroll_top,
                w * label_length,
                line_height,
            );

            text_rect.with_vertical_horizontal_margin(PADDING_VERTICALLY, PADDING_HORIZONTALLY).translate_x(delta_x).translate_y(delta_y)
        })
    };

    Rc::new(CustomViewState { rect, label: display_location.content.clone(), kind: display_location.style })
}

/// `getRendering(state, styles)`: a linha tokenizada e o retângulo com a margem horizontal.
fn get_rendering(editor: &Rc<CodeEditorWidget>, language_service: &Rc<dyn ILanguageService>, state: &CustomViewState) -> Rc<CustomViewRendering> {
    let label = JsString::from(state.label.as_str());
    let language_id_codec = language_service.language_id_codec();
    let model = editor.get_model().expect("a dica só é mostrada com modelo");
    let tokenized = model.tokenization().tokenize_lines_at(1, std::slice::from_ref(&label)).and_then(|lines| lines.into_iter().next());
    let line = match tokenized {
        Some(t) if state.kind == InlineCompletionHintStyle::Code => TokenArray::from_line_tokens(&t).to_line_tokens(&label, language_id_codec),
        _ => LineTokens::create_empty(label, language_id_codec),
    };

    let rect = state.rect.map(|r, _| r.with_vertical_horizontal_margin(0.0, PADDING_HORIZONTALLY));
    Rc::new(CustomViewRendering { rect, line })
}

/// `InlineEditsCustomView`.
pub struct InlineEditsCustomView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    is_hovered: IObservable<bool>,
    overlay_hovered: Rc<ObservableValue<bool, ()>>,
    min_editor_scroll_height: IObservable<f64>,
    visual: IObservable<Option<Rc<CustomViewVisual>>>,
}

impl InlineEditsCustomView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        display_location: IObservable<Option<InlineSuggestHint>>,
        tab_action: IObservable<InlineEditTabAction>,
        editor_type: IObservable<InlineCompletionEditorType>,
        theme_service: Rc<dyn IThemeService>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(Rc::clone(&on_did_click));

        let is_hovered_value = observable_value::<bool, ()>("isHovered", false);
        let overlay_hovered = observable_value::<bool, ()>("overlayHovered", false);

        let editor_obs = observable_code_editor(&editor);

        let styles: IObservable<Rc<CustomViewStyles>> = derived_styles(&tab_action, &editor_type, &theme_service);

        let state: IObservable<Option<Rc<CustomViewState>>> = display_location.map({
            let (editor, editor_obs) = (Rc::clone(&editor), Rc::clone(&editor_obs));
            move |display_location, _| display_location.map(|dl| get_state(&editor, &editor_obs, &dl))
        });

        let view: IObservable<Option<Rc<CustomViewRendering>>> = state.map({
            let (editor, language_service) = (Rc::clone(&editor), Rc::clone(&language_service));
            move |state, _| state.map(|s| get_rendering(&editor, &language_service, &s))
        });

        let min_editor_scroll_height: IObservable<f64> = {
            let (state, editor) = (Rc::clone(&state), Rc::clone(&editor));
            derived(move |reader| {
                let Some(s) = state.read(reader) else {
                    return 0.0;
                };
                s.rect.read(reader).bottom + editor.get_scroll_top()
            })
        };

        // `overlay`
        let visual: IObservable<Option<Rc<CustomViewVisual>>> = {
            let view = Rc::clone(&view);
            derived(move |reader| {
                let rendering = view.read(reader)?;
                let styles = styles.read(reader);
                Some(Rc::new(CustomViewVisual {
                    rect: rendering.rect.read(reader),
                    line: rendering.line.clone(),
                    border_color: Rc::clone(&styles.border),
                    background_color: styles.background,
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(
            const_observable::<Option<OverlayWidgetPosition>>(None),
            {
                let (state, editor_obs) = (Rc::clone(&state), Rc::clone(&editor_obs));
                derived_observable_with_cache::<f64>(None, move |reader, prev| {
                    let Some(s) = state.read(reader) else {
                        return prev.copied().unwrap_or(0.0);
                    };

                    let current = s.rect.map(|rect, _| rect.right).read(reader)
                        + editor_obs.layout_info_vertical_scrollbar_width.read(reader)
                        + PADDING_HORIZONTALLY
                        - editor_obs.layout_info_content_left.read(reader);
                    current.max(prev.copied().unwrap_or(0.0)) // will run into infinite loop otherwise TODO: fix this
                })
                .recompute_initially_and_on_change(base.store(), None)
            },
        )));

        base.register(autorun({
            let (view, overlay_hovered, is_hovered_value) = (Rc::clone(&view), Rc::clone(&overlay_hovered), Rc::clone(&is_hovered_value));
            move |reader| {
                if view.read(reader).is_none() {
                    is_hovered_value.set(false, None);
                    return;
                }
                is_hovered_value.set(overlay_hovered.to_observable().read(reader), None);
            }
        }));

        Rc::new(Self { base, on_did_click, is_hovered: is_hovered_value.to_observable(), overlay_hovered, min_editor_scroll_height, visual })
    }

    /// O dado que o pintor nativo lê (`None` sem dica).
    pub fn visual(&self) -> IObservable<Option<Rc<CustomViewVisual>>> {
        Rc::clone(&self.visual)
    }

    /// O `overlay.isHovered`: o pintor avisa quando o ponteiro entra ou sai da caixa.
    pub fn notify_hover(&self, hovered: bool) {
        self.overlay_hovered.set(hovered, None);
    }

    /// O `onclick` da caixa: o pintor entrega o evento.
    pub fn notify_click(&self, event: MouseEvent) {
        self.on_did_click.fire(InlineEditClickEvent::new(event, false));
    }
}

/// O `styles` do construtor: a cor de borda por ação da tecla e o fundo por tipo de editor.
fn derived_styles(
    tab_action: &IObservable<InlineEditTabAction>,
    editor_type: &IObservable<InlineCompletionEditorType>,
    theme_service: &Rc<dyn IThemeService>,
) -> IObservable<Rc<CustomViewStyles>> {
    let (editor_type, theme_service) = (Rc::clone(editor_type), Rc::clone(theme_service));
    tab_action.map(move |tab_action, reader| {
        let border = match tab_action {
            InlineEditTabAction::Inactive => INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND,
            InlineEditTabAction::Jump => INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND,
            InlineEditTabAction::Accept => INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND,
        };
        Rc::new(CustomViewStyles {
            border: get_editor_blended_color(ValueOrObservable::Value(border), &theme_service).read(reader),
            background: get_editor_background_color(editor_type.read(reader)),
        })
    })
}

impl IInlineEditsView for InlineEditsCustomView {
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

impl IDisposable for InlineEditsCustomView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
