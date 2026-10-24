//! Porte de `src/vs/platform/policy/common/policyIpc.ts`: o canal de IPC do serviço de políticas (o lado do
//! servidor, [`PolicyChannel`], e o cliente, [`PolicyChannelClient`]).
//!
//! Decisões de tradução:
//! - Os valores que atravessam o canal são [`Marshalled`]. Um `PolicyValue` viaja como texto, número ou booleano;
//!   um dicionário (`IStringDictionary`) como um objeto, com `null` para a política sem valor. Um
//!   `PolicyDefinition` viaja como o JSON do original: `type`, `managedSettings` e `restrictedValue`; o callback
//!   `value` é uma função e o `JSON.stringify` o omite, então ele não chega ao servidor.
//! - Os `managedSettings` de uma definição são um [`Dynamic`](je_base::dynamic::Dynamic) e passam pelas mesmas
//!   conversões das configurações brutas (`raw_managed_settings_to_marshalled`).
//! - `IServerChannel.call` com um comando desconhecido lança (`throw`), como o original; um argumento que não é um
//!   dicionário de definições válidas rejeita a promessa (o tipo do original impede essa forma).
//! - **`extends AbstractPolicyService`** é uma [`AbstractPolicyService`] guardada por composição. O método
//!   abstrato `_updatePolicyDefinitions(policyDefinitions)` do cliente recebe a lista que a chamada passou, e o
//!   [`PolicyUpdater`] da base não a recebe: o cliente a guarda antes de delegar (a base chama o atualizador de
//!   forma síncrona, logo em seguida).
//! - O ouvinte de `onDidChange` do cliente fica preso ao cliente (o original não o descarta nunca).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::{IChannel, IServerChannel};
use je_base::promise::Promise;

use super::file_managed_settings_ipc::{raw_managed_settings_from_marshalled, raw_managed_settings_to_marshalled};
use super::policy::{
    AbstractPolicyService, PolicyDefinition, PolicyDefinitionType, PolicyDefinitions, PolicyService, PolicyUpdater, PolicyValue, PolicyValueSource, PolicyValues, SerializedPolicy,
};

/// Um dicionário como valor de canal; uma chave repetida fica com o último valor, na posição do primeiro.
fn dictionary_to_marshalled(entries: IndexMap<String, Marshalled>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(key, value)| (JsString::from(key.as_str()), value)).collect())
}

/// As entradas de um objeto de canal; qualquer outro valor não tem entradas (o `for...in` não itera nada).
fn dictionary_entries(value: &Marshalled) -> &[(JsString, Marshalled)] {
    match value {
        Marshalled::Object(entries) => entries,
        _ => &[],
    }
}

/// A propriedade `name` de um objeto de canal.
fn property<'a>(entries: &'a [(JsString, Marshalled)], name: &str) -> Option<&'a Marshalled> {
    entries.iter().find(|(key, _)| *key == name).map(|(_, value)| value)
}

/// O `PolicyValue` como valor de canal.
pub fn policy_value_to_marshalled(value: &PolicyValue) -> Marshalled {
    match value {
        PolicyValue::String(text) => Marshalled::String(JsString::from(text.as_str())),
        PolicyValue::Number(number) => Marshalled::Number(*number),
        PolicyValue::Boolean(flag) => Marshalled::Bool(*flag),
    }
}

/// O inverso de [`policy_value_to_marshalled`]; o que não é texto, número nem booleano é `None`.
pub fn policy_value_from_marshalled(value: &Marshalled) -> Option<PolicyValue> {
    match value {
        Marshalled::String(text) => Some(PolicyValue::String(text.to_string_lossy())),
        Marshalled::Number(number) => Some(PolicyValue::Number(*number)),
        Marshalled::Bool(flag) => Some(PolicyValue::Boolean(*flag)),
        _ => None,
    }
}

/// Os valores de políticas (um `IStringDictionary<PolicyValue>`) como valor de canal.
pub fn policy_values_to_marshalled(values: &PolicyValues) -> Marshalled {
    dictionary_to_marshalled(values.iter().map(|(name, value)| (name.clone(), policy_value_to_marshalled(value))).collect())
}

fn policy_definition_type_name(type_: PolicyDefinitionType) -> &'static str {
    match type_ {
        PolicyDefinitionType::String => "string",
        PolicyDefinitionType::Number => "number",
        PolicyDefinitionType::Boolean => "boolean",
    }
}

/// Um `PolicyDefinition` como o JSON do original o escreve: sem o callback `value`.
pub fn policy_definition_to_marshalled(definition: &PolicyDefinition) -> Marshalled {
    let mut entries = vec![(JsString::from("type"), Marshalled::String(JsString::from(policy_definition_type_name(definition.type_))))];
    if let Some(managed_settings) = &definition.managed_settings {
        entries.push((JsString::from("managedSettings"), raw_managed_settings_to_marshalled(managed_settings)));
    }
    if let Some(restricted_value) = &definition.restricted_value {
        entries.push((JsString::from("restrictedValue"), policy_value_to_marshalled(restricted_value)));
    }
    Marshalled::Object(entries)
}

/// O inverso de [`policy_definition_to_marshalled`].
pub fn policy_definition_from_marshalled(value: &Marshalled) -> Result<PolicyDefinition, Error> {
    let entries = match value {
        Marshalled::Object(entries) => entries,
        _ => return Err(Error::new("Invalid policy definition: expected an object")),
    };
    let type_ = match property(entries, "type") {
        Some(Marshalled::String(text)) => match text.to_string_lossy().as_str() {
            "string" => PolicyDefinitionType::String,
            "number" => PolicyDefinitionType::Number,
            "boolean" => PolicyDefinitionType::Boolean,
            other => return Err(Error::new(format!("Invalid policy definition type: {other}"))),
        },
        _ => return Err(Error::new("Invalid policy definition: missing type")),
    };
    let mut definition = PolicyDefinition::new(type_);
    definition.managed_settings = property(entries, "managedSettings").filter(|value| !matches!(value, Marshalled::Undefined)).map(raw_managed_settings_from_marshalled);
    definition.restricted_value = property(entries, "restrictedValue").and_then(policy_value_from_marshalled);
    Ok(definition)
}

/// As definições (um `IStringDictionary<PolicyDefinition>`) como valor de canal.
pub fn policy_definitions_to_marshalled(definitions: &PolicyDefinitions) -> Marshalled {
    dictionary_to_marshalled(definitions.iter().map(|(name, definition)| (name.clone(), policy_definition_to_marshalled(definition))).collect())
}

/// O inverso de [`policy_definitions_to_marshalled`]: cada definição vem como um objeto novo, sem identidade
/// compartilhada com a do chamador.
pub fn policy_definitions_from_marshalled(value: &Marshalled) -> Result<PolicyDefinitions, Error> {
    if !matches!(value, Marshalled::Object(_)) {
        return Err(Error::new("Invalid policy definitions: expected an object"));
    }
    dictionary_entries(value).iter().map(|(name, definition)| Ok((name.to_string_lossy(), Rc::new(policy_definition_from_marshalled(definition)?)))).collect()
}

/// `PolicyChannel`.
pub struct PolicyChannel {
    service: Rc<dyn PolicyService>,
    on_did_change_event: Event<Marshalled>,
    disposables: Rc<DisposableStore>,
}

impl PolicyChannel {
    pub fn new(service: Rc<dyn PolicyService>) -> Self {
        let disposables = DisposableStore::new();
        let source = service.clone();
        let on_did_change_event = service.on_did_change().map(
            move |names| {
                let values: IndexMap<String, Marshalled> =
                    names.iter().map(|name| (name.clone(), source.get_policy_value(name).map_or(Marshalled::Null, |value| policy_value_to_marshalled(&value)))).collect();
                dictionary_to_marshalled(values)
            },
            Some(&*disposables),
        );
        Self { service, on_did_change_event, disposables }
    }
}

impl IServerChannel for PolicyChannel {
    fn listen(&self, _ctx: &Marshalled, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        match event {
            "onDidChange" => self.on_did_change_event.clone(),
            _ => errors::throw(Error::new(format!("Event not found: {event}"))),
        }
    }

    fn call(&self, _ctx: &Marshalled, command: &str, arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "updatePolicyDefinitions" => match policy_definitions_from_marshalled(&arg) {
                Ok(definitions) => self.service.update_policy_definitions(definitions).then(|values| policy_values_to_marshalled(&values)),
                Err(error) => Promise::rejected(error),
            },
            _ => errors::throw(Error::new(format!("Call not found: {command}"))),
        }
    }
}

impl IDisposable for PolicyChannel {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `PolicyChannelClient`.
pub struct PolicyChannelClient {
    base: Rc<Disposable>,
    service: Rc<AbstractPolicyService>,
    /// O argumento da chamada em curso de `updatePolicyDefinitions`, para o atualizador da base.
    pending_definitions: Rc<RefCell<PolicyDefinitions>>,
}

impl PolicyChannelClient {
    pub fn new(policies_data: Vec<(String, SerializedPolicy)>, channel: Rc<dyn IChannel>) -> Self {
        let pending_definitions: Rc<RefCell<PolicyDefinitions>> = Rc::default();

        // `_updatePolicyDefinitions(policyDefinitions)`
        let updater: PolicyUpdater = {
            let (channel, pending_definitions) = (channel.clone(), pending_definitions.clone());
            Rc::new(move |service: &Rc<AbstractPolicyService>| {
                let service = service.clone();
                channel.call("updatePolicyDefinitions", policy_definitions_to_marshalled(&pending_definitions.borrow()), CancellationToken::None).then(move |result| {
                    for (name, value) in dictionary_entries(&result) {
                        if let Some(value) = policy_value_from_marshalled(value) {
                            service.update_policy_value(&name.to_string_lossy(), Some(value), PolicyValueSource::Device);
                        }
                    }
                })
            })
        };
        let service = AbstractPolicyService::new(updater);

        for (name, SerializedPolicy { definition, value }) in policies_data {
            service.set_definition(&name, Rc::new(definition));
            if let Some(value) = value {
                service.update_policy_value(&name, Some(value), PolicyValueSource::Device);
            }
        }

        let base = Disposable::new();
        let listener_service = service.clone();
        base.register(channel.listen("onDidChange", Marshalled::Undefined).listen(move |policies| {
            let entries = dictionary_entries(policies);
            for (name, value) in entries {
                let name = name.to_string_lossy();
                match value {
                    Marshalled::Null => {
                        listener_service.update_policy_value(&name, None, PolicyValueSource::Device);
                    }
                    value => {
                        if let Some(value) = policy_value_from_marshalled(value) {
                            listener_service.update_policy_value(&name, Some(value), PolicyValueSource::Device);
                        }
                    }
                }
            }
            listener_service.fire_did_change(entries.iter().map(|(name, _)| name.to_string_lossy()).collect());
        }));

        Self { base, service, pending_definitions }
    }
}

impl PolicyService for PolicyChannelClient {
    fn on_did_change(&self) -> Event<Vec<String>> {
        self.service.on_did_change()
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        *self.pending_definitions.borrow_mut() = policy_definitions.clone();
        self.service.update_policy_definitions(policy_definitions)
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.service.get_policy_value(name)
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        self.service.get_policy_value_source(name)
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        self.service.serialize()
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        PolicyService::policy_definitions(&self.service)
    }
}

impl IDisposable for PolicyChannelClient {
    fn dispose(&self) {
        self.base.dispose();
        self.service.dispose();
    }
}
