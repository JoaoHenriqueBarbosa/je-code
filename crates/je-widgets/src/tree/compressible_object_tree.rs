//! Porte da parte `CompressibleObjectTree` de `src/vs/base/browser/ui/tree/objectTree.ts`: a árvore de
//! objetos cujos nós de filho único se fundem num só (`1/11/111`), sobre o `CompressibleObjectTreeModel`.
//!
//! Do TypeScript para o Rust:
//! - `CompressibleObjectTree extends ObjectTree` é uma struct que faz `Deref` para a [`AbstractTree`] do
//!   [`CompressibleObjectTreeModel`]; os métodos do `ObjectTree` valem pelo trait [`ObjectTreeMethods`].
//! - O renderer do usuário é um [`ICompressibleTreeRenderer`]: o nó que ele recebe em
//!   `render_compressed_elements` é o nó comprimido (`TreeNode<Option<CompressedRef<T>>, F>`), cujo
//!   `element` tem a lista `elements`.
//! - O `memoize` do `compressedTreeNodeProvider` é o modelo guardado numa célula preenchida no
//!   `create_model`.
//! - Pendente: o `CompressibleStickyScrollDelegate` (que comprime os nós fixados do sticky scroll num só)
//!   não foi portado. Ele depende do `Proxy` que o original cria para a linha fixada ser um objeto diferente
//!   da linha da lista (o `compressedStickyNodes` é um `Map` por nó); o sticky scroll do `je-widgets` usa o
//!   mesmo `Rc` do nó (ver `sticky_scroll.rs`), então o mapa também valeria para a linha da lista.
//!   `// TODO(integração)`.

use std::cell::RefCell;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::Event as BaseEvent;
use je_base::lifecycle::IDisposable;
use je_ui::{NodeId, Ui};

use crate::list::list_types::{IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use crate::tree::abstract_tree::{AbstractTree, AbstractTreeOptions, AbstractTreeOptionsUpdate};
use crate::tree::compressed_object_tree_model::{CompressedRef, CompressibleObjectTreeModel, CompressibleObjectTreeModelOptions, ElementMapper, ICompressedTreeElement};
use crate::tree::index_tree_model::TreeNode;
use crate::tree::object_tree::{OptionDelegate, object_tree_model_options};
use crate::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;
use crate::tree::tree_types::{ITreeModel, ITreeRenderer, ITreeSorter, TreeElementRenderDetails, TreeNodeApi};

type Model<T, F> = CompressibleObjectTreeModel<T, F>;
type CompressedNode<T, F> = TreeNode<Option<CompressedRef<T>>, F>;

/// `ICompressibleTreeRenderer<T, TFilterData, TTemplateData>`.
pub trait ICompressibleTreeRenderer<T: Clone + Eq + Hash + 'static, F: Clone + 'static>: ITreeRenderer<Model<T, F>> {
    fn render_compressed_elements(&self, node: &Rc<CompressedNode<T, F>>, index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>);

    fn dispose_compressed_elements(&self, _node: &Rc<CompressedNode<T, F>>, _index: i64, _template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {}
}

/// `CompressibleTemplateData`.
struct CompressibleTemplateData<T: Clone + Eq + Hash + 'static, F: Clone + 'static> {
    compressed_tree_node: RefCell<Option<Rc<CompressedNode<T, F>>>>,
    data: TemplateData,
}

/// O modelo, preenchido quando o `create_model` roda (o `compressedTreeNodeProvider` do original).
type ModelCell<T, F> = Rc<RefCell<Option<Rc<Model<T, F>>>>>;

/// `CompressibleRenderer<T, TFilterData, TTemplateData>`.
struct CompressibleRenderer<T: Clone + Eq + Hash + 'static, F: Clone + 'static> {
    model: ModelCell<T, F>,
    renderer: Rc<dyn ICompressibleTreeRenderer<T, F>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> CompressibleRenderer<T, F> {
    /// `getCompressedTreeNode`.
    fn compressed_tree_node(&self, element: &Option<T>) -> Rc<CompressedNode<T, F>> {
        let model = self.model.borrow().clone().expect("o modelo ainda não foi criado");
        model.compressed_model().get_node(element)
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> ITreeRenderer<Model<T, F>> for CompressibleRenderer<T, F> {
    fn template_id(&self) -> String {
        self.renderer.template_id()
    }

    fn row_class_name(&self) -> Option<String> {
        self.renderer.row_class_name()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let data = self.renderer.render_template(container);
        Rc::new(CompressibleTemplateData::<T, F> { compressed_tree_node: RefCell::new(None), data })
    }

    fn render_element(&self, node: &Rc<<Model<T, F> as ITreeModel>::Node>, index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        let template = compressible_template::<T, F>(template_data);
        let compressed_tree_node = self.compressed_tree_node(&node.element());

        if compressed_tree_node.element.as_ref().expect("nó comprimido").elements.len() == 1 {
            *template.compressed_tree_node.borrow_mut() = None;
            self.renderer.render_element(node, index, &template.data, details);
        } else {
            *template.compressed_tree_node.borrow_mut() = Some(compressed_tree_node.clone());
            self.renderer.render_compressed_elements(&compressed_tree_node, index, &template.data, details);
        }
    }

    fn dispose_element(&self, node: &Rc<<Model<T, F> as ITreeModel>::Node>, index: i64, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        let template = compressible_template::<T, F>(template_data);
        let compressed = template.compressed_tree_node.borrow().clone();

        match compressed {
            Some(compressed) => self.renderer.dispose_compressed_elements(&compressed, index, &template.data, details),
            None => self.renderer.dispose_element(node, index, &template.data, details),
        }
    }

    fn render_twistie(&self, element: &Option<T>, twistie: NodeId) -> bool {
        self.renderer.render_twistie(element, twistie)
    }

    fn on_did_change_twistie_state(&self) -> Option<BaseEvent<Option<T>>> {
        self.renderer.on_did_change_twistie_state()
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        self.renderer.dispose_template(&compressible_template::<T, F>(template_data).data);
    }
}

fn compressible_template<T: Clone + Eq + Hash + 'static, F: Clone + 'static>(template_data: &TemplateData) -> Rc<CompressibleTemplateData<T, F>> {
    Rc::downcast::<CompressibleTemplateData<T, F>>(template_data.clone()).unwrap_or_else(|_| panic!("o template não é da árvore comprimível"))
}

/// `ICompressibleKeyboardNavigationLabelProvider<T>`.
pub trait ICompressibleKeyboardNavigationLabelProvider<T>: IKeyboardNavigationLabelProvider<Option<T>> {
    /// `getCompressedNodeKeyboardNavigationLabel`.
    fn get_compressed_node_keyboard_navigation_label(&self, elements: &[T]) -> Vec<Option<String>>;
}

/// `asObjectTreeOptions`: o rótulo de navegação de um nó comprimido é o de seus elementos juntos.
struct CompressibleLabelProvider<T: Clone + Eq + Hash + 'static, F: Clone + 'static> {
    model: ModelCell<T, F>,
    provider: Rc<dyn ICompressibleKeyboardNavigationLabelProvider<T>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> IKeyboardNavigationLabelProvider<Option<T>> for CompressibleLabelProvider<T, F> {
    fn get_keyboard_navigation_label(&self, element: &Option<T>) -> Vec<Option<String>> {
        let compressed = self.model.borrow().clone().and_then(|model| je_base::errors::catch(|| model.compressed_model().get_node(element)).ok());

        match compressed.as_ref().and_then(|node| node.element.as_ref()) {
            Some(compressed) if compressed.elements.len() != 1 => self.provider.get_compressed_node_keyboard_navigation_label(&compressed.elements),
            _ => self.provider.get_keyboard_navigation_label(element),
        }
    }
}

/// `ICompressibleObjectTreeOptions<T, TFilterData>`.
pub struct CompressibleObjectTreeOptions<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    pub tree: AbstractTreeOptions<Model<T, F>>,
    pub sorter: Option<Rc<dyn ITreeSorter<T>>>,
    pub compression_enabled: Option<bool>,
    pub element_mapper: Option<ElementMapper<T>>,
    /// O provedor que também rotula nós comprimidos (substitui o `keyboard_navigation_label_provider` de `tree`).
    pub keyboard_navigation_label_provider: Option<Rc<dyn ICompressibleKeyboardNavigationLabelProvider<T>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Default for CompressibleObjectTreeOptions<T, F> {
    fn default() -> Self {
        Self { tree: AbstractTreeOptions::default(), sorter: None, compression_enabled: None, element_mapper: None, keyboard_navigation_label_provider: None }
    }
}

/// `ICompressibleObjectTreeOptionsUpdate<T>`.
pub struct CompressibleObjectTreeOptionsUpdate<T> {
    pub base: AbstractTreeOptionsUpdate<Option<T>>,
    pub compression_enabled: Option<bool>,
}

impl<T> Default for CompressibleObjectTreeOptionsUpdate<T> {
    fn default() -> Self {
        Self { base: AbstractTreeOptionsUpdate::default(), compression_enabled: None }
    }
}

/// `CompressibleObjectTree<T, TFilterData>`.
pub struct CompressibleObjectTree<T: Clone + Eq + Hash + 'static, F: Clone + 'static = ()> {
    tree: Rc<AbstractTree<Model<T, F>>>,
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> Deref for CompressibleObjectTree<T, F> {
    type Target = AbstractTree<Model<T, F>>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> CompressibleObjectTree<T, F> {
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<T>>,
        renderers: Vec<Rc<dyn ICompressibleTreeRenderer<T, F>>>,
        options: CompressibleObjectTreeOptions<T, F>,
    ) -> Rc<Self> {
        let model_cell: ModelCell<T, F> = Rc::default();
        let compressible_renderers: Vec<Rc<dyn ITreeRenderer<Model<T, F>>>> =
            renderers.into_iter().map(|renderer| Rc::new(CompressibleRenderer { model: model_cell.clone(), renderer }) as Rc<dyn ITreeRenderer<Model<T, F>>>).collect();

        let CompressibleObjectTreeOptions { mut tree, sorter, compression_enabled, element_mapper, keyboard_navigation_label_provider } = options;

        if let Some(provider) = keyboard_navigation_label_provider {
            tree.keyboard_navigation_label_provider = Some(Rc::new(CompressibleLabelProvider { model: model_cell.clone(), provider }));
        }

        let create_model = {
            let model_cell = model_cell.clone();

            move |tree_options: &AbstractTreeOptions<Model<T, F>>| {
                let model = Rc::new(CompressibleObjectTreeModel::new(user, CompressibleObjectTreeModelOptions { base: object_tree_model_options(tree_options, sorter), compression_enabled, element_mapper }));
                *model_cell.borrow_mut() = Some(model.clone());
                model
            }
        };

        Rc::new(Self { tree: AbstractTree::new(user, ui, container, Rc::new(OptionDelegate(delegate)), compressible_renderers, tree, create_model) })
    }

    /// A árvore abstrata por baixo (para quem precisa do `Rc`).
    pub fn abstract_tree(&self) -> &Rc<AbstractTree<Model<T, F>>> {
        &self.tree
    }

    /// `setChildren`.
    pub fn set_children(&self, element: &Option<T>, children: Vec<ICompressedTreeElement<T>>, options: ObjectTreeModelSetChildrenOptions<T, F>) {
        self.tree.model.set_children(element, children, options);
    }

    /// `updateOptions`: além das opções da árvore, liga ou desliga a compressão.
    pub fn update_options(&self, update: &CompressibleObjectTreeOptionsUpdate<T>) {
        self.tree.update_options(&update.base);

        if let Some(enabled) = update.compression_enabled {
            self.tree.model.set_compression_enabled(enabled);
        }
    }

    /// `isCompressionEnabled`.
    pub fn is_compression_enabled(&self) -> bool {
        self.tree.model.is_compression_enabled()
    }

    /// `getCompressedTreeNode`.
    pub fn get_compressed_tree_node(&self, element: &Option<T>) -> Rc<CompressedNode<T, F>> {
        self.tree.model.compressed_model().get_node(element)
    }

    /// `resort`.
    pub fn resort(&self, element: &Option<T>, recursive: bool) {
        self.tree.model.resort(element, recursive);
    }
}

impl<T: Clone + Eq + Hash + 'static, F: Clone + 'static> IDisposable for CompressibleObjectTree<T, F> {
    fn dispose(&self) {
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CompressibleObjectTree"
    }
}
