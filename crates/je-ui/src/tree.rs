//! A árvore de nós, numa arena.

use je_render::kurbo::{Rect, RoundedRect, Vec2};
use je_render::{DisplayList, Rgba, Shape};
use je_text::{TextStyle, with_text_system};

/// Aplica a opacidade ao alfa da cor.
fn fade(color: Rgba, opacity: f32) -> Rgba {
    if opacity >= 1.0 {
        return color;
    }
    Rgba { a: (f32::from(color.a) * opacity).round() as u8, ..color }
}

/// O índice de um nó na arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

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
    /// Um codicon centrado no nó, no tamanho dado.
    Codicon { id: String, size: f32, color: Rgba },
}

/// O estilo resolvido de um nó.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeStyle {
    pub background: Option<Rgba>,
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
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self { background: None, border: None, radius: 0.0, clip: false, visible: true, pointer_events: PointerEvents::Inherit, z_index: 0, cursor: None, opacity: 1.0, content: Content::None }
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
    alive: bool,
}

/// A árvore de UI de uma janela.
pub struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    root: NodeId,
}

impl Node {
    fn new(parent: Option<NodeId>, role: Role) -> Self {
        Self { parent, children: Vec::new(), bounds: Rect::ZERO, fill: (false, false), right: None, bottom: None, style: NodeStyle::default(), role, label: None, classes: Vec::new(), alive: true }
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
        Self { nodes: vec![root], free: Vec::new(), root: NodeId(0) }
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
                self.nodes[slot as usize] = node;
                NodeId(slot)
            }
            None => {
                self.nodes.push(node);
                NodeId(u32::try_from(self.nodes.len() - 1).expect("mais de 4 bilhões de nós"))
            }
        }
    }

    /// Tira o nó do pai sem destruí-lo (o `element.remove()`): a subárvore continua viva e pode voltar.
    pub fn detach(&mut self, id: NodeId) {
        if !self.node(id).alive {
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
        self.nodes.get(id.0 as usize).is_some_and(|n| n.alive)
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

    /// Remove o nó e a subárvore dele (o `remove` do DOM).
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root || !self.node(id).alive {
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

    /// `classList.contains`.
    pub fn has_class(&self, id: NodeId, class: &str) -> bool {
        self.node(id).classes.iter().any(|c| c == class)
    }

    /// Os filhos diretos que têm a classe (o `:scope > .classe` do `querySelectorAll`).
    pub fn children_with_class(&self, id: NodeId, class: &str) -> Vec<NodeId> {
        self.node(id).children.iter().copied().filter(|c| self.has_class(*c, class)).collect()
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
        let root = self.bounds(self.root);
        self.paint_node(self.root, root, list, Vec2::ZERO, scale, 1.0);
    }

    fn paint_node(&self, id: NodeId, own: Rect, list: &mut DisplayList, origin: Vec2, scale: f64, inherited_opacity: f32) {
        let node = self.node(id);
        let style = &node.style;
        let opacity = inherited_opacity * style.opacity;
        if !style.visible || opacity <= 0.0 {
            return;
        }
        let rect = own + origin;
        let device = Rect::new(rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale);
        let radius = style.radius * scale;
        if let Some(bg) = style.background {
            if radius > 0.0 {
                list.fill(Shape::RoundedRect(RoundedRect::from_rect(device, radius)), fade(bg, opacity));
            } else {
                list.fill_rect(device, fade(bg, opacity));
            }
        }
        match &style.content {
            Content::None => {}
            Content::Text { text, style: text_style, color, line_height } => {
                let line = with_text_system(|t| t.line(text, text_style));
                let top = rect.y0 + (line_height - f64::from(line.line_height)) / 2.0;
                if (scale - 1.0).abs() < f64::EPSILON {
                    line.paint(list, rect.x0 as f32, top as f32, fade(*color, opacity));
                } else {
                    let scaled = TextStyle { size_64: (f64::from(text_style.size_64) * scale).round() as u32, ..text_style.clone() };
                    let line = with_text_system(|t| t.line(text, &scaled));
                    let top = rect.y0 * scale + (line_height * scale - f64::from(line.line_height)) / 2.0;
                    line.paint(list, (rect.x0 * scale) as f32, top as f32, fade(*color, opacity));
                }
            }
            Content::Codicon { id: icon, size, color } => {
                let px = (f64::from(*size) * scale) as f32;
                if let Some(line) = with_text_system(|t| t.codicon(icon, px)) {
                    let x = device.x0 + (device.width() - f64::from(line.width)) / 2.0;
                    let y = device.y0 + (device.height() - f64::from(line.line_height)) / 2.0;
                    line.paint(list, x as f32, y as f32, fade(*color, opacity));
                }
            }
        }
        if style.clip {
            list.push_clip(if radius > 0.0 { Shape::RoundedRect(RoundedRect::from_rect(device, radius)) } else { Shape::Rect(device) });
        }
        let child_origin = Vec2::new(rect.x0, rect.y0);
        for child in self.paint_order(id) {
            let child_rect = self.resolve(child, (own.width(), own.height()));
            self.paint_node(child, child_rect, list, child_origin, scale, opacity);
        }
        if style.clip {
            list.pop_clip();
        }
        if let Some(border) = style.border {
            // A borda do CSS fica dentro da caixa (box-sizing: border-box): o traço de largura w é
            // centrado a w/2 da borda externa.
            let w = border.width * scale;
            let inset = device.inset(-w / 2.0);
            let shape = if radius > 0.0 { Shape::RoundedRect(RoundedRect::from_rect(inset, (radius - w / 2.0).max(0.0))) } else { Shape::Rect(inset) };
            list.stroke(shape, w, fade(border.color, opacity));
        }
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
