//! Porte do `TreeRenderer` de `src/vs/base/browser/ui/tree/abstractTree.ts`: o renderer que envolve o
//! renderer do usuário com a linha da árvore (`.monaco-tl-row` com o recuo, o twistie e o conteúdo), as
//! guias de indentação e o estado de colapso.
//!
//! Do TypeScript para o Rust:
//! - O DOM do original é a árvore de nós do `je-ui`. As classes (`monaco-tl-row`, `monaco-tl-indent`,
//!   `monaco-tl-twistie`, `monaco-tl-contents`, `indent-guide`, `collapsible`, `collapsed`) são as mesmas.
//! - O CSS de `tree.css` que posiciona as peças vira geometria dos nós no `render_tree_element`: o twistie
//!   ocupa 16 px mais 6 px de `padding-right` a partir de `indentSize`, o conteúdo vem logo depois, e a
//!   largura do `.monaco-tl-indent` é `indentSize + indent - 16`. Divergência: os nós filhos não se
//!   reposicionam sozinhos quando a linha muda de largura (não há `flex`).
//! - `aria-expanded` não existe no `je-ui` (só `role` e `label`) e não é escrito.
//! - Os `Map` por objeto do original (`renderedNodes`, `renderedElements`, `SetMap` das guias) são
//!   mapas por identidade de `Rc` ([`ById`]) e por elemento (o `Map<T, ...>` do JavaScript compara
//!   por `===`, aqui `Eq + Hash`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::{Rc, Weak};

use je_base::event::Event as BaseEvent;
use je_base::lifecycle::{ById, DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_base::map::SetMap;
use je_base::themables::ThemeIcon;
use je_ui::{NodeId, Role, Ui};

use crate::list::list_types::{IListRenderer, ListElementRenderDetails, TemplateData};
use crate::tree::tree_types::{CollapseStateChangeEvent, ITreeModel, ITreeRenderer, TreeElementRenderDetails, TreeNodeApi};

/// `RenderIndentGuides`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderIndentGuides {
    None,
    OnHover,
    Always,
}

/// `ITreeRendererOptions<T>`: os campos de `IAbstractTreeOptionsUpdate` que o renderer lê.
/// `(element: T) => string | undefined`: a classe extra do twistie.
pub type TwistieClassProvider<E> = Rc<dyn Fn(&E) -> Option<String>>;

pub struct TreeRendererOptions<E> {
    pub indent: Option<f64>,
    pub default_indent: Option<f64>,
    pub render_indent_guides: Option<RenderIndentGuides>,
    pub hide_twisties_of_childless_elements: Option<bool>,
    pub twistie_additional_css_class: Option<TwistieClassProvider<E>>,
}

impl<E> Default for TreeRendererOptions<E> {
    fn default() -> Self {
        Self { indent: None, default_indent: None, render_indent_guides: None, hide_twisties_of_childless_elements: None, twistie_additional_css_class: None }
    }
}

impl<E> Clone for TreeRendererOptions<E> {
    fn clone(&self) -> Self {
        Self {
            indent: self.indent,
            default_indent: self.default_indent,
            render_indent_guides: self.render_indent_guides,
            hide_twisties_of_childless_elements: self.hide_twisties_of_childless_elements,
            twistie_additional_css_class: self.twistie_additional_css_class.clone(),
        }
    }
}

/// `Collection<T>`: uma lista de elementos que avisa quando muda.
pub trait Collection<N> {
    fn elements(&self) -> Vec<Rc<N>>;
    fn on_did_change(&self) -> BaseEvent<Vec<Rc<N>>>;
}

/// `EventCollection<T>`: guarda o último valor que o evento entregou.
pub struct EventCollection<N: 'static> {
    disposables: Rc<DisposableStore>,
    on_did_change: BaseEvent<Vec<Rc<N>>>,
    elements: Rc<RefCell<Vec<Rc<N>>>>,
}

impl<N: 'static> EventCollection<N> {
    pub fn new(on_did_change: BaseEvent<Vec<Rc<N>>>, elements: Vec<Rc<N>>) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let elements = Rc::new(RefCell::new(elements));
        let target = elements.clone();
        let on_did_change = on_did_change.for_each(move |e| *target.borrow_mut() = e.clone(), Some(&disposables));
        Rc::new(Self { disposables, on_did_change, elements })
    }
}

impl<N: 'static> Collection<N> for EventCollection<N> {
    fn elements(&self) -> Vec<Rc<N>> {
        self.elements.borrow().clone()
    }

    fn on_did_change(&self) -> BaseEvent<Vec<Rc<N>>> {
        self.on_did_change.clone()
    }
}

impl<N: 'static> IDisposable for EventCollection<N> {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `ITreeListTemplateData<T>`.
pub struct TreeListTemplateData {
    pub container: NodeId,
    pub indent: NodeId,
    pub twistie: NodeId,
    pub indent_guides_disposable: RefCell<DisposableRef>,
    pub indent_size: Cell<f64>,
    pub template_data: TemplateData,
}

/// O `SetMap<ITreeNode, HTMLDivElement>` das guias de indentação renderizadas, por nó.
pub type RenderedIndentGuides<N> = Rc<RefCell<SetMap<ById<N>, NodeId>>>;

const DEFAULT_INDENT: f64 = 8.0;
/// A largura do `.monaco-tl-twistie` (16 px) mais o `padding-right` (6 px) de `tree.css`.
const TWISTIE_WIDTH: f64 = 22.0;

fn expanded_icon_classes() -> Vec<String> {
    ThemeIcon::from_id("tree-item-expanded").as_class_name_array()
}

/// `TreeRenderer<T, TFilterData, TRef, TTemplateData>`.
pub struct TreeRenderer<M: ITreeModel>
where
    M::Element: Eq + Hash,
{
    weak_self: Weak<TreeRenderer<M>>,
    ui: Ui,
    template_id: String,
    renderer: Rc<dyn ITreeRenderer<M>>,
    model: Rc<M>,
    active_nodes: Rc<dyn Collection<M::Node>>,
    rendered_indent_guides: RenderedIndentGuides<M::Node>,
    rendered_elements: RefCell<HashMap<M::Element, Rc<M::Node>>>,
    rendered_nodes: RefCell<HashMap<ById<M::Node>, Rc<TreeListTemplateData>>>,
    indent: Cell<f64>,
    default_indent: Cell<f64>,
    hide_twisties_of_childless_elements: Cell<bool>,
    twistie_additional_css_class: RefCell<Option<TwistieClassProvider<M::Element>>>,
    should_render_indent_guides: Cell<bool>,
    active_indent_nodes: RefCell<HashSet<ById<M::Node>>>,
    indent_guides_disposable: RefCell<DisposableRef>,
    disposables: Rc<DisposableStore>,
}

impl<M: ITreeModel> TreeRenderer<M>
where
    M::Element: Eq + Hash,
{
    pub fn new(
        ui: &Ui,
        renderer: Rc<dyn ITreeRenderer<M>>,
        model: Rc<M>,
        on_did_change_collapse_state: BaseEvent<CollapseStateChangeEvent<M::Node>>,
        active_nodes: Rc<dyn Collection<M::Node>>,
        rendered_indent_guides: RenderedIndentGuides<M::Node>,
        options: &TreeRendererOptions<M::Element>,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            ui: ui.clone(),
            template_id: renderer.template_id(),
            renderer: renderer.clone(),
            model,
            active_nodes,
            rendered_indent_guides,
            rendered_elements: RefCell::new(HashMap::new()),
            rendered_nodes: RefCell::new(HashMap::new()),
            indent: Cell::new(DEFAULT_INDENT),
            default_indent: Cell::new(DEFAULT_INDENT),
            hide_twisties_of_childless_elements: Cell::new(false),
            twistie_additional_css_class: RefCell::new(None),
            should_render_indent_guides: Cell::new(false),
            active_indent_nodes: RefCell::new(HashSet::new()),
            indent_guides_disposable: RefCell::new(none()),
            disposables: disposables.clone(),
        });

        this.update_options(options);

        let weak = this.weak_self.clone();
        disposables.add(on_did_change_collapse_state.listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_node_twistie_state(&e.node);
            }
        }));

        if let Some(event) = renderer.on_did_change_twistie_state() {
            let weak = this.weak_self.clone();
            disposables.add(event.listen(move |element| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_twistie_state(element);
                }
            }));
        }

        this
    }

    pub fn template_id(&self) -> &str {
        &self.template_id
    }

    /// O renderer do usuário que esta instância envolve (a lista de sticky precisa dele).
    pub fn renderer(&self) -> &Rc<dyn ITreeRenderer<M>> {
        &self.renderer
    }

    pub fn update_options(&self, options: &TreeRendererOptions<M::Element>) {
        if let Some(default_indent) = options.default_indent {
            self.default_indent.set(default_indent);
        }

        if options.indent.is_some() || options.default_indent.is_some() {
            let indent = options.indent.map_or(self.indent.get(), |i| i.clamp(0.0, 40.0));
            let needs_rerender = indent != self.indent.get() || options.default_indent.is_some();

            if needs_rerender {
                self.indent.set(indent);

                let rendered: Vec<_> = self.rendered_nodes.borrow().iter().map(|(n, t)| (n.0.clone(), t.clone())).collect();

                for (node, template_data) in rendered {
                    template_data.indent_size.set(self.default_indent.get() + (node.depth() as f64 - 1.0) * self.indent.get());
                    self.render_tree_element(&node, &template_data);
                }
            }
        }

        if let Some(render_indent_guides) = options.render_indent_guides {
            let should_render_indent_guides = render_indent_guides != RenderIndentGuides::None;

            if should_render_indent_guides != self.should_render_indent_guides.get() {
                self.should_render_indent_guides.set(should_render_indent_guides);

                let rendered: Vec<_> = self.rendered_nodes.borrow().iter().map(|(n, t)| (n.0.clone(), t.clone())).collect();

                for (node, template_data) in rendered {
                    self.render_indent_guides(&node, &template_data);
                }

                self.indent_guides_disposable.borrow().dispose();

                if should_render_indent_guides {
                    let disposables = DisposableStore::new();
                    let weak = self.weak_self.clone();
                    disposables.add(self.active_nodes.on_did_change().listen(move |nodes| {
                        if let Some(this) = weak.upgrade() {
                            this.on_did_change_active_nodes(nodes);
                        }
                    }));
                    *self.indent_guides_disposable.borrow_mut() = disposables;
                    self.on_did_change_active_nodes(&self.active_nodes.elements());
                }
            }
        }

        if let Some(hide) = options.hide_twisties_of_childless_elements {
            self.hide_twisties_of_childless_elements.set(hide);
        }

        if let Some(f) = &options.twistie_additional_css_class {
            *self.twistie_additional_css_class.borrow_mut() = Some(f.clone());
        }
    }

    /// `renderElement` para um nó que a lista também pode estar renderizando (a linha do sticky scroll).
    /// O original renderiza um `Proxy` do nó, que tem identidade própria nos `Map` do renderer; aqui o
    /// nó é o mesmo `Rc`, então os mapas `rendered_nodes` e `rendered_elements` voltam ao que eram antes
    /// da chamada, e a linha da lista continua sendo a dona do nó (o twistie dela segue o colapso).
    pub fn render_element_detached(&self, node: &Rc<M::Node>, index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let key = ById(node.clone());
        let element = node.element();
        let previous_node = self.rendered_nodes.borrow().get(&key).cloned();
        let previous_element = self.rendered_elements.borrow().get(&element).cloned();

        IListRenderer::render_element(self, node, index, template_data, details);

        match previous_node {
            Some(template) => self.rendered_nodes.borrow_mut().insert(key, template),
            None => self.rendered_nodes.borrow_mut().remove(&key),
        };
        match previous_element {
            Some(rendered) => self.rendered_elements.borrow_mut().insert(element, rendered),
            None => self.rendered_elements.borrow_mut().remove(&element),
        };
    }

    fn on_did_change_twistie_state(&self, element: &M::Element) {
        let Some(node) = self.rendered_elements.borrow().get(element).cloned() else { return };
        self.on_did_change_node_twistie_state(&node);
    }

    fn on_did_change_node_twistie_state(&self, node: &Rc<M::Node>) {
        let Some(template_data) = self.rendered_nodes.borrow().get(&ById(node.clone())).cloned() else { return };
        self.on_did_change_active_nodes(&self.active_nodes.elements());
        self.render_tree_element(node, &template_data);
    }

    fn render_tree_element(&self, node: &Rc<M::Node>, template_data: &Rc<TreeListTemplateData>) {
        let indent_size = template_data.indent_size.get();
        let element = node.element();

        {
            let mut tree = self.ui.tree_mut();
            // `templateData.twistie.className = templateData.twistie.classList.item(0)`
            tree.set_class_name(template_data.twistie, "monaco-tl-twistie");

            let height = tree.bounds(template_data.container).height();
            tree.set_bounds(template_data.twistie, je_render::kurbo::Rect::new(indent_size, 0.0, indent_size + TWISTIE_WIDTH, height));
            let indent_width = (indent_size + self.indent.get() - 16.0).max(0.0);
            tree.set_bounds(template_data.indent, je_render::kurbo::Rect::new(16.0, 0.0, 16.0 + indent_width, height));
        }

        let twistie_rendered = self.renderer.render_twistie(&element, template_data.twistie);

        {
            let mut tree = self.ui.tree_mut();
            let hide = self.hide_twisties_of_childless_elements.get();

            if node.collapsible() && (!hide || node.visible_children_count() > 0) {
                if !twistie_rendered {
                    for class in expanded_icon_classes() {
                        tree.add_class(template_data.twistie, &class);
                    }
                }

                tree.add_class(template_data.twistie, "collapsible");
                tree.toggle_class(template_data.twistie, "collapsed", node.collapsed());
            } else {
                tree.remove_class(template_data.twistie, "collapsible");
                tree.remove_class(template_data.twistie, "collapsed");
            }

            if let Some(additional) = &*self.twistie_additional_css_class.borrow()
                && let Some(class) = additional(&element)
            {
                tree.add_class(template_data.twistie, &class);
            }
        }

        self.render_indent_guides(node, template_data);
    }

    fn render_indent_guides(&self, node: &Rc<M::Node>, template_data: &Rc<TreeListTemplateData>) {
        {
            let mut tree = self.ui.tree_mut();
            let children = tree.children(template_data.indent).to_vec();
            for child in children {
                tree.remove(child);
            }
        }

        template_data.indent_guides_disposable.borrow().dispose();

        if !self.should_render_indent_guides.get() {
            return;
        }

        let disposable_store = DisposableStore::new();
        let mut node = node.clone();

        loop {
            let reference = self.model.get_node_location(&node);
            let Some(parent_ref) = self.model.get_parent_node_location(&reference) else { break };

            if self.model.ref_is_falsy(&parent_ref) {
                break;
            }

            let parent = self.model.get_node(&parent_ref);

            let guide = {
                let mut tree = self.ui.tree_mut();
                let guide = tree.create_detached(Role::Generic);
                tree.add_class(guide, "indent-guide");
                let height = tree.bounds(template_data.container).height();
                tree.set_bounds(guide, je_render::kurbo::Rect::new(0.0, 0.0, self.indent.get(), height));

                if self.active_indent_nodes.borrow().contains(&ById(parent.clone())) {
                    tree.add_class(guide, "active");
                }

                let first = tree.children(template_data.indent).first().copied();
                if first.is_some() {
                    tree.insert_before(template_data.indent, guide, first);
                } else {
                    tree.append_child(template_data.indent, guide);
                }
                guide
            };

            self.rendered_indent_guides.borrow_mut().add(ById(parent.clone()), guide);

            let rendered = self.rendered_indent_guides.clone();
            let key = ById(parent.clone());
            disposable_store.add(to_disposable(move || rendered.borrow_mut().delete(&key, &guide)));

            node = parent;
        }

        *template_data.indent_guides_disposable.borrow_mut() = disposable_store;
    }

    fn on_did_change_active_nodes(&self, nodes: &[Rc<M::Node>]) {
        if !self.should_render_indent_guides.get() {
            return;
        }

        let mut set: HashSet<ById<M::Node>> = HashSet::new();

        for node in nodes {
            let reference = self.model.get_node_location(node);

            // O `try/catch` do original: o nó pode já ter saído do modelo.
            let parent = je_base::errors::catch(|| self.model.get_parent_node_location(&reference).filter(|r| !self.model.ref_is_falsy(r)).map(|r| self.model.get_node(&r)));
            let Ok(parent) = parent else { continue };

            if node.collapsible() && !node.children().is_empty() && !node.collapsed() {
                set.insert(ById(node.clone()));
            } else if let Some(parent) = parent {
                set.insert(ById(parent));
            }
        }

        let previous = self.active_indent_nodes.borrow().clone();
        let mut tree_guides: Vec<(NodeId, bool)> = Vec::new();

        for node in &previous {
            if !set.contains(node) {
                self.rendered_indent_guides.borrow().for_each(node, |line| tree_guides.push((*line, false)));
            }
        }

        for node in &set {
            if !previous.contains(node) {
                self.rendered_indent_guides.borrow().for_each(node, |line| tree_guides.push((*line, true)));
            }
        }

        {
            let mut tree = self.ui.tree_mut();
            for (line, active) in tree_guides {
                tree.toggle_class(line, "active", active);
            }
        }

        *self.active_indent_nodes.borrow_mut() = set;
    }
}

impl<M: ITreeModel> IListRenderer<Rc<M::Node>> for TreeRenderer<M>
where
    M::Element: Eq + Hash,
{
    fn template_id(&self) -> String {
        self.template_id.clone()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        if let Some(class) = self.renderer.row_class_name() {
            self.ui.tree_mut().add_class(container, &class);
        }

        let (indent, twistie, contents) = {
            let mut tree = self.ui.tree_mut();
            let el = tree.create(container, Role::Generic);
            tree.add_class(el, "monaco-tl-row");
            let indent = tree.create(el, Role::Generic);
            tree.add_class(indent, "monaco-tl-indent");
            let twistie = tree.create(el, Role::Generic);
            tree.add_class(twistie, "monaco-tl-twistie");
            let contents = tree.create(el, Role::Generic);
            tree.add_class(contents, "monaco-tl-contents");
            (indent, twistie, contents)
        };

        let template_data = self.renderer.render_template(contents);

        Rc::new(TreeListTemplateData { container, indent, twistie, indent_guides_disposable: RefCell::new(none()), indent_size: Cell::new(0.0), template_data })
    }

    fn render_element(&self, node: &Rc<M::Node>, index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let template_data = tree_template_data(template_data);

        template_data.indent_size.set(self.default_indent.get() + (node.depth() as f64 - 1.0) * self.indent.get());
        self.rendered_nodes.borrow_mut().insert(ById(node.clone()), template_data.clone());
        self.rendered_elements.borrow_mut().insert(node.element(), node.clone());

        {
            // As partes da linha acompanham a altura dada pela lista.
            let mut tree = self.ui.tree_mut();
            let container = tree.bounds(template_data.container);
            let height = details.and_then(|d| d.height).unwrap_or_else(|| container.height());
            let indent_size = template_data.indent_size.get();
            tree.set_bounds(template_data.indent, je_render::kurbo::Rect::new(16.0, 0.0, 16.0 + (indent_size + self.indent.get() - 16.0).max(0.0), height));
            let contents_left = indent_size + TWISTIE_WIDTH;
            let contents = tree.children(tree.parent(template_data.twistie).unwrap_or(template_data.container)).last().copied();
            if let Some(contents) = contents {
                tree.set_bounds(contents, je_render::kurbo::Rect::new(contents_left, 0.0, contents_left.max(container.width()), height));
            }
        }

        self.render_tree_element(node, &template_data);
        self.renderer.render_element(node, index, &template_data.template_data, Some(&TreeElementRenderDetails { height: details.and_then(|d| d.height), on_scroll: details.and_then(|d| d.on_scroll), indent: template_data.indent_size.get() }));
    }

    fn dispose_element(&self, node: &Rc<M::Node>, index: i64, template_data: &TemplateData, details: Option<&ListElementRenderDetails>) {
        let template_data = tree_template_data(template_data);

        template_data.indent_guides_disposable.borrow().dispose();

        self.renderer.dispose_element(node, index, &template_data.template_data, Some(&TreeElementRenderDetails { height: details.and_then(|d| d.height), on_scroll: details.and_then(|d| d.on_scroll), indent: template_data.indent_size.get() }));

        if details.is_some_and(|d| d.height.is_some()) {
            let is_current = self.rendered_nodes.borrow().get(&ById(node.clone())).is_some_and(|t| Rc::ptr_eq(t, &template_data));

            if is_current {
                self.rendered_nodes.borrow_mut().remove(&ById(node.clone()));

                let element = node.element();
                let owns_element = self.rendered_elements.borrow().get(&element).is_some_and(|n| Rc::ptr_eq(n, node));

                if owns_element {
                    self.rendered_elements.borrow_mut().remove(&element);
                }
            }
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        self.renderer.dispose_template(&tree_template_data(template_data).template_data);
    }
}

/// O `ITreeListTemplateData` que o `render_template` da árvore devolveu.
pub fn tree_template_data(template_data: &TemplateData) -> Rc<TreeListTemplateData> {
    Rc::downcast::<TreeListTemplateData>(template_data.clone()).unwrap_or_else(|_| panic!("o template não é da árvore"))
}

impl<M: ITreeModel> IDisposable for TreeRenderer<M>
where
    M::Element: Eq + Hash,
{
    fn dispose(&self) {
        self.rendered_nodes.borrow_mut().clear();
        self.rendered_elements.borrow_mut().clear();
        self.indent_guides_disposable.borrow().dispose();
        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TreeRenderer"
    }
}
