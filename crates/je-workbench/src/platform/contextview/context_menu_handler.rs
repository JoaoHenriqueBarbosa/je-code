//! Porte de `src/vs/platform/contextview/browser/contextMenuHandler.ts`: o [`ContextMenuHandler`], que monta o
//! [`Menu`] de `je-widgets` dentro da visão de contexto (`IContextViewService`), bloqueia o ponteiro no resto da
//! interface, esconde o menu ao perder o foco ou num clique de fora, devolve o foco a quem o tinha e avisa a
//! telemetria e as notificações do que o menu roda.
//!
//! Vive no `je-workbench` (não no `je-platform`) porque monta o `Menu` do `je-widgets`, e a plataforma não depende de
//! widgets: o contrato do delegado é o [`IContextMenuDelegate`] da plataforma, e o que o original lê do delegado e
//! depende dos widgets (`getActionViewItem`, `getActionsContext`, `getKeyBinding`) viaja em
//! [`ContextMenuExtras`], pela porta `native_extras` do delegado.
//!
//! Divergências de tradução:
//! - O contêiner da visão de contexto é um [`ViewElement`] nativo ([`native_node`]): o nó do `Ui` é o que o `Menu`
//!   usa. Um elemento que não é nativo não tem nó, e a visão se monta vazia.
//! - `$('.context-view-block')` com `position: fixed` e `z-index: -1` é um nó `context-view-block` que cobre a janela
//!   (`set_bounds` em coordenadas do contêiner); a ordem de empilhamento é a de irmãos (o bloco é o primeiro
//!   filho, o menu vem depois). O `mousedown` que ele detém é o `PointerDown`.
//! - `addDisposableListener(targetWindow, BLUR)` não existe (o `je-ui` não tem evento de janela); o `MOUSE_DOWN` na
//!   janela é o `PointerDown` na raiz da `Ui`, e "o alvo está dentro do contêiner" sobe pelos pais do nó.
//! - `delegate.domForShadowRoot` e `useWindowContainerForShadowRoot` não existem no contrato da plataforma (a árvore
//!   nativa não tem raiz de sombra): o `showContextView` sai com `None` e `false`.
//! - O `IMenuStyles` (o `defaultMenuStyles` do tema) é parâmetro do construtor.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, IRunEvent};
use je_base::dynamic::Dynamic;
use je_base::errors::is_cancellation_error;
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, combined_disposable};
use je_platform::contextview::context_view::{
    AnchorTarget, HideData, IContextMenuDelegate, IContextViewCloseAnimation, IContextViewDelegate, IContextViewService, hide_data_to,
};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::telemetry::ITelemetryService;
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, PointerButton, Role, Ui};
use je_widgets::action_bar::IActionViewItemProvider;
use je_widgets::dom::{create_el, is_ancestor, window_size};
use je_widgets::menu::{GetKeyBinding, IMenuOptions, IMenuStyles, Menu};

use crate::services::contextview::native_context_view_service::native_node;

/// `IContextMenuHandlerOptions`.
#[derive(Clone, Copy, Debug)]
pub struct IContextMenuHandlerOptions {
    pub block_mouse: bool,
}

impl Default for IContextMenuHandlerOptions {
    fn default() -> Self {
        Self { block_mouse: true }
    }
}

/// O que o `IContextMenuDelegate` do original carrega e depende dos widgets: o provedor dos itens
/// (`getActionViewItem`), o contexto das ações (`getActionsContext`) e o atalho (`getKeyBinding`).
#[derive(Clone, Default)]
pub struct ContextMenuExtras {
    pub get_action_view_item: Option<IActionViewItemProvider>,
    pub get_actions_context: Option<Rc<dyn Fn() -> Option<Dynamic>>>,
    pub get_key_binding: Option<GetKeyBinding>,
}

/// O que o handler guarda entre `showContextMenu`, `render` e `onHide` (os campos privados do original).
struct HandlerState {
    ui: Ui,
    context_view_service: Rc<dyn IContextViewService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notification_service: Rc<dyn INotificationService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    menu_styles: Rc<IMenuStyles>,
    focus_to_return: Cell<Option<NodeId>>,
    last_container: Cell<Option<NodeId>>,
    block: Cell<Option<NodeId>>,
    block_disposable: RefCell<Option<DisposableRef>>,
    block_mouse: Cell<bool>,
}

impl HandlerState {
    fn hide(&self, did_cancel: bool) {
        self.context_view_service.hide_context_view(Some(Rc::new(did_cancel) as HideData));
    }

    fn on_action_run(&self, e: &IRunEvent, log_telemetry: bool) {
        if log_telemetry {
            self.telemetry_service.public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(e.action.id().as_str())), ("from", Dynamic::from("contextMenu"))])));
        }
        self.hide(false);
    }

    fn on_did_action_run(&self, e: &IRunEvent) {
        if let Some(error) = &e.error
            && !is_cancellation_error(error)
        {
            self.notification_service.error(vec![NotificationMessage::Error(error.clone())]);
        }
    }

    fn dispose_block_disposable(&self) {
        let disposable = self.block_disposable.borrow_mut().take();
        if let Some(disposable) = disposable {
            disposable.dispose();
        }
    }
}

/// O `ContextMenuHandler`.
pub struct ContextMenuHandler {
    state: Rc<HandlerState>,
}

impl ContextMenuHandler {
    pub fn new(
        ui: &Ui,
        context_view_service: Rc<dyn IContextViewService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        notification_service: Rc<dyn INotificationService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        menu_styles: Rc<IMenuStyles>,
    ) -> Self {
        Self {
            state: Rc::new(HandlerState {
                ui: ui.clone(),
                context_view_service,
                telemetry_service,
                notification_service,
                keybinding_service,
                menu_styles,
                focus_to_return: Cell::new(None),
                last_container: Cell::new(None),
                block: Cell::new(None),
                block_disposable: RefCell::new(None),
                block_mouse: Cell::new(true),
            }),
        }
    }

    /// `configure(options)`.
    pub fn configure(&self, options: IContextMenuHandlerOptions) {
        self.state.block_mouse.set(options.block_mouse);
    }

    /// `showContextMenu(delegate)`. `extras` é o que o delegado carrega e depende dos widgets
    /// ([`ContextMenuExtras`]); o padrão usa os itens, o contexto e os atalhos padrão.
    pub fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        let actions = delegate.get_actions();
        if actions.is_empty() {
            return; // Don't render an empty context menu
        }

        let extras = delegate.native_extras().and_then(|extras| extras.downcast_ref::<ContextMenuExtras>().cloned()).unwrap_or_default();
        self.state.focus_to_return.set(self.state.ui.focused());

        let view = Rc::new(MenuViewDelegate { state: self.state.clone(), delegate, extras, actions, menu: RefCell::new(None) });
        self.state.context_view_service.show_context_view(view, None, false);
    }
}

impl IDisposable for ContextMenuHandler {
    fn dispose(&self) {
        self.state.dispose_block_disposable();
    }

    fn constructor_name(&self) -> &'static str {
        "ContextMenuHandler"
    }
}

/// O objeto literal que o original passa ao `showContextView`.
struct MenuViewDelegate {
    state: Rc<HandlerState>,
    delegate: Rc<dyn IContextMenuDelegate>,
    extras: ContextMenuExtras,
    actions: Vec<ActionRef>,
    menu: RefCell<Option<Rc<Menu>>>,
}

impl MenuViewDelegate {
    /// O bloco invisível que impede a interação com o resto da interface.
    fn render_block(&self, container: NodeId) {
        let state = &self.state;
        let (width, height) = window_size(&state.ui);
        let block = {
            let mut tree = state.ui.tree_mut();
            let origin = tree.absolute_bounds(container);
            let block = create_el(&mut tree, container, Role::Generic, "context-view-block");
            // `position: fixed; left: 0; top: 0; width: 100%; height: 100%`, em coordenadas do contêiner.
            tree.set_bounds(block, Rect::new(-origin.x0, -origin.y0, width - origin.x0, height - origin.y0));
            tree.style_mut(block).cursor = Some(je_ui::Cursor::Default);
            block
        };
        state.block.set(Some(block));

        state.dispose_block_disposable();
        let listener = state.ui.listen(block, EventKind::PointerDown, false, |e| e.stop_propagation());
        *state.block_disposable.borrow_mut() = Some(listener);
    }
}

impl IContextViewDelegate for MenuViewDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.delegate.get_anchor()
    }

    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        let state = self.state.clone();
        let container_node = native_node(container)?;
        state.last_container.set(Some(container_node));
        if let Some(class_name) = self.delegate.get_menu_class_name().filter(|name| !name.is_empty()) {
            let mut tree = state.ui.tree_mut();
            for class in class_name.split_whitespace() {
                tree.add_class(container_node, class);
            }
        }

        // Render invisible div to block mouse interaction in the rest of the UI
        if state.block_mouse.get() {
            self.render_block(container_node);
        }

        let menu_disposables = DisposableStore::new();

        let action_runner: Rc<dyn IActionRunner> = match self.delegate.action_runner() {
            Some(runner) => runner,
            None => {
                let runner = menu_disposables.add_rc(ActionRunner::new());
                runner
            }
        };
        let log_telemetry = !self.delegate.skip_telemetry();
        {
            let state = state.clone();
            menu_disposables.add(action_runner.on_will_run().listen(move |e| state.on_action_run(e, log_telemetry)));
        }
        {
            let state = state.clone();
            menu_disposables.add(action_runner.on_did_run().listen(move |e| state.on_did_action_run(e)));
        }

        let get_key_binding: GetKeyBinding = self.extras.get_key_binding.clone().unwrap_or_else(|| {
            let keybinding_service = state.keybinding_service.clone();
            Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false))
        });
        let menu = Menu::new(
            &state.ui,
            container_node,
            &self.actions,
            IMenuOptions {
                action_view_item_provider: self.extras.get_action_view_item.clone(),
                context: self.extras.get_actions_context.as_ref().and_then(|get_context| get_context()),
                action_runner: Some(action_runner),
                get_key_binding: Some(get_key_binding),
                ..IMenuOptions::default()
            },
            state.menu_styles.clone(),
        );

        {
            let state = state.clone();
            menu_disposables.add(menu.on_did_cancel().listen(move |_| state.hide(true)));
        }
        {
            let state = state.clone();
            menu_disposables.add(menu.on_did_blur().listen(move |_| state.hide(true)));
        }
        {
            let state = state.clone();
            let ui = state.ui.clone();
            menu_disposables.add(ui.listen(ui.root(), EventKind::PointerDown, false, move |e| {
                if e.default_prevented() {
                    return;
                }

                // Don't do anything as we are likely creating a context menu
                if e.pointer.is_some_and(|pointer| pointer.button == PointerButton::Secondary) {
                    return;
                }

                // O alvo está dentro do contêiner do menu: não esconde.
                if is_ancestor(&state.ui, e.target, container_node) {
                    return;
                }

                state.hide(true);
            }));
        }

        *self.menu.borrow_mut() = Some(menu.clone());
        let disposables: DisposableRef = menu_disposables;
        let menu: DisposableRef = menu;
        Some(combined_disposable(vec![disposables, menu]))
    }

    fn can_relayout(&self) -> bool {
        false
    }

    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.delegate.anchor_alignment()
    }

    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.delegate.anchor_axis_alignment()
    }

    fn focus(&self) {
        let menu = self.menu.borrow().clone();
        if let Some(menu) = menu {
            menu.focus(self.delegate.auto_select_first_item());
        }
    }

    fn on_hide(&self, data: Option<HideData>) {
        let state = &self.state;
        let did_cancel = hide_data_to::<bool>(data.as_ref()).unwrap_or(false);
        self.delegate.on_hide(did_cancel);

        if let Some(block) = state.block.take() {
            state.ui.remove(block);
        }

        state.dispose_block_disposable();

        if let Some(last_container) = state.last_container.get() {
            let active = state.ui.focused();
            if active.is_some_and(|active| active == last_container || is_ancestor(&state.ui, active, last_container))
                && let Some(focus_to_return) = state.focus_to_return.get()
                && state.ui.tree().is_alive(focus_to_return)
            {
                state.ui.focus(focus_to_return);
            }
        }

        state.last_container.set(None);
    }

    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.delegate.close_animation()
    }

    fn layer(&self) -> Option<i32> {
        self.delegate.layer()
    }
}

/// O `Any` do delegado como [`ContextMenuExtras`] (para quem monta o delegado entregar o que depende dos widgets).
pub fn native_extras(extras: ContextMenuExtras) -> Option<Rc<dyn Any>> {
    Some(Rc::new(extras))
}
