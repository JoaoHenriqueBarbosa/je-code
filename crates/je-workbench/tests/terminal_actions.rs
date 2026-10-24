//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalActions.test.ts` (suíte `terminalActions`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o guarda criado no começo de cada caso.
//! - **`makeFakeFolder(name, uri)`.** É o `WorkspaceFolder` do porte (`uri`, `name`, `index: 0`); o `toResource` do
//!   `IWorkspaceFolder` não existe lá e o teste não o lê.
//! - **`makePair(folder, cwd?, isAbsolute?)`.** O `cwd` que é `URI | IWorkspaceFolder` é o [`CwdArg`]; o par é um `Rc`, porque a
//!   função compara os objetos por identidade (`selectedPairs.has(x)`), como no original.
//! - **`deepStrictEqual`.** É `assert_eq!` sobre os pares, que o [`PartialEq`] do par compara campo a campo.
//! - **`URI.joinPath(root, 'a')`.** É `join_path` do hospedeiro.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uri::URI;
use je_platform::resources::{join_path, uri_file};
use je_platform::workspace::WorkspaceFolder;
use je_workbench::contrib::terminal::browser::terminal_actions::{WorkspaceFolderCwdPair, shrink_workspace_folder_cwd_pairs};

/// O `cwd?: URI | IWorkspaceFolder` de `makePair` (os casos só passam a pasta, mas a função aceita as duas formas).
#[allow(dead_code)]
enum CwdArg<'a> {
    Uri(&'a URI),
    Folder(&'a WorkspaceFolder),
}

fn make_fake_folder(name: &str, uri: URI) -> WorkspaceFolder {
    WorkspaceFolder::new(uri, name, 0, None)
}

fn make_pair(folder: &WorkspaceFolder, cwd: Option<CwdArg<'_>>, is_absolute: bool) -> Rc<WorkspaceFolderCwdPair> {
    let cwd_uri = match cwd {
        None => folder.uri.clone(),
        Some(CwdArg::Uri(uri)) => uri.clone(),
        Some(CwdArg::Folder(other)) => other.uri.clone(),
    };
    let is_overridden = cwd_uri.to_string() != folder.uri.to_string();
    Rc::new(WorkspaceFolderCwdPair { folder: folder.clone(), cwd: cwd_uri, is_absolute, is_overridden })
}

struct Folders {
    a: WorkspaceFolder,
    b: WorkspaceFolder,
    c: WorkspaceFolder,
    d: WorkspaceFolder,
}

fn folders() -> Folders {
    let root = uri_file("/some-root");
    Folders {
        a: make_fake_folder("a", join_path(&root, &["a"])),
        b: make_fake_folder("b", join_path(&root, &["b"])),
        c: make_fake_folder("c", join_path(&root, &["c"])),
        d: make_fake_folder("d", join_path(&root, &["d"])),
    }
}

fn a_pair(folder: &WorkspaceFolder) -> Rc<WorkspaceFolderCwdPair> {
    make_pair(folder, None, false)
}

/// `makePair(folder, other)`: o `cwd` aponta para a pasta `other`.
fn pointing_to(folder: &WorkspaceFolder, other: &WorkspaceFolder) -> Rc<WorkspaceFolderCwdPair> {
    make_pair(folder, Some(CwdArg::Folder(other)), false)
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_return_empty_when_given_array_is_empty() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(shrink_workspace_folder_cwd_pairs(&[]), Vec::<Rc<WorkspaceFolderCwdPair>>::new());
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_return_the_only_single_pair_when_given_argument_is_a_single_element_array() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pairs = vec![a_pair(&f.a)];
    assert_eq!(shrink_workspace_folder_cwd_pairs(&pairs), pairs);
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_return_all_pairs_when_no_repeated_cwds() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pairs = vec![a_pair(&f.a), a_pair(&f.b), a_pair(&f.c)];
    assert_eq!(shrink_workspace_folder_cwd_pairs(&pairs), pairs);
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_select_the_pair_that_has_the_same_uri_when_repeated_cwds_exist_all_repeated() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pair_a = a_pair(&f.a);
    let pair_b = pointing_to(&f.b, &f.a); // CWD points to A
    let pair_c = pointing_to(&f.c, &f.a); // CWD points to A
    assert_eq!(shrink_workspace_folder_cwd_pairs(&[pair_a.clone(), pair_b, pair_c]), vec![pair_a]);
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_select_the_pair_that_has_the_same_uri_when_repeated_cwds_exist_two_repeated_plus_one_different() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pair_a = a_pair(&f.a);
    let pair_b = pointing_to(&f.b, &f.a); // CWD points to A
    let pair_c = a_pair(&f.c);
    assert_eq!(shrink_workspace_folder_cwd_pairs(&[pair_a.clone(), pair_b, pair_c.clone()]), vec![pair_a, pair_c]);
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_select_the_pair_that_has_the_same_uri_when_repeated_cwds_exist_two_repeated_plus_two_repeated() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pair_a = a_pair(&f.a);
    let pair_b = pointing_to(&f.b, &f.a); // CWD points to A
    let pair_c = a_pair(&f.c);
    let pair_d = pointing_to(&f.d, &f.c);
    assert_eq!(shrink_workspace_folder_cwd_pairs(&[pair_a.clone(), pair_b, pair_c.clone(), pair_d]), vec![pair_a, pair_c]);
}

#[test]
fn shrink_workspace_folder_cwd_pairs_should_select_the_pair_that_has_the_same_uri_when_repeated_cwds_exist_two_repeated_plus_two_repeated_reverse_order() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = folders();
    let pair_b = pointing_to(&f.b, &f.a); // CWD points to A
    let pair_a = a_pair(&f.a);
    let pair_d = pointing_to(&f.d, &f.c);
    let pair_c = a_pair(&f.c);
    assert_eq!(shrink_workspace_folder_cwd_pairs(&[pair_a.clone(), pair_b, pair_c.clone(), pair_d]), vec![pair_a, pair_c]);
}
