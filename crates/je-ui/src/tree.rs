//! A árvore de nós, numa arena.

use std::collections::HashMap;

use je_render::kurbo::{BezPath, Rect, RoundedRect, RoundedRectRadii, Shape as _, Vec2};
use je_render::{DisplayList, Rgba, Shape};
use je_text::{TextStyle, with_text_system};

use crate::paragraph::{Paragraph, SpanFrame};

/// Aplica a opacidade ao alfa da cor.
fn fade(color: Rgba, opacity: f32) -> Rgba {
    if opacity >= 1.0 {
        return color;
    }
    Rgba { a: (f32::from(color.a) * opacity).round() as u8, ..color }
}

/// O índice de um nó na arena, com a geração do espaço: um `NodeId` de um nó removido não vale para o nó
/// que depois reutiliza o espaço (o elemento removido do DOM continua sendo outro objeto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32, u32);

/// Borda de largura uniforme (o caso de todas as bordas do workbench).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f64,
    pub color: Rgba,
}

/// O papel de acessibilidade do nó (o `role` do ARIA).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Generic,
    Button,
    Toolbar,
    Tab,
    TabList,
    Tree,
    TreeItem,
    List,
    ListItem,
    TextInput,
    Menu,
    MenuBar,
    MenuItem,
    Status,
    Region,
    Separator,
    Checkbox,
    Radio,
    RadioGroup,
    Switch,
    Dialog,
    AlertDialog,
    ProgressBar,
    ComboBox,
    ListBox,
    Option,
    Link,
    Image,
    Group,
    Heading,
    Presentation,
    MenuItemCheckbox,
    MenuItemRadio,
    Tooltip,
    Alert,
}

/// Os nomes ARIA dos papéis (o valor do atributo `role`), na ordem do enum. `Generic` não tem nome: é o
/// elemento sem o atributo.
const ROLE_NAMES: [(Role, &str); 34] = [
    (Role::Button, "button"),
    (Role::Toolbar, "toolbar"),
    (Role::Tab, "tab"),
    (Role::TabList, "tablist"),
    (Role::Tree, "tree"),
    (Role::TreeItem, "treeitem"),
    (Role::List, "list"),
    (Role::ListItem, "listitem"),
    (Role::TextInput, "textbox"),
    (Role::Menu, "menu"),
    (Role::MenuBar, "menubar"),
    (Role::MenuItem, "menuitem"),
    (Role::Status, "status"),
    (Role::Region, "region"),
    (Role::Separator, "separator"),
    (Role::Checkbox, "checkbox"),
    (Role::Radio, "radio"),
    (Role::RadioGroup, "radiogroup"),
    (Role::Switch, "switch"),
    (Role::Dialog, "dialog"),
    (Role::AlertDialog, "alertdialog"),
    (Role::ProgressBar, "progressbar"),
    (Role::ComboBox, "combobox"),
    (Role::ListBox, "listbox"),
    (Role::Option, "option"),
    (Role::Link, "link"),
    (Role::Image, "img"),
    (Role::Group, "group"),
    (Role::Heading, "heading"),
    (Role::Presentation, "presentation"),
    (Role::MenuItemCheckbox, "menuitemcheckbox"),
    (Role::MenuItemRadio, "menuitemradio"),
    (Role::Tooltip, "tooltip"),
    (Role::Alert, "alert"),
];

impl Role {
    /// O valor do atributo `role` do ARIA para este papel; `None` para `Generic` (sem o atributo).
    pub fn aria_name(self) -> Option<&'static str> {
        ROLE_NAMES.iter().find(|(role, _)| *role == self).map(|(_, name)| *name)
    }

    /// O papel de um valor do atributo `role` (`None` se o nome não é um papel que o `je-ui` conhece).
    pub fn from_aria_name(name: &str) -> Option<Role> {
        ROLE_NAMES.iter().find(|(_, candidate)| *candidate == name).map(|(role, _)| *role)
    }
}

/// O `pointer-events` do CSS. É herdado: `None` num nó vale para os descendentes, até um que declare
/// `Auto` (o `pointer-events: initial` do CSS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerEvents {
    #[default]
    Inherit,
    Auto,
    None,
}

/// O cursor do mouse sobre o nó (o `cursor` do CSS, só os valores que o workbench usa).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    Text,
    ColResize,
    RowResize,
    EwResize,
    NsResize,
    EResize,
    WResize,
    NResize,
    SResize,
    NwseResize,
    NeswResize,
    AllScroll,
}

/// O conteúdo próprio de um nó, pintado depois do fundo e antes dos filhos.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    None,
    /// Texto numa linha, com a origem no canto superior esquerdo da caixa de linha (`line_height`),
    /// centrado verticalmente nela como o Chromium centra a linha de texto.
    Text { text: String, style: TextStyle, color: Rgba, line_height: f64 },
    /// Um parágrafo: trechos com estilo, cor e link, quebrados na largura da caixa do nó pelo motor de texto
    /// (ver [`Paragraph`]). O [`Tree::text_content`] devolve o texto concatenado dos trechos.
    Paragraph(Paragraph),
    /// Um codicon centrado no nó, no tamanho dado.
    Codicon { id: String, size: f32, color: Rgba },
    /// Conteúdo pintado por quem monta a árvore, na ordem de pintura do nó (o `<canvas>` do VS Code: o
    /// editor, o terminal). O número identifica a superfície para o [`Tree::paint_hosted`].
    Host(u32),
}

/// O `color` do CSS de um nó: a cor do texto e dos codicons dele, herdada pelos descendentes que não declaram a
/// própria. A resolução é [`Tree::resolved_foreground`], que a pintura e a leitura ([`Tree::text_color`]) usam.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Foreground {
    /// Sem declaração (o padrão): o conteúdo do nó pinta com a cor própria que traz ([`Content::Text`],
    /// [`Content::Codicon`]). Para um descendente que herda, o nó é transparente: a cor vem do antepassado seguinte.
    #[default]
    Content,
    /// `color: <cor>`.
    Color(Rgba),
    /// `color: inherit`: a cor do pai, resolvida do mesmo modo. Sem nenhuma declaração acima, vale a cor própria do conteúdo.
    Inherit,
    /// `color: <cor>`, e `color: inherit` quando algum antepassado tem a classe `class` (a regra de descendente
    /// `.<class> .<nó> { color: inherit }`, como o `.monaco-list:focus .selected .reference-file` do peek).
    ColorUnlessUnder { color: Rgba, class: &'static str },
}

/// O estilo resolvido de um nó.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeStyle {
    pub background: Option<Rgba>,
    /// O `color` do CSS (ver [`Foreground`]).
    pub foreground: Foreground,
    pub border: Option<Border>,
    pub radius: f64,
    /// `overflow: hidden`: recorta os filhos ao retângulo (com o raio).
    pub clip: bool,
    /// `display: none` quando falso: o nó e a subárvore não pintam nem recebem ponteiro.
    pub visible: bool,
    /// `pointer-events`, herdado como no CSS.
    pub pointer_events: PointerEvents,
    /// O `z-index` entre irmãos: maior pinta por cima; empate fica na ordem do DOM.
    pub z_index: i32,
    /// O `cursor`: `None` herda do pai, como o CSS.
    pub cursor: Option<Cursor>,
    /// O `opacity` do CSS, de 0 a 1, acumulado com o dos antepassados. É a aproximação por alfa de cada
    /// cor pintada (sem camada de composição), exata para o que o workbench faz: esmaecer uma barra de
    /// rolagem sem sobreposição interna.
    pub opacity: f32,
    pub content: Content,
    /// Raio por canto (superior esquerdo, superior direito, inferior direito, inferior esquerdo), no
    /// lugar de `radius` (o `border-radius` com quatro valores).
    pub corner_radii: Option<[f64; 4]>,
    /// Largura da borda por lado (cima, direita, baixo, esquerda), no lugar de `Border::width` (o
    /// `border-width` com quatro valores; `Border::color` continua valendo para todos).
    pub border_widths: Option<[f64; 4]>,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self { background: None, foreground: Foreground::Content, border: None, radius: 0.0, clip: false, visible: true, pointer_events: PointerEvents::Inherit, z_index: 0, cursor: None, opacity: 1.0, content: Content::None, corner_radii: None, border_widths: None }
    }
}

struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    /// Retângulo relativo ao pai, em pixels lógicos (o `top/left/width/height` do `style`).
    bounds: Rect,
    /// `width: 100%` e `height: 100%`: a dimensão vira a do pai e ignora a do `bounds`.
    fill: (bool, bool),
    /// `right` e `bottom` do CSS: distância da borda de trás do nó à borda de trás do pai. Quando
    /// presentes, no lugar de `left`/`top` do `bounds`.
    right: Option<f64>,
    bottom: Option<f64>,
    style: NodeStyle,
    role: Role,
    label: Option<String>,
    /// O `classList`: nomes de classe, sem estilo (a regra de CSS vira código do widget), mas o
    /// código do VS Code lê e escreve classes como estado e como marcador de estrutura.
    classes: Vec<String>,
    /// Os atributos do elemento que o `je-ui` não modela em campo próprio (`aria-checked`, `aria-disabled`,
    /// `disabled`, `data-*`, `tabindex`, `draggable`...). `aria-label` e `role` não ficam aqui: são o
    /// rótulo e o papel do nó (ver [`Tree::attribute`]).
    attributes: HashMap<String, String>,
    alive: bool,
    /// Num nó `Role::Link` filho de um parágrafo, o número do link no parágrafo ([`crate::Span::link`]).
    link: Option<u32>,
    /// Quantas vezes o espaço da arena já foi reutilizado.
    generation: u32,
}

/// A árvore de UI de uma janela.
pub struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    root: NodeId,
    /// O nó com o foco do teclado (espelho do [`crate::Ui::focused`]), para a pintura do contorno de foco.
    focused: Option<NodeId>,
}

impl Node {
    fn new(parent: Option<NodeId>, role: Role) -> Self {
        Self { parent, children: Vec::new(), bounds: Rect::ZERO, fill: (false, false), right: None, bottom: None, style: NodeStyle::default(), role, label: None, classes: Vec::new(), attributes: HashMap::new(), alive: true, link: None, generation: 0 }
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        let root = Node::new(None, Role::Region);
        Self { nodes: vec![root], free: Vec::new(), root: NodeId(0, 0), focused: None }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    /// Cria um nó como último filho de `parent` (o `append` do DOM).
    pub fn create(&mut self, parent: NodeId, role: Role) -> NodeId {
        let id = self.alloc(Node::new(Some(parent), role));
        self.node_mut(parent).children.push(id);
        id
    }

    /// Cria um nó solto, sem pai (o `document.createElement`). Entra na árvore com
    /// [`Tree::append_child`] ou [`Tree::insert_before`].
    pub fn create_detached(&mut self, role: Role) -> NodeId {
        self.alloc(Node::new(None, role))
    }

    fn alloc(&mut self, node: Node) -> NodeId {
        match self.free.pop() {
            Some(slot) => {
                let generation = self.nodes[slot as usize].generation + 1;
                self.nodes[slot as usize] = Node { generation, ..node };
                NodeId(slot, generation)
            }
            None => {
                self.nodes.push(node);
                NodeId(u32::try_from(self.nodes.len() - 1).expect("mais de 4 bilhões de nós"), 0)
            }
        }
    }

    /// Tira o nó do pai sem destruí-lo (o `element.remove()`): a subárvore continua viva e pode voltar.
    pub fn detach(&mut self, id: NodeId) {
        if !self.is_alive(id) {
            return;
        }
        if let Some(parent) = self.node_mut(id).parent.take() {
            self.node_mut(parent).children.retain(|c| *c != id);
        }
    }

    /// Põe `child` como último filho de `parent`, tirando-o do pai anterior (o `appendChild`).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.insert_before(parent, child, None);
    }

    /// Põe `child` em `parent` antes de `reference` (no fim, se `None`), como o `insertBefore`.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        assert!(child != self.root && !self.is_ancestor(child, parent), "ciclo na árvore de nós");
        self.detach(child);
        let index = reference.and_then(|r| self.node(parent).children.iter().position(|c| *c == r)).unwrap_or(self.node(parent).children.len());
        self.node_mut(parent).children.insert(index, child);
        self.node_mut(child).parent = Some(parent);
    }

    /// `ancestor` é o próprio `id` ou um dos antepassados dele.
    pub fn is_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(n) = current {
            if n == ancestor {
                return true;
            }
            current = self.node(n).parent;
        }
        false
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn is_alive(&self, id: NodeId) -> bool {
        self.nodes.get(id.0 as usize).is_some_and(|n| n.alive && n.generation == id.1)
    }

    /// O nó e todos os descendentes, em pré-ordem.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.node(n).children.iter().rev());
        }
        out
    }

    /// O `textContent` do nó: o texto de [`Content::Text`] e dos parágrafos de toda a subárvore, em pré-ordem,
    /// concatenado.
    pub fn text_content(&self, id: NodeId) -> String {
        self.subtree(id)
            .into_iter()
            .map(|node| match &self.node(node).style.content {
                Content::Text { text, .. } => text.clone(),
                Content::Paragraph(paragraph) => paragraph.text_content(),
                _ => String::new(),
            })
            .collect()
    }

    /// O link do parágrafo do nó no ponto (`x`, `y`) da janela, se o nó tem [`Content::Paragraph`] e o ponto cai
    /// sobre um trecho de link (ver [`Paragraph::link_at`]).
    pub fn paragraph_link_at(&self, id: NodeId, x: f64, y: f64) -> Option<u32> {
        let Content::Paragraph(paragraph) = &self.node(id).style.content else { return None };
        let rect = self.absolute_bounds(id);
        paragraph.link_at(rect.width(), x - rect.x0, y - rect.y0)
    }

    /// Os nós de link (`Role::Link`) do parágrafo, com o número de cada um, na ordem dos filhos.
    pub fn paragraph_link_nodes(&self, paragraph: NodeId) -> Vec<(u32, NodeId)> {
        self.node(paragraph).children.iter().filter_map(|child| self.node(*child).link.map(|link| (link, *child))).collect()
    }

    /// O número do link no parágrafo, se o nó é um nó de link criado por [`crate::Ui::sync_paragraph_links`].
    pub fn link_index(&self, node: NodeId) -> Option<u32> {
        self.node(node).link
    }

    /// Cria o nó de link do parágrafo: `Role::Link`, com o nome acessível, sem caixa (retângulo vazio) e sem
    /// ponteiro, porque o clique continua sendo do parágrafo ([`Tree::paragraph_link_at`]).
    pub(crate) fn add_paragraph_link(&mut self, paragraph: NodeId, link: u32, name: String) -> NodeId {
        let id = self.create(paragraph, Role::Link);
        let node = self.node_mut(id);
        node.link = Some(link);
        node.label = Some(name);
        node.style.pointer_events = PointerEvents::None;
        id
    }

    /// As caixas do nó de link em coordenadas da janela, uma por linha que o link ocupa (ver
    /// [`Paragraph::link_boxes`]). Vazio se o nó não é um link de parágrafo.
    pub fn link_boxes(&self, node: NodeId) -> Vec<Rect> {
        let (Some(link), Some(parent)) = (self.node(node).link, self.node(node).parent) else { return Vec::new() };
        let Content::Paragraph(paragraph) = &self.node(parent).style.content else { return Vec::new() };
        let rect = self.absolute_bounds(parent);
        paragraph.link_boxes(rect.width(), link).into_iter().map(|b| b + Vec2::new(rect.x0, rect.y0)).collect()
    }

    /// A ordem de Tab: os nós com `tabindex` positivo em ordem crescente (empate na ordem da árvore), depois os de
    /// `tabindex` zero e os links de parágrafo na ordem da árvore. Fora ficam o `tabindex` negativo, o atributo
    /// `disabled` e a subárvore de um nó oculto (`display: none`).
    pub fn tab_order(&self) -> Vec<NodeId> {
        let mut positive: Vec<(i32, NodeId)> = Vec::new();
        let mut natural = Vec::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let node = self.node(id);
            if !node.style.visible {
                continue;
            }
            let tab_index = node.attributes.get("tabindex").and_then(|value| value.trim().parse::<i32>().ok()).or(node.link.map(|_| 0));
            if !node.attributes.contains_key("disabled") {
                match tab_index {
                    Some(index) if index > 0 => positive.push((index, id)),
                    Some(0) => natural.push(id),
                    _ => {}
                }
            }
            stack.extend(node.children.iter().rev());
        }
        positive.sort_by_key(|(index, _)| *index);
        positive.into_iter().map(|(_, id)| id).chain(natural).collect()
    }

    /// Guarda o nó com o foco (chamado pelo [`crate::Ui`]).
    pub(crate) fn set_focused(&mut self, node: Option<NodeId>) {
        self.focused = node;
    }

    /// O número do link do parágrafo que tem o foco, se o foco está num nó de link filho dele.
    fn focused_link_of(&self, paragraph: NodeId) -> Option<u32> {
        let focused = self.focused.filter(|f| self.is_alive(*f))?;
        let node = self.node(focused);
        node.parent.filter(|parent| *parent == paragraph).and(node.link)
    }

    /// Remove o nó e a subárvore dele (o `remove` do DOM).
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root || !self.is_alive(id) {
            return;
        }
        if let Some(parent) = self.node_mut(id).parent.take() {
            self.node_mut(parent).children.retain(|c| *c != id);
        }
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            let node = self.node_mut(n);
            node.alive = false;
            stack.extend(std::mem::take(&mut node.children));
            self.free.push(n.0);
        }
        if self.focused.is_some_and(|f| !self.is_alive(f)) {
            self.focused = None;
        }
    }

    /// O retângulo explícito do nó, relativo ao pai. Dimensão marcada com [`Tree::set_fill`] e posição
    /// marcada com [`Tree::set_right`] ou [`Tree::set_bottom`] valem o que o CSS valeria.
    pub fn set_bounds(&mut self, id: NodeId, bounds: Rect) {
        self.node_mut(id).bounds = bounds;
    }

    /// `width: 100%` (e/ou `height: 100%`): a dimensão passa a ser a do pai, resolvida a cada leitura.
    pub fn set_fill(&mut self, id: NodeId, width: bool, height: bool) {
        self.node_mut(id).fill = (width, height);
    }

    /// `right: v`: ancora a borda direita a `v` da borda direita do pai (no lugar do `x0` do `bounds`).
    pub fn set_right(&mut self, id: NodeId, right: Option<f64>) {
        self.node_mut(id).right = right;
    }

    /// `bottom: v`: ancora a borda de baixo a `v` da borda de baixo do pai (no lugar do `y0`).
    pub fn set_bottom(&mut self, id: NodeId, bottom: Option<f64>) {
        self.node_mut(id).bottom = bottom;
    }

    /// O retângulo resolvido do nó, relativo ao pai.
    pub fn bounds(&self, id: NodeId) -> Rect {
        let size = self.node(id).parent.map_or((0.0, 0.0), |p| {
            let r = self.bounds(p);
            (r.width(), r.height())
        });
        self.resolve(id, size)
    }

    fn resolve(&self, id: NodeId, parent_size: (f64, f64)) -> Rect {
        let node = self.node(id);
        let width = if node.fill.0 { parent_size.0 } else { node.bounds.width() };
        let height = if node.fill.1 { parent_size.1 } else { node.bounds.height() };
        let x0 = node.right.map_or(node.bounds.x0, |r| parent_size.0 - r - width);
        let y0 = node.bottom.map_or(node.bounds.y0, |b| parent_size.1 - b - height);
        Rect::new(x0, y0, x0 + width, y0 + height)
    }

    pub fn style_mut(&mut self, id: NodeId) -> &mut NodeStyle {
        &mut self.node_mut(id).style
    }

    pub fn set_label(&mut self, id: NodeId, label: impl Into<String>) {
        self.node_mut(id).label = Some(label.into());
    }

    pub fn label(&self, id: NodeId) -> Option<&str> {
        self.node(id).label.as_deref()
    }

    /// Troca o papel de acessibilidade (o `setAttribute('role', ...)` e o `removeAttribute('role')`).
    pub fn set_role(&mut self, id: NodeId, role: Role) {
        self.node_mut(id).role = role;
    }

    pub fn clear_label(&mut self, id: NodeId) {
        self.node_mut(id).label = None;
    }

    /// `setAttribute(name, value)`. `aria-label` é o rótulo do nó ([`Tree::set_label`]) e `role` o papel
    /// ([`Tree::set_role`], quando o nome é um papel conhecido); os demais (`aria-checked`, `disabled`,
    /// `data-index`...) ficam guardados no nó, como texto, e vão para a árvore de acessibilidade com ele.
    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: impl Into<String>) {
        let value = value.into();
        match name {
            "aria-label" => self.set_label(id, value),
            "role" => match Role::from_aria_name(&value) {
                Some(role) => self.set_role(id, role),
                None => {
                    self.node_mut(id).attributes.insert(name.to_owned(), value);
                }
            },
            _ => {
                self.node_mut(id).attributes.insert(name.to_owned(), value);
            }
        }
    }

    /// `getAttribute(name)`: `None` se o atributo não existe (o `null` do DOM).
    pub fn attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        let node = self.node(id);
        match name {
            "aria-label" => node.label.as_deref(),
            "role" => node.role.aria_name().or_else(|| node.attributes.get(name).map(String::as_str)),
            _ => node.attributes.get(name).map(String::as_str),
        }
    }

    /// `hasAttribute(name)`.
    pub fn has_attribute(&self, id: NodeId, name: &str) -> bool {
        self.attribute(id, name).is_some()
    }

    /// `removeAttribute(name)`.
    pub fn remove_attribute(&mut self, id: NodeId, name: &str) {
        match name {
            "aria-label" => self.clear_label(id),
            "role" => {
                self.set_role(id, Role::Generic);
                self.node_mut(id).attributes.remove(name);
            }
            _ => {
                self.node_mut(id).attributes.remove(name);
            }
        }
    }

    pub fn role(&self, id: NodeId) -> Role {
        self.node(id).role
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.node(id).children
    }

    /// O retângulo do nó em coordenadas da janela.
    pub fn absolute_bounds(&self, id: NodeId) -> Rect {
        let mut rect = self.bounds(id);
        let mut parent = self.node(id).parent;
        while let Some(p) = parent {
            let origin = self.bounds(p);
            rect = rect + Vec2::new(origin.x0, origin.y0);
            parent = self.node(p).parent;
        }
        rect
    }

    /// Filhos na ordem de pintura: `z_index` crescente, empate na ordem do DOM.
    fn paint_order(&self, id: NodeId) -> Vec<NodeId> {
        let mut children = self.node(id).children.clone();
        children.sort_by_key(|c| self.node(*c).style.z_index);
        children
    }

    /// `classList.add`.
    pub fn add_class(&mut self, id: NodeId, class: &str) {
        if !self.has_class(id, class) {
            self.node_mut(id).classes.push(class.to_owned());
        }
    }

    /// `className = value`: troca todas as classes (separadas por espaço).
    pub fn set_class_name(&mut self, id: NodeId, value: &str) {
        self.node_mut(id).classes = value.split_whitespace().map(str::to_owned).collect();
    }

    /// `classList.remove`.
    pub fn remove_class(&mut self, id: NodeId, class: &str) {
        self.node_mut(id).classes.retain(|c| c != class);
    }

    /// `classList.toggle(class, force)`.
    pub fn toggle_class(&mut self, id: NodeId, class: &str, force: bool) {
        if force {
            self.add_class(id, class);
        } else {
            self.remove_class(id, class);
        }
    }

    /// `classList`: as classes do nó, na ordem em que entraram (o `className.split(' ')`).
    pub fn classes(&self, id: NodeId) -> &[String] {
        &self.node(id).classes
    }

    /// `classList.contains`.
    pub fn has_class(&self, id: NodeId, class: &str) -> bool {
        self.node(id).classes.iter().any(|c| c == class)
    }

    /// Os filhos diretos que têm a classe (o `:scope > .classe` do `querySelectorAll`).
    pub fn children_with_class(&self, id: NodeId, class: &str) -> Vec<NodeId> {
        self.node(id).children.iter().copied().filter(|c| self.has_class(*c, class)).collect()
    }

    /// A cor do `color` do CSS em vigor sobre o nó, ou `None` quando vale a cor própria do conteúdo. O nó declara
    /// ([`Foreground::Color`]), herda do pai ([`Foreground::Inherit`]) ou não participa ([`Foreground::Content`], que
    /// só encerra a busca no próprio nó consultado e é transparente nos antepassados).
    pub fn resolved_foreground(&self, id: NodeId) -> Option<Rgba> {
        let mut current = id;
        loop {
            let node = self.node(current);
            match node.style.foreground {
                Foreground::Color(color) => return Some(color),
                Foreground::ColorUnlessUnder { color, class } if !self.has_ancestor_with_class(current, class) => return Some(color),
                Foreground::Content if current == id => return None,
                Foreground::Content | Foreground::Inherit | Foreground::ColorUnlessUnder { .. } => {}
            }
            current = node.parent?;
        }
    }

    /// A cor com que o texto ou o codicon do nó é pintado: a de [`Tree::resolved_foreground`] ou, sem ela, a do conteúdo.
    /// `None` num nó sem [`Content::Text`] nem [`Content::Codicon`].
    pub fn text_color(&self, id: NodeId) -> Option<Rgba> {
        let (Content::Text { color, .. } | Content::Codicon { color, .. }) = &self.node(id).style.content else { return None };
        Some(self.resolved_foreground(id).unwrap_or(*color))
    }

    /// Algum antepassado de `id` (sem contar o próprio nó) tem a classe.
    fn has_ancestor_with_class(&self, id: NodeId, class: &str) -> bool {
        let mut current = self.node(id).parent;
        while let Some(n) = current {
            if self.has_class(n, class) {
                return true;
            }
            current = self.node(n).parent;
        }
        false
    }

    /// Algum nó entre `id` e `ancestor` (os dois fora da conta) declara uma cor ([`Foreground::Color`] ou
    /// [`Foreground::ColorUnlessUnder`]): o texto de `id` herda dela, não da cor que `ancestor` dá aos descendentes.
    pub fn declares_foreground_below(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut current = self.node(id).parent;
        while let Some(n) = current {
            if n == ancestor {
                return false;
            }
            if matches!(self.node(n).style.foreground, Foreground::Color(_) | Foreground::ColorUnlessUnder { .. }) {
                return true;
            }
            current = self.node(n).parent;
        }
        false
    }

    /// O cursor em vigor sobre o nó: o dele ou o do antepassado mais próximo que define um.
    pub fn cursor(&self, id: NodeId) -> Cursor {
        let mut current = Some(id);
        while let Some(n) = current {
            if let Some(cursor) = self.node(n).style.cursor {
                return cursor;
            }
            current = self.node(n).parent;
        }
        Cursor::Default
    }

    pub fn style(&self, id: NodeId) -> &NodeStyle {
        &self.node(id).style
    }

    /// O nó visível mais ao fundo sob o ponto (coordenadas da janela), respeitando recortes.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<NodeId> {
        let root = self.bounds(self.root);
        self.hit(self.root, root, true, x, y)
    }

    /// `rect` é o retângulo resolvido do nó, relativo ao pai; `(x, y)` está nas coordenadas do pai;
    /// `inherited` é o `pointer-events` em vigor no pai.
    fn hit(&self, id: NodeId, rect: Rect, inherited: bool, x: f64, y: f64) -> Option<NodeId> {
        let node = self.node(id);
        if !node.style.visible {
            return None;
        }
        let receives = match node.style.pointer_events {
            PointerEvents::Inherit => inherited,
            PointerEvents::Auto => true,
            PointerEvents::None => false,
        };
        let local = (x - rect.x0, y - rect.y0);
        let inside = rect.contains(je_render::kurbo::Point::new(x, y));
        if node.style.clip && !inside {
            return None;
        }
        // O último filho (e o de maior z-index) fica por cima.
        for &child in self.paint_order(id).iter().rev() {
            let child_rect = self.resolve(child, (rect.width(), rect.height()));
            if let Some(found) = self.hit(child, child_rect, receives, local.0, local.1) {
                return Some(found);
            }
        }
        (inside && id != self.root && receives).then_some(id)
    }

    /// Pinta a árvore na lista de exibição, em pixels de dispositivo (`scale` = devicePixelRatio).
    pub fn paint(&self, list: &mut DisplayList, scale: f64) {
        self.paint_hosted(list, scale, &mut |_, _, _| {});
    }

    /// Como [`Tree::paint`], chamando `host` para cada nó com [`Content::Host`], com o retângulo absoluto do
    /// nó em pixels lógicos, no ponto da ordem de pintura em que o nó está (o que vem depois pinta por cima).
    pub fn paint_hosted(&self, list: &mut DisplayList, scale: f64, host: &mut dyn FnMut(u32, Rect, &mut DisplayList)) {
        let root = self.bounds(self.root);
        self.paint_node(self.root, root, list, Vec2::ZERO, scale, 1.0, host);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_node(&self, id: NodeId, own: Rect, list: &mut DisplayList, origin: Vec2, scale: f64, inherited_opacity: f32, host: &mut dyn FnMut(u32, Rect, &mut DisplayList)) {
        let node = self.node(id);
        let style = &node.style;
        let opacity = inherited_opacity * style.opacity;
        if !style.visible || opacity <= 0.0 {
            return;
        }
        let rect = own + origin;
        let device = Rect::new(rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale);
        let radii = style.corner_radii.map_or([style.radius * scale; 4], |r| r.map(|v| v * scale));
        if let Some(bg) = style.background {
            if radii.iter().any(|r| *r > 0.0) {
                list.fill(rounded_shape(device, radii), fade(bg, opacity));
            } else {
                list.fill_rect(device, fade(bg, opacity));
            }
        }
        match &style.content {
            Content::None => {}
            Content::Host(surface) => host(*surface, rect, list),
            Content::Text { text, style: text_style, color, line_height } => {
                let color = self.resolved_foreground(id).unwrap_or(*color);
                let line = with_text_system(|t| t.line(text, text_style));
                let top = rect.y0 + half_leading(*line_height, f64::from(line.line_height));
                if (scale - 1.0).abs() < f64::EPSILON {
                    line.paint(list, rect.x0 as f32, top as f32, fade(color, opacity));
                } else {
                    let scaled = TextStyle { size_64: (f64::from(text_style.size_64) * scale).round() as u32, ..text_style.clone() };
                    let line = with_text_system(|t| t.line(text, &scaled));
                    let top = rect.y0 * scale + half_leading(*line_height * scale, f64::from(line.line_height));
                    line.paint(list, (rect.x0 * scale) as f32, top as f32, fade(color, opacity));
                }
            }
            Content::Paragraph(paragraph) => {
                let ring = paragraph.focus_color.zip(self.focused_link_of(id)).map(|(color, link)| (link, color, scale));
                if (scale - 1.0).abs() < f64::EPSILON {
                    paint_paragraph(paragraph, rect.x0, rect.y0, rect.width(), list, opacity, ring);
                } else {
                    paint_paragraph(&paragraph.scaled(scale), rect.x0 * scale, rect.y0 * scale, rect.width() * scale, list, opacity, ring);
                }
            }
            Content::Codicon { id: icon, size, color } => {
                let color = self.resolved_foreground(id).unwrap_or(*color);
                let px = (f64::from(*size) * scale) as f32;
                if let Some(line) = with_text_system(|t| t.codicon(icon, px)) {
                    let x = device.x0 + (device.width() - f64::from(line.width)) / 2.0;
                    let y = device.y0 + (device.height() - f64::from(line.line_height)) / 2.0;
                    line.paint(list, x as f32, y as f32, fade(color, opacity));
                }
            }
        }
        if style.clip {
            list.push_clip(rounded_shape(device, radii));
        }
        let child_origin = Vec2::new(rect.x0, rect.y0);
        for child in self.paint_order(id) {
            let child_rect = self.resolve(child, (own.width(), own.height()));
            self.paint_node(child, child_rect, list, child_origin, scale, opacity, host);
        }
        if style.clip {
            list.pop_clip();
        }
        if let Some(border) = style.border {
            if let Some(widths) = style.border_widths {
                // Larguras diferentes por lado: o anel entre a caixa externa e a caixa interna.
                let w = widths.map(|v| v * scale);
                list.fill(border_ring(device, radii, w), fade(border.color, opacity));
            } else {
                // A borda do CSS fica dentro da caixa (box-sizing: border-box): o traço de largura w é
                // centrado a w/2 da borda externa.
                let w = border.width * scale;
                let inset = device.inset(-w / 2.0);
                list.stroke(rounded_shape(inset, radii.map(|r| (r - w / 2.0).max(0.0))), w, fade(border.color, opacity));
            }
        }
    }
}

/// Pinta o parágrafo com o canto superior esquerdo em (`x`, `y`) e a largura dada, em pixels de dispositivo: cada
/// linha ocupa um `line-height` e o texto dela fica centrado nele como o Chromium centra a linha. `focus` é o link
/// com o foco do teclado (número, cor e largura do contorno em pixels de dispositivo): o contorno de 1px sólido
/// fica dentro de cada caixa do link (`outline-offset: -1px`).
fn paint_paragraph(paragraph: &Paragraph, x: f64, y: f64, width: f64, list: &mut DisplayList, opacity: f32, focus: Option<(u32, Rgba, f64)>) {
    let layout = paragraph.layout(width);
    for (index, line) in layout.lines.iter().enumerate() {
        let top = y + index as f64 * paragraph.line_height + half_leading(paragraph.line_height, f64::from(line.line_height));
        let baseline = top + f64::from(line.baseline);
        for piece in &line.pieces {
            let Some(span) = paragraph.spans.get(piece.span) else { continue };
            if let (true, Some(frame)) = (piece.boxed, &span.frame) {
                let frame_rect = Rect::new(x + f64::from(piece.x), baseline - f64::from(piece.ascent) - frame.inset_y, x + f64::from(piece.x + piece.width), baseline + f64::from(piece.descent) + frame.inset_y);
                paint_frame(frame, frame_rect, list, opacity);
            }
            piece.paint(list, x as f32, baseline as f32, fade(paragraph.color_of(span), opacity));
        }
    }
    if let Some((link, color, ring_width)) = focus {
        for link_box in paragraph.link_boxes(width, link) {
            let moved = link_box + Vec2::new(x, y);
            list.stroke(Shape::Rect(moved.inset(-ring_width / 2.0)), ring_width, fade(color, opacity));
        }
    }
}

/// O fundo e a borda da moldura de um trecho (a mesma regra do fundo e da borda de um nó: a borda fica dentro da caixa).
fn paint_frame(frame: &SpanFrame, rect: Rect, list: &mut DisplayList, opacity: f32) {
    let radii = [frame.radius; 4];
    if let Some(background) = frame.background {
        list.fill(rounded_shape(rect, radii), fade(background, opacity));
    }
    if let Some(border) = frame.border {
        let inset = rect.inset(-border.width / 2.0);
        list.stroke(rounded_shape(inset, radii.map(|r| (r - border.width / 2.0).max(0.0))), border.width, fade(border.color, opacity));
    }
}

/// O deslocamento do topo do texto dentro da caixa de linha: metade do espaço que sobra (o `line-height` menos a
/// altura natural da fonte), arredondada para BAIXO. É o `FontHeight::AddLeading` do Blink
/// (`ascent += half_leading.Floor()`), medido contra o oráculo (H26): sem o `floor`, linhas de espaço ímpar
/// (p.ex. `line-height: 10px` com fonte de 13 px) saíam 1 px abaixo do Chromium.
pub(crate) fn half_leading(line_height: f64, natural: f64) -> f64 {
    ((line_height - natural) / 2.0).floor()
}

/// O retângulo com o raio de cada canto (superior esquerdo, superior direito, inferior direito, inferior
/// esquerdo); sem raio nenhum é um retângulo simples.
fn rounded_shape(rect: Rect, radii: [f64; 4]) -> Shape {
    if radii.iter().all(|r| *r <= 0.0) {
        Shape::Rect(rect)
    } else {
        Shape::RoundedRect(RoundedRect::from_rect(rect, RoundedRectRadii::new(radii[0], radii[1], radii[2], radii[3])))
    }
}

/// A borda com largura por lado (cima, direita, baixo, esquerda): a caixa externa menos a interna, com o
/// raio interno de cada canto reduzido pela maior das duas larguras que o tocam.
fn border_ring(outer: Rect, radii: [f64; 4], widths: [f64; 4]) -> Shape {
    let [top, right, bottom, left] = widths;
    let inner = Rect::new(outer.x0 + left, outer.y0 + top, outer.x1 - right, outer.y1 - bottom);
    let mut path = shape_path(&rounded_shape(outer, radii));
    if inner.width() > 0.0 && inner.height() > 0.0 {
        let inner_radii = [radii[0] - left.max(top), radii[1] - right.max(top), radii[2] - right.max(bottom), radii[3] - left.max(bottom)].map(|r| r.max(0.0));
        let hole = shape_path(&rounded_shape(inner, inner_radii)).reverse_subpaths();
        path.extend(hole);
    }
    Shape::Path(path)
}

fn shape_path(shape: &Shape) -> BezPath {
    match shape {
        Shape::Rect(r) => r.to_path(0.1),
        Shape::RoundedRect(r) => r.to_path(0.1),
        Shape::Path(p) => p.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_respeita_ordem_e_recorte() {
        let mut tree = Tree::new();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 100.0, 100.0));
        let a = tree.create(root, Role::Generic);
        tree.set_bounds(a, Rect::new(10.0, 10.0, 60.0, 60.0));
        tree.style_mut(a).clip = true;
        let b = tree.create(a, Role::Button);
        tree.set_bounds(b, Rect::new(40.0, 40.0, 90.0, 90.0));
        let c = tree.create(root, Role::Button);
        tree.set_bounds(c, Rect::new(30.0, 30.0, 40.0, 40.0));
        assert_eq!(tree.hit_test(35.0, 35.0), Some(c), "o último filho fica por cima");
        assert_eq!(tree.hit_test(55.0, 55.0), Some(b), "filho em coordenadas relativas");
        assert_eq!(tree.hit_test(80.0, 80.0), None, "fora do recorte do pai");
        tree.remove(a);
        assert_eq!(tree.hit_test(55.0, 55.0), None);
        assert_eq!(tree.absolute_bounds(c), Rect::new(30.0, 30.0, 40.0, 40.0));
    }
}
