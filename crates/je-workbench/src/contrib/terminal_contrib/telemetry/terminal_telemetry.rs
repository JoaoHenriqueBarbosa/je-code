//! Porte de `src/vs/workbench/contrib/terminalContrib/telemetry/browser/terminalTelemetry.ts`: a telemetria de criação dos
//! terminais (o tipo de shell sanitizado, o local, a qualidade da integração de shell) e de mudança do tipo de shell.
//!
//! Decisões de tradução:
//! - **`extends Disposable`** e **`this._store`**: o campo `store` (um `DisposableStore`), onde ficam os armazéns de cada instância.
//! - **`cancelOnDispose(store)`**, **`timeout(10000, token)`** e **`Promise.race`** são o [`cancel_on_dispose`], o
//!   [`timeout_with_token`] e o [`Promise::race`] do porte. `Event.toPromise(event, store)` é a promessa de um `once` do evento
//!   registrado no armazém da instância (que o descarta no fim).
//! - **`isCancellationError(error)`** é o [`is_cancellation_error`].
//! - **`terminalEditorService.getInputFromResource(instance.resource)`** com `getWindowById(...)` e `isAuxiliaryWindow(...)` é o
//!   [`ITerminalEditorService::get_input_from_resource`], que devolve se o editor está em janela auxiliar (a janela é do
//!   frontend); o `try { ... } catch { }` é o `Err`.
//! - **`TelemetryTrustedValue`** é o objeto de [`TelemetryTrustedValue::to_dynamic`].
//! - **`registerWorkbenchContribution2(ID, ..., WorkbenchPhase.AfterRestored)`** (`terminal.telemetry.contribution.ts`) não existe:
//!   quem monta o workbench cria a contribuição com [`TerminalTelemetryContribution::new`] depois da restauração.
//! - **`getSanitizedShellType`**: as listas de permissão são os conjuntos [`SHELL_TYPE_EXECUTABLE_ALLOW_LIST`] e as regex (no
//!   `zyarr`); `path.basename` é o `PathModule::native` do ambiente do processo.
//! - **`instance.xterm?.isImageAddonLoaded ?? false`** é o `ITerminalInstance::is_image_addon_loaded`.

use std::rc::Rc;

use je_base::async_::{async_fn, timeout_with_token};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, is_cancellation_error};
use je_base::cancellation::cancel_on_dispose;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::arrays::OneOrMany;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::telemetry::telemetry_utils::TelemetryTrustedValue;
use je_platform::terminal::capabilities::capabilities::{CommandDetection, PromptTypeDetection};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::terminal::{IShellLaunchConfig, TerminalLocation};
use zyarr::RegExp;

use super::terminal_profile_configuration_telemetry::TerminalProfileConfigurationTelemetry;
use crate::contrib::terminal::browser::terminal::{ITerminalEditorService, ITerminalInstance, ITerminalService};
use crate::services::lifecycle::lifecycle::ILifecycleService;

/// `TerminalTelemetryContribution`.
pub struct TerminalTelemetryContribution {
    store: Rc<DisposableStore>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    terminal_editor_service: Rc<dyn ITerminalEditorService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl TerminalTelemetryContribution {
    pub const ID: &'static str = "terminalTelemetry";

    pub fn new(
        lifecycle_service: Rc<dyn ILifecycleService>,
        terminal_service: Rc<dyn ITerminalService>,
        terminal_editor_service: Rc<dyn ITerminalEditorService>,
        configuration_service: Rc<dyn IConfigurationService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let this = Rc::new(Self { store, lifecycle_service, terminal_editor_service, telemetry_service });

        this.store.add(TerminalProfileConfigurationTelemetry::new(configuration_service, this.telemetry_service.clone()) as DisposableRef);

        let weak = Rc::downgrade(&this);
        this.store.add(terminal_service.on_did_create_instance().listen(move |instance| {
            let Some(this) = weak.upgrade() else { return };
            let instance = instance.clone();
            let _ = async_fn(async move { this.handle_created_instance(instance).await });
        }));

        let weak = Rc::downgrade(&this);
        this.store.add(terminal_service.on_any_instance_shell_type_changed().listen(move |instance| {
            if let Some(this) = weak.upgrade() {
                this.log_shell_type_changed(instance.as_ref());
            }
        }));

        this
    }

    /// O corpo do `async instance => { ... }` do `onDidCreateInstance`.
    async fn handle_created_instance(&self, instance: Rc<dyn ITerminalInstance>) -> Result<(), Error> {
        let store = DisposableStore::new();
        self.store.add(store.clone());
        let cancellation_token = cancel_on_dispose(&store);

        let race = self.wait_until_ready(&instance, &store, &cancellation_token);
        if let Err(error) = race.await {
            if cancellation_token.is_cancellation_requested() && is_cancellation_error(&error) {
                return Ok(());
            }
            return Err(error);
        }

        // Determine window status, this is done some time after the process is ready and could
        // reflect the terminal being moved.
        let is_in_aux_window = self.terminal_editor_service.get_input_from_resource(&instance.resource()).map(|input| input.is_in_auxiliary_window).unwrap_or(false);

        self.log_create_instance(instance.as_ref(), is_in_aux_window);
        let disposable: DisposableRef = store;
        self.store.delete(&disposable);
        Ok(())
    }

    /// O `Promise.race([...])` de `onDidCreateInstance`.
    fn wait_until_ready(&self, instance: &Rc<dyn ITerminalInstance>, store: &Rc<DisposableStore>, cancellation_token: &CancellationToken) -> Promise<()> {
        // Wait for process ready so the shell launch config is fully resolved, then
        // allow another 10 seconds for the shell integration to be fully initialized
        let token = cancellation_token.clone();
        let process_ready = instance.process_ready().then_promise(move |_| timeout_with_token(10000, &token));

        // If the terminal is disposed, it's ready to report on immediately
        let (disposed, disposed_resolver) = Promise::<()>::new();
        store.add(instance.on_disposed().once().listen(move |_| disposed_resolver.resolve(())));

        // If the app is shutting down, flush
        let (shutting_down, shutting_down_resolver) = Promise::<()>::new();
        store.add(self.lifecycle_service.on_will_shutdown().once().listen(move |_| shutting_down_resolver.resolve(())));

        Promise::race(vec![process_ready, disposed, shutting_down])
    }

    fn log_create_instance(&self, instance: &dyn ITerminalInstance, is_in_aux_window: bool) {
        let shell_launch_config = instance.shell_launch_config();
        let slc = shell_launch_config.borrow();
        let capabilities = instance.capabilities();
        let command_detection = capabilities.get::<CommandDetection>();
        let prompt_type = capabilities.get::<PromptTypeDetection>().and_then(|detection| detection.prompt_type());

        let location = match instance.target() {
            Some(TerminalLocation::Panel) => "view",
            Some(TerminalLocation::Editor) => {
                if is_in_aux_window {
                    "editor-auxwindow"
                } else {
                    "editor"
                }
            }
            None => "unknown",
        };
        let is_login_shell = match &slc.args {
            Some(OneOrMany::One(args)) => args.split(' ').any(|arg| arg == "-l" || arg == "--login"),
            Some(OneOrMany::Many(args)) => args.iter().any(|arg| arg == "-l" || arg == "--login"),
            None => false,
        };
        let trusted = |value: Dynamic| TelemetryTrustedValue { value }.to_dynamic();
        self.telemetry_service.public_log2(
            "terminal/createInstance",
            Some(Dynamic::object([
                ("location", Dynamic::string(location)),
                ("shellType", trusted(Dynamic::string(get_sanitized_shell_type(&slc)))),
                ("promptType", trusted(prompt_type.map_or(Dynamic::Undefined, |prompt_type| Dynamic::string(&prompt_type)))),
                ("isCustomPtyImplementation", Dynamic::Bool(slc.custom_pty_implementation.is_some())),
                ("isExtensionOwnedTerminal", Dynamic::Bool(slc.is_extension_owned_terminal == Some(true))),
                ("isLoginShell", Dynamic::Bool(is_login_shell)),
                ("isReconnect", Dynamic::Bool(slc.attach_persistent_process.is_some())),
                ("hasRemoteAuthority", Dynamic::Bool(instance.has_remote_authority())),
                (
                    "shellIntegrationQuality",
                    Dynamic::Number(match &command_detection {
                        Some(command_detection) if command_detection.has_rich_command_detection() => 2.0,
                        Some(_) => 1.0,
                        None => 0.0,
                    }),
                ),
                ("shellIntegrationInjected", Dynamic::Bool(instance.used_shell_integration_injection())),
                (
                    "shellIntegrationInjectionFailureReason",
                    instance.shell_integration_injection_failure_reason().map_or(Dynamic::Undefined, |reason| Dynamic::string(reason.as_str())),
                ),
                ("imageAddonLoaded", Dynamic::Bool(instance.is_image_addon_loaded())),
                ("terminalSessionId", Dynamic::string(&instance.session_id())),
            ])),
        );
    }

    fn log_shell_type_changed(&self, instance: &dyn ITerminalInstance) {
        let shell_type = instance.shell_type().map_or("unknown", |shell_type| shell_type.as_str());
        self.telemetry_service.public_log2(
            "terminal/shellTypeChanged",
            Some(Dynamic::object([
                ("shellType", TelemetryTrustedValue { value: Dynamic::string(shell_type) }.to_dynamic()),
                ("terminalSessionId", Dynamic::string(&instance.session_id())),
            ])),
        );
    }
}

impl IDisposable for TerminalTelemetryContribution {
    fn dispose(&self) {
        self.store.dispose();
    }
}

// #region Shell Type

/// Os tipos de shell que a telemetria pode relatar (`AllowedShellType`), no texto do `const enum`.
mod allowed_shell_type {
    pub const UNKNOWN: &str = "unknown";

    // Windows only
    pub const CYGWIN: &str = "cygwin-bash";
    pub const GIT_BASH: &str = "git-bash";
    pub const MSYS2: &str = "msys2-bash";
    pub const WINDOWS_POWER_SHELL: &str = "windows-powershell";
    pub const WSL: &str = "wsl";

    // Common Unix shells
    pub const PWSH_PREVIEW: &str = "pwsh-preview";

    // Lanugage REPLs
    pub const PYTHON: &str = "python";
}

/// Os `AllowedShellType` que casam direto com o nome do executável, sem a extensão (`shellTypeExecutableAllowList`).
pub const SHELL_TYPE_EXECUTABLE_ALLOW_LIST: [&str; 50] = [
    // Windows only
    "cmd",
    "wsl",
    // Common Unix shells
    "bash",
    "fish",
    "pwsh",
    "sh",
    "ssh",
    "tmux",
    "zsh",
    // More shells
    "amm",
    "ash",
    "csh",
    "dash",
    "elvish",
    "ion",
    "ksh",
    "mksh",
    "msh",
    "nu",
    "rc",
    "scsh",
    "tcsh",
    "termux",
    "xonsh",
    // Lanugage REPLs
    "clj",
    "sbcl",
    "crystal",
    "deno",
    "iex",
    "erl",
    "fsi",
    "go",
    "ghci",
    "jshell",
    "julia",
    "lua",
    "node",
    "ocaml",
    "perl",
    "php",
    "swipl",
    "python",
    "R",
    "irb",
    "scala",
    "racket",
    "gst",
    "pharo",
    "tclsh",
    "ts-node",
];

/// Dynamic executables that map to a single type (`shellTypeExecutableRegexAllowList`), com as flags `i`.
const SHELL_TYPE_EXECUTABLE_REGEX_ALLOW_LIST: [(&str, &str); 2] =
    [(r"^(?:pwsh|powershell)-preview$", allowed_shell_type::PWSH_PREVIEW), (r"^python(?:\d+(?:\.\d+)?)?$", allowed_shell_type::PYTHON)];

/// Path-based look ups (`shellTypePathRegexAllowList`), com as flags `i`.
const SHELL_TYPE_PATH_REGEX_ALLOW_LIST: [(&str, &str); 5] = [
    // Cygwin uses bash.exe, so look up based on the path
    (r"\\Cygwin(?:64)?\\.+\\bash\.exe$", allowed_shell_type::CYGWIN),
    // Git bash uses bash.exe, so look up based on the path
    (r"\\Git\\.+\\bash\.exe$", allowed_shell_type::GIT_BASH),
    // Msys2 uses bash.exe, so look up based on the path
    (r"\\msys(?:32|64)\\.+\\(?:bash|msys2)\.exe$", allowed_shell_type::MSYS2),
    // WindowsPowerShell should always be installed on this path, we cannot just look at the
    // executable name since powershell is the CLI on other platforms sometimes (eg. snap package)
    (r"\\WindowsPowerShell\\v1.0\\powershell.exe$", allowed_shell_type::WINDOWS_POWER_SHELL),
    // WSL executables will represent some other shell in the end, but it's difficult to determine
    // when we log
    (r"\\Windows\\(?:System32|SysWOW64|Sysnative)\\(?:bash|wsl)\.exe$", allowed_shell_type::WSL),
];

fn regex_matches(pattern: &str, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().collect();
    RegExp::from_str(pattern, "i").expect("a expressão de shell é válida").exec(&units, 0).is_some()
}

/// `getSanitizedShellType(slc)`.
pub fn get_sanitized_shell_type(slc: &IShellLaunchConfig) -> &'static str {
    let Some(executable) = slc.executable.as_deref().filter(|executable| !executable.is_empty()) else {
        return allowed_shell_type::UNKNOWN;
    };
    let executable_file = PathModule::native(&ProcessEnvironment).basename(executable, None);
    // `.replace(/\.[^\.]+$/, '')`: tira a última extensão (um ponto seguido de ao menos um caractere sem ponto até o fim).
    let executable_file_without_ext = match executable_file.rfind('.') {
        Some(index) if index + 1 < executable_file.len() => &executable_file[..index],
        _ => executable_file.as_str(),
    };
    for (pattern, shell_type) in SHELL_TYPE_PATH_REGEX_ALLOW_LIST {
        if regex_matches(pattern, executable) {
            return shell_type;
        }
    }
    for (pattern, shell_type) in SHELL_TYPE_EXECUTABLE_REGEX_ALLOW_LIST {
        if regex_matches(pattern, executable_file_without_ext) {
            return shell_type;
        }
    }
    if let Some(shell_type) = SHELL_TYPE_EXECUTABLE_ALLOW_LIST.iter().find(|shell_type| **shell_type == executable_file_without_ext) {
        return shell_type;
    }
    allowed_shell_type::UNKNOWN
}

// #endregion Shell Type
