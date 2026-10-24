//! Porte de `src/vs/workbench/services/configuration/test/browser/configurationEditing.test.ts` (suíte `ConfigurationEditing`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`suiteSetup`.** O registro do `Registry` é por thread (cada caso roda na sua), então cada caso registra a
//!   configuração `_test` depois do [`RegistryGuard`] (que restaura o registro ao fim do caso).
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`, `setup` e `teardown`.** O guarda de vazamentos é criado antes do
//!   [`Fixture`] (o `setup`); o `Drop` do [`Env`] descarta o `disposables` do original antes de o guarda conferir. Todo
//!   caso roda dentro de `run_with_faked_timers` (os casos `async` do original usam `runWithFakedTimers` ou só esperam
//!   promessas, que o relógio virtual não altera).
//! - **`workbenchInstantiationService`, `TestTextFileService`, `TextModelResolverService`, `INotificationService`.** São o
//!   [`TestConfigurationEditingServices`] (um recorte do serviço de arquivo de texto e do resolvedor de modelos fiel ao
//!   que o teste observa), embrulhado no [`StubbedServices`], que faz o papel do `sinon`: `stub(ITextFileService,
//!   'isDirty', true)` é [`StubbedServices::dirty`] e o `prompt` do `INotificationService` é o contador `prompts`.
//! - **`new ConfigurationEditing(null, ...)`.** É o `ConfigurationEditing::new(None, ...)` sobre os mesmos serviços do
//!   `WorkspaceService`; o `KeybindingsEditingService` e o `CommandService` do original não são lidos por nenhum caso.
//! - **`RemoteAgentService`.** Sem `remoteAuthority` o serviço real não resolve ambiente nenhum: é o
//!   [`TestRemoteAgentService`] (`getEnvironment` devolve `null`).
//! - **`JSON.stringify(x, null, '\t')`** é o `json_stringify` com o recuo `"\t"`.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use je_base::lifecycle::IDisposable;
use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use common::{RegistryGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, j, jstr, node, null_log, prop, run_with_faked_timers, s, strs};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_base::uuid::generate_uuid;
use je_instantiation::Injectable;
use je_platform::configuration::{ConfigurationUpdateOptions, ConfigurationUpdateOverrides};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::js::{from_json, object_keys, to_json};
use je_platform::policy::FilePolicyService;
use je_platform::resources::join_path;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data::file_user_data_provider::FileUserDataProvider;
use je_platform::user_data_profile::{IUserDataProfilesService, InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};
use je_platform::configuration::registry::{PropertySchema, configuration_registry};
use je_platform::workspace::{AnyWorkspaceIdentifier, EmptyWorkspaceIdentifier, IWorkspaceContextService, SingleFolderWorkspaceIdentifier};
use je_platform::workspaces::workspace_identifier::get_single_folder_workspace_identifier;
use je_workbench::services::configuration::browser::configuration_service::{WorkspaceService, register_workspace_service_configuration};
use je_workbench::services::configuration::common::configuration::{
    FOLDER_SETTINGS_PATH, IConfigurationCache, IWorkbenchConfigurationService, ConfigurationKey, USER_STANDALONE_CONFIGURATIONS, WORKSPACE_STANDALONE_CONFIGURATIONS,
};
use je_workbench::services::configuration::common::configuration_editing::{
    ConfigurationEditing, ConfigurationEditingErrorCode, ConfigurationEditingModel, ConfigurationEditingOptions, ConfigurationEditingServices, ConfigurationEditingValue,
    EditableConfigurationTarget, PromptAction, configuration_editing_error_code,
};
use je_workbench::services::user_data_profile::user_data_profile::IUserDataProfileService;
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;
use je_workbench::test::browser::workbench_test_services::{TestConfigurationEditingServices, TestEnvironmentService, TestRemoteAgentService};

const ROOT_SCHEME: &str = "vscode-tests";

/// `const ROOT = URI.file('tests').with({ scheme: 'vscode-tests' })`.
fn root() -> URI {
    common::uri_file("tests").with(&UriChange { scheme: Field::from(ROOT_SCHEME), ..UriChange::default() })
}

/// `class ConfigurationCache implements IConfigurationCache` do arquivo de teste (não guarda nada).
struct ConfigurationCache;

impl IConfigurationCache for ConfigurationCache {
    fn needs_caching(&self, _resource: &URI) -> bool {
        false
    }

    fn read(&self, _key: &ConfigurationKey) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn write(&self, _key: &ConfigurationKey, _content: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn remove(&self, _key: &ConfigurationKey) -> Promise<()> {
        Promise::resolved(())
    }
}

/// Os `sinon.stub` do teste sobre o [`TestConfigurationEditingServices`]: `isDirty` forçado e o contador do `prompt`.
struct StubbedServices {
    inner: Rc<TestConfigurationEditingServices>,
    /// `instantiationService.stub(ITextFileService, 'isDirty', true)`.
    dirty: Cell<bool>,
    /// As chamadas do `INotificationService.prompt` (o `sinon.stub()` do caso `do not notify error`).
    prompts: Cell<usize>,
}

impl ConfigurationEditingServices for StubbedServices {
    fn create_model_reference(&self, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>> {
        self.inner.create_model_reference(resource)
    }

    fn is_dirty(&self, resource: &URI) -> bool {
        self.dirty.get() || self.inner.is_dirty(resource)
    }

    fn save(&self, resource: &URI) -> Promise<()> {
        self.inner.save(resource)
    }

    fn revert(&self, resource: &URI) -> Promise<()> {
        self.inner.revert(resource)
    }

    fn write(&self, resource: &URI, content: &str) -> Promise<()> {
        self.inner.write(resource, content)
    }

    fn notify_error(&self, message: &str) {
        self.inner.notify_error(message);
    }

    fn prompt_error(&self, message: &str, actions: Vec<PromptAction>) {
        self.prompts.set(self.prompts.get() + 1);
        self.inner.prompt_error(message, actions);
    }

    fn open_user_settings(&self) {
        self.inner.open_user_settings();
    }

    fn open_remote_settings(&self) {
        self.inner.open_remote_settings();
    }

    fn open_workspace_settings(&self) {
        self.inner.open_workspace_settings();
    }

    fn open_folder_settings(&self, folder_uri: &URI) {
        self.inner.open_folder_settings(folder_uri);
    }

    fn open_file(&self, resource: &URI) {
        self.inner.open_file(resource);
    }

    fn enable_auto_save_after_short_delay(&self, resource: &URI) -> DisposableRef {
        self.inner.enable_auto_save_after_short_delay(resource)
    }
}

/// `{ type: 'string', default: 'isSet' }`.
fn plain() -> PropertySchema {
    prop("{ 'type': 'string', 'default': 'isSet' }")
}

/// `{ type: 'string', default: 'isSet', policy: { name, category: PolicyCategory.Extensions, minimumVersion, localization } }`.
fn policy_property(name: &str) -> PropertySchema {
    prop(&format!(
        "{{ 'type': 'string', 'default': 'isSet', 'policy': {{ 'name': '{name}', 'category': 'Extensions', 'minimumVersion': '1.0.0', 'localization': {{ 'description': {{ 'key': '', 'value': '' }} }} }} }}"
    ))
}

/// O `setup` da suíte e o que os casos usam dele.
struct Fixture {
    file_service: Rc<dyn IFileService>,
    user_data_profile_service: Rc<UserDataProfileService>,
    environment_service: Rc<TestEnvironmentService>,
    workspace_service: Rc<WorkspaceService>,
    services: Rc<StubbedServices>,
    test_object: Rc<ConfigurationEditing>,
    policy_file: URI,
    /// `disposables`: descartado no `Drop` (o `teardown`), depois do restante do `Fixture`.
    disposables: Rc<DisposableStore>,
    /// Restaura o registro de configuração depois de o `disposables` ser descartado (o campo cai depois do `Drop`).
    _registry: RegistryGuard,
}

impl Drop for Fixture {
    /// O `teardown`: `disposables.clear()`.
    fn drop(&mut self) {
        self.disposables.dispose();
    }
}

impl Fixture {
    async fn new() -> Fixture {
        // O módulo `configurationService.ts` registra `workbench.settings.applyToAllProfiles` quando é carregado.
        register_workspace_service_configuration();
        let registry = RegistryGuard::new();

        // `suiteSetup`
        configuration_registry().register_configuration(node(
            "_test",
            vec![
                ("configurationEditing.service.testSetting", plain()),
                ("configurationEditing.service.testSettingTwo", plain()),
                ("configurationEditing.service.testSettingThree", plain()),
                ("configurationEditing.service.policySetting", policy_property("configurationEditing.service.policySetting")),
            ],
        ));

        let disposables = DisposableStore::new();
        let log = null_log();
        let file_service = disposables.add_rc(FileService::new(log.clone()));
        let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        disposables.add(file_service.register_provider(ROOT_SCHEME, file_system_provider.clone()).unwrap());

        let workspace_folder = join_path(&root(), &[&generate_uuid()]);
        file_service.create_folder(&workspace_folder).await.unwrap();

        let environment_service = TestEnvironmentService::new();
        let policy_file = join_path(&workspace_folder, &["policies.json"]);
        environment_service.set_policy_file(Some(policy_file.clone()));
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
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
        let user_data_provider = disposables.add_rc(FileUserDataProvider::new(
            ROOT_SCHEME,
            file_system_provider.clone(),
            schemas::VSCODE_USER_DATA,
            user_data_profiles_service.clone(),
            uri_identity_service.clone(),
            log.clone(),
        ));
        disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, user_data_provider).unwrap());

        let policy_service = disposables.add_rc(FilePolicyService::new(policy_file.clone(), file_service.clone(), log.clone()));
        let workspace_service = disposables.add_rc(WorkspaceService::new(
            None,
            Rc::new(ConfigurationCache),
            None,
            user_data_profile_service.clone(),
            user_data_profiles_service.clone(),
            file_service.clone(),
            Rc::new(TestRemoteAgentService),
            uri_identity_service.clone(),
            null_log(),
            policy_service,
        ));
        workspace_service
            .initialize(&AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier {
                id: get_single_folder_workspace_identifier(&workspace_folder).id,
                uri: workspace_folder.clone(),
            }))
            .await
            .unwrap();
        workspace_service.initialize(&AnyWorkspaceIdentifier::SingleFolder(get_single_folder_workspace_identifier(&workspace_folder))).await.unwrap();

        let services = Rc::new(StubbedServices { inner: TestConfigurationEditingServices::new(file_service_contract.clone()), dirty: Cell::new(false), prompts: Cell::new(0) });
        let configuration_service: Rc<dyn IWorkbenchConfigurationService> = workspace_service.clone();
        let context_service: Rc<dyn IWorkspaceContextService> = workspace_service.clone();
        let test_object = ConfigurationEditing::new(
            None,
            configuration_service,
            context_service,
            user_data_profile_service.clone(),
            user_data_profiles_service.clone(),
            file_service_contract.clone(),
            services.clone(),
            uri_identity_service.clone(),
        );

        Fixture {
            file_service: file_service_contract,
            user_data_profile_service,
            environment_service,
            workspace_service,
            services,
            test_object,
            policy_file,
            disposables,
            _registry: registry,
        }
    }

    /// `userDataProfileService.currentProfile.settingsResource`.
    fn settings_resource(&self) -> URI {
        self.user_data_profile_service.current_profile().settings_resource
    }

    /// `joinPath(workspaceService.getWorkspace().folders[0].uri, relative)`.
    fn workspace_path(&self, relative: &str) -> URI {
        join_path(&self.workspace_service.get_workspace().folders()[0].uri, &[relative])
    }

    /// `joinPath(environmentService.userRoamingDataHome, relative)`.
    fn user_path(&self, relative: &str) -> URI {
        join_path(&self.environment_service.user_roaming_data_home(), &[relative])
    }

    /// `fileService.writeFile(resource, VSBuffer.fromString(content))`.
    async fn write_file(&self, resource: &URI, content: &str) {
        self.file_service.write_file(resource, VSBuffer::from_string(content).into(), None).await.unwrap();
    }

    /// `json.parse((await fileService.readFile(resource)).value.toString())`.
    async fn read_parsed(&self, resource: &URI) -> Dynamic {
        parse(&self.read_text(resource).await)
    }

    /// `(await fileService.readFile(resource)).value.toString()`.
    async fn read_text(&self, resource: &URI) -> String {
        self.file_service.read_file(resource, None, None).await.unwrap().value.to_string()
    }

    /// `testObject.writeConfiguration(target, { key, value }, options)`.
    fn write(&self, target: EditableConfigurationTarget, key: &str, value: Dynamic, options: ConfigurationEditingOptions) -> Promise<()> {
        self.test_object.write_configuration(target, ConfigurationEditingValue { key: key.to_owned(), value }, options)
    }

    /// O `try { await writeConfiguration(...) } catch (error) { return error.code }` e o `assert.fail` depois do `try`.
    async fn write_error_code(&self, target: EditableConfigurationTarget, key: &str, value: Dynamic, options: ConfigurationEditingOptions) -> ConfigurationEditingErrorCode {
        let error = self.write(target, key, value, options).await.expect_err("deveria falhar");
        configuration_editing_error_code(&error).expect("o erro deve ter o code")
    }
}

/// `{ donotNotifyError: true }`.
fn donot_notify_error() -> ConfigurationEditingOptions {
    ConfigurationEditingOptions { options: ConfigurationUpdateOptions { donot_notify_error: true, ..Default::default() }, scopes: None }
}

/// `{}`: sem opções.
fn no_options() -> ConfigurationEditingOptions {
    ConfigurationEditingOptions::default()
}

/// `json.parse(text)`.
fn parse(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    from_json(&json::parse_str(text, &mut errors).expect("JSON válido"))
}

/// `parsed[key]`.
fn at(value: &Dynamic, key: &str) -> Dynamic {
    value.get(&jstr(key))
}

/// `assert.strictEqual(parsed[key], expected)`.
#[track_caller]
fn assert_text(actual: &Dynamic, expected: &str) {
    assert_deep_strict_equal(actual, &s(expected));
}

/// `{ 'version': '1.0.0', tasks: [{ 'taskName': 'myTask' }] }`.
fn full_tasks() -> Dynamic {
    j("{ 'version': '1.0.0', 'tasks': [{ 'taskName': 'myTask' }] }")
}

/// `assert.strictEqual(parsed['version'], '1.0.0'); assert.strictEqual(parsed['tasks'][0]['taskName'], 'myTask')`.
#[track_caller]
fn assert_full_tasks(parsed: &Dynamic) {
    assert_text(&at(parsed, "version"), "1.0.0");
    assert_text(&at(&at(&at(parsed, "tasks"), "0"), "taskName"), "myTask");
}

/// O arquivo de tarefas de `WORKSPACE_STANDALONE_CONFIGURATIONS['tasks']`.
fn workspace_tasks_path() -> &'static str {
    WORKSPACE_STANDALONE_CONFIGURATIONS.iter().find(|(key, _)| *key == "tasks").expect("tasks").1
}

/// O arquivo de tarefas de `USER_STANDALONE_CONFIGURATIONS['tasks']`.
fn user_tasks_path() -> &'static str {
    USER_STANDALONE_CONFIGURATIONS.iter().find(|(key, _)| *key == "tasks").expect("tasks").1
}

/// O arquivo de `USER_STANDALONE_CONFIGURATIONS['mcp']`.
fn user_mcp_path() -> &'static str {
    USER_STANDALONE_CONFIGURATIONS.iter().find(|(key, _)| *key == "mcp").expect("mcp").1
}

/// `FOLDER_SETTINGS_PATH` relativo à pasta (`joinPath(folder, FOLDER_SETTINGS_PATH)`).
fn folder_settings_of(folder: &URI) -> URI {
    join_path(folder, &[FOLDER_SETTINGS_PATH])
}

/// O corpo comum dos casos: `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o caso, no relógio virtual.
fn run<F: Future<Output = ()> + 'static>(case: impl FnOnce(Fixture) -> F + 'static) {
    run_with_faked_timers(async move {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = Fixture::new().await;
        case(fixture).await;
    });
}

#[test]
fn errors_cases_invalid_key() {
    run(|f| async move {
        let code = f.write_error_code(EditableConfigurationTarget::Workspace, "unknown.key", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorUnknownKey);
    });
}

#[test]
fn errors_cases_no_workspace() {
    run(|f| async move {
        f.workspace_service.initialize(&AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: generate_uuid() })).await.unwrap();
        let code = f.write_error_code(EditableConfigurationTarget::Workspace, "configurationEditing.service.testSetting", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorNoWorkspaceOpened);
    });
}

#[test]
fn errors_cases_invalid_configuration() {
    run(|f| async move {
        f.write_file(&f.settings_resource(), ",,,,,,,,,,,,,,").await;
        let code = f.write_error_code(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorInvalidConfiguration);
    });
}

#[test]
fn errors_cases_invalid_global_tasks_configuration() {
    run(|f| async move {
        let resource = f.user_path(user_tasks_path());
        f.write_file(&resource, ",,,,,,,,,,,,,,").await;
        let code = f.write_error_code(EditableConfigurationTarget::UserLocal, "tasks.configurationEditing.service.testSetting", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorInvalidConfiguration);
    });
}

#[test]
fn errors_cases_dirty() {
    run(|f| async move {
        f.services.dirty.set(true);
        let code = f.write_error_code(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorConfigurationFileDirty);
    });
}

#[test]
fn do_not_notify_error() {
    run(|f| async move {
        f.services.dirty.set(true);
        let code = f.write_error_code(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", s("value"), donot_notify_error()).await;
        assert!(f.services.prompts.get() != 1, "o prompt não deve ser chamado uma vez");
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorConfigurationFileDirty);
    });
}

#[test]
fn errors_cases_error_policy_configuration() {
    run(|f| async move {
        let promise = f.workspace_service.on_did_change_configuration().to_promise(None);
        f.write_file(&f.policy_file, "{ \"configurationEditing.service.policySetting\": \"policyValue\" }").await;
        promise.await.unwrap();
        let code = f.write_error_code(EditableConfigurationTarget::UserLocal, "configurationEditing.service.policySetting", s("value"), donot_notify_error()).await;
        assert_eq!(code, ConfigurationEditingErrorCode::ErrorPolicyConfiguration);
    });
}

#[test]
fn write_policy_setting_when_not_set() {
    run(|f| async move {
        f.write(EditableConfigurationTarget::UserLocal, "configurationEditing.service.policySetting", s("value"), donot_notify_error()).await.unwrap();
        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_text(&at(&parsed, "configurationEditing.service.policySetting"), "value");
    });
}

#[test]
fn write_one_setting_empty_file() {
    run(|f| async move {
        f.write(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", s("value"), no_options()).await.unwrap();
        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_text(&at(&parsed, "configurationEditing.service.testSetting"), "value");
    });
}

#[test]
fn write_one_setting_existing_file() {
    run(|f| async move {
        f.write_file(&f.settings_resource(), "{ \"my.super.setting\": \"my.super.value\" }").await;
        f.write(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_text(&at(&parsed, "configurationEditing.service.testSetting"), "value");
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn remove_an_existing_setting_existing_file() {
    run(|f| async move {
        f.write_file(&f.settings_resource(), "{ \"my.super.setting\": \"my.super.value\", \"configurationEditing.service.testSetting\": \"value\" }").await;
        f.write(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", Dynamic::Undefined, no_options()).await.unwrap();

        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_eq!(object_keys(&parsed), strs(&["my.super.setting"]));
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn remove_non_existing_setting_existing_file() {
    run(|f| async move {
        f.write_file(&f.settings_resource(), "{ \"my.super.setting\": \"my.super.value\" }").await;
        f.write(EditableConfigurationTarget::UserLocal, "configurationEditing.service.testSetting", Dynamic::Undefined, no_options()).await.unwrap();

        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_eq!(object_keys(&parsed), strs(&["my.super.setting"]));
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn write_overridable_settings_to_user_settings() {
    run(|f| async move {
        let value = j("{ 'configurationEditing.service.testSetting': 'overridden value' }");
        f.write(EditableConfigurationTarget::UserLocal, "[language]", value.clone(), no_options()).await.unwrap();

        let parsed = f.read_parsed(&f.settings_resource()).await;
        assert_deep_strict_equal(&at(&parsed, "[language]"), &value);
    });
}

#[test]
fn write_overridable_settings_to_workspace_settings() {
    run(|f| async move {
        let value = j("{ 'configurationEditing.service.testSetting': 'overridden value' }");
        f.write(EditableConfigurationTarget::Workspace, "[language]", value.clone(), no_options()).await.unwrap();

        let parsed = f.read_parsed(&f.workspace_path(FOLDER_SETTINGS_PATH)).await;
        assert_deep_strict_equal(&at(&parsed, "[language]"), &value);
    });
}

#[test]
fn write_overridable_settings_to_workspace_folder_settings() {
    run(|f| async move {
        let value = j("{ 'configurationEditing.service.testSetting': 'overridden value' }");
        let folder_settings_file = folder_settings_of(&f.workspace_service.get_workspace().folders()[0].uri);
        let options = ConfigurationEditingOptions {
            options: ConfigurationUpdateOptions::default(),
            scopes: Some(ConfigurationUpdateOverrides { override_identifiers: None, resource: Some(folder_settings_file.clone()) }),
        };
        f.write(EditableConfigurationTarget::WorkspaceFolder, "[language]", value.clone(), options).await.unwrap();

        let parsed = f.read_parsed(&folder_settings_file).await;
        assert_deep_strict_equal(&at(&parsed, "[language]"), &value);
    });
}

#[test]
fn write_workspace_standalone_setting_empty_file() {
    run(|f| async move {
        let target = f.workspace_path(workspace_tasks_path());
        f.write(EditableConfigurationTarget::Workspace, "tasks.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&target).await;
        assert_text(&at(&parsed, "service.testSetting"), "value");
    });
}

#[test]
fn write_user_standalone_setting_empty_file() {
    run(|f| async move {
        let target = f.user_path(user_tasks_path());
        f.write(EditableConfigurationTarget::UserLocal, "tasks.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&target).await;
        assert_text(&at(&parsed, "service.testSetting"), "value");
    });
}

#[test]
fn write_workspace_standalone_setting_existing_file() {
    run(|f| async move {
        let target = f.workspace_path(workspace_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": \"my.super.value\" }").await;

        f.write(EditableConfigurationTarget::Workspace, "tasks.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&target).await;
        assert_text(&at(&parsed, "service.testSetting"), "value");
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn write_user_standalone_setting_existing_file() {
    run(|f| async move {
        let target = f.user_path(user_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": \"my.super.value\" }").await;

        f.write(EditableConfigurationTarget::UserLocal, "tasks.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&target).await;
        assert_text(&at(&parsed, "service.testSetting"), "value");
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn write_user_standalone_mcp_setting_existing_file() {
    run(|f| async move {
        let target = f.user_path(user_mcp_path());
        f.write_file(&target, "{ \"my.super.setting\": \"my.super.value\" }").await;

        f.write(EditableConfigurationTarget::UserLocal, "mcp.service.testSetting", s("value"), no_options()).await.unwrap();

        let parsed = f.read_parsed(&target).await;
        assert_text(&at(&parsed, "service.testSetting"), "value");
        assert_text(&at(&parsed, "my.super.setting"), "my.super.value");
    });
}

#[test]
fn write_workspace_standalone_setting_empty_file_full_json() {
    run(|f| async move {
        f.write(EditableConfigurationTarget::Workspace, "tasks", full_tasks(), no_options()).await.unwrap();

        let target = f.workspace_path(workspace_tasks_path());
        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

#[test]
fn write_user_standalone_setting_empty_file_full_json() {
    run(|f| async move {
        f.write(EditableConfigurationTarget::UserLocal, "tasks", full_tasks(), no_options()).await.unwrap();

        let target = f.user_path(user_tasks_path());
        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

#[test]
fn write_workspace_standalone_setting_existing_file_full_json() {
    run(|f| async move {
        let target = f.workspace_path(workspace_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": \"my.super.value\" }").await;

        f.write(EditableConfigurationTarget::Workspace, "tasks", full_tasks(), no_options()).await.unwrap();

        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

#[test]
fn write_user_standalone_setting_existing_file_full_json() {
    run(|f| async move {
        let target = f.user_path(user_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": \"my.super.value\" }").await;

        f.write(EditableConfigurationTarget::UserLocal, "tasks", full_tasks(), no_options()).await.unwrap();

        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

#[test]
fn write_workspace_standalone_setting_existing_file_with_json_errors_full_json() {
    run(|f| async move {
        let target = f.workspace_path(workspace_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": ").await; // invalid JSON

        f.write(EditableConfigurationTarget::Workspace, "tasks", full_tasks(), no_options()).await.unwrap();

        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

#[test]
fn write_user_standalone_setting_existing_file_with_json_errors_full_json() {
    run(|f| async move {
        let target = f.user_path(user_tasks_path());
        f.write_file(&target, "{ \"my.super.setting\": ").await; // invalid JSON

        f.write(EditableConfigurationTarget::UserLocal, "tasks", full_tasks(), no_options()).await.unwrap();

        assert_full_tasks(&f.read_parsed(&target).await);
    });
}

/// O arquivo de tarefas com duas tarefas, que a escrita deve substituir inteiro.
const TWO_TASKS_FILE: &str = "{\n\t\t\t\"version\": \"1.0.0\",\n\t\t\t\"tasks\": [\n\t\t\t\t{\n\t\t\t\t\t\"taskName\": \"myTask1\"\n\t\t\t\t},\n\t\t\t\t{\n\t\t\t\t\t\"taskName\": \"myTask2\"\n\t\t\t\t}\n\t\t\t]\n\t\t}";

/// `JSON.stringify({ 'version': '1.0.0', tasks: [{ 'taskName': 'myTask1' }] }, null, '\t')`.
fn one_task_expected() -> String {
    json_stringify(&to_json(&j("{ 'version': '1.0.0', 'tasks': [{ 'taskName': 'myTask1' }] }")).expect("JSON"), Some("\t"))
}

#[test]
fn write_workspace_standalone_setting_should_replace_complete_file() {
    run(|f| async move {
        let target = f.workspace_path(workspace_tasks_path());
        f.write_file(&target, TWO_TASKS_FILE).await;

        f.write(EditableConfigurationTarget::Workspace, "tasks", j("{ 'version': '1.0.0', 'tasks': [{ 'taskName': 'myTask1' }] }"), no_options()).await.unwrap();

        assert_eq!(f.read_text(&target).await, one_task_expected());
    });
}

#[test]
fn write_user_standalone_setting_should_replace_complete_file() {
    run(|f| async move {
        let target = f.user_path(user_tasks_path());
        f.write_file(&target, TWO_TASKS_FILE).await;

        f.write(EditableConfigurationTarget::UserLocal, "tasks", j("{ 'version': '1.0.0', 'tasks': [{ 'taskName': 'myTask1' }] }"), no_options()).await.unwrap();

        assert_eq!(f.read_text(&target).await, one_task_expected());
    });
}
