//! Porte de `src/vs/workbench/contrib/terminalContrib/typeAhead/common/terminalTypeAheadConfiguration.ts`: os
//! identificadores de configuração do eco local (type ahead) do terminal e o esquema das suas propriedades.
//!
//! Divergências:
//! - **`const enum`** de string é enum com `as_str()` (a convenção de `terminal_settings.rs`).
//! - **`IStringDictionary<IConfigurationPropertySchema>`** é a lista de pares `(chave, propriedade)` na ordem do original, com
//!   cada propriedade como [`PropertySchema`].
//! - **`getValue<ITerminalTypeAheadConfiguration>(...)`** devolve um valor dinâmico; [`ITerminalTypeAheadConfiguration::from_dynamic`]
//!   lê os campos que o addon usa, e um campo ausente é `None` (o `undefined` do original).

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::PropertySchema;

/// `DEFAULT_LOCAL_ECHO_EXCLUDE`.
pub const DEFAULT_LOCAL_ECHO_EXCLUDE: [&str; 4] = ["vim", "vi", "nano", "tmux"];

/// `TerminalTypeAheadSettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalTypeAheadSettingId {
    LocalEchoLatencyThreshold,
    LocalEchoEnabled,
    LocalEchoExcludePrograms,
    LocalEchoStyle,
}

impl TerminalTypeAheadSettingId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalEchoLatencyThreshold => "terminal.integrated.localEchoLatencyThreshold",
            Self::LocalEchoEnabled => "terminal.integrated.localEchoEnabled",
            Self::LocalEchoExcludePrograms => "terminal.integrated.localEchoExcludePrograms",
            Self::LocalEchoStyle => "terminal.integrated.localEchoStyle",
        }
    }
}

/// `ITerminalTypeAheadConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct ITerminalTypeAheadConfiguration {
    pub local_echo_latency_threshold: Option<f64>,
    pub local_echo_exclude_programs: Option<Vec<String>>,
    /// `'auto' | 'on' | 'off'`.
    pub local_echo_enabled: Option<String>,
    /// `'bold' | 'dim' | 'italic' | 'underlined' | 'inverted' | string`.
    pub local_echo_style: Option<String>,
}

impl ITerminalTypeAheadConfiguration {
    /// Os campos do objeto de configuração `terminal.integrated`.
    pub fn from_dynamic(value: &Dynamic) -> Self {
        let string = |key: &str| match value.get_str(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        Self {
            local_echo_latency_threshold: match value.get_str("localEchoLatencyThreshold") {
                Dynamic::Number(number) => Some(number),
                _ => None,
            },
            local_echo_exclude_programs: match value.get_str("localEchoExcludePrograms") {
                Dynamic::Array(items) => Some(items.to_vec().into_iter().filter_map(|item| if let Dynamic::String(text) = item { Some(text.to_string_lossy()) } else { None }).collect()),
                _ => None,
            },
            local_echo_enabled: string("localEchoEnabled"),
            local_echo_style: string("localEchoStyle"),
        }
    }
}

fn strings(items: &[&str]) -> Dynamic {
    Dynamic::array(items.iter().map(|item| Dynamic::string(item)))
}

/// `terminalTypeAheadConfiguration`.
pub fn terminal_type_ahead_configuration() -> Vec<(&'static str, PropertySchema)> {
    vec![
        (
            TerminalTypeAheadSettingId::LocalEchoLatencyThreshold.as_str(),
            PropertySchema::from_entries([
                (
                    "description",
                    Dynamic::string(&localize(
                        "terminal.integrated.localEchoLatencyThreshold",
                        "Length of network delay, in milliseconds, where local edits will be echoed on the terminal without waiting for server acknowledgement. If '0', local echo will always be on, and if '-1' it will be disabled.",
                        &[],
                    )),
                ),
                ("type", Dynamic::string("integer")),
                ("minimum", Dynamic::Number(-1.0)),
                ("default", Dynamic::Number(30.0)),
                ("tags", strings(&["preview"])),
            ]),
        ),
        (
            TerminalTypeAheadSettingId::LocalEchoEnabled.as_str(),
            PropertySchema::from_entries([
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "terminal.integrated.localEchoEnabled",
                        "When local echo should be enabled. This will override {0}",
                        &["`#terminal.integrated.localEchoLatencyThreshold#`"],
                    )),
                ),
                ("type", Dynamic::string("string")),
                ("enum", strings(&["on", "off", "auto"])),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("terminal.integrated.localEchoEnabled.on", "Always enabled", &[])),
                        Dynamic::string(&localize("terminal.integrated.localEchoEnabled.off", "Always disabled", &[])),
                        Dynamic::string(&localize("terminal.integrated.localEchoEnabled.auto", "Enabled only for remote workspaces", &[])),
                    ]),
                ),
                ("default", Dynamic::string("off")),
                ("tags", strings(&["preview"])),
            ]),
        ),
        (
            TerminalTypeAheadSettingId::LocalEchoExcludePrograms.as_str(),
            PropertySchema::from_entries([
                (
                    "description",
                    Dynamic::string(&localize(
                        "terminal.integrated.localEchoExcludePrograms",
                        "Local echo will be disabled when any of these program names are found in the terminal title.",
                        &[],
                    )),
                ),
                ("type", Dynamic::string("array")),
                ("items", Dynamic::object([("type", Dynamic::string("string")), ("uniqueItems", Dynamic::Bool(true))])),
                ("default", strings(&DEFAULT_LOCAL_ECHO_EXCLUDE)),
                ("tags", strings(&["preview"])),
            ]),
        ),
        (
            TerminalTypeAheadSettingId::LocalEchoStyle.as_str(),
            PropertySchema::from_entries([
                (
                    "description",
                    Dynamic::string(&localize("terminal.integrated.localEchoStyle", "Terminal style of locally echoed text; either a font style or an RGB color.", &[])),
                ),
                ("default", Dynamic::string("dim")),
                (
                    "anyOf",
                    Dynamic::array([
                        Dynamic::object([("enum", strings(&["bold", "dim", "italic", "underlined", "inverted", "#ff0000"]))]),
                        Dynamic::object([("type", Dynamic::string("string")), ("format", Dynamic::string("color-hex"))]),
                    ]),
                ),
                ("tags", strings(&["preview"])),
            ]),
        ),
    ]
}
