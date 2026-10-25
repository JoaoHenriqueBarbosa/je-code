//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsDeletionView.ts`:
//! `InlineEditsDeletionView`, a view de uma exclusão de linhas inteiras (uma moldura sobre as linhas originais).
//!
//! Notas de tradução:
//! - o `_uiState` anônimo do construtor (`{ originalRange, deletions, editorType }`) é o [`DeletionViewUiState`];
//! - o `n.div` com a moldura (`originalSeparatorDeletion`, `originalOverlayDeletion` e `originalOverlayHiderDeletion`) é o
//!   [`DeletionVisual`] com as três caixas na ordem de pintura do original, lido pelo pintor nativo em
//!   [`InlineEditsDeletionView::visual`]. O `mapOutFalsy` do original só mantém a árvore DOM estável entre
//!   recálculos; aqui há um único dado calculado, então ele não aparece. A visibilidade (`display: block | none`) é
//!   [`InlineEditsDeletionView::is_displayed`], e a moldura só existe com o layout presente;
//! - `ICodeEditor` e o `observableCodeEditor(editor)` do construtor são o `Rc<CodeEditorWidget>` e o
//!   [`observable_code_editor`]. A cor de borda e os fundos são identificadores do tema (`asCssVariable` do original);
//! - `_originalDisplayRange` é um observável de `Rc<LineRange>`: o `LineRange` do original é um objeto novo a cada
//!   estado, e a igualdade estrita dele é a identidade do `Rc`, como no original;
//! - `_editorMaxContentWidthInRange` (`derived(...).map((v, r) => v.read(r))`) é o mesmo `derived` seguido de um `map`
//!   que lê o observável interno, que guarda a maior largura observada e recomeça quando a edição ou a versão do texto
//!   mudam;
//! - `_editorLayoutInfo` é o [`DeletionLayoutInfo`] dentro de um `Rc` (a igualdade estrita é a identidade);
//! - `getPrefixTrim` recebe o `Rc<CodeEditorWidget>` e devolve um [`PrefixTrim`], embrulhado em `Rc` no `_maxPrefixTrim`;
//! - o `intersectHorizontal(new OffsetRange(overlayHider.left, Number.MAX_SAFE_INTEGER))` é montado direto com o
//!   [`Rect`]: o `OffsetRange` do crate é inteiro e arredondaria a borda, e o limite da direita nunca restringe.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, ObservableExt, const_observable, derived, derived_observable_with_cache};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::core_2d::rect::Rect;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::line_range::LineRange;
use crate::observable_code_editor::observable_code_editor;
use crate::position::Position;
use crate::range::Range;

use super::super::inline_edit_with_changes::InlineEditWithChanges;
use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{INLINE_EDITS_BORDER_RADIUS, ORIGINAL_BACKGROUND_COLOR, get_editor_background_color, get_original_border_color};
use super::super::utils::utils::{PrefixTrim, get_prefix_trim, max_content_width_in_range};
use super::NonOverflowOverlayWidget;

const HORIZONTAL_PADDING: f64 = 0.0;
const VERTICAL_PADDING: f64 = 0.0;
const BORDER_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH: f64 = 3.0;
const BORDER_RADIUS: f64 = INLINE_EDITS_BORDER_RADIUS;

/// O `_uiState` do construtor.
#[derive(Clone, Debug, PartialEq)]
pub struct DeletionViewUiState {
    pub original_range: LineRange,
    pub deletions: Vec<Range>,
    pub editor_type: InlineCompletionEditorType,
}

/// O `_editorLayoutInfo` do original: `{ codeRect, contentLeft }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeletionLayoutInfo {
    pub code_rect: Rect,
    pub content_left: f64,
}

/// Uma das caixas da moldura: o `div` com `position: absolute` no retângulo, `box-sizing: border-box` e os estilos
/// que o original dá a cada uma.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeletionBox {
    pub rect: Rect,
    /// O `border-radius` em pixels (zero na caixa que esconde o lado esquerdo).
    pub border_radius: f64,
    /// A largura da borda em pixels (zero sem borda).
    pub border_width: f64,
    /// A cor da borda, como identificador do tema.
    pub border_color: Option<&'static str>,
    /// A cor do fundo, como identificador do tema.
    pub background_color: Option<&'static str>,
}

/// O que o pintor nativo lê da view: as três caixas, na ordem de pintura. O contêiner tem `pointer-events: none`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeletionVisual {
    /// `originalSeparatorDeletion`: a margem na cor do fundo do editor que separa a moldura do texto vizinho.
    pub separator: DeletionBox,
    /// `originalOverlayDeletion`: a moldura sobre as linhas originais.
    pub overlay: DeletionBox,
    /// `originalOverlayHiderDeletion`: esconde o lado esquerdo da moldura quando ela passa do `contentLeft`, para a
    /// borda do conteúdo ficar suave.
    pub hider: DeletionBox,
}

/// `InlineEditsDeletionView`.
pub struct InlineEditsDeletionView {
    base: Rc<Disposable>,
    display: IObservable<bool>,
    visual: IObservable<Option<Rc<DeletionVisual>>>,
    is_hovered: IObservable<bool>,
}

impl InlineEditsDeletionView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        edit: IObservable<Option<Rc<InlineEditWithChanges>>>,
        ui_state: IObservable<Option<Rc<DeletionViewUiState>>>,
        tab_action: IObservable<InlineEditTabAction>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);

        let original_start_position: IObservable<Option<Position>> = {
            let edit = Rc::clone(&edit);
            derived(move |reader| {
                let inline_edit = edit.read(reader)?;
                Some(Position::new(inline_edit.original_line_range().start_line_number, 1))
            })
        };

        let original_end_position: IObservable<Option<Position>> = {
            let edit = Rc::clone(&edit);
            derived(move |reader| {
                let inline_edit = edit.read(reader)?;
                Some(Position::new(inline_edit.original_line_range().end_line_number_exclusive, 1))
            })
        };

        let original_display_range: IObservable<Option<Rc<LineRange>>> = ui_state.map(|s, _| s.map(|s| Rc::new(s.original_range)));
        let original_vertical_start_position: IObservable<Option<f64>> =
            editor_obs.observe_position(&original_start_position, base.store()).map(|p, _| p.map(|p| p.y));
        let original_vertical_end_position: IObservable<Option<f64>> =
            editor_obs.observe_position(&original_end_position, base.store()).map(|p, _| p.map(|p| p.y));

        let editor_max_content_width_in_range: IObservable<f64> = {
            let editor_obs = Rc::clone(&editor_obs);
            derived(move |reader| -> IObservable<f64> {
                let Some(original_display_range) = original_display_range.read(reader) else {
                    return const_observable(0.0);
                };
                editor_obs.version_id.read(reader);

                // Take the max value that we observed.
                // Reset when either the edit changes or the editor text version.
                let editor_obs = Rc::clone(&editor_obs);
                derived_observable_with_cache::<f64>(None, move |reader, last_value| {
                    let max_width = max_content_width_in_range(&editor_obs, *original_display_range, Some(reader));
                    max_width.max(last_value.copied().unwrap_or(0.0))
                })
            })
            .map(|v, r| v.read(r))
        };

        let max_prefix_trim: IObservable<Rc<PrefixTrim>> = {
            let (ui_state, editor) = (Rc::clone(&ui_state), Rc::clone(&editor));
            derived(move |reader| {
                let Some(state) = ui_state.read(reader) else {
                    return Rc::new(PrefixTrim { prefix_trim: 0, prefix_left_offset: 0.0 });
                };
                Rc::new(get_prefix_trim(&state.deletions, state.original_range, &[], &editor, Some(reader)))
            })
        };

        let editor_layout_info: IObservable<Option<Rc<DeletionLayoutInfo>>> = {
            let (edit, ui_state, editor, editor_obs) = (Rc::clone(&edit), Rc::clone(&ui_state), Rc::clone(&editor), Rc::clone(&editor_obs));
            let font_info = editor_obs.get_option(FONT_INFO);
            derived(move |reader| {
                let inline_edit = edit.read(reader)?;
                ui_state.read(reader)?;

                let editor_layout = editor_obs.layout_info.read(reader);
                let horizontal_scroll_offset = editor_obs.scroll_left.read(reader);
                let w = font_info.read(reader).typical_halfwidth_character_width;

                let right = editor_layout.content_left + editor_max_content_width_in_range.read(reader).max(w) - horizontal_scroll_offset;

                let range = inline_edit.original_line_range();
                let selection_top = match original_vertical_start_position.read(reader) {
                    Some(top) => top,
                    None => editor.get_top_for_line_number(range.start_line_number, None) - editor_obs.scroll_top.read(reader),
                };
                let selection_bottom = match original_vertical_end_position.read(reader) {
                    Some(bottom) => bottom,
                    None => editor.get_top_for_line_number(range.end_line_number_exclusive, None) - editor_obs.scroll_top.read(reader),
                };

                let left = editor_layout.content_left + max_prefix_trim.read(reader).prefix_left_offset - horizontal_scroll_offset;

                if right <= left {
                    return None;
                }

                let code_rect = Rect::from_left_top_right_bottom(left, selection_top, right, selection_bottom)
                    .with_vertical_horizontal_margin(VERTICAL_PADDING, HORIZONTAL_PADDING);

                Some(Rc::new(DeletionLayoutInfo { code_rect, content_left: editor_layout.content_left }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        // `_originalOverlay`
        let original_border_color = get_original_border_color(&tab_action);
        let visual: IObservable<Option<Rc<DeletionVisual>>> = {
            let (editor_layout_info, ui_state) = (Rc::clone(&editor_layout_info), Rc::clone(&ui_state));
            derived(move |reader| {
                let layout_info = editor_layout_info.read(reader)?;

                // Create an overlay which hides the left hand side of the original overlay when it overflows to the left
                // such that there is a smooth transition at the edge of content left
                let overlay_hider = Rect::from_left_top_right_bottom(
                    layout_info.content_left - BORDER_RADIUS - BORDER_WIDTH,
                    layout_info.code_rect.top,
                    layout_info.content_left,
                    layout_info.code_rect.bottom,
                );

                let code_rect = layout_info.code_rect;
                let left = code_rect.left.max(overlay_hider.left);
                let overlay_rect = Rect::new(left, code_rect.top, left.max(code_rect.right), code_rect.bottom);

                let editor_type = ui_state.read(reader).map(|s| s.editor_type);
                let separator_width =
                    if editor_type == Some(InlineCompletionEditorType::DiffEditor) { WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH } else { WIDGET_SEPARATOR_WIDTH };
                let separator_rect = overlay_rect.with_margin(separator_width);
                let editor_background = get_editor_background_color(editor_type.unwrap_or(InlineCompletionEditorType::TextEditor));

                Some(Rc::new(DeletionVisual {
                    separator: DeletionBox {
                        rect: separator_rect,
                        border_radius: BORDER_RADIUS,
                        border_width: BORDER_WIDTH + separator_width,
                        border_color: Some(editor_background),
                        background_color: None,
                    },
                    overlay: DeletionBox {
                        rect: overlay_rect,
                        border_radius: BORDER_RADIUS,
                        border_width: BORDER_WIDTH,
                        border_color: Some(original_border_color.read(reader)),
                        background_color: Some(ORIGINAL_BACKGROUND_COLOR),
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

        let display = ui_state.map(|s, _| s.is_some());

        let min_content_width_in_px = derived(move |reader| match editor_layout_info.read(reader) {
            Some(info) => info.code_rect.width(),
            None => 0.0,
        });
        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(const_observable(None), min_content_width_in_px)));

        Rc::new(Self { base, display, visual, is_hovered: const_observable(false) })
    }

    /// `_display`: a view só aparece com o estado de interface presente (`display: block | none`).
    pub fn is_displayed(&self) -> IObservable<bool> {
        Rc::clone(&self.display)
    }

    /// O dado que o pintor nativo lê (`None` sem layout).
    pub fn visual(&self) -> IObservable<Option<Rc<DeletionVisual>>> {
        Rc::clone(&self.visual)
    }
}

impl IInlineEditsView for InlineEditsDeletionView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        Event::none()
    }
}

impl IDisposable for InlineEditsDeletionView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
