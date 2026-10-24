//! Porte de `src/vs/workbench/services/configuration/common/configurationModels.ts`: os parsers do arquivo de workspace
//! (`.code-workspace`) e das configurações isoladas (`tasks.json`, `launch.json`, `mcp.json`), e a `Configuration` do
//! workbench, que conhece o workspace.
//!
//! Decisões de tradução:
//! - **Herança**: as subclasses de `ConfigurationModelParser` trocam o `doParseRaw` (`protected`). Aqui cada uma é um
//!   [`ParseRawOverride`] instalado no parser da plataforma, e a struct pública o expõe por `Deref`, como a subclasse
//!   expõe os métodos da base. A `Configuration` do workbench guarda a da plataforma e expõe os métodos que a subclasse
//!   troca; os demais chegam por `Deref`.
//! - **`IStoredWorkspaceFolder`** é o [`RawWorkspaceFolder`]: o formato `{ path }` ou `{ uri }` (com `name`). Um
//!   elemento de `folders` que não tem nenhum dos dois formatos não é guardado (o original o guarda como veio e
//!   `toWorkspaceFolders` o ignora depois, com o mesmo efeito).
//! - O `console.error` das mensagens de conflito é `eprintln!`.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::map::ResourceMap;
use je_base::objects;
use je_base::uri::URI;
use je_platform::configuration::models::{Configuration as BaseConfiguration, ConfigurationModel, ConfigurationModelParser, ConfigurationParseOptions, ParseRawOverride, ParsedRaw};
use je_platform::configuration::{ConfigurationChange, ConfigurationOverrides, to_values_tree};
use je_platform::js::object_keys;
use je_platform::log::LogServiceRef;
use je_platform::workspace::{RawWorkspaceFolder, Workspace};

/// `IStoredWorkspaceFolder`.
pub type StoredWorkspaceFolder = RawWorkspaceFolder;

/// O nome de uma pasta guardada: `undefined` e os valores falsos (`''`, `false`, `0`, `null`) são "sem nome"; qualquer
/// outro valor que não seja texto invalida o formato.
fn stored_folder_name(folder: &Dynamic) -> Result<Option<String>, ()> {
    match folder.get_str("name") {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        Dynamic::String(name) if name.is_empty() => Ok(None),
        Dynamic::String(name) => Ok(Some(name.to_string_lossy())),
        Dynamic::Bool(false) => Ok(None),
        Dynamic::Number(n) if n == 0.0 => Ok(None),
        _ => Err(()),
    }
}

/// `isRawFileWorkspaceFolder || isRawUriWorkspaceFolder` sobre um valor dinâmico do arquivo de workspace.
fn stored_workspace_folder_from_dynamic(folder: &Dynamic) -> Option<StoredWorkspaceFolder> {
    if !matches!(folder, Dynamic::Object(_)) {
        return None;
    }
    if let Dynamic::String(path) = folder.get_str("path")
        && let Ok(name) = stored_folder_name(folder)
    {
        return Some(RawWorkspaceFolder::Path { path: path.to_string_lossy(), name });
    }
    if let Dynamic::String(uri) = folder.get_str("uri")
        && let Ok(name) = stored_folder_name(folder)
    {
        return Some(RawWorkspaceFolder::Uri { uri: uri.to_string_lossy(), name });
    }
    None
}

/// O estado que o `doParseRaw` de `WorkspaceConfigurationModelParser` preenche.
struct WorkspaceParseState {
    name: String,
    log: LogServiceRef,
    folders: RefCell<Vec<StoredWorkspaceFolder>>,
    transient: Cell<bool>,
    settings_model_parser: RefCell<ConfigurationModelParser>,
    launch_model: RefCell<ConfigurationModel>,
    tasks_model: RefCell<ConfigurationModel>,
}

impl WorkspaceParseState {
    fn create_configuration_model_from(&self, raw: &Dynamic, key: &str) -> ConfigurationModel {
        let data = raw.get_str(key);
        if data.is_truthy() {
            let name = self.name.clone();
            let contents = to_values_tree(&data, &move |message| eprintln!("Conflict in settings file {name}: {message}"));
            let scoped_contents = ObjectRef::new();
            scoped_contents.set(&JsString::from(key), contents);
            let keys = object_keys(&data).iter().map(|k| JsString::from(format!("{key}.{k}"))).collect();
            return ConfigurationModel::new(Dynamic::Object(scoped_contents), keys, Vec::new(), None, self.log.clone());
        }
        ConfigurationModel::create_empty_model(self.log.clone())
    }
}

impl ParseRawOverride for WorkspaceParseState {
    fn do_parse_raw(&self, parser: &ConfigurationModelParser, raw: &Dynamic, options: Option<&ConfigurationParseOptions>) -> ParsedRaw {
        let folders = raw.get_str("folders");
        *self.folders.borrow_mut() = match &folders {
            Dynamic::Array(items) if folders.is_truthy() => items.to_vec().iter().filter_map(stored_workspace_folder_from_dynamic).collect(),
            _ => Vec::new(),
        };
        self.transient.set(matches!(raw.get_str("transient"), Dynamic::Bool(true)));
        self.settings_model_parser.borrow_mut().parse_raw(&raw.get_str("settings"), options);
        *self.launch_model.borrow_mut() = self.create_configuration_model_from(raw, "launch");
        *self.tasks_model.borrow_mut() = self.create_configuration_model_from(raw, "tasks");
        parser.do_parse_raw_base(raw, options)
    }
}

/// `WorkspaceConfigurationModelParser`.
pub struct WorkspaceConfigurationModelParser {
    parser: ConfigurationModelParser,
    state: Rc<WorkspaceParseState>,
}

impl WorkspaceConfigurationModelParser {
    pub fn new(name: impl Into<String>, log: LogServiceRef) -> Self {
        let name = name.into();
        let state = Rc::new(WorkspaceParseState {
            name: name.clone(),
            log: log.clone(),
            folders: RefCell::new(Vec::new()),
            transient: Cell::new(false),
            settings_model_parser: RefCell::new(ConfigurationModelParser::new(name.clone(), log.clone())),
            launch_model: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
            tasks_model: RefCell::new(ConfigurationModel::create_empty_model(log.clone())),
        });
        Self { parser: ConfigurationModelParser::with_override(name, log, state.clone()), state }
    }

    /// `folders`.
    pub fn folders(&self) -> Vec<StoredWorkspaceFolder> {
        self.state.folders.borrow().clone()
    }

    /// `transient`.
    pub fn transient(&self) -> bool {
        self.state.transient.get()
    }

    /// `settingsModel`.
    pub fn settings_model(&self) -> ConfigurationModel {
        self.state.settings_model_parser.borrow().configuration_model()
    }

    /// `launchModel`.
    pub fn launch_model(&self) -> ConfigurationModel {
        self.state.launch_model.borrow().clone()
    }

    /// `tasksModel`.
    pub fn tasks_model(&self) -> ConfigurationModel {
        self.state.tasks_model.borrow().clone()
    }

    /// `reparseWorkspaceSettings(configurationParseOptions)`.
    pub fn reparse_workspace_settings(&self, configuration_parse_options: &ConfigurationParseOptions) {
        self.state.settings_model_parser.borrow_mut().reparse(configuration_parse_options);
    }

    /// `getRestrictedWorkspaceSettings()`.
    pub fn get_restricted_workspace_settings(&self) -> Vec<JsString> {
        self.state.settings_model_parser.borrow().restricted_configurations().to_vec()
    }
}

impl Deref for WorkspaceConfigurationModelParser {
    type Target = ConfigurationModelParser;

    fn deref(&self) -> &ConfigurationModelParser {
        &self.parser
    }
}

impl DerefMut for WorkspaceConfigurationModelParser {
    fn deref_mut(&mut self) -> &mut ConfigurationModelParser {
        &mut self.parser
    }
}

/// O `doParseRaw` de `StandaloneConfigurationModelParser`: o conteúdo inteiro sob a chave do escopo.
struct StandaloneParseOverride {
    scope: String,
}

impl ParseRawOverride for StandaloneParseOverride {
    fn do_parse_raw(&self, parser: &ConfigurationModelParser, raw: &Dynamic, _options: Option<&ConfigurationParseOptions>) -> ParsedRaw {
        let contents = parser.to_values_tree_reporting_conflicts(raw);
        let scoped_contents = ObjectRef::new();
        scoped_contents.set(&JsString::from(self.scope.as_str()), contents);
        let keys = object_keys(raw).iter().map(|key| JsString::from(format!("{}.{key}", self.scope))).collect();
        ParsedRaw { contents: Dynamic::Object(scoped_contents), keys, overrides: Vec::new(), restricted: Vec::new(), has_excluded_properties: false }
    }
}

/// `StandaloneConfigurationModelParser`.
pub struct StandaloneConfigurationModelParser(ConfigurationModelParser);

impl StandaloneConfigurationModelParser {
    pub fn new(name: impl Into<String>, scope: impl Into<String>, log: LogServiceRef) -> Self {
        Self(ConfigurationModelParser::with_override(name, log, Rc::new(StandaloneParseOverride { scope: scope.into() })))
    }
}

impl Deref for StandaloneConfigurationModelParser {
    type Target = ConfigurationModelParser;

    fn deref(&self) -> &ConfigurationModelParser {
        &self.0
    }
}

impl DerefMut for StandaloneConfigurationModelParser {
    fn deref_mut(&mut self) -> &mut ConfigurationModelParser {
        &mut self.0
    }
}

/// `Configuration`: a da plataforma mais o workspace, que o `getValue`, o `inspect` e o `keys` repassam.
pub struct Configuration {
    base: Rc<BaseConfiguration>,
    workspace: Option<Rc<Workspace>>,
}

impl Configuration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        defaults: ConfigurationModel,
        policy: ConfigurationModel,
        application: ConfigurationModel,
        local_user: ConfigurationModel,
        remote_user: ConfigurationModel,
        workspace_configuration: ConfigurationModel,
        folders: ResourceMap<ConfigurationModel>,
        memory_configuration: ConfigurationModel,
        memory_configuration_by_resource: ResourceMap<ConfigurationModel>,
        workspace: Option<Rc<Workspace>>,
        log: LogServiceRef,
    ) -> Self {
        Self {
            base: Rc::new(BaseConfiguration::new(
                defaults,
                policy,
                application,
                local_user,
                remote_user,
                workspace_configuration,
                folders,
                memory_configuration,
                memory_configuration_by_resource,
                log,
            )),
            workspace,
        }
    }

    /// A `Configuration` da plataforma, que o `ConfigurationChangeEvent` guarda.
    pub fn base(&self) -> Rc<BaseConfiguration> {
        self.base.clone()
    }

    /// `getValue(key, overrides)`.
    pub fn get_value(&self, key: Option<&JsString>, overrides: &ConfigurationOverrides) -> Dynamic {
        self.base.get_value(key, overrides, self.workspace.as_deref())
    }

    /// `inspect(key, overrides)`.
    pub fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> je_platform::configuration::models::ConfigurationInspectValue {
        self.base.inspect(key, overrides, self.workspace.as_deref())
    }

    /// `keys()`.
    pub fn keys(&self) -> je_platform::configuration::models::ConfigurationKeys {
        self.base.keys(self.workspace.as_deref())
    }

    /// `compareAndDeleteFolderConfiguration(folder)`: a configuração da primeira pasta (a do workspace) não sai.
    pub fn compare_and_delete_folder_configuration(&self, folder: &URI) -> ConfigurationChange {
        if let Some(workspace) = &self.workspace {
            let folders = workspace.folders();
            if let Some(first) = folders.first()
                && first.uri.to_string(false) == folder.to_string(false)
            {
                // Do not remove workspace configuration
                return ConfigurationChange::default();
            }
        }
        self.base.compare_and_delete_folder_configuration(folder)
    }

    /// `compare(other)`.
    pub fn compare(&self, other: &Configuration) -> ConfigurationChange {
        let compare = |from_keys: &[JsString], to_keys: &[JsString], override_identifier: Option<&JsString>| -> Vec<JsString> {
            let mut keys: Vec<JsString> = Vec::new();
            keys.extend(to_keys.iter().filter(|key| !from_keys.contains(key)).cloned());
            keys.extend(from_keys.iter().filter(|key| !to_keys.contains(key)).cloned());
            keys.extend(
                from_keys
                    .iter()
                    .filter(|key| {
                        // Ignore if the key does not exist in both models
                        if !to_keys.contains(key) {
                            return false;
                        }
                        let overrides = ConfigurationOverrides { override_identifier: override_identifier.cloned(), resource: None };
                        // Compare workspace value
                        if !objects::equals(&self.get_value(Some(key), &overrides), &other.get_value(Some(key), &overrides)) {
                            return true;
                        }
                        // Compare workspace folder value
                        self.workspace.as_ref().is_some_and(|workspace| {
                            workspace.folders().iter().any(|folder| {
                                let overrides = ConfigurationOverrides { override_identifier: override_identifier.cloned(), resource: Some(folder.uri.clone()) };
                                !objects::equals(&self.get_value(Some(key), &overrides), &other.get_value(Some(key), &overrides))
                            })
                        })
                    })
                    .cloned(),
            );
            keys
        };
        let keys = compare(&self.all_keys(), &other.all_keys(), None);
        let mut overrides: Vec<(JsString, Vec<JsString>)> = Vec::new();
        let mut all_override_identifiers = self.all_override_identifiers();
        all_override_identifiers.extend(other.all_override_identifiers());
        for override_identifier in arrays::distinct(&all_override_identifiers, Clone::clone) {
            let keys = compare(
                &self.get_all_keys_for_override_identifier(&override_identifier),
                &other.get_all_keys_for_override_identifier(&override_identifier),
                Some(&override_identifier),
            );
            if !keys.is_empty() {
                overrides.push((override_identifier, keys));
            }
        }
        ConfigurationChange { keys, overrides }
    }
}

impl Deref for Configuration {
    type Target = BaseConfiguration;

    fn deref(&self) -> &BaseConfiguration {
        &self.base
    }
}
