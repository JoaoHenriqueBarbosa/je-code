//! Porte de `src/vs/workbench/contrib/editSessions/common/editSessions.ts`: os tipos, as constantes, as chaves de contexto, os
//! contratos dos serviços (`IEditSessionsStorageService` e `IEditSessionsLogService`), `decodeEditSessionFileContent` e
//! `hashedEditSessionId`.
//!
//! Divergências:
//! - `Change` (união de `Addition` e `Deletion`) é um enum com o `contents` presente só na adição.
//! - `EditSession.workspaceStateId` e `machine` são `Option<String>`.
//! - `SyncResource` (`'editSessions' | 'workspaceState'`) é o enum [`SyncResource`], distinto do `SyncResource` do Settings Sync
//!   (`je_platform::user_data_sync::user_data_sync::SyncResource`), com a conversão para o `ServerResource` do cliente.
//! - `EDIT_SESSION_SYNC_CATEGORY`, `EDIT_SESSIONS_TITLE` (`ILocalizedString`), `EDIT_SESSIONS_SIGNED_IN`,
//!   `EDIT_SESSIONS_PENDING` e `EDIT_SESSIONS_SHOW_VIEW` (`RawContextKey`) são funções, que criam o valor (e registram a chave de
//!   contexto) no primeiro uso, como as demais chaves do porte. `EDIT_SESSIONS_VIEW_ICON` é [`edit_sessions_view_icon`]: o
//!   `registerIcon` devolve o `ThemeIcon` com o id dado (o registro de ícones ainda não foi portado, como em `default_view_icon`).
//! - `JSON.parse(content) as EditSession` é [`EditSession::parse`]: o original lê o objeto sem checar a forma (um campo
//!   ausente só falha no primeiro uso), e o porte checa a forma na leitura e devolve `TypeError` com o campo que falta. O
//!   `fileType` desconhecido é lido como `File` (o único valor do `enum`, que nada consulta) e um `type` que não é `Addition` nem
//!   `Deletion` é erro.
//! - [`command_title`] é o `title: localize2(...)` de um `IAction2Options`, [`action2_options`] monta as opções de um comando sem
//!   menu e [`FnAction`] é o `registerAction2(class extends Action2 { ... })` das fatias do `editSessions`: a classe anônima do
//!   original fecha sobre o `that` da contribuição, e aqui o fecho do `run` faz o mesmo.
//! - `lastReadResources` e `lastWrittenResources` são `Map` compartilhados e mutáveis no original (o consumidor chama `set` em um
//!   e `get` no outro), então o contrato devolve o mesmo [`EditSessionResourceMap`] (`Rc<RefCell<..>>`) a cada chamada.
//! - `storeClient` (propriedade com leitura e escrita) é o par [`IEditSessionsStorageService::store_client`] e
//!   [`IEditSessionsStorageService::set_store_client`].
//! - `write(resource, content: string | EditSession)` recebe o [`EditSessionContent`]. O original preenche `content.machine` no
//!   objeto do chamador; o porte recebe o valor e o preenche na cópia que serializa (o chamador não lê o campo depois).
//! - `IEditSessionsLogService extends ILogService` é o trait [`IEditSessionsLogService`], com `LogService` como supertrait.
//! - [`EditSession::to_dynamic`] é o `JSON.stringify` do objeto, com as chaves na ordem em que a `EditSessionsContribution` as cria
//!   (`folders`, `version`, `workspaceStateId`, e `machine` por último, acrescentado pelo serviço de armazenamento), e as pastas e
//!   as mudanças na ordem de `editSessions.contribution.ts`. O que é `undefined` sai omitido, como no `JSON.stringify`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::buffer::{VSBuffer, decode_base64};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::hash::StringSha1;
use je_base::json::{Object, Value};
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_instantiation::{ServicesAccessor, service_id};
use je_platform::action::{CommandAction, CommandActionTitle, LocalizedString, Title, localize2};
use je_platform::actions::actions::{Action2, Action2Options, register_action2};
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
use je_platform::log::LogService;
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::user_data_sync::user_data_sync::{IResourceRefHandle, ServerResource, SyncResource as SettingsSyncResource};

use super::edit_sessions_storage_client::EditSessionsStoreClient;

/// `EDIT_SESSION_SYNC_CATEGORY`.
pub fn edit_session_sync_category() -> LocalizedString {
    localize2("cloud changes", "Cloud Changes")
}

/// O `title: ILocalizedString` de um `IAction2Options` (o `localize2(...)`) como título do comando.
pub fn command_title(localized: LocalizedString) -> Title {
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// O `IAction2Options` de um comando sem menu nem atalho (só o `f1` opcional).
pub fn action2_options(command: CommandAction, f1: bool) -> Action2Options {
    Action2Options { command, menu: Vec::new(), keybinding: Vec::new(), f1 }
}

/// `registerAction2(class extends Action2 { constructor() { super(options) } run(accessor, ...args) { ... } })`: a ação que as
/// fatias do `editSessions` registram, com as opções e o corpo do `run` dados (a classe anônima do original fecha sobre o `that`
/// da contribuição; aqui é o fecho que o faz).
pub struct FnAction {
    options: Action2Options,
    run: Box<dyn Fn(&ServicesAccessor, &[Dynamic])>,
}

impl FnAction {
    /// `registerAction2(new FnAction(...))`.
    pub fn register(options: Action2Options, run: impl Fn(&ServicesAccessor, &[Dynamic]) + 'static) -> DisposableRef {
        register_action2(Rc::new(Self { options, run: Box::new(run) }))
    }
}

impl Action2 for FnAction {
    fn desc(&self) -> Action2Options {
        self.options.clone()
    }

    fn run(&self, accessor: &ServicesAccessor, args: &[Dynamic]) {
        (self.run)(accessor, args);
    }
}

/// `SyncResource` (`'editSessions' | 'workspaceState'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncResource {
    EditSessions,
    WorkspaceState,
}

impl SyncResource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditSessions => "editSessions",
            Self::WorkspaceState => "workspaceState",
        }
    }
}

impl From<SyncResource> for ServerResource {
    fn from(resource: SyncResource) -> Self {
        match resource {
            SyncResource::EditSessions => ServerResource::EditSessions,
            SyncResource::WorkspaceState => ServerResource::Sync(SettingsSyncResource::WorkspaceState),
        }
    }
}

/// O `{ ref: string; content: string }` de `read`, `lastReadResources` e `lastWrittenResources`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSessionResource {
    pub r#ref: String,
    pub content: String,
}

/// `Map<SyncResource, { ref: string; content: string }>`, compartilhado e mutável como o `Map` do original.
pub type EditSessionResourceMap = Rc<RefCell<HashMap<SyncResource, EditSessionResource>>>;

/// O `string | EditSession` de `write`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditSessionContent {
    Text(String),
    Session(EditSession),
}

impl From<String> for EditSessionContent {
    fn from(content: String) -> Self {
        Self::Text(content)
    }
}

impl From<EditSession> for EditSessionContent {
    fn from(session: EditSession) -> Self {
        Self::Session(session)
    }
}

/// O `'read' | 'write'` de `initialize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializeReason {
    Read,
    Write,
}

/// `IEditSessionsStorageService`.
pub trait IEditSessionsStorageService {
    /// `SIZE_LIMIT`.
    fn size_limit(&self) -> usize;

    /// `isSignedIn`.
    fn is_signed_in(&self) -> bool;
    fn on_did_sign_in(&self) -> Event<()>;
    fn on_did_sign_out(&self) -> Event<()>;

    /// `storeClient` (leitura).
    fn store_client(&self) -> Option<Rc<EditSessionsStoreClient>>;
    /// `storeClient = ...` (escrita).
    fn set_store_client(&self, store_client: Option<Rc<EditSessionsStoreClient>>);

    fn last_read_resources(&self) -> EditSessionResourceMap;
    fn last_written_resources(&self) -> EditSessionResourceMap;

    fn initialize(&self, reason: InitializeReason, silent: bool) -> Promise<bool>;
    fn read(&self, resource: SyncResource, r#ref: Option<String>) -> Promise<Option<EditSessionResource>>;
    fn write(&self, resource: SyncResource, content: EditSessionContent) -> Promise<String>;
    /// `delete(resource, ref: string | null)`.
    fn delete(&self, resource: SyncResource, r#ref: Option<String>) -> Promise<()>;
    fn list(&self, resource: SyncResource) -> Promise<Vec<IResourceRefHandle>>;
    fn get_machine_by_id(&self, machine_id: &str) -> Promise<Option<String>>;
}

service_id!(pub I_EDIT_SESSIONS_STORAGE_SERVICE: dyn IEditSessionsStorageService = "IEditSessionsStorageService");

/// `IEditSessionsLogService`: um `ILogService` (a interface do original não acrescenta nada).
pub trait IEditSessionsLogService: LogService {}

service_id!(pub I_EDIT_SESSIONS_LOG_SERVICE: dyn IEditSessionsLogService = "IEditSessionsLogService");

/// `ChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeType {
    Addition = 1,
    Deletion = 2,
}

/// `FileType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    File = 1,
}

/// `Change`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Addition { relative_file_path: String, file_type: FileType, contents: String },
    Deletion { relative_file_path: String, file_type: FileType },
}

impl Change {
    pub fn relative_file_path(&self) -> &str {
        match self {
            Self::Addition { relative_file_path, .. } | Self::Deletion { relative_file_path, .. } => relative_file_path,
        }
    }

    pub fn change_type(&self) -> ChangeType {
        match self {
            Self::Addition { .. } => ChangeType::Addition,
            Self::Deletion { .. } => ChangeType::Deletion,
        }
    }

    /// O objeto do `JSON.stringify`, com as chaves na ordem do `workingChanges.push({ type, fileType, contents, relativeFilePath })`.
    pub fn to_dynamic(&self) -> Dynamic {
        let (file_type, contents) = match self {
            Self::Addition { file_type, contents, .. } => (*file_type, Dynamic::string(contents)),
            Self::Deletion { file_type, .. } => (*file_type, Dynamic::Undefined),
        };
        Dynamic::object([
            ("type", Dynamic::Number(f64::from(self.change_type() as i32))),
            ("fileType", Dynamic::Number(f64::from(file_type as i32))),
            ("contents", contents),
            ("relativeFilePath", Dynamic::string(self.relative_file_path())),
        ])
    }
}

/// `Folder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub canonical_identity: Option<String>,
    pub working_changes: Vec<Change>,
    pub absolute_uri: Option<String>,
}

impl Folder {
    /// O objeto do `JSON.stringify`, com as chaves na ordem do `folders.push({ workingChanges, name, canonicalIdentity, absoluteUri })`.
    pub fn to_dynamic(&self) -> Dynamic {
        let optional = |value: &Option<String>| value.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
        Dynamic::object([
            ("workingChanges", Dynamic::array(self.working_changes.iter().map(Change::to_dynamic))),
            ("name", Dynamic::string(&self.name)),
            ("canonicalIdentity", optional(&self.canonical_identity)),
            ("absoluteUri", optional(&self.absolute_uri)),
        ])
    }
}

pub const EDIT_SESSION_SCHEMA_VERSION: u32 = 3;

/// `EditSession`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSession {
    pub version: u32,
    pub workspace_state_id: Option<String>,
    pub machine: Option<String>,
    pub folders: Vec<Folder>,
}

impl EditSession {
    /// O objeto do `JSON.stringify` (ver o topo do módulo para a ordem das chaves).
    pub fn to_dynamic(&self) -> Dynamic {
        let optional = |value: &Option<String>| value.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
        Dynamic::object([
            ("folders", Dynamic::array(self.folders.iter().map(Folder::to_dynamic))),
            ("version", Dynamic::Number(f64::from(self.version))),
            ("workspaceStateId", optional(&self.workspace_state_id)),
            ("machine", optional(&self.machine)),
        ])
    }
}

/// O `TypeError` de um campo que falta (ou tem outro tipo) no objeto de `JSON.parse(content)`.
fn shape_error(owner: &str, key: &str, expected: &str) -> Error {
    Error::named("TypeError", format!("Invalid edit session content: {owner}.{key} is not {expected}"))
}

fn as_object<'a>(value: &'a Value, owner: &str) -> Result<&'a Object, Error> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(Error::named("TypeError", format!("Invalid edit session content: {owner} is not an object"))),
    }
}

fn required_string(object: &Object, owner: &str, key: &str) -> Result<String, Error> {
    match object.get(key) {
        Some(Value::String(text)) => Ok(text.to_string_lossy()),
        _ => Err(shape_error(owner, key, "a string")),
    }
}

fn optional_string(object: &Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn required_array<'a>(object: &'a Object, owner: &str, key: &str) -> Result<&'a [Value], Error> {
    match object.get(key) {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(shape_error(owner, key, "an array")),
    }
}

impl Change {
    /// O `Change` que o objeto do `JSON.parse` descreve.
    fn from_json(value: &Value) -> Result<Self, Error> {
        let object = as_object(value, "change")?;
        let relative_file_path = required_string(object, "change", "relativeFilePath")?;
        match object.get("type") {
            Some(Value::Number(number)) if *number == f64::from(ChangeType::Addition as i32) => {
                Ok(Self::Addition { relative_file_path, file_type: FileType::File, contents: required_string(object, "change", "contents")? })
            }
            Some(Value::Number(number)) if *number == f64::from(ChangeType::Deletion as i32) => Ok(Self::Deletion { relative_file_path, file_type: FileType::File }),
            _ => Err(shape_error("change", "type", "a ChangeType")),
        }
    }
}

impl Folder {
    /// A `Folder` que o objeto do `JSON.parse` descreve.
    fn from_json(value: &Value) -> Result<Self, Error> {
        let object = as_object(value, "folder")?;
        Ok(Self {
            name: required_string(object, "folder", "name")?,
            canonical_identity: optional_string(object, "canonicalIdentity"),
            working_changes: required_array(object, "folder", "workingChanges")?.iter().map(Change::from_json).collect::<Result<_, _>>()?,
            absolute_uri: optional_string(object, "absoluteUri"),
        })
    }
}

impl EditSession {
    /// `JSON.parse(content) as EditSession` (ver as divergências do módulo).
    pub fn parse(content: &str) -> Result<Self, Error> {
        let value = parse_json(content)?;
        let object = as_object(&value, "editSession")?;
        let version = match object.get("version") {
            Some(Value::Number(number)) if number.fract() == 0.0 && *number >= 0.0 && *number <= f64::from(u32::MAX) => *number as u32,
            _ => return Err(shape_error("editSession", "version", "a version number")),
        };
        Ok(Self {
            version,
            workspace_state_id: optional_string(object, "workspaceStateId"),
            machine: optional_string(object, "machine"),
            folders: required_array(object, "editSession", "folders")?.iter().map(Folder::from_json).collect::<Result<_, _>>()?,
        })
    }
}

pub const EDIT_SESSIONS_SIGNED_IN_KEY: &str = "editSessionsSignedIn";

/// `EDIT_SESSIONS_SIGNED_IN`.
pub fn edit_sessions_signed_in() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new(EDIT_SESSIONS_SIGNED_IN_KEY, false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

pub const EDIT_SESSIONS_PENDING_KEY: &str = "editSessionsPending";

/// `EDIT_SESSIONS_PENDING`.
pub fn edit_sessions_pending() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new(EDIT_SESSIONS_PENDING_KEY, false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

pub const EDIT_SESSIONS_CONTAINER_ID: &str = "workbench.view.editSessions";
pub const EDIT_SESSIONS_DATA_VIEW_ID: &str = "workbench.views.editSessions.data";

/// `EDIT_SESSIONS_TITLE`.
pub fn edit_sessions_title() -> LocalizedString {
    localize2("cloud changes", "Cloud Changes")
}

/// `EDIT_SESSIONS_VIEW_ICON`.
pub fn edit_sessions_view_icon() -> ThemeIcon {
    ThemeIcon::from_id("edit-sessions-view-icon")
}

/// `EDIT_SESSIONS_SHOW_VIEW`.
pub fn edit_sessions_show_view() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new("editSessionsShowView", false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

pub const EDIT_SESSIONS_SCHEME: &str = "vscode-edit-sessions";
pub const EDIT_SESSIONS_LOG_ID: &str = "editSessions";

/// `decodeEditSessionFileContent(version, content)`.
pub fn decode_edit_session_file_content(version: u32, content: &str) -> Result<VSBuffer, Error> {
    match version {
        1 => Ok(VSBuffer::from_string(content)),
        2 => Ok(decode_base64(content)),
        _ => Err(Error::new("Upgrade to a newer version to decode this content.")),
    }
}

/// `hashedEditSessionId(editSessionId)`.
pub fn hashed_edit_session_id(edit_session_id: &str) -> String {
    let mut sha1 = StringSha1::new();
    let units: Vec<u16> = edit_session_id.encode_utf16().collect();
    sha1.update(&units);
    sha1.digest()
}
