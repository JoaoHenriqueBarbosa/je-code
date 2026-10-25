//! `Terminal#shellIntegration` (`vscode.TerminalShellIntegration`) e `vscode.TerminalShellExecution`, os objetos que o
//! `ExtHostTerminalShellIntegration` portado (`extHostTerminalShellIntegration.ts`) produz, e os eventos
//! `onDidChangeTerminalShellIntegration`, `onDidStartTerminalShellExecution` e `onDidEndTerminalShellExecution` de `window`.
//!
//! Os objetos JavaScript carregam só o id (como `Terminal`): [`ShellObjects`] guarda os portados por id. `TerminalShellExecution#read()`
//! devolve um objeto com `[Symbol.asyncIterator]`, `next` e `return` que repassam ao `AsyncIterator<string>` portado (`next()` e
//! `return_()`); `for await (const data of execution.read())` consome o fluxo de dados da execução.
//!
//! Divergências:
//! - Os objetos entregues à extensão são cópias com o mesmo `_id`: `===` entre o `shellIntegration` de um evento e o de
//!   `Terminal#shellIntegration` falha (a mesma divergência de `Terminal`, ver `terminal.rs`).
//! - `Object.freeze` de `env` e do `value` dele não tem equivalente (são objetos novos a cada leitura).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{AsyncIterable, AsyncIterator};
use je_base::promise::Promise;
use je_workbench::api::common::ext_host_terminal_service::VscodeTerminal;
use je_workbench::api::common::ext_host_terminal_shell_integration::{
    Terminal, TerminalShellExecution, TerminalShellExecutionCommandLine, TerminalShellExecutionEndEvent, TerminalShellExecutionStartEvent, TerminalShellIntegration,
    TerminalShellIntegrationChangeEvent,
};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::promise_to_js;
use crate::api::terminal::TerminalObjects;
use crate::convert::{exported, field, opt_string, text_or_undefined, uri_to_js};

/// O `{ value, done }` do protocolo de iteração.
fn iteration_result(value: JsData, done: bool) -> JsData {
    JsData::Object(vec![("value".to_owned(), value), ("done".to_owned(), JsData::Bool(done))])
}

/// Os objetos de integração com o shell que o JavaScript segura, por id (o objeto carrega só o `_id`).
#[derive(Default)]
pub struct ShellObjects {
    next: Cell<u64>,
    integrations: RefCell<Vec<(u64, Rc<TerminalShellIntegration>)>>,
    executions: RefCell<Vec<(u64, Rc<TerminalShellExecution>)>>,
}

impl ShellObjects {
    fn next_id(&self) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        id
    }

    fn object(class: &str, id: u64) -> JsData {
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    /// O objeto `TerminalShellIntegration` de `integration` (a mesma integração vira sempre o mesmo id).
    pub fn integration_to_js(&self, integration: &Rc<TerminalShellIntegration>) -> JsData {
        let existing = self.integrations.borrow().iter().find(|(_, item)| Rc::ptr_eq(item, integration)).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_id();
            self.integrations.borrow_mut().push((id, integration.clone()));
            id
        });
        Self::object("TerminalShellIntegration", id)
    }

    /// O objeto `TerminalShellExecution` de `execution` (a mesma execução vira sempre o mesmo id).
    pub fn execution_to_js(&self, execution: &Rc<TerminalShellExecution>) -> JsData {
        let existing = self.executions.borrow().iter().find(|(_, item)| Rc::ptr_eq(item, execution)).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_id();
            self.executions.borrow_mut().push((id, execution.clone()));
            id
        });
        Self::object("TerminalShellExecution", id)
    }

    fn lookup<T: Clone>(items: &RefCell<Vec<(u64, T)>>, this: &JsData, what: &str) -> Result<T, String> {
        let Some(JsData::Num(id)) = field(this, "_id") else { return Err(format!("{what}: this inválido")) };
        items.borrow().iter().find(|(item_id, _)| *item_id == *id as u64).map(|(_, item)| item.clone()).ok_or_else(|| format!("{what}: objeto desconhecido"))
    }

    fn integration_of(&self, this: &JsData) -> Result<Rc<TerminalShellIntegration>, String> {
        Self::lookup(&self.integrations, this, "TerminalShellIntegration")
    }

    fn execution_of(&self, this: &JsData) -> Result<Rc<TerminalShellExecution>, String> {
        Self::lookup(&self.executions, this, "TerminalShellExecution")
    }
}

/// `vscode.TerminalShellExecutionCommandLine`.
fn command_line_to_js(command_line: &TerminalShellExecutionCommandLine) -> JsData {
    JsData::Object(vec![
        ("value".to_owned(), JsData::Str(command_line.value.clone())),
        ("confidence".to_owned(), JsData::Num(f64::from(command_line.confidence.value()))),
        ("isTrusted".to_owned(), JsData::Bool(command_line.is_trusted)),
    ])
}

/// O `Terminal` opaco de um evento como o objeto `Terminal` da extensão.
fn terminal_to_js(objects: &TerminalObjects, terminal: &Terminal) -> JsData {
    match terminal.clone().downcast::<VscodeTerminal>() {
        Ok(terminal) => objects.to_js(&terminal, None),
        Err(_) => JsData::Undefined,
    }
}

/// `vscode.TerminalShellIntegrationChangeEvent`.
pub fn change_event_to_js(objects: &TerminalObjects, event: &TerminalShellIntegrationChangeEvent) -> JsData {
    JsData::Object(vec![
        ("terminal".to_owned(), terminal_to_js(objects, &event.terminal)),
        ("shellIntegration".to_owned(), objects.shell.integration_to_js(&event.shell_integration)),
    ])
}

/// `vscode.TerminalShellExecutionStartEvent`.
pub fn start_event_to_js(objects: &TerminalObjects, event: &TerminalShellExecutionStartEvent) -> JsData {
    JsData::Object(vec![
        ("terminal".to_owned(), terminal_to_js(objects, &event.terminal)),
        ("shellIntegration".to_owned(), objects.shell.integration_to_js(&event.shell_integration)),
        ("execution".to_owned(), objects.shell.execution_to_js(&event.execution)),
    ])
}

/// `vscode.TerminalShellExecutionEndEvent`.
pub fn end_event_to_js(objects: &TerminalObjects, event: &TerminalShellExecutionEndEvent) -> JsData {
    JsData::Object(vec![
        ("terminal".to_owned(), terminal_to_js(objects, &event.terminal)),
        ("shellIntegration".to_owned(), objects.shell.integration_to_js(&event.shell_integration)),
        ("execution".to_owned(), objects.shell.execution_to_js(&event.execution)),
        ("exitCode".to_owned(), event.exit_code.map_or(JsData::Undefined, |code| JsData::Num(f64::from(code)))),
    ])
}

/// `TerminalShellExecution#read()`: o `AsyncIterable<string>` portado como um objeto com `[Symbol.asyncIterator]`.
fn read_to_js(runner: &NodeRunner, adapter: &zjsc::api::embed_node::JsHandle, execution: &TerminalShellExecution) -> Result<JsData, String> {
    let iterator = execution.read().async_iterator();
    let next = {
        let (runner, iterator) = (runner.clone(), iterator.clone());
        runner.clone().make_function(
            "next",
            0,
            Rc::new(move |_| {
                Ok(promise_to_js(&runner, iterator.next(), |item| match item {
                    Some(data) => iteration_result(JsData::Str(data.clone()), false),
                    None => iteration_result(JsData::Undefined, true),
                }))
            }),
        )
    };
    let close = {
        let runner = runner.clone();
        runner.clone().make_function(
            "return",
            0,
            Rc::new(move |_| {
                let closing = iterator.return_().unwrap_or_else(|| Promise::resolved(()));
                Ok(promise_to_js(&runner, closing, |_| iteration_result(JsData::Undefined, true)))
            }),
        )
    };
    runner.call_data(adapter, &[JsData::Handle(Rc::new(next)), JsData::Handle(Rc::new(close))])
}

/// Define as classes `TerminalShellIntegration` e `TerminalShellExecution` (protótipo dos objetos que o host entrega).
pub fn define_classes(runner: &NodeRunner, objects: &Rc<TerminalObjects>) {
    let integration_getters: Vec<(String, MethodCallback)> = vec![
        {
            let objects = objects.clone();
            (
                "cwd".to_owned(),
                Rc::new(move |this: &JsData, _: &[JsData]| Ok(objects.shell.integration_of(this)?.cwd().map_or(JsData::Undefined, |cwd| uri_to_js(&cwd)))) as MethodCallback,
            )
        },
        {
            let objects = objects.clone();
            (
                "env".to_owned(),
                Rc::new(move |this: &JsData, _: &[JsData]| {
                    Ok(objects.shell.integration_of(this)?.env().map_or(JsData::Undefined, |env| {
                        let value = env.value.into_iter().map(|(key, item)| (key, text_or_undefined(item))).collect();
                        JsData::Object(vec![("isTrusted".to_owned(), JsData::Bool(env.is_trusted)), ("value".to_owned(), JsData::Object(value))])
                    }))
                }) as MethodCallback,
            )
        },
    ];
    let execute_command: (String, u32, MethodCallback) = {
        let (objects, runner) = (objects.clone(), runner.clone());
        (
            "executeCommand".to_owned(),
            2,
            Rc::new(move |this: &JsData, args: &[JsData]| {
                let integration = objects.shell.integration_of(this)?;
                let command_line_or_executable = opt_string(args.first()).ok_or("executeCommand: o comando precisa ser texto")?;
                // Um array pode chegar por referência: atravessa por valor aqui.
                let arguments: Option<Vec<String>> = match args.get(1).map(|value| exported(&runner, value)) {
                    Some(JsData::Array(items)) => Some(items.iter().filter_map(|item| opt_string(Some(item))).collect()),
                    _ => None,
                };
                let execution = integration.execute_command(&command_line_or_executable, arguments.as_deref()).map_err(|error| error.message)?;
                Ok(objects.shell.execution_to_js(&execution))
            }) as MethodCallback,
        )
    };
    runner.define_class(ClassSpec {
        name: "TerminalShellIntegration".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("TerminalShellIntegration não tem construtor público: use Terminal#shellIntegration".to_owned())),
        methods: vec![execute_command],
        getters: integration_getters,
        statics: vec![],
        parent: None,
    });

    let adapter = Rc::new(
        runner
            .eval(
                "(next, close) => ({ [Symbol.asyncIterator]() { return this; }, next() { return next(); }, return() { return close(); } })",
                "je-exthost/shell-execution-read.js",
            )
            .expect("a função auxiliar de TerminalShellExecution#read avalia"),
    );
    let execution_getters: Vec<(String, MethodCallback)> = vec![
        {
            let objects = objects.clone();
            ("commandLine".to_owned(), Rc::new(move |this: &JsData, _: &[JsData]| Ok(command_line_to_js(&objects.shell.execution_of(this)?.command_line()))) as MethodCallback)
        },
        {
            let objects = objects.clone();
            (
                "cwd".to_owned(),
                Rc::new(move |this: &JsData, _: &[JsData]| Ok(objects.shell.execution_of(this)?.cwd().map_or(JsData::Undefined, |cwd| uri_to_js(&cwd)))) as MethodCallback,
            )
        },
    ];
    let read: (String, u32, MethodCallback) = {
        let (objects, runner) = (objects.clone(), runner.clone());
        (
            "read".to_owned(),
            0,
            Rc::new(move |this: &JsData, _: &[JsData]| read_to_js(&runner, &adapter, &*objects.shell.execution_of(this)?)) as MethodCallback,
        )
    };
    runner.define_class(ClassSpec {
        name: "TerminalShellExecution".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("TerminalShellExecution não tem construtor público: use TerminalShellIntegration#executeCommand".to_owned())),
        methods: vec![read],
        getters: execution_getters,
        statics: vec![],
        parent: None,
    });
}
