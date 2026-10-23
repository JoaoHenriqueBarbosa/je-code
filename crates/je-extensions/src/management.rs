//! Porte dos tipos de `src/vs/platform/extensionManagement/common/extensionManagement.ts` que o scanner, o perfil,
//! o instalador e o marketplace compartilham: plataforma-alvo, `Metadata`, extensão local e do marketplace, opções,
//! erros e os padrões de identificador. As interfaces de serviço (`IExtensionManagementService` e afins) viram as
//! structs dos módulos que as implementam.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use je_base::severity::Severity;

use crate::manifest::{ExtensionManifest, ExtensionType, TargetPlatform};

pub const EXTENSION_IDENTIFIER_PATTERN: &str = "^([a-z0-9A-Z][a-z0-9-A-Z]*)\\.([a-z0-9A-Z][a-z0-9-A-Z]*)$";
pub const EXTENSION_PUBLISHER_IDENTIFIER_PATTERN: &str = "^([a-z0-9A-Z][a-z0-9-A-Z]*)$";
pub const WEB_EXTENSION_TAG: &str = "__web_extension";
pub const LANGUAGE_MODEL_CHAT_PROVIDER_EXTENSION_TAG: &str = "language-models";
pub const EXTENSION_INSTALL_SKIP_WALKTHROUGH_CONTEXT: &str = "skipWalkthrough";
pub const EXTENSION_INSTALL_SKIP_PUBLISHER_TRUST_CONTEXT: &str = "skipPublisherTrust";
pub const EXTENSION_INSTALL_SOURCE_CONTEXT: &str = "extensionInstallSource";
pub const EXTENSION_INSTALL_DEP_PACK_CONTEXT: &str = "dependecyOrPackExtensionInstall";
pub const EXTENSION_INSTALL_CLIENT_TARGET_PLATFORM_CONTEXT: &str = "clientTargetPlatform";
pub const DISABLED_EXTENSIONS_STORAGE_PATH: &str = "extensionsIdentifiers/disabled";
pub const ENABLED_EXTENSIONS_STORAGE_PATH: &str = "extensionsIdentifiers/enabled";
pub const ALLOWED_EXTENSIONS_CONFIG_KEY: &str = "extensions.allowed";
pub const VERIFY_EXTENSION_SIGNATURE_CONFIG_KEY: &str = "extensions.verifySignature";
pub const EXTENSION_REQUESTS_TIMEOUT_CONFIG_KEY: &str = "extensions.requestTimeout";

/// `^([a-z0-9A-Z][a-z0-9-A-Z]*)$` sem o `\.`: um segmento do identificador.
fn is_identifier_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphanumeric()) && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Casa o [`EXTENSION_IDENTIFIER_PATTERN`]: `publisher.name`.
pub fn is_extension_identifier(id: &str) -> bool {
    id.split_once('.').is_some_and(|(publisher, name)| is_identifier_segment(publisher) && is_identifier_segment(name))
}

/// Casa o [`EXTENSION_PUBLISHER_IDENTIFIER_PATTERN`].
pub fn is_extension_publisher_identifier(id: &str) -> bool {
    is_identifier_segment(id)
}

/// `ExtensionInstallSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionInstallSource {
    Command,
    SettingsSync,
}

/// `IProductVersion`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductVersion {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// `TargetPlatformToString`.
pub fn target_platform_to_string(target_platform: TargetPlatform) -> &'static str {
    match target_platform {
        TargetPlatform::Win32X64 => "Windows 64 bit",
        TargetPlatform::Win32Arm64 => "Windows ARM",
        TargetPlatform::LinuxX64 => "Linux 64 bit",
        TargetPlatform::LinuxArm64 => "Linux ARM 64",
        TargetPlatform::LinuxArmhf => "Linux ARM",
        TargetPlatform::AlpineX64 => "Alpine Linux 64 bit",
        TargetPlatform::AlpineArm64 => "Alpine ARM 64",
        TargetPlatform::DarwinX64 => "Mac",
        TargetPlatform::DarwinArm64 => "Mac Silicon",
        TargetPlatform::Web => "Web",
        TargetPlatform::Universal => "universal",
        TargetPlatform::Unknown => "unknown",
        TargetPlatform::Undefined => "undefined",
    }
}

/// `toTargetPlatform`: o que não é reconhecido vira `Unknown` (e `undefined` também, como no original).
pub fn to_target_platform(target_platform: &str) -> TargetPlatform {
    match target_platform {
        "win32-x64" => TargetPlatform::Win32X64,
        "win32-arm64" => TargetPlatform::Win32Arm64,
        "linux-x64" => TargetPlatform::LinuxX64,
        "linux-arm64" => TargetPlatform::LinuxArm64,
        "linux-armhf" => TargetPlatform::LinuxArmhf,
        "alpine-x64" => TargetPlatform::AlpineX64,
        "alpine-arm64" => TargetPlatform::AlpineArm64,
        "darwin-x64" => TargetPlatform::DarwinX64,
        "darwin-arm64" => TargetPlatform::DarwinArm64,
        "web" => TargetPlatform::Web,
        "universal" => TargetPlatform::Universal,
        _ => TargetPlatform::Unknown,
    }
}

/// `matches[4] as TargetPlatform`: o texto do nome da pasta vira a plataforma, e um texto que não é uma
/// plataforma conhecida vira `Unknown` (o original mantém o texto cru; aqui o tipo é fechado).
pub fn to_target_platform_lossy(text: &str) -> TargetPlatform {
    match text {
        "undefined" => TargetPlatform::Undefined,
        other => to_target_platform(other),
    }
}

/// `Platform | 'alpine'` do `getTargetPlatform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPlatform {
    Windows,
    Linux,
    Alpine,
    Mac,
    Web,
}

impl HostPlatform {
    /// A plataforma de compilação atual (sem detectar Alpine; ver [`crate::util::compute_target_platform`]).
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(all(target_arch = "wasm32", target_os = "unknown")) {
            Self::Web
        } else {
            Self::Linux
        }
    }
}

/// O `process.arch` do Node para a arquitetura de compilação.
pub fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "arm",
        "x86" => "ia32",
        other => other,
    }
}

/// `getTargetPlatform`.
pub fn get_target_platform(platform: HostPlatform, arch: Option<&str>) -> TargetPlatform {
    use TargetPlatform::*;
    match (platform, arch) {
        (HostPlatform::Windows, Some("x64")) => Win32X64,
        (HostPlatform::Windows, Some("arm64")) => Win32Arm64,
        (HostPlatform::Linux, Some("x64")) => LinuxX64,
        (HostPlatform::Linux, Some("arm64")) => LinuxArm64,
        (HostPlatform::Linux, Some("arm")) => LinuxArmhf,
        (HostPlatform::Alpine, Some("x64")) => AlpineX64,
        (HostPlatform::Alpine, Some("arm64")) => AlpineArm64,
        (HostPlatform::Mac, Some("x64")) => DarwinX64,
        (HostPlatform::Mac, Some("arm64")) => DarwinArm64,
        (HostPlatform::Web, _) => Web,
        _ => Unknown,
    }
}

/// `isNotWebExtensionInWebTargetPlatform`.
pub fn is_not_web_extension_in_web_target_platform(all_target_platforms: &[TargetPlatform], product_target_platform: TargetPlatform) -> bool {
    // Not a web extension in web target platform
    product_target_platform == TargetPlatform::Web && !all_target_platforms.contains(&TargetPlatform::Web)
}

/// `isTargetPlatformCompatible`.
pub fn is_target_platform_compatible(
    extension_target_platform: TargetPlatform,
    all_target_platforms: &[TargetPlatform],
    product_target_platform: TargetPlatform,
) -> bool {
    // Not compatible when extension is not a web extension in web target platform
    if is_not_web_extension_in_web_target_platform(all_target_platforms, product_target_platform) {
        return false;
    }
    match extension_target_platform {
        // Compatible when extension target platform is not defined or universal
        TargetPlatform::Undefined | TargetPlatform::Universal => true,
        // Not compatible when extension target platform is unknown
        TargetPlatform::Unknown => false,
        // Compatible when extension and product target platforms matches
        other => other == product_target_platform,
    }
}

/// `IGalleryExtensionProperties`.
#[derive(Clone, Debug, PartialEq)]
pub struct GalleryExtensionProperties {
    pub dependencies: Option<Vec<String>>,
    pub extension_pack: Option<Vec<String>>,
    pub engine: Option<String>,
    pub enabled_api_proposals: Option<Vec<String>>,
    pub localized_languages: Option<Vec<String>>,
    pub target_platform: TargetPlatform,
    pub is_pre_release_version: bool,
    pub executes_code: Option<bool>,
}

/// `ExtensionBlockingOrigin`: quem declarou o bloqueio. Um valor desconhecido degrada para `Service`, o mais severo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionBlockingOrigin {
    Service,
    Policy,
}

impl ExtensionBlockingOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Policy => "policy",
        }
    }
}

/// `IExtensionBlockingInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionBlockingInfo {
    pub origin: ExtensionBlockingOrigin,
}

/// `IGalleryExtensionAsset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryExtensionAsset {
    pub uri: String,
    pub fallback_uri: String,
}

/// `IGalleryExtensionAssets`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryExtensionAssets {
    pub manifest: Option<GalleryExtensionAsset>,
    pub readme: Option<GalleryExtensionAsset>,
    pub changelog: Option<GalleryExtensionAsset>,
    pub license: Option<GalleryExtensionAsset>,
    pub repository: Option<GalleryExtensionAsset>,
    pub download: GalleryExtensionAsset,
    pub icon: Option<GalleryExtensionAsset>,
    pub signature: Option<GalleryExtensionAsset>,
    pub core_translations: Vec<(String, GalleryExtensionAsset)>,
}

/// `IGalleryExtensionVersion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryExtensionVersion {
    pub version: String,
    pub date: String,
    pub is_pre_release_version: bool,
    pub target_platforms: Vec<TargetPlatform>,
}

/// `IGalleryExtension`. O `telemetryData` e o `queryContext` do original ficam fora: não há telemetria no je-code.
#[derive(Clone, Debug, PartialEq)]
pub struct GalleryExtension {
    pub name: String,
    /// `identifier.id` e `identifier.uuid`.
    pub identifier: crate::manifest::ExtensionIdentifierInfo,
    pub version: String,
    pub display_name: String,
    pub publisher_id: String,
    pub publisher: String,
    pub publisher_display_name: String,
    pub publisher_domain: Option<(String, bool)>,
    pub publisher_link: Option<String>,
    pub publisher_sponsor_link: Option<String>,
    pub description: String,
    pub install_count: f64,
    pub rating: f64,
    pub rating_count: f64,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub release_date: f64,
    pub last_updated: f64,
    pub preview: bool,
    pub private: bool,
    /// Presente só quando o marketplace (ou uma política) declara a extensão, ou a versão em mãos, bloqueada.
    /// Ausência significa "não se sabe bloqueada", não "sabidamente segura".
    pub blocking_info: Option<ExtensionBlockingInfo>,
    pub has_pre_release_version: bool,
    pub has_release_version: bool,
    pub is_signed: bool,
    pub all_target_platforms: Vec<TargetPlatform>,
    pub assets: GalleryExtensionAssets,
    pub properties: GalleryExtensionProperties,
    pub details_link: Option<String>,
    pub rating_link: Option<String>,
    pub support_link: Option<String>,
}

/// `InstallSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallSource {
    Gallery,
    Vsix,
    Resource,
}

/// `IGalleryMetadata` mais o resto de `Metadata`: tudo opcional. As chaves desconhecidas (de versões mais novas do
/// VS Code) sobrevivem em `extra`, porque o `extensions.json` é compartilhado.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_pre_release_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_platform: Option<TargetPlatform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_application_scoped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_machine_scoped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_builtin: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_system: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_release: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_pre_release_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_timestamp: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InstallSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Metadata {
    /// `{ ...this, ...patch }` do JavaScript: o que o `patch` define sobrescreve, o que ele não define fica.
    pub fn merged_with(&self, patch: &Metadata) -> Metadata {
        macro_rules! pick {
            ($($field:ident),*) => { Metadata { $($field: patch.$field.clone().or_else(|| self.$field.clone()),)* extra: { let mut extra = self.extra.clone(); for (k, v) in &patch.extra { extra.insert(k.clone(), v.clone()); } extra } } };
        }
        pick!(
            id, publisher_id, private, publisher_display_name, is_pre_release_version, target_platform, is_application_scoped, is_machine_scoped,
            is_builtin, is_system, updated, pre_release, has_pre_release_version, installed_timestamp, pinned, source, size
        )
    }
}

/// `IExtension` (e a extensão escaneada) reduzida ao que o gerenciamento usa. Uma `LocalExtension` é a
/// `ILocalExtension` do original.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalExtension {
    pub extension_type: ExtensionType,
    pub is_builtin: bool,
    pub identifier: crate::manifest::ExtensionIdentifierInfo,
    pub manifest: ExtensionManifest,
    /// O caminho da URI da pasta da extensão.
    pub location: String,
    pub target_platform: TargetPlatform,
    pub publisher_display_name: Option<String>,
    pub readme_url: Option<String>,
    pub changelog_url: Option<String>,
    pub is_valid: bool,
    pub validations: Vec<(Severity, String)>,
    pub pre_release: bool,
    pub is_workspace_scoped: bool,
    pub is_machine_scoped: bool,
    pub is_application_scoped: bool,
    pub publisher_id: Option<String>,
    pub installed_timestamp: Option<f64>,
    pub is_pre_release_version: bool,
    pub has_pre_release_version: bool,
    pub private: bool,
    pub updated: bool,
    pub pinned: bool,
    pub force_auto_update: bool,
    pub source: InstallSource,
    pub size: f64,
}

/// `SortBy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    NoneOrRelevance,
    LastUpdatedDate,
    Title,
    PublisherName,
    InstallCount,
    PublishedDate,
    AverageRating,
    WeightedRating,
}

impl SortBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoneOrRelevance => "NoneOrRelevance",
            Self::LastUpdatedDate => "LastUpdatedDate",
            Self::Title => "Title",
            Self::PublisherName => "PublisherName",
            Self::InstallCount => "InstallCount",
            Self::PublishedDate => "PublishedDate",
            Self::AverageRating => "AverageRating",
            Self::WeightedRating => "WeightedRating",
        }
    }
}

/// `SortOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Default = 0,
    Ascending = 1,
    Descending = 2,
}

/// `FilterType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    Category,
    ExtensionId,
    ExtensionName,
    ExcludeWithFlags,
    Featured,
    SearchText,
    Tag,
    Target,
}

impl FilterType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Category => "Category",
            Self::ExtensionId => "ExtensionId",
            Self::ExtensionName => "ExtensionName",
            Self::ExcludeWithFlags => "ExcludeWithFlags",
            Self::Featured => "Featured",
            Self::SearchText => "SearchText",
            Self::Tag => "Tag",
            Self::Target => "Target",
        }
    }
}

/// `IQueryOptions`.
#[derive(Clone, Debug, Default)]
pub struct QueryOptions {
    pub text: Option<String>,
    pub exclude: Option<Vec<String>>,
    pub page_size: Option<u32>,
    pub sort_by: Option<SortBy>,
    pub sort_order: Option<SortOrder>,
    pub source: Option<String>,
    pub include_pre_release: bool,
    pub product_version: Option<ProductVersion>,
}

/// `StatisticType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatisticType {
    Install,
    Uninstall,
}

/// `InstallOperation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallOperation {
    None = 1,
    Install,
    Update,
    Migrate,
}

/// `IExtensionInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionInfo {
    pub id: String,
    pub uuid: Option<String>,
    pub version: Option<String>,
    pub pre_release: Option<bool>,
    pub has_pre_release: Option<bool>,
    pub current_version: Option<String>,
}

/// `IExtensionQueryOptions`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionQueryOptions {
    pub target_platform: Option<TargetPlatform>,
    pub product_version: Option<ProductVersion>,
    pub compatible: bool,
    pub query_all_versions: bool,
    pub include_blocked_versions: bool,
    pub source: Option<String>,
}

/// `IDeprecationInfo`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeprecationInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disallow_install: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_info: Option<String>,
}

/// `ISearchPrefferedResults`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPreferredResults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_results: Option<Vec<String>>,
}

/// O `extensionOrPublisher` de `MaliciousExtensionInfo`: um publisher inteiro ou uma extensão.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensionOrPublisher {
    Publisher(String),
    Extension(crate::manifest::ExtensionIdentifierInfo),
}

/// `MaliciousExtensionInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaliciousExtensionInfo {
    pub extension_or_publisher: ExtensionOrPublisher,
    pub learn_more_link: Option<String>,
}

/// `IExtensionsControlManifest`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionsControlManifest {
    pub malicious: Vec<MaliciousExtensionInfo>,
    pub deprecated: IndexMap<String, DeprecationInfo>,
    pub search: Vec<SearchPreferredResults>,
    pub auto_update: Option<IndexMap<String, String>>,
}

/// `ExtensionGalleryErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionGalleryErrorCode {
    Timeout,
    Cancelled,
    ClientError,
    ServerError,
    Failed,
    DownloadFailedWriting,
    Offline,
}

/// `ExtensionGalleryError`.
#[derive(Clone, Debug)]
pub struct ExtensionGalleryError {
    pub message: String,
    pub code: ExtensionGalleryErrorCode,
}

impl std::fmt::Display for ExtensionGalleryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionGalleryError {}

/// `ExtensionManagementErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionManagementErrorCode {
    NotFound,
    Unsupported,
    Deprecated,
    Malicious,
    Incompatible,
    IncompatibleApi,
    IncompatibleTargetPlatform,
    ReleaseVersionNotFound,
    Invalid,
    Download,
    DownloadSignature,
    DownloadFailedWriting,
    UpdateMetadata,
    Extract,
    Scanning,
    ScanningExtension,
    ReadRemoved,
    UnsetRemoved,
    Delete,
    Rename,
    IntializeDefaultProfile,
    AddToProfile,
    InstalledExtensionNotFound,
    PostInstall,
    CorruptZip,
    IncompleteZip,
    PackageNotSigned,
    SignatureVerificationInternal,
    SignatureVerificationFailed,
    NotAllowed,
    Gallery,
    Cancelled,
    Unknown,
    Internal,
}

/// `ExtensionManagementError`.
#[derive(Clone, Debug)]
pub struct ExtensionManagementError {
    pub message: String,
    pub code: ExtensionManagementErrorCode,
}

impl ExtensionManagementError {
    pub fn new(message: impl Into<String>, code: ExtensionManagementErrorCode) -> Self {
        Self { message: message.into(), code }
    }
}

impl std::fmt::Display for ExtensionManagementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionManagementError {}

/// `toExtensionManagementError`: embrulha um erro qualquer (uma `io::Error`, por exemplo) com o código dado, sem
/// re-embrulhar um que já é da gerência.
pub fn to_extension_management_error(error: &(dyn std::error::Error + 'static), code: ExtensionManagementErrorCode) -> ExtensionManagementError {
    match error.downcast_ref::<ExtensionManagementError>() {
        Some(existing) => existing.clone(),
        None => ExtensionManagementError::new(error.to_string(), code),
    }
}

/// `InstallOptions`.
#[derive(Clone, Debug, Default)]
pub struct InstallOptions {
    pub is_builtin: Option<bool>,
    pub is_workspace_scoped: Option<bool>,
    pub is_machine_scoped: Option<bool>,
    pub is_application_scoped: Option<bool>,
    pub pinned: Option<bool>,
    pub donot_include_pack_and_dependencies: bool,
    pub install_given_version: bool,
    pub pre_release: Option<bool>,
    pub install_pre_release_version: bool,
    pub donot_verify_signature: bool,
    pub operation: Option<InstallOperation>,
    /// O caminho da URI do `extensions.json` do perfil; vazio é o do perfil padrão.
    pub profile_location: Option<String>,
    pub product_version: Option<ProductVersion>,
    pub keep_existing: bool,
}

/// `UninstallOptions`.
#[derive(Clone, Debug, Default)]
pub struct UninstallOptions {
    pub profile_location: Option<String>,
    pub donot_include_pack: bool,
    pub donot_check_dependents: bool,
    pub version_only: bool,
    pub remove: bool,
}

/// `getExtensionBlockedMessage` (de `extensionManagementUtil.ts`): o resumo do bloqueio, por origem.
pub fn get_extension_blocked_message(info: &ExtensionBlockingInfo) -> String {
    match info.origin {
        ExtensionBlockingOrigin::Policy => "This extension is not permitted by your organization's policy.".to_owned(),
        ExtensionBlockingOrigin::Service => "This extension has been blocked by the Extension Marketplace and must not be used.".to_owned(),
    }
}
