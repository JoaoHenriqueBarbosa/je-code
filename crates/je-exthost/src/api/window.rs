//! `vscode.window`: as mensagens (`showInformationMessage`, `showWarningMessage`, `showErrorMessage`), a barra de status
//! (`createStatusBarItem`, `setStatusBarMessage`), os seletores (`showQuickPick`, `showInputBox`), os canais de saída
//! (`createOutputChannel`) e o progresso (`withProgress`), mais as classes `vscode.ThemeColor` e `vscode.StatusBarItem` (esta só como
//! protótipo dos itens que o host entrega).
//!
//! As mensagens são o porte de `extHostMessageService.ts` (a lógica de `showMessage`: opções, itens, `isCloseAffordance`) sobre o
//! proxy `MainThreadMessageService`; a barra de status liga o `ExtHostStatusBar` portado; os seletores ligam o `ExtHostQuickOpen`
//! (módulo `quick_input`), os canais de saída o `ExtHostOutputService` (módulo `output_channel`) e o progresso o `ExtHostProgress`
//! (módulo `progress`). Cada grupo só entra em `vscode.window` se o binder registrou o cliente `MainThread*` de que ele depende.
//! Os editores de texto (`activeTextEditor`, `visibleTextEditors`, `showTextDocument` e os eventos `onDidChange*TextEditor*`) são o
//! módulo `text_editors`, sobre o `ExtHostDocumentsAndEditors` e o `ExtHostEditors`; como eles têm acessores, o namespace é uma
//! instância de classe nativa (`accessor_namespace`) com as funções no protótipo.
//!
//! Os terminais (`createTerminal`, `terminals`, `activeTerminal`, `onDidOpenTerminal`, `onDidCloseTerminal`,
//! `onDidChangeActiveTerminal`, `onDidChangeTerminalState`, `registerTerminalProfileProvider`, `vscode.Terminal`) são o módulo
//! `terminal`, sobre o `ExtHostTerminalService`; as árvores (`createTreeView`, `registerTreeDataProvider`, `vscode.TreeView`,
//! `vscode.TreeItem`) são o módulo `tree_views`, sobre o `ExtHostTreeViews`. As funções que recebem objetos da extensão (o `pty`, o
//! provedor de árvore) são funções POR REFERÊNCIA, próprias do namespace (o objeto de `accessor_namespace` com campos de função).
//! `createQuickPick` e `createInputBox` (`vscode.QuickPick`, `vscode.InputBox`, com `items`, `activeItems`, `selectedItems`, `value`,
//! `placeholder`, `busy`, `canSelectMany`, `buttons`, `show`, `hide`, `dispose` e os eventos de aceitação, valor, seleção, ocultação e
//! botão) são o módulo `quick_session`, sobre as sessões do `ExtHostQuickOpen`.
//!
//! Falta (depende de serviços do host ainda não ligados, sem fachada): `createTextEditorDecorationType` e
//! `TextEditor#insertSnippet`/`setDecorations` (ver o
//! cabeçalho de `text_editors.rs`); `tabGroups` e `createWebviewPanel`; o que falta de terminal e de árvore está nos cabeçalhos de
//! `terminal.rs` e `tree_views.rs`. Os registros de provedores de
//! linguagem (`registerHoverProvider`...) e os diagnósticos não são de `window`: estão em `vscode.languages` (módulo `languages`).
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
//! - Os seletores, os canais de saída e o progresso têm as divergências listadas nos cabeçalhos de `quick_input.rs`,
//!   `output_channel.rs` e `progress.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::themables::ThemeColor;
use je_platform::log::LogServiceRef;
use je_workbench::api::common::ext_host_output::ExtHostOutputService;
use je_workbench::api::common::ext_host_progress::ExtHostProgress;
use je_workbench::api::common::ext_host_protocol::{MainThreadMessageOptions, MainThreadMessageServiceShape, MessageCommandDto, StatusBarColor};
use je_workbench::api::common::ext_host_quick_open::ExtHostQuickOpen;
use je_workbench::api::common::ext_host_status_bar::{ExtHostStatusBar, ExtHostStatusBarEntry, StatusBarCommand, TimeoutOrThenable};
use je_workbench::api::common::ext_host_terminal_service::{WorkerExtHostTerminalService, WorkerTerminalCreation};
use je_workbench::api::common::ext_host_terminal_shell_integration::ExtHostTerminalShellIntegration;
use je_workbench::api::common::ext_host_tree_views::ExtHostTreeViews;
use je_workbench::api::common::ext_host_types::{Command, Documentation, StatusBarAlignment};
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{promise_to_js, settle_js_result};
use crate::api::disposable::DisposableTable;
use crate::api::events::EventTables;
use crate::api::js_object::JsObjects;
use crate::api::notebooks::{self, NotebookApi};
use crate::api::output_channel::{self, OutputChannels};
use crate::api::progress::{self, ProgressReporters};
use crate::api::quick_input;
use crate::api::quick_session::{self, QuickSessions};
use crate::api::terminal::{self, TerminalObjects};
use crate::api::tabs::{self, TabsApi};
use crate::api::text_editors::{self, TextEditorApi, TextObjects};
use crate::api::tree_views::{self, TreeViewObjects};
use crate::api::accessor_namespace;
use crate::convert::{api_to_js, dynamic_to_js, field, js_to_dynamic, opt_bool, opt_number, opt_string, string_arg};

/// Os serviços do host que o namespace usa. Cada grupo de membros de `vscode.window` só existe quando o serviço dele existe: o
/// binder registra os clientes `MainThread*` que quer ligar.
pub struct WindowServices {
    /// Sem o cliente `MainThreadMessageService` não há `showInformationMessage`, `showWarningMessage` nem `showErrorMessage`.
    pub message_service: Option<Rc<dyn MainThreadMessageServiceShape>>,
    pub log_service: LogServiceRef,
    /// Sem o cliente `MainThreadStatusBar` do workbench não há barra de status, e os dois membros dela não existem em `vscode.window`.
    pub status_bar: Option<Rc<ExtHostStatusBar>>,
    /// Sem o cliente `MainThreadQuickOpen` não há `showQuickPick` nem `showInputBox`.
    pub quick_open: Option<Rc<ExtHostQuickOpen>>,
    /// Sem o cliente `MainThreadOutputService` (e o ambiente de saída do binder) não há `createOutputChannel`.
    pub output: Option<Rc<ExtHostOutputService>>,
    /// Sem o cliente `MainThreadProgress` não há `withProgress`.
    pub progress: Option<Rc<ExtHostProgress>>,
    /// Sem o cliente `MainThreadTerminalService` não há `createTerminal`, `terminals` nem os eventos de terminal.
    pub terminal: Option<Rc<WorkerExtHostTerminalService>>,
    /// Sem o cliente `MainThreadTerminalShellIntegration` não há `Terminal#shellIntegration` nem os eventos de execução de comando.
    pub terminal_shell_integration: Option<Rc<ExtHostTerminalShellIntegration<WorkerTerminalCreation>>>,
    /// Sem o cliente `MainThreadTreeViews` não há `createTreeView` nem `registerTreeDataProvider`.
    pub tree_views: Option<Rc<ExtHostTreeViews>>,
    /// Sem os clientes `MainThreadDocuments` e `MainThreadTextEditors` não há `activeTextEditor`, `visibleTextEditors`,
    /// `showTextDocument` nem os eventos de editor.
    pub text_editors: Option<TextEditorApi>,
    /// Sem o cliente `MainThreadEditorTabs` não há `tabGroups`.
    pub tabs: Option<TabsApi>,
    /// Sem os clientes `MainThreadNotebook*` não há `activeNotebookEditor`, `visibleNotebookEditors`, `showNotebookDocument` nem os eventos.
    pub notebooks: Option<NotebookApi>,
}

/// `isMessageItem(item)`: um objeto com `title` não vazio.
fn is_message_item(item: Option<&JsData>) -> bool {
    matches!(item.and_then(|item| field(item, "title")), Some(JsData::Str(title)) if !title.is_empty())
}

/// `ExtHostMessageService.showMessage` sem a extensão (ver as divergências do módulo): `args` são `(message, optionsOrFirstItem?,
/// ...rest)`.
fn show_message(runner: &NodeRunner, message_service: &Rc<dyn MainThreadMessageServiceShape>, log_service: &LogServiceRef, severity: Severity, args: &[JsData]) -> Result<JsData, String> {
    let message = string_arg(args, 0, "a mensagem")?;
    let options_or_first_item = args.get(1);
    let rest: &[JsData] = args.get(2..).unwrap_or(&[]);

    let mut options = MainThreadMessageOptions::default();
    let items: Vec<JsData> = if matches!(options_or_first_item, Some(JsData::Str(_))) || is_message_item(options_or_first_item) {
        options_or_first_item.into_iter().chain(rest.iter()).cloned().collect()
    } else {
        if let Some(provided) = options_or_first_item.filter(|provided| matches!(provided, JsData::Object(_))) {
            options.modal = opt_bool(field(provided, "modal"));
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
                        log_service.warn("Only one message item can have 'isCloseAffordance'");
                    } else {
                        has_close_affordance = true;
                    }
                }
            }
            _ => log_service.warn("Invalid message item"),
        }
    }

    let chosen = message_service.rpc_show_message(severity, &message, options, commands);
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

/// O nome de um item sem `name` (no original, `"{label} (Extension)"` com o nome da extensão chamadora).
fn default_item_name() -> String {
    localize("extensionLabel", "{0} (Extension)", &["Extension"])
}

fn status_bar_color_to_js(color: StatusBarColor) -> JsData {
    match color {
        StatusBarColor::String(text) => JsData::Str(text),
        StatusBarColor::ThemeColor(color) => api_to_js(&color),
    }
}

pub(crate) fn theme_color_of(value: &JsData) -> Result<ThemeColor, String> {
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
                entry.set_name(Some(opt_string(value).filter(|name| !name.is_empty()).unwrap_or_else(default_item_name).as_str()));
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

/// O que o namespace `window` guarda de cada chamada que devolve um objeto vivo para a extensão: os itens de barra de status, os canais de
/// saída e os `Progress` das tarefas em andamento (os objetos JavaScript carregam só o id).
#[derive(Default)]
pub struct WindowState {
    pub status_items: Rc<StatusBarItems>,
    pub output_channels: Rc<OutputChannels>,
    pub progress_reporters: Rc<ProgressReporters>,
    /// Os documentos, editores e construtores de edição de texto (compartilhados com `workspace`).
    pub text_objects: Rc<TextObjects>,
    /// Os terminais que o JavaScript segura.
    pub terminal_objects: Rc<TerminalObjects>,
    /// As visões de árvore que o JavaScript segura.
    pub tree_view_objects: Rc<TreeViewObjects>,
    /// As sessões de `createQuickPick` e `createInputBox` que o JavaScript segura.
    pub quick_sessions: Rc<QuickSessions>,
}

/// O namespace `window`.
pub fn namespace(runner: &NodeRunner, services: Rc<WindowServices>, table: &Rc<DisposableTable>, events: &Rc<EventTables>, state: &WindowState) -> JsData {
    // Os acessores (`activeTextEditor`, `onDidChangeActiveTextEditor`...) pedem o protótipo de uma classe nativa: o namespace é a
    // instância dela (ver `accessor_namespace`), e as funções moram no protótipo.
    let mut methods: Vec<(String, u32, MethodCallback)> = Vec::new();
    let mut getters: Vec<(String, MethodCallback)> = Vec::new();
    let mut add = |name: &str, length: u32, body: NativeCallback| methods.push((name.to_owned(), length, Rc::new(move |_: &JsData, args: &[JsData]| body(args))));

    if let Some(message_service) = services.message_service.clone() {
        for (name, severity) in [("showInformationMessage", Severity::Info), ("showWarningMessage", Severity::Warning), ("showErrorMessage", Severity::Error)] {
            let (runner, message_service, log_service) = (runner.clone(), message_service.clone(), services.log_service.clone());
            add(name, 1, Rc::new(move |args| show_message(&runner, &message_service, &log_service, severity, args)));
        }
    }

    if let Some(quick_open) = services.quick_open.clone() {
        let (pick_runner, pick_quick_open, pick_events) = (runner.clone(), quick_open.clone(), events.clone());
        add("showQuickPick", 3, Rc::new(move |args| quick_input::show_quick_pick(&pick_runner, &pick_quick_open, &pick_events, args)));
        let (input_runner, input_quick_open, input_events) = (runner.clone(), quick_open.clone(), events.clone());
        add("showInputBox", 2, Rc::new(move |args| quick_input::show_input_box(&input_runner, &input_quick_open, &input_events, args)));
        let (create_quick_open, create_sessions) = (quick_open.clone(), state.quick_sessions.clone());
        add("createQuickPick", 0, Rc::new(move |_| quick_session::create_quick_pick(&create_quick_open, &create_sessions)));
        let create_sessions = state.quick_sessions.clone();
        add("createInputBox", 0, Rc::new(move |_| quick_session::create_input_box(&quick_open, &create_sessions)));
    }

    if let Some(output) = services.output.clone() {
        let create_channels = state.output_channels.clone();
        add("createOutputChannel", 2, Rc::new(move |args| output_channel::create_output_channel(&output, &create_channels, args)));
    }

    if let Some(ext_host_progress) = services.progress.clone() {
        let (progress_runner, reporters, progress_events) = (runner.clone(), state.progress_reporters.clone(), events.clone());
        add("withProgress", 2, Rc::new(move |args| progress::with_progress(&progress_runner, &ext_host_progress, &reporters, &progress_events, args)));
    }

    if let Some(status_bar) = services.status_bar.clone() {
        let (create_bar, create_items) = (status_bar.clone(), state.status_items.clone());
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
                // O host não sabe qual extensão chamou: sem `id`, o item ganha um identificador próprio, e sem `name`, o nome padrão
                // (o original os deriva da extensão, que aqui não existe).
                let id = id.unwrap_or_else(|| format!("je.exthost.statusbar.{}", create_items.next.get() + 1));
                let entry = create_bar.create_status_bar_entry(None, Some(id.as_str()), alignment, opt_number(priority));
                entry.set_name(Some(default_item_name().as_str()));
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
    if let Some(text_editors) = &services.text_editors {
        let (editor_methods, editor_getters) = text_editors::window_members(runner, text_editors, table);
        methods.extend(editor_methods);
        getters.extend(editor_getters);
    }
    // Os membros de editor de notebook (`activeNotebookEditor`, `showNotebookDocument`...) vêm do módulo `notebooks`.
    if let Some(api) = &services.notebooks {
        let (notebook_methods, notebook_getters) = notebooks::window_members(runner, api, table);
        methods.extend(notebook_methods);
        getters.extend(notebook_getters);
    }
    // Os membros que recebem objetos da extensão (o `pty`, o provedor de árvore) são funções por referência, próprias do namespace.
    let mut references: Vec<(String, JsData)> = Vec::new();
    let js = JsObjects::new(runner);
    if let Some(service) = &services.terminal {
        let members = terminal::window_members(runner, &js, service, services.terminal_shell_integration.as_ref(), &state.terminal_objects, table, events);
        references.extend(members.references);
        getters.extend(members.getters);
    }
    if let Some(views) = &services.tree_views {
        references.extend(tree_views::window_members(runner, &js, views, &state.tree_view_objects, table, events));
    }
    if let Some(tabs) = &services.tabs {
        references.push(tabs::window_member(runner, tabs, table));
    }
    match accessor_namespace(runner, "window", methods, getters) {
        JsData::Object(mut fields) => {
            fields.extend(references);
            JsData::Object(fields)
        }
        other => other,
    }
}

/// As classes deste módulo: `ThemeColor`, o protótipo de `StatusBarItem`, os de `OutputChannel` e `LogOutputChannel`, o de `Progress` e
/// os de `TextDocument`, `TextEditor` e `TextEditorEdit`.
pub fn classes(runner: &NodeRunner, state: &WindowState, table: &Rc<DisposableTable>) -> Vec<(String, JsData)> {
    let _status_bar_item = define_status_bar_item(runner, &state.status_items, table);
    output_channel::define_classes(runner, &state.output_channels, table);
    progress::define_class(runner, &state.progress_reporters);
    text_editors::define_classes(runner, &state.text_objects);
    terminal::define_classes(runner, &state.terminal_objects);
    let tree_view_classes = tree_views::define_classes(runner, &state.tree_view_objects, table);
    quick_session::define_classes(runner, &state.quick_sessions, table);
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
    let mut classes = vec![
        ("ThemeColor".to_owned(), JsData::Handle(Rc::new(theme_color))),
        ("TerminalProfile".to_owned(), terminal::terminal_profile_class(runner)),
        ("TerminalLink".to_owned(), terminal::terminal_link_class(runner)),
        ("TreeItem".to_owned(), tree_views::tree_item_class(runner)),
    ];
    classes.extend(tree_view_classes);
    classes
}
