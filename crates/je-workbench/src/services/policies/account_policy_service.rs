//! Porte de `src/vs/workbench/services/policies/common/accountPolicyService.ts`: o serviço de políticas que resolve
//! o valor de cada política a partir da conta padrão, das configurações gerenciadas (servidor, MDM nativo e
//! arquivo) e do portão "Require Approved Account".
//!
//! Decisões de tradução:
//! - **`extends AbstractPolicyService`** é uma [`AbstractPolicyService`] guardada por composição, como o
//!   `MultiplexPolicyService` de `je_platform` faz. O método abstrato `_updatePolicyDefinitions` é o
//!   [`PolicyUpdater`] da base, que chama [`Inner::update_policy_definitions`] por um `Weak` (a base não segura o
//!   serviço vivo).
//! - **`implements IPolicyService, IAccountPolicyGateService, IManagedSettingsService`** são três `impl` de trait
//!   sobre [`AccountPolicyService`]; o `IPolicyService` delega ao estado da base.
//! - **`this.policyDefinitions`**, que o original passa por referência e que continua sendo mutado depois do
//!   primeiro `await`, é lido de novo da base depois dele ([`PolicyService::policy_definitions`]).
//! - **`policy.value(policyData)`** passa pela função dinâmica da [`PolicyDefinition`]
//!   ([`call_policy_value`](super::policy_value_callback::call_policy_value)); `policy.managedSettings` é o objeto
//!   dinâmico cujas chaves próprias são as chaves declaradas.
//! - **`ChatAccountPolicyGateActiveContext`** é [`chat_account_policy_gate_active_context`], uma função (o
//!   `RawContextKey` não é um valor constante no porte).
//! - **`Event.toPromise(onDidChangeGateInfo)`** do `whenAccountPolicySettled` é `to_promise(None)`.
//! - **`JSON.parse` do valor da política de organizações aprovadas**: um texto que não é JSON fica como texto
//!   e, não sendo lista, vira a lista vazia (o `try`/`catch` do original).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::default_account::IPolicyData;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::json;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::policy::{ManagedSettingValue, ManagedSettingsData};
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};
use je_platform::js::{from_json, trim};
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::policy::copilot_managed_settings::{
    IFileManagedSettingsService, IManagedSettingResolution, IManagedSettingsService, INativeManagedSettingsService, MANAGED_SETTINGS_CHANNELS, ManagedSettingsChannel,
    collect_managed_settings_definitions, has_managed_settings_definitions, has_raw_managed_settings, is_managed_setting_governance_active, pick_managed_settings, project_managed_settings,
};
use je_platform::policy::managed_settings_freshness::{IManagedSettingsFreshness, ManagedSettingsFreshnessState, is_managed_settings_freshness_blocking};
use je_platform::policy::{
    AbstractPolicyService, PolicyDefinition, PolicyDefinitions, PolicyService, PolicyUpdater, PolicyValue, PolicyValueSource, PolicyValues, SerializedPolicy, get_restricted_policy_value,
};

use super::policy_value_callback::{call_policy_value, empty_policy_data};
use crate::services::accounts::default_account::IDefaultAccountService;

/// Policy name (declared by `chat.approvedAccountOrganizations`) holding the list of
/// GitHub organization logins that satisfy the gate. The token `*` is a wildcard.
pub const APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME: &str = "ChatApprovedAccountOrganizations";

/// `AccountPolicyGateState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountPolicyGateState {
    Inactive,
    Satisfied,
    /// Gate active and NOT satisfied: restricted values are applied to all gated policies.
    Restricted,
}

impl AccountPolicyGateState {
    /// O texto do original (`'inactive' | 'satisfied' | 'restricted'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Satisfied => "satisfied",
            Self::Restricted => "restricted",
        }
    }
}

/// `AccountPolicyGateUnsatisfiedReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountPolicyGateUnsatisfiedReason {
    NoAccount,
    WrongProvider,
    OrgNotApproved,
    PolicyNotResolved,
    ManagedSettingsRefresh,
}

impl AccountPolicyGateUnsatisfiedReason {
    /// O texto do original (`'noAccount' | 'wrongProvider' | ...`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoAccount => "noAccount",
            Self::WrongProvider => "wrongProvider",
            Self::OrgNotApproved => "orgNotApproved",
            Self::PolicyNotResolved => "policyNotResolved",
            Self::ManagedSettingsRefresh => "managedSettingsRefresh",
        }
    }
}

/// `IAccountPolicyGateInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct IAccountPolicyGateInfo {
    pub state: AccountPolicyGateState,
    pub reason: Option<AccountPolicyGateUnsatisfiedReason>,
    pub approved_organizations: Option<Vec<String>>,
    pub managed_settings_freshness: Option<IManagedSettingsFreshness>,
}

impl IAccountPolicyGateInfo {
    fn inactive() -> Self {
        Self { state: AccountPolicyGateState::Inactive, reason: None, approved_organizations: None, managed_settings_freshness: None }
    }

    fn restricted(reason: AccountPolicyGateUnsatisfiedReason, approved_organizations: Option<Vec<String>>) -> Self {
        Self { state: AccountPolicyGateState::Restricted, reason: Some(reason), approved_organizations, managed_settings_freshness: None }
    }
}

/// `ChatAccountPolicyGateActiveContext`.
pub fn chat_account_policy_gate_active_context() -> RawContextKey {
    RawContextKey::new(
        "chatAccountPolicyGateActive",
        false,
        ContextKeyMeta::Info {
            ty: "boolean".to_owned(),
            description: localize("chatAccountPolicyGateActive", "True when account policy or managed-settings enforcement prevents this client from using AI features.", &[]),
        },
    )
}

/// Read-only accessor for the Account Policy gate state. Backed by the same
/// `AccountPolicyService` instance that drives policy enforcement, so UX consumers
/// (notifications, context keys, telemetry) cannot drift from the authoritative
/// gate decision.
pub trait IAccountPolicyGateService {
    fn gate_info(&self) -> IAccountPolicyGateInfo;
    fn on_did_change_gate_info(&self) -> Event<IAccountPolicyGateInfo>;
    /// Completes after the gate and policy values incorporate the initialized account.
    fn when_initialized(&self) -> Promise<()>;
}

service_id!(pub I_ACCOUNT_POLICY_GATE_SERVICE: dyn IAccountPolicyGateService = "accountPolicyGateService");

/// Waits for authoritative policy, including settled fail-closed restrictions.
pub fn when_account_policy_settled(gate_service: Rc<dyn IAccountPolicyGateService>) -> Promise<()> {
    async_fn(async move {
        gate_service.when_initialized().await?;
        loop {
            let info = gate_service.gate_info();
            let pending = info.reason == Some(AccountPolicyGateUnsatisfiedReason::PolicyNotResolved)
                || info.managed_settings_freshness.as_ref().map(IManagedSettingsFreshness::state) == Some(ManagedSettingsFreshnessState::Pending);
            if !pending {
                break;
            }
            gate_service.on_did_change_gate_info().to_promise(None).await?;
        }
        Ok(())
    })
}

/// `IResolvedPolicyData`.
struct IResolvedPolicyData {
    policy_data: IPolicyData,
    managed_setting_resolutions: indexmap::IndexMap<String, IManagedSettingResolution>,
    active_managed_settings_sources: Vec<ManagedSettingsChannel>,
}

struct Inner {
    me: Weak<Inner>,
    service: Rc<AbstractPolicyService>,
    log_service: LogServiceRef,
    default_account_service: Rc<dyn IDefaultAccountService>,
    gate_info: RefCell<IAccountPolicyGateInfo>,
    on_did_change_gate_info: Rc<Emitter<IAccountPolicyGateInfo>>,
    managed_settings: RefCell<ManagedSettingsData>,
    on_did_change_managed_settings: Rc<Emitter<()>>,
    /// Read-only: the MultiplexPolicyService owns calling updatePolicyDefinitions.
    managed_policy_reader: Option<Rc<dyn PolicyService>>,
    native_managed_settings_service: Option<Rc<dyn INativeManagedSettingsService>>,
    file_managed_settings_service: Option<Rc<dyn IFileManagedSettingsService>>,
    initialization: RefCell<Option<Promise<()>>>,
    base: Rc<Disposable>,
}

/// `AccountPolicyService`.
pub struct AccountPolicyService {
    inner: Rc<Inner>,
}

impl AccountPolicyService {
    pub fn new(
        log_service: LogServiceRef,
        default_account_service: Rc<dyn IDefaultAccountService>,
        managed_policy_service: Option<Rc<dyn PolicyService>>,
        native_managed_settings_service: Option<Rc<dyn INativeManagedSettingsService>>,
        file_managed_settings_service: Option<Rc<dyn IFileManagedSettingsService>>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_gate_info = Emitter::new();
        let on_did_change_managed_settings = Emitter::new();
        base.register(on_did_change_gate_info.clone());
        base.register(on_did_change_managed_settings.clone());
        let inner = Rc::new_cyclic(|me: &Weak<Inner>| {
            let weak = me.clone();
            let updater: PolicyUpdater = Rc::new(move |_| match weak.upgrade() {
                Some(inner) => inner.update_policy_definitions(),
                None => Promise::resolved(()),
            });
            Inner {
                me: me.clone(),
                service: AbstractPolicyService::new(updater),
                log_service,
                default_account_service,
                gate_info: RefCell::new(IAccountPolicyGateInfo::inactive()),
                on_did_change_gate_info,
                managed_settings: RefCell::new(ManagedSettingsData::new()),
                on_did_change_managed_settings,
                managed_policy_reader: managed_policy_service,
                native_managed_settings_service,
                file_managed_settings_service,
                initialization: RefCell::new(None),
                base,
            }
        });

        let _ = inner.update_policy_definitions();
        inner.register_update_listener(inner.default_account_service.on_did_change_policy_data().map(|_| (), None));
        inner.register_update_listener(inner.default_account_service.on_did_change_default_account().map(|_| (), None));
        inner.register_update_listener(inner.default_account_service.on_did_change_managed_settings_freshness().map(|_| (), None));
        if let Some(reader) = &inner.managed_policy_reader {
            let weak = inner.me.clone();
            inner.base.register(reader.on_did_change().listen(move |names| {
                if names.iter().any(|name| name == APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME)
                    && let Some(inner) = weak.upgrade()
                {
                    let _ = inner.update_policy_definitions();
                }
            }));
        }
        if let Some(native) = &inner.native_managed_settings_service {
            inner.register_update_listener(native.on_did_change_managed_settings().map(|_| (), None));
        }
        if let Some(file) = &inner.file_managed_settings_service {
            inner.register_update_listener(file.on_did_change_raw_managed_settings().map(|_| (), None));
        }

        // The initial account load sets `currentDefaultAccount` but does NOT fire
        // `onDidChangeDefaultAccount`. Re-evaluate once the account has resolved
        // so the gate doesn't stay stuck on `noAccount`.
        let initialization = inner.initialize();
        *inner.initialization.borrow_mut() = Some(initialization.clone());
        let log_service = inner.log_service.clone();
        let _ = initialization.catch(move |error| {
            log_service.error_args(&LogMessage::Text("AccountPolicyService: Failed to initialize account policy".to_owned()), &[LogArg::Error(error)]);
        });
        Rc::new(Self { inner })
    }
}

impl Inner {
    /// Um ouvinte que reavalia as definições a cada disparo do evento.
    fn register_update_listener(&self, event: Event<()>) {
        let weak = self.me.clone();
        self.base.register(event.listen(move |_| {
            if let Some(inner) = weak.upgrade() {
                let _ = inner.update_policy_definitions();
            }
        }));
    }

    /// `initialize()`.
    fn initialize(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.default_account_service.get_default_account().await?;
            this.update_policy_definitions().await?;
            Ok(())
        })
    }

    /// `_updatePolicyDefinitions(this.policyDefinitions)`.
    fn update_policy_definitions(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let definitions = PolicyService::policy_definitions(&this.service);
            this.log_service.trace(&format!("AccountPolicyService#_updatePolicyDefinitions: Got {} policy definitions", definitions.len()));
            let managed_settings = this.update_copilot_managed_setting_definitions(definitions).await?;

            let mut updated: Vec<String> = Vec::new();
            let resolved_policy_data = this.get_policy_data(managed_settings);

            let previous_info = this.gate_info.borrow().clone();
            let gate_info = this.compute_gate_info();
            *this.gate_info.borrow_mut() = gate_info.clone();
            let gate_info_changed = previous_info != gate_info;

            // `policyNotResolved` is a transient state where the user IS in an approved
            // org but account-side policy data hasn't loaded yet. We don't force restricted
            // values here: `policy.value(policyData)` naturally returns undefined when
            // `policyData` is null, so no account overrides slip through. Forcing
            // `restrictedValue` would transiently flip `chat.disableAIFeatures = true`,
            // surfacing confusing "Unable to write" errors and a UI flash.
            let gate_restricted = gate_info.state == AccountPolicyGateState::Restricted && gate_info.reason != Some(AccountPolicyGateUnsatisfiedReason::PolicyNotResolved);

            for (key, definition) in PolicyService::policy_definitions(&this.service) {
                let resolved_policy = this.resolve_policy_value(&definition, resolved_policy_data.as_ref(), gate_restricted);
                let changed = match resolved_policy {
                    Some((value, source)) => this.service.update_policy_value(&key, Some(value), source),
                    None => this.service.update_policy_value(&key, None, PolicyValueSource::Device),
                };
                if changed {
                    updated.push(key);
                }
            }

            if !updated.is_empty() {
                this.service.fire_did_change(updated);
            }
            if gate_info_changed {
                this.on_did_change_gate_info.fire(gate_info);
            }
            Ok(())
        })
    }

    /// `resolvePolicyValue(policy, resolvedPolicyData, gateRestricted)`.
    fn resolve_policy_value(&self, policy: &PolicyDefinition, resolved_policy_data: Option<&IResolvedPolicyData>, gate_restricted: bool) -> Option<(PolicyValue, PolicyValueSource)> {
        if gate_restricted && (policy.value.is_some() || policy.restricted_value.is_some()) {
            return Some((get_restricted_policy_value(policy), PolicyValueSource::AccountGate));
        }

        let value_provider = &policy.value;
        let resolved_policy_data = resolved_policy_data?;
        value_provider.as_ref()?;

        let IResolvedPolicyData { policy_data, managed_setting_resolutions, active_managed_settings_sources } = resolved_policy_data;
        let value = call_policy_value(value_provider, policy_data)?;

        let mut source = PolicyValueSource::Account;
        if let Some(declared) = &policy.managed_settings {
            let managed_settings = policy_data.managed_settings.clone().unwrap_or_default();
            let applied_keys: Vec<String> = declared.own_keys().into_iter().map(|key| key.to_string_lossy()).filter(|key| managed_settings.contains_key(key)).collect();
            if !applied_keys.is_empty() {
                let without_managed_setting_keys = |keys: &HashSet<String>| -> IPolicyData {
                    let mut data = policy_data.clone();
                    data.managed_settings = Some(managed_settings.iter().filter(|(key, _)| !keys.contains(*key)).map(|(key, entry)| (key.clone(), entry.clone())).collect());
                    data
                };
                let all_applied_keys: HashSet<String> = applied_keys.iter().cloned().collect();
                if call_policy_value(value_provider, &without_managed_setting_keys(&all_applied_keys)).as_ref() != Some(&value) {
                    let mut contributing_channels: Vec<ManagedSettingsChannel> = Vec::new();
                    for key in &applied_keys {
                        if let Some(channel) = managed_setting_resolutions_source(managed_setting_resolutions, key)
                            && !contributing_channels.contains(&channel)
                        {
                            contributing_channels.push(channel);
                        }
                    }

                    let mut causal_channels: Vec<ManagedSettingsChannel> = Vec::new();
                    for channel in &contributing_channels {
                        let channel_keys: HashSet<String> = applied_keys.iter().filter(|key| managed_setting_resolutions_source(managed_setting_resolutions, key) == Some(*channel)).cloned().collect();
                        if call_policy_value(value_provider, &without_managed_setting_keys(&channel_keys)).as_ref() != Some(&value) {
                            causal_channels.push(*channel);
                        }
                    }

                    let channels = if causal_channels.is_empty() { &contributing_channels } else { &causal_channels };
                    source = if channels.len() == 1 { policy_value_source_for_managed_settings_channel(channels[0]) } else { PolicyValueSource::MixedManagedSettings };
                }
            }
        }

        // A policy can also react to the mere *presence* of managed settings rather than to a
        // declared key, so probe for that too and attribute it to the governing channels.
        if source == PolicyValueSource::Account && policy_data.managed_settings_active == Some(true) {
            let mut probe = policy_data.clone();
            probe.managed_settings = None;
            probe.managed_settings_active = Some(false);
            if call_policy_value(value_provider, &probe).as_ref() != Some(&value) && !active_managed_settings_sources.is_empty() {
                source = if active_managed_settings_sources.len() == 1 {
                    policy_value_source_for_managed_settings_channel(active_managed_settings_sources[0])
                } else {
                    PolicyValueSource::MixedManagedSettings
                };
            }
        }

        Some((value, source))
    }

    /// `updateCopilotManagedSettingDefinitions(policyDefinitions)`.
    fn update_copilot_managed_setting_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<Option<ManagedSettingsData>> {
        let native = self.native_managed_settings_service.clone();
        async_fn(async move {
            let Some(native) = native else { return Ok(None) };
            if !has_managed_settings_definitions(&policy_definitions) {
                return Ok(Some(native.managed_settings()));
            }

            Ok(Some(native.update_policy_definitions(policy_definitions).await?))
        })
    }

    /// `getPolicyData(mdmManagedSettings)`.
    fn get_policy_data(&self, mdm_managed_settings: Option<ManagedSettingsData>) -> Option<IResolvedPolicyData> {
        let account_policy_data = self.default_account_service.policy_data();
        let native_managed_settings = mdm_managed_settings.or_else(|| self.native_managed_settings_service.as_ref().map(|native| native.managed_settings()));
        let file_managed_settings = self.file_managed_settings_service.as_ref().map(|file| file.managed_settings());

        // Share channel resolution, including the force-on sandbox floor, with Policy Diagnostics.
        let account_managed_settings = account_policy_data.as_ref().and_then(|data| data.managed_settings.as_ref());
        let pick = pick_managed_settings(native_managed_settings.as_ref(), account_managed_settings, file_managed_settings.as_ref());
        let mut active_sources: HashSet<ManagedSettingsChannel> =
            pick.resolutions.iter().filter(|(key, _)| is_managed_setting_governance_active(key)).map(|(_, resolution)| resolution.source).collect();
        if account_policy_data.as_ref().and_then(|data| data.managed_settings_active) == Some(true) {
            active_sources.insert(ManagedSettingsChannel::Server);
        }
        let raw_file_managed_settings = self.file_managed_settings_service.as_ref().map(|file| file.raw_managed_settings());
        if has_raw_managed_settings(raw_file_managed_settings.as_ref()) {
            active_sources.insert(ManagedSettingsChannel::File);
        }
        let active_managed_settings_sources: Vec<ManagedSettingsChannel> = MANAGED_SETTINGS_CHANNELS.into_iter().filter(|source| active_sources.contains(source)).collect();
        if *self.managed_settings.borrow() != pick.values {
            *self.managed_settings.borrow_mut() = pick.values.clone();
            self.on_did_change_managed_settings.fire(());
        }
        let declared_managed_settings = collect_managed_settings_definitions(&PolicyService::policy_definitions(&self.service));
        let log_service = self.log_service.clone();
        let managed_settings_data = project_managed_settings(&pick.values, &declared_managed_settings, Some(&|message: &str| log_service.warn(&format!("[AccountPolicy] {message}"))));
        if account_policy_data.is_none() && active_managed_settings_sources.is_empty() && managed_settings_data.is_empty() {
            return None;
        }

        let mut policy_data = account_policy_data.unwrap_or_else(empty_policy_data);
        policy_data.managed_settings = Some(managed_settings_data);
        policy_data.managed_settings_active = Some(!active_managed_settings_sources.is_empty());
        Some(IResolvedPolicyData { policy_data, managed_setting_resolutions: pick.resolutions, active_managed_settings_sources })
    }

    /// `computeGateInfo()`.
    fn compute_gate_info(&self) -> IAccountPolicyGateInfo {
        let freshness = self.default_account_service.managed_settings_freshness();
        if is_managed_settings_freshness_blocking(&freshness) {
            return IAccountPolicyGateInfo {
                state: AccountPolicyGateState::Restricted,
                reason: Some(AccountPolicyGateUnsatisfiedReason::ManagedSettingsRefresh),
                approved_organizations: None,
                managed_settings_freshness: Some(freshness),
            };
        }

        let Some(managed_policy_reader) = &self.managed_policy_reader else {
            return IAccountPolicyGateInfo::inactive();
        };

        let approved_raw = managed_policy_reader.get_policy_value(APPROVED_ACCOUNT_ORGANIZATIONS_POLICY_NAME);
        let approved_orgs = parse_approved_organizations(approved_raw.as_ref());
        if approved_orgs.is_empty() {
            return IAccountPolicyGateInfo::inactive();
        }

        let Some(account) = self.default_account_service.current_default_account() else {
            return IAccountPolicyGateInfo::restricted(AccountPolicyGateUnsatisfiedReason::NoAccount, Some(approved_orgs));
        };

        let configured_provider = self.default_account_service.get_default_account_authentication_provider();
        if account.authentication_provider.id != configured_provider.id {
            return IAccountPolicyGateInfo::restricted(AccountPolicyGateUnsatisfiedReason::WrongProvider, Some(approved_orgs));
        }

        // Org membership is checked BEFORE policy-data resolution so users definitively
        // NOT in an approved org are restricted immediately, even while policy data is
        // still loading. `policyNotResolved` is reserved for users who ARE in an approved
        // org: a transient state that resolves on its own.
        if !approved_orgs.iter().any(|org| org == "*") {
            let account_orgs: Vec<String> =
                account.entitlements_data.as_ref().map(|entitlements| entitlements.organization_login_list.iter().map(|org| org.to_lowercase()).collect()).unwrap_or_default();
            let intersects = account_orgs.iter().any(|org| approved_orgs.contains(org));
            if !intersects {
                return IAccountPolicyGateInfo::restricted(AccountPolicyGateUnsatisfiedReason::OrgNotApproved, Some(approved_orgs));
            }
        }

        if self.default_account_service.policy_data().is_none() {
            return IAccountPolicyGateInfo::restricted(AccountPolicyGateUnsatisfiedReason::PolicyNotResolved, Some(approved_orgs));
        }

        IAccountPolicyGateInfo { state: AccountPolicyGateState::Satisfied, reason: None, approved_organizations: Some(approved_orgs), managed_settings_freshness: None }
    }
}

/// `managedSettingResolutions.get(key)?.source`.
fn managed_setting_resolutions_source(resolutions: &indexmap::IndexMap<String, IManagedSettingResolution>, key: &str) -> Option<ManagedSettingsChannel> {
    resolutions.get(key).map(|resolution| resolution.source)
}

/// `policyValueSourceForManagedSettingsChannel(channel)`.
fn policy_value_source_for_managed_settings_channel(channel: ManagedSettingsChannel) -> PolicyValueSource {
    match channel {
        ManagedSettingsChannel::NativeMdm => PolicyValueSource::NativeMdm,
        ManagedSettingsChannel::Server => PolicyValueSource::ServerManagedSettings,
        ManagedSettingsChannel::File => PolicyValueSource::FileManagedSettings,
    }
}

/// `parseApprovedOrganizations(raw)`.
fn parse_approved_organizations(raw: Option<&PolicyValue>) -> Vec<String> {
    // Array-typed policies are delivered as JSON-stringified arrays: see
    // `PolicyConfiguration.parse` for the same normalisation.
    let value = match raw {
        Some(PolicyValue::String(text)) => {
            let mut errors = Vec::new();
            match json::parse_str(text, &mut errors) {
                Some(parsed) if errors.is_empty() => from_json(&parsed),
                _ => Dynamic::string(text),
            }
        }
        Some(other) => other.to_dynamic(),
        None => Dynamic::Undefined,
    };
    let Dynamic::Array(items) = value else { return Vec::new() };
    items
        .to_vec()
        .into_iter()
        .filter_map(|item| match item {
            Dynamic::String(text) => Some(trim(&text).to_string_lossy().to_lowercase()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect()
}

impl PolicyService for AccountPolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        PolicyService::on_did_change(&self.inner.service)
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        PolicyService::update_policy_definitions(&self.inner.service, policy_definitions)
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        PolicyService::get_policy_value(&self.inner.service, name)
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        PolicyService::get_policy_value_source(&self.inner.service, name)
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        PolicyService::serialize(&self.inner.service)
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        PolicyService::policy_definitions(&self.inner.service)
    }
}

impl IAccountPolicyGateService for AccountPolicyService {
    fn gate_info(&self) -> IAccountPolicyGateInfo {
        self.inner.gate_info.borrow().clone()
    }

    fn on_did_change_gate_info(&self) -> Event<IAccountPolicyGateInfo> {
        self.inner.on_did_change_gate_info.event()
    }

    fn when_initialized(&self) -> Promise<()> {
        self.inner.initialization.borrow().clone().expect("a inicialização começa no construtor")
    }
}

impl IManagedSettingsService for AccountPolicyService {
    fn on_did_change_managed_settings(&self) -> Event<()> {
        self.inner.on_did_change_managed_settings.event()
    }

    fn get_managed_settings(&self) -> ManagedSettingsData {
        self.inner.managed_settings.borrow().clone()
    }

    fn get_managed_setting_value(&self, key: &str) -> Option<ManagedSettingValue> {
        self.inner.managed_settings.borrow().get(key).cloned()
    }
}

impl IDisposable for AccountPolicyService {
    fn dispose(&self) {
        self.inner.base.dispose();
        self.inner.service.dispose();
    }
}
