//! Porte de `src/vs/workbench/api/common/extHostConfiguration.ts`: o lado da extensão das configurações
//! (`vscode.workspace.getConfiguration`, `onDidChangeConfiguration`, `WorkspaceConfiguration` e o `inspect`).
//!
//! Divergências:
//! - O `vscode.WorkspaceConfiguration` (um objeto congelado com `has`, `get`, `update`, `inspect` e as propriedades
//!   do valor, misturadas sem sobrescrever) é o [`WorkspaceConfiguration`]. Os quatro métodos são métodos do struct;
//!   [`WorkspaceConfiguration::property`] é o `config[nome]` do original (um membro vence uma propriedade de mesmo
//!   nome, como `mixin(result, config, false)`), [`WorkspaceConfiguration::set_property`] é a escrita num objeto
//!   congelado (sempre `TypeError`), e [`WorkspaceConfiguration::to_dynamic`] é o objeto congelado só com os valores
//!   (o `update` devolve uma promessa e [`Dynamic`] não tem esse tipo, então a ponte com o zjsc o liga ao método).
//! - Os `Proxy` de `_toReadonlyValue` e de `cloneOnWriteProxy` não existem. O valor lido é congelado em profundidade
//!   (a escrita lança, como o `set` do proxy somente leitura) e `get` devolve uma cópia profunda do objeto ou do array
//!   (o que o proxy de clonar ao escrever mostra a quem lê: as escritas ficam na cópia e a configuração não muda).
//! - `vscode.ConfigurationScope` é [`ConfigurationScopeArg`] (a união `Uri | TextDocument | WorkspaceFolder | { uri?,
//!   languageId }`, com `Null`) e `scope?: ... | null` é `Option<&ConfigurationScopeArg>` (`None` é `undefined`). O
//!   `resource: null` de `IConfigurationOverrides` e o `undefined` são ambos `None` em [`ConfigurationOverrides`]; a
//!   diferença só importa ao aviso de `_validateConfigurationAccess` e é guardada à parte (`resource_defined`).
//! - `ConfigurationTarget | boolean | undefined | null` de `update` é `Option<`[`ConfigurationTargetArg`]`>`.
//! - `ConfigurationInspect<T>` guarda cada valor como [`Dynamic`] (`undefined` é `Dynamic::Undefined`); o
//!   [`ConfigurationInspect::to_dynamic`] mantém as chaves ausentes como `undefined`, que é o que
//!   `Object.keys(...)` enxerga no original.
//! - O evento `vscode.ConfigurationChangeEvent` é o [`ExtHostConfigurationChangeEvent`] (o `Object.freeze({
//!   affectsConfiguration })`).
//! - `ExtHostConfigProvider` se refere a si mesmo para criar o `WorkspaceConfiguration`: guarda um [`Weak`] do `Rc`.
//! - Os métodos de RPC (`$initializeConfiguration`, `$acceptConfigurationChanged`) são o trait
//!   [`ExtHostConfigurationShape`] (prefixo `rpc_`); o `$acceptConfigurationChanged` do provedor é
//!   [`ExtHostConfigProvider::accept_configuration_changed`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::Barrier;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::objects;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, InspectValue, PreviousConfiguration};
use je_platform::configuration::registry::{ConfigurationScope, is_override_property};
use je_platform::configuration::{ConfigurationChange, ConfigurationOverrides, ConfigurationTarget, ConfigurationValue};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::workspace::Workspace;

use super::ext_host_document_data::TextDocument;
use super::ext_host_protocol::{
    ExtHostConfigurationShape, IConfigurationInitData, IMainContext, MainContext, MainThreadConfigurationShape,
};
use super::ext_host_types::ConfigurationTarget as ExtHostConfigurationTarget;
use super::ext_host_workspace::{ExtHostWorkspace, WorkspaceFolder};

/// `lookUp(tree, key)`: o valor no caminho `a.b.c`. Como no original, o laço para num nó falso (um `false` ou um `0`
/// no meio do caminho é o resultado), e uma chave vazia dá `undefined`.
fn look_up(tree: &Dynamic, key: &str) -> Dynamic {
    if key.is_empty() {
        return Dynamic::Undefined;
    }
    let mut node = tree.clone();
    for part in key.split('.') {
        if !node.is_truthy() {
            break;
        }
        node = node.get(&JsString::from(part));
    }
    node
}

/// `ConfigurationInspect<T>`.
#[derive(Clone, Debug)]
pub struct ConfigurationInspect {
    pub key: String,

    pub default_value: Dynamic,
    pub global_local_value: Dynamic,
    pub global_remote_value: Dynamic,
    pub global_value: Dynamic,
    pub workspace_value: Dynamic,
    pub workspace_folder_value: Dynamic,

    pub default_language_value: Dynamic,
    pub global_local_language_value: Dynamic,
    pub global_remote_language_value: Dynamic,
    pub global_language_value: Dynamic,
    pub workspace_language_value: Dynamic,
    pub workspace_folder_language_value: Dynamic,

    pub language_ids: Option<Vec<String>>,
}

impl ConfigurationInspect {
    /// O objeto do original, com as chaves ausentes como `undefined`.
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("key", Dynamic::string(&self.key)),
            ("defaultValue", self.default_value.clone()),
            ("globalLocalValue", self.global_local_value.clone()),
            ("globalRemoteValue", self.global_remote_value.clone()),
            ("globalValue", self.global_value.clone()),
            ("workspaceValue", self.workspace_value.clone()),
            ("workspaceFolderValue", self.workspace_folder_value.clone()),
            ("defaultLanguageValue", self.default_language_value.clone()),
            ("globalLocalLanguageValue", self.global_local_language_value.clone()),
            ("globalRemoteLanguageValue", self.global_remote_language_value.clone()),
            ("globalLanguageValue", self.global_language_value.clone()),
            ("workspaceLanguageValue", self.workspace_language_value.clone()),
            ("workspaceFolderLanguageValue", self.workspace_folder_language_value.clone()),
            (
                "languageIds",
                self.language_ids.as_ref().map_or(Dynamic::Undefined, |ids| Dynamic::array(ids.iter().map(|id| Dynamic::string(id)))),
            ),
        ])
    }
}

/// `vscode.ConfigurationScope`, mais o `null`.
#[derive(Clone)]
pub enum ConfigurationScopeArg {
    /// Um `vscode.Uri`.
    Uri(URI),
    /// Um `vscode.TextDocument` ou `{ uri, languageId }`.
    ResourceLanguage { uri: URI, language_id: String },
    /// `{ languageId }`.
    Language { language_id: String },
    /// Um `vscode.WorkspaceFolder`.
    WorkspaceFolder(Rc<WorkspaceFolder>),
    /// `null`: qualquer recurso.
    Null,
}

impl From<URI> for ConfigurationScopeArg {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

impl From<&TextDocument> for ConfigurationScopeArg {
    fn from(document: &TextDocument) -> Self {
        Self::ResourceLanguage { uri: document.uri().clone(), language_id: document.language_id() }
    }
}

impl From<Rc<WorkspaceFolder>> for ConfigurationScopeArg {
    fn from(folder: Rc<WorkspaceFolder>) -> Self {
        Self::WorkspaceFolder(folder)
    }
}

/// `ExtHostConfigurationTarget | boolean`, o alvo de `update`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationTargetArg {
    /// `true` é o usuário e `false` o workspace.
    Bool(bool),
    Target(ExtHostConfigurationTarget),
}

impl From<bool> for ConfigurationTargetArg {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<ExtHostConfigurationTarget> for ConfigurationTargetArg {
    fn from(target: ExtHostConfigurationTarget) -> Self {
        Self::Target(target)
    }
}

/// O `IConfigurationOverrides` de `scopeToOverrides`, mais se o `resource` foi dado (o `null` conta como dado).
struct ScopeOverrides {
    overrides: ConfigurationOverrides,
    resource_defined: bool,
}

fn scope_to_overrides(scope: Option<&ConfigurationScopeArg>) -> Option<ScopeOverrides> {
    Some(match scope? {
        ConfigurationScopeArg::Uri(uri) => ScopeOverrides { overrides: ConfigurationOverrides::with_resource(uri.clone()), resource_defined: true },
        ConfigurationScopeArg::ResourceLanguage { uri, language_id } if !language_id.is_empty() => ScopeOverrides {
            overrides: ConfigurationOverrides { override_identifier: Some(JsString::from(language_id.as_str())), resource: Some(uri.clone()) },
            resource_defined: true,
        },
        // `isWorkspaceFolder`: um objeto com `uri` e sem `languageId` válido
        ConfigurationScopeArg::ResourceLanguage { uri, .. } => ScopeOverrides { overrides: ConfigurationOverrides::with_resource(uri.clone()), resource_defined: true },
        ConfigurationScopeArg::Language { language_id } if !language_id.is_empty() => ScopeOverrides {
            overrides: ConfigurationOverrides::with_identifier(language_id.as_str()),
            resource_defined: false,
        },
        ConfigurationScopeArg::Language { .. } => return None,
        ConfigurationScopeArg::WorkspaceFolder(folder) => ScopeOverrides { overrides: ConfigurationOverrides::with_resource(folder.uri().clone()), resource_defined: true },
        ConfigurationScopeArg::Null => ScopeOverrides { overrides: ConfigurationOverrides::default(), resource_defined: true },
    })
}

/// O `ConfigurationTarget` da plataforma de um `ExtHostConfigurationTarget | boolean | undefined | null`.
fn parse_configuration_target(arg: Option<ConfigurationTargetArg>) -> Option<ConfigurationTarget> {
    Some(match arg? {
        ConfigurationTargetArg::Bool(true) => ConfigurationTarget::User,
        ConfigurationTargetArg::Bool(false) => ConfigurationTarget::Workspace,
        ConfigurationTargetArg::Target(ExtHostConfigurationTarget::Global) => ConfigurationTarget::User,
        ConfigurationTargetArg::Target(ExtHostConfigurationTarget::Workspace) => ConfigurationTarget::Workspace,
        ConfigurationTargetArg::Target(ExtHostConfigurationTarget::WorkspaceFolder) => ConfigurationTarget::WorkspaceFolder,
    })
}

/// `ExtHostConfiguration`.
pub struct ExtHostConfiguration {
    this: Weak<ExtHostConfiguration>,
    proxy: Rc<dyn MainThreadConfigurationShape>,
    log_service: LogServiceRef,
    ext_host_workspace: Rc<ExtHostWorkspace>,
    barrier: Barrier,
    actual: RefCell<Option<Rc<ExtHostConfigProvider>>>,
}

impl ExtHostConfiguration {
    pub fn new(ext_host_rpc: &dyn IMainContext, ext_host_workspace: Rc<ExtHostWorkspace>, log_service: LogServiceRef) -> Rc<ExtHostConfiguration> {
        Rc::new_cyclic(|this| ExtHostConfiguration {
            this: this.clone(),
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_CONFIGURATION),
            log_service,
            ext_host_workspace,
            barrier: Barrier::new(),
            actual: RefCell::new(None),
        })
    }

    pub fn get_config_provider(&self) -> Promise<Rc<ExtHostConfigProvider>> {
        let this = self.this.upgrade().expect("o ExtHostConfiguration está vivo enquanto um método dele roda");
        self.barrier.wait().then(move |_| this.actual.borrow().clone().expect("a barreira só abre depois de $initializeConfiguration"))
    }
}

impl ExtHostConfigurationShape for ExtHostConfiguration {
    fn rpc_initialize_configuration(&self, data: IConfigurationInitData) {
        let provider = ExtHostConfigProvider::new(self.proxy.clone(), self.ext_host_workspace.clone(), data, self.log_service.clone());
        // Push the config provider into ExtHostWorkspace so it can read settings synchronously
        // (DI cycle: ExtHostConfiguration depends on ExtHostWorkspace, so we cannot inject the reverse).
        self.ext_host_workspace.set_config_provider(&provider);
        *self.actual.borrow_mut() = Some(provider);
        self.barrier.open();
    }

    fn rpc_accept_configuration_changed(&self, data: IConfigurationInitData, change: ConfigurationChange) {
        drop(self.get_config_provider().then(move |provider| provider.accept_configuration_changed(data, change)));
    }
}

/// O `vscode.ConfigurationChangeEvent`.
pub struct ExtHostConfigurationChangeEvent {
    event: ConfigurationChangeEvent,
}

impl ExtHostConfigurationChangeEvent {
    /// `affectsConfiguration(section, scope?)`.
    pub fn affects_configuration(&self, section: &str, scope: Option<&ConfigurationScopeArg>) -> bool {
        self.event.affects_configuration(section, scope_to_overrides(scope).map(|scope| scope.overrides).as_ref())
    }
}

/// `ExtHostConfigProvider`.
pub struct ExtHostConfigProvider {
    this: Weak<ExtHostConfigProvider>,
    on_did_change_configuration: Rc<Emitter<ExtHostConfigurationChangeEvent>>,
    proxy: Rc<dyn MainThreadConfigurationShape>,
    ext_host_workspace: Rc<ExtHostWorkspace>,
    configuration_scopes: RefCell<HashMap<String, Option<ConfigurationScope>>>,
    configuration: RefCell<Rc<Configuration>>,
    log_service: LogServiceRef,
}

impl ExtHostConfigProvider {
    pub fn new(
        proxy: Rc<dyn MainThreadConfigurationShape>,
        ext_host_workspace: Rc<ExtHostWorkspace>,
        data: IConfigurationInitData,
        log_service: LogServiceRef,
    ) -> Rc<ExtHostConfigProvider> {
        Rc::new_cyclic(|this| ExtHostConfigProvider {
            this: this.clone(),
            on_did_change_configuration: Emitter::new(),
            proxy,
            ext_host_workspace,
            configuration_scopes: RefCell::new(Self::to_map(data.configuration_scopes)),
            configuration: RefCell::new(Rc::new(Configuration::parse(&data.data, log_service.clone()))),
            log_service,
        })
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<ExtHostConfigurationChangeEvent> {
        self.on_did_change_configuration.event()
    }

    pub fn accept_configuration_changed(&self, data: IConfigurationInitData, change: ConfigurationChange) {
        let previous = PreviousConfiguration { data: self.configuration.borrow().to_data(), workspace: self.platform_workspace() };
        *self.configuration.borrow_mut() = Rc::new(Configuration::parse(&data.data, self.log_service.clone()));
        *self.configuration_scopes.borrow_mut() = Self::to_map(data.configuration_scopes);
        let event = self.to_configuration_change_event(change, previous);
        self.on_did_change_configuration.fire(event);
    }

    /// O `Workspace` base do workspace da extensão (`this._extHostWorkspace.workspace`), que a configuração lê.
    fn platform_workspace(&self) -> Option<Rc<Workspace>> {
        self.ext_host_workspace.workspace().map(|workspace| workspace.workspace().clone())
    }

    pub fn get_configuration(
        &self,
        section: Option<&str>,
        scope: Option<&ConfigurationScopeArg>,
        extension_description: Option<&ExtensionDescription>,
    ) -> WorkspaceConfiguration {
        let ScopeOverrides { overrides, resource_defined } = scope_to_overrides(scope).unwrap_or(ScopeOverrides { overrides: ConfigurationOverrides::default(), resource_defined: false });
        let section = section.filter(|section| !section.is_empty()).map(str::to_owned);
        let workspace = self.platform_workspace();
        let section_key = section.as_deref().map(JsString::from);
        let config = Self::to_readonly_value(self.configuration.borrow().get_value(section_key.as_ref(), &overrides, workspace.as_deref()));
        let extension_identifier = extension_description.map(|extension| extension.identifier.clone());

        if let Some(section) = &section {
            self.validate_configuration_access(section, resource_defined, overrides.resource.as_ref(), extension_identifier.as_ref());
        }

        WorkspaceConfiguration {
            provider: self.this.upgrade().expect("o ExtHostConfigProvider está vivo enquanto um método dele roda"),
            section,
            overrides,
            resource_defined,
            config,
            extension_identifier,
        }
    }

    /// `_toReadonlyValue`: o valor congelado em profundidade (ver as divergências do módulo).
    fn to_readonly_value(result: Dynamic) -> Dynamic {
        objects::deep_freeze(&result)
    }

    fn validate_configuration_access(&self, key: &str, resource_defined: bool, resource: Option<&URI>, extension_id: Option<&ExtensionIdentifier>) {
        let scope = if is_override_property(&JsString::from(key)) { Some(ConfigurationScope::Resource) } else { self.configuration_scopes.borrow().get(key).copied().flatten() };
        let extension_id_text = extension_id.map(|extension_id| format!("[{}] ", extension_id.value)).unwrap_or_default();
        match scope {
            Some(ConfigurationScope::Resource) => {
                if !resource_defined {
                    self.log_service.warn(&format!("{extension_id_text}Accessing a resource scoped configuration without providing a resource is not expected. To get the effective value for '{key}', provide the URI of a resource or 'null' for any resource."));
                }
            }
            Some(ConfigurationScope::Window) => {
                if resource.is_some() {
                    self.log_service.warn(&format!("{extension_id_text}Accessing a window scoped configuration for a resource is not expected. To associate '{key}' to a resource, define its scope to 'resource' in configuration contributions in 'package.json'."));
                }
            }
            _ => {}
        }
    }

    fn to_configuration_change_event(&self, change: ConfigurationChange, previous: PreviousConfiguration) -> ExtHostConfigurationChangeEvent {
        ExtHostConfigurationChangeEvent {
            event: ConfigurationChangeEvent::new(change, Some(previous), self.configuration.borrow().clone(), self.platform_workspace(), self.log_service.clone()),
        }
    }

    fn to_map(scopes: Vec<(String, Option<ConfigurationScope>)>) -> HashMap<String, Option<ConfigurationScope>> {
        scopes.into_iter().collect()
    }
}

/// O que `config[nome]` lê num [`WorkspaceConfiguration`].
#[derive(Clone, Debug)]
pub enum WorkspaceConfigurationProperty {
    /// Um dos membros `has`, `get`, `update` ou `inspect`.
    Method(&'static str),
    /// Uma propriedade do valor (somente leitura).
    Value(Dynamic),
}

/// `vscode.WorkspaceConfiguration`.
pub struct WorkspaceConfiguration {
    provider: Rc<ExtHostConfigProvider>,
    section: Option<String>,
    overrides: ConfigurationOverrides,
    resource_defined: bool,
    config: Dynamic,
    extension_identifier: Option<ExtensionIdentifier>,
}

impl WorkspaceConfiguration {
    /// `section ? `${section}.${key}` : key`.
    fn full_key(&self, key: &str) -> String {
        match &self.section {
            Some(section) => format!("{section}.{key}"),
            None => key.to_owned(),
        }
    }

    pub fn has(&self, key: &str) -> bool {
        !look_up(&self.config, key).is_undefined()
    }

    pub fn get(&self, key: &str, default_value: Option<Dynamic>) -> Dynamic {
        self.provider.validate_configuration_access(&self.full_key(key), self.resource_defined, self.overrides.resource.as_ref(), self.extension_identifier.as_ref());
        let result = look_up(&self.config, key);
        if result.is_undefined() {
            return default_value.unwrap_or(Dynamic::Undefined);
        }
        // um objeto ou array é devolvido como cópia: as escritas ficam nela (ver as divergências do módulo)
        if matches!(result, Dynamic::Object(_) | Dynamic::Array(_)) { objects::deep_clone(&result) } else { result }
    }

    pub fn update(&self, key: &str, value: Dynamic, ext_host_configuration_target: Option<ConfigurationTargetArg>, scope_to_language: Option<bool>) -> Promise<()> {
        let key = self.full_key(key);
        let target = parse_configuration_target(ext_host_configuration_target);
        if !value.is_undefined() {
            self.provider.proxy.rpc_update_configuration_option(target, &key, value, Some(self.overrides.clone()), scope_to_language)
        } else {
            self.provider.proxy.rpc_remove_configuration_option(target, &key, Some(self.overrides.clone()), scope_to_language)
        }
    }

    pub fn inspect(&self, key: &str) -> Option<ConfigurationInspect> {
        let key = self.full_key(key);
        let workspace = self.provider.platform_workspace();
        let config = self.provider.configuration.borrow().inspect(key.as_str(), &self.overrides, workspace.as_deref());

        let value_of = |inspect: &Option<InspectValue>| inspect.as_ref().map_or(Dynamic::Undefined, InspectValue::value);
        let override_of = |inspect: &Option<InspectValue>| inspect.as_ref().map_or(Dynamic::Undefined, InspectValue::override_value);
        // `a ?? b`
        let or_else = |a: Dynamic, b: Dynamic| if a.is_nullish() { b } else { a };
        let clone = |value: Dynamic| objects::deep_clone(&value);

        let (default, application, user) = (config.default_inspect(), config.application(), config.user());
        let (user_local, user_remote) = (config.user_local(), config.user_remote());
        let (workspace_inspect, workspace_folder) = (config.workspace(), config.workspace_folder());

        Some(ConfigurationInspect {
            key,

            default_value: clone(or_else(config.policy().unwrap_or(Dynamic::Undefined), value_of(&default))),
            global_local_value: clone(value_of(&user_local)),
            global_remote_value: clone(value_of(&user_remote)),
            global_value: clone(or_else(value_of(&user), value_of(&application))),
            workspace_value: clone(value_of(&workspace_inspect)),
            workspace_folder_value: clone(value_of(&workspace_folder)),

            default_language_value: clone(override_of(&default)),
            global_local_language_value: clone(override_of(&user_local)),
            global_remote_language_value: clone(override_of(&user_remote)),
            global_language_value: clone(or_else(override_of(&user), override_of(&application))),
            workspace_language_value: clone(override_of(&workspace_inspect)),
            workspace_folder_language_value: clone(override_of(&workspace_folder)),

            language_ids: config.override_identifiers().map(|ids| ids.iter().map(JsString::to_string_lossy).collect()),
        })
    }

    /// `config[name]`: os membros vencem as propriedades do valor (o `mixin` sem sobrescrever).
    pub fn property(&self, name: &str) -> WorkspaceConfigurationProperty {
        match name {
            "has" => WorkspaceConfigurationProperty::Method("has"),
            "get" => WorkspaceConfigurationProperty::Method("get"),
            "update" => WorkspaceConfigurationProperty::Method("update"),
            "inspect" => WorkspaceConfigurationProperty::Method("inspect"),
            _ => WorkspaceConfigurationProperty::Value(match &self.config {
                Dynamic::Object(_) | Dynamic::Array(_) => self.config.get(&JsString::from(name)),
                _ => Dynamic::Undefined,
            }),
        }
    }

    /// `config[name] = value`: o objeto é congelado (`Object.freeze(result)`), então a escrita sempre lança.
    pub fn set_property(&self, name: &str, _value: Dynamic) -> Result<(), Error> {
        Err(Error::type_error(format!("Cannot assign to read only property '{name}' of object '#<Object>'")))
    }

    /// O objeto congelado com as propriedades do valor (sem os membros; ver as divergências do módulo).
    pub fn to_dynamic(&self) -> Dynamic {
        let result = Dynamic::object(Vec::<(&str, Dynamic)>::new());
        if matches!(self.config, Dynamic::Object(_)) {
            objects::mixin(&result, &self.config, false);
        }
        if let Dynamic::Object(object) = &result {
            object.freeze();
        }
        result
    }
}
