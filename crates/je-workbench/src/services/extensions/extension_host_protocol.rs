//! Porte parcial de `src/vs/workbench/services/extensions/common/extensionHostProtocol.ts`: o `IExtensionDescriptionDelta` (que o
//! `IExtensionHostManager.deltaExtensions` recebe do `AbstractExtensionService`) e o `IExtensionDescriptionSnapshot`, mais a
//! codificação em [`Dynamic`] (o `JSON` do RPC) das formas que cruzam o protocolo com o host: identificador de extensão, motivo de
//! ativação, descrição de extensão e delta.
//!
//! Fora do porte (entram com quem os lê): `IExtensionHostInitData`, `UIKind`, `RemoteTrustOption`, `ExtensionHostExitCode` e as
//! mensagens do protocolo (`MessageType`, `createMessageOfType`): o host em processo não faz o aperto de mãos por mensagens.
//!
//! Divergências:
//! - `{ [extensionId: string]: string[] }` é `HashMap<String, Vec<String>>` (a chave é o `ExtensionIdentifier.toKey`).
//! - **Forma no fio.** O `IExtensionDescription` do original é o manifesto espalhado com os campos de execução ao lado (`identifier`,
//!   `isBuiltin`, `extensionLocation`...). O porte guarda o `ExtensionDescription` do `je-extensions` (manifesto tipado e o local como
//!   texto), então a forma no fio é a do original com `extensionLocation` como o texto da URI (não as `UriComponents`); o manifesto
//!   passa pelo `serde` e as chaves de execução saem do objeto antes de ele voltar a ser manifesto. O `ExtensionIdentifier` viaja como
//!   `{ value, _lower }`, o mesmo objeto do original.

use std::collections::HashMap;

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{Error, SerializedError};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use serde_json::Value;

use super::extension_activation::ExtensionActivationReason;
use super::extension_description_registry::SharedExtensionDescription;
use super::rpc_protocol::parse_json_and_restore_buffer_refs;

/// `IExtensionDescriptionDelta`.
#[derive(Clone, Debug)]
pub struct IExtensionDescriptionDelta {
    pub version_id: i64,
    pub to_remove: Vec<ExtensionIdentifier>,
    pub to_add: Vec<SharedExtensionDescription>,
    pub add_activation_events: HashMap<String, Vec<String>>,
    pub my_to_remove: Vec<ExtensionIdentifier>,
    pub my_to_add: Vec<ExtensionIdentifier>,
}

/// `IExtensionDescriptionSnapshot`.
#[derive(Clone, Debug)]
pub struct IExtensionDescriptionSnapshot {
    pub version_id: i64,
    pub all_extensions: Vec<SharedExtensionDescription>,
    pub my_extensions: Vec<ExtensionIdentifier>,
    pub activation_events: HashMap<String, Vec<String>>,
}

// #region Codificação no fio

fn field(value: &Dynamic, name: &str) -> Dynamic {
    match value {
        Dynamic::Object(_) => value.get_str(name),
        _ => Dynamic::Undefined,
    }
}

fn dynamic_to_json(value: &Dynamic) -> Result<Value, Error> {
    let text = json_stringify(value).ok_or_else(|| Error::new("o valor não é serializável como JSON"))?;
    serde_json::from_str(&text).map_err(|error| Error::new(format!("JSON inválido no protocolo do host de extensões: {error}")))
}

fn json_to_dynamic(value: &Value) -> Dynamic {
    parse_json_and_restore_buffer_refs(&value.to_string(), &[])
}

fn text_field(value: &Dynamic, name: &str) -> Result<String, Error> {
    match field(value, name) {
        Dynamic::String(text) => Ok(text.to_string_lossy()),
        _ => Err(Error::new(format!("campo '{name}' ausente ou não é texto"))),
    }
}

fn number_field(value: &Dynamic, name: &str) -> Result<f64, Error> {
    match field(value, name) {
        Dynamic::Number(number) => Ok(number),
        _ => Err(Error::new(format!("campo '{name}' ausente ou não é número"))),
    }
}

fn list_field(value: &Dynamic, name: &str) -> Result<Vec<Dynamic>, Error> {
    match field(value, name) {
        Dynamic::Array(items) => Ok(items.to_vec()),
        _ => Err(Error::new(format!("campo '{name}' ausente ou não é lista"))),
    }
}

/// O `ExtensionIdentifier` como o objeto do original: `{ value, _lower }`.
pub fn extension_identifier_to_dynamic(id: &ExtensionIdentifier) -> Dynamic {
    Dynamic::object([("value", Dynamic::string(&id.value)), ("_lower", Dynamic::string(id.lower()))])
}

/// O inverso de [`extension_identifier_to_dynamic`].
pub fn extension_identifier_from_dynamic(value: &Dynamic) -> Result<ExtensionIdentifier, Error> {
    Ok(ExtensionIdentifier::new(text_field(value, "value")?))
}

/// O `ExtensionActivationReason` como objeto.
pub fn extension_activation_reason_to_dynamic(reason: &ExtensionActivationReason) -> Dynamic {
    Dynamic::object([
        ("startup", Dynamic::Bool(reason.startup)),
        ("extensionId", extension_identifier_to_dynamic(&reason.extension_id)),
        ("activationEvent", Dynamic::string(&reason.activation_event)),
    ])
}

/// O inverso de [`extension_activation_reason_to_dynamic`].
pub fn extension_activation_reason_from_dynamic(value: &Dynamic) -> Result<ExtensionActivationReason, Error> {
    Ok(ExtensionActivationReason {
        startup: matches!(field(value, "startup"), Dynamic::Bool(true)),
        extension_id: extension_identifier_from_dynamic(&field(value, "extensionId"))?,
        activation_event: text_field(value, "activationEvent")?,
    })
}

/// As chaves de execução do `IExtensionDescription` que ficam ao lado do manifesto no fio.
const RUNTIME_KEYS: [&str; 9] =
    ["id", "identifier", "isBuiltin", "isUserBuiltin", "isUnderDevelopment", "extensionLocation", "uuid", "targetPlatform", "publisherDisplayName"];

/// O `IExtensionDescription` no fio (ver a nota do módulo).
pub fn extension_description_to_dynamic(description: &ExtensionDescription) -> Result<Dynamic, Error> {
    let manifest = serde_json::to_value(&description.manifest).map_err(|error| Error::new(format!("manifesto não serializável: {error}")))?;
    let Value::Object(mut object) = manifest else { return Err(Error::new("o manifesto não é um objeto")) };
    object.insert("id".into(), Value::String(description.id.clone()));
    object.insert("identifier".into(), serde_json::json!({ "value": description.identifier.value, "_lower": description.identifier.lower() }));
    object.insert("isBuiltin".into(), Value::Bool(description.is_builtin));
    object.insert("isUserBuiltin".into(), Value::Bool(description.is_user_builtin));
    object.insert("isUnderDevelopment".into(), Value::Bool(description.is_under_development));
    object.insert("extensionLocation".into(), Value::String(description.extension_location.clone()));
    if let Some(uuid) = &description.uuid {
        object.insert("uuid".into(), Value::String(uuid.clone()));
    }
    object.insert("targetPlatform".into(), serde_json::to_value(description.target_platform).map_err(|error| Error::new(error.to_string()))?);
    if let Some(name) = &description.publisher_display_name {
        object.insert("publisherDisplayName".into(), Value::String(name.clone()));
    }
    object.insert("preRelease".into(), Value::Bool(description.pre_release));
    Ok(json_to_dynamic(&Value::Object(object)))
}

/// O inverso de [`extension_description_to_dynamic`].
pub fn extension_description_from_dynamic(value: &Dynamic) -> Result<ExtensionDescription, Error> {
    let Value::Object(mut object) = dynamic_to_json(value)? else { return Err(Error::new("a descrição da extensão não é um objeto")) };
    let mut take = |key: &str| object.remove(key);
    let id = match take("id") {
        Some(Value::String(id)) => id,
        _ => return Err(Error::new("descrição de extensão sem 'id'")),
    };
    let identifier = match take("identifier") {
        Some(Value::Object(identifier)) => match identifier.get("value") {
            Some(Value::String(value)) => ExtensionIdentifier::new(value.clone()),
            _ => return Err(Error::new("descrição de extensão com 'identifier' inválido")),
        },
        _ => ExtensionIdentifier::new(id.clone()),
    };
    let flag = |value: Option<Value>| matches!(value, Some(Value::Bool(true)));
    let is_builtin = flag(take("isBuiltin"));
    let is_user_builtin = flag(take("isUserBuiltin"));
    let is_under_development = flag(take("isUnderDevelopment"));
    let extension_location = match take("extensionLocation") {
        Some(Value::String(location)) => location,
        _ => return Err(Error::new("descrição de extensão sem 'extensionLocation'")),
    };
    let uuid = match take("uuid") {
        Some(Value::String(uuid)) => Some(uuid),
        _ => None,
    };
    let target_platform = match take("targetPlatform") {
        Some(platform) => serde_json::from_value(platform).map_err(|error| Error::new(format!("'targetPlatform' inválido: {error}")))?,
        None => je_extensions::manifest::TargetPlatform::Undefined,
    };
    let publisher_display_name = match take("publisherDisplayName") {
        Some(Value::String(name)) => Some(name),
        _ => None,
    };
    let pre_release = flag(take("preRelease"));
    debug_assert!(RUNTIME_KEYS.iter().all(|key| !object.contains_key(*key)));
    let manifest = serde_json::from_value(Value::Object(object)).map_err(|error| Error::new(format!("manifesto inválido: {error}")))?;
    Ok(ExtensionDescription {
        id,
        identifier,
        is_builtin,
        is_user_builtin,
        is_under_development,
        extension_location,
        uuid,
        target_platform,
        publisher_display_name,
        pre_release,
        manifest,
    })
}

fn identifiers_to_dynamic(ids: &[ExtensionIdentifier]) -> Dynamic {
    Dynamic::array(ids.iter().map(extension_identifier_to_dynamic))
}

fn identifiers_from_dynamic(value: &Dynamic, name: &str) -> Result<Vec<ExtensionIdentifier>, Error> {
    list_field(value, name)?.iter().map(extension_identifier_from_dynamic).collect()
}

fn activation_events_to_dynamic(events: &HashMap<String, Vec<String>>) -> Dynamic {
    let mut keys: Vec<&String> = events.keys().collect();
    keys.sort();
    Dynamic::object(keys.into_iter().map(|key| (key.as_str(), Dynamic::array(events[key].iter().map(|event| Dynamic::string(event))))))
}

fn activation_events_from_dynamic(value: &Dynamic) -> Result<HashMap<String, Vec<String>>, Error> {
    let Dynamic::Object(object) = value else { return Err(Error::new("'addActivationEvents' não é um objeto")) };
    let mut result = HashMap::new();
    for (key, events) in object.entries() {
        let Dynamic::Array(events) = events else { return Err(Error::new("os eventos de ativação de uma extensão não são uma lista")) };
        let mut list = Vec::new();
        for event in events.to_vec() {
            match event {
                Dynamic::String(text) => list.push(text.to_string_lossy()),
                _ => return Err(Error::new("evento de ativação que não é texto")),
            }
        }
        result.insert(key.to_string_lossy(), list);
    }
    Ok(result)
}

impl IExtensionDescriptionDelta {
    /// O delta no fio (`$startExtensionHost` e `$deltaExtensions`).
    pub fn to_dynamic(&self) -> Result<Dynamic, Error> {
        let to_add = self.to_add.iter().map(|description| extension_description_to_dynamic(description)).collect::<Result<Vec<_>, _>>()?;
        Ok(Dynamic::object([
            ("versionId", Dynamic::Number(self.version_id as f64)),
            ("toRemove", identifiers_to_dynamic(&self.to_remove)),
            ("toAdd", Dynamic::array(to_add)),
            ("addActivationEvents", activation_events_to_dynamic(&self.add_activation_events)),
            ("myToRemove", identifiers_to_dynamic(&self.my_to_remove)),
            ("myToAdd", identifiers_to_dynamic(&self.my_to_add)),
        ]))
    }

    /// O inverso de [`IExtensionDescriptionDelta::to_dynamic`].
    pub fn from_dynamic(value: &Dynamic) -> Result<Self, Error> {
        let to_add = list_field(value, "toAdd")?
            .iter()
            .map(|description| extension_description_from_dynamic(description).map(std::rc::Rc::new))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            version_id: number_field(value, "versionId")? as i64,
            to_remove: identifiers_from_dynamic(value, "toRemove")?,
            to_add,
            add_activation_events: activation_events_from_dynamic(&field(value, "addActivationEvents"))?,
            my_to_remove: identifiers_from_dynamic(value, "myToRemove")?,
            my_to_add: identifiers_from_dynamic(value, "myToAdd")?,
        })
    }
}

/// Um objeto JSON de pares (`{ [key: string]: string | null }`) como lista de pares, e o inverso.
pub fn environment_to_dynamic(env: &[(String, Option<String>)]) -> Dynamic {
    Dynamic::object(env.iter().map(|(key, value)| (key.as_str(), value.as_ref().map_or(Dynamic::Null, |value| Dynamic::string(value)))))
}

/// O inverso de [`environment_to_dynamic`].
pub fn environment_from_dynamic(value: &Dynamic) -> Result<Vec<(String, Option<String>)>, Error> {
    let Dynamic::Object(object) = value else { return Err(Error::new("o ambiente remoto não é um objeto")) };
    object
        .entries()
        .into_iter()
        .map(|(key, value)| match value {
            Dynamic::String(text) => Ok((key.to_string_lossy(), Some(text.to_string_lossy()))),
            Dynamic::Null => Ok((key.to_string_lossy(), None)),
            _ => Err(Error::new("valor de ambiente que não é texto nem null")),
        })
        .collect()
}

/// Um `SerializedError` (`transformErrorForSerialization`) como o objeto do original: `{ $isError, name, message, stack, noTelemetry,
/// code, cause }`.
pub fn serialized_error_to_dynamic(error: &SerializedError) -> Dynamic {
    let optional = |value: &Option<String>| value.as_ref().map_or(Dynamic::Undefined, |text| Dynamic::string(text));
    Dynamic::object([
        ("$isError", Dynamic::Bool(true)),
        ("name", Dynamic::string(&error.name)),
        ("message", Dynamic::string(&error.message)),
        ("stack", optional(&error.stack)),
        ("noTelemetry", Dynamic::Bool(error.no_telemetry)),
        ("code", optional(&error.code)),
        ("cause", error.cause.as_deref().map_or(Dynamic::Undefined, serialized_error_to_dynamic)),
    ])
}

/// O inverso de [`serialized_error_to_dynamic`].
pub fn serialized_error_from_dynamic(value: &Dynamic) -> SerializedError {
    let text = |name: &str| match field(value, name) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    };
    SerializedError {
        name: text("name").unwrap_or_else(|| "Error".to_owned()),
        message: text("message").unwrap_or_default(),
        stack: text("stack"),
        no_telemetry: matches!(field(value, "noTelemetry"), Dynamic::Bool(true)),
        code: text("code"),
        cause: match field(value, "cause") {
            cause @ Dynamic::Object(_) => Some(Box::new(serialized_error_from_dynamic(&cause))),
            _ => None,
        },
    }
}

// #endregion
