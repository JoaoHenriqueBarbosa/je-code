//! Porte de `src/vs/workbench/services/policies/test/browser/managedSettingsUpdate.test.ts` (suíte `Managed settings update presentation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o [`LeakGuard`] criado no começo de cada caso, e o `store` do original é
//!   o próprio guarda.
//! - `mock<IDefaultAccountService>()` com `managedSettingsCompatibilityError` e `onDidChangeManagedSettingsCompatibilityError`
//!   é [`StubDefaultAccountService`], e `mock<IUpdateService>()` com `state` e `onStateChange` é [`StubUpdateService`]; o
//!   método que o original não sobrescreve falha com o erro `Method not implemented.` do `mock`.
//! - `mock<IProductService>()` com `nameShort` e `version` é o `ProductService` com esses dois campos (`version` vazia é o
//!   `undefined` do produto sem versão); `mock<IBannerService>()` com `show` e `hide` é [`StubBannerService`].
//! - `new MockContextKeyService()` é o `MockContextKeyService`, e `ManagedSettingsUpdateRequiredContext.getValue(contexts)`
//!   é lido como `Option<bool>` (`undefined` é `None`).
//! - `configuration.setUserConfiguration(ChatAIDisabledSettingId, true)` seguido do `onDidChangeConfigurationEmitter.fire({ ... })`
//!   é `set_user_configuration` e `fire_configuration_change` (o evento real com a chave alterada, em que `affectsConfiguration`
//!   da chave vale `true`).
//! - `new TestInstantiationService()` com `stub(...)` é o `InstantiationService` sobre o `ServiceCollection` com esses
//!   serviços; `AccessibleViewRegistry.getImplementations().find(name === 'managedSettingsUpdate')` é a busca em
//!   `get_implementations()` depois de `register_accessible_view(ui)`, que o módulo do original faz ao carregar.
//! - O botão `append(mainWindow.document.body, $('button', ...))` com `focus()` e `blur()` é um nó `Button` na raiz do `Ui`,
//!   e `document.activeElement === target` é `ui.focused() == Some(target)`.
//! - `for (const isSessionsWindow of [false, true]) test(...)` é um laço dentro de um `#[test]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::JsString;
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IPolicyData};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::observable::autorun;
use je_base::observable::base::Observable;
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::accessibility::accessible_view::{AccessibleContentProvider, AccessibleViewContentProvider};
use je_platform::accessibility::accessible_view_registry::{IAccessibleViewImplementation, get_implementations};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::policy::managed_settings_freshness::IManagedSettingsFreshness;
use je_platform::product::{IProductService, ProductService};
use je_platform::update::update::{DisablementReason, State, Update, UpdateService, UpdateType};
use je_ui::{Role, Ui};
use je_workbench::services::accounts::default_account::{
    IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use je_workbench::services::accounts::managed_settings::IManagedSettingsCompatibilityError;
use je_workbench::services::banner::banner_service::{IBannerItem, IBannerService};
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::services::policies::managed_settings_update::{
    I_MANAGED_SETTINGS_UPDATE_SERVICE, IManagedSettingsUpdateAction, IManagedSettingsUpdateInfo, IManagedSettingsUpdateService, get_managed_settings_update_info,
    managed_settings_update_required_context,
};
use je_workbench::services::policies::managed_settings_update_contribution::{ManagedSettingsUpdateContribution, register_accessible_view};
use je_workbench::services::policies::managed_settings_update_service::ManagedSettingsUpdateService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};

/// `ChatAIDisabledSettingId`.
const CHAT_AI_DISABLED_SETTING_ID: &str = "chat.disableAIFeatures";

fn not_overridden() -> ! {
    throw(Error::new("Method not implemented."))
}

/// `new class extends mock<IDefaultAccountService>() { managedSettingsCompatibilityError; onDidChangeManagedSettingsCompatibilityError }`.
struct StubDefaultAccountService {
    error: RefCell<Option<IManagedSettingsCompatibilityError>>,
    changed: Rc<Emitter<Option<IManagedSettingsCompatibilityError>>>,
}

impl IDefaultAccountService for StubDefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        not_overridden()
    }
    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        not_overridden()
    }
    fn policy_data(&self) -> Option<IPolicyData> {
        not_overridden()
    }
    fn current_default_account(&self) -> Option<IDefaultAccount> {
        not_overridden()
    }
    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        not_overridden()
    }
    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        not_overridden()
    }
    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        not_overridden()
    }
    fn managed_settings_fetched_at(&self) -> Option<f64> {
        not_overridden()
    }
    fn managed_settings_raw_response(&self) -> Dynamic {
        not_overridden()
    }
    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        self.error.borrow().clone()
    }
    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        self.changed.event()
    }
    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        not_overridden()
    }
    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        not_overridden()
    }
    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        not_overridden()
    }
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        not_overridden()
    }
    fn set_default_account_provider(&self, _provider: Rc<dyn IDefaultAccountProvider>) -> Result<(), Error> {
        not_overridden()
    }
    fn refresh(&self, _options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        not_overridden()
    }
    fn sign_in(&self, _options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        not_overridden()
    }
    fn sign_out(&self) -> Promise<()> {
        not_overridden()
    }
    fn resolve_git_hub_url(&self, _path: &str) -> Option<String> {
        not_overridden()
    }
}

/// `new class extends mock<IUpdateService>() { state; onStateChange }`.
struct StubUpdateService {
    state: RefCell<State>,
    changed: Rc<Emitter<State>>,
}

impl UpdateService for StubUpdateService {
    fn on_state_change(&self) -> Event<State> {
        self.changed.event()
    }
    fn state(&self) -> State {
        self.state.borrow().clone()
    }
    fn check_for_updates(&self, _explicit: bool) -> Promise<()> {
        not_overridden()
    }
    fn download_update(&self, _explicit: bool) -> Promise<()> {
        not_overridden()
    }
    fn apply_update(&self) -> Promise<()> {
        not_overridden()
    }
    fn quit_and_install(&self) -> Promise<()> {
        not_overridden()
    }
    fn is_latest_version(&self) -> Promise<Option<bool>> {
        not_overridden()
    }
    fn apply_specific_update(&self, _package_path: &str) -> Promise<()> {
        not_overridden()
    }
    fn set_internal_org(&self, _internal_org: Option<&str>) -> Promise<()> {
        not_overridden()
    }
}

/// `new class extends mock<IBannerService>() { show; hide }`.
struct StubBannerService {
    on_show: Box<dyn Fn(IBannerItem)>,
    on_hide: Box<dyn Fn(&str)>,
}

impl StubBannerService {
    fn new(on_show: impl Fn(IBannerItem) + 'static, on_hide: impl Fn(&str) + 'static) -> Rc<Self> {
        Rc::new(Self { on_show: Box::new(on_show), on_hide: Box::new(on_hide) })
    }
}

impl IBannerService for StubBannerService {
    fn focus(&self) {
        not_overridden()
    }
    fn focus_next_action(&self) {
        not_overridden()
    }
    fn focus_previous_action(&self) {
        not_overridden()
    }
    fn hide(&self, id: &str) {
        (self.on_hide)(id);
    }
    fn show(&self, item: IBannerItem) {
        (self.on_show)(item);
    }
}

type Compat = IManagedSettingsCompatibilityError;

fn editor_environment() -> Rc<dyn IWorkbenchEnvironmentService> {
    environment(false)
}

/// `new class extends mock<IWorkbenchEnvironmentService>() { override readonly isSessionsWindow = ... }`.
fn environment(is_sessions_window: bool) -> Rc<dyn IWorkbenchEnvironmentService> {
    TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window, ..EnvironmentOverrides::default() })
}

/// `product`: `nameShort = 'Code - Insiders'`, `version = '1.140.0'`.
fn product() -> Rc<dyn IProductService> {
    Rc::new(ProductService { name_short: "Code - Insiders".to_owned(), version: "1.140.0".to_owned(), ..ProductService::default() })
}

/// `error`.
fn error() -> Compat {
    IManagedSettingsCompatibilityError {
        error_code: "client_update_required",
        client_version: Some(JsString::from("1.139.0")),
        minimum_client_version: Some(JsString::from("1.141.0")),
    }
}

/// `{ ...error, minimumClientVersion }`.
fn error_with_minimum(minimum_client_version: &str) -> Compat {
    IManagedSettingsCompatibilityError { minimum_client_version: Some(JsString::from(minimum_client_version)), ..error() }
}

/// `{ errorCode: 'client_update_required' }`.
fn bare_error() -> Compat {
    IManagedSettingsCompatibilityError { error_code: "client_update_required", client_version: None, minimum_client_version: None }
}

/// `{ errorCode: 'client_update_required', minimumClientVersion: '1.141.0' }`.
fn required_only_error() -> Compat {
    IManagedSettingsCompatibilityError { error_code: "client_update_required", client_version: None, minimum_client_version: Some(JsString::from("1.141.0")) }
}

/// `idle`.
fn idle() -> State {
    State::idle(UpdateType::Archive, None, None)
}

/// `update`.
fn update() -> Update {
    Update { version: "1.141.0".to_owned(), product_version: Some("1.141.0".to_owned()), timestamp: None, url: None, sha256hash: None }
}

fn ready() -> State {
    State::ready(update(), true, false)
}

const DISABLED_REASONS: [DisablementReason; 7] = [
    DisablementReason::NotBuilt,
    DisablementReason::DisabledByEnvironment,
    DisablementReason::ManuallyDisabled,
    DisablementReason::Policy,
    DisablementReason::MissingConfiguration,
    DisablementReason::InvalidConfiguration,
    DisablementReason::RunningAsAdmin,
];

const POLICY_DISABLED_STATUS: &str = "Built-in updates are disabled by your organization. Contact your administrator for an approved update.";

fn action(label: &str, href: &str) -> Option<IManagedSettingsUpdateAction> {
    Some(IManagedSettingsUpdateAction { label: label.to_owned(), href: href.to_owned() })
}

/// O que `createService` devolve: o serviço e os dois setters.
struct Fixture {
    service: Rc<ManagedSettingsUpdateService>,
    account: Rc<StubDefaultAccountService>,
    updater: Rc<StubUpdateService>,
}

impl Fixture {
    /// `createService(initialError)`.
    fn new(guard: &LeakGuard, initial_error: Option<Compat>) -> Self {
        let changed = guard.add_rc(Emitter::new());
        let update_changed = guard.add_rc(Emitter::new());
        let account = Rc::new(StubDefaultAccountService { error: RefCell::new(initial_error), changed });
        let updater = Rc::new(StubUpdateService { state: RefCell::new(idle()), changed: update_changed });
        let service = guard.add_rc(ManagedSettingsUpdateService::new(account.clone(), product(), updater.clone()));
        Self { service, account, updater }
    }

    /// `setError(value)`.
    fn set_error(&self, value: Option<Compat>) {
        *self.account.error.borrow_mut() = value.clone();
        self.account.changed.fire(value);
    }

    /// `setUpdate(value)`.
    fn set_update(&self, value: State) {
        *self.updater.state.borrow_mut() = value.clone();
        self.updater.changed.fire(value);
    }
}

/// `ManagedSettingsUpdateRequiredContext.getValue(contexts)`.
fn visible(contexts: &MockContextKeyService) -> Option<bool> {
    match managed_settings_update_required_context().get_value(contexts) {
        Dynamic::Bool(value) => Some(value),
        _ => None,
    }
}

/// `new TestConfigurationService()` com o emissor no guarda.
fn configuration(guard: &LeakGuard) -> Rc<TestConfigurationService> {
    let configuration = TestConfigurationService::new(None);
    guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
    configuration
}

/// `new TestInstantiationService()` com `stub(IManagedSettingsUpdateService, service)` e `stub(IWorkbenchEnvironmentService, environment)`.
fn help_services(guard: &LeakGuard, service: &Rc<ManagedSettingsUpdateService>, environment: Rc<dyn IWorkbenchEnvironmentService>) -> Rc<InstantiationService> {
    let update_service: Rc<dyn IManagedSettingsUpdateService> = service.clone();
    guard.add_rc(InstantiationService::new(ServiceCollection::new().with(I_MANAGED_SETTINGS_UPDATE_SERVICE, update_service).with(I_WORKBENCH_ENVIRONMENT_SERVICE, environment)))
}

/// `AccessibleViewRegistry.getImplementations().find(implementation => implementation.name === 'managedSettingsUpdate')!`.
fn find_help(guard: &LeakGuard, ui: &Ui) -> Rc<dyn IAccessibleViewImplementation> {
    guard.add(register_accessible_view(ui.clone()));
    get_implementations().into_iter().find(|implementation| implementation.name() == "managedSettingsUpdate").expect("a ajuda do bloqueio está registrada")
}

/// `services.invokeFunction(accessor => help.getProvider(accessor))`.
fn get_provider(services: &InstantiationService, help: &Rc<dyn IAccessibleViewImplementation>) -> Option<Rc<AccessibleContentProvider>> {
    services.invoke_function(|accessor| help.get_provider(accessor)).map(|provider| match provider {
        AccessibleViewContentProvider::Content(provider) => provider,
        AccessibleViewContentProvider::Extension(_) => panic!("a ajuda do bloqueio devolve um AccessibleContentProvider"),
    })
}

#[test]
fn identifies_the_requesting_product_and_handles_missing_version_metadata() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let no_version_product = ProductService { name_short: "Code".to_owned(), version: String::new(), ..ProductService::default() };
    let known = get_managed_settings_update_info(&error(), &*product(), &idle());
    let fallback = get_managed_settings_update_info(&bare_error(), &*product(), &idle()).message;
    let required_only = get_managed_settings_update_info(&required_only_error(), &no_version_product, &idle()).message;
    let unavailable = get_managed_settings_update_info(&bare_error(), &no_version_product, &idle()).detail;

    assert_eq!(
        (known, fallback, required_only, unavailable),
        (
            IManagedSettingsUpdateInfo {
                title: "Update required by your organization".to_owned(),
                message: "Your organization requires Code - Insiders 1.141.0 or later to use AI features.".to_owned(),
                detail: Some("Installed: 1.139.0".to_owned()),
                action: action("Check for Updates", "command:update.checkForUpdate"),
                update_status: None,
            },
            "Your organization requires an update to Code - Insiders to use AI features.".to_owned(),
            "Your organization requires Code 1.141.0 or later to use AI features.".to_owned(),
            None,
        )
    );
}

#[test]
fn uses_actionable_update_states_without_pretending_disabled_or_busy_updates_can_run() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let states = [
        idle(),
        State::available_for_download(update(), None),
        State::downloaded(update(), true, false),
        ready(),
        State::disabled(DisablementReason::Policy),
        State::disabled(DisablementReason::NotBuilt),
        State::downloading(Some(update()), true, false, None, None, None),
    ];
    let results: Vec<_> = states
        .iter()
        .map(|state| {
            let info = get_managed_settings_update_info(&error(), &*product(), state);
            (info.action, info.update_status)
        })
        .collect();

    assert_eq!(
        results,
        vec![
            (action("Check for Updates", "command:update.checkForUpdate"), None),
            (action("Download Update", "command:update.downloadUpdate"), None),
            (action("Install Update", "command:update.installUpdate"), None),
            (action("Restart to Update", "command:update.restartToUpdate"), None),
            (None, Some(POLICY_DISABLED_STATUS.to_owned())),
            (None, None),
            (None, Some("An update operation is in progress.".to_owned())),
        ]
    );
}

#[test]
fn all_disabled_updater_reasons_omit_fallback_actions_and_only_policy_attributes_management_to_the_organization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let results: Vec<_> = DISABLED_REASONS
        .iter()
        .map(|&reason| {
            let info = get_managed_settings_update_info(&error(), &*product(), &State::disabled(reason));
            (reason, info.action, info.update_status)
        })
        .collect();
    let expected: Vec<_> = DISABLED_REASONS
        .iter()
        .map(|&reason| (reason, None, (reason == DisablementReason::Policy).then(|| POLICY_DISABLED_STATUS.to_owned())))
        .collect();

    assert_eq!(results, expected);
}

#[test]
fn reads_initially_blocked_state_and_recomputes_late_changes_removal_and_reapplication_without_duplicates() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let seen: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    let update_info = fixture.service.update_info();
    guard.add(autorun({
        let seen = seen.clone();
        move |reader| {
            let info = update_info.read(reader);
            seen.borrow_mut().push(info.map(|info| format!("{} {} {}", info.message, info.detail.unwrap_or_else(|| "undefined".to_owned()), info.action.map_or_else(|| "undefined".to_owned(), |action| action.label))));
        }
    }));
    fixture.set_error(Some(error()));
    fixture.set_update(ready());
    fixture.set_error(None);
    fixture.set_update(idle());
    fixture.set_error(Some(error_with_minimum("1.142.0")));
    fixture.set_error(None);

    assert_eq!(
        *seen.borrow(),
        vec![
            Some("Your organization requires Code - Insiders 1.141.0 or later to use AI features. Installed: 1.139.0 Check for Updates".to_owned()),
            Some("Your organization requires Code - Insiders 1.141.0 or later to use AI features. Installed: 1.139.0 Restart to Update".to_owned()),
            None,
            Some("Your organization requires Code - Insiders 1.142.0 or later to use AI features. Installed: 1.139.0 Check for Updates".to_owned()),
            None,
        ]
    );
}

#[test]
fn does_not_infer_min_version_from_other_restrictions_or_update_availability() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, None);
    let seen: Rc<RefCell<Vec<Option<IManagedSettingsUpdateInfo>>>> = Rc::default();
    let update_info = fixture.service.update_info();
    guard.add(autorun({
        let seen = seen.clone();
        move |reader| seen.borrow_mut().push(update_info.read(reader))
    }));
    fixture.set_update(ready());

    assert_eq!(*seen.borrow(), vec![None]);
}

#[test]
fn banner_and_read_only_view_context_follow_late_compatibility_and_honor_personal_ai_hiding() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, None);
    let configuration = configuration(&guard);
    let contexts = MockContextKeyService::new();
    let shown: Rc<RefCell<Vec<IBannerItem>>> = Rc::default();
    let active: Rc<RefCell<Option<IBannerItem>>> = Rc::default();
    let banners = StubBannerService::new(
        {
            let (shown, active) = (shown.clone(), active.clone());
            move |item| {
                shown.borrow_mut().push(item.clone());
                *active.borrow_mut() = Some(item);
            }
        },
        {
            let active = active.clone();
            move |id| {
                let matches = active.borrow().as_ref().is_some_and(|item| item.id == id);
                if matches {
                    *active.borrow_mut() = None;
                }
            }
        },
    );
    let contribution = guard.add_rc(ManagedSettingsUpdateContribution::new(
        fixture.service.clone(),
        configuration.clone(),
        contexts.clone(),
        banners,
        editor_environment(),
    ));
    let states: Rc<RefCell<Vec<(Option<bool>, bool)>>> = Rc::default();
    let capture = {
        let (states, contexts, active) = (states.clone(), contexts.clone(), active.clone());
        move || states.borrow_mut().push((visible(&contexts), active.borrow().is_some()))
    };
    capture();
    fixture.set_error(Some(error()));
    capture();
    fixture.set_error(Some(error()));
    let on_close = active.borrow().as_ref().and_then(|item| item.on_close.clone());
    if let Some(on_close) = on_close {
        on_close();
    }
    *active.borrow_mut() = None;
    fixture.set_update(ready());
    capture();
    fixture.set_error(None);
    capture();
    fixture.set_error(Some(error()));
    capture();
    let _ = configuration.set_user_configuration(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(true), None);
    fire_configuration_change(&configuration, &[CHAT_AI_DISABLED_SETTING_ID]);
    capture();
    contribution.dispose();
    capture();

    let base_message = get_managed_settings_update_info(&error(), &*product(), &idle()).message;
    let shown_view: Vec<_> = shown
        .borrow()
        .iter()
        .map(|item| (item.neutral, item.message.to_text(), item.actions.as_ref().and_then(|actions| actions.first()).map(|action| action.label.clone())))
        .collect();
    assert_eq!(
        (states.borrow().clone(), shown_view),
        (
            vec![
                (Some(false), false),
                (Some(true), true),
                (Some(true), false),
                (Some(false), false),
                (Some(true), true),
                (Some(false), false),
                (Some(false), false),
            ],
            vec![
                (Some(true), base_message.clone(), Some("Check for Updates".to_owned())),
                (Some(true), base_message, Some("Restart to Update".to_owned())),
            ],
        )
    );
}

#[test]
fn initially_blocked_editor_publishes_the_banner_without_opening_a_chat_view() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let context = MockContextKeyService::new();
    let count = Rc::new(RefCell::new(0));
    let configuration = configuration(&guard);
    guard.add_rc(ManagedSettingsUpdateContribution::new(
        fixture.service.clone(),
        configuration,
        context.clone(),
        StubBannerService::new({ let count = count.clone(); move |_| *count.borrow_mut() += 1 }, |_| {}),
        editor_environment(),
    ));

    assert_eq!((*count.borrow(), visible(&context)), (1, Some(true)));
}

#[test]
fn banner_explicitly_identifies_organization_managed_updates_but_does_not_attribute_dev_builds_to_an_administrator() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let shown: Rc<RefCell<Vec<String>>> = Rc::default();
    let configuration = configuration(&guard);
    guard.add_rc(ManagedSettingsUpdateContribution::new(
        fixture.service.clone(),
        configuration,
        MockContextKeyService::new(),
        StubBannerService::new({ let shown = shown.clone(); move |item| shown.borrow_mut().push(item.message.to_text()) }, |_| {}),
        editor_environment(),
    ));
    fixture.set_update(State::disabled(DisablementReason::Policy));
    fixture.set_update(State::disabled(DisablementReason::NotBuilt));

    assert_eq!(
        *shown.borrow(),
        vec![
            "Your organization requires Code - Insiders 1.141.0 or later to use AI features.".to_owned(),
            format!("Your organization requires Code - Insiders 1.141.0 or later to use AI features. {POLICY_DISABLED_STATUS}"),
            "Your organization requires Code - Insiders 1.141.0 or later to use AI features.".to_owned(),
        ]
    );
}

#[test]
fn banner_remains_informative_without_a_fallback_link_for_every_disabled_updater_reason() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let configuration = configuration(&guard);
    let active: Rc<RefCell<Option<IBannerItem>>> = Rc::default();
    guard.add_rc(ManagedSettingsUpdateContribution::new(
        fixture.service.clone(),
        configuration,
        MockContextKeyService::new(),
        StubBannerService::new(
            {
                let active = active.clone();
                move |item| *active.borrow_mut() = Some(item)
            },
            {
                let active = active.clone();
                move |_| *active.borrow_mut() = None
            },
        ),
        editor_environment(),
    ));
    let states: Vec<_> = DISABLED_REASONS
        .iter()
        .map(|&reason| {
            fixture.set_update(State::disabled(reason));
            let active = active.borrow();
            (
                active.is_some(),
                active.as_ref().and_then(|item| item.actions.clone()),
                active.as_ref().is_some_and(|item| item.message.to_text().contains("Update Instructions")),
            )
        })
        .collect();

    assert_eq!(states, DISABLED_REASONS.iter().map(|_| (true, Some(Vec::new()), false)).collect::<Vec<_>>());
}

#[test]
fn agents_keeps_its_update_context_without_touching_banners_on_startup_late_changes_or_recovery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let configuration = configuration(&guard);
    let contexts = MockContextKeyService::new();
    let banner_calls: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let contribution = guard.add_rc(ManagedSettingsUpdateContribution::new(
        fixture.service.clone(),
        configuration,
        contexts.clone(),
        StubBannerService::new(
            {
                let banner_calls = banner_calls.clone();
                move |_| banner_calls.borrow_mut().push("show")
            },
            {
                let banner_calls = banner_calls.clone();
                move |_| banner_calls.borrow_mut().push("hide")
            },
        ),
        environment(true),
    ));
    let mut states = vec![visible(&contexts)];
    fixture.set_update(State::disabled(DisablementReason::Policy));
    states.push(visible(&contexts));
    fixture.set_error(None);
    states.push(visible(&contexts));
    fixture.set_error(Some(error()));
    states.push(visible(&contexts));
    contribution.dispose();
    states.push(visible(&contexts));

    assert_eq!(
        (banner_calls.borrow().clone(), states),
        (Vec::new(), vec![Some(true), Some(true), Some(false), Some(true), Some(false)])
    );
}

#[test]
fn accessibility_help_uses_the_concise_requirement_and_restores_focus_without_claiming_a_usable_composer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let services = help_services(&guard, &fixture.service, editor_environment());
    let ui = Ui::new();
    let root = ui.root();
    let target = ui.tree_mut().create(root, Role::Button);
    guard.add(to_disposable({
        let ui = ui.clone();
        move || ui.remove(target)
    }));
    ui.focus(target);
    let help = find_help(&guard, &ui);
    let provider = guard.add_rc(get_provider(&services, &help).expect("há bloqueio ativo"));
    let text = provider.provide_content();
    ui.blur();
    (provider.on_close)();
    fixture.set_error(None);

    assert_eq!(
        (
            text.contains("Update required by your organization"),
            text.contains("Your organization requires Code - Insiders 1.141.0 or later to use AI features."),
            text.contains("Installed: 1.139.0"),
            text.contains("Use Tab or Shift+Tab"),
            text.contains("Chat is read-only"),
            text.contains("Focus Banner"),
            ui.focused() == Some(target),
            get_provider(&services, &help).is_none(),
        ),
        (true, true, true, true, true, true, true, true)
    );
}

#[test]
fn agents_accessibility_help_describes_recovery_without_suggesting_a_nonexistent_banner() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new(&guard, Some(error()));
    let services = help_services(&guard, &fixture.service, environment(true));
    let help = find_help(&guard, &Ui::new());
    let provider = guard.add_rc(get_provider(&services, &help).expect("há bloqueio ativo"));
    let text = provider.provide_content();

    assert_eq!(
        (
            text.contains("Your organization requires Code - Insiders 1.141.0 or later to use AI features."),
            text.contains("Use Tab or Shift+Tab"),
            text.contains("banner"),
            text.contains("The Agents window is blocked") && text.contains("The overlay explains"),
            text.contains("Chat is read-only"),
        ),
        (true, true, false, true, false)
    );
}

#[test]
fn accessibility_help_omits_nonexistent_update_actions_when_updates_are_disabled() {
    for is_sessions_window in [false, true] {
        // `accessibility help omits nonexistent update actions in ${isSessionsWindow ? 'Agents' : 'editor'} when updates are disabled`
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = Fixture::new(&guard, Some(error()));
        fixture.set_update(State::disabled(DisablementReason::Policy));
        let services = help_services(&guard, &fixture.service, environment(is_sessions_window));
        let help = find_help(&guard, &Ui::new());
        let provider = guard.add_rc(get_provider(&services, &help).expect("há bloqueio ativo"));
        let text = provider.provide_content();

        assert_eq!(
            (
                text.contains("The available update action is"),
                text.contains("reach Open Editor Window"),
                text.contains("Contact your administrator for an approved update."),
            ),
            (false, is_sessions_window, true)
        );
    }
}

#[test]
fn help_describes_the_overlay_or_notice_and_only_currently_available_actions() {
    for is_sessions_window in [false, true] {
        // `help describes the ${isSessionsWindow ? 'Agents overlay' : 'Chat notice'} and only currently available actions`
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = Fixture::new(&guard, Some(error()));
        let services = help_services(&guard, &fixture.service, environment(is_sessions_window));
        let help = find_help(&guard, &Ui::new());
        let states = [idle(), State::downloading(Some(update()), true, false, None, None, None), State::Uninitialized, State::disabled(DisablementReason::Policy)];
        let results: Vec<_> = states
            .iter()
            .map(|state| {
                fixture.set_update(state.clone());
                let provider = guard.add_rc(get_provider(&services, &help).expect("há bloqueio ativo"));
                let text = provider.provide_content();
                (
                    text.contains("The Agents window is blocked"),
                    text.contains("Chat is read-only"),
                    text.contains("move between available actions"),
                    text.contains("The available update action is Check for Updates."),
                    text.contains("reach Open Editor Window"),
                    text.contains("Focus Banner"),
                )
            })
            .collect();
        let expected: Vec<_> = (0..states.len()).map(|index| (is_sessions_window, !is_sessions_window, true, index == 0, is_sessions_window, !is_sessions_window)).collect();

        assert_eq!(results, expected);
    }
}
