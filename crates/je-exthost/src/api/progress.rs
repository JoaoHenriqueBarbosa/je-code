//! `vscode.window.withProgress` e a classe `Progress` que a tarefa recebe, ligados ao `ExtHostProgress` portado (`extHostProgress.ts`).
//!
//! A tarefa da extensão `(progress, token) => Thenable<R>` roda dentro da chamada do `ExtHostProgress`: o `progress` é um objeto
//! `{ _id }` de uma classe nativa cujo `report({ message?, increment? })` entrega o passo ao `Progress` do Rust (que repassa, no máximo a
//! cada 100 ms, ao `MainThreadProgress`), e o `token` é o `CancellationToken` do host (o de `CancellationToken.None` se a opção
//! `cancellable` não foi pedida; o cancelamento do usuário no workbench o dispara).
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: a extensão chamadora é sempre `je.exthost`, marcada como "em desenvolvimento" (é o que o
//!   `ExtHostProgress` lê para não atribuir o progresso a uma extensão: sem identidade real não há como o workbench oferecer a ação
//!   "Manage Extension" nem a telemetria por extensão). A origem da notificação (`source`) usa `je.exthost` e o nome `exthost`.
//! - O valor que a tarefa devolve e o `Progress` da tarefa atravessam a fronteira do motor: o resultado volta como a cópia do valor
//!   (objeto comum e lista perdem a identidade; função e promessa ficam), e o `report` aceita só `message` (texto) e `increment`
//!   (número) do passo. Um `report` depois de a tarefa terminar não faz nada, como o `Progress` do original.
//! - A opção `location` aceita o valor de `ProgressLocation` ou `{ viewId }`, como `ProgressLocation | { viewId: string }` do original.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::promise::{MaybePromise, Promise};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::progress::progress::{IProgress, Progress};
use je_workbench::api::common::ext_host_progress::{ExtHostProgress, ProgressOptions, ProgressTaskFn};
use je_workbench::api::common::ext_host_protocol::IProgressStepDto;
use je_workbench::api::common::ext_host_type_converters::ProgressLocationArg;
use je_workbench::services::extensions::extensions::null_extension_description;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::events::EventTables;
use crate::api::HOST_EXTENSION;
use crate::convert::{dynamic_to_js, field, js_to_error, opt_bool, opt_number, opt_string};

/// Os `Progress` das tarefas em andamento, por id (o objeto JavaScript carrega só o id, `_id`).
#[derive(Default)]
pub struct ProgressReporters {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<Progress<IProgressStepDto>>>>,
}

impl ProgressReporters {
    /// O objeto `Progress` para `progress`, e o id dele (para esquecê-lo quando a tarefa terminar).
    fn add(&self, progress: Rc<Progress<IProgressStepDto>>) -> (u64, JsData) {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, progress);
        (id, JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("Progress".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))]))
    }

    fn remove(&self, id: u64) {
        self.items.borrow_mut().remove(&id);
    }

    /// O `Progress` do objeto `this`; `None` se a tarefa já terminou.
    fn of(&self, this: &JsData) -> Option<Rc<Progress<IProgressStepDto>>> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.items.borrow().get(&(*id as u64)).cloned(),
            _ => None,
        }
    }
}

/// A extensão que o `ExtHostProgress` enxerga (ver as divergências do módulo).
fn host_extension() -> ExtensionDescription {
    let mut extension = null_extension_description();
    extension.id = HOST_EXTENSION.to_owned();
    extension.identifier = ExtensionIdentifier::new(HOST_EXTENSION);
    extension.manifest.name = "exthost".to_owned();
    extension.is_under_development = true;
    extension
}

/// As `ProgressOptions` da extensão: `location` (obrigatória), `title` e `cancellable`.
fn progress_options_of(options: Option<&JsData>) -> Result<ProgressOptions, String> {
    let Some(options @ JsData::Object(_)) = options else { return Err("withProgress: as opções precisam ser um objeto".to_owned()) };
    let location = match field(options, "location") {
        Some(JsData::Num(value)) => ProgressLocationArg::Value(*value as i32),
        Some(view @ JsData::Object(_)) => match opt_string(field(view, "viewId")) {
            Some(view_id) => ProgressLocationArg::ViewId(view_id),
            None => return Err("withProgress: location precisa ser ProgressLocation ou { viewId }".to_owned()),
        },
        _ => return Err("withProgress: location precisa ser ProgressLocation ou { viewId }".to_owned()),
    };
    Ok(ProgressOptions { location, title: opt_string(field(options, "title")), cancellable: opt_bool(field(options, "cancellable")) })
}

/// `window.withProgress(options, task)`.
pub fn with_progress(runner: &NodeRunner, progress: &ExtHostProgress, reporters: &Rc<ProgressReporters>, events: &Rc<EventTables>, args: &[JsData]) -> Result<JsData, String> {
    let options = progress_options_of(args.first())?;
    let Some(JsData::Handle(task)) = args.get(1).cloned() else { return Err("withProgress: a tarefa precisa ser função".to_owned()) };
    let (task_runner, task_reporters, task_events) = (runner.clone(), reporters.clone(), events.clone());
    let run: ProgressTaskFn<Dynamic> = Box::new(move |report, token| {
        let (id, progress_object) = task_reporters.add(report);
        let outcome = match task_runner.call_data_throwing(&task, &[progress_object, task_events.token_to_js(token)]) {
            Ok(result) => match settle_js_result(&task_runner, result) {
                MaybePromise::Value(value) => Promise::resolved(value),
                MaybePromise::Promise(promise) => promise,
            },
            Err(thrown) => Promise::rejected(js_to_error(&thrown)),
        };
        let finished_reporters = task_reporters.clone();
        let _ = outcome.then_result(move |_| {
            finished_reporters.remove(id);
            Ok(())
        });
        outcome
    });
    Ok(promise_to_js(runner, progress.with_progress(&host_extension(), options, run), dynamic_to_js))
}

/// Define a classe `Progress` (o protótipo do objeto que a tarefa recebe).
pub fn define_class(runner: &NodeRunner, reporters: &Rc<ProgressReporters>) {
    let reporters = reporters.clone();
    let report: MethodCallback = Rc::new(move |this, args| {
        if let Some(progress) = reporters.of(this) {
            let step = args.first();
            progress.report(IProgressStepDto {
                message: step.and_then(|step| opt_string(field(step, "message"))),
                increment: step.and_then(|step| opt_number(field(step, "increment"))),
                total: None,
            });
        }
        Ok(JsData::Undefined)
    });
    runner.define_class(ClassSpec {
        name: "Progress".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("Progress não tem construtor público: ele chega à tarefa de window.withProgress".to_owned())),
        methods: vec![("report".to_owned(), 1, report)],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
}
