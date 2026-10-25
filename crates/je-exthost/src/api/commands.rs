//! `vscode.commands`: `registerCommand`, `executeCommand` e `getCommands` sobre o `ExtHostCommands` portado.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::promise::{MaybePromise, Promise};
use je_workbench::api::common::ext_host_commands::{CommandCallback, ExtHostCommands};
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::disposable::DisposableTable;
use crate::convert::{dynamic_to_js, error_to_js, js_to_dynamic, js_to_error, string_arg};

/// Um resultado JavaScript como o valor ou a promessa que o `ExtHostCommands` espera: um objeto com `then` é adotado.
pub fn settle_js_result(runner: &NodeRunner, result: JsData) -> MaybePromise<Dynamic> {
    let JsData::Handle(thenable) = &result else { return MaybePromise::Value(js_to_dynamic(&result)) };
    let (promise, resolver) = Promise::<Dynamic>::new();
    let on_fulfilled = {
        let resolver = resolver.clone();
        runner.make_function("fulfilled", 1, Rc::new(move |args| {
            resolver.resolve(js_to_dynamic(args.first().unwrap_or(&JsData::Undefined)));
            Ok(JsData::Undefined)
        }))
    };
    let on_rejected = runner.make_function("rejected", 1, Rc::new(move |args| {
        resolver.reject(js_to_error(args.first().unwrap_or(&JsData::Undefined)));
        Ok(JsData::Undefined)
    }));
    match runner.call_method(thenable, "then", &[JsData::Handle(Rc::new(on_fulfilled)), JsData::Handle(Rc::new(on_rejected))]) {
        Ok(_) => MaybePromise::Promise(promise),
        // Sem `then` chamável: é um valor comum (uma `Date`, por exemplo).
        Err(_) => MaybePromise::Value(js_to_dynamic(&result)),
    }
}

/// Chama a função JavaScript `function` e entrega o resultado como o valor ou a promessa que o `ExtHost*` espera; um `throw` vira promessa
/// rejeitada.
pub fn call_settled(runner: &NodeRunner, function: &JsHandle, args: &[JsData]) -> MaybePromise<Dynamic> {
    match runner.call_data_throwing(function, args) {
        Ok(result) => settle_js_result(runner, result),
        Err(thrown) => MaybePromise::Promise(Promise::rejected(js_to_error(&thrown))),
    }
}

/// `MaybePromise<T>` com `convert` (que pode falhar) aplicado ao valor quando ele chegar.
pub fn convert_maybe<T: Clone + 'static, U: Clone + 'static>(value: MaybePromise<T>, convert: impl Fn(T) -> Result<U, Error> + 'static) -> MaybePromise<U> {
    match value {
        MaybePromise::Value(value) => match convert(value) {
            Ok(converted) => MaybePromise::Value(converted),
            Err(error) => MaybePromise::Promise(Promise::rejected(error)),
        },
        MaybePromise::Promise(promise) => MaybePromise::Promise(promise.then_result(move |outcome| convert(outcome?))),
    }
}

/// Uma promessa do Rust como a promessa JavaScript que assenta quando ela assentar.
pub fn promise_to_js<T: Clone + 'static>(runner: &NodeRunner, promise: Promise<T>, convert: impl Fn(&T) -> JsData + 'static) -> JsData {
    promise_outcome_to_js(runner, promise, move |outcome| outcome.map(&convert).map_err(error_to_js))
}

/// Como [`promise_to_js`], mas `convert` decide o desfecho da promessa do JavaScript a partir do resultado inteiro (um `catch` do
/// original que recupera a rejeição, por exemplo): `Ok` resolve com o valor e `Err` rejeita com o motivo.
pub fn promise_outcome_to_js<T: Clone + 'static>(
    runner: &NodeRunner,
    promise: Promise<T>,
    convert: impl Fn(Result<&T, &je_base::errors::Error>) -> Result<JsData, JsData> + 'static,
) -> JsData {
    let target = Rc::new(runner.new_promise());
    let runner = runner.clone();
    let settled = target.clone();
    let _ = promise.then_result(move |outcome| {
        let result = match &outcome {
            Ok(value) => Ok(value),
            Err(error) => Err(error),
        };
        match convert(result) {
            Ok(value) => runner.resolve_promise(&settled, &value),
            Err(reason) => runner.reject_promise(&settled, &reason),
        };
        Ok(())
    });
    JsData::Handle(target)
}

/// O namespace `commands`.
pub fn namespace(runner: &NodeRunner, commands: &Rc<ExtHostCommands>, table: &Rc<DisposableTable>) -> JsData {
    let register = {
        let (runner, commands, table) = (runner.clone(), commands.clone(), table.clone());
        runner.clone().make_function("registerCommand", 2, Rc::new(move |args| {
            let id = string_arg(args, 0, "registerCommand: o id")?;
            let Some(JsData::Handle(callback)) = args.get(1).cloned() else { return Err("registerCommand: o callback precisa ser função".to_owned()) };
            let callback: CommandCallback = {
                let runner = runner.clone();
                let callback: Rc<JsHandle> = callback;
                Rc::new(move |command_args: &[Dynamic]| {
                    let js_args: Vec<JsData> = command_args.iter().map(dynamic_to_js).collect();
                    call_settled(&runner, &callback, &js_args)
                })
            };
            let disposable = commands.register_command(true, &id, callback, None, None).map_err(|error| error.message)?;
            Ok(table.to_js(Rc::new(disposable)))
        }))
    };
    let execute = {
        let (runner, commands) = (runner.clone(), commands.clone());
        runner.clone().make_function("executeCommand", 1, Rc::new(move |args| {
            let id = string_arg(args, 0, "executeCommand: o id")?;
            let command_args: Vec<Dynamic> = args[1..].iter().map(js_to_dynamic).collect();
            Ok(promise_to_js(&runner, commands.execute_command(&id, command_args), dynamic_to_js))
        }))
    };
    let get_commands = {
        let (runner, commands) = (runner.clone(), commands.clone());
        runner.clone().make_function("getCommands", 0, Rc::new(move |args| {
            let filter_internal = matches!(args.first(), Some(JsData::Bool(true)));
            Ok(promise_to_js(&runner, commands.get_commands(filter_internal), |ids| JsData::Array(ids.iter().cloned().map(JsData::Str).collect())))
        }))
    };
    JsData::Object(vec![
        ("registerCommand".to_owned(), JsData::Handle(Rc::new(register))),
        ("executeCommand".to_owned(), JsData::Handle(Rc::new(execute))),
        ("getCommands".to_owned(), JsData::Handle(Rc::new(get_commands))),
    ])
}
