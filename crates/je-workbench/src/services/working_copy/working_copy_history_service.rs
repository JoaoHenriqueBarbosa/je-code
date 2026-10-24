//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyHistoryService.ts`: o modelo do histórico de uma
//! working copy (`WorkingCopyHistoryModel`, com a persistência em `entries.json`), o serviço que guarda os modelos
//! (`WorkingCopyHistoryService`) e a versão do desktop (`NativeWorkingCopyHistoryService`, que grava tudo ao desligar e
//! agenda a gravação a cada 5 minutos).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados são argumentos dos construtores, na ordem do original. `@IFileService` é o
//!   `Rc<FileService>` concreto: o contrato [`je_platform::files::i_file_service::IFileService`] do porte não tem `cloneFile`
//!   (que o modelo usa para copiar a working copy para a entrada). `@ILogService` é o [`LogServiceRef`].
//! - **Entradas.** As entradas são objetos compartilhados por referência (ver [`WorkingCopyHistoryEntry`]); `entries.indexOf`
//!   é a comparação por `Rc::ptr_eq`.
//! - **Classe abstrata.** `WorkingCopyHistoryService` (abstrata, com `getModelOptions()`) é uma struct concreta que recebe o
//!   `getModelOptions` como fecho; `NativeWorkingCopyHistoryService extends WorkingCopyHistoryService` é a composição: a
//!   struct guarda o serviço base e o expõe por `Deref` e por [`NativeWorkingCopyHistoryService::service`].
//! - **`this` nas funções `async`.** O `Rc` do próprio objeto: o modelo recebe `self: &Rc<Self>` e o serviço guarda um `Weak`
//!   de si mesmo (`Rc::new_cyclic`).
//! - **`entries.json`.** `JSON.parse` e `JSON.stringify` são o `serde_json` (com `preserve_order`, que mantém a ordem das
//!   chaves de `JSON.stringify`: `version`, `resource`, `entries` e, em cada entrada, `id`, `source`, `sourceDescription`,
//!   `timestamp`). Um arquivo com JSON válido mas com forma diferente da esperada é tratado como arquivo ausente (no original
//!   a forma errada lançaria um `TypeError` mais adiante).
//! - **`historyEntriesNameMatcher`.** O `RegExp` `[A-Za-z0-9]{4}<extname escapado>` (sem âncoras) é a busca equivalente
//!   em [`matches_entry_name`].
//! - **Configuração numérica.** `getValue<number>(...)` devolve o valor dinâmico e a aritmética do JavaScript sobre ele (`undefined`
//!   vira `NaN`, em que toda comparação é falsa) é [`configuration_number`].
//! - **`SaveSourceRegistry.registerSource` estático.** Os três registros (`default.source`, `moved.source` e `renamed.source`)
//!   são feitos pelas funções [`file_saved_source`], [`file_moved_source`] e [`file_renamed_source`], que o construtor do
//!   serviço chama para registrar os rótulos como o carregamento da classe faz.
//! - **Registro.** `registerWorkbenchContribution(WorkingCopyHistoryTracker, Restored)` e o registro do serviço nativo no
//!   `workbench.desktop.main` ficam para a montagem do workbench.
//! - **`localize`.** A chave e a mensagem são as do original; o `localize` do porte devolve a mensagem em inglês.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::{Limiter, RunOnceScheduler, async_fn};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::extpath::{index_of_path, random_path};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::nls::localize;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::EnvironmentService as _;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::{FileOperationError, FileOperationResult, FileStat, PartialFileDeleteOptions, ResolveFileOptions, to_radix};
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::resources::{ext_uri, host_environment, join_path};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::working_copy_history::{
    IWorkingCopyHistoryService, MAX_PARALLEL_HISTORY_IO_OPS, WorkingCopyHistoryEntry, WorkingCopyHistoryEntryDescriptor, WorkingCopyHistoryEntryProperties,
    WorkingCopyHistoryEntryRef, WorkingCopyHistoryEntryWorkingCopy, WorkingCopyHistoryEvent,
};
use crate::common::editor::{SaveSource, with_save_source_registry};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, WillShutdownEvent, WillShutdownEventJoiner};
use crate::services::remote::remote_agent_service::IRemoteAgentService;

/// `SaveSourceRegistry.registerSource(id, localize(id, label))`.
pub(super) fn register_save_source(id: &str, label: &str) -> SaveSource {
    with_save_source_registry(|registry| registry.register_source(id, &localize(id, label, &[])))
}

/// `WorkingCopyHistoryModel.FILE_SAVED_SOURCE`.
pub fn file_saved_source() -> SaveSource {
    register_save_source("default.source", "File Saved")
}

/// `WorkingCopyHistoryService.FILE_MOVED_SOURCE`.
pub fn file_moved_source() -> SaveSource {
    register_save_source("moved.source", "File Moved")
}

/// `WorkingCopyHistoryService.FILE_RENAMED_SOURCE`.
pub fn file_renamed_source() -> SaveSource {
    register_save_source("renamed.source", "File Renamed")
}

/// `Date.now()`.
fn now_ms() -> i64 {
    i64::try_from(date_now()).unwrap_or(i64::MAX)
}

/// O número que o JavaScript tira do valor da configuração numa expressão aritmética (`ToNumber`): `undefined` e o que não é
/// número é `NaN`, e toda comparação com `NaN` é falsa.
fn to_js_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        Dynamic::Null => 0.0,
        Dynamic::Bool(flag) => f64::from(u8::from(*flag)),
        Dynamic::String(text) => {
            let text = text.to_string_lossy();
            let text = text.trim();
            if text.is_empty() { 0.0 } else { text.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

/// `configurationService.getValue<number>(section, { resource })` usado como número.
pub fn configuration_number(configuration_service: &dyn IConfigurationService, section: &str, resource: Option<&URI>) -> f64 {
    let overrides = ConfigurationOverrides { resource: resource.cloned(), ..ConfigurationOverrides::default() };

    to_js_number(&configuration_service.get_value(Some(section), Some(&overrides)))
}

/// `Array.prototype.slice(start, end)` sobre os números que a configuração produz (índice negativo conta do fim).
fn js_slice(entries: &[WorkingCopyHistoryEntryRef], start: f64, end: Option<f64>) -> Vec<WorkingCopyHistoryEntryRef> {
    let len = entries.len() as f64;
    let relative = |index: f64| {
        let index = if index.is_nan() { 0.0 } else { index.trunc() };
        if index < 0.0 { (len + index).max(0.0) } else { index.min(len) }
    };
    let (start, end) = (relative(start), end.map_or(len, relative));
    if start >= end { Vec::new() } else { entries[start as usize..end as usize].to_vec() }
}

/// O `historyEntriesNameMatcher` (`new RegExp(`[A-Za-z0-9]{4}${escapeRegExpCharacters(extname)}`)`) aplicado com `test`: o
/// nome contém quatro caracteres alfanuméricos seguidos da extensão.
fn matches_entry_name(name: &str, extname: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 4 {
        return false;
    }

    (0..=bytes.len() - 4).any(|start| bytes[start..start + 4].iter().all(u8::is_ascii_alphanumeric) && bytes[start + 4..].starts_with(extname.as_bytes()))
}

/// `error instanceof FileOperationError && error.fileOperationResult === FileOperationResult.FILE_NOT_FOUND`.
fn is_file_not_found(error: &Error) -> bool {
    FileOperationError::from_error(error).is_some_and(|error| error.file_operation_result == FileOperationResult::FileNotFound)
}

/// `{ recursive: true }`.
fn recursive_delete() -> Option<PartialFileDeleteOptions> {
    Some(PartialFileDeleteOptions { recursive: Some(true), ..PartialFileDeleteOptions::default() })
}

/// `ISerializedWorkingCopyHistoryModelEntry`.
#[derive(Clone)]
struct SerializedEntry {
    id: String,
    timestamp: Option<i64>,
    source: Option<SaveSource>,
    source_description: Option<String>,
}

/// `ISerializedWorkingCopyHistoryModel`.
#[derive(Clone)]
struct SerializedModel {
    resource: String,
    entries: Vec<SerializedEntry>,
}

/// `JSON.parse(...)` do arquivo de entradas, lido como o `ISerializedWorkingCopyHistoryModel`.
fn parse_serialized_model(text: &str) -> Option<SerializedModel> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let entries = value
        .get("entries")?
        .as_array()?
        .iter()
        .map(|entry| {
            Some(SerializedEntry {
                id: entry.get("id")?.as_str()?.to_string(),
                timestamp: entry.get("timestamp").and_then(serde_json::Value::as_f64).map(|timestamp| timestamp as i64),
                source: entry.get("source").and_then(serde_json::Value::as_str).map(str::to_string),
                source_description: entry.get("sourceDescription").and_then(serde_json::Value::as_str).map(str::to_string),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let resource = value.get("resource").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();

    Some(SerializedModel { resource, entries })
}

/// `JSON.parse((await fileService.readFile(file)).value.toString())`: rejeita quando o arquivo não existe, não é JSON ou tem outra
/// forma.
fn read_serialized_model(file_service: &Rc<FileService>, file: &URI) -> Promise<SerializedModel> {
    let (file_service, file) = (file_service.clone(), file.clone());

    async_fn(async move {
        let content = file_service.read_file(&file, None, None).await?;
        let text = content.value.with_bytes(|bytes| String::from_utf8_lossy(bytes).into_owned());

        parse_serialized_model(&text).ok_or_else(|| Error::new("Unexpected contents of the working copy history entries file"))
    })
}

/// `IWorkingCopyHistoryModelOptions`.
#[derive(Clone, Copy, Debug)]
pub struct WorkingCopyHistoryModelOptions {
    /// Whether to flush when the model changes. If not configured, `model.store()` has to be called explicitly.
    pub flush_on_change: bool,
}

/// Os quatro emissores de entrada que o serviço entrega a cada modelo.
#[derive(Clone)]
pub struct WorkingCopyHistoryEmitters {
    pub added: Rc<Emitter<WorkingCopyHistoryEvent>>,
    pub changed: Rc<Emitter<WorkingCopyHistoryEvent>>,
    pub replaced: Rc<Emitter<WorkingCopyHistoryEvent>>,
    pub removed: Rc<Emitter<WorkingCopyHistoryEvent>>,
}

/// O que `setWorkingCopy` calcula a partir do recurso da working copy.
#[derive(Clone)]
struct ModelLocation {
    working_copy_resource: URI,
    working_copy_name: String,
    /// A extensão do `historyEntriesNameMatcher`.
    entries_extname: String,
    history_entries_folder: URI,
    history_entries_listing_file: URI,
}

struct ModelState {
    location: ModelLocation,
    entries: Vec<WorkingCopyHistoryEntryRef>,
    version_id: u64,
    stored_version_id: u64,
}

/// `WorkingCopyHistoryModel`.
pub struct WorkingCopyHistoryModel {
    history_home: URI,
    emitters: WorkingCopyHistoryEmitters,
    options: WorkingCopyHistoryModelOptions,
    file_service: Rc<FileService>,
    label_service: Rc<dyn ILabelService>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IConfigurationService>,
    state: RefCell<ModelState>,
    when_resolved: RefCell<Option<Promise<()>>>,
    store_limiter: Limiter<()>,
}

impl WorkingCopyHistoryModel {
    pub const ENTRIES_FILE: &'static str = "entries.json";

    const MAX_ENTRIES_SETTING: &'static str = "workbench.localHistory.maxFileEntries";
    const MERGE_PERIOD_SETTING: &'static str = "workbench.localHistory.mergeWindow";

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        working_copy_resource: &URI,
        history_home: &URI,
        emitters: WorkingCopyHistoryEmitters,
        options: WorkingCopyHistoryModelOptions,
        file_service: Rc<FileService>,
        label_service: Rc<dyn ILabelService>,
        log_service: LogServiceRef,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let location = Self::locate(working_copy_resource, history_home, label_service.as_ref());

        Rc::new(Self {
            history_home: history_home.clone(),
            emitters,
            options,
            file_service,
            label_service,
            log_service,
            configuration_service,
            state: RefCell::new(ModelState { location, entries: Vec::new(), version_id: 0, stored_version_id: 0 }),
            when_resolved: RefCell::new(None),
            store_limiter: Limiter::new(1),
        })
    }

    /// O cálculo de `setWorkingCopy` (nome, casador de nomes e locais) para um recurso.
    fn locate(working_copy_resource: &URI, history_home: &URI, label_service: &dyn ILabelService) -> ModelLocation {
        let history_entries_folder = Self::to_history_entries_folder(history_home, working_copy_resource);

        ModelLocation {
            working_copy_resource: working_copy_resource.clone(),
            working_copy_name: label_service.get_uri_basename_label(working_copy_resource),
            entries_extname: ext_uri().extname(working_copy_resource),
            history_entries_listing_file: join_path(&history_entries_folder, &[Self::ENTRIES_FILE]),
            history_entries_folder,
        }
    }

    fn set_working_copy(&self, working_copy_resource: &URI) {
        let location = Self::locate(working_copy_resource, &self.history_home, self.label_service.as_ref());

        // Update working copy and locations, reset entries and resolved cache
        let mut state = self.state.borrow_mut();
        state.location = location;
        state.entries = Vec::new();
        drop(state);
        *self.when_resolved.borrow_mut() = None;
    }

    fn to_history_entries_folder(history_home: &URI, working_copy_resource: &URI) -> URI {
        let hashed = hash(&Dynamic::String(working_copy_resource.to_string(false)));

        join_path(history_home, &[&to_radix(i64::from(hashed), 16)])
    }

    fn location(&self) -> ModelLocation {
        self.state.borrow().location.clone()
    }

    /// `this.versionId++`.
    fn bump_version(&self) {
        self.state.borrow_mut().version_id += 1;
    }

    fn index_of(&self, entry: &WorkingCopyHistoryEntryRef) -> Option<usize> {
        self.state.borrow().entries.iter().position(|candidate| Rc::ptr_eq(candidate, entry))
    }

    pub fn add_entry(
        self: &Rc<Self>,
        source: Option<SaveSource>,
        source_description: Option<String>,
        timestamp: Option<i64>,
        token: CancellationToken,
    ) -> Promise<WorkingCopyHistoryEntryRef> {
        let this = self.clone();

        async_fn(async move {
            let source = source.unwrap_or_else(file_saved_source);
            let timestamp = timestamp.unwrap_or_else(now_ms);
            let mut entry_to_replace: Option<WorkingCopyHistoryEntryRef> = None;

            // Figure out if the last entry should be replaced based
            // on settings that can define a interval for when an
            // entry is not added as new entry but should replace.
            // However, when save source is different, never replace.
            let (last_entry, working_copy_resource) = {
                let state = this.state.borrow();
                (state.entries.last().cloned(), state.location.working_copy_resource.clone())
            };
            if let Some(last_entry) = last_entry
                && last_entry.source() == source
            {
                let configured_replace_interval = configuration_number(this.configuration_service.as_ref(), Self::MERGE_PERIOD_SETTING, Some(&working_copy_resource));
                if (timestamp - last_entry.timestamp()) as f64 <= configured_replace_interval * 1000.0 /* convert to millies */ {
                    entry_to_replace = Some(last_entry);
                }
            }

            let entry = match entry_to_replace {
                // Replace lastest entry in history
                Some(entry_to_replace) => this.do_replace_entry(&entry_to_replace, source, source_description, timestamp).await?,

                // Add entry to history
                None => this.do_add_entry(source, source_description, timestamp).await?,
            };

            // Flush now if configured
            if this.options.flush_on_change && !token.is_cancellation_requested() {
                this.store(&token).await?;
            }

            Ok(entry)
        })
    }

    fn do_add_entry(self: &Rc<Self>, source: SaveSource, source_description: Option<String>, timestamp: i64) -> Promise<WorkingCopyHistoryEntryRef> {
        let this = self.clone();

        async_fn(async move {
            let location = this.location();
            let working_copy_resource = location.working_copy_resource.clone();

            // Perform a fast clone operation with minimal overhead to a new random location
            let environment = host_environment();
            let id = format!("{}{}", random_path(None, None, 4, environment.as_ref()), ext_uri().extname(&working_copy_resource));
            let entry_location = join_path(&location.history_entries_folder, &[&id]);
            this.file_service.clone_file(&working_copy_resource, &entry_location).await?;

            // Add to list of entries
            let entry = WorkingCopyHistoryEntry::new(
                id,
                WorkingCopyHistoryEntryWorkingCopy { resource: working_copy_resource, name: location.working_copy_name },
                entry_location,
                timestamp,
                source,
                source_description,
            );
            this.state.borrow_mut().entries.push(entry.clone());

            // Update version ID of model to use for storing later
            this.bump_version();

            // Events
            this.emitters.added.fire(WorkingCopyHistoryEvent { entry: entry.clone() });

            Ok(entry)
        })
    }

    fn do_replace_entry(
        self: &Rc<Self>,
        entry: &WorkingCopyHistoryEntryRef,
        source: SaveSource,
        source_description: Option<String>,
        timestamp: i64,
    ) -> Promise<WorkingCopyHistoryEntryRef> {
        let (this, entry) = (self.clone(), entry.clone());

        async_fn(async move {
            let working_copy_resource = this.location().working_copy_resource;

            // Perform a fast clone operation with minimal overhead to the existing location
            this.file_service.clone_file(&working_copy_resource, &entry.location).await?;

            // Update entry
            entry.set_source(source);
            entry.set_source_description(source_description);
            entry.set_timestamp(timestamp);

            // Update version ID of model to use for storing later
            this.bump_version();

            // Events
            this.emitters.replaced.fire(WorkingCopyHistoryEvent { entry: entry.clone() });

            Ok(entry)
        })
    }

    pub fn remove_entry(self: &Rc<Self>, entry: &WorkingCopyHistoryEntryRef, token: CancellationToken) -> Promise<bool> {
        let (this, entry) = (self.clone(), entry.clone());

        async_fn(async move {
            // Make sure to await resolving when removing entries
            this.resolve_entries_once().await?;

            if token.is_cancellation_requested() {
                return Ok(false);
            }

            let Some(index) = this.index_of(&entry) else {
                return Ok(false);
            };

            // Delete from disk
            this.delete_entry(&entry).await;

            // Remove from model
            {
                let mut state = this.state.borrow_mut();
                if index < state.entries.len() {
                    state.entries.remove(index);
                }
            }

            // Update version ID of model to use for storing later
            this.bump_version();

            // Events
            this.emitters.removed.fire(WorkingCopyHistoryEvent { entry: entry.clone() });

            // Flush now if configured
            if this.options.flush_on_change && !token.is_cancellation_requested() {
                this.store(&token).await?;
            }

            Ok(true)
        })
    }

    pub fn update_entry(self: &Rc<Self>, entry: &WorkingCopyHistoryEntryRef, properties: WorkingCopyHistoryEntryProperties, token: CancellationToken) -> Promise<()> {
        let (this, entry) = (self.clone(), entry.clone());

        async_fn(async move {
            // Make sure to await resolving when updating entries
            this.resolve_entries_once().await?;

            if token.is_cancellation_requested() {
                return Ok(());
            }

            if this.index_of(&entry).is_none() {
                return Ok(());
            }

            // Update entry
            entry.set_source(properties.source);

            // Update version ID of model to use for storing later
            this.bump_version();

            // Events
            this.emitters.changed.fire(WorkingCopyHistoryEvent { entry: entry.clone() });

            // Flush now if configured
            if this.options.flush_on_change && !token.is_cancellation_requested() {
                this.store(&token).await?;
            }

            Ok(())
        })
    }

    pub fn get_entries(self: &Rc<Self>) -> Promise<Vec<WorkingCopyHistoryEntryRef>> {
        let this = self.clone();

        async_fn(async move {
            // Make sure to await resolving when all entries are asked for
            this.resolve_entries_once().await?;

            // Return as many entries as configured by user settings
            let (entries, working_copy_resource) = {
                let state = this.state.borrow();
                (state.entries.clone(), state.location.working_copy_resource.clone())
            };
            let configured_max_entries = configuration_number(this.configuration_service.as_ref(), Self::MAX_ENTRIES_SETTING, Some(&working_copy_resource));
            if entries.len() as f64 > configured_max_entries {
                return Ok(js_slice(&entries, entries.len() as f64 - configured_max_entries, None));
            }

            Ok(entries)
        })
    }

    pub fn has_entries(self: &Rc<Self>, skip_resolve: bool) -> Promise<bool> {
        let this = self.clone();

        async_fn(async move {
            // Make sure to await resolving unless explicitly skipped
            if !skip_resolve {
                this.resolve_entries_once().await?;
            }

            Ok(!this.state.borrow().entries.is_empty())
        })
    }

    fn resolve_entries_once(self: &Rc<Self>) -> Promise<()> {
        let existing = self.when_resolved.borrow().clone();
        if let Some(existing) = existing {
            return existing;
        }

        let when_resolved = self.do_resolve_entries();
        *self.when_resolved.borrow_mut() = Some(when_resolved.clone());

        when_resolved
    }

    fn do_resolve_entries(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            // Resolve from disk
            let mut entries = this.resolve_entries_from_disk().await?;

            // We now need to merge our in-memory entries with the
            // entries we have found on disk because it is possible
            // that new entries have been added before the entries
            // listing file was updated
            let in_memory_entries = this.state.borrow().entries.clone();
            for entry in in_memory_entries {
                Self::set_by_id(&mut entries, entry);
            }

            // Set as entries, sorted by timestamp
            let mut entries: Vec<WorkingCopyHistoryEntryRef> = entries.into_iter().map(|(_, entry)| entry).collect();
            entries.sort_by_key(|entry| entry.timestamp());
            this.state.borrow_mut().entries = entries;

            Ok(())
        })
    }

    /// `entries.set(entry.id, entry)` de um `Map<string, ...>`: troca o valor mantendo a posição da chave.
    fn set_by_id(entries: &mut Vec<(String, WorkingCopyHistoryEntryRef)>, entry: WorkingCopyHistoryEntryRef) {
        match entries.iter_mut().find(|(id, _)| *id == entry.id) {
            Some(existing) => existing.1 = entry,
            None => entries.push((entry.id.clone(), entry)),
        }
    }

    fn resolve_entries_from_disk(self: &Rc<Self>) -> Promise<Vec<(String, WorkingCopyHistoryEntryRef)>> {
        let this = self.clone();

        async_fn(async move {
            let location = this.location();

            // Resolve entries listing file and children of history folder
            let entry_listing = this.read_entries_file();
            let entry_stats = this.read_entries_folder();
            let (entry_listing, entry_stats) = (entry_listing.await?, entry_stats.await?);

            // Add from raw folder children
            let mut entries: Vec<(String, WorkingCopyHistoryEntryRef)> = Vec::new();
            if let Some(entry_stats) = entry_stats {
                for entry_stat in entry_stats {
                    Self::set_by_id(
                        &mut entries,
                        WorkingCopyHistoryEntry::new(
                            entry_stat.name.clone(),
                            WorkingCopyHistoryEntryWorkingCopy { resource: location.working_copy_resource.clone(), name: location.working_copy_name.clone() },
                            entry_stat.resource.clone(),
                            entry_stat.mtime.unwrap_or_default(),
                            file_saved_source(),
                            None,
                        ),
                    );
                }
            }

            // Update from listing (to have more specific metadata)
            if let Some(entry_listing) = entry_listing {
                for entry in entry_listing.entries {
                    let existing_entry = entries.iter().find(|(id, _)| *id == entry.id).map(|(_, existing)| existing.clone());
                    if let Some(existing_entry) = existing_entry {
                        Self::set_by_id(
                            &mut entries,
                            WorkingCopyHistoryEntry::new(
                                existing_entry.id.clone(),
                                existing_entry.working_copy.clone(),
                                existing_entry.location.clone(),
                                entry.timestamp.unwrap_or_else(|| existing_entry.timestamp()),
                                entry.source.unwrap_or_else(|| existing_entry.source()),
                                entry.source_description.or_else(|| existing_entry.source_description()),
                            ),
                        );
                    }
                }
            }

            Ok(entries)
        })
    }

    pub fn move_entries(self: &Rc<Self>, target: &Rc<Self>, source: SaveSource, token: CancellationToken) -> Promise<()> {
        let (this, target) = (self.clone(), target.clone());

        async_fn(async move {
            let timestamp = now_ms();
            let source_location = this.location();
            let source_description = this.label_service.get_uri_label(&source_location.working_copy_resource, &UriLabelOptions::default());

            // Move all entries into the target folder so that we preserve
            // any existing history entries that might already be present

            let source_history_entries_folder = source_location.history_entries_folder.clone();
            let target_location = target.location();
            let target_history_entries_folder = target_location.history_entries_folder.clone();
            let entries_to_move = this.state.borrow().entries.clone();
            let moved: Result<(), Error> = async {
                for entry in &entries_to_move {
                    this.file_service.move_(&entry.location, &join_path(&target_history_entries_folder, &[&entry.id]), Some(true)).await?;
                }
                this.file_service.del(&source_history_entries_folder, recursive_delete()).await
            }
            .await;
            if let Err(error) = moved
                && !is_file_not_found(&error)
            {
                // In case of an error (unless not found), fallback to moving the entire folder
                if let Err(error) = this.file_service.move_(&source_history_entries_folder, &target_history_entries_folder, Some(true)).await
                    && !is_file_not_found(&error)
                {
                    this.trace_error(&error);
                }
            }

            // Merge our entries with target entries before updating associated working copy
            let mut all_entries: Vec<WorkingCopyHistoryEntryRef> = Vec::new();
            let target_entries = target.state.borrow().entries.clone();
            for entry in this.state.borrow().entries.iter().cloned().chain(target_entries) {
                if !all_entries.iter().any(|candidate| candidate.id == entry.id) {
                    all_entries.push(entry);
                }
            }
            all_entries.sort_by_key(|entry| entry.timestamp());

            // Update our associated working copy
            this.set_working_copy(&target_location.working_copy_resource);

            // Restore our entries and ensure correct metadata
            {
                let mut state = this.state.borrow_mut();
                for entry in all_entries {
                    state.entries.push(WorkingCopyHistoryEntry::new(
                        entry.id.clone(),
                        WorkingCopyHistoryEntryWorkingCopy { resource: target_location.working_copy_resource.clone(), name: target_location.working_copy_name.clone() },
                        join_path(&target_history_entries_folder, &[&entry.id]),
                        entry.timestamp(),
                        entry.source(),
                        entry.source_description(),
                    ));
                }
            }

            // Add entry for the move
            this.add_entry(Some(source), Some(source_description), Some(timestamp), token.clone()).await?;

            // Store model again to updated location
            this.store(&token).await
        })
    }

    pub fn store(self: &Rc<Self>, token: &CancellationToken) -> Promise<()> {
        if !self.should_store() {
            return Promise::resolved(());
        }

        // Use a `Limiter` to prevent multiple `store` operations
        // potentially running at the same time

        let (this, token) = (self.clone(), token.clone());

        self.store_limiter.queue(move || {
            async_fn(async move {
                if token.is_cancellation_requested() || !this.should_store() {
                    return Ok(());
                }

                this.do_store(&token).await
            })
        })
    }

    fn should_store(&self) -> bool {
        let state = self.state.borrow();

        state.stored_version_id != state.version_id
    }

    fn do_store(self: &Rc<Self>, token: &CancellationToken) -> Promise<()> {
        let (this, token) = (self.clone(), token.clone());

        async_fn(async move {
            let history_entries_folder = this.location().history_entries_folder;

            // Make sure to await resolving when persisting
            this.resolve_entries_once().await?;

            if token.is_cancellation_requested() {
                return Ok(());
            }

            // Cleanup based on max-entries setting
            this.clean_up_entries().await;

            // Without entries, remove the history folder
            let stored_version = this.state.borrow().version_id;
            if this.state.borrow().entries.is_empty() {
                if let Err(error) = this.file_service.del(&history_entries_folder, recursive_delete()).await {
                    this.trace_error(&error);
                }
            }
            // If we still have entries, update the entries meta file
            else {
                this.write_entries_file().await?;
            }

            // Mark as stored version
            this.state.borrow_mut().stored_version_id = stored_version;

            Ok(())
        })
    }

    fn clean_up_entries(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            let (entries, working_copy_resource) = {
                let state = this.state.borrow();
                (state.entries.clone(), state.location.working_copy_resource.clone())
            };
            let configured_max_entries = configuration_number(this.configuration_service.as_ref(), Self::MAX_ENTRIES_SETTING, Some(&working_copy_resource));
            if entries.len() as f64 <= configured_max_entries {
                return Ok(()); // nothing to cleanup
            }

            let keep_from = entries.len() as f64 - configured_max_entries;
            let entries_to_delete = js_slice(&entries, 0.0, Some(keep_from));
            let entries_to_keep = js_slice(&entries, keep_from, None);

            // Delete entries from disk as instructed
            for entry_to_delete in &entries_to_delete {
                this.delete_entry(entry_to_delete).await;
            }

            // Make sure to update our in-memory model as well
            // because it will be persisted right after
            this.state.borrow_mut().entries = entries_to_keep;

            // Events
            for entry in entries_to_delete {
                this.emitters.removed.fire(WorkingCopyHistoryEvent { entry });
            }

            Ok(())
        })
    }

    /// `deleteEntry(entry)`: o erro é só registrado, então a promessa nunca rejeita.
    fn delete_entry(self: &Rc<Self>, entry: &WorkingCopyHistoryEntryRef) -> Promise<()> {
        let (this, entry) = (self.clone(), entry.clone());

        async_fn(async move {
            if let Err(error) = this.file_service.del(&entry.location, None).await {
                this.trace_error(&error);
            }

            Ok(())
        })
    }

    fn write_entries_file(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            let (location, entries) = {
                let state = this.state.borrow();
                (state.location.clone(), state.entries.clone())
            };

            let file_saved_source = file_saved_source();
            let serialized_entries: Vec<serde_json::Value> = entries
                .iter()
                .map(|entry| {
                    let mut serialized = serde_json::Map::new();
                    serialized.insert("id".to_string(), entry.id.clone().into());
                    if entry.source() != file_saved_source {
                        serialized.insert("source".to_string(), entry.source().into());
                    }
                    if let Some(source_description) = entry.source_description() {
                        serialized.insert("sourceDescription".to_string(), source_description.into());
                    }
                    serialized.insert("timestamp".to_string(), entry.timestamp().into());

                    serde_json::Value::Object(serialized)
                })
                .collect();
            let mut serialized_model = serde_json::Map::new();
            serialized_model.insert("version".to_string(), 1.into());
            serialized_model.insert("resource".to_string(), location.working_copy_resource.to_string(false).to_string_lossy().into());
            serialized_model.insert("entries".to_string(), serialized_entries.into());

            let text = serde_json::Value::Object(serialized_model).to_string();
            this.file_service.write_file(&location.history_entries_listing_file, FileContentSource::Buffer(VSBuffer::from_string(&text)), None).await?;

            Ok(())
        })
    }

    fn read_entries_file(self: &Rc<Self>) -> Promise<Option<SerializedModel>> {
        let this = self.clone();

        async_fn(async move {
            let listing_file = this.location().history_entries_listing_file;

            Ok(match read_serialized_model(&this.file_service, &listing_file).await {
                Ok(model) => Some(model),
                Err(error) => {
                    if !is_file_not_found(&error) {
                        this.trace_error(&error);
                    }
                    None
                }
            })
        })
    }

    fn read_entries_folder(self: &Rc<Self>) -> Promise<Option<Vec<FileStat>>> {
        let this = self.clone();

        async_fn(async move {
            let location = this.location();

            // Resolve children of folder on disk
            let resolve_options = ResolveFileOptions { resolve_metadata: true, ..ResolveFileOptions::default() };
            let raw_entries = match this.file_service.resolve(&location.history_entries_folder, Some(resolve_options)).await {
                Ok(stat) => stat.children,
                Err(error) => {
                    if !is_file_not_found(&error) {
                        this.trace_error(&error);
                    }
                    None
                }
            };

            // Skip entries that do not seem to have valid file name
            Ok(raw_entries.map(|raw_entries| {
                raw_entries
                    .into_iter()
                    .filter(|entry| {
                        !ext_uri().is_equal(Some(&entry.resource), Some(&location.history_entries_listing_file), false) // not the listings file
                            && matches_entry_name(&entry.name, &location.entries_extname) // matching our expected file pattern for entries
                    })
                    .collect()
            }))
        })
    }

    fn trace_error(&self, error: &Error) {
        self.log_service.trace(&format!("[Working Copy History Service] {error}"));
    }
}

/// `WorkingCopyHistoryService` (abstrata no original): guarda um [`WorkingCopyHistoryModel`] por working copy.
pub struct WorkingCopyHistoryService {
    this: Weak<WorkingCopyHistoryService>,
    base: Rc<Disposable>,
    on_did_add_entry: Rc<Emitter<WorkingCopyHistoryEvent>>,
    on_did_change_entry: Rc<Emitter<WorkingCopyHistoryEvent>>,
    on_did_replace_entry: Rc<Emitter<WorkingCopyHistoryEvent>>,
    on_did_move_entries: Rc<Emitter<()>>,
    on_did_remove_entry: Rc<Emitter<WorkingCopyHistoryEvent>>,
    on_did_remove_entries: Rc<Emitter<()>>,
    local_history_home: Rc<DeferredPromise<URI>>,
    models: RefCell<ResourceMap<Rc<WorkingCopyHistoryModel>>>,
    file_service: Rc<FileService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    label_service: Rc<dyn ILabelService>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IConfigurationService>,
    /// `getModelOptions()`, o método abstrato.
    get_model_options: Rc<dyn Fn() -> WorkingCopyHistoryModelOptions>,
}

impl WorkingCopyHistoryService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file_service: Rc<FileService>,
        remote_agent_service: Rc<dyn IRemoteAgentService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        label_service: Rc<dyn ILabelService>,
        log_service: LogServiceRef,
        configuration_service: Rc<dyn IConfigurationService>,
        get_model_options: Rc<dyn Fn() -> WorkingCopyHistoryModelOptions>,
    ) -> Rc<Self> {
        // Registra os rótulos das origens de salvar como o carregamento das classes faz
        file_saved_source();
        file_moved_source();
        file_renamed_source();

        let base = Disposable::new();
        let on_did_add_entry = Emitter::new();
        let on_did_change_entry = Emitter::new();
        let on_did_replace_entry = Emitter::new();
        let on_did_move_entries = Emitter::new();
        let on_did_remove_entry = Emitter::new();
        let on_did_remove_entries = Emitter::new();
        base.register(on_did_add_entry.clone());
        base.register(on_did_change_entry.clone());
        base.register(on_did_replace_entry.clone());
        base.register(on_did_move_entries.clone());
        base.register(on_did_remove_entry.clone());
        base.register(on_did_remove_entries.clone());

        let ext_uri = uri_identity_service.ext_uri();
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_add_entry,
            on_did_change_entry,
            on_did_replace_entry,
            on_did_move_entries,
            on_did_remove_entry,
            on_did_remove_entries,
            local_history_home: Rc::new(DeferredPromise::new()),
            models: RefCell::new(ResourceMap::with_key_fn(Rc::new(move |resource: &URI| ext_uri.get_comparison_key(resource, false)))),
            file_service,
            remote_agent_service,
            environment_service,
            uri_identity_service,
            label_service,
            log_service,
            configuration_service,
            get_model_options,
        });

        this.resolve_local_history_home();

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de histórico está vivo enquanto um método dele roda")
    }

    /// Os modelos conhecidos (`this.models.values()`), para a subclasse gravar todos.
    pub fn models(&self) -> Vec<Rc<WorkingCopyHistoryModel>> {
        self.models.borrow().values().cloned().collect()
    }

    /// `logService` (protegido no original).
    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    fn resolve_local_history_home(&self) {
        let this = self.rc();

        let _ = async_fn(async move {
            let mut history_home: Option<URI> = None;

            // Prefer history to be stored in the remote if we are connected to a remote
            match this.remote_agent_service.get_environment().await {
                Ok(Some(remote_env)) => history_home = Some(remote_env.local_history_home),
                Ok(None) => {}
                Err(error) => this.log_service.trace(&error.to_string()), // ignore and fallback to local
            }

            // But fallback to local if there is no remote
            let history_home = history_home.unwrap_or_else(|| this.environment_service.local_history_home());

            let _ = this.local_history_home.complete(history_home);

            Ok(())
        });
    }

    fn do_move_entries(
        self: &Rc<Self>,
        source: Rc<WorkingCopyHistoryModel>,
        save_source: SaveSource,
        source_working_copy_resource: URI,
        target_working_copy_resource: URI,
    ) -> Promise<URI> {
        let this = self.clone();

        async_fn(async move {
            // Move to target via model
            let target = this.get_model(&target_working_copy_resource).await?;
            source.move_entries(&target, save_source, CancellationToken::None).await?;

            // Update model in our map
            {
                let mut models = this.models.borrow_mut();
                models.delete(&source_working_copy_resource);
                models.set(target_working_copy_resource.clone(), source);
            }

            Ok(target_working_copy_resource)
        })
    }

    fn get_model(self: &Rc<Self>, resource: &URI) -> Promise<Rc<WorkingCopyHistoryModel>> {
        let (this, resource) = (self.clone(), resource.clone());

        async_fn(async move {
            let history_home = this.local_history_home.promise.clone().await?;

            let existing = this.models.borrow().get(&resource).cloned();
            let model = match existing {
                Some(model) => model,
                None => {
                    let emitters = WorkingCopyHistoryEmitters {
                        added: this.on_did_add_entry.clone(),
                        changed: this.on_did_change_entry.clone(),
                        replaced: this.on_did_replace_entry.clone(),
                        removed: this.on_did_remove_entry.clone(),
                    };
                    let model = WorkingCopyHistoryModel::new(
                        &resource,
                        &history_home,
                        emitters,
                        (this.get_model_options)(),
                        this.file_service.clone(),
                        this.label_service.clone(),
                        this.log_service.clone(),
                        this.configuration_service.clone(),
                    );
                    this.models.borrow_mut().set(resource, model.clone());
                    model
                }
            };

            Ok(model)
        })
    }
}

impl IDisposable for WorkingCopyHistoryService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyHistoryService for WorkingCopyHistoryService {
    fn on_did_add_entry(&self) -> Event<WorkingCopyHistoryEvent> {
        self.on_did_add_entry.event()
    }

    fn on_did_change_entry(&self) -> Event<WorkingCopyHistoryEvent> {
        self.on_did_change_entry.event()
    }

    fn on_did_replace_entry(&self) -> Event<WorkingCopyHistoryEvent> {
        self.on_did_replace_entry.event()
    }

    fn on_did_remove_entry(&self) -> Event<WorkingCopyHistoryEvent> {
        self.on_did_remove_entry.event()
    }

    fn on_did_move_entries(&self) -> Event<()> {
        self.on_did_move_entries.event()
    }

    fn on_did_remove_entries(&self) -> Event<()> {
        self.on_did_remove_entries.event()
    }

    fn move_entries(&self, source: &URI, target: &URI) -> Promise<Vec<URI>> {
        let (this, source, target) = (self.rc(), source.clone(), target.clone());

        async_fn(async move {
            let limiter: Limiter<URI> = Limiter::new(MAX_PARALLEL_HISTORY_IO_OPS);
            let mut promises: Vec<Promise<URI>> = Vec::new();

            let ext_uri = this.uri_identity_service.ext_uri();
            let models: Vec<(URI, Rc<WorkingCopyHistoryModel>)> = this.models.borrow().entries().map(|(resource, model)| (resource.clone(), model.clone())).collect();
            for (resource, model) in models {
                if !ext_uri.is_equal_or_parent(&resource, &source, false, false) {
                    continue; // model does not match moved resource
                }

                // Determine new resulting target resource
                let target_resource = if ext_uri.is_equal(Some(&source), Some(&resource), false) {
                    target.clone() // file got moved
                } else {
                    // parent folder got moved
                    let path = resource.path();
                    let index = index_of_path(&path.to_string_lossy(), &source.path().to_string_lossy(), false).map_or(-1, |index| index as i64);
                    let start = (index + source.path().len() as i64 + 1).clamp(0, path.len() as i64) as usize;
                    join_path(&target, &[&path.slice(start, path.len()).to_string_lossy()])
                };

                // Figure out save source
                let save_source = if ext_uri.is_equal(Some(&ext_uri.dirname(&resource)), Some(&ext_uri.dirname(&target_resource)), false) {
                    file_renamed_source()
                } else {
                    file_moved_source()
                };

                // Move entries to target queued
                let this = this.clone();
                promises.push(limiter.queue(move || this.do_move_entries(model, save_source, resource, target_resource)));
            }

            if promises.is_empty() {
                return Ok(Vec::new());
            }

            // Await move operations
            let resources = Promise::all(promises).await?;

            // Events
            this.on_did_move_entries.fire(());

            Ok(resources)
        })
    }

    fn add_entry(&self, descriptor: WorkingCopyHistoryEntryDescriptor, token: CancellationToken) -> Promise<Option<WorkingCopyHistoryEntryRef>> {
        let this = self.rc();

        async_fn(async move {
            if !this.file_service.has_provider(&descriptor.resource) {
                return Ok(None); // we require the working copy resource to be file service accessible
            }

            // Resolve history model for working copy
            let model = this.get_model(&descriptor.resource).await?;
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            // Add to model
            Ok(Some(model.add_entry(descriptor.source, None, descriptor.timestamp, token).await?))
        })
    }

    fn update_entry(&self, entry: &WorkingCopyHistoryEntryRef, properties: WorkingCopyHistoryEntryProperties, token: CancellationToken) -> Promise<()> {
        let (this, entry) = (self.rc(), entry.clone());

        async_fn(async move {
            // Resolve history model for working copy
            let model = this.get_model(&entry.working_copy.resource).await?;
            if token.is_cancellation_requested() {
                return Ok(());
            }

            // Rename in model
            model.update_entry(&entry, properties, token).await
        })
    }

    fn remove_entry(&self, entry: &WorkingCopyHistoryEntryRef, token: CancellationToken) -> Promise<bool> {
        let (this, entry) = (self.rc(), entry.clone());

        async_fn(async move {
            // Resolve history model for working copy
            let model = this.get_model(&entry.working_copy.resource).await?;
            if token.is_cancellation_requested() {
                return Ok(false);
            }

            // Remove from model
            model.remove_entry(&entry, token).await
        })
    }

    fn remove_all(&self, token: CancellationToken) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let history_home = this.local_history_home.promise.clone().await?;
            if token.is_cancellation_requested() {
                return Ok(());
            }

            // Clear models
            this.models.borrow_mut().clear();

            // Remove from disk
            this.file_service.del(&history_home, recursive_delete()).await?;

            // Events
            this.on_did_remove_entries.fire(());

            Ok(())
        })
    }

    fn get_entries(&self, resource: &URI, token: CancellationToken) -> Promise<Vec<WorkingCopyHistoryEntryRef>> {
        let (this, resource) = (self.rc(), resource.clone());

        async_fn(async move {
            let model = this.get_model(&resource).await?;
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            model.get_entries().await
        })
    }

    fn get_all(&self, token: CancellationToken) -> Promise<Vec<URI>> {
        let this = self.rc();

        async_fn(async move {
            let history_home = this.local_history_home.promise.clone().await?;
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            let all: Rc<RefCell<ResourceMap<bool>>> = Rc::new(RefCell::new(ResourceMap::new()));

            // Fill in all known model resources (they might not have yet persisted to disk)
            let models: Vec<(URI, Rc<WorkingCopyHistoryModel>)> = this.models.borrow().entries().map(|(resource, model)| (resource.clone(), model.clone())).collect();
            for (resource, model) in models {
                let has_in_memory_entries = model.has_entries(true /* skip resolving because we resolve below from disk */).await?;
                if has_in_memory_entries {
                    all.borrow_mut().set(resource, true);
                }
            }

            // Resolve all other resources by iterating the history home folder
            // (ignore errors: history might be entirely empty)
            if let Ok(resolved_history_home) = this.file_service.resolve(&history_home, None).await
                && let Some(children) = resolved_history_home.children
            {
                let limiter: Limiter<()> = Limiter::new(MAX_PARALLEL_HISTORY_IO_OPS);
                let mut promises: Vec<Promise<()>> = Vec::new();

                for child in children {
                    let (file_service, all, token) = (this.file_service.clone(), all.clone(), token.clone());
                    promises.push(limiter.queue(move || {
                        async_fn(async move {
                            if token.is_cancellation_requested() {
                                return Ok(());
                            }

                            // ignore - model might be missing or corrupt, but we need it
                            let entries_file = join_path(&child.resource, &[WorkingCopyHistoryModel::ENTRIES_FILE]);
                            if let Ok(serialized_model) = read_serialized_model(&file_service, &entries_file).await
                                && !serialized_model.entries.is_empty()
                            {
                                all.borrow_mut().set(URI::parse(serialized_model.resource.as_str(), false), true);
                            }

                            Ok(())
                        })
                    }));
                }

                Promise::all(promises).await?;
            }

            let resources: Vec<URI> = all.borrow().keys().cloned().collect();

            Ok(resources)
        })
    }
}

/// `STORE_ALL_INTERVAL`.
const STORE_ALL_INTERVAL: u64 = 5 * 60 * 1000; // 5min

/// `NativeWorkingCopyHistoryService`.
pub struct NativeWorkingCopyHistoryService {
    base: Rc<Disposable>,
    service: Rc<WorkingCopyHistoryService>,
    store_all_cts: CancellationTokenSource,
    store_all_scheduler: Rc<RunOnceScheduler>,
}

impl NativeWorkingCopyHistoryService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file_service: Rc<FileService>,
        remote_agent_service: Rc<dyn IRemoteAgentService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        label_service: Rc<dyn ILabelService>,
        lifecycle_service: Rc<dyn ILifecycleService>,
        log_service: LogServiceRef,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let is_remotely_stored = environment_service.remote_authority().is_some();
        let service = WorkingCopyHistoryService::new(
            file_service,
            remote_agent_service,
            environment_service,
            uri_identity_service,
            label_service,
            log_service,
            configuration_service,
            // because the connection might drop anytime
            Rc::new(move || WorkingCopyHistoryModelOptions { flush_on_change: is_remotely_stored }),
        );

        let base = Disposable::new();
        base.register(service.clone());

        let store_all_cts = CancellationTokenSource::default();
        base.register(Rc::new(store_all_cts.clone()));
        let store_all_scheduler = {
            let (service, store_all_cts) = (service.clone(), store_all_cts.clone());
            RunOnceScheduler::new(
                move || {
                    let _ = Self::store_all(&service, &store_all_cts.token());
                },
                STORE_ALL_INTERVAL,
            )
        };
        base.register(store_all_scheduler.clone());

        let this = Rc::new(Self { base, service, store_all_cts, store_all_scheduler });
        this.register_listeners(is_remotely_stored, &lifecycle_service);

        this
    }

    /// O [`WorkingCopyHistoryService`] que esta classe estende, como o contrato `IWorkingCopyHistoryService`.
    pub fn service(&self) -> Rc<dyn IWorkingCopyHistoryService> {
        self.service.clone()
    }

    fn register_listeners(&self, is_remotely_stored: bool, lifecycle_service: &Rc<dyn ILifecycleService>) {
        if is_remotely_stored {
            return;
        }

        // Local: persist all on shutdown
        let (service, store_all_cts, store_all_scheduler) = (self.service.clone(), self.store_all_cts.clone(), self.store_all_scheduler.clone());
        self.base.register(lifecycle_service.on_will_shutdown().listen(move |e| Self::on_will_shutdown(&service, &store_all_cts, &store_all_scheduler, e)));

        // Local: schedule persist on change
        let store_all_scheduler = self.store_all_scheduler.clone();
        let changes = je_base::event::any(vec![
            self.service.on_did_add_entry(),
            self.service.on_did_change_entry(),
            self.service.on_did_replace_entry(),
            self.service.on_did_remove_entry(),
        ]);
        self.base.register(changes.listen(move |_| Self::on_did_change_models(&store_all_scheduler)));
    }

    fn on_will_shutdown(service: &Rc<WorkingCopyHistoryService>, store_all_cts: &CancellationTokenSource, store_all_scheduler: &Rc<RunOnceScheduler>, e: &WillShutdownEvent) {
        // Dispose the scheduler...
        store_all_scheduler.dispose();
        store_all_cts.dispose_with(true);

        // ...because we now explicitly store all models
        e.join(
            Self::store_all(service, &e.token),
            WillShutdownEventJoiner { id: "join.workingCopyHistory".to_string(), label: localize("join.workingCopyHistory", "Saving local history", &[]), order: None },
        );
    }

    fn on_did_change_models(store_all_scheduler: &Rc<RunOnceScheduler>) {
        if !store_all_scheduler.is_scheduled() {
            store_all_scheduler.schedule(None);
        }
    }

    fn store_all(service: &Rc<WorkingCopyHistoryService>, token: &CancellationToken) -> Promise<()> {
        let limiter: Limiter<()> = Limiter::new(MAX_PARALLEL_HISTORY_IO_OPS);
        let mut promises: Vec<Promise<()>> = Vec::new();

        for model in service.models() {
            let (token, log_service) = (token.clone(), service.log_service().clone());
            promises.push(limiter.queue(move || {
                async_fn(async move {
                    if token.is_cancellation_requested() {
                        return Ok(());
                    }

                    if let Err(error) = model.store(&token).await {
                        log_service.trace(&error.to_string());
                    }

                    Ok(())
                })
            }));
        }

        Promise::all(promises).then(|_| ())
    }
}

impl Deref for NativeWorkingCopyHistoryService {
    type Target = WorkingCopyHistoryService;

    fn deref(&self) -> &WorkingCopyHistoryService {
        &self.service
    }
}

impl IDisposable for NativeWorkingCopyHistoryService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
