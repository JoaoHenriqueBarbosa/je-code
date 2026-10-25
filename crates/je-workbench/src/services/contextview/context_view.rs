//! Porte de `src/vs/platform/contextview/browser/contextView.ts` e de `contextViewService.ts` sobre a árvore de widgets
//! nativa (`je-ui`): o [`IContextViewService`] com o `data` livre de `hideContextView` e de `onHide`, e o
//! [`IContextMenuService`].
//!
//! Por que existe ao lado de `je_platform::contextview`: o contrato daquele módulo trabalha com o `ViewElement` (um elemento
//! sem árvore de widgets), e os seletores das migalhas desenham em nós do `Ui` (`NodeId`). Este módulo é o mesmo contrato
//! sobre `NodeId`, montado sobre a `ContextView` de `je-widgets` (que só carrega o `didCancel` booleano no `hide`).
//!
//! Notas de tradução:
//! - **`hideContextView(data?: any)` e `onHide(data?: any)`** são o [`HideData`] (`Rc<dyn Any>`), lido de volta com
//!   [`hide_data_to`]. A `ContextView` base só entrega um `bool` ao `onHide`; o serviço guarda o `data` do último
//!   `hideContextView` e o entrega ao delegado quando a visão de fato esconde (um clique fora esconde sem `data`, como no
//!   original).
//! - **`IOpenContextView`** (o `{ close }` que `showContextView` devolve) é o [`DisposableRef`] devolvido.
//! - **`IContextViewDelegate`** troca o `HTMLElement | StandardMouseEvent | IAnchor` do `getAnchor` pela [`Anchor`] de
//!   `je-widgets`; `onDOMEvent` não entra (as migalhas não o usam).
//! - **`IContextMenuService.showContextMenu(IContextMenuDelegate | IContextMenuMenuDelegate)`** é o [`ContextMenuRequest`].
//!   `IContextMenuMenuDelegate.menuActionOptions` (`IMenuActionOptions`) ainda não existe na plataforma e fica de fora.
//!   Só o contrato do serviço de menu de contexto é portado aqui; a implementação (`ContextMenuService` com o
//!   `ContextMenuHandler`, que monta o `Menu`) depende do menu de widgets e vem com ele.

use std::cell::RefCell;
use std::any::Any;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_instantiation::service_id;
use je_platform::actions::MenuId;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_ui::{NodeId, Ui};
use je_widgets::context_view::ContextView;
use je_widgets::context_view_api::{Anchor, AnchorAlignment, AnchorAxisAlignment, AnchorPosition, ContextViewDOMPosition, IContextMenuDelegate, IContextViewCloseAnimation};
use je_widgets::context_view_api::IContextViewDelegate as IBaseContextViewDelegate;

/// O `data?: any` de `hideContextView` e de `onHide`.
pub type HideData = Rc<dyn Any>;

/// Lê o `data` como `T`, ou `None` se faltar ou for de outro tipo.
pub fn hide_data_to<T: Any + Clone>(data: Option<&HideData>) -> Option<T> {
    data.and_then(|data| data.downcast_ref::<T>()).cloned()
}

/// `IContextViewDelegate`.
pub trait IContextViewDelegate {
    fn can_relayout(&self) -> bool {
        true
    }
    /// `getAnchor()`.
    fn get_anchor(&self) -> Anchor;
    /// `render(container)`: monta o conteúdo e devolve o que desmonta.
    fn render(&self, container: NodeId) -> Option<DisposableRef>;
    /// `onHide(data)`.
    fn on_hide(&self, _data: Option<HideData>) {}
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        None
    }
    fn focus(&self) {}
    fn layout(&self) {}
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        None
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        None
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        None
    }
    /// `layer`: as visões de camada maior ficam por cima.
    fn layer(&self) -> i32 {
        0
    }
}

/// `IContextViewService`.
pub trait IContextViewService {
    /// `showContextView(delegate, container?, shadowRoot?)`.
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<NodeId>, shadow_root: bool) -> DisposableRef;
    /// `hideContextView(data?)`.
    fn hide_context_view(&self, data: Option<HideData>);
    /// `getContextViewElement()`.
    fn get_context_view_element(&self) -> NodeId;
    fn layout(&self);
}

service_id!(pub I_CONTEXT_VIEW_SERVICE: dyn IContextViewService = "contextViewService");

/// O delegado do serviço visto pela `ContextView` base: o `onHide(didCancel)` dela vira o `onHide(data)` do original.
struct DelegateAdapter {
    inner: Rc<dyn IContextViewDelegate>,
    pending_hide_data: Rc<RefCell<Option<HideData>>>,
}

impl IBaseContextViewDelegate for DelegateAdapter {
    fn get_anchor(&self) -> Anchor {
        self.inner.get_anchor()
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        self.inner.render(container)
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
        self.inner.close_animation()
    }

    fn layer(&self) -> i32 {
        self.inner.layer()
    }
}

/// `ContextViewService`.
pub struct ContextViewService {
    view: Rc<ContextView>,
    pending_hide_data: Rc<RefCell<Option<HideData>>>,
}

impl ContextViewService {
    /// `new ContextView(container, ContextViewDOMPosition.ABSOLUTE)` sobre o contêiner da janela.
    pub fn new(ui: &Ui, container: NodeId) -> Rc<Self> {
        Rc::new(Self { view: ContextView::new(ui, container, ContextViewDOMPosition::Absolute), pending_hide_data: Rc::new(RefCell::new(None)) })
    }
}

impl IContextViewService for ContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<NodeId>, shadow_root: bool) -> DisposableRef {
        if let Some(container) = container {
            let position = if shadow_root { ContextViewDOMPosition::FixedShadow } else { ContextViewDOMPosition::Absolute };
            self.view.set_container(Some(container), position);
        }
        *self.pending_hide_data.borrow_mut() = None;
        self.view.show(Rc::new(DelegateAdapter { inner: delegate, pending_hide_data: self.pending_hide_data.clone() }));

        let (view, pending) = (self.view.clone(), self.pending_hide_data.clone());
        to_disposable(move || {
            *pending.borrow_mut() = None;
            view.hide();
        })
    }

    fn hide_context_view(&self, data: Option<HideData>) {
        *self.pending_hide_data.borrow_mut() = data;
        self.view.hide();
        // Se a visão não estava aberta (ou já escondia), o `data` não tem a quem ser entregue.
        *self.pending_hide_data.borrow_mut() = None;
    }

    fn get_context_view_element(&self) -> NodeId {
        self.view.get_view_element()
    }

    fn layout(&self) {
        self.view.layout();
    }
}

/// `IContextMenuMenuDelegate`: `{ menuId?, menuActionOptions?, contextKeyService?, getActions? } & Omit<IContextMenuDelegate, 'getActions'>`.
/// O `Omit<IContextMenuDelegate, 'getActions'>` é o delegado base, cujo `get_actions` é ignorado.
#[derive(Clone)]
pub struct IContextMenuMenuDelegate {
    /// O `MenuId` que preenche o menu de contexto.
    pub menu_id: Option<MenuId>,
    /// O serviço de chaves de contexto que guia o menu.
    pub context_key_service: Option<Rc<dyn IContextKeyService>>,
    /// `getActions?()`: ações extras, colocadas antes das do menu.
    pub get_actions: Option<Rc<dyn Fn() -> Vec<ActionRef>>>,
    /// O resto do `IContextMenuDelegate`.
    pub base: Rc<dyn IContextMenuDelegate>,
}

/// `IContextMenuDelegate | IContextMenuMenuDelegate`.
#[derive(Clone)]
pub enum ContextMenuRequest {
    Delegate(Rc<dyn IContextMenuDelegate>),
    Menu(IContextMenuMenuDelegate),
}

/// `IContextMenuService`.
pub trait IContextMenuService {
    /// `onDidShowContextMenu`.
    fn on_did_show_context_menu(&self) -> Event<()>;
    /// `onDidHideContextMenu`.
    fn on_did_hide_context_menu(&self) -> Event<()>;
    /// `showContextMenu(delegate)`.
    fn show_context_menu(&self, request: ContextMenuRequest);
}

service_id!(pub I_CONTEXT_MENU_SERVICE: dyn IContextMenuService = "contextMenuService");
