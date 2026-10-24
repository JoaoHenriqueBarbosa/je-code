//! Porte de `src/vs/platform/userDataProfile/common/userDataProfile.ts` e `node/userDataProfile.ts`.
//!
//! Representação:
//! - O `IUserDataProfile` é um valor ([`UserDataProfile`]); os campos que o original muda em lugar (`workspaces`)
//!   são alterados no estado do serviço, e quem lê recebe cópias.
//! - O armazenamento (`getStoredProfiles` & companhia, métodos protegidos que as subclasses sobrescrevem) é o trait
//!   [`ProfilesStore`]; [`InMemoryProfilesStore`] é o `InMemoryUserDataProfilesService` e [`StateProfilesStore`] é o
//!   `UserDataProfilesService` do `node`, sobre o serviço de estado (`storage.json`).
//! - `IUriIdentityService.extUri` é um [`ExtUri`] entregue ao construtor.
//! - O `IEnvironmentService` que o serviço lê é [`UserDataProfilesEnvironment`]: só os caminhos que ele usa.

use crate::files::i_file_service::IFileService;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{async_fn, promises};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_instantiation::service_id;
use zyarr::RegExp;

use crate::files::file_service::FileService;
use crate::files::files::{FileOperationResult, PartialFileDeleteOptions, to_file_operation_result};
use crate::log::LogServiceRef;
use crate::state::StateService;
use crate::workspace::AnyWorkspaceIdentifier;

pub const AGENTS_WINDOW_PROFILE_ID: &str = "agents";

/// `ProfileResourceType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileResourceType {
    Settings,
    Keybindings,
    Snippets,
    Prompts,
    Tasks,
    Extensions,
    GlobalState,
    Mcp,
    LanguageModels,
}

impl ProfileResourceType {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileResourceType::Settings => "settings",
            ProfileResourceType::Keybindings => "keybindings",
            ProfileResourceType::Snippets => "snippets",
            ProfileResourceType::Prompts => "prompts",
            ProfileResourceType::Tasks => "tasks",
            ProfileResourceType::Extensions => "extensions",
            ProfileResourceType::GlobalState => "globalState",
            ProfileResourceType::Mcp => "mcp",
            ProfileResourceType::LanguageModels => "languageModels",
        }
    }
}

/// `UseDefaultProfileFlags` (`ProfileResourceTypeFlags`): quais recursos o perfil pega do perfil padrão.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseDefaultProfileFlags {
    pub settings: Option<bool>,
    pub keybindings: Option<bool>,
    pub snippets: Option<bool>,
    pub prompts: Option<bool>,
    pub tasks: Option<bool>,
    pub extensions: Option<bool>,
    pub global_state: Option<bool>,
    pub mcp: Option<bool>,
    pub language_models: Option<bool>,
}

fn agents_window_profile_flags() -> UseDefaultProfileFlags {
    UseDefaultProfileFlags {
        settings: Some(true),
        keybindings: Some(true),
        prompts: Some(true),
        mcp: Some(true),
        language_models: Some(true),
        snippets: Some(true),
        tasks: Some(true),
        extensions: Some(true),
        global_state: None,
    }
}

impl UseDefaultProfileFlags {
    fn to_dynamic(self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        for (name, value) in [
            (ProfileResourceType::Settings, self.settings),
            (ProfileResourceType::Keybindings, self.keybindings),
            (ProfileResourceType::Snippets, self.snippets),
            (ProfileResourceType::Prompts, self.prompts),
            (ProfileResourceType::Tasks, self.tasks),
            (ProfileResourceType::Extensions, self.extensions),
            (ProfileResourceType::GlobalState, self.global_state),
            (ProfileResourceType::Mcp, self.mcp),
            (ProfileResourceType::LanguageModels, self.language_models),
        ] {
            if let Some(value) = value {
                entries.push((name.as_str(), Dynamic::Bool(value)));
            }
        }
        Dynamic::object(entries)
    }

    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        let flag = |name: ProfileResourceType| match value.get_str(name.as_str()) {
            Dynamic::Bool(b) => Some(b),
            _ => None,
        };
        Some(Self {
            settings: flag(ProfileResourceType::Settings),
            keybindings: flag(ProfileResourceType::Keybindings),
            snippets: flag(ProfileResourceType::Snippets),
            prompts: flag(ProfileResourceType::Prompts),
            tasks: flag(ProfileResourceType::Tasks),
            extensions: flag(ProfileResourceType::Extensions),
            global_state: flag(ProfileResourceType::GlobalState),
            mcp: flag(ProfileResourceType::Mcp),
            language_models: flag(ProfileResourceType::LanguageModels),
        })
    }
}

/// `IUserDataProfile`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserDataProfile {
    pub id: String,
    pub is_default: bool,
    pub name: String,
    pub icon: Option<String>,
    pub location: URI,
    pub global_storage_home: URI,
    pub settings_resource: URI,
    pub keybindings_resource: URI,
    pub tasks_resource: URI,
    pub snippets_home: URI,
    pub prompts_home: URI,
    pub extensions_resource: URI,
    pub mcp_resource: URI,
    pub language_models_resource: URI,
    pub agent_plugins_home: URI,
    pub cache_home: URI,
    pub use_default_flags: Option<UseDefaultProfileFlags>,
    pub is_internal: Option<bool>,
    pub is_transient: Option<bool>,
    pub is_agents_window_profile: Option<bool>,
    pub workspaces: Option<Vec<URI>>,
}

/// `IUserDataProfileOptions`.
#[derive(Clone, Debug, Default)]
pub struct UserDataProfileOptions {
    pub icon: Option<String>,
    pub use_default_flags: Option<UseDefaultProfileFlags>,
    pub transient: Option<bool>,
    pub workspaces: Option<Vec<URI>>,
}

/// `IUserDataProfileUpdateOptions`: `icon` `Some(None)` é o `null` do original (remover o ícone).
#[derive(Clone, Debug, Default)]
pub struct UserDataProfileUpdateOptions {
    pub name: Option<String>,
    pub icon: Option<Option<String>>,
    pub use_default_flags: Option<UseDefaultProfileFlags>,
    pub transient: Option<bool>,
    pub workspaces: Option<Vec<URI>>,
}

/// `DidChangeProfilesEvent`.
#[derive(Clone, Debug)]
pub struct DidChangeProfilesEvent {
    pub added: Vec<UserDataProfile>,
    pub removed: Vec<UserDataProfile>,
    pub updated: Vec<UserDataProfile>,
    pub all: Vec<UserDataProfile>,
}

/// `WillCreateProfileEvent` e `WillRemoveProfileEvent`: o perfil e o `join` das promessas que o emissor espera.
#[derive(Clone)]
pub struct WillChangeProfileEvent {
    pub profile: UserDataProfile,
    joiners: Rc<RefCell<Vec<Promise<()>>>>,
}

impl WillChangeProfileEvent {
    fn new(profile: UserDataProfile) -> Self {
        Self { profile, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    pub fn join(&self, promise: Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }
}

/// `IUserDataProfilesService`.
pub trait IUserDataProfilesService {
    fn profiles_home(&self) -> URI;
    fn default_profile(&self) -> UserDataProfile;
    fn on_did_change_profiles(&self) -> Event<DidChangeProfilesEvent>;
    fn profiles(&self) -> Vec<UserDataProfile>;
    fn on_did_reset_workspaces(&self) -> Event<()>;
    fn create_named_profile(&self, name: &str, options: Option<UserDataProfileOptions>, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile>;
    fn create_transient_profile(&self, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile>;
    fn create_profile(&self, id: &str, name: &str, options: Option<UserDataProfileOptions>, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile>;
    fn update_profile(&self, profile: &UserDataProfile, options: UserDataProfileUpdateOptions) -> Promise<UserDataProfile>;
    fn remove_profile(&self, profile: &UserDataProfile) -> Promise<()>;
    fn set_profile_for_workspace(&self, workspace_identifier: &AnyWorkspaceIdentifier, profile: &UserDataProfile) -> Promise<()>;
    fn reset_workspaces(&self) -> Promise<()>;
    fn clean_up(&self) -> Promise<()>;
    fn clean_up_transient_profiles(&self) -> Promise<()>;
}

service_id!(pub I_USER_DATA_PROFILES_SERVICE: dyn IUserDataProfilesService = "IUserDataProfilesService");

/// `toUserDataProfile`.
pub fn to_user_data_profile(
    id: &str,
    name: &str,
    location: &URI,
    profiles_cache_home: &URI,
    options: Option<&UserDataProfileOptions>,
    default_profile: Option<&UserDataProfile>,
    ext_uri: &ExtUri,
) -> UserDataProfile {
    let is_agents_window_profile = id == AGENTS_WINDOW_PROFILE_ID;
    let flags = options.and_then(|o| o.use_default_flags).unwrap_or_default();
    let join = |fragment: &str| ext_uri.join_path(location, &[fragment]);
    let pick = |used: Option<bool>, from_default: fn(&UserDataProfile) -> &URI, fragment: &str| match default_profile {
        Some(default) if used == Some(true) => from_default(default).clone(),
        _ => join(fragment),
    };
    UserDataProfile {
        id: id.to_string(),
        name: name.to_string(),
        location: location.clone(),
        is_default: false,
        icon: options.and_then(|o| o.icon.clone()),
        global_storage_home: pick(flags.global_state, |p| &p.global_storage_home, "globalStorage"),
        settings_resource: pick(flags.settings, |p| &p.settings_resource, "settings.json"),
        keybindings_resource: pick(flags.keybindings, |p| &p.keybindings_resource, "keybindings.json"),
        tasks_resource: pick(flags.tasks, |p| &p.tasks_resource, "tasks.json"),
        snippets_home: pick(flags.snippets, |p| &p.snippets_home, "snippets"),
        prompts_home: pick(flags.prompts, |p| &p.prompts_home, "prompts"),
        extensions_resource: pick(flags.extensions, |p| &p.extensions_resource, "extensions.json"),
        mcp_resource: pick(flags.mcp, |p| &p.mcp_resource, "mcp.json"),
        language_models_resource: pick(flags.language_models, |p| &p.language_models_resource, "chatLanguageModels.json"),
        agent_plugins_home: match default_profile {
            Some(default) => default.agent_plugins_home.clone(),
            None => join("agent-plugins"),
        },
        cache_home: ext_uri.join_path(profiles_cache_home, &[id]),
        use_default_flags: options.and_then(|o| o.use_default_flags),
        is_transient: options.and_then(|o| o.transient),
        is_internal: Some(is_agents_window_profile || options.and_then(|o| o.transient).unwrap_or(false)),
        is_agents_window_profile: Some(is_agents_window_profile),
        workspaces: options.and_then(|o| o.workspaces.clone()),
    }
}

/// `StoredUserDataProfile`. `location` e `name` ausentes ou vazios marcam o perfil como inválido.
#[derive(Clone, Debug)]
pub struct StoredUserDataProfile {
    pub name: Option<String>,
    pub location: Option<URI>,
    pub icon: Option<String>,
    pub use_default_flags: Option<UseDefaultProfileFlags>,
}

/// `StoredProfileAssociations`: `workspaces` e `emptyWindows` (da URI do workspace ou da janela vazia ao id do perfil).
#[derive(Clone, Debug, Default)]
pub struct StoredProfileAssociations {
    pub workspaces: Option<Vec<(String, String)>>,
    pub empty_windows: Option<Vec<(String, String)>>,
}

/// O armazenamento dos perfis e das associações (os métodos protegidos do original).
pub trait ProfilesStore {
    fn get_stored_profiles(&self, profiles_home: &URI, ext_uri: &ExtUri) -> Vec<StoredUserDataProfile>;
    fn save_stored_profiles(&self, stored_profiles: Vec<StoredUserDataProfile>, profiles_home: &URI, ext_uri: &ExtUri);
    fn get_stored_profile_associations(&self) -> StoredProfileAssociations;
    fn save_stored_profile_associations(&self, stored_profile_associations: StoredProfileAssociations);
    fn get_default_profile_extensions_location(&self) -> Option<URI> {
        None
    }
}

/// `InMemoryUserDataProfilesService`: guarda tudo em memória.
#[derive(Default)]
pub struct InMemoryProfilesStore {
    stored_profiles: RefCell<Vec<StoredUserDataProfile>>,
    stored_profile_associations: RefCell<StoredProfileAssociations>,
}

impl ProfilesStore for InMemoryProfilesStore {
    fn get_stored_profiles(&self, _profiles_home: &URI, _ext_uri: &ExtUri) -> Vec<StoredUserDataProfile> {
        self.stored_profiles.borrow().clone()
    }
    fn save_stored_profiles(&self, stored_profiles: Vec<StoredUserDataProfile>, _profiles_home: &URI, _ext_uri: &ExtUri) {
        *self.stored_profiles.borrow_mut() = stored_profiles;
    }
    fn get_stored_profile_associations(&self) -> StoredProfileAssociations {
        self.stored_profile_associations.borrow().clone()
    }
    fn save_stored_profile_associations(&self, stored_profile_associations: StoredProfileAssociations) {
        *self.stored_profile_associations.borrow_mut() = stored_profile_associations;
    }
}

const PROFILES_KEY: &str = "userDataProfiles";
const PROFILE_ASSOCIATIONS_KEY: &str = "profileAssociations";

/// O `UserDataProfilesService` do `node`: lê e grava as chaves `userDataProfiles` e `profileAssociations` do estado
/// (o `storage.json`), o formato que o VS Code escreve.
pub struct StateProfilesStore {
    state_service: Rc<dyn StateService>,
    default_extensions_location: Option<URI>,
}

impl StateProfilesStore {
    /// `default_extensions_location` é `extensionsPath/extensions.json`, no esquema da pasta de perfis.
    pub fn new(state_service: Rc<dyn StateService>, default_extensions_location: Option<URI>) -> Self {
        Self { state_service, default_extensions_location }
    }
}

fn string_pairs(value: &Dynamic) -> Option<Vec<(String, String)>> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    Some(
        value
            .own_entries()
            .into_iter()
            .filter_map(|(key, value)| match value {
                Dynamic::String(s) => Some((key.to_string_lossy(), s.to_string_lossy())),
                _ => None,
            })
            .collect(),
    )
}

fn pairs_to_dynamic(pairs: &[(String, String)]) -> Dynamic {
    Dynamic::object(pairs.iter().map(|(k, v)| (JsString::from(k.as_str()), Dynamic::string(v))).collect::<Vec<_>>())
}

impl ProfilesStore for StateProfilesStore {
    fn get_stored_profiles(&self, profiles_home: &URI, ext_uri: &ExtUri) -> Vec<StoredUserDataProfile> {
        let stored = self.state_service.get_item(PROFILES_KEY, Dynamic::array([]));
        let Dynamic::Array(items) = stored else { return Vec::new() };
        items
            .to_vec()
            .iter()
            .map(|p| {
                let location = match p.get_str("location") {
                    Dynamic::String(relative) => Some(ext_uri.join_path(profiles_home, &[relative.to_string_lossy().as_str()])),
                    other @ Dynamic::Object(_) => {
                        let part = |name: &str| match other.get_str(name) {
                            Dynamic::String(s) => Some(s),
                            _ => None,
                        };
                        Some(URI::from(
                            &je_base::uri::UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") },
                            false,
                        ))
                    }
                    _ => None,
                };
                StoredUserDataProfile {
                    name: match p.get_str("name") {
                        Dynamic::String(s) => Some(s.to_string_lossy()),
                        _ => None,
                    },
                    location,
                    icon: match p.get_str("icon") {
                        Dynamic::String(s) => Some(s.to_string_lossy()),
                        _ => None,
                    },
                    use_default_flags: UseDefaultProfileFlags::from_dynamic(&p.get_str("useDefaultFlags")),
                }
            })
            .collect()
    }

    fn save_stored_profiles(&self, stored_profiles: Vec<StoredUserDataProfile>, profiles_home: &URI, ext_uri: &ExtUri) {
        if stored_profiles.is_empty() {
            self.state_service.remove_item(PROFILES_KEY);
            return;
        }
        let items: Vec<Dynamic> = stored_profiles
            .iter()
            .map(|profile| {
                let relative = profile.location.as_ref().and_then(|l| ext_uri.relative_path(profiles_home, l));
                let mut entries: Vec<(&str, Dynamic)> = vec![("name", profile.name.as_deref().map_or(Dynamic::Undefined, Dynamic::string))];
                if let Some(icon) = &profile.icon {
                    entries.push(("icon", Dynamic::string(icon)));
                }
                if let Some(flags) = profile.use_default_flags {
                    entries.push(("useDefaultFlags", flags.to_dynamic()));
                }
                entries.push(("location", relative.as_deref().map_or(Dynamic::Undefined, Dynamic::string)));
                Dynamic::object(entries)
            })
            .collect();
        self.state_service.set_item(PROFILES_KEY, Dynamic::array(items));
    }

    fn get_stored_profile_associations(&self) -> StoredProfileAssociations {
        let stored = self.state_service.get_item(PROFILE_ASSOCIATIONS_KEY, Dynamic::object(Vec::<(&str, Dynamic)>::new()));
        StoredProfileAssociations { workspaces: string_pairs(&stored.get_str("workspaces")), empty_windows: string_pairs(&stored.get_str("emptyWindows")) }
    }

    fn save_stored_profile_associations(&self, associations: StoredProfileAssociations) {
        if associations.empty_windows.is_some() || associations.workspaces.is_some() {
            let mut entries: Vec<(&str, Dynamic)> = Vec::new();
            if let Some(workspaces) = &associations.workspaces {
                entries.push(("workspaces", pairs_to_dynamic(workspaces)));
            }
            if let Some(empty_windows) = &associations.empty_windows {
                entries.push(("emptyWindows", pairs_to_dynamic(empty_windows)));
            }
            self.state_service.set_item(PROFILE_ASSOCIATIONS_KEY, Dynamic::object(entries));
        } else {
            self.state_service.remove_item(PROFILE_ASSOCIATIONS_KEY);
        }
    }

    fn get_default_profile_extensions_location(&self) -> Option<URI> {
        self.default_extensions_location.clone()
    }
}

/// O que o serviço lê do ambiente: `userRoamingDataHome`, `cacheHome` e `agentSessionsWorkspace`.
#[derive(Clone, Debug)]
pub struct UserDataProfilesEnvironment {
    pub user_roaming_data_home: URI,
    pub cache_home: URI,
    pub agent_sessions_workspace: Option<URI>,
}

/// `UserDataProfilesObject`.
#[derive(Clone, Default)]
struct ProfilesObject {
    profiles: Vec<UserDataProfile>,
    empty_windows: Vec<(String, UserDataProfile)>,
}

impl ProfilesObject {
    fn empty_window(&self, window_id: &str) -> Option<&UserDataProfile> {
        self.empty_windows.iter().find(|(id, _)| id == window_id).map(|(_, p)| p)
    }

    fn set_empty_window(&mut self, window_id: &str, profile: UserDataProfile) {
        match self.empty_windows.iter_mut().find(|(id, _)| id == window_id) {
            Some(entry) => entry.1 = profile,
            None => self.empty_windows.push((window_id.to_string(), profile)),
        }
    }

    fn delete_empty_window(&mut self, window_id: &str) {
        self.empty_windows.retain(|(id, _)| id != window_id);
    }
}

const SYSTEM_PROFILES_HOME: &str = "builtin";

/// O workspace de uma associação: a URI do workspace ou o id da janela vazia.
#[derive(Clone, Debug)]
enum WorkspaceKey {
    Uri(URI),
    Id(String),
}

struct State {
    profiles_object: Option<ProfilesObject>,
    transient: ProfilesObject,
    profile_creation_promises: HashMap<String, Promise<UserDataProfile>>,
}

/// `UserDataProfilesService`.
pub struct UserDataProfilesService {
    me: std::rc::Weak<UserDataProfilesService>,
    base: Rc<Disposable>,
    environment: UserDataProfilesEnvironment,
    file_service: Rc<dyn IFileService>,
    ext_uri: ExtUri,
    log_service: LogServiceRef,
    store: Rc<dyn ProfilesStore>,
    profiles_home: URI,
    profiles_cache_home: URI,
    state: RefCell<State>,
    on_did_change_profiles: Rc<Emitter<DidChangeProfilesEvent>>,
    on_will_create_profile: Rc<Emitter<WillChangeProfileEvent>>,
    on_will_remove_profile: Rc<Emitter<WillChangeProfileEvent>>,
    on_did_reset_workspaces: Rc<Emitter<()>>,
}

impl UserDataProfilesService {
    pub fn new(
        environment: UserDataProfilesEnvironment,
        file_service: Rc<dyn IFileService>,
        ext_uri: ExtUri,
        log_service: LogServiceRef,
        store: Rc<dyn ProfilesStore>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_profiles = Emitter::new();
        let on_will_create_profile = Emitter::new();
        let on_will_remove_profile = Emitter::new();
        let on_did_reset_workspaces = Emitter::new();
        base.register(on_did_change_profiles.clone());
        base.register(on_will_create_profile.clone());
        base.register(on_will_remove_profile.clone());
        base.register(on_did_reset_workspaces.clone());
        let profiles_home = ext_uri.join_path(&environment.user_roaming_data_home, &["profiles"]);
        let profiles_cache_home = ext_uri.join_path(&environment.cache_home, &["CachedProfilesData"]);
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            environment,
            file_service,
            ext_uri,
            log_service,
            store,
            profiles_home,
            profiles_cache_home,
            state: RefCell::new(State { profiles_object: None, transient: ProfilesObject::default(), profile_creation_promises: HashMap::new() }),
            on_did_change_profiles,
            on_will_create_profile,
            on_will_remove_profile,
            on_did_reset_workspaces,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `onWillCreateProfile`.
    pub fn on_will_create_profile(&self) -> Event<WillChangeProfileEvent> {
        self.on_will_create_profile.event()
    }

    /// `onWillRemoveProfile`.
    pub fn on_will_remove_profile(&self) -> Event<WillChangeProfileEvent> {
        self.on_will_remove_profile.event()
    }

    /// `init()`.
    pub fn init(&self) {
        self.state.borrow_mut().profiles_object = None;
    }

    fn profiles_object(&self) -> ProfilesObject {
        if let Some(object) = self.state.borrow().profiles_object.clone() {
            return object;
        }
        let default_profile = self.create_default_profile();
        let mut profiles: Vec<UserDataProfile> = vec![default_profile.clone()];

        let stored = errors::catch(|| self.store.get_stored_profiles(&self.profiles_home, &self.ext_uri));
        match stored {
            Ok(stored_profiles) => {
                for stored_profile in stored_profiles {
                    if Self::is_invalid_profile(&stored_profile) {
                        let which = stored_profile.location.as_ref().map(|l| l.to_string(false).to_string_lossy()).or(stored_profile.name.clone()).unwrap_or_default();
                        self.log_service.warn(&format!("Skipping the invalid stored profile {which}"));
                        continue;
                    }
                    let location = stored_profile.location.clone().expect("validado acima");
                    let id = self.ext_uri.basename(&location, None);
                    profiles.push(to_user_data_profile(
                        &id,
                        stored_profile.name.as_deref().unwrap_or_default(),
                        &location,
                        &self.profiles_cache_home,
                        Some(&UserDataProfileOptions {
                            icon: stored_profile.icon.clone(),
                            use_default_flags: if id == AGENTS_WINDOW_PROFILE_ID { Some(agents_window_profile_flags()) } else { stored_profile.use_default_flags },
                            ..Default::default()
                        }),
                        Some(&default_profile),
                        &self.ext_uri,
                    ));
                }
            }
            Err(error) => self.log_service.error(&error.to_string()),
        }

        let mut empty_windows = ProfilesObject::default();
        if !profiles.is_empty() {
            let associations = errors::catch(|| self.store.get_stored_profile_associations());
            match associations {
                Ok(associations) => {
                    if let Some(workspaces) = associations.workspaces {
                        for (workspace_path, profile_id) in workspaces {
                            let workspace = URI::parse(workspace_path.as_str(), false);
                            if let Some(profile) = profiles.iter_mut().find(|p| p.id == profile_id) {
                                let mut workspaces = profile.workspaces.clone().unwrap_or_default();
                                workspaces.push(workspace);
                                profile.workspaces = Some(workspaces);
                            }
                        }
                    }
                    if let Some(windows) = associations.empty_windows {
                        for (window_id, profile_id) in windows {
                            if let Some(profile) = profiles.iter().find(|p| p.id == profile_id) {
                                empty_windows.set_empty_window(&window_id, profile.clone());
                            }
                        }
                    }
                }
                Err(error) => self.log_service.error(&error.to_string()),
            }
        }

        let object = ProfilesObject { profiles, empty_windows: empty_windows.empty_windows };
        self.state.borrow_mut().profiles_object = Some(object.clone());
        object
    }

    fn is_invalid_profile(stored_profile: &StoredUserDataProfile) -> bool {
        stored_profile.name.as_deref().is_none_or(str::is_empty) || stored_profile.location.is_none()
    }

    fn create_default_profile(&self) -> UserDataProfile {
        let mut default_profile = to_user_data_profile(
            "__default__profile__",
            &localize("defaultProfile", "Default", &[]),
            &self.environment.user_roaming_data_home,
            &self.profiles_cache_home,
            None,
            None,
            &self.ext_uri,
        );
        if let Some(extensions) = self.store.get_default_profile_extensions_location() {
            default_profile.extensions_resource = extensions;
        }
        default_profile.is_default = true;
        default_profile
    }

    fn all_profiles(&self) -> Vec<UserDataProfile> {
        let object = self.profiles_object();
        let mut profiles = object.profiles;
        profiles.extend(self.state.borrow().transient.profiles.clone());
        profiles
    }

    fn do_create_profile(
        &self,
        id: &str,
        name: &str,
        options: Option<UserDataProfileOptions>,
        workspace_identifier: Option<AnyWorkspaceIdentifier>,
    ) -> Promise<UserDataProfile> {
        if name.is_empty() {
            return Promise::rejected(Error::new("Name of the profile is mandatory and must be of type `string`"));
        }
        let existing_promise = self.state.borrow().profile_creation_promises.get(name).cloned();
        if let Some(promise) = existing_promise {
            return promise;
        }

        let this = self.rc();
        let (id, name_owned) = (id.to_string(), name.to_string());
        let promise = async_fn(async move {
            let result = this.create_profile_inner(&id, &name_owned, options, workspace_identifier).await;
            this.state.borrow_mut().profile_creation_promises.remove(&name_owned);
            result
        });
        // O corpo roda de forma síncrona até o primeiro `await`: se já terminou, não há o que guardar.
        if !promise.is_settled() {
            self.state.borrow_mut().profile_creation_promises.insert(name.to_string(), promise.clone());
        }
        promise
    }

    async fn create_profile_inner(
        self: &Rc<Self>,
        id: &str,
        name: &str,
        mut options: Option<UserDataProfileOptions>,
        workspace_identifier: Option<AnyWorkspaceIdentifier>,
    ) -> Result<UserDataProfile, Error> {
        let existing = self.all_profiles().into_iter().find(|p| {
            p.id == id
                || (id != AGENTS_WINDOW_PROFILE_ID
                    && p.is_transient != Some(true)
                    && options.as_ref().and_then(|o| o.transient) != Some(true)
                    && p.name == name)
        });
        if existing.is_some() {
            return Err(Error::new(format!("Profile with {name} name already exists")));
        }

        let workspace = workspace_identifier.as_ref().map(Self::get_workspace);
        if let Some(WorkspaceKey::Uri(uri)) = &workspace {
            let mut with_workspace = options.take().unwrap_or_default();
            with_workspace.workspaces = Some(vec![uri.clone()]);
            options = Some(with_workspace);
        }

        let location = if id == AGENTS_WINDOW_PROFILE_ID {
            self.ext_uri.join_path(&self.profiles_home, &[SYSTEM_PROFILES_HOME, id])
        } else {
            self.ext_uri.join_path(&self.profiles_home, &[id])
        };
        let default_profile = self.default_profile_value();
        let agents_options = UserDataProfileOptions::default();
        let profile = to_user_data_profile(
            id,
            name,
            &location,
            &self.profiles_cache_home,
            if id == AGENTS_WINDOW_PROFILE_ID { Some(&agents_options) } else { options.as_ref() },
            Some(&default_profile),
            &self.ext_uri,
        );

        self.file_service.create_folder(&profile.location).await?;

        let event = WillChangeProfileEvent::new(profile.clone());
        self.on_will_create_profile.fire(event.clone());
        let joiners = event.joiners.borrow().clone();
        promises::settled(joiners).await?;

        if let Some(WorkspaceKey::Id(window_id)) = &workspace {
            self.update_empty_window_association(window_id, Some(&profile), profile.is_transient == Some(true));
        }

        self.update_profiles(vec![profile.clone()], vec![], vec![], false);

        Ok(self.all_profiles().into_iter().find(|p| p.id == profile.id).unwrap_or(profile))
    }

    fn default_profile_value(&self) -> UserDataProfile {
        self.all_profiles().into_iter().next().expect("o perfil padrão sempre existe")
    }

    fn get_workspace(identifier: &AnyWorkspaceIdentifier) -> WorkspaceKey {
        match identifier {
            AnyWorkspaceIdentifier::SingleFolder(folder) => WorkspaceKey::Uri(folder.uri.clone()),
            AnyWorkspaceIdentifier::Workspace(workspace) => WorkspaceKey::Uri(workspace.config_path.clone()),
            AnyWorkspaceIdentifier::Empty(empty) => WorkspaceKey::Id(empty.id.clone()),
        }
    }

    fn is_profile_associated_to_workspace(&self, profile: &UserDataProfile) -> bool {
        if profile.workspaces.as_ref().is_some_and(|w| !w.is_empty()) {
            return true;
        }
        let object = self.profiles_object();
        let same_location = |window_profile: &UserDataProfile| self.ext_uri.is_equal(Some(&window_profile.location), Some(&profile.location), false);
        if object.empty_windows.iter().any(|(_, p)| same_location(p)) {
            return true;
        }
        self.state.borrow().transient.empty_windows.iter().any(|(_, p)| same_location(p))
    }

    fn update_profiles(&self, added: Vec<UserDataProfile>, removed: Vec<UserDataProfile>, updated: Vec<UserDataProfile>, do_not_trigger: bool) {
        // No original os objetos de `added` e `updated` são os mesmos de `allProfiles`, e a lista de workspaces vazia
        // vira `undefined` neles também.
        let normalize = |mut list: Vec<UserDataProfile>| {
            for profile in &mut list {
                if profile.workspaces.as_ref().is_some_and(Vec::is_empty) {
                    profile.workspaces = None;
                }
            }
            list
        };
        let (added, updated) = (normalize(added), normalize(updated));
        let mut all_profiles: Vec<UserDataProfile> = self.all_profiles();
        all_profiles.extend(added.iter().cloned());
        let transient_profiles = std::mem::take(&mut self.state.borrow_mut().transient.profiles);
        // `profilesObject` precisa existir antes de mexer nas associações das janelas vazias.
        let _ = self.profiles_object();

        let mut profiles: Vec<UserDataProfile> = Vec::new();
        for mut profile in all_profiles {
            // removed
            if removed.iter().any(|p| profile.id == p.id) {
                let mut state = self.state.borrow_mut();
                if let Some(object) = state.profiles_object.as_mut() {
                    let ids: Vec<String> = object.empty_windows.iter().filter(|(_, p)| p.id == profile.id).map(|(w, _)| w.clone()).collect();
                    for window_id in ids {
                        object.delete_empty_window(&window_id);
                    }
                }
                continue;
            }

            if !profile.is_default {
                profile = updated.iter().find(|p| profile.id == p.id).cloned().unwrap_or(profile);
                let transient_profile = transient_profiles.iter().find(|p| profile.id == p.id);
                if profile.is_transient == Some(true) {
                    self.state.borrow_mut().transient.profiles.push(profile.clone());
                } else if transient_profile.is_some() {
                    // Move the empty window associations from the transient profile to the persisted profile
                    let mut state = self.state.borrow_mut();
                    if let Some((window_id, _)) = state.transient.empty_windows.iter().find(|(_, p)| profile.id == p.id).cloned() {
                        state.transient.delete_empty_window(&window_id);
                        if let Some(object) = state.profiles_object.as_mut() {
                            object.set_empty_window(&window_id, profile.clone());
                        }
                    }
                }
            }

            if profile.workspaces.as_ref().is_some_and(Vec::is_empty) {
                profile.workspaces = None;
            }
            profiles.push(profile);
        }

        self.update_stored_profiles(&profiles);
        if !do_not_trigger {
            self.trigger_profiles_changes(added, removed, updated);
        }
    }

    fn trigger_profiles_changes(&self, added: Vec<UserDataProfile>, removed: Vec<UserDataProfile>, updated: Vec<UserDataProfile>) {
        self.on_did_change_profiles.fire(DidChangeProfilesEvent { added, removed, updated, all: self.all_profiles() });
    }

    fn update_empty_window_association(&self, window_id: &str, new_profile: Option<&UserDataProfile>, transient: bool) {
        // Force transient if the new profile to associate is transient
        let transient = if new_profile.is_some_and(|p| p.is_transient == Some(true)) { true } else { transient };
        let _ = self.profiles_object();
        let mut state = self.state.borrow_mut();
        if transient {
            match new_profile {
                Some(profile) => state.transient.set_empty_window(window_id, profile.clone()),
                None => state.transient.delete_empty_window(window_id),
            }
        } else {
            // Unset the transient association if any
            state.transient.delete_empty_window(window_id);
            if let Some(object) = state.profiles_object.as_mut() {
                match new_profile {
                    Some(profile) => object.set_empty_window(window_id, profile.clone()),
                    None => object.delete_empty_window(window_id),
                }
            }
        }
    }

    fn update_stored_profiles(&self, profiles: &[UserDataProfile]) {
        let mut stored_profiles: Vec<StoredUserDataProfile> = Vec::new();
        let mut workspaces: Vec<(String, String)> = Vec::new();
        let mut empty_windows: Vec<(String, String)> = Vec::new();

        for profile in profiles {
            if profile.is_transient == Some(true) {
                continue;
            }
            if !profile.is_default {
                stored_profiles.push(StoredUserDataProfile {
                    location: Some(profile.location.clone()),
                    name: Some(profile.name.clone()),
                    icon: profile.icon.clone(),
                    use_default_flags: profile.use_default_flags,
                });
            }
            if let Some(profile_workspaces) = &profile.workspaces {
                for workspace in profile_workspaces {
                    let key = workspace.to_string(false).to_string_lossy();
                    match workspaces.iter_mut().find(|(k, _)| *k == key) {
                        Some(entry) => entry.1 = profile.id.clone(),
                        None => workspaces.push((key, profile.id.clone())),
                    }
                }
            }
        }

        if let Some(object) = self.state.borrow().profiles_object.as_ref() {
            for (window_id, profile) in &object.empty_windows {
                empty_windows.push((window_id.clone(), profile.id.clone()));
            }
        }

        self.store.save_stored_profile_associations(StoredProfileAssociations { workspaces: Some(workspaces), empty_windows: Some(empty_windows) });
        self.store.save_stored_profiles(stored_profiles, &self.profiles_home, &self.ext_uri);
        self.state.borrow_mut().profiles_object = None;
    }

    /// `getProfileForWorkspace`.
    pub fn get_profile_for_workspace(&self, workspace_identifier: &AnyWorkspaceIdentifier) -> Option<UserDataProfile> {
        let workspace = Self::get_workspace(workspace_identifier);
        match workspace {
            WorkspaceKey::Uri(uri) => {
                if self.ext_uri.is_equal(Some(&uri), self.environment.agent_sessions_workspace.as_ref(), false) {
                    return self.all_profiles().into_iter().find(|p| p.is_agents_window_profile == Some(true));
                }
                self.all_profiles().into_iter().find(|p| p.workspaces.as_ref().is_some_and(|ws| ws.iter().any(|w| self.ext_uri.is_equal(Some(w), Some(&uri), false))))
            }
            WorkspaceKey::Id(id) => {
                let object = self.profiles_object();
                object.empty_window(&id).cloned().or_else(|| self.state.borrow().transient.empty_window(&id).cloned())
            }
        }
    }

    /// `unsetWorkspace`.
    pub fn unset_workspace(&self, workspace_identifier: &AnyWorkspaceIdentifier, transient: bool) {
        match Self::get_workspace(workspace_identifier) {
            WorkspaceKey::Uri(uri) => {
                if let Some(currently_associated_profile) = self.get_profile_for_workspace(workspace_identifier) {
                    let remaining: Vec<URI> = currently_associated_profile
                        .workspaces
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|w| !self.ext_uri.is_equal(Some(w), Some(&uri), false))
                        .collect();
                    let _ = self.rc().update_profile_sync(&currently_associated_profile, UserDataProfileUpdateOptions { workspaces: Some(remaining), ..Default::default() });
                }
            }
            WorkspaceKey::Id(id) => {
                self.update_empty_window_association(&id, None, transient);
                self.update_stored_profiles(&self.all_profiles());
            }
        }
    }

    /// O corpo (síncrono) de `updateProfile`.
    fn update_profile_sync(&self, profile: &UserDataProfile, options: UserDataProfileUpdateOptions) -> Result<UserDataProfile, Error> {
        if profile.is_agents_window_profile == Some(true) {
            return Err(Error::new("Cannot update agents window profile"));
        }

        let default_profile = self.default_profile_value();
        let mut profiles_to_update: Vec<UserDataProfile> = Vec::new();
        for existing in self.all_profiles() {
            let mut profile_to_update: Option<UserDataProfile> = None;
            if profile.id == existing.id {
                if !existing.is_default {
                    profile_to_update = Some(to_user_data_profile(
                        &existing.id,
                        options.name.as_deref().unwrap_or(&existing.name),
                        &existing.location,
                        &self.profiles_cache_home,
                        Some(&UserDataProfileOptions {
                            icon: match &options.icon {
                                Some(icon) => icon.clone(),
                                None => existing.icon.clone(),
                            },
                            transient: options.transient.or(existing.is_transient),
                            use_default_flags: options.use_default_flags.or(existing.use_default_flags),
                            workspaces: options.workspaces.clone().or_else(|| existing.workspaces.clone()),
                        }),
                        Some(&default_profile),
                        &self.ext_uri,
                    ));
                } else if let Some(workspaces) = &options.workspaces {
                    let mut updated = existing.clone();
                    updated.workspaces = Some(workspaces.clone());
                    // O perfil padrão é mutado no lugar: grava a mudança no estado.
                    self.set_default_workspaces(Some(workspaces.clone()));
                    profile_to_update = Some(updated);
                }
            } else if let Some(option_workspaces) = &options.workspaces {
                let workspaces: Option<Vec<URI>> = existing
                    .workspaces
                    .as_ref()
                    .map(|ws| ws.iter().filter(|w1| !option_workspaces.iter().any(|w2| self.ext_uri.is_equal(Some(w1), Some(w2), false))).cloned().collect());
                if existing.workspaces.as_ref().map(Vec::len) != workspaces.as_ref().map(Vec::len) {
                    let mut updated = existing.clone();
                    updated.workspaces = workspaces;
                    profile_to_update = Some(updated);
                }
            }
            if let Some(profile_to_update) = profile_to_update {
                profiles_to_update.push(profile_to_update);
            }
        }

        if profiles_to_update.is_empty() {
            if profile.is_default {
                return Err(Error::new("Cannot update default profile"));
            }
            return Err(Error::new(format!("Profile '{}' does not exist", profile.name)));
        }

        self.update_profiles(vec![], vec![], profiles_to_update, false);

        self.all_profiles().into_iter().find(|p| p.id == profile.id).ok_or_else(|| Error::new(format!("Profile '{}' was not updated", profile.name)))
    }

    fn set_default_workspaces(&self, workspaces: Option<Vec<URI>>) {
        let _ = self.profiles_object();
        if let Some(object) = self.state.borrow_mut().profiles_object.as_mut()
            && let Some(default) = object.profiles.first_mut()
        {
            default.workspaces = workspaces;
        }
    }
}

impl IDisposable for UserDataProfilesService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataProfilesService for UserDataProfilesService {
    fn profiles_home(&self) -> URI {
        self.profiles_home.clone()
    }

    fn default_profile(&self) -> UserDataProfile {
        self.default_profile_value()
    }

    fn on_did_change_profiles(&self) -> Event<DidChangeProfilesEvent> {
        self.on_did_change_profiles.event()
    }

    fn profiles(&self) -> Vec<UserDataProfile> {
        self.all_profiles()
    }

    fn on_did_reset_workspaces(&self) -> Event<()> {
        self.on_did_reset_workspaces.event()
    }

    fn create_named_profile(&self, name: &str, options: Option<UserDataProfileOptions>, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        self.create_profile(&hash_to_hex(&generate_uuid()), name, options, workspace_identifier)
    }

    fn create_transient_profile(&self, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        let name_prefix = "Temp";
        let name_reg_ex = RegExp::from_str("Temp\\s(\\d+)", "").expect("expressão fixa");
        let mut name_index: i64 = 0;
        for profile in self.all_profiles() {
            let units: Vec<u16> = profile.name.encode_utf16().collect();
            let index = name_reg_ex
                .exec(&units, 0)
                .and_then(|m| m.captures.get(1).cloned().flatten())
                .and_then(|range| String::from_utf16_lossy(&units[range]).parse::<i64>().ok())
                .unwrap_or(0);
            name_index = name_index.max(index);
        }
        let name = format!("{name_prefix} {}", name_index + 1);
        self.create_profile(&hash_to_hex(&generate_uuid()), &name, Some(UserDataProfileOptions { transient: Some(true), ..Default::default() }), workspace_identifier)
    }

    fn create_profile(&self, id: &str, name: &str, options: Option<UserDataProfileOptions>, workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        self.do_create_profile(id, name, options, workspace_identifier)
    }

    fn update_profile(&self, profile: &UserDataProfile, options: UserDataProfileUpdateOptions) -> Promise<UserDataProfile> {
        match self.update_profile_sync(profile, options) {
            Ok(profile) => Promise::resolved(profile),
            Err(error) => Promise::rejected(error),
        }
    }

    fn remove_profile(&self, profile_to_remove: &UserDataProfile) -> Promise<()> {
        let this = self.rc();
        let profile_to_remove = profile_to_remove.clone();
        async_fn(async move {
            if profile_to_remove.is_default {
                return Err(Error::new("Cannot remove default profile"));
            }
            let Some(profile) = this.all_profiles().into_iter().find(|p| p.id == profile_to_remove.id) else {
                return Err(Error::new(format!("Profile '{}' does not exist", profile_to_remove.name)));
            };

            let event = WillChangeProfileEvent::new(profile.clone());
            this.on_will_remove_profile.fire(event.clone());
            // `Promise.allSettled(joiners)` não rejeita: os erros dos joiners são ignorados.
            let joiners = event.joiners.borrow().clone();
            let _ = Promise::all_settled(joiners).await;

            this.update_profiles(vec![], vec![profile.clone()], vec![], false);

            if let Err(error) = this.file_service.del(&profile.cache_home, Some(PartialFileDeleteOptions { recursive: Some(true), ..Default::default() })).await
                && to_file_operation_result(&error) != FileOperationResult::FileNotFound
            {
                this.log_service.error(&error.to_string());
            }
            Ok(())
        })
    }

    fn set_profile_for_workspace(&self, workspace_identifier: &AnyWorkspaceIdentifier, profile_to_set: &UserDataProfile) -> Promise<()> {
        let Some(profile) = self.all_profiles().into_iter().find(|p| p.id == profile_to_set.id) else {
            return Promise::rejected(Error::new(format!("Profile '{}' does not exist", profile_to_set.name)));
        };
        match Self::get_workspace(workspace_identifier) {
            WorkspaceKey::Uri(workspace) => {
                let mut workspaces = profile.workspaces.clone().unwrap_or_default();
                if !workspaces.iter().any(|w| self.ext_uri.is_equal(Some(w), Some(&workspace), false)) {
                    workspaces.push(workspace);
                    return match self.update_profile_sync(&profile, UserDataProfileUpdateOptions { workspaces: Some(workspaces), ..Default::default() }) {
                        Ok(_) => Promise::resolved(()),
                        Err(error) => Promise::rejected(error),
                    };
                }
            }
            WorkspaceKey::Id(window_id) => {
                self.update_empty_window_association(&window_id, Some(&profile), false);
                self.update_stored_profiles(&self.all_profiles());
            }
        }
        Promise::resolved(())
    }

    fn reset_workspaces(&self) -> Promise<()> {
        {
            let _ = self.profiles_object();
            let mut state = self.state.borrow_mut();
            state.transient.empty_windows.clear();
            if let Some(object) = state.profiles_object.as_mut() {
                object.empty_windows.clear();
                for profile in &mut object.profiles {
                    profile.workspaces = None;
                }
            }
            for profile in &mut state.transient.profiles {
                profile.workspaces = None;
            }
        }
        self.update_profiles(vec![], vec![], self.all_profiles(), false);
        self.on_did_reset_workspaces.fire(());
        Promise::resolved(())
    }

    fn clean_up(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let cleaned = async {
                if this.file_service.exists(&this.profiles_home).await? {
                    let stat = this.file_service.resolve(&this.profiles_home, None).await?;
                    let profiles = this.all_profiles();
                    let removals: Vec<Promise<()>> = stat
                        .children
                        .unwrap_or_default()
                        .iter()
                        .filter(|child| {
                            child.is_directory
                                && child.name != SYSTEM_PROFILES_HOME
                                && profiles.iter().all(|p| !this.ext_uri.is_equal(Some(&p.location), Some(&child.resource), false))
                        })
                        .map(|child| this.file_service.del(&child.resource, Some(PartialFileDeleteOptions { recursive: Some(true), ..Default::default() })))
                        .collect();
                    Promise::all(removals).await?;
                }
                Ok::<(), Error>(())
            }
            .await;
            if let Err(error) = cleaned {
                this.log_service.error(&format!("Error deleting redundant profile folders {error}"));
            }

            let result = errors::catch(|| {
                let existing = this.store.get_stored_profiles(&this.profiles_home, &this.ext_uri);
                let mut valid: Vec<StoredUserDataProfile> = Vec::new();
                for stored_profile in this.store.get_stored_profiles(&this.profiles_home, &this.ext_uri) {
                    if Self::is_invalid_profile(&stored_profile) {
                        this.log_service.warn(&format!("Invalid user data profile found: {}", stored_profile.name.clone().unwrap_or_default()));
                    } else {
                        valid.push(stored_profile);
                    }
                }
                if existing.len() != valid.len() {
                    this.store.save_stored_profiles(valid, &this.profiles_home, &this.ext_uri);
                }
            });
            if let Err(error) = result {
                this.log_service.error(&format!("Error removing invalid stored profiles {error}"));
            }
            Ok(())
        })
    }

    fn clean_up_transient_profiles(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let unassociated: Vec<UserDataProfile> = this.state.borrow().transient.profiles.clone().into_iter().filter(|p| !this.is_profile_associated_to_workspace(p)).collect();
            let removals: Vec<Promise<()>> = unassociated.iter().map(|p| this.remove_profile(p)).collect();
            let _ = Promise::all_settled(removals).await;
            Ok(())
        })
    }
}

/// `hash(value).toString(16)`.
fn hash_to_hex(value: &str) -> String {
    let h = hash(&Dynamic::string(value));
    if h < 0 { format!("-{:x}", i64::from(h).unsigned_abs()) } else { format!("{h:x}") }
}

/// `StateService` guardado como `StateProfilesStore` (açúcar para quem monta o serviço do `node`).
pub fn state_backed_store(state_service: Rc<dyn StateService>, default_extensions_location: Option<URI>) -> Rc<dyn ProfilesStore> {
    Rc::new(StateProfilesStore::new(state_service, default_extensions_location))
}
