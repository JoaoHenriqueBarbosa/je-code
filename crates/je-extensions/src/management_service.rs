//! Porte de `src/vs/platform/extensionManagement/node/extensionManagementService.ts` e da parte de
//! `common/abstractExtensionManagementService.ts` que o fluxo de instalação usa: instalar um VSIX, instalar do
//! marketplace, desinstalar, atualizar metadados, marcar para remoção (`.obsolete`) e a limpeza.
//!
//! Layout em disco, o mesmo do VS Code: cada extensão numa pasta `publisher.name-version[-targetPlatform]` dentro
//! da pasta de extensões; o `extensions.json` do perfil lista o que está instalado; o `.obsolete` (um JSON
//! `{ "<pasta>": true }`) lista as pastas marcadas para remoção, apagadas na limpeza.
//!
//! Diferenças de execução em relação ao original:
//! - Síncrono: não há tarefas paralelas, fila de instalação nem cancelamento. Os eventos de instalar e desinstalar
//!   disparam em ordem.
//! - Dependências e packs são instalados do marketplace antes da extensão principal (o original instala o conjunto
//!   em paralelo e desfaz tudo se algo falha); uma falha numa dependência aborta antes da principal.
//! - A verificação de assinatura do VSIX (`extensionSignatureVerificationService`, o módulo nativo `@vscode/vsce-sign`)
//!   não existe aqui: toda instalação do marketplace segue como `donotVerifySignature`.
//!   TODO(integração): trait de verificação quando houver uma implementação em Rust.

use std::io::Cursor;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use je_base::event::{Emitter, Event};
use je_base::uuid::generate_uuid;
use je_platform::log::Logger;
use serde_json::Value;

use crate::allowed::{AllowedCandidate, AllowedExtensionsService};
use crate::fs::{ExtFs, compute_size, is_not_found};
use crate::gallery::GalleryApi;
use crate::lifecycle;
use crate::management::{
    ExtensionInfo, ExtensionManagementError, ExtensionManagementErrorCode, ExtensionsControlManifest, GalleryExtension, InstallOperation, InstallOptions,
    InstallSource, LocalExtension, Metadata, ProductVersion, StatisticType, UninstallOptions, get_extension_blocked_message, is_target_platform_compatible,
    target_platform_to_string, to_extension_management_error,
};
use crate::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use crate::paths;
use crate::profile::UserDataProfiles;
use crate::profile_scanner::{ExtensionsProfileScannerService, ProfileExtensionSource};
use crate::scanner::{ExtensionsProduct, ExtensionsScannerService, ManifestMetadata, ScanOptions, ScannedExtension, SystemExtensionsScanOptions, UserExtensionsScanOptions};
use crate::util::{ExtensionKey, are_same_extensions, find_matching_malicious_entry, get_gallery_extension_id, group_by_extension};
use crate::validator::is_engine_valid;
use crate::vsix;

/// O sufixo das pastas apagadas em duas etapas (renomeadas antes do `rm`).
const DELETED_FOLDER_POSTFIX: &str = ".vsctmp";

type ManagementResult<T> = Result<T, ExtensionManagementError>;

/// O conteúdo do `.obsolete`: pasta da extensão para `true`.
type RemovedExtensions = indexmap::IndexMap<String, bool>;

/// De onde vem a extensão a instalar.
enum InstallSourceRef<'a> {
    /// Um VSIX, pelo caminho (da URI) do arquivo.
    Vsix(&'a str),
    Gallery(&'a GalleryExtension),
}

/// `InstallExtensionResult`.
#[derive(Clone, Debug)]
pub struct InstallExtensionResult {
    pub identifier: ExtensionIdentifierInfo,
    pub operation: InstallOperation,
    pub local: Option<LocalExtension>,
    pub error: Option<ExtensionManagementError>,
    pub profile_location: String,
}

/// `DidUninstallExtensionEvent`.
#[derive(Clone, Debug)]
pub struct DidUninstallExtensionEvent {
    pub identifier: ExtensionIdentifierInfo,
    pub error: Option<ExtensionManagementErrorCode>,
    pub profile_location: String,
    pub application_scoped: bool,
}

/// O que o serviço precisa, agrupado.
pub struct ExtensionManagementServiceParts {
    pub fs: Rc<dyn ExtFs>,
    pub logger: Rc<dyn Logger>,
    pub scanner: Rc<ExtensionsScannerService>,
    pub profile_scanner: Rc<ExtensionsProfileScannerService>,
    pub profiles: UserDataProfiles,
    /// `getCurrentExtensionsManifestLocation()`: o `extensions.json` do perfil atual.
    pub current_profile_location: String,
    pub product: ExtensionsProduct,
    pub allowed: Rc<AllowedExtensionsService>,
    pub gallery: Rc<dyn GalleryApi>,
    /// A pasta dos VSIX baixados (`extensionsDownloader.extensionsDownloadDir`).
    pub downloads_dir: String,
    pub target_platform: TargetPlatform,
    /// `globalStorageHome` do perfil padrão.
    pub global_storage_home: String,
}

/// `ExtensionManagementService` (nativo).
pub struct ExtensionManagementService {
    fs: Rc<dyn ExtFs>,
    logger: Rc<dyn Logger>,
    scanner: Rc<ExtensionsScannerService>,
    profile_scanner: Rc<ExtensionsProfileScannerService>,
    profiles: UserDataProfiles,
    current_profile_location: String,
    product: ExtensionsProduct,
    allowed: Rc<AllowedExtensionsService>,
    gallery: Rc<dyn GalleryApi>,
    downloads_dir: String,
    target_platform: TargetPlatform,
    global_storage_home: String,
    obsolete_resource: String,
    on_did_install_extensions: Rc<Emitter<Vec<InstallExtensionResult>>>,
    on_did_uninstall_extension: Rc<Emitter<DidUninstallExtensionEvent>>,
    on_did_update_extension_metadata: Rc<Emitter<(LocalExtension, String)>>,
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

fn error(message: impl Into<String>, code: ExtensionManagementErrorCode) -> ExtensionManagementError {
    ExtensionManagementError::new(message, code)
}

fn io_error(error: &std::io::Error, code: ExtensionManagementErrorCode) -> ExtensionManagementError {
    to_extension_management_error(error, code)
}

impl ExtensionManagementService {
    pub fn new(parts: ExtensionManagementServiceParts) -> Self {
        let obsolete_resource = paths::join_path(parts.scanner.user_extensions_location(), &[".obsolete"]);
        Self {
            fs: parts.fs,
            logger: parts.logger,
            scanner: parts.scanner,
            profile_scanner: parts.profile_scanner,
            profiles: parts.profiles,
            current_profile_location: parts.current_profile_location,
            product: parts.product,
            allowed: parts.allowed,
            gallery: parts.gallery,
            downloads_dir: parts.downloads_dir,
            target_platform: parts.target_platform,
            global_storage_home: parts.global_storage_home,
            obsolete_resource,
            on_did_install_extensions: Emitter::new(),
            on_did_uninstall_extension: Emitter::new(),
            on_did_update_extension_metadata: Emitter::new(),
        }
    }

    pub fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        self.on_did_install_extensions.event()
    }

    pub fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        self.on_did_uninstall_extension.event()
    }

    pub fn on_did_update_extension_metadata(&self) -> Event<(LocalExtension, String)> {
        self.on_did_update_extension_metadata.event()
    }

    pub fn target_platform(&self) -> TargetPlatform {
        self.target_platform
    }

    fn product_version(&self) -> ProductVersion {
        ProductVersion { version: self.product.version.clone(), date: self.product.date.clone() }
    }

    fn profile_or_current<'a>(&'a self, profile_location: Option<&'a str>) -> &'a str {
        profile_location.unwrap_or(&self.current_profile_location)
    }

    // ---- leitura ----

    /// `getInstalled`: `extension_type` `None` traz as embutidas e as do usuário.
    pub fn get_installed(
        &self,
        extension_type: Option<ExtensionType>,
        profile_location: Option<&str>,
        product_version: Option<ProductVersion>,
        language: Option<&str>,
    ) -> ManagementResult<Vec<LocalExtension>> {
        let profile_location = profile_location.unwrap_or(&self.profiles.default_profile.extensions_resource).to_owned();
        let user_options = UserExtensionsScanOptions {
            profile_location,
            include_invalid: true,
            language: language.map(str::to_owned),
            product_version: product_version.or_else(|| Some(self.product_version())),
            use_cache: false,
        };
        let scan_error = |e: crate::scanner::ScanError| error(e.to_string(), ExtensionManagementErrorCode::Scanning);
        let scanned = match extension_type {
            Some(ExtensionType::System) => self.scanner.scan_system_extensions(&SystemExtensionsScanOptions { language: language.map(str::to_owned), ..Default::default() }).map_err(scan_error)?,
            Some(ExtensionType::User) => self.scanner.scan_user_extensions(&user_options).map_err(scan_error)?,
            None => self
                .scanner
                .scan_all_extensions(&SystemExtensionsScanOptions { language: language.map(str::to_owned), ..Default::default() }, &user_options)
                .map_err(scan_error)?,
        };
        Ok(scanned.iter().map(|e| self.to_local_extension(e)).collect())
    }

    /// `scanAllUserInstalledExtensions`: todas as versões na pasta de extensões, instaladas ou não em algum perfil.
    pub fn scan_all_user_installed_extensions(&self) -> ManagementResult<Vec<LocalExtension>> {
        let scanned = self.scanner.scan_all_user_extensions().map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::Scanning))?;
        Ok(scanned.iter().map(|e| self.to_local_extension(e)).collect())
    }

    /// `scanInstalledExtensionAtLocation`.
    pub fn scan_installed_extension_at_location(&self, location: &str) -> Option<LocalExtension> {
        match self.scanner.scan_existing_extension(location, ExtensionType::User, &ScanOptions { include_invalid: true, language: None }) {
            Ok(extension) => extension.map(|e| self.to_local_extension(&e)),
            Err(e) => {
                self.logger.error(&e.to_string());
                None
            }
        }
    }

    /// `getManifest(vsix)`: o `extension/package.json` do VSIX.
    pub fn get_manifest(&self, vsix_path: &str) -> ManagementResult<Value> {
        let data = self.fs.read(vsix_path).map_err(|e| io_error(&e, ExtensionManagementErrorCode::Extract))?;
        vsix::get_manifest(Cursor::new(data))
    }

    /// `toLocalExtension`.
    fn to_local_extension(&self, extension: &ScannedExtension) -> LocalExtension {
        let children = self.fs.read_dir(&extension.location).unwrap_or_default();
        let matches = |prefix: &str| {
            children.iter().find(|c| {
                let lower = c.name.to_lowercase();
                lower == prefix || lower == format!("{prefix}.txt") || lower == format!("{prefix}.md")
            })
        };
        let metadata = extension.metadata.as_ref();
        LocalExtension {
            identifier: extension.identifier.clone(),
            extension_type: extension.extension_type,
            is_builtin: extension.is_builtin || metadata.is_some_and(|m| m.is_builtin == Some(true)),
            location: extension.location.clone(),
            manifest: extension.manifest.clone(),
            target_platform: extension.target_platform,
            validations: extension.validations.clone(),
            is_valid: extension.is_valid,
            readme_url: matches("readme").map(|c| paths::join_path(&extension.location, &[&c.name])),
            changelog_url: matches("changelog").map(|c| paths::join_path(&extension.location, &[&c.name])),
            publisher_display_name: metadata.and_then(|m| m.publisher_display_name.clone()),
            publisher_id: metadata.and_then(|m| m.publisher_id.clone()),
            is_application_scoped: metadata.is_some_and(|m| m.is_application_scoped == Some(true)),
            is_machine_scoped: metadata.is_some_and(|m| m.is_machine_scoped == Some(true)),
            is_pre_release_version: metadata.is_some_and(|m| m.is_pre_release_version == Some(true)),
            has_pre_release_version: metadata.is_some_and(|m| m.has_pre_release_version == Some(true)),
            pre_release: extension.pre_release,
            installed_timestamp: metadata.and_then(|m| m.installed_timestamp),
            updated: metadata.is_some_and(|m| m.updated == Some(true)),
            pinned: metadata.is_some_and(|m| m.pinned == Some(true)),
            force_auto_update: extension.force_auto_update,
            private: metadata.is_some_and(|m| m.private == Some(true)),
            is_workspace_scoped: false,
            source: metadata.and_then(|m| m.source).unwrap_or(if extension.identifier.uuid.is_some() { InstallSource::Gallery } else { InstallSource::Vsix }),
            size: metadata.and_then(|m| m.size).unwrap_or(0.0),
        }
    }

    fn scan_local_extension(&self, location: &str, extension_type: ExtensionType, profile_location: Option<&str>) -> ManagementResult<LocalExtension> {
        let scan_error = |e: crate::scanner::ScanError| error(e.to_string(), ExtensionManagementErrorCode::ScanningExtension);
        if let Some(profile_location) = profile_location {
            let scanned = self
                .scanner
                .scan_user_extensions(&UserExtensionsScanOptions { profile_location: profile_location.to_owned(), include_invalid: true, ..Default::default() })
                .map_err(scan_error)?;
            if let Some(found) = scanned.iter().find(|e| paths::is_equal(&e.location, location)) {
                return Ok(self.to_local_extension(found));
            }
        } else if let Some(found) = self.scanner.scan_existing_extension(location, extension_type, &ScanOptions { include_invalid: true, language: None }).map_err(scan_error)? {
            return Ok(self.to_local_extension(&found));
        }
        Err(error(format!("Cannot read the extension from {location}"), ExtensionManagementErrorCode::ScanningExtension))
    }

    // ---- verificação de instalação ----

    /// `canInstall`: `Ok(())` ou a mensagem do motivo.
    pub fn can_install(&self, extension: &GalleryExtension) -> Result<(), String> {
        if let Some(blocking_info) = &extension.blocking_info {
            return Err(get_extension_blocked_message(blocking_info));
        }
        let candidate = AllowedCandidate::Fields {
            id: &extension.identifier.id,
            publisher_display_name: Some(&extension.publisher_display_name),
            version: None,
            prerelease: None,
            target_platform: None,
        };
        if let Err(reason) = self.allowed.is_allowed(&candidate) {
            return Err(format!("This extension cannot be installed because {reason}"));
        }
        if !self.is_extension_platform_compatible(extension) {
            return Err(format!(
                "The '{}' extension is not available in {} for the {} platform. [Learn Why](https://aka.ms/vscode-platform-specific-extensions)",
                if extension.display_name.is_empty() { &extension.identifier.id } else { &extension.display_name },
                self.product.name_long,
                target_platform_to_string(self.target_platform)
            ));
        }
        Ok(())
    }

    fn is_extension_platform_compatible(&self, extension: &GalleryExtension) -> bool {
        extension.all_target_platforms.iter().any(|platform| is_target_platform_compatible(*platform, &extension.all_target_platforms, self.target_platform))
    }

    fn control_manifest(&self) -> ExtensionsControlManifest {
        match self.gallery.get_extensions_control_manifest() {
            Ok(manifest) => manifest,
            Err(e) => {
                self.logger.trace(&format!("ExtensionManagementService.refreshControlCache - failed to get extension control manifest {e}"));
                ExtensionsControlManifest::default()
            }
        }
    }

    /// `checkAndGetCompatibleVersion`: a versão a instalar e o manifesto dela.
    fn check_and_get_compatible_version(&self, extension: &GalleryExtension, same_version: bool, install_pre_release: bool, product_version: &ProductVersion) -> ManagementResult<(GalleryExtension, ExtensionManifest)> {
        let control = self.control_manifest();
        let gallery_error = |e: crate::management::ExtensionGalleryError| error(e.message, ExtensionManagementErrorCode::Gallery);
        if find_matching_malicious_entry(&extension.identifier, &control.malicious).is_some() {
            return Err(error(format!("Can't install '{}' extension since it was reported to be problematic.", extension.identifier.id), ExtensionManagementErrorCode::Malicious));
        }
        let deprecation = control.deprecated.get(&extension.identifier.id.to_lowercase());
        let compatible = if let Some(replacement) = deprecation.and_then(|d| d.extension.as_ref()).filter(|e| e.get("autoMigrate").is_some_and(|a| !a.is_null())) {
            let replacement_id = replacement.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            self.logger.info(&format!("The '{}' extension is deprecated, fetching the compatible '{replacement_id}' extension instead.", extension.identifier.id));
            let found = self
                .gallery
                .get_extensions(
                    &[ExtensionInfo { id: replacement_id.clone(), pre_release: replacement.get("preRelease").and_then(Value::as_bool), ..Default::default() }],
                    &crate::management::ExtensionQueryOptions { target_platform: Some(self.target_platform), compatible: true, product_version: Some(product_version.clone()), ..Default::default() },
                )
                .map_err(gallery_error)?;
            found.into_iter().next().ok_or_else(|| {
                error(
                    format!("Can't install '{}' extension since it was deprecated and the replacement extension '{replacement_id}' can't be found.", extension.identifier.id),
                    ExtensionManagementErrorCode::Deprecated,
                )
            })?
        } else {
            if let Some(blocking_info) = &extension.blocking_info {
                return Err(error(get_extension_blocked_message(blocking_info), ExtensionManagementErrorCode::NotAllowed));
            }
            if self.can_install(extension).is_err() {
                return Err(error(
                    format!(
                        "The '{}' extension is not available in {} for the {} platform.",
                        extension.identifier.id,
                        self.product.name_long,
                        target_platform_to_string(self.target_platform)
                    ),
                    ExtensionManagementErrorCode::IncompatibleTargetPlatform,
                ));
            }
            match self.get_compatible_version(extension, same_version, install_pre_release, product_version).map_err(gallery_error)? {
                Some(compatible) => compatible,
                None => {
                    // If no compatible release version is found, check if the extension has a release version or not and throw relevant error
                    if !install_pre_release
                        && extension.has_pre_release_version
                        && extension.properties.is_pre_release_version
                        && !self.gallery.get_extensions(&[ExtensionInfo { id: extension.identifier.id.clone(), uuid: extension.identifier.uuid.clone(), ..Default::default() }], &Default::default()).map_err(gallery_error)?.is_empty()
                    {
                        return Err(error(
                            format!("Can't install release version of '{}' extension because it has no release version.", if extension.display_name.is_empty() { &extension.identifier.id } else { &extension.display_name }),
                            ExtensionManagementErrorCode::ReleaseVersionNotFound,
                        ));
                    }
                    return Err(error(
                        format!(
                            "Can't install '{}' extension because it is not compatible with the current version of {} (version {}).",
                            extension.identifier.id, self.product.name_long, product_version.version
                        ),
                        ExtensionManagementErrorCode::Incompatible,
                    ));
                }
            }
        };
        self.logger.info(&format!("Getting Manifest... {}", compatible.identifier.id));
        let manifest = self
            .gallery
            .get_manifest(&compatible)
            .map_err(gallery_error)?
            .ok_or_else(|| error(format!("Missing manifest for extension {}", compatible.identifier.id), ExtensionManagementErrorCode::Invalid))?;
        if manifest.get("version").and_then(Value::as_str) != Some(compatible.version.as_str()) {
            return Err(error(
                format!("Cannot install '{}' extension because of version mismatch in Marketplace", compatible.identifier.id),
                ExtensionManagementErrorCode::Invalid,
            ));
        }
        Ok((compatible, ExtensionManifest::from_value(manifest)))
    }

    fn get_compatible_version(
        &self,
        extension: &GalleryExtension,
        same_version: bool,
        include_pre_release: bool,
        product_version: &ProductVersion,
    ) -> Result<Option<GalleryExtension>, crate::management::ExtensionGalleryError> {
        let options = || crate::management::ExtensionQueryOptions {
            target_platform: Some(self.target_platform),
            compatible: true,
            product_version: Some(product_version.clone()),
            ..Default::default()
        };
        let info = |pre_release: Option<bool>, version: Option<String>| ExtensionInfo {
            id: extension.identifier.id.clone(),
            uuid: extension.identifier.uuid.clone(),
            pre_release,
            version,
            ..Default::default()
        };
        let mut compatible: Option<GalleryExtension> = None;
        if !same_version && extension.has_pre_release_version && extension.properties.is_pre_release_version != include_pre_release {
            compatible = self.gallery.get_extensions(&[info(Some(include_pre_release), None)], &options())?.into_iter().next();
        }
        if compatible.is_none() && self.gallery.is_extension_compatible(extension, include_pre_release, self.target_platform, Some(product_version)) {
            compatible = Some(extension.clone());
        }
        if compatible.is_none() {
            compatible = if same_version {
                self.gallery.get_extensions(&[info(None, Some(extension.version.clone()))], &options())?.into_iter().next()
            } else {
                self.gallery.get_compatible_extension(extension, include_pre_release, self.target_platform, Some(product_version))?
            };
        }
        Ok(compatible)
    }

    // ---- instalação ----

    /// `install(vsix, options)`: instala um VSIX. `vsix_path` é o caminho (da URI) do arquivo.
    pub fn install(&self, vsix_path: &str, options: &InstallOptions) -> ManagementResult<LocalExtension> {
        self.logger.trace(&format!("ExtensionManagementService#install {vsix_path}"));
        let manifest_value = self.get_manifest(vsix_path)?;
        let manifest = ExtensionManifest::from_value(manifest_value);
        let extension_id = get_gallery_extension_id(Some(&manifest.publisher), &manifest.name);
        if let Some(engine) = manifest.engines.vscode.as_deref().filter(|e| !e.is_empty())
            && !is_engine_valid(engine, &self.product.version, self.product.date.as_deref())
        {
            return Err(error(
                format!("Unable to install extension '{extension_id}' as it is not compatible with VS Code '{}'.", self.product.version),
                ExtensionManagementErrorCode::Unknown,
            ));
        }
        let candidate = AllowedCandidate::Fields { id: &extension_id, publisher_display_name: None, version: Some(&manifest.version), prerelease: None, target_platform: None };
        if let Err(reason) = self.allowed.is_allowed(&candidate) {
            return Err(error(format!("This extension cannot be installed because {reason}"), ExtensionManagementErrorCode::NotAllowed));
        }
        let result = self.install_extensions_batch(vec![(manifest, InstallSourceKind::Vsix(vsix_path.to_owned()), options.clone())]);
        let result = result.into_iter().find(|r| are_same_extensions(&r.identifier, &ExtensionIdentifierInfo::new(extension_id.clone())));
        match result {
            Some(InstallExtensionResult { local: Some(local), .. }) => Ok(local),
            Some(InstallExtensionResult { error: Some(e), .. }) => Err(e),
            _ => Err(error(format!("Unknown error while installing extension {extension_id}"), ExtensionManagementErrorCode::Unknown)),
        }
    }

    /// `installFromGallery`.
    pub fn install_from_gallery(&self, extension: &GalleryExtension, options: &InstallOptions) -> ManagementResult<LocalExtension> {
        let results = self.install_gallery_extensions(&[(extension.clone(), options.clone())])?;
        if let Some(result) = results.iter().find(|r| are_same_extensions(&r.identifier, &extension.identifier)) {
            if let Some(local) = &result.local {
                return Ok(local.clone());
            }
            if let Some(e) = &result.error {
                return Err(e.clone());
            }
        }
        // O resultado da extensão redirecionada (substituta de uma extensão descontinuada).
        match results.into_iter().next() {
            Some(InstallExtensionResult { local: Some(local), .. }) => Ok(local),
            Some(InstallExtensionResult { error: Some(e), .. }) => Err(e),
            _ => Err(error(format!("Unknown error while installing extension {}", extension.identifier.id), ExtensionManagementErrorCode::Unknown)),
        }
    }

    /// `installGalleryExtensions`.
    pub fn install_gallery_extensions(&self, extensions: &[(GalleryExtension, InstallOptions)]) -> ManagementResult<Vec<InstallExtensionResult>> {
        if !self.gallery.is_enabled() {
            return Err(error("Marketplace is not enabled", ExtensionManagementErrorCode::NotAllowed));
        }
        let mut results = Vec::new();
        let mut installable = Vec::new();
        for (extension, options) in extensions {
            let product_version = options.product_version.clone().unwrap_or_else(|| self.product_version());
            match self.check_and_get_compatible_version(extension, options.install_given_version, options.install_pre_release_version, &product_version) {
                Ok((compatible, manifest)) => installable.push((manifest, InstallSourceKind::Gallery(Box::new(compatible)), options.clone())),
                Err(e) => results.push(InstallExtensionResult {
                    identifier: extension.identifier.clone(),
                    operation: InstallOperation::Install,
                    local: None,
                    error: Some(e),
                    profile_location: options.profile_location.clone().unwrap_or_else(|| self.current_profile_location.clone()),
                }),
            }
        }
        if !installable.is_empty() {
            results.extend(self.install_extensions_batch(installable));
        }
        Ok(results)
    }

    /// `installExtensions` do original, reduzido: instala em ordem, cada uma com as dependências e os packs do marketplace.
    fn install_extensions_batch(&self, extensions: Vec<(ExtensionManifest, InstallSourceKind, InstallOptions)>) -> Vec<InstallExtensionResult> {
        let mut results = Vec::new();
        for (manifest, source, options) in extensions {
            let profile_location = options.profile_location.clone().unwrap_or_else(|| self.current_profile_location.clone());
            let identifier = match &source {
                InstallSourceKind::Gallery(extension) => extension.identifier.clone(),
                InstallSourceKind::Vsix(_) => ExtensionIdentifierInfo::new(get_gallery_extension_id(Some(&manifest.publisher), &manifest.name)),
            };
            let outcome = self.install_with_dependencies(&manifest, &source, &options, &profile_location);
            let operation = match &outcome {
                Ok((_, operation)) => *operation,
                Err(_) => InstallOperation::Install,
            };
            let (local, error) = match outcome {
                Ok((local, _)) => (Some(local), None),
                Err(e) => (None, Some(e)),
            };
            if let Some(error) = &error {
                self.logger.error(&format!("Failed to install extension {} {}", identifier.id, error.message));
            }
            results.push(InstallExtensionResult { identifier, operation, local, error, profile_location });
        }
        self.on_did_install_extensions.fire(results.clone());
        results
    }

    fn install_with_dependencies(
        &self,
        manifest: &ExtensionManifest,
        source: &InstallSourceKind,
        options: &InstallOptions,
        profile_location: &str,
    ) -> ManagementResult<(LocalExtension, InstallOperation)> {
        if !options.donot_include_pack_and_dependencies && self.gallery.is_enabled() {
            self.install_pack_and_dependencies(manifest, options, profile_location)?;
        }
        let key = match source {
            InstallSourceKind::Gallery(extension) => ExtensionKey::new(extension.identifier.clone(), extension.version.clone(), extension.properties.target_platform),
            InstallSourceKind::Vsix(_) => ExtensionKey::new(
                ExtensionIdentifierInfo::new(get_gallery_extension_id(Some(&manifest.publisher), &manifest.name)),
                manifest.version.clone(),
                TargetPlatform::Undefined,
            ),
        };
        let source_ref = match source {
            InstallSourceKind::Vsix(path) => InstallSourceRef::Vsix(path),
            InstallSourceKind::Gallery(extension) => InstallSourceRef::Gallery(extension),
        };
        self.install_in_profile(&key, manifest, &source_ref, options, profile_location)
    }

    /// As extensões em `extensionDependencies` e `extensionPack` que ainda não estão no perfil, instaladas do marketplace.
    fn install_pack_and_dependencies(&self, manifest: &ExtensionManifest, options: &InstallOptions, profile_location: &str) -> ManagementResult<()> {
        let wanted: Vec<String> = manifest.extension_dependencies.iter().flatten().chain(manifest.extension_pack.iter().flatten()).cloned().collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let installed = self.get_installed(None, Some(profile_location), None, None)?;
        let missing: Vec<ExtensionInfo> = wanted
            .iter()
            .filter(|id| !installed.iter().any(|e| are_same_extensions(&e.identifier, &ExtensionIdentifierInfo::new((*id).clone()))))
            .map(|id| ExtensionInfo { id: id.clone(), pre_release: Some(options.install_pre_release_version), ..Default::default() })
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let found = self
            .gallery
            .get_extensions(&missing, &crate::management::ExtensionQueryOptions { target_platform: Some(self.target_platform), compatible: true, ..Default::default() })
            .map_err(|e| error(e.message, ExtensionManagementErrorCode::Gallery))?;
        for extension in found {
            let dependency_options = InstallOptions { donot_include_pack_and_dependencies: false, operation: None, ..options.clone() };
            self.install_from_gallery(&extension, &dependency_options)?;
        }
        Ok(())
    }

    /// `InstallExtensionInProfileTask#doRun`.
    fn install_in_profile(
        &self,
        key: &ExtensionKey,
        manifest: &ExtensionManifest,
        source: &InstallSourceRef<'_>,
        options: &InstallOptions,
        profile_location: &str,
    ) -> ManagementResult<(LocalExtension, InstallOperation)> {
        let product_version = options.product_version.clone();
        let installed = self.get_installed(Some(ExtensionType::User), Some(profile_location), product_version.clone(), None)?;
        let existing = installed.iter().find(|i| are_same_extensions(&i.identifier, &key.identifier)).cloned();
        let operation = if existing.is_some() { InstallOperation::Update } else { InstallOperation::Install };
        let system = self.get_installed(Some(ExtensionType::System), Some(profile_location), product_version, None)?;
        if let Some(existing_system) = system.iter().find(|i| are_same_extensions(&i.identifier, &key.identifier)) {
            if !existing_system.force_auto_update {
                return Err(error(
                    format!(
                        "Extension '{}' is a built-in extension and not allowed to be updated in the current product quality '{}'.",
                        existing_system.identifier.id,
                        self.product.quality.as_deref().unwrap_or("undefined")
                    ),
                    ExtensionManagementErrorCode::Unsupported,
                ));
            }
            if je_base::semver::gt(&existing_system.manifest.version, &manifest.version) {
                return Err(error(
                    format!(
                        "Extension '{}' is a built-in extension with version '{}' and cannot be downgraded to version '{}'.",
                        existing_system.identifier.id, existing_system.manifest.version, manifest.version
                    ),
                    ExtensionManagementErrorCode::Unsupported,
                ));
            }
        }

        let mut metadata = Metadata {
            is_application_scoped: options.is_application_scoped.or_else(|| existing.as_ref().map(|e| e.is_application_scoped)).filter(|v| *v),
            is_machine_scoped: options.is_machine_scoped.or_else(|| existing.as_ref().map(|e| e.is_machine_scoped)).filter(|v| *v),
            is_builtin: options.is_builtin.or_else(|| existing.as_ref().map(|e| e.is_builtin)).filter(|v| *v),
            is_system: existing.as_ref().is_some_and(|e| e.extension_type == ExtensionType::System).then_some(true),
            installed_timestamp: Some(now_ms()),
            pinned: if options.install_given_version { Some(true) } else { options.pinned.or_else(|| existing.as_ref().map(|e| e.pinned)) },
            source: Some(match source {
                InstallSourceRef::Vsix(_) => InstallSource::Vsix,
                InstallSourceRef::Gallery(_) => InstallSource::Gallery,
            }),
            ..Metadata::default()
        };

        let mut local: Option<LocalExtension> = None;
        match source {
            InstallSourceRef::Vsix(_) => {
                if let Some(existing) = &existing
                    && key.equals(&ExtensionKey::new(existing.identifier.clone(), existing.manifest.version.clone(), TargetPlatform::Undefined))
                    && self.delete_extension_from_disk(existing, "existing").is_err()
                {
                    return Err(error(
                        format!("Please restart VS Code before reinstalling {}.", manifest.display_name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&manifest.name)),
                        ExtensionManagementErrorCode::Unknown,
                    ));
                }
                // Remove the extension with same version if it is already uninstalled.
                // Installing a VSIX extension shall replace the existing extension always.
                if let Some(same_version) = self.unset_if_removed(key)?
                    && self.delete_extension_from_disk(&same_version, "existing").is_err()
                {
                    return Err(error(
                        format!("Please restart VS Code before reinstalling {}.", manifest.display_name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&manifest.name)),
                        ExtensionManagementErrorCode::Unknown,
                    ));
                }
            }
            InstallSourceRef::Gallery(gallery) => {
                metadata.id = gallery.identifier.uuid.clone();
                metadata.publisher_id = Some(gallery.publisher_id.clone());
                metadata.publisher_display_name = Some(gallery.publisher_display_name.clone());
                metadata.target_platform = Some(gallery.properties.target_platform);
                metadata.updated = Some(existing.is_some());
                metadata.private = Some(gallery.private);
                metadata.is_pre_release_version = Some(gallery.properties.is_pre_release_version);
                metadata.has_pre_release_version = Some(existing.as_ref().is_some_and(|e| e.has_pre_release_version) || gallery.properties.is_pre_release_version);
                metadata.pre_release = options.pre_release.or(Some(options.install_pre_release_version || gallery.properties.is_pre_release_version || existing.as_ref().is_some_and(|e| e.pre_release)));
                if let Some(existing) = &existing
                    && existing.extension_type != ExtensionType::System
                    && existing.manifest.version == gallery.version
                {
                    let updated = self.update_metadata_in_profile(existing, &metadata, profile_location)?;
                    return Ok((updated, operation));
                }
                // Unset if the extension is uninstalled and return the unset extension.
                local = self.unset_if_removed(key)?;
            }
        }

        let local = match local {
            Some(local) => local,
            None => self.extract_extension(key, manifest, source, options)?,
        };

        if paths::is_equal(&self.profiles.default_profile.extensions_resource, profile_location) {
            self.scanner.initialize_default_profile_extensions().map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::IntializeDefaultProfile))?;
        }
        let profile_source = ProfileExtensionSource { identifier: local.identifier.clone(), version: local.manifest.version.clone(), location: local.location.clone() };
        // Para uma extensão de metadados vazios, o original grava o objeto mesmo assim.
        self.profile_scanner
            .add_extensions_to_profile(&[(profile_source, Some(metadata))], profile_location, !local.is_valid)
            .map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::AddToProfile))?;
        let result = self.scan_local_extension(&local.location, ExtensionType::User, Some(profile_location))?;
        if matches!(source, InstallSourceRef::Vsix(_)) {
            self.update_gallery_metadata(&result, options, profile_location);
        }
        if let InstallSourceRef::Gallery(gallery) = source {
            self.gallery.report_statistic(&gallery.publisher, &gallery.name, &gallery.version, StatisticType::Install);
        }
        Ok((result, operation))
    }

    /// `updateMetadata` (privado da tarefa): completa os metadados de uma extensão de VSIX com os do marketplace, se
    /// ela existir lá. Falhas são ignoradas.
    fn update_gallery_metadata(&self, extension: &LocalExtension, options: &InstallOptions, profile_location: &str) {
        let attempt = || -> Option<()> {
            let id = extension.identifier.id.clone();
            let mut found = self.gallery.get_extensions(&[ExtensionInfo { id: id.clone(), version: Some(extension.manifest.version.clone()), ..Default::default() }], &Default::default()).ok()?;
            if found.is_empty() {
                found = self.gallery.get_extensions(&[ExtensionInfo { id, ..Default::default() }], &Default::default()).ok()?;
            }
            let gallery = found.first()?;
            let metadata = Metadata {
                id: gallery.identifier.uuid.clone(),
                publisher_display_name: Some(gallery.publisher_display_name.clone()),
                publisher_id: Some(gallery.publisher_id.clone()),
                is_pre_release_version: Some(gallery.properties.is_pre_release_version),
                has_pre_release_version: Some(extension.has_pre_release_version || gallery.properties.is_pre_release_version),
                pre_release: Some(gallery.properties.is_pre_release_version || options.install_pre_release_version),
                ..Metadata::default()
            };
            self.update_metadata_in_profile(extension, &metadata, profile_location).ok()?;
            Some(())
        };
        let _ = attempt();
    }

    /// `extractUserExtension`: extrai o VSIX numa pasta temporária, grava o `__metadata` e renomeia para a pasta final.
    fn extract_extension(&self, key: &ExtensionKey, manifest: &ExtensionManifest, source: &InstallSourceRef<'_>, options: &InstallOptions) -> ManagementResult<LocalExtension> {
        let _ = options;
        let (zip_path, downloaded) = match source {
            InstallSourceRef::Vsix(path) => ((*path).to_owned(), false),
            InstallSourceRef::Gallery(gallery) => {
                let location = paths::join_path(&self.downloads_dir, &[&generate_uuid()]);
                self.gallery
                    .download(gallery, &location, InstallOperation::Install, self.fs.as_ref())
                    .map_err(|e| error(e.message, if e.code == crate::management::ExtensionGalleryErrorCode::DownloadFailedWriting { ExtensionManagementErrorCode::DownloadFailedWriting } else { ExtensionManagementErrorCode::Download }))?;
                (location, true)
            }
        };
        let result = self.extract_user_extension(key, &zip_path, matches!(source, InstallSourceRef::Vsix(_)));
        if downloaded && let Err(e) = self.fs.remove(&zip_path, false) {
            self.logger.warn(&format!("Error while deleting the downloaded file {zip_path} {e}"));
        }
        let _ = manifest;
        result
    }

    fn extract_user_extension(&self, key: &ExtensionKey, zip_path: &str, remove_if_exists: bool) -> ManagementResult<LocalExtension> {
        let user_location = self.scanner.user_extensions_location().to_owned();
        let folder_name = key.to_string();
        let temp_location = paths::join_path(&user_location, &[&format!(".{}", generate_uuid())]);
        let extension_location = paths::join_path(&user_location, &[&folder_name]);

        if self.fs.exists(&extension_location) {
            if !remove_if_exists {
                match self.scan_local_extension(&extension_location, ExtensionType::User, None) {
                    Ok(existing) => return Ok(existing),
                    Err(e) => self.logger.warn(&format!("Error while scanning the existing extension at {extension_location}. Deleting the existing extension and extracting it. {e}")),
                }
            }
            if let Err(e) = self.delete_folder(key.id(), &extension_location, "removeExisting") {
                return Err(error(
                    format!("Unable to delete the existing folder '{extension_location}' while installing the extension '{}'. Please delete the folder manually and try again", key.id()),
                    e.code,
                ));
            }
        }

        let outcome = (|| -> ManagementResult<()> {
            // Extract
            let data = self.fs.read(zip_path).map_err(|e| io_error(&e, ExtensionManagementErrorCode::Extract))?;
            self.logger.trace(&format!("Started extracting the extension from {zip_path} to {extension_location}"));
            vsix::extract(Cursor::new(data), self.fs.as_ref(), &temp_location)?;
            self.logger.info(&format!("Extracted extension to {extension_location}: {}", key.id()));

            let mut metadata = ManifestMetadata { installed_timestamp: Some(now_ms()), target_platform: Some(key.target_platform), size: None };
            match compute_size(self.fs.as_ref(), &temp_location) {
                Ok(size) => metadata.size = Some(size as f64),
                Err(e) => self.logger.warn(&format!("Error while getting the size of the extracted extension : {temp_location} {e}")),
            }
            self.scanner.update_manifest_metadata(&temp_location, &metadata).map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::UpdateMetadata))?;

            // Rename
            match self.fs.rename(&temp_location, &extension_location) {
                Ok(()) => self.logger.info(&format!("Renamed to {extension_location}")),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::AlreadyExists) => {
                    self.logger.info(&format!("Rename failed because extension was installed by another source. So ignoring renaming. {}", key.id()));
                    let _ = self.fs.remove(&temp_location, true);
                }
                Err(e) => {
                    self.logger.info(&format!("Rename failed because of {e}. Deleted from extracted location {temp_location}"));
                    return Err(io_error(&e, ExtensionManagementErrorCode::Rename));
                }
            }
            Ok(())
        })();
        if let Err(e) = outcome {
            let _ = self.fs.remove(&temp_location, true);
            return Err(e);
        }
        self.scan_local_extension(&extension_location, ExtensionType::User, None)
    }

    // ---- metadados ----

    /// `updateMetadata(local, metadata, profileLocation)`.
    pub fn update_metadata(&self, local: &LocalExtension, metadata: &Metadata, profile_location: Option<&str>) -> ManagementResult<LocalExtension> {
        self.logger.trace(&format!("ExtensionManagementService#updateMetadata {}", local.identifier.id));
        let mut metadata = metadata.clone();
        if metadata.is_pre_release_version == Some(true) {
            metadata.pre_release = Some(true);
            metadata.has_pre_release_version = Some(true);
        }
        // unset if false
        if metadata.is_machine_scoped == Some(false) {
            metadata.is_machine_scoped = None;
        }
        if metadata.is_builtin == Some(false) {
            metadata.is_builtin = None;
        }
        if metadata.pinned == Some(false) {
            metadata.pinned = None;
        }
        let profile_location = self.profile_or_current(profile_location).to_owned();
        let updated = self.update_metadata_in_profile(local, &metadata, &profile_location)?;
        self.on_did_update_extension_metadata.fire((updated.clone(), profile_location));
        Ok(updated)
    }

    fn update_metadata_in_profile(&self, local: &LocalExtension, metadata: &Metadata, profile_location: &str) -> ManagementResult<LocalExtension> {
        let source = ProfileExtensionSource::from(local);
        self.profile_scanner.update_metadata(&[(source, metadata.clone())], profile_location).map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::UpdateMetadata))?;
        self.scan_local_extension(&local.location, local.extension_type, Some(profile_location))
    }

    // ---- desinstalação ----

    /// `uninstall`: tira a extensão (e o pack, se houver) do perfil; as dependentes impedem, salvo `donotCheckDependents`.
    /// Com `remove`, ou quando a extensão sai de todos os perfis (ver o [`crate::watcher`]), a pasta é marcada para remoção.
    pub fn uninstall(&self, extension: &LocalExtension, options: &UninstallOptions) -> ManagementResult<()> {
        let profile_location = if extension.is_application_scoped {
            self.profiles.default_profile.extensions_resource.clone()
        } else {
            options.profile_location.clone().unwrap_or_else(|| self.current_profile_location.clone())
        };
        let installed = self.get_installed(Some(ExtensionType::User), Some(&profile_location), None, None)?;
        let mut to_uninstall: Vec<LocalExtension> = vec![extension.clone()];
        if !options.donot_include_pack {
            for packed in self.get_all_pack_extensions_to_uninstall(extension, &installed, &mut Vec::new()) {
                if !to_uninstall.iter().any(|e| are_same_extensions(&e.identifier, &packed.identifier)) {
                    to_uninstall.push(packed);
                }
            }
        }
        if !options.donot_check_dependents {
            for candidate in &to_uninstall {
                let dependents = Self::get_dependents(candidate, &installed);
                let remaining: Vec<&LocalExtension> = dependents.iter().filter(|d| !to_uninstall.iter().any(|e| are_same_extensions(&e.identifier, &d.identifier))).collect();
                if !remaining.is_empty() {
                    return Err(error(Self::dependents_error_message(candidate, &remaining, extension), ExtensionManagementErrorCode::Unknown));
                }
            }
        }
        // As extensões de escopo de aplicação e as de `remove` saem também dos outros perfis em que estão.
        let mut targets: Vec<(LocalExtension, String)> = to_uninstall.iter().map(|e| (e.clone(), profile_location.clone())).collect();
        if options.remove || extension.is_application_scoped {
            for profile in &self.profiles.profiles {
                if paths::is_equal(&profile.extensions_resource, &profile_location) {
                    continue;
                }
                let in_profile = self.get_installed(Some(ExtensionType::User), Some(&profile.extensions_resource), None, None)?;
                if let Some(found) = in_profile.into_iter().find(|e| are_same_extensions(&e.identifier, &extension.identifier)) {
                    targets.push((found, profile.extensions_resource.clone()));
                }
            }
        }
        let mut outcome: ManagementResult<()> = Ok(());
        for (target, location) in &targets {
            self.logger.info(&format!("Uninstalling extension from the profile: {}@{} {location}", target.identifier.id, target.manifest.version));
            let removed = self.profile_scanner.remove_extensions_from_profile(std::slice::from_ref(&target.identifier), location);
            match removed {
                Ok(()) => {
                    lifecycle::post_uninstall(target, &self.global_storage_home, self.fs.as_ref(), self.logger.as_ref());
                    if target.identifier.uuid.is_some() {
                        let manifest = &target.manifest;
                        self.gallery.report_statistic(&manifest.publisher, &manifest.name, &manifest.version, StatisticType::Uninstall);
                    }
                    self.on_did_uninstall_extension.fire(DidUninstallExtensionEvent {
                        identifier: target.identifier.clone(),
                        error: None,
                        profile_location: location.clone(),
                        application_scoped: target.is_application_scoped,
                    });
                }
                Err(e) => {
                    let e = error(e.to_string(), ExtensionManagementErrorCode::Unknown);
                    self.logger.error(&format!("Failed to uninstall extension from the profile: {}@{} {location} {}", target.identifier.id, target.manifest.version, e.message));
                    self.on_did_uninstall_extension.fire(DidUninstallExtensionEvent {
                        identifier: target.identifier.clone(),
                        error: Some(e.code),
                        profile_location: location.clone(),
                        application_scoped: target.is_application_scoped,
                    });
                    outcome = Err(e);
                    break;
                }
            }
        }
        outcome?;
        if options.remove {
            let mut seen: Vec<&LocalExtension> = Vec::new();
            for (target, _) in &targets {
                if !seen.iter().any(|e| e.location == target.location) {
                    seen.push(target);
                }
            }
            for target in seen {
                self.delete_extensions(&[target])?;
            }
        }
        Ok(())
    }

    fn get_all_pack_extensions_to_uninstall(&self, extension: &LocalExtension, installed: &[LocalExtension], checked: &mut Vec<String>) -> Vec<LocalExtension> {
        if checked.contains(&extension.location) {
            return Vec::new();
        }
        checked.push(extension.location.clone());
        let pack = extension.manifest.extension_pack.clone().unwrap_or_default();
        if pack.is_empty() {
            return Vec::new();
        }
        let packed: Vec<LocalExtension> =
            installed.iter().filter(|i| !i.is_builtin && pack.iter().any(|id| are_same_extensions(&ExtensionIdentifierInfo::new(id.clone()), &i.identifier))).cloned().collect();
        let mut nested = Vec::new();
        for packed_extension in &packed {
            nested.extend(self.get_all_pack_extensions_to_uninstall(packed_extension, installed, checked));
        }
        packed.into_iter().chain(nested).collect()
    }

    fn get_dependents(extension: &LocalExtension, installed: &[LocalExtension]) -> Vec<LocalExtension> {
        installed
            .iter()
            .filter(|e| e.manifest.extension_dependencies.as_ref().is_some_and(|deps| deps.iter().any(|id| are_same_extensions(&ExtensionIdentifierInfo::new(id.clone()), &extension.identifier))))
            .cloned()
            .collect()
    }

    fn dependents_error_message(depending: &LocalExtension, dependents: &[&LocalExtension], to_uninstall: &LocalExtension) -> String {
        let name = |e: &LocalExtension| e.manifest.display_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| e.manifest.name.clone());
        let direct = depending.location == to_uninstall.location;
        match (direct, dependents.len()) {
            (true, 1) => format!("Cannot uninstall '{}' extension. '{}' extension depends on this.", name(to_uninstall), name(dependents[0])),
            (true, 2) => format!("Cannot uninstall '{}' extension. '{}' and '{}' extensions depend on this.", name(to_uninstall), name(dependents[0]), name(dependents[1])),
            (true, _) => format!("Cannot uninstall '{}' extension. '{}', '{}' and other extension depend on this.", name(to_uninstall), name(dependents[0]), name(dependents[1])),
            (false, 1) => format!(
                "Cannot uninstall '{}' extension . It includes uninstalling '{}' extension and '{}' extension depends on this.",
                name(to_uninstall),
                name(depending),
                name(dependents[0])
            ),
            (false, 2) => format!(
                "Cannot uninstall '{}' extension. It includes uninstalling '{}' extension and '{}' and '{}' extensions depend on this.",
                name(to_uninstall),
                name(depending),
                name(dependents[0]),
                name(dependents[1])
            ),
            (false, _) => format!(
                "Cannot uninstall '{}' extension. It includes uninstalling '{}' extension and '{}', '{}' and other extensions depend on this.",
                name(to_uninstall),
                name(depending),
                name(dependents[0]),
                name(dependents[1])
            ),
        }
    }

    // ---- `.obsolete` e remoção ----

    /// `withRemovedExtensions`: lê o `.obsolete`, deixa o `update` mexer e grava de volta (ou apaga se ficou vazio).
    fn with_removed_extensions(&self, update: Option<&mut dyn FnMut(&mut RemovedExtensions)>) -> ManagementResult<RemovedExtensions> {
        let raw = match self.fs.read_to_string(&self.obsolete_resource) {
            Ok(raw) => Some(raw),
            Err(e) if is_not_found(&e) => None,
            Err(e) => return Err(io_error(&e, ExtensionManagementErrorCode::ReadRemoved)),
        };
        let mut removed: RemovedExtensions = raw
            .filter(|r| !r.is_empty())
            .and_then(|r| serde_json::from_str::<Value>(crate::json_util::strip_bom(&r)).ok())
            .and_then(|v| v.as_object().cloned())
            .map(|o| o.into_iter().filter_map(|(k, v)| Some((k, v.as_bool()?))).collect())
            .unwrap_or_default();
        if let Some(update) = update {
            update(&mut removed);
            if removed.is_empty() {
                if let Err(e) = self.fs.remove(&self.obsolete_resource, false)
                    && !is_not_found(&e)
                {
                    return Err(io_error(&e, ExtensionManagementErrorCode::UnsetRemoved));
                }
            } else {
                let object: serde_json::Map<String, Value> = removed.iter().map(|(k, v)| (k.clone(), Value::Bool(*v))).collect();
                self.fs.write(&self.obsolete_resource, Value::Object(object).to_string().as_bytes()).map_err(|e| io_error(&e, ExtensionManagementErrorCode::UnsetRemoved))?;
            }
        }
        Ok(removed)
    }

    /// `deleteExtensions` / `setExtensionsForRemoval`: marca as pastas (que existem) como removidas, no `.obsolete`.
    pub fn delete_extensions(&self, extensions: &[&LocalExtension]) -> ManagementResult<()> {
        let keys: Vec<ExtensionKey> = extensions
            .iter()
            .filter(|e| self.fs.exists(&e.location))
            .map(|e| ExtensionKey::new(e.identifier.clone(), e.manifest.version.clone(), e.target_platform))
            .collect();
        self.with_removed_extensions(Some(&mut |removed| {
            for key in &keys {
                removed.insert(key.to_string(), true);
                self.logger.info(&format!("Marked extension as removed {key}"));
            }
        }))?;
        Ok(())
    }

    /// `unsetExtensionsForRemoval`: tira as chaves do `.obsolete`; `true` para cada uma que estava marcada.
    pub fn unset_extensions_for_removal(&self, keys: &[ExtensionKey]) -> ManagementResult<Vec<bool>> {
        let mut results = Vec::new();
        self.with_removed_extensions(Some(&mut |removed| {
            for key in keys {
                results.push(removed.shift_remove(&key.to_string()).is_some_and(|v| v));
            }
        }))?;
        Ok(results)
    }

    fn unset_if_removed(&self, key: &ExtensionKey) -> ManagementResult<Option<LocalExtension>> {
        // If the same version of extension is marked as removed, remove it from there and return the local.
        let removed = self.unset_extensions_for_removal(std::slice::from_ref(key))?;
        if removed.first() == Some(&true) {
            self.logger.info(&format!("Removed the extension from removed list: {}", key.id()));
            let user_extensions = self.scan_all_user_installed_extensions()?;
            return Ok(user_extensions.into_iter().find(|i| ExtensionKey::new(i.identifier.clone(), i.manifest.version.clone(), i.target_platform).equals(key)));
        }
        Ok(None)
    }

    /// `deleteExtension`: só apaga extensões dentro da pasta de extensões do usuário.
    fn delete_extension_from_disk(&self, extension: &LocalExtension, kind: &str) -> ManagementResult<()> {
        if paths::is_equal_or_parent(&extension.location, self.scanner.user_extensions_location()) {
            self.delete_folder(&extension.identifier.id, &extension.location, kind)?;
            self.unset_extensions_for_removal(&[ExtensionKey::new(extension.identifier.clone(), extension.manifest.version.clone(), extension.target_platform)])?;
        }
        Ok(())
    }

    /// `deleteExtensionFromLocation`: renomeia para `<pasta>.<hash>.vsctmp` e apaga, para a pasta sumir de uma vez.
    fn delete_folder(&self, id: &str, location: &str, kind: &str) -> ManagementResult<()> {
        self.logger.trace(&format!("Deleting {kind} extension from disk {id} {location}"));
        let renamed = paths::join_path(&paths::dirname(location), &[&format!("{}.{}{DELETED_FOLDER_POSTFIX}", paths::basename(location), &generate_uuid()[..8])]);
        self.fs.rename(location, &renamed).map_err(|e| io_error(&e, ExtensionManagementErrorCode::Rename))?;
        self.fs.remove(&renamed, true).map_err(|e| io_error(&e, ExtensionManagementErrorCode::Delete))?;
        self.logger.info(&format!("Deleted {kind} extension from disk {id} {location}"));
        Ok(())
    }

    /// `cleanUp`: apaga as pastas `.vsctmp`, as extensões embutidas desatualizadas instaladas pelo usuário e as marcadas
    /// para remoção. Falhas são registradas e ignoradas, como no original.
    pub fn clean_up(&self) {
        self.logger.trace("ExtensionManagementService#cleanUp");
        let outcome = (|| -> ManagementResult<()> {
            self.remove_temporarily_deleted_folders();
            self.remove_stale_auto_update_builtin_extensions()?;
            self.delete_extensions_marked_for_removal()?;
            self.initialize_extension_size()?;
            Ok(())
        })();
        if let Err(e) = outcome {
            self.logger.error(&e.message);
        }
    }

    fn remove_temporarily_deleted_folders(&self) {
        let location = self.scanner.user_extensions_location();
        let children = match self.fs.read_dir(location) {
            Ok(children) => children,
            Err(e) => {
                if !is_not_found(&e) {
                    self.logger.error(&e.to_string());
                }
                return;
            }
        };
        for child in children {
            if !child.is_directory || !child.name.ends_with(DELETED_FOLDER_POSTFIX) {
                continue;
            }
            let path = paths::join_path(location, &[&child.name]);
            self.logger.trace(&format!("Deleting the temporarily deleted folder {path}"));
            if let Err(e) = self.fs.remove(&path, true)
                && !is_not_found(&e)
            {
                self.logger.error(&e.to_string());
            }
        }
    }

    fn remove_stale_auto_update_builtin_extensions(&self) -> ManagementResult<()> {
        let scan_error = |e: crate::scanner::ScanError| error(e.to_string(), ExtensionManagementErrorCode::Scanning);
        let builtin = self.scanner.scan_system_extensions(&SystemExtensionsScanOptions::default()).map_err(scan_error)?;
        let user = self.scanner.scan_all_user_extensions().map_err(scan_error)?;
        let stale: Vec<&ScannedExtension> = user
            .iter()
            .filter(|user_extension| {
                if !self.product.built_in_extensions_enabled_with_auto_updates.iter().any(|id| id.to_lowercase() == user_extension.identifier.id.to_lowercase()) {
                    return false;
                }
                builtin.iter().find(|e| are_same_extensions(&e.identifier, &user_extension.identifier)).is_some_and(|b| je_base::semver::lt(&user_extension.manifest.version, &b.manifest.version))
            })
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        self.logger.info(&format!("Removing stale auto-update builtin extensions: {}", stale.iter().map(|e| format!("{}@{}", e.identifier.id, e.manifest.version)).collect::<Vec<_>>().join(", ")));
        let identifiers: Vec<ExtensionIdentifierInfo> = stale.iter().map(|e| e.identifier.clone()).collect();
        self.profile_scanner.remove_extensions_from_profile(&identifiers, &self.profiles.default_profile.extensions_resource).map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::Unknown))?;
        for extension in stale {
            let local = self.to_local_extension(extension);
            if let Err(e) = self.delete_extension_from_disk(&local, "stale auto-update builtin") {
                self.logger.error(&e.message);
            }
        }
        Ok(())
    }

    fn delete_extensions_marked_for_removal(&self) -> ManagementResult<()> {
        let removed = self.with_removed_extensions(None)?;
        if removed.is_empty() {
            self.logger.debug("No extensions are marked as removed.");
            return Ok(());
        }
        self.logger.debug(&format!("Deleting extensions marked as removed: {}", removed.keys().cloned().collect::<Vec<_>>().join(", ")));
        let extensions = self.scan_all_user_installed_extensions()?;
        let key_of = |e: &LocalExtension| ExtensionKey::new(e.identifier.clone(), e.manifest.version.clone(), e.target_platform).to_string();
        let installed: std::collections::HashSet<String> =
            extensions.iter().filter(|e| !removed.contains_key(&key_of(e))).map(|e| e.identifier.id.to_lowercase()).collect();
        // running post uninstall tasks for extensions that are not installed anymore
        for mut group in group_by_extension(extensions.clone(), |e| &e.identifier) {
            group.sort_by(|a, b| je_base::semver::rcompare(&a.manifest.version, &b.manifest.version));
            let latest = &group[0];
            if !installed.contains(&latest.identifier.id.to_lowercase()) {
                lifecycle::post_uninstall(latest, &self.global_storage_home, self.fs.as_ref(), self.logger.as_ref());
            }
        }
        for extension in extensions.iter().filter(|e| e.installed_timestamp.is_some() /* Installed by System */ && removed.contains_key(&key_of(e))) {
            if let Err(e) = self.delete_extension_from_disk(extension, "marked for removal") {
                self.logger.error(&e.message);
            }
        }
        Ok(())
    }

    /// `initializeExtensionSize`: calcula o tamanho das extensões instaladas pelo sistema que ainda não o têm.
    fn initialize_extension_size(&self) -> ManagementResult<()> {
        let scan_error = |e: crate::scanner::ScanError| error(e.to_string(), ExtensionManagementErrorCode::Scanning);
        for extension in self.scanner.scan_all_user_extensions().map_err(scan_error)? {
            let has_timestamp = extension.metadata.as_ref().is_some_and(|m| m.installed_timestamp.is_some());
            let has_size = extension.metadata.as_ref().is_some_and(|m| m.size.is_some());
            if has_timestamp && !has_size {
                let size = compute_size(self.fs.as_ref(), &extension.location).map_err(|e| io_error(&e, ExtensionManagementErrorCode::Scanning))?;
                self.scanner
                    .update_manifest_metadata(&extension.location, &ManifestMetadata { size: Some(size as f64), ..Default::default() })
                    .map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::UpdateMetadata))?;
            }
        }
        Ok(())
    }

    /// `copyExtensions`: copia as extensões (sem as de escopo de aplicação) de um perfil para outro.
    pub fn copy_extensions(&self, from_profile_location: &str, to_profile_location: &str) -> ManagementResult<()> {
        let from_extensions = self.get_installed(Some(ExtensionType::User), Some(from_profile_location), None, None)?;
        let scanned = self.profile_scanner.scan_profile_extensions(from_profile_location, None).map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::Scanning))?;
        let mut sources: Vec<(ProfileExtensionSource, Option<Metadata>)> = Vec::new();
        for extension in from_extensions.iter().filter(|e| !e.is_application_scoped) {
            let metadata = scanned.iter().find(|s| are_same_extensions(&s.identifier, &extension.identifier)).and_then(|s| s.metadata.clone());
            sources.push((ProfileExtensionSource::from(extension), metadata));
        }
        self.profile_scanner.add_extensions_to_profile(&sources, to_profile_location, false).map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::AddToProfile))?;
        Ok(())
    }

    /// `installFromLocation`: registra no perfil uma extensão que já está numa pasta.
    pub fn install_from_location(&self, location: &str, profile_location: &str) -> ManagementResult<LocalExtension> {
        let local = self
            .scan_installed_extension_at_location(location)
            .filter(|l| !l.manifest.name.is_empty() && !l.manifest.version.is_empty())
            .ok_or_else(|| error(format!("Cannot find a valid extension from the location {location}"), ExtensionManagementErrorCode::Unknown))?;
        let metadata = Metadata { source: Some(InstallSource::Resource), ..Metadata::default() };
        self.profile_scanner
            .add_extensions_to_profile(&[(ProfileExtensionSource::from(&local), Some(metadata))], profile_location, false)
            .map_err(|e| error(e.to_string(), ExtensionManagementErrorCode::AddToProfile))?;
        Ok(local)
    }
}

/// A fonte de uma instalação com a extensão do marketplace já resolvida.
enum InstallSourceKind {
    Vsix(String),
    Gallery(Box<GalleryExtension>),
}
