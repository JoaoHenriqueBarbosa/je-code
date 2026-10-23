//! Porte de `src/vs/platform/extensionManagement/common/extensionNls.ts`: a localização do manifesto, que troca
//! cada `%chave%` pelo texto do `package.nls.json` (ou do pacote de idioma).
//!
//! O manifesto é o JSON cru: a troca é uma travessia de `serde_json::Value` no lugar, com a mesma ordem e as mesmas
//! regras do original (o título e a categoria de um comando viram `{ value, original }` quando o texto traduzido
//! difere do original).

use je_platform::log::Logger;
use serde_json::{Map, Value};

/// `ITranslations`: cada valor é um texto ou `{ message, comment }`.
pub type Translations = Map<String, Value>;

/// O texto de uma entrada de tradução: o próprio texto ou o `message` do objeto.
fn message_of(entry: Option<&Value>) -> Option<&str> {
    match entry? {
        Value::String(text) => Some(text),
        Value::Object(object) => object.get("message").and_then(Value::as_str),
        _ => None,
    }
}

/// `localizeManifest`. Erros são registrados e ignorados: a localização é de melhor esforço (#164630).
pub fn localize_manifest(logger: &dyn Logger, manifest: &mut Value, translations: &Translations, fallback_translations: Option<&Translations>) {
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or("undefined").to_owned();
    if let Value::Object(root) = manifest {
        for key in root.keys().cloned().collect::<Vec<_>>() {
            process_entry(logger, &name, root.get_mut(&key).expect("a chave acabou de ser listada"), &key, translations, fallback_translations, false);
        }
    }
}

/// `processEntry` do original, sobre o valor de uma entrada (`obj[key]`).
fn process_entry(
    logger: &dyn Logger,
    manifest_name: &str,
    value: &mut Value,
    key: &str,
    messages: &Translations,
    original_messages: Option<&Translations>,
    command: bool,
) {
    match value {
        Value::String(text) => {
            let length = text.chars().count();
            if length > 1 && text.starts_with('%') && text.ends_with('%') {
                let message_key = &text[1..text.len() - 1];
                let mut translated = messages.get(message_key);
                // If the messages come from a language pack they might miss some keys
                // Fill them from the original messages.
                if translated.is_none() {
                    translated = original_messages.and_then(|original| original.get(message_key));
                }
                let message = message_of(translated).filter(|m| !m.is_empty());

                // This branch returns ILocalizedString's instead of Strings so that the Command Palette can contain
                // both the localized and the original value.
                let original = original_messages.and_then(|original| original.get(message_key));
                let original_message = message_of(original).filter(|m| !m.is_empty());

                let Some(message) = message else {
                    if original_message.is_none() {
                        logger.warn(&format!("[{manifest_name}]: Couldn't find message for key {message_key}."));
                    }
                    return;
                };

                *value = match original_message {
                    // if we are translating the title or category of a command and the original value is not the
                    // same as the translated value
                    Some(original_message) if command && (key == "title" || key == "category") && original_message != message => {
                        let mut localized = Map::new();
                        localized.insert("value".to_owned(), Value::from(message));
                        localized.insert("original".to_owned(), Value::from(original_message));
                        Value::Object(localized)
                    }
                    _ => Value::from(message),
                };
            }
        }
        Value::Object(object) => {
            for child_key in object.keys().cloned().collect::<Vec<_>>() {
                let child_command = if child_key == "commands" { true } else { command };
                process_entry(logger, manifest_name, object.get_mut(&child_key).expect("chave listada"), &child_key, messages, original_messages, child_command);
            }
        }
        Value::Array(items) => {
            for item in items {
                process_entry(logger, manifest_name, item, "", messages, original_messages, command);
            }
        }
        _ => {}
    }
}
