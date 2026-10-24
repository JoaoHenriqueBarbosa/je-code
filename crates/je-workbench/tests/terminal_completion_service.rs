//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalCompletionService.test.ts` (suíte `TerminalCompletionService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada caso e o `setup`
//!   da suíte é o [`TerminalCompletionFixture`] (em `tests/support/terminal_completion_support.rs`, com a lista dos dublês). Os
//!   `setup` das suítes internas são as funções `*_setup` chamadas no começo dos casos delas. Os casos `async` rodam em
//!   `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`isWindows`.** É a plataforma do hospedeiro (`Platform::detect().is_windows`). O `if (isWindows) { test(...) } else {
//!   test(...) }` do original registra um caso por plataforma; aqui os dois são `#[test]` e o que não vale na plataforma de
//!   execução sai no começo com um `return` (o original nem registra o caso). Os dois casos de conversão de caminho do Git Bash
//!   (`gitBashToWindowsPath` e `windowsToGitBashPath`) são funções puras e rodam em qualquer plataforma.
//! - **Literais de texto do original.** `'\['`, `'\ '`, `'\!'`, `'\$'`, `'\&'` e `'\/'` de uma string JavaScript são o próprio
//!   caractere (escape sem efeito), então os rótulos e detalhes do caso `completion label escaping` não levam barra invertida.
//! - **`assert.deepStrictEqual` de `assertCompletions` e `assertPartialCompletionsExist`.** Comparam tuplas
//!   `(label, detail, kind, replacementRange)`; `e.detail ?? ''` e `e.kind ?? Folder` são o `unwrap_or` dos mapeamentos.
//! - **`assert(!result)` e `assert.strictEqual(result, undefined)`** são `result.is_none()`.
//! - **`result.find(...)` e `.at(-1)`** são `iter().find(...)` e `last()`.
//! - **`getEnabledProviders` e `includes`.** O método protegido é público no porte (`get_enabled_providers`); `result.includes(p)`
//!   compara a identidade do `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use indexmap::IndexMap;
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_base::uri::{URI, UriComponents};
use je_platform::resources::uri_file;
use je_platform::terminal::capabilities::capabilities::{IShellEnvDetectionCapability, ShellEnvDetection};
use je_platform::terminal::capabilities::shell_env_detection_capability::ShellEnvDetectionCapability;
use je_platform::terminal::terminal::{TerminalShellType, WindowsShellType};
use je_workbench::contrib::terminal_contrib::suggest::common::terminal_suggest_configuration::TerminalSuggestSettingId;
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_item::{ITerminalCompletion, TerminalCompletionItemKind};
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_service::{ITerminalCompletionProvider, TerminalCompletionResourceOptions};
use je_workbench::contrib::terminal_contrib::suggest::terminal_git_bash_helpers::{git_bash_to_windows_path, windows_to_git_bash_path};
use je_workbench::test::common::workbench_test_services::FileStatChild;
use support::terminal_completion_support::{MockCompletionProvider, TerminalCompletionFixture, home_dir, shell};

const PROVIDER: &str = "testProvider";

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `pathSeparator`.
fn path_separator() -> &'static str {
    if is_windows() { "\\" } else { "/" }
}

/// `IAssertionTerminalCompletion`.
struct Expected {
    label: String,
    detail: Option<String>,
    kind: Option<TerminalCompletionItemKind>,
}

/// `{ label, detail }`.
fn expected(label: &str, detail: &str) -> Expected {
    Expected { label: label.to_string(), detail: Some(detail.to_string()), kind: None }
}

/// `{ label, detail, kind: TerminalCompletionItemKind.File }`.
fn expected_file(label: &str, detail: &str) -> Expected {
    Expected { kind: Some(TerminalCompletionItemKind::File), ..expected(label, detail) }
}

/// `standardTildeItem`.
fn standard_tilde_item() -> Expected {
    expected("~", &home_dir())
}

type Tuple = (String, String, TerminalCompletionItemKind, Option<[i64; 2]>);

/// O mapeamento dos itens reais que os dois `assert` do original fazem.
fn actual_tuples(actual: &[ITerminalCompletion]) -> Vec<Tuple> {
    actual
        .iter()
        .map(|e| (e.label.text().to_string(), e.detail().unwrap_or_default(), e.kind.unwrap_or(TerminalCompletionItemKind::Folder), e.replacement_range))
        .collect()
}

/// Assert the set of completions exist exactly, including their order.
#[track_caller]
fn assert_completions(actual: Option<Vec<ITerminalCompletion>>, expected: &[Expected], replacement_range: [i64; 2], path_sep: Option<&str>) {
    let sep = path_sep.unwrap_or(path_separator());
    let actual = actual_tuples(&actual.expect("as completions deviam existir"));
    let expected: Vec<Tuple> = expected
        .iter()
        .map(|e| {
            (
                e.label.replace('/', sep),
                e.detail.as_ref().map_or(String::new(), |detail| detail.replace('/', sep)),
                e.kind.unwrap_or(TerminalCompletionItemKind::Folder),
                Some(replacement_range),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

/// Assert a set of completions exist within the actual set.
#[track_caller]
fn assert_partial_completions_exist(actual: Option<Vec<ITerminalCompletion>>, expected_partial: &[Expected], replacement_range: [i64; 2]) {
    let actual = actual_tuples(&actual.expect("as completions deviam existir"));
    let sep = path_separator();
    for e in expected_partial {
        let expected_item: Tuple = (
            e.label.replace('/', sep),
            e.detail.as_ref().map_or(String::new(), |detail| detail.replace('/', sep)),
            e.kind.unwrap_or(TerminalCompletionItemKind::Folder),
            Some(replacement_range),
        );
        assert_eq!(actual.iter().find(|item| item.1 == expected_item.1), Some(&expected_item));
    }
}

fn uri(value: &str) -> URI {
    URI::parse(value, false)
}

/// `{ resource }` sem nenhum dos opcionais.
fn child(resource: &str) -> FileStatChild {
    FileStatChild { resource: uri(resource), is_file: None, is_directory: None, is_symbolic_link: None, executable: None }
}

fn dir(resource: &str) -> FileStatChild {
    FileStatChild { is_directory: Some(true), ..child(resource) }
}

fn file(resource: &str) -> FileStatChild {
    FileStatChild { is_file: Some(true), ..child(resource) }
}

fn executable_file(resource: &str) -> FileStatChild {
    FileStatChild { executable: Some(true), ..file(resource) }
}

/// `{ showFiles, showDirectories, cwd, pathSeparator }`.
fn options(cwd: URI, show_files: Option<bool>, show_directories: Option<bool>, path_sep: &str) -> TerminalCompletionResourceOptions {
    TerminalCompletionResourceOptions { show_files, show_directories, ..TerminalCompletionResourceOptions::new(cwd, path_sep) }
}

/// `validResources = ...; childResources = ...`.
fn set_resources(fixture: &TerminalCompletionFixture, valid_resources: Vec<URI>, child_resources: Vec<FileStatChild>) {
    *fixture.valid_resources.borrow_mut() = valid_resources;
    *fixture.child_resources.borrow_mut() = child_resources;
}

/// `terminalCompletionService.resolveResources(resourceOptions, promptValue, cursorPosition, provider, capabilities, shellType?)`.
async fn resolve_resources(
    fixture: &TerminalCompletionFixture,
    resource_options: &TerminalCompletionResourceOptions,
    prompt_value: &str,
    cursor_position: i64,
    shell_type: TerminalShellType,
) -> Option<Vec<ITerminalCompletion>> {
    fixture
        .terminal_completion_service
        .resolve_resources(resource_options, prompt_value, cursor_position, PROVIDER, fixture.capabilities.clone(), shell_type)
        .await
        .unwrap()
}

/// `{ scheme: 'vscode-remote', authority: 'wsl+Ubuntu', path }`.
fn remote_uri(path: &str) -> URI {
    URI::from(
        &UriComponents { scheme: Some("vscode-remote".into()), authority: Some("wsl+Ubuntu".into()), path: Some(path.into()), ..UriComponents::default() },
        false,
    )
}

/// `{ [key]: value }` do ambiente do shell.
fn env_map(entries: &[(&str, &str)]) -> IndexMap<String, Option<String>> {
    entries.iter().map(|(key, value)| (key.to_string(), Some(value.to_string()))).collect()
}

/// `shellEnvDetection = store.add(new ShellEnvDetectionCapability()); setEnvironment(env, true); capabilities.add(...)`.
fn add_shell_env_detection(
    fixture: &TerminalCompletionFixture,
    guard: &common::LeakGuard,
    entries: &[(&str, &str)],
) -> Rc<ShellEnvDetectionCapability> {
    let shell_env_detection = guard.add_rc(ShellEnvDetectionCapability::new());
    shell_env_detection.set_environment(Some(&env_map(entries)), true);
    fixture.capabilities.add::<ShellEnvDetection>(shell_env_detection.clone());
    shell_env_detection
}

// suite('resolveResources should return undefined')

#[test]
fn resolve_resources_should_return_undefined_if_neither_show_files_nor_show_directories_are_true() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, None, path_separator());
        set_resources(&f, vec![uri("file:///test")], vec![]);
        let result = resolve_resources(&f, &resource_options, "cd ", 3, None).await;
        assert!(result.is_none());
    });
}

// suite('resolveResources should return folder completions')

fn folder_completions_setup(f: &TerminalCompletionFixture) {
    set_resources(f, vec![uri("file:///test")], vec![FileStatChild { is_file: Some(false), ..dir("file:///test/folder1/") }, file("file:///test/file1.txt")]);
}

#[test]
fn pipe_should_return_root_level_completions() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        folder_completions_setup(&f);
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "", 1, None).await;

        assert_completions(
            result,
            &[expected(".", "/test/"), expected("./folder1/", "/test/folder1/"), expected("../", "/"), standard_tilde_item()],
            [1, 1],
            None,
        );
    });
}

#[test]
fn dot_slash_pipe_should_return_folder_completions() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        folder_completions_setup(&f);
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "./", 3, None).await;

        assert_completions(result, &[expected("./", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./../", "/")], [1, 3], None);
    });
}

#[test]
fn dot_dot_slash_pipe_should_return_parent_folder_completions() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        folder_completions_setup(&f);
        // Scenario: cwd is /parent/folder1, sibling is /parent/folder2
        // When typing ../, should see contents of /parent/ (folder1 and folder2)
        set_resources(&f, vec![uri("file:///parent/folder1"), uri("file:///parent")], vec![dir("file:///parent/folder1/"), dir("file:///parent/folder2/")]);
        let resource_options = options(uri("file:///parent/folder1"), None, Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "../", 3, None).await;

        assert_completions(
            result,
            &[expected("../", "/parent/"), expected("../folder1/", "/parent/folder1/"), expected("../folder2/", "/parent/folder2/"), expected("../../", "/")],
            [0, 3],
            None,
        );
    });
}

#[test]
fn cd_dot_slash_pipe_should_return_folder_completions() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        folder_completions_setup(&f);
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "cd ./", 5, None).await;

        assert_completions(result, &[expected("./", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./../", "/")], [3, 5], None);
    });
}

#[test]
fn cd_dot_slash_f_pipe_should_return_folder_completions() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        folder_completions_setup(&f);
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "cd ./f", 6, None).await;

        assert_completions(result, &[expected("./", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./../", "/")], [3, 6], None);
    });
}

// suite('resolveResources should handle file and folder completion requests correctly')

fn file_and_folder_setup(f: &TerminalCompletionFixture) {
    set_resources(
        f,
        vec![uri("file:///test")],
        vec![executable_file("file:///test/.hiddenFile"), dir("file:///test/.hiddenFolder/"), dir("file:///test/folder1/"), executable_file("file:///test/file1.txt")],
    );
}

#[test]
fn dot_slash_pipe_should_handle_hidden_files_and_folders() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        file_and_folder_setup(&f);
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "./", 2, None).await;

        assert_completions(
            result,
            &[
                expected("./", "/test/"),
                expected_file("./.hiddenFile", "/test/.hiddenFile"),
                expected("./.hiddenFolder/", "/test/.hiddenFolder/"),
                expected("./folder1/", "/test/folder1/"),
                expected_file("./file1.txt", "/test/file1.txt"),
                expected("./../", "/"),
            ],
            [0, 2],
            None,
        );
    });
}

#[test]
fn dot_slash_h_pipe_should_handle_hidden_files_and_folders() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        file_and_folder_setup(&f);
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "./h", 3, None).await;

        assert_completions(
            result,
            &[
                expected("./", "/test/"),
                expected_file("./.hiddenFile", "/test/.hiddenFile"),
                expected("./.hiddenFolder/", "/test/.hiddenFolder/"),
                expected("./folder1/", "/test/folder1/"),
                expected_file("./file1.txt", "/test/file1.txt"),
                expected("./../", "/"),
            ],
            [0, 3],
            None,
        );
    });
}

// suite('~ -> $HOME')

/// O `setup` da suíte `~ -> $HOME`: o ambiente do shell, os recursos e as opções.
fn tilde_setup(f: &TerminalCompletionFixture, guard: &common::LeakGuard) -> TerminalCompletionResourceOptions {
    add_shell_env_detection(f, guard, &[("HOME", "/home"), ("USERPROFILE", "/home")]);

    let resource_options = options(uri("file:///test/folder1"), Some(true), Some(true), path_separator()); // Updated to reflect home directory
    set_resources(
        f,
        vec![uri("file:///test"), uri("file:///test/folder1"), uri("file:///home"), uri("file:///home/vscode"), uri("file:///home/vscode/foo"), uri("file:///home/vscode/bar.txt")],
        vec![dir("file:///home/vscode"), dir("file:///home/vscode/foo"), executable_file("file:///home/vscode/bar.txt")],
    );
    resource_options
}

#[test]
fn tilde_pipe_should_return_completion_for_tilde() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = tilde_setup(&f, &guard);
        assert_partial_completions_exist(resolve_resources(&f, &resource_options, "~", 1, None).await, &[expected("~", "/home/")], [0, 1]);
    });
}

#[test]
fn tilde_slash_pipe_should_return_folder_completions_relative_to_home() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = tilde_setup(&f, &guard);
        assert_completions(
            resolve_resources(&f, &resource_options, "~/", 2, None).await,
            &[expected("~/", "/home/"), expected("~/vscode/", "/home/vscode/")],
            [0, 2],
            None,
        );
    });
}

#[test]
fn tilde_slash_vscode_slash_pipe_should_return_folder_completions_relative_to_home_vscode() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = tilde_setup(&f, &guard);
        assert_completions(
            resolve_resources(&f, &resource_options, "~/vscode/", 9, None).await,
            &[expected("~/vscode/", "/home/vscode/"), expected("~/vscode/foo/", "/home/vscode/foo/"), expected_file("~/vscode/bar.txt", "/home/vscode/bar.txt")],
            [0, 9],
            None,
        );
    });
}

// suite('resolveResources edge cases and advanced scenarios')

#[test]
fn c_colon_slash_foo_slash_pipe_absolute_paths_on_windows() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///C:"), None, Some(true), path_separator());
        set_resources(
            &f,
            vec![uri("file:///C:/Foo")],
            vec![FileStatChild { is_file: Some(false), ..dir("file:///C:/Foo/Bar") }, FileStatChild { is_directory: Some(false), ..file("file:///C:/Foo/Baz.txt") }],
        );
        let result = resolve_resources(&f, &resource_options, "C:/Foo/", 7, None).await;

        assert_completions(result, &[expected("C:/Foo/", "C:/Foo/"), expected("C:/Foo/Bar/", "C:/Foo/Bar/")], [0, 7], None);
    });
}

#[test]
fn c_colon_slash_foo_slash_pipe_case_insensitivity_on_windows() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///c:"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///c:/foo")], vec![FileStatChild { is_file: Some(false), ..dir("file:///c:/foo/Bar") }]);
        let result = resolve_resources(&f, &resource_options, "c:/foo/", 7, None).await;

        assert_completions(
            result,
            &[
                // Note that the detail is normalizes drive letters to capital case intentionally
                expected("c:/foo/", "C:/foo/"),
                expected("c:/foo/Bar/", "C:/foo/Bar/"),
            ],
            [0, 7],
            None,
        );
    });
}

#[test]
fn slash_foo_slash_pipe_absolute_paths_not_on_windows() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///"), None, Some(true), path_separator());
        set_resources(
            &f,
            vec![uri("file:///foo")],
            vec![FileStatChild { is_file: Some(false), ..dir("file:///foo/Bar") }, FileStatChild { is_directory: Some(false), ..file("file:///foo/Baz.txt") }],
        );
        let result = resolve_resources(&f, &resource_options, "/foo/", 5, None).await;

        assert_completions(result, &[expected("/foo/", "/foo/"), expected("/foo/Bar/", "/foo/Bar/")], [0, 5], None);
    });
}

#[test]
fn dot_backslash_folder_case_insensitivity_should_resolve_correctly_on_windows() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///C:/test"), None, Some(true), "\\");

        set_resources(&f, vec![uri("file:///C:/test")], vec![dir("file:///C:/test/FolderA/"), dir("file:///C:/test/anotherFolder/")]);

        let result = resolve_resources(&f, &resource_options, ".\\folder", 8, None).await;

        assert_completions(
            result,
            &[
                expected(".\\", "C:\\test\\"),
                expected(".\\FolderA\\", "C:\\test\\FolderA\\"),
                expected(".\\anotherFolder\\", "C:\\test\\anotherFolder\\"),
                expected(".\\..\\", "C:\\"),
            ],
            [0, 8],
            None,
        );
    });
}

#[test]
fn dot_slash_folder_case_sensitivity_should_resolve_correctly_on_mac_unix() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), "/");
        set_resources(&f, vec![uri("file:///test")], vec![dir("file:///test/FolderA/"), dir("file:///test/foldera/")]);

        let result = resolve_resources(&f, &resource_options, "./folder", 8, None).await;

        assert_completions(
            result,
            &[expected("./", "/test/"), expected("./FolderA/", "/test/FolderA/"), expected("./foldera/", "/test/foldera/"), expected("./../", "/")],
            [0, 8],
            None,
        );
    });
}

#[test]
fn pipe_empty_input_should_resolve_to_current_directory() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test")], vec![dir("file:///test/folder1/"), dir("file:///test/folder2/")]);
        let result = resolve_resources(&f, &resource_options, "", 0, None).await;

        assert_completions(
            result,
            &[expected(".", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./folder2/", "/test/folder2/"), expected("../", "/"), standard_tilde_item()],
            [0, 0],
            None,
        );
    });
}

#[test]
fn should_ignore_environment_variable_setting_prefixes() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test")], vec![dir("file:///test/folder1/"), dir("file:///test/folder2/")]);
        let result = resolve_resources(&f, &resource_options, "FOO=./", 2, None).await;

        // Must not include FOO= prefix in completions
        assert_completions(
            result,
            &[expected(".", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./folder2/", "/test/folder2/"), expected("../", "/"), standard_tilde_item()],
            [0, 2],
            None,
        );
    });
}

#[test]
fn should_not_return_completions_when_relative_folder_prefix_does_not_exist() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test")], vec![dir("file:///test/src/"), dir("file:///test/vs/")]);
        let result = resolve_resources(&f, &resource_options, "s/", 2, None).await;

        assert!(result.is_none());
    });
}

#[test]
fn dot_slash_pipe_should_handle_large_directories_with_many_results_gracefully() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test")], (0..1000).map(|i| dir(&format!("file:///test/folder{i}/"))).collect());
        let result = resolve_resources(&f, &resource_options, "./", 2, None).await;

        let result = result.expect("as completions deviam existir");
        // includes the 1000 folders + ./ and ./../
        assert_eq!(result.len(), 1002);
        let sep = path_separator();
        assert_eq!(result[0].label.text(), format!(".{sep}"));
        assert_eq!(result.last().map(|completion| completion.label.text()), Some(format!(".{sep}..{sep}").as_str()));
    });
}

#[test]
fn dot_slash_folder_pipe_should_include_current_folder_with_trailing_slash_is_missing() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test")], vec![dir("file:///test/folder1/"), dir("file:///test/folder2/")]);
        let result = resolve_resources(&f, &resource_options, "./folder1", 10, None).await;

        assert_completions(
            result,
            &[expected("./", "/test/"), expected("./folder1/", "/test/folder1/"), expected("./folder2/", "/test/folder2/"), expected("./../", "/")],
            [1, 10],
            None,
        );
    });
}

#[test]
fn should_resolve_nested_folder_when_name_matches_cwd_basename() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(&f, vec![uri("file:///test"), uri("file:///test/test")], vec![dir("file:///test/test/"), dir("file:///test/test/inner/")]);
        let result = resolve_resources(&f, &resource_options, "test/", 5, None).await;

        assert_completions(
            result,
            &[
                expected("./test/", "/test/test/"),
                expected("./test/inner/", "/test/test/inner/"),
                // ../` from the viewed folder (/test/test/) goes to /test/, not /
                expected("./test/../", "/test/"),
            ],
            [0, 5],
            None,
        );
    });
}

#[test]
fn test_slash_pipe_should_normalize_current_and_parent_folders() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), None, Some(true), path_separator());
        set_resources(
            &f,
            vec![uri("file:///test"), uri("file:///test/folder1"), uri("file:///test/folder2")],
            vec![dir("file:///test/folder1/"), dir("file:///test/folder2/")],
        );
        let result = resolve_resources(&f, &resource_options, "./test/", 7, None).await;

        assert_completions(
            result,
            &[expected("./test/", "/test/"), expected("./test/folder1/", "/test/folder1/"), expected("./test/folder2/", "/test/folder2/"), expected("./test/../", "/")],
            [0, 7],
            None,
        );
    });
}

// suite('cdpath')

/// O `setup` da suíte `cdpath`: os recursos e o ambiente do shell com `CDPATH`.
fn cdpath_setup(f: &TerminalCompletionFixture, guard: &common::LeakGuard) -> Rc<ShellEnvDetectionCapability> {
    set_resources(
        f,
        vec![uri("file:///test"), uri("file:///cdpath_value")],
        vec![dir("file:///cdpath_value/folder1/"), file("file:///cdpath_value/file1.txt")],
    );

    add_shell_env_detection(f, guard, &[("CDPATH", "/cdpath_value")])
}

#[test]
fn cd_pipe_should_show_paths_from_cdpath_relative() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        cdpath_setup(&f, &guard);
        let _ = f.configuration_service.set_user_configuration("terminal.integrated.suggest.cdPath", Dynamic::string("relative"), None);
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "cd ", 3, None).await;

        assert_partial_completions_exist(result, &[expected("folder1", "CDPATH /cdpath_value/folder1/")], [3, 3]);
    });
}

#[test]
fn cd_pipe_should_show_paths_from_cdpath_absolute() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        cdpath_setup(&f, &guard);
        let _ = f.configuration_service.set_user_configuration("terminal.integrated.suggest.cdPath", Dynamic::string("absolute"), None);
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "cd ", 3, None).await;

        assert_partial_completions_exist(result, &[expected("/cdpath_value/folder1/", "CDPATH")], [3, 3]);
    });
}

#[test]
fn cd_pipe_should_support_pulling_from_multiple_paths_in_cdpath() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let shell_env_detection = cdpath_setup(&f, &guard);
        let _ = f.configuration_service.set_user_configuration("terminal.integrated.suggest.cdPath", Dynamic::string("relative"), None);
        let path_prefix = if is_windows() { "c:\\" } else { "/" };
        let delimiter = if is_windows() { ";" } else { ":" };
        let separator = if is_windows() { "\\" } else { "/" };
        shell_env_detection.set_environment(
            Some(&env_map(&[("CDPATH", &format!("{path_prefix}cdpath1_value{delimiter}{path_prefix}cdpath2_value{separator}inner_dir"))])),
            true,
        );

        let uri_path_prefix = if is_windows() { "file:///c:/" } else { "file:///" };
        set_resources(
            &f,
            vec![
                uri(&format!("{uri_path_prefix}test")),
                uri(&format!("{uri_path_prefix}cdpath1_value")),
                uri(&format!("{uri_path_prefix}cdpath2_value")),
                uri(&format!("{uri_path_prefix}cdpath2_value/inner_dir")),
            ],
            vec![
                dir(&format!("{uri_path_prefix}cdpath1_value/folder1/")),
                dir(&format!("{uri_path_prefix}cdpath1_value/folder2/")),
                file(&format!("{uri_path_prefix}cdpath1_value/file1.txt")),
                dir(&format!("{uri_path_prefix}cdpath2_value/inner_dir/folder1/")),
                dir(&format!("{uri_path_prefix}cdpath2_value/inner_dir/folder2/")),
                file(&format!("{uri_path_prefix}cdpath2_value/inner_dir/file1.txt")),
            ],
        );

        let resource_options = options(uri(&format!("{uri_path_prefix}test")), Some(true), Some(true), path_separator());
        let result = resolve_resources(&f, &resource_options, "cd ", 3, None).await;

        let final_prefix = if is_windows() { "C:\\" } else { "/" };
        assert_partial_completions_exist(
            result,
            &[
                expected("folder1", &format!("CDPATH {final_prefix}cdpath1_value/folder1/")),
                expected("folder2", &format!("CDPATH {final_prefix}cdpath1_value/folder2/")),
                expected("folder1", &format!("CDPATH {final_prefix}cdpath2_value/inner_dir/folder1/")),
                expected("folder2", &format!("CDPATH {final_prefix}cdpath2_value/inner_dir/folder2/")),
            ],
            [3, 3],
        );
    });
}

// suite('gitbash')

#[test]
fn should_convert_git_bash_absolute_path_to_windows_absolute_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(git_bash_to_windows_path("/", None), "C:\\");
    assert_eq!(git_bash_to_windows_path("/c/", None), "C:\\");
    assert_eq!(git_bash_to_windows_path("/c/Users/foo", None), "C:\\Users\\foo");
    assert_eq!(git_bash_to_windows_path("/d/bar", None), "D:\\bar");
}

#[test]
fn should_convert_windows_absolute_path_to_git_bash_absolute_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(windows_to_git_bash_path("C:\\"), "/c/");
    assert_eq!(windows_to_git_bash_path("C:\\Users\\foo"), "/c/Users/foo");
    assert_eq!(windows_to_git_bash_path("D:\\bar"), "/d/bar");
    assert_eq!(windows_to_git_bash_path("E:\\some\\path"), "/e/some/path");
}

/// O `setup` dos três casos `resolveResources` do Git Bash: `C:\Users\foo` com uma pasta e um arquivo.
fn git_bash_setup(f: &TerminalCompletionFixture) -> TerminalCompletionResourceOptions {
    let resource_options = options(uri_file("C:\\Users\\foo"), Some(true), Some(true), "/");
    set_resources(
        f,
        vec![uri_file("C:\\Users\\foo"), uri_file("C:\\Users\\foo\\bar"), uri_file("C:\\Users\\foo\\baz.txt")],
        vec![
            FileStatChild { resource: uri_file("C:\\Users\\foo\\bar"), is_directory: Some(true), is_file: Some(false), ..child("file:///") },
            FileStatChild { resource: uri_file("C:\\Users\\foo\\baz.txt"), is_file: Some(true), executable: Some(true), ..child("file:///") },
        ],
    );
    resource_options
}

#[test]
fn resolve_resources_with_c_colon_slash_style_absolute_path_for_git_bash() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = git_bash_setup(&f);
        let result = resolve_resources(&f, &resource_options, "C:/Users/foo/", 13, shell(WindowsShellType::GitBash)).await;
        assert_completions(
            result,
            &[
                expected("C:/Users/foo/", "C:\\Users\\foo\\"),
                expected("C:/Users/foo/bar/", "C:\\Users\\foo\\bar\\"),
                expected_file("C:/Users/foo/baz.txt", "C:\\Users\\foo\\baz.txt"),
            ],
            [0, 13],
            Some("/"),
        );
    });
}

#[test]
fn resolve_resources_with_cwd_as_windows_path_relative() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = git_bash_setup(&f);
        let result = resolve_resources(&f, &resource_options, "./", 2, shell(WindowsShellType::GitBash)).await;
        assert_completions(
            result,
            &[
                expected("./", "C:\\Users\\foo\\"),
                expected("./bar/", "C:\\Users\\foo\\bar\\"),
                expected_file("./baz.txt", "C:\\Users\\foo\\baz.txt"),
                expected("./../", "C:\\Users\\"),
            ],
            [0, 2],
            Some("/"),
        );
    });
}

#[test]
fn resolve_resources_with_cwd_as_windows_path_absolute() {
    if !is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = git_bash_setup(&f);
        let result = resolve_resources(&f, &resource_options, "/c/Users/foo/", 13, shell(WindowsShellType::GitBash)).await;
        assert_completions(
            result,
            &[
                expected("/c/Users/foo/", "C:\\Users\\foo\\"),
                expected("/c/Users/foo/bar/", "C:\\Users\\foo\\bar\\"),
                expected_file("/c/Users/foo/baz.txt", "C:\\Users\\foo\\baz.txt"),
            ],
            [0, 13],
            Some("/"),
        );
    });
}

// suite('symlink support')

#[test]
fn should_include_symlink_target_information_in_completions() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());

        set_resources(&f, vec![uri("file:///test")], vec![]);

        // Create mock children including a symbolic link
        *f.child_resources.borrow_mut() = vec![
            file("file:///test/regular-file.txt"),
            FileStatChild { is_symbolic_link: Some(true), ..file("file:///test/symlink-file") },
            FileStatChild { is_symbolic_link: Some(true), ..dir("file:///test/symlink-folder") },
            dir("file:///test/regular-folder"),
        ];

        let result = resolve_resources(&f, &resource_options, "ls ", 3, None).await;

        // Find the symlink completion
        let result = result.expect("as completions deviam existir");
        let symlink_file_completion = result.iter().find(|c| c.label.text() == "./symlink-file");
        let symlink_folder_completion = result.iter().find(|c| c.label.text() == "./symlink-folder/");
        assert_eq!(
            symlink_file_completion.and_then(ITerminalCompletion::detail).as_deref(),
            Some("/test/symlink-file -> /target/actual-file.txt"),
            "Symlink file detail should match target"
        );
        assert_eq!(
            symlink_folder_completion.and_then(ITerminalCompletion::detail).as_deref(),
            Some("/test/symlink-folder -> /target/actual-folder"),
            "Symlink folder detail should match target"
        );
    });
}

// suite('remote file completion (e.g. WSL)')

fn remote_test_env() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([("HOME".to_string(), "/home/remoteuser".to_string()), ("USERPROFILE".to_string(), "/home/remoteuser".to_string())])
}

#[test]
fn slash_absolute_path_should_preserve_remote_authority() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        f.terminal_completion_service.set_process_env(remote_test_env());
        let resource_options = options(remote_uri("/home/remoteuser"), None, Some(true), "/");
        set_resources(
            &f,
            vec![remote_uri("/home"), remote_uri("/home/remoteuser")],
            vec![FileStatChild { resource: remote_uri("/home/remoteuser"), is_directory: Some(true), ..child("file:///") }],
        );
        let result = resolve_resources(&f, &resource_options, "/home/", 6, None).await;

        // Check that results exist and have the correct scheme/authority
        let result = result.filter(|completions| !completions.is_empty()).expect("Should return completions for remote absolute path");
        // Verify completions contain paths resolved via the remote file service (not local file://)
        let absolute_completion = result.iter().find(|c| c.label.text() == "/home/").expect("Should have absolute path completion");
        assert!(absolute_completion.detail().is_some_and(|detail| detail.contains("/home/")), "Detail should show remote path");
    });
}

#[test]
fn tilde_slash_should_preserve_remote_authority_for_tilde_expansion() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        f.terminal_completion_service.set_process_env(remote_test_env());
        let resource_options = options(remote_uri("/home/remoteuser/project"), None, Some(true), "/");
        set_resources(
            &f,
            vec![remote_uri("/home/remoteuser"), remote_uri("/home/remoteuser/project")],
            vec![
                FileStatChild { resource: remote_uri("/home/remoteuser/Documents"), is_directory: Some(true), ..child("file:///") },
                FileStatChild { resource: remote_uri("/home/remoteuser/project"), is_directory: Some(true), ..child("file:///") },
            ],
        );
        let result = resolve_resources(&f, &resource_options, "~/", 2, None).await;

        // Check that results exist for remote tilde path
        let result = result.filter(|completions| !completions.is_empty()).expect("Should return completions for remote tilde path");
        // Verify the tilde path was resolved using the remote home directory
        let documents_completion = result.iter().find(|c| c.detail().is_some_and(|detail| detail.contains("Documents")));
        assert!(documents_completion.is_some(), "Should find Documents folder from remote home");
    });
}

#[test]
fn dot_slash_relative_should_preserve_remote_authority_for_relative_paths() {
    if is_windows() {
        return;
    }
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        f.terminal_completion_service.set_process_env(remote_test_env());
        let resource_options = options(remote_uri("/home/remoteuser/project"), None, Some(true), "/");
        set_resources(
            &f,
            vec![remote_uri("/home/remoteuser/project")],
            vec![
                FileStatChild { resource: remote_uri("/home/remoteuser/project/src"), is_directory: Some(true), ..child("file:///") },
                FileStatChild { resource: remote_uri("/home/remoteuser/project/docs"), is_directory: Some(true), ..child("file:///") },
            ],
        );
        let result = resolve_resources(&f, &resource_options, "./", 2, None).await;

        // Check that results exist for remote relative path
        let result = result.filter(|completions| !completions.is_empty()).expect("Should return completions for remote relative path");
        // Verify completions are from the remote filesystem
        let src_completion = result.iter().find(|c| c.detail().is_some_and(|detail| detail.contains("/home/remoteuser/project/src")));
        assert!(src_completion.is_some(), "Should find src folder completion with remote path in detail");
    });
}

// suite('completion label escaping')

#[test]
fn pipe_should_escape_special_characters_in_file_folder_names_for_posix_shells() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = TerminalCompletionFixture::new(&guard.store());
        let resource_options = options(uri("file:///test"), Some(true), Some(true), path_separator());
        set_resources(
            &f,
            vec![uri("file:///test")],
            vec![dir("file:///test/[folder1]/"), dir("file:///test/folder 2/"), dir("file:///test/!special$chars&/"), executable_file("file:///test/!special$chars2&")],
        );
        let result = resolve_resources(&f, &resource_options, "", 0, None).await;

        assert_completions(
            result,
            &[
                expected(".", "/test/"),
                expected("./[folder1]/", "/test/[folder1]/"),
                expected("./folder 2/", "/test/folder 2/"),
                expected("./!special$chars&/", "/test/!special$chars&/"),
                expected_file("./!special$chars2&", "/test/!special$chars2&"),
                expected("../", "/"),
                standard_tilde_item(),
            ],
            [0, 0],
            None,
        );
    });
}

// suite('Provider Configuration')

/// `result.includes(provider)`: a identidade do objeto.
fn includes(providers: &[Rc<dyn ITerminalCompletionProvider>], provider: &Rc<MockCompletionProvider>) -> bool {
    providers.iter().any(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(provider)))
}

/// `{ [id]: enabled, ... }` da configuração dos provedores.
fn providers_configuration(entries: &[(&str, bool)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(id, enabled)| (*id, Dynamic::Bool(*enabled))))
}

/// `providers` do caso como a lista que o serviço recebe.
fn as_providers(providers: &[&Rc<MockCompletionProvider>]) -> Vec<Rc<dyn ITerminalCompletionProvider>> {
    providers.iter().map(|provider| (*provider).clone() as Rc<dyn ITerminalCompletionProvider>).collect()
}

#[test]
fn should_enable_providers_by_default_when_no_configuration_exists() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = TerminalCompletionFixture::new(&guard.store());
    let test_terminal_completion_service = f.create_service();
    let default_provider = MockCompletionProvider::new("terminal-suggest");
    let new_provider = MockCompletionProvider::new("new-extension-provider");
    let providers = as_providers(&[&default_provider, &new_provider]);

    // Set empty configuration (no provider keys)
    let _ = f.configuration_service.set_user_configuration(TerminalSuggestSettingId::Providers.as_str(), providers_configuration(&[]), None);

    let result = test_terminal_completion_service.get_enabled_providers(providers);

    // Both providers should be enabled since they're not explicitly disabled
    assert_eq!(result.len(), 2, "Should enable both providers by default");
    assert!(includes(&result, &default_provider), "Should include default provider");
    assert!(includes(&result, &new_provider), "Should include new provider");
}

#[test]
fn should_disable_providers_when_explicitly_set_to_false() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = TerminalCompletionFixture::new(&guard.store());
    let test_terminal_completion_service = f.create_service();
    let provider1 = MockCompletionProvider::new("provider1");
    let provider2 = MockCompletionProvider::new("provider2");
    let providers = as_providers(&[&provider1, &provider2]);

    // Disable provider1, leave provider2 unconfigured
    let _ = f.configuration_service.set_user_configuration(TerminalSuggestSettingId::Providers.as_str(), providers_configuration(&[("provider1", false)]), None);

    let result = test_terminal_completion_service.get_enabled_providers(providers);

    // Only provider2 should be enabled
    assert_eq!(result.len(), 1, "Should enable only one provider");
    assert!(includes(&result, &provider2), "Should include unconfigured provider");
    assert!(!includes(&result, &provider1), "Should not include disabled provider");
}

#[test]
fn should_enable_providers_when_explicitly_set_to_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = TerminalCompletionFixture::new(&guard.store());
    let test_terminal_completion_service = f.create_service();
    let provider1 = MockCompletionProvider::new("provider1");
    let provider2 = MockCompletionProvider::new("provider2");
    let providers = as_providers(&[&provider1, &provider2]);

    // Explicitly enable provider1, leave provider2 unconfigured
    let _ = f.configuration_service.set_user_configuration(TerminalSuggestSettingId::Providers.as_str(), providers_configuration(&[("provider1", true)]), None);

    let result = test_terminal_completion_service.get_enabled_providers(providers);

    // Both providers should be enabled
    assert_eq!(result.len(), 2, "Should enable both providers");
    assert!(includes(&result, &provider1), "Should include explicitly enabled provider");
    assert!(includes(&result, &provider2), "Should include unconfigured provider");
}

#[test]
fn should_handle_mixed_configuration_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = TerminalCompletionFixture::new(&guard.store());
    let test_terminal_completion_service = f.create_service();
    let provider1 = MockCompletionProvider::new("provider1");
    let provider2 = MockCompletionProvider::new("provider2");
    let provider3 = MockCompletionProvider::new("provider3");
    let providers = as_providers(&[&provider1, &provider2, &provider3]);

    // Mixed configuration: enable provider1, disable provider2, leave provider3 unconfigured
    let _ = f.configuration_service.set_user_configuration(
        TerminalSuggestSettingId::Providers.as_str(),
        providers_configuration(&[("provider1", true), ("provider2", false)]),
        None,
    );

    let result = test_terminal_completion_service.get_enabled_providers(providers);

    // provider1 and provider3 should be enabled, provider2 should be disabled
    assert_eq!(result.len(), 2, "Should enable two providers");
    assert!(includes(&result, &provider1), "Should include explicitly enabled provider");
    assert!(includes(&result, &provider3), "Should include unconfigured provider");
    assert!(!includes(&result, &provider2), "Should not include disabled provider");
}
