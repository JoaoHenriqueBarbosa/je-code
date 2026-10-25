//! Porte de `src/vs/base/browser/ui/highlightedlabel/highlightedLabel.ts`: o [`HighlightedLabel`], um texto em
//! que trechos (os `matches` de um filtro difuso) ganham destaque, com ícones `$(id)` opcionais.
//!
//! Como o DOM vira `je-ui`:
//!
//! - O `span.monaco-highlighted-label` é um nó; cada trecho de texto é um nó filho de [`Content::Text`] e cada
//!   realce um `span.highlight` (um nó com as classes) com o texto dentro, como no `innerHTML` do original.
//!   Os filhos são posicionados por [`crate::icon_labels::layout_parts`] com a largura medida em
//!   [`crate::text::measure_text`].
//! - A regra de CSS do workbench (`.monaco-workbench .monaco-list .monaco-list-row .monaco-highlighted-label
//!   .highlight { color: var(--vscode-list-highlightForeground) }`) vira [`HighlightedLabelStyles`]: a cor do
//!   texto, a cor, o fundo e o peso do realce, o tamanho da fonte e a altura da linha. O nó cresce até a largura do
//!   texto (o `span` em linha) e a altura da linha; a posição fica com quem o contém. O fundo do realce
//!   (`.referenceMatch .highlight { background-color }` do peek) é um nó sob o texto do `span`, com a altura da área de
//!   conteúdo da fonte ([`crate::icon_labels::SpanPart::background`]).
//! - **Cor herdada.** Com [`HighlightedLabelStyles::inherit_foreground`] o texto não fixa a cor: herda a do antepassado
//!   ([`je_ui::Foreground::Inherit`]), como o CSS faz com o rótulo de uma linha selecionada; o realce com cor própria a
//!   declara, e sem cor própria herda a do texto.
//! - O `IManagedHover` do título vem do delegado de hover da camada de base ([`crate::hover_api`]).
//! - [`HighlightedLabel::set_max_width`] é o `overflow: hidden; text-overflow: ellipsis` do contêiner: o texto
//!   que passa da largura é cortado com reticências.
//!
//! Divergência: `escapeNewLines` do original muda os `highlights` do chamador no lugar; em [`HighlightedLabel::set`]
//! a lista chega por referência e a cópia é que muda (a função estática [`HighlightedLabel::escape_new_lines`]
//! muda no lugar, como o original).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_render::Rgba;
use je_theme::ColorTheme;
use je_ui::{NodeId, Role, Ui};

use crate::dom::{clear_node, create_el};
use crate::hover_api::{IHoverDelegate, ManagedHoverContent, ManagedHoverContentOrFactory, IManagedHover, HoverPlacement, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::icon_labels::{InlineStyle, LabelPart, Rendered, SpanPart, build_parts_with, layout_parts, render_label_with_icons, resize};
use crate::text::{UI_FONT_SIZE, natural_line_height, ui_font};

/// O `IHighlight`: o trecho `[start, end)` em unidades UTF-16 e as classes extras do realce.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct IHighlight {
    pub start: usize,
    pub end: usize,
    pub extra_classes: Vec<String>,
}

impl IHighlight {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end, extra_classes: Vec::new() }
    }
}

/// O `IHighlightedLabelOptions`.
#[derive(Clone, Default)]
pub struct IHighlightedLabelOptions {
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
}

/// O estilo de um [`HighlightedLabel`]: as regras de CSS que o original deixa para a folha de estilo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighlightedLabelStyles {
    /// A cor do texto (herdada do dono). Com [`HighlightedLabelStyles::inherit_foreground`], só vale quando nenhum antepassado
    /// declara cor.
    pub foreground: Rgba,
    /// A cor do `.highlight` (`--vscode-list-highlightForeground`); sem cor, o realce herda a do texto.
    pub highlight_foreground: Option<Rgba>,
    /// O `background-color` do `.highlight` (o `peekViewResult.matchHighlightBackground` do peek); sem cor, sem fundo.
    pub highlight_background: Option<Rgba>,
    /// O `font-weight` do `.highlight`.
    pub highlight_weight: u16,
    pub font_size: f32,
    pub font_weight: u16,
    /// O `line-height` herdado; sem valor, o da fonte (`normal`).
    pub line_height: Option<f64>,
    pub italic: bool,
    pub strikethrough: bool,
    /// O texto não declara cor: herda a do antepassado (`color` do CSS, [`je_ui::Foreground::Inherit`]), como o rótulo de
    /// uma linha de lista que muda de cor com a seleção. Falso: o texto pinta sempre com [`HighlightedLabelStyles::foreground`].
    pub inherit_foreground: bool,
}

/// O que o workbench dá a um rótulo numa lista: texto em `foreground`, realce em `list.highlightForeground`.
pub fn default_highlighted_label_styles(theme: &ColorTheme) -> HighlightedLabelStyles {
    HighlightedLabelStyles {
        foreground: theme.get_color("foreground").unwrap_or(Rgba::opaque(0xcc, 0xcc, 0xcc)),
        highlight_foreground: theme.get_color("list.highlightForeground"),
        highlight_background: None,
        highlight_weight: 400,
        font_size: UI_FONT_SIZE,
        font_weight: 400,
        line_height: None,
        italic: false,
        strikethrough: false,
        inherit_foreground: false,
    }
}

impl HighlightedLabelStyles {
    pub(crate) fn resolved_line_height(&self) -> f64 {
        self.line_height.unwrap_or_else(|| natural_line_height(&ui_font(self.font_size, self.font_weight)))
    }

    pub(crate) fn inline_style(&self) -> InlineStyle {
        InlineStyle {
            size_px: self.font_size,
            weight: self.font_weight,
            color: self.foreground,
            line_height: self.resolved_line_height(),
            italic: self.italic,
            strikethrough: self.strikethrough,
        }
    }
}

pub struct HighlightedLabel {
    base: Rc<Disposable>,
    ui: Ui,
    dom_node: NodeId,
    options: IHighlightedLabelOptions,
    styles: Cell<HighlightedLabelStyles>,
    text: RefCell<String>,
    title: RefCell<String>,
    highlights: RefCell<Vec<IHighlight>>,
    did_ever_render: Cell<bool>,
    support_icons: Cell<bool>,
    max_width: Cell<Option<f64>>,
    rendered: RefCell<Vec<Rendered>>,
    width: Cell<f64>,
    custom_hover: RefCell<Option<Rc<dyn IManagedHover>>>,
}

impl HighlightedLabel {
    /// Cria o rótulo como último filho de `container` (o `dom.append(container, dom.$('span.monaco-highlighted-label'))`).
    pub fn new(ui: &Ui, container: NodeId, options: Option<IHighlightedLabelOptions>, styles: HighlightedLabelStyles) -> Rc<Self> {
        let dom_node = create_el(&mut ui.tree_mut(), container, Role::Generic, "monaco-highlighted-label");
        let this = Rc::new(Self {
            base: Disposable::new(),
            ui: ui.clone(),
            dom_node,
            options: options.unwrap_or_default(),
            styles: Cell::new(styles),
            text: RefCell::new(String::new()),
            title: RefCell::new(String::new()),
            highlights: RefCell::new(Vec::new()),
            did_ever_render: Cell::new(false),
            support_icons: Cell::new(false),
            max_width: Cell::new(None),
            rendered: RefCell::new(Vec::new()),
            width: Cell::new(0.0),
            custom_hover: RefCell::new(None),
        });
        this.relayout();
        this
    }

    /// O elemento (o `get element()`).
    pub fn element(&self) -> NodeId {
        self.dom_node
    }

    /// A largura do texto como está (cortado, se há [`HighlightedLabel::set_max_width`]).
    pub fn width(&self) -> f64 {
        self.width.get()
    }

    /// A altura da caixa de linha.
    pub fn line_height(&self) -> f64 {
        self.styles.get().resolved_line_height()
    }

    /// O texto atual (depois do `escapeNewLines`).
    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    /// Muda o estilo e refaz os nós (o CSS de outra classe do dono mudando a cor ou o peso).
    pub fn set_styles(&self, styles: HighlightedLabelStyles) {
        if styles != self.styles.get() {
            self.styles.set(styles);
            if self.did_ever_render.get() {
                self.render(self.support_icons.get());
            } else {
                self.relayout();
            }
        }
    }

    /// A largura máxima da linha: o excedente é cortado com `…` (o `text-overflow: ellipsis` do contêiner).
    pub fn set_max_width(&self, max_width: Option<f64>) {
        if self.max_width.replace(max_width) != max_width {
            self.relayout();
        }
    }

    /// `set(text, highlights, title, escapeNewLines, supportIcons)`.
    pub fn set(&self, text: Option<&str>, highlights: &[IHighlight], title: &str, escape_new_lines: bool, support_icons: bool) {
        let mut text = text.unwrap_or_default().to_owned();
        let mut highlights = highlights.to_vec();
        if escape_new_lines {
            text = Self::escape_new_lines(&text, &mut highlights);
        }

        if self.did_ever_render.get() && *self.text.borrow() == text && *self.title.borrow() == title && *self.highlights.borrow() == highlights {
            return;
        }

        *self.text.borrow_mut() = text;
        *self.title.borrow_mut() = title.to_owned();
        *self.highlights.borrow_mut() = highlights;
        self.render(support_icons);
    }

    fn render(&self, support_icons: bool) {
        self.support_icons.set(support_icons);
        let styles = self.styles.get();
        let units: Vec<u16> = self.text.borrow().encode_utf16().collect();
        let sub = |start: usize, end: usize| String::from_utf16_lossy(&units[start.min(units.len())..end.min(units.len()).max(start.min(units.len()))]);
        let parts_of = |s: String| -> Vec<LabelPart> {
            if support_icons { render_label_with_icons(&s, true) } else { vec![LabelPart::Text(s)] }
        };

        let mut children: Vec<LabelPart> = Vec::new();
        let mut pos = 0;
        for highlight in self.highlights.borrow().iter() {
            if highlight.end == highlight.start {
                continue;
            }

            if pos < highlight.start {
                children.extend(parts_of(sub(pos, highlight.start)));
                pos = highlight.start;
            }

            let mut classes = vec!["highlight".to_owned()];
            classes.extend(highlight.extra_classes.iter().cloned());
            children.push(LabelPart::Span(SpanPart {
                class_names: classes,
                color: styles.highlight_foreground,
                background: styles.highlight_background,
                weight: Some(styles.highlight_weight),
                children: parts_of(sub(pos, highlight.end)),
                ..SpanPart::default()
            }));
            pos = highlight.end;
        }

        if pos < units.len() {
            children.extend(parts_of(sub(pos, units.len())));
        }

        {
            let mut tree = self.ui.tree_mut();
            clear_node(&mut tree, self.dom_node);
            *self.rendered.borrow_mut() = build_parts_with(&mut tree, self.dom_node, &children, &styles.inline_style(), styles.inherit_foreground);
        }
        self.relayout();

        let title = self.title.borrow().clone();
        let existing = self.custom_hover.borrow().clone();
        if existing.is_none() && !title.is_empty() {
            let delegate = self.options.hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Mouse));
            let hover = get_base_layer_hover_delegate().setup_managed_hover(delegate, self.dom_node, ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(title)), None);
            *self.custom_hover.borrow_mut() = Some(hover);
        } else if let Some(hover) = existing {
            hover.update(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(title)), None);
        }

        self.did_ever_render.set(true);
    }

    /// Posiciona os nós e dá ao `span` o tamanho do texto.
    fn relayout(&self) {
        let line_height = self.line_height();
        let mut tree = self.ui.tree_mut();
        let width = layout_parts(&mut tree, &self.rendered.borrow(), line_height, self.max_width.get());
        self.width.set(width);
        resize(&mut tree, self.dom_node, width, line_height);
    }

    /// `HighlightedLabel.escapeNewLines`: troca cada quebra de linha por `⏎` e ajusta os realces (muda
    /// `highlights` no lugar, como o original).
    pub fn escape_new_lines(text: &str, highlights: &mut [IHighlight]) -> String {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut out: Vec<u16> = Vec::with_capacity(units.len());
        let (mut total, mut i) = (0i64, 0usize);
        while i < units.len() {
            let (length, extra) = match (units[i], units.get(i + 1)) {
                (13, Some(10)) => (2, -1i64),
                (13 | 10, _) => (1, 0),
                (unit, _) => {
                    out.push(unit);
                    i += 1;
                    continue;
                }
            };
            let offset = i as i64 + total;
            for highlight in highlights.iter_mut() {
                if highlight.end as i64 <= offset {
                    continue;
                }
                if highlight.start as i64 >= offset {
                    highlight.start = (highlight.start as i64 + extra) as usize;
                }
                if highlight.end as i64 >= offset {
                    highlight.end = (highlight.end as i64 + extra) as usize;
                }
            }
            total += extra;
            out.push(0x23CE);
            i += length;
        }
        String::from_utf16_lossy(&out)
    }
}

impl IDisposable for HighlightedLabel {
    fn dispose(&self) {
        self.base.dispose();
        let hover = self.custom_hover.borrow_mut().take();
        if let Some(hover) = hover {
            hover.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "HighlightedLabel"
    }
}
