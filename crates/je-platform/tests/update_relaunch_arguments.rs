//! Porte de `src/vs/platform/update/test/electron-main/updateRelaunchArguments.test.ts` (suíte `Win32UpdateService - relaunch arguments`), caso a caso e na mesma ordem.
//!
//! Divergências: `Partial<NativeParsedArgs>` é um literal no estilo do TypeScript (`j("{...}")`, em texto
//! cru para as barras invertidas passarem em dobro pelo JSON); `{ _: [], ...overrides }` só acrescenta `_`
//! quando o literal não traz o seu. `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar.

mod common;

use common::j;
use je_base::dynamic::Dynamic;
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::update::update_relaunch_arguments::{get_relaunch_arguments, quote_windows_argument};

fn args(overrides: &str) -> NativeParsedArgs {
    let args = NativeParsedArgs::from_dynamic(&j(overrides));
    if !args.has("_") {
        args.set("_", Dynamic::array(vec![]));
    }
    args
}

fn get_arguments(overrides: &str, raw_args: &[&str]) -> String {
    get_relaunch_arguments(&args(overrides), raw_args, Some(r"C:\cwd"))
}

#[test]
fn quote_windows_argument_cases() {
    assert_eq!(quote_windows_argument("--disable-gpu"), "--disable-gpu");
    assert_eq!(quote_windows_argument(r"C:\Users\test\ext"), r"C:\Users\test\ext");
    assert_eq!(quote_windows_argument(r"C:\path with space\ext"), r#""C:\path with space\ext""#);
    assert_eq!(quote_windows_argument(r#"a"b"#), r#""a\"b""#);
    assert_eq!(quote_windows_argument(r"C:\ends with slash\"), r#""C:\ends with slash\\""#);
}

#[test]
fn carries_forward_curated_path_and_flag_arguments() {
    let result = get_arguments(
        r"{'user-data-dir': 'C:\\data', 'extensions-dir': 'C:\\path with space\\ext', 'disable-gpu': true, 'disable-lcd-text': true}",
        &[],
    );

    assert_eq!(result, r#"--user-data-dir=C:\data "--extensions-dir=C:\path with space\ext" --disable-gpu --disable-lcd-text"#);
}

#[test]
fn returns_empty_string_when_no_relevant_arguments_are_present() {
    assert_eq!(get_arguments("{}", &[]), "");
}

#[test]
fn ignores_transient_and_one_shot_arguments() {
    let result = get_arguments(
        r"{'_': ['C:\\some\\file.txt'], 'wait': true, 'new-window': true, 'install-extension': ['some.extension'], 'profile': 'work', 'profile-temp': true, 'crash-reporter-id': 'derived-id', 'logsPath': 'C:\\logs', 'extensions-dir': 'C:\\ext'}",
        &[],
    );

    assert_eq!(result, r"--extensions-dir=C:\ext");
}

#[test]
fn carries_forward_additional_environment_string_and_boolean_arguments() {
    let result = get_arguments("{'proxy-server': 'http://localhost:8080', 'disable-updates': true}", &["--no-sandbox"]);

    assert_eq!(result, "--proxy-server=http://localhost:8080 --disable-updates --no-sandbox");
}

#[test]
fn carries_forward_explicit_negated_flags_from_raw_arguments() {
    let result = get_arguments("{'no-sandbox': false, 'no-proxy-server': false}", &["--no-sandbox", "--no-proxy-server"]);

    assert_eq!(result, "--no-sandbox --no-proxy-server");
}

#[test]
fn carries_forward_string_values_that_start_with_a_hyphen() {
    let result = get_arguments("{'js-flags': '--max-old-space-size=8192', 'enable-tracing': '-*,v8'}", &[]);

    assert_eq!(result, "--js-flags=--max-old-space-size=8192 --enable-tracing=-*,v8");
}

#[test]
fn ignores_flag_arguments_that_are_not_set_to_true() {
    let result = get_arguments("{'disable-gpu': false, 'user-data-dir': ''}", &[]);

    assert_eq!(result, "");
}

#[test]
fn resolves_relative_path_arguments_against_the_current_working_directory() {
    let result = get_arguments(r"{'extensions-dir': '.\\extensions', 'user-data-dir': '..\\data', 'trace-startup-file': 'trace.json', 'locale': 'de'}", &[]);

    assert_eq!(result, r"--user-data-dir=C:\data --extensions-dir=C:\cwd\extensions --locale=de --trace-startup-file=C:\cwd\trace.json");
}

#[test]
fn ignores_negated_flags_that_appear_after_the_end_of_options_marker() {
    let result = get_arguments(r"{'extensions-dir': 'C:\\ext'}", &["--no-proxy-server", "--", "--no-sandbox"]);

    assert_eq!(result, r"--extensions-dir=C:\ext --no-proxy-server");
}
