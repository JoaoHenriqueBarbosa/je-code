//! Porte de `src/vs/platform/environment/node/argvHelper.ts`: a análise validada do argv do processo
//! principal e do CLI, e `addArg`.
//!
//! Decisões de tradução: o `assert(...)` do modo `--goto` é um [`Error`] devolvido (`Result`); os avisos
//! (`console.warn`) vão para o stderr; `process.argv`, `process.execPath` e `process.env` entram por parâmetro
//! (e por [`Environment`]) em vez de lidos do processo.

use je_base::errors::Error;
use je_base::nls::localize;
use je_base::path::{Environment, PathModule, Platform};

use super::argv::{ErrorReporter, NATIVE_CLI_COMMANDS, OPTIONS, parse_args};
use super::native_parsed_args::NativeParsedArgs;

fn warn(message: &str) {
    eprintln!("{message}");
}

struct WarningReporter {
    /// `Some(comando)` dentro de um subcomando.
    command: Option<String>,
}

impl WarningReporter {
    fn is_native_command(&self) -> bool {
        self.command.as_deref().is_some_and(|c| NATIVE_CLI_COMMANDS.contains(&c))
    }
}

impl ErrorReporter for WarningReporter {
    fn on_unknown_option(&self, id: &str) {
        match &self.command {
            None => warn(&localize("unknownOption", "Warning: '{0}' is not in the list of known options, but still passed to Electron/Chromium.", &[id])),
            Some(command) => {
                if !self.is_native_command() {
                    warn(&localize("unknownSubCommandOption", "Warning: '{0}' is not in the list of known options for subcommand '{1}'", &[id, command]));
                }
            }
        }
    }

    fn on_multiple_values(&self, id: &str, val: &str) {
        warn(&localize("multipleValues", "Option '{0}' is defined more than once. Using value '{1}'.", &[id, val]));
    }

    fn on_empty_value(&self, id: &str) {
        warn(&localize("emptyValue", "Option '{0}' requires a non empty value. Ignoring the option.", &[id]));
    }

    fn on_deprecated_option(&self, deprecated_option: &str, message: &str) {
        warn(&localize("deprecatedArgument", "Option '{0}' is deprecated: {1}", &[deprecated_option, message]));
    }

    fn get_subcommand_reporter(&self, command: &str) -> Option<Box<dyn ErrorReporter>> {
        // `getSubcommandReporter: NATIVE_CLI_COMMANDS.includes(command) ? getSubcommandReporter : undefined`
        // vale para os reporters de subcomando; o do nível raiz sempre o tem.
        if self.command.is_some() && !NATIVE_CLI_COMMANDS.contains(&self.command.as_deref().unwrap_or("")) {
            return None;
        }
        Some(Box::new(WarningReporter { command: Some(command.to_owned()) }))
    }
}

/// `/^(\w:)?[^:]+(:\d*){0,2}:?$/.test(arg)`.
fn is_goto_argument(arg: &str) -> bool {
    let chars: Vec<char> = arg.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let prefixes: &[usize] = if chars.len() >= 2 && is_word(chars[0]) && chars[1] == ':' { &[2, 0] } else { &[0] };
    prefixes.iter().any(|&prefix| {
        let rest = &chars[prefix..];
        let non_colon = rest.iter().take_while(|c| **c != ':').count();
        if non_colon == 0 {
            return false;
        }
        let mut tail = &rest[non_colon..];
        for _ in 0..2 {
            if tail.first() == Some(&':') {
                tail = &tail[1..];
                let digits = tail.iter().take_while(|c| c.is_ascii_digit()).count();
                tail = &tail[digits..];
            }
        }
        if tail.first() == Some(&':') {
            tail = &tail[1..];
        }
        tail.is_empty()
    })
}

/// `parseAndValidate(cmdLineArgs, reportWarnings)`.
pub fn parse_and_validate(cmd_line_args: &[String], report_warnings: bool) -> Result<NativeParsedArgs, Error> {
    let reporter = WarningReporter { command: None };
    let args = NativeParsedArgs::from_dynamic(&parse_args(
        cmd_line_args,
        &OPTIONS,
        if report_warnings { &reporter } else { &super::argv::IgnoringReporter },
    ));
    if args.get_bool("goto") {
        for arg in args.positional() {
            if !is_goto_argument(&arg) {
                return Err(Error::named("AssertionError", localize("gotoValidation", "Arguments in `--goto` mode should be in the format of `FILE(:LINE(:CHARACTER))`.", &[])));
            }
        }
    }

    Ok(args)
}

fn strip_app_path(argv: &[String]) -> Option<Vec<String>> {
    let index = argv.iter().position(|a| !a.starts_with('-'))?;
    let mut result = argv[..index].to_vec();
    result.extend_from_slice(&argv[index + 1..]);
    Some(result)
}

/// `parseMainProcessArgv(processArgv)`: o argv cru do processo, como `Electron . --verbose --wait`.
/// `exec_path` é `process.execPath`.
pub fn parse_main_process_argv(process_argv: &[String], env: &dyn Environment, exec_path: &str) -> Result<NativeParsedArgs, Error> {
    let mut args: Vec<String> = process_argv.iter().skip(1).cloned().collect();

    // When code.exe is configured to 'Run as administrator' on Windows, the CLI launcher (code.cmd) sets
    // ELECTRON_RUN_AS_NODE=1 and passes cli.js as the first argument. The elevated process does not inherit
    // the environment variable so Electron starts as a GUI app with cli.js as a stray positional argument.
    // Detect and strip it. The path may include a version subdirectory (e.g.,
    // 2ca3b2734b\resources\app\out\cli.js).
    if env.platform() == Platform::Win32 && !args.is_empty() {
        let path = PathModule::native(env);
        let resolved_arg = path.resolve(&[&args[0]]).to_lowercase();
        let install_dir = format!("{}\\", path.dirname(exec_path).to_lowercase());
        if resolved_arg.starts_with(&install_dir) && resolved_arg.ends_with("\\resources\\app\\out\\cli.js") {
            args.remove(0);
        }
    }

    // If dev, remove the first non-option argument: it's the app location
    if env.env_var("VSCODE_DEV").is_some_and(|v| !v.is_empty()) {
        args = strip_app_path(&args).unwrap_or_default();
    }

    // If called from CLI, don't report warnings as they are already reported.
    let report_warnings = !is_launched_from_cli(env);
    parse_and_validate(&args, report_warnings)
}

/// `parseCLIProcessArgv(processArgv)`: o argv cru do CLI, como `Electron cli.js . --verbose --wait`.
pub fn parse_cli_process_argv(process_argv: &[String], env: &dyn Environment) -> Result<NativeParsedArgs, Error> {
    // remove the first non-option argument: it's always the app location
    let mut args: Vec<String> = process_argv.iter().skip(2).cloned().collect();

    // If dev, remove the first non-option argument: it's the app location
    if env.env_var("VSCODE_DEV").is_some_and(|v| !v.is_empty()) {
        args = strip_app_path(&args).unwrap_or_default();
    }

    parse_and_validate(&args, true)
}

/// `addArg(argv, ...args)`: acrescenta antes do marcador `--`, se houver.
pub fn add_arg(argv: Vec<String>, args: &[&str]) -> Vec<String> {
    let mut argv = argv;
    let new_args = args.iter().map(|arg| (*arg).to_owned());
    match argv.iter().position(|a| a == "--") {
        // if the we have an argument "--" (end of argument marker) we cannot add arguments at the end.
        // rather, we add arguments before the "--" marker.
        Some(end_of_args_marker_index) => {
            argv.splice(end_of_args_marker_index..end_of_args_marker_index, new_args);
        }
        None => argv.extend(new_args),
    }

    argv
}

/// `isLaunchedFromCli(env)`.
pub fn is_launched_from_cli(env: &dyn Environment) -> bool {
    env.env_var("VSCODE_CLI").as_deref() == Some("1")
}
