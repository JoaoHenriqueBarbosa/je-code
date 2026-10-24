//! Teste de ponta a ponta de `vscode.window` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host local
//! (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), mostra mensagens e cria itens de barra de status, e o efeito
//! aparece nos serviços do workbench.
//!
//! - **Mensagens**: `showInformationMessage`, `showErrorMessage` e `showWarningMessage` passam pelo `MainThreadMessageService` REAL. A
//!   modal chega ao `IDialogService` (o dublê grava a pergunta e aperta o primeiro botão, e a promessa da extensão devolve o item
//!   escolhido); a não modal chega ao `INotificationService` (o dublê grava a notificação).
//! - **Barra de status**: `createStatusBarItem` e `setStatusBarMessage` passam pelo `MainThreadStatusBar` REAL até um
//!   `IExtensionStatusBarItemService` em memória, que mostra o item (texto, alinhamento, prioridade, tooltip, comando, nome), a troca do
//!   texto por atribuição (`item.text = ...`, um acessor de escrita nativo) e a remoção por `dispose()`.
//!
//! Dublês: `RecordingNotificationService`, `RecordingDialogService` e `InMemoryStatusBar` (os serviços do workbench que o teste observa).

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_editor::languages::StringOrMarkdown;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::dialogs::dialogs::{Confirmation, ConfirmationResult, DialogDetail, DialogTypeOrSeverity, IDialogService, Input, InputResult, Prompt, PromptCancel, PromptResult};
use je_platform::dialogs::dialogs::CheckboxResult;
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, PromptChoiceKind, PromptOptions, StatusHandle, StatusMessageOptions,
};
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::api::browser::main_thread_status_bar::{
    ExtensionStatusBarEntry, ExtensionStatusBarItem, ExtensionStatusBarItemChangeEvent, IExtensionStatusBarItemService, MainThreadStatusBar, StatusBarTooltip, StatusBarUpdateKind,
};
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadMessageServiceShape, MainThreadStatusBarShape, ICommandDto, StatusBarColor};
use je_workbench::services::statusbar::statusbar::StatusbarAlignment;
use je_base::themables::ThemeColor;
use je_platform::accessibility::IAccessibilityInformation;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  command('t.ask', async () => {
    const choice = await vscode.window.showInformationMessage('Salvar?', { modal: true, detail: 'agora' }, 'Sim', 'Não');
    return choice;
  });
  command('t.askItem', async () => {
    const chosen = await vscode.window.showErrorMessage('Falhou', { modal: true }, { title: 'Tentar de novo' }, { title: 'Fechar', isCloseAffordance: true });
    return chosen ? chosen.title : null;
  });
  command('t.notify', () => {
    vscode.window.showWarningMessage('Cuidado com isso', 'Entendi');
    return 'enviado';
  });
  command('t.status', () => {
    const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 50);
    item.text = '$(check) Pronto';
    item.tooltip = 'Tudo certo';
    item.command = 't.notify';
    item.name = 'Meu item';
    item.color = new vscode.ThemeColor('statusBar.foreground');
    item.show();
    globalThis.__item = item;
    return { text: item.text, alignment: item.alignment, priority: item.priority, tooltip: item.tooltip, command: item.command };
  });
  command('t.statusUpdate', () => { globalThis.__item.text = 'Mudou'; return globalThis.__item.text; });
  command('t.statusDispose', () => { globalThis.__item.dispose(); return 'descartado'; });
  command('t.message', () => {
    globalThis.__messageDisposable = vscode.window.setStatusBarMessage('Compilando...');
    return 'ok';
  });
  command('t.messageDispose', () => { globalThis.__messageDisposable.dispose(); return 'ok'; });
};
"#;

/// Uma pergunta que chegou ao `IDialogService`.
#[derive(Debug, PartialEq)]
struct AskedPrompt {
    severity: Option<Severity>,
    message: String,
    detail: Option<String>,
    labels: Vec<String>,
}

/// O `IDialogService` que grava cada `prompt` e aperta o primeiro botão que não é o de cancelar (ou o de cancelar, se for o único).
struct RecordingDialogService {
    prompts: Rc<RefCell<Vec<AskedPrompt>>>,
}

impl IDialogService for RecordingDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        let mut buttons: Vec<(String, Rc<dyn Fn(CheckboxResult) -> MaybePromise<Dynamic>>)> =
            prompt.buttons.unwrap_or_default().into_iter().map(|button| (button.label, button.base.run)).collect();
        if let Some(PromptCancel::Button(cancel)) = prompt.cancel_button {
            buttons.push((cancel.label.unwrap_or_default(), cancel.base.run));
        }
        self.prompts.borrow_mut().push(AskedPrompt {
            severity: match prompt.base.dialog_type {
                Some(DialogTypeOrSeverity::Severity(severity)) => Some(severity),
                _ => None,
            },
            message: prompt.base.message.clone(),
            detail: match &prompt.base.detail {
                Some(DialogDetail::Text(text)) => Some(text.clone()),
                _ => None,
            },
            labels: buttons.iter().map(|(label, _)| label.clone()).collect(),
        });
        let result = buttons.first().map(|(_, run)| match run(CheckboxResult { checkbox_checked: Some(false) }) {
            MaybePromise::Value(value) => value,
            MaybePromise::Promise(_) => panic!("o botão devolveu uma promessa"),
        });
        Promise::resolved(PromptResult { checkbox_checked: None, result })
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        panic!("o teste não exercita IDialogService.input")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("o teste não exercita IDialogService.info")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("o teste não exercita IDialogService.warn")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("o teste não exercita IDialogService.error")
    }

    fn about(&self) -> Promise<()> {
        panic!("o teste não exercita IDialogService.about")
    }
}

struct NoopStatusHandle;

impl StatusHandle for NoopStatusHandle {
    fn close(&self) {}
}

/// Uma notificação gravada: a gravidade, o texto e quantas ações primárias ela trouxe.
#[derive(Debug, PartialEq)]
struct ShownNotification {
    severity: Severity,
    message: String,
    primary_actions: usize,
}

/// O `INotificationService` que grava cada `notify`.
struct RecordingNotificationService {
    shown: Rc<RefCell<Vec<ShownNotification>>>,
}

impl INotificationService for RecordingNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {}

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        panic!("o teste não exercita INotificationService.getFilter")
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }

    fn remove_filter(&self, _source_id: &str) {}

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        let message = match &notification.message {
            NotificationMessage::Text(text) => text.clone(),
            other => panic!("esperava texto, veio {other:?}"),
        };
        self.shown.borrow_mut().push(ShownNotification {
            severity: notification.severity,
            message,
            primary_actions: notification.actions.as_ref().map_or(0, |actions| actions.primary.len()),
        });
        Rc::new(NoOpNotification::new())
    }

    fn info(&self, _message: Vec<NotificationMessage>) {}

    fn warn(&self, _message: Vec<NotificationMessage>) {}

    fn error(&self, _message: Vec<NotificationMessage>) {}

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        panic!("o teste não exercita INotificationService.prompt")
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoopStatusHandle)
    }
}

/// O `IExtensionStatusBarItemService` em memória: guarda os itens que o `MainThreadStatusBar` define.
struct InMemoryStatusBar {
    entries: RefCell<Vec<(String, ExtensionStatusBarItem)>>,
    changes: Rc<Emitter<ExtensionStatusBarItemChangeEvent>>,
}

impl IExtensionStatusBarItemService for InMemoryStatusBar {
    fn on_did_change(&self) -> Event<ExtensionStatusBarItemChangeEvent> {
        self.changes.event()
    }

    fn set_or_update_entry(
        &self,
        id: &str,
        _status_id: &str,
        _extension_id: Option<&str>,
        name: &str,
        text: &str,
        tooltip: Option<StatusBarTooltip>,
        command: Option<ICommandDto>,
        _color: Option<StatusBarColor>,
        _background_color: Option<ThemeColor>,
        align_left: bool,
        priority: Option<f64>,
        _accessibility_information: Option<IAccessibilityInformation>,
    ) -> StatusBarUpdateKind {
        let item = ExtensionStatusBarItem {
            entry: ExtensionStatusBarEntry {
                name: name.to_owned(),
                text: text.to_owned(),
                tooltip: match tooltip {
                    Some(StatusBarTooltip::Value(StringOrMarkdown::String(text))) => Some(text),
                    _ => None,
                },
                command: command.map(|command| command.id),
                aria_label: None,
                role: None,
            },
            alignment: if align_left { StatusbarAlignment::Left } else { StatusbarAlignment::Right },
            priority: priority.unwrap_or(0.0),
        };
        let mut entries = self.entries.borrow_mut();
        match entries.iter_mut().find(|(known, _)| known == id) {
            Some((_, existing)) => {
                *existing = item;
                StatusBarUpdateKind::DidUpdate
            }
            None => {
                entries.push((id.to_owned(), item));
                StatusBarUpdateKind::DidDefine
            }
        }
    }

    fn unset_entry(&self, id: &str) {
        self.entries.borrow_mut().retain(|(known, _)| known != id);
    }

    fn get_entries(&self) -> Vec<(String, ExtensionStatusBarItem)> {
        self.entries.borrow().clone()
    }
}

/// Liga `MainThreadMessageService` e `MainThreadStatusBar` (reais) ao host.
struct Binder {
    message_service: Rc<MainThreadMessageService>,
    status_bar_service: Rc<InMemoryStatusBar>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let message_shape: Rc<dyn MainThreadMessageServiceShape> = self.message_service.clone();
        context.set(&MainContext::MAIN_THREAD_MESSAGE_SERVICE, message_shape);
        let status_bar = MainThreadStatusBar::new(&*context, self.status_bar_service.clone());
        let status_bar_shape: Rc<dyn MainThreadStatusBarShape> = status_bar.clone();
        context.set(&MainContext::MAIN_THREAD_STATUS_BAR, status_bar_shape);
        let clients: [Rc<dyn IDisposable>; 2] = [self.message_service.clone(), status_bar];
        clients.into_iter().collect()
    }
}

struct Observed {
    prompts: Rc<RefCell<Vec<AskedPrompt>>>,
    notifications: Rc<RefCell<Vec<ShownNotification>>>,
    status_bar: Rc<InMemoryStatusBar>,
}

fn setup(workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let prompts = Rc::new(RefCell::new(Vec::new()));
    let notifications = Rc::new(RefCell::new(Vec::new()));
    let status_bar = Rc::new(InMemoryStatusBar { entries: RefCell::new(Vec::new()), changes: Emitter::new() });
    let message_service = MainThreadMessageService::new(
        Rc::new(RecordingNotificationService { shown: notifications.clone() }),
        workbench.command_service.clone(),
        Rc::new(RecordingDialogService { prompts: prompts.clone() }),
        workbench.extension_service.clone(),
    );
    (Rc::new(Binder { message_service, status_bar_service: status_bar.clone() }), Observed { prompts, notifications, status_bar })
}

fn text(value: &Dynamic, key: &str) -> String {
    match value.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto em '{key}', veio {other:?}"),
    }
}

#[test]
fn modal_messages_reach_the_dialog_service_and_return_the_chosen_item() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        // Itens de texto: o dublê aperta o primeiro botão, e a extensão recebe o texto dele.
        assert_eq!(workbench.execute("t.ask", Vec::new()), Dynamic::string("Sim"));
        // Itens `MessageItem`: o de `isCloseAffordance` vira o botão de cancelar; o primeiro botão volta como o item (`title`).
        assert_eq!(workbench.execute("t.askItem", Vec::new()), Dynamic::string("Tentar de novo"));

        let prompts = observed.prompts.borrow();
        assert_eq!(
            *prompts,
            vec![
                AskedPrompt {
                    severity: Some(Severity::Info),
                    message: "Salvar?".to_owned(),
                    detail: Some("agora".to_owned()),
                    // sem `isCloseAffordance`, o `MainThreadMessageService` acrescenta o botão padrão de cancelar
                    labels: vec!["Sim".to_owned(), "Não".to_owned(), "Cancel".to_owned()],
                },
                AskedPrompt { severity: Some(Severity::Error), message: "Falhou".to_owned(), detail: None, labels: vec!["Tentar de novo".to_owned(), "Fechar".to_owned()] },
            ]
        );
    });
}

#[test]
fn non_modal_messages_reach_the_notification_service() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.notify", Vec::new()), Dynamic::string("enviado"));
        assert_eq!(*observed.notifications.borrow(), vec![ShownNotification { severity: Severity::Warning, message: "Cuidado com isso".to_owned(), primary_actions: 1 }]);
        assert!(observed.prompts.borrow().is_empty(), "uma mensagem não modal não abre diálogo");
    });
}

#[test]
fn status_bar_items_are_defined_updated_and_removed_in_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let described = workbench.execute("t.status", Vec::new());
        assert_eq!(text(&described, "text"), "$(check) Pronto");
        assert_eq!(described.get_str("alignment"), Dynamic::Number(2.0), "StatusBarAlignment.Right");
        assert_eq!(described.get_str("priority"), Dynamic::Number(50.0));
        assert_eq!(text(&described, "tooltip"), "Tudo certo");
        assert_eq!(text(&described, "command"), "t.notify");

        let entries = observed.status_bar.get_entries();
        assert_eq!(entries.len(), 1, "o item aparece depois do show(), na volta do laço de eventos");
        let item = &entries[0].1;
        assert_eq!(item.entry.text, "$(check) Pronto");
        assert_eq!(item.entry.name, "Meu item");
        assert_eq!(item.entry.tooltip.as_deref(), Some("Tudo certo"));
        assert_eq!(item.entry.command.as_deref(), Some("t.notify"));
        assert_eq!(item.alignment, StatusbarAlignment::Right);
        assert_eq!(item.priority, 50.0);

        // `item.text = ...` é um acessor de escrita nativo: o item do workbench troca o texto.
        assert_eq!(workbench.execute("t.statusUpdate", Vec::new()), Dynamic::string("Mudou"));
        let entries = observed.status_bar.get_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.entry.text, "Mudou");

        assert_eq!(workbench.execute("t.statusDispose", Vec::new()), Dynamic::string("descartado"));
        assert!(observed.status_bar.get_entries().is_empty(), "dispose() remove o item do workbench");
    });
}

#[test]
fn status_bar_messages_show_and_clear_with_the_disposable() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert!(observed.status_bar.get_entries().is_empty());
        assert_eq!(workbench.execute("t.message", Vec::new()), Dynamic::string("ok"));
        let entries = observed.status_bar.get_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.entry.text, "Compilando...");
        assert_eq!(entries[0].1.alignment, StatusbarAlignment::Left);

        assert_eq!(workbench.execute("t.messageDispose", Vec::new()), Dynamic::string("ok"));
        assert!(observed.status_bar.get_entries().is_empty(), "o Disposable da mensagem a remove da barra");
    });
}
