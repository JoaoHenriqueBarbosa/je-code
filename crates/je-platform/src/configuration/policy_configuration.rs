//! Porte de `IPolicyConfiguration`, `NullPolicyConfiguration` e `PolicyConfiguration` de
//! `src/vs/platform/configuration/common/configurations.ts` (o `DefaultConfiguration` está em [`super::defaults`]).
//!
//! Decisões de tradução:
//! - As políticas e seus valores vêm do [`PolicyService`]; o esquema da propriedade (`policy`,
//!   `policyReference`, `type`) é lido do registro como valores dinâmicos, como no resto do porte.
//! - `async` + `await` viram [`Promise`]; o trecho síncrono do `async` roda de imediato, como no original.
//! - O `policy.value` e o `policy.managedSettings` ficam como valores dinâmicos opacos
//!   ([`PolicyDefinition`]), e `isSamePolicyDefinition` os compara por identidade.

use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{ArrayRef, Dynamic, ObjectRef, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseError, ParseErrorCode, ParseOptions, Value, Visitor};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::objects;
use je_base::promise::Promise;

use super::defaults::DefaultConfiguration;
use super::models::ConfigurationModel;
use super::registry::{PropertySchema, configuration_registry};
use crate::js::from_json;
use crate::log::{LogArg, LogServiceRef, NullLogService};
use crate::policy::{PolicyDefinition, PolicyDefinitionRef, PolicyDefinitionType, PolicyService, PolicyValue};

/// `IPolicyConfiguration`.
pub trait PolicyConfigurationApi {
    fn on_did_change_configuration(&self) -> Event<ConfigurationModel>;
    fn configuration_model(&self) -> ConfigurationModel;
    fn initialize(&self) -> Promise<ConfigurationModel>;
}

/// `NullPolicyConfiguration`.
pub struct NullPolicyConfiguration {
    configuration_model: ConfigurationModel,
}

impl NullPolicyConfiguration {
    pub fn new() -> Self {
        Self { configuration_model: ConfigurationModel::create_empty_model(NullLogService::shared()) }
    }
}

impl Default for NullPolicyConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyConfigurationApi for NullPolicyConfiguration {
    fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        Event::none()
    }

    fn configuration_model(&self) -> ConfigurationModel {
        self.configuration_model.clone()
    }

    fn initialize(&self) -> Promise<ConfigurationModel> {
        Promise::resolved(self.configuration_model.clone())
    }
}

fn strings_to_dynamic(items: &[JsString]) -> Dynamic {
    Dynamic::array(items.iter().map(|item| Dynamic::String(item.clone())))
}

/// O `ParsedType` do original: um objeto ou uma lista.
struct PolicyValueParser {
    root: ArrayRef,
    current_parent: Dynamic,
    previous_parents: Vec<Dynamic>,
    current_property: Option<JsString>,
    parse_errors: Vec<ParseError>,
    failure: Option<Error>,
}

impl PolicyValueParser {
    fn on_value(&mut self, value: Dynamic) -> ControlFlow<()> {
        match &self.current_parent {
            Dynamic::Array(array) => array.push(value),
            Dynamic::Object(object) => {
                if let Some(property) = &self.current_property {
                    if object.has_own(property) {
                        self.failure = Some(Error::new(format!("Duplicate property found: {property}")));
                        return ControlFlow::Break(());
                    }
                    object.set(property, value);
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn begin(&mut self, container: Dynamic) -> ControlFlow<()> {
        self.on_value(container.clone())?;
        self.previous_parents.push(std::mem::replace(&mut self.current_parent, container));
        self.current_property = None;
        ControlFlow::Continue(())
    }

    fn end(&mut self) -> ControlFlow<()> {
        self.current_parent = self.previous_parents.pop().unwrap_or(Dynamic::Undefined);
        ControlFlow::Continue(())
    }
}

impl Visitor for PolicyValueParser {
    fn on_object_begin(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.begin(Dynamic::Object(ObjectRef::new()))
    }

    fn on_object_property(&mut self, property: &JsString, _: usize, _: usize) -> ControlFlow<()> {
        self.current_property = Some(property.clone());
        ControlFlow::Continue(())
    }

    fn on_object_end(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.end()
    }

    fn on_array_begin(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.begin(Dynamic::Array(ArrayRef::new(Vec::new())))
    }

    fn on_array_end(&mut self, _: usize, _: usize) -> ControlFlow<()> {
        self.end()
    }

    fn on_literal_value(&mut self, value: &Value, _: usize, _: usize) -> ControlFlow<()> {
        self.on_value(from_json(value))
    }

    fn on_error(&mut self, error: ParseErrorCode, offset: usize, length: usize) -> ControlFlow<()> {
        self.parse_errors.push(ParseError { error, offset, length });
        ControlFlow::Continue(())
    }
}

/// `PolicyConfiguration.parse(content)`: o JSON de uma política de objeto ou lista; rejeita chave duplicada.
fn parse_policy_value(content: &str) -> Result<Dynamic, Error> {
    let mut parser = PolicyValueParser {
        root: ArrayRef::new(Vec::new()),
        current_parent: Dynamic::Undefined,
        previous_parents: Vec::new(),
        current_property: None,
        parse_errors: Vec::new(),
        failure: None,
    };
    parser.current_parent = Dynamic::Array(parser.root.clone());
    let mut raw = Dynamic::object::<&str>([]);

    if !content.is_empty() {
        let units: Vec<u16> = content.encode_utf16().collect();
        json::visit(&units, &mut parser, ParseOptions::default());
        if let Some(failure) = parser.failure.take() {
            return Err(failure);
        }
        let first = parser.root.get(0);
        if first.is_truthy() {
            raw = first;
        }
    }

    if !parser.parse_errors.is_empty() {
        // `getErrorMessage(e.error)` sobre o código numérico: o texto do número.
        return Err(Error::new(parser.parse_errors.iter().map(|e| (e.error as i32).to_string()).collect::<Vec<_>>().join("\n")));
    }

    Ok(raw)
}

/// `PolicyConfiguration`.
pub struct PolicyConfiguration {
    base: Rc<Disposable>,
    on_did_change_configuration: Rc<Emitter<ConfigurationModel>>,
    default_configuration: Rc<DefaultConfiguration>,
    policy_service: Rc<dyn PolicyService>,
    log_service: LogServiceRef,
    configuration_model: RefCell<ConfigurationModel>,
    /// Last definition submitted per policy name; avoids redundant re-registration.
    submitted_policy_definitions: RefCell<OrderedMap<String, PolicyDefinitionRef>>,
    /// Maps each policy-controlled setting key to its policy name, so removed keys can be re-resolved.
    policy_name_by_key: RefCell<OrderedMap<JsString, String>>,
}

impl PolicyConfiguration {
    pub fn new(default_configuration: Rc<DefaultConfiguration>, policy_service: Rc<dyn PolicyService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        Rc::new(Self {
            base,
            on_did_change_configuration,
            default_configuration,
            policy_service,
            configuration_model: RefCell::new(ConfigurationModel::create_empty_model(log_service.clone())),
            log_service,
            submitted_policy_definitions: RefCell::new(OrderedMap::new()),
            policy_name_by_key: RefCell::new(OrderedMap::new()),
        })
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        self.on_did_change_configuration.event()
    }

    /// `configurationModel`.
    pub fn configuration_model(&self) -> ConfigurationModel {
        self.configuration_model.borrow().clone()
    }

    fn trace_keys(&self, message: &str, keys: &[JsString]) {
        self.log_service.trace_args(message, &[LogArg::Value(strings_to_dynamic(keys))]);
    }

    /// `initialize()`.
    pub fn initialize(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        self.log_service.trace("PolicyConfiguration#initialize");

        let mut keys = self.default_configuration.configuration_model().keys();
        keys.extend(configuration_registry().get_excluded_configuration_properties().keys());
        let this = self.clone();
        self.update_policy_definitions(&keys).then(move |keys| {
            this.update(&keys, false);

            let weak = Rc::downgrade(&this);
            this.base.register(this.policy_service.on_did_change().listen(move |policy_names| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_policies(policy_names);
                }
            }));
            let weak = Rc::downgrade(&this);
            this.base.register(this.default_configuration.on_did_change_configuration().listen(move |change| {
                if let Some(this) = weak.upgrade() {
                    let target = this.clone();
                    let _ = this.update_policy_definitions(&change.properties).then(move |keys| target.update(&keys, true));
                }
            }));
            this.configuration_model()
        })
    }

    /// `toPolicyDefinitionType(configType, policyName)`.
    fn to_policy_definition_type(&self, config_type: &Dynamic, policy_name: &str) -> Option<PolicyDefinitionType> {
        // `configType` may be a single type or a union (e.g. `['array', 'null']`).
        // Normalize to an array and keep only the types we can represent as policies.
        let config_types: Vec<Dynamic> = match config_type {
            Dynamic::Array(items) => items.to_vec(),
            other => vec![other.clone()],
        };
        let supported: Vec<String> = config_types
            .iter()
            .filter_map(|t| match t {
                Dynamic::String(text) => Some(text.to_string_lossy()),
                _ => None,
            })
            .filter(|t| matches!(t.as_str(), "string" | "number" | "array" | "object" | "boolean"))
            .collect();
        if supported.is_empty() {
            self.log_service.warn(&format!(
                "PolicyConfiguration#updatePolicyDefinitions - policy '{policy_name}' has unsupported type '{}'",
                match config_type {
                    Dynamic::String(text) => text.to_string_lossy(),
                    Dynamic::Undefined => "undefined".to_owned(),
                    other => format!("{other:?}"),
                }
            ));
            return None;
        }
        Some(if supported.iter().any(|t| t == "number") {
            PolicyDefinitionType::Number
        } else if supported.iter().any(|t| t == "boolean") {
            PolicyDefinitionType::Boolean
        } else {
            PolicyDefinitionType::String
        })
    }

    fn property_of(&self, key: &JsString) -> Option<PropertySchema> {
        let registry = configuration_registry();
        registry.get_configuration_properties().get(key).or_else(|| registry.get_excluded_configuration_properties().get(key))
    }

    /// `updatePolicyDefinitions(properties)`.
    fn update_policy_definitions(&self, properties: &[JsString]) -> Promise<Vec<JsString>> {
        self.trace_keys("PolicyConfiguration#updatePolicyDefinitions", properties);
        let mut keys: Vec<JsString> = Vec::new();
        let mut policy_names: Vec<String> = Vec::new();
        let add_name = |names: &mut Vec<String>, name: String| {
            if !names.contains(&name) {
                names.push(name);
            }
        };

        for key in properties {
            let Some(config) = self.property_of(key) else {
                keys.push(key.clone()); // deregistered: update() will clear this key's applied policy value
                if let Some(removed) = self.policy_name_by_key.borrow_mut().remove(key) {
                    add_name(&mut policy_names, removed);
                }
                continue;
            };
            if let Some(policy_name) = config.policy_name().or_else(|| config.policy_reference_name()) {
                keys.push(key.clone());
                add_name(&mut policy_names, policy_name.clone());
                self.policy_name_by_key.borrow_mut().insert(key.clone(), policy_name);
            }
        }

        let mut changed_definitions: Vec<(String, PolicyDefinitionRef)> = Vec::new();
        for policy_name in policy_names {
            let definition = self.resolve_policy_definition(&policy_name);
            if let Some(definition) = definition {
                let previous = self.submitted_policy_definitions.borrow().get(&policy_name).cloned();
                if !Self::is_same_policy_definition(previous.as_deref(), &definition) {
                    self.submitted_policy_definitions.borrow_mut().insert(policy_name.clone(), definition.clone());
                    changed_definitions.push((policy_name, definition));
                }
            }
        }

        if !changed_definitions.is_empty() {
            return self.policy_service.update_policy_definitions(changed_definitions).then(move |_| keys);
        }

        Promise::resolved(keys)
    }

    fn is_same_policy_definition(a: Option<&PolicyDefinition>, b: &PolicyDefinition) -> bool {
        let same = |x: &Option<Dynamic>, y: &Option<Dynamic>| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => x.strict_equals(y),
            _ => false,
        };
        a.is_some_and(|a| a.type_ == b.type_ && same(&a.value, &b.value) && same(&a.managed_settings, &b.managed_settings) && a.restricted_value == b.restricted_value)
    }

    /// Resolve the authoritative definition: owner wins; references provide a bare type fallback.
    fn resolve_policy_definition(&self, policy_name: &str) -> Option<PolicyDefinitionRef> {
        let registry = configuration_registry();

        if let Some(owner_key) = registry.get_policy_configurations().get(&policy_name.to_owned()).cloned()
            && let Some(config) = self.property_of(&owner_key)
        {
            let policy = config.get("policy");
            if policy.is_truthy() {
                let type_ = self.to_policy_definition_type(&config.get("type"), policy_name)?;
                let optional = |name: &str| Some(policy.get_str(name)).filter(|v| !v.is_undefined());
                return Some(Rc::new(PolicyDefinition {
                    type_,
                    value: optional("value"),
                    managed_settings: optional("managedSettings"),
                    restricted_value: optional("restrictedValue").and_then(|v| PolicyValue::from_dynamic(&v)),
                }));
            }
        }

        let reference_keys = registry.get_policy_reference_configurations().get(&policy_name.to_owned()).cloned();
        for reference_key in reference_keys.unwrap_or_default() {
            if let Some(config) = self.property_of(&reference_key)
                && config.get("policyReference").is_truthy()
            {
                let type_ = self.to_policy_definition_type(&config.get("type"), policy_name)?;
                return Some(Rc::new(PolicyDefinition::new(type_)));
            }
        }

        None
    }

    fn on_did_change_policies(&self, policy_names: &[String]) {
        self.log_service.trace_args(
            "PolicyConfiguration#onDidChangePolicies",
            &[LogArg::Value(Dynamic::array(policy_names.iter().map(|name| Dynamic::string(name))))],
        );
        let registry = configuration_registry();
        let policy_configurations = registry.get_policy_configurations();
        let policy_reference_configurations = registry.get_policy_reference_configurations();
        let mut keys: Vec<JsString> = Vec::new();
        for policy_name in policy_names {
            if let Some(owner) = policy_configurations.get(policy_name) {
                keys.push(owner.clone());
            }
            if let Some(references) = policy_reference_configurations.get(policy_name) {
                keys.extend(references.iter().cloned());
            }
        }
        self.update(&keys, true);
    }

    fn update(&self, keys: &[JsString], trigger: bool) {
        self.trace_keys("PolicyConfiguration#update", keys);
        let mut changed: Vec<(JsString, Dynamic)> = Vec::new();
        let was_empty = self.configuration_model().is_empty();

        for key in keys {
            let property = self.property_of(key);
            let policy_name = property.as_ref().and_then(|p| p.policy_name().or_else(|| p.policy_reference_name()));
            if let (Some(property), Some(policy_name)) = (&property, policy_name) {
                let mut policy_value = self.policy_service.get_policy_value(&policy_name).map_or(Dynamic::Undefined, |v| v.to_dynamic());
                // `property.type` may be a single type or a union (e.g. `['array', 'null']`).
                // A string policy value carries a JSON payload that must be parsed unless the
                // setting itself is (or can be) a plain string.
                let property_type = property.get("type");
                let accepts_string_type = match &property_type {
                    Dynamic::Array(types) => types.to_vec().iter().any(|t| *t == Dynamic::string("string")),
                    other => *other == Dynamic::string("string"),
                };
                if let Dynamic::String(content) = &policy_value
                    && !accepts_string_type
                {
                    match parse_policy_value(&content.to_string_lossy()) {
                        Ok(parsed) => policy_value = parsed,
                        Err(error) => {
                            self.log_service.error_args(
                                &crate::log::LogMessage::from(format!("Error parsing policy value {policy_name}:")),
                                &[LogArg::from(error.message.as_str())],
                            );
                            continue;
                        }
                    }
                }
                let differs = if was_empty { !policy_value.is_undefined() } else { !objects::equals(&self.configuration_model().get_value(key), &policy_value) };
                if differs {
                    changed.push((key.clone(), policy_value));
                }
            } else if !self.configuration_model().get_value(key).is_undefined() {
                changed.push((key.clone(), Dynamic::Undefined));
            }
        }

        if !changed.is_empty() {
            self.log_service.trace_args(
                "PolicyConfiguration#changed",
                &[LogArg::Value(Dynamic::array(changed.iter().map(|(key, value)| Dynamic::array([Dynamic::String(key.clone()), value.clone()]))))],
            );
            let old = self.configuration_model();
            let new_model = ConfigurationModel::create_empty_model(self.log_service.clone());
            for key in old.keys() {
                new_model.set_value(&key, old.get_value(&key));
            }
            for (key, policy_value) in changed {
                if policy_value.is_undefined() {
                    new_model.remove_value(&key);
                } else {
                    new_model.set_value(&key, policy_value);
                }
            }
            *self.configuration_model.borrow_mut() = new_model.clone();
            if trigger {
                self.on_did_change_configuration.fire(new_model);
            }
        }
    }
}

impl PolicyConfigurationApi for Rc<PolicyConfiguration> {
    fn on_did_change_configuration(&self) -> Event<ConfigurationModel> {
        PolicyConfiguration::on_did_change_configuration(self)
    }

    fn configuration_model(&self) -> ConfigurationModel {
        PolicyConfiguration::configuration_model(self)
    }

    fn initialize(&self) -> Promise<ConfigurationModel> {
        PolicyConfiguration::initialize(self)
    }
}

impl IDisposable for PolicyConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}
