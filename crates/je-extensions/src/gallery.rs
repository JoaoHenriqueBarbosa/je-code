//! Porte de `src/vs/platform/extensionManagement/common/extensionGalleryService.ts` e do manifesto do marketplace
//! (`extensionGalleryManifest.ts`, `extensionGalleryManifestService.ts`): o cliente da API do marketplace, com a
//! consulta por extensão e por texto (`POST extensionquery` com os filtros e as flags do original), a API do "latest" por
//! extensão, a escolha da versão compatível e o download do VSIX.
//!
//! A URL do serviço vem do `product.json` (`extensionsGallery.serviceUrl`), nunca é fixa: [`GalleryManifest::from_product`].
//!
//! Diferenças de execução: a E/S é síncrona ([`HttpClient`]); a telemetria do original some (o je-code não coleta);
//! o `CancellationToken` some (a chamada é síncrona).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use je_platform::log::Logger;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::allowed::{AllowedCandidate, AllowedExtensionsService};
use crate::fs::ExtFs;
use crate::http::{HttpClient, HttpErrorKind, HttpRequest, HttpResponse};
use crate::management::{
    ExtensionBlockingInfo, ExtensionBlockingOrigin, ExtensionGalleryError, ExtensionGalleryErrorCode, ExtensionInfo, ExtensionOrPublisher, ExtensionQueryOptions,
    ExtensionsControlManifest, FilterType, GalleryExtension, GalleryExtensionAsset, GalleryExtensionAssets, GalleryExtensionProperties, GalleryExtensionVersion,
    InstallOperation, MaliciousExtensionInfo, ProductVersion, QueryOptions, SearchPreferredResults, SortBy, StatisticType, WEB_EXTENSION_TAG,
    is_extension_identifier, is_not_web_extension_in_web_target_platform, is_target_platform_compatible, to_target_platform,
};
use crate::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use crate::util::{adopt_to_gallery_extension_id, are_same_extensions, get_gallery_extension_id};
use crate::validator::{is_engine_valid, parse_date_ms};
use je_base::semver;

// ---- manifesto do marketplace ----

/// `ExtensionGalleryResourceType`.
pub mod resource_type {
    pub const EXTENSION_QUERY_SERVICE: &str = "ExtensionQueryService";
    pub const EXTENSION_LATEST_VERSION_URI: &str = "ExtensionLatestVersionUriTemplate";
    pub const EXTENSION_STATISTICS_URI: &str = "ExtensionStatisticsUriTemplate";
    pub const PUBLISHER_VIEW_URI: &str = "PublisherViewUriTemplate";
    pub const EXTENSION_DETAILS_VIEW_URI: &str = "ExtensionDetailsViewUriTemplate";
    pub const EXTENSION_RATING_VIEW_URI: &str = "ExtensionRatingViewUriTemplate";
    pub const EXTENSION_RESOURCE_URI: &str = "ExtensionResourceUriTemplate";
    pub const CONTACT_SUPPORT_URI: &str = "ContactSupportUri";
}

/// `Flag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// `IExtensionGalleryManifest`.
#[derive(Clone, Debug, Default)]
pub struct GalleryManifest {
    /// `(id, type)`: o `id` é o modelo da URL.
    pub resources: Vec<(String, String)>,
    pub filtering: Vec<(String, i64)>,
    pub sorting: Vec<(String, i64)>,
    pub flags: Vec<(String, i64)>,
}

impl GalleryManifest {
    /// `ExtensionGalleryManifestService#getExtensionGalleryManifest`: monta o manifesto de `extensionsGallery` do
    /// `product.json`. `None` sem `serviceUrl`.
    pub fn from_product(extensions_gallery: &Value) -> Option<GalleryManifest> {
        let text = |key: &str| extensions_gallery.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
        let service_url = text("serviceUrl")?;
        let mut resources = vec![
            (format!("{service_url}/extensionquery"), resource_type::EXTENSION_QUERY_SERVICE.to_owned()),
            (format!("{service_url}/vscode/{{publisher}}/{{name}}/latest"), resource_type::EXTENSION_LATEST_VERSION_URI.to_owned()),
            (
                format!("{service_url}/publishers/{{publisher}}/extensions/{{name}}/{{version}}/stats?statType={{statTypeName}}"),
                resource_type::EXTENSION_STATISTICS_URI.to_owned(),
            ),
        ];
        if let Some(publisher_url) = text("publisherUrl") {
            resources.push((format!("{publisher_url}/{{publisher}}"), resource_type::PUBLISHER_VIEW_URI.to_owned()));
        }
        if let Some(item_url) = text("itemUrl") {
            resources.push((format!("{item_url}?itemName={{publisher}}.{{name}}"), resource_type::EXTENSION_DETAILS_VIEW_URI.to_owned()));
            resources.push((format!("{item_url}?itemName={{publisher}}.{{name}}&ssr=false#review-details"), resource_type::EXTENSION_RATING_VIEW_URI.to_owned()));
        }
        if let Some(template) = text("resourceUrlTemplate") {
            resources.push((template.to_owned(), resource_type::EXTENSION_RESOURCE_URI.to_owned()));
        }
        let named = |items: &[(&str, i64)]| items.iter().map(|(name, value)| ((*name).to_owned(), *value)).collect::<Vec<_>>();
        Some(GalleryManifest {
            resources,
            filtering: named(&[("Tag", 1), ("ExtensionId", 4), ("Category", 5), ("ExtensionName", 7), ("Target", 8), ("Featured", 9), ("SearchText", 10), ("ExcludeWithFlags", 12)]),
            sorting: named(&[
                ("NoneOrRelevance", 0),
                ("LastUpdatedDate", 1),
                ("Title", 2),
                ("PublisherName", 3),
                ("InstallCount", 4),
                ("AverageRating", 6),
                ("PublishedDate", 10),
                ("WeightedRating", 12),
            ]),
            flags: named(&[
                ("None", 0x0),
                ("IncludeVersions", 0x1),
                ("IncludeFiles", 0x2),
                ("IncludeCategoryAndTags", 0x4),
                ("IncludeSharedAccounts", 0x8),
                ("IncludeVersionProperties", 0x10),
                ("ExcludeNonValidated", 0x20),
                ("IncludeInstallationTargets", 0x40),
                ("IncludeAssetUri", 0x80),
                ("IncludeStatistics", 0x100),
                ("IncludeLatestVersionOnly", 0x200),
                ("Unpublished", 0x1000),
                ("IncludeNameConflictInfo", 0x8000),
                ("IncludeLatestPrereleaseAndStableVersionOnly", 0x10000),
            ]),
        })
    }

    /// `getExtensionGalleryManifestResourceUri`: casa `Nome` ou `Nome/versão`; o primeiro recurso do mesmo nome vale.
    pub fn resource_uri(&self, type_name: &str) -> Option<&str> {
        let (name, version) = match type_name.split_once('/') {
            Some((name, version)) => (name, Some(version)),
            None => (type_name, None),
        };
        for (id, resource_type) in &self.resources {
            let (resource_name, resource_version) = match resource_type.split_once('/') {
                Some((name, version)) => (name, Some(version)),
                None => (resource_type.as_str(), None),
            };
            if resource_name != name {
                continue;
            }
            if version.is_none_or(|v| v.is_empty()) || resource_version == version {
                return Some(id);
            }
            break;
        }
        None
    }

    fn filter_value(&self, name: &str) -> Option<i64> {
        self.filtering.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    fn sort_value(&self, name: &str) -> Option<i64> {
        self.sorting.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    fn flag_value(&self, name: &str) -> Option<i64> {
        self.flags.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

/// `format2`: troca `{chave}` pelos valores; um marcador sem valor fica como está.
pub fn format_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match values.iter().find(|(k, _)| *k == key) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---- tipos crus da API ----

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGalleryExtensionFile {
    #[serde(default)]
    pub asset_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RawGalleryExtensionProperty {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
}

/// `IRawGalleryExtensionBlocking`: o contrato traz mais, ignorado. A `origin` de tipo errado degrada para `service`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RawGalleryExtensionBlocking {
    #[serde(default)]
    pub origin: Option<Value>,
}

/// `IRawGalleryExtensionVersion`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGalleryExtensionVersion {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub last_updated: String,
    #[serde(default)]
    pub asset_uri: String,
    #[serde(default)]
    pub fallback_asset_uri: String,
    /// `None` quando a resposta omite `files` (o "latest" inválido que força o fallback para a consulta).
    #[serde(default)]
    pub files: Option<Vec<RawGalleryExtensionFile>>,
    #[serde(default)]
    pub properties: Option<Vec<RawGalleryExtensionProperty>>,
    #[serde(default)]
    pub target_platform: Option<String>,
    #[serde(default)]
    pub flags: Option<String>,
    #[serde(default)]
    pub blocking: Option<RawGalleryExtensionBlocking>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatistic {
    #[serde(default)]
    statistic_name: String,
    #[serde(default)]
    value: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPublisher {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    publisher_id: String,
    #[serde(default)]
    publisher_name: String,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    is_domain_verified: Option<bool>,
    #[serde(default)]
    link_type: Option<String>,
}

/// `IRawGalleryExtension`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGalleryExtension {
    #[serde(default)]
    extension_id: String,
    #[serde(default)]
    extension_name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    publisher: RawPublisher,
    #[serde(default)]
    pub versions: Vec<RawGalleryExtensionVersion>,
    #[serde(default)]
    statistics: Vec<RawStatistic>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    last_updated: String,
    #[serde(default)]
    categories: Option<Vec<String>>,
    #[serde(default)]
    pub flags: String,
    #[serde(default)]
    pub blocking: Option<RawGalleryExtensionBlocking>,
    #[serde(default)]
    link_type: Option<String>,
    #[serde(default)]
    rating_link_type: Option<String>,
}

#[derive(Deserialize)]
struct RawQueryResult {
    #[serde(default)]
    results: Vec<RawQueryResultSet>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQueryResultSet {
    #[serde(default)]
    extensions: Vec<RawGalleryExtension>,
    #[serde(default)]
    result_metadata: Vec<RawResultMetadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResultMetadata {
    #[serde(default)]
    metadata_type: String,
    #[serde(default)]
    metadata_items: Vec<RawMetadataItem>,
}

#[derive(Deserialize)]
struct RawMetadataItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    count: f64,
}

const ASSET_ICON: &str = "Microsoft.VisualStudio.Services.Icons.Default";
const ASSET_DETAILS: &str = "Microsoft.VisualStudio.Services.Content.Details";
const ASSET_CHANGELOG: &str = "Microsoft.VisualStudio.Services.Content.Changelog";
const ASSET_MANIFEST: &str = "Microsoft.VisualStudio.Code.Manifest";
const ASSET_VSIX: &str = "Microsoft.VisualStudio.Services.VSIXPackage";
const ASSET_LICENSE: &str = "Microsoft.VisualStudio.Services.Content.License";
const ASSET_REPOSITORY: &str = "Microsoft.VisualStudio.Services.Links.Source";
const ASSET_SIGNATURE: &str = "Microsoft.VisualStudio.Services.VsixSignature";

const PROPERTY_DEPENDENCY: &str = "Microsoft.VisualStudio.Code.ExtensionDependencies";
const PROPERTY_EXTENSION_PACK: &str = "Microsoft.VisualStudio.Code.ExtensionPack";
const PROPERTY_ENGINE: &str = "Microsoft.VisualStudio.Code.Engine";
const PROPERTY_PRE_RELEASE: &str = "Microsoft.VisualStudio.Code.PreRelease";
const PROPERTY_ENABLED_API_PROPOSALS: &str = "Microsoft.VisualStudio.Code.EnabledApiProposals";
const PROPERTY_LOCALIZED_LANGUAGES: &str = "Microsoft.VisualStudio.Code.LocalizedLanguages";
const PROPERTY_SPONSOR_LINK: &str = "Microsoft.VisualStudio.Code.SponsorLink";
const PROPERTY_SUPPORT_LINK: &str = "Microsoft.VisualStudio.Services.Links.Support";
const PROPERTY_EXECUTES_CODE: &str = "Microsoft.VisualStudio.Code.ExecutesCode";
const PROPERTY_PRIVATE: &str = "PrivateMarketplace";

/// O token de `flags` da extensão que a declara bloqueada.
const EXTENSION_FLAG_BLOCKED: &str = "blocked";

const DEFAULT_PAGE_SIZE: u32 = 10;

// ---- consulta ----

#[derive(Clone, Debug)]
struct Criterium {
    filter_type: FilterType,
    value: Option<String>,
}

/// `Query`: imutável, cada `with_*` devolve outra.
#[derive(Clone, Debug)]
struct Query {
    page_number: u32,
    page_size: u32,
    sort_by: SortBy,
    sort_order: u32,
    flags: Vec<Flag>,
    criteria: Vec<Criterium>,
    asset_types: Vec<String>,
}

impl Query {
    fn new() -> Self {
        Self { page_number: 1, page_size: DEFAULT_PAGE_SIZE, sort_by: SortBy::NoneOrRelevance, sort_order: 0, flags: Vec::new(), criteria: Vec::new(), asset_types: Vec::new() }
    }

    fn with_page(mut self, page_number: u32, page_size: Option<u32>) -> Self {
        self.page_number = page_number;
        if let Some(size) = page_size {
            self.page_size = size;
        }
        self
    }

    fn with_filter(mut self, filter_type: FilterType, values: &[String]) -> Self {
        if values.is_empty() {
            self.criteria.push(Criterium { filter_type, value: None });
        } else {
            self.criteria.extend(values.iter().map(|value| Criterium { filter_type, value: Some(value.clone()) }));
        }
        self
    }

    fn with_sort_by(mut self, sort_by: SortBy) -> Self {
        self.sort_by = sort_by;
        self
    }

    fn with_sort_order(mut self, sort_order: u32) -> Self {
        self.sort_order = sort_order;
        self
    }

    /// `withFlags`: substitui as flags (sem repetir, na ordem da primeira aparição).
    fn with_flags(mut self, flags: &[Flag]) -> Self {
        let mut distinct: Vec<Flag> = Vec::new();
        for flag in flags {
            if !distinct.contains(flag) {
                distinct.push(*flag);
            }
        }
        self.flags = distinct;
        self
    }

    fn add_flags(self, extra: &[Flag]) -> Self {
        let mut all = self.flags.clone();
        all.extend_from_slice(extra);
        self.with_flags(&all)
    }
}

/// `VersionKind`, e a versão exata como texto.
#[derive(Clone, Debug, PartialEq, Eq)]
enum VersionCriterion {
    Release,
    Prerelease,
    Latest,
    Exact(String),
}

struct ExtensionVersionCriteria {
    product_version: ProductVersion,
    target_platform: TargetPlatform,
    compatible: bool,
    version: VersionCriterion,
    include_blocked_versions: bool,
}

/// Uma lista de identificadores com a escolha de pré-lançamento, ou um valor só.
enum IncludePreRelease {
    All(bool),
    PerExtension(Vec<(ExtensionIdentifierInfo, bool)>),
}

struct ExtensionsCriteria {
    product_version: ProductVersion,
    target_platform: TargetPlatform,
    compatible: bool,
    include_pre_release: IncludePreRelease,
    versions: Vec<(ExtensionIdentifierInfo, String)>,
    include_blocked_versions: bool,
    is_query_for_release_version_from_pre_release_version: bool,
}

// ---- funções de apoio ----

fn get_statistic(statistics: &[RawStatistic], name: &str) -> f64 {
    statistics.iter().find(|s| s.statistic_name == name).map_or(0.0, |s| s.value)
}

fn files(version: &RawGalleryExtensionVersion) -> &[RawGalleryExtensionFile] {
    version.files.as_deref().unwrap_or_default()
}

fn properties(version: &RawGalleryExtensionVersion) -> &[RawGalleryExtensionProperty] {
    version.properties.as_deref().unwrap_or_default()
}

fn property<'a>(version: &'a RawGalleryExtensionVersion, key: &str) -> Option<&'a str> {
    properties(version).iter().find(|p| p.key == key).map(|p| p.value.as_str())
}

fn platform_query(version: &RawGalleryExtensionVersion, separator: &str) -> String {
    match &version.target_platform {
        Some(platform) if !platform.is_empty() => format!("{separator}targetPlatform={platform}"),
        _ => String::new(),
    }
}

fn get_version_asset(version: &RawGalleryExtensionVersion, asset_type: &str) -> Option<GalleryExtensionAsset> {
    files(version).iter().find(|f| f.asset_type == asset_type).map(|_| GalleryExtensionAsset {
        uri: format!("{}/{asset_type}{}", version.asset_uri, platform_query(version, "?")),
        fallback_uri: format!("{}/{asset_type}{}", version.fallback_asset_uri, platform_query(version, "?")),
    })
}

fn get_download_asset(version: &RawGalleryExtensionVersion) -> GalleryExtensionAsset {
    GalleryExtensionAsset {
        uri: format!("{}/{ASSET_VSIX}?redirect=true{}", version.fallback_asset_uri, platform_query(version, "&")),
        fallback_uri: format!("{}/{ASSET_VSIX}{}", version.fallback_asset_uri, platform_query(version, "?")),
    }
}

/// `(git|ssh|http(s)?)|(git@[\w.]+))(:(//)?)([\w.@:/\-~]+)(.git)(/)?`, sem âncora: procura o padrão em qualquer ponto.
fn looks_like_git_url(value: &str) -> bool {
    static REGEX: std::sync::OnceLock<zyarr::RegExp> = std::sync::OnceLock::new();
    let regex = REGEX.get_or_init(|| zyarr::RegExp::from_str(r"((git|ssh|http(s)?)|(git@[\w.]+))(:(//)?)([\w.@:/\-~]+)(.git)(/)?", "").expect("regex válido"));
    let units: Vec<u16> = value.encode_utf16().collect();
    regex.exec(&units, 0).is_some()
}

fn get_repository_asset(version: &RawGalleryExtensionVersion) -> Option<GalleryExtensionAsset> {
    if version.properties.is_some() {
        let found = properties(version).iter().filter(|p| p.key == ASSET_REPOSITORY).find(|p| looks_like_git_url(&p.value));
        return found.map(|p| GalleryExtensionAsset { uri: p.value.clone(), fallback_uri: p.value.clone() });
    }
    get_version_asset(version, ASSET_REPOSITORY)
}

fn get_core_translation_assets(version: &RawGalleryExtensionVersion) -> Vec<(String, GalleryExtensionAsset)> {
    const PREFIX: &str = "Microsoft.VisualStudio.Code.Translation.";
    files(version)
        .iter()
        .filter(|f| f.asset_type.starts_with(PREFIX))
        .filter_map(|f| get_version_asset(version, &f.asset_type).map(|asset| (f.asset_type[PREFIX.len()..].to_owned(), asset)))
        .collect()
}

fn get_extensions_list(version: &RawGalleryExtensionVersion, key: &str) -> Vec<String> {
    match property(version, key) {
        Some(value) if !value.is_empty() => value.split(',').map(adopt_to_gallery_extension_id).collect(),
        _ => Vec::new(),
    }
}

fn get_comma_list(version: &RawGalleryExtensionVersion, key: &str) -> Vec<String> {
    match property(version, key) {
        Some(value) if !value.is_empty() => value.split(',').map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

fn get_engine(version: &RawGalleryExtensionVersion) -> String {
    property(version, PROPERTY_ENGINE).unwrap_or_default().to_owned()
}

fn set_engine(version: &mut RawGalleryExtensionVersion, engine: &str) {
    version.properties.get_or_insert_with(Vec::new).push(RawGalleryExtensionProperty { key: PROPERTY_ENGINE.to_owned(), value: engine.to_owned() });
}

fn is_pre_release_version(version: &RawGalleryExtensionVersion) -> bool {
    property(version, PROPERTY_PRE_RELEASE) == Some("true")
}

fn is_private_extension(version: &RawGalleryExtensionVersion) -> bool {
    property(version, PROPERTY_PRIVATE) == Some("true")
}

/// `getBlockingInfo`: o estado de bloqueio de uma extensão numa versão. Em cada granularidade o flag e o objeto são duas
/// representações do mesmo fato, então qualquer um sozinho já significa bloqueado: um produtor que emite um sem o outro é
/// defeituoso, e falhar de forma segura é a resposta certa. O bloqueio da extensão vale mais que o da versão.
pub fn get_blocking_info(
    extension_flags: &str,
    extension_blocking: Option<&RawGalleryExtensionBlocking>,
    version_flags: Option<&str>,
    version_blocking: Option<&RawGalleryExtensionBlocking>,
) -> Option<ExtensionBlockingInfo> {
    if is_blocked(Some(extension_flags), extension_blocking) {
        return Some(ExtensionBlockingInfo { origin: to_blocking_origin(extension_blocking.and_then(|b| b.origin.as_ref())) });
    }
    if is_blocked(version_flags, version_blocking) {
        return Some(ExtensionBlockingInfo { origin: to_blocking_origin(version_blocking.and_then(|b| b.origin.as_ref())) });
    }
    None
}

fn is_blocked(flags: Option<&str>, blocking: Option<&RawGalleryExtensionBlocking>) -> bool {
    blocking.is_some() || has_extension_flag(flags, EXTENSION_FLAG_BLOCKED)
}

fn to_blocking_origin(origin: Option<&Value>) -> ExtensionBlockingOrigin {
    match origin {
        Some(Value::String(text)) if text.trim().to_lowercase() == ExtensionBlockingOrigin::Policy.as_str() => ExtensionBlockingOrigin::Policy,
        _ => ExtensionBlockingOrigin::Service,
    }
}

/// Casa um token inteiro: `unblocked` não casa `blocked`.
fn has_extension_flag(flags: Option<&str>, flag: &str) -> bool {
    flags.is_some_and(|flags| flags.split(',').any(|f| f.trim().to_lowercase() == flag))
}

fn executes_code(version: &RawGalleryExtensionVersion) -> Option<bool> {
    property(version, PROPERTY_EXECUTES_CODE).map(|value| value == "true")
}

fn target_platform_of(version: &RawGalleryExtensionVersion) -> TargetPlatform {
    match &version.target_platform {
        Some(platform) if !platform.is_empty() => to_target_platform(platform),
        _ => TargetPlatform::Undefined,
    }
}

fn get_all_target_platforms(raw: &RawGalleryExtension) -> Vec<TargetPlatform> {
    let mut all: Vec<TargetPlatform> = Vec::new();
    for version in &raw.versions {
        let platform = target_platform_of(version);
        if !all.contains(&platform) {
            all.push(platform);
        }
    }
    let is_web_extension = raw.tags.as_ref().is_some_and(|tags| tags.iter().any(|t| t == WEB_EXTENSION_TAG));
    let web_index = all.iter().position(|p| *p == TargetPlatform::Web);
    if is_web_extension {
        if web_index.is_none() {
            all.push(TargetPlatform::Web);
        }
    } else if let Some(index) = web_index {
        all.remove(index);
    }
    all
}

/// `sortExtensionVersions`: o Marketplace já ordena por versão; aqui só se põe a plataforma preferida na frente das
/// demais da mesma versão.
pub fn sort_extension_versions(versions: &mut Vec<RawGalleryExtensionVersion>, preferred_target_platform: TargetPlatform) {
    let mut index = 0;
    while index < versions.len() {
        if index > 0 && versions[index].version == versions[index - 1].version {
            let mut insertion_index = index;
            // put it at the beginning
            if target_platform_of(&versions[index]) == preferred_target_platform {
                while insertion_index > 0 && versions[insertion_index - 1].version == versions[index].version {
                    insertion_index -= 1;
                }
            }
            if insertion_index != index {
                let version = versions.remove(index);
                versions.insert(insertion_index, version);
            }
        }
        index += 1;
    }
}

/// `filterLatestExtensionVersionsForTargetPlatform`: de uma lista ordenada por versão decrescente, devolve as versões de
/// outras plataformas, no máximo um lançamento e um pré-lançamento compatíveis (o mais novo de cada), trocando o universal
/// pelo específico da plataforma quando têm a mesma versão.
pub fn filter_latest_extension_versions_for_target_platform(
    versions: &[RawGalleryExtensionVersion],
    target_platform: TargetPlatform,
    all_target_platforms: &[TargetPlatform],
) -> Vec<RawGalleryExtensionVersion> {
    let mut latest: Vec<RawGalleryExtensionVersion> = Vec::new();
    let mut pre_release_index: Option<usize> = None;
    let mut release_index: Option<usize> = None;
    for version in versions {
        let version_platform = target_platform_of(version);
        if !is_target_platform_compatible(version_platform, all_target_platforms, target_platform) {
            latest.push(version.clone());
            continue;
        }
        let slot = if is_pre_release_version(version) { &mut pre_release_index } else { &mut release_index };
        match *slot {
            None => {
                *slot = Some(latest.len());
                latest.push(version.clone());
            }
            Some(index) => {
                if version_platform == target_platform && latest[index].version == version.version {
                    latest[index] = version.clone();
                }
            }
        }
    }
    latest
}

fn js_date(text: &str) -> f64 {
    parse_date_ms(text).unwrap_or(f64::NAN)
}

/// `toExtension`.
fn to_extension(raw: &RawGalleryExtension, version: &RawGalleryExtensionVersion, all_target_platforms: &[TargetPlatform], manifest: &GalleryManifest, product: &GalleryProduct) -> GalleryExtension {
    let latest_version = &raw.versions[0];
    let assets = GalleryExtensionAssets {
        manifest: get_version_asset(version, ASSET_MANIFEST),
        readme: get_version_asset(version, ASSET_DETAILS),
        changelog: get_version_asset(version, ASSET_CHANGELOG),
        license: get_version_asset(version, ASSET_LICENSE),
        repository: get_repository_asset(version),
        download: get_download_asset(version),
        icon: get_version_asset(version, ASSET_ICON),
        signature: get_version_asset(version, ASSET_SIGNATURE),
        core_translations: get_core_translation_assets(version),
    };
    let publisher_name = raw.publisher.publisher_name.as_str();
    let extension_name = raw.extension_name.as_str();
    let details_view = manifest.resource_uri(raw.link_type.as_deref().unwrap_or(resource_type::EXTENSION_DETAILS_VIEW_URI));
    let publisher_view = manifest.resource_uri(raw.publisher.link_type.as_deref().unwrap_or(resource_type::PUBLISHER_VIEW_URI));
    let rating_view = manifest.resource_uri(raw.rating_link_type.as_deref().unwrap_or(resource_type::EXTENSION_RATING_VIEW_URI));
    let id = get_gallery_extension_id(Some(publisher_name), extension_name);
    let signed = assets.signature.is_some();
    GalleryExtension {
        identifier: ExtensionIdentifierInfo::with_uuid(id.clone(), raw.extension_id.clone()),
        name: raw.extension_name.clone(),
        version: version.version.clone(),
        display_name: raw.display_name.clone(),
        publisher_id: raw.publisher.publisher_id.clone(),
        publisher: raw.publisher.publisher_name.clone(),
        publisher_display_name: raw.publisher.display_name.clone(),
        publisher_domain: raw.publisher.domain.as_ref().filter(|d| !d.is_empty()).map(|d| (d.clone(), raw.publisher.is_domain_verified == Some(true))),
        publisher_link: publisher_view.map(|u| format_template(u, &[("publisher", publisher_name)])),
        publisher_sponsor_link: property(latest_version, PROPERTY_SPONSOR_LINK).map(str::to_owned),
        description: raw.short_description.clone().unwrap_or_default(),
        install_count: get_statistic(&raw.statistics, "install"),
        rating: get_statistic(&raw.statistics, "averagerating"),
        rating_count: get_statistic(&raw.statistics, "ratingcount"),
        categories: raw.categories.clone().unwrap_or_default(),
        tags: raw.tags.clone().unwrap_or_default(),
        release_date: js_date(&raw.release_date),
        last_updated: js_date(&raw.last_updated),
        preview: raw.flags.contains("preview"),
        private: is_private_extension(latest_version),
        blocking_info: get_blocking_info(&raw.flags, raw.blocking.as_ref(), version.flags.as_deref(), version.blocking.as_ref()),
        has_pre_release_version: product.extension_properties.get(&id.to_lowercase()).and_then(|p| p.has_prerelease_version).unwrap_or_else(|| is_pre_release_version(latest_version)),
        has_release_version: true,
        is_signed: signed,
        all_target_platforms: all_target_platforms.to_vec(),
        properties: GalleryExtensionProperties {
            dependencies: Some(get_extensions_list(version, PROPERTY_DEPENDENCY)),
            extension_pack: Some(get_extensions_list(version, PROPERTY_EXTENSION_PACK)),
            engine: Some(get_engine(version)),
            enabled_api_proposals: Some(get_comma_list(version, PROPERTY_ENABLED_API_PROPOSALS)),
            localized_languages: Some(get_comma_list(version, PROPERTY_LOCALIZED_LANGUAGES)),
            target_platform: target_platform_of(version),
            is_pre_release_version: is_pre_release_version(version),
            executes_code: executes_code(version),
        },
        assets,
        support_link: property(latest_version, PROPERTY_SUPPORT_LINK).map(str::to_owned),
        details_link: details_view.map(|u| format_template(u, &[("publisher", publisher_name), ("name", extension_name)])),
        rating_link: rating_view.map(|u| format_template(u, &[("publisher", publisher_name), ("name", extension_name)])),
    }
}

// ---- serviço ----

/// `extensionProperties[id]` do `product.json`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionPropertiesInfo {
    pub has_prerelease_version: Option<bool>,
    pub exclude_version_range: Option<String>,
}

/// O que o serviço lê do `IProductService`.
#[derive(Clone, Debug, Default)]
pub struct GalleryProduct {
    pub version: String,
    pub date: Option<String>,
    pub name_short: String,
    /// `extensionProperties`, com o id em minúscula.
    pub extension_properties: HashMap<String, ExtensionPropertiesInfo>,
    /// `defaultChatAgent.extensionId`: sempre fica por último na lista de uma consulta.
    pub default_chat_agent_extension_id: Option<String>,
    /// `extensionsGallery.extensionUrlTemplate`: o fallback do "latest" (`unpkg`).
    pub unpkg_resource_api: Option<String>,
    /// `extensionsGallery.controlUrl`.
    pub extensions_control_url: Option<String>,
}

impl GalleryProduct {
    /// Lê o que importa do `product.json`.
    pub fn from_product_json(product: &Value) -> Self {
        let gallery = product.get("extensionsGallery");
        let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
        let extension_properties = product
            .get("extensionProperties")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .map(|(id, info)| {
                        (
                            id.to_lowercase(),
                            ExtensionPropertiesInfo {
                                has_prerelease_version: info.get("hasPrereleaseVersion").and_then(Value::as_bool),
                                exclude_version_range: text(info.get("excludeVersionRange")),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            version: text(product.get("version")).unwrap_or_default(),
            date: text(product.get("date")),
            name_short: text(product.get("nameShort")).unwrap_or_default(),
            extension_properties,
            default_chat_agent_extension_id: text(product.pointer("/defaultChatAgent/extensionId")),
            unpkg_resource_api: text(gallery.and_then(|g| g.get("extensionUrlTemplate"))),
            extensions_control_url: text(gallery.and_then(|g| g.get("controlUrl"))),
        }
    }
}

/// O resultado de uma consulta paginada (`IPager`): a primeira página, o total e a função de página.
pub struct GalleryPage {
    pub first_page: Vec<GalleryExtension>,
    pub total: u32,
    pub page_size: u32,
}

/// `ExtensionGalleryService`.
pub struct ExtensionGalleryService {
    manifest: Option<GalleryManifest>,
    http: Rc<dyn HttpClient>,
    allowed: Rc<AllowedExtensionsService>,
    logger: Rc<dyn Logger>,
    product: GalleryProduct,
    target_platform: TargetPlatform,
    common_headers: Vec<(String, String)>,
    request_timeout: Option<Duration>,
}

/// O resultado de uma chamada ao marketplace.
pub type GalleryResult<T> = Result<T, ExtensionGalleryError>;

fn failed(message: impl Into<String>) -> ExtensionGalleryError {
    ExtensionGalleryError { message: message.into(), code: ExtensionGalleryErrorCode::Failed }
}

fn from_http_error(error: &crate::http::HttpError) -> ExtensionGalleryError {
    let code = match error.kind {
        HttpErrorKind::Offline => ExtensionGalleryErrorCode::Offline,
        HttpErrorKind::Timeout => ExtensionGalleryErrorCode::Timeout,
        HttpErrorKind::Other => ExtensionGalleryErrorCode::Failed,
    };
    ExtensionGalleryError { message: error.message.clone(), code }
}

/// A interface pública do serviço do marketplace (`IExtensionGalleryService`): o gerenciamento de extensões depende
/// dela, não da implementação, para os testes poderem substituí-la.
pub trait GalleryApi {
    /// `isEnabled`: há um serviço de galeria configurado.
    fn is_enabled(&self) -> bool;
    /// `getExtensions`.
    fn get_extensions(&self, extension_infos: &[ExtensionInfo], options: &ExtensionQueryOptions) -> GalleryResult<Vec<GalleryExtension>>;
    /// `isExtensionCompatible`.
    fn is_extension_compatible(&self, extension: &GalleryExtension, include_pre_release: bool, target_platform: TargetPlatform, product_version: Option<&ProductVersion>) -> bool;
    /// `getCompatibleExtension`.
    fn get_compatible_extension( &self, extension: &GalleryExtension, include_pre_release: bool, target_platform: TargetPlatform, product_version: Option<&ProductVersion>, ) -> GalleryResult<Option<GalleryExtension>>;
    /// `query`: a primeira página de uma busca por texto (`category:`, `tag:` e `featured` viram filtros).
    fn query(&self, options: &QueryOptions) -> GalleryResult<GalleryPage>;
    /// A página `page_index` (a partir de 0) da mesma consulta.
    fn query_page(&self, options: &QueryOptions, page_index: u32) -> GalleryResult<Vec<GalleryExtension>>;
    /// `download`: baixa o VSIX para `location` (um caminho de URI). `operation` acrescenta `install=true` ou `update=true`.
    fn download(&self, extension: &GalleryExtension, location: &str, operation: InstallOperation, fs: &dyn ExtFs) -> GalleryResult<()>;
    /// `downloadSignatureArchive`.
    fn download_signature_archive(&self, extension: &GalleryExtension, location: &str, fs: &dyn ExtFs) -> GalleryResult<()>;
    /// `reportStatistic`: erros são ignorados.
    fn report_statistic(&self, publisher: &str, name: &str, version: &str, statistic: StatisticType);
    /// `getReadme`.
    fn get_readme(&self, extension: &GalleryExtension) -> GalleryResult<String>;
    /// `getChangelog`.
    fn get_changelog(&self, extension: &GalleryExtension) -> GalleryResult<String>;
    /// `getManifest`.
    fn get_manifest(&self, extension: &GalleryExtension) -> GalleryResult<Option<Value>>;
    /// `getCoreTranslation`.
    fn get_core_translation(&self, extension: &GalleryExtension, language_id: &str) -> GalleryResult<Option<Value>>;
    /// `getAllVersions`.
    fn get_all_versions(&self, identifier: &ExtensionIdentifierInfo) -> GalleryResult<Vec<GalleryExtensionVersion>>;
    /// `getAllCompatibleVersions`.
    fn get_all_compatible_versions(&self, identifier: &ExtensionIdentifierInfo, include_pre_release: bool, target_platform: TargetPlatform) -> GalleryResult<Vec<GalleryExtensionVersion>>;
    /// `getExtensionsControlManifest`: a lista de extensões maliciosas, descontinuadas e as preferências de busca.
    fn get_extensions_control_manifest(&self) -> GalleryResult<ExtensionsControlManifest>;
}

/// O que o construtor de [`ExtensionGalleryService`] precisa, agrupado.
pub struct ExtensionGalleryServiceParts {
    pub manifest: Option<GalleryManifest>,
    pub http: Rc<dyn HttpClient>,
    pub allowed: Rc<AllowedExtensionsService>,
    pub logger: Rc<dyn Logger>,
    pub product: GalleryProduct,
    pub target_platform: TargetPlatform,
    /// `resolveMarketplaceHeaders`: ver [`crate::marketplace`].
    pub common_headers: Vec<(String, String)>,
    pub request_timeout: Option<Duration>,
}

impl ExtensionGalleryService {
    pub fn new(parts: ExtensionGalleryServiceParts) -> Self {
        let ExtensionGalleryServiceParts { manifest, http, allowed, logger, product, target_platform, common_headers, request_timeout } = parts;
        Self { manifest, http, allowed, logger, product, target_platform, common_headers, request_timeout }
    }


    fn manifest(&self) -> GalleryResult<&GalleryManifest> {
        self.manifest.as_ref().ok_or_else(|| failed("No extension gallery service configured."))
    }


    fn product_version(&self) -> ProductVersion {
        ProductVersion { version: self.product.version.clone(), date: self.product.date.clone() }
    }


    fn get_extensions_using_query_api(&self, extension_infos: &[ExtensionInfo], options: &ExtensionQueryOptions, manifest: &GalleryManifest) -> GalleryResult<Vec<GalleryExtension>> {
        let mut names: Vec<String> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        let mut include_pre_release: Vec<(ExtensionIdentifierInfo, bool)> = Vec::new();
        let mut versions: Vec<(ExtensionIdentifierInfo, String)> = Vec::new();
        let mut is_query_for_release_version_from_pre_release_version = true;
        for info in extension_infos {
            let identifier = match &info.uuid {
                Some(uuid) => ExtensionIdentifierInfo::with_uuid(info.id.clone(), uuid.clone()),
                None => ExtensionIdentifierInfo::new(info.id.clone()),
            };
            match &info.uuid {
                Some(uuid) => ids.push(uuid.clone()),
                None => names.push(info.id.clone()),
            }
            match &info.version {
                Some(version) if !version.is_empty() => versions.push((identifier, version.clone())),
                _ => include_pre_release.push((identifier, info.pre_release == Some(true))),
            }
            is_query_for_release_version_from_pre_release_version = is_query_for_release_version_from_pre_release_version && (info.has_pre_release == Some(true) && info.pre_release != Some(true));
        }
        if ids.is_empty() && names.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = Query::new().with_page(1, Some(u32::try_from(extension_infos.len()).unwrap_or(u32::MAX)));
        if !ids.is_empty() {
            query = query.with_filter(FilterType::ExtensionId, &ids);
        }
        if !names.is_empty() {
            query = query.with_filter(FilterType::ExtensionName, &names);
        }
        if options.query_all_versions {
            query = query.add_flags(&[Flag::IncludeVersions]);
        }
        let (extensions, _total) = self.query_gallery_extensions(
            query,
            &ExtensionsCriteria {
                target_platform: options.target_platform.unwrap_or(self.target_platform),
                include_pre_release: IncludePreRelease::PerExtension(include_pre_release),
                versions,
                compatible: options.compatible,
                product_version: options.product_version.clone().unwrap_or_else(|| self.product_version()),
                include_blocked_versions: options.include_blocked_versions,
                is_query_for_release_version_from_pre_release_version,
            },
            manifest,
        )?;
        Ok(extensions)
    }


    fn get_extensions_using_resource_api(
        &self,
        extension_infos: &[ExtensionInfo],
        options: &ExtensionQueryOptions,
        resource_uri: &str,
        manifest: &GalleryManifest,
    ) -> GalleryResult<Vec<GalleryExtension>> {
        let mut result: Vec<GalleryExtension> = Vec::new();
        let mut to_query: Vec<ExtensionInfo> = Vec::new();
        let mut to_fetch_latest: Vec<&ExtensionInfo> = Vec::new();
        for info in extension_infos {
            if !is_extension_identifier(&info.id) {
                continue;
            }
            if info.version.as_ref().is_some_and(|v| !v.is_empty()) || info.uuid.is_none() {
                to_query.push(info.clone());
            } else {
                to_fetch_latest.push(info);
            }
        }
        for info in to_fetch_latest {
            match self.get_latest_gallery_extension(info, options, resource_uri, manifest) {
                Ok(LatestOutcome::Found(extension)) => result.push(*extension),
                Ok(LatestOutcome::Outdated) => {
                    self.logger.debug(&format!(
                        "Skipping query API fallback for extension {} because the latest gallery version is older than the current version",
                        info.id
                    ));
                }
                Ok(LatestOutcome::Fallback(_reason)) => to_query.push(info.clone()),
                Err(error) => {
                    if matches!(error.code, ExtensionGalleryErrorCode::Offline | ExtensionGalleryErrorCode::Cancelled | ExtensionGalleryErrorCode::Timeout) {
                        return Err(error);
                    }
                    self.logger.error(&format!("Error while getting the latest version for the extension {}. {}", info.id, error.message));
                    to_query.push(info.clone());
                }
            }
        }
        if !to_query.is_empty() {
            result.extend(self.get_extensions_using_query_api(&to_query, options, manifest)?);
        }
        Ok(result)
    }


    fn get_latest_gallery_extension(&self, info: &ExtensionInfo, options: &ExtensionQueryOptions, resource_uri: &str, manifest: &GalleryManifest) -> GalleryResult<LatestOutcome> {
        let Some(raw) = self.get_latest_raw_gallery_extension_with_fallback(info, resource_uri)? else {
            return Ok(LatestOutcome::Fallback("NOT_FOUND"));
        };
        if raw.versions.is_empty() || raw.versions.iter().any(|v| v.files.is_none()) {
            return Ok(LatestOutcome::Fallback("INVALID_RESPONSE"));
        }
        let all_target_platforms = get_all_target_platforms(&raw);
        let version = self.get_valid_raw_gallery_extension_version_from_latest_versions(&raw, &raw.versions.clone(), info, options, &all_target_platforms)?;
        let Some(version) = version else {
            if let Some(current) = &info.current_version {
                let latest = raw.versions.first().map(|v| v.version.as_str());
                if latest.is_some_and(|latest| semver::lt(latest, current)) {
                    return Ok(LatestOutcome::Outdated);
                }
            }
            return Ok(LatestOutcome::Fallback("NOT_COMPATIBLE"));
        };
        Ok(LatestOutcome::Found(Box::new(to_extension(&raw, &version, &all_target_platforms, manifest, &self.product))))
    }


    fn get_valid_raw_gallery_extension_version_from_latest_versions(
        &self,
        raw: &RawGalleryExtension,
        latest_versions: &[RawGalleryExtensionVersion],
        info: &ExtensionInfo,
        options: &ExtensionQueryOptions,
        all_target_platforms: &[TargetPlatform],
    ) -> GalleryResult<Option<RawGalleryExtensionVersion>> {
        let target_platform = options.target_platform.unwrap_or(self.target_platform);
        let for_platform = filter_latest_extension_versions_for_target_platform(latest_versions, target_platform, all_target_platforms);
        let product_version = options.product_version.clone().unwrap_or_else(|| self.product_version());
        let pick = |kind: VersionCriterion, compatible: bool| {
            self.get_valid_raw_gallery_extension_version(
                raw,
                for_platform.clone(),
                &ExtensionVersionCriteria { target_platform, compatible, product_version: product_version.clone(), version: kind, include_blocked_versions: false },
                all_target_platforms,
            )
        };
        let pre_release = info.pre_release == Some(true);
        let result = pick(if pre_release { VersionCriterion::Prerelease } else { VersionCriterion::Release }, options.compatible);
        if !pre_release {
            return Ok(result);
        }
        let prerelease_version = result;
        let release_version = pick(VersionCriterion::Release, options.compatible);
        if let (Some(pre), Some(release)) = (&prerelease_version, &release_version) {
            return Ok(Some(if semver::gt(&release.version, &pre.version) { release.clone() } else { pre.clone() }));
        }
        if options.compatible {
            if let Some(release) = &release_version {
                let any_prerelease = pick(VersionCriterion::Prerelease, false);
                if any_prerelease.as_ref().is_none_or(|any| semver::gt(&release.version, &any.version)) {
                    return Ok(release_version);
                }
            }
            return Ok(prerelease_version);
        }
        Ok(prerelease_version.or(release_version))
    }


    fn get_latest_raw_gallery_extension_with_fallback(&self, info: &ExtensionInfo, resource_uri: &str) -> GalleryResult<Option<RawGalleryExtension>> {
        let mut parts = info.id.split('.');
        let (publisher, name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let values = [("publisher", publisher), ("name", name)];
        match self.get_latest_raw_gallery_extension(&format_template(resource_uri, &values)) {
            Ok(result) => return Ok(result),
            Err(error) => {
                if matches!(error.code, ExtensionGalleryErrorCode::Offline | ExtensionGalleryErrorCode::Cancelled | ExtensionGalleryErrorCode::Timeout | ExtensionGalleryErrorCode::ClientError) {
                    return Err(error);
                }
                let Some(fallback) = &self.product.unpkg_resource_api else { return Err(error) };
                self.logger.error(&format!(
                    "Error while getting the latest version for the extension {} from {resource_uri}. Trying the fallback {fallback} {:?}",
                    info.id, error.code
                ));
            }
        }
        let fallback = self.product.unpkg_resource_api.as_deref().unwrap_or_default();
        self.get_latest_raw_gallery_extension(&format_template(fallback, &values))
    }


    fn get_latest_raw_gallery_extension(&self, url: &str) -> GalleryResult<Option<RawGalleryExtension>> {
        let request = HttpRequest::get(url)
            .with_headers(&self.common_headers)
            .with_header("Content-Type", "application/json")
            .with_header("Accept", "application/json;api-version=7.2-preview")
            .with_header("Accept-Encoding", "gzip")
            .with_timeout(self.request_timeout);
        let response = self.http.request(&request).map_err(|e| from_http_error(&e))?;
        let status_error = |response: &HttpResponse| {
            let code = if response.is_client_error() {
                ExtensionGalleryErrorCode::ClientError
            } else if response.is_server_error() {
                ExtensionGalleryErrorCode::ServerError
            } else {
                ExtensionGalleryErrorCode::Failed
            };
            ExtensionGalleryError { message: format!("Unexpected HTTP response: {}", response.status), code }
        };
        if response.status == 404 {
            return Ok(None);
        }
        if response.status != 200 {
            return Err(status_error(&response));
        }
        response.json::<RawGalleryExtension>().map_err(|e| failed(e.to_string()))
    }


    /// `queryRawGalleryExtensions`.
    fn query_raw_gallery_extensions(&self, query: Query, manifest: &GalleryManifest) -> GalleryResult<(Vec<RawGalleryExtension>, u32)> {
        let Some(query_api) = manifest.resource_uri(resource_type::EXTENSION_QUERY_SERVICE) else {
            return Err(failed("No extension gallery query service configured."));
        };
        // Always exclude non validated extensions
        let mut query = query.clone().add_flags(&[Flag::ExcludeNonValidated]).with_filter(FilterType::Target, &["Microsoft.VisualStudio.Code".to_owned()]);
        // Always exclude unpublished extensions
        if let Some(unpublished) = manifest.flag_value(Flag::Unpublished.as_str()) {
            query = query.with_filter(FilterType::ExcludeWithFlags, &[unpublished.to_string()]);
        }
        let criteria: Vec<Value> = query
            .criteria
            .iter()
            .filter_map(|c| {
                let filter_type = manifest.filter_value(c.filter_type.as_str())?;
                let mut criterium = Map::new();
                criterium.insert("filterType".into(), json!(filter_type));
                if let Some(value) = &c.value {
                    criterium.insert("value".into(), json!(value));
                }
                Some(Value::Object(criterium))
            })
            .collect();
        let mut filter = Map::new();
        filter.insert("criteria".into(), Value::Array(criteria));
        filter.insert("pageNumber".into(), json!(query.page_number));
        filter.insert("pageSize".into(), json!(query.page_size));
        if let Some(sort_by) = manifest.sort_value(query.sort_by.as_str()) {
            filter.insert("sortBy".into(), json!(sort_by));
        }
        filter.insert("sortOrder".into(), json!(query.sort_order));
        let flags = query.flags.iter().filter_map(|f| manifest.flag_value(f.as_str())).fold(0i64, |acc, value| acc | value);
        let body = json!({ "filters": [Value::Object(filter)], "assetTypes": query.asset_types, "flags": flags }).to_string();

        let request = HttpRequest::post(query_api, body.clone().into_bytes())
            .with_headers(&self.common_headers)
            .with_header("Content-Type", "application/json")
            .with_header("Accept", "application/json;api-version=3.0-preview.1")
            .with_header("Accept-Encoding", "gzip")
            .with_header("Content-Length", &body.chars().count().to_string());
        let response = self.http.request(&request).map_err(|e| from_http_error(&e))?;
        if response.status >= 400 && response.status < 500 {
            return Ok((Vec::new(), 0));
        }
        let Some(result) = response.json::<RawQueryResult>().map_err(|e| failed(e.to_string()))? else { return Ok((Vec::new(), 0)) };
        let Some(first) = result.results.into_iter().next() else { return Ok((Vec::new(), 0)) };
        let total = first
            .result_metadata
            .iter()
            .find(|m| m.metadata_type == "ResultCount")
            .and_then(|m| m.metadata_items.iter().find(|i| i.name == "TotalCount"))
            .map_or(0, |i| i.count as u32);
        Ok((first.extensions, total))
    }


    /// `queryGalleryExtensions`.
    fn query_gallery_extensions(&self, query: Query, criteria: &ExtensionsCriteria, manifest: &GalleryManifest) -> GalleryResult<(Vec<GalleryExtension>, u32)> {
        let flags = query.flags.clone();
        let mut query = query;
        // If both version flags (IncludeLatestVersionOnly and IncludeVersions) are included, then only include latest versions.
        if query.flags.contains(&Flag::IncludeLatestVersionOnly) && query.flags.contains(&Flag::IncludeVersions) {
            let kept: Vec<Flag> = query.flags.iter().copied().filter(|f| *f != Flag::IncludeVersions).collect();
            query = query.with_flags(&kept);
        }
        // If version flags are not included, default is to query for latest versions.
        if !query.flags.contains(&Flag::IncludeLatestVersionOnly) && !query.flags.contains(&Flag::IncludeVersions) {
            query = query.add_flags(&[Flag::IncludeLatestVersionOnly]);
        }
        // If versions criteria exist or every requested extension is for release version and has a pre-release version,
        // then remove latest flags and add all versions flag.
        if !criteria.versions.is_empty() || criteria.is_query_for_release_version_from_pre_release_version {
            let mut kept: Vec<Flag> = query.flags.iter().copied().filter(|f| *f != Flag::IncludeLatestVersionOnly).collect();
            kept.push(Flag::IncludeVersions);
            query = query.with_flags(&kept);
        }
        // Add necessary extension flags
        query = query.add_flags(&[Flag::IncludeAssetUri, Flag::IncludeCategoryAndTags, Flag::IncludeFiles, Flag::IncludeStatistics, Flag::IncludeVersionProperties]);

        let (raw_extensions, total) = self.query_raw_gallery_extensions(query.clone(), manifest)?;
        let has_all_versions = !query.flags.contains(&Flag::IncludeLatestVersionOnly);

        let identifier_of = |raw: &RawGalleryExtension| {
            ExtensionIdentifierInfo::with_uuid(get_gallery_extension_id(Some(&raw.publisher.publisher_name), &raw.extension_name), raw.extension_id.clone())
        };
        let include_pre_release_for = |identifier: &ExtensionIdentifierInfo| match &criteria.include_pre_release {
            IncludePreRelease::All(value) => *value,
            IncludePreRelease::PerExtension(list) => list.iter().any(|(candidate, include)| *include && are_same_extensions(candidate, identifier)),
        };
        let criteria_for = |identifier: &ExtensionIdentifierInfo| ExtensionVersionCriteria {
            compatible: criteria.compatible,
            target_platform: criteria.target_platform,
            product_version: criteria.product_version.clone(),
            version: criteria
                .versions
                .iter()
                .find(|(candidate, _)| are_same_extensions(candidate, identifier))
                .map(|(_, version)| VersionCriterion::Exact(version.clone()))
                .unwrap_or(if include_pre_release_for(identifier) { VersionCriterion::Latest } else { VersionCriterion::Release }),
            include_blocked_versions: criteria.include_blocked_versions,
        }
        ;

        if has_all_versions {
            let mut extensions = Vec::new();
            for raw in &raw_extensions {
                let all_target_platforms = get_all_target_platforms(raw);
                let identifier = identifier_of(raw);
                let version_criteria = criteria_for(&identifier);
                if let Some(version) = self.get_valid_raw_gallery_extension_version(raw, raw.versions.clone(), &version_criteria, &all_target_platforms) {
                    extensions.push(to_extension(raw, &version, &all_target_platforms, manifest, &self.product));
                }
            }
            return Ok((extensions, total));
        }

        let mut result: Vec<(usize, GalleryExtension)> = Vec::new();
        let mut need_all_versions: Vec<(String, usize)> = Vec::new();
        for (index, raw) in raw_extensions.iter().enumerate() {
            let identifier = identifier_of(raw);
            let include_pre_release = include_pre_release_for(&identifier);
            let all_target_platforms = get_all_target_platforms(raw);
            if criteria.compatible {
                if is_not_web_extension_in_web_target_platform(&all_target_platforms, criteria.target_platform) {
                    continue;
                }
                let candidate = AllowedCandidate::Fields {
                    id: &identifier.id,
                    publisher_display_name: Some(&raw.publisher.display_name),
                    version: None,
                    prerelease: None,
                    target_platform: None,
                };
                if self.allowed.is_allowed(&candidate).is_err() {
                    continue;
                }
            }
            let version_criteria = criteria_for(&identifier);
            let version = self.get_valid_raw_gallery_extension_version(raw, raw.versions.clone(), &version_criteria, &all_target_platforms);
            let extension = version.map(|v| to_extension(raw, &v, &all_target_platforms, manifest, &self.product));
            let needs_all = match &extension {
                None => true,
                // Need all versions if the extension is a pre-release version but the query is to look for a release
                // version or the extension has no release version.
                Some(e) if e.properties.is_pre_release_version && (!include_pre_release || !e.has_release_version) => true,
                // Need all versions if the extension is a release version with a different target platform than requested
                // and also has a pre-release version: it is a platform specific extension and can have a newer release
                // version supporting this platform (vscode#139628).
                Some(e) if !e.properties.is_pre_release_version && e.properties.target_platform != criteria.target_platform && e.has_pre_release_version => true,
                Some(_) => false,
            };
            if needs_all {
                need_all_versions.push((raw.extension_id.clone(), index));
            } else if let Some(extension) = extension {
                result.push((index, extension));
            }
        }
        if !need_all_versions.is_empty() {
            let mut kept: Vec<Flag> = flags.iter().copied().filter(|f| *f != Flag::IncludeLatestVersionOnly).collect();
            kept.push(Flag::IncludeVersions);
            let ids: Vec<String> = need_all_versions.iter().map(|(id, _)| id.clone()).collect();
            let all_versions_query = Query::new().with_flags(&kept).with_page(1, Some(u32::try_from(ids.len()).unwrap_or(u32::MAX))).with_filter(FilterType::ExtensionId, &ids);
            let (extensions, _) = self.query_gallery_extensions(all_versions_query, criteria, manifest)?;
            for extension in extensions {
                if let Some((_, index)) = need_all_versions.iter().find(|(id, _)| Some(id) == extension.identifier.uuid.as_ref()) {
                    result.push((*index, extension));
                }
            }
        }
        result.sort_by_key(|(index, _)| *index);
        Ok((result.into_iter().map(|(_, e)| e).collect(), total))
    }


    /// `getValidRawGalleryExtensionVersion`.
    fn get_valid_raw_gallery_extension_version(
        &self,
        raw: &RawGalleryExtension,
        mut versions: Vec<RawGalleryExtensionVersion>,
        criteria: &ExtensionVersionCriteria,
        all_target_platforms: &[TargetPlatform],
    ) -> Option<RawGalleryExtensionVersion> {
        let extension_id = get_gallery_extension_id(Some(&raw.publisher.publisher_name), &raw.extension_name);
        sort_extension_versions(&mut versions, criteria.target_platform);
        if criteria.compatible && is_not_web_extension_in_web_target_platform(all_target_platforms, criteria.target_platform) {
            return None;
        }
        let exact = match &criteria.version {
            VersionCriterion::Exact(version) => Some(version.clone()),
            _ => None,
        };
        for version in &mut versions {
            if criteria.compatible {
                self.set_engine_if_not_exists(&extension_id, version);
            }
            let version = &*version;
            let candidate = CandidateVersion {
                id: extension_id.clone(),
                version: version.version.clone(),
                is_pre_release_version: is_pre_release_version(version),
                target_platform: target_platform_of(version),
                engine: get_engine(version),
                manifest_asset: get_version_asset(version, ASSET_MANIFEST),
                blocked: is_blocked(version.flags.as_deref(), version.blocking.as_ref()),
            };
            if self.is_valid_version(&candidate, criteria.compatible, &criteria.product_version, Some(criteria.target_platform), &criteria.version, &raw.publisher.display_name, all_target_platforms) {
                return Some(version.clone());
            }
            if let Some(exact) = &exact
                && &version.version == exact
            {
                if criteria.include_blocked_versions && is_blocked(version.flags.as_deref(), version.blocking.as_ref()) {
                    return Some(version.clone());
                }
                return None;
            }
        }
        if exact.is_some() || criteria.compatible {
            return None;
        }
        // Fallback: Return the latest version. This can happen when the extension does not have a release version or does
        // not have a version compatible with the given target platform.
        versions.into_iter().find(|v| !is_blocked(v.flags.as_deref(), v.blocking.as_ref()))
    }


    fn set_engine_if_not_exists(&self, extension_id: &str, version: &mut RawGalleryExtensionVersion) {
        if !get_engine(version).is_empty() {
            return;
        }
        if let Some(engine) = self.get_engine_from_manifest(extension_id, &version.version, get_version_asset(version, ASSET_MANIFEST).as_ref()) {
            set_engine(version, &engine);
        }
    }


    /// `getEngine`: o `engines.vscode` do manifesto, quando a versão não traz a propriedade.
    fn get_engine_from_manifest(&self, extension_id: &str, version: &str, manifest_asset: Option<&GalleryExtensionAsset>) -> Option<String> {
        let Some(asset) = manifest_asset else {
            self.logger.error(&format!("Missing engine and manifest asset for the extension {extension_id} with version {version}"));
            return None;
        };
        let response = match self.get_asset(asset, &[("Accept-Encoding".to_owned(), "gzip".to_owned())]) {
            Ok(response) => response,
            Err(error) => {
                self.logger.error(&format!("Error while getting the engine for the version {version}. {error}"));
                return None;
            }
        };
        match response.json::<Value>() {
            Ok(Some(manifest)) => manifest.pointer("/engines/vscode").and_then(Value::as_str).map(str::to_owned),
            _ => {
                self.logger.error(&format!("Manifest was not found for the extension {extension_id} with version {version}"));
                None
            }
        }
    }


    #[allow(clippy::too_many_arguments)]
    fn is_valid_version(
        &self,
        extension: &CandidateVersion,
        compatible: bool,
        product_version: &ProductVersion,
        target_platform: Option<TargetPlatform>,
        version: &VersionCriterion,
        publisher_display_name: &str,
        all_target_platforms: &[TargetPlatform],
    ) -> bool {
        let properties = self.product.extension_properties.get(&extension.id.to_lowercase());
        let has_pre_release = properties.and_then(|p| p.has_prerelease_version);
        let exclude_range = properties.and_then(|p| p.exclude_version_range.as_deref());
        if extension.blocked {
            return false;
        }
        if extension.is_pre_release_version && has_pre_release == Some(false) {
            return false;
        }
        if exclude_range.is_some_and(|range| semver::satisfies(&extension.version, range)) {
            return false;
        }
        match version {
            VersionCriterion::Exact(exact) if &extension.version != exact => return false,
            VersionCriterion::Release if extension.is_pre_release_version => return false,
            VersionCriterion::Prerelease if !extension.is_pre_release_version => return false,
            _ => {}
        }
        if let Some(target_platform) = target_platform
            && !is_target_platform_compatible(extension.target_platform, all_target_platforms, target_platform)
        {
            return false;
        }
        if compatible {
            let candidate = AllowedCandidate::Fields {
                id: &extension.id,
                publisher_display_name: Some(publisher_display_name),
                version: Some(&extension.version),
                prerelease: Some(extension.is_pre_release_version),
                target_platform: Some(extension.target_platform),
            };
            if self.allowed.is_allowed(&candidate).is_err() {
                return false;
            }
            if !self.is_engine_valid(&extension.id, &extension.version, &extension.engine, extension.manifest_asset.as_ref(), product_version) {
                return false;
            }
        }
        true
    }


    fn is_engine_valid(&self, extension_id: &str, version: &str, engine: &str, manifest_asset: Option<&GalleryExtensionAsset>, product_version: &ProductVersion) -> bool {
        let engine = if engine.is_empty() { self.get_engine_from_manifest(extension_id, version, manifest_asset).unwrap_or_default() } else { engine.to_owned() };
        if engine.is_empty() {
            self.logger.error(&format!("Missing engine for the extension {extension_id} with version {version}"));
            return false;
        }
        is_engine_valid(&engine, &product_version.version, product_version.date.as_deref())
    }


    fn build_text_query(&self, options: &QueryOptions, manifest: &GalleryManifest, page_size: u32) -> (Query, String) {
        let mut text = options.text.clone().unwrap_or_default();
        let mut query = Query::new().with_page(1, Some(page_size));
        if !text.is_empty() {
            let (rest, categories) = extract_prefixed(&text, "category:");
            for category in categories {
                query = query.with_filter(FilterType::Category, &[category]);
            }
            let (rest, tags) = extract_prefixed(&rest, "tag:");
            for tag in tags {
                query = query.with_filter(FilterType::Tag, &[tag]);
            }
            let (rest, featured) = extract_featured(&rest);
            if featured {
                query = query.with_filter(FilterType::Featured, &[]);
            }
            text = rest.trim().to_owned();
            if !text.is_empty() {
                let truncated: String = if text.encode_utf16().count() < 200 { text.clone() } else { String::from_utf16_lossy(&text.encode_utf16().take(200).collect::<Vec<_>>()) };
                query = query.with_filter(FilterType::SearchText, &[truncated]);
            }
            if manifest.sort_value(SortBy::NoneOrRelevance.as_str()).is_some() {
                query = query.with_sort_by(SortBy::NoneOrRelevance);
            }
        } else if manifest.sort_value(SortBy::InstallCount.as_str()).is_some() {
            query = query.with_sort_by(SortBy::InstallCount);
        }
        if let Some(sort_by) = options.sort_by
            && manifest.sort_value(sort_by.as_str()).is_some()
        {
            query = query.with_sort_by(sort_by);
        }
        if let Some(sort_order) = options.sort_order {
            query = query.with_sort_order(sort_order as u32);
        }
        (query, text)
    }


    fn run_query(&self, query: Query, options: &QueryOptions, manifest: &GalleryManifest) -> GalleryResult<GalleryPage> {
        let page_size = query.page_size;
        let (extensions, total) = self.query_gallery_extensions(
            query,
            &ExtensionsCriteria {
                target_platform: self.target_platform,
                compatible: false,
                include_pre_release: IncludePreRelease::All(options.include_pre_release),
                product_version: options.product_version.clone().unwrap_or_else(|| self.product_version()),
                versions: Vec::new(),
                include_blocked_versions: false,
                is_query_for_release_version_from_pre_release_version: false,
            },
            manifest,
        )?;
        let mut result = Vec::new();
        let mut default_chat_agent: Option<GalleryExtension> = None;
        for extension in extensions {
            let is_chat_agent = self.product.default_chat_agent_extension_id.as_ref().is_some_and(|id| are_same_extensions(&extension.identifier, &ExtensionIdentifierInfo::new(id.clone())));
            if is_chat_agent {
                default_chat_agent = Some(extension);
            } else {
                result.push(extension);
            }
        }
        result.extend(default_chat_agent);
        Ok(GalleryPage { first_page: result, total, page_size })
    }


    /// `getAsset`: a URL principal e, se falhar, a de reserva.
    fn get_asset(&self, asset: &GalleryExtensionAsset, extra_headers: &[(String, String)]) -> GalleryResult<HttpResponse> {
        let build = |url: &str| HttpRequest::get(url).with_headers(&self.common_headers).with_headers(extra_headers).with_timeout(self.request_timeout);
        let first: Result<HttpResponse, String> = match self.http.request(&build(&asset.uri)) {
            Ok(response) if response.status == 200 => return Ok(response),
            Ok(response) => Err(format!("Expected 200, got back {} instead.\n\n{}", response.status, response.text())),
            Err(error) => Err(error.message),
        };
        self.logger.debug(&format!("Asset request failed, trying the fallback: {}", first.err().unwrap_or_default()));
        self.http.request(&build(&asset.fallback_uri)).map_err(|e| from_http_error(&e))
    }


    fn get_text_asset(&self, asset: Option<&GalleryExtensionAsset>) -> GalleryResult<Option<String>> {
        match asset {
            Some(asset) => Ok(Some(self.get_asset(asset, &[])?.text())),
            None => Ok(None),
        }
    }


    fn get_versions(&self, identifier: &ExtensionIdentifierInfo, only_compatible: Option<(VersionCriterion, TargetPlatform)>) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        let manifest = self.manifest()?.clone();
        let mut query = Query::new().with_flags(&[Flag::IncludeVersions, Flag::IncludeCategoryAndTags, Flag::IncludeFiles, Flag::IncludeVersionProperties]).with_page(1, Some(1));
        query = match &identifier.uuid {
            Some(uuid) => query.with_filter(FilterType::ExtensionId, std::slice::from_ref(uuid)),
            None => query.with_filter(FilterType::ExtensionName, std::slice::from_ref(&identifier.id)),
        };
        let (raws, _) = self.query_raw_gallery_extensions(query, &manifest)?;
        let Some(raw) = raws.first() else { return Ok(Vec::new()) };
        let all_target_platforms = get_all_target_platforms(raw);
        if let Some((_, platform)) = &only_compatible
            && is_not_web_extension_in_web_target_platform(&all_target_platforms, *platform)
        {
            return Ok(Vec::new());
        }
        let product_version = self.product_version();
        let mut versions: Vec<RawGalleryExtensionVersion> = Vec::new();
        for version in &raw.versions {
            let candidate = CandidateVersion {
                id: identifier.id.clone(),
                version: version.version.clone(),
                is_pre_release_version: is_pre_release_version(version),
                target_platform: target_platform_of(version),
                engine: get_engine(version),
                manifest_asset: get_version_asset(version, ASSET_MANIFEST),
                blocked: is_blocked(version.flags.as_deref(), version.blocking.as_ref()),
            };
            let criterion = only_compatible.as_ref().map_or(VersionCriterion::Exact(version.version.clone()), |(kind, _)| kind.clone());
            if self.is_valid_version(&candidate, only_compatible.is_some(), &product_version, only_compatible.as_ref().map(|(_, p)| *p), &criterion, &raw.publisher.display_name, &all_target_platforms) {
                versions.push(version.clone());
            }
        }
        sort_extension_versions(&mut versions, only_compatible.as_ref().map_or(self.target_platform, |(_, p)| *p));
        let mut result: Vec<GalleryExtensionVersion> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for version in &versions {
            let platform = target_platform_of(version);
            match seen.get(&version.version) {
                Some(index) => result[*index].target_platforms.push(platform),
                None => {
                    seen.insert(version.version.clone(), result.len());
                    result.push(GalleryExtensionVersion {
                        version: version.version.clone(),
                        date: version.last_updated.clone(),
                        is_pre_release_version: is_pre_release_version(version),
                        target_platforms: vec![platform],
                    });
                }
            }
        }
        Ok(result)
    }
}

impl GalleryApi for ExtensionGalleryService {

    /// `isEnabled`: há um serviço de galeria configurado.
    fn is_enabled(&self) -> bool {
        self.manifest.is_some()
    }


    /// `getExtensions`.
    fn get_extensions(&self, extension_infos: &[ExtensionInfo], options: &ExtensionQueryOptions) -> GalleryResult<Vec<GalleryExtension>> {
        let manifest = self.manifest()?.clone();
        let latest_resource = manifest.resource_uri(resource_type::EXTENSION_LATEST_VERSION_URI).map(str::to_owned);
        let mut result = match latest_resource {
            Some(uri) => self.get_extensions_using_resource_api(extension_infos, options, &uri, &manifest)?,
            None => self.get_extensions_using_query_api(extension_infos, options, &manifest)?,
        };
        let uuids: Vec<Option<String>> = result.iter().map(|r| r.identifier.uuid.clone()).collect();
        let mut by_name: Vec<ExtensionInfo> = Vec::new();
        for info in extension_infos {
            if let Some(uuid) = &info.uuid
                && !uuids.contains(&Some(uuid.clone()))
            {
                by_name.push(ExtensionInfo { uuid: None, ..info.clone() });
            }
        }
        if !by_name.is_empty() {
            result.extend(self.get_extensions_using_query_api(&by_name, options, &manifest)?);
        }
        Ok(result)
    }


    /// `isExtensionCompatible`.
    fn is_extension_compatible(&self, extension: &GalleryExtension, include_pre_release: bool, target_platform: TargetPlatform, product_version: Option<&ProductVersion>) -> bool {
        let candidate = CandidateVersion {
            id: extension.identifier.id.clone(),
            version: extension.version.clone(),
            is_pre_release_version: extension.properties.is_pre_release_version,
            target_platform: extension.properties.target_platform,
            engine: extension.properties.engine.clone().unwrap_or_default(),
            manifest_asset: extension.assets.manifest.clone(),
            blocked: extension.blocking_info.is_some(),
        };
        self.is_valid_version(
            &candidate,
            true,
            &product_version.cloned().unwrap_or_else(|| self.product_version()),
            Some(target_platform),
            &if include_pre_release { VersionCriterion::Latest } else { VersionCriterion::Release },
            &extension.publisher_display_name,
            &extension.all_target_platforms,
        )
    }


    /// `getCompatibleExtension`.
    fn get_compatible_extension(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<&ProductVersion>,
    ) -> GalleryResult<Option<GalleryExtension>> {
        if is_not_web_extension_in_web_target_platform(&extension.all_target_platforms, target_platform) {
            return Ok(None);
        }
        if self.is_extension_compatible(extension, include_pre_release, target_platform, product_version) {
            return Ok(Some(extension.clone()));
        }
        let candidate = AllowedCandidate::Fields {
            id: &extension.identifier.id,
            publisher_display_name: Some(&extension.publisher_display_name),
            version: None,
            prerelease: None,
            target_platform: None,
        };
        if self.allowed.is_allowed(&candidate).is_err() {
            return Ok(None);
        }
        let result = self.get_extensions(
            &[ExtensionInfo {
                id: extension.identifier.id.clone(),
                uuid: extension.identifier.uuid.clone(),
                pre_release: Some(include_pre_release),
                has_pre_release: Some(extension.has_pre_release_version),
                ..ExtensionInfo::default()
            }],
            &ExtensionQueryOptions { compatible: true, product_version: product_version.cloned(), query_all_versions: true, target_platform: Some(target_platform), ..ExtensionQueryOptions::default() },
        )?;
        Ok(result.into_iter().next())
    }


    /// `query`: a primeira página de uma busca por texto (`category:`, `tag:` e `featured` viram filtros).
    fn query(&self, options: &QueryOptions) -> GalleryResult<GalleryPage> {
        let manifest = self.manifest()?.clone();
        let page_size = options.page_size.unwrap_or(50);
        let (query, _) = self.build_text_query(options, &manifest, page_size);
        self.run_query(query, options, &manifest)
    }


    /// A página `page_index` (a partir de 0) da mesma consulta.
    fn query_page(&self, options: &QueryOptions, page_index: u32) -> GalleryResult<Vec<GalleryExtension>> {
        let manifest = self.manifest()?.clone();
        let page_size = options.page_size.unwrap_or(50);
        let (query, _) = self.build_text_query(options, &manifest, page_size);
        Ok(self.run_query(query.with_page(page_index + 1, None), options, &manifest)?.first_page)
    }


    /// `download`: baixa o VSIX para `location` (um caminho de URI). `operation` acrescenta `install=true` ou `update=true`.
    fn download(&self, extension: &GalleryExtension, location: &str, operation: InstallOperation, fs: &dyn ExtFs) -> GalleryResult<()> {
        self.logger.trace(&format!("ExtensionGalleryService#download {}", extension.identifier.id));
        let param = match operation {
            InstallOperation::Install => Some("install"),
            InstallOperation::Update => Some("update"),
            _ => None,
        };
        let with_param = |uri: &str| {
            let separator = if uri.contains('?') { '&' } else { '?' };
            match param {
                Some(param) => format!("{uri}{separator}{param}=true"),
                None => uri.to_owned(),
            }
        };
        let asset = GalleryExtensionAsset { uri: with_param(&extension.assets.download.uri), fallback_uri: with_param(&extension.assets.download.fallback_uri) };
        let response = self.get_asset(&asset, &[])?;
        if !response.is_success() {
            return Err(ExtensionGalleryError { message: format!("Expected 200, got back {} instead.", response.status), code: ExtensionGalleryErrorCode::ServerError });
        }
        if let Err(error) = fs.write(location, &response.body) {
            if let Err(delete_error) = fs.remove(location, false) {
                self.logger.warn(&format!("Error while deleting the file {location} {delete_error}"));
            }
            return Err(ExtensionGalleryError { message: error.to_string(), code: ExtensionGalleryErrorCode::DownloadFailedWriting });
        }
        Ok(())
    }


    /// `downloadSignatureArchive`.
    fn download_signature_archive(&self, extension: &GalleryExtension, location: &str, fs: &dyn ExtFs) -> GalleryResult<()> {
        let Some(signature) = &extension.assets.signature else { return Err(failed("No signature asset found")) };
        let response = self.get_asset(signature, &[])?;
        if let Err(error) = fs.write(location, &response.body) {
            let _ = fs.remove(location, false);
            return Err(ExtensionGalleryError { message: error.to_string(), code: ExtensionGalleryErrorCode::DownloadFailedWriting });
        }
        Ok(())
    }


    /// `reportStatistic`: erros são ignorados.
    fn report_statistic(&self, publisher: &str, name: &str, version: &str, statistic: StatisticType) {
        let Some(manifest) = &self.manifest else { return };
        let Some(resource) = manifest.resource_uri(resource_type::EXTENSION_STATISTICS_URI) else { return };
        let stat = match statistic {
            StatisticType::Install => "install",
            StatisticType::Uninstall => "uninstall",
        };
        let url = format_template(resource, &[("publisher", publisher), ("name", name), ("version", version), ("statTypeName", stat)]);
        let request = HttpRequest::post(url, Vec::new()).with_headers(&self.common_headers).with_header("Accept", "*/*;api-version=4.0-preview.1");
        let _ = self.http.request(&request);
    }


    /// `getReadme`.
    fn get_readme(&self, extension: &GalleryExtension) -> GalleryResult<String> {
        Ok(self.get_text_asset(extension.assets.readme.as_ref())?.unwrap_or_default())
    }


    /// `getChangelog`.
    fn get_changelog(&self, extension: &GalleryExtension) -> GalleryResult<String> {
        Ok(self.get_text_asset(extension.assets.changelog.as_ref())?.unwrap_or_default())
    }


    /// `getManifest`.
    fn get_manifest(&self, extension: &GalleryExtension) -> GalleryResult<Option<Value>> {
        match self.get_text_asset(extension.assets.manifest.as_ref())? {
            Some(text) if !text.is_empty() => serde_json::from_str(&text).map(Some).map_err(|e| failed(e.to_string())),
            _ => Ok(None),
        }
    }


    /// `getCoreTranslation`.
    fn get_core_translation(&self, extension: &GalleryExtension, language_id: &str) -> GalleryResult<Option<Value>> {
        let wanted = language_id.to_uppercase();
        let Some((_, asset)) = extension.assets.core_translations.iter().find(|(language, _)| *language == wanted) else { return Ok(None) };
        let text = self.get_asset(asset, &[])?.text();
        if text.is_empty() { Ok(None) } else { serde_json::from_str(&text).map(Some).map_err(|e| failed(e.to_string())) }
    }


    /// `getAllVersions`.
    fn get_all_versions(&self, identifier: &ExtensionIdentifierInfo) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        self.get_versions(identifier, None)
    }


    /// `getAllCompatibleVersions`.
    fn get_all_compatible_versions(&self, identifier: &ExtensionIdentifierInfo, include_pre_release: bool, target_platform: TargetPlatform) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        self.get_versions(identifier, Some((if include_pre_release { VersionCriterion::Latest } else { VersionCriterion::Release }, target_platform)))
    }


    /// `getExtensionsControlManifest`: a lista de extensões maliciosas, descontinuadas e as preferências de busca.
    fn get_extensions_control_manifest(&self) -> GalleryResult<ExtensionsControlManifest> {
        self.manifest()?;
        let Some(url) = &self.product.extensions_control_url else { return Ok(ExtensionsControlManifest { auto_update: Some(Default::default()), ..ExtensionsControlManifest::default() }) };
        let request = HttpRequest::get(url).with_headers(&self.common_headers).with_timeout(self.request_timeout);
        let response = self.http.request(&request).map_err(|e| from_http_error(&e))?;
        if !response.is_success() {
            return Err(failed(format!("Expected 200, got back {} instead.", response.status)));
        }
        let Some(raw) = response.json::<Value>().map_err(|e| failed(e.to_string()))? else { return Ok(ExtensionsControlManifest::default()) };
        let learn_more = raw.get("learnMoreLinks").and_then(Value::as_object);
        let malicious = raw
            .get("malicious")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|id| MaliciousExtensionInfo {
                        extension_or_publisher: if is_extension_identifier(id) {
                            ExtensionOrPublisher::Extension(ExtensionIdentifierInfo::new(id.to_owned()))
                        } else {
                            ExtensionOrPublisher::Publisher(id.to_owned())
                        },
                        learn_more_link: learn_more.and_then(|links| links.get(id)).and_then(Value::as_str).map(str::to_owned),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut deprecated = indexmap::IndexMap::new();
        if let Some(object) = raw.get("deprecated").and_then(Value::as_object) {
            for (id, value) in object {
                match value {
                    Value::Bool(true) => {
                        deprecated.insert(id.to_lowercase(), crate::management::DeprecationInfo { disallow_install: Some(true), ..Default::default() });
                    }
                    Value::Object(_) => {
                        if let Ok(info) = serde_json::from_value(value.clone()) {
                            deprecated.insert(id.to_lowercase(), info);
                        }
                    }
                    _ => {}
                }
            }
        }
        let search: Vec<SearchPreferredResults> = raw.get("search").and_then(|s| serde_json::from_value(s.clone()).ok()).unwrap_or_default();
        let auto_update = raw.get("autoUpdate").and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect());
        Ok(ExtensionsControlManifest { malicious, deprecated, search, auto_update })
    }
}

enum LatestOutcome {
    Found(Box<GalleryExtension>),
    /// A consulta cai para a API de busca; o texto é o motivo (`NOT_FOUND`, `INVALID_RESPONSE`, `NOT_COMPATIBLE`).
    Fallback(&'static str),
    Outdated,
}

struct CandidateVersion {
    id: String,
    version: String,
    is_pre_release_version: bool,
    target_platform: TargetPlatform,
    engine: String,
    manifest_asset: Option<GalleryExtensionAsset>,
    blocked: bool,
}

/// `text.replace(/\b<prefix>("([^"]*)"|([^"]\S*))(\s+|\b|$)/g, ...)`: tira cada `prefix:valor` (ou `prefix:"valor"`) do texto
/// e devolve o texto restante e os valores.
fn extract_prefixed(text: &str, prefix: &str) -> (String, Vec<String>) {
    let mut values = Vec::new();
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let prefix_chars: Vec<char> = prefix.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let at_boundary = index == 0 || !is_word_char(chars[index - 1]);
        if at_boundary && chars[index..].starts_with(&prefix_chars) {
            let mut cursor = index + prefix_chars.len();
            let value: Option<String> = if chars.get(cursor) == Some(&'"') {
                chars[cursor + 1..].iter().position(|c| *c == '"').map(|end| {
                    let value: String = chars[cursor + 1..cursor + 1 + end].iter().collect();
                    cursor += end + 2;
                    value
                })
            } else if chars.get(cursor).is_some_and(|c| *c != '"') {
                let end = chars[cursor..].iter().position(|c| c.is_whitespace()).map_or(chars.len(), |p| cursor + p);
                let value: String = chars[cursor..end].iter().collect();
                cursor = end;
                Some(value)
            } else {
                None
            };
            if let Some(value) = value {
                // `(\s+|\b|$)`: consome o espaço que se segue.
                while chars.get(cursor).is_some_and(|c| c.is_whitespace()) {
                    cursor += 1;
                }
                values.push(value);
                index = cursor;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    (out, values)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `text.replace(/\bfeatured(\s+|\b|$)/g, ...)`.
fn extract_featured(text: &str) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    let word: Vec<char> = "featured".chars().collect();
    let mut out = String::new();
    let mut found = false;
    let mut index = 0;
    while index < chars.len() {
        let at_boundary = index == 0 || !is_word_char(chars[index - 1]);
        let end = index + word.len();
        if at_boundary && chars[index..].starts_with(&word) && chars.get(end).is_none_or(|c| !is_word_char(*c)) {
            found = true;
            index = end;
            while chars.get(index).is_some_and(|c| c.is_whitespace()) {
                index += 1;
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    (out, found)
}
