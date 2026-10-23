//! Porte de `src/vs/platform/extensionManagement/common/extensionsProfileScannerService.ts` e de
//! `node/extensionsProfileScannerService.ts`: lê e escreve o `extensions.json` de um perfil, a lista das extensões
//! instaladas com `identifier`, `version`, `location` (o JSON de uma URI), `relativeLocation` e `metadata`.
//!
//! Diferenças de execução: a E/S é síncrona ([`crate::fs::ExtFs`]) e a fila de acesso por arquivo
//! (`resourcesAccessQueueMap`) some, porque nenhuma chamada se intercala (um ouvinte de evento que releia o perfil
//! roda aninhado, dentro da escrita em curso).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_platform::log::Logger;
use serde_json::{Map, Value};

use crate::fs::{ExtFs, is_not_found};
use crate::manifest::ExtensionIdentifierInfo;
use crate::management::{LocalExtension, Metadata};
use crate::paths;
use crate::profile::UserDataProfiles;
use crate::util::are_same_extensions;

/// `ExtensionsProfileScanningErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionsProfileScanningErrorCode {
    /// A extensão de um perfil que não existe.
    ErrorProfileNotFound,
    /// O arquivo do perfil é inválido.
    ErrorInvalidContent,
    /// Qualquer outra falha (leitura, escrita, JSON malformado): o original propaga o erro do JS sem código.
    Other,
}

/// `ExtensionsProfileScanningError`.
#[derive(Clone, Debug)]
pub struct ExtensionsProfileScanningError {
    pub message: String,
    pub code: ExtensionsProfileScanningErrorCode,
}

impl std::fmt::Display for ExtensionsProfileScanningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionsProfileScanningError {}

impl ExtensionsProfileScanningError {
    fn other(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: ExtensionsProfileScanningErrorCode::Other }
    }
}

/// `IScannedProfileExtension`. A `location` é o caminho da URI.
#[derive(Clone, Debug, PartialEq)]
pub struct ScannedProfileExtension {
    pub identifier: ExtensionIdentifierInfo,
    pub version: String,
    pub location: String,
    pub metadata: Option<Metadata>,
}

/// `ProfileExtensionsEvent`.
#[derive(Clone, Debug)]
pub struct ProfileExtensionsEvent {
    pub extensions: Vec<ScannedProfileExtension>,
    pub profile_location: String,
}

/// `DidAddProfileExtensionsEvent` e `DidRemoveProfileExtensionsEvent`.
#[derive(Clone, Debug)]
pub struct DidProfileExtensionsEvent {
    pub extensions: Vec<ScannedProfileExtension>,
    pub profile_location: String,
    pub error: Option<String>,
}

/// `IProfileExtensionsScanOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileExtensionsScanOptions {
    /// `None` é a chave ausente (`{}`), distinta de `Some(false)` na comparação das entradas do scanner.
    pub bail_out_when_file_not_found: Option<bool>,
}

/// O que `addExtensionsToProfile` e `updateMetadata` leem de uma `IExtension`.
#[derive(Clone, Debug)]
pub struct ProfileExtensionSource {
    pub identifier: ExtensionIdentifierInfo,
    pub version: String,
    pub location: String,
}

impl From<&LocalExtension> for ProfileExtensionSource {
    fn from(extension: &LocalExtension) -> Self {
        Self { identifier: extension.identifier.clone(), version: extension.manifest.version.clone(), location: extension.location.clone() }
    }
}

/// `isIExtensionIdentifier`.
fn is_extension_identifier_value(value: &Value) -> bool {
    value.as_object().is_some_and(|o| {
        o.get("id").is_some_and(Value::is_string)
            && o.get("uuid").is_none_or(|uuid| match uuid {
                Value::String(_) => true,
                // `!thing.uuid ||`: um uuid "falso" (null, vazio, 0, false) passa.
                Value::Null | Value::Bool(false) => true,
                Value::Number(n) => n.as_f64() == Some(0.0),
                _ => false,
            })
    })
}

/// `isUriComponents`: tem `path` e `scheme` de texto.
fn is_uri_components(value: &Value) -> bool {
    value.as_object().is_some_and(|o| o.get("path").is_some_and(Value::is_string) && o.get("scheme").is_some_and(Value::is_string))
}

/// `isStoredProfileExtension`.
fn is_stored_profile_extension(value: &Value) -> bool {
    let Some(object) = value.as_object() else { return false };
    let location_ok = match object.get("location") {
        Some(location) if is_uri_components(location) => true,
        Some(Value::String(text)) => !text.is_empty(),
        _ => false,
    };
    object.get("identifier").is_some_and(is_extension_identifier_value)
        && location_ok
        && object.get("relativeLocation").is_none_or(|v| v.is_string() || v.is_null())
        && object.get("version").and_then(Value::as_str).is_some_and(|v| !v.is_empty())
}

/// `AbstractExtensionsProfileScannerService` (e a subclasse de nó, que só fixa o `extensionsLocation`).
pub struct ExtensionsProfileScannerService {
    extensions_location: String,
    fs: Rc<dyn ExtFs>,
    profiles: UserDataProfiles,
    logger: Rc<dyn Logger>,
    on_add_extensions: Rc<Emitter<ProfileExtensionsEvent>>,
    on_did_add_extensions: Rc<Emitter<DidProfileExtensionsEvent>>,
    on_remove_extensions: Rc<Emitter<ProfileExtensionsEvent>>,
    on_did_remove_extensions: Rc<Emitter<DidProfileExtensionsEvent>>,
    migration: RefCell<Option<Option<Vec<Value>>>>,
}

type UpdateFn<'a> = &'a mut dyn FnMut(Vec<ScannedProfileExtension>) -> Vec<ScannedProfileExtension>;

impl ExtensionsProfileScannerService {
    pub fn new(extensions_location: &str, fs: Rc<dyn ExtFs>, profiles: UserDataProfiles, logger: Rc<dyn Logger>) -> Self {
        Self {
            extensions_location: extensions_location.to_owned(),
            fs,
            profiles,
            logger,
            on_add_extensions: Emitter::new(),
            on_did_add_extensions: Emitter::new(),
            on_remove_extensions: Emitter::new(),
            on_did_remove_extensions: Emitter::new(),
            migration: RefCell::new(None),
        }
    }

    pub fn on_add_extensions(&self) -> Event<ProfileExtensionsEvent> {
        self.on_add_extensions.event()
    }

    pub fn on_did_add_extensions(&self) -> Event<DidProfileExtensionsEvent> {
        self.on_did_add_extensions.event()
    }

    pub fn on_remove_extensions(&self) -> Event<ProfileExtensionsEvent> {
        self.on_remove_extensions.event()
    }

    pub fn on_did_remove_extensions(&self) -> Event<DidProfileExtensionsEvent> {
        self.on_did_remove_extensions.event()
    }

    pub fn scan_profile_extensions(
        &self,
        profile_location: &str,
        options: Option<ProfileExtensionsScanOptions>,
    ) -> Result<Vec<ScannedProfileExtension>, ExtensionsProfileScanningError> {
        self.with_profile_extensions(profile_location, None, options)
    }

    /// `addExtensionsToProfile`: devolve as extensões que entraram (as já presentes, não).
    pub fn add_extensions_to_profile(
        &self,
        extensions: &[(ProfileExtensionSource, Option<Metadata>)],
        profile_location: &str,
        keep_existing_versions: bool,
    ) -> Result<Vec<ScannedProfileExtension>, ExtensionsProfileScanningError> {
        let mut extensions_to_remove: Vec<ScannedProfileExtension> = Vec::new();
        let mut extensions_to_add: Vec<ScannedProfileExtension> = Vec::new();
        let outcome = self.with_profile_extensions(
            profile_location,
            Some(&mut |existing_extensions: Vec<ScannedProfileExtension>| {
                let mut result: Vec<ScannedProfileExtension> = Vec::new();
                if keep_existing_versions {
                    result.extend(existing_extensions);
                } else {
                    for existing in existing_extensions {
                        if extensions.iter().any(|(e, _)| are_same_extensions(&e.identifier, &existing.identifier) && e.version != existing.version) {
                            // Remove the existing extension with different version
                            extensions_to_remove.push(existing);
                        } else {
                            result.push(existing);
                        }
                    }
                }
                for (extension, metadata) in extensions {
                    let index = result.iter().position(|e| are_same_extensions(&e.identifier, &extension.identifier) && e.version == extension.version);
                    let extension_to_add = ScannedProfileExtension {
                        identifier: extension.identifier.clone(),
                        version: extension.version.clone(),
                        location: extension.location.clone(),
                        metadata: metadata.clone(),
                    };
                    match index {
                        None => {
                            extensions_to_add.push(extension_to_add.clone());
                            result.push(extension_to_add);
                        }
                        Some(index) => result[index] = extension_to_add,
                    }
                }
                if !extensions_to_add.is_empty() {
                    self.on_add_extensions.fire(ProfileExtensionsEvent { extensions: extensions_to_add.clone(), profile_location: profile_location.to_owned() });
                }
                if !extensions_to_remove.is_empty() {
                    self.on_remove_extensions.fire(ProfileExtensionsEvent { extensions: extensions_to_remove.clone(), profile_location: profile_location.to_owned() });
                }
                result
            }),
            None,
        );
        let error = outcome.as_ref().err().map(ToString::to_string);
        if !extensions_to_add.is_empty() {
            self.on_did_add_extensions.fire(DidProfileExtensionsEvent {
                extensions: extensions_to_add.clone(),
                profile_location: profile_location.to_owned(),
                error: error.clone(),
            });
        }
        if !extensions_to_remove.is_empty() {
            self.on_did_remove_extensions.fire(DidProfileExtensionsEvent { extensions: extensions_to_remove, profile_location: profile_location.to_owned(), error });
        }
        outcome.map(|_| extensions_to_add)
    }

    /// `updateMetadata`: junta o `metadata` novo ao existente das extensões (mesmo `id` e versão).
    pub fn update_metadata(
        &self,
        extensions: &[(ProfileExtensionSource, Metadata)],
        profile_location: &str,
    ) -> Result<Vec<ScannedProfileExtension>, ExtensionsProfileScanningError> {
        let mut updated_extensions: Vec<ScannedProfileExtension> = Vec::new();
        self.with_profile_extensions(
            profile_location,
            Some(&mut |profile_extensions: Vec<ScannedProfileExtension>| {
                let mut result = Vec::new();
                for mut profile_extension in profile_extensions {
                    let found = extensions.iter().find(|(e, _)| {
                        are_same_extensions(&ExtensionIdentifierInfo::new(&e.identifier.id), &ExtensionIdentifierInfo::new(&profile_extension.identifier.id))
                            && e.version == profile_extension.version
                    });
                    if let Some((_, metadata)) = found {
                        profile_extension.metadata = Some(profile_extension.metadata.clone().unwrap_or_default().merged_with(metadata));
                        updated_extensions.push(profile_extension.clone());
                    }
                    result.push(profile_extension);
                }
                result
            }),
            None,
        )?;
        Ok(updated_extensions)
    }

    /// `removeExtensionsFromProfile`.
    pub fn remove_extensions_from_profile(&self, extensions: &[ExtensionIdentifierInfo], profile_location: &str) -> Result<(), ExtensionsProfileScanningError> {
        let mut extensions_to_remove: Vec<ScannedProfileExtension> = Vec::new();
        let outcome = self.with_profile_extensions(
            profile_location,
            Some(&mut |profile_extensions: Vec<ScannedProfileExtension>| {
                let mut result = Vec::new();
                for e in profile_extensions {
                    if extensions.iter().any(|extension| are_same_extensions(&e.identifier, extension)) {
                        extensions_to_remove.push(e);
                    } else {
                        result.push(e);
                    }
                }
                if !extensions_to_remove.is_empty() {
                    self.on_remove_extensions.fire(ProfileExtensionsEvent { extensions: extensions_to_remove.clone(), profile_location: profile_location.to_owned() });
                }
                result
            }),
            None,
        );
        if !extensions_to_remove.is_empty() {
            self.on_did_remove_extensions.fire(DidProfileExtensionsEvent {
                extensions: extensions_to_remove,
                profile_location: profile_location.to_owned(),
                error: outcome.as_ref().err().map(ToString::to_string),
            });
        }
        outcome.map(|_| ())
    }

    fn with_profile_extensions(
        &self,
        file: &str,
        update_fn: Option<UpdateFn<'_>>,
        options: Option<ProfileExtensionsScanOptions>,
    ) -> Result<Vec<ScannedProfileExtension>, ExtensionsProfileScanningError> {
        let mut extensions: Vec<ScannedProfileExtension> = Vec::new();

        // Read
        let mut stored_profile_extensions: Option<Value> = None;
        match self.fs.read_to_string(file) {
            Ok(content) => {
                let trimmed = content.trim();
                let text = if trimmed.is_empty() { "[]" } else { trimmed };
                stored_profile_extensions = Some(serde_json::from_str(text).map_err(|e| ExtensionsProfileScanningError::other(e.to_string()))?);
            }
            Err(error) if !is_not_found(&error) => return Err(ExtensionsProfileScanningError::other(error.to_string())),
            Err(error) => {
                // migrate from old location, remove this after couple of releases
                if paths::is_equal(file, &self.profiles.default_profile.extensions_resource) {
                    stored_profile_extensions = self.migrate_from_old_default_profile_extensions_location()?.map(Value::Array);
                }
                if stored_profile_extensions.is_none() && options.is_some_and(|o| o.bail_out_when_file_not_found == Some(true)) {
                    return Err(ExtensionsProfileScanningError {
                        message: error.to_string(),
                        code: ExtensionsProfileScanningErrorCode::ErrorProfileNotFound,
                    });
                }
            }
        }

        if let Some(stored) = stored_profile_extensions {
            let Value::Array(mut stored_entries) = stored else { return Err(self.invalid_content_error(file)) };
            // TODO @sandy081: Remove this migration after couple of releases
            let mut migrate = false;
            for entry in &mut stored_entries {
                if !is_stored_profile_extension(entry) {
                    return Err(self.invalid_content_error(file));
                }
                let object = entry.as_object_mut().expect("validado como objeto");
                let relative_location = object.get("relativeLocation").and_then(Value::as_str).filter(|r| !r.is_empty()).map(str::to_owned);
                let location = if let Some(relative) = relative_location {
                    // Extension in new format. No migration needed.
                    paths::join_path(&self.extensions_location, &[&relative])
                } else if let Some(text) = object.get("location").and_then(Value::as_str) {
                    self.logger.warn(&format!("Extensions profile: Ignoring extension with invalid location: {text}"));
                    continue;
                } else {
                    let location = object["location"].get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
                    if let Some(relative_path) = self.to_relative_path(&location) {
                        // Extension in old format. Migrate to new format.
                        migrate = true;
                        object.insert("relativeLocation".to_owned(), Value::from(relative_path));
                    }
                    location
                };
                if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut)
                    && metadata.get("hasPreReleaseVersion").is_none_or(Value::is_null)
                    && metadata.get("preRelease").is_some_and(is_truthy)
                {
                    migrate = true;
                    metadata.insert("hasPreReleaseVersion".to_owned(), Value::Bool(true));
                }
                let identifier = &object["identifier"];
                let id = identifier["id"].as_str().unwrap_or_default().to_owned();
                let metadata: Option<Metadata> = object.get("metadata").filter(|m| m.is_object()).and_then(|m| serde_json::from_value(m.clone()).ok());
                let uuid = metadata
                    .as_ref()
                    .and_then(|m| m.id.clone())
                    .or_else(|| identifier.get("uuid").and_then(Value::as_str).map(str::to_owned))
                    .filter(|u| !u.is_empty());
                extensions.push(ScannedProfileExtension {
                    identifier: match uuid {
                        Some(uuid) => ExtensionIdentifierInfo::with_uuid(id, uuid),
                        None => ExtensionIdentifierInfo::new(id),
                    },
                    location,
                    version: object["version"].as_str().unwrap_or_default().to_owned(),
                    metadata,
                });
            }
            if migrate {
                self.write_json(file, &Value::Array(stored_entries))?;
            }
        }

        // Update
        if let Some(update_fn) = update_fn {
            extensions = update_fn(extensions);
            let stored: Vec<Value> = extensions
                .iter()
                .map(|e| {
                    let mut object = Map::new();
                    let mut identifier = Map::new();
                    identifier.insert("id".to_owned(), Value::from(e.identifier.id.clone()));
                    if let Some(uuid) = &e.identifier.uuid {
                        identifier.insert("uuid".to_owned(), Value::from(uuid.clone()));
                    }
                    object.insert("identifier".to_owned(), Value::Object(identifier));
                    object.insert("version".to_owned(), Value::from(e.version.clone()));
                    // retain old format so that old clients can read it
                    object.insert("location".to_owned(), paths::uri_json(&e.location));
                    if let Some(relative) = self.to_relative_path(&e.location) {
                        object.insert("relativeLocation".to_owned(), Value::from(relative));
                    }
                    if let Some(metadata) = &e.metadata {
                        object.insert("metadata".to_owned(), serde_json::to_value(metadata).expect("o metadata serializa"));
                    }
                    Value::Object(object)
                })
                .collect();
            self.write_json(file, &Value::Array(stored))?;
        }
        Ok(extensions)
    }

    fn write_json(&self, file: &str, value: &Value) -> Result<(), ExtensionsProfileScanningError> {
        self.fs.write(file, value.to_string().as_bytes()).map_err(|e| ExtensionsProfileScanningError::other(e.to_string()))
    }

    fn invalid_content_error(&self, file: &str) -> ExtensionsProfileScanningError {
        ExtensionsProfileScanningError {
            message: format!("Invalid extensions content in {file}"),
            code: ExtensionsProfileScanningErrorCode::ErrorInvalidContent,
        }
    }

    /// `toRelativePath`: o nome da pasta, quando a extensão mora direto na pasta de extensões.
    fn to_relative_path(&self, extension_location: &str) -> Option<String> {
        paths::is_equal(&paths::dirname(extension_location), &self.extensions_location).then(|| paths::basename(extension_location))
    }

    fn migrate_from_old_default_profile_extensions_location(&self) -> Result<Option<Vec<Value>>, ExtensionsProfileScanningError> {
        if let Some(done) = &*self.migration.borrow() {
            return Ok(done.clone());
        }
        let default_profile = &self.profiles.default_profile;
        let old_location = paths::join_path(&default_profile.location, &["extensions.json"]);
        let old_init_location = paths::join_path(&self.extensions_location, &[".init-default-profile-extensions"]);
        let content = match self.fs.read_to_string(&old_location) {
            Ok(content) => content,
            Err(error) if is_not_found(&error) => {
                *self.migration.borrow_mut() = Some(None);
                return Ok(None);
            }
            Err(error) => return Err(ExtensionsProfileScanningError::other(error.to_string())),
        };
        self.logger.info(&format!("Migrating extensions from old default profile location {old_location}"));
        let mut stored_profile_extensions: Option<Vec<Value>> = None;
        match serde_json::from_str::<Value>(&content) {
            Ok(Value::Array(items)) if items.iter().all(is_stored_profile_extension) => stored_profile_extensions = Some(items),
            Ok(_) => self.logger.warn("Skipping migrating from old default profile locaiton: Found invalid data"),
            Err(error) => self.logger.error(&error.to_string()),
        }
        if let Some(stored) = &stored_profile_extensions {
            match self.fs.create(&default_profile.extensions_resource, Value::Array(stored.clone()).to_string().as_bytes()) {
                Ok(()) => self.logger.info(&format!(
                    "Migrated extensions from old default profile location to new location {old_location} {}",
                    default_profile.extensions_resource
                )),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => self.logger.info(&format!(
                    "Migration from old default profile location to new location is done by another window {old_location}"
                )),
                Err(error) => return Err(ExtensionsProfileScanningError::other(error.to_string())),
            }
        }
        for location in [&old_location, &old_init_location] {
            if let Err(error) = self.fs.remove(location, false)
                && !is_not_found(&error)
            {
                self.logger.error(&error.to_string());
            }
        }
        *self.migration.borrow_mut() = Some(stored_profile_extensions.clone());
        Ok(stored_profile_extensions)
    }
}

/// `!!value` do JavaScript.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
