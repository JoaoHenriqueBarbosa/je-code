//! Porte de `src/vs/workbench/browser/parts/titlebar/menubarControl.ts`: o `MenubarControl` (menus de topo do
//! `MenuId.MenubarMainMenu`, atualização agendada, recentes, estado do serviço de atualização, aviso de acessibilidade) e o
//! `CustomMenubarControl`, que monta o `MenuBar` de `je_widgets::menu_bar` no contêiner do menu compacto.
//!
//! Divergências:
//! - **Herança vira composição.** A base `MenubarControl` e as sobrescritas de `CustomMenubarControl` (`onDidChangeWindowFocus`,
//!   `onUpdateStateChange`, `onDidChangeRecentlyOpened`, `onUpdateKeybindings`, `registerListeners`) são métodos de um só
//!   tipo, [`CustomMenubarControl`], com a guarda `visible` das sobrescritas aplicada antes da lógica da base.
//! - **DOM vira árvore de widgets.** O `HTMLElement` do contêiner é o [`MenuBarContainer`] com o nó ligado por
//!   [`MenuBarContainer::attach`]; sem nó ligado `setupCustomMenubar` não monta nada, como o `if (!this.container)` do original.
//!   A classe `inactive` é a classe do nó. O `FOCUS_IN` do contêiner é o `onFocusStateChange(true)` do `MenuBar`, e o
//!   `FOCUS_OUT` é o `onFocusStateChange(false)`.
//! - **Eventos de janela.** O `RESIZE` da janela principal e o `onDidChangeFullscreen` chegam por
//!   [`CustomMenubarControl::on_window_resize`] e [`CustomMenubarControl::on_did_change_fullscreen`], chamados pelo dono da
//!   janela (que decide o caso `isIOS && pointerEvents` do original).
//! - **`instanceof`.** `Separator`, `MenuItemAction` e `SubmenuItemAction` são distinguidos por [`FlatMenuEntry`] no primeiro
//!   nível e por `as_any` e [`SubmenuItemAction::from_action`] nos aninhados.
//! - **Estilos.** `defaultMenuStyles` vem de um [`MenubarStylesProvider`], porque o tema de widgets é do dono da janela.
//! - **`IOpenRecentAction`** é o [`OpenRecentMenuAction`]: a ação mais `uri` e `remoteAuthority`.
//! - **`getMenuBarVisibility`, `hasNativeMenu`, `isFullscreen`, `isMacintosh` e `isNative`** são do [`TitleBarHost`].
//!
//! [`TitleBarHost`]: crate::services::layout::layout_service::TitleBarHost

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionCallback, ActionRef, ActionRunner, IAction, IActionRunner, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::Emitter;
use je_base::labels::{mnemonic_menu_label, unmnemonic_label};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::MaybePromise;
use je_base::strings::{ELLIPSIS, truncate_middle};
use je_base::uri::URI;
use je_editor::core_2d::dimension::Dimension;
use je_platform::accessibility::IAccessibilityService;
use je_platform::action::{CommandAction, CommandActionTitle, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2Keybinding, Action2Options, IMenu, IMenuService, MenuAction, MenuActionOptions, MenuCreateOptions, MenuItemAction, SubmenuItemAction, register_action2};
use je_platform::actions::menu_entry_action_view_item::{FlatMenuEntry, get_flat_context_menu_entries};
use je_platform::actions::menu_ids;
use je_platform::commands::ICommandService;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkeys::is_web_context;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::IKeybindingService;
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};
use je_platform::label::label::{ILabelService, UriLabelOptions, Verbosity, WorkspaceLabelOptions, WorkspaceLabelTarget};
use je_platform::notification::{BasePromptChoice, INotificationService, NotificationMessage, PromptChoice, PromptChoiceKind, Severity};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::ITelemetryService;
use je_platform::update::{State, UpdateService};
use je_platform::workspaces::{IWorkspacesService, Recent, RecentWorkspaceOrFolder, RecentlyOpened};
use je_instantiation::ServicesAccessor;
use je_base::key_codes::{KeyCode, KeyMod};
use je_widgets::menu::{HorizontalDirection, IMenuDirection, IMenuStyles, VerticalDirection};
use je_widgets::menu_bar::{IMenuBarOptions, MenuBar, MenuBarMenu, MenuBarStyles};

use crate::browser::parts::activitybar::activitybar_part::MenuBarContainer;
use crate::services::host::host::{IHostService, OpenWindowOptions, WindowOpenable};
use crate::services::layout::layout_service::{ActivityBarPosition, MAIN_WINDOW, TitleBarHost, WindowId};
use crate::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService};

/// `MenuSettings.MenuBarVisibility`.
pub const MENU_BAR_VISIBILITY_SETTING: &str = "window.menuBarVisibility";

/// `MenuSettings.MenuStyle`.
const MENU_STYLE_SETTING: &str = "window.menuStyle";

/// `OpenRecentAction.ID`.
const OPEN_RECENT_ACTION_ID: &str = "workbench.action.openRecent";

/// As configurações que refazem a barra (`MenubarControl.keys`).
const KEYS: [&str; 5] = [MENU_BAR_VISIBILITY_SETTING, "window.enableMenuBarMnemonics", "window.customMenuBarAltFocus", "workbench.sideBar.location", "window.nativeTabs"];

const MAX_MENU_RECENT_ENTRIES: usize = 10;
const MAX_MENU_RECENT_LABEL_LENGTH: usize = 120;

/// `MenuBarVisibility`: `'classic' | 'visible' | 'toggle' | 'hidden' | 'compact'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuBarVisibility {
    Classic,
    Visible,
    Toggle,
    Hidden,
    Compact,
}

impl MenuBarVisibility {
    /// O texto da configuração; `None` para um valor desconhecido.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "classic" => Some(Self::Classic),
            "visible" => Some(Self::Visible),
            "toggle" => Some(Self::Toggle),
            "hidden" => Some(Self::Hidden),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

/// O `CustomMenubarControl` que a barra de atividades instala.
pub trait ICustomMenubarControl: IDisposable {
    /// `create(parent)`.
    fn create(&self, parent: &Rc<MenuBarContainer>);

    /// `toggleFocus()`.
    fn toggle_focus(&self);
}

/// `instantiationService.createInstance(CustomMenubarControl)`.
pub type MenubarFactory = Rc<dyn Fn() -> Rc<dyn ICustomMenubarControl>>;

/// `defaultMenuStyles` e as cores da barra (`default_menu_bar_styles`), resolvidas no tema atual.
pub type MenubarStylesProvider = Rc<dyn Fn() -> (IMenuStyles, MenuBarStyles)>;

/// As dependências injetadas no construtor de `CustomMenubarControl`.
pub struct MenubarControlServices {
    pub menu_service: Rc<dyn IMenuService>,
    pub workspaces_service: Rc<dyn IWorkspacesService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub label_service: Rc<dyn ILabelService>,
    pub update_service: Rc<dyn UpdateService>,
    pub storage_service: Rc<dyn StorageService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub preferences_service: Rc<dyn IPreferencesService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub host_service: Rc<dyn IHostService>,
    pub command_service: Rc<dyn ICommandService>,
    pub title_bar_host: Rc<dyn TitleBarHost>,
    pub platform: Platform,
    pub styles: MenubarStylesProvider,
}

/// `IOpenRecentAction`: `IAction & { uri; remoteAuthority? }`.
pub struct OpenRecentMenuAction {
    pub action: ActionRef,
    pub uri: URI,
    pub remote_authority: Option<String>,
}

struct TopLevelMenu {
    /// `title.original`: a chave de `menus` e de `topLevelTitles`.
    key: String,
    menu: Rc<dyn IMenu>,
    /// `mnemonicTitle ?? value`.
    title: String,
}

/// O que `updateActions` distingue com `instanceof`.
enum MenuNode<'a> {
    Separator,
    Item(&'a MenuItemAction),
    Submenu(Rc<SubmenuItemAction>),
    Other,
}

fn title_text(title: &Title) -> String {
    match title {
        Title::Plain(text) => text.clone(),
        Title::Localized(title) => title.mnemonic_title.clone().unwrap_or_else(|| title.value.clone()),
    }
}

fn classify(action: &ActionRef) -> (String, MenuNode<'_>) {
    let id = action.id();
    if action.as_any().is::<Separator>() {
        return (id, MenuNode::Separator);
    }
    if let Some(submenu) = SubmenuItemAction::from_action(action) {
        return (id, MenuNode::Submenu(submenu));
    }
    match action.as_any().downcast_ref::<MenuItemAction>() {
        Some(item) => (id, MenuNode::Item(item)),
        None => (id, MenuNode::Other),
    }
}

fn entry_node(entry: &FlatMenuEntry) -> (String, MenuNode<'_>) {
    match entry {
        FlatMenuEntry::Separator => (String::new(), MenuNode::Separator),
        FlatMenuEntry::Item(item) => (item.id(), MenuNode::Item(item)),
        FlatMenuEntry::Submenu(submenu) => (submenu.as_action().id(), MenuNode::Submenu(submenu.clone())),
    }
}

/// O teste de `browserEvent.ctrlKey || shiftKey` (ou `metaKey || altKey` no macOS) do clique num recente.
fn open_in_new_window(event: &Dynamic, is_macintosh: bool) -> bool {
    if !matches!(event, Dynamic::Object(_)) {
        return false;
    }
    let flag = |key: &str| matches!(event.get_str(key), Dynamic::Bool(true));
    if is_macintosh { flag("metaKey") || flag("altKey") } else { flag("ctrlKey") || flag("shiftKey") }
}

thread_local! {
    /// `focusMenuBarEmitter`: criado, junto com a ação, no primeiro uso (issue 205836).
    static FOCUS_MENU_BAR_EMITTER: RefCell<Option<Rc<Emitter<()>>>> = const { RefCell::new(None) };
}

struct FocusMenuBarAction;

impl Action2 for FocusMenuBarAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("focusMenu", "Focus Application Menu");
        Action2Options {
            command: CommandAction::new("workbench.actions.menubar.focus", Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None })),
            menu: Vec::new(),
            keybinding: vec![Action2Keybinding {
                keybindings: Keybindings { primary: Some(KeyMod::ALT | KeyCode::F10.bits()), ..Keybindings::default() },
                weight: KeybindingWeight::WORKBENCH_CONTRIB,
                args: Dynamic::Undefined,
                when: Some(is_web_context().expr()),
            }],
            f1: true,
        }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let emitter = FOCUS_MENU_BAR_EMITTER.with(|cell| cell.borrow().clone());
        if let Some(emitter) = emitter {
            emitter.fire(());
        }
    }
}

/// `enableFocusMenuBarAction()`.
fn enable_focus_menu_bar_action() -> Rc<Emitter<()>> {
    if let Some(emitter) = FOCUS_MENU_BAR_EMITTER.with(|cell| cell.borrow().clone()) {
        return emitter;
    }
    let emitter = Emitter::new();
    FOCUS_MENU_BAR_EMITTER.with(|cell| *cell.borrow_mut() = Some(emitter.clone()));
    register_action2(Rc::new(FocusMenuBarAction));
    emitter
}

/// `CustomMenubarControl` (com a base `MenubarControl`).
pub struct CustomMenubarControl {
    weak_self: Weak<CustomMenubarControl>,
    base: Rc<Disposable>,
    services: MenubarControlServices,
    main_menu: Rc<dyn IMenu>,
    main_menu_disposables: Rc<DisposableStore>,
    top_level: RefCell<Vec<TopLevelMenu>>,
    recently_opened: RefCell<RecentlyOpened>,
    menu_updater: Rc<RunOnceScheduler>,
    menubar: RefCell<Option<Rc<MenuBar>>>,
    container: RefCell<Option<Rc<MenuBarContainer>>>,
    always_on_mnemonics: Cell<bool>,
    focus_inside_menubar: Cell<bool>,
    pending_first_time_update: Cell<bool>,
    visible: Cell<bool>,
    action_runner: Rc<ActionRunner>,
    web_navigation_menu: Rc<dyn IMenu>,
    on_visibility_change: Rc<Emitter<bool>>,
    on_focus_state_change: Rc<Emitter<bool>>,
    reinstall_disposables: Rc<DisposableStore>,
    update_actions_disposables: Rc<DisposableStore>,
}

impl CustomMenubarControl {
    pub fn new(services: MenubarControlServices) -> Rc<Self> {
        let base = Disposable::new();
        let main_menu = services.menu_service.create_menu(&menu_ids::menubar_main_menu(), services.context_key_service.clone(), None);
        base.register(main_menu.clone());
        let web_navigation_menu = services.menu_service.create_menu(&menu_ids::menubar_home_menu(), services.context_key_service.clone(), None);
        base.register(web_navigation_menu.clone());

        let store = |base: &Rc<Disposable>| {
            let store = DisposableStore::new();
            base.register(store.clone());
            store
        };
        let emitter = |base: &Rc<Disposable>| {
            let emitter = Emitter::new();
            base.register(emitter.clone());
            emitter
        };
        let action_runner = ActionRunner::new();
        base.register(action_runner.clone());

        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let updater_weak = weak.clone();
            let menu_updater = RunOnceScheduler::new(
                move || {
                    if let Some(this) = updater_weak.upgrade() {
                        this.do_update_menubar(false);
                    }
                },
                200,
            );
            let cancel_updater = menu_updater.clone();
            base.register(to_disposable(move || cancel_updater.cancel()));
            Self {
                weak_self: weak.clone(),
                main_menu_disposables: store(&base),
                top_level: RefCell::new(Vec::new()),
                recently_opened: RefCell::new(RecentlyOpened::default()),
                menu_updater,
                menubar: RefCell::new(None),
                container: RefCell::new(None),
                always_on_mnemonics: Cell::new(false),
                focus_inside_menubar: Cell::new(false),
                pending_first_time_update: Cell::new(false),
                visible: Cell::new(true),
                action_runner,
                web_navigation_menu,
                on_visibility_change: emitter(&base),
                on_focus_state_change: emitter(&base),
                reinstall_disposables: store(&base),
                update_actions_disposables: store(&base),
                main_menu,
                services,
                base,
            }
        });

        this.setup_main_menu();
        this.notify_user_of_custom_menubar_accessibility();

        let telemetry = this.services.telemetry_service.clone();
        this.base.register(this.action_runner.on_did_run().listen(move |event| {
            telemetry.public_log2("workbenchActionExecuted", Some(Dynamic::object(vec![("id", Dynamic::string(&event.action.id())), ("from", Dynamic::string("menu"))])));
        }));

        let weak = Rc::downgrade(&this);
        this.services.workspaces_service.get_recently_opened().then(move |recently_opened| {
            if let Some(this) = weak.upgrade() {
                *this.recently_opened.borrow_mut() = recently_opened;
            }
        });

        this.register_listeners();
        this
    }

    // #region MenubarControl

    fn register_listeners(&self) {
        let s = &self.services;
        let weak = self.weak_self.clone();

        macro_rules! on {
            ($event:expr, |$this:ident, $arg:pat_param| $body:expr) => {{
                let weak = weak.clone();
                self.base.register($event.listen(move |$arg| {
                    if let Some($this) = weak.upgrade() {
                        $body
                    }
                }));
            }};
        }

        // Listen for window focus changes
        on!(s.host_service.on_did_change_focus(), |this, focus| this.on_did_change_window_focus(*focus));
        // Update when config changes
        on!(s.configuration_service.on_did_change_configuration(), |this, event| this.on_configuration_updated(event));
        // Listen to update service
        on!(s.update_service.on_state_change(), |this, _| this.on_update_state_change());
        // Listen for changes in recently opened menu
        on!(s.workspaces_service.on_did_change_recently_opened(), |this, _| this.on_did_change_recently_opened());
        // Listen to keybindings change
        on!(s.keybinding_service.on_did_update_keybindings(), |this, _| this.update_menubar());
        // Update recent menu items on formatter registration
        on!(s.label_service.on_did_change_formatters(), |this, _| this.on_did_change_recently_opened());
        // Listen for changes on the main menu
        on!(self.main_menu.on_did_change(), |this, _| {
            this.setup_main_menu();
            this.do_update_menubar(true);
        });

        // Mnemonics require fullscreen in web
        if s.platform.is_web {
            on!(self.web_navigation_menu.on_did_change(), |this, _| this.update_menubar());
            let focus_emitter = enable_focus_menu_bar_action();
            on!(focus_emitter.event(), |this, _| {
                let menubar = this.menubar.borrow().clone();
                if let Some(menubar) = menubar {
                    menubar.toggle_focus();
                }
            });
        }
    }

    fn setup_main_menu(&self) {
        self.main_menu_disposables.clear();
        let mut top_level: Vec<TopLevelMenu> = Vec::new();
        let groups = self.main_menu.get_actions(None);
        if let Some((_, actions)) = groups.first() {
            for action in actions {
                if let MenuAction::Submenu(submenu) = action
                    && let Title::Localized(title) = &submenu.item.title
                {
                    let menu = self.services.menu_service.create_menu(
                        &submenu.item.submenu,
                        self.services.context_key_service.clone(),
                        Some(MenuCreateOptions { emit_events_for_submenu_changes: true, event_debounce_delay: None }),
                    );
                    self.main_menu_disposables.add(menu.clone());
                    let entry = TopLevelMenu { key: title.original.clone(), menu, title: title.mnemonic_title.clone().unwrap_or_else(|| title.value.clone()) };
                    match top_level.iter_mut().find(|existing| existing.key == entry.key) {
                        Some(existing) => *existing = entry,
                        None => top_level.push(entry),
                    }
                }
            }
        }
        *self.top_level.borrow_mut() = top_level;
    }

    fn update_menubar(&self) {
        self.menu_updater.schedule(None);
    }

    /// `calculateActionLabel`: o rótulo não é alterado por nenhuma ação.
    pub fn calculate_action_label(&self, action: &dyn IAction) -> String {
        action.label()
    }

    /// `onUpdateStateChange`, com a guarda `visible` da sobrescrita.
    fn on_update_state_change(&self) {
        if !self.visible.get() {
            return;
        }
        self.update_menubar();
    }

    /// `onUpdateKeybindings`, com a guarda `visible` da sobrescrita.
    pub fn on_update_keybindings(&self) {
        if !self.visible.get() {
            return;
        }
        self.update_menubar();
    }

    fn get_open_recent_actions(&self) -> Vec<ActionRef> {
        let recently_opened = self.recently_opened.borrow().clone();
        let mut result: Vec<ActionRef> = Vec::new();

        if !recently_opened.workspaces.is_empty() {
            for recent in recently_opened.workspaces.iter().take(MAX_MENU_RECENT_ENTRIES) {
                let recent = match recent {
                    RecentWorkspaceOrFolder::Workspace(workspace) => Recent::Workspace(workspace.clone()),
                    RecentWorkspaceOrFolder::Folder(folder) => Recent::Folder(folder.clone()),
                };
                result.push(self.create_open_recent_menu_action(&recent).action);
            }
            result.push(Separator::new());
        }

        if !recently_opened.files.is_empty() {
            for file in recently_opened.files.iter().take(MAX_MENU_RECENT_ENTRIES) {
                result.push(self.create_open_recent_menu_action(&Recent::File(file.clone())).action);
            }
            result.push(Separator::new());
        }

        result
    }

    fn menubar_hidden(&self) -> bool {
        let host = &self.services.title_bar_host;
        if host.is_macintosh() && host.is_native() {
            return false;
        }
        self.current_menubar_visibility() == Some(MenuBarVisibility::Hidden)
    }

    fn on_did_change_window_focus(&self, has_focus: bool) {
        if !self.visible.get() {
            return;
        }
        // When we regain focus, update the recent menu items
        if has_focus {
            self.on_did_change_recently_opened();
        }
        let Some((ui, node)) = self.container.borrow().as_ref().and_then(|container| container.target()) else {
            return;
        };
        {
            let mut tree = ui.tree_mut();
            if has_focus {
                tree.remove_class(node, "inactive");
            } else {
                tree.add_class(node, "inactive");
            }
        }
        if !has_focus {
            let menubar = self.menubar.borrow().clone();
            if let Some(menubar) = menubar {
                menubar.blur();
            }
        }
    }

    fn on_configuration_updated(&self, event: &ConfigurationChangeEvent) {
        if KEYS.iter().any(|key| event.affects_configuration(*key, None)) {
            self.update_menubar();
        }

        if event.affects_configuration("editor.accessibilitySupport", None) {
            self.notify_user_of_custom_menubar_accessibility();
        }

        // Since we try not update when hidden, we should
        // try to update the recently opened list on visibility changes
        if event.affects_configuration(MENU_BAR_VISIBILITY_SETTING, None) {
            self.on_did_change_recently_opened();
        }
    }

    fn on_did_change_recently_opened(&self) {
        if !self.visible.get() {
            return;
        }
        // Do not update recently opened when the menubar is hidden #108712
        if !self.menubar_hidden() {
            let weak = self.weak_self.clone();
            self.services.workspaces_service.get_recently_opened().then(move |recently_opened| {
                if let Some(this) = weak.upgrade() {
                    *this.recently_opened.borrow_mut() = recently_opened;
                    this.update_menubar();
                }
            });
        }
    }

    fn create_open_recent_menu_action(&self, recent: &Recent) -> OpenRecentMenuAction {
        let non_empty = |label: &Option<String>| label.clone().filter(|label| !label.is_empty());
        let long = WorkspaceLabelOptions { verbose: Verbosity::Long };
        let labels = &self.services.label_service;

        let (uri, label, command_id, openable, remote_authority) = match recent {
            Recent::Folder(folder) => {
                let uri = folder.folder_uri.clone();
                let label = non_empty(&folder.label).unwrap_or_else(|| labels.get_workspace_label(&WorkspaceLabelTarget::Uri(uri.clone()), Some(&long)));
                (uri.clone(), label, "openRecentFolder", WindowOpenable::Folder { folder_uri: uri, label: None }, folder.remote_authority.clone())
            }
            Recent::Workspace(workspace) => {
                let uri = workspace.workspace.config_path.clone();
                let label = non_empty(&workspace.label).unwrap_or_else(|| labels.get_workspace_label(&WorkspaceLabelTarget::Workspace(workspace.workspace.clone()), Some(&long)));
                (uri.clone(), label, "openRecentWorkspace", WindowOpenable::Workspace { workspace_uri: uri, label: None }, workspace.remote_authority.clone())
            }
            Recent::File(file) => {
                let uri = file.file_uri.clone();
                let label = non_empty(&file.label).unwrap_or_else(|| labels.get_uri_label(&uri, &UriLabelOptions { append_workspace_suffix: Some(true), ..UriLabelOptions::default() }));
                (uri.clone(), label, "openRecentFile", WindowOpenable::File { file_uri: uri, label: None }, file.remote_authority.clone())
            }
        };

        let units: Vec<u16> = label.encode_utf16().collect();
        let label = unmnemonic_label(&truncate_middle(&units, MAX_MENU_RECENT_LABEL_LENGTH, &ELLIPSIS).to_string_lossy());

        let host = self.services.host_service.clone();
        let is_macintosh = self.services.platform.is_macintosh;
        let run_authority = remote_authority.clone().filter(|authority| !authority.is_empty());
        let action = to_action(ToActionProps {
            id: command_id.to_owned(),
            label,
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |args: &[Dynamic]| {
                let force_new_window = args.first().is_some_and(|event| open_in_new_window(event, is_macintosh));
                let options = OpenWindowOptions {
                    force_new_window: Some(force_new_window),
                    // local window if remoteAuthority is not set or can not be deducted from the openable
                    remote_authority: Some(run_authority.clone()),
                    ..OpenWindowOptions::default()
                };
                MaybePromise::Promise(host.open_window(vec![openable.clone()], Some(options)).then(|_| Dynamic::Undefined))
            }),
        });

        OpenRecentMenuAction { action, uri, remote_authority }
    }

    fn notify_user_of_custom_menubar_accessibility(&self) {
        if self.services.platform.is_web || self.services.platform.is_macintosh {
            return;
        }
        let s = &self.services;
        let has_been_notified = s.storage_service.get_boolean("menubar/accessibleMenubarNotified", StorageScope::Application, Some(false)).unwrap_or(false);
        let using_custom_menubar = !s.title_bar_host.has_native_menu();

        if has_been_notified || using_custom_menubar || !s.accessibility_service.is_screen_reader_optimized() {
            return;
        }

        let message = localize("menubar.customTitlebarAccessibilityNotification", "Accessibility support is enabled for you. For the most accessible experience, we recommend the custom menu style.", &[]);
        let preferences = s.preferences_service.clone();
        s.notification_service.prompt(
            Severity::Info,
            NotificationMessage::Text(message),
            vec![PromptChoiceKind::Plain(PromptChoice {
                base: BasePromptChoice {
                    telemetry_id: None,
                    label: localize("goToSetting", "Open Settings", &[]),
                    keep_open: false,
                    run: Rc::new(move || {
                        preferences.open_user_settings(Some(&IOpenSettingsOptions { query: Some(MENU_STYLE_SETTING.to_owned()), ..IOpenSettingsOptions::default() }));
                    }),
                },
                is_secondary: false,
            })],
            None,
        );

        s.storage_service.store("menubar/accessibleMenubarNotified", true.into(), StorageScope::Application, StorageTarget::User);
    }

    // #endregion

    // #region CustomMenubarControl

    fn do_update_menubar(&self, first_time: bool) {
        if !self.focus_inside_menubar.get() {
            self.setup_custom_menubar(first_time);
        }

        if first_time {
            self.pending_first_time_update.set(true);
        }
    }

    fn get_update_action(&self) -> Option<ActionRef> {
        let service = self.services.update_service.clone();
        let make = |id: &str, label: String, run: Option<Rc<dyn Fn()>>| -> ActionRef {
            to_action(ToActionProps {
                id: id.to_owned(),
                label,
                tooltip: None,
                enabled: Some(run.is_some()),
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    if let Some(run) = &run {
                        run();
                    }
                    MaybePromise::Value(Dynamic::Undefined)
                }),
            })
        };

        match service.state() {
            State::Idle(_) => Some(make("update.check", localize("checkForUpdates", "Check for &&Updates...", &[]), Some(Rc::new(move || {
                service.check_for_updates(true);
            })))),
            State::CheckingForUpdates(_) => Some(make("update.checking", localize("checkingForUpdates", "Checking for Updates...", &[]), None)),
            State::AvailableForDownload(_) => Some(make("update.downloadNow", localize("download now", "D&&ownload Update", &[]), Some(Rc::new(move || {
                service.download_update(true);
            })))),
            State::Downloading(_) | State::Overwriting(_) => Some(make("update.downloading", localize("DownloadingUpdate", "Downloading Update...", &[]), None)),
            State::Downloaded(_) => {
                if self.services.platform.is_macintosh {
                    None
                } else {
                    Some(make("update.install", localize("installUpdate...", "Install &&Update...", &[]), Some(Rc::new(move || {
                        service.apply_update();
                    }))))
                }
            }
            State::Updating(_) => Some(make("update.updating", localize("installingUpdate", "Installing Update...", &[]), None)),
            State::Cancelling => Some(make("update.cancelling", localize("cancellingUpdate", "Cancelling Update...", &[]), None)),
            State::Ready(_) => Some(make("update.restart", localize("restartToUpdate", "Restart to &&Update", &[]), Some(Rc::new(move || {
                service.quit_and_install();
            })))),
            _ => None,
        }
    }

    fn current_menubar_visibility(&self) -> Option<MenuBarVisibility> {
        self.services.title_bar_host.get_menu_bar_visibility().as_deref().and_then(MenuBarVisibility::parse)
    }

    fn current_disable_menu_bar_alt_focus(&self) -> bool {
        // `typeof settingValue === 'boolean'` ? `!settingValue` : false
        match self.services.configuration_service.get_value(Some("window.customMenuBarAltFocus"), None) {
            Dynamic::Bool(value) => !value,
            _ => false,
        }
    }

    fn insert_actions_before(&self, next_action_id: &str, target: &mut Vec<ActionRef>) {
        match next_action_id {
            OPEN_RECENT_ACTION_ID => target.extend(self.get_open_recent_actions()),
            "workbench.action.showAboutDialog" => {
                if !self.services.platform.is_macintosh && !self.services.platform.is_web && let Some(update_action) = self.get_update_action() {
                    update_action.set_label(mnemonic_menu_label(&update_action.label(), false, &self.services.platform));
                    target.push(update_action);
                    target.push(Separator::new());
                }
            }
            _ => {}
        }
    }

    fn current_enable_menu_bar_mnemonics(&self) -> bool {
        let enabled = match self.services.configuration_service.get_value(Some("window.enableMenuBarMnemonics"), None) {
            Dynamic::Bool(value) => value,
            _ => true,
        };
        enabled && (!self.services.platform.is_web || self.services.title_bar_host.is_fullscreen(MAIN_WINDOW))
    }

    fn current_compact_menu_mode(&self) -> Option<IMenuDirection> {
        if self.current_menubar_visibility() != Some(MenuBarVisibility::Compact) {
            return None;
        }
        let configuration = &self.services.configuration_service;
        let is = |key: &str, value: &str| configuration.get_value(Some(key), None).strict_equals(&Dynamic::from(value));

        // Menu bar lives in activity bar and should flow based on its location
        let horizontal = if is("workbench.sideBar.location", "right") { HorizontalDirection::Left } else { HorizontalDirection::Right };
        let vertical = if is("workbench.activityBar.location", ActivityBarPosition::Bottom.as_str()) { VerticalDirection::Above } else { VerticalDirection::Below };
        Some(IMenuDirection { horizontal, vertical })
    }

    fn on_did_visibility_change(&self, visible: bool) {
        self.visible.set(visible);
        self.on_did_change_recently_opened();
        self.on_visibility_change.fire(visible);
    }

    /// `updateActions`: reconstrói `target` a partir dos nós do menu.
    fn update_actions(&self, nodes: Vec<(String, MenuNode<'_>)>, target: &mut Vec<ActionRef>, top_level_title: &str, store: &Rc<DisposableStore>) {
        target.clear();
        let platform = &self.services.platform;

        for (id, node) in nodes {
            self.insert_actions_before(&id, target);

            match node {
                MenuNode::Separator => target.push(Separator::new()),
                MenuNode::Submenu(submenu) => {
                    // use mnemonicTitle whenever possible
                    let title = title_text(&submenu.item.title);
                    let children: Vec<(String, MenuNode<'_>)> = submenu.actions().iter().map(classify).collect();
                    let mut submenu_actions: Vec<ActionRef> = Vec::new();
                    self.update_actions(children, &mut submenu_actions, top_level_title, store);

                    if !submenu_actions.is_empty() {
                        target.push(SubmenuAction::new(&id, &mnemonic_menu_label(&title, false, platform), submenu_actions, None));
                    }
                }
                MenuNode::Item(item) => {
                    let mut title = title_text(&item.item.title);
                    if let Some(Toggled::Info(info)) = &item.item.toggled {
                        title = info.mnemonic_title.clone().or_else(|| info.title.clone()).unwrap_or(title);
                    }

                    let command_service = self.services.command_service.clone();
                    let command_id = id.clone();
                    let callback: ActionCallback = Rc::new(move |_: Dynamic| MaybePromise::Promise(command_service.execute_command(&command_id, Vec::new())));
                    let new_action = store.add_rc(Action::new(&id, &mnemonic_menu_label(&title, false, platform), &item.class().unwrap_or_default(), item.enabled(), Some(callback)));
                    new_action.set_tooltip(item.tooltip());
                    new_action.set_checked(item.checked());
                    target.push(new_action);
                }
                MenuNode::Other => {}
            }
        }

        // Append web navigation menu items to the file menu when not compact
        if top_level_title == "File" && self.current_compact_menu_mode().is_none() {
            target.extend(self.get_web_navigation_actions());
        }
    }

    /// `updateActions(this.toActionsArray(menu), target, key, store)`.
    fn fill_menu_actions(&self, menu: &Rc<dyn IMenu>, target: &mut Vec<ActionRef>, key: &str, store: &Rc<DisposableStore>) {
        // `toActionsArray`: getFlatContextMenuActions(menu.getActions({ shouldForwardArgs: true }))
        let groups = menu.get_actions(Some(&MenuActionOptions { should_forward_args: true, ..MenuActionOptions::default() }));
        let entries = get_flat_context_menu_entries(&groups, None);
        self.update_actions(entries.iter().map(entry_node).collect(), target, key, store);
    }

    /// O corpo do `onDidChange` de um menu de topo: refaz as ações e atualiza só aquele menu.
    fn refresh_top_level_menu(&self, key: &str, menu: &Rc<dyn IMenu>, store: &Rc<DisposableStore>) {
        let mut actions: Vec<ActionRef> = Vec::new();
        self.fill_menu_actions(menu, &mut actions, key, store);
        let title = self.top_level.borrow().iter().find(|entry| entry.key == key).map(|entry| entry.title.clone()).unwrap_or_default();
        let menubar = self.menubar.borrow().clone();
        if let Some(menubar) = menubar {
            menubar.update_menu(&MenuBarMenu { actions, label: mnemonic_menu_label(&title, false, &self.services.platform) });
        }
    }

    /// Liga `menu.onDidChange` a [`CustomMenubarControl::refresh_top_level_menu`] com o seu `DisposableStore`.
    fn watch_top_level_menu(&self, key: &str, source: &MenuSource) {
        let changed = DisposableStore::new();
        self.reinstall_disposables.add_rc(changed.clone());
        let (weak, key, menu) = (self.weak_self.clone(), key.to_owned(), source.menu.clone());
        self.reinstall_disposables.add(source.watched.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade()
                && !this.focus_inside_menubar.get()
            {
                changed.clear();
                this.refresh_top_level_menu(&key, &menu, &changed);
            }
        }));
    }

    fn setup_custom_menubar(&self, first_time: bool) {
        // If there is no container, we cannot setup the menubar
        let Some(container) = self.container.borrow().clone() else {
            return;
        };
        let Some((ui, node)) = container.target() else {
            return;
        };

        if first_time {
            // Reset and create new menubar
            if self.menubar.borrow().is_some() {
                self.reinstall_disposables.clear();
            }

            let menubar = self.reinstall_disposables.add_rc(MenuBar::new(&ui, node, self.get_menu_bar_options(), (self.services.styles)().0));
            *self.menubar.borrow_mut() = Some(menubar.clone());

            let weak = self.weak_self.clone();
            self.services.accessibility_service.always_underline_access_keys().then(move |value| {
                if let Some(this) = weak.upgrade() {
                    this.always_on_mnemonics.set(value);
                    let menubar = this.menubar.borrow().clone();
                    if let Some(menubar) = menubar {
                        menubar.update(Some(this.get_menu_bar_options()));
                    }
                }
            });

            let weak = self.weak_self.clone();
            self.reinstall_disposables.add(menubar.on_focus_state_change().listen(move |focused| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if *focused {
                    // `FOCUS_IN` do contêiner: para as atualizações antes do foco, para as chaves de contexto funcionarem
                    this.focus_inside_menubar.set(true);
                }
                this.on_focus_state_change.fire(*focused);

                // When the menubar loses focus, update it to clear any pending updates
                if !*focused {
                    if this.pending_first_time_update.get() {
                        this.setup_custom_menubar(true);
                        this.pending_first_time_update.set(false);
                    } else {
                        this.update_menubar();
                    }

                    this.focus_inside_menubar.set(false);
                }
            }));

            let weak = self.weak_self.clone();
            self.reinstall_disposables.add(menubar.on_visibility_change().listen(move |visible| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_visibility_change(*visible);
                }
            }));

            // Fire visibility change for the first install if menu is shown
            if menubar.is_visible() {
                self.on_did_visibility_change(true);
            }
        } else {
            let menubar = self.menubar.borrow().clone();
            if let Some(menubar) = menubar {
                menubar.update(Some(self.get_menu_bar_options()));
            }
        }

        // Update the menu actions
        let top_level: Vec<(String, Rc<dyn IMenu>, String)> = self.top_level.borrow().iter().map(|entry| (entry.key.clone(), entry.menu.clone(), entry.title.clone())).collect();
        for (key, menu, title) in top_level {
            if first_time {
                self.watch_top_level_menu(&key, &MenuSource { menu: menu.clone(), watched: menu.clone() });

                // For the file menu, we need to update if the web nav menu updates as well
                if key == "File" {
                    self.watch_top_level_menu(&key, &MenuSource { menu: menu.clone(), watched: self.web_navigation_menu.clone() });
                }
            }

            let mut actions: Vec<ActionRef> = Vec::new();
            self.update_actions_disposables.clear();
            self.fill_menu_actions(&menu, &mut actions, &key, &self.update_actions_disposables);

            let menubar = self.menubar.borrow().clone();
            if let Some(menubar) = menubar {
                let entry = MenuBarMenu { actions, label: mnemonic_menu_label(&title, false, &self.services.platform) };
                if first_time {
                    menubar.push(&[entry]);
                } else {
                    menubar.update_menu(&entry);
                }
            }
        }
    }

    fn get_web_navigation_actions(&self) -> Vec<ActionRef> {
        if !self.services.platform.is_web {
            return Vec::new(); // only for web
        }

        let mut web_navigation_actions: Vec<ActionRef> = Vec::new();
        for (_, actions) in self.web_navigation_menu.get_actions(None) {
            for action in actions {
                if let MenuAction::Item(action) = action {
                    let title = title_text(&action.item.title);
                    let command_service = self.services.command_service.clone();
                    let id = action.id();
                    web_navigation_actions.push(to_action(ToActionProps {
                        id: id.clone(),
                        label: mnemonic_menu_label(&title, false, &self.services.platform),
                        tooltip: None,
                        enabled: Some(action.enabled()),
                        checked: None,
                        class: action.class(),
                        alt: None,
                        run: Rc::new(move |args: &[Dynamic]| {
                            command_service.execute_command(&id, vec![args.first().cloned().unwrap_or(Dynamic::Undefined)]);
                            MaybePromise::Value(Dynamic::Undefined)
                        }),
                    }));
                }
            }

            web_navigation_actions.push(Separator::new());
        }

        if !web_navigation_actions.is_empty() {
            web_navigation_actions.pop();
        }

        web_navigation_actions
    }

    fn get_menu_bar_options(&self) -> IMenuBarOptions {
        let keybinding_service = self.services.keybinding_service.clone();
        let host = self.services.title_bar_host.clone();
        let weak = self.weak_self.clone();
        let action_runner: Rc<dyn IActionRunner> = self.action_runner.clone();
        IMenuBarOptions {
            enable_mnemonics: self.current_enable_menu_bar_mnemonics(),
            disable_alt_focus: self.current_disable_menu_bar_alt_focus(),
            visibility: self.services.title_bar_host.get_menu_bar_visibility(),
            action_runner: Some(action_runner),
            get_keybinding: Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false))),
            always_on_mnemonics: self.always_on_mnemonics.get(),
            compact_mode: self.current_compact_menu_mode(),
            get_compact_menu_actions: Some(Rc::new(move || weak.upgrade().map(|this| this.get_web_navigation_actions()).unwrap_or_default())),
            styles: Some(Rc::new((self.services.styles)().1)),
            is_fullscreen: Some(Rc::new(move || host.is_fullscreen(MAIN_WINDOW))),
        }
    }

    /// O `RESIZE` da janela principal: fecha o menu aberto.
    pub fn on_window_resize(&self) {
        let menubar = self.menubar.borrow().clone();
        if let Some(menubar) = menubar {
            menubar.blur();
        }
    }

    /// `onDidChangeFullscreen(windowId)`: no navegador os mnemônicos exigem tela cheia.
    pub fn on_did_change_fullscreen(&self, window_id: WindowId) {
        if self.services.platform.is_web && window_id == MAIN_WINDOW {
            self.update_menubar();
        }
    }

    /// `onVisibilityChange`.
    pub fn on_visibility_change(&self) -> je_base::event::Event<bool> {
        self.on_visibility_change.event()
    }

    /// `onFocusStateChange`.
    pub fn on_focus_state_change(&self) -> je_base::event::Event<bool> {
        self.on_focus_state_change.event()
    }

    /// `getMenubarItemsDimensions()`.
    pub fn get_menubar_items_dimensions(&self) -> Dimension {
        match self.menubar.borrow().as_ref() {
            Some(menubar) => Dimension { width: menubar.get_width(), height: menubar.get_height() },
            None => Dimension { width: 0.0, height: 0.0 },
        }
    }

    /// `layout(dimension)`.
    pub fn layout(&self, _dimension: &Dimension) {
        let menubar = self.menubar.borrow().clone();
        if let Some(menubar) = menubar {
            menubar.update(Some(self.get_menu_bar_options()));
        }
    }

    // #endregion
}

/// De onde um menu de topo escuta mudanças: o próprio menu, ou o menu de navegação da web para o menu `File`.
struct MenuSource {
    menu: Rc<dyn IMenu>,
    watched: Rc<dyn IMenu>,
}

impl ICustomMenubarControl for CustomMenubarControl {
    fn create(&self, parent: &Rc<MenuBarContainer>) {
        *self.container.borrow_mut() = Some(parent.clone());

        // Build the menubar
        self.do_update_menubar(true);
    }

    fn toggle_focus(&self) {
        let menubar = self.menubar.borrow().clone();
        if let Some(menubar) = menubar {
            menubar.toggle_focus();
        }
    }
}

impl IDisposable for CustomMenubarControl {
    fn dispose(&self) {
        self.base.dispose();
    }
}
