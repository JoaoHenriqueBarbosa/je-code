//! Porte de `src/vs/platform/extensionManagement/common/allowedExtensionsService.ts`: a política `extensions.allowed`
//! (quais extensões, publishers e versões podem ser instalados).
//!
//! A configuração entra como um leitor (`Fn() -> Option<Value>`) e o aviso de mudança como uma chamada explícita
//! ([`AllowedExtensionsService::on_configuration_changed`]): o serviço de configuração do `je-platform` é quem
//! observa o arquivo e chama.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{Emitter, Event};
use serde_json::Value;

use crate::management::{ALLOWED_EXTENSIONS_CONFIG_KEY, GalleryExtension, LocalExtension};
use crate::manifest::TargetPlatform;

/// O comando que abre a configuração, no formato `createCommandUri('workbench.action.openSettings', { query: '@id:extensions.allowed' }).toString()`.
const SETTINGS_COMMAND_LINK: &str = "command:workbench.action.openSettings?%7B%22query%22%3A%22%40id%3Aextensions.allowed%22%7D";

/// A extensão que `isAllowed` examina: uma da galeria, uma local ou só os campos (`{ id, publisherDisplayName, ... }`).
pub enum AllowedCandidate<'a> {
    Gallery(&'a GalleryExtension),
    Local(&'a LocalExtension),
    Fields { id: &'a str, publisher_display_name: Option<&'a str>, version: Option<&'a str>, prerelease: Option<bool>, target_platform: Option<TargetPlatform> },
}

/// O motivo (um `IMarkdownString` no original, só o texto) pelo qual a extensão não é permitida.
pub type NotAllowedReason = String;

/// `AllowedExtensionsService`.
pub struct AllowedExtensionsService {
    publisher_orgs: Vec<String>,
    read_configuration: Box<dyn Fn() -> Option<Value>>,
    value: RefCell<Option<IndexMap<String, Value>>>,
    on_did_change: Rc<Emitter<()>>,
}

impl AllowedExtensionsService {
    /// `publisher_orgs` é o `extensionPublisherOrgs` do produto; `read_configuration` lê `extensions.allowed`.
    pub fn new(publisher_orgs: &[String], read_configuration: impl Fn() -> Option<Value> + 'static) -> Self {
        let service = Self {
            publisher_orgs: publisher_orgs.iter().map(|p| p.to_lowercase()).collect(),
            read_configuration: Box::new(read_configuration),
            value: RefCell::new(None),
            on_did_change: Emitter::new(),
        };
        *service.value.borrow_mut() = service.get_allowed_extensions_value();
        service
    }

    pub fn on_did_change_allowed_extensions_config_value(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// O valor atual (`allowedExtensionsConfigValue`), com as chaves em minúscula; `None` libera tudo.
    pub fn allowed_extensions_config_value(&self) -> Option<IndexMap<String, Value>> {
        self.value.borrow().clone()
    }

    /// Chamar quando `affectsConfiguration('extensions.allowed')`.
    pub fn on_configuration_changed(&self) {
        *self.value.borrow_mut() = self.get_allowed_extensions_value();
        self.on_did_change.fire(());
    }

    /// A chave de configuração que este serviço lê.
    pub const CONFIG_KEY: &'static str = ALLOWED_EXTENSIONS_CONFIG_KEY;

    fn get_allowed_extensions_value(&self) -> Option<IndexMap<String, Value>> {
        let Some(Value::Object(value)) = (self.read_configuration)() else { return None };
        let entries: IndexMap<String, Value> = value.into_iter().map(|(key, value)| (key.to_lowercase(), value)).collect();
        if entries.len() == 1 && entries.get("*") == Some(&Value::Bool(true)) {
            return None;
        }
        Some(entries)
    }

    /// `isAllowed`: `Ok(())` quando permitida, ou o motivo.
    pub fn is_allowed(&self, extension: &AllowedCandidate<'_>) -> Result<(), NotAllowedReason> {
        let value_ref = self.value.borrow();
        let Some(config) = value_ref.as_ref() else { return Ok(()) };

        let (id, version, target_platform, prerelease, publisher, publisher_display_name): (String, String, TargetPlatform, bool, String, Option<String>) = match extension {
            AllowedCandidate::Gallery(e) => (
                e.identifier.id.to_lowercase(),
                e.version.clone(),
                e.properties.target_platform,
                e.properties.is_pre_release_version,
                e.publisher.to_lowercase(),
                Some(e.publisher_display_name.to_lowercase()),
            ),
            AllowedCandidate::Local(e) => (
                e.identifier.id.to_lowercase(),
                e.manifest.version.clone(),
                e.target_platform,
                e.pre_release,
                e.manifest.publisher.to_lowercase(),
                e.publisher_display_name.as_ref().map(|n| n.to_lowercase()),
            ),
            AllowedCandidate::Fields { id, publisher_display_name, version, prerelease, target_platform } => (
                id.to_lowercase(),
                version.map_or("*".to_owned(), str::to_owned),
                target_platform.unwrap_or(TargetPlatform::Universal),
                prerelease.unwrap_or(false),
                id.split('.').next().unwrap_or("").to_lowercase(),
                publisher_display_name.map(str::to_lowercase),
            ),
        };

        let settings_link = SETTINGS_COMMAND_LINK;
        let extension_reason = format!("it is not in the [allowed list]({settings_link})");
        if let Some(extension_value) = config.get(&id) {
            match extension_value {
                Value::Bool(allowed) => return if *allowed { Ok(()) } else { Err(extension_reason) },
                Value::String(text) if text == "stable" && prerelease => {
                    return Err(format!("the pre-release versions of this extension are not in the [allowed list]({settings_link})"));
                }
                Value::Array(versions) if version != "*" => {
                    let allowed = versions.iter().filter_map(Value::as_str).any(|candidate| matches_version(candidate, &version, target_platform));
                    if !allowed {
                        return Err(format!("the version {version} of this extension is not in the [allowed list]({settings_link})"));
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        let publisher_key = match &publisher_display_name {
            Some(name) if self.publisher_orgs.contains(name) => name.clone(),
            _ => publisher,
        };
        if let Some(publisher_value) = config.get(&publisher_key) {
            match publisher_value {
                Value::Bool(allowed) => {
                    return if *allowed { Ok(()) } else { Err(format!("the extensions from this publisher are not in the [allowed list]({settings_link})")) };
                }
                Value::String(text) if text == "stable" && prerelease => {
                    return Err(format!("the pre-release versions from this publisher are not in the [allowed list]({settings_link})"));
                }
                _ => return Ok(()),
            }
        }

        if config.get("*") == Some(&Value::Bool(true)) {
            return Ok(());
        }
        Err(extension_reason)
    }
}

/// `^(?<version>\d+\.\d+\.\d+(-.*)?)(@(?<platform>.+))?$` aplicado a uma entrada da lista, e a comparação do
/// `some` do original.
fn matches_version(entry: &str, version: &str, target_platform: TargetPlatform) -> bool {
    let (candidate_version, platform) = match entry.split_once('@') {
        Some((v, p)) if !p.is_empty() => (v, Some(p)),
        Some(_) => return false,
        None => (entry, None),
    };
    let numbers = candidate_version.split('-').next().unwrap_or("");
    let parts: Vec<&str> = numbers.split('.').collect();
    if !(parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))) {
        return false;
    }
    if candidate_version != version {
        return false;
    }
    if target_platform != TargetPlatform::Universal
        && let Some(platform) = platform
        && target_platform.as_str() != platform
    {
        return false;
    }
    true
}
