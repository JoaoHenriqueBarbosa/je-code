//! Porte de `src/vs/workbench/services/keybinding/test/browser/keybindingEditing.test.ts` (suíte `KeybindingsEditing`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`), e o `setup` ([`setup`]) roda logo depois; o `disposables` do original é o store do guarda.
//!   Os casos `async` rodam em `run_with_faked_timers`, e `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService({ fileService, configurationService, environmentService }, disposables)`.** O
//!   contêiner não entra: o [`setup`] monta os serviços que o `KeybindingsEditingService` lê (o `FileService` com o
//!   provider em memória do esquema `vscode-tests` e o `FileUserDataProvider` do esquema `vscode-userdata`, o
//!   `ModelService` sobre a configuração `files.eol = '\n'`, o `UserDataProfileService`) e cria o serviço com
//!   `Injectable::create`. O `TextModelResolverService` e o `TextFileService` são o [`TestTextEditingServices`] (ver
//!   `support/keybinding_editing_support.rs`).
//! - **`instantiationService.stub(ITextFileService, 'isDirty', true)`** é `TestTextEditingServices::stub_is_dirty`.
//! - **`IUserFriendlyKeybinding`** é o [`Dynamic`] da entrada (objeto com `key`, `command`, `when` e `systemWide`); o
//!   `deepStrictEqual` compara os objetos sem importar a ordem das chaves, como o do Node. `JSON.stringify(keybindings)`
//!   é o `json_stringify` compacto, e `json.parse` é o `parse_str`.
//! - **`aResolvedKeybindingItem`.** Os argumentos nomeados são o [`ItemSpec`]; `OS` é o sistema do processo.
//! - **`ResolvedKeybindingItem`.** Tem um oitavo argumento a mais que o do original (`systemWide`), sempre falso aqui.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, null_log, run_with_faked_timers};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::json_formatter::json_stringify;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{KeyCodeChord, ResolvedKeybinding};
use je_base::network::schemas;
use je_base::uri::{Field, URI, UriChange};
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::js::{from_json, to_json};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_base::platform::OperatingSystem;
use je_platform::resources::{join_path, uri_file};

/// O `OS` de `platform.ts` do sistema que roda o teste.
fn host_os() -> OperatingSystem {
    if cfg!(windows) {
        OperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        OperatingSystem::Macintosh
    } else {
        OperatingSystem::Linux
    }
}
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data::file_user_data_provider::FileUserDataProvider;
use je_platform::user_data_profile::{IUserDataProfilesService, InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};
use je_workbench::services::keybinding::keybinding_editing::{IKeybindingEditingService, KeybindingsEditingService};
use je_workbench::services::textfile::textfiles::ITextFileService;
use je_workbench::services::textmodel_resolver::resolver_service::ITextModelService;
use je_workbench::services::user_data_profile::user_data_profile::IUserDataProfileService;
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;
use support::keybinding_editing_support::TestTextEditingServices;

/// `interface Modifiers`.
#[derive(Clone, Copy, Default)]
struct Modifiers {
    meta_key: bool,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
}

/// `{ keyCode: KeyCode; modifiers?: Modifiers }`.
#[derive(Clone, Copy)]
struct ChordSpec {
    key_code: KeyCode,
    modifiers: Option<Modifiers>,
}

/// `{ keyCode }`.
fn key(key_code: KeyCode) -> ChordSpec {
    ChordSpec { key_code, modifiers: None }
}

/// `{ keyCode, modifiers: { altKey: true } }`.
fn alt_key(key_code: KeyCode) -> ChordSpec {
    ChordSpec { key_code, modifiers: Some(Modifiers { alt_key: true, ..Modifiers::default() }) }
}

/// Os argumentos nomeados de `aResolvedKeybindingItem`.
#[derive(Default)]
struct ItemSpec {
    command: Option<&'static str>,
    when: Option<&'static str>,
    is_default: Option<bool>,
    first_chord: Option<ChordSpec>,
    second_chord: Option<ChordSpec>,
}

/// `aResolvedKeybindingItem({ command, when, isDefault, firstChord, secondChord })`.
fn a_resolved_keybinding_item(spec: ItemSpec) -> Rc<ResolvedKeybindingItem> {
    let a_simple_keybinding = |chord: ChordSpec| {
        let modifiers = chord.modifiers.unwrap_or_default();
        KeyCodeChord::new(modifiers.ctrl_key, modifiers.shift_key, modifiers.alt_key, modifiers.meta_key, chord.key_code)
    };
    let mut chords: Vec<KeyCodeChord> = Vec::new();
    if let Some(first_chord) = spec.first_chord {
        chords.push(a_simple_keybinding(first_chord));
        if let Some(second_chord) = spec.second_chord {
            chords.push(a_simple_keybinding(second_chord));
        }
    }
    let keybinding: Option<Rc<dyn ResolvedKeybinding>> =
        if chords.is_empty() { None } else { Some(Rc::new(USLayoutResolvedKeybinding::from_chords(chords, host_os()))) };
    let command = spec.command.filter(|command| !command.is_empty()).unwrap_or("some command");
    ResolvedKeybindingItem::new(
        keybinding,
        Some(command),
        Dynamic::Null,
        spec.when.filter(|when| !when.is_empty()).and_then(|when| ContextKeyExpr::deserialize(Some(when))),
        spec.is_default.unwrap_or(true),
        None,
        false,
        false,
    )
}

/// `const ROOT = URI.file('tests').with({ scheme: 'vscode-tests' })`.
fn root() -> URI {
    uri_file("tests").with(&UriChange { scheme: Field::from("vscode-tests"), ..UriChange::default() })
}

/// Uma entrada de `keybindings.json` só com propriedades de texto (`{ key, command, when }`), na ordem dada.
fn entry(properties: &[(&str, &str)]) -> Dynamic {
    Dynamic::object(properties.iter().map(|(name, value)| (*name, Dynamic::string(value))))
}

/// `{ key, command, systemWide: true }`.
fn system_wide_entry(key: &str, command: &str) -> Dynamic {
    Dynamic::object([("key", Dynamic::string(key)), ("command", Dynamic::string(command)), ("systemWide", Dynamic::Bool(true))])
}

/// `JSON.stringify(keybindings || [])`.
fn stringify(keybindings: Vec<Dynamic>) -> String {
    json_stringify(&to_json(&Dynamic::array(keybindings)).expect("um array tem forma em JSON"), None)
}

/// O que o `setup` do original monta.
struct Fixture {
    file_service: Rc<dyn IFileService>,
    user_data_profile_service: Rc<UserDataProfileService>,
    services: Rc<TestTextEditingServices>,
    test_object: Rc<KeybindingsEditingService>,
}

async fn setup(disposables: &LeakGuard) -> Fixture {
    let environment_service = TestEnvironmentService::new();

    let log = null_log();
    let file_service = disposables.add_rc(FileService::new(log.clone()));
    let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    disposables.add(file_service.register_provider("vscode-tests", file_system_provider.clone()).unwrap());

    let user_folder = join_path(&root(), &["User"]);
    file_service.create_folder(&user_folder).await.unwrap();

    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    test_configuration_service.set_user_configuration("files", Dynamic::object([("eol", Dynamic::string("\n"))]), None).await.unwrap();

    let file_service_contract: Rc<dyn IFileService> = file_service.clone();
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract,)));
    let user_data_profiles_service = disposables.add_rc(UserDataProfilesService::new(
        UserDataProfilesEnvironment {
            user_roaming_data_home: environment_service.user_roaming_data_home(),
            cache_home: environment_service.cache_home(),
            agent_sessions_workspace: Some(environment_service.agent_sessions_workspace()),
        },
        file_service.clone(),
        uri_identity_service.ext_uri(),
        log.clone(),
        Rc::new(InMemoryProfilesStore::default()),
    ));
    let user_data_profile_service = disposables.add_rc(UserDataProfileService::create(user_data_profiles_service.default_profile(), ()));
    let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service.clone();
    let user_data_provider = disposables.add_rc(FileUserDataProvider::new(
        "vscode-tests",
        file_system_provider.clone(),
        schemas::VSCODE_USER_DATA,
        user_data_profiles_service.clone(),
        uri_identity_contract,
        log,
    ));
    disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, user_data_provider).unwrap());

    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    ));

    let file_service_contract: Rc<dyn IFileService> = file_service;
    let services = TestTextEditingServices::new(file_service_contract.clone(), model_service);
    let text_model_service: Rc<dyn ITextModelService> = services.clone();
    let text_file_service: Rc<dyn ITextFileService> = services.clone();
    let profile_service: Rc<dyn IUserDataProfileService> = user_data_profile_service.clone();
    let test_object = disposables.add_rc(KeybindingsEditingService::create((), (text_model_service, text_file_service, file_service_contract.clone(), profile_service)));

    Fixture { file_service: file_service_contract, user_data_profile_service, services, test_object }
}

impl Fixture {
    /// `userDataProfileService.currentProfile.keybindingsResource`.
    fn keybindings_resource(&self) -> URI {
        self.user_data_profile_service.current_profile().keybindings_resource
    }

    /// `fileService.writeFile(keybindingsResource, VSBuffer.fromString(content))`.
    async fn write_raw(&self, content: &str) {
        self.file_service.write_file(&self.keybindings_resource(), VSBuffer::from_string(content).into(), None).await.unwrap();
    }

    /// `writeToKeybindingsFile(...keybindings)`.
    async fn write_to_keybindings_file(&self, keybindings: Vec<Dynamic>) {
        self.write_raw(&stringify(keybindings)).await;
    }

    /// `getUserKeybindings()`.
    async fn get_user_keybindings(&self) -> Dynamic {
        let content = self.file_service.read_file(&self.keybindings_resource(), None, None).await.unwrap().value.to_string();
        let mut errors = Vec::new();
        from_json(&json::parse_str(&content, &mut errors).expect("o arquivo tem conteúdo JSON"))
    }

    /// `assert.deepStrictEqual(await getUserKeybindings(), expected)`.
    #[track_caller]
    async fn assert_user_keybindings(&self, expected: Vec<Dynamic>) {
        common::assert_deep_strict_equal(&self.get_user_keybindings().await, &Dynamic::array(expected));
    }
}

const PARSE_ERRORS_MESSAGE: &str = "Unable to write to the keybindings configuration file. Please open it to correct errors/warnings in the file and try again.";

#[test]
fn errors_cases_parse_errors() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_raw(",,,,,,,,,,,,,,").await;
        let result = fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }), "alt+c", None).await;
        match result {
            Ok(()) => panic!("Should fail with parse errors"),
            Err(error) => assert_eq!(error.message, PARSE_ERRORS_MESSAGE),
        }
    });
}

#[test]
fn errors_cases_parse_errors_2() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_raw("[{\"key\": }]").await;
        let result = fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }), "alt+c", None).await;
        match result {
            Ok(()) => panic!("Should fail with parse errors"),
            Err(error) => assert_eq!(error.message, PARSE_ERRORS_MESSAGE),
        }
    });
}

#[test]
fn errors_cases_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.services.stub_is_dirty(true);
        let result = fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }), "alt+c", None).await;
        match result {
            Ok(()) => panic!("Should fail with dirty error"),
            Err(error) => assert_eq!(
                error.message,
                "Unable to write because the keybindings configuration file has unsaved changes. Please save it first and then try again."
            ),
        }
    });
}

#[test]
fn errors_cases_did_not_find_an_array() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_raw("{\"key\": \"alt+c\", \"command\": \"hello\"}").await;
        let result = fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }), "alt+c", None).await;
        match result {
            Ok(()) => panic!("Should fail"),
            Err(error) => assert_eq!(
                error.message,
                "Unable to write to the keybindings configuration file. It has an object which is not of type Array. Please open the file to clean up and try again."
            ),
        }
    });
}

#[test]
fn edit_a_default_keybinding_to_an_empty_file() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_raw("").await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "a")]), entry(&[("key", "escape"), ("command", "-a")])];
        fixture
            .test_object
            .edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("a"), ..ItemSpec::default() }), "alt+c", None)
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn edit_a_default_keybinding_to_an_empty_array() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "a")]), entry(&[("key", "escape"), ("command", "-a")])];
        fixture
            .test_object
            .edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("a"), ..ItemSpec::default() }), "alt+c", None)
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn edit_a_default_keybinding_in_an_existing_array() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("command", "b"), ("key", "shift+c")])]).await;
        let expected = vec![
            entry(&[("key", "shift+c"), ("command", "b")]),
            entry(&[("key", "alt+c"), ("command", "a")]),
            entry(&[("key", "escape"), ("command", "-a")]),
        ];
        fixture
            .test_object
            .edit_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("a"), ..ItemSpec::default() }), "alt+c", None)
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn add_another_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "a")])];
        fixture
            .test_object
            .add_keybinding(a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("a"), ..ItemSpec::default() }), "alt+c", None)
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn add_a_new_default_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "a")])];
        fixture.test_object.add_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), ..ItemSpec::default() }), "alt+c", None).await.unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn add_a_new_default_keybinding_using_edit() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "a")])];
        fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), ..ItemSpec::default() }), "alt+c", None).await.unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn edit_an_user_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "escape"), ("command", "b")])]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "b")])];
        fixture
            .test_object
            .edit_keybinding(
                a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("b"), is_default: Some(false), ..ItemSpec::default() }),
                "alt+c",
                None,
            )
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn edit_an_user_keybinding_with_more_than_one_element() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "escape"), ("command", "b")]), entry(&[("key", "alt+shift+g"), ("command", "c")])]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "b")]), entry(&[("key", "alt+shift+g"), ("command", "c")])];
        fixture
            .test_object
            .edit_keybinding(
                a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("b"), is_default: Some(false), ..ItemSpec::default() }),
                "alt+c",
                None,
            )
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn edit_a_user_keybinding_preserves_system_wide_on_other_entries() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "escape"), ("command", "b")]), system_wide_entry("alt+shift+g", "c")]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "b")]), system_wide_entry("alt+shift+g", "c")];
        fixture
            .test_object
            .edit_keybinding(
                a_resolved_keybinding_item(ItemSpec { first_chord: Some(key(KeyCode::Escape)), command: Some("b"), is_default: Some(false), ..ItemSpec::default() }),
                "alt+c",
                None,
            )
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn remove_a_default_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "-a")])];
        fixture
            .test_object
            .remove_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), first_chord: Some(alt_key(KeyCode::KeyC)), ..ItemSpec::default() }))
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn remove_a_default_keybinding_should_not_ad_duplicate_entries() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "-a")])];
        for _ in 0..5 {
            fixture
                .test_object
                .remove_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), first_chord: Some(alt_key(KeyCode::KeyC)), ..ItemSpec::default() }))
                .await
                .unwrap();
        }
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn remove_a_user_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "b")])]).await;
        fixture
            .test_object
            .remove_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("b"), first_chord: Some(alt_key(KeyCode::KeyC)), is_default: Some(false), ..ItemSpec::default() }))
            .await
            .unwrap();
        fixture.assert_user_keybindings(vec![]).await;
    });
}

#[test]
fn reset_an_edited_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "b")])]).await;
        fixture
            .test_object
            .reset_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("b"), first_chord: Some(alt_key(KeyCode::KeyC)), is_default: Some(false), ..ItemSpec::default() }))
            .await
            .unwrap();
        fixture.assert_user_keybindings(vec![]).await;
    });
}

#[test]
fn reset_a_removed_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-b")])]).await;
        fixture.test_object.reset_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("b"), is_default: Some(false), ..ItemSpec::default() })).await.unwrap();
        fixture.assert_user_keybindings(vec![]).await;
    });
}

#[test]
fn reset_multiple_removed_keybindings() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-b")])]).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+shift+c"), ("command", "-b")])]).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "escape"), ("command", "-b")])]).await;
        fixture.test_object.reset_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("b"), is_default: Some(false), ..ItemSpec::default() })).await.unwrap();
        fixture.assert_user_keybindings(vec![]).await;
    });
}

#[test]
fn add_a_new_keybinding_to_unassigned_keybinding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-a")])]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "-a")]), entry(&[("key", "shift+alt+c"), ("command", "a")])];
        fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), is_default: Some(false), ..ItemSpec::default() }), "shift+alt+c", None).await.unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn add_when_expression() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-a")])]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "-a")]), entry(&[("key", "shift+alt+c"), ("command", "a"), ("when", "editorTextFocus")])];
        fixture
            .test_object
            .edit_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), is_default: Some(false), ..ItemSpec::default() }), "shift+alt+c", Some("editorTextFocus"))
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn update_command_and_when_expression() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")])]).await;
        let expected = vec![
            entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")]),
            entry(&[("key", "shift+alt+c"), ("command", "a"), ("when", "editorTextFocus")]),
        ];
        fixture
            .test_object
            .edit_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), is_default: Some(false), ..ItemSpec::default() }), "shift+alt+c", Some("editorTextFocus"))
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn update_when_expression() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture
            .write_to_keybindings_file(vec![
                entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")]),
                entry(&[("key", "shift+alt+c"), ("command", "a"), ("when", "editorTextFocus && !editorReadonly")]),
            ])
            .await;
        let expected = vec![
            entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")]),
            entry(&[("key", "shift+alt+c"), ("command", "a"), ("when", "editorTextFocus")]),
        ];
        fixture
            .test_object
            .edit_keybinding(
                a_resolved_keybinding_item(ItemSpec { command: Some("a"), is_default: Some(false), when: Some("editorTextFocus && !editorReadonly"), ..ItemSpec::default() }),
                "shift+alt+c",
                Some("editorTextFocus"),
            )
            .await
            .unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}

#[test]
fn remove_when_expression() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables).await;
        fixture.write_to_keybindings_file(vec![entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")])]).await;
        let expected = vec![entry(&[("key", "alt+c"), ("command", "-a"), ("when", "editorTextFocus && !editorReadonly")]), entry(&[("key", "shift+alt+c"), ("command", "a")])];
        fixture.test_object.edit_keybinding(a_resolved_keybinding_item(ItemSpec { command: Some("a"), is_default: Some(false), ..ItemSpec::default() }), "shift+alt+c", None).await.unwrap();
        fixture.assert_user_keybindings(expected).await;
    });
}
