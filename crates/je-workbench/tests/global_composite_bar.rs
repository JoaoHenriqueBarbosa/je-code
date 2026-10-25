//! Porte de `src/vs/workbench/test/browser/parts/globalCompositeBar.test.ts` (suítes `GlobalCompositeBar`,
//! `AccountsActivityActionViewItem - updateAvatar`, `- addOrUpdateAccount` e `- account tiers`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso.
//! - **`Reflect.get(GlobalCompositeBar.prototype, ...)` com um `this` de objeto literal.** Os métodos testados são
//!   `pub` em [`GlobalCompositeBar`] e [`AccountsCore`] (a lógica do item de contas sem a árvore). A barra do caso é a real,
//!   montada por [`GlobalCompositeBar::from_action_bar`] sobre uma `ActionBar` e um armazenamento em memória; o
//!   `accountsVisibilityPreference` que o original troca no objeto é o valor gravado nesse armazenamento, e o
//!   `_onDidChange` é o `on_did_change` da barra.
//! - **O `this` do item de contas** é o [`AccountsCore`], com as dependências reduzidas de [`AccountsCoreDeps`]: um
//!   `configurationService` que devolve o valor dado, um serviço de conta padrão que devolve a conta corrente, uma
//!   autenticação sem provedores e sem sessões e a sessão do embedder resolvida como `undefined`.
//! - **`avatarImg` e `label`.** O `<img>` é o [`AvatarImage`] do núcleo e o `label` é um nó da árvore, onde a classe
//!   `has-avatar` é lida.
//! - **`deepStrictEqual` de objetos literais** é `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{Action, ActionRef, Separator};
use je_base::default_account::{IDefaultAccount, IDefaultAccountAuthenticationProvider};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableStore;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::storage::storage::InMemoryStorageService;
use je_ui::{Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use je_workbench::browser::parts::global_composite_bar::{
    AccountsCore, AccountsCoreDeps, AvatarImage, GlobalCompositeBar, GroupedAccount, IAccountsAuthentication, IAccountsDefaultAccount,
};
use je_workbench::common::activity::{ACCOUNTS_SHARED_SIGN_IN_GROUP, GLOBAL_ACTIVITY_ID};
use je_workbench::services::agent_host::codex_account_service::{CodexAccountStatus, ICodexAccountInfo, ICodexAccountService};
use je_workbench::services::authentication::authentication::{
    AuthenticationProviderInformation, AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChange, IAuthenticationProvider,
};

/// `createGlobalBar(accountsVisible)`: a barra com as ações "Accounts" (se visível) e "Manage".
fn create_global_bar(guard: &LeakGuard, accounts_visible: bool) -> (Rc<GlobalCompositeBar>, Rc<ActionBar>) {
    let ui = Ui::new();
    let container = ui.tree_mut().create(ui.root(), Role::Generic);
    let action_bar = ActionBar::new(&ui, container, IActionBarOptions::default());
    let storage = guard.add_rc(InMemoryStorageService::new());
    let bar = GlobalCompositeBar::from_action_bar(&ui, container, action_bar.clone(), Rc::new(storage));
    // O estado inicial do original: `accountsVisibilityPreference: accountsVisible`, com as ações empurradas à mão.
    if bar.accounts_visibility_preference() != accounts_visible {
        bar.global_activity_action_bar().clear();
        bar.set_accounts_visibility_preference(accounts_visible);
        if accounts_visible {
            bar.global_activity_action_bar().push_action(bar.account_action().clone(), IActionOptions::default());
        }
        bar.global_activity_action_bar().push_action(Action::new(GLOBAL_ACTIVITY_ID, "", "", true, None), IActionOptions::default());
    }
    guard.add_rc(bar.clone());
    (bar, action_bar)
}

#[test]
fn counts_only_gaps_between_rendered_actions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (bar, action_bar) = create_global_bar(&guard, true);
    let two_actions = bar.get_height(36.0, 8.0);
    action_bar.pull(0);
    let one_action = bar.get_height(36.0, 8.0);
    action_bar.clear();
    let empty = bar.get_height(36.0, 8.0);

    assert_eq!((two_actions, one_action, empty), (80.0, 36.0, 0.0));
}

#[test]
fn reports_size_changes_when_accounts_is_toggled_without_duplicating_actions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (bar, action_bar) = create_global_bar(&guard, true);
    let heights = Rc::new(RefCell::new(vec![bar.get_height(36.0, 8.0)]));
    let recording = heights.clone();
    let listening = bar.clone();
    guard.add(bar.on_did_change().listen(move |_| recording.borrow_mut().push(listening.get_height(36.0, 8.0))));

    bar.toggle_accounts_activity();
    bar.set_accounts_visibility_preference(false);
    bar.toggle_accounts_activity();
    bar.toggle_accounts_activity();
    bar.set_accounts_visibility_preference(true);
    bar.toggle_accounts_activity();
    bar.toggle_accounts_activity();

    assert_eq!((heights.borrow().clone(), action_bar.length()), (vec![80.0, 36.0, 80.0], 2));
}

#[test]
fn keeps_a_hidden_accounts_action_hidden_when_its_preference_is_unchanged() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (bar, action_bar) = create_global_bar(&guard, false);
    let heights = Rc::new(RefCell::new(Vec::<f64>::new()));
    let recording = heights.clone();
    let listening = bar.clone();
    guard.add(bar.on_did_change().listen(move |_| recording.borrow_mut().push(listening.get_height(28.0, 0.0))));

    bar.toggle_accounts_activity();

    assert_eq!((heights.borrow().clone(), bar.get_height(28.0, 0.0), action_bar.length()), (Vec::<f64>::new(), 28.0, 1));
}

/// `createDefaultAccount(providerId, accountName)`.
fn create_default_account(provider_id: &str, account_name: &str) -> IDefaultAccount {
    IDefaultAccount {
        authentication_provider: IDefaultAccountAuthenticationProvider { id: provider_id.to_owned(), name: provider_id.to_owned(), enterprise: false, extra: Default::default() },
        account_name: account_name.to_owned(),
        session_id: "test-session".to_owned(),
        enterprise: false,
        entitlements_data: None,
        extra: Default::default(),
    }
}

/// `defaultAccountService: { currentDefaultAccount }`.
struct DefaultAccountStub(Option<IDefaultAccount>);

impl IAccountsDefaultAccount for DefaultAccountStub {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        Event::none()
    }

    fn current_default_account(&self) -> Option<IDefaultAccount> {
        self.0.clone()
    }

    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        Promise::resolved(self.0.clone())
    }
}

/// `authenticationService: { getProviderIds: () => [], isDynamicAuthenticationProvider: () => false, getSessions: async () => [] }`.
struct AuthenticationStub;

impl IAccountsAuthentication for AuthenticationStub {
    fn on_did_register_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        Event::none()
    }

    fn on_did_unregister_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        Event::none()
    }

    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange> {
        Event::none()
    }

    fn get_provider_ids(&self) -> Vec<String> {
        Vec::new()
    }

    fn is_dynamic_authentication_provider(&self, _id: &str) -> bool {
        false
    }

    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider> {
        panic!("nenhum provedor registrado: {id}")
    }

    fn get_sessions(&self, _id: &str) -> Promise<Vec<AuthenticationSession>> {
        Promise::resolved(Vec::new())
    }
}

/// `commandService`: nada o chama nestes casos.
struct NullCommandService;

impl ICommandService for NullCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        Promise::resolved(Dynamic::Undefined)
    }
}

/// `codexAccountService: { agent: 'codex', account: { status }, onDidChangeAccount: Event.None, signIn() {}, signOut() {} }`.
struct CodexAccountStub {
    status: CodexAccountStatus,
}

impl ICodexAccountService for CodexAccountStub {
    fn account(&self) -> ICodexAccountInfo {
        ICodexAccountInfo { status: self.status, email: None }
    }

    fn on_did_change_account(&self) -> Event<ICodexAccountInfo> {
        Event::none()
    }

    fn sign_in(&self) {}

    fn sign_out(&self) {}
}

/// O `this` do item de contas: o [`AccountsCore`] com as dependências do caso.
fn create_accounts_core(
    ui: &Ui,
    configuration: impl Fn(Option<&str>) -> Dynamic + 'static,
    current_default_account: Option<IDefaultAccount>,
    codex_status: CodexAccountStatus,
) -> Rc<AccountsCore> {
    let configuration_service = TestConfigurationService::new(None);
    configuration_service.stub_get_value_with(move |section, _| configuration(section));
    AccountsCore::new(
        ui,
        AccountsCoreDeps {
            authentication_service: Rc::new(AuthenticationStub),
            default_account_service: Rc::new(DefaultAccountStub(current_default_account)),
            configuration_service,
            log_service: NullLogService::shared(),
            command_service: Rc::new(NullCommandService),
            codex_account_service: Rc::new(CodexAccountStub { status: codex_status }),
            // `FileAccess.uriToBrowserUri` deixa intacto o que não é `file` nem `vscode-remote`.
            uri_to_browser_uri: Rc::new(|uri| uri.clone()),
            session_from_embedder: Rc::new(|| Promise::resolved(None)),
        },
    )
}

fn account(id: &str, label: &str, icon: Option<URI>) -> GroupedAccount {
    GroupedAccount { account: AuthenticationSessionAccount { id: id.to_owned(), label: label.to_owned(), icon }, can_sign_out: true }
}

fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `{ src: harness.avatarImg.src, hasAvatarClass: harness.label.classList.contains('has-avatar') }`.
fn read_avatar(ui: &Ui, core: &AccountsCore, label: je_ui::NodeId) -> (Option<String>, bool) {
    let AvatarImage { src, .. } = core.avatar_img().expect("o img foi criado");
    (src, ui.tree().has_class(label, "has-avatar"))
}

/// `harness = createHarness(groupedAccounts, currentDefaultAccount)` e `updateAvatar.call(harness)`.
fn update_avatar_for(first_icon: &URI, default_icon: Option<URI>, default_account: Option<IDefaultAccount>) -> (Option<String>, bool) {
    let ui = Ui::new();
    let label = ui.tree_mut().create(ui.root(), Role::Generic);
    let core = create_accounts_core(&ui, |_| Dynamic::Bool(true), default_account, CodexAccountStatus::SignedOut);
    core.grouped_accounts().set("github", vec![account("first-id", "first-account", Some(first_icon.clone()))]);
    core.grouped_accounts().set("microsoft", vec![account("default-id", "default-account", default_icon)]);
    core.attach_label(label);
    core.update_avatar();
    read_avatar(&ui, &core, label)
}

#[test]
fn prefers_the_current_default_account_avatar_over_the_first_account_with_an_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (first_icon, default_icon) = (uri("https://example.com/first.png"), uri("https://example.com/default.png"));

    let actual = update_avatar_for(&first_icon, Some(default_icon.clone()), Some(create_default_account("microsoft", "default-account")));

    assert_eq!(actual, (Some(default_icon.to_string(true).to_string_lossy()), true));
}

#[test]
fn falls_back_to_the_first_account_with_an_icon_when_there_is_no_default_account() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (first_icon, default_icon) = (uri("https://example.com/first.png"), uri("https://example.com/default.png"));

    let actual = update_avatar_for(&first_icon, Some(default_icon), None);

    assert_eq!(actual, (Some(first_icon.to_string(true).to_string_lossy()), true));
}

#[test]
fn falls_back_to_the_first_account_with_an_icon_when_the_matching_default_account_has_no_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let first_icon = uri("https://example.com/first.png");

    let actual = update_avatar_for(&first_icon, None, Some(create_default_account("microsoft", "default-account")));

    assert_eq!(actual, (Some(first_icon.to_string(true).to_string_lossy()), true));
}

#[test]
fn updates_the_icon_of_an_existing_account_including_clearing_a_stale_one() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let ui = Ui::new();
        let core = create_accounts_core(&ui, |_| Dynamic::Bool(true), None, CodexAccountStatus::SignedOut);
        core.grouped_accounts().set("github", vec![account("account-id", "account", Some(uri("https://example.com/stale.png")))]);
        let icon_of = |core: &AccountsCore| core.grouped_accounts().get("github").and_then(|accounts| accounts[0].account.icon.clone());

        core.add_or_update_account("github", AuthenticationSessionAccount { id: "account-id".to_owned(), label: "account".to_owned(), icon: Some(uri("https://example.com/fresh.png")) })
            .await
            .unwrap();
        let updated = icon_of(&core);

        core.add_or_update_account("github", AuthenticationSessionAccount { id: "account-id".to_owned(), label: "account".to_owned(), icon: None }).await.unwrap();
        let cleared = icon_of(&core);

        assert_eq!((updated, cleared), (Some(uri("https://example.com/fresh.png")), None));
    });
}

/// O `createHarness(initialized)` da suíte `account tiers`, e a chamada de `resolveMainMenuActions` com o menu
/// `{ getActions: () => [[ACCOUNTS_SHARED_SIGN_IN_GROUP, [github]], ['z_manage', [manage]]] }`.
fn resolve_labels(initialized: bool) -> Vec<String> {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    // `getValue: key => key === 'chat.disableAIFeatures' ? false : true`.
    let core = create_accounts_core(&ui, |section| Dynamic::Bool(section != Some("chat.disableAIFeatures")), None, CodexAccountStatus::SignedOut);
    core.set_initialized(initialized);
    let action_disposables = DisposableStore::new();
    guard.add(action_disposables.clone());
    let github = guard.add_rc(Action::new("github", "Sign in with GitHub", "", true, None));
    let github: ActionRef = github;
    let manage = guard.add_rc(Action::new("manage", "Manage Language Model Access", "", true, None));
    let manage: ActionRef = manage;
    let groups = vec![(ACCOUNTS_SHARED_SIGN_IN_GROUP.to_owned(), vec![github]), ("z_manage".to_owned(), vec![manage])];

    let labels = Rc::new(RefCell::new(Vec::new()));
    let recording = labels.clone();
    let disposables = action_disposables.clone();
    run_with_faked_timers(async move {
        let actions = core.resolve_main_menu_actions_for_groups(groups, &disposables).await.unwrap();
        *recording.borrow_mut() =
            actions.iter().map(|action| if action.as_any().is::<Separator>() { "separator".to_owned() } else { action.label() }).collect::<Vec<_>>();
    });
    let result = labels.borrow().clone();
    result
}

#[test]
fn delays_chatgpt_without_hiding_other_account_commands_while_shared_accounts_resolve() {
    assert_eq!(resolve_labels(false), vec!["Loading...", "separator", "Sign in with GitHub", "separator", "Manage Language Model Access"]);
}

#[test]
fn places_shared_sign_in_before_chatgpt_and_unrelated_account_commands() {
    assert_eq!(resolve_labels(true), vec!["Sign in with GitHub", "separator", "Sign in to ChatGPT", "separator", "Manage Language Model Access"]);
}

