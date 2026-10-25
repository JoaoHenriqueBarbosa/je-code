//! Porte lógico de `src/vs/workbench/contrib/terminal/browser/terminalInstance.ts`: a classe [`TerminalInstance`] (o ciclo de
//! vida, o título e o rótulo, o processo, as capacidades, os eventos, o `dispose`, o `sendText`, o `runCommand`, o
//! `reuseTerminal` e o `relaunch`), o `parseExitResult`, o `TerminalLabelComputer` (o cálculo do título e da descrição da aba a
//! partir dos modelos `tabs.title` e `tabs.description`), o `getCwdResource` (a resolução do diretório de trabalho detectado
//! para um URI que existe) e o `guessShellTypeFromExecutable`.
//!
//! Fica fora da classe, porque depende de DOM, de serviços de interface ou de peças do xterm.js que ainda não existem no porte
//! (a UI nativa da Fase 6 as traz):
//! - o elemento `wrapper`, `attachToElement`, `detachFromElement`, `_open`, `layout`, `_resize` (e o `TerminalResizeDebouncer`),
//!   `_evaluateColsAndRows`, `_getDimension`, `setVisible` além do estado e do evento, o foco (`focus`, `focusWhenReady`,
//!   `hasFocus`, `_setFocus`), a seleção, a rolagem, `hasSelection`/`selection`/`clearSelection`, o arrasto de arquivos
//!   (`TerminalInstanceDragAndDropController`), o tratador de teclas (`attachCustomKeyEventHandler`), os ouvintes de mouse e
//!   `handleMouseEvent`, `forceScrollbarVisibility`/`resetScrollbarVisibility`, o `aria-label`, `updateAccessibilitySupport`;
//! - a barra de rolagem horizontal e as dimensões fixas por interface (`setFixedDimensions`, `toggleSizeToContentWidth`,
//!   `_addScrollbar`, `_removeScrollbar`, `_refreshScrollbar`);
//! - as chaves de contexto (`IContextKey` de foco, seleção, buffer alternativo e integração com o shell; só a chave do tipo de
//!   shell, que o chamador passa, é usada), as contribuições (`TerminalExtensionsRegistry`, `getContribution`), o
//!   `TerminalWidgetManager`, o sino e o sinal de acessibilidade, o `TerminalInstanceColorProvider`;
//! - o `LineDataEventAddon` (`onLineData`), o `onBinary`/`onKey`/`onSelectionChange` do xterm (o contrato do emulador não os tem),
//!   o `scrollToBottom`, `clearBuffer` e `clearDecorations` do `XtermTerminal`, o `windowsPty`/`reflowCursorLine` e o
//!   `disableStdin` das opções do emulador, o progresso (`progressState`), `registerMarker`/`addBufferMarker`/`scrollToMark`;
//! - o seletor de ícone (`TerminalIconPicker`) e o de cor (`createQuickPick`) de `changeIcon`/`changeColor`, a
//!   `TerminalLaunchHelpAction` da notificação de saída e o ouvinte de "qualquer tecla para fechar" do `waitOnExit`.
//!
//! Divergências da classe:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`] (aceita até 15), na ordem do
//!   original, só com os serviços que o código lógico usa. `IAccessibilityService`, `ITelemetryService` e `INotificationService`
//!   são lidos do serviço de instanciação quando usados (e a ausência vale como o serviço indefinido). Os parâmetros estáticos
//!   (`_terminalShellTypeContextKey` e `_shellLaunchConfig`) são [`TerminalInstanceArgs`], com o construtor do emulador
//!   (`xtermCtor`, que no original vem de `TerminalInstance.getXtermConstructor`) no lugar do módulo `@xterm/xterm`.
//! - **`IShellLaunchConfig` compartilhado.** É um [`SharedShellLaunchConfig`], porque o gerenciador de processo e o
//!   resolvedor de perfil o alteram no lugar (ver `terminal_process_manager`).
//! - **`ShellIntegrationAddon`.** No original é criado pelo `XtermTerminal`; aqui a instância o cria e o ativa no emulador
//!   (`XtermTerminal::raw`) logo depois de criar o `XtermTerminal`, e entrega as capacidades dele ao multiplexador.
//! - **Membros privados que os testes alcançam por colchete.** `_createProcess`, `_onTitleChange` e `_handleShellTypeChange` são
//!   [`TerminalInstance::create_process`], [`TerminalInstance::on_title_change`] e
//!   [`TerminalInstance::handle_shell_type_change`] (públicos), e os campos `_cwd` e `_userHome` que um teste escreve são
//!   [`TerminalInstance::override_cwd_and_user_home`].
//! - **`@debounce`.** Os métodos `relaunch` (1000), `_updateProcessCwd` (2000) e `_refreshShellIntegrationInfoStatus` (500) usam o
//!   [`Debounce`] do `je-base`. O `_fireMaximumDimensionsChanged` (50) fica fora com `_evaluateColsAndRows`.
//! - **`async`.** Os métodos `async` devolvem [`Promise`] e rodam como tarefas locais ([`run_async`]); o `throw` é o `Err`.
//! - **Eventos com `ITerminalInstance`.** `onWillDispose`, `onDisposed`, `onTitleChanged`, `onProcessIdReady` e `onIconChanged`
//!   levam o `Rc` da própria instância; o `onExit` é do emissor que sobrevive ao descarte, como no original.
//! - **`TerminalLabelComputer.refreshLabel(instance)`.** Recebe o retrato [`LabelInstance`] da instância ([`TerminalInstance::label_instance`]).
//! - **`getCwdResource`.** Usa o [`get_cwd_resource`] com os serviços de caminho e de arquivos da instância; o
//!   `canHandleResource` é o `hasProvider` do `IFileService`.
//! - **`description` e `LabelInstance.description`.** O `string | undefined` da descrição vira `String`, e o ausente é o texto
//!   vazio na comparação do `refreshLabel`.
//!
//! Divergências das funções e do `TerminalLabelComputer`:
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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::OneOrMany;
use je_base::async_::{AutoOpenBarrier, timeout};
use je_base::decorators::Debounce;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::labels::{Separator, TemplateValue, normalize_drive_letter, template, tildify};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_instantiation::{Injectable, InstantiationService, ServiceId};
use je_platform::accessibility::I_ACCESSIBILITY_SERVICE;
use je_platform::commands::commands::I_COMMAND_SERVICE;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::notification::notification::{I_NOTIFICATION_SERVICE, Notification};
use je_platform::path::path_service::IPathService;
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::terminal::capabilities::capabilities::{
    AnyTerminalCapabilityChangeEvent, CommandDetection, CwdDetection, ICommandDetectionCapability, ITerminalCapabilityStore, NaiveCwdDetection,
    TerminalCapability, TerminalCapabilityImpl, TerminalCommandConfidence,
};
use je_platform::terminal::capabilities::command_detection::prompt_input_model::PromptInputState;
use je_platform::terminal::capabilities::terminal_capability_store::{TerminalCapabilityStoreExt, TerminalCapabilityStoreMultiplexer};
use je_platform::terminal::environment_variable_shared::deserialize_environment_variable_collections;
use je_platform::terminal::shell_integration_addon::ShellIntegrationAddon;
use je_platform::terminal::terminal::{
    GeneralShellType, IProcessDataEvent, IProcessProperty, IShellLaunchConfig, ITerminalDimensionsOverride, ITerminalLaunchError, ITerminalLaunchOutcome,
    ITerminalLogService, InitialText, PosixShellType, ProcessPropertyType, ShellCwd, ShellIntegrationInjectionFailureReason, ShellIntegrationStatus, ShellType,
    TerminalExitReason, TerminalIcon, TerminalLocation, TerminalShellType, TerminalType, TitleEventSource, UnicodeVersion, WaitOnExitValue, WindowsShellType,
};
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_platform::terminal::terminal_strings::{ITerminalFormatMessageOptions, format_message_for_terminal};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};
use je_platform::workspace_trust::{IWorkspaceTrustRequestService, WorkspaceTrustRequestOptions};

use super::terminal::{ITerminalConfigurationService, ITerminalInstance};
use super::terminal_process_manager::{TerminalProcessManager, TerminalProcessManagerArgs, run_async};
use super::terminal_status_list::{ITerminalStatusList, TerminalStatus, TerminalStatusList};
use super::xterm::xterm_terminal::{IXtermRaw, IXtermTerminalOptions, XtermCtor, XtermTerminal, XtermTerminalArgs};
use crate::contrib::terminal::common::environment_variable::IEnvironmentVariableInfo;
use crate::contrib::terminal::common::terminal::{
    ITerminalProcessManager, ITerminalProcessInfo, ITerminalProfileResolverService, ITerminalStatus, ITerminalStatusHoverAction, IShellLaunchConfigResolveOptions,
    ProcessState, SharedShellLaunchConfig, TerminalCommandId,
};
use crate::contrib::terminal::common::terminal_backend::ITerminalBackend;
use crate::contrib::terminal::common::terminal_environment::{
    ITerminalBackendWslPath, ShellResource, get_shell_integration_timeout, get_uri_label_for_shell, prepare_path_for_shell,
};
use je_platform::contextkey::contextkey::IContextKey;
use je_platform::opener::opener::{I_OPENER_SERVICE, OpenResource};
use je_platform::terminal::terminal::IShellIntegration;
use je_platform::terminal::xterm_headless::{CsiParam, IHeadlessTerminal};

use crate::contrib::terminal::terminal_uri::get_terminal_uri;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::history::history::IHistoryService;

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

// ---------------------------------------------------------------------------------------------------------------------
// TerminalInstance
// ---------------------------------------------------------------------------------------------------------------------

/// `Constants.DefaultCols`.
const DEFAULT_COLS: i32 = 80;
/// `Constants.DefaultRows`.
const DEFAULT_ROWS: i32 = 30;
/// `Constants.WaitForContainerThreshold`: the maximum amount of milliseconds to wait for a container before starting to create
/// the terminal process. This period helps ensure the terminal has good initial dimensions to work with if it's going to be a
/// foreground terminal.
const WAIT_FOR_CONTAINER_THRESHOLD: u64 = 100;

/// `shellIntegrationSupportedShellTypes`.
const SHELL_INTEGRATION_SUPPORTED_SHELL_TYPES: [ShellType; 4] = [
    ShellType::Posix(PosixShellType::Bash),
    ShellType::Posix(PosixShellType::Zsh),
    ShellType::General(GeneralShellType::PowerShell),
    ShellType::General(GeneralShellType::Python),
];

thread_local! {
    /// `TerminalInstance._instanceIdCounter`.
    static INSTANCE_ID_COUNTER: Cell<u32> = const { Cell::new(1) };
}

/// `\w`.
fn is_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// `/first\s*second/i` sobre o texto já em minúsculas.
fn matches_spaced_words(lower: &str, first: &str, second: &str) -> bool {
    lower.match_indices(first).any(|(index, _)| lower[index + first.len()..].trim_start().starts_with(second))
}

/// `/\bword\b/i` sobre o texto já em minúsculas.
fn matches_whole_word(lower: &str, word: &str) -> bool {
    lower.match_indices(word).any(|(index, _)| {
        let before = lower[..index].chars().next_back();
        let after = lower[index + word.len()..].chars().next();
        !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
    })
}

/// `agentCliTitlePatterns`: patterns for detecting agent CLIs from the OSC title they emit, na ordem do original (a primeira que
/// casa vale). O `codex` não entra: it does not report osc title.
fn agent_cli_shell_type_from_title(title: &str) -> Option<GeneralShellType> {
    let lower = title.to_lowercase();
    if matches_spaced_words(&lower, "claude", "code") {
        Some(GeneralShellType::Claude)
    } else if matches_spaced_words(&lower, "command", "code") {
        Some(GeneralShellType::CommandCode)
    } else if matches_whole_word(&lower, "copilot") {
        Some(GeneralShellType::Copilot)
    } else if matches_whole_word(&lower, "gemini") {
        Some(GeneralShellType::Gemini)
    } else {
        None
    }
}

/// `guessShellTypeFromExecutable`.
pub fn guess_shell_type_from_executable(os: OperatingSystem, executable: &str) -> TerminalShellType {
    let exe_basename = PathModule::native(&ProcessEnvironment).basename(executable, None);
    // `generalShellTypeMap`, na ordem do original.
    let general = if exe_basename == "julia" {
        Some(GeneralShellType::Julia)
    } else if exe_basename == "node" {
        Some(GeneralShellType::Node)
    } else if exe_basename == "nu" {
        Some(GeneralShellType::NuShell)
    } else if exe_basename.starts_with("pwsh") || exe_basename.ends_with("powershell") {
        // `/^pwsh(-preview)?|powershell$/`: o `^` só vale para a primeira alternativa, e o `$` só para a segunda.
        Some(GeneralShellType::PowerShell)
    } else if exe_basename == "py" || exe_basename == "python" {
        Some(GeneralShellType::Python)
    } else if exe_basename.starts_with("xonsh") {
        Some(GeneralShellType::Xonsh)
    } else {
        None
    };
    if let Some(general) = general {
        return Some(ShellType::General(general));
    }

    if os == OperatingSystem::Windows {
        match exe_basename.as_str() {
            "cmd" => Some(ShellType::Windows(WindowsShellType::CommandPrompt)),
            "bash" => Some(ShellType::Windows(WindowsShellType::GitBash)),
            "wsl" => Some(ShellType::Windows(WindowsShellType::Wsl)),
            _ => None,
        }
    } else {
        [PosixShellType::Bash, PosixShellType::Csh, PosixShellType::Fish, PosixShellType::Ksh, PosixShellType::Sh, PosixShellType::Zsh]
            .into_iter()
            .find(|shell_type| exe_basename == shell_type.as_str())
            .map(ShellType::Posix)
    }
}

/// `onIconChanged`: `{ instance, userInitiated }`.
#[derive(Clone)]
pub struct IconChangedEvent {
    pub instance: Rc<dyn ITerminalInstance>,
    pub user_initiated: bool,
}

/// Os parâmetros estáticos do construtor da [`TerminalInstance`].
pub struct TerminalInstanceArgs {
    /// `_terminalShellTypeContextKey`.
    pub terminal_shell_type_context_key: Rc<dyn IContextKey>,
    /// `_shellLaunchConfig`.
    pub shell_launch_config: IShellLaunchConfig,
    /// O construtor do emulador (ver "Divergências").
    pub xterm_ctor: XtermCtor,
}

/// Um ouvinte que só roda enquanto a instância existe (a referência é fraca).
fn on_weak<S: 'static, T: 'static>(weak: Weak<S>, handler: impl Fn(&Rc<S>, &T) + 'static) -> impl Fn(&T) + 'static {
    move |value| {
        if let Some(strong) = weak.upgrade() {
            handler(&strong, value);
        }
    }
}

/// O que `getCwdResource` lê dos serviços da instância.
struct InstanceCwdServices {
    path_service: Rc<dyn IPathService>,
    file_service: Rc<dyn IFileService>,
}

impl ICwdResourceServices for InstanceCwdServices {
    fn file_uri(&self, path: &str) -> Promise<URI> {
        self.path_service.file_uri(path)
    }

    fn can_handle_resource(&self, resource: &URI) -> Promise<bool> {
        Promise::resolved(self.file_service.has_provider(resource))
    }

    fn exists(&self, resource: &URI) -> Promise<bool> {
        self.file_service.exists(resource)
    }
}

/// `TerminalInstance`.
pub struct TerminalInstance {
    this: Weak<Self>,
    /// O `_store` do `Disposable` do original.
    store: Rc<DisposableStore>,
    instance_id: u32,
    resource: URI,
    process_manager: Rc<dyn ITerminalProcessManager>,

    /// Resolves when xterm.js is ready, this will be `None` if the terminal instance is disposed before xterm.js could be created.
    xterm_ready_promise: RefCell<Promise<Option<Rc<XtermTerminal>>>>,
    xterm: RefCell<Option<Rc<XtermTerminal>>>,
    xterm_ctor: RefCell<Option<XtermCtor>>,
    shell_integration: RefCell<Option<Rc<ShellIntegrationAddon>>>,
    terminal_shell_type_context_key: Rc<dyn IContextKey>,

    latest_xterm_write_data: Cell<u32>,
    latest_xterm_parse_data: Cell<u32>,
    is_exiting: Cell<bool>,
    is_disposing: Cell<bool>,
    exit_code: Cell<Option<i32>>,
    exit_reason: Cell<Option<TerminalExitReason>>,
    shell_type: Cell<TerminalShellType>,
    agent_shell_type_from_sequence: Cell<Option<GeneralShellType>>,
    title: RefCell<String>,
    title_source: Cell<TitleEventSource>,
    cols: Cell<i32>,
    rows: Cell<i32>,
    fixed_cols: Cell<Option<i32>>,
    fixed_rows: Cell<Option<i32>>,
    cwd: RefCell<Option<String>>,
    initial_cwd: RefCell<Option<String>>,
    injected_args: RefCell<Option<Vec<String>>>,
    dimensions_override: Cell<Option<ITerminalDimensionsOverride>>,
    initial_data_events_listener: Rc<MutableDisposable<dyn IDisposable>>,
    initial_data_events: RefCell<Option<Vec<String>>>,
    container_ready_barrier: AutoOpenBarrier,
    icon: RefCell<Option<TerminalIcon>>,
    message_title_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    description: RefCell<Option<String>>,
    process_name: RefCell<String>,
    sequence: RefCell<Option<String>>,
    static_title: RefCell<Option<String>>,
    workspace_folder: RefCell<Option<WorkspaceFolder>>,
    label_computer: RefCell<Option<Rc<TerminalLabelComputer>>>,
    user_home: RefCell<Option<String>>,
    used_shell_integration_injection: Cell<bool>,
    shell_integration_injection_info: Cell<Option<ShellIntegrationInjectionFailureReason>>,
    is_visible: Cell<bool>,
    target: Cell<Option<TerminalLocation>>,
    shell_launch_config: RefCell<SharedShellLaunchConfig>,
    is_remote_resolver_terminal: bool,
    session_id: String,

    pub capabilities: Rc<TerminalCapabilityStoreMultiplexer>,
    pub status_list: Rc<TerminalStatusList>,

    relaunch_debounce: Debounce<(), ()>,
    refresh_shell_integration_info_status_debounce: Debounce<(), ()>,
    update_process_cwd_debounce: Debounce<(), ()>,

    /// The onExit event is special in that it fires and is disposed after the terminal instance itself is disposed.
    on_exit: Rc<Emitter<Option<ExitCodeOrError>>>,
    on_will_dispose: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_disposed: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_process_id_ready: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_process_replay_complete: Rc<Emitter<()>>,
    on_title_changed: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_icon_changed: Rc<Emitter<IconChangedEvent>>,
    on_will_data: Rc<Emitter<String>>,
    on_data: Rc<Emitter<String>>,
    on_dimensions_changed: Rc<Emitter<()>>,
    on_did_input_data: Rc<Emitter<String>>,
    on_did_change_has_child_processes: Rc<Emitter<bool>>,
    on_did_execute_text: Rc<Emitter<()>>,
    on_did_change_target: Rc<Emitter<Option<TerminalLocation>>>,
    on_did_send_text: Rc<Emitter<String>>,
    on_did_change_shell_type: Rc<Emitter<TerminalShellType>>,
    on_did_change_visibility: Rc<Emitter<bool>>,

    instantiation_service: Rc<InstantiationService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    terminal_profile_resolver_service: Rc<dyn ITerminalProfileResolverService>,
    path_service: Rc<dyn IPathService>,
    file_service: Rc<dyn IFileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: Rc<dyn ITerminalLogService>,
    workbench_environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    history_service: Rc<dyn IHistoryService>,
}

impl TerminalInstance {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a instância de terminal foi descartada da memória")
    }

    fn as_instance(&self) -> Rc<dyn ITerminalInstance> {
        self.rc()
    }

    /// Um serviço lido do serviço de instanciação quando usado (ver "Divergências").
    fn optional_service<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Option<Rc<T>> {
        self.instantiation_service.invoke_function(|accessor| accessor.try_get(id))
    }

    fn config_flag(&self, key: &str) -> bool {
        self.configuration_service.get_value(Some(key), None).is_truthy()
    }

    // --- getters ---

    /// `xtermReadyPromise`.
    pub fn xterm_ready_promise(&self) -> Promise<Option<Rc<XtermTerminal>>> {
        self.xterm_ready_promise.borrow().clone()
    }

    /// `xterm`.
    pub fn xterm(&self) -> Option<Rc<XtermTerminal>> {
        self.xterm.borrow().clone()
    }

    /// `shellIntegration` do `XtermTerminal` (ver "Divergências").
    pub fn shell_integration(&self) -> Option<Rc<ShellIntegrationAddon>> {
        self.shell_integration.borrow().clone()
    }

    pub fn used_shell_integration_injection(&self) -> bool {
        self.used_shell_integration_injection.get()
    }

    pub fn shell_integration_injection_failure_reason(&self) -> Option<ShellIntegrationInjectionFailureReason> {
        self.shell_integration_injection_info.get()
    }

    pub fn ext_environment_variable_collection(&self) -> Option<Rc<dyn je_platform::terminal::environment_variable::IMergedEnvironmentVariableCollection>> {
        self.process_manager.ext_environment_variable_collection()
    }

    /// `waitOnExit`: só o valor verdadeiro (`false` e o texto vazio valem como ausentes).
    pub fn wait_on_exit(&self) -> Option<WaitOnExitValue> {
        let slc = self.shell_launch_config.borrow().clone();
        let slc = slc.borrow();
        let value = slc.attach_persistent_process.as_ref().and_then(|process| process.wait_on_exit.clone()).filter(Self::is_truthy_wait_on_exit).or_else(|| slc.wait_on_exit.clone());
        value.filter(Self::is_truthy_wait_on_exit)
    }

    fn is_truthy_wait_on_exit(value: &WaitOnExitValue) -> bool {
        match value {
            WaitOnExitValue::Bool(value) => *value,
            WaitOnExitValue::Text(text) => !text.is_empty(),
            WaitOnExitValue::Callback(_) => true,
        }
    }

    /// `waitOnExit = value`.
    pub fn set_wait_on_exit(&self, value: Option<WaitOnExitValue>) {
        self.shell_launch_config.borrow().borrow_mut().wait_on_exit = value;
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible.get()
    }

    pub fn target(&self) -> Option<TerminalLocation> {
        self.target.get()
    }

    /// `target = value`.
    pub fn set_target(&self, value: Option<TerminalLocation>) {
        self.target.set(value);
        self.on_did_change_target.fire(value);
    }

    pub fn instance_id(&self) -> u32 {
        self.instance_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn resource(&self) -> &URI {
        &self.resource
    }

    pub fn cols(&self) -> i32 {
        if let Some(fixed_cols) = self.fixed_cols.get() {
            return fixed_cols;
        }
        if let Some(dimensions_override) = self.dimensions_override.get()
            && dimensions_override.cols != 0
        {
            if dimensions_override.force_exact_size.unwrap_or(false) {
                return dimensions_override.cols;
            }
            return dimensions_override.cols.max(2).min(self.cols.get());
        }
        self.cols.get()
    }

    pub fn rows(&self) -> i32 {
        if let Some(fixed_rows) = self.fixed_rows.get() {
            return fixed_rows;
        }
        if let Some(dimensions_override) = self.dimensions_override.get()
            && dimensions_override.rows != 0
        {
            if dimensions_override.force_exact_size.unwrap_or(false) {
                return dimensions_override.rows;
            }
            return dimensions_override.rows.max(2).min(self.rows.get());
        }
        self.rows.get()
    }

    /// `isDisposed`: `this._store.isDisposed`.
    pub fn is_disposed(&self) -> bool {
        self.store.is_disposed()
    }

    pub fn fixed_cols(&self) -> Option<i32> {
        self.fixed_cols.get()
    }

    pub fn fixed_rows(&self) -> Option<i32> {
        self.fixed_rows.get()
    }

    pub fn max_cols(&self) -> i32 {
        self.cols.get()
    }

    pub fn max_rows(&self) -> i32 {
        self.rows.get()
    }

    pub fn process_id(&self) -> Option<i32> {
        self.process_manager.shell_process_id()
    }

    /// `processReady`.
    pub fn process_ready(&self) -> Promise<()> {
        self.process_manager.pty_process_ready()
    }

    pub fn has_child_processes(&self) -> bool {
        let attach_has_child_processes = self.shell_launch_config.borrow().borrow().attach_persistent_process.as_ref().and_then(|process| process.has_child_processes).unwrap_or(false);
        attach_has_child_processes || self.process_manager.has_child_processes()
    }

    pub fn initial_data_events(&self) -> Option<Vec<String>> {
        self.initial_data_events.borrow().clone()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code.get()
    }

    pub fn exit_reason(&self) -> Option<TerminalExitReason> {
        self.exit_reason.get()
    }

    /// `isTitleSetByProcess`.
    pub fn is_title_set_by_process(&self) -> bool {
        self.message_title_disposable.value().is_some()
    }

    /// `shellLaunchConfig`.
    pub fn shell_launch_config(&self) -> SharedShellLaunchConfig {
        self.shell_launch_config.borrow().clone()
    }

    pub fn shell_type(&self) -> TerminalShellType {
        self.shell_type.get()
    }

    pub fn os(&self) -> Option<OperatingSystem> {
        self.process_manager.os()
    }

    pub fn has_remote_authority(&self) -> bool {
        self.process_manager.remote_authority().is_some()
    }

    pub fn remote_authority(&self) -> Option<String> {
        self.process_manager.remote_authority()
    }

    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    pub fn title_source(&self) -> TitleEventSource {
        self.title_source.get()
    }

    /// `icon`.
    pub fn icon(&self) -> Option<TerminalIcon> {
        self.get_icon()
    }

    /// `color`.
    pub fn color(&self) -> Option<String> {
        let slc = self.shell_launch_config.borrow().clone();
        let slc = slc.borrow();
        if let Some(color) = slc.color.as_ref().filter(|color| !color.is_empty()) {
            return Some(color.clone());
        }
        slc.attach_persistent_process.as_ref().and_then(|process| process.color.clone()).filter(|color| !color.is_empty())
    }

    pub fn process_name(&self) -> String {
        self.process_name.borrow().clone()
    }

    pub fn sequence(&self) -> Option<String> {
        self.sequence.borrow().clone()
    }

    pub fn static_title(&self) -> Option<String> {
        self.static_title.borrow().clone()
    }

    pub fn workspace_folder(&self) -> Option<WorkspaceFolder> {
        self.workspace_folder.borrow().clone()
    }

    pub fn cwd(&self) -> Option<String> {
        self.cwd.borrow().clone()
    }

    pub fn initial_cwd(&self) -> Option<String> {
        self.initial_cwd.borrow().clone()
    }

    /// `description`.
    pub fn description(&self) -> Option<String> {
        if let Some(description) = self.description.borrow().as_ref().filter(|description| !description.is_empty()) {
            return Some(description.clone());
        }
        let slc = self.shell_launch_config.borrow().clone();
        let slc = slc.borrow();
        match slc.attach_persistent_process.as_ref().and_then(|process| process.type_).or(slc.type_) {
            Some(TerminalType::Task) => Some(type_task()),
            Some(TerminalType::Local) => Some(type_local()),
            None => None,
        }
    }

    pub fn user_home(&self) -> Option<String> {
        self.user_home.borrow().clone()
    }

    pub fn shell_integration_nonce(&self) -> String {
        self.process_manager.shell_integration_nonce()
    }

    pub fn injected_args(&self) -> Option<Vec<String>> {
        self.injected_args.borrow().clone()
    }

    /// `persistentProcessId`.
    pub fn persistent_process_id(&self) -> Option<u32> {
        self.process_manager.persistent_process_id()
    }

    /// `reconnectionProperties`.
    pub fn reconnection_properties(&self) -> Option<je_platform::terminal::terminal::IReconnectionProperties> {
        let slc = self.shell_launch_config.borrow().clone();
        let slc = slc.borrow();
        slc.attach_persistent_process.as_ref().and_then(|process| process.reconnection_properties.clone()).or_else(|| slc.reconnection_properties.clone())
    }

    /// `shouldPersist`.
    pub fn should_persist(&self) -> bool {
        let is_transient = self.shell_launch_config.borrow().borrow().is_transient.unwrap_or(false);
        let reconnection_allowed = self.reconnection_properties().is_none() || self.configuration_service.get_value(Some("task.reconnection"), None).strict_equals(&Dynamic::Bool(true));
        self.process_manager.should_persist() && !is_transient && reconnection_allowed
    }

    // --- events ---

    /// `onExit`.
    pub fn on_exit(&self) -> Event<Option<ExitCodeOrError>> {
        self.on_exit.event()
    }

    pub fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_will_dispose.event()
    }

    pub fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_disposed.event()
    }

    pub fn on_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_process_id_ready.event()
    }

    pub fn on_process_replay_complete(&self) -> Event<()> {
        self.on_process_replay_complete.event()
    }

    pub fn on_title_changed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_title_changed.event()
    }

    pub fn on_icon_changed(&self) -> Event<IconChangedEvent> {
        self.on_icon_changed.event()
    }

    pub fn on_will_data(&self) -> Event<String> {
        self.on_will_data.event()
    }

    pub fn on_data(&self) -> Event<String> {
        self.on_data.event()
    }

    pub fn on_dimensions_changed(&self) -> Event<()> {
        self.on_dimensions_changed.event()
    }

    pub fn on_did_input_data(&self) -> Event<String> {
        self.on_did_input_data.event()
    }

    pub fn on_did_change_has_child_processes(&self) -> Event<bool> {
        self.on_did_change_has_child_processes.event()
    }

    pub fn on_did_execute_text(&self) -> Event<()> {
        self.on_did_execute_text.event()
    }

    pub fn on_did_change_target(&self) -> Event<Option<TerminalLocation>> {
        self.on_did_change_target.event()
    }

    pub fn on_did_send_text(&self) -> Event<String> {
        self.on_did_send_text.event()
    }

    pub fn on_did_change_shell_type(&self) -> Event<TerminalShellType> {
        self.on_did_change_shell_type.event()
    }

    pub fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    /// O retrato que o [`TerminalLabelComputer`] lê (o `Pick<ITerminalInstance, ...>` do original).
    pub fn label_instance(&self) -> LabelInstance {
        let capabilities: Rc<dyn ITerminalCapabilityStore> = self.capabilities.clone();
        LabelInstance {
            shell_launch_config: self.shell_launch_config.borrow().borrow().clone(),
            shell_type: self.shell_type.get(),
            cwd: self.cwd(),
            fixed_cols: self.fixed_cols.get(),
            fixed_rows: self.fixed_rows.get(),
            initial_cwd: self.initial_cwd(),
            process_name: self.process_name(),
            sequence: self.sequence(),
            user_home: self.user_home(),
            workspace_folder: self.workspace_folder(),
            static_title: self.static_title(),
            capabilities,
            title: self.title(),
            description: self.description().unwrap_or_default(),
            progress_state: None,
            os: self.os(),
        }
    }

    /// `this._labelComputer?.refreshLabel(this, reset)`.
    fn refresh_label(&self, reset: bool) {
        let label_computer = self.label_computer.borrow().clone();
        if let Some(label_computer) = label_computer {
            label_computer.refresh_label(&self.label_instance(), reset);
        }
    }

    // --- construção ---

    fn get_icon(&self) -> Option<TerminalIcon> {
        if self.icon.borrow().is_none() && self.process_manager.process_state() as i32 >= ProcessState::Launching as i32 {
            // `getIconRegistry().getIcon(...)` do `terminal.integrated.tabs.defaultIcon`: o registro de ícones não existe no porte,
            // então o ícone é o `ThemeIcon` do id configurado.
            let icon_id = je_platform::js::to_js_string(&self.configuration_service.get_value(Some(TerminalSettingId::TabsDefaultIcon.as_str()), None)).to_string_lossy();
            if !icon_id.is_empty() && icon_id != "undefined" {
                *self.icon.borrow_mut() = Some(TerminalIcon::Theme(ThemeIcon::from_id(icon_id)));
            }
        }
        self.icon.borrow().clone()
    }

    /// `_initDimensions`: sem contêiner, vale o tamanho de reserva.
    fn init_dimensions(&self) {
        self.cols.set(DEFAULT_COLS);
        self.rows.set(DEFAULT_ROWS);
    }

    fn wire_capability_listeners(this: &Rc<Self>) {
        this.store.add(this.capabilities.on_did_add_capability().listen(on_weak(
            this.this.clone(),
            |this: &Rc<Self>, event: &AnyTerminalCapabilityChangeEvent| {
                this.log_service.debug(&format!("terminalInstance added capability {:?}", event.id));
            },
        )));
        this.store.add(this.capabilities.on_did_remove_capability().listen(on_weak(
            this.this.clone(),
            |this: &Rc<Self>, event: &AnyTerminalCapabilityChangeEvent| {
                this.log_service.debug(&format!("terminalInstance removed capability {:?}", event.id));
            },
        )));

        let capability_listeners: Rc<DisposableMap<TerminalCapability, dyn IDisposable>> = DisposableMap::new();
        this.store.add_rc(capability_listeners.clone());
        let listeners = capability_listeners.clone();
        this.store.add(this.capabilities.on_did_add_capability().listen(on_weak(this.this.clone(), move |this: &Rc<Self>, event: &AnyTerminalCapabilityChangeEvent| {
            if let Some(previous) = listeners.get(&event.id) {
                previous.dispose();
            }
            let weak = this.this.clone();
            let refresh_info = move || {
                if let Some(this) = weak.upgrade() {
                    this.refresh_label(false);
                    this.refresh_shell_integration_info_status();
                }
            };
            match &event.capability {
                TerminalCapabilityImpl::CwdDetection(capability) => {
                    listeners.set(
                        event.id,
                        capability.on_did_change_cwd().listen(on_weak(this.this.clone(), |this: &Rc<Self>, cwd: &String| {
                            *this.cwd.borrow_mut() = Some(cwd.clone());
                            let title = this.title();
                            this.set_title(Some(&title), TitleEventSource::Config);
                        })),
                        false,
                    );
                }
                TerminalCapabilityImpl::CommandDetection(capability) => {
                    let prompt_input_model = capability.prompt_input_model();
                    prompt_input_model.set_shell_type(this.shell_type.get());
                    // Use DisposableStore to track multiple listeners for this capability
                    let store = DisposableStore::new();
                    let refresh_info = Rc::new(refresh_info);
                    for input_event in [prompt_input_model.on_did_start_input(), prompt_input_model.on_did_change_input(), prompt_input_model.on_did_finish_input()] {
                        let refresh_info = refresh_info.clone();
                        store.add(input_event.listen(move |_| refresh_info()));
                    }
                    store.add(capability.on_command_executed().listen(on_weak(this.this.clone(), |this: &Rc<Self>, command: &Rc<dyn je_platform::terminal::capabilities::capabilities::ITerminalCommand>| {
                        // Only generate ID if command doesn't already have one (i.e., it's a manual command, not Copilot-initiated)
                        // The tool terminal sets the command ID before command start, so this won't override it
                        let command_line = command.command();
                        if command.id().is_none_or(|id| id.is_empty()) && !command_line.is_empty() {
                            let command_id = generate_uuid();
                            if let Some(shell_integration) = this.shell_integration() {
                                shell_integration.set_next_command_id(&command_line, &command_id);
                            }
                            let _ = this.process_manager.set_next_command_id(&command_line, &command_id);
                        }
                    })));
                    listeners.set(event.id, store, false);
                }
                TerminalCapabilityImpl::PromptTypeDetection(capability) => {
                    listeners.set(event.id, capability.on_prompt_type_changed().listen(move |_| refresh_info()), false);
                }
                _ => {}
            }
        })));
        this.store.add(this.on_did_change_shell_type.event().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _: &TerminalShellType| {
            this.refresh_shell_integration_info_status();
        })));
        let listeners = capability_listeners;
        this.store.add(this.capabilities.on_did_remove_capability().listen(move |event| {
            listeners.delete_and_dispose(&event.id);
        }));
    }

    fn wire_process_manager(this: &Rc<Self>) {
        let process_manager = this.process_manager.clone();
        this.store.add(process_manager.on_process_ready().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _| {
            this.handle_process_ready();
        })));
        this.store.add(process_manager.on_process_exit().listen(on_weak(this.this.clone(), |this: &Rc<Self>, exit_code: &Option<i32>| {
            let _ = this.on_process_exit(exit_code.map(ExitCodeOrError::Code));
        })));
        this.store.add(process_manager.on_did_change_property().listen(on_weak(this.this.clone(), |this: &Rc<Self>, property: &IProcessProperty| {
            this.handle_process_property(property);
        })));

        let weak = this.this.clone();
        this.initial_data_events_listener.set(Some(process_manager.on_process_data().listen(move |event| {
            if let Some(this) = weak.upgrade()
                && let Some(events) = this.initial_data_events.borrow_mut().as_mut()
            {
                events.push(event.data.borrow().clone());
            }
        })));
        this.store.add(process_manager.on_process_replay_complete().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _| {
            this.on_process_replay_complete.fire(());
        })));
        this.store.add(process_manager.on_environment_variable_info_changed().listen(on_weak(
            this.this.clone(),
            |this: &Rc<Self>, info: &Rc<dyn IEnvironmentVariableInfo>| {
                this.on_environment_variable_info_changed(info.clone());
            },
        )));
        this.store.add(process_manager.on_pty_disconnect().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _| {
            this.status_list.add(
                Rc::new(ITerminalStatus {
                    id: TerminalStatus::Disconnected.as_str().to_owned(),
                    severity: Severity::Error,
                    icon: Some(ThemeIcon::from_id("debug-disconnect")),
                    tooltip: Some(localize("disconnectStatus", "Lost connection to process", &[])),
                    detailed_tooltip: None,
                    hover_actions: None,
                }),
                None,
            );
        })));
        this.store.add(process_manager.on_pty_reconnect().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _| {
            this.status_list.remove_id(TerminalStatus::Disconnected.as_str());
        })));
    }

    /// O `onProcessReady` do `_createProcessManager`.
    fn handle_process_ready(&self) {
        let this = self.rc();
        runtime::spawn_local(async move {
            this.on_process_id_ready.fire(this.as_instance());
            match this.get_initial_cwd().await {
                Ok(initial_cwd) => *this.initial_cwd.borrow_mut() = Some(initial_cwd),
                Err(error) => {
                    errors::on_unexpected_error(&error);
                    return;
                }
            }
            // Set the initial name based on the _resolved_ shell launch config, this will also ensure the resolved icon gets shown
            if this.label_computer.borrow().is_none() {
                let label_computer = this.instantiation_service.create_instance::<TerminalLabelComputer>(());
                this.store.add_rc(label_computer.clone());
                this.store.add(label_computer.on_did_change_label().listen(on_weak(this.this.clone(), |this: &Rc<Self>, label: &LabelChange| {
                    let was_changed = *this.title.borrow() != label.title || this.description.borrow().as_deref().unwrap_or("") != label.description;
                    if was_changed {
                        *this.title.borrow_mut() = label.title.clone();
                        *this.description.borrow_mut() = Some(label.description.clone());
                        this.on_title_changed.fire(this.as_instance());
                    }
                })));
                *this.label_computer.borrow_mut() = Some(label_computer);
            }
            let slc = this.shell_launch_config();
            let (name, title_template, executable) = {
                let slc = slc.borrow();
                (slc.name.clone().filter(|name| !name.is_empty()), slc.title_template.clone(), slc.executable.clone())
            };
            if name.is_some() && title_template.is_none() {
                this.set_title(name.as_deref(), TitleEventSource::Api);
            } else {
                // Listen to xterm.js' sequence title change event, trigger this async to ensure _xtermReadyPromise is ready
                // constructed since this is called from the ctor
                let weak = this.this.clone();
                runtime::set_timeout(
                    move || {
                        let Some(this) = weak.upgrade() else { return };
                        runtime::spawn_local(async move {
                            if let Ok(Some(xterm)) = this.xterm_ready_promise().await {
                                this.listen_to_sequence_title(&xterm);
                            }
                        });
                    },
                    0,
                );
                // When a title template is provided, use the name as the initial process name so it can be referenced via
                // ${process} in the template
                if title_template.is_some() && name.is_some() {
                    this.set_title(name.as_deref(), TitleEventSource::Process);
                } else {
                    this.set_title(executable.as_deref(), TitleEventSource::Process);
                }
            }
        });
    }

    /// `this._messageTitleDisposable.value = xterm.raw.onTitleChange(e => this._onTitleChange(e))`.
    fn listen_to_sequence_title(&self, xterm: &Rc<XtermTerminal>) {
        self.message_title_disposable.set(Some(xterm.raw().on_title_change().listen(on_weak(self.this.clone(), |this: &Rc<Self>, title: &JsString| {
            this.on_title_change(&title.to_string_lossy());
        }))));
    }

    /// O `onDidChangeProperty` do `_createProcessManager`.
    fn handle_process_property(&self, property: &IProcessProperty) {
        match property {
            IProcessProperty::Cwd(value) => {
                *self.cwd.borrow_mut() = Some(value.clone());
                self.refresh_label(false);
            }
            IProcessProperty::InitialCwd(value) => {
                *self.initial_cwd.borrow_mut() = Some(value.clone());
                *self.cwd.borrow_mut() = Some(value.clone());
                let title = self.title();
                self.set_title(Some(&title), TitleEventSource::Config);
                let slc = self.shell_launch_config.borrow().clone();
                *self.icon.borrow_mut() = slc.borrow().attach_persistent_process.as_ref().and_then(|process| process.icon.clone()).or_else(|| slc.borrow().icon.clone());
                self.on_icon_changed.fire(IconChangedEvent { instance: self.as_instance(), user_initiated: false });
            }
            IProcessProperty::Title(value) => self.set_title(Some(value), TitleEventSource::Process),
            IProcessProperty::OverrideDimensions(value) => self.set_override_dimensions(*value, true),
            IProcessProperty::ResolvedShellLaunchConfig(value) => self.set_resolved_shell_launch_config(value),
            IProcessProperty::ShellType(value) => self.handle_shell_type_change(*value),
            IProcessProperty::HasChildProcesses(value) => self.on_did_change_has_child_processes.fire(*value),
            IProcessProperty::UsedShellIntegrationInjection(_) => self.used_shell_integration_injection.set(true),
            IProcessProperty::ShellIntegrationInjectionFailureReason(value) => self.shell_integration_injection_info.set(*value),
            IProcessProperty::FixedDimensions(_) | IProcessProperty::FailedShellIntegrationActivation(_) => {}
        }
    }

    // --- xterm ---

    /// `_createXterm`: create xterm.js instance and attach data listeners.
    fn create_xterm(&self) -> Promise<Option<Rc<XtermTerminal>>> {
        let this = self.rc();
        run_async(async move {
            // `await TerminalInstance.getXtermConstructor(...)`
            Promise::resolved(()).await?;
            if this.is_disposed() {
                return Ok(None);
            }
            let Some(xterm_ctor) = this.xterm_ctor.borrow_mut().take() else { return Ok(None) };

            let executable_is_undefined = this.shell_launch_config.borrow().borrow().executable.is_none();
            let disable_shell_integration_reporting =
                executable_is_undefined || this.shell_type.get().is_none_or(|shell_type| !SHELL_INTEGRATION_SUPPORTED_SHELL_TYPES.contains(&shell_type));
            let xterm = this.instantiation_service.create_instance::<XtermTerminal>(XtermTerminalArgs {
                xterm_ctor,
                options: IXtermTerminalOptions { cols: this.cols.get(), rows: this.rows.get(), scrollback: None, detached: false, ..Default::default() },
            });
            *this.xterm.borrow_mut() = Some(xterm.clone());
            let raw: Rc<dyn IXtermRaw> = xterm.raw().clone();

            // The shell integration addon (created by the XtermTerminal in the original)
            let shell_integration = ShellIntegrationAddon::new(
                &this.process_manager.shell_integration_nonce(),
                Some(disable_shell_integration_reporting),
                Some(this.on_did_execute_text.event()),
                this.optional_service(I_TELEMETRY_SERVICE),
                this.log_service.clone(),
            );
            shell_integration.activate(raw.clone());
            this.store.add_rc(shell_integration.clone());
            *this.shell_integration.borrow_mut() = Some(shell_integration.clone());

            // Write initial text, deferring onLineFeed listener when applicable to avoid firing onLineData events containing
            // initialText
            if this.shell_launch_config.borrow().borrow().initial_text.is_some() {
                this.write_initial_text(&xterm, None);
            }

            this.store.add(this.process_manager.on_process_data().listen(on_weak(this.this.clone(), |this: &Rc<Self>, event: &Rc<IProcessDataEvent>| {
                this.handle_process_data(event.clone());
            })));
            this.store.add(raw.on_data().listen(on_weak(this.this.clone(), |this: &Rc<Self>, data: &JsString| {
                let _ = this.handle_on_data(data.to_string_lossy());
            })));
            // Init conpty compat and link handler after process creation as they rely on the underlying process OS
            this.store.add(this.process_manager.on_process_ready().listen(on_weak(this.this.clone(), {
                let raw = raw.clone();
                move |this: &Rc<Self>, process_traits: &je_platform::terminal::terminal::IProcessReadyEvent| {
                    // Respond to DA1 with basic conformance. Note that including this is required to avoid a long delay in conpty
                    // 1.22+ where it waits for the response.
                    if process_traits.windows_pty.is_some() {
                        let weak = this.this.clone();
                        this.store.add(raw.parser().register_csi_handler(
                            'c',
                            Rc::new(move |params: &[CsiParam]| {
                                if params.is_empty() || (params.len() == 1 && params[0].is(0)) {
                                    if let Some(this) = weak.upgrade() {
                                        let _ = this.handle_on_data("\x1b[?61;4c".to_owned());
                                    }
                                    return true;
                                }
                                false
                            }),
                        ));
                    }
                }
            })));
            this.store.add(this.process_manager.on_restore_commands().listen(on_weak(
                this.this.clone(),
                |this: &Rc<Self>, commands: &je_platform::terminal::capabilities::capabilities::ISerializedCommandDetectionCapability| {
                    if let Some(shell_integration) = this.shell_integration() {
                        shell_integration.deserialize(commands);
                    }
                },
            )));

            // Register and update the terminal's shell integration status
            this.store.add(shell_integration.on_did_change_seen_sequences().run_and_subscribe(
                {
                    let weak = this.this.clone();
                    move |_| {
                        if let Some(this) = weak.upgrade()
                            && this.shell_integration().is_some_and(|shell_integration| !shell_integration.seen_sequences().is_empty())
                        {
                            this.refresh_shell_integration_info_status();
                        }
                    }
                },
                None,
            ));

            this.capabilities.add(shell_integration.capabilities());

            let weak = this.this.clone();
            // O resultado não tem consumidor: o `then` do original só atribui o `_userHome`.
            let _ = this.path_service.user_home().then(move |user_home| {
                if let Some(this) = weak.upgrade() {
                    *this.user_home.borrow_mut() = Some(je_platform::resources::fs_path(&user_home));
                }
            });

            Ok(Some(xterm))
        })
    }

    /// `_writeInitialText`.
    fn write_initial_text(&self, xterm: &Rc<XtermTerminal>, callback: Option<Box<dyn FnOnce()>>) {
        let initial_text = self.shell_launch_config.borrow().borrow().initial_text.clone();
        let raw = xterm.raw();
        match initial_text {
            None => {
                if let Some(callback) = callback {
                    callback();
                }
            }
            Some(InitialText::Text(text)) => raw.write(&format!("{text}\r\n"), callback),
            Some(InitialText::WithTrailingNewLine { text, trailing_new_line }) => {
                if trailing_new_line {
                    raw.write(&format!("{text}\r\n"), callback);
                } else {
                    raw.write(&text, callback);
                }
            }
        }
    }

    /// `_handleOnData`.
    fn handle_on_data(&self, data: String) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            this.process_manager.write(&data).await?;
            this.on_did_input_data.fire(data);
            Ok(())
        })
    }

    /// `_onProcessData`.
    fn handle_process_data(&self, event: Rc<IProcessDataEvent>) {
        // Ensure events are split by SI command execute and command finished sequence to ensure the output of the command can
        // be read by extensions and the output of the command is of a consistent form respectively. This must be done here as
        // xterm.js does not currently have a listener for when individual data events are parsed, only `onWriteParsed` which
        // fires when the write buffer is flushed.
        let data = event.data.borrow().clone();
        let (leading_segmented_data, last_data) = split_command_sequences(&data);

        // Write all leading segmented data first, followed by the last data, tracking commit if necessary
        for segment in &leading_segmented_data {
            self.write_process_data(segment, None);
        }
        if event.track_commit {
            let (promise, resolver) = Promise::new();
            *event.write_promise.borrow_mut() = Some(promise);
            self.write_process_data(&last_data, Some(Box::new(move || resolver.resolve(()))));
        } else {
            self.write_process_data(&last_data, None);
        }
    }

    /// `_writeProcessData`.
    fn write_process_data(&self, data: &str, callback: Option<Box<dyn FnOnce()>>) {
        self.on_will_data.fire(data.to_owned());
        let message_id = self.latest_xterm_write_data.get() + 1;
        self.latest_xterm_write_data.set(message_id);
        let xterm = self.xterm();
        if let Some(xterm) = xterm {
            let weak = self.this.clone();
            let data = data.to_owned();
            xterm.raw().write(
                &data.clone(),
                Some(Box::new(move || {
                    let Some(this) = weak.upgrade() else { return };
                    this.latest_xterm_parse_data.set(message_id);
                    this.process_manager.acknowledge_data_event(data.encode_utf16().count());
                    if let Some(callback) = callback {
                        callback();
                    }
                    this.on_data.fire(data);
                })),
            );
        }
    }

    /// `_refreshShellIntegrationInfoStatus` (with `@debounce(500)`).
    fn refresh_shell_integration_info_status(&self) {
        self.refresh_shell_integration_info_status_debounce.call(());
    }

    fn do_refresh_shell_integration_info_status(&self) {
        let Some(shell_integration) = self.shell_integration() else { return };
        if self.xterm().is_none() {
            return;
        }
        let command_detection = self.capabilities.get::<CommandDetection>();
        let cmd_detection_type = if command_detection.as_ref().is_some_and(|command_detection| command_detection.has_rich_command_detection()) {
            localize("shellIntegration.rich", "Rich", &[])
        } else if command_detection.is_some() {
            localize("shellIntegration.basic", "Basic", &[])
        } else if self.used_shell_integration_injection.get() {
            localize("shellIntegration.injectionFailed", "Injection failed to activate", &[])
        } else {
            localize("shellIntegration.no", "No", &[])
        };

        let mut detailed_additions: Vec<String> = Vec::new();
        if let Some(shell_type) = self.shell_type.get() {
            detailed_additions.push(format!("Shell type: `{}`", shell_type.as_str()));
        }
        if let Some(cwd) = self.cwd().filter(|cwd| !cwd.is_empty()) {
            detailed_additions.push(format!("Current working directory: `{cwd}`"));
        }
        let seen_sequences = shell_integration.seen_sequences();
        if !seen_sequences.is_empty() {
            detailed_additions.push(format!("Seen sequences: {}", seen_sequences.iter().map(|sequence| format!("`{sequence}`")).collect::<Vec<_>>().join(", ")));
        }
        if let Some(prompt_type) = self.capabilities.get::<je_platform::terminal::capabilities::capabilities::PromptTypeDetection>().and_then(|capability| capability.prompt_type()) {
            detailed_additions.push(format!("Prompt type: `{prompt_type}`"));
        }
        if let Some(command_detection) = &command_detection {
            let combined_string = command_detection.prompt_input_model().get_combined_string(None).to_string_lossy();
            detailed_additions.push(format!("Prompt input: ```{combined_string}```"));
        }
        let detailed_additions_string =
            if detailed_additions.is_empty() { String::new() } else { format!("\n\n{}", detailed_additions.iter().map(|addition| format!("- {addition}")).collect::<Vec<_>>().join("\n")) };

        let shell_integration_label = localize("shellIntegration", "Shell integration", &[]);
        self.status_list.add(
            Rc::new(ITerminalStatus {
                id: TerminalStatus::ShellIntegrationInfo.as_str().to_owned(),
                severity: Severity::Info,
                icon: None,
                tooltip: Some(format!("{shell_integration_label}: {cmd_detection_type}")),
                detailed_tooltip: Some(format!("{shell_integration_label}: {cmd_detection_type}{detailed_additions_string}")),
                hover_actions: None,
            }),
            None,
        );
    }

    // --- comandos e texto ---

    /// `runCommand`.
    pub fn run_command(&self, command_line: &str, should_execute: bool, command_id: Option<&str>, force_bracketed_paste_mode: Option<bool>, command_line_for_metadata: Option<&str>) -> Promise<()> {
        let this = self.rc();
        let command_line = command_line.to_owned();
        let command_id = command_id.map(str::to_owned);
        let command_line_for_metadata = command_line_for_metadata.map(str::to_owned);
        run_async(async move {
            let mut command_detection = this.capabilities.get::<CommandDetection>();
            let si_injection_enabled = this.configuration_service.get_value(Some(TerminalSettingId::ShellIntegrationEnabled.as_str()), None).strict_equals(&Dynamic::Bool(true));
            let timeout_ms = get_shell_integration_timeout(
                &*this.configuration_service,
                si_injection_enabled,
                this.has_remote_authority(),
                Some(this.process_manager.process_ready_timestamp()),
            );

            if command_detection.as_ref().is_none_or(|command_detection| command_detection.prompt_input_model().state() != PromptInputState::Input) {
                let store = DisposableStore::new();
                let (waiting, resolver) = Promise::new();
                let wait_for_prompt_input = {
                    let store = store.clone();
                    let resolver = resolver.clone();
                    move |capability: &Rc<dyn ICommandDetectionCapability>| {
                        let prompt_input_model = capability.prompt_input_model();
                        if prompt_input_model.state() == PromptInputState::Input {
                            resolver.resolve(());
                        } else {
                            let resolver = resolver.clone();
                            store.add(prompt_input_model.on_did_start_input().listen(move |_| resolver.resolve(())));
                        }
                    }
                };
                match &command_detection {
                    Some(capability) => wait_for_prompt_input(capability),
                    None => {
                        store.add(this.capabilities.on_did_add_command_detection_capability().listen(move |capability| wait_for_prompt_input(capability)));
                    }
                }
                let timed_out = timeout(timeout_ms as u64);
                Promise::race(vec![waiting.clone(), (*timed_out).clone()]).await?;
                store.dispose();
                // `commandDetection = capability` do ouvinte: relê o que o store de capacidades tem agora.
                command_detection = this.capabilities.get::<CommandDetection>();
            }

            // If a command ID was provided and we have command detection, set it as the next command ID so it will be used when the
            // shell sends the command start sequence
            if let Some(command_id) = &command_id
                && !command_id.is_empty()
                && command_detection.is_some()
            {
                let command_line_to_report = command_line_for_metadata.clone().unwrap_or_else(|| command_line.clone());
                if let Some(shell_integration) = this.shell_integration() {
                    shell_integration.set_next_command_id(&command_line_to_report, command_id);
                }
                this.process_manager.set_next_command_id(&command_line_to_report, command_id).await?;
            }

            // Determine whether to send ETX (ctrl+c) before running the command. Only do this when the command will be executed
            // immediately or when command detection shows the prompt contains text.
            if should_execute && command_detection.as_ref().is_none_or(|command_detection| !command_detection.prompt_input_model().value().is_empty()) {
                this.send_text("\x03", false, None).await?;
                // Wait a little before running the command to avoid the sequences being echoed while the ^C is being evaluated
                timeout(100).await?;
            }
            // By default, use bracketed paste mode only when not running the command; callers can override this by explicitly
            // enabling it via the bracketedPasteMode argument.
            this.send_text(&command_line, should_execute, Some(!should_execute || force_bracketed_paste_mode.unwrap_or(false))).await
        })
    }

    /// `sendText`.
    pub fn send_text(&self, text: &str, should_execute: bool, force_bracketed_paste_mode: Option<bool>) -> Promise<()> {
        let this = self.rc();
        let mut text = text.to_owned();
        run_async(async move {
            // Apply bracketed paste sequences if the terminal has the mode enabled, this will prevent the text from triggering
            // keybindings and ensure new lines are handled properly
            if force_bracketed_paste_mode.unwrap_or(false) && this.xterm().is_some_and(|xterm| xterm.raw().bracketed_paste_mode()) {
                text = format!("\x1b[200~{text}\x1b[201~");
            }

            // Normalize line endings to 'enter' press.
            text = text.replace("\r\n", "\r").replace('\n', "\r");
            if should_execute && !text.ends_with('\r') {
                text.push('\r');
            }

            // Send it to the process
            this.log_service.debug(&format!("sending data (vscode) {text:?}"));
            this.process_manager.write(&text).await?;
            this.on_did_input_data.fire(text.clone());
            this.on_did_send_text.fire(text);
            if should_execute {
                this.on_did_execute_text.fire(());
            }
            Ok(())
        })
    }

    /// `sendSignal`.
    pub fn send_signal(&self, signal: &str) -> Promise<()> {
        self.log_service.debug(&format!("sending signal (vscode) {signal:?}"));
        self.process_manager.send_signal(signal)
    }

    /// `sendPath`.
    pub fn send_path(&self, original_path: ShellCwd, should_execute: bool) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            let prepared = this.prepare_path_for_shell(original_path).await?;
            this.send_text(&prepared, should_execute, None).await
        })
    }

    /// `preparePathForShell`.
    pub fn prepare_path_for_shell(&self, original_path: ShellCwd) -> Promise<String> {
        let this = self.rc();
        run_async(async move {
            // Wait for shell type to be ready
            this.process_ready().await?;
            let resource = match &original_path {
                ShellCwd::Path(path) => ShellResource::Path(path.as_str()),
                ShellCwd::Uri(uri) => ShellResource::Uri(uri),
            };
            let executable = this.shell_launch_config.borrow().borrow().executable.clone();
            let backend = this.process_manager.backend();
            prepare_path_for_shell(
                resource,
                executable.as_deref(),
                &this.title(),
                this.shell_type.get(),
                backend.as_deref().map(|backend| backend as &dyn ITerminalBackendWslPath),
                this.os(),
                None,
            )
            .await
        })
    }

    /// `getUriLabelForShell`.
    pub fn get_uri_label_for_shell(&self, uri: URI) -> Promise<String> {
        let this = self.rc();
        run_async(async move {
            // Wait for shell type to be ready
            this.process_ready().await?;
            let Some(backend) = this.process_manager.backend() else {
                return Err(Error::new("Cannot read properties of undefined (reading 'getWslPath')"));
            };
            get_uri_label_for_shell(ShellResource::Uri(&uri), &*backend, this.shell_type.get(), this.os(), None).await
        })
    }

    /// `setVisible` (a parte de DOM e de redimensionamento fica fora, ver o cabeçalho).
    pub fn set_visible(&self, visible: bool) {
        let did_change = self.is_visible.get() != visible;
        self.is_visible.set(visible);
        if did_change {
            self.on_did_change_visibility.fire(visible);
        }
    }

    /// `clearBuffer` (o do `XtermTerminal` fica fora, ver o cabeçalho).
    pub fn clear_buffer(&self) -> Promise<()> {
        self.process_manager.clear_buffer()
    }

    // --- processo ---

    /// `_createProcess`: público porque o teste do original o alcança por colchete.
    pub fn create_process(&self) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            if this.is_disposed() {
                return Ok(());
            }
            if !this.ensure_workspace_trusted_for_process().await? {
                return Ok(());
            }
            let cwd = this.cwd();
            let user_home = this.user_home();
            if this.workspace_context_service.get_workspace().folders().is_empty()
                && let (Some(cwd), Some(user_home)) = (cwd.as_deref().filter(|cwd| !cwd.is_empty()), user_home.as_deref().filter(|user_home| !user_home.is_empty()))
                && normalize_drive_letter(cwd, Platform::detect().is_windows) != normalize_drive_letter(user_home, Platform::detect().is_windows)
            {
                // something strange is going on if cwd is not userHome in an empty workspace
                let _ = this.on_process_exit(Some(ExitCodeOrError::Error(ITerminalLaunchError {
                    message: localize(
                        "workspaceEmptyCreateTerminalCwd",
                        "Cannot launch a terminal process in an empty workspace with cwd {0} different from userHome {1}",
                        &[cwd, user_home],
                    ),
                    code: None,
                })));
                return Ok(());
            }
            let slc = this.shell_launch_config();
            let original_icon = slc.borrow().icon.clone();
            let cols = if this.cols.get() == 0 { DEFAULT_COLS } else { this.cols.get() };
            let rows = if this.rows.get() == 0 { DEFAULT_ROWS } else { this.rows.get() };
            match this.process_manager.create_process(slc.clone(), cols, rows).await? {
                Some(ITerminalLaunchOutcome::Error(error)) => {
                    let _ = this.on_process_exit(Some(ExitCodeOrError::Error(error)));
                }
                Some(ITerminalLaunchOutcome::Result(result)) => *this.injected_args.borrow_mut() = Some(result.injected_args),
                None => {}
            }
            if this.is_disposed() {
                return Ok(());
            }
            let (icon_changed, has_color) = {
                let slc = slc.borrow();
                (original_icon != slc.icon, slc.color.as_ref().is_some_and(|color| !color.is_empty()))
            };
            if icon_changed || has_color {
                let slc = slc.borrow();
                *this.icon.borrow_mut() = slc.attach_persistent_process.as_ref().and_then(|process| process.icon.clone()).or_else(|| slc.icon.clone());
                this.on_icon_changed.fire(IconChangedEvent { instance: this.as_instance(), user_initiated: false });
            }
            Ok(())
        })
    }

    /// `_ensureWorkspaceTrustedForProcess`: ensures the workspace is trusted before a process is (re)launched. When trust is
    /// denied the terminal is marked as exited and `false` is returned, signalling the caller not to launch a process. Remote
    /// terminals in a remote workspace are still permitted when trust is denied, to match initial creation behavior; local
    /// terminals are always blocked.
    fn ensure_workspace_trusted_for_process(&self) -> Promise<bool> {
        let this = self.rc();
        run_async(async move {
            let trusted = this.is_remote_resolver_terminal || this.trust().await?;
            let is_remote_terminal = this.remote_authority().is_some_and(|authority| !authority.is_empty());
            let environment_has_remote_authority = this.workbench_environment_service.remote_authority().is_some_and(|authority| !authority.is_empty());
            if !trusted && !(is_remote_terminal && environment_has_remote_authority) {
                let _ = this.on_process_exit(Some(ExitCodeOrError::Error(ITerminalLaunchError {
                    message: localize("workspaceNotTrustedCreateTerminal", "Cannot launch a terminal process in an untrusted workspace", &[]),
                    code: None,
                })));
                return Ok(false);
            }
            Ok(true)
        })
    }

    /// `_trust`.
    fn trust(&self) -> Promise<bool> {
        let this = self.rc();
        run_async(async move {
            if this.config_flag(TerminalSettingId::AllowInUntrustedWorkspace.as_str()) {
                this.log_service.info(&format!("Workspace trust check bypassed due to {}", TerminalSettingId::AllowInUntrustedWorkspace.as_str()));
                return Ok(true);
            }
            let trust_request = this
                .workspace_trust_request_service
                .request_workspace_trust(Some(WorkspaceTrustRequestOptions {
                    message: Some(localize("terminal.requestTrust", "Creating a terminal process requires executing code", &[])),
                    ..WorkspaceTrustRequestOptions::default()
                }))
                .await?;
            Ok(trust_request == Some(true))
        })
    }

    /// `_onProcessExit`: called when either a process tied to a terminal has exited or when a terminal renderer simulates a
    /// process exiting (e.g. custom execution task). `exit_code_or_error` is `None` when the terminal was exited through user
    /// action.
    fn on_process_exit(&self, exit_code_or_error: Option<ExitCodeOrError>) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            // Prevent dispose functions being triggered multiple times
            if this.is_exiting.get() || this.is_disposed() {
                return Ok(());
            }
            let slc_snapshot = this.shell_launch_config.borrow().borrow().clone();
            let initial_cwd = this.initial_cwd();
            let parsed_exit_result = parse_exit_result(exit_code_or_error.as_ref(), &slc_snapshot, this.process_manager.process_state(), initial_cwd.as_deref());

            if this.used_shell_integration_injection.get()
                && this.process_manager.process_state() == ProcessState::KilledDuringLaunch
                && parsed_exit_result.code != Some(0)
            {
                this.relaunch_with_shell_integration_disabled(parsed_exit_result.message.as_deref());
                this.on_exit.fire(exit_code_or_error);
                return Ok(());
            }

            this.is_exiting.set(true);

            this.flush_xterm_data().await?;

            this.exit_code.set(parsed_exit_result.code);
            let exit_message = parsed_exit_result.message;

            this.log_service.debug(&format!(
                "Terminal process exit instanceId {} code {:?} processState {:?}",
                this.instance_id,
                this.exit_code.get(),
                this.process_manager.process_state() as i32
            ));

            // Fire onExit BEFORE running any disposition logic (in particular before `dispose()` below, which fires `onDisposed`).
            // Consumers racing `onExit` against `onDisposed` need to see the exit code event first so they can return the
            // captured exit code.
            this.on_exit.fire(exit_code_or_error);

            // Bail if disposed during flush; the work below would touch disposed services.
            if this.is_disposed() {
                return Ok(());
            }

            // Only trigger wait on exit when the exit was *not* triggered by the user (via the `workbench.action.terminal.kill`
            // command).
            let wait_on_exit = this.wait_on_exit();
            if let Some(wait_on_exit) = wait_on_exit
                && this.process_manager.process_state() != ProcessState::KilledByUser
            {
                let task = this.clone();
                runtime::spawn_local(async move {
                    let Ok(Some(xterm)) = task.xterm_ready_promise().await else { return };
                    let raw = xterm.raw();
                    if let Some(exit_message) = &exit_message {
                        raw.write(&format_message_for_terminal(exit_message, ITerminalFormatMessageOptions::default()), None);
                    }
                    let options = ITerminalFormatMessageOptions { exclude_leading_new_line: true, loud_formatting: false };
                    match &wait_on_exit {
                        WaitOnExitValue::Text(text) => raw.write(&format_message_for_terminal(text, options), None),
                        WaitOnExitValue::Callback(callback) => {
                            if let Some(exit_code) = task.exit_code() {
                                raw.write(&format_message_for_terminal(&callback(exit_code), options), None);
                            }
                        }
                        WaitOnExitValue::Bool(_) => {}
                    }
                });
            } else {
                if let Some(exit_message) = &exit_message {
                    let failed_during_launch = this.process_manager.process_state() == ProcessState::KilledDuringLaunch;
                    let ctrl_d = JsString::from("\x04");
                    let show_exit_alert = this.terminal_configuration_service.config().get_str("showExitAlert").is_truthy()
                        && this.xterm().and_then(|xterm| xterm.last_input_event()).as_ref() != Some(&ctrl_d);
                    if failed_during_launch || show_exit_alert {
                        if let Some(notification_service) = this.optional_service(I_NOTIFICATION_SERVICE) {
                            notification_service.notify(Notification::new(Severity::Error, exit_message.clone()));
                        }
                    } else {
                        // Log to help surface the error in case users report issues with showExitAlert disabled
                        this.log_service.warn(exit_message);
                    }
                }
                this.dispose_with_reason(Some(TerminalExitReason::Process));
            }

            // Dispose of the onExit event if the terminal will not be reused again
            if this.is_disposed() {
                this.on_exit.dispose();
            }
            Ok(())
        })
    }

    /// `_relaunchWithShellIntegrationDisabled`.
    fn relaunch_with_shell_integration_disabled(&self, exit_message: Option<&str>) {
        self.shell_launch_config.borrow().borrow_mut().ignore_shell_integration = Some(true);
        self.relaunch();
        let learn_more_service = self.optional_service(I_OPENER_SERVICE);
        let command_service = self.optional_service(I_COMMAND_SERVICE);
        self.status_list.add(
            Rc::new(ITerminalStatus {
                id: TerminalStatus::ShellIntegrationAttentionNeeded.as_str().to_owned(),
                severity: Severity::Warning,
                icon: Some(ThemeIcon::from_id("warning")),
                tooltip: Some(format!(
                    "{} {}",
                    exit_message.unwrap_or("undefined"),
                    localize("launchFailed.exitCodeOnlyShellIntegration", "Disabling shell integration in user settings might help.", &[])
                )),
                detailed_tooltip: None,
                hover_actions: Some(vec![
                    ITerminalStatusHoverAction {
                        command_id: TerminalCommandId::ShellIntegrationLearnMore.as_str().to_owned(),
                        label: localize("shellIntegration.learnMore", "Learn more about shell integration", &[]),
                        run: Rc::new(move || {
                            if let Some(opener_service) = &learn_more_service {
                                let _ = opener_service.open(&OpenResource::Text(JsString::from("https://code.visualstudio.com/docs/terminal/shell-integration?referrer=in-product")), None);
                            }
                        }),
                    },
                    ITerminalStatusHoverAction {
                        command_id: "workbench.action.openSettings".to_owned(),
                        label: localize("shellIntegration.openSettings", "Open user settings", &[]),
                        run: Rc::new(move || {
                            if let Some(command_service) = &command_service {
                                let _ = command_service.execute_command("workbench.action.openSettings", vec![Dynamic::string("terminal.integrated.shellIntegration.enabled")]);
                            }
                        }),
                    },
                ]),
            }),
            None,
        );
        if let Some(telemetry_service) = self.optional_service(I_TELEMETRY_SERVICE) {
            telemetry_service.public_log2("terminal/shellIntegrationFailureProcessExit", None);
        }
    }

    /// `_flushXtermData`: ensure write calls to xterm.js have finished before resolving.
    fn flush_xterm_data(&self) -> Promise<()> {
        if self.latest_xterm_write_data.get() == self.latest_xterm_parse_data.get() {
            return Promise::resolved(());
        }
        let this = self.rc();
        run_async(async move {
            let mut retries = 0;
            loop {
                timeout(20).await?;
                retries += 1;
                if this.latest_xterm_write_data.get() == this.latest_xterm_parse_data.get() || retries == 5 {
                    return Ok(());
                }
            }
        })
    }

    /// `dispose(reason)`.
    pub fn dispose_with_reason(&self, reason: Option<TerminalExitReason>) {
        let (is_task, wait_on_exit) = (self.shell_launch_config.borrow().borrow().type_ == Some(TerminalType::Task), self.wait_on_exit());
        if is_task && reason == Some(TerminalExitReason::Process) && self.exit_code.get() != Some(0) && wait_on_exit.is_none() {
            return;
        }
        if self.is_disposed() {
            return;
        }
        self.log_service.trace(&format!("terminalInstance#dispose (instanceId: {})", self.instance_id));
        self.is_disposing.set(true);

        // Fire onWillDispose before disposing xterm so that contributions can clean up their xterm addons while the raw terminal
        // is still alive.
        self.on_will_dispose.fire(self.as_instance());

        let xterm = self.xterm.borrow().clone();
        if let Some(xterm) = xterm {
            xterm.dispose();
        }

        if self.exit_reason.get().is_none() {
            self.exit_reason.set(Some(reason.unwrap_or(TerminalExitReason::Unknown)));
        }

        self.process_manager.dispose();
        // Process manager dispose/shutdown doesn't fire process exit, trigger with undefined if it hasn't happened yet
        let _ = self.on_process_exit(None);

        // Fire onDisposed only after xterm has been disposed so that subscribers observe a fully disposed instance.
        self.on_disposed.fire(self.as_instance());

        self.store.dispose();
    }

    /// `detachProcessAndDispose`.
    pub fn detach_process_and_dispose(&self, reason: TerminalExitReason) -> Promise<()> {
        let this = self.rc();
        run_async(async move {
            // Detach the process and dispose the instance, without the instance dispose the terminal won't go away. Force
            // persist if the detach was requested by the user (not shutdown).
            this.process_manager.detach_from_process(Some(reason == TerminalExitReason::User)).await?;
            this.dispose_with_reason(Some(reason));
            Ok(())
        })
    }

    /// `reuseTerminal`.
    pub fn reuse_terminal(&self, shell: IShellLaunchConfig, reset: bool) -> Promise<()> {
        let this = self.rc();
        let mut shell = shell;
        run_async(async move {
            // Relaunching reaches the process manager directly, so re-check workspace trust here to ensure the reuse path cannot
            // bypass the gate enforced on initial process creation.
            if !this.ensure_workspace_trusted_for_process().await? {
                return Ok(());
            }

            let xterm = this.xterm();
            if let Some(xterm) = xterm {
                if !reset {
                    // Ensure new processes' output starts at start of new line
                    let (written, resolver) = Promise::new();
                    xterm.raw().write("\n\x1b[G", Some(Box::new(move || resolver.resolve(()))));
                    written.await?;
                }

                // Print initialText if specified
                if let Some(initial_text) = shell.initial_text.clone() {
                    this.shell_launch_config.borrow().borrow_mut().initial_text = Some(initial_text);
                    let (written, resolver) = Promise::new();
                    this.write_initial_text(&xterm, Some(Box::new(move || resolver.resolve(()))));
                    written.await?;
                }

                // Clean up waitOnExit state
                if this.is_exiting.get() && this.wait_on_exit().is_some() {
                    this.is_exiting.set(false);
                }
            }

            // Dispose the environment info widget if it exists
            this.status_list.remove_id(TerminalStatus::RelaunchNeeded.as_str());

            if !reset {
                // HACK: Force initialText to be non-falsy for reused terminals such that the conptyInheritCursor flag is passed to
                // the node-pty, this flag can cause a Window to stop responding in Windows 10 1903 so we only want to use it when
                // something is definitely written to the terminal.
                shell.initial_text = Some(InitialText::Text(" ".to_owned()));
            }

            // Set the new shell launch config
            let shared: SharedShellLaunchConfig = Rc::new(RefCell::new(shell)); // Must be done before calling _createProcess()
            *this.shell_launch_config.borrow_mut() = shared.clone();
            this.agent_shell_type_from_sequence.set(None);
            let cols = if this.cols.get() == 0 { DEFAULT_COLS } else { this.cols.get() };
            let rows = if this.rows.get() == 0 { DEFAULT_ROWS } else { this.rows.get() };
            match this.process_manager.relaunch(shared, cols, rows, reset).await? {
                Some(ITerminalLaunchOutcome::Error(error)) => {
                    let _ = this.on_process_exit(Some(ExitCodeOrError::Error(error)));
                }
                Some(ITerminalLaunchOutcome::Result(result)) => *this.injected_args.borrow_mut() = Some(result.injected_args),
                None => {}
            }
            Ok(())
        })
    }

    /// `relaunch` (with `@debounce(1000)`).
    pub fn relaunch(&self) {
        self.relaunch_debounce.call(());
    }

    fn do_relaunch(&self) {
        // Clear the attachPersistentProcess flag to ensure we create a new process instead of trying to reattach to the existing
        // one during relaunch.
        let mut shell_launch_config = self.shell_launch_config.borrow().borrow().clone();
        shell_launch_config.attach_persistent_process = None;

        // O resultado não é aguardado (`this.reuseTerminal(shellLaunchConfig, true)` sem `await`).
        let _ = self.reuse_terminal(shell_launch_config, true);
    }

    // --- título e tipo de shell ---

    /// `_onTitleChange`: público porque o teste do original o alcança por colchete.
    pub fn on_title_change(&self, title: &str) {
        if self.is_title_set_by_process() {
            self.set_title(Some(title), TitleEventSource::Sequence);
        }
        // Agent CLIs run as `node`, so the OSC title is our only cross-platform signal.
        if let Some(shell_type) = agent_cli_shell_type_from_title(title) {
            self.agent_shell_type_from_sequence.set(Some(shell_type));
            self.set_shell_type(Some(ShellType::General(shell_type)));
        }
    }

    /// `_handleShellTypeChange`: público porque o teste do original o alcança por colchete.
    pub fn handle_shell_type_change(&self, shell_type: TerminalShellType) {
        // Once an agent CLI is locked in, ignore stale `node`/undefined reports from the pty until a real shell takes over
        // (meaning the agent exited).
        if self.agent_shell_type_from_sequence.get().is_some() {
            if shell_type.is_none() || shell_type == Some(ShellType::General(GeneralShellType::Node)) {
                return;
            }
            self.agent_shell_type_from_sequence.set(None);
        }
        self.set_shell_type(shell_type);
    }

    /// `setShellType`.
    pub fn set_shell_type(&self, shell_type: TerminalShellType) {
        if self.shell_type.get() == shell_type {
            return;
        }
        self.shell_type.set(shell_type);
        match shell_type {
            None => self.terminal_shell_type_context_key.reset(),
            Some(shell_type) => self.terminal_shell_type_context_key.set(Dynamic::string(shell_type.as_str())),
        }
        self.on_did_change_shell_type.fire(shell_type);
        self.refresh_label(false);
    }

    /// `_updateTitleProperties`.
    fn update_title_properties(&self, title: Option<&str>, event_source: TitleEventSource) -> String {
        let Some(title) = title else {
            if event_source == TitleEventSource::Api {
                *self.static_title.borrow_mut() = None;
                self.title_source.set(TitleEventSource::Process);
                self.message_title_disposable.clear();
                if let Some(xterm) = self.xterm() {
                    self.listen_to_sequence_title(&xterm);
                }
            }
            return self.process_name();
        };
        let mut title = title.to_owned();
        let is_windows_backend = self.process_manager.os() == Some(OperatingSystem::Windows);
        match event_source {
            TitleEventSource::Process => {
                if is_windows_backend {
                    // Extract the file name without extension
                    title = PathModule::win32(&ProcessEnvironment).parse(&title).name;
                } else {
                    let first_space_index = title.find(' ');
                    if title.starts_with('/') {
                        title = PathModule::native(&ProcessEnvironment).basename(&title, None);
                    } else if let Some(first_space_index) = first_space_index {
                        title.truncate(first_space_index);
                    }
                }
                *self.process_name.borrow_mut() = title.clone();
            }
            TitleEventSource::Api => {
                // If the title has not been set by the API or the rename command, unregister the handler that automatically
                // updates the terminal name
                *self.static_title.borrow_mut() = Some(title.clone());
                self.message_title_disposable.clear();
            }
            TitleEventSource::Sequence => {
                // On Windows, some shells will fire this with the full path which we want to trim to show just the file name.
                // This should only happen if the title looks like an absolute Windows file path
                let mut sequence = title.clone();
                if is_windows_backend && looks_like_windows_file_path(&title) {
                    sequence = PathModule::win32(&ProcessEnvironment).parse(&title).name;
                }
                *self.sequence.borrow_mut() = Some(sequence);
            }
            TitleEventSource::Config => {}
        }
        self.title_source.set(event_source);
        title
    }

    /// `rename`.
    pub fn rename(&self, title: Option<&str>, source: Option<TitleEventSource>) {
        let title = title.filter(|title| !title.is_empty());
        self.set_title(title, source.unwrap_or(TitleEventSource::Api));
    }

    /// `_setTitle`.
    fn set_title(&self, title: Option<&str>, event_source: TitleEventSource) {
        let is_task = self.shell_launch_config.borrow().borrow().type_ == Some(TerminalType::Task);
        if (is_task || self.title_source.get() == TitleEventSource::Api) && event_source == TitleEventSource::Process {
            return;
        }

        let reset = title.is_none_or(str::is_empty);
        let title = self.update_title_properties(title, event_source);
        let title_changed = title != *self.title.borrow();
        *self.title.borrow_mut() = title;
        self.refresh_label(reset);

        if title_changed {
            self.on_title_changed.fire(self.as_instance());
        }
    }

    /// `changeIcon(icon)` (sem o seletor de ícone, ver o cabeçalho).
    pub fn change_icon(&self, icon: TerminalIcon) -> TerminalIcon {
        *self.icon.borrow_mut() = Some(icon.clone());
        self.on_icon_changed.fire(IconChangedEvent { instance: self.as_instance(), user_initiated: true });
        icon
    }

    /// `changeColor(color, skipQuickPick)` (sem o seletor de cor, ver o cabeçalho): a cor dada, ou o reinício da cor da aba.
    pub fn change_color(&self, color: Option<&str>, skip_quick_pick: bool) -> Option<String> {
        if let Some(color) = color.filter(|color| !color.is_empty()) {
            self.shell_launch_config.borrow().borrow_mut().color = Some(color.to_owned());
            self.on_icon_changed.fire(IconChangedEvent { instance: self.as_instance(), user_initiated: true });
            return Some(color.to_owned());
        } else if skip_quick_pick {
            // Reset this tab's color
            self.shell_launch_config.borrow().borrow_mut().color = Some(String::new());
            self.on_icon_changed.fire(IconChangedEvent { instance: self.as_instance(), user_initiated: true });
            return None;
        }
        None
    }

    /// `setOverrideDimensions` (o `_resize` fica fora, ver o cabeçalho).
    pub fn set_override_dimensions(&self, dimensions: Option<ITerminalDimensionsOverride>, _immediate: bool) {
        if let Some(current) = self.dimensions_override.get()
            && current.force_exact_size.unwrap_or(false)
            && dimensions.is_none()
            && self.rows.get() == 0
            && self.cols.get() == 0
        {
            // this terminal never had a real size => keep the last dimensions override exact size
            self.cols.set(current.cols);
            self.rows.set(current.rows);
        }
        self.dimensions_override.set(dimensions);
    }

    /// `_setResolvedShellLaunchConfig`.
    fn set_resolved_shell_launch_config(&self, shell_launch_config: &IShellLaunchConfig) {
        let slc = self.shell_launch_config.borrow().clone();
        let mut slc = slc.borrow_mut();
        slc.args = shell_launch_config.args.clone();
        slc.cwd = shell_launch_config.cwd.clone();
        slc.executable = shell_launch_config.executable.clone();
        slc.env = shell_launch_config.env.clone();
    }

    // --- variáveis de ambiente, cwd e propriedades ---

    /// `_onEnvironmentVariableInfoChanged` (o `aria-label` fica fora, ver o cabeçalho).
    fn on_environment_variable_info_changed(&self, info: Rc<dyn IEnvironmentVariableInfo>) {
        self.refresh_environment_variable_info_widget_state(Some(info));
    }

    /// `_refreshEnvironmentVariableInfoWidgetState`.
    fn refresh_environment_variable_info_widget_state(&self, info: Option<Rc<dyn IEnvironmentVariableInfo>>) {
        let this = self.rc();
        runtime::spawn_local(async move {
            // Check if the status should exist
            let Some(info) = info else {
                this.status_list.remove_id(TerminalStatus::RelaunchNeeded.as_str());
                this.status_list.remove_id(TerminalStatus::EnvironmentVariableInfoChangesActive.as_str());
                return;
            };

            // Recreate the process seamlessly without informing the use if the following conditions are met.
            if info.requires_action() && this.should_relaunch_for_environment_change().await {
                this.relaunch();
                return;
            }
            // Re-create statuses
            let cwd = this.shell_launch_config.borrow().borrow().cwd.clone();
            let workspace_folder = {
                let resource = cwd.as_ref().map(|cwd| match cwd {
                    ShellCwd::Path(path) => ShellResource::Path(path.as_str()),
                    ShellCwd::Uri(uri) => ShellResource::Uri(uri),
                });
                crate::contrib::terminal::common::terminal_environment::get_workspace_for_terminal(
                    resource,
                    &*this.workspace_context_service,
                    &HistoryRoots(this.history_service.clone()),
                )
            };
            let scope = je_platform::terminal::environment_variable::EnvironmentVariableScope { workspace_folder };
            this.status_list.add(Rc::new(info.get_status(Some(&scope))), None);
        });
    }

    /// As condições de `_refreshEnvironmentVariableInfoWidgetState` para recriar o processo sem avisar o usuário.
    async fn should_relaunch_for_environment_change(&self) -> bool {
        let (is_feature_terminal, has_custom_pty, is_extension_owned, has_attach) = {
            let slc = self.shell_launch_config.borrow().clone();
            let slc = slc.borrow();
            (
                slc.is_feature_terminal.unwrap_or(false),
                slc.custom_pty_implementation.is_some(),
                slc.is_extension_owned_terminal.unwrap_or(false),
                slc.attach_persistent_process.is_some(),
            )
        };
        let reconnecting_task = self.reconnection_properties().is_some() && self.configuration_service.get_value(Some("task.reconnection"), None).strict_equals(&Dynamic::Bool(true));
        // The feature is enabled
        if !self.terminal_configuration_service.config().get_str("environmentChangesRelaunch").is_truthy()
            // Has not been interacted with
            || self.process_manager.has_written_data()
            // Not a feature terminal or is a reconnecting task terminal
            || !(!is_feature_terminal || reconnecting_task)
            // Not a custom pty
            || has_custom_pty
            // Not an extension owned terminal
            || is_extension_owned
            // Not a reconnected or revived terminal
            || has_attach
        {
            return false;
        }
        // Not a Windows remote using ConPTY which cannot relaunch (#187084). ConPTY is used on Windows builds 18309+.
        if self.process_manager.remote_authority().is_some_and(|authority| !authority.is_empty())
            && let Ok(OperatingSystem::Windows) = self.process_manager.get_backend_os().await
            && self.process_manager.process_traits().and_then(|traits| traits.windows_pty).is_some_and(|windows_pty| windows_pty.build_number >= 18309)
        {
            return false;
        }
        true
    }

    /// `updateConfig`.
    pub fn update_config(&self) {
        self.refresh_environment_variable_info_widget_state(self.process_manager.environment_variable_info());
    }

    /// `_updateUnicodeVersion`.
    fn update_unicode_version(&self) {
        let version = UnicodeVersion::from_config_text(&je_platform::js::to_js_string(&self.terminal_configuration_service.config().get_str("unicodeVersion")).to_string_lossy());
        let _ = self.process_manager.set_unicode_version(version);
    }

    /// `getInitialCwd`.
    pub fn get_initial_cwd(&self) -> Promise<String> {
        if self.initial_cwd.borrow().as_deref().is_none_or(str::is_empty) {
            *self.initial_cwd.borrow_mut() = Some(self.process_manager.initial_cwd());
        }
        Promise::resolved(self.initial_cwd().unwrap_or_default())
    }

    /// `getSpeculativeCwd`.
    pub fn get_speculative_cwd(&self) -> Promise<String> {
        if let Some(cwd_detection) = self.capabilities.get::<CwdDetection>() {
            return Promise::resolved(cwd_detection.get_cwd());
        }
        if let Some(naive_cwd_detection) = self.capabilities.get::<NaiveCwdDetection>() {
            return naive_cwd_detection.get_cwd();
        }
        Promise::resolved(self.process_manager.initial_cwd())
    }

    /// `getCwdResource`.
    pub fn get_cwd_resource(&self) -> Promise<Option<URI>> {
        get_cwd_resource(
            &*self.capabilities,
            self.remote_authority().as_deref(),
            Rc::new(InstanceCwdServices { path_service: self.path_service.clone(), file_service: self.file_service.clone() }),
        )
    }

    /// `_refreshProperty`.
    pub fn refresh_property(&self, property_type: ProcessPropertyType) -> Promise<IProcessProperty> {
        let this = self.rc();
        run_async(async move {
            this.process_ready().await?;
            this.process_manager.refresh_property(property_type).await
        })
    }

    /// `_updateProperty`.
    pub fn update_property(&self, property: IProcessProperty) -> Promise<()> {
        self.process_manager.update_property(property)
    }

    /// `_updateProcessCwd` (with `@debounce(2000)`).
    pub fn update_process_cwd(&self) {
        self.update_process_cwd_debounce.call(());
    }

    fn do_update_process_cwd(&self) {
        if self.is_disposed() || self.shell_launch_config.borrow().borrow().custom_pty_implementation.is_some() {
            return;
        }
        // reset cwd if it has changed, so file based url paths can be resolved
        let this = self.rc();
        runtime::spawn_local(async move {
            match this.refresh_property(ProcessPropertyType::Cwd).await {
                Ok(IProcessProperty::Cwd(_)) => {}
                Ok(_) => errors::on_unexpected_error(&Error::new("cwd is not a string")),
                Err(error) => {
                    // Swallow this as it means the process has been killed
                    if error.message != "Cannot refresh property when process is not set" {
                        errors::on_unexpected_error(&error);
                    }
                }
            }
        });
    }

    /// Escreve os campos privados `_cwd` e `_userHome`, que o teste do original altera por colchete.
    pub fn override_cwd_and_user_home(&self, cwd: Option<&str>, user_home: Option<&str>) {
        *self.cwd.borrow_mut() = cwd.map(str::to_owned);
        *self.user_home.borrow_mut() = user_home.map(str::to_owned);
    }
}

/// O `IHistoryService` real como o contrato reduzido do `terminalEnvironment` (`getWorkspaceForTerminal`).
struct HistoryRoots(Rc<dyn IHistoryService>);

impl crate::contrib::terminal::common::terminal_environment::IHistoryService for HistoryRoots {
    fn get_last_active_workspace_root(&self, scheme_filter: Option<&str>, authority_filter: Option<&str>) -> Option<URI> {
        self.0.get_last_active_workspace_root(scheme_filter, authority_filter)
    }
}

/// `title.match(/^[a-zA-Z]:\\.+\.[a-zA-Z]{1,3}/)`: um caminho absoluto do Windows que termina numa extensão de 1 a 3 letras.
fn looks_like_windows_file_path(title: &str) -> bool {
    let mut chars = title.chars();
    let (Some(drive), Some(':'), Some('\\')) = (chars.next(), chars.next(), chars.next()) else { return false };
    if !drive.is_ascii_alphabetic() {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    // `.+\.[a-zA-Z]{1,3}`: ao menos um caractere (que não seja quebra de linha), um ponto e de uma a três letras
    (1..rest.len()).any(|dot| {
        rest[dot] == '.'
            && !rest[..dot].iter().any(|character| matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            && rest.get(dot + 1).is_some_and(|character| character.is_ascii_alphabetic())
    })
}

/// O `ev.data.matchAll(/(?<seq>\x1b\][16]33;(?:C|D(?:;\d+)?)\x07)/g)` de `_onProcessData`: os trechos antes de cada sequência
/// (cada trecho seguido da própria sequência) e o resto depois da última.
fn split_command_sequences(data: &str) -> (Vec<String>, String) {
    let mut leading_segmented_data: Vec<String> = Vec::new();
    let mut position = 0;
    let mut search_from = 0;
    while let Some(relative) = data[search_from..].find("\x1b]") {
        let start = search_from + relative;
        match command_sequence_length(&data[start..]) {
            Some(length) => {
                leading_segmented_data.push(data[position..start].to_owned());
                leading_segmented_data.push(data[start..start + length].to_owned());
                position = start + length;
                search_from = position;
            }
            None => search_from = start + 1,
        }
    }
    (leading_segmented_data, data[position..].to_owned())
}

/// O tamanho da sequência `ESC ] (1|6) 33 ; (C | D (; dígitos)?) BEL` no começo do texto, se houver uma.
fn command_sequence_length(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("\x1b]")?;
    let rest = rest.strip_prefix(['1', '6'])?.strip_prefix("33;")?;
    let prefix = text.len() - rest.len();
    if let Some(after) = rest.strip_prefix('C') {
        return after.strip_prefix('\x07').map(|_| prefix + 2);
    }
    let after = rest.strip_prefix('D')?;
    if after.starts_with('\x07') {
        return Some(prefix + 2);
    }
    let digits = after.strip_prefix(';')?;
    let digit_count = digits.chars().take_while(char::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    digits[digit_count..].starts_with('\x07').then_some(prefix + 1 + 1 + digit_count + 1)
}

impl ITerminalInstance for TerminalInstance {
    fn instance_id(&self) -> u32 {
        self.instance_id
    }

    fn title(&self) -> String {
        self.title()
    }

    fn is_disposed(&self) -> bool {
        self.is_disposed()
    }

    fn shell_launch_config(&self) -> SharedShellLaunchConfig {
        self.shell_launch_config()
    }

    fn on_will_dispose(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_will_dispose()
    }

    fn on_disposed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_disposed()
    }

    fn relaunch(&self) {
        self.relaunch();
    }

    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        self.capabilities.clone()
    }

    fn shell_type(&self) -> TerminalShellType {
        self.shell_type()
    }

    fn target(&self) -> Option<TerminalLocation> {
        self.target()
    }

    fn process_id(&self) -> Option<i32> {
        self.process_id()
    }

    fn process_ready(&self) -> Promise<()> {
        self.process_ready()
    }

    fn cols(&self) -> i32 {
        self.cols()
    }

    fn rows(&self) -> i32 {
        self.rows()
    }

    fn max_cols(&self) -> i32 {
        self.max_cols()
    }

    fn max_rows(&self) -> i32 {
        self.max_rows()
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code()
    }

    fn exit_reason(&self) -> Option<TerminalExitReason> {
        self.exit_reason()
    }

    fn initial_data_events(&self) -> Option<Vec<String>> {
        self.initial_data_events()
    }

    fn on_data(&self) -> Event<String> {
        self.on_data()
    }

    fn run_command(&self, command_line: &str, should_execute: bool) -> Promise<()> {
        self.run_command(command_line, should_execute, None, None, None)
    }

    fn send_text(&self, text: &str, should_execute: bool) -> Promise<()> {
        self.send_text(text, should_execute, None)
    }

    fn rename(&self, title: Option<&str>) {
        self.rename(title, None);
    }

    fn dispose_with_reason(&self, reason: Option<TerminalExitReason>) {
        self.dispose_with_reason(reason);
    }

    fn resource(&self) -> URI {
        self.resource().clone()
    }

    fn has_remote_authority(&self) -> bool {
        self.has_remote_authority()
    }

    fn used_shell_integration_injection(&self) -> bool {
        self.used_shell_integration_injection()
    }

    fn shell_integration_injection_failure_reason(&self) -> Option<ShellIntegrationInjectionFailureReason> {
        self.shell_integration_injection_failure_reason()
    }

    fn session_id(&self) -> String {
        self.session_id().to_owned()
    }

    fn has_child_processes(&self) -> bool {
        self.has_child_processes()
    }

    fn persistent_process_id(&self) -> Option<u32> {
        self.persistent_process_id()
    }

    fn title_source(&self) -> TitleEventSource {
        self.title_source()
    }

    fn static_title(&self) -> Option<String> {
        self.static_title()
    }

    fn icon(&self) -> Option<TerminalIcon> {
        self.icon()
    }

    fn color(&self) -> Option<String> {
        self.color()
    }

    fn on_exit_signal(&self) -> Event<()> {
        self.on_exit().signal()
    }

    fn on_title_changed(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_title_changed()
    }

    fn on_icon_changed(&self) -> Event<IconChangedEvent> {
        self.on_icon_changed()
    }

    fn on_process_id_ready(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_process_id_ready()
    }

    fn on_dimensions_changed(&self) -> Event<()> {
        self.on_dimensions_changed()
    }

    fn on_did_input_data(&self) -> Event<String> {
        self.on_did_input_data()
    }

    fn on_did_change_shell_type(&self) -> Event<TerminalShellType> {
        self.on_did_change_shell_type()
    }
}

impl IDisposable for TerminalInstance {
    fn dispose(&self) {
        self.dispose_with_reason(None);
    }
}

impl Injectable for TerminalInstance {
    type Args = TerminalInstanceArgs;
    type Deps = (
        Rc<InstantiationService>,
        Rc<dyn ITerminalConfigurationService>,
        Rc<dyn ITerminalProfileResolverService>,
        Rc<dyn IPathService>,
        Rc<dyn IFileService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn ITerminalLogService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IWorkspaceTrustRequestService>,
        Rc<dyn IHistoryService>,
    );

    fn create(
        args: TerminalInstanceArgs,
        (
            instantiation_service,
            terminal_configuration_service,
            terminal_profile_resolver_service,
            path_service,
            file_service,
            configuration_service,
            log_service,
            workbench_environment_service,
            workspace_context_service,
            workspace_trust_request_service,
            history_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let TerminalInstanceArgs { terminal_shell_type_context_key, shell_launch_config, xterm_ctor } = args;
        let mut slc = shell_launch_config;

        let is_remote_resolver_terminal = slc.remote_resolver_terminal;
        slc.remote_resolver_terminal = false;

        let instance_id = INSTANCE_ID_COUNTER.with(|counter| {
            let id = counter.get();
            counter.set(id + 1);
            id
        });
        let attach = slc.attach_persistent_process.clone();
        let fixed_rows = attach.as_ref().and_then(|process| process.fixed_dimensions).and_then(|dimensions| dimensions.rows).map(|rows| rows as i32);
        let fixed_cols = attach.as_ref().and_then(|process| process.fixed_dimensions).and_then(|dimensions| dimensions.cols).map(|cols| cols as i32);
        slc.shell_integration_environment_reporting =
            match configuration_service.get_value(Some(TerminalSettingId::ShellIntegrationEnvironmentReporting.as_str()), None) {
                Dynamic::Bool(value) => Some(value),
                _ => None,
            };

        let resource = get_terminal_uri(workspace_context_service.get_workspace().id(), f64::from(instance_id), Some(""), None);

        if let Some(attach) = &attach {
            if attach.hide_from_user == Some(true) {
                slc.hide_from_user = attach.hide_from_user;
            }
            if attach.is_feature_terminal == Some(true) {
                slc.is_feature_terminal = attach.is_feature_terminal;
            }
            if let Some(type_) = attach.type_ {
                slc.type_ = Some(type_);
            }
            if let Some(tab_actions) = &attach.tab_actions {
                slc.tab_actions = Some(tab_actions.clone());
            }
        }

        let mut workspace_folder: Option<WorkspaceFolder> = None;
        let cwd_uri = slc.cwd.as_ref().filter(|cwd| !matches!(cwd, ShellCwd::Path(path) if path.is_empty())).map(|cwd| match cwd {
            ShellCwd::Path(path) => URI::from(&UriComponents { scheme: Some("file".into()), path: Some(path.as_str().into()), ..UriComponents::default() }, false),
            ShellCwd::Uri(uri) => uri.clone(),
        });
        if let Some(cwd_uri) = cwd_uri {
            workspace_folder = workspace_context_service.get_workspace_folder(&cwd_uri);
        }
        if workspace_folder.is_none() {
            let active_workspace_root_uri = history_service.get_last_active_workspace_root(None, None);
            workspace_folder = active_workspace_root_uri.and_then(|uri| workspace_context_service.get_workspace_folder(&uri));
        }

        // Resolve just the icon ahead of time so that it shows up immediately in the tabs. This is disabled in remote because this
        // needs to be sync and the OS may differ on the remote which would result in the wrong profile being selected and the wrong
        // icon being permanently attached to the terminal. This also doesn't work when the default profile setting is set to null,
        // that's handled after the process is created.
        if slc.executable.as_deref().is_none_or(str::is_empty) && workbench_environment_service.remote_authority().is_none_or(|authority| authority.is_empty()) {
            terminal_profile_resolver_service.resolve_icon(&mut slc, Platform::detect().os);
        }
        let icon = attach.as_ref().and_then(|process| process.icon.clone()).or_else(|| slc.icon.clone());

        let status_list = instantiation_service.create_instance::<TerminalStatusList>(());
        let deserialized_collections = attach
            .as_ref()
            .and_then(|process| process.environment_variable_collections.clone())
            .map(deserialize_environment_variable_collections);
        let shell_integration_nonce = slc.shell_integration_nonce.clone().or_else(|| attach.as_ref().map(|process| process.shell_integration_nonce.clone()));
        let process_manager: Rc<dyn ITerminalProcessManager> = instantiation_service.create_instance::<TerminalProcessManager>(TerminalProcessManagerArgs {
            instance_id,
            cwd: slc.cwd.clone(),
            environment_variable_collections: deserialized_collections,
            shell_integration_nonce,
        });
        let capabilities = TerminalCapabilityStoreMultiplexer::new();
        capabilities.add(process_manager.capabilities());

        let store = DisposableStore::new();
        store.add_rc(capabilities.clone());
        store.add_rc(status_list.clone());
        let initial_data_events_listener: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        store.add_rc(initial_data_events_listener.clone());
        let message_title_disposable: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        store.add_rc(message_title_disposable.clone());

        let this = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            this: weak.clone(),
            store: store.clone(),
            instance_id,
            resource,
            process_manager,
            xterm_ready_promise: RefCell::new(Promise::resolved(None)),
            xterm: RefCell::new(None),
            xterm_ctor: RefCell::new(Some(xterm_ctor)),
            shell_integration: RefCell::new(None),
            terminal_shell_type_context_key,
            latest_xterm_write_data: Cell::new(0),
            latest_xterm_parse_data: Cell::new(0),
            is_exiting: Cell::new(false),
            is_disposing: Cell::new(false),
            exit_code: Cell::new(None),
            exit_reason: Cell::new(None),
            shell_type: Cell::new(None),
            agent_shell_type_from_sequence: Cell::new(None),
            title: RefCell::new(String::new()),
            title_source: Cell::new(TitleEventSource::Process),
            cols: Cell::new(0),
            rows: Cell::new(0),
            fixed_cols: Cell::new(fixed_cols),
            fixed_rows: Cell::new(fixed_rows),
            cwd: RefCell::new(None),
            initial_cwd: RefCell::new(None),
            injected_args: RefCell::new(None),
            dimensions_override: Cell::new(None),
            initial_data_events_listener,
            initial_data_events: RefCell::new(Some(Vec::new())),
            container_ready_barrier: AutoOpenBarrier::new(WAIT_FOR_CONTAINER_THRESHOLD),
            icon: RefCell::new(icon),
            message_title_disposable,
            description: RefCell::new(None),
            process_name: RefCell::new(String::new()),
            sequence: RefCell::new(None),
            static_title: RefCell::new(None),
            workspace_folder: RefCell::new(workspace_folder),
            label_computer: RefCell::new(None),
            user_home: RefCell::new(None),
            used_shell_integration_injection: Cell::new(false),
            shell_integration_injection_info: Cell::new(None),
            is_visible: Cell::new(false),
            target: Cell::new(None),
            shell_launch_config: RefCell::new(Rc::new(RefCell::new(slc))),
            is_remote_resolver_terminal,
            session_id: generate_uuid(),
            capabilities,
            status_list,
            relaunch_debounce: Debounce::without_reducer(1000, {
                let weak = weak.clone();
                move |_: ()| {
                    if let Some(this) = weak.upgrade() {
                        this.do_relaunch();
                    }
                }
            }),
            refresh_shell_integration_info_status_debounce: Debounce::without_reducer(500, {
                let weak = weak.clone();
                move |_: ()| {
                    if let Some(this) = weak.upgrade() {
                        this.do_refresh_shell_integration_info_status();
                    }
                }
            }),
            update_process_cwd_debounce: Debounce::without_reducer(2000, {
                let weak = weak.clone();
                move |_: ()| {
                    if let Some(this) = weak.upgrade() {
                        this.do_update_process_cwd();
                    }
                }
            }),
            on_exit: Emitter::new(),
            on_will_dispose: store.add_rc(Emitter::new()),
            on_disposed: store.add_rc(Emitter::new()),
            on_process_id_ready: store.add_rc(Emitter::new()),
            on_process_replay_complete: store.add_rc(Emitter::new()),
            on_title_changed: store.add_rc(Emitter::new()),
            on_icon_changed: store.add_rc(Emitter::new()),
            on_will_data: store.add_rc(Emitter::new()),
            on_data: store.add_rc(Emitter::new()),
            on_dimensions_changed: store.add_rc(Emitter::new()),
            on_did_input_data: store.add_rc(Emitter::new()),
            on_did_change_has_child_processes: store.add_rc(Emitter::new()),
            on_did_execute_text: store.add_rc(Emitter::new()),
            on_did_change_target: store.add_rc(Emitter::new()),
            on_did_send_text: store.add_rc(Emitter::new()),
            on_did_change_shell_type: store.add_rc(Emitter::new()),
            on_did_change_visibility: store.add_rc(Emitter::new()),
            instantiation_service,
            terminal_configuration_service,
            terminal_profile_resolver_service,
            path_service,
            file_service,
            configuration_service,
            log_service,
            workbench_environment_service,
            workspace_context_service,
            workspace_trust_request_service,
            history_service,
        });

        this.log_service.trace(&format!("terminalInstance#ctor (instanceId: {})", this.instance_id));
        Self::wire_capability_listeners(&this);

        // When a custom pty is used set the name immediately so it gets passed over to the exthost and is available when
        // Pseudoterminal.open fires.
        let (has_custom_pty, has_title_template, name) = {
            let slc = this.shell_launch_config();
            let slc = slc.borrow();
            (slc.custom_pty_implementation.is_some(), slc.title_template.is_some(), slc.name.clone())
        };
        if has_custom_pty && !has_title_template {
            this.set_title(name.as_deref(), TitleEventSource::Api);
        }
        this.init_dimensions();
        Self::wire_process_manager(&this);

        let ready = this.create_xterm();
        *this.xterm_ready_promise.borrow_mut() = ready.clone();
        let weak = this.this.clone();
        let _ = ready
            .then_promise(move |_| {
                let Some(this) = weak.upgrade() else { return Promise::resolved(()) };
                // A promessa da barreira não referencia a instância; a espera segura só uma referência fraca, senão a instância
                // (dona da barreira) ficaria retida por esta tarefa pendente até a barreira abrir.
                let container_ready = this.container_ready_barrier.wait();
                let weak_task = Rc::downgrade(&this);
                drop(this);
                run_async(async move {
                    // Wait for a period to allow a container to be ready
                    container_ready.await?;
                    let Some(task) = weak_task.upgrade() else { return Ok(()) };

                    // Resolve the executable ahead of time if shell integration is enabled, this should not be done for custom PTYs
                    // as that would cause extension Pseudoterminal-based terminals to hang in resolver extensions
                    let mut os: Option<OperatingSystem> = None;
                    let slc = task.shell_launch_config();
                    let (has_custom_pty, has_executable) = {
                        let slc = slc.borrow();
                        (slc.custom_pty_implementation.is_some(), slc.executable.as_deref().is_some_and(|executable| !executable.is_empty()))
                    };
                    if !has_custom_pty && task.terminal_configuration_service.config().get_str("shellIntegration").get_str("enabled").is_truthy() && !has_executable {
                        let backend_os = task.process_manager.get_backend_os().await?;
                        os = Some(backend_os);
                        let remote_authority = task.remote_authority();
                        let default_profile = task
                            .terminal_profile_resolver_service
                            .get_default_profile(IShellLaunchConfigResolveOptions {
                                remote_authority: remote_authority.as_deref(),
                                os: backend_os,
                                allow_automation_shell: None,
                                allow_agent_host_shell: None,
                            })
                            .await?;
                        let mut slc = slc.borrow_mut();
                        slc.executable = Some(default_profile.path.clone());
                        slc.args = default_profile.args.clone();
                        // Only use default icon and color and env if they are undefined in the SLC
                        if slc.icon.is_none() {
                            slc.icon = default_profile.icon.clone();
                        }
                        if slc.color.is_none() {
                            slc.color = default_profile.color.clone();
                        }
                        if slc.env.is_none() {
                            slc.env = default_profile.env.clone();
                        }
                    }

                    // Resolve the shell type ahead of time to allow features that depend upon it to work before the process is
                    // actually created (like terminal suggest manual request)
                    let executable = slc.borrow().executable.clone().filter(|executable| !executable.is_empty());
                    if let (Some(os), Some(executable)) = (os, executable) {
                        task.set_shell_type(guess_shell_type_from_executable(os, &executable));
                    }

                    task.create_process().await?;

                    // Re-establish the title after reconnect
                    let attach = slc.borrow().attach_persistent_process.clone();
                    if let Some(attach) = attach {
                        *task.cwd.borrow_mut() = Some(attach.cwd.clone());
                        task.set_title(Some(&attach.title), attach.title_source);
                        task.set_shell_type(task.shell_type.get());
                    }
                    Ok(())
                })
            })
            .catch({
                let weak = this.this.clone();
                move |error| {
                    // Ignore exceptions if the terminal is already disposed
                    if let Some(this) = weak.upgrade()
                        && !this.is_disposed()
                    {
                        errors::on_unexpected_error(&error);
                    }
                }
            });

        // Configuration listeners
        this.store.add(this.configuration_service.on_did_change_configuration().listen(on_weak(this.this.clone(), |this: &Rc<Self>, event: &Rc<je_platform::configuration::models::ConfigurationChangeEvent>| {
            if event.affects_configuration("terminal.integrated", None) {
                this.update_config();
                this.set_visible(this.is_visible.get());
            }
            if event.affects_configuration(TerminalSettingId::UnicodeVersion.as_str(), None) {
                this.update_unicode_version();
            }
            if event.affects_configuration(TerminalSettingId::TerminalTitle.as_str(), None)
                || event.affects_configuration(TerminalSettingId::TerminalTitleSeparator.as_str(), None)
                || event.affects_configuration(TerminalSettingId::TerminalDescription.as_str(), None)
            {
                this.refresh_label(false);
            }
        })));
        this.store.add(this.workspace_context_service.on_did_change_workspace_folders().listen(on_weak(this.this.clone(), |this: &Rc<Self>, _| {
            this.refresh_label(false);
        })));

        // Clear out initial data events after 10 seconds, hopefully extension hosts are up and running at that point.
        let weak = this.this.clone();
        let initial_data_events_timeout = runtime::set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    *this.initial_data_events.borrow_mut() = None;
                    this.initial_data_events_listener.clear();
                }
            },
            10000,
        );
        this.store.add(to_disposable(move || runtime::clear_timeout(initial_data_events_timeout)));

        this
    }
}

