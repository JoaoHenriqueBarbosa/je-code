//! Porte de `src/vs/workbench/contrib/relauncher/test/browser/relauncher.test.ts` (suíte `SettingsChangeRelauncher`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `DisposableStore` da suíte (limpo no `teardown`) são o guarda criado
//!   no começo de cada caso, e `disposables.add(new SettingsChangeRelauncher(...))` é `ds.add_rc(...)`.
//! - `restartCount`, `confirmResult` e `confirmCount` (variáveis da suíte zeradas no `setup`) são o [`State`] criado em
//!   cada caso; os literais `{ hasFocus: true, restart }`, `{ confirm }`, `{ status: SyncStatus.Idle }`, `{ isEnabled }`,
//!   `{ onDidTurnOnSync: Event.None }` e `{ nameLong }` são os dublês abaixo, que implementam os contratos do módulo de
//!   produção (os de sincronização e o do diálogo são os estreitos descritos nas divergências dele).
//! - `initialConfiguration.window ??= {}` é a chave `window` com o objeto vazio em [`create_configuration`], e o objeto de
//!   configuração é compartilhado com o `TestConfigurationService` (o `applyChange` do original o muta depois de criar o
//!   relauncher), o que [`set_nested`] reproduz sobre o mesmo [`Dynamic`].
//! - `onDidChangeConfigurationEmitter.fire({ source, affectedKeys, change, affectsConfiguration })` é o
//!   `ConfigurationChangeEvent` da chave, com o `source` definido depois; o `affectsConfiguration` dele segue a regra de
//!   seção do evento real (que para as chaves exatas dos casos coincide com o `configuration === key` do original).
//! - `await Promise.resolve()` é `Promise::resolved(()).await` dentro de `run_with_faked_timers` (as microtarefas só andam
//!   ali), e vem no fim de [`change_setting`].
//! - `isNative`, `isLinux` e `isMacintosh` entram pelo `Platform::detect()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::models::{Configuration, ConfigurationModel};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationChange, ConfigurationTarget};
use je_platform::dialogs::dialogs::{Confirmation, ConfirmationResult};
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::user_data_sync::user_data_sync::SyncStatus;
use je_base::map::ResourceMap;
use je_workbench::contrib::relauncher::{
    RelauncherDialogService, RelauncherUserDataSyncEnablementService, RelauncherUserDataSyncService, RelauncherUserDataSyncWorkbenchService,
    SettingsChangeRelauncher,
};
use je_workbench::services::host::host::{IHostService, ReloadOptions};

/// `restartCount`, `confirmCount` e `confirmResult`.
struct State {
    restart_count: Cell<u32>,
    confirm_count: Cell<u32>,
    confirm_result: Cell<bool>,
}

impl State {
    fn new(confirm_result: bool) -> Rc<Self> {
        Rc::new(Self { restart_count: Cell::new(0), confirm_count: Cell::new(0), confirm_result: Cell::new(confirm_result) })
    }
}

/// `{ hasFocus: true, restart: async () => { restartCount++; } }`.
struct TestHostService {
    state: Rc<State>,
}

impl IHostService for TestHostService {
    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        errors::throw(Error::new("Method not implemented."))
    }

    fn restart(&self) -> Promise<()> {
        self.state.restart_count.set(self.state.restart_count.get() + 1);
        Promise::resolved(())
    }

    fn has_focus(&self) -> bool {
        true
    }
}

/// `{ confirm: async () => { confirmCount++; return { confirmed: confirmResult }; } }`.
struct TestDialogService {
    state: Rc<State>,
}

impl RelauncherDialogService for TestDialogService {
    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        self.state.confirm_count.set(self.state.confirm_count.get() + 1);
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: self.state.confirm_result.get() })
    }
}

/// `{ status: SyncStatus.Idle }`.
struct TestUserDataSyncService;

impl RelauncherUserDataSyncService for TestUserDataSyncService {
    fn status(&self) -> SyncStatus {
        SyncStatus::Idle
    }
}

/// `{ isEnabled: () => true }`.
struct TestUserDataSyncEnablementService;

impl RelauncherUserDataSyncEnablementService for TestUserDataSyncEnablementService {
    fn is_enabled(&self) -> bool {
        true
    }
}

/// `{ onDidTurnOnSync: Event.None }`.
struct TestUserDataSyncWorkbenchService;

impl RelauncherUserDataSyncWorkbenchService for TestUserDataSyncWorkbenchService {
    fn on_did_turn_on_sync(&self) -> Event<()> {
        Event::none()
    }
}

/// O objeto de configuração de um caso: `{ chat: { ... : value } }` com o `window` vazio que o `createRelauncher` garante.
fn create_configuration(path: &[&str], value: bool) -> Dynamic {
    let chat = path.iter().rev().fold(Dynamic::Bool(value), |inner, key| Dynamic::object([(*key, inner)]));
    Dynamic::object([("window", Dynamic::object(Vec::<(&str, Dynamic)>::new())), ("chat", chat)])
}

/// `configuration.chat.a.b.c = value`: muta o objeto compartilhado com o serviço de configuração.
fn set_nested(configuration: &Dynamic, path: &[&str], value: bool) {
    let (last, parents) = path.split_last().expect("o caminho não é vazio");
    let parent = parents.iter().fold(configuration.get_str("chat"), |current, key| current.get_str(key));
    let Dynamic::Object(parent) = parent else { panic!("o pai é um objeto") };
    parent.set(&JsString::from(*last), Dynamic::Bool(value));
}

fn empty_model() -> ConfigurationModel {
    ConfigurationModel::create_empty_model(NullLogService::shared())
}

fn empty_configuration() -> Configuration {
    Configuration::new(
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        ResourceMap::new(),
        empty_model(),
        ResourceMap::new(),
        NullLogService::shared(),
    )
}

/// `createRelauncher(initialConfiguration)`.
fn create_relauncher(ds: &LeakGuard, state: &Rc<State>, configuration: Dynamic) -> Rc<TestConfigurationService> {
    let configuration_service = TestConfigurationService::new(Some(configuration));
    ds.add_rc(configuration_service.on_did_change_configuration_emitter.clone());

    let product_service = ProductService { name_long: "Test Product".to_owned(), quality: None, ..ProductService::default() };

    ds.add_rc(SettingsChangeRelauncher::new(
        Platform::detect(),
        Rc::new(TestHostService { state: state.clone() }),
        configuration_service.clone(),
        Rc::new(TestUserDataSyncService),
        Rc::new(TestUserDataSyncEnablementService),
        Rc::new(TestUserDataSyncWorkbenchService),
        Rc::new(product_service),
        Rc::new(TestDialogService { state: state.clone() }),
    ));

    configuration_service
}

/// `fireChange(configurationService, key, source = ConfigurationTarget.USER)`.
fn fire_change(configuration_service: &TestConfigurationService, key: &str, source: ConfigurationTarget) {
    let change = ConfigurationChange { keys: vec![JsString::from(key)], overrides: Vec::new() };
    let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
    event.source.set(Some(source));
    configuration_service.on_did_change_configuration_emitter.fire(Rc::new(event));
}

/// `changeSetting(key, createConfiguration, applyChange, source = ConfigurationTarget.USER)`: `path` é o caminho sob `chat`
/// do `key`, que o `createConfiguration` e o `applyChange` do original escrevem à mão.
fn change_setting(ds: &LeakGuard, state: &Rc<State>, key: &str, path: &[&str], source: ConfigurationTarget) {
    let configuration = create_configuration(path, true);
    let configuration_service = create_relauncher(ds, state, configuration.clone());

    set_nested(&configuration, path, false);
    fire_change(&configuration_service, key, source);
    run_with_faked_timers(async {
        Promise::resolved(()).await.expect("a promessa resolvida");
    });
}

#[test]
fn prompts_to_restart_when_chat_agent_host_claude_agent_enabled_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(true);
    change_setting(&ds, &state, "chat.agentHost.claudeAgent.enabled", &["agentHost", "claudeAgent", "enabled"], ConfigurationTarget::User);

    assert_eq!(state.confirm_count.get(), 1, "should prompt to restart");
    assert_eq!(state.restart_count.get(), 1, "should restart when confirmed");
}

#[test]
fn does_not_prompt_to_restart_when_chat_agent_host_codex_agent_enabled_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(true);
    change_setting(&ds, &state, "chat.agentHost.codexAgent.enabled", &["agentHost", "codexAgent", "enabled"], ConfigurationTarget::User);

    assert_eq!(state.confirm_count.get(), 0, "should not prompt to restart");
    assert_eq!(state.restart_count.get(), 0, "should not restart");
}

#[test]
fn does_not_prompt_to_restart_when_chat_agent_host_byok_models_enabled_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(true);
    change_setting(&ds, &state, "chat.agentHost.byokModels.enabled", &["agentHost", "byokModels", "enabled"], ConfigurationTarget::User);

    assert_eq!(state.confirm_count.get(), 0, "should not prompt to restart");
    assert_eq!(state.restart_count.get(), 0, "should not restart");
}

#[test]
fn prompts_to_restart_when_chat_editor_codex_prefer_agent_host_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(true);
    change_setting(&ds, &state, "chat.editor.codex.preferAgentHost", &["editor", "codex", "preferAgentHost"], ConfigurationTarget::User);

    assert_eq!(state.confirm_count.get(), 1, "should prompt to restart");
    assert_eq!(state.restart_count.get(), 1, "should restart when confirmed");
}

#[test]
fn does_not_restart_when_the_confirmation_is_declined() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(false);
    change_setting(&ds, &state, "chat.agentHost.claudeAgent.enabled", &["agentHost", "claudeAgent", "enabled"], ConfigurationTarget::User);

    assert_eq!(state.confirm_count.get(), 1, "should prompt to restart");
    assert_eq!(state.restart_count.get(), 0, "should not restart when declined");
}

#[test]
fn does_not_prompt_when_only_the_default_value_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let state = State::new(true);
    change_setting(&ds, &state, "chat.agentHost.claudeAgent.enabled", &["agentHost", "claudeAgent", "enabled"], ConfigurationTarget::Default);

    assert_eq!(state.confirm_count.get(), 0, "should not prompt for default changes");
    assert_eq!(state.restart_count.get(), 0);
}
