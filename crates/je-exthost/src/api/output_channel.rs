//! `vscode.window.createOutputChannel` e as classes `OutputChannel` e `LogOutputChannel`, ligadas ao `ExtHostOutputService` portado
//! (`extHostOutput.ts`).
//!
//! O canal que a extensão segura é o objeto `{ _id }` de uma classe nativa: os métodos e acessores (`name`, `logLevel`,
//! `onDidChangeLogLevel`) consultam o [`OutputChannelHandle`] do host pela tabela [`OutputChannels`], que carrega a mesma validação
//! (`Channel has been closed`) e a mesma espera pela criação do canal real que os literais do original.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: a extensão chamadora é sempre `je.exthost`, então o identificador do canal
//!   (`extension-output-je.exthost-#1-Nome`) e a pasta de logs do canal `{ log: true }` usam esse identificador, e o nível de log
//!   por extensão (`extensionLogLevel`) é o configurado para ele.
//! - Os argumentos extras de `trace`, `debug`, `info`, `warn` e `error` (`...args`) são ignorados: o `ExtHostOutputChannel` portado
//!   recebe só a mensagem. Um `Error` vira o texto da pilha (`stack`) ou `nome: mensagem`.
//! - `append`, `appendLine`, `replace` e as mensagens de log aceitam só texto (o original repassa qualquer valor ao logger, que o
//!   formata); um valor de outro tipo é um `TypeError`.
//! - `name` é o nome já sem espaços nas pontas (o original também devolve o nome aparado), e `show` aceita `ViewColumn` (número) ou
//!   `preserveFocus` (booleano) no primeiro argumento, como o original.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors::Error;
use je_extensions::manifest::ExtensionIdentifier;
use je_workbench::api::common::ext_host_output::{ExtHostOutputService, OutputChannelHandle, OutputChannelOptions, ShowArgument};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::disposable::DisposableTable;
use crate::api::enums::log_level_value;
use crate::api::events::event_function;
use crate::api::HOST_EXTENSION;
use crate::convert::{field, opt_bool, string_arg};

/// Os canais de saída vivos, por id (o objeto JavaScript carrega só o id, `_id`). Um canal `{ log: true }` que a extensão pede duas
/// vezes é o mesmo [`OutputChannelHandle`] e ganha o mesmo id.
#[derive(Default)]
pub struct OutputChannels {
    next: Cell<u64>,
    items: RefCell<Vec<(u64, Rc<OutputChannelHandle>)>>,
}

impl OutputChannels {
    /// O objeto `OutputChannel` (ou `LogOutputChannel`, se o canal tem nível de log) para `handle`.
    fn add(&self, handle: Rc<OutputChannelHandle>) -> JsData {
        let known = self.items.borrow().iter().find(|(_, known)| Rc::ptr_eq(known, &handle)).map(|(id, _)| *id);
        let id = known.unwrap_or_else(|| {
            let id = self.next.get() + 1;
            self.next.set(id);
            self.items.borrow_mut().push((id, handle.clone()));
            id
        });
        let class = if handle.log_level().is_some() { "LogOutputChannel" } else { "OutputChannel" };
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    fn handle_of(&self, this: &JsData) -> Result<Rc<OutputChannelHandle>, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => {
                let id = *id as u64;
                let found = self.items.borrow().iter().find(|(known, _)| *known == id).map(|(_, handle)| handle.clone());
                found.ok_or_else(|| "OutputChannel: o canal não existe mais".to_owned())
            }
            _ => Err("OutputChannel: this inválido".to_owned()),
        }
    }
}

/// `window.createOutputChannel(name, options?)`: `options` é o `languageId` (texto) ou `{ log: true }`.
pub fn create_output_channel(service: &ExtHostOutputService, channels: &OutputChannels, args: &[JsData]) -> Result<JsData, String> {
    let name = string_arg(args, 0, "createOutputChannel: o nome")?;
    let options = match args.get(1) {
        Some(JsData::Str(language_id)) => OutputChannelOptions::LanguageId(language_id.clone()),
        Some(options @ JsData::Object(_)) if opt_bool(field(options, "log")) == Some(true) => OutputChannelOptions::Log,
        _ => OutputChannelOptions::None,
    };
    let handle = service.create_output_channel(&name, options, &ExtensionIdentifier::new(HOST_EXTENSION)).map_err(|error| error.message)?;
    Ok(channels.add(handle))
}

/// O texto do primeiro argumento (`value: string`).
fn text_arg(args: &[JsData]) -> Result<String, Error> {
    string_arg(args, 0, "o texto").map_err(Error::new)
}

/// A mensagem de log: texto, ou um `Error` (a pilha, ou `nome: mensagem`).
fn log_message_arg(args: &[JsData]) -> Result<String, Error> {
    let invalid = || Error::new("a mensagem precisa ser texto ou Error");
    match args.first() {
        Some(JsData::Str(text)) => Ok(text.clone()),
        Some(error @ JsData::Object(_)) => {
            let text = |name: &str| match field(error, name) {
                Some(JsData::Str(text)) => Some(text.clone()),
                _ => None,
            };
            match (text("stack"), text("message")) {
                (Some(stack), _) => Ok(stack),
                (None, Some(message)) => Ok(text("name").map_or_else(|| message.clone(), |name| format!("{name}: {message}"))),
                (None, None) => Err(invalid()),
            }
        }
        _ => Err(invalid()),
    }
}

/// `show(columnOrPreserveFocus?, preserveFocus?)`.
fn show_arguments(args: &[JsData]) -> (Option<ShowArgument>, Option<bool>) {
    let column_or_preserve_focus = match args.first() {
        Some(JsData::Bool(flag)) => Some(ShowArgument::PreserveFocus(*flag)),
        Some(JsData::Num(column)) => Some(ShowArgument::Column(*column as i32)),
        _ => None,
    };
    (column_or_preserve_focus, opt_bool(args.get(1)))
}

/// Um método do canal: valida e repassa ao [`OutputChannelHandle`]; o `Err` do host (`Channel has been closed`) vira o `TypeError`.
fn method(
    channels: &Rc<OutputChannels>,
    name: &str,
    length: u32,
    body: impl Fn(&OutputChannelHandle, &[JsData]) -> Result<(), Error> + 'static,
) -> (String, u32, MethodCallback) {
    let channels = channels.clone();
    (
        name.to_owned(),
        length,
        Rc::new(move |this, args| {
            body(&*channels.handle_of(this)?, args).map_err(|error| error.message)?;
            Ok(JsData::Undefined)
        }),
    )
}

/// `append`, `appendLine`, `clear`, `replace`, `show`, `hide` e `dispose`: os métodos que `OutputChannel` e `LogOutputChannel` têm.
fn channel_methods(channels: &Rc<OutputChannels>) -> Vec<(String, u32, MethodCallback)> {
    vec![
        method(channels, "append", 1, |channel, args| channel.append(&text_arg(args)?)),
        method(channels, "appendLine", 1, |channel, args| channel.append_line(&text_arg(args)?)),
        method(channels, "clear", 0, |channel, _| channel.clear()),
        method(channels, "replace", 1, |channel, args| channel.replace(&text_arg(args)?)),
        method(channels, "show", 2, |channel, args| {
            let (column_or_preserve_focus, preserve_focus) = show_arguments(args);
            channel.show(column_or_preserve_focus, preserve_focus)
        }),
        method(channels, "hide", 0, |channel, _| channel.hide()),
        method(channels, "dispose", 0, |channel, _| {
            channel.dispose();
            Ok(())
        }),
    ]
}

/// `trace`, `debug`, `info`, `warn` e `error`: o que só o `LogOutputChannel` tem.
fn log_methods(channels: &Rc<OutputChannels>) -> Vec<(String, u32, MethodCallback)> {
    vec![
        method(channels, "trace", 1, |channel, args| channel.trace(&log_message_arg(args)?)),
        method(channels, "debug", 1, |channel, args| channel.debug(&log_message_arg(args)?)),
        method(channels, "info", 1, |channel, args| channel.info(&log_message_arg(args)?)),
        method(channels, "warn", 1, |channel, args| channel.warn(&log_message_arg(args)?)),
        method(channels, "error", 1, |channel, args| channel.error(&log_message_arg(args)?)),
    ]
}

fn name_getter(channels: &Rc<OutputChannels>) -> (String, MethodCallback) {
    let channels = channels.clone();
    ("name".to_owned(), Rc::new(move |this, _| Ok(JsData::Str(channels.handle_of(this)?.name().to_owned()))))
}

/// Define as classes `OutputChannel` e `LogOutputChannel` (os protótipos dos canais que `createOutputChannel` devolve).
pub fn define_classes(runner: &NodeRunner, channels: &Rc<OutputChannels>, table: &Rc<DisposableTable>) {
    runner.define_class(ClassSpec {
        name: "OutputChannel".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("OutputChannel não tem construtor público: use window.createOutputChannel".to_owned())),
        methods: channel_methods(channels),
        getters: vec![name_getter(channels)],
        statics: vec![],
        parent: None,
    });

    let (level_channels, event_channels) = (channels.clone(), channels.clone());
    let (event_runner, event_table) = (runner.clone(), table.clone());
    let mut methods = channel_methods(channels);
    methods.extend(log_methods(channels));
    let mut getters: Vec<(String, MethodCallback)> = vec![name_getter(channels)];
    getters.push((
        "logLevel".to_owned(),
        Rc::new(move |this, _| {
            let level = level_channels.handle_of(this)?.log_level().ok_or("LogOutputChannel: o canal não tem nível de log")?;
            Ok(JsData::Num(f64::from(log_level_value(level))))
        }),
    ));
    getters.push((
        "onDidChangeLogLevel".to_owned(),
        Rc::new(move |this, _| {
            let event = event_channels.handle_of(this)?.on_did_change_log_level().ok_or("LogOutputChannel: o canal não tem nível de log")?;
            Ok(event_function(&event_runner, &event_table, "onDidChangeLogLevel", event, |level| JsData::Num(f64::from(log_level_value(*level)))))
        }),
    ));
    runner.define_class(ClassSpec {
        name: "LogOutputChannel".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("LogOutputChannel não tem construtor público: use window.createOutputChannel".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}
