//! Porte de `src/vs/platform/contextkey/browser/contextKeyService.ts`: o serviço de chaves de contexto
//! (raiz, escopos e sobreposição), os eventos de mudança e a leitura de `config.*`.
//!
//! Divergências:
//! - O rastreio de foco do DOM (`FOCUS_IN` em cada janela, que liga `inputFocus`) não existe aqui: o
//!   host de UI chama [`ContextKeyService::set_input_focused`] quando um campo de texto ganha ou perde
//!   o foco. A chave `inputFocus` nasce `false`, como no original.
//! - Os comandos `_setContext`, `getContextKeyInfo` e `_generateContextKeyInfo` ficam para o registro de
//!   comandos ([`crate::commands`]), que a UI liga a [`set_context`] e a [`RawContextKey::all`].
//! - O serviço raiz lê a configuração por [`ContextKeyConfigurationService`] (o `IConfigurationService`
//!   ainda não é um trait neste porte).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Event, PauseableEmitter};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable};
use je_base::objects;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;

use super::contextkey::{
    ContextKeyExpression, IContext, IContextKey, IContextKeyChangeEvent, IContextKeyService, IContextKeyServiceTarget,
    IScopedContextKeyService, ReadableSet,
};
use super::contextkeys::input_focused_context;
use crate::js::{to_json, to_js_string};

const KEYBINDING_CONTEXT_ATTR: &str = "data-keybinding-context";

/// A mudança de configuração que o serviço raiz precisa: a origem era o padrão, e as chaves afetadas.
#[derive(Clone, Debug)]
pub struct ContextKeyConfigurationChange {
    /// `event.source === ConfigurationTarget.DEFAULT`.
    pub source_is_default: bool,
    pub affected_keys: Vec<String>,
}

/// O que o serviço raiz lê do `IConfigurationService`.
pub trait ContextKeyConfigurationService {
    fn get_value(&self, section: &str) -> Dynamic;
    fn on_did_change_configuration(&self) -> Event<ContextKeyConfigurationChange>;
}

impl ContextKeyConfigurationService for crate::configuration::service::ConfigurationService {
    fn get_value(&self, section: &str) -> Dynamic {
        crate::configuration::service::ConfigurationService::get_value(self, Some(section), None)
    }

    fn on_did_change_configuration(&self) -> Event<ContextKeyConfigurationChange> {
        crate::configuration::service::ConfigurationService::on_did_change_configuration(self).map(
            |event| ContextKeyConfigurationChange {
                source_is_default: event.source.get() == Some(crate::configuration::ConfigurationTarget::Default),
                affected_keys: event.affected_keys.iter().map(JsString::to_string_lossy).collect(),
            },
            None,
        )
    }
}

/// O nó de valores de um contexto: o `Context` do original e as suas especializações.
pub trait ContextNode: IContext {
    fn set_value(&self, key: &str, value: Dynamic) -> bool;
    fn remove_value(&self, key: &str) -> bool;
    /// `value`: cópia dos valores próprios (inclui `_contextId`).
    fn value(&self) -> BTreeMap<String, Dynamic>;
    fn update_parent(&self, parent: Rc<dyn ContextNode>);
    fn collect_all_values(&self) -> BTreeMap<String, Dynamic>;
}

/// `Context`.
pub struct Context {
    parent: RefCell<Option<Rc<dyn ContextNode>>>,
    value: RefCell<BTreeMap<String, Dynamic>>,
}

impl Context {
    pub fn new(id: i64, parent: Option<Rc<dyn ContextNode>>) -> Rc<Self> {
        let mut value = BTreeMap::new();
        value.insert("_contextId".to_string(), Dynamic::Number(id as f64));
        Rc::new(Self { parent: RefCell::new(parent), value: RefCell::new(value) })
    }
}

impl IContext for Context {
    fn get_value(&self, key: &str) -> Dynamic {
        let own = self.value.borrow().get(key).cloned().unwrap_or(Dynamic::Undefined);
        if own.is_undefined() {
            let parent = self.parent.borrow().clone();
            if let Some(parent) = parent {
                return parent.get_value(key);
            }
        }
        own
    }
}

impl ContextNode for Context {
    fn set_value(&self, key: &str, value: Dynamic) -> bool {
        let current = self.value.borrow().get(key).cloned().unwrap_or(Dynamic::Undefined);
        if !objects::equals(&current, &value) {
            self.value.borrow_mut().insert(key.to_string(), value);
            return true;
        }
        false
    }

    fn remove_value(&self, key: &str) -> bool {
        self.value.borrow_mut().remove(key).is_some()
    }

    fn value(&self) -> BTreeMap<String, Dynamic> {
        self.value.borrow().clone()
    }

    fn update_parent(&self, parent: Rc<dyn ContextNode>) {
        *self.parent.borrow_mut() = Some(parent);
    }

    fn collect_all_values(&self) -> BTreeMap<String, Dynamic> {
        let parent = self.parent.borrow().clone();
        let mut result = parent.map(|p| p.collect_all_values()).unwrap_or_default();
        for (k, v) in self.value.borrow().iter() {
            result.insert(k.clone(), v.clone());
        }
        result.remove("_contextId");
        result
    }
}

/// `NullContext`: o contexto de um serviço descartado.
struct NullContext;

impl IContext for NullContext {
    fn get_value(&self, _key: &str) -> Dynamic {
        Dynamic::Undefined
    }
}

impl ContextNode for NullContext {
    fn set_value(&self, _key: &str, _value: Dynamic) -> bool {
        false
    }
    fn remove_value(&self, _key: &str) -> bool {
        false
    }
    fn value(&self) -> BTreeMap<String, Dynamic> {
        BTreeMap::new()
    }
    fn update_parent(&self, _parent: Rc<dyn ContextNode>) {}
    fn collect_all_values(&self) -> BTreeMap<String, Dynamic> {
        BTreeMap::new()
    }
}

type ChangeEvent = Rc<dyn IContextKeyChangeEvent>;

/// `SimpleContextKeyChangeEvent`.
struct SimpleContextKeyChangeEvent {
    key: String,
}

impl IContextKeyChangeEvent for SimpleContextKeyChangeEvent {
    fn affects_some(&self, keys: &dyn ReadableSet) -> bool {
        keys.has(&self.key)
    }
    fn all_keys_contained_in(&self, keys: &dyn ReadableSet) -> bool {
        self.affects_some(keys)
    }
}

/// `ArrayContextKeyChangeEvent`.
pub struct ArrayContextKeyChangeEvent {
    keys: Vec<String>,
}

impl ArrayContextKeyChangeEvent {
    pub fn new(keys: Vec<String>) -> Self {
        Self { keys }
    }
}

impl IContextKeyChangeEvent for ArrayContextKeyChangeEvent {
    fn affects_some(&self, keys: &dyn ReadableSet) -> bool {
        self.keys.iter().any(|k| keys.has(k))
    }
    fn all_keys_contained_in(&self, keys: &dyn ReadableSet) -> bool {
        self.keys.iter().all(|k| keys.has(k))
    }
}

/// `CompositeContextKeyChangeEvent`.
struct CompositeContextKeyChangeEvent {
    events: Vec<ChangeEvent>,
}

impl IContextKeyChangeEvent for CompositeContextKeyChangeEvent {
    fn affects_some(&self, keys: &dyn ReadableSet) -> bool {
        self.events.iter().any(|e| e.affects_some(keys))
    }
    fn all_keys_contained_in(&self, keys: &dyn ReadableSet) -> bool {
        self.events.iter().all(|e| e.all_keys_contained_in(keys))
    }
}

/// O contêiner raiz: lê `config.*` do serviço de configuração, com cache invalidado pelas mudanças.
struct ConfigAwareContextValuesContainer {
    base: Rc<Context>,
    configuration_service: Rc<dyn ContextKeyConfigurationService>,
    values: Rc<RefCell<TernarySearchTree<String, Dynamic>>>,
    listener: RefCell<Option<DisposableRef>>,
}

const CONFIG_KEY_PREFIX: &str = "config.";

impl ConfigAwareContextValuesContainer {
    fn new(
        id: i64,
        configuration_service: Rc<dyn ContextKeyConfigurationService>,
        emitter: Rc<PauseableEmitter<ChangeEvent>>,
    ) -> Rc<Self> {
        let values: Rc<RefCell<TernarySearchTree<String, Dynamic>>> = Rc::new(RefCell::new(TernarySearchTree::for_config_keys()));
        let cache = values.clone();
        let listener = configuration_service.on_did_change_configuration().listen(move |event| {
            if event.source_is_default {
                // new setting, reset everything
                let all_keys: Vec<String> = cache.borrow().iter_entries().map(|(k, _)| k).collect();
                cache.borrow_mut().clear();
                emitter.fire(Rc::new(ArrayContextKeyChangeEvent::new(all_keys)));
            } else {
                let mut changed_keys = Vec::new();
                for config_key in &event.affected_keys {
                    let context_key = format!("config.{config_key}");
                    let cached_items = cache.borrow().find_superstr(&context_key);
                    if let Some(cached_items) = cached_items {
                        changed_keys.extend(cached_items.map(|(key, _)| key));
                        cache.borrow_mut().delete_superstr(&context_key);
                    }
                    if cache.borrow().has(&context_key) {
                        changed_keys.push(context_key.clone());
                        cache.borrow_mut().delete(&context_key);
                    }
                }
                emitter.fire(Rc::new(ArrayContextKeyChangeEvent::new(changed_keys)));
            }
        });
        Rc::new(Self { base: Context::new(id, None), configuration_service, values, listener: RefCell::new(Some(listener)) })
    }
}

impl IDisposable for ConfigAwareContextValuesContainer {
    fn dispose(&self) {
        if let Some(listener) = self.listener.borrow_mut().take() {
            listener.dispose();
        }
    }
}

impl IContext for ConfigAwareContextValuesContainer {
    fn get_value(&self, key: &str) -> Dynamic {
        if !key.starts_with(CONFIG_KEY_PREFIX) {
            return self.base.get_value(key);
        }
        if let Some(cached) = self.values.borrow().get(&key.to_string()) {
            return cached.clone();
        }
        let config_key = &key[CONFIG_KEY_PREFIX.len()..];
        let config_value = self.configuration_service.get_value(config_key);
        let value = match &config_value {
            Dynamic::Array(_) => match to_json(&config_value) {
                Some(json) => Dynamic::String(je_base::marshalling::stringify(&je_base::marshalling::from_json(&json))),
                None => Dynamic::Undefined,
            },
            other => other.clone(),
        };
        self.values.borrow_mut().set(key.to_string(), value.clone());
        value
    }
}

impl ContextNode for ConfigAwareContextValuesContainer {
    fn set_value(&self, key: &str, value: Dynamic) -> bool {
        self.base.set_value(key, value)
    }
    fn remove_value(&self, key: &str) -> bool {
        self.base.remove_value(key)
    }
    fn value(&self) -> BTreeMap<String, Dynamic> {
        self.base.value()
    }
    fn update_parent(&self, parent: Rc<dyn ContextNode>) {
        self.base.update_parent(parent);
    }
    fn collect_all_values(&self) -> BTreeMap<String, Dynamic> {
        let mut result: BTreeMap<String, Dynamic> = self.values.borrow().iter_entries().collect();
        for (k, v) in self.base.collect_all_values() {
            result.insert(k, v);
        }
        result
    }
}

/// `ContextKey`.
struct ContextKey {
    service: Rc<AbstractContextKeyService>,
    key: String,
    default_value: Dynamic,
}

impl ContextKey {
    fn new(service: Rc<AbstractContextKeyService>, key: &str, default_value: Dynamic) -> Rc<Self> {
        let this = Rc::new(Self { service, key: key.to_string(), default_value });
        this.reset();
        this
    }
}

impl IContextKey for ContextKey {
    fn set(&self, value: Dynamic) {
        self.service.set_context(&self.key, value);
    }

    fn reset(&self) {
        if self.default_value.is_undefined() {
            self.service.remove_context(&self.key);
        } else {
            self.service.set_context(&self.key, self.default_value.clone());
        }
    }

    fn get(&self) -> Dynamic {
        self.service.get_context_key_value(&self.key)
    }
}

/// O estado só do serviço raiz.
struct RootData {
    last_context_id: Cell<i64>,
    contexts: RefCell<HashMap<i64, Rc<dyn ContextNode>>>,
    input_focused_context: RefCell<Option<Rc<dyn IContextKey>>>,
}

/// O estado só do serviço escopado.
struct ScopedData {
    parent: RefCell<Rc<AbstractContextKeyService>>,
    dom_node: Rc<dyn IContextKeyServiceTarget>,
    parent_change_listener: Rc<MutableDisposable<dyn IDisposable>>,
}

enum Kind {
    Root(RootData),
    Scoped(ScopedData),
}

/// `AbstractContextKeyService` e as suas duas subclasses (`ContextKeyService`, `ScopedContextKeyService`).
pub struct AbstractContextKeyService {
    me: Weak<AbstractContextKeyService>,
    base: Rc<Disposable>,
    is_disposed: Cell<bool>,
    my_context_id: Cell<i64>,
    on_did_change_context: Rc<PauseableEmitter<ChangeEvent>>,
    kind: Kind,
}

/// `ContextKeyService`: o serviço raiz.
pub type ContextKeyService = AbstractContextKeyService;
/// `ScopedContextKeyService`.
pub type ScopedContextKeyService = AbstractContextKeyService;

fn new_emitter() -> Rc<PauseableEmitter<ChangeEvent>> {
    PauseableEmitter::with_options(
        Default::default(),
        Some(Rc::new(|events: Vec<ChangeEvent>| -> ChangeEvent { Rc::new(CompositeContextKeyChangeEvent { events }) })),
    )
}

impl AbstractContextKeyService {
    /// `new ContextKeyService(configurationService)`.
    pub fn new(configuration_service: Rc<dyn ContextKeyConfigurationService>) -> Rc<Self> {
        let base = Disposable::new();
        let emitter = new_emitter();
        base.register(emitter.clone());
        let my_context = ConfigAwareContextValuesContainer::new(0, configuration_service, emitter.clone());
        base.register(my_context.clone());
        let mut contexts: HashMap<i64, Rc<dyn ContextNode>> = HashMap::new();
        contexts.insert(0, my_context);
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            is_disposed: Cell::new(false),
            my_context_id: Cell::new(0),
            on_did_change_context: emitter,
            kind: Kind::Root(RootData {
                last_context_id: Cell::new(0),
                contexts: RefCell::new(contexts),
                input_focused_context: RefCell::new(None),
            }),
        });
        let key = input_focused_context().bind_to(&*this);
        if let Kind::Root(root) = &this.kind {
            *root.input_focused_context.borrow_mut() = Some(key);
        }
        this
    }

    /// Liga `inputFocus`: o host de UI chama quando um campo editável ganha ou perde o foco.
    pub fn set_input_focused(&self, focused: bool) {
        if let Kind::Root(root) = &self.kind
            && let Some(key) = root.input_focused_context.borrow().as_ref()
        {
            key.set(Dynamic::Bool(focused));
        }
    }

    fn new_scoped(parent: Rc<AbstractContextKeyService>, dom_node: Rc<dyn IContextKeyServiceTarget>) -> Rc<Self> {
        let my_context_id = parent.create_child_context(None);
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base: Disposable::new(),
            is_disposed: Cell::new(false),
            my_context_id: Cell::new(my_context_id),
            on_did_change_context: new_emitter(),
            kind: Kind::Scoped(ScopedData {
                parent: RefCell::new(parent),
                dom_node: dom_node.clone(),
                parent_change_listener: MutableDisposable::new(),
            }),
        });
        this.base.register(this.on_did_change_context.clone());
        if let Kind::Scoped(scoped) = &this.kind {
            this.base.register(scoped.parent_change_listener.clone());
        }
        this.update_parent_change_listener();

        if dom_node.has_attribute(KEYBINDING_CONTEXT_ATTR) {
            eprintln!("Element already has context attribute");
        }
        dom_node.set_attribute(KEYBINDING_CONTEXT_ATTR, &my_context_id.to_string());
        this
    }

    fn scoped(&self) -> Option<&ScopedData> {
        match &self.kind {
            Kind::Scoped(s) => Some(s),
            Kind::Root(_) => None,
        }
    }

    /// Encaminha os eventos do pai, a não ser que todas as chaves tenham sido sobrescritas por este escopo.
    fn update_parent_change_listener(&self) {
        let Some(scoped) = self.scoped() else { return };
        let me = self.me.clone();
        let parent = scoped.parent.borrow().clone();
        let listener = parent.on_did_change_context().listen(move |event| {
            let Some(this) = me.upgrade() else { return };
            let Some(scoped) = this.scoped() else { return };
            let this_container = scoped.parent.borrow().get_context_values_container_node(this.my_context_id.get());
            let this_values: HashSet<String> = this_container.value().into_keys().collect();
            if !event.all_keys_contained_in(&this_values) {
                this.on_did_change_context.fire(event.clone());
            }
        });
        scoped.parent_change_listener.set(Some(listener));
    }

    pub fn context_id_value(&self) -> i64 {
        self.my_context_id.get()
    }

    fn throw_if_disposed(&self) {
        if self.is_disposed.get() {
            errors::throw(Error::new("AbstractContextKeyService has been disposed"));
        }
    }

    pub fn set_context(&self, key: &str, value: Dynamic) {
        if self.is_disposed.get() {
            return;
        }
        let my_context = self.get_context_values_container_node(self.my_context_id.get());
        if my_context.set_value(key, value) {
            self.on_did_change_context.fire(Rc::new(SimpleContextKeyChangeEvent { key: key.to_string() }));
        }
    }

    pub fn remove_context(&self, key: &str) {
        if self.is_disposed.get() {
            return;
        }
        if self.get_context_values_container_node(self.my_context_id.get()).remove_value(key) {
            self.on_did_change_context.fire(Rc::new(SimpleContextKeyChangeEvent { key: key.to_string() }));
        }
    }

    /// `getContextValuesContainer`.
    pub fn get_context_values_container_node(&self, context_id: i64) -> Rc<dyn ContextNode> {
        if self.is_disposed.get() {
            return Rc::new(NullContext);
        }
        match &self.kind {
            Kind::Root(root) => root.contexts.borrow().get(&context_id).cloned().unwrap_or_else(|| Rc::new(NullContext)),
            Kind::Scoped(scoped) => scoped.parent.borrow().get_context_values_container_node(context_id),
        }
    }

    /// `createChildContext`: `None` usa o contexto deste serviço como pai.
    pub fn create_child_context(&self, parent_context_id: Option<i64>) -> i64 {
        let parent_id = parent_context_id.unwrap_or(self.my_context_id.get());
        match &self.kind {
            Kind::Root(root) => {
                if self.is_disposed.get() {
                    errors::throw(Error::new("ContextKeyService has been disposed"));
                }
                let id = root.last_context_id.get() + 1;
                root.last_context_id.set(id);
                let parent = self.get_context_values_container_node(parent_id);
                root.contexts.borrow_mut().insert(id, Context::new(id, Some(parent)));
                id
            }
            Kind::Scoped(scoped) => {
                if self.is_disposed.get() {
                    errors::throw(Error::new("ScopedContextKeyService has been disposed"));
                }
                let parent = scoped.parent.borrow().clone();
                parent.create_child_context(Some(parent_id))
            }
        }
    }

    /// `disposeContext`. No escopo sempre encaminha ao pai, mesmo depois de descartado: um contexto filho
    /// pode ser descartado depois de nós e ainda precisa chegar à raiz para sair de `contexts`.
    pub fn dispose_context(&self, context_id: i64) {
        match &self.kind {
            Kind::Root(root) => {
                if !self.is_disposed.get() {
                    root.contexts.borrow_mut().remove(&context_id);
                }
            }
            Kind::Scoped(scoped) => {
                let parent = scoped.parent.borrow().clone();
                parent.dispose_context(context_id);
            }
        }
    }

    fn me(&self) -> Rc<AbstractContextKeyService> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }
}

impl IDisposable for AbstractContextKeyService {
    fn dispose(&self) {
        if let Kind::Scoped(scoped) = &self.kind {
            if self.is_disposed.get() {
                return;
            }
            // Solta o ouvinte do pai antes de `disposeContext`, para não encaminhar eventos do pai com o
            // serviço já em desmonte.
            scoped.parent_change_listener.clear();
            let parent = scoped.parent.borrow().clone();
            parent.dispose_context(self.my_context_id.get());
            scoped.dom_node.remove_attribute(KEYBINDING_CONTEXT_ATTR);
        }
        self.base.dispose();
        self.is_disposed.set(true);
    }
}

impl IScopedContextKeyService for AbstractContextKeyService {}

/// `findContextAttr`: o id do contexto do nó mais próximo que tenha o atributo; `0` se nenhum; `-1`
/// (o `NaN` do original) se o atributo for vazio.
fn find_context_attr(dom_node: Option<Rc<dyn IContextKeyServiceTarget>>) -> i64 {
    let mut node = dom_node;
    while let Some(current) = node {
        if current.has_attribute(KEYBINDING_CONTEXT_ATTR) {
            return match current.get_attribute(KEYBINDING_CONTEXT_ATTR) {
                Some(attr) if !attr.is_empty() => parse_int(&attr),
                _ => -1,
            };
        }
        node = current.parent_element();
    }
    0
}

/// `parseInt(text, 10)`: os dígitos do começo, `NaN` (aqui `-1`) se não houver.
fn parse_int(text: &str) -> i64 {
    let digits: String = text.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(-1)
}

impl IContextKeyService for AbstractContextKeyService {
    fn on_did_change_context(&self) -> Event<ChangeEvent> {
        self.on_did_change_context.event()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        self.on_did_change_context.pause();
        struct Resume<'a>(&'a PauseableEmitter<ChangeEvent>);
        impl Drop for Resume<'_> {
            fn drop(&mut self) {
                self.0.resume();
            }
        }
        let _resume = Resume(&self.on_did_change_context);
        callback();
    }

    fn create_key(&self, key: &str, default_value: Dynamic) -> Rc<dyn IContextKey> {
        self.throw_if_disposed();
        ContextKey::new(self.me(), key, default_value)
    }

    fn context_matches_rules(&self, rules: Option<&ContextKeyExpression>) -> bool {
        self.throw_if_disposed();
        let context = self.get_context_values_container_node(self.my_context_id.get());
        match rules {
            Some(rules) => rules.evaluate(&*context),
            None => true,
        }
    }

    fn get_context_key_value(&self, key: &str) -> Dynamic {
        if self.is_disposed.get() {
            return Dynamic::Undefined;
        }
        self.get_context_values_container_node(self.my_context_id.get()).get_value(key)
    }

    fn create_scoped(&self, target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        self.throw_if_disposed();
        Self::new_scoped(self.me(), target)
    }

    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        self.throw_if_disposed();
        OverlayContextKeyService::new(self.me(), overlay)
    }

    fn get_context(&self, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        if self.is_disposed.get() {
            return Rc::new(NullContext);
        }
        self.get_context_values_container_node(find_context_attr(target))
    }

    fn update_parent(&self, parent_context_key_service: Rc<dyn IContextKeyService>) {
        let Some(scoped) = self.scoped() else {
            errors::throw(Error::new("Cannot update parent of root ContextKeyService"));
        };
        let Some(new_parent) = parent_context_key_service.as_abstract() else {
            errors::throw(Error::new("Not supported."));
        };
        if Rc::ptr_eq(&scoped.parent.borrow(), &new_parent) {
            return;
        }

        let old_parent = scoped.parent.borrow().clone();
        let this_container = old_parent.get_context_values_container_node(self.my_context_id.get());
        let old_all_values = this_container.collect_all_values();
        *scoped.parent.borrow_mut() = new_parent.clone();
        self.update_parent_change_listener();
        let new_parent_container = new_parent.get_context_values_container_node(new_parent.my_context_id.get());
        this_container.update_parent(new_parent_container);

        let new_all_values = this_container.collect_all_values();
        let mut changed_keys: Vec<String> = Vec::new();
        for (key, value) in &new_all_values {
            if !objects::equals(old_all_values.get(key).unwrap_or(&Dynamic::Undefined), value) {
                changed_keys.push(key.clone());
            }
        }
        for (key, value) in &old_all_values {
            if !objects::equals(new_all_values.get(key).unwrap_or(&Dynamic::Undefined), value) && !changed_keys.contains(key) {
                changed_keys.push(key.clone());
            }
        }
        self.on_did_change_context.fire(Rc::new(ArrayContextKeyChangeEvent::new(changed_keys)));
    }

    fn context_id(&self) -> i64 {
        self.my_context_id.get()
    }

    fn get_context_values_container(&self, context_id: i64) -> Rc<dyn IContext> {
        self.get_context_values_container_node(context_id)
    }

    fn as_abstract(&self) -> Option<Rc<AbstractContextKeyService>> {
        Some(self.me())
    }
}

/// `OverlayContext`.
struct OverlayContext {
    parent: Rc<dyn IContext>,
    overlay: Rc<HashMap<String, Dynamic>>,
}

impl IContext for OverlayContext {
    fn get_value(&self, key: &str) -> Dynamic {
        match self.overlay.get(key) {
            Some(value) => value.clone(),
            None => self.parent.get_value(key),
        }
    }
}

/// `OverlayContextKeyService`: valores sobrepostos sem escrever no serviço pai.
pub struct OverlayContextKeyService {
    me: Weak<OverlayContextKeyService>,
    parent: Rc<dyn IContextKeyService>,
    overlay: Rc<HashMap<String, Dynamic>>,
}

impl OverlayContextKeyService {
    fn new(parent: Rc<dyn IContextKeyService>, overlay: Vec<(String, Dynamic)>) -> Rc<Self> {
        let overlay: HashMap<String, Dynamic> = overlay.into_iter().collect();
        Rc::new_cyclic(|me| Self { me: me.clone(), parent, overlay: Rc::new(overlay) })
    }
}

impl IContextKeyService for OverlayContextKeyService {
    fn on_did_change_context(&self) -> Event<ChangeEvent> {
        self.parent.on_did_change_context()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        self.parent.buffer_change_events(callback);
    }

    fn create_key(&self, _key: &str, _default_value: Dynamic) -> Rc<dyn IContextKey> {
        errors::throw(Error::new("Not supported."))
    }

    fn get_context(&self, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        Rc::new(OverlayContext { parent: self.parent.get_context(target), overlay: self.overlay.clone() })
    }

    fn get_context_values_container(&self, context_id: i64) -> Rc<dyn IContext> {
        Rc::new(OverlayContext { parent: self.parent.get_context_values_container(context_id), overlay: self.overlay.clone() })
    }

    fn context_matches_rules(&self, rules: Option<&ContextKeyExpression>) -> bool {
        let context = self.get_context_values_container(self.context_id());
        match rules {
            Some(rules) => rules.evaluate(&*context),
            None => true,
        }
    }

    fn get_context_key_value(&self, key: &str) -> Dynamic {
        match self.overlay.get(key) {
            Some(value) => value.clone(),
            None => self.parent.get_context_key_value(key),
        }
    }

    fn create_scoped(&self, _target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        errors::throw(Error::new("Not supported."))
    }

    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        OverlayContextKeyService::new(self.me.upgrade().expect("o serviço está vivo enquanto é usado"), overlay)
    }

    fn update_parent(&self, _parent_context_key_service: Rc<dyn IContextKeyService>) {
        errors::throw(Error::new("Not supported."))
    }

    fn context_id(&self) -> i64 {
        self.parent.context_id()
    }
}

/// `setContext` (o comando `_setContext`): cria a chave com o valor, trocando URIs por texto.
pub fn set_context(context_key_service: &dyn IContextKeyService, context_key: &Dynamic, context_value: &Dynamic) {
    context_key_service.create_key(&to_js_string(context_key).to_string_lossy(), stringify_uris(context_value));
}

/// `$mid === MarshalledId.Uri` vira o texto da URI (`URI.revive(obj).toString()`).
fn stringify_uris(context_value: &Dynamic) -> Dynamic {
    objects::clone_and_change(context_value, &|obj| {
        if matches!(obj, Dynamic::Object(_)) && obj.get_str("$mid").strict_equals(&Dynamic::Number(f64::from(je_base::marshalling_ids::MarshalledId::Uri as i32))) {
            let part = |name: &str| match obj.get_str(name) {
                Dynamic::String(s) => Some(s),
                _ => None,
            };
            let components = UriComponents {
                scheme: part("scheme"),
                authority: part("authority"),
                path: part("path"),
                query: part("query"),
                fragment: part("fragment"),
            };
            return Dynamic::String(URI::from(&components, false).to_string(false));
        }
        Dynamic::Undefined
    })
}
