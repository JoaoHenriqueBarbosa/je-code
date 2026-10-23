//! Porte de `src/vs/platform/extensions/common/extensions.ts`: o manifesto (`IExtensionManifest`) com todos os
//! `contributes`, `ExtensionIdentifier` e seus conjuntos e mapas, `ExtensionType`, `TargetPlatform` e os nomes dos
//! arquivos de cache de manifesto.
//!
//! O manifesto vem de um `package.json` escrito por terceiros: cada campo é lido de forma tolerante (um campo com
//! o tipo errado vira `None` em vez de derrubar o manifesto inteiro; quem acusa o tipo errado é o
//! [`crate::validator`], que trabalha sobre o JSON cru). As chaves que o tipo não conhece ficam em `extra`, para o
//! manifesto sobreviver a uma ida e volta sem perda.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

pub const USER_MANIFEST_CACHE_FILE_PREFIX: &str = "extensions.user";
pub const BUILTIN_MANIFEST_CACHE_FILE_PREFIX: &str = "extensions.builtin";
pub const UNDEFINED_PUBLISHER: &str = "undefined_publisher";
pub const EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY: &str = "extensions.experimental.enableAgentsWindowCapability";

/// `EXTENSION_CATEGORIES`.
pub const EXTENSION_CATEGORIES: [&str; 20] = [
    "AI",
    "Azure",
    "Chat",
    "Data Science",
    "Debuggers",
    "Extension Packs",
    "Education",
    "Formatters",
    "Keymaps",
    "Language Packs",
    "Linters",
    "Machine Learning",
    "Notebooks",
    "Programming Languages",
    "SCM Providers",
    "Snippets",
    "Testing",
    "Themes",
    "Visualization",
    "Other",
];

/// `ExtensionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionType {
    System = 0,
    User = 1,
}

fn manifest_cache_file_prefix(extension_type: ExtensionType) -> &'static str {
    match extension_type {
        ExtensionType::System => BUILTIN_MANIFEST_CACHE_FILE_PREFIX,
        ExtensionType::User => USER_MANIFEST_CACHE_FILE_PREFIX,
    }
}

/// `^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?$`: um idioma como o `platform.language` o fornece (`de`, `zh-cn`).
fn is_scan_language(language: &str) -> bool {
    let bytes = language.as_bytes();
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    !bytes.is_empty()
        && bytes.len() <= 32
        && alnum(bytes[0])
        && alnum(bytes[bytes.len() - 1])
        && bytes.iter().all(|&b| alnum(b) || b == b'-')
}

/// `getManifestCacheFileName`: o nome do arquivo de cache para o tipo e o idioma de varredura. Um idioma malformado
/// usa o arquivo sem sufixo, o que só custa um cache perdido.
pub fn get_manifest_cache_file_name(extension_type: ExtensionType, language: Option<&str>) -> String {
    let prefix = manifest_cache_file_prefix(extension_type);
    // `toLowerCase` do JS: Unicode completo.
    let normalized = language.map(str::to_lowercase);
    match normalized {
        Some(language) if is_scan_language(&language) => format!("{prefix}.{language}.cache"),
        _ => format!("{prefix}.cache"),
    }
}

/// `isManifestCacheFileName`: `ignore_path_casing` é o da pasta que contém o arquivo.
pub fn is_manifest_cache_file_name(name: &str, extension_type: ExtensionType, ignore_path_casing: bool) -> bool {
    let candidate = if ignore_path_casing { name.to_lowercase() } else { name.to_owned() };
    candidate.starts_with(&format!("{}.", manifest_cache_file_prefix(extension_type))) && candidate.ends_with(".cache")
}

/// `parseEnabledApiProposalNames`: tira o `@versão` de cada proposta.
pub fn parse_enabled_api_proposal_names<S: AsRef<str>>(enabled_api_proposals: &[S]) -> Vec<String> {
    enabled_api_proposals.iter().map(|p| p.as_ref().split('@').next().unwrap_or("").to_owned()).collect()
}

/// `TargetPlatform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetPlatform {
    #[serde(rename = "win32-x64")]
    Win32X64,
    #[serde(rename = "win32-arm64")]
    Win32Arm64,
    #[serde(rename = "linux-x64")]
    LinuxX64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
    #[serde(rename = "linux-armhf")]
    LinuxArmhf,
    #[serde(rename = "alpine-x64")]
    AlpineX64,
    #[serde(rename = "alpine-arm64")]
    AlpineArm64,
    #[serde(rename = "darwin-x64")]
    DarwinX64,
    #[serde(rename = "darwin-arm64")]
    DarwinArm64,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "universal")]
    Universal,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "undefined")]
    Undefined,
}

impl TargetPlatform {
    /// O valor de texto do enum do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Win32X64 => "win32-x64",
            Self::Win32Arm64 => "win32-arm64",
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::LinuxArmhf => "linux-armhf",
            Self::AlpineX64 => "alpine-x64",
            Self::AlpineArm64 => "alpine-arm64",
            Self::DarwinX64 => "darwin-x64",
            Self::DarwinArm64 => "darwin-arm64",
            Self::Web => "web",
            Self::Universal => "universal",
            Self::Unknown => "unknown",
            Self::Undefined => "undefined",
        }
    }
}

impl std::fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `ExtensionKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionKind {
    Ui,
    Workspace,
    Web,
}

/// `ALL_EXTENSION_KINDS`.
pub const ALL_EXTENSION_KINDS: [ExtensionKind; 3] = [ExtensionKind::Ui, ExtensionKind::Workspace, ExtensionKind::Web];

/// Lê um campo de forma tolerante: tipo errado vira `None`.
pub(crate) fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

/// Idem, com o valor padrão do tipo no lugar do `None`.
pub(crate) fn lenient_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    Ok(lenient::<D, T>(deserializer)?.unwrap_or_default())
}

/// Declara uma struct de contribuição: todo campo é opcional e tolerante, e as chaves desconhecidas ficam em `extra`.
macro_rules! contribution {
    ($(#[$meta:meta])* pub struct $name:ident { $($(#[$field_meta:meta])* pub $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $(
                $(#[$field_meta])*
                #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
                pub $field: Option<$ty>,
            )*
            #[serde(flatten)]
            pub extra: Map<String, Value>,
        }
    };
}

/// `ILocalizedString | string`: o título de um comando depois da localização vira um objeto.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocalizedOrString {
    Plain(String),
    Localized { value: String, original: String },
}

/// `fileMatch: string | string[]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

contribution! {
    /// `ICommand`.
    pub struct CommandContribution { pub command: String, pub title: LocalizedOrString, pub category: LocalizedOrString }
}
contribution! {
    /// `IDebugger`.
    pub struct DebuggerContribution { pub label: String, pub r#type: String, pub runtime: String }
}
contribution! {
    /// `IGrammar`.
    pub struct GrammarContribution { pub language: String }
}
contribution! {
    /// `IJSONValidation`.
    pub struct JsonValidationContribution { pub file_match: OneOrMany, pub url: String }
}
contribution! {
    /// `IJSONValidationRegistry`.
    pub struct JsonValidationRegistryContribution { pub url: String }
}
contribution! {
    /// `IKeyBinding`.
    pub struct KeyBindingContribution { pub command: String, pub key: String, pub when: String, pub mac: String, pub linux: String, pub win: String }
}
contribution! {
    /// `ILanguage`.
    pub struct LanguageContribution { pub id: String, pub extensions: Vec<String>, pub aliases: Vec<String> }
}
contribution! {
    /// `IMenu`.
    pub struct MenuContribution { pub command: String, pub alt: String, pub when: String, pub group: String }
}
contribution! {
    /// `ISnippet`.
    pub struct SnippetContribution { pub language: String }
}
contribution! {
    /// `ITheme`.
    pub struct ThemeContribution { pub label: String }
}
contribution! {
    /// `IViewContainer`.
    pub struct ViewContainerContribution { pub id: String, pub title: String }
}
contribution! {
    /// `IView`.
    pub struct ViewContribution { pub id: String, pub name: String }
}
contribution! {
    /// O `defaults` de `IColor`.
    pub struct ColorDefaults { pub light: String, pub dark: String, pub high_contrast: String }
}
contribution! {
    /// `IColor`.
    pub struct ColorContribution { pub id: String, pub description: String, pub defaults: ColorDefaults }
}
contribution! {
    /// `ITranslation`.
    pub struct TranslationContribution { pub id: String, pub path: String }
}
contribution! {
    /// `ILocalizationContribution`.
    pub struct LocalizationContribution {
        pub language_id: String,
        pub language_name: String,
        pub localized_language_name: String,
        pub translations: Vec<TranslationContribution>,
        pub minimal_translations: IndexMap<String, String>,
    }
}
contribution! {
    /// O item de `selector` de `IWebviewEditor`.
    pub struct WebviewEditorSelector { pub filename_pattern: String }
}
contribution! {
    /// `IWebviewEditor`.
    pub struct WebviewEditorContribution { pub view_type: String, pub priority: String, pub selector: Vec<WebviewEditorSelector> }
}
contribution! {
    /// `ICodeActionContributionAction`.
    pub struct CodeActionContributionAction { pub kind: String, pub title: String, pub description: String }
}
contribution! {
    /// `ICodeActionContribution`.
    pub struct CodeActionContribution { pub languages: Vec<String>, pub actions: Vec<CodeActionContributionAction> }
}
contribution! {
    /// `IAuthenticationContribution`.
    pub struct AuthenticationContribution { pub id: String, pub label: String, pub authorization_server_globs: Vec<String> }
}
contribution! {
    /// O `doneOn` (obsoleto) de `IWalkthroughStep`.
    pub struct WalkthroughDoneOn { pub command: String }
}
contribution! {
    /// `IWalkthroughStep`. O `media` é uma união de quatro formas (imagem, markdown, svg, vídeo), mantida crua.
    pub struct WalkthroughStep {
        pub id: String,
        pub title: String,
        pub description: String,
        pub media: Value,
        pub completion_events: Vec<String>,
        pub done_on: WalkthroughDoneOn,
        pub when: String,
    }
}
contribution! {
    /// `IWalkthrough`.
    pub struct WalkthroughContribution {
        pub id: String,
        pub title: String,
        pub icon: String,
        pub description: String,
        pub steps: Vec<WalkthroughStep>,
        pub featured_for: Vec<String>,
        pub when: String,
    }
}
contribution! {
    /// `IStartEntry`.
    pub struct StartEntryContribution { pub title: String, pub description: String, pub command: String, pub when: String, pub category: String }
}
contribution! {
    /// `INotebookEntry`.
    pub struct NotebookEntryContribution { pub r#type: String, pub display_name: String }
}
contribution! {
    /// `INotebookRendererContribution`.
    pub struct NotebookRendererContribution { pub id: String, pub display_name: String, pub mime_types: Vec<String> }
}
contribution! {
    /// `IDebugVisualizationContribution`.
    pub struct DebugVisualizationContribution { pub id: String, pub when: String }
}
contribution! {
    /// O item de `commands` de `IChatParticipantContribution`.
    pub struct ChatParticipantCommand { pub name: String }
}
contribution! {
    /// `IChatParticipantContribution`.
    pub struct ChatParticipantContribution {
        pub id: String,
        pub name: String,
        pub full_name: String,
        pub description: String,
        pub is_default: bool,
        pub commands: Vec<ChatParticipantCommand>,
    }
}
contribution! {
    /// `IToolContribution`.
    pub struct ToolContribution { pub name: String, pub display_name: String, pub model_description: String, pub user_description: String }
}
contribution! {
    /// `IToolSetContribution`.
    pub struct ToolSetContribution { pub name: String, pub reference_name: String, pub description: String, pub icon: String, pub tools: Vec<String> }
}
contribution! {
    /// `IMcpCollectionContribution`.
    pub struct McpCollectionContribution { pub id: String, pub label: String, pub when: String }
}
contribution! {
    /// `IChatFileContribution`.
    pub struct ChatFileContribution { pub path: String, pub name: String, pub description: String, pub when: String, pub session_types: Vec<String> }
}

contribution! {
    /// `IExtensionContributions`. `configuration` e `configurationDefaults` são `any` no original.
    pub struct ExtensionContributions {
        pub commands: Vec<CommandContribution>,
        pub configuration: Value,
        pub configuration_defaults: Value,
        pub debuggers: Vec<DebuggerContribution>,
        pub grammars: Vec<GrammarContribution>,
        pub json_validation: Vec<JsonValidationContribution>,
        pub json_validation_registry: Vec<JsonValidationRegistryContribution>,
        pub keybindings: Vec<KeyBindingContribution>,
        pub languages: Vec<LanguageContribution>,
        pub menus: IndexMap<String, Vec<MenuContribution>>,
        pub snippets: Vec<SnippetContribution>,
        pub themes: Vec<ThemeContribution>,
        pub icon_themes: Vec<ThemeContribution>,
        pub product_icon_themes: Vec<ThemeContribution>,
        pub views_containers: IndexMap<String, Vec<ViewContainerContribution>>,
        pub views: IndexMap<String, Vec<ViewContribution>>,
        pub colors: Vec<ColorContribution>,
        pub localizations: Vec<LocalizationContribution>,
        pub custom_editors: Vec<WebviewEditorContribution>,
        pub code_actions: Vec<CodeActionContribution>,
        pub authentication: Vec<AuthenticationContribution>,
        pub walkthroughs: Vec<WalkthroughContribution>,
        pub start_entries: Vec<StartEntryContribution>,
        pub notebooks: Vec<NotebookEntryContribution>,
        pub notebook_renderer: Vec<NotebookRendererContribution>,
        pub debug_visualizers: Vec<DebugVisualizationContribution>,
        pub chat_participants: Vec<ChatParticipantContribution>,
        pub chat_prompt_files: Vec<ChatFileContribution>,
        pub chat_instructions: Vec<ChatFileContribution>,
        pub chat_agents: Vec<ChatFileContribution>,
        pub chat_skills: Vec<ChatFileContribution>,
        pub chat_plugins: Vec<ChatFileContribution>,
        pub language_model_tools: Vec<ToolContribution>,
        pub language_model_tool_sets: Vec<ToolSetContribution>,
        pub mcp_server_definition_providers: Vec<McpCollectionContribution>,
    }
}

contribution! {
    /// `IExtensionCapabilities`. `virtualWorkspaces` e `untrustedWorkspaces` aceitam booleano ou objeto, e ficam crus.
    pub struct ExtensionCapabilities {
        pub virtual_workspaces: Value,
        pub untrusted_workspaces: Value,
        pub agents_window: Value,
    }
}

/// `getWorkspaceSupportTypeMessage`: a descrição quando o suporte não é `true`.
pub fn get_workspace_support_type_message(support_type: Option<&Value>) -> Option<String> {
    let object = support_type?.as_object()?;
    if object.get("supported") != Some(&Value::Bool(true)) {
        return object.get("description").and_then(Value::as_str).map(str::to_owned);
    }
    None
}

contribution! {
    /// `repository` e `bugs`.
    pub struct UrlReference { pub url: String }
}

contribution! {
    /// `engines: { vscode }`.
    pub struct Engines { pub vscode: String }
}

/// `extensionKind: ExtensionKind | ExtensionKind[]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionKindOrList {
    One(ExtensionKind),
    Many(Vec<ExtensionKind>),
}

/// `IExtensionManifest` (`IRelaxedExtensionManifest`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    #[serde(default, deserialize_with = "lenient_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_or_default")]
    pub publisher: String,
    #[serde(default, deserialize_with = "lenient_or_default")]
    pub version: String,
    #[serde(default, deserialize_with = "lenient_or_default")]
    pub engines: Engines,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub preview: Option<bool>,
    /// Por ora só aponta para pacotes de l10n.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub l10n: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub activation_events: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub extension_dependencies: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub extension_affinity: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub extension_pack: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub extension_kind: Option<ExtensionKindOrList>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub contributes: Option<ExtensionContributions>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub repository: Option<UrlReference>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub bugs: Option<UrlReference>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub original_enabled_api_proposals: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub enabled_api_proposals: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub scripts: Option<IndexMap<String, String>>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ExtensionCapabilities>,
    /// Qualquer outra chave do `package.json` (`scripts`, `dependencies`, `__metadata` e afins).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ExtensionManifest {
    /// Lê um manifesto de um JSON já validado como objeto, de forma tolerante.
    pub fn from_value(value: Value) -> ExtensionManifest {
        serde_json::from_value(value).unwrap_or_default()
    }
}

/// `isLanguagePackExtension`.
pub fn is_language_pack_extension(manifest: &ExtensionManifest) -> bool {
    manifest.contributes.as_ref().and_then(|c| c.localizations.as_ref()).is_some_and(|l| !l.is_empty())
}

/// `isApplicationScopedExtension`.
pub fn is_application_scoped_extension(manifest: &ExtensionManifest) -> bool {
    is_language_pack_extension(manifest)
}

/// `isAuthenticationProviderExtension`.
pub fn is_authentication_provider_extension(manifest: &ExtensionManifest) -> bool {
    manifest.contributes.as_ref().and_then(|c| c.authentication.as_ref()).is_some_and(|a| !a.is_empty())
}

/// `isResolverExtension`: o `remote_name` é o `getRemoteName(remoteAuthority)`.
pub fn is_resolver_extension(manifest: &ExtensionManifest, remote_name: Option<&str>) -> bool {
    match remote_name {
        Some(name) => {
            let event = format!("onResolveRemoteAuthority:{name}");
            manifest.activation_events.as_ref().is_some_and(|events| events.contains(&event))
        }
        None => false,
    }
}

/// `IExtensionIdentifier`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExtensionIdentifierInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

impl ExtensionIdentifierInfo {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), uuid: None }
    }

    pub fn with_uuid(id: impl Into<String>, uuid: impl Into<String>) -> Self {
        Self { id: id.into(), uuid: Some(uuid.into()) }
    }
}

/// `ExtensionIdentifier`: a versão canônica do identificador, preservando a caixa original; a igualdade ignora a caixa.
#[derive(Clone, Debug)]
pub struct ExtensionIdentifier {
    pub value: String,
    lower: String,
}

impl ExtensionIdentifier {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let lower = value.to_lowercase();
        Self { value, lower }
    }

    /// `ExtensionIdentifier.equals`, com `None` fazendo o papel de `null`/`undefined`.
    pub fn equals(a: Option<&str>, b: Option<&str>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => crate::js::equals_ignore_case(a, b),
            _ => false,
        }
    }

    /// `ExtensionIdentifier.toKey`: a chave de índice (minúscula).
    pub fn to_key(id: &str) -> String {
        id.to_lowercase()
    }

    pub fn lower(&self) -> &str {
        &self.lower
    }
}

impl PartialEq for ExtensionIdentifier {
    fn eq(&self, other: &Self) -> bool {
        self.lower == other.lower
    }
}

impl Eq for ExtensionIdentifier {}

/// `ExtensionIdentifierSet`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionIdentifierSet {
    set: HashSet<String>,
}

impl ExtensionIdentifierSet {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(iterable: I) -> Self {
        let mut set = Self::default();
        for id in iterable {
            set.add(id.as_ref());
        }
        set
    }

    pub fn size(&self) -> usize {
        self.set.len()
    }

    pub fn add(&mut self, id: &str) {
        self.set.insert(ExtensionIdentifier::to_key(id));
    }

    pub fn delete(&mut self, id: &str) -> bool {
        self.set.remove(&ExtensionIdentifier::to_key(id))
    }

    pub fn has(&self, id: &str) -> bool {
        self.set.contains(&ExtensionIdentifier::to_key(id))
    }
}

/// `ExtensionIdentifierMap`: mantém a ordem de inserção (o `Map` do JS).
#[derive(Clone, Debug)]
pub struct ExtensionIdentifierMap<T> {
    map: IndexMap<String, T>,
}

impl<T> Default for ExtensionIdentifierMap<T> {
    fn default() -> Self {
        Self { map: IndexMap::new() }
    }
}

impl<T> ExtensionIdentifierMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// `delete` do `Map` preserva a ordem dos demais.
    pub fn delete(&mut self, id: &str) {
        self.map.shift_remove(&ExtensionIdentifier::to_key(id));
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.map.get(&ExtensionIdentifier::to_key(id))
    }

    pub fn has(&self, id: &str) -> bool {
        self.map.contains_key(&ExtensionIdentifier::to_key(id))
    }

    /// `set` de uma chave existente mantém a posição original, como o `Map`.
    pub fn set(&mut self, id: &str, value: T) {
        self.map.insert(ExtensionIdentifier::to_key(id), value);
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.map.values()
    }

    pub fn into_values(self) -> impl Iterator<Item = T> {
        self.map.into_values()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.map.iter()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// `ExtensionError`: um erro claramente de uma extensão.
#[derive(Clone, Debug)]
pub struct ExtensionError {
    pub extension: ExtensionIdentifier,
    pub message: String,
}

impl ExtensionError {
    pub fn new(extension: ExtensionIdentifier, cause_message: &str, message: Option<&str>) -> Self {
        let detail = match message {
            Some(m) if !m.is_empty() && !cause_message.is_empty() => format!("{m}: {cause_message}"),
            Some(m) if !m.is_empty() => m.to_owned(),
            _ => cause_message.to_owned(),
        };
        let message = format!("Error in extension {}: {}", ExtensionIdentifier::to_key(&extension.value), detail);
        Self { extension, message }
    }
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionError {}
