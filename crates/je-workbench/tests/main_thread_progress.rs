//! Porte de `src/vs/workbench/api/test/browser/mainThreadProgress.test.ts` (suíte `MainThreadProgress`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o
//!   `DisposableStore` dele; os casos `async` rodam em `run_with_faked_timers`.
//! - `keeps extension progress messages serializable...` confere por tipo (`IProgressOptionsDto['title'] extends string`,
//!   `'telemetry' extends keyof IProgressOptionsDto`): em Rust o equivalente é o desestruturamento exaustivo do DTO (um
//!   campo de capacidade a mais não compila) e a atribuição de `title` e `message` a `Option<String>`.
//! - `new class extends mock<X>() { override ... }` vira dublê com o que o caso sobrescreve ([`RecordingProgressService`]);
//!   o `mock<ExtHostProgressShape>()` e o `mock<ICommandService>()` que ninguém chama são [`NullExtHostProgress`] e
//!   `MockCommandService` (falham se forem chamados). `mock<IExtensionDescription>()` com `override` é a
//!   `null_extension_description()` com os campos trocados.
//! - O `publicOptions` de `extension-host attribution...` leva um `telemetryId` extra que o host ignora: o
//!   `ProgressOptions` da API não tem esse campo, então o objeto extra não é representável e o caso passa só o que o tipo
//!   aceita (o que o original prova ignorado já é garantido pelo tipo).
//! - `for (const location of [...])` gera um `#[test]` por local (`..._15` é `Notification`, `..._10` é `Window`).
//! - `await promise` e `assert.deepStrictEqual` sobre objetos viram `assert_eq!` sobre tuplas/structs com as mesmas
//!   propriedades.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::promise::Promise;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::notification::notification::{NotificationSource, NotificationSourceOrText};
use je_platform::notification::notification_legacy::is_legacy_extension_link_parsing;
use je_platform::notification::notification_message::NotificationPart;
use je_platform::notification::notification_telemetry::{
    get_notification_telemetry_source, NotificationTelemetryId, NotificationTelemetryOrigin, NotificationTelemetrySource, NotificationTelemetrySourceId,
};
use je_platform::progress::progress::{IProgress, IProgressService, ProgressLocation, ProgressLocationId, ProgressOptions, ProgressStep, ProgressTask};
use je_workbench::api::browser::main_thread_progress::MainThreadProgress;
use je_workbench::api::common::ext_host_progress::{ExtHostProgress, ProgressOptions as ApiProgressOptions};
use je_workbench::api::common::ext_host_protocol::{ExtHostProgressShape, IProgressOptionsDto, IProgressStepDto, MainThreadProgressShape};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::mock_command_service::MockCommandService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `new class extends mock<ExtHostProgressShape>() { }`.
struct NullExtHostProgress;

impl ExtHostProgressShape for NullExtHostProgress {
    fn rpc_accept_progress_canceled(&self, _handle: u32) {
        panic!("$acceptProgressCanceled is not a function")
    }
}

/// O `{ report: step => reports.push(step) }` que os dublês entregam à tarefa.
struct CollectingProgress(Rc<RefCell<Vec<ProgressStep>>>);

impl IProgress<ProgressStep> for CollectingProgress {
    fn report(&self, item: ProgressStep) {
        self.0.borrow_mut().push(item);
    }
}

/// `new class extends mock<IProgressService>() { override withProgress(value, task) { options.push(value); return task(...); } }`.
struct RecordingProgressService {
    options: Rc<RefCell<Vec<ProgressOptions>>>,
    reports: Rc<RefCell<Vec<ProgressStep>>>,
}

impl IProgressService for RecordingProgressService {
    fn with_progress(&self, value: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        self.options.borrow_mut().push(value);
        task(Rc::new(CollectingProgress(self.reports.clone())))
    }
}

/// Os dublês de um caso: o serviço de progresso que grava e o `MainThreadProgress` sobre ele.
struct Fixture {
    options: Rc<RefCell<Vec<ProgressOptions>>>,
    reports: Rc<RefCell<Vec<ProgressStep>>>,
    main_thread: Rc<MainThreadProgress>,
}

fn create_main_thread_progress(store: &Rc<je_base::lifecycle::DisposableStore>) -> Fixture {
    let options: Rc<RefCell<Vec<ProgressOptions>>> = Rc::default();
    let reports: Rc<RefCell<Vec<ProgressStep>>> = Rc::default();
    let proxy: Rc<dyn ExtHostProgressShape> = Rc::new(NullExtHostProgress);
    let main_thread = store.add_rc(MainThreadProgress::new(
        &*single_proxy_rpc_protocol(proxy),
        Rc::new(RecordingProgressService { options: options.clone(), reports: reports.clone() }),
        Rc::new(MockCommandService::default()),
    ));
    Fixture { options, reports, main_thread }
}

/// `{ origin, notificationId, extensionId }`.
fn source(origin: NotificationTelemetryOrigin, notification_id: NotificationTelemetrySourceId, extension_id: &str) -> NotificationTelemetrySource {
    NotificationTelemetrySource { origin, notification_id, extension_id: extension_id.to_owned() }
}

/// `typeof x === 'string' | undefined`: só compila para `Option<String>`.
fn is_optional_string(_value: &Option<String>) -> bool {
    true
}

#[test]
fn keeps_extension_progress_messages_serializable_and_the_capability_local_to_the_bridge() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // O desestruturamento exaustivo falha na compilação se o DTO ganhar `legacy_extension_link_parsing` ou `telemetry`.
    let dto = IProgressOptionsDto { title: Some("title".to_owned()), ..IProgressOptionsDto::new(ProgressLocation::Notification) };
    let IProgressOptionsDto { location: _, title, telemetry_id: _, source: _, total: _, cancellable: _, buttons: _ } = dto;
    let step = IProgressStepDto { message: Some("message".to_owned()), ..IProgressStepDto::default() };
    let IProgressStepDto { message, increment: _, total: _ } = step;

    struct Contract {
        title_is_string: bool,
        message_is_string: bool,
        exposes_capability: bool,
        exposes_telemetry_capability: bool,
    }
    let contract = Contract {
        title_is_string: is_optional_string(&title),
        message_is_string: is_optional_string(&message),
        exposes_capability: false,
        exposes_telemetry_capability: false,
    };
    assert_eq!(
        (contract.title_is_string, contract.message_is_string, contract.exposes_capability, contract.exposes_telemetry_capability),
        (true, true, false, false)
    );
}

#[test]
fn extension_host_attribution_ignores_public_extra_options_and_arbitrary_display_sources() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let fixture = create_main_thread_progress(&store);
        let proxy: Rc<dyn MainThreadProgressShape> = fixture.main_thread.clone();
        let ext_host = ExtHostProgress::new(&*single_proxy_rpc_protocol(proxy));
        let notification_options = |title: Option<&str>| ApiProgressOptions {
            location: types::ProgressLocation::Notification.into(),
            title: title.map(str::to_owned),
            cancellable: None,
        };
        for under_development in [false, true] {
            let mut extension = null_extension_description();
            extension.identifier = ExtensionIdentifier::new("Publisher.Extension");
            extension.manifest.name = "private display name".to_owned();
            extension.is_under_development = under_development;
            ext_host
                .with_progress(&extension, notification_options(Some("private title")), Box::new(|_progress, _token| Promise::resolved(())))
                .await
                .expect("withProgress");
        }
        ext_host
            .with_progress_from_source(
                NotificationSourceOrText::Source(NotificationSource { id: "private server URL".to_owned(), label: "private server".to_owned() }),
                notification_options(None),
                Box::new(|_progress, _token| Promise::resolved(())),
                Some(NotificationTelemetryId::AuthenticationSignIn),
            )
            .await
            .expect("withProgressFromSource com telemetria");
        ext_host
            .with_progress_from_source(
                NotificationSourceOrText::Source(NotificationSource { id: "publisher.extension".to_owned(), label: "private label".to_owned() }),
                notification_options(None),
                Box::new(|_progress, _token| Promise::resolved(())),
                None,
            )
            .await
            .expect("withProgressFromSource");
        let telemetry: Vec<NotificationTelemetrySource> =
            fixture.options.borrow().iter().map(|value| get_notification_telemetry_source(value.telemetry.as_ref())).collect();
        assert_eq!(
            telemetry,
            vec![
                source(NotificationTelemetryOrigin::Extension, NotificationTelemetrySourceId::ExtensionProgress, "publisher.extension"),
                source(NotificationTelemetryOrigin::Extension, NotificationTelemetrySourceId::ExtensionProgress, "unknown"),
                source(NotificationTelemetryOrigin::Core, NotificationTelemetrySourceId::Core(NotificationTelemetryId::AuthenticationSignIn), "none"),
                source(NotificationTelemetryOrigin::Extension, NotificationTelemetrySourceId::ExtensionProgress, "unknown"),
            ]
        );
    });
}

/// O corpo de `preserves legacy extension links at progress location ${location}`.
fn preserves_legacy_extension_links_at_progress_location(location: ProgressLocation) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let Fixture { options, reports, main_thread: service } = create_main_thread_progress(&store);
        let title = "[Show Logs](command:python.viewOutput)";
        let message = "[Check details](command:java.show.server.task.status)";

        let dto = IProgressOptionsDto { title: Some(title.to_owned()), ..IProgressOptionsDto::new(location) };
        let promise = service.rpc_start_progress(1, dto, Some("test.extension"));
        service.rpc_progress_report(1, IProgressStepDto { message: Some(message.to_owned()), ..IProgressStepDto::default() });
        service.rpc_progress_end(1);
        promise.await.expect("$startProgress");

        let observed: Vec<(ProgressLocationId, Option<NotificationPart>, bool, NotificationTelemetrySource)> = options
            .borrow()
            .iter()
            .map(|value| {
                (
                    value.location.clone(),
                    value.title.clone(),
                    is_legacy_extension_link_parsing(value.legacy_extension_link_parsing.as_ref()),
                    get_notification_telemetry_source(value.telemetry.as_ref()),
                )
            })
            .collect();
        let expected_options = vec![(
            ProgressLocationId::Location(location),
            Some(NotificationPart::Text(title.to_owned())),
            true,
            source(NotificationTelemetryOrigin::Extension, NotificationTelemetrySourceId::ExtensionProgress, "test.extension"),
        )];
        let expected_reports = vec![ProgressStep { message: Some(NotificationPart::Text(message.to_owned())), ..ProgressStep::default() }];
        assert_eq!((observed, reports.borrow().clone()), (expected_options, expected_reports));
    });
}

#[test]
fn preserves_legacy_extension_links_at_progress_location_15() {
    preserves_legacy_extension_links_at_progress_location(ProgressLocation::Notification);
}

#[test]
fn preserves_legacy_extension_links_at_progress_location_10() {
    preserves_legacy_extension_links_at_progress_location(ProgressLocation::Window);
}
