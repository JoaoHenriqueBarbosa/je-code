//! Testes próprios de `processes.rs`, que o `processes.test.ts` do VS Code não cobre: a lista de chaves
//! preservadas e as bordas das expressões de `keysToRemove`. Não é porte de teste do VS Code.

use je_base::process::ProcessEnvironment;
use je_base::processes::sanitize_process_environment;

fn env(entries: &[(&str, &str)]) -> ProcessEnvironment {
    entries.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn sanitize_process_environment_preserves_the_given_keys() {
    let mut e = env(&[("ELECTRON_RUN_AS_NODE", "1"), ("SNAP", "x"), ("SNAP_NAME", "y"), ("SNAPSHOT", "z"), ("VSCODE_ENV_APPEND", "a"), ("VSCODE_ENV_APPENDX", "b")]);
    sanitize_process_environment(&mut e, &["ELECTRON_RUN_AS_NODE"]);
    // `SNAPSHOT` não casa com `^SNAP(|_.*)$`; `VSCODE_ENV_APPENDX` fica pelo lookahead negativo.
    assert_eq!(e, env(&[("ELECTRON_RUN_AS_NODE", "1"), ("SNAPSHOT", "z"), ("VSCODE_ENV_APPEND", "a"), ("VSCODE_ENV_APPENDX", "b")]));
}
