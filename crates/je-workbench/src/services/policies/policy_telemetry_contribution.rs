//! Porte de `src/vs/workbench/services/policies/browser/policyTelemetry.contribution.ts`: relata, por telemetria,
//! quais políticas corporativas estão efetivas, de qual fonte vieram e em que faixa de valor caíram (nunca o
//! valor cru).
//!
//! Divergências:
//! - A telemetria do porte é local (nada sai da máquina); a semântica do original é mantida sobre um
//!   [`ITelemetryService`] qualquer.
//! - `publicLog2<E, T>` (a classificação GDPR é só tipo do TypeScript) vira `public_log2` com o dado como
//!   [`Dynamic`]. As chaves do objeto (`devicePolicyCount`, `defaultModelSet`...) são o esquema do evento e
//!   ficam como no original.
//! - A assinatura `JSON.stringify(event)` do original, que só serve para comparar com a anterior, vira a
//!   igualdade estrutural de [`PolicyAppliedEvent`] (o `undefined` some do JSON; aqui é um `None` igual a si).
//! - As 21 propriedades `*Source` do evento saem de uma tabela ([`TRACKED_SOURCES`]) em vez de 21 campos.
//! - `registerWorkbenchContribution2(..., AfterRestored)` não tem equivalente: o registro de contribuições
//!   não está portado, quem monta o workbench cria a contribuição com [`PolicyTelemetryContribution::new`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::policy::{PolicyService, PolicyValue, PolicyValueSource};
use je_platform::telemetry::ITelemetryService;

/// `PolicyNames`: os nomes das políticas que o evento acompanha.
mod policy_names {
    pub const DEFAULT_MODEL: &str = "ChatDefaultModel";
    pub const TOOLS_AUTO_APPROVE: &str = "ChatToolsAutoApprove";
    pub const ENABLED_PLUGINS: &str = "ChatEnabledPlugins";
    pub const EXTRA_MARKETPLACES: &str = "ChatExtraMarketplaces";
    pub const APPROVED_ORGS: &str = "ChatApprovedAccountOrganizations";
    pub const OTEL_ENABLED: &str = "CopilotOtelEnabled";
    pub const TELEMETRY_LEVEL: &str = "TelemetryLevel";
    pub const ENABLE_FEEDBACK: &str = "EnableFeedback";
    pub const TOOLS_ELIGIBLE_FOR_AUTO_APPROVAL: &str = "ChatToolsEligibleForAutoApproval";
    pub const TOOLS_TERMINAL_ENABLE_AUTO_APPROVE: &str = "ChatToolsTerminalEnableAutoApprove";
    pub const MCP_ACCESS: &str = "ChatMCP";
    pub const ALLOWED_MCP_SERVERS: &str = "ChatAllowedMcpServers";
    pub const DENIED_MCP_SERVERS: &str = "ChatDeniedMcpServers";
    pub const ALLOW_MANAGED_MCP_SERVERS_ONLY: &str = "ChatAllowManagedMcpServersOnly";
    pub const MCP_ENTERPRISE_MANAGED_AUTH_IDP: &str = "McpEnterpriseManagedAuthIdp";
    pub const PLUGINS_ENABLED: &str = "ChatPluginsEnabled";
    pub const STRICT_PLUGIN_ONLY_CUSTOMIZATION: &str = "ChatStrictPluginOnlyCustomization";
    pub const HOOKS: &str = "ChatHooks";
    pub const ALLOW_MANAGED_HOOKS_ONLY: &str = "ChatAllowManagedHooksOnly";
    pub const AGENT_MODE: &str = "ChatAgentMode";
    pub const OTEL_PROTOCOL: &str = "CopilotOtelProtocol";
    pub const OTEL_CAPTURE_IDENTITY: &str = "CopilotOtelCaptureIdentity";
    pub const OTEL_HEADERS: &str = "CopilotOtelHeaders";
    pub const AGENT_NETWORK_FILTER: &str = "ChatAgentNetworkFilter";
    pub const AGENT_ALLOWED_NETWORK_DOMAINS: &str = "ChatAgentAllowedNetworkDomains";
    pub const AGENT_DENIED_NETWORK_DOMAINS: &str = "ChatAgentDeniedNetworkDomains";
}

use policy_names as names;

/// O nome do evento de telemetria.
pub const POLICY_APPLIED_EVENT_NAME: &str = "policy.applied";

/// O atraso do agendador que agrupa mudanças (ms).
const REPORT_DELAY_MS: u64 = 500;

/// As propriedades `*Source` do evento, na ordem do original: (chave no evento, política).
const TRACKED_SOURCES: [(&str, &str); 21] = [
    ("toolsEligibleForAutoApprovalSource", names::TOOLS_ELIGIBLE_FOR_AUTO_APPROVAL),
    ("toolsTerminalEnableAutoApproveSource", names::TOOLS_TERMINAL_ENABLE_AUTO_APPROVE),
    ("mcpAccessSource", names::MCP_ACCESS),
    ("allowedMcpServersSource", names::ALLOWED_MCP_SERVERS),
    ("deniedMcpServersSource", names::DENIED_MCP_SERVERS),
    ("allowManagedMcpServersOnlySource", names::ALLOW_MANAGED_MCP_SERVERS_ONLY),
    ("mcpEnterpriseManagedAuthIdpSource", names::MCP_ENTERPRISE_MANAGED_AUTH_IDP),
    ("pluginsEnabledSource", names::PLUGINS_ENABLED),
    ("enabledPluginsSource", names::ENABLED_PLUGINS),
    ("extraMarketplacesSource", names::EXTRA_MARKETPLACES),
    ("strictPluginOnlyCustomizationSource", names::STRICT_PLUGIN_ONLY_CUSTOMIZATION),
    ("hooksSource", names::HOOKS),
    ("allowManagedHooksOnlySource", names::ALLOW_MANAGED_HOOKS_ONLY),
    ("agentModeSource", names::AGENT_MODE),
    ("otelEnabledSource", names::OTEL_ENABLED),
    ("otelProtocolSource", names::OTEL_PROTOCOL),
    ("otelCaptureIdentitySource", names::OTEL_CAPTURE_IDENTITY),
    ("otelHeadersSource", names::OTEL_HEADERS),
    ("agentNetworkFilterSource", names::AGENT_NETWORK_FILTER),
    ("agentAllowedNetworkDomainsSource", names::AGENT_ALLOWED_NETWORK_DOMAINS),
    ("agentDeniedNetworkDomainsSource", names::AGENT_DENIED_NETWORK_DOMAINS),
];

/// O valor de texto de um `PolicyValueSource` (o `const enum` do original é de strings).
pub fn policy_value_source_name(source: PolicyValueSource) -> &'static str {
    match source {
        PolicyValueSource::Device => "device",
        PolicyValueSource::NativeMdm => "nativeMdm",
        PolicyValueSource::ServerManagedSettings => "serverManagedSettings",
        PolicyValueSource::FileManagedSettings => "fileManagedSettings",
        PolicyValueSource::MixedManagedSettings => "mixedManagedSettings",
        PolicyValueSource::Account => "account",
        PolicyValueSource::AccountGate => "accountGate",
    }
}

/// `PolicySource`: `PolicyValueSource | 'none'`, com `None` no lugar de `'none'`.
type PolicySource = Option<PolicyValueSource>;

fn policy_source_name(source: PolicySource) -> &'static str {
    source.map_or("none", policy_value_source_name)
}

/// `PolicyAppliedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyAppliedEvent {
    pub device_policy_count: u32,
    pub native_mdm_policy_count: u32,
    pub server_managed_settings_policy_count: u32,
    pub file_managed_settings_policy_count: u32,
    pub mixed_managed_settings_policy_count: u32,
    pub account_policy_count: u32,
    pub account_gate_policy_count: u32,
    pub default_model_set: bool,
    pub tools_auto_approve_set: bool,
    pub enabled_plugins_set: bool,
    pub extra_marketplaces_set: bool,
    pub approved_orgs_set: bool,
    pub otel_set: bool,
    pub telemetry_level_set: bool,
    pub enable_feedback_set: bool,
    pub default_model_forced_to_auto: bool,
    pub tools_auto_approve_forced_off: bool,
    pub otel_forced_enabled: bool,
    pub telemetry_level: Option<String>,
    /// As propriedades `*Source`, na ordem de [`TRACKED_SOURCES`].
    pub sources: Vec<(&'static str, PolicySource)>,
}

impl PolicyAppliedEvent {
    /// O objeto que o `publicLog2` recebe.
    pub fn to_dynamic(&self) -> Dynamic {
        let count = |value: u32| Dynamic::Number(f64::from(value));
        let mut entries: Vec<(&str, Dynamic)> = vec![
            ("devicePolicyCount", count(self.device_policy_count)),
            ("nativeMdmPolicyCount", count(self.native_mdm_policy_count)),
            ("serverManagedSettingsPolicyCount", count(self.server_managed_settings_policy_count)),
            ("fileManagedSettingsPolicyCount", count(self.file_managed_settings_policy_count)),
            ("mixedManagedSettingsPolicyCount", count(self.mixed_managed_settings_policy_count)),
            ("accountPolicyCount", count(self.account_policy_count)),
            ("accountGatePolicyCount", count(self.account_gate_policy_count)),
            ("defaultModelSet", Dynamic::Bool(self.default_model_set)),
            ("toolsAutoApproveSet", Dynamic::Bool(self.tools_auto_approve_set)),
            ("enabledPluginsSet", Dynamic::Bool(self.enabled_plugins_set)),
            ("extraMarketplacesSet", Dynamic::Bool(self.extra_marketplaces_set)),
            ("approvedOrgsSet", Dynamic::Bool(self.approved_orgs_set)),
            ("otelSet", Dynamic::Bool(self.otel_set)),
            ("telemetryLevelSet", Dynamic::Bool(self.telemetry_level_set)),
            ("enableFeedbackSet", Dynamic::Bool(self.enable_feedback_set)),
            ("defaultModelForcedToAuto", Dynamic::Bool(self.default_model_forced_to_auto)),
            ("toolsAutoApproveForcedOff", Dynamic::Bool(self.tools_auto_approve_forced_off)),
            ("otelForcedEnabled", Dynamic::Bool(self.otel_forced_enabled)),
            ("telemetryLevel", self.telemetry_level.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
        ];
        entries.extend(self.sources.iter().map(|(field, source)| (*field, Dynamic::string(policy_source_name(*source)))));
        Dynamic::object(entries)
    }
}

const KNOWN_TELEMETRY_LEVELS: [&str; 4] = ["off", "crash", "error", "all"];

/// `telemetryLevelBucket(rawValue)`: o nível forçado quando é um dos conhecidos, `unknown` para qualquer
/// outro valor (o valor cru nunca sai).
fn telemetry_level_bucket(raw_value: Option<&PolicyValue>) -> Option<String> {
    let raw_value = raw_value?;
    Some(match raw_value {
        PolicyValue::String(level) if KNOWN_TELEMETRY_LEVELS.contains(&level.as_str()) => level.clone(),
        _ => "unknown".to_owned(),
    })
}

/// `PolicyTelemetryContribution`.
pub struct PolicyTelemetryContribution {
    base: Rc<Disposable>,
}

impl PolicyTelemetryContribution {
    pub const ID: &'static str = "workbench.contrib.policyTelemetry";

    pub fn new(policy_service: Rc<dyn PolicyService>, telemetry_service: Rc<dyn ITelemetryService>) -> Rc<Self> {
        let base = Disposable::new();

        let last_event: Rc<RefCell<Option<PolicyAppliedEvent>>> = Rc::new(RefCell::new(None));
        let scheduler = RunOnceScheduler::new(
            {
                let policy_service = policy_service.clone();
                move || report(policy_service.as_ref(), telemetry_service.as_ref(), &last_event)
            },
            REPORT_DELAY_MS,
        );
        base.register(scheduler.clone());
        scheduler.schedule(None);
        base.register(policy_service.on_did_change().listen(move |_| scheduler.schedule(None)));

        Rc::new(Self { base })
    }
}

impl IDisposable for PolicyTelemetryContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `report()`: só publica quando o evento difere do último publicado.
fn report(policy_service: &dyn PolicyService, telemetry_service: &dyn ITelemetryService, last_event: &RefCell<Option<PolicyAppliedEvent>>) {
    let event = build_event(policy_service);
    if last_event.borrow().as_ref() == Some(&event) {
        return;
    }
    telemetry_service.public_log2(POLICY_APPLIED_EVENT_NAME, Some(event.to_dynamic()));
    *last_event.borrow_mut() = Some(event);
}

/// `buildEvent()`.
fn build_event(policy_service: &dyn PolicyService) -> PolicyAppliedEvent {
    let value = |name: &str| policy_service.get_policy_value(name);
    let is_set = |name: &str| value(name).is_some();
    let source = |name: &str| -> PolicySource {
        value(name).map(|_| policy_service.get_policy_value_source(name).unwrap_or(PolicyValueSource::Device))
    };

    let mut device_policy_count = 0;
    let mut native_mdm_policy_count = 0;
    let mut server_managed_settings_policy_count = 0;
    let mut file_managed_settings_policy_count = 0;
    let mut mixed_managed_settings_policy_count = 0;
    let mut account_policy_count = 0;
    let mut account_gate_policy_count = 0;
    for (name, _) in policy_service.policy_definitions() {
        if let Some(source) = source(&name) {
            match source {
                PolicyValueSource::Device => device_policy_count += 1,
                PolicyValueSource::NativeMdm => native_mdm_policy_count += 1,
                PolicyValueSource::ServerManagedSettings => server_managed_settings_policy_count += 1,
                PolicyValueSource::FileManagedSettings => file_managed_settings_policy_count += 1,
                PolicyValueSource::MixedManagedSettings => mixed_managed_settings_policy_count += 1,
                PolicyValueSource::Account => account_policy_count += 1,
                PolicyValueSource::AccountGate => account_gate_policy_count += 1,
            }
        }
    }

    let default_model = value(names::DEFAULT_MODEL);
    let tools_auto_approve = value(names::TOOLS_AUTO_APPROVE);
    let otel = value(names::OTEL_ENABLED);
    let telemetry_level = value(names::TELEMETRY_LEVEL);

    PolicyAppliedEvent {
        device_policy_count,
        native_mdm_policy_count,
        server_managed_settings_policy_count,
        file_managed_settings_policy_count,
        mixed_managed_settings_policy_count,
        account_policy_count,
        account_gate_policy_count,
        default_model_set: default_model.is_some(),
        tools_auto_approve_set: tools_auto_approve.is_some(),
        enabled_plugins_set: is_set(names::ENABLED_PLUGINS),
        extra_marketplaces_set: is_set(names::EXTRA_MARKETPLACES),
        approved_orgs_set: is_set(names::APPROVED_ORGS),
        otel_set: otel.is_some(),
        telemetry_level_set: telemetry_level.is_some(),
        enable_feedback_set: is_set(names::ENABLE_FEEDBACK),
        default_model_forced_to_auto: default_model == Some(PolicyValue::String("auto".to_owned())),
        tools_auto_approve_forced_off: tools_auto_approve == Some(PolicyValue::Boolean(false)),
        otel_forced_enabled: otel == Some(PolicyValue::Boolean(true)),
        telemetry_level: telemetry_level_bucket(telemetry_level.as_ref()),
        sources: TRACKED_SOURCES.iter().map(|(field, name)| (*field, source(name))).collect(),
    }
}
