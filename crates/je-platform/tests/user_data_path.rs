//! Porte de `src/vs/platform/environment/test/node/userDataPath.test.ts` (suíte `User data path`), caso a caso e na mesma ordem.
//!
//! Divergências: o original muda `process.env['VSCODE_PORTABLE']` e `process.env['VSCODE_APPDATA']` e
//! restaura no `finally`. Alterar o ambiente do processo exige `unsafe` (proibido no projeto) e quebraria os
//! testes em paralelo, então cada caso passa um [`FixedEnvironment`] com a variável e o resto igual. O
//! `process.argv` é o argv do executável de teste.

use je_base::dynamic::Dynamic;
use je_base::path::{FixedEnvironment, Platform};
use je_platform::environment::argv::{IgnoringReporter, OPTIONS, parse_args};
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::environment::user_data_path::{ProcessPlatform, get_user_data_path_in};
use je_platform::product::ProductService;

fn process_args() -> NativeParsedArgs {
    let argv: Vec<String> = std::env::args().collect();
    NativeParsedArgs::from_dynamic(&parse_args(&argv, &OPTIONS, &IgnoringReporter))
}

fn environment(variables: &[(&str, &str)]) -> FixedEnvironment {
    let mut env = FixedEnvironment::new(Platform::Posix, "/work");
    for (name, value) in variables {
        env.vars.insert((*name).to_owned(), (*value).to_owned());
    }
    env
}

fn path_of(env: &FixedEnvironment, args: &NativeParsedArgs) -> String {
    get_user_data_path_in(env, ProcessPlatform::Linux, "/home/user", args, &ProductService::default().name_short).unwrap()
}

#[test]
fn get_user_data_path_default() {
    let path = path_of(&environment(&[]), &process_args());
    assert!(!path.is_empty());
    assert_eq!(path, "/home/user/.config/Code");
}

#[test]
fn get_user_data_path_portable_mode() {
    let portable_dir = "portable-dir";

    let path = path_of(&environment(&[("VSCODE_PORTABLE", portable_dir)]), &process_args());

    assert!(path.contains(portable_dir));
}

#[test]
fn get_user_data_path_user_data_dir() {
    let cli_user_data_dir = "cli-data-dir";
    let args = process_args();
    args.set("user-data-dir", Dynamic::string(cli_user_data_dir));

    let path = path_of(&environment(&[]), &args);

    assert!(path.contains(cli_user_data_dir));
}

#[test]
fn get_user_data_path_vscode_appdata() {
    let app_data_dir = "appdata-dir";

    let path = path_of(&environment(&[("VSCODE_APPDATA", app_data_dir)]), &process_args());

    assert!(path.contains(app_data_dir));
}
