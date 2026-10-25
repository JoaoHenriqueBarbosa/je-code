//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsInsertionView.ts`:
//! `InlineEditsInsertionView`, a view de uma inserção de várias linhas (o texto novo aparece como texto fantasma, com uma
//! moldura que cobre só as linhas inseridas).
//!
//! Notas de tradução:
//! - o objeto anônimo do construtor (`{ lineNumber, startColumn, text, editorType }`) é o [`InsertionViewInput`];
//! - o `n.div` da raiz (a moldura de baixo, a moldura com a borda e o escudo que esconde o lado esquerdo) tem o mesmo formato
//!   da moldura da view de exclusão (separador, sobreposição e escudo, nesta ordem de pintura), então o dado que o pintor
//!   nativo lê em [`InlineEditsInsertionView::visual`] é o [`DeletionVisual`]: `originalUnderlayInsertion` é o `separator`,
//!   `originalOverlayInsertion` o `overlay` e `originalOverlayHiderInsertion` o `hider`. O `mapOutFalsy` do original só mantém
//!   a árvore DOM estável entre recálculos; aqui há um único dado calculado, então ele não aparece. A visibilidade
//!   (`display: block | none`) é [`InlineEditsInsertionView::is_displayed`];
//! - o `GhostTextView` é criado direto (o `@IInstantiationService` do original só existia para isso); o `@ILanguageService`
//!   serve ao `languageIdCodec` do ghost text e à tokenização que mede as linhas. O dado do widget do ghost text (o objeto
//!   `{ ghostText, handleInlineCompletionShown, warning }`) é o [`GhostTextWidgetData`]; o `handleInlineCompletionShown` é
//!   `None`, o `() => {}` do original;
//! - `textModel.getEOL()` é o [`Eol`](crate::model::Eol) do modelo, comparado por unidades UTF-16. O `text.split(eol)` do
//!   JavaScript é [`split_units`]; `startsWith(eol, i)` e `endsWith(eol, j)` são `starts_with` e `ends_with` sobre a fatia que
//!   começa em `i` e a que termina em `j`;
//! - `renderLines(new LineSource([tokens]), renderOptions, [], $('div'), true).minWidthInPx` (com `withSetWidth(false)` e
//!   `withScrollBeyondLastColumn(0)`) é a largura em caracteres de [`rendered_line_chars`] vezes
//!   `typicalHalfwidthCharacterWidth`, a mesma medida da view de substituição de linhas; as linhas são tokenizadas por
//!   [`tokenize_line_for_render`];
//! - o `observePosition(observableValue(this, new Position(...)), reader.store)` do `_overlayLayout` é o `observe_position` de
//!   um observável constante, com o widget de medição preso ao `store` do derivado;
//! - `intersectHorizontal(new OffsetRange(overlayHider.left, Number.MAX_SAFE_INTEGER))` é o [`intersect_horizontal_from`],
//!   montado direto com o [`Rect`]: o `OffsetRange` do crate é inteiro e arredondaria a borda;
//! - `isHovered` é o `isHovered` do `GhostTextView`; a view não define `minEditorScrollHeight`;
//! - `this._input.read(undefined)` do fundo do editor é a leitura do mesmo valor que o derivado já leu.

use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::js_string::trim;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, IReaderWithStore, ObservableExt, const_observable, derived};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::inline_completions::ghost_text::{GhostText, GhostTextOrReplacement, GhostTextPart};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::inline_completions::view::ghost_text_view::{GhostTextView, GhostTextViewOptions, GhostTextWidgetData, IGhostTextWidgetData};
use crate::inline_decorations::{InlineDecoration, InlineDecorationType};
use crate::language_service::ILanguageService;
use crate::line_range::LineRange;
use crate::observable_code_editor::observable_code_editor;
use crate::position::Position;
use crate::range::Range;

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{
    INLINE_EDITS_BORDER_RADIUS, MODIFIED_BACKGROUND_COLOR, get_editor_background_color, get_modified_border_color,
};
use super::super::utils::utils::{PrefixTrim, get_prefix_trim};
use super::NonOverflowOverlayWidget;
use super::inline_edits_deletion_view::{DeletionBox, DeletionVisual};
use super::inline_edits_line_replacement_view::{rendered_line_chars, tokenize_line_for_render};
use super::inline_edits_side_by_side_view::intersect_horizontal_from;

const BORDER_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH: f64 = 3.0;
const BORDER_RADIUS: f64 = INLINE_EDITS_BORDER_RADIUS;

/// O objeto que o construtor recebe: `{ lineNumber, startColumn, text, editorType }`.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertionViewInput {
    pub line_number: i32,
    pub start_column: i32,
    pub text: JsString,
    pub editor_type: InlineCompletionEditorType,
}

/// O `_state` do original: `{ lineNumber, column, text }`.
struct InsertionState {
    line_number: i32,
    column: i32,
    text: JsString,
}

/// O `_trimVertically` do original.
struct TrimVertically {
    top_offset: f64,
    content_height: f64,
    lines_top: usize,
    lines_bottom: usize,
}

/// O `_overlayLayout` do original.
struct OverlayLayout {
    overlay: Rect,
    starts_at_content_left: bool,
    content_left: f64,
    min_content_width_required: f64,
}

/// `text.split(separator)` do JavaScript sobre unidades UTF-16 (o separador nunca é vazio: é o fim de linha do modelo).
fn split_units(text: &[u16], separator: &[u16]) -> Vec<JsString> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + separator.len() <= text.len() {
        if text[i..].starts_with(separator) {
            parts.push(JsString::from(&text[start..i]));
            i += separator.len();
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(JsString::from(&text[start..]));
    parts
}

/// `InlineEditsInsertionView`.
pub struct InlineEditsInsertionView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    is_hovered: IObservable<bool>,
    display: IObservable<bool>,
    visual: IObservable<Option<Rc<DeletionVisual>>>,
    start_line_offset: IObservable<f64>,
    original_lines: IObservable<Option<LineRange>>,
}

impl InlineEditsInsertionView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        input: IObservable<Option<Rc<InsertionViewInput>>>,
        tab_action: IObservable<InlineEditTabAction>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(on_did_click.clone());

        let state: IObservable<Option<Rc<InsertionState>>> = {
            let (input, editor) = (Rc::clone(&input), Rc::clone(&editor));
            derived(move |reader| {
                let state = input.read(reader)?;

                let text_model = editor.get_model().expect("o editor tem modelo");
                let eol = text_model.get_eol();
                let eol = eol.as_units();
                let units = state.text.units();

                if state.start_column == 1
                    && state.line_number > 1
                    && text_model.get_line_length(state.line_number) != 0
                    && units.ends_with(eol)
                    && !units.starts_with(eol)
                {
                    let end_of_line_column = text_model.get_line_length(state.line_number - 1) as i32 + 1;
                    let mut text = JsString::from(eol);
                    text.push_units(&units[..units.len() - eol.len()]);
                    return Some(Rc::new(InsertionState { line_number: state.line_number - 1, column: end_of_line_column, text }));
                }

                Some(Rc::new(InsertionState { line_number: state.line_number, column: state.start_column, text: state.text.clone() }))
            })
        };

        let trim_vertically: IObservable<Rc<TrimVertically>> = {
            let (state, editor) = (Rc::clone(&state), Rc::clone(&editor));
            derived(move |reader| {
                let Some(state) = state.read(reader) else {
                    return Rc::new(TrimVertically { top_offset: 0.0, content_height: 0.0, lines_top: 0, lines_bottom: 0 });
                };

                let units = state.text.units();
                let line_height = editor.get_line_height_for_position(Position::new(state.line_number, 1));
                let eol = editor.get_model().expect("o editor tem modelo").get_eol();
                let eol = eol.as_units();
                let line_count = split_units(units, eol).len();

                // Count leading/trailing blank lines so the overlay can be trimmed to the actual inserted content.
                let mut lines_top = 0_usize;
                let mut lines_bottom = 0_usize;
                if !trim(units).is_empty() {
                    let mut i = 0;
                    while i < units.len() && units[i..].starts_with(eol) {
                        lines_top += 1;
                        i += eol.len();
                    }

                    let mut j = units.len();
                    while j > i && units[..j].ends_with(eol) {
                        lines_bottom += 1;
                        j -= eol.len();
                    }
                }

                Rc::new(TrimVertically {
                    top_offset: lines_top as f64 * line_height,
                    content_height: (line_count as f64 - lines_top as f64 - lines_bottom as f64) * line_height,
                    lines_top,
                    lines_bottom,
                })
            })
        };

        let max_prefix_trim: IObservable<Rc<PrefixTrim>> = {
            let (state, trim_vertically, editor) = (Rc::clone(&state), Rc::clone(&trim_vertically), Rc::clone(&editor));
            derived(move |reader| {
                let Some(state) = state.read(reader) else {
                    return Rc::new(PrefixTrim { prefix_trim: 0, prefix_left_offset: 0.0 });
                };

                let text_model = editor.get_model().expect("o editor tem modelo");
                let eol = text_model.get_eol();

                let trim_vertically = trim_vertically.read(reader);

                let lines = split_units(state.text.units(), eol.as_units());
                let end = lines.len().saturating_sub(trim_vertically.lines_bottom);
                let mut modified_lines: Vec<JsString> = lines.get(trim_vertically.lines_top..end).unwrap_or_default().to_vec();
                if trim_vertically.lines_top == 0
                    && let Some(first) = modified_lines.first_mut()
                {
                    let mut content = text_model.get_line_content(state.line_number);
                    content.push_units(first.units());
                    *first = content;
                }

                let original_range = LineRange::new(state.line_number, state.line_number + if trim_vertically.lines_top > 0 { 0 } else { 1 });

                Rc::new(get_prefix_trim(&[], original_range, &modified_lines, &editor, Some(reader)))
            })
        };

        let ghost_text: IObservable<Option<Rc<GhostText>>> = {
            let (state, max_prefix_trim, editor) = (Rc::clone(&state), Rc::clone(&max_prefix_trim), Rc::clone(&editor));
            derived(move |reader| {
                let state = state.read(reader);
                let prefix_trim = max_prefix_trim.read(reader);
                let state = state?;

                let eol = editor.get_model().expect("o editor tem modelo").get_eol();
                let modified_lines = split_units(state.text.units(), eol.as_units());

                let inline_decorations: Vec<InlineDecoration> = modified_lines
                    .iter()
                    .enumerate()
                    .map(|(i, line)| {
                        let line_number = i as i32 + 1;
                        InlineDecoration::new(
                            Range::new(line_number, if i == 0 { 1 } else { prefix_trim.prefix_trim + 1 }, line_number, line.len() as i32 + 1),
                            "modified-background",
                            InlineDecorationType::Regular,
                        )
                    })
                    .collect();

                Some(Rc::new(GhostText::new(
                    state.line_number,
                    vec![GhostTextPart::new(state.column, state.text.clone(), false, inline_decorations)],
                )))
            })
        };

        let ghost_text_view = GhostTextView::new(
            Rc::clone(&editor),
            {
                let ghost_text = Rc::clone(&ghost_text);
                derived(move |reader| -> Option<Rc<dyn IGhostTextWidgetData>> {
                    let ghost_text = ghost_text.read(reader)?;
                    Some(Rc::new(GhostTextWidgetData {
                        ghost_text: GhostTextOrReplacement::GhostText((*ghost_text).clone()),
                        warning: None,
                        // This is a no-op for the insertion view, as it is handled by the InlineEditsView.
                        on_inline_completion_shown: None,
                    }))
                })
            },
            GhostTextViewOptions {
                extra_classes: vec!["inline-edit".to_owned()],
                is_clickable: true,
                should_keep_cursor_stable: true,
                ..GhostTextViewOptions::default()
            },
            language_service.language_id_codec(),
        );
        base.register(ghost_text_view.clone());

        let is_hovered = Rc::clone(&ghost_text_view.is_hovered);

        base.register(ghost_text_view.on_did_click().listen({
            let on_did_click = Rc::clone(&on_did_click);
            move |e| on_did_click.fire(InlineEditClickEvent::new(e.clone(), false))
        }));

        let display: IObservable<bool> = state.map(|state, _| state.is_some());

        let editor_max_content_width_in_range: IObservable<f64> = {
            let (state, editor, editor_obs, language_service) =
                (Rc::clone(&state), Rc::clone(&editor), Rc::clone(&editor_obs), Rc::clone(&language_service));
            let font_info = editor_obs.get_option(FONT_INFO);
            derived(move |reader| {
                let Some(state) = state.read(reader) else {
                    return 0.0;
                };
                editor_obs.version_id.read(reader);
                let text_model = editor.get_model().expect("o editor tem modelo");
                let eol = text_model.get_eol();
                let eol = eol.as_units();

                let mut text = if state.text.units().starts_with(eol) {
                    JsString::new()
                } else {
                    text_model.get_value_in_range(&Range::new(state.line_number, 1, state.line_number, state.column))
                };
                text.push_units(state.text.units());
                text.push_units(
                    text_model
                        .get_value_in_range(&Range::new(state.line_number, state.column, state.line_number, text_model.get_line_length(state.line_number) as i32 + 1))
                        .units(),
                );
                let lines = split_units(text.units(), eol);

                // update when font info changes
                let typical_halfwidth_character_width = font_info.read(reader).typical_halfwidth_character_width;
                let language_id_codec = language_service.language_id_codec();
                let line_widths = lines.iter().map(|line| {
                    let tokens = tokenize_line_for_render(&text_model, state.line_number, line, &language_id_codec);
                    rendered_line_chars(&editor, &tokens, &[]) * typical_halfwidth_character_width
                });

                // Take the max value that we observed.
                // Reset when either the edit changes or the editor text version.
                line_widths.fold(f64::NEG_INFINITY, f64::max)
            })
        };

        let start_line_offset: IObservable<f64> = trim_vertically.map(|v, _| v.top_offset);
        let original_lines: IObservable<Option<LineRange>> = {
            let editor = Rc::clone(&editor);
            state.map(move |state, _| {
                state.map(|state| {
                    let line_count = CursorSimpleModel::get_line_count(&*editor.get_model().expect("o editor tem modelo"));
                    LineRange::new(state.line_number, (state.line_number + 2).min(line_count + 1))
                })
            })
        };

        let overlay_layout: IObservable<Option<Rc<OverlayLayout>>> = {
            let (ghost_text, state, editor, editor_obs) = (Rc::clone(&ghost_text), Rc::clone(&state), Rc::clone(&editor), Rc::clone(&editor_obs));
            let (max_prefix_trim, trim_vertically) = (Rc::clone(&max_prefix_trim), Rc::clone(&trim_vertically));
            derived(move |reader| {
                ghost_text.read(reader);
                let state = state.read(reader)?;

                // Update the overlay when the position changes
                editor_obs
                    .observe_position(&const_observable(Some(Position::new(state.line_number, state.column))), &reader.store())
                    .read(reader);

                let editor_layout = editor_obs.layout_info.read(reader);
                let horizontal_scroll_offset = editor_obs.scroll_left.read(reader);
                let vertical_scrollbar_width = editor_obs.layout_info_vertical_scrollbar_width.read(reader);

                let right = editor_layout.content_left + editor_max_content_width_in_range.read(reader) - horizontal_scroll_offset;
                let prefix_left_offset = max_prefix_trim.read(reader).prefix_left_offset;
                let left = editor_layout.content_left + prefix_left_offset - horizontal_scroll_offset;
                if right <= left {
                    return None;
                }

                let trim = trim_vertically.read(reader);

                let scroll_top = editor_obs.scroll_top.read(reader);
                // Derive the overlay height synchronously from the model (via _trimVertically) rather than the
                // asynchronously measured ghost text view zone height, which is transiently just a single line while
                // the view zone is (re)created. Because it uses the same line height and line accounting as the trims,
                // top/height/bottom stay consistent and height is always positive: leading and trailing blank lines
                // can never cover every inserted line.
                let top = editor.get_top_for_line_number(state.line_number, None) - scroll_top + trim.top_offset;
                let bottom = top + trim.content_height;

                let overlay = Rect::new(left, top, right, bottom);

                Some(Rc::new(OverlayLayout {
                    overlay,
                    starts_at_content_left: prefix_left_offset == 0.0,
                    content_left: editor_layout.content_left,
                    min_content_width_required: prefix_left_offset + overlay.width() + vertical_scrollbar_width,
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(
            const_observable(None),
            {
                let overlay_layout = Rc::clone(&overlay_layout);
                derived(move |reader| overlay_layout.read(reader).map_or(0.0, |info| info.min_content_width_required))
            },
        )));

        let modified_border_color = get_modified_border_color(&tab_action);
        let visual: IObservable<Option<Rc<DeletionVisual>>> = {
            let overlay_layout = Rc::clone(&overlay_layout);
            derived(move |reader| {
                let layout = overlay_layout.read(reader)?;

                // Create an overlay which hides the left hand side of the original overlay when it overflows to the left
                // such that there is a smooth transition at the edge of content left
                let overlay_hider = Rect::from_left_top_right_bottom(
                    layout.content_left - BORDER_RADIUS - BORDER_WIDTH,
                    layout.overlay.top,
                    layout.content_left,
                    layout.overlay.bottom,
                );

                let editor_type = input.read(reader).map_or(InlineCompletionEditorType::TextEditor, |input| input.editor_type);
                let separator_width =
                    if editor_type == InlineCompletionEditorType::DiffEditor { WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH } else { WIDGET_SEPARATOR_WIDTH };
                let overlay_rect = intersect_horizontal_from(
                    &layout.overlay.with_margins(0.0, BORDER_WIDTH, BORDER_WIDTH, if layout.starts_at_content_left { 0.0 } else { BORDER_WIDTH }),
                    overlay_hider.left,
                );
                let underlay_rect = overlay_rect.with_vertical_horizontal_margin(separator_width, separator_width);

                let editor_background = get_editor_background_color(editor_type);
                Some(Rc::new(DeletionVisual {
                    separator: DeletionBox {
                        rect: underlay_rect,
                        border_radius: BORDER_RADIUS,
                        border_width: BORDER_WIDTH + separator_width,
                        border_color: Some(editor_background),
                        background_color: None,
                    },
                    overlay: DeletionBox {
                        rect: overlay_rect,
                        border_radius: BORDER_RADIUS,
                        border_width: BORDER_WIDTH,
                        border_color: Some(modified_border_color.read(reader)),
                        background_color: Some(MODIFIED_BACKGROUND_COLOR),
                    },
                    hider: DeletionBox {
                        rect: overlay_hider,
                        border_radius: 0.0,
                        border_width: 0.0,
                        border_color: None,
                        background_color: Some(editor_background),
                    },
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        Rc::new(Self { base, on_did_click, is_hovered, display, visual, start_line_offset, original_lines })
    }

    /// `_display`: a view só aparece com o estado presente (`display: block | none`).
    pub fn is_displayed(&self) -> IObservable<bool> {
        Rc::clone(&self.display)
    }

    /// O dado que o pintor nativo lê (`None` sem layout): o separador, a moldura e o escudo, nesta ordem de pintura.
    pub fn visual(&self) -> IObservable<Option<Rc<DeletionVisual>>> {
        Rc::clone(&self.visual)
    }

    /// `startLineOffset`: o `topOffset` do corte vertical.
    pub fn start_line_offset(&self) -> IObservable<f64> {
        Rc::clone(&self.start_line_offset)
    }

    /// `originalLines`: o intervalo de linhas originais que a inserção cobre (`None` sem inserção).
    pub fn original_lines(&self) -> IObservable<Option<LineRange>> {
        Rc::clone(&self.original_lines)
    }
}

impl IInlineEditsView for InlineEditsInsertionView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for InlineEditsInsertionView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
