//! Porte de `src/vs/platform/policy/test/node/nativeManagedSettingsService.test.ts`, caso a caso e na mesma
//! ordem.
//!
//! Divergências: o módulo nativo `@vscode/policy-watcher` é a [`NativePolicyWatcherFactory`] injetada (como o
//! original faz), e `Disposable.None` é `lifecycle::none()`. As definições de política são `PolicyDefinition`s
//! com os `managedSettings` como valor dinâmico. Os resultados são `IndexMap`s comparados sem ordem, como o
//! `deepStrictEqual`; as definições observadas (`policies`) são comparadas pelo `type` de cada chave
//! (`NativePolicyDefinitions`). O laço sobre as folhas de captura de telemetria vira cinco testes, um por folha.
//! Cada teste roda sob o relógio virtual (`async_test`), e o `DeferredManagedSettingsChannel` fala `Marshalled`, o
//! valor de canal do porte: o que o original passa como objeto passa pelas conversões de
//! `file_managed_settings_ipc`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::async_test;
use common::{j, obj, s};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, Disposable, DisposableStore, IDisposable, plain_disposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::IChannel;
use je_base::policy::{ManagedSettingType, PolicyValue};
use je_base::promise::{Promise, Resolver};
use je_platform::log::NullLogService;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, COPILOT_OTEL_CAPTURE_IDENTITY_KEY, COPILOT_STRICT_MARKETPLACES_KEY, INativeManagedSettingsService,
    MANAGED_SETTINGS_CONTROL_DEFINITIONS, ManagedSettingsData, normalize_managed_settings, pick_managed_settings,
};
use je_platform::policy::file_managed_settings_ipc::managed_settings_to_marshalled;
use je_platform::policy::native_managed_settings_ipc::NativeManagedSettingsChannelClient;
use je_platform::policy::native_managed_settings_service::{
    NativeManagedSettingsService, NativePolicyCallback, NativePolicyDefinitions, NativePolicyUpdate, NativePolicyWatcherFactory,
};
use je_platform::policy::policy::{PolicyDefinition, PolicyDefinitionRef, PolicyDefinitionType, PolicyDefinitions};

const POLICY_NAME: &str = "ChatToolsAutoApprove";

fn text(value: &str) -> PolicyValue {
    PolicyValue::String(value.to_owned())
}

/// Um `ManagedSettingsData` literal.
fn managed(entries: Vec<(&str, PolicyValue)>) -> ManagedSettingsData {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

/// Um `Record<string, PolicyValue | undefined>` literal.
fn update(entries: Vec<(&str, Option<PolicyValue>)>) -> NativePolicyUpdate {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

/// As chaves que o vigia deve observar: as dadas e os controles de transporte (`...MANAGED_SETTINGS_CONTROL_DEFINITIONS`).
fn watched(extra: Vec<(&str, ManagedSettingType)>) -> NativePolicyDefinitions {
    let mut policies: NativePolicyDefinitions = extra.into_iter().map(|(key, type_)| (key.to_owned(), type_)).collect();
    policies.extend(MANAGED_SETTINGS_CONTROL_DEFINITIONS.iter().map(|(key, definition)| (key.clone(), definition.r#type)));
    policies
}

/// `{ type }` sem `managedSettings`.
fn plain_policy(type_: PolicyDefinitionType) -> PolicyDefinitionRef {
    Rc::new(PolicyDefinition::new(type_))
}

/// `{ type, managedSettings: { [key]: { type } } }`.
fn managed_policy(type_: PolicyDefinitionType, managed_settings: &[(&str, &str)]) -> PolicyDefinitionRef {
    let mut definition = PolicyDefinition::new(type_);
    definition.managed_settings = Some(obj(managed_settings.iter().map(|(key, setting_type)| (*key, obj(vec![("type", s(setting_type))]))).collect::<Vec<_>>()));
    Rc::new(definition)
}

fn policies(entries: Vec<(&str, PolicyDefinitionRef)>) -> PolicyDefinitions {
    entries.into_iter().map(|(name, definition)| (name.to_owned(), definition)).collect()
}

/// O callback `onDidChange` que o vigia recebeu (`onDidChange` do original).
type CallbackSlot = Rc<RefCell<Option<NativePolicyCallback>>>;

/// `onDidChange?.(update)`.
fn fire_native(slot: &CallbackSlot, entries: Vec<(&str, Option<PolicyValue>)>) {
    let callback = slot.borrow().clone().expect("o vigia foi criado");
    callback(update(entries));
}

/// Uma fábrica que confere o que foi observado (`on_create`), guarda o callback e o chama com `initial`
/// (`callback({...})`), devolvendo `Disposable.None`.
fn factory(slot: &CallbackSlot, initial: Vec<(&str, Option<PolicyValue>)>, on_create: impl Fn(&NativePolicyDefinitions) + 'static) -> NativePolicyWatcherFactory {
    let slot = slot.clone();
    let initial = update(initial);
    NativePolicyWatcherFactory::new(move |_product_name, policies, callback, _options| {
        on_create(policies);
        *slot.borrow_mut() = Some(callback.clone());
        callback(initial.clone());
        Ok(lifecycle::none())
    })
}

fn new_service(d: &Rc<DisposableStore>, watcher_factory: NativePolicyWatcherFactory) -> Rc<NativeManagedSettingsService> {
    d.add_rc(NativeManagedSettingsService::new(NullLogService::shared(), "com.github.copilot", None, watcher_factory))
}

/// `pickManagedSettings(native, server, file).values`.
fn pick_values(native: &ManagedSettingsData, server: Option<&ManagedSettingsData>, file: Option<&ManagedSettingsData>) -> ManagedSettingsData {
    pick_managed_settings(Some(native), server, file).values
}

#[test]
fn watches_managed_settings_keys_from_policy_definitions_and_exposes_raw_managed_settings() {
    async_test(|d| async move {
        let slot = CallbackSlot::default();
        let watcher_factory = factory(&slot, vec![], |policies| {
            assert_eq!(*policies, watched(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, ManagedSettingType::String)]));
        });

        let service = new_service(&d, watcher_factory);
        service
            .update_policy_definitions(policies(vec![(POLICY_NAME, managed_policy(PolicyDefinitionType::Boolean, &[(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, "string")]))]))
            .await
            .unwrap();

        fire_native(&slot, vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, Some(text("disable")))]);
        assert_eq!(service.managed_settings(), managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));

        fire_native(&slot, vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, Some(text("enable")))]);
        assert_eq!(service.managed_settings(), managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable"))]));
    });
}

#[test]
fn watches_transport_controls_without_a_managed_settings_policy_definition() {
    async_test(|d| async move {
        let watched_settings: Rc<RefCell<NativePolicyDefinitions>> = Rc::default();
        let slot = CallbackSlot::default();
        let watcher_factory = {
            let watched_settings = watched_settings.clone();
            factory(&slot, vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, Some(PolicyValue::Boolean(true)))], move |policies| *watched_settings.borrow_mut() = policies.clone())
        };

        let service = new_service(&d, watcher_factory);
        service.update_policy_definitions(policies(vec![("OtherPolicy", plain_policy(PolicyDefinitionType::Boolean))])).await.unwrap();

        assert_eq!(
            (watched_settings.borrow().clone(), service.managed_settings()),
            (watched(vec![]), managed(vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(true))]))
        );
    });
}

#[test]
fn applies_native_telemetry_file_path_updates_and_removals_without_watcher_metadata() {
    async_test(|d| async move {
        let file_path = "telemetry.filePath";
        let slot = CallbackSlot::default();
        let service = new_service(&d, factory(&slot, vec![], |_| {}));

        service.update_policy_definitions(policies(vec![("TelemetryFilePath", managed_policy(PolicyDefinitionType::String, &[(file_path, "string")]))])).await.unwrap();
        fire_native(&slot, vec![(file_path, Some(text("/managed/copilot.jsonl")))]);
        let updated = service.managed_settings();
        fire_native(&slot, vec![(file_path, None)]);

        assert_eq!((updated, service.managed_settings()), (managed(vec![(file_path, text("/managed/copilot.jsonl"))]), managed(vec![])));
    });
}

#[test]
fn observes_strict_marketplace_policy_without_a_configuration_registration_and_preserves_channel_precedence() {
    async_test(|d| async move {
        let key = COPILOT_STRICT_MARKETPLACES_KEY;
        let slot = CallbackSlot::default();
        let watcher_factory = factory(&slot, vec![(key, Some(text("[]")))], move |policies| {
            assert_eq!(policies.get(key), Some(&ManagedSettingType::String));
        });
        let service = new_service(&d, watcher_factory);
        service.initialize().await.unwrap();
        let server = normalize_managed_settings(&j("{ 'strictKnownMarketplaces': [{ 'source': 'github', 'repo': 'server/catalog' }] }"), None);
        let file = normalize_managed_settings(&j("{ 'strictKnownMarketplaces': [{ 'source': 'github', 'repo': 'file/catalog' }] }"), None);
        let native = pick_values(&service.managed_settings(), Some(&server), Some(&file));
        fire_native(&slot, vec![(key, None)]);
        let empty = ManagedSettingsData::new();
        assert_eq!(
            (
                native,
                pick_values(&service.managed_settings(), Some(&server), Some(&file)),
                pick_values(&service.managed_settings(), Some(&empty), Some(&file)),
                pick_values(&service.managed_settings(), Some(&empty), Some(&empty)),
            ),
            (managed(vec![(key, text("[]"))]), server.clone(), file.clone(), managed(vec![]))
        );
    });
}

/// `native capture.${leaf} selects telemetry without a corresponding Local capture policy`.
fn native_capture_selects_telemetry_without_a_local_capture_policy(leaf: &str) {
    let key = format!("telemetry.capture.{leaf}");
    async_test(|d| async move {
        let slot = CallbackSlot::default();
        let expected_key = key.clone();
        let watcher_factory = factory(&slot, vec![(key.as_str(), Some(PolicyValue::Boolean(false)))], move |policies| {
            assert_eq!(policies.get(&expected_key), Some(&ManagedSettingType::Boolean));
        });
        let service = new_service(&d, watcher_factory);
        service.initialize().await.unwrap();
        let values: Vec<ManagedSettingsData> = [false, true]
            .into_iter()
            .map(|identity| pick_values(&service.managed_settings(), Some(&managed(vec![(COPILOT_OTEL_CAPTURE_IDENTITY_KEY, PolicyValue::Boolean(identity))])), None))
            .collect();
        assert_eq!(values, vec![managed(vec![(key.as_str(), PolicyValue::Boolean(false))]), managed(vec![(key.as_str(), PolicyValue::Boolean(false))])]);
    });
}

#[test]
fn native_capture_prompts_selects_telemetry_without_a_corresponding_local_capture_policy() {
    native_capture_selects_telemetry_without_a_local_capture_policy("prompts");
}

#[test]
fn native_capture_responses_selects_telemetry_without_a_corresponding_local_capture_policy() {
    native_capture_selects_telemetry_without_a_local_capture_policy("responses");
}

#[test]
fn native_capture_tool_arguments_selects_telemetry_without_a_corresponding_local_capture_policy() {
    native_capture_selects_telemetry_without_a_local_capture_policy("toolArguments");
}

#[test]
fn native_capture_tool_output_selects_telemetry_without_a_corresponding_local_capture_policy() {
    native_capture_selects_telemetry_without_a_local_capture_policy("toolOutput");
}

#[test]
fn native_capture_policy_detail_selects_telemetry_without_a_corresponding_local_capture_policy() {
    native_capture_selects_telemetry_without_a_local_capture_policy("policyDetail");
}

#[test]
fn without_observed_declared_telemetry_keys_native_delivery_leaves_the_server_block_active() {
    async_test(|d| async move {
        let slot = CallbackSlot::default();
        let key = "telemetry.capture.prompts";
        let watcher_factory = factory(&slot, vec![], move |policies| {
            assert_eq!(
                (policies.get("telemetry").copied(), policies.get("telemetry.futureControl").copied(), policies.get(key).copied()),
                (None, None, Some(ManagedSettingType::Boolean))
            );
        });
        let service = new_service(&d, watcher_factory);
        service.initialize().await.unwrap();
        let server = managed(vec![(COPILOT_OTEL_CAPTURE_IDENTITY_KEY, PolicyValue::Boolean(true))]);
        let before = pick_values(&service.managed_settings(), Some(&server), None);
        fire_native(&slot, vec![(key, Some(PolicyValue::Boolean(false)))]);
        let during = pick_values(&service.managed_settings(), Some(&server), None);
        fire_native(&slot, vec![(key, None)]);
        let after = pick_values(&service.managed_settings(), Some(&server), None);
        assert_eq!((before, during, after), (server.clone(), managed(vec![(key, PolicyValue::Boolean(false))]), server));
    });
}

#[test]
fn watches_all_current_runtime_permission_controls_before_configuration_policies_register() {
    async_test(|d| async move {
        let expected: NativePolicyDefinitions = [
            ("permissions.model", ManagedSettingType::String),
            ("permissions.disableBypassPermissionsMode", ManagedSettingType::String),
            ("permissions.disableAssistedPermissionsMode", ManagedSettingType::Boolean),
            ("permissions.defaultMode", ManagedSettingType::String),
            ("permissions.ask", ManagedSettingType::String),
            ("permissions.allow", ManagedSettingType::String),
            ("permissions.deny", ManagedSettingType::String),
            ("permissions.limitTo", ManagedSettingType::String),
        ]
        .into_iter()
        .map(|(key, type_)| (key.to_owned(), type_))
        .collect();
        let watched_permissions: Rc<RefCell<NativePolicyDefinitions>> = Rc::default();
        let slot = CallbackSlot::default();
        let watcher_factory = {
            let watched_permissions = watched_permissions.clone();
            factory(
                &slot,
                vec![("permissions.disableAssistedPermissionsMode", Some(PolicyValue::Boolean(false))), ("permissions.defaultMode", Some(text("manual")))],
                move |policies| *watched_permissions.borrow_mut() = policies.iter().filter(|(key, _)| key.starts_with("permissions.")).map(|(key, type_)| (key.clone(), *type_)).collect(),
            )
        };
        let service = new_service(&d, watcher_factory);
        service.initialize().await.unwrap();
        service.update_policy_definitions(vec![]).await.unwrap();
        assert_eq!(
            (watched_permissions.borrow().clone(), service.managed_settings()),
            (expected, managed(vec![("permissions.disableAssistedPermissionsMode", PolicyValue::Boolean(false)), ("permissions.defaultMode", text("manual"))]))
        );
    });
}

#[test]
fn clears_stale_watcher_values_when_managed_settings_definitions_are_removed() {
    async_test(|d| async move {
        let removable_key = "test.removableSetting";
        let slot = CallbackSlot::default();
        let dispose_count = Rc::new(Cell::new(0));
        let watcher_factory = {
            let (slot, dispose_count) = (slot.clone(), dispose_count.clone());
            NativePolicyWatcherFactory::new(move |_product_name, _policies, callback, _options| {
                *slot.borrow_mut() = Some(callback.clone());
                callback(NativePolicyUpdate::new());
                let dispose_count = dispose_count.clone();
                Ok(plain_disposable(move || dispose_count.set(dispose_count.get() + 1)))
            })
        };

        let service = new_service(&d, watcher_factory);
        service.update_policy_definitions(policies(vec![(POLICY_NAME, managed_policy(PolicyDefinitionType::Boolean, &[(removable_key, "string")]))])).await.unwrap();

        fire_native(&slot, vec![(removable_key, Some(text("value")))]);
        service.update_policy_definitions(vec![]).await.unwrap();

        assert_eq!((service.managed_settings(), dispose_count.get()), (managed(vec![]), 1));
    });
}

#[test]
fn keeps_raw_managed_settings_while_definitions_are_unchanged() {
    async_test(|d| async move {
        let watcher_create_count = Rc::new(Cell::new(0));
        let slot = CallbackSlot::default();
        let watcher_factory = {
            let watcher_create_count = watcher_create_count.clone();
            factory(&slot, vec![], move |_| watcher_create_count.set(watcher_create_count.get() + 1))
        };

        let service = new_service(&d, watcher_factory);
        let bypass_policy = || managed_policy(PolicyDefinitionType::Boolean, &[(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, "string")]);
        service.update_policy_definitions(policies(vec![(POLICY_NAME, bypass_policy()), ("OtherPolicy", bypass_policy())])).await.unwrap();

        fire_native(&slot, vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, Some(text("disable")))]);
        service.update_policy_definitions(policies(vec![(POLICY_NAME, bypass_policy())])).await.unwrap();

        assert_eq!(
            (service.managed_settings(), watcher_create_count.get()),
            (managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]), 1)
        );
    });
}

#[test]
fn retries_watcher_creation_after_initialization_fails() {
    async_test(|d| async move {
        let watcher_create_count = Rc::new(Cell::new(0));
        let watcher_factory = {
            let watcher_create_count = watcher_create_count.clone();
            NativePolicyWatcherFactory::new(move |_product_name, _policies, callback, _options| {
                watcher_create_count.set(watcher_create_count.get() + 1);
                if watcher_create_count.get() == 1 {
                    return Err(Error::new("initial watcher creation failed"));
                }
                callback(update(vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, Some(PolicyValue::Boolean(true)))]));
                Ok(lifecycle::none())
            })
        };

        let service = new_service(&d, watcher_factory);
        assert!(service.initialize().await.is_err());

        assert_eq!(
            (service.initialize().await.unwrap(), watcher_create_count.get()),
            (managed(vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(true))]), 2)
        );
    });
}

#[test]
fn removes_raw_managed_settings_whose_definitions_are_no_longer_watched() {
    async_test(|d| async move {
        let other_managed_setting_key = "permissions.otherManagedSetting";
        let slot = CallbackSlot::default();
        let service = new_service(&d, factory(&slot, vec![], |_| {}));

        service
            .update_policy_definitions(policies(vec![(
                POLICY_NAME,
                managed_policy(PolicyDefinitionType::Boolean, &[(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, "string"), (other_managed_setting_key, "boolean")]),
            )]))
            .await
            .unwrap();

        fire_native(&slot, vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, Some(text("disable"))), (other_managed_setting_key, Some(PolicyValue::Boolean(true)))]);
        service
            .update_policy_definitions(policies(vec![(POLICY_NAME, managed_policy(PolicyDefinitionType::Boolean, &[(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, "string")]))]))
            .await
            .unwrap();

        assert_eq!(service.managed_settings(), managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
    });
}

/// `DeferredManagedSettingsChannel`: um canal cujo instantâneo inicial o teste resolve quando quer.
struct DeferredManagedSettingsChannel {
    base: Rc<Disposable>,
    on_did_change_managed_settings: Rc<Emitter<Marshalled>>,
    initial_snapshot: Promise<Marshalled>,
    resolve_initial_snapshot: Resolver<Marshalled>,
    update_policy_definitions_result: RefCell<ManagedSettingsData>,
}

impl DeferredManagedSettingsChannel {
    fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_managed_settings: Rc<Emitter<Marshalled>> = Emitter::new();
        base.register(on_did_change_managed_settings.clone());
        let (initial_snapshot, resolve_initial_snapshot) = Promise::new();
        Rc::new(Self { base, on_did_change_managed_settings, initial_snapshot, resolve_initial_snapshot, update_policy_definitions_result: RefCell::new(ManagedSettingsData::new()) })
    }

    fn fire(&self, managed_settings: &ManagedSettingsData) {
        self.on_did_change_managed_settings.fire(managed_settings_to_marshalled(managed_settings));
    }

    fn resolve_initial_snapshot(&self, managed_settings: &ManagedSettingsData) {
        self.resolve_initial_snapshot.resolve(managed_settings_to_marshalled(managed_settings));
    }
}

impl IChannel for DeferredManagedSettingsChannel {
    fn call(&self, command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "getManagedSettings" => self.initial_snapshot.clone(),
            "updatePolicyDefinitions" => Promise::resolved(managed_settings_to_marshalled(&self.update_policy_definitions_result.borrow())),
            _ => je_base::errors::throw(Error::new(format!("Call not found: {command}"))),
        }
    }

    fn listen(&self, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        assert_eq!(event, "onDidChangeManagedSettings");
        self.on_did_change_managed_settings.event()
    }
}

impl IDisposable for DeferredManagedSettingsChannel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `RetryManagedSettingsChannel`: o primeiro `getManagedSettings` fica pendente até o teste rejeitá-lo.
struct RetryManagedSettingsChannel {
    initial_snapshot: Promise<Marshalled>,
    reject_initial_snapshot: Resolver<Marshalled>,
    get_managed_settings_call_count: Cell<u32>,
}

impl RetryManagedSettingsChannel {
    fn new() -> Rc<Self> {
        let (initial_snapshot, reject_initial_snapshot) = Promise::new();
        Rc::new(Self { initial_snapshot, reject_initial_snapshot, get_managed_settings_call_count: Cell::new(0) })
    }

    fn reject_initial_snapshot(&self) {
        self.reject_initial_snapshot.reject(Error::new("initial native managed-settings read failed"));
    }
}

impl IChannel for RetryManagedSettingsChannel {
    fn call(&self, command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        assert_eq!(command, "getManagedSettings");
        self.get_managed_settings_call_count.set(self.get_managed_settings_call_count.get() + 1);
        if self.get_managed_settings_call_count.get() == 1 {
            return self.initial_snapshot.clone();
        }
        Promise::resolved(managed_settings_to_marshalled(&managed(vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(true))])))
    }

    fn listen(&self, _event: &str, _arg: Marshalled) -> Event<Marshalled> {
        Event::none()
    }
}

#[test]
fn channel_client_keeps_newer_event_state_when_initial_snapshot_resolves_later() {
    async_test(|d| async move {
        let channel = d.add_rc(DeferredManagedSettingsChannel::new());
        let client = d.add_rc(NativeManagedSettingsChannelClient::new(channel.clone(), NullLogService::shared()));

        channel.fire(&managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
        channel.resolve_initial_snapshot(&managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable"))]));
        client.initialize().await.unwrap();

        assert_eq!(client.managed_settings(), managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
    });
}

#[test]
fn channel_client_update_policy_definitions_updates_cache_without_firing_change_event() {
    async_test(|d| async move {
        let channel = d.add_rc(DeferredManagedSettingsChannel::new());
        let client = d.add_rc(NativeManagedSettingsChannelClient::new(channel.clone(), NullLogService::shared()));
        channel.resolve_initial_snapshot(&managed(vec![]));
        channel.initial_snapshot.clone().await.unwrap();

        let event_count = Rc::new(Cell::new(0));
        let counter = event_count.clone();
        d.add(client.on_did_change_managed_settings().listen(move |_| counter.set(counter.get() + 1)));

        *channel.update_policy_definitions_result.borrow_mut() = managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]);
        let result = client.update_policy_definitions(vec![]).await.unwrap();

        assert_eq!(
            (result, client.managed_settings(), event_count.get()),
            (
                managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]),
                managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]),
                0
            )
        );
    });
}

#[test]
fn channel_client_retries_a_failed_initial_snapshot() {
    async_test(|d| async move {
        let channel = RetryManagedSettingsChannel::new();
        let client = d.add_rc(NativeManagedSettingsChannelClient::new(channel.clone(), NullLogService::shared()));
        let initial = client.initialize();

        channel.reject_initial_snapshot();
        assert!(initial.await.is_err());

        assert_eq!(client.initialize().await.unwrap(), managed(vec![(COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, PolicyValue::Boolean(true))]));
        assert_eq!(channel.get_managed_settings_call_count.get(), 2);
    });
}
