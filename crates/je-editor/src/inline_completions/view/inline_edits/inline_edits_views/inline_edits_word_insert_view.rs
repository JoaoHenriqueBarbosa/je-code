//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsWordInsertView.ts`:
//! `InlineEditsWordInsertView`, a view de uma palavra inserida (o texto numa caixa abaixo da posição, com uma ponta que
//! aponta para ela).
//!
//! Notas de tradução:
//! - o `n.div` com os quatro filhos (fundo inferior, texto modificado, fundo com borda e a ponta em SVG) é o
//!   [`WordInsertVisual`], que o pintor nativo lê em [`InlineEditsWordInsertView::visual`] (ver as notas do módulo
//!   pai). Os filhos saem na ordem de pintura do original: [`WordInsertLayout::lower_background`], o texto em
//!   [`WordInsertLayout::modified`], [`WordInsertLayout::background`] (com a borda) e a ponta;
//! - o `rectToProps(reader => layout.read(reader).x)` do original só repassa o retângulo, que aqui é o campo do layout;
//! - `ObservableCodeEditor` vem como `Rc<ObservableCodeEditor>` (é o que `observable_code_editor` devolve);
//! - o `intersectVertical(new OffsetRange(modified.top - 2, Number.MAX_SAFE_INTEGER))` do fundo inferior é o
//!   [`Rect::with_top`] com a borda de cima mais baixa entre as duas: o `OffsetRange` do crate é inteiro e arredondaria a
//!   borda, e o limite de baixo nunca restringe (o fundo vai até `modified.bottom + 4`);
//! - o `background: 'var(--vscode-inlineEdit-wordReplacementView-background)'` do fundo com borda usa uma cor que nenhum
//!   arquivo do original registra; sem a variável o CSS descarta a declaração, então o fundo fica sem preenchimento e
//!   [`WordInsertVisual`] não carrega essa cor;
//! - o `transform: scale(1.4, 1.4)` do SVG da ponta gira em torno do centro da caixa `12x18` (a origem padrão do CSS):
//!   [`TAIL_SCALE`] e [`tail_path`] dão a forma nas coordenadas da `viewBox`.

use std::rc::Rc;

use je_base::JsString;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, ObservableExt, const_observable, derived};

use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::font_size::FONT_SIZE;
use crate::config::editor_options::font_weight::FONT_WEIGHT;
use crate::config::editor_options::registry::FONT_FAMILY;
use crate::core_2d::point::Point;
use crate::core_2d::rect::Rect;
use crate::edits::text_edit::TextReplacement;
use crate::editor_browser::{OverlayWidgetPlacement, OverlayWidgetPosition, OverlayWidgetPositionCoordinates};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable};

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{MODIFIED_CHANGED_TEXT_OVERLAY_COLOR, get_editor_background_color, get_modified_border_color};
use super::super::utils::utils::{PathBuilder, PathCommand};
use super::NonOverflowOverlayWidget;

/// A largura da `viewBox` do SVG da ponta.
pub const TAIL_VIEW_BOX_WIDTH: f64 = 12.0;
/// A altura da `viewBox` do SVG da ponta.
pub const TAIL_VIEW_BOX_HEIGHT: f64 = 18.0;
/// O fator do `transform: scale(1.4, 1.4)` da ponta.
pub const TAIL_SCALE: f64 = 1.4;

/// O `path` do SVG da ponta, nas coordenadas da `viewBox` (`0 0 12 18`). O `d` do original termina no ponto em que
/// começa, então o preenchimento fecha o contorno sozinho.
pub fn tail_path() -> Vec<PathCommand> {
    let p = Point::new;
    let mut path = PathBuilder::new();
    path.move_to(p(5.06445, 0.0))
        .line_to(p(7.35759, 0.0))
        .curve_to2(p(7.35759, 0.0), p(7.35759, 8.47059), p(7.35759, 11.1176))
        .curve_to2(p(7.35759, 13.7647), p(9.4552, 18.0), p(13.4674, 18.0))
        .curve_to2(p(17.4795, 18.0), p(-2.58445, 18.0), p(0.281373, 18.0))
        .curve_to2(p(3.14719, 18.0), p(5.06477, 14.2941), p(5.06477, 11.1176))
        .curve_to2(p(5.06477, 7.94118), p(5.06445, 0.0), p(5.06445, 0.0));
    path.build()
}

/// O `_layout` do original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordInsertLayout {
    pub modified: Rect,
    pub center: Point,
    pub background: Rect,
    pub lower_background: Rect,
}

impl WordInsertLayout {
    /// A posição (`left`, `top`) do SVG da ponta: `center.x - 9` e `center.y + 4`.
    pub fn tail_position(&self) -> Point {
        Point::new(self.center.x - 9.0, self.center.y + 4.0)
    }
}

/// O que o pintor nativo lê da view: o layout e os estilos dos filhos do `n.div`.
#[derive(Clone, Debug, PartialEq)]
pub struct WordInsertVisual {
    pub layout: WordInsertLayout,
    /// O texto da caixa `modified` (`this._edit.text`).
    pub text: JsString,
    /// A cor da borda do fundo (`getModifiedBorderColor(tabAction)`), como identificador do tema.
    pub modified_border_color: &'static str,
    /// A cor do fundo inferior (`--vscode-editor-background`).
    pub lower_background_color: &'static str,
    /// A cor do fundo da caixa `modified` e da ponta (`--vscode-inlineEdit-modifiedChangedTextBackground`).
    pub modified_background_color: &'static str,
    pub font_family: Rc<String>,
    pub font_size: f64,
    pub font_weight: Rc<String>,
}

/// `InlineEditsWordInsertView`.
pub struct InlineEditsWordInsertView {
    base: Rc<Disposable>,
    visual: IObservable<Option<Rc<WordInsertVisual>>>,
    is_hovered: IObservable<bool>,
}

impl InlineEditsWordInsertView {
    /// `edit` must be single-line in both sides.
    pub fn new(editor: Rc<ObservableCodeEditor>, edit: TextReplacement, tab_action: IObservable<InlineEditTabAction>) -> Rc<Self> {
        let base = Disposable::new();

        let start_position = edit.range.get_start_position();
        let start = editor.observe_position(&const_observable(Some(start_position)), base.store());

        let content_left = Rc::clone(&editor.layout_info_content_left);
        let scroll_left = Rc::clone(&editor.scroll_left);
        let line_height = editor.observe_line_height_for_position(ValueOrObservable::Value(start_position));
        let font_info = editor.get_option(FONT_INFO);
        let font_family = editor.get_option(FONT_FAMILY);
        let font_size = editor.get_option(FONT_SIZE);
        let font_weight = editor.get_option(FONT_WEIGHT);
        let modified_border_color = get_modified_border_color(&tab_action);
        let text = edit.text;

        // `_layout` e o `n.div` que lê o layout.
        let visual = derived(move |reader| {
            let start = start.read(reader)?;
            let content_left = content_left.read(reader);
            let line_height = line_height.read(reader);

            let w = font_info.read(reader).typical_halfwidth_character_width;
            let width = text.len() as f64 * w + 5.0;

            let center = Point::new(content_left + start.x + w / 2.0 - scroll_left.read(reader), start.y);

            let modified = Rect::from_left_top_width_height(center.x - width / 2.0, center.y + line_height + 5.0, width, line_height);
            let background = Rect::hull(&[Rect::from_point(center), modified]).with_margin(4.0);
            let lower_background = background.with_top(background.top.max(modified.top - 2.0));

            Some(Rc::new(WordInsertVisual {
                layout: WordInsertLayout { modified, center, background, lower_background },
                text: text.clone(),
                modified_border_color: modified_border_color.read(reader),
                lower_background_color: get_editor_background_color(InlineCompletionEditorType::TextEditor),
                modified_background_color: MODIFIED_CHANGED_TEXT_OVERLAY_COLOR,
                font_family: font_family.read(reader),
                font_size: *font_size.read(reader),
                font_weight: font_weight.read(reader),
            }))
        })
        .recompute_initially_and_on_change(base.store(), None);

        let position = const_observable(Some(OverlayWidgetPosition {
            preference: Some(OverlayWidgetPlacement::Coordinates(OverlayWidgetPositionCoordinates { top: 0.0, left: 0.0 })),
            stack_ordinal: None,
        }));
        base.register(editor.create_overlay_widget(NonOverflowOverlayWidget::new(position, const_observable(0.0))));

        Rc::new(Self { base, visual, is_hovered: const_observable(false) })
    }

    /// O dado que o pintor nativo lê (`None` enquanto a posição não foi medida).
    pub fn visual(&self) -> IObservable<Option<Rc<WordInsertVisual>>> {
        Rc::clone(&self.visual)
    }
}

impl IInlineEditsView for InlineEditsWordInsertView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        Event::none()
    }
}

impl IDisposable for InlineEditsWordInsertView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
