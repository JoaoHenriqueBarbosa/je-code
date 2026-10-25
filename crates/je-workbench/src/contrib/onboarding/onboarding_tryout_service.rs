//! Porte de `src/vs/workbench/contrib/onboarding/browser/onboardingTryoutService.ts`: o serviço que decide a
//! disponibilidade de um "experimente o recurso", prepara e executa o tryout, junta chamadas concorrentes e emite a
//! telemetria de início e de desfecho.
//!
//! Divergências:
//! - **O que o serviço precisa e o porte ainda não tinha.** O contrato `IOnboardingTryoutService`, os registros
//!   `registerOnboardingTryout` e `registerOnboardingTryoutPresentation`, `parseOnboardingTryoutArguments`,
//!   `createOnboardingTryoutUri` e `parseOnboardingTryoutUri` (de `common/onboardingTryout.ts`) e as opções de
//!   execução `IOnboardingTryoutRunOptions` (de `platform/onboarding/common/onboardingTryoutHandoff.ts`) entram aqui,
//!   porque o serviço e o teste dele são os únicos consumidores. `parseExternalOnboardingTryoutUri` fica de fora (nada
//!   neste arquivo o chama). `OnboardingTryoutSource` é um enum, então o `isOnboardingTryoutSource` do original (que
//!   valida um `unknown`) não tem o que validar.
//! - **Chat fora do escopo do porte.** O `IChatEntitlementService`, o `IChatSentiment`, o `ChatEntitlement` e o
//!   `chatRequiresSetup` são o recorte que o serviço lê (sentimento, direito de uso, acesso anônimo, modelos BYOK e os
//!   três eventos de mudança), com a regra de `chatRequiresSetup` idêntica à do original.
//! - `IOnboardingTryoutRunContext.options` não existe no [`OnboardingTryoutRunContext`] do porte, então o contexto que
//!   o serviço entrega às apresentações não leva as opções (nenhuma apresentação portada as lê).
//! - `IOnboardingTryoutScenario` (`IOnboardingScenario & { tryout }`) é o `Rc<OnboardingScenario>` do registro: o filtro
//!   de `isTryout` garante o `tryout` presente.
//! - `run` lança de forma síncrona ([`errors::throw`]) para argumentos e opções inválidos, como o original, e o resto
//!   vira rejeição da promessa. O `try/catch/finally` do corpo é a sequência explícita de [`OnboardingTryoutService::run`]:
//!   o `finally` (limpeza e telemetria de desfecho) roda também quando o `onDidLaunch` lança, e uma exceção dele
//!   substitui o resultado, como no original.
//! - `doRun` roda dentro de [`async_fn_maybe`] para que um `throw` síncrono (por exemplo do `getAvailability` de uma
//!   apresentação) vire rejeição, e devolve a promessa de `run()` pela adoção do `return` de uma função `async`.
//! - A classificação GDPR dos eventos (`TryoutClassification`) é só tipo do TypeScript e não tem equivalente.
//! - `extends Disposable` é o [`DisposableStore`] guardado em `store`, que também responde por `this._store.isDisposed`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn_maybe, race_cancellation_error};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::html_content::create_command_uri;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::{URI, decode_uri_component};
use je_base::uuid::{generate_uuid, is_uuid};
use je_platform::contextkey::contextkey::{ContextKeyExpression, IContextKeyService};
use je_platform::telemetry::ITelemetryService;

use super::onboarding_registry::onboarding_scenario_registry;
use super::onboarding_scenario::{OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario, OnboardingTrigger};
use super::onboarding_tryout::{
    AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND, IOnboardingTryoutPresentation, OnboardingTryoutAvailability,
    OnboardingTryoutLaunchKind, OnboardingTryoutMetadata, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutTargetWindow, OnboardingTryoutUnavailable,
    RUN_ONBOARDING_TRYOUT_COMMAND_ID, is_onboarding_tryout_id, onboarding_tryout_presentation_registry,
};
use super::onboarding_tryout::OnboardingTryoutSetupAction;
use super::onboarding_tryout::OnboardingTryoutCommand;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// `ChatEntitlement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatEntitlement {
    /// Signed out.
    Unknown,
    /// Signed in but not yet resolved.
    Unresolved,
    /// Signed in and entitled to Free.
    Available,
    /// Signed in but not entitled to Free.
    Unavailable,
    /// Signed-up to Free.
    Free,
    /// Signed-up to EDU.
    Edu,
    /// Signed-up to Pro.
    Pro,
    /// Signed-up to Pro Plus.
    ProPlus,
    /// Signed-up to Business.
    Business,
    /// Signed-up to Enterprise.
    Enterprise,
    /// Signed-up to Max.
    Max,
}

/// `IChatSentiment`: todos os campos são opcionais no original.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChatSentiment {
    /// Whether the user has completed the setup flow or not, regardless of the outcome.
    pub completed: Option<bool>,
    /// User has Chat installed.
    pub installed: Option<bool>,
    /// User signals no intent in using Chat (esconde toda a UI).
    pub hidden: Option<bool>,
    /// User signals intent to disable Chat (desativa a funcionalidade, sem esconder a UI).
    pub disabled: Option<bool>,
    /// Chat is disabled at the workspace level.
    pub disabled_in_workspace: Option<bool>,
    /// Chat is disabled due to missing workspace trust.
    pub untrusted: Option<bool>,
    /// User signals intent to use Chat later.
    pub later: Option<bool>,
    /// User has registered as Free or Pro user.
    pub registered: Option<bool>,
}

/// `IChatSetupRequirement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatSetupRequirement {
    /// Whether the setup flow has been completed (any outcome).
    pub completed: bool,
    /// Whether the chat extension is disabled for a reason other than trust.
    pub disabled: bool,
    /// Whether the chat extension is disabled because the workspace is untrusted.
    pub untrusted: bool,
    /// The user's last known or resolved entitlement.
    pub entitlement: ChatEntitlement,
    /// Whether anonymous (signed-out) Chat access is enabled.
    pub anonymous: bool,
    /// Whether BYOK models are available.
    pub has_byok_models: bool,
}

/// `chatRequiresSetup`: whether Chat requires setup before it can service a request.
pub fn chat_requires_setup(context: &ChatSetupRequirement) -> bool {
    (!context.completed && !context.has_byok_models) // Setup not completed (unless BYOK models are available)
        || context.disabled // Extension disabled: run setup to enable
        || context.untrusted // Workspace untrusted: run setup to ask for trust
        || context.entitlement == ChatEntitlement::Available // Entitlement available: run setup to sign up
        || (context.entitlement == ChatEntitlement::Unknown // Entitlement unknown: run setup to sign in / sign up
            && !context.anonymous // unless anonymous access is enabled
            && !context.has_byok_models) // unless BYOK models are available
}

/// `IChatEntitlementService`, reduzido ao que o serviço de tryout lê.
pub trait IChatEntitlementService {
    fn sentiment(&self) -> ChatSentiment;
    fn entitlement(&self) -> ChatEntitlement;
    fn anonymous(&self) -> bool;
    fn has_byok_models(&self) -> bool;
    fn on_did_change_sentiment(&self) -> Event<()>;
    fn on_did_change_entitlement(&self) -> Event<()>;
    fn on_did_change_anonymous(&self) -> Event<()>;
}

/// `OnboardingTryoutSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingTryoutSource {
    ReleaseNotes,
    ExternalLink,
    Direct,
}

impl OnboardingTryoutSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReleaseNotes => "releaseNotes",
            Self::ExternalLink => "externalLink",
            Self::Direct => "direct",
        }
    }
}

/// `IOnboardingTryoutRunOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingTryoutRunOptions {
    pub source: OnboardingTryoutSource,
    pub run_id: Option<String>,
}

/// `IOnboardingTryoutService`.
pub trait IOnboardingTryoutService {
    fn on_did_change(&self) -> Event<()>;
    fn get_tryout(&self, id: &str) -> Option<Rc<OnboardingScenario>>;
    fn get_tryouts(&self) -> Vec<Rc<OnboardingScenario>>;
    fn get_availability(&self, id: &str) -> OnboardingTryoutAvailability;
    /// Lança de forma síncrona se `id` ou `options` forem inválidos.
    fn run(&self, id: &str, token: CancellationToken, options: Option<OnboardingTryoutRunOptions>) -> Promise<OnboardingTryoutResult>;
}

fn invalid_arguments_error() -> Error {
    Error::new(localize("onboarding.tryout.invalidArguments", "Expected exactly one valid feature example identifier.", &[]))
}

/// `parseOnboardingTryoutArguments`.
pub fn parse_onboarding_tryout_arguments(args: &[Dynamic]) -> Result<String, Error> {
    match args {
        [Dynamic::String(id)] if is_onboarding_tryout_id(&id.to_string_lossy()) => Ok(id.to_string_lossy()),
        _ => Err(invalid_arguments_error()),
    }
}

/// `createOnboardingTryoutUri`.
pub fn create_onboarding_tryout_uri(id: &str) -> Result<URI, Error> {
    let id = parse_onboarding_tryout_arguments(&[Dynamic::string(id)])?;
    Ok(create_command_uri(&RUN_ONBOARDING_TRYOUT_COMMAND_ID.encode_utf16().collect::<Vec<_>>(), &[Dynamic::string(&id)]))
}

/// `parseOnboardingTryoutUri`: `None` para o que não é o link de comando do tryout; erro para argumentos inválidos.
pub fn parse_onboarding_tryout_uri(uri: &URI) -> Result<Option<String>, Error> {
    if uri.scheme().to_string_lossy() != schemas::COMMAND || uri.path().to_string_lossy() != RUN_ONBOARDING_TRYOUT_COMMAND_ID {
        return Ok(None);
    }
    let decoded = decode_uri_component(uri.query().units())?;
    let parsed = json_parse(&decoded).map_err(|error| Error::new(error.to_string()))?;
    match Dynamic::from_json(&parsed) {
        Dynamic::Array(args) => parse_onboarding_tryout_arguments(&args.to_vec()).map(Some),
        _ => Err(invalid_arguments_error()),
    }
}

/// `IOnboardingTryout<TPayload>`, com os metadados agrupados em `metadata` (no original são os campos soltos).
#[derive(Clone)]
pub struct OnboardingTryout {
    pub id: String,
    pub when: Option<ContextKeyExpression>,
    pub presentation: OnboardingPresentationRef,
    pub metadata: OnboardingTryoutMetadata,
}

/// `registerOnboardingTryout`.
pub fn register_onboarding_tryout(tryout: OnboardingTryout) -> DisposableRef {
    let OnboardingTryout { id, when, presentation, metadata } = tryout;
    if let Err(error) = parse_onboarding_tryout_arguments(&[Dynamic::string(&id)]) {
        errors::throw(error);
    }
    onboarding_scenario_registry().register(OnboardingScenario {
        id,
        seen_key: None,
        when,
        trigger: OnboardingTrigger::Command { command_id: RUN_ONBOARDING_TRYOUT_COMMAND_ID.to_string() },
        priority: None,
        presentation,
        tryout: Some(Rc::new(metadata)),
        experiment: None,
        developer_mode_variations: None,
        repeatable: None,
    })
}

/// `IOnboardingTryoutPresentationDefinition<TPayload>`, com o payload como [`Dynamic`].
#[derive(Clone)]
pub struct OnboardingTryoutPresentationDefinition {
    pub kind: String,
    pub on_did_change_availability: Option<Event<()>>,
    pub is_payload: Rc<dyn Fn(&Dynamic) -> bool>,
    pub get_availability: Rc<dyn Fn(&Dynamic) -> OnboardingTryoutAvailability>,
    pub prepare: Rc<dyn Fn(&Dynamic, &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation>>,
}

struct DefinedTryoutPresentation {
    definition: OnboardingTryoutPresentationDefinition,
}

impl DefinedTryoutPresentation {
    /// `getPayload`.
    fn payload_of(&self, scenario: &OnboardingScenario) -> Dynamic {
        let payload = &scenario.presentation.payload;
        if !(self.definition.is_payload)(payload) {
            errors::throw(Error::new(localize("onboarding.tryout.invalidPayload", "The feature example '{0}' has an invalid presentation.", &[scenario.id.as_str()])));
        }
        payload.clone()
    }
}

impl IOnboardingTryoutPresentation for DefinedTryoutPresentation {
    fn kind(&self) -> &str {
        &self.definition.kind
    }

    fn on_did_change_availability(&self) -> Option<Event<()>> {
        self.definition.on_did_change_availability.clone()
    }

    fn get_availability(&self, scenario: &OnboardingScenario) -> OnboardingTryoutAvailability {
        (self.definition.get_availability)(&self.payload_of(scenario))
    }

    fn prepare(&self, scenario: &OnboardingScenario, context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        (self.definition.prepare)(&self.payload_of(scenario), context)
    }
}

/// `registerOnboardingTryoutPresentation`.
pub fn register_onboarding_tryout_presentation(definition: OnboardingTryoutPresentationDefinition) -> DisposableRef {
    onboarding_tryout_presentation_registry().register(Rc::new(DefinedTryoutPresentation { definition }))
}

/// `TryoutEvent`.
struct TryoutEvent {
    tryout_id: String,
    run_id: String,
    source: OnboardingTryoutSource,
}

impl TryoutEvent {
    /// `{ tryoutId, source, runId }`, na ordem do original (`{ tryoutId: scenario.id, ...runOptions }`).
    fn entries(&self) -> Vec<(&'static str, Dynamic)> {
        vec![
            ("tryoutId", Dynamic::string(&self.tryout_id)),
            ("source", Dynamic::string(self.source.as_str())),
            ("runId", Dynamic::string(&self.run_id)),
        ]
    }
}

/// `launchResult` de `TryoutOutcomeEvent`.
fn launch_kind_text(kind: Option<OnboardingTryoutLaunchKind>) -> &'static str {
    match kind {
        Some(OnboardingTryoutLaunchKind::Opened) => "opened",
        Some(OnboardingTryoutLaunchKind::Executed) => "executed",
        Some(OnboardingTryoutLaunchKind::Prepared) => "prepared",
        None => "none",
    }
}

/// `result.kind`, ou `error` quando não houve resultado.
fn result_kind_text(result: Option<&OnboardingTryoutResult>) -> &'static str {
    match result {
        Some(OnboardingTryoutResult::Launched { kind, .. }) => launch_kind_text(Some(*kind)),
        Some(OnboardingTryoutResult::Routed) => "routed",
        Some(OnboardingTryoutResult::Cancelled) => "cancelled",
        Some(OnboardingTryoutResult::Unavailable(_)) => "unavailable",
        None => "error",
    }
}

/// `IActiveTryoutRun`.
struct ActiveTryoutRun {
    id: String,
    store: Rc<DisposableStore>,
    cancellation: CancellationTokenSource,
    promise: RefCell<Promise<OnboardingTryoutResult>>,
}

/// `OnboardingTryoutService`.
pub struct OnboardingTryoutService {
    store: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<()>>,
    presentation_listeners: Rc<DisposableStore>,
    active_run: RefCell<Option<Rc<ActiveTryoutRun>>>,
    context_keys: RefCell<HashSet<String>>,
    context_key_service: Rc<dyn IContextKeyService>,
    chat_entitlement_service: Rc<dyn IChatEntitlementService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    me: Weak<OnboardingTryoutService>,
}

impl OnboardingTryoutService {
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        chat_entitlement_service: Rc<dyn IChatEntitlementService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_change = store.add_rc(Emitter::new());
        let presentation_listeners = store.add_rc(DisposableStore::new());
        let this = Rc::new_cyclic(|me| Self {
            store: store.clone(),
            on_did_change,
            presentation_listeners,
            active_run: RefCell::new(None),
            context_keys: RefCell::new(HashSet::new()),
            context_key_service: context_key_service.clone(),
            chat_entitlement_service: chat_entitlement_service.clone(),
            environment_service,
            telemetry_service,
            me: me.clone(),
        });

        let refresh = {
            let weak = this.me.clone();
            move |_: &()| {
                if let Some(this) = weak.upgrade() {
                    this.refresh_contributions();
                }
            }
        };
        store.add(onboarding_scenario_registry().on_did_change().listen(refresh.clone()));
        store.add(onboarding_tryout_presentation_registry().on_did_change().listen(refresh));
        let weak = this.me.clone();
        store.add(context_key_service.on_did_change_context().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            let keys = this.context_keys.borrow().clone();
            if event.affects_some(&keys) {
                this.on_did_change.fire(());
            }
        }));
        let announce = {
            let weak = this.me.clone();
            move |_: &()| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change.fire(());
                }
            }
        };
        store.add(chat_entitlement_service.on_did_change_sentiment().listen(announce.clone()));
        store.add(chat_entitlement_service.on_did_change_entitlement().listen(announce.clone()));
        store.add(chat_entitlement_service.on_did_change_anonymous().listen(announce));
        let weak = this.me.clone();
        store.add(plain_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let active = this.active_run.borrow().clone();
            if let Some(active) = active {
                active.store.dispose();
            }
        }));
        this.refresh_contributions();
        this
    }

    fn refresh_contributions(&self) {
        self.context_keys.borrow_mut().clear();
        self.presentation_listeners.clear();
        let mut kinds: Vec<String> = Vec::new();
        for scenario in self.get_tryouts() {
            if let Some(when) = &scenario.when {
                self.context_keys.borrow_mut().extend(when.keys());
            }
            let kind = self.get_presentation_kind(&scenario);
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        for kind in kinds {
            let presentation = onboarding_tryout_presentation_registry().get(&kind);
            if let Some(on_did_change_availability) = presentation.and_then(|presentation| presentation.on_did_change_availability()) {
                let weak = self.me.clone();
                self.presentation_listeners.add(on_did_change_availability.listen(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_change.fire(());
                    }
                }));
            }
        }
        self.on_did_change.fire(());
    }

    fn get_presentation_kind(&self, scenario: &OnboardingScenario) -> String {
        if targets_agents(scenario) && !self.environment_service.is_sessions_window() {
            AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND.to_string()
        } else {
            scenario.presentation.kind.clone()
        }
    }

    /// `doRun`.
    fn do_run(this: Rc<Self>, context: OnboardingTryoutRunContext) -> Promise<OnboardingTryoutResult> {
        async_fn_maybe(async move {
            let id = context.id.clone();
            let token = context.token.clone();
            if token.is_cancellation_requested() || this.store.is_disposed() {
                return Ok(MaybePromise::Value(OnboardingTryoutResult::Cancelled));
            }
            let Some(scenario) = this.get_tryout(&id) else {
                return Err(Error::new(localize("onboarding.tryout.unknown", "The feature example '{0}' is not available in this version.", &[id.as_str()])));
            };
            if let Some(result) = not_ready_result(this.get_availability(&id)) {
                return Ok(MaybePromise::Value(result));
            }

            let presentation_kind = this.get_presentation_kind(&scenario);
            let Some(presentation) = onboarding_tryout_presentation_registry().get(&presentation_kind) else {
                return Ok(MaybePromise::Value(OnboardingTryoutResult::Unavailable(unavailable())));
            };
            let preparation = presentation.prepare(&scenario, &context);
            let prepared = race_cancellation_error(&preparation, &token).await?;
            let run = match prepared {
                OnboardingTryoutPreparation::Ready { run } => run,
                OnboardingTryoutPreparation::Cancelled => return Ok(MaybePromise::Value(OnboardingTryoutResult::Cancelled)),
                OnboardingTryoutPreparation::Unavailable(unavailable) => {
                    return Ok(MaybePromise::Value(OnboardingTryoutResult::Unavailable(unavailable)));
                }
            };
            if token.is_cancellation_requested() {
                return Ok(MaybePromise::Value(OnboardingTryoutResult::Cancelled));
            }
            let same_scenario = this.get_tryout(&id).is_some_and(|current| Rc::ptr_eq(&current, &scenario));
            let same_presentation = onboarding_tryout_presentation_registry().get(&presentation_kind).is_some_and(|current| Rc::ptr_eq(&current, &presentation));
            if !same_scenario || !same_presentation {
                return Ok(MaybePromise::Value(OnboardingTryoutResult::Unavailable(unavailable())));
            }
            if let Some(result) = not_ready_result(this.get_availability(&id)) {
                return Ok(MaybePromise::Value(result));
            }
            Ok(MaybePromise::Promise(race_cancellation_error(&run(), &token)))
        })
    }
}

fn targets_agents(scenario: &OnboardingScenario) -> bool {
    scenario.tryout.as_ref().is_some_and(|tryout| tryout.target_window == Some(OnboardingTryoutTargetWindow::Agents))
}

/// `isTryout`.
fn is_tryout(scenario: &OnboardingScenario) -> bool {
    scenario.tryout.is_some() && matches!(&scenario.trigger, OnboardingTrigger::Command { command_id } if command_id == RUN_ONBOARDING_TRYOUT_COMMAND_ID)
}

/// `unavailable`.
fn unavailable() -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable {
        message: localize("onboarding.tryout.unavailable", "This feature example is not available in this version.", &[]),
        action: None,
    }
}

/// `asResult`, para quem já conferiu `availability.kind !== 'ready'`: `None` quando está pronto.
fn not_ready_result(availability: OnboardingTryoutAvailability) -> Option<OnboardingTryoutResult> {
    match availability {
        OnboardingTryoutAvailability::Ready => None,
        OnboardingTryoutAvailability::Hidden => Some(OnboardingTryoutResult::Unavailable(OnboardingTryoutUnavailable {
            message: localize("onboarding.tryout.aiHidden", "AI features are disabled.", &[]),
            action: None,
        })),
        OnboardingTryoutAvailability::Unavailable(unavailable) => Some(OnboardingTryoutResult::Unavailable(unavailable)),
    }
}

impl IDisposable for OnboardingTryoutService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl IOnboardingTryoutService for OnboardingTryoutService {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn get_tryout(&self, id: &str) -> Option<Rc<OnboardingScenario>> {
        onboarding_scenario_registry().get_scenario(id).filter(|scenario| is_tryout(scenario))
    }

    fn get_tryouts(&self) -> Vec<Rc<OnboardingScenario>> {
        onboarding_scenario_registry().get_scenarios().into_iter().filter(|scenario| is_tryout(scenario)).collect()
    }

    fn get_availability(&self, id: &str) -> OnboardingTryoutAvailability {
        let Some(scenario) = self.get_tryout(id) else {
            return OnboardingTryoutAvailability::Unavailable(unavailable());
        };
        let tryout = scenario.tryout.clone().expect("o filtro de isTryout garante o tryout");
        let is_ai = tryout.is_ai == Some(true);

        if is_ai && self.chat_entitlement_service.sentiment().hidden == Some(true) {
            return OnboardingTryoutAvailability::Hidden;
        }

        if targets_agents(&scenario) && !self.environment_service.is_sessions_window() {
            return match onboarding_tryout_presentation_registry().get(AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND) {
                Some(presentation) => presentation.get_availability(&scenario),
                None => OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable {
                    message: localize("onboarding.tryout.agentsUnavailable", "This example requires the desktop Agents window.", &[]),
                    action: None,
                }),
            };
        }

        if is_ai {
            let sentiment = self.chat_entitlement_service.sentiment();
            if sentiment.disabled_in_workspace == Some(true) {
                return OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable {
                    message: localize("onboarding.tryout.chatDisabledInWorkspace", "Chat is disabled in this workspace.", &[]),
                    action: None,
                });
            }
            let requirement = ChatSetupRequirement {
                completed: sentiment.completed == Some(true),
                disabled: sentiment.disabled == Some(true),
                untrusted: sentiment.untrusted == Some(true),
                entitlement: self.chat_entitlement_service.entitlement(),
                anonymous: self.chat_entitlement_service.anonymous(),
                has_byok_models: self.chat_entitlement_service.has_byok_models(),
            };
            if chat_requires_setup(&requirement) {
                return OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable {
                    message: localize("onboarding.tryout.chatSetupRequired", "Set up Chat before trying this example.", &[]),
                    action: Some(OnboardingTryoutSetupAction {
                        label: localize("onboarding.tryout.setUpChat", "Set Up Chat", &[]),
                        command: OnboardingTryoutCommand { id: "workbench.action.chat.triggerSetup".to_string(), arguments: None },
                    }),
                });
            }
        }

        if let Some(when) = &scenario.when
            && !self.context_key_service.context_matches_rules(Some(when))
        {
            return OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable {
                message: tryout
                    .unavailable_message
                    .clone()
                    .unwrap_or_else(|| localize("onboarding.tryout.notAvailableHere", "This example is not available in the current context.", &[])),
                action: tryout.setup.clone(),
            });
        }

        match onboarding_tryout_presentation_registry().get(&scenario.presentation.kind) {
            Some(presentation) => presentation.get_availability(&scenario),
            None => OnboardingTryoutAvailability::Unavailable(unavailable()),
        }
    }

    fn run(&self, id: &str, token: CancellationToken, options: Option<OnboardingTryoutRunOptions>) -> Promise<OnboardingTryoutResult> {
        if let Err(error) = parse_onboarding_tryout_arguments(&[Dynamic::string(id)]) {
            errors::throw(error);
        }
        if options.as_ref().is_some_and(|options| options.run_id.as_deref().is_some_and(|run_id| !is_uuid(run_id))) {
            errors::throw(Error::new(localize("onboarding.tryout.invalidRunOptions", "The feature example run context is invalid.", &[])));
        }
        let current = self.active_run.borrow().clone();
        if let Some(current) = current
            && current.id == id
            && !current.cancellation.token().is_cancellation_requested()
        {
            return current.promise.borrow().clone();
        }

        let this = self.me.upgrade().expect("o serviço está vivo enquanto é usado");
        let store = DisposableStore::new();
        let previous = self.active_run.borrow().clone();
        if let Some(previous) = previous {
            previous.store.dispose();
        }
        let cancellation = CancellationTokenSource::new(Some(&token));
        let active_run = Rc::new(ActiveTryoutRun {
            id: id.to_string(),
            store: store.clone(),
            cancellation: cancellation.clone(),
            promise: RefCell::new(Promise::resolved(OnboardingTryoutResult::Cancelled)),
        });
        *self.active_run.borrow_mut() = Some(active_run.clone());
        store.add(plain_disposable({
            let cancellation = cancellation.clone();
            move || cancellation.dispose_with(true)
        }));

        let id = id.to_string();
        let promise = je_base::async_::async_fn(async move {
            Promise::resolved(()).await?;
            let scenario = this.get_tryout(&id);
            let source = options.as_ref().map_or(OnboardingTryoutSource::Direct, |options| options.source);
            let run_id = options.and_then(|options| options.run_id).unwrap_or_else(generate_uuid);
            let telemetry_data = scenario
                .filter(|scenario| !targets_agents(scenario) || this.environment_service.is_sessions_window())
                .map(|scenario| Rc::new(TryoutEvent { tryout_id: scenario.id.clone(), run_id, source }));
            let start_time = runtime::date_now();
            let finished = Rc::new(Cell::new(false));
            let launch_result: Rc<Cell<Option<OnboardingTryoutLaunchKind>>> = Rc::new(Cell::new(None));
            let guidance_result: Rc<RefCell<Option<OnboardingRunResult>>> = Rc::new(RefCell::new(None));
            let on_did_launch: Rc<dyn Fn(OnboardingTryoutLaunchKind)> = {
                let (finished, launch_result, cancellation) = (finished.clone(), launch_result.clone(), cancellation.clone());
                let (telemetry_service, telemetry_data) = (this.telemetry_service.clone(), telemetry_data.clone());
                Rc::new(move |kind| {
                    if finished.get() || cancellation.token().is_cancellation_requested() || launch_result.get().is_some() {
                        return;
                    }
                    launch_result.set(Some(kind));
                    if let Some(data) = &telemetry_data {
                        telemetry_service.public_log2("onboarding.tryoutStarted", Some(Dynamic::object(data.entries())));
                    }
                })
            };
            let on_did_finish_guidance: Rc<dyn Fn(&OnboardingRunResult)> = {
                let (finished, guidance_result) = (finished.clone(), guidance_result.clone());
                Rc::new(move |guidance| {
                    if !finished.get() {
                        *guidance_result.borrow_mut() = Some(*guidance);
                    }
                })
            };

            let context = OnboardingTryoutRunContext {
                id: id.clone(),
                token: cancellation.token(),
                store: store.clone(),
                on_did_launch: Some(on_did_launch.clone()),
                on_did_finish_guidance: Some(on_did_finish_guidance),
            };
            let mut result: Option<OnboardingTryoutResult> = None;
            let outcome = match Self::do_run(this.clone(), context).await {
                Ok(done) => {
                    result = Some(done.clone());
                    let launched = match &done {
                        OnboardingTryoutResult::Launched { kind, .. } => errors::catch(|| on_did_launch(*kind)),
                        _ => Ok(()),
                    };
                    launched.map(|()| done)
                }
                Err(error) => Err(error),
            };
            let outcome = match outcome {
                Err(error) if is_cancellation_error(&error) => {
                    result = Some(OnboardingTryoutResult::Cancelled);
                    Ok(OnboardingTryoutResult::Cancelled)
                }
                other => other,
            };

            // finally
            finished.set(true);
            let is_current = this.active_run.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &active_run));
            if is_current {
                *this.active_run.borrow_mut() = None;
            }
            store.dispose();
            if let Some(data) = &telemetry_data
                && result != Some(OnboardingTryoutResult::Routed)
            {
                let guidance = *guidance_result.borrow();
                let mut entries = data.entries();
                entries.extend([
                    ("result", Dynamic::string(result_kind_text(result.as_ref()))),
                    ("launchResult", Dynamic::string(launch_kind_text(launch_result.get()))),
                    (
                        "guidanceOutcome",
                        match guidance {
                            Some(guidance) if guidance.shown => Dynamic::string(guidance.outcome.as_str()),
                            Some(_) => Dynamic::string("notShown"),
                            None => Dynamic::Undefined,
                        },
                    ),
                    ("dismissReason", guidance.map_or(Dynamic::Undefined, |guidance| Dynamic::string(guidance.dismiss_reason.as_str()))),
                    ("durationMs", Dynamic::Number(runtime::date_now().saturating_sub(start_time) as f64)),
                ]);
                this.telemetry_service.public_log2("onboarding.tryoutOutcome", Some(Dynamic::object(entries)));
            }
            outcome
        });
        *active_run.promise.borrow_mut() = promise.clone();
        promise
    }
}
