//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsSideBySideView.ts`:
//! `InlineEditsSideBySideView`, a view de uma edição lado a lado (as linhas originais com uma moldura e, à direita delas,
//! um editor de leitura com as linhas novas).
//!
//! Notas de tradução:
//! - o `n.div` da raiz (`_nonOverflowView`: o SVG de fundo, o contêiner do editor de prévia, a sobreposição do lado original
//!   e a do lado modificado) é o [`SideBySideVisual`], lido pelo pintor nativo em [`InlineEditsSideBySideView::visual`] (ver
//!   as notas do módulo pai). O `mapOutFalsy` do original só mantém a árvore DOM estável entre recálculos; aqui há um único
//!   dado calculado, então ele não aparece. A visibilidade (`display: block | none`, o `_display`) é
//!   [`InlineEditsSideBySideView::is_displayed`];
//! - `ENABLE_OVERFLOW` é a constante `false` do original, então o `_shouldOverflow` é sempre falso e o ramo `[]` do
//!   `_nonOverflowView` nunca roda: o derivado, o `StickyScrollController` e o `_stickyScrollHeight` que só o alimentavam
//!   não aparecem, e as três partes sempre estão no visual;
//! - `ICodeEditor` e o `observableCodeEditor(editor)` do construtor são o `Rc<CodeEditorWidget>` e o
//!   [`observable_code_editor`]; `@IThemeService` é o `Rc<dyn IThemeService>`. `@IInstantiationService` (que criava o
//!   `EmbeddedCodeEditorWidget`) e `@IUserInteractionService` (o `createHoverTracker` do contêiner) saem: o editor de
//!   prévia é criado pela [`IEmbeddedCodeEditorFactory`], e o ponteiro sobre o contêiner chega por
//!   [`InlineEditsSideBySideView::notify_hover`], como na view de substituição de palavras. O `onclick` do contêiner é o
//!   [`InlineEditsSideBySideView::notify_click`] (o `onmousedown` com `preventDefault`, que impede o editor de perder o
//!   foco, é do pintor);
//! - o `getWindow(...).innerWidth` e o `getBoundingClientRect()` do contêiner do editor são o [`ISideBySideViewHost`]: o
//!   dono do contêiner nativo é quem os sabe. A largura da janela é lida a cada cálculo do layout, como no original; o
//!   retângulo é o de `observeEditorBoundingClientRect`;
//! - o `_uiState` anônimo do construtor (`{ newTextLineCount, editorType }`) é o [`SideBySideUiState`];
//! - o `editorContainer.readEffect(reader)` do `_updatePreviewEditor` só relia o nó DOM recriado; o contêiner é dado do
//!   visual, então a leitura não existe;
//! - a view zone `$('div.diagonal-fill.inline-edits-view-zone')` é a [`DiagonalFillViewZone`], cujo `dom_node` é o
//!   [`DiagonalFillNode`] que o pintor reconhece para desenhar o preenchimento diagonal;
//! - as bordas (`border`, `borderTop`...) e os cantos arredondados são [`SideBySideBorder`] e o [`BorderRadius`] de
//!   `utils`; `asCssVariable(id)` é o identificador do tema e `getEditorBlendedColor(...).toString()` é a [`Color`] opaca
//!   (o pintor serializa), ambos no [`WordReplacementColor`];
//! - `intersectHorizontal(new OffsetRange(left, Number.MAX_SAFE_INTEGER))` é o [`intersect_horizontal_from`], montado direto
//!   com o [`Rect`]: o `OffsetRange` do crate é inteiro e arredondaria a borda, e o limite da direita nunca restringe;
//! - `Color.transparent` de `_originalBackgroundColor` é [`Color::transparent_color`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, IReader, Observable, ObservableExt, SettableExt, autorun, const_observable, derived, derived_observable_with_cache, observable_from_event,
    observable_value,
};
use je_theme::color::Color;

use crate::code_editor_widget::{CodeEditorWidget, CodeEditorWidgetOptions, ICodeEditor};
use crate::core_2d::dimension::Dimension;
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{IViewZone, IViewZoneChangeAccessor, MouseEvent};
use crate::embedded_code_editor_widget::{EmbeddedCodeEditorWidget, IEmbeddedCodeEditorFactory};
use crate::inline_completions::controller::inline_completion_context_keys::inline_completion_context_keys;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::line_range::LineRange;
use crate::observable_code_editor::{ValueOrObservable, observable_code_editor};
use crate::position::Position;
use crate::range::Range;
use crate::services::theme_service::IThemeService;
use crate::text_model::TextModel;

use super::super::inline_edit_with_changes::InlineEditWithChanges;
use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{
    INLINE_EDITS_BORDER_RADIUS, MODIFIED_BACKGROUND_COLOR, ORIGINAL_BACKGROUND_COLOR, get_editor_background_color, get_editor_blended_color,
    get_modified_border_color, get_original_border_color,
};
use super::super::utils::utils::{
    BorderRadius, PathBuilder, PathCommand, get_content_render_width, get_offset_for_pos, max_content_width_in_range, observe_editor_bounding_client_rect,
};
use super::NonOverflowOverlayWidget;
use super::inline_edits_word_replacement_view::WordReplacementColor;

const HORIZONTAL_PADDING: f64 = 0.0;
const VERTICAL_PADDING: f64 = 0.0;

const BORDER_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_WIDTH: f64 = 1.0;
const WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH: f64 = 3.0;
const BORDER_RADIUS: f64 = INLINE_EDITS_BORDER_RADIUS;
const ORIGINAL_END_PADDING: f64 = 20.0;
const MODIFIED_END_PADDING: f64 = 12.0;

/// O `translate(-0.5 -0.5)` do SVG de fundo, em pixels (x e y).
pub const BACKGROUND_SVG_TRANSLATE: f64 = -0.5;
/// A largura que o editor de prévia ganha além da do conteúdo, para ele não rolar na horizontal.
const PREVIEW_EDITOR_EXTRA_WIDTH: f64 = 15.0;

/// O que o original lê do ambiente nativo do editor: `getWindow(editor.getContainerDomNode()).innerWidth` e
/// `editor.getContainerDomNode().getBoundingClientRect()` (ver as notas do módulo).
pub trait ISideBySideViewHost {
    /// `getBoundingClientRect()` do contêiner do editor, em coordenadas da janela.
    fn editor_bounding_client_rect(&self) -> Rect;

    /// `window.innerWidth`.
    fn window_inner_width(&self) -> f64;
}

/// O `_uiState` do construtor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SideBySideUiState {
    pub new_text_line_count: i32,
    pub editor_type: InlineCompletionEditorType,
}

/// O `_previewEditorLayoutInfo` do original.
#[derive(Clone, Debug, PartialEq)]
pub struct SideBySideLayoutInfo {
    pub code_rect: Rect,
    pub edit_rect: Rect,
    pub code_scroll_left: f64,
    pub content_left: f64,

    pub is_insertion: bool,
    pub max_content_width: f64,
    pub should_show_shadow: bool,
    pub desired_preview_editor_scroll_left: f64,
    pub preview_editor_width: f64,
}

/// O dado de uma borda: a largura em pixels (`solid`) e a cor.
#[derive(Clone)]
pub struct SideBySideBorder {
    pub width: f64,
    pub color: WordReplacementColor,
}

/// Uma caixa da moldura: o `div` com `position: absolute` no retângulo (em coordenadas do overlay) e `box-sizing: border-box`.
#[derive(Clone)]
pub struct SideBySideBox {
    pub rect: Rect,
    pub border_radius: BorderRadius,
    pub border_top: Option<SideBySideBorder>,
    pub border_right: Option<SideBySideBorder>,
    pub border_bottom: Option<SideBySideBorder>,
    pub border_left: Option<SideBySideBorder>,
    /// A cor do fundo.
    pub background_color: Option<WordReplacementColor>,
}

/// `originalCornerCutoutSideBySide`: o canto que encaixa a moldura original na moldura do lado modificado quando esta
/// desce mais. O `div` interno `originalCornerCutoutBorder` tem borda em cima e à direita e o `border-radius: 0 100% 0 0`.
#[derive(Clone)]
pub struct SideBySideCornerCutout {
    pub rect: Rect,
    /// `originalCornerCutoutBackground`: cobre o retângulo todo com a cor do fundo original misturada com a do editor.
    pub background_color: Rc<Color>,
    /// `originalCornerCutoutBorder`: a borda em cima e à direita (largura e cor) e o fundo do editor.
    pub border: SideBySideBorder,
    pub border_background_color: &'static str,
}

/// O lado original (o `_originalOverlay`, que só existe quando a edição não é uma inserção).
#[derive(Clone)]
pub struct SideBySideOriginal {
    /// `originalSeparatorSideBySide`.
    pub separator: SideBySideBox,
    /// `originalOverlaySideBySide`.
    pub overlay: SideBySideBox,
    /// `originalCornerCutoutSideBySide` (`display: none` quando a moldura do lado modificado não desce mais).
    pub corner_cutout: Option<SideBySideCornerCutout>,
    /// `originalOverlaySideBySideHider`: esconde o lado esquerdo da moldura quando ela passa do `contentLeft`; o fundo é o
    /// do editor.
    pub hider: SideBySideBox,
}

/// O lado modificado (o `_modifiedOverlay`).
#[derive(Clone)]
pub struct SideBySideModified {
    /// `modifiedInsertionSideBySide`: o traço da inserção (vazio quando não é inserção); o fundo é a cor da borda do
    /// modificado, como identificador do tema.
    pub insertion_rect: Rect,
    pub insertion_color: &'static str,
    /// `modifiedSeparatorSideBySide`.
    pub separator: SideBySideBox,
    /// `modifiedOverlaySideBySide`.
    pub overlay: SideBySideBox,
}

/// O `_editorContainer`: `position: absolute`, `overflow: hidden`, `cursor: pointer`. O `div.preview` de dentro
/// (`pointer-events: none`) é o editor de prévia, que ocupa `width` e `height` (a largura mais a folga contra a rolagem).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SideBySideEditorContainer {
    pub top: f64,
    pub left: f64,
    /// A largura do contêiner (que recorta a view zone).
    pub width: f64,
    /// O tamanho dado ao editor de prévia (`previewEditor.layout`).
    pub editor_width: f64,
    pub editor_height: f64,
}

/// O que o pintor nativo lê da view. O SVG de fundo tem `overflow: visible`, `pointer-events: none` e
/// `translate(`[`BACKGROUND_SVG_TRANSLATE`]` `[`BACKGROUND_SVG_TRANSLATE`]`)`; as sobreposições têm `pointer-events: none`.
#[derive(Clone)]
pub struct SideBySideVisual {
    /// `rightOfModifiedBackgroundCoverUp`: o caminho (`None` com o fundo original transparente) e o preenchimento, o fundo do
    /// editor como identificador do tema.
    pub cover_up_path: Option<Vec<PathCommand>>,
    pub cover_up_fill: &'static str,
    pub editor_container: SideBySideEditorContainer,
    /// `None` quando a edição é uma inserção.
    pub original: Option<SideBySideOriginal>,
    pub modified: SideBySideModified,
    pub layout: Rc<SideBySideLayoutInfo>,
}

/// O `dom_node` da view zone `diagonal-fill`: o pintor o reconhece e desenha o preenchimento diagonal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagonalFillNode;

/// A view zone do preenchimento diagonal do editor de prévia.
struct DiagonalFillViewZone {
    after_line_number: i32,
    height_in_lines: f64,
}

impl IViewZone for DiagonalFillViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number
    }

    fn height_in_lines(&self) -> Option<f64> {
        Some(self.height_in_lines)
    }

    fn show_in_hidden_areas(&self) -> bool {
        true
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        Rc::new(DiagonalFillNode)
    }
}

/// `intersectHorizontal(new OffsetRange(left, Number.MAX_SAFE_INTEGER))` (ver as notas do módulo).
fn intersect_horizontal_from(rect: &Rect, left: f64) -> Rect {
    let new_left = rect.left.max(left);
    Rect::new(new_left, rect.top, new_left.max(rect.right), rect.bottom)
}

/// O objeto de opções `{ ...entries }`.
fn options_object<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

/// As opções cruas do editor de prévia (o segundo argumento do `EmbeddedCodeEditorWidget`).
fn preview_editor_options() -> Dynamic {
    options_object([
        ("glyphMargin", Dynamic::from(false)),
        ("lineNumbers", Dynamic::from("off")),
        ("minimap", options_object([("enabled", Dynamic::from(false))])),
        (
            "guides",
            options_object([
                ("indentation", Dynamic::from(false)),
                ("bracketPairs", Dynamic::from(false)),
                ("bracketPairsHorizontal", Dynamic::from(false)),
                ("highlightActiveIndentation", Dynamic::from(false)),
            ]),
        ),
        ("editContext", Dynamic::from(false)), // is a bit faster
        ("rulers", Dynamic::array([])),
        ("padding", options_object([("top", Dynamic::from(0)), ("bottom", Dynamic::from(0))])),
        ("folding", Dynamic::from(false)),
        ("selectOnLineNumbers", Dynamic::from(false)),
        ("selectionHighlight", Dynamic::from(false)),
        ("columnSelection", Dynamic::from(false)),
        ("overviewRulerBorder", Dynamic::from(false)),
        ("overviewRulerLanes", Dynamic::from(0)),
        ("lineDecorationsWidth", Dynamic::from(0)),
        ("lineNumbersMinChars", Dynamic::from(0)),
        ("revealHorizontalRightPadding", Dynamic::from(0)),
        (
            "bracketPairColorization",
            options_object([("enabled", Dynamic::from(true)), ("independentColorPoolPerBracketType", Dynamic::from(false))]),
        ),
        ("scrollBeyondLastLine", Dynamic::from(false)),
        (
            "scrollbar",
            options_object([
                ("vertical", Dynamic::from("hidden")),
                ("horizontal", Dynamic::from("hidden")),
                ("handleMouseWheel", Dynamic::from(false)),
            ]),
        ),
        ("readOnly", Dynamic::from(true)),
        ("wordWrap", Dynamic::from("off")),
        ("wordWrapOverride1", Dynamic::from("off")),
        ("wordWrapOverride2", Dynamic::from("off")),
    ])
}

/// Uma borda `solid` com a cor do tema.
fn theme_border(width: f64, color: &'static str) -> Option<SideBySideBorder> {
    Some(SideBySideBorder { width, color: WordReplacementColor::Theme(color) })
}

/// `InlineEditsSideBySideView`.
pub struct InlineEditsSideBySideView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    display: IObservable<bool>,
    hovered: Rc<ObservableValue<bool, ()>>,
    preview_editor: Rc<EmbeddedCodeEditorWidget>,
    visual: IObservable<Option<Rc<SideBySideVisual>>>,
}

impl InlineEditsSideBySideView {
    /// This is an approximation and should be improved by using the real parameters used bellow.
    pub fn fits_inside_viewport(editor: &Rc<CodeEditorWidget>, text_model: &Rc<TextModel>, edit: &InlineEditWithChanges, reader: &dyn IReader) -> bool {
        let editor_obs = observable_code_editor(editor);
        let editor_width = editor_obs.layout_info_width.read(reader);
        let editor_content_left = editor_obs.layout_info_content_left.read(reader);
        let editor_vertical_scrollbar = editor.get_layout_info().vertical_scrollbar_width;
        let minimap = editor_obs.layout_info_minimap.read(reader);
        let minimap_width = if minimap.minimap_left != 0.0 { minimap.minimap_width } else { 0.0 };

        let max_original_content = max_content_width_in_range(&editor_obs, edit.display_range(), None /* do not reconsider on each layout info change */);
        let max_modified_content = edit.line_edit().new_lines.iter().fold(0.0_f64, |max, line| max.max(get_content_render_width(line, editor, text_model)));
        let original_padding = ORIGINAL_END_PADDING; // padding after last line of original editor
        let modified_padding = MODIFIED_END_PADDING + 2.0 * BORDER_WIDTH; // padding after last line of modified editor

        max_original_content + max_modified_content + original_padding + modified_padding
            < editor_width - editor_content_left - editor_vertical_scrollbar - minimap_width
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        edit: IObservable<Option<Rc<InlineEditWithChanges>>>,
        preview_text_model: Rc<TextModel>,
        ui_state: IObservable<Option<Rc<SideBySideUiState>>>,
        tab_action: IObservable<InlineEditTabAction>,
        theme_service: Rc<dyn IThemeService>,
        host: Rc<dyn ISideBySideViewHost>,
        preview_editor_factory: &dyn IEmbeddedCodeEditorFactory,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(on_did_click.clone());

        let editor_obs = observable_code_editor(&editor);

        let display: IObservable<bool> = ui_state.map(|s, _| s.is_some());
        let hovered = observable_value::<bool, ()>("isHovered", false);
        let separator_width_obs: IObservable<f64> = ui_state.map(|s, _| {
            if s.is_some_and(|s| s.editor_type == InlineCompletionEditorType::DiffEditor) { WIDGET_SEPARATOR_DIFF_EDITOR_WIDTH } else { WIDGET_SEPARATOR_WIDTH }
        });

        let preview_editor = EmbeddedCodeEditorWidget::new(
            preview_editor_options(),
            CodeEditorWidgetOptions { contributions: Some(Vec::new()), telemetry_data: None },
            vec![(inline_completion_context_keys().in_inline_edits_preview_editor.key().to_owned(), Dynamic::from(true))],
            Rc::clone(&editor),
            preview_editor_factory,
        );
        base.register(preview_editor.clone());
        let preview_editor_obs = observable_code_editor(preview_editor.editor());

        let active_view_zones: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let update_preview_editor: IObservable<()> = {
            let (preview_editor, preview_editor_obs, display, edit, ui_state, preview_text_model, active_view_zones) = (
                Rc::clone(&preview_editor),
                Rc::clone(&preview_editor_obs),
                Rc::clone(&display),
                Rc::clone(&edit),
                Rc::clone(&ui_state),
                Rc::clone(&preview_text_model),
                Rc::clone(&active_view_zones),
            );
            derived(move |reader| {
                preview_editor_obs.model.read(reader); // update when the model is set

                // Setting this here explicitly to make sure that the preview editor is
                // visible when needed, we're also checking that these fields are defined
                // because of the auto run initial
                // Before removing these, verify with a non-monospace font family
                display.read(reader);

                let (Some(ui_state), Some(edit)) = (ui_state.read(reader), edit.read(reader)) else {
                    return;
                };

                let range = edit.original_line_range();
                let line_count = CursorSimpleModel::get_line_count(&*preview_text_model);

                let mut hidden_areas: Vec<Range> = Vec::new();
                if range.start_line_number > 1 {
                    hidden_areas.push(Range::new(1, 1, range.start_line_number - 1, 1));
                }
                if range.start_line_number + ui_state.new_text_line_count < line_count + 1 {
                    hidden_areas.push(Range::new(range.start_line_number + ui_state.new_text_line_count, 1, line_count + 1, 1));
                }

                preview_editor.editor().set_hidden_areas(&hidden_areas, None, Some(true));

                // O original questiona se esta é a forma certa de tratar as view zones.
                let previous_view_zones = std::mem::take(&mut *active_view_zones.borrow_mut());

                let reduced_lines_count = (range.end_line_number_exclusive - range.start_line_number) - ui_state.new_text_line_count;
                let mut added_view_zones: Vec<String> = Vec::new();
                preview_editor.editor().change_view_zones(&mut |change_accessor: &mut dyn IViewZoneChangeAccessor| {
                    for id in &previous_view_zones {
                        change_accessor.remove_zone(id);
                    }

                    if reduced_lines_count > 0 {
                        added_view_zones.push(change_accessor.add_zone(Rc::new(DiagonalFillViewZone {
                            after_line_number: range.start_line_number + ui_state.new_text_line_count - 1,
                            height_in_lines: f64::from(reduced_lines_count),
                        })));
                    }
                });
                *active_view_zones.borrow_mut() = added_view_zones;
            })
        };

        let preview_editor_width: IObservable<f64> = {
            let (edit, update_preview_editor, preview_editor_obs) = (Rc::clone(&edit), Rc::clone(&update_preview_editor), Rc::clone(&preview_editor_obs));
            derived(move |reader| {
                let Some(edit) = edit.read(reader) else {
                    return 0.0;
                };
                update_preview_editor.read(reader);

                max_content_width_in_range(&preview_editor_obs, edit.modified_line_range(), Some(reader))
            })
        };

        let cursor_pos_if_touches_edit: IObservable<Option<Position>> = {
            let (editor_obs, edit) = (Rc::clone(&editor_obs), Rc::clone(&edit));
            derived(move |reader| {
                let cursor_pos = editor_obs.cursor_position.read(reader);
                let edit = edit.read(reader);
                let (Some(edit), Some(cursor_pos)) = (edit, cursor_pos) else {
                    return None;
                };
                edit.modified_line_range().contains(cursor_pos.line_number).then_some(cursor_pos)
            })
        };

        let original_start_position: IObservable<Option<Position>> = {
            let edit = Rc::clone(&edit);
            derived(move |reader| edit.read(reader).map(|inline_edit| Position::new(inline_edit.original_line_range().start_line_number, 1)))
        };
        let original_end_position: IObservable<Option<Position>> = {
            let edit = Rc::clone(&edit);
            derived(move |reader| edit.read(reader).map(|inline_edit| Position::new(inline_edit.original_line_range().end_line_number_exclusive, 1)))
        };
        let original_vertical_start_position: IObservable<Option<f64>> =
            editor_obs.observe_position(&original_start_position, base.store()).map(|p, _| p.map(|p| p.y));
        let original_vertical_end_position: IObservable<Option<f64>> =
            editor_obs.observe_position(&original_end_position, base.store()).map(|p, _| p.map(|p| p.y));
        let original_display_range: IObservable<Option<Rc<LineRange>>> = edit.map(|e, _| e.map(|e| Rc::new(e.display_range())));
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

        let editor_dom_content_rect = {
            let host = Rc::clone(&host);
            observe_editor_bounding_client_rect(&editor, move || host.editor_bounding_client_rect(), base.store())
        };

        let preview_editor_layout_info: IObservable<Option<Rc<SideBySideLayoutInfo>>> = {
            let (editor, editor_obs, edit, ui_state, preview_editor_obs, host) =
                (Rc::clone(&editor), Rc::clone(&editor_obs), Rc::clone(&edit), Rc::clone(&ui_state), Rc::clone(&preview_editor_obs), Rc::clone(&host));
            derived(move |reader| {
                let inline_edit = edit.read(reader)?;
                ui_state.read(reader)?;

                let range = inline_edit.original_line_range();

                let horizontal_scroll_offset = editor_obs.scroll_left.read(reader);

                let editor_content_max_width_in_range = editor_max_content_width_in_range.read(reader);
                let editor_layout = editor_obs.layout_info.read(reader);
                let preview_content_width = preview_editor_width.read(reader);
                let editor_content_area_width = editor_layout.content_width - editor_layout.vertical_scrollbar_width;
                let editor_bounding_client_rect = editor_dom_content_rect.read(reader);
                let client_content_area_right = editor_layout.content_left + editor_layout.content_width + editor_bounding_client_rect.left;
                let window_inner_width = host.window_inner_width();
                let remaining_width_right_of_content = window_inner_width - client_content_area_right;
                let remaining_width_right_of_editor = window_inner_width - editor_bounding_client_rect.right;
                let desired_minimum_width = (editor_layout.content_width * 0.3).min(preview_content_width).min(100.0);
                const IN_EDITOR_DISPLACEMENT: f64 = 0.0;
                let maximum_available_width = IN_EDITOR_DISPLACEMENT + remaining_width_right_of_content;

                let cursor_pos = cursor_pos_if_touches_edit.read(reader);

                let max_preview_editor_left = f64::max(
                    // We're starting from the content area right and moving it left by IN_EDITOR_DISPLACEMENT and also by an amount to ensure some minimum desired width
                    editor_content_area_width + horizontal_scroll_offset - IN_EDITOR_DISPLACEMENT - f64::max(0.0, desired_minimum_width - maximum_available_width),
                    // But we don't want that the moving left ends up covering the cursor, so this will push it to the right again
                    f64::min(
                        cursor_pos.map_or(0.0, |cursor_pos| get_offset_for_pos(&editor_obs, cursor_pos, reader) + 50.0),
                        editor_content_area_width + horizontal_scroll_offset,
                    ),
                );
                let preview_editor_left_in_text_area = (editor_content_max_width_in_range + ORIGINAL_END_PADDING).min(max_preview_editor_left);

                let max_content_width = editor_content_max_width_in_range + ORIGINAL_END_PADDING + preview_content_width + 70.0;

                let dist = max_preview_editor_left - preview_editor_left_in_text_area;

                let desired_preview_editor_scroll_left;
                let code_right;
                if preview_editor_left_in_text_area > horizontal_scroll_offset {
                    desired_preview_editor_scroll_left = 0.0;
                    code_right = editor_layout.content_left + preview_editor_left_in_text_area - horizontal_scroll_offset;
                } else {
                    desired_preview_editor_scroll_left = horizontal_scroll_offset - preview_editor_left_in_text_area;
                    code_right = editor_layout.content_left;
                }

                let selection_top = match original_vertical_start_position.read(reader) {
                    Some(top) => top,
                    None => editor.get_top_for_line_number(range.start_line_number, None) - editor_obs.scroll_top.read(reader),
                };
                let selection_bottom = match original_vertical_end_position.read(reader) {
                    Some(bottom) => bottom,
                    None => editor.get_bottom_for_line_number(range.end_line_number_exclusive - 1, None) - editor_obs.scroll_top.read(reader),
                };

                // O original deixa o `prefixLeftOffset` de `getPrefixTrim` de fora do cálculo.
                let code_left = editor_layout.content_left - horizontal_scroll_offset;

                let mut code_rect = Rect::from_left_top_right_bottom(code_left, selection_top, code_right, selection_bottom);
                let is_insertion = code_rect.height() == 0.0;
                if !is_insertion {
                    code_rect = code_rect.with_vertical_horizontal_margin(VERTICAL_PADDING, HORIZONTAL_PADDING);
                }

                let preview_line_heights =
                    preview_editor_obs.observe_line_heights_for_line_range(ValueOrObservable::Value(inline_edit.modified_line_range())).read(reader);
                let edit_height: f64 = preview_line_heights.iter().sum();
                let code_height = selection_bottom - selection_top;
                let preview_editor_height = code_height.max(edit_height);

                let clipped = dist == 0.0;
                let code_edit_dist = 0.0;
                let preview_editor_width = (preview_content_width + MODIFIED_END_PADDING)
                    .min(remaining_width_right_of_editor + editor_layout.width - editor_layout.content_left - code_edit_dist);

                let mut edit_rect = Rect::from_left_top_width_height(code_rect.right + code_edit_dist, selection_top, preview_editor_width, preview_editor_height);
                if !is_insertion {
                    edit_rect = edit_rect.with_vertical_horizontal_margin(VERTICAL_PADDING, HORIZONTAL_PADDING).translate_x(HORIZONTAL_PADDING + BORDER_WIDTH);
                } else {
                    // Align top of edit with insertion line
                    edit_rect = edit_rect.with_vertical_horizontal_margin(VERTICAL_PADDING, HORIZONTAL_PADDING).translate_y(VERTICAL_PADDING);
                }

                // debugView(debugLogRects({ codeRect, editRect }, this._editor.getDomNode()!), reader);

                Some(Rc::new(SideBySideLayoutInfo {
                    code_rect,
                    edit_rect,
                    code_scroll_left: horizontal_scroll_offset,
                    content_left: editor_layout.content_left,

                    is_insertion,
                    max_content_width,
                    should_show_shadow: clipped,
                    desired_preview_editor_scroll_left,
                    preview_editor_width,
                }))
            })
        };

        let original_background_color: IObservable<Rc<Color>> = {
            let theme_service = Rc::clone(&theme_service);
            observable_from_event(theme_service.on_did_color_theme_change(), {
                let theme_service = Rc::clone(&theme_service);
                move |_| {
                    Rc::new(theme_service.get_color_theme().get_color(ORIGINAL_BACKGROUND_COLOR, None).unwrap_or_else(Color::transparent_color))
                }
            })
        };
        let editor_background_color: IObservable<&'static str> =
            ui_state.map(|s, _| get_editor_background_color(s.map_or(InlineCompletionEditorType::TextEditor, |s| s.editor_type)));

        let original_border_color = get_original_border_color(&tab_action);
        let modified_border_color = get_modified_border_color(&tab_action);
        let original_background_blended = get_editor_blended_color(ValueOrObservable::Value(ORIGINAL_BACKGROUND_COLOR), &theme_service);
        let modified_border_blended = get_editor_blended_color(ValueOrObservable::Observable(Rc::clone(&modified_border_color)), &theme_service);

        // `_nonOverflowView`
        let visual: IObservable<Option<Rc<SideBySideVisual>>> = {
            let layout_obs = Rc::clone(&preview_editor_layout_info);
            derived(move |reader| {
                let layout_info = layout_obs.read(reader)?;

                let editor_background = editor_background_color.read(reader);
                let separator_width = separator_width_obs.read(reader);
                let is_modified_lower = layout_info.code_rect.bottom < layout_info.edit_rect.bottom;

                // `_backgroundSvg`
                let cover_up_path = if original_background_color.read(reader).is_transparent() {
                    None
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(layout_info.code_rect.get_right_top())
                        .line_to(layout_info.code_rect.get_right_top().delta_x(1000.0))
                        .line_to(layout_info.code_rect.get_right_bottom().delta_x(1000.0))
                        .line_to(layout_info.code_rect.get_right_bottom());
                    Some(path.build())
                };

                // `_originalOverlay`
                let original = if layout_info.is_insertion {
                    None
                } else {
                    let original_border = original_border_color.read(reader);
                    let separator_border = theme_border(BORDER_WIDTH + separator_width, editor_background);
                    let original_border_side = theme_border(BORDER_WIDTH, original_border);
                    let has_border_left = layout_info.code_scroll_left != 0.0;
                    let transition_rect_size = BORDER_RADIUS * 2.0 + BORDER_WIDTH * 2.0;
                    let rounded_left = BorderRadius { top_left: BORDER_RADIUS, top_right: 0.0, bottom_left: BORDER_RADIUS, bottom_right: 0.0 };

                    // Create an overlay which hides the left hand side of the original overlay when it overflows to the left
                    // such that there is a smooth transition at the edge of content left
                    let overlay_hider = Rect::from_left_top_right_bottom(
                        layout_info.content_left - BORDER_RADIUS - BORDER_WIDTH,
                        layout_info.code_rect.top,
                        layout_info.content_left,
                        layout_info.code_rect.bottom + transition_rect_size,
                    );

                    let overlay_rect = intersect_horizontal_from(&layout_info.code_rect, overlay_hider.left);
                    let separator_rect = intersect_horizontal_from(
                        &overlay_rect.with_margins(separator_width, 0.0, separator_width, separator_width),
                        overlay_hider.left,
                    );
                    let transition_rect = intersect_horizontal_from(
                        &Rect::from_left_top_width_height(
                            overlay_rect.right - transition_rect_size + BORDER_WIDTH,
                            overlay_rect.bottom - BORDER_WIDTH,
                            transition_rect_size,
                            transition_rect_size,
                        ),
                        overlay_hider.left,
                    );

                    Some(SideBySideOriginal {
                        separator: SideBySideBox {
                            rect: separator_rect,
                            border_radius: rounded_left,
                            border_top: separator_border.clone(),
                            border_right: None,
                            border_bottom: separator_border.clone(),
                            border_left: if has_border_left { None } else { separator_border },
                            background_color: None,
                        },
                        overlay: SideBySideBox {
                            rect: overlay_rect,
                            border_radius: rounded_left,
                            border_top: original_border_side.clone(),
                            border_right: None,
                            border_bottom: original_border_side.clone(),
                            border_left: if has_border_left { None } else { original_border_side },
                            background_color: Some(WordReplacementColor::Theme(ORIGINAL_BACKGROUND_COLOR)),
                        },
                        corner_cutout: is_modified_lower.then(|| SideBySideCornerCutout {
                            rect: transition_rect,
                            background_color: original_background_blended.read(reader),
                            border: SideBySideBorder { width: BORDER_WIDTH, color: WordReplacementColor::Theme(original_border) },
                            border_background_color: editor_background,
                        }),
                        hider: SideBySideBox {
                            rect: overlay_hider,
                            border_radius: BorderRadius::from(0.0),
                            border_top: None,
                            border_right: None,
                            border_bottom: None,
                            border_left: None,
                            background_color: Some(WordReplacementColor::Theme(editor_background)),
                        },
                    })
                };

                // `_modifiedOverlay`
                let modified = {
                    let border_radius = BorderRadius {
                        top_left: 0.0,
                        top_right: BORDER_RADIUS,
                        bottom_right: BORDER_RADIUS,
                        bottom_left: if is_modified_lower { BORDER_RADIUS } else { 0.0 },
                    };
                    let modified_border = Some(SideBySideBorder { width: 1.0, color: WordReplacementColor::Resolved(modified_border_blended.read(reader)) });
                    let separator_border = theme_border(BORDER_WIDTH + separator_width, editor_background);

                    let overlay_rect = layout_info.edit_rect.with_vertical_horizontal_margin(0.0, BORDER_WIDTH);
                    let separator_rect = overlay_rect.with_margins(separator_width, separator_width, separator_width, 0.0);

                    let insertion_rect = if !layout_info.is_insertion || layout_info.content_left >= overlay_rect.left {
                        Rect::from_left_top_width_height(overlay_rect.left, overlay_rect.top, 0.0, 0.0)
                    } else {
                        Rect::new(layout_info.content_left, overlay_rect.top, overlay_rect.left, overlay_rect.top + BORDER_WIDTH * 2.0)
                    };

                    SideBySideModified {
                        insertion_rect,
                        insertion_color: modified_border_color.read(reader),
                        separator: SideBySideBox {
                            rect: separator_rect,
                            border_radius,
                            border_top: separator_border.clone(),
                            border_right: separator_border.clone(),
                            border_bottom: separator_border,
                            border_left: None,
                            background_color: None,
                        },
                        overlay: SideBySideBox {
                            rect: overlay_rect,
                            border_radius,
                            border_top: modified_border.clone(),
                            border_right: modified_border.clone(),
                            border_bottom: modified_border.clone(),
                            border_left: modified_border,
                            background_color: Some(WordReplacementColor::Theme(MODIFIED_BACKGROUND_COLOR)),
                        },
                    }
                };

                // `_editorContainer`: a posição, a largura que recorta a view zone e o tamanho do editor de prévia
                let editor_rect = layout_info.edit_rect.with_vertical_horizontal_margin(-VERTICAL_PADDING, -HORIZONTAL_PADDING);

                Some(Rc::new(SideBySideVisual {
                    cover_up_path,
                    cover_up_fill: editor_background,
                    editor_container: SideBySideEditorContainer {
                        top: editor_rect.top,
                        left: editor_rect.left,
                        width: layout_info.preview_editor_width + HORIZONTAL_PADDING, // Set width to clip view zone
                        editor_width: layout_info.preview_editor_width + PREVIEW_EDITOR_EXTRA_WIDTH, // Make sure editor does not scroll horizontally
                        editor_height: editor_rect.height(),
                    },
                    original,
                    modified,
                    layout: layout_info,
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(
            const_observable(None),
            derived({
                let layout_obs = Rc::clone(&preview_editor_layout_info);
                move |reader| layout_obs.read(reader).map_or(0.0, |layout_info| layout_info.max_content_width)
            }),
        )));

        preview_editor.editor().set_model(Some(Rc::clone(&preview_text_model)));

        base.register(autorun({
            let (preview_editor, layout_obs) = (Rc::clone(&preview_editor), Rc::clone(&preview_editor_layout_info));
            move |reader| {
                let Some(layout_info) = layout_obs.read(reader) else {
                    return;
                };
                let editor_rect = layout_info.edit_rect.with_vertical_horizontal_margin(-VERTICAL_PADDING, -HORIZONTAL_PADDING);

                preview_editor
                    .editor()
                    .layout(Some(Dimension { height: editor_rect.height(), width: layout_info.preview_editor_width + PREVIEW_EDITOR_EXTRA_WIDTH }));
            }
        }));

        base.register(autorun({
            let (preview_editor, layout_obs) = (Rc::clone(&preview_editor), Rc::clone(&preview_editor_layout_info));
            move |reader| {
                let Some(layout_info) = layout_obs.read(reader) else {
                    return;
                };

                preview_editor.editor().set_scroll_left(layout_info.desired_preview_editor_scroll_left, None);
            }
        }));

        update_preview_editor.recompute_initially_and_on_change(base.store(), None);

        Rc::new(Self { base, on_did_click, display, hovered, preview_editor, visual })
    }

    /// `previewEditor`: o editor embutido com as linhas novas.
    pub fn preview_editor(&self) -> &Rc<EmbeddedCodeEditorWidget> {
        &self.preview_editor
    }

    /// `_display`: a view só aparece com o estado de interface presente (`display: block | none`).
    pub fn is_displayed(&self) -> IObservable<bool> {
        Rc::clone(&self.display)
    }

    /// O dado que o pintor nativo lê (`None` sem estado de interface ou sem edição).
    pub fn visual(&self) -> IObservable<Option<Rc<SideBySideVisual>>> {
        Rc::clone(&self.visual)
    }

    /// O `createHoverTracker` do `_editorContainer`: o pintor avisa quando o ponteiro entra ou sai dele.
    pub fn notify_hover(&self, hovered: bool) {
        self.hovered.set(hovered, None);
    }

    /// O `onclick` do `_editorContainer`: o pintor entrega o evento.
    pub fn notify_click(&self, event: MouseEvent) {
        self.on_did_click.fire(InlineEditClickEvent::new(event, false));
    }
}

impl IInlineEditsView for InlineEditsSideBySideView {
    fn is_hovered(&self) -> IObservable<bool> {
        self.hovered.to_observable()
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for InlineEditsSideBySideView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
