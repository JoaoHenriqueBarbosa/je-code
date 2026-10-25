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
//!   contexto) no primeiro uso, como as demais chaves do porte. `EDIT_SESSIONS_VIEW_ICON` fica para a fatia das vistas, que usa o
//!   registro de ícones.
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
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::action::{LocalizedString, localize2};
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
use je_platform::log::LogService;
use je_platform::user_data_sync::user_data_sync::{IResourceRefHandle, ServerResource, SyncResource as SettingsSyncResource};

use super::edit_sessions_storage_client::EditSessionsStoreClient;

/// `EDIT_SESSION_SYNC_CATEGORY`.
pub fn edit_session_sync_category() -> LocalizedString {
    localize2("cloud changes", "Cloud Changes")
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
