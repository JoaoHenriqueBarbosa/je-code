//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts`, classe `Extension`: a extensão
//! como o workbench a mostra, montada de uma extensão local, da extensão da galeria e/ou de uma extensão de recurso.
//!
//! Recortes e notas de tradução:
//! - **Injeção de dependência.** O construtor do original recebe os provedores de estado, o servidor, as extensões e
//!   cinco serviços por decorador (`IExtensionGalleryService`, `ITelemetryService`, `ILogService`, `IFileService`,
//!   `IProductService`). Aqui os serviços vêm agrupados em [`ExtensionServices`]. O `IExtensionGalleryService` é o
//!   [`GalleryApi`] do `je-extensions` (síncrono, sem token de cancelamento: o `token` de `getManifest`, `getReadme` e
//!   `getChangelog` não chega até ele), e o `FileAccess` (um singleton no original) é uma instância compartilhada.
//! - **`getGalleryExtensionTelemetryData`** não carrega o `...extension.telemetryData` do original: a
//!   [`GalleryExtension`] do porte não tem telemetria própria.
//! - **Campos mutáveis** do original (`enablementState`, `local`, `gallery`, `missingFromGallery`, `deprecationInfo`) são
//!   `Cell`/`RefCell`, porque a extensão é compartilhada por referência e o serviço do workbench as atualiza no lugar.
//! - **`galleryResourcesCache`** só guarda o manifesto (a única chave que o original usa), e uma rejeição não fica em
//!   cache (o `catch` do original a remove).
//! - **Versões inválidas** em `outdated`: o `semver.gt` e o `semver.eq` do original lançam e o `try/catch` devolve
//!   `false`; os de `je_base::semver` devolvem `false` direto, com o mesmo resultado.
//! - O resto do arquivo (`Extensions` e `ExtensionsWorkbenchService`) depende do serviço de instalação do workbench, da
//!   galeria assíncrona, de URL, editor, progresso, notificações, sincronização e atualização, que o porte ainda não
//!   tem; entra quando esses serviços existirem.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::network::FileAccess;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::semver;
use je_base::uri::URI;
use je_extensions::gallery::GalleryApi;
use je_extensions::management::{DeprecationInfo, ExtensionsControlManifest, GalleryExtension, LocalExtension, MaliciousExtensionInfo};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use je_extensions::util::find_matching_malicious_entry;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::product::IProductService;
use je_platform::telemetry::telemetry::{ITelemetryData, ITelemetryService};
use je_platform::telemetry::telemetry_utils::TelemetryTrustedValue;

use super::extensions::{ExtensionRuntimeState, ExtensionState, IExtension};
use crate::services::extension_management::extension_management::{EnablementState, IExtensionManagementServer, IResourceExtension};

/// `ShowCurrentReleaseNotesActionId` (`contrib/update/common/update.ts`).
const SHOW_CURRENT_RELEASE_NOTES_ACTION_ID: &str = "update.showCurrentReleaseNotes";

/// `IExtensionStateProvider<T>`: `(extension: Extension) => T`.
pub type ExtensionStateProvider<T> = Rc<dyn Fn(&Extension) -> T>;

/// O `{ resourceExtension: IResourceExtension; isWorkspaceScoped: boolean }` do construtor.
#[derive(Clone, Debug)]
pub struct ResourceExtensionInfo {
    pub resource_extension: IResourceExtension,
    pub is_workspace_scoped: bool,
}

/// Os serviços que o construtor do original recebe por decorador.
pub struct ExtensionServices {
    /// `IExtensionGalleryService`.
    pub gallery_service: Rc<dyn GalleryApi>,
    /// `ITelemetryService`.
    pub telemetry_service: Rc<dyn ITelemetryService>,
    /// `ILogService`.
    pub log_service: LogServiceRef,
    /// `IFileService`.
    pub file_service: Rc<dyn IFileService>,
    /// `IProductService`.
    pub product_service: Rc<dyn IProductService>,
    /// `FileAccess`.
    pub file_access: Rc<FileAccess>,
}

/// `Extension`.
pub struct Extension {
    enablement_state: Cell<EnablementState>,
    gallery_manifest_cache: RefCell<Option<Promise<Option<ExtensionManifest>>>>,
    missing_from_gallery: Cell<bool>,
    malicious: RefCell<Option<MaliciousExtensionInfo>>,
    deprecation_info: RefCell<Option<DeprecationInfo>>,
    state_provider: ExtensionStateProvider<ExtensionState>,
    runtime_state_provider: ExtensionStateProvider<Option<ExtensionRuntimeState>>,
    server: Option<Rc<IExtensionManagementServer>>,
    local: RefCell<Option<Rc<LocalExtension>>>,
    gallery: RefCell<Option<Rc<GalleryExtension>>>,
    resource_extension_info: Option<ResourceExtensionInfo>,
    services: ExtensionServices,
}

/// `'' || undefined` do JavaScript: a cadeia vazia é falsa.
fn non_empty(text: Option<&String>) -> Option<&String> {
    text.filter(|text| !text.is_empty())
}

impl Extension {
    /// `constructor(stateProvider, runtimeStateProvider, server, local, gallery, resourceExtensionInfo, ...services)`.
    pub fn new(
        state_provider: ExtensionStateProvider<ExtensionState>,
        runtime_state_provider: ExtensionStateProvider<Option<ExtensionRuntimeState>>,
        server: Option<Rc<IExtensionManagementServer>>,
        local: Option<Rc<LocalExtension>>,
        gallery: Option<Rc<GalleryExtension>>,
        resource_extension_info: Option<ResourceExtensionInfo>,
        services: ExtensionServices,
    ) -> Self {
        Self {
            enablement_state: Cell::new(EnablementState::EnabledGlobally),
            gallery_manifest_cache: RefCell::new(None),
            missing_from_gallery: Cell::new(false),
            malicious: RefCell::new(None),
            deprecation_info: RefCell::new(None),
            state_provider,
            runtime_state_provider,
            server,
            local: RefCell::new(local),
            gallery: RefCell::new(gallery),
            resource_extension_info,
            services,
        }
    }

    /// `enablementState = value`.
    pub fn set_enablement_state(&self, enablement_state: EnablementState) {
        self.enablement_state.set(enablement_state);
    }

    /// `local = value`.
    pub fn set_local(&self, local: Option<Rc<LocalExtension>>) {
        *self.local.borrow_mut() = local;
    }

    /// `missingFromGallery = missing`.
    pub fn set_missing_from_gallery(&self, missing: bool) {
        self.missing_from_gallery.set(missing);
    }

    /// `deprecationInfo = value`.
    pub fn set_deprecation_info(&self, deprecation_info: Option<DeprecationInfo>) {
        *self.deprecation_info.borrow_mut() = deprecation_info;
    }

    /// `uuid`.
    pub fn uuid(&self) -> Option<String> {
        match self.gallery() {
            Some(gallery) => gallery.identifier.uuid.clone(),
            None => self.local().and_then(|local| local.identifier.uuid.clone()),
        }
    }

    /// `setExtensionsControlManifest(extensionsControlManifest)`.
    pub fn set_extensions_control_manifest(&self, extensions_control_manifest: &ExtensionsControlManifest) {
        let identifier = self.identifier();
        *self.malicious.borrow_mut() = find_matching_malicious_entry(&identifier, &extensions_control_manifest.malicious).cloned();
        *self.deprecation_info.borrow_mut() = extensions_control_manifest.deprecated.get(&identifier.id.to_lowercase()).cloned();
    }

    /// `getGalleryManifest(token)`.
    pub fn get_gallery_manifest(&self, _token: CancellationToken) -> Promise<Option<ExtensionManifest>> {
        let Some(gallery) = self.gallery() else {
            return Promise::resolved(None);
        };
        if let Some(cache) = self.gallery_manifest_cache.borrow().clone() {
            return cache;
        }
        if gallery.assets.manifest.is_none() {
            self.services.log_service.error_args(
                &LogMessage::from(localize("Manifest is not found", "Manifest is not found", &[]).as_str()),
                &[LogArg::Value(Dynamic::string(&self.identifier().id))],
            );
            return Promise::resolved(None);
        }
        match self.services.gallery_service.get_manifest(&gallery) {
            Ok(manifest) => {
                let promise = Promise::resolved(manifest.map(ExtensionManifest::from_value));
                *self.gallery_manifest_cache.borrow_mut() = Some(promise.clone());
                promise
            }
            Err(error) => Promise::rejected(Error::new(error.message)),
        }
    }

    fn platform() -> Platform {
        Platform::detect()
    }

    /// Um caminho de pasta ou arquivo local, como o recurso que o original guarda.
    fn file_uri(path: &str) -> URI {
        URI::file(path, Self::platform())
    }

    /// Aplica `read` ao manifesto da extensão local ou, na falta dela, ao da extensão de recurso (`getManifestFromLocalOrResource`).
    fn with_manifest<R>(&self, read: impl FnOnce(Option<&ExtensionManifest>) -> R) -> R {
        if let Some(local) = self.local() {
            return read(Some(&local.manifest));
        }
        match self.resource_extension() {
            Some(resource_extension) => read(Some(&resource_extension.manifest)),
            None => read(None),
        }
    }

    fn local_icon_url(&self) -> Option<String> {
        let local = self.local()?;
        let icon = non_empty(local.manifest.icon.as_ref())?;
        let uri = URI::join_path(&Self::file_uri(&local.location), &[icon.as_str()], Self::platform());
        Some(self.services.file_access.uri_to_browser_uri(&uri).to_string(true).to_string_lossy())
    }

    fn resource_extension_icon_url(&self) -> Option<String> {
        let resource_extension = self.resource_extension()?;
        let icon = non_empty(resource_extension.manifest.icon.as_ref())?;
        let uri = URI::join_path(&resource_extension.location, &[icon.as_str()], Self::platform());
        Some(self.services.file_access.uri_to_browser_uri(&uri).to_string(true).to_string_lossy())
    }

    fn gallery_icon_url(&self) -> Option<String> {
        self.gallery()?.assets.icon.as_ref().map(|icon| icon.uri.clone())
    }

    fn default_icon_url(&self) -> Option<String> {
        let local = self.local()?;
        if self.extension_type() != ExtensionType::System {
            return None;
        }
        let contributes = local.manifest.contributes.as_ref()?;
        if !contributes.themes.is_empty() {
            return Some(self.services.file_access.as_browser_uri("vs/workbench/contrib/extensions/browser/media/theme-icon.png").to_string(true).to_string_lossy());
        }
        if !contributes.grammars.is_empty() {
            return Some(self.services.file_access.as_browser_uri("vs/workbench/contrib/extensions/browser/media/language-icon.svg").to_string(true).to_string_lossy());
        }
        None
    }

    /// `getLocal()`: a extensão local, quando ela não está desatualizada.
    fn get_local(&self) -> Option<Rc<LocalExtension>> {
        self.local().filter(|_| !self.outdated())
    }

    /// Os pares de `telemetryData` (`getGalleryExtensionTelemetryData` ou `getLocalExtensionTelemetryData`).
    fn telemetry_entries(&self) -> Vec<(String, Dynamic)> {
        let optional = |value: &Option<String>| value.as_deref().map_or(Dynamic::Null, Dynamic::string);
        if let Some(gallery) = self.gallery() {
            return vec![
                ("id".to_owned(), TelemetryTrustedValue { value: Dynamic::string(&gallery.identifier.id) }.to_dynamic()),
                ("name".to_owned(), TelemetryTrustedValue { value: Dynamic::string(&gallery.name) }.to_dynamic()),
                ("extensionVersion".to_owned(), Dynamic::string(&gallery.version)),
                ("galleryId".to_owned(), optional(&gallery.identifier.uuid)),
                ("publisherId".to_owned(), Dynamic::string(&gallery.publisher_id)),
                ("publisherName".to_owned(), Dynamic::string(&gallery.publisher)),
                ("publisherDisplayName".to_owned(), Dynamic::string(&gallery.publisher_display_name)),
                ("isPreReleaseVersion".to_owned(), Dynamic::Bool(gallery.properties.is_pre_release_version)),
                ("dependencies".to_owned(), Dynamic::Bool(gallery.properties.dependencies.as_ref().is_some_and(|dependencies| !dependencies.is_empty()))),
                ("isSigned".to_owned(), Dynamic::Bool(gallery.is_signed)),
            ];
        }
        if let Some(local) = self.local() {
            return vec![
                ("id".to_owned(), Dynamic::string(&local.identifier.id)),
                ("name".to_owned(), Dynamic::string(&local.manifest.name)),
                ("galleryId".to_owned(), Dynamic::Null),
                ("publisherId".to_owned(), optional(&local.publisher_id)),
                ("publisherName".to_owned(), Dynamic::string(&local.manifest.publisher)),
                ("publisherDisplayName".to_owned(), optional(&local.publisher_display_name)),
                ("dependencies".to_owned(), Dynamic::Bool(local.manifest.extension_dependencies.as_ref().is_some_and(|dependencies| !dependencies.is_empty()))),
            ];
        }
        Vec::new()
    }

    fn read_resource(&self, resource: &URI) -> Promise<String> {
        self.services.file_service.read_file(resource, None, None).then(|content| content.value.to_string())
    }
}

impl IExtension for Extension {
    fn extension_type(&self) -> ExtensionType {
        self.local().map_or(ExtensionType::User, |local| local.extension_type)
    }

    fn is_builtin(&self) -> bool {
        self.local().is_some_and(|local| local.is_builtin)
    }

    fn is_workspace_scoped(&self) -> bool {
        if let Some(local) = self.local() {
            return local.is_workspace_scoped;
        }
        self.resource_extension_info.as_ref().is_some_and(|info| info.is_workspace_scoped)
    }

    fn state(&self) -> ExtensionState {
        (self.state_provider)(self)
    }

    fn name(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return gallery.name.clone();
        }
        self.with_manifest(|manifest| manifest.map(|manifest| manifest.name.clone()).unwrap_or_default())
    }

    fn display_name(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return if gallery.display_name.is_empty() { gallery.name.clone() } else { gallery.display_name.clone() };
        }
        match self.with_manifest(|manifest| manifest.and_then(|manifest| manifest.display_name.clone())) {
            Some(display_name) => display_name,
            None => self.name(),
        }
    }

    fn identifier(&self) -> ExtensionIdentifierInfo {
        if let Some(gallery) = self.gallery() {
            return gallery.identifier.clone();
        }
        if let Some(resource_extension) = self.resource_extension() {
            return resource_extension.identifier;
        }
        self.local().map_or_else(|| ExtensionIdentifierInfo::new(""), |local| local.identifier.clone())
    }

    fn publisher(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return gallery.publisher.clone();
        }
        self.with_manifest(|manifest| manifest.map(|manifest| manifest.publisher.clone()).unwrap_or_default())
    }

    fn publisher_display_name(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return if gallery.publisher_display_name.is_empty() { gallery.publisher.clone() } else { gallery.publisher_display_name.clone() };
        }
        if let Some(display_name) = self.local().and_then(|local| non_empty(local.publisher_display_name.as_ref()).cloned()) {
            return display_name;
        }
        self.publisher()
    }

    fn publisher_url(&self) -> Option<URI> {
        let gallery = self.gallery()?;
        non_empty(gallery.publisher_link.as_ref()).map(|link| URI::parse(link.as_str(), false))
    }

    fn publisher_domain(&self) -> Option<(String, bool)> {
        self.gallery()?.publisher_domain.clone()
    }

    fn publisher_sponsor_link(&self) -> Option<URI> {
        let gallery = self.gallery()?;
        non_empty(gallery.publisher_sponsor_link.as_ref()).map(|link| URI::parse(link.as_str(), false))
    }

    fn pinned(&self) -> bool {
        self.local().is_some_and(|local| local.pinned)
    }

    fn version(&self) -> String {
        match self.local() {
            Some(local) => local.manifest.version.clone(),
            None => self.latest_version(),
        }
    }

    fn is_private(&self) -> bool {
        if let Some(gallery) = self.gallery() {
            return gallery.private;
        }
        self.local().is_some_and(|local| local.private)
    }

    fn latest_version(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return gallery.version.clone();
        }
        self.with_manifest(|manifest| manifest.map(|manifest| manifest.version.clone()).unwrap_or_default())
    }

    fn pre_release(&self) -> bool {
        self.local().is_some_and(|local| local.pre_release)
    }

    fn is_pre_release_version(&self) -> bool {
        if let Some(local) = self.local() {
            return local.is_pre_release_version;
        }
        self.gallery().is_some_and(|gallery| gallery.properties.is_pre_release_version)
    }

    fn has_pre_release_version(&self) -> bool {
        match self.gallery() {
            Some(gallery) => gallery.has_pre_release_version,
            None => self.local().is_some_and(|local| local.has_pre_release_version),
        }
    }

    fn has_release_version(&self) -> bool {
        self.resource_extension().is_some() || self.gallery().is_some_and(|gallery| gallery.has_release_version)
    }

    fn description(&self) -> String {
        if let Some(gallery) = self.gallery() {
            return gallery.description.clone();
        }
        self.with_manifest(|manifest| manifest.and_then(|manifest| manifest.description.clone()).unwrap_or_default())
    }

    fn url(&self) -> Option<String> {
        self.gallery()?.details_link.clone()
    }

    fn repository(&self) -> Option<String> {
        self.gallery()?.assets.repository.as_ref().map(|repository| repository.uri.clone())
    }

    fn support_url(&self) -> Option<String> {
        non_empty(self.gallery()?.support_link.as_ref()).cloned()
    }

    fn icon_url(&self) -> Option<String> {
        [self.gallery_icon_url(), self.resource_extension_icon_url(), self.local_icon_url(), self.default_icon_url()]
            .into_iter()
            .flatten()
            .find(|url| !url.is_empty())
    }

    fn icon_url_fallback(&self) -> Option<String> {
        self.gallery()?.assets.icon.as_ref().map(|icon| icon.fallback_uri.clone())
    }

    fn license_url(&self) -> Option<String> {
        self.gallery()?.assets.license.as_ref().map(|license| license.uri.clone())
    }

    fn install_count(&self) -> Option<f64> {
        self.gallery().map(|gallery| gallery.install_count)
    }

    fn rating(&self) -> Option<f64> {
        self.gallery().map(|gallery| gallery.rating)
    }

    fn rating_count(&self) -> Option<f64> {
        self.gallery().map(|gallery| gallery.rating_count)
    }

    fn rating_url(&self) -> Option<String> {
        self.gallery()?.rating_link.clone()
    }

    fn outdated(&self) -> bool {
        let (Some(gallery), Some(local)) = (self.gallery(), self.local()) else {
            return false;
        };
        // Do not allow updating system extensions in stable
        if self.extension_type() == ExtensionType::System && self.services.product_service.quality() == Some("stable") {
            let identifier = self.identifier().id.to_lowercase();
            let enabled_with_auto_updates = self.services.product_service.built_in_extensions_enabled_with_auto_updates();
            if !enabled_with_auto_updates.iter().any(|id| id.to_lowercase() == identifier) {
                return false;
            }
        }
        if !local.pre_release && gallery.properties.is_pre_release_version {
            return false;
        }
        if semver::gt(&self.latest_version(), &self.version()) {
            return true;
        }
        self.outdated_target_platform()
    }

    fn outdated_target_platform(&self) -> bool {
        let (Some(gallery), Some(local)) = (self.gallery(), self.local()) else {
            return false;
        };
        !matches!(local.target_platform, TargetPlatform::Undefined | TargetPlatform::Web)
            && gallery.properties.target_platform != TargetPlatform::Web
            && local.target_platform != gallery.properties.target_platform
            && semver::eq(&self.latest_version(), &self.version())
    }

    fn runtime_state(&self) -> Option<ExtensionRuntimeState> {
        (self.runtime_state_provider)(self)
    }

    fn enablement_state(&self) -> EnablementState {
        self.enablement_state.get()
    }

    fn tags(&self) -> Vec<String> {
        match self.gallery() {
            Some(gallery) => gallery.tags.iter().filter(|tag| !tag.starts_with('_')).cloned().collect(),
            None => Vec::new(),
        }
    }

    fn categories(&self) -> Vec<String> {
        if let Some(local) = self.local() {
            if let Some(categories) = &local.manifest.categories {
                if !self.outdated() {
                    return categories.clone();
                }
            }
        }
        if let Some(gallery) = self.gallery() {
            return gallery.categories.clone();
        }
        match self.resource_extension() {
            Some(resource_extension) => resource_extension.manifest.categories.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn dependencies(&self) -> Vec<String> {
        if let Some(local) = self.local() {
            if let Some(dependencies) = &local.manifest.extension_dependencies {
                if !self.outdated() {
                    return dependencies.clone();
                }
            }
        }
        if let Some(gallery) = self.gallery() {
            return gallery.properties.dependencies.clone().unwrap_or_default();
        }
        match self.resource_extension() {
            Some(resource_extension) => resource_extension.manifest.extension_dependencies.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn extension_pack(&self) -> Vec<String> {
        if let Some(local) = self.local() {
            if let Some(extension_pack) = &local.manifest.extension_pack {
                if !self.outdated() {
                    return extension_pack.clone();
                }
            }
        }
        if let Some(gallery) = self.gallery() {
            return gallery.properties.extension_pack.clone().unwrap_or_default();
        }
        match self.resource_extension() {
            Some(resource_extension) => resource_extension.manifest.extension_pack.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn telemetry_data(&self) -> Dynamic {
        Dynamic::object(self.telemetry_entries())
    }

    fn preview(&self) -> bool {
        let local_preview = self.local().and_then(|local| local.manifest.preview);
        local_preview.or_else(|| self.gallery().map(|gallery| gallery.preview)).unwrap_or(false)
    }

    fn get_manifest(&self, token: CancellationToken) -> Promise<Option<ExtensionManifest>> {
        if let Some(local) = self.get_local() {
            return Promise::resolved(Some(local.manifest.clone()));
        }
        if self.gallery().is_some() {
            return self.get_gallery_manifest(token);
        }
        if let Some(resource_extension) = self.resource_extension() {
            return Promise::resolved(Some(resource_extension.manifest));
        }
        Promise::resolved(None)
    }

    fn has_readme(&self) -> bool {
        if self.local().is_some_and(|local| non_empty(local.readme_url.as_ref()).is_some()) {
            return true;
        }
        if self.gallery().is_some_and(|gallery| gallery.assets.readme.is_some()) {
            return true;
        }
        if self.resource_extension().is_some_and(|resource_extension| resource_extension.readme_uri.is_some()) {
            return true;
        }
        self.extension_type() == ExtensionType::System
    }

    fn get_readme(&self, _token: CancellationToken) -> Promise<String> {
        if let Some(readme_url) = self.get_local().and_then(|local| non_empty(local.readme_url.as_ref()).cloned()) {
            return self.read_resource(&Self::file_uri(&readme_url));
        }

        if let Some(gallery) = self.gallery() {
            if gallery.assets.readme.is_some() {
                return match self.services.gallery_service.get_readme(&gallery) {
                    Ok(readme) => Promise::resolved(readme),
                    Err(error) => Promise::rejected(Error::new(error.message)),
                };
            }
            self.services.telemetry_service.public_log(
                "extensions:NotFoundReadMe",
                Some(ITelemetryData { from: None, target: None, extra: self.telemetry_entries() }),
            );
        }

        if self.extension_type() == ExtensionType::System {
            return Promise::resolved(format!(
                "# {}\n**Notice:** This extension is bundled with Visual Studio Code. It can be disabled but not uninstalled.\n## Features\n{}\n",
                non_empty(Some(&self.display_name())).cloned().unwrap_or_else(|| self.name()),
                self.description()
            ));
        }

        if let Some(readme_uri) = self.resource_extension().and_then(|resource_extension| resource_extension.readme_uri) {
            return self.read_resource(&readme_uri);
        }

        Promise::rejected(Error::new("not available"))
    }

    fn has_changelog(&self) -> bool {
        if self.local().is_some_and(|local| non_empty(local.changelog_url.as_ref()).is_some()) {
            return true;
        }
        if self.gallery().is_some_and(|gallery| gallery.assets.changelog.is_some()) {
            return true;
        }
        self.extension_type() == ExtensionType::System
    }

    fn get_changelog(&self, _token: CancellationToken) -> Promise<String> {
        if let Some(changelog_url) = self.get_local().and_then(|local| non_empty(local.changelog_url.as_ref()).cloned()) {
            return self.read_resource(&Self::file_uri(&changelog_url));
        }

        if let Some(gallery) = self.gallery() {
            if gallery.assets.changelog.is_some() {
                return match self.services.gallery_service.get_changelog(&gallery) {
                    Ok(changelog) => Promise::resolved(changelog),
                    Err(error) => Promise::rejected(Error::new(error.message)),
                };
            }
        }

        if self.extension_type() == ExtensionType::System {
            return Promise::resolved(format!(
                "Please check the [VS Code Release Notes](command:{SHOW_CURRENT_RELEASE_NOTES_ACTION_ID}) for changes to the built-in extensions."
            ));
        }

        Promise::rejected(Error::new("not available"))
    }

    fn server(&self) -> Option<Rc<IExtensionManagementServer>> {
        self.server.clone()
    }

    fn local(&self) -> Option<Rc<LocalExtension>> {
        self.local.borrow().clone()
    }

    fn gallery(&self) -> Option<Rc<GalleryExtension>> {
        self.gallery.borrow().clone()
    }

    fn set_gallery(&self, gallery: Option<Rc<GalleryExtension>>) {
        *self.gallery.borrow_mut() = gallery;
        *self.gallery_manifest_cache.borrow_mut() = None;
    }

    fn resource_extension(&self) -> Option<IResourceExtension> {
        if let Some(info) = &self.resource_extension_info {
            return Some(info.resource_extension.clone());
        }
        let local = self.local()?;
        if local.is_workspace_scoped {
            return Some(IResourceExtension {
                identifier: local.identifier.clone(),
                location: Self::file_uri(&local.location),
                manifest: local.manifest.clone(),
                changelog_uri: non_empty(local.changelog_url.as_ref()).map(|url| Self::file_uri(url)),
                readme_uri: non_empty(local.readme_url.as_ref()).map(|url| Self::file_uri(url)),
            });
        }
        None
    }

    fn is_malicious(&self) -> Option<bool> {
        Some(self.malicious.borrow().is_some() || self.enablement_state() == EnablementState::DisabledByMalicious)
    }

    fn malicious_info_link(&self) -> Option<String> {
        self.malicious.borrow().as_ref().and_then(|malicious| malicious.learn_more_link.clone())
    }

    fn deprecation_info(&self) -> Option<DeprecationInfo> {
        self.deprecation_info.borrow().clone()
    }

    fn missing_from_gallery(&self) -> bool {
        self.missing_from_gallery.get()
    }
}
