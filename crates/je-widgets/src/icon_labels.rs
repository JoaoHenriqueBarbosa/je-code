//! Porte de `src/vs/base/browser/ui/iconLabel/iconLabels.ts`: `renderLabelWithIcons` e `renderIcon`, mais o
//! fluxo em linha que o DOM faz de graça com `span` e texto (e que o `je-ui`, sem motor de layout, precisa
//! escrever).
//!
//! Como o DOM vira `je-ui`:
//!
//! - O original devolve `Array<HTMLSpanElement | string>`; aqui é [`Vec<LabelPart>`]: texto puro
//!   ([`LabelPart::Text`]) ou um `span` com classes e filhos ([`LabelPart::Span`]). Um ícone é um `span` com as
//!   classes `codicon codicon-<id>` (mais `codicon-modifier-<m>` e `codicon-colored`) e o id do glifo.
//! - A expressão regular `(\\)?\$\((<nome>(?:~<modificador>)?)\)` vira uma varredura manual (o `je-widgets`
//!   não depende do motor de expressões regulares do `je-base`), com a mesma gramática de nome e modificador
//!   de `ThemeIcon`.
//! - Um nó de texto do DOM vira um nó de [`Content::Text`] sem classes; um `span`, um nó com as classes e os
//!   filhos dentro dele. [`build_parts`] cria os nós e [`layout_parts`] os posiciona da esquerda para a
//!   direita numa caixa de linha de `line_height`, como o fluxo em linha do navegador, cortando com
//!   reticências quando há uma largura máxima (o `text-overflow: ellipsis`).
//!
//! Fica de fora: o `codicon-modifier-spin` e `codicon-modifier-anim` (a animação de giro dos ícones) e o itálico
//! (o `je-text` ainda não tem estilo itálico: o estado fica nas classes e em [`InlineStyle::italic`], o texto
//! sai em tipo normal, TODO(integração)).

use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_ui::{Content, NodeId, Role, Tree};

use crate::dom::create_el;
use crate::text::{ellipsize, measure_text, ui_font};

/// O tamanho do glifo de um ícone embutido no texto (`font: normal normal normal 16px/1 codicon`).
pub const INLINE_ICON_SIZE: f64 = 16.0;

/// Um elemento de `Array<HTMLSpanElement | string>`.
#[derive(Clone, Debug, PartialEq)]
pub enum LabelPart {
    Text(String),
    Span(SpanPart),
}

/// Um `span` com classes. `icon` é o id do glifo quando o `span` é um ícone (o `::before` do `codicon`).
/// `color` e `weight` são a regra de CSS que a classe do `span` carrega (a cor e o `font-weight` do
/// `.highlight`), aplicada aos filhos.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SpanPart {
    pub class_names: Vec<String>,
    pub icon: Option<String>,
    pub color: Option<Rgba>,
    pub weight: Option<u16>,
    pub children: Vec<LabelPart>,
}

impl SpanPart {
    /// `dom.$('span.a.b', undefined, ...children)`.
    pub fn with_classes(class_names: &[&str], children: Vec<LabelPart>) -> Self {
        Self { class_names: class_names.iter().map(|c| (*c).to_owned()).collect(), children, ..Self::default() }
    }
}

/// `renderIcon(icon, renderDefaultColor)`.
pub fn render_icon(icon: &ThemeIcon, render_default_color: bool) -> SpanPart {
    let mut class_names = icon.as_class_name_array();
    if render_default_color {
        class_names.push("codicon-colored".to_owned());
    }
    let id = icon.id.split('~').next().unwrap_or_default().to_owned();
    SpanPart { class_names, icon: Some(id), ..SpanPart::default() }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// Casa `$(nome(~modificador)?)` no começo de `chars` e devolve o tamanho do trecho e o texto entre
/// parênteses.
fn match_icon(chars: &[char]) -> Option<(usize, String)> {
    if chars.len() < 4 || chars[0] != '$' || chars[1] != '(' {
        return None;
    }
    let mut i = 2;
    while i < chars.len() && is_name_char(chars[i]) {
        i += 1;
    }
    if i == 2 {
        return None;
    }
    if i < chars.len() && chars[i] == '~' {
        let modifier_start = i + 1;
        let mut j = modifier_start;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j > modifier_start {
            i = j;
        }
    }
    (i < chars.len() && chars[i] == ')').then(|| (i + 1, chars[2..i].iter().collect()))
}

/// `renderLabelWithIcons(text, renderIconsInDefaultColor)`.
pub fn render_label_with_icons(text: &str, render_icons_in_default_color: bool) -> Vec<LabelPart> {
    let chars: Vec<char> = text.chars().collect();
    let mut elements = Vec::new();
    let (mut text_start, mut i) = (0, 0);
    while i < chars.len() {
        let escaped = chars[i] == '\\' && chars.get(i + 1) == Some(&'$');
        let found = if escaped { match_icon(&chars[i + 1..]).map(|(len, name)| (len + 1, name)) } else { match_icon(&chars[i..]) };
        let Some((len, codicon)) = found else {
            i += 1;
            continue;
        };
        if text_start < i {
            elements.push(LabelPart::Text(chars[text_start..i].iter().collect()));
        }
        text_start = i + len;
        i = text_start;
        elements.push(if escaped {
            LabelPart::Text(format!("$({codicon})"))
        } else {
            LabelPart::Span(render_icon(&ThemeIcon::from_id(codicon), render_icons_in_default_color))
        });
    }
    if text_start < chars.len() {
        elements.push(LabelPart::Text(chars[text_start..].iter().collect()));
    }
    elements
}

/// O estilo do texto de uma linha em fluxo: o que o CSS herda do elemento dono.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineStyle {
    pub size_px: f32,
    pub weight: u16,
    pub color: Rgba,
    /// A altura da caixa de linha (`line-height`).
    pub line_height: f64,
    pub italic: bool,
    /// `text-decoration: line-through`.
    pub strikethrough: bool,
}

impl InlineStyle {
    pub fn text_style(&self) -> TextStyle {
        ui_font(self.size_px, self.weight)
    }
}

/// O que [`build_parts`] criou, na ordem do DOM, para [`layout_parts`].
#[derive(Clone, Debug)]
pub enum Rendered {
    /// Um nó de texto: o nó, o texto inteiro (o que cabe é decidido no layout) e o estilo com que foi criado.
    Run { node: NodeId, text: String, style: InlineStyle },
    /// Um ícone embutido.
    Icon { node: NodeId },
    /// Um `span`: o nó e o que há dentro.
    Span { node: NodeId, children: Vec<Rendered> },
}

/// `dom.reset(container, ...parts)` sem o `clearNode`: acrescenta os nós de `parts` a `container`.
pub fn build_parts(tree: &mut Tree, container: NodeId, parts: &[LabelPart], style: &InlineStyle) -> Vec<Rendered> {
    parts
        .iter()
        .map(|part| match part {
            LabelPart::Text(text) => {
                let node = create_el(tree, container, Role::Generic, "");
                tree.style_mut(node).content = Content::Text { text: text.clone(), style: style.text_style(), color: style.color, line_height: style.line_height };
                if style.strikethrough {
                    let line = create_el(tree, node, Role::Presentation, "");
                    tree.style_mut(line).background = Some(style.color);
                }
                Rendered::Run { node, text: text.clone(), style: *style }
            }
            LabelPart::Span(span) => {
                let node = create_el(tree, container, Role::Generic, &span.class_names.join(" "));
                let style = &InlineStyle { color: span.color.unwrap_or(style.color), weight: span.weight.unwrap_or(style.weight), ..*style };
                if let Some(icon) = &span.icon {
                    tree.style_mut(node).content = Content::Codicon { id: icon.clone(), size: INLINE_ICON_SIZE as f32, color: style.color };
                    return Rendered::Icon { node };
                }
                let children = build_parts(tree, node, &span.children, style);
                Rendered::Span { node, children }
            }
        })
        .collect()
}

/// A largura que ainda cabe numa linha (o `overflow: hidden` do dono): `INFINITY` se não há limite.
struct Flow {
    remaining: f64,
}

/// Posiciona `rendered` (filhos do mesmo nó) da esquerda para a direita a partir de `x = 0`, na caixa de
/// linha de `style.line_height`, e devolve a largura. Com `max_width`, o que passa dele é cortado com `…` e o
/// que vem depois some (`visible = false`).
pub fn layout_parts(tree: &mut Tree, rendered: &[Rendered], line_height: f64, max_width: Option<f64>) -> f64 {
    let mut flow = Flow { remaining: max_width.unwrap_or(f64::INFINITY) };
    layout_flow(tree, rendered, line_height, &mut flow)
}

fn layout_flow(tree: &mut Tree, rendered: &[Rendered], line_height: f64, flow: &mut Flow) -> f64 {
    let mut x = 0.0;
    for item in rendered {
        let node = match item {
            Rendered::Run { node, .. } | Rendered::Icon { node } | Rendered::Span { node, .. } => *node,
        };
        // Depois do ponto de corte nada mais cabe: o que sobra some, como no `overflow: hidden`.
        if flow.remaining <= 0.0 {
            tree.style_mut(node).visible = false;
            tree.set_bounds(node, Rect::new(x, 0.0, x, line_height));
            continue;
        }
        tree.style_mut(node).visible = true;
        let width = match item {
            Rendered::Run { text, style, .. } => {
                let text_style = style.text_style();
                let full = measure_text(text, &text_style);
                let (shown, width) = if full <= flow.remaining {
                    (text.clone(), full)
                } else {
                    let cut = ellipsize(text, &text_style, flow.remaining);
                    let w = measure_text(&cut, &text_style);
                    (cut, w)
                };
                if let Content::Text { text, .. } = &mut tree.style_mut(node).content {
                    *text = shown;
                }
                if style.strikethrough
                    && let Some(&line) = tree.children(node).first()
                {
                    let y = (line_height / 2.0).round();
                    tree.set_bounds(line, Rect::new(0.0, y, width, y + 1.0));
                }
                tree.set_bounds(node, Rect::new(x, 0.0, x + width, line_height));
                flow.remaining = if full <= flow.remaining { flow.remaining - width } else { 0.0 };
                width
            }
            Rendered::Icon { .. } => {
                let top = (line_height - INLINE_ICON_SIZE) / 2.0;
                tree.set_bounds(node, Rect::new(x, top, x + INLINE_ICON_SIZE, top + INLINE_ICON_SIZE));
                if INLINE_ICON_SIZE <= flow.remaining {
                    flow.remaining -= INLINE_ICON_SIZE;
                } else {
                    tree.style_mut(node).visible = false;
                    flow.remaining = 0.0;
                }
                INLINE_ICON_SIZE
            }
            Rendered::Span { children, .. } => {
                let width = layout_flow(tree, children, line_height, flow);
                tree.set_bounds(node, Rect::new(x, 0.0, x + width, line_height));
                width
            }
        };
        x += width;
    }
    x
}

/// Muda o tamanho do nó e mantém a posição (o `style.width/height` sem tocar em `left/top`).
pub(crate) fn resize(tree: &mut Tree, node: NodeId, width: f64, height: f64) {
    let r = tree.bounds(node);
    tree.set_bounds(node, Rect::new(r.x0, r.y0, r.x0 + width, r.y0 + height));
}
