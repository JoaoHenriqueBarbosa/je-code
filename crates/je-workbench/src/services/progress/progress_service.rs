//! Porte de `src/vs/workbench/services/progress/browser/progressService.ts`: o `ProgressService`, que mostra o progresso de
//! uma tarefa numa notificação, na barra de status (janela), no indicador de uma view ou de um container de views, na barra
//! de atividades ou num diálogo.
//!
//! Notas de tradução:
//! - **Colaboradores.** Os serviços que o original injeta e que o porte ainda não tem (ou tem reduzidos) entram como contratos
//!   estreitos deste módulo, com os mesmos nomes de operação do original: [`IProgressActivityService`]
//!   (`showViewContainerActivity` com `ProgressBadge`), [`IProgressPaneCompositeService`] (`getProgressIndicator`),
//!   [`IProgressViewsService`] (`getViewProgressIndicator`), [`IProgressStatusbarService`] (`addEntry`) e
//!   [`IProgressDialogFactory`] (o `Dialog` de `base/browser/ui/dialog`, com o `layoutService`, o `keybindingService` e o
//!   `hostService` que só servem para montá-lo). O `INotificationService`, o `IUserActivityService` e o
//!   `IViewDescriptorService` (recortado em [`IProgressViewDescriptorService`], que todo `IViewDescriptorService`
//!   implementa) são os do porte. O `IProgressIndicator` e o `IProgressRunner` são os de
//!   `je_editor::code_action::platform`.
//! - **Genéricos.** `withProgress<R>` e o `<P extends Promise<R>, R>` dos métodos privados são `Promise<()>`, como o
//!   [`IProgressService`] do porte (ver `je_platform::progress::progress`).
//! - **`Bad progress location`** lança de forma síncrona no original; aqui é um pânico com [`Error`] (`errors::throw`), que o
//!   chamador captura com `errors::catch`.
//! - **O objeto anônimo `progressStateModel`** (uma classe que estende `Disposable` e implementa `IProgress`) é o
//!   [`ProgressStateModel`].
//! - **`Timeout`** é o [`TimerId`] de `je_base::runtime`; `Date.now()` é `runtime::date_now()`.
//! - `-Number.MAX_VALUE` é `-f64::MAX`.
//! - O `Dialog` pendente só é criado pelo [`IProgressDialogFactory`]; o `dialog.show().then(...)` é a promessa de
//!   [`IProgressDialog::show`].
//! - `registerSingleton(IProgressService, ProgressService, InstantiationType.Delayed)` não tem equivalente: quem monta o
//!   workbench cria o serviço com [`ProgressService::new`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionCallback, ActionRef};
use je_base::async_::{RunOnceScheduler, async_fn, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::linked_text::parse_linked_text;
use je_base::nls::localize;
use je_base::icon_labels::strip_icons;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::runtime::{self, TimerId};
use je_editor::code_action::platform::{IProgressIndicator, IProgressRunner, ProgressTotal};
use je_platform::notification::notification_legacy::is_legacy_extension_link_parsing;
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::notification_telemetry::{NotificationActionTelemetry, NotificationActionTelemetryId, with_notification_action_telemetry};
use je_platform::notification::{
    INotificationService, Notification, NotificationActions, NotificationHandle, NotificationMessage, NotificationPriority, NotificationProgressProperties, NotificationProperties,
    NotificationSourceOrText, NotificationsFilter, Severity,
};
use je_platform::progress::progress::{
    IProgress, IProgressService, Progress, ProgressCancellable, ProgressLocation, ProgressLocationId, ProgressOptions, ProgressStep, ProgressTask, ProgressType,
};

use crate::common::views::{IViewDescriptorService, ViewContainerLocation, ViewContainerRc, ViewDescriptorRc};
use crate::services::statusbar::statusbar::StatusbarAlignment;
use crate::services::user_activity::user_activity_service::{IUserActivityService, MarkActiveOptions};

// ---------------------------------------------------------------------------------------------------------------------
// Contratos dos colaboradores
// ---------------------------------------------------------------------------------------------------------------------

/// `IActivityService.showViewContainerActivity(viewletId, { badge: new ProgressBadge(() => '') })`.
pub trait IProgressActivityService {
    fn show_view_container_activity(&self, view_container_id: &str) -> DisposableRef;
}

/// `IPaneCompositePartService.getProgressIndicator(id, viewContainerLocation)`.
pub trait IProgressPaneCompositeService {
    fn get_progress_indicator(&self, id: &str, view_container_location: ViewContainerLocation) -> Option<Rc<dyn IProgressIndicator>>;
}

/// `IViewsService.getViewProgressIndicator(viewId)`.
pub trait IProgressViewsService {
    fn get_view_progress_indicator(&self, view_id: &str) -> Option<Rc<dyn IProgressIndicator>>;
}

/// `showProgress?: boolean | 'loading' | 'syncing'` de `IStatusbarEntry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusbarShowProgress {
    Flag(bool),
    Loading,
    Syncing,
}

/// `IStatusbarEntry`, só com os campos que o progresso usa.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusbarProgressEntry {
    pub name: String,
    pub text: String,
    pub show_progress: StatusbarShowProgress,
    pub aria_label: String,
    pub tooltip: String,
    pub command: Option<String>,
}

/// `IStatusbarEntryAccessor`.
pub trait IStatusbarEntryAccessor: IDisposable {
    fn update(&self, entry: StatusbarProgressEntry);
}

/// `IStatusbarService.addEntry(entry, id, alignment, priority)`.
pub trait IProgressStatusbarService {
    fn add_entry(&self, entry: StatusbarProgressEntry, id: &str, alignment: StatusbarAlignment, priority: f64) -> Rc<dyn IStatusbarEntryAccessor>;
}

/// O resultado de `dialog.show()`: `{ button?: number }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressDialogResult {
    pub button: Option<i32>,
}

/// `Dialog` (o diálogo `pending` do progresso).
pub trait IProgressDialog: IDisposable {
    /// `show()`.
    fn show(&self) -> Promise<ProgressDialogResult>;
    /// `updateMessage(message)`.
    fn update_message(&self, message: &str);
}

/// `createWorkbenchDialogOptions({ type: 'pending', detail, cancelId, disableCloseAction, disableDefaultAction }, ...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressDialogOptions {
    pub detail: Option<String>,
    pub cancel_id: i32,
    pub disable_close_action: bool,
    pub disable_default_action: bool,
}

/// `new Dialog(this.layoutService.activeContainer, message, buttons, createWorkbenchDialogOptions(...))`.
pub trait IProgressDialogFactory {
    fn create_dialog(&self, message: &str, buttons: Vec<String>, options: ProgressDialogOptions) -> Rc<dyn IProgressDialog>;
}

/// O recorte do `IViewDescriptorService` que o progresso lê (as mesmas operações, com os mesmos nomes). Todo
/// [`IViewDescriptorService`] o implementa.
pub trait IProgressViewDescriptorService {
    fn get_view_container_by_id(&self, id: &str) -> Option<ViewContainerRc>;
    fn get_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation>;
    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc>;
    fn get_view_container_by_view_id(&self, id: &str) -> Option<ViewContainerRc>;
}

impl<T: IViewDescriptorService + ?Sized> IProgressViewDescriptorService for T {
    fn get_view_container_by_id(&self, id: &str) -> Option<ViewContainerRc> {
        IViewDescriptorService::get_view_container_by_id(self, id)
    }

    fn get_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        IViewDescriptorService::get_view_container_location(self, view_container)
    }

    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc> {
        IViewDescriptorService::get_view_descriptor_by_id(self, id)
    }

    fn get_view_container_by_view_id(&self, id: &str) -> Option<ViewContainerRc> {
        IViewDescriptorService::get_view_container_by_view_id(self, id)
    }
}

/// As dependências do [`ProgressService`] (o que o construtor injeta).
pub struct ProgressServiceParts {
    pub activity_service: Rc<dyn IProgressActivityService>,
    pub pane_composite_service: Rc<dyn IProgressPaneCompositeService>,
    pub view_descriptor_service: Rc<dyn IProgressViewDescriptorService>,
    pub views_service: Rc<dyn IProgressViewsService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub statusbar_service: Rc<dyn IProgressStatusbarService>,
    pub dialog_factory: Rc<dyn IProgressDialogFactory>,
    pub user_activity_service: Rc<dyn IUserActivityService>,
}

// ---------------------------------------------------------------------------------------------------------------------
// ProgressService
// ---------------------------------------------------------------------------------------------------------------------

/// `[IProgressWindowOptions, Progress<IProgressStep>]`.
struct WindowProgressTask {
    options: ProgressOptions,
    progress: Rc<Progress<ProgressStep>>,
}

/// O texto do `toString()` de `string | NotificationText`.
fn part_to_string(part: &NotificationPart) -> String {
    match part {
        NotificationPart::Text(text) => text.clone(),
        NotificationPart::Rich(rich) => rich.to_string(),
    }
}

/// `part?.toString()`, e `''` conta como ausente nos `if (progressTitle && ...)`.
fn non_empty(part: Option<&NotificationPart>) -> Option<String> {
    part.map(part_to_string).filter(|text| !text.is_empty())
}

/// A truthiness de `string | NotificationText`: o texto vazio é falso, e o objeto `NotificationText` é sempre verdadeiro.
fn truthy_part(part: &NotificationPart) -> bool {
    match part {
        NotificationPart::Text(text) => !text.is_empty(),
        NotificationPart::Rich(_) => true,
    }
}

/// `ProgressService`.
pub struct ProgressService {
    base: Rc<Disposable>,
    this: Weak<Self>,
    parts: ProgressServiceParts,
    window_progress_stack: RefCell<Vec<Rc<WindowProgressTask>>>,
    window_progress_status_entry: RefCell<Option<Rc<dyn IStatusbarEntryAccessor>>>,
}

impl ProgressService {
    pub fn new(parts: ProgressServiceParts) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            base: Disposable::new(),
            this: this.clone(),
            parts,
            window_progress_stack: RefCell::new(Vec::new()),
            window_progress_status_entry: RefCell::new(None),
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `withProgress(options, originalTask, onDidCancel)`.
    pub fn with_progress(&self, options: ProgressOptions, original_task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let location = options.location.clone();

        let user_activity_service = self.parts.user_activity_service.clone();
        let task: ProgressTask = Box::new(move |progress| {
            let active_lock = user_activity_service.mark_active(Some(MarkActiveOptions { extend_only: true, when_held_for: Some(15_000) }));
            let promise = original_task(progress);
            async_fn(async move {
                let result = promise.await;
                active_lock.dispose();
                result
            })
        });

        match location {
            ProgressLocationId::View(location) => self.handle_string_location(&location, task, &options),
            ProgressLocationId::Location(location) => match location {
                ProgressLocation::Notification => {
                    let mut priority = options.priority;
                    if priority != Some(NotificationPriority::Urgent) {
                        if self.parts.notification_service.get_filter(None) == NotificationsFilter::Error {
                            priority = Some(NotificationPriority::Silent);
                        } else if let Some(NotificationSourceOrText::Source(source)) = &options.source
                            && self.parts.notification_service.get_filter(Some(source)) == NotificationsFilter::Error
                        {
                            priority = Some(NotificationPriority::Silent);
                        }
                    }

                    self.with_notification_progress(ProgressOptions { location: ProgressLocation::Notification.into(), priority, ..options }, task, on_did_cancel)
                }
                ProgressLocation::Window => {
                    let kind = options.kind;
                    if options.command.is_some() {
                        // Window progress with command get's shown in the status bar
                        return self.with_window_progress(ProgressOptions { location: ProgressLocation::Window.into(), kind, ..options }, task);
                    }
                    // Window progress without command can be shown as silent notification
                    // which will first appear in the status bar and can then be brought to
                    // the front when clicking.
                    let delay = options.delay.or(Some(150.0 /* default for ProgressLocation.Window */));
                    self.with_notification_progress(
                        ProgressOptions { delay, priority: Some(NotificationPriority::Silent), location: ProgressLocation::Notification.into(), kind, ..options },
                        task,
                        on_did_cancel,
                    )
                }
                ProgressLocation::Explorer => {
                    self.with_pane_composite_progress("workbench.view.explorer", ViewContainerLocation::Sidebar, task, ProgressOptions { location: location.into(), ..options })
                }
                ProgressLocation::Scm => self.handle_string_location("workbench.scm", task, &options),
                ProgressLocation::Extensions => {
                    self.with_pane_composite_progress("workbench.view.extensions", ViewContainerLocation::Sidebar, task, ProgressOptions { location: location.into(), ..options })
                }
                ProgressLocation::Dialog => self.with_dialog_progress(options, task, on_did_cancel),
            },
        }
    }

    /// `handleStringLocation(location)`.
    fn handle_string_location(&self, location: &str, task: ProgressTask, options: &ProgressOptions) -> Promise<()> {
        let options = ProgressOptions { location: ProgressLocationId::View(location.to_owned()), ..options.clone() };
        if let Some(view_container) = self.parts.view_descriptor_service.get_view_container_by_id(location) {
            let view_container_location = self.parts.view_descriptor_service.get_view_container_location(&view_container);
            if let Some(view_container_location) = view_container_location {
                return self.with_pane_composite_progress(location, view_container_location, task, options);
            }
        }

        if self.parts.view_descriptor_service.get_view_descriptor_by_id(location).is_some() {
            return self.with_view_progress(location, task, options);
        }

        errors::throw(Error::new(format!("Bad progress location: {location}")))
    }

    fn with_window_progress(&self, options: ProgressOptions, callback: ProgressTask) -> Promise<()> {
        let this = self.this.clone();
        let progress = Rc::new(Progress::new(move |_| {
            if let Some(this) = this.upgrade() {
                this.update_window_progress(0);
            }
        }));
        let task = Rc::new(WindowProgressTask { options, progress: progress.clone() });

        let promise = callback(progress);

        let delay_handle: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
        let this = self.rc();
        let timer_task = task.clone();
        let timer_promise = promise.clone();
        let timer_handle = delay_handle.clone();
        delay_handle.set(Some(runtime::set_timeout(
            move || {
                timer_handle.set(None);
                this.window_progress_stack.borrow_mut().insert(0, timer_task.clone());
                this.update_window_progress(0);

                // show progress for at least 150ms
                let finished = this.clone();
                let finished_task = timer_task.clone();
                drop(Promise::all(vec![timeout(150).promise, timer_promise]).finally(move || {
                    let idx = finished.window_progress_stack.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, &finished_task));
                    if let Some(idx) = idx {
                        finished.window_progress_stack.borrow_mut().remove(idx);
                    }
                    finished.update_window_progress(0);
                }));
            },
            150,
        )));

        // cancel delay if promise finishes below 150ms
        promise.finally(move || {
            if let Some(handle) = delay_handle.take() {
                runtime::clear_timeout(handle);
            }
        })
    }

    fn update_window_progress(&self, idx: usize) {
        let entry = self.window_progress_stack.borrow().get(idx).cloned();

        // We still have progress to show
        if let Some(entry) = entry {
            let options = &entry.options;

            let progress_title = non_empty(options.title.as_ref());
            let progress_message = non_empty(entry.progress.value().and_then(|step| step.message).as_ref());
            let progress_command = options.command.clone();
            let text: String;
            let title: String;
            let source = match &options.source {
                Some(NotificationSourceOrText::Source(source)) => Some(source.label.clone()).filter(|label| !label.is_empty()),
                Some(NotificationSourceOrText::Text(text)) => Some(text.clone()).filter(|text| !text.is_empty()),
                None => None,
            };

            if let (Some(progress_title), Some(progress_message)) = (&progress_title, &progress_message) {
                // <title>: <message>
                text = localize("progress.text2", "{0}: {1}", &[progress_title.as_str(), progress_message.as_str()]);
                title = match &source {
                    Some(source) => localize("progress.title3", "[{0}] {1}: {2}", &[source.as_str(), progress_title.as_str(), progress_message.as_str()]),
                    None => text.clone(),
                };
            } else if let Some(progress_title) = &progress_title {
                // <title>
                text = progress_title.clone();
                title = match &source {
                    Some(source) => localize("progress.title2", "[{0}]: {1}", &[source.as_str(), progress_title.as_str()]),
                    None => text.clone(),
                };
            } else if let Some(progress_message) = &progress_message {
                // <message>
                text = progress_message.clone();
                title = match &source {
                    Some(source) => localize("progress.title2", "[{0}]: {1}", &[source.as_str(), progress_message.as_str()]),
                    None => text.clone(),
                };
            } else {
                // no title, no message -> no progress. try with next on stack
                self.update_window_progress(idx + 1);
                return;
            }

            let status_entry_properties = StatusbarProgressEntry {
                name: localize("status.progress", "Progress Message", &[]),
                text: text.clone(),
                show_progress: match options.kind {
                    Some(ProgressType::Loading) => StatusbarShowProgress::Loading,
                    Some(ProgressType::Syncing) => StatusbarShowProgress::Syncing,
                    None => StatusbarShowProgress::Flag(true),
                },
                aria_label: text,
                tooltip: strip_icons(&title.encode_utf16().collect::<Vec<u16>>()).to_string_lossy().trim().to_owned(),
                command: progress_command,
            };

            let accessor = self.window_progress_status_entry.borrow().clone();
            match accessor {
                Some(accessor) => accessor.update(status_entry_properties),
                None => {
                    let accessor = self.parts.statusbar_service.add_entry(status_entry_properties, "status.progress", StatusbarAlignment::Left, -f64::MAX /* almost last entry */);
                    *self.window_progress_status_entry.borrow_mut() = Some(accessor);
                }
            }
        }
        // Progress is done so we remove the status entry
        else {
            let accessor = self.window_progress_status_entry.borrow_mut().take();
            if let Some(accessor) = accessor {
                accessor.dispose();
            }
        }
    }

    fn with_notification_progress(&self, options: ProgressOptions, callback: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let options = Rc::new(options);
        let progress_state_model = ProgressStateModel::new(callback, on_did_cancel);

        let service = self.rc();

        let create_window_progress: Rc<dyn Fn() -> DisposableRef> = {
            let service = service.clone();
            let options = options.clone();
            let progress_state_model = progress_state_model.clone();
            Rc::new(move || {
                let options = options.clone();
                let to_plain_text = {
                    let options = options.clone();
                    move |message: Option<&NotificationPart>| -> Option<NotificationPart> {
                        match message {
                            Some(NotificationPart::Text(text)) if is_legacy_extension_link_parsing(options.legacy_extension_link_parsing.as_ref()) => {
                                let units: Vec<u16> = text.encode_utf16().collect();
                                Some(NotificationPart::Text(parse_linked_text(&units).to_js_string().to_string_lossy()))
                            }
                            Some(message) => Some(NotificationPart::Text(part_to_string(message))),
                            None => None,
                        }
                    }
                };

                // Create a promise that we can resolve as needed
                // when the outside calls dispose on us
                let promise = Rc::new(DeferredPromise::<()>::new());

                let window_options = ProgressOptions {
                    title: to_plain_text(options.title.as_ref()),
                    command: Some("notifications.showList".to_owned()),
                    kind: options.kind,
                    ..ProgressOptions::new(ProgressLocation::Window)
                };
                let task_state = progress_state_model.clone();
                let task_promise = promise.clone();
                let task_plain_text = to_plain_text.clone();
                drop(service.with_window_progress(
                    window_options,
                    Box::new(move |progress| {
                        let report_progress = {
                            let progress = progress.clone();
                            move |step: &ProgressStep| {
                                if step.message.as_ref().is_some_and(truthy_part) {
                                    progress.report(ProgressStep { message: task_plain_text(step.message.as_ref()), ..ProgressStep::default() });
                                }
                            }
                        };

                        // Apply any progress that was made already
                        if let Some(step) = task_state.step() {
                            report_progress(&step);
                        }

                        // Continue to report progress as it happens
                        let on_did_report_listener = task_state.on_did_report().listen(move |step| report_progress(step));
                        drop(task_promise.promise.finally(move || on_did_report_listener.dispose()));

                        // When the progress model gets disposed, we are done as well
                        let completing = task_promise.clone();
                        task_state.on_will_dispose().once().listen(move |_| drop(completing.complete(())));

                        task_promise.promise.clone()
                    }),
                ));

                // Dispose means completing our promise
                to_disposable(move || drop(promise.complete(())))
            })
        };

        let create_notification: Rc<dyn Fn(NotificationPart, Option<NotificationPriority>, Option<f64>) -> Rc<dyn NotificationHandle>> = {
            let service = service.clone();
            let options = options.clone();
            let progress_state_model = progress_state_model.clone();
            let create_window_progress = create_window_progress.clone();
            Rc::new(move |message: NotificationPart, priority: Option<NotificationPriority>, increment: Option<f64>| {
                let notification_disposables = DisposableStore::new();

                let mut primary_actions: Vec<ActionRef> = options.primary_actions.clone().unwrap_or_default();
                let secondary_actions: Vec<ActionRef> = options.secondary_actions.clone().unwrap_or_default();

                if let Some(buttons) = &options.buttons {
                    for (index, button) in buttons.iter().enumerate() {
                        let state = progress_state_model.clone();
                        let callback: ActionCallback = Rc::new(move |_| {
                            state.cancel(Some(index as i32));
                            MaybePromise::Promise(Promise::resolved(Dynamic::Undefined))
                        });
                        let button_action = notification_disposables.add_rc(Action::new(&format!("progress.button.{button}"), button, "", true, Some(callback)));

                        primary_actions.push(button_action);
                    }
                }

                let cancel_label = match &options.cancellable {
                    Some(ProgressCancellable::Flag(true)) => Some(localize("cancel", "Cancel", &[])),
                    Some(ProgressCancellable::Label(label)) if !label.is_empty() => Some(label.clone()),
                    _ => None,
                };
                if let Some(cancel_label) = cancel_label {
                    let state = progress_state_model.clone();
                    let callback: ActionCallback = Rc::new(move |_| {
                        state.cancel(None);
                        MaybePromise::Promise(Promise::resolved(Dynamic::Undefined))
                    });
                    let cancel_action = notification_disposables.add_rc(Action::new("progress.cancel", &cancel_label, "", true, Some(callback)));
                    let cancel_action: ActionRef = with_notification_action_telemetry(cancel_action, Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::ProgressCancel)));

                    primary_actions.push(cancel_action);
                }

                let strip = |text: &str| strip_icons(&text.encode_utf16().collect::<Vec<u16>>()).to_string_lossy();
                let notification_message = match &message {
                    NotificationPart::Text(text) => NotificationMessage::Text(strip(text)),
                    NotificationPart::Rich(rich) => NotificationMessage::Rich(rich.map_text(|text| strip_icons(text.units()))),
                };
                let notification = service.parts.notification_service.notify(Notification {
                    properties: NotificationProperties {
                        telemetry: options.telemetry.clone(),
                        legacy_extension_link_parsing: options.legacy_extension_link_parsing.clone(),
                        sticky: false,
                        priority,
                        never_show_again: None,
                    },
                    id: None,
                    severity: Severity::Info,
                    message: notification_message,
                    source: options.source.clone(),
                    actions: Some(NotificationActions { primary: primary_actions, secondary: secondary_actions }),
                    progress: Some(match increment {
                        Some(increment) if increment >= 0.0 => NotificationProgressProperties { infinite: false, total: Some(100.0), worked: Some(increment) },
                        _ => NotificationProgressProperties { infinite: true, total: None, worked: None },
                    }),
                });

                // Switch to window based progress once the notification
                // changes visibility to hidden and is still ongoing.
                // Remove that window based progress once the notification
                // shows again.
                let window_progress_disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
                let on_visibility_change: Rc<dyn Fn(bool)> = {
                    let window_progress_disposable = window_progress_disposable.clone();
                    let progress_state_model = progress_state_model.clone();
                    let create_window_progress = create_window_progress.clone();
                    Rc::new(move |visible: bool| {
                        // Clear any previous running window progress
                        let previous = window_progress_disposable.borrow_mut().take();
                        if let Some(previous) = previous {
                            previous.dispose();
                        }

                        // Create new window progress if notification got hidden
                        if !visible && !progress_state_model.done() {
                            *window_progress_disposable.borrow_mut() = Some(create_window_progress());
                        }
                    })
                };
                let on_visibility = on_visibility_change.clone();
                notification.on_did_change_visibility().listen_in(move |visible| on_visibility(*visible), &notification_disposables);
                if priority == Some(NotificationPriority::Silent) {
                    on_visibility_change(false);
                }

                // Clear upon dispose
                let disposables = notification_disposables.clone();
                notification.on_did_close().once().listen(move |_| {
                    disposables.dispose();
                    let previous = window_progress_disposable.borrow_mut().take();
                    if let Some(previous) = previous {
                        previous.dispose();
                    }
                });

                notification
            })
        };

        let update_progress = |notification: &Rc<dyn NotificationHandle>, increment: Option<f64>| match increment {
            Some(increment) if increment >= 0.0 => {
                notification.progress().total(100.0); // always percentage based
                notification.progress().worked(increment);
            }
            _ => notification.progress().infinite(),
        };

        let notification_handle: Rc<RefCell<Option<Rc<dyn NotificationHandle>>>> = Rc::new(RefCell::new(None));
        let notification_timeout: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
        // hoisted to make sure a delayed notification shows the most recent message
        let title_and_message: Rc<RefCell<Option<NotificationPart>>> = Rc::new(RefCell::new(None));

        let update_notification: Rc<dyn Fn(Option<&ProgressStep>)> = {
            let options = options.clone();
            let notification_handle = notification_handle.clone();
            let notification_timeout = notification_timeout.clone();
            let title_and_message = title_and_message.clone();
            let create_notification = create_notification.clone();
            Rc::new(move |step: Option<&ProgressStep>| {
                let step_message = step.and_then(|step| step.message.clone()).filter(truthy_part);
                let option_title = options.title.clone().filter(truthy_part);

                // full message (inital or update)
                *title_and_message.borrow_mut() = match (&step_message, &option_title) {
                    (Some(message), Some(title)) => Some(match (title, message) {
                        (NotificationPart::Text(title), NotificationPart::Text(message)) => NotificationPart::Text(format!("{title}: {message}")),
                        _ => NotificationPart::Rich(NotificationText::concat([title.clone(), NotificationPart::Text(": ".to_owned()), message.clone()])),
                    }),
                    _ => option_title.or(step_message),
                };

                let current = title_and_message.borrow().clone();
                let no_handle = notification_handle.borrow().is_none();
                if no_handle
                    && let Some(current) = &current
                {
                    // create notification now or after a delay
                    let delay = options.delay.filter(|delay| *delay > 0.0);
                    let increment = step.and_then(|step| step.increment);
                    if let Some(delay) = delay {
                        if notification_timeout.get().is_none() {
                            let (handle, titles, create, priority) = (notification_handle.clone(), title_and_message.clone(), create_notification.clone(), options.priority);
                            notification_timeout.set(Some(runtime::set_timeout(
                                move || {
                                    let message = titles.borrow().clone().expect("a mensagem existe quando o atraso termina");
                                    let created = create(message, priority, increment);
                                    *handle.borrow_mut() = Some(created);
                                },
                                delay as u64,
                            )));
                        }
                    } else {
                        let created = create_notification(current.clone(), options.priority, increment);
                        *notification_handle.borrow_mut() = Some(created);
                    }
                }

                let handle = notification_handle.borrow().clone();
                if let Some(handle) = handle {
                    if let Some(current) = &current {
                        handle.update_message(match current {
                            NotificationPart::Text(text) => NotificationMessage::Text(text.clone()),
                            NotificationPart::Rich(rich) => NotificationMessage::Rich(rich.clone()),
                        });
                    }

                    if let Some(increment) = step.and_then(|step| step.increment) {
                        update_progress(&handle, Some(increment));
                    }
                }
            })
        };

        // Show initially
        update_notification(progress_state_model.step().as_ref());
        let listener = {
            let update_notification = update_notification.clone();
            progress_state_model.on_did_report().listen(move |step| update_notification(Some(step)))
        };
        progress_state_model.on_will_dispose().once().listen(move |_| listener.dispose());

        // Clean up eventually
        let promise = progress_state_model.promise();
        let cleanup_promise = promise.clone();
        let has_delay = options.delay.is_some_and(|delay| delay > 0.0);
        drop(async_fn(async move {
            let outcome: Result<(), Error> = async {
                // with a delay we only wait for the finish of the promise
                if has_delay {
                    cleanup_promise.await
                }
                // without a delay we show the notification for at least 800ms
                // to reduce the chance of the notification flashing up and hiding
                else {
                    Promise::all(vec![timeout(800).promise, cleanup_promise]).await.map(|_| ())
                }
            }
            .await;
            if let Some(handle) = notification_timeout.take() {
                runtime::clear_timeout(handle);
            }
            let handle = notification_handle.borrow().clone();
            if let Some(handle) = handle {
                handle.close();
            }
            outcome
        }));

        promise
    }

    fn with_pane_composite_progress(&self, pane_composite_id: &str, view_container_location: ViewContainerLocation, task: ProgressTask, options: ProgressOptions) -> Promise<()> {
        // show in viewlet
        let progress_indicator = self.parts.pane_composite_service.get_progress_indicator(pane_composite_id, view_container_location);
        let promise = match progress_indicator {
            Some(progress_indicator) => self.with_composite_progress(progress_indicator, task, &options),
            None => task(Rc::new(Progress::new(|_: ProgressStep| {}))),
        };

        // show on activity bar
        if view_container_location == ViewContainerLocation::Sidebar {
            self.show_on_activity_bar(pane_composite_id, &options, &promise);
        }

        promise
    }

    fn with_view_progress(&self, view_id: &str, task: ProgressTask, options: ProgressOptions) -> Promise<()> {
        // show in viewlet
        let progress_indicator = self.parts.views_service.get_view_progress_indicator(view_id);
        let promise = match progress_indicator {
            Some(progress_indicator) => self.with_composite_progress(progress_indicator, task, &options),
            None => task(Rc::new(Progress::new(|_: ProgressStep| {}))),
        };

        let Some(view_container) = self.parts.view_descriptor_service.get_view_container_by_view_id(view_id) else {
            return promise;
        };

        // show on activity bar
        self.show_on_activity_bar(&view_container.id, &options, &promise);

        promise
    }

    fn show_on_activity_bar(&self, viewlet_id: &str, options: &ProgressOptions, promise: &Promise<()>) {
        let activity_progress: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let delay_handle: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
        let activity_service = self.parts.activity_service.clone();
        let viewlet_id = viewlet_id.to_owned();
        let (timer_handle, timer_activity) = (delay_handle.clone(), activity_progress.clone());
        let delay = options.delay.filter(|delay| *delay != 0.0).unwrap_or(300.0) as u64;
        delay_handle.set(Some(runtime::set_timeout(
            move || {
                timer_handle.set(None);
                let handle = activity_service.show_view_container_activity(&viewlet_id);
                let start_time_visible = runtime::date_now();
                let min_time_visible = 300;
                *timer_activity.borrow_mut() = Some(to_disposable(move || {
                    let d = runtime::date_now().saturating_sub(start_time_visible);
                    if d < min_time_visible {
                        // should at least show for Nms
                        runtime::set_timeout(move || handle.dispose(), min_time_visible - d);
                    } else {
                        // shown long enough
                        handle.dispose();
                    }
                }));
            },
            delay,
        )));
        drop(promise.finally(move || {
            if let Some(handle) = delay_handle.take() {
                runtime::clear_timeout(handle);
            }
            let activity = activity_progress.borrow_mut().take();
            if let Some(activity) = activity {
                activity.dispose();
            }
        }));
    }

    fn with_composite_progress(&self, progress_indicator: Rc<dyn IProgressIndicator>, task: ProgressTask, options: &ProgressOptions) -> Promise<()> {
        let discrete_progress_runner: Rc<RefCell<Option<Rc<dyn IProgressRunner>>>> = Rc::new(RefCell::new(None));
        let promise_slot: Rc<RefCell<Option<Promise<()>>>> = Rc::new(RefCell::new(None));
        let delay = options.delay.map(|delay| delay as u64);

        let update_progress: Rc<dyn Fn(Option<(f64, Option<f64>)>)> = {
            let discrete_progress_runner = discrete_progress_runner.clone();
            let promise_slot = promise_slot.clone();
            Rc::new(move |step_or_total| {
                // Figure out whether discrete progress applies
                // by figuring out the "total" progress to show
                // and the increment if any.
                //
                // Discrete
                if let Some((total, increment)) = step_or_total {
                    if discrete_progress_runner.borrow().is_none() {
                        let runner = progress_indicator.show(ProgressTotal::Total(total), delay);
                        *discrete_progress_runner.borrow_mut() = Some(runner);
                        let promise = promise_slot.borrow().clone().expect("a promessa existe depois da tarefa");
                        let runner_slot = discrete_progress_runner.clone();
                        drop(promise.catch(|_| ()).finally(move || {
                            let runner = runner_slot.borrow().clone();
                            if let Some(runner) = runner {
                                runner.done();
                            }
                        }));
                    }

                    if let Some(increment) = increment {
                        let runner = discrete_progress_runner.borrow().clone();
                        if let Some(runner) = runner {
                            runner.worked(increment);
                        }
                    }
                }
                // Infinite
                else {
                    let runner = discrete_progress_runner.borrow().clone();
                    if let Some(runner) = runner {
                        runner.done();
                    }
                    let promise = promise_slot.borrow().clone().expect("a promessa existe depois da tarefa");
                    drop(progress_indicator.show_while(&promise, delay));
                }
            })
        };

        let report_update = update_progress.clone();
        let promise = task(Rc::new(Progress::new(move |progress: ProgressStep| {
            // `typeof stepOrTotal.increment === 'number'`: total ?? 100 (always percentage based)
            report_update(progress.increment.map(|increment| (progress.total.unwrap_or(100.0), Some(increment))));
        })));
        *promise_slot.borrow_mut() = Some(promise.clone());

        update_progress(options.total.map(|total| (total, None)));

        promise
    }

    fn with_dialog_progress(&self, options: ProgressOptions, task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let disposables = DisposableStore::new();

        let dialog: Rc<RefCell<Option<Rc<dyn IProgressDialog>>>> = Rc::new(RefCell::new(None));
        let task_completed = Rc::new(Cell::new(false));

        let create_dialog: Rc<dyn Fn(&str) -> Rc<dyn IProgressDialog>> = {
            let dialog_factory = self.parts.dialog_factory.clone();
            let options = options.clone();
            let disposables = disposables.clone();
            let task_completed = task_completed.clone();
            let dialog = dialog.clone();
            Rc::new(move |message| {
                let mut buttons = options.buttons.clone().unwrap_or_default();
                if options.sticky != Some(true) {
                    buttons.push(match &options.cancellable {
                        Some(ProgressCancellable::Flag(true)) => localize("cancel", "Cancel", &[]),
                        Some(ProgressCancellable::Label(label)) if !label.is_empty() => label.clone(),
                        _ => localize("dismiss", "Dismiss", &[]),
                    });
                }

                let created = dialog_factory.create_dialog(
                    message,
                    buttons.clone(),
                    ProgressDialogOptions {
                        detail: options.detail.clone(),
                        cancel_id: buttons.len() as i32 - 1,
                        disable_close_action: options.sticky == Some(true),
                        disable_default_action: options.sticky == Some(true),
                    },
                );

                let created_disposable: DisposableRef = created.clone();
                disposables.add(created_disposable);

                let (task_completed, on_did_cancel, dialog) = (task_completed.clone(), on_did_cancel.clone(), dialog.clone());
                let shown = created.clone();
                drop(created.show().then(move |dialog_result| {
                    // The dialog may close as a result of disposing it after the
                    // task has completed. In that case, we do not want to trigger
                    // the `onDidCancel` callback.
                    // However, if the task is still running, this means that the
                    // user has clicked the cancel button and we want to trigger
                    // the `onDidCancel` callback.
                    if !task_completed.get()
                        && let Some(on_did_cancel) = &on_did_cancel
                    {
                        on_did_cancel(dialog_result.button);
                    }
                    let current = dialog.borrow().clone();
                    if let Some(current) = current {
                        current.dispose();
                    } else {
                        shown.dispose();
                    }
                }));

                created
            })
        };

        // In order to support the `delay` option, we use a scheduler
        // that will guard each access to the dialog behind a delay
        // that is either the original delay for one invocation and
        // otherwise runs without delay.
        let delay = Rc::new(Cell::new(options.delay.map_or(0, |delay| delay as u64)));
        let latest_message: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let scheduler = {
            let (delay, latest_message, dialog, create_dialog) = (delay.clone(), latest_message.clone(), dialog.clone(), create_dialog.clone());
            RunOnceScheduler::new(
                move || {
                    delay.set(0); // since we have run once, we reset the delay

                    let message = latest_message.borrow().clone().filter(|message| !message.is_empty());
                    if let Some(message) = message {
                        let existing = dialog.borrow().clone();
                        match existing {
                            None => {
                                let created = create_dialog(&message);
                                *dialog.borrow_mut() = Some(created);
                            }
                            Some(existing) => existing.update_message(&message),
                        }
                    }
                },
                0,
            )
        };
        disposables.add_rc(scheduler.clone());

        let update_dialog: Rc<dyn Fn(Option<&NotificationPart>)> = {
            let (latest_message, scheduler, delay) = (latest_message.clone(), scheduler.clone(), delay.clone());
            Rc::new(move |message| {
                *latest_message.borrow_mut() = message.map(part_to_string);

                // Make sure to only run one dialog update and not multiple
                if !scheduler.is_scheduled() {
                    scheduler.schedule(Some(delay.get()));
                }
            })
        };

        let report_update = update_dialog.clone();
        let promise = task(Rc::new(Progress::new(move |progress: ProgressStep| report_update(progress.message.as_ref()))));

        let (finished_completed, finished_disposables) = (task_completed, disposables);
        drop(promise.finally(move || {
            finished_completed.set(true);
            finished_disposables.dispose();
        }));

        if let Some(title) = options.title.as_ref().filter(|title| truthy_part(title)) {
            update_dialog(Some(title));
        }

        promise
    }
}

impl IDisposable for ProgressService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IProgressService for ProgressService {
    fn with_progress(&self, options: ProgressOptions, task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        ProgressService::with_progress(self, options, task, on_did_cancel)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ProgressStateModel
// ---------------------------------------------------------------------------------------------------------------------

/// O `progressStateModel` de `withNotificationProgress`: a classe anônima que estende `Disposable` e implementa `IProgress`.
struct ProgressStateModel {
    base: Rc<Disposable>,
    on_did_report: Rc<Emitter<ProgressStep>>,
    on_will_dispose: Rc<Emitter<()>>,
    step: RefCell<Option<ProgressStep>>,
    done: Cell<bool>,
    promise: RefCell<Option<Promise<()>>>,
    on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>,
}

impl ProgressStateModel {
    fn new(callback: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_report = Emitter::new();
        base.register(on_did_report.clone());
        let on_will_dispose = Emitter::new();
        base.register(on_will_dispose.clone());
        let model = Rc::new(Self {
            base,
            on_did_report,
            on_will_dispose,
            step: RefCell::new(None),
            done: Cell::new(false),
            promise: RefCell::new(None),
            on_did_cancel,
        });

        let promise = callback(model.clone());
        *model.promise.borrow_mut() = Some(promise.clone());

        let finished = model.clone();
        drop(promise.finally(move || finished.dispose()));
        model
    }

    fn on_did_report(&self) -> Event<ProgressStep> {
        self.on_did_report.event()
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn step(&self) -> Option<ProgressStep> {
        self.step.borrow().clone()
    }

    fn done(&self) -> bool {
        self.done.get()
    }

    fn promise(&self) -> Promise<()> {
        self.promise.borrow().clone().expect("a promessa existe depois do construtor")
    }

    fn cancel(&self, choice: Option<i32>) {
        if let Some(on_did_cancel) = &self.on_did_cancel {
            on_did_cancel(choice);
        }

        self.dispose();
    }
}

impl IProgress<ProgressStep> for ProgressStateModel {
    fn report(&self, step: ProgressStep) {
        *self.step.borrow_mut() = Some(step.clone());

        self.on_did_report.fire(step);
    }
}

impl IDisposable for ProgressStateModel {
    fn dispose(&self) {
        self.done.set(true);
        self.on_will_dispose.fire(());

        self.base.dispose();
    }
}
