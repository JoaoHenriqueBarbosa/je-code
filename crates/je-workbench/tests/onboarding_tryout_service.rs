//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingTryoutService.test.ts` (suíte
//! `OnboardingTryoutService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `store.add` vira `guard.add` / `guard.add_rc`.
//! - **`await`.** É o `.await` da promessa do porte; `assert.rejects(promise, /texto/)` é a rejeição conferida pelo
//!   trecho da mensagem, e `candidate => candidate === error` compara a mensagem do erro lançado pelo appender.
//! - **`for (...) test(...)`.** Vira um teste por combinação, na ordem do original, sobre uma função comum.
//! - **`upcastPartial<...>`.** O `IChatEntitlementService` é o recorte do serviço (com o `sentiment` num `Cell` que o
//!   caso altera), o ambiente é o `TestEnvironmentService` com `is_sessions_window` trocado, e o `ContextKeyService` é o
//!   real, com o `TestConfigurationService`.
//! - **`CapturingTelemetryService`.** É o dublê que guarda `(nome, dados)` de cada `publicLog2`; o `snapshot` troca o
//!   `runId` por "é um UUID" e o `durationMs` por "é um inteiro não negativo", como o original.
//! - **`registerOnboardingTryoutPresentation({ isPayload, getAvailability, prepare })`.** É o
//!   [`OnboardingTryoutPresentationDefinition`] com o payload `undefined` como [`Dynamic::Undefined`]; as funções
//!   `async` que devolvem a preparação são promessas já resolvidas (ou [`async_fn`] quando o corpo espera).
//! - **`TelemetryService.createWithLevel`.** É o `TelemetryService` real com um appender que lança ou que anota os nomes.
//! - **`deepStrictEqual` de objetos literais.** É [`assert_deep_strict_equal`] sobre [`Dynamic`] quando há objetos com
//!   `undefined`, e `assert_eq!` de tuplas nos demais casos.
//! - **`service.getTryout(...)?.trigger`.** O gatilho não tem igualdade; o caso confere a variante `Command` e o id.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_base::uuid::is_uuid;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::product::{EnabledTelemetryLevels, IProductService, ProductService};
use je_platform::telemetry::{
    CommonPropertyValue, ITelemetryAppender, ITelemetryService, TelemetryLevel, TelemetryService, TelemetryServiceConfig,
};
use je_workbench::contrib::onboarding::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext, onboarding_presentation_registry};
use je_workbench::contrib::onboarding::onboarding_registry::onboarding_scenario_registry;
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingDismissReason, OnboardingOutcome, OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario, OnboardingTrigger,
};
use je_workbench::contrib::onboarding::onboarding_tryout::{
    AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND, IOnboardingTryoutPresentation, OnboardingTryoutAvailability, OnboardingTryoutCommand,
    OnboardingTryoutLaunchKind, OnboardingTryoutMetadata, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutSetupAction, OnboardingTryoutTargetWindow, OnboardingTryoutUnavailable,
    RUN_ONBOARDING_TRYOUT_COMMAND_ID, onboarding_tryout_presentation_registry,
};
use je_workbench::contrib::onboarding::onboarding_tryout_service::{
    ChatEntitlement, ChatSentiment, IChatEntitlementService, IOnboardingTryoutService, OnboardingTryout,
    OnboardingTryoutPresentationDefinition, OnboardingTryoutRunOptions, OnboardingTryoutService, OnboardingTryoutSource,
    create_onboarding_tryout_uri, parse_onboarding_tryout_arguments, parse_onboarding_tryout_uri, register_onboarding_tryout,
    register_onboarding_tryout_presentation,
};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};

/// `CapturingTelemetryService`.
#[derive(Default)]
struct CapturingTelemetryService {
    events: RefCell<Vec<(String, Option<Dynamic>)>>,
}

impl CapturingTelemetryService {
    fn names(&self) -> Vec<String> {
        self.events.borrow().iter().map(|(name, _)| name.clone()).collect()
    }

    /// `events[index].data`.
    fn data(&self, index: usize) -> Dynamic {
        self.events.borrow()[index].1.clone().unwrap_or(Dynamic::Undefined)
    }

    /// `events[index].data?.runId`.
    fn run_id(&self, index: usize) -> Dynamic {
        self.data(index).get_str("runId")
    }

    /// `snapshot`.
    fn snapshot(&self) -> Dynamic {
        Dynamic::array(self.events.borrow().iter().map(|(name, data)| {
            let data = data.clone().unwrap_or(Dynamic::Undefined);
            let entries: Vec<(String, Dynamic)> = data
                .own_keys()
                .into_iter()
                .map(|key| {
                    let value = data.get(&key);
                    let key = key.to_string_lossy();
                    let value = match key.as_str() {
                        "runId" => Dynamic::Bool(matches!(&value, Dynamic::String(text) if is_uuid(&text.to_string_lossy()))),
                        "durationMs" => Dynamic::Bool(matches!(value, Dynamic::Number(number) if number.fract() == 0.0 && number >= 0.0)),
                        _ => value,
                    };
                    (key, value)
                })
                .collect();
            Dynamic::object([("name", Dynamic::string(name)), ("data", Dynamic::object(entries))])
        }))
    }
}

impl ITelemetryService for CapturingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::None
    }
    fn session_id(&self) -> String {
        "someValue.sessionId".to_string()
    }
    fn machine_id(&self) -> String {
        "someValue.machineId".to_string()
    }
    fn sqm_id(&self) -> String {
        "someValue.sqmId".to_string()
    }
    fn dev_device_id(&self) -> String {
        "someValue.devDeviceId".to_string()
    }
    fn first_session_date(&self) -> String {
        "someValue.firstSessionDate".to_string()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, _event_name: &str, _data: Option<je_base::actions::ITelemetryData>) {}
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.events.borrow_mut().push((event_name.to_string(), data));
        }
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<je_base::actions::ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// O recorte do `IChatEntitlementService` que o caso `hidden AI` altera por meio do `sentiment`.
struct TestChatEntitlementService {
    sentiment: Rc<Cell<ChatSentiment>>,
    changed: Event<()>,
}

impl IChatEntitlementService for TestChatEntitlementService {
    fn sentiment(&self) -> ChatSentiment {
        self.sentiment.get()
    }
    fn entitlement(&self) -> ChatEntitlement {
        ChatEntitlement::Pro
    }
    fn anonymous(&self) -> bool {
        false
    }
    fn has_byok_models(&self) -> bool {
        false
    }
    fn on_did_change_sentiment(&self) -> Event<()> {
        self.changed.clone()
    }
    fn on_did_change_entitlement(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_anonymous(&self) -> Event<()> {
        Event::none()
    }
}

struct Fixture {
    service: Rc<OnboardingTryoutService>,
    context: Rc<ContextKeyService>,
    sentiment: Rc<Cell<ChatSentiment>>,
    changed: Rc<Emitter<()>>,
    telemetry: Rc<CapturingTelemetryService>,
}

/// `createService(agents, telemetryService?)`.
fn create_service(guard: &LeakGuard, agents: bool, telemetry_service: Option<Rc<dyn ITelemetryService>>) -> Fixture {
    let configuration = TestConfigurationService::new(None);
    guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
    let context = guard.add_rc(ContextKeyService::new(configuration));
    let sentiment = Rc::new(Cell::new(ChatSentiment { completed: Some(true), installed: Some(true), ..ChatSentiment::default() }));
    let changed = guard.add_rc(Emitter::<()>::new());
    let chat: Rc<dyn IChatEntitlementService> = Rc::new(TestChatEntitlementService { sentiment: sentiment.clone(), changed: changed.event() });
    let telemetry = Rc::new(CapturingTelemetryService::default());
    let environment: Rc<dyn IWorkbenchEnvironmentService> =
        TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window: agents, ..EnvironmentOverrides::default() });
    let context_service: Rc<dyn IContextKeyService> = context.clone();
    let telemetry_dyn: Rc<dyn ITelemetryService> = telemetry.clone();
    let service = guard.add_rc(OnboardingTryoutService::new(context_service, chat, environment, telemetry_service.unwrap_or(telemetry_dyn)));
    Fixture { service, context, sentiment, changed, telemetry }
}

/// `registerTryout(overrides)`.
fn register_tryout(guard: &LeakGuard, overrides: impl FnOnce(&mut OnboardingTryout)) -> DisposableRef {
    let mut tryout = OnboardingTryout {
        id: "test.tryout".to_string(),
        when: None,
        presentation: OnboardingPresentationRef { kind: "test.tryoutPresentation".to_string(), payload: Dynamic::Undefined },
        metadata: OnboardingTryoutMetadata {
            title: "Test example".to_string(),
            description: "Prepare the test example.".to_string(),
            ..OnboardingTryoutMetadata::default()
        },
    };
    overrides(&mut tryout);
    guard.add(register_onboarding_tryout(tryout))
}

/// `registerPresentation(prepare)`, com o `getAvailability` padrão (`ready`).
fn register_presentation(guard: &LeakGuard, prepare: impl Fn(OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> + 'static) -> DisposableRef {
    guard.add(register_onboarding_tryout_presentation(OnboardingTryoutPresentationDefinition {
        kind: "test.tryoutPresentation".to_string(),
        on_did_change_availability: None,
        is_payload: Rc::new(|value: &Dynamic| matches!(value, Dynamic::Undefined)),
        get_availability: Rc::new(|_: &Dynamic| OnboardingTryoutAvailability::Ready),
        prepare: Rc::new(move |_: &Dynamic, context: &OnboardingTryoutRunContext| prepare(context.clone())),
    }))
}

/// A apresentação do `registerWindowOpener(open)`.
struct WindowOpener {
    open: Rc<dyn Fn(&str)>,
}

impl IOnboardingTryoutPresentation for WindowOpener {
    fn kind(&self) -> &str {
        AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND
    }

    fn get_availability(&self, _scenario: &OnboardingScenario) -> OnboardingTryoutAvailability {
        OnboardingTryoutAvailability::Ready
    }

    fn prepare(&self, scenario: &OnboardingScenario, _context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let (id, open) = (scenario.id.clone(), self.open.clone());
        Promise::resolved(ready(move || {
            open(&id);
            Promise::resolved(OnboardingTryoutResult::Routed)
        }))
    }
}

/// `registerWindowOpener(open)`.
fn register_window_opener(guard: &LeakGuard, open: impl Fn(&str) + 'static) -> DisposableRef {
    let opener: Rc<dyn IOnboardingTryoutPresentation> = Rc::new(WindowOpener { open: Rc::new(open) });
    guard.add(onboarding_tryout_presentation_registry().register(opener))
}

/// `{ kind: 'ready', run }`.
fn ready(run: impl Fn() -> Promise<OnboardingTryoutResult> + 'static) -> OnboardingTryoutPreparation {
    OnboardingTryoutPreparation::Ready { run: Rc::new(run) }
}

fn launched(kind: OnboardingTryoutLaunchKind) -> OnboardingTryoutResult {
    OnboardingTryoutResult::Launched { kind, target_scope: None }
}

fn unavailable(message: &str) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable { message: message.to_string(), action: None }
}

fn run_options(source: OnboardingTryoutSource, run_id: Option<&str>) -> Option<OnboardingTryoutRunOptions> {
    Some(OnboardingTryoutRunOptions { source, run_id: run_id.map(str::to_string) })
}

/// `service.run(id)`.
fn run(fixture: &Fixture, id: &str) -> Promise<OnboardingTryoutResult> {
    fixture.service.run(id, CancellationToken::None, None)
}

/// `assert.rejects(promise, /expected/)`.
async fn assert_rejects(promise: Promise<OnboardingTryoutResult>, expected: &str) {
    let error = promise.await.expect_err("a promessa deveria rejeitar");
    assert!(error.to_string().contains(expected), "mensagem inesperada: {error}");
}

fn opt(value: Option<&str>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::string)
}

fn event(name: &str, entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object([("name", Dynamic::string(name)), ("data", Dynamic::object(entries))])
}

/// `{ name: 'onboarding.tryoutStarted', data: { tryoutId, source, runId: true } }`.
fn started(source: &str) -> Dynamic {
    event("onboarding.tryoutStarted", vec![("tryoutId", Dynamic::string("test.tryout")), ("source", Dynamic::string(source)), ("runId", Dynamic::Bool(true))])
}

/// `{ name: 'onboarding.tryoutOutcome', data: { ... } }`.
fn outcome(source: &str, result: &str, launch_result: &str, guidance_outcome: Option<&str>, dismiss_reason: Option<&str>) -> Dynamic {
    event(
        "onboarding.tryoutOutcome",
        vec![
            ("tryoutId", Dynamic::string("test.tryout")),
            ("source", Dynamic::string(source)),
            ("runId", Dynamic::Bool(true)),
            ("result", Dynamic::string(result)),
            ("launchResult", Dynamic::string(launch_result)),
            ("guidanceOutcome", opt(guidance_outcome)),
            ("dismissReason", opt(dismiss_reason)),
            ("durationMs", Dynamic::Bool(true)),
        ],
    )
}

fn events_of(events: Vec<Dynamic>) -> Dynamic {
    Dynamic::array(events)
}

fn kind_text(kind: OnboardingTryoutLaunchKind) -> &'static str {
    match kind {
        OnboardingTryoutLaunchKind::Opened => "opened",
        OnboardingTryoutLaunchKind::Executed => "executed",
        OnboardingTryoutLaunchKind::Prepared => "prepared",
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn reports_a_cohort_marker_and_outcome(kind: OnboardingTryoutLaunchKind) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        register_tryout(&guard, |_| {});
        register_presentation(&guard, move |_| Promise::resolved(ready(move || Promise::resolved(launched(kind)))));

        fixture.service.run("test.tryout", CancellationToken::None, run_options(OnboardingTryoutSource::ReleaseNotes, None)).await.unwrap();

        let same_run = fixture.telemetry.run_id(0).deep_strict_equals(&fixture.telemetry.run_id(1));
        assert_deep_strict_equal(&fixture.telemetry.snapshot(), &events_of(vec![
            started("releaseNotes"),
            outcome("releaseNotes", kind_text(kind), kind_text(kind), None, None),
        ]));
        assert!(same_run);
    });
}

#[test]
fn reports_a_cohort_marker_and_outcome_for_an_opened_example() {
    reports_a_cohort_marker_and_outcome(OnboardingTryoutLaunchKind::Opened);
}

#[test]
fn reports_a_cohort_marker_and_outcome_for_an_executed_example() {
    reports_a_cohort_marker_and_outcome(OnboardingTryoutLaunchKind::Executed);
}

#[test]
fn reports_a_cohort_marker_and_outcome_for_a_prepared_example() {
    reports_a_cohort_marker_and_outcome(OnboardingTryoutLaunchKind::Prepared);
}

#[test]
fn reports_a_launch_before_guidance_finishes_without_duplicating_it_at_completion() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let launched_promise = Rc::new(DeferredPromise::<()>::new());
        let finish = Rc::new(DeferredPromise::<()>::new());
        let run_id = "01234567-89ab-4cde-8fab-0123456789ab";
        register_tryout(&guard, |_| {});
        {
            let (launched_promise, finish) = (launched_promise.clone(), finish.clone());
            register_presentation(&guard, move |context| {
                let (launched_promise, finish) = (launched_promise.clone(), finish.clone());
                Promise::resolved(ready(move || {
                    let on_did_launch = context.on_did_launch.clone().expect("onDidLaunch");
                    let on_did_finish_guidance = context.on_did_finish_guidance.clone().expect("onDidFinishGuidance");
                    on_did_launch(OnboardingTryoutLaunchKind::Opened);
                    launched_promise.complete(());
                    let finish = finish.clone();
                    async_fn(async move {
                        finish.promise.clone().await?;
                        on_did_finish_guidance(&OnboardingRunResult {
                            outcome: OnboardingOutcome::Skipped,
                            shown: true,
                            dismiss_reason: OnboardingDismissReason::EscapeKey,
                            last_step_index: 0.0,
                            step_count: 2.0,
                        });
                        Ok(launched(OnboardingTryoutLaunchKind::Opened))
                    })
                }))
            });
        }

        let pending = fixture.service.run("test.tryout", CancellationToken::None, run_options(OnboardingTryoutSource::ExternalLink, Some(run_id)));
        launched_promise.promise.clone().await.unwrap();
        let before_finishing = fixture.telemetry.names();
        finish.complete(());
        pending.await.unwrap();

        assert_eq!(before_finishing, strings(&["onboarding.tryoutStarted"]));
        for index in 0..2 {
            assert!(fixture.telemetry.run_id(index).deep_strict_equals(&Dynamic::string(run_id)));
        }
        assert_deep_strict_equal(&fixture.telemetry.snapshot(), &events_of(vec![
            started("externalLink"),
            outcome("externalLink", "opened", "opened", Some("skipped"), Some("escapeKey")),
        ]));
    });
}

#[test]
fn does_not_report_starts_or_sensitive_details_for_failed_and_unavailable_attempts() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        register_tryout(&guard, |_| {});
        let failed = register_presentation(&guard, |_| Promise::rejected(Error::new("sensitive failure detail")));
        assert_rejects(run(&fixture, "test.tryout"), "sensitive failure detail").await;
        failed.dispose();
        register_presentation(&guard, |_| Promise::resolved(OnboardingTryoutPreparation::Unavailable(unavailable("sensitive availability detail"))));
        run(&fixture, "test.tryout").await.unwrap();

        assert_deep_strict_equal(&fixture.telemetry.snapshot(), &events_of(
            ["error", "unavailable"].into_iter().map(|result| outcome("direct", result, "none", None, None)).collect(),
        ));
    });
}

#[test]
fn ignores_late_lifecycle_callbacks_after_cancellation() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let launched_promise = Rc::new(DeferredPromise::<OnboardingTryoutRunContext>::new());
        let finish = Rc::new(DeferredPromise::<OnboardingTryoutResult>::new());
        register_tryout(&guard, |_| {});
        {
            let (launched_promise, finish) = (launched_promise.clone(), finish.clone());
            register_presentation(&guard, move |context| {
                let (launched_promise, finish) = (launched_promise.clone(), finish.clone());
                Promise::resolved(ready(move || {
                    let on_did_launch = context.on_did_launch.clone().expect("onDidLaunch");
                    on_did_launch(OnboardingTryoutLaunchKind::Prepared);
                    launched_promise.complete(context.clone());
                    finish.promise.clone()
                }))
            });
        }

        let pending = fixture.service.run("test.tryout", cancellation.token(), None);
        let context = launched_promise.promise.clone().await.unwrap();
        cancellation.cancel();
        pending.await.unwrap();
        context.on_did_launch.clone().expect("onDidLaunch")(OnboardingTryoutLaunchKind::Opened);
        context.on_did_finish_guidance.clone().expect("onDidFinishGuidance")(&OnboardingRunResult {
            outcome: OnboardingOutcome::Completed,
            shown: true,
            dismiss_reason: OnboardingDismissReason::Completed,
            last_step_index: 1.0,
            step_count: 2.0,
        });
        finish.complete(launched(OnboardingTryoutLaunchKind::Opened));

        assert_deep_strict_equal(&fixture.telemetry.snapshot(), &events_of(vec![
            started("direct"),
            outcome("direct", "cancelled", "prepared", None, None),
        ]));
    });
}

/// `ITelemetryAppender` do caso que lança uma vez no evento dado (o `log` do appender do original).
struct ThrowingAppender {
    event_name: &'static str,
    should_throw: Cell<bool>,
    error: Error,
}

impl ITelemetryAppender for ThrowingAppender {
    fn log(&self, event_name: &str, _data: &Dynamic) {
        if self.should_throw.get() && event_name == self.event_name {
            self.should_throw.set(false);
            errors::throw(self.error.clone());
        }
    }

    fn flush(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `ITelemetryAppender` que só anota os nomes (`log: name => events.push(name)`).
#[derive(Default)]
struct NamesAppender {
    events: RefCell<Vec<String>>,
}

impl ITelemetryAppender for NamesAppender {
    fn log(&self, event_name: &str, _data: &Dynamic) {
        self.events.borrow_mut().push(event_name.to_string());
    }

    fn flush(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `upcastPartial<IProductService>({ enabledTelemetryLevels: { usage, error: true } })`.
fn product(usage: bool) -> Rc<dyn IProductService> {
    Rc::new(ProductService { enabled_telemetry_levels: Some(EnabledTelemetryLevels { usage, error: true }), ..ProductService::default() })
}

fn cleans_up_and_allows_another_run_when_telemetry_throws(event_name: &'static str) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let error = Error::new("Telemetry appender failed");
        let appender: Rc<dyn ITelemetryAppender> = Rc::new(ThrowingAppender { event_name, should_throw: Cell::new(true), error: error.clone() });
        let telemetry = guard.add_rc(TelemetryService::create_with_level(
            TelemetryServiceConfig { appenders: vec![appender], ..TelemetryServiceConfig::default() },
            TelemetryLevel::Usage,
            product(true),
        ));
        let telemetry_dyn: Rc<dyn ITelemetryService> = telemetry;
        let fixture = create_service(&guard, true, Some(telemetry_dyn));
        let contexts: Rc<RefCell<Vec<OnboardingTryoutRunContext>>> = Rc::default();
        let disposed = Rc::new(Cell::new(0));
        register_tryout(&guard, |_| {});
        {
            let (contexts, disposed) = (contexts.clone(), disposed.clone());
            register_presentation(&guard, move |context| {
                contexts.borrow_mut().push(context.clone());
                let disposed = disposed.clone();
                context.store.add(plain_disposable(move || disposed.set(disposed.get() + 1)));
                Promise::resolved(ready(|| Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))))
            });
        }

        let failure = run(&fixture, "test.tryout").await.expect_err("o telemetry deveria lançar");
        assert_eq!(failure.to_string(), error.to_string());
        let after_failure = (disposed.get(), contexts.borrow()[0].token.is_cancellation_requested());
        let result = run(&fixture, "test.tryout").await.unwrap();

        assert_eq!(
            (after_failure, result, contexts.borrow().len(), disposed.get()),
            ((1, true), launched(OnboardingTryoutLaunchKind::Executed), 2, 2)
        );
    });
}

#[test]
fn cleans_up_and_allows_another_run_when_onboarding_tryout_started_telemetry_throws() {
    cleans_up_and_allows_another_run_when_telemetry_throws("onboarding.tryoutStarted");
}

#[test]
fn cleans_up_and_allows_another_run_when_onboarding_tryout_outcome_telemetry_throws() {
    cleans_up_and_allows_another_run_when_telemetry_throws("onboarding.tryoutOutcome");
}

#[test]
fn does_not_report_a_launch_when_preparation_is_cancelled() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        register_tryout(&guard, |_| {});
        register_presentation(&guard, |_| Promise::resolved(OnboardingTryoutPreparation::Cancelled));
        run(&fixture, "test.tryout").await.unwrap();

        assert_deep_strict_equal(&fixture.telemetry.snapshot(), &events_of(vec![outcome("direct", "cancelled", "none", None, None)]));
    });
}

fn respects_telemetry_level(level: TelemetryLevel, usage_enabled: bool) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let appender = Rc::new(NamesAppender::default());
        let appender_dyn: Rc<dyn ITelemetryAppender> = appender.clone();
        let telemetry = guard.add_rc(TelemetryService::create_with_level(
            TelemetryServiceConfig { appenders: vec![appender_dyn], ..TelemetryServiceConfig::default() },
            level,
            product(usage_enabled),
        ));
        let telemetry_dyn: Rc<dyn ITelemetryService> = telemetry;
        let fixture = create_service(&guard, true, Some(telemetry_dyn));
        register_tryout(&guard, |_| {});
        register_presentation(&guard, |_| Promise::resolved(ready(|| Promise::resolved(launched(OnboardingTryoutLaunchKind::Opened)))));
        run(&fixture, "test.tryout").await.unwrap();

        let expected = if level == TelemetryLevel::Usage && usage_enabled { strings(&["onboarding.tryoutStarted", "onboarding.tryoutOutcome"]) } else { Vec::new() };
        assert_eq!(*appender.events.borrow(), expected);
    });
}

#[test]
fn respects_telemetry_level_none_with_product_usage_telemetry_true() {
    respects_telemetry_level(TelemetryLevel::None, true);
}

#[test]
fn respects_telemetry_level_error_with_product_usage_telemetry_true() {
    respects_telemetry_level(TelemetryLevel::Error, true);
}

#[test]
fn respects_telemetry_level_usage_with_product_usage_telemetry_false() {
    respects_telemetry_level(TelemetryLevel::Usage, false);
}

#[test]
fn respects_telemetry_level_usage_with_product_usage_telemetry_true() {
    respects_telemetry_level(TelemetryLevel::Usage, true);
}

#[test]
fn registering_and_reading_an_example_do_not_prepare_or_run_it() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let prepared = Rc::new(Cell::new(0));
        {
            let prepared = prepared.clone();
            register_presentation(&guard, move |_| {
                prepared.set(prepared.get() + 1);
                Promise::resolved(ready(|| Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))))
            });
        }
        register_tryout(&guard, |_| {});

        let availability = fixture.service.get_availability("test.tryout");
        let ids: Vec<String> = fixture.service.get_tryouts().iter().map(|scenario| scenario.id.clone()).collect();
        let trigger_is_the_tryout_command = matches!(
            fixture.service.get_tryout("test.tryout").map(|scenario| scenario.trigger.clone()),
            Some(OnboardingTrigger::Command { command_id }) if command_id == RUN_ONBOARDING_TRYOUT_COMMAND_ID
        );
        assert_eq!(
            (availability, ids, trigger_is_the_tryout_command, prepared.get()),
            (OnboardingTryoutAvailability::Ready, strings(&["test.tryout"]), true, 0)
        );
    });
}

/// `IOnboardingPresentation` do caso dos espaços de nome independentes.
struct AutomaticPresentation;

impl IOnboardingPresentation for AutomaticPresentation {
    fn kind(&self) -> &str {
        "test.tryoutPresentation"
    }

    fn run(&self, _scenario: &OnboardingScenario, _context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        Promise::resolved(OnboardingRunResult {
            outcome: OnboardingOutcome::Completed,
            shown: false,
            dismiss_reason: OnboardingDismissReason::Completed,
            last_step_index: 0.0,
            step_count: 0.0,
        })
    }
}

#[test]
fn tryout_and_automatic_presentations_use_independent_kind_namespaces() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let automatic: Rc<dyn IOnboardingPresentation> = Rc::new(AutomaticPresentation);
        guard.add(onboarding_presentation_registry().register(automatic.clone()));
        register_presentation(&guard, |_| Promise::resolved(ready(|| Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed)))));
        register_tryout(&guard, |_| {});

        let registered = onboarding_presentation_registry().get(automatic.kind()).is_some_and(|current| Rc::ptr_eq(&current, &automatic));
        assert_eq!((registered, fixture.service.get_availability("test.tryout")), (true, OnboardingTryoutAvailability::Ready));
    });
}

#[test]
fn runs_with_prepared_context_and_disposes_its_lifetime_afterward() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        register_tryout(&guard, |_| {});
        {
            let events = events.clone();
            register_presentation(&guard, move |context| {
                events.borrow_mut().push(format!("prepare:{}", context.id));
                let disposed_events = events.clone();
                context.store.add(plain_disposable(move || disposed_events.borrow_mut().push("dispose".to_string())));
                let (run_events, prepared_target) = (events.clone(), "selected-target");
                Promise::resolved(ready(move || {
                    run_events.borrow_mut().push(format!("run:{prepared_target}"));
                    Promise::resolved(launched(OnboardingTryoutLaunchKind::Prepared))
                }))
            });
        }

        let result = run(&fixture, "test.tryout").await.unwrap();
        assert_eq!(
            (result, events.borrow().clone()),
            (launched(OnboardingTryoutLaunchKind::Prepared), strings(&["prepare:test.tryout", "run:selected-target", "dispose"]))
        );
    });
}

#[test]
fn joins_concurrent_invocations_while_preparation_is_pending() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let started_promise = Rc::new(DeferredPromise::<()>::new());
        let finish = Rc::new(DeferredPromise::<OnboardingTryoutPreparation>::new());
        let preparations = Rc::new(Cell::new(0));
        let executions = Rc::new(Cell::new(0));
        register_tryout(&guard, |_| {});
        {
            let (started_promise, finish, preparations) = (started_promise.clone(), finish.clone(), preparations.clone());
            register_presentation(&guard, move |_| {
                preparations.set(preparations.get() + 1);
                started_promise.complete(());
                finish.promise.clone()
            });
        }
        let first = fixture.service.run("test.tryout", CancellationToken::None, run_options(OnboardingTryoutSource::ReleaseNotes, None));
        started_promise.promise.clone().await.unwrap();
        let second = run(&fixture, "test.tryout");
        {
            let executions = executions.clone();
            finish.complete(ready(move || {
                executions.set(executions.get() + 1);
                Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))
            }));
        }

        let results = (first.await.unwrap(), second.await.unwrap());
        let events: Vec<(String, Dynamic)> = (0..fixture.telemetry.names().len()).map(|index| (fixture.telemetry.names()[index].clone(), fixture.telemetry.data(index).get_str("source"))).collect();
        assert_eq!(
            (results, preparations.get(), executions.get()),
            ((launched(OnboardingTryoutLaunchKind::Executed), launched(OnboardingTryoutLaunchKind::Executed)), 1, 1)
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "onboarding.tryoutStarted");
        assert_eq!(events[1].0, "onboarding.tryoutOutcome");
        for (_, source) in &events {
            assert!(source.deep_strict_equals(&Dynamic::string("releaseNotes")));
        }
    });
}

#[test]
fn a_different_invocation_cancels_the_active_run_before_starting() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let first_started = Rc::new(DeferredPromise::<()>::new());
        let first_preparation = Rc::new(DeferredPromise::<OnboardingTryoutPreparation>::new());
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        register_tryout(&guard, |_| {});
        register_tryout(&guard, |tryout| tryout.id = "test.otherTryout".to_string());
        {
            let (first_started, first_preparation, events) = (first_started.clone(), first_preparation.clone(), events.clone());
            register_presentation(&guard, move |context| {
                events.borrow_mut().push(format!("prepare:{}", context.id));
                if context.id == "test.tryout" {
                    first_started.complete(());
                    return first_preparation.promise.clone();
                }
                let events = events.clone();
                Promise::resolved(ready(move || {
                    events.borrow_mut().push(format!("run:{}", context.id));
                    Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))
                }))
            });
        }

        let first = run(&fixture, "test.tryout");
        first_started.promise.clone().await.unwrap();
        let second = run(&fixture, "test.otherTryout");

        assert_eq!(
            (first.await.unwrap(), second.await.unwrap()),
            (OnboardingTryoutResult::Cancelled, launched(OnboardingTryoutLaunchKind::Executed))
        );
        assert_eq!(*events.borrow(), strings(&["prepare:test.tryout", "prepare:test.otherTryout", "run:test.otherTryout"]));
    });
}

#[test]
fn a_cancelled_invocation_does_not_absorb_a_new_request_for_the_same_id() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let first_started = Rc::new(DeferredPromise::<()>::new());
        let first_preparation = Rc::new(DeferredPromise::<OnboardingTryoutPreparation>::new());
        let preparations = Rc::new(Cell::new(0));
        register_tryout(&guard, |_| {});
        {
            let (first_started, first_preparation, preparations) = (first_started.clone(), first_preparation.clone(), preparations.clone());
            register_presentation(&guard, move |_| {
                preparations.set(preparations.get() + 1);
                if preparations.get() == 1 {
                    first_started.complete(());
                    return first_preparation.promise.clone();
                }
                Promise::resolved(ready(|| Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))))
            });
        }

        let first = fixture.service.run("test.tryout", cancellation.token(), None);
        first_started.promise.clone().await.unwrap();
        cancellation.cancel();
        let second = run(&fixture, "test.tryout");

        assert_eq!(
            ((first.await.unwrap(), second.await.unwrap()), preparations.get()),
            ((OnboardingTryoutResult::Cancelled, launched(OnboardingTryoutLaunchKind::Executed)), 2)
        );
    });
}

#[test]
fn rechecks_conditions_after_asynchronous_preparation() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let enabled = fixture.context.create_key("tryoutEnabled", Dynamic::Bool(true));
        let ran = Rc::new(Cell::new(false));
        register_tryout(&guard, |tryout| {
            tryout.when = Some(ContextKeyExpr::has("tryoutEnabled"));
            tryout.metadata.unavailable_message = Some("Enable the example.".to_string());
        });
        {
            let ran = ran.clone();
            register_presentation(&guard, move |_| {
                enabled.set(Dynamic::Bool(false));
                let ran = ran.clone();
                Promise::resolved(ready(move || {
                    ran.set(true);
                    Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))
                }))
            });
        }

        let result = run(&fixture, "test.tryout").await.unwrap();
        assert_eq!(
            (result, ran.get()),
            (OnboardingTryoutResult::Unavailable(OnboardingTryoutUnavailable { message: "Enable the example.".to_string(), action: None }), false)
        );
    });
}

#[test]
fn does_not_run_an_example_removed_during_preparation() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let ran = Rc::new(Cell::new(false));
        let registration = register_tryout(&guard, |_| {});
        {
            let ran = ran.clone();
            register_presentation(&guard, move |_| {
                registration.dispose();
                let ran = ran.clone();
                Promise::resolved(ready(move || {
                    ran.set(true);
                    Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))
                }))
            });
        }

        let result = run(&fixture, "test.tryout").await.unwrap();
        assert_eq!((matches!(result, OnboardingTryoutResult::Unavailable(_)), ran.get()), (true, false));
    });
}

#[test]
fn cancelling_preparation_prevents_execution_and_disposes_context() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let started_promise = Rc::new(DeferredPromise::<()>::new());
        let finish = Rc::new(DeferredPromise::<OnboardingTryoutPreparation>::new());
        let disposed = Rc::new(Cell::new(false));
        let ran = Rc::new(Cell::new(false));
        register_tryout(&guard, |_| {});
        {
            let (started_promise, finish, disposed) = (started_promise.clone(), finish.clone(), disposed.clone());
            register_presentation(&guard, move |context| {
                let disposed = disposed.clone();
                context.store.add(plain_disposable(move || disposed.set(true)));
                started_promise.complete(());
                finish.promise.clone()
            });
        }

        let pending = fixture.service.run("test.tryout", cancellation.token(), None);
        started_promise.promise.clone().await.unwrap();
        cancellation.cancel();
        let result = pending.await.unwrap();
        {
            let ran = ran.clone();
            finish.complete(ready(move || {
                ran.set(true);
                Promise::resolved(launched(OnboardingTryoutLaunchKind::Executed))
            }));
        }
        finish.promise.clone().await.unwrap();

        assert_eq!((result, disposed.get(), ran.get()), (OnboardingTryoutResult::Cancelled, true, false));
    });
}

#[test]
fn service_disposal_cancels_a_pending_preparation() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let started_promise = Rc::new(DeferredPromise::<()>::new());
        let finish = Rc::new(DeferredPromise::<OnboardingTryoutPreparation>::new());
        register_tryout(&guard, |_| {});
        {
            let (started_promise, finish) = (started_promise.clone(), finish.clone());
            register_presentation(&guard, move |_| {
                started_promise.complete(());
                finish.promise.clone()
            });
        }
        let pending = run(&fixture, "test.tryout");
        started_promise.promise.clone().await.unwrap();
        fixture.service.dispose();
        let result = pending.await.unwrap();
        finish.complete(OnboardingTryoutPreparation::Cancelled);

        assert_eq!(result, OnboardingTryoutResult::Cancelled);
    });
}

#[test]
fn hidden_ai_cannot_prepare_or_route_and_changes_are_announced() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, false, None);
        let notifications = Rc::new(Cell::new(0));
        let routed = Rc::new(Cell::new(false));
        register_tryout(&guard, |tryout| {
            tryout.metadata.is_ai = Some(true);
            tryout.metadata.target_window = Some(OnboardingTryoutTargetWindow::Agents);
        });
        {
            let routed = routed.clone();
            register_window_opener(&guard, move |_| routed.set(true));
        }
        {
            let notifications = notifications.clone();
            guard.add(fixture.service.on_did_change().listen(move |_| notifications.set(notifications.get() + 1)));
        }
        fixture.sentiment.set(ChatSentiment { hidden: Some(true), ..fixture.sentiment.get() });
        fixture.changed.fire(());

        let availability = fixture.service.get_availability("test.tryout");
        let result = run(&fixture, "test.tryout").await.unwrap();
        assert_eq!(
            (availability, matches!(result, OnboardingTryoutResult::Unavailable(_)), routed.get(), notifications.get()),
            (OnboardingTryoutAvailability::Hidden, true, false, 1)
        );
    });
}

#[test]
fn routes_a_known_agents_example_before_evaluating_destination_only_requirements() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, false, None);
        let routed: Rc<RefCell<Vec<String>>> = Rc::default();
        register_tryout(&guard, |tryout| {
            tryout.metadata.target_window = Some(OnboardingTryoutTargetWindow::Agents);
            tryout.when = Some(ContextKeyExpr::has("onlyDefinedInAgents"));
        });
        {
            let routed = routed.clone();
            register_window_opener(&guard, move |id| routed.borrow_mut().push(id.to_string()));
        }

        let result = run(&fixture, "test.tryout").await.unwrap();
        assert_eq!(
            (result, routed.borrow().clone(), fixture.telemetry.names()),
            (OnboardingTryoutResult::Routed, strings(&["test.tryout"]), Vec::new())
        );
    });
}

#[test]
fn offers_setup_without_executing_it_during_availability_checks() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        let action = OnboardingTryoutSetupAction {
            label: "Open setting".to_string(),
            command: OnboardingTryoutCommand { id: "test.openSetting".to_string(), arguments: Some(vec![Dynamic::string("setting.id")]) },
        };
        {
            let action = action.clone();
            register_tryout(&guard, move |tryout| {
                tryout.when = Some(ContextKeyExpr::false_());
                tryout.metadata.unavailable_message = Some("Enable the example.".to_string());
                tryout.metadata.setup = Some(action);
            });
        }
        assert_eq!(
            fixture.service.get_availability("test.tryout"),
            OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable { message: "Enable the example.".to_string(), action: Some(action) })
        );
    });
}

#[test]
fn rejects_unknown_and_non_opted_in_scenarios() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        guard.add(onboarding_scenario_registry().register(OnboardingScenario {
            id: "test.privateTour".to_string(),
            seen_key: None,
            when: None,
            trigger: OnboardingTrigger::Command { command_id: "test.privateTour".to_string() },
            priority: None,
            presentation: OnboardingPresentationRef { kind: "spotlight".to_string(), payload: Dynamic::Undefined },
            tryout: None,
            experiment: None,
            developer_mode_variations: None,
            repeatable: None,
        }));
        assert_rejects(run(&fixture, "test.unknown"), "not available").await;
        assert_rejects(run(&fixture, "test.privateTour"), "not available").await;
        assert!(fixture.telemetry.names().is_empty());
    });
}

#[test]
fn propagates_preparation_and_execution_failures_instead_of_claiming_success() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = create_service(&guard, true, None);
        register_tryout(&guard, |_| {});
        let registration = register_presentation(&guard, |_| Promise::rejected(Error::new("preparation failed")));
        assert_rejects(run(&fixture, "test.tryout"), "preparation failed").await;
        registration.dispose();
        register_presentation(&guard, |_| Promise::resolved(ready(|| Promise::rejected(Error::new("execution failed")))));
        assert_rejects(run(&fixture, "test.tryout"), "execution failed").await;
    });
}

#[test]
fn round_trips_standard_command_links_and_accepts_the_existing_encoded_markdown_shape() {
    let created = create_onboarding_tryout_uri("test.tryout").unwrap();
    let encoded = format!("command:{RUN_ONBOARDING_TRYOUT_COMMAND_ID}?%5B%22test.tryout%22%5D");
    assert_eq!(
        (
            parse_onboarding_tryout_uri(&URI::parse(created.to_string(false), false)).unwrap(),
            parse_onboarding_tryout_uri(&URI::parse(encoded.as_str(), false)).unwrap(),
            parse_onboarding_tryout_uri(&URI::parse("https://code.visualstudio.com/updates", false)).unwrap(),
        ),
        (Some("test.tryout".to_string()), Some("test.tryout".to_string()), None)
    );
}

#[test]
fn only_accepts_one_bounded_id_not_payloads_or_extra_arguments() {
    let empty_object = || Dynamic::object(Vec::<(&str, Dynamic)>::new());
    let cases: Vec<Vec<Dynamic>> = vec![
        vec![],
        vec![Dynamic::string("test.tryout"), empty_object()],
        vec![empty_object()],
        vec![Dynamic::Null],
        vec![Dynamic::Number(1.0)],
        vec![Dynamic::string("")],
        vec![Dynamic::string("../file")],
        vec![Dynamic::string(&"a".repeat(129))],
    ];
    for args in cases {
        let error = parse_onboarding_tryout_arguments(&args).expect_err("os argumentos deveriam ser rejeitados");
        assert!(error.to_string().contains("exactly one"), "mensagem inesperada: {error}");
    }
    let object_uri = format!("command:{RUN_ONBOARDING_TRYOUT_COMMAND_ID}?%7B%22id%22%3A%22test.tryout%22%7D");
    let error = parse_onboarding_tryout_uri(&URI::parse(object_uri.as_str(), false)).expect_err("o objeto deveria ser rejeitado");
    assert!(error.to_string().contains("exactly one"), "mensagem inesperada: {error}");
}
