//! Porte de `src/vs/platform/policy/common/policy.ts`.
//!
//! Decisões de tradução:
//! - **`PolicyDefinition`** é compartilhado e comparado por identidade (`this.policyDefinitions[name] !==
//!   policyDefinitions[name]`): o handle é `Rc<PolicyDefinition>`, comparado com `Rc::ptr_eq`. O callback
//!   `value(policyData)` e `managedSettings` dependem de tipos de `vs/base/common` que não estão portados
//!   (`IPolicyData`, `IManagedSettingsPolicyDefinitions`) e ficam como valores dinâmicos.
//! - **`IStringDictionary<T>`** é uma lista de pares na ordem de inserção.
//! - **`instanceof NullPolicyService`** é [`PolicyService::is_null`].
//! - **`AbstractPolicyService`** é uma struct que recebe o método abstrato `_updatePolicyDefinitions` como
//!   um fecho ([`PolicyUpdater`]); quem "herda" guarda o `Rc<AbstractPolicyService>` e delega o trait.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;

/// `PolicyValue`: `string | number | boolean`.
#[derive(Clone, Debug, PartialEq)]
pub enum PolicyValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

impl PolicyValue {
    /// O valor como [`Dynamic`].
    pub fn to_dynamic(&self) -> Dynamic {
        match self {
            Self::String(text) => Dynamic::string(text),
            Self::Number(number) => Dynamic::Number(*number),
            Self::Boolean(value) => Dynamic::Bool(*value),
        }
    }

    /// De um [`Dynamic`] com a forma de um `PolicyValue`; qualquer outro tipo é `None`.
    pub fn from_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(text) => Some(Self::String(text.to_string_lossy())),
            Dynamic::Number(number) => Some(Self::Number(*number)),
            Dynamic::Bool(value) => Some(Self::Boolean(*value)),
            _ => None,
        }
    }
}

/// `PolicyValueSource`: a família da fonte que produziu um valor de política efetivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyValueSource {
    Device,
    NativeMdm,
    ServerManagedSettings,
    FileManagedSettings,
    MixedManagedSettings,
    Account,
    AccountGate,
}

/// `type: 'string' | 'number' | 'boolean'` de `PolicyDefinition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyDefinitionType {
    String,
    Number,
    Boolean,
}

/// `PolicyDefinition`.
#[derive(Clone, Debug)]
pub struct PolicyDefinition {
    pub type_: PolicyDefinitionType,
    /// `value?: (policyData) => ...`: deve ser pura e determinística. Valor dinâmico (uma função).
    pub value: Option<Dynamic>,
    pub managed_settings: Option<Dynamic>,
    pub restricted_value: Option<PolicyValue>,
}

impl PolicyDefinition {
    pub fn new(type_: PolicyDefinitionType) -> Self {
        Self { type_, value: None, managed_settings: None, restricted_value: None }
    }
}

/// `toSerializablePolicyDefinition(definition)`: sem o callback `value`, que não se clona.
pub fn to_serializable_policy_definition(definition: &PolicyDefinition) -> PolicyDefinition {
    PolicyDefinition { type_: definition.type_, value: None, managed_settings: definition.managed_settings.clone(), restricted_value: definition.restricted_value.clone() }
}

/// `getRestrictedPolicyValue(definition)`: o valor quando o portão de política de conta está ativo mas
/// não satisfeito.
pub fn get_restricted_policy_value(definition: &PolicyDefinition) -> PolicyValue {
    if let Some(restricted) = &definition.restricted_value {
        return restricted.clone();
    }
    match definition.type_ {
        PolicyDefinitionType::Boolean => PolicyValue::Boolean(false),
        PolicyDefinitionType::Number => PolicyValue::Number(0.0),
        PolicyDefinitionType::String => PolicyValue::String(String::new()),
    }
}

/// O handle compartilhado de uma definição.
pub type PolicyDefinitionRef = Rc<PolicyDefinition>;

/// Um `IStringDictionary` de definições.
pub type PolicyDefinitions = Vec<(String, PolicyDefinitionRef)>;

/// Um `IStringDictionary` de valores.
pub type PolicyValues = Vec<(String, PolicyValue)>;

/// O elemento de `serialize()`: `{ definition, value }`.
#[derive(Clone, Debug)]
pub struct SerializedPolicy {
    pub definition: PolicyDefinition,
    pub value: Option<PolicyValue>,
}

/// `IPolicyService`.
pub trait PolicyService {
    fn on_did_change(&self) -> Event<Vec<String>>;
    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues>;
    fn get_policy_value(&self, name: &str) -> Option<PolicyValue>;
    /// Returns the source of the effective value, or `None` when no value is set.
    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource>;
    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>>;
    fn policy_definitions(&self) -> PolicyDefinitions;

    /// `policyService instanceof NullPolicyService`.
    fn is_null(&self) -> bool {
        false
    }
}

/// O método abstrato `_updatePolicyDefinitions(policyDefinitions)`.
pub type PolicyUpdater = Rc<dyn Fn(&Rc<AbstractPolicyService>) -> Promise<()>>;

/// `AbstractPolicyService`.
pub struct AbstractPolicyService {
    base: Rc<Disposable>,
    policy_definitions: RefCell<PolicyDefinitions>,
    policies: RefCell<OrderedMap<String, PolicyValue>>,
    policy_value_sources: RefCell<OrderedMap<String, PolicyValueSource>>,
    on_did_change: Rc<Emitter<Vec<String>>>,
    updater: PolicyUpdater,
}

impl AbstractPolicyService {
    pub fn new(updater: PolicyUpdater) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new(Self {
            base,
            policy_definitions: RefCell::new(Vec::new()),
            policies: RefCell::new(OrderedMap::new()),
            policy_value_sources: RefCell::new(OrderedMap::new()),
            on_did_change,
            updater,
        })
    }

    /// `this.policies`.
    pub fn policies(&self) -> OrderedMap<String, PolicyValue> {
        self.policies.borrow().clone()
    }

    /// `this.policies = policies`.
    pub fn set_policies(&self, policies: OrderedMap<String, PolicyValue>) {
        *self.policies.borrow_mut() = policies;
    }

    /// `this._onDidChange.fire(names)`.
    pub fn fire_did_change(&self, names: Vec<String>) {
        self.on_did_change.fire(names);
    }

    /// A definição de uma política, se registrada (`this.policyDefinitions[name]`).
    pub fn definition(&self, name: &str) -> Option<PolicyDefinitionRef> {
        self.policy_definitions.borrow().iter().find(|(n, _)| n == name).map(|(_, d)| d.clone())
    }

    fn get_policy_values(&self) -> PolicyValues {
        self.policies.borrow().iter().map(|(name, value)| (name.clone(), value.clone())).collect()
    }

    fn get_stored_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        if !self.policies.borrow().contains_key(&name.to_owned()) {
            return None;
        }
        Some(self.policy_value_sources.borrow().get(&name.to_owned()).copied().unwrap_or(PolicyValueSource::Device))
    }

    /// `updatePolicyValue(name, value, source)`: devolve se algo mudou.
    pub fn update_policy_value(&self, name: &str, value: Option<PolicyValue>, source: PolicyValueSource) -> bool {
        let name = name.to_owned();
        let Some(value) = value else {
            let value_deleted = self.policies.borrow_mut().remove(&name).is_some();
            let source_deleted = self.policy_value_sources.borrow_mut().remove(&name).is_some();
            return value_deleted || source_deleted;
        };

        let value_changed = self.policies.borrow().get(&name) != Some(&value);
        let source_changed = self.get_stored_policy_value_source(&name) != Some(source);
        if !value_changed && !source_changed {
            return false;
        }

        self.policies.borrow_mut().insert(name.clone(), value);
        self.policy_value_sources.borrow_mut().insert(name, source);
        true
    }

    /// `clearPolicyValues()`.
    pub fn clear_policy_values(&self) {
        self.policies.borrow_mut().clear();
        self.policy_value_sources.borrow_mut().clear();
    }

    /// `updatePolicyDefinitions(policyDefinitions)`.
    pub fn update_policy_definitions(self: &Rc<Self>, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        // Replace existing definitions; identity comparison avoids redundant watcher churn.
        let mut changed = false;
        for (name, definition) in policy_definitions {
            let mut definitions = self.policy_definitions.borrow_mut();
            match definitions.iter_mut().find(|(n, _)| *n == name) {
                Some(existing) => {
                    if !Rc::ptr_eq(&existing.1, &definition) {
                        existing.1 = definition;
                        changed = true;
                    }
                }
                None => {
                    definitions.push((name, definition));
                    changed = true;
                }
            }
        }

        let this = self.clone();
        if changed { (self.updater)(self).then(move |()| this.get_policy_values()) } else { Promise::resolved(self.get_policy_values()) }
    }
}

impl PolicyService for Rc<AbstractPolicyService> {
    fn on_did_change(&self) -> Event<Vec<String>> {
        self.on_did_change.event()
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        AbstractPolicyService::update_policy_definitions(self, policy_definitions)
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.policies.borrow().get(&name.to_owned()).cloned()
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        self.get_stored_policy_value_source(name)
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        Some(
            self.policy_definitions
                .borrow()
                .iter()
                .map(|(name, definition)| {
                    (name.clone(), SerializedPolicy { definition: to_serializable_policy_definition(definition), value: self.policies.borrow().get(name).cloned() })
                })
                .collect(),
        )
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        self.policy_definitions.borrow().clone()
    }
}

impl IDisposable for AbstractPolicyService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `NullPolicyService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullPolicyService;

impl PolicyService for NullPolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        Event::none()
    }

    fn update_policy_definitions(&self, _policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        Promise::resolved(Vec::new())
    }

    fn get_policy_value(&self, _name: &str) -> Option<PolicyValue> {
        None
    }

    fn get_policy_value_source(&self, _name: &str) -> Option<PolicyValueSource> {
        None
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        None
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        Vec::new()
    }

    fn is_null(&self) -> bool {
        true
    }
}
