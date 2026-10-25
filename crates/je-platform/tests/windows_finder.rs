//! Porte de `src/vs/platform/windows/test/electron-main/windowsFinder.test.ts`.
//!
//! Um `#[test]` por caso do original, na mesma ordem.
//!
//! Notas de tradução:
//! - `createTestCodeWindow` é o [`TestCodeWindow`]: o `ICodeWindow` do original implementa todos os membros e lança em quase
//!   todos; aqui só existem os do contrato [`CodeWindow`] (os demais não têm como ser chamados por quem usa o contrato
//!   portado), e os que o original lança lançam. O `lastFocusTime` da opção não é lido por `findWindowOnFile` e não é
//!   membro do contrato, então não é guardado.
//! - `win: null!` do original é `has_native_window() == false`.
//! - `FileAccess.asFileUri('.../fixtures').fsPath` só alimenta contas de caminho (nenhum arquivo é lido): é uma pasta sob a
//!   pasta temporária do sistema.
//! - `assert.strictEqual(window, expected)` entre objetos é a identidade do `Rc` ([`Rc::ptr_eq`]).
//! - `workspace === testWorkspace` do resolvedor é a igualdade de valor do identificador.
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` fica no fim da suíte no original; o [`LeakGuard`] é criado no começo do
//!   corpo e descartado no fim, depois de esvaziar as microtarefas.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use common::{ensure_no_disposables_are_leaked_in_test_suite, join, run_with_real_timers};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::{ext_uri_biased_ignore_path_case, uri_file};
use je_platform::window::electron_main::{Rectangle, WindowUiState};
use je_platform::windows::windows_finder::find_window_on_file;
use je_platform::workspace::{AnyWorkspaceIdentifier, RawWorkspaceFolder, ResolvedWorkspace, SingleFolderWorkspaceIdentifier, WorkspaceIdentifier};
use je_platform::workspaces::to_workspace_folders;
use je_platform::workspaces::workspaces_management_main_service::{CodeWindow, NativeWindowConfiguration};

fn fixtures_folder() -> String {
    join(&[&std::env::temp_dir().to_string_lossy(), "windowsFinderTest", "fixtures"])
}

/// `testWorkspace`: uma constante da suíte, com o `id` de `Date.now().toString()` calculado uma vez.
fn test_workspace() -> WorkspaceIdentifier {
    static ID: OnceLock<String> = OnceLock::new();
    let id = ID.get_or_init(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis()).to_string());
    WorkspaceIdentifier { id: id.clone(), config_path: uri_file(&join(&[&fixtures_folder(), "workspaces.json"])) }
}

/// `localWorkspaceResolver`: resolve só o `testWorkspace`, com as duas pastas de `testWorkspaceFolders`.
fn local_workspace_resolver() -> impl Fn(&WorkspaceIdentifier) -> Promise<Option<ResolvedWorkspace>> {
    let test_workspace = test_workspace();
    let test_workspace_folders = to_workspace_folders(
        &[
            RawWorkspaceFolder::Path { path: join(&[&fixtures_folder(), "vscode_workspace_1_folder"]), name: None },
            RawWorkspaceFolder::Path { path: join(&[&fixtures_folder(), "vscode_workspace_2_folder"]), name: None },
        ],
        &test_workspace.config_path,
        &ext_uri_biased_ignore_path_case(),
    );
    move |workspace: &WorkspaceIdentifier| {
        Promise::resolved(if *workspace == test_workspace {
            Some(ResolvedWorkspace { identifier: test_workspace.clone(), remote_authority: None, transient: None, folders: test_workspace_folders.clone() })
        } else {
            None
        })
    }
}

/// O `ICodeWindow` de teste do original.
struct TestCodeWindow {
    opened_workspace: Option<AnyWorkspaceIdentifier>,
}

impl CodeWindow for TestCodeWindow {
    fn has_native_window(&self) -> bool {
        false
    }

    fn is_simple_full_screen(&self) -> bool {
        false
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn id(&self) -> i32 {
        -1
    }

    fn backup_path(&self) -> Option<String> {
        None
    }

    fn is_extension_development_host(&self) -> bool {
        false
    }

    fn is_extension_test_host(&self) -> bool {
        false
    }

    fn get_bounds(&self) -> Rectangle {
        panic!("Method not implemented.")
    }

    fn serialize_window_state(&self) -> WindowUiState {
        panic!("Method not implemented")
    }

    fn opened_workspace(&self) -> Option<AnyWorkspaceIdentifier> {
        self.opened_workspace.clone()
    }

    fn config(&self) -> Option<Rc<RefCell<NativeWindowConfiguration>>> {
        None
    }

    fn remote_authority(&self) -> Option<String> {
        None
    }

    fn focus(&self) {
        panic!("Method not implemented.")
    }
}

fn create_test_code_window(opened_folder_uri: Option<URI>, opened_workspace: Option<WorkspaceIdentifier>) -> Rc<dyn CodeWindow> {
    let opened_workspace = match opened_folder_uri {
        Some(uri) => Some(AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id: String::new(), uri })),
        None => opened_workspace.map(AnyWorkspaceIdentifier::Workspace),
    };
    Rc::new(TestCodeWindow { opened_workspace })
}

fn assert_window(actual: Option<Rc<dyn CodeWindow>>, expected: Option<&Rc<dyn CodeWindow>>) {
    match (actual, expected) {
        (None, None) => {}
        (Some(actual), Some(expected)) => assert!(Rc::ptr_eq(&actual, expected), "a janela achada não é a esperada"),
        (Some(_), None) => panic!("esperava nenhuma janela, mas achou uma"),
        (None, Some(_)) => panic!("esperava uma janela, mas não achou nenhuma"),
    }
}

fn folder_uri(segments: &[&str]) -> URI {
    let fixtures_folder = fixtures_folder();
    let mut parts = vec![fixtures_folder.as_str()];
    parts.extend_from_slice(segments);
    uri_file(&join(&parts))
}

/// As três janelas do nível da suíte: `vscodeFolderWindow`, `lastActiveWindow` e `noVscodeFolderWindow`.
fn windows() -> Vec<Rc<dyn CodeWindow>> {
    vec![
        create_test_code_window(Some(folder_uri(&["vscode_folder"])), None),
        create_test_code_window(None, None),
        create_test_code_window(Some(folder_uri(&["no_vscode_folder"])), None),
    ]
}

#[test]
fn new_window_without_folder_when_no_windows_exist() {
    run_with_real_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver = local_workspace_resolver();

        assert_window(find_window_on_file(&[], &uri_file("nonexisting"), &resolver).await.unwrap(), None);
        assert_window(find_window_on_file(&[], &folder_uri(&["no_vscode_folder", "file.txt"]), &resolver).await.unwrap(), None);
    });
}

#[test]
fn existing_window_with_folder() {
    run_with_real_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver = local_workspace_resolver();
        let windows = windows();
        let (vscode_folder_window, no_vscode_folder_window) = (&windows[0], &windows[2]);

        assert_window(find_window_on_file(&windows, &folder_uri(&["no_vscode_folder", "file.txt"]), &resolver).await.unwrap(), Some(no_vscode_folder_window));

        assert_window(find_window_on_file(&windows, &folder_uri(&["vscode_folder", "file.txt"]), &resolver).await.unwrap(), Some(vscode_folder_window));

        let window = create_test_code_window(Some(folder_uri(&["vscode_folder", "nested_folder"])), None);
        assert_window(
            find_window_on_file(std::slice::from_ref(&window), &folder_uri(&["vscode_folder", "nested_folder", "subfolder", "file.txt"]), &resolver).await.unwrap(),
            Some(&window),
        );
    });
}

#[test]
fn more_specific_existing_window_wins() {
    run_with_real_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver = local_workspace_resolver();

        let window = create_test_code_window(Some(folder_uri(&["no_vscode_folder"])), None);
        let nested_folder_window = create_test_code_window(Some(folder_uri(&["no_vscode_folder", "nested_folder"])), None);
        assert_window(
            find_window_on_file(&[window, nested_folder_window.clone()], &folder_uri(&["no_vscode_folder", "nested_folder", "subfolder", "file.txt"]), &resolver).await.unwrap(),
            Some(&nested_folder_window),
        );
    });
}

#[test]
fn workspace_folder_wins() {
    run_with_real_timers(async {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let resolver = local_workspace_resolver();

        let window = create_test_code_window(None, Some(test_workspace()));
        assert_window(
            find_window_on_file(std::slice::from_ref(&window), &folder_uri(&["vscode_workspace_2_folder", "nested_vscode_folder", "subfolder", "file.txt"]), &resolver).await.unwrap(),
            Some(&window),
        );
    });
}
