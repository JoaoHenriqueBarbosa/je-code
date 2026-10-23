//! Porte de `src/vs/base/test/common/processes.test.ts` (suíte `Processes`), caso a caso e na mesma ordem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::process::ProcessEnvironment;
use je_base::processes;

fn env(entries: &[(&str, &str)]) -> ProcessEnvironment {
    entries.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn sanitize_process_environment() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let mut e = env(&[
        ("FOO", "bar"),
        ("ELECTRON_ENABLE_STACK_DUMPING", "x"),
        ("ELECTRON_ENABLE_LOGGING", "x"),
        ("ELECTRON_NO_ASAR", "x"),
        ("ELECTRON_NO_ATTACH_CONSOLE", "x"),
        ("ELECTRON_RUN_AS_NODE", "x"),
        ("VSCODE_CLI", "x"),
        ("VSCODE_DEV", "x"),
        ("VSCODE_IPC_HOOK", "x"),
        ("VSCODE_NLS_CONFIG", "x"),
        ("VSCODE_PORTABLE", "3"),
        ("VSCODE_PID", "x"),
        ("VSCODE_SHELL_LOGIN", "1"),
        ("VSCODE_CODE_CACHE_PATH", "x"),
        ("VSCODE_NEW_VAR", "x"),
        ("GDK_PIXBUF_MODULE_FILE", "x"),
        ("GDK_PIXBUF_MODULEDIR", "x"),
        ("VSCODE_PYTHON_BASH_ACTIVATE", "source /path/to/venv/bin/activate"),
        ("VSCODE_PYTHON_ZSH_ACTIVATE", "source /path/to/venv/bin/activate"),
        ("VSCODE_PYTHON_PWSH_ACTIVATE", ". /path/to/venv/Scripts/Activate.ps1"),
        ("VSCODE_PYTHON_FISH_ACTIVATE", "source /path/to/venv/bin/activate.fish"),
        ("VSCODE_PYTHON_AUTOACTIVATE_GUARD", "1"),
    ]);
    processes::sanitize_process_environment(&mut e, &[]);
    assert_eq!(e.get("FOO").map(String::as_str), Some("bar"));
    assert_eq!(e.get("VSCODE_SHELL_LOGIN").map(String::as_str), Some("1"));
    assert_eq!(e.get("VSCODE_PORTABLE").map(String::as_str), Some("3"));
    assert_eq!(e.get("VSCODE_PYTHON_BASH_ACTIVATE"), None);
    assert_eq!(e.get("VSCODE_PYTHON_ZSH_ACTIVATE"), None);
    assert_eq!(e.get("VSCODE_PYTHON_PWSH_ACTIVATE"), None);
    assert_eq!(e.get("VSCODE_PYTHON_FISH_ACTIVATE"), None);
    assert_eq!(e.get("VSCODE_PYTHON_AUTOACTIVATE_GUARD"), None);
    assert_eq!(e.len(), 3);
}

#[test]
fn remove_dangerous_env_variables() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let mut e = env(&[
        ("SAFE", "value"),
        ("DEBUG", "value"),
        ("debug", "value"),
        ("NODE_OPTIONS", "--import=data:text/javascript,"),
        ("Node_Options", "--require=module"),
        ("VSCODE_NODE_OPTIONS", "--import=data:text/javascript,"),
        ("Vscode_Node_Options", "--require=module"),
        ("LD_PRELOAD", "library"),
        ("ld_preload", "library"),
        ("DYLD_INSERT_LIBRARIES", "library"),
        ("Dyld_Insert_Libraries", "library"),
    ]);

    processes::remove_dangerous_env_variables(Some(&mut e));

    assert_eq!(e, env(&[("SAFE", "value")]));
}
