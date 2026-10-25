//! Teste de ponta a ponta de `vscode.window.tabGroups` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host
//! local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), lê os grupos e as abas, escuta `onDidChangeTabGroups` e
//! `onDidChangeTabs` e fecha abas e grupos com `tabGroups.close`.
//!
//! O lado do ExtHost é o `ExtHostEditorTabs` REAL (portado de `extHostEditorTabs.ts`), ligado pelo host em processo. O lado do workbench
//! é um dublê do `MainThreadEditorTabsShape` ([`Panel`]) que anota `$closeTab`, `$closeGroup` e `$moveTab` e faz o papel da thread
//! principal: empurra `$acceptEditorTabModel`, `$acceptTabGroupUpdate` e `$acceptTabOperation` pelo proxy `ExtHostEditorTabsShape` que o
//! contexto em processo entrega, e o teste confere o que a extensão enxerga.

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{
    AnyInputDto, CustomInputDto, EditorTabDto, EditorTabGroupDto, ExtHostContext, ExtHostEditorTabsShape, IMainContext, MainContext, MainThreadEditorTabsShape,
    NotebookDiffInputDto, NotebookInputDto, TabModelOperationKind, TabOperation, TextDiffInputDto, TextInputDto, WebviewInputDto,
};
use je_workbench::api::common::ext_host_type_converters::EditorGroupColumn;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  const log = (text) => globalThis.__events.push(text);
  globalThis.__events = [];
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const names = (items) => items.map((item) => item.label).join(',');
  const columns = (items) => items.map((item) => item.viewColumn).join(',');
  const flag = (tab) => (tab.isActive ? 'A' : '-') + (tab.isDirty ? 'D' : '-') + (tab.isPinned ? 'P' : '-') + (tab.isPreview ? 'V' : '-');
  const kind = (input) => {
    if (input instanceof vscode.TabInputText) { return 'text:' + input.uri.path; }
    if (input instanceof vscode.TabInputTextDiff) { return 'diff:' + input.original.path + '>' + input.modified.path; }
    if (input instanceof vscode.TabInputCustom) { return 'custom:' + input.uri.path + ':' + input.viewType; }
    if (input instanceof vscode.TabInputNotebook) { return 'notebook:' + input.uri.path + ':' + input.notebookType; }
    if (input instanceof vscode.TabInputNotebookDiff) { return 'notebookdiff:' + input.original.path + '>' + input.modified.path + ':' + input.notebookType; }
    if (input instanceof vscode.TabInputWebview) { return 'webview:' + input.viewType; }
    if (input instanceof vscode.TabInputTerminal) { return 'terminal'; }
    return String(input);
  };
  const describeTab = (tab) => tab.label + '|' + flag(tab) + '|' + kind(tab.input);
  const describeGroup = (group) => group.viewColumn + ':' + group.isActive + ':' + group.tabs.map(describeTab).join(';');
  const tabGroups = vscode.window.tabGroups;

  command('t.tabsNoGroup', () => {
    try { return String(tabGroups.activeTabGroup.viewColumn); } catch (error) { return error.message; }
  });

  command('t.tabsListen', () => {
    context.subscriptions.push(tabGroups.onDidChangeTabGroups((event) => {
      log('groups:opened=' + columns(event.opened) + ';closed=' + columns(event.closed) + ';changed=' + columns(event.changed));
    }));
    context.subscriptions.push(tabGroups.onDidChangeTabs((event) => {
      log('tabs:opened=' + names(event.opened) + ';closed=' + names(event.closed) + ';changed=' + names(event.changed)
        + ';same=' + (event.closed.length > 0 && event.closed[0] === globalThis.__saved));
    }));
    return 'ok';
  });

  command('t.tabsSnapshot', () => JSON.stringify(tabGroups.all.map(describeGroup)));
  command('t.tabsActive', () => describeGroup(tabGroups.activeTabGroup));
  command('t.tabsSave', () => { globalThis.__saved = tabGroups.all[0].tabs[0]; return globalThis.__saved.label; });

  command('t.tabsIdentity', () => JSON.stringify([
    tabGroups.activeTabGroup === tabGroups.all[0],
    tabGroups.all[0].tabs[0] === tabGroups.all[0].activeTab,
    tabGroups.all[0].tabs[0].group === tabGroups.all[0],
    tabGroups.all[0] !== tabGroups.all[1],
    tabGroups.all[1].tabs[0] === tabGroups.all[1].tabs[0],
  ]));

  command('t.tabsInputs', () => JSON.stringify([
    new vscode.TabInputText(vscode.Uri.file('/x/a.txt')).uri.path,
    new vscode.TabInputWebview('v').viewType,
    new vscode.TabInputTerminal() instanceof vscode.TabInputTerminal,
    new vscode.TabInputCustom(vscode.Uri.file('/x/b'), 'c').viewType,
  ]));

  command('t.tabsCloseTab', async () => String(await tabGroups.close(tabGroups.all[1].tabs[1])));
  command('t.tabsCloseTabs', async () => String(await tabGroups.close([tabGroups.all[1].tabs[0], tabGroups.all[1].tabs[1]], true)));
  command('t.tabsCloseGroup', async () => String(await tabGroups.close(tabGroups.all[1])));
  command('t.tabsCloseGroups', async () => String(await tabGroups.close([tabGroups.all[0], tabGroups.all[1]], false)));
  command('t.tabsCloseNothing', async () => String(await tabGroups.close([])));
  command('t.tabsCloseMixed', async () => {
    try { await tabGroups.close([tabGroups.all[0], tabGroups.all[0].tabs[0]]); return 'sem erro'; } catch (error) { return error.message; }
  });

  globalThis.__activated = true;
};
"#;

fn uri(path: &str) -> URI {
    URI::file(path.to_owned(), je_platform::resources::host_platform())
}

fn tab(id: &str, label: &str, input: AnyInputDto, is_active: bool, is_dirty: bool, is_pinned: bool, is_preview: bool) -> EditorTabDto {
    EditorTabDto { id: id.to_owned(), label: label.to_owned(), input, editor_id: None, is_active, is_pinned, is_preview, is_dirty }
}

fn text(path: &str) -> AnyInputDto {
    AnyInputDto::Text(TextInputDto { uri: uri(path) })
}

fn group(group_id: u32, view_column: EditorGroupColumn, is_active: bool, tabs: Vec<EditorTabDto>) -> EditorTabGroupDto {
    EditorTabGroupDto { is_active, view_column, tabs, group_id }
}

/// O modelo completo que o teste empurra: um grupo ativo com duas abas e um grupo com cinco entradas de tipos diferentes.
fn model() -> Vec<EditorTabGroupDto> {
    vec![
        group(
            10,
            0.0,
            true,
            vec![
                tab("t1", "a.txt", text("/proj/a.txt"), true, true, false, false),
                tab("t2", "a.txt (diff)", AnyInputDto::TextDiff(TextDiffInputDto { original: uri("/proj/a.txt"), modified: uri("/proj/b.txt") }), false, false, true, false),
            ],
        ),
        group(
            20,
            1.0,
            false,
            vec![
                tab("t3", "Preview", AnyInputDto::Custom(CustomInputDto { view_type: "hex.view".to_owned(), uri: uri("/proj/c.bin") }), true, false, false, true),
                tab("t4", "bash", AnyInputDto::Terminal, false, false, false, false),
                tab("t5", "Notebook", AnyInputDto::Notebook(NotebookInputDto { notebook_type: "jupyter".to_owned(), uri: uri("/proj/n.ipynb") }), false, false, false, false),
                tab("t6", "Web", AnyInputDto::Webview(WebviewInputDto { view_type: "markdown.preview".to_owned() }), false, false, false, false),
                tab(
                    "t7",
                    "NbDiff",
                    AnyInputDto::NotebookDiff(NotebookDiffInputDto { notebook_type: "jupyter".to_owned(), original: uri("/proj/n.ipynb"), modified: uri("/proj/n2.ipynb") }),
                    false,
                    false,
                    false,
                    false,
                ),
            ],
        ),
    ]
}

const GROUP_ONE: &str = "1:true:a.txt|AD--|text:/proj/a.txt;a.txt (diff)|--P-|diff:/proj/a.txt>/proj/b.txt";
const GROUP_TWO: &str = "2:false:Preview|A--V|custom:/proj/c.bin:hex.view;bash|----|terminal;Notebook|----|notebook:/proj/n.ipynb:jupyter;Web|----|webview:markdown.preview;NbDiff|----|notebookdiff:/proj/n.ipynb>/proj/n2.ipynb:jupyter";

/// O papel da thread principal: o `MainThreadEditorTabsShape` que anota as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostEditorTabsShape>>>,
    log: RefCell<Vec<String>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), log: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostEditorTabsShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadEditorTabsShape for Panel {
    fn rpc_move_tab(&self, tab_id: &str, index: usize, view_column: EditorGroupColumn, preserve_focus: Option<bool>) {
        self.log.borrow_mut().push(format!("moveTab:{tab_id}:{index}:{view_column}:{preserve_focus:?}"));
    }

    fn rpc_close_tab(&self, tab_ids: Vec<String>, preserve_focus: Option<bool>) -> Promise<bool> {
        self.log.borrow_mut().push(format!("closeTab:{}:{preserve_focus:?}", tab_ids.join(",")));
        Promise::resolved(true)
    }

    fn rpc_close_group(&self, group_ids: Vec<u32>, preserve_focus: Option<bool>) -> Promise<bool> {
        let ids: Vec<String> = group_ids.iter().map(u32::to_string).collect();
        self.log.borrow_mut().push(format!("closeGroup:{}:{preserve_focus:?}", ids.join(",")));
        Promise::resolved(true)
    }
}

struct Binder {
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_EDITOR_TABS));
        let panel: Rc<dyn MainThreadEditorTabsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_EDITOR_TABS, panel);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Panel::new();
    (Rc::new(Binder { panel: panel.clone() }), panel)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

/// Empurra o modelo completo e esvazia o laço.
fn push_model(workbench: &Workbench, panel: &Panel) {
    panel.ext_host().rpc_accept_editor_tab_model(model());
    workbench.pump();
}

#[test]
fn active_tab_group_throws_before_the_main_thread_pushes_a_model() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _panel| {
        assert_eq!(run(workbench, "t.tabsNoGroup"), Dynamic::string("Assertion Failed: Argument is `undefined` or `null`."));
    });
}

#[test]
fn the_extension_sees_the_groups_the_tabs_and_their_inputs() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        push_model(workbench, &panel);

        let expected = format!("[\"{GROUP_ONE}\",\"{GROUP_TWO}\"]");
        assert_eq!(run(workbench, "t.tabsSnapshot"), Dynamic::string(expected.as_str()));
        assert_eq!(run(workbench, "t.tabsActive"), Dynamic::string(GROUP_ONE), "o grupo ativo é o do DTO com isActive");
    });
}

#[test]
fn tab_and_group_objects_keep_their_identity_between_reads() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        push_model(workbench, &panel);
        assert_eq!(run(workbench, "t.tabsIdentity"), Dynamic::string("[true,true,true,true,true]"));
    });
}

#[test]
fn tab_input_classes_are_constructible() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _panel| {
        assert_eq!(run(workbench, "t.tabsInputs"), Dynamic::string(r#"["/x/a.txt","v",true,"c"]"#));
    });
}

#[test]
fn model_and_operations_fire_the_change_events_with_the_same_objects() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.tabsListen");
        push_model(workbench, &panel);
        run(workbench, "t.tabsSave");

        // Abre uma aba no índice 1 do grupo 10: ela vira a ativa e a anterior deixa de ser.
        let opened = tab("t9", "new.rs", text("/proj/new.rs"), true, false, false, false);
        panel.ext_host().rpc_accept_tab_operation(TabOperation { kind: TabModelOperationKind::TabOpen, index: 1, tab_dto: opened, group_id: 10, old_index: None });
        workbench.pump();
        let expected_group_one = "1:true:a.txt|-D--|text:/proj/a.txt;new.rs|A---|text:/proj/new.rs;a.txt (diff)|--P-|diff:/proj/a.txt>/proj/b.txt";
        assert_eq!(run(workbench, "t.tabsActive"), Dynamic::string(expected_group_one));

        // Fecha a aba a.txt (índice 0), a mesma que a extensão guardou.
        let closed = tab("t1", "a.txt", text("/proj/a.txt"), false, true, false, false);
        panel.ext_host().rpc_accept_tab_operation(TabOperation { kind: TabModelOperationKind::TabClose, index: 0, tab_dto: closed, group_id: 10, old_index: None });
        workbench.pump();

        // Atualiza a aba do diff (agora suja).
        let updated = tab("t2", "a.txt (diff)", AnyInputDto::TextDiff(TextDiffInputDto { original: uri("/proj/a.txt"), modified: uri("/proj/b.txt") }), false, true, true, false);
        panel.ext_host().rpc_accept_tab_operation(TabOperation { kind: TabModelOperationKind::TabUpdate, index: 1, tab_dto: updated, group_id: 10, old_index: None });
        workbench.pump();

        // O grupo 20 passa a ser o ativo e muda de coluna.
        panel.ext_host().rpc_accept_tab_group_update(group(20, 2.0, true, Vec::new()));
        workbench.pump();
        assert_eq!(run(workbench, "t.tabsIdentity"), Dynamic::string("[false,true,true,true,true]"), "o grupo ativo mudou, as identidades dos demais objetos não");

        assert_eq!(
            events(workbench),
            concat!(
                r#"["groups:opened=1,2;closed=;changed=","#,
                r#""tabs:opened=new.rs;closed=;changed=;same=false","#,
                r#""tabs:opened=;closed=a.txt;changed=;same=true","#,
                r#""tabs:opened=;closed=;changed=a.txt (diff);same=false","#,
                r#""groups:opened=;closed=;changed=3"]"#
            )
        );
    });
}

#[test]
fn a_new_full_model_reuses_the_objects_of_the_groups_and_tabs_that_remain() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.tabsListen");
        push_model(workbench, &panel);
        run(workbench, "t.tabsSave");
        // O mesmo modelo de novo: nada é aberto nem fechado, os grupos são todos `changed`, e o objeto guardado continua o mesmo.
        push_model(workbench, &panel);
        assert_eq!(run(workbench, "t.tabsIdentity"), Dynamic::string("[true,true,true,true,true]"));
        assert_eq!(run(workbench, "t.tabsSave"), Dynamic::string("a.txt"));
        assert_eq!(events(workbench), r#"["groups:opened=1,2;closed=;changed=","groups:opened=;closed=;changed=1,2"]"#);
    });
}

#[test]
fn close_forwards_tabs_and_groups_to_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        push_model(workbench, &panel);

        assert_eq!(run(workbench, "t.tabsCloseTab"), Dynamic::string("true"));
        assert_eq!(run(workbench, "t.tabsCloseTabs"), Dynamic::string("true"));
        assert_eq!(run(workbench, "t.tabsCloseGroup"), Dynamic::string("true"));
        assert_eq!(run(workbench, "t.tabsCloseGroups"), Dynamic::string("true"));
        assert_eq!(run(workbench, "t.tabsCloseNothing"), Dynamic::string("true"), "uma lista vazia resolve sem falar com a thread principal");
        assert_eq!(
            panel.lines(),
            vec!["closeTab:t4:None".to_owned(), "closeTab:t3,t4:Some(true)".to_owned(), "closeGroup:20:None".to_owned(), "closeGroup:10,20:Some(false)".to_owned()]
        );
    });
}

#[test]
fn close_rejects_a_list_that_mixes_tabs_and_groups() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        push_model(workbench, &panel);
        assert_eq!(run(workbench, "t.tabsCloseMixed"), Dynamic::string("Illegal argument: close não aceita uma lista que mistura abas e grupos"));
        assert!(panel.lines().is_empty());
    });
}
