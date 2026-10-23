//! Porte de `src/vs/base/common/product.ts`: a `IProductConfiguration` completa, o formato do
//! `product.json`.
//!
//! Todas as interfaces do original são estruturas com os campos de mesmo nome (camelCase no JSON, snake_case
//! aqui), lidas e escritas por `serde`. Dois pontos fazem a leitura ser sem perda:
//!
//! - toda estrutura guarda em `extra` as chaves que não conhece (o `product.json` do VS Code tem dezenas
//!   que a interface não declara, como `win32x64AppId` e `darwinCredits`) e as escreve de volta;
//! - um campo opcional ausente continua ausente na escrita.
//!
//! `IStringDictionary<T>` é um [`IndexMap`] (a ordem das chaves do arquivo se mantém; a dos campos de uma estrutura é a da declaração). Os campos de tipo
//! `unknown` (`IBuiltInExtension.metadata`) são [`serde_json::Value`]. Uma união de objetos distinguida por
//! um campo (`media.type`, `source`) é um `enum` etiquetado; uma união sem etiqueta é um `enum` sem etiqueta,
//! tentado na ordem do original. `trustedExtensionAuthAccess` e `trustedMcpAuthAccess`
//! (`string[] | IStringDictionary<string[]>`) são [`StringsOrDictionary`].

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::policy::ExtensionConfigurationPolicy;

pub use crate::record::{Extra, JsNumber};

/// `IStringDictionary<T>`.
pub type IStringDictionary<T> = IndexMap<String, T>;

/// `PlatformName`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformName {
    Web,
    Windows,
    Mac,
    Linux,
}

/// `string[] | IStringDictionary<string[]>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringsOrDictionary {
    Strings(Vec<String>),
    Dictionary(IStringDictionary<Vec<String>>),
}

/// `boolean | 'limited'`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrLimited {
    Bool(bool),
    Limited(String),
}

record! {
    /// `IBuiltInExtension`.
    pub struct IBuiltInExtension {
        req {
            name: String,
            version: String,
            #[serde(default)]
            metadata: serde_json::Value,
        }
        // `repo` é obrigatório na interface, mas o `product.json` real tem entradas sem ele
        // (`TypeScriptTeam.jsts-chat-features`): aqui é opcional.
        opt { repo: String }
    }
}

/// O `path` de uma mídia `image`: um caminho ou um por tema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImagePath {
    Path(String),
    Themed(ThemedImagePath),
}

record! {
    /// `{ hc: string; hcLight?: string; light: string; dark: string }`.
    pub struct ThemedImagePath {
        req { hc: String, light: String, dark: String }
        opt { hc_light: String }
    }
}

/// `IProductWalkthroughStep['media']`, discriminado por `type`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum IProductWalkthroughMedia {
    #[serde(rename_all = "camelCase")]
    Image { path: ImagePath, alt_text: String },
    #[serde(rename_all = "camelCase")]
    Svg { path: String, alt_text: String },
    #[serde(rename_all = "camelCase")]
    Markdown { path: String },
}

record! {
    /// `IProductWalkthroughStep`.
    pub struct IProductWalkthroughStep {
        req { id: String, title: String, when: String, description: String, media: IProductWalkthroughMedia }
    }
}

record! {
    /// `IProductWalkthrough`.
    pub struct IProductWalkthrough {
        req { id: String, steps: Vec<IProductWalkthroughStep> }
    }
}

record! {
    /// `IFeaturedExtension`.
    pub struct IFeaturedExtension {
        req { id: String, title: String, description: String, image_path: String }
    }
}

record! {
    /// `IChatSessionRecommendation`.
    pub struct IChatSessionRecommendation {
        req { extension_id: String, extension_name: String, display_name: String, name: String, description: String }
        opt { post_install_command: String }
    }
}

record! {
    /// `{ scopes: string[] }`.
    pub struct IAuthenticationProviderScopes {
        req { scopes: Vec<String> }
    }
}

record! {
    /// `ConfigurationSyncStore`.
    pub struct ConfigurationSyncStore {
        req {
            url: String,
            insiders_url: String,
            stable_url: String,
            authentication_providers: IStringDictionary<IAuthenticationProviderScopes>,
        }
        opt { can_switch: bool }
    }
}

record! {
    /// `Omit<ConfigurationSyncStore, 'insidersUrl' | 'stableUrl'>`.
    pub struct EditSessionsStore {
        req { url: String, authentication_providers: IStringDictionary<IAuthenticationProviderScopes> }
        opt { can_switch: bool }
    }
}

record! {
    /// `ExtensionUntrustedWorkspaceSupport`.
    pub struct ExtensionUntrustedWorkspaceSupport {
        opt {
            #[serde(rename = "default")]
            default_value: BoolOrLimited,
            #[serde(rename = "override")]
            override_value: BoolOrLimited,
        }
    }
}

record! {
    /// `ExtensionVirtualWorkspaceSupport`.
    pub struct ExtensionVirtualWorkspaceSupport {
        opt {
            #[serde(rename = "default")]
            default_value: bool,
            #[serde(rename = "override")]
            override_value: bool,
        }
    }
}

record! {
    /// `IAgentSdkProductConfig`: o download de um SDK de agente sob demanda. O `{sdkTarget}` do
    /// `urlTemplate` é trocado pela tripla (plataforma, arquitetura, libc) do host.
    pub struct IAgentSdkProductConfig {
        req { version: String, url_template: String }
    }
}

record! {
    /// `IDictationRuntimeProductConfig`.
    pub struct IDictationRuntimeProductConfig {
        req { version: String, url_template: String }
    }
}

record! {
    /// `IProductConfiguration.win32ContextMenu[arch]`.
    pub struct IWin32ContextMenu {
        req { clsid: String }
    }
}

record! {
    /// `IProductConfiguration.tasConfig`.
    pub struct ITasConfig {
        req { endpoint: String, telemetry_event_name: String, assignment_context_telemetry_property_name: String }
    }
}

record! {
    /// `IProductConfiguration.extensionsGallery`.
    pub struct IExtensionsGallery {
        req { service_url: String, control_url: String, extension_url_template: String, resource_url_template: String, nls_base_url: String }
        opt { access_skus: Vec<String>, access_scopes: Vec<String> }
    }
}

record! {
    /// `IProductConfiguration.copilotVersions`.
    pub struct ICopilotVersions {
        req { runtime: String, sdk: String }
    }
}

record! {
    /// `IProductConfiguration.mcpGallery`.
    pub struct IMcpGallery {
        req {
            service_url: String,
            item_web_url: String,
            publisher_url: String,
            support_url: String,
            privacy_policy_url: String,
            terms_of_service_url: String,
            report_url: String,
        }
    }
}

record! {
    /// `IProductConfiguration.connectorRecommendations[id]`.
    pub struct IConnectorRecommendation {
        req { name: String, description: String, icon_url: String }
    }
}

record! {
    /// `IProductConfiguration.crashReporter`.
    pub struct ICrashReporter {
        req { company_name: String, product_name: String }
    }
}

record! {
    /// `IProductConfiguration.enabledTelemetryLevels`.
    pub struct IEnabledTelemetryLevels {
        req { error: bool, usage: bool }
    }
}

record! {
    /// `IProductConfiguration.aiConfig`.
    pub struct IAiConfig {
        req { aria_key: String }
    }
}

record! {
    /// `IProductConfiguration.parentPolicyConfig`: a identidade de política do VS Code pai quando roda
    /// embutido.
    pub struct IParentPolicyConfig {
        opt { win32_reg_value_name: String, darwin_bundle_identifier: String, url_protocol: String }
    }
}

record! {
    /// `IProductOnboardingKeymap`.
    pub struct IProductOnboardingKeymap {
        req { id: String, label: String, description: String }
        opt { extension_id: String }
    }
}

/// `IProductOnboardingTheme['type']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnboardingThemeType {
    #[serde(rename = "dark")]
    Dark,
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "hcDark")]
    HcDark,
    #[serde(rename = "hcLight")]
    HcLight,
}

record! {
    /// `IProductOnboardingTheme`.
    pub struct IProductOnboardingTheme {
        req { id: String, label: String, theme_id: String, r#type: OnboardingThemeType }
    }
}

record! {
    /// `ITunnelApplicationConfig`.
    pub struct ITunnelApplicationConfig {
        req { authentication_providers: IStringDictionary<IAuthenticationProviderScopes>, editor_web_url: String, extension: IRemoteExtensionTip }
    }
}

record! {
    /// `IRemoteExtensionTip.startEntry`.
    pub struct IRemoteExtensionTipStartEntry {
        req { help_link: String, start_connect_label: String, start_command: String, priority: JsNumber }
    }
}

record! {
    /// `IRemoteExtensionTip`.
    pub struct IRemoteExtensionTip {
        req { friendly_name: String, extension_id: String }
        opt { supported_platforms: Vec<PlatformName>, start_entry: IRemoteExtensionTipStartEntry }
    }
}

record! {
    /// `IVirtualWorkspaceExtensionTip`.
    pub struct IVirtualWorkspaceExtensionTip {
        req { friendly_name: String, extension_id: String, start_entry: IRemoteExtensionTipStartEntry }
        opt { supported_platforms: Vec<PlatformName> }
    }
}

record! {
    /// `ISettingsEditorOpenCondition`.
    pub struct ISettingsEditorOpenCondition {
        opt { prerelease: PrereleaseCondition, description_override: String }
    }
}

/// `boolean | string`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrereleaseCondition {
    Bool(bool),
    Text(String),
}

record! {
    /// `IFileLanguageCondition` (e, com `contentPattern`, `IFileContentCondition`).
    pub struct IFileLanguageCondition {
        req { languages: Vec<String> }
        opt { content_pattern: String, important: bool, when_installed: Vec<String>, when_not_installed: Vec<String> }
    }
}

record! {
    /// `IFilePathCondition` (e, com `contentPattern`, `IFileContentCondition`).
    pub struct IFilePathCondition {
        req { path_glob: String }
        opt { content_pattern: String, important: bool, when_installed: Vec<String>, when_not_installed: Vec<String> }
    }
}

/// `IFileOpenCondition`: uma condição por linguagem ou por caminho (cada uma com `contentPattern`
/// opcional, que é o `IFileContentCondition`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IFileOpenCondition {
    Language(IFileLanguageCondition),
    Path(IFilePathCondition),
}

record! {
    /// `IExtensionRecommendations`.
    pub struct IExtensionRecommendations {
        // `onFileOpen` é obrigatório na interface, mas o `product.json` real tem entradas só com
        // `onSettingsEditorOpen`.
        opt { on_file_open: Vec<IFileOpenCondition>, on_settings_editor_open: ISettingsEditorOpenCondition }
    }
}

record! {
    /// `IAppCenterConfiguration`.
    pub struct IAppCenterConfiguration {
        req {
            #[serde(rename = "win32-x64")]
            win32_x64: String,
            #[serde(rename = "win32-arm64")]
            win32_arm64: String,
            #[serde(rename = "linux-x64")]
            linux_x64: String,
            darwin: String,
            #[serde(rename = "darwin-universal")]
            darwin_universal: String,
            #[serde(rename = "darwin-arm64")]
            darwin_arm64: String,
        }
    }
}

record! {
    /// A dica de `IConfigBasedExtensionTip.recommendations[id]`.
    pub struct IConfigBasedRecommendation {
        req { name: String }
        opt { content_pattern: String, important: bool, is_extension_pack: bool, when_not_installed: Vec<String> }
    }
}

record! {
    /// `IConfigBasedExtensionTip`.
    pub struct IConfigBasedExtensionTip {
        req { config_path: String, config_name: String, recommendations: IStringDictionary<IConfigBasedRecommendation> }
        opt { config_scheme: String }
    }
}

record! {
    /// A dica de `IExeBasedExtensionTip.recommendations[id]`.
    pub struct IExeBasedRecommendation {
        req { name: String }
        opt { important: bool, is_extension_pack: bool, when_not_installed: Vec<String> }
    }
}

record! {
    /// `IExeBasedExtensionTip`.
    pub struct IExeBasedExtensionTip {
        req { friendly_name: String, recommendations: IStringDictionary<IExeBasedRecommendation> }
        opt { windows_path: String, important: bool }
    }
}

record! {
    /// `ISurveyData`.
    pub struct ISurveyData {
        req { survey_id: String, survey_url: String, language_id: String, edit_count: JsNumber, user_probability: JsNumber }
    }
}

record! {
    /// `IAiGeneratedWorkspaceTrust`.
    pub struct IAiGeneratedWorkspaceTrust {
        req { title: String, checkbox_text: String, trust_option: String, dont_trust_option: String, startup_trust_request_learn_more: String }
    }
}

record! {
    /// `{ id: string; name: string }`, um provedor de autenticação do chat.
    pub struct IChatProvider {
        req { id: String, name: String }
    }
}

record! {
    /// `IDefaultChatAgent.provider`.
    pub struct IChatProviders {
        req { default: IChatProvider, enterprise: IChatProvider, google: IChatProvider, apple: IChatProvider, microsoft: IChatProvider }
    }
}

record! {
    /// `IDefaultChatAgent`.
    pub struct IDefaultChatAgent {
        req {
            extension_id: String,
            chat_extension_id: String,
            chat_extension_output_id: String,
            chat_extension_output_extension_state_command: String,
            documentation_url: String,
            skus_documentation_url: String,
            public_code_matches_url: String,
            manage_plan_url: String,
            upgrade_plan_url: String,
            sign_up_url: String,
            terms_statement_url: String,
            privacy_statement_url: String,
            provider: IChatProviders,
            provider_extension_id: String,
            provider_uri_setting: String,
            provider_scopes: Vec<Vec<String>>,
            entitlement_url: String,
            entitlement_signup_limited_url: String,
            token_entitlement_url: String,
            mcp_registry_data_url: String,
            managed_settings_url: String,
            chat_quota_exceeded_context: String,
            completions_quota_exceeded_context: String,
            walkthrough_command: String,
            completions_menu_command: String,
            chat_refresh_token_command: String,
            generate_commit_message_command: String,
            resolve_merge_conflicts_command: String,
            completions_advanced_setting: String,
            completions_enablement_setting: String,
            next_edit_suggestions_setting: String,
        }
        // `optimizeUsageDocumentationUrl` é obrigatório na interface, mas falta no `product.json` do VS Code estável.
        opt { optimize_usage_documentation_url: String, mcp_connectors_url: String }
    }
}

record! {
    /// `IExtensionsEnabledWithApiProposal`-style property: `extensionProperties[id]`.
    pub struct IExtensionProperties {
        opt { has_prerelease_version: bool, exclude_version_range: String }
    }
}

record! {
    /// `IProductConfiguration`: o conteúdo do `product.json`.
    pub struct IProductConfiguration {
        req {
            version: String,
            name_short: String,
            name_long: String,
            application_name: String,
            url_protocol: String,
            /// location for extensions (e.g. ~/.vscode-insiders)
            data_folder_name: String,
            /// location for shared data (e.g. ~/.vscode-insiders-shared)
            shared_data_folder_name: String,
            server_application_name: String,
            extension_properties: IStringDictionary<IExtensionProperties>,
            built_in_extensions_enabled_with_auto_updates: Vec<String>,
            default_chat_agent: IDefaultChatAgent,
        }
        opt {
            date: String,
            quality: String,
            commit: String,
            win32_app_user_model_id: String,
            win32_mutex_name: String,
            win32_setup_mutex_name: String,
            win32_reg_value_name: String,
            win32_name_version: String,
            win32_versioned_update: bool,
            win32_context_menu: IStringDictionary<IWin32ContextMenu>,
            linux_desktop_name: String,
            embedder_identifier: String,
            agents_telemetry_app_name: String,
            built_in_extensions: Vec<IBuiltInExtension>,
            walkthrough_metadata: Vec<IProductWalkthrough>,
            featured_extensions: Vec<IFeaturedExtension>,
            download_url: String,
            update_url: String,
            web_url: String,
            web_endpoint_url_template: String,
            webview_content_external_base_url_template: String,
            target: String,
            nls_core_base_url: String,
            /// Build-time hash of the commit and core NLS tables used to identify translated-message caches.
            nls_metadata_hash: String,
            settings_search_build_id: JsNumber,
            settings_search_url: String,
            tas_config: ITasConfig,
            extensions_gallery: IExtensionsGallery,
            agent_sdks: IStringDictionary<IAgentSdkProductConfig>,
            copilot_versions: ICopilotVersions,
            dictation_runtime: IDictationRuntimeProductConfig,
            mcp_gallery: IMcpGallery,
            extension_publisher_orgs: Vec<String>,
            trusted_extension_publishers: Vec<String>,
            extension_recommendations: IStringDictionary<IExtensionRecommendations>,
            connector_recommendations: IStringDictionary<IConnectorRecommendation>,
            config_based_extension_tips: IStringDictionary<IConfigBasedExtensionTip>,
            exe_based_extension_tips: IStringDictionary<IExeBasedExtensionTip>,
            remote_extension_tips: IStringDictionary<IRemoteExtensionTip>,
            virtual_workspace_extension_tips: IStringDictionary<IVirtualWorkspaceExtensionTip>,
            extension_keywords: IStringDictionary<Vec<String>>,
            keymap_extension_tips: Vec<String>,
            web_extension_tips: Vec<String>,
            language_extension_tips: Vec<String>,
            trusted_extension_url_public_keys: IStringDictionary<Vec<String>>,
            trusted_extension_auth_access: StringsOrDictionary,
            trusted_mcp_auth_access: StringsOrDictionary,
            inherit_auth_account_preference: IStringDictionary<Vec<String>>,
            trusted_extension_protocol_handlers: Vec<String>,
            command_palette_suggested_command_ids: Vec<String>,
            crash_reporter: ICrashReporter,
            remove_telemetry_machine_id: bool,
            enabled_telemetry_levels: IEnabledTelemetryLevels,
            enable_telemetry: bool,
            open_to_welcome_main_page: bool,
            ai_config: IAiConfig,
            documentation_url: String,
            server_documentation_url: String,
            release_notes_url: String,
            keyboard_shortcuts_url_mac: String,
            keyboard_shortcuts_url_linux: String,
            keyboard_shortcuts_url_win: String,
            introductory_videos_url: String,
            tips_and_tricks_url: String,
            newsletter_signup_url: String,
            you_tube_url: String,
            request_feature_url: String,
            report_issue_url: String,
            report_marketplace_issue_url: String,
            license_url: String,
            server_license_url: String,
            privacy_statement_url: String,
            show_telemetry_opt_out: bool,
            server_greeting: Vec<String>,
            server_license: Vec<String>,
            server_license_prompt: String,
            server_data_folder_name: String,
            tunnel_application_name: String,
            tunnel_application_config: ITunnelApplicationConfig,
            nps_survey_url: String,
            surveys: Vec<ISurveyData>,
            checksums: IStringDictionary<String>,
            checksum_fail_more_info_url: String,
            app_center: IAppCenterConfiguration,
            portable: String,
            extension_kind: IStringDictionary<Vec<String>>,
            extension_point_extension_kind: IStringDictionary<Vec<String>>,
            extension_synced_keys: IStringDictionary<Vec<String>>,
            extension_enabled_api_proposals: IStringDictionary<Vec<String>>,
            extension_untrusted_workspace_support: IStringDictionary<ExtensionUntrustedWorkspaceSupport>,
            extension_virtual_workspaces_support: IStringDictionary<ExtensionVirtualWorkspaceSupport>,
            extensions_force_version_by_quality: Vec<String>,
            sessions_window_allowed_extensions: Vec<String>,
            msft_internal_domains: Vec<String>,
            link_protection_trusted_domains: Vec<String>,
            auth_client_id_metadata_url: String,
            #[serde(rename = "configurationSync.store")]
            configuration_sync_store: ConfigurationSyncStore,
            #[serde(rename = "editSessions.store")]
            edit_sessions_store: EditSessionsStore,
            darwin_universal_asset_id: String,
            darwin_bundle_identifier: String,
            profile_templates_url: String,
            commonly_used_settings: Vec<String>,
            ai_generated_workspace_trust: IAiGeneratedWorkspaceTrust,
            chat_participant_registry: String,
            chat_session_recommendations: Vec<IChatSessionRecommendation>,
            emergency_alert_url: String,
            voice_ws_url: String,
            remote_default_extensions_if_installed_locally: Vec<String>,
            /// Maps an extension-contributed setting key to either a full enterprise policy (the setting
            /// owns the policy) or a reference to a policy owned by an in-code setting.
            extension_configuration_policy: IStringDictionary<ExtensionConfigurationPolicy>,
            onboarding_keymaps: Vec<IProductOnboardingKeymap>,
            onboarding_themes: Vec<IProductOnboardingTheme>,
            /// When running as an embedded app, the parent VS Code's policy identity.
            parent_policy_config: IParentPolicyConfig,
        }
    }
}

impl IProductConfiguration {
    /// Lê um `product.json` (um objeto JSON) como configuração de produto.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Escreve a configuração como JSON, com as chaves desconhecidas de volta.
    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("a configuração de produto sempre serializa")
    }
}
