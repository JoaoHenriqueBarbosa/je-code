//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionRecommendationNotificationService.ts`: as notificações
//! que recomendam extensões (por arquivo aberto, por executável e pelo workspace), uma visível por vez, com fila.
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor entram agrupados em
//!   [`ExtensionRecommendationNotificationServiceParts`]. O `IExtensionsWorkbenchService` é o [`ExtensionsWorkbenchService`]
//!   concreto; o `IWorkbenchExtensionManagementService` é o [`IWorkbenchExtensionInstallService`].
//! - **`open` e `openSearch`.** O `ExtensionsWorkbenchService` do porte não abre editores nem a viewlet (a UI nativa fica
//!   fora do modelo lógico); as duas chamadas que as ações "Install" e "Show Recommendations" fazem passam pelo
//!   [`IExtensionsOpener`], que a camada de apresentação implementa.
//! - **`async` das ações** (`run`) viram promessas soltas, como o original, que descarta o valor devolvido.
//! - **Fila.** `pendingNotificaitons.splice(index, 1)` do cancelamento remove o item daquele índice se ele ainda existir.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, create_cancelable_promise, race_cancellable_promises, race_cancellation, timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::is_cancellation_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::{CancelablePromise, Promise};
use je_extensions::management::{ExtensionInfo, ExtensionQueryOptions, InstallOptions};
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_extensions::util::are_same_extensions;
use je_platform::configuration::service::IConfigurationService;
use je_platform::extension_recommendations::extension_recommendations::{
    ExtensionRecommendationsRequest, IExtensionRecommendationNotificationService, RecommendationSource, RecommendationsNotificationResult, RecommendedExtension,
    recommendation_source_to_string,
};
use je_platform::notification::{
    BasePromptChoice, INotificationService, NotificationHandle, NotificationMessage, NotificationPriority, NotificationProperties, PromptChoice, PromptChoiceKind, PromptChoiceWithMenu,
    PromptOptions, Severity,
};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, SyncResource};

use super::extensions::IExtension;
use super::extensions_service::{ExtensionsWorkbenchService, InstallArg, InstallExtensionOptions};
use super::extensions_workbench_service::Extension;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extension_management::extension_install_service::{IWorkbenchExtensionInstallService, InstallExtensionInfo};
use crate::services::extension_management::extension_management::{EnablementState, IWorkbenchExtensionEnablementService, local_extension_uri};
use crate::services::extension_recommendations::extension_recommendations::IExtensionIgnoredRecommendationsService;

const IGNORE_IMPORTANT_EXTENSION_RECOMMENDATION_STORAGE_KEY: &str = "extensionsAssistant/importantRecommendationsIgnore";
const DONOT_SHOW_WORKSPACE_RECOMMENDATIONS_STORAGE_KEY: &str = "extensionsAssistant/workspaceRecommendationsIgnore";

/// `IExtensionsWorkbenchService.open(extension, { pinned: true })` e `openSearch(value)`: o que abre a extensão e a
/// busca na interface.
pub trait IExtensionsOpener {
    fn open(&self, extension: &Rc<Extension>, pinned: bool) -> Promise<()>;
    fn open_search(&self, value: &str);
}

/// Os serviços que o construtor do original recebe por decorador.
pub struct ExtensionRecommendationNotificationServiceParts {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub storage_service: Rc<dyn StorageService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub extensions_workbench_service: Rc<ExtensionsWorkbenchService>,
    pub extensions_opener: Rc<dyn IExtensionsOpener>,
    pub extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    pub extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    pub extension_ignored_recommendations_service: Rc<dyn IExtensionIgnoredRecommendationsService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
}

/// `RecommendationsNotificationActions`.
#[derive(Clone)]
struct RecommendationsNotificationActions {
    on_did_install_recommended_extensions: Rc<dyn Fn(&[Rc<Extension>])>,
    on_did_show_recommended_extensions: Rc<dyn Fn(&[Rc<Extension>])>,
    on_did_cancel_recommended_extensions: Rc<dyn Fn(&[Rc<Extension>])>,
    on_did_never_show_recommended_extensions_again: Rc<dyn Fn(&[Rc<Extension>])>,
}

// ---------------------------------------------------------------------------------------------------------------------
// RecommendationsNotification
// ---------------------------------------------------------------------------------------------------------------------

/// `RecommendationsNotification`.
struct RecommendationsNotification {
    base: Rc<Disposable>,
    on_did_close: Rc<Emitter<()>>,
    on_did_change_visibility: Rc<Emitter<bool>>,
    notification_handle: RefCell<Option<Rc<dyn NotificationHandle>>>,
    cancelled: Rc<Cell<bool>>,
    on_did_close_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    on_did_change_visibility_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    severity: Severity,
    message: String,
    choices: Vec<PromptChoiceKind>,
    notification_service: Rc<dyn INotificationService>,
    this: Weak<RecommendationsNotification>,
}

impl RecommendationsNotification {
    fn new(severity: Severity, message: String, choices: Vec<PromptChoiceKind>, notification_service: Rc<dyn INotificationService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_close = Emitter::<()>::new();
        let on_did_change_visibility = Emitter::<bool>::new();
        base.register(on_did_close.clone());
        base.register(on_did_change_visibility.clone());
        let on_did_close_disposable = MutableDisposable::<dyn IDisposable>::new();
        let on_did_change_visibility_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(on_did_close_disposable.clone());
        base.register(on_did_change_visibility_disposable.clone());
        Rc::new_cyclic(|weak| Self {
            base,
            on_did_close,
            on_did_change_visibility,
            notification_handle: RefCell::new(None),
            cancelled: Rc::new(Cell::new(false)),
            on_did_close_disposable,
            on_did_change_visibility_disposable,
            severity,
            message,
            choices,
            notification_service,
            this: weak.clone(),
        })
    }

    fn prompt(&self, priority: NotificationPriority, sticky: bool) -> Rc<dyn NotificationHandle> {
        let cancelled = self.cancelled.clone();
        self.notification_service.prompt(
            self.severity,
            NotificationMessage::Text(self.message.clone()),
            self.choices.clone(),
            Some(PromptOptions {
                properties: NotificationProperties { sticky, priority: Some(priority), ..NotificationProperties::default() },
                on_cancel: Some(Rc::new(move || cancelled.set(true))),
            }),
        )
    }

    fn show(&self) {
        if self.notification_handle.borrow().is_none() {
            let handle = self.prompt(NotificationPriority::Optional, true);
            self.update_notification_handle(handle);
        }
    }

    fn hide(&self) {
        let handle = self.notification_handle.borrow().clone();
        if let Some(handle) = handle {
            self.on_did_close_disposable.clear();
            handle.close();
            self.cancelled.set(false);
            let handle = self.prompt(NotificationPriority::Silent, false);
            self.update_notification_handle(handle);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }

    fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    fn update_notification_handle(&self, notification_handle: Rc<dyn NotificationHandle>) {
        self.on_did_close_disposable.clear();
        self.on_did_change_visibility_disposable.clear();
        *self.notification_handle.borrow_mut() = Some(notification_handle.clone());

        let weak = self.this.clone();
        self.on_did_close_disposable.set(Some(notification_handle.on_did_close().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            this.on_did_close_disposable.dispose();
            this.on_did_change_visibility_disposable.dispose();

            this.on_did_close.fire(());

            this.on_did_close.dispose();
            this.on_did_change_visibility.dispose();
        })));
        let weak = self.this.clone();
        self.on_did_change_visibility_disposable.set(Some(notification_handle.on_did_change_visibility().listen(move |visible| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_visibility.fire(*visible);
            }
        })));
    }
}

impl IDisposable for RecommendationsNotification {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `PendingRecommendationsNotification`.
struct PendingRecommendationsNotification {
    recommendations_notification: Rc<RecommendationsNotification>,
    source: RecommendationSource,
    #[allow(dead_code)]
    token: CancellationToken,
}

/// `VisibleRecommendationsNotification`.
struct VisibleRecommendationsNotification {
    recommendations_notification: Rc<RecommendationsNotification>,
    source: RecommendationSource,
    from: f64,
}

// ---------------------------------------------------------------------------------------------------------------------
// ExtensionRecommendationNotificationService
// ---------------------------------------------------------------------------------------------------------------------

/// `ExtensionRecommendationNotificationService`.
pub struct ExtensionRecommendationNotificationService {
    this: Weak<ExtensionRecommendationNotificationService>,
    base: Rc<Disposable>,

    recommended_extensions: RefCell<Vec<String>>,
    recommendation_sources: RefCell<Vec<RecommendationSource>>,

    hide_visible_notification_promise: RefCell<Option<CancelablePromise<()>>>,
    visible_notification: RefCell<Option<VisibleRecommendationsNotification>>,
    pending_notifications: RefCell<Vec<PendingRecommendationsNotification>>,

    p: ExtensionRecommendationNotificationServiceParts,
}

impl ExtensionRecommendationNotificationService {
    /// `constructor(...)`.
    pub fn new(parts: ExtensionRecommendationNotificationServiceParts) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: Disposable::new(),
            recommended_extensions: RefCell::new(Vec::new()),
            recommendation_sources: RefCell::new(Vec::new()),
            hide_visible_notification_promise: RefCell::new(None),
            visible_notification: RefCell::new(None),
            pending_notifications: RefCell::new(Vec::new()),
            p: parts,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn publish_popup(&self, user_reaction: &str, extension_id: &str, source: RecommendationSource) {
        let data = Dynamic::object([
            ("userReaction", Dynamic::string(user_reaction)),
            ("extensionId", Dynamic::string(extension_id)),
            ("source", Dynamic::string(recommendation_source_to_string(source))),
        ]);
        self.p.telemetry_service.public_log2("extensionRecommendations:popup", Some(data));
    }

    fn publish_workspace_popup(&self, user_reaction: &str) {
        self.p.telemetry_service.public_log2("extensionWorkspaceRecommendations:popup", Some(Dynamic::object([("userReaction", Dynamic::string(user_reaction))])));
    }

    /// `promptRecommendationsNotification(...)`.
    fn prompt_recommendations_notification(&self, request: ExtensionRecommendationsRequest, actions: RecommendationsNotificationActions) -> Promise<RecommendationsNotificationResult> {
        let this = self.rc();
        async_fn(async move {
            let ExtensionRecommendationsRequest { source, extensions: extension_ids, name, search_value } = request;

            if this.has_to_ignore_recommendation_notifications() {
                return Ok(RecommendationsNotificationResult::Ignored);
            }

            // Do not show exe based recommendations in remote window.
            if source == RecommendationSource::Exe && this.p.workbench_environment_service.remote_authority().is_some_and(|authority| !authority.is_empty()) {
                return Ok(RecommendationsNotificationResult::IncompatibleWindow);
            }

            // Ignore exe recommendation if the window
            //      => has shown an exe based recommendation already
            //      => or has shown any two recommendations already
            if source == RecommendationSource::Exe && (this.recommendation_sources.borrow().contains(&RecommendationSource::Exe) || this.recommendation_sources.borrow().len() >= 2) {
                return Ok(RecommendationsNotificationResult::TooMany);
            }

            this.recommendation_sources.borrow_mut().push(source);

            // Ignore exe recommendation if recommendations are already shown.
            if source == RecommendationSource::Exe
                && extension_ids.iter().all(|id| id.as_gallery().is_some_and(|id| this.recommended_extensions.borrow().iter().any(|recommended| recommended == id)))
            {
                return Ok(RecommendationsNotificationResult::Ignored);
            }

            let extensions = this.get_installable_extensions(extension_ids.clone()).await?;
            if extensions.is_empty() {
                return Ok(RecommendationsNotificationResult::Ignored);
            }

            {
                let mut recommended = this.recommended_extensions.borrow_mut();
                for id in extension_ids.iter().filter_map(RecommendedExtension::as_gallery) {
                    if !recommended.iter().any(|existing| existing == id) {
                        recommended.push(id.to_owned());
                    }
                }
            }

            let extensions_message = if extensions.len() == 1 {
                localize("extensionFromPublisher", "'{0}' extension from {1}", &[&extensions[0].display_name(), &extensions[0].publisher_display_name()])
            } else {
                let mut publishers: Vec<String> = Vec::new();
                for extension in &extensions {
                    let publisher = extension.publisher_display_name();
                    if !publishers.contains(&publisher) {
                        publishers.push(publisher);
                    }
                }
                if publishers.len() > 2 {
                    localize("extensionsFromMultiplePublishers", "extensions from {0}, {1} and others", &[&publishers[0], &publishers[1]])
                } else if publishers.len() == 2 {
                    localize("extensionsFromPublishers", "extensions from {0} and {1}", &[&publishers[0], &publishers[1]])
                } else {
                    localize("extensionsFromPublisher", "extensions from {0}", &[&publishers[0]])
                }
            };

            let mut message = localize("recommended", "Do you want to install the recommended {0} for {1}?", &[&extensions_message, &name]);
            if source == RecommendationSource::Exe {
                message = localize(
                    "exeRecommended",
                    "You have {0} installed on your system. Do you want to install the recommended {1} for it?",
                    &[&name, &extensions_message],
                );
            }
            let search_value = match search_value.filter(|value| !value.is_empty()) {
                Some(value) => value,
                None if source == RecommendationSource::Workspace => "@recommended".to_owned(),
                None => extensions.iter().map(|extension| format!("@id:{}", extension.identifier().id)).collect::<Vec<_>>().join(" "),
            };

            let donot_show_again_label = if source == RecommendationSource::Workspace {
                localize("donotShowAgain", "Don't Show Again for this Repository", &[])
            } else if extensions.len() > 1 {
                localize("donotShowAgainExtension", "Don't Show Again for these Extensions", &[])
            } else {
                localize("donotShowAgainExtensionSingle", "Don't Show Again for this Extension", &[])
            };

            let shown = this.register_p(this.show_recommendations_notification(extensions.clone(), message, search_value, donot_show_again_label, source, actions));
            let installed = this.register_p(this.wait_until_recommendations_are_installed(extensions));
            let raced = race_cancellable_promises(vec![shown, installed]);
            this.register_p(raced.clone());
            raced.await
        })
    }

    /// `showRecommendationsNotification(...)`.
    fn show_recommendations_notification(
        &self,
        extensions: Vec<Rc<Extension>>,
        message: String,
        search_value: String,
        donot_show_again_label: String,
        source: RecommendationSource,
        actions: RecommendationsNotificationActions,
    ) -> CancelablePromise<RecommendationsNotificationResult> {
        let this = self.rc();
        create_cancelable_promise(move |token| {
            async_fn(async move {
                let mut accepted = false;
                let install_extensions: Rc<dyn Fn(bool)> = {
                    let this = this.clone();
                    let extensions = extensions.clone();
                    let search_value = search_value.clone();
                    let actions = actions.clone();
                    Rc::new(move |is_machine_scoped: bool| {
                        this.p.extensions_opener.open_search(&search_value);
                        (actions.on_did_install_recommended_extensions)(&extensions);
                        let mut gallery_extensions = Vec::new();
                        let mut resource_extensions: Vec<Rc<Extension>> = Vec::new();
                        for extension in &extensions {
                            if let Some(gallery) = extension.gallery() {
                                gallery_extensions.push(InstallExtensionInfo {
                                    extension: (*gallery).clone(),
                                    options: InstallOptions { is_machine_scoped: Some(is_machine_scoped), ..InstallOptions::default() },
                                });
                            } else if extension.resource_extension().is_some() {
                                resource_extensions.push(extension.clone());
                            }
                        }
                        let opened: Vec<Promise<()>> = extensions.iter().map(|extension| this.p.extensions_opener.open(extension, true)).collect();
                        let installed_gallery: Promise<()> = if gallery_extensions.is_empty() {
                            Promise::resolved(())
                        } else {
                            this.p.extension_management_service.install_gallery_extensions(gallery_extensions).then(|_| ())
                        };
                        let installed_resources: Vec<Promise<()>> = resource_extensions
                            .iter()
                            .map(|extension| this.p.extensions_workbench_service.install(InstallArg::Extension(extension.clone()), InstallExtensionOptions::default(), None).then(|_| ()))
                            .collect();
                        let _ = Promise::all_settled(opened).then(|_| ());
                        let _ = installed_gallery.then(|_| ());
                        let _ = Promise::all_settled(installed_resources).then(|_| ());
                    })
                };

                let mut choices: Vec<PromptChoiceKind> = Vec::new();
                let user_data_sync = &this.p.user_data_sync_enablement_service;
                let menu = if user_data_sync.is_enabled() && user_data_sync.is_resource_enabled(SyncResource::Extensions, None) {
                    let install_extensions = install_extensions.clone();
                    vec![BasePromptChoice {
                        telemetry_id: None,
                        label: localize("install and do no sync", "Install (Do not sync)", &[]),
                        keep_open: false,
                        run: Rc::new(move || install_extensions(true)),
                    }]
                } else {
                    Vec::new()
                };
                let install_base = BasePromptChoice {
                    telemetry_id: None,
                    label: localize("install", "Install", &[]),
                    keep_open: false,
                    run: {
                        let install_extensions = install_extensions.clone();
                        Rc::new(move || install_extensions(false))
                    },
                };
                // `menu: undefined` é um `IPromptChoice` simples.
                choices.push(if menu.is_empty() {
                    PromptChoiceKind::Plain(PromptChoice { base: install_base, is_secondary: false })
                } else {
                    PromptChoiceKind::WithMenu(PromptChoiceWithMenu { base: install_base, menu })
                });
                choices.push(PromptChoiceKind::Plain(PromptChoice {
                    base: BasePromptChoice {
                        telemetry_id: None,
                        label: localize("show recommendations", "Show Recommendations", &[]),
                        keep_open: false,
                        run: {
                            let this = this.clone();
                            let extensions = extensions.clone();
                            let search_value = search_value.clone();
                            let actions = actions.clone();
                            Rc::new(move || {
                                (actions.on_did_show_recommended_extensions)(&extensions);
                                for extension in &extensions {
                                    let _ = this.p.extensions_opener.open(extension, true);
                                }
                                this.p.extensions_opener.open_search(&search_value);
                            })
                        },
                    },
                    is_secondary: false,
                }));
                choices.push(PromptChoiceKind::Plain(PromptChoice {
                    base: BasePromptChoice {
                        telemetry_id: None,
                        label: donot_show_again_label,
                        keep_open: false,
                        run: {
                            let extensions = extensions.clone();
                            let actions = actions.clone();
                            Rc::new(move || (actions.on_did_never_show_recommended_extensions_again)(&extensions))
                        },
                    },
                    is_secondary: true,
                }));

                match this.do_show_recommendations_notification(Severity::Info, message, choices, source, token).await {
                    Ok(value) => accepted = value,
                    Err(error) => {
                        if !is_cancellation_error(&error) {
                            return Err(error);
                        }
                    }
                }

                if accepted {
                    Ok(RecommendationsNotificationResult::Accepted)
                } else {
                    (actions.on_did_cancel_recommended_extensions)(&extensions);
                    Ok(RecommendationsNotificationResult::Cancelled)
                }
            })
        })
    }

    /// `waitUntilRecommendationsAreInstalled(extensions)`.
    fn wait_until_recommendations_are_installed(&self, extensions: Vec<Rc<Extension>>) -> CancelablePromise<RecommendationsNotificationResult> {
        let installed_extensions: Rc<RefCell<Vec<String>>> = Rc::default();
        let disposables = DisposableStore::new();
        let management = self.p.extension_management_service.clone();
        create_cancelable_promise(move |token| {
            let on_cancel_disposables = disposables.clone();
            disposables.add(token.on_cancellation_requested().listen(move |_| on_cancel_disposables.dispose()));
            Promise::with_executor(move |resolver| {
                disposables.add(management.on_install_extension().listen(move |event| {
                    installed_extensions.borrow_mut().push(event.identifier.id.to_lowercase());
                    let installed = installed_extensions.borrow();
                    if extensions.iter().all(|extension| installed.contains(&extension.identifier().id.to_lowercase())) {
                        resolver.resolve(RecommendationsNotificationResult::Accepted);
                    }
                }));
            })
        })
    }

    /// Show recommendations in Queue.
    /// At any time only one recommendation is shown.
    /// If a new recommendation comes in
    ///      => If no recommendation is visible, show it immediately
    ///      => Otherwise, add to the pending queue
    ///          => If it is not exe based and has higher or same priority as current, hide the current notification after showing it for 3s.
    ///          => Otherwise wait until the current notification is hidden.
    fn do_show_recommendations_notification(&self, severity: Severity, message: String, choices: Vec<PromptChoiceKind>, source: RecommendationSource, token: CancellationToken) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            let disposables = DisposableStore::new();
            let attempt = async {
                let recommendations_notification = RecommendationsNotification::new(severity, message, choices, this.p.notification_service.clone());
                disposables.add(recommendations_notification.clone());
                {
                    let weak = Rc::downgrade(&this);
                    disposables.add(recommendations_notification.on_did_change_visibility().filter(|visible| !*visible, None).once().listen(move |_| {
                        if let Some(this) = weak.upgrade() {
                            this.show_next_notification();
                        }
                    }));
                }
                let has_visible = this.visible_notification.borrow().is_some();
                if has_visible {
                    let index = this.pending_notifications.borrow().len();
                    {
                        let weak = Rc::downgrade(&this);
                        disposables.add(token.on_cancellation_requested().listen(move |_| {
                            if let Some(this) = weak.upgrade() {
                                let mut pending = this.pending_notifications.borrow_mut();
                                if index < pending.len() {
                                    pending.remove(index);
                                }
                            }
                        }));
                    }
                    this.pending_notifications.borrow_mut().push(PendingRecommendationsNotification {
                        recommendations_notification: recommendations_notification.clone(),
                        source,
                        token: token.clone(),
                    });
                    let visible_source = this.visible_notification.borrow().as_ref().map(|visible| visible.source);
                    if source != RecommendationSource::Exe && visible_source.is_some_and(|visible_source| source <= visible_source) {
                        this.hide_visible_notification(3000);
                    }
                } else {
                    *this.visible_notification.borrow_mut() =
                        Some(VisibleRecommendationsNotification { recommendations_notification: recommendations_notification.clone(), source, from: je_base::runtime::date_now() as f64 });
                    recommendations_notification.show();
                }
                let closed: Promise<()> = {
                    let disposables = disposables.clone();
                    let notification = recommendations_notification.clone();
                    Promise::with_executor(move |resolver| {
                        disposables.add(notification.on_did_close().once().listen(move |_| resolver.resolve(())));
                    })
                };
                race_cancellation(&closed, &token).await?;
                Ok::<bool, je_base::errors::Error>(!recommendations_notification.is_cancelled())
            };
            let result = attempt.await;
            disposables.dispose();
            result
        })
    }

    /// `showNextNotification()`.
    fn show_next_notification(&self) {
        let index = self.get_next_pending_notification_index();
        let next_notification = index.filter(|index| *index < self.pending_notifications.borrow().len()).map(|index| self.pending_notifications.borrow_mut().remove(index));

        // Show the next notification after a delay of 500ms (after the current notification is dismissed).
        let this = self.rc();
        let _ = timeout(if next_notification.is_some() { 500 } else { 0 }).then(move |_| {
            this.unset_visible_notification();
            if let Some(next_notification) = next_notification {
                *this.visible_notification.borrow_mut() = Some(VisibleRecommendationsNotification {
                    recommendations_notification: next_notification.recommendations_notification.clone(),
                    source: next_notification.source,
                    from: je_base::runtime::date_now() as f64,
                });
                next_notification.recommendations_notification.show();
            }
        });
    }

    /// Return the recent high priority pending notification.
    fn get_next_pending_notification_index(&self) -> Option<usize> {
        let pending = self.pending_notifications.borrow();
        if pending.is_empty() {
            return None;
        }
        let mut index = pending.len() - 1;
        for i in 0..pending.len() {
            if pending[i].source <= pending[index].source {
                index = i;
            }
        }
        Some(index)
    }

    /// `hideVisibleNotification(timeInMillis)`.
    fn hide_visible_notification(&self, time_in_millis: u64) {
        if self.hide_visible_notification_promise.borrow().is_some() {
            return;
        }
        let visible = self.visible_notification.borrow().as_ref().map(|visible| (visible.recommendations_notification.clone(), visible.from));
        if let Some((recommendations_notification, from)) = visible {
            let elapsed = je_base::runtime::date_now() as f64 - from;
            let wait = (time_in_millis as f64 - elapsed).max(0.0) as u64;
            let promise = timeout(wait);
            *self.hide_visible_notification_promise.borrow_mut() = Some(promise.clone());
            let _ = promise.then(move |_| recommendations_notification.hide());
        }
    }

    /// `unsetVisibileNotification()`.
    fn unset_visible_notification(&self) {
        let promise = self.hide_visible_notification_promise.borrow_mut().take();
        if let Some(promise) = promise {
            promise.cancel();
        }
        *self.visible_notification.borrow_mut() = None;
    }

    /// `getInstallableExtensions(recommendations)`.
    fn get_installable_extensions(&self, recommendations: Vec<RecommendedExtension>) -> Promise<Vec<Rc<Extension>>> {
        let this = self.rc();
        async_fn(async move {
            let mut result: Vec<Rc<Extension>> = Vec::new();
            if !recommendations.is_empty() {
                let mut gallery_extensions: Vec<String> = Vec::new();
                let mut resource_extensions = Vec::new();
                for recommendation in recommendations {
                    match recommendation {
                        RecommendedExtension::Gallery(id) => gallery_extensions.push(id),
                        RecommendedExtension::Resource(uri) => resource_extensions.push(uri),
                    }
                }
                if !gallery_extensions.is_empty() {
                    let infos = gallery_extensions.into_iter().map(|id| ExtensionInfo { id, ..ExtensionInfo::default() }).collect();
                    let options = ExtensionQueryOptions { source: Some("install-recommendations".to_owned()), ..ExtensionQueryOptions::default() };
                    let extensions = this.p.extensions_workbench_service.get_extensions(infos, options, CancellationToken::None).await?;
                    for extension in extensions {
                        if let Some(gallery) = extension.gallery() {
                            if this.p.extension_management_service.can_install(&gallery).await?.is_ok() {
                                result.push(extension);
                            }
                        }
                    }
                }
                if !resource_extensions.is_empty() {
                    let extensions = this.p.extensions_workbench_service.get_resource_extensions(resource_extensions, true).await?;
                    for extension in extensions {
                        if this.p.extensions_workbench_service.can_install(&extension).await?.is_ok() {
                            result.push(extension);
                        }
                    }
                }
            }
            Ok(result)
        })
    }

    /// `addToImportantRecommendationsIgnore(id)`.
    fn add_to_important_recommendations_ignore(&self, id: &str) {
        let mut important_recommendations_ignore_list = self.ignored_recommendations();
        if !important_recommendations_ignore_list.contains(&id.to_lowercase()) {
            important_recommendations_ignore_list.push(id.to_lowercase());
            let serialized = serde_json::to_string(&important_recommendations_ignore_list).unwrap_or_else(|_| "[]".to_owned());
            self.p.storage_service.store(IGNORE_IMPORTANT_EXTENSION_RECOMMENDATION_STORAGE_KEY, serialized.as_str().into(), StorageScope::Profile, StorageTarget::User);
        }
    }

    /// `setIgnoreRecommendationsConfig(configVal)`.
    fn set_ignore_recommendations_config(&self, config_val: bool) {
        let _ = self.p.configuration_service.update_value("extensions.ignoreRecommendations", Dynamic::Bool(config_val), None, None);
    }

    /// `_registerP(o)`: cancela a promessa quando o serviço é descartado.
    fn register_p<T: Clone + 'static>(&self, promise: CancelablePromise<T>) -> CancelablePromise<T> {
        let to_cancel = promise.clone();
        self.base.register(to_disposable(move || to_cancel.cancel()));
        promise
    }

    fn actions_for_important_install(&self, source: RecommendationSource) -> RecommendationsNotificationActions {
        let on_popup = |reaction: &'static str| -> Rc<dyn Fn(&[Rc<Extension>])> {
            let this = self.rc();
            Rc::new(move |extensions: &[Rc<Extension>]| extensions.iter().for_each(|extension| this.publish_popup(reaction, &extension.identifier().id, source)))
        };
        let never_show_again: Rc<dyn Fn(&[Rc<Extension>])> = {
            let this = self.rc();
            Rc::new(move |extensions: &[Rc<Extension>]| {
                for extension in extensions {
                    this.add_to_important_recommendations_ignore(&extension.identifier().id);
                    this.publish_popup("neverShowAgain", &extension.identifier().id, source);
                }
                let yes = this.clone();
                let no = this.clone();
                this.p.notification_service.prompt(
                    Severity::Info,
                    NotificationMessage::Text(localize("ignoreExtensionRecommendations", "Do you want to ignore all extension recommendations?", &[])),
                    vec![
                        PromptChoiceKind::Plain(PromptChoice {
                            base: BasePromptChoice {
                                telemetry_id: None,
                                label: localize("ignoreAll", "Yes, Ignore All", &[]),
                                keep_open: false,
                                run: Rc::new(move || yes.set_ignore_recommendations_config(true)),
                            },
                            is_secondary: false,
                        }),
                        PromptChoiceKind::Plain(PromptChoice {
                            base: BasePromptChoice { telemetry_id: None, label: localize("no", "No", &[]), keep_open: false, run: Rc::new(move || no.set_ignore_recommendations_config(false)) },
                            is_secondary: false,
                        }),
                    ],
                    None,
                );
            })
        };
        RecommendationsNotificationActions {
            on_did_install_recommended_extensions: on_popup("install"),
            on_did_show_recommended_extensions: on_popup("show"),
            on_did_cancel_recommended_extensions: on_popup("cancelled"),
            on_did_never_show_recommended_extensions_again: never_show_again,
        }
    }

    fn actions_for_workspace(&self) -> RecommendationsNotificationActions {
        let on_popup = |reaction: &'static str| -> Rc<dyn Fn(&[Rc<Extension>])> {
            let this = self.rc();
            Rc::new(move |_: &[Rc<Extension>]| this.publish_workspace_popup(reaction))
        };
        let this = self.rc();
        RecommendationsNotificationActions {
            on_did_install_recommended_extensions: on_popup("install"),
            on_did_show_recommended_extensions: on_popup("show"),
            on_did_cancel_recommended_extensions: on_popup("cancelled"),
            on_did_never_show_recommended_extensions_again: Rc::new(move |_: &[Rc<Extension>]| {
                this.publish_workspace_popup("neverShowAgain");
                this.p.storage_service.store(DONOT_SHOW_WORKSPACE_RECOMMENDATIONS_STORAGE_KEY, true.into(), StorageScope::Workspace, StorageTarget::Machine);
            }),
        }
    }
}

impl IExtensionRecommendationNotificationService for ExtensionRecommendationNotificationService {
    /// Ignored Important Recommendations.
    fn ignored_recommendations(&self) -> Vec<String> {
        let stored = self.p.storage_service.get(IGNORE_IMPORTANT_EXTENSION_RECOMMENDATION_STORAGE_KEY, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        let ids: Vec<String> = serde_json::from_str(&stored).unwrap_or_default();
        let mut result: Vec<String> = Vec::new();
        for id in ids.into_iter().map(|id| id.to_lowercase()) {
            if !result.contains(&id) {
                result.push(id);
            }
        }
        result
    }

    fn has_to_ignore_recommendation_notifications(&self) -> bool {
        let config = self.p.configuration_service.get_value(Some("extensions"), None);
        use je_base::dynamic::Truthy;
        config.get_str("ignoreRecommendations").is_truthy() || config.get_str("showRecommendationsOnlyOnDemand").is_truthy()
    }

    fn prompt_important_extensions_install_notification(&self, extension_recommendations: ExtensionRecommendationsRequest) -> Promise<RecommendationsNotificationResult> {
        let ignored_recommendations: Vec<String> =
            self.p.extension_ignored_recommendations_service.ignored_recommendations().into_iter().chain(self.ignored_recommendations()).collect();
        let extensions: Vec<RecommendedExtension> = extension_recommendations
            .extensions
            .iter()
            .filter(|extension| extension.as_gallery().is_none_or(|id| !ignored_recommendations.iter().any(|ignored| ignored == id)))
            .cloned()
            .collect();
        if extensions.is_empty() {
            return Promise::resolved(RecommendationsNotificationResult::Ignored);
        }
        let source = extension_recommendations.source;
        let actions = self.actions_for_important_install(source);
        self.prompt_recommendations_notification(ExtensionRecommendationsRequest { extensions, ..extension_recommendations }, actions)
    }

    fn prompt_workspace_recommendations(&self, recommendations: Vec<RecommendedExtension>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if this.p.storage_service.get_boolean(DONOT_SHOW_WORKSPACE_RECOMMENDATIONS_STORAGE_KEY, StorageScope::Workspace, Some(false)).unwrap_or(false) {
                return Ok(());
            }

            let installed = this.p.extension_management_service.get_installed().await?;
            let installed: Vec<_> =
                installed.into_iter().filter(|local| this.p.extension_enablement_service.get_enablement_state(local) != EnablementState::DisabledByExtensionKind).collect();
            let ext_uri = this.p.uri_identity_service.ext_uri();
            let recommendations: Vec<RecommendedExtension> = recommendations
                .into_iter()
                .filter(|recommendation| {
                    installed.iter().all(|local| match recommendation {
                        RecommendedExtension::Gallery(id) => !are_same_extensions(&ExtensionIdentifierInfo::new(id.clone()), &local.identifier),
                        RecommendedExtension::Resource(uri) => !ext_uri.is_equal(Some(uri), Some(&local_extension_uri(local)), false),
                    })
                })
                .collect();
            if recommendations.is_empty() {
                return Ok(());
            }

            let actions = this.actions_for_workspace();
            this.prompt_recommendations_notification(
                ExtensionRecommendationsRequest {
                    extensions: recommendations,
                    source: RecommendationSource::Workspace,
                    name: localize("this repository", "this repository", &[]),
                    search_value: None,
                },
                actions,
            )
            .await?;
            Ok(())
        })
    }
}

impl IDisposable for ExtensionRecommendationNotificationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
