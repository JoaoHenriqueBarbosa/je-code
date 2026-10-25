//! A tradução nativa do Markdown renderizado (`IRenderedMarkdown.element`, a árvore sanitizada de [`Element`] que
//! `je_base::markdown::render_markdown` entrega) para nós do `je-ui`, que o template do editor de configurações usa para a
//! descrição e para a mensagem de depreciação (`renderSettingMarkdown` de `settingsTree.ts`). No VS Code isso é o DOM do
//! navegador com o CSS de `.setting-item-markdown`; aqui é um nó por bloco, empilhados pelo [`MarkdownView::layout`].
//!
//! O porte de `IMarkdownRendererService` (a camada que dá ao `renderMarkdown` o ambiente e escolhe os widgets) ainda não
//! existe na plataforma; quando existir, este módulo migra para `je-widgets` como o mapeamento "árvore para widgets" que o
//! `//!` de `je_base::markdown` deixou em aberto, e o [`OwnedMarkdownEnvironment`] sai daqui.
//!
//! Um bloco (parágrafo, item de lista, título, bloco de código) é UM nó com [`Content::Paragraph`]: os trechos inline
//! (`strong`, `em`, `a`, `code`, `span.codicon`, `br`) são [`Span`]s do mesmo texto, e quem quebra as linhas na largura do bloco
//! é o motor de texto (`je-text`, parley), como o navegador faz no `white-space: normal`, inclusive depois de hífen. O
//! `textContent` do nó é o texto do bloco ([`je_ui::Tree::text_content`]), sem nó por palavra.
//!
//! Do CSS de `settingsEditor2.css` (`.setting-item-markdown`) para a geometria:
//! - `* { margin: 0 }` e `*:not(:last-child) { margin-bottom: 8px }`: cada bloco (parágrafo, item de lista, título, citação,
//!   bloco de código) fica a 8 px do seguinte, e o último não tem margem.
//! - `a`: cor `textLink.foreground`, `textLink.activeForeground` sob o ponteiro, cursor de mão. O trecho do link carrega o
//!   número do destino; o clique no bloco, sobre um trecho de link, chama [`RenderedMarkdown::activate_link`] com o
//!   `data-href`, como o `renderMarkdown` do original faz com os ouvintes de clique.
//! - `code`: fonte monoespaçada de 11 px, `padding: 1px 3px`, raio de 4 px e borda de 1 px (`textPreformat.*`), numa caixa
//!   em linha que não quebra por dentro.
//! - `ul` e `ol` têm o recuo padrão do navegador (40 px) e o marcador fora do conteúdo; a citação (`blockquote`) recua 15 px e
//!   ganha uma barra de 3 px à esquerda de cada um dos blocos dela.
//!
//! Divergências:
//! - **Elementos sem equivalente.** Tabelas viram blocos empilhados (uma célula por bloco), o tachado (`del`) não é
//!   desenhado (o parágrafo do `je-ui` não tem decoração de texto), e imagens saem antes da tradução
//!   ([`clean_rendered_markdown`]).
//! - **Espaço em branco.** O `white-space: normal` é aplicado aqui, ao montar os trechos: sequências de espaço, tabulação e
//!   quebra de linha do texto viram um espaço, e o espaço no começo e no fim do bloco (e junto de um `br`) é descartado.
//!
//! Acessibilidade e teclado do link, como o `a` do Chromium: cada link do bloco é um nó filho `Role::Link` do nó do bloco
//! ([`Ui::sync_paragraph_links`]), sem caixa de layout própria, com o texto do link por nome acessível e as caixas vindas da
//! disposição do parágrafo ([`Tree::link_boxes`]). Ele entra na ordem de Tab, o `Enter` o ativa (um `Click` com o nó do
//! link por alvo, que o ouvinte do bloco resolve para o destino), o `Space` não o ativa (como no Chromium) e o foco desenha
//! o contorno de 1 px de `focusBorder` dentro de cada caixa do link (`a:focus { outline-offset: -1px }`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::browser::html_parser::{Element, Node};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::markdown::{MarkdownEnvironment, RenderedMarkdown};
use je_base::network::FileAccess;
use je_base::path::Environment;
use je_base::resources::ExtUri;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::{CODICON_FAMILY, TextStyle, codicon_code_point};
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, Paragraph, Role, Span, SpanFrame, SpanKind, Tree, Ui};
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
/// `code { font-size: 11px; padding: 1px 3px; border: 1px }`.
const CODE_FONT_SIZE: f32 = 11.0;
const CODE_PADDING_X: f64 = 3.0;
const CODE_PADDING_Y: f64 = 1.0;
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
    /// `focusBorder`: a cor do contorno de foco de um link.
    pub focus_border: Rgba,
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
            focus_border: theme.get_color("focusBorder").unwrap_or(Rgba::opaque(0x00, 0x7f, 0xd4)),
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

struct Marker {
    node: NodeId,
    width: f64,
}

/// Um bloco pronto (parágrafo, item de lista, título, citação, código, régua): o nó dele e o que o enfeita.
struct Block {
    indent: f64,
    quote_bar: Option<NodeId>,
    marker: Option<Marker>,
    /// O nó do parágrafo, ou a régua.
    node: NodeId,
    line_height: f64,
    rule: bool,
}

/// Um bloco em montagem: os trechos ainda não viraram nó.
struct Draft {
    indent: f64,
    quote_bar: Option<NodeId>,
    marker: Option<Marker>,
    line_height: f64,
    spans: Vec<Span>,
    /// Os destinos dos links do bloco, na ordem do número que os trechos carregam.
    links: Vec<Rc<LinkTarget>>,
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
    /// Os nós de bloco que têm link: ganham os nós de link quando a montagem solta a árvore.
    linked: Vec<NodeId>,
    current: Option<Draft>,
    /// O texto montado até aqui termina em espaço (ou o bloco está no começo): o próximo espaço é descartado.
    trailing_space: bool,
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

    /// A cor do texto de um trecho: a do link dentro de um `a`, a do texto fora.
    fn text_color(&self, ctx: &Ctx, plain: Rgba) -> Rgba {
        if ctx.link.is_some() { self.styles.link_foreground } else { plain }
    }

    /// O bloco em andamento, criado (com o marcador pendente de lista e a barra da citação) se não existir.
    fn ensure_current(&mut self, ctx: &Ctx) -> &mut Draft {
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
            self.current = Some(Draft { indent: ctx.indent, quote_bar, marker, line_height, spans: Vec::new(), links: Vec::new() });
        }
        self.current.as_mut().expect("o bloco em andamento acabou de ser criado")
    }

    /// Fecha o bloco em andamento: sem trechos, ele some (o marcador e a barra dele também).
    fn flush(&mut self) {
        self.trailing_space = true;
        let Some(mut draft) = self.current.take() else { return };
        while draft.spans.last().is_some_and(|span| span.kind == SpanKind::LineBreak) {
            draft.spans.pop();
        }
        trim_trailing_space(&mut draft.spans);
        if draft.spans.is_empty() {
            for node in [draft.marker.as_ref().map(|marker| marker.node), draft.quote_bar].into_iter().flatten() {
                self.tree.remove(node);
            }
            return;
        }
        self.finish(draft, true, None);
    }

    /// Cria o nó do bloco com os trechos do rascunho. `wrap` falso é o `white-space: pre`; `background` é o fundo do bloco.
    fn finish(&mut self, draft: Draft, wrap: bool, background: Option<Rgba>) {
        let node = self.tree.create(self.root, Role::Generic);
        let mut paragraph = Paragraph::new(draft.spans, draft.line_height);
        paragraph.wrap = wrap;
        if !draft.links.is_empty() {
            paragraph.link_hover_color = Some(self.styles.link_active_foreground);
            paragraph.focus_color = Some(self.styles.focus_border);
        }
        let style = self.tree.style_mut(node);
        style.content = Content::Paragraph(paragraph);
        if let Some(color) = background {
            style.background = Some(color);
            style.radius = CODE_RADIUS;
        }
        if !draft.links.is_empty() {
            self.attach_links(node, draft.links);
            self.linked.push(node);
        }
        self.blocks.push(Block { indent: draft.indent, quote_bar: draft.quote_bar, marker: draft.marker, node, line_height: draft.line_height, rule: false });
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

    /// O bloco de código: cada linha do texto é um trecho, separado por quebra, sem colapsar espaço nem quebrar na largura.
    fn preformatted(&mut self, element: &Element, ctx: &Ctx) {
        self.flush();
        let text = element.text_content().to_string_lossy();
        let style = self.mono_style();
        let color = self.styles.code_foreground;
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let mut spans = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                spans.push(Span::line_break(style.clone(), color));
            }
            if !line.is_empty() {
                spans.push(Span::text(*line, style.clone(), color));
            }
        }
        if spans.is_empty() {
            return;
        }
        let draft = Draft { indent: ctx.indent, quote_bar: None, marker: None, line_height: self.styles.line_height, spans, links: Vec::new() };
        self.finish(draft, false, self.styles.code_background);
    }

    fn rule(&mut self, ctx: &Ctx) {
        self.flush();
        let node = self.tree.create(self.root, Role::Separator);
        self.tree.style_mut(node).background = Some(self.styles.foreground);
        self.tree.style_mut(node).opacity = 0.4;
        self.blocks.push(Block { indent: ctx.indent, quote_bar: None, marker: None, node, line_height: self.styles.line_height, rule: true });
    }

    /// Acrescenta o trecho ao bloco em andamento; o número do link do `ctx` é o do destino na lista do bloco.
    fn push_span(&mut self, mut span: Span, ctx: &Ctx) {
        let draft = self.ensure_current(ctx);
        if let Some(target) = &ctx.link {
            let known = draft.links.iter().position(|link| Rc::ptr_eq(link, target));
            let index = known.unwrap_or(draft.links.len());
            if known.is_none() {
                draft.links.push(target.clone());
            }
            span.link = Some(index as u32);
        }
        draft.spans.push(span);
    }

    fn push_break(&mut self, ctx: &Ctx) {
        let span = Span::line_break(self.text_style(ctx), self.styles.foreground);
        trim_trailing_space(&mut self.ensure_current(ctx).spans);
        self.push_span(span, &Ctx { link: None, ..ctx.clone() });
        self.trailing_space = true;
    }

    /// O texto com o `white-space: normal`: sequências de espaço viram um, e o espaço no começo do bloco é descartado.
    fn push_text(&mut self, text: &str, ctx: &Ctx) {
        let mut collapsed = String::with_capacity(text.len());
        let mut after_space = self.trailing_space;
        for ch in text.chars() {
            if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                if !after_space {
                    collapsed.push(' ');
                }
                after_space = true;
            } else {
                collapsed.push(ch);
                after_space = false;
            }
        }
        if collapsed.is_empty() {
            return;
        }
        self.trailing_space = after_space;
        let span = Span::text(collapsed, self.text_style(ctx), self.text_color(ctx, self.styles.foreground));
        self.push_span(span, ctx);
    }

    fn push_code(&mut self, text: &str, ctx: &Ctx) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        let frame = SpanFrame {
            inset_x: CODE_PADDING_X + CODE_BORDER,
            inset_y: CODE_PADDING_Y + CODE_BORDER,
            background: self.styles.code_background,
            border: self.styles.code_border.map(|color| Border { width: CODE_BORDER, color }),
            radius: CODE_RADIUS,
        };
        let span = Span::text(text, self.mono_style(), self.text_color(ctx, self.styles.code_foreground)).with_frame(frame);
        self.trailing_space = false;
        self.push_span(span, ctx);
    }

    fn push_codicon(&mut self, id: &str, ctx: &Ctx) {
        let Some(glyph) = codicon_code_point(id) else { return };
        let span = Span::icon(glyph, TextStyle::new(CODICON_FAMILY, CODICON_SIZE, 400), self.text_color(ctx, self.styles.foreground));
        self.trailing_space = false;
        self.push_span(span, ctx);
    }

    /// Os ouvintes dos links de um bloco: o clique sobre um trecho de link ativa o destino; o ponteiro sobre ele troca a cor e
    /// o cursor.
    fn attach_links(&mut self, node: NodeId, links: Vec<Rc<LinkTarget>>) {
        let links = Rc::new(links);
        let ui = self.ui.clone();
        self.disposables.add(self.ui.listen(node, EventKind::Click, false, {
            let (ui, links) = (ui.clone(), links.clone());
            move |e| {
                // O clique do ponteiro tem o ponto; o `Enter` num link com foco chega com o nó do link por alvo.
                let index = {
                    let tree = ui.tree();
                    match e.pointer {
                        Some(pointer) => tree.paragraph_link_at(node, pointer.x, pointer.y),
                        None => tree.link_index(e.target),
                    }
                };
                let Some(index) = index else { return };
                let Some(link) = links.get(index as usize) else { return };
                (link.activate)(&link.href);
                e.prevent_default();
                e.stop_propagation();
            }
        }));
        self.disposables.add(self.ui.listen(node, EventKind::PointerMove, false, {
            let ui = ui.clone();
            move |e| {
                let link = e.pointer.and_then(|pointer| ui.tree().paragraph_link_at(node, pointer.x, pointer.y));
                set_hover_link(&mut ui.tree_mut(), node, link);
            }
        }));
        self.disposables.add(self.ui.listen(node, EventKind::PointerLeave, false, move |_| {
            set_hover_link(&mut ui.tree_mut(), node, None);
        }));
    }
}

/// Tira o espaço do fim do último trecho de texto (o `white-space: normal` descarta o espaço no fim da linha).
fn trim_trailing_space(spans: &mut Vec<Span>) {
    if let Some(last) = spans.last_mut()
        && last.kind == SpanKind::Text
        && last.frame.is_none()
        && last.text.ends_with(' ')
    {
        last.text.pop();
        if last.text.is_empty() {
            spans.pop();
        }
    }
}

/// O link sob o ponteiro: pinta com a cor ativa (`textLink.activeForeground`) e põe o cursor de mão.
fn set_hover_link(tree: &mut Tree, node: NodeId, link: Option<u32>) {
    let style = tree.style_mut(node);
    if let Content::Paragraph(paragraph) = &mut style.content
        && paragraph.hover_link != link
    {
        paragraph.hover_link = link;
    }
    style.cursor = link.map(|_| Cursor::Pointer);
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
            linked: Vec::new(),
            current: None,
            trailing_space: true,
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
        let linked = std::mem::take(&mut builder.linked);
        drop(tree);
        for node in linked {
            ui.sync_paragraph_links(node);
        }
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
            let height = match &tree.style(block.node).content {
                _ if block.rule => 1.0,
                Content::Paragraph(paragraph) => paragraph.height(available),
                _ => 0.0,
            };
            tree.set_bounds(block.node, Rect::new(block.indent, y, block.indent + available, y + height));
            y += height;
            let bottom = y;
            if let Some(marker) = &block.marker {
                tree.set_bounds(marker.node, Rect::new((block.indent - marker.width - MARKER_GAP).max(0.0), top, (block.indent - MARKER_GAP).max(marker.width), top + block.line_height));
            }
            if let Some(bar) = block.quote_bar {
                tree.set_bounds(bar, Rect::new((block.indent - QUOTE_INDENT).max(0.0), top, (block.indent - QUOTE_INDENT).max(0.0) + QUOTE_BAR_WIDTH, bottom));
            }
            if index + 1 < count {
                y += BLOCK_MARGIN;
            }
        }
        tree.set_bounds(self.root, Rect::new(origin.x0, origin.y0, origin.x0 + width, origin.y0 + y));
        y
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
