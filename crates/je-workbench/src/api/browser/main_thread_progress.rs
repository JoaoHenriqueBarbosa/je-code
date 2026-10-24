//! Porte de `src/vs/workbench/api/browser/mainThreadProgress.ts`: a ponte entre o `IProgressService` do workbench e o
//! progresso que as extensões abrem no host.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadProgress)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostProgress`.
//! - `withProgress<R>` é `Promise<()>` (ver `je_platform::progress::progress`), e o `try/catch` em volta dele é
//!   `errors::catch`: o pânico com um `Error` (a opção inválida que o serviço lança de forma síncrona) vai para
//!   `onUnexpectedExternalError`.
//! - O `{ ...options, legacyExtensionLinkParsing, telemetry }` é a montagem de um `ProgressOptions` a partir dos
//!   campos do DTO (que não tem `legacyExtensionLinkParsing` nem `telemetry`); `title` e `message`, que no DTO são
//!   texto, entram como [`NotificationPart::Text`].
//! - O `handle: number` é `u32`; o `extensionId?: string` é `Option<&str>`.
//! - O `Map` de progressos fica num `Rc<RefCell<HashMap>>`, porque a tarefa que o serviço chama o captura.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::actions::{to_action, ToActionProps};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, on_unexpected_external_error};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise, Resolver};
use je_platform::commands::commands::ICommandService;
use je_platform::notification::notification::NotificationPriority;
use je_platform::notification::notification_legacy::LEGACY_EXTENSION_LINK_PARSING;
use je_platform::notification::notification_message::NotificationPart;
use je_platform::notification::notification_telemetry::{
    extension_notification_telemetry, with_notification_action_telemetry, NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetry,
};
use je_platform::progress::progress::{IProgress, IProgressService, ProgressLocation, ProgressLocationId, ProgressOptions, ProgressStep, ProgressTask};

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostProgressShape, IMainContext, IProgressOptionsDto, IProgressStepDto, MainThreadProgressShape,
};

/// O valor do mapa `_progress`: `{ resolve, progress }`.
struct ProgressEntry {
    resolve: Resolver<()>,
    progress: Rc<dyn IProgress<ProgressStep>>,
}

type ProgressEntries = Rc<RefCell<HashMap<u32, ProgressEntry>>>;

/// `MainThreadProgress`.
pub struct MainThreadProgress {
    progress_service: Rc<dyn IProgressService>,
    progress: ProgressEntries,
    proxy: Rc<dyn ExtHostProgressShape>,
    command_service: Rc<dyn ICommandService>,
}

impl MainThreadProgress {
    const URGENT_PROGRESS_SOURCES: [&'static str; 2] = ["vscode.github-authentication", "vscode.microsoft-authentication"];

    pub fn new(ext_host_context: &dyn IMainContext, progress_service: Rc<dyn IProgressService>, command_service: Rc<dyn ICommandService>) -> Rc<Self> {
        Rc::new(Self {
            progress_service,
            progress: Rc::default(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_PROGRESS),
            command_service,
        })
    }

    /// `_createTask`.
    fn create_task(&self, handle: u32) -> ProgressTask {
        let entries = self.progress.clone();
        Box::new(move |progress| {
            Promise::with_executor(move |resolve| {
                entries.borrow_mut().insert(handle, ProgressEntry { resolve, progress });
            })
        })
    }
}

impl IDisposable for MainThreadProgress {
    fn dispose(&self) {
        let entries: Vec<ProgressEntry> = self.progress.borrow_mut().drain().map(|(_, entry)| entry).collect();
        for entry in entries {
            entry.resolve.resolve(());
        }
    }
}

impl MainThreadProgressShape for MainThreadProgress {
    fn rpc_start_progress(&self, handle: u32, options: IProgressOptionsDto, extension_id: Option<&str>) -> Promise<()> {
        let task = self.create_task(handle);
        let telemetry = match options.telemetry_id {
            Some(telemetry_id) => NotificationTelemetry::Id(telemetry_id),
            None => NotificationTelemetry::Extension(extension_notification_telemetry(extension_id, "progress")),
        };
        let mut progress_options = ProgressOptions::new(options.location.clone());
        progress_options.title = options.title.map(NotificationPart::Text);
        progress_options.source = options.source;
        progress_options.total = options.total;
        progress_options.cancellable = options.cancellable;
        progress_options.buttons = options.buttons;
        progress_options.legacy_extension_link_parsing = Some(LEGACY_EXTENSION_LINK_PARSING);
        progress_options.telemetry = Some(telemetry);

        if let (ProgressLocationId::Location(ProgressLocation::Notification), Some(extension_id)) = (&options.location, extension_id) {
            let source_is_urgent = Self::URGENT_PROGRESS_SOURCES.iter().any(|source| *source == extension_id);
            let command_service = self.command_service.clone();
            let manage_extension_id = extension_id.to_owned();
            let manage_extension = to_action(ToActionProps {
                id: extension_id.to_owned(),
                label: localize("manageExtension", "Manage Extension", &[]),
                tooltip: None,
                enabled: None,
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    MaybePromise::Promise(command_service.execute_command("_extensions.manage", vec![Dynamic::string(&manage_extension_id)]))
                }),
            });
            progress_options.priority = Some(if source_is_urgent { NotificationPriority::Urgent } else { NotificationPriority::Default });
            progress_options.location = ProgressLocationId::Location(ProgressLocation::Notification);
            progress_options.secondary_actions = Some(vec![with_notification_action_telemetry(
                manage_extension,
                Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::ManageExtension)),
            )]);
        }

        let progress_service = self.progress_service.clone();
        let proxy = self.proxy.clone();
        let started = errors::catch(move || {
            progress_service.with_progress(progress_options, task, Some(Rc::new(move |_choice| proxy.rpc_accept_progress_canceled(handle))))
        });
        if let Err(error) = started {
            // the withProgress-method will throw synchronously when invoked with bad options
            // which is then an enternal/extension error
            on_unexpected_external_error(&error);
        }
        Promise::resolved(())
    }

    fn rpc_progress_report(&self, handle: u32, message: IProgressStepDto) {
        let progress = self.progress.borrow().get(&handle).map(|entry| entry.progress.clone());
        if let Some(progress) = progress {
            progress.report(ProgressStep { message: message.message.map(NotificationPart::Text), increment: message.increment, total: message.total });
        }
    }

    fn rpc_progress_end(&self, handle: u32) {
        let entry = self.progress.borrow_mut().remove(&handle);
        if let Some(entry) = entry {
            entry.resolve.resolve(());
        }
    }
}
