//! A implementação nativa do `IContextViewService` de `je_platform::contextview::context_view` (o contrato único, porte de
//! `src/vs/platform/contextview/browser/contextView.ts`) sobre a `ContextView` de `je-widgets` e a árvore de widgets
//! (`je-ui`).
//!
//! O contrato da plataforma trabalha com o [`ViewElement`], que aqui carrega um [`NodeId`] como identificador nativo opaco
//! ([`node_element`] cria, [`native_node`] lê). Quem desenha em nós do `Ui` (as migalhas) recebe o contêiner como
//! `ViewElement` e o converte com [`native_node`].
//!
//! Notas de tradução:
//! - **`hideContextView(data?: any)` e `onHide(data?: any)`**: a `ContextView` base só entrega um `bool` ao `onHide`; o
//!   serviço guarda o `data` do último `hideContextView` e o entrega ao delegado quando a visão de fato esconde (um clique
//!   fora esconde sem `data`, como no original).
//! - **`getAnchor`**: o [`AnchorTarget`] da plataforma vira a `Anchor` de `je-widgets` ([`widget_anchor`]).
//! - **`IContextMenuService`** não tem implementação aqui: a `ContextMenuService` com o `ContextMenuHandler` (que monta o
//!   `Menu`) depende do menu de widgets e vem com ele.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_platform::contextview::context_view::{
    AnchorTarget, HideData, IContextViewCloseAnimation as PlatformCloseAnimation, IContextViewDelegate, IContextViewService,
};
use je_platform::contextview::view_element::ViewElement;
use je_ui::{NodeId, Ui};
use je_base::layout::IAnchor;
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{Anchor, AnchorAlignment, AnchorAxisAlignment, AnchorPosition, ContextViewDOMPosition, IContextViewCloseAnimation};
use je_widgets::context_view_api::IContextViewDelegate as IBaseContextViewDelegate;

/// O `ViewElement` que representa o nó `node` da árvore de widgets.
pub fn node_element(node: NodeId) -> Rc<ViewElement> {
    ViewElement::from_native(Rc::new(node))
}

/// O nó do `Ui` que o elemento representa, ou `None` se ele não é nativo.
pub fn native_node(element: &ViewElement) -> Option<NodeId> {
    element.native_as::<NodeId>()
}

/// `HTMLElement | StandardMouseEvent | IAnchor` da plataforma para a `Anchor` de `je-widgets`. Um elemento não nativo vira o
/// retângulo que ele mede.
pub fn widget_anchor(target: AnchorTarget) -> Anchor {
    match target {
        AnchorTarget::Element(element) => match native_node(&element) {
            Some(node) => Anchor::Node(node),
            None => {
                let rect = element.rect();
                Anchor::Rect(IAnchor { x: rect.left, y: rect.top, width: Some(rect.width), height: Some(rect.height) })
            }
        },
        AnchorTarget::Mouse { posx, posy } => Anchor::Mouse { x: posx, y: posy },
        AnchorTarget::Anchor(anchor) => Anchor::Rect(anchor),
    }
}

/// O delegado do serviço visto pela `ContextView` base: o `onHide(didCancel)` dela vira o `onHide(data)` do original.
struct DelegateAdapter {
    inner: Rc<dyn IContextViewDelegate>,
    pending_hide_data: Rc<RefCell<Option<HideData>>>,
}

impl IBaseContextViewDelegate for DelegateAdapter {
    fn get_anchor(&self) -> Anchor {
        widget_anchor(self.inner.get_anchor())
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        self.inner.render(&node_element(container))
    }

    fn focus(&self) {
        self.inner.focus();
    }

    fn layout(&self) {
        self.inner.layout();
    }

    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.inner.anchor_alignment()
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        self.inner.anchor_position()
    }

    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.inner.anchor_axis_alignment()
    }

    fn can_relayout(&self) -> bool {
        self.inner.can_relayout()
    }

    fn on_hide(&self, _did_cancel: Option<bool>) {
        let data = self.pending_hide_data.borrow_mut().take();
        self.inner.on_hide(data);
    }

    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.inner.close_animation().map(|PlatformCloseAnimation { class_name, duration, required_ancestor_classes }| IContextViewCloseAnimation {
            class_name,
            duration,
            required_ancestor_classes,
        })
    }

    fn layer(&self) -> i32 {
        self.inner.layer().unwrap_or(0)
    }
}

/// `ContextViewService` sobre a `ContextView` de `je-widgets`.
pub struct NativeContextViewService {
    view: Rc<ContextView>,
    pending_hide_data: Rc<RefCell<Option<HideData>>>,
}

impl NativeContextViewService {
    /// `new ContextView(container, ContextViewDOMPosition.ABSOLUTE)` sobre o contêiner da janela.
    pub fn new(ui: &Ui, container: NodeId) -> Rc<Self> {
        Rc::new(Self { view: ContextView::new(ui, container, ContextViewDOMPosition::Absolute), pending_hide_data: Rc::new(RefCell::new(None)) })
    }
}

impl IContextViewService for NativeContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<Rc<ViewElement>>, shadow_root: bool) {
        if let Some(container) = container.as_deref().and_then(native_node) {
            let position = if shadow_root { ContextViewDOMPosition::FixedShadow } else { ContextViewDOMPosition::Absolute };
            self.view.set_container(Some(container), position);
        }
        *self.pending_hide_data.borrow_mut() = None;
        self.view.show(Rc::new(DelegateAdapter { inner: delegate, pending_hide_data: self.pending_hide_data.clone() }));
    }

    fn hide_context_view(&self, data: Option<HideData>) {
        *self.pending_hide_data.borrow_mut() = data;
        self.view.hide();
        // Se a visão não estava aberta (ou já escondia), o `data` não tem a quem ser entregue.
        *self.pending_hide_data.borrow_mut() = None;
    }

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        node_element(self.view.get_view_element())
    }

    fn layout(&self) {
        self.view.layout();
    }
}
