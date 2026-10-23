//! Porte de `src/vs/platform/environment/test/node/environmentService.test.ts` (suíte `EnvironmentService`), caso a caso e na mesma ordem.
//!
//! Divergências: `process.argv` do Mocha é o argv do executável de teste do Rust. O `product` é o
//! [`ProductService`] padrão (os nomes do VS Code estável). O `NativeEnvironmentService` recebe o diretório de
//! instalação (`FileAccess.asFileUri('')` do original) por parâmetro.

mod common;

use common::{j, assert_deep_strict_equal};
use je_platform::environment::argv::{OPTIONS, IgnoringReporter, parse_args};
use je_platform::environment::environment::ExtensionHostDebugParams;
use je_platform::environment::environment_service::parse_extension_host_debug_port;
use je_platform::environment::native::NativeEnvironmentService;
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::environment::environment::NativeEnvironmentServiceApi;
use je_platform::product::ProductService;

fn parse_args_of(args: &[&str]) -> NativeParsedArgs {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    NativeParsedArgs::from_dynamic(&parse_args(&args, &OPTIONS, &IgnoringReporter))
}

fn params(port: Option<f64>, break_: bool, debug_id: Option<&str>, env: Option<&str>) -> ExtensionHostDebugParams {
    ExtensionHostDebugParams { port, break_, debug_id: debug_id.map(str::to_owned), env: env.map(j) }
}

fn equal_params(actual: &ExtensionHostDebugParams, expected: &ExtensionHostDebugParams) {
    assert_eq!((actual.port, actual.break_, &actual.debug_id), (expected.port, expected.break_, &expected.debug_id));
    match (&actual.env, &expected.env) {
        (Some(a), Some(b)) => assert_deep_strict_equal(a, b),
        (a, b) => assert!(a.is_none() && b.is_none(), "env diferente: {a:?} e {b:?}"),
    }
}

#[test]
fn parse_extension_host_port_when_built() {
    let parse = |a: &[&str]| parse_extension_host_debug_port(&parse_args_of(a), true);

    equal_params(&parse(&[]), &params(None, false, None, None));
    equal_params(&parse(&["--debugPluginHost"]), &params(None, false, None, None));
    equal_params(&parse(&["--debugPluginHost=1234"]), &params(Some(1234.0), false, None, None));
    equal_params(&parse(&["--debugBrkPluginHost"]), &params(None, false, None, None));
    equal_params(&parse(&["--debugBrkPluginHost=5678"]), &params(Some(5678.0), true, None, None));
    equal_params(&parse(&["--debugPluginHost=1234", "--debugBrkPluginHost=5678", "--debugId=7"]), &params(Some(5678.0), true, Some("7"), None));

    equal_params(&parse(&["--inspect-extensions"]), &params(None, false, None, None));
    equal_params(&parse(&["--inspect-extensions=1234"]), &params(Some(1234.0), false, None, None));
    equal_params(&parse(&["--inspect-brk-extensions"]), &params(None, false, None, None));
    equal_params(&parse(&["--inspect-brk-extensions=5678"]), &params(Some(5678.0), true, None, None));
    equal_params(&parse(&["--inspect-extensions=1234", "--inspect-brk-extensions=5678", "--debugId=7"]), &params(Some(5678.0), true, Some("7"), None));
    equal_params(
        &parse(&["--inspect-extensions=1234", "--inspect-brk-extensions=5678", "--extensionEnvironment={\"COOL\":\"1\"}"]),
        &params(Some(5678.0), true, None, Some("{'COOL': '1'}")),
    );
}

#[test]
fn parse_extension_host_port_when_unbuilt() {
    let parse = |a: &[&str]| parse_extension_host_debug_port(&parse_args_of(a), false);

    equal_params(&parse(&[]), &params(Some(5870.0), false, None, None));
    equal_params(&parse(&["--debugPluginHost"]), &params(Some(5870.0), false, None, None));
    equal_params(&parse(&["--debugPluginHost=1234"]), &params(Some(1234.0), false, None, None));
    equal_params(&parse(&["--debugBrkPluginHost"]), &params(Some(5870.0), false, None, None));
    equal_params(&parse(&["--debugBrkPluginHost=5678"]), &params(Some(5678.0), true, None, None));
    equal_params(&parse(&["--debugPluginHost=1234", "--debugBrkPluginHost=5678", "--debugId=7"]), &params(Some(5678.0), true, Some("7"), None));

    equal_params(&parse(&["--inspect-extensions"]), &params(Some(5870.0), false, None, None));
    equal_params(&parse(&["--inspect-extensions=1234"]), &params(Some(1234.0), false, None, None));
    equal_params(&parse(&["--inspect-brk-extensions"]), &params(Some(5870.0), false, None, None));
    equal_params(&parse(&["--inspect-brk-extensions=5678"]), &params(Some(5678.0), true, None, None));
    equal_params(&parse(&["--inspect-extensions=1234", "--inspect-brk-extensions=5678", "--debugId=7"]), &params(Some(5678.0), true, Some("7"), None));
}

// https://github.com/microsoft/vscode/issues/78440
#[test]
fn careful_with_boolean_file_names() {
    let actual = parse_args_of(&["-r", "arg.txt"]);
    assert!(actual.get_bool("reuse-window"));
    assert_eq!(actual.positional(), vec!["arg.txt".to_owned()]);

    let actual = parse_args_of(&["-r", "true.txt"]);
    assert!(actual.get_bool("reuse-window"));
    assert_eq!(actual.positional(), vec!["true.txt".to_owned()]);
}

#[test]
fn user_data_dir() {
    let process_argv: Vec<String> = std::env::args().collect();
    let process_args = || NativeParsedArgs::from_dynamic(&parse_args(&process_argv, &OPTIONS, &IgnoringReporter));
    let service1 = NativeEnvironmentService::native(process_args(), ProductService::default(), "/app").unwrap();
    assert!(!service1.user_data_path().is_empty());

    let args = process_args();
    args.set("user-data-dir", je_base::dynamic::Dynamic::string("/userDataDir/folder"));

    let service2 = NativeEnvironmentService::native(args, ProductService::default(), "/app").unwrap();
    assert_ne!(service1.user_data_path(), service2.user_data_path());
}
