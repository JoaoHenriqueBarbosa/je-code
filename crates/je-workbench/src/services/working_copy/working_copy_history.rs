//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyHistory.ts`: o contrato do histórico local das
//! working copies (`IWorkingCopyHistoryService`), a entrada do histórico e os eventos.
//!
//! Divergências:
//! - **Entrada compartilhada.** `IWorkingCopyHistoryEntry` é um objeto JS que o serviço, o modelo e os ouvintes dos eventos
//!   compartilham por referência (os testes comparam com `strictEqual` o objeto do evento com o devolvido por `addEntry`, e
//!   `removeEntry`/`updateEntry` o acham por identidade). Aqui é a struct [`WorkingCopyHistoryEntry`] atrás de um `Rc`
//!   ([`WorkingCopyHistoryEntryRef`]), com `timestamp`, `source` e `sourceDescription` (as propriedades mutáveis) em
//!   `Cell`/`RefCell`; a identidade é `Rc::ptr_eq`.
//! - **`workingCopy`.** O objeto literal `{ resource, name }` é [`WorkingCopyHistoryEntryWorkingCopy`].
//! - **`timestamp`.** `number` (milissegundos desde a época) é `i64`, o tipo do `mtime` do `IFileStat`.
//! - **`SaveSource`.** É o `SaveSource` do porte (`String`); `source?: SaveSource` é `Option<SaveSource>`.
//! - **Eventos.** `Event<IWorkingCopyHistoryEvent>` é `Event<WorkingCopyHistoryEvent>` e `Event<void>` é `Event<()>`.
//! - **`CancellationToken`.** Os métodos recebem o [`CancellationToken`] por valor (`CancellationToken.None` é
//!   `CancellationToken::None`).
//! - **`_serviceBrand` e `createDecorator`.** O identificador é o [`I_WORKING_COPY_HISTORY_SERVICE`] de `service_id!`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::common::editor::SaveSource;

/// A working copy a que uma entrada do histórico pertence: `{ resource: URI; name: string }`.
#[derive(Clone, Debug)]
pub struct WorkingCopyHistoryEntryWorkingCopy {
    pub resource: URI,
    pub name: String,
}

/// `IWorkingCopyHistoryEntry`.
pub struct WorkingCopyHistoryEntry {
    /// Unique identifier of this entry for the working copy.
    pub id: String,
    /// The associated working copy of this entry.
    pub working_copy: WorkingCopyHistoryEntryWorkingCopy,
    /// The location on disk of this history entry.
    pub location: URI,
    timestamp: Cell<i64>,
    source: RefCell<SaveSource>,
    source_description: RefCell<Option<String>>,
}

/// Uma entrada do histórico como o original a passa adiante: um objeto compartilhado por referência.
pub type WorkingCopyHistoryEntryRef = Rc<WorkingCopyHistoryEntry>;

impl WorkingCopyHistoryEntry {
    pub fn new(
        id: String,
        working_copy: WorkingCopyHistoryEntryWorkingCopy,
        location: URI,
        timestamp: i64,
        source: SaveSource,
        source_description: Option<String>,
    ) -> WorkingCopyHistoryEntryRef {
        Rc::new(Self { id, working_copy, location, timestamp: Cell::new(timestamp), source: RefCell::new(source), source_description: RefCell::new(source_description) })
    }

    /// The time when this history entry was created.
    pub fn timestamp(&self) -> i64 {
        self.timestamp.get()
    }

    pub fn set_timestamp(&self, timestamp: i64) {
        self.timestamp.set(timestamp);
    }

    /// Associated source with the history entry.
    pub fn source(&self) -> SaveSource {
        self.source.borrow().clone()
    }

    pub fn set_source(&self, source: SaveSource) {
        *self.source.borrow_mut() = source;
    }

    /// Optional additional metadata associated with the source that can help to describe the source.
    pub fn source_description(&self) -> Option<String> {
        self.source_description.borrow().clone()
    }

    pub fn set_source_description(&self, source_description: Option<String>) {
        *self.source_description.borrow_mut() = source_description;
    }
}

/// `IWorkingCopyHistoryEvent`.
#[derive(Clone)]
pub struct WorkingCopyHistoryEvent {
    /// The entry this event is about.
    pub entry: WorkingCopyHistoryEntryRef,
}

/// `IWorkingCopyHistoryEntryDescriptor`.
#[derive(Clone, Debug)]
pub struct WorkingCopyHistoryEntryDescriptor {
    /// The associated resource of this history entry.
    pub resource: URI,
    /// Optional associated timestamp to use for the history entry. If not provided, the current time will be used.
    pub timestamp: Option<i64>,
    /// Optional source why the entry was added.
    pub source: Option<SaveSource>,
}

impl WorkingCopyHistoryEntryDescriptor {
    /// `{ resource }`.
    pub fn new(resource: URI) -> Self {
        Self { resource, timestamp: None, source: None }
    }

    /// `{ resource, source }`.
    pub fn with_source(resource: URI, source: &str) -> Self {
        Self { resource, timestamp: None, source: Some(source.to_string()) }
    }
}

/// O `properties: { source: SaveSource }` de `updateEntry`.
#[derive(Clone, Debug)]
pub struct WorkingCopyHistoryEntryProperties {
    pub source: SaveSource,
}

/// `IWorkingCopyHistoryService`.
pub trait IWorkingCopyHistoryService {
    /// An event when an entry is added to the history.
    fn on_did_add_entry(&self) -> Event<WorkingCopyHistoryEvent>;

    /// An event when an entry is changed in the history.
    fn on_did_change_entry(&self) -> Event<WorkingCopyHistoryEvent>;

    /// An event when an entry is replaced in the history.
    fn on_did_replace_entry(&self) -> Event<WorkingCopyHistoryEvent>;

    /// An event when an entry is removed from the history.
    fn on_did_remove_entry(&self) -> Event<WorkingCopyHistoryEvent>;

    /// An event when entries are moved in history.
    fn on_did_move_entries(&self) -> Event<()>;

    /// An event when all entries are removed from the history.
    fn on_did_remove_entries(&self) -> Event<()>;

    /// Adds a new entry to the history for the given working copy with an optional associated descriptor.
    fn add_entry(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken) -> Promise<Option<WorkingCopyHistoryEntryRef>>;

    /// Updates an entry in the local history if found.
    fn update_entry(&self, entry: &WorkingCopyHistoryEntryRef, properties: WorkingCopyHistoryEntryProperties, token: CancellationToken) -> Promise<()>;

    /// Removes an entry from the local history if found.
    fn remove_entry(&self, entry: &WorkingCopyHistoryEntryRef, token: CancellationToken) -> Promise<bool>;

    /// Moves entries that either match the `source` or are a child of `source` to the `target`.
    ///
    /// Returns a list of resources for entries that have moved.
    fn move_entries(&self, source: &URI, target: &URI) -> Promise<Vec<URI>>;

    /// Gets all history entries for the provided resource.
    fn get_entries(&self, resource: &URI, token: CancellationToken) -> Promise<Vec<WorkingCopyHistoryEntryRef>>;

    /// Returns all resources for which history entries exist.
    fn get_all(&self, token: CancellationToken) -> Promise<Vec<URI>>;

    /// Removes all entries from all of local history.
    fn remove_all(&self, token: CancellationToken) -> Promise<()>;
}

service_id!(pub I_WORKING_COPY_HISTORY_SERVICE: dyn IWorkingCopyHistoryService = "workingCopyHistoryService");

/// A limit on how many I/O operations we allow to run in parallel. We do not want to spam the file system with too many
/// requests at the same time, so we limit to a maximum degree of parallellism.
pub const MAX_PARALLEL_HISTORY_IO_OPS: usize = 20;
