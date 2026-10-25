//! Porte de `src/vs/platform/userDataProfile/electron-main/userDataProfile.ts`: o `UserDataProfilesMainService`, o
//! serviço de perfis do processo principal, que acrescenta ao serviço do `node` o perfil da janela de agentes, as
//! janelas vazias associadas a perfis e o `agentPluginsHome` do perfil padrão.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o serviço é o
//! mesmo. O original só repassa chamadas entre processos (o `UserDataProfilesService` de `userDataProfileIpc.ts`, do
//! lado do renderer, chama este pelo canal): aqui quem precisa do serviço o chama direto.
//!
//! Decisões de tradução:
//! - **Herança**: `UserDataProfilesMainService extends UserDataProfilesService` (o do `node`). O Rust não tem herança:
//!   a struct guarda o [`UserDataProfilesService`] e implementa `Deref` para ele, então tudo o que o original herda
//!   (`defaultProfile`, `profiles`, `createNamedProfile`, `getProfileForWorkspace`, `unsetWorkspace`,
//!   `onWillCreateProfile`, `onWillRemoveProfile`, `cleanUpTransientProfiles`...) é chamado nela como no original.
//!   [`UserDataProfilesMainService::as_service`] entrega o `Rc` do serviço base a quem o recebe por valor (por exemplo o
//!   `WorkspacesManagementMainService`). `IUserDataProfilesMainService` é só um `refineServiceDecorator` (a mesma
//!   interface com mais métodos), então não ganha trait.
//! - **`createDefaultProfile` sobrescrito**: o gancho é `UserDataProfilesService::set_agent_plugins_home` (`pub(super)`
//!   no serviço base), chamado no construtor com o `agentPluginsHome`.
//! - **`@IStateService` e `@IUriIdentityService`**: o estado entra como `Rc<dyn StateService>`, e o `IUriIdentityService`
//!   é o [`ExtUri`] entregue ao construtor. O `UserDataProfilesService` do `node` (a camada que grava `userDataProfiles`
//!   e `profileAssociations` no `storage.json`) é o [`StateProfilesStore`](super::StateProfilesStore) montado aqui.
//! - **`@INativeEnvironmentService` e `@IProductService`**: o serviço só lê `userRoamingDataHome`, `cacheHome`,
//!   `agentSessionsWorkspace`, `extensionsPath`, `userHome`, `args['agent-plugins-dir']` e `dataFolderName`; eles
//!   entram como [`UserDataProfilesMainEnvironment`]. `process.env` e `process.platform` são o `Environment` do `je_base`.
//! - **`NativeParsedArgs`**: só `args['agent-plugins-dir']` é lido, então chega como `Option<String>`.

use std::ops::Deref;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::path::{Environment, PathModule};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{URI, UriChange};

use super::{AGENTS_WINDOW_PROFILE_ID, IUserDataProfilesService, UserDataProfile, UserDataProfilesEnvironment, UserDataProfilesService, state_backed_store};
use crate::files::i_file_service::IFileService;
use crate::log::LogServiceRef;
use crate::state::StateService;
use crate::workspace::EmptyWorkspaceIdentifier;

/// O que o `UserDataProfilesMainService` lê do `INativeEnvironmentService`, do `args` e do `IProductService`.
#[derive(Clone, Debug)]
pub struct UserDataProfilesMainEnvironment {
    /// `userRoamingDataHome`, `cacheHome` e `agentSessionsWorkspace`, que o serviço base lê.
    pub profiles: UserDataProfilesEnvironment,
    /// `extensionsPath`: o caminho (de arquivo) da pasta de extensões; o `extensions.json` do perfil padrão fica nela.
    pub extensions_path: String,
    /// `userHome`.
    pub user_home: URI,
    /// `args['agent-plugins-dir']`.
    pub agent_plugins_dir: Option<String>,
    /// `productService.dataFolderName`.
    pub data_folder_name: String,
}

/// `UserDataProfilesMainService`.
pub struct UserDataProfilesMainService {
    service: Rc<UserDataProfilesService>,
}

impl UserDataProfilesMainService {
    pub fn new(
        environment: UserDataProfilesMainEnvironment,
        state_service: Rc<dyn StateService>,
        file_service: Rc<dyn IFileService>,
        ext_uri: ExtUri,
        log_service: LogServiceRef,
        process: Rc<dyn Environment>,
    ) -> Rc<Self> {
        let UserDataProfilesMainEnvironment { profiles, extensions_path, user_home, agent_plugins_dir, data_folder_name } = environment;

        // `getDefaultProfileExtensionsLocation` do `node`:
        // `joinPath(URI.file(extensionsPath).with({ scheme: this.profilesHome.scheme }), 'extensions.json')`.
        let scheme = profiles.user_roaming_data_home.scheme().clone();
        let extensions_folder = URI::file(extensions_path.as_str(), process.platform()).with(&UriChange { scheme: scheme.into(), ..Default::default() });
        let default_extensions_location = ext_uri.join_path(&extensions_folder, &["extensions.json"]);

        let data_folder = ext_uri.join_path(&user_home, &[data_folder_name.as_str()]);
        let agent_plugins_path = get_agent_plugins_path(agent_plugins_dir.as_deref(), &data_folder, &ext_uri, process.as_ref());
        let agent_plugins_home = URI::file(agent_plugins_path.as_str(), process.platform());

        let service = UserDataProfilesService::new(profiles, file_service, ext_uri, log_service, state_backed_store(state_service, Some(default_extensions_location)));
        service.set_agent_plugins_home(agent_plugins_home);
        Rc::new(Self { service })
    }

    /// O serviço base (o `UserDataProfilesService` do `node`), para quem o recebe por valor.
    pub fn as_service(&self) -> Rc<UserDataProfilesService> {
        self.service.clone()
    }

    /// `createAgentsWindowProfile`.
    pub fn create_agents_window_profile(&self) -> Promise<UserDataProfile> {
        if let Some(existing) = self.service.profiles().into_iter().find(|profile| profile.id == AGENTS_WINDOW_PROFILE_ID) {
            return Promise::resolved(existing);
        }

        self.service.create_profile(AGENTS_WINDOW_PROFILE_ID, "Agents", None, None)
    }

    /// `getAssociatedEmptyWindows`.
    pub fn get_associated_empty_windows(&self) -> Vec<EmptyWorkspaceIdentifier> {
        self.service.associated_empty_window_ids().into_iter().map(|id| EmptyWorkspaceIdentifier { id }).collect()
    }
}

impl Deref for UserDataProfilesMainService {
    type Target = UserDataProfilesService;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

impl IDisposable for UserDataProfilesMainService {
    fn dispose(&self) {
        self.service.dispose();
    }
}

/// `getAgentPluginsPath(args, userHome)`: `userHome` é a pasta de dados do produto no home (`~/.vscode`), como o
/// original a passa.
pub fn get_agent_plugins_path(agent_plugins_dir: Option<&str>, user_home: &URI, ext_uri: &ExtUri, process: &dyn Environment) -> String {
    let path = PathModule::native(process);

    if let Some(cli_agent_plugins_dir) = agent_plugins_dir.filter(|dir| !dir.is_empty()) {
        return path.resolve(&[cli_agent_plugins_dir]);
    }

    if let Some(vscode_agent_plugins) = process.env_var("VSCODE_AGENT_PLUGINS").filter(|value| !value.is_empty()) {
        return vscode_agent_plugins;
    }

    if let Some(vscode_portable) = process.env_var("VSCODE_PORTABLE").filter(|value| !value.is_empty()) {
        return path.join(&[vscode_portable.as_str(), "agent-plugins"]);
    }

    ext_uri.join_path(user_home, &["agent-plugins"]).fs_path(process.platform()).to_string_lossy()
}
