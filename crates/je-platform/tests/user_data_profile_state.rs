//! Casos que NÃO são do original: o `UserDataProfilesService` do `node` lê e escreve as chaves `userDataProfiles` e
//! `profileAssociations` do `storage.json` exatamente como o VS Code (`location` relativo à pasta `profiles`,
//! `useDefaultFlags` só com as chaves ligadas, associações por URI de workspace e por id de janela vazia).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::files::async_test;
use je_base::dynamic::Dynamic;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::resources::{host_environment, uri_file};
use je_platform::state::{StateReadService, StateService};
use je_platform::user_data_profile::{
    IUserDataProfilesService, StateProfilesStore, UserDataProfileOptions, UserDataProfilesEnvironment, UserDataProfilesService,
};

#[derive(Default)]
struct FakeState {
    items: RefCell<Vec<(String, Dynamic)>>,
}

impl StateReadService for FakeState {
    fn get_item(&self, key: &str, default_value: Dynamic) -> Dynamic {
        self.items.borrow().iter().find(|(k, _)| k == key).map_or(default_value, |(_, v)| v.clone())
    }
}

impl StateService for FakeState {
    fn set_item(&self, key: &str, data: Dynamic) {
        let mut items = self.items.borrow_mut();
        match items.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = data,
            None => items.push((key.to_string(), data)),
        }
    }
    fn set_items(&self, items: &[(String, Dynamic)]) {
        for (k, v) in items {
            self.set_item(k, v.clone());
        }
    }
    fn remove_item(&self, key: &str) {
        self.items.borrow_mut().retain(|(k, _)| k != key);
    }
    fn close(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

#[test]
fn reads_and_writes_the_vscode_state_format() {
    async_test(|disposables| async move {
        let log_service = NullLogService::shared();
        let file_service = disposables.add_rc(FileService::new(log_service.clone()));
        let provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, provider).unwrap());

        let ext_uri = ExtUri::unbiased(host_environment());
        let home = uri_file("/User").with(&UriChange { scheme: Field::Set(schemas::VSCODE_USER_DATA.into()), ..Default::default() });
        let environment = UserDataProfilesEnvironment { user_roaming_data_home: home.clone(), cache_home: home, agent_sessions_workspace: None };

        // O que o VS Code grava num perfil real.
        let state = Rc::new(FakeState::default());
        state.set_item(
            "userDataProfiles",
            Dynamic::array([Dynamic::object([
                ("name", Dynamic::string("Work")),
                ("icon", Dynamic::string("briefcase")),
                ("useDefaultFlags", Dynamic::object([("settings", Dynamic::Bool(true))])),
                ("location", Dynamic::string("abc123")),
            ])]),
        );
        state.set_item(
            "profileAssociations",
            Dynamic::object([("workspaces", Dynamic::object([("file:///work/project", Dynamic::string("abc123"))])), ("emptyWindows", Dynamic::object(Vec::<(&str, Dynamic)>::new()))]),
        );

        let store = Rc::new(StateProfilesStore::new(state.clone(), None));
        let service = UserDataProfilesService::new(environment, file_service, ext_uri, log_service, store);
        disposables.add_rc(service.clone());

        let profiles = service.profiles();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[1].id, "abc123");
        assert_eq!(profiles[1].name, "Work");
        assert_eq!(profiles[1].icon.as_deref(), Some("briefcase"));
        assert_eq!(profiles[1].use_default_flags.and_then(|f| f.settings), Some(true));
        assert_eq!(profiles[1].settings_resource, profiles[0].settings_resource);
        assert_eq!(profiles[1].workspaces.as_ref().map(|w| w[0].to_string(false).to_string_lossy()), Some("file:///work/project".to_string()));

        // Criar outro perfil regrava as duas chaves no mesmo formato (`location` relativo).
        let created = service.create_profile("fff", "Other", Some(UserDataProfileOptions::default()), None).await.unwrap();
        assert_eq!(created.location, URI::parse("vscode-userdata:/User/profiles/fff", false));
        let stored = state.get_item("userDataProfiles", Dynamic::Undefined);
        let Dynamic::Array(items) = stored else { panic!("esperava a lista de perfis") };
        let items = items.to_vec();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].get_str("location"), Dynamic::string("fff"));
        assert_eq!(items[1].get_str("name"), Dynamic::string("Other"));
        let associations = state.get_item("profileAssociations", Dynamic::Undefined);
        assert_eq!(associations.get_str("workspaces").get_str("file:///work/project"), Dynamic::string("abc123"));
    });
}
