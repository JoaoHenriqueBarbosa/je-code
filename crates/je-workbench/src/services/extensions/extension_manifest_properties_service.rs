//! Porte de `src/vs/workbench/services/extensions/common/extensionManifestPropertiesService.ts`: onde uma
//! extensão pode rodar (`ExtensionKind`: interface, workspace, web), se roda num workspace sem confiança, num
//! workspace virtual e na janela de agentes, combinando o manifesto, o `product.json` e as configurações do
//! usuário.
//!
//! Divergências:
//! - O `extends Disposable` vira o campo `base` ([`Disposable`]), como nos demais serviços; o `dispose` do
//!   serviço descarta o `base`.
//! - Os métodos de [`IExtensionManifestPropertiesService`] ficam só no `impl` do trait (sem duplicar em métodos
//!   próprios). O `isBuiltin?: boolean` de `canExecuteOnSessionsWindow` é um `bool` obrigatório (o padrão do
//!   original é `false`).
//! - `registerSingleton(..., InstantiationType.Delayed)` não tem equivalente: o registro de singletons do
//!   workbench não está portado, quem monta o workbench cria o serviço com
//!   [`ExtensionManifestPropertiesService::new`]. Os identificadores de serviço (`createDecorator`) estão
//!   declarados.
//! - `isWeb` é lido uma vez, de [`Platform::detect`], na criação do serviço.
//! - O `ExtensionKind` do original (`environment.ts`) é o de [`je_extensions::manifest`], o mesmo de
//!   `ALL_EXTENSION_KINDS` e do `extensionKind` do manifesto. As configurações e o `product.json` aceitam também
//!   `-web` (o `ExtensionKind | '-web'` do original), representado por [`ConfiguredExtensionKind`]; um texto que
//!   não é nenhum dos quatro valores é ignorado (o original o repassaria como veio).
//! - O manifesto do porte guarda `publisher` e `name` como texto (vazio quando ausente). Um `publisher` vazio
//!   vale como ausente e cai em `undefined_publisher`, como o `publisher ?? UNDEFINED_PUBLISHER` do original.
//! - As chaves de `contributes` (`Object.keys(manifest.contributes)`) vêm da serialização do manifesto: um
//!   ponto de contribuição com valor do tipo errado (que o manifesto tolerante descarta) não conta.
//! - `ExtensionUntrustedWorkspaceSupportType` e `ExtensionVirtualWorkspaceSupportType` (`boolean | 'limited'`)
//!   são [`BoolOrLimited`], o tipo do `product.json` portado.
//! - O `declared` de `capabilities.agentsWindow.supported` só vale se for booleano.
//! - `WORKSPACE_TRUST_EXTENSION_SUPPORT` é de `workbench/services/workspaces/common/workspaceTrust.ts`, ainda não
//!   portado; a constante mora aqui até esse arquivo entrar.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::platform::Platform;
use je_base::product::{BoolOrLimited, ExtensionUntrustedWorkspaceSupport, ExtensionVirtualWorkspaceSupport};
use je_extensions::manifest::{
    ALL_EXTENSION_KINDS, EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, ExtensionIdentifierInfo, ExtensionIdentifierMap, ExtensionKind, ExtensionKindOrList,
    ExtensionManifest,
};
use je_extensions::util::get_gallery_extension_id;
use je_instantiation::service_id;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::product::IProductService;
use je_platform::workspace_trust::IWorkspaceTrustEnablementService;
use serde_json::Value;

use super::extensions_registry::extensions_registry;

pub const EXTENSIONS_SUPPORT_AGENTS_WINDOW: &str = "extensions.supportAgentsWindow";

/// `WORKSPACE_TRUST_EXTENSION_SUPPORT`.
pub const WORKSPACE_TRUST_EXTENSION_SUPPORT: &str = "extensions.supportUntrustedWorkspaces";

/// `SESSIONS_WINDOW_ALLOWED_CONTRIBUTION_POINTS`.
const SESSIONS_WINDOW_ALLOWED_CONTRIBUTION_POINTS: [&str; 10] = [
    "themes",
    "iconThemes",
    "productIconThemes",
    "colors",
    "keybindings",
    "jsonValidation",
    "jsonValidationRegistry",
    "localizations",
    "grammars",
    "languages",
];

/// `ExtensionKind | '-web'`: o que as configurações e o `product.json` aceitam como tipo de extensão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfiguredExtensionKind {
    Kind(ExtensionKind),
    /// `-web`: a extensão não roda no navegador.
    NotWeb,
}

impl ConfiguredExtensionKind {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "ui" => Some(Self::Kind(ExtensionKind::Ui)),
            "workspace" => Some(Self::Kind(ExtensionKind::Workspace)),
            "web" => Some(Self::Kind(ExtensionKind::Web)),
            "-web" => Some(Self::NotWeb),
            _ => None,
        }
    }

    fn parse_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(text) => Self::parse(&text.to_string_lossy()),
            _ => None,
        }
    }

    /// O `toArray` do original para um valor único: `ui` sozinho também roda no workspace.
    fn single_to_array(self) -> Vec<Self> {
        match self {
            Self::Kind(ExtensionKind::Ui) => vec![Self::Kind(ExtensionKind::Ui), Self::Kind(ExtensionKind::Workspace)],
            other => vec![other],
        }
    }

    /// O `toArray` do original para o valor de uma configuração (`ExtensionKind | ExtensionKind[]`).
    fn array_of(value: &Dynamic) -> Vec<Self> {
        match value {
            Dynamic::Array(items) => items.to_vec().iter().filter_map(Self::parse_dynamic).collect(),
            single => Self::parse_dynamic(single).map(Self::single_to_array).unwrap_or_default(),
        }
    }
}

/// `IExtensionManifestPropertiesService`.
pub trait IExtensionManifestPropertiesService {
    fn prefers_execute_on_ui(&self, manifest: &ExtensionManifest) -> bool;
    fn prefers_execute_on_workspace(&self, manifest: &ExtensionManifest) -> bool;
    fn prefers_execute_on_web(&self, manifest: &ExtensionManifest) -> bool;

    fn can_execute_on_ui(&self, manifest: &ExtensionManifest) -> bool;
    fn can_execute_on_workspace(&self, manifest: &ExtensionManifest) -> bool;
    fn can_execute_on_web(&self, manifest: &ExtensionManifest) -> bool;
    fn can_execute_on_sessions_window(&self, manifest: &ExtensionManifest, is_builtin: bool) -> bool;

    fn get_extension_kind(&self, manifest: &ExtensionManifest) -> Vec<ExtensionKind>;
    fn get_user_configured_extension_kind(&self, extension_identifier: &ExtensionIdentifierInfo) -> Option<Vec<ConfiguredExtensionKind>>;
    fn get_extension_untrusted_workspace_support_type(&self, manifest: &ExtensionManifest) -> BoolOrLimited;
    fn get_extension_virtual_workspace_support_type(&self, manifest: &ExtensionManifest) -> BoolOrLimited;
}

service_id!(pub I_EXTENSION_MANIFEST_PROPERTIES_SERVICE: dyn IExtensionManifestPropertiesService = "extensionManifestPropertiesService");

/// `ExtensionIdentifierMap` a partir de pares (identificador, valor).
fn identifier_map<V>(entries: impl IntoIterator<Item = (String, V)>) -> ExtensionIdentifierMap<V> {
    let mut map = ExtensionIdentifierMap::new();
    for (id, value) in entries {
        map.set(&id, value);
    }
    map
}

/// O `ExtensionIdentifierMap` de um objeto de configuração (`getValue(...) || {}` e `Object.keys`): as chaves são
/// identificadores de extensão, e um valor `undefined` é como ausente.
fn identifier_map_of(configured: &Dynamic) -> ExtensionIdentifierMap<Dynamic> {
    if !configured.is_truthy() {
        return ExtensionIdentifierMap::new();
    }
    identifier_map(configured.own_entries().into_iter().filter(|(_, value)| !value.is_undefined()).map(|(id, value)| (id.to_string_lossy(), value)))
}

/// `getGalleryExtensionId(manifest.publisher, manifest.name)`.
fn gallery_id_of(manifest: &ExtensionManifest) -> String {
    get_gallery_extension_id(Some(manifest.publisher.as_str()).filter(|publisher| !publisher.is_empty()), &manifest.name)
}

/// A verdade JavaScript de um ponto de entrada (`manifest.main`, `manifest.browser`).
fn has_entry_point(entry_point: &Option<String>) -> bool {
    entry_point.as_deref().is_some_and(|path| !path.is_empty())
}

/// `isNonEmptyArray`.
fn is_non_empty_array(items: &Option<Vec<String>>) -> bool {
    items.as_ref().is_some_and(|items| !items.is_empty())
}

/// O valor `boolean | 'limited'` de uma configuração.
fn bool_or_limited_of_dynamic(value: &Dynamic) -> Option<BoolOrLimited> {
    match value {
        Dynamic::Bool(boolean) => Some(BoolOrLimited::Bool(*boolean)),
        Dynamic::String(text) => Some(BoolOrLimited::Limited(text.to_string_lossy())),
        _ => None,
    }
}

/// As chaves de `manifest.contributes` (`Object.keys`).
fn contribution_points(manifest: &ExtensionManifest) -> Vec<String> {
    match manifest.contributes.as_ref().map(serde_json::to_value) {
        Some(Ok(Value::Object(contributes))) => contributes.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `ExtensionKind[]` do `product.json`, ignorando o que não for um tipo de extensão.
fn kinds_of(values: &[String]) -> Vec<ExtensionKind> {
    values
        .iter()
        .filter_map(|value| match ConfiguredExtensionKind::parse(value) {
            Some(ConfiguredExtensionKind::Kind(kind)) => Some(kind),
            _ => None,
        })
        .collect()
}

/// `ExtensionManifestPropertiesService`.
pub struct ExtensionManifestPropertiesService {
    base: Rc<Disposable>,
    product_service: Rc<dyn IProductService>,
    configuration_service: Rc<dyn IConfigurationService>,
    workspace_trust_enablement_service: Rc<dyn IWorkspaceTrustEnablementService>,
    log_service: LogServiceRef,
    is_web: bool,

    extension_point_extension_kinds_map: OnceCell<HashMap<String, Vec<ExtensionKind>>>,
    product_extension_kinds_map: OnceCell<ExtensionIdentifierMap<Vec<ConfiguredExtensionKind>>>,
    configured_extension_kinds_map: OnceCell<ExtensionIdentifierMap<Dynamic>>,

    product_virtual_workspace_support_map: OnceCell<ExtensionIdentifierMap<ExtensionVirtualWorkspaceSupport>>,
    configured_virtual_workspace_support_map: OnceCell<ExtensionIdentifierMap<Dynamic>>,
    configured_sessions_window_support_map: OnceCell<ExtensionIdentifierMap<Dynamic>>,

    /// `{ supported, version? }` cru de cada extensão, como está no `settings.json`.
    configured_extension_workspace_trust_request_map: ExtensionIdentifierMap<Dynamic>,
    product_extension_workspace_trust_request_map: HashMap<String, ExtensionUntrustedWorkspaceSupport>,
}

impl ExtensionManifestPropertiesService {
    pub fn new(
        product_service: Rc<dyn IProductService>,
        configuration_service: Rc<dyn IConfigurationService>,
        workspace_trust_enablement_service: Rc<dyn IWorkspaceTrustEnablementService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        // Workspace trust request type (settings.json)
        let configured_extension_workspace_trust_request_map =
            identifier_map_of(&configuration_service.inspect(WORKSPACE_TRUST_EXTENSION_SUPPORT, &ConfigurationOverrides::default()).user_value());

        // Workspace trust request type (product.json)
        let product_extension_workspace_trust_request_map: HashMap<String, ExtensionUntrustedWorkspaceSupport> = product_service
            .extension_untrusted_workspace_support()
            .map(|dictionary| dictionary.iter().map(|(id, support)| (id.clone(), support.clone())).collect())
            .unwrap_or_default();

        Rc::new(Self {
            base: Disposable::new(),
            product_service,
            configuration_service,
            workspace_trust_enablement_service,
            log_service,
            is_web: Platform::detect().is_web,
            extension_point_extension_kinds_map: OnceCell::new(),
            product_extension_kinds_map: OnceCell::new(),
            configured_extension_kinds_map: OnceCell::new(),
            product_virtual_workspace_support_map: OnceCell::new(),
            configured_virtual_workspace_support_map: OnceCell::new(),
            configured_sessions_window_support_map: OnceCell::new(),
            configured_extension_workspace_trust_request_map,
            product_extension_workspace_trust_request_map,
        })
    }

    /// `isWeb ? ['workspace', 'web'] : ['workspace']`.
    fn workspace_kinds(&self) -> Vec<ExtensionKind> {
        if self.is_web { vec![ExtensionKind::Workspace, ExtensionKind::Web] } else { vec![ExtensionKind::Workspace] }
    }

    fn deduce_extension_kind(&self, manifest: &ExtensionManifest) -> Vec<ExtensionKind> {
        // Not an UI extension if it has main
        if has_entry_point(&manifest.main) {
            if has_entry_point(&manifest.browser) {
                return self.workspace_kinds();
            }
            return vec![ExtensionKind::Workspace];
        }

        if has_entry_point(&manifest.browser) {
            return vec![ExtensionKind::Web];
        }

        let mut result = ALL_EXTENSION_KINDS.to_vec();

        if is_non_empty_array(&manifest.extension_pack) || is_non_empty_array(&manifest.extension_dependencies) {
            // Extension pack defaults to [workspace, web] in web and only [workspace] in desktop
            result = self.workspace_kinds();
        }

        if manifest.contributes.is_some() {
            for contribution in contribution_points(manifest) {
                let supported_extension_kinds = self.get_supported_extension_kinds_for_extension_point(&contribution);
                if !supported_extension_kinds.is_empty() {
                    result.retain(|extension_kind| supported_extension_kinds.contains(extension_kind));
                }
            }
        }

        if result.is_empty() {
            self.log_service.warn_args(
                &LogMessage::from("Cannot deduce extensionKind for extension"),
                &[LogArg::Value(Dynamic::string(&gallery_id_of(manifest)))],
            );
        }

        result
    }

    fn get_supported_extension_kinds_for_extension_point(&self, extension_point: &str) -> Vec<ExtensionKind> {
        let extension_point_extension_kinds_map = self.extension_point_extension_kinds_map.get_or_init(|| {
            extensions_registry()
                .get_extension_points()
                .iter()
                // `e.defaultExtensionKind || [] /* supports all */`
                .map(|point| (point.name.clone(), point.default_extension_kind.clone().unwrap_or_default()))
                .collect()
        });

        if let Some(extension_point_extension_kind) = extension_point_extension_kinds_map.get(extension_point) {
            return extension_point_extension_kind.clone();
        }

        if let Some(extension_point_extension_kind) = self.product_service.extension_point_extension_kind().and_then(|kinds| kinds.get(extension_point)) {
            return kinds_of(extension_point_extension_kind);
        }

        /* Unknown extension point */
        self.workspace_kinds()
    }

    fn get_configured_extension_kind(&self, manifest: &ExtensionManifest) -> Option<Vec<ConfiguredExtensionKind>> {
        let extension_identifier = ExtensionIdentifierInfo::new(gallery_id_of(manifest));

        // check in config
        if let Some(result) = self.get_user_configured_extension_kind(&extension_identifier) {
            return Some(result);
        }

        // check product.json
        if let Some(result) = self.get_product_extension_kind(manifest) {
            return Some(result);
        }

        // check the manifest itself
        manifest.extension_kind.as_ref().map(|extension_kind| {
            let kinds = match extension_kind {
                ExtensionKindOrList::One(kind) => ConfiguredExtensionKind::Kind(*kind).single_to_array(),
                ExtensionKindOrList::Many(kinds) => kinds.iter().map(|kind| ConfiguredExtensionKind::Kind(*kind)).collect(),
            };
            kinds
                .into_iter()
                .filter(|kind| matches!(kind, ConfiguredExtensionKind::Kind(ExtensionKind::Ui | ExtensionKind::Workspace)))
                .collect()
        })
    }

    fn get_product_extension_kind(&self, manifest: &ExtensionManifest) -> Option<Vec<ConfiguredExtensionKind>> {
        let product_extension_kinds_map = self.product_extension_kinds_map.get_or_init(|| {
            identifier_map(self.product_service.extension_kind().into_iter().flatten().map(|(id, kinds)| {
                (id.clone(), kinds.iter().filter_map(|kind| ConfiguredExtensionKind::parse(kind)).collect())
            }))
        });

        product_extension_kinds_map.get(&gallery_id_of(manifest)).cloned()
    }

    fn get_product_virtual_workspace_support(&self, manifest: &ExtensionManifest) -> Option<&ExtensionVirtualWorkspaceSupport> {
        let product_workspace_schemes_map = self.product_virtual_workspace_support_map.get_or_init(|| {
            identifier_map(self.product_service.extension_virtual_workspaces_support().into_iter().flatten().map(|(id, support)| (id.clone(), support.clone())))
        });

        product_workspace_schemes_map.get(&gallery_id_of(manifest))
    }

    fn get_configured_virtual_workspace_support(&self, manifest: &ExtensionManifest) -> Option<BoolOrLimited> {
        let configured_workspace_schemes_map = self
            .configured_virtual_workspace_support_map
            .get_or_init(|| identifier_map_of(&self.configuration_service.get_value(Some("extensions.supportVirtualWorkspaces"), None)));

        configured_workspace_schemes_map.get(&gallery_id_of(manifest)).and_then(bool_or_limited_of_dynamic)
    }

    fn get_configured_sessions_window_support(&self, manifest: &ExtensionManifest) -> Option<bool> {
        let configured_sessions_window_support_map = self
            .configured_sessions_window_support_map
            .get_or_init(|| identifier_map_of(&self.configuration_service.get_value(Some(EXTENSIONS_SUPPORT_AGENTS_WINDOW), None)));

        configured_sessions_window_support_map.get(&gallery_id_of(manifest)).map(|support| support.is_truthy())
    }

    fn get_configured_extension_workspace_trust_request(&self, manifest: &ExtensionManifest) -> Option<BoolOrLimited> {
        let extension_workspace_trust_request = self.configured_extension_workspace_trust_request_map.get(&gallery_id_of(manifest))?;

        if extension_workspace_trust_request.is_truthy() {
            let version = extension_workspace_trust_request.get_str("version");
            let same_version = matches!(&version, Dynamic::String(version) if version.to_string_lossy() == manifest.version);
            if version.is_undefined() || same_version {
                return bool_or_limited_of_dynamic(&extension_workspace_trust_request.get_str("supported"));
            }
        }

        None
    }

    fn get_product_extension_workspace_trust_request(&self, manifest: &ExtensionManifest) -> Option<&ExtensionUntrustedWorkspaceSupport> {
        self.product_extension_workspace_trust_request_map.get(&gallery_id_of(manifest))
    }
}

impl IExtensionManifestPropertiesService for ExtensionManifestPropertiesService {
    fn can_execute_on_sessions_window(&self, manifest: &ExtensionManifest, is_builtin: bool) -> bool {
        if let Some(configured_sessions_window_support) = self.get_configured_sessions_window_support(manifest) {
            return configured_sessions_window_support;
        }

        let declared_sessions_window_support =
            manifest.capabilities.as_ref().and_then(|capabilities| capabilities.agents_window.as_ref()).and_then(|agents_window| agents_window.get("supported")).and_then(Value::as_bool);
        if let Some(declared_sessions_window_support) = declared_sessions_window_support
            && (is_builtin || self.configuration_service.get_value(Some(EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY), None).is_truthy())
        {
            return declared_sessions_window_support;
        }

        if is_builtin {
            return manifest.contributes.as_ref().is_none_or(|contributes| {
                contributes.debuggers.is_none() && contributes.views.is_none() && contributes.views_containers.is_none() && contributes.walkthroughs.is_none()
            });
        }

        // Extensions without declared support must not have executable code.
        if has_entry_point(&manifest.main) || has_entry_point(&manifest.browser) {
            return false;
        }

        // Only allow extensions that contribute to themes and other declarative, non-executing contribution points
        contribution_points(manifest).iter().all(|point| SESSIONS_WINDOW_ALLOWED_CONTRIBUTION_POINTS.contains(&point.as_str()))
    }

    fn prefers_execute_on_ui(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).first() == Some(&ExtensionKind::Ui)
    }

    fn prefers_execute_on_workspace(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).first() == Some(&ExtensionKind::Workspace)
    }

    fn prefers_execute_on_web(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).first() == Some(&ExtensionKind::Web)
    }

    fn can_execute_on_ui(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).contains(&ExtensionKind::Ui)
    }

    fn can_execute_on_workspace(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).contains(&ExtensionKind::Workspace)
    }

    fn can_execute_on_web(&self, manifest: &ExtensionManifest) -> bool {
        self.get_extension_kind(manifest).contains(&ExtensionKind::Web)
    }

    fn get_extension_kind(&self, manifest: &ExtensionManifest) -> Vec<ExtensionKind> {
        let deduced_extension_kind = self.deduce_extension_kind(manifest);
        let configured_extension_kind = self.get_configured_extension_kind(manifest);

        if let Some(configured_extension_kind) = configured_extension_kind.filter(|kinds| !kinds.is_empty()) {
            let mut result: Vec<ExtensionKind> = configured_extension_kind
                .iter()
                .filter_map(|extension_kind| match extension_kind {
                    ConfiguredExtensionKind::Kind(kind) => Some(*kind),
                    ConfiguredExtensionKind::NotWeb => None,
                })
                .collect();
            let opted_out_from_web = configured_extension_kind.contains(&ConfiguredExtensionKind::NotWeb);

            // If opted out from web without specifying other extension kinds then default to ui, workspace
            if opted_out_from_web && result.is_empty() {
                result.push(ExtensionKind::Ui);
                result.push(ExtensionKind::Workspace);
            }

            // Add web kind if not opted out from web and can run in web
            if self.is_web
                && !opted_out_from_web
                && !configured_extension_kind.contains(&ConfiguredExtensionKind::Kind(ExtensionKind::Web))
                && deduced_extension_kind.contains(&ExtensionKind::Web)
            {
                result.push(ExtensionKind::Web);
            }

            return result;
        }

        deduced_extension_kind
    }

    fn get_user_configured_extension_kind(&self, extension_identifier: &ExtensionIdentifierInfo) -> Option<Vec<ConfiguredExtensionKind>> {
        let configured_extension_kinds_map =
            self.configured_extension_kinds_map.get_or_init(|| identifier_map_of(&self.configuration_service.get_value(Some("remote.extensionKind"), None)));

        configured_extension_kinds_map
            .get(&extension_identifier.id)
            .filter(|user_configured_extension_kind| user_configured_extension_kind.is_truthy())
            .map(ConfiguredExtensionKind::array_of)
    }

    fn get_extension_untrusted_workspace_support_type(&self, manifest: &ExtensionManifest) -> BoolOrLimited {
        // Workspace trust feature is disabled, or extension has no entry point
        if !self.workspace_trust_enablement_service.is_workspace_trust_enabled() || !has_entry_point(&manifest.main) {
            return BoolOrLimited::Bool(true);
        }

        // Get extension workspace trust requirements from settings.json
        let configured_workspace_trust_request = self.get_configured_extension_workspace_trust_request(manifest);

        // Get extension workspace trust requirements from product.json
        let product_workspace_trust_request = self.get_product_extension_workspace_trust_request(manifest);

        // Use settings.json override value if it exists
        if let Some(configured_workspace_trust_request) = configured_workspace_trust_request {
            return configured_workspace_trust_request;
        }

        // Use product.json override value if it exists
        if let Some(override_value) = product_workspace_trust_request.and_then(|request| request.override_value.clone()) {
            return override_value;
        }

        // Use extension manifest value if it exists
        let manifest_supported = manifest
            .capabilities
            .as_ref()
            .and_then(|capabilities| capabilities.untrusted_workspaces.as_ref())
            .and_then(|untrusted_workspaces| untrusted_workspaces.get("supported"))
            .and_then(|supported| serde_json::from_value::<BoolOrLimited>(supported.clone()).ok());
        if let Some(manifest_supported) = manifest_supported {
            return manifest_supported;
        }

        // Use product.json default value if it exists
        if let Some(default_value) = product_workspace_trust_request.and_then(|request| request.default_value.clone()) {
            return default_value;
        }

        BoolOrLimited::Bool(false)
    }

    fn get_extension_virtual_workspace_support_type(&self, manifest: &ExtensionManifest) -> BoolOrLimited {
        // check user configured
        if let Some(user_configured_virtual_workspace_support) = self.get_configured_virtual_workspace_support(manifest) {
            return user_configured_virtual_workspace_support;
        }

        let product_configured_workspace_schemes = self.get_product_virtual_workspace_support(manifest);

        // check override from product
        if let Some(override_value) = product_configured_workspace_schemes.and_then(|schemes| schemes.override_value) {
            return BoolOrLimited::Bool(override_value);
        }

        // check the manifest
        match manifest.capabilities.as_ref().and_then(|capabilities| capabilities.virtual_workspaces.as_ref()) {
            Some(Value::Bool(virtual_workspaces)) => return BoolOrLimited::Bool(*virtual_workspaces),
            Some(Value::Object(virtual_workspaces)) => match virtual_workspaces.get("supported") {
                Some(Value::Bool(supported)) => return BoolOrLimited::Bool(*supported),
                Some(Value::String(supported)) if supported == "limited" => return BoolOrLimited::Limited(supported.clone()),
                _ => {}
            },
            _ => {}
        }

        // check default from product
        if let Some(default_value) = product_configured_workspace_schemes.and_then(|schemes| schemes.default_value) {
            return BoolOrLimited::Bool(default_value);
        }

        // Default - supports virtual workspace
        BoolOrLimited::Bool(true)
    }
}

impl IDisposable for ExtensionManifestPropertiesService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
