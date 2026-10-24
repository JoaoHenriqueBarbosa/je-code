//! Porte de `src/vs/workbench/services/progress/test/browser/progressService.test.ts` (suíte `ProgressService notification messages`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Todo caso roda sob o relógio virtual (`runWithFakedTimers({}, ...)`), no `async_test`, que também cria o guarda de
//!   vazamentos (`ensureNoDisposablesAreLeakedInTestSuite()`) e o `store`.
//! - `createServices()` é [`create_services`]: o `NotificationService` real sobre um `InMemoryStorageService`, o `toDisposable`
//!   que fecha as notificações que sobraram, e o `ProgressService` com os dublês dos colaboradores (`new class extends
//!   mock<...>() { }`): os [`MockActivity`], [`MockPaneComposite`], [`MockViews`], [`MockViewDescriptors`] e [`MockDialogs`] não
//!   implementam nenhuma operação (chamar uma é falha, como o `mock` do original); o [`RecordingStatusbar`] guarda cada entrada
//!   (`addEntry` e `update`), e o [`ActiveUserActivity`] devolve `Disposable.None` em `markActive`. `ILayoutService`,
//!   `IKeybindingService` e `IHostService` só servem para montar o `Dialog` e são o [`MockDialogs`] (veja
//!   `progress_service.rs`).
//! - `for (const cancel of [false, true])` gera os dois `#[test]`, na ordem do original: `automatic_completion...` e
//!   `explicit_cancellation...`.
//! - `assert.deepStrictEqual({ ... }, { ... })` entre objetos é a comparação das tuplas, na ordem dos campos.
//! - `notification.message.linkedText.nodes` é `item.message().linked_text.nodes`; os nós são comparados como
//!   `LinkedTextNode` (um link sem `title`).

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::async_test;
use je_base::JsString;
use je_base::actions::IActionRunner;
use je_base::async_::timeout;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_base::linked_text::{Link, LinkedTextNode};
use je_base::promise::{DeferredPromise, Promise};
use je_editor::code_action::platform::IProgressIndicator;
use je_platform::notification::notification_legacy::LEGACY_EXTENSION_LINK_PARSING;
use je_platform::notification::notification_message::NotificationText;
use je_platform::notification::notification_telemetry::{
    NotificationTelemetry, NotificationTelemetryId, NotificationTelemetryOrigin, NotificationTelemetrySource, NotificationTelemetrySourceId, extension_notification_telemetry,
};
use je_platform::notification::INotificationService;
use je_platform::progress::progress::{IProgress, ProgressCancellable, ProgressLocation, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::storage::storage::{InMemoryStorageService, StorageService};
use je_workbench::browser::parts::notifications::notifications_commands::{NotificationActionRunner, notification_view_item_context};
use je_workbench::common::notification_telemetry::{NotificationTelemetrySurface, log_notification_shown};
use je_workbench::common::notifications::INotificationsModel;
use je_workbench::common::views::{ViewContainerLocation, ViewContainerRc, ViewDescriptorRc};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::services::progress::progress_service::{
    IProgressActivityService, IProgressDialog, IProgressDialogFactory, IProgressPaneCompositeService, IProgressStatusbarService, IProgressViewDescriptorService, IProgressViewsService,
    IStatusbarEntryAccessor, ProgressDialogOptions, ProgressService, ProgressServiceParts, StatusbarProgressEntry,
};
use je_workbench::services::statusbar::statusbar::StatusbarAlignment;
use je_workbench::services::user_activity::user_activity_service::{IUserActivityService, MarkActiveOptions};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;

/// `new class extends mock<IActivityService>() { }`.
struct MockActivity;

impl IProgressActivityService for MockActivity {
    fn show_view_container_activity(&self, _view_container_id: &str) -> DisposableRef {
        panic!("IActivityService.showViewContainerActivity não implementado (mock)");
    }
}

/// `new class extends mock<IPaneCompositePartService>() { }`.
struct MockPaneComposite;

impl IProgressPaneCompositeService for MockPaneComposite {
    fn get_progress_indicator(&self, _id: &str, _view_container_location: ViewContainerLocation) -> Option<Rc<dyn IProgressIndicator>> {
        panic!("IPaneCompositePartService.getProgressIndicator não implementado (mock)");
    }
}

/// `new class extends mock<IViewsService>() { }`.
struct MockViews;

impl IProgressViewsService for MockViews {
    fn get_view_progress_indicator(&self, _view_id: &str) -> Option<Rc<dyn IProgressIndicator>> {
        panic!("IViewsService.getViewProgressIndicator não implementado (mock)");
    }
}

/// `new class extends mock<IViewDescriptorService>() { }`.
struct MockViewDescriptors;

impl IProgressViewDescriptorService for MockViewDescriptors {
    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        panic!("IViewDescriptorService.getViewContainerById não implementado (mock)");
    }

    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        panic!("IViewDescriptorService.getViewContainerLocation não implementado (mock)");
    }

    fn get_view_descriptor_by_id(&self, _id: &str) -> Option<ViewDescriptorRc> {
        panic!("IViewDescriptorService.getViewDescriptorById não implementado (mock)");
    }

    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        panic!("IViewDescriptorService.getViewContainerByViewId não implementado (mock)");
    }
}

/// `new class extends mock<ILayoutService>() { }`, `mock<IKeybindingService>()` e `mock<IHostService>()`.
struct MockDialogs;

impl IProgressDialogFactory for MockDialogs {
    fn create_dialog(&self, _message: &str, _buttons: Vec<String>, _options: ProgressDialogOptions) -> Rc<dyn IProgressDialog> {
        panic!("Dialog não implementado (mock)");
    }
}

/// `new class extends mock<IStatusbarEntryAccessor>() { override update(value) { statusEntries.push(value); } override dispose() { } }`.
struct RecordedAccessor {
    status_entries: Rc<RefCell<Vec<StatusbarProgressEntry>>>,
}

impl IStatusbarEntryAccessor for RecordedAccessor {
    fn update(&self, entry: StatusbarProgressEntry) {
        self.status_entries.borrow_mut().push(entry);
    }
}

impl IDisposable for RecordedAccessor {
    fn dispose(&self) {}
}

/// `new class extends mock<IStatusbarService>() { override addEntry(entry) { statusEntries.push(entry); return ...; } }`.
struct RecordingStatusbar {
    status_entries: Rc<RefCell<Vec<StatusbarProgressEntry>>>,
}

impl IProgressStatusbarService for RecordingStatusbar {
    fn add_entry(&self, entry: StatusbarProgressEntry, _id: &str, _alignment: StatusbarAlignment, _priority: f64) -> Rc<dyn IStatusbarEntryAccessor> {
        self.status_entries.borrow_mut().push(entry);
        Rc::new(RecordedAccessor { status_entries: self.status_entries.clone() })
    }
}

/// `new class extends mock<IUserActivityService>() { override markActive() { return Disposable.None; } }`.
struct ActiveUserActivity;

impl IUserActivityService for ActiveUserActivity {
    fn is_active(&self) -> bool {
        panic!("IUserActivityService.isActive não implementado (mock)");
    }

    fn on_did_change_is_active(&self) -> Event<bool> {
        panic!("IUserActivityService.onDidChangeIsActive não implementado (mock)");
    }

    fn mark_active(&self, _opts: Option<MarkActiveOptions>) -> DisposableRef {
        none()
    }
}

/// O que `createServices()` devolve.
struct Services {
    notifications: Rc<NotificationService>,
    progress_service: Rc<ProgressService>,
    status_entries: Rc<RefCell<Vec<StatusbarProgressEntry>>>,
    telemetry: Rc<TestNotificationTelemetryService>,
    action_runner: Rc<NotificationActionRunner>,
}

impl Services {
    /// `notifications.model.notifications[index]`.
    fn notification(&self, index: usize) -> Rc<je_workbench::common::notifications::NotificationViewItem> {
        self.notifications.model.notifications()[index].clone()
    }

    /// `notifications.model.notifications.length`.
    fn remaining(&self) -> usize {
        self.notifications.model.notifications().len()
    }
}

fn create_services(store: &Rc<DisposableStore>) -> Services {
    let telemetry = Rc::new(TestNotificationTelemetryService::new());
    let storage = store.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let notifications = store.add_rc(NotificationService::new(storage_service));
    {
        let notifications = notifications.clone();
        store.add(to_disposable(move || {
            for notification in notifications.model.notifications() {
                notification.close();
            }
        }));
    }
    let status_entries = Rc::new(RefCell::new(Vec::new()));
    let notification_service: Rc<dyn INotificationService> = notifications.clone();
    let progress_service = store.add_rc(ProgressService::new(ProgressServiceParts {
        activity_service: Rc::new(MockActivity),
        pane_composite_service: Rc::new(MockPaneComposite),
        view_descriptor_service: Rc::new(MockViewDescriptors),
        views_service: Rc::new(MockViews),
        notification_service: notification_service.clone(),
        statusbar_service: Rc::new(RecordingStatusbar { status_entries: status_entries.clone() }),
        dialog_factory: Rc::new(MockDialogs),
        user_activity_service: Rc::new(ActiveUserActivity),
    }));
    let action_runner = store.add_rc(NotificationActionRunner::new(None, telemetry.clone(), notification_service));
    Services { notifications, progress_service, status_entries, telemetry, action_runner }
}

/// `() => deferred.p` e `() => timeout(n)`: a tarefa que devolve a promessa dada.
fn task_returning(promise: Promise<()>) -> ProgressTask {
    Box::new(move |_| promise)
}

/// A tarefa que guarda o `IProgress` recebido (`reporter = progress`) e devolve a promessa dada.
fn task_capturing(slot: &Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>>, promise: Promise<()>) -> ProgressTask {
    let slot = slot.clone();
    Box::new(move |progress| {
        *slot.borrow_mut() = Some(progress);
        promise
    })
}

/// `progress!.report({ message })`.
fn report(slot: &Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>>, step: ProgressStep) {
    let progress = slot.borrow().clone().expect("o progresso foi recebido");
    progress.report(step);
}

fn text_node(text: &str) -> LinkedTextNode {
    LinkedTextNode::Text(JsString::from(text))
}

fn link_node(label: &str, href: &str) -> LinkedTextNode {
    LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: None })
}

/// `event.<field>` de um evento de telemetria.
fn field(event: &Dynamic, name: &str) -> Dynamic {
    event.get_str(name)
}

/// `for (const cancel of [false, true])`.
fn attributed_progress(cancel: bool) {
    async_test(|store| async move {
        let services = create_services(&store);
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let cancellations = Rc::new(Cell::new(0));
        let (cancelled, completer) = (cancellations.clone(), deferred.clone());
        let promise = services.progress_service.with_progress(
            ProgressOptions {
                telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::AuthenticationSignIn)),
                title: Some("private server name".into()),
                cancellable: Some(ProgressCancellable::Label("Private cancellation label".to_owned())),
                ..ProgressOptions::new(ProgressLocation::Notification)
            },
            task_returning(deferred.promise.clone()),
            Some(Rc::new(move |_| {
                cancelled.set(cancelled.get() + 1);
                drop(completer.complete(()));
            })),
        );
        let item = services.notification(0);
        let created = services.telemetry.events();
        log_notification_shown(&*services.telemetry, &item, NotificationTelemetrySurface::Toast);
        if cancel {
            services.action_runner.run(&item.actions().primary[0], Some(notification_view_item_context(&item))).await.unwrap();
        } else {
            deferred.complete(()).await.unwrap();
        }
        promise.await.unwrap();
        timeout(1000).await.unwrap();

        let flags: Vec<(Dynamic, Dynamic)> = services.telemetry.shown().iter().map(|event| (field(event, "hasProgress"), field(event, "cancellable"))).collect();
        let interactions: Vec<(String, String, String)> = services
            .telemetry
            .interactions()
            .iter()
            .map(|event| {
                let text = |name: &str| match field(event, name) {
                    Dynamic::String(value) => value.to_string_lossy(),
                    other => panic!("esperava texto em {name}, veio {other:?}"),
                };
                (text("interaction"), text("actionId"), text("actionRole"))
            })
            .collect();
        let private_payload =
            json_stringify(&Dynamic::array(services.telemetry.shown().into_iter().chain(services.telemetry.interactions()))).is_some_and(|payload| payload.contains("private"));
        assert_eq!(created.len(), 0);
        assert_eq!(
            item.telemetry,
            NotificationTelemetrySource {
                origin: NotificationTelemetryOrigin::Core,
                notification_id: NotificationTelemetrySourceId::Core(NotificationTelemetryId::AuthenticationSignIn),
                extension_id: "none".to_owned(),
            }
        );
        assert_eq!(flags, vec![(Dynamic::Bool(true), Dynamic::Bool(true))]);
        assert_eq!(
            interactions,
            if cancel { vec![("progressCancel".to_owned(), "progress.cancel".to_owned(), "primary".to_owned())] } else { Vec::new() }
        );
        assert_eq!(cancellations.get(), if cancel { 1 } else { 0 });
        assert_eq!(services.remaining(), 0);
        assert!(!private_payload);
    });
}

#[test]
fn automatic_completion_of_attributed_progress() {
    attributed_progress(false);
}

#[test]
fn explicit_cancellation_of_attributed_progress() {
    attributed_progress(true);
}

#[test]
fn delayed_progress_that_completes_before_its_delay_never_creates_a_notification() {
    async_test(|store| async move {
        let services = create_services(&store);
        let options = ProgressOptions {
            telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::PluginRepositoryClone)),
            title: Some("Private repository".into()),
            delay: Some(500.0),
            cancellable: Some(ProgressCancellable::Flag(true)),
            ..ProgressOptions::new(ProgressLocation::Notification)
        };
        let promise = services.progress_service.with_progress(options, task_returning(timeout(100).promise), None);
        promise.await.unwrap();
        timeout(500).await.unwrap();
        assert_eq!((services.remaining(), services.telemetry.events().len()), (0, 0));
    });
}

#[test]
fn delayed_extension_progress_preserves_attribution_through_updates_and_window_fallback() {
    async_test(|store| async move {
        let services = create_services(&store);
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let reporter: Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>> = Rc::new(RefCell::new(None));
        let promise = services.progress_service.with_progress(
            ProgressOptions {
                telemetry: Some(NotificationTelemetry::Extension(extension_notification_telemetry(Some("Publisher.Extension"), "progress"))),
                title: Some("private title".into()),
                cancellable: Some(ProgressCancellable::Flag(true)),
                ..ProgressOptions::new(ProgressLocation::Window)
            },
            task_capturing(&reporter, deferred.promise.clone()),
            None,
        );
        timeout(100).await.unwrap();
        let before_delay = services.remaining();
        report(&reporter, ProgressStep { message: Some("private report".into()), increment: Some(20.0), total: None });
        timeout(100).await.unwrap();
        let item = services.notification(0);
        let before_exposure = services.telemetry.events();
        log_notification_shown(&*services.telemetry, &item, NotificationTelemetrySurface::Center);
        report(&reporter, ProgressStep { message: Some("another private report".into()), increment: Some(30.0), total: None });
        log_notification_shown(&*services.telemetry, &item, NotificationTelemetrySurface::Center);
        deferred.complete(()).await.unwrap();
        promise.await.unwrap();
        timeout(1000).await.unwrap();

        let exposures: Vec<(Dynamic, Dynamic, Dynamic, Dynamic)> = services
            .telemetry
            .shown()
            .iter()
            .map(|event| (field(event, "notificationId"), field(event, "extensionId"), field(event, "surface"), field(event, "cancellable")))
            .collect();
        assert_eq!((before_delay, before_exposure.len()), (0, 0));
        assert_eq!(
            exposures,
            vec![(Dynamic::string("extension.progress"), Dynamic::string("publisher.extension"), Dynamic::string("center"), Dynamic::Bool(true))]
        );
        assert_eq!(services.telemetry.interactions().len(), 0);
        assert_eq!(services.remaining(), 0);
    });
}

#[test]
fn keeps_core_progress_titles_and_updates_literal() {
    async_test(|store| async move {
        let services = create_services(&store);
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let title = "README.md [Open](command:unexpected)";
        let message = "[Help](https://example.com)";
        let progress: Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>> = Rc::new(RefCell::new(None));
        let promise = services.progress_service.with_progress(
            ProgressOptions { title: Some(title.into()), ..ProgressOptions::new(ProgressLocation::Notification) },
            task_capturing(&progress, deferred.promise.clone()),
            None,
        );
        let notification = services.notification(0);
        let initial = notification.message().linked_text.nodes.clone();
        report(&progress, ProgressStep { message: Some(message.into()), increment: None, total: None });
        let updated = notification.message().linked_text.nodes.clone();
        deferred.complete(()).await.unwrap();
        promise.await.unwrap();
        timeout(1000).await.unwrap();

        assert_eq!(initial, vec![text_node(title)]);
        assert_eq!(updated, vec![text_node(&format!("{title}: {message}"))]);
    });
}

#[test]
fn preserves_structured_titles_without_trusting_progress_text_or_later_updates() {
    async_test(|store| async move {
        let services = create_services(&store);
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let link = ("Show Logs", "command:showLogs");
        let message = "[Open](command:unexpected)";
        let progress: Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>> = Rc::new(RefCell::new(None));
        let promise = services.progress_service.with_progress(
            ProgressOptions { title: Some(NotificationText::link(link.0, link.1, None).into()), ..ProgressOptions::new(ProgressLocation::Notification) },
            task_capturing(&progress, deferred.promise.clone()),
            None,
        );
        let notification = services.notification(0);
        report(&progress, ProgressStep { message: Some(message.into()), increment: None, total: None });
        let literal_update = notification.message().linked_text.nodes.clone();
        notification.update_visibility(true);
        notification.update_visibility(false);
        timeout(200).await.unwrap();
        let status_text = services.status_entries.borrow().last().map(|entry| entry.text.clone());

        report(&progress, ProgressStep { message: Some(NotificationText::link("Details", "command:showDetails", None).into()), increment: None, total: None });
        let structured_update = notification.message().linked_text.nodes.clone();
        report(&progress, ProgressStep { message: Some("Details".into()), increment: None, total: None });
        let plain_update = notification.message().linked_text.nodes.clone();
        deferred.complete(()).await.unwrap();
        promise.await.unwrap();
        timeout(1000).await.unwrap();

        assert_eq!(literal_update, vec![link_node(link.0, link.1), text_node(&format!(": {message}"))]);
        assert_eq!(status_text, Some(format!("Show Logs: {message}")));
        assert_eq!(structured_update, vec![link_node(link.0, link.1), text_node(": "), link_node("Details", "command:showDetails")]);
        assert_eq!(plain_update, vec![link_node(link.0, link.1), text_node(": Details")]);
    });
}

#[test]
fn preserves_legacy_extension_progress_links_and_status_bar_text() {
    async_test(|store| async move {
        let services = create_services(&store);
        let deferred = Rc::new(DeferredPromise::<()>::new());
        let title = "[Show Logs](command:python.viewOutput)";
        let message = "[Check details](command:java.show.server.task.status)";
        let progress: Rc<RefCell<Option<Rc<dyn IProgress<ProgressStep>>>>> = Rc::new(RefCell::new(None));
        let promise = services.progress_service.with_progress(
            ProgressOptions {
                title: Some(title.into()),
                legacy_extension_link_parsing: Some(LEGACY_EXTENSION_LINK_PARSING),
                ..ProgressOptions::new(ProgressLocation::Notification)
            },
            task_capturing(&progress, deferred.promise.clone()),
            None,
        );
        let notification = services.notification(0);
        report(&progress, ProgressStep { message: Some(message.into()), increment: None, total: None });
        let raw = notification.message().raw.clone();
        let nodes = notification.message().linked_text.nodes.clone();
        notification.update_visibility(true);
        notification.update_visibility(false);
        timeout(200).await.unwrap();
        let status_text = services.status_entries.borrow().last().map(|entry| entry.text.clone());
        deferred.complete(()).await.unwrap();
        promise.await.unwrap();
        timeout(1000).await.unwrap();

        assert_eq!(raw, format!("{title}: {message}"));
        assert_eq!(
            nodes,
            vec![link_node("Show Logs", "command:python.viewOutput"), text_node(": "), link_node("Check details", "command:java.show.server.task.status")]
        );
        assert_eq!(status_text, Some("Show Logs: Check details".to_owned()));
    });
}
