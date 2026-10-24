//! `vscode.window.createTerminal`, `terminals`, `activeTerminal`, `onDidOpenTerminal`, `onDidCloseTerminal`,
//! `onDidChangeActiveTerminal`, `onDidChangeTerminalState`, `registerTerminalProfileProvider` e a classe `vscode.Terminal`.
//!
//! É a ponte para o `ExtHostTerminalService` portado (`extHostTerminalService.ts`) na forma [`WorkerExtHostTerminalService::new_local`]:
//! o serviço fala com o cliente `MainThreadTerminalService` do workbench (que cria o terminal de verdade), e o `Pseudoterminal` que a
//! extensão entrega em `ExtensionTerminalOptions` é adaptado por [`JsPseudoterminal`] (os eventos `onDidWrite`, `onDidClose`,
//! `onDidOverrideDimensions` e `onDidChangeName` viram eventos do Rust, e `open`, `close`, `handleInput` e `setDimensions` chamam o
//! objeto da extensão com o `this` dele).
//!
//! Falta: `TerminalOptions#location` (e `parentTerminal`), `iconPath` como `ThemeIcon`, `Terminal#shellIntegration`,
//! `onDidChangeTerminalDimensions`, `onDidWriteTerminalData`, `onDidExecuteTerminalCommand`, `registerTerminalLinkProvider`,
//! `registerTerminalQuickFixProvider`, `registerTerminalCompletionProvider` (propostas de API ou fora do pedido) e
//! `environmentVariableCollection`.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: `registerTerminalProfileProvider` usa a extensão `je.exthost`, então a proposta
//!   `terminalTitle` nunca está habilitada e o `titleTemplate` de um perfil é ignorado, como o original faz sem a proposta.
//! - O `Pseudoterminal` devolvido por um `TerminalProfileProvider` atravessa o motor como cópia (a promessa do provedor entrega o
//!   resultado por valor): os métodos chegam, o `this` do objeto original não. O `pty` passado direto a `createTerminal` vai por
//!   referência e mantém o `this`.
//! - `Terminal#creationOptions` devolve o próprio objeto que a extensão passou (sem o `Object.freeze` do original); para o terminal
//!   criado por outro lado (o workbench, uma tarefa) é um objeto novo com o que o `IShellLaunchConfig` trazia.
//! - O objeto `Terminal` que a extensão recebe de um evento e o de `createTerminal` são cópias com o mesmo `_id`: `===` entre
//!   eles falha, comparar por `name` ou guardar o objeto devolvido funciona.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::arrays::OneOrMany;
use je_base::cancellation::CancellationToken;
use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeColor;
use je_platform::terminal::terminal::{ITerminalEnvironment, ShellCwd, TerminalEnvironmentValue};
use je_workbench::api::common::ext_host_terminal_service::{
    ExtensionTerminalOptions, Pseudoterminal, TerminalCreationOptions, TerminalDimensions, TerminalIconPath, TerminalOptions, TerminalProfile, TerminalProfileProvider,
    TerminalProfileResult, VscodeTerminal, WorkerExtHostTerminalService,
};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, EventTables};
use crate::api::js_object::JsObjects;
use crate::api::languages::host_extension;
use crate::convert::{dynamic_to_js, field, is_nullish, opt_bool, opt_number, opt_string, uri_from_js, uri_to_js};

// ---- os objetos `Terminal`

struct TerminalEntry {
    id: u64,
    terminal: Rc<VscodeTerminal>,
    /// O objeto de opções que a extensão passou a `createTerminal`, devolvido por `creationOptions`.
    options: Option<JsData>,
}

/// Os terminais que o JavaScript segura, por id (o objeto `Terminal` carrega só o `_id`).
#[derive(Default)]
pub struct TerminalObjects {
    next: Cell<u64>,
    items: RefCell<Vec<TerminalEntry>>,
}

impl TerminalObjects {
    /// O objeto `Terminal` de `terminal` (o mesmo terminal vira sempre o mesmo id). `options` só vale na primeira vez.
    pub fn to_js(&self, terminal: &Rc<VscodeTerminal>, options: Option<JsData>) -> JsData {
        let existing = self.items.borrow().iter().find(|entry| Rc::ptr_eq(&entry.terminal, terminal)).map(|entry| entry.id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next.get() + 1;
            self.next.set(id);
            self.items.borrow_mut().push(TerminalEntry { id, terminal: terminal.clone(), options });
            id
        });
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("Terminal".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    fn entry_of(&self, this: &JsData) -> Result<(Rc<VscodeTerminal>, Option<JsData>), String> {
        let Some(JsData::Num(id)) = field(this, "_id") else { return Err("Terminal: this inválido".to_owned()) };
        self.items.borrow().iter().find(|entry| entry.id == *id as u64).map(|entry| (entry.terminal.clone(), entry.options.clone())).ok_or_else(|| "Terminal: terminal desconhecido".to_owned())
    }
}

// ---- as opções, do JavaScript para o Rust

fn shell_args_from_js(value: Option<&JsData>) -> Option<OneOrMany<String>> {
    match value {
        Some(JsData::Str(arg)) => Some(OneOrMany::One(arg.clone())),
        Some(JsData::Array(items)) => Some(OneOrMany::Many(items.iter().filter_map(|item| opt_string(Some(item))).collect())),
        _ => None,
    }
}

fn environment_from_js(value: Option<&JsData>) -> Option<ITerminalEnvironment> {
    let Some(JsData::Object(fields)) = value else { return None };
    Some(
        fields
            .iter()
            .filter(|(key, _)| key != CLASS_KEY)
            .map(|(key, item)| {
                let value = match item {
                    JsData::Str(text) => TerminalEnvironmentValue::Value(text.clone()),
                    JsData::Null => TerminalEnvironmentValue::Null,
                    _ => TerminalEnvironmentValue::Undefined,
                };
                (key.clone(), value)
            })
            .collect(),
    )
}

fn cwd_from_js(value: Option<&JsData>) -> Result<Option<ShellCwd>, String> {
    match value {
        Some(JsData::Str(path)) => Ok(Some(ShellCwd::Path(path.clone()))),
        Some(uri @ JsData::Object(_)) => Ok(Some(ShellCwd::Uri(uri_from_js(uri)?))),
        _ => Ok(None),
    }
}

/// `iconPath`: `Uri | { light: Uri; dark: Uri }` (o `ThemeIcon` está em "Falta").
fn icon_from_js(value: Option<&JsData>) -> Result<Option<TerminalIconPath>, String> {
    match value {
        Some(icon) if field(icon, "light").is_some() && field(icon, "dark").is_some() => Ok(Some(TerminalIconPath::LightDark {
            light: uri_from_js(field(icon, "light").ok_or("iconPath.light")?)?,
            dark: uri_from_js(field(icon, "dark").ok_or("iconPath.dark")?)?,
        })),
        Some(icon @ JsData::Object(_)) if field(icon, "scheme").is_some() => Ok(Some(TerminalIconPath::Uri(uri_from_js(icon)?))),
        _ => Ok(None),
    }
}

fn color_from_js(value: Option<&JsData>) -> Result<Option<ThemeColor>, String> {
    match value {
        Some(color @ JsData::Object(_)) => crate::api::window::theme_color_of(color).map(Some),
        _ => Ok(None),
    }
}

/// `vscode.TerminalOptions`.
fn terminal_options_from_js(data: &JsData) -> Result<TerminalOptions, String> {
    Ok(TerminalOptions {
        name: opt_string(field(data, "name")),
        shell_path: opt_string(field(data, "shellPath")),
        shell_args: shell_args_from_js(field(data, "shellArgs")),
        cwd: cwd_from_js(field(data, "cwd"))?,
        env: environment_from_js(field(data, "env")),
        icon_path: icon_from_js(field(data, "iconPath"))?,
        color: color_from_js(field(data, "color"))?,
        message: opt_string(field(data, "message")),
        strict_env: opt_bool(field(data, "strictEnv")),
        hide_from_user: opt_bool(field(data, "hideFromUser")),
        is_transient: opt_bool(field(data, "isTransient")),
        shell_integration_nonce: opt_string(field(data, "shellIntegrationNonce")),
        title_template: opt_string(field(data, "titleTemplate")),
        ..TerminalOptions::default()
    })
}

/// As opções, do Rust de volta para o JavaScript (`Terminal#creationOptions` de um terminal que a extensão não criou).
fn terminal_options_to_js(options: &TerminalOptions) -> JsData {
    let mut fields: Vec<(String, JsData)> = Vec::new();
    let mut put = |name: &str, value: Option<JsData>| {
        if let Some(value) = value {
            fields.push((name.to_owned(), value));
        }
    };
    put("name", options.name.clone().map(JsData::Str));
    put("shellPath", options.shell_path.clone().map(JsData::Str));
    put(
        "shellArgs",
        options.shell_args.as_ref().map(|args| match args {
            OneOrMany::One(arg) => JsData::Str(arg.clone()),
            OneOrMany::Many(args) => JsData::Array(args.iter().cloned().map(JsData::Str).collect()),
        }),
    );
    put(
        "cwd",
        options.cwd.as_ref().map(|cwd| match cwd {
            ShellCwd::Path(path) => JsData::Str(path.clone()),
            ShellCwd::Uri(uri) => uri_to_js(uri),
        }),
    );
    put(
        "env",
        options.env.as_ref().map(|env| {
            JsData::Object(
                env.iter()
                    .map(|(key, value)| {
                        let value = match value {
                            TerminalEnvironmentValue::Value(text) => JsData::Str(text.clone()),
                            TerminalEnvironmentValue::Null => JsData::Null,
                            TerminalEnvironmentValue::Undefined => JsData::Undefined,
                        };
                        (key.clone(), value)
                    })
                    .collect(),
            )
        }),
    );
    put("message", options.message.clone().map(JsData::Str));
    put("strictEnv", options.strict_env.map(JsData::Bool));
    put("hideFromUser", options.hide_from_user.map(JsData::Bool));
    put("isTransient", options.is_transient.map(JsData::Bool));
    JsData::Object(fields)
}

fn creation_options_to_js(options: &TerminalCreationOptions) -> JsData {
    match options {
        TerminalCreationOptions::Terminal(options) => terminal_options_to_js(options),
        TerminalCreationOptions::Extension(options) => JsData::Object(vec![("name".to_owned(), JsData::Str(options.name.clone()))]),
    }
}

// ---- o pseudoterminal da extensão

fn dimensions_to_js(dimensions: TerminalDimensions) -> JsData {
    JsData::Object(vec![("columns".to_owned(), JsData::Num(f64::from(dimensions.columns))), ("rows".to_owned(), JsData::Num(f64::from(dimensions.rows)))])
}

fn dimensions_from_js(value: &JsData) -> Option<TerminalDimensions> {
    Some(TerminalDimensions { columns: opt_number(field(value, "columns"))? as i32, rows: opt_number(field(value, "rows"))? as i32 })
}

/// O `vscode.Pseudoterminal` da extensão: o objeto por referência e as assinaturas dos eventos dele.
struct JsPseudoterminal {
    js: JsObjects,
    pty: Rc<JsHandle>,
    table: Rc<DisposableTable>,
    write: Rc<Emitter<String>>,
    close: Rc<Emitter<Option<i32>>>,
    dimensions: Rc<Emitter<Option<TerminalDimensions>>>,
    name: Rc<Emitter<String>>,
    /// Os ids (na tabela de descartáveis) das assinaturas que o host fez nos eventos da extensão.
    subscriptions: RefCell<Vec<u64>>,
}

impl JsPseudoterminal {
    fn new(js: JsObjects, pty: Rc<JsHandle>, table: Rc<DisposableTable>) -> Rc<Self> {
        Rc::new(Self {
            js,
            pty,
            table,
            write: Emitter::new(),
            close: Emitter::new(),
            dimensions: Emitter::new(),
            name: Emitter::new(),
            subscriptions: RefCell::new(Vec::new()),
        })
    }

    fn target(&self) -> JsData {
        JsData::Handle(self.pty.clone())
    }

    /// Assina `pty[name]` (um `Event<T>` da extensão) e entrega cada valor a `forward`; `false` se o objeto não tem o evento.
    fn subscribe(&self, name: &str, forward: impl Fn(&JsData) + 'static) -> bool {
        if !self.js.is_function(&self.target(), name) {
            return false;
        }
        let JsData::Handle(event) = self.js.property(&self.target(), name) else { return false };
        let listener = self.js.runner().make_function(
            name,
            1,
            Rc::new(move |args| {
                forward(args.first().unwrap_or(&JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        );
        match self.js.runner().call_data(&event, &[JsData::Handle(Rc::new(listener))]) {
            Ok(subscription) => {
                if let Some(JsData::Num(id)) = field(&subscription, "_id") {
                    self.subscriptions.borrow_mut().push(*id as u64);
                }
                true
            }
            Err(message) => {
                on_unexpected_error(&Error::new(message));
                false
            }
        }
    }

    /// `pty[method](...args)`; uma exceção da extensão vai ao tratador de erros inesperados.
    fn call(&self, method: &str, args: &[JsData]) {
        if let Err(message) = self.js.runner().call_method(&self.pty, method, args) {
            on_unexpected_error(&Error::new(message));
        }
    }
}

impl Pseudoterminal for JsPseudoterminal {
    fn on_did_write(&self) -> Event<String> {
        let write = self.write.clone();
        self.subscribe("onDidWrite", move |data| {
            if let JsData::Str(text) = data {
                write.fire(text.clone());
            }
        });
        self.write.event()
    }

    fn on_did_override_dimensions(&self) -> Option<Event<Option<TerminalDimensions>>> {
        let dimensions = self.dimensions.clone();
        self.subscribe("onDidOverrideDimensions", move |data| dimensions.fire(dimensions_from_js(data))).then(|| self.dimensions.event())
    }

    fn on_did_close(&self) -> Option<Event<Option<i32>>> {
        let close = self.close.clone();
        self.subscribe("onDidClose", move |data| close.fire(opt_number(Some(data)).map(|code| code as i32))).then(|| self.close.event())
    }

    fn on_did_change_name(&self) -> Option<Event<String>> {
        let name = self.name.clone();
        self.subscribe("onDidChangeName", move |data| {
            if let JsData::Str(text) = data {
                name.fire(text.clone());
            }
        })
        .then(|| self.name.event())
    }

    fn open(&self, initial_dimensions: Option<TerminalDimensions>) {
        self.call("open", &[initial_dimensions.map_or(JsData::Undefined, dimensions_to_js)]);
    }

    fn close(&self) {
        self.call("close", &[]);
        let subscriptions = std::mem::take(&mut *self.subscriptions.borrow_mut());
        for id in subscriptions {
            self.table.dispose(id);
        }
    }

    fn handle_input(&self, data: &str) {
        if self.js.is_function(&self.target(), "handleInput") {
            self.call("handleInput", &[JsData::Str(data.to_owned())]);
        }
    }

    fn set_dimensions(&self, dimensions: TerminalDimensions) {
        if self.js.is_function(&self.target(), "setDimensions") {
            self.call("setDimensions", &[dimensions_to_js(dimensions)]);
        }
    }
}

// ---- o contexto do namespace

#[derive(Clone)]
struct TerminalContext {
    js: JsObjects,
    table: Rc<DisposableTable>,
    events: Rc<EventTables>,
    service: Rc<WorkerExtHostTerminalService>,
    objects: Rc<TerminalObjects>,
}

impl TerminalContext {
    fn extension_options(&self, data: &JsData, pty: Rc<JsHandle>) -> Result<ExtensionTerminalOptions, String> {
        Ok(ExtensionTerminalOptions {
            name: opt_string(field(data, "name")).ok_or("ExtensionTerminalOptions: o name precisa ser texto")?,
            pty: JsPseudoterminal::new(self.js.clone(), pty, self.table.clone()),
            icon_path: icon_from_js(field(data, "iconPath"))?,
            color: color_from_js(field(data, "color"))?,
            location: None,
            is_transient: opt_bool(field(data, "isTransient")),
            shell_integration_nonce: opt_string(field(data, "shellIntegrationNonce")),
            title_template: opt_string(field(data, "titleTemplate")),
        })
    }

    /// `TerminalOptions | ExtensionTerminalOptions` vindo de uma cópia (o resultado de um provedor de perfil).
    fn creation_options(&self, data: &JsData) -> Result<TerminalCreationOptions, String> {
        match field(data, "pty").filter(|pty| !is_nullish(Some(*pty))) {
            Some(pty) => {
                let handle = match pty {
                    JsData::Handle(handle) => handle.clone(),
                    copy => Rc::new(self.js.runner().import(copy)),
                };
                self.extension_options(data, handle).map(TerminalCreationOptions::Extension)
            }
            None => terminal_options_from_js(data).map(TerminalCreationOptions::Terminal),
        }
    }

    /// `createTerminal(nameOrOptions?, shellPath?, shellArgs?)`.
    fn create_terminal(&self, args: &[JsData]) -> Result<JsData, String> {
        let (terminal, options) = match args.first() {
            Some(options @ JsData::Handle(handle)) => {
                let data = self.js.runner().export(handle);
                let terminal = match field(&data, "pty").filter(|pty| !is_nullish(Some(*pty))) {
                    Some(_) => {
                        let JsData::Handle(pty) = self.js.property(options, "pty") else { return Err("createTerminal: o pty precisa ser um objeto".to_owned()) };
                        self.service.create_extension_terminal(&self.extension_options(&data, pty)?, None)
                    }
                    None => self.service.create_terminal_from_options(terminal_options_from_js(&data)?, None).map_err(|error| error.message)?,
                };
                (terminal, Some(options.clone()))
            }
            _ => {
                // A função é por referência: um array (`shellArgs`) chega como `Handle` e atravessa por valor aqui.
                let by_value = |value: Option<&JsData>| match value {
                    Some(JsData::Handle(handle)) => Some(self.js.runner().export(handle)),
                    other => other.cloned(),
                };
                let (name, shell_path, shell_args) = (opt_string(args.first()), opt_string(args.get(1)), shell_args_from_js(by_value(args.get(2)).as_ref()));
                let terminal = self.service.create_terminal(name, shell_path, shell_args).map_err(|error| error.message)?;
                (terminal, None)
            }
        };
        Ok(self.objects.to_js(&terminal, options))
    }
}

// ---- o provedor de perfil

struct JsProfileProvider {
    context: TerminalContext,
    provider: Rc<JsHandle>,
}

impl JsProfileProvider {
    /// `TerminalProfile | TerminalOptions | ExtensionTerminalOptions | undefined`.
    fn result_from_js(&self, value: &JsData) -> Result<Option<TerminalProfileResult>, Error> {
        if is_nullish(Some(value)) {
            return Ok(None);
        }
        let to_error = |message: String| Error::new(message);
        Ok(Some(match field(value, "options") {
            Some(options) => TerminalProfileResult::Profile(TerminalProfile { options: self.context.creation_options(options).map_err(to_error)? }),
            None => TerminalProfileResult::Options(self.context.creation_options(value).map_err(to_error)?),
        }))
    }
}

impl TerminalProfileProvider for JsProfileProvider {
    fn provide_terminal_profile(&self, token: CancellationToken) -> MaybePromise<Option<TerminalProfileResult>> {
        let runner = self.context.js.runner();
        let result = runner.call_method(&self.provider, "provideTerminalProfile", &[self.context.events.token_to_js(token)]);
        match result {
            Ok(result) => match settle_js_result(runner, result) {
                MaybePromise::Value(value) => match self.result_from_js(&dynamic_to_js(&value)) {
                    Ok(converted) => MaybePromise::Value(converted),
                    Err(error) => MaybePromise::Promise(Promise::rejected(error)),
                },
                MaybePromise::Promise(promise) => {
                    let (context, provider) = (self.context.clone(), self.provider.clone());
                    MaybePromise::Promise(promise.then_result(move |outcome| JsProfileProvider { context, provider }.result_from_js(&dynamic_to_js(&outcome?))))
                }
            },
            Err(message) => MaybePromise::Promise(Promise::rejected(Error::new(message))),
        }
    }
}

// ---- a classe `Terminal`

fn terminal_state_to_js(terminal: &VscodeTerminal) -> JsData {
    let state = terminal.state();
    JsData::Object(vec![
        ("isInteractedWith".to_owned(), JsData::Bool(state.is_interacted_with)),
        ("shell".to_owned(), state.shell.map_or(JsData::Undefined, |shell| JsData::Str(shell.as_str().to_owned()))),
    ])
}

/// Define a classe `Terminal` (protótipo dos objetos que o host entrega).
pub fn define_classes(runner: &NodeRunner, objects: &Rc<TerminalObjects>) {
    let getter = |name: &str, read: Rc<dyn Fn(&Rc<VscodeTerminal>, Option<JsData>) -> JsData>| -> (String, MethodCallback) {
        let objects = objects.clone();
        (
            name.to_owned(),
            Rc::new(move |this, _| {
                let (terminal, options) = objects.entry_of(this)?;
                Ok(read(&terminal, options))
            }),
        )
    };
    let method = |name: &str, length: u32, body: Rc<dyn Fn(&Rc<VscodeTerminal>, &[JsData]) -> Result<(), Error>>| -> (String, u32, MethodCallback) {
        let objects = objects.clone();
        (
            name.to_owned(),
            length,
            Rc::new(move |this, args| {
                let (terminal, _) = objects.entry_of(this)?;
                body(&terminal, args).map_err(|error| error.message)?;
                Ok(JsData::Undefined)
            }),
        )
    };
    let process_runner = runner.clone();
    let getters: Vec<(String, MethodCallback)> = vec![
        getter("name", Rc::new(|terminal, _| JsData::Str(terminal.name()))),
        getter(
            "processId",
            Rc::new(move |terminal, _| promise_to_js(&process_runner, terminal.process_id(), |pid| pid.map_or(JsData::Undefined, |pid| JsData::Num(f64::from(pid))))),
        ),
        getter(
            "creationOptions",
            Rc::new(|terminal, options| options.unwrap_or_else(|| creation_options_to_js(terminal.creation_options()))),
        ),
        getter(
            "exitStatus",
            Rc::new(|terminal, _| match terminal.exit_status() {
                Some(status) => JsData::Object(vec![
                    ("code".to_owned(), status.code.map_or(JsData::Undefined, |code| JsData::Num(f64::from(code)))),
                    ("reason".to_owned(), JsData::Num(f64::from(status.reason as i32))),
                ]),
                None => JsData::Undefined,
            }),
        ),
        getter("state", Rc::new(|terminal, _| terminal_state_to_js(terminal))),
    ];
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        method("sendText", 2, Rc::new(|terminal, args| terminal.send_text(&opt_string(args.first()).unwrap_or_default(), opt_bool(args.get(1))))),
        method("show", 1, Rc::new(|terminal, args| terminal.show(opt_bool(args.first()).unwrap_or(false)))),
        method("hide", 0, Rc::new(|terminal, _| terminal.hide())),
        method(
            "dispose",
            0,
            Rc::new(|terminal, _| {
                terminal.dispose();
                Ok(())
            }),
        ),
    ];
    runner.define_class(ClassSpec {
        name: "Terminal".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("Terminal não tem construtor público: use window.createTerminal".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}

/// A classe `vscode.TerminalProfile` (`new TerminalProfile(options)`).
pub fn terminal_profile_class(runner: &NodeRunner) -> JsData {
    let constructor = runner.define_class(ClassSpec {
        name: "TerminalProfile".to_owned(),
        length: 1,
        construct: Rc::new(|args| Ok(JsData::Object(vec![("options".to_owned(), args.first().cloned().unwrap_or(JsData::Undefined))]))),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Handle(Rc::new(constructor))
}

// ---- os membros de `vscode.window`

/// Os membros de `window` que o terminal acrescenta: as funções que recebem objetos da extensão (`createTerminal`,
/// `registerTerminalProfileProvider`, por referência) e os acessores (`terminals`, `activeTerminal` e os eventos).
pub struct TerminalMembers {
    pub references: Vec<(String, JsData)>,
    pub getters: Vec<(String, MethodCallback)>,
}

fn event_getter<T: 'static>(
    runner: &NodeRunner,
    table: &Rc<DisposableTable>,
    name: &'static str,
    source: impl Fn() -> Event<T> + 'static,
    to_js: impl Fn(&T) -> JsData + Clone + 'static,
) -> (String, MethodCallback) {
    let (runner, table) = (runner.clone(), table.clone());
    (name.to_owned(), Rc::new(move |_, _| Ok(event_function(&runner, &table, name, source(), to_js.clone()))))
}

pub fn window_members(
    runner: &NodeRunner,
    js: &JsObjects,
    service: &Rc<WorkerExtHostTerminalService>,
    objects: &Rc<TerminalObjects>,
    table: &Rc<DisposableTable>,
    events: &Rc<EventTables>,
) -> TerminalMembers {
    let context = TerminalContext { js: js.clone(), table: table.clone(), events: events.clone(), service: service.clone(), objects: objects.clone() };
    let reference = |name: &str, length: u32, body: NativeCallback| (name.to_owned(), JsData::Handle(Rc::new(runner.make_function_by_reference(name, length, body))));

    let create_context = context.clone();
    let profile_context = context;
    let references = vec![
        reference("createTerminal", 3, Rc::new(move |args| create_context.create_terminal(args))),
        reference(
            "registerTerminalProfileProvider",
            2,
            Rc::new(move |args| {
                let id = opt_string(args.first()).ok_or("registerTerminalProfileProvider: o id precisa ser texto")?;
                let Some(JsData::Handle(provider)) = args.get(1) else { return Err("registerTerminalProfileProvider: o provedor precisa ser um objeto".to_owned()) };
                let provider = Rc::new(JsProfileProvider { context: profile_context.clone(), provider: provider.clone() });
                let registration = profile_context.service.register_profile_provider(&host_extension(), &id, provider).map_err(|error| error.message)?;
                Ok(profile_context.table.to_js(registration))
            }),
        ),
    ];

    let (terminals_service, terminals_objects) = (service.clone(), objects.clone());
    let (active_service, active_objects) = (service.clone(), objects.clone());
    let getters = vec![
        (
            "terminals".to_owned(),
            Rc::new(move |_: &JsData, _: &[JsData]| Ok(JsData::Array(terminals_service.terminals().iter().map(|terminal| terminals_objects.to_js(terminal, None)).collect()))) as MethodCallback,
        ),
        (
            "activeTerminal".to_owned(),
            Rc::new(move |_: &JsData, _: &[JsData]| Ok(active_service.active_terminal().map_or(JsData::Undefined, |terminal| active_objects.to_js(&terminal, None)))) as MethodCallback,
        ),
        {
            let (source, to_js_objects) = (service.clone(), objects.clone());
            event_getter(runner, table, "onDidOpenTerminal", move || source.on_did_open_terminal(), move |terminal: &Rc<VscodeTerminal>| to_js_objects.to_js(terminal, None))
        },
        {
            let (source, to_js_objects) = (service.clone(), objects.clone());
            event_getter(runner, table, "onDidCloseTerminal", move || source.on_did_close_terminal(), move |terminal: &Rc<VscodeTerminal>| to_js_objects.to_js(terminal, None))
        },
        {
            let (source, to_js_objects) = (service.clone(), objects.clone());
            event_getter(
                runner,
                table,
                "onDidChangeActiveTerminal",
                move || source.on_did_change_active_terminal(),
                move |terminal: &Option<Rc<VscodeTerminal>>| terminal.as_ref().map_or(JsData::Undefined, |terminal| to_js_objects.to_js(terminal, None)),
            )
        },
        {
            let (source, to_js_objects) = (service.clone(), objects.clone());
            event_getter(runner, table, "onDidChangeTerminalState", move || source.on_did_change_terminal_state(), move |terminal: &Rc<VscodeTerminal>| to_js_objects.to_js(terminal, None))
        },
    ];
    TerminalMembers { references, getters }
}
