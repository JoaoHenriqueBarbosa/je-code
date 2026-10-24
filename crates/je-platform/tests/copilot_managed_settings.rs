//! Porte de `src/vs/platform/policy/test/common/copilotManagedSettings.test.ts` (suítes `Copilot managed
//! settings projection` e `Copilot managed settings precedence (pickManagedSettings)`), caso a caso e na mesma
//! ordem.
//!
//! Divergências:
//! - Os documentos de entrada são valores dinâmicos (`j("{...}")` para os literais, `obj` quando a chave é
//!   uma constante); os resultados são `IndexMap`s comparados sem ordem, como o `deepStrictEqual`.
//! - Os `for` que geram um `test(...)` por combinação (a suíte de precedência) viram um único `#[test]` que
//!   percorre as mesmas combinações, na mesma ordem; a mensagem de falha leva a combinação.
//! - `new class extends NullManagedSettingsService { override getManagedSettingValue... }` é [`BagService`],
//!   um serviço sobre um mapa.
//! - `TestConfigurationService` é o de `je_platform::configuration::test_configuration_service`, sem valores
//!   configurados (o `inspect` devolve o literal com `policyValue` `undefined`).
//! - `{ a: undefined }` não existe em `ManagedSettingsData`: o caso do buraco `undefined` testa a chave ausente
//!   na camada de cima, que é o mesmo comportamento.
//! - O caso do `__proto__` usa um documento com uma chave própria `__proto__` (o `Dynamic` não tem
//!   protótipos): o que se afirma é que a chave vira uma chave comum, sem criar outra.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{arr, j, n, obj, s};
use je_base::default_account::IPolicyData;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::policy::{IManagedSettingsPolicyDefinitions, ManagedSettingType, ManagedSettingValue, PolicyValue};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY, COPILOT_MODEL_KEY, COPILOT_OTEL_CAPTURE_IDENTITY_KEY, COPILOT_PERMISSION_RULES_KEYS, COPILOT_SANDBOX_ALLOW_BYPASS_KEY,
    COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, COPILOT_SANDBOX_AUTH_GH_KEY,
    COPILOT_SANDBOX_AUTH_GIT_KEY, COPILOT_SANDBOX_DENIED_PATHS_KEY, COPILOT_SANDBOX_ENABLED_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY, COPILOT_SANDBOX_MCP_SERVERS_KEY,
    COPILOT_SANDBOX_READONLY_PATHS_KEY, COPILOT_SANDBOX_READWRITE_PATHS_KEY, COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY,
    COPILOT_TOP_LEVEL_MODEL_KEY, IForceRemoteSettingsRefreshResolution, IManagedSettingResolution, IManagedSettingsContribution, IManagedSettingsPick, IManagedSettingsService,
    MANAGED_SETTINGS_CONTROL_DEFINITIONS, ManagedSettingsChannel, ManagedSettingsData, collect_managed_settings_definitions, has_managed_permission_denials,
    has_managed_settings_definitions, managed_model_value, managed_setting_definition, managed_setting_value, managed_settings_disabled_value, normalize_managed_settings,
    pick_managed_settings, project_managed_settings, requires_copilot_agent_host, resolve_force_remote_settings_refresh,
};
use je_platform::policy::{PolicyDefinition, PolicyDefinitionRef, PolicyDefinitionType};

use ManagedSettingsChannel::{File, NativeMdm, Server};

// --- helpers ---

fn text(value: &str) -> PolicyValue {
    PolicyValue::String(value.to_owned())
}

fn flag(value: bool) -> PolicyValue {
    PolicyValue::Boolean(value)
}

/// Um `ManagedSettingsData` literal.
fn managed(entries: Vec<(&str, PolicyValue)>) -> ManagedSettingsData {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

/// `{ [key]: value }`.
fn single(key: &str, value: PolicyValue) -> ManagedSettingsData {
    managed(vec![(key, value)])
}

fn normalize(document: &Dynamic) -> ManagedSettingsData {
    normalize_managed_settings(document, None)
}

/// `{ vscode: { [key]: value } }`.
fn nested(key: &str, value: Dynamic) -> Dynamic {
    obj(vec![("vscode", obj(vec![(key, value)]))])
}

/// `{ sandbox: { ...path: value } }` com o caminho dado de fora para dentro.
fn nest(path: &[&str], value: Dynamic) -> Dynamic {
    path.iter().rev().fold(value, |inner, key| obj(vec![(*key, inner)]))
}

fn project(values: &ManagedSettingsData, definitions: &IManagedSettingsPolicyDefinitions) -> ManagedSettingsData {
    project_managed_settings(values, definitions, None)
}

fn definitions_of(entries: Vec<(&str, ManagedSettingType)>) -> IManagedSettingsPolicyDefinitions {
    entries.into_iter().map(|(key, type_)| (key.to_owned(), managed_setting_definition(type_))).collect()
}

/// `{ type: 'boolean' }`, `{ type: 'string' }`... como declaração dinâmica de uma política.
fn declaration(key: &str, type_name: &str) -> Dynamic {
    obj(vec![(key, obj(vec![("type", s(type_name))]))])
}

fn policy(name: &str, type_: PolicyDefinitionType, managed_settings: Option<Dynamic>) -> (String, PolicyDefinitionRef) {
    let mut definition = PolicyDefinition::new(type_);
    definition.managed_settings = managed_settings;
    (name.to_owned(), Rc::new(definition))
}

fn policy_data(managed_settings: Option<ManagedSettingsData>, managed_settings_active: Option<bool>, preview: Option<bool>) -> IPolicyData {
    IPolicyData {
        mcp: None,
        chat_preview_features_enabled: preview,
        chat_agent_enabled: None,
        cloud_session_storage_enabled: None,
        mcp_registry_url: None,
        mcp_access: None,
        managed_settings,
        managed_settings_active,
        extra: Default::default(),
    }
}

fn with_bag(bag: ManagedSettingsData) -> IPolicyData {
    policy_data(Some(bag), None, None)
}

/// `new class extends NullManagedSettingsService { getManagedSettingValue(key) { return bag[key]; } }`.
struct BagService(ManagedSettingsData);

impl IManagedSettingsService for BagService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        Event::none()
    }

    fn get_managed_settings(&self) -> ManagedSettingsData {
        self.0.clone()
    }

    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        self.0.get(key).cloned()
    }
}

/// O serviço que devolve `value` só para `permissions.deny` (`value` pode ser `undefined`).
struct DenyOnlyService(Option<PolicyValue>);

impl IManagedSettingsService for DenyOnlyService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        Event::none()
    }

    fn get_managed_settings(&self) -> ManagedSettingsData {
        ManagedSettingsData::new()
    }

    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        if key == "permissions.deny" { self.0.clone() } else { None }
    }
}

fn contribution(channel: ManagedSettingsChannel, value: PolicyValue) -> IManagedSettingsContribution {
    IManagedSettingsContribution { channel, value }
}

fn resolution(value: PolicyValue, source: ManagedSettingsChannel, contributions: Vec<IManagedSettingsContribution>) -> IManagedSettingResolution {
    IManagedSettingResolution { value, source, contributions }
}

/// `native === undefined ? undefined : { [key]: native }` para os três canais.
fn pick_flags(key: &str, native: Option<bool>, server: Option<bool>, file: Option<bool>) -> IManagedSettingsPick {
    let bag = |value: Option<bool>| value.map(|value| single(key, flag(value)));
    pick_managed_settings(bag(native).as_ref(), bag(server).as_ref(), bag(file).as_ref())
}

/// Os três valores possíveis de cada canal: `[undefined, false, true]`.
const TRI_STATE: [Option<bool>; 3] = [None, Some(false), Some(true)];

fn combination_label(native: Option<bool>, server: Option<bool>, file: Option<bool>) -> String {
    format!("{{native: {native:?}, server: {server:?}, file: {file:?}}}")
}

// --- suíte: Copilot managed settings projection ---

#[test]
fn only_non_empty_deny_rules_require_agent_host_across_delivery_channels() {
    for key in COPILOT_PERMISSION_RULES_KEYS {
        let leaf = key.split('.').nth(1).unwrap();
        let empty = normalize(&obj(vec![("permissions", obj(vec![(leaf, arr(vec![]))]))]));
        let rules = normalize(&obj(vec![("permissions", obj(vec![(leaf, arr(vec![s("Shell")]))]))]));
        let denials = |values: ManagedSettingsData| has_managed_permission_denials(&BagService(values));
        let is_deny = key == "permissions.deny";

        assert_eq!(empty, single(key, text("[]")), "{key}");
        assert_eq!(rules, single(key, text("[\"Shell\"]")), "{key}");
        assert_eq!(MANAGED_SETTINGS_CONTROL_DEFINITIONS.get(key), Some(&managed_setting_definition(ManagedSettingType::String)), "{key}");
        assert!(!denials(ManagedSettingsData::new()), "absent {key}");
        assert!(!denials(empty.clone()), "emptyGate {key}");
        assert_eq!(denials(rules.clone()), is_deny, "rulesGate {key}");
        assert!(!denials(pick_managed_settings(Some(&empty), Some(&rules), Some(&rules)).values), "overridden {key}");
        assert_eq!(denials(pick_managed_settings(Some(&ManagedSettingsData::new()), Some(&rules), Some(&ManagedSettingsData::new())).values), is_deny, "server {key}");
        assert_eq!(denials(pick_managed_settings(Some(&ManagedSettingsData::new()), Some(&ManagedSettingsData::new()), Some(&rules)).values), is_deny, "file {key}");
    }
}

#[test]
fn permission_settings_other_than_deny_do_not_require_agent_host() {
    let permissions = |entries: Vec<(&str, Dynamic)>| normalize(&obj(vec![("permissions", obj(entries))]));
    let values: Vec<ManagedSettingsData> = vec![
        permissions(vec![]),
        normalize(&j("{'telemetry':{'enabled':true},'sandbox':{'enabled':false}}")),
        permissions(vec![("defaultMode", s("manual"))]),
        permissions(vec![("disableAssistedPermissionsMode", Dynamic::Bool(false))]),
        permissions(vec![("disableBypassPermissionsMode", s("enable"))]),
        permissions(vec![("limitTo", arr(vec![]))]),
        permissions(vec![("allow", arr(vec![s("Shell")])), ("ask", arr(vec![s("Read")]))]),
        permissions(vec![("future", obj(Vec::<(&str, Dynamic)>::new()))]),
        permissions(vec![("future", Dynamic::Null)]),
        permissions(vec![("defaultMode", s("manual")), ("disableBypassPermissionsMode", s("disable"))]),
        single("permissions.ask", text("not json")),
        single("permissions.allow", text("{}")),
        single("permissions.deny", flag(false)),
    ];
    let results: Vec<bool> = values.into_iter().map(|bag| has_managed_permission_denials(&BagService(bag))).collect();
    assert_eq!(results, vec![false, false, false, false, false, false, false, false, false, false, false, false, true]);
}

#[test]
fn deny_lists_are_empty_only_when_they_are_valid_empty_json_arrays() {
    let strings = ["[]", " [ \n\t ] ", "[\"Shell\"]", "[\"\"]", "[null]", "[{}]", "{}", "null", "false", "\"\"", "", "not json", "[", "[] trailing", "[/*comment*/]"];
    let mut values: Vec<Option<PolicyValue>> = strings.iter().map(|value| Some(text(value))).collect();
    values.extend([Some(flag(false)), Some(PolicyValue::Number(0.0)), None]);
    let results: Vec<bool> = values.into_iter().map(|value| has_managed_permission_denials(&DenyOnlyService(value))).collect();
    assert_eq!(results, vec![false, false, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, false]);
}

#[test]
fn only_managed_sandbox_true_or_non_empty_deny_rules_require_copilot() {
    let configuration_service = TestConfigurationService::new(None);
    let sandbox = COPILOT_SANDBOX_ENABLED_KEY;
    let values: Vec<ManagedSettingsData> = vec![
        ManagedSettingsData::new(),
        single(sandbox, flag(true)),
        single(sandbox, flag(false)),
        single(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, flag(false)),
        managed(vec![(sandbox, flag(true)), (COPILOT_SANDBOX_ALLOW_BYPASS_KEY, flag(true))]),
        managed(vec![(sandbox, flag(false)), ("permissions.ask", text("[\"Shell\"]"))]),
        managed(vec![(sandbox, flag(false)), ("permissions.ask", text("[]"))]),
        managed(vec![(COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, flag(true)), (sandbox, flag(true))]),
        managed(vec![(COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY, flag(true)), ("permissions.allow", text("[]"))]),
        managed(vec![(sandbox, flag(false)), ("permissions.deny", text("[\"Shell\"]"))]),
        managed(vec![(sandbox, flag(false)), ("permissions.deny", text("[]"))]),
        single(sandbox, text("true")),
        managed(vec![(COPILOT_SANDBOX_MCP_SERVERS_KEY, flag(true)), (COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, text("[]"))]),
    ];
    let results: Vec<bool> = values.into_iter().map(|bag| requires_copilot_agent_host(&BagService(bag), &*configuration_service)).collect();
    assert_eq!(results, vec![false, true, false, false, true, false, false, true, false, true, false, false, false]);
}

#[test]
fn only_a_managed_true_local_escape_hatch_overrides_required_sandbox_and_permission_routing() {
    let configuration_service = TestConfigurationService::new(None);
    let key = COPILOT_TEMPORARY_DEFAULT_TO_LOCAL_HARNESS_KEY;
    let declarations = collect_managed_settings_definitions(&[policy("ChatTemporaryDefaultToLocalHarness", PolicyDefinitionType::Boolean, Some(declaration(key, "boolean")))]);
    let values = [Dynamic::Undefined, Dynamic::Bool(false), Dynamic::Bool(true), s("true")];
    let results: Vec<(ManagedSettingsData, bool)> = values
        .into_iter()
        .map(|value| {
            let normalized = normalize(&nested("temporaryDefaultToLocalHarness", value));
            let projected = project(&normalized, &declarations);
            let mut bag = projected.clone();
            bag.insert(COPILOT_SANDBOX_ENABLED_KEY.to_owned(), flag(true));
            bag.insert("permissions.deny".to_owned(), text("[\"Shell\"]"));
            (projected, requires_copilot_agent_host(&BagService(bag), &*configuration_service))
        })
        .collect();
    assert_eq!(
        results,
        vec![
            (ManagedSettingsData::new(), true),
            (single(key, flag(false)), true),
            (single(key, flag(true)), false),
            (ManagedSettingsData::new(), true),
        ]
    );
}

#[test]
fn only_boolean_false_prohibits_local_without_permission_or_sandbox_requirements() {
    let configuration = TestConfigurationService::new(None);
    let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
    let values = [Dynamic::Undefined, Dynamic::Bool(false), Dynamic::Bool(true), s("false"), n(0.0), Dynamic::Null];
    let results: Vec<(ManagedSettingsData, bool)> = values
        .into_iter()
        .map(|value| {
            let bag = normalize(&nested("temporaryAllowLocalHarness", value));
            let projected = project(&bag, &MANAGED_SETTINGS_CONTROL_DEFINITIONS);
            (projected, requires_copilot_agent_host(&BagService(bag), &*configuration))
        })
        .collect();
    assert_eq!(
        results,
        vec![
            (ManagedSettingsData::new(), false),
            (single(key, flag(false)), true),
            (single(key, flag(true)), false),
            (ManagedSettingsData::new(), false),
            (ManagedSettingsData::new(), false),
            (ManagedSettingsData::new(), false),
        ]
    );
    assert_eq!(MANAGED_SETTINGS_CONTROL_DEFINITIONS.get(key), Some(&managed_setting_definition(ManagedSettingType::Boolean)));
}

#[test]
fn temporary_local_permission_uses_native_mdm_over_server_over_file_precedence() {
    let key = COPILOT_TEMPORARY_ALLOW_LOCAL_HARNESS_KEY;
    let bag = |value: bool| single(key, flag(value));
    let empty = ManagedSettingsData::new();
    assert_eq!(
        vec![
            pick_managed_settings(Some(&bag(false)), Some(&bag(true)), Some(&bag(true))).values,
            pick_managed_settings(Some(&empty), Some(&bag(false)), Some(&bag(true))).values,
            pick_managed_settings(Some(&empty), Some(&empty), Some(&bag(true))).values,
        ],
        vec![bag(false), bag(false), bag(true)]
    );
}

/// O `definitions` compartilhado dos casos de coleta e projeção.
fn shared_definitions() -> Vec<(String, PolicyDefinitionRef)> {
    vec![
        policy("PolicyA", PolicyDefinitionType::Boolean, Some(declaration("permissions.disableBypassPermissionsMode", "string"))),
        policy(
            "PolicyB",
            PolicyDefinitionType::Number,
            Some(obj(vec![("limits.maxFoo", j("{'type':'number'}")), ("flags.enableBar", j("{'type':'boolean'}"))])),
        ),
        policy("PolicyC", PolicyDefinitionType::String, None),
    ]
}

#[test]
fn collect_managed_settings_definitions_aggregates_declarations_across_all_policies() {
    assert_eq!(
        collect_managed_settings_definitions(&shared_definitions()),
        definitions_of(vec![
            ("permissions.disableBypassPermissionsMode", ManagedSettingType::String),
            ("limits.maxFoo", ManagedSettingType::Number),
            ("flags.enableBar", ManagedSettingType::Boolean),
        ])
    );
}

#[test]
fn collect_managed_settings_definitions_returns_empty_when_nothing_is_declared() {
    assert_eq!(collect_managed_settings_definitions(&[policy("P", PolicyDefinitionType::String, None)]), IManagedSettingsPolicyDefinitions::new());
}

#[test]
fn identity_capture_preserves_explicit_false_drops_invalid_types_and_leaves_omission_unset() {
    let key = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    let declarations = collect_managed_settings_definitions(&[policy("CopilotOtelCaptureIdentity", PolicyDefinitionType::Boolean, Some(declaration(key, "boolean")))]);
    let value = managed_setting_value(key);
    assert_eq!(project(&single(key, flag(false)), &declarations), single(key, flag(false)), "denied");
    assert_eq!(project(&single(key, flag(true)), &declarations), single(key, flag(true)), "allowed");
    assert_eq!(project(&single(key, text("false")), &declarations), ManagedSettingsData::new(), "invalid");
    assert_eq!(value.call(&with_bag(single("telemetry.lockCaptureContent", flag(true)))), None, "absent");
    assert_eq!(value.call(&with_bag(managed(vec![(key, flag(false)), ("telemetry.captureContent", flag(true))]))), Some(flag(false)), "explicitFalse");
}

#[test]
fn uses_ordinary_flattening_and_projection_for_managed_telemetry_file_paths_without_losing_sibling_controls() {
    let file_path = "telemetry.filePath";
    let declarations = definitions_of(vec![
        (file_path, ManagedSettingType::String),
        ("telemetry.enabled", ManagedSettingType::Boolean),
        ("telemetry.capture.identity", ManagedSettingType::Boolean),
        ("telemetry.resourceAttributes", ManagedSettingType::String),
    ]);
    let normalize_with = |value: Dynamic| {
        normalize(&obj(vec![(
            "telemetry",
            obj(vec![
                ("filePath", value),
                ("enabled", Dynamic::Bool(false)),
                ("capture", j("{'identity':false}")),
                ("resourceAttributes", j("{'service.namespace':'acme'}")),
            ]),
        )]))
    };
    let project_with = |value: Dynamic| project(&normalize_with(value), &declarations);
    let siblings = |extra: Vec<(&str, PolicyValue)>| {
        let mut bag = managed(extra);
        bag.extend(managed(vec![
            ("telemetry.enabled", flag(false)),
            ("telemetry.capture.identity", flag(false)),
            ("telemetry.resourceAttributes", text("{\"service.namespace\":\"acme\"}")),
        ]));
        bag
    };

    assert_eq!(project_with(s("/var/log/copilot.jsonl")), siblings(vec![(file_path, text("/var/log/copilot.jsonl"))]), "string");
    assert_eq!(project_with(Dynamic::Null), siblings(vec![]), "null");
    assert_eq!(normalize_with(j("{'path':'/var/log/copilot.jsonl'}")), siblings(vec![("telemetry.filePath.path", text("/var/log/copilot.jsonl"))]), "object");
    assert_eq!(project_with(j("{'path':'/var/log/copilot.jsonl'}")), siblings(vec![]), "objectProjected");
    assert_eq!(project_with(arr(vec![s("/var/log/copilot.jsonl")])), siblings(vec![]), "array");

    let warnings = RefCell::new(Vec::<String>::new());
    let on_warn = |warning: &str| warnings.borrow_mut().push(warning.to_owned());
    assert_eq!(project_managed_settings(&normalize_with(n(1.0)), &declarations, Some(&on_warn)), siblings(vec![]), "number");
    assert_eq!(project(&normalize(&j("{'telemetry':{'enabled':false}}")), &declarations), single("telemetry.enabled", flag(false)), "missing");
    assert_eq!(*warnings.borrow(), vec!["Ignoring managed setting \"telemetry.filePath\": expected string, got number".to_owned()], "numberWarnings");
}

#[test]
fn has_managed_settings_definitions_detects_whether_any_policy_declares_a_managed_key() {
    assert!(has_managed_settings_definitions(&shared_definitions()), "withKeys");
    assert!(!has_managed_settings_definitions(&[policy("P", PolicyDefinitionType::String, None)]), "none");
    assert!(!has_managed_settings_definitions(&[]), "empty");
}

#[test]
fn managed_setting_value_locks_to_the_managed_value_when_set_else_undefined() {
    let value = managed_setting_value("permissions.disableBypassPermissionsMode");
    assert_eq!(value.call(&with_bag(single("permissions.disableBypassPermissionsMode", text("disable")))), Some(text("disable")), "set");
    assert_eq!(value.call(&with_bag(single("other.key", text("x")))), None, "otherKey");
    assert_eq!(value.call(&policy_data(None, None, None)), None, "noBag");
}

#[test]
fn managed_setting_value_returns_the_same_memoized_callback_per_key_stable_reference_identity() {
    assert_eq!(managed_setting_value("permissions.disableBypassPermissionsMode"), managed_setting_value("permissions.disableBypassPermissionsMode"));
    assert_ne!(managed_setting_value("permissions.disableBypassPermissionsMode"), managed_setting_value("some.other.key"));
}

#[test]
fn managed_model_value_prefers_the_top_level_key_falls_back_to_the_legacy_nested_key() {
    let value = managed_model_value();
    let call = |entries: Vec<(&str, PolicyValue)>| value.call(&with_bag(managed(entries)));
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, text("opus")), (COPILOT_MODEL_KEY, text("gemini"))]), Some(text("opus")), "bothPresent");
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, text("opus"))]), Some(text("opus")), "topLevelOnly");
    assert_eq!(call(vec![(COPILOT_MODEL_KEY, text("gemini"))]), Some(text("gemini")), "legacyOnly");
    assert_eq!(call(vec![("other.key", text("x"))]), None, "neither");
    assert_eq!(value.call(&policy_data(None, None, None)), None, "noBag");
}

#[test]
fn managed_model_value_trims_values_and_treats_a_blank_top_level_value_as_unset_falls_through_to_legacy() {
    let value = managed_model_value();
    let call = |entries: Vec<(&str, PolicyValue)>| value.call(&with_bag(managed(entries)));
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, text("  opus  "))]), Some(text("opus")), "trimsTopLevel");
    assert_eq!(call(vec![(COPILOT_MODEL_KEY, text("  gemini  "))]), Some(text("gemini")), "trimsLegacy");
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, text("   ")), (COPILOT_MODEL_KEY, text("gemini"))]), Some(text("gemini")), "blankTopLevelFallsBack");
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, text("   ")), (COPILOT_MODEL_KEY, text("  "))]), None, "bothBlank");
    assert_eq!(call(vec![(COPILOT_TOP_LEVEL_MODEL_KEY, PolicyValue::Number(42.0))]), None, "nonString");
}

#[test]
fn managed_model_value_returns_the_same_memoized_callback_stable_reference_identity() {
    assert_eq!(managed_model_value(), managed_model_value());
}

#[test]
fn managed_settings_disabled_value_forces_false_only_while_managed_settings_are_active() {
    assert_eq!(managed_settings_disabled_value(&policy_data(None, Some(true), None)), Some(flag(false)), "active");
    assert_eq!(managed_settings_disabled_value(&policy_data(None, Some(false), None)), None, "inactive");
    assert_eq!(managed_settings_disabled_value(&policy_data(None, None, None)), None, "unset");
    assert_eq!(managed_settings_disabled_value(&policy_data(None, None, Some(false))), None, "previewFeaturesDisabled");
}

#[test]
fn force_remote_settings_refresh_resolves_across_all_channels_and_reports_the_winning_source() {
    let key = COPILOT_FORCE_REMOTE_SETTINGS_REFRESH_KEY;
    let bag = |value: PolicyValue| single(key, value);
    let effective = |source| IForceRemoteSettingsRefreshResolution::Effective { source };
    let not_effective = IForceRemoteSettingsRefreshResolution::NotEffective;
    assert_eq!(resolve_force_remote_settings_refresh(None, Some(&bag(flag(true))), None), effective(Server), "serverTrue");
    assert_eq!(resolve_force_remote_settings_refresh(Some(&bag(flag(true))), Some(&bag(flag(false))), None), effective(NativeMdm), "nativeTrue");
    assert_eq!(resolve_force_remote_settings_refresh(Some(&bag(flag(false))), Some(&bag(flag(true))), None), not_effective, "nativeFalse");
    // A malformed value is treated as absent so it cannot mask a lower-precedence channel.
    assert_eq!(resolve_force_remote_settings_refresh(Some(&bag(text("true"))), Some(&bag(flag(true))), None), effective(Server), "malformedNative");
    // The file channel participates; a native/server-only resolver ignored it.
    assert_eq!(resolve_force_remote_settings_refresh(None, None, Some(&bag(flag(true)))), effective(File), "fileTrue");
    assert_eq!(resolve_force_remote_settings_refresh(None, Some(&bag(flag(false))), Some(&bag(flag(true)))), not_effective, "serverBeatsFile");
    assert_eq!(resolve_force_remote_settings_refresh(None, None, None), not_effective, "unset");
}

#[test]
fn project_managed_settings_keeps_declared_typed_keys_drops_undeclared_and_type_mismatched() {
    let projected = project(
        &managed(vec![
            ("permissions.disableBypassPermissionsMode", text("disable")), // declared string -> kept
            ("limits.maxFoo", PolicyValue::Number(5.0)),                   // declared number -> kept
            ("flags.enableBar", text("true")),                             // declared boolean, got string -> dropped
            ("unknown.key", text("x")),                                    // undeclared -> dropped
        ]),
        &collect_managed_settings_definitions(&shared_definitions()),
    );
    assert_eq!(projected, managed(vec![("permissions.disableBypassPermissionsMode", text("disable")), ("limits.maxFoo", PolicyValue::Number(5.0))]));
}

#[test]
fn project_managed_settings_validates_without_coercing_string_stays_a_string() {
    assert_eq!(
        project(&single("permissions.disableBypassPermissionsMode", text("false")), &definitions_of(vec![("permissions.disableBypassPermissionsMode", ManagedSettingType::String)])),
        single("permissions.disableBypassPermissionsMode", text("false")),
    );
}

#[test]
fn project_managed_settings_warns_once_per_type_mismatch() {
    let warnings = RefCell::new(Vec::<String>::new());
    let on_warn = |message: &str| warnings.borrow_mut().push(message.to_owned());
    project_managed_settings(&single("flags.enableBar", text("true")), &definitions_of(vec![("flags.enableBar", ManagedSettingType::Boolean)]), Some(&on_warn));
    assert_eq!(warnings.borrow().len(), 1);
}

#[test]
fn normalizes_and_projects_managed_sandbox_allowlists_including_an_explicitly_empty_list() {
    let key = COPILOT_SANDBOX_ALLOWED_HOSTS_KEY;
    assert_eq!(MANAGED_SETTINGS_CONTROL_DEFINITIONS.get(key), Some(&managed_setting_definition(ManagedSettingType::String)), "definition");
    let hosts = [Dynamic::Undefined, arr(vec![]), arr(vec![s("example.com"), s("*.example.com")])];
    let values: Vec<ManagedSettingsData> = hosts
        .into_iter()
        .map(|allowed_hosts| project(&normalize(&nest(&["sandbox", "userPolicy", "network"], obj(vec![("allowedHosts", allowed_hosts)]))), &MANAGED_SETTINGS_CONTROL_DEFINITIONS))
        .collect();
    assert_eq!(values, vec![ManagedSettingsData::new(), single(key, text("[]")), single(key, text("[\"example.com\",\"*.example.com\"]"))], "values");
    assert_eq!(project(&single(key, text("[\"native.example\"]")), &MANAGED_SETTINGS_CONTROL_DEFINITIONS), single(key, text("[\"native.example\"]")), "native");
    let selected = pick_managed_settings(
        Some(&single(key, text("[]"))),
        Some(&single(key, text("[\"server.example\"]"))),
        Some(&single(key, text("[\"file.example\"]"))),
    )
    .values;
    assert_eq!(selected, single(key, text("[]")), "selected");
}

#[test]
fn normalizes_and_projects_managed_sandbox_filesystem_lists() {
    let values = project(
        &normalize(&nest(
            &["sandbox", "userPolicy", "filesystem"],
            obj(vec![("readwritePaths", arr(vec![s("C:\\Work")])), ("readonlyPaths", arr(vec![])), ("deniedPaths", arr(vec![s("C:\\Secrets")]))]),
        )),
        &MANAGED_SETTINGS_CONTROL_DEFINITIONS,
    );
    assert_eq!(
        values,
        managed(vec![
            (COPILOT_SANDBOX_READWRITE_PATHS_KEY, text("[\"C:\\\\Work\"]")),
            (COPILOT_SANDBOX_READONLY_PATHS_KEY, text("[]")),
            (COPILOT_SANDBOX_DENIED_PATHS_KEY, text("[\"C:\\\\Secrets\"]")),
        ])
    );
}

// --- suíte: Copilot managed settings precedence (pickManagedSettings) ---

#[test]
fn telemetry_selects_one_whole_block() {
    let highers = [
        "{}",
        "{'capture':{}}",
        "{'endpoint':'https://higher.example'}",
        "{'capture':{'identity':false}}",
        "{'capture':{'identity':true}}",
        "{'futureControl':{}}",
        "{'futureControl':['unknown']}",
        "{'futureControl':true}",
        "{'resourceAttributes':{}}",
        "{'resourceAttributes':{'host.name':'higher-host'},'headers':{'authorization':'higher'}}",
    ];
    let identity = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    for higher in highers {
        for lower_identity in [false, true] {
            for native in [false, true] {
                let selected = normalize(&obj(vec![("telemetry", j(higher))]));
                // Native delivery observes declared flat keys, not normalized empty/unknown object blocks.
                if native && selected.keys().any(|key| !["telemetry.endpoint", identity, "telemetry.resourceAttributes", "telemetry.headers"].contains(&key.as_str())) {
                    continue;
                }
                let label = format!("{} block: {higher}, lower identity {lower_identity}", if native { "native" } else { "server" });
                let lower = normalize(&j(&format!(
                    "{{'telemetry':{{'enabled':true,'capture':{{'identity':{lower_identity}}},'captureContent':true,'lockCaptureContent':true,'endpoint':'https://lower.example','protocol':'grpc','serviceName':'lower','resourceAttributes':{{'host.name':'lower-host','extra':'lower'}},'headers':{{'authorization':'lower'}}}}}}"
                )));
                let pick = if native {
                    pick_managed_settings(Some(&selected), Some(&lower), Some(&lower))
                } else {
                    pick_managed_settings(None, Some(&selected), Some(&lower))
                };
                let projected = project(&pick.values, &definitions_of(vec![(identity, ManagedSettingType::Boolean)]));
                let expected_projected = match selected.get(identity) {
                    Some(PolicyValue::Boolean(value)) => single(identity, flag(*value)),
                    _ => ManagedSettingsData::new(),
                };
                assert_eq!(pick.values, selected, "values: {label}");
                assert_eq!(pick.active_sources, vec![if native { NativeMdm } else { Server }], "activeSources: {label}");
                assert_eq!(projected, expected_projected, "projected: {label}");
            }
        }
    }
}

#[test]
fn absent_telemetry_blocks_fall_through_without_changing_unrelated_per_key_or_sandbox_resolution() {
    let file = normalize(&j("{'telemetry':{'capture':{'identity':true}}}"));
    let mut file_bag = file.clone();
    file_bag.insert("fileOnly".to_owned(), flag(true));
    file_bag.insert(COPILOT_SANDBOX_ENABLED_KEY.to_owned(), flag(true));
    let pick = pick_managed_settings(
        Some(&managed(vec![("model", text("native")), (COPILOT_SANDBOX_ENABLED_KEY, flag(false))])),
        Some(&managed(vec![("model", text("server")), ("serverOnly", flag(true))])),
        Some(&file_bag),
    );
    assert_eq!(
        pick.values,
        managed(vec![
            ("model", text("native")),
            ("serverOnly", flag(true)),
            ("fileOnly", flag(true)),
            (COPILOT_SANDBOX_ENABLED_KEY, flag(true)),
            (COPILOT_OTEL_CAPTURE_IDENTITY_KEY, flag(true)),
        ])
    );
}

#[test]
fn flat_native_telemetry_selects_the_block_and_keeps_contested_key_provenance() {
    let key = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    let pick = pick_managed_settings(Some(&single(key, flag(false))), Some(&managed(vec![(key, flag(true)), ("telemetry.enabled", flag(true))])), None);
    let expected = IManagedSettingsPick {
        values: single(key, flag(false)),
        resolutions: [(key.to_owned(), resolution(flag(false), NativeMdm, vec![contribution(NativeMdm, flag(false)), contribution(Server, flag(true))]))].into_iter().collect(),
        suppressed_telemetry: [("telemetry.enabled".to_owned(), vec![contribution(Server, flag(true))])].into_iter().collect(),
        active_sources: vec![NativeMdm],
    };
    assert_eq!(pick, expected);
}

#[test]
fn an_omitted_malformed_file_path_leaves_the_existing_higher_telemetry_controls_authoritative() {
    let pick = pick_managed_settings(
        None,
        Some(&normalize(&j("{'telemetry':{'filePath':null,'enabled':false}}"))),
        Some(&normalize(&j("{'telemetry':{'endpoint':'https://collector.example','enabled':true}}"))),
    );
    assert_eq!(pick.values, single("telemetry.enabled", flag(false)));
}

#[test]
fn retains_excluded_telemetry_provenance_without_applying_lower_only_leaves_or_activating_their_sources() {
    let key = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    let pick = pick_managed_settings(
        Some(&single("telemetry.capture.prompts", flag(false))),
        Some(&managed(vec![(key, flag(false)), ("telemetry.endpoint", text("server-endpoint"))])),
        Some(&managed(vec![(key, flag(true)), ("telemetry.endpoint", text("file-endpoint"))])),
    );
    assert_eq!(pick.values, single("telemetry.capture.prompts", flag(false)), "values");
    assert_eq!(pick.resolutions.keys().cloned().collect::<Vec<_>>(), vec!["telemetry.capture.prompts".to_owned()], "resolvedKeys");
    assert_eq!(
        pick.suppressed_telemetry.iter().map(|(key, contributions)| (key.clone(), contributions.clone())).collect::<Vec<_>>(),
        vec![
            (key.to_owned(), vec![contribution(Server, flag(false)), contribution(File, flag(true))]),
            ("telemetry.endpoint".to_owned(), vec![contribution(Server, text("server-endpoint")), contribution(File, text("file-endpoint"))]),
        ],
        "suppressed"
    );
    assert_eq!(pick.active_sources, vec![NativeMdm], "activeSources");
}

#[test]
fn an_empty_server_telemetry_block_preserves_excluded_file_provenance_separately_from_policy_values() {
    let key = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    let pick = pick_managed_settings(None, Some(&normalize(&j("{'telemetry':{}}"))), Some(&single(key, flag(true))));
    assert_eq!(pick.values, single("telemetry", text("{}")), "values");
    assert_eq!(project(&pick.values, &definitions_of(vec![(key, ManagedSettingType::Boolean)])), ManagedSettingsData::new(), "projected");
    assert_eq!(pick.resolutions.get("telemetry"), Some(&resolution(text("{}"), Server, vec![contribution(Server, text("{}"))])), "marker");
    assert_eq!(
        pick.suppressed_telemetry.iter().map(|(key, contributions)| (key.clone(), contributions.clone())).collect::<Vec<_>>(),
        vec![(key.to_owned(), vec![contribution(File, flag(true))])],
        "suppressed"
    );
}

#[test]
fn diagnostic_provenance_does_not_change_effective_policy_or_unrelated_precedence_across_telemetry_lifecycle_states() {
    let identity = COPILOT_OTEL_CAPTURE_IDENTITY_KEY;
    let enabled = "telemetry.enabled";
    let sandbox = COPILOT_SANDBOX_ENABLED_KEY;
    let server_telemetry = vec![(identity, flag(true)), (enabled, flag(true))];
    let unrelated = vec![("shared", text("native")), ("serverOnly", text("server")), ("fileOnly", text("file")), (sandbox, flag(true))];
    // (name, telemetry nativo, esperado, suprimidas)
    let states: Vec<(&str, Vec<(&str, PolicyValue)>, Vec<(&str, PolicyValue)>, Vec<&str>)> = vec![
        ("absent", vec![], server_telemetry.clone(), vec![]),
        ("managed false", vec![(identity, flag(false))], vec![(identity, flag(false))], vec![enabled]),
        ("managed true", vec![(identity, flag(true))], vec![(identity, flag(true))], vec![enabled]),
        ("identity omitted", vec![("telemetry.capture.prompts", flag(false))], vec![("telemetry.capture.prompts", flag(false))], vec![identity, enabled]),
        ("withdrawn", vec![], server_telemetry.clone(), vec![]),
    ];
    for (name, telemetry, expected, suppressed) in states {
        let mut native = managed(vec![("shared", text("native")), (sandbox, flag(false))]);
        native.extend(managed(telemetry));
        let mut server = managed(vec![("shared", text("server")), ("serverOnly", text("server"))]);
        server.extend(managed(server_telemetry.clone()));
        let file = managed(vec![("shared", text("file")), ("fileOnly", text("file")), (sandbox, flag(true)), (identity, flag(false)), (enabled, flag(true))]);
        let pick = pick_managed_settings(Some(&native), Some(&server), Some(&file));

        let mut expected_values = managed(unrelated.clone());
        expected_values.extend(managed(expected));
        let resolution_values: ManagedSettingsData = pick.resolutions.iter().map(|(key, resolution)| (key.clone(), resolution.value.clone())).collect();
        let mut suppressed_keys: Vec<String> = pick.suppressed_telemetry.keys().cloned().collect();
        suppressed_keys.sort();

        assert_eq!(pick.values, expected_values, "values: {name}");
        assert_eq!(resolution_values, expected_values, "resolutionValues: {name}");
        assert_eq!(suppressed_keys, suppressed.iter().map(|key| key.to_string()).collect::<Vec<_>>(), "suppressed: {name}");
        assert_eq!(pick.active_sources, vec![NativeMdm, Server, File], "activeSources: {name}");
        assert_eq!(pick.resolutions.get("shared").map(|resolution| resolution.source), Some(NativeMdm), "sharedSource: {name}");
        assert_eq!(pick.resolutions.get(sandbox).map(|resolution| resolution.source), Some(File), "sandboxSource: {name}");
    }
}

#[test]
fn removing_a_higher_telemetry_block_reveals_the_lower_block_replacing_it_with_empty_does_not() {
    let lower = normalize(&j("{'telemetry':{'capture':{'identity':true}}}"));
    let denied = pick_managed_settings(None, Some(&normalize(&j("{'telemetry':{'capture':{'identity':false}}}"))), Some(&lower)).values;
    let empty = pick_managed_settings(None, Some(&normalize(&j("{'telemetry':{}}"))), Some(&lower)).values;
    let withdrawn = pick_managed_settings(None, Some(&normalize(&j("{}"))), Some(&lower)).values;
    assert_eq!(denied, single(COPILOT_OTEL_CAPTURE_IDENTITY_KEY, flag(false)), "denied");
    assert_eq!(empty, single("telemetry", text("{}")), "empty");
    assert_eq!(withdrawn, lower, "withdrawn");
}

#[test]
fn managed_sandbox_enablement_is_force_on_wins_across_every_channel_combination() {
    let key = COPILOT_SANDBOX_ENABLED_KEY;
    for native in TRI_STATE {
        for server in TRI_STATE {
            for file in TRI_STATE {
                let pick = pick_flags(key, native, server, file);
                let enabled = [native, server, file].contains(&Some(true));
                let expected = if enabled { Some(true) } else { native.or(server).or(file) };
                assert_eq!(pick.values.get(key), expected.map(flag).as_ref(), "{}", combination_label(native, server, file));
            }
        }
    }
}

#[test]
fn sandbox_force_on_reports_the_restrictive_source_while_retaining_all_contributions() {
    let key = COPILOT_SANDBOX_ENABLED_KEY;
    let pick = pick_flags(key, Some(false), Some(true), Some(true));
    let expected = IManagedSettingsPick {
        values: single(key, flag(true)),
        resolutions: [(key.to_owned(), resolution(flag(true), Server, vec![contribution(NativeMdm, flag(false)), contribution(Server, flag(true)), contribution(File, flag(true))]))]
            .into_iter()
            .collect(),
        suppressed_telemetry: Default::default(),
        active_sources: vec![Server],
    };
    assert_eq!(pick, expected);
}

#[test]
fn sandbox_servers_are_force_on_wins_across_all_managed_channels() {
    for (field, key) in [("sandboxMcpServers", COPILOT_SANDBOX_MCP_SERVERS_KEY), ("sandboxLspServers", COPILOT_SANDBOX_LSP_SERVERS_KEY)] {
        for native in TRI_STATE {
            for server in TRI_STATE {
                for file in TRI_STATE {
                    let pick = pick_flags(key, native, server, file);
                    let expected = if [native, server, file].contains(&Some(true)) { Some(true) } else { native.or(server).or(file) };
                    assert_eq!(pick.values.get(key), expected.map(flag).as_ref(), "{field} {}", combination_label(native, server, file));
                }
            }
        }
    }
}

#[test]
fn sandbox_servers_project_booleans_and_restore_the_remaining_policy_after_removal() {
    for (field, key) in [("sandboxMcpServers", COPILOT_SANDBOX_MCP_SERVERS_KEY), ("sandboxLspServers", COPILOT_SANDBOX_LSP_SERVERS_KEY)] {
        let pick = pick_flags(key, Some(false), Some(true), Some(false));
        let projected: Vec<ManagedSettingsData> = [Dynamic::Bool(true), Dynamic::Bool(false), s("true")]
            .into_iter()
            .map(|value| project(&normalize(&obj(vec![("sandbox", obj(vec![(field, value)]))])), &MANAGED_SETTINGS_CONTROL_DEFINITIONS))
            .collect();
        assert_eq!(projected, vec![single(key, flag(true)), single(key, flag(false)), ManagedSettingsData::new()], "projected {field}");
        assert_eq!(pick.values.get(key), Some(&flag(true)), "value {field}");
        assert_eq!(
            pick.resolutions.get(key),
            Some(&resolution(flag(true), Server, vec![contribution(NativeMdm, flag(false)), contribution(Server, flag(true)), contribution(File, flag(false))])),
            "resolution {field}"
        );
        assert_eq!(pick_flags(key, Some(false), None, Some(false)).values.get(key), Some(&flag(false)), "removed {field}");
        let malformed = pick_managed_settings(Some(&single(key, text("false"))), Some(&single(key, flag(true))), None);
        assert_eq!(malformed.values.get(key), Some(&flag(true)), "malformed {field}");
    }
}

/// As seis chaves deny-wins.
const DENY_WINS_KEYS: [&str; 6] = [
    COPILOT_SANDBOX_ALLOW_BYPASS_KEY,
    COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY,
    COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY,
    COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY,
    COPILOT_SANDBOX_AUTH_GIT_KEY,
    COPILOT_SANDBOX_AUTH_GH_KEY,
];

#[test]
fn deny_wins_keys_are_deny_wins_across_every_channel_combination() {
    for key in DENY_WINS_KEYS {
        for native in TRI_STATE {
            for server in TRI_STATE {
                for file in TRI_STATE {
                    let pick = pick_flags(key, native, server, file);
                    let denied = [native, server, file].contains(&Some(false));
                    let expected = if denied { Some(false) } else { native.or(server).or(file) };
                    assert_eq!(pick.values.get(key), expected.map(flag).as_ref(), "{key} {}", combination_label(native, server, file));
                }
            }
        }
    }
}

#[test]
fn deny_wins_keys_report_the_restrictive_source_and_restore_the_remaining_value_after_removal() {
    for key in DENY_WINS_KEYS {
        let pick = pick_flags(key, Some(true), Some(false), Some(true));
        assert_eq!(pick.values.get(key), Some(&flag(false)), "value {key}");
        assert_eq!(
            pick.resolutions.get(key),
            Some(&resolution(flag(false), Server, vec![contribution(NativeMdm, flag(true)), contribution(Server, flag(false)), contribution(File, flag(true))])),
            "resolution {key}"
        );
        assert_eq!(pick.active_sources, vec![Server], "activeSources {key}");
        assert_eq!(pick_flags(key, Some(true), None, Some(true)).values.get(key), Some(&flag(true)), "removed {key}");
        let malformed = pick_managed_settings(Some(&single(key, text("true"))), Some(&single(key, flag(false))), None);
        assert_eq!(malformed.values.get(key), Some(&flag(false)), "malformed {key}");
    }
}

#[test]
fn outbound_network_access_is_projected_from_the_canonical_boolean_managed_setting() {
    let key = COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY;
    let project_value = |value: Dynamic| project(&normalize(&nest(&["sandbox", "userPolicy", "network"], obj(vec![("allowOutbound", value)]))), &MANAGED_SETTINGS_CONTROL_DEFINITIONS);
    assert_eq!(project_value(Dynamic::Bool(false)), single(key, flag(false)), "denied");
    assert_eq!(project_value(Dynamic::Bool(true)), single(key, flag(true)), "allowed");
    assert_eq!(project_value(s("false")), ManagedSettingsData::new(), "invalid");
}

#[test]
fn sandbox_developer_tool_access_projects_only_the_canonical_boolean_values() {
    let key = COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY;
    let results: Vec<ManagedSettingsData> = [Dynamic::Bool(false), Dynamic::Bool(true), s("false")]
        .into_iter()
        .map(|value| project(&normalize(&nest(&["sandbox"], obj(vec![("allowDevToolAccess", value)]))), &MANAGED_SETTINGS_CONTROL_DEFINITIONS))
        .collect();
    assert_eq!(results, vec![single(key, flag(false)), single(key, flag(true)), ManagedSettingsData::new()]);
}

#[test]
fn sandbox_authentication_projects_only_canonical_nested_boolean_values() {
    for (field, key) in [("git", COPILOT_SANDBOX_AUTH_GIT_KEY), ("gh", COPILOT_SANDBOX_AUTH_GH_KEY)] {
        let results: Vec<ManagedSettingsData> = [Dynamic::Bool(false), Dynamic::Bool(true), s("false")]
            .into_iter()
            .map(|value| project(&normalize(&nest(&["sandbox", "auth"], obj(vec![(field, value)]))), &MANAGED_SETTINGS_CONTROL_DEFINITIONS))
            .collect();
        assert_eq!(results, vec![single(key, flag(false)), single(key, flag(true)), ManagedSettingsData::new()], "{field}");
    }
}

#[test]
fn sandbox_local_network_access_projects_only_canonical_nested_boolean_values() {
    let key = COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY;
    let results: Vec<ManagedSettingsData> = [Dynamic::Bool(false), Dynamic::Bool(true), s("false")]
        .into_iter()
        .map(|value| project(&normalize(&nest(&["sandbox", "userPolicy", "network"], obj(vec![("allowLocalNetwork", value)]))), &MANAGED_SETTINGS_CONTROL_DEFINITIONS))
        .collect();
    assert_eq!(results, vec![single(key, flag(false)), single(key, flag(true)), ManagedSettingsData::new()]);
}

#[test]
fn malformed_higher_precedence_sandbox_values_cannot_mask_a_managed_force_on() {
    let key = COPILOT_SANDBOX_ENABLED_KEY;
    let string = pick_managed_settings(Some(&single(key, text("false"))), Some(&single(key, flag(true))), None);
    let number = pick_managed_settings(Some(&single(key, PolicyValue::Number(0.0))), Some(&single(key, flag(false))), Some(&single(key, flag(true))));
    assert_eq!(string.values.get(key), Some(&flag(true)), "string");
    assert_eq!(number.values.get(key), Some(&flag(true)), "number");
}

#[test]
fn distinct_keys_each_win_from_their_highest_precedence_channel_a_lower_channel_fills_a_gap_the_higher_ones_leave() {
    // The headline per-key behavior: `shared` is contested by all three (native wins) while
    // `nativeOnly`/`serverOnly`/`fileOnly` are each supplied by a single channel and all survive.
    let pick = pick_managed_settings(
        Some(&managed(vec![("shared", text("native")), ("nativeOnly", text("n"))])),
        Some(&managed(vec![("shared", text("server")), ("serverOnly", text("s"))])),
        Some(&managed(vec![("shared", text("file")), ("fileOnly", text("f"))])),
    );
    assert_eq!(pick.values, managed(vec![("shared", text("native")), ("nativeOnly", text("n")), ("serverOnly", text("s")), ("fileOnly", text("f"))]));
    assert_eq!(pick.active_sources, vec![NativeMdm, Server, File]);
    assert_eq!(
        pick.resolutions.get("shared"),
        Some(&resolution(text("native"), NativeMdm, vec![contribution(NativeMdm, text("native")), contribution(Server, text("server")), contribution(File, text("file"))]))
    );
}

#[test]
fn with_native_absent_the_mid_tier_server_wins_a_contested_key_over_file() {
    let pick = pick_managed_settings(None, Some(&single("k", text("server"))), Some(&single("k", text("file"))));
    assert_eq!(pick.resolutions.get("k"), Some(&resolution(text("server"), Server, vec![contribution(Server, text("server")), contribution(File, text("file"))])));
    assert_eq!(pick.active_sources, vec![Server]);
}

#[test]
fn falsy_but_present_values_are_real_contributions_and_win_over_a_lower_channel() {
    // `false`, `0` and `''` must not be mistaken for "unset": a higher channel that sets them
    // still locks the key against a lower channel's value.
    let pick = pick_managed_settings(
        Some(&managed(vec![("flag", flag(false)), ("count", PolicyValue::Number(0.0)), ("name", text(""))])),
        None,
        Some(&managed(vec![("flag", flag(true)), ("count", PolicyValue::Number(99.0)), ("name", text("lower"))])),
    );
    assert_eq!(pick.values, managed(vec![("flag", flag(false)), ("count", PolicyValue::Number(0.0)), ("name", text(""))]));
    assert_eq!(pick.active_sources, vec![NativeMdm]);
}

#[test]
fn an_explicit_undefined_hole_in_a_higher_channel_falls_through_to_a_lower_channel() {
    // O mapa não guarda `undefined`: o buraco é a chave ausente na camada de cima, que cai para a de baixo.
    let pick = pick_managed_settings(Some(&single("b", text("native"))), Some(&single("a", text("server"))), None);
    assert_eq!(pick.values, managed(vec![("b", text("native")), ("a", text("server"))]));
    assert_eq!(pick.resolutions.get("a").map(|resolution| resolution.source), Some(Server));
}

#[test]
fn the_merged_bag_is_a_fresh_object_never_an_alias_of_an_input_channel_bag() {
    // AccountPolicyService projects `pick.values` directly, relying on it not aliasing/mutating a
    // channel's bag.
    let native = single("a", text("native"));
    let mut pick = pick_managed_settings(Some(&native), None, None);
    assert_eq!(pick.values, single("a", text("native")));
    pick.values.insert("b".to_owned(), text("changed"));
    assert_eq!(native, single("a", text("native")));
}

#[test]
fn empty_absent_channels_contribute_nothing_and_active_sources_skips_a_non_contributing_middle_channel() {
    let partial = pick_managed_settings(Some(&ManagedSettingsData::new()), Some(&single("b", text("server"))), None);
    let gap = pick_managed_settings(Some(&single("x", text("n"))), None, Some(&single("y", text("f")))).active_sources;
    let all_undefined = pick_managed_settings(None, None, None);
    let empty = ManagedSettingsData::new();
    let all_empty = pick_managed_settings(Some(&empty), Some(&empty), Some(&empty));
    let nothing = IManagedSettingsPick { values: ManagedSettingsData::new(), resolutions: Default::default(), suppressed_telemetry: Default::default(), active_sources: vec![] };

    assert_eq!(
        partial,
        IManagedSettingsPick {
            values: single("b", text("server")),
            resolutions: [("b".to_owned(), resolution(text("server"), Server, vec![contribution(Server, text("server"))]))].into_iter().collect(),
            suppressed_telemetry: Default::default(),
            active_sources: vec![Server],
        },
        "partial"
    );
    // native + file contribute, server does not: activeSources must skip the gap.
    assert_eq!(gap, vec![NativeMdm, File], "gap");
    assert_eq!(all_undefined, nothing, "allUndefined");
    assert_eq!(all_empty, nothing, "allEmpty");
}

#[test]
fn a_malicious_proto_key_does_not_pollute_any_prototype_chain() {
    // Um documento com uma chave própria `__proto__` de valor objeto (o vetor clássico de poluição de
    // protótipo): a mescla o trata como uma chave comum e não cria nenhuma outra.
    let malicious = normalize(&obj(vec![("__proto__", j("{'polluted':true}"))]));
    let pick = pick_managed_settings(Some(&malicious), None, None);
    assert_eq!(pick.values, single("__proto__.polluted", flag(true)));
    assert!(!pick.values.contains_key("polluted"));
    assert!(!pick.values.contains_key("__proto__"));
}
