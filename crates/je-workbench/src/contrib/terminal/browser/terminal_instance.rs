//! Porte parcial de `src/vs/workbench/contrib/terminal/browser/terminalInstance.ts`: `parseExitResult`,
//! `TerminalLabelComputer` (o cálculo do título e da descrição da aba a partir dos modelos `tabs.title` e `tabs.description`)
//! e `getCwdResource` (a resolução do diretório de trabalho detectado para um URI que existe).
//!
//! O resto do arquivo (a classe `TerminalInstance` com o processo, o título, o `rename`, o `runCommand`, o descarte e a parte
//! de DOM; `TerminalInstanceColorProvider`, o `guessShellTypeFromExecutable`...) depende de `ITerminalProcessManager`,
//! `ITerminalChildProcess`, `ITerminalBackend`, `ITerminalInstanceService`, `ITerminalService` e do
//! `workbenchInstantiationService` inteiro, que ainda não existem no porte, e do emulador de terminal da Fase 6, e entra
//! com eles.
//!
//! Divergências:
//! - **`getCwdResource`.** É [`get_cwd_resource`], uma função sobre o store de capacidades, o `remoteAuthority` e as três
//!   operações que o método lê dos serviços (`IPathService.fileURI`, `IFileService.canHandleResource` e
//!   `IFileService.exists`), reunidas em [`ICwdResourceServices`]: a classe `TerminalInstance` as injeta quando existir.
//! - **`Pick<ITerminalInstance, ...>`.** É [`LabelInstance`], com os campos que o cálculo lê.
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original; a classe
//!   herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **`parseExitResult`.** O original declara `| undefined` no retorno mas nunca o devolve; aqui o retorno é
//!   [`ExitResult`]. `ITerminalLaunchError | number | undefined` é [`ExitCodeOrError`] dentro de um `Option`. O
//!   `exitCodeOrError.message.match(/.*error code:\s*(\d+).*$/)` é lido na última linha da mensagem (o `.*$` sem a flag `m`
//!   só chega ao fim do texto sem atravessar quebras de linha), o que é o mesmo para as mensagens de uma linha só do
//!   `node-pty`; um `\s*` que atravessasse a quebra de linha entre `error code:` e os dígitos não é tratado.
//! - **`localeCompare(..., { sensitivity })`.** `case` compara os textos sem ignorar a caixa e `base` ignora a caixa; a
//!   equivalência de acentos (que a `Intl.Collator` também ignora nas duas sensibilidades) não é modelada.
//! - **`ITerminalLabelTemplateProperties`.** Os campos ausentes (`undefined`) não entram no dicionário do `template`, e os
//!   textos vazios entram como texto vazio, que o `template` trata do mesmo jeito.
//! - **`IProgressState`.** É [`ProgressState`] (`state` numérico e `value`).
//! - **`workspaceFolder.name`.** É `String` no porte, não `string | undefined`: um nome vazio vale como ausente e cai no
//!   nome base do caminho (o `instance.workspaceFolder?.name ?? ...` do original).
//! - **`isWindows`/`OS`.** São a plataforma detectada ([`Platform::detect`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::arrays::OneOrMany;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::labels::{Separator, TemplateValue, template, tildify};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_instantiation::Injectable;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::files::i_file_service::IFileService;
use je_platform::terminal::capabilities::capabilities::{
    CwdDetection, ITerminalCapabilityStore, TerminalCapability, TerminalCapabilityImpl, TerminalCommandConfidence,
};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::terminal::{GeneralShellType, IShellLaunchConfig, ITerminalLaunchError, ShellType, TerminalType};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};

use super::terminal::ITerminalConfigurationService;
use crate::contrib::terminal::common::terminal::ProcessState;

/// `ITerminalLaunchError | number`: o que `parseExitResult` recebe quando não é `undefined`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitCodeOrError {
    Code(i32),
    Error(ITerminalLaunchError),
}

/// `{ code: number | undefined; message: string | undefined }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitResult {
    pub code: Option<i32>,
    pub message: Option<String>,
}

/// `parseExitResult`: monta a mensagem amigável para o código de saída (ou erro de lançamento) do processo.
pub fn parse_exit_result(
    exit_code_or_error: Option<&ExitCodeOrError>,
    shell_launch_config: &IShellLaunchConfig,
    process_state: ProcessState,
    initial_cwd: Option<&str>,
) -> ExitResult {
    // Only return a message if the exit code is non-zero
    let exit_code_or_error = match exit_code_or_error {
        None => return ExitResult { code: None, message: None },
        Some(ExitCodeOrError::Code(0)) => return ExitResult { code: Some(0), message: None },
        Some(exit_code_or_error) => exit_code_or_error,
    };

    let code = match exit_code_or_error {
        ExitCodeOrError::Code(code) => Some(*code),
        ExitCodeOrError::Error(error) => error.code,
    };

    // Create exit code message
    let mut message: Option<String> = None;
    match exit_code_or_error {
        ExitCodeOrError::Code(code) => {
            let code = code.to_string();
            let mut command_line: Option<String> = None;
            if let Some(executable) = shell_launch_config.executable.as_ref().filter(|executable| !executable.is_empty()) {
                let mut line = executable.clone();
                match &shell_launch_config.args {
                    Some(OneOrMany::One(args)) if !args.is_empty() => {
                        line.push_str(&format!(" {args}"));
                    }
                    Some(OneOrMany::Many(args)) if !args.is_empty() => {
                        line.push_str(&args.iter().map(|arg| format!(" '{arg}'")).collect::<Vec<_>>().join(","));
                    }
                    _ => {}
                }
                command_line = Some(line);
            }
            message = Some(match (process_state == ProcessState::KilledDuringLaunch, command_line) {
                (true, Some(command_line)) => localize(
                    "launchFailed.exitCodeAndCommandLine",
                    "The terminal process \"{0}\" failed to launch (exit code: {1}).",
                    &[command_line.as_str(), code.as_str()],
                ),
                (true, None) => localize("launchFailed.exitCodeOnly", "The terminal process failed to launch (exit code: {0}).", &[code.as_str()]),
                (false, Some(command_line)) => localize(
                    "terminated.exitCodeAndCommandLine",
                    "The terminal process \"{0}\" terminated with exit code: {1}.",
                    &[command_line.as_str(), code.as_str()],
                ),
                (false, None) => localize("terminated.exitCodeOnly", "The terminal process terminated with exit code: {0}.", &[code.as_str()]),
            });
        }
        ExitCodeOrError::Error(error) => {
            // Ignore internal errors
            if !error.message.contains("Could not find pty with id") {
                // Convert conpty code-based failures into human friendly messages
                let mut inner_message = error.message.clone();
                match conpty_error_code(&error.message) {
                    Some(5) => {
                        inner_message = format!(
                            "Access was denied to the path containing your executable \"{}\". Manage and change your permissions to get this to work",
                            shell_launch_config.executable.as_deref().unwrap_or("undefined")
                        );
                    }
                    Some(267) => {
                        inner_message = format!("Invalid starting directory \"{}\", review your terminal.integrated.cwd setting", initial_cwd.unwrap_or("undefined"));
                    }
                    Some(1260) => {
                        inner_message = "Windows cannot open this program because it has been prevented by a software restriction policy. For more information, open Event Viewer or contact your system Administrator".to_owned();
                    }
                    _ => {}
                }
                message = Some(localize("launchFailed.errorMessage", "The terminal process failed to launch: {0}.", &[inner_message.as_str()]));
            }
        }
    }

    ExitResult { code, message }
}

/// `exitCodeOrError.message.match(/.*error code:\s*(\d+).*$/)` e `parseInt(match[1])`: o código da última linha.
fn conpty_error_code(message: &str) -> Option<i64> {
    let last_line = message.rsplit(['\n', '\r', '\u{2028}', '\u{2029}']).next().unwrap_or(message);
    let marker = "error code:";
    // O `.*` guloso fica com o último `error code:` da linha que seja seguido de espaços e de ao menos um dígito.
    last_line.rmatch_indices(marker).find_map(|(index, _)| {
        let after = last_line[index + marker.len()..].trim_start();
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() { None } else { digits.parse::<i64>().ok() }
    })
}

/// O que `getCwdResource` lê dos serviços: `IPathService.fileURI`, `IFileService.canHandleResource` e `IFileService.exists`.
pub trait ICwdResourceServices {
    /// `IPathService.fileURI(path)`: o URI do caminho no ambiente remoto.
    fn file_uri(&self, path: &str) -> Promise<URI>;

    /// `IFileService.canHandleResource(resource)`: se há provedor para o esquema do recurso.
    fn can_handle_resource(&self, resource: &URI) -> Promise<bool>;

    /// `IFileService.exists(resource)`.
    fn exists(&self, resource: &URI) -> Promise<bool>;
}

/// `TerminalInstance.getCwdResource`: o diretório de trabalho detectado como URI, ou `None` se não há cwd, se o serviço de
/// arquivos não trata o esquema ou se o diretório não existe.
pub fn get_cwd_resource(capabilities: &dyn ITerminalCapabilityStore, remote_authority: Option<&str>, services: Rc<dyn ICwdResourceServices>) -> Promise<Option<URI>> {
    let Some(cwd) = capabilities.get::<CwdDetection>().map(|cwd_detection| cwd_detection.get_cwd()).filter(|cwd| !cwd.is_empty()) else {
        return Promise::resolved(None);
    };
    let resource = if remote_authority.is_some_and(|authority| !authority.is_empty()) {
        services.file_uri(&cwd)
    } else {
        Promise::resolved(URI::file(cwd.as_str(), je_platform::resources::host_platform()))
    };
    // In VS Code web (server-linux-x64-web accessed via browser), remoteAuthority is falsy from the terminal's perspective, so
    // URI.file() is used above. The browser FileService has no file:// provider registered (only the remote provider), so
    // guard with canHandleResource before calling exists() to avoid an ENOPRO error propagating to callers.
    resource.then_promise(move |resource| {
        services.can_handle_resource(&resource).then_promise(move |can_handle| {
            if !can_handle {
                return Promise::resolved(None);
            }
            services.exists(&resource).then(move |exists| exists.then_some(resource))
        })
    })
}

/// `IProgressState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressState {
    /// `0` nenhum, `1` valor, `2` erro, `3` indeterminado, `4` aviso.
    pub state: i32,
    pub value: f64,
}

/// `Pick<ITerminalInstance, 'shellLaunchConfig' | 'shellType' | 'cwd' | 'fixedCols' | 'fixedRows' | 'initialCwd' |
/// 'processName' | 'sequence' | 'userHome' | 'workspaceFolder' | 'staticTitle' | 'capabilities' | 'title' |
/// 'description' | 'progressState' | 'os'>`.
#[derive(Clone)]
pub struct LabelInstance {
    pub shell_launch_config: IShellLaunchConfig,
    pub shell_type: Option<ShellType>,
    pub cwd: Option<String>,
    pub fixed_cols: Option<i32>,
    pub fixed_rows: Option<i32>,
    pub initial_cwd: Option<String>,
    pub process_name: String,
    pub sequence: Option<String>,
    pub user_home: Option<String>,
    pub workspace_folder: Option<WorkspaceFolder>,
    pub static_title: Option<String>,
    pub capabilities: Rc<dyn ITerminalCapabilityStore>,
    pub title: String,
    pub description: String,
    pub progress_state: Option<ProgressState>,
    pub os: Option<OperatingSystem>,
}

/// `TerminalLabelType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalLabelType {
    Title,
    Description,
}

/// O argumento de `onDidChangeLabel` (`{ title, description }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelChange {
    pub title: String,
    pub description: String,
}

/// O texto localizado de `terminalStrings.typeLocal`.
fn type_local() -> String {
    localize("local", "Local", &[])
}

/// O texto localizado de `terminalStrings.typeTask`.
fn type_task() -> String {
    localize("task", "Task", &[])
}

/// `\n`, `\r` e `\t` (`/[\n\r\t]/g`).
fn is_label_control_character(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\t')
}

/// `JS String(value)` de um valor de configuração.
fn config_text(value: &Dynamic) -> String {
    je_platform::js::to_js_string(value).to_string_lossy()
}

/// `TerminalLabelComputer`.
pub struct TerminalLabelComputer {
    base: Rc<Disposable>,
    title: RefCell<String>,
    description: RefCell<String>,
    on_did_change_label: Rc<Emitter<LabelChange>>,

    file_service: Rc<dyn IFileService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl TerminalLabelComputer {
    /// Agent CLIs whose tab title should come from their own escape sequences rather than the configured template or a static
    /// profile name (`agentCliShellTypes`).
    const AGENT_CLI_SHELL_TYPES: [GeneralShellType; 5] =
        [GeneralShellType::Claude, GeneralShellType::Codex, GeneralShellType::CommandCode, GeneralShellType::Copilot, GeneralShellType::Gemini];

    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    pub fn description(&self) -> String {
        self.description.borrow().clone()
    }

    pub fn on_did_change_label(&self) -> Event<LabelChange> {
        self.on_did_change_label.event()
    }

    pub fn refresh_label(&self, instance: &LabelInstance, reset: bool) {
        let tabs = self.terminal_configuration_service.config().get_str("tabs");
        let use_agent_cli_title = tabs.get_str("allowAgentCliTitle").is_truthy()
            && matches!(instance.shell_type, Some(ShellType::General(shell_type)) if Self::AGENT_CLI_SHELL_TYPES.contains(&shell_type));
        let title_template = match &instance.shell_launch_config.title_template {
            Some(title_template) => title_template.clone(),
            None if use_agent_cli_title => "${sequence}".to_owned(),
            None => config_text(&tabs.get_str("title")),
        };
        let title = self.compute_label(instance, &title_template, TerminalLabelType::Title, reset);
        let description = self.compute_label(instance, &config_text(&tabs.get_str("description")), TerminalLabelType::Description, false);
        *self.title.borrow_mut() = title.clone();
        *self.description.borrow_mut() = description.clone();
        if title != instance.title || description != instance.description || reset {
            self.on_did_change_label.fire(LabelChange { title, description });
        }
    }

    fn compute_label(&self, instance: &LabelInstance, label_template: &str, label_type: TerminalLabelType, reset: bool) -> String {
        let host = Platform::detect();
        let native_path = PathModule::native(&ProcessEnvironment);

        let type_ = instance.shell_launch_config.attach_persistent_process.as_ref().and_then(|process| process.type_).or(instance.shell_launch_config.type_);
        let command_detection = match instance.capabilities.get_capability(TerminalCapability::CommandDetection) {
            Some(TerminalCapabilityImpl::CommandDetection(command_detection)) => Some(command_detection),
            _ => None,
        };
        let prompt_input_model = command_detection.as_ref().map(|command_detection| command_detection.prompt_input_model());
        let non_task_spinner = if type_ == Some(TerminalType::Task) { "" } else { " $(loading~spin)" };

        let mut cwd = instance.cwd.clone().filter(|cwd| !cwd.is_empty()).or_else(|| instance.initial_cwd.clone().filter(|cwd| !cwd.is_empty())).unwrap_or_default();
        let os = instance.os.unwrap_or(host.os);
        let user_home = instance.user_home.clone().unwrap_or_default();
        cwd = tildify(&cwd, &user_home, os, &host);
        if os != OperatingSystem::Windows && !cwd.is_empty() && !user_home.is_empty() && cwd == user_home {
            cwd = "~".to_owned();
        }

        let executing_command = command_detection.as_ref().is_some_and(|command_detection| command_detection.executing_command().is_some_and(|command| !command.is_empty()));
        let workspace_folder_basename =
            instance.workspace_folder.as_ref().map(|folder| native_path.basename(&folder.uri.fs_path(je_platform::resources::host_platform()).to_string_lossy(), None));

        let mut properties: HashMap<String, TemplateValue> = HashMap::new();
        let mut set = |key: &str, value: Option<String>| {
            if let Some(value) = value {
                properties.insert(key.to_owned(), TemplateValue::Text(value));
            }
        };
        set("cwd", Some(cwd.clone()));
        set("cwdFolder", Some(String::new()));
        set("workspaceFolder", workspace_folder_basename.clone());
        set("local", (type_ == Some(TerminalType::Local)).then(type_local));
        set("process", Some(instance.process_name.clone()));
        set("sequence", instance.sequence.clone());
        set("task", (type_ == Some(TerminalType::Task)).then(type_task));
        set(
            "fixedDimensions",
            Some(match (instance.fixed_cols.filter(|cols| *cols != 0), instance.fixed_rows.filter(|rows| *rows != 0)) {
                (Some(cols), Some(rows)) => format!("\u{2194}{cols} \u{2195}{rows}"),
                (Some(cols), None) => format!("\u{2194}{cols}"),
                (None, Some(rows)) => format!("\u{2195}{rows}"),
                (None, None) => String::new(),
            }),
        );
        set("shellType", instance.shell_type.map(|shell_type| shell_type.as_str().to_owned()));
        // Shell command requires high confidence
        set(
            "shellCommand",
            match (&command_detection, &prompt_input_model) {
                (Some(command_detection), Some(prompt_input_model))
                    if executing_command && command_detection.executing_command_confidence() == Some(TerminalCommandConfidence::High) =>
                {
                    Some(prompt_input_model.value().to_string_lossy() + non_task_spinner)
                }
                _ => None,
            },
        );
        // Shell prompt input does not require high confidence as it's largely for VS Code developers
        set(
            "shellPromptInput",
            match &prompt_input_model {
                Some(prompt_input_model) if executing_command => Some(prompt_input_model.get_combined_string(Some(true)).to_string_lossy() + non_task_spinner),
                Some(prompt_input_model) => Some(prompt_input_model.get_combined_string(Some(true)).to_string_lossy()),
                None => None,
            },
        );
        set("progress", Some(Self::progress_state_string(instance.progress_state)));
        properties.insert(
            "separator".to_owned(),
            TemplateValue::Separator(Separator { label: config_text(&self.terminal_configuration_service.config().get_str("tabs").get_str("separator")) }),
        );
        let workspace_folder_name =
            instance.workspace_folder.as_ref().map(|folder| folder.name.clone()).filter(|name| !name.is_empty()).or(workspace_folder_basename);
        if let Some(workspace_folder_name) = workspace_folder_name {
            properties.insert("workspaceFolderName".to_owned(), TemplateValue::Text(workspace_folder_name));
        }

        let label_template = label_template.trim();
        if label_template.is_empty() {
            return if label_type == TerminalLabelType::Title { instance.process_name.clone() } else { String::new() };
        }
        if !reset && label_type == TerminalLabelType::Title {
            if let Some(static_title) = instance.static_title.as_ref().filter(|static_title| !static_title.is_empty()) {
                let stripped: String = static_title.chars().filter(|character| !is_label_control_character(*character)).collect();
                if !stripped.is_empty() {
                    return stripped;
                }
                return instance.process_name.chars().filter(|character| !is_label_control_character(*character)).collect();
            }
        }
        let detection = instance.capabilities.has(TerminalCapability::CwdDetection) || instance.capabilities.has(TerminalCapability::NaiveCwdDetection);
        let folders = self.workspace_context_service.get_workspace().folders();
        let multi_root_workspace = folders.len() > 1;

        // Only set cwdFolder if detection is on
        if !cwd.is_empty() && detection && (!instance.shell_launch_config.is_feature_terminal.is_truthy() || label_type == TerminalLabelType::Title) {
            let cwd_uri = URI::from(
                &UriComponents {
                    scheme: Some(
                        instance
                            .workspace_folder
                            .as_ref()
                            .map(|folder| folder.uri.scheme().clone())
                            .filter(|scheme| !scheme.is_empty())
                            .unwrap_or_else(|| "file".into()),
                    ),
                    path: instance.cwd.as_ref().filter(|cwd| !cwd.is_empty()).map(|cwd| native_path.resolve(&[cwd.as_str()]).as_str().into()),
                    ..UriComponents::default()
                },
                false,
            );
            // Multi-root workspaces always show cwdFolder to disambiguate them, otherwise only show
            // when it differs from the workspace folder in which it was launched from
            let mut show_cwd = false;
            if multi_root_workspace {
                show_cwd = true;
            } else if let Some(workspace_folder) = &instance.workspace_folder {
                let case_sensitive = self.file_service.has_capability(&workspace_folder.uri, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
                let (left, right) = (cwd_uri.fs_path(je_platform::resources::host_platform()).to_string_lossy(), workspace_folder.uri.fs_path(je_platform::resources::host_platform()).to_string_lossy());
                show_cwd = if case_sensitive { left != right } else { left.to_lowercase() != right.to_lowercase() };
            }
            if show_cwd {
                properties.insert("cwdFolder".to_owned(), TemplateValue::Text(native_path.basename(&cwd, None)));
            }
        }

        // Remove special characters that could mess with rendering
        let label: String = template(label_template, &properties).chars().filter(|character| !is_label_control_character(*character)).collect();
        let label = label.trim().to_owned();
        if label.is_empty() && label_type == TerminalLabelType::Title { instance.process_name.clone() } else { label }
    }

    fn progress_state_string(progress_state: Option<ProgressState>) -> String {
        let Some(progress_state) = progress_state else { return String::new() };
        match progress_state.state {
            1 => format!("{}%", (progress_state.value + 0.5).floor()),
            2 => "$(error)".to_owned(),
            3 => "$(loading~spin)".to_owned(),
            4 => "$(alert)".to_owned(),
            _ => String::new(),
        }
    }
}

impl Injectable for TerminalLabelComputer {
    type Args = ();
    type Deps = (Rc<dyn IFileService>, Rc<dyn ITerminalConfigurationService>, Rc<dyn IWorkspaceContextService>);

    fn create(_: (), (file_service, terminal_configuration_service, workspace_context_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_label = Emitter::new();
        base.register(on_did_change_label.clone());
        Rc::new(Self {
            base,
            title: RefCell::new(String::new()),
            description: RefCell::new(String::new()),
            on_did_change_label,
            file_service,
            terminal_configuration_service,
            workspace_context_service,
        })
    }
}

impl IDisposable for TerminalLabelComputer {
    fn dispose(&self) {
        self.base.dispose();
    }
}
