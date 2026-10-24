//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm/decorationStyles.ts` (com os quatro ícones de decoração de
//! `terminalIcons.ts`): as classes das decorações, o texto do tooltip, o estado visual de cada comando e o tamanho do ícone
//! em função da fonte.
//!
//! Decisões de tradução:
//! - **`const enum DecorationSelector`.** É o enum [`DecorationSelector`] com `as_str()` devolvendo o valor do original.
//! - **`registerIcon(id, ...)`.** O ícone registrado é o `ThemeIcon` com o `id` registrado (`terminal-decoration-mark` etc.),
//!   que é o que o original devolve; o registro de ícones e o ícone padrão do Codicon (`circleSmallFilled`, `circle`,
//!   `errorSmall` e `circleFilled`) só servem ao seletor de ícones, e a classe CSS vem do `id`.
//! - **`isNumber(command.duration)`.** O `duration` do [`IBaseTerminalCommand`] é `f64`, e o `undefined` do original é `NaN`
//!   (que o `isNumber` também recusa); [`command_duration`] faz a ponte. O `exitCode` é `Option<f64>`.
//! - **`command.exitCode` como condição.** É `Some(n)` com `n` diferente de zero (e de `NaN`), a veracidade do JavaScript.
//! - **`localize(..., args)`.** Os argumentos numéricos são [`NlsArg::Number`]; a mensagem `terminalPromptCommandSuccess`
//!   sem argumento fica com o `{0}` intacto, como o `localize` do original sem argumentos.
//! - **`new Date(startTimestamp).toLocaleString()`.** É o formato `M/D/AAAA, h:mm:ss AM` da localidade `en-US` no fuso do
//!   sistema (o porte ainda não tem o `Intl.DateTimeFormat`).
//! - **`element.style`.** As quatro propriedades escritas são [`DecorationStyleProperty`], e o elemento é o
//!   [`IDecorationElement`].

use je_base::date::{from_now, get_duration_string, local_time_zone};
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::nls::{NlsArg, localize, localize_args};
use je_base::runtime;
use je_base::themables::ThemeIcon;
use je_base::types::is_number;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::terminal_settings::TerminalSettingId;

use super::xterm_decoration::{DecorationStyleProperty, IDecorationElement};

/// `DecorationStyles.DefaultDimension`.
const DEFAULT_DIMENSION: f64 = 16.0;

/// `DecorationStyles.MarginLeft`.
const MARGIN_LEFT: f64 = -17.0;

/// `DecorationSelector`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationSelector {
    CommandDecoration,
    Hide,
    ErrorColor,
    DefaultColor,
    Default,
    Codicon,
    XtermDecoration,
    OverviewRuler,
}

impl DecorationSelector {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommandDecoration => "terminal-command-decoration",
            Self::Hide => "hide",
            Self::ErrorColor => "error",
            Self::DefaultColor => "default-color",
            Self::Default => "default",
            Self::Codicon => "codicon",
            Self::XtermDecoration => "xterm-decoration",
            Self::OverviewRuler => ".xterm-decoration-overview-ruler",
        }
    }
}

/// `terminalDecorationMark`.
pub fn terminal_decoration_mark() -> ThemeIcon {
    ThemeIcon::from_id("terminal-decoration-mark")
}

/// `terminalDecorationIncomplete`.
pub fn terminal_decoration_incomplete() -> ThemeIcon {
    ThemeIcon::from_id("terminal-decoration-incomplete")
}

/// `terminalDecorationError`.
pub fn terminal_decoration_error() -> ThemeIcon {
    ThemeIcon::from_id("terminal-decoration-error")
}

/// `terminalDecorationSuccess`.
pub fn terminal_decoration_success() -> ThemeIcon {
    ThemeIcon::from_id("terminal-decoration-success")
}

/// O `command.duration` como número do JavaScript: `None` quando o original é `undefined` (aqui, `NaN`).
pub fn command_duration(command: &dyn ITerminalCommand) -> Option<f64> {
    Some(command.duration()).filter(|duration| !duration.is_nan())
}

/// `!!command.exitCode`.
fn is_failure_code(exit_code: Option<f64>) -> bool {
    exit_code.is_some_and(|exit_code| exit_code != 0.0 && !exit_code.is_nan())
}

pub fn get_terminal_decoration_hover_content(command: Option<&dyn ITerminalCommand>, hover_message: Option<&str>, show_command_actions: Option<bool>) -> String {
    let hover_message = hover_message.filter(|hover_message| !hover_message.is_empty());
    let mut hover_content = if show_command_actions.unwrap_or(false) {
        format!("{}\n\n---\n\n", localize("terminalPromptContextMenu", "Show Command Actions", &[]))
    } else {
        String::new()
    };
    match command {
        None => match hover_message {
            Some(hover_message) => hover_content = hover_message.to_owned(),
            None => return String::new(),
        },
        Some(command) if command.mark_properties().is_some() || hover_message.is_some() => {
            let mark_message = command.mark_properties().and_then(|properties| properties.hover_message).filter(|message| !message.is_empty());
            match mark_message.as_deref().or(hover_message) {
                Some(message) => hover_content = message.to_owned(),
                None => return String::new(),
            }
        }
        Some(command) => {
            let from_now_text = from_now(Some(command.timestamp()), true, false, false);
            let exit_code = command.exit_code();
            if let Some(duration) = command_duration(command) {
                let duration_text = get_duration_string(duration, false);
                if is_failure_code(exit_code) {
                    if exit_code == Some(-1.0) {
                        hover_content += &localize(
                            "terminalPromptCommandFailed.duration",
                            "Command executed {0}, took {1} and failed",
                            &[&from_now_text, &duration_text],
                        );
                    } else {
                        hover_content += &localize_args(
                            "terminalPromptCommandFailedWithExitCode.duration",
                            "Command executed {0}, took {1} and failed (Exit Code {2})",
                            &[NlsArg::Str(from_now_text), NlsArg::Str(duration_text), NlsArg::Number(exit_code.unwrap_or_default())],
                        );
                    }
                } else {
                    hover_content += &localize("terminalPromptCommandSuccess.duration", "Command executed {0} and took {1}", &[&from_now_text, &duration_text]);
                }
            } else if is_failure_code(exit_code) {
                if exit_code == Some(-1.0) {
                    hover_content += &localize("terminalPromptCommandFailed", "Command executed {0} and failed", &[&from_now_text]);
                } else {
                    hover_content += &localize_args(
                        "terminalPromptCommandFailedWithExitCode",
                        "Command executed {0} and failed (Exit Code {1})",
                        &[NlsArg::Str(from_now_text), NlsArg::Number(exit_code.unwrap_or_default())],
                    );
                }
            } else {
                hover_content += &localize("terminalPromptCommandSuccess", "Command executed {0} now", &[]);
            }
        }
    }
    hover_content
}

/// `ITerminalCommandDecorationPersistedState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ITerminalCommandDecorationPersistedState {
    pub exit_code: Option<f64>,
    pub timestamp: Option<f64>,
    pub duration: Option<f64>,
}

/// `TerminalCommandDecorationStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCommandDecorationStatus {
    Unknown,
    Running,
    Success,
    Error,
}

impl TerminalCommandDecorationStatus {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Running => "running",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

/// `ITerminalCommandDecorationState`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITerminalCommandDecorationState {
    pub status: TerminalCommandDecorationStatus,
    pub icon: ThemeIcon,
    pub class_names: Vec<String>,
    pub exit_code: Option<f64>,
    pub exit_code_text: String,
    pub start_timestamp: Option<f64>,
    pub start_text: String,
    pub duration: Option<f64>,
    pub duration_text: String,
    pub hover_message: String,
}

pub fn get_terminal_command_decoration_tooltip(command: Option<&dyn ITerminalCommand>, stored_state: Option<&ITerminalCommandDecorationPersistedState>) -> String {
    if let Some(command) = command {
        return get_terminal_decoration_hover_content(Some(command), None, None);
    }
    let Some(stored_state) = stored_state else { return String::new() };
    let Some(timestamp) = stored_state.timestamp.filter(|timestamp| !timestamp.is_nan()) else { return String::new() };
    let exit_code = stored_state.exit_code;
    let from_now_text = from_now(Some(timestamp), true, false, false);
    let mut hover_content = String::new();
    if let Some(duration) = stored_state.duration.filter(|duration| !duration.is_nan()) {
        let duration_text = get_duration_string(duration.max(0.0), false);
        if is_failure_code(exit_code) {
            if exit_code == Some(-1.0) {
                hover_content += &localize("terminalPromptCommandFailed.duration", "Command executed {0}, took {1} and failed", &[&from_now_text, &duration_text]);
            } else {
                hover_content += &localize_args(
                    "terminalPromptCommandFailedWithExitCode.duration",
                    "Command executed {0}, took {1} and failed (Exit Code {2})",
                    &[NlsArg::Str(from_now_text), NlsArg::Str(duration_text), NlsArg::Number(exit_code.unwrap_or_default())],
                );
            }
        } else {
            hover_content += &localize("terminalPromptCommandSuccess.duration", "Command executed {0} and took {1}", &[&from_now_text, &duration_text]);
        }
    } else if is_failure_code(exit_code) {
        if exit_code == Some(-1.0) {
            hover_content += &localize("terminalPromptCommandFailed", "Command executed {0} and failed", &[&from_now_text]);
        } else {
            hover_content += &localize_args(
                "terminalPromptCommandFailedWithExitCode",
                "Command executed {0} and failed (Exit Code {1})",
                &[NlsArg::Str(from_now_text), NlsArg::Number(exit_code.unwrap_or_default())],
            );
        }
    } else {
        hover_content += &localize("terminalPromptCommandSuccess.", "Command executed {0} ", &[&from_now_text]);
    }
    hover_content
}

/// `getTerminalCommandDecorationState(command, storedState?, now = Date.now())`.
pub fn get_terminal_command_decoration_state(
    command: Option<&dyn ITerminalCommand>,
    stored_state: Option<&ITerminalCommandDecorationPersistedState>,
    now: Option<f64>,
) -> ITerminalCommandDecorationState {
    let now = now.unwrap_or_else(|| runtime::date_now() as f64);
    let unknown_text = localize("terminalCommandDecoration.unknown", "Unknown", &[]);
    let running_text = localize("terminalCommandDecoration.running", "Running", &[]);
    let mut status = TerminalCommandDecorationStatus::Unknown;
    let exit_code = command.and_then(|command| command.exit_code()).or_else(|| stored_state.and_then(|state| state.exit_code));
    let mut exit_code_text = unknown_text.clone();
    let start_timestamp = command.map(|command| command.timestamp()).or_else(|| stored_state.and_then(|state| state.timestamp));
    let mut start_text = unknown_text.clone();
    let mut duration_ms: Option<f64> = None;
    let mut duration_text = unknown_text;

    if let Some(start_timestamp) = start_timestamp.filter(|timestamp| !timestamp.is_nan()) {
        start_text = to_locale_string(start_timestamp);
    }
    let since_start = || start_timestamp.map(|start_timestamp| (now - start_timestamp).max(0.0));

    if let Some(command) = command {
        match command.exit_code() {
            None => {
                status = TerminalCommandDecorationStatus::Running;
                exit_code_text = running_text;
                duration_ms = since_start();
            }
            Some(code) if code != 0.0 => {
                status = TerminalCommandDecorationStatus::Error;
                exit_code_text = number_to_js_string(code);
                duration_ms = command_duration(command).or_else(since_start);
            }
            Some(code) => {
                status = TerminalCommandDecorationStatus::Success;
                exit_code_text = number_to_js_string(code);
                duration_ms = command_duration(command).or_else(since_start);
            }
        }
    } else if let Some(stored_state) = stored_state {
        match stored_state.exit_code {
            None => {
                status = TerminalCommandDecorationStatus::Running;
                exit_code_text = running_text;
                duration_ms = since_start();
            }
            Some(code) if code != 0.0 => {
                status = TerminalCommandDecorationStatus::Error;
                exit_code_text = number_to_js_string(code);
                duration_ms = stored_state.duration;
            }
            Some(code) => {
                status = TerminalCommandDecorationStatus::Success;
                exit_code_text = number_to_js_string(code);
                duration_ms = stored_state.duration;
            }
        }
    }

    if let Some(duration_ms) = duration_ms.filter(|duration_ms| is_number(&Dynamic::Number(*duration_ms))) {
        duration_text = get_duration_string(duration_ms.max(0.0), false);
    }

    let mut class_names: Vec<String> = Vec::new();
    let icon = match status {
        TerminalCommandDecorationStatus::Running | TerminalCommandDecorationStatus::Unknown => {
            class_names.push(DecorationSelector::DefaultColor.as_str().to_owned());
            class_names.push(DecorationSelector::Default.as_str().to_owned());
            terminal_decoration_incomplete()
        }
        TerminalCommandDecorationStatus::Error => {
            class_names.push(DecorationSelector::ErrorColor.as_str().to_owned());
            terminal_decoration_error()
        }
        TerminalCommandDecorationStatus::Success => {
            class_names.push("success".to_owned());
            terminal_decoration_success()
        }
    };

    let hover_message = get_terminal_command_decoration_tooltip(command, stored_state);

    ITerminalCommandDecorationState {
        status,
        icon,
        class_names,
        exit_code,
        exit_code_text,
        start_timestamp,
        start_text,
        duration: duration_ms,
        duration_text,
        hover_message,
    }
}

/// `new Date(timestamp).toLocaleString()` na localidade `en-US` e no fuso do sistema.
fn to_locale_string(timestamp: f64) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(timestamp as i64) else { return "Invalid Date".to_owned() };
    let zoned = timestamp.to_zoned(local_time_zone());
    let hour = match zoned.hour() % 12 {
        0 => 12,
        hour => hour,
    };
    let meridiem = if zoned.hour() < 12 { "AM" } else { "PM" };
    format!("{}/{}/{}, {}:{:02}:{:02} {}", zoned.month(), zoned.day(), zoned.year(), hour, zoned.minute(), zoned.second(), meridiem)
}

/// `updateLayout(configurationService, element?)`.
pub fn update_layout(configuration_service: &dyn IConfigurationService, element: Option<&dyn IDecorationElement>) {
    let Some(element) = element else { return };
    let overrides = ConfigurationOverrides::default();
    let font_size_value = configuration_service.inspect(TerminalSettingId::FontSize.as_str(), &overrides);
    let font_size = font_size_value.value();
    let default_font_size = font_size_value.default_value();
    let line_height = configuration_service.inspect(TerminalSettingId::LineHeight.as_str(), &overrides).value();
    if let (Dynamic::Number(font_size), Dynamic::Number(default_font_size), Dynamic::Number(line_height)) = (&font_size, &default_font_size, &line_height)
        && !font_size.is_nan()
        && !default_font_size.is_nan()
        && !line_height.is_nan()
    {
        let ratio = font_size / default_font_size;
        let scalar = if ratio <= 1.0 { ratio } else { 1.0 };
        // must be inlined to override the inlined styles from xterm
        element.set_style(DecorationStyleProperty::Width, &format!("{}px", number_to_js_string(scalar * DEFAULT_DIMENSION)));
        element.set_style(DecorationStyleProperty::Height, &format!("{}px", number_to_js_string(scalar * DEFAULT_DIMENSION * line_height)));
        element.set_style(DecorationStyleProperty::FontSize, &format!("{}px", number_to_js_string(scalar * DEFAULT_DIMENSION)));
        element.set_style(DecorationStyleProperty::MarginLeft, &format!("{}px", number_to_js_string(scalar * MARGIN_LEFT)));
    }
}
