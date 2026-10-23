//! Porte de `src/vs/platform/workspace/test/common/workspace.test.ts` (suíte `Workspace`), caso a caso e na mesma ordem.
//!
//! Divergências: o `assert.strictEqual(actual, expected)` entre pastas (identidade) compara o URI e o índice
//! (a pasta é um valor, não um objeto com identidade).

mod common;

use std::rc::Rc;

use common::*;
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::uri::{Field, URI, UriChange};
use je_platform::resources::{ext_uri_biased_ignore_path_case, host_environment, uri_file};
use je_platform::workspace::{RawWorkspaceFolder, Workspace, WorkspaceFolder};
use je_platform::workspaces::to_workspace_folders;

fn join(a: &str, b: &str) -> String {
    let env = host_environment();
    PathModule::native(env.as_ref()).join(&[a, b])
}

struct Fixture {
    _guard: LeakGuard,
    file_folder: String,
    abc_folder: String,
}

impl Fixture {
    fn new() -> Self {
        let windows = Platform::detect().is_windows;
        Self {
            _guard: ensure_no_disposables_are_leaked_in_test_suite(),
            file_folder: if windows { "c:\\src" } else { "/src" }.to_string(),
            abc_folder: if windows { "c:\\abc" } else { "/abc" }.to_string(),
        }
    }

    fn uri(&self, name: &str) -> URI {
        uri_file(&join(&self.file_folder, name))
    }

    fn workspace(&self, folders: Vec<WorkspaceFolder>) -> Rc<Workspace> {
        let is_linux = Platform::detect().is_linux;
        Workspace::new("", folders, false, None, Rc::new(move |_: &URI| !is_linux), None)
    }

    fn workspace_config_uri(&self) -> URI {
        self.uri("test.code-workspace")
    }
}

fn folder(uri: URI, index: usize) -> WorkspaceFolder {
    WorkspaceFolder::new(uri, "", index, None)
}

fn assert_same_folder(actual: Option<WorkspaceFolder>, expected: &WorkspaceFolder) {
    let actual = actual.expect("esperava uma pasta");
    assert_eq!(actual.uri, expected.uri);
    assert_eq!(actual.index, expected.index);
}

#[test]
fn get_folder_returns_the_folder_with_given_uri() {
    let f = Fixture::new();
    let expected = folder(f.uri("test"), 2);
    let test_object = f.workspace(vec![folder(f.uri("main"), 0), expected.clone(), folder(uri_file("/src/code"), 2)]);
    assert_same_folder(test_object.get_folder(&expected.uri), &expected);
}

#[test]
fn get_folder_returns_the_folder_if_the_uri_is_sub() {
    let f = Fixture::new();
    let expected = folder(f.uri("test"), 0);
    let test_object = f.workspace(vec![expected.clone(), folder(f.uri("main"), 1), folder(uri_file("/src/code"), 2)]);
    assert_same_folder(test_object.get_folder(&uri_file(&join(&f.file_folder, "test/a"))), &expected);
}

#[test]
fn get_folder_returns_the_closest_folder_if_the_uri_is_sub() {
    let f = Fixture::new();
    let expected = folder(f.uri("test"), 2);
    let test_object = f.workspace(vec![folder(f.uri("main"), 0), folder(uri_file("/src/code"), 1), expected.clone()]);
    assert_same_folder(test_object.get_folder(&uri_file(&join(&f.file_folder, "test/a"))), &expected);
}

#[test]
fn get_folder_returns_the_folder_even_if_the_uri_has_query_path() {
    let f = Fixture::new();
    let expected = folder(f.uri("test"), 2);
    let test_object = f.workspace(vec![folder(f.uri("main"), 0), folder(uri_file("/src/code"), 1), expected.clone()]);
    let with_query = uri_file(&join(&f.file_folder, "test/a")).with(&UriChange { query: Field::Set("somequery".into()), ..Default::default() });
    assert_same_folder(test_object.get_folder(&with_query), &expected);
}

#[test]
fn get_folder_returns_null_if_the_uri_is_not_sub() {
    let f = Fixture::new();
    let test_object = f.workspace(vec![folder(f.uri("test"), 0), folder(uri_file("/src/code"), 1)]);
    assert!(test_object.get_folder(&uri_file(&join(&f.file_folder, "main/a"))).is_none());
}

fn path(p: &str) -> RawWorkspaceFolder {
    RawWorkspaceFolder::Path { path: p.to_string(), name: None }
}

fn named(p: &str, name: &str) -> RawWorkspaceFolder {
    RawWorkspaceFolder::Path { path: p.to_string(), name: Some(name.to_string()) }
}

fn raw_path(folder: &WorkspaceFolder) -> String {
    match folder.raw.as_ref() {
        Some(RawWorkspaceFolder::Path { path, .. }) => path.clone(),
        other => panic!("esperava um caminho, veio {other:?}"),
    }
}

fn fs_path(uri: &URI) -> String {
    je_platform::resources::fs_path(uri)
}

fn check(actual: &WorkspaceFolder, uri: &URI, raw: &str, index: usize, name: &str) {
    assert_eq!(fs_path(&actual.uri), fs_path(uri));
    assert_eq!(raw_path(actual), raw);
    assert_eq!(actual.index, index);
    assert_eq!(actual.name, name);
}

#[test]
fn to_workspace_folders_with_single_absolute_folder() {
    let f = Fixture::new();
    let actual = to_workspace_folders(&[path("/src/test")], &f.workspace_config_uri(), &ext_uri_biased_ignore_path_case());
    assert_eq!(actual.len(), 1);
    check(&actual[0], &f.uri("test"), "/src/test", 0, "test");
}

#[test]
fn to_workspace_folders_with_single_relative_folder() {
    let f = Fixture::new();
    let actual = to_workspace_folders(&[path("./test")], &f.workspace_config_uri(), &ext_uri_biased_ignore_path_case());
    assert_eq!(actual.len(), 1);
    check(&actual[0], &f.uri("test"), "./test", 0, "test");
}

#[test]
fn to_workspace_folders_with_single_absolute_folder_with_name() {
    let f = Fixture::new();
    let actual = to_workspace_folders(&[named("/src/test", "hello")], &f.workspace_config_uri(), &ext_uri_biased_ignore_path_case());
    assert_eq!(actual.len(), 1);
    check(&actual[0], &f.uri("test"), "/src/test", 0, "hello");
}

#[test]
fn to_workspace_folders_with_multiple_unique_absolute_folders() {
    let f = Fixture::new();
    let actual = to_workspace_folders(&[path("/src/test2"), path("/src/test3"), path("/src/test1")], &f.workspace_config_uri(), &ext_uri_biased_ignore_path_case());
    assert_eq!(actual.len(), 3);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &f.uri("test3"), "/src/test3", 1, "test3");
    check(&actual[2], &f.uri("test1"), "/src/test1", 2, "test1");
}

#[test]
fn to_workspace_folders_with_multiple_unique_absolute_folders_with_names() {
    let f = Fixture::new();
    let actual = to_workspace_folders(
        &[path("/src/test2"), named("/src/test3", "noName"), path("/src/test1")],
        &f.workspace_config_uri(),
        &ext_uri_biased_ignore_path_case(),
    );
    assert_eq!(actual.len(), 3);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &f.uri("test3"), "/src/test3", 1, "noName");
    check(&actual[2], &f.uri("test1"), "/src/test1", 2, "test1");
}

#[test]
fn to_workspace_folders_with_multiple_unique_absolute_and_relative_folders() {
    let f = Fixture::new();
    let abc_test3 = uri_file(&join(&f.abc_folder, "test3"));
    let actual = to_workspace_folders(
        &[path("/src/test2"), named("/abc/test3", "noName"), path("./test1")],
        &f.workspace_config_uri(),
        &ext_uri_biased_ignore_path_case(),
    );
    assert_eq!(actual.len(), 3);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &abc_test3, "/abc/test3", 1, "noName");
    check(&actual[2], &f.uri("test1"), "./test1", 2, "test1");
}

#[test]
fn to_workspace_folders_with_multiple_absolute_folders_with_duplicates() {
    let f = Fixture::new();
    let actual = to_workspace_folders(
        &[path("/src/test2"), named("/src/test2", "noName"), path("/src/test1")],
        &f.workspace_config_uri(),
        &ext_uri_biased_ignore_path_case(),
    );
    assert_eq!(actual.len(), 2);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &f.uri("test1"), "/src/test1", 1, "test1");
}

#[test]
fn to_workspace_folders_with_multiple_absolute_and_relative_folders_with_duplicates() {
    let f = Fixture::new();
    let abc_test1 = uri_file(&join(&f.abc_folder, "test1"));
    let actual = to_workspace_folders(
        &[path("/src/test2"), named("/src/test3", "noName"), path("./test3"), path("/abc/test1")],
        &f.workspace_config_uri(),
        &ext_uri_biased_ignore_path_case(),
    );
    assert_eq!(actual.len(), 3);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &f.uri("test3"), "/src/test3", 1, "noName");
    check(&actual[2], &abc_test1, "/abc/test1", 2, "test1");
}

#[test]
fn to_workspace_folders_with_multiple_absolute_and_relative_folders_with_invalid_paths() {
    let f = Fixture::new();
    let abc_test1 = uri_file(&join(&f.abc_folder, "test1"));
    let actual = to_workspace_folders(
        &[path("/src/test2"), named("", "noName"), path("./test3"), path("/abc/test1")],
        &f.workspace_config_uri(),
        &ext_uri_biased_ignore_path_case(),
    );
    assert_eq!(actual.len(), 3);
    check(&actual[0], &f.uri("test2"), "/src/test2", 0, "test2");
    check(&actual[1], &f.uri("test3"), "./test3", 1, "test3");
    check(&actual[2], &abc_test1, "/abc/test1", 2, "test1");
}
