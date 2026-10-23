//! Porte de `src/vs/workbench/services/policies/test/browser/policyTelemetryContribution.test.ts`, caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado em `run`; `sinon.useFakeTimers()` e
//!   `clock.tick(ms)` viram `runtime::with_faked_timers` e `runtime::advance_fake_time`.
//! - `TestPolicyService extends AbstractPolicyService` guarda o `Rc<AbstractPolicyService>` e delega o trait
//!   `PolicyService`. O `sinon.stub(policyService, 'getPolicyValueSource').returns(undefined)` vira o campo
//!   `hide_sources`.
//! - O `telemetryService` de mentira registra `(nome, dado)`. Um [`Dynamic`] de objeto compara por identidade,
//!   então o dado é reduzido a um [`Snapshot`] (pares chave/valor ordenados) para o `deepStrictEqual`.
//! - O caso parametrizado por fonte (`for ... of`) vira um laço com uma execução isolada por fonte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::policy::{
    AbstractPolicyService, PolicyDefinition, PolicyDefinitionType, PolicyDefinitions, PolicyService, PolicyValue, PolicyValueSource, PolicyValues,
    SerializedPolicy,
};
use je_platform::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_workbench::services::policies::policy_telemetry_contribution::{policy_value_source_name, PolicyTelemetryContribution};

struct TestPolicyService {
    inner: Rc<AbstractPolicyService>,
    hide_sources: Cell<bool>,
}

impl TestPolicyService {
    fn new() -> Rc<Self> {
        let inner = AbstractPolicyService::new(Rc::new(|_| Promise::resolved(())));
        Rc::new(Self { inner, hide_sources: Cell::new(false) })
    }

    fn set_policy(&self, name: &str, value: PolicyValue, source: Option<PolicyValueSource>) {
        let type_ = match value {
            PolicyValue::String(_) => PolicyDefinitionType::String,
            PolicyValue::Number(_) => PolicyDefinitionType::Number,
            PolicyValue::Boolean(_) => PolicyDefinitionType::Boolean,
        };
        let _ = self.inner.update_policy_definitions(vec![(name.to_owned(), Rc::new(PolicyDefinition::new(type_)))]);
        self.inner.update_policy_value(name, Some(value), source.unwrap_or(PolicyValueSource::Device));
    }

    fn remove_policy(&self, name: &str) {
        self.inner.update_policy_value(name, None, PolicyValueSource::Device);
    }

    fn fire_change(&self) {
        self.inner.fire_did_change(Vec::new());
    }
}

impl PolicyService for TestPolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        self.inner.on_did_change()
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        PolicyService::update_policy_definitions(&self.inner, policy_definitions)
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.inner.get_policy_value(name)
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        if self.hide_sources.get() { None } else { self.inner.get_policy_value_source(name) }
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        self.inner.serialize()
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        self.inner.policy_definitions()
    }
}

/// Os pares chave/valor de um dado de evento, ordenados por chave.
type Snapshot = Vec<(String, String)>;

fn snapshot(data: &Dynamic) -> Snapshot {
    let mut entries: Snapshot = data.own_entries().into_iter().map(|(key, value)| (key.to_string_lossy(), format!("{value:?}"))).collect();
    entries.sort();
    entries
}

/// O `telemetryService` do teste: só o `publicLog2` faz algo.
#[derive(Default)]
struct RecordingTelemetryService {
    events: RefCell<Vec<(String, Snapshot)>>,
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::Usage
    }
    fn session_id(&self) -> String {
        String::new()
    }
    fn machine_id(&self) -> String {
        String::new()
    }
    fn sqm_id(&self) -> String {
        String::new()
    }
    fn dev_device_id(&self) -> String {
        String::new()
    }
    fn first_session_date(&self) -> String {
        String::new()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        let data = data.expect("o evento policy.applied leva dado");
        self.events.borrow_mut().push((event_name.to_owned(), snapshot(&data)));
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

impl RecordingTelemetryService {
    fn events(&self) -> Vec<(String, Snapshot)> {
        self.events.borrow().clone()
    }

    fn data(&self, index: usize) -> Snapshot {
        self.events.borrow()[index].1.clone()
    }
}

/// `POLICY_SOURCE_FIELDS`: (campo do evento, política).
const POLICY_SOURCE_FIELDS: [(&str, &str); 21] = [
    ("toolsEligibleForAutoApprovalSource", "ChatToolsEligibleForAutoApproval"),
    ("toolsTerminalEnableAutoApproveSource", "ChatToolsTerminalEnableAutoApprove"),
    ("mcpAccessSource", "ChatMCP"),
    ("allowedMcpServersSource", "ChatAllowedMcpServers"),
    ("deniedMcpServersSource", "ChatDeniedMcpServers"),
    ("allowManagedMcpServersOnlySource", "ChatAllowManagedMcpServersOnly"),
    ("mcpEnterpriseManagedAuthIdpSource", "McpEnterpriseManagedAuthIdp"),
    ("pluginsEnabledSource", "ChatPluginsEnabled"),
    ("enabledPluginsSource", "ChatEnabledPlugins"),
    ("extraMarketplacesSource", "ChatExtraMarketplaces"),
    ("strictPluginOnlyCustomizationSource", "ChatStrictPluginOnlyCustomization"),
    ("hooksSource", "ChatHooks"),
    ("allowManagedHooksOnlySource", "ChatAllowManagedHooksOnly"),
    ("agentModeSource", "ChatAgentMode"),
    ("otelEnabledSource", "CopilotOtelEnabled"),
    ("otelProtocolSource", "CopilotOtelProtocol"),
    ("otelCaptureIdentitySource", "CopilotOtelCaptureIdentity"),
    ("otelHeadersSource", "CopilotOtelHeaders"),
    ("agentNetworkFilterSource", "ChatAgentNetworkFilter"),
    ("agentAllowedNetworkDomainsSource", "ChatAgentAllowedNetworkDomains"),
    ("agentDeniedNetworkDomainsSource", "ChatAgentDeniedNetworkDomains"),
];

fn number(value: usize) -> Dynamic {
    Dynamic::Number(value as f64)
}

fn flag(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn source_text(source: PolicyValueSource) -> Dynamic {
    text(policy_value_source_name(source))
}

/// `EMPTY_EVENT`.
fn empty_event() -> Vec<(String, Dynamic)> {
    let mut entries: Vec<(String, Dynamic)> = [
        "devicePolicyCount",
        "nativeMdmPolicyCount",
        "serverManagedSettingsPolicyCount",
        "fileManagedSettingsPolicyCount",
        "mixedManagedSettingsPolicyCount",
        "accountPolicyCount",
        "accountGatePolicyCount",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), number(0)))
    .collect();
    entries.extend(
        [
            "defaultModelSet",
            "toolsAutoApproveSet",
            "enabledPluginsSet",
            "extraMarketplacesSet",
            "approvedOrgsSet",
            "otelSet",
            "telemetryLevelSet",
            "enableFeedbackSet",
            "defaultModelForcedToAuto",
            "toolsAutoApproveForcedOff",
            "otelForcedEnabled",
        ]
        .into_iter()
        .map(|key| (key.to_owned(), flag(false))),
    );
    entries.push(("telemetryLevel".to_owned(), Dynamic::Undefined));
    entries.extend(POLICY_SOURCE_FIELDS.iter().map(|(field, _)| ((*field).to_owned(), text("none"))));
    entries
}

/// `{ ...EMPTY_EVENT, ...overrides }`: toda chave sobrescrita já existe no `EMPTY_EVENT`.
fn event_with(overrides: Vec<(&str, Dynamic)>) -> Snapshot {
    let mut entries = empty_event();
    for (key, value) in overrides {
        let entry = entries.iter_mut().find(|(k, _)| k == key).unwrap_or_else(|| panic!("chave desconhecida: {key}"));
        entry.1 = value;
    }
    let mut snapshot: Snapshot = entries.into_iter().map(|(key, value)| (key, format!("{value:?}"))).collect();
    snapshot.sort();
    snapshot
}

/// `{ name: 'policy.applied', data }`.
fn applied(data: Snapshot) -> (String, Snapshot) {
    ("policy.applied".to_owned(), data)
}

fn string_value(value: &str) -> PolicyValue {
    PolicyValue::String(value.to_owned())
}

/// O `setup`/`teardown` da suíte: guarda de vazamento e relógio virtual; `clock.tick(ms)` é `tick`.
fn run(body: impl FnOnce(&LeakGuard)) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| body(&guard));
}

fn tick(ms: u64) {
    runtime::advance_fake_time(ms);
}

/// `createContribution(policyService)`: devolve o `events` do telemetryService de mentira.
fn create_contribution(guard: &LeakGuard, policy_service: &Rc<TestPolicyService>) -> Rc<RecordingTelemetryService> {
    let telemetry_service = Rc::new(RecordingTelemetryService::default());
    guard.add_rc(policy_service.inner.clone());
    guard.add_rc(PolicyTelemetryContribution::new(policy_service.clone(), telemetry_service.clone()));
    telemetry_service
}

// suite('PolicyTelemetryContribution')

#[test]
fn emits_an_empty_applied_event_at_startup_when_no_policies_are_set() {
    run(|guard| {
        let events = create_contribution(guard, &TestPolicyService::new());
        tick(500);

        assert_eq!(events.events(), vec![applied(event_with(vec![]))]);
    });
}

#[test]
fn reports_every_applied_policy_and_value_bucket() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("ChatDefaultModel", string_value("auto"), None);
        policy_service.set_policy("ChatToolsAutoApprove", PolicyValue::Boolean(false), None);
        policy_service.set_policy("ChatEnabledPlugins", string_value("[]"), None);
        policy_service.set_policy("ChatExtraMarketplaces", string_value("[]"), None);
        policy_service.set_policy("ChatApprovedAccountOrganizations", string_value("[]"), None);
        policy_service.set_policy("CopilotOtelEnabled", PolicyValue::Boolean(true), None);
        policy_service.set_policy("TelemetryLevel", string_value("all"), None);
        policy_service.set_policy("EnableFeedback", PolicyValue::Boolean(false), None);

        let events = create_contribution(guard, &policy_service);
        tick(500);

        assert_eq!(
            events.data(0),
            event_with(vec![
                ("devicePolicyCount", number(8)),
                ("defaultModelSet", flag(true)),
                ("toolsAutoApproveSet", flag(true)),
                ("enabledPluginsSet", flag(true)),
                ("extraMarketplacesSet", flag(true)),
                ("approvedOrgsSet", flag(true)),
                ("otelSet", flag(true)),
                ("telemetryLevelSet", flag(true)),
                ("enableFeedbackSet", flag(true)),
                ("defaultModelForcedToAuto", flag(true)),
                ("toolsAutoApproveForcedOff", flag(true)),
                ("otelForcedEnabled", flag(true)),
                ("telemetryLevel", text("all")),
                ("enabledPluginsSource", source_text(PolicyValueSource::Device)),
                ("extraMarketplacesSource", source_text(PolicyValueSource::Device)),
                ("otelEnabledSource", source_text(PolicyValueSource::Device)),
            ])
        );
    });
}

#[test]
fn buckets_unexpected_values_without_reporting_them() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("TelemetryLevel", PolicyValue::Number(1.0), None);

        let events = create_contribution(guard, &policy_service);
        tick(500);

        assert_eq!(
            events.data(0),
            event_with(vec![("devicePolicyCount", number(1)), ("telemetryLevelSet", flag(true)), ("telemetryLevel", text("unknown"))])
        );
    });
}

#[test]
fn counts_applied_policies_outside_the_reported_set() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("OtherPolicy", PolicyValue::Boolean(true), None);

        let events = create_contribution(guard, &policy_service);
        tick(500);

        assert_eq!(events.data(0), event_with(vec![("devicePolicyCount", number(1))]));
    });
}

#[test]
fn partitions_every_effective_policy_by_source() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("DevicePolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::Device));
        policy_service.set_policy("NativeMdmPolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::NativeMdm));
        policy_service.set_policy("ServerManagedSettingsPolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::ServerManagedSettings));
        policy_service.set_policy("FileManagedSettingsPolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::FileManagedSettings));
        policy_service.set_policy("MixedManagedSettingsPolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::MixedManagedSettings));
        policy_service.set_policy("AccountPolicy", PolicyValue::Boolean(true), Some(PolicyValueSource::Account));
        policy_service.set_policy("AccountGatePolicy", PolicyValue::Boolean(false), Some(PolicyValueSource::AccountGate));
        policy_service.set_policy("UnknownSourcePolicy", PolicyValue::Boolean(true), None);

        let events = create_contribution(guard, &policy_service);
        tick(500);

        assert_eq!(
            events.data(0),
            event_with(vec![
                ("devicePolicyCount", number(2)),
                ("nativeMdmPolicyCount", number(1)),
                ("serverManagedSettingsPolicyCount", number(1)),
                ("fileManagedSettingsPolicyCount", number(1)),
                ("mixedManagedSettingsPolicyCount", number(1)),
                ("accountPolicyCount", number(1)),
                ("accountGatePolicyCount", number(1)),
            ])
        );
    });
}

#[test]
fn maps_each_tracked_policy_to_its_own_source_field() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        let events = create_contribution(guard, &policy_service);
        let mut expected = Vec::new();

        for (field, name) in POLICY_SOURCE_FIELDS {
            policy_service.set_policy(name, PolicyValue::Boolean(false), Some(PolicyValueSource::Device));
            policy_service.fire_change();
            tick(500);
            expected.push(applied(event_with(vec![
                ("devicePolicyCount", number(1)),
                ("enabledPluginsSet", flag(name == "ChatEnabledPlugins")),
                ("extraMarketplacesSet", flag(name == "ChatExtraMarketplaces")),
                ("otelSet", flag(name == "CopilotOtelEnabled")),
                (field, source_text(PolicyValueSource::Device)),
            ])));
            policy_service.remove_policy(name);
        }

        assert_eq!(events.events(), expected);
    });
}

#[test]
fn reports_every_tracked_policy_source() {
    // Cada fonte do original é um caso `reports every tracked policy source: <fonte>`; `None` é o
    // `device fallback`, em que `getPolicyValueSource` devolve `undefined`.
    let cases: [(Option<PolicyValueSource>, &str); 8] = [
        (Some(PolicyValueSource::Device), "devicePolicyCount"),
        (Some(PolicyValueSource::NativeMdm), "nativeMdmPolicyCount"),
        (Some(PolicyValueSource::ServerManagedSettings), "serverManagedSettingsPolicyCount"),
        (Some(PolicyValueSource::FileManagedSettings), "fileManagedSettingsPolicyCount"),
        (Some(PolicyValueSource::MixedManagedSettings), "mixedManagedSettingsPolicyCount"),
        (Some(PolicyValueSource::Account), "accountPolicyCount"),
        (Some(PolicyValueSource::AccountGate), "accountGatePolicyCount"),
        (None, "devicePolicyCount"),
    ];
    for (source, count_field) in cases {
        run(|guard| {
            let policy_service = TestPolicyService::new();
            for (_, name) in POLICY_SOURCE_FIELDS {
                policy_service.set_policy(name, PolicyValue::Boolean(false), source);
            }
            if source.is_none() {
                policy_service.hide_sources.set(true);
            }

            let events = create_contribution(guard, &policy_service);
            tick(500);

            let mut overrides = vec![
                (count_field, number(POLICY_SOURCE_FIELDS.len())),
                ("enabledPluginsSet", flag(true)),
                ("extraMarketplacesSet", flag(true)),
                ("otelSet", flag(true)),
            ];
            overrides.extend(POLICY_SOURCE_FIELDS.iter().map(|(field, _)| (*field, source_text(source.unwrap_or(PolicyValueSource::Device)))));
            assert_eq!(events.events(), vec![applied(event_with(overrides))]);
        });
    }
}

#[test]
fn reports_policy_sources_without_collecting_sensitive_values() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("ChatAllowedMcpServers", string_value("[]"), Some(PolicyValueSource::Device));
        policy_service.set_policy("CopilotOtelHeaders", string_value("{\"Authorization\":\"sensitive-token\"}"), Some(PolicyValueSource::ServerManagedSettings));
        policy_service.set_policy("ChatAgentAllowedNetworkDomains", string_value("[\"sensitive.example\"]"), Some(PolicyValueSource::NativeMdm));
        policy_service.set_policy("ChatMCP", string_value("all"), Some(PolicyValueSource::Device));

        let events = create_contribution(guard, &policy_service);
        tick(500);

        assert_eq!(
            events.events(),
            vec![applied(event_with(vec![
                ("devicePolicyCount", number(2)),
                ("serverManagedSettingsPolicyCount", number(1)),
                ("nativeMdmPolicyCount", number(1)),
                ("allowedMcpServersSource", source_text(PolicyValueSource::Device)),
                ("otelHeadersSource", source_text(PolicyValueSource::ServerManagedSettings)),
                ("agentAllowedNetworkDomainsSource", source_text(PolicyValueSource::NativeMdm)),
                ("mcpAccessSource", source_text(PolicyValueSource::Device)),
            ]))]
        );
    });
}

#[test]
fn reports_source_swaps_with_unchanged_counts_and_values_then_policy_removal() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        policy_service.set_policy("ChatHooks", PolicyValue::Boolean(false), Some(PolicyValueSource::Device));
        policy_service.set_policy("ChatMCP", string_value("none"), Some(PolicyValueSource::ServerManagedSettings));
        let events = create_contribution(guard, &policy_service);
        tick(500);

        policy_service.set_policy("ChatHooks", PolicyValue::Boolean(false), Some(PolicyValueSource::ServerManagedSettings));
        policy_service.set_policy("ChatMCP", string_value("none"), Some(PolicyValueSource::Device));
        policy_service.fire_change();
        tick(500);
        policy_service.fire_change();
        tick(500);

        policy_service.remove_policy("ChatHooks");
        policy_service.remove_policy("ChatMCP");
        policy_service.fire_change();
        tick(500);

        assert_eq!(
            events.events(),
            vec![
                applied(event_with(vec![
                    ("devicePolicyCount", number(1)),
                    ("serverManagedSettingsPolicyCount", number(1)),
                    ("hooksSource", source_text(PolicyValueSource::Device)),
                    ("mcpAccessSource", source_text(PolicyValueSource::ServerManagedSettings)),
                ])),
                applied(event_with(vec![
                    ("devicePolicyCount", number(1)),
                    ("serverManagedSettingsPolicyCount", number(1)),
                    ("hooksSource", source_text(PolicyValueSource::ServerManagedSettings)),
                    ("mcpAccessSource", source_text(PolicyValueSource::Device)),
                ])),
                applied(event_with(vec![])),
            ]
        );
    });
}

#[test]
fn coalesces_startup_changes_and_re_emits_only_when_the_resolved_policy_state_changes() {
    run(|guard| {
        let policy_service = TestPolicyService::new();
        let events = create_contribution(guard, &policy_service);

        policy_service.set_policy("TelemetryLevel", string_value("off"), None);
        policy_service.fire_change();
        tick(500);

        policy_service.set_policy("TelemetryLevel", string_value("all"), None);
        policy_service.fire_change();
        tick(500);
        policy_service.fire_change();
        tick(500);

        assert_eq!(
            events.events(),
            vec![
                applied(event_with(vec![("devicePolicyCount", number(1)), ("telemetryLevelSet", flag(true)), ("telemetryLevel", text("off"))])),
                applied(event_with(vec![("devicePolicyCount", number(1)), ("telemetryLevelSet", flag(true)), ("telemetryLevel", text("all"))])),
            ]
        );
    });
}
