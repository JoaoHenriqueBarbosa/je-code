//! Porte de `src/vs/workbench/contrib/debug/browser/debugConfigurationManager.ts`: o `ConfigurationManager` (provedores de
//! configuração, `launch.json` por pasta, do workspace e do usuário, seleção da configuração e das configurações dinâmicas)
//! e as `Launch`.
//!
//! Notas de tradução:
//! - **Serviços.** Os parâmetros decorados do construtor são o [`ConfigurationManagerServices`]: os serviços reais do porte
//!   (`IWorkspaceContextService`, `IConfigurationService`, `StorageService`, `IExtensionService`, `IHistoryService`,
//!   `IUriIdentityService`, `IRemoteAgentService`, `IContextKeyService`, `LogService`) e dois recortes estreitos para o que o
//!   porte ainda não tem como contrato: [`ILaunchIo`] (`IFileService.readFile`, `ITextFileService.write`,
//!   `IEditorService.openEditor` com seleção, `IPreferencesService`) e [`IDynamicQuickInput`] (o `createQuickPick` das
//!   configurações dinâmicas).
//! - **`AbstractLaunch`, `Launch`, `WorkspaceLaunch` e `UserLaunch`** são a struct [`Launch`] com o [`LaunchKind`]: o
//!   comportamento comum é o mesmo código e o que difere (uri, nome, `getConfig`, `openConfigFile`, `hidden`,
//!   `__configurationTarget`) é o `match` na variante.
//! - **`IConfig`, `ICompound` e `IGlobalConfig`** são objetos JSON; os valores `Dynamic` do serviço de configuração são
//!   convertidos em [`dynamic_to_value`] e [`value_to_dynamic`].
//! - **`IAdapterManager`.** O que o gerenciador usa dele é o [`IManagerAdapterManager`] (o `AdapterManager` o implementa).
//! - `launchSchema` e o registro de esquemas JSON (`jsonRegistry.registerSchema`, `setCompoundSchemaValues`) NÃO foram
//!   portados: o registro de contribuições JSON não existe no porte (o esquema não influencia nenhum comportamento lógico
//!   do gerenciador).
//! - `Event.any(onDidChangeWorkspaceFolders, onDidChangeWorkbenchState)` é o `any` dos dois eventos mapeados para `()`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, sequence};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event, any};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides};
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::log::LogService;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, WorkspaceFolder};
use je_storage::StorageValue;
use serde_json::{Map, Value, json};

use crate::services::extensions::extensions::IExtensionService;
use crate::services::history::history::IHistoryService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

use super::debug::{DebugConfigurationProviderTriggerKind, IConfig, IConfigurationManager, IDebugConfigurationProvider, ILaunch};
use super::debugger::Debugger;
use super::debug_utils::{get_effective_config_for_platform, get_visible_and_sorted};

const DEBUG_SELECTED_CONFIG_NAME_KEY: &str = "debug.selectedconfigname";
const DEBUG_SELECTED_ROOT: &str = "debug.selectedroot";
// Debug type is only stored if a dynamic configuration is used for better restore
const DEBUG_SELECTED_TYPE: &str = "debug.selectedtype";
const DEBUG_RECENT_DYNAMIC_CONFIGURATIONS: &str = "debug.recentdynamicconfigurations";
const ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME: &str = "onDebugDynamicConfigurations";

/// `CONTEXT_DEBUG_CONFIGURATION_TYPE`.
pub fn context_debug_configuration_type() -> RawContextKey {
    RawContextKey::new("debugConfigurationType", Dynamic::Undefined, ContextKeyMeta::Hide)
}

// ---- conversões Dynamic <-> JSON

/// O `Dynamic` do serviço de configuração como objeto JSON.
pub fn dynamic_to_value(value: &Dynamic) -> Value {
    match value {
        Dynamic::Bool(flag) => Value::Bool(*flag),
        Dynamic::Number(number) => {
            if number.fract() == 0.0 && number.abs() < 9e15 {
                json!(*number as i64)
            } else {
                json!(*number)
            }
        }
        Dynamic::String(text) => Value::String(text.to_string_lossy()),
        Dynamic::Array(items) => Value::Array(items.to_vec().iter().map(dynamic_to_value).collect()),
        Dynamic::Object(object) => Value::Object(object.entries().iter().map(|(key, value)| (key.to_string_lossy(), dynamic_to_value(value))).collect()),
        _ => Value::Null,
    }
}

/// O objeto JSON como `Dynamic`.
pub fn value_to_dynamic(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(flag) => Dynamic::Bool(*flag),
        Value::Number(number) => Dynamic::Number(number.as_f64().unwrap_or(0.0)),
        Value::String(text) => Dynamic::string(text),
        Value::Array(items) => Dynamic::array(items.iter().map(value_to_dynamic)),
        Value::Object(object) => Dynamic::object(object.iter().map(|(key, value)| (key.clone(), value_to_dynamic(value)))),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(_) => true,
    }
}

// ---- contratos

/// `IGuessedDebugger.withConfig`.
#[derive(Clone)]
pub struct DynamicPick {
    pub launch: Rc<dyn ILaunchFull>,
    pub config: IConfig,
    pub label: String,
}

/// `IGuessedDebugger`.
#[derive(Clone, Default)]
pub struct GuessedDebugger {
    pub debugger: Option<Rc<Debugger>>,
    pub with_config: Option<DynamicPick>,
}

/// O que o gerenciador usa do `IAdapterManager`.
pub trait IManagerAdapterManager {
    fn activate_debuggers(&self, activation_event: &str, debug_type: Option<&str>) -> Promise<()>;
    fn get_debugger_label(&self, debug_type: &str) -> Option<String>;
    fn get_enabled_debugger(&self, debug_type: &str) -> Option<Rc<Debugger>>;
    fn guess_debugger(&self, getting_configurations: bool) -> Promise<Option<GuessedDebugger>>;
    fn on_did_debuggers_ext_point_read(&self) -> Event<()>;
}

/// As opções de `openConfigFile`.
#[derive(Clone, Debug, Default)]
pub struct OpenConfigFileOptions {
    pub preserve_focus: bool,
    pub r#type: Option<String>,
    pub suppress_initial_configs: Option<bool>,
    pub use_initial_configs: Option<bool>,
}

/// O resultado de `openConfigFile`: `{ editor, created }` (o editor é só "abriu").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenConfigFileResult {
    pub editor_opened: bool,
    pub created: bool,
}

/// `ILaunch` completo (o [`ILaunch`] de `debug.rs` é o recorte opaco que o serviço repassa).
pub trait ILaunchFull: ILaunch {
    fn uri(&self) -> URI;
    fn name(&self) -> String;
    fn workspace(&self) -> Option<WorkspaceFolder>;
    fn hidden(&self) -> bool;
    fn get_compound(&self, name: &str) -> Option<Value>;
    fn get_configuration_names(&self, ignore_compounds_and_presentation: bool) -> Vec<String>;
    fn get_configuration(&self, name: &str) -> Option<IConfig>;
    fn get_initial_configuration_content(&self, folder_uri: Option<URI>, debug_type: Option<String>, use_initial_configs: bool, token: Option<CancellationToken>) -> Promise<String>;
    fn open_config_file(&self, options: OpenConfigFileOptions, token: Option<CancellationToken>) -> Promise<OpenConfigFileResult>;
}

/// `IFileService.readFile`, `ITextFileService.write`, `IEditorService.openEditor` e `IPreferencesService`.
pub trait ILaunchIo {
    /// O conteúdo do arquivo; erro se não existe.
    fn read_file(&self, resource: &URI) -> Promise<String>;
    fn write_text(&self, resource: &URI, content: &str) -> Promise<()>;
    /// `openEditor({ resource, options: { selection, preserveFocus, pinned, revealIfVisible } }, ACTIVE_GROUP)`: se abriu.
    fn open_editor(&self, resource: &URI, selection: Option<(i64, i64)>, preserve_focus: bool, pinned: bool) -> Promise<bool>;
    /// `preferencesService.userSettingsResource`.
    fn user_settings_resource(&self) -> URI;
    /// `preferencesService.openUserSettings({ jsonEditor: true, preserveFocus, revealSetting: { key: 'launch' } })`.
    fn open_user_settings(&self, preserve_focus: bool) -> Promise<bool>;
}

/// Um item do quick pick de configurações dinâmicas (`IDynamicPickItem`).
#[derive(Clone)]
pub struct DynamicPickItem {
    pub label: String,
    pub description: Option<String>,
    pub launch: Rc<dyn ILaunchFull>,
    pub config: IConfig,
}

/// O quick pick de `getDynamicProviders().pick()` (`createQuickPick<IDynamicPickItem>()`).
pub trait IDynamicQuickPick: IDisposable {
    fn set_busy(&self, busy: bool);
    fn set_placeholder(&self, placeholder: &str);
    fn set_items(&self, items: Vec<DynamicPickItem>);
    fn show(&self);
    /// `onDidAccept`: o item ativo (`input.activeItems[0]`).
    fn on_did_accept(&self) -> Event<Option<DynamicPickItem>>;
    /// `onDidTriggerItemButton`: o item do botão de fixar.
    fn on_did_trigger_item_button(&self) -> Event<DynamicPickItem>;
    fn on_did_hide(&self) -> Event<()>;
}

pub trait IDynamicQuickInput {
    fn create_quick_pick(&self) -> Rc<dyn IDynamicQuickPick>;
}

/// Os serviços do construtor do `ConfigurationManager`.
#[derive(Clone)]
pub struct ConfigurationManagerServices {
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub quick_input: Rc<dyn IDynamicQuickInput>,
    pub storage_service: Rc<dyn StorageService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub history_service: Rc<dyn IHistoryService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub log_service: Rc<dyn LogService>,
    pub launch_io: Rc<dyn ILaunchIo>,
}

// ---- Launch

/// As três variantes de `AbstractLaunch`.
#[derive(Clone)]
pub enum LaunchKind {
    /// `Launch`: o `launch.json` de uma pasta do workspace.
    Folder(WorkspaceFolder),
    /// `WorkspaceLaunch`: o `launch` do arquivo do workspace.
    Workspace,
    /// `UserLaunch`: o `launch` das configurações do usuário.
    User,
}

/// `AbstractLaunch` + `Launch` + `WorkspaceLaunch` + `UserLaunch`.
pub struct Launch {
    configuration_manager: Weak<ConfigurationManager>,
    adapter_manager: Rc<dyn IManagerAdapterManager>,
    services: ConfigurationManagerServices,
    kind: LaunchKind,
}

impl Launch {
    fn manager(&self) -> Rc<ConfigurationManager> {
        self.configuration_manager.upgrade().expect("o gerenciador está vivo")
    }

    /// `getConfig()`: o valor de `launch` do escopo desta `Launch`.
    fn get_config(&self) -> Option<Value> {
        let value = match &self.kind {
            LaunchKind::Folder(workspace) => {
                let overrides = ConfigurationOverrides { resource: Some(workspace.uri.clone()), ..ConfigurationOverrides::default() };
                self.services.configuration_service.inspect("launch", &overrides).workspace_folder_value()
            }
            LaunchKind::Workspace => self.services.configuration_service.inspect("launch", &ConfigurationOverrides::default()).workspace_value(),
            LaunchKind::User => self.services.configuration_service.inspect("launch", &ConfigurationOverrides::default()).user_value(),
        };
        match value {
            Dynamic::Undefined | Dynamic::Null => None,
            value => Some(dynamic_to_value(&value)),
        }
    }

    fn get_deduplicated_config(&self) -> Option<Value> {
        let original = self.get_config()?;
        let named = |list: Option<&Value>| -> Vec<Value> {
            list.and_then(Value::as_array)
                .map(|items| items.iter().filter(|item| item.get("name").is_some_and(Value::is_string)).cloned().collect())
                .unwrap_or_default()
        };
        let compounds = named(original.get("compounds"));
        let configurations = named(original.get("configurations"));
        Some(json!({
            "version": original.get("version").cloned().unwrap_or(Value::Null),
            "compounds": distinguish_configs_by_name(&compounds),
            "configurations": distinguish_configs_by_name(&configurations),
        }))
    }

    /// `writeConfiguration(configuration)` (só a `Launch` de pasta).
    pub fn write_configuration(&self, configuration: IConfig) -> Promise<()> {
        let LaunchKind::Folder(workspace) = &self.kind else {
            return Promise::rejected(Error::new("writeConfiguration só existe na Launch de pasta"));
        };
        // note: we don't get the deduplicated config since we don't want that to 'leak' into the file
        let mut full_config = self.get_config().and_then(|c| c.as_object().cloned()).unwrap_or_default();
        let mut configurations = full_config.get("configurations").and_then(Value::as_array).cloned().unwrap_or_default();
        configurations.push(configuration);
        full_config.insert("configurations".to_owned(), Value::Array(configurations));
        self.services.configuration_service.update_value(
            "launch",
            value_to_dynamic(&Value::Object(full_config)),
            Some(ConfigurationUpdateOverrides { override_identifiers: None, resource: Some(workspace.uri.clone()) }),
            Some(ConfigurationTarget::WorkspaceFolder),
        )
    }
}

fn distinguish_configs_by_name(things: &[Value]) -> Value {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    Value::Array(
        things
            .iter()
            .map(|thing| {
                let name = thing.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
                let no = seen.get(&name).copied().unwrap_or(0);
                seen.insert(name.clone(), no + 1);
                if no == 0 {
                    thing.clone()
                } else {
                    let mut renamed = thing.as_object().cloned().unwrap_or_default();
                    renamed.insert("name".to_owned(), Value::String(format!("{name} ({no})")));
                    Value::Object(renamed)
                }
            })
            .collect(),
    )
}

impl ILaunch for Launch {}

impl ILaunchFull for Launch {
    fn uri(&self) -> URI {
        match &self.kind {
            LaunchKind::Folder(workspace) => je_platform::resources::ext_uri().join_path(&workspace.uri, &["/.vscode/launch.json"]),
            LaunchKind::Workspace => self.services.context_service.get_workspace().configuration().expect("o arquivo do workspace"),
            LaunchKind::User => self.services.launch_io.user_settings_resource(),
        }
    }

    fn name(&self) -> String {
        match &self.kind {
            LaunchKind::Folder(workspace) => workspace.name.clone(),
            LaunchKind::Workspace => localize("workspace", "workspace", &[]),
            LaunchKind::User => localize("user settings", "user settings", &[]),
        }
    }

    fn workspace(&self) -> Option<WorkspaceFolder> {
        match &self.kind {
            LaunchKind::Folder(workspace) => Some(workspace.clone()),
            _ => None,
        }
    }

    fn hidden(&self) -> bool {
        matches!(self.kind, LaunchKind::User)
    }

    fn get_compound(&self, name: &str) -> Option<Value> {
        let config = self.get_deduplicated_config()?;
        config.get("compounds")?.as_array()?.iter().find(|compound| compound.get("name").and_then(Value::as_str) == Some(name)).cloned()
    }

    fn get_configuration_names(&self, ignore_compounds_and_presentation: bool) -> Vec<String> {
        let Some(config) = self.get_deduplicated_config() else {
            return Vec::new();
        };
        let configurations_list = config.get("configurations").and_then(Value::as_array).cloned();
        let compounds_list = config.get("compounds").and_then(Value::as_array).cloned();
        if configurations_list.is_none() && compounds_list.is_none() {
            return Vec::new();
        }

        let mut configurations: Vec<Value> = Vec::new();
        if let Some(list) = &configurations_list {
            configurations.extend(list.iter().filter(|cfg| cfg.get("name").is_some_and(Value::is_string)).cloned());
        }
        if ignore_compounds_and_presentation {
            return configurations.iter().map(|c| c["name"].as_str().unwrap_or_default().to_owned()).collect();
        }

        if let Some(list) = &compounds_list {
            configurations.extend(
                list.iter()
                    .filter(|compound| {
                        compound.get("name").is_some_and(Value::is_string) && compound.get("configurations").and_then(Value::as_array).is_some_and(|c| !c.is_empty())
                    })
                    .cloned(),
            );
        }
        let target_os = self.manager().get_target_operating_system();
        let resolved: Vec<Value> = configurations
            .iter()
            .map(|c| if !truthy(c.get("configurations")) { get_effective_config_for_platform(c, Some(target_os)) } else { c.clone() })
            .collect();
        get_visible_and_sorted(&resolved).iter().map(|c| c["name"].as_str().unwrap_or_default().to_owned()).collect()
    }

    fn get_configuration(&self, name: &str) -> Option<IConfig> {
        // We need to clone the configuration in order to be able to make changes to it #42198
        let config = self.get_deduplicated_config()?;
        let configuration = config.get("configurations")?.as_array()?.iter().find(|c| c.get("name").and_then(Value::as_str) == Some(name))?.clone();

        let effective_configuration = get_effective_config_for_platform(&configuration, Some(self.manager().get_target_operating_system()));
        let target = match &self.kind {
            LaunchKind::User => ConfigurationTarget::User,
            LaunchKind::Workspace => ConfigurationTarget::Workspace,
            LaunchKind::Folder(_) => ConfigurationTarget::WorkspaceFolder,
        };
        let mut result = effective_configuration.as_object().cloned().unwrap_or_default();
        result.insert("__configurationTarget".to_owned(), json!(target as i64));
        Some(Value::Object(result))
    }

    fn get_initial_configuration_content(&self, folder_uri: Option<URI>, debug_type: Option<String>, use_initial_configs: bool, token: Option<CancellationToken>) -> Promise<String> {
        let adapter_manager = self.adapter_manager.clone();
        let manager = self.manager();
        async_fn(async move {
            let mut content = String::new();
            let adapter: Option<GuessedDebugger> = match debug_type {
                Some(debug_type) => Some(GuessedDebugger { debugger: adapter_manager.get_enabled_debugger(&debug_type), with_config: None }),
                None => adapter_manager.guess_debugger(true).await?,
            };
            if let Some(adapter) = adapter {
                if let (Some(with_config), Some(debugger)) = (&adapter.with_config, &adapter.debugger) {
                    content = debugger.get_initial_configuration_content(Some(vec![with_config.config.clone()])).await?;
                } else if let Some(debugger) = &adapter.debugger {
                    let initial_configs = if use_initial_configs {
                        manager.provide_debug_configurations(folder_uri, &debugger.debug_type(), token.unwrap_or(CancellationToken::None)).await?
                    } else {
                        Vec::new()
                    };
                    content = debugger.get_initial_configuration_content(Some(initial_configs)).await?;
                }
            }
            Ok(content)
        })
    }

    fn open_config_file(&self, options: OpenConfigFileOptions, token: Option<CancellationToken>) -> Promise<OpenConfigFileResult> {
        let this = self.manager().launch_rc(self);
        let io = self.services.launch_io.clone();
        async_fn(async move {
            match &this.kind {
                LaunchKind::Folder(workspace) => {
                    let resource = this.uri();
                    let mut created = false;
                    let content = match io.read_file(&resource).await {
                        Ok(content) => content,
                        Err(_) => {
                            // launch.json not found: create one by collecting launch configs from debugConfigProviders
                            let content = this
                                .get_initial_configuration_content(Some(workspace.uri.clone()), options.r#type.clone(), !options.suppress_initial_configs.unwrap_or(false), token)
                                .await?;
                            if content.is_empty() {
                                // Cancelled
                                return Ok(OpenConfigFileResult { editor_opened: false, created: false });
                            }
                            created = true; // pin only if config file is created #8727
                            if let Err(error) = io.write_text(&resource, &content).await {
                                return Err(Error::new(localize(
                                    "DebugConfig.failed",
                                    "Unable to create 'launch.json' file inside the '.vscode' folder ({0}).",
                                    &[&error.message],
                                )));
                            }
                            content
                        }
                    };

                    let selected_name = this.manager().selected_configuration().name;
                    let index = content.find(&format!("\"{}\"", selected_name.unwrap_or_else(|| "undefined".to_owned())));
                    let mut start_line_number = 1;
                    if let Some(index) = index {
                        for c in content[..index].chars() {
                            if c == '\n' {
                                start_line_number += 1;
                            }
                        }
                    }
                    let selection = if start_line_number > 1 { Some((start_line_number, 4)) } else { None };
                    let opened = io.open_editor(&resource, selection, options.preserve_focus, created).await?;
                    Ok(OpenConfigFileResult { editor_opened: opened, created })
                }
                LaunchKind::Workspace => {
                    let launch_exist_in_file = this.get_config().is_some();
                    if !launch_exist_in_file {
                        // Launch property in workspace config not found: create one by collecting launch configs from debugConfigProviders
                        let content = this
                            .get_initial_configuration_content(None, options.r#type.clone(), options.use_initial_configs.unwrap_or(false), token)
                            .await?;
                        if !content.is_empty() {
                            let units: Vec<u16> = content.encode_utf16().collect();
                            let parsed = je_base::jsonc::parse(&units).map_err(|error| Error::new(format!("{error:?}")))?;
                            this.services
                                .configuration_service
                                .update_value("launch", je_platform::js::from_json(&parsed), None, Some(ConfigurationTarget::Workspace))
                                .await?;
                        } else {
                            return Ok(OpenConfigFileResult { editor_opened: false, created: false });
                        }
                    }

                    let resource = this.services.context_service.get_workspace().configuration().expect("o arquivo do workspace");
                    let opened = io.open_editor(&resource, None, options.preserve_focus, false).await?;
                    Ok(OpenConfigFileResult { editor_opened: opened, created: false })
                }
                LaunchKind::User => {
                    let opened = io.open_user_settings(options.preserve_focus).await?;
                    Ok(OpenConfigFileResult { editor_opened: opened, created: false })
                }
            }
        })
    }
}

// ---- ConfigurationManager

/// `selectedConfiguration`.
#[derive(Clone)]
pub struct SelectedConfiguration {
    pub launch: Option<Rc<dyn ILaunchFull>>,
    pub name: Option<String>,
    pub get_config: Rc<dyn Fn() -> Promise<Option<IConfig>>>,
    pub r#type: Option<String>,
}

/// O resultado de `getAllConfigurations`.
#[derive(Clone)]
pub struct ConfigurationEntry {
    pub launch: Rc<dyn ILaunchFull>,
    pub name: String,
    pub presentation: Option<Value>,
}

/// O que `getDynamicProviders` devolve para cada tipo.
#[derive(Clone)]
pub struct DynamicProvider {
    pub label: String,
    pub r#type: String,
    pub manager: Weak<ConfigurationManager>,
}

impl DynamicProvider {
    /// `getProvider()`.
    pub fn get_provider(&self) -> Promise<Option<Rc<IDebugConfigurationProvider>>> {
        let manager = self.manager.upgrade().expect("o gerenciador está vivo");
        let debug_type = self.r#type.clone();
        async_fn(async move {
            manager.adapter_manager.activate_debuggers(ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME, Some(&debug_type)).await?;
            Ok(manager
                .config_providers
                .borrow()
                .iter()
                .find(|p| p.r#type == debug_type && p.trigger_kind == DebugConfigurationProviderTriggerKind::Dynamic && p.provide_debug_configurations.is_some())
                .cloned())
        })
    }

    /// `pick()`.
    pub fn pick(&self) -> Promise<Option<DynamicPick>> {
        let manager = self.manager.upgrade().expect("o gerenciador está vivo");
        let debug_type = self.r#type.clone();
        async_fn(async move {
            // Do a late 'onDebugDynamicConfigurationsName' activation so extensions are not activated too early #108578
            manager.adapter_manager.activate_debuggers(ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME, Some(&debug_type)).await?;

            let disposables = DisposableStore::new();
            let token = Rc::new(CancellationTokenSource::new(None));
            disposables.add_rc(token.clone());
            let input = manager.services.quick_input.create_quick_pick();
            disposables.add(input.clone());
            input.set_busy(true);
            input.set_placeholder(&localize("selectConfiguration", "Select Launch Configuration", &[]));
            let (chosen_promise, resolver) = Promise::<Option<DynamicPickItem>>::new();
            {
                let accept_resolver = resolver.clone();
                disposables.add(input.on_did_accept().listen(move |item| accept_resolver.resolve(item.clone())));
                let pin_manager = manager.clone();
                let pin_resolver = resolver.clone();
                disposables.add(input.on_did_trigger_item_button().listen(move |item| {
                    pin_resolver.resolve(None);
                    let manager = pin_manager.clone();
                    let item = item.clone();
                    async_fn(async move {
                        let DynamicPickItem { launch, config, .. } = item;
                        launch
                            .open_config_file(
                                OpenConfigFileOptions { preserve_focus: false, r#type: config.get("type").and_then(Value::as_str).map(str::to_owned), suppress_initial_configs: Some(true), use_initial_configs: None },
                                None,
                            )
                            .await?;
                        // Only Launch have a pin trigger button
                        if let Some(launch) = manager.launch_rc_by_ptr(&launch) {
                            launch.write_configuration(config.clone()).await?;
                        }
                        let name = config.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
                        manager.select_configuration(Some(launch), Some(name.clone()), None, None).await?;
                        manager.remove_recent_dynamic_configurations(&name, config.get("type").and_then(Value::as_str).unwrap_or_default());
                        Ok(())
                    });
                }));
                disposables.add(input.on_did_hide().listen(move |_| resolver.resolve(None)));
            }
            let cancel_token = token.clone();
            let chosen_promise = chosen_promise.finally(move || cancel_token.cancel());

            // This await invokes the extension providers, which might fail due to several reasons, therefore we gate this logic
            // under a try/catch to prevent leaving the Debug Tab selector in a borked state.
            let items = match manager.get_dynamic_configurations_by_type(&debug_type, token.token()).await {
                Ok(items) => items,
                Err(err) => {
                    manager.services.log_service.error(&err.message);
                    disposables.dispose();
                    return Ok(None);
                }
            };
            input.set_items(items);
            input.set_busy(false);
            input.show();
            let chosen = chosen_promise.await?;
            disposables.dispose();
            Ok(chosen.map(|item| DynamicPick { label: item.label, launch: item.launch, config: item.config }))
        })
    }
}

/// `ConfigurationManager`.
pub struct ConfigurationManager {
    adapter_manager: Rc<dyn IManagerAdapterManager>,
    services: ConfigurationManagerServices,
    launches: RefCell<Vec<Rc<Launch>>>,
    selected_name: RefCell<Option<String>>,
    selected_launch: RefCell<Option<Rc<Launch>>>,
    get_selected_config: RefCell<Rc<dyn Fn() -> Promise<Option<IConfig>>>>,
    selected_type: RefCell<Option<String>>,
    selected_dynamic: Cell<bool>,
    to_dispose: Rc<DisposableStore>,
    on_did_select_configuration_name: Rc<Emitter<()>>,
    config_providers: RefCell<Vec<Rc<IDebugConfigurationProvider>>>,
    debug_configuration_type_context: Rc<dyn IContextKey>,
    on_did_change_configuration_providers: Rc<Emitter<()>>,
    target_operating_system: Cell<OperatingSystem>,
    me: Weak<ConfigurationManager>,
}

impl ConfigurationManager {
    pub fn new(adapter_manager: Rc<dyn IManagerAdapterManager>, services: ConfigurationManagerServices) -> Rc<Self> {
        let to_dispose = DisposableStore::new();
        let on_did_change_configuration_providers = Emitter::new();
        let on_did_select_configuration_name = Emitter::new();
        to_dispose.add(on_did_change_configuration_providers.clone());
        to_dispose.add(on_did_select_configuration_name.clone());
        let debug_configuration_type_context = context_debug_configuration_type().bind_to(&*services.context_key_service);

        let this = Rc::new_cyclic(|me| Self {
            adapter_manager,
            services,
            launches: RefCell::new(Vec::new()),
            selected_name: RefCell::new(None),
            selected_launch: RefCell::new(None),
            get_selected_config: RefCell::new(Rc::new(|| Promise::resolved(None))),
            selected_type: RefCell::new(None),
            selected_dynamic: Cell::new(false),
            to_dispose,
            on_did_select_configuration_name,
            config_providers: RefCell::new(Vec::new()),
            debug_configuration_type_context,
            on_did_change_configuration_providers,
            target_operating_system: Cell::new(Platform::detect().os),
            me: me.clone(),
        });
        this.init_launches();
        this.register_listeners();

        let storage = &this.services.storage_service;
        let previous_selected_root = storage.get(DEBUG_SELECTED_ROOT, StorageScope::Workspace, None);
        let previous_selected_type = storage.get(DEBUG_SELECTED_TYPE, StorageScope::Workspace, None).filter(|t| !t.is_empty());
        let previous_selected_launch = this.launches.borrow().iter().find(|l| Some(l.uri().to_string(false).to_string_lossy()) == previous_selected_root).cloned();
        let previous_selected_name = storage.get(DEBUG_SELECTED_CONFIG_NAME_KEY, StorageScope::Workspace, None);

        let dynamic_config = previous_selected_type.map(|t| t);
        if let Some(previous_selected_launch) = previous_selected_launch.filter(|l| !l.get_configuration_names(false).is_empty()) {
            this.select_configuration(Some(previous_selected_launch as Rc<dyn ILaunchFull>), previous_selected_name, None, dynamic_config);
        } else if !this.launches.borrow().is_empty() {
            this.select_configuration(None, previous_selected_name, None, dynamic_config);
        }

        this.resolve_target_operating_system();
        this
    }

    fn this(&self) -> Rc<ConfigurationManager> {
        self.me.upgrade().expect("o gerenciador está vivo")
    }

    /// O `Rc` de uma `Launch` já guardada (as `Launch` chamam o gerenciador por `&self`).
    fn launch_rc(&self, launch: &Launch) -> Rc<Launch> {
        self.launches.borrow().iter().find(|l| std::ptr::eq(Rc::as_ptr(l), launch)).cloned().expect("a Launch pertence ao gerenciador")
    }

    fn launch_rc_by_ptr(&self, launch: &Rc<dyn ILaunchFull>) -> Option<Rc<Launch>> {
        self.launches.borrow().iter().find(|l| std::ptr::addr_eq(Rc::as_ptr(l), Rc::as_ptr(launch))).cloned()
    }

    fn resolve_target_operating_system(&self) {
        let this = self.this();
        drop(self.services.remote_agent_service.get_environment().then_result(move |result| {
            // Ignore remote environment failures and fall back to the local OS.
            if let Ok(environment) = result {
                let target_operating_system = environment.map(|e| e.os).unwrap_or(Platform::detect().os);
                if this.target_operating_system.get() != target_operating_system {
                    this.target_operating_system.set(target_operating_system);
                    this.on_did_select_configuration_name.fire(());
                }
            }
            Ok(())
        }));
    }

    pub fn get_target_operating_system(&self) -> OperatingSystem {
        self.target_operating_system.get()
    }

    pub fn on_did_change_configuration_providers(&self) -> Event<()> {
        self.on_did_change_configuration_providers.event()
    }

    pub fn register_debug_configuration_provider(&self, debug_configuration_provider: Rc<IDebugConfigurationProvider>) -> DisposableRef {
        self.config_providers.borrow_mut().push(debug_configuration_provider.clone());
        self.on_did_change_configuration_providers.fire(());
        let this = self.this();
        to_disposable(move || {
            this.unregister_debug_configuration_provider(&debug_configuration_provider);
            this.on_did_change_configuration_providers.fire(());
        })
    }

    pub fn unregister_debug_configuration_provider(&self, debug_configuration_provider: &Rc<IDebugConfigurationProvider>) {
        let ix = self.config_providers.borrow().iter().position(|p| Rc::ptr_eq(p, debug_configuration_provider));
        if let Some(ix) = ix {
            self.config_providers.borrow_mut().remove(ix);
        }
    }

    /// If scope is not specified, a value of `DebugConfigurationProvideTrigger.Initial` is assumed.
    pub fn has_debug_configuration_provider(&self, debug_type: &str, trigger_kind: Option<DebugConfigurationProviderTriggerKind>) -> bool {
        let trigger_kind = trigger_kind.unwrap_or(DebugConfigurationProviderTriggerKind::Initial);

        // check if there are providers for the given type that contribute a provideDebugConfigurations method
        self.config_providers.borrow().iter().any(|p| p.provide_debug_configurations.is_some() && p.r#type == debug_type && p.trigger_kind == trigger_kind)
    }

    pub fn resolve_configuration_by_providers(self: &Rc<Self>, folder_uri: Option<URI>, debug_type: Option<String>, config: IConfig, token: CancellationToken) -> Promise<Option<IConfig>> {
        let this = self.clone();
        async_fn(async move {
            let resolve_debug_configuration_for_type = |kind: Option<String>, config: Option<IConfig>| {
                let this = this.clone();
                let folder_uri = folder_uri.clone();
                let token = token.clone();
                async_fn(async move {
                    if kind.as_deref() != Some("*") {
                        this.adapter_manager.activate_debuggers("onDebugResolve", kind.as_deref()).await?;
                    }
                    let mut config = config;
                    let providers: Vec<Rc<IDebugConfigurationProvider>> = this.config_providers.borrow().clone();
                    for p in providers {
                        if p.r#type == kind.clone().unwrap_or_default() && p.resolve_debug_configuration.is_some() && config.is_some() {
                            let resolve = p.resolve_debug_configuration.clone().expect("o resolvedor");
                            config = resolve(folder_uri.clone(), config.take().expect("a configuração"), token.clone()).await?;
                        }
                    }
                    Ok(config)
                })
            };

            let mut resolved_type: Option<String> = config.get("type").and_then(Value::as_str).map(str::to_owned).or(debug_type.clone());
            let mut result: Option<IConfig> = Some(config);
            let mut seen: HashSet<Option<String>> = HashSet::new();
            while result.is_some() && !seen.contains(&resolved_type) {
                seen.insert(resolved_type.clone());
                result = resolve_debug_configuration_for_type(resolved_type.clone(), result).await?;
                result = resolve_debug_configuration_for_type(Some("*".to_owned()), result).await?;
                resolved_type = result.as_ref().and_then(|r| r.get("type")).and_then(Value::as_str).map(str::to_owned).or(debug_type.clone());
            }
            Ok(result)
        })
    }

    pub fn resolve_debug_configuration_with_substituted_variables(
        self: &Rc<Self>,
        folder_uri: Option<URI>,
        debug_type: Option<String>,
        config: IConfig,
        token: CancellationToken,
    ) -> Promise<Option<IConfig>> {
        // pipe the config through the promises sequentially. Append at the end the '*' types
        let wanted = debug_type.unwrap_or_default();
        let providers: Vec<Rc<IDebugConfigurationProvider>> = {
            let all = self.config_providers.borrow();
            all.iter()
                .filter(|p| p.r#type == wanted && p.resolve_debug_configuration_with_substituted_variables.is_some())
                .chain(all.iter().filter(|p| p.r#type == "*" && p.resolve_debug_configuration_with_substituted_variables.is_some()))
                .cloned()
                .collect()
        };
        let result: Rc<RefCell<Option<IConfig>>> = Rc::new(RefCell::new(Some(config)));
        let factories: Vec<Box<dyn FnOnce() -> Promise<Option<()>>>> = providers
            .into_iter()
            .map(|provider| {
                let result = result.clone();
                let folder_uri = folder_uri.clone();
                let token = token.clone();
                Box::new(move || {
                    async_fn(async move {
                        // If any provider returned undefined or null make sure to respect that and do not pass the result to more resolver
                        let current = result.borrow().clone();
                        if let Some(current) = current {
                            let resolve = provider.resolve_debug_configuration_with_substituted_variables.clone().expect("o resolvedor");
                            let resolved = resolve(folder_uri, current, token).await?;
                            *result.borrow_mut() = resolved;
                        }
                        Ok(None)
                    })
                }) as Box<dyn FnOnce() -> Promise<Option<()>>>
            })
            .collect();
        let sequenced = sequence(factories);
        async_fn(async move {
            sequenced.await?;
            let resolved = result.borrow().clone();
            Ok(resolved)
        })
    }

    pub fn provide_debug_configurations(self: &Rc<Self>, folder_uri: Option<URI>, debug_type: &str, token: CancellationToken) -> Promise<Vec<IConfig>> {
        let this = self.clone();
        let debug_type = debug_type.to_owned();
        async_fn(async move {
            this.adapter_manager.activate_debuggers("onDebugInitialConfigurations", None).await?;

            let pending: Vec<Promise<Vec<IConfig>>> = this
                .config_providers
                .borrow()
                .iter()
                .filter(|p| p.r#type == debug_type && p.trigger_kind == DebugConfigurationProviderTriggerKind::Initial && p.provide_debug_configurations.is_some())
                .map(|p| (p.provide_debug_configurations.clone().expect("o provedor"))(folder_uri.clone(), token.clone()))
                .collect();
            let results = Promise::all(pending).await?;
            Ok(results.into_iter().flatten().collect())
        })
    }

    pub fn get_dynamic_providers(self: &Rc<Self>) -> Promise<Vec<DynamicProvider>> {
        let this = self.clone();
        async_fn(async move {
            this.services.extension_service.when_installed_extensions_registered().await?;
            let mut debug_dynamic_extensions_types: Vec<String> = Vec::new();
            let mut add = |t: String| {
                if !debug_dynamic_extensions_types.contains(&t) {
                    debug_dynamic_extensions_types.push(t);
                }
            };
            for e in this.services.extension_service.extensions() {
                let Some(activation_events) = e.manifest.activation_events.clone() else {
                    continue;
                };
                let mut explicit_types: Vec<String> = Vec::new();
                let mut has_generic_event = false;
                for event in &activation_events {
                    if event == ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME {
                        has_generic_event = true;
                    } else if let Some(t) = event.strip_prefix(&format!("{ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME}:")) {
                        explicit_types.push(t.to_owned());
                    }
                }

                if !explicit_types.is_empty() {
                    explicit_types.into_iter().for_each(&mut add);
                } else if has_generic_event {
                    let debugger_type = e.manifest.contributes.as_ref().and_then(|c| c.debuggers.as_ref()).and_then(|d| d.first()).and_then(|d| d.r#type.clone());
                    if let Some(debugger_type) = debugger_type.filter(|t| !t.is_empty()) {
                        add(debugger_type);
                    }
                }
            }

            for config_provider in this.config_providers.borrow().iter() {
                if config_provider.trigger_kind == DebugConfigurationProviderTriggerKind::Dynamic {
                    add(config_provider.r#type.clone());
                }
            }

            Ok(debug_dynamic_extensions_types
                .into_iter()
                .map(|t| DynamicProvider { label: this.adapter_manager.get_debugger_label(&t).unwrap_or_default(), r#type: t, manager: Rc::downgrade(&this) })
                .collect())
        })
    }

    pub fn get_dynamic_configurations_by_type(self: &Rc<Self>, debug_type: &str, token: CancellationToken) -> Promise<Vec<DynamicPickItem>> {
        let this = self.clone();
        let debug_type = debug_type.to_owned();
        async_fn(async move {
            // Do a late 'onDebugDynamicConfigurationsName' activation so extensions are not activated too early #108578
            this.adapter_manager.activate_debuggers(ON_DEBUG_DYNAMIC_CONFIGURATIONS_NAME, Some(&debug_type)).await?;

            let mut picks: Vec<Promise<Vec<DynamicPickItem>>> = Vec::new();
            let provider = this
                .config_providers
                .borrow()
                .iter()
                .find(|p| p.r#type == debug_type && p.trigger_kind == DebugConfigurationProviderTriggerKind::Dynamic && p.provide_debug_configurations.is_some())
                .cloned();
            for launch in this.get_launches() {
                if let Some(provider) = &provider {
                    let provide = provider.provide_debug_configurations.clone().expect("o provedor");
                    let launch_for_items = launch.clone();
                    picks.push(provide(launch.workspace().map(|w| w.uri), token.clone()).then(move |configurations| {
                        configurations
                            .into_iter()
                            .map(|config| DynamicPickItem {
                                label: config.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                                description: Some(launch_for_items.name()),
                                config,
                                launch: launch_for_items.clone(),
                            })
                            .collect()
                    }));
                }
            }

            Ok(Promise::all(picks).await?.into_iter().flatten().collect())
        })
    }

    pub fn get_all_configurations(&self) -> Vec<ConfigurationEntry> {
        let mut all: Vec<ConfigurationEntry> = Vec::new();
        for l in self.get_launches() {
            for name in l.get_configuration_names(false) {
                let config = l.get_configuration(&name).or_else(|| l.get_compound(&name));
                if let Some(config) = config {
                    all.push(ConfigurationEntry { launch: l.clone(), name, presentation: config.get("presentation").cloned() });
                }
            }
        }

        let as_values: Vec<Value> = all
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let mut object = Map::new();
                object.insert("name".to_owned(), json!(entry.name));
                if let Some(presentation) = &entry.presentation {
                    object.insert("presentation".to_owned(), presentation.clone());
                }
                object.insert("__index".to_owned(), json!(index));
                Value::Object(object)
            })
            .collect();
        get_visible_and_sorted(&as_values).iter().map(|v| all[v["__index"].as_u64().unwrap_or(0) as usize].clone()).collect()
    }

    pub fn remove_recent_dynamic_configurations(&self, name: &str, debug_type: &str) {
        let remaining: Vec<Value> = self
            .get_recent_dynamic_configurations()
            .into_iter()
            .filter(|c| c.0 != name || c.1 != debug_type)
            .map(|(name, debug_type)| json!({ "name": name, "type": debug_type }))
            .collect();
        self.services.storage_service.store(DEBUG_RECENT_DYNAMIC_CONFIGURATIONS, StorageValue::String(Value::Array(remaining).to_string()), StorageScope::Workspace, StorageTarget::Machine);
        if self.selected_configuration().name.as_deref() == Some(name) && self.selected_type.borrow().as_deref() == Some(debug_type) && self.selected_dynamic.get() {
            self.this().select_configuration(None, None, None, None);
        } else {
            self.on_did_select_configuration_name.fire(());
        }
    }

    /// `getRecentDynamicConfigurations()`: `(name, type)`.
    pub fn get_recent_dynamic_configurations(&self) -> Vec<(String, String)> {
        let text = self.services.storage_service.get(DEBUG_RECENT_DYNAMIC_CONFIGURATIONS, StorageScope::Workspace, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        serde_json::from_str::<Vec<Value>>(&text)
            .unwrap_or_default()
            .iter()
            .map(|c| (c["name"].as_str().unwrap_or_default().to_owned(), c["type"].as_str().unwrap_or_default().to_owned()))
            .collect()
    }

    fn register_listeners(&self) {
        let this = self.this();
        let weak = Rc::downgrade(&this);
        let folders = self.services.context_service.on_did_change_workspace_folders().map(|_| (), None);
        let state = self.services.context_service.on_did_change_workbench_state().map(|_| (), None);
        let changed = any(vec![folders, state]);
        let listener_weak = weak.clone();
        self.to_dispose.add(changed.listen(move |_| {
            if let Some(this) = listener_weak.upgrade() {
                this.init_launches();
                this.select_configuration(None, None, None, None);
            }
        }));

        let configuration_weak = weak.clone();
        self.to_dispose.add(self.services.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration("launch", None)
                && let Some(this) = configuration_weak.upgrade()
            {
                // A change happen in the launch.json. If there is already a launch configuration selected, do not change the selection.
                this.select_configuration(None, None, None, None);
            }
        }));
    }

    fn init_launches(&self) {
        let make = |kind: LaunchKind| {
            Rc::new(Launch { configuration_manager: self.me.clone(), adapter_manager: self.adapter_manager.clone(), services: self.services.clone(), kind })
        };
        let mut launches: Vec<Rc<Launch>> = self.services.context_service.get_workspace().folders().into_iter().map(|folder| make(LaunchKind::Folder(folder))).collect();
        if self.services.context_service.get_workbench_state() == WorkbenchState::Workspace {
            launches.push(make(LaunchKind::Workspace));
        }
        launches.push(make(LaunchKind::User));
        *self.launches.borrow_mut() = launches;

        let selected = self.selected_launch.borrow().clone();
        if let Some(selected) = selected
            && !self.launches.borrow().iter().any(|l| Rc::ptr_eq(l, &selected))
        {
            self.this().select_configuration(None, None, None, None);
        }
    }

    pub fn get_launches(&self) -> Vec<Rc<dyn ILaunchFull>> {
        self.launches.borrow().iter().map(|l| l.clone() as Rc<dyn ILaunchFull>).collect()
    }

    pub fn get_launch(&self, workspace_uri: Option<&URI>) -> Option<Rc<dyn ILaunchFull>> {
        let workspace_uri = workspace_uri?;
        let ext_uri = self.services.uri_identity_service.ext_uri();
        self.launches.borrow().iter().find(|l| l.workspace().is_some_and(|w| ext_uri.is_equal(Some(&w.uri), Some(workspace_uri), false))).map(|l| l.clone() as Rc<dyn ILaunchFull>)
    }

    pub fn selected_configuration(&self) -> SelectedConfiguration {
        SelectedConfiguration {
            launch: self.selected_launch.borrow().clone().map(|l| l as Rc<dyn ILaunchFull>),
            name: self.selected_name.borrow().clone(),
            get_config: self.get_selected_config.borrow().clone(),
            r#type: self.selected_type.borrow().clone(),
        }
    }

    pub fn on_did_select_configuration(&self) -> Event<()> {
        self.on_did_select_configuration_name.event()
    }

    pub fn get_workspace_launch(&self) -> Option<Rc<dyn ILaunchFull>> {
        if self.services.context_service.get_workbench_state() == WorkbenchState::Workspace {
            return self.launches.borrow().last().map(|l| l.clone() as Rc<dyn ILaunchFull>);
        }
        None
    }

    /// `selectConfiguration(launch, name, config, dynamicConfig)`; `dynamic_config` é o `type` do `{ type }`.
    pub fn select_configuration(self: &Rc<Self>, launch: Option<Rc<dyn ILaunchFull>>, name: Option<String>, config: Option<IConfig>, dynamic_config: Option<String>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let mut launch: Option<Rc<Launch>> = launch.and_then(|l| this.launch_rc_by_ptr(&l));
            if launch.is_none() {
                let root_uri = this.services.history_service.get_last_active_workspace_root(None, None);
                let found = this.get_launch(root_uri.as_ref()).and_then(|l| this.launch_rc_by_ptr(&l));
                launch = match found {
                    Some(found) if !found.get_configuration_names(false).is_empty() => Some(found),
                    other => {
                        let with_names = this.launches.borrow().iter().find(|l| !l.get_configuration_names(false).is_empty()).cloned();
                        with_names.or(other).or_else(|| this.launches.borrow().first().cloned())
                    }
                };
            }

            let previous_launch = this.selected_launch.borrow().clone();
            let previous_name = this.selected_name.borrow().clone();
            let previous_selected_dynamic = this.selected_dynamic.get();
            *this.selected_launch.borrow_mut() = launch.clone();
            if let Some(selected) = &launch {
                this.services.storage_service.store(DEBUG_SELECTED_ROOT, StorageValue::String(selected.uri().to_string(false).to_string_lossy()), StorageScope::Workspace, StorageTarget::Machine);
            } else {
                this.services.storage_service.remove(DEBUG_SELECTED_ROOT, StorageScope::Workspace);
            }

            let names = launch.as_ref().map(|l| l.get_configuration_names(false)).unwrap_or_default();
            {
                let manager = this.clone();
                let launch = launch.clone();
                let config = config.clone();
                *this.get_selected_config.borrow_mut() = Rc::new(move || {
                    let selected_name = manager.selected_name.borrow().clone();
                    let selected = selected_name.and_then(|n| launch.as_ref().and_then(|l| l.get_configuration(&n)));
                    Promise::resolved(selected.or_else(|| config.clone()))
                });
            }

            let mut config = config;
            let mut debug_type = config.as_ref().and_then(|c| c.get("type")).and_then(Value::as_str).map(str::to_owned);
            if name.as_ref().is_some_and(|n| names.contains(n)) {
                this.set_selected_launch_name(name.clone());
            } else if let Some(dynamic_type) = dynamic_config.clone().filter(|t| !t.is_empty()) {
                // We could not find the previously used name and config is not passed. We should get all dynamic configurations from providers
                // And potentially auto select the previously used dynamic configuration #96293
                debug_type = Some(dynamic_type.clone());
                if config.is_none() {
                    let providers: Vec<DynamicProvider> = this.get_dynamic_providers().await?.into_iter().filter(|p| p.r#type == dynamic_type).collect();
                    let launch_for_config = launch.clone();
                    let wanted_name = name.clone();
                    *this.get_selected_config.borrow_mut() = Rc::new(move || {
                        let providers = providers.clone();
                        let launch = launch_for_config.clone();
                        let wanted_name = wanted_name.clone();
                        async_fn(async move {
                            let activated: Vec<Promise<Option<Rc<IDebugConfigurationProvider>>>> = providers.iter().map(|p| p.get_provider()).collect();
                            let activated_providers = Promise::all(activated).await?;
                            let provider = activated_providers.into_iter().next().flatten();
                            if let (Some(provider), Some(launch)) = (provider, launch)
                                && let Some(workspace) = launch.workspace()
                            {
                                let token = CancellationTokenSource::new(None);
                                let provide = provider.provide_debug_configurations.clone().expect("o provedor");
                                let dynamic_configs = provide(Some(workspace.uri), token.token()).await?;
                                if let Some(found) = dynamic_configs.into_iter().find(|c| c.get("name").and_then(Value::as_str) == wanted_name.as_deref()) {
                                    return Ok(Some(found));
                                }
                            }
                            Ok(None)
                        })
                    });
                }
                this.set_selected_launch_name(name.clone());

                let mut recent_dynamic_providers = this.get_recent_dynamic_configurations();
                if let Some(name) = name.as_ref().filter(|n| !n.is_empty()) {
                    // We need to store the recently used dynamic configurations to be able to show them in UI #110009
                    recent_dynamic_providers.insert(0, (name.clone(), dynamic_type.clone()));
                    recent_dynamic_providers = je_base::arrays::distinct(&recent_dynamic_providers, |t| format!("{} : {}", t.0, t.1));
                    let stored: Vec<Value> = recent_dynamic_providers.iter().map(|(name, debug_type)| json!({ "name": name, "type": debug_type })).collect();
                    this.services.storage_service.store(DEBUG_RECENT_DYNAMIC_CONFIGURATIONS, StorageValue::String(Value::Array(stored).to_string()), StorageScope::Workspace, StorageTarget::Machine);
                }
            } else if this.selected_name.borrow().as_ref().is_none_or(|selected| !names.contains(selected)) {
                // We could not find the configuration to select, pick the first one, or reset the selection if there is no launch configuration
                let name_to_set = names.first().cloned();
                this.set_selected_launch_name(name_to_set);
            }

            let selected_name = this.selected_name.borrow().clone();
            if config.is_none()
                && let (Some(launch), Some(selected_name)) = (&launch, &selected_name)
            {
                config = launch.get_configuration(selected_name);
                debug_type = config.as_ref().and_then(|c| c.get("type")).and_then(Value::as_str).map(str::to_owned);
            }

            let selected_type = dynamic_config.clone().filter(|t| !t.is_empty()).or_else(|| config.as_ref().and_then(|c| c.get("type")).and_then(Value::as_str).map(str::to_owned));
            *this.selected_type.borrow_mut() = selected_type.clone();
            this.selected_dynamic.set(dynamic_config.is_some());
            // Only store the selected type if we are having a dynamic configuration. Otherwise restoring this configuration from
            // storage might be misindentified as a dynamic configuration
            if dynamic_config.is_some() {
                match &selected_type {
                    Some(selected_type) => this.services.storage_service.store(DEBUG_SELECTED_TYPE, StorageValue::String(selected_type.clone()), StorageScope::Workspace, StorageTarget::Machine),
                    None => this.services.storage_service.remove(DEBUG_SELECTED_TYPE, StorageScope::Workspace),
                }
            } else {
                this.services.storage_service.remove(DEBUG_SELECTED_TYPE, StorageScope::Workspace);
            }

            match debug_type.filter(|t| !t.is_empty()) {
                Some(debug_type) => this.debug_configuration_type_context.set(Dynamic::string(&debug_type)),
                None => this.debug_configuration_type_context.reset(),
            }

            let launch_changed = match (&this.selected_launch.borrow().clone(), &previous_launch) {
                (None, None) => false,
                (Some(a), Some(b)) => !Rc::ptr_eq(a, b),
                _ => true,
            };
            if launch_changed || *this.selected_name.borrow() != previous_name || previous_selected_dynamic != this.selected_dynamic.get() {
                this.on_did_select_configuration_name.fire(());
            }
            Ok(())
        })
    }

    fn set_selected_launch_name(&self, selected_name: Option<String>) {
        *self.selected_name.borrow_mut() = selected_name.clone();

        match selected_name.filter(|n| !n.is_empty()) {
            Some(selected_name) => self.services.storage_service.store(DEBUG_SELECTED_CONFIG_NAME_KEY, StorageValue::String(selected_name), StorageScope::Workspace, StorageTarget::Machine),
            None => self.services.storage_service.remove(DEBUG_SELECTED_CONFIG_NAME_KEY, StorageScope::Workspace),
        }
    }

    pub fn dispose(&self) {
        self.to_dispose.dispose();
    }
}

impl IDisposable for ConfigurationManager {
    fn dispose(&self) {
        ConfigurationManager::dispose(self);
    }
}

impl IConfigurationManager for ConfigurationManager {
    fn register_debug_configuration_provider(&self, provider: Rc<IDebugConfigurationProvider>) -> DisposableRef {
        ConfigurationManager::register_debug_configuration_provider(self, provider)
    }

    fn unregister_debug_configuration_provider(&self, provider: &Rc<IDebugConfigurationProvider>) {
        ConfigurationManager::unregister_debug_configuration_provider(self, provider);
    }

    fn get_launch(&self, workspace_uri: Option<&URI>) -> Option<Rc<dyn ILaunch>> {
        ConfigurationManager::get_launch(self, workspace_uri).map(|launch| launch as Rc<dyn ILaunch>)
    }
}
