//! A ponte entre o `IContextMenuProvider` de `je-widgets` (o que o `ToolBar`, o `DropdownMenu` e o `ButtonWithDropdown`
//! recebem) e o `IContextMenuService` da plataforma: no original o serviço é o provedor (a assinatura é a mesma),
//! aqui são dois contratos de tipos diferentes, e [`ContextMenuServiceProvider`] converte um no outro.
//!
//! O que o delegado dos widgets carrega e a plataforma não conhece (`getActionViewItem`, `getActionsContext`,
//! `getKeyBinding`) viaja em [`ContextMenuExtras`] pela porta `native_extras` do delegado da plataforma, e o
//! [`ContextMenuHandler`](super::context_menu_handler::ContextMenuHandler) o lê.

use std::any::Any;
use std::rc::Rc;

use je_base::actions::{ActionRef, IActionRunner};
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService, IContextViewCloseAnimation};
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::button::{ContextMenuDelegate, IContextMenuProvider as IButtonContextMenuProvider};
use je_widgets::context_view_api::{Anchor, IContextMenuDelegate as IWidgetContextMenuDelegate, IContextMenuProvider};

use super::context_menu_handler::{ContextMenuExtras, native_extras};
use crate::services::contextview::native_context_view_service::node_element;

/// O `IContextMenuService` visto como `IContextMenuProvider` dos widgets.
pub struct ContextMenuServiceProvider {
    service: Rc<dyn IContextMenuService>,
}

impl ContextMenuServiceProvider {
    pub fn new(service: Rc<dyn IContextMenuService>) -> Rc<Self> {
        Rc::new(Self { service })
    }
}

impl IContextMenuProvider for ContextMenuServiceProvider {
    fn show_context_menu(&self, delegate: Rc<dyn IWidgetContextMenuDelegate>) {
        self.service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(WidgetDelegate { inner: delegate })));
    }
}

/// `HTMLElement | StandardMouseEvent | IAnchor` dos widgets como a âncora da plataforma.
fn anchor_target(anchor: Anchor) -> AnchorTarget {
    match anchor {
        Anchor::Node(node) => AnchorTarget::Element(node_element(node)),
        Anchor::Mouse { x, y } => AnchorTarget::Mouse { posx: x, posy: y },
        Anchor::Rect(anchor) => AnchorTarget::Anchor(anchor),
    }
}

/// O `IContextMenuService` visto como o `IContextMenuProvider` mínimo do botão com menu (`ButtonWithDropdown`).
pub struct ButtonContextMenuServiceProvider {
    service: Rc<dyn IContextMenuService>,
}

impl ButtonContextMenuServiceProvider {
    pub fn new(service: Rc<dyn IContextMenuService>) -> Rc<Self> {
        Rc::new(Self { service })
    }
}

impl IButtonContextMenuProvider for ButtonContextMenuServiceProvider {
    fn show_context_menu(&self, delegate: ContextMenuDelegate) {
        self.service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(ButtonDelegate { delegate })));
    }
}

/// O delegado do botão com menu como delegado da plataforma.
struct ButtonDelegate {
    delegate: ContextMenuDelegate,
}

impl IContextMenuDelegate for ButtonDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        anchor_target((self.delegate.get_anchor)())
    }
    fn get_actions(&self) -> Vec<ActionRef> {
        (self.delegate.get_actions)()
    }
    fn on_hide(&self, _did_cancel: bool) {
        (self.delegate.on_hide)();
    }
    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.delegate.action_runner.clone()
    }
    fn layer(&self) -> Option<i32> {
        self.delegate.layer
    }
}

/// O delegado dos widgets como delegado da plataforma.
struct WidgetDelegate {
    inner: Rc<dyn IWidgetContextMenuDelegate>,
}

impl IContextMenuDelegate for WidgetDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        anchor_target(self.inner.get_anchor())
    }
    fn get_actions(&self) -> Vec<ActionRef> {
        self.inner.get_actions()
    }
    fn get_menu_class_name(&self) -> Option<String> {
        self.inner.get_menu_class_name()
    }
    fn on_hide(&self, did_cancel: bool) {
        self.inner.on_hide(did_cancel);
    }
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.inner.close_animation().map(|animation| IContextViewCloseAnimation {
            class_name: animation.class_name,
            duration: animation.duration,
            required_ancestor_classes: animation.required_ancestor_classes,
        })
    }
    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.inner.action_runner()
    }
    fn skip_telemetry(&self) -> bool {
        self.inner.skip_telemetry()
    }
    fn auto_select_first_item(&self) -> bool {
        self.inner.auto_select_first_item()
    }
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.inner.anchor_alignment()
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.inner.anchor_axis_alignment()
    }
    /// `layer` indefinido (a plataforma) é a camada `0` (os widgets).
    fn layer(&self) -> Option<i32> {
        Some(self.inner.layer()).filter(|layer| *layer != 0)
    }
    fn native_extras(&self) -> Option<Rc<dyn Any>> {
        let (view_item, context, key_binding) = (self.inner.clone(), self.inner.clone(), self.inner.clone());
        native_extras(ContextMenuExtras {
            get_action_view_item: Some(Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| view_item.get_action_view_item(action, options))),
            get_actions_context: Some(Rc::new(move || context.get_actions_context(None))),
            get_key_binding: Some(Rc::new(move |action: &ActionRef| key_binding.get_key_binding(action))),
        })
    }
}
