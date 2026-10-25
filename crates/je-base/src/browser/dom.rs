//! Porte do que o teste de `src/vs/base/browser/dom.ts` (e `window.ts`) exercita, sobre um modelo de documento nativo.
//!
//! Como o navegador virou Rust:
//! - O DOM é um arena por documento ([`Document`]); um [`Node`] é o par (documento, índice). Há elementos e nós de texto, com
//!   atributos, `classList`, filhos e pai. É o que `$`, `h`, `copyAttributes`, `trackAttributes` e `findParentWithClass`
//!   leem e escrevem. A árvore de widgets do `je-ui` é outra coisa (retângulos e papéis, sem etiqueta HTML).
//! - Uma janela ([`CodeWindow`]) tem o documento, o identificador (`vscodeWindowId`), a fila de quadros de animação
//!   (`scheduleAtNextAnimationFrame` e companhia) e o contexto recente dos `ResizeObserver`. Como não existe compositor, quem
//!   hospeda a janela chama [`CodeWindow::render_frame`] a cada quadro; é o `requestAnimationFrame` que dispara.
//! - O registro de janelas (`getWindows`, `getWindowById`, `hasWindow`, `registerWindow`) é por thread, com a janela
//!   principal de id 1. Os eventos `onDidRegisterWindow` e companhia não são portados aqui. `getWindowById` devolve a janela, não
//!   o par `{ window, disposables }`.
//! - Os timers (`setInterval`, `setTimeout`) são os do `runtime` do thread, um só para todas as janelas, então
//!   [`disposable_window_interval`] não recebe janela.
//! - Um `MutationObserver` entrega as mudanças numa microtarefa; [`track_attributes`] faz o mesmo sobre `queue_microtask`.
//! - O callback de um `ResizeObserver` que lança vira um `Result`: o `Err` vai para `onUnexpectedError`, como o `catch` do original.
//! - Atributo `onxxx` em `$` é atribuição de propriedade, e atribuir texto a um manipulador de evento não faz nada, então
//!   `$` o ignora.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::{Rc, Weak};

use crate::async_::{IntervalHandle, clear_interval, set_interval};
use crate::dynamic::Dynamic;
use crate::errors::{Error, on_unexpected_error};
use crate::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, none, to_disposable};
use crate::numbers::is_point_within_triangle;
use crate::runtime::{self, TimerId};

// ---------------------------------------------------------------------------------------------------------------------
// Documento e nós
// ---------------------------------------------------------------------------------------------------------------------

enum NodeData {
    Element { tag_name: String, attributes: Vec<(String, String)> },
    Text(String),
}

struct NodeSlot {
    data: NodeData,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// Um `MutationObserver` de atributos de [`track_attributes`].
struct AttributeTracker {
    from: Node,
    to: Node,
    filter: Option<Vec<String>>,
    pending: RefCell<Vec<String>>,
    scheduled: Cell<bool>,
    disposed: Cell<bool>,
}

impl AttributeTracker {
    fn record(self: &Rc<Self>, name: &str) {
        if self.disposed.get() || self.filter.as_ref().is_some_and(|filter| !filter.iter().any(|candidate| candidate == name)) {
            return;
        }
        self.pending.borrow_mut().push(name.to_owned());
        if !self.scheduled.replace(true) {
            let tracker = self.clone();
            runtime::queue_microtask(move || tracker.flush());
        }
    }

    fn flush(&self) {
        self.scheduled.set(false);
        if self.disposed.get() {
            return;
        }
        for name in std::mem::take(&mut *self.pending.borrow_mut()) {
            copy_attribute(&self.from, &self.to, &name);
        }
    }
}

/// O `document`: dono dos nós e dos observadores de atributos.
pub struct Document {
    view: Weak<CodeWindow>,
    nodes: RefCell<Vec<NodeSlot>>,
    trackers: RefCell<Vec<Rc<AttributeTracker>>>,
    hidden: Cell<bool>,
}

impl Document {
    fn new(view: Weak<CodeWindow>) -> Rc<Self> {
        Rc::new(Self { view, nodes: RefCell::new(Vec::new()), trackers: RefCell::new(Vec::new()), hidden: Cell::new(false) })
    }

    /// `document.visibilityState`: `visible` até alguém esconder a janela ([`Document::set_hidden`]).
    pub fn visibility_state(&self) -> &'static str {
        if self.hidden.get() { "hidden" } else { "visible" }
    }

    /// Esconde ou mostra a janela do documento (uma janela minimizada ou coberta).
    pub fn set_hidden(&self, hidden: bool) {
        self.hidden.set(hidden);
    }

    fn alloc(self: &Rc<Self>, data: NodeData) -> Node {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(NodeSlot { data, parent: None, children: Vec::new() });
        Node { document: self.clone(), index: nodes.len() - 1 }
    }

    /// `document.createElement(tagName)`: o nome vira maiúsculo, como no `tagName` de um elemento HTML.
    pub fn create_element(self: &Rc<Self>, tag_name: &str) -> Node {
        self.alloc(NodeData::Element { tag_name: tag_name.to_ascii_uppercase(), attributes: Vec::new() })
    }

    /// `document.createTextNode(data)`.
    pub fn create_text_node(self: &Rc<Self>, data: &str) -> Node {
        self.alloc(NodeData::Text(data.to_owned()))
    }

    /// `document.createEvent('MouseEvent')`: um evento sem `view`.
    pub fn create_event(&self) -> UiEvent {
        UiEvent { view: None }
    }

    /// `document.defaultView`.
    pub fn default_view(&self) -> Option<Rc<CodeWindow>> {
        self.view.upgrade()
    }

    fn attribute_changed(&self, node: &Node, name: &str) {
        let trackers: Vec<Rc<AttributeTracker>> = self.trackers.borrow().iter().filter(|tracker| tracker.from == *node).cloned().collect();
        for tracker in trackers {
            tracker.record(name);
        }
    }
}

/// Um nó do documento (elemento ou texto).
#[derive(Clone)]
pub struct Node {
    document: Rc<Document>,
    index: usize,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.document, &other.document) && self.index == other.index
    }
}

impl Eq for Node {}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.index)
    }
}

impl Node {
    fn with<R>(&self, read: impl FnOnce(&NodeSlot) -> R) -> R {
        read(&self.document.nodes.borrow()[self.index])
    }

    fn with_mut<R>(&self, write: impl FnOnce(&mut NodeSlot) -> R) -> R {
        write(&mut self.document.nodes.borrow_mut()[self.index])
    }

    fn at(&self, index: usize) -> Node {
        Node { document: self.document.clone(), index }
    }

    /// `ownerDocument`.
    pub fn owner_document(&self) -> Rc<Document> {
        self.document.clone()
    }

    /// `isHTMLElement(node)` (um nó de texto não é).
    pub fn is_html_element(&self) -> bool {
        self.with(|slot| matches!(slot.data, NodeData::Element { .. }))
    }

    /// `tagName`: `None` num nó de texto (`undefined`).
    pub fn tag_name(&self) -> Option<String> {
        self.with(|slot| match &slot.data {
            NodeData::Element { tag_name, .. } => Some(tag_name.clone()),
            NodeData::Text(_) => None,
        })
    }

    pub fn parent_node(&self) -> Option<Node> {
        self.with(|slot| slot.parent).map(|parent| self.at(parent))
    }

    pub fn first_child(&self) -> Option<Node> {
        self.with(|slot| slot.children.first().copied()).map(|child| self.at(child))
    }

    pub fn child_nodes(&self) -> Vec<Node> {
        self.with(|slot| slot.children.clone()).into_iter().map(|child| self.at(child)).collect()
    }

    pub fn first_element_child(&self) -> Option<Node> {
        self.child_nodes().into_iter().find(Node::is_html_element)
    }

    pub fn child_element_count(&self) -> usize {
        self.child_nodes().iter().filter(|child| child.is_html_element()).count()
    }

    /// `appendChild`: tira o filho do pai anterior e o põe por último.
    pub fn append_child(&self, child: &Node) -> Node {
        assert!(Rc::ptr_eq(&self.document, &child.document), "WrongDocumentError");
        let mut ancestor = Some(self.clone());
        while let Some(current) = ancestor {
            assert!(current != *child, "HierarchyRequestError");
            ancestor = current.parent_node();
        }
        if let Some(old_parent) = child.parent_node() {
            old_parent.with_mut(|slot| slot.children.retain(|candidate| *candidate != child.index));
        }
        child.with_mut(|slot| slot.parent = Some(self.index));
        self.with_mut(|slot| slot.children.push(child.index));
        child.clone()
    }

    /// `append(text)`: um nó de texto novo, por último.
    pub fn append_text(&self, data: &str) -> Node {
        let text = self.document.create_text_node(data);
        self.append_child(&text)
    }

    /// `textContent`: o texto de todos os descendentes de texto, na ordem.
    pub fn text_content(&self) -> String {
        match self.with(|slot| match &slot.data {
            NodeData::Text(data) => Some(data.clone()),
            NodeData::Element { .. } => None,
        }) {
            Some(data) => data,
            None => self.child_nodes().iter().map(Node::text_content).collect(),
        }
    }

    /// `getAttribute`.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        let name = name.to_ascii_lowercase();
        self.with(|slot| match &slot.data {
            NodeData::Element { attributes, .. } => attributes.iter().find(|(candidate, _)| *candidate == name).map(|(_, value)| value.clone()),
            NodeData::Text(_) => None,
        })
    }

    /// `setAttribute`: o nome vira minúsculo, como num documento HTML.
    pub fn set_attribute(&self, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        self.with_mut(|slot| {
            if let NodeData::Element { attributes, .. } = &mut slot.data {
                match attributes.iter_mut().find(|(candidate, _)| *candidate == name) {
                    Some((_, current)) => *current = value.to_owned(),
                    None => attributes.push((name.clone(), value.to_owned())),
                }
            }
        });
        self.document.attribute_changed(self, &name);
    }

    /// `removeAttribute`.
    pub fn remove_attribute(&self, name: &str) {
        let name = name.to_ascii_lowercase();
        let removed = self.with_mut(|slot| match &mut slot.data {
            NodeData::Element { attributes, .. } => {
                let before = attributes.len();
                attributes.retain(|(candidate, _)| *candidate != name);
                attributes.len() != before
            }
            NodeData::Text(_) => false,
        });
        if removed {
            self.document.attribute_changed(self, &name);
        }
    }

    /// `attributes`: nome e valor, na ordem em que entraram.
    pub fn attributes(&self) -> Vec<(String, String)> {
        self.with(|slot| match &slot.data {
            NodeData::Element { attributes, .. } => attributes.clone(),
            NodeData::Text(_) => Vec::new(),
        })
    }

    /// `id`.
    pub fn id(&self) -> String {
        self.get_attribute("id").unwrap_or_default()
    }

    pub fn set_id(&self, id: &str) {
        self.set_attribute("id", id);
    }

    /// `className`.
    pub fn class_name(&self) -> String {
        self.get_attribute("class").unwrap_or_default()
    }

    pub fn set_class_name(&self, class_name: &str) {
        self.set_attribute("class", class_name);
    }

    /// Os tokens do `classList`: os do `class`, sem repetição, na ordem.
    fn class_tokens(&self) -> Vec<String> {
        let mut tokens: Vec<String> = Vec::new();
        for token in self.class_name().split_whitespace() {
            if !tokens.iter().any(|candidate| candidate == token) {
                tokens.push(token.to_owned());
            }
        }
        tokens
    }

    /// `classList.contains` (o token vazio nunca está).
    pub fn class_list_contains(&self, class: &str) -> bool {
        !class.is_empty() && self.class_tokens().iter().any(|candidate| candidate == class)
    }

    /// `classList.length`.
    pub fn class_list_length(&self) -> usize {
        self.class_tokens().len()
    }

    /// `classList.add`: reescreve o `class` com os tokens separados por um espaço.
    pub fn class_list_add(&self, class: &str) {
        let mut tokens = self.class_tokens();
        if !tokens.iter().any(|candidate| candidate == class) {
            tokens.push(class.to_owned());
        }
        self.set_class_name(&tokens.join(" "));
    }

    /// `classList.remove`.
    pub fn class_list_remove(&self, class: &str) {
        let mut tokens = self.class_tokens();
        tokens.retain(|candidate| candidate != class);
        self.set_class_name(&tokens.join(" "));
    }

    /// As declarações do atributo `style` (`nome: valor;`), na ordem em que entraram.
    fn style_declarations(&self) -> Vec<(String, String)> {
        self.get_attribute("style")
            .unwrap_or_default()
            .split(';')
            .filter_map(|declaration| declaration.split_once(':'))
            .map(|(property, current)| (property.trim().to_owned(), current.trim().to_owned()))
            .collect()
    }

    /// `style.setProperty(name, value)`: o atributo `style` guarda `nome: valor;` por propriedade.
    pub fn style_set_property(&self, name: &str, value: &str) {
        let mut declarations = self.style_declarations();
        match declarations.iter_mut().find(|(property, _)| property == name) {
            Some((_, current)) => *current = value.to_owned(),
            None => declarations.push((name.to_owned(), value.to_owned())),
        }
        let style: Vec<String> = declarations.iter().map(|(property, current)| format!("{property}: {current};")).collect();
        self.set_attribute("style", &style.join(" "));
    }

    /// `style.getPropertyValue(name)`: vazio quando o nó não define a propriedade.
    pub fn style_get_property_value(&self, name: &str) -> String {
        self.style_declarations().into_iter().find(|(property, _)| property == name).map(|(_, value)| value).unwrap_or_default()
    }

    /// `getComputedStyle(node).getPropertyValue(name)` de uma propriedade personalizada (`--nome`), que herda: vale a do
    /// próprio nó ou, sem ela, a do ancestral mais próximo que a define; vazio sem nenhuma.
    pub fn computed_style_property_value(&self, name: &str) -> String {
        let mut current = Some(self.clone());
        while let Some(node) = current {
            let value = node.style_get_property_value(name);
            if !value.is_empty() {
                return value;
            }
            current = node.parent_node();
        }
        String::new()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `$` e `h`
// ---------------------------------------------------------------------------------------------------------------------

/// Um filho de `$`: nó ou texto.
pub enum Child<'a> {
    Node(&'a Node),
    Text(&'a str),
}

struct Selector<'a> {
    tag: &'a str,
    id: Option<&'a str>,
    classes: Vec<&'a str>,
    name: Option<&'a str>,
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_name_char(c: char) -> bool {
    is_word_char(c) || c == '-'
}

fn take_run(text: &str, accept: impl Fn(char) -> bool) -> (&str, &str) {
    text.split_at(text.find(|c| !accept(c)).unwrap_or(text.len()))
}

/// `tag#id.a.b@nome`: as expressões regulares `SELECTOR_REGEX` e `H_REGEX` do original, que casam o prefixo e ignoram o resto.
fn parse_selector(description: &str) -> Selector<'_> {
    let (tag, mut rest) = take_run(description, is_name_char);
    let mut id = None;
    if let Some(after) = rest.strip_prefix('#') {
        let (run, tail) = take_run(after, is_name_char);
        if !run.is_empty() {
            id = Some(run);
            rest = tail;
        }
    }
    let mut classes = Vec::new();
    while let Some(after) = rest.strip_prefix('.') {
        let (run, tail) = take_run(after, is_name_char);
        if run.is_empty() {
            break;
        }
        classes.push(run);
        rest = tail;
    }
    let name = rest.strip_prefix('@').map(|after| take_run(after, is_word_char).0).filter(|run| !run.is_empty());
    Selector { tag, id, classes, name }
}

fn tag_or_div(tag: &str) -> &str {
    if tag.is_empty() { "div" } else { tag }
}

/// `$(description, attrs, ...children)`. Atributo com valor `None` (`undefined`) é pulado; `selected` só entra, como
/// `"true"`, quando o valor não é vazio.
pub fn create_el(document: &Rc<Document>, description: &str, attrs: &[(&str, Option<&str>)], children: &[Child<'_>]) -> Node {
    let selector = parse_selector(description);
    let result = document.create_element(tag_or_div(selector.tag));
    if let Some(id) = selector.id {
        result.set_id(id);
    }
    if !selector.classes.is_empty() {
        result.set_class_name(&selector.classes.join(" "));
    }
    for (name, value) in attrs {
        let Some(value) = value else { continue };
        let is_event_handler = name.strip_prefix("on").is_some_and(|rest| !rest.is_empty() && rest.chars().all(is_word_char));
        if is_event_handler {
            continue;
        }
        if *name == "selected" {
            if !value.is_empty() {
                result.set_attribute(name, "true");
            }
        } else {
            result.set_attribute(name, value);
        }
    }
    for child in children {
        match child {
            Child::Node(node) => {
                result.append_child(node);
            }
            Child::Text(text) => {
                result.append_text(text);
            }
        }
    }
    result
}

/// Um valor de `style` em `h`: número vira `px`.
pub enum StyleValue<'a> {
    Number(f64),
    Text(&'a str),
}

/// Um atributo de `h`.
pub enum HAttribute<'a> {
    ClassName(&'a str),
    Style(Vec<(&'a str, StyleValue<'a>)>),
    TabIndex(i32),
    /// Qualquer outro: `setAttribute(camelCaseToHyphenCase(chave), valor.toString())`.
    Other(&'a str, String),
}

/// O que `h` devolve: o `root` e cada elemento marcado com `@nome`, de todos os filhos.
pub struct HResult {
    pub root: Node,
    named: Vec<(String, Node)>,
}

impl HResult {
    /// `result[name]`; `root` é o elemento criado por esta chamada.
    pub fn get(&self, name: &str) -> Option<&Node> {
        if name == "root" { Some(&self.root) } else { self.named.iter().rev().find(|(candidate, _)| candidate == name).map(|(_, node)| node) }
    }
}

/// Um filho de `h`: elemento, texto ou o resultado de outro `h`.
pub enum HChild<'a> {
    Node(&'a Node),
    Text(&'a str),
    Result(&'a HResult),
}

fn camel_case_to_hyphen_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    for c in text.chars() {
        if previous.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
        previous = Some(c);
    }
    out
}

/// `h(tag, attributes, children)`: monta elementos aninhados.
pub fn h(document: &Rc<Document>, tag: &str, attributes: &[HAttribute<'_>], children: &[HChild<'_>]) -> HResult {
    let selector = parse_selector(tag);
    let el = document.create_element(tag_or_div(selector.tag));
    if let Some(id) = selector.id {
        el.set_id(id);
    }
    let mut class_names: Vec<&str> = selector.classes.clone();
    for attribute in attributes {
        if let HAttribute::ClassName(value) = attribute {
            class_names.extend(value.split('.').filter(|class_name| !class_name.is_empty()));
        }
    }
    if !class_names.is_empty() {
        el.set_class_name(&class_names.join(" "));
    }

    let mut named: Vec<(String, Node)> = Vec::new();
    if let Some(name) = selector.name {
        named.push((name.to_owned(), el.clone()));
    }
    for child in children {
        match child {
            HChild::Node(node) if node.is_html_element() => {
                el.append_child(node);
            }
            HChild::Node(_) => {}
            HChild::Text(text) => {
                el.append_text(text);
            }
            HChild::Result(result) => {
                named.extend(result.named.iter().cloned());
                el.append_child(&result.root);
            }
        }
    }
    for attribute in attributes {
        match attribute {
            HAttribute::ClassName(_) => {}
            HAttribute::Style(properties) => {
                for (css_key, css_value) in properties {
                    let value = match css_value {
                        StyleValue::Number(number) => format!("{number}px"),
                        StyleValue::Text(text) => (*text).to_owned(),
                    };
                    el.style_set_property(&camel_case_to_hyphen_case(css_key), &value);
                }
            }
            HAttribute::TabIndex(index) => el.set_attribute("tabindex", &index.to_string()),
            HAttribute::Other(key, value) => el.set_attribute(&camel_case_to_hyphen_case(key), value),
        }
    }
    HResult { root: el, named }
}

// ---------------------------------------------------------------------------------------------------------------------
// Classes e atributos
// ---------------------------------------------------------------------------------------------------------------------

/// O segundo argumento de `findParentWithClass`: parar numa classe ou num nó.
pub enum StopAt<'a> {
    Class(&'a str),
    Node(&'a Node),
}

/// `findParentWithClass(node, clazz, stopAtClazzOrNode)`: o nó ou o antepassado mais próximo que tem todas as classes.
pub fn find_parent_with_class(node: &Node, classes: &[&str], stop_at: Option<StopAt<'_>>) -> Option<Node> {
    let mut current = Some(node.clone());
    while let Some(candidate) = current {
        if !candidate.is_html_element() {
            return None;
        }
        if classes.iter().all(|class| candidate.class_list_contains(class)) {
            return Some(candidate);
        }
        let stop = match &stop_at {
            Some(StopAt::Class(class)) => candidate.class_list_contains(class),
            Some(StopAt::Node(stop_node)) => candidate == **stop_node,
            None => false,
        };
        if stop {
            return None;
        }
        current = candidate.parent_node();
    }
    None
}

/// `hasParentWithClass`.
pub fn has_parent_with_class(node: &Node, classes: &[&str], stop_at: Option<StopAt<'_>>) -> bool {
    find_parent_with_class(node, classes, stop_at).is_some()
}

/// `copyAttributes(from, to, filter)`.
pub fn copy_attributes(from: &Node, to: &Node, filter: Option<&[&str]>) {
    for (name, value) in from.attributes() {
        if filter.is_none_or(|names| names.contains(&name.as_str())) {
            to.set_attribute(&name, &value);
        }
    }
}

fn copy_attribute(from: &Node, to: &Node, name: &str) {
    match from.get_attribute(name) {
        Some(value) if !value.is_empty() => to.set_attribute(name, &value),
        _ => to.remove_attribute(name),
    }
}

/// `trackAttributes(from, to, filter)`: copia agora e depois acompanha as mudanças de `from` (entregues numa microtarefa).
pub fn track_attributes(from: &Node, to: &Node, filter: Option<&[&str]>) -> DisposableRef {
    copy_attributes(from, to, filter);
    let tracker = Rc::new(AttributeTracker {
        from: from.clone(),
        to: to.clone(),
        filter: filter.map(|names| names.iter().map(|name| (*name).to_owned()).collect()),
        pending: RefCell::new(Vec::new()),
        scheduled: Cell::new(false),
        disposed: Cell::new(false),
    });
    from.document.trackers.borrow_mut().push(tracker.clone());
    to_disposable(move || {
        tracker.disposed.set(true);
        tracker.from.document.trackers.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &tracker));
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Janelas
// ---------------------------------------------------------------------------------------------------------------------

/// Um evento de interface: só a `view` que o `getWindow` lê.
pub struct UiEvent {
    pub view: Option<Rc<CodeWindow>>,
}

/// O argumento de `getWindow` e `getDocument`: um nó ou um evento.
pub enum DomTarget<'a> {
    Node(&'a Node),
    Event(&'a UiEvent),
}

struct AnimationFrameQueueItem {
    runner: RefCell<Option<Box<dyn FnOnce()>>>,
    priority: i32,
    canceled: Cell<bool>,
}

impl AnimationFrameQueueItem {
    fn execute(&self) {
        if self.canceled.get() {
            return;
        }
        let runner = self.runner.borrow_mut().take();
        if let Some(runner) = runner {
            runner();
        }
    }
}

impl IDisposable for AnimationFrameQueueItem {
    fn dispose(&self) {
        self.canceled.set(true);
    }
}

#[derive(Default)]
struct FrameQueues {
    next: Vec<Rc<AnimationFrameQueueItem>>,
    current: Vec<Rc<AnimationFrameQueueItem>>,
    requested: bool,
    in_runner: bool,
}

/// O que um `ResizeObserver` nativo observa e solta.
pub trait ResizeObserverBackend {
    fn observe(&self, target: &Node);
    fn unobserve(&self, target: &Node);
    fn disconnect(&self);
}

/// Uma entrada de `ResizeObserverEntry`: o alvo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResizeObserverEntry {
    pub target: Node,
}

/// O callback que o `ResizeObserver` recebe; o `Err` é uma exceção lançada.
pub type ResizeObserverCallback = Rc<dyn Fn(&[ResizeObserverEntry]) -> Result<(), Error>>;

/// O construtor de `ResizeObserver` de uma janela (`targetWindow.ResizeObserver`), ou o injetado nas opções.
pub type ResizeObserverCtor = Rc<dyn Fn(Rc<dyn Fn(&[ResizeObserverEntry])>) -> Rc<dyn ResizeObserverBackend>>;

#[derive(Default)]
struct RecentResizeObserverContext {
    names: BTreeSet<String>,
    overflow: bool,
}

/// O manipulador de `setTimeout(handler, timeout, ...args)`: recebe os `args` do timer.
pub type TimerHandler = Rc<dyn Fn(&[Dynamic])>;

/// `window.setTimeout(handler, timeout, ...args)`: devolve o identificador do timer.
pub type SetTimeoutFn = Rc<dyn Fn(TimerHandler, u64, &[Dynamic]) -> i64>;

/// `window.clearTimeout(timeoutId)`.
pub type ClearTimeoutFn = Rc<dyn Fn(Option<i64>)>;

/// Os timers da janela: os que valem agora (`window.setTimeout`, atribuíveis) e os originais que
/// `enableMultiWindowAwareTimeout` define em `vscodeOriginalSetTimeout` e `vscodeOriginalClearTimeout`.
struct WindowTimers {
    set_timeout: SetTimeoutFn,
    clear_timeout: ClearTimeoutFn,
    original_set_timeout: Option<SetTimeoutFn>,
    original_clear_timeout: Option<ClearTimeoutFn>,
}

impl WindowTimers {
    /// Os timers nativos do `runtime` do thread.
    fn native() -> Self {
        Self {
            set_timeout: Rc::new(|handler, timeout, args| {
                let args = args.to_vec();
                runtime::set_timeout(move || handler(&args), timeout).raw() as i64
            }),
            clear_timeout: Rc::new(|timeout_id| {
                if let Some(timeout_id) = timeout_id {
                    runtime::clear_timeout(TimerId::from_raw(timeout_id as u64));
                }
            }),
            original_set_timeout: None,
            original_clear_timeout: None,
        }
    }
}

/// Uma janela de código: documento, id, quadros de animação, timers e contexto dos `ResizeObserver`.
pub struct CodeWindow {
    id: Cell<Option<u32>>,
    document: Rc<Document>,
    frames: RefCell<FrameQueues>,
    animation_frame_callbacks: RefCell<Vec<Box<dyn FnOnce()>>>,
    timers: RefCell<WindowTimers>,
    resize_observer_ctor: RefCell<Option<ResizeObserverCtor>>,
    recent_resize_observers: RefCell<Option<RecentResizeObserverContext>>,
}

impl CodeWindow {
    /// Uma janela sem id (`vscodeWindowId` indefinido): ver [`ensure_code_window`].
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|view| Self {
            id: Cell::new(None),
            document: Document::new(view.clone()),
            frames: RefCell::new(FrameQueues::default()),
            animation_frame_callbacks: RefCell::new(Vec::new()),
            timers: RefCell::new(WindowTimers::native()),
            resize_observer_ctor: RefCell::new(None),
            recent_resize_observers: RefCell::new(None),
        })
    }

    pub fn document(&self) -> Rc<Document> {
        self.document.clone()
    }

    /// `vscodeWindowId`.
    pub fn vscode_window_id(&self) -> Option<u32> {
        self.id.get()
    }

    /// `window.setTimeout(handler, timeout, ...args)`, o que estiver atribuído agora.
    pub fn set_timeout(&self, handler: impl Fn(&[Dynamic]) + 'static, timeout: u64, args: &[Dynamic]) -> i64 {
        let set_timeout = self.set_timeout_fn();
        set_timeout(Rc::new(handler), timeout, args)
    }

    /// `window.clearTimeout(timeoutId)`, o que estiver atribuído agora.
    pub fn clear_timeout(&self, timeout_id: Option<i64>) {
        let clear_timeout = self.clear_timeout_fn();
        clear_timeout(timeout_id);
    }

    /// O `window.setTimeout` de agora (para quem o guarda antes de sobrescrever).
    pub fn set_timeout_fn(&self) -> SetTimeoutFn {
        self.timers.borrow().set_timeout.clone()
    }

    /// O `window.clearTimeout` de agora.
    pub fn clear_timeout_fn(&self) -> ClearTimeoutFn {
        self.timers.borrow().clear_timeout.clone()
    }

    /// `window.setTimeout = ...; window.clearTimeout = ...`.
    pub fn assign_timeout_fns(&self, set_timeout: SetTimeoutFn, clear_timeout: ClearTimeoutFn) {
        let mut timers = self.timers.borrow_mut();
        timers.set_timeout = set_timeout;
        timers.clear_timeout = clear_timeout;
    }

    /// `Object.defineProperty(window, 'vscodeOriginalSetTimeout' | 'vscodeOriginalClearTimeout', { get })`.
    pub fn define_original_timeout_fns(&self, set_timeout: SetTimeoutFn, clear_timeout: ClearTimeoutFn) {
        let mut timers = self.timers.borrow_mut();
        timers.original_set_timeout = Some(set_timeout);
        timers.original_clear_timeout = Some(clear_timeout);
    }

    /// `window.vscodeOriginalSetTimeout`: indefinido enquanto ninguém o define.
    pub fn vscode_original_set_timeout(&self) -> Option<SetTimeoutFn> {
        self.timers.borrow().original_set_timeout.clone()
    }

    /// `window.vscodeOriginalClearTimeout`.
    pub fn vscode_original_clear_timeout(&self) -> Option<ClearTimeoutFn> {
        self.timers.borrow().original_clear_timeout.clone()
    }

    /// Instala o `ResizeObserver` da janela.
    pub fn set_resize_observer_ctor(&self, ctor: ResizeObserverCtor) {
        *self.resize_observer_ctor.borrow_mut() = Some(ctor);
    }

    /// `requestAnimationFrame(callback)`: roda no próximo [`CodeWindow::render_frame`].
    pub fn request_animation_frame(&self, callback: impl FnOnce() + 'static) {
        self.animation_frame_callbacks.borrow_mut().push(Box::new(callback));
    }

    /// O quadro: roda os callbacks pedidos até aqui, na ordem; o que eles pedirem fica para o quadro seguinte.
    pub fn render_frame(&self) {
        let callbacks = std::mem::take(&mut *self.animation_frame_callbacks.borrow_mut());
        for callback in callbacks {
            callback();
        }
    }

    /// `scheduleAtNextAnimationFrame(window, runner, priority)`.
    pub fn schedule_at_next_animation_frame(self: &Rc<Self>, runner: impl FnOnce() + 'static, priority: i32) -> DisposableRef {
        let item = Rc::new(AnimationFrameQueueItem { runner: RefCell::new(Some(Box::new(runner))), priority, canceled: Cell::new(false) });
        let needs_request = {
            let mut frames = self.frames.borrow_mut();
            frames.next.push(item.clone());
            !std::mem::replace(&mut frames.requested, true)
        };
        if needs_request {
            let window = Rc::downgrade(self);
            self.request_animation_frame(move || {
                if let Some(window) = window.upgrade() {
                    window.run_animation_frame();
                }
            });
        }
        item
    }

    /// `runAtThisOrScheduleAtNextAnimationFrame`: dentro de um quadro, entra na fila do quadro corrente.
    pub fn run_at_this_or_schedule_at_next_animation_frame(self: &Rc<Self>, runner: impl FnOnce() + 'static, priority: i32) -> DisposableRef {
        if self.frames.borrow().in_runner {
            let item = Rc::new(AnimationFrameQueueItem { runner: RefCell::new(Some(Box::new(runner))), priority, canceled: Cell::new(false) });
            self.frames.borrow_mut().current.push(item.clone());
            item
        } else {
            self.schedule_at_next_animation_frame(runner, priority)
        }
    }

    fn run_animation_frame(&self) {
        {
            let mut frames = self.frames.borrow_mut();
            if !frames.requested {
                return;
            }
            frames.requested = false;
            frames.current = std::mem::take(&mut frames.next);
            frames.in_runner = true;
        }
        loop {
            let top = {
                let mut frames = self.frames.borrow_mut();
                if frames.current.is_empty() {
                    break;
                }
                // `sort` é estável, como o do JS: empate de prioridade mantém a ordem.
                frames.current.sort_by(|a, b| b.priority.cmp(&a.priority));
                frames.current.remove(0)
            };
            top.execute();
        }
        self.frames.borrow_mut().in_runner = false;
    }

    fn reset_frames(&self) {
        *self.frames.borrow_mut() = FrameQueues::default();
    }
}

/// `ensureCodeWindow(window, fallbackWindowId)`: dá o id à janela que ainda não tem.
pub fn ensure_code_window(target_window: &CodeWindow, fallback_window_id: u32) {
    if target_window.id.get().is_none() {
        target_window.id.set(Some(fallback_window_id));
    }
}

thread_local! {
    static MAIN_WINDOW: Rc<CodeWindow> = {
        let window = CodeWindow::new();
        ensure_code_window(&window, 1);
        window
    };
    static WINDOWS: RefCell<BTreeMap<u32, Rc<CodeWindow>>> = RefCell::new(BTreeMap::from([(1, main_window())]));
}

/// `mainWindow`.
pub fn main_window() -> Rc<CodeWindow> {
    MAIN_WINDOW.with(Rc::clone)
}

/// `isAuxiliaryWindow(window)`.
pub fn is_auxiliary_window(target_window: &Rc<CodeWindow>) -> bool {
    !Rc::ptr_eq(target_window, &main_window()) && target_window.vscode_window_id().is_some()
}

/// `registerWindow(window)`: a janela precisa ter id. Já registrada, não faz nada.
pub fn register_window(target_window: &Rc<CodeWindow>) -> DisposableRef {
    let id = target_window.vscode_window_id().expect("registerWindow exige uma janela com vscodeWindowId");
    if has_window(id) {
        return none();
    }
    WINDOWS.with(|windows| windows.borrow_mut().insert(id, target_window.clone()));
    let window = target_window.clone();
    to_disposable(move || {
        WINDOWS.with(|windows| windows.borrow_mut().remove(&id));
        window.reset_frames();
    })
}

/// `getWindows()`.
pub fn get_windows() -> Vec<Rc<CodeWindow>> {
    WINDOWS.with(|windows| windows.borrow().values().cloned().collect())
}

/// `getWindowsCount()`.
pub fn get_windows_count() -> usize {
    WINDOWS.with(|windows| windows.borrow().len())
}

/// `getWindowId(window)`.
pub fn get_window_id(target_window: &CodeWindow) -> Option<u32> {
    target_window.vscode_window_id()
}

/// `hasWindow(windowId)`.
pub fn has_window(window_id: u32) -> bool {
    WINDOWS.with(|windows| windows.borrow().contains_key(&window_id))
}

/// `getWindowById(windowId)`.
pub fn get_window_by_id(window_id: u32) -> Option<Rc<CodeWindow>> {
    WINDOWS.with(|windows| windows.borrow().get(&window_id).cloned())
}

/// `getWindowById(windowId, true)`: sem id, ou com id desconhecido, a janela principal.
pub fn get_window_by_id_or_main(window_id: Option<u32>) -> Rc<CodeWindow> {
    window_id.and_then(get_window_by_id).unwrap_or_else(main_window)
}

/// `getWindow(e)`: a janela do documento do nó, a `view` do evento, ou a principal.
pub fn get_window(target: Option<DomTarget<'_>>) -> Rc<CodeWindow> {
    let window = match target {
        Some(DomTarget::Node(node)) => node.document.default_view(),
        Some(DomTarget::Event(event)) => event.view.clone(),
        None => None,
    };
    window.unwrap_or_else(main_window)
}

/// `getDocument(e)`.
pub fn get_document(target: Option<DomTarget<'_>>) -> Rc<Document> {
    get_window(target).document()
}

// ---------------------------------------------------------------------------------------------------------------------
// Timers e quadros
// ---------------------------------------------------------------------------------------------------------------------

/// `disposableWindowInterval(window, handler, interval, iterations)`: o `handler` devolve `true` para parar. Com
/// `iterations`, para na iteração de número `iterations` sem chamar o `handler`.
pub fn disposable_window_interval(handler: impl Fn() -> bool + 'static, interval: u64, iterations: Option<u64>) -> DisposableRef {
    let timer: Rc<RefCell<Option<IntervalHandle>>> = Rc::new(RefCell::new(None));
    let stop: Rc<dyn Fn()> = {
        let timer = timer.clone();
        Rc::new(move || {
            let handle = timer.borrow_mut().take();
            if let Some(handle) = handle {
                clear_interval(&handle);
            }
        })
    };
    let iteration = Cell::new(0u64);
    let tick_stop = stop.clone();
    let handle = set_interval(
        move || {
            iteration.set(iteration.get() + 1);
            if iterations.is_some_and(|limit| iteration.get() >= limit) || handler() {
                tick_stop();
            }
        },
        interval,
    );
    *timer.borrow_mut() = Some(handle);
    to_disposable(move || stop())
}

/// `AnimationFrameScheduler`: junta vários `schedule()` num só callback no próximo quadro.
pub struct AnimationFrameScheduler {
    runner: Rc<dyn Fn()>,
    window: Rc<CodeWindow>,
    pending_runner: Rc<MutableDisposable<dyn IDisposable>>,
}

impl AnimationFrameScheduler {
    /// A janela é a do nó (`getWindow(node)`).
    pub fn new(node: &Node, runner: impl Fn() + 'static) -> Self {
        Self { runner: Rc::new(runner), window: get_window(Some(DomTarget::Node(node))), pending_runner: MutableDisposable::new() }
    }

    /// Cancela o que estiver agendado.
    pub fn cancel(&self) {
        self.pending_runner.clear();
    }

    /// Agenda para o próximo quadro; já agendado, mantém o agendamento. Dentro de um quadro, roda nele.
    pub fn schedule(&self) {
        if self.pending_runner.value().is_some() {
            return;
        }
        let pending = self.pending_runner.clone();
        let runner = self.runner.clone();
        let item = self.window.run_at_this_or_schedule_at_next_animation_frame(
            move || {
                pending.clear();
                runner();
            },
            0,
        );
        self.pending_runner.set(Some(item));
    }

    pub fn is_scheduled(&self) -> bool {
        self.pending_runner.value().is_some()
    }
}

impl IDisposable for AnimationFrameScheduler {
    fn dispose(&self) {
        self.pending_runner.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ResizeObserver
// ---------------------------------------------------------------------------------------------------------------------

/// Limite do contexto, para um quadro grande não gerar um valor de telemetria sem limite.
const MAX_RECENT_DISPOSABLE_RESIZE_OBSERVERS: usize = 8;

fn record_disposable_resize_observer_invocation(target_window: &Rc<CodeWindow>, name: &str) {
    let first_in_frame = target_window.recent_resize_observers.borrow().is_none();
    if first_in_frame {
        *target_window.recent_resize_observers.borrow_mut() = Some(RecentResizeObserverContext::default());
        // Os callbacks de `ResizeObserver` e o aviso de laço chegam depois dos de `requestAnimationFrame`: um pedido feito
        // aqui limpa o contexto no quadro seguinte, depois de a telemetria ver o aviso do quadro atual.
        let window = Rc::downgrade(target_window);
        target_window.request_animation_frame(move || {
            if let Some(window) = window.upgrade() {
                *window.recent_resize_observers.borrow_mut() = None;
            }
        });
    }
    let mut guard = target_window.recent_resize_observers.borrow_mut();
    let Some(context) = guard.as_mut() else { return };
    if context.names.contains(name) {
        return;
    }
    if context.names.len() < MAX_RECENT_DISPOSABLE_RESIZE_OBSERVERS {
        context.names.insert(name.to_owned());
    } else {
        context.overflow = true;
        if let Some(largest) = context.names.iter().next_back().cloned() {
            if name < largest.as_str() {
                context.names.remove(&largest);
                context.names.insert(name.to_owned());
            }
        }
    }
}

/// `DisposableResizeObserver`: o `callback` roda de forma síncrona com as entradas entregues; a exceção dele vai para
/// `onUnexpectedError`.
pub struct DisposableResizeObserver {
    base: Rc<Disposable>,
    observer: Rc<dyn ResizeObserverBackend>,
    pub name: String,
}

impl DisposableResizeObserver {
    /// `resize_observer_ctor` é o `options.resizeObserverCtor`; sem ele vale o da janela, e sem nenhum dos dois é um `TypeError`.
    pub fn new(name: &str, callback: ResizeObserverCallback, target_window: &Rc<CodeWindow>, resize_observer_ctor: Option<ResizeObserverCtor>) -> Result<Self, Error> {
        let ctor = resize_observer_ctor
            .or_else(|| target_window.resize_observer_ctor.borrow().clone())
            .ok_or_else(|| Error::type_error("ResizeObserver is not available in this window"))?;
        let window = Rc::downgrade(target_window);
        let observer_name = name.to_owned();
        let observer = ctor(Rc::new(move |entries: &[ResizeObserverEntry]| {
            if let Some(window) = window.upgrade() {
                record_disposable_resize_observer_invocation(&window, &observer_name);
            }
            if let Err(error) = callback(entries) {
                on_unexpected_error(&error);
            }
        }));
        let base = Disposable::new();
        let to_disconnect = observer.clone();
        base.register(to_disposable(move || to_disconnect.disconnect()));
        Ok(Self { base, observer, name: name.to_owned() })
    }

    /// `observe(target)`: devolve o que para de observar o alvo.
    pub fn observe(&self, target: &Node) -> DisposableRef {
        self.observer.observe(target);
        let observer = self.observer.clone();
        let target = target.clone();
        to_disposable(move || observer.unobserve(&target))
    }
}

impl IDisposable for DisposableResizeObserver {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `getRecentDisposableResizeObserverContextForLoopError(message, targetWindow)`: se a mensagem é o aviso de laço do
/// `ResizeObserver`, o contexto estável dos observadores embrulhados que rodaram há pouco na janela (`None` é a principal).
pub fn get_recent_disposable_resize_observer_context_for_loop_error(message: Option<&str>, target_window: Option<&CodeWindow>) -> Option<String> {
    let message = message.filter(|text| text.contains("ResizeObserver loop"))?;
    let read = |window: &CodeWindow| {
        window.recent_resize_observers.borrow().as_ref().map(|context| {
            let mut names: Vec<&str> = context.names.iter().map(String::as_str).collect();
            if context.overflow {
                names.push("<overflow>");
            }
            format!("[ResizeObserverLoopContext({})] {message}", names.join(","))
        })
    };
    match target_window {
        Some(window) => read(window),
        None => read(&main_window()),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// SafeTriangle
// ---------------------------------------------------------------------------------------------------------------------

/// O que o `SafeTriangle` lê de `getBoundingClientRect()`.
#[derive(Clone, Copy, Debug)]
pub struct ClientRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// A conversão de uma atribuição a um `Int16Array`: trunca e dá a volta módulo 65536.
fn to_int16(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let wrapped = value.trunc().rem_euclid(65536.0);
    if wrapped >= 32768.0 { wrapped - 65536.0 } else { wrapped }
}

/// O "triângulo seguro" que um hover ocupa para não ser dispensado cedo: os quatro triângulos da origem para as arestas do alvo.
pub struct SafeTriangle {
    /// Quatro pontos (x, y), já como `Int16Array`.
    points: [f64; 8],
    origin_x: f64,
    origin_y: f64,
}

impl SafeTriangle {
    pub fn new(origin_x: f64, origin_y: f64, target: ClientRect) -> Self {
        let ClientRect { left, right, top, bottom } = target;
        let points = [left, top, right, top, left, bottom, right, bottom].map(to_int16);
        Self { points, origin_x, origin_y }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        (0..4).any(|i| {
            let p1 = 2 * i;
            let p2 = 2 * ((i + 1) % 4);
            is_point_within_triangle(x, y, self.origin_x, self.origin_y, self.points[p1], self.points[p1 + 1], self.points[p2], self.points[p2 + 1])
        })
    }
}
