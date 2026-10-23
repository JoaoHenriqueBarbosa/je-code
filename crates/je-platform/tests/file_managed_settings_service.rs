//! Porte de `src/vs/platform/policy/test/common/fileManagedSettingsService.test.ts` (suítes
//! `normalizeManagedSettings`, `FileManagedSettingsService` e `FileManagedSettingsChannelClient`), caso a caso e
//! na mesma ordem.
//!
//! Divergências: os documentos de entrada são valores dinâmicos (`j("{...}")` para os literais, `obj` quando a
//! chave é uma constante), e os resultados são `IndexMap`s comparados sem ordem, como o `deepStrictEqual`.
//! `JSON.parse(JSON.stringify(x))` é o mesmo laço sobre o valor de canal ([`json_round_trip`]).
//! `Object.create({ telemetry: {} })` (um objeto cuja única `telemetry` é herdada) é o objeto vazio: o valor
//! dinâmico não tem protótipos, e o que o caso afirma (uma chave herdada não conta) é que o resultado é `{}`.
//! A suíte do serviço roda sob o relógio virtual (`async_test`, o `runWithFakedTimers`); `await new
//! Promise(resolve => setTimeout(resolve, 10))` é `timeout(10)`, e `Event.toPromise(evento)` é
//! `to_promise(None)`. O `DeferredManagedSettingsChannel` fala `Marshalled`, o valor de canal do porte: o que
//! o original passa como objeto passa pelas conversões de `file_managed_settings_ipc`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::files::async_test;
use common::{assert_deep_strict_equal, dyn_strs, ensure_no_disposables_are_leaked_in_test_suite, j, obj, stringify};
use je_base::async_::timeout;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::default_account::IPolicyData;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::marshalling::{self, Marshalled};
use je_base::parts::ipc::IChannel;
use je_base::path::Platform;
use je_base::policy::PolicyValue;
use je_base::promise::{Promise, Resolver};
use je_base::uri::{URI, UriChange};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::policy::copilot_managed_settings::{
    COPILOT_ALLOW_MANAGED_HOOKS_ONLY_KEY, COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_KEY, COPILOT_AUTO_TIER_KEY, COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, COPILOT_ENABLED_PLUGINS_KEY,
    COPILOT_EXTRA_MARKETPLACES_KEY, COPILOT_MODEL_KEY, COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY, COPILOT_TOP_LEVEL_MODEL_KEY, IFileManagedSettingsService, ManagedSettingsData,
    RawManagedSettingsData, managed_model_value, normalize_managed_settings, pick_managed_settings,
};
use je_platform::policy::file_managed_settings_ipc::{
    FileManagedSettingsChannelClient, managed_settings_from_marshalled, managed_settings_to_marshalled, raw_managed_settings_to_marshalled,
};
use je_platform::policy::file_managed_settings_service::FileManagedSettingsService;

fn text(value: &str) -> PolicyValue {
    PolicyValue::String(value.to_owned())
}

/// Um `ManagedSettingsData` literal.
fn managed(entries: Vec<(&str, PolicyValue)>) -> ManagedSettingsData {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

fn normalize(document: &Dynamic) -> ManagedSettingsData {
    normalize_managed_settings(document, None)
}

/// `JSON.parse(JSON.stringify(data))`.
fn json_round_trip(data: &ManagedSettingsData) -> ManagedSettingsData {
    let json = marshalling::stringify(&managed_settings_to_marshalled(data));
    managed_settings_from_marshalled(&marshalling::parse(json.units(), Platform::Posix))
}

/// `{ managedSettings }` como o `IPolicyData` que o callback do modelo recebe.
fn policy_data(managed_settings: ManagedSettingsData) -> IPolicyData {
    IPolicyData {
        mcp: None,
        chat_preview_features_enabled: None,
        chat_agent_enabled: None,
        cloud_session_storage_enabled: None,
        mcp_registry_url: None,
        mcp_access: None,
        managed_settings: Some(managed_settings),
        managed_settings_active: None,
        extra: Default::default(),
    }
}

fn model_value(managed_settings: ManagedSettingsData) -> Option<PolicyValue> {
    managed_model_value().call(&policy_data(managed_settings))
}

/// `normalizeManagedSettings(document, msg => warnings.push(msg))`.
fn normalize_collecting_warnings(document: &Dynamic) -> (ManagedSettingsData, Vec<String>) {
    let warnings = RefCell::new(Vec::new());
    let result = normalize_managed_settings(document, Some(&|message: &str| warnings.borrow_mut().push(message.to_owned())));
    (result, warnings.into_inner())
}

// ---- suíte `normalizeManagedSettings` ----

#[test]
fn flattens_scalar_leaves_to_dot_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&j("{ 'permissions': { 'disableBypassPermissionsMode': 'disable' } }"));
    assert_eq!(result, managed(vec![("permissions.disableBypassPermissionsMode", text("disable"))]));
}

#[test]
fn auto_tier_variants_resolve_atomically_without_mixing_managed_sources() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let native = normalize(&j("{ 'autoTier': { 'overridable': 'efficiency' } }"));
    let server = normalize(&j("{ 'autoTier': 'intelligence' }"));
    let file = normalize(&j("{ 'autoTier': { 'overridable': 'balance' } }"));
    // (native, effective, removed)
    assert_eq!(
        (native.clone(), pick_managed_settings(Some(&native), Some(&server), Some(&file)).values, pick_managed_settings(None, Some(&server), Some(&file)).values),
        (
            managed(vec![(COPILOT_AUTO_TIER_KEY, text(r#"{"overridable":"efficiency"}"#))]),
            managed(vec![(COPILOT_AUTO_TIER_KEY, text(r#"{"overridable":"efficiency"}"#))]),
            managed(vec![(COPILOT_AUTO_TIER_KEY, text(r#""intelligence""#))]),
        )
    );
}

#[test]
fn json_stringifies_structured_keys_enabled_plugins() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let plugins = j("{ 'plugin@marketplace': false }");
    let result = normalize(&obj(vec![(COPILOT_ENABLED_PLUGINS_KEY, plugins.clone())]));
    assert_eq!(result, managed(vec![(COPILOT_ENABLED_PLUGINS_KEY, text(&stringify(&plugins)))]));
}

#[test]
fn normalizes_customization_lockdown_controls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        normalize(&obj(vec![
            (COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY, Dynamic::Bool(true)),
            (COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_KEY, Dynamic::Bool(true)),
            (COPILOT_ALLOW_MANAGED_HOOKS_ONLY_KEY, Dynamic::Bool(false)),
        ])),
        managed(vec![
            (COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY, PolicyValue::Boolean(true)),
            (COPILOT_ALLOW_MANAGED_MCP_SERVERS_ONLY_KEY, PolicyValue::Boolean(true)),
            (COPILOT_ALLOW_MANAGED_HOOKS_ONLY_KEY, PolicyValue::Boolean(false)),
        ])
    );
}

#[test]
fn drops_a_non_boolean_strict_plugin_only_customization_value() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(normalize(&obj(vec![(COPILOT_STRICT_PLUGIN_ONLY_CUSTOMIZATION_KEY, dyn_strs(&["skills", "unknown"]))])), managed(vec![]));
}

#[test]
fn normalizes_extra_known_marketplaces_from_schema_format_to_config_dict() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&obj(vec![(
        COPILOT_EXTRA_MARKETPLACES_KEY,
        j("{
            'a': { 'source': { 'source': 'github', 'repo': 'github/agent-skills' }, 'autoUpdate': true },
            'b': { 'source': { 'source': 'git', 'url': 'https://example.com/repo.git', 'ref': 'v1' }, 'autoUpdate': false },
            'c': { 'source': { 'source': 'github', 'repo': 'github/copilot-plugins' } }
        }"),
    )]));
    assert_eq!(
        result,
        managed(vec![(
            COPILOT_EXTRA_MARKETPLACES_KEY,
            text(
                r#"{"a":"{\"source\":\"github/agent-skills\",\"autoUpdate\":true}","b":"{\"source\":\"https://example.com/repo.git#v1\",\"autoUpdate\":false}","c":"github/copilot-plugins"}"#
            ),
        )])
    );
}

#[test]
fn ignores_non_boolean_marketplace_auto_update_with_warning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (result, warnings) = normalize_collecting_warnings(&obj(vec![(
        COPILOT_EXTRA_MARKETPLACES_KEY,
        j("{ 'a': { 'source': { 'source': 'github', 'repo': 'github/agent-skills' }, 'autoUpdate': 'yes' } }"),
    )]));
    assert_eq!(result, managed(vec![(COPILOT_EXTRA_MARKETPLACES_KEY, text(r#"{"a":"github/agent-skills"}"#))]));
    assert_eq!(warnings, vec![r#"Ignoring invalid autoUpdate for extraKnownMarketplaces entry "a": expected boolean"#.to_owned()]);
}

#[test]
fn drops_malformed_marketplace_entries_with_warning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (result, warnings) = normalize_collecting_warnings(&obj(vec![(
        COPILOT_EXTRA_MARKETPLACES_KEY,
        j("{ 'good': { 'source': { 'source': 'github', 'repo': 'a/b' } }, 'bad': {} }"),
    )]));
    assert_eq!(result, managed(vec![(COPILOT_EXTRA_MARKETPLACES_KEY, text(r#"{"good":"a/b"}"#))]));
    assert_eq!(warnings.len(), 1);
}

#[test]
fn handles_mixed_scalar_and_structured_keys() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&obj(vec![
        ("permissions", j("{ 'disableBypassPermissionsMode': 'disable' }")),
        ("strictKnownMarketplaces", dyn_strs(&["github/foo"])),
        (COPILOT_ENABLED_PLUGINS_KEY, j("{ 'plugin': true }")),
    ]));
    assert_eq!(
        result,
        managed(vec![
            ("permissions.disableBypassPermissionsMode", text("disable")),
            ("strictKnownMarketplaces", text(r#"["github/foo"]"#)),
            (COPILOT_ENABLED_PLUGINS_KEY, text(r#"{"plugin":true}"#)),
        ])
    );
}

#[test]
fn flattens_the_model_setting_nested_under_permissions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // The server/file managed-settings schema carries `model` under `permissions`
    // (alongside disableBypassPermissionsMode); it must flatten to `permissions.model`,
    // which is the key the ChatDefaultModel policy value callback reads.
    let result = normalize(&j("{ 'permissions': { 'model': 'auto' } }"));
    assert_eq!(result, managed(vec![("permissions.model", text("auto"))]));
    assert_eq!(COPILOT_MODEL_KEY, "permissions.model");
    assert_eq!(model_value(result), Some(text("auto")));
}

#[test]
fn carries_the_top_level_model_setting_as_the_model_bag_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&j("{ 'model': 'auto' }"));
    assert_eq!(result, managed(vec![("model", text("auto"))]));
    assert_eq!(COPILOT_TOP_LEVEL_MODEL_KEY, "model");
    assert_eq!(model_value(result), Some(text("auto")));
}

#[test]
fn keeps_top_level_and_legacy_model_keys_distinct_with_the_top_level_value_winning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&j("{ 'model': 'opus', 'permissions': { 'model': 'gemini' } }"));
    assert_eq!(result, managed(vec![("model", text("opus")), ("permissions.model", text("gemini"))]));
    assert_eq!(model_value(result), Some(text("opus")));
}

#[test]
fn handles_empty_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(normalize(&j("{}")), managed(vec![]));
}

#[test]
fn retains_permission_key_presence_through_serialization_without_treating_an_empty_block_as_configured() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let documents = [
        j("{ 'permissions': {} }"),
        j("{ 'permissions': { 'future': {} } }"),
        j("{ 'permissions': { 'future': null } }"),
        j("{ 'permissions': { 'limitTo': [] } }"),
        j("{ 'permissions': { 'disableAssistedPermissionsMode': false } }"),
    ];
    assert_eq!(
        documents.iter().map(|document| json_round_trip(&normalize(document))).collect::<Vec<_>>(),
        vec![
            managed(vec![]),
            managed(vec![("permissions.future", text("{}"))]),
            managed(vec![("permissions.future", text("null"))]),
            managed(vec![("permissions.limitTo", text("[]"))]),
            managed(vec![("permissions.disableAssistedPermissionsMode", PolicyValue::Boolean(false))]),
        ]
    );
}

#[test]
fn retains_empty_and_future_only_telemetry_block_presence_through_json_serialization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let documents = [j("{ 'telemetry': {} }"), j("{ 'telemetry': { 'capture': {} } }"), j("{ 'telemetry': { 'future': ['value'] } }")];
    assert_eq!(
        documents.iter().map(|document| json_round_trip(&normalize(document))).collect::<Vec<_>>(),
        vec![managed(vec![("telemetry", text("{}"))]), managed(vec![("telemetry", text("{}"))]), managed(vec![("telemetry", text("{}"))])]
    );
}

#[test]
fn does_not_interpret_malformed_or_inherited_telemetry_values_as_a_block_anchor() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let documents = [
        j("{}"),
        j("{ 'telemetry': null }"),
        j("{ 'telemetry': false }"),
        j("{ 'telemetry': '{}' }"),
        j("{ 'telemetry': [] }"),
        // Object.create({ telemetry: {} }): a chave só existe no protótipo.
        obj(Vec::<(&str, Dynamic)>::new()),
    ];
    assert_eq!(documents.iter().map(normalize).collect::<Vec<_>>(), documents.iter().map(|_| managed(vec![])).collect::<Vec<_>>());
}

#[test]
fn drops_a_structured_key_whose_value_is_not_an_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = normalize(&obj(vec![(COPILOT_ENABLED_PLUGINS_KEY, Dynamic::string("already-a-string"))]));
    assert_eq!(result, managed(vec![]));
}

// ---- suíte `FileManagedSettingsService` ----

fn managed_settings_file() -> URI {
    URI::file("managed-settings.json", Platform::Posix).with(&UriChange { scheme: "vscode-tests".into(), ..Default::default() })
}

/// O `FileService` com o provider em memória do esquema `vscode-tests` e o `NullLogService`.
fn setup(d: &Rc<DisposableStore>) -> (Rc<FileService>, LogServiceRef) {
    let log_service = NullLogService::shared();
    let file_service = d.add_rc(FileService::new(log_service.clone()));
    let in_memory_provider = d.add_rc(InMemoryFileSystemProvider::new());
    d.add(file_service.register_provider("vscode-tests", in_memory_provider).unwrap());
    (file_service, log_service)
}

async fn write_managed_settings_file(file_service: &Rc<FileService>, content: &str) {
    file_service.write_file(&managed_settings_file(), VSBuffer::from_string(content).into(), None).await.unwrap();
}

fn new_service(d: &Rc<DisposableStore>, file_service: &Rc<FileService>, log_service: &LogServiceRef) -> Rc<FileManagedSettingsService> {
    d.add_rc(FileManagedSettingsService::new(managed_settings_file(), file_service.clone(), log_service.clone()))
}

/// "Wait for initial read": já há configurações, ou espera a primeira mudança.
async fn wait_for_initial_read(service: &Rc<FileManagedSettingsService>) {
    if service.managed_settings().is_empty() {
        service.on_did_change_managed_settings().to_promise(None).await.unwrap();
    }
}

#[test]
fn reads_managed_settings_json_on_startup() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        write_managed_settings_file(
            &file_service,
            &stringify(&j("{ 'permissions': { 'disableBypassPermissionsMode': 'disable' }, 'strictKnownMarketplaces': ['github/foo'] }")),
        )
        .await;

        let service = new_service(&d, &file_service, &log_service);
        service.initialize().await.unwrap();

        assert_eq!(
            service.managed_settings(),
            managed(vec![("permissions.disableBypassPermissionsMode", text("disable")), ("strictKnownMarketplaces", text(r#"["github/foo"]"#))])
        );
    });
}

#[test]
fn retains_permission_rules_in_both_raw_and_normalized_settings() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        let raw = j("{ 'permissions': { 'deny': ['Shell(echo denied *)'], 'ask': ['Shell(echo ask *)'], 'allow': ['Shell(echo *)'] } }");
        write_managed_settings_file(&file_service, &stringify(&raw)).await;

        let service = new_service(&d, &file_service, &log_service);
        service.on_did_change_raw_managed_settings().to_promise(None).await.unwrap();

        assert_deep_strict_equal(&service.raw_managed_settings(), &raw);
        assert_eq!(
            service.managed_settings(),
            managed(vec![
                ("permissions.allow", text(r#"["Shell(echo *)"]"#)),
                ("permissions.ask", text(r#"["Shell(echo ask *)"]"#)),
                ("permissions.deny", text(r#"["Shell(echo denied *)"]"#)),
            ])
        );
    });
}

#[test]
fn returns_empty_object_when_file_does_not_exist() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        let service = new_service(&d, &file_service, &log_service);

        // Give the async refresh a chance to run
        timeout(10).await.unwrap();

        assert_eq!(service.managed_settings(), managed(vec![]));
    });
}

#[test]
fn fires_event_when_file_changes() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        write_managed_settings_file(&file_service, &stringify(&j("{ 'permissions': { 'disableBypassPermissionsMode': 'disable' } }"))).await;

        let service = new_service(&d, &file_service, &log_service);

        // Wait for initial read
        wait_for_initial_read(&service).await;

        // Update the file
        let change_promise = service.on_did_change_managed_settings().to_promise(None);

        write_managed_settings_file(&file_service, &stringify(&j("{ 'strictKnownMarketplaces': ['github/foo'] }"))).await;

        change_promise.await.unwrap();

        assert_eq!(service.managed_settings(), managed(vec![("strictKnownMarketplaces", text(r#"["github/foo"]"#))]));
    });
}

#[test]
fn returns_empty_object_when_the_file_is_malformed_json() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        write_managed_settings_file(&file_service, "{ not: valid json").await;

        let service = new_service(&d, &file_service, &log_service);
        timeout(10).await.unwrap();

        assert_eq!(service.managed_settings(), managed(vec![]));
    });
}

#[test]
fn returns_empty_object_when_the_file_is_not_a_json_object() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        write_managed_settings_file(&file_service, &stringify(&dyn_strs(&["not", "an", "object"]))).await;

        let service = new_service(&d, &file_service, &log_service);
        timeout(10).await.unwrap();

        assert_eq!(service.managed_settings(), managed(vec![]));
    });
}

#[test]
fn clears_managed_settings_and_fires_when_the_file_is_deleted() {
    async_test(|d| async move {
        let (file_service, log_service) = setup(&d);

        write_managed_settings_file(&file_service, &stringify(&j("{ 'permissions': { 'disableBypassPermissionsMode': 'disable' } }"))).await;

        let service = new_service(&d, &file_service, &log_service);

        // Wait for initial read
        wait_for_initial_read(&service).await;

        let change_promise = service.on_did_change_managed_settings().to_promise(None);

        file_service.del(&managed_settings_file(), None).await.unwrap();

        change_promise.await.unwrap();

        assert_eq!(service.managed_settings(), managed(vec![]));
    });
}

// ---- suíte `FileManagedSettingsChannelClient` ----

/// `DeferredManagedSettingsChannel`: um canal cujos instantâneos iniciais o teste resolve quando quer.
struct DeferredManagedSettingsChannel {
    base: Rc<Disposable>,
    on_did_change_raw_managed_settings: Rc<Emitter<Marshalled>>,
    on_did_change_managed_settings: Rc<Emitter<Marshalled>>,
    initial_raw_snapshot: Promise<Marshalled>,
    resolve_initial_raw_snapshot: Resolver<Marshalled>,
    initial_snapshot: Promise<Marshalled>,
    resolve_initial_snapshot: Resolver<Marshalled>,
}

impl DeferredManagedSettingsChannel {
    fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_raw_managed_settings: Rc<Emitter<Marshalled>> = Emitter::new();
        base.register(on_did_change_raw_managed_settings.clone());
        let on_did_change_managed_settings: Rc<Emitter<Marshalled>> = Emitter::new();
        base.register(on_did_change_managed_settings.clone());
        let (initial_raw_snapshot, resolve_initial_raw_snapshot) = Promise::new();
        let (initial_snapshot, resolve_initial_snapshot) = Promise::new();
        Rc::new(Self { base, on_did_change_raw_managed_settings, on_did_change_managed_settings, initial_raw_snapshot, resolve_initial_raw_snapshot, initial_snapshot, resolve_initial_snapshot })
    }

    fn fire_raw(&self, managed_settings: &RawManagedSettingsData) {
        self.on_did_change_raw_managed_settings.fire(raw_managed_settings_to_marshalled(managed_settings));
    }

    fn fire(&self, managed_settings: &ManagedSettingsData) {
        self.on_did_change_managed_settings.fire(managed_settings_to_marshalled(managed_settings));
    }

    fn resolve_initial_snapshot(&self, managed_settings: &ManagedSettingsData) {
        self.resolve_initial_snapshot.resolve(managed_settings_to_marshalled(managed_settings));
    }

    fn resolve_initial_raw_snapshot(&self, managed_settings: &RawManagedSettingsData) {
        self.resolve_initial_raw_snapshot.resolve(raw_managed_settings_to_marshalled(managed_settings));
    }
}

impl IChannel for DeferredManagedSettingsChannel {
    fn call(&self, command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "getRawManagedSettings" => self.initial_raw_snapshot.clone(),
            "getManagedSettings" => self.initial_snapshot.clone(),
            _ => je_base::errors::throw(je_base::errors::Error::new(format!("Call not found: {command}"))),
        }
    }

    fn listen(&self, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        match event {
            "onDidChangeRawManagedSettings" => self.on_did_change_raw_managed_settings.event(),
            "onDidChangeManagedSettings" => self.on_did_change_managed_settings.event(),
            _ => je_base::errors::throw(je_base::errors::Error::new(format!("Event not found: {event}"))),
        }
    }
}

impl IDisposable for DeferredManagedSettingsChannel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn new_client(d: &Rc<DisposableStore>, channel: &Rc<DeferredManagedSettingsChannel>) -> Rc<FileManagedSettingsChannelClient> {
    d.add_rc(Rc::new(FileManagedSettingsChannelClient::new(channel.clone())))
}

#[test]
fn retains_empty_telemetry_presence_without_modifying_raw_transport() {
    async_test(|d| async move {
        let channel = d.add_rc(DeferredManagedSettingsChannel::new());
        let client = new_client(&d, &channel);
        let raw = j("{ 'telemetry': {} }");
        channel.resolve_initial_raw_snapshot(&raw);
        channel.resolve_initial_snapshot(&json_round_trip(&normalize(&raw)));
        client.initialize().await.unwrap();
        assert_deep_strict_equal(&client.raw_managed_settings(), &j("{ 'telemetry': {} }"));
        assert_eq!(client.managed_settings(), managed(vec![("telemetry", text("{}"))]));
    });
}

#[test]
fn keeps_newer_event_state_when_the_initial_snapshot_resolves_later() {
    async_test(|d| async move {
        let channel = d.add_rc(DeferredManagedSettingsChannel::new());
        let client = new_client(&d, &channel);

        // A change event arrives before the initial getManagedSettings call resolves; the later,
        // stale snapshot must not clobber the newer event-delivered state.
        channel.fire_raw(&j("{ 'permissions': { 'allow': ['Shell(echo *)'] } }"));
        channel.fire(&managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
        channel.resolve_initial_raw_snapshot(&j("{ 'permissions': { 'deny': ['Shell(echo *)'] } }"));
        channel.resolve_initial_snapshot(&managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("enable"))]));
        client.initialize().await.unwrap();

        assert_deep_strict_equal(&client.raw_managed_settings(), &j("{ 'permissions': { 'allow': ['Shell(echo *)'] } }"));
        assert_eq!(client.managed_settings(), managed(vec![(COPILOT_DISABLE_BYPASS_PERMISSIONS_MODE_KEY, text("disable"))]));
    });
}
