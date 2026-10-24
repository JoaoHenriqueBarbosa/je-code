//! Porte dos breakpoints de `src/vs/workbench/contrib/debug/common/debugModel.ts`: `Enablement`, `BaseBreakpoint`,
//! `Breakpoint`, `FunctionBreakpoint`, `DataBreakpoint`, `ExceptionBreakpoint`, `InstructionBreakpoint`.
//!
//! Notas de tradução:
//! - A herança (`BaseBreakpoint extends Enablement`, `Breakpoint extends BaseBreakpoint`...) é composição: cada classe
//!   tem um [`BaseBreakpoint`] em `base`, e os campos mutáveis do original (`enabled`, `condition`, `hitCondition`...) são
//!   `Cell`/`RefCell`, porque os breakpoints são compartilhados por `Rc` entre o modelo, a sessão e os eventos.
//! - Os campos `?`/`undefined` são `Option`. Em [`IBreakpointUpdateData`](super::debug_types::IBreakpointUpdateData) o campo
//!   `Option<Option<..>>` guarda o `hasOwnProperty` (externo) e o valor (interno).
//! - `ITextFileService` é o recorte [`IDirtyChecker`] (só o `isDirty`); `IUriIdentityService` e `ILogService` são as
//!   referências `Rc` do original.
//! - `toJSON()` devolve o `serde_json::Value` do objeto que o `JSON.stringify` serializaria (o `uri` é o texto da URI).
//! - `DataBreakpoint.sessionDataIdForAddr` (`WeakMap<IDebugSession, string | null>`) é um mapa pelo id da sessão.
//! - `InstructionBreakpoint.address` (`bigint`) é `u128`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::log::LogService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use serde_json::{Value, json};

use super::debug::DataBreakpointSource;
use super::debug_protocol::{DataBreakpointAccessType, Source as ProtocolSource};
use super::debug_source::get_uri_from_source;
use super::debug_types::{IBreakpointUpdateData, IDebugSessionFull, capability};

/// O recorte de `ITextFileService` que os breakpoints leem.
pub trait IDirtyChecker {
    fn is_dirty(&self, resource: &URI) -> bool;
}

/// `DebugProtocol.Breakpoint`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DapBreakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub source: Option<ProtocolSource>,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    pub instruction_reference: Option<String>,
    pub offset: Option<i64>,
}

impl DapBreakpoint {
    /// O objeto `Breakpoint` do DAP (corpo de resposta ou de evento).
    pub fn from_value(value: &Value) -> Self {
        let number = |key: &str| value.get(key).and_then(Value::as_i64);
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            id: number("id"),
            verified: value.get("verified").and_then(Value::as_bool).unwrap_or(false),
            message: text("message"),
            source: value.get("source").filter(|s| s.is_object()).map(protocol_source_from_value),
            line: number("line"),
            column: number("column"),
            end_line: number("endLine"),
            end_column: number("endColumn"),
            instruction_reference: text("instructionReference"),
            offset: number("offset"),
        }
    }
}

/// `DebugProtocol.Source` a partir do objeto do DAP.
pub fn protocol_source_from_value(value: &Value) -> ProtocolSource {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    ProtocolSource {
        name: text("name"),
        path: text("path"),
        source_reference: value.get("sourceReference").and_then(Value::as_i64),
        presentation_hint: match value.get("presentationHint").and_then(Value::as_str) {
            Some("normal") => Some(super::debug_protocol::SourcePresentationHint::Normal),
            Some("emphasize") => Some(super::debug_protocol::SourcePresentationHint::Emphasize),
            Some("deemphasize") => Some(super::debug_protocol::SourcePresentationHint::Deemphasize),
            _ => None,
        },
        origin: text("origin"),
        sources: value.get("sources").and_then(Value::as_array).map(|sources| sources.iter().map(protocol_source_from_value).collect()),
        adapter_data: value.get("adapterData").cloned(),
        checksums: None,
    }
}

/// `DebugProtocol.Source` como objeto do DAP (só os campos presentes).
pub fn protocol_source_to_value(source: &ProtocolSource) -> Value {
    let mut object = serde_json::Map::new();
    if let Some(name) = &source.name {
        object.insert("name".into(), json!(name));
    }
    if let Some(path) = &source.path {
        object.insert("path".into(), json!(path));
    }
    if let Some(reference) = source.source_reference {
        object.insert("sourceReference".into(), json!(reference));
    }
    if let Some(origin) = &source.origin {
        object.insert("origin".into(), json!(origin));
    }
    if let Some(adapter_data) = &source.adapter_data {
        object.insert("adapterData".into(), adapter_data.clone());
    }
    Value::Object(object)
}

/// `IBreakpointSessionData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBreakpointSessionData {
    pub data: DapBreakpoint,
    pub supports_conditional_breakpoints: bool,
    pub supports_hit_conditional_breakpoints: bool,
    pub supports_log_points: bool,
    pub supports_function_breakpoints: bool,
    pub supports_data_breakpoints: bool,
    pub supports_instruction_breakpoints: bool,
    pub session_id: String,
}

/// `toBreakpointSessionData(data, capabilities)`.
pub fn to_breakpoint_session_data(data: DapBreakpoint, capabilities: &Value) -> IBreakpointSessionData {
    IBreakpointSessionData {
        data,
        supports_conditional_breakpoints: capability(capabilities, "supportsConditionalBreakpoints"),
        supports_hit_conditional_breakpoints: capability(capabilities, "supportsHitConditionalBreakpoints"),
        supports_log_points: capability(capabilities, "supportsLogPoints"),
        supports_function_breakpoints: capability(capabilities, "supportsFunctionBreakpoints"),
        supports_data_breakpoints: capability(capabilities, "supportsDataBreakpoints"),
        supports_instruction_breakpoints: capability(capabilities, "supportsInstructionBreakpoints"),
        session_id: String::new(),
    }
}

/// `Enablement`.
pub struct Enablement {
    pub enabled: Cell<bool>,
    id: String,
}

impl Enablement {
    pub fn new(enabled: bool, id: String) -> Self {
        Self { enabled: Cell::new(enabled), id }
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }
}

/// `IBaseBreakpointOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IBaseBreakpointOptions {
    pub enabled: Option<bool>,
    pub hit_condition: Option<String>,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub mode: Option<String>,
    pub mode_label: Option<String>,
}

impl IBaseBreakpointOptions {
    /// Lê as opções comuns de um objeto JSON (o `toJSON()` que o armazenamento gravou).
    pub fn from_json(value: &Value) -> Self {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            enabled: value.get("enabled").and_then(Value::as_bool),
            hit_condition: text("hitCondition"),
            condition: text("condition"),
            log_message: text("logMessage"),
            mode: text("mode"),
            mode_label: text("modeLabel"),
        }
    }
}

/// `BaseBreakpoint` (sem a herança de `Enablement`: `enablement` é o membro).
pub struct BaseBreakpoint {
    pub enablement: Enablement,
    session_data: RefCell<Vec<(String, IBreakpointSessionData)>>,
    pub data: RefCell<Option<IBreakpointSessionData>>,
    pub hit_condition: RefCell<Option<String>>,
    pub condition: RefCell<Option<String>>,
    pub log_message: RefCell<Option<String>>,
    pub mode: RefCell<Option<String>>,
    pub mode_label: RefCell<Option<String>>,
}

impl BaseBreakpoint {
    pub fn new(id: String, opts: &IBaseBreakpointOptions) -> Self {
        Self {
            enablement: Enablement::new(opts.enabled.unwrap_or(true), id),
            session_data: RefCell::new(Vec::new()),
            data: RefCell::new(None),
            hit_condition: RefCell::new(opts.hit_condition.clone()),
            condition: RefCell::new(opts.condition.clone()),
            log_message: RefCell::new(opts.log_message.clone()),
            mode: RefCell::new(opts.mode.clone()),
            mode_label: RefCell::new(opts.mode_label.clone()),
        }
    }

    pub fn get_id(&self) -> String {
        self.enablement.get_id()
    }

    pub fn enabled(&self) -> bool {
        self.enablement.enabled.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enablement.enabled.set(enabled);
    }

    /// `setSessionData(sessionId, data)`.
    pub fn set_session_data(&self, session_id: &str, data: Option<IBreakpointSessionData>) {
        {
            let mut session_data = self.session_data.borrow_mut();
            match data {
                None => session_data.retain(|(id, _)| id != session_id),
                Some(mut data) => {
                    data.session_id = session_id.to_owned();
                    match session_data.iter_mut().find(|(id, _)| id == session_id) {
                        Some(entry) => entry.1 = data,
                        None => session_data.push((session_id.to_owned(), data)),
                    }
                }
            }
        }

        let all_data: Vec<IBreakpointSessionData> = self.session_data.borrow().iter().map(|(_, data)| data.clone()).collect();
        let mut seen = HashSet::new();
        let verified_data: Vec<&IBreakpointSessionData> = all_data
            .iter()
            .filter(|d| d.data.verified)
            .filter(|d| seen.insert((d.data.line, d.data.column)))
            .collect();
        *self.data.borrow_mut() = if !verified_data.is_empty() {
            // In case multiple session verified the breakpoint and they provide different data show the intial data that the user set (corner case)
            if verified_data.len() == 1 { Some(verified_data[0].clone()) } else { None }
        } else {
            // No session verified the breakpoint
            all_data.first().cloned()
        };
    }

    pub fn message(&self) -> Option<String> {
        self.data.borrow().as_ref().and_then(|data| data.data.message.clone())
    }

    pub fn verified(&self) -> bool {
        self.data.borrow().as_ref().is_none_or(|data| data.data.verified)
    }

    pub fn sessions_that_verified(&self) -> Vec<String> {
        self.session_data.borrow().iter().filter(|(_, data)| data.data.verified).map(|(id, _)| id.clone()).collect()
    }

    pub fn get_id_from_adapter(&self, session_id: &str) -> Option<i64> {
        self.session_data.borrow().iter().find(|(id, _)| id == session_id).and_then(|(_, data)| data.data.id)
    }

    pub fn get_debug_protocol_breakpoint(&self, session_id: &str) -> Option<DapBreakpoint> {
        self.session_data.borrow().iter().find(|(id, _)| id == session_id).map(|(_, data)| DapBreakpoint {
            id: data.data.id,
            verified: data.data.verified,
            message: data.data.message.clone(),
            source: data.data.source.clone(),
            line: data.data.line,
            column: data.data.column,
            end_line: data.data.end_line,
            end_column: data.data.end_column,
            instruction_reference: data.data.instruction_reference.clone(),
            offset: data.data.offset,
        })
    }

    /// `toJSON()` da classe base.
    pub fn to_json(&self) -> serde_json::Map<String, Value> {
        let mut object = serde_json::Map::new();
        object.insert("id".into(), json!(self.get_id()));
        object.insert("enabled".into(), json!(self.enabled()));
        for (key, value) in [
            ("condition", &self.condition),
            ("hitCondition", &self.hit_condition),
            ("logMessage", &self.log_message),
            ("mode", &self.mode),
            ("modeLabel", &self.mode_label),
        ] {
            if let Some(text) = value.borrow().as_ref() {
                object.insert(key.into(), json!(text));
            }
        }
        object
    }
}

/// `IBreakpointOptions`.
#[derive(Clone, Debug)]
pub struct IBreakpointOptions {
    pub base: IBaseBreakpointOptions,
    pub uri: URI,
    pub line_number: i64,
    pub column: Option<i64>,
    pub adapter_data: Option<Value>,
    pub triggered_by: Option<String>,
}

/// `Breakpoint`.
pub struct Breakpoint {
    pub base: BaseBreakpoint,
    sessions_did_trigger: RefCell<Option<HashSet<String>>>,
    uri_value: URI,
    adapter_data_value: RefCell<Option<Value>>,
    line_number_value: Cell<i64>,
    column_value: Cell<Option<i64>>,
    pub triggered_by: RefCell<Option<String>>,
    text_file_service: Rc<dyn IDirtyChecker>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: Rc<dyn LogService>,
}

impl Breakpoint {
    /// O construtor: `id` ausente gera um uuid.
    pub fn new(
        opts: IBreakpointOptions,
        text_file_service: Rc<dyn IDirtyChecker>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log_service: Rc<dyn LogService>,
        id: Option<String>,
    ) -> Rc<Self> {
        Rc::new(Self {
            base: BaseBreakpoint::new(id.unwrap_or_else(generate_uuid), &opts.base),
            sessions_did_trigger: RefCell::new(None),
            uri_value: opts.uri,
            adapter_data_value: RefCell::new(opts.adapter_data),
            line_number_value: Cell::new(opts.line_number),
            column_value: Cell::new(opts.column),
            triggered_by: RefCell::new(opts.triggered_by),
            text_file_service,
            uri_identity_service,
            log_service,
        })
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    pub fn enabled(&self) -> bool {
        self.base.enabled()
    }

    pub fn condition(&self) -> Option<String> {
        self.base.condition.borrow().clone()
    }

    pub fn hit_condition(&self) -> Option<String> {
        self.base.hit_condition.borrow().clone()
    }

    pub fn log_message(&self) -> Option<String> {
        self.base.log_message.borrow().clone()
    }

    pub fn mode(&self) -> Option<String> {
        self.base.mode.borrow().clone()
    }

    /// `toDAP()` (`SourceBreakpoint`).
    pub fn to_dap(&self) -> Value {
        let (line_number, column) = self.session_agnostic_data();
        let mut object = serde_json::Map::new();
        object.insert("line".into(), json!(line_number));
        if let Some(column) = column {
            object.insert("column".into(), json!(column));
        }
        for (key, value) in [("condition", self.condition()), ("hitCondition", self.hit_condition()), ("logMessage", self.log_message()), ("mode", self.mode())] {
            if let Some(text) = value {
                object.insert(key.into(), json!(text));
            }
        }
        Value::Object(object)
    }

    pub fn original_uri(&self) -> URI {
        self.uri_value.clone()
    }

    pub fn line_number(&self) -> i64 {
        match self.base.data.borrow().as_ref() {
            Some(data) if self.verified() => data.data.line.unwrap_or_else(|| self.line_number_value.get()),
            _ => self.line_number_value.get(),
        }
    }

    pub fn verified(&self) -> bool {
        match self.base.data.borrow().as_ref() {
            Some(data) => data.data.verified && !self.text_file_service.is_dirty(&self.uri_value),
            None => true,
        }
    }

    pub fn pending(&self) -> bool {
        if self.base.data.borrow().is_some() {
            return false;
        }
        self.triggered_by.borrow().is_some()
    }

    pub fn uri(&self) -> URI {
        if self.verified()
            && let Some(data) = self.base.data.borrow().as_ref()
            && let Some(source) = &data.data.source
        {
            return get_uri_from_source(source, source.path.as_deref(), &data.session_id, &*self.uri_identity_service, &*self.log_service);
        }
        self.uri_value.clone()
    }

    pub fn column(&self) -> Option<i64> {
        match self.base.data.borrow().as_ref() {
            Some(data) if self.verified() && data.data.column.is_some() => data.data.column,
            _ => self.column_value.get(),
        }
    }

    pub fn message(&self) -> Option<String> {
        if self.text_file_service.is_dirty(&self.uri()) {
            return Some(localize("breakpointDirtydHover", "Unverified breakpoint. File is modified, please restart debug session.", &[]));
        }
        self.base.message()
    }

    pub fn adapter_data(&self) -> Option<Value> {
        if let Some(data) = self.base.data.borrow().as_ref()
            && let Some(adapter_data) = data.data.source.as_ref().and_then(|source| source.adapter_data.clone())
        {
            return Some(adapter_data);
        }
        self.adapter_data_value.borrow().clone()
    }

    pub fn end_line_number(&self) -> Option<i64> {
        self.base.data.borrow().as_ref().filter(|_| self.verified()).and_then(|data| data.data.end_line)
    }

    pub fn end_column(&self) -> Option<i64> {
        self.base.data.borrow().as_ref().filter(|_| self.verified()).and_then(|data| data.data.end_column)
    }

    /// `sessionAgnosticData`: `{ lineNumber, column }`.
    pub fn session_agnostic_data(&self) -> (i64, Option<i64>) {
        (self.line_number_value.get(), self.column_value.get())
    }

    pub fn supported(&self) -> bool {
        let data = self.base.data.borrow();
        let Some(data) = data.as_ref() else {
            return true;
        };
        if self.log_message().is_some_and(|m| !m.is_empty()) && !data.supports_log_points {
            return false;
        }
        if self.condition().is_some_and(|m| !m.is_empty()) && !data.supports_conditional_breakpoints {
            return false;
        }
        if self.hit_condition().is_some_and(|m| !m.is_empty()) && !data.supports_hit_conditional_breakpoints {
            return false;
        }
        true
    }

    pub fn set_session_data(&self, session_id: &str, data: Option<IBreakpointSessionData>) {
        self.base.set_session_data(session_id, data);
        if self.adapter_data_value.borrow().is_none() {
            let adapter_data = self.adapter_data();
            *self.adapter_data_value.borrow_mut() = adapter_data;
        }
    }

    pub fn to_json(&self) -> Value {
        let mut object = self.base.to_json();
        object.insert("uri".into(), uri_to_value(&self.uri_value));
        object.insert("lineNumber".into(), json!(self.line_number_value.get()));
        if let Some(column) = self.column_value.get() {
            object.insert("column".into(), json!(column));
        }
        if let Some(adapter_data) = self.adapter_data() {
            object.insert("adapterData".into(), adapter_data);
        }
        if let Some(triggered_by) = self.triggered_by.borrow().as_ref() {
            object.insert("triggeredBy".into(), json!(triggered_by));
        }
        Value::Object(object)
    }

    pub fn to_display_string(&self) -> String {
        format!("{} {}", ext_uri().basename_or_authority(&self.uri()), self.line_number())
    }

    pub fn set_session_did_trigger(&self, session_id: &str, did_trigger: bool) {
        if did_trigger {
            self.sessions_did_trigger.borrow_mut().get_or_insert_with(HashSet::new).insert(session_id.to_owned());
        } else if let Some(set) = self.sessions_did_trigger.borrow_mut().as_mut() {
            set.remove(session_id);
        }
    }

    pub fn get_session_did_trigger(&self, session_id: &str) -> bool {
        self.sessions_did_trigger.borrow().as_ref().is_some_and(|set| set.contains(session_id))
    }

    /// `update(data)`.
    pub fn update(&self, data: &IBreakpointUpdateData) {
        if let Some(Some(line_number)) = data.line_number {
            self.line_number_value.set(line_number);
        }
        if let Some(column) = data.column {
            self.column_value.set(column);
        }
        if let Some(condition) = &data.condition {
            *self.base.condition.borrow_mut() = condition.clone();
        }
        if let Some(hit_condition) = &data.hit_condition {
            *self.base.hit_condition.borrow_mut() = hit_condition.clone();
        }
        if let Some(log_message) = &data.log_message {
            *self.base.log_message.borrow_mut() = log_message.clone();
        }
        if let Some(mode) = &data.mode {
            *self.base.mode.borrow_mut() = mode.clone();
            *self.base.mode_label.borrow_mut() = data.mode_label.clone().flatten();
        }
        if let Some(triggered_by) = &data.triggered_by {
            *self.triggered_by.borrow_mut() = triggered_by.clone();
            *self.sessions_did_trigger.borrow_mut() = None;
        }
    }

}

/// `IBreakpointData` de `debug.ts` (com o `triggeredBy` e o `modeLabel` que a struct mínima de `debug.rs` não tem).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakpointData {
    pub id: Option<String>,
    pub line_number: i64,
    pub column: Option<i64>,
    pub enabled: Option<bool>,
    pub condition: Option<String>,
    pub log_message: Option<String>,
    pub hit_condition: Option<String>,
    pub triggered_by: Option<String>,
    pub mode: Option<String>,
    pub mode_label: Option<String>,
}

/// `IFunctionBreakpointOptions` do modelo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FunctionBreakpointOptions {
    pub base: IBaseBreakpointOptions,
    pub name: String,
}

/// `FunctionBreakpoint`.
pub struct FunctionBreakpoint {
    pub base: BaseBreakpoint,
    pub name: RefCell<String>,
}

impl FunctionBreakpoint {
    pub fn new(opts: FunctionBreakpointOptions, id: Option<String>) -> Rc<Self> {
        Rc::new(Self { base: BaseBreakpoint::new(id.unwrap_or_else(generate_uuid), &opts.base), name: RefCell::new(opts.name) })
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    pub fn to_dap(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("name".into(), json!(self.name.borrow().clone()));
        if let Some(condition) = self.base.condition.borrow().as_ref() {
            object.insert("condition".into(), json!(condition));
        }
        if let Some(hit_condition) = self.base.hit_condition.borrow().as_ref() {
            object.insert("hitCondition".into(), json!(hit_condition));
        }
        Value::Object(object)
    }

    pub fn to_json(&self) -> Value {
        let mut object = self.base.to_json();
        object.insert("name".into(), json!(self.name.borrow().clone()));
        Value::Object(object)
    }

    pub fn supported(&self) -> bool {
        self.base.data.borrow().as_ref().is_none_or(|data| data.supports_function_breakpoints)
    }

    pub fn to_display_string(&self) -> String {
        self.name.borrow().clone()
    }
}

/// `IDataBreakpointOptions` do modelo.
#[derive(Clone, Debug)]
pub struct DataBreakpointOptions {
    pub base: IBaseBreakpointOptions,
    pub description: String,
    pub src: DataBreakpointSource,
    pub can_persist: bool,
    /// `initialSessionData`: o id da sessão e o `dataId`.
    pub initial_session_data: Option<(String, String)>,
    pub access_types: Option<Vec<DataBreakpointAccessType>>,
    pub access_type: DataBreakpointAccessType,
}

/// `DataBreakpoint`.
pub struct DataBreakpoint {
    pub base: BaseBreakpoint,
    session_data_id_for_addr: RefCell<HashMap<String, Option<String>>>,
    pub description: String,
    pub src: DataBreakpointSource,
    pub can_persist: bool,
    pub access_types: Option<Vec<DataBreakpointAccessType>>,
    pub access_type: DataBreakpointAccessType,
}

impl DataBreakpoint {
    pub fn new(opts: DataBreakpointOptions, id: Option<String>) -> Rc<Self> {
        let mut map = HashMap::new();
        if let Some((session_id, data_id)) = opts.initial_session_data.clone() {
            map.insert(session_id, Some(data_id));
        }
        Rc::new(Self {
            base: BaseBreakpoint::new(id.unwrap_or_else(generate_uuid), &opts.base),
            session_data_id_for_addr: RefCell::new(map),
            description: opts.description,
            src: opts.src,
            can_persist: opts.can_persist,
            access_types: opts.access_types,
            access_type: opts.access_type,
        })
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    /// `toDAP(session)`.
    pub fn to_dap(self: &Rc<Self>, session: Rc<dyn IDebugSessionFull>) -> Promise<Option<Value>> {
        let this = self.clone();
        je_base::async_::async_fn(async move {
            let data_id = match &this.src {
                DataBreakpointSource::Variable { data_id } => data_id.clone(),
                DataBreakpointSource::Address { address, bytes } => {
                    let session_id = session.get_id();
                    let cached = this.session_data_id_for_addr.borrow().get(&session_id).cloned().flatten().filter(|id| !id.is_empty());
                    match cached {
                        Some(id) => id,
                        None => {
                            let info = session.data_bytes_breakpoint_info(address, *bytes).await?;
                            match info.and_then(|info| info.data_id).filter(|id| !id.is_empty()) {
                                Some(id) => {
                                    this.session_data_id_for_addr.borrow_mut().insert(session_id, Some(id.clone()));
                                    id
                                }
                                None => return Ok(None),
                            }
                        }
                    }
                }
            };
            let mut object = serde_json::Map::new();
            object.insert("dataId".into(), json!(data_id));
            object.insert("accessType".into(), json!(access_type_text(this.access_type)));
            if let Some(condition) = this.base.condition.borrow().as_ref() {
                object.insert("condition".into(), json!(condition));
            }
            if let Some(hit_condition) = this.base.hit_condition.borrow().as_ref() {
                object.insert("hitCondition".into(), json!(hit_condition));
            }
            Ok(Some(Value::Object(object)))
        })
    }

    pub fn to_json(&self) -> Value {
        let mut object = self.base.to_json();
        object.insert("description".into(), json!(self.description));
        object.insert(
            "src".into(),
            match &self.src {
                DataBreakpointSource::Variable { data_id } => json!({ "type": 0, "dataId": data_id }),
                DataBreakpointSource::Address { address, bytes } => json!({ "type": 1, "address": address, "bytes": bytes }),
            },
        );
        if let Some(access_types) = &self.access_types {
            object.insert("accessTypes".into(), json!(access_types.iter().map(|t| access_type_text(*t)).collect::<Vec<_>>()));
        }
        object.insert("accessType".into(), json!(access_type_text(self.access_type)));
        object.insert("canPersist".into(), json!(self.can_persist));
        Value::Object(object)
    }

    pub fn supported(&self) -> bool {
        self.base.data.borrow().as_ref().is_none_or(|data| data.supports_data_breakpoints)
    }

    pub fn to_display_string(&self) -> String {
        self.description.clone()
    }
}

/// `'read' | 'write' | 'readWrite'`.
pub fn access_type_text(access_type: DataBreakpointAccessType) -> &'static str {
    match access_type {
        DataBreakpointAccessType::Read => "read",
        DataBreakpointAccessType::Write => "write",
        DataBreakpointAccessType::ReadWrite => "readWrite",
    }
}

/// `DebugProtocol.ExceptionBreakpointsFilter`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExceptionBreakpointsFilter {
    pub filter: String,
    pub label: String,
    pub description: Option<String>,
    pub default: Option<bool>,
    pub supports_condition: Option<bool>,
    pub condition_description: Option<String>,
}

impl ExceptionBreakpointsFilter {
    pub fn from_value(value: &Value) -> Self {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            filter: text("filter").unwrap_or_default(),
            label: text("label").unwrap_or_default(),
            description: text("description"),
            default: value.get("default").and_then(Value::as_bool),
            supports_condition: value.get("supportsCondition").and_then(Value::as_bool),
            condition_description: text("conditionDescription"),
        }
    }
}

/// `IExceptionBreakpointOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExceptionBreakpointOptions {
    pub base: IBaseBreakpointOptions,
    pub filter: String,
    pub label: String,
    pub supports_condition: bool,
    pub description: Option<String>,
    pub condition_description: Option<String>,
    pub fallback: Option<bool>,
}

impl ExceptionBreakpointOptions {
    /// O objeto que `toJSON()` gravou (o `new ExceptionBreakpoint(parsed)` do teste).
    pub fn from_json(value: &Value) -> Self {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            base: IBaseBreakpointOptions::from_json(value),
            filter: text("filter").unwrap_or_default(),
            label: text("label").unwrap_or_default(),
            supports_condition: value.get("supportsCondition").and_then(Value::as_bool).unwrap_or(false),
            description: text("description"),
            condition_description: text("conditionDescription"),
            fallback: value.get("fallback").and_then(Value::as_bool),
        }
    }
}

/// `ExceptionBreakpoint`.
pub struct ExceptionBreakpoint {
    pub base: BaseBreakpoint,
    supported_sessions: RefCell<HashSet<String>>,
    pub filter: String,
    pub label: String,
    pub supports_condition: bool,
    pub description: Option<String>,
    pub condition_description: Option<String>,
    fallback: Cell<bool>,
}

impl ExceptionBreakpoint {
    pub fn new(opts: ExceptionBreakpointOptions, id: Option<String>) -> Rc<Self> {
        Rc::new(Self {
            base: BaseBreakpoint::new(id.unwrap_or_else(generate_uuid), &opts.base),
            supported_sessions: RefCell::new(HashSet::new()),
            filter: opts.filter,
            label: opts.label,
            supports_condition: opts.supports_condition,
            description: opts.description,
            condition_description: opts.condition_description,
            fallback: Cell::new(opts.fallback.unwrap_or(false)),
        })
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    pub fn to_json(&self) -> Value {
        let mut object = self.base.to_json();
        object.insert("filter".into(), json!(self.filter));
        object.insert("label".into(), json!(self.label));
        object.insert("supportsCondition".into(), json!(self.supports_condition));
        if let Some(condition_description) = &self.condition_description {
            object.insert("conditionDescription".into(), json!(condition_description));
        }
        object.insert("fallback".into(), json!(self.fallback.get()));
        if let Some(description) = &self.description {
            object.insert("description".into(), json!(description));
        }
        Value::Object(object)
    }

    pub fn set_supported_session(&self, session_id: &str, supported: bool) {
        if supported {
            self.supported_sessions.borrow_mut().insert(session_id.to_owned());
        } else {
            self.supported_sessions.borrow_mut().remove(session_id);
        }
    }

    /// Used to specify which breakpoints to show when no session is specified.
    pub fn set_fallback(&self, is_fallback: bool) {
        self.fallback.set(is_fallback);
    }

    pub fn supported(&self) -> bool {
        true
    }

    /// Checks if the breakpoint is applicable for the specified session. If `session_id` is `None`, returns true if this
    /// breakpoint is a fallback breakpoint.
    pub fn is_supported_session(&self, session_id: Option<&str>) -> bool {
        match session_id.filter(|id| !id.is_empty()) {
            Some(id) => self.supported_sessions.borrow().contains(id),
            None => self.fallback.get(),
        }
    }

    /// O `ExceptionBreakpointsFilter` equivalente (o `matches` do original aceita o próprio breakpoint).
    pub fn as_filter(&self) -> ExceptionBreakpointsFilter {
        ExceptionBreakpointsFilter {
            filter: self.filter.clone(),
            label: self.label.clone(),
            description: self.description.clone(),
            default: Some(self.enabled()),
            supports_condition: Some(self.supports_condition),
            condition_description: self.condition_description.clone(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.base.enabled()
    }

    pub fn matches(&self, filter: &ExceptionBreakpointsFilter) -> bool {
        self.filter == filter.filter
            && self.label == filter.label
            && self.supports_condition == filter.supports_condition.unwrap_or(false)
            && self.condition_description == filter.condition_description
            && self.description == filter.description
    }

    pub fn to_display_string(&self) -> String {
        self.label.clone()
    }
}

/// `IInstructionBreakpointOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstructionBreakpointOptions {
    pub base: IBaseBreakpointOptions,
    pub instruction_reference: String,
    pub offset: i64,
    pub can_persist: bool,
    pub address: u128,
}

/// `InstructionBreakpoint`.
pub struct InstructionBreakpoint {
    pub base: BaseBreakpoint,
    pub instruction_reference: String,
    pub offset: i64,
    pub can_persist: bool,
    pub address: u128,
}

impl InstructionBreakpoint {
    pub fn new(opts: InstructionBreakpointOptions, id: Option<String>) -> Rc<Self> {
        Rc::new(Self {
            base: BaseBreakpoint::new(id.unwrap_or_else(generate_uuid), &opts.base),
            instruction_reference: opts.instruction_reference,
            offset: opts.offset,
            can_persist: opts.can_persist,
            address: opts.address,
        })
    }

    pub fn get_id(&self) -> String {
        self.base.get_id()
    }

    pub fn to_dap(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("instructionReference".into(), json!(self.instruction_reference));
        if let Some(condition) = self.base.condition.borrow().as_ref() {
            object.insert("condition".into(), json!(condition));
        }
        if let Some(hit_condition) = self.base.hit_condition.borrow().as_ref() {
            object.insert("hitCondition".into(), json!(hit_condition));
        }
        if let Some(mode) = self.base.mode.borrow().as_ref() {
            object.insert("mode".into(), json!(mode));
        }
        object.insert("offset".into(), json!(self.offset));
        Value::Object(object)
    }

    pub fn to_json(&self) -> Value {
        let mut object = self.base.to_json();
        object.insert("instructionReference".into(), json!(self.instruction_reference));
        object.insert("offset".into(), json!(self.offset));
        object.insert("canPersist".into(), json!(self.can_persist));
        object.insert("address".into(), json!(self.address.to_string()));
        Value::Object(object)
    }

    pub fn supported(&self) -> bool {
        self.base.data.borrow().as_ref().is_none_or(|data| data.supports_instruction_breakpoints)
    }

    pub fn to_display_string(&self) -> String {
        self.instruction_reference.clone()
    }
}

// ---- JSON do armazenamento (o `toJSON()` gravado e o `JSON.parse(...).map(...)` do `DebugStorage`)

/// O `URI#toJSON` (`$mid` e os componentes não vazios; os caches `external` e `fsPath` não são gravados).
pub fn uri_to_value(uri: &URI) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("$mid".into(), json!(1));
    for (key, value) in [("scheme", uri.scheme()), ("authority", uri.authority()), ("path", uri.path()), ("query", uri.query()), ("fragment", uri.fragment())] {
        if !value.is_empty() {
            object.insert(key.into(), json!(value.to_string_lossy()));
        }
    }
    Value::Object(object)
}

/// `URI.revive(value)`: o objeto do `toJSON` ou o texto da URI.
pub fn uri_from_value(value: &Value) -> Option<URI> {
    match value {
        Value::String(text) => Some(URI::parse(text.as_str(), false)),
        Value::Object(object) => {
            let part = |key: &str| object.get(key).and_then(Value::as_str).map(je_base::JsString::from);
            Some(URI::from(
                &je_base::uri::UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") },
                false,
            ))
        }
        _ => None,
    }
}

fn json_text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl Breakpoint {
    /// `new Breakpoint(JSON.parse(...)[i], textFileService, uriIdentityService, logService, id)`; o objeto sem `uri` válido
    /// ou sem `lineNumber` devolve `None` (o original lançaria e o `loadBreakpoints` registraria o erro).
    pub fn from_json(
        value: &Value,
        text_file_service: Rc<dyn IDirtyChecker>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log_service: Rc<dyn LogService>,
    ) -> Option<Rc<Breakpoint>> {
        let uri = uri_from_value(value.get("uri")?)?;
        Some(Breakpoint::new(
            IBreakpointOptions {
                base: IBaseBreakpointOptions::from_json(value),
                uri,
                line_number: value.get("lineNumber")?.as_i64()?,
                column: value.get("column").and_then(Value::as_i64),
                adapter_data: value.get("adapterData").cloned(),
                triggered_by: json_text(value, "triggeredBy"),
            },
            text_file_service,
            uri_identity_service,
            log_service,
            json_text(value, "id"),
        ))
    }
}

impl FunctionBreakpoint {
    /// `new FunctionBreakpoint(fb, fb.id)`.
    pub fn from_json(value: &Value) -> Rc<FunctionBreakpoint> {
        FunctionBreakpoint::new(FunctionBreakpointOptions { base: IBaseBreakpointOptions::from_json(value), name: json_text(value, "name").unwrap_or_default() }, json_text(value, "id"))
    }
}

impl DataBreakpoint {
    /// `new DataBreakpoint(dbp, dbp.id)`, com a compatibilidade com o `dataId` solto da versão 1.87.
    pub fn from_json(value: &Value) -> Rc<DataBreakpoint> {
        let src = match value.get("src") {
            Some(src) if src.get("type").and_then(Value::as_i64) == Some(1) => {
                DataBreakpointSource::Address { address: json_text(src, "address").unwrap_or_default(), bytes: src.get("bytes").and_then(Value::as_i64) }
            }
            Some(src) => DataBreakpointSource::Variable { data_id: json_text(src, "dataId").unwrap_or_default() },
            None => DataBreakpointSource::Variable { data_id: json_text(value, "dataId").unwrap_or_default() },
        };
        let access_type = |text: &str| match text {
            "read" => DataBreakpointAccessType::Read,
            "write" => DataBreakpointAccessType::Write,
            _ => DataBreakpointAccessType::ReadWrite,
        };
        DataBreakpoint::new(
            DataBreakpointOptions {
                base: IBaseBreakpointOptions::from_json(value),
                description: json_text(value, "description").unwrap_or_default(),
                src,
                can_persist: value.get("canPersist").and_then(Value::as_bool).unwrap_or(false),
                initial_session_data: None,
                access_types: value
                    .get("accessTypes")
                    .and_then(Value::as_array)
                    .map(|types| types.iter().filter_map(Value::as_str).map(access_type).collect()),
                access_type: access_type(&json_text(value, "accessType").unwrap_or_default()),
            },
            json_text(value, "id"),
        )
    }
}
