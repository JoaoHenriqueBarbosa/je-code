//! Porte de `src/vs/workbench/services/configuration/browser/configurationService.ts`: o `WorkspaceService`, que é ao
//! mesmo tempo o serviço de configuração do workbench (padrões, política, aplicação, usuário local e remoto,
//! workspace, pasta e memória) e o `IWorkspaceContextService` (pastas, estado do workbench e edição das pastas).
//!
//! Decisões de tradução:
//! - **Sobrecargas** de `getValue`, `updateValue` e `reloadConfiguration`: [`WorkspaceService::get_value`] recebe a seção
//!   e os `overrides` opcionais; [`WorkspaceService::update_value`] recebe os `overrides` (já na forma
//!   `IConfigurationUpdateOverrides`), o alvo e as opções; [`WorkspaceService::reload_configuration_for`] recebe o
//!   alvo (`ConfigurationTarget | IWorkspaceFolder`) como [`ReloadTarget`]. Os métodos do trait
//!   [`IConfigurationService`] (reduzido) chamam essas formas.
//! - **`IInstantiationService`**: `acquireInstantiationService` recebe a fábrica de que o serviço precisa (o
//!   [`WorkspaceInstantiation`]): criar o `ConfigurationEditing` e entregar o `IJSONEditingService`.
//! - **`Workspace` com `initialized`**: o `Workspace` da plataforma mais o campo `workspace_initialized` do serviço.
//! - **`RegisterConfigurationSchemasContribution` e `ConfigurationDefaultOverridesContribution`** (as contribuições do
//!   fim do arquivo original) dependem do serviço de extensões, do registro de esquemas JSON, da renderização de
//!   markdown e do serviço de atribuição de experimentos, que não foram portados; a configuração
//!   `workbench.settings.applyToAllProfiles` é registrada por [`register_workspace_service_configuration`].
//! - `mark(...)` (marcas de desempenho) não tem equivalente e foi omitido.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{Barrier, Queue, async_fn, promises, run_when_global_idle};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::objects;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::defaults::DefaultConfigurationChange;
use je_platform::configuration::models::{
    Configuration as BaseConfiguration, ConfigurationChangeEvent, ConfigurationInspectValue, ConfigurationKeys, ConfigurationModel, PreviousConfiguration, merge_changes,
};
use je_platform::configuration::policy_configuration::{NullPolicyConfiguration, PolicyConfiguration, PolicyConfigurationApi};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry, key_from_override_identifiers};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOptions, ConfigurationUpdateOverrides, ConfigurationValue,
    configuration_target_to_string,
};
use je_platform::files::file_service::FileService;
use je_platform::log::LogServiceRef;
use je_platform::policy::PolicyService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_platform::workspace::{
    AnyWorkspaceIdentifier, CurrentWorkspaceQuery, EmptyWorkspaceIdentifier, IWorkspaceContextService, SingleFolderWorkspaceIdentifier, WorkbenchState, Workspace, WorkspaceFolder,
    WorkspaceFoldersChangeEvent, WorkspaceFoldersWillChangeEvent, WorkspaceIdentifier, to_workspace_folder,
};
use je_platform::workspaces::workspaces::{WorkspaceFolderCreationData, get_stored_workspace_folder, to_workspace_folders};

use super::configuration::{ApplicationConfiguration, DefaultConfiguration, FolderConfiguration, ParseOptionsPatch, RemoteUserConfiguration, UserConfiguration, WorkspaceConfiguration};
use crate::services::configuration::common::configuration::{
    APPLICATION_SCOPES, APPLY_ALL_PROFILES_SETTING, FOLDER_CONFIG_FOLDER_NAME, IConfigurationCache, IWorkbenchConfigurationService, LOCAL_MACHINE_PROFILE_SCOPES, LOCAL_MACHINE_SCOPES,
    PROFILE_SCOPES, RestrictedSettings,
};
use crate::services::configuration::common::configuration_editing::{
    ConfigurationEditing, ConfigurationEditingOptions, ConfigurationEditingValue, EditableConfigurationTarget,
};
use crate::services::configuration::common::configuration_models::{Configuration, StoredWorkspaceFolder};
use crate::services::configuration::common::json_editing::IJsonEditingService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;
use crate::services::user_data_profile::user_data_profile::{DidChangeUserDataProfileEvent, IUserDataProfileService};

fn emitter<T: 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// O que o serviço pede ao `IInstantiationService` depois de `acquireInstantiationService`.
pub trait WorkspaceInstantiation {
    /// `instantiationService.createInstance(ConfigurationEditing, remoteSettingsResource)`.
    fn create_configuration_editing(&self, remote_settings_resource: Option<URI>) -> Rc<ConfigurationEditing>;
    /// `accessor.get(IJSONEditingService)`.
    fn json_editing_service(&self) -> Rc<dyn IJsonEditingService>;
}

/// `ConfigurationTarget | IWorkspaceFolder` de `reloadConfiguration`.
#[derive(Clone)]
pub enum ReloadTarget {
    Target(ConfigurationTarget),
    Folder(WorkspaceFolder),
}

/// `getLocalUserConfigurationScopes(userDataProfile, hasRemote)`.
fn get_local_user_configuration_scopes(user_data_profile: &UserDataProfile, has_remote: bool) -> Option<Vec<ConfigurationScope>> {
    let is_default_profile = user_data_profile.is_default || user_data_profile.use_default_flags.is_some_and(|flags| flags.settings.unwrap_or(false));
    if is_default_profile {
        return has_remote.then(|| LOCAL_MACHINE_SCOPES.to_vec());
    }
    Some(if has_remote { LOCAL_MACHINE_PROFILE_SCOPES.to_vec() } else { PROFILE_SCOPES.to_vec() })
}

/// A chave de uma pasta nos mapas por recurso (`ResourceMap` usa `uri.toString()`).
fn folder_key(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// Ordena como `localeCompare` (aproximado pela ordem do texto).
fn sorted(mut keys: Vec<JsString>) -> Vec<JsString> {
    keys.sort_by_key(JsString::to_string_lossy);
    keys
}

/// Se `delta(a, b)` tem alguma diferença (`added` ou `removed`).
fn differs(a: &[JsString], b: &[JsString]) -> bool {
    let (set_a, set_b): (HashSet<&JsString>, HashSet<&JsString>) = (a.iter().collect(), b.iter().collect());
    set_a != set_b
}

/// A configuração `workbench.settings.applyToAllProfiles` que o fim do arquivo original registra.
pub fn register_workspace_service_configuration() {
    let property = PropertySchema::from_entries([
        ("type", Dynamic::string("array")),
        ("description", Dynamic::string(&je_base::nls::localize("setting description", "Configure settings to be applied for all profiles.", &[]))),
        ("default", Dynamic::array([])),
        ("scope", ConfigurationScope::Application.to_dynamic()),
        ("additionalProperties", Dynamic::Bool(true)),
        ("uniqueItems", Dynamic::Bool(true)),
    ]);
    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("workbench".to_owned()),
            order: Some(7.0),
            title: Some(je_base::nls::localize("workbenchConfigurationTitle", "Workbench", &[])),
            type_: Some(Dynamic::string("object")),
            ..Default::default()
        }
        .with_properties([(APPLY_ALL_PROFILES_SETTING, property)]),
    ));
}

/// `WorkspaceService`.
pub struct WorkspaceService {
    base: Rc<Disposable>,
    this: Weak<Self>,
    workspace: RefCell<Option<Rc<Workspace>>>,
    workspace_initialized: Cell<bool>,
    init_remote_user_configuration_barrier: Barrier,
    complete_workspace_barrier: Barrier,
    configuration_cache: Rc<dyn IConfigurationCache>,
    configuration: RefCell<Rc<Configuration>>,
    initialized: Cell<bool>,
    default_configuration: Rc<DefaultConfiguration>,
    policy_configuration: Rc<dyn PolicyConfigurationApi>,
    application_configuration: RefCell<Option<Rc<ApplicationConfiguration>>>,
    application_configuration_disposables: Rc<DisposableStore>,
    local_user_configuration: Rc<UserConfiguration>,
    remote_user_configuration: Option<Rc<RemoteUserConfiguration>>,
    workspace_configuration: Rc<WorkspaceConfiguration>,
    cached_folder_configs: Rc<DisposableMap<String, FolderConfiguration>>,
    workspace_editing_queue: Queue<()>,
    on_did_change_configuration: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    on_will_change_workspace_folders: Rc<Emitter<WorkspaceFoldersWillChangeEvent>>,
    on_did_change_workspace_folders: Rc<Emitter<WorkspaceFoldersChangeEvent>>,
    on_did_change_workspace_name: Rc<Emitter<()>>,
    on_did_change_workbench_state: Rc<Emitter<WorkbenchState>>,
    is_workspace_trusted: Cell<bool>,
    restricted_settings: RefCell<RestrictedSettings>,
    on_did_change_restricted_settings: Rc<Emitter<RestrictedSettings>>,
    instantiation: RefCell<Option<Rc<dyn WorkspaceInstantiation>>>,
    configuration_editing: RefCell<Option<Promise<Rc<ConfigurationEditing>>>>,
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    file_service: Rc<FileService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log: LogServiceRef,
}

impl WorkspaceService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        remote_authority: Option<String>,
        configuration_cache: Rc<dyn IConfigurationCache>,
        configuration_defaults: Option<Dynamic>,
        user_data_profile_service: Rc<dyn IUserDataProfileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        file_service: Rc<FileService>,
        remote_agent_service: Rc<dyn IRemoteAgentService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log: LogServiceRef,
        policy_service: Rc<dyn PolicyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let current_profile = user_data_profile_service.current_profile();
        let default_configuration = DefaultConfiguration::new(&current_profile.id, configuration_cache.clone(), configuration_defaults, log.clone());
        base.register(default_configuration.clone());
        let policy_configuration: Rc<dyn PolicyConfigurationApi> = if policy_service.is_null() {
            Rc::new(NullPolicyConfiguration::new())
        } else {
            let policy_configuration = PolicyConfiguration::new(default_configuration.base(), policy_service, log.clone());
            base.register(policy_configuration.clone());
            Rc::new(policy_configuration)
        };
        let empty = || ConfigurationModel::create_empty_model(log.clone());
        let configuration = Configuration::new(
            default_configuration.configuration_model(),
            policy_configuration.configuration_model(),
            empty(),
            empty(),
            empty(),
            empty(),
            ResourceMap::new(),
            empty(),
            ResourceMap::new(),
            None,
            log.clone(),
        );
        let application_configuration_disposables = DisposableStore::new();
        base.register(application_configuration_disposables.clone());

        let local_user_configuration = UserConfiguration::new(
            current_profile.settings_resource.clone(),
            Some(current_profile.tasks_resource.clone()),
            Some(current_profile.mcp_resource.clone()),
            je_platform::configuration::models::ConfigurationParseOptions { scopes: get_local_user_configuration_scopes(&current_profile, remote_authority.is_some()), ..Default::default() },
            file_service.clone(),
            &*uri_identity_service,
            log.clone(),
        );
        base.register(local_user_configuration.clone());
        let remote_user_configuration = remote_authority.as_deref().map(|remote_authority| {
            let remote = RemoteUserConfiguration::new(remote_authority, configuration_cache.clone(), file_service.clone(), &*uri_identity_service, &*remote_agent_service, log.clone());
            base.register(remote.clone());
            remote
        });
        let workspace_configuration = WorkspaceConfiguration::new(configuration_cache.clone(), file_service.clone(), &*uri_identity_service, log.clone());
        base.register(workspace_configuration.clone());

        let cached_folder_configs = DisposableMap::new();
        base.register(cached_folder_configs.clone());
        let this = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            this: weak.clone(),
            init_remote_user_configuration_barrier: Barrier::new(),
            complete_workspace_barrier: Barrier::new(),
            workspace: RefCell::new(None),
            workspace_initialized: Cell::new(false),
            configuration_cache,
            configuration: RefCell::new(Rc::new(configuration)),
            initialized: Cell::new(false),
            default_configuration,
            policy_configuration,
            application_configuration: RefCell::new(None),
            application_configuration_disposables,
            local_user_configuration,
            remote_user_configuration,
            workspace_configuration,
            cached_folder_configs,
            workspace_editing_queue: Queue::new(),
            on_did_change_configuration: emitter(&base),
            on_will_change_workspace_folders: emitter(&base),
            on_did_change_workspace_folders: emitter(&base),
            on_did_change_workspace_name: emitter(&base),
            on_did_change_workbench_state: emitter(&base),
            is_workspace_trusted: Cell::new(true),
            restricted_settings: RefCell::new(RestrictedSettings::default()),
            on_did_change_restricted_settings: emitter(&base),
            instantiation: RefCell::new(None),
            configuration_editing: RefCell::new(None),
            user_data_profile_service,
            user_data_profiles_service,
            file_service,
            remote_agent_service,
            uri_identity_service,
            log,
            base,
        });
        this.wire();
        this
    }

    /// O que o construtor liga depois de o serviço existir (os ouvintes que chamam métodos dele).
    fn wire(self: &Rc<Self>) {
        self.create_application_configuration();
        let weak = self.this.clone();
        self.base.register(self.local_user_configuration.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |model| {
                if let Some(this) = weak.upgrade() {
                    this.on_local_user_configuration_changed(model.clone());
                }
            }
        }));
        if let Some(remote_user_configuration) = &self.remote_user_configuration {
            let remote = remote_user_configuration.clone();
            let weak_init = weak.clone();
            self.base.register(remote_user_configuration.on_did_initialize().listen(move |remote_user_configuration_model| {
                if let Some(this) = weak_init.upgrade() {
                    let weak_change = weak_init.clone();
                    this.base.register(remote.on_did_change_configuration().listen(move |model| {
                        if let Some(this) = weak_change.upgrade() {
                            this.on_remote_user_configuration_changed(model.clone());
                        }
                    }));
                    this.on_remote_user_configuration_changed(remote_user_configuration_model.clone());
                    this.init_remote_user_configuration_barrier.open();
                }
            }));
        } else {
            self.init_remote_user_configuration_barrier.open();
        }

        self.base.register(self.workspace_configuration.on_did_update_configuration().listen({
            let weak = weak.clone();
            move |from_cache| {
                if let Some(this) = weak.upgrade() {
                    let from_cache = *from_cache;
                    let target = this.clone();
                    let _ = this.on_workspace_configuration_changed(from_cache).then(move |()| {
                        target.workspace_initialized.set(target.workspace_configuration.initialized());
                        target.check_and_mark_workspace_complete(from_cache);
                    });
                }
            }
        }));

        self.base.register(self.default_configuration.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |DefaultConfigurationChange { defaults, properties }| {
                if let Some(this) = weak.upgrade() {
                    this.on_default_configuration_changed(defaults.clone(), Some(properties.clone()));
                }
            }
        }));
        self.base.register(self.policy_configuration.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |model| {
                if let Some(this) = weak.upgrade() {
                    this.on_policy_configuration_changed(model.clone());
                }
            }
        }));
        self.base.register(self.user_data_profile_service.on_did_change_current_profile().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_user_data_profile_changed(event.clone());
            }
        }));
    }

    fn config(&self) -> Rc<Configuration> {
        self.configuration.borrow().clone()
    }

    fn current_workspace(&self) -> Rc<Workspace> {
        self.workspace.borrow().clone().expect("o workspace só existe depois de initialize")
    }

    fn create_application_configuration(self: &Rc<Self>) {
        self.application_configuration_disposables.clear();
        let profile = self.user_data_profile_service.current_profile();
        if profile.is_default || profile.use_default_flags.is_some_and(|flags| flags.settings.unwrap_or(false)) {
            *self.application_configuration.borrow_mut() = None;
        } else {
            let application_configuration = ApplicationConfiguration::new(&*self.user_data_profiles_service, self.file_service.clone(), &*self.uri_identity_service, self.log.clone());
            self.base.register(application_configuration.clone());
            let weak = self.this.clone();
            self.application_configuration_disposables.add(application_configuration.on_did_change_configuration().listen(move |model| {
                if let Some(this) = weak.upgrade() {
                    this.on_application_configuration_changed(model.clone());
                }
            }));
            *self.application_configuration.borrow_mut() = Some(application_configuration);
        }
    }

    fn application_configuration(&self) -> Option<Rc<ApplicationConfiguration>> {
        self.application_configuration.borrow().clone()
    }

    // Workspace Context Service Impl

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.on_did_change_configuration.event()
    }

    /// `getCompleteWorkspace()`.
    pub fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        let this = self.this.upgrade().expect("o serviço está vivo");
        this.complete_workspace_barrier.wait().then(move |_| this.current_workspace())
    }

    /// `getWorkspace()`.
    pub fn get_workspace(&self) -> Rc<Workspace> {
        self.current_workspace()
    }

    /// `getWorkbenchState()`.
    pub fn get_workbench_state(&self) -> WorkbenchState {
        let workspace = self.current_workspace();
        // Workspace has configuration file
        if workspace.configuration().is_some() {
            return WorkbenchState::Workspace;
        }

        // Folder has single root
        if workspace.folders().len() == 1 {
            return WorkbenchState::Folder;
        }

        // Empty
        WorkbenchState::Empty
    }

    /// `hasWorkspaceData()`.
    pub fn has_workspace_data(&self) -> bool {
        self.get_workbench_state() != WorkbenchState::Empty
    }

    /// `getWorkspaceFolder(resource)`.
    pub fn get_workspace_folder(&self, resource: &URI) -> Option<WorkspaceFolder> {
        self.current_workspace().get_folder(resource)
    }

    /// `addFolders(foldersToAdd, index?)`.
    pub fn add_folders(self: &Rc<Self>, folders_to_add: Vec<WorkspaceFolderCreationData>, index: Option<usize>) -> Promise<()> {
        self.update_folders(folders_to_add, Vec::new(), index)
    }

    /// `removeFolders(foldersToRemove)`.
    pub fn remove_folders(self: &Rc<Self>, folders_to_remove: Vec<URI>) -> Promise<()> {
        self.update_folders(Vec::new(), folders_to_remove, None)
    }

    /// `updateFolders(foldersToAdd, foldersToRemove, index?)`.
    pub fn update_folders(self: &Rc<Self>, folders_to_add: Vec<WorkspaceFolderCreationData>, folders_to_remove: Vec<URI>, index: Option<usize>) -> Promise<()> {
        let this = self.clone();
        self.workspace_editing_queue.queue(move || this.do_update_folders(folders_to_add, folders_to_remove, index))
    }

    /// `isInsideWorkspace(resource)`.
    pub fn is_inside_workspace(&self, resource: &URI) -> bool {
        self.get_workspace_folder(resource).is_some()
    }

    /// `isCurrentWorkspace(workspaceIdOrFolder)`.
    pub fn is_current_workspace(&self, workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        let workspace = self.current_workspace();
        match self.get_workbench_state() {
            WorkbenchState::Folder => {
                let folder_uri = match workspace_id_or_folder {
                    CurrentWorkspaceQuery::Uri(uri) => Some(uri),
                    CurrentWorkspaceQuery::SingleFolder(identifier) => Some(&identifier.uri),
                    CurrentWorkspaceQuery::Workspace(_) => None,
                };
                folder_uri.is_some_and(|uri| self.uri_identity_service.ext_uri().is_equal(Some(uri), workspace.folders().first().map(|folder| &folder.uri), false))
            }
            WorkbenchState::Workspace => matches!(workspace_id_or_folder, CurrentWorkspaceQuery::Workspace(identifier) if workspace.id() == identifier.id),
            WorkbenchState::Empty => false,
        }
    }

    fn do_update_folders(self: &Rc<Self>, folders_to_add: Vec<WorkspaceFolderCreationData>, folders_to_remove: Vec<URI>, index: Option<usize>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.get_workbench_state() != WorkbenchState::Workspace {
                return Ok(()); // we need a workspace to begin with
            }

            if folders_to_add.len() + folders_to_remove.len() == 0 {
                return Ok(()); // nothing to do
            }

            // Remove first (if any)
            let mut current_workspace_folders = this.get_workspace().folders();
            let mut new_stored_folders: Vec<StoredWorkspaceFolder> = current_workspace_folders
                .iter()
                .filter_map(|folder| folder.raw.clone().map(|raw| (raw, folder)))
                .filter(|(_, folder)| !this.contains(&folders_to_remove, &folder.uri)) // keep entries which are unrelated
                .map(|(raw, _)| raw)
                .collect();

            let mut folders_have_changed = current_workspace_folders.len() != new_stored_folders.len();

            // Add afterwards (if any)
            if !folders_to_add.is_empty() {
                // Recompute current workspace folders if we have folders to add
                let workspace_config_path = this.get_workspace().configuration().expect("um workspace multi-raiz tem arquivo de configuração");
                let ext_uri = this.uri_identity_service.ext_uri();
                let workspace_config_folder = ext_uri.dirname(&workspace_config_path);
                current_workspace_folders = to_workspace_folders(&new_stored_folders, &workspace_config_path, &ext_uri);
                let current_workspace_folder_uris: Vec<URI> = current_workspace_folders.iter().map(|folder| folder.uri.clone()).collect();

                let mut stored_folders_to_add: Vec<StoredWorkspaceFolder> = Vec::new();

                for folder_to_add in &folders_to_add {
                    let folder_uri = &folder_to_add.uri;
                    if this.contains(&current_workspace_folder_uris, folder_uri) {
                        continue; // already existing
                    }
                    if let Ok(result) = this.file_service.stat(folder_uri).await
                        && !result.is_directory
                    {
                        continue;
                    }
                    stored_folders_to_add.push(get_stored_workspace_folder(folder_uri, false, folder_to_add.name.as_deref(), &workspace_config_folder, &ext_uri));
                }

                // Apply to array of newStoredFolders
                if !stored_folders_to_add.is_empty() {
                    folders_have_changed = true;

                    match index {
                        Some(index) if index < new_stored_folders.len() => drop(new_stored_folders.splice(index..index, stored_folders_to_add)),
                        _ => new_stored_folders.extend(stored_folders_to_add),
                    }
                }
            }

            // Set folders if we recorded a change
            if folders_have_changed {
                return this.set_folders(new_stored_folders).await;
            }

            Ok(())
        })
    }

    fn set_folders(self: &Rc<Self>, folders: Vec<StoredWorkspaceFolder>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(instantiation) = this.instantiation.borrow().clone() else {
                return Err(Error::new("Cannot update workspace folders because workspace service is not yet ready to accept writes."));
            };
            this.workspace_configuration.set_folders(&folders, &*instantiation.json_editing_service()).await?;
            this.on_workspace_configuration_changed(false).await
        })
    }

    fn contains(&self, resources: &[URI], to_check: &URI) -> bool {
        let ext_uri = self.uri_identity_service.ext_uri();
        resources.iter().any(|resource| ext_uri.is_equal(Some(resource), Some(to_check), false))
    }

    // Workspace Configuration Service Impl

    /// `getConfigurationData()`.
    pub fn get_configuration_data(&self) -> ConfigurationData {
        self.config().to_data()
    }

    /// `getValue(section?, overrides?)`.
    pub fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        self.config().get_value(section.map(JsString::from).as_ref(), &overrides.cloned().unwrap_or_default())
    }

    /// `updateValue(key, value, overrides?, target?, options?)`.
    pub fn update_value(
        self: &Rc<Self>,
        key: &str,
        value: Dynamic,
        overrides: Option<ConfigurationUpdateOverrides>,
        target: Option<ConfigurationTarget>,
        options: Option<ConfigurationUpdateOptions>,
    ) -> Promise<()> {
        let (this, key) = (self.clone(), key.to_owned());
        async_fn(async move {
            let mut value = value;
            let mut overrides = overrides;
            let mut targets: Vec<ConfigurationTarget> = target.into_iter().collect();

            if let Some(overrides) = overrides.as_mut()
                && let Some(identifiers) = overrides.override_identifiers.take()
            {
                let mut distinct: Vec<JsString> = Vec::new();
                for identifier in identifiers {
                    if !distinct.contains(&identifier) {
                        distinct.push(identifier);
                    }
                }
                overrides.override_identifiers = (!distinct.is_empty()).then_some(distinct);
            }

            if targets.is_empty() {
                if overrides.as_ref().and_then(|o| o.override_identifiers.as_ref()).is_some_and(|ids| ids.len() > 1) {
                    return Err(Error::new("Configuration Target is required while updating the value for multiple override identifiers"));
                }
                let inspect = this.inspect(
                    &key,
                    &ConfigurationOverrides {
                        resource: overrides.as_ref().and_then(|o| o.resource.clone()),
                        override_identifier: overrides.as_ref().and_then(|o| o.override_identifiers.as_ref()).and_then(|ids| ids.first().cloned()),
                    },
                );
                targets.extend(Self::derive_configuration_targets(&value, &inspect));

                // Remove the setting, if the value is same as default value and is updated only in user target
                if objects::equals(&value, &inspect.default_value()) && targets.len() == 1 && matches!(targets[0], ConfigurationTarget::User | ConfigurationTarget::UserLocal) {
                    value = Dynamic::Undefined;
                }
            }

            promises::settled(targets.into_iter().map(|target| this.write_configuration_value(&key, value.clone(), target, overrides.clone(), options)).collect()).await?;
            Ok(())
        })
    }

    /// `reloadConfiguration(target?)`.
    pub fn reload_configuration_for(self: &Rc<Self>, target: Option<ReloadTarget>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(target) = target else {
                this.reload_default_configuration();
                let application = this.reload_application_configuration(true).await?;
                let (local, remote) = this.reload_user_configuration().await?;
                this.reload_workspace_configuration().await?;
                return this.load_configuration(application, local, remote, true).await;
            };

            let target = match target {
                ReloadTarget::Folder(folder) => return this.reload_workspace_folder_configuration(&folder).await,
                ReloadTarget::Target(target) => target,
            };

            match target {
                ConfigurationTarget::Default => this.reload_default_configuration(),
                ConfigurationTarget::User => {
                    let (local, remote) = this.reload_user_configuration().await?;
                    return this.load_configuration(this.config().application_configuration(), local, remote, true).await;
                }
                ConfigurationTarget::UserLocal => {
                    this.reload_local_user_configuration(false, None).await?;
                }
                ConfigurationTarget::UserRemote => {
                    this.reload_remote_user_configuration(false).await?;
                }
                ConfigurationTarget::Workspace | ConfigurationTarget::WorkspaceFolder => return this.reload_workspace_configuration().await,
                ConfigurationTarget::Application | ConfigurationTarget::Memory => {}
            }
            Ok(())
        })
    }

    /// `hasCachedConfigurationDefaultsOverrides()`.
    pub fn has_cached_configuration_defaults_overrides(&self) -> bool {
        self.default_configuration.has_cached_configuration_defaults_overrides()
    }

    /// `inspect(key, overrides?)`.
    pub fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> ConfigurationInspectValue {
        self.config().inspect(key, overrides)
    }

    /// `keys()`.
    pub fn keys(&self) -> ConfigurationKeys {
        self.config().keys()
    }

    /// `whenRemoteConfigurationLoaded()`.
    pub fn when_remote_configuration_loaded(&self) -> Promise<()> {
        self.init_remote_user_configuration_barrier.wait().then(|_| ())
    }

    /// At present, all workspaces (empty, single-folder, multi-root) in local and remote can be initialized without
    /// requiring extension host except following case:
    ///
    /// A multi root workspace with .code-workspace file that has to be resolved by an extension. Because of readonly
    /// `rootPath` property in extension API we have to resolve multi root workspace before extension host starts so
    /// that `rootPath` can be set to first folder.
    ///
    /// This restriction is lifted partially for web in `MainThreadWorkspace`. In web, we start extension host with
    /// empty `rootPath` in this case.
    ///
    /// Related root path issue discussion is being tracked here - https://github.com/microsoft/vscode/issues/69335
    pub fn initialize(self: &Rc<Self>, arg: &AnyWorkspaceIdentifier) -> Promise<()> {
        let (this, arg) = (self.clone(), arg.clone());
        async_fn(async move {
            let trigger = this.initialized.get();
            this.initialized.set(false);
            let workspace = this.create_workspace(&arg).await?;
            this.update_workspace_and_initialize_configuration(workspace, trigger).await?;
            this.check_and_mark_workspace_complete(false);
            Ok(())
        })
    }

    /// `updateWorkspaceTrust(trusted)`.
    pub fn update_workspace_trust(&self, trusted: bool) {
        if self.is_workspace_trusted.get() != trusted {
            self.is_workspace_trusted.set(trusted);
            let configuration = self.config();
            let data = configuration.to_data();
            let workspace = self.current_workspace();
            let mut folder_configuration_models: Vec<Option<ConfigurationModel>> = Vec::new();
            let this = self.this.upgrade().expect("o serviço está vivo");
            for folder in workspace.folders() {
                let folder_configuration = this.cached_folder_configs.get(&folder_key(&folder.uri));
                let configuration_model = folder_configuration.map(|folder_configuration| {
                    let model = folder_configuration.update_workspace_trust(trusted);
                    configuration.update_folder_configuration(&folder.uri, model.clone());
                    model
                });
                folder_configuration_models.push(configuration_model);
            }
            if self.get_workbench_state() == WorkbenchState::Folder {
                if let Some(Some(model)) = folder_configuration_models.first() {
                    configuration.update_workspace_configuration(model.clone());
                }
            } else {
                configuration.update_workspace_configuration(self.workspace_configuration.update_workspace_trust(trusted));
            }
            self.update_restricted_settings();

            let restricted_settings = self.restricted_settings.borrow().clone();
            let mut keys: Vec<JsString> = Vec::new();
            keys.extend(restricted_settings.user_local.clone().unwrap_or_default());
            keys.extend(restricted_settings.user_remote.clone().unwrap_or_default());
            keys.extend(restricted_settings.workspace.clone().unwrap_or_default());
            if let Some(workspace_folder) = &restricted_settings.workspace_folder {
                for value in workspace_folder.values() {
                    keys.extend(value.iter().cloned());
                }
            }
            let keys = je_base::arrays::distinct(&keys, Clone::clone);
            if !keys.is_empty() {
                self.trigger_configuration_change(
                    ConfigurationChange { keys, overrides: Vec::new() },
                    Some(PreviousConfiguration { workspace: Some(workspace), data }),
                    ConfigurationTarget::Workspace,
                );
            }
        }
    }

    /// `acquireInstantiationService(instantiationService)`.
    pub fn acquire_instantiation_service(&self, instantiation: Rc<dyn WorkspaceInstantiation>) {
        *self.instantiation.borrow_mut() = Some(instantiation);
    }

    /// `isSettingAppliedForAllProfiles(key)`.
    pub fn is_setting_applied_for_all_profiles(&self, key: &str) -> bool {
        let scope = configuration_registry().get_configuration_properties().get(key).and_then(|property| property.scope());
        if scope.is_some_and(|scope| APPLICATION_SCOPES.contains(&scope)) {
            return true;
        }
        let all_profiles_settings = self.get_value(Some(APPLY_ALL_PROFILES_SETTING), None);
        matches!(&all_profiles_settings, Dynamic::Array(items) if items.to_vec().iter().any(|item| matches!(item, Dynamic::String(setting) if setting.to_string_lossy() == key)))
    }

    /// `restrictedSettings`.
    pub fn restricted_settings(&self) -> RestrictedSettings {
        self.restricted_settings.borrow().clone()
    }

    /// `onDidChangeRestrictedSettings`.
    pub fn on_did_change_restricted_settings(&self) -> Event<RestrictedSettings> {
        self.on_did_change_restricted_settings.event()
    }

    /// `onWillChangeWorkspaceFolders`.
    pub fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        self.on_will_change_workspace_folders.event()
    }

    /// `onDidChangeWorkspaceFolders`.
    pub fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        self.on_did_change_workspace_folders.event()
    }

    /// `onDidChangeWorkspaceName`.
    pub fn on_did_change_workspace_name(&self) -> Event<()> {
        self.on_did_change_workspace_name.event()
    }

    /// `onDidChangeWorkbenchState`.
    pub fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        self.on_did_change_workbench_state.event()
    }

    fn ignore_path_casing_predicate(&self) -> je_base::ternary_search_tree::UriPredicate {
        let ext_uri = self.uri_identity_service.ext_uri();
        Rc::new(move |uri: &URI| ext_uri.ignore_path_casing(uri))
    }

    fn create_workspace(self: &Rc<Self>, arg: &AnyWorkspaceIdentifier) -> Promise<Rc<Workspace>> {
        match arg {
            AnyWorkspaceIdentifier::Workspace(identifier) => self.create_multi_folder_workspace(identifier.clone()),
            AnyWorkspaceIdentifier::SingleFolder(identifier) => Promise::resolved(self.create_single_folder_workspace(identifier)),
            AnyWorkspaceIdentifier::Empty(identifier) => Promise::resolved(self.create_empty_workspace(identifier)),
        }
    }

    fn create_multi_folder_workspace(self: &Rc<Self>, workspace_identifier: WorkspaceIdentifier) -> Promise<Rc<Workspace>> {
        let this = self.clone();
        async_fn(async move {
            this.workspace_configuration.initialize(WorkspaceIdentifier { id: workspace_identifier.id.clone(), config_path: workspace_identifier.config_path.clone() }, this.is_workspace_trusted.get()).await?;
            let workspace_config_path = workspace_identifier.config_path.clone();
            let workspace_folders = to_workspace_folders(&this.workspace_configuration.get_folders(), &workspace_config_path, &this.uri_identity_service.ext_uri());
            let workspace = Workspace::new(
                workspace_identifier.id,
                workspace_folders,
                this.workspace_configuration.is_transient(),
                Some(workspace_config_path),
                this.ignore_path_casing_predicate(),
                None,
            );
            this.workspace_initialized.set(this.workspace_configuration.initialized());
            Ok(workspace)
        })
    }

    fn create_single_folder_workspace(&self, identifier: &SingleFolderWorkspaceIdentifier) -> Rc<Workspace> {
        let name = self.uri_identity_service.ext_uri().basename_or_authority(&identifier.uri);
        let workspace = Workspace::new(identifier.id.clone(), vec![to_workspace_folder(&identifier.uri, name)], false, None, self.ignore_path_casing_predicate(), None);
        self.workspace_initialized.set(true);
        workspace
    }

    fn create_empty_workspace(&self, identifier: &EmptyWorkspaceIdentifier) -> Rc<Workspace> {
        let workspace = Workspace::new(identifier.id.clone(), Vec::new(), false, None, self.ignore_path_casing_predicate(), None);
        self.workspace_initialized.set(true);
        workspace
    }

    fn check_and_mark_workspace_complete(self: &Rc<Self>, from_cache: bool) {
        if !self.complete_workspace_barrier.is_open() && self.workspace_initialized.get() {
            self.complete_workspace_barrier.open();
            let _ = self.validate_workspace_folders_and_reload(from_cache);
        }
    }

    fn update_workspace_and_initialize_configuration(self: &Rc<Self>, workspace: Rc<Workspace>, trigger: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let has_workspace_before = this.workspace.borrow().is_some();
            let mut previous_state: Option<WorkbenchState> = None;
            let mut previous_workspace_path: Option<String> = None;
            let mut previous_folders: Vec<WorkspaceFolder> = Vec::new();

            if has_workspace_before {
                previous_state = Some(this.get_workbench_state());
                let current = this.current_workspace();
                previous_workspace_path = current.configuration().map(|configuration| je_platform::resources::fs_path(&configuration));
                previous_folders = current.folders();
                current.update(&workspace);
            } else {
                *this.workspace.borrow_mut() = Some(workspace);
            }

            this.initialize_configuration(trigger).await?;

            // Trigger changes after configuration initialization so that configuration is up to date.
            if has_workspace_before {
                let new_state = this.get_workbench_state();
                if let Some(previous_state) = previous_state
                    && new_state != previous_state
                {
                    this.on_did_change_workbench_state.fire(new_state);
                }

                let current = this.current_workspace();
                let new_workspace_path = current.configuration().map(|configuration| je_platform::resources::fs_path(&configuration));
                if previous_workspace_path.is_some() && new_workspace_path != previous_workspace_path || Some(new_state) != previous_state {
                    this.on_did_change_workspace_name.fire(());
                }

                let folder_changes = Self::compare_folders(&previous_folders, &current.folders());
                if !folder_changes.added.is_empty() || !folder_changes.removed.is_empty() || !folder_changes.changed.is_empty() {
                    this.handle_will_change_workspace_folders(&folder_changes, false).await?;
                    this.on_did_change_workspace_folders.fire(folder_changes);
                }
            }

            if !this.local_user_configuration.has_tasks_loaded() {
                // Reload local user configuration again to load user tasks
                let weak = this.this.clone();
                this.base.register(run_when_global_idle(
                    move |_| {
                        if let Some(this) = weak.upgrade() {
                            let local = this.config().local_user_configuration();
                            let _ = this.reload_local_user_configuration(false, Some(local));
                        }
                    },
                    None,
                ));
            }
            Ok(())
        })
    }

    fn compare_folders(current_folders: &[WorkspaceFolder], new_folders: &[WorkspaceFolder]) -> WorkspaceFoldersChangeEvent {
        let mut result = WorkspaceFoldersChangeEvent::default();
        result.added = new_folders
            .iter()
            .filter(|new_folder| !current_folders.iter().any(|current_folder| new_folder.uri.to_string(false) == current_folder.uri.to_string(false)))
            .cloned()
            .collect();
        for (current_index, current_folder) in current_folders.iter().enumerate() {
            let new_index = new_folders.iter().position(|new_folder| current_folder.uri.to_string(false) == new_folder.uri.to_string(false));
            match new_index {
                Some(new_index) => {
                    if current_index != new_index || current_folder.name != new_folders[new_index].name {
                        result.changed.push(current_folder.clone());
                    }
                }
                None => result.removed.push(current_folder.clone()),
            }
        }
        result
    }

    fn initialize_configuration(self: &Rc<Self>, trigger: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.default_configuration.initialize().await?;

            let init_policy_configuration_promise = this.policy_configuration.initialize();
            let init_application_configuration_promise = match this.application_configuration() {
                Some(application_configuration) => application_configuration.initialize(),
                None => Promise::resolved(ConfigurationModel::create_empty_model(this.log.clone())),
            };
            let init_user_configuration = {
                let this = this.clone();
                let init_application_configuration_promise = init_application_configuration_promise.clone();
                async_fn(async move {
                    let local = this.local_user_configuration.initialize();
                    let remote = match &this.remote_user_configuration {
                        Some(remote) => remote.initialize(),
                        None => Promise::resolved(ConfigurationModel::create_empty_model(this.log.clone())),
                    };
                    let (mut local, remote) = (local.await?, remote.await?);
                    if this.application_configuration().is_some() {
                        let application_configuration_model = init_application_configuration_promise.await?;
                        local = this.local_user_configuration.reparse(Some(ParseOptionsPatch::exclude_of(&application_configuration_model.get_value(APPLY_ALL_PROFILES_SETTING))));
                    }
                    Ok((local, remote))
                })
            };

            init_policy_configuration_promise.await?;
            let application = init_application_configuration_promise.await?;
            let (local, remote) = init_user_configuration.await?;

            this.load_configuration(application, local, remote, trigger).await
        })
    }

    fn reload_default_configuration(&self) {
        let model = self.default_configuration.reload();
        self.on_default_configuration_changed(model, None);
    }

    fn reload_application_configuration(self: &Rc<Self>, donot_trigger: bool) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let Some(application_configuration) = this.application_configuration() else {
                return Ok(ConfigurationModel::create_empty_model(this.log.clone()));
            };
            let model = application_configuration.load_configuration().await?;
            if !donot_trigger {
                this.on_application_configuration_changed(model.clone());
            }
            Ok(model)
        })
    }

    fn reload_user_configuration(self: &Rc<Self>) -> Promise<(ConfigurationModel, ConfigurationModel)> {
        let this = self.clone();
        async_fn(async move {
            let (local, remote) = (this.reload_local_user_configuration(true, None), this.reload_remote_user_configuration(true));
            Ok((local.await?, remote.await?))
        })
    }

    /// `reloadLocalUserConfiguration(donotTrigger?, settingsConfiguration?)`.
    pub fn reload_local_user_configuration(self: &Rc<Self>, donot_trigger: bool, settings_configuration: Option<ConfigurationModel>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let model = this.local_user_configuration.reload(settings_configuration).await?;
            if !donot_trigger {
                this.on_local_user_configuration_changed(model.clone());
            }
            Ok(model)
        })
    }

    fn reload_remote_user_configuration(self: &Rc<Self>, donot_trigger: bool) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            let Some(remote) = &this.remote_user_configuration else {
                return Ok(ConfigurationModel::create_empty_model(this.log.clone()));
            };
            let model = remote.reload().await?;
            if !donot_trigger {
                this.on_remote_user_configuration_changed(model.clone());
            }
            Ok(model)
        })
    }

    fn reload_workspace_configuration(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            match this.get_workbench_state() {
                WorkbenchState::Folder => {
                    let folder = this.current_workspace().folders().remove(0);
                    this.on_workspace_folder_configuration_changed(&folder).await
                }
                WorkbenchState::Workspace => {
                    this.workspace_configuration.reload().await?;
                    this.on_workspace_configuration_changed(false).await
                }
                WorkbenchState::Empty => Ok(()),
            }
        })
    }

    fn reload_workspace_folder_configuration(self: &Rc<Self>, folder: &WorkspaceFolder) -> Promise<()> {
        self.on_workspace_folder_configuration_changed(folder)
    }

    fn load_configuration(
        self: &Rc<Self>,
        application_configuration_model: ConfigurationModel,
        user_configuration_model: ConfigurationModel,
        remote_user_configuration_model: ConfigurationModel,
        trigger: bool,
    ) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // reset caches
            this.cached_folder_configs.clear_and_dispose_all();

            let folders = this.current_workspace().folders();
            let folder_configurations = this.load_folder_configurations(&folders).await?;

            let workspace_configuration = this.get_workspace_configuration_model(&folder_configurations);
            let mut folder_configuration_models: ResourceMap<ConfigurationModel> = ResourceMap::new();
            for (index, folder_configuration) in folder_configurations.into_iter().enumerate() {
                folder_configuration_models.set(folders[index].uri.clone(), folder_configuration);
            }

            let current_configuration = this.config();
            let log = this.log.clone();
            *this.configuration.borrow_mut() = Rc::new(Configuration::new(
                this.default_configuration.configuration_model(),
                this.policy_configuration.configuration_model(),
                application_configuration_model,
                user_configuration_model,
                remote_user_configuration_model,
                workspace_configuration,
                folder_configuration_models,
                ConfigurationModel::create_empty_model(log.clone()),
                ResourceMap::new(),
                Some(this.current_workspace()),
                log,
            ));

            this.initialized.set(true);

            if trigger {
                let change = this.config().compare(&current_configuration);
                this.trigger_configuration_change(
                    change,
                    Some(PreviousConfiguration { workspace: Some(this.current_workspace()), data: current_configuration.to_data() }),
                    ConfigurationTarget::Workspace,
                );
            }

            this.update_restricted_settings();
            Ok(())
        })
    }

    fn get_workspace_configuration_model(&self, folder_configurations: &[ConfigurationModel]) -> ConfigurationModel {
        match self.get_workbench_state() {
            WorkbenchState::Folder => folder_configurations[0].clone(),
            WorkbenchState::Workspace => self.workspace_configuration.get_configuration(),
            WorkbenchState::Empty => ConfigurationModel::create_empty_model(self.log.clone()),
        }
    }

    fn on_user_data_profile_changed(self: &Rc<Self>, event: DidChangeUserDataProfileEvent) {
        let this = self.clone();
        let (previous, profile) = (event.previous.clone(), event.profile.clone());
        event.join(async_fn(async move {
            let local_user = this.local_user_configuration.reset(
                profile.settings_resource.clone(),
                Some(profile.tasks_resource.clone()),
                Some(profile.mcp_resource.clone()),
                je_platform::configuration::models::ConfigurationParseOptions {
                    scopes: get_local_user_configuration_scopes(&profile, this.remote_user_configuration.is_some()),
                    ..Default::default()
                },
            );
            let uses_default_settings = |profile: &UserDataProfile| profile.use_default_flags.is_some_and(|flags| flags.settings.unwrap_or(false));
            let mut application_promise: Option<Promise<ConfigurationModel>> = None;
            if previous.is_default != profile.is_default || uses_default_settings(&previous) != uses_default_settings(&profile) {
                this.create_application_configuration();
                if this.application_configuration().is_some() {
                    application_promise = Some(this.reload_application_configuration(true));
                }
            }
            let mut local_user = local_user.await?;
            let application = match application_promise {
                Some(promise) => promise.await?,
                None => this.config().application_configuration(),
            };
            if this.application_configuration().is_some() {
                local_user = this.local_user_configuration.reparse(Some(ParseOptionsPatch::exclude_of(&application.get_value(APPLY_ALL_PROFILES_SETTING))));
            }
            this.load_configuration(application, local_user, this.config().remote_user_configuration(), true).await
        }));
    }

    fn on_default_configuration_changed(&self, configuration_model: ConfigurationModel, properties: Option<Vec<JsString>>) {
        if self.workspace.borrow().is_some() {
            let configuration = self.config();
            let workspace = self.current_workspace();
            let previous = PreviousConfiguration { workspace: Some(workspace.clone()), data: configuration.to_data() };
            let change = configuration.compare_and_update_default_configuration(configuration_model, properties);
            if let Some(application_configuration) = self.application_configuration() {
                configuration.update_application_configuration(application_configuration.reparse(None));
            }
            if let Some(remote_user_configuration) = &self.remote_user_configuration {
                configuration.update_local_user_configuration(self.local_user_configuration.reparse(None));
                configuration.update_remote_user_configuration(remote_user_configuration.reparse());
            }
            let this = self.this.upgrade().expect("o serviço está vivo");
            if self.get_workbench_state() == WorkbenchState::Folder {
                let first = workspace.folders().remove(0);
                if let Some(folder_configuration) = this.cached_folder_configs.get(&folder_key(&first.uri)) {
                    configuration.update_workspace_configuration(folder_configuration.reparse());
                    configuration.update_folder_configuration(&first.uri, folder_configuration.reparse());
                }
            } else {
                configuration.update_workspace_configuration(self.workspace_configuration.reparse_workspace_settings());
                for folder in workspace.folders() {
                    if let Some(folder_configuration) = this.cached_folder_configs.get(&folder_key(&folder.uri)) {
                        configuration.update_folder_configuration(&folder.uri, folder_configuration.reparse());
                    }
                }
            }
            self.trigger_configuration_change(change, Some(previous), ConfigurationTarget::Default);
            self.update_restricted_settings();
        }
    }

    fn previous_configuration(&self) -> PreviousConfiguration {
        PreviousConfiguration { workspace: self.workspace.borrow().clone(), data: self.config().to_data() }
    }

    fn on_policy_configuration_changed(&self, policy_configuration: ConfigurationModel) {
        let previous = self.previous_configuration();
        let change = self.config().compare_and_update_policy_configuration(policy_configuration);
        self.trigger_configuration_change(change, Some(previous), ConfigurationTarget::Default);
    }

    fn on_application_configuration_changed(&self, application_configuration: ConfigurationModel) {
        let configuration = self.config();
        let previous = self.previous_configuration();
        let previous_all_profiles_settings = Self::string_list(&configuration.application_configuration().get_value(APPLY_ALL_PROFILES_SETTING));
        let mut change = configuration.compare_and_update_application_configuration(application_configuration);
        let current_all_profiles_settings = Self::string_list(&self.get_value(Some(APPLY_ALL_PROFILES_SETTING), None));
        let configuration_properties = configuration_registry().get_configuration_properties();
        let mut changed_keys: Vec<JsString> = Vec::new();
        for changed_key in &change.keys {
            let scope = configuration_properties.get(changed_key).and_then(|property| property.scope());
            if scope.is_some_and(|scope| APPLICATION_SCOPES.contains(&scope)) {
                changed_keys.push(changed_key.clone());
                if changed_key.to_string_lossy() == APPLY_ALL_PROFILES_SETTING {
                    for previous_all_profile_setting in &previous_all_profiles_settings {
                        if !current_all_profiles_settings.contains(previous_all_profile_setting) {
                            changed_keys.push(previous_all_profile_setting.clone());
                        }
                    }
                    for current_all_profile_setting in &current_all_profiles_settings {
                        if !previous_all_profiles_settings.contains(current_all_profile_setting) {
                            changed_keys.push(current_all_profile_setting.clone());
                        }
                    }
                }
            } else if current_all_profiles_settings.contains(changed_key) {
                changed_keys.push(changed_key.clone());
            }
        }
        change.keys = changed_keys;
        if change.keys.iter().any(|key| key.to_string_lossy() == APPLY_ALL_PROFILES_SETTING) {
            configuration.update_local_user_configuration(self.local_user_configuration.reparse(Some(ParseOptionsPatch { exclude: Some(current_all_profiles_settings) })));
        }
        self.trigger_configuration_change(change, Some(previous), ConfigurationTarget::User);
    }

    /// `getValue<string[]>(...) ?? []` com a lista de textos.
    fn string_list(value: &Dynamic) -> Vec<JsString> {
        match value {
            Dynamic::Array(items) => items
                .to_vec()
                .into_iter()
                .filter_map(|item| match item {
                    Dynamic::String(text) => Some(text),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn on_local_user_configuration_changed(&self, user_configuration: ConfigurationModel) {
        let previous = self.previous_configuration();
        let change = self.config().compare_and_update_local_user_configuration(user_configuration);
        self.trigger_configuration_change(change, Some(previous), ConfigurationTarget::User);
    }

    fn on_remote_user_configuration_changed(&self, user_configuration: ConfigurationModel) {
        let previous = self.previous_configuration();
        let change = self.config().compare_and_update_remote_user_configuration(user_configuration);
        self.trigger_configuration_change(change, Some(previous), ConfigurationTarget::User);
    }

    fn on_workspace_configuration_changed(self: &Rc<Self>, from_cache: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let workspace = this.workspace.borrow().clone();
            if let Some(workspace) = workspace
                && let Some(configuration) = workspace.configuration()
            {
                let ext_uri = this.uri_identity_service.ext_uri();
                let mut new_folders = to_workspace_folders(&this.workspace_configuration.get_folders(), &configuration, &ext_uri);

                // Validate only if workspace is initialized
                if this.workspace_initialized.get() {
                    let WorkspaceFoldersChangeEvent { added, removed, changed } = Self::compare_folders(&workspace.folders(), &new_folders);

                    // If changed validate new folders
                    if !added.is_empty() || !removed.is_empty() || !changed.is_empty() {
                        new_folders = this.to_valid_workspace_folders(&new_folders).await?;
                    }
                    // Otherwise use existing
                    else {
                        new_folders = workspace.folders();
                    }
                }

                this.update_workspace_configuration(new_folders, this.workspace_configuration.get_configuration(), from_cache).await?;
            }
            Ok(())
        })
    }

    fn update_restricted_settings(&self) {
        let this = self.this.upgrade().expect("o serviço está vivo");
        let previous = self.restricted_settings.borrow().clone();
        let mut changed = false;

        let all_properties = configuration_registry().get_configuration_properties();
        let default_restricted_settings = sorted(all_properties.entries().into_iter().filter(|(_, property)| property.restricted()).map(|(key, _)| key).collect());
        changed |= differs(&default_restricted_settings, &previous.default);

        let application = sorted(self.application_configuration().map(|c| c.get_restricted_settings()).unwrap_or_default());
        changed |= differs(&application, previous.application.as_deref().unwrap_or_default());

        let user_local = sorted(self.local_user_configuration.get_restricted_settings());
        changed |= differs(&user_local, previous.user_local.as_deref().unwrap_or_default());

        let user_remote = sorted(self.remote_user_configuration.as_ref().map(|c| c.get_restricted_settings()).unwrap_or_default());
        changed |= differs(&user_remote, previous.user_remote.as_deref().unwrap_or_default());

        let mut workspace_folder_map: ResourceMap<Vec<JsString>> = ResourceMap::new();
        let folders = self.current_workspace().folders();
        for workspace_folder in &folders {
            let folder_restricted_settings = sorted(this.cached_folder_configs.get(&folder_key(&workspace_folder.uri)).map(|c| c.get_restricted_settings()).unwrap_or_default());
            if !folder_restricted_settings.is_empty() {
                workspace_folder_map.set(workspace_folder.uri.clone(), folder_restricted_settings.clone());
            }
            let previous_folder = previous.workspace_folder.as_ref().and_then(|map| map.get(&workspace_folder.uri).cloned()).unwrap_or_default();
            changed |= differs(&folder_restricted_settings, &previous_folder);
        }

        let workspace = if self.get_workbench_state() == WorkbenchState::Workspace {
            sorted(self.workspace_configuration.get_restricted_settings())
        } else {
            folders.first().and_then(|folder| workspace_folder_map.get(&folder.uri).cloned()).unwrap_or_default()
        };
        changed |= differs(&workspace, previous.workspace.as_deref().unwrap_or_default());

        if changed {
            let non_empty = |list: Vec<JsString>| (!list.is_empty()).then_some(list);
            *self.restricted_settings.borrow_mut() = RestrictedSettings {
                default: default_restricted_settings,
                application: non_empty(application),
                user_local: non_empty(user_local),
                user_remote: non_empty(user_remote),
                workspace: non_empty(workspace),
                workspace_folder: (workspace_folder_map.size() > 0).then(|| Rc::new(workspace_folder_map)),
            };
            self.on_did_change_restricted_settings.fire(self.restricted_settings());
        }
    }

    fn update_workspace_configuration(self: &Rc<Self>, workspace_folders: Vec<WorkspaceFolder>, configuration: ConfigurationModel, from_cache: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let previous = this.previous_configuration();
            let change = this.config().compare_and_update_workspace_configuration(configuration);
            let workspace = this.current_workspace();
            let changes = Self::compare_folders(&workspace.folders(), &workspace_folders);
            if !changes.added.is_empty() || !changes.removed.is_empty() || !changes.changed.is_empty() {
                workspace.set_folders(workspace_folders);
                let change = this.on_folders_changed().await?;
                this.handle_will_change_workspace_folders(&changes, from_cache).await?;
                this.trigger_configuration_change(change, Some(previous), ConfigurationTarget::WorkspaceFolder);
                this.on_did_change_workspace_folders.fire(changes);
            } else {
                this.trigger_configuration_change(change, Some(previous), ConfigurationTarget::Workspace);
            }
            this.update_restricted_settings();
            Ok(())
        })
    }

    fn handle_will_change_workspace_folders(self: &Rc<Self>, changes: &WorkspaceFoldersChangeEvent, from_cache: bool) -> Promise<()> {
        let event = WorkspaceFoldersWillChangeEvent::new(changes.clone(), from_cache);
        self.on_will_change_workspace_folders.fire(event.clone());
        // Ignore the errors of the joiners
        promises::settled(event.joined()).then_result(|_| Ok(()))
    }

    fn on_workspace_folder_configuration_changed(self: &Rc<Self>, folder: &WorkspaceFolder) -> Promise<()> {
        let (this, folder) = (self.clone(), folder.clone());
        async_fn(async move {
            let folder_configuration = this.load_folder_configurations(std::slice::from_ref(&folder)).await?.remove(0);
            let previous = this.previous_configuration();
            let configuration = this.config();
            let folder_configuration_change = configuration.compare_and_update_folder_configuration(&folder.uri, folder_configuration.clone());
            if this.get_workbench_state() == WorkbenchState::Folder {
                let workspace_configuration_change = configuration.compare_and_update_workspace_configuration(folder_configuration);
                this.trigger_configuration_change(merge_changes(&[folder_configuration_change, workspace_configuration_change]), Some(previous), ConfigurationTarget::Workspace);
            } else {
                this.trigger_configuration_change(folder_configuration_change, Some(previous), ConfigurationTarget::WorkspaceFolder);
            }
            this.update_restricted_settings();
            Ok(())
        })
    }

    fn on_folders_changed(self: &Rc<Self>) -> Promise<ConfigurationChange> {
        let this = self.clone();
        async_fn(async move {
            let mut changes: Vec<ConfigurationChange> = Vec::new();
            let folders = this.current_workspace().folders();
            let configuration = this.config();

            // Remove the configurations of deleted folders
            for key in this.cached_folder_configs.keys() {
                if !folders.iter().any(|folder| folder_key(&folder.uri) == key) {
                    this.cached_folder_configs.delete_and_dispose(&key);
                    let uri = configuration.folder_configurations().borrow().keys().find(|uri| folder_key(uri) == key).cloned();
                    if let Some(uri) = uri {
                        changes.push(configuration.compare_and_delete_folder_configuration(&uri));
                    }
                }
            }

            let to_initialize: Vec<WorkspaceFolder> = folders.into_iter().filter(|folder| !this.cached_folder_configs.has(&folder_key(&folder.uri))).collect();
            if !to_initialize.is_empty() {
                let folder_configurations = this.load_folder_configurations(&to_initialize).await?;
                for (index, folder_configuration) in folder_configurations.into_iter().enumerate() {
                    changes.push(configuration.compare_and_update_folder_configuration(&to_initialize[index].uri, folder_configuration));
                }
            }
            Ok(merge_changes(&changes))
        })
    }

    fn load_folder_configurations(self: &Rc<Self>, folders: &[WorkspaceFolder]) -> Promise<Vec<ConfigurationModel>> {
        Promise::all(
            folders
                .iter()
                .map(|folder| {
                    let key = folder_key(&folder.uri);
                    let folder_configuration = match self.cached_folder_configs.get(&key) {
                        Some(folder_configuration) => folder_configuration,
                        None => {
                            let folder_configuration = FolderConfiguration::new(
                                !self.initialized.get(),
                                folder.clone(),
                                FOLDER_CONFIG_FOLDER_NAME,
                                self.get_workbench_state(),
                                self.is_workspace_trusted.get(),
                                self.file_service.clone(),
                                &*self.uri_identity_service,
                                self.log.clone(),
                                self.configuration_cache.clone(),
                            );
                            let (weak, folder) = (self.this.clone(), folder.clone());
                            folder_configuration.add_related(folder_configuration.on_did_change().listen(move |()| {
                                if let Some(this) = weak.upgrade() {
                                    let _ = this.on_workspace_folder_configuration_changed(&folder);
                                }
                            }));
                            self.cached_folder_configs.set(key, folder_configuration.clone(), false);
                            folder_configuration
                        }
                    };
                    folder_configuration.load_configuration()
                })
                .collect(),
        )
    }

    fn validate_workspace_folders_and_reload(self: &Rc<Self>, from_cache: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let workspace = this.current_workspace();
            let valid_workspace_folders = this.to_valid_workspace_folders(&workspace.folders()).await?;
            let WorkspaceFoldersChangeEvent { removed, .. } = Self::compare_folders(&workspace.folders(), &valid_workspace_folders);
            if !removed.is_empty() {
                this.update_workspace_configuration(valid_workspace_folders, this.workspace_configuration.get_configuration(), from_cache).await?;
            }
            Ok(())
        })
    }

    // Filter out workspace folders which are files (not directories)
    // Workspace folders those cannot be resolved are not filtered because they are handled by the Explorer.
    fn to_valid_workspace_folders(self: &Rc<Self>, workspace_folders: &[WorkspaceFolder]) -> Promise<Vec<WorkspaceFolder>> {
        let (this, workspace_folders) = (self.clone(), workspace_folders.to_vec());
        async_fn(async move {
            let mut valid_workspace_folders = Vec::new();
            for workspace_folder in workspace_folders {
                match this.file_service.stat(&workspace_folder.uri).await {
                    Ok(result) => {
                        if !result.is_directory {
                            continue;
                        }
                    }
                    Err(error) => this.log.warn(&format!("Ignoring the error while validating workspace folder {} - {}", workspace_folder.uri.to_string(false), error.message)),
                }
                valid_workspace_folders.push(workspace_folder);
            }
            Ok(valid_workspace_folders)
        })
    }

    fn write_configuration_value(
        self: &Rc<Self>,
        key: &str,
        value: Dynamic,
        target: ConfigurationTarget,
        overrides: Option<ConfigurationUpdateOverrides>,
        options: Option<ConfigurationUpdateOptions>,
    ) -> Promise<()> {
        let (this, key) = (self.clone(), key.to_owned());
        async_fn(async move {
            let Some(instantiation) = this.instantiation.borrow().clone() else {
                return Err(Error::new("Cannot write configuration because the configuration service is not yet ready to accept writes."));
            };

            if target == ConfigurationTarget::Default {
                return Err(Error::new("Invalid configuration target"));
            }

            if target == ConfigurationTarget::Memory {
                let previous = this.previous_configuration();
                let overrides_value = overrides.clone().unwrap_or_default();
                this.config().update_value(key.as_str(), value, &overrides_value);
                let identifiers = overrides_value.override_identifiers.clone().filter(|ids| !ids.is_empty());
                this.trigger_configuration_change(
                    ConfigurationChange {
                        keys: match &identifiers {
                            Some(ids) => vec![key_from_override_identifiers(ids), JsString::from(key.as_str())],
                            None => vec![JsString::from(key.as_str())],
                        },
                        overrides: identifiers.map(|ids| ids.into_iter().map(|id| (id, vec![JsString::from(key.as_str())])).collect()).unwrap_or_default(),
                    },
                    Some(previous),
                    target,
                );
                return Ok(());
            }

            let Some(editable_configuration_target) = this.to_editable_configuration_target(target, &key) else {
                return Err(Error::new("Invalid configuration target"));
            };

            if editable_configuration_target == EditableConfigurationTarget::UserRemote && this.remote_user_configuration.is_none() {
                return Err(Error::new("Invalid configuration target"));
            }

            let mut overrides = overrides;
            if let Some(overrides) = overrides.as_mut()
                && let Some(identifiers) = overrides.override_identifiers.as_mut()
                && identifiers.len() > 1
            {
                let configuration_model = this.get_configuration_model_for_editable_configuration_target(editable_configuration_target, overrides.resource.as_ref());
                if let Some(configuration_model) = configuration_model {
                    identifiers.sort_by(|a, b| a.units().cmp(b.units()));
                    let sorted_identifiers = identifiers.clone();
                    let existing_overrides = configuration_model.overrides().into_iter().find(|existing| {
                        let mut existing_identifiers = existing.identifiers.clone();
                        existing_identifiers.sort_by(|a, b| a.units().cmp(b.units()));
                        existing_identifiers == sorted_identifiers
                    });
                    if let Some(existing_overrides) = existing_overrides {
                        *identifiers = existing_overrides.identifiers;
                    }
                }
            }

            // Use same instance of ConfigurationEditing to make sure all writes go through the same queue
            let configuration_editing = {
                let existing = this.configuration_editing.borrow().clone();
                match existing {
                    Some(promise) => promise,
                    None => {
                        let target_service = this.clone();
                        let promise = target_service.remote_agent_service.get_environment().then(move |environment| instantiation.create_configuration_editing(environment.map(|environment| environment.settings_path)));
                        *this.configuration_editing.borrow_mut() = Some(promise.clone());
                        promise
                    }
                }
            };
            configuration_editing
                .await?
                .write_configuration(
                    editable_configuration_target,
                    ConfigurationEditingValue { key: key.clone(), value },
                    ConfigurationEditingOptions { options: options.unwrap_or_default(), scopes: overrides.clone() },
                )
                .await?;
            match editable_configuration_target {
                EditableConfigurationTarget::UserLocal => {
                    if this.application_configuration().is_some() && this.is_setting_applied_for_all_profiles(&key) {
                        this.reload_application_configuration(false).await?;
                    } else {
                        this.reload_local_user_configuration(false, None).await?;
                    }
                    Ok(())
                }
                EditableConfigurationTarget::UserRemote => this.reload_remote_user_configuration(false).await.map(|_| ()),
                EditableConfigurationTarget::Workspace => this.reload_workspace_configuration().await,
                EditableConfigurationTarget::WorkspaceFolder => {
                    let workspace_folder = overrides.as_ref().and_then(|overrides| overrides.resource.as_ref()).and_then(|resource| this.current_workspace().get_folder(resource));
                    match workspace_folder {
                        Some(workspace_folder) => this.reload_workspace_folder_configuration(&workspace_folder).await,
                        None => Ok(()),
                    }
                }
            }
        })
    }

    fn get_configuration_model_for_editable_configuration_target(&self, target: EditableConfigurationTarget, resource: Option<&URI>) -> Option<ConfigurationModel> {
        let configuration = self.config();
        match target {
            EditableConfigurationTarget::UserLocal => Some(configuration.local_user_configuration()),
            EditableConfigurationTarget::UserRemote => Some(configuration.remote_user_configuration()),
            EditableConfigurationTarget::Workspace => Some(configuration.workspace_configuration()),
            EditableConfigurationTarget::WorkspaceFolder => resource.and_then(|resource| configuration.folder_configurations().borrow().get(resource).cloned()),
        }
    }

    /// `getConfigurationModel(target, resource?)`.
    pub fn get_configuration_model(&self, target: ConfigurationTarget, resource: Option<&URI>) -> Option<ConfigurationModel> {
        let configuration = self.config();
        match target {
            ConfigurationTarget::UserLocal => Some(configuration.local_user_configuration()),
            ConfigurationTarget::UserRemote => Some(configuration.remote_user_configuration()),
            ConfigurationTarget::Workspace => Some(configuration.workspace_configuration()),
            ConfigurationTarget::WorkspaceFolder => resource.and_then(|resource| configuration.folder_configurations().borrow().get(resource).cloned()),
            _ => None,
        }
    }

    fn derive_configuration_targets(value: &Dynamic, inspect: &ConfigurationInspectValue) -> Vec<ConfigurationTarget> {
        if objects::equals(value, &inspect.value()) {
            return Vec::new();
        }

        let mut defined_targets: Vec<ConfigurationTarget> = Vec::new();
        if !inspect.workspace_folder_value().is_undefined() {
            defined_targets.push(ConfigurationTarget::WorkspaceFolder);
        }
        if !inspect.workspace_value().is_undefined() {
            defined_targets.push(ConfigurationTarget::Workspace);
        }
        if !inspect.user_remote_value().is_undefined() {
            defined_targets.push(ConfigurationTarget::UserRemote);
        }
        if !inspect.user_local_value().is_undefined() {
            defined_targets.push(ConfigurationTarget::UserLocal);
        }
        if !inspect.application_value().is_undefined() {
            defined_targets.push(ConfigurationTarget::Application);
        }

        if value.is_undefined() {
            // Remove the setting in all defined targets
            return defined_targets;
        }

        vec![defined_targets.first().copied().unwrap_or(ConfigurationTarget::User)]
    }

    fn trigger_configuration_change(&self, change: ConfigurationChange, previous: Option<PreviousConfiguration>, target: ConfigurationTarget) {
        if !change.keys.is_empty() {
            if target != ConfigurationTarget::Default {
                self.log.debug(&format!(
                    "Configuration keys changed in {} target {}",
                    configuration_target_to_string(target),
                    change.keys.iter().map(JsString::to_string_lossy).collect::<Vec<_>>().join(" ")
                ));
            }
            let configuration_change_event = ConfigurationChangeEvent::new(change, previous, self.config().base(), self.workspace.borrow().clone(), self.log.clone());
            configuration_change_event.source.set(Some(target));
            self.on_did_change_configuration.fire(Rc::new(configuration_change_event));
        }
    }

    fn to_editable_configuration_target(&self, target: ConfigurationTarget, key: &str) -> Option<EditableConfigurationTarget> {
        match target {
            ConfigurationTarget::Application => Some(EditableConfigurationTarget::UserLocal),
            ConfigurationTarget::User => {
                if self.remote_user_configuration.is_some() {
                    let scope = configuration_registry().get_configuration_properties().get(key).and_then(|property| property.scope());
                    if matches!(scope, Some(ConfigurationScope::Machine | ConfigurationScope::MachineOverridable | ConfigurationScope::ApplicationMachine)) {
                        return Some(EditableConfigurationTarget::UserRemote);
                    }
                    if !self.inspect(key, &ConfigurationOverrides::default()).user_remote_value().is_undefined() {
                        return Some(EditableConfigurationTarget::UserRemote);
                    }
                }
                Some(EditableConfigurationTarget::UserLocal)
            }
            ConfigurationTarget::UserLocal => Some(EditableConfigurationTarget::UserLocal),
            ConfigurationTarget::UserRemote => Some(EditableConfigurationTarget::UserRemote),
            ConfigurationTarget::Workspace => Some(EditableConfigurationTarget::Workspace),
            ConfigurationTarget::WorkspaceFolder => Some(EditableConfigurationTarget::WorkspaceFolder),
            ConfigurationTarget::Default | ConfigurationTarget::Memory => None,
        }
    }
}

impl IDisposable for WorkspaceService {
    fn dispose(&self) {
        self.base.dispose();
        self.workspace_editing_queue.dispose();
    }
}

impl IConfigurationService for WorkspaceService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        WorkspaceService::on_did_change_configuration(self)
    }
    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        Some(WorkspaceService::get_configuration_data(self))
    }
    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        WorkspaceService::get_value(self, section, overrides)
    }
    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        match self.this.upgrade() {
            Some(this) => WorkspaceService::update_value(&this, key, value, overrides, target, None),
            None => Promise::rejected(Error::new("WorkspaceService is disposed")),
        }
    }
    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(WorkspaceService::inspect(self, key, overrides))
    }
    fn reload_configuration(&self) -> Promise<()> {
        match self.this.upgrade() {
            Some(this) => WorkspaceService::reload_configuration_for(&this, None),
            None => Promise::rejected(Error::new("WorkspaceService is disposed")),
        }
    }
    fn keys(&self) -> ConfigurationKeys {
        WorkspaceService::keys(self)
    }
}

impl IWorkbenchConfigurationService for WorkspaceService {
    fn restricted_settings(&self) -> RestrictedSettings {
        WorkspaceService::restricted_settings(self)
    }
    fn on_did_change_restricted_settings(&self) -> Event<RestrictedSettings> {
        WorkspaceService::on_did_change_restricted_settings(self)
    }
    fn when_remote_configuration_loaded(&self) -> Promise<()> {
        WorkspaceService::when_remote_configuration_loaded(self)
    }
    fn initialize(&self, arg: &AnyWorkspaceIdentifier) -> Promise<()> {
        match self.this.upgrade() {
            Some(this) => WorkspaceService::initialize(&this, arg),
            None => Promise::rejected(Error::new("WorkspaceService is disposed")),
        }
    }
    fn is_setting_applied_for_all_profiles(&self, setting: &str) -> bool {
        WorkspaceService::is_setting_applied_for_all_profiles(self, setting)
    }
}

impl IWorkspaceContextService for WorkspaceService {
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState> {
        WorkspaceService::on_did_change_workbench_state(self)
    }
    fn on_did_change_workspace_name(&self) -> Event<()> {
        WorkspaceService::on_did_change_workspace_name(self)
    }
    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent> {
        WorkspaceService::on_will_change_workspace_folders(self)
    }
    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent> {
        WorkspaceService::on_did_change_workspace_folders(self)
    }
    fn get_complete_workspace(&self) -> Promise<Rc<Workspace>> {
        WorkspaceService::get_complete_workspace(self)
    }
    fn get_workspace(&self) -> Rc<Workspace> {
        WorkspaceService::get_workspace(self)
    }
    fn get_workbench_state(&self) -> WorkbenchState {
        WorkspaceService::get_workbench_state(self)
    }
    fn get_workspace_folder(&self, resource: &URI) -> Option<WorkspaceFolder> {
        WorkspaceService::get_workspace_folder(self, resource)
    }
    fn is_current_workspace(&self, workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool {
        WorkspaceService::is_current_workspace(self, workspace_id_or_folder)
    }
    fn is_inside_workspace(&self, resource: &URI) -> bool {
        WorkspaceService::is_inside_workspace(self, resource)
    }
    fn has_workspace_data(&self) -> bool {
        WorkspaceService::has_workspace_data(self)
    }
}
