//! `vscode.window`: as mensagens (`showInformationMessage`, `showWarningMessage`, `showErrorMessage`) e a barra de status
//! (`createStatusBarItem`, `setStatusBarMessage`), mais as classes `vscode.ThemeColor` e `vscode.StatusBarItem` (esta só como
//! protótipo dos itens que o host entrega).
//!
//! As mensagens são o porte de `extHostMessageService.ts` (a lógica de `showMessage`: opções, itens, `isCloseAffordance`) sobre o
//! proxy `MainThreadMessageService`; a barra de status liga o `ExtHostStatusBar` portado.
//!
//! Falta (depende de serviços do host ainda não portados, sem fachada): `showQuickPick`, `showInputBox`, `createQuickPick`,
//! `createInputBox` (`ExtHostQuickOpen`); `createOutputChannel` (`ExtHostOutputService` e `MainThreadOutputService`);
//! `withProgress` (o `ExtHostProgress` está portado, mas o `MainThreadProgress` pede o `IProgressService` do workbench montado em
//! processo); `activeTextEditor`, `visibleTextEditors`, `onDidChangeActiveTextEditor`, `showTextDocument` (o
//! `ExtHostDocumentsAndEditors`, ver o cabeçalho de `workspace.rs`); `tabGroups`, `createTreeView`, `createWebviewPanel`,
//! `createTerminal` e os registros de provedores.
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: a `source` da notificação (`{ identifier, label }`) não é preenchida, porque o host
//!   não sabe qual extensão chamou. O `MainThreadMessageService` aceita a `source` ausente (é o caso `None` do teste dele).
//! - `useCustom` das opções é ignorado: o original exige a proposta `resolvers` da extensão chamadora, e sem a identidade dela não
//!   há como conferir.
//! - O item de mensagem que a promessa devolve é a cópia que atravessou a fronteira (o motor copia objetos comuns): comparar por
//!   identidade (`choice === item`) falha, comparar por `title` funciona. Para itens de texto não muda nada.
//! - `StatusBarItem#tooltip` aceita só texto (a `MarkdownString` ainda não é classe do `vscode`); `accessibilityInformation`,
//!   `tooltip2` e o `id` de um item criado sem identificador não estão expostos.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::themables::ThemeColor;
use je_platform::log::LogServiceRef;
use je_workbench::api::common::ext_host_protocol::{MainThreadMessageOptions, MainThreadMessageServiceShape, MessageCommandDto, StatusBarColor};
use je_workbench::api::common::ext_host_status_bar::{ExtHostStatusBar, ExtHostStatusBarEntry, StatusBarCommand, TimeoutOrThenable};
use je_workbench::api::common::ext_host_types::{Command, Documentation, StatusBarAlignment};
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::disposable::DisposableTable;
use crate::convert::{api_to_js, dynamic_to_js, field, js_to_dynamic, opt_number, opt_string, string_arg};

/// Os serviços do host que o namespace usa.
pub struct WindowServices {
    pub message_service: Rc<dyn MainThreadMessageServiceShape>,
    pub log_service: LogServiceRef,
    /// Sem o cliente `MainThreadStatusBar` do workbench não há barra de status, e os dois membros dela não existem em `vscode.window`.
    pub status_bar: Option<Rc<ExtHostStatusBar>>,
}

/// `isMessageItem(item)`: um objeto com `title` não vazio.
fn is_message_item(item: Option<&JsData>) -> bool {
    matches!(item.and_then(|item| field(item, "title")), Some(JsData::Str(title)) if !title.is_empty())
}

fn flag(options: &JsData, name: &str) -> Option<bool> {
    match field(options, name) {
        Some(JsData::Bool(value)) => Some(*value),
        _ => None,
    }
}

/// `ExtHostMessageService.showMessage` sem a extensão (ver as divergências do módulo): `args` são `(message, optionsOrFirstItem?,
/// ...rest)`.
fn show_message(runner: &NodeRunner, services: &WindowServices, severity: Severity, args: &[JsData]) -> Result<JsData, String> {
    let message = string_arg(args, 0, "a mensagem")?;
    let options_or_first_item = args.get(1);
    let rest: &[JsData] = args.get(2..).unwrap_or(&[]);

    let mut options = MainThreadMessageOptions::default();
    let items: Vec<JsData> = if matches!(options_or_first_item, Some(JsData::Str(_))) || is_message_item(options_or_first_item) {
        options_or_first_item.into_iter().chain(rest.iter()).cloned().collect()
    } else {
        if let Some(provided) = options_or_first_item.filter(|provided| matches!(provided, JsData::Object(_))) {
            options.modal = flag(provided, "modal");
            options.detail = opt_string(field(provided, "detail"));
        }
        rest.to_vec()
    };

    let mut commands: Vec<MessageCommandDto> = Vec::new();
    let mut has_close_affordance = false;
    for (handle, item) in items.iter().enumerate() {
        match item {
            JsData::Str(title) => commands.push(MessageCommandDto { title: title.clone(), is_close_affordance: false, handle: handle as f64 }),
            JsData::Object(_) => {
                let title = opt_string(field(item, "title")).unwrap_or_default();
                let is_close_affordance = matches!(field(item, "isCloseAffordance"), Some(JsData::Bool(true)));
                commands.push(MessageCommandDto { title, is_close_affordance, handle: handle as f64 });
                if is_close_affordance {
                    if has_close_affordance {
                        services.log_service.warn("Only one message item can have 'isCloseAffordance'");
                    } else {
                        has_close_affordance = true;
                    }
                }
            }
            _ => services.log_service.warn("Invalid message item"),
        }
    }

    let chosen = services.message_service.rpc_show_message(severity, &message, options, commands);
    Ok(promise_to_js(runner, chosen, move |handle| match handle {
        Some(handle) => items.get(*handle as usize).cloned().unwrap_or(JsData::Undefined),
        None => JsData::Undefined,
    }))
}

/// Uma promessa JavaScript (ou valor) como `Promise<()>` do Rust, para o `thenable` de `setStatusBarMessage`.
fn thenable_to_promise(runner: &NodeRunner, value: JsData) -> Promise<()> {
    match settle_js_result(runner, value) {
        MaybePromise::Promise(promise) => promise.then_result(|_| Ok(())),
        MaybePromise::Value(_) => Promise::resolved(()),
    }
}

/// Os itens de barra de status vivos, por id (o objeto JavaScript carrega só o id).
#[derive(Default)]
pub struct StatusBarItems {
    next: Cell<u64>,
    items: RefCell<HashMap<u64, Rc<ExtHostStatusBarEntry>>>,
}

impl StatusBarItems {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn add(&self, entry: Rc<ExtHostStatusBarEntry>) -> JsData {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.items.borrow_mut().insert(id, entry);
        JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("StatusBarItem".to_owned())), ("_id".to_owned(), JsData::Num(id as f64))])
    }

    fn entry_of(&self, this: &JsData) -> Result<Rc<ExtHostStatusBarEntry>, String> {
        match field(this, "_id") {
            Some(JsData::Num(id)) => self.items.borrow().get(&(*id as u64)).cloned().ok_or_else(|| "StatusBarItem: o item já foi descartado".to_owned()),
            _ => Err("StatusBarItem: this inválido".to_owned()),
        }
    }
}

fn status_bar_color_to_js(color: StatusBarColor) -> JsData {
    match color {
        StatusBarColor::String(text) => JsData::Str(text),
        StatusBarColor::ThemeColor(color) => api_to_js(&color),
    }
}

fn theme_color_of(value: &JsData) -> Result<ThemeColor, String> {
    ThemeColor::from_dynamic(&js_to_dynamic(value)).map_err(|error| error.message)
}

fn is_nullish_value(value: Option<&JsData>) -> bool {
    matches!(value, None | Some(JsData::Undefined) | Some(JsData::Null))
}

/// A classe `StatusBarItem`: acessores de leitura e escrita sobre o `ExtHostStatusBarEntry`.
fn define_status_bar_item(runner: &NodeRunner, items: &Rc<StatusBarItems>, table: &Rc<DisposableTable>) -> JsData {
    let _ = table;
    let get = |name: &str, read: Rc<dyn Fn(&ExtHostStatusBarEntry) -> JsData>| -> (String, MethodCallback) {
        let items = items.clone();
        (name.to_owned(), Rc::new(move |this, _| Ok(read(&*items.entry_of(this)?))))
    };
    let set = |name: &str, write: Rc<dyn Fn(&ExtHostStatusBarEntry, Option<&JsData>) -> Result<(), String>>| -> (String, MethodCallback) {
        let items = items.clone();
        (
            name.to_owned(),
            Rc::new(move |this, args| {
                write(&*items.entry_of(this)?, args.first())?;
                Ok(JsData::Undefined)
            }),
        )
    };
    let method = |name: &str, body: Rc<dyn Fn(&Rc<ExtHostStatusBarEntry>)>| -> (String, u32, MethodCallback) {
        let items = items.clone();
        (
            name.to_owned(),
            0,
            Rc::new(move |this, _| {
                body(&items.entry_of(this)?);
                Ok(JsData::Undefined)
            }),
        )
    };
    let dispose_items = items.clone();
    let getters: Vec<(String, MethodCallback)> = vec![
        get("alignment", Rc::new(|entry| JsData::Num(f64::from(entry.alignment().value())))),
        get("priority", Rc::new(|entry| entry.priority().map_or(JsData::Undefined, JsData::Num))),
        get("text", Rc::new(|entry| JsData::Str(entry.text()))),
        get("name", Rc::new(|entry| entry.name().map_or(JsData::Undefined, JsData::Str))),
        get(
            "tooltip",
            Rc::new(|entry| match entry.tooltip() {
                Some(Documentation::Text(text)) => JsData::Str(text),
                _ => JsData::Undefined,
            }),
        ),
        get("color", Rc::new(|entry| entry.color().map_or(JsData::Undefined, status_bar_color_to_js))),
        get("backgroundColor", Rc::new(|entry| entry.background_color().map_or(JsData::Undefined, |color| api_to_js(&color)))),
        get(
            "command",
            Rc::new(|entry| match entry.command() {
                Some(StatusBarCommand::Id(id)) => JsData::Str(id),
                Some(StatusBarCommand::Command(command)) => dynamic_to_js(&command.to_dynamic()),
                None => JsData::Undefined,
            }),
        ),
    ];
    let setters: Vec<(String, MethodCallback)> = vec![
        set(
            "text",
            Rc::new(|entry, value| {
                entry.set_text(opt_string(value).as_deref().ok_or("StatusBarItem.text precisa ser texto")?);
                Ok(())
            }),
        ),
        set(
            "name",
            Rc::new(|entry, value| {
                entry.set_name(opt_string(value).as_deref());
                Ok(())
            }),
        ),
        set(
            "tooltip",
            Rc::new(|entry, value| {
                entry.set_tooltip(opt_string(value).map(Documentation::Text));
                Ok(())
            }),
        ),
        set(
            "color",
            Rc::new(|entry, value| {
                let color = match value {
                    Some(JsData::Str(text)) => Some(StatusBarColor::String(text.clone())),
                    Some(object @ JsData::Object(_)) => Some(StatusBarColor::ThemeColor(theme_color_of(object)?)),
                    _ => None,
                };
                entry.set_color(color);
                Ok(())
            }),
        ),
        set(
            "backgroundColor",
            Rc::new(|entry, value| {
                entry.set_background_color(match value {
                    Some(object @ JsData::Object(_)) => Some(theme_color_of(object)?),
                    _ => None,
                });
                Ok(())
            }),
        ),
        set(
            "command",
            Rc::new(|entry, value| {
                let command = match value {
                    Some(JsData::Str(id)) => Some(StatusBarCommand::Id(id.clone())),
                    Some(object @ JsData::Object(_)) => Some(StatusBarCommand::Command(Rc::new(Command::from_dynamic(&js_to_dynamic(object)).map_err(|error| error.message)?))),
                    value if is_nullish_value(value) => None,
                    _ => return Err("StatusBarItem.command precisa ser texto ou Command".to_owned()),
                };
                entry.set_command(command);
                Ok(())
            }),
        ),
    ];
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        method("show", Rc::new(|entry| entry.show())),
        method("hide", Rc::new(|entry| entry.hide())),
        (
            "dispose".to_owned(),
            0,
            Rc::new(move |this, _| {
                let entry = dispose_items.entry_of(this)?;
                entry.dispose();
                if let Some(JsData::Num(id)) = field(this, "_id") {
                    dispose_items.items.borrow_mut().remove(&(*id as u64));
                }
                Ok(JsData::Undefined)
            }),
        ),
    ];
    let constructor = runner.define_accessor_class(
        ClassSpec {
            name: "StatusBarItem".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("StatusBarItem não tem construtor público: use window.createStatusBarItem".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
    JsData::Handle(Rc::new(constructor))
}

/// O namespace `window`.
pub fn namespace(runner: &NodeRunner, services: Rc<WindowServices>, table: &Rc<DisposableTable>, items: &Rc<StatusBarItems>) -> JsData {
    let mut members: Vec<(String, JsData)> = Vec::new();
    let mut add = |name: &str, length: u32, body: NativeCallback| members.push((name.to_owned(), JsData::Handle(Rc::new(runner.make_function(name, length, body)))));

    for (name, severity) in [("showInformationMessage", Severity::Info), ("showWarningMessage", Severity::Warning), ("showErrorMessage", Severity::Error)] {
        let (runner, services) = (runner.clone(), services.clone());
        add(name, 1, Rc::new(move |args| show_message(&runner, &services, severity, args)));
    }

    if let Some(status_bar) = services.status_bar.clone() {
        let (create_bar, create_items) = (status_bar.clone(), items.clone());
        add(
            "createStatusBarItem",
            3,
            Rc::new(move |args| {
                // `createStatusBarItem(alignmentOrId?, priorityOrAlignment?, priority?)` e sua variante com `id`
                let (id, alignment, priority) = match args.first() {
                    Some(JsData::Str(id)) => (Some(id.clone()), args.get(1), args.get(2)),
                    other => (None, other, args.get(1)),
                };
                let alignment = opt_number(alignment).and_then(|value| StatusBarAlignment::from_value(value as i32));
                let entry = create_bar.create_status_bar_entry(None, id.as_deref(), alignment, opt_number(priority));
                Ok(create_items.add(entry))
            }),
        );
        let (message_bar, message_runner, message_table) = (status_bar, runner.clone(), table.clone());
        add(
            "setStatusBarMessage",
            2,
            Rc::new(move |args| {
                let text = string_arg(args, 0, "setStatusBarMessage: o texto")?;
                let timeout_or_thenable = match args.get(1) {
                    Some(JsData::Num(timeout)) => Some(TimeoutOrThenable::Timeout(*timeout as u64)),
                    Some(thenable @ JsData::Handle(_)) => Some(TimeoutOrThenable::Thenable(thenable_to_promise(&message_runner, thenable.clone()))),
                    _ => None,
                };
                let disposable: Rc<dyn IDisposable> = Rc::new(message_bar.set_status_bar_message(&text, timeout_or_thenable));
                Ok(message_table.to_js(disposable))
            }),
        );
    }
    JsData::Object(members)
}

/// As classes deste módulo: `ThemeColor` e o protótipo de `StatusBarItem`.
pub fn classes(runner: &NodeRunner, items: &Rc<StatusBarItems>, table: &Rc<DisposableTable>) -> Vec<(String, JsData)> {
    let _status_bar_item = define_status_bar_item(runner, items, table);
    let theme_color = runner.define_class(ClassSpec {
        name: "ThemeColor".to_owned(),
        length: 1,
        construct: Rc::new(|args| {
            let id = string_arg(args, 0, "ThemeColor: o id")?;
            Ok(JsData::Object(vec![("id".to_owned(), JsData::Str(id))]))
        }),
        methods: vec![],
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    vec![("ThemeColor".to_owned(), JsData::Handle(Rc::new(theme_color)))]
}
