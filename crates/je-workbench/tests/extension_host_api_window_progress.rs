//! Teste de ponta a ponta de `vscode.window.withProgress` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo
//! host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), abre progressos, e o efeito aparece no
//! `IProgressService` do workbench.
//!
//! O lado do workbench é o `MainThreadProgress` REAL, ligado pelo binder do teste sobre um `IProgressService` que grava as opções de
//! cada progresso e os passos reportados, e que guarda o `onDidCancel` para o teste simular o cancelamento do usuário.
//!
//! - **Ciclo**: `withProgress` abre o progresso com as opções da extensão (`location`, `title`, `cancellable`), o `progress.report` chega
//!   ao workbench e o progresso termina quando a promessa da tarefa assenta (com o valor, ou com a exceção da tarefa).
//! - **Cancelamento**: o cancelamento do usuário dispara o token da tarefa (`onCancellationRequested`, `isCancellationRequested`).
//! - **Locais**: `ProgressLocation.Notification`, `Window`, `SourceControl` e `{ viewId }`.
//! - **Validação**: as opções que não são objeto e a exceção síncrona da tarefa.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::commands::commands::ICommandService;
use je_platform::notification::notification_message::NotificationPart;
use je_platform::progress::progress::{IProgress, IProgressService, ProgressCancellable, ProgressLocation, ProgressLocationId, ProgressOptions, ProgressStep, ProgressTask};
use je_workbench::api::browser::main_thread_progress::MainThreadProgress;
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadProgressShape};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  // Abre um progresso que só termina quando o teste manda (`t.finish`).
  command('t.start', () => {
    globalThis.__result = vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: 'Trabalhando', cancellable: false },
      (progress, token) => {
        progress.report({ message: 'metade', increment: 50 });
        globalThis.__cancellable = token.isCancellationRequested === false;
        return new Promise((resolve) => { globalThis.__finish = resolve; });
      },
    );
    return 'started';
  });
  command('t.finish', async () => {
    globalThis.__finish('feito');
    return String(await globalThis.__result);
  });
  command('t.startCancellable', () => {
    globalThis.__result = vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: 'Longo', cancellable: true },
      (progress, token) => new Promise((resolve) => {
        token.onCancellationRequested(() => resolve('cancelado:' + token.isCancellationRequested));
      }),
    );
    return 'started';
  });
  command('t.result', async () => String(await globalThis.__result));
  command('t.startLocation', () => {
    vscode.window.withProgress({ location: vscode.ProgressLocation.SourceControl }, () => Promise.resolve(1));
    vscode.window.withProgress({ location: { viewId: 'explorer' }, title: 'Na vista' }, () => 2);
    return 'started';
  });
  command('t.syncValue', async () => await vscode.window.withProgress({ location: vscode.ProgressLocation.Window }, () => 7));
  command('t.throwing', async () => {
    try {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Window }, () => { throw new Error('quebrou'); });
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
  command('t.badOptions', () => {
    try { vscode.window.withProgress(5, () => 1); return 'sem erro'; } catch (error) { return error.message; }
  });
  command('t.badTask', () => {
    try { vscode.window.withProgress({ location: vscode.ProgressLocation.Window }, 5); return 'sem erro'; } catch (error) { return error.message; }
  });
  command('t.enum', () => JSON.stringify([
    vscode.ProgressLocation.SourceControl,
    vscode.ProgressLocation.Window,
    vscode.ProgressLocation.Notification,
    vscode.ProgressLocation[15],
  ]));
};
"#;

/// As opções de um progresso como o `IProgressService` as recebeu.
#[derive(Debug, PartialEq)]
struct StartedProgress {
    location: &'static str,
    title: Option<String>,
    cancellable: bool,
}

fn location_name(location: &ProgressLocationId) -> &'static str {
    if matches!(location, ProgressLocationId::Location(ProgressLocation::Notification)) {
        "notification"
    } else if matches!(location, ProgressLocationId::Location(ProgressLocation::Window)) {
        "window"
    } else if matches!(location, ProgressLocationId::Location(ProgressLocation::Scm)) {
        "scm"
    } else if matches!(location, ProgressLocationId::View(_)) {
        "view"
    } else {
        "other"
    }
}

/// O `{ report: step => reports.push(step) }` que o serviço entrega à tarefa.
struct CollectingProgress(Rc<RefCell<Vec<(Option<String>, Option<f64>)>>>);

impl IProgress<ProgressStep> for CollectingProgress {
    fn report(&self, item: ProgressStep) {
        let message = match item.message {
            Some(NotificationPart::Text(text)) => Some(text),
            _ => None,
        };
        self.0.borrow_mut().push((message, item.increment));
    }
}

/// O `IProgressService` que grava cada progresso e deixa o teste cancelá-los.
struct RecordingProgressService {
    started: Rc<RefCell<Vec<StartedProgress>>>,
    reports: Rc<RefCell<Vec<(Option<String>, Option<f64>)>>>,
    cancels: RefCell<Vec<Rc<dyn Fn(Option<i32>)>>>,
    finished: Rc<Cell<u32>>,
}

impl RecordingProgressService {
    /// O usuário cancela todos os progressos abertos (o `onDidCancel` do serviço, com `undefined` como escolha).
    fn cancel_all(&self) {
        let cancels = self.cancels.borrow().clone();
        for cancel in cancels {
            cancel(None);
        }
    }
}

impl IProgressService for RecordingProgressService {
    fn with_progress(&self, value: ProgressOptions, task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        self.started.borrow_mut().push(StartedProgress {
            location: location_name(&value.location),
            title: match value.title {
                Some(NotificationPart::Text(text)) => Some(text),
                _ => None,
            },
            cancellable: matches!(value.cancellable, Some(ProgressCancellable::Flag(true))),
        });
        if let Some(on_did_cancel) = on_did_cancel {
            self.cancels.borrow_mut().push(on_did_cancel);
        }
        let finished = self.finished.clone();
        task(Rc::new(CollectingProgress(self.reports.clone()))).then(move |()| finished.set(finished.get() + 1))
    }
}

/// Liga o `MainThreadProgress` (real) ao host, sobre o `IProgressService` gravador.
struct Binder {
    service: Rc<RecordingProgressService>,
    command_service: Rc<dyn ICommandService>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let progress = MainThreadProgress::new(&*context, self.service.clone(), self.command_service.clone());
        let shape: Rc<dyn MainThreadProgressShape> = progress.clone();
        context.set(&MainContext::MAIN_THREAD_PROGRESS, shape);
        let client: Rc<dyn IDisposable> = progress;
        vec![client]
    }
}

struct Observed {
    service: Rc<RecordingProgressService>,
}

fn setup(workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let service = Rc::new(RecordingProgressService {
        started: Rc::default(),
        reports: Rc::default(),
        cancels: RefCell::new(Vec::new()),
        finished: Rc::default(),
    });
    (Rc::new(Binder { service: service.clone(), command_service: workbench.command_service.clone() }), Observed { service })
}

#[test]
fn with_progress_opens_the_progress_reports_steps_and_ends_when_the_task_settles() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.start", Vec::new()), Dynamic::string("started"));

        let service = &observed.service;
        assert_eq!(*service.started.borrow(), vec![StartedProgress { location: "notification", title: Some("Trabalhando".to_owned()), cancellable: false }]);
        assert_eq!(*service.reports.borrow(), vec![(Some("metade".to_owned()), Some(50.0))]);
        assert_eq!(service.finished.get(), 0, "a tarefa ainda não terminou");
        assert_eq!(workbench.probe("String(globalThis.__cancellable)"), "true", "sem `cancellable` o token da tarefa é o `CancellationToken.None`");

        assert_eq!(workbench.execute("t.finish", Vec::new()), Dynamic::string("feito"));
        assert_eq!(service.finished.get(), 1, "a promessa da tarefa assentou: o progresso terminou no workbench");
    });
}

#[test]
fn a_cancellable_progress_fires_the_token_of_the_task() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.startCancellable", Vec::new()), Dynamic::string("started"));
        assert_eq!(*observed.service.started.borrow(), vec![StartedProgress { location: "window", title: Some("Longo".to_owned()), cancellable: true }]);
        assert_eq!(observed.service.finished.get(), 0);

        observed.service.cancel_all();
        workbench.pump();
        assert_eq!(workbench.execute("t.result", Vec::new()), Dynamic::string("cancelado:true"));
        assert_eq!(observed.service.finished.get(), 1);
    });
}

#[test]
fn the_location_accepts_every_progress_location_and_a_view_id() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.startLocation", Vec::new()), Dynamic::string("started"));
        assert_eq!(
            *observed.service.started.borrow(),
            vec![
                StartedProgress { location: "scm", title: None, cancellable: false },
                StartedProgress { location: "view", title: Some("Na vista".to_owned()), cancellable: false },
            ]
        );
        assert_eq!(observed.service.finished.get(), 2, "as duas tarefas terminaram");
    });
}

#[test]
fn a_task_that_returns_a_plain_value_resolves_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.syncValue", Vec::new()), Dynamic::Number(7.0));
        assert_eq!(observed.service.finished.get(), 1);
    });
}

#[test]
fn a_task_that_throws_rejects_and_still_ends_the_progress() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.throwing", Vec::new()), Dynamic::string("erro:quebrou"));
        assert_eq!(observed.service.finished.get(), 1, "o progresso termina mesmo com a falha da tarefa");
    });
}

#[test]
fn invalid_options_or_task_throw_before_opening_a_progress() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.badOptions", Vec::new()), Dynamic::string("withProgress: as opções precisam ser um objeto"));
        assert_eq!(workbench.execute("t.badTask", Vec::new()), Dynamic::string("withProgress: a tarefa precisa ser função"));
        assert!(observed.service.started.borrow().is_empty());
    });
}

#[test]
fn the_progress_location_enumeration_is_exposed_in_both_directions() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.enum", Vec::new()), Dynamic::string(r#"[1,10,15,"Notification"]"#));
    });
}
