//! Leitura de propriedades, chamada de métodos e escuta de eventos de objetos da extensão POR REFERÊNCIA.
//!
//! O motor entrega um objeto comum por cópia (só as propriedades próprias enumeráveis), então ler `pty.onDidWrite` ou
//! `treeDataProvider.onDidChangeTreeData` de uma cópia perderia os acessores e o `this` dos métodos de classe, e os elementos de uma
//! árvore perderiam a identidade. Aqui pequenos auxiliares em JavaScript (`target[name]`, `target[method](...args)`, a escuta de um
//! evento) entregam o valor a funções nativas por referência (`make_function_by_reference`), que o guardam como
//! [`JsData::Handle`]: o objeto que a extensão criou, com a identidade e o protótipo intactos. `identify` dá um número estável a cada
//! objeto (um `WeakMap` do lado do JavaScript), para o Rust reconhecer o mesmo elemento em chamadas diferentes.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::promise::{MaybePromise, Promise};
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::convert::js_to_error;

/// O resultado de um método da extensão (ou o valor de um evento) com os objetos por referência: `undefined`/`null`, um valor só, ou
/// os itens de um array.
#[derive(Clone)]
pub enum JsResult {
    Nullish,
    Single(JsData),
    Many(Vec<JsData>),
}

const NULLISH: f64 = 0.0;
const SINGLE: f64 = 1.0;

/// Os auxiliares por referência, ligados a um [`NodeRunner`].
#[derive(Clone)]
pub struct JsObjects {
    runner: NodeRunner,
    reader: Rc<JsHandle>,
    function_probe: Rc<JsHandle>,
    identity: Rc<JsHandle>,
    invoker: Rc<JsHandle>,
    event_wrapper: Rc<JsHandle>,
}

impl JsObjects {
    pub fn new(runner: &NodeRunner) -> Self {
        let evaluate = |source: &str, name: &str| Rc::new(runner.eval(source, name).unwrap_or_else(|message| panic!("{name} não avalia: {message}")));
        Self {
            runner: runner.clone(),
            reader: evaluate("(function (target, name, sink) { sink(target[name]); })", "je-exthost/property.js"),
            function_probe: evaluate(
                "(function (target, name) { return target !== undefined && target !== null && typeof target[name] === 'function'; })",
                "je-exthost/function-probe.js",
            ),
            identity: evaluate(
                "(function () { var ids = new WeakMap(); var next = 0; return function (value) { \
                 if (value === null || (typeof value !== 'object' && typeof value !== 'function')) { return -1; } \
                 var id = ids.get(value); if (id === undefined) { id = ++next; ids.set(value, id); } return id; }; })()",
                "je-exthost/identity.js",
            ),
            invoker: evaluate(
                "(function (target, method, args, emit, done, fail) { \
                 var finish = function (result) { \
                   if (result === undefined || result === null) { done(0); } \
                   else if (Array.isArray(result)) { for (var i = 0; i < result.length; i++) { emit(result[i]); } done(2); } \
                   else { emit(result); done(1); } }; \
                 try { var result = target[method].apply(target, args); \
                   if (result !== undefined && result !== null && typeof result.then === 'function') { result.then(finish, fail); } else { finish(result); } } \
                 catch (error) { fail(error); } })",
                "je-exthost/invoke.js",
            ),
            event_wrapper: evaluate(
                "(function (emit, done) { return function (value) { \
                 if (value === undefined || value === null) { done(0); } \
                 else if (Array.isArray(value)) { for (var i = 0; i < value.length; i++) { emit(value[i]); } done(2); } \
                 else { emit(value); done(1); } }; })",
                "je-exthost/event-wrapper.js",
            ),
        }
    }

    pub fn runner(&self) -> &NodeRunner {
        &self.runner
    }

    /// Chama `function(...args, sink)` e devolve o que ela entregou ao `sink`, por referência (objetos e funções como
    /// [`JsData::Handle`]); `undefined` se ela não chamou o `sink`.
    pub fn capture(&self, function: &JsHandle, args: &[JsData]) -> Result<JsData, String> {
        let slot = Rc::new(RefCell::new(JsData::Undefined));
        let sink = {
            let slot = slot.clone();
            self.runner.make_function_by_reference(
                "sink",
                1,
                Rc::new(move |args| {
                    *slot.borrow_mut() = args.first().cloned().unwrap_or(JsData::Undefined);
                    Ok(JsData::Undefined)
                }),
            )
        };
        let mut all = args.to_vec();
        all.push(JsData::Handle(Rc::new(sink)));
        self.runner.call_data(function, &all)?;
        let value = slot.borrow().clone();
        Ok(value)
    }

    /// `target[name]`, com objetos e funções como [`JsData::Handle`]; `undefined` se a leitura lança ou o alvo não é um objeto.
    pub fn property(&self, target: &JsData, name: &str) -> JsData {
        self.capture(&self.reader, &[target.clone(), JsData::Str(name.to_owned())]).unwrap_or(JsData::Undefined)
    }

    /// `typeof target[name] === 'function'`.
    pub fn is_function(&self, target: &JsData, name: &str) -> bool {
        matches!(self.runner.call_data(&self.function_probe, &[target.clone(), JsData::Str(name.to_owned())]), Ok(JsData::Bool(true)))
    }

    /// O número estável do objeto (o mesmo objeto dá sempre o mesmo número); `None` para um valor que não é objeto.
    pub fn identify(&self, value: &JsData) -> Option<u64> {
        match self.runner.call_data(&self.identity, std::slice::from_ref(value)) {
            Ok(JsData::Num(id)) if id >= 0.0 => Some(id as u64),
            _ => None,
        }
    }

    /// O par de funções `(emit, done)` que junta os valores entregues por referência: `emit(valor)` guarda um valor e `done(tipo)` entrega
    /// o resultado a `on_done` (e recomeça a coleta).
    fn collector(&self, on_done: impl Fn(JsResult) + 'static) -> (JsData, JsData) {
        let values: Rc<RefCell<Vec<JsData>>> = Rc::default();
        let emit_values = values.clone();
        let emit = self.runner.make_function_by_reference(
            "emit",
            1,
            Rc::new(move |args| {
                emit_values.borrow_mut().push(args.first().cloned().unwrap_or(JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        );
        let done = self.runner.make_function(
            "done",
            1,
            Rc::new(move |args| {
                let mut collected = std::mem::take(&mut *values.borrow_mut());
                let result = match args.first() {
                    Some(JsData::Num(kind)) if *kind == NULLISH => JsResult::Nullish,
                    Some(JsData::Num(kind)) if *kind == SINGLE && collected.len() == 1 => JsResult::Single(collected.remove(0)),
                    _ => JsResult::Many(collected),
                };
                on_done(result);
                Ok(JsData::Undefined)
            }),
        );
        (JsData::Handle(Rc::new(emit)), JsData::Handle(Rc::new(done)))
    }

    /// `target[method](...args)` com o resultado por referência (os itens de um array chegam um a um). Uma promessa da extensão vira
    /// uma promessa do Rust; uma exceção (ou rejeição) é uma promessa rejeitada.
    pub fn invoke(&self, target: &JsData, method: &str, args: Vec<JsData>) -> MaybePromise<JsResult> {
        let (promise, resolver) = Promise::<JsResult>::new();
        let done_resolver = resolver.clone();
        let (emit, done) = self.collector(move |result| done_resolver.resolve(result));
        let fail_resolver = resolver;
        let fail = self.runner.make_function(
            "fail",
            1,
            Rc::new(move |args| {
                fail_resolver.reject(js_to_error(args.first().unwrap_or(&JsData::Undefined)));
                Ok(JsData::Undefined)
            }),
        );
        let called = self.runner.call_data(&self.invoker, &[target.clone(), JsData::Str(method.to_owned()), JsData::Array(args), emit, done, JsData::Handle(Rc::new(fail))]);
        if let Err(message) = called {
            return MaybePromise::Promise(Promise::rejected(Error::new(message)));
        }
        match promise.result() {
            Some(Ok(result)) => MaybePromise::Value(result),
            _ => MaybePromise::Promise(promise),
        }
    }

    /// Assina `event` (uma função `Event<T>` da extensão, ou um `Event` do host) entregando cada valor a `on_value`, com os objetos
    /// por referência. Devolve o id do `Disposable` da assinatura na tabela de descartáveis, quando a extensão devolve um.
    pub fn listen(&self, event: &JsHandle, on_value: impl Fn(JsResult) + 'static) -> Result<Option<u64>, String> {
        let (emit, done) = self.collector(on_value);
        let wrapper = self.runner.call_data(&self.event_wrapper, &[emit, done])?;
        let subscription = self.runner.call_data(event, &[wrapper])?;
        Ok(match crate::convert::field(&subscription, "_id") {
            Some(JsData::Num(id)) => Some(*id as u64),
            _ => None,
        })
    }
}
