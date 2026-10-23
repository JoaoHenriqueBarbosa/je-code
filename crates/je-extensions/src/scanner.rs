//! Porte de `src/vs/platform/extensionManagement/common/extensionsScannerService.ts` e do
//! `node/extensionsScannerService.ts`: varre as extensões embutidas (`resources/app/extensions`) e a pasta do
//! usuário, lê o `package.json` e o `package.nls.json` de cada uma, localiza o manifesto, valida e escolhe a versão
//! (a mais nova, preferindo a do usuário sobre a embutida quando ela é mais nova).
//!
//! Diferenças de execução em relação ao original:
//! - A E/S é síncrona ([`crate::fs::ExtFs`]) e os caminhos são os da URI.
//! - A validação do cache de varredura (o `ThrottledDelayer(3000)` do `CachedExtensionsScanner`) é uma chamada
//!   explícita, [`ExtensionsScannerService::validate_pending_caches`], que o chamador agenda (o laço de eventos do
//!   produto é quem tem o relógio); o resultado da validação é o mesmo.
//! - A mensagem de erro de um `package.json` com JSON inválido é a do `serde_json`, não a do V8.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::severity::Severity;
use je_platform::log::Logger;
use serde_json::{Map, Value};

use crate::fs::{ExtFs, is_not_found};
use crate::json_util::{parse_lenient, strip_bom, to_string_tab_indented};
use crate::management::{InstallSource, Metadata, ProductVersion};
use crate::manifest::{
    ExtensionIdentifier, ExtensionIdentifierInfo, ExtensionIdentifierMap, ExtensionManifest, ExtensionType, TargetPlatform, UNDEFINED_PUBLISHER,
    get_manifest_cache_file_name, parse_enabled_api_proposal_names,
};
use crate::nls::localize_manifest;
use crate::paths;
use crate::profile::{UserDataProfile, UserDataProfiles};
use crate::profile_scanner::{
    ExtensionsProfileScannerService, ExtensionsProfileScanningErrorCode, ProfileExtensionSource, ProfileExtensionsScanOptions, ScannedProfileExtension,
};
use crate::util::{are_same_extensions, compute_target_platform, get_extension_id, get_gallery_extension_id};
use crate::validator::validate_extension_manifest;
use je_base::semver;

/// `ManifestMetadata`: o `__metadata` que o instalador escreve no `package.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManifestMetadata {
    pub target_platform: Option<TargetPlatform>,
    pub installed_timestamp: Option<f64>,
    pub size: Option<f64>,
}

/// `IScannedExtension` (e `IRelaxedScannedExtension`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScannedExtension {
    pub extension_type: ExtensionType,
    pub is_builtin: bool,
    pub identifier: ExtensionIdentifierInfo,
    pub manifest: ExtensionManifest,
    pub location: String,
    pub target_platform: TargetPlatform,
    pub publisher_display_name: Option<String>,
    pub metadata: Option<Metadata>,
    pub is_valid: bool,
    pub validations: Vec<(Severity, String)>,
    pub pre_release: bool,
    pub force_auto_update: bool,
}

/// `SystemExtensionsScanOptions`.
#[derive(Clone, Debug, Default)]
pub struct SystemExtensionsScanOptions {
    pub check_control_file: bool,
    pub language: Option<String>,
}

/// `UserExtensionsScanOptions`.
#[derive(Clone, Debug, Default)]
pub struct UserExtensionsScanOptions {
    pub profile_location: String,
    pub include_invalid: bool,
    pub language: Option<String>,
    pub use_cache: bool,
    pub product_version: Option<ProductVersion>,
}

/// `ScanOptions`.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    pub include_invalid: bool,
    pub language: Option<String>,
}

/// Erro de varredura: o do perfil (que o chamador distingue por código) ou uma falha qualquer.
#[derive(Clone, Debug)]
pub enum ScanError {
    Profile(crate::profile_scanner::ExtensionsProfileScanningError),
    Other(String),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile(error) => error.fmt(f),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<crate::profile_scanner::ExtensionsProfileScanningError> for ScanError {
    fn from(error: crate::profile_scanner::ExtensionsProfileScanningError) -> Self {
        Self::Profile(error)
    }
}

impl From<std::io::Error> for ScanError {
    fn from(error: std::io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

/// O que o scanner lê do `IProductService`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionsProduct {
    pub version: String,
    /// `nameLong`, para as mensagens de erro de instalação.
    pub name_long: String,
    pub date: Option<String>,
    pub commit: Option<String>,
    pub quality: Option<String>,
    pub built_in_extensions_enabled_with_auto_updates: Vec<String>,
    /// Os `name` de `builtInExtensions` (as extensões embutidas baixadas em desenvolvimento).
    pub built_in_extensions: Vec<String>,
}

impl ExtensionsProduct {
    /// Lê o que importa do `product.json`.
    pub fn from_product_json(product: &Value) -> Self {
        let text = |key: &str| product.get(key).and_then(Value::as_str).map(str::to_owned);
        let names = |key: &str| -> Vec<String> {
            product.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
        };
        Self {
            version: text("version").unwrap_or_default(),
            name_long: text("nameLong").unwrap_or_default(),
            date: text("date"),
            commit: text("commit"),
            quality: text("quality"),
            built_in_extensions_enabled_with_auto_updates: names("builtInExtensionsEnabledWithAutoUpdates"),
            built_in_extensions: product
                .get("builtInExtensions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|e| e.get("name").and_then(Value::as_str)).map(str::to_owned).collect())
                .unwrap_or_default(),
        }
    }
}

/// O que o scanner lê do `IEnvironmentService`.
#[derive(Clone, Debug, Default)]
pub struct ScannerEnvironment {
    pub is_built: bool,
    pub skip_builtin_extensions: Vec<String>,
    pub is_extension_development: bool,
    pub extension_development_locations: Vec<String>,
    /// `platform.language`: o idioma de varredura quando nenhum é pedido.
    pub language: String,
}

/// Onde a varredura procura.
#[derive(Clone, Debug)]
pub struct ScannerLocations {
    pub system_extensions_location: String,
    pub user_extensions_location: String,
    /// `<home>/.vscode-oss-dev/extensions/control.json`.
    pub extensions_control_location: String,
    /// `.build/builtInExtensions`, onde o desenvolvimento do VS Code baixa as embutidas do marketplace.
    pub dev_builtin_extensions_location: String,
}

/// `Translations`: id da extensão para o caminho do pacote de tradução.
pub type TranslationsMap = HashMap<String, String>;

/// `ExtensionScannerInput`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionScannerInput {
    pub location: String,
    pub mtime: Option<u64>,
    pub application_extensions_location: Option<String>,
    pub application_extensions_location_mtime: Option<u64>,
    pub profile: bool,
    pub profile_scan_options: Option<ProfileExtensionsScanOptions>,
    pub extension_type: ExtensionType,
    pub validate: bool,
    pub product_version: String,
    pub product_date: Option<String>,
    pub product_commit: Option<String>,
    pub dev_mode: bool,
    pub language: Option<String>,
    pub translations: TranslationsMap,
}

impl ExtensionScannerInput {
    /// `ExtensionScannerInput.equals`.
    pub fn equals(a: &ExtensionScannerInput, b: &ExtensionScannerInput) -> bool {
        a == b
    }

    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("location".into(), paths::uri_json(&self.location));
        if let Some(mtime) = self.mtime {
            o.insert("mtime".into(), Value::from(mtime));
        }
        if let Some(location) = &self.application_extensions_location {
            o.insert("applicationExtensionslocation".into(), paths::uri_json(location));
        }
        if let Some(mtime) = self.application_extensions_location_mtime {
            o.insert("applicationExtensionslocationMtime".into(), Value::from(mtime));
        }
        o.insert("profile".into(), Value::from(self.profile));
        if let Some(options) = &self.profile_scan_options {
            let mut options_json = Map::new();
            if let Some(bail) = options.bail_out_when_file_not_found {
                options_json.insert("bailOutWhenFileNotFound".into(), Value::from(bail));
            }
            o.insert("profileScanOptions".into(), Value::Object(options_json));
        }
        o.insert("type".into(), Value::from(self.extension_type as u8));
        o.insert("validate".into(), Value::from(self.validate));
        o.insert("productVersion".into(), Value::from(self.product_version.clone()));
        if let Some(date) = &self.product_date {
            o.insert("productDate".into(), Value::from(date.clone()));
        }
        if let Some(commit) = &self.product_commit {
            o.insert("productCommit".into(), Value::from(commit.clone()));
        }
        o.insert("devMode".into(), Value::from(self.dev_mode));
        if let Some(language) = &self.language {
            o.insert("language".into(), Value::from(language.clone()));
        }
        let translations: Map<String, Value> = self.translations.iter().map(|(k, v)| (k.clone(), Value::from(v.clone()))).collect();
        o.insert("translations".into(), Value::Object(translations));
        Value::Object(o)
    }

    fn from_json(value: &Value) -> Option<ExtensionScannerInput> {
        let o = value.as_object()?;
        let location_of = |key: &str| o.get(key).and_then(|v| v.get("path")).and_then(Value::as_str).map(str::to_owned);
        let text = |key: &str| o.get(key).and_then(Value::as_str).map(str::to_owned);
        let number = |key: &str| o.get(key).and_then(Value::as_u64);
        let profile_scan_options = o
            .get("profileScanOptions")
            .and_then(Value::as_object)
            .map(|options| ProfileExtensionsScanOptions { bail_out_when_file_not_found: options.get("bailOutWhenFileNotFound").and_then(Value::as_bool) });
        Some(ExtensionScannerInput {
            location: location_of("location")?,
            mtime: number("mtime"),
            application_extensions_location: location_of("applicationExtensionslocation"),
            application_extensions_location_mtime: number("applicationExtensionslocationMtime"),
            profile: o.get("profile")?.as_bool()?,
            profile_scan_options,
            extension_type: match o.get("type")?.as_u64()? {
                0 => ExtensionType::System,
                _ => ExtensionType::User,
            },
            validate: o.get("validate")?.as_bool()?,
            product_version: text("productVersion")?,
            product_date: text("productDate"),
            product_commit: text("productCommit"),
            dev_mode: o.get("devMode")?.as_bool()?,
            language: text("language"),
            translations: o.get("translations")?.as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect(),
        })
    }
}

impl ScannedExtension {
    /// A forma `IRelaxedScannedExtension` do JSON, a que o cache em disco guarda.
    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("type".into(), Value::from(self.extension_type as u8));
        o.insert("identifier".into(), serde_json::to_value(&self.identifier).expect("identificador serializa"));
        o.insert("manifest".into(), serde_json::to_value(&self.manifest).expect("manifesto serializa"));
        o.insert("location".into(), paths::uri_json(&self.location));
        o.insert("isBuiltin".into(), Value::from(self.is_builtin));
        o.insert("targetPlatform".into(), serde_json::to_value(self.target_platform).expect("plataforma serializa"));
        if let Some(name) = &self.publisher_display_name {
            o.insert("publisherDisplayName".into(), Value::from(name.clone()));
        }
        if let Some(metadata) = &self.metadata {
            o.insert("metadata".into(), serde_json::to_value(metadata).expect("metadata serializa"));
        }
        o.insert("isValid".into(), Value::from(self.is_valid));
        let validations: Vec<Value> = self.validations.iter().map(|(severity, message)| Value::Array(vec![Value::from(*severity as u8), Value::from(message.clone())])).collect();
        o.insert("validations".into(), Value::Array(validations));
        o.insert("preRelease".into(), Value::from(self.pre_release));
        o.insert("forceAutoUpdate".into(), Value::from(self.force_auto_update));
        Value::Object(o)
    }

    fn from_json(value: &Value) -> Option<ScannedExtension> {
        let o = value.as_object()?;
        let validations = o
            .get("validations")?
            .as_array()?
            .iter()
            .map(|pair| {
                let pair = pair.as_array()?;
                let severity = match pair.first()?.as_u64()? {
                    0 => Severity::Ignore,
                    1 => Severity::Info,
                    2 => Severity::Warning,
                    _ => Severity::Error,
                };
                Some((severity, pair.get(1)?.as_str()?.to_owned()))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(ScannedExtension {
            extension_type: if o.get("type")?.as_u64()? == 0 { ExtensionType::System } else { ExtensionType::User },
            is_builtin: o.get("isBuiltin")?.as_bool()?,
            identifier: serde_json::from_value(o.get("identifier")?.clone()).ok()?,
            manifest: ExtensionManifest::from_value(o.get("manifest")?.clone()),
            location: o.get("location")?.get("path")?.as_str()?.to_owned(),
            target_platform: serde_json::from_value(o.get("targetPlatform")?.clone()).ok()?,
            publisher_display_name: o.get("publisherDisplayName").and_then(Value::as_str).map(str::to_owned),
            metadata: o.get("metadata").and_then(|m| serde_json::from_value(m.clone()).ok()),
            is_valid: o.get("isValid")?.as_bool()?,
            validations,
            pre_release: o.get("preRelease")?.as_bool()?,
            force_auto_update: o.get("forceAutoUpdate")?.as_bool()?,
        })
    }
}

fn product_auto_update_ids(product: &ExtensionsProduct, environment: &ScannerEnvironment) -> std::collections::HashSet<String> {
    let mut result = std::collections::HashSet::new();
    for id in &product.built_in_extensions_enabled_with_auto_updates {
        let lower = id.to_lowercase();
        if environment.skip_builtin_extensions.iter().any(|skip| skip.to_lowercase() == lower) {
            continue;
        }
        result.insert(lower);
    }
    result
}

/// `getLocalizedMessages` do original: os valores traduzidos e o pacote padrão (o `package.nls.json`).
struct LocalizedMessages {
    values: Option<Map<String, Value>>,
    default: Option<String>,
}

/// O scanner puro (sem cache): `ExtensionsScanner`.
struct ExtensionsScanner {
    fs: Rc<dyn ExtFs>,
    logger: Rc<dyn Logger>,
    profile_scanner: Rc<ExtensionsProfileScannerService>,
    product_quality: Option<String>,
    auto_update_ids: std::collections::HashSet<String>,
}

impl ExtensionsScanner {
    fn scan_extensions(&self, input: &ExtensionScannerInput) -> Result<Vec<ScannedExtension>, ScanError> {
        if input.profile { self.scan_extensions_from_profile(input) } else { self.scan_extensions_from_location(input) }
    }

    fn scan_extensions_from_location(&self, input: &ExtensionScannerInput) -> Result<Vec<ScannedExtension>, ScanError> {
        let children = self.fs.read_dir(&input.location)?;
        if children.is_empty() {
            return Ok(Vec::new());
        }
        let mut extensions = Vec::new();
        for child in children {
            if !child.is_directory {
                continue;
            }
            // Do not consider user extension folder starting with `.`
            if input.extension_type == ExtensionType::User && child.name.starts_with('.') {
                continue;
            }
            let mut child_input = input.clone();
            child_input.location = paths::join_path(&input.location, &[&child.name]);
            if let Some(extension) = self.scan_extension(&child_input, None)? {
                extensions.push(extension);
            }
        }
        // Sort: Make sure extensions are in the same order always. Helps cache invalidation even if the order changes.
        extensions.sort_by(|a, b| a.location.cmp(&b.location));
        Ok(extensions)
    }

    fn scan_extensions_from_profile(&self, input: &ExtensionScannerInput) -> Result<Vec<ScannedExtension>, ScanError> {
        let mut profile_extensions = self.scan_extensions_from_profile_resource(&input.location, &|_| true, input)?;
        if let Some(application_location) = &input.application_extensions_location
            && !paths::is_equal(&input.location, application_location)
        {
            profile_extensions.retain(|e| !e.metadata.as_ref().is_some_and(|m| m.is_application_scoped == Some(true)));
            let application_extensions = self.scan_extensions_from_profile_resource(
                application_location,
                &|e| e.metadata.as_ref().is_some_and(|m| m.is_builtin == Some(true) || m.is_application_scoped == Some(true)),
                input,
            )?;
            profile_extensions.extend(application_extensions);
        }
        Ok(profile_extensions)
    }

    fn scan_extensions_from_profile_resource(
        &self,
        profile_resource: &str,
        filter: &dyn Fn(&ScannedProfileExtension) -> bool,
        input: &ExtensionScannerInput,
    ) -> Result<Vec<ScannedExtension>, ScanError> {
        let scanned = self.profile_scanner.scan_profile_extensions(profile_resource, input.profile_scan_options)?;
        let mut extensions = Vec::new();
        for extension_info in scanned {
            if filter(&extension_info) {
                let mut extension_input = input.clone();
                extension_input.location = extension_info.location.clone();
                if let Some(extension) = self.scan_extension(&extension_input, Some(&extension_info))? {
                    extensions.push(extension);
                }
            }
        }
        Ok(extensions)
    }

    fn scan_one_or_multiple_extensions(&self, input: &ExtensionScannerInput) -> Vec<ScannedExtension> {
        let result = (|| -> Result<Vec<ScannedExtension>, ScanError> {
            if self.fs.exists(&paths::join_path(&input.location, &["package.json"])) {
                Ok(self.scan_extension(input, None)?.into_iter().collect())
            } else {
                self.scan_extensions(input)
            }
        })();
        result.unwrap_or_else(|error| {
            self.logger.error(&format!("Error scanning extensions at {}: {error}", input.location));
            Vec::new()
        })
    }

    /// `scanExtension`. Sem `scanned_profile_extension`, um manifesto ilegível dá `None`; com ele, uma extensão
    /// inválida que carrega o motivo.
    fn scan_extension(&self, input: &ExtensionScannerInput, scanned_profile_extension: Option<&ScannedProfileExtension>) -> Result<Option<ScannedExtension>, ScanError> {
        let mut validations: Vec<(Severity, String)> = Vec::new();
        let mut is_valid = true;
        let mut manifest = match self.scan_extension_manifest(&input.location) {
            Ok(manifest) => manifest,
            Err(error) => {
                if let Some(profile_extension) = scanned_profile_extension {
                    validations.push((Severity::Error, error.to_string()));
                    is_valid = false;
                    let (publisher, name) = match profile_extension.identifier.id.split_once('.') {
                        // `id.split('.')` desestrutura só os dois primeiros pedaços.
                        Some((publisher, rest)) => (publisher.to_owned(), rest.split('.').next().unwrap_or("").to_owned()),
                        None => (profile_extension.identifier.id.clone(), "undefined".to_owned()),
                    };
                    let mut object = Map::new();
                    object.insert("name".into(), Value::from(name));
                    object.insert("publisher".into(), Value::from(publisher));
                    object.insert("version".into(), Value::from(profile_extension.version.clone()));
                    object.insert("engines".into(), serde_json::json!({ "vscode": "" }));
                    object
                } else {
                    if input.extension_type != ExtensionType::System {
                        self.logger.error(&error.to_string());
                    }
                    return Ok(None);
                }
            }
        };

        // allow publisher to be undefined to make the initial extension authoring experience smoother
        if !manifest.get("publisher").is_some_and(is_truthy) {
            manifest.insert("publisher".into(), Value::from(UNDEFINED_PUBLISHER));
        }

        let manifest_metadata = manifest.get("__metadata").cloned();
        let metadata: Option<Metadata> = if let Some(profile_extension) = scanned_profile_extension {
            let mut metadata = profile_extension.metadata.clone().unwrap_or_default();
            metadata.size = manifest_metadata.as_ref().and_then(|m| m.get("size")).and_then(Value::as_f64);
            Some(metadata)
        } else {
            manifest_metadata.as_ref().filter(|m| m.is_object()).map(|m| Metadata {
                installed_timestamp: m.get("installedTimestamp").and_then(Value::as_f64),
                size: m.get("size").and_then(Value::as_f64),
                target_platform: m.get("targetPlatform").and_then(|t| serde_json::from_value(t.clone()).ok()),
                ..Metadata::default()
            })
        };
        manifest.shift_remove("__metadata");

        let publisher = manifest.get("publisher").and_then(Value::as_str).unwrap_or(UNDEFINED_PUBLISHER).to_owned();
        let name = js_string(manifest.get("name"));
        let id = get_gallery_extension_id(Some(&publisher), &name);
        let uuid = metadata.as_ref().and_then(|m| m.id.clone()).filter(|u| !u.is_empty());
        let identifier = match uuid {
            Some(uuid) => ExtensionIdentifierInfo::with_uuid(id.clone(), uuid),
            None => ExtensionIdentifierInfo::new(id.clone()),
        };
        let extension_type = if metadata.as_ref().is_some_and(|m| m.is_system == Some(true)) { ExtensionType::System } else { input.extension_type };
        let is_builtin = extension_type == ExtensionType::System || metadata.as_ref().is_some_and(|m| m.is_builtin == Some(true));

        let mut manifest_value = Value::Object(manifest);
        if let Err(error) = self.translate_manifest(&input.location, &mut manifest_value, input) {
            self.logger.warn(&format!("Failed to translate manifest {error}"));
        }

        if input.validate {
            let found = validate_extension_manifest(&input.product_version, input.product_date.as_deref(), &input.location, &manifest_value, is_builtin);
            for (severity, message) in &found {
                if *severity == Severity::Error {
                    is_valid = false;
                    self.logger.error(&format_message(&input.location, message));
                }
            }
            validations.extend(found);
        }

        if let Some(Value::Array(proposals)) = manifest_value.get("enabledApiProposals").cloned() {
            let names: Vec<String> = proposals.iter().filter_map(Value::as_str).map(str::to_owned).collect();
            let object = manifest_value.as_object_mut().expect("manifesto é objeto");
            object.insert("originalEnabledApiProposals".into(), Value::Array(proposals));
            object.insert("enabledApiProposals".into(), Value::from(parse_enabled_api_proposal_names(&names)));
        }

        let target_platform = metadata.as_ref().and_then(|m| m.target_platform).unwrap_or(TargetPlatform::Undefined);
        let force_auto_update = self.auto_update_ids.contains(&id.to_lowercase()) && self.product_quality.as_deref() == Some("stable");
        Ok(Some(ScannedExtension {
            extension_type,
            is_builtin,
            identifier,
            manifest: ExtensionManifest::from_value(manifest_value),
            location: input.location.clone(),
            target_platform,
            publisher_display_name: metadata.as_ref().and_then(|m| m.publisher_display_name.clone()),
            pre_release: metadata.as_ref().is_some_and(|m| m.pre_release == Some(true)),
            metadata,
            is_valid,
            validations,
            force_auto_update,
        }))
    }

    /// `validate`, para uma extensão que já foi escaneada (o caminho de extensões em desenvolvimento).
    fn validate(&self, extension: &mut ScannedExtension, input: &ExtensionScannerInput) {
        let manifest = serde_json::to_value(&extension.manifest).expect("manifesto serializa");
        let found = validate_extension_manifest(&input.product_version, input.product_date.as_deref(), &input.location, &manifest, extension.is_builtin);
        for (severity, message) in &found {
            if *severity == Severity::Error {
                extension.is_valid = false;
                self.logger.error(&format_message(&input.location, message));
            }
        }
        extension.validations.extend(found);
    }

    fn scan_extension_manifest(&self, extension_location: &str) -> Result<Map<String, Value>, ScanError> {
        let manifest_location = paths::join_path(extension_location, &["package.json"]);
        let content = match self.fs.read_to_string(&manifest_location) {
            Ok(content) => content,
            Err(error) => {
                if !is_not_found(&error) {
                    self.logger.error(&format_message(extension_location, &format!("Cannot read file {manifest_location}: {error}.")));
                }
                return Err(error.into());
            }
        };
        let content = strip_bom(&content);
        let parsed: Value = match serde_json::from_str(content) {
            Ok(value) => value,
            Err(error) => {
                // invalid JSON, let's get good errors
                let (_, issues) = parse_lenient(content);
                for issue in issues {
                    self.logger.error(&format_message(
                        extension_location,
                        &format!("Failed to parse {manifest_location}: [{}, {}] {}.", issue.offset, issue.length, issue.message),
                    ));
                }
                return Err(ScanError::Other(error.to_string()));
            }
        };
        match parsed {
            Value::Object(manifest) => Ok(manifest),
            _ => {
                let message = format_message(extension_location, &format!("Invalid manifest file {manifest_location}: Not a JSON object."));
                self.logger.error(&message);
                Err(ScanError::Other(message))
            }
        }
    }

    fn translate_manifest(&self, extension_location: &str, manifest: &mut Value, input: &ExtensionScannerInput) -> Result<(), ScanError> {
        let Some(localized) = self.get_localized_messages(extension_location, manifest, input) else { return Ok(()) };
        let mut errors = Vec::new();
        // `resolveOriginalMessageBundle` devolve `undefined` quando não há pacote padrão.
        let defaults = self.resolve_original_message_bundle(localized.default.as_deref(), &mut errors);
        if !errors.is_empty() {
            for message in errors {
                self.logger.error(&format_message(
                    extension_location,
                    &format!("Failed to parse {}: {message}.", localized.default.as_deref().unwrap_or("undefined")),
                ));
            }
            return Ok(());
        }
        let values = localized.values.unwrap_or_default();
        localize_manifest(self.logger.as_ref(), manifest, &values, defaults.as_ref());
        Ok(())
    }

    fn get_localized_messages(&self, extension_location: &str, manifest: &Value, input: &ExtensionScannerInput) -> Option<LocalizedMessages> {
        let default_package_nls = paths::join_path(extension_location, &["package.nls.json"]);
        let report_errors = |localized: &str, errors: &[crate::json_util::ParseIssue]| {
            for error in errors {
                self.logger.error(&format_message(extension_location, &format!("Failed to parse {localized}: {}.", error.message)));
            }
        };
        let report_invalid_format = |localized: &str| {
            self.logger.error(&format_message(extension_location, &format!("Invalid format {localized}: JSON object expected.")));
        };

        let translation_id = format!("{}.{}", js_string(manifest.get("publisher")), js_string(manifest.get("name")));
        if let Some(translation_path) = input.translations.get(&translation_id).filter(|p| !p.is_empty()) {
            let translation_resource = paths::from_fs_path(translation_path);
            let outcome = (|| {
                let content = self.fs.read_to_string(&translation_resource).ok()?;
                let (bundle, errors) = parse_lenient(strip_bom(&content));
                if !errors.is_empty() {
                    report_errors(&translation_resource, &errors);
                    return Some(LocalizedMessages { values: None, default: Some(default_package_nls.clone()) });
                }
                match bundle {
                    Some(Value::Object(bundle)) => {
                        let values = bundle.get("contents").and_then(|c| c.get("package")).and_then(Value::as_object).cloned();
                        Some(LocalizedMessages { values, default: Some(default_package_nls.clone()) })
                    }
                    _ => {
                        report_invalid_format(&translation_resource);
                        Some(LocalizedMessages { values: None, default: Some(default_package_nls.clone()) })
                    }
                }
            })();
            return Some(outcome.unwrap_or(LocalizedMessages { values: None, default: Some(default_package_nls) }));
        }

        if !self.fs.exists(&default_package_nls) {
            return None;
        }
        let (localized, original) = self.find_message_bundles(extension_location, input);
        match self.fs.read_to_string(&localized) {
            Ok(content) => {
                let (messages, errors) = parse_lenient(strip_bom(&content));
                if !errors.is_empty() {
                    report_errors(&localized, &errors);
                    return Some(LocalizedMessages { values: None, default: original });
                }
                match messages {
                    Some(Value::Object(messages)) => Some(LocalizedMessages { values: Some(messages), default: original }),
                    _ => {
                        report_invalid_format(&localized);
                        Some(LocalizedMessages { values: None, default: original })
                    }
                }
            }
            Err(_) => Some(LocalizedMessages { values: None, default: original }),
        }
    }

    /// `resolveOriginalMessageBundle`: o `package.nls.json` padrão, ou `None` se não houver caminho ou leitura.
    fn resolve_original_message_bundle(&self, original: Option<&str>, errors: &mut Vec<String>) -> Option<Map<String, Value>> {
        let content = self.fs.read_to_string(original?).ok()?;
        let (value, issues) = parse_lenient(strip_bom(&content));
        errors.extend(issues.into_iter().map(|i| i.message));
        match value {
            Some(Value::Object(map)) => Some(map),
            _ => None,
        }
    }

    /// `findMessageBundles`: o `package.nls.<locale>.json` mais específico que existe, caindo de `pt-br` para `pt`
    /// e por fim para o `package.nls.json`. Devolve (localizado, original); `original` é `None` quando o localizado
    /// já é o padrão.
    fn find_message_bundles(&self, extension_location: &str, input: &ExtensionScannerInput) -> (String, Option<String>) {
        let default_bundle = paths::join_path(extension_location, &["package.nls.json"]);
        let language = match &input.language {
            Some(language) if !input.dev_mode && language != "pseudo" && !language.is_empty() => language.clone(),
            _ => return (default_bundle, None),
        };
        let mut locale = language;
        loop {
            let to_check = paths::join_path(extension_location, &[&format!("package.nls.{locale}.json")]);
            if self.fs.exists(&to_check) {
                return (to_check, Some(default_bundle));
            }
            match locale.rfind('-') {
                None => return (default_bundle, None),
                Some(index) => locale.truncate(index),
            }
        }
    }
}

fn format_message(extension_location: &str, message: &str) -> String {
    format!("[{extension_location}]: {message}")
}

/// `${value}` do JavaScript para o que aparece como nome ou publisher.
fn js_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
        None => "undefined".to_owned(),
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `CachedExtensionsScanner`: o mesmo scanner com o resultado guardado em `<cacheHome>/extensions.<tipo>[.<idioma>].cache`.
struct CachedScanner {
    pending_validations: RefCell<Vec<(String, ExtensionScannerInput)>>,
}

/// `AbstractExtensionsScannerService` + `NativeExtensionsScannerService`.
pub struct ExtensionsScannerService {
    pub locations: ScannerLocations,
    environment: ScannerEnvironment,
    product: ExtensionsProduct,
    current_profile: UserDataProfile,
    profiles: UserDataProfiles,
    fs: Rc<dyn ExtFs>,
    logger: Rc<dyn Logger>,
    profile_scanner: Rc<ExtensionsProfileScannerService>,
    scanner: ExtensionsScanner,
    system_cached: CachedScanner,
    user_cached: CachedScanner,
    get_translations: Box<dyn Fn(&str) -> TranslationsMap>,
    on_did_change_cache: Rc<Emitter<ExtensionType>>,
    target_platform: RefCell<Option<TargetPlatform>>,
}

/// O que o construtor precisa, agrupado.
pub struct ExtensionsScannerServiceParts {
    pub locations: ScannerLocations,
    pub environment: ScannerEnvironment,
    pub product: ExtensionsProduct,
    pub current_profile: UserDataProfile,
    pub profiles: UserDataProfiles,
    pub fs: Rc<dyn ExtFs>,
    pub logger: Rc<dyn Logger>,
    pub profile_scanner: Rc<ExtensionsProfileScannerService>,
    /// `getTranslations(language)`: o pacote de tradução por extensão. No VS Code nativo vem do
    /// `translationsConfigFile` (o `nls.config`) do idioma de exibição.
    pub get_translations: Box<dyn Fn(&str) -> TranslationsMap>,
}

impl ExtensionsScannerService {
    pub fn new(parts: ExtensionsScannerServiceParts) -> Self {
        let ExtensionsScannerServiceParts { locations, environment, product, current_profile, profiles, fs, logger, profile_scanner, get_translations } = parts;
        let scanner = ExtensionsScanner {
            fs: fs.clone(),
            logger: logger.clone(),
            profile_scanner: profile_scanner.clone(),
            product_quality: product.quality.clone(),
            auto_update_ids: product_auto_update_ids(&product, &environment),
        };
        Self {
            locations,
            environment,
            product,
            current_profile,
            profiles,
            fs,
            logger,
            profile_scanner,
            scanner,
            system_cached: CachedScanner { pending_validations: RefCell::new(Vec::new()) },
            user_cached: CachedScanner { pending_validations: RefCell::new(Vec::new()) },
            get_translations,
            on_did_change_cache: Emitter::new(),
            target_platform: RefCell::new(None),
        }
    }

    pub fn system_extensions_location(&self) -> &str {
        &self.locations.system_extensions_location
    }

    pub fn user_extensions_location(&self) -> &str {
        &self.locations.user_extensions_location
    }

    pub fn on_did_change_cache(&self) -> Event<ExtensionType> {
        self.on_did_change_cache.event()
    }

    fn target_platform(&self) -> TargetPlatform {
        *self.target_platform.borrow_mut().get_or_insert_with(|| compute_target_platform(self.fs.as_ref()))
    }

    /// `scanAllExtensions`.
    pub fn scan_all_extensions(&self, system: &SystemExtensionsScanOptions, user: &UserExtensionsScanOptions) -> Result<Vec<ScannedExtension>, ScanError> {
        let system_extensions = self.scan_system_extensions(system)?;
        let user_extensions = self.scan_user_extensions(user)?;
        Ok(self.dedup_extensions(Some(system_extensions), Some(user_extensions), None, self.target_platform(), true))
    }

    /// `scanSystemExtensions`.
    pub fn scan_system_extensions(&self, scan_options: &SystemExtensionsScanOptions) -> Result<Vec<ScannedExtension>, ScanError> {
        let default_system_extensions = self.scan_default_system_extensions(scan_options.language.as_deref())?;
        let dev_system_extensions = self.scan_dev_system_extensions(scan_options.language.as_deref(), scan_options.check_control_file)?;
        let mut all: Vec<ScannedExtension> = default_system_extensions.into_iter().chain(dev_system_extensions).collect();
        if !self.environment.skip_builtin_extensions.is_empty() {
            let skip: std::collections::HashSet<String> = self.environment.skip_builtin_extensions.iter().map(|id| id.to_lowercase()).collect();
            all.retain(|ext| !skip.contains(&ext.identifier.id.to_lowercase()));
        }
        Ok(self.apply_scan_options(all, ScanType::Type(ExtensionType::System), ApplyOptions { pick_latest: false, ..ApplyOptions::default() }))
    }

    /// `scanUserExtensions`.
    pub fn scan_user_extensions(&self, scan_options: &UserExtensionsScanOptions) -> Result<Vec<ScannedExtension>, ScanError> {
        self.logger.trace(&format!("Started scanning user extensions {}", scan_options.profile_location));
        let profile_scan_options = paths::is_equal(&scan_options.profile_location, &self.profiles.default_profile.extensions_resource)
            .then_some(ProfileExtensionsScanOptions { bail_out_when_file_not_found: Some(true) });
        let input = self.create_extension_scanner_input(
            &scan_options.profile_location,
            true,
            ExtensionType::User,
            scan_options.language.as_deref(),
            true,
            profile_scan_options,
            scan_options.product_version.clone().unwrap_or_else(|| self.product_version()),
        );
        let use_cache = scan_options.use_cache && !input.dev_mode;
        let scan = |input: &ExtensionScannerInput| if use_cache { self.cached_scan(&self.user_cached, input) } else { self.scanner.scan_extensions(input) };
        let extensions = match scan(&input) {
            Ok(extensions) => extensions,
            Err(ScanError::Profile(error)) if error.code == ExtensionsProfileScanningErrorCode::ErrorProfileNotFound => {
                self.do_initialize_default_profile_extensions();
                scan(&input)?
            }
            Err(error) => return Err(error),
        };
        let extensions = self.apply_scan_options(
            extensions,
            ScanType::Type(ExtensionType::User),
            ApplyOptions { include_invalid: scan_options.include_invalid, pick_latest: true, ..ApplyOptions::default() },
        );
        self.logger.trace(&format!("Scanned user extensions: {}", extensions.len()));
        Ok(extensions)
    }

    /// `scanAllUserExtensions`, com as opções padrão do original (`includeInvalid` e `includeAllVersions`).
    pub fn scan_all_user_extensions(&self) -> Result<Vec<ScannedExtension>, ScanError> {
        self.scan_all_user_extensions_with(true, true)
    }

    pub fn scan_all_user_extensions_with(&self, include_invalid: bool, include_all_versions: bool) -> Result<Vec<ScannedExtension>, ScanError> {
        let input = self.create_extension_scanner_input(&self.locations.user_extensions_location, false, ExtensionType::User, None, true, None, self.product_version());
        let extensions = self.scanner.scan_extensions(&input)?;
        Ok(self.apply_scan_options(
            extensions,
            ScanType::Type(ExtensionType::User),
            ApplyOptions { include_all_versions, include_invalid, pick_latest: false },
        ))
    }

    /// `scanExtensionsUnderDevelopment`.
    pub fn scan_extensions_under_development(&self, existing_extensions: &[ScannedExtension], scan_options: &ScanOptions) -> Vec<ScannedExtension> {
        if !(self.environment.is_extension_development && !self.environment.extension_development_locations.is_empty()) {
            return Vec::new();
        }
        let mut extensions = Vec::new();
        for location in &self.environment.extension_development_locations {
            let input = self.create_extension_scanner_input(location, false, ExtensionType::User, scan_options.language.as_deref(), false, None, self.product_version());
            for mut extension in self.scanner.scan_one_or_multiple_extensions(&input) {
                // Override the extension type from the existing extensions
                extension.extension_type = existing_extensions.iter().find(|e| are_same_extensions(&e.identifier, &extension.identifier)).map_or(extension.extension_type, |e| e.extension_type);
                // Validate the extension
                self.scanner.validate(&mut extension, &input);
                extensions.push(extension);
            }
        }
        self.apply_scan_options(extensions, ScanType::Development, ApplyOptions { include_invalid: scan_options.include_invalid, pick_latest: true, ..ApplyOptions::default() })
    }

    /// `scanExistingExtension`.
    pub fn scan_existing_extension(&self, extension_location: &str, extension_type: ExtensionType, scan_options: &ScanOptions) -> Result<Option<ScannedExtension>, ScanError> {
        let input = self.create_extension_scanner_input(extension_location, false, extension_type, scan_options.language.as_deref(), true, None, self.product_version());
        let Some(extension) = self.scanner.scan_extension(&input, None)? else { return Ok(None) };
        if !scan_options.include_invalid && !extension.is_valid {
            return Ok(None);
        }
        Ok(Some(extension))
    }

    /// `scanOneOrMultipleExtensions`.
    pub fn scan_one_or_multiple_extensions(&self, extension_location: &str, extension_type: ExtensionType, scan_options: &ScanOptions) -> Vec<ScannedExtension> {
        let input = self.create_extension_scanner_input(extension_location, false, extension_type, scan_options.language.as_deref(), true, None, self.product_version());
        let extensions = self.scanner.scan_one_or_multiple_extensions(&input);
        self.apply_scan_options(extensions, ScanType::Type(extension_type), ApplyOptions { include_invalid: scan_options.include_invalid, pick_latest: true, ..ApplyOptions::default() })
    }

    /// `scanMultipleExtensions`.
    pub fn scan_multiple_extensions(&self, extension_locations: &[String], extension_type: ExtensionType, scan_options: &ScanOptions) -> Vec<ScannedExtension> {
        let mut extensions = Vec::new();
        for location in extension_locations {
            extensions.extend(self.scan_one_or_multiple_extensions(location, extension_type, scan_options));
        }
        self.apply_scan_options(extensions, ScanType::Type(extension_type), ApplyOptions { include_invalid: scan_options.include_invalid, pick_latest: true, ..ApplyOptions::default() })
    }

    /// `updateManifestMetadata`: escreve o `__metadata` no `package.json`, com indentação de tabulação.
    pub fn update_manifest_metadata(&self, extension_location: &str, metadata: &ManifestMetadata) -> Result<(), ScanError> {
        let manifest_location = paths::join_path(extension_location, &["package.json"]);
        let content = self.fs.read_to_string(&manifest_location)?;
        let mut manifest: Value = serde_json::from_str(strip_bom(&content)).map_err(|e| ScanError::Other(e.to_string()))?;
        let object = manifest.as_object_mut().ok_or_else(|| ScanError::Other("Invalid manifest: Not a JSON object.".into()))?;
        let mut merged = object.get("__metadata").and_then(Value::as_object).cloned().unwrap_or_default();
        if let Some(platform) = metadata.target_platform {
            merged.insert("targetPlatform".into(), serde_json::to_value(platform).expect("plataforma serializa"));
        }
        if let Some(timestamp) = metadata.installed_timestamp {
            merged.insert("installedTimestamp".into(), number_value(timestamp));
        }
        if let Some(size) = metadata.size {
            merged.insert("size".into(), number_value(size));
        }
        object.insert("__metadata".into(), Value::Object(merged));
        self.fs.write(&manifest_location, to_string_tab_indented(&manifest).as_bytes())?;
        Ok(())
    }

    /// `initializeDefaultProfileExtensions`.
    pub fn initialize_default_profile_extensions(&self) -> Result<(), ScanError> {
        match self.profile_scanner.scan_profile_extensions(
            &self.profiles.default_profile.extensions_resource,
            Some(ProfileExtensionsScanOptions { bail_out_when_file_not_found: Some(true) }),
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.code == ExtensionsProfileScanningErrorCode::ErrorProfileNotFound => {
                self.do_initialize_default_profile_extensions();
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn do_initialize_default_profile_extensions(&self) {
        let outcome = (|| -> Result<(), ScanError> {
            self.logger.info(&format!("Started initializing default profile extensions in extensions installation folder. {}", self.locations.user_extensions_location));
            let user_extensions = self.scan_all_user_extensions_with(true, false)?;
            if !user_extensions.is_empty() {
                let sources: Vec<(ProfileExtensionSource, Option<Metadata>)> =
                    user_extensions.iter().map(|e| (to_profile_source(e), e.metadata.clone())).collect();
                self.profile_scanner.add_extensions_to_profile(&sources, &self.profiles.default_profile.extensions_resource, false)?;
            } else if let Err(error) = self.fs.create(&self.profiles.default_profile.extensions_resource, b"[]")
                && !is_not_found(&error)
            {
                self.logger.warn(&format!(
                    "Failed to create default profile extensions manifest in extensions installation folder. {} {error}",
                    self.locations.user_extensions_location
                ));
            }
            self.logger.info(&format!("Completed initializing default profile extensions in extensions installation folder. {}", self.locations.user_extensions_location));
            Ok(())
        })();
        if let Err(error) = outcome {
            self.logger.error(&error.to_string());
        }
    }

    fn apply_scan_options(&self, mut extensions: Vec<ScannedExtension>, scan_type: ScanType, options: ApplyOptions) -> Vec<ScannedExtension> {
        if !options.include_all_versions {
            let (system, user, development) = match scan_type {
                ScanType::Type(ExtensionType::System) => (Some(extensions), None, None),
                ScanType::Type(ExtensionType::User) => (None, Some(extensions), None),
                ScanType::Development => (None, None, Some(extensions)),
            };
            extensions = self.dedup_extensions(system, user, development, self.target_platform(), options.pick_latest);
        }
        if !options.include_invalid {
            extensions.retain(|extension| extension.is_valid);
        }
        extensions.sort_by_key(|extension| paths::basename(&extension.location));
        extensions
    }

    fn dedup_extensions(
        &self,
        system: Option<Vec<ScannedExtension>>,
        user: Option<Vec<ScannedExtension>>,
        development: Option<Vec<ScannedExtension>>,
        target_platform: TargetPlatform,
        pick_latest: bool,
    ) -> Vec<ScannedExtension> {
        let pick = |existing: &ScannedExtension, extension: &ScannedExtension, is_development: bool| -> bool {
            if !is_development && !(existing.is_builtin || extension.is_builtin) {
                let scoped = |e: &ScannedExtension| e.metadata.as_ref().is_some_and(|m| m.is_application_scoped == Some(true));
                if scoped(existing) && !scoped(extension) {
                    return false;
                }
                if !scoped(existing) && scoped(extension) {
                    return true;
                }
            }
            if existing.is_valid && !extension.is_valid {
                return false;
            }
            if existing.is_valid == extension.is_valid {
                if pick_latest && semver::gt(&existing.manifest.version, &extension.manifest.version) {
                    self.logger.debug(&format!(
                        "Skipping extension {} with lower version {} in favour of {} with version {}",
                        extension.location, extension.manifest.version, existing.location, existing.manifest.version
                    ));
                    return false;
                }
                if semver::eq(&existing.manifest.version, &extension.manifest.version) {
                    if existing.extension_type == ExtensionType::System {
                        self.logger.debug(&format!("Skipping extension {} in favour of system extension {} with same version", extension.location, existing.location));
                        return false;
                    }
                    if existing.target_platform == target_platform {
                        self.logger.debug(&format!("Skipping extension {} from different target platform {}", extension.location, extension.target_platform));
                        return false;
                    }
                }
            }
            if is_development {
                self.logger.warn(&format!("Overwriting user extension {} with {}.", existing.location, extension.location));
            } else {
                self.logger.debug(&format!("Overwriting user extension {} with {}.", existing.location, extension.location));
            }
            true
        };

        let mut result: ExtensionIdentifierMap<ScannedExtension> = ExtensionIdentifierMap::new();
        let has_system = system.is_some();
        for extension in system.into_iter().flatten() {
            let existing = result.get(&extension.identifier.id);
            if existing.is_none_or(|existing| pick(existing, &extension, false)) {
                result.set(&extension.identifier.id.clone(), extension);
            }
        }
        let auto_update = product_auto_update_ids(&self.product, &self.environment);
        for extension in user.into_iter().flatten() {
            let existing = result.get(&extension.identifier.id);
            if existing.is_none() && has_system && extension.extension_type == ExtensionType::System {
                self.logger.debug(&format!("Skipping obsolete system extension {}.", extension.location));
                continue;
            }
            if auto_update.contains(&extension.identifier.id.to_lowercase()) && !extension.force_auto_update {
                self.logger.info(&format!(
                    "Skipping user installed builtin extension {} with version {} because it is not allowed to in the current product quality {}",
                    extension.identifier.id,
                    extension.manifest.version,
                    self.product.quality.as_deref().unwrap_or("undefined")
                ));
                continue;
            }
            if existing.is_none_or(|existing| pick(existing, &extension, false)) {
                result.set(&extension.identifier.id.clone(), extension);
            }
        }
        for extension in development.into_iter().flatten() {
            let existing = result.get(&extension.identifier.id);
            // O original grava a extensão em desenvolvimento em qualquer caso, depois de consultar o `pick` (que só
            // registra o aviso de sobrescrita).
            if let Some(existing) = existing {
                pick(existing, &extension, true);
            }
            result.set(&extension.identifier.id.clone(), extension);
        }
        result.into_values().collect()
    }

    fn scan_default_system_extensions(&self, language: Option<&str>) -> Result<Vec<ScannedExtension>, ScanError> {
        self.logger.trace("Started scanning system extensions");
        let input = self.create_extension_scanner_input(&self.locations.system_extensions_location, false, ExtensionType::System, language, true, None, self.product_version());
        let result = if input.dev_mode { self.scanner.scan_extensions(&input)? } else { self.cached_scan(&self.system_cached, &input)? };
        self.logger.trace(&format!("Scanned system extensions: {}", result.len()));
        Ok(result)
    }

    fn scan_dev_system_extensions(&self, language: Option<&str>, check_control_file: bool) -> Result<Vec<ScannedExtension>, ScanError> {
        let dev_system_extensions_list: &[String] = if self.environment.is_built { &[] } else { &self.product.built_in_extensions };
        if dev_system_extensions_list.is_empty() {
            return Ok(Vec::new());
        }
        self.logger.trace("Started scanning dev system extensions");
        let control = if check_control_file { self.get_built_in_extension_control() } else { HashMap::new() };
        let mut locations = Vec::new();
        for name in dev_system_extensions_list {
            let state = control.get(name).filter(|s| !s.is_empty()).map_or("marketplace", String::as_str);
            match state {
                "disabled" => {}
                "marketplace" => locations.push(paths::join_path(&self.locations.dev_builtin_extensions_location, &[name])),
                other => locations.push(paths::from_fs_path(other)),
            }
        }
        let mut result = Vec::new();
        for location in locations {
            let input = self.create_extension_scanner_input(&location, false, ExtensionType::System, language, true, None, self.product_version());
            if let Some(extension) = self.scanner.scan_extension(&input, None)? {
                result.push(extension);
            }
        }
        self.logger.trace(&format!("Scanned dev system extensions: {}", result.len()));
        Ok(result)
    }

    fn get_built_in_extension_control(&self) -> HashMap<String, String> {
        let parsed = self.fs.read_to_string(&self.locations.extensions_control_location).ok().and_then(|c| serde_json::from_str::<Value>(strip_bom(&c)).ok());
        match parsed {
            Some(Value::Object(map)) => map.into_iter().filter_map(|(k, v)| Some((k, v.as_str()?.to_owned()))).collect(),
            _ => HashMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_extension_scanner_input(
        &self,
        location: &str,
        profile: bool,
        extension_type: ExtensionType,
        language: Option<&str>,
        validate: bool,
        profile_scan_options: Option<ProfileExtensionsScanOptions>,
        product_version: ProductVersion,
    ) -> ExtensionScannerInput {
        let translations = (self.get_translations)(language.unwrap_or(&self.environment.language));
        let mtime = self.get_mtime(location);
        let default_resource = &self.profiles.default_profile.extensions_resource;
        let application_extensions_location = (profile && !paths::is_equal(location, default_resource)).then(|| default_resource.clone());
        let application_extensions_location_mtime = application_extensions_location.as_deref().and_then(|l| self.get_mtime(l));
        ExtensionScannerInput {
            location: location.to_owned(),
            mtime,
            application_extensions_location,
            application_extensions_location_mtime,
            profile,
            profile_scan_options,
            extension_type,
            validate,
            product_version: product_version.version,
            product_date: product_version.date,
            product_commit: self.product.commit.clone(),
            dev_mode: !self.environment.is_built,
            language: language.map(str::to_owned),
            translations,
        }
    }

    fn get_mtime(&self, location: &str) -> Option<u64> {
        self.fs.stat(location).ok().map(|stat| stat.mtime)
    }

    fn product_version(&self) -> ProductVersion {
        ProductVersion { version: self.product.version.clone(), date: self.product.date.clone() }
    }

    // ---- cache ----

    fn cached_scan(&self, cache: &CachedScanner, input: &ExtensionScannerInput) -> Result<Vec<ScannedExtension>, ScanError> {
        let cache_file = self.cache_file(input);
        if let Some((cached_input, cached_result)) = self.read_extension_cache(&cache_file)
            && ExtensionScannerInput::equals(&cached_input, input)
        {
            let revived: Option<Vec<ScannedExtension>> = cached_result.iter().map(ScannedExtension::from_json).collect();
            if let Some(extensions) = revived {
                self.logger.debug(&format!(
                    "Using cached extensions scan result {} {}",
                    if input.extension_type == ExtensionType::System { "system" } else { "user" },
                    input.location
                ));
                // Each cache file needs validating on its own, scanning one language says nothing about the others
                let mut pending = cache.pending_validations.borrow_mut();
                pending.retain(|(file, _)| file != &cache_file);
                pending.push((cache_file, input.clone()));
                return Ok(extensions);
            }
        }
        let result = self.scanner.scan_extensions(input)?;
        self.write_extension_cache(&cache_file, input, &result);
        Ok(result)
    }

    fn read_extension_cache(&self, cache_file: &str) -> Option<(ExtensionScannerInput, Vec<Value>)> {
        let raw = match self.fs.read_to_string(cache_file) {
            Ok(raw) => raw,
            Err(error) => {
                if !is_not_found(&error) {
                    self.logger.debug(&format!("Error while reading the extension cache file: {cache_file} {error}"));
                }
                return None;
            }
        };
        let parsed: Value = serde_json::from_str(strip_bom(&raw)).ok()?;
        let input = ExtensionScannerInput::from_json(parsed.get("input")?)?;
        let result = parsed.get("result")?.as_array()?.clone();
        Some((input, result))
    }

    fn write_extension_cache(&self, cache_file: &str, input: &ExtensionScannerInput, result: &[ScannedExtension]) {
        let mut object = Map::new();
        object.insert("input".into(), input.to_json());
        object.insert("result".into(), Value::Array(result.iter().map(ScannedExtension::to_json).collect()));
        if let Err(error) = self.fs.write(cache_file, Value::Object(object).to_string().as_bytes()) {
            self.logger.debug(&format!("Error while writing the extension cache file: {cache_file} {error}"));
        }
    }

    /// `validatePendingCaches`: reescaneia cada cache que serviu um resultado e apaga o que divergiu.
    pub fn validate_pending_caches(&self) {
        for (cache, extension_type) in [(&self.system_cached, ExtensionType::System), (&self.user_cached, ExtensionType::User)] {
            let pending: Vec<(String, ExtensionScannerInput)> = std::mem::take(&mut *cache.pending_validations.borrow_mut());
            let mut changed = false;
            for (cache_file, input) in pending {
                changed |= self.validate_cache(&cache_file, &input);
            }
            if changed {
                self.on_did_change_cache.fire(extension_type);
            }
        }
    }

    /// `validateCache`: `true` quando o cache era inválido e foi apagado.
    fn validate_cache(&self, cache_file: &str, input: &ExtensionScannerInput) -> bool {
        let Some((_, actual)) = self.read_extension_cache(cache_file) else {
            // Cache has been deleted by someone else, which is perfectly fine...
            return false;
        };
        let expected: Vec<Value> = match self.scanner.scan_extensions(input) {
            Ok(result) => result.iter().map(ScannedExtension::to_json).collect(),
            Err(_) => Vec::new(),
        };
        if expected == actual {
            // Cache is valid and running with it is perfectly fine...
            return false;
        }
        self.logger.info("Invalidating Cache");
        match self.fs.remove(cache_file, false) {
            Ok(()) => true,
            Err(error) => {
                self.logger.error(&error.to_string());
                false
            }
        }
    }

    fn cache_file(&self, input: &ExtensionScannerInput) -> String {
        let profile = self.profile_of(input);
        paths::join_path(&profile.cache_home, &[&get_manifest_cache_file_name(input.extension_type, input.language.as_deref())])
    }

    fn profile_of(&self, input: &ExtensionScannerInput) -> &UserDataProfile {
        if input.extension_type == ExtensionType::System || !input.profile {
            return &self.profiles.default_profile;
        }
        if paths::is_equal(&input.location, &self.current_profile.extensions_resource) {
            return &self.current_profile;
        }
        self.profiles.profiles.iter().find(|p| paths::is_equal(&input.location, &p.extensions_resource)).unwrap_or(&self.current_profile)
    }
}

fn number_value(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 { Value::from(number as i64) } else { Value::from(number) }
}

fn to_profile_source(extension: &ScannedExtension) -> ProfileExtensionSource {
    ProfileExtensionSource { identifier: extension.identifier.clone(), version: extension.manifest.version.clone(), location: extension.location.clone() }
}

#[derive(Clone, Copy)]
enum ScanType {
    Type(ExtensionType),
    Development,
}

#[derive(Clone, Copy, Default)]
struct ApplyOptions {
    include_all_versions: bool,
    include_invalid: bool,
    pick_latest: bool,
}

/// `IExtensionDescription`, reduzida ao que o extension host lê.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionDescription {
    pub id: String,
    pub identifier: ExtensionIdentifier,
    pub is_builtin: bool,
    pub is_user_builtin: bool,
    pub is_under_development: bool,
    pub extension_location: String,
    pub uuid: Option<String>,
    pub target_platform: TargetPlatform,
    pub publisher_display_name: Option<String>,
    pub pre_release: bool,
    pub manifest: ExtensionManifest,
}

/// `toExtensionDescription`.
pub fn to_extension_description(extension: &ScannedExtension, is_under_development: bool) -> ExtensionDescription {
    let id = get_extension_id(&extension.manifest.publisher, &extension.manifest.name);
    ExtensionDescription {
        identifier: ExtensionIdentifier::new(id.clone()),
        id,
        is_builtin: extension.extension_type == ExtensionType::System,
        is_user_builtin: extension.extension_type == ExtensionType::User && extension.is_builtin,
        is_under_development,
        extension_location: extension.location.clone(),
        uuid: extension.identifier.uuid.clone(),
        target_platform: extension.target_platform,
        publisher_display_name: extension.publisher_display_name.clone(),
        pre_release: extension.pre_release,
        manifest: extension.manifest.clone(),
    }
}

/// O texto de origem da instalação de um `Metadata` ausente: galeria se há `uuid`, senão VSIX.
pub fn default_install_source(extension: &ScannedExtension) -> InstallSource {
    extension.metadata.as_ref().and_then(|m| m.source).unwrap_or(if extension.identifier.uuid.is_some() { InstallSource::Gallery } else { InstallSource::Vsix })
}
