//! Porte de `src/vs/platform/userDataProfile/test/electron-main/userDataProfileMainService.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - O `TestEnvironmentService` (que estende `AbstractNativeEnvironmentService`) é o [`UserDataProfilesMainEnvironment`]:
//!   `userRoamingDataHome` com o esquema `vscode-userdata`, `cacheHome` em `cache`, `extensionsPath` em
//!   `extensions.json`, `userHome` igual à pasta `User`, sem `agent-plugins-dir` (o `args` do original é um objeto
//!   vazio) e o `dataFolderName` do produto padrão. O `stateResource` (`state.json`) é o arquivo do `StateService`.
//! - O `UriIdentityService` do original é o `ExtUri` sem viés (o provider em memória distingue maiúsculas).
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` fica no começo da suíte no original, antes do `setup`: o `async_test`
//!   cria a guarda antes do corpo, que monta o serviço.
//! - O `setup` roda no começo do corpo de cada teste, dentro do `runWithFakedTimers` do `async_test`.
//! - `assert.equal(x, undefined)` é `is_none()`; os `deepStrictEqual` de `useDefaultFlags` comparam o
//!   `UseDefaultProfileFlags` com só as chaves informadas; `.toString()` das URIs é `to_string(false)`.

mod common;

use std::rc::Rc;

use common::files::async_test;
use je_base::lifecycle::DisposableStore;
use je_base::network::schemas;
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::resources::{host_environment, uri_file};
use je_platform::state::{SaveStrategy, StateServiceImpl};
use je_platform::user_data_profile::{
    IUserDataProfilesService, UseDefaultProfileFlags, UserDataProfilesEnvironment, UserDataProfilesMainEnvironment, UserDataProfilesMainService,
};
use je_platform::workspace::{AnyWorkspaceIdentifier, EmptyWorkspaceIdentifier, SingleFolderWorkspaceIdentifier, WorkspaceIdentifier};

struct Setup {
    test_object: Rc<UserDataProfilesMainService>,
    user_roaming_data_home: URI,
}

fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::Set("vscode-tests".into()), ..Default::default() })
}

async fn setup(disposables: &Rc<DisposableStore>) -> Setup {
    let log_service = NullLogService::shared();
    let file_service = disposables.add_rc(FileService::new(log_service.clone()));
    let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, file_system_provider).unwrap());

    let ext_uri = ExtUri::unbiased(host_environment());
    let app_settings_home = ext_uri.join_path(&root(), &["User"]);
    let user_roaming_data_home = app_settings_home.with(&UriChange { scheme: Field::Set(schemas::VSCODE_USER_DATA.into()), ..Default::default() });
    let environment = UserDataProfilesMainEnvironment {
        profiles: UserDataProfilesEnvironment {
            user_roaming_data_home: user_roaming_data_home.clone(),
            cache_home: ext_uri.join_path(&user_roaming_data_home, &["cache"]),
            agent_sessions_workspace: None,
        },
        extensions_path: ext_uri.join_path(&user_roaming_data_home, &["extensions.json"]).path().to_string_lossy(),
        user_home: app_settings_home,
        agent_plugins_dir: None,
        data_folder_name: ProductService::default().data_folder_name,
    };
    let state_service = disposables.add_rc(StateServiceImpl::new(
        SaveStrategy::Delayed,
        ext_uri.join_path(&user_roaming_data_home, &["state.json"]),
        log_service.clone(),
        file_service.clone(),
    ));

    let test_object = UserDataProfilesMainService::new(environment, state_service.clone(), file_service, ext_uri, log_service, host_environment());
    disposables.add_rc(test_object.clone());
    state_service.init().await.unwrap();
    Setup { test_object, user_roaming_data_home }
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

#[test]
fn default_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        assert!(test_object.default_profile().is_default);
    });
}

#[test]
fn profiles_always_include_default_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        assert_eq!(test_object.profiles().len(), 1);
        assert!(test_object.profiles()[0].is_default);
    });
}

#[test]
fn default_profile_when_there_are_profiles() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        test_object.create_named_profile("test", None, None).await.unwrap();
        assert!(test_object.default_profile().is_default);
    });
}

#[test]
fn default_profile_when_profiles_are_removed() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        let profile = test_object.create_named_profile("test", None, None).await.unwrap();
        test_object.remove_profile(&profile).await.unwrap();
        assert!(test_object.default_profile().is_default);
    });
}

#[test]
fn when_no_profile_is_set() {
    async_test(|disposables| async move {
        let Setup { test_object, user_roaming_data_home } = setup(&disposables).await;
        test_object.create_named_profile("profile1", None, None).await.unwrap();

        assert!(test_object.get_profile_for_workspace(&AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: "id".to_string() })).is_none());
        assert!(
            test_object
                .get_profile_for_workspace(&AnyWorkspaceIdentifier::Workspace(WorkspaceIdentifier { id: "id".to_string(), config_path: user_roaming_data_home.clone() }))
                .is_none()
        );
        assert!(
            test_object
                .get_profile_for_workspace(&AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id: "id".to_string(), uri: user_roaming_data_home }))
                .is_none()
        );
    });
}

#[test]
fn set_profile_to_a_workspace() {
    async_test(|disposables| async move {
        let Setup { test_object, user_roaming_data_home } = setup(&disposables).await;
        let workspace = AnyWorkspaceIdentifier::Workspace(WorkspaceIdentifier { id: "id".to_string(), config_path: user_roaming_data_home });
        let profile = test_object.create_named_profile("profile1", None, None).await.unwrap();

        test_object.set_profile_for_workspace(&workspace, &profile).await.unwrap();

        assert_eq!(test_object.get_profile_for_workspace(&workspace).map(|p| p.id), Some(profile.id));
    });
}

#[test]
fn set_profile_to_a_folder() {
    async_test(|disposables| async move {
        let Setup { test_object, user_roaming_data_home } = setup(&disposables).await;
        let workspace = AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id: "id".to_string(), uri: user_roaming_data_home });
        let profile = test_object.create_named_profile("profile1", None, None).await.unwrap();

        test_object.set_profile_for_workspace(&workspace, &profile).await.unwrap();

        assert_eq!(test_object.get_profile_for_workspace(&workspace).map(|p| p.id), Some(profile.id));
    });
}

#[test]
fn set_profile_to_a_window() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        let workspace = AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: "id".to_string() });
        let profile = test_object.create_named_profile("profile1", None, None).await.unwrap();

        test_object.set_profile_for_workspace(&workspace, &profile).await.unwrap();

        assert_eq!(test_object.get_profile_for_workspace(&workspace).map(|p| p.id), Some(profile.id));
    });
}

#[test]
fn creating_agents_window_profile_should_return_profile_with_agent_window_default_flags() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables).await;
        let profile = test_object.create_agents_window_profile().await.unwrap();
        let default_profile = test_object.default_profile();

        assert_eq!(profile.id, "agents");
        assert_eq!(profile.name, "Agents");
        assert!(!profile.is_default);
        assert_eq!(profile.is_agents_window_profile, Some(true));
        assert_eq!(profile.is_internal, Some(true));
        assert_eq!(
            profile.use_default_flags,
            Some(UseDefaultProfileFlags {
                settings: Some(true),
                keybindings: Some(true),
                prompts: Some(true),
                mcp: Some(true),
                language_models: Some(true),
                snippets: Some(true),
                tasks: Some(true),
                extensions: Some(true),
                global_state: None,
            })
        );
        assert_eq!(text(&profile.settings_resource), text(&default_profile.settings_resource));
        assert_eq!(text(&profile.keybindings_resource), text(&default_profile.keybindings_resource));
        assert_eq!(text(&profile.snippets_home), text(&default_profile.snippets_home));
        assert_eq!(text(&profile.tasks_resource), text(&default_profile.tasks_resource));
        assert_eq!(text(&profile.extensions_resource), text(&default_profile.extensions_resource));
        assert_eq!(text(&profile.prompts_home), text(&default_profile.prompts_home));
        assert_eq!(text(&profile.mcp_resource), text(&default_profile.mcp_resource));
        assert_eq!(text(&profile.language_models_resource), text(&default_profile.language_models_resource));
    });
}
