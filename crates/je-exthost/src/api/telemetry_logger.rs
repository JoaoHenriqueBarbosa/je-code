//! `vscode.TelemetryLogger` e `vscode.env.createTelemetryLogger(sender, options?)` (`extHost.api.impl.ts` e `extHostTelemetry.ts`),
//! sobre o `ExtHostTelemetry` e o `ExtHostTelemetryLogger` do porte, que já trazem a regra inteira: níveis de telemetria, propriedades
//! comuns, `cleanData`, prefixo do evento, modo só de log, `flush` no `dispose` e a limpeza dos loggers descartados.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões, então a extensão do logger (o prefixo `publisher.name/` do evento e `common.extname`)
//!   é sempre a extensão do host (`je.exthost`), como nos demais namespaces.
//! - O `TelemetrySender` chega como cópia do objeto: só as funções que são propriedades próprias dele atravessam (uma classe com os
//!   métodos no protótipo falha na validação, com a mensagem do original), e as funções rodam sem `this`.
//! - O `TypeError` da validação e o erro que o `sender` lança chegam à extensão como `Error` comum (o motor recebe só a mensagem).
//! - O objeto devolvido não é congelado, e um logger descartado continua legível (`isUsageEnabled` e `isErrorsEnabled` guardam o último
//!   valor), como no original.
//! - O erro que o `flush` do `sender` lança dentro de `dispose` sai de `dispose` depois do `finally` do original (o logger já foi
//!   removido), e não por pânico.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_workbench::api::common::ext_host_telemetry::{ExtHostTelemetry, ExtHostTelemetryLogger, TelemetryLoggerOptions, TelemetrySender};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::languages::host_extension;
use crate::convert::{dynamic_to_js, error_to_js, field, is_nullish, js_to_dynamic, js_to_error, opt_bool, string_arg};

/// O `TelemetrySender` da extensão: as funções do objeto copiado. O que elas lançam fica guardado até a chamada da API que as
/// disparou terminar, porque o [`TelemetrySender`] do porte não devolve erro.
struct JsTelemetrySender {
    runner: NodeRunner,
    sender: JsData,
    thrown: RefCell<Option<JsData>>,
}

impl JsTelemetrySender {
    fn call(&self, name: &str, args: &[JsData]) {
        let Some(JsData::Handle(function)) = field(&self.sender, name) else {
            return;
        };
        if let Err(thrown) = self.runner.call_data_throwing(function, args) {
            self.thrown.borrow_mut().get_or_insert(thrown);
        }
    }

    /// O resultado da chamada da API: o primeiro erro que o `sender` lançou nela, se houve.
    fn finish(&self) -> Result<JsData, String> {
        match self.thrown.borrow_mut().take() {
            Some(thrown) => Err(js_to_error(&thrown).message),
            None => Ok(JsData::Undefined),
        }
    }
}

impl TelemetrySender for JsTelemetrySender {
    fn send_event_data(&self, event_name: &str, data: &Dynamic) {
        self.call("sendEventData", &[JsData::Str(event_name.to_owned()), dynamic_to_js(data)]);
    }

    fn send_error_data(&self, error: &Error, data: &Dynamic) {
        self.call("sendErrorData", &[error_to_js(error), dynamic_to_js(data)]);
    }

    fn has_flush(&self) -> bool {
        matches!(field(&self.sender, "flush"), Some(JsData::Handle(_)))
    }

    fn flush(&self) -> Result<Promise<()>, Error> {
        self.call("flush", &[]);
        Ok(Promise::resolved(()))
    }
}

/// Os loggers vivos por id; o objeto JavaScript carrega só o id (`_id`).
#[derive(Default)]
pub struct TelemetryLoggers {
    next: Cell<u64>,
    entries: RefCell<Vec<(u64, Rc<ExtHostTelemetryLogger>, Rc<JsTelemetrySender>)>>,
}

fn instance(id: u64) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("TelemetryLogger".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
}

impl TelemetryLoggers {
    fn add(&self, logger: Rc<ExtHostTelemetryLogger>, sender: Rc<JsTelemetrySender>) -> JsData {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.entries.borrow_mut().push((id, logger, sender));
        instance(id)
    }

    fn of(&self, this: &JsData) -> Result<(Rc<ExtHostTelemetryLogger>, Rc<JsTelemetrySender>), String> {
        let Some(JsData::Num(id)) = field(this, "_id") else { return Err("esperava um TelemetryLogger".to_owned()) };
        let id = *id as u64;
        self.entries
            .borrow()
            .iter()
            .find(|(known, _, _)| *known == id)
            .map(|(_, logger, sender)| (logger.clone(), sender.clone()))
            .ok_or_else(|| "TelemetryLogger desconhecido".to_owned())
    }

    fn to_js(&self, logger: &Rc<ExtHostTelemetryLogger>) -> JsData {
        self.entries.borrow().iter().find(|(_, known, _)| Rc::ptr_eq(known, logger)).map_or(JsData::Undefined, |(id, _, _)| instance(*id))
    }
}

/// As checagens de `ExtHostTelemetryLogger.validateSender`, sobre o objeto copiado do motor (o `Dynamic` do porte vê uma função como
/// objeto, então a checagem de `typeof` do original não serve aqui).
fn validate_sender(sender: &JsData) -> Result<(), String> {
    let is_function = |name: &str| matches!(field(sender, name), Some(JsData::Handle(_)));
    if !matches!(sender, JsData::Object(_) | JsData::Array(_) | JsData::Null) {
        return Err("TelemetrySender argument is invalid".to_owned());
    }
    if matches!(sender, JsData::Null) {
        return Err("Cannot read properties of null (reading 'sendEventData')".to_owned());
    }
    if !is_function("sendEventData") {
        return Err("TelemetrySender.sendEventData must be a function".to_owned());
    }
    if !is_function("sendErrorData") {
        return Err("TelemetrySender.sendErrorData must be a function".to_owned());
    }
    match field(sender, "flush") {
        None | Some(JsData::Undefined | JsData::Handle(_)) => Ok(()),
        Some(_) => Err("TelemetrySender.flush must be a function or undefined".to_owned()),
    }
}

/// `createTelemetryLogger(sender, options?)`.
pub fn create_logger(runner: &NodeRunner, telemetry: &ExtHostTelemetry, loggers: &TelemetryLoggers, args: &[JsData]) -> Result<JsData, String> {
    let sender = args.first().cloned().unwrap_or(JsData::Undefined);
    validate_sender(&sender)?;
    let options = args.get(1).filter(|options| !is_nullish(Some(*options))).map(|options| TelemetryLoggerOptions {
        ignore_built_in_common_properties: opt_bool(field(options, "ignoreBuiltInCommonProperties")),
        ignore_unhandled_errors: opt_bool(field(options, "ignoreUnhandledErrors")),
        additional_common_properties: field(options, "additionalCommonProperties").filter(|value| !is_nullish(Some(*value))).map(js_to_dynamic),
    });
    let js_sender = Rc::new(JsTelemetrySender { runner: runner.clone(), sender, thrown: RefCell::new(None) });
    let logger = telemetry.instantiate_logger(&host_extension(), js_sender.clone(), options);
    Ok(loggers.add(logger, js_sender))
}

/// A classe `TelemetryLogger` (não é exportada em `vscode`: é uma interface no `vscode.d.ts`).
pub fn define_class(runner: &NodeRunner, loggers: &Rc<TelemetryLoggers>, table: &Rc<DisposableTable>) {
    let usage = loggers.clone();
    let errors = loggers.clone();
    let states = loggers.clone();
    let (states_runner, states_table) = (runner.clone(), table.clone());
    let getters: Vec<(String, MethodCallback)> = vec![
        ("isUsageEnabled".to_owned(), Rc::new(move |this, _| Ok(JsData::Bool(usage.of(this)?.0.is_usage_enabled())))),
        ("isErrorsEnabled".to_owned(), Rc::new(move |this, _| Ok(JsData::Bool(errors.of(this)?.0.is_errors_enabled())))),
        (
            "onDidChangeEnableStates".to_owned(),
            Rc::new(move |this, _| {
                let event = states.of(this)?.0.on_did_change_enable_states();
                let to_js = states.clone();
                Ok(event_function(&states_runner, &states_table, "onDidChangeEnableStates", event, move |logger| to_js.to_js(logger)))
            }),
        ),
    ];
    let (log_usage, log_error, dispose) = (loggers.clone(), loggers.clone(), loggers.clone());
    let data_arg = |args: &[JsData]| args.get(1).filter(|data| !is_nullish(Some(*data))).map(js_to_dynamic);
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        (
            "logUsage".to_owned(),
            2,
            Rc::new(move |this, args| {
                let (logger, sender) = log_usage.of(this)?;
                let name = string_arg(args, 0, "logUsage: o nome do evento")?;
                logger.log_usage(&name, data_arg(args).as_ref());
                sender.finish()
            }),
        ),
        (
            "logError".to_owned(),
            2,
            Rc::new(move |this, args| {
                let (logger, sender) = log_error.of(this)?;
                let data = data_arg(args);
                match args.first() {
                    Some(JsData::Str(name)) => logger.log_error_event(name, data.as_ref()),
                    Some(exception) => logger.log_error_exception(&js_to_error(exception), data.as_ref()),
                    None => return Err("logError: faltou o nome do evento ou o erro".to_owned()),
                }
                sender.finish()
            }),
        ),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                let (logger, sender) = dispose.of(this)?;
                logger.dispose();
                sender.finish()
            }),
        ),
    ];
    runner.define_class(ClassSpec {
        name: "TelemetryLogger".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("TelemetryLogger não tem construtor público: use env.createTelemetryLogger".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}
