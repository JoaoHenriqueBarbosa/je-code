//! Porte de `src/vs/workbench/test/common/notificationTelemetry.test.ts` (suíte `Notification telemetry contract`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup` viram o [`Fixture`], criado no começo de cada caso: o guarda de
//!   vazamentos, o `InMemoryStorageService`, o `NotificationService`, o `TestNotificationTelemetryService` e o
//!   `toDisposable` que fecha as notificações que sobraram, registrados na mesma ordem do original.
//! - Os valores que o TypeScript aceita em tempo de execução e o tipo do porte já recusa não são representáveis (o original
//!   os passa com `@ts-expect-error`). No segundo caso, o `telemetry` forjado (`undefined`, texto, objeto com `origin`, objeto
//!   com `id`) é o único valor que o tipo `Option<NotificationTelemetry>` deixa passar como "não confiável", `None`, repetido
//!   as 5 vezes do original; `withNotificationActionTelemetry(action, 'private action metadata')` é a chamada com `None`. No
//!   terceiro, o `{ ...metadata }` (uma cópia que perde a marca da ponte) é `None`: o tipo opaco
//!   `ExtensionNotificationTelemetry` não se copia campo a campo. No sétimo, `{ extensionButtonIndex: Infinity }` é `i64::MAX`,
//!   que o tipo (inteiro) aceita e o limite `0..=1000` do original recusa do mesmo jeito.
//! - `Object.values(NotificationTelemetryId)` é a lista [`ALL_TELEMETRY_IDS`], na ordem de declaração do enum.
//! - Os eventos de telemetria (`Record<string, unknown>`) são lidos como [`Dynamic`]; `deepStrictEqual` de objetos é
//!   `assert_deep_strict_equal` e o `({ instanceId, timeSinceShownMs, ...event })` é [`without`].
//! - O último caso confere em tempo de compilação, no original, que cada campo numérico ou booleano das classificações
//!   (`NotificationShownClassification`, `NotificationInteractionClassification`) é `isMeasurement` e nenhum campo de texto
//!   é. Essas anotações são só tipos do compilador de telemetria e não existem no porte (ver `notification_telemetry.rs`);
//!   o caso confere o que elas descrevem sobre o evento de verdade: os 13 campos da interação (os 8 da exposição mais 5) e
//!   que os valores numéricos e booleanos são exatamente os 5 campos de medida ([`MEASUREMENT_FIELDS`]) e todo o resto é texto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashSet;
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{ActionRef, IAction, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::lifecycle::to_disposable;
use je_base::promise::MaybePromise;
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetry, NotificationTelemetryId, NotificationTelemetryOrigin, NotificationTelemetrySource,
    NotificationTelemetrySourceId, extension_notification_telemetry, get_notification_action_telemetry, get_notification_telemetry_source, with_notification_action_telemetry,
};
use je_platform::notification::{
    BasePromptChoice, NeverShowAgainOptions, Notification, NotificationActions, NotificationHandle, NotificationMessage, NotificationPriority, NotificationProgress as _,
    NotificationProgressProperties, NotificationProperties, NotificationSource, NotificationSourceOrText, PromptChoice, PromptChoiceKind, PromptChoiceWithMenu, PromptOptions,
    Severity, INotificationService,
};
use je_platform::storage::storage::{AbstractStorageService, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_workbench::common::notification_telemetry::{
    NotificationActionRole, NotificationInteraction, NotificationTelemetrySurface, log_notification_action, log_notification_expansion, log_notification_interaction,
    log_notification_shown,
};
use je_workbench::common::notifications::{ChoiceAction, INotificationsModel, NotificationViewItem};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;

/// `Object.values(NotificationTelemetryId)`.
const ALL_TELEMETRY_IDS: [NotificationTelemetryId; 35] = [
    NotificationTelemetryId::AgentHostProgress,
    NotificationTelemetryId::AuthenticationSignIn,
    NotificationTelemetryId::AuthenticationContinue,
    NotificationTelemetryId::ExtensionsActivation,
    NotificationTelemetryId::ExtensionInstall,
    NotificationTelemetryId::ExtensionDownload,
    NotificationTelemetryId::ExtensionDownloadComplete,
    NotificationTelemetryId::ExtensionDownloadError,
    NotificationTelemetryId::ExtensionsDisabled,
    NotificationTelemetryId::ExtensionDependencyLoop,
    NotificationTelemetryId::ExtensionsAutoRestart,
    NotificationTelemetryId::ExtensionHostUnresponsive,
    NotificationTelemetryId::ExtensionHostVersionMismatch,
    NotificationTelemetryId::ExtensionHostCrashRepeated,
    NotificationTelemetryId::RemoteExtensionHostCrashRepeated,
    NotificationTelemetryId::RemoteReconnect,
    NotificationTelemetryId::RemoteAgentHostSshConnect,
    NotificationTelemetryId::RemoteAgentHostSshConnectError,
    NotificationTelemetryId::RemoteAgentHostTunnelConnect,
    NotificationTelemetryId::RemoteAgentHostTunnelConnectError,
    NotificationTelemetryId::RemoteAgentHostTunnelAuthenticationError,
    NotificationTelemetryId::RemoteAgentHostWslConnect,
    NotificationTelemetryId::RemoteAgentHostWslConnectError,
    NotificationTelemetryId::RemoteAgentHostUpdate,
    NotificationTelemetryId::RemoteAgentHostReconnect,
    NotificationTelemetryId::PluginPackageOperation,
    NotificationTelemetryId::PluginUpdate,
    NotificationTelemetryId::PluginRepositoryClone,
    NotificationTelemetryId::PluginRepositoryUpdate,
    NotificationTelemetryId::PluginRepositoryRepair,
    NotificationTelemetryId::DictationModelPrepare,
    NotificationTelemetryId::DictationModelImport,
    NotificationTelemetryId::FileSaveParticipants,
    NotificationTelemetryId::WorkingCopySaveParticipants,
    NotificationTelemetryId::FileOperationParticipants,
];

/// Os campos de medida (`isMeasurement: true`) do evento de interação.
const MEASUREMENT_FIELDS: [&str; 5] = ["instanceId", "hasProgress", "cancellable", "extensionButtonIndex", "timeSinceShownMs"];

/// `setup`.
struct Fixture {
    guard: LeakGuard,
    service: Rc<NotificationService>,
    storage: Rc<AbstractStorageService>,
    telemetry: Rc<TestNotificationTelemetryService>,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = guard.add_rc(InMemoryStorageService::new());
        let service = guard.add_rc(NotificationService::new(Rc::new(storage.clone())));
        let telemetry = Rc::new(TestNotificationTelemetryService::new());
        {
            let service = service.clone();
            guard.add(to_disposable(move || {
                for item in service.model.notifications() {
                    item.close();
                }
            }));
        }
        Self { guard, service, storage, telemetry }
    }

    /// `notify(options)`: o handle e o item que ficou no modelo.
    fn notify(&self, notification: Notification) -> (Rc<dyn NotificationHandle>, Rc<NotificationViewItem>) {
        let handle = self.service.notify(notification);
        (handle, self.service.model.notifications()[0].clone())
    }

    /// `notify()` sem opções: `{ severity: Severity.Info, message: 'Synthetic notification' }`.
    fn notify_default(&self) -> (Rc<dyn NotificationHandle>, Rc<NotificationViewItem>) {
        self.notify(synthetic())
    }
}

fn synthetic() -> Notification {
    Notification::new(Severity::Info, "Synthetic notification")
}

fn properties(telemetry: Option<NotificationTelemetry>) -> NotificationProperties {
    NotificationProperties { telemetry, ..NotificationProperties::default() }
}

/// `toAction({ id, label, run: () => { } })`.
fn action(id: &str, label: &str) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(|_| MaybePromise::Value(Dynamic::Undefined)),
    })
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto, veio {other:?}"),
    }
}

fn number_of(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        other => panic!("esperava número, veio {other:?}"),
    }
}

/// `({ a, b, ...event })`: o evento sem os campos descartados, na mesma ordem.
fn without(event: &Dynamic, dropped: &[&str]) -> Dynamic {
    Dynamic::object(event.own_entries().into_iter().filter(|(key, _)| !dropped.contains(&key.to_string_lossy().as_str())).collect::<Vec<_>>())
}

/// `{ origin, notificationId, extensionId }`.
fn source(origin: NotificationTelemetryOrigin, notification_id: NotificationTelemetrySourceId, extension_id: &str) -> NotificationTelemetrySource {
    NotificationTelemetrySource { origin, notification_id, extension_id: extension_id.to_owned() }
}

fn unknown_source() -> NotificationTelemetrySource {
    source(NotificationTelemetryOrigin::Unknown, NotificationTelemetrySourceId::Unknown, "unknown")
}

fn plain_choice(telemetry_id: NotificationActionTelemetryId, label: &str, is_secondary: bool) -> PromptChoice {
    PromptChoice { base: BasePromptChoice { telemetry_id: Some(telemetry_id), label: label.to_owned(), keep_open: false, run: Rc::new(|| {}) }, is_secondary }
}

// ---- os casos ---------------------------------------------------------------------------------------

#[test]
fn all_built_in_type_ids_are_unique_and_independent_of_content_and_dedup_ids() {
    let fixture = Fixture::new();
    let ids = ALL_TELEMETRY_IDS;
    let sources: Vec<NotificationTelemetrySource> = ids
        .iter()
        .map(|id| {
            let mut notification = synthetic();
            notification.properties = properties(Some(NotificationTelemetry::Id(*id)));
            notification.id = Some("private/dedup".to_owned());
            notification.message = NotificationMessage::from("private message");
            notification.source = Some(NotificationSourceOrText::Text("private source".to_owned()));
            fixture.notify(notification).1.telemetry.clone()
        })
        .collect();

    assert_eq!(ids.iter().map(|id| id.as_str()).collect::<HashSet<_>>().len(), ids.len());
    assert_eq!(
        sources,
        ids.iter().map(|id| source(NotificationTelemetryOrigin::Core, NotificationTelemetrySourceId::Core(*id), "none")).collect::<Vec<_>>()
    );
    assert_eq!(
        [NotificationTelemetryId::AuthenticationSignIn, NotificationTelemetryId::AgentHostProgress, NotificationTelemetryId::PluginRepositoryClone].map(NotificationTelemetryId::as_str),
        ["authentication.signIn", "agentHost.progress", "plugins.repository.clone"]
    );
    assert!(fixture.telemetry.events().is_empty());
}

#[test]
fn unknown_and_forged_metadata_never_expose_message_error_source_dedup_or_action_content() {
    let fixture = Fixture::new();
    for _ in 0..5 {
        let action = action("private action ID", "private button label");
        with_notification_action_telemetry(action.clone(), None);
        let mut notification = synthetic();
        notification.properties = properties(None);
        notification.id = Some("private dedup ID".to_owned());
        notification.message = NotificationMessage::Error(Error::new("private error message"));
        notification.source = Some(NotificationSourceOrText::Source(NotificationSource { id: "private source ID".to_owned(), label: "private display name".to_owned() }));
        notification.actions = Some(NotificationActions { primary: vec![action.clone()], secondary: Vec::new() });
        let (_, item) = fixture.notify(notification);
        log_notification_shown(&*fixture.telemetry, &item, NotificationTelemetrySurface::Toast);
        log_notification_action(&*fixture.telemetry, &item, &action, None);
        item.close();
    }

    let expected = Dynamic::object([
        ("origin", Dynamic::from("unknown")),
        ("notificationId", Dynamic::from("unknown")),
        ("extensionId", Dynamic::from("unknown")),
        ("surface", Dynamic::from("toast")),
        ("severity", Dynamic::from("info")),
        ("hasProgress", Dynamic::Bool(false)),
        ("cancellable", Dynamic::Bool(false)),
        ("interaction", Dynamic::from("primaryAction")),
        ("actionId", Dynamic::from("unknown")),
        ("actionRole", Dynamic::from("primary")),
        ("extensionButtonIndex", Dynamic::from(-1.0)),
    ]);
    let interactions = fixture.telemetry.interactions();
    assert_eq!(interactions.len(), 5);
    for interaction in &interactions {
        assert_deep_strict_equal(&without(interaction, &["instanceId", "timeSinceShownMs"]), &expected);
    }
    let events = Dynamic::array(
        fixture.telemetry.events().into_iter().map(|(event_name, data)| Dynamic::object([("eventName", Dynamic::from(event_name)), ("data", data)])),
    );
    assert!(!json_stringify(&events).expect("os eventos serializam").contains("private"));
}

#[test]
fn extension_attribution_requires_bridge_issued_metadata_and_a_publisher_extension_identifier() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let issued = |extension_id: Option<&str>, kind: &str| NotificationTelemetry::Extension(extension_notification_telemetry(extension_id, kind));
    let long_identifier = format!("publisher.{}", "a".repeat(256));
    let extension = |notification_id: NotificationTelemetrySourceId, extension_id: &str| source(NotificationTelemetryOrigin::Extension, notification_id, extension_id);
    assert_eq!(
        vec![
            get_notification_telemetry_source(Some(&issued(Some("Publisher.Extension"), "message"))),
            get_notification_telemetry_source(None),
            get_notification_telemetry_source(Some(&issued(None, "progress"))),
            get_notification_telemetry_source(Some(&issued(Some("user@example.com"), "message"))),
            get_notification_telemetry_source(Some(&issued(Some("/private/extension"), "progress"))),
            get_notification_telemetry_source(Some(&issued(Some(&long_identifier), "message"))),
        ],
        vec![
            extension(NotificationTelemetrySourceId::ExtensionMessage, "publisher.extension"),
            unknown_source(),
            extension(NotificationTelemetrySourceId::ExtensionProgress, "unknown"),
            extension(NotificationTelemetrySourceId::ExtensionMessage, "unknown"),
            extension(NotificationTelemetrySourceId::ExtensionProgress, "unknown"),
            extension(NotificationTelemetrySourceId::ExtensionMessage, "unknown"),
        ]
    );
}

#[test]
fn prompt_metadata_and_audited_primary_secondary_and_dropdown_choices_survive_handle_updates() {
    let fixture = Fixture::new();
    let choices = vec![
        PromptChoiceKind::WithMenu(PromptChoiceWithMenu {
            base: plain_choice(NotificationActionTelemetryId::Continue, "Continue", false).base,
            menu: vec![plain_choice(NotificationActionTelemetryId::Decline, "Decline", false).base],
        }),
        PromptChoiceKind::Plain(plain_choice(NotificationActionTelemetryId::Reload, "Reload", true)),
    ];
    let handle = fixture.service.prompt(
        Severity::Warning,
        NotificationMessage::from("private prompt"),
        choices,
        Some(PromptOptions { properties: properties(Some(NotificationTelemetry::Id(NotificationTelemetryId::AuthenticationContinue))), on_cancel: None }),
    );
    let item = fixture.service.model.notifications()[0].clone();
    let primary = item.actions().primary[0].clone();
    let secondary = item.actions().secondary[0].clone();
    let choice_action = primary.as_any().downcast_ref::<ChoiceAction>().expect("a ação primária é um ChoiceAction");
    let dropdown = fixture.guard.add_rc(choice_action.menu().expect("o menu")[0].clone());
    let dropdown_action: ActionRef = dropdown.clone();
    handle.update_message(NotificationMessage::from("another private prompt"));
    handle.update_severity(Severity::Error);
    handle.update_actions(Some(NotificationActions { primary: vec![primary.clone()], secondary: vec![secondary.clone()] }));
    item.update_visibility(true);
    log_notification_action(&*fixture.telemetry, &item, &primary, None);
    log_notification_action(&*fixture.telemetry, &item, &secondary, None);
    log_notification_action(&*fixture.telemetry, &item, &dropdown_action, None);

    assert_eq!(item.telemetry, source(NotificationTelemetryOrigin::Core, NotificationTelemetrySourceId::Core(NotificationTelemetryId::AuthenticationContinue), "none"));
    assert!(fixture.telemetry.shown().is_empty());
    let actions: Vec<(String, String, f64, String)> = fixture
        .telemetry
        .interactions()
        .iter()
        .map(|event| (text_of(&event.get_str("actionId")), text_of(&event.get_str("actionRole")), number_of(&event.get_str("timeSinceShownMs")), text_of(&event.get_str("surface"))))
        .collect();
    let expected = |action_id: &str, role: &str| (action_id.to_owned(), role.to_owned(), -1.0, "unknown".to_owned());
    assert_eq!(actions, vec![expected("continue", "primary"), expected("reload", "secondary"), expected("decline", "primary")]);
}

#[test]
fn deduplication_and_re_rendering_retain_correlation_but_closing_and_independent_progress_do_not() {
    let fixture = Fixture::new();
    let options = || {
        let mut notification = Notification::new(Severity::Info, "Installing");
        notification.properties = properties(Some(NotificationTelemetry::Id(NotificationTelemetryId::ExtensionInstall)));
        notification.id = Some("private dedup".to_owned());
        notification
    };
    let with_progress = || Notification { progress: Some(NotificationProgressProperties { infinite: true, ..NotificationProgressProperties::default() }), ..options() };
    let toast = NotificationTelemetrySurface::Toast;
    let telemetry = &*fixture.telemetry;

    let first = fixture.notify(options()).1;
    log_notification_shown(telemetry, &first, toast);
    first.update_message(NotificationMessage::from("Installing update"));
    first.update_severity(Severity::Warning);
    log_notification_shown(telemetry, &first, toast);
    let replacement = fixture.notify(options()).1;
    log_notification_shown(telemetry, &replacement, toast);
    log_notification_shown(telemetry, &replacement, NotificationTelemetrySurface::Center);
    log_notification_shown(telemetry, &replacement, toast);
    log_notification_interaction(telemetry, &replacement, NotificationInteraction::Dismiss, None, None, NotificationActionRole::None);
    replacement.close();
    let next = fixture.notify(options()).1;
    log_notification_shown(telemetry, &next, toast);
    let progress1 = fixture.notify(with_progress()).1;
    let progress2 = fixture.notify(with_progress()).1;
    log_notification_shown(telemetry, &progress1, toast);
    log_notification_shown(telemetry, &progress2, toast);

    let shown = telemetry.shown();
    let interactions = telemetry.interactions();
    let instance_ids: Vec<f64> = shown.iter().map(|event| number_of(&event.get_str("instanceId"))).collect();
    assert_eq!(shown.iter().map(|event| text_of(&event.get_str("surface"))).collect::<Vec<_>>(), ["toast", "center", "toast", "toast", "toast"]);
    assert_eq!(instance_ids.iter().map(|id| id.to_bits()).collect::<HashSet<_>>().len(), 4);
    assert!(instance_ids[0] == instance_ids[1] && instance_ids[0] == number_of(&interactions[0].get_str("instanceId")));
    assert!(number_of(&interactions[0].get_str("timeSinceShownMs")) >= 0.0);
    assert_eq!(fixture.service.model.notifications().len(), 3);
}

#[test]
fn a_duplicate_with_different_safe_attribution_does_not_inherit_exposure() {
    let fixture = Fixture::new();
    let with_telemetry = |id: NotificationTelemetryId| {
        let mut notification = synthetic();
        notification.id = Some("dedup".to_owned());
        notification.properties = properties(Some(NotificationTelemetry::Id(id)));
        notification
    };
    let first = fixture.notify(with_telemetry(NotificationTelemetryId::ExtensionInstall)).1;
    log_notification_shown(&*fixture.telemetry, &first, NotificationTelemetrySurface::Toast);
    let second = fixture.notify(with_telemetry(NotificationTelemetryId::ExtensionDownload)).1;
    log_notification_shown(&*fixture.telemetry, &second, NotificationTelemetrySurface::Toast);

    let shown = fixture.telemetry.shown();
    assert_eq!(fixture.service.model.notifications().len(), 1);
    assert_eq!(shown.iter().map(|event| text_of(&event.get_str("notificationId"))).collect::<Vec<_>>(), ["extensions.install", "extensions.downloadVsix"]);
    assert!(number_of(&shown[0].get_str("instanceId")) != number_of(&shown[1].get_str("instanceId")));
}

#[test]
fn creation_suppression_visibility_flags_automatic_close_and_programmatic_expansion_are_not_interactions_or_exposures() {
    let fixture = Fixture::new();
    fixture.storage.store("neverAgain", StorageValue::Bool(true), StorageScope::Application, StorageTarget::User);
    let never_show_again = || NeverShowAgainOptions { id: "neverAgain".to_owned(), is_secondary: false, scope: None };
    let suppressed_properties = NotificationProperties {
        telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::ExtensionsDisabled)),
        never_show_again: Some(never_show_again()),
        ..NotificationProperties::default()
    };
    fixture.service.notify(Notification { properties: suppressed_properties.clone(), ..Notification::new(Severity::Info, "Suppressed") });
    fixture.service.prompt(Severity::Info, NotificationMessage::from("Suppressed prompt"), Vec::new(), Some(PromptOptions { properties: suppressed_properties, on_cancel: None }));
    let mut silent = synthetic();
    silent.properties.priority = Some(NotificationPriority::Silent);
    let (handle, item) = fixture.notify(silent);
    item.update_visibility(true);
    item.expand();
    item.collapse(true);
    item.toggle();
    handle.progress().infinite();
    handle.progress().done();
    handle.close();

    assert!(fixture.telemetry.events().is_empty());
    assert_eq!(fixture.service.model.notifications().len(), 0);
}

#[test]
fn only_an_explicitly_annotated_cancel_action_is_cancellation_and_invalid_indexes_are_omitted() {
    let fixture = Fixture::new();
    let fake_cancel = action("progress.cancel", "Cancel");
    let cancel = with_notification_action_telemetry(action("arbitrary", "Skip"), Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::ProgressCancel)));
    let invalid_index = with_notification_action_telemetry(action("extension", "Private"), Some(NotificationActionTelemetry::ExtensionButtonIndex(i64::MAX)));
    let mut notification = synthetic();
    notification.properties = properties(Some(NotificationTelemetry::Extension(extension_notification_telemetry(Some("publisher.extension"), "progress"))));
    notification.progress = Some(NotificationProgressProperties { infinite: true, ..NotificationProgressProperties::default() });
    notification.actions = Some(NotificationActions { primary: vec![fake_cancel.clone(), cancel.clone(), invalid_index.clone()], secondary: Vec::new() });
    let (_, item) = fixture.notify(notification);
    log_notification_action(&*fixture.telemetry, &item, &fake_cancel, None);
    log_notification_action(&*fixture.telemetry, &item, &cancel, None);
    log_notification_action(&*fixture.telemetry, &item, &invalid_index, None);
    cancel.set_enabled(false);
    log_notification_action(&*fixture.telemetry, &item, &cancel, None);

    let interactions: Vec<(String, String, f64, bool)> = fixture
        .telemetry
        .interactions()
        .iter()
        .map(|event| {
            (
                text_of(&event.get_str("interaction")),
                text_of(&event.get_str("actionId")),
                number_of(&event.get_str("extensionButtonIndex")),
                matches!(event.get_str("cancellable"), Dynamic::Bool(true)),
            )
        })
        .collect();
    let expected = |interaction: &str, action_id: &str| (interaction.to_owned(), action_id.to_owned(), -1.0, true);
    assert_eq!(interactions, vec![expected("primaryAction", "unknown"), expected("progressCancel", "progress.cancel"), expected("primaryAction", "unknown")]);
    assert_eq!(get_notification_action_telemetry(&invalid_index), None);
}

#[test]
fn expansion_is_recorded_only_for_a_user_requested_state_change_that_can_occur() {
    let fixture = Fixture::new();
    let (_, item) = fixture.notify_default();
    let telemetry = &*fixture.telemetry;
    log_notification_expansion(telemetry, &item, true, None);
    item.expand();
    log_notification_expansion(telemetry, &item, true, None);
    log_notification_expansion(telemetry, &item, false, None);
    item.collapse(false);
    log_notification_expansion(telemetry, &item, false, None);
    item.update_actions(Some(NotificationActions { primary: vec![action("action", "Action")], secondary: Vec::new() }));
    log_notification_expansion(telemetry, &item, false, None);

    assert_eq!(telemetry.interactions().iter().map(|event| text_of(&event.get_str("interaction"))).collect::<Vec<_>>(), ["expand", "collapse"]);
}

#[test]
fn gdpr_classifications_mark_every_numeric_and_boolean_field_as_a_measurement_and_no_string_field() {
    let fixture = Fixture::new();
    let (_, item) = fixture.notify_default();
    log_notification_shown(&*fixture.telemetry, &item, NotificationTelemetrySurface::Toast);
    let primary = action("action", "Action");
    item.update_actions(Some(NotificationActions { primary: vec![primary.clone()], secondary: Vec::new() }));
    log_notification_action(&*fixture.telemetry, &item, &primary, Some(NotificationTelemetrySurface::Toast));

    let shown = fixture.telemetry.shown().remove(0);
    let interaction = fixture.telemetry.interactions().remove(0);
    // As chaves da exposição estão todas na interação (`...shown`), e a interação acrescenta 5.
    assert_eq!(shown.own_keys().len(), 8);
    assert!(shown.own_keys().iter().all(|key| interaction.has_property(key)));

    // `CheckMeasurements`: número ou booleano é medida, e só ele; texto nunca é medida.
    let checks: Vec<Dynamic> = interaction
        .own_entries()
        .into_iter()
        .map(|(key, value)| {
            let is_measurement_field = MEASUREMENT_FIELDS.contains(&key.to_string_lossy().as_str());
            let is_numeric_or_boolean = matches!(value, Dynamic::Number(_) | Dynamic::Bool(_));
            Dynamic::Bool(is_measurement_field == is_numeric_or_boolean)
        })
        .collect();
    assert_deep_strict_equal(&Dynamic::array(checks), &Dynamic::array((0..13).map(|_| Dynamic::Bool(true))));
}
