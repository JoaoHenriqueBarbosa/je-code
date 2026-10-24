//! Teste de ponta a ponta de `vscode.window.createTreeView` e `registerTreeDataProvider` (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria uma visão
//! de árvore com um `TreeDataProvider` de classe (com `this`, `getParent` e `onDidChangeTreeData`), usa `TreeItem` e
//! `TreeItemCollapsibleState`, assina a seleção e a visibilidade, muda `title` e `message`, chama `reveal` e dispara o refresh.
//!
//! O lado do ExtHost é o `ExtHostTreeViews` REAL (portado de `extHostTreeViews.ts`), ligado pelo host em processo. O lado do workbench
//! é um dublê do `MainThreadTreeViewsShape` ([`Renderer`]) que faz o papel da thread principal: guarda os registros e os pedidos
//! (`$registerTreeViewDataProvider`, `$reveal`, `$refresh`, `$setTitle`, `$setMessage`) e, quando o teste manda, pede ao ExtHost o que
//! a visão real pediria (`$getChildren`, `$setSelectionAndFocus`, `$setVisible`). O cliente real (`MainThreadTreeViews`) depende da
//! camada de visões da interface, que não está ligada nesta suíte.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, Workbench};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::languages::StringOrMarkdown;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostTreeViewsShape, IMainContext, ItemsToRefresh, MainContext, MainThreadTreeViewsShape, RevealItemInfo, TreeChildrenBatch, TreeViewRegistrationOptions,
};
use je_workbench::common::views::{IRevealOptions, IViewBadge, RevealExpand};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const subscribe = (disposable) => context.subscriptions.push(disposable);
  const command = (id, handler) => subscribe(vscode.commands.registerCommand(id, handler));

  class Node {
    constructor(label, children) { this.label = label; this.children = children || []; }
  }
  const leaf1 = new Node('leaf1');
  const leaf2 = new Node('leaf2');
  const root1 = new Node('root1', [leaf1, leaf2]);
  const root2 = new Node('root2');
  const parents = new Map([[leaf1, root1], [leaf2, root1]]);

  class Provider {
    constructor() {
      this.calls = 0;
      this.roots = [root1, root2];
      this.changed = new vscode.EventEmitter();
      this.onDidChangeTreeData = this.changed.event;
    }
    getTreeItem(node) {
      const item = new vscode.TreeItem(node.label, node.children.length ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
      item.id = node.label;
      item.description = 'desc ' + node.label;
      return item;
    }
    getChildren(node) {
      this.calls++;
      return node ? node.children : this.roots;
    }
    getParent(node) { return parents.get(node); }
  }
  const provider = new Provider();
  const view = vscode.window.createTreeView('je.tree', { treeDataProvider: provider, showCollapseAll: true });
  subscribe(view);
  subscribe(vscode.window.registerTreeDataProvider('je.plain', { getTreeItem: (n) => new vscode.TreeItem(String(n)), getChildren: () => ['a', 'b'] }));
  subscribe(view.onDidChangeSelection((event) => log('selection:' + event.selection.map((node) => node.label).join(',')))
  );
  subscribe(view.onDidChangeVisibility((event) => log('visible:' + event.visible)));
  subscribe(view.onDidExpandElement((event) => log('expand:' + event.element.label)));

  command('t.calls', () => String(provider.calls));
  command('t.state', () => JSON.stringify({
    visible: view.visible,
    selection: view.selection.map((node) => node.label),
    title: view.title,
    message: view.message,
  }));
  command('t.setTitle', () => { view.title = 'Minha árvore'; view.description = 'detalhe'; view.message = 'olá'; return 'ok'; });
  command('t.reveal', async () => { await view.reveal(leaf2, { select: true, focus: true, expand: true }); return 'revealed'; });
  command('t.fire', () => { provider.changed.fire(root1); return 'fired'; });
};
"#;

/// O que o dublê guarda do registro de cada visão.
#[derive(Debug, PartialEq)]
struct Registration {
    view_id: String,
    show_collapse_all: bool,
    can_select_many: bool,
}

/// O papel da thread principal: o `MainThreadTreeViewsShape` que anota os pedidos.
struct Renderer {
    ext_host: RefCell<Option<Rc<dyn ExtHostTreeViewsShape>>>,
    registrations: RefCell<Vec<Registration>>,
    reveals: RefCell<Vec<(String, bool, IRevealOptions)>>,
    refreshes: RefCell<Vec<(String, Option<Vec<String>>)>>,
    titles: RefCell<Vec<(String, String, Option<String>)>>,
    messages: RefCell<Vec<(String, String)>>,
}

impl Renderer {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            ext_host: RefCell::new(None),
            registrations: RefCell::new(Vec::new()),
            reveals: RefCell::new(Vec::new()),
            refreshes: RefCell::new(Vec::new()),
            titles: RefCell::new(Vec::new()),
            messages: RefCell::new(Vec::new()),
        })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostTreeViewsShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }
}

impl IDisposable for Renderer {
    fn dispose(&self) {}
}

impl MainThreadTreeViewsShape for Renderer {
    fn rpc_register_tree_view_data_provider(&self, tree_view_id: &str, options: TreeViewRegistrationOptions) -> Promise<()> {
        self.registrations.borrow_mut().push(Registration { view_id: tree_view_id.to_owned(), show_collapse_all: options.show_collapse_all, can_select_many: options.can_select_many });
        Promise::resolved(())
    }

    fn rpc_refresh(&self, view_id: &str, items_to_refresh: Option<ItemsToRefresh>) -> Promise<()> {
        let handles = items_to_refresh.map(|items| items.iter().map(|(handle, _)| handle.clone()).collect::<Vec<String>>());
        self.refreshes.borrow_mut().push((view_id.to_owned(), handles));
        Promise::resolved(())
    }

    fn rpc_reveal(&self, tree_view_id: &str, item_info: Option<RevealItemInfo>, options: IRevealOptions) -> Promise<()> {
        self.reveals.borrow_mut().push((tree_view_id.to_owned(), item_info.is_some(), options));
        Promise::resolved(())
    }

    fn rpc_set_message(&self, tree_view_id: &str, message: StringOrMarkdown) {
        let text = match message {
            StringOrMarkdown::String(text) => text,
            _ => String::from("<markdown>"),
        };
        self.messages.borrow_mut().push((tree_view_id.to_owned(), text));
    }

    fn rpc_set_title(&self, tree_view_id: &str, title: &str, description: Option<&str>) {
        self.titles.borrow_mut().push((tree_view_id.to_owned(), title.to_owned(), description.map(str::to_owned)));
    }

    fn rpc_set_badge(&self, _tree_view_id: &str, _badge: Option<IViewBadge>) {}

    fn rpc_resolve_drop_file_data(&self, _destination_view_id: &str, _request_id: f64, _data_item_id: &str) -> Promise<VSBuffer> {
        Promise::rejected(Error::new("o dublê não arrasta arquivos"))
    }

    fn rpc_dispose_tree(&self, _tree_view_id: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_log_resolve_tree_node_failure(&self, _extension_id: &str) {}
}

/// Liga o dublê da thread principal ao host: registra o cliente e entrega a ele o proxy do ExtHost.
struct Binder {
    renderer: Rc<Renderer>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.renderer.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_TREE_VIEWS));
        let renderer: Rc<dyn MainThreadTreeViewsShape> = self.renderer.clone();
        context.set(&MainContext::MAIN_THREAD_TREE_VIEWS, renderer);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Renderer>) {
    let renderer = Renderer::new();
    (Rc::new(Binder { renderer: renderer.clone() }), renderer)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        Dynamic::Number(number) => number.to_string(),
        _ => "-".to_owned(),
    }
}

/// `handle|label|collapsibleState|description` de cada item de um lote de filhos.
fn summarize(result: &Promise<Option<Vec<TreeChildrenBatch>>>) -> Vec<String> {
    let batches = match result.result() {
        Some(Ok(Some(batches))) => batches,
        other => panic!("$getChildren não assentou com filhos: {:?}", other.map(|result| result.map(|batches| batches.is_some()))),
    };
    assert_eq!(batches.len(), 1, "um lote por pedido");
    batches[0]
        .children
        .iter()
        .map(|child| {
            let item = child.to_dynamic();
            let label = item.get_str("label");
            format!(
                "{}|{}|{}|{}",
                text_of(&item.get_str("handle")),
                text_of(&label.get_str("label")),
                text_of(&item.get_str("collapsibleState")),
                text_of(&item.get_str("description"))
            )
        })
        .collect()
}

fn children(workbench: &Workbench, renderer: &Renderer, view_id: &str, handles: Option<Vec<String>>) -> Vec<String> {
    let result = renderer.ext_host().rpc_get_children(view_id, handles);
    workbench.pump();
    summarize(&result)
}

#[test]
fn create_tree_view_and_register_tree_data_provider_reach_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |_workbench, renderer| {
        assert_eq!(
            *renderer.registrations.borrow(),
            vec![
                Registration { view_id: "je.tree".to_owned(), show_collapse_all: true, can_select_many: false },
                Registration { view_id: "je.plain".to_owned(), show_collapse_all: false, can_select_many: false },
            ]
        );
    });
}

#[test]
fn get_children_calls_the_provider_with_this_and_keeps_element_identity() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        assert_eq!(
            children(workbench, &renderer, "je.tree", None),
            vec!["1/root1|root1|1|desc root1", "1/root2|root2|0|desc root2"],
            "o TreeItem do getTreeItem vira o ITreeItem: id, label, estado de colapso e descrição"
        );
        // Os filhos de `root1` vêm do mesmo objeto que `getChildren` entregou (o ExtHost guarda o elemento e o devolve ao provedor).
        assert_eq!(
            children(workbench, &renderer, "je.tree", Some(vec!["1/root1".to_owned()])),
            vec!["1/leaf1|leaf1|0|desc leaf1", "1/leaf2|leaf2|0|desc leaf2"]
        );
        assert_eq!(run(workbench, "t.calls"), Dynamic::string("2"), "o `this` do provedor (o contador) sobrevive às chamadas");

        // O provedor sem `getParent` nem evento também registra: `je.plain` devolve elementos de texto.
        assert_eq!(children(workbench, &renderer, "je.plain", None), vec!["0/0:a|a|0|-", "0/0:b|b|0|-"]);
    });
}

#[test]
fn selection_visibility_and_title_flow_between_the_view_and_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        children(workbench, &renderer, "je.tree", None);
        children(workbench, &renderer, "je.tree", Some(vec!["1/root1".to_owned()]));

        renderer.ext_host().rpc_set_visible("je.tree", true);
        renderer.ext_host().rpc_set_selection_and_focus("je.tree", vec!["1/leaf1".to_owned()], "1/leaf1");
        workbench.pump();
        assert_eq!(events(workbench), r#"["visible:true","selection:leaf1"]"#);
        assert_eq!(run(workbench, "t.state"), Dynamic::string(r#"{"visible":true,"selection":["leaf1"],"title":"","message":""}"#));

        assert_eq!(run(workbench, "t.setTitle"), Dynamic::string("ok"));
        assert_eq!(*renderer.titles.borrow(), vec![("je.tree".to_owned(), "Minha árvore".to_owned(), None), ("je.tree".to_owned(), "Minha árvore".to_owned(), Some("detalhe".to_owned()))]);
        assert_eq!(*renderer.messages.borrow(), vec![("je.tree".to_owned(), "olá".to_owned())]);
    });
}

#[test]
fn reveal_sends_the_element_and_the_options_to_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        children(workbench, &renderer, "je.tree", None);
        children(workbench, &renderer, "je.tree", Some(vec!["1/root1".to_owned()]));

        assert_eq!(run(workbench, "t.reveal"), Dynamic::string("revealed"));
        assert_eq!(
            *renderer.reveals.borrow(),
            vec![("je.tree".to_owned(), true, IRevealOptions { select: Some(true), focus: Some(true), expand: Some(RevealExpand::Bool(true)) })],
            "o elemento chega com a identidade que getChildren entregou, e as opções vão como a extensão passou"
        );
    });
}

#[test]
fn on_did_change_tree_data_refreshes_the_changed_item() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, renderer| {
        children(workbench, &renderer, "je.tree", None);

        assert_eq!(run(workbench, "t.fire"), Dynamic::string("fired"));
        // O refresh é adiado por 200 ms de debounce.
        std::thread::sleep(Duration::from_millis(300));
        workbench.pump();
        assert_eq!(*renderer.refreshes.borrow(), vec![("je.tree".to_owned(), Some(vec!["1/root1".to_owned()]))]);
    });
}
