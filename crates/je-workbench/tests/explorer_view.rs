//! Porte de `src/vs/workbench/contrib/files/test/browser/explorerView.test.ts` (suíte `Files - ExplorerView`), caso a caso e na
//! mesma ordem.
//!
//! Casos de fora (a suíte aninhada `context menu`, 17 casos entre os `for` expandidos), todos exigem infraestrutura que o
//! porte não tem: `WorkbenchCompressibleAsyncDataTree` com `setInput`/`expand`/`setSelection`/`setFocus`, o `ExplorerView`
//! como `ViewPane` (`renderBody`, `setTreeInput`, `selectResource`), `MouseEvent`/`KeyboardEvent` despachados em elementos
//! do DOM, `IContextMenuService.showContextMenu` e `FilesRenderer`/`FilesFilter`/`ExplorerFindProvider` instanciados por
//! `stubInstance`. A UI é nativa e essas peças (árvore de widgets, menu de contexto, vistas) ainda não existem.
//! Os casos: `waits for a newly queued tree refresh before selecting a folder`, `preserves folder selection after
//! reopening a hidden Explorer`, `selects an unselected item with {0,1,2} previously selected items`, `preserves {1,2}
//! selected items when the target belongs to the selection`, `replaces a selection hidden in a collapsed folder`, `keeps the
//! compressed-folder navigation target when replacing the selection`, `selects the keyboard context-menu target without
//! opening it`, `preserves multi-selection for a keyboard context menu on a selected item`, `preserves the selection when
//! cancelling the context menu`, `keeps the newly selected target when cancelling the context menu`, `leaves the selection
//! unchanged for an editable input`, `preserves the background context menu`, `keeps bulk keyboard commands on the
//! selection after deselecting the focused item`.
//!
//! Notas de tradução:
//! - **`assert.deepStrictEqual` sobre `ExplorerItem[]`.** Os itens são os mesmos objetos: identidade (`Rc::ptr_eq`).
//! - **`createStat.call(this, ...)`.** O título completo do caso do mocha é o primeiro argumento de `to_resource`.
//! - **`CompressedNavigationController`.** Sem o `templateData` de DOM (ver `explorer_viewer.rs`); o caso exercita o
//!   índice, a contagem e o item corrente.
//! - **`TestEnvironmentService`.** O [`TestEnvironmentService`] do porte.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::runtime::date_now;
use je_base::uri::{Field, URI, UriChange};
use je_base::lifecycle::IDisposable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{join_path, uri_file};
use je_platform::workspace::{IWorkspace, WorkbenchState, WorkspaceFolder};
use je_workbench::contrib::files::browser::views::explorer_decorations_provider::{LIST_INVALID_ITEM_FOREGROUND, provide_decorations};
use je_workbench::contrib::files::browser::views::explorer_view::{self, CompressedNavigationControllerProvider, should_preserve_workspace_name_case};
use je_workbench::contrib::files::browser::views::explorer_viewer::{CompressedNavigationController, ICompressedNavigationController};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::services::decorations::decorations::{DecorationData, DecorationLetter};
use je_workbench::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestFileService};

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;

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
