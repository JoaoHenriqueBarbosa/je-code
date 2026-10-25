//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollWidget.ts`: o widget de overlay que mostra no topo do
//! editor as linhas de início dos escopos que cruzam a janela visível (`StickyScrollWidget`), o estado que o controlador
//! entrega a ele (`StickyScrollWidgetState`) e as linhas já desenhadas (`RenderedStickyLine`, `StickyFoldingIcon`).
//!
//! Representação (o que é DOM vira dado que o pintor nativo lê):
//! - Os quatro nós de estrutura (`_rootDomNode`, `_lineNumbersDomNode`, `_linesDomNodeScrollable` e `_linesDomNode`) são o
//!   [`StickyScrollWidgetLayout`], lido por [`StickyScrollWidget::layout`]: o `display` do raiz é `visible`; as larguras e
//!   alturas de estilo são os campos de mesmo sentido; o `left` do `_linesDomNode` é `lines_left`; a variável
//!   `--vscode-editorStickyScroll-scrollableWidth` é `lines_scrollable_width`; a variável
//!   `--vscode-editorStickyScroll-foldingOpacityTransition` é `folding_opacity_transition_seconds`; e a classe `peek` do
//!   raiz (`editor instanceof EmbeddedCodeEditorWidget`) é `is_peek`.
//! - Cada linha desenhada é um [`RenderedStickyLine`]: o `innerHTML` do nó da linha é [`RenderedStickyLine::html`] (a saída de
//!   `renderViewLine`, que o pintor consome com o [`CharacterMapping`], como `view_line_renderer` descreve), o `style.top` e o
//!   `style.zIndex` dos dois nós (linha e número) são [`RenderedStickyLine::top`] e [`RenderedStickyLine::z_index`], e o
//!   `.remove()` do nó é o estado [`RenderedStickyLine::is_attached`] (uma linha removida não é mais pintada nem recebe
//!   foco). O ícone de dobra é o [`StickyFoldingIcon`], com o `cursor` e a `opacity` derivados de
//!   [`StickyFoldingIcon::is_visible`] (`pointer` e `1` quando visível, `default` e `0` quando não) e a classe do ícone em
//!   [`StickyFoldingIcon::icon`] (o id do codicon, fixado na criação, como o `className` do original).
//! - O texto do número de linha é [`RenderedStickyLine::line_number_text`] (o `innerText` do nó interno), com a largura e o
//!   recuo do nó interno em `inner_line_number_width` e `inner_line_number_padding_left`.
//! - **Nós como dados.** O original identifica o que o mouse acertou pelos atributos `data-sticky-*` do nó DOM e dos
//!   ancestrais. Aqui cada nó que o widget modela tem um [`ElementId`] (o mesmo tipo do `element` dos alvos do mouse em
//!   `editor_browser`), com a descrição em [`StickyNode`]: o nó da linha, o do número, o interno do número, o ícone de dobra
//!   e os `<span>` de texto que o pintor registra com [`StickyScrollWidget::register_text_span`] (só o pintor sabe quais
//!   spans a saída do `renderViewLine` virou). As consultas por nó (`getLineIndexFromChildDomNode`, `isInStickyLine`,
//!   `isInFoldingIconDomNode`, `getLineNumberFromChildDomNode` e `getEditorPositionFromNode`) recebem o `ElementId` e
//!   respondem como o `_getAttributeValue` responderia andando pelos ancestrais: o nó, e todo nó dentro dele, pertence à
//!   linha em que foi registrado. O `getColumnOfNodeOffset` (que conta os irmãos anteriores do nó) é o
//!   `sibling_index` do [`StickyNode`], que o widget fixa para os nós que cria e o pintor informa para os spans.
//! - `getCurrentLines()` e o getter `lineNumbers` devolvem a mesma lista; os dois são [`StickyScrollWidget::line_numbers`].
//! - `text-decoration: underline` do span sob o mouse com o modificador (feito pelo controlador) é
//!   [`StickyScrollWidget::set_text_underlined`], lido por [`StickyScrollWidget::is_text_underlined`].
//! - `lineDomNode.focus()` é o estado [`StickyScrollWidget::focused_line_index`], que o pintor lê para mover o foco; a linha
//!   precisa estar anexada (um nó solto não recebe foco).
//! - Os `mouseenter` e `mouseleave` do nó dos números (`_setFoldingHoverListeners`) são
//!   [`StickyScrollWidget::on_line_numbers_mouse_enter`] e [`StickyScrollWidget::on_line_numbers_mouse_leave`], que o pintor
//!   chama e que só valem enquanto `editor.showFoldingControls` era `mouseover` na última renderização (o ouvinte DOM só
//!   existe nesse caso).
//! - `editor.getScrollLeft()` e os demais eventos do editor são os mesmos do original; `FoldingModel` e `FoldingRegions` são
//!   os de `folding`.
//!
//! Divergências:
//! - **Largura da linha.** `lineHTMLNode.scrollWidth` lê o layout do DOM. Sem DOM, a largura inicial de
//!   [`RenderedStickyLine::scroll_width`] é o deslocamento horizontal da última coluna do `CharacterMapping` vezes a largura
//!   de um caractere de meia largura da fonte do editor (a conta de uma linha com otimizações monoespaçadas, que é como o
//!   original a renderiza: `useMonospaceOptimizations` verdadeiro), arredondada como o `scrollWidth`. O pintor que mede o
//!   texto reporta o valor real com [`RenderedStickyLine::set_scroll_width`].
//! - **Estilo e acessibilidade.** `editor.applyFontInfo(node)`, as classes (`sticky-widget`, `sticky-line-content`,
//!   `stickyLine<N>`...), os papéis (`role=list|listitem|none`), o `tabIndex` e a política `createTrustedTypesPolicy` são do
//!   pintor e do DOM; o widget entrega os dados de que eles precisam e nada mais.
//! - O `try { LineDecoration.filter(...) } catch { [] }` não existe: [`LineDecoration::filter`] não lança.
//! - `_renderRootNode` é `async` no original mas não tem `await`, então roda inteiro na chamada: é o método síncrono
//!   [`StickyScrollWidget::set_state`] por dentro. O `Math.max(...)` do `_minContentWidthInPx` lê as linhas da renderização
//!   anterior (a atribuição de `_renderedStickyLines` vem depois), e `Math.max()` de uma lista vazia é `-Infinity`; os dois
//!   comportamentos foram preservados.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};

use crate::code_editor_widget::{CodeEditorWidget, EditorViewModel, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::effective_options::{EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH, FullwidthCharacterWidth};
use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::config::editor_options::line_numbers::{LINE_NUMBERS, RenderLineNumbersType};
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::config::editor_options::simple_keys::{SHOW_FOLDING_CONTROLS, ShowFoldingControls};
use crate::config::editor_options::sticky_scroll::STICKY_SCROLL;
use crate::config::editor_options::EditorOption;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{
    ElementId, IOverlayWidget, OverlayWidgetPlacement, OverlayWidgetPosition, OverlayWidgetPositionPreference,
};
use crate::embedded_code_editor_widget::is_embedded_code_editor;
use crate::folding::folding_decorations::{FOLDING_COLLAPSED_ICON, FOLDING_EXPANDED_ICON};
use crate::folding::folding_model::FoldingModel;
use crate::line_decorations::LineDecoration;
use crate::multicursor::listen_weak;
use crate::position::Position;
use crate::view_line_renderer::{CharacterMapping, DomPosition, RenderLineInput, RenderLineInputOptions, render_view_line2};
use crate::view_model_types::IViewModel;

/// `getId()`.
pub const STICKY_SCROLL_WIDGET_ID: &str = "editor.contrib.stickyScrollWidget";

/// `this.domNode.style.width = '26px'` do ícone de dobra.
pub const STICKY_FOLDING_ICON_WIDTH: f64 = 26.0;

/// `StickyScrollWidgetState`.
#[derive(Clone, Debug, PartialEq)]
pub struct StickyScrollWidgetState {
    pub start_line_numbers: Vec<i32>,
    pub end_line_numbers: Vec<i32>,
    pub last_line_relative_position: f64,
    /// O índice (na lista de linhas) da linha cujo fim é mostrado (`showEndForLine`, `null` é `None`).
    pub show_end_for_line: Option<usize>,
}

impl StickyScrollWidgetState {
    pub fn new(
        start_line_numbers: Vec<i32>,
        end_line_numbers: Vec<i32>,
        last_line_relative_position: f64,
        show_end_for_line: Option<usize>,
    ) -> Self {
        Self { start_line_numbers, end_line_numbers, last_line_relative_position, show_end_for_line }
    }

    /// `equals(other)`.
    pub fn equals(&self, other: Option<&StickyScrollWidgetState>) -> bool {
        other.is_some_and(|other| {
            self.last_line_relative_position == other.last_line_relative_position
                && self.show_end_for_line == other.show_end_for_line
                && self.start_line_numbers == other.start_line_numbers
                && self.end_line_numbers == other.end_line_numbers
        })
    }

    /// `StickyScrollWidgetState.Empty`.
    pub fn empty() -> Self {
        Self::new(Vec::new(), Vec::new(), 0.0, None)
    }
}

/// O `{ height: number }` de `onDidChangeStickyScrollHeight`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickyScrollHeightChange {
    pub height: f64,
}

/// Os estilos dos nós de estrutura do widget (ver o topo do módulo).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StickyScrollWidgetLayout {
    /// `editor instanceof EmbeddedCodeEditorWidget` (a classe `peek` do raiz).
    pub is_peek: bool,
    /// `_rootDomNode.style.display` (`block` é `true`, `none` é `false`).
    pub visible: bool,
    /// A altura dos três nós (`style.height`), atualizada só quando o widget está visível.
    pub height: f64,
    /// `_rootDomNode.style.width`.
    pub root_width: f64,
    /// `_lineNumbersDomNode.style.width`.
    pub line_numbers_width: f64,
    /// `--vscode-editorStickyScroll-scrollableWidth` do `_linesDomNodeScrollable`.
    pub lines_scrollable_width: f64,
    /// `_linesDomNode.style.left`.
    pub lines_left: f64,
    /// A duração (`0.5s` ou `0s`) de `--vscode-editorStickyScroll-foldingOpacityTransition`.
    pub folding_opacity_transition_seconds: f64,
}

/// O que um [`ElementId`] do widget designa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickyNodeKind {
    /// O nó da linha (`data-sticky-is-line`).
    Line,
    /// O nó do número da linha (`data-sticky-is-line-number`).
    LineNumber,
    /// O `<span>` interno do número, que guarda o texto.
    LineNumberInner,
    /// O ícone de dobra (`data-sticky-is-folding-icon`).
    FoldingIcon,
    /// Um `<span>` do conteúdo da linha, registrado pelo pintor.
    TextSpan,
}

/// Um nó do widget e o que o original lê dele (ver o topo do módulo).
#[derive(Clone, Debug, PartialEq)]
pub struct StickyNode {
    pub kind: StickyNodeKind,
    /// O `data-sticky-line-index` herdado (o índice na lista de linhas do estado).
    pub line_index: usize,
    /// `children.length > 0`.
    pub has_children: bool,
    /// Quantos irmãos anteriores o nó tem (o `previousSibling` que `getColumnOfNodeOffset` percorre).
    pub sibling_index: u32,
    /// `innerText` (e `textContent`) do nó.
    pub text: JsString,
    /// `innerText === innerHTML`: o nó é um `<span>` que só tem o texto, sem entidades nem filhos.
    pub inner_text_is_inner_html: bool,
}

/// O ícone de dobra de uma linha (`StickyFoldingIcon`).
pub struct StickyFoldingIcon {
    pub element: ElementId,
    pub is_collapsed: Cell<bool>,
    pub folding_start_line: i32,
    pub folding_end_line: i32,
    /// A altura (e o `lineHeight`) do ícone: a altura da linha.
    pub dimension: f64,
    /// O `style.left` do ícone dentro do nó do número.
    pub left: f64,
    icon: &'static str,
    visible: Cell<bool>,
}

impl StickyFoldingIcon {
    fn new(element: ElementId, is_collapsed: bool, folding_start_line: i32, folding_end_line: i32, dimension: f64) -> Self {
        Self {
            element,
            is_collapsed: Cell::new(is_collapsed),
            folding_start_line,
            folding_end_line,
            dimension,
            left: 0.0,
            icon: if is_collapsed { FOLDING_COLLAPSED_ICON } else { FOLDING_EXPANDED_ICON },
            visible: Cell::new(false),
        }
    }

    /// O id do codicon (`ThemeIcon.asClassName(isCollapsed ? foldingCollapsedIcon : foldingExpandedIcon)` na criação).
    pub fn icon(&self) -> &'static str {
        self.icon
    }

    /// `setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        self.visible.set(visible);
    }

    /// `style.opacity === '1'` (e `style.cursor === 'pointer'`).
    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }
}

/// Um `<span>` de texto de uma linha registrado pelo pintor.
struct RegisteredSpan {
    element: ElementId,
    node: StickyNode,
}

/// A linha sticky desenhada (`RenderedStickyLine`).
pub struct RenderedStickyLine {
    /// O `data-sticky-line-index`.
    pub index: usize,
    pub line_number: i32,
    pub line_element: ElementId,
    pub line_number_element: ElementId,
    pub line_number_inner_element: ElementId,
    /// O `innerHTML` do nó da linha: a saída de `renderViewLine`.
    pub html: JsString,
    pub character_mapping: CharacterMapping,
    /// O `innerText` do nó interno do número (vazio quando o editor não mostra o número).
    pub line_number_text: String,
    /// `lineNumberHTMLNode.style.width` (`layoutInfo.contentLeft`).
    pub line_number_width: f64,
    /// `innerLineNumberHTML.style.width` (`layoutInfo.lineNumbersWidth`).
    pub inner_line_number_width: f64,
    /// `innerLineNumberHTML.style.paddingLeft` (`layoutInfo.lineNumbersLeft`).
    pub inner_line_number_padding_left: f64,
    pub folding_icon: Option<StickyFoldingIcon>,
    /// A altura da linha (`lineHeight`, `style.height` e `style.lineHeight` dos dois nós).
    pub height: f64,
    scroll_width: Cell<f64>,
    top: Cell<f64>,
    z_index: Cell<i32>,
    attached: Cell<bool>,
    text_spans: RefCell<Vec<RegisteredSpan>>,
}

impl RenderedStickyLine {
    /// `new RenderedStickyLine(editor, viewModel, layoutInfo, foldingModel, isOnGlyphMargin, index, lineNumber)`.
    #[allow(clippy::too_many_arguments)]
    fn new(
        editor: &Rc<CodeEditorWidget>,
        view_model: &Rc<EditorViewModel>,
        layout_info: &EditorLayoutInfo,
        folding_model: Option<&Rc<FoldingModel>>,
        is_on_glyph_margin: bool,
        index: usize,
        line_number: i32,
        elements: &ElementAllocator,
    ) -> Self {
        let view_line_number = view_model
            .coordinates_converter()
            .convert_model_position_to_view_position(Position::new(line_number, 1), None, None, None)
            .line_number;
        let line_rendering_data = view_model.get_view_line_rendering_data(view_line_number);
        let line_number_option = editor.get_option(LINE_NUMBERS).render_type;
        let vertical_scrollbar_size = editor.get_option(SCROLLBAR).vertical_scrollbar_size;

        let actual_inline_decorations = LineDecoration::filter(
            &line_rendering_data.inline_decorations,
            view_line_number,
            line_rendering_data.min_column,
            line_rendering_data.max_column,
        );

        let line_height = editor.get_line_height_for_position(Position::new(line_number, 1));
        let text_direction = view_model.get_text_direction(line_number);
        let font_info = editor.get_option(FONT_INFO);
        let render_line_input = RenderLineInput::new(RenderLineInputOptions {
            use_monospace_optimizations: true,
            can_use_halfwidth_rightwards_arrow: true,
            line_content: line_rendering_data.content.clone(),
            continues_with_wrapped_line: line_rendering_data.continues_with_wrapped_line,
            is_basic_ascii: line_rendering_data.is_basic_ascii,
            contains_rtl: line_rendering_data.contains_rtl,
            faux_indent_length: 0,
            line_tokens: Rc::clone(&line_rendering_data.tokens),
            line_decorations: actual_inline_decorations,
            tab_size: line_rendering_data.tab_size,
            start_visible_column: line_rendering_data.start_visible_column,
            space_width: font_info.space_width,
            middot_width: 1.0,
            wsmiddot_width: 1.0,
            stop_rendering_line_after: 500,
            render_whitespace: "none".to_owned(),
            render_control_characters: true,
            font_ligatures: true,
            selections_on_line: None,
            text_direction: Some(text_direction),
            vertical_scrollbar_size,
            render_new_line_when_empty: false,
            use_two_cell_fullwidth_characters: *editor.get_option(EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH)
                == FullwidthCharacterWidth::TwoCells,
        });

        let render_output = render_view_line2(&render_line_input);
        let character_mapping = render_output.character_mapping;

        let line_number_text = if line_number_option == RenderLineNumbersType::On
            || (line_number_option == RenderLineNumbersType::Interval && line_number % 10 == 0)
        {
            line_number.to_string()
        } else if line_number_option == RenderLineNumbersType::Relative {
            let position = editor.get_position().expect("o editor com modelo tem posição");
            (line_number - position.line_number).abs().to_string()
        } else {
            String::new()
        };

        let mut folding_icon =
            Self::render_folding_icon_for_line(editor, folding_model, line_number, line_height, is_on_glyph_margin, elements);
        if let Some(folding_icon) = folding_icon.as_mut() {
            folding_icon.left = layout_info.line_numbers_width + layout_info.line_numbers_left;
        }

        // `lineHTMLNode.scrollWidth` (ver "Divergências" no topo do módulo).
        let scroll_width = (f64::from(character_mapping.get_horizontal_offset(character_mapping.length as i32))
            * font_info.typical_halfwidth_character_width)
            .round();

        Self {
            index,
            line_number,
            line_element: elements.next(),
            line_number_element: elements.next(),
            line_number_inner_element: elements.next(),
            html: render_output.html,
            character_mapping,
            line_number_text,
            line_number_width: layout_info.content_left,
            inner_line_number_width: layout_info.line_numbers_width,
            inner_line_number_padding_left: layout_info.line_numbers_left,
            folding_icon,
            height: line_height,
            scroll_width: Cell::new(scroll_width),
            top: Cell::new(0.0),
            z_index: Cell::new(0),
            attached: Cell::new(true),
            text_spans: RefCell::new(Vec::new()),
        }
    }

    /// `_renderFoldingIconForLine(editor, foldingModel, line, lineHeight, isOnGlyphMargin)`.
    fn render_folding_icon_for_line(
        editor: &Rc<CodeEditorWidget>,
        folding_model: Option<&Rc<FoldingModel>>,
        line: i32,
        line_height: f64,
        is_on_glyph_margin: bool,
        elements: &ElementAllocator,
    ) -> Option<StickyFoldingIcon> {
        let show_folding_controls = *editor.get_option(SHOW_FOLDING_CONTROLS);
        let folding_model = folding_model?;
        if show_folding_controls == ShowFoldingControls::Never {
            return None;
        }
        let folding_regions = folding_model.regions();
        // `findRange` devolve `-1` sem região; o `getStartLineNumber(-1)` do original lê um índice inexistente e vale `0`,
        // que nunca é uma linha (as linhas começam em 1).
        let index_of_folding_region = usize::try_from(folding_regions.find_range(line as u32)).ok()?;
        let start_line_number = folding_regions.get_start_line_number(index_of_folding_region);
        let is_folding_scope = line as u32 == start_line_number;
        if !is_folding_scope {
            return None;
        }
        let is_collapsed = folding_regions.is_collapsed(index_of_folding_region);
        let folding_icon = StickyFoldingIcon::new(
            elements.next(),
            is_collapsed,
            start_line_number as i32,
            folding_regions.get_end_line_number(index_of_folding_region) as i32,
            line_height,
        );
        folding_icon.set_visible(if is_on_glyph_margin { true } else { is_collapsed || show_folding_controls == ShowFoldingControls::Always });
        Some(folding_icon)
    }

    /// `style.top` dos nós da linha e do número.
    pub fn top(&self) -> f64 {
        self.top.get()
    }

    /// `style.zIndex` dos nós da linha e do número.
    pub fn z_index(&self) -> i32 {
        self.z_index.get()
    }

    /// `lineHTMLNode.scrollWidth`.
    pub fn scroll_width(&self) -> f64 {
        self.scroll_width.get()
    }

    /// O valor medido pelo pintor para `scrollWidth` (ver "Divergências" no topo do módulo).
    pub fn set_scroll_width(&self, scroll_width: f64) {
        self.scroll_width.set(scroll_width);
    }

    /// O nó ainda está na árvore (não passou por `.remove()`).
    pub fn is_attached(&self) -> bool {
        self.attached.get()
    }

    /// O nó da linha que o `ElementId` designa, se for um dos desta linha.
    fn node(&self, element: ElementId) -> Option<StickyNode> {
        let make = |kind: StickyNodeKind, has_children: bool, sibling_index: u32, text: JsString, inner_text_is_inner_html: bool| {
            Some(StickyNode { kind, line_index: self.index, has_children, sibling_index, text, inner_text_is_inner_html })
        };
        if element == self.line_element {
            return make(StickyNodeKind::Line, true, 0, JsString::default(), false);
        }
        if element == self.line_number_element {
            return make(StickyNodeKind::LineNumber, true, 0, JsString::default(), false);
        }
        if element == self.line_number_inner_element {
            return make(StickyNodeKind::LineNumberInner, false, 0, JsString::from(self.line_number_text.as_str()), true);
        }
        if self.folding_icon.as_ref().is_some_and(|icon| icon.element == element) {
            // O ícone vem depois do `<span>` interno do número.
            return make(StickyNodeKind::FoldingIcon, false, 1, JsString::default(), true);
        }
        self.text_spans.borrow().iter().find(|span| span.element == element).map(|span| span.node.clone())
    }
}

/// Os identificadores de elemento de um widget. O prefixo (um por widget) mantém os ids de dois widgets distintos
/// disjuntos, para um `ElementId` de outro widget nunca ser lido como nó deste.
struct ElementAllocator {
    prefix: u64,
    next: Cell<u64>,
}

thread_local! {
    /// O próximo prefixo de [`ElementAllocator`].
    static NEXT_WIDGET_SERIAL: Cell<u64> = const { Cell::new(0) };
}

impl ElementAllocator {
    fn new() -> Self {
        let serial = NEXT_WIDGET_SERIAL.with(|serial| {
            serial.set(serial.get() + 1);
            serial.get()
        });
        Self { prefix: serial << 32, next: Cell::new(0) }
    }

    fn next(&self) -> ElementId {
        self.next.set(self.next.get() + 1);
        ElementId(self.prefix | self.next.get())
    }
}

/// O que `_findRenderingData` devolve.
struct RenderingData {
    line_numbers: Vec<i32>,
    last_line_relative_position: f64,
}

/// `StickyScrollWidget`.
pub struct StickyScrollWidget {
    me: Weak<StickyScrollWidget>,
    base: Rc<Disposable>,
    folding_icon_store: Rc<DisposableStore>,
    editor: Weak<CodeEditorWidget>,
    elements: ElementAllocator,

    state: RefCell<Option<StickyScrollWidgetState>>,
    rendered_sticky_lines: RefCell<Vec<Rc<RenderedStickyLine>>>,
    line_numbers: RefCell<Vec<i32>>,
    last_line_relative_position: Cell<f64>,
    min_content_width_in_px: Cell<f64>,
    is_on_glyph_margin: Cell<bool>,
    height: Cell<f64>,
    layout: Cell<StickyScrollWidgetLayout>,
    folding_hover_active: Cell<bool>,
    focused_line_index: Cell<Option<usize>>,
    underlined_elements: RefCell<HashSet<ElementId>>,

    on_did_change_sticky_scroll_height: Rc<Emitter<StickyScrollHeightChange>>,
}

impl StickyScrollWidget {
    /// `new StickyScrollWidget(editor)`.
    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        let base = Disposable::new();
        let folding_icon_store = DisposableStore::new();
        base.register(folding_icon_store.clone());
        let on_did_change_sticky_scroll_height = Emitter::new();
        base.register(on_did_change_sticky_scroll_height.clone());

        let this = Rc::new_cyclic(|me: &Weak<StickyScrollWidget>| {
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &StickyScrollWidget, e| {
                if e.has_changed(EditorOption::StickyScroll) {
                    this.update_scroll_left_position();
                }
            }));
            base.register(listen_weak(me, editor.on_did_scroll_change(), |this: &StickyScrollWidget, e| {
                if e.scroll_left_changed {
                    this.update_scroll_left_position();
                }
                if e.scroll_width_changed {
                    this.update_widget_width();
                }
            }));
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &StickyScrollWidget, _| {
                this.update_scroll_left_position();
                this.update_widget_width();
            }));
            base.register(listen_weak(me, editor.on_did_layout_change(), |this: &StickyScrollWidget, _| {
                this.update_widget_width();
            }));

            StickyScrollWidget {
                me: me.clone(),
                base: base.clone(),
                folding_icon_store,
                editor: Rc::downgrade(editor),
                elements: ElementAllocator::new(),
                state: RefCell::new(None),
                rendered_sticky_lines: RefCell::new(Vec::new()),
                line_numbers: RefCell::new(Vec::new()),
                last_line_relative_position: Cell::new(0.0),
                min_content_width_in_px: Cell::new(0.0),
                is_on_glyph_margin: Cell::new(false),
                height: Cell::new(-1.0),
                layout: Cell::new(StickyScrollWidgetLayout { is_peek: is_embedded_code_editor(editor), ..StickyScrollWidgetLayout::default() }),
                folding_hover_active: Cell::new(false),
                focused_line_index: Cell::new(None),
                underlined_elements: RefCell::new(HashSet::new()),
                on_did_change_sticky_scroll_height,
            }
        });
        this.set_height(0.0);
        this.update_scroll_left_position();
        this.update_widget_width();
        this
    }

    /// `height`.
    pub fn height(&self) -> f64 {
        self.height.get()
    }

    /// `onDidChangeStickyScrollHeight`.
    pub fn on_did_change_sticky_scroll_height(&self) -> Event<StickyScrollHeightChange> {
        self.on_did_change_sticky_scroll_height.event()
    }

    /// Os estilos dos nós de estrutura (ver o topo do módulo).
    pub fn layout(&self) -> StickyScrollWidgetLayout {
        self.layout.get()
    }

    fn update_layout(&self, update: impl FnOnce(&mut StickyScrollWidgetLayout)) {
        let mut layout = self.layout.get();
        update(&mut layout);
        self.layout.set(layout);
    }

    /// `lineNumbers` e `getCurrentLines()`.
    pub fn line_numbers(&self) -> Vec<i32> {
        self.line_numbers.borrow().clone()
    }

    /// `lineNumberCount`.
    pub fn line_number_count(&self) -> usize {
        self.line_numbers.borrow().len()
    }

    /// As linhas desenhadas e anexadas, na ordem em que aparecem (o que o pintor desenha).
    pub fn rendered_lines(&self) -> Vec<Rc<RenderedStickyLine>> {
        self.rendered_sticky_lines.borrow().iter().filter(|line| line.is_attached()).cloned().collect()
    }

    /// `getRenderedStickyLine(lineNumber)`.
    pub fn get_rendered_sticky_line(&self, line_number: i32) -> Option<Rc<RenderedStickyLine>> {
        self.rendered_sticky_lines.borrow().iter().find(|sticky_line| sticky_line.line_number == line_number).cloned()
    }

    /// `setState(state, foldingModel, rebuildFromIndexCandidate)`.
    pub fn set_state(
        &self,
        state: Option<StickyScrollWidgetState>,
        folding_model: Option<Rc<FoldingModel>>,
        rebuild_from_index_candidate: Option<usize>,
    ) {
        let current_state_and_previous_state_undefined = self.state.borrow().is_none() && state.is_none();
        let current_state_defined_and_equals_previous_state =
            self.state.borrow().as_ref().is_some_and(|previous| previous.equals(state.as_ref()));
        if rebuild_from_index_candidate.is_none()
            && (current_state_and_previous_state_undefined || current_state_defined_and_equals_previous_state)
        {
            return;
        }
        let data = self.find_rendering_data(state.as_ref());
        let previous_line_numbers = self.line_numbers.replace(data.line_numbers.clone());
        self.last_line_relative_position.set(data.last_line_relative_position);
        let rebuild_from_index = Self::find_index_to_rebuild_from(&previous_line_numbers, &data.line_numbers, rebuild_from_index_candidate);
        self.render_root_node(&data.line_numbers, data.last_line_relative_position, folding_model.as_ref(), rebuild_from_index);
        *self.state.borrow_mut() = state;
    }

    /// `_findRenderingData(state)`.
    fn find_rendering_data(&self, state: Option<&StickyScrollWidgetState>) -> RenderingData {
        let empty = || RenderingData { line_numbers: Vec::new(), last_line_relative_position: 0.0 };
        let Some(state) = state else {
            return empty();
        };
        let mut candidate_line_numbers = state.start_line_numbers.clone();
        if let Some(show_end_for_line) = state.show_end_for_line
            && let (Some(slot), Some(end_line_number)) =
                (candidate_line_numbers.get_mut(show_end_for_line), state.end_line_numbers.get(show_end_for_line))
        {
            *slot = *end_line_number;
        }
        let Some(editor) = self.editor.upgrade() else {
            return empty();
        };
        let mut total_height = 0.0;
        for candidate_line_number in &candidate_line_numbers {
            let position = Position::new(*candidate_line_number, 1);
            let view_model = editor.get_view_model();
            if view_model.is_some_and(|view_model| position.line_number <= view_model.get_line_count()) {
                total_height += editor.get_line_height_for_position(Position::new(*candidate_line_number, 1));
            }
        }
        if total_height == 0.0 {
            return empty();
        }
        RenderingData { line_numbers: candidate_line_numbers, last_line_relative_position: state.last_line_relative_position }
    }

    /// `_findIndexToRebuildFrom(previousLineNumbers, newLineNumbers, rebuildFromIndexCandidate)`.
    fn find_index_to_rebuild_from(
        previous_line_numbers: &[i32],
        new_line_numbers: &[i32],
        rebuild_from_index_candidate: Option<usize>,
    ) -> usize {
        if new_line_numbers.is_empty() {
            return 0;
        }
        if let Some(rebuild_from_index_candidate) = rebuild_from_index_candidate {
            return rebuild_from_index_candidate;
        }
        new_line_numbers.iter().position(|start_line_number| !previous_line_numbers.contains(start_line_number)).unwrap_or(0)
    }

    /// `updateScrollLeftPosition`: o `left` de `_linesDomNode`.
    fn update_scroll_left_position(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let lines_left = if editor.get_option(STICKY_SCROLL).scroll_with_editor { -editor.get_scroll_left() } else { 0.0 };
        self.update_layout(|layout| layout.lines_left = lines_left);
    }

    /// `_updateWidgetWidth()`.
    fn update_widget_width(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let layout_info = editor.get_layout_info();
        let line_numbers_width = layout_info.content_left;
        let lines_scrollable_width = editor.get_scroll_width() - layout_info.vertical_scrollbar_width;
        let root_width = layout_info.width - layout_info.vertical_scrollbar_width;
        self.update_layout(|layout| {
            layout.line_numbers_width = line_numbers_width;
            layout.lines_scrollable_width = lines_scrollable_width;
            layout.root_width = root_width;
        });
    }

    /// `_useFoldingOpacityTransition(requireTransitions)`.
    fn use_folding_opacity_transition(&self, require_transitions: bool) {
        self.update_layout(|layout| layout.folding_opacity_transition_seconds = if require_transitions { 0.5 } else { 0.0 });
    }

    /// `_setFoldingIconsVisibility(allVisible)`.
    fn set_folding_icons_visibility(&self, all_visible: bool) {
        for line in self.rendered_sticky_lines.borrow().iter() {
            let Some(folding_icon) = line.folding_icon.as_ref() else {
                continue;
            };
            folding_icon.set_visible(if all_visible { true } else { folding_icon.is_collapsed.get() });
        }
    }

    /// `_renderRootNode(lineNumbers, lastLineRelativePosition, foldingModel, rebuildFromIndex)`.
    fn render_root_node(
        &self,
        line_numbers: &[i32],
        last_line_relative_position: f64,
        folding_model: Option<&Rc<FoldingModel>>,
        rebuild_from_index: usize,
    ) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(view_model) = editor.get_view_model() else {
            self.clear_widget();
            return;
        };
        let Some(&last_line_number) = line_numbers.last() else {
            self.clear_widget();
            return;
        };
        let previous_lines = self.rendered_sticky_lines.borrow().clone();
        let mut rendered_sticky_lines: Vec<Rc<RenderedStickyLine>> = Vec::new();
        let mut top = 0.0;
        for (i, rendered_line) in previous_lines.iter().enumerate() {
            if i < rebuild_from_index {
                self.update_position(rendered_line, top, rendered_line.line_number == last_line_number);
                top += rendered_line.height;
                rendered_sticky_lines.push(Rc::clone(rendered_line));
            } else {
                rendered_line.attached.set(false);
            }
        }
        let layout_info = editor.get_layout_info();
        for (i, &line_number) in line_numbers.iter().enumerate().skip(rebuild_from_index) {
            if line_number > view_model.get_line_count() {
                continue;
            }
            let sticky_line = self.render_child_node(
                &editor,
                &view_model,
                i,
                line_number,
                top,
                last_line_number == line_number,
                folding_model,
                &layout_info,
            );
            top += sticky_line.height;
            rendered_sticky_lines.push(sticky_line);
        }
        if folding_model.is_some() {
            self.set_folding_hover_listeners();
            self.use_folding_opacity_transition(!self.is_on_glyph_margin.get());
        }
        self.min_content_width_in_px.set(
            previous_lines.iter().map(|line| line.scroll_width()).fold(f64::NEG_INFINITY, f64::max) + layout_info.vertical_scrollbar_width,
        );
        *self.rendered_sticky_lines.borrow_mut() = rendered_sticky_lines;
        self.set_height(top + last_line_relative_position);
        if let Some(widget) = self.as_overlay_widget() {
            editor.layout_overlay_widget(&widget);
        }
    }

    /// `this` como `Rc<dyn IOverlayWidget>`.
    fn as_overlay_widget(&self) -> Option<Rc<dyn IOverlayWidget>> {
        let widget: Rc<StickyScrollWidget> = self.me.upgrade()?;
        Some(widget)
    }

    /// `_clearWidget()`.
    fn clear_widget(&self) {
        for sticky_line in self.rendered_sticky_lines.borrow().iter() {
            sticky_line.attached.set(false);
        }
        self.set_height(0.0);
    }

    /// `_setHeight(height)`.
    fn set_height(&self, height: f64) {
        if self.height.get() == height {
            return;
        }
        self.height.set(height);

        self.update_layout(|layout| {
            if height == 0.0 {
                layout.visible = false;
            } else {
                layout.visible = true;
                layout.height = height;
            }
        });

        self.on_did_change_sticky_scroll_height.fire(StickyScrollHeightChange { height });
    }

    /// `_setFoldingHoverListeners()`.
    fn set_folding_hover_listeners(&self) {
        self.folding_icon_store.clear();
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        if *editor.get_option(SHOW_FOLDING_CONTROLS) != ShowFoldingControls::Mouseover {
            return;
        }
        self.folding_hover_active.set(true);
        let me = self.me.clone();
        self.folding_icon_store.add(to_disposable(move || {
            if let Some(this) = me.upgrade() {
                this.folding_hover_active.set(false);
            }
        }));
    }

    /// O `MOUSE_ENTER` do nó dos números de linha (só vale com `showFoldingControls` igual a `mouseover`).
    pub fn on_line_numbers_mouse_enter(&self) {
        if !self.folding_hover_active.get() {
            return;
        }
        self.is_on_glyph_margin.set(true);
        self.set_folding_icons_visibility(true);
    }

    /// O `MOUSE_LEAVE` do nó dos números de linha (só vale com `showFoldingControls` igual a `mouseover`).
    pub fn on_line_numbers_mouse_leave(&self) {
        if !self.folding_hover_active.get() {
            return;
        }
        self.is_on_glyph_margin.set(false);
        self.use_folding_opacity_transition(true);
        self.set_folding_icons_visibility(false);
    }

    /// `_renderChildNode(viewModel, index, line, top, isLastLine, foldingModel, layoutInfo)`.
    #[allow(clippy::too_many_arguments)]
    fn render_child_node(
        &self,
        editor: &Rc<CodeEditorWidget>,
        view_model: &Rc<EditorViewModel>,
        index: usize,
        line: i32,
        top: f64,
        is_last_line: bool,
        folding_model: Option<&Rc<FoldingModel>>,
        layout_info: &EditorLayoutInfo,
    ) -> Rc<RenderedStickyLine> {
        let rendered_line = Rc::new(RenderedStickyLine::new(
            editor,
            view_model,
            layout_info,
            folding_model,
            self.is_on_glyph_margin.get(),
            index,
            line,
            &self.elements,
        ));
        self.update_position(&rendered_line, top, is_last_line);
        rendered_line
    }

    /// `_updatePosition(stickyLine, top, isLastLine)`.
    fn update_position(&self, sticky_line: &RenderedStickyLine, top: f64, is_last_line: bool) {
        if is_last_line {
            sticky_line.z_index.set(0);
            let is_collapsed = sticky_line.folding_icon.as_ref().is_some_and(|folding_icon| folding_icon.is_collapsed.get());
            sticky_line.top.set(top + self.last_line_relative_position.get() + if is_collapsed { 1.0 } else { 0.0 });
        } else {
            sticky_line.z_index.set(1);
            sticky_line.top.set(top);
        }
    }

    /// `focusLineWithIndex(index)`: um índice fora da lista (inclusive `-1`) não faz nada.
    pub fn focus_line_with_index(&self, index: i64) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if self.rendered_sticky_lines.borrow().get(index).is_some_and(|line| line.is_attached()) {
            self.focused_line_index.set(Some(index));
        }
    }

    /// O índice da linha que recebeu `focus()` por último (a que o pintor deve focar), ou `None`.
    pub fn focused_line_index(&self) -> Option<usize> {
        self.focused_line_index.get()
    }

    /// O pintor consumiu o pedido de foco de [`StickyScrollWidget::focused_line_index`].
    pub fn clear_focused_line_index(&self) {
        self.focused_line_index.set(None);
    }

    /// Registra um `<span>` de texto do conteúdo da linha de índice `line_index` e devolve o identificador dele (ou `None`
    /// se a linha não está anexada). O pintor chama ao montar a saída de `renderViewLine`: `sibling_index` é a posição do
    /// span entre os irmãos (o `spanIndex` de `getColumnOfNodeOffset`), `text` o `innerText` (e `textContent`),
    /// `has_children` se tem elementos filhos e `inner_text_is_inner_html` se o `innerText` é igual ao `innerHTML` (sem
    /// entidades como `&nbsp;`). Registrar de novo o mesmo span devolve o mesmo identificador.
    pub fn register_text_span(
        &self,
        line_index: usize,
        sibling_index: u32,
        text: JsString,
        has_children: bool,
        inner_text_is_inner_html: bool,
    ) -> Option<ElementId> {
        let lines = self.rendered_sticky_lines.borrow();
        let line = lines.iter().find(|line| line.is_attached() && line.index == line_index)?;
        let mut spans = line.text_spans.borrow_mut();
        let node = StickyNode { kind: StickyNodeKind::TextSpan, line_index, has_children, sibling_index, text, inner_text_is_inner_html };
        if let Some(existing) = spans.iter_mut().find(|span| span.node.sibling_index == sibling_index) {
            existing.node = node;
            return Some(existing.element);
        }
        let element = self.elements.next();
        spans.push(RegisteredSpan { element, node });
        Some(element)
    }

    /// O nó do widget que o identificador designa (só os das linhas anexadas).
    pub fn get_node(&self, element: ElementId) -> Option<StickyNode> {
        self.rendered_sticky_lines.borrow().iter().filter(|line| line.is_attached()).find_map(|line| line.node(element))
    }

    /// `style.textDecoration = 'underline'` (ou `'none'`) do span.
    pub fn set_text_underlined(&self, element: ElementId, underlined: bool) {
        if underlined {
            self.underlined_elements.borrow_mut().insert(element);
        } else {
            self.underlined_elements.borrow_mut().remove(&element);
        }
    }

    /// `style.textDecoration === 'underline'`.
    pub fn is_text_underlined(&self, element: ElementId) -> bool {
        self.underlined_elements.borrow().contains(&element)
    }

    /// `getEditorPositionFromNode(spanDomNode)`: dado um nó folha, tenta achar a posição no editor.
    pub fn get_editor_position_from_node(&self, element: Option<ElementId>) -> Option<Position> {
        let node = self.get_node(element?)?;
        if node.has_children {
            // This is not a leaf node
            return None;
        }
        let rendered_sticky_line = self.get_rendered_sticky_line_from_child_node(&node)?;
        // `getColumnOfNodeOffset(characterMapping, spanNode, 0)`.
        let column = rendered_sticky_line
            .character_mapping
            .get_column(&DomPosition::new(node.sibling_index, 0), node.text.len() as u32);
        Some(Position::new(rendered_sticky_line.line_number, column))
    }

    /// `getLineNumberFromChildDomNode(domNode)`.
    pub fn get_line_number_from_child_element(&self, element: Option<ElementId>) -> Option<i32> {
        let node = self.get_node(element?)?;
        self.get_rendered_sticky_line_from_child_node(&node).map(|line| line.line_number)
    }

    /// `_getRenderedStickyLineFromChildDomNode(domNode)`: a linha na posição do índice que o nó guarda.
    fn get_rendered_sticky_line_from_child_node(&self, node: &StickyNode) -> Option<Rc<RenderedStickyLine>> {
        self.rendered_sticky_lines.borrow().get(node.line_index).cloned()
    }

    /// `getLineIndexFromChildDomNode(domNode)`: o `data-sticky-line-index` do nó (ou do ancestral que o tem).
    pub fn get_line_index_from_child_element(&self, element: Option<ElementId>) -> Option<usize> {
        self.get_node(element?).map(|node| node.line_index)
    }

    /// `isInStickyLine(domNode)`: o nó é (ou está dentro de) uma linha sticky.
    pub fn is_in_sticky_line(&self, element: Option<ElementId>) -> bool {
        element.and_then(|element| self.get_node(element)).is_some_and(|node| matches!(node.kind, StickyNodeKind::Line | StickyNodeKind::TextSpan))
    }

    /// `isInFoldingIconDomNode(domNode)`: o nó é o ícone de dobra de uma linha sticky.
    pub fn is_in_folding_icon_element(&self, element: Option<ElementId>) -> bool {
        element.and_then(|element| self.get_node(element)).is_some_and(|node| node.kind == StickyNodeKind::FoldingIcon)
    }
}

impl IOverlayWidget for StickyScrollWidget {
    fn get_id(&self) -> String {
        STICKY_SCROLL_WIDGET_ID.to_owned()
    }

    fn get_position(&self) -> Option<OverlayWidgetPosition> {
        Some(OverlayWidgetPosition {
            preference: Some(OverlayWidgetPlacement::Preference(OverlayWidgetPositionPreference::TopCenter)),
            stack_ordinal: Some(10.0),
        })
    }

    fn get_min_content_width_in_px(&self) -> Option<f64> {
        Some(self.min_content_width_in_px.get())
    }
}

impl IDisposable for StickyScrollWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}
