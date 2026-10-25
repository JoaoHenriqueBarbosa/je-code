//! O contrato de `contextview.ts` e de `contextView.ts` (`IContextViewService`) que o `ActionWidgetService` consome: a âncora
//! (`HTMLElement | StandardMouseEvent | IAnchor`), a animação de fechamento, o `IDelegate` e o serviço.
//!
//! Divergências:
//! - **`onHide(data)` e `hideContextView(data)`.** O `data: any` do original é o [`HideData`] (`Rc<dyn Any>`): quem esconde
//!   (o `ActionWidgetService` com o `didCancel`, as migalhas com `{ source, didPick, sideBySide }`) entrega o valor que
//!   o `onHide` do delegado lê de volta com [`hide_data_to`], que é o `data?.didPick` do original.
//! - **`IOpenContextView`.** O `showContextView` devolve `{ close }` no original; o `ActionWidgetService` descarta o retorno,
//!   então o contrato não o devolve.
//! - **`onDOMEvent`, `useWindowContainerForShadowRoot`.** Os eventos de DOM do contêiner e a raiz de sombra não existem na
//!   árvore nativa; entram com os widgets que os usam.

use std::any::Any;
use std::rc::Rc;

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::parts::contextmenu::IContextMenuEvent;
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, IAnchor, IRect};
use je_base::lifecycle::DisposableRef;
use je_instantiation::service_id;

use super::view_element::ViewElement;
use crate::actions::{MenuActionOptions, MenuId};
use crate::contextkey::contextkey::IContextKeyService;

pub const CONTEXT_VIEW_MENU_MOTION_CLASS: &str = "context-view-menu-motion";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS: &str = "context-view-menu-motion-closing";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION: f64 = 150.0;
pub const CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE: &str = "--vscode-context-view-close-animation-duration";

/// `CONTEXT_VIEW_MENU_MOTION_ANCESTOR_CLASSES`: `[['modern-ui', 'modern-ui-frosted-glass'], 'monaco-enable-motion']`. Cada
/// elemento é uma exigência; as classes dentro dele são alternativas (`some`), e uma `string` do original é a lista de uma
/// classe só.
pub fn context_view_menu_motion_ancestor_classes() -> Vec<Vec<String>> {
    vec![vec!["modern-ui".to_owned(), "modern-ui-frosted-glass".to_owned()], vec!["monaco-enable-motion".to_owned()]]
}

/// `IContextViewCloseAnimation`.
#[derive(Clone, Debug, PartialEq)]
pub struct IContextViewCloseAnimation {
    pub class_name: String,
    pub duration: f64,
    pub required_ancestor_classes: Vec<Vec<String>>,
}

/// `hasRequiredAncestorClasses(element, classNames)`: o elemento ou algum ancestral tem todas as exigências.
pub fn has_required_ancestor_classes(element: &Rc<ViewElement>, class_names: &[Vec<String>]) -> bool {
    if class_names.is_empty() {
        return true;
    }
    let mut candidate = Some(element.clone());
    while let Some(current) = candidate {
        if class_names.iter().all(|requirement| requirement.iter().any(|class_name| current.class_list_contains(class_name))) {
            return true;
        }
        candidate = current.parent_element();
    }
    false
}

/// O `data?: any` de `hideContextView` e de `onHide`.
pub type HideData = Rc<dyn Any>;

/// Lê o `data` como `T` (o valor que quem escondeu entregou), ou `None` se faltar ou for de outro tipo.
pub fn hide_data_to<T: Any + Clone>(data: Option<&HideData>) -> Option<T> {
    data.and_then(|data| data.downcast_ref::<T>()).cloned()
}

/// `HTMLElement | StandardMouseEvent | IAnchor`.
#[derive(Clone)]
pub enum AnchorTarget {
    /// Posiciona a visão no elemento. Um elemento nativo ([`ViewElement::from_native`]) é medido por quem o criou (o
    /// serviço do workbench resolve o nó no `Ui`); o `rect` do elemento só vale para os não nativos.
    Element(Rc<ViewElement>),
    /// `StandardMouseEvent`: `posx` e `posy`.
    Mouse { posx: f64, posy: f64 },
    /// Um retângulo avulso.
    Anchor(IAnchor),
}

/// `getAnchorRect(anchor)`.
pub fn get_anchor_rect(anchor: &AnchorTarget) -> IRect {
    match anchor {
        AnchorTarget::Element(element) => element.rect(),
        AnchorTarget::Anchor(anchor) => IRect {
            top: anchor.y,
            left: anchor.x,
            width: anchor.width.filter(|width| *width != 0.0).unwrap_or(1.0),
            height: anchor.height.filter(|height| *height != 0.0).unwrap_or(2.0),
        },
        // We are about to position the context view where the mouse cursor is. To prevent the view being exactly under the
        // mouse when showing and thus potentially triggering an action within, we treat the mouse location like a small sized
        // block element.
        AnchorTarget::Mouse { posx, posy } => IRect { top: *posy, left: *posx, width: 2.0, height: 2.0 },
    }
}

/// `IContextViewDelegate` (`IDelegate` do `contextview.ts`).
pub trait IContextViewDelegate {
    fn get_anchor(&self) -> AnchorTarget;
    /// `render(container)`: monta o conteúdo no contêiner e devolve o que desmonta.
    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef>;
    fn can_relayout(&self) -> bool {
        true
    }
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
    fn focus(&self) {}
    /// `onHide(data)`.
    fn on_hide(&self, _data: Option<HideData>) {}
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        None
    }
    /// `layer`: as visões de camada maior ficam por cima (`undefined` é `None`).
    fn layer(&self) -> Option<i32> {
        None
    }
}

/// `IContextViewService`.
pub trait IContextViewService {
    /// `showContextView(delegate, container, shadowRoot)`.
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<Rc<ViewElement>>, shadow_root: bool);
    /// `hideContextView(data)`.
    fn hide_context_view(&self, data: Option<HideData>);
    fn layout(&self);
    /// `getContextViewElement()`.
    fn get_context_view_element(&self) -> Rc<ViewElement>;
}

service_id!(pub I_CONTEXT_VIEW_SERVICE: dyn IContextViewService = "contextViewService");

/// `IContextMenuDelegate` (`contextmenu.ts`), no que não depende de widgets: o `getAnchor`, o `getActions` e as opções
/// escalares. `getActionViewItem`, `getCheckedActionsRepresentation` e `getActionsContext` dependem dos tipos de
/// `je-widgets` e entram com a implementação do `ContextMenuHandler`.
pub trait IContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget;
    fn get_actions(&self) -> Vec<ActionRef>;
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
    fn layer(&self) -> Option<i32> {
        None
    }
    /// A parte do delegado que depende dos widgets nativos (`getActionViewItem`, `getActionsContext`, `getKeyBinding`),
    /// opaca para a plataforma, que não depende de `je-widgets`: quem monta o menu (o `ContextMenuHandler` do workbench)
    /// a lê com `downcast_ref`. Sem valor, o menu usa os itens, o contexto e os atalhos padrão.
    fn native_extras(&self) -> Option<Rc<dyn Any>> {
        None
    }
}

/// `IContextMenuMenuDelegate`: `{ menuId?, menuActionOptions?, contextKeyService?, getActions? } & Omit<IContextMenuDelegate, 'getActions'>`.
/// O `Omit<IContextMenuDelegate, 'getActions'>` é o delegado base, cujo `get_actions` é ignorado.
/// `getActionsContext` do `IContextMenuDelegate` base é o campo [`IContextMenuMenuDelegate::get_actions_context`], porque o
/// trait base não o carrega (o `IContextMenuEvent` de `base/parts/contextmenu` vem de `je-base`).
#[derive(Clone)]
pub struct IContextMenuMenuDelegate {
    /// O `MenuId` que preenche o menu de contexto.
    pub menu_id: Option<MenuId>,
    /// `menuActionOptions`: como as ações do menu são invocadas (`arg`, `args`, `shouldForwardArgs`, `renderShortTitle`).
    pub menu_action_options: Option<MenuActionOptions>,
    /// `getActionsContext?(event?)`: o contexto passado às ações quando rodam.
    pub get_actions_context: Option<GetActionsContext>,
    /// O serviço de chaves de contexto que guia o menu.
    pub context_key_service: Option<Rc<dyn IContextKeyService>>,
    /// `getActions?()`: ações extras, colocadas antes das do menu.
    pub get_actions: Option<Rc<dyn Fn() -> Vec<ActionRef>>>,
    /// O resto do `IContextMenuDelegate`.
    pub base: Rc<dyn IContextMenuDelegate>,
}

/// `getActionsContext?(event?: IContextMenuEvent): unknown`.
pub type GetActionsContext = Rc<dyn Fn(Option<&IContextMenuEvent>) -> Option<Dynamic>>;

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
