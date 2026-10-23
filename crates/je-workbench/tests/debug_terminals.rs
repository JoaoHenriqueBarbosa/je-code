//! Porte de `src/vs/workbench/contrib/debug/test/node/terminals.test.ts` (suíte `Debug - prepareCommand`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `prepareCommand(shell, args, ...)` com `trim()` vira o auxiliar `prepare`; o sistema só importa
//!   para o shell padrão e para a letra de unidade, e todos os casos nomeiam o shell e não passam `cwd`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::debug::terminals::{EnvEntry, prepare_command};

fn prepare(shell: &str, args: &[&str], args_can_be_interpreted_by_shell: bool) -> String {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    prepare_command(shell, &args, args_can_be_interpreted_by_shell, None, None).trim().to_string()
}

fn prepare_env(shell: &str, env: &[EnvEntry<'_>]) -> String {
    prepare_command(shell, &[], false, None, Some(env)).trim().to_string()
}

#[test]
fn bash() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(prepare("bash", &["{$} ("], false), "\\{\\$\\}\\ \\(");
    assert_eq!(prepare("bash", &["hello", "world", "--flag=true"], false), "hello world --flag=true");
    assert_eq!(prepare("bash", &[" space arg "], false), "\\ space\\ arg\\");

    assert_eq!(prepare("bash", &["{$} ("], true), "{$} (");
    assert_eq!(prepare("bash", &["hello", "world", "--flag=true"], true), "hello world --flag=true");
    assert_eq!(prepare("bash", &[" space arg "], true), "space arg");
}

#[test]
fn bash_do_not_escape_greater_and_less_than() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        prepare("bash", &["arg1", ">", "> hello.txt", "<", "<input.in"], false),
        "arg1 > \\>\\ hello.txt < \\<input.in"
    );
}

#[test]
fn cmd() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(prepare("cmd.exe", &["^!< "], false), "\"^^^!^< \"");
    assert_eq!(prepare("cmd.exe", &["hello", "world", "--flag=true"], false), "hello world --flag=true");
    assert_eq!(prepare("cmd.exe", &[" space arg "], false), "\" space arg \"");
    assert_eq!(prepare("cmd.exe", &["\"A>0\""], false), "\"\"\"A^>0\"\"\"");
    assert_eq!(prepare("cmd.exe", &[""], false), "\"\"");

    assert_eq!(prepare("cmd.exe", &["^!< "], true), "^!<");
    assert_eq!(prepare("cmd.exe", &["hello", "world", "--flag=true"], true), "hello world --flag=true");
    assert_eq!(prepare("cmd.exe", &[" space arg "], true), "space arg");
    assert_eq!(prepare("cmd.exe", &["\"A>0\""], true), "\"A>0\"");
    assert_eq!(prepare("cmd.exe", &[""], true), "");
}

#[test]
fn cmd_do_not_escape_greater_and_less_than() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        prepare("cmd.exe", &["arg1", ">", "> hello.txt", "<", "<input.in"], false),
        "arg1 > \"^> hello.txt\" < ^<input.in"
    );
}

#[test]
fn powershell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(prepare("powershell", &["!< "], false), "& '!< '");
    assert_eq!(prepare("powershell", &["hello", "world", "--flag=true"], false), "& 'hello' 'world' '--flag=true'");
    assert_eq!(prepare("powershell", &[" space arg "], false), "& ' space arg '");
    assert_eq!(prepare("powershell", &["\"A>0\""], false), "& '\"A>0\"'");
    assert_eq!(prepare("powershell", &[""], false), "& ''");

    assert_eq!(prepare("powershell", &["!< "], true), "!<");
    assert_eq!(prepare("powershell", &["hello", "world", "--flag=true"], true), "hello world --flag=true");
    assert_eq!(prepare("powershell", &[" space arg "], true), "space arg");
    assert_eq!(prepare("powershell", &["\"A>0\""], true), "\"A>0\"");
    assert_eq!(prepare("powershell", &[""], true), "");
}

#[test]
fn powershell_do_not_escape_greater_and_less_than() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        prepare("powershell", &["arg1", ">", "> hello.txt", "<", "<input.in"], false),
        "& 'arg1' > '> hello.txt' < '<input.in'"
    );
}

#[test]
fn powershell_quotes_environment_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let left_single_quotation_mark = '\u{2018}';
    let right_single_quotation_mark = '\u{2019}';

    assert_eq!(
        vec![
            prepare_env("powershell", &[("SIMPLE", Some("hello"))]),
            prepare_env("powershell", &[("SPACES", Some("hello world"))]),
            prepare_env("powershell", &[("EMPTY", Some(""))]),
            prepare_env("powershell", &[("QUOTE", Some("hello'world"))]),
            prepare_env("powershell", &[("MULTI", Some("it's 'ok'"))]),
            prepare_env("powershell", &[("LEFT_QUOTE", Some(&format!("hello{left_single_quotation_mark}world")))]),
            prepare_env("powershell", &[("RIGHT_QUOTE", Some(&format!("hello{right_single_quotation_mark}world")))]),
            prepare_env("powershell", &[("TRAILING", Some("C:\\work\\"))]),
            prepare_env("powershell", &[("BACKSLASH_QUOTE", Some("C:\\it's\\path\\"))]),
        ],
        vec![
            "${env:SIMPLE}='hello';".to_string(),
            "${env:SPACES}='hello world';".to_string(),
            "${env:EMPTY}='';".to_string(),
            "${env:QUOTE}='hello''world';".to_string(),
            "${env:MULTI}='it''s ''ok''';".to_string(),
            format!("${{env:LEFT_QUOTE}}='hello{left_single_quotation_mark}{left_single_quotation_mark}world';"),
            format!("${{env:RIGHT_QUOTE}}='hello{right_single_quotation_mark}{right_single_quotation_mark}world';"),
            "${env:TRAILING}='C:\\work\\';".to_string(),
            "${env:BACKSLASH_QUOTE}='C:\\it''s\\path\\';".to_string(),
        ]
    );
}
