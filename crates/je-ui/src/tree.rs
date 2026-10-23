//! A árvore de nós, numa arena.

use je_render::kurbo::{Rect, RoundedRect, Vec2};
use je_render::{DisplayList, Rgba, Shape};
use je_text::{TextStyle, with_text_system};

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
    pub visible: bool,
    pub content: Content,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self { background: None, border: None, radius: 0.0, clip: false, visible: true, content: Content::None }
    }
}

struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    /// Retângulo relativo ao pai, em pixels lógicos.
    bounds: Rect,
    style: NodeStyle,
    role: Role,
    label: Option<String>,
    alive: bool,
}

/// A árvore de UI de uma janela.
pub struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    root: NodeId,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        let root = Node { parent: None, children: Vec::new(), bounds: Rect::ZERO, style: NodeStyle::default(), role: Role::Region, label: None, alive: true };
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
        let node = Node { parent: Some(parent), children: Vec::new(), bounds: Rect::ZERO, style: NodeStyle::default(), role, label: None, alive: true };
        let id = match self.free.pop() {
            Some(slot) => {
                self.nodes[slot as usize] = node;
                NodeId(slot)
            }
            None => {
                self.nodes.push(node);
                NodeId(u32::try_from(self.nodes.len() - 1).expect("mais de 4 bilhões de nós"))
            }
        };
        self.node_mut(parent).children.push(id);
        id
    }

    /// Remove o nó e a subárvore dele (o `remove` do DOM).
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root || !self.node(id).alive {
            return;
        }
        if let Some(parent) = self.node(id).parent {
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

    pub fn set_bounds(&mut self, id: NodeId, bounds: Rect) {
        self.node_mut(id).bounds = bounds;
    }

    pub fn bounds(&self, id: NodeId) -> Rect {
        self.node(id).bounds
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

    pub fn role(&self, id: NodeId) -> Role {
        self.node(id).role
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.node(id).children
    }

    /// O retângulo do nó em coordenadas da janela.
    pub fn absolute_bounds(&self, id: NodeId) -> Rect {
        let mut rect = self.node(id).bounds;
        let mut parent = self.node(id).parent;
        while let Some(p) = parent {
            rect = rect + Vec2::new(self.node(p).bounds.x0, self.node(p).bounds.y0);
            parent = self.node(p).parent;
        }
        rect
    }

    /// O nó visível mais ao fundo sob o ponto (coordenadas da janela), respeitando recortes.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<NodeId> {
        self.hit(self.root, x, y)
    }

    fn hit(&self, id: NodeId, x: f64, y: f64) -> Option<NodeId> {
        let node = self.node(id);
        if !node.style.visible {
            return None;
        }
        let local = (x - node.bounds.x0, y - node.bounds.y0);
        let inside = node.bounds.contains(je_render::kurbo::Point::new(x, y));
        if node.style.clip && !inside {
            return None;
        }
        // O último filho fica por cima (ordem do DOM sem z-index).
        for &child in node.children.iter().rev() {
            if let Some(found) = self.hit(child, local.0, local.1) {
                return Some(found);
            }
        }
        (inside && id != self.root).then_some(id)
    }

    /// Pinta a árvore na lista de exibição, em pixels de dispositivo (`scale` = devicePixelRatio).
    pub fn paint(&self, list: &mut DisplayList, scale: f64) {
        self.paint_node(self.root, list, Vec2::ZERO, scale);
    }

    fn paint_node(&self, id: NodeId, list: &mut DisplayList, origin: Vec2, scale: f64) {
        let node = self.node(id);
        let style = &node.style;
        if !style.visible {
            return;
        }
        let rect = node.bounds + origin;
        let device = Rect::new(rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale);
        let radius = style.radius * scale;
        if let Some(bg) = style.background {
            if radius > 0.0 {
                list.fill(Shape::RoundedRect(RoundedRect::from_rect(device, radius)), bg);
            } else {
                list.fill_rect(device, bg);
            }
        }
        match &style.content {
            Content::None => {}
            Content::Text { text, style: text_style, color, line_height } => {
                let line = with_text_system(|t| t.line(text, text_style));
                let top = rect.y0 + (line_height - f64::from(line.line_height)) / 2.0;
                if (scale - 1.0).abs() < f64::EPSILON {
                    line.paint(list, rect.x0 as f32, top as f32, *color);
                } else {
                    let scaled = TextStyle { size_64: (f64::from(text_style.size_64) * scale).round() as u32, ..text_style.clone() };
                    let line = with_text_system(|t| t.line(text, &scaled));
                    let top = rect.y0 * scale + (line_height * scale - f64::from(line.line_height)) / 2.0;
                    line.paint(list, (rect.x0 * scale) as f32, top as f32, *color);
                }
            }
            Content::Codicon { id: icon, size, color } => {
                let px = (f64::from(*size) * scale) as f32;
                if let Some(line) = with_text_system(|t| t.codicon(icon, px)) {
                    let x = device.x0 + (device.width() - f64::from(line.width)) / 2.0;
                    let y = device.y0 + (device.height() - f64::from(line.line_height)) / 2.0;
                    line.paint(list, x as f32, y as f32, *color);
                }
            }
        }
        if style.clip {
            list.push_clip(if radius > 0.0 { Shape::RoundedRect(RoundedRect::from_rect(device, radius)) } else { Shape::Rect(device) });
        }
        let child_origin = Vec2::new(rect.x0, rect.y0);
        for &child in &node.children {
            self.paint_node(child, list, child_origin, scale);
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
            list.stroke(shape, w, border.color);
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
