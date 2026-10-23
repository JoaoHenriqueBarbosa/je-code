//! Os contratos de `contextview.ts` que outros widgets consomem (`IAnchor`, `IDelegate`,
//! `IContextViewProvider`, as constantes e a animação de fechamento) e de `contextmenu.ts`
//! (`IContextMenuDelegate`, `IContextMenuProvider`, `IContextMenuEvent`). A implementação da visão de contexto
//! é o módulo [`crate::context_view`]; quem a injeta nos widgets é a camada de plataforma.
//!
//! `HTMLElement | StandardMouseEvent | IAnchor` do `getAnchor` é [`Anchor`]. Os campos opcionais do
//! `IDelegate` (`focus?`, `layout?`, `anchorAlignment?`...) são métodos com implementação padrão. Como
//! `onDOMEvent?` é "definido ou não" no original (muda o que a `ContextView` faz com o evento), a
//! presença dele é [`IContextViewDelegate::handles_dom_events`].

use std::rc::Rc;

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::DisposableRef;
pub use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, IAnchor};
use je_ui::NodeId;

use crate::action_view_items::{IActionViewItem, IActionViewItemOptions};

/// `ContextViewDOMPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextViewDOMPosition {
    Absolute = 1,
    Fixed,
    FixedShadow,
}

pub const CONTEXT_VIEW_MENU_MOTION_CLASS: &str = "context-view-menu-motion";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS: &str = "context-view-menu-motion-closing";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION: f64 = 150.0;
pub const CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE: &str = "--vscode-context-view-close-animation-duration";

/// `contextViewMenuCloseAnimation`: a animação de fechamento dos menus (só com `modern-ui` e movimento ligado).
pub fn context_view_menu_close_animation() -> IContextViewCloseAnimation {
    let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    IContextViewCloseAnimation {
        class_name: CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS.to_owned(),
        duration: CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION,
        required_ancestor_classes: vec![owned(&["modern-ui", "modern-ui-frosted-glass"]), owned(&["monaco-enable-motion"])],
    }
}

/// O que `getAnchor()` devolve: um nó, a posição de um evento de mouse, ou um retângulo avulso.
#[derive(Clone, Copy, Debug)]
pub enum Anchor {
    /// Posiciona a visão no elemento.
    Node(NodeId),
    /// Posiciona a visão no ponteiro (`StandardMouseEvent`: `posx`, `posy`).
    Mouse { x: f64, y: f64 },
    /// Posiciona a visão num retângulo.
    Rect(IAnchor),
}

/// `IContextViewCloseAnimation`.
#[derive(Clone, Debug, PartialEq)]
pub struct IContextViewCloseAnimation {
    pub class_name: String,
    pub duration: f64,
    pub required_ancestor_classes: Vec<Vec<String>>,
}

/// `IDelegate`.
pub trait IContextViewDelegate {
    fn get_anchor(&self) -> Anchor;
    /// `render(container)`: monta o conteúdo no contêiner e devolve o que desmonta.
    fn render(&self, container: NodeId) -> Option<DisposableRef>;
    fn focus(&self) {}
    fn layout(&self) {}
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        None
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        None
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        None
    }
    fn can_relayout(&self) -> bool {
        true
    }
    /// O `onDOMEvent` está definido? Sem ele, um clique de captura fora do contêiner esconde a visão.
    fn handles_dom_events(&self) -> bool {
        false
    }
    /// `onDOMEvent(e, activeElement)`: o evento (`click`, `keydown`, `focus`, `blur` do contêiner) e o nó
    /// em foco. Só é chamado se [`IContextViewDelegate::handles_dom_events`].
    fn on_dom_event(&self, _event: &je_ui::Event, _active_element: Option<NodeId>) {}
    /// `onHide(data)`.
    fn on_hide(&self, _did_cancel: Option<bool>) {}
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        None
    }
    /// `layer`: as visões de camada maior ficam por cima.
    fn layer(&self) -> i32 {
        0
    }
}

/// `IContextViewProvider`.
pub trait IContextViewProvider {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<NodeId>);
    fn hide_context_view(&self);
    fn layout(&self);
}

/// `IContextMenuEvent`: os modificadores do evento que abriu o menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IContextMenuEvent {
    pub shift_key: Option<bool>,
    pub ctrl_key: Option<bool>,
    pub alt_key: Option<bool>,
    pub meta_key: Option<bool>,
}

/// O retorno de `getCheckedActionsRepresentation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckedActionsRepresentation {
    Radio,
    Checkbox,
}

/// `IContextMenuDelegate`. O `ContextMenuLocation` do `getAnchor` é [`Anchor::Rect`] (ou `Node`/`Mouse`).
pub trait IContextMenuDelegate {
    fn get_anchor(&self) -> Anchor;
    fn get_actions(&self) -> Vec<ActionRef>;
    fn get_checked_actions_representation(&self, _action: &ActionRef) -> Option<CheckedActionsRepresentation> {
        None
    }
    fn get_action_view_item(&self, _action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        None
    }
    fn get_actions_context(&self, _event: Option<&IContextMenuEvent>) -> Option<Dynamic> {
        None
    }
    fn get_key_binding(&self, _action: &ActionRef) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }
    fn get_menu_class_name(&self) -> Option<String> {
        None
    }
    fn on_hide(&self, _did_cancel: bool) {}
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        None
    }
    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        None
    }
    fn skip_telemetry(&self) -> bool {
        false
    }
    fn auto_select_first_item(&self) -> bool {
        false
    }
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        None
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        None
    }
    fn dom_for_shadow_root(&self) -> Option<NodeId> {
        None
    }
    /// Monta a raiz de sombra no contêiner da janela em vez de em `dom_for_shadow_root`.
    fn use_window_container_for_shadow_root(&self) -> bool {
        false
    }
    /// Menus de contexto personalizados com camada maior ficam por cima.
    fn layer(&self) -> i32 {
        0
    }
}

/// `IContextMenuProvider`.
pub trait IContextMenuProvider {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>);
}
