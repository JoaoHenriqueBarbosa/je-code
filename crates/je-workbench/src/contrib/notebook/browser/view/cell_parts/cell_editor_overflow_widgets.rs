//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellEditorOverflowWidgets.ts`: `CellEditorOverflowWidgets`, o
//! anfitrião externo dos popups do editor de uma célula (sugestões, dicas de parâmetro, hover), que fica inerte e escondido enquanto o
//! editor da célula não está ativo.
//!
//! Divergências:
//! - `DOM.append(container, DOM.$('.monaco-editor'))` é `create_el` na árvore do `je-ui`; `domNode.inert` é o atributo `inert` do
//!   nó (presente com o valor vazio quando inerte), e `DOM.setVisibility` é `show` e `hide`.
//! - `ICodeEditor` é o contrato estreito [`ICellEditorOverflowEditor`], só com o que a classe usa: o `contextKeyService.createScoped`
//!   e os três cancelamentos. `ContentHoverController.get(editor)?.cancel()`, `ParameterHintsController.get(editor)?.cancel()` e
//!   `SuggestController.get(editor)?.cancelSuggestWidget()` são `cancel_content_hover`, `cancel_parameter_hints` e
//!   `cancel_suggest_widget`; o `?.` (controlador ausente) é a implementação não fazer nada.
//! - `createScoped(this.domNode)` recebe o [`UiNodeTarget`], o nó como alvo do serviço de chaves de contexto: os atributos do alvo são
//!   os do nó na árvore e o `parentElement` é o pai do nó. O adaptador é público porque as demais partes de célula que criam serviços
//!   escopados por nó (barras de ferramentas, barra de status) o reutilizam.
//! - Os 5 casos de `cellEditorOverflowWidgets.test.ts` seguem adiados. O `cellRenderer` já existe (`view/renderers/cell_renderer.rs`),
//!   mas os casos dependem do que ainda não existe no `je-editor`: o `ContentHoverController` (com `HoverStartMode` e
//!   `MarkdownHoverParticipant`), o `ParameterHintsController` (só o modelo está portado), o nó DOM de overflow do `CodeEditorWidget`
//!   (`getOverflowWidgetsDomNode`, onde o popup `.monaco-hover` e o `.suggest-widget` são anexados) e a implementação do
//!   `ICellRendererEditorFactory` sobre o `CodeEditorWidget`. O [`ICellEditorOverflowEditor`] estreito continua até o `CodeEditorWidget`
//!   expor os três cancelamentos.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_platform::contextkey::contextkey::{IContextKeyServiceTarget, IScopedContextKeyService};
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::{create_el, hide, show};

/// `ICodeEditor`, só com o que `CellEditorOverflowWidgets` usa.
pub trait ICellEditorOverflowEditor {
    /// `contextKeyService.createScoped(target)`.
    fn create_scoped_context_key_service(&self, target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService>;
    /// `ContentHoverController.get(editor)?.cancel()`.
    fn cancel_content_hover(&self);
    /// `ParameterHintsController.get(editor)?.cancel()`.
    fn cancel_parameter_hints(&self);
    /// `SuggestController.get(editor)?.cancelSuggestWidget()`.
    fn cancel_suggest_widget(&self);
}

/// Um nó da árvore como alvo do serviço de chaves de contexto escopado (o `HTMLElement` de `createScoped(element)`).
pub struct UiNodeTarget {
    ui: Ui,
    node: NodeId,
}

impl UiNodeTarget {
    pub fn new(ui: &Ui, node: NodeId) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), node })
    }
}

impl IContextKeyServiceTarget for UiNodeTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        let parent = self.ui.tree().parent(self.node)?;
        Some(UiNodeTarget::new(&self.ui, parent))
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.ui.tree_mut().set_attribute(self.node, attr, value);
    }

    fn remove_attribute(&self, attr: &str) {
        self.ui.tree_mut().remove_attribute(self.node, attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.ui.tree().has_attribute(self.node, attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.ui.tree().attribute(self.node, attr).map(str::to_owned)
    }
}

/// `CellEditorOverflowWidgets`.
pub struct CellEditorOverflowWidgets {
    base: Rc<Disposable>,
    ui: Ui,
    /// `domNode`.
    dom_node: NodeId,
    editor: RefCell<Option<Rc<dyn ICellEditorOverflowEditor>>>,
}

impl CellEditorOverflowWidgets {
    pub fn new(ui: &Ui, container: NodeId) -> Rc<Self> {
        let dom_node = create_el(&mut ui.tree_mut(), container, Role::Generic, "monaco-editor");
        let this = Rc::new(Self { base: Disposable::new(), ui: ui.clone(), dom_node, editor: RefCell::new(None) });
        this.set_visible(false);
        this
    }

    /// `domNode`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    pub fn bind_editor(&self, editor: Rc<dyn ICellEditorOverflowEditor>) {
        *self.editor.borrow_mut() = Some(editor.clone());
        let scoped = editor.create_scoped_context_key_service(UiNodeTarget::new(&self.ui, self.dom_node));
        self.base.register(to_disposable(move || scoped.dispose()));
    }

    pub fn set_visible(&self, visible: bool) {
        {
            let mut tree = self.ui.tree_mut();
            if visible {
                tree.remove_attribute(self.dom_node, "inert");
                show(&mut tree, self.dom_node);
            } else {
                tree.set_attribute(self.dom_node, "inert", "");
                hide(&mut tree, self.dom_node);
            }
        }

        if !visible {
            let editor = self.editor.borrow().clone();
            if let Some(editor) = editor {
                editor.cancel_content_hover();
                editor.cancel_parameter_hints();
                editor.cancel_suggest_widget();
            }
        }
    }
}

impl IDisposable for CellEditorOverflowWidgets {
    fn dispose(&self) {
        self.set_visible(false);
        *self.editor.borrow_mut() = None;
        self.ui.tree_mut().remove(self.dom_node);
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellEditorOverflowWidgets"
    }
}
