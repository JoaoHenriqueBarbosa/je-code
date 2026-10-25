//! Porte de `src/vs/workbench/contrib/files/test/browser/explorerView.test.ts` (suíte `Files - ExplorerView`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`assert.deepStrictEqual` sobre `ExplorerItem[]`.** Os itens são os mesmos objetos: identidade (`Rc::ptr_eq`).
//! - **`createStat.call(this, ...)`.** O título completo do caso do mocha é o primeiro argumento de `to_resource`.
//! - **`CompressedNavigationController`.** Sem o `templateData` de DOM (ver `explorer_viewer.rs`); o caso exercita o
//!   índice, a contagem e o item corrente.
//! - **`TestEnvironmentService`.** O [`TestEnvironmentService`] do porte.
//! - **A suíte `context menu`.** Monta o [`ExplorerView`] com a árvore de teste do original ([`ExplorerViewHarness`], ver as
//!   divergências em `support/explorer_view_support.rs`). O `dispatchEvent` do `contextmenu` é o [`ExplorerView::on_context_menu`]
//!   com o evento que a árvore emitiria; `await shown` é a promessa dele; `menu` é o delegado que o `IContextMenuService` de teste
//!   guardou (`menu.base.on_hide(true)` é o `menu.onHide(true)`); `menu?.menuActionOptions?.arg` e `menu?.getActionsContext?.()`
//!   são lidos como `URI`.
//! - **Os dois casos de `refresh`.** O `refreshGate` é o `DeferredPromise` que o `getChildren` da fonte de dados de teste espera
//!   ([`ExplorerViewHarness::close_refresh_gate`]); `selectedBeforeRefresh` é o `Cell` que o `then` da seleção liga, e o registro é
//!   a tupla `(selectedBeforeRefresh, focus, selection)`.
//! - **`keeps the compressed-folder navigation target ...`.** Os controladores são os do `controllers` do dublê de `FilesRenderer`
//!   ([`MockCompressedNavigationController`] com `current = parent`, `items = [parent, child]`, `index = 0` e `count = 2`). A entrada
//!   da árvore do porte é a lista dos filhos da raiz e fica fixa, então o `tree.updateChildren(root)` é refazer a entrada com o
//!   estado da vista (`set_input(root.children(), view_state)`), como o `refresh` da vista faz.
//! - **`opened`.** O `tree.onDidOpen` registrado depois do `setup`; as seleções do menu não abrem nada.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "editor_input_services/mod.rs"]
mod editor_input_services;
mod support;
#[path = "support/anything_quick_access_support.rs"]
mod anything_quick_access_support;
#[path = "support/explorer_view_support.rs"]
mod explorer_view_support;
#[path = "support/extension_service_doubles.rs"]
mod extension_service_doubles;
#[path = "support/main_thread_support.rs"]
mod main_thread_support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "../../je-editor/tests/support/test_clipboard_service.rs"]
mod test_clipboard_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use explorer_view_support::{ExplorerViewHarness, MockCompressedNavigationController};
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::{Field, URI, UriChange};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{join_path, uri_file};
use je_platform::workspace::{IWorkspace, WorkbenchState, WorkspaceFolder};
use je_workbench::contrib::files::browser::views::explorer_decorations_provider::{LIST_INVALID_ITEM_FOREGROUND, provide_decorations};
use je_workbench::contrib::files::browser::views::explorer_view::{self, CompressedNavigationControllerProvider, should_preserve_workspace_name_case};
use je_workbench::contrib::files::browser::views::explorer_viewer::{CompressedNavigationController, ICompressedNavigationController};
use je_workbench::contrib::files::browser_files::{ExplorerReveal, IExplorerView};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::services::decorations::decorations::{DecorationData, DecorationLetter};
use je_workbench::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestFileService};

/// `join` do `path` nativo.
fn join(paths: &[&str]) -> String {
    PathModule::native(&ProcessEnvironment).join(paths)
}

/// `btoa(text)` para o texto ASCII do título de um caso.
fn btoa(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, byte)| acc | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `toResource.call(this, path)`.
fn to_resource(full_title: &str, path: &str) -> URI {
    uri_file(&join(&["/", &btoa(full_title), path]))
}

/// Os serviços do corpo da suíte.
struct Services {
    file_service: Rc<dyn IFileService>,
    config_service: Rc<dyn IConfigurationService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
}

impl Services {
    fn new() -> Self {
        Self { file_service: TestFileService::new(), config_service: TestConfigurationService::new(None), files_config_service: Rc::new(NullFilesConfigurationService) }
    }

    /// `createStat.call(this, path, name, isFolder, hasChildren, size, mtime, isSymLink, isUnknown)`.
    #[allow(clippy::too_many_arguments)]
    fn create_stat(&self, title: &str, path: &str, name: &str, is_folder: bool, mtime: f64, is_sym_link: bool, is_unknown: bool) -> Rc<ExplorerItem> {
        ExplorerItem::new(
            to_resource(title, path),
            self.file_service.clone(),
            self.config_service.clone(),
            self.files_config_service.clone(),
            None,
            Some(is_folder),
            Some(is_sym_link),
            Some(false),
            Some(false),
            Some(name.to_string()),
            Some(mtime),
            is_unknown,
        )
    }
}

/// O `noNavigationController` do caso `getContext`.
struct NoNavigationController;

impl CompressedNavigationControllerProvider for NoNavigationController {
    fn get_compressed_navigation_controller(&self, _stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<dyn ICompressedNavigationController>>> {
        None
    }
}

/// `assert.deepStrictEqual(actual, expected)` sobre listas de itens (a identidade de cada um).
fn assert_same_items(actual: &[Rc<ExplorerItem>], expected: &[&Rc<ExplorerItem>]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!(Rc::ptr_eq(a, e));
    }
}

#[test]
fn get_context() {
    const TITLE: &str = "Files - ExplorerView getContext";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;
    let s1 = services.create_stat(TITLE, "/", "/", true, d, false, false);
    let s2 = services.create_stat(TITLE, "/path", "path", true, d, false, false);
    let s3 = services.create_stat(TITLE, "/path/to", "to", true, d, false, false);
    let s4 = services.create_stat(TITLE, "/path/to/stat", "stat", false, d, false, false);
    let no_navigation_controller = NoNavigationController;

    assert_same_items(&explorer_view::get_context(&[s1.clone()], &[s2.clone(), s3.clone(), s4.clone()], true, &no_navigation_controller), &[&s2, &s3, &s4]);
    assert_same_items(&explorer_view::get_context(&[s1.clone()], &[s1.clone(), s3.clone(), s4.clone()], true, &no_navigation_controller), &[&s1, &s3, &s4]);
    assert_same_items(&explorer_view::get_context(&[s1.clone()], &[s3.clone(), s1.clone(), s4.clone()], false, &no_navigation_controller), &[&s1]);
    assert_same_items(&explorer_view::get_context(&[], &[s3.clone(), s1.clone(), s4.clone()], false, &no_navigation_controller), &[]);
    assert_same_items(&explorer_view::get_context(&[], &[s3.clone(), s1.clone(), s4.clone()], true, &no_navigation_controller), &[&s3, &s1, &s4]);
}

/// `resources(items)`: `items.map(item => item.resource)`.
fn resources(items: &[Rc<ExplorerItem>]) -> Vec<URI> {
    items.iter().map(|item| item.resource()).collect()
}

/// `tree.onDidOpen(e => opened.push(e.element))`, registrado depois do `setup`.
fn track_opened(guard: &LeakGuard, harness: &ExplorerViewHarness) -> Rc<RefCell<Vec<Rc<ExplorerItem>>>> {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let sink = opened.clone();
    guard.add(harness.view.tree().on_did_open().listen(move |event| sink.borrow_mut().push(event.element.clone())));
    opened
}

/// `assertContext(expected, clicked)`.
fn assert_context(harness: &ExplorerViewHarness, opened: &RefCell<Vec<Rc<ExplorerItem>>>, expected: &[&Rc<ExplorerItem>], clicked: &Rc<ExplorerItem>) {
    let expected: Vec<URI> = expected.iter().map(|item| item.resource()).collect();
    assert_eq!(resources(&harness.view.tree().get_selection()), expected);
    assert_eq!(resources(&harness.view.get_context(true)), expected);
    assert_eq!(harness.menu_target(), Some(clicked.resource()));
    assert_eq!(harness.menu_selection(), Some(expected));
    assert_eq!(resources(&opened.borrow()), Vec::<URI>::new());
}

/// O corpo dos casos assíncronos da suíte `context menu`: o `setup` roda antes e o guarda de vazamento fecha depois.
///
/// O guarda é compartilhado (`Rc`) e este fecho guarda a posse dele até o caso terminar: o `teardown` do mocha roda depois
/// do corpo do teste. Se o caso recebesse o guarda por valor e não o usasse, o `async move` do caso não o capturaria e ele
/// seria descartado ao devolver o futuro, antes de o corpo rodar, descartando a vista e a árvore (`Invalid index N`).
fn run_context_menu_case<F, Fut>(case: F)
where
    F: FnOnce(Rc<LeakGuard>, Rc<ExplorerViewHarness>) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    run_with_real_timers(async move {
        let guard = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
        let harness = Rc::new(ExplorerViewHarness::new(&guard));
        harness.setup().await;
        case(guard.clone(), harness).await;
        drop(guard);
    });
}

#[test]
fn context_menu_waits_for_a_newly_queued_tree_refresh_before_selecting_a_folder() {
    run_context_menu_case(|_guard, h| async move {
        h.view.set_visible(true);
        h.view.set_tree_input().await.unwrap();
        let tree = h.view.tree();
        tree.set_focus(&[h.target.clone()], None);
        tree.set_selection(&[h.target.clone()], None);
        let refresh_gate = h.close_refresh_gate();
        let refresh = h.view.set_tree_input();
        let selected = Rc::new(Cell::new(false));
        let selection = h.view.select_resource(Some(&h.folder.resource()), Some(ExplorerReveal::Mode("force".to_string())), None).then({
            let selected = selected.clone();
            move |_| selected.set(true)
        });
        timeout(0).await.unwrap();
        let selected_before_refresh = selected.get();
        refresh_gate.complete(()).await.unwrap();
        Promise::all(vec![refresh, selection]).await.unwrap();

        assert_eq!(
            (selected_before_refresh, resources(&tree.get_focus()), resources(&tree.get_selection())),
            (false, vec![h.folder.resource()], vec![h.folder.resource()])
        );
    });
}

#[test]
fn context_menu_preserves_folder_selection_after_reopening_a_hidden_explorer() {
    run_context_menu_case(|_guard, h| async move {
        h.view.set_visible(true);
        h.view.set_tree_input().await.unwrap();
        let tree = h.view.tree();
        tree.set_focus(&[h.target.clone()], None);
        tree.set_selection(&[h.target.clone()], None);
        h.view.set_visible(false);
        let refresh_gate = h.close_refresh_gate();
        h.view.set_visible(true);
        let selected = Rc::new(Cell::new(false));
        let selection = h.view.select_resource(Some(&h.folder.resource()), Some(ExplorerReveal::Mode("force".to_string())), None).then({
            let selected = selected.clone();
            move |_| selected.set(true)
        });
        timeout(0).await.unwrap();
        let selected_before_refresh = selected.get();
        refresh_gate.complete(()).await.unwrap();
        selection.await.unwrap();

        assert_eq!(
            (selected_before_refresh, resources(&tree.get_focus()), resources(&tree.get_selection())),
            (false, vec![h.folder.resource()], vec![h.folder.resource()])
        );
    });
}

/// Os casos `selects an unselected item with ${selectionSize} previously selected items`.
fn selects_an_unselected_item(selection_size: usize) {
    run_context_menu_case(move |guard, h| async move {
        let opened = track_opened(&guard, &h);
        let previous: Vec<Rc<ExplorerItem>> = [h.first.clone(), h.second.clone()].into_iter().take(selection_size).collect();
        h.view.tree().set_selection(&previous, None);

        h.show_context_menu(Some(&h.target), false).await.unwrap();

        assert_context(&h, &opened, &[&h.target], &h.target);
    });
}

#[test]
fn context_menu_selects_an_unselected_item_with_0_previously_selected_items() {
    selects_an_unselected_item(0);
}

#[test]
fn context_menu_selects_an_unselected_item_with_1_previously_selected_items() {
    selects_an_unselected_item(1);
}

#[test]
fn context_menu_selects_an_unselected_item_with_2_previously_selected_items() {
    selects_an_unselected_item(2);
}

/// Os casos `preserves ${selectionSize} selected items when the target belongs to the selection`.
fn preserves_selected_items(selection_size: usize) {
    run_context_menu_case(move |guard, h| async move {
        let opened = track_opened(&guard, &h);
        let selection: Vec<Rc<ExplorerItem>> = [h.first.clone(), h.second.clone()].into_iter().take(selection_size).collect();
        h.view.tree().set_selection(&selection, None);

        h.show_context_menu(Some(&h.first), false).await.unwrap();

        let expected: Vec<&Rc<ExplorerItem>> = selection.iter().collect();
        assert_context(&h, &opened, &expected, &h.first);
    });
}

#[test]
fn context_menu_preserves_1_selected_items_when_the_target_belongs_to_the_selection() {
    preserves_selected_items(1);
}

#[test]
fn context_menu_preserves_2_selected_items_when_the_target_belongs_to_the_selection() {
    preserves_selected_items(2);
}

#[test]
fn context_menu_replaces_a_selection_hidden_in_a_collapsed_folder() {
    run_context_menu_case(|guard, h| async move {
        let opened = track_opened(&guard, &h);
        let tree = h.view.tree();
        tree.set_selection(&[h.first.clone(), h.second.clone()], None);
        tree.collapse(&h.folder, false);
        assert_same_items(&tree.get_selection(), &[&h.first, &h.second]);

        h.show_context_menu(Some(&h.target), false).await.unwrap();

        assert_context(&h, &opened, &[&h.target], &h.target);
    });
}

#[test]
fn context_menu_keeps_the_compressed_folder_navigation_target_when_replacing_the_selection() {
    run_context_menu_case(|_guard, h| async move {
        let parent = h.create_item("/workspace/compact", true, Some(&h.root));
        let child = h.create_item("/workspace/compact/child", true, Some(&parent));
        h.root.add_child(&parent);
        parent.add_child(&child);
        let tree = h.view.tree();
        // tree.updateOptions({ compressionEnabled: true }); await tree.updateChildren(root);
        tree.set_compression_enabled(true);
        tree.widget().set_input(h.root.children(), Some(tree.get_view_state())).await.unwrap();
        // `current = parent`, `items = [parent, child]`, `index = 0`, `count = 2`
        let controller = MockCompressedNavigationController::new(parent.clone(), vec![parent.clone(), child.clone()], 0, 2);
        h.set_compressed_navigation_controllers(&parent, vec![controller.clone()]);
        h.set_compressed_navigation_controllers(&child, vec![controller]);
        tree.set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(Some(&child), false).await.unwrap();

        assert_same_items(&tree.get_selection(), &[&child]);
        assert_same_items(&h.view.get_context(true), &[&parent]);
        assert_eq!(h.menu_target(), Some(parent.resource()));
    });
}

#[test]
fn context_menu_selects_the_keyboard_context_menu_target_without_opening_it() {
    run_context_menu_case(|guard, h| async move {
        let opened = track_opened(&guard, &h);
        h.view.tree().set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(Some(&h.target), true).await.unwrap();

        assert_context(&h, &opened, &[&h.target], &h.target);
    });
}

#[test]
fn context_menu_preserves_multi_selection_for_a_keyboard_context_menu_on_a_selected_item() {
    run_context_menu_case(|guard, h| async move {
        let opened = track_opened(&guard, &h);
        h.view.tree().set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(Some(&h.first), true).await.unwrap();

        assert_context(&h, &opened, &[&h.first, &h.second], &h.first);
    });
}

#[test]
fn context_menu_preserves_the_selection_when_cancelling_the_context_menu() {
    run_context_menu_case(|guard, h| async move {
        let opened = track_opened(&guard, &h);
        h.view.tree().set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(Some(&h.first), false).await.unwrap();
        h.menu().expect("o menu foi mostrado").base.on_hide(true);

        assert_context(&h, &opened, &[&h.first, &h.second], &h.first);
    });
}

#[test]
fn context_menu_keeps_the_newly_selected_target_when_cancelling_the_context_menu() {
    run_context_menu_case(|guard, h| async move {
        let opened = track_opened(&guard, &h);
        h.view.tree().set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(Some(&h.target), false).await.unwrap();
        h.menu().expect("o menu foi mostrado").base.on_hide(true);

        assert_context(&h, &opened, &[&h.target], &h.target);
    });
}

#[test]
fn context_menu_leaves_the_selection_unchanged_for_an_editable_input() {
    run_context_menu_case(|_guard, h| async move {
        let tree = h.view.tree();
        tree.set_selection(&[h.first.clone(), h.second.clone()], None);

        let input = h.append_input();
        h.view.on_context_menu(&h.context_menu_event(Some(&h.target), false, Some(input))).await.unwrap();

        assert_same_items(&tree.get_selection(), &[&h.first, &h.second]);
        assert!(h.menu().is_none());
    });
}

#[test]
fn context_menu_preserves_the_background_context_menu() {
    run_context_menu_case(|_guard, h| async move {
        let tree = h.view.tree();
        tree.set_selection(&[h.first.clone(), h.second.clone()], None);

        h.show_context_menu(None, false).await.unwrap();

        assert_same_items(&tree.get_selection(), &[&h.first, &h.second]);
        assert_eq!(h.menu_target(), Some(h.root.resource()));
        assert_eq!(h.menu_selection(), Some(Vec::<URI>::new()));
    });
}

#[test]
fn context_menu_keeps_bulk_keyboard_commands_on_the_selection_after_deselecting_the_focused_item() {
    run_context_menu_case(|_guard, h| async move {
        let tree = h.view.tree();
        tree.set_selection(&[h.first.clone(), h.second.clone(), h.target.clone()], None);
        tree.set_focus(&[h.target.clone()], None);
        tree.set_selection(&[h.first.clone(), h.second.clone()], None);

        assert_same_items(&h.view.get_context(true), &[&h.first, &h.second]);
    });
}

#[test]
fn decoration_provider() {
    const TITLE: &str = "Files - ExplorerView decoration provider";
    let services = Services::new();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;
    let s1 = services.create_stat(TITLE, "/path", "path", true, d, false, false);
    s1.set_error(Some(Error::new("A test error")));
    let s2 = services.create_stat(TITLE, "/path/to", "to", true, d, true, false);
    let s3 = services.create_stat(TITLE, "/path/to/stat", "stat", false, d, false, false);
    assert_eq!(provide_decorations(&s3), None);
    assert_eq!(
        provide_decorations(&s2),
        Some(DecorationData {
            tooltip: Some("Symbolic Link".to_string()),
            letter: Some(DecorationLetter::Text("\u{2937}".to_string())),
            ..DecorationData::default()
        })
    );
    assert_eq!(
        provide_decorations(&s1),
        Some(DecorationData {
            tooltip: Some("Unable to resolve workspace folder (A test error)".to_string()),
            letter: Some(DecorationLetter::Text("!".to_string())),
            color: Some(LIST_INVALID_ITEM_FOREGROUND.to_string()),
            ..DecorationData::default()
        })
    );

    let unknown = services.create_stat(TITLE, "/path/to/stat", "stat", false, d, false, true);
    assert_eq!(
        provide_decorations(&unknown),
        Some(DecorationData {
            tooltip: Some("Unknown File Type".to_string()),
            letter: Some(DecorationLetter::Text("?".to_string())),
            ..DecorationData::default()
        })
    );
}

/// O `IWorkspace` literal `{ id: 'test', folders: [], configuration }` do caso.
struct TestWorkspace {
    configuration: Option<URI>,
}

impl IWorkspace for TestWorkspace {
    fn id(&self) -> String {
        "test".to_string()
    }

    fn folders(&self) -> Vec<WorkspaceFolder> {
        Vec::new()
    }

    fn transient(&self) -> bool {
        false
    }

    fn configuration(&self) -> Option<URI> {
        self.configuration.clone()
    }

    fn name(&self) -> Option<String> {
        None
    }
}

#[test]
fn preserves_workspace_name_case_only_for_user_named_workspaces() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let environment = TestEnvironmentService::new();
    let untitled_workspaces_home = environment.untitled_workspaces_home();
    let workspace = |configuration: Option<URI>| TestWorkspace { configuration };
    let upper_home = untitled_workspaces_home.with(&UriChange { path: Field::Set(untitled_workspaces_home.path().to_upper_case()), ..UriChange::default() });

    assert!(!should_preserve_workspace_name_case(WorkbenchState::Empty, &workspace(None), &*environment));
    assert!(should_preserve_workspace_name_case(WorkbenchState::Folder, &workspace(None), &*environment));
    assert!(!should_preserve_workspace_name_case(
        WorkbenchState::Workspace,
        &workspace(Some(join_path(&untitled_workspaces_home, &["1234", "workspace.json"]))),
        &*environment
    ));
    assert!(!should_preserve_workspace_name_case(
        WorkbenchState::Workspace,
        &workspace(Some(join_path(&upper_home, &["1234", "workspace.json"]))),
        &*environment
    ));
    assert!(should_preserve_workspace_name_case(
        WorkbenchState::Workspace,
        &workspace(Some(uri_file("/some/path/myWorkspace.code-workspace"))),
        &*environment
    ));
}

#[test]
fn compressed_navigation_controller() {
    const TITLE: &str = "Files - ExplorerView compressed navigation controller";
    let services = Services::new();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = date_now() as f64;
    let s1 = services.create_stat(TITLE, "/path", "path", true, d, false, false);
    let s2 = services.create_stat(TITLE, "/path/to", "to", true, d, false, false);
    let s3 = services.create_stat(TITLE, "/path/to/stat", "stat", false, d, false, false);

    let navigation_controller = CompressedNavigationController::new("id", vec![s1.clone(), s2.clone(), s3.clone()], 1, false);
    let disposable: Rc<dyn IDisposable> = navigation_controller.clone();
    guard.add(disposable);

    assert_eq!(navigation_controller.count(), 3);
    assert_eq!(navigation_controller.index(), 2);
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s3));
    navigation_controller.next();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s3));
    navigation_controller.previous();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s2));
    navigation_controller.previous();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s1));
    navigation_controller.previous();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s1));
    navigation_controller.last();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s3));
    navigation_controller.first();
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s1));
    navigation_controller.set_index(1);
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s2));
    navigation_controller.set_index(44);
    assert!(Rc::ptr_eq(&navigation_controller.current(), &s2));
}
