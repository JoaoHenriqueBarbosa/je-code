//! Porte de `src/vs/workbench/contrib/debug/test/node/debugAdapter.test.ts` (suíte `Debug - Debug Adapter`), caso a caso e na mesma ordem.
//!
//! NÃO foi portado (sem `#[test]`): `round-trips Windows batch arguments without executing metacharacters`. O original
//! pula o caso fora do Windows (`this.skip()`) e ele só tem sentido executando o `cmd.exe` real com um `.cmd` e um script
//! `node`; neste porte (sem `child_process`) não há o que rodar, e um caso que sempre pula não prova nada.
//!
//! Os 3 casos portados: `escapes Windows batch commands and arguments`, `escapes backslash runs around quotes`,
//! `rejects invalid Windows batch command characters`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso.
//! - As cadeias são literais brutos com as barras já desfeitas do escape do TypeScript.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::debug::debug_adapter::prepare_windows_batch_command;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn escapes_windows_batch_commands_and_arguments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        prepare_windows_batch_command(
            r"C:\Program Files\adapter.cmd",
            &strings(&["plain", "with spaces", r#"quote" & calc.exe & ""#, "|<>()^%!", r"C:\path\", "%PATH:z=z%"])
        )
        .unwrap(),
        strings(&[
            "/e:ON",
            "/v:OFF",
            "/d",
            "/c",
            r#"""C:\Program Files\adapter.cmd" plain "with spaces" "quote"" & calc.exe & """ "|<>()^%%cd:~,%!" "C:\path\\" "%%cd:~,%PATH:z=z%%cd:~,%"""#
        ])
    );
}

#[test]
fn escapes_backslash_runs_around_quotes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        prepare_windows_batch_command("adapter.cmd", &strings(&[r"two\\", r"three\\\", r#"two\\"quote"#, r#"three\\\"quote"#])).unwrap(),
        strings(&[
            "/e:ON",
            "/v:OFF",
            "/d",
            "/c",
            r#"""adapter.cmd" "two\\\\" "three\\\\\\" "two\\\\""quote" "three\\\\\\""quote"""#
        ])
    );
}

#[test]
fn rejects_invalid_windows_batch_command_characters() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let results = [
        prepare_windows_batch_command("adapter.cmd", &strings(&["safe\r\ncalc.exe"])).is_err(),
        prepare_windows_batch_command("adapter.cmd", &strings(&["safe\0calc.exe"])).is_err(),
        prepare_windows_batch_command("adapter\".cmd", &[]).is_err(),
    ];
    assert_eq!(results, [true, true, true]);
}
