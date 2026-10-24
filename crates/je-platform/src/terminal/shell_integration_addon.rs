//! Porte de `src/vs/platform/terminal/common/xterm/shellIntegrationAddon.ts`: os identificadores de OSC da
//! integração de shell (`ShellIntegrationOscPs`), o [`ShellIntegrationAddon`] (os tratadores de OSC 633, 133, 1337, 7 e 9
//! registrados no `parser` do xterm, e a criação das capacidades) e as funções puras de serialização e de leitura das
//! sequências (`deserializeVSCodeOscMessage`, `serializeVSCodeOscMessage`, `parseKeyValueAssignment`,
//! `parseMarkSequence`).
//!
//! Divergências:
//! - As mensagens são `&str`/`String`: as funções só mexem em caracteres ASCII (`\`, `;` e controles até `0x20`) e em
//!   dígitos hexadecimais, e o `String.fromCharCode(parseInt(hex, 16))` de `0x00` a `0xff` é o `char` de mesmo código.
//! - `{ id?: string; hidden?: boolean }` é [`MarkSequence`] (`id` opcional, `hidden` sempre definido, como o retorno do
//!   original), e `{ key, value: string | undefined }` é [`KeyValueAssignment`].
//! - `(string | undefined)[]` é uma fatia de `Option<&str>`.
//! - A classe que estende `Disposable` e implementa `IShellIntegration` e `ITerminalAddon` é a struct
//!   [`ShellIntegrationAddon`] com um [`Disposable`] por composição. `loadAddon(addon)` do xterm é
//!   [`ShellIntegrationAddon::activate`]; os tratadores de OSC guardam só uma referência fraca ao addon. Os métodos
//!   `protected` (`_createOrGet*`) são públicos, para os testes trocarem as capacidades do store como a subclasse
//!   `TestShellIntegrationAddon` do original faz.
//! - `capabilities` é um [`TerminalCapabilityStore`] concreto ([`ShellIntegrationAddon::capabilities`]); o
//!   [`IShellIntegration`] devolve o mesmo store como `ITerminalCapabilityStore`. `seenSequences` (um `Set`) é a lista de
//!   textos na ordem de inserção, e os eventos disparam um retrato dela.
//! - `parseInt(args[0])` é [`parse_int`] (`NaN` quando não há dígito). `JSON.parse` é o [`json_parse`] estrito do je-base, e o
//!   objeto resultante vira o dicionário do ambiente (`null` é `None`; um valor que não é objeto vale um ambiente vazio,
//!   o que o `Object.entries` do original também dá para um número ou um booleano).
//! - `value.replace(/\x1b\[[0-9;]*m/g, '')` é [`strip_sgr_sequences`].
//! - O `switch` do `Property` do original cai no `SetMark` quando a chave da propriedade não é conhecida (não há `break`
//!   depois do `switch` interno): o porte repete a queda. O mesmo vale para o `SetMark` de `OSC 1337`, que cai no
//!   `default` e devolve `true`.
//! - `_ensureCapabilitiesOrAddFailureTelemetry` é uma função `async` sem `await`; o porte a executa direto. O
//!   `setTimeout` de 10 s é o `runtime::set_timeout`.
//! - `deserialize` sem o addon ativado lança `Error('Cannot restore commands before addon is activated')`; aqui é o
//!   [`errors::throw`].
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::{self, JsString};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::runtime::{self, TimerId};
use je_base::strings::remove_ansi_escape_codes_from_prompt;
use je_base::uri::URI;
use indexmap::IndexMap;

use crate::log::LogServiceRef;
use crate::telemetry::telemetry::ITelemetryService;
use crate::terminal::capabilities::buffer_mark_capability::BufferMarkCapability;
use crate::terminal::capabilities::capabilities::{
    BufferMarkDetection, CommandDetection, CwdDetection, IBufferMarkCapability, ICommandDetectionCapability, ICwdDetectionCapability, IHandleCommandOptions,
    IMarkProperties, IPromptTypeDetectionCapability, ISerializedCommandDetectionCapability, IShellEnvDetectionCapability, ITerminalCapabilityStore,
    PartialCommandDetection, PromptTypeDetection, ShellEnvDetection, TerminalCapability,
};
use crate::terminal::capabilities::command_detection_capability::CommandDetectionCapability;
use crate::terminal::capabilities::cwd_detection_capability::CwdDetectionCapability;
use crate::terminal::capabilities::partial_command_detection_capability::PartialCommandDetectionCapability;
use crate::terminal::capabilities::prompt_type_detection_capability::PromptTypeDetectionCapability;
use crate::terminal::capabilities::shell_env_detection_capability::ShellEnvDetectionCapability;
use crate::terminal::capabilities::terminal_capability_store::{TerminalCapabilityStore, TerminalCapabilityStoreExt};
use crate::terminal::terminal::{IShellIntegration, ShellIntegrationStatus};
use crate::terminal::terminal_environment::sanitize_cwd;
use crate::terminal::xterm_headless::{IHeadlessTerminal, OscHandler};

/// `ShellIntegrationOscPs`: o `Ps` de cada família de sequências OSC.
pub mod shell_integration_osc_ps {
    pub const FINAL_TERM: i32 = 133;
    pub const VS_CODE: i32 = 633;
    pub const I_TERM: i32 = 1337;
    pub const SET_CWD: i32 = 7;
    pub const SET_WINDOWS_FRIENDLY_CWD: i32 = 9;
}

/// O resultado de [`parse_key_value_assignment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValueAssignment {
    pub key: String,
    pub value: Option<String>,
}

/// O resultado de [`parse_mark_sequence`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkSequence {
    pub id: Option<String>,
    pub hidden: bool,
}

/// `deserializeVSCodeOscMessage`: desfaz o escape de `serializeVSCodeOscMessage`. Uma barra invertida seguida de outra
/// barra vira uma barra, e uma barra seguida de `x` e dois dígitos hexadecimais (em qualquer caixa) vira o caractere.
pub fn deserialize_vscode_osc_message(message: &str) -> String {
    let chars: Vec<char> = message.chars().collect();
    let mut result = String::with_capacity(message.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' {
            // Backslash ('\') followed by an escape operator: either another '\', or 'x' and two hex chars.
            match chars.get(index + 1) {
                Some('\\') => {
                    result.push('\\');
                    index += 2;
                    continue;
                }
                Some('x') => {
                    if let (Some(high), Some(low)) = (chars.get(index + 2).and_then(|c| c.to_digit(16)), chars.get(index + 3).and_then(|c| c.to_digit(16))) {
                        result.push(char::from_u32(high * 16 + low).expect("um byte é um ponto de código válido"));
                        index += 4;
                        continue;
                    }
                }
                _ => {}
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

/// `serializeVSCodeOscMessage`: escapa a barra invertida (`\\`), o ponto e vírgula e os caracteres até `0x20` (`\xAB`).
pub fn serialize_vscode_osc_message(message: &str) -> String {
    let mut result = String::with_capacity(message.len());
    for character in message.chars() {
        // Match backslash ('\'), semicolon (';'), or characters 0x20 and below
        if character == '\\' {
            result.push_str("\\\\");
        } else if character == ';' || u32::from(character) <= 0x20 {
            result.push_str(&format!("\\x{:02x}", u32::from(character)));
        } else {
            result.push(character);
        }
    }
    result
}

/// `parseKeyValueAssignment`: separa `chave=valor` no primeiro `=`; sem `=`, o valor é `None`.
pub fn parse_key_value_assignment(message: &str) -> KeyValueAssignment {
    match message.find('=') {
        None => KeyValueAssignment { key: message.to_owned(), value: None },
        Some(separator_index) => {
            KeyValueAssignment { key: message[..separator_index].to_owned(), value: Some(message[separator_index + 1..].to_owned()) }
        }
    }
}

/// `parseMarkSequence`: lê as propriedades `Id=<id>` e `Hidden` de uma sequência `SetMark`.
pub fn parse_mark_sequence(sequence: &[Option<&str>]) -> MarkSequence {
    let mut id = None;
    let mut hidden = false;
    for property in sequence {
        // Sanity check, this shouldn't happen in practice
        let Some(property) = property else { continue };
        if *property == "Hidden" {
            hidden = true;
        }
        if let Some(rest) = property.strip_prefix("Id=") {
            id = Some(rest.to_owned());
        }
    }
    MarkSequence { id, hidden }
}

/// `parseInt(text)` (base 10, ou 16 com o prefixo `0x`): `NaN` quando não há dígito.
pub fn parse_int(text: &str) -> f64 {
    let trimmed = text.trim_start_matches(|c: char| u16::try_from(u32::from(c)).is_ok_and(js_string::is_js_whitespace));
    let (negative, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (radix, digits_source) = match rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        Some(hex) => (16, hex),
        None => (10, rest),
    };
    let mut value = 0.0_f64;
    let mut found = false;
    for digit in digits_source.chars().map_while(|c| c.to_digit(radix)) {
        value = value * f64::from(radix) + f64::from(digit);
        found = true;
    }
    if !found {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

/// `value.replace(/\x1b\[[0-9;]*m/g, '')`: tira as sequências de cor (SGR) do texto.
pub fn strip_sgr_sequences(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\u{1b}' && chars.get(index + 1) == Some(&'[') {
            let mut end = index + 2;
            while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == ';') {
                end += 1;
            }
            if chars.get(end) == Some(&'m') {
                index = end + 1;
                continue;
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

/// O `JSON.parse(...)` do ambiente do shell como o dicionário que `setEnvironment` recebe (`Object.entries`).
fn json_to_environment(value: &Value) -> IndexMap<String, Option<String>> {
    let mut environment = IndexMap::new();
    if let Value::Object(object) = value {
        for (key, value) in object.iter() {
            let value = match value {
                Value::Null => None,
                Value::String(text) => Some(text.to_string_lossy()),
                Value::Bool(flag) => Some(flag.to_string()),
                Value::Number(number) => Some(number.to_string()),
                other => Some(format!("{other:?}")),
            };
            environment.insert(key.to_string_lossy(), value);
        }
    }
    environment
}

/// `/^file:\/\/.*\//.test(text)`: o prefixo `file://` e, na mesma linha (`.` não casa terminador de linha), uma barra.
fn is_file_uri_with_path(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("file://") else {
        return false;
    };
    rest.chars().take_while(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')).any(|c| c == '/')
}

/// `FinalTermOscPt`: sequências criadas pelo FinalTerm.
mod final_term_osc_pt {
    /// The start of the prompt, this is expected to always appear at the start of a line. Format: `OSC 133 ; A ST`
    pub const PROMPT_START: &str = "A";
    /// The start of a command, ie. where the user inputs their command. Format: `OSC 133 ; B ST`
    pub const COMMAND_START: &str = "B";
    /// Sent just before the command output begins. Format: `OSC 133 ; C ST`
    pub const COMMAND_EXECUTED: &str = "C";
    /// Sent just after a command has finished. The exit code is optional, when not specified it means no command
    /// was run (ie. enter on empty prompt or ctrl+c). Format: `OSC 133 ; D [; <ExitCode>] ST`
    pub const COMMAND_FINISHED: &str = "D";
}

/// `VSCodeOscPt`: sequências específicas do VS Code, algumas baseadas nas do FinalTerm. A decisão de ir para
/// sequências próprias foi tentar melhorar a confiabilidade e impedir que aplicações confundam o terminal. Se vários
/// scripts de integração rodam, o VS Code prioriza os dele.
mod vscode_osc_pt {
    /// `OSC 633 ; A ST`
    pub const PROMPT_START: &str = "A";
    /// `OSC 633 ; B ST`
    pub const COMMAND_START: &str = "B";
    /// `OSC 633 ; C ST`
    pub const COMMAND_EXECUTED: &str = "C";
    /// `OSC 633 ; D [; <ExitCode>] ST`
    pub const COMMAND_FINISHED: &str = "D";
    /// Explicitly set the command line. Format: `OSC 633 ; E [; <CommandLine> [; <Nonce>]] ST`
    pub const COMMAND_LINE: &str = "E";
    /// Similar to prompt start but for line continuations. Format: `OSC 633 ; F ST`
    pub const CONTINUATION_START: &str = "F";
    /// Similar to command start but for line continuations. Format: `OSC 633 ; G ST`
    pub const CONTINUATION_END: &str = "G";
    /// The start of the right prompt. Format: `OSC 633 ; H ST`
    pub const RIGHT_PROMPT_START: &str = "H";
    /// The end of the right prompt. Format: `OSC 633 ; I ST`
    pub const RIGHT_PROMPT_END: &str = "I";
    /// Set the value of an arbitrary property, only known properties will be handled by VS Code. Format:
    /// `OSC 633 ; P ; <Property>=<Value> ST`
    pub const PROPERTY: &str = "P";
    /// Sets a mark/point-of-interest in the buffer. Format: `OSC 633 ; SetMark [; Id=<string>] [; Hidden] ST`
    pub const SET_MARK: &str = "SetMark";
    /// Sends the shell's complete environment in JSON format. Format: `OSC 633 ; EnvJson ; <Environment> ; <Nonce> ST`
    pub const ENV_JSON: &str = "EnvJson";
    /// Delete a single environment variable from cached environment. Format:
    /// `OSC 633 ; EnvSingleDelete ; <EnvironmentKey> ; <EnvironmentValue> [; <Nonce>] ST`
    pub const ENV_SINGLE_DELETE: &str = "EnvSingleDelete";
    /// The start of the collecting user's environment variables individually. Format:
    /// `OSC 633 ; EnvSingleStart ; <Clear> [; <Nonce>] ST`
    pub const ENV_SINGLE_START: &str = "EnvSingleStart";
    /// Sets an entry of single environment variable to transactional pending map of environment variables. Format:
    /// `OSC 633 ; EnvSingleEntry ; <EnvironmentKey> ; <EnvironmentValue> [; <Nonce>] ST`
    pub const ENV_SINGLE_ENTRY: &str = "EnvSingleEntry";
    /// The end of the collecting user's environment variables individually. Format:
    /// `OSC 633 ; EnvSingleEnd [; <Nonce>] ST`
    pub const ENV_SINGLE_END: &str = "EnvSingleEnd";
}

/// `ITermOscPt`: sequências do iTerm.
mod iterm_osc_pt {
    /// Sets a mark/point-of-interest in the buffer. Format: `OSC 1337 ; SetMark ST`
    pub const SET_MARK: &str = "SetMark";
    /// Reports current working directory (CWD). Format: `OSC 1337 ; CurrentDir=<Cwd> ST`
    pub const CURRENT_DIR: &str = "CurrentDir";
}

/// O timeout, em milissegundos, até avisar que a integração com o shell não criou as capacidades.
const ACTIVATION_TIMEOUT_MS: u64 = 10_000;

/// The shell integration addon extends xterm by reading shell integration sequences and creating
/// capabilities and passing along relevant sequences to the capabilities. This is meant to
/// encapsulate all handling/parsing of sequences so the capabilities don't need to.
pub struct ShellIntegrationAddon {
    base: Rc<Disposable>,
    this: Weak<Self>,
    terminal: RefCell<Option<Rc<dyn IHeadlessTerminal>>>,
    capabilities: Rc<TerminalCapabilityStore>,
    nonce: String,
    disable_telemetry: Option<bool>,
    on_did_execute_text: Option<Event<()>>,
    telemetry_service: Option<Rc<dyn ITelemetryService>>,
    log_service: LogServiceRef,
    has_updated_telemetry: Cell<bool>,
    activation_timeout: Cell<Option<TimerId>>,
    common_protocol_disposables: RefCell<Vec<DisposableRef>>,
    seen_sequences: RefCell<Vec<String>>,
    status: Cell<ShellIntegrationStatus>,
    on_did_change_status: Rc<Emitter<ShellIntegrationStatus>>,
    on_did_change_seen_sequences: Rc<Emitter<Vec<String>>>,
}

impl ShellIntegrationAddon {
    pub fn new(
        nonce: &str,
        disable_telemetry: Option<bool>,
        on_did_execute_text: Option<Event<()>>,
        telemetry_service: Option<Rc<dyn ITelemetryService>>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let capabilities = TerminalCapabilityStore::new();
        base.register(capabilities.clone());
        let on_did_change_status: Rc<Emitter<ShellIntegrationStatus>> = Emitter::new();
        base.register(on_did_change_status.clone());
        let on_did_change_seen_sequences: Rc<Emitter<Vec<String>>> = Emitter::new();
        base.register(on_did_change_seen_sequences.clone());
        let addon = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            terminal: RefCell::new(None),
            capabilities,
            nonce: nonce.to_owned(),
            disable_telemetry,
            on_did_execute_text,
            telemetry_service,
            log_service,
            has_updated_telemetry: Cell::new(false),
            activation_timeout: Cell::new(None),
            common_protocol_disposables: RefCell::new(Vec::new()),
            seen_sequences: RefCell::new(Vec::new()),
            status: Cell::new(ShellIntegrationStatus::Off),
            on_did_change_status,
            on_did_change_seen_sequences,
        });
        let weak = addon.this.clone();
        addon.base.register(to_disposable(move || {
            if let Some(addon) = weak.upgrade() {
                addon.clear_activation_timeout();
                addon.dispose_common_protocol();
            }
        }));
        addon
    }

    /// `readonly capabilities`.
    pub fn capabilities(&self) -> Rc<TerminalCapabilityStore> {
        self.capabilities.clone()
    }

    fn terminal(&self) -> Option<Rc<dyn IHeadlessTerminal>> {
        self.terminal.borrow().clone()
    }

    fn dispose_common_protocol(&self) {
        let disposables = std::mem::take(&mut *self.common_protocol_disposables.borrow_mut());
        for disposable in &disposables {
            disposable.dispose();
        }
    }

    /// `(data: string) => boolean` do `registerOscHandler`, com a referência fraca ao addon.
    fn osc_handler(&self, handler: impl Fn(&Self, &str) -> bool + 'static) -> OscHandler {
        let weak = self.this.clone();
        Rc::new(move |data: &str| weak.upgrade().is_some_and(|this| handler(&*this, data)))
    }

    /// `activate(xterm)` do `ITerminalAddon`.
    pub fn activate(&self, xterm: Rc<dyn IHeadlessTerminal>) {
        *self.terminal.borrow_mut() = Some(xterm.clone());
        let partial_command_detection = PartialCommandDetectionCapability::new(xterm.clone(), self.on_did_execute_text.clone());
        self.base.register(partial_command_detection.clone());
        self.capabilities.add::<PartialCommandDetection>(partial_command_detection);
        let parser = xterm.parser();
        self.base.register(parser.register_osc_handler(shell_integration_osc_ps::VS_CODE, self.osc_handler(|this, data| this.handle_vscode_sequence(data))));
        self.base.register(parser.register_osc_handler(shell_integration_osc_ps::I_TERM, self.osc_handler(|this, data| this.do_handle_iterm_sequence(data))));
        self.common_protocol_disposables
            .borrow_mut()
            .push(parser.register_osc_handler(shell_integration_osc_ps::FINAL_TERM, self.osc_handler(|this, data| this.handle_final_term_sequence(data))));
        self.base.register(parser.register_osc_handler(shell_integration_osc_ps::SET_CWD, self.osc_handler(|this, data| this.do_handle_set_cwd(data))));
        self.base.register(
            parser.register_osc_handler(shell_integration_osc_ps::SET_WINDOWS_FRIENDLY_CWD, self.osc_handler(|this, data| this.do_handle_set_windows_friendly_cwd(data))),
        );
        self.ensure_capabilities_or_add_failure_telemetry();
    }

    /// `getMarkerId(terminal, vscodeMarkerId)`: o original não devolve o marcador (o método não tem `return`).
    pub fn get_marker_id(&self, terminal: &Rc<dyn IHeadlessTerminal>, vscode_marker_id: &str) {
        self.create_or_get_buffer_mark_detection(terminal).get_mark(vscode_marker_id);
    }

    pub fn set_next_command_id(&self, command: &str, command_id: &str) {
        if let Some(terminal) = self.terminal() {
            self.create_or_get_command_detection(&terminal).set_next_command_id(command, command_id);
        }
    }

    fn mark_sequence_seen(&self, sequence: &str) {
        if !self.seen_sequences.borrow().iter().any(|seen| seen == sequence) {
            self.seen_sequences.borrow_mut().push(sequence.to_owned());
            let seen_sequences = self.seen_sequences.borrow().clone();
            self.on_did_change_seen_sequences.fire(seen_sequences);
        }
    }

    fn handle_final_term_sequence(&self, data: &str) -> bool {
        let did_handle = self.do_handle_final_term_sequence(data);
        if self.status.get() == ShellIntegrationStatus::Off {
            self.status.set(ShellIntegrationStatus::FinalTerm);
            self.on_did_change_status.fire(self.status.get());
        }
        did_handle
    }

    fn do_handle_final_term_sequence(&self, data: &str) -> bool {
        let Some(terminal) = self.terminal() else {
            return false;
        };

        // Pass the sequence along to the capability
        // It was considered to disable the common protocol in order to not confuse the VS Code
        // shell integration if both happen for some reason. This doesn't work for powerlevel10k
        // though when instant prompt is enabled though. If this does end up being a problem we could pass
        // a type flag through the capability calls
        let mut parts = data.split(';');
        let command = parts.next().unwrap_or_default();
        let args: Vec<&str> = parts.collect();
        self.log_service.trace(&format!("ShellIntegrationAddon#_doHandleFinalTermSequence: received sequence {command}"));
        self.mark_sequence_seen(command);
        match command {
            final_term_osc_pt::PROMPT_START => {
                self.create_or_get_command_detection(&terminal).handle_prompt_start(None);
                true
            }
            final_term_osc_pt::COMMAND_START => {
                // Ignore the command line for these sequences as it's unreliable for example in powerlevel10k
                self.create_or_get_command_detection(&terminal).handle_command_start(Some(&IHandleCommandOptions { ignore_command_line: Some(true), ..IHandleCommandOptions::default() }));
                true
            }
            final_term_osc_pt::COMMAND_EXECUTED => {
                self.create_or_get_command_detection(&terminal).handle_command_executed(None);
                true
            }
            final_term_osc_pt::COMMAND_FINISHED => {
                let exit_code = if args.len() == 1 { Some(parse_int(args[0])) } else { None };
                self.create_or_get_command_detection(&terminal).handle_command_finished(exit_code, None);
                true
            }
            _ => false,
        }
    }

    fn handle_vscode_sequence(&self, data: &str) -> bool {
        let did_handle = self.do_handle_vscode_sequence(data);
        if !self.has_updated_telemetry.get() && did_handle {
            if let Some(telemetry_service) = &self.telemetry_service {
                telemetry_service.public_log2("terminal/shellIntegrationActivationSucceeded", None);
            }
            self.has_updated_telemetry.set(true);
            self.clear_activation_timeout();
        }
        if self.status.get() != ShellIntegrationStatus::VSCode {
            self.status.set(ShellIntegrationStatus::VSCode);
            self.on_did_change_status.fire(self.status.get());
        }
        did_handle
    }

    fn ensure_capabilities_or_add_failure_telemetry(&self) {
        let Some(telemetry_service) = self.telemetry_service.clone() else {
            return;
        };
        if self.disable_telemetry == Some(true) {
            return;
        }
        let weak = self.this.clone();
        self.activation_timeout.set(Some(runtime::set_timeout(
            move || {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if this.capabilities.get::<CommandDetection>().is_none() && this.capabilities.get::<CwdDetection>().is_none() {
                    telemetry_service.public_log2("terminal/shellIntegrationActivationTimeout", None);
                    this.log_service.warn("Shell integration failed to add capabilities within 10 seconds");
                }
                this.has_updated_telemetry.set(true);
            },
            ACTIVATION_TIMEOUT_MS,
        )));
    }

    fn clear_activation_timeout(&self) {
        if let Some(timeout) = self.activation_timeout.take() {
            runtime::clear_timeout(timeout);
        }
    }

    fn do_handle_vscode_sequence(&self, data: &str) -> bool {
        let Some(terminal) = self.terminal() else {
            return false;
        };

        // Pass the sequence along to the capability
        let args_index = data.find(';');
        let command = args_index.map_or(data, |index| &data[..index]);
        self.log_service.trace(&format!("ShellIntegrationAddon#_doHandleVSCodeSequence: received sequence {command}"));
        self.mark_sequence_seen(command);
        // Cast to strict checked index access
        let args: Vec<&str> = args_index.map_or_else(Vec::new, |index| data[index + 1..].split(';').collect());
        let arg = |index: usize| args.get(index).copied();
        let nonce_matches = |value: Option<&str>| value == Some(self.nonce.as_str());
        match command {
            vscode_osc_pt::PROMPT_START => {
                self.create_or_get_command_detection(&terminal).handle_prompt_start(None);
                true
            }
            vscode_osc_pt::COMMAND_START => {
                self.create_or_get_command_detection(&terminal).handle_command_start(None);
                true
            }
            vscode_osc_pt::COMMAND_EXECUTED => {
                self.create_or_get_command_detection(&terminal).handle_command_executed(None);
                true
            }
            vscode_osc_pt::COMMAND_FINISHED => {
                let exit_code = arg(0).map(parse_int);
                self.create_or_get_command_detection(&terminal).handle_command_finished(exit_code, None);
                true
            }
            vscode_osc_pt::COMMAND_LINE => {
                let command_line = match arg(0) {
                    Some(arg0) => deserialize_vscode_osc_message(arg0),
                    None => String::new(),
                };
                self.create_or_get_command_detection(&terminal).set_command_line(&command_line, nonce_matches(arg(1)));
                true
            }
            vscode_osc_pt::CONTINUATION_START => {
                self.create_or_get_command_detection(&terminal).handle_continuation_start();
                true
            }
            vscode_osc_pt::CONTINUATION_END => {
                self.create_or_get_command_detection(&terminal).handle_continuation_end();
                true
            }
            vscode_osc_pt::ENV_JSON => {
                if let Some(arg0) = arg(0) {
                    let text: Vec<u16> = deserialize_vscode_osc_message(arg0).encode_utf16().collect();
                    match json_parse(&text) {
                        Ok(env) => self.create_or_get_shell_env_detection().set_environment(Some(&json_to_environment(&env)), nonce_matches(arg(1))),
                        Err(_) => self.log_service.warn_args(&"Failed to parse environment from shell integration sequence".into(), &[arg0.into()]),
                    }
                }
                true
            }
            vscode_osc_pt::ENV_SINGLE_START => {
                self.create_or_get_shell_env_detection().start_environment_single_var(arg(0) == Some("1"), nonce_matches(arg(1)));
                true
            }
            vscode_osc_pt::ENV_SINGLE_DELETE => {
                if let (Some(arg0), Some(arg1)) = (arg(0), arg(1)) {
                    let env = deserialize_vscode_osc_message(arg1);
                    self.create_or_get_shell_env_detection().delete_environment_single_var(arg0, Some(env.as_str()), nonce_matches(arg(2)));
                }
                true
            }
            vscode_osc_pt::ENV_SINGLE_ENTRY => {
                if let (Some(arg0), Some(arg1)) = (arg(0), arg(1)) {
                    let env = deserialize_vscode_osc_message(arg1);
                    self.create_or_get_shell_env_detection().set_environment_single_var(arg0, Some(env.as_str()), nonce_matches(arg(2)));
                }
                true
            }
            vscode_osc_pt::ENV_SINGLE_END => {
                self.create_or_get_shell_env_detection().end_environment_single_var(nonce_matches(arg(0)));
                true
            }
            vscode_osc_pt::RIGHT_PROMPT_START => {
                self.create_or_get_command_detection(&terminal).handle_right_prompt_start();
                true
            }
            vscode_osc_pt::RIGHT_PROMPT_END => {
                self.create_or_get_command_detection(&terminal).handle_right_prompt_end();
                true
            }
            vscode_osc_pt::PROPERTY => {
                let deserialized = arg(0).map_or_else(String::new, deserialize_vscode_osc_message);
                let KeyValueAssignment { key, value } = parse_key_value_assignment(&deserialized);
                let Some(value) = value else {
                    return true;
                };
                match key.as_str() {
                    "ContinuationPrompt" => {
                        self.update_continuation_prompt(&remove_ansi_escape_codes_from_prompt(&value.encode_utf16().collect::<Vec<u16>>()).to_string_lossy());
                        return true;
                    }
                    "Cwd" => {
                        // OSC 633 ; P ; Cwd=<value> ; <nonce> ST - the nonce is optional and only
                        // present when emitted by a trusted shell integration script. CWD updates
                        // without a matching nonce are treated as untrusted to mitigate spoofing
                        // via OSC sequences injected through arbitrary terminal output.
                        let nonce = arg(1);
                        self.update_cwd(&value, nonce.is_some() && nonce_matches(nonce));
                        return true;
                    }
                    "IsWindows" => {
                        self.create_or_get_command_detection(&terminal).set_is_windows_pty(value == "True");
                        return true;
                    }
                    "HasRichCommandDetection" => {
                        self.create_or_get_command_detection(&terminal).set_has_rich_command_detection(value == "True");
                        return true;
                    }
                    "Prompt" => {
                        // Remove escape sequences from the user's prompt
                        self.update_prompt_terminator(&strip_sgr_sequences(&value));
                        return true;
                    }
                    "PromptType" => {
                        self.create_or_get_prompt_type_detection().set_prompt_type(&value);
                        return true;
                    }
                    "Task" => {
                        self.create_or_get_buffer_mark_detection(&terminal);
                        if let Some(command_detection) = self.capabilities.get::<CommandDetection>() {
                            command_detection.set_is_command_storage_disabled();
                        }
                        return true;
                    }
                    // Sem `break` no original: a chave desconhecida cai no `SetMark`.
                    _ => {}
                }
                self.add_mark_from_sequence(&terminal, &args);
                true
            }
            vscode_osc_pt::SET_MARK => {
                self.add_mark_from_sequence(&terminal, &args);
                true
            }
            // Unrecognized sequence
            _ => false,
        }
    }

    /// `this._createOrGetBufferMarkDetection(this._terminal).addMark(parseMarkSequence(args))`.
    fn add_mark_from_sequence(&self, terminal: &Rc<dyn IHeadlessTerminal>, args: &[&str]) {
        let sequence: Vec<Option<&str>> = args.iter().copied().map(Some).collect();
        let MarkSequence { id, hidden } = parse_mark_sequence(&sequence);
        self.create_or_get_buffer_mark_detection(terminal).add_mark(Some(IMarkProperties { id, hidden: Some(hidden), ..IMarkProperties::default() }));
    }

    fn update_continuation_prompt(&self, value: &str) {
        let Some(terminal) = self.terminal() else {
            return;
        };
        self.create_or_get_command_detection(&terminal).set_continuation_prompt(value);
    }

    fn update_prompt_terminator(&self, prompt: &str) {
        let Some(terminal) = self.terminal() else {
            return;
        };
        let prompt = JsString::from(prompt);
        let units = prompt.units();
        let newline = u16::from(b'\n');
        let last_prompt_line = units.iter().rposition(|unit| *unit == newline).map_or(units, |index| &units[index + 1..]);
        let space = u16::from(b' ');
        let prompt_terminator = if js_string::trim(last_prompt_line).len() == 1 {
            // The prompt line contains a single character, treat the full line as the
            // terminator for example "⮞ "
            last_prompt_line
        } else {
            // `lastIndexOf(' ')` de -1 faz o `substring(-1)` valer a linha inteira.
            last_prompt_line.iter().rposition(|unit| *unit == space).map_or(last_prompt_line, |index| &last_prompt_line[index..])
        };
        if !prompt_terminator.is_empty() {
            self.create_or_get_command_detection(&terminal)
                .set_prompt_terminator(&JsString::from(prompt_terminator).to_string_lossy(), &JsString::from(last_prompt_line).to_string_lossy());
        }
    }

    fn update_cwd(&self, value: &str, is_trusted: bool) {
        let value = sanitize_cwd(value);
        self.create_or_get_cwd_detection().update_cwd(&value, Some(is_trusted));
        if let Some(command_detection) = self.capabilities.get::<CommandDetection>() {
            command_detection.set_cwd(&value);
        }
    }

    fn do_handle_iterm_sequence(&self, data: &str) -> bool {
        let Some(terminal) = self.terminal() else {
            return false;
        };

        let command = data.split(';').next().unwrap_or_default();
        self.mark_sequence_seen(&format!("{};{command}", shell_integration_osc_ps::I_TERM));
        if command == iterm_osc_pt::SET_MARK {
            self.create_or_get_buffer_mark_detection(&terminal).add_mark(None);
        }
        // Checking for known `<key>=<value>` pairs.
        // Note that unlike `VSCodeOscPt.Property`, iTerm2 does not interpret backslash or hex-escape sequences.
        // See: https://github.com/gnachman/iTerm2/blob/bb0882332cec5196e4de4a4225978d746e935279/sources/VT100Terminal.m#L2089-L2105
        let KeyValueAssignment { key, value } = parse_key_value_assignment(command);

        let Some(value) = value else {
            // No '=' was found, so it's not a property assignment.
            return true;
        };

        if key == iterm_osc_pt::CURRENT_DIR {
            // Encountered: `OSC 1337 ; CurrentDir=<Cwd> ST`. The iTerm2 protocol has no
            // nonce, so cwd updates received this way are always considered untrusted.
            self.update_cwd(&value, false);
            return true;
        }

        // Unrecognized sequence
        false
    }

    fn do_handle_set_windows_friendly_cwd(&self, data: &str) -> bool {
        if self.terminal().is_none() {
            return false;
        }

        let mut parts = data.split(';');
        let command = parts.next().unwrap_or_default();
        let args: Vec<&str> = parts.collect();
        self.mark_sequence_seen(&format!("{};{command}", shell_integration_osc_ps::SET_WINDOWS_FRIENDLY_CWD));
        if command == "9" {
            // Encountered `OSC 9 ; 9 ; <cwd> ST`. The ConEmu/Windows-friendly cwd protocol
            // has no nonce, so cwd updates received this way are always considered untrusted.
            if let Some(cwd) = args.first() {
                self.update_cwd(cwd, false);
            }
            return true;
        }

        // Unrecognized sequence
        false
    }

    /// Handles the sequence: `OSC 7 ; scheme://cwd ST`
    fn do_handle_set_cwd(&self, data: &str) -> bool {
        if self.terminal().is_none() {
            return false;
        }

        let command = data.split(';').next().unwrap_or_default();
        self.mark_sequence_seen(&format!("{};{command}", shell_integration_osc_ps::SET_CWD));

        if is_file_uri_with_path(command) {
            let uri = URI::parse(command, false);
            if !uri.path().is_empty() {
                // The `OSC 7 ; scheme://cwd ST` protocol has no nonce, so cwd updates received
                // this way are always considered untrusted.
                self.update_cwd(&uri.path().to_string_lossy(), false);
                return true;
            }
        }

        // Unrecognized sequence
        false
    }

    pub fn serialize(&self) -> ISerializedCommandDetectionCapability {
        let Some(terminal) = self.terminal() else {
            return Self::empty_serialized_command_detection();
        };
        if !self.capabilities.has(TerminalCapability::CommandDetection) {
            return Self::empty_serialized_command_detection();
        }
        self.create_or_get_command_detection(&terminal).serialize()
    }

    fn empty_serialized_command_detection() -> ISerializedCommandDetectionCapability {
        ISerializedCommandDetectionCapability { is_windows_pty: false, has_rich_command_detection: false, commands: Vec::new(), prompt_input_model: None }
    }

    pub fn deserialize(&self, serialized: &ISerializedCommandDetectionCapability) {
        let Some(terminal) = self.terminal() else {
            errors::throw(Error::new("Cannot restore commands before addon is activated"));
        };
        let command_detection = self.create_or_get_command_detection(&terminal);
        command_detection.deserialize(serialized);
        if let Some(cwd) = command_detection.cwd() {
            // Cwd gets set when the command is deserialized, so we need to update it here
            self.update_cwd(&cwd, false);
        }
    }

    /// `_createOrGetCwdDetection()`.
    pub fn create_or_get_cwd_detection(&self) -> Rc<dyn ICwdDetectionCapability> {
        if let Some(cwd_detection) = self.capabilities.get::<CwdDetection>() {
            return cwd_detection;
        }
        let cwd_detection = CwdDetectionCapability::new();
        self.base.register(cwd_detection.clone());
        self.capabilities.add::<CwdDetection>(cwd_detection.clone());
        cwd_detection
    }

    /// `_createOrGetCommandDetection(terminal)`.
    pub fn create_or_get_command_detection(&self, terminal: &Rc<dyn IHeadlessTerminal>) -> Rc<dyn ICommandDetectionCapability> {
        if let Some(command_detection) = self.capabilities.get::<CommandDetection>() {
            return command_detection;
        }
        let command_detection = CommandDetectionCapability::new(terminal.clone(), self.log_service.clone());
        self.base.register(command_detection.clone());
        self.capabilities.add::<CommandDetection>(command_detection.clone());
        command_detection
    }

    /// `_createOrGetBufferMarkDetection(terminal)`.
    pub fn create_or_get_buffer_mark_detection(&self, terminal: &Rc<dyn IHeadlessTerminal>) -> Rc<dyn IBufferMarkCapability> {
        if let Some(buffer_mark_detection) = self.capabilities.get::<BufferMarkDetection>() {
            return buffer_mark_detection;
        }
        let buffer_mark_detection = BufferMarkCapability::new(terminal.clone());
        self.base.register(buffer_mark_detection.clone());
        self.capabilities.add::<BufferMarkDetection>(buffer_mark_detection.clone());
        buffer_mark_detection
    }

    /// `_createOrGetShellEnvDetection()`.
    pub fn create_or_get_shell_env_detection(&self) -> Rc<dyn IShellEnvDetectionCapability> {
        if let Some(shell_env_detection) = self.capabilities.get::<ShellEnvDetection>() {
            return shell_env_detection;
        }
        let shell_env_detection = ShellEnvDetectionCapability::new();
        self.base.register(shell_env_detection.clone());
        self.capabilities.add::<ShellEnvDetection>(shell_env_detection.clone());
        shell_env_detection
    }

    /// `_createOrGetPromptTypeDetection()`.
    pub fn create_or_get_prompt_type_detection(&self) -> Rc<dyn IPromptTypeDetectionCapability> {
        if let Some(prompt_type_detection) = self.capabilities.get::<PromptTypeDetection>() {
            return prompt_type_detection;
        }
        let prompt_type_detection = PromptTypeDetectionCapability::new();
        self.base.register(prompt_type_detection.clone());
        self.capabilities.add::<PromptTypeDetection>(prompt_type_detection.clone());
        prompt_type_detection
    }
}

impl IDisposable for ShellIntegrationAddon {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IShellIntegration for ShellIntegrationAddon {
    fn capabilities(&self) -> Rc<dyn ITerminalCapabilityStore> {
        self.capabilities.clone()
    }

    fn seen_sequences(&self) -> Vec<String> {
        self.seen_sequences.borrow().clone()
    }

    fn status(&self) -> ShellIntegrationStatus {
        self.status.get()
    }

    fn on_did_change_status(&self) -> Event<ShellIntegrationStatus> {
        self.on_did_change_status.event()
    }

    fn on_did_change_seen_sequences(&self) -> Event<Vec<String>> {
        self.on_did_change_seen_sequences.event()
    }

    fn deserialize(&self, serialized: &ISerializedCommandDetectionCapability) {
        ShellIntegrationAddon::deserialize(self, serialized);
    }

    fn set_next_command_id(&self, command: &str, command_id: &str) {
        ShellIntegrationAddon::set_next_command_id(self, command, command_id);
    }
}
