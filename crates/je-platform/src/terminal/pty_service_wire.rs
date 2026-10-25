//! A codificação dos valores que atravessam o canal do pty host (`ptyHost`, `localPty`): o equivalente do JSON que o
//! `ProxyChannel` do VS Code envia para cada argumento, resultado e evento do `IPtyService`.
//!
//! Não tem arquivo próprio no original (o JavaScript serializa o objeto como está): aqui cada tipo do contrato
//! ([`super::pty_service`] e [`super::terminal`]) implementa [`Wire`], com as mesmas chaves e formas do objeto do
//! original. As chaves de valor `undefined` não são escritas (`JSON.stringify` as omite) e a leitura trata chave
//! ausente, `undefined` e `null` do mesmo jeito.
//!
//! Divergências:
//! - Valores que são função (`customPtyImplementation`, o `waitOnExit` do tipo função) não existem em JSON: são
//!   omitidos ao escrever, como o `JSON.stringify` faz. `waitOnExit` de função lido do outro lado nunca aparece.
//! - A chave `[remoteResolverTerminal]` do `IShellLaunchConfig` é um `Symbol`: nunca atravessa o canal.
//! - `IMarkProperties.marker` (um `IMarker` vivo do xterm) não é serializável: o `ISerializedTerminalCommand` que
//!   vem de um `serialize()` já o troca pelas linhas, e o marcador fica de fora.
//! - Um `URI` viaja como [`Marshalled::Uri`]; um `UriComponents` simples como objeto com as cinco chaves.
//! - `IReconnectionProperties.data` (`unknown`) passa pelo JSON (`safeStringify` e leitura de volta).

use std::collections::BTreeMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json;
use je_base::marshalling::{self, Marshalled};
use je_base::performance::PerformanceMark;
use je_base::platform::OperatingSystem;
use je_base::process::ProcessEnvironment;
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::UriComponents;

use super::capabilities::capabilities::{IMarkProperties, ISerializedCommandDetectionCapability, ISerializedPromptInputModel, ISerializedTerminalCommand, TerminalCommandConfidence};
use super::capabilities::command_detection::prompt_input_model::IPromptInputModelState;
use super::environment_variable::{
    EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableCollectionDescription, IEnvironmentVariableMutator, IEnvironmentVariableMutatorOptions,
    ISerializableEnvironmentVariableCollections,
};
use super::pty_service::{
    IGetTerminalLayoutInfoArgs, IPersistentTerminalProcessLaunchConfig, IProcessDetails, IPtyHostAttachTarget, IPtyHostLatencyMeasurement, IRawTerminalInstanceLayoutInfo,
    IRawTerminalTabLayoutInfo, IRawTerminalsLayoutInfo, IRequestResolveVariablesEvent, ISerializedTerminalState, ISetTerminalLayoutInfoArgs, PtyDidChangeProperty, PtyDidRequestDetach,
    PtyProcessData, PtyProcessExit, PtyProcessOrphanQuestion, PtyProcessReady, PtyProcessReplay, WslPathDirection,
};
use super::terminal::{
    GeneralShellType, IAttachPersistentProcess, IFixedTerminalDimensions, IFreedPortProcess, IProcessDataEvent, IProcessProperty, IProcessReadyEvent, IProcessReadyWindowsPty,
    IReconnectionProperties, IShellLaunchConfig, ITerminalDimensionsOverride, ITerminalLaunchError, ITerminalLaunchOutcome, ITerminalLaunchResult, ITerminalProcessOptions,
    ITerminalProcessShellIntegrationOptions, ITerminalProfile, ITerminalTabAction, InitialText, LightDarkIcon, PosixShellType, ProcessData, ProcessPropertyType, ShellCwd, ShellIntegrationInjectionFailureReason,
    ShellType, TerminalEnvironmentValue, TerminalIcon, TerminalType, TitleEventSource, UnicodeVersion, UriLike, WaitOnExitValue, WindowsPtyBackend, WindowsShellType,
};
use super::terminal_recorder::{IPtyHostProcessReplayEvent, ReplayEntry};
use crate::workspace::WorkspaceFolder;

/// Um valor que atravessa o canal.
pub trait Wire: Sized {
    fn to_wire(&self) -> Marshalled;
    fn from_wire(value: &Marshalled) -> Result<Self, Error>;
}

fn invalid(what: &str, value: &Marshalled) -> Error {
    Error::new(format!("Invalid {what} on the pty channel: {value:?}"))
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn is_absent(value: &Marshalled) -> bool {
    matches!(value, Marshalled::Undefined | Marshalled::Null)
}

/// O objeto sendo escrito: as chaves `undefined` ficam de fora.
#[derive(Default)]
pub struct ObjectWriter(Vec<(JsString, Marshalled)>);

impl ObjectWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put<T: Wire>(mut self, key: &str, value: &T) -> Self {
        let wire = value.to_wire();
        if !matches!(wire, Marshalled::Undefined) {
            self.0.push((js(key), wire));
        }
        self
    }

    pub fn finish(self) -> Marshalled {
        Marshalled::Object(self.0)
    }
}

/// O objeto sendo lido: uma chave ausente lê como `undefined`.
pub struct ObjectReader<'a> {
    entries: &'a [(JsString, Marshalled)],
}

impl<'a> ObjectReader<'a> {
    pub fn new(value: &'a Marshalled, what: &str) -> Result<Self, Error> {
        match value {
            Marshalled::Object(entries) => Ok(Self { entries }),
            other => Err(invalid(what, other)),
        }
    }

    pub fn raw(&self, key: &str) -> &'a Marshalled {
        const ABSENT: Marshalled = Marshalled::Undefined;
        self.entries.iter().find(|(entry, _)| *entry == key).map_or(&ABSENT, |(_, value)| value)
    }

    pub fn get<T: Wire>(&self, key: &str) -> Result<T, Error> {
        T::from_wire(self.raw(key))
    }
}

macro_rules! wire_integer {
    ($($ty:ty),+) => {$(
        impl Wire for $ty {
            fn to_wire(&self) -> Marshalled {
                Marshalled::Number(*self as f64)
            }

            fn from_wire(value: &Marshalled) -> Result<Self, Error> {
                match value {
                    Marshalled::Number(number) if number.fract() == 0.0 => Ok(*number as $ty),
                    other => Err(invalid(stringify!($ty), other)),
                }
            }
        }
    )+};
}

wire_integer!(i32, u32, u64, usize);

impl Wire for f64 {
    fn to_wire(&self) -> Marshalled {
        Marshalled::Number(*self)
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Number(number) => Ok(*number),
            other => Err(invalid("number", other)),
        }
    }
}

impl Wire for bool {
    fn to_wire(&self) -> Marshalled {
        Marshalled::Bool(*self)
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Bool(flag) => Ok(*flag),
            other => Err(invalid("boolean", other)),
        }
    }
}

impl Wire for String {
    fn to_wire(&self) -> Marshalled {
        Marshalled::String(js(self))
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::String(text) => Ok(text.to_string_lossy()),
            other => Err(invalid("string", other)),
        }
    }
}

impl Wire for JsString {
    fn to_wire(&self) -> Marshalled {
        Marshalled::String(self.clone())
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::String(text) => Ok(text.clone()),
            other => Err(invalid("string", other)),
        }
    }
}

/// O retorno de um método `Promise<void>`: o resultado é `undefined` e nada é lido.
impl Wire for () {
    fn to_wire(&self) -> Marshalled {
        Marshalled::Undefined
    }

    fn from_wire(_value: &Marshalled) -> Result<Self, Error> {
        Ok(())
    }
}

impl<T: Wire> Wire for Option<T> {
    fn to_wire(&self) -> Marshalled {
        self.as_ref().map_or(Marshalled::Undefined, Wire::to_wire)
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        if is_absent(value) { Ok(None) } else { T::from_wire(value).map(Some) }
    }
}

impl<T: Wire> Wire for Vec<T> {
    fn to_wire(&self) -> Marshalled {
        Marshalled::Array(self.iter().map(Wire::to_wire).collect())
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Array(items) => items.iter().map(T::from_wire).collect(),
            other => Err(invalid("array", other)),
        }
    }
}

impl Wire for ProcessEnvironment {
    fn to_wire(&self) -> Marshalled {
        Marshalled::Object(self.iter().map(|(key, value)| (js(key), Marshalled::String(js(value)))).collect())
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            // `IProcessEnvironment` é `string | undefined` por chave: o que não é texto não entra.
            Marshalled::Object(entries) => Ok(entries
                .iter()
                .filter_map(|(key, value)| match value {
                    Marshalled::String(text) => Some((key.to_string_lossy(), text.to_string_lossy())),
                    _ => None,
                })
                .collect::<BTreeMap<_, _>>()),
            other => Err(invalid("environment", other)),
        }
    }
}

/// Um valor dinâmico (`unknown`) atravessa como JSON.
fn dynamic_to_wire(value: &Dynamic) -> Marshalled {
    je_base::objects::safe_stringify(value).and_then(|text| json::parse_str(&text, &mut Vec::new())).map_or(Marshalled::Undefined, |parsed| marshalling::from_json(&parsed))
}

pub fn dynamic_from_wire(value: &Marshalled) -> Dynamic {
    marshalling::json_stringify(value)
        .and_then(|text| json::parse_str(&text.to_string_lossy(), &mut Vec::new()))
        .map_or(Dynamic::Undefined, |parsed| crate::js::from_json(&parsed))
}

macro_rules! wire_enum {
    ($(#[$meta:meta])* $ty:ty, $what:literal, $to:expr, $from:expr) => {
        impl Wire for $ty {
            fn to_wire(&self) -> Marshalled {
                let to: fn(&$ty) -> Marshalled = $to;
                to(self)
            }

            fn from_wire(value: &Marshalled) -> Result<Self, Error> {
                let from: fn(&Marshalled) -> Option<$ty> = $from;
                from(value).ok_or_else(|| invalid($what, value))
            }
        }
    };
}

fn text_of(value: &Marshalled) -> Option<String> {
    match value {
        Marshalled::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn number_of(value: &Marshalled) -> Option<f64> {
    match value {
        Marshalled::Number(number) => Some(*number),
        _ => None,
    }
}

wire_enum!(
    UnicodeVersion,
    "unicode version",
    |version| Marshalled::String(js(version.as_str())),
    |value| match text_of(value)?.as_str() {
        "6" => Some(UnicodeVersion::V6),
        "11" => Some(UnicodeVersion::V11),
        _ => None,
    }
);

wire_enum!(
    WslPathDirection,
    "wsl path direction",
    |direction| Marshalled::String(js(direction.as_str())),
    |value| match text_of(value)?.as_str() {
        "unix-to-win" => Some(WslPathDirection::UnixToWin),
        "win-to-unix" => Some(WslPathDirection::WinToUnix),
        _ => None,
    }
);

wire_enum!(
    TitleEventSource,
    "title event source",
    |source| Marshalled::Number(*source as u8 as f64),
    |value| match number_of(value)? as i64 {
        0 => Some(TitleEventSource::Api),
        1 => Some(TitleEventSource::Process),
        2 => Some(TitleEventSource::Sequence),
        3 => Some(TitleEventSource::Config),
        _ => None,
    }
);

wire_enum!(
    OperatingSystem,
    "operating system",
    |os| Marshalled::Number(*os as u8 as f64),
    |value| match number_of(value)? as i64 {
        1 => Some(OperatingSystem::Windows),
        2 => Some(OperatingSystem::Macintosh),
        3 => Some(OperatingSystem::Linux),
        _ => None,
    }
);

wire_enum!(
    TerminalType,
    "terminal type",
    |type_| Marshalled::String(js(if *type_ == TerminalType::Task { "Task" } else { "Local" })),
    |value| match text_of(value)?.as_str() {
        "Task" => Some(TerminalType::Task),
        "Local" => Some(TerminalType::Local),
        _ => None,
    }
);

/// Os ids de `ProcessPropertyType`, na ordem do original.
const PROCESS_PROPERTY_TYPES: [ProcessPropertyType; 11] = [
    ProcessPropertyType::Cwd,
    ProcessPropertyType::InitialCwd,
    ProcessPropertyType::FixedDimensions,
    ProcessPropertyType::Title,
    ProcessPropertyType::ShellType,
    ProcessPropertyType::HasChildProcesses,
    ProcessPropertyType::ResolvedShellLaunchConfig,
    ProcessPropertyType::OverrideDimensions,
    ProcessPropertyType::FailedShellIntegrationActivation,
    ProcessPropertyType::UsedShellIntegrationInjection,
    ProcessPropertyType::ShellIntegrationInjectionFailureReason,
];

wire_enum!(
    ProcessPropertyType,
    "process property type",
    |type_| Marshalled::String(js(type_.as_str())),
    |value| {
        let name = text_of(value)?;
        PROCESS_PROPERTY_TYPES.into_iter().find(|type_| type_.as_str() == name)
    }
);

const SHELL_INTEGRATION_FAILURE_REASONS: [ShellIntegrationInjectionFailureReason; 9] = [
    ShellIntegrationInjectionFailureReason::InjectionSettingDisabled,
    ShellIntegrationInjectionFailureReason::NoExecutable,
    ShellIntegrationInjectionFailureReason::FeatureTerminal,
    ShellIntegrationInjectionFailureReason::IgnoreShellIntegrationFlag,
    ShellIntegrationInjectionFailureReason::UnsupportedWindowsBuild,
    ShellIntegrationInjectionFailureReason::UnsupportedArgs,
    ShellIntegrationInjectionFailureReason::UnsupportedShell,
    ShellIntegrationInjectionFailureReason::FailedToSetStickyBit,
    ShellIntegrationInjectionFailureReason::FailedToCreateTmpDir,
];

wire_enum!(
    ShellIntegrationInjectionFailureReason,
    "shell integration failure reason",
    |reason| Marshalled::String(js(reason.as_str())),
    |value| {
        let name = text_of(value)?;
        SHELL_INTEGRATION_FAILURE_REASONS.into_iter().find(|reason| reason.as_str() == name)
    }
);

const POSIX_SHELLS: [PosixShellType; 6] = [PosixShellType::Bash, PosixShellType::Fish, PosixShellType::Sh, PosixShellType::Csh, PosixShellType::Ksh, PosixShellType::Zsh];
const WINDOWS_SHELLS: [WindowsShellType; 3] = [WindowsShellType::CommandPrompt, WindowsShellType::Wsl, WindowsShellType::GitBash];
const GENERAL_SHELLS: [GeneralShellType; 11] = [
    GeneralShellType::Claude,
    GeneralShellType::Codex,
    GeneralShellType::CommandCode,
    GeneralShellType::Copilot,
    GeneralShellType::Gemini,
    GeneralShellType::PowerShell,
    GeneralShellType::Python,
    GeneralShellType::Julia,
    GeneralShellType::NuShell,
    GeneralShellType::Node,
    GeneralShellType::Xonsh,
];

wire_enum!(
    ShellType,
    "shell type",
    |shell| Marshalled::String(js(shell.as_str())),
    |value| {
        let name = text_of(value)?;
        POSIX_SHELLS
            .into_iter()
            .find(|shell| shell.as_str() == name)
            .map(ShellType::from)
            .or_else(|| WINDOWS_SHELLS.into_iter().find(|shell| shell.as_str() == name).map(ShellType::from))
            .or_else(|| GENERAL_SHELLS.into_iter().find(|shell| shell.as_str() == name).map(ShellType::from))
    }
);

wire_enum!(
    TerminalCommandConfidence,
    "command confidence",
    |confidence| Marshalled::String(js(match confidence {
        TerminalCommandConfidence::Low => "low",
        TerminalCommandConfidence::Medium => "medium",
        TerminalCommandConfidence::High => "high",
    })),
    |value| match text_of(value)?.as_str() {
        "low" => Some(TerminalCommandConfidence::Low),
        "medium" => Some(TerminalCommandConfidence::Medium),
        "high" => Some(TerminalCommandConfidence::High),
        _ => None,
    }
);

wire_enum!(
    EnvironmentVariableMutatorType,
    "mutator type",
    |type_| Marshalled::Number(*type_ as u8 as f64),
    |value| match number_of(value)? as i64 {
        1 => Some(EnvironmentVariableMutatorType::Replace),
        2 => Some(EnvironmentVariableMutatorType::Append),
        3 => Some(EnvironmentVariableMutatorType::Prepend),
        _ => None,
    }
);

impl Wire for UriLike {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Uri(uri) => Marshalled::Uri(uri.clone()),
            Self::Components(components) => ObjectWriter::new()
                .put("scheme", &components.scheme)
                .put("authority", &components.authority)
                .put("path", &components.path)
                .put("query", &components.query)
                .put("fragment", &components.fragment)
                .finish(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Uri(uri) => Ok(Self::Uri(uri.clone())),
            other => {
                let object = ObjectReader::new(other, "URI")?;
                Ok(Self::Components(UriComponents {
                    scheme: object.get("scheme")?,
                    authority: object.get("authority")?,
                    path: object.get("path")?,
                    query: object.get("query")?,
                    fragment: object.get("fragment")?,
                }))
            }
        }
    }
}

impl Wire for ThemeIcon {
    fn to_wire(&self) -> Marshalled {
        // `color` é um `ThemeColor`, `{ id }`.
        let color = self.color.as_ref().map(|color| ObjectWriter::new().put("id", &color.id).finish());
        put_raw(ObjectWriter::new().put("id", &self.id), "color", color.unwrap_or(Marshalled::Undefined)).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "theme icon")?;
        // `color` é um `ThemeColor`, `{ id }`.
        let color = match object.raw("color") {
            Marshalled::Object(_) => Some(ThemeColor::new(ObjectReader::new(object.raw("color"), "theme color")?.get::<String>("id")?)),
            Marshalled::String(id) => Some(ThemeColor::new(id.to_string_lossy())),
            _ => None,
        };
        Ok(Self { id: object.get("id")?, color })
    }
}

impl Wire for TerminalIcon {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Theme(icon) => icon.to_wire(),
            Self::Uri(uri) => uri.to_wire(),
            Self::LightDark(icons) => ObjectWriter::new().put("light", &icons.light).put("dark", &icons.dark).finish(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Uri(uri) => Ok(Self::Uri(UriLike::Uri(uri.clone()))),
            Marshalled::Object(_) => {
                let object = ObjectReader::new(value, "terminal icon")?;
                if !is_absent(object.raw("id")) {
                    Ok(Self::Theme(ThemeIcon::from_wire(value)?))
                } else if !is_absent(object.raw("light")) {
                    Ok(Self::LightDark(LightDarkIcon { light: object.get("light")?, dark: object.get("dark")? }))
                } else {
                    Ok(Self::Uri(UriLike::from_wire(value)?))
                }
            }
            other => Err(invalid("terminal icon", other)),
        }
    }
}

impl Wire for IFixedTerminalDimensions {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("cols", &self.cols).put("rows", &self.rows).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "fixed dimensions")?;
        Ok(Self { cols: object.get("cols")?, rows: object.get("rows")? })
    }
}

impl Wire for ITerminalDimensionsOverride {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("cols", &self.cols).put("rows", &self.rows).put("forceExactSize", &self.force_exact_size).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "dimensions override")?;
        Ok(Self { cols: object.get("cols")?, rows: object.get("rows")?, force_exact_size: object.get("forceExactSize")? })
    }
}

impl Wire for IReconnectionProperties {
    fn to_wire(&self) -> Marshalled {
        let mut object = ObjectWriter::new().put("ownerId", &self.owner_id);
        if let Some(data) = &self.data {
            let wire = dynamic_to_wire(data);
            if !matches!(wire, Marshalled::Undefined) {
                object.0.push((js("data"), wire));
            }
        }
        object.finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "reconnection properties")?;
        let data = object.raw("data");
        Ok(Self { owner_id: object.get("ownerId")?, data: if matches!(data, Marshalled::Undefined) { None } else { Some(dynamic_from_wire(data)) } })
    }
}

impl Wire for WaitOnExitValue {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Bool(flag) => Marshalled::Bool(*flag),
            Self::Text(text) => Marshalled::String(js(text)),
            // Uma função não atravessa o canal.
            Self::Callback(_) => Marshalled::Undefined,
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Bool(flag) => Ok(Self::Bool(*flag)),
            Marshalled::String(text) => Ok(Self::Text(text.to_string_lossy())),
            other => Err(invalid("wait on exit", other)),
        }
    }
}

impl Wire for ITerminalTabAction {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("label", &self.label).put("icon", &self.icon).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "tab action")?;
        Ok(Self { id: object.get("id")?, label: object.get("label")?, icon: object.get("icon")? })
    }
}

impl Wire for WorkspaceFolder {
    /// `IWorkspaceFolderData`: `{ uri, name, index }`.
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("uri", &UriLike::Uri(self.uri.clone())).put("name", &self.name).put("index", &self.index).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "workspace folder")?;
        let uri = match object.raw("uri") {
            Marshalled::Uri(uri) => uri.clone(),
            other => return Err(invalid("workspace folder uri", other)),
        };
        Ok(WorkspaceFolder::new(uri, object.get::<String>("name")?, object.get::<usize>("index")?, None))
    }
}

impl Wire for EnvironmentVariableScope {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("workspaceFolder", &self.workspace_folder).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        Ok(Self { workspace_folder: ObjectReader::new(value, "environment scope")?.get("workspaceFolder")? })
    }
}

impl Wire for IEnvironmentVariableMutatorOptions {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("applyAtProcessCreation", &self.apply_at_process_creation).put("applyAtShellIntegration", &self.apply_at_shell_integration).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "mutator options")?;
        Ok(Self { apply_at_process_creation: object.get("applyAtProcessCreation")?, apply_at_shell_integration: object.get("applyAtShellIntegration")? })
    }
}

impl Wire for IEnvironmentVariableMutator {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("variable", &self.variable)
            .put("value", &self.value)
            .put("type", &self.mutator_type)
            .put("scope", &self.scope)
            .put("options", &self.options)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "environment variable mutator")?;
        Ok(Self { variable: object.get("variable")?, value: object.get("value")?, mutator_type: object.get("type")?, scope: object.get("scope")?, options: object.get("options")? })
    }
}

impl Wire for IEnvironmentVariableCollectionDescription {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("description", &self.description).put("scope", &self.scope).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "environment variable description")?;
        Ok(Self { description: object.get("description")?, scope: object.get("scope")? })
    }
}

/// Um par `[chave, valor]` (as tuplas de `ISerializableEnvironmentVariableCollection`).
fn pair_to_wire<T: Wire>(key: &str, value: &T) -> Marshalled {
    Marshalled::Array(vec![Marshalled::String(js(key)), value.to_wire()])
}

fn pair_from_wire<T: Wire>(value: &Marshalled) -> Result<(String, T), Error> {
    match value {
        Marshalled::Array(items) if items.len() == 2 => Ok((String::from_wire(&items[0])?, T::from_wire(&items[1])?)),
        other => Err(invalid("pair", other)),
    }
}

/// `ISerializableEnvironmentVariableCollections`: `[extension, collection, descriptions][]`.
fn collections_to_wire(collections: &ISerializableEnvironmentVariableCollections) -> Marshalled {
    Marshalled::Array(
        collections
            .iter()
            .map(|(extension, collection, descriptions)| {
                Marshalled::Array(vec![
                    Marshalled::String(js(extension)),
                    Marshalled::Array(collection.iter().map(|(variable, mutator)| pair_to_wire(variable, mutator)).collect()),
                    Marshalled::Array(descriptions.iter().map(|(variable, description)| pair_to_wire(variable, description)).collect()),
                ])
            })
            .collect(),
    )
}

fn collections_from_wire(value: &Marshalled) -> Result<ISerializableEnvironmentVariableCollections, Error> {
    let Marshalled::Array(items) = value else { return Err(invalid("environment variable collections", value)) };
    items
        .iter()
        .map(|item| match item {
            Marshalled::Array(parts) if parts.len() == 3 => {
                let collection = match &parts[1] {
                    Marshalled::Array(entries) => entries.iter().map(pair_from_wire::<IEnvironmentVariableMutator>).collect::<Result<Vec<_>, _>>()?,
                    other => return Err(invalid("environment variable collection", other)),
                };
                let descriptions = match &parts[2] {
                    Marshalled::Array(entries) => entries.iter().map(pair_from_wire::<IEnvironmentVariableCollectionDescription>).collect::<Result<Vec<_>, _>>()?,
                    other => return Err(invalid("environment variable descriptions", other)),
                };
                Ok((String::from_wire(&parts[0])?, collection, descriptions))
            }
            other => Err(invalid("environment variable collections entry", other)),
        })
        .collect()
}

/// `ISerializableEnvironmentVariableCollections` não é um tipo próprio (é um `Vec` de tuplas), então tem o seu invólucro.
pub struct EnvironmentCollections(pub ISerializableEnvironmentVariableCollections);

impl Wire for EnvironmentCollections {
    fn to_wire(&self) -> Marshalled {
        collections_to_wire(&self.0)
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        collections_from_wire(value).map(Self)
    }
}

fn optional_collections_to_wire(collections: &Option<ISerializableEnvironmentVariableCollections>) -> Marshalled {
    collections.as_ref().map_or(Marshalled::Undefined, collections_to_wire)
}

fn optional_collections_from_wire(value: &Marshalled) -> Result<Option<ISerializableEnvironmentVariableCollections>, Error> {
    if is_absent(value) { Ok(None) } else { collections_from_wire(value).map(Some) }
}

/// Escreve uma chave cujo valor já é um [`Marshalled`] (as que não têm tipo [`Wire`] próprio).
fn put_raw(mut object: ObjectWriter, key: &str, wire: Marshalled) -> ObjectWriter {
    if !matches!(wire, Marshalled::Undefined) {
        object.0.push((js(key), wire));
    }
    object
}

impl Wire for ITerminalProcessShellIntegrationOptions {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("enabled", &self.enabled).put("suggestEnabled", &self.suggest_enabled).put("nonce", &self.nonce).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "shell integration options")?;
        Ok(Self { enabled: object.get("enabled")?, suggest_enabled: object.get("suggestEnabled")?, nonce: object.get("nonce")? })
    }
}

impl Wire for ITerminalProcessOptions {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new().put("shellIntegration", &self.shell_integration).put("windowsUseConptyDll", &self.windows_use_conpty_dll);
        let object = put_raw(object, "environmentVariableCollections", optional_collections_to_wire(&self.environment_variable_collections));
        object.put("workspaceFolder", &self.workspace_folder).put("isScreenReaderOptimized", &self.is_screen_reader_optimized).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process options")?;
        Ok(Self {
            shell_integration: object.get("shellIntegration")?,
            windows_use_conpty_dll: object.get("windowsUseConptyDll")?,
            environment_variable_collections: optional_collections_from_wire(object.raw("environmentVariableCollections"))?,
            workspace_folder: object.get("workspaceFolder")?,
            is_screen_reader_optimized: object.get("isScreenReaderOptimized")?,
        })
    }
}

impl<T: Wire> Wire for OneOrMany<T> {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::One(one) => one.to_wire(),
            Self::Many(many) => many.to_wire(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::Array(_) => Vec::<T>::from_wire(value).map(Self::Many),
            other => T::from_wire(other).map(Self::One),
        }
    }
}

/// `ITerminalEnvironment`: `{ [key]: string | null | undefined }`, na ordem do objeto.
fn terminal_environment_to_wire(environment: &[(String, TerminalEnvironmentValue)]) -> Marshalled {
    Marshalled::Object(
        environment
            .iter()
            .filter_map(|(key, value)| match value {
                TerminalEnvironmentValue::Value(text) => Some((js(key), Marshalled::String(js(text)))),
                TerminalEnvironmentValue::Null => Some((js(key), Marshalled::Null)),
                TerminalEnvironmentValue::Undefined => None,
            })
            .collect(),
    )
}

fn terminal_environment_from_wire(value: &Marshalled) -> Result<Vec<(String, TerminalEnvironmentValue)>, Error> {
    match value {
        Marshalled::Object(entries) => entries
            .iter()
            .map(|(key, value)| match value {
                Marshalled::String(text) => Ok((key.to_string_lossy(), TerminalEnvironmentValue::Value(text.to_string_lossy()))),
                Marshalled::Null => Ok((key.to_string_lossy(), TerminalEnvironmentValue::Null)),
                Marshalled::Undefined => Ok((key.to_string_lossy(), TerminalEnvironmentValue::Undefined)),
                other => Err(invalid("terminal environment value", other)),
            })
            .collect(),
        other => Err(invalid("terminal environment", other)),
    }
}

/// O `ITerminalEnvironment` como valor próprio, para o `put`/`get` das structs.
pub struct TerminalEnvironment(pub Vec<(String, TerminalEnvironmentValue)>);

impl Wire for TerminalEnvironment {
    fn to_wire(&self) -> Marshalled {
        terminal_environment_to_wire(&self.0)
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        terminal_environment_from_wire(value).map(Self)
    }
}

impl Wire for ShellCwd {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Path(path) => Marshalled::String(js(path)),
            Self::Uri(uri) => Marshalled::Uri(uri.clone()),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::String(path) => Ok(Self::Path(path.to_string_lossy())),
            Marshalled::Uri(uri) => Ok(Self::Uri(uri.clone())),
            other => Err(invalid("cwd", other)),
        }
    }
}

impl Wire for InitialText {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Text(text) => Marshalled::String(js(text)),
            Self::WithTrailingNewLine { text, trailing_new_line } => ObjectWriter::new().put("text", text).put("trailingNewLine", trailing_new_line).finish(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::String(text) => Ok(Self::Text(text.to_string_lossy())),
            other => {
                let object = ObjectReader::new(other, "initial text")?;
                Ok(Self::WithTrailingNewLine { text: object.get("text")?, trailing_new_line: object.get("trailingNewLine")? })
            }
        }
    }
}

impl Wire for IAttachPersistentProcess {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new()
            .put("id", &self.id)
            .put("findRevivedId", &self.find_revived_id)
            .put("pid", &self.pid)
            .put("title", &self.title)
            .put("titleSource", &self.title_source)
            .put("cwd", &self.cwd)
            .put("icon", &self.icon)
            .put("color", &self.color)
            .put("hasChildProcesses", &self.has_child_processes)
            .put("fixedDimensions", &self.fixed_dimensions);
        let object = put_raw(object, "environmentVariableCollections", optional_collections_to_wire(&self.environment_variable_collections));
        object
            .put("reconnectionProperties", &self.reconnection_properties)
            .put("type", &self.type_)
            .put("waitOnExit", &self.wait_on_exit)
            .put("hideFromUser", &self.hide_from_user)
            .put("isFeatureTerminal", &self.is_feature_terminal)
            .put("shellIntegrationNonce", &self.shell_integration_nonce)
            .put("tabActions", &self.tab_actions)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "attach persistent process")?;
        Ok(Self {
            id: object.get("id")?,
            find_revived_id: object.get("findRevivedId")?,
            pid: object.get("pid")?,
            title: object.get("title")?,
            title_source: object.get("titleSource")?,
            cwd: object.get("cwd")?,
            icon: object.get("icon")?,
            color: object.get("color")?,
            has_child_processes: object.get("hasChildProcesses")?,
            fixed_dimensions: object.get("fixedDimensions")?,
            environment_variable_collections: optional_collections_from_wire(object.raw("environmentVariableCollections"))?,
            reconnection_properties: object.get("reconnectionProperties")?,
            type_: object.get("type")?,
            wait_on_exit: object.get("waitOnExit")?,
            hide_from_user: object.get("hideFromUser")?,
            is_feature_terminal: object.get("isFeatureTerminal")?,
            shell_integration_nonce: object.get("shellIntegrationNonce")?,
            tab_actions: object.get("tabActions")?,
        })
    }
}

impl Wire for IShellLaunchConfig {
    fn to_wire(&self) -> Marshalled {
        let env = self.env.as_ref().map(|env| TerminalEnvironment(env.clone()));
        ObjectWriter::new()
            .put("name", &self.name)
            .put("type", &self.type_)
            .put("executable", &self.executable)
            .put("args", &self.args)
            .put("cwd", &self.cwd)
            .put("env", &env)
            .put("ignoreConfigurationCwd", &self.ignore_configuration_cwd)
            .put("reconnectionProperties", &self.reconnection_properties)
            .put("waitOnExit", &self.wait_on_exit)
            .put("initialText", &self.initial_text)
            .put("extHostTerminalId", &self.ext_host_terminal_id)
            .put("attachPersistentProcess", &self.attach_persistent_process)
            .put("strictEnv", &self.strict_env)
            .put("useShellEnvironment", &self.use_shell_environment)
            .put("hideFromUser", &self.hide_from_user)
            .put("forcePersist", &self.force_persist)
            .put("isFeatureTerminal", &self.is_feature_terminal)
            .put("isExtensionOwnedTerminal", &self.is_extension_owned_terminal)
            .put("icon", &self.icon)
            .put("color", &self.color)
            .put("parentTerminalId", &self.parent_terminal_id)
            .put("fixedDimensions", &self.fixed_dimensions)
            .put("isTransient", &self.is_transient)
            .put("forceShellIntegration", &self.force_shell_integration)
            .put("ignoreShellIntegration", &self.ignore_shell_integration)
            .put("tabActions", &self.tab_actions)
            .put("shellIntegrationEnvironmentReporting", &self.shell_integration_environment_reporting)
            .put("shellIntegrationNonce", &self.shell_integration_nonce)
            .put("titleTemplate", &self.title_template)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "shell launch config")?;
        let env: Option<TerminalEnvironment> = object.get("env")?;
        Ok(Self {
            custom_pty_implementation: None,
            name: object.get("name")?,
            type_: object.get("type")?,
            executable: object.get("executable")?,
            args: object.get("args")?,
            cwd: object.get("cwd")?,
            env: env.map(|env| env.0),
            ignore_configuration_cwd: object.get("ignoreConfigurationCwd")?,
            reconnection_properties: object.get("reconnectionProperties")?,
            wait_on_exit: object.get("waitOnExit")?,
            initial_text: object.get("initialText")?,
            ext_host_terminal_id: object.get("extHostTerminalId")?,
            attach_persistent_process: object.get("attachPersistentProcess")?,
            strict_env: object.get("strictEnv")?,
            use_shell_environment: object.get("useShellEnvironment")?,
            hide_from_user: object.get("hideFromUser")?,
            force_persist: object.get("forcePersist")?,
            is_feature_terminal: object.get("isFeatureTerminal")?,
            is_extension_owned_terminal: object.get("isExtensionOwnedTerminal")?,
            remote_resolver_terminal: false,
            icon: object.get("icon")?,
            color: object.get("color")?,
            parent_terminal_id: object.get("parentTerminalId")?,
            fixed_dimensions: object.get("fixedDimensions")?,
            is_transient: object.get("isTransient")?,
            force_shell_integration: object.get("forceShellIntegration")?,
            ignore_shell_integration: object.get("ignoreShellIntegration")?,
            tab_actions: object.get("tabActions")?,
            shell_integration_environment_reporting: object.get("shellIntegrationEnvironmentReporting")?,
            shell_integration_nonce: object.get("shellIntegrationNonce")?,
            title_template: object.get("titleTemplate")?,
        })
    }
}

impl Wire for ITerminalProfile {
    fn to_wire(&self) -> Marshalled {
        let env = self.env.as_ref().map(|env| TerminalEnvironment(env.clone()));
        ObjectWriter::new()
            .put("profileName", &self.profile_name)
            .put("path", &self.path)
            .put("isDefault", &self.is_default)
            .put("isUnsafePath", &self.is_unsafe_path)
            .put("requiresUnsafePath", &self.requires_unsafe_path)
            .put("isAutoDetected", &self.is_auto_detected)
            .put("isFromPath", &self.is_from_path)
            .put("args", &self.args)
            .put("env", &env)
            .put("overrideName", &self.override_name)
            .put("color", &self.color)
            .put("icon", &self.icon)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "terminal profile")?;
        let env: Option<TerminalEnvironment> = object.get("env")?;
        Ok(Self {
            profile_name: object.get("profileName")?,
            path: object.get("path")?,
            is_default: object.get("isDefault")?,
            is_unsafe_path: object.get("isUnsafePath")?,
            requires_unsafe_path: object.get("requiresUnsafePath")?,
            is_auto_detected: object.get("isAutoDetected")?,
            is_from_path: object.get("isFromPath")?,
            args: object.get("args")?,
            env: env.map(|env| env.0),
            override_name: object.get("overrideName")?,
            color: object.get("color")?,
            icon: object.get("icon")?,
        })
    }
}

impl Wire for IProcessDetails {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new()
            .put("id", &self.id)
            .put("pid", &self.pid)
            .put("title", &self.title)
            .put("titleSource", &self.title_source)
            .put("cwd", &self.cwd)
            .put("workspaceId", &self.workspace_id)
            .put("workspaceName", &self.workspace_name)
            .put("isOrphan", &self.is_orphan)
            .put("icon", &self.icon)
            .put("color", &self.color)
            .put("fixedDimensions", &self.fixed_dimensions);
        let object = put_raw(object, "environmentVariableCollections", optional_collections_to_wire(&self.environment_variable_collections));
        object
            .put("reconnectionProperties", &self.reconnection_properties)
            .put("waitOnExit", &self.wait_on_exit)
            .put("hideFromUser", &self.hide_from_user)
            .put("isFeatureTerminal", &self.is_feature_terminal)
            .put("type", &self.type_)
            .put("hasChildProcesses", &self.has_child_processes)
            .put("shellIntegrationNonce", &self.shell_integration_nonce)
            .put("tabActions", &self.tab_actions)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process details")?;
        Ok(Self {
            id: object.get("id")?,
            pid: object.get("pid")?,
            title: object.get("title")?,
            title_source: object.get("titleSource")?,
            cwd: object.get("cwd")?,
            workspace_id: object.get("workspaceId")?,
            workspace_name: object.get("workspaceName")?,
            is_orphan: object.get("isOrphan")?,
            icon: object.get("icon")?,
            color: object.get("color")?,
            fixed_dimensions: object.get("fixedDimensions")?,
            environment_variable_collections: optional_collections_from_wire(object.raw("environmentVariableCollections"))?,
            reconnection_properties: object.get("reconnectionProperties")?,
            wait_on_exit: object.get("waitOnExit")?,
            hide_from_user: object.get("hideFromUser")?,
            is_feature_terminal: object.get("isFeatureTerminal")?,
            type_: object.get("type")?,
            has_child_processes: object.get("hasChildProcesses")?,
            shell_integration_nonce: object.get("shellIntegrationNonce")?,
            tab_actions: object.get("tabActions")?,
        })
    }
}

impl Wire for IPtyHostAttachTarget {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new()
            .put("id", &self.id)
            .put("pid", &self.pid)
            .put("title", &self.title)
            .put("titleSource", &self.title_source)
            .put("cwd", &self.cwd)
            .put("workspaceId", &self.workspace_id)
            .put("workspaceName", &self.workspace_name)
            .put("isOrphan", &self.is_orphan)
            .put("icon", &self.icon)
            .put("fixedDimensions", &self.fixed_dimensions);
        let object = put_raw(object, "environmentVariableCollections", optional_collections_to_wire(&self.environment_variable_collections));
        object
            .put("reconnectionProperties", &self.reconnection_properties)
            .put("waitOnExit", &self.wait_on_exit)
            .put("hideFromUser", &self.hide_from_user)
            .put("isFeatureTerminal", &self.is_feature_terminal)
            .put("type", &self.type_)
            .put("hasChildProcesses", &self.has_child_processes)
            .put("shellIntegrationNonce", &self.shell_integration_nonce)
            .put("tabActions", &self.tab_actions)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "attach target")?;
        Ok(Self {
            id: object.get("id")?,
            pid: object.get("pid")?,
            title: object.get("title")?,
            title_source: object.get("titleSource")?,
            cwd: object.get("cwd")?,
            workspace_id: object.get("workspaceId")?,
            workspace_name: object.get("workspaceName")?,
            is_orphan: object.get("isOrphan")?,
            icon: object.get("icon")?,
            fixed_dimensions: object.get("fixedDimensions")?,
            environment_variable_collections: optional_collections_from_wire(object.raw("environmentVariableCollections"))?,
            reconnection_properties: object.get("reconnectionProperties")?,
            wait_on_exit: object.get("waitOnExit")?,
            hide_from_user: object.get("hideFromUser")?,
            is_feature_terminal: object.get("isFeatureTerminal")?,
            type_: object.get("type")?,
            has_child_processes: object.get("hasChildProcesses")?,
            shell_integration_nonce: object.get("shellIntegrationNonce")?,
            tab_actions: object.get("tabActions")?,
        })
    }
}

impl<T: Wire> Wire for IRawTerminalInstanceLayoutInfo<T> {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("relativeSize", &self.relative_size).put("terminal", &self.terminal).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "terminal layout instance")?;
        Ok(Self { relative_size: object.get("relativeSize")?, terminal: object.get("terminal")? })
    }
}

impl<T: Wire> Wire for IRawTerminalTabLayoutInfo<T> {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("isActive", &self.is_active)
            .put("activePersistentProcessId", &self.active_persistent_process_id)
            .put("terminals", &self.terminals)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "terminal layout tab")?;
        Ok(Self { is_active: object.get("isActive")?, active_persistent_process_id: object.get("activePersistentProcessId")?, terminals: object.get("terminals")? })
    }
}

/// `background: T[] | null`: o `null` do original é o `Marshalled::Null`.
impl<T: Wire> Wire for IRawTerminalsLayoutInfo<T> {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new().put("tabs", &self.tabs);
        put_raw(object, "background", self.background.as_ref().map_or(Marshalled::Null, Wire::to_wire)).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "terminals layout")?;
        Ok(Self { tabs: object.get("tabs")?, background: object.get("background")? })
    }
}

impl Wire for ISetTerminalLayoutInfoArgs {
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new().put("workspaceId", &self.workspace_id).put("tabs", &self.tabs);
        put_raw(object, "background", self.background.as_ref().map_or(Marshalled::Null, Wire::to_wire)).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "set terminal layout args")?;
        Ok(Self { workspace_id: object.get("workspaceId")?, tabs: object.get("tabs")?, background: object.get("background")? })
    }
}

impl Wire for IGetTerminalLayoutInfoArgs {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("workspaceId", &self.workspace_id).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        Ok(Self { workspace_id: ObjectReader::new(value, "get terminal layout args")?.get("workspaceId")? })
    }
}

impl Wire for IPtyHostLatencyMeasurement {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("label", &self.label).put("latency", &self.latency).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "latency measurement")?;
        Ok(Self { label: object.get("label")?, latency: object.get("latency")? })
    }
}

impl Wire for PerformanceMark {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("name", &self.name).put("startTime", &self.start_time).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "performance mark")?;
        Ok(Self { name: object.get("name")?, start_time: object.get("startTime")? })
    }
}

impl Wire for IFreedPortProcess {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("port", &self.port).put("processId", &self.process_id).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "freed port process")?;
        Ok(Self { port: object.get("port")?, process_id: object.get("processId")? })
    }
}

impl Wire for ITerminalLaunchOutcome {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Error(error) => ObjectWriter::new().put("message", &error.message).put("code", &error.code).finish(),
            Self::Result(result) => ObjectWriter::new().put("injectedArgs", &result.injected_args).finish(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "launch outcome")?;
        if is_absent(object.raw("message")) {
            Ok(Self::Result(ITerminalLaunchResult { injected_args: object.get("injectedArgs")? }))
        } else {
            Ok(Self::Error(ITerminalLaunchError { message: object.get("message")?, code: object.get("code")? }))
        }
    }
}

impl Wire for IRequestResolveVariablesEvent {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("requestId", &self.request_id).put("workspaceId", &self.workspace_id).put("originalText", &self.original_text).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "resolve variables event")?;
        Ok(Self { request_id: object.get("requestId")?, workspace_id: object.get("workspaceId")?, original_text: object.get("originalText")? })
    }
}

impl Wire for IProcessReadyEvent {
    fn to_wire(&self) -> Marshalled {
        let windows_pty = self.windows_pty.map(|pty| ObjectWriter::new().put("backend", &"conpty".to_owned()).put("buildNumber", &pty.build_number).finish());
        put_raw(ObjectWriter::new().put("pid", &self.pid).put("cwd", &self.cwd), "windowsPty", windows_pty.unwrap_or(Marshalled::Undefined)).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process ready event")?;
        let windows_pty = match object.raw("windowsPty") {
            absent if is_absent(absent) => None,
            pty => {
                let pty = ObjectReader::new(pty, "windows pty")?;
                Some(IProcessReadyWindowsPty { backend: WindowsPtyBackend::Conpty, build_number: pty.get("buildNumber")? })
            }
        };
        Ok(Self { pid: object.get("pid")?, cwd: object.get("cwd")?, windows_pty })
    }
}

impl Wire for IProcessProperty {
    /// `{ type, value }`.
    fn to_wire(&self) -> Marshalled {
        let object = ObjectWriter::new().put("type", &self.property_type());
        let object = match self {
            Self::Cwd(value) | Self::InitialCwd(value) | Self::Title(value) => object.put("value", value),
            Self::FixedDimensions(value) => object.put("value", value),
            Self::ShellType(value) => object.put("value", value),
            Self::HasChildProcesses(value) => object.put("value", value),
            Self::ResolvedShellLaunchConfig(value) => object.put("value", value),
            Self::OverrideDimensions(value) => object.put("value", value),
            Self::FailedShellIntegrationActivation(value) | Self::UsedShellIntegrationInjection(value) => object.put("value", value),
            Self::ShellIntegrationInjectionFailureReason(value) => object.put("value", value),
        };
        object.finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process property")?;
        Ok(match object.get::<ProcessPropertyType>("type")? {
            ProcessPropertyType::Cwd => Self::Cwd(object.get("value")?),
            ProcessPropertyType::InitialCwd => Self::InitialCwd(object.get("value")?),
            ProcessPropertyType::FixedDimensions => Self::FixedDimensions(object.get("value")?),
            ProcessPropertyType::Title => Self::Title(object.get("value")?),
            ProcessPropertyType::ShellType => Self::ShellType(object.get("value")?),
            ProcessPropertyType::HasChildProcesses => Self::HasChildProcesses(object.get("value")?),
            ProcessPropertyType::ResolvedShellLaunchConfig => Self::ResolvedShellLaunchConfig(object.get("value")?),
            ProcessPropertyType::OverrideDimensions => Self::OverrideDimensions(object.get("value")?),
            ProcessPropertyType::FailedShellIntegrationActivation => Self::FailedShellIntegrationActivation(object.get("value")?),
            ProcessPropertyType::UsedShellIntegrationInjection => Self::UsedShellIntegrationInjection(object.get("value")?),
            ProcessPropertyType::ShellIntegrationInjectionFailureReason => Self::ShellIntegrationInjectionFailureReason(object.get("value")?),
        })
    }
}

impl Wire for ReplayEntry {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("cols", &self.cols).put("rows", &self.rows).put("data", &self.data).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "replay entry")?;
        Ok(Self { cols: object.get("cols")?, rows: object.get("rows")?, data: object.get("data")? })
    }
}

impl Wire for IMarkProperties {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("hoverMessage", &self.hover_message)
            .put("disableCommandStorage", &self.disable_command_storage)
            .put("hidden", &self.hidden)
            .put("id", &self.id)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "mark properties")?;
        Ok(Self { hover_message: object.get("hoverMessage")?, disable_command_storage: object.get("disableCommandStorage")?, hidden: object.get("hidden")?, marker: None, id: object.get("id")? })
    }
}

impl Wire for ISerializedTerminalCommand {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("command", &self.command)
            .put("commandLineConfidence", &self.command_line_confidence)
            .put("isTrusted", &self.is_trusted)
            .put("timestamp", &self.timestamp)
            .put("duration", &self.duration)
            .put("id", &self.id)
            .put("cwd", &self.cwd)
            .put("exitCode", &self.exit_code)
            .put("commandStartLineContent", &self.command_start_line_content)
            .put("markProperties", &self.mark_properties)
            .put("executedX", &self.executed_x)
            .put("startX", &self.start_x)
            .put("startLine", &self.start_line)
            .put("promptStartLine", &self.prompt_start_line)
            .put("endLine", &self.end_line)
            .put("executedLine", &self.executed_line)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "serialized command")?;
        Ok(Self {
            command: object.get("command")?,
            command_line_confidence: object.get("commandLineConfidence")?,
            is_trusted: object.get("isTrusted")?,
            timestamp: object.get("timestamp")?,
            duration: object.get("duration")?,
            id: object.get("id")?,
            cwd: object.get("cwd")?,
            exit_code: object.get("exitCode")?,
            command_start_line_content: object.get("commandStartLineContent")?,
            mark_properties: object.get("markProperties")?,
            executed_x: object.get("executedX")?,
            start_x: object.get("startX")?,
            start_line: object.get("startLine")?,
            prompt_start_line: object.get("promptStartLine")?,
            end_line: object.get("endLine")?,
            executed_line: object.get("executedLine")?,
        })
    }
}

impl Wire for IPromptInputModelState {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("value", &self.value)
            .put("prefix", &self.prefix)
            .put("suffix", &self.suffix)
            .put("cursorIndex", &self.cursor_index)
            .put("ghostTextIndex", &self.ghost_text_index)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "prompt input model state")?;
        Ok(Self {
            value: object.get("value")?,
            prefix: object.get("prefix")?,
            suffix: object.get("suffix")?,
            cursor_index: object.get("cursorIndex")?,
            ghost_text_index: object.get("ghostTextIndex")?,
        })
    }
}

impl Wire for ISerializedPromptInputModel {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("modelState", &self.model_state)
            .put("commandStartX", &self.command_start_x)
            .put("lastPromptLine", &self.last_prompt_line)
            .put("continuationPrompt", &self.continuation_prompt)
            .put("lastUserInput", &self.last_user_input)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "serialized prompt input model")?;
        Ok(Self {
            model_state: object.get("modelState")?,
            command_start_x: object.get("commandStartX")?,
            last_prompt_line: object.get("lastPromptLine")?,
            continuation_prompt: object.get("continuationPrompt")?,
            last_user_input: object.get("lastUserInput")?,
        })
    }
}

impl Wire for ISerializedCommandDetectionCapability {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("isWindowsPty", &self.is_windows_pty)
            .put("hasRichCommandDetection", &self.has_rich_command_detection)
            .put("commands", &self.commands)
            .put("promptInputModel", &self.prompt_input_model)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "serialized command detection")?;
        Ok(Self {
            is_windows_pty: object.get("isWindowsPty")?,
            has_rich_command_detection: object.get("hasRichCommandDetection")?,
            commands: object.get("commands")?,
            prompt_input_model: object.get("promptInputModel")?,
        })
    }
}

impl Wire for IPtyHostProcessReplayEvent {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("events", &self.events).put("commands", &self.commands).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "replay event")?;
        Ok(Self { events: object.get("events")?, commands: object.get("commands")? })
    }
}

impl Wire for IPersistentTerminalProcessLaunchConfig {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("env", &self.env).put("executableEnv", &self.executable_env).put("options", &self.options).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "persistent launch config")?;
        Ok(Self { env: object.get("env")?, executable_env: object.get("executableEnv")?, options: object.get("options")? })
    }
}

impl Wire for ISerializedTerminalState {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new()
            .put("id", &self.id)
            .put("shellLaunchConfig", &self.shell_launch_config)
            .put("processDetails", &self.process_details)
            .put("processLaunchConfig", &self.process_launch_config)
            .put("unicodeVersion", &self.unicode_version)
            .put("replayEvent", &self.replay_event)
            .put("timestamp", &self.timestamp)
            .finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "serialized terminal state")?;
        Ok(Self {
            id: object.get("id")?,
            shell_launch_config: object.get("shellLaunchConfig")?,
            process_details: object.get("processDetails")?,
            process_launch_config: object.get("processLaunchConfig")?,
            unicode_version: object.get("unicodeVersion")?,
            replay_event: object.get("replayEvent")?,
            timestamp: object.get("timestamp")?,
        })
    }
}

/// `string | IProcessDataEvent`.
impl Wire for ProcessData {
    fn to_wire(&self) -> Marshalled {
        match self {
            Self::Text(text) => Marshalled::String(js(text)),
            Self::Event(event) => ObjectWriter::new().put("data", &*event.data.borrow()).put("trackCommit", &event.track_commit).finish(),
        }
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        match value {
            Marshalled::String(text) => Ok(Self::Text(text.to_string_lossy())),
            other => {
                let object = ObjectReader::new(other, "process data event")?;
                Ok(Self::Event(IProcessDataEvent::new(object.get::<String>("data")?, object.get("trackCommit")?)))
            }
        }
    }
}

impl Wire for PtyProcessData {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("event", &self.event).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process data")?;
        Ok(Self { id: object.get("id")?, event: object.get("event")? })
    }
}

impl Wire for PtyProcessReady {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("event", &self.event).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process ready")?;
        Ok(Self { id: object.get("id")?, event: object.get("event")? })
    }
}

impl Wire for PtyProcessReplay {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("event", &self.event).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process replay")?;
        Ok(Self { id: object.get("id")?, event: object.get("event")? })
    }
}

impl Wire for PtyProcessOrphanQuestion {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        Ok(Self { id: ObjectReader::new(value, "orphan question")?.get("id")? })
    }
}

impl Wire for PtyDidRequestDetach {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("requestId", &self.request_id).put("workspaceId", &self.workspace_id).put("instanceId", &self.instance_id).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "detach request")?;
        Ok(Self { request_id: object.get("requestId")?, workspace_id: object.get("workspaceId")?, instance_id: object.get("instanceId")? })
    }
}

impl Wire for PtyDidChangeProperty {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("property", &self.property).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "property change")?;
        Ok(Self { id: object.get("id")?, property: object.get("property")? })
    }
}

impl Wire for PtyProcessExit {
    fn to_wire(&self) -> Marshalled {
        ObjectWriter::new().put("id", &self.id).put("event", &self.event).finish()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        let object = ObjectReader::new(value, "process exit")?;
        Ok(Self { id: object.get("id")?, event: object.get("event")? })
    }
}

/// Um `Rc` de valor compartilhado atravessa como o valor.
impl<T: Wire> Wire for Rc<T> {
    fn to_wire(&self) -> Marshalled {
        (**self).to_wire()
    }

    fn from_wire(value: &Marshalled) -> Result<Self, Error> {
        T::from_wire(value).map(Rc::new)
    }
}
