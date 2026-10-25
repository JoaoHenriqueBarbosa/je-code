//! Porte de `src/vs/platform/contextview/browser/contextMenuService.ts`: o [`ContextMenuService`] (a implementação de
//! `IContextMenuService` que monta o menu pelo [`ContextMenuHandler`]) e o [`ContextMenuMenuDelegate`] que transforma um
//! delegado "de menu" (`menuId`) no delegado com as ações do menu.
//!
//! Vive no `je-workbench` pelo mesmo motivo do handler (monta o `Menu` do `je-widgets`).
//!
//! Divergências de tradução:
//! - `{ ...delegate, onHide }` (copiar o delegado trocando só o `onHide`) é [`HideNotifyingDelegate`], que repassa
//!   tudo ao delegado de dentro e acrescenta o que o `onHide` do serviço faz (`resetKeyStatus` do
//!   `ModifierKeyEmitter` da `Ui` e o `onDidHideContextMenu`). O `{ ...delegate, getActions }` de `transform` é
//!   [`TransformedDelegate`], pelo mesmo caminho.
//! - `ModifierKeyEmitter.getInstance()` é por [`Ui`] (ver `je-widgets`), por isso o serviço recebe a `Ui`.
//! - O handler nasce no primeiro `showContextMenu` ou `configure` (o getter preguiçoso do original).
//! - `ContextMenuMenuDelegate.is` (o `menuId instanceof MenuId`) é a variante [`ContextMenuRequest::Menu`] com
//!   `menu_id`; um delegado "de menu" sem `menu_id` entrega só o `getActions` dele.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::{ActionRef, IActionRunner, Separator};
use je_base::event::{Emitter, Event};
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment};
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::actions::menu_entry_action_view_item::get_flat_context_menu_actions;
use je_platform::actions::{IMenuService, MenuId};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{
    AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuMenuDelegate, IContextMenuService, IContextViewCloseAnimation, IContextViewService,
};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::INotificationService;
use je_platform::telemetry::ITelemetryService;
use je_ui::Ui;
use je_widgets::menu::IMenuStyles;
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;

use super::context_menu_handler::{ContextMenuExtras, ContextMenuHandler, IContextMenuHandlerOptions, native_extras};

/// O que `ContextMenuService` precisa para criar o handler quando o primeiro menu abre.
struct HandlerDeps {
    context_view_service: Rc<dyn IContextViewService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notification_service: Rc<dyn INotificationService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    menu_styles: Rc<IMenuStyles>,
}

/// `ContextMenuService`.
pub struct ContextMenuService {
    base: Rc<Disposable>,
    ui: Ui,
    deps: HandlerDeps,
    menu_service: Rc<dyn IMenuService>,
    context_key_service: Rc<dyn IContextKeyService>,
    context_menu_handler: RefCell<Option<Rc<ContextMenuHandler>>>,
    on_did_show_context_menu: Rc<Emitter<()>>,
    on_did_hide_context_menu: Rc<Emitter<()>>,
}

impl ContextMenuService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        telemetry_service: Rc<dyn ITelemetryService>,
        notification_service: Rc<dyn INotificationService>,
        context_view_service: Rc<dyn IContextViewService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
        menu_styles: Rc<IMenuStyles>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_show_context_menu = Emitter::new();
        base.register(on_did_show_context_menu.clone());
        let on_did_hide_context_menu = Emitter::new();
        base.register(on_did_hide_context_menu.clone());
        Rc::new(Self {
            base,
            ui: ui.clone(),
            deps: HandlerDeps { context_view_service, telemetry_service, notification_service, keybinding_service, menu_styles },
            menu_service,
            context_key_service,
            context_menu_handler: RefCell::new(None),
            on_did_show_context_menu,
            on_did_hide_context_menu,
        })
    }

    /// O `get contextMenuHandler()`: cria o handler na primeira vez.
    fn context_menu_handler(&self) -> Rc<ContextMenuHandler> {
        let existing = self.context_menu_handler.borrow().clone();
        if let Some(handler) = existing {
            return handler;
        }
        let deps = &self.deps;
        let handler = Rc::new(ContextMenuHandler::new(
            &self.ui,
            deps.context_view_service.clone(),
            deps.telemetry_service.clone(),
            deps.notification_service.clone(),
            deps.keybinding_service.clone(),
            deps.menu_styles.clone(),
        ));
        *self.context_menu_handler.borrow_mut() = Some(handler.clone());
        handler
    }

    /// `configure(options)`.
    pub fn configure(&self, options: IContextMenuHandlerOptions) {
        self.context_menu_handler().configure(options);
    }
}

impl IContextMenuService for ContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        self.on_did_show_context_menu.event()
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        self.on_did_hide_context_menu.event()
    }

    /// `showContextMenu(delegate)`.
    fn show_context_menu(&self, request: ContextMenuRequest) {
        let delegate = ContextMenuMenuDelegate::transform(request, self.menu_service.clone(), self.context_key_service.clone());

        let handler = self.context_menu_handler();
        handler.show_context_menu(Rc::new(HideNotifyingDelegate { inner: delegate, ui: self.ui.clone(), on_did_hide_context_menu: self.on_did_hide_context_menu.clone() }));
        self.on_did_show_context_menu.fire(());
    }
}

impl IDisposable for ContextMenuService {
    fn dispose(&self) {
        let handler = self.context_menu_handler.borrow_mut().take();
        if let Some(handler) = handler {
            handler.dispose();
        }
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ContextMenuService"
    }
}

/// `{ ...delegate, onHide: (didCancel) => { delegate.onHide?.(didCancel); ModifierKeyEmitter.getInstance().resetKeyStatus(); this._onDidHideContextMenu.fire(); } }`.
struct HideNotifyingDelegate {
    inner: Rc<dyn IContextMenuDelegate>,
    ui: Ui,
    on_did_hide_context_menu: Rc<Emitter<()>>,
}

impl IContextMenuDelegate for HideNotifyingDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.inner.get_anchor()
    }
    fn get_actions(&self) -> Vec<ActionRef> {
        self.inner.get_actions()
    }
    fn get_menu_class_name(&self) -> Option<String> {
        self.inner.get_menu_class_name()
    }
    fn on_hide(&self, did_cancel: bool) {
        self.inner.on_hide(did_cancel);

        ModifierKeyEmitter::get_instance(&self.ui).reset_key_status();
        self.on_did_hide_context_menu.fire(());
    }
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.inner.close_animation()
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
    fn layer(&self) -> Option<i32> {
        self.inner.layer()
    }
    fn native_extras(&self) -> Option<Rc<dyn Any>> {
        self.inner.native_extras()
    }
}

/// `ContextMenuMenuDelegate` (o `namespace` do original).
pub struct ContextMenuMenuDelegate;

impl ContextMenuMenuDelegate {
    /// `transform(delegate, menuService, globalContextKeyService)`: um delegado comum passa como está, um delegado
    /// "de menu" ganha as ações do menu depois das que ele mesmo dá.
    pub fn transform(request: ContextMenuRequest, menu_service: Rc<dyn IMenuService>, global_context_key_service: Rc<dyn IContextKeyService>) -> Rc<dyn IContextMenuDelegate> {
        match request {
            ContextMenuRequest::Delegate(delegate) => delegate,
            ContextMenuRequest::Menu(menu) => Rc::new(TransformedDelegate { menu, menu_service, global_context_key_service }),
        }
    }
}

/// `{ ...delegate, getActions }` de um delegado "de menu".
struct TransformedDelegate {
    menu: IContextMenuMenuDelegate,
    menu_service: Rc<dyn IMenuService>,
    global_context_key_service: Rc<dyn IContextKeyService>,
}

impl TransformedDelegate {
    fn menu_actions(&self, menu_id: &MenuId) -> Vec<ActionRef> {
        let context_key_service = self.menu.context_key_service.clone().unwrap_or_else(|| self.global_context_key_service.clone());
        let menu = self.menu_service.get_menu_actions(menu_id, &*context_key_service, self.menu.menu_action_options.as_ref());
        get_flat_context_menu_actions(&menu, None)
    }
}

impl IContextMenuDelegate for TransformedDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.menu.base.get_anchor()
    }
    fn get_actions(&self) -> Vec<ActionRef> {
        let target = self.menu.menu_id.as_ref().map(|menu_id| self.menu_actions(menu_id)).unwrap_or_default();
        match &self.menu.get_actions {
            None => target,
            Some(get_actions) => Separator::join(&[get_actions(), target]),
        }
    }
    fn get_menu_class_name(&self) -> Option<String> {
        self.menu.base.get_menu_class_name()
    }
    fn on_hide(&self, did_cancel: bool) {
        self.menu.base.on_hide(did_cancel);
    }
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        self.menu.base.close_animation()
    }
    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.menu.base.action_runner()
    }
    fn skip_telemetry(&self) -> bool {
        self.menu.base.skip_telemetry()
    }
    fn auto_select_first_item(&self) -> bool {
        self.menu.base.auto_select_first_item()
    }
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.menu.base.anchor_alignment()
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.menu.base.anchor_axis_alignment()
    }
    fn layer(&self) -> Option<i32> {
        self.menu.base.layer()
    }
    /// O `getActionsContext` do delegado "de menu" some no `Menu` junto com o que o `base` traz.
    fn native_extras(&self) -> Option<Rc<dyn Any>> {
        let base = self.menu.base.native_extras().and_then(|extras| extras.downcast_ref::<ContextMenuExtras>().cloned()).unwrap_or_default();
        let get_actions_context = self.menu.get_actions_context.clone().map(|get| -> Rc<dyn Fn() -> Option<je_base::dynamic::Dynamic>> { Rc::new(move || get(None)) }).or(base.get_actions_context.clone());
        native_extras(ContextMenuExtras { get_actions_context, ..base })
    }
}
