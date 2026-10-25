//! A tradução nativa do Markdown renderizado (`IRenderedMarkdown.element`, a árvore sanitizada de [`Element`] que
//! `je_base::markdown::render_markdown` entrega) para nós do `je-ui`, que o template do editor de configurações usa para a
//! descrição e para a mensagem de depreciação (`renderSettingMarkdown` de `settingsTree.ts`). No VS Code isso é o DOM do
//! navegador com o CSS de `.setting-item-markdown`; aqui é um fluxo de linhas calculado no [`MarkdownView::layout`].
//!
//! O porte de `IMarkdownRendererService` (a camada que dá ao `renderMarkdown` o ambiente e escolhe os widgets) ainda não
//! existe na plataforma; quando existir, este módulo migra para `je-widgets` como o mapeamento "árvore para widgets" que o
//! `//!` de `je_base::markdown` deixou em aberto, e o [`OwnedMarkdownEnvironment`] sai daqui.
//!
//! Do CSS de `settingsEditor2.css` (`.setting-item-markdown`) para a geometria:
//! - `* { margin: 0 }` e `*:not(:last-child) { margin-bottom: 8px }`: cada bloco (parágrafo, item de lista, título, citação,
//!   bloco de código) fica a 8 px do seguinte, e o último não tem margem.
//! - `a`: cor `textLink.foreground`, `textLink.activeForeground` sob o ponteiro, cursor de mão. Um `a` é um nó por palavra (o
//!   texto quebra em linhas), todos com o mesmo destino; Click, Space e Enter chamam [`RenderedMarkdown::activate_link`] com o
//!   `data-href`, como o `renderMarkdown` do original faz com os ouvintes de clique e de teclado.
//! - `code`: fonte monoespaçada de 11 px, linha de 15 px, `padding: 1px 3px`, raio de 4 px e borda de 1 px
//!   (`textPreformat.*`), numa caixa que não quebra.
//! - `ul` e `ol` têm o recuo padrão do navegador (40 px) e o marcador fora do conteúdo; a citação (`blockquote`) recua 15 px e
//!   ganha uma barra de 3 px à esquerda de cada um dos blocos dela.
//!
//! Divergências:
//! - **Fluxo.** Não há motor de CSS: o texto é medido por palavra com [`measure_text`] e quebrado na largura dada, e uma
//!   linha só quebra em espaço (a `white-space: normal` do navegador também quebra depois de hífen, o que não existe aqui).
//! - **Elementos sem equivalente.** Tabelas viram blocos empilhados (uma célula por bloco), o tachado (`del`) não é
//!   desenhado (o nó de texto do `je-ui` não tem decoração), e imagens saem antes da tradução ([`clean_rendered_markdown`]).
//! - **Foco.** O `je-ui` não tem `tabindex`: um link é alcançado pelo ponteiro, e o foco por teclado entra com o foco do nó.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::browser::html_parser::{Element, Node};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::markdown::{MarkdownEnvironment, RenderedMarkdown};
use je_base::network::FileAccess;
use je_base::path::Environment;
use je_base::resources::ExtUri;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, Role, Tree, Ui};
use je_widgets::dom::StandardKeyboardEvent;
use je_widgets::text::{UI_FONT_SIZE, WORKBENCH_LINE_HEIGHT, measure_text, ui_font};

/// O espaçamento entre blocos (`*:not(:last-child) { margin-bottom: 8px }`).
const BLOCK_MARGIN: f64 = 8.0;
/// O `padding-left` padrão de `ul` e `ol`.
const LIST_INDENT: f64 = 40.0;
/// A distância entre o marcador de uma lista e o conteúdo.
const MARKER_GAP: f64 = 6.0;
/// O recuo de uma citação.
const QUOTE_INDENT: f64 = 15.0;
const QUOTE_BAR_WIDTH: f64 = 3.0;
/// `code { font-size: 11px; line-height: 15px; padding: 1px 3px; border: 1px }`.
const CODE_FONT_SIZE: f32 = 11.0;
const CODE_LINE_HEIGHT: f64 = 15.0;
const CODE_PADDING_X: f64 = 3.0;
const CODE_BORDER: f64 = 1.0;
const CODE_RADIUS: f64 = 4.0;
/// A família de `--monaco-monospace-font` no Linux.
const MONOSPACE_FAMILIES: &str = "\"Droid Sans Mono\", \"monospace\", monospace";
/// O tamanho do codicon de um `span.codicon`.
const CODICON_SIZE: f32 = 16.0;

/// As cores e as medidas do texto de um [`MarkdownView`].
#[derive(Clone)]
pub struct MarkdownViewStyles {
    pub foreground: Rgba,
    pub link_foreground: Rgba,
    pub link_active_foreground: Rgba,
    pub code_foreground: Rgba,
    pub code_background: Option<Rgba>,
    pub code_border: Option<Rgba>,
    pub font_size: f32,
    pub line_height: f64,
}

impl MarkdownViewStyles {
    /// As cores de `.setting-item-markdown` (`foreground`, `textLink.*` e `textPreformat.*`).
    pub fn from_theme(theme: &ColorTheme) -> Self {
        let foreground = theme.get_color("foreground").unwrap_or(Rgba::opaque(0xcc, 0xcc, 0xcc));
        let link_foreground = theme.get_color("textLink.foreground").unwrap_or(Rgba::opaque(0x37, 0x94, 0xff));
        Self {
            foreground,
            link_foreground,
            link_active_foreground: theme.get_color("textLink.activeForeground").unwrap_or(link_foreground),
            code_foreground: theme.get_color("textPreformat.foreground").unwrap_or(foreground),
            code_background: theme.get_color("textPreformat.background"),
            code_border: theme.get_color("textPreformat.border"),
            font_size: UI_FONT_SIZE,
            line_height: WORKBENCH_LINE_HEIGHT,
        }
    }
}

/// O que o `renderMarkdown` lê do ambiente (`resources.extUri`, `process.platform` e `FileAccess`), com dono: o
/// [`MarkdownEnvironment`] do `je-base` só empresta.
pub struct OwnedMarkdownEnvironment {
    pub ext_uri: Rc<ExtUri>,
    pub env: Rc<dyn Environment>,
    pub file_access: Rc<FileAccess>,
}

impl OwnedMarkdownEnvironment {
    pub fn borrowed(&self) -> MarkdownEnvironment<'_> {
        MarkdownEnvironment { ext_uri: &self.ext_uri, env: &*self.env, file_access: &self.file_access }
    }
}

/// `cleanRenderedMarkdown`: tira as imagens da árvore. Como no original, depois de remover um filho o índice avança, e o
/// irmão que ocupou o lugar dele não é examinado.
pub fn clean_rendered_markdown(element: &mut Element) {
    let mut index = 0;
    while index < element.children.len() {
        let is_image = matches!(&element.children[index], Node::Element(child) if child.tag_name.eq_ignore_ascii_case("img"));
        if is_image {
            element.children.remove(index);
        } else if let Node::Element(child) = &mut element.children[index] {
            clean_rendered_markdown(child);
        }
        index += 1;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AtomKind {
    Word,
    Break,
}

/// Uma peça de uma linha: uma palavra, uma caixa de código, um codicon, ou uma quebra de linha.
struct Atom {
    kind: AtomKind,
    node: Option<NodeId>,
    /// O texto da caixa de código (a caixa é o `node`, o texto é este filho).
    inner: Option<(NodeId, f64)>,
    width: f64,
    height: f64,
    space_before: bool,
}

struct Marker {
    node: NodeId,
    width: f64,
}

/// Um bloco (parágrafo, item de lista, título, citação, código, régua).
struct Block {
    indent: f64,
    quote_bar: Option<NodeId>,
    marker: Option<Marker>,
    atoms: Vec<Atom>,
    line_height: f64,
    preformatted: bool,
    rule: Option<NodeId>,
    background: Option<NodeId>,
    space_width: f64,
}

/// O que se herda ao descer pela árvore (negrito, itálico, ligação...).
#[derive(Clone)]
struct Ctx {
    indent: f64,
    quote: bool,
    bold: bool,
    italic: bool,
    scale: f32,
    link: Option<Rc<LinkTarget>>,
}

/// O destino de um link e a ação de ativá-lo.
struct LinkTarget {
    href: JsString,
    activate: Rc<dyn Fn(&JsString)>,
}

struct Builder<'a> {
    ui: &'a Ui,
    tree: &'a mut Tree,
    root: NodeId,
    styles: &'a MarkdownViewStyles,
    disposables: &'a Rc<DisposableStore>,
    activate: Option<Rc<dyn Fn(&JsString)>>,
    blocks: Vec<Block>,
    current: Option<Block>,
    pending_space: bool,
    pending_marker: Option<String>,
}

impl<'a> Builder<'a> {
    fn base_ctx(&self) -> Ctx {
        Ctx { indent: 0.0, quote: false, bold: false, italic: false, scale: 1.0, link: None }
    }

    fn text_style(&self, ctx: &Ctx) -> TextStyle {
        ui_font(self.styles.font_size * ctx.scale, if ctx.bold { 700 } else { 400 }).italic(ctx.italic)
    }

    fn mono_style(&self) -> TextStyle {
        TextStyle::new(MONOSPACE_FAMILIES, CODE_FONT_SIZE, 400)
    }

    /// O bloco em andamento, criado (com o marcador pendente de lista e a barra da citação) se não existir.
    fn ensure_current(&mut self, ctx: &Ctx) -> &mut Block {
        if self.current.is_none() {
            let line_height = self.styles.line_height * f64::from(ctx.scale);
            let marker = self.pending_marker.take().map(|text| {
                let style = self.text_style(ctx);
                let node = self.tree.create(self.root, Role::Generic);
                self.tree.style_mut(node).content = Content::Text { text: text.clone(), style: style.clone(), color: self.styles.foreground, line_height };
                Marker { node, width: measure_text(&text, &style) }
            });
            let quote_bar = ctx.quote.then(|| {
                let node = self.tree.create(self.root, Role::Generic);
                self.tree.style_mut(node).background = Some(self.styles.foreground);
                self.tree.style_mut(node).opacity = 0.4;
                node
            });
            let space_width = measure_text(" ", &self.text_style(ctx));
            self.current = Some(Block { indent: ctx.indent, quote_bar, marker, atoms: Vec::new(), line_height, preformatted: false, rule: None, background: None, space_width });
        }
        self.current.as_mut().expect("o bloco em andamento acabou de ser criado")
    }

    /// Fecha o bloco em andamento: sem peças, ele some (o marcador e a barra dele também).
    fn flush(&mut self) {
        self.pending_space = false;
        let Some(mut block) = self.current.take() else { return };
        while block.atoms.last().is_some_and(|atom| atom.kind == AtomKind::Break) {
            block.atoms.pop();
        }
        if block.atoms.is_empty() && block.rule.is_none() {
            for node in [block.marker.as_ref().map(|marker| marker.node), block.quote_bar, block.background].into_iter().flatten() {
                self.tree.remove(node);
            }
            return;
        }
        self.blocks.push(block);
    }

    fn walk(&mut self, nodes: &[Node], ctx: &Ctx) {
        for node in nodes {
            match node {
                Node::Text(text) => self.push_text(&text.to_string_lossy(), ctx),
                Node::Element(element) => self.element(element, ctx),
                // Comentários não têm texto visível.
                Node::Comment(_) => {}
            }
        }
    }

    fn block(&mut self, element: &Element, ctx: &Ctx) {
        self.flush();
        self.walk(&element.children, ctx);
        self.flush();
    }

    fn element(&mut self, element: &Element, ctx: &Ctx) {
        let tag = element.tag_name.to_ascii_lowercase();
        match tag.as_str() {
            // Imagens saem em `clean_rendered_markdown`; estes não têm texto visível.
            "img" | "script" | "style" => {}
            "br" => self.push_break(ctx),
            "strong" | "b" => self.walk(&element.children, &Ctx { bold: true, ..ctx.clone() }),
            "em" | "i" => self.walk(&element.children, &Ctx { italic: true, ..ctx.clone() }),
            "a" => {
                let link = element.get_attribute("data-href").zip(self.activate.clone()).map(|(href, activate)| Rc::new(LinkTarget { href: href.clone(), activate }));
                let link = link.or_else(|| ctx.link.clone());
                self.walk(&element.children, &Ctx { link, ..ctx.clone() });
            }
            "code" => self.push_code(&element.text_content().to_string_lossy(), ctx),
            "span" => match codicon_id(element) {
                Some(id) => self.push_codicon(&id, ctx),
                None => self.walk(&element.children, ctx),
            },
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                // Os tamanhos padrão do navegador, em `em`.
                let scale = match tag.as_str() {
                    "h1" => 2.0,
                    "h2" => 1.5,
                    "h3" => 1.17,
                    "h4" => 1.0,
                    "h5" => 0.83,
                    _ => 0.67,
                };
                self.block(element, &Ctx { bold: true, scale, ..ctx.clone() });
            }
            "ul" | "ol" => self.list(element, ctx, tag == "ol"),
            "blockquote" => {
                self.flush();
                self.walk(&element.children, &Ctx { quote: true, indent: ctx.indent + QUOTE_INDENT, ..ctx.clone() });
                self.flush();
            }
            "pre" => self.preformatted(element, ctx),
            "hr" => self.rule(ctx),
            "p" | "div" | "li" | "td" | "th" | "dt" | "dd" | "figure" | "figcaption" | "section" | "article" => self.block(element, ctx),
            // `table`, `tr`, `thead`, `tbody` e o que o sanitizador deixar passar: o conteúdo segue no fluxo.
            _ => self.walk(&element.children, ctx),
        }
    }

    fn list(&mut self, element: &Element, ctx: &Ctx, ordered: bool) {
        self.flush();
        let mut number: i64 = element.get_attribute("start").and_then(|start| start.to_string_lossy().trim().parse().ok()).unwrap_or(1);
        let inner = Ctx { indent: ctx.indent + LIST_INDENT, ..ctx.clone() };
        for child in &element.children {
            match child {
                Node::Element(item) if item.tag_name.eq_ignore_ascii_case("li") => {
                    self.flush();
                    self.pending_marker = Some(if ordered { format!("{number}.") } else { "\u{2022}".to_owned() });
                    number += 1;
                    self.walk(&item.children, &inner);
                    self.flush();
                    self.pending_marker = None;
                }
                Node::Element(other) => self.element(other, &inner),
                Node::Text(text) => self.push_text(&text.to_string_lossy(), &inner),
                Node::Comment(_) => {}
            }
        }
        self.flush();
    }

    fn preformatted(&mut self, element: &Element, ctx: &Ctx) {
        self.flush();
        let text = element.text_content().to_string_lossy();
        let style = self.mono_style();
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let background = self.styles.code_background.map(|color| {
            let node = self.tree.create(self.root, Role::Generic);
            self.tree.style_mut(node).background = Some(color);
            self.tree.style_mut(node).radius = CODE_RADIUS;
            node
        });
        let color = self.styles.code_foreground;
        let line_height = self.styles.line_height;
        let mut atoms = Vec::new();
        for line in lines {
            if !line.is_empty() {
                let node = self.tree.create(self.root, Role::Generic);
                self.tree.style_mut(node).content = Content::Text { text: line.to_owned(), style: style.clone(), color, line_height };
                atoms.push(Atom { kind: AtomKind::Word, node: Some(node), inner: None, width: measure_text(line, &style), height: line_height, space_before: false });
            }
            atoms.push(Atom { kind: AtomKind::Break, node: None, inner: None, width: 0.0, height: 0.0, space_before: false });
        }
        let space_width = measure_text(" ", &style);
        self.blocks.push(Block { indent: ctx.indent, quote_bar: None, marker: None, atoms, line_height, preformatted: true, rule: None, background, space_width });
        if let Some(block) = self.blocks.last_mut() {
            while block.atoms.last().is_some_and(|atom| atom.kind == AtomKind::Break) {
                block.atoms.pop();
            }
        }
    }

    fn rule(&mut self, ctx: &Ctx) {
        self.flush();
        let node = self.tree.create(self.root, Role::Separator);
        self.tree.style_mut(node).background = Some(self.styles.foreground);
        self.tree.style_mut(node).opacity = 0.4;
        let line_height = self.styles.line_height;
        self.blocks.push(Block { indent: ctx.indent, quote_bar: None, marker: None, atoms: Vec::new(), line_height, preformatted: false, rule: Some(node), background: None, space_width: 0.0 });
    }

    fn push_break(&mut self, ctx: &Ctx) {
        self.pending_space = false;
        self.ensure_current(ctx).atoms.push(Atom { kind: AtomKind::Break, node: None, inner: None, width: 0.0, height: 0.0, space_before: false });
    }

    /// Cria o nó de texto de uma palavra (com o papel e as cores de link quando dentro de um `a`).
    fn word_node(&mut self, word: &str, style: &TextStyle, ctx: &Ctx, color: Rgba) -> NodeId {
        let line_height = self.styles.line_height * f64::from(ctx.scale);
        let node = self.tree.create(self.root, if ctx.link.is_some() { Role::Link } else { Role::Generic });
        let node_style = self.tree.style_mut(node);
        node_style.content = Content::Text { text: word.to_owned(), style: style.clone(), color, line_height };
        if ctx.link.is_some() {
            node_style.cursor = Some(Cursor::Pointer);
        }
        node
    }

    fn push_text(&mut self, text: &str, ctx: &Ctx) {
        if text.chars().next().is_some_and(char::is_whitespace) {
            self.pending_space = true;
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        for (index, word) in words.iter().enumerate() {
            self.push_word(word, ctx);
            if index + 1 < words.len() {
                self.pending_space = true;
            }
        }
        if !words.is_empty() && text.chars().next_back().is_some_and(char::is_whitespace) {
            self.pending_space = true;
        }
    }

    /// O espaço antes da próxima peça: só se havia espaço no texto e a linha já tem algo.
    fn take_space(&mut self, ctx: &Ctx) -> bool {
        let space = self.pending_space;
        self.pending_space = false;
        space && self.ensure_current(ctx).atoms.last().is_some_and(|atom| atom.kind != AtomKind::Break)
    }

    fn push_word(&mut self, word: &str, ctx: &Ctx) {
        let space_before = self.take_space(ctx);
        let style = self.text_style(ctx);
        let color = if ctx.link.is_some() { self.styles.link_foreground } else { self.styles.foreground };
        let node = self.word_node(word, &style, ctx, color);
        self.tree.set_label(node, word);
        self.attach_link(node, ctx);
        let atom = Atom { kind: AtomKind::Word, node: Some(node), inner: None, width: measure_text(word, &style), height: self.styles.line_height * f64::from(ctx.scale), space_before };
        self.ensure_current(ctx).atoms.push(atom);
    }

    fn push_code(&mut self, text: &str, ctx: &Ctx) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        let space_before = self.take_space(ctx);
        let style = self.mono_style();
        let text_width = measure_text(&text, &style);
        let color = if ctx.link.is_some() { self.styles.link_foreground } else { self.styles.code_foreground };

        let node = self.tree.create(self.root, if ctx.link.is_some() { Role::Link } else { Role::Generic });
        let inner = self.tree.create(node, Role::Generic);
        self.tree.style_mut(inner).content = Content::Text { text, style, color, line_height: CODE_LINE_HEIGHT };
        {
            let box_style = self.tree.style_mut(node);
            box_style.background = self.styles.code_background;
            box_style.radius = CODE_RADIUS;
            box_style.border = self.styles.code_border.map(|color| Border { width: CODE_BORDER, color });
            if ctx.link.is_some() {
                box_style.cursor = Some(Cursor::Pointer);
            }
        }
        self.attach_link(node, ctx);
        let width = text_width + 2.0 * (CODE_PADDING_X + CODE_BORDER);
        let atom = Atom { kind: AtomKind::Word, node: Some(node), inner: Some((inner, text_width)), width, height: CODE_LINE_HEIGHT + 2.0 * CODE_BORDER, space_before };
        self.ensure_current(ctx).atoms.push(atom);
    }

    fn push_codicon(&mut self, id: &str, ctx: &Ctx) {
        let space_before = self.take_space(ctx);
        let node = self.tree.create(self.root, Role::Generic);
        let color = if ctx.link.is_some() { self.styles.link_foreground } else { self.styles.foreground };
        self.tree.style_mut(node).content = Content::Codicon { id: id.to_owned(), size: CODICON_SIZE, color };
        let atom = Atom { kind: AtomKind::Word, node: Some(node), inner: None, width: f64::from(CODICON_SIZE), height: f64::from(CODICON_SIZE), space_before };
        self.ensure_current(ctx).atoms.push(atom);
    }

    /// Os ouvintes de um link (Click, Space e Enter ativam; o ponteiro sobre ele troca a cor).
    fn attach_link(&mut self, node: NodeId, ctx: &Ctx) {
        let Some(link) = &ctx.link else { return };
        let ui = self.ui.clone();
        let activate_click = link.activate.clone();
        let href = link.href.clone();
        self.disposables.add(self.ui.listen(node, EventKind::Click, false, {
            let href = href.clone();
            move |e| {
                activate_click(&href);
                e.prevent_default();
                e.stop_propagation();
            }
        }));
        let activate_key = link.activate.clone();
        self.disposables.add(self.ui.listen(node, EventKind::KeyDown, false, move |e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            if key.equals(KeyCode::Space.bits()) || key.equals(KeyCode::Enter.bits()) {
                activate_key(&href);
                e.prevent_default();
                e.stop_propagation();
            }
        }));
        let (active, normal) = (self.styles.link_active_foreground, self.styles.link_foreground);
        for (kind, color) in [(EventKind::PointerEnter, active), (EventKind::PointerLeave, normal)] {
            let ui = ui.clone();
            self.disposables.add(self.ui.listen(node, kind, false, move |_| {
                set_text_color(&mut ui.tree_mut(), node, color);
            }));
        }
    }
}

/// Troca a cor do texto de um nó (e a do texto dentro de uma caixa de código).
fn set_text_color(tree: &mut Tree, node: NodeId, color: Rgba) {
    let targets: Vec<NodeId> = std::iter::once(node).chain(tree.children(node).iter().copied()).collect();
    for target in targets {
        if let Content::Text { color: current, .. } = &mut tree.style_mut(target).content {
            *current = color;
        }
    }
}

/// O id do codicon de um `span.codicon.codicon-<id>`.
fn codicon_id(element: &Element) -> Option<String> {
    let classes = element.get_attribute("class")?.to_string_lossy();
    classes.split_whitespace().find_map(|class| class.strip_prefix("codicon-").filter(|id| !id.starts_with("modifier-")).map(str::to_owned))
}

/// O Markdown (ou o texto simples) de uma descrição como nós do `je-ui`.
pub struct MarkdownView {
    ui: Ui,
    root: NodeId,
    blocks: Vec<Block>,
    rendered: Option<RenderedMarkdown>,
    disposables: Rc<DisposableStore>,
}

impl MarkdownView {
    fn build(
        ui: &Ui,
        styles: &MarkdownViewStyles,
        rendered: Option<RenderedMarkdown>,
        plain_text: Option<&str>,
        environment: Option<Rc<OwnedMarkdownEnvironment>>,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let mut tree = ui.tree_mut();
        let root = tree.create_detached(Role::Generic);
        tree.add_class(root, "setting-item-markdown");

        let activate: Option<Rc<dyn Fn(&JsString)>> = match (&rendered, environment) {
            (Some(rendered), Some(environment)) => {
                let rendered = rendered.clone();
                Some(Rc::new(move |href: &JsString| {
                    rendered.activate_link(href, &environment.borrowed());
                }))
            }
            _ => None,
        };
        let mut builder = Builder {
            ui,
            tree: &mut tree,
            root,
            styles,
            disposables: &disposables,
            activate,
            blocks: Vec::new(),
            current: None,
            pending_space: false,
            pending_marker: None,
        };
        let ctx = builder.base_ctx();
        if let Some(rendered) = &rendered {
            let element = rendered.element.borrow();
            builder.walk(&element.children, &ctx);
        }
        if let Some(text) = plain_text {
            // `innerText = text`: cada `\n` vira um `<br>`.
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    builder.push_break(&ctx);
                }
                builder.push_text(line, &ctx);
            }
        }
        builder.flush();
        let blocks = std::mem::take(&mut builder.blocks);
        drop(tree);
        Rc::new(Self { ui: ui.clone(), root, blocks, rendered, disposables })
    }

    /// O Markdown já renderizado (e sanitizado) como nós. `environment` serve à ativação dos links.
    pub fn from_rendered(ui: &Ui, rendered: RenderedMarkdown, environment: Rc<OwnedMarkdownEnvironment>, styles: &MarkdownViewStyles) -> Rc<Self> {
        Self::build(ui, styles, Some(rendered), None, Some(environment))
    }

    /// Um texto simples (`element.innerText = text`).
    pub fn from_plain_text(ui: &Ui, text: &str, styles: &MarkdownViewStyles) -> Rc<Self> {
        Self::build(ui, styles, None, Some(text), None)
    }

    /// O nó raiz, solto: quem usa o anexa onde precisa.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Posiciona os blocos na largura dada e devolve a altura. A origem do nó raiz é a que o dono deu a ele.
    pub fn layout(&self, width: f64) -> f64 {
        let mut tree = self.ui.tree_mut();
        let origin = tree.bounds(self.root);
        let mut y = 0.0;
        let count = self.blocks.len();
        for (index, block) in self.blocks.iter().enumerate() {
            let top = y;
            let available = (width - block.indent).max(0.0);
            if let Some(rule) = block.rule {
                tree.set_bounds(rule, Rect::new(block.indent, y, block.indent + available, y + 1.0));
                y += 1.0;
            } else {
                y = Self::layout_lines(&mut tree, block, available, y);
            }
            let bottom = y;
            if let Some(marker) = &block.marker {
                tree.set_bounds(marker.node, Rect::new((block.indent - marker.width - MARKER_GAP).max(0.0), top, (block.indent - MARKER_GAP).max(marker.width), top + block.line_height));
            }
            if let Some(bar) = block.quote_bar {
                tree.set_bounds(bar, Rect::new((block.indent - QUOTE_INDENT).max(0.0), top, (block.indent - QUOTE_INDENT).max(0.0) + QUOTE_BAR_WIDTH, bottom));
            }
            if let Some(background) = block.background {
                tree.set_bounds(background, Rect::new(block.indent, top, block.indent + available, bottom));
            }
            if index + 1 < count {
                y += BLOCK_MARGIN;
            }
        }
        tree.set_bounds(self.root, Rect::new(origin.x0, origin.y0, origin.x0 + width, origin.y0 + y));
        y
    }

    /// Quebra as peças de um bloco em linhas e devolve onde o bloco termina.
    fn layout_lines(tree: &mut Tree, block: &Block, available: f64, top: f64) -> f64 {
        let line_height = block.line_height;
        let (mut x, mut line_y, mut used) = (0.0_f64, top, false);
        for atom in &block.atoms {
            if atom.kind == AtomKind::Break {
                line_y += line_height;
                x = 0.0;
                used = false;
                continue;
            }
            let mut gap = if atom.space_before && used { block.space_width } else { 0.0 };
            if !block.preformatted && used && x + gap + atom.width > available {
                line_y += line_height;
                x = 0.0;
                gap = 0.0;
            }
            x += gap;
            if let Some(node) = atom.node {
                let left = block.indent + x;
                let offset = ((line_height - atom.height) / 2.0).max(0.0);
                tree.set_bounds(node, Rect::new(left, line_y + offset, left + atom.width, line_y + offset + atom.height));
                if let Some((inner, text_width)) = atom.inner {
                    let inner_left = CODE_BORDER + CODE_PADDING_X;
                    tree.set_bounds(inner, Rect::new(inner_left, CODE_BORDER, inner_left + text_width, CODE_BORDER + CODE_LINE_HEIGHT));
                }
            }
            x += atom.width;
            used = true;
        }
        line_y + line_height
    }

    /// A altura que o último [`MarkdownView::layout`] deu ao nó raiz.
    pub fn height(&self) -> f64 {
        self.ui.tree().bounds(self.root).height()
    }
}

impl IDisposable for MarkdownView {
    fn dispose(&self) {
        self.disposables.dispose();
        if let Some(rendered) = &self.rendered {
            rendered.dispose();
        }
        if self.ui.tree().is_alive(self.root) {
            self.ui.remove(self.root);
        }
    }

    fn constructor_name(&self) -> &'static str {
        "MarkdownView"
    }
}

/// Para quem guarda uma vista que muda a cada render (o campo `RefCell<Option<Rc<MarkdownView>>>` do template).
pub type MarkdownViewSlot = RefCell<Option<Rc<MarkdownView>>>;
