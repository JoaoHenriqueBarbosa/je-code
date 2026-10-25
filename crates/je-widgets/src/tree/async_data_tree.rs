//! Porte de `src/vs/base/browser/ui/tree/asyncDataTree.ts` (a `AsyncDataTree` e o navegador). As peças
//! (nó interno, envoltório, renderer, opções) estão em [`crate::tree::async_data_tree_node`] e a
//! `CompressibleAsyncDataTree` em [`crate::tree::compressible_async_data_tree`].
//!
//! Do TypeScript para o Rust:
//! - As funções `async` do original são funções que devolvem [`Promise`] e rodam o corpo com
//!   [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`, e cada `await` custa o que
//!   custa no motor JavaScript (uma rodada de microtarefas), de modo que a ordem das continuações é a do
//!   original (o teste `asyncDataTree.test.ts` depende disso). Um `return promessa` de dentro de uma função
//!   `async` é [`async_fn_maybe`] (a adoção custa duas rodadas). Onde o original tem uma camada `async`
//!   a mais (`updateChildren` sobre `_updateChildren`), a camada existe aqui também.
//! - `AsyncDataTree` não herda da `ObjectTree`: tem uma por baixo (`this.tree`). Aqui ela é a
//!   [`AbstractTree`] do modelo do backend (`B::Model`), e a `AsyncDataTree` faz `Deref` para ela: os
//!   métodos que o original repassa sem tradução de elemento (`layout`, `domFocus`, `scrollTop`,
//!   `onDidChangeCollapseState`, `focusNext`, `updateOptions`...) valem direto por esse `Deref`, com os
//!   elementos do nível de baixo (`Option<AsyncNode>`); os que o original traduz (`collapse`, `getNode`,
//!   `getSelection`, ...) são métodos próprios, que têm precedência.
//! - `CompressibleAsyncDataTree extends AsyncDataTree` troca a árvore de baixo e sobrescreve cinco métodos
//!   (`createTree`, `shouldAutoExpandSingleChild`, `asTreeElement`, `getViewState`, `render`,
//!   `processChildren`). A herança é o parâmetro `B` ([`AsyncDataTreeBackend`]): o backend comum
//!   ([`ObjectBackend`]) usa o `ObjectTreeModel` e o comprimível o `CompressibleObjectTreeModel`.
//! - O `TInput | T` é o [`InputOrElement`]. Os argumentos opcionais com valor padrão (`element =
//!   this.root.element`) são `Option` (`None` é o padrão). `updateChildren(element, recursive, rerender,
//!   options)` não tem argumentos nomeados: passe todos.
//! - O `Iterable<T>` é um `Vec<T>`; o `Iterable<T> | Promise<Iterable<T>>` do `getChildren` é um
//!   `MaybePromise<Vec<T>>` (o valor direto é o `isIterable(children)`). Os filhos são avaliados de uma
//!   vez (o original avalia sob demanda).
//! - Os `Map` por objeto (`nodes`, `subTreeRefreshPromises`, `refreshPromises`) são mapas por identidade
//!   do nó ([`ByRef`]); os com ordem de inserção (`forEach` do `Map`) são [`OrderedMap`].
//! - `get_data_node` lança (pânico com o `Error`) quando falta o nó, como o `throw` do original; dentro de
//!   uma função `async` isso rejeita a promessa.
//! - Fora de escopo, `// TODO(integração)`: a busca assíncrona (`AsyncFindProvider`, `AsyncFindController`,
//!   `AsyncFindFilter`, `findMode`, `findMatchType`, `openFind`, `closeFind` e o `focusNavigationFilter`
//!   do foco, que dependem do controlador de busca de [`crate::tree::find`]), a arrastar e soltar
//!   (`AsyncDataTreeNodeListDragAndDrop`) e os eventos `onTap`, `onKeyDown`, `onContextMenu` (a árvore
//!   abstrata do `je-widgets` ainda não os tem).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use je_base::async_::{async_fn, async_fn_maybe, create_cancelable_promise, promises, timeout};
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::{CancelablePromise, MaybePromise, Promise};
use je_ui::{NodeId, Ui};

use crate::common::throw;
use crate::list::list_types::{BrowserEvent, IIdentityProvider, IListVirtualDelegate};
use crate::tree::abstract_tree::{AbstractTree, ExpandOnlyOnTwistieClick, TreeNavigator};
use crate::tree::async_data_tree_node::{
    AsyncDataTreeNodeWrapper, AsyncDataTreeOptions, AsyncDataTreeRenderer, AsyncDataTreeUpdateChildrenOptions, AsyncNode, AsyncNodeMapper, IAsyncDataTreeRenderer, IAsyncDataTreeViewState,
    NodeIdentityProvider, ViewStateContext, as_object_tree_options, create_async_data_tree_node, dfs, intersects, AsyncNodeDelegate,
};
use crate::tree::object_tree::{ObjectTree, ObjectTreeMethods, ObjectTreeOptions};
use crate::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use crate::tree::tree_types::{
    CollapseStateChangeEvent, CollapsedValue, IAsyncDataSource, IObjectTreeElement, ITreeModel, ITreeNavigator, ITreeRenderer, ITreeSorter, InputOrElement, ObjectTreeElementCollapseState, TreeEvent,
    TreeMouseEvent, TreeNodeApi, tree_error,
};

/// O que o backend precisa para montar a árvore de baixo (o que o `createTree` do original lê da árvore
/// de dados).
pub struct TreeParts<TInput: Clone + 'static, T: Clone + 'static, F: Clone + 'static> {
    pub ui: Ui,
    pub node_mapper: Rc<AsyncNodeMapper<TInput, T, F>>,
    /// `_onDidChangeNodeSlowState.event`.
    pub on_did_change_node_slow_state: BaseEvent<AsyncNode<TInput, T>>,
}

/// O que muda entre a [`AsyncDataTree`] e a `CompressibleAsyncDataTree` (ver o `//!`).
pub trait AsyncDataTreeBackend<TInput, T, F>: Sized + 'static
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
{
    /// O modelo da árvore de baixo.
    type Model: ITreeModel<Element = Option<AsyncNode<TInput, T>>, Ref = Option<AsyncNode<TInput, T>>, FilterData = F>;
    /// O renderer que o usuário implementa para este backend.
    type Renderer: ?Sized;

    /// `createTree`.
    fn create_tree(
        &self,
        parts: TreeParts<TInput, T, F>,
        user: &str,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<Self::Renderer>>,
        options: &AsyncDataTreeOptions<T, F>,
    ) -> Rc<AbstractTree<Self::Model>>;

    /// `this.tree.setChildren(...)`, com o elemento já na forma da árvore de baixo.
    fn set_children(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, children: Vec<IObjectTreeElement<AsyncNode<TInput, T>>>, options: ObjectTreeModelSetChildrenOptions<AsyncNode<TInput, T>, F>);

    /// `this.tree.resort(...)`.
    fn resort(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, recursive: bool);

    /// `shouldAutoExpandSingleChild`: `base` é o que a `AsyncDataTree` responderia.
    fn should_auto_expand_single_child(&self, tree: &AbstractTree<Self::Model>, base: bool, node: &AsyncNode<TInput, T>) -> bool;

    /// A parte de `processChildren` que a variante comprimível acrescenta (o filtro), antes da ordenação.
    fn filter_children(&self, children: Vec<T>) -> Vec<T>;

    /// Os ids dos nós expandidos para o `getViewState` (a varredura da árvore de baixo).
    fn expanded_ids(&self, tree: &AbstractTree<Self::Model>, get_id: &dyn Fn(&T) -> String) -> Vec<String>;

    /// `render`.
    fn render(&self, tree: &AsyncDataTree<TInput, T, F, Self>, node: &AsyncNode<TInput, T>, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>, options: Option<AsyncDataTreeUpdateChildrenOptions<T>>);
}

/// O backend da `AsyncDataTree` comum: uma `ObjectTree` por baixo.
pub struct ObjectBackend;

impl<TInput, T, F> AsyncDataTreeBackend<TInput, T, F> for ObjectBackend
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
{
    type Model = ObjectTreeModel<AsyncNode<TInput, T>, F>;
    type Renderer = dyn IAsyncDataTreeRenderer<TInput, T, F>;

    fn create_tree(
        &self,
        parts: TreeParts<TInput, T, F>,
        user: &str,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<Self::Renderer>>,
        options: &AsyncDataTreeOptions<T, F>,
    ) -> Rc<AbstractTree<Self::Model>> {
        let object_tree_renderers: Vec<Rc<dyn ITreeRenderer<Self::Model>>> = renderers
            .into_iter()
            .map(|renderer| Rc::new(AsyncDataTreeRenderer::new(&parts.ui, renderer, parts.node_mapper.clone(), parts.on_did_change_node_slow_state.clone(), ())) as Rc<dyn ITreeRenderer<Self::Model>>)
            .collect();
        let object_tree_options = ObjectTreeOptions { tree: as_object_tree_options::<TInput, T, F, Self::Model>(options), sorter: None };

        ObjectTree::new(user, &parts.ui, container, Rc::new(AsyncNodeDelegate(delegate)), object_tree_renderers, object_tree_options).abstract_tree().clone()
    }

    fn set_children(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, children: Vec<IObjectTreeElement<AsyncNode<TInput, T>>>, options: ObjectTreeModelSetChildrenOptions<AsyncNode<TInput, T>, F>) {
        tree.model.set_children(element, children, options);
    }

    fn resort(&self, tree: &AbstractTree<Self::Model>, element: &Option<AsyncNode<TInput, T>>, recursive: bool) {
        tree.model.resort(element, recursive);
    }

    fn should_auto_expand_single_child(&self, _tree: &AbstractTree<Self::Model>, base: bool, _node: &AsyncNode<TInput, T>) -> bool {
        base
    }

    fn filter_children(&self, children: Vec<T>) -> Vec<T> {
        children
    }

    fn expanded_ids(&self, tree: &AbstractTree<Self::Model>, get_id: &dyn Fn(&T) -> String) -> Vec<String> {
        let mut expanded = Vec::new();
        let root = tree.get_root_node();
        let mut stack = vec![root.clone()];

        while let Some(node) = stack.pop() {
            if !Rc::ptr_eq(&node, &root)
                && node.collapsible()
                && !node.collapsed()
                && let Some(element) = &node.element
            {
                expanded.push(get_id(&element.element_t()));
            }

            stack.extend(node.children());
        }

        expanded
    }

    fn render(&self, tree: &AsyncDataTree<TInput, T, F, Self>, node: &AsyncNode<TInput, T>, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>, options: Option<AsyncDataTreeUpdateChildrenOptions<T>>) {
        tree.render_base(node, view_state_context, options);
    }
}

/// O `nodes` do original: do elemento (a raiz é `None`) para o nó interno.
type Nodes<TInput, T> = Rc<RefCell<HashMap<Option<T>, AsyncNode<TInput, T>>>>;

/// O `getDefaultCollapseState` do original.
type DefaultCollapseState<T> = Box<dyn Fn(&T) -> Option<ObjectTreeElementCollapseState>>;

/// O `refreshPromises` do original.
type RefreshPromises<TInput, T> = RefCell<OrderedMap<AsyncNode<TInput, T>, CancelablePromise<Vec<T>>>>;

/// `AsyncDataTree<TInput, T, TFilterData>`.
pub struct AsyncDataTree<TInput, T, F, B>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
    B: AsyncDataTreeBackend<TInput, T, F>,
{
    pub(crate) user: String,
    pub(crate) tree: Rc<AbstractTree<B::Model>>,
    pub(crate) root: AsyncNode<TInput, T>,
    nodes: Nodes<TInput, T>,
    sorter: Option<Rc<dyn ITreeSorter<T>>>,
    get_default_collapse_state: DefaultCollapseState<T>,
    sub_tree_refresh_promises: RefCell<OrderedMap<AsyncNode<TInput, T>, CancelablePromise<()>>>,
    refresh_promises: RefreshPromises<TInput, T>,
    pub(crate) identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    auto_expand_single_children: bool,
    data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
    on_did_render: Rc<Emitter<()>>,
    on_did_change_node_slow_state: Rc<Emitter<AsyncNode<TInput, T>>>,
    node_mapper: Rc<AsyncNodeMapper<TInput, T, F>>,
    disposables: Rc<DisposableStore>,
    pub(crate) backend: B,
}

impl<TInput, T, F, B> Deref for AsyncDataTree<TInput, T, F, B>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
    B: AsyncDataTreeBackend<TInput, T, F>,
{
    type Target = AbstractTree<B::Model>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl<TInput: Clone + 'static, T: Clone + Eq + Hash + 'static, F: Clone + 'static> AsyncDataTree<TInput, T, F, ObjectBackend> {
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn IAsyncDataTreeRenderer<TInput, T, F>>>,
        data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
        options: AsyncDataTreeOptions<T, F>,
    ) -> Rc<Self> {
        Self::construct(user, ui, container, delegate, renderers, data_source, options, ObjectBackend)
    }
}

impl<TInput, T, F, B> AsyncDataTree<TInput, T, F, B>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
    B: AsyncDataTreeBackend<TInput, T, F>,
{
    /// O construtor, com o backend já escolhido.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn construct(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<B::Renderer>>,
        data_source: Rc<dyn IAsyncDataSource<TInput, T>>,
        options: AsyncDataTreeOptions<T, F>,
        backend: B,
    ) -> Rc<Self> {
        let on_did_change_node_slow_state = Emitter::<AsyncNode<TInput, T>>::new();
        let node_mapper = Rc::new(AsyncNodeMapper::<TInput, T, F>::default());
        let parts = TreeParts { ui: ui.clone(), node_mapper: node_mapper.clone(), on_did_change_node_slow_state: on_did_change_node_slow_state.event() };
        let tree = backend.create_tree(parts, user, container, delegate, renderers, &options);

        let root = create_async_data_tree_node(None, None, None, true, None);
        let nodes = Rc::new(RefCell::new(HashMap::from([(None, root.clone())])));
        let collapse_by_default = options.collapse_by_default.clone();

        let this = Rc::new(Self {
            user: user.to_owned(),
            tree,
            root,
            nodes,
            sorter: options.sorter,
            get_default_collapse_state: Box::new(move |element: &T| {
                collapse_by_default.as_ref().map(|f| if f(element) { ObjectTreeElementCollapseState::PreserveOrCollapsed } else { ObjectTreeElementCollapseState::PreserveOrExpanded })
            }),
            sub_tree_refresh_promises: RefCell::default(),
            refresh_promises: RefCell::default(),
            identity_provider: options.identity_provider,
            auto_expand_single_children: options.auto_expand_single_children,
            data_source,
            on_did_render: Emitter::new(),
            on_did_change_node_slow_state,
            node_mapper,
            disposables: DisposableStore::new(),
            backend,
        });

        let weak = Rc::downgrade(&this);
        this.tree.on_did_change_collapse_state().listen_in(
            move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_collapse_state(event);
                }
            },
            &this.disposables,
        );

        this
    }

    /// A árvore abstrata por baixo (para quem precisa do `Rc`, como o `Deref` não dá).
    pub fn abstract_tree(&self) -> &Rc<AbstractTree<B::Model>> {
        &self.tree
    }

    // Eventos traduzidos

    /// `onDidChangeFocus`.
    pub fn on_did_change_focus(&self) -> BaseEvent<TreeEvent<T>> {
        self.tree.on_did_change_focus().map(as_tree_event::<TInput, T>, None)
    }

    /// `onDidChangeSelection`.
    pub fn on_did_change_selection(&self) -> BaseEvent<TreeEvent<T>> {
        self.tree.on_did_change_selection().map(as_tree_event::<TInput, T>, None)
    }

    /// `onMouseClick`.
    pub fn on_mouse_click(&self) -> BaseEvent<TreeMouseEvent<T>> {
        self.tree.on_mouse_click().map(as_tree_mouse_event::<TInput, T>, None)
    }

    /// `onMouseDblClick`.
    pub fn on_mouse_dbl_click(&self) -> BaseEvent<TreeMouseEvent<T>> {
        self.tree.on_mouse_dbl_click().map(as_tree_mouse_event::<TInput, T>, None)
    }

    /// `onPointer`.
    pub fn on_pointer(&self) -> BaseEvent<TreeMouseEvent<T>> {
        self.tree.on_pointer().map(as_tree_mouse_event::<TInput, T>, None)
    }

    /// `expandOnlyOnTwistieClick`.
    pub fn expand_only_on_twistie_click(&self) -> ExpandOnlyOnTwistieClick<T> {
        match self.tree.expand_only_on_twistie_click() {
            ExpandOnlyOnTwistieClick::Always(always) => ExpandOnlyOnTwistieClick::Always(always),
            ExpandOnlyOnTwistieClick::When(f) => {
                let nodes = self.nodes.clone();
                ExpandOnlyOnTwistieClick::When(Rc::new(move |element: &T| {
                    let node = nodes.borrow().get(&Some(element.clone())).cloned();
                    f(&node)
                }))
            }
        }
    }

    // Widget

    /// `lastVisibleElement`.
    pub fn last_visible_element(&self) -> T {
        self.tree.last_visible_element().expect("a árvore não tem elementos visíveis").element_t()
    }

    /// `navigate(start?)`.
    pub fn navigate(&self, start: Option<&T>) -> AsyncDataTreeNavigator<TInput, T, B::Model> {
        let start_node = start.map(|start| Some(self.get_data_node(&InputOrElement::Element(start.clone()))));
        AsyncDataTreeNavigator { navigator: self.tree.navigate(start_node.as_ref()), marker: PhantomData }
    }

    // Model

    /// `getInput`.
    pub fn get_input(&self) -> Option<TInput> {
        self.root.element.borrow().as_ref().and_then(|element| element.as_input().cloned())
    }

    /// `setInput`.
    pub fn set_input(self: &Rc<Self>, input: TInput, view_state: Option<IAsyncDataTreeViewState>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            this.cancel_all_refresh_promises(false);

            *this.root.element.borrow_mut() = Some(InputOrElement::Input(input.clone()));

            let view_state_context = view_state.clone().map(ViewStateContext::new);

            this.update_children_inner(Some(InputOrElement::Input(input)), true, false, view_state_context.clone(), None).await?;

            if let Some(context) = &view_state_context {
                let focus: Vec<_> = context.focus.borrow().iter().cloned().map(Some).collect();
                let selection: Vec<_> = context.selection.borrow().iter().cloned().map(Some).collect();
                this.tree.set_focus(&focus, None);
                this.tree.set_selection(&selection, None);
            }

            if let Some(scroll_top) = view_state.and_then(|view_state| view_state.scroll_top) {
                this.tree.set_scroll_top(scroll_top);
            }

            Ok(())
        })
    }

    /// `updateChildren(element = this.root.element, recursive = true, rerender = false, options?)`.
    pub fn update_children(self: &Rc<Self>, element: Option<&InputOrElement<TInput, T>>, recursive: bool, rerender: bool, options: Option<AsyncDataTreeUpdateChildrenOptions<T>>) -> Promise<()> {
        let element = element.cloned().or_else(|| self.root.element.borrow().clone());
        let this = self.clone();

        async_fn(async move { this.update_children_inner(element, recursive, rerender, None, options).await })
    }

    /// `cancelAllRefreshPromises`.
    pub fn cancel_all_refresh_promises(&self, include_sub_trees: bool) {
        let refresh_promises = std::mem::take(&mut *self.refresh_promises.borrow_mut());

        for promise in refresh_promises.values() {
            promise.cancel();
        }

        if include_sub_trees {
            let sub_tree_refresh_promises = std::mem::take(&mut *self.sub_tree_refresh_promises.borrow_mut());

            for promise in sub_tree_refresh_promises.values() {
                promise.cancel();
            }
        }
    }

    /// `_updateChildren`.
    fn update_children_inner(
        self: &Rc<Self>,
        element: Option<InputOrElement<TInput, T>>,
        recursive: bool,
        rerender: bool,
        view_state_context: Option<Rc<ViewStateContext<TInput, T>>>,
        options: Option<AsyncDataTreeUpdateChildrenOptions<T>>,
    ) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            let Some(element) = element.filter(|_| this.root.element.borrow().is_some()) else {
                return Err(tree_error(&this.user, "Tree input not set"));
            };

            this.wait_for_refresh(&this.root).await?;

            let node = this.get_data_node(&element);
            this.refresh_and_render_node(node.clone(), recursive, view_state_context, options).await?;

            if rerender {
                // missing nodes are fine, this could've resulted from
                // parallel refresh calls, removing `node` altogether
                let _ = errors::catch(|| this.tree.rerender_element(Some(&node)));
            }

            Ok(())
        })
    }

    /// O `if (node.refreshPromise) { await node.refreshPromise; await Event.toPromise(this._onDidRender.event); }`
    /// que o `_updateChildren` e o `expand` repetem.
    async fn wait_for_refresh(&self, node: &AsyncNode<TInput, T>) -> Result<(), Error> {
        let refresh_promise = node.refresh_promise.borrow().clone();

        if let Some(refresh_promise) = refresh_promise {
            refresh_promise.await?;
            self.on_did_render.event().to_promise(None).await?;
        }

        Ok(())
    }

    /// `resort(element = this.root.element, recursive = true)`.
    pub fn resort(&self, element: Option<&InputOrElement<TInput, T>>, recursive: bool) {
        let node = self.data_node_of(element);
        self.backend.resort(&self.tree, &self.tree_ref(&node), recursive);
    }

    /// `hasNode`.
    pub fn has_node(&self, element: &InputOrElement<TInput, T>) -> bool {
        let InputOrElement::Element(element) = element else {
            return true; // `element === this.root.element`
        };

        let node = self.nodes.borrow().get(&Some(element.clone())).cloned();

        match node {
            None => false,
            Some(node) => self.tree.has_element(&node),
        }
    }

    // View

    /// `rerender(element?)`.
    pub fn rerender(&self, element: Option<&T>) {
        match element {
            None => self.tree.rerender_element(None),
            Some(element) => {
                let node = self.get_data_node(&InputOrElement::Element(element.clone()));
                self.tree.rerender_element(Some(&node));
            }
        }
    }

    /// `updateElementHeight`.
    pub fn update_element_height(&self, element: &T, height: Option<f64>) {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        ObjectTreeMethods::update_element_height(&*self.tree, &node, height);
    }

    /// `updateWidth`.
    pub fn update_width(&self, element: &T) {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.update_width(&Some(node));
    }

    // Tree

    /// `getNode(element = this.root.element)`.
    pub fn get_node(&self, element: Option<&InputOrElement<TInput, T>>) -> Rc<AsyncDataTreeNodeWrapper<TInput, T, F>> {
        let data_node = self.data_node_of(element);
        let node = self.tree.get_node(&self.tree_ref(&data_node));
        AsyncDataTreeNodeWrapper::mapped(&self.node_mapper, &node)
    }

    /// `collapse`.
    pub fn collapse(&self, element: &T, recursive: bool) -> bool {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.collapse(&self.tree_ref(&node), recursive)
    }

    /// `expand`.
    pub fn expand(self: &Rc<Self>, element: &T, recursive: bool) -> Promise<bool> {
        let this = self.clone();
        let element = InputOrElement::Element(element.clone());

        async_fn(async move {
            if this.root.element.borrow().is_none() {
                return Err(tree_error(&this.user, "Tree input not set"));
            }

            this.wait_for_refresh(&this.root).await?;

            let node = this.get_data_node(&element);

            if this.tree.has_element(&node) && !this.tree.is_collapsible(&Some(node.clone())) {
                return Ok(false);
            }

            this.wait_for_refresh(&node).await?;

            if node != this.root && node.refresh_promise.borrow().is_none() && !this.tree.is_collapsed(&Some(node.clone())) {
                return Ok(false);
            }

            let result = this.tree.expand(&this.tree_ref(&node), recursive);

            this.wait_for_refresh(&node).await?;

            Ok(result)
        })
    }

    /// `toggleCollapsed`.
    pub fn toggle_collapsed(&self, element: &T, recursive: bool) -> bool {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.toggle_collapsed(&Some(node), recursive)
    }

    /// `expandTo`.
    pub fn expand_to(self: &Rc<Self>, element: &T) -> Promise<()> {
        let this = self.clone();
        let mut element = InputOrElement::Element(element.clone());

        async_fn(async move {
            let missing_get_parent = || Error::new("Can't expand to element without getParent method");
            let mut elements: Vec<T> = Vec::new();

            while !this.has_node(&element) {
                element = this.data_source.get_parent(element.as_element().expect("a entrada sempre é um nó"))
                    .ok_or_else(missing_get_parent)?;

                if let InputOrElement::Element(parent) = &element {
                    elements.push(parent.clone());
                }
            }

            for element in elements.iter().rev() {
                this.expand(element, false).await?;
            }

            this.tree.expand_to(&Some(this.get_data_node(&element)));

            Ok(())
        })
    }

    /// `isCollapsible`.
    pub fn is_collapsible(&self, element: &T) -> bool {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.is_collapsible(&Some(node))
    }

    /// `isCollapsed`.
    pub fn is_collapsed(&self, element: &InputOrElement<TInput, T>) -> bool {
        let node = self.get_data_node(element);
        self.tree.is_collapsed(&Some(node))
    }

    /// `setAnchor`.
    pub fn set_anchor(&self, element: Option<&T>) {
        let node = element.map(|element| Some(self.get_data_node(&InputOrElement::Element(element.clone()))));
        self.tree.set_anchor(node.as_ref());
    }

    /// `getAnchor`.
    pub fn get_anchor(&self) -> Option<T> {
        self.tree.get_anchor().flatten().map(|node| node.element_t())
    }

    /// `setSelection`.
    pub fn set_selection(&self, elements: &[T], browser_event: Option<BrowserEvent>) {
        let nodes: Vec<_> = elements.iter().map(|element| Some(self.get_data_node(&InputOrElement::Element(element.clone())))).collect();
        self.tree.set_selection(&nodes, browser_event);
    }

    /// `getSelection`.
    pub fn get_selection(&self) -> Vec<T> {
        Self::elements_of(self.tree.get_selection())
    }

    /// `setFocus`.
    pub fn set_focus(&self, elements: &[T], browser_event: Option<BrowserEvent>) {
        let nodes: Vec<_> = elements.iter().map(|element| Some(self.get_data_node(&InputOrElement::Element(element.clone())))).collect();
        self.tree.set_focus(&nodes, browser_event);
    }

    /// `getFocus`.
    pub fn get_focus(&self) -> Vec<T> {
        Self::elements_of(self.tree.get_focus())
    }

    /// `getStickyScrollFocus`.
    pub fn get_sticky_scroll_focus(&self) -> Vec<T> {
        Self::elements_of(self.tree.get_sticky_scroll_focus())
    }

    /// Os elementos dos nós (`nodes.map(n => n!.element as T)`).
    fn elements_of(nodes: Vec<Option<AsyncNode<TInput, T>>>) -> Vec<T> {
        nodes.into_iter().map(|node| node.expect("o nó nulo não é elemento").element_t()).collect()
    }

    /// `reveal`.
    pub fn reveal(&self, element: &T, relative_top: Option<f64>) {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.reveal(&Some(node), relative_top);
    }

    /// `getRelativeTop`.
    pub fn get_relative_top(&self, element: &T) -> Option<f64> {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.get_relative_top(&Some(node))
    }

    // Tree navigation

    /// `getParentElement`: `None` é o pai nulo (a raiz).
    pub fn get_parent_element(&self, element: &T) -> Option<InputOrElement<TInput, T>> {
        let node = self.get_data_node(&InputOrElement::Element(element.clone()));
        self.tree.get_parent_element(&Some(node)).map(|node| node.element_value())
    }

    /// `getFirstElementChild(element = this.root.element)`.
    pub fn get_first_element_child(&self, element: Option<&InputOrElement<TInput, T>>) -> Option<InputOrElement<TInput, T>> {
        let data_node = self.data_node_of(element);
        self.tree.get_first_element_child(&self.tree_ref(&data_node)).flatten().map(|node| node.element_value())
    }

    // Implementation

    /// O nó de dados do elemento, ou a raiz se não houver (o `element = this.root.element`).
    fn data_node_of(&self, element: Option<&InputOrElement<TInput, T>>) -> AsyncNode<TInput, T> {
        match element {
            Some(element) => self.get_data_node(element),
            None => self.root.clone(),
        }
    }

    /// O `node === this.root ? null : node` das chamadas à árvore de baixo.
    fn tree_ref(&self, node: &AsyncNode<TInput, T>) -> Option<AsyncNode<TInput, T>> {
        if *node == self.root { None } else { Some(node.clone()) }
    }

    /// `getDataNode`.
    pub fn get_data_node(&self, element: &InputOrElement<TInput, T>) -> AsyncNode<TInput, T> {
        let node = self.nodes.borrow().get(&element.as_element().cloned()).cloned();

        let Some(node) = node else {
            let identity = self.identity_provider.as_ref().zip(element.as_element()).map(|(provider, element)| provider.get_id(element)).filter(|id| !id.is_empty());
            throw(&tree_error(&self.user, &format!("Data tree node not found{}", identity.map(|id| format!(": {id}")).unwrap_or_default())).to_string())
        };

        node
    }

    /// `refreshAndRenderNode`.
    fn refresh_and_render_node(
        self: &Rc<Self>,
        node: AsyncNode<TInput, T>,
        recursive: bool,
        view_state_context: Option<Rc<ViewStateContext<TInput, T>>>,
        options: Option<AsyncDataTreeUpdateChildrenOptions<T>>,
    ) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            if this.disposables.is_disposed() {
                return Ok(()); // tree disposed during refresh, again (#228211)
            }

            this.refresh_node(node.clone(), recursive, view_state_context.clone()).await?;

            if this.disposables.is_disposed() {
                return Ok(()); // tree disposed during refresh (#199264)
            }

            this.backend.render(&this, &node, view_state_context.as_ref(), options);

            Ok(())
        })
    }

    /// `refreshNode`.
    fn refresh_node(self: &Rc<Self>, node: AsyncNode<TInput, T>, recursive: bool, view_state_context: Option<Rc<ViewStateContext<TInput, T>>>) -> Promise<()> {
        let this = self.clone();

        async_fn_maybe(async move {
            let intersecting = {
                let sub_tree_refresh_promises = this.sub_tree_refresh_promises.borrow();
                sub_tree_refresh_promises.iter().find(|(refresh_node, _)| intersects(refresh_node, &node)).map(|(_, refresh_promise)| refresh_promise.clone())
            };

            if let Some(refresh_promise) = intersecting {
                let (this, node, view_state_context) = (this.clone(), node.clone(), view_state_context.clone());
                return Ok(MaybePromise::Promise(refresh_promise.then_promise(move |()| this.refresh_node(node, recursive, view_state_context))));
            }

            if node != this.root {
                let tree_node = this.tree.get_node(&Some(node.clone()));

                if tree_node.collapsed() {
                    node.has_children.set(this.data_source.has_children(&node.element_value()));
                    node.stale.set(true);
                    this.set_children(&node, Vec::new(), recursive, view_state_context.as_ref());
                    return Ok(MaybePromise::Value(()));
                }
            }

            Ok(MaybePromise::Promise(this.do_refresh_sub_tree(node, recursive, view_state_context)))
        })
    }

    /// `doRefreshSubTree`.
    fn do_refresh_sub_tree(self: &Rc<Self>, node: AsyncNode<TInput, T>, recursive: bool, view_state_context: Option<Rc<ViewStateContext<TInput, T>>>) -> Promise<()> {
        let this = self.clone();

        async_fn_maybe(async move {
            let cancelable_promise = create_cancelable_promise({
                let (this, node, view_state_context) = (this.clone(), node.clone(), view_state_context.clone());

                move |_token| {
                    async_fn(async move {
                        let children_to_refresh = this.do_refresh_node(node.clone(), recursive, view_state_context.clone()).await?;
                        node.stale.set(false);

                        let refreshes = children_to_refresh.iter().map(|child| this.do_refresh_sub_tree(child.clone(), recursive, view_state_context.clone())).collect();
                        promises::settled(refreshes).await?;

                        Ok(())
                    })
                }
            });

            *node.refresh_promise.borrow_mut() = Some(cancelable_promise.clone());
            this.sub_tree_refresh_promises.borrow_mut().insert(node.clone(), cancelable_promise.clone());

            {
                let (this, node) = (this.clone(), node.clone());

                cancelable_promise.finally(move || {
                    *node.refresh_promise.borrow_mut() = None;
                    this.sub_tree_refresh_promises.borrow_mut().remove(&node);
                });
            }

            Ok(MaybePromise::Promise(cancelable_promise.promise.clone()))
        })
    }

    /// `doRefreshNode`.
    fn do_refresh_node(self: &Rc<Self>, node: AsyncNode<TInput, T>, recursive: bool, view_state_context: Option<Rc<ViewStateContext<TInput, T>>>) -> Promise<Vec<AsyncNode<TInput, T>>> {
        let this = self.clone();

        async_fn(async move {
            node.has_children.set(this.data_source.has_children(&node.element_value()));

            let children_promise: Promise<Vec<T>> = if !node.has_children.get() {
                Promise::resolved(Vec::new())
            } else {
                match this.do_get_children(&node) {
                    MaybePromise::Value(children) => Promise::resolved(children),
                    MaybePromise::Promise(children) => {
                        let slow_timeout = timeout(800);

                        {
                            let (this, node) = (this.clone(), node.clone());

                            slow_timeout.then_result(move |result| {
                                if result.is_ok() {
                                    node.slow.set(true);
                                    this.on_did_change_node_slow_state.fire(node.clone());
                                }

                                Ok(())
                            });
                        }

                        children.finally(move || slow_timeout.cancel())
                    }
                }
            };

            let awaited = children_promise.await;

            let outcome = match awaited {
                Ok(children) => errors::catch(|| this.set_children(&node, children, recursive, view_state_context.as_ref())),
                Err(error) => Err(error),
            };

            let result = outcome.or_else(|error| {
                errors::catch(|| {
                    if node != this.root && this.tree.has_element(&node) {
                        this.tree.collapse(&Some(node.clone()), false);
                    }
                })?;

                if is_cancellation_error(&error) { Ok(Vec::new()) } else { Err(error) }
            });

            if node.slow.get() {
                node.slow.set(false);
                this.on_did_change_node_slow_state.fire(node.clone());
            }

            result
        })
    }

    /// `doGetChildren`.
    fn do_get_children(self: &Rc<Self>, node: &AsyncNode<TInput, T>) -> MaybePromise<Vec<T>> {
        let existing = self.refresh_promises.borrow().get(node).cloned();

        if let Some(result) = existing {
            return MaybePromise::Promise(result.promise);
        }

        match self.data_source.get_children(&node.element_value()) {
            MaybePromise::Value(children) => MaybePromise::Value(self.process_children(children)),
            MaybePromise::Promise(children) => {
                let result = {
                    let this = self.clone();

                    create_cancelable_promise(move |_token| async_fn(async move { Ok(this.process_children(children.await?)) }))
                };

                self.refresh_promises.borrow_mut().insert(node.clone(), result.clone());

                let (this, node) = (self.clone(), node.clone());

                MaybePromise::Promise(result.promise.finally(move || {
                    this.refresh_promises.borrow_mut().remove(&node);
                }))
            }
        }
    }

    /// `_onDidChangeCollapseState`.
    fn on_did_change_collapse_state(self: &Rc<Self>, event: &CollapseStateChangeEvent<<B::Model as ITreeModel>::Node>) {
        let Some(element) = event.node.element() else { return };

        if !event.node.collapsed() && element.stale.get() {
            if event.deep {
                self.collapse(&element.element_t(), false);
            } else {
                self.refresh_and_render_node(element, false, None, None).catch(|error| {
                    errors::on_unexpected_error(&error);
                });
            }
        }
    }

    /// `setChildren`.
    fn set_children(&self, node: &AsyncNode<TInput, T>, children_elements: Vec<T>, recursive: bool, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>) -> Vec<AsyncNode<TInput, T>> {
        // perf: if the node was and still is a leaf, avoid all this hassle
        if node.children.borrow().is_empty() && children_elements.is_empty() {
            return Vec::new();
        }

        let mut nodes_to_forget: HashMap<T, AsyncNode<TInput, T>> = HashMap::new();
        let mut children_tree_nodes_by_id: HashMap<String, (AsyncNode<TInput, T>, bool)> = HashMap::new();

        for child in node.children.borrow().iter() {
            nodes_to_forget.insert(child.element_t(), child.clone());

            if self.identity_provider.is_some() {
                let collapsed = self.tree.has_element(child) && self.tree.is_collapsed(&Some(child.clone()));
                children_tree_nodes_by_id.insert(child.id.clone().unwrap_or_default(), (child.clone(), collapsed));
            }
        }

        let mut children_to_refresh: Vec<AsyncNode<TInput, T>> = Vec::new();

        let children: Vec<AsyncNode<TInput, T>> = children_elements
            .into_iter()
            .map(|element| {
                let has_children = self.data_source.has_children(&InputOrElement::Element(element.clone()));

                let Some(identity_provider) = &self.identity_provider else {
                    let async_data_tree_node = create_async_data_tree_node(Some(InputOrElement::Element(element.clone())), Some(node), None, has_children, (self.get_default_collapse_state)(&element));

                    if has_children && async_data_tree_node.default_collapse_state == Some(ObjectTreeElementCollapseState::PreserveOrExpanded) {
                        children_to_refresh.push(async_data_tree_node.clone());
                    }

                    return async_data_tree_node;
                };

                let id = identity_provider.get_id(&element);

                if let Some((async_data_tree_node, collapsed)) = children_tree_nodes_by_id.get(&id) {
                    let old_element = async_data_tree_node.element_t();
                    nodes_to_forget.remove(&old_element);
                    self.nodes.borrow_mut().remove(&Some(old_element));
                    self.nodes.borrow_mut().insert(Some(element.clone()), async_data_tree_node.clone());

                    *async_data_tree_node.element.borrow_mut() = Some(InputOrElement::Element(element));
                    async_data_tree_node.has_children.set(has_children);

                    if recursive {
                        if *collapsed {
                            let old_children = std::mem::take(&mut *async_data_tree_node.children.borrow_mut());

                            for child in &old_children {
                                dfs(child, &mut |node| {
                                    self.nodes.borrow_mut().remove(&Some(node.element_t()));
                                });
                            }

                            async_data_tree_node.stale.set(true);
                        } else {
                            children_to_refresh.push(async_data_tree_node.clone());
                        }
                    } else if has_children && !*collapsed {
                        children_to_refresh.push(async_data_tree_node.clone());
                    }

                    return async_data_tree_node.clone();
                }

                let child_async_data_tree_node = create_async_data_tree_node(Some(InputOrElement::Element(element.clone())), Some(node), Some(id.clone()), has_children, (self.get_default_collapse_state)(&element));

                if let Some(context) = view_state_context {
                    if context.view_state.focus.as_ref().is_some_and(|focus| focus.contains(&id)) {
                        context.focus.borrow_mut().push(child_async_data_tree_node.clone());
                    }

                    if context.view_state.selection.as_ref().is_some_and(|selection| selection.contains(&id)) {
                        context.selection.borrow_mut().push(child_async_data_tree_node.clone());
                    }
                }

                // O original tem dois ramos com o mesmo corpo (expandido no estado de visão; ou com filhos e
                // expandido por padrão): é a disjunção.
                let expanded_in_view_state = view_state_context.is_some_and(|context| context.view_state.expanded.as_ref().is_some_and(|expanded| expanded.contains(&id)));

                if expanded_in_view_state || (has_children && child_async_data_tree_node.default_collapse_state == Some(ObjectTreeElementCollapseState::PreserveOrExpanded)) {
                    children_to_refresh.push(child_async_data_tree_node.clone());
                }

                child_async_data_tree_node
            })
            .collect();

        for node in nodes_to_forget.values() {
            dfs(node, &mut |node| {
                self.nodes.borrow_mut().remove(&Some(node.element_t()));
            });
        }

        for child in &children {
            self.nodes.borrow_mut().insert(Some(child.element_t()), child.clone());
        }

        *node.children.borrow_mut() = children.clone();

        // TODO@joao this doesn't take filter into account
        if *node != self.root && children.len() == 1 && children_to_refresh.is_empty() && self.backend.should_auto_expand_single_child(&self.tree, self.auto_expand_single_children, &children[0]) {
            children[0].force_expanded.set(true);
            children_to_refresh.push(children[0].clone());
        }

        children_to_refresh
    }

    /// `render` (a versão da `AsyncDataTree`; os backends a chamam).
    pub(crate) fn render_base(&self, node: &AsyncNode<TInput, T>, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>, options: Option<AsyncDataTreeUpdateChildrenOptions<T>>) {
        let children_nodes = node.children.borrow().clone();
        let children: Vec<_> = children_nodes.iter().map(|node| self.as_tree_element(node, view_state_context)).collect();

        let object_tree_options = options.map(|options| ObjectTreeModelSetChildrenOptions {
            diff_depth: options.diff_depth,
            diff_identity_provider: options.diff_identity_provider.map(|provider| Rc::new(NodeIdentityProvider(provider)) as Rc<dyn IIdentityProvider<AsyncNode<TInput, T>>>),
            on_did_create_node: None,
            on_did_delete_node: None,
        });

        self.backend.set_children(&self.tree, &self.tree_ref(node), children, object_tree_options.unwrap_or_default());

        if *node != self.root {
            self.tree.set_collapsible(&Some(node.clone()), Some(node.has_children.get()));
        }

        self.on_did_render.fire(());
    }

    /// `asTreeElement`.
    fn as_tree_element(&self, node: &AsyncNode<TInput, T>, view_state_context: Option<&Rc<ViewStateContext<TInput, T>>>) -> IObjectTreeElement<AsyncNode<TInput, T>> {
        if node.stale.get() {
            return IObjectTreeElement { element: node.clone(), children: None, collapsible: Some(node.has_children.get()), collapsed: Some(CollapsedValue::Bool(true)) };
        }

        let in_view_state = view_state_context.is_some_and(|context| {
            context.view_state.expanded.as_ref().is_some_and(|expanded| node.id.as_ref().is_some_and(|id| !id.is_empty() && expanded.contains(id)))
        });

        let collapsed = if in_view_state {
            Some(CollapsedValue::Bool(false))
        } else if node.force_expanded.get() {
            node.force_expanded.set(false);
            Some(CollapsedValue::Bool(false))
        } else {
            node.default_collapse_state.map(CollapsedValue::State)
        };

        let children = if node.has_children.get() {
            let children_nodes = node.children.borrow().clone();
            children_nodes.iter().map(|child| self.as_tree_element(child, view_state_context)).collect()
        } else {
            Vec::new()
        };

        IObjectTreeElement { element: node.clone(), children: Some(children), collapsible: Some(node.has_children.get()), collapsed }
    }

    /// `processChildren`.
    fn process_children(&self, children: Vec<T>) -> Vec<T> {
        let mut children = self.backend.filter_children(children);

        if let Some(sorter) = &self.sorter {
            children.sort_by(|a, b| sorter.compare(a, b).cmp(&0));
        }

        children
    }

    // view state

    /// `getViewState`.
    pub fn get_view_state(&self) -> IAsyncDataTreeViewState {
        let Some(identity_provider) = &self.identity_provider else {
            throw(&tree_error(&self.user, "Can't get tree view state without an identity provider").to_string());
        };

        let get_id = |element: &T| identity_provider.get_id(element);
        let focus = self.get_focus().iter().map(get_id).collect();
        let selection = self.get_selection().iter().map(get_id).collect();
        let expanded = self.backend.expanded_ids(&self.tree, &get_id);

        IAsyncDataTreeViewState { focus: Some(focus), selection: Some(selection), expanded: Some(expanded), scroll_top: Some(self.tree.scroll_top()) }
    }
}

impl<TInput, T, F, B> IDisposable for AsyncDataTree<TInput, T, F, B>
where
    TInput: Clone + 'static,
    T: Clone + Eq + Hash + 'static,
    F: Clone + 'static,
    B: AsyncDataTreeBackend<TInput, T, F>,
{
    fn dispose(&self) {
        self.on_did_render.dispose();
        self.on_did_change_node_slow_state.dispose();
        self.disposables.dispose();
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "AsyncDataTree"
    }
}

/// `asTreeEvent`.
fn as_tree_event<TInput: Clone, T: Clone>(event: &TreeEvent<Option<AsyncNode<TInput, T>>>) -> TreeEvent<T> {
    TreeEvent { elements: event.elements.iter().map(|node| node.as_ref().expect("o nó nulo não é elemento").element_t()).collect(), browser_event: event.browser_event }
}

/// `asTreeMouseEvent`.
fn as_tree_mouse_event<TInput: Clone, T: Clone>(event: &TreeMouseEvent<Option<AsyncNode<TInput, T>>>) -> TreeMouseEvent<T> {
    TreeMouseEvent {
        browser_event: event.browser_event,
        is_handled_by_list: event.is_handled_by_list.clone(),
        element: event.element.clone().flatten().map(|node| node.element_t()),
        target: event.target,
    }
}

/// `AsyncDataTreeNavigator<TInput, T>`.
pub struct AsyncDataTreeNavigator<TInput, T, M: ITreeModel> {
    navigator: TreeNavigator<M>,
    marker: PhantomData<(TInput, T)>,
}

impl<TInput: Clone + 'static, T: Clone + 'static, M: ITreeModel<Element = Option<AsyncNode<TInput, T>>>> ITreeNavigator<T> for AsyncDataTreeNavigator<TInput, T, M>
where
    TreeNavigator<M>: ITreeNavigator<Option<AsyncNode<TInput, T>>>,
{
    fn current(&mut self) -> Option<T> {
        self.navigator.current().flatten().map(|node| node.element_t())
    }

    fn previous(&mut self) -> Option<T> {
        self.navigator.previous();
        self.current()
    }

    fn first(&mut self) -> Option<T> {
        self.navigator.first();
        self.current()
    }

    fn last(&mut self) -> Option<T> {
        self.navigator.last();
        self.current()
    }

    fn next(&mut self) -> Option<T> {
        self.navigator.next();
        self.current()
    }
}
