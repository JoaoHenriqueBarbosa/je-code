//! Porte de `src/vs/workbench/browser/parts/globalCompositeBar.ts`: a [`GlobalCompositeBar`] (a barra vertical com as ações
//! "Accounts" e "Manage" da barra de atividades), os itens [`GlobalActivityActionViewItem`] e
//! [`AccountsActivityActionViewItem`], as variantes simples e as funções de visibilidade das contas.
//!
//! Divergências:
//! - **`createInstance` e os decoradores `@I...Service`.** O que o `instantiationService` injeta chega em
//!   [`GlobalCompositeBarServices`]; a barra cria os itens com ele.
//! - **Contratos reduzidos.** [`IAccountsAuthentication`] e [`IAccountsDefaultAccount`] são o recorte de
//!   `IAuthenticationService` e `IDefaultAccountService` que o item de contas chama (o que o `Reflect.get(...prototype)` do teste
//!   original já faz ao trocar o `this` por um objeto com meia dúzia de campos); os adaptadores [`AuthenticationAdapter`] e
//!   [`DefaultAccountAdapter`] ligam os serviços reais.
//! - **`private` testável.** `toggleAccountsActivity`, `updateAvatar`, `addOrUpdateAccount`, `resolveMainMenuActions` e
//!   `getDefaultAccountAvatarIcon` do original são `pub` aqui (a suíte os chama pelo protótipo).
//! - **O avatar.** A árvore nativa não tem imagem: o `<img class="accounts-avatar">` é o [`AvatarImage`] (o `src` e o `display`),
//!   e a classe `has-avatar` do rótulo é a do nó. Quem pinta a imagem lê o [`AvatarImage`].
//! - **`resolveMainMenuActions`.** O `await super.resolveMainMenuActions(...)` do original tem o resultado descartado e não tem
//!   efeito colateral (só avalia o menu); não é chamado.
//! - **`IWorkbenchEnvironmentService`.** O original o recebe e não o lê; não entra.
//! - **O toque (`Tap`).** O `Gesture` do toque não existe na árvore; o clique e o teclado fazem o mesmo.
//! - **`FileAccess.uriToBrowserUri`** (global no original) é o `uri_to_browser_uri` de [`GlobalCompositeBarServices`].
//! - **`runWhenWindowIdle(getWindow(this.element), ...)`** é o `run_when_global_idle`, e o `Promise.allSettled` é o
//!   `Promise::all_settled`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IAction, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::async_::{async_fn, run_when_global_idle};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::actions::actions::{IMenu, IMenuService, MenuAction, MenuActionGroup, MenuActionOptions, MenuId};
use je_platform::actions::menu_ids::{accounts_context, global_activity};
use je_platform::actions::menu_entry_action_view_item::{get_context_menu_actions, menu_action_as_action};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService, IContextViewCloseAnimation};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::log::{LogMessage, LogServiceRef, Logger};
use je_platform::product::IProductService;
use je_platform::secrets::secrets::ISecretStorageService;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_ui::{Event, EventKind, NodeId, PointerButton, Role, Ui};
use je_widgets::action_bar::{ActionBar, ActionsOrientation, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use je_widgets::action_view_items::{BaseActionViewItem, IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::create_detached_el;
use je_widgets::hover_api::IHoverDelegate2;
use je_widgets::text::{set_node_text, ui_font};

use crate::browser::actions::menu_motion::{WORKBENCH_MENU_MOTION_CLASS, workbench_menu_close_animation};
use crate::browser::parts::composite_bar_actions::{
    CompositeBarAction, CompositeBarItemCore, CompositeBarItemHooks, ICompositeBarActionItem, ICompositeBarActionViewItemOptions,
};
use crate::browser::parts::pane_composite_bar::{ActivityHoverOptions, CompositeBarColors, ContentElement, IGlobalCompositeBar, global_composite_bar_height};
use crate::common::activity::{ACCOUNTS_ACTIVITY_ID, ACCOUNTS_SHARED_SIGN_IN_GROUP, GLOBAL_ACTIVITY_ID};
use crate::services::activity::activity::{ActivityChangeTarget, IActivityService};
use crate::services::authentication::authentication::{
    ACCOUNTS_AVATAR_SETTING, AuthenticationProviderInformation, AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChange,
    IAuthenticationProvider, IAuthenticationServiceProviders, INTERNAL_AUTH_PROVIDER_PREFIX,
};
use crate::services::authentication::authentication_service::{AuthenticationSessionInfo, get_current_authentication_session_info};
use crate::services::accounts::default_account::IDefaultAccountService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};
use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

/// `ACTIVITY_BAR_BADGE_BACKGROUND` e `ACTIVITY_BAR_BADGE_FOREGROUND` (os identificadores das cores registradas).
const ACTIVITY_BAR_BADGE_BACKGROUND: &str = "activityBarBadge.background";
const ACTIVITY_BAR_BADGE_FOREGROUND: &str = "activityBarBadge.foreground";

/// `ACCOUNTS_ACTION_INDEX`.
const ACCOUNTS_ACTION_INDEX: i64 = 0;

/// `GlobalCompositeBar.ACCOUNTS_ICON`: o ícone que `registerIcon('accounts-view-bar-icon', Codicon.account, ...)` devolve.
pub fn accounts_icon() -> ThemeIcon {
    ThemeIcon::from_id("accounts-view-bar-icon")
}

/// O `DEFAULT_ICON` de `userDataProfileIcons.ts` (`registerIcon('settings-view-bar-icon', Codicon.settingsGear, ...)`).
pub fn default_profile_icon() -> ThemeIcon {
    ThemeIcon::from_id("settings-view-bar-icon")
}

/// `ACCOUNTS_VISIBILITY_PREFERENCE_KEY`.
pub const ACCOUNTS_VISIBILITY_PREFERENCE_KEY: &str = "workbench.activity.showAccounts";

/// `isAccountsActionVisible(storageService)`.
pub fn is_accounts_action_visible(storage_service: &dyn StorageService) -> bool {
    storage_service.get_boolean(ACCOUNTS_VISIBILITY_PREFERENCE_KEY, StorageScope::Profile, Some(true)).unwrap_or(true)
}

/// `setAccountsActionVisible(storageService, visible)`.
fn set_accounts_action_visible(storage_service: &dyn StorageService, visible: bool) {
    storage_service.store(ACCOUNTS_VISIBILITY_PREFERENCE_KEY, StorageValue::Bool(visible), StorageScope::Profile, StorageTarget::User);
}

/// O recorte de `IAuthenticationService` que o item de contas chama.
pub trait IAccountsAuthentication {
    fn on_did_register_authentication_provider(&self) -> BaseEvent<AuthenticationProviderInformation>;
    fn on_did_unregister_authentication_provider(&self) -> BaseEvent<AuthenticationProviderInformation>;
    fn on_did_change_sessions(&self) -> BaseEvent<AuthenticationSessionsChange>;
    fn get_provider_ids(&self) -> Vec<String>;
    fn is_dynamic_authentication_provider(&self, id: &str) -> bool;
    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider>;
    /// `getSessions(providerId)`.
    fn get_sessions(&self, id: &str) -> Promise<Vec<AuthenticationSession>>;
}

/// Liga o `IAuthenticationService` ao [`IAccountsAuthentication`].
pub struct AuthenticationAdapter(pub Rc<dyn IAuthenticationServiceProviders>);

impl IAccountsAuthentication for AuthenticationAdapter {
    fn on_did_register_authentication_provider(&self) -> BaseEvent<AuthenticationProviderInformation> {
        self.0.on_did_register_authentication_provider()
    }
    fn on_did_unregister_authentication_provider(&self) -> BaseEvent<AuthenticationProviderInformation> {
        self.0.on_did_unregister_authentication_provider()
    }
    fn on_did_change_sessions(&self) -> BaseEvent<AuthenticationSessionsChange> {
        self.0.on_did_change_sessions()
    }
    fn get_provider_ids(&self) -> Vec<String> {
        self.0.get_provider_ids()
    }
    fn is_dynamic_authentication_provider(&self, id: &str) -> bool {
        self.0.is_dynamic_authentication_provider(id)
    }
    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider> {
        self.0.get_provider(id)
    }
    fn get_sessions(&self, id: &str) -> Promise<Vec<AuthenticationSession>> {
        self.0.get_sessions(id, None, None, None)
    }
}

/// O recorte de `IDefaultAccountService` que o item de contas chama.
pub trait IAccountsDefaultAccount {
    fn on_did_change_default_account(&self) -> BaseEvent<Option<je_base::default_account::IDefaultAccount>>;
    fn current_default_account(&self) -> Option<je_base::default_account::IDefaultAccount>;
    fn get_default_account(&self) -> Promise<Option<je_base::default_account::IDefaultAccount>>;
}

/// Liga o `IDefaultAccountService` ao [`IAccountsDefaultAccount`].
pub struct DefaultAccountAdapter(pub Rc<dyn IDefaultAccountService>);

impl IAccountsDefaultAccount for DefaultAccountAdapter {
    fn on_did_change_default_account(&self) -> BaseEvent<Option<je_base::default_account::IDefaultAccount>> {
        self.0.on_did_change_default_account()
    }
    fn current_default_account(&self) -> Option<je_base::default_account::IDefaultAccount> {
        self.0.current_default_account()
    }
    fn get_default_account(&self) -> Promise<Option<je_base::default_account::IDefaultAccount>> {
        self.0.get_default_account()
    }
}

/// O que o `instantiationService` injeta nos itens globais.
#[derive(Clone)]
pub struct GlobalCompositeBarServices {
    pub theme_service: Rc<dyn IThemeService>,
    pub hover_service: Rc<dyn IHoverDelegate2>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub activity_service: Rc<dyn IActivityService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub storage_service: Rc<dyn StorageService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub user_data_profile_service: Rc<dyn IUserDataProfileService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub authentication_service: Rc<dyn IAccountsAuthentication>,
    pub product_service: Rc<dyn IProductService>,
    pub secret_storage_service: Rc<dyn ISecretStorageService>,
    pub log_service: LogServiceRef,
    pub command_service: Rc<dyn ICommandService>,
    pub default_account_service: Rc<dyn IAccountsDefaultAccount>,
    /// `FileAccess.uriToBrowserUri(uri)`, que o original chama no global.
    pub uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
}

/// `contextMenuAlignmentOptions`: `() => ({ anchorAlignment, anchorAxisAlignment })`.
pub type ContextMenuAlignmentOptions = Rc<dyn Fn() -> Option<(AnchorAlignment, AnchorAxisAlignment)>>;

/// O `fillContextMenuActions` do item de contas.
pub type FillContextMenuActions = Rc<dyn Fn(&mut Vec<ActionRef>)>;

/// `GlobalCompositeBar`.
pub struct GlobalCompositeBar {
    store: Rc<DisposableStore>,
    /// `readonly element: HTMLElement`.
    element: NodeId,
    ui: Ui,
    storage_service: Rc<dyn StorageService>,
    global_activity_action: Rc<Action>,
    account_action: Rc<Action>,
    global_activity_action_bar: Rc<ActionBar>,
    on_did_change: Rc<Emitter<()>>,
}

impl GlobalCompositeBar {
    /// `new GlobalCompositeBar(contextMenuActionsProvider, colors, activityHoverOptions, ...services)`.
    pub fn new(
        ui: &Ui,
        context_menu_actions_provider: Rc<dyn Fn() -> Vec<ActionRef>>,
        colors: Rc<dyn Fn(&dyn IColorTheme) -> CompositeBarColors>,
        activity_hover_options: ActivityHoverOptions,
        services: GlobalCompositeBarServices,
    ) -> Rc<Self> {
        let element = create_detached_el(&mut ui.tree_mut(), Role::Generic, "");
        let configuration_service = services.configuration_service.clone();
        let context_menu_alignment_options: ContextMenuAlignmentOptions = Rc::new(move || {
            let left = matches!(configuration_service.get_value(Some("workbench.sideBar.location"), None), Dynamic::String(ref location) if location.to_string_lossy() == "left");
            Some((if left { AnchorAlignment::Right } else { AnchorAlignment::Left }, AnchorAxisAlignment::Horizontal))
        });
        let storage_service = services.storage_service.clone();
        let ui_for_items = ui.clone();
        let (item_services, item_storage) = (services.clone(), storage_service.clone());
        let action_view_item_provider: IActionViewItemProvider = Rc::new(move |action, options| {
            let item_options = ICompositeBarActionViewItemOptions {
                base: IActionViewItemOptions { draggable: false, ..options.clone() },
                icon: true,
                colors: colors.clone(),
                hover_options: activity_hover_options.clone(),
                has_popup: true,
                compact: false,
            };
            if action.id() == GLOBAL_ACTIVITY_ID {
                let item: Rc<dyn IActionViewItem> =
                    GlobalActivityActionViewItem::new(&ui_for_items, context_menu_actions_provider.clone(), item_options, context_menu_alignment_options.clone(), item_services.clone());
                return Some(item);
            }
            if action.id() == ACCOUNTS_ACTIVITY_ID {
                let storage_service = item_storage.clone();
                let fill: FillContextMenuActions = Rc::new(move |actions| {
                    let storage_service = storage_service.clone();
                    let hide_accounts = to_action(ToActionProps {
                        id: "hideAccounts".to_owned(),
                        label: localize("hideAccounts", "Hide Accounts", &[]),
                        tooltip: None,
                        enabled: None,
                        checked: None,
                        class: None,
                        alt: None,
                        run: Rc::new(move |_| {
                            set_accounts_action_visible(&*storage_service, false);
                            MaybePromise::Value(Dynamic::Undefined)
                        }),
                    });
                    let separator: ActionRef = Separator::new();
                    actions.splice(0..0, [hide_accounts, separator]);
                });
                let item: Rc<dyn IActionViewItem> =
                    AccountsActivityActionViewItem::new(&ui_for_items, context_menu_actions_provider.clone(), item_options, context_menu_alignment_options.clone(), fill, item_services.clone());
                return Some(item);
            }
            // `throw new Error(`No view item for action '${action.id}'`)`.
            je_base::errors::throw(je_base::errors::Error::new(format!("No view item for action '{}'", action.id())))
        });
        let action_bar = ActionBar::new(
            ui,
            element,
            IActionBarOptions {
                action_view_item_provider: Some(action_view_item_provider),
                orientation: Some(ActionsOrientation::Vertical),
                aria_label: Some(localize("manage", "Manage", &[])),
                prevent_loop_navigation: true,
                ..IActionBarOptions::default()
            },
        );
        let this = Self::from_action_bar(ui, element, action_bar, storage_service);
        this.register_listeners(&services);
        this
    }

    /// O resto do construtor: as duas ações, a visibilidade das contas e a ação global no fim da barra. Compartilhado com os
    /// testes, que montam a barra de ações própria.
    pub fn from_action_bar(ui: &Ui, element: NodeId, action_bar: Rc<ActionBar>, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let store = DisposableStore::new();
        let global_activity_action = Action::new(GLOBAL_ACTIVITY_ID, "", "", true, None);
        store.add(global_activity_action.clone());
        let account_action = Action::new(ACCOUNTS_ACTIVITY_ID, "", "", true, None);
        store.add(account_action.clone());
        store.add(action_bar.clone());
        let on_did_change = Emitter::new();
        store.add(on_did_change.clone());
        let this = Rc::new(Self { store, element, ui: ui.clone(), storage_service, global_activity_action, account_action, global_activity_action_bar: action_bar, on_did_change });
        if this.accounts_visibility_preference() {
            this.global_activity_action_bar.push_action(this.account_action.clone(), IActionOptions { index: Some(ACCOUNTS_ACTION_INDEX), ..IActionOptions::default() });
        }
        this.global_activity_action_bar.push_action(this.global_activity_action.clone(), IActionOptions::default());
        this
    }

    /// `registerListeners()`.
    fn register_listeners(self: &Rc<Self>, services: &GlobalCompositeBarServices) {
        let weak = Rc::downgrade(self);
        let storage_service = services.storage_service.clone();
        let _ = services.extension_service.when_installed_extensions_registered().then(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if !this.store.is_disposed() {
                let listener_weak = Rc::downgrade(&this);
                let listener = storage_service.on_did_change_value(StorageScope::Profile, Some(ACCOUNTS_VISIBILITY_PREFERENCE_KEY), &this.store).listen(move |_| {
                    if let Some(this) = listener_weak.upgrade() {
                        this.toggle_accounts_activity();
                    }
                });
                this.store.add(listener);
            }
        });
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> BaseEvent<()> {
        self.on_did_change.event()
    }

    /// `element`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// `create(parent)`: `parent.appendChild(this.element)`.
    pub fn create_in(&self, parent: NodeId) {
        self.ui.tree_mut().append_child(parent, self.element);
    }

    /// `focus()`.
    pub fn focus(&self) {
        self.global_activity_action_bar.focus(true);
    }

    /// `getHeight(actionHeight, actionGap)`.
    pub fn get_height(&self, action_height: f64, action_gap: f64) -> f64 {
        global_composite_bar_height(self.global_activity_action_bar.length(), action_height, action_gap)
    }

    /// `getContextMenuActions()`.
    pub fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        let storage_service = self.storage_service.clone();
        let current = self.accounts_visibility_preference();
        let toggle = to_action(ToActionProps {
            id: "toggleAccountsVisibility".to_owned(),
            label: localize("accounts", "Accounts", &[]),
            tooltip: None,
            enabled: None,
            checked: Some(current),
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                set_accounts_action_visible(&*storage_service, !is_accounts_action_visible(&*storage_service));
                MaybePromise::Value(Dynamic::Undefined)
            }),
        });
        vec![toggle]
    }

    /// `toggleAccountsActivity()`.
    pub fn toggle_accounts_activity(&self) {
        let accounts_visible = self.global_activity_action_bar.length() == 2;
        if accounts_visible == self.accounts_visibility_preference() {
            return;
        }
        if accounts_visible {
            self.global_activity_action_bar.pull(ACCOUNTS_ACTION_INDEX as usize);
        } else {
            self.global_activity_action_bar.push_action(self.account_action.clone(), IActionOptions { index: Some(ACCOUNTS_ACTION_INDEX), ..IActionOptions::default() });
        }
        self.on_did_change.fire(());
    }

    /// `get accountsVisibilityPreference()`.
    pub fn accounts_visibility_preference(&self) -> bool {
        is_accounts_action_visible(&*self.storage_service)
    }

    /// `set accountsVisibilityPreference(value)`.
    pub fn set_accounts_visibility_preference(&self, value: bool) {
        set_accounts_action_visible(&*self.storage_service, value);
    }

    /// `globalActivityActionBar`.
    pub fn global_activity_action_bar(&self) -> &Rc<ActionBar> {
        &self.global_activity_action_bar
    }

    /// `accountAction`.
    pub fn account_action(&self) -> &Rc<Action> {
        &self.account_action
    }
}

impl IDisposable for GlobalCompositeBar {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl IGlobalCompositeBar for GlobalCompositeBar {
    fn get_height(&self, action_height: f64, action_gap: f64) -> f64 {
        GlobalCompositeBar::get_height(self, action_height, action_gap)
    }

    fn client_width(&self) -> f64 {
        self.ui.tree().bounds(self.element).width()
    }

    fn create(&self, parent: &Rc<ContentElement>) {
        if let Some(node) = parent.node() {
            self.create_in(node);
        }
    }

    fn focus(&self) {
        GlobalCompositeBar::focus(self);
    }

    fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        GlobalCompositeBar::get_context_menu_actions(self)
    }

    fn on_did_change(&self) -> BaseEvent<()> {
        GlobalCompositeBar::on_did_change(self)
    }
}

/// Os pontos que o item de contas sobrescreve de `AbstractGlobalActivityActionViewItem`.
trait GlobalActivityMenuHooks {
    /// `resolveMainMenuActions(menu, disposables)`.
    fn resolve_main_menu_actions(&self, menu: Rc<dyn IMenu>, disposables: Rc<DisposableStore>) -> Promise<Vec<ActionRef>>;

    /// `resolveContextMenuActions(disposables)`.
    fn resolve_context_menu_actions(&self, disposables: Rc<DisposableStore>) -> Promise<Vec<ActionRef>>;
}

/// O delegado do menu que o item abre (`showContextMenu({ getAnchor, getActions, getMenuClassName, onHide, ... })`).
struct GlobalMenuDelegate {
    anchor: AnchorTarget,
    actions: Vec<ActionRef>,
    alignment: Option<(AnchorAlignment, AnchorAxisAlignment)>,
    disposables: Rc<DisposableStore>,
}

impl IContextMenuDelegate for GlobalMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }

    fn get_menu_class_name(&self) -> Option<String> {
        Some(WORKBENCH_MENU_MOTION_CLASS.to_owned())
    }

    fn on_hide(&self, _did_cancel: bool) {
        self.disposables.dispose();
    }

    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        Some(workbench_menu_close_animation())
    }

    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        self.alignment.map(|(alignment, _)| alignment)
    }

    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        self.alignment.map(|(_, axis)| axis)
    }
}

/// `AbstractGlobalActivityActionViewItem`: o estado e os ouvintes que os dois itens globais compartilham.
struct GlobalActivityCore {
    composite: Rc<CompositeBarItemCore>,
    menu_id: MenuId,
    context_menu_actions_provider: Rc<dyn Fn() -> Vec<ActionRef>>,
    context_menu_alignment_options: ContextMenuAlignmentOptions,
    services: GlobalCompositeBarServices,
    menu_hooks: RefCell<Option<Weak<dyn GlobalActivityMenuHooks>>>,
}

impl GlobalActivityCore {
    #[allow(clippy::too_many_arguments)]
    fn new(
        composite: Rc<CompositeBarItemCore>,
        menu_id: MenuId,
        context_menu_actions_provider: Rc<dyn Fn() -> Vec<ActionRef>>,
        context_menu_alignment_options: ContextMenuAlignmentOptions,
        services: GlobalCompositeBarServices,
    ) -> Rc<Self> {
        let this = Rc::new(Self { composite, menu_id, context_menu_actions_provider, context_menu_alignment_options, services, menu_hooks: RefCell::new(None) });
        this.update_item_activity();
        let weak = Rc::downgrade(&this);
        let listener = this.services.activity_service.on_did_change_activity().listen(move |target| {
            let Some(this) = weak.upgrade() else { return };
            if let ActivityChangeTarget::Id(id) = target {
                if *id == this.composite.composite_bar_action_item().id {
                    this.update_item_activity();
                }
            }
        });
        this.composite.base().register(listener);
        this
    }

    /// `updateItemActivity()`.
    fn update_item_activity(&self) {
        self.composite.composite_action().set_activities(self.services.activity_service.get_activity(&self.composite.composite_bar_action_item().id));
    }

    fn set_menu_hooks(&self, hooks: Weak<dyn GlobalActivityMenuHooks>) {
        *self.menu_hooks.borrow_mut() = Some(hooks);
    }

    fn hooks(&self) -> Option<Rc<dyn GlobalActivityMenuHooks>> {
        self.menu_hooks.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// `render(container)` do item abstrato: o clique, o menu de contexto e o teclado.
    fn render(self: &Rc<Self>, container: NodeId) {
        self.composite.render(container);
        let ui = self.composite.base().ui().clone();
        let base = self.composite.base();

        let weak = Rc::downgrade(self);
        base.register(ui.listen(container, EventKind::PointerDown, false, move |event| {
            event.stop(true);
            let is_left_click = event.pointer.is_none_or(|pointer| pointer.button != PointerButton::Secondary);
            // Left-click run
            if is_left_click {
                if let Some(this) = weak.upgrade() {
                    this.run();
                }
            }
        }));

        // The rest of the activity bar uses context menu event for the context menu, so we match this.
        let weak = Rc::downgrade(self);
        base.register(ui.listen(container, EventKind::ContextMenu, false, move |event: &mut Event| {
            // Let the item decide on the context menu instead of the toolbar.
            event.stop(false);
            let Some(this) = weak.upgrade() else { return };
            let anchor = match event.pointer {
                Some(pointer) => AnchorTarget::Mouse { posx: pointer.x, posy: pointer.y },
                None => this.label_anchor(),
            };
            let disposables = DisposableStore::new();
            let resolved = match this.hooks() {
                Some(hooks) => hooks.resolve_context_menu_actions(disposables.clone()),
                None => Promise::resolved(this.default_context_menu_actions()),
            };
            let services = this.services.clone();
            let _ = resolved.then(move |actions| {
                services.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(GlobalMenuDelegate { anchor, actions, alignment: None, disposables })));
            });
        }));

        let weak = Rc::downgrade(self);
        base.register(ui.listen(container, EventKind::KeyUp, false, move |event| {
            let is_trigger = event.key.is_some_and(|key| key.key_code == KeyCode::Enter || key.key_code == KeyCode::Space);
            if is_trigger {
                event.stop(true);
                if let Some(this) = weak.upgrade() {
                    this.run();
                }
            }
        }));
    }

    /// `getAnchor: () => this.label`.
    fn label_anchor(&self) -> AnchorTarget {
        let node = self.composite.label().or(self.composite.container()).unwrap_or_else(|| self.composite.base().ui().root());
        AnchorTarget::Element(ViewElement::from_native(Rc::new(node)))
    }

    /// `resolveContextMenuActions(disposables)` do item abstrato.
    fn default_context_menu_actions(&self) -> Vec<ActionRef> {
        (self.context_menu_actions_provider)()
    }

    /// `resolveMainMenuActions(menu, disposables)` do item abstrato: `getActionBarActions(menu.getActions({ renderShortTitle: true })).secondary`.
    fn default_main_menu_actions(menu: &dyn IMenu) -> Vec<ActionRef> {
        let options = MenuActionOptions { render_short_title: true, ..MenuActionOptions::default() };
        get_context_menu_actions(&menu.get_actions(Some(&options)), None).secondary
    }

    /// `run()`.
    fn run(self: &Rc<Self>) {
        let this = self.clone();
        let _ = async_fn(async move {
            let disposables = DisposableStore::new();
            let menu = this.services.menu_service.create_menu(&this.menu_id, this.services.context_key_service.clone(), None);
            let menu_disposable: DisposableRef = menu.clone();
            disposables.add(menu_disposable);
            let actions = match this.hooks() {
                Some(hooks) => hooks.resolve_main_menu_actions(menu, disposables.clone()).await?,
                None => Self::default_main_menu_actions(&*menu),
            };
            let alignment = (this.context_menu_alignment_options)();
            this.services.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(GlobalMenuDelegate { anchor: this.label_anchor(), actions, alignment, disposables })));
            Ok(())
        });
    }
}

/// `GlobalActivityActionViewItem`.
pub struct GlobalActivityActionViewItem {
    core: Rc<GlobalActivityCore>,
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
    theme_service: Rc<dyn IThemeService>,
    profile_badge: Cell<Option<NodeId>>,
    profile_badge_content: Cell<Option<NodeId>>,
    action: Rc<CompositeBarAction>,
}

impl GlobalActivityActionViewItem {
    pub fn new(
        ui: &Ui,
        context_menu_actions_provider: Rc<dyn Fn() -> Vec<ActionRef>>,
        options: ICompositeBarActionViewItemOptions,
        context_menu_alignment_options: ContextMenuAlignmentOptions,
        services: GlobalCompositeBarServices,
    ) -> Rc<Self> {
        let profile_icon = |services: &GlobalCompositeBarServices| {
            services.user_data_profile_service.current_profile().icon.map(ThemeIcon::from_id).unwrap_or_else(default_profile_icon)
        };
        let action = CompositeBarAction::new(ICompositeBarActionItem {
            id: GLOBAL_ACTIVITY_ID.to_owned(),
            name: localize("manage", "Manage", &[]),
            keybinding_id: None,
            class_names: Some(profile_icon(&services).as_class_name_array()),
            icon_url: None,
        });
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let composite = CompositeBarItemCore::new(
                ui,
                weak.clone() as Weak<dyn IActionViewItem>,
                action.clone(),
                options,
                Rc::new(|_| true),
                services.theme_service.clone(),
                services.hover_service.clone(),
                services.keybinding_service.clone(),
            );
            Self {
                core: GlobalActivityCore::new(composite, global_activity(), context_menu_actions_provider, context_menu_alignment_options, services.clone()),
                user_data_profile_service: services.user_data_profile_service.clone(),
                theme_service: services.theme_service.clone(),
                profile_badge: Cell::new(None),
                profile_badge_content: Cell::new(None),
                action: action.clone(),
            }
        });
        let base = this.core.composite.base();
        base.register(action.clone());
        let hooks: Weak<dyn CompositeBarItemHooks> = Rc::downgrade(&this) as Weak<dyn CompositeBarItemHooks>;
        this.core.composite.set_hooks(hooks);
        let weak = Rc::downgrade(&this);
        let action_for_listener = action.clone();
        let services_for_listener = services.clone();
        let listener = services.user_data_profile_service.on_did_change_current_profile().listen(move |_| {
            if weak.upgrade().is_none() {
                return;
            }
            let mut item = action_for_listener.composite_bar_action_item();
            item.class_names = Some(profile_icon(&services_for_listener).as_class_name_array());
            action_for_listener.set_composite_bar_action_item(item);
        });
        base.register(listener);
        this
    }

    /// `updateProfileBadge()`.
    fn update_profile_badge(&self) {
        let (Some(profile_badge), Some(content)) = (self.profile_badge.get(), self.profile_badge_content.get()) else { return };
        let ui = self.core.composite.base().ui().clone();
        let mut tree = ui.tree_mut();
        tree.style_mut(content).content = je_ui::Content::None;
        tree.style_mut(profile_badge).visible = false;

        let profile = self.user_data_profile_service.current_profile();
        if profile.is_default {
            return;
        }
        if profile.icon.as_deref().is_some_and(|icon| icon != default_profile_icon().id) {
            return;
        }
        if !self.action.activities().is_empty() {
            return;
        }
        tree.style_mut(profile_badge).visible = true;
        tree.add_class(content, "profile-text-overlay");
        let theme = self.theme_service.get_color_theme();
        let foreground = theme.get_color("profileBadge.foreground", None).map(|color| je_theme::to_rgba8(&color)).unwrap_or(je_render::Rgba::new(255, 255, 255, 255));
        tree.style_mut(content).background = theme.get_color("profileBadge.background", None).map(|color| je_theme::to_rgba8(&color));
        let initials: String = profile.name.chars().take(2).collect::<String>().to_uppercase();
        set_node_text(&mut tree, content, &initials, &ui_font(9.0, 600), foreground, 10.0);
    }
}

impl CompositeBarItemHooks for GlobalActivityActionViewItem {
    fn compute_title(&self) -> Option<String> {
        let profile = self.user_data_profile_service.current_profile();
        if profile.is_default {
            return None;
        }
        Some(localize("manage profile", "Manage {0} (Profile)", &[&profile.name]))
    }

    fn did_update_activity(&self) {
        self.update_profile_badge();
    }
}

impl IActionViewItem for GlobalActivityActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.core.composite.base()
    }

    fn render(&self, container: NodeId) {
        self.core.render(container);
        let ui = self.core.composite.base().ui().clone();
        let (badge, content) = {
            let mut tree = ui.tree_mut();
            let badge = je_widgets::dom::create_el(&mut tree, container, Role::Generic, "profile-badge");
            let content = je_widgets::dom::create_el(&mut tree, badge, Role::Generic, "profile-badge-content");
            (badge, content)
        };
        self.profile_badge.set(Some(badge));
        self.profile_badge_content.set(Some(content));
        self.update_profile_badge();
    }

    fn update_label(&self) {
        self.core.composite.update_label();
    }

    fn update_aria_label(&self) {
        self.core.composite.update_title();
    }

    fn get_hover_contents(&self) -> Option<je_widgets::hover_api::ManagedHoverContent> {
        None
    }

    fn layout(&self, _vertical: bool, width: f64, height: f64) {
        self.core.composite.layout(width, height);
        if let Some(badge) = self.profile_badge.get() {
            self.core.composite.base().ui().tree_mut().set_bounds(badge, je_render::kurbo::Rect::new(0.0, 0.0, width, height));
        }
    }
}

impl IDisposable for GlobalActivityActionViewItem {
    fn dispose(&self) {
        self.core.composite.dispose();
    }
}

/// O `AuthenticationSessionAccount & { canSignOut: boolean }` do `groupedAccounts`.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupedAccount {
    pub account: AuthenticationSessionAccount,
    pub can_sign_out: bool,
}

/// O `Map<string, ...>` do `groupedAccounts`: a ordem de inserção é a ordem de iteração.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupedAccounts(Vec<(String, Vec<GroupedAccount>)>);

impl GroupedAccounts {
    /// `map.get(providerId)`.
    pub fn get(&self, provider_id: &str) -> Option<&Vec<GroupedAccount>> {
        self.0.iter().find(|(id, _)| id == provider_id).map(|(_, accounts)| accounts)
    }

    /// `map.set(providerId, accounts)`.
    pub fn set(&mut self, provider_id: &str, accounts: Vec<GroupedAccount>) {
        match self.0.iter_mut().find(|(id, _)| id == provider_id) {
            Some(entry) => entry.1 = accounts,
            None => self.0.push((provider_id.to_owned(), accounts)),
        }
    }

    /// `map.delete(providerId)`.
    pub fn delete(&mut self, provider_id: &str) {
        self.0.retain(|(id, _)| id != provider_id);
    }

    /// `map.values()`.
    pub fn values(&self) -> impl Iterator<Item = &Vec<GroupedAccount>> {
        self.0.iter().map(|(_, accounts)| accounts)
    }
}

/// O `<img class="accounts-avatar">` do rótulo: o `src` e se aparece (`style.display`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AvatarImage {
    /// `src` (ausente é o atributo removido).
    pub src: Option<String>,
    /// `style.display !== 'none'`.
    pub visible: bool,
}

/// O estado e a lógica do `AccountsActivityActionViewItem` que não dependem da árvore.
pub struct AccountsCore {
    this: Weak<AccountsCore>,
    ui: Ui,
    grouped_accounts: RefCell<GroupedAccounts>,
    problematic_providers: RefCell<HashSet<String>>,
    initialized: Cell<bool>,
    session_from_embedder: Lazy<Promise<Option<AuthenticationSessionInfo>>>,
    avatar_img: RefCell<Option<AvatarImage>>,
    label: Cell<Option<NodeId>>,
    authentication_service: Rc<dyn IAccountsAuthentication>,
    default_account_service: Rc<dyn IAccountsDefaultAccount>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    command_service: Rc<dyn ICommandService>,
    uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
}

/// O que o [`AccountsCore`] lê dos serviços (o recorte do `this` que o teste original monta à mão).
#[derive(Clone)]
pub struct AccountsCoreDeps {
    pub authentication_service: Rc<dyn IAccountsAuthentication>,
    pub default_account_service: Rc<dyn IAccountsDefaultAccount>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub log_service: LogServiceRef,
    pub command_service: Rc<dyn ICommandService>,
    pub uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
    /// O executor do `new Lazy(() => getCurrentAuthenticationSessionInfo(secretStorageService, productService))`.
    pub session_from_embedder: Rc<dyn Fn() -> Promise<Option<AuthenticationSessionInfo>>>,
}

impl AccountsCoreDeps {
    /// As dependências dos serviços que o `instantiationService` injeta.
    pub fn from_services(services: &GlobalCompositeBarServices) -> Self {
        let secret_storage_service = services.secret_storage_service.clone();
        let product_service = services.product_service.clone();
        Self {
            authentication_service: services.authentication_service.clone(),
            default_account_service: services.default_account_service.clone(),
            configuration_service: services.configuration_service.clone(),
            log_service: services.log_service.clone(),
            command_service: services.command_service.clone(),
            uri_to_browser_uri: services.uri_to_browser_uri.clone(),
            session_from_embedder: Rc::new(move || get_current_authentication_session_info(secret_storage_service.clone(), product_service.clone())),
        }
    }
}

impl AccountsCore {
    pub fn new(ui: &Ui, deps: AccountsCoreDeps) -> Rc<Self> {
        let session_from_embedder = deps.session_from_embedder.clone();
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            ui: ui.clone(),
            grouped_accounts: RefCell::new(GroupedAccounts::default()),
            problematic_providers: RefCell::new(HashSet::new()),
            initialized: Cell::new(false),
            session_from_embedder: Lazy::new(move || session_from_embedder()),
            avatar_img: RefCell::new(None),
            label: Cell::new(None),
            authentication_service: deps.authentication_service,
            default_account_service: deps.default_account_service,
            configuration_service: deps.configuration_service,
            log_service: deps.log_service,
            command_service: deps.command_service,
            uri_to_browser_uri: deps.uri_to_browser_uri,
        })
    }

    /// O `avatarImg` criado no `render`: o `<img>` escondido dentro do rótulo.
    pub fn attach_label(&self, label: NodeId) {
        self.label.set(Some(label));
        *self.avatar_img.borrow_mut() = Some(AvatarImage { src: None, visible: false });
        self.update_avatar();
    }

    /// O `avatarImg`.
    pub fn avatar_img(&self) -> Option<AvatarImage> {
        self.avatar_img.borrow().clone()
    }

    /// `avatarImg.onerror`: esconde a imagem e tira `has-avatar` do rótulo.
    pub fn on_avatar_error(&self) {
        if let Some(avatar) = self.avatar_img.borrow_mut().as_mut() {
            avatar.visible = false;
        }
        self.set_has_avatar(false);
    }

    /// `groupedAccounts`.
    pub fn grouped_accounts(&self) -> std::cell::RefMut<'_, GroupedAccounts> {
        self.grouped_accounts.borrow_mut()
    }

    /// `initialized`.
    pub fn set_initialized(&self, initialized: bool) {
        self.initialized.set(initialized);
    }

    fn set_has_avatar(&self, has_avatar: bool) {
        if let Some(label) = self.label.get() {
            let mut tree = self.ui.tree_mut();
            if has_avatar {
                tree.add_class(label, "has-avatar");
            } else {
                tree.remove_class(label, "has-avatar");
            }
        }
    }

    /// `updateAvatar()`.
    pub fn update_avatar(&self) {
        if self.avatar_img.borrow().is_none() {
            return;
        }

        let mut avatar_icon: Option<URI> = None;
        if matches!(self.configuration_service.get_value(Some(ACCOUNTS_AVATAR_SETTING), None), Dynamic::Bool(true)) {
            avatar_icon = self.get_default_account_avatar_icon();
            if avatar_icon.is_none() {
                avatar_icon = self.grouped_accounts.borrow().values().flatten().find_map(|account| account.account.icon.clone());
            }
        }

        match avatar_icon {
            Some(icon) => {
                *self.avatar_img.borrow_mut() = Some(AvatarImage { src: Some((self.uri_to_browser_uri)(&icon).to_string(true).to_string_lossy()), visible: true });
                self.set_has_avatar(true);
            }
            None => {
                *self.avatar_img.borrow_mut() = Some(AvatarImage { src: None, visible: false });
                self.set_has_avatar(false);
            }
        }
    }

    /// `getDefaultAccountAvatarIcon()`.
    pub fn get_default_account_avatar_icon(&self) -> Option<URI> {
        let current_default_account = self.default_account_service.current_default_account()?;
        let grouped = self.grouped_accounts.borrow();
        let accounts = grouped.get(&current_default_account.authentication_provider.id)?;
        accounts.iter().find(|account| account.account.label == current_default_account.account_name)?.account.icon.clone()
    }

    /// `addOrUpdateAccount(providerId, account)`.
    pub fn add_or_update_account(&self, provider_id: &str, account: AuthenticationSessionAccount) -> Promise<()> {
        let this = self.this.upgrade().expect("AccountsCore vivo");
        let provider_id = provider_id.to_owned();
        async_fn(async move {
            if this.grouped_accounts.borrow().get(&provider_id).is_none() {
                this.grouped_accounts.borrow_mut().set(&provider_id, Vec::new());
            }

            let session_from_embedder = this.session_from_embedder.value().await?;
            let mut can_sign_out = true;
            if let Some(embedder) = session_from_embedder {
                // if we have a session from the embedder, and that session says we can't sign out, and that session is
                // associated with the account we are adding/updating
                if embedder.can_sign_out == Some(false) {
                    let sessions = this.authentication_service.get_sessions(&provider_id).await?;
                    if sessions.iter().any(|session| session.id == embedder.id && session.account.id == account.id) {
                        can_sign_out = false;
                    }
                }
            }

            let mut grouped = this.grouped_accounts.borrow_mut();
            let mut accounts = grouped.get(&provider_id).cloned().unwrap_or_default();
            match accounts.iter_mut().find(|existing| existing.account.label == account.label) {
                Some(existing) => {
                    // if we have an existing account and we discover that we can't sign out of it, update the account
                    // to mark it as "can't sign out"
                    if !can_sign_out {
                        existing.can_sign_out = can_sign_out;
                    }
                    existing.account.icon = account.icon.clone();
                }
                None => accounts.push(GroupedAccount { account, can_sign_out }),
            }
            grouped.set(&provider_id, accounts);
            Ok(())
        })
    }

    /// `removeAccount(providerId, account)`.
    pub fn remove_account(&self, provider_id: &str, account: &AuthenticationSessionAccount) {
        let mut grouped = self.grouped_accounts.borrow_mut();
        let Some(accounts) = grouped.get(provider_id) else { return };
        let mut accounts = accounts.clone();
        let Some(index) = accounts.iter().position(|existing| existing.account.id == account.id) else { return };
        accounts.remove(index);
        if accounts.is_empty() {
            grouped.delete(provider_id);
        } else {
            grouped.set(provider_id, accounts);
        }
    }

    /// `addAccountsFromProvider(providerId)`: nunca rejeita, como o `try/catch` do original.
    pub fn add_accounts_from_provider(&self, provider_id: &str) -> Promise<()> {
        let this = self.this.upgrade().expect("AccountsCore vivo");
        let provider_id = provider_id.to_owned();
        async_fn(async move {
            match this.authentication_service.get_sessions(&provider_id).await {
                Ok(sessions) => {
                    this.problematic_providers.borrow_mut().remove(&provider_id);
                    for session in sessions {
                        if let Err(error) = this.add_or_update_account(&provider_id, session.account).await {
                            this.log_service.error_args(&LogMessage::from(error), &[]);
                        }
                    }
                }
                Err(error) => {
                    this.log_service.error_args(&LogMessage::from(error), &[]);
                    this.problematic_providers.borrow_mut().insert(provider_id);
                }
            }
            Ok(())
        })
    }

    /// `registerListeners()` do item de contas, devolvendo o que o item registra.
    pub fn register_listeners(self: &Rc<Self>) -> Vec<DisposableRef> {
        let mut listeners: Vec<DisposableRef> = Vec::new();
        let weak = Rc::downgrade(self);
        listeners.push(self.authentication_service.on_did_register_authentication_provider().listen({
            let weak = weak.clone();
            move |provider| {
                let Some(this) = weak.upgrade() else { return };
                let id = provider.id.clone();
                let _ = async_fn(async move {
                    this.add_accounts_from_provider(&id).await?;
                    this.update_avatar();
                    Ok(())
                });
            }
        }));
        listeners.push(self.authentication_service.on_did_unregister_authentication_provider().listen({
            let weak = weak.clone();
            move |provider| {
                let Some(this) = weak.upgrade() else { return };
                this.grouped_accounts.borrow_mut().delete(&provider.id);
                this.problematic_providers.borrow_mut().remove(&provider.id);
                this.update_avatar();
            }
        }));
        listeners.push(self.authentication_service.on_did_change_sessions().listen({
            let weak = weak.clone();
            move |change| {
                let Some(this) = weak.upgrade() else { return };
                let change = change.clone();
                let _ = async_fn(async move {
                    for removed in change.event.removed.iter().flatten() {
                        this.remove_account(&change.provider_id, &removed.account);
                    }
                    for changed in change.event.changed.iter().flatten().chain(change.event.added.iter().flatten()) {
                        if let Err(error) = this.add_or_update_account(&change.provider_id, changed.account.clone()).await {
                            this.log_service.error_args(&LogMessage::from(error), &[]);
                        }
                    }
                    this.update_avatar();
                    Ok(())
                });
            }
        }));
        listeners.push(self.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    if event.affects_configuration(ACCOUNTS_AVATAR_SETTING, None) {
                        this.update_avatar();
                    }
                }
            }
        }));
        listeners.push(self.default_account_service.on_did_change_default_account().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_avatar();
            }
        }));
        listeners
    }

    /// `doInitialize()`.
    pub fn do_initialize(&self, store: &Rc<DisposableStore>) -> Promise<()> {
        let this = self.this.upgrade().expect("AccountsCore vivo");
        let store = store.clone();
        async_fn(async move {
            // Resolve extension-host-backed GitHub state before exposing agent-specific accounts.
            this.default_account_service.get_default_account().await?;
            if store.is_disposed() {
                return Ok(());
            }
            let provider_ids = this.authentication_service.get_provider_ids();
            let results = Promise::all_settled(provider_ids.iter().map(|provider_id| this.add_accounts_from_provider(provider_id)).collect()).await?;

            // Log any errors that occurred while initializing. We try to be best effort here to show the most amount of accounts
            for result in results {
                if let Err(error) = result {
                    this.log_service.error_args(&LogMessage::from(error), &[]);
                }
            }

            this.initialized.set(true);
            this.update_avatar();
            Ok(())
        })
    }

    /// `resolveMainMenuActions(accountsMenu, disposables)`.
    pub fn resolve_main_menu_actions(&self, accounts_menu: &dyn IMenu, disposables: &Rc<DisposableStore>) -> Promise<Vec<ActionRef>> {
        let groups: Vec<(String, Vec<ActionRef>)> =
            accounts_menu.get_actions(None).iter().map(|(group, actions)| (group.clone(), actions.iter().map(menu_action_as_action).collect())).collect();
        self.resolve_main_menu_actions_for_groups(groups, disposables)
    }

    /// `resolveMainMenuActions` sobre os grupos do menu já como ações.
    pub fn resolve_main_menu_actions_for_groups(&self, other_commands: Vec<(String, Vec<ActionRef>)>, disposables: &Rc<DisposableStore>) -> Promise<Vec<ActionRef>> {
        let this = self.this.upgrade().expect("AccountsCore vivo");
        let disposables = disposables.clone();
        async_fn(async move {
            let providers: Vec<String> = this.authentication_service.get_provider_ids().into_iter().filter(|p| !p.starts_with(INTERNAL_AUTH_PROVIDER_PREFIX)).collect();
            let mut menus: Vec<ActionRef> = Vec::new();

            let registered_providers: Vec<&String> = providers.iter().filter(|id| !this.authentication_service.is_dynamic_authentication_provider(id)).collect();
            let dynamic_providers: Vec<&String> = providers.iter().filter(|id| this.authentication_service.is_dynamic_authentication_provider(id)).collect();

            if !this.initialized.get() {
                let loading: ActionRef = Action::new("noAccountsAvailable", &localize("loading", "Loading...", &[]), "", false, None);
                disposables.add(Rc::new(DisposedAction(loading.clone())));
                menus.push(loading);
            } else {
                for provider_id in &registered_providers {
                    let provider = this.authentication_service.get_provider(provider_id);
                    let accounts = this.grouped_accounts.borrow().get(provider_id).cloned();
                    let Some(accounts) = accounts else {
                        if this.problematic_providers.borrow().contains(*provider_id) {
                            let unavailable: ActionRef =
                                Action::new("providerUnavailable", &localize("authProviderUnavailable", "{0} is currently unavailable", &[&provider.label()]), "", false, None);
                            disposables.add(Rc::new(DisposedAction(unavailable.clone())));
                            menus.push(unavailable);
                            // try again in the background so that if the failure was intermittent, we can resolve it on the next showing of the menu
                            this.add_accounts_from_provider(provider_id).await?;
                        }
                        continue;
                    };

                    let can_use_mcp = provider.authorization_servers().is_some_and(|servers| !servers.is_empty());
                    for entry in accounts {
                        let label = entry.account.label.clone();
                        let command = |id: &str, title: String, command_id: &'static str| {
                            let command_service = this.command_service.clone();
                            let (provider_id, label) = (provider_id.to_string(), label.clone());
                            command_action(id, title, true, move || {
                                command_service.execute_command(command_id, vec![account_args(&provider_id, &label)]);
                            })
                        };
                        let mut provider_sub_menu_actions: Vec<ActionRef> = vec![command(
                            &format!("configureSessions{label}"),
                            localize("manageTrustedExtensions", "Manage Trusted Extensions", &[]),
                            "_manageTrustedExtensionsForAccount",
                        )];
                        if can_use_mcp {
                            provider_sub_menu_actions.push(command(
                                &format!("configureSessions{label}"),
                                localize("manageTrustedMCPServers", "Manage Trusted MCP Servers", &[]),
                                "_manageTrustedMCPServersForAccount",
                            ));
                        }
                        if entry.can_sign_out {
                            provider_sub_menu_actions.push(command("signOut", localize("signOut", "Sign Out", &[]), "_signOutOfAccount"));
                        }
                        let sub_menu: ActionRef = SubmenuAction::new("activitybar.submenu", &format!("{label} ({})", provider.label()), provider_sub_menu_actions, None);
                        menus.push(sub_menu);
                    }
                }

                if !dynamic_providers.is_empty() && !registered_providers.is_empty() {
                    menus.push(Separator::new());
                }

                for provider_id in &dynamic_providers {
                    let provider = this.authentication_service.get_provider(provider_id);
                    let accounts = this.grouped_accounts.borrow().get(provider_id).cloned();
                    // Provide _some_ discoverable way to manage dynamic authentication providers. This will either show up inside
                    // the account submenu or as a top-level menu item if there are no accounts.
                    let manage_dynamic_auth_providers_action = |command_service: Rc<dyn ICommandService>| {
                        command_action("manageDynamicAuthProviders", localize("manageDynamicAuthProviders", "Manage Dynamic Authentication Providers...", &[]), true, move || {
                            command_service.execute_command("workbench.action.removeDynamicAuthenticationProviders", Vec::new());
                        })
                    };
                    let Some(accounts) = accounts else {
                        if this.problematic_providers.borrow().contains(*provider_id) {
                            let unavailable: ActionRef =
                                Action::new("providerUnavailable", &localize("authProviderUnavailable", "{0} is currently unavailable", &[&provider.label()]), "", false, None);
                            disposables.add(Rc::new(DisposedAction(unavailable.clone())));
                            menus.push(unavailable);
                            // try again in the background so that if the failure was intermittent, we can resolve it on the next showing of the menu
                            this.add_accounts_from_provider(provider_id).await?;
                        }
                        menus.push(manage_dynamic_auth_providers_action(this.command_service.clone()));
                        continue;
                    };

                    for entry in accounts {
                        let label = entry.account.label.clone();
                        let command_service = this.command_service.clone();
                        let (mcp_provider_id, mcp_label) = (provider_id.to_string(), label.clone());
                        let manage_mcp_action = command_action(
                            &format!("configureSessions{label}"),
                            localize("manageTrustedMCPServers", "Manage Trusted MCP Servers", &[]),
                            true,
                            move || {
                                command_service.execute_command("_manageTrustedMCPServersForAccount", vec![account_args(&mcp_provider_id, &mcp_label)]);
                            },
                        );
                        let mut provider_sub_menu_actions: Vec<ActionRef> = vec![manage_mcp_action, manage_dynamic_auth_providers_action(this.command_service.clone())];
                        if entry.can_sign_out {
                            let command_service = this.command_service.clone();
                            let (out_provider_id, out_label) = (provider_id.to_string(), label.clone());
                            provider_sub_menu_actions.push(command_action("signOut", localize("signOut", "Sign Out", &[]), true, move || {
                                command_service.execute_command("_signOutOfAccount", vec![account_args(&out_provider_id, &out_label)]);
                            }));
                        }
                        let sub_menu: ActionRef = SubmenuAction::new("activitybar.submenu", &format!("{label} ({})", provider.label()), provider_sub_menu_actions, None);
                        menus.push(sub_menu);
                    }
                }
            }

            let (shared_account_command_groups, remaining_command_groups): (Vec<_>, Vec<_>) =
                other_commands.into_iter().partition(|(group, _)| group == ACCOUNTS_SHARED_SIGN_IN_GROUP);
            let append_command_groups = |menus: &mut Vec<ActionRef>, groups: Vec<(String, Vec<ActionRef>)>| {
                for (_, actions) in groups {
                    if !menus.is_empty() && !actions.is_empty() {
                        menus.push(Separator::new());
                    }
                    menus.extend(actions);
                }
            };
            append_command_groups(&mut menus, shared_account_command_groups);

            // O menu "Sign in to ChatGPT" (`createCodexAccountMenuActions`) é da camada de IA, recortada do produto.
            append_command_groups(&mut menus, remaining_command_groups);

            Ok(menus)
        })
    }
}

/// `disposables.add(action)`: descarta a [`Action`] por baixo quando o menu fecha.
struct DisposedAction(ActionRef);

impl IDisposable for DisposedAction {
    fn dispose(&self) {
        if let Some(action) = self.0.as_base_action() {
            action.dispose();
        }
    }
}

/// `toAction({ id, label, enabled, run })`.
fn command_action(id: &str, label: String, enabled: bool, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label,
        tooltip: None,
        enabled: Some(enabled),
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

/// O `{ providerId, accountLabel }` que os comandos de conta recebem.
fn account_args(provider_id: &str, account_label: &str) -> Dynamic {
    Dynamic::Object(je_base::dynamic::ObjectRef::from_entries([
        ("providerId", Dynamic::String(provider_id.into())),
        ("accountLabel", Dynamic::String(account_label.into())),
    ]))
}

/// `AccountsActivityActionViewItem`.
pub struct AccountsActivityActionViewItem {
    core: Rc<GlobalActivityCore>,
    accounts: Rc<AccountsCore>,
    fill_context_menu_actions: FillContextMenuActions,
    store: Rc<DisposableStore>,
}

impl AccountsActivityActionViewItem {
    /// `ACCOUNTS_VISIBILITY_PREFERENCE_KEY`.
    pub const ACCOUNTS_VISIBILITY_PREFERENCE_KEY: &'static str = ACCOUNTS_VISIBILITY_PREFERENCE_KEY;

    pub fn new(
        ui: &Ui,
        context_menu_actions_provider: Rc<dyn Fn() -> Vec<ActionRef>>,
        options: ICompositeBarActionViewItemOptions,
        context_menu_alignment_options: ContextMenuAlignmentOptions,
        fill_context_menu_actions: FillContextMenuActions,
        services: GlobalCompositeBarServices,
    ) -> Rc<Self> {
        let action = CompositeBarAction::new(ICompositeBarActionItem {
            id: ACCOUNTS_ACTIVITY_ID.to_owned(),
            name: localize("accounts", "Accounts", &[]),
            keybinding_id: None,
            class_names: Some(accounts_icon().as_class_name_array()),
            icon_url: None,
        });
        let accounts = AccountsCore::new(ui, AccountsCoreDeps::from_services(&services));
        let store = DisposableStore::new();
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let composite = CompositeBarItemCore::new(
                ui,
                weak.clone() as Weak<dyn IActionViewItem>,
                action.clone(),
                options,
                Rc::new(|_| true),
                services.theme_service.clone(),
                services.hover_service.clone(),
                services.keybinding_service.clone(),
            );
            Self {
                core: GlobalActivityCore::new(composite, accounts_context(), context_menu_actions_provider, context_menu_alignment_options, services.clone()),
                accounts: accounts.clone(),
                fill_context_menu_actions,
                store: store.clone(),
            }
        });
        let base = this.core.composite.base();
        base.register(action.clone());
        base.register(store.clone());
        let hooks: Weak<dyn GlobalActivityMenuHooks> = Rc::downgrade(&this) as Weak<dyn GlobalActivityMenuHooks>;
        this.core.set_menu_hooks(hooks);
        for listener in accounts.register_listeners() {
            base.register(listener);
        }
        this.initialize(&services);
        this
    }

    /// `initialize()`: this function exists to ensure that the accounts are added for auth providers that had already been
    /// registered before the menu was created.
    fn initialize(self: &Rc<Self>, services: &GlobalCompositeBarServices) {
        let this = self.clone();
        let lifecycle_service = services.lifecycle_service.clone();
        let _ = async_fn(async move {
            // Resolving the menu doesn't need to happen immediately, so we can wait until after the workbench has been restored
            // and only run this when the system is idle.
            lifecycle_service.when(LifecyclePhase::Restored).await?;
            if this.store.is_disposed() {
                return Ok(());
            }
            let weak = Rc::downgrade(&this);
            let idle = run_when_global_idle(
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.accounts.do_initialize(&this.store);
                    }
                },
                None,
            );
            this.store.add(idle);
            Ok(())
        });
    }

    /// O núcleo das contas.
    pub fn accounts(&self) -> &Rc<AccountsCore> {
        &self.accounts
    }
}

impl GlobalActivityMenuHooks for AccountsActivityActionViewItem {
    fn resolve_main_menu_actions(&self, menu: Rc<dyn IMenu>, disposables: Rc<DisposableStore>) -> Promise<Vec<ActionRef>> {
        self.accounts.resolve_main_menu_actions(&*menu, &disposables)
    }

    fn resolve_context_menu_actions(&self, _disposables: Rc<DisposableStore>) -> Promise<Vec<ActionRef>> {
        let mut actions = self.core.default_context_menu_actions();
        (self.fill_context_menu_actions)(&mut actions);
        Promise::resolved(actions)
    }
}

impl IActionViewItem for AccountsActivityActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.core.composite.base()
    }

    fn render(&self, container: NodeId) {
        self.core.render(container);
        if let Some(label) = self.core.composite.label() {
            self.accounts.attach_label(label);
        }
    }

    fn update_label(&self) {
        self.core.composite.update_label();
    }

    fn update_aria_label(&self) {
        self.core.composite.update_title();
    }

    fn get_hover_contents(&self) -> Option<je_widgets::hover_api::ManagedHoverContent> {
        None
    }

    fn layout(&self, _vertical: bool, width: f64, height: f64) {
        self.core.composite.layout(width, height);
    }
}

impl IDisposable for AccountsActivityActionViewItem {
    fn dispose(&self) {
        self.core.composite.dispose();
    }
}

/// `simpleActivityContextMenuActions(storageService, isAccount)`.
fn simple_activity_context_menu_actions(storage_service: &Rc<dyn StorageService>, is_account: bool) -> Vec<ActionRef> {
    let mut actions: Vec<ActionRef> = Vec::new();
    if is_account {
        let storage = storage_service.clone();
        actions.push(command_action("hideAccounts", localize("hideAccounts", "Hide Accounts", &[]), true, move || set_accounts_action_visible(&*storage, false)));
        actions.push(Separator::new());
    }
    let toggling = storage_service.clone();
    actions.push(to_action(ToActionProps {
        id: "toggle.hideAccounts".to_owned(),
        label: localize("accounts", "Accounts", &[]),
        tooltip: None,
        enabled: None,
        checked: Some(is_accounts_action_visible(&**storage_service)),
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            set_accounts_action_visible(&*toggling, !is_accounts_action_visible(&*toggling));
            MaybePromise::Value(Dynamic::Undefined)
        }),
    }));
    actions.push(to_action(ToActionProps {
        id: "toggle.hideManage".to_owned(),
        label: localize("manage", "Manage", &[]),
        tooltip: None,
        enabled: Some(false),
        checked: Some(true),
        class: None,
        alt: None,
        run: Rc::new(|_| je_base::errors::throw(je_base::errors::Error::new("\"Manage\" can not be hidden".to_owned()))),
    }));
    actions
}

/// As opções do item compacto das variantes simples: as cores do selo da barra de atividades e `compact`.
fn simple_item_options(hover_options: ActivityHoverOptions, base: IActionViewItemOptions) -> ICompositeBarActionViewItemOptions {
    ICompositeBarActionViewItemOptions {
        base,
        icon: true,
        colors: Rc::new(|theme| CompositeBarColors {
            badge_background: theme.get_color(ACTIVITY_BAR_BADGE_BACKGROUND, None),
            badge_foreground: theme.get_color(ACTIVITY_BAR_BADGE_FOREGROUND, None),
            ..CompositeBarColors::default()
        }),
        hover_options,
        has_popup: true,
        compact: true,
    }
}

/// `new SimpleAccountActivityActionViewItem(hoverOptions, options, ...)`: o item de contas compacto, sem o gancho de contexto.
pub fn simple_account_activity_action_view_item(
    ui: &Ui,
    hover_options: ActivityHoverOptions,
    options: IActionViewItemOptions,
    services: GlobalCompositeBarServices,
) -> Rc<AccountsActivityActionViewItem> {
    let storage_service = services.storage_service.clone();
    AccountsActivityActionViewItem::new(
        ui,
        Rc::new(move || simple_activity_context_menu_actions(&storage_service, true)),
        simple_item_options(hover_options, options),
        Rc::new(|| None),
        Rc::new(|_| {}),
        services,
    )
}

/// `new SimpleGlobalActivityActionViewItem(hoverOptions, options, ...)`: o item "Manage" compacto.
pub fn simple_global_activity_action_view_item(
    ui: &Ui,
    hover_options: ActivityHoverOptions,
    options: IActionViewItemOptions,
    services: GlobalCompositeBarServices,
) -> Rc<GlobalActivityActionViewItem> {
    let storage_service = services.storage_service.clone();
    GlobalActivityActionViewItem::new(
        ui,
        Rc::new(move || simple_activity_context_menu_actions(&storage_service, false)),
        simple_item_options(hover_options, options),
        Rc::new(|| None),
        services,
    )
}
