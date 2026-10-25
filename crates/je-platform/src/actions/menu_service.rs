//! Porte de `src/vs/platform/actions/common/menuService.ts`: o `MenuService`, o estado persistido de "esconder"
//! (`PersistedMenuHideState`), a ordenação e o agrupamento dos itens de menu, e o menu observável (`MenuImpl`).
//!
//! Decisões de tradução:
//! - `IMenuItemHide.isHidden` (um getter no original) é um `Rc<dyn Fn() -> bool>`, lido a cada consulta; a ação
//!   `toggle` é uma implementação de [`IAction`] cujo `checked` também é lido na hora.
//! - `localeCompare` é o [`Collator`] da locale padrão (o `Intl.Collator` do V8).
//! - Os eventos de mudança do `MenuImpl` são um [`DebounceEmitter`] com o mesmo `merge`; o ouvinte preguiçoso de
//!   contexto e de estado de esconder começa com o primeiro ouvinte e acaba com o último, como no original.
//! - O `MenuRegistry` guarda os itens por valor (não por referência, como os objetos do JavaScript): alterar um
//!   item depois de registrado não é observável, ao contrário do original.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::actions::{ActionRef, IAction, Separator, ToActionProps, to_action};
use je_base::arrays::remove_fast_without_keeping_order;
use je_base::async_::RunOnceScheduler;
use je_base::collator::locale_compare;
use je_base::dynamic::Dynamic;
use je_base::event::{DebounceEmitter, Emitter, EmitterOptions, Event};
use je_base::json::{self, Value};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::JsString;

use super::actions::{
    AnyMenuItem, IMenu, IMenuService, MenuAction, MenuActionGroup, MenuActionOptions, MenuChangeEvent, MenuCreateOptions, MenuId, MenuItemAction, MenuItemHide,
    SubmenuItemAction, menu_registry,
};
use crate::action::{Title, Toggled};
use crate::commands::ICommandService;
use crate::contextkey::{ContextKeyExpression, IContextKeyService};
use crate::keybinding::keybinding::IKeybindingService;
use crate::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

const HIDDEN_COMMANDS_KEY: &str = "menu.hiddenCommands";

/// O título como texto de exibição (`typeof title === 'string' ? title : title.value`).
fn title_value(title: &Title) -> String {
    match title {
        Title::Plain(text) => text.clone(),
        Title::Localized(localized) => localized.value.clone(),
    }
}

/// O título como texto de ordenação (`typeof title === 'string' ? title : title.original`).
fn title_original(title: &Title) -> String {
    match title {
        Title::Plain(text) => text.clone(),
        Title::Localized(localized) => localized.original.clone(),
    }
}

/// `MenuService`.
pub struct MenuService {
    base: Rc<Disposable>,
    command_service: Rc<dyn ICommandService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    hidden_states: Rc<PersistedMenuHideState>,
}

impl MenuService {
    pub fn new(command_service: Rc<dyn ICommandService>, keybinding_service: Rc<dyn IKeybindingService>, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let hidden_states = PersistedMenuHideState::new(storage_service);
        base.register(hidden_states.clone());
        Rc::new(Self { base, command_service, keybinding_service, hidden_states })
    }

    fn actions_context(&self) -> ActionsContext<'_> {
        ActionsContext { hidden_states: &self.hidden_states, command_service: &self.command_service, keybinding_service: &self.keybinding_service }
    }
}

impl IMenuService for MenuService {
    fn create_menu(&self, id: &MenuId, context_key_service: Rc<dyn IContextKeyService>, options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        let options = options.unwrap_or_default();
        // `{ emitEventsForSubmenuChanges: false, eventDebounceDelay: 50, ...options }`
        MenuImpl::new(
            id.clone(),
            self.hidden_states.clone(),
            options.emit_events_for_submenu_changes,
            options.event_debounce_delay.unwrap_or(50),
            self.command_service.clone(),
            self.keybinding_service.clone(),
            context_key_service,
        )
    }

    fn get_menu_actions(&self, id: &MenuId, context_key_service: &dyn IContextKeyService, options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        create_menu_action_groups(id, options, &self.actions_context(), context_key_service)
    }

    fn get_menu_contexts(&self, id: &MenuId) -> HashSet<String> {
        let info = MenuInfoSnapshot::new(id.clone(), false, false);
        let mut contexts = info.structure_context_keys.clone();
        contexts.extend(info.precondition_context_keys.iter().cloned());
        contexts.extend(info.toggled_context_keys.iter().cloned());
        contexts
    }

    fn reset_hidden_states(&self, menu_ids: Option<&[MenuId]>) {
        self.hidden_states.reset(menu_ids);
    }
}

impl IDisposable for MenuService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `PersistedMenuHideState`.
pub struct PersistedMenuHideState {
    storage_service: Rc<dyn StorageService>,
    disposables: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<()>>,
    ignore_change_event: Cell<bool>,
    /// `Record<string, string[] | undefined>`, na ordem de inserção.
    data: RefCell<Vec<(String, Vec<String>)>>,
    hidden_by_default_cache: RefCell<HashMap<String, bool>>,
}

/// `JSON.parse(raw)` do registro: uma entrada por menu, só as listas de texto.
fn parse_hidden_data(raw: &str) -> Option<Vec<(String, Vec<String>)>> {
    let mut errors = Vec::new();
    let value = json::parse_str(raw, &mut errors).filter(|_| errors.is_empty())?;
    let Value::Object(object) = value else { return None };
    Some(
        object
            .iter()
            .filter_map(|(key, value)| match value {
                Value::Array(items) => Some((
                    key.to_string_lossy(),
                    items.iter().filter_map(|item| if let Value::String(text) = item { Some(text.to_string_lossy()) } else { None }).collect(),
                )),
                _ => None,
            })
            .collect(),
    )
}

impl PersistedMenuHideState {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let raw = storage_service.get(HIDDEN_COMMANDS_KEY, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_owned());
        let data = parse_hidden_data(&raw).unwrap_or_default();
        let disposables = DisposableStore::new();
        let on_did_change = Emitter::new();
        let state = Rc::new(Self {
            storage_service: storage_service.clone(),
            disposables: disposables.clone(),
            on_did_change,
            ignore_change_event: Cell::new(false),
            data: RefCell::new(data),
            hidden_by_default_cache: RefCell::new(HashMap::new()),
        });
        let weak = Rc::downgrade(&state);
        let listener = storage_service.on_did_change_value(StorageScope::Profile, Some(HIDDEN_COMMANDS_KEY), &disposables).listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if !this.ignore_change_event.get() {
                let raw = this.storage_service.get(HIDDEN_COMMANDS_KEY, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_owned());
                if let Some(data) = parse_hidden_data(&raw) {
                    *this.data.borrow_mut() = data;
                }
            }
            this.on_did_change.fire(());
        });
        disposables.add(listener);
        state
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn is_hidden_by_default(&self, menu: &MenuId, command_id: &str) -> bool {
        self.hidden_by_default_cache.borrow().get(&format!("{}/{command_id}", menu.id)).copied().unwrap_or(false)
    }

    pub fn set_default_state(&self, menu: &MenuId, command_id: &str, hidden: bool) {
        self.hidden_by_default_cache.borrow_mut().insert(format!("{}/{command_id}", menu.id), hidden);
    }

    pub fn is_hidden(&self, menu: &MenuId, command_id: &str) -> bool {
        let hidden_by_default = self.is_hidden_by_default(menu, command_id);
        let state = self.data.borrow().iter().find(|(id, _)| *id == menu.id).is_some_and(|(_, entries)| entries.iter().any(|e| e == command_id));
        if hidden_by_default { !state } else { state }
    }

    pub fn update_hidden(&self, menu: &MenuId, command_id: &str, hidden: bool) {
        let hidden = if self.is_hidden_by_default(menu, command_id) { !hidden } else { hidden };
        {
            let mut data = self.data.borrow_mut();
            let position = data.iter().position(|(id, _)| *id == menu.id);
            if !hidden {
                // remove and cleanup
                if let Some(position) = position {
                    let entries = &mut data[position].1;
                    if let Some(index) = entries.iter().position(|e| e == command_id) {
                        remove_fast_without_keeping_order(entries, index);
                    }
                    if entries.is_empty() {
                        data.remove(position);
                    }
                }
            } else {
                // add unless already added
                match position {
                    None => data.push((menu.id.clone(), vec![command_id.to_owned()])),
                    Some(position) => {
                        let entries = &mut data[position].1;
                        if !entries.iter().any(|e| e == command_id) {
                            entries.push(command_id.to_owned());
                        }
                    }
                }
            }
        }
        self.persist();
    }

    pub fn reset(&self, menus: Option<&[MenuId]>) {
        match menus {
            // reset all
            None => self.data.borrow_mut().clear(),
            // reset only for a specific menu
            Some(menus) => self.data.borrow_mut().retain(|(id, _)| !menus.iter().any(|menu| menu.id == *id)),
        }
        self.persist();
    }

    fn persist(&self) {
        self.ignore_change_event.set(true);
        let mut object = json::Object::new();
        for (id, entries) in self.data.borrow().iter() {
            object.insert(JsString::from(id.as_str()), Value::Array(entries.iter().map(|e| Value::String(JsString::from(e.as_str()))).collect()));
        }
        let raw = json_stringify(&Value::Object(object), None);
        self.storage_service.store(HIDDEN_COMMANDS_KEY, StorageValue::String(raw), StorageScope::Profile, StorageTarget::User);
        self.ignore_change_event.set(false);
    }
}

impl IDisposable for PersistedMenuHideState {
    fn dispose(&self) {
        self.on_did_change.dispose();
        self.disposables.dispose();
    }
}

/// `IMenuItem | ISubmenuItem`.
type AnyItem = AnyMenuItem;
/// `[string, Array<IMenuItem | ISubmenuItem>]`.
type MenuItemGroup = (String, Vec<AnyItem>);

fn item_group(item: &AnyItem) -> &str {
    match item {
        AnyMenuItem::Menu(item) => item.group.as_deref().unwrap_or(""),
        AnyMenuItem::Submenu(item) => item.group.as_deref().unwrap_or(""),
    }
}

fn item_when(item: &AnyItem) -> Option<&ContextKeyExpression> {
    match item {
        AnyMenuItem::Menu(item) => item.when.as_ref(),
        AnyMenuItem::Submenu(item) => item.when.as_ref(),
    }
}

fn group_menu_items(menu_items: Vec<AnyItem>) -> Vec<MenuItemGroup> {
    let mut groups: Vec<MenuItemGroup> = Vec::new();
    for item in menu_items {
        match groups.last_mut() {
            Some((group, items)) if group == item_group(&item) => items.push(item),
            _ => groups.push((item_group(&item).to_owned(), vec![item])),
        }
    }
    groups
}

/// O que as funções de criação de ações compartilham do serviço.
struct ActionsContext<'a> {
    hidden_states: &'a Rc<PersistedMenuHideState>,
    command_service: &'a Rc<dyn ICommandService>,
    keybinding_service: &'a Rc<dyn IKeybindingService>,
}

fn create_menu_action_groups(id: &MenuId, options: Option<&MenuActionOptions>, context: &ActionsContext<'_>, context_key_service: &dyn IContextKeyService) -> Vec<MenuActionGroup> {
    let mut menu_items: Vec<AnyItem> =
        menu_registry().get_menu_items(id).into_iter().filter(|item| context_key_service.context_matches_rules(item_when(item))).collect();
    menu_items.sort_by(compare_menu_items);
    create_action_groups(id, group_menu_items(menu_items), options, context, context_key_service)
}

fn create_action_groups(
    menu_id: &MenuId,
    menu_groups: Vec<MenuItemGroup>,
    options: Option<&MenuActionOptions>,
    context: &ActionsContext<'_>,
    context_key_service: &dyn IContextKeyService,
) -> Vec<MenuActionGroup> {
    let mut result: Vec<MenuActionGroup> = Vec::new();

    for (id, items) in menu_groups {
        let mut active_actions: Vec<MenuAction> = Vec::new();
        for item in items {
            match item {
                AnyMenuItem::Menu(item) => {
                    context.hidden_states.set_default_state(menu_id, &item.command.id, item.is_hidden_by_default.unwrap_or(false));
                    let menu_hide = create_menu_hide(menu_id, &item.command.id, &title_value(&item.command.title), context.hidden_states);
                    let menu_keybinding =
                        create_configure_keybinding_action(context.command_service.clone(), context.keybinding_service.clone(), &item.command.id, item.when.clone(), true);
                    active_actions.push(MenuAction::Item(MenuItemAction::new(
                        item.command,
                        item.alt,
                        options.cloned(),
                        Some(menu_hide),
                        Some(menu_keybinding),
                        context_key_service,
                        context.command_service.clone(),
                    )));
                }
                AnyMenuItem::Submenu(item) => {
                    let menu_hide = create_menu_hide(menu_id, &item.submenu.id, &title_value(&item.title), context.hidden_states);
                    let groups = create_menu_action_groups(&item.submenu, options, context, context_key_service);
                    let lists: Vec<Vec<ActionRef>> = groups.iter().map(|(_, actions)| actions.iter().map(menu_action_as_action).collect()).collect();
                    let submenu_actions = Separator::join(&lists);
                    if !submenu_actions.is_empty() {
                        active_actions.push(MenuAction::Submenu(SubmenuItemAction::new(item, Some(menu_hide), submenu_actions)));
                    }
                }
            }
        }
        if !active_actions.is_empty() {
            result.push((id, active_actions));
        }
    }
    result
}

/// A [`MenuAction`] como [`IAction`] (o `Array<MenuItemAction | SubmenuItemAction>` do original já são ações).
pub fn menu_action_as_action(action: &MenuAction) -> ActionRef {
    match action {
        MenuAction::Item(item) => item.clone(),
        MenuAction::Submenu(submenu) => submenu.as_action(),
    }
}

/// `MenuInfoSnapshot`.
struct MenuInfoSnapshot {
    id: MenuId,
    collect_context_keys_for_submenus: bool,
    menu_groups: Vec<MenuItemGroup>,
    all_menu_ids: Vec<MenuId>,
    structure_context_keys: HashSet<String>,
    precondition_context_keys: HashSet<String>,
    toggled_context_keys: HashSet<String>,
}

impl MenuInfoSnapshot {
    fn new(id: MenuId, collect_context_keys_for_submenus: bool, sorted: bool) -> Self {
        let mut info = Self {
            id,
            collect_context_keys_for_submenus,
            menu_groups: Vec::new(),
            all_menu_ids: Vec::new(),
            structure_context_keys: HashSet::new(),
            precondition_context_keys: HashSet::new(),
            toggled_context_keys: HashSet::new(),
        };
        info.refresh(sorted);
        info
    }

    /// `refresh()`; `sorted` escolhe a ordenação de `MenuInfo` (o snapshot não ordena).
    fn refresh(&mut self, sorted: bool) {
        // reset
        self.all_menu_ids.clear();
        self.structure_context_keys.clear();
        self.precondition_context_keys.clear();
        self.toggled_context_keys.clear();

        let mut menu_items = menu_registry().get_menu_items(&self.id);
        if sorted {
            menu_items.sort_by(compare_menu_items);
        }
        self.menu_groups = group_menu_items(menu_items.clone());

        for item in &menu_items {
            // keep keys and submenu ids for eventing
            self.collect_context_keys_and_submenu_ids(item);
        }
        if !self.all_menu_ids.contains(&self.id) {
            self.all_menu_ids.push(self.id.clone());
        }
    }

    fn collect_context_keys_and_submenu_ids(&mut self, item: &AnyItem) {
        Self::fill_in_kb_expr_keys(item_when(item), &mut self.structure_context_keys);

        match item {
            AnyMenuItem::Menu(item) => {
                // keep precondition keys for event if applicable
                Self::fill_in_kb_expr_keys(item.command.precondition.as_ref(), &mut self.precondition_context_keys);
                // keep toggled keys for event if applicable
                if let Some(toggled) = &item.command.toggled {
                    let expression = match toggled {
                        Toggled::Info(info) => &info.condition,
                        Toggled::Expression(expression) => expression,
                    };
                    Self::fill_in_kb_expr_keys(Some(expression), &mut self.toggled_context_keys);
                }
            }
            AnyMenuItem::Submenu(item) if self.collect_context_keys_for_submenus => {
                // recursively collect context keys from submenus so that this
                // menu fires events when context key changes affect submenus
                for nested in menu_registry().get_menu_items(&item.submenu) {
                    self.collect_context_keys_and_submenu_ids(&nested);
                }
                if !self.all_menu_ids.contains(&item.submenu) {
                    self.all_menu_ids.push(item.submenu.clone());
                }
            }
            AnyMenuItem::Submenu(_) => {}
        }
    }

    fn fill_in_kb_expr_keys(exp: Option<&ContextKeyExpression>, set: &mut HashSet<String>) {
        if let Some(exp) = exp {
            set.extend(exp.keys());
        }
    }
}

/// `MenuInfo.compareMenuItems`.
fn compare_menu_items(a: &AnyItem, b: &AnyItem) -> Ordering {
    let (a_group, b_group) = (item_group(a), item_group(b));

    if a_group != b_group {
        // Falsy groups come last
        if a_group.is_empty() {
            return Ordering::Greater;
        } else if b_group.is_empty() {
            return Ordering::Less;
        }

        // 'navigation' group comes first
        if a_group == "navigation" {
            return Ordering::Less;
        } else if b_group == "navigation" {
            return Ordering::Greater;
        }

        // lexical sort for groups
        let value = locale_compare(a_group, b_group);
        if value != Ordering::Equal {
            return value;
        }
    }

    // sort on priority - default is 0
    let priority = |item: &AnyItem| {
        let order = match item {
            AnyMenuItem::Menu(item) => item.order,
            AnyMenuItem::Submenu(item) => item.order,
        };
        order.filter(|o| !o.is_nan()).unwrap_or(0.0)
    };
    let (a_priority, b_priority) = (priority(a), priority(b));
    if a_priority < b_priority {
        return Ordering::Less;
    } else if a_priority > b_priority {
        return Ordering::Greater;
    }

    // sort on titles
    let title = |item: &AnyItem| match item {
        AnyMenuItem::Menu(item) => title_original(&item.command.title),
        AnyMenuItem::Submenu(item) => title_original(&item.title),
    };
    locale_compare(&title(a), &title(b))
}

/// `MenuImpl`.
struct MenuImpl {
    base: Rc<Disposable>,
    menu_info: Rc<RefCell<MenuInfoSnapshot>>,
    hidden_states: Rc<PersistedMenuHideState>,
    command_service: Rc<dyn ICommandService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    context_key_service: Rc<dyn IContextKeyService>,
    on_did_change: Rc<DebounceEmitter<MenuChangeEvent>>,
}

impl MenuImpl {
    fn new(
        id: MenuId,
        hidden_states: Rc<PersistedMenuHideState>,
        emit_events_for_submenu_changes: bool,
        event_debounce_delay: u64,
        command_service: Rc<dyn ICommandService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let menu_info = Rc::new(RefCell::new(MenuInfoSnapshot::new(id, emit_events_for_submenu_changes, true)));

        // When context keys or storage state changes we need to check if the menu also has changed. However,
        // we only do that when someone listens on this menu because (1) these events are
        // firing often and (2) menu are often leaked
        let lazy_listener = DisposableStore::new();
        base.register(lazy_listener.clone());

        let merge = |events: Vec<MenuChangeEvent>| {
            let mut merged = MenuChangeEvent { is_structural_change: false, is_enablement_change: false, is_toggle_change: false };
            for item in events {
                merged.is_structural_change = merged.is_structural_change || item.is_structural_change;
                merged.is_enablement_change = merged.is_enablement_change || item.is_enablement_change;
                merged.is_toggle_change = merged.is_toggle_change || item.is_toggle_change;
                if merged.is_structural_change && merged.is_enablement_change && merged.is_toggle_change {
                    // everything is TRUE, no need to continue iterating
                    break;
                }
            }
            merged
        };

        // O emissor precisa do início e do fim do ouvinte preguiçoso, que por sua vez dispara o emissor: o ciclo é
        // fechado por um `Weak` preenchido depois da criação.
        let emitter_slot: Rc<RefCell<std::rc::Weak<DebounceEmitter<MenuChangeEvent>>>> = Rc::new(RefCell::new(std::rc::Weak::new()));
        let start_lazy_listener: Rc<dyn Fn(&Emitter<MenuChangeEvent>)> = {
            let (lazy_listener, menu_info, context_key_service, hidden_states, emitter_slot) =
                (lazy_listener.clone(), menu_info.clone(), context_key_service.clone(), hidden_states.clone(), emitter_slot.clone());
            Rc::new(move |_| {
                let fire = {
                    let emitter_slot = emitter_slot.clone();
                    move |event: MenuChangeEvent| {
                        if let Some(emitter) = emitter_slot.borrow().upgrade() {
                            emitter.fire(event);
                        }
                    }
                };
                let fire_for_context = fire.clone();
                let menu_info_for_context = menu_info.clone();
                lazy_listener.add(context_key_service.on_did_change_context().listen(move |e| {
                    let info = menu_info_for_context.borrow();
                    let is_structural_change = e.affects_some(&info.structure_context_keys);
                    let is_enablement_change = e.affects_some(&info.precondition_context_keys);
                    let is_toggle_change = e.affects_some(&info.toggled_context_keys);
                    drop(info);
                    if is_structural_change || is_enablement_change || is_toggle_change {
                        fire_for_context(MenuChangeEvent { is_structural_change, is_enablement_change, is_toggle_change });
                    }
                }));
                lazy_listener.add(hidden_states.on_did_change().listen(move |()| {
                    fire(MenuChangeEvent { is_structural_change: true, is_enablement_change: false, is_toggle_change: false });
                }));
            })
        };
        let stop_lazy_listener: Rc<dyn Fn(&Emitter<MenuChangeEvent>)> = {
            let lazy_listener = lazy_listener.clone();
            Rc::new(move |_| lazy_listener.clear())
        };
        let on_did_change = DebounceEmitter::with_options(
            EmitterOptions {
                on_will_add_first_listener: Some(start_lazy_listener),
                on_did_remove_last_listener: Some(stop_lazy_listener),
                ..EmitterOptions::default()
            },
            merge,
            Some(event_debounce_delay),
        );
        *emitter_slot.borrow_mut() = Rc::downgrade(&on_did_change);

        // Rebuild this menu whenever the menu registry reports an event for this MenuId.
        // This usually happen while code and extensions are loaded and affects the over
        // structure of the menu
        let rebuild_menu_soon = RunOnceScheduler::new(
            {
                let (menu_info, emitter_slot) = (menu_info.clone(), emitter_slot.clone());
                move || {
                    menu_info.borrow_mut().refresh(true);
                    if let Some(emitter) = emitter_slot.borrow().upgrade() {
                        emitter.fire(MenuChangeEvent { is_structural_change: true, is_enablement_change: true, is_toggle_change: true });
                    }
                }
            },
            event_debounce_delay,
        );
        base.register(rebuild_menu_soon.clone());
        base.register(menu_registry().on_did_change_menu().listen({
            let (menu_info, rebuild_menu_soon) = (menu_info.clone(), rebuild_menu_soon.clone());
            move |e| {
                if menu_info.borrow().all_menu_ids.iter().any(|id| e.has(id)) {
                    rebuild_menu_soon.schedule(None);
                }
            }
        }));

        Rc::new(Self { base, menu_info, hidden_states, command_service, keybinding_service, context_key_service, on_did_change })
    }
}

impl IMenu for MenuImpl {
    fn on_did_change(&self) -> Event<MenuChangeEvent> {
        self.on_did_change.event()
    }

    fn get_actions(&self, options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        let groups: Vec<MenuItemGroup> = self
            .menu_info
            .borrow()
            .menu_groups
            .iter()
            .map(|(id, items)| (id.clone(), items.iter().filter(|item| self.context_key_service.context_matches_rules(item_when(item))).cloned().collect()))
            .collect();
        let id = self.menu_info.borrow().id.clone();
        let context = ActionsContext { hidden_states: &self.hidden_states, command_service: &self.command_service, keybinding_service: &self.keybinding_service };
        create_action_groups(&id, groups, options, &context, &*self.context_key_service)
    }
}

impl IDisposable for MenuImpl {
    fn dispose(&self) {
        self.base.dispose();
        self.on_did_change.dispose();
    }
}

/// A ação `toggle` de `createMenuHide`: o `checked` é lido na hora, do estado persistido.
struct ToggleHideAction {
    id: String,
    label: RefCell<String>,
    tooltip: RefCell<String>,
    class: RefCell<Option<String>>,
    enabled: Cell<bool>,
    menu: MenuId,
    command_id: String,
    states: Rc<PersistedMenuHideState>,
}

impl IAction for ToggleHideAction {
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
        self.enabled.get()
    }
    fn set_enabled(&self, value: bool) {
        self.enabled.set(value);
    }
    fn checked(&self) -> Option<bool> {
        Some(!self.states.is_hidden(&self.menu, &self.command_id))
    }
    fn set_checked(&self, _value: Option<bool>) {}
    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        self.states.update_hidden(&self.menu, &self.command_id, self.checked().unwrap_or(false));
        Promise::resolved(Dynamic::Undefined)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `createMenuHide`: `id` é o do comando ou o do submenu, e `title` o texto de exibição.
fn create_menu_hide(menu: &MenuId, id: &str, title: &str, states: &Rc<PersistedMenuHideState>) -> MenuItemHide {
    let hide = to_action(ToActionProps {
        id: format!("hide/{}/{id}", menu.id),
        label: localize("hide.label", "Hide '{0}'", &[title]),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: {
            let (states, menu, id) = (states.clone(), menu.clone(), id.to_owned());
            Rc::new(move |_| {
                states.update_hidden(&menu, &id, true);
                MaybePromise::Value(Dynamic::Undefined)
            })
        },
    });

    let toggle = Rc::new(ToggleHideAction {
        id: format!("toggle/{}/{id}", menu.id),
        label: RefCell::new(title.to_owned()),
        tooltip: RefCell::new(title.to_owned()),
        class: RefCell::new(None),
        enabled: Cell::new(true),
        menu: menu.clone(),
        command_id: id.to_owned(),
        states: states.clone(),
    });

    let is_hidden: Rc<dyn Fn() -> bool> = {
        let toggle = toggle.clone();
        Rc::new(move || !toggle.checked().unwrap_or(false))
    };
    MenuItemHide { is_hidden, hide, toggle }
}

/// `createConfigureKeybindingAction`.
pub fn create_configure_keybinding_action(
    command_service: Rc<dyn ICommandService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    command_id: &str,
    when: Option<ContextKeyExpression>,
    enabled: bool,
) -> ActionRef {
    let command_id = command_id.to_owned();
    to_action(ToActionProps {
        id: format!("configureKeybinding/{command_id}"),
        label: localize("configure keybinding", "Configure Keybinding", &[]),
        tooltip: None,
        enabled: Some(enabled),
        checked: None,
        class: None,
        alt: None,
        run: {
            let command_id = command_id.clone();
            Rc::new(move |_| {
                // Only set the when clause when there is no keybinding
                // It is possible that the action and the keybinding have different when clauses
                let has_keybinding = keybinding_service.lookup_keybinding(&command_id, None, false).is_some(); // This may only be called inside the `run()` method as it can be expensive on startup. #210529
                let when_value = if !has_keybinding { when.as_ref().map(ContextKeyExpression::serialize) } else { None };
                let query = format!("@command:{command_id}{}", when_value.map(|w| format!(" +when:{w}")).unwrap_or_default());
                let _ = command_service.execute_command("workbench.action.openGlobalKeybindings", vec![Dynamic::string(&query)]);
                MaybePromise::Value(Dynamic::Undefined)
            })
        },
    })
}
