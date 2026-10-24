//! Porte de `src/vs/platform/extensionManagement/common/extensionGalleryManifest.ts`: o manifesto da galeria de
//! extensões (os recursos e as capacidades de consulta que ela oferece), o estado de acesso a ela e o contrato do
//! serviço que a expõe.
//!
//! Divergências:
//! - Os `const enum` de valor textual (`ExtensionGalleryResourceType`, `Flag`, `ExtensionGalleryManifestStatus`) são
//!   enums com `as_str`, o texto que o original grava. O `type` de um recurso continua texto livre (`string`), porque
//!   a galeria pode declarar tipos que o cliente não conhece.
//! - `number` dos valores de capacidade é `f64`. Os campos `readonly x?: T` são `Option<T>`.
//! - O manifesto trafega para o processo compartilhado e para o servidor remoto por canal: [`IExtensionGalleryManifest::to_marshalled`]
//!   é o objeto que o `JSON.stringify` do original escreveria (campo ausente omitido).
//! - `Event<IExtensionGalleryManifest | null>` é `Event<Option<IExtensionGalleryManifest>>`.

use je_base::JsString;
use je_base::event::Event;
use je_base::marshalling::Marshalled;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `ExtensionGalleryResourceType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionGalleryResourceType {
    ExtensionQueryService,
    ExtensionLatestVersionUri,
    ExtensionStatisticsUri,
    PublisherViewUri,
    ExtensionDetailsViewUri,
    ExtensionRatingViewUri,
    ExtensionResourceUri,
    ContactSupportUri,
}

impl ExtensionGalleryResourceType {
    /// O texto do tipo, como a galeria o declara.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExtensionQueryService => "ExtensionQueryService",
            Self::ExtensionLatestVersionUri => "ExtensionLatestVersionUriTemplate",
            Self::ExtensionStatisticsUri => "ExtensionStatisticsUriTemplate",
            Self::PublisherViewUri => "PublisherViewUriTemplate",
            Self::ExtensionDetailsViewUri => "ExtensionDetailsViewUriTemplate",
            Self::ExtensionRatingViewUri => "ExtensionRatingViewUriTemplate",
            Self::ExtensionResourceUri => "ExtensionResourceUriTemplate",
            Self::ContactSupportUri => "ContactSupportUri",
        }
    }
}

/// `Flag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    None,
    IncludeVersions,
    IncludeFiles,
    IncludeCategoryAndTags,
    IncludeSharedAccounts,
    IncludeVersionProperties,
    ExcludeNonValidated,
    IncludeInstallationTargets,
    IncludeAssetUri,
    IncludeStatistics,
    IncludeLatestVersionOnly,
    Unpublished,
    IncludeNameConflictInfo,
    IncludeLatestPrereleaseAndStableVersionOnly,
}

impl Flag {
    /// O nome do flag, como a galeria o declara.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::IncludeVersions => "IncludeVersions",
            Self::IncludeFiles => "IncludeFiles",
            Self::IncludeCategoryAndTags => "IncludeCategoryAndTags",
            Self::IncludeSharedAccounts => "IncludeSharedAccounts",
            Self::IncludeVersionProperties => "IncludeVersionProperties",
            Self::ExcludeNonValidated => "ExcludeNonValidated",
            Self::IncludeInstallationTargets => "IncludeInstallationTargets",
            Self::IncludeAssetUri => "IncludeAssetUri",
            Self::IncludeStatistics => "IncludeStatistics",
            Self::IncludeLatestVersionOnly => "IncludeLatestVersionOnly",
            Self::Unpublished => "Unpublished",
            Self::IncludeNameConflictInfo => "IncludeNameConflictInfo",
            Self::IncludeLatestPrereleaseAndStableVersionOnly => "IncludeLatestPrereleaseAndStableVersionOnly",
        }
    }
}

/// `ExtensionGalleryManifestResource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGalleryManifestResource {
    pub id: String,
    pub r#type: String,
}

/// `ExtensionQueryCapabilityValue`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionQueryCapabilityValue {
    pub name: String,
    pub value: f64,
}

/// `IExtensionGalleryManifest.capabilities.extensionQuery`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionQueryCapabilities {
    pub filtering: Option<Vec<ExtensionQueryCapabilityValue>>,
    pub sorting: Option<Vec<ExtensionQueryCapabilityValue>>,
    pub flags: Option<Vec<ExtensionQueryCapabilityValue>>,
}

/// `IExtensionGalleryManifest.capabilities.signing`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningCapabilities {
    pub all_public_repository_signed: bool,
    pub all_private_repository_signed: Option<bool>,
}

/// `IExtensionGalleryManifest.capabilities.extensions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionsCapabilities {
    pub include_public_extensions: Option<bool>,
    pub include_private_extensions: Option<bool>,
}

/// `IExtensionGalleryManifest.capabilities`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionGalleryManifestCapabilities {
    pub extension_query: ExtensionQueryCapabilities,
    pub signing: Option<SigningCapabilities>,
    pub extensions: Option<ExtensionsCapabilities>,
}

/// `IExtensionGalleryManifest`.
#[derive(Clone, Debug, PartialEq)]
pub struct IExtensionGalleryManifest {
    pub version: String,
    pub resources: Vec<ExtensionGalleryManifestResource>,
    pub capabilities: ExtensionGalleryManifestCapabilities,
}

/// O objeto do `JSON.stringify`: as entradas na ordem dada, com as chaves como texto.
fn object(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(key, value)| (JsString::from(key), value)).collect())
}

fn text(value: &str) -> Marshalled {
    Marshalled::String(JsString::from(value))
}

fn capability_values(values: &[ExtensionQueryCapabilityValue]) -> Marshalled {
    Marshalled::Array(values.iter().map(|value| object(vec![("name", text(&value.name)), ("value", Marshalled::Number(value.value))])).collect())
}

/// Acrescenta a entrada só quando o valor existe (o `JSON.stringify` omite `undefined`).
fn push_some<T>(entries: &mut Vec<(&'static str, Marshalled)>, key: &'static str, value: &Option<T>, convert: impl FnOnce(&T) -> Marshalled) {
    if let Some(value) = value {
        entries.push((key, convert(value)));
    }
}

impl IExtensionGalleryManifest {
    /// O manifesto como o `JSON.stringify` o escreveria, para o canal do processo compartilhado e do servidor remoto.
    pub fn to_marshalled(&self) -> Marshalled {
        let resources = Marshalled::Array(self.resources.iter().map(|resource| object(vec![("id", text(&resource.id)), ("type", text(&resource.r#type))])).collect());

        let query = &self.capabilities.extension_query;
        let mut extension_query = Vec::new();
        push_some(&mut extension_query, "filtering", &query.filtering, |values| capability_values(values));
        push_some(&mut extension_query, "sorting", &query.sorting, |values| capability_values(values));
        push_some(&mut extension_query, "flags", &query.flags, |values| capability_values(values));

        let mut capabilities = vec![("extensionQuery", object(extension_query))];
        push_some(&mut capabilities, "signing", &self.capabilities.signing, |signing| {
            let mut entries = vec![("allPublicRepositorySigned", Marshalled::Bool(signing.all_public_repository_signed))];
            push_some(&mut entries, "allPrivateRepositorySigned", &signing.all_private_repository_signed, |signed| Marshalled::Bool(*signed));
            object(entries)
        });
        push_some(&mut capabilities, "extensions", &self.capabilities.extensions, |extensions| {
            let mut entries = Vec::new();
            push_some(&mut entries, "includePublicExtensions", &extensions.include_public_extensions, |include| Marshalled::Bool(*include));
            push_some(&mut entries, "includePrivateExtensions", &extensions.include_private_extensions, |include| Marshalled::Bool(*include));
            object(entries)
        });

        object(vec![("version", text(&self.version)), ("resources", resources), ("capabilities", object(capabilities))])
    }
}

/// `ExtensionGalleryManifestStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionGalleryManifestStatus {
    Available,
    RequiresSignIn,
    AccessDenied,
    Unavailable,
}

impl ExtensionGalleryManifestStatus {
    /// O texto do estado, como o original o grava.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::RequiresSignIn => "requiresSignIn",
            Self::AccessDenied => "accessDenied",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `IExtensionGalleryManifestService`.
pub trait IExtensionGalleryManifestService {
    fn extension_gallery_manifest_status(&self) -> ExtensionGalleryManifestStatus;
    fn on_did_change_extension_gallery_manifest_status(&self) -> Event<ExtensionGalleryManifestStatus>;
    fn on_did_change_extension_gallery_manifest(&self) -> Event<Option<IExtensionGalleryManifest>>;
    fn get_extension_gallery_manifest(&self) -> Promise<Option<IExtensionGalleryManifest>>;
}

service_id!(pub I_EXTENSION_GALLERY_MANIFEST_SERVICE: dyn IExtensionGalleryManifestService = "IExtensionGalleryManifestService");

/// `getExtensionGalleryManifestResourceUri`: o `id` do primeiro recurso do tipo pedido. `type` é `nome` ou
/// `nome/versão`; sem versão vale o primeiro recurso do nome, com versão o primeiro recurso do nome só vale se a
/// versão for a mesma (se não for, a busca para: os demais recursos do mesmo nome não são consultados).
pub fn get_extension_gallery_manifest_resource_uri(manifest: &IExtensionGalleryManifest, r#type: &str) -> Option<String> {
    let mut parts = r#type.split('/');
    let name = parts.next().unwrap_or_default();
    let version = parts.next();
    for resource in &manifest.resources {
        let mut resource_parts = resource.r#type.split('/');
        let resource_name = resource_parts.next().unwrap_or_default();
        let resource_version = resource_parts.next();
        if resource_name != name {
            continue;
        }
        if matches!(version, None | Some("")) || resource_version == version {
            return Some(resource.id.clone());
        }
        break;
    }
    None
}

/// `ExtensionGalleryServiceUrlConfigKey`.
pub const EXTENSION_GALLERY_SERVICE_URL_CONFIG_KEY: &str = "extensions.gallery.serviceUrl";

/// `ExtensionGalleryAuthProviderConfigKey`.
pub const EXTENSION_GALLERY_AUTH_PROVIDER_CONFIG_KEY: &str = "extensions.gallery.authProvider";
