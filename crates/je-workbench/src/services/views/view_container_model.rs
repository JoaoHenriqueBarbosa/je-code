//! Porte de `src/vs/workbench/services/views/common/viewContainerModel.ts`: o `ViewContainerModel`, que mantém as views
//! de um contêiner (todas, ativas e visíveis), a ordem, o tamanho, o estado recolhido e a visibilidade, e persiste esse
//! estado (`ViewDescriptorsState`) no workspace e no perfil.
//!
//! Divergências:
//! - **Injeção.** `createInstance(ViewContainerModel, viewContainer)` e `createInstance(ViewDescriptorsState, storageId,
//!   name)` são [`Injectable`] com os argumentos estáticos como `Args` e os serviços como `Deps`.
//! - **Estado compartilhado.** O objeto `IViewDescriptorState` é o mesmo no mapa do `ViewDescriptorsState` e no item do
//!   modelo (o original muda um e lê o outro), então é um `Rc<RefCell<...>>`. `undefined` dos campos é `None`.
//! - **JSON.** `JSON.parse` e `JSON.stringify` do estado persistido são `serde_json` (com `preserve_order`, que mantém a
//!   ordem das chaves como o objeto do JavaScript). Os campos `undefined` (`size`, `order`) ficam fora do objeto, como o
//!   `JSON.stringify` os omite, e os números inteiros saem sem a parte decimal. Um JSON inválido lança como o
//!   `JSON.parse`, e a lista global que não é um array vira lista vazia (o `reduce` do original lançaria `TypeError`).
//! - **Ordenação.** `Array.prototype.sort` dos itens e dos eventos é [`sort_by_compare`], que segue o V8 (o comparador do
//!   original não é uma ordem total quando misturam views com e sem `group`).
//! - **Eventos de contexto.** `Event.filter(onDidChangeContext, e => e.affectsSome(this.contextKeys))` é o `listen` com o
//!   filtro dentro do ouvinte, e o `CounterSet` ganha o adaptador [`ContextKeySet`] para ser um `ReadableSet`.
//! - **Logger.** `Lazy<ILogger>` com `windowLogGroup` (`services/log/common/logConstants.ts`, ainda não portado) está em
//!   [`create_views_logger`]; os argumentos extras dos `trace` entram na própria mensagem.
//! - **`move`** é [`IViewContainerModel::move_`]. O `move` do original com um id desconhecido falha com `TypeError` ao
//!   ler `viewDescriptor` de `undefined`; aqui lança [`Error`] com a mesma mensagem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::{CompareResult, r#move, sort_by_compare};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::{CounterSet, OrderedMap};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::contextkey::{IContextKeyService, ReadableSet};
use je_platform::log::{LoggerGroup, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use serde_json::{Map, Value};

use crate::common::views::{
    ContainerInfoChange, IAddedViewDescriptorRef, IAddedViewDescriptorState, IViewContainerModel, IViewDescriptorRef, VIEWS_LOG_ID, ViewContainerRc,
    ViewDescriptorMove, ViewDescriptorRc, ViewDescriptorsChange, ViewIcon, ViewSize, default_view_icon, views_log_name, views_registry,
};

/// `getViewsStateStorageId(viewContainerStorageId)`.
pub fn get_views_state_storage_id(view_container_storage_id: &str) -> String {
    format!("{view_container_storage_id}.hidden")
}

/// `loggerService.createLogger(VIEWS_LOG_ID, { name: VIEWS_LOG_NAME, group: windowLogGroup })`.
pub(crate) fn create_views_logger(logger_service: &dyn LoggerService) -> LoggerRef {
    // `windowLogGroup`: `{ id: windowLogId, name: localize('window', "Window") }`, com `windowLogId = 'rendererLog'`.
    let group = LoggerGroup { id: "rendererLog".to_string(), name: localize("window", "Window", &[]) };
    logger_service.create_logger(
        ResourceOrId::from(VIEWS_LOG_ID),
        Some(LoggerOptions { name: Some(views_log_name()), group: Some(group), ..LoggerOptions::default() }),
    )
}

/// `JSON.parse(text)`: lança se o texto não é JSON.
pub(crate) fn parse_json(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(error) => errors::throw(Error::named("SyntaxError", error.to_string())),
    }
}

/// O número do JavaScript como valor JSON: inteiro sem a parte decimal, como o `JSON.stringify` o escreve.
fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// `IStoredWorkspaceViewState`, lido do objeto JSON (`isHidden`, `collapsed`, `size` e `order` podem faltar).
struct StoredWorkspaceViewState {
    collapsed: Option<bool>,
    is_hidden: Option<bool>,
    size: Option<f64>,
    order: Option<f64>,
}

impl StoredWorkspaceViewState {
    fn from_json(value: &Value) -> Self {
        Self {
            collapsed: value.get("collapsed").and_then(Value::as_bool),
            is_hidden: value.get("isHidden").and_then(Value::as_bool),
            size: value.get("size").and_then(Value::as_f64),
            order: value.get("order").and_then(Value::as_f64),
        }
    }
}

/// `IStoredGlobalViewState`.
#[derive(Clone)]
struct StoredGlobalViewState {
    id: String,
    is_hidden: bool,
    order: Option<f64>,
}

impl StoredGlobalViewState {
    fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("id".to_string(), Value::String(self.id.clone()));
        object.insert("isHidden".to_string(), Value::Bool(self.is_hidden));
        if let Some(order) = self.order {
            object.insert("order".to_string(), json_number(order));
        }
        Value::Object(object)
    }
}

/// `IViewDescriptorState`.
#[derive(Clone, Copy, Default)]
struct ViewDescriptorState {
    visible_global: Option<bool>,
    visible_workspace: Option<bool>,
    collapsed: Option<bool>,
    active: bool,
    order: Option<f64>,
    size: Option<f64>,
}

/// O `IViewDescriptorState` compartilhado entre o `ViewDescriptorsState` e o item do modelo.
type StateRc = Rc<RefCell<ViewDescriptorState>>;

/// O `{ id, visible }` de `onDidChangeStoredState`.
#[derive(Clone)]
struct StoredStateChange {
    id: String,
    visible: bool,
}

/// `ViewDescriptorsState`.
struct ViewDescriptorsState {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    this: Weak<ViewDescriptorsState>,
    workspace_views_state_storage_id: String,
    global_views_state_storage_id: String,
    state: RefCell<HashMap<String, StateRc>>,
    on_did_change_stored_state: Rc<Emitter<Vec<StoredStateChange>>>,
    logger: Lazy<LoggerRef>,
    view_container_name: String,
    storage_service: Rc<dyn StorageService>,
    /// `_globalViewsStatesValue`.
    global_views_states_value: RefCell<Option<String>>,
}

impl Injectable for ViewDescriptorsState {
    type Args = (String, String);
    type Deps = (Rc<dyn StorageService>, Rc<dyn LoggerService>);

    fn create((view_container_storage_id, view_container_name): Self::Args, (storage_service, logger_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change_stored_state = Emitter::new();
        base.register(on_did_change_stored_state.clone());
        let global_views_state_storage_id = get_views_state_storage_id(&view_container_storage_id);
        let this = Rc::new_cyclic(|this| Self {
            base,
            store,
            this: this.clone(),
            workspace_views_state_storage_id: view_container_storage_id,
            global_views_state_storage_id,
            state: RefCell::new(HashMap::new()),
            on_did_change_stored_state,
            logger: Lazy::new(move || create_views_logger(logger_service.as_ref())),
            view_container_name,
            storage_service,
            global_views_states_value: RefCell::new(None),
        });

        let weak = this.this.clone();
        this.base.register(this.storage_service.on_did_change_value(StorageScope::Profile, Some(this.global_views_state_storage_id.as_str()), &this.store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_change();
            }
        }));

        let state = this.initialize();
        *this.state.borrow_mut() = state;
        this
    }
}

impl IDisposable for ViewDescriptorsState {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ViewDescriptorsState {
    fn set(&self, id: &str, state: StateRc) {
        self.state.borrow_mut().insert(id.to_string(), state);
    }

    fn get(&self, id: &str) -> Option<StateRc> {
        self.state.borrow().get(id).cloned()
    }

    fn update_state(&self, view_descriptors: &[ViewDescriptorRc]) {
        self.update_workspace_state(view_descriptors);
        self.update_global_state(view_descriptors);
    }

    fn update_workspace_state(&self, view_descriptors: &[ViewDescriptorRc]) {
        let mut stored_views_states = self.get_stored_workspace_state();
        for view_descriptor in view_descriptors {
            if let Some(view_state) = self.get(&view_descriptor.id) {
                let view_state = *view_state.borrow();
                let mut stored = Map::new();
                stored.insert("collapsed".to_string(), Value::Bool(view_state.collapsed.unwrap_or(false)));
                stored.insert("isHidden".to_string(), Value::Bool(!view_state.visible_workspace.unwrap_or(false)));
                if let Some(size) = view_state.size {
                    stored.insert("size".to_string(), json_number(size));
                }
                if view_descriptor.workspace
                    && let Some(order) = view_state.order
                {
                    stored.insert("order".to_string(), json_number(order));
                }
                stored_views_states.insert(view_descriptor.id.clone(), Value::Object(stored));
            }
        }

        if !stored_views_states.is_empty() {
            self.storage_service.store(
                &self.workspace_views_state_storage_id,
                StorageValue::String(Value::Object(stored_views_states).to_string()),
                StorageScope::Workspace,
                StorageTarget::Machine,
            );
        } else {
            self.storage_service.remove(&self.workspace_views_state_storage_id, StorageScope::Workspace);
        }
    }

    fn update_global_state(&self, view_descriptors: &[ViewDescriptorRc]) {
        let mut stored_global_state = self.get_stored_global_state();
        for view_descriptor in view_descriptors {
            let state = self.get(&view_descriptor.id).map(|state| *state.borrow());
            stored_global_state.insert(
                view_descriptor.id.clone(),
                StoredGlobalViewState {
                    id: view_descriptor.id.clone(),
                    is_hidden: match state {
                        Some(state) if view_descriptor.can_toggle_visibility => !state.visible_global.unwrap_or(false),
                        _ => false,
                    },
                    order: match state {
                        Some(state) if !view_descriptor.workspace => state.order,
                        _ => None,
                    },
                },
            );
        }
        self.set_stored_global_state(&stored_global_state);
    }

    fn on_did_storage_change(&self) {
        if self.global_views_states_value() != self.get_stored_global_views_states_value() /* This checks if current window changed the value or not */ {
            *self.global_views_states_value.borrow_mut() = None;
            let stored_views_visibility_states = self.get_stored_global_state();
            let stored_workspace_views_states = self.get_stored_workspace_state();
            let mut changed_states: Vec<StoredStateChange> = Vec::new();
            for (id, stored_state) in stored_views_visibility_states.iter() {
                if let Some(state) = self.get(id) {
                    if state.borrow().visible_global != Some(!stored_state.is_hidden) {
                        if !stored_state.is_hidden {
                            self.logger.value().trace(&format!("View visibility state changed: {id} is now visible ({})", self.view_container_name));
                        }
                        changed_states.push(StoredStateChange { id: id.clone(), visible: !stored_state.is_hidden });
                    }
                } else {
                    let workspace_view_state = stored_workspace_views_states.get(id).map(StoredWorkspaceViewState::from_json);
                    self.set(
                        id,
                        Rc::new(RefCell::new(ViewDescriptorState {
                            active: false,
                            visible_global: Some(!stored_state.is_hidden),
                            visible_workspace: workspace_view_state.as_ref().and_then(|state| state.is_hidden).map(|is_hidden| !is_hidden),
                            collapsed: workspace_view_state.as_ref().and_then(|state| state.collapsed),
                            order: workspace_view_state.as_ref().and_then(|state| state.order),
                            size: workspace_view_state.as_ref().and_then(|state| state.size),
                        })),
                    );
                }
            }
            if !changed_states.is_empty() {
                self.on_did_change_stored_state.fire(changed_states.clone());
                // Update the in memory state after firing the event
                // so that the views can update their state accordingly
                for changed_state in changed_states {
                    if let Some(state) = self.get(&changed_state.id) {
                        state.borrow_mut().visible_global = Some(changed_state.visible);
                    }
                }
            }
        }
    }

    fn initialize(&self) -> HashMap<String, StateRc> {
        let mut view_states: HashMap<String, StateRc> = HashMap::new();
        let workspace_views_states = self.get_stored_workspace_state();
        for (id, value) in workspace_views_states.iter() {
            let workspace_view_state = StoredWorkspaceViewState::from_json(value);
            view_states.insert(
                id.clone(),
                Rc::new(RefCell::new(ViewDescriptorState {
                    active: false,
                    visible_global: None,
                    visible_workspace: workspace_view_state.is_hidden.map(|is_hidden| !is_hidden),
                    collapsed: workspace_view_state.collapsed,
                    order: workspace_view_state.order,
                    size: workspace_view_state.size,
                })),
            );
        }

        // Migrate to `viewletStateStorageId`
        let value = self.storage_service.get(&self.global_views_state_storage_id, StorageScope::Workspace, Some("[]")).unwrap_or_else(|| "[]".to_string());
        let (workspace_visibility_states, _) = Self::parse_stored_global_state(&value);
        if !workspace_visibility_states.is_empty() {
            for stored in workspace_visibility_states.values() {
                match view_states.get(&stored.id) {
                    // Not migrated to `viewletStateStorageId`
                    Some(view_state) => {
                        let mut view_state = view_state.borrow_mut();
                        if view_state.visible_workspace.is_none() {
                            view_state.visible_workspace = Some(!stored.is_hidden);
                        }
                    }
                    None => {
                        view_states.insert(
                            stored.id.clone(),
                            Rc::new(RefCell::new(ViewDescriptorState {
                                active: false,
                                collapsed: None,
                                visible_global: None,
                                visible_workspace: Some(!stored.is_hidden),
                                order: None,
                                size: None,
                            })),
                        );
                    }
                }
            }
            self.storage_service.remove(&self.global_views_state_storage_id, StorageScope::Workspace);
        }

        let (state, has_duplicates) = Self::parse_stored_global_state(&self.global_views_states_value());
        if has_duplicates {
            self.set_stored_global_state(&state);
        }
        for stored in state.values() {
            match view_states.get(&stored.id) {
                Some(view_state) => {
                    let mut view_state = view_state.borrow_mut();
                    view_state.visible_global = Some(!stored.is_hidden);
                    if stored.order.is_some() {
                        view_state.order = stored.order;
                    }
                }
                None => {
                    view_states.insert(
                        stored.id.clone(),
                        Rc::new(RefCell::new(ViewDescriptorState {
                            active: false,
                            visible_global: Some(!stored.is_hidden),
                            order: stored.order,
                            collapsed: None,
                            visible_workspace: None,
                            size: None,
                        })),
                    );
                }
            }
        }
        view_states
    }

    /// `getStoredWorkspaceState()`: o `IStringDictionary` do JSON guardado no workspace.
    fn get_stored_workspace_state(&self) -> Map<String, Value> {
        let value = self.storage_service.get(&self.workspace_views_state_storage_id, StorageScope::Workspace, Some("{}")).unwrap_or_else(|| "{}".to_string());
        match parse_json(&value) {
            Value::Object(object) => object,
            _ => Map::new(),
        }
    }

    fn get_stored_global_state(&self) -> OrderedMap<String, StoredGlobalViewState> {
        Self::parse_stored_global_state(&self.global_views_states_value()).0
    }

    fn set_stored_global_state(&self, stored_global_state: &OrderedMap<String, StoredGlobalViewState>) {
        let value = Value::Array(stored_global_state.values().map(StoredGlobalViewState::to_json).collect()).to_string();
        self.set_global_views_states_value(value);
    }

    fn parse_stored_global_state(value: &str) -> (OrderedMap<String, StoredGlobalViewState>, bool) {
        let stored_value = match parse_json(value) {
            Value::Array(items) => items,
            _ => Vec::new(),
        };
        let mut has_duplicates = false;
        let mut state: OrderedMap<String, StoredGlobalViewState> = OrderedMap::new();
        for stored_state in &stored_value {
            let stored = match stored_state {
                // migration
                Value::String(id) => StoredGlobalViewState { id: id.clone(), is_hidden: true, order: None },
                other => StoredGlobalViewState {
                    id: other.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                    is_hidden: other.get("isHidden").and_then(Value::as_bool).unwrap_or(false),
                    order: other.get("order").and_then(Value::as_f64),
                },
            };
            has_duplicates = has_duplicates || state.contains_key(&stored.id);
            state.insert(stored.id.clone(), stored);
        }
        (state, has_duplicates)
    }

    /// O getter `globalViewsStatesValue`.
    fn global_views_states_value(&self) -> String {
        let cached = self.global_views_states_value.borrow().clone();
        match cached {
            Some(value) if !value.is_empty() => value,
            _ => {
                let value = self.get_stored_global_views_states_value();
                *self.global_views_states_value.borrow_mut() = Some(value.clone());
                value
            }
        }
    }

    /// O setter `globalViewsStatesValue`.
    fn set_global_views_states_value(&self, global_views_states_value: String) {
        if self.global_views_states_value() != global_views_states_value {
            *self.global_views_states_value.borrow_mut() = Some(global_views_states_value.clone());
            self.set_stored_global_views_states_value(&global_views_states_value);
        }
    }

    fn get_stored_global_views_states_value(&self) -> String {
        self.storage_service.get(&self.global_views_state_storage_id, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_string())
    }

    fn set_stored_global_views_states_value(&self, value: &str) {
        self.storage_service.store(&self.global_views_state_storage_id, StorageValue::String(value.to_string()), StorageScope::Profile, StorageTarget::User);
    }
}

/// `IViewDescriptorItem`.
#[derive(Clone)]
struct ViewDescriptorItem {
    view_descriptor: ViewDescriptorRc,
    state: StateRc,
}

impl ViewDescriptorItem {
    /// `item1 === item2`.
    fn is_same(&self, other: &ViewDescriptorItem) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

/// O `{ index, visibleIndex, viewDescriptorItem }` de `find`, sem o `index`, que o original nunca lê.
struct FoundViewDescriptorItem {
    visible_index: usize,
    view_descriptor_item: ViewDescriptorItem,
}

/// O `CounterSet<string>` das chaves de contexto como `ReadableSet`.
struct ContextKeySet<'a>(&'a CounterSet<String>);

impl ReadableSet for ContextKeySet<'_> {
    fn has(&self, value: &str) -> bool {
        self.0.has(&value.to_string())
    }
}

/// `ViewContainerModel`.
pub struct ViewContainerModel {
    base: Rc<Disposable>,
    this: Weak<ViewContainerModel>,
    view_container: ViewContainerRc,
    context_key_service: Rc<dyn IContextKeyService>,
    context_keys: RefCell<CounterSet<String>>,
    view_descriptor_items: RefCell<Vec<ViewDescriptorItem>>,
    view_descriptors_state: Rc<ViewDescriptorsState>,

    // Container Info
    title: RefCell<Option<String>>,
    icon: RefCell<Option<ViewIcon>>,
    keybinding_id: RefCell<Option<String>>,
    on_did_change_container_info: Rc<Emitter<ContainerInfoChange>>,

    // All View Descriptors
    on_did_change_all_view_descriptors: Rc<Emitter<ViewDescriptorsChange>>,
    // Active View Descriptors
    on_did_change_active_view_descriptors: Rc<Emitter<ViewDescriptorsChange>>,
    // Visible View Descriptors
    on_did_add_visible_view_descriptors: Rc<Emitter<Vec<IAddedViewDescriptorRef>>>,
    on_did_remove_visible_view_descriptors: Rc<Emitter<Vec<IViewDescriptorRef>>>,
    on_did_move_visible_view_descriptors: Rc<Emitter<ViewDescriptorMove>>,

    logger: Lazy<LoggerRef>,
}

impl Injectable for ViewContainerModel {
    type Args = ViewContainerRc;
    type Deps = (Rc<InstantiationService>, Rc<dyn IContextKeyService>, Rc<dyn LoggerService>);

    fn create(view_container: Self::Args, (instantiation_service, context_key_service, logger_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_container_info = Emitter::new();
        base.register(on_did_change_container_info.clone());
        let on_did_change_all_view_descriptors = Emitter::new();
        base.register(on_did_change_all_view_descriptors.clone());
        let on_did_change_active_view_descriptors = Emitter::new();
        base.register(on_did_change_active_view_descriptors.clone());
        let on_did_add_visible_view_descriptors = Emitter::new();
        base.register(on_did_add_visible_view_descriptors.clone());
        let on_did_remove_visible_view_descriptors = Emitter::new();
        base.register(on_did_remove_visible_view_descriptors.clone());
        let on_did_move_visible_view_descriptors = Emitter::new();
        base.register(on_did_move_visible_view_descriptors.clone());

        let storage_id = view_container.storage_id.clone().filter(|storage_id| !storage_id.is_empty()).unwrap_or_else(|| format!("{}.state", view_container.id));
        let view_descriptors_state = instantiation_service.create_instance::<ViewDescriptorsState>((storage_id, view_container.title.original.clone()));
        base.register(view_descriptors_state.clone());

        let this = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            view_container,
            context_key_service,
            context_keys: RefCell::new(CounterSet::new()),
            view_descriptor_items: RefCell::new(Vec::new()),
            view_descriptors_state,
            title: RefCell::new(None),
            icon: RefCell::new(None),
            keybinding_id: RefCell::new(None),
            on_did_change_container_info,
            on_did_change_all_view_descriptors,
            on_did_change_active_view_descriptors,
            on_did_add_visible_view_descriptors,
            on_did_remove_visible_view_descriptors,
            on_did_move_visible_view_descriptors,
            logger: Lazy::new(move || create_views_logger(logger_service.as_ref())),
        });

        let weak = this.this.clone();
        this.base.register(this.context_key_service.on_did_change_context().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            let affects = {
                let context_keys = this.context_keys.borrow();
                event.affects_some(&ContextKeySet(&context_keys))
            };
            if affects {
                this.on_did_change_context();
            }
        }));
        let weak = this.this.clone();
        this.base.register(this.view_descriptors_state.on_did_change_stored_state.event().listen(move |items| {
            if let Some(this) = weak.upgrade() {
                let items: Vec<(String, bool)> = items.iter().map(|item| (item.id.clone(), item.visible)).collect();
                this.update_visibility(&items);
            }
        }));

        this.update_container_info();
        this
    }
}

impl IDisposable for ViewContainerModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ViewContainerModel {
    fn update_container_info(&self) {
        /* Use default container info if one of the visible view descriptors belongs to the current container by default */
        let visible_view_descriptors = self.visible_view_descriptors();
        let use_default_container_info = self.view_container.always_use_container_info
            || visible_view_descriptors.is_empty()
            || visible_view_descriptors.iter().any(|v| views_registry().get_view_container(&v.id).is_some_and(|container| Rc::ptr_eq(&container, &self.view_container)));
        let first = visible_view_descriptors.first();
        let title = if use_default_container_info {
            self.view_container.title.value.clone()
        } else {
            first
                .and_then(|view| view.container_title.clone().filter(|title| !title.is_empty()))
                .or_else(|| first.map(|view| view.name.value.clone()).filter(|name| !name.is_empty()))
                .unwrap_or_default()
        };
        let mut title_changed = false;
        if self.title.borrow().as_deref() != Some(title.as_str()) {
            *self.title.borrow_mut() = Some(title);
            title_changed = true;
        }

        let icon = if use_default_container_info {
            self.view_container.icon.clone()
        } else {
            Some(first.and_then(|view| view.container_icon.clone()).unwrap_or_else(|| ViewIcon::Theme(default_view_icon())))
        };
        let mut icon_changed = false;
        if !self.is_equal_icon(icon.as_ref()) {
            *self.icon.borrow_mut() = icon;
            icon_changed = true;
        }

        let keybinding_id = self.view_container.open_command_action_descriptor.as_ref().map(|descriptor| descriptor.id.clone()).or_else(|| {
            self.active_view_descriptors().iter().find_map(|view| view.open_command_action_descriptor.as_ref().map(|descriptor| descriptor.id.clone()))
        });
        let mut keybinding_id_changed = false;
        if *self.keybinding_id.borrow() != keybinding_id {
            *self.keybinding_id.borrow_mut() = keybinding_id;
            keybinding_id_changed = true;
        }

        if title_changed || icon_changed || keybinding_id_changed {
            self.on_did_change_container_info.fire(ContainerInfoChange { title: title_changed, icon: icon_changed, keybinding_id: keybinding_id_changed, badge_enablement: false });
        }
    }

    fn is_equal_icon(&self, icon: Option<&ViewIcon>) -> bool {
        match (icon, &*self.icon.borrow()) {
            (Some(ViewIcon::Uri(icon)), Some(ViewIcon::Uri(current))) => ext_uri().is_equal(Some(icon), Some(current), false),
            (Some(ViewIcon::Theme(icon)), Some(ViewIcon::Theme(current))) => ThemeIcon::is_equal(icon, current),
            (None, None) => true,
            _ => false,
        }
    }

    fn update_visibility(&self, view_descriptors: &[(String, bool)]) {
        // First: Update and remove the view descriptors which are asked to be hidden
        let view_descriptor_items_to_hide: Vec<FoundViewDescriptorItem> =
            view_descriptors.iter().filter(|(_, visible)| !visible).filter_map(|(id, _)| self.find_and_ignore_if_not_found(id)).collect();
        let mut removed: Vec<IViewDescriptorRef> = Vec::new();
        for FoundViewDescriptorItem { view_descriptor_item, visible_index } in &view_descriptor_items_to_hide {
            if self.update_view_descriptor_item_visibility(view_descriptor_item, false) {
                removed.push(IViewDescriptorRef { view_descriptor: view_descriptor_item.view_descriptor.clone(), index: *visible_index });
            }
        }
        if !removed.is_empty() {
            self.broad_cast_removed_visible_view_descriptors(removed);
        }

        // Second: Update and add the view descriptors which are asked to be shown
        let mut added: Vec<IAddedViewDescriptorRef> = Vec::new();
        for (id, visible) in view_descriptors {
            if !visible {
                continue;
            }
            let Some(FoundViewDescriptorItem { view_descriptor_item, visible_index }) = self.find_and_ignore_if_not_found(id) else {
                continue;
            };
            if self.update_view_descriptor_item_visibility(&view_descriptor_item, true) {
                let state = *view_descriptor_item.state.borrow();
                added.push(IAddedViewDescriptorRef {
                    index: visible_index,
                    view_descriptor: view_descriptor_item.view_descriptor.clone(),
                    size: state.size,
                    collapsed: state.collapsed.unwrap_or(false),
                });
            }
        }
        if !added.is_empty() {
            self.broad_cast_added_visible_view_descriptors(added);
        }
    }

    fn update_view_descriptor_item_visibility(&self, view_descriptor_item: &ViewDescriptorItem, visible: bool) -> bool {
        if !view_descriptor_item.view_descriptor.can_toggle_visibility {
            return false;
        }
        if Self::is_view_descriptor_visible_when_active(view_descriptor_item) == visible {
            return false;
        }

        // update visibility
        if view_descriptor_item.view_descriptor.workspace {
            view_descriptor_item.state.borrow_mut().visible_workspace = Some(visible);
        } else {
            view_descriptor_item.state.borrow_mut().visible_global = Some(visible);
            if visible {
                self.logger.value().trace(&format!("Showing view {} in the container {}", view_descriptor_item.view_descriptor.id, self.view_container.id));
            }
        }

        // return `true` only if visibility is changed
        Self::is_view_descriptor_visible(view_descriptor_item) == visible
    }

    /// `add(addedViewDescriptorStates)`.
    pub fn add(&self, added_view_descriptor_states: Vec<IAddedViewDescriptorState>) {
        let mut added_items: Vec<ViewDescriptorItem> = Vec::new();
        for added_view_descriptor_state in added_view_descriptor_states {
            let view_descriptor = added_view_descriptor_state.view_descriptor.clone();

            if let Some(when) = &view_descriptor.when {
                for key in when.keys() {
                    self.context_keys.borrow_mut().add(key);
                }
            }

            let state: StateRc = match self.view_descriptors_state.get(&view_descriptor.id) {
                Some(state) => {
                    // set defaults if not set
                    {
                        let mut current = state.borrow_mut();
                        if view_descriptor.workspace {
                            current.visible_workspace = match added_view_descriptor_state.visible {
                                None => current.visible_workspace.or(Some(!view_descriptor.hide_by_default)),
                                visible => visible,
                            };
                        } else {
                            let is_visible = current.visible_global;
                            current.visible_global = match added_view_descriptor_state.visible {
                                None => current.visible_global.or(Some(!view_descriptor.hide_by_default)),
                                visible => visible,
                            };
                            if current.visible_global == Some(true) && !is_visible.unwrap_or(false) {
                                self.logger.value().trace(&format!(
                                    "Added view {} in the container {} and showing it. {:?} {:?} {:?}",
                                    view_descriptor.id, self.view_container.id, is_visible, view_descriptor.hide_by_default, added_view_descriptor_state.visible
                                ));
                            }
                        }
                        current.collapsed = match added_view_descriptor_state.collapsed {
                            None => current.collapsed.or(Some(view_descriptor.collapsed)),
                            collapsed => collapsed,
                        };
                    }
                    state
                }
                None => Rc::new(RefCell::new(ViewDescriptorState {
                    active: false,
                    visible_global: Some(added_view_descriptor_state.visible.unwrap_or(!view_descriptor.hide_by_default)),
                    visible_workspace: Some(added_view_descriptor_state.visible.unwrap_or(!view_descriptor.hide_by_default)),
                    collapsed: Some(added_view_descriptor_state.collapsed.unwrap_or(view_descriptor.collapsed)),
                    order: None,
                    size: None,
                })),
            };
            self.view_descriptors_state.set(&view_descriptor.id, state.clone());
            let active = self.context_key_service.context_matches_rules(view_descriptor.when.as_ref());
            state.borrow_mut().active = active;
            added_items.push(ViewDescriptorItem { view_descriptor, state });
        }
        {
            let mut items = self.view_descriptor_items.borrow_mut();
            items.extend(added_items.iter().cloned());
            self.sort_items(&mut items);
        }
        self.on_did_change_all_view_descriptors
            .fire(ViewDescriptorsChange { added: added_items.iter().map(|item| item.view_descriptor.clone()).collect(), removed: Vec::new() });

        let mut added_active_items: Vec<(ViewDescriptorItem, bool)> = Vec::new();
        for view_descriptor_item in &added_items {
            if view_descriptor_item.state.borrow().active {
                added_active_items.push((view_descriptor_item.clone(), Self::is_view_descriptor_visible(view_descriptor_item)));
            }
        }
        if !added_active_items.is_empty() {
            self.on_did_change_active_view_descriptors
                .fire(ViewDescriptorsChange { added: added_active_items.iter().map(|(item, _)| item.view_descriptor.clone()).collect(), removed: Vec::new() });
        }

        let mut added_visible_descriptors: Vec<IAddedViewDescriptorRef> = Vec::new();
        for (view_descriptor_item, visible) in &added_active_items {
            if *visible && Self::is_view_descriptor_visible(view_descriptor_item) {
                let visible_index = self.find(&view_descriptor_item.view_descriptor.id).visible_index;
                let state = *view_descriptor_item.state.borrow();
                added_visible_descriptors.push(IAddedViewDescriptorRef {
                    index: visible_index,
                    view_descriptor: view_descriptor_item.view_descriptor.clone(),
                    size: state.size,
                    collapsed: state.collapsed.unwrap_or(false),
                });
            }
        }
        self.broad_cast_added_visible_view_descriptors(added_visible_descriptors);
    }

    /// `remove(viewDescriptors)`.
    pub fn remove(&self, view_descriptors: &[ViewDescriptorRc]) {
        let mut removed: Vec<ViewDescriptorRc> = Vec::new();
        let mut removed_items: Vec<ViewDescriptorItem> = Vec::new();
        let mut removed_active_descriptors: Vec<ViewDescriptorRc> = Vec::new();
        let mut removed_visible_descriptors: Vec<IViewDescriptorRef> = Vec::new();

        for view_descriptor in view_descriptors {
            if let Some(when) = &view_descriptor.when {
                for key in when.keys() {
                    self.context_keys.borrow_mut().delete(&key);
                }
            }
            let item = self.view_descriptor_items.borrow().iter().find(|item| item.view_descriptor.id == view_descriptor.id).cloned();
            if let Some(view_descriptor_item) = item {
                removed.push(view_descriptor.clone());
                if view_descriptor_item.state.borrow().active {
                    removed_active_descriptors.push(view_descriptor_item.view_descriptor.clone());
                }
                if Self::is_view_descriptor_visible(&view_descriptor_item) {
                    let visible_index = self.find(&view_descriptor_item.view_descriptor.id).visible_index;
                    removed_visible_descriptors.push(IViewDescriptorRef { index: visible_index, view_descriptor: view_descriptor_item.view_descriptor.clone() });
                }
                removed_items.push(view_descriptor_item);
            }
        }

        // update state
        for item in &removed_items {
            let mut items = self.view_descriptor_items.borrow_mut();
            if let Some(index) = items.iter().position(|candidate| candidate.is_same(item)) {
                items.remove(index);
            }
        }

        self.broad_cast_removed_visible_view_descriptors(removed_visible_descriptors);
        if !removed_active_descriptors.is_empty() {
            self.on_did_change_active_view_descriptors.fire(ViewDescriptorsChange { added: Vec::new(), removed: removed_active_descriptors });
        }
        if !removed.is_empty() {
            self.on_did_change_all_view_descriptors.fire(ViewDescriptorsChange { added: Vec::new(), removed });
        }
    }

    fn on_did_change_context(&self) {
        let mut added_active_items: Vec<(ViewDescriptorItem, bool)> = Vec::new();
        let mut removed_active_items: Vec<ViewDescriptorItem> = Vec::new();

        let items = self.view_descriptor_items.borrow().clone();
        for item in &items {
            let was_active = item.state.borrow().active;
            let is_active = self.context_key_service.context_matches_rules(item.view_descriptor.when.as_ref());
            if was_active != is_active {
                if is_active {
                    added_active_items.push((item.clone(), Self::is_view_descriptor_visible_when_active(item)));
                } else {
                    removed_active_items.push(item.clone());
                }
            }
        }

        let mut removed_visible_descriptors: Vec<IViewDescriptorRef> = Vec::new();
        for item in &removed_active_items {
            if Self::is_view_descriptor_visible(item) {
                let visible_index = self.find(&item.view_descriptor.id).visible_index;
                removed_visible_descriptors.push(IViewDescriptorRef { index: visible_index, view_descriptor: item.view_descriptor.clone() });
            }
        }

        // Update the State
        for item in &removed_active_items {
            item.state.borrow_mut().active = false;
        }
        for (item, _) in &added_active_items {
            item.state.borrow_mut().active = true;
        }

        self.broad_cast_removed_visible_view_descriptors(removed_visible_descriptors);

        if !added_active_items.is_empty() || !removed_active_items.is_empty() {
            self.on_did_change_active_view_descriptors.fire(ViewDescriptorsChange {
                added: added_active_items.iter().map(|(item, _)| item.view_descriptor.clone()).collect(),
                removed: removed_active_items.iter().map(|item| item.view_descriptor.clone()).collect(),
            });
        }

        let mut added_visible_descriptors: Vec<IAddedViewDescriptorRef> = Vec::new();
        for (item, visible_when_active) in &added_active_items {
            if *visible_when_active && Self::is_view_descriptor_visible(item) {
                let visible_index = self.find(&item.view_descriptor.id).visible_index;
                let state = *item.state.borrow();
                added_visible_descriptors.push(IAddedViewDescriptorRef {
                    index: visible_index,
                    view_descriptor: item.view_descriptor.clone(),
                    size: state.size,
                    collapsed: state.collapsed.unwrap_or(false),
                });
            }
        }
        self.broad_cast_added_visible_view_descriptors(added_visible_descriptors);
    }

    fn broad_cast_added_visible_view_descriptors(&self, mut added: Vec<IAddedViewDescriptorRef>) {
        if !added.is_empty() {
            let ids = added.iter().map(|v| v.view_descriptor.id.as_str()).collect::<Vec<_>>().join(",");
            let reason = format!("Added views:{ids} in {}", self.view_container.id);
            sort_by_compare(&mut added, |a, b| a.index as CompareResult - b.index as CompareResult);
            self.on_did_add_visible_view_descriptors.fire(added);
            self.update_state(&reason);
        }
    }

    fn broad_cast_removed_visible_view_descriptors(&self, mut removed: Vec<IViewDescriptorRef>) {
        if !removed.is_empty() {
            let ids = removed.iter().map(|v| v.view_descriptor.id.as_str()).collect::<Vec<_>>().join(",");
            let reason = format!("Removed views:{ids} from {}", self.view_container.id);
            sort_by_compare(&mut removed, |a, b| b.index as CompareResult - a.index as CompareResult);
            self.on_did_remove_visible_view_descriptors.fire(removed);
            self.update_state(&reason);
        }
    }

    fn broad_cast_moved_view_descriptors(&self, from: IViewDescriptorRef, to: IViewDescriptorRef) {
        let reason = format!("Moved view {} to {} in {}", from.view_descriptor.id, to.view_descriptor.id, self.view_container.id);
        self.on_did_move_visible_view_descriptors.fire(ViewDescriptorMove { from, to });
        self.update_state(&reason);
    }

    fn update_state(&self, reason: &str) {
        self.logger.value().trace(reason);
        self.view_descriptors_state.update_state(&self.all_view_descriptors());
        self.update_container_info();
    }

    fn is_view_descriptor_visible(view_descriptor_item: &ViewDescriptorItem) -> bool {
        if !view_descriptor_item.state.borrow().active {
            return false;
        }
        Self::is_view_descriptor_visible_when_active(view_descriptor_item)
    }

    fn is_view_descriptor_visible_when_active(view_descriptor_item: &ViewDescriptorItem) -> bool {
        let state = view_descriptor_item.state.borrow();
        if view_descriptor_item.view_descriptor.workspace {
            return state.visible_workspace.unwrap_or(false);
        }
        state.visible_global.unwrap_or(false)
    }

    fn find(&self, id: &str) -> FoundViewDescriptorItem {
        match self.find_and_ignore_if_not_found(id) {
            Some(result) => result,
            None => errors::throw(Error::new(format!("view descriptor {id} not found"))),
        }
    }

    fn find_and_ignore_if_not_found(&self, id: &str) -> Option<FoundViewDescriptorItem> {
        let items = self.view_descriptor_items.borrow();
        let mut visible_index = 0;
        for view_descriptor_item in items.iter() {
            if view_descriptor_item.view_descriptor.id == id {
                return Some(FoundViewDescriptorItem { visible_index, view_descriptor_item: view_descriptor_item.clone() });
            }
            if Self::is_view_descriptor_visible(view_descriptor_item) {
                visible_index += 1;
            }
        }
        None
    }

    /// `this.viewDescriptorItems.sort(this.compareViewDescriptors.bind(this))`.
    fn sort_items(&self, items: &mut [ViewDescriptorItem]) {
        sort_by_compare(items, |a, b| Self::compare_view_descriptors(a, b));
    }

    fn compare_view_descriptors(a: &ViewDescriptorItem, b: &ViewDescriptorItem) -> CompareResult {
        if a.view_descriptor.id == b.view_descriptor.id {
            return 0.0;
        }

        let by_order = Self::get_view_order(a) - Self::get_view_order(b);
        // `(orderA - orderB) || groupOrder`: zero e `NaN` são falsos.
        if by_order != 0.0 && !by_order.is_nan() { by_order } else { Self::get_group_order_result(&a.view_descriptor, &b.view_descriptor) }
    }

    fn get_view_order(view_descriptor_item: &ViewDescriptorItem) -> f64 {
        let view_order = view_descriptor_item.state.borrow().order.or(view_descriptor_item.view_descriptor.order);
        view_order.unwrap_or(f64::MAX)
    }

    fn get_group_order_result(a: &ViewDescriptorRc, b: &ViewDescriptorRc) -> CompareResult {
        let (Some(a_group), Some(b_group)) = (a.group.as_deref().filter(|group| !group.is_empty()), b.group.as_deref().filter(|group| !group.is_empty())) else {
            return 0.0;
        };

        if a_group == b_group {
            return 0.0;
        }

        // `a.group < b.group`: a comparação de strings do JavaScript é por unidade de código UTF-16.
        if a_group.encode_utf16().lt(b_group.encode_utf16()) { -1.0 } else { 1.0 }
    }
}

impl IViewContainerModel for ViewContainerModel {
    fn view_container(&self) -> ViewContainerRc {
        self.view_container.clone()
    }

    fn title(&self) -> String {
        self.title.borrow().clone().unwrap_or_default()
    }

    fn icon(&self) -> Option<ViewIcon> {
        self.icon.borrow().clone()
    }

    fn keybinding_id(&self) -> Option<String> {
        self.keybinding_id.borrow().clone()
    }

    fn on_did_change_container_info(&self) -> Event<ContainerInfoChange> {
        self.on_did_change_container_info.event()
    }

    fn all_view_descriptors(&self) -> Vec<ViewDescriptorRc> {
        self.view_descriptor_items.borrow().iter().map(|item| item.view_descriptor.clone()).collect()
    }

    fn on_did_change_all_view_descriptors(&self) -> Event<ViewDescriptorsChange> {
        self.on_did_change_all_view_descriptors.event()
    }

    fn active_view_descriptors(&self) -> Vec<ViewDescriptorRc> {
        self.view_descriptor_items.borrow().iter().filter(|item| item.state.borrow().active).map(|item| item.view_descriptor.clone()).collect()
    }

    fn on_did_change_active_view_descriptors(&self) -> Event<ViewDescriptorsChange> {
        self.on_did_change_active_view_descriptors.event()
    }

    fn visible_view_descriptors(&self) -> Vec<ViewDescriptorRc> {
        self.view_descriptor_items.borrow().iter().filter(|item| Self::is_view_descriptor_visible(item)).map(|item| item.view_descriptor.clone()).collect()
    }

    fn on_did_add_visible_view_descriptors(&self) -> Event<Vec<IAddedViewDescriptorRef>> {
        self.on_did_add_visible_view_descriptors.event()
    }

    fn on_did_remove_visible_view_descriptors(&self) -> Event<Vec<IViewDescriptorRef>> {
        self.on_did_remove_visible_view_descriptors.event()
    }

    fn on_did_move_visible_view_descriptors(&self) -> Event<ViewDescriptorMove> {
        self.on_did_move_visible_view_descriptors.event()
    }

    fn is_visible(&self, id: &str) -> bool {
        let view_descriptor_item = self.view_descriptor_items.borrow().iter().find(|v| v.view_descriptor.id == id).cloned();
        match view_descriptor_item {
            Some(view_descriptor_item) => Self::is_view_descriptor_visible(&view_descriptor_item),
            None => errors::throw(Error::new(format!("Unknown view {id}"))),
        }
    }

    fn set_visible(&self, id: &str, visible: bool) {
        self.update_visibility(&[(id.to_string(), visible)]);
    }

    fn is_collapsed(&self, id: &str) -> bool {
        self.find(id).view_descriptor_item.state.borrow().collapsed.unwrap_or(false)
    }

    fn set_collapsed(&self, id: &str, collapsed: bool) {
        let view_descriptor_item = self.find(id).view_descriptor_item;
        if view_descriptor_item.state.borrow().collapsed != Some(collapsed) {
            view_descriptor_item.state.borrow_mut().collapsed = Some(collapsed);
        }
        self.view_descriptors_state.update_state(&self.all_view_descriptors());
    }

    fn get_size(&self, id: &str) -> Option<f64> {
        self.find(id).view_descriptor_item.state.borrow().size
    }

    fn set_sizes(&self, new_sizes: &[ViewSize]) {
        for ViewSize { id, size } in new_sizes {
            let view_descriptor_item = self.find(id).view_descriptor_item;
            if view_descriptor_item.state.borrow().size != Some(*size) {
                view_descriptor_item.state.borrow_mut().size = Some(*size);
            }
        }
        self.view_descriptors_state.update_state(&self.all_view_descriptors());
    }

    fn move_(&self, from: &str, to: &str) {
        let (from_index, to_index, from_view_descriptor, to_view_descriptor) = {
            let mut items = self.view_descriptor_items.borrow_mut();
            let from_index = items.iter().position(|v| v.view_descriptor.id == from);
            let to_index = items.iter().position(|v| v.view_descriptor.id == to);
            let (Some(from_index), Some(to_index)) = (from_index, to_index) else {
                errors::throw(Error::new("Cannot read properties of undefined (reading 'viewDescriptor')"));
            };

            let from_view_descriptor = items[from_index].view_descriptor.clone();
            let to_view_descriptor = items[to_index].view_descriptor.clone();

            r#move(&mut items, from_index, to_index);

            for (index, item) in items.iter().enumerate() {
                item.state.borrow_mut().order = Some(index as f64);
            }
            (from_index, to_index, from_view_descriptor, to_view_descriptor)
        };

        self.broad_cast_moved_view_descriptors(
            IViewDescriptorRef { index: from_index, view_descriptor: from_view_descriptor },
            IViewDescriptorRef { index: to_index, view_descriptor: to_view_descriptor },
        );
    }
}
