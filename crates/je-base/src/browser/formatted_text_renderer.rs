//! Porte de `src/vs/base/browser/formattedTextRenderer.ts`: o texto com `**negrito**`, `__itálico__`, `` ``código`` ``,
//! `[[ação]]` e quebras de linha, que vira uma árvore de elementos.
//!
//! Divergências (a UI é nativa, sem DOM nem eventos de navegador):
//! - O `HTMLElement` é o [`Element`] do `html_parser`. O `target` do original (o elemento que recebe o texto e é devolvido)
//!   entra por valor e volta em [`RenderedFormattedText::element`], com os atributos que tinha e os filhos trocados.
//! - O ouvinte de `click` que `addStandardDisposableListener(a, 'click', ...)` põe em cada `<a>` de ação é o registro em
//!   [`RenderedFormattedText::actions`] (o caminho do `<a>` na árvore e o índice da ação) mais
//!   [`RenderedFormattedText::dispatch_click`], que quem ouve o clique do widget nativo chama com o caminho do alvo. O
//!   ouvinte é um item do `disposables` do [`ContentActionHandler`]: depois do `dispose` o clique não chega mais ao
//!   `callback`, como o `removeEventListener` do original.
//! - `IMouseEvent | IKeyboardEvent` é o [`ContentActionEvent`] (o clique do widget é sempre `Mouse`; o `Keyboard` existe
//!   porque o tipo do `callback` o admite).
//! - Os trechos de texto são UTF-16, como no original (`source[index]` lê uma unidade).

use std::cell::Cell;
use std::rc::Rc;

use crate::JsString;
use crate::browser::html_parser::{Element, Node};
use crate::lifecycle::{DisposableStore, to_disposable};

/// `IMouseEvent | IKeyboardEvent`: o evento que ativou a ação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentActionEvent {
    Mouse,
    Keyboard,
}

/// `IContentActionHandler`.
#[derive(Clone)]
pub struct ContentActionHandler {
    pub callback: Rc<dyn Fn(&str, &ContentActionEvent)>,
    pub disposables: Rc<DisposableStore>,
}

/// `FormattedTextRenderOptions`.
#[derive(Clone, Default)]
pub struct FormattedTextRenderOptions {
    pub action_handler: Option<ContentActionHandler>,
    pub render_code_segments: bool,
}

/// O `<a>` de uma ação: o caminho dele a partir do elemento raiz (índices em `children`) e o ouvinte de clique.
#[derive(Clone)]
pub struct ActionAnchor {
    pub path: Vec<usize>,
    pub index: usize,
    active: Rc<Cell<bool>>,
    callback: Rc<dyn Fn(&str, &ContentActionEvent)>,
}

/// O elemento devolvido por [`render_text`] e [`render_formatted_text`] e os ouvintes de ação dele.
#[derive(Clone)]
pub struct RenderedFormattedText {
    pub element: Element,
    pub actions: Vec<ActionAnchor>,
}

impl RenderedFormattedText {
    /// O `click` despachado no nó que está em `path`: chama o `callback` da ação, se o nó é o `<a>` de uma ação ainda ouvida.
    /// Devolve se o `callback` foi chamado.
    pub fn dispatch_click(&self, path: &[usize]) -> bool {
        let Some(anchor) = self.actions.iter().find(|anchor| anchor.path == path) else { return false };
        if !anchor.active.get() {
            return false;
        }
        (anchor.callback)(&anchor.index.to_string(), &ContentActionEvent::Mouse);
        true
    }
}

fn into_target(target: Option<Element>) -> Element {
    target.unwrap_or_else(|| Element::new("div", Vec::new()))
}

/// `renderText(text, _options, target)`: o texto como está.
pub fn render_text(text: &str, _options: Option<&FormattedTextRenderOptions>, target: Option<Element>) -> RenderedFormattedText {
    let mut element = into_target(target);
    element.children = vec![Node::Text(JsString::from(text))];
    RenderedFormattedText { element, actions: Vec::new() }
}

/// `renderFormattedText(formattedText, options, target)`.
pub fn render_formatted_text(formatted_text: &str, options: Option<&FormattedTextRenderOptions>, target: Option<Element>) -> RenderedFormattedText {
    let mut element = into_target(target);
    element.children = Vec::new();
    let render_code_segments = options.is_some_and(|options| options.render_code_segments);
    let tree = parse_formatted_text(&formatted_text.encode_utf16().collect::<Vec<u16>>(), render_code_segments);
    let action_handler = options.and_then(|options| options.action_handler.as_ref());
    let mut actions = Vec::new();
    let mut children = Vec::new();
    render_children(&tree, tree.root, &mut children, &mut Vec::new(), &mut actions, action_handler, render_code_segments);
    element.children = children;
    RenderedFormattedText { element, actions }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatType {
    Invalid,
    Root,
    Text,
    Bold,
    Italics,
    Action,
    ActionClose,
    Code,
    NewLine,
}

/// `IFormatParseTree`, num arena: os filhos são índices (o original compartilha referências entre `current` e a pilha).
struct ParseNode {
    type_: FormatType,
    content: Vec<u16>,
    index: usize,
    children: Vec<usize>,
}

struct ParseTree {
    nodes: Vec<ParseNode>,
    root: usize,
}

impl ParseTree {
    fn add(&mut self, type_: FormatType, content: Vec<u16>) -> usize {
        self.nodes.push(ParseNode { type_, content, index: 0, children: Vec::new() });
        self.nodes.len() - 1
    }
}

fn format_tag_type(unit: u16, support_code_segments: bool) -> FormatType {
    match char::from_u32(u32::from(unit)) {
        Some('*') => FormatType::Bold,
        Some('_') => FormatType::Italics,
        Some('[') => FormatType::Action,
        Some(']') => FormatType::ActionClose,
        Some('`') if support_code_segments => FormatType::Code,
        _ => FormatType::Invalid,
    }
}

fn parse_formatted_text(content: &[u16], parse_code_segments: bool) -> ParseTree {
    let mut tree = ParseTree { nodes: Vec::new(), root: 0 };
    tree.root = tree.add(FormatType::Root, Vec::new());

    let mut action_view_item_index = 0;
    let mut current = tree.root;
    let mut stack: Vec<usize> = Vec::new();
    let mut position = 0;

    while position < content.len() {
        let mut next = content[position];
        position += 1;
        let peek = content.get(position).copied();

        let is_escaped_format_type = next == u16::from(b'\\') && peek.is_some_and(|peek| format_tag_type(peek, parse_code_segments) != FormatType::Invalid);
        if is_escaped_format_type {
            // unread the backslash if it escapes a format tag type
            next = content[position];
            position += 1;
        }

        let is_format_tag = format_tag_type(next, parse_code_segments) != FormatType::Invalid;
        if !is_escaped_format_type && is_format_tag && Some(next) == content.get(position).copied() {
            position += 1;

            if tree.nodes[current].type_ == FormatType::Text {
                current = stack.pop().expect("um nó de texto sempre foi empilhado");
            }

            let type_ = format_tag_type(next, parse_code_segments);
            let current_type = tree.nodes[current].type_;
            if current_type == type_ || (current_type == FormatType::Action && type_ == FormatType::ActionClose) {
                current = stack.pop().expect("um nó aberto sempre foi empilhado");
            } else {
                let new_current = tree.add(type_, Vec::new());
                if type_ == FormatType::Action {
                    tree.nodes[new_current].index = action_view_item_index;
                    action_view_item_index += 1;
                }
                tree.nodes[current].children.push(new_current);
                stack.push(current);
                current = new_current;
            }
        } else if next == u16::from(b'\n') {
            if tree.nodes[current].type_ == FormatType::Text {
                current = stack.pop().expect("um nó de texto sempre foi empilhado");
            }
            let new_line = tree.add(FormatType::NewLine, Vec::new());
            tree.nodes[current].children.push(new_line);
        } else if tree.nodes[current].type_ != FormatType::Text {
            let text_current = tree.add(FormatType::Text, vec![next]);
            tree.nodes[current].children.push(text_current);
            stack.push(current);
            current = text_current;
        } else {
            tree.nodes[current].content.push(next);
        }
    }

    // O resto da pilha (um literal mal formatado) não tem tratamento no original.
    tree
}

/// Os filhos de `node_id` renderizados em `out`; `path` é o caminho de `out` a partir do elemento raiz.
fn render_children(
    tree: &ParseTree,
    node_id: usize,
    out: &mut Vec<Node>,
    path: &mut Vec<usize>,
    actions: &mut Vec<ActionAnchor>,
    action_handler: Option<&ContentActionHandler>,
    render_code_segments: bool,
) {
    for &child_id in &tree.nodes[node_id].children {
        render_node(tree, child_id, out, path, actions, action_handler, render_code_segments);
    }
}

/// `_renderFormattedText` para um nó que não é a raiz: o que não vira elemento (`ActionClose`, uma ação sem manipulador) some
/// com os filhos.
fn render_node(
    tree: &ParseTree,
    node_id: usize,
    out: &mut Vec<Node>,
    path: &mut Vec<usize>,
    actions: &mut Vec<ActionAnchor>,
    action_handler: Option<&ContentActionHandler>,
    render_code_segments: bool,
) {
    let node = &tree.nodes[node_id];
    let tag = match node.type_ {
        FormatType::Text => {
            out.push(Node::Text(JsString::from(&node.content[..])));
            return;
        }
        FormatType::Bold => "b",
        FormatType::Italics => "i",
        FormatType::Code if render_code_segments => "code",
        FormatType::Action if action_handler.is_some() => "a",
        FormatType::NewLine => "br",
        _ => return,
    };

    let position = out.len();
    if let (FormatType::Action, Some(handler)) = (node.type_, action_handler) {
        let active = Rc::new(Cell::new(true));
        let listener = Rc::clone(&active);
        handler.disposables.add(to_disposable(move || listener.set(false)));
        let mut anchor_path = path.clone();
        anchor_path.push(position);
        actions.push(ActionAnchor { path: anchor_path, index: node.index, active, callback: Rc::clone(&handler.callback) });
    }

    let mut element = Element::new(tag, Vec::new());
    path.push(position);
    render_children(tree, node_id, &mut element.children, path, actions, action_handler, render_code_segments);
    path.pop();
    out.push(Node::Element(element));
}
