//! Porte de `src/vs/platform/environment/node/argv.ts`: a tabela de opções do `code` ([`options`]),
//! o `parseArgs`, o formatador de ajuda e as mensagens de versão e de leitura do stdin.
//!
//! Decisões de tradução:
//! - **`OptionDescriptions<T>`** (um tipo mapeado sobre `NativeParsedArgs`) é [`OptionDescriptions`], uma lista
//!   ordenada de `(id, descrição)`: a ordem das chaves do original é a ordem de iteração do `parseArgs`, que
//!   decide a ordem das propriedades do resultado e a ordem do texto de ajuda.
//! - **`parseArgs<T>`** devolve o objeto dinâmico (só com as chaves que apareceram); [`NativeParsedArgs`]
//!   embrulha esse objeto.
//! - **Textos da ajuda** passam por [`localize`], que devolve a mensagem padrão em inglês.

use std::sync::LazyLock;

use je_base::JsString;
use je_base::dynamic::{ArrayRef, Dynamic, ObjectRef, Truthy};
use je_base::nls::localize;

use super::minimist::{MinimistOptions, minimist};

/// `helpCategories`: `o`, `e`, `t` e `m`, na ordem do original.
const HELP_CATEGORIES: [(char, &str, &str); 4] = [
    ('o', "optionsUpperCase", "Options"),
    ('e', "extensionsManagement", "Extensions Management"),
    ('t', "troubleshooting", "Troubleshooting"),
    ('m', "mcp", "Model Context Protocol"),
];

fn help_category(cat: char) -> String {
    HELP_CATEGORIES.iter().find(|(c, ..)| *c == cat).map(|(_, key, message)| localize(key, message, &[])).unwrap_or_default()
}

/// `type: 'boolean' | 'string' | 'string[]'` de uma `Option`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionType {
    Boolean,
    String,
    StringArray,
}

/// `Option<OptionType>`.
#[derive(Clone, Debug)]
pub struct OptionDef {
    pub type_: OptionType,
    pub alias: Option<String>,
    /// old deprecated ids
    pub deprecates: Vec<String>,
    pub args: Vec<String>,
    pub description: Option<String>,
    pub deprecation_message: Option<String>,
    pub allow_empty_value: bool,
    pub cat: Option<char>,
    pub global: bool,
}

impl OptionDef {
    pub fn new(type_: OptionType) -> Self {
        Self {
            type_,
            alias: None,
            deprecates: Vec::new(),
            args: Vec::new(),
            description: None,
            deprecation_message: None,
            allow_empty_value: false,
            cat: None,
            global: false,
        }
    }

    pub fn alias(mut self, alias: &str) -> Self {
        self.alias = Some(alias.to_owned());
        self
    }

    pub fn deprecates(mut self, ids: &[&str]) -> Self {
        self.deprecates = ids.iter().map(|id| (*id).to_owned()).collect();
        self
    }

    pub fn args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = Some(description.to_owned());
        self
    }

    pub fn allow_empty_value(mut self) -> Self {
        self.allow_empty_value = true;
        self
    }

    pub fn cat(mut self, cat: char) -> Self {
        self.cat = Some(cat);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

/// `Subcommand<T>`.
#[derive(Clone, Debug)]
pub struct Subcommand {
    pub description: Option<String>,
    pub deprecation_message: Option<String>,
    pub options: OptionDescriptions,
}

/// Uma entrada de `OptionDescriptions`: uma opção ou um subcomando.
#[derive(Clone, Debug)]
pub enum OptionDescription {
    Option(OptionDef),
    Subcommand(Subcommand),
}

impl From<OptionDef> for OptionDescription {
    fn from(option: OptionDef) -> Self {
        Self::Option(option)
    }
}

/// `OptionDescriptions<T>`: as opções na ordem de declaração.
pub type OptionDescriptions = Vec<(String, OptionDescription)>;

/// `{ type: 'boolean' }` etc., o `o(...)` curto dos testes.
pub fn option(type_: OptionType) -> OptionDef {
    OptionDef::new(type_)
}

fn boolean() -> OptionDef {
    option(OptionType::Boolean)
}

fn string() -> OptionDef {
    option(OptionType::String)
}

fn string_array() -> OptionDef {
    option(OptionType::StringArray)
}

fn subcommand(description: Option<&str>, options: OptionDescriptions) -> OptionDescription {
    OptionDescription::Subcommand(Subcommand { description: description.map(str::to_owned), deprecation_message: None, options })
}

/// `NATIVE_CLI_COMMANDS`.
pub const NATIVE_CLI_COMMANDS: [&str; 3] = ["tunnel", "serve-web", "agent"];

fn cli_options() -> OptionDescriptions {
    let cli_data_dir = || string().args(&["dir"]).description(&localize("cliDataDir", "Directory where CLI metadata should be stored.", &[]));
    vec![
        ("cli-data-dir".into(), cli_data_dir().into()),
        ("disable-telemetry".into(), boolean().into()),
        ("telemetry-level".into(), string().into()),
    ]
}

fn profile_option() -> OptionDef {
    string().cat('o').args(&["profileName"]).description(&localize(
        "profileName",
        "Opens the provided folder or workspace with the given profile and associates the profile with the workspace. If the profile does not exist, a new empty one is created.",
        &[],
    ))
}

fn help_option(cat: Option<char>) -> OptionDef {
    let option = boolean().alias("h").description(&localize("help", "Print usage.", &[]));
    match cat {
        Some(cat) => option.cat(cat),
        None => option,
    }
}

/// `OPTIONS`.
pub fn options() -> OptionDescriptions {
    let l = |key: &str, message: &str| localize(key, message, &[]);
    let mut o: OptionDescriptions = Vec::new();

    o.push((
        "chat".to_owned(),
        subcommand(
        Some("Pass in a prompt to run in a chat session in the current working directory."),
        vec![
            ("_".into(), string_array().description(&l("prompt", "The prompt to use as chat.")).into()),
            (
                "mode".into(),
                string()
                    .cat('o')
                    .alias("m")
                    .args(&["mode"])
                    .description(&l(
                        "chatMode",
                        "The mode to use for the chat session. Available options: 'ask', 'edit', 'agent', or the identifier of a custom mode. Defaults to 'agent'.",
                    ))
                    .into(),
            ),
            (
                "add-file".into(),
                string_array().cat('o').alias("a").args(&["path"]).description(&l("addFile", "Add files as context to the chat session.")).into(),
            ),
            ("maximize".into(), boolean().cat('o').description(&l("chatMaximize", "Maximize the chat session view.")).into()),
            (
                "reuse-window".into(),
                boolean()
                    .cat('o')
                    .alias("r")
                    .description(&l("reuseWindowForChat", "Force to use the last active window for the chat session."))
                    .into(),
            ),
            (
                "new-window".into(),
                boolean().cat('o').alias("n").description(&l("newWindowForChat", "Force to open an empty window for the chat session.")).into(),
            ),
            ("profile".into(), profile_option().into()),
            ("help".into(), help_option(None).into()),
        ],
        ),
    ));
    o.push(("serve-web".to_owned(), subcommand(Some("Run a server that displays the editor UI in browsers."), cli_options())));
    o.push(("agent".to_owned(), subcommand(Some("Start and interact with AI agent hosts."), cli_options())));
    let mut tunnel_options = cli_options();
    tunnel_options.push((
        "user".into(),
        subcommand(
            None,
            vec![(
                "login".into(),
                subcommand(None, vec![("provider".into(), string().into()), ("access-token".into(), string().into())]),
            )],
        ),
    ));
    o.push((
        "tunnel".to_owned(),
        subcommand(Some("Make the current machine accessible from vscode.dev or other machines through a secure tunnel."), tunnel_options),
    ));

    let mut add = |id: &str, def: OptionDef| o.push((id.to_owned(), def.into()));

    add("diff", boolean().cat('o').alias("d").args(&["file", "file"]).description(&l("diff", "Compare two files with each other.")));
    add(
        "merge",
        boolean().cat('o').alias("m").args(&["path1", "path2", "base", "result"]).description(&l(
            "merge",
            "Perform a three-way merge by providing paths for two modified versions of a file, the common origin of both modified versions and the output file to save merge results.",
        )),
    );
    add("add", boolean().cat('o').alias("a").args(&["folder"]).description(&l("add", "Add folder(s) to the last active window.")));
    add("remove", boolean().cat('o').args(&["folder"]).description(&l("remove", "Remove folder(s) from the last active window.")));
    add(
        "goto",
        boolean().cat('o').alias("g").args(&["file:line[:character]"]).description(&l("goto", "Open a file at the path on the specified line and character position.")),
    );
    add("new-window", boolean().cat('o').alias("n").description(&l("newWindow", "Force to open a new window.")));
    add("reuse-window", boolean().cat('o').alias("r").description(&l("reuseWindow", "Force to open a file or folder in an already opened window.")));
    add(
        "agents",
        boolean().cat('o').deprecates(&["sessions"]).description(&l(
            "agents",
            "Opens the agents window. If a folder is provided, opens a new session with that folder selected.",
        )),
    );
    add("session-title-base64", string());
    add("wait", boolean().cat('o').alias("w").description(&l("wait", "Wait for the files to be closed before returning.")));
    add("waitMarkerFilePath", string());
    add("locale", string().cat('o').args(&["locale"]).description(&l("locale", "The locale to use (e.g. en-US or zh-TW).")));
    add(
        "user-data-dir",
        string().cat('o').args(&["dir"]).description(&l(
            "userDataDir",
            "Specifies the directory that user data is kept in. Can be used to open multiple distinct instances of Code.",
        )),
    );
    add("profile", profile_option());
    add("help", help_option(Some('o')));

    add("extensions-dir", string().deprecates(&["extensionHomePath"]).cat('e').args(&["dir"]).description(&l("extensionHomePath", "Set the root path for extensions.")));
    add("extensions-download-dir", string());
    add("builtin-extensions-dir", string());
    add("shared-data-dir", string());
    add("list-extensions", boolean().cat('e').description(&l("listExtensions", "List the installed extensions.")));
    add("agent-plugins-dir", string());
    add("agents-user-data-dir", string());
    add("agents-extensions-dir", string());
    add("show-versions", boolean().cat('e').description(&l("showVersions", "Show versions of installed extensions, when using --list-extensions.")));
    add(
        "category",
        string().allow_empty_value().cat('e').description(&l("category", "Filters installed extensions by provided category, when using --list-extensions.")).args(&["category"]),
    );
    add(
        "install-extension",
        string_array().cat('e').args(&["ext-id | path"]).description(&l(
            "installExtension",
            "Installs or updates an extension. The argument is either an extension id or a path to a VSIX. The identifier of an extension is '${publisher}.${name}'. Use '--force' argument to update to latest version. To install a specific version provide '@${version}'. For example: 'vscode.csharp@1.2.3'.",
        )),
    );
    add("pre-release", boolean().cat('e').description(&l("install prerelease", "Installs the pre-release version of the extension, when using --install-extension")));
    add("uninstall-extension", string_array().cat('e').args(&["ext-id"]).description(&l("uninstallExtension", "Uninstalls an extension.")));
    add("update-extensions", boolean().cat('e').description(&l("updateExtensions", "Update the installed extensions.")));
    add(
        "enable-proposed-api",
        string_array().allow_empty_value().cat('e').args(&["ext-id"]).description(&l(
            "experimentalApis",
            "Enables proposed API features for extensions. Can receive one or more extension IDs to enable individually.",
        )),
    );

    add(
        "add-mcp",
        string_array().cat('m').args(&["json"]).description(&l(
            "addMcp",
            "Adds a Model Context Protocol server definition to the user profile. Accepts JSON input in the form '{\"name\":\"server-name\",\"command\":...}'",
        )),
    );

    add("version", boolean().cat('t').alias("v").description(&l("version", "Print version.")));
    add("verbose", boolean().cat('t').global().description(&l("verbose", "Print verbose output (implies --wait).")));
    add(
        "log",
        string_array().cat('t').args(&["level"]).global().description(&l(
            "log",
            "Log level to use. Default is 'info'. Allowed values are 'critical', 'error', 'warn', 'info', 'debug', 'trace', 'off'. You can also configure the log level of an extension by passing extension id and log level in the following format: '${publisher}.${name}:${logLevel}'. For example: 'vscode.csharp:trace'. Can receive one or more such entries.",
        )),
    );
    add("status", boolean().cat('t').alias("s").description(&l("status", "Print process usage and diagnostics information.")));
    add("prof-startup", boolean().cat('t').description(&l("prof-startup", "Run CPU profiler during startup.")));
    add("prof-append-timers", string());
    add("prof-duration-markers", string_array());
    add("prof-duration-markers-file", string());
    add("no-cached-data", boolean());
    add("prof-startup-prefix", string());
    add("prof-v8-extensions", boolean());
    add(
        "disable-extensions",
        boolean().deprecates(&["disableExtensions"]).cat('t').description(&l(
            "disableExtensions",
            "Disable all installed extensions. This option is not persisted and is effective only when the command opens a new window.",
        )),
    );
    add(
        "disable-extension",
        string_array().cat('t').args(&["ext-id"]).description(&l(
            "disableExtension",
            "Disable the provided extension. This option is not persisted and is effective only when the command opens a new window.",
        )),
    );
    add("sync", string().cat('t').description(&l("turn sync", "Turn sync on or off.")).args(&["on | off"]));

    add(
        "inspect-extensions",
        string().allow_empty_value().deprecates(&["debugPluginHost"]).args(&["port"]).cat('t').description(&l(
            "inspect-extensions",
            "Allow debugging and profiling of extensions. Check the developer tools for the connection URI.",
        )),
    );
    add(
        "inspect-brk-extensions",
        string().allow_empty_value().deprecates(&["debugBrkPluginHost"]).args(&["port"]).cat('t').description(&l(
            "inspect-brk-extensions",
            "Allow debugging and profiling of extensions with the extension host being paused after start. Check the developer tools for the connection URI.",
        )),
    );
    add("disable-lcd-text", boolean().cat('t').description(&l("disableLCDText", "Disable LCD font rendering.")));
    add("disable-gpu", boolean().cat('t').description(&l("disableGPU", "Disable GPU hardware acceleration.")));
    add(
        "disable-chromium-sandbox",
        boolean().cat('t').description(&l(
            "disableChromiumSandbox",
            "Use this option only when there is requirement to launch the application as sudo user on Linux or when running as an elevated user in an applocker environment on Windows.",
        )),
    );
    add("sandbox", boolean());
    add(
        "locate-shell-integration-path",
        string().cat('t').args(&["shell"]).description(&l(
            "locateShellIntegrationPath",
            "Print the path to a terminal shell integration script. Allowed values are 'bash', 'pwsh', 'zsh' or 'fish'.",
        )),
    );
    add("telemetry", boolean().cat('t').description(&l("telemetry", "Shows all telemetry events which VS code collects.")));

    add("remote", string().allow_empty_value());
    add("folder-uri", string_array().cat('o').args(&["uri"]));
    add("file-uri", string_array().cat('o').args(&["uri"]));

    add("locate-extension", string_array());
    add("extensionDevelopmentPath", string_array());
    add("extensionDevelopmentKind", string_array());
    add("extensionTestsPath", string());
    add("extensionEnvironment", string());
    add("debugId", string());
    add("debugRenderer", boolean());
    add("inspect-ptyhost", string().allow_empty_value());
    add("inspect-brk-ptyhost", string().allow_empty_value());
    add("inspect-agenthost", string().allow_empty_value());
    add("inspect-brk-agenthost", string().allow_empty_value());
    add("inspect-sharedprocess", string().allow_empty_value());
    add("inspect-brk-sharedprocess", string().allow_empty_value());
    add("export-default-configuration", string());
    add("export-policy-data", string().allow_empty_value());
    add("export-default-keybindings", string().allow_empty_value());
    add("install-source", string());
    add("enable-smoke-test-driver", boolean());
    add("skip-sessions-welcome", boolean());
    add("logExtensionHostCommunication", boolean());
    add("skip-release-notes", boolean());
    add("skip-welcome", boolean());
    add("disable-telemetry", boolean());
    add("telemetry-level", string());
    add("disable-updates", boolean());
    add("share-secrets-with-agents-app", boolean());
    add(
        "transient",
        boolean().cat('t').description(&l("transient", "Run with temporary data and extension directories, as if launched for the first time.")),
    );
    add("use-inmemory-secretstorage", boolean().deprecates(&["disable-keytar"]));
    add("password-store", string());
    add("disable-workspace-trust", boolean());
    add("disable-crash-reporter", boolean());
    add("crash-reporter-directory", string());
    add("crash-reporter-id", string());
    add("skip-add-to-recently-opened", boolean());
    add("open-url", boolean());
    add("file-write", boolean());
    add("file-chmod", boolean());
    add("install-builtin-extension", string_array());
    add("force", boolean());
    add("do-not-sync", boolean());
    add("do-not-include-pack-dependencies", boolean());
    add("trace", boolean());
    add("trace-memory-infra", boolean());
    add("trace-category-filter", string());
    add("trace-options", string());
    add("preserve-env", boolean());
    add("force-user-env", boolean());
    add("force-disable-user-env", boolean());
    add("open-devtools", boolean());
    add("disable-gpu-sandbox", boolean());
    add("logsPath", string());
    add("__enable-file-policy", boolean());
    add("editSessionId", string());
    add("continueOn", string());
    add("enable-coi", boolean());
    add("unresponsive-sample-interval", string());
    add("unresponsive-sample-period", string());
    add("enable-rdp-display-tracking", boolean());
    add("disable-layout-restore", boolean());
    add("disable-experiments", boolean());

    // chromium flags
    add("no-proxy-server", boolean());
    // Minimist incorrectly parses keys that start with `--no`
    // https://github.com/substack/minimist/blob/aeb3e27dae0412de5c0494e9563a5f10c82cc7a9/index.js#L118-L121
    // If --no-sandbox is passed via cli wrapper it will be treated as --sandbox which is incorrect, we use
    // the alias here to make sure --no-sandbox is always respected.
    // For https://github.com/microsoft/vscode/issues/128279
    add("no-sandbox", boolean().alias("sandbox"));
    add("proxy-server", string());
    add("proxy-bypass-list", string());
    add("proxy-pac-url", string());
    add("js-flags", string()); // chrome js flags
    add("inspect", string().allow_empty_value());
    add("inspect-brk", string().allow_empty_value());
    add("nolazy", boolean()); // node inspect
    add("force-device-scale-factor", string());
    add("force-renderer-accessibility", boolean());
    add("ignore-certificate-errors", boolean());
    add("allow-insecure-localhost", boolean());
    add("log-net-log", string());
    add("vmodule", string());
    add("_urls", string_array());
    add("disable-dev-shm-usage", boolean());
    add("profile-temp", boolean());
    add("ozone-platform", string());
    add("enable-tracing", string());
    add("trace-startup-format", string());
    add("trace-startup-file", string());
    add("trace-startup-duration", string());
    add("xdg-portal-required-version", string());

    add("_", string_array()); // main arguments
    o
}

/// `OPTIONS`, montada uma vez.
pub static OPTIONS: LazyLock<OptionDescriptions> = LazyLock::new(options);

/// `ErrorReporter`.
pub trait ErrorReporter {
    fn on_unknown_option(&self, id: &str);
    fn on_multiple_values(&self, id: &str, used_value: &str);
    fn on_empty_value(&self, id: &str);
    fn on_deprecated_option(&self, deprecated_id: &str, message: &str);

    fn get_subcommand_reporter(&self, _command: &str) -> Option<Box<dyn ErrorReporter>> {
        None
    }
}

/// `ignoringReporter`.
pub struct IgnoringReporter;

impl ErrorReporter for IgnoringReporter {
    fn on_unknown_option(&self, _id: &str) {}
    fn on_multiple_values(&self, _id: &str, _used_value: &str) {}
    fn on_empty_value(&self, _id: &str) {}
    fn on_deprecated_option(&self, _deprecated_id: &str, _message: &str) {}
}

fn key(name: &str) -> JsString {
    JsString::from(name)
}

/// `parseArgs(args, options, errorReporter)`: o objeto de argumentos (só as chaves presentes).
pub fn parse_args(args: &[String], options: &OptionDescriptions, error_reporter: &dyn ErrorReporter) -> Dynamic {
    // Find the first non-option arg, which also isn't the value for a previous `--flag`
    let first_possible_command = args.iter().find(|a| {
        !a.is_empty() && !a.starts_with('-') && options.iter().any(|(id, description)| id == *a && matches!(description, OptionDescription::Subcommand(_)))
    });

    let mut alias: Vec<(String, String)> = Vec::new();
    let mut string_options: Vec<String> = vec!["_".to_owned()];
    let mut boolean_options: Vec<String> = Vec::new();
    let mut global_options: OptionDescriptions = Vec::new();
    let mut command: Option<&Subcommand> = None;
    for (option_id, description) in options {
        match description {
            OptionDescription::Subcommand(subcommand) => {
                if Some(option_id) == first_possible_command {
                    command = Some(subcommand);
                }
            }
            OptionDescription::Option(o) => {
                if let Some(option_alias) = &o.alias {
                    alias.push((option_id.clone(), option_alias.clone()));
                }

                match o.type_ {
                    OptionType::String | OptionType::StringArray => {
                        string_options.push(option_id.clone());
                        string_options.extend(o.deprecates.iter().cloned());
                    }
                    OptionType::Boolean => {
                        boolean_options.push(option_id.clone());
                        boolean_options.extend(o.deprecates.iter().cloned());
                    }
                }
                if o.global {
                    global_options.push((option_id.clone(), description.clone()));
                }
            }
        }
    }
    if let (Some(command), Some(first_possible_command)) = (command, first_possible_command) {
        let mut sub_options = global_options;
        for (option_id, description) in &command.options {
            match sub_options.iter_mut().find(|(id, _)| id == option_id) {
                Some(existing) => existing.1 = description.clone(),
                None => sub_options.push((option_id.clone(), description.clone())),
            }
        }
        let new_args: Vec<String> = args.iter().filter(|a| *a != first_possible_command).cloned().collect();
        let reporter = error_reporter.get_subcommand_reporter(first_possible_command);
        let reporter_ref: &dyn ErrorReporter = reporter.as_deref().unwrap_or(&IgnoringReporter);
        let subcommand_options = parse_args(&new_args, &sub_options, reporter_ref);
        return Dynamic::object([(first_possible_command.as_str(), subcommand_options), ("_", Dynamic::array([]))]);
    }

    // Pre-process args: extract '--no-KEY' where 'no-KEY' is a declared boolean option.
    // Minimist incorrectly handles these by stripping the 'no-' prefix and setting KEY=false.
    // For example, '--no-proxy-server' should set 'no-proxy-server=true', not 'proxy-server=false'.
    // See https://github.com/substack/minimist/blob/aeb3e27dae0412de5c0494e9563a5f10c82cc7a9/index.js#L118-L121
    let mut extracted_no_booleans: Vec<String> = Vec::new();
    let filtered_args: Vec<String> = args
        .iter()
        .filter(|a| {
            if let Some(no_key) = a.strip_prefix("--").filter(|rest| rest.starts_with("no-")) {
                // Extract 'no-KEY' from '--no-KEY'
                if boolean_options.iter().any(|b| b == no_key) {
                    if !extracted_no_booleans.iter().any(|e| e == no_key) {
                        extracted_no_booleans.push(no_key.to_owned());
                    }
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    // remove aliases to avoid confusion
    let parsed_args = minimist(&filtered_args, &MinimistOptions { string: string_options, boolean: boolean_options, alias });

    // Restore the '--no-KEY' booleans that were extracted to prevent minimist from mangling them
    for no_key in &extracted_no_booleans {
        parsed_args.set(&key(no_key), Dynamic::Bool(true));
    }

    let cleaned_args = ObjectRef::new();
    let remaining_args = parsed_args;

    // https://github.com/microsoft/vscode/issues/58177, https://github.com/microsoft/vscode/issues/106617
    let positional: Vec<Dynamic> = match remaining_args.get(&key("_")) {
        Some(Dynamic::Array(items)) => items.to_vec().iter().map(stringify_positional).filter(|arg| !arg.is_empty()).map(|arg| Dynamic::string(&arg)).collect(),
        _ => Vec::new(),
    };
    cleaned_args.set(&key("_"), Dynamic::Array(ArrayRef::new(positional)));

    remaining_args.delete(&key("_"));

    for (option_id, description) in options {
        let OptionDescription::Option(o) = description else {
            continue;
        };
        if let Some(option_alias) = &o.alias {
            remaining_args.delete(&key(option_alias));
        }

        let mut val = remaining_args.get(&key(option_id)).unwrap_or(Dynamic::Undefined);
        for deprecated_id in &o.deprecates {
            if remaining_args.has_own(&key(deprecated_id)) {
                if !val.is_truthy() {
                    val = remaining_args.get(&key(deprecated_id)).unwrap_or(Dynamic::Undefined);
                    if val.is_truthy() {
                        let message = o.deprecation_message.clone().unwrap_or_else(|| localize("deprecated.useInstead", "Use {0} instead.", &[option_id]));
                        error_reporter.on_deprecated_option(deprecated_id, &message);
                    }
                }
                remaining_args.delete(&key(deprecated_id));
            }
        }

        if !val.is_undefined() {
            match o.type_ {
                OptionType::StringArray => {
                    let mut list: Vec<Dynamic> = match &val {
                        Dynamic::Array(items) => items.to_vec(),
                        other => vec![other.clone()],
                    };
                    if !o.allow_empty_value {
                        let sanitized: Vec<Dynamic> = list.iter().filter(|v| !is_empty_string(v)).cloned().collect();
                        if sanitized.len() != list.len() {
                            error_reporter.on_empty_value(option_id);
                            list = sanitized;
                            val = if list.is_empty() { Dynamic::Undefined } else { Dynamic::Array(ArrayRef::new(list)) };
                        } else {
                            val = Dynamic::Array(ArrayRef::new(list));
                        }
                    } else {
                        val = Dynamic::Array(ArrayRef::new(list));
                    }
                }
                OptionType::String => {
                    if let Dynamic::Array(items) = &val {
                        // take the last
                        let last = items.to_vec().pop().unwrap_or(Dynamic::Undefined);
                        error_reporter.on_multiple_values(option_id, &stringify_positional(&last));
                        val = last;
                    } else if !val.is_truthy() && !o.allow_empty_value {
                        error_reporter.on_empty_value(option_id);
                        val = Dynamic::Undefined;
                    }
                }
                OptionType::Boolean => {}
            }
            cleaned_args.set(&key(option_id), val);

            if let Some(message) = &o.deprecation_message {
                error_reporter.on_deprecated_option(option_id, message);
            }
        }
        remaining_args.delete(&key(option_id));
    }

    for unknown in remaining_args.keys() {
        error_reporter.on_unknown_option(&unknown.to_string_lossy());
    }

    Dynamic::Object(cleaned_args)
}

fn is_empty_string(value: &Dynamic) -> bool {
    matches!(value, Dynamic::String(text) if text.is_empty())
}

/// `String(arg)` de um argumento posicional (texto ou número).
fn stringify_positional(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        Dynamic::Number(n) => je_base::dynamic::number_to_js_string(*n),
        Dynamic::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn format_usage(option_id: &str, option: &OptionDef) -> String {
    let args = if option.args.is_empty() { String::new() } else { format!(" <{}>", option.args.join("> <")) };
    match &option.alias {
        Some(alias) => format!("-{alias} --{option_id}{args}"),
        None => format!("--{option_id}{args}"),
    }
}

/// `formatOptions(options, columns)`: exportada só para testes no original.
pub fn format_options(options: &OptionDescriptions, columns: usize) -> Vec<String> {
    let mut usage_texts: Vec<(String, String)> = Vec::new();
    for (option_id, description) in options {
        match description {
            OptionDescription::Option(o) => usage_texts.push((format_usage(option_id, o), o.description.clone().unwrap_or_else(|| "undefined".to_owned()))),
            // `formatUsage(optionId, subcommand)`: o subcomando não tem `alias` nem `args`.
            OptionDescription::Subcommand(s) => usage_texts.push((format!("--{option_id}"), s.description.clone().unwrap_or_else(|| "undefined".to_owned()))),
        }
    }
    format_usage_texts(&usage_texts, columns)
}

fn format_usage_texts(usage_texts: &[(String, String)], columns: usize) -> Vec<String> {
    let max_length = usage_texts.iter().map(|(usage, _)| usage.encode_utf16().count()).fold(12, usize::max);
    let arg_length = max_length + 2 /*left padding*/ + 1 /*right padding*/;
    if columns < arg_length + 25 {
        // Use a condensed version on narrow terminals
        return usage_texts.iter().flat_map(|(usage, description)| [format!("  {usage}"), format!("      {description}")]).collect();
    }
    let description_columns = columns - arg_length - 1;
    let mut result = Vec::new();
    for (usage, description) in usage_texts {
        let wrapped_description = wrap_text(description, description_columns);
        let key_padding = indent(arg_length - usage.encode_utf16().count() - 2 /*left padding*/);
        result.push(format!("  {usage}{key_padding}{}", wrapped_description.first().cloned().unwrap_or_default()));
        for line in wrapped_description.iter().skip(1) {
            result.push(format!("{}{line}", indent(arg_length)));
        }
    }
    result
}

fn indent(count: usize) -> String {
    " ".repeat(count)
}

/// `text.trim()` do JavaScript (espaço em branco Unicode).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut text: Vec<u16> = text.encode_utf16().collect();
    while !text.is_empty() {
        let mut index = if text.len() < columns { text.len() } else { last_index_of_space(&text, columns) };
        if index == 0 {
            index = columns;
        }
        let line = String::from_utf16_lossy(&text[..index.min(text.len())]);
        let rest = String::from_utf16_lossy(&text[index.min(text.len())..]);
        lines.push(js_trim(&line).to_owned());
        text = js_trim_start(&rest).encode_utf16().collect();
    }
    lines
}

/// `text.lastIndexOf(' ', columns)`: o último espaço em `0..=columns`, ou `-1` (que o `slice(0, -1)` do JS
/// trataria como "tudo menos o último"). `-1` vira `len - 1`, como no JS.
fn last_index_of_space(text: &[u16], columns: usize) -> usize {
    let upper = columns.min(text.len() - 1);
    (0..=upper).rev().find(|&i| text[i] == u16::from(b' ')).unwrap_or(text.len() - 1)
}

/// `buildHelpMessage(...)`.
pub fn build_help_message(
    product_name: &str,
    executable_name: &str,
    version: &str,
    options: &OptionDescriptions,
    capabilities: HelpCapabilities,
    columns: usize,
) -> String {
    let input_files = if capabilities.no_input_files {
        String::new()
    } else if capabilities.is_chat {
        format!(" [{}]", localize("cliPrompt", "prompt", &[]))
    } else {
        format!(" [{}...]", localize("paths", "paths", &[]))
    };
    let subcommand = if capabilities.is_chat { " chat" } else { "" };

    let mut help = vec![format!("{product_name} {version}"), String::new()];
    help.push(format!(
        "{}: {executable_name}{subcommand} [{}]{input_files}",
        localize("usage", "Usage", &[]),
        localize("options", "options", &[])
    ));
    help.push(String::new());
    if !capabilities.no_pipe {
        help.push(build_stdin_message(executable_name, capabilities.is_chat));
        help.push(String::new());
    }
    let mut options_by_category: Vec<(char, OptionDescriptions)> = Vec::new();
    let mut subcommands: Vec<(String, String)> = Vec::new();
    for (option_id, description) in options {
        match description {
            OptionDescription::Subcommand(s) => {
                if let Some(description) = &s.description {
                    subcommands.push((option_id.clone(), description.clone()));
                }
            }
            OptionDescription::Option(o) => {
                if let (Some(_), Some(cat)) = (&o.description, o.cat) {
                    match options_by_category.iter_mut().find(|(c, _)| *c == cat) {
                        Some((_, list)) => list.push((option_id.clone(), description.clone())),
                        None => options_by_category.push((cat, vec![(option_id.clone(), description.clone())])),
                    }
                }
            }
        }
    }

    for (category, _, _) in HELP_CATEGORIES {
        if let Some((_, category_options)) = options_by_category.iter().find(|(c, _)| *c == category) {
            help.push(help_category(category));
            help.extend(format_options(category_options, columns));
            help.push(String::new());
        }
    }

    if !subcommands.is_empty() {
        help.push(localize("subcommands", "Subcommands", &[]));
        help.extend(format_usage_texts(&subcommands, columns));
        help.push(String::new());
    }

    help.join("\n")
}

/// O terceiro argumento de `buildHelpMessage`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HelpCapabilities {
    pub no_pipe: bool,
    pub no_input_files: bool,
    pub is_chat: bool,
}

/// `buildStdinMessage(executableName, isChat)`.
pub fn build_stdin_message(executable_name: &str, is_chat: bool) -> String {
    let example = match (cfg!(windows), is_chat) {
        (true, true) => format!("echo Hello World | {executable_name} chat <prompt> -"),
        (true, false) => format!("echo Hello World | {executable_name} -"),
        (false, true) => format!("ps aux | grep code | {executable_name} chat <prompt> -"),
        (false, false) => format!("ps aux | grep code | {executable_name} -"),
    };

    localize("stdinUsage", "To read from stdin, append '-' (e.g. '{0}')", &[&example])
}

/// `buildVersionMessage(version, commit)`.
pub fn build_version_message(version: Option<&str>, commit: Option<&str>) -> String {
    let version = version.filter(|v| !v.is_empty()).map_or_else(|| localize("unknownVersion", "Unknown version", &[]), str::to_owned);
    let commit = commit.filter(|v| !v.is_empty()).map_or_else(|| localize("unknownCommit", "Unknown commit", &[]), str::to_owned);
    format!("{version}\n{commit}\n{}", node_arch())
}

/// `process.arch`.
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    }
}
