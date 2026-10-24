//! Porte de `src/vs/workbench/api/browser/mainThreadMessageService.ts`: a ponte entre as mensagens que as extensões
//! mostram (`showInformationMessage` e afins) e os serviços de notificação e de diálogo do workbench.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadMessageService)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda e o original não usa o contexto (os testes passam `null!`), então o construtor não o
//!   recebe.
//! - O `extensionService.onDidChangeExtensions` é o [`IExtensionService::on_did_change_extensions`], que o
//!   `extension.identifier.value` das extensões removidas alimenta.
//! - `$showMessage` é `rpc_show_message` (o nome do contrato [`MainThreadMessageServiceShape`]); `handle: number` é
//!   `f64`, como no DTO, e a promessa que `new Promise(resolve => ...)` cria é um [`Promise::with_executor`] cujo
//!   `resolve` é copiado para cada fecho.
//! - O `T` dos botões do diálogo (`IPromptButton<number>`) é [`Dynamic`]: o `handle` entra como `Dynamic::Number` e o
//!   resultado volta a `Option<f64>`.
//! - `nls.localize({ key: 'ok', comment: [...] }, "&&OK")` é `localize("ok", "&&OK")`.

use std::rc::Rc;

use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_platform::commands::commands::ICommandService;
use je_platform::dialogs::dialogs::{
    BaseDialogOptions, CheckboxResult, CustomDialog, DialogDetail, DialogTypeOrSeverity, IDialogService, Prompt, PromptBaseButton, PromptButton, PromptCancel, PromptCancelButton,
};
use je_platform::notification::notification_legacy::LEGACY_EXTENSION_LINK_PARSING;
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetry, extension_notification_telemetry, with_notification_action_telemetry,
};
use je_platform::notification::{INotificationService, Notification, NotificationActions, NotificationPriority, NotificationProperties, NotificationSource, NotificationSourceOrText};

use crate::api::common::ext_host_protocol::{MainThreadMessageOptions, MainThreadMessageServiceShape, MessageCommandDto};
use crate::services::extensions::extensions::IExtensionService;

/// `MainThreadMessageService`.
pub struct MainThreadMessageService {
    extensions_listener: DisposableRef,
    notification_service: Rc<dyn INotificationService>,
    command_service: Rc<dyn ICommandService>,
    dialog_service: Rc<dyn IDialogService>,
}

impl MainThreadMessageService {
    const URGENT_NOTIFICATION_SOURCES: [&'static str; 2] = ["vscode.github-authentication", "vscode.microsoft-authentication"];

    pub fn new(
        notification_service: Rc<dyn INotificationService>,
        command_service: Rc<dyn ICommandService>,
        dialog_service: Rc<dyn IDialogService>,
        extension_service: Rc<dyn IExtensionService>,
    ) -> Rc<Self> {
        let removal_target = notification_service.clone();
        let extensions_listener = extension_service.on_did_change_extensions().listen(move |e| {
            for extension in &e.removed {
                removal_target.remove_filter(&extension.identifier.value);
            }
        });
        Rc::new(Self { extensions_listener, notification_service, command_service, dialog_service })
    }

    /// `_showMessage`.
    fn show_message(&self, severity: Severity, message: &str, commands: Vec<MessageCommandDto>, options: &MainThreadMessageOptions) -> Promise<Option<f64>> {
        let notification_service = self.notification_service.clone();
        let command_service = self.command_service.clone();
        let message = message.to_owned();
        let options = options.clone();

        Promise::with_executor(move |resolver| {
            let primary_actions: Vec<ActionRef> = commands
                .iter()
                .map(|command| {
                    let (resolver, handle) = (resolver.clone(), command.handle);
                    with_notification_action_telemetry(
                        to_action(ToActionProps {
                            id: format!("_extension_message_handle_{}", number_to_text(handle)),
                            label: command.title.clone(),
                            tooltip: None,
                            enabled: Some(true),
                            checked: None,
                            class: None,
                            alt: None,
                            run: Rc::new(move |_| {
                                resolver.resolve(Some(handle));
                                MaybePromise::Promise(Promise::resolved(Dynamic::Undefined))
                            }),
                        }),
                        Some(NotificationActionTelemetry::ExtensionButtonIndex(handle as i64)),
                    )
                })
                .collect();

            let mut source: Option<NotificationSourceOrText> = None;
            let mut source_is_urgent = false;
            if let Some(option_source) = &options.source {
                let source_id = option_source.identifier.value.clone();
                source_is_urgent = Self::URGENT_NOTIFICATION_SOURCES.contains(&source_id.as_str());
                source = Some(NotificationSourceOrText::Source(NotificationSource { label: option_source.label.clone(), id: source_id }));
            }

            let source = source.unwrap_or_else(|| NotificationSourceOrText::Text(localize("defaultSource", "Extension", &[])));

            let mut secondary_actions: Vec<ActionRef> = Vec::new();
            if let Some(option_source) = &options.source {
                let extension_id = option_source.identifier.value.clone();
                secondary_actions.push(with_notification_action_telemetry(
                    to_action(ToActionProps {
                        id: extension_id.clone(),
                        label: localize("manageExtension", "Manage Extension", &[]),
                        tooltip: None,
                        enabled: None,
                        checked: None,
                        class: None,
                        alt: None,
                        run: Rc::new(move |_| MaybePromise::Promise(command_service.execute_command("_extensions.manage", vec![Dynamic::from(extension_id.as_str())]))),
                    }),
                    Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::ManageExtension)),
                ));
            }

            let message_handle = notification_service.notify(Notification {
                properties: NotificationProperties {
                    telemetry: Some(NotificationTelemetry::Extension(extension_notification_telemetry(options.source.as_ref().map(|source| source.identifier.value.as_str()), "message"))),
                    legacy_extension_link_parsing: Some(LEGACY_EXTENSION_LINK_PARSING),
                    sticky: source_is_urgent,
                    priority: Some(if source_is_urgent { NotificationPriority::Urgent } else { NotificationPriority::Default }),
                    never_show_again: None,
                },
                id: None,
                severity,
                message: message.into(),
                source: Some(source),
                actions: Some(NotificationActions { primary: primary_actions, secondary: secondary_actions }),
                progress: None,
            });

            // if promise has not been resolved yet, now is the time to ensure a return value
            // otherwise if already resolved it means the user clicked one of the buttons
            message_handle.on_did_close().once().listen(move |_| {
                resolver.resolve(None);
            });
        })
    }

    /// `_showModalMessage`.
    fn show_modal_message(&self, severity: Severity, message: &str, detail: Option<&str>, commands: &[MessageCommandDto], use_custom: Option<bool>) -> Promise<Option<f64>> {
        let mut buttons: Vec<PromptButton> = Vec::new();
        let mut cancel_button: Option<PromptCancelButton> = None;

        for command in commands {
            let handle = command.handle;
            let run: Rc<dyn Fn(CheckboxResult) -> MaybePromise<Dynamic>> = Rc::new(move |_| MaybePromise::Value(Dynamic::Number(handle)));

            if command.is_close_affordance {
                cancel_button = Some(PromptCancelButton { label: Some(command.title.clone()), base: PromptBaseButton { run } });
            } else {
                buttons.push(PromptButton { label: command.title.clone(), base: PromptBaseButton { run } });
            }
        }

        let cancel_button = cancel_button.unwrap_or_else(|| {
            let label = if !buttons.is_empty() { localize("cancel", "Cancel", &[]) } else { localize("ok", "&&OK", &[]) };
            PromptCancelButton { label: Some(label), base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(Dynamic::Undefined)) } }
        });

        let mut base = BaseDialogOptions::new(message);
        base.dialog_type = Some(DialogTypeOrSeverity::Severity(severity));
        base.detail = detail.map(|detail| DialogDetail::Text(detail.to_owned()));
        base.custom = use_custom.map(CustomDialog::Flag);

        self.dialog_service
            .prompt(Prompt { base, buttons: Some(buttons), cancel_button: Some(PromptCancel::Button(cancel_button)) })
            .then(|prompt_result| match prompt_result.result {
                Some(Dynamic::Number(handle)) => Some(handle),
                _ => None,
            })
    }
}

/// O `${command.handle}` do JavaScript: um número inteiro sai sem a parte decimal.
fn number_to_text(value: f64) -> String {
    je_base::dynamic::number_to_js_string(value)
}

impl IDisposable for MainThreadMessageService {
    fn dispose(&self) {
        self.extensions_listener.dispose();
    }
}

impl MainThreadMessageServiceShape for MainThreadMessageService {
    fn rpc_show_message(&self, severity: Severity, message: &str, options: MainThreadMessageOptions, commands: Vec<MessageCommandDto>) -> Promise<Option<f64>> {
        if options.modal.unwrap_or(false) {
            self.show_modal_message(severity, message, options.detail.as_deref(), &commands, options.use_custom)
        } else {
            self.show_message(severity, message, commands, &options)
        }
    }
}
