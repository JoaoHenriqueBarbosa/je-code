//! Porte de `src/vs/platform/actions/common/actions.ts`.
//!
//! Representação:
//! - `MenuId` é um valor comparado pelo identificador (no original, por identidade; os identificadores são únicos).
//!   Os embutidos (`MenuId.CommandPalette`...) são as funções de [`menu_ids`](super::menu_ids), registradas todas no
//!   primeiro uso, como o carregamento do módulo faz.
//! - `MenuItemAction` implementa [`IAction`] (e não estende `Action`, como o original).
//! - `Action2` é o trait [`Action2`]; `registerAction2(new X())` é [`register_action2`] com a instância.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::actions::{ActionRef, IAction, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Event, MicrotaskEmitter};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, dispose_array, mark_as_singleton, to_disposable};
use je_base::linked_list::LinkedList;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_instantiation::{ServicesAccessor, service_id};

use super::menu_ids::{self, BUILTIN_MENU_IDS};
use crate::action::{CommandAction, CommandActionTitle, Icon, LocalizedString, Title, Toggled};
use crate::commands::{Command, CommandHandler, CommandMetadata, CommandDescription, ICommandService, commands_registry};
use crate::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKeyService};
use crate::keybinding::keybindings_registry::{KeybindingRule, Keybindings, keybindings_registry};

/// `IMenuItem`.
#[derive(Clone)]
pub struct MenuItem {
    pub command: CommandAction,
    /// Comando alternativo para este item.
    pub alt: Option<CommandAction>,
    /// O item some se a expressão der falso.
    pub when: Option<ContextKeyExpression>,
    pub group: Option<String>,
    pub order: Option<f64>,
    pub is_hidden_by_default: Option<bool>,
}

impl MenuItem {
    pub fn new(command: CommandAction) -> Self {
        Self { command, alt: None, when: None, group: None, order: None, is_hidden_by_default: None }
    }
}

/// `isSplitButton`: `boolean | { togglePrimaryAction: true; primaryActionIds? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitButton {
    Plain(bool),
    TogglePrimaryAction { primary_action_ids: Option<Vec<String>> },
}

/// `ISubmenuItem`.
#[derive(Clone)]
pub struct SubmenuItem {
    pub title: Title,
    pub submenu: MenuId,
    pub icon: Option<Icon>,
    pub when: Option<ContextKeyExpression>,
    pub group: Option<String>,
    pub order: Option<f64>,
    pub is_selection: Option<bool>,
    /// Botão dividido: a primeira ação é a principal e o resto vai num menu.
    pub is_split_button: Option<SplitButton>,
}

/// `IMenuItem | ISubmenuItem`.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)] // valor de registro, copiado pouco; o `Box` complicaria o uso dos dois tipos
pub enum AnyMenuItem {
    Menu(MenuItem),
    Submenu(SubmenuItem),
}

/// `isIMenuItem`.
pub fn is_menu_item(item: &AnyMenuItem) -> bool {
    matches!(item, AnyMenuItem::Menu(_))
}

/// `isISubmenuItem`.
pub fn is_submenu_item(item: &AnyMenuItem) -> bool {
    matches!(item, AnyMenuItem::Submenu(_))
}

thread_local! {
    static MENU_IDS: RefCell<Option<HashMap<String, MenuId>>> = const { RefCell::new(None) };
}

fn with_menu_ids<R>(f: impl FnOnce(&mut HashMap<String, MenuId>) -> R) -> R {
    MENU_IDS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let map = guard.get_or_insert_with(|| BUILTIN_MENU_IDS.iter().map(|(_, ident)| ((*ident).to_string(), MenuId { id: (*ident).to_string() })).collect());
        f(map)
    })
}

/// `MenuId`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MenuId {
    pub id: String,
}

impl MenuId {
    /// `MenuId.for(identifier)`: cria ou reaproveita o `MenuId` com o identificador.
    pub fn for_id(identifier: &str) -> MenuId {
        with_menu_ids(|ids| ids.entry(identifier.to_string()).or_insert_with(|| MenuId { id: identifier.to_string() }).clone())
    }

    /// `new MenuId(identifier)`: lança `TypeError` se já existir um com o identificador (use `for_id` ou um identificador único).
    pub fn new(identifier: &str) -> MenuId {
        let exists = with_menu_ids(|ids| ids.contains_key(identifier));
        if exists {
            errors::throw(Error::type_error(format!("MenuId with identifier '{identifier}' already exists. Use MenuId.for(ident) or a unique identifier")));
        }
        MenuId::for_id(identifier)
    }
}

/// `IMenuActionOptions`.
#[derive(Clone, Default)]
pub struct MenuActionOptions {
    pub arg: Option<Dynamic>,
    pub args: Option<Vec<Dynamic>>,
    pub should_forward_args: bool,
    pub render_short_title: bool,
}

/// `IMenuChangeEvent`.
#[derive(Clone)]
pub struct MenuChangeEvent {
    pub is_structural_change: bool,
    pub is_toggle_change: bool,
    pub is_enablement_change: bool,
}

/// `MenuItemAction | SubmenuItemAction`.
#[derive(Clone)]
pub enum MenuAction {
    Item(Rc<MenuItemAction>),
    Submenu(Rc<SubmenuItemAction>),
}

/// Um grupo do menu: `[string, Array<MenuItemAction | SubmenuItemAction>]`.
pub type MenuActionGroup = (String, Vec<MenuAction>);

/// `IMenu`.
pub trait IMenu: IDisposable {
    fn on_did_change(&self) -> Event<MenuChangeEvent>;
    fn get_actions(&self, options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup>;
}

/// `IMenuData`.
pub struct MenuData {
    pub contexts: HashSet<String>,
    pub actions: Vec<MenuActionGroup>,
}

/// `IMenuCreateOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuCreateOptions {
    pub emit_events_for_submenu_changes: bool,
    pub event_debounce_delay: Option<u64>,
}

/// `IMenuService`.
pub trait IMenuService {
    /// Crie um menu para o identificador: ele avisa quando as entradas mudam (posição, habilitação, marcação).
    fn create_menu(&self, id: &MenuId, context_key_service: Rc<dyn IContextKeyService>, options: Option<MenuCreateOptions>) -> Rc<dyn IMenu>;
    /// As ações para o contexto atual, sem criar um menu.
    fn get_menu_actions(&self, id: &MenuId, context_key_service: &dyn IContextKeyService, options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup>;
    /// Os nomes dos contextos que o menu escuta.
    fn get_menu_contexts(&self, id: &MenuId) -> HashSet<String>;
    /// Reseta o estado de escondido de todos os itens (`None`) ou dos menus informados.
    fn reset_hidden_states(&self, menu_ids: Option<&[MenuId]>);
}

service_id!(pub I_MENU_SERVICE: dyn IMenuService = "menuService");

/// `IMenuRegistryChangeEvent`: o conjunto dos menus que mudaram.
#[derive(Clone, Debug, Default)]
pub struct MenuRegistryChangeEvent {
    ids: Rc<HashSet<MenuId>>,
}

impl MenuRegistryChangeEvent {
    fn for_id(id: &MenuId) -> Self {
        Self { ids: Rc::new(HashSet::from([id.clone()])) }
    }

    fn merge(events: Vec<MenuRegistryChangeEvent>) -> MenuRegistryChangeEvent {
        let mut ids = HashSet::new();
        for event in events {
            ids.extend(event.ids.iter().cloned());
        }
        MenuRegistryChangeEvent { ids: Rc::new(ids) }
    }

    /// `has(id)`.
    pub fn has(&self, id: &MenuId) -> bool {
        self.ids.contains(id)
    }
}

/// `MenuRegistry`.
pub struct MenuRegistry {
    commands: RefCell<Vec<(String, CommandAction)>>,
    menu_items: RefCell<HashMap<MenuId, LinkedList<AnyMenuItem>>>,
    on_did_change_menu: Rc<MicrotaskEmitter<MenuRegistryChangeEvent>>,
}

thread_local! {
    static MENU_REGISTRY: Rc<MenuRegistry> = {
        let merge: Rc<dyn Fn(Vec<MenuRegistryChangeEvent>) -> MenuRegistryChangeEvent> = Rc::new(MenuRegistryChangeEvent::merge);
        Rc::new(MenuRegistry {
            commands: RefCell::new(Vec::new()),
            menu_items: RefCell::new(HashMap::new()),
            on_did_change_menu: MicrotaskEmitter::with_options(Default::default(), Some(merge)),
        })
    };
}

/// `MenuRegistry`: o registro do thread.
pub fn menu_registry() -> Rc<MenuRegistry> {
    MENU_REGISTRY.with(Rc::clone)
}

impl MenuRegistry {
    /// `onDidChangeMenu`.
    pub fn on_did_change_menu(&self) -> Event<MenuRegistryChangeEvent> {
        self.on_did_change_menu.event()
    }

    /// `addCommand`.
    pub fn add_command(self: &Rc<Self>, command: CommandAction) -> DisposableRef {
        let id = command.id.clone();
        {
            let mut commands = self.commands.borrow_mut();
            match commands.iter_mut().find(|(k, _)| *k == id) {
                Some(entry) => entry.1 = command,
                None => commands.push((id.clone(), command)),
            }
        }
        self.on_did_change_menu.fire(MenuRegistryChangeEvent::for_id(&menu_ids::command_palette()));

        let this = Rc::clone(self);
        mark_as_singleton(to_disposable(move || {
            let removed = {
                let mut commands = this.commands.borrow_mut();
                let before = commands.len();
                commands.retain(|(k, _)| *k != id);
                commands.len() != before
            };
            if removed {
                this.on_did_change_menu.fire(MenuRegistryChangeEvent::for_id(&menu_ids::command_palette()));
            }
        }))
    }

    /// `getCommand`.
    pub fn get_command(&self, id: &str) -> Option<CommandAction> {
        self.commands.borrow().iter().find(|(k, _)| k == id).map(|(_, c)| c.clone())
    }

    /// `getCommands`.
    pub fn get_commands(&self) -> Vec<(String, CommandAction)> {
        self.commands.borrow().clone()
    }

    /// `appendMenuItem`.
    pub fn append_menu_item(self: &Rc<Self>, id: &MenuId, item: AnyMenuItem) -> DisposableRef {
        let removal = {
            let mut menu_items = self.menu_items.borrow_mut();
            menu_items.entry(id.clone()).or_default().push(item)
        };
        self.on_did_change_menu.fire(MenuRegistryChangeEvent::for_id(id));
        let this = Rc::clone(self);
        let id = id.clone();
        mark_as_singleton(to_disposable(move || {
            removal.remove();
            this.on_did_change_menu.fire(MenuRegistryChangeEvent::for_id(&id));
        }))
    }

    /// `appendMenuItems`.
    pub fn append_menu_items(self: &Rc<Self>, items: Vec<(MenuId, AnyMenuItem)>) -> DisposableRef {
        let result = DisposableStore::new();
        for (id, item) in items {
            result.add(self.append_menu_item(&id, item));
        }
        result
    }

    /// `getMenuItems`.
    pub fn get_menu_items(&self, id: &MenuId) -> Vec<AnyMenuItem> {
        let mut result: Vec<AnyMenuItem> = match self.menu_items.borrow().get(id) {
            Some(list) => list.iter().collect(),
            None => Vec::new(),
        };
        if *id == menu_ids::command_palette() {
            // CommandPalette is special because it shows all commands by default
            self.append_implicit_items(&mut result);
        }
        result
    }

    fn append_implicit_items(&self, result: &mut Vec<AnyMenuItem>) {
        let mut set: HashSet<String> = HashSet::new();
        for item in result.iter() {
            if let AnyMenuItem::Menu(item) = item {
                set.insert(item.command.id.clone());
                if let Some(alt) = &item.alt {
                    set.insert(alt.id.clone());
                }
            }
        }
        for (id, command) in self.commands.borrow().iter() {
            if !set.contains(id) {
                result.push(AnyMenuItem::Menu(MenuItem::new(command.clone())));
            }
        }
    }
}

/// `IMenuItemHide`.
#[derive(Clone)]
pub struct MenuItemHide {
    /// `isHidden`: lido na hora da consulta (o original é um getter).
    pub is_hidden: Rc<dyn Fn() -> bool>,
    pub hide: ActionRef,
    pub toggle: ActionRef,
}

/// `SubmenuItemAction`: uma `SubmenuAction` com o item e as ações de esconder.
pub struct SubmenuItemAction {
    base: Rc<SubmenuAction>,
    pub item: SubmenuItem,
    pub hide_actions: Option<MenuItemHide>,
}

impl SubmenuItemAction {
    pub fn new(item: SubmenuItem, hide_actions: Option<MenuItemHide>, actions: Vec<ActionRef>) -> Rc<Self> {
        let label = match &item.title {
            Title::Plain(text) => text.clone(),
            Title::Localized(title) => title.value.clone(),
        };
        let base = SubmenuAction::new(&format!("submenuitem.{}", item.submenu.id), &label, actions, Some("submenu"));
        Rc::new(Self { base, item, hide_actions })
    }

    /// A `SubmenuAction` por baixo, como `IAction`.
    pub fn as_action(&self) -> ActionRef {
        self.base.clone()
    }

    pub fn actions(&self) -> &[ActionRef] {
        self.base.actions()
    }
}

fn title_value(title: &Title) -> String {
    match title {
        Title::Plain(text) => text.clone(),
        Title::Localized(title) => title.value.clone(),
    }
}

/// `MenuItemAction`: implementa `IAction` e NÃO estende `Action`, para ninguém assinar os eventos de `Action` nem as
/// propriedades modificadas.
pub struct MenuItemAction {
    pub item: CommandAction,
    pub alt: Option<Rc<MenuItemAction>>,
    options: Option<MenuActionOptions>,
    pub hide_actions: Option<MenuItemHide>,
    pub menu_keybinding: Option<ActionRef>,
    id: String,
    label: RefCell<String>,
    tooltip: RefCell<String>,
    class: RefCell<Option<String>>,
    enabled: RefCell<bool>,
    checked: RefCell<Option<bool>>,
    command_service: Rc<dyn ICommandService>,
}

impl MenuItemAction {
    /// `MenuItemAction.label`.
    pub fn label_of(action: &CommandAction, options: Option<&MenuActionOptions>) -> String {
        match (&action.short_title, options.is_some_and(|o| o.render_short_title)) {
            (Some(short), true) => title_value(short),
            _ => title_value(&action.title),
        }
    }

    pub fn new(
        item: CommandAction,
        alt: Option<CommandAction>,
        options: Option<MenuActionOptions>,
        hide_actions: Option<MenuItemHide>,
        menu_keybinding: Option<ActionRef>,
        context_key_service: &dyn IContextKeyService,
        command_service: Rc<dyn ICommandService>,
    ) -> Rc<Self> {
        let mut label = Self::label_of(&item, options.as_ref());
        let mut tooltip = match &item.tooltip {
            Some(crate::action::Tooltip::Plain(text)) => text.clone(),
            Some(crate::action::Tooltip::Localized(localized)) => localized.value.clone(),
            None => String::new(),
        };
        let enabled = item.precondition.as_ref().is_none_or(|p| context_key_service.context_matches_rules(Some(p)));
        let mut checked: Option<bool> = None;

        let mut icon: Option<ThemeIcon> = None;

        if let Some(toggled) = &item.toggled {
            let (condition, toggled_icon, toggled_tooltip, toggled_title) = match toggled {
                Toggled::Info(info) => (info.condition.clone(), info.icon.clone(), info.tooltip.clone(), info.title.clone()),
                Toggled::Expression(expression) => (expression.clone(), None, None, None),
            };
            let is_checked = context_key_service.context_matches_rules(Some(&condition));
            checked = Some(is_checked);
            if is_checked && let Some(toggled_tooltip) = toggled_tooltip.filter(|t| !t.is_empty()) {
                tooltip = toggled_tooltip;
            }
            if is_checked && let Some(Icon::Theme(theme_icon)) = toggled_icon {
                icon = Some(theme_icon);
            }
            if is_checked && let Some(toggled_title) = toggled_title.filter(|t| !t.is_empty()) {
                label = toggled_title;
            }
        }

        if icon.is_none() {
            icon = match &item.icon {
                Some(Icon::Theme(theme_icon)) => Some(theme_icon.clone()),
                _ => None,
            };
        }

        let alt = alt.map(|alt| MenuItemAction::new(alt, None, options.clone(), hide_actions.clone(), None, context_key_service, command_service.clone()));
        Rc::new(Self {
            id: item.id.clone(),
            label: RefCell::new(label),
            tooltip: RefCell::new(tooltip),
            class: RefCell::new(icon.as_ref().map(ThemeIcon::as_class_name)),
            enabled: RefCell::new(enabled),
            checked: RefCell::new(checked),
            item,
            alt,
            options,
            hide_actions,
            menu_keybinding,
            command_service,
        })
    }
}

impl IAction for MenuItemAction {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn label(&self) -> String {
        self.label.borrow().clone()
    }
    fn set_label(&self, value: String) {
        *self.label.borrow_mut() = value;
    }
    fn tooltip(&self) -> String {
        self.tooltip.borrow().clone()
    }
    fn set_tooltip(&self, value: String) {
        *self.tooltip.borrow_mut() = value;
    }
    fn class(&self) -> Option<String> {
        self.class.borrow().clone()
    }
    fn set_class(&self, value: Option<String>) {
        *self.class.borrow_mut() = value;
    }
    fn enabled(&self) -> bool {
        *self.enabled.borrow()
    }
    fn set_enabled(&self, value: bool) {
        *self.enabled.borrow_mut() = value;
    }
    fn checked(&self) -> Option<bool> {
        *self.checked.borrow()
    }
    fn set_checked(&self, value: Option<bool>) {
        *self.checked.borrow_mut() = value;
    }
    fn alt(&self) -> Option<ActionRef> {
        self.alt.clone().map(|alt| alt as ActionRef)
    }
    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        let mut run_args: Vec<Dynamic> = Vec::new();

        if let Some(options_args) = self.options.as_ref().and_then(|o| o.args.clone()) {
            run_args.extend(options_args);
        } else if let Some(arg) = self.options.as_ref().and_then(|o| o.arg.clone()).filter(je_base::dynamic::Truthy::is_truthy) {
            run_args.push(arg);
        }

        if self.options.as_ref().is_some_and(|o| o.should_forward_args) {
            run_args.extend(args.iter().cloned());
        }

        self.command_service.execute_command(&self.id, run_args)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// #region --- IAction2

/// `{ id: MenuId; precondition?: null } & Omit<IMenuItem, 'command'>`: um item de menu da ação.
#[derive(Clone)]
pub struct Action2MenuItem {
    pub id: MenuId,
    /// `precondition: null`: o item não herda a pré-condição do comando.
    pub null_precondition: bool,
    pub alt: Option<CommandAction>,
    pub when: Option<ContextKeyExpression>,
    pub group: Option<String>,
    pub order: Option<f64>,
    pub is_hidden_by_default: Option<bool>,
}

/// `Omit<IKeybindingRule, 'id'>`.
#[derive(Clone)]
pub struct Action2Keybinding {
    pub keybindings: Keybindings,
    pub weight: i64,
    pub args: Dynamic,
    pub when: Option<ContextKeyExpression>,
}

/// `IAction2Options`: a descrição de um comando mais os menus, os atalhos e o `f1` (paleta de comandos).
#[derive(Clone)]
pub struct Action2Options {
    pub command: CommandAction,
    /// Um ou vários itens de menu.
    pub menu: Vec<Action2MenuItem>,
    /// Um ou vários atalhos.
    pub keybinding: Vec<Action2Keybinding>,
    /// Atalho para adicionar à paleta de comandos (o `title` precisa ser localizado).
    pub f1: bool,
}

/// `IAction2F1RequiredOptions`.
#[derive(Clone)]
pub struct Action2F1RequiredOptions {
    pub title: CommandActionTitle,
    pub category: Option<LocalizedString>,
}

/// `Action2`.
pub trait Action2 {
    fn desc(&self) -> Action2Options;
    fn run(&self, accessor: &ServicesAccessor, args: &[Dynamic]);
}

/// `registerAction2`.
pub fn register_action2(action: Rc<dyn Action2>) -> DisposableRef {
    let mut disposables: Vec<DisposableRef> = Vec::new(); // not using `DisposableStore` to reduce startup perf cost

    let Action2Options { command, menu, keybinding, f1 } = action.desc();

    let registry = commands_registry();
    if registry.get_command(&command.id).is_some() {
        errors::throw(Error::new(format!("Cannot register two commands with the same id: {}", command.id)));
    }

    // command
    let runner = action.clone();
    let handler: CommandHandler = Rc::new(move |accessor, args| {
        runner.run(accessor, args);
        Dynamic::Undefined
    });
    let metadata = command.metadata.clone().unwrap_or_else(|| {
        CommandMetadata::new(match &command.title {
            Title::Plain(text) => CommandDescription::Plain(text.clone()),
            Title::Localized(title) => CommandDescription::Localized(LocalizedString { value: title.value.clone(), original: title.original.clone() }),
        })
    });
    disposables.push(registry.register_command(Command { id: command.id.clone(), handler, metadata: Some(metadata) }));

    // menu
    let menus = menu_registry();
    for item in menu {
        let mut item_command = command.clone();
        if item.null_precondition {
            item_command.precondition = None;
        }
        disposables.push(menus.append_menu_item(
            &item.id,
            AnyMenuItem::Menu(MenuItem {
                command: item_command,
                alt: item.alt,
                when: item.when,
                group: item.group,
                order: item.order,
                is_hidden_by_default: item.is_hidden_by_default,
            }),
        ));
    }
    if f1 {
        disposables.push(menus.append_menu_item(
            &menu_ids::command_palette(),
            AnyMenuItem::Menu(MenuItem { when: command.precondition.clone(), ..MenuItem::new(command.clone()) }),
        ));
        disposables.push(menus.add_command(command.clone()));
    }

    // keybinding
    for item in keybinding {
        let when = match &command.precondition {
            Some(precondition) => ContextKeyExpr::and([Some(precondition.clone()), item.when.clone()]),
            None => item.when.clone(),
        };
        disposables.push(keybindings_registry().register_keybinding_rule(KeybindingRule {
            keybindings: item.keybindings,
            id: command.id.clone(),
            weight: item.weight,
            args: item.args,
            when,
        }));
    }

    to_disposable(move || {
        dispose_array(&disposables);
    })
}

// #endregion
