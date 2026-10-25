//! O pedaço de `src/vs/platform/product/common/productService.ts` (`IProductService`) de que os serviços
//! portados precisam: o trait [`IProductService`] e uma implementação local ([`ProductService`]).
//!
//! O je-code lê e escreve os MESMOS diretórios do VS Code estável (retrocompatibilidade com o perfil, as
//! extensões e os temas do usuário), então o padrão é o `product.json` do VS Code estável 1.143.1:
//! `nameShort` `Code`, `dataFolderName` `.vscode` e `sharedDataFolderName` `.vscode-shared`.
//!
//! Divergência: o `IProductService` do original é o `IProductConfiguration` inteiro (uma centena de campos,
//! que a IA e o marketplace da Microsoft enchem). O trait expõe só os campos que os serviços portados leem,
//! e cresce quando um serviço novo precisar de mais um. [`je_base::product::IProductConfiguration`], o
//! `product.json` completo, também o implementa.

use std::cell::RefCell;

use je_base::product::{
    ConfigurationSyncStore, ExtensionUntrustedWorkspaceSupport, ExtensionVirtualWorkspaceSupport, ICopilotVersions, IExeBasedExtensionTip, IExtensionRecommendations,
    IExtensionsGallery, IRemoteExtensionTip, IStringDictionary, ITasConfig, IVirtualWorkspaceExtensionTip, StringsOrDictionary,
};
use je_base::product::IConfigBasedExtensionTip as IRawConfigBasedExtensionTip;
use je_instantiation::service_id;

/// `IProductService`, reduzido aos campos que os serviços portados leem.
pub trait IProductService {
    /// `version`.
    fn version(&self) -> &str;
    /// `nameShort`: o nome da pasta de dados do usuário (`~/.config/Code`).
    fn name_short(&self) -> &str;
    /// `nameLong`.
    fn name_long(&self) -> &str;
    /// `applicationName`.
    fn application_name(&self) -> &str;
    /// `urlProtocol`: o esquema dos links que abrem o aplicativo (`vscode://`).
    fn url_protocol(&self) -> &str;
    /// `dataFolderName`: a pasta no home com extensões e `argv.json` (`~/.vscode`).
    fn data_folder_name(&self) -> &str;
    /// `sharedDataFolderName`: a pasta de dados compartilhados entre aplicativos (`~/.vscode-shared`).
    fn shared_data_folder_name(&self) -> &str;
    /// `quality` (`stable`, `insider`), quando o produto declara.
    fn quality(&self) -> Option<&str>;
    /// `updateUrl`: o servidor de atualizações do produto, quando ele declara (lido pelo `AbstractUpdateService`).
    /// Padrão: o produto não declara.
    fn update_url(&self) -> Option<&str> {
        None
    }
    /// `trustedExtensionProtocolHandlers`: as extensões cujas URLs abrem sem confirmação (vazio quando o produto não declara).
    fn trusted_extension_protocol_handlers(&self) -> Vec<String>;
    /// `commit`, quando o produto declara.
    fn commit(&self) -> Option<&str>;
    /// `enableTelemetry`.
    fn enable_telemetry(&self) -> bool;
    /// `aiConfig.ariaKey`, quando o produto declara.
    fn ai_config_aria_key(&self) -> Option<&str>;
    /// `msftInternalDomains`.
    fn msft_internal_domains(&self) -> &[String];
    /// `enabledTelemetryLevels`, quando o produto declara.
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels>;
    /// `trustedExtensionAuthAccess`, quando o produto declara. Devolve uma cópia (e não uma referência)
    /// porque o original lê a propriedade a cada chamada e um produto de teste pode mudá-la entre elas.
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary>;
    /// `trustedMcpAuthAccess`, quando o produto declara. Devolve cópia pelo mesmo motivo de
    /// [`IProductService::trusted_extension_auth_access`].
    fn trusted_mcp_auth_access(&self) -> Option<StringsOrDictionary>;
    /// `extensionKind`: o `ExtensionKind[]` que o produto fixa por identificador de extensão.
    fn extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>>;
    /// `extensionPointExtensionKind`: o `ExtensionKind[]` que o produto fixa por ponto de extensão.
    fn extension_point_extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>>;
    /// `extensionUntrustedWorkspaceSupport`.
    fn extension_untrusted_workspace_support(&self) -> Option<&IStringDictionary<ExtensionUntrustedWorkspaceSupport>>;
    /// `extensionVirtualWorkspacesSupport`.
    fn extension_virtual_workspaces_support(&self) -> Option<&IStringDictionary<ExtensionVirtualWorkspaceSupport>>;
    /// `configurationSync.store`: o servidor do Settings Sync, quando o produto declara.
    fn configuration_sync_store(&self) -> Option<&ConfigurationSyncStore>;
    /// `defaultChatAgent`, só com os dois identificadores de extensão que o `ExtensionEnablementService` lê
    /// (`extensionId` e `chatExtensionId`). Padrão: o produto não declara o agente de chat.
    fn default_chat_agent(&self) -> Option<DefaultChatAgentExtensions> {
        None
    }
    /// `sessionsWindowAllowedExtensions`: as extensões sempre habilitadas na janela de sessões. Padrão: nenhuma.
    fn sessions_window_allowed_extensions(&self) -> Vec<String> {
        Vec::new()
    }
    /// `extensionsGallery`: a galeria de extensões do produto, quando ele declara. Padrão: nenhuma.
    fn extensions_gallery(&self) -> Option<&IExtensionsGallery> {
        None
    }
    /// `embedderIdentifier`: quem embute o workbench na web, quando o produto declara. Padrão: nenhum.
    fn embedder_identifier(&self) -> Option<&str> {
        None
    }
    /// `removeTelemetryMachineId`: se o identificador de máquina da telemetria é suprimido. Padrão: falso.
    fn remove_telemetry_machine_id(&self) -> bool {
        false
    }
    /// `remoteExtensionTips` e `virtualWorkspaceExtensionTips` (só os nomes dos remotos, o que o
    /// `cleanRemoteAuthority` lê). Padrão: o produto não declara nenhum.
    fn remote_authoring_config(&self) -> Option<crate::telemetry::telemetry_utils::RemoteAuthoringConfig> {
        None
    }
    /// `extensionRecommendations`: as extensões que o produto recomenda por evento, quando ele declara. Padrão: nenhuma.
    fn extension_recommendations(&self) -> Option<&IStringDictionary<IExtensionRecommendations>> {
        None
    }
    /// `keymapExtensionTips`: as extensões de mapa de teclas que o produto recomenda. Padrão: nenhuma.
    fn keymap_extension_tips(&self) -> Option<&Vec<String>> {
        None
    }
    /// `languageExtensionTips`: as extensões de idioma que o produto recomenda. Padrão: nenhuma.
    fn language_extension_tips(&self) -> Option<&Vec<String>> {
        None
    }
    /// `webExtensionTips`: as extensões que o produto recomenda na web. Padrão: nenhuma.
    fn web_extension_tips(&self) -> Option<&Vec<String>> {
        None
    }
    /// `remoteExtensionTips`. Padrão: nenhuma.
    fn remote_extension_tips(&self) -> Option<&IStringDictionary<IRemoteExtensionTip>> {
        None
    }
    /// `virtualWorkspaceExtensionTips`. Padrão: nenhuma.
    fn virtual_workspace_extension_tips(&self) -> Option<&IStringDictionary<IVirtualWorkspaceExtensionTip>> {
        None
    }
    /// `configBasedExtensionTips`. Padrão: nenhuma.
    fn config_based_extension_tips(&self) -> Option<&IStringDictionary<IRawConfigBasedExtensionTip>> {
        None
    }
    /// `exeBasedExtensionTips`. Padrão: nenhuma.
    fn exe_based_extension_tips(&self) -> Option<&IStringDictionary<IExeBasedExtensionTip>> {
        None
    }
    /// `copilotVersions`: as versões do runtime e do SDK do Copilot empacotados, quando o produto declara. Padrão: nenhuma.
    fn copilot_versions(&self) -> Option<&ICopilotVersions> {
        None
    }
    /// `builtInExtensionsEnabledWithAutoUpdates`: os identificadores das extensões embutidas que se atualizam também no
    /// canal estável. Padrão: nenhuma.
    fn built_in_extensions_enabled_with_auto_updates(&self) -> Vec<String> {
        Vec::new()
    }
    /// `date`: a data de build do produto (ISO 8601). Padrão: ausente.
    fn date(&self) -> Option<&str> {
        None
    }
    /// `trustedExtensionPublishers`: os publishers (em minúsculas) cujas extensões se atualizam sem a espera da
    /// quarentena. Padrão: nenhum.
    fn trusted_extension_publishers(&self) -> Vec<String> {
        Vec::new()
    }
    /// `extensionKeywords`: as palavras-chave de busca por extensão de arquivo (`ext:py`). Padrão: nenhuma.
    fn extension_keywords(&self) -> Option<&IStringDictionary<Vec<String>>> {
        None
    }
    /// `extensionEnabledApiProposals`: as propostas de API que o produto libera por identificador de extensão (lido pelo
    /// `ExtensionsProposedApi`). Padrão: o produto não declara nenhuma.
    fn extension_enabled_api_proposals(&self) -> Option<&IStringDictionary<Vec<String>>> {
        None
    }
    /// `extensionSyncedKeys`: as chaves de estado que o produto sincroniza por identificador de extensão em minúsculas (lido
    /// pelo `ExtensionStorageService`). Padrão: o produto não declara nenhuma.
    fn extension_synced_keys(&self) -> Option<&IStringDictionary<Vec<String>>> {
        None
    }
    /// `tasConfig`: o endpoint do serviço de experimentos (TAS) e os nomes da telemetria dele, quando o produto declara.
    /// Padrão: o produto não declara.
    fn tas_config(&self) -> Option<&ITasConfig> {
        None
    }
}

/// `IDefaultChatAgent`, reduzido aos identificadores das extensões do agente de chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultChatAgentExtensions {
    /// `extensionId`: a extensão de completions.
    pub extension_id: String,
    /// `chatExtensionId`: a extensão de chat.
    pub chat_extension_id: String,
}

/// `IEnabledTelemetryLevels`: o que o produto permite coletar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnabledTelemetryLevels {
    pub error: bool,
    pub usage: bool,
}

service_id!(pub I_PRODUCT_SERVICE: dyn IProductService = "productService");

/// Uma implementação de [`IProductService`] com os valores do `product.json` do VS Code estável 1.143.1.
#[derive(Clone, Debug, PartialEq)]
pub struct ProductService {
    pub version: String,
    pub name_short: String,
    pub name_long: String,
    pub application_name: String,
    pub url_protocol: String,
    pub data_folder_name: String,
    pub shared_data_folder_name: String,
    pub quality: Option<String>,
    /// `updateUrl`.
    pub update_url: Option<String>,
    pub trusted_extension_protocol_handlers: Vec<String>,
    pub commit: Option<String>,
    pub enable_telemetry: bool,
    pub ai_config_aria_key: Option<String>,
    pub msft_internal_domains: Vec<String>,
    pub enabled_telemetry_levels: Option<EnabledTelemetryLevels>,
    /// Mutável por dentro: o original lê a propriedade a cada chamada, e um produto de teste a reatribui
    /// depois que o serviço que a consome já existe.
    pub trusted_extension_auth_access: RefCell<Option<StringsOrDictionary>>,
    /// Mutável por dentro, pelo mesmo motivo de `trusted_extension_auth_access`.
    pub trusted_mcp_auth_access: RefCell<Option<StringsOrDictionary>>,
    pub extension_kind: Option<IStringDictionary<Vec<String>>>,
    pub extension_point_extension_kind: Option<IStringDictionary<Vec<String>>>,
    pub extension_untrusted_workspace_support: Option<IStringDictionary<ExtensionUntrustedWorkspaceSupport>>,
    pub extension_virtual_workspaces_support: Option<IStringDictionary<ExtensionVirtualWorkspaceSupport>>,
    pub configuration_sync_store: Option<ConfigurationSyncStore>,
    pub default_chat_agent: Option<DefaultChatAgentExtensions>,
    pub sessions_window_allowed_extensions: Vec<String>,
    pub extensions_gallery: Option<IExtensionsGallery>,
    /// `copilotVersions`: as versões do runtime e do SDK do Copilot empacotados.
    pub copilot_versions: Option<ICopilotVersions>,
    /// `date`.
    pub date: Option<String>,
    /// `trustedExtensionPublishers`.
    pub trusted_extension_publishers: Vec<String>,
    /// `extensionKeywords`.
    pub extension_keywords: Option<IStringDictionary<Vec<String>>>,
    /// `extensionRecommendations`.
    pub extension_recommendations: Option<IStringDictionary<IExtensionRecommendations>>,
    /// `keymapExtensionTips`.
    pub keymap_extension_tips: Option<Vec<String>>,
    /// `languageExtensionTips`.
    pub language_extension_tips: Option<Vec<String>>,
    /// `webExtensionTips`.
    pub web_extension_tips: Option<Vec<String>>,
    /// `remoteExtensionTips`.
    pub remote_extension_tips: Option<IStringDictionary<IRemoteExtensionTip>>,
    /// `virtualWorkspaceExtensionTips`.
    pub virtual_workspace_extension_tips: Option<IStringDictionary<IVirtualWorkspaceExtensionTip>>,
    /// `configBasedExtensionTips`.
    pub config_based_extension_tips: Option<IStringDictionary<IRawConfigBasedExtensionTip>>,
    /// `exeBasedExtensionTips`.
    pub exe_based_extension_tips: Option<IStringDictionary<IExeBasedExtensionTip>>,
    /// `tasConfig`.
    pub tas_config: Option<ITasConfig>,
}

impl Default for ProductService {
    fn default() -> Self {
        Self {
            version: "1.143.1".to_owned(),
            name_short: "Code".to_owned(),
            name_long: "Visual Studio Code".to_owned(),
            application_name: "code".to_owned(),
            url_protocol: "vscode".to_owned(),
            data_folder_name: ".vscode".to_owned(),
            shared_data_folder_name: ".vscode-shared".to_owned(),
            quality: Some("stable".to_owned()),
            update_url: None,
            trusted_extension_protocol_handlers: Vec::new(),
            commit: None,
            enable_telemetry: true,
            ai_config_aria_key: None,
            msft_internal_domains: Vec::new(),
            enabled_telemetry_levels: Some(EnabledTelemetryLevels { error: true, usage: true }),
            trusted_extension_auth_access: RefCell::new(None),
            trusted_mcp_auth_access: RefCell::new(None),
            extension_kind: None,
            extension_point_extension_kind: None,
            extension_untrusted_workspace_support: None,
            extension_virtual_workspaces_support: None,
            configuration_sync_store: None,
            default_chat_agent: None,
            sessions_window_allowed_extensions: Vec::new(),
            extensions_gallery: None,
            copilot_versions: None,
            date: None,
            trusted_extension_publishers: Vec::new(),
            extension_keywords: None,
            extension_recommendations: None,
            keymap_extension_tips: None,
            language_extension_tips: None,
            web_extension_tips: None,
            remote_extension_tips: None,
            virtual_workspace_extension_tips: None,
            config_based_extension_tips: None,
            exe_based_extension_tips: None,
            tas_config: None,
        }
    }
}

impl ProductService {
    /// `productService.trustedExtensionAuthAccess = value`.
    pub fn set_trusted_extension_auth_access(&self, value: Option<StringsOrDictionary>) {
        *self.trusted_extension_auth_access.borrow_mut() = value;
    }

    /// `productService.trustedMcpAuthAccess = value`.
    pub fn set_trusted_mcp_auth_access(&self, value: Option<StringsOrDictionary>) {
        *self.trusted_mcp_auth_access.borrow_mut() = value;
    }
}

impl IProductService for ProductService {
    fn version(&self) -> &str {
        &self.version
    }
    fn name_short(&self) -> &str {
        &self.name_short
    }
    fn name_long(&self) -> &str {
        &self.name_long
    }
    fn application_name(&self) -> &str {
        &self.application_name
    }
    fn url_protocol(&self) -> &str {
        &self.url_protocol
    }
    fn data_folder_name(&self) -> &str {
        &self.data_folder_name
    }
    fn shared_data_folder_name(&self) -> &str {
        &self.shared_data_folder_name
    }
    fn quality(&self) -> Option<&str> {
        self.quality.as_deref()
    }
    fn update_url(&self) -> Option<&str> {
        self.update_url.as_deref()
    }
    fn trusted_extension_protocol_handlers(&self) -> Vec<String> {
        self.trusted_extension_protocol_handlers.clone()
    }
    fn commit(&self) -> Option<&str> {
        self.commit.as_deref()
    }
    fn enable_telemetry(&self) -> bool {
        self.enable_telemetry
    }
    fn ai_config_aria_key(&self) -> Option<&str> {
        self.ai_config_aria_key.as_deref()
    }
    fn msft_internal_domains(&self) -> &[String] {
        &self.msft_internal_domains
    }
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels> {
        self.enabled_telemetry_levels
    }
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_extension_auth_access.borrow().clone()
    }
    fn trusted_mcp_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_mcp_auth_access.borrow().clone()
    }
    fn extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_kind.as_ref()
    }
    fn extension_point_extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_point_extension_kind.as_ref()
    }
    fn extension_untrusted_workspace_support(&self) -> Option<&IStringDictionary<ExtensionUntrustedWorkspaceSupport>> {
        self.extension_untrusted_workspace_support.as_ref()
    }
    fn extension_virtual_workspaces_support(&self) -> Option<&IStringDictionary<ExtensionVirtualWorkspaceSupport>> {
        self.extension_virtual_workspaces_support.as_ref()
    }
    fn configuration_sync_store(&self) -> Option<&ConfigurationSyncStore> {
        self.configuration_sync_store.as_ref()
    }
    fn default_chat_agent(&self) -> Option<DefaultChatAgentExtensions> {
        self.default_chat_agent.clone()
    }
    fn sessions_window_allowed_extensions(&self) -> Vec<String> {
        self.sessions_window_allowed_extensions.clone()
    }
    fn extensions_gallery(&self) -> Option<&IExtensionsGallery> {
        self.extensions_gallery.as_ref()
    }
    fn copilot_versions(&self) -> Option<&ICopilotVersions> {
        self.copilot_versions.as_ref()
    }
    fn date(&self) -> Option<&str> {
        self.date.as_deref()
    }
    fn trusted_extension_publishers(&self) -> Vec<String> {
        self.trusted_extension_publishers.clone()
    }
    fn extension_keywords(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_keywords.as_ref()
    }
    fn extension_recommendations(&self) -> Option<&IStringDictionary<IExtensionRecommendations>> {
        self.extension_recommendations.as_ref()
    }
    fn keymap_extension_tips(&self) -> Option<&Vec<String>> {
        self.keymap_extension_tips.as_ref()
    }
    fn language_extension_tips(&self) -> Option<&Vec<String>> {
        self.language_extension_tips.as_ref()
    }
    fn web_extension_tips(&self) -> Option<&Vec<String>> {
        self.web_extension_tips.as_ref()
    }
    fn remote_extension_tips(&self) -> Option<&IStringDictionary<IRemoteExtensionTip>> {
        self.remote_extension_tips.as_ref()
    }
    fn virtual_workspace_extension_tips(&self) -> Option<&IStringDictionary<IVirtualWorkspaceExtensionTip>> {
        self.virtual_workspace_extension_tips.as_ref()
    }
    fn config_based_extension_tips(&self) -> Option<&IStringDictionary<IRawConfigBasedExtensionTip>> {
        self.config_based_extension_tips.as_ref()
    }
    fn exe_based_extension_tips(&self) -> Option<&IStringDictionary<IExeBasedExtensionTip>> {
        self.exe_based_extension_tips.as_ref()
    }
    fn tas_config(&self) -> Option<&ITasConfig> {
        self.tas_config.as_ref()
    }
}

impl IProductService for je_base::product::IProductConfiguration {
    fn tas_config(&self) -> Option<&ITasConfig> {
        self.tas_config.as_ref()
    }
    fn keymap_extension_tips(&self) -> Option<&Vec<String>> {
        self.keymap_extension_tips.as_ref()
    }
    fn language_extension_tips(&self) -> Option<&Vec<String>> {
        self.language_extension_tips.as_ref()
    }
    fn web_extension_tips(&self) -> Option<&Vec<String>> {
        self.web_extension_tips.as_ref()
    }
    fn remote_extension_tips(&self) -> Option<&IStringDictionary<IRemoteExtensionTip>> {
        self.remote_extension_tips.as_ref()
    }
    fn virtual_workspace_extension_tips(&self) -> Option<&IStringDictionary<IVirtualWorkspaceExtensionTip>> {
        self.virtual_workspace_extension_tips.as_ref()
    }
    fn config_based_extension_tips(&self) -> Option<&IStringDictionary<IRawConfigBasedExtensionTip>> {
        self.config_based_extension_tips.as_ref()
    }
    fn exe_based_extension_tips(&self) -> Option<&IStringDictionary<IExeBasedExtensionTip>> {
        self.exe_based_extension_tips.as_ref()
    }
    fn embedder_identifier(&self) -> Option<&str> {
        self.embedder_identifier.as_deref()
    }
    fn remove_telemetry_machine_id(&self) -> bool {
        self.remove_telemetry_machine_id.unwrap_or(false)
    }
    fn remote_authoring_config(&self) -> Option<crate::telemetry::telemetry_utils::RemoteAuthoringConfig> {
        if self.remote_extension_tips.is_none() && self.virtual_workspace_extension_tips.is_none() {
            return None;
        }
        Some(crate::telemetry::telemetry_utils::RemoteAuthoringConfig {
            remote_extension_tips: self.remote_extension_tips.as_ref().map(|tips| tips.keys().cloned().collect()),
            virtual_workspace_extension_tips: self.virtual_workspace_extension_tips.as_ref().map(|tips| tips.keys().cloned().collect()),
        })
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn name_short(&self) -> &str {
        &self.name_short
    }
    fn name_long(&self) -> &str {
        &self.name_long
    }
    fn application_name(&self) -> &str {
        &self.application_name
    }
    fn url_protocol(&self) -> &str {
        &self.url_protocol
    }
    fn data_folder_name(&self) -> &str {
        &self.data_folder_name
    }
    fn shared_data_folder_name(&self) -> &str {
        &self.shared_data_folder_name
    }
    fn quality(&self) -> Option<&str> {
        self.quality.as_deref()
    }
    fn update_url(&self) -> Option<&str> {
        self.update_url.as_deref()
    }
    fn trusted_extension_protocol_handlers(&self) -> Vec<String> {
        self.trusted_extension_protocol_handlers.clone().unwrap_or_default()
    }
    fn extension_enabled_api_proposals(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_enabled_api_proposals.as_ref()
    }
    fn commit(&self) -> Option<&str> {
        self.commit.as_deref()
    }
    fn enable_telemetry(&self) -> bool {
        self.enable_telemetry.unwrap_or(false)
    }
    fn ai_config_aria_key(&self) -> Option<&str> {
        self.ai_config.as_ref().map(|config| config.aria_key.as_str())
    }
    fn msft_internal_domains(&self) -> &[String] {
        self.msft_internal_domains.as_deref().unwrap_or(&[])
    }
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels> {
        self.enabled_telemetry_levels.as_ref().map(|levels| EnabledTelemetryLevels { error: levels.error, usage: levels.usage })
    }
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_extension_auth_access.clone()
    }
    fn trusted_mcp_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_mcp_auth_access.clone()
    }
    fn extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_kind.as_ref()
    }
    fn extension_point_extension_kind(&self) -> Option<&IStringDictionary<Vec<String>>> {
        self.extension_point_extension_kind.as_ref()
    }
    fn extension_untrusted_workspace_support(&self) -> Option<&IStringDictionary<ExtensionUntrustedWorkspaceSupport>> {
        self.extension_untrusted_workspace_support.as_ref()
    }
    fn extension_virtual_workspaces_support(&self) -> Option<&IStringDictionary<ExtensionVirtualWorkspaceSupport>> {
        self.extension_virtual_workspaces_support.as_ref()
    }
    fn configuration_sync_store(&self) -> Option<&ConfigurationSyncStore> {
        self.configuration_sync_store.as_ref()
    }
    fn default_chat_agent(&self) -> Option<DefaultChatAgentExtensions> {
        Some(DefaultChatAgentExtensions { extension_id: self.default_chat_agent.extension_id.clone(), chat_extension_id: self.default_chat_agent.chat_extension_id.clone() })
    }
    fn sessions_window_allowed_extensions(&self) -> Vec<String> {
        self.sessions_window_allowed_extensions.clone().unwrap_or_default()
    }
    fn extensions_gallery(&self) -> Option<&IExtensionsGallery> {
        self.extensions_gallery.as_ref()
    }
    fn extension_recommendations(&self) -> Option<&IStringDictionary<IExtensionRecommendations>> {
        self.extension_recommendations.as_ref()
    }
    fn copilot_versions(&self) -> Option<&ICopilotVersions> {
        self.copilot_versions.as_ref()
    }
    fn built_in_extensions_enabled_with_auto_updates(&self) -> Vec<String> {
        self.built_in_extensions_enabled_with_auto_updates.clone()
    }
}
