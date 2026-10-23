//! Porte de `src/vs/platform/userDataProfile/test/common/userDataProfileService.test.ts` (suíte `UserDataProfileService (Common)`), caso a caso e na mesma ordem.
//!
//! Divergências: o `TestEnvironmentService` é o `UserDataProfilesEnvironment` (`userRoamingDataHome` com o esquema
//! `vscode-userdata`, `cacheHome` igual); o `UriIdentityService` é o `ExtUri` sem viés (o provider em memória distingue
//! maiúsculas); o `setup` roda no começo do corpo de cada teste, dentro do `runWithFakedTimers`; os
//! `deepStrictEqual` de `useDefaultFlags` comparam o `UseDefaultProfileFlags` com só as chaves informadas.

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
use je_platform::resources::{host_environment, uri_file};
use je_platform::user_data_profile::{
    IUserDataProfilesService, InMemoryProfilesStore, UseDefaultProfileFlags, UserDataProfile, UserDataProfileOptions, UserDataProfileUpdateOptions,
    UserDataProfilesEnvironment, UserDataProfilesService,
};
use je_platform::workspace::{AnyWorkspaceIdentifier, SingleFolderWorkspaceIdentifier};

struct Setup {
    test_object: Rc<UserDataProfilesService>,
    environment: UserDataProfilesEnvironment,
    ext_uri: ExtUri,
}

fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::Set("vscode-tests".into()), ..Default::default() })
}

fn setup(disposables: &Rc<DisposableStore>) -> Setup {
    let log_service = NullLogService::shared();
    let file_service = disposables.add_rc(FileService::new(log_service.clone()));
    let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    let root = root();
    disposables.add(file_service.register_provider(root.scheme().to_string_lossy().as_str(), file_system_provider.clone()).unwrap());
    disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, file_system_provider).unwrap());

    let ext_uri = ExtUri::unbiased(host_environment());
    let user_roaming_data_home = ext_uri.join_path(&root, &["User"]).with(&UriChange { scheme: Field::Set(schemas::VSCODE_USER_DATA.into()), ..Default::default() });
    let environment = UserDataProfilesEnvironment { user_roaming_data_home: user_roaming_data_home.clone(), cache_home: user_roaming_data_home, agent_sessions_workspace: None };
    let test_object = UserDataProfilesService::new(environment.clone(), file_service, ext_uri.clone(), log_service, Rc::new(InMemoryProfilesStore::default()));
    disposables.add_rc(test_object.clone());
    Setup { test_object, environment, ext_uri }
}

fn single_folder(workspace: &URI) -> AnyWorkspaceIdentifier {
    AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id: workspace.path().to_string_lossy(), uri: workspace.clone() })
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn flags(configure: impl FnOnce(&mut UseDefaultProfileFlags)) -> UseDefaultProfileFlags {
    let mut flags = UseDefaultProfileFlags::default();
    configure(&mut flags);
    flags
}

fn named_with_flags(test_object: &UserDataProfilesService, use_default_flags: UseDefaultProfileFlags) -> je_base::promise::Promise<UserDataProfile> {
    test_object.create_named_profile("name", Some(UserDataProfileOptions { use_default_flags: Some(use_default_flags), ..Default::default() }), None)
}

#[test]
fn default_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, environment, ext_uri } = setup(&disposables);
        let home = environment.user_roaming_data_home.clone();
        let default_profile = test_object.default_profile();
        assert!(default_profile.is_default);
        assert_eq!(default_profile.use_default_flags, None);
        assert_eq!(text(&default_profile.location), text(&home));
        assert_eq!(text(&default_profile.global_storage_home), text(&ext_uri.join_path(&home, &["globalStorage"])));
        assert_eq!(text(&default_profile.keybindings_resource), text(&ext_uri.join_path(&home, &["keybindings.json"])));
        assert_eq!(text(&default_profile.settings_resource), text(&ext_uri.join_path(&home, &["settings.json"])));
        assert_eq!(text(&default_profile.snippets_home), text(&ext_uri.join_path(&home, &["snippets"])));
        assert_eq!(text(&default_profile.tasks_resource), text(&ext_uri.join_path(&home, &["tasks.json"])));
        assert_eq!(text(&default_profile.extensions_resource), text(&ext_uri.join_path(&home, &["extensions.json"])));
    });
}

#[test]
fn profiles_always_include_default_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        assert_eq!(test_object.profiles().len(), 1);
        assert!(test_object.profiles()[0].is_default);
    });
}

#[test]
fn create_profile_with_id() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_profile("id", "name", None, None).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(profile.id, "id");
        assert_eq!(profile.name, "name");
        assert!(profile.is_transient != Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
        assert_eq!(test_object.profiles()[1].name, profile.name);
    });
}

#[test]
fn create_profile_with_id_name_and_transient() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_profile("id", "name", Some(UserDataProfileOptions { transient: Some(true), ..Default::default() }), None).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(profile.id, "id");
        assert_eq!(profile.name, "name");
        assert_eq!(profile.is_transient, Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
    });
}

#[test]
fn create_transient_profiles() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile1 = test_object.create_transient_profile(None).await.unwrap();
        let profile2 = test_object.create_transient_profile(None).await.unwrap();
        let profile3 = test_object.create_transient_profile(None).await.unwrap();
        let profile4 = test_object.create_profile("id", "name", Some(UserDataProfileOptions { transient: Some(true), ..Default::default() }), None).await.unwrap();
        assert_eq!(test_object.profiles().len(), 5);

        assert_eq!(profile1.name, "Temp 1");
        assert_eq!(profile1.is_transient, Some(true));
        assert_eq!(test_object.profiles()[1].id, profile1.id);

        assert_eq!(profile2.name, "Temp 2");
        assert_eq!(profile2.is_transient, Some(true));
        assert_eq!(test_object.profiles()[2].id, profile2.id);

        assert_eq!(profile3.name, "Temp 3");
        assert_eq!(profile3.is_transient, Some(true));
        assert_eq!(test_object.profiles()[3].id, profile3.id);

        assert_eq!(profile4.name, "name");
        assert_eq!(profile4.is_transient, Some(true));
        assert_eq!(test_object.profiles()[4].id, profile4.id);
    });
}

#[test]
fn create_transient_profile_when_a_normal_profile_with_temp_is_already_created() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        test_object.create_named_profile("Temp 1", None, None).await.unwrap();
        let profile1 = test_object.create_transient_profile(None).await.unwrap();
        assert_eq!(profile1.name, "Temp 2");
        assert_eq!(profile1.is_transient, Some(true));
    });
}

#[test]
fn profiles_include_default_profile_with_extension_resource_defined_when_transiet_prrofile_is_created() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        test_object.create_transient_profile(None).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert!(test_object.profiles()[0].is_default);
    });
}

#[test]
fn profiles_include_default_profile_with_extension_resource_undefined_when_transiet_prrofile_is_removed() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_transient_profile(None).await.unwrap();
        test_object.remove_profile(&profile).await.unwrap();
        assert_eq!(test_object.profiles().len(), 1);
        assert!(test_object.profiles()[0].is_default);
    });
}

#[test]
fn update_named_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_named_profile("name", None, None).await.unwrap();
        test_object.update_profile(&profile, UserDataProfileUpdateOptions { name: Some("name changed".into()), ..Default::default() }).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(test_object.profiles()[1].name, "name changed");
        assert!(test_object.profiles()[1].is_transient != Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
    });
}

#[test]
fn persist_transient_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_transient_profile(None).await.unwrap();
        test_object.update_profile(&profile, UserDataProfileUpdateOptions { name: Some("saved".into()), transient: Some(false), ..Default::default() }).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(test_object.profiles()[1].name, "saved");
        assert!(test_object.profiles()[1].is_transient != Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
    });
}

#[test]
fn persist_transient_profile_2() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_profile("id", "name", Some(UserDataProfileOptions { transient: Some(true), ..Default::default() }), None).await.unwrap();
        test_object.update_profile(&profile, UserDataProfileUpdateOptions { name: Some("saved".into()), transient: Some(false), ..Default::default() }).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(test_object.profiles()[1].name, "saved");
        assert!(test_object.profiles()[1].is_transient != Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
    });
}

#[test]
fn save_transient_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_transient_profile(None).await.unwrap();
        test_object.update_profile(&profile, UserDataProfileUpdateOptions { name: Some("saved".into()), ..Default::default() }).await.unwrap();
        assert_eq!(test_object.profiles().len(), 2);
        assert_eq!(test_object.profiles()[1].name, "saved");
        assert_eq!(test_object.profiles()[1].is_transient, Some(true));
        assert_eq!(test_object.profiles()[1].id, profile.id);
    });
}

macro_rules! use_default_flag_test {
    ($name:ident, $flag:ident, $resource:ident) => {
        #[test]
        fn $name() {
            async_test(|disposables| async move {
                let Setup { test_object, .. } = setup(&disposables);
                let profile = named_with_flags(&test_object, flags(|f| f.$flag = Some(true))).await.unwrap();
                assert!(!profile.is_default);
                assert_eq!(profile.use_default_flags, Some(flags(|f| f.$flag = Some(true))));
                assert_eq!(text(&profile.$resource), text(&test_object.default_profile().$resource));
            });
        }
    };
}

use_default_flag_test!(profile_using_default_profile_for_settings, settings, settings_resource);
use_default_flag_test!(profile_using_default_profile_for_keybindings, keybindings, keybindings_resource);
use_default_flag_test!(profile_using_default_profile_for_snippets, snippets, snippets_home);
use_default_flag_test!(profile_using_default_profile_for_tasks, tasks, tasks_resource);
use_default_flag_test!(profile_using_default_profile_for_global_state, global_state, global_storage_home);
use_default_flag_test!(profile_using_default_profile_for_extensions, extensions, extensions_resource);
use_default_flag_test!(profile_using_default_profile_for_language_models, language_models, language_models_resource);

#[test]
fn update_profile_using_default_profile_for_keybindings() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let mut profile = test_object.create_named_profile("name", None, None).await.unwrap();
        profile = test_object
            .update_profile(&profile, UserDataProfileUpdateOptions { use_default_flags: Some(flags(|f| f.keybindings = Some(true))), ..Default::default() })
            .await
            .unwrap();
        assert!(!profile.is_default);
        assert_eq!(profile.use_default_flags, Some(flags(|f| f.keybindings = Some(true))));
        assert_eq!(text(&profile.keybindings_resource), text(&test_object.default_profile().keybindings_resource));
    });
}

#[test]
fn create_profile_with_a_workspace_associates_it_to_the_profile() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let workspace = uri_file("/workspace1");
        let profile = test_object.create_profile("id", "name", Some(UserDataProfileOptions::default()), Some(single_folder(&workspace))).await.unwrap();
        assert_eq!(profile.workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&profile.workspaces.unwrap()[0]), text(&workspace));
    });
}

#[test]
fn associate_workspace_to_a_profile_should_update_workspaces() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let profile = test_object.create_profile("id", "name", Some(UserDataProfileOptions::default()), None).await.unwrap();
        let workspace = uri_file("/workspace1");
        let promise = test_object.on_did_change_profiles().to_promise(None);
        test_object.set_profile_for_workspace(&single_folder(&workspace), &profile).await.unwrap();
        let actual = promise.await.unwrap();
        assert_eq!(actual.added.len(), 0);
        assert_eq!(actual.removed.len(), 0);
        assert_eq!(actual.updated.len(), 1);
        assert_eq!(actual.updated[0].id, profile.id);
        assert_eq!(actual.updated[0].workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&actual.updated[0].workspaces.as_ref().unwrap()[0]), text(&workspace));
    });
}

#[test]
fn associate_same_workspace_to_a_profile_should_not_duplicate() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let workspace = uri_file("/workspace1");
        let profile = test_object
            .create_profile("id", "name", Some(UserDataProfileOptions { workspaces: Some(vec![workspace.clone()]), ..Default::default() }), None)
            .await
            .unwrap();
        test_object.set_profile_for_workspace(&single_folder(&workspace), &profile).await.unwrap();
        assert_eq!(test_object.profiles()[1].workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&test_object.profiles()[1].workspaces.as_ref().unwrap()[0]), text(&workspace));
    });
}

#[test]
fn associate_workspace_to_another_profile_should_update_workspaces() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let workspace = uri_file("/workspace1");
        let profile1 = test_object.create_profile("id", "name", Some(UserDataProfileOptions::default()), Some(single_folder(&workspace))).await.unwrap();
        let profile2 = test_object.create_profile("id1", "name1", None, None).await.unwrap();
        let promise = test_object.on_did_change_profiles().to_promise(None);
        test_object.set_profile_for_workspace(&single_folder(&workspace), &profile2).await.unwrap();
        let actual = promise.await.unwrap();
        assert_eq!(actual.added.len(), 0);
        assert_eq!(actual.removed.len(), 0);
        assert_eq!(actual.updated.len(), 2);
        assert_eq!(actual.updated[0].id, profile1.id);
        assert_eq!(actual.updated[0].workspaces, None);
        assert_eq!(actual.updated[1].id, profile2.id);
        assert_eq!(actual.updated[1].workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&actual.updated[1].workspaces.as_ref().unwrap()[0]), text(&workspace));
    });
}

#[test]
fn unassociate_workspace_to_a_profile_should_update_workspaces() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let workspace = uri_file("/workspace1");
        let profile = test_object.create_profile("id", "name", Some(UserDataProfileOptions::default()), Some(single_folder(&workspace))).await.unwrap();
        let promise = test_object.on_did_change_profiles().to_promise(None);
        test_object.unset_workspace(&single_folder(&workspace), false);
        let actual = promise.await.unwrap();
        assert_eq!(actual.added.len(), 0);
        assert_eq!(actual.removed.len(), 0);
        assert_eq!(actual.updated.len(), 1);
        assert_eq!(actual.updated[0].id, profile.id);
        assert_eq!(actual.updated[0].workspaces, None);
    });
}

#[test]
fn update_profile_workspaces_add_workspace() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let mut profile = test_object.create_named_profile("name", None, None).await.unwrap();
        let workspace = uri_file("/workspace1");
        profile = test_object.update_profile(&profile, UserDataProfileUpdateOptions { workspaces: Some(vec![workspace.clone()]), ..Default::default() }).await.unwrap();
        assert_eq!(profile.workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&profile.workspaces.unwrap()[0]), text(&workspace));
    });
}

#[test]
fn update_profile_workspaces_remove_workspace() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let mut profile = test_object.create_named_profile("name", None, None).await.unwrap();
        let workspace = uri_file("/workspace1");
        profile = test_object.update_profile(&profile, UserDataProfileUpdateOptions { workspaces: Some(vec![workspace]), ..Default::default() }).await.unwrap();
        profile = test_object.update_profile(&profile, UserDataProfileUpdateOptions { workspaces: Some(vec![]), ..Default::default() }).await.unwrap();
        assert_eq!(profile.workspaces, None);
    });
}

#[test]
fn update_profile_workspaces_replace_workspace() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let mut profile = test_object.create_named_profile("name", None, None).await.unwrap();
        profile = test_object.update_profile(&profile, UserDataProfileUpdateOptions { workspaces: Some(vec![uri_file("/workspace1")]), ..Default::default() }).await.unwrap();
        let workspace = uri_file("/workspace2");
        profile = test_object.update_profile(&profile, UserDataProfileUpdateOptions { workspaces: Some(vec![workspace.clone()]), ..Default::default() }).await.unwrap();
        assert_eq!(profile.workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&profile.workspaces.unwrap()[0]), text(&workspace));
    });
}

#[test]
fn update_default_profile_workspaces_add_workspace() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let workspace = uri_file("/workspace1");
        test_object.update_profile(&test_object.default_profile(), UserDataProfileUpdateOptions { workspaces: Some(vec![workspace.clone()]), ..Default::default() }).await.unwrap();
        assert_eq!(test_object.profiles().len(), 1);
        assert_eq!(test_object.profiles()[0], test_object.default_profile());
        assert!(test_object.default_profile().is_default);
        assert_eq!(test_object.default_profile().workspaces.as_ref().map(Vec::len), Some(1));
        assert_eq!(text(&test_object.default_profile().workspaces.unwrap()[0]), text(&workspace));
    });
}

#[test]
fn can_create_transient_and_persistent_profiles_with_same_name() {
    async_test(|disposables| async move {
        let Setup { test_object, .. } = setup(&disposables);
        let transient = || Some(UserDataProfileOptions { transient: Some(true), ..Default::default() });
        let profile1 = test_object.create_named_profile("name", transient(), None).await.unwrap();
        let profile2 = test_object.create_named_profile("name", transient(), None).await.unwrap();
        let profile3 = test_object.create_named_profile("name", None, None).await.unwrap();
        assert_eq!(profile1.name, "name");
        assert_eq!(profile1.is_transient, Some(true));
        assert_eq!(profile2.name, "name");
        assert_eq!(profile2.is_transient, Some(true));
        assert_eq!(profile3.name, "name");
        assert!(profile3.is_transient != Some(true));
        assert_eq!(test_object.profiles().len(), 4);
        assert_eq!(test_object.profiles()[1].id, profile3.id);
        assert_eq!(test_object.profiles()[2].id, profile1.id);
        assert_eq!(test_object.profiles()[3].id, profile2.id);
    });
}
