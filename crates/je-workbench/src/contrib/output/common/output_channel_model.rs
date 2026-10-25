//! Porte de `src/vs/workbench/contrib/output/common/outputChannelModel.ts`: o `ILogEntry` (de
//! `services/output/common/output.ts`), `parseLogEntryAt`, os provedores de conteúdo (`FileContentProvider` e
//! `MultiFileContentProvider`) e os modelos de canal (`FileOutputChannelModel`, `MultiFileOutputChannelModel`,
//! `OutputChannelBackedByFile` e `DelegatedOutputChannelModel`).
//!
//! Notas de tradução:
//! - **`new Date(match[1]).getTime()`** é o `Date.parse` do porte ([`try_parse_date`], fuso local para data e hora sem
//!   deslocamento); um valor que não interpreta vira `NaN`, como no JavaScript.
//! - **`LOG_ENTRY_REGEX`** é um [`JsRegExp`] sem estado (sem `g`), então cada `exec` parte do zero.
//! - **`ITextModel`** é o [`TextModel`] concreto.
//! - **Injeção de dependências.** Os decoradores do construtor (`@IFileService`, `@IModelService`, `@ILogService`,
//!   `@ILoggerService`, `@IEditorWorkerService` e o `@IInstantiationService` que só serve para `createInstance(TextModel, ...)`)
//!   viram o [`OutputChannelModelServices`], passado a cada construtor. O `createInstance(TextModel, content, LOG_MIME,
//!   TextModel.DEFAULT_CREATION_OPTIONS, null)` é o `TextModel::new` com o `UndoRedoService` do pacote (o modelo temporário
//!   que lê as entradas de log e é descartado em seguida).
//! - **Classes abstratas.** `AbstractFileOutputChannelModel` é a struct de mesmo nome, e os métodos abstratos e as
//!   sobrescritas das subclasses (`clear`, `update`, `updateChannelSources`, `append`, `replace`, `source`) são o trait
//!   [`FileChannelBehavior`], um por subclasse: `FileOutputChannelModel`, `MultiFileOutputChannelModel` e
//!   `OutputChannelBackedByFile` são as funções `create` dos tipos de mesmo nome, que devolvem a struct com o comportamento
//!   da subclasse. Os métodos que a classe abstrata lança como "Not supported" são o corpo padrão do trait.
//! - **`source`** é `IOutputContentSource | ReadonlyArray<IOutputContentSource>`: sempre a lista (uma fonte, no caso de um
//!   arquivo só).
//! - **`async`/`await`** são cadeias de [`Promise`]; os `Disposable` herdados são o [`DisposableStore`] do tipo, e o
//!   `_register(x)` é `store.add(x)`.
//! - **`IEditorWorkerService.computeMoreMinimalEdits`.** O contrato é o do porte; o serviço que o implementa com um modelo
//!   (o `compute_more_minimal_edits` de `editor_web_worker.rs`, que é o corpo do `$computeMoreMinimalEdits` do worker) é de
//!   quem monta o `IEditorWorkerService`.

use std::cell::{Cell, LazyCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays::{binary_search, sorted_diff};
use je_base::async_::{ThrottledDelayer, promises};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::js_string::{JsString, substring};
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::editor_worker::IEditorWorkerService;
use je_editor::language_service::ILanguageSelection;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::files::files::{FileOperationResult, FileReadStreamOptions, ReadFileOptions, to_file_operation_result};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogLevel, LogService, Logger, LoggerLogLevel, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use je_platform::resources::{ext_uri, join_path};

use crate::contrib::update::update_utils::try_parse_date;
use crate::services::output::output::{IOutputContentSource, LOG_MIME, OutputChannelUpdateMode};

/// `ILogEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct ILogEntry {
    pub range: Range,
    pub timestamp: f64,
    pub timestamp_range: Range,
    pub log_level: LogLevel,
    pub log_level_range: Range,
    pub category: Option<String>,
}

thread_local! {
    static LOG_ENTRY_REGEX: LazyCell<JsRegExp> = LazyCell::new(|| {
        JsRegExp::from_str(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s(\[(info|trace|debug|error|warning)\])\s(\[(.*?)\])?", "")
            .expect("regex do registro de log válida")
    });
}

/// `parseLogEntryAt`.
pub fn parse_log_entry_at(model: &TextModel, line_number: i32) -> Option<ILogEntry> {
    let line_content = CursorSimpleModel::get_line_content(model, line_number);
    let found = LOG_ENTRY_REGEX.with(|regex| regex.exec(line_content.units()))?;
    let group = |index: usize| found.captures.get(index).and_then(|range| range.clone()).map(|range| JsString::from(&line_content.units()[range]));

    let timestamp_text = group(1).expect("o grupo do carimbo de tempo participa").to_string_lossy();
    let timestamp = try_parse_date(Some(&timestamp_text)).unwrap_or(f64::NAN);
    let timestamp_range = Range::new(line_number, 1, line_number, timestamp_text.encode_utf16().count() as i32);
    let log_level = parse_log_level(&group(3).expect("o grupo do nível participa").to_string_lossy());
    let log_level_text_length = group(2).expect("o grupo do nível entre colchetes participa").len() as i32;
    let log_level_range = Range::new(line_number, timestamp_range.end_column + 1, line_number, timestamp_range.end_column + 1 + log_level_text_length);
    let category = group(5).map(|category| category.to_string_lossy());
    let start_line = line_number;
    let mut end_line = line_number;

    let line_count = CursorSimpleModel::get_line_count(model);
    while end_line < line_count {
        let next_line_content = CursorSimpleModel::get_line_content(model, end_line + 1);
        let is_last_line = end_line + 1 == line_count && next_line_content.is_empty(); // Last line will be always empty
        if is_last_line || LOG_ENTRY_REGEX.with(|regex| regex.exec(next_line_content.units()).is_some()) {
            break;
        }
        end_line += 1;
    }
    let range = Range::new(start_line, 1, end_line, CursorSimpleModel::get_line_max_column(model, end_line));
    Some(ILogEntry { range, timestamp, timestamp_range, log_level, log_level_range, category })
}

/// `logEntryIterator(model, process)`.
fn log_entry_iterator<'a, T>(model: &'a TextModel, mut process: impl FnMut(ILogEntry) -> T + 'a) -> impl Iterator<Item = T> + 'a {
    let mut line_number = 1;
    std::iter::from_fn(move || {
        while line_number <= CursorSimpleModel::get_line_count(model) {
            if let Some(log_entry) = parse_log_entry_at(model, line_number) {
                line_number = log_entry.range.end_line_number + 1;
                return Some(process(log_entry));
            }
            line_number += 1;
        }
        None
    })
}

/// `changeStartLineNumber(logEntry, lineNumber)`.
fn change_start_line_number(log_entry: &ILogEntry, line_number: i32) -> ILogEntry {
    ILogEntry {
        range: Range::new(
            line_number,
            log_entry.range.start_column,
            line_number + log_entry.range.end_line_number - log_entry.range.start_line_number,
            log_entry.range.end_column,
        ),
        timestamp_range: Range::new(line_number, log_entry.timestamp_range.start_column, line_number, log_entry.timestamp_range.end_column),
        log_level_range: Range::new(line_number, log_entry.log_level_range.start_column, line_number, log_entry.log_level_range.end_column),
        ..log_entry.clone()
    }
}

fn parse_log_level(level: &str) -> LogLevel {
    match level.to_lowercase().as_str() {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warning" => LogLevel::Warning,
        "error" => LogLevel::Error,
        _ => errors::throw(Error::new(format!("Unknown log level: {level}"))),
    }
}

/// Os serviços que os decoradores `@I...` dos construtores do original injetam (ver as notas do módulo).
#[derive(Clone)]
pub struct OutputChannelModelServices {
    pub file_service: Rc<dyn IFileService>,
    pub model_service: Rc<dyn IModelService>,
    pub log_service: Rc<dyn LogService>,
    pub logger_service: Rc<dyn LoggerService>,
    pub editor_worker_service: Rc<dyn IEditorWorkerService>,
    pub undo_redo_service: Rc<UndoRedoService>,
}

impl OutputChannelModelServices {
    /// `instantiationService.createInstance(TextModel, content, LOG_MIME, TextModel.DEFAULT_CREATION_OPTIONS, null)`.
    fn create_log_model(&self, content: impl Into<TextSource>) -> Rc<TextModel> {
        TextModel::new(content.into(), LOG_MIME, TextModel::default_creation_options(), None, self.undo_redo_service.clone())
    }
}

/// `IOutputChannelModel`.
pub trait IOutputChannelModel: IDisposable {
    fn on_dispose(&self) -> Event<()>;
    fn source(&self) -> Vec<IOutputContentSource>;
    fn get_log_entries(&self) -> Vec<ILogEntry>;
    fn append(&self, output: &str);
    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>, immediate: bool);
    fn update_channel_sources(&self, sources: Vec<IOutputContentSource>);
    fn load_model(&self) -> Promise<Rc<TextModel>>;
    fn clear(&self);
    fn replace(&self, value: &str);
}

/// O que `getContent()` devolve: `{ name?, content, consume }`.
#[derive(Clone)]
pub struct ContentChunk {
    pub name: String,
    pub content: String,
    pub consume: Rc<dyn Fn()>,
}

impl ContentChunk {
    fn empty(name: &str) -> Self {
        Self { name: name.to_owned(), content: String::new(), consume: Rc::new(|| { /* No Op */ }) }
    }
}

/// `IContentProvider`.
pub trait IContentProvider: IDisposable {
    fn on_did_append(&self) -> Event<()>;
    fn on_did_reset(&self) -> Event<()>;
    fn reset(&self);
    fn watch(&self);
    fn unwatch(&self);
    fn get_content(&self) -> Promise<ContentChunk>;
    fn get_log_entries(&self) -> Vec<ILogEntry>;
}

/// `promise.catch(error => { if (!isCancellationError(error)) throw error; })`: o erro que não é de cancelamento vira um
/// erro inesperado, como a rejeição não tratada do original.
fn ignore_cancellation(promise: Promise<()>) {
    promise.catch(|error| {
        if !is_cancellation_error(&error) {
            errors::on_unexpected_error(&error);
        }
    });
}

/// `FileContentProvider`.
pub struct FileContentProvider {
    me: Weak<FileContentProvider>,
    on_did_append: Rc<Emitter<()>>,
    on_did_reset: Rc<Emitter<()>>,
    watching: Cell<bool>,
    sync_delayer: ThrottledDelayer<()>,
    etag: RefCell<Option<String>>,
    log_entries: RefCell<Vec<ILogEntry>>,
    start_offset: Cell<u64>,
    end_offset: Cell<u64>,
    pub resource: URI,
    pub name: String,
    services: OutputChannelModelServices,
}

impl FileContentProvider {
    pub fn new(source: &IOutputContentSource, services: &OutputChannelModelServices) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            on_did_append: Emitter::new(),
            on_did_reset: Emitter::new(),
            watching: Cell::new(false),
            sync_delayer: ThrottledDelayer::new(500),
            etag: RefCell::new(Some(String::new())),
            log_entries: RefCell::new(Vec::new()),
            start_offset: Cell::new(0),
            end_offset: Cell::new(0),
            resource: source.resource.clone(),
            name: source.name.clone().unwrap_or_default(),
            services: services.clone(),
        })
    }

    /// `reset(offset?)`.
    pub fn reset_at(&self, offset: Option<u64>) {
        let offset = offset.unwrap_or(self.start_offset.get());
        self.start_offset.set(offset);
        self.end_offset.set(offset);
        self.log_entries.borrow_mut().clear();
    }

    pub fn reset_to_end(&self) {
        self.start_offset.set(self.end_offset.get());
        self.log_entries.borrow_mut().clear();
    }

    fn poll(&self) {
        let weak = self.me.clone();
        let polling = self.sync_delayer.trigger(
            move |_token| {
                let Some(this) = weak.upgrade() else {
                    return Promise::resolved(());
                };
                this.do_watch().then_promise(move |()| {
                    if let Some(this) = weak.upgrade() {
                        this.poll();
                    }
                    Promise::resolved(())
                })
            },
            None,
        );
        ignore_cancellation(polling);
    }

    fn do_watch(&self) -> Promise<()> {
        if !self.services.file_service.has_provider(&self.resource) {
            return Promise::resolved(());
        }
        let weak = self.me.clone();
        self.services.file_service.stat(&self.resource).then_result(move |result| match result {
            Ok(stat) => {
                if let Some(this) = weak.upgrade()
                    && *this.etag.borrow() != stat.etag
                {
                    *this.etag.borrow_mut() = stat.etag.clone();
                    if stat.size.is_some_and(|size| this.end_offset.get() > size) {
                        this.reset_at(Some(0));
                        this.on_did_reset.fire(());
                    } else {
                        this.on_did_append.fire(());
                    }
                }
                Ok(())
            }
            Err(error) => {
                if to_file_operation_result(&error) != FileOperationResult::FileNotFound {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        })
    }

    /// `getContent(donotConsumeLogEntries?)`.
    pub fn get_content_with(&self, donot_consume_log_entries: bool) -> Promise<ContentChunk> {
        let name = self.name.clone();
        if !self.services.file_service.has_provider(&self.resource) {
            return Promise::resolved(ContentChunk::empty(&name));
        }
        let weak = self.me.clone();
        let options = ReadFileOptions { stream: FileReadStreamOptions { position: Some(self.end_offset.get()), ..Default::default() }, ..Default::default() };
        self.services.file_service.read_file(&self.resource, Some(options), None).then_result(move |result| match result {
            Ok(file_content) => {
                let Some(this) = weak.upgrade() else {
                    return Ok(ContentChunk::empty(&name));
                };
                let content = file_content.value.to_string();
                let last_entry = this.log_entries.borrow().last().cloned();
                let log_entries = if donot_consume_log_entries { Vec::new() } else { this.parse_log_entries(&content, last_entry.as_ref()) };
                let consumed = Cell::new(false);
                let byte_length = file_content.value.byte_length() as u64;
                let etag = file_content.etag().to_owned();
                Ok(ContentChunk {
                    name,
                    content,
                    consume: Rc::new(move || {
                        if !consumed.replace(true) {
                            this.end_offset.set(this.end_offset.get() + byte_length);
                            *this.etag.borrow_mut() = Some(etag.clone());
                            this.log_entries.borrow_mut().extend(log_entries.iter().cloned());
                        }
                    }),
                })
            }
            Err(error) => {
                if to_file_operation_result(&error) != FileOperationResult::FileNotFound {
                    Err(error)
                } else {
                    Ok(ContentChunk::empty(&name))
                }
            }
        })
    }

    fn parse_log_entries(&self, content: &str, last_log_entry: Option<&ILogEntry>) -> Vec<ILogEntry> {
        let model = self.services.create_log_model(content);
        let log_entries = (|| {
            if parse_log_entry_at(&model, 1).is_none() {
                return Vec::new();
            }
            let mut log_entries = Vec::new();
            // O closure lê o número a cada entrada, como o `logEntryStartLineNumber` capturado no original.
            let log_entry_start_line_number = std::cell::Cell::new(last_log_entry.map_or(1, |entry| entry.range.end_line_number + 1));
            for entry in log_entry_iterator(&model, |entry| change_start_line_number(&entry, log_entry_start_line_number.get())) {
                log_entry_start_line_number.set(entry.range.end_line_number + 1);
                log_entries.push(entry);
            }
            log_entries
        })();
        model.dispose();
        log_entries
    }
}

impl IContentProvider for FileContentProvider {
    fn on_did_append(&self) -> Event<()> {
        self.on_did_append.event()
    }

    fn on_did_reset(&self) -> Event<()> {
        self.on_did_reset.event()
    }

    fn reset(&self) {
        self.reset_at(None);
    }

    fn watch(&self) {
        if !self.watching.get() {
            self.services.log_service.trace(&format!("Started polling {}", self.resource.to_string(false).to_string_lossy()));
            self.poll();
            self.watching.set(true);
        }
    }

    fn unwatch(&self) {
        if self.watching.get() {
            self.sync_delayer.cancel();
            self.watching.set(false);
            self.services.log_service.trace(&format!("Stopped polling {}", self.resource.to_string(false).to_string_lossy()));
        }
    }

    fn get_content(&self) -> Promise<ContentChunk> {
        self.get_content_with(false)
    }

    fn get_log_entries(&self) -> Vec<ILogEntry> {
        self.log_entries.borrow().clone()
    }
}

impl IDisposable for FileContentProvider {
    fn dispose(&self) {
        self.unwatch();
        self.sync_delayer.dispose();
        self.on_did_append.dispose();
        self.on_did_reset.dispose();
    }
}

/// `process(model, logEntry, name)` do `combineLogEntries`: a entrada com o nome do arquivo como categoria, e o texto dela.
fn process_log_entry(model: &TextModel, log_entry: ILogEntry, name: &str) -> (ILogEntry, JsString) {
    let line_content = model.get_value_in_range(&log_entry.range);
    let name_length = name.encode_utf16().count() as i32;
    let content = if name.is_empty() {
        line_content
    } else {
        let units = line_content.units();
        let split = i64::from(log_entry.log_level_range.end_column);
        let mut combined = substring(units, 0, split).to_vec();
        combined.extend(" [".encode_utf16());
        combined.extend(name.encode_utf16());
        combined.push(u16::from(b']'));
        combined.extend_from_slice(substring(units, split, units.len() as i64));
        JsString::from_units(combined)
    };
    let end_column = if name.is_empty() { log_entry.range.end_column } else { log_entry.range.end_column + name_length + 3 };
    let range = Range::new(log_entry.range.start_line_number, log_entry.log_level_range.start_column, log_entry.range.end_line_number, end_column);
    (ILogEntry { category: Some(name.to_owned()), range, ..log_entry }, content)
}

/// `MultiFileContentProvider`.
pub struct MultiFileContentProvider {
    me: Weak<MultiFileContentProvider>,
    on_did_append: Rc<Emitter<()>>,
    log_entries: RefCell<Vec<ILogEntry>>,
    file_content_provider_items: RefCell<Vec<(Rc<FileContentProvider>, Rc<DisposableStore>)>>,
    watching: Cell<bool>,
    services: OutputChannelModelServices,
}

impl MultiFileContentProvider {
    pub fn new(files_infos: &[IOutputContentSource], services: &OutputChannelModelServices) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            on_did_append: Emitter::new(),
            log_entries: RefCell::new(Vec::new()),
            file_content_provider_items: RefCell::new(Vec::new()),
            watching: Cell::new(false),
            services: services.clone(),
        });
        for file in files_infos {
            let item = this.create_file_content_provider(file);
            this.file_content_provider_items.borrow_mut().push(item);
        }
        this
    }

    fn create_file_content_provider(&self, file: &IOutputContentSource) -> (Rc<FileContentProvider>, Rc<DisposableStore>) {
        let disposables = DisposableStore::new();
        let file_output = FileContentProvider::new(file, &self.services);
        disposables.add(file_output.clone());
        let on_did_append = self.on_did_append.clone();
        disposables.add(file_output.on_did_append().listen(move |_| on_did_append.fire(())));
        (file_output, disposables)
    }

    fn providers(&self) -> Vec<Rc<FileContentProvider>> {
        self.file_content_provider_items.borrow().iter().map(|(output, _)| output.clone()).collect()
    }

    pub fn update_files(&self, files: &[IOutputContentSource]) {
        let was_watching = self.watching.get();
        if was_watching {
            self.unwatch();
        }

        let current: Vec<IOutputContentSource> =
            self.providers().iter().map(|output| IOutputContentSource { name: Some(output.name.clone()), resource: output.resource.clone() }).collect();
        let result = sorted_diff(&current, files, |a, b| f64::from(ext_uri().compare(&a.resource, &b.resource, false)));
        for splice in result {
            let outputs: Vec<_> = splice.to_insert.iter().map(|file| self.create_file_content_provider(file)).collect();
            let outputs_to_remove: Vec<_> = self.file_content_provider_items.borrow_mut().splice(splice.start..splice.start + splice.delete_count, outputs).collect();
            for (_, disposables) in outputs_to_remove {
                disposables.dispose();
            }
        }

        if was_watching {
            self.watch();
        }
    }

    pub fn reset_to_end(&self) {
        for output in self.providers() {
            output.reset_to_end();
        }
        self.log_entries.borrow_mut().clear();
    }

    /// `combineLogEntries(outputs, lastEntry)`.
    fn combine_log_entries(&self, outputs: Vec<ContentChunk>, last_entry: Option<&ILogEntry>) -> (Vec<ILogEntry>, String) {
        let outputs: Vec<ContentChunk> = outputs.into_iter().filter(|output| !output.content.is_empty()).collect();

        if outputs.is_empty() {
            return (Vec::new(), String::new());
        }

        let mut log_entries: Vec<ILogEntry> = Vec::new();
        let mut contents: Vec<JsString> = Vec::new();

        let model = self.services.create_log_model(outputs[0].content.as_str());
        for (log_entry, content) in log_entry_iterator(&model, |entry| process_log_entry(&model, entry, &outputs[0].name)) {
            log_entries.push(log_entry);
            contents.push(content);
        }
        model.dispose();

        for ContentChunk { content, name, .. } in &outputs[1..] {
            let model = self.services.create_log_model(content.as_str());
            {
                let mut iterator = log_entry_iterator(&model, |entry| process_log_entry(&model, entry, name));
                let mut next = iterator.next();
                while let Some((log_entry, content)) = next {
                    let mut log_entries_to_add = vec![log_entry.clone()];
                    let mut contents_to_add = vec![content];

                    let insertion_index;

                    // If the timestamp is greater than or equal to the last timestamp,
                    // we can just append all the entries at the end
                    if log_entry.timestamp >= log_entries[log_entries.len() - 1].timestamp {
                        insertion_index = log_entries.len();
                        next = iterator.next();
                        while let Some((entry, content)) = next {
                            log_entries_to_add.push(entry);
                            contents_to_add.push(content);
                            next = iterator.next();
                        }
                    } else {
                        if log_entry.timestamp <= log_entries[0].timestamp {
                            // If the timestamp is less than or equal to the first timestamp
                            // then insert at the beginning
                            insertion_index = 0;
                        } else {
                            // Otherwise, find the insertion index
                            let index = binary_search(&log_entries, &log_entry, |a, b| a.timestamp - b.timestamp);
                            insertion_index = (if index < 0 { !index } else { index }) as usize;
                        }

                        // Collect all entries that have a timestamp less than or equal to the timestamp at the insertion index
                        next = iterator.next();
                        loop {
                            match next {
                                Some((entry, content)) if entry.timestamp <= log_entries[insertion_index].timestamp => {
                                    log_entries_to_add.push(entry);
                                    contents_to_add.push(content);
                                    next = iterator.next();
                                }
                                _ => break,
                            }
                        }
                    }

                    contents.splice(insertion_index..insertion_index, contents_to_add);
                    log_entries.splice(insertion_index..insertion_index, log_entries_to_add);
                }
            }
            model.dispose();
        }

        let mut content: Vec<u16> = Vec::new();
        let mut updated_log_entries: Vec<ILogEntry> = Vec::new();
        let mut log_entry_start_line_number = last_entry.map_or(1, |entry| entry.range.end_line_number + 1);
        for (log_entry, entry_content) in log_entries.iter().zip(&contents) {
            content.extend_from_slice(entry_content.units());
            content.push(u16::from(b'\n'));
            let updated_log_entry = change_start_line_number(log_entry, log_entry_start_line_number);
            log_entry_start_line_number = updated_log_entry.range.end_line_number + 1;
            updated_log_entries.push(updated_log_entry);
        }

        (updated_log_entries, JsString::from_units(content).to_string_lossy())
    }
}

impl IContentProvider for MultiFileContentProvider {
    fn on_did_append(&self) -> Event<()> {
        self.on_did_append.event()
    }

    fn on_did_reset(&self) -> Event<()> {
        Event::none()
    }

    fn reset(&self) {
        for output in self.providers() {
            output.reset_at(None);
        }
        self.log_entries.borrow_mut().clear();
    }

    fn watch(&self) {
        if !self.watching.get() {
            self.watching.set(true);
            for output in self.providers() {
                output.watch();
            }
        }
    }

    fn unwatch(&self) {
        if self.watching.get() {
            self.watching.set(false);
            for output in self.providers() {
                output.unwatch();
            }
        }
    }

    fn get_content(&self) -> Promise<ContentChunk> {
        let weak = self.me.clone();
        let outputs = Promise::all(self.providers().iter().map(|output| output.get_content_with(true)).collect());
        outputs.then(move |outputs| {
            let this = weak.upgrade().expect("o provedor de vários arquivos vive enquanto o conteúdo é lido");
            let last_entry = this.log_entries.borrow().last().cloned();
            let (log_entries, content) = this.combine_log_entries(outputs.clone(), last_entry.as_ref());
            let consumed = Cell::new(false);
            let consumers: Vec<Rc<dyn Fn()>> = outputs.iter().map(|output| output.consume.clone()).collect();
            ContentChunk {
                name: String::new(),
                content,
                consume: Rc::new(move || {
                    if !consumed.replace(true) {
                        for consume in &consumers {
                            consume();
                        }
                        this.log_entries.borrow_mut().extend(log_entries.iter().cloned());
                    }
                }),
            }
        })
    }

    fn get_log_entries(&self) -> Vec<ILogEntry> {
        self.log_entries.borrow().clone()
    }
}

impl IDisposable for MultiFileContentProvider {
    fn dispose(&self) {
        for (_, disposables) in self.file_content_provider_items.borrow().iter() {
            disposables.dispose();
        }
        self.on_did_append.dispose();
    }
}

/// O que as subclasses de `AbstractFileOutputChannelModel` acrescentam: os membros abstratos e as sobrescritas. O corpo
/// padrão dos que a classe abstrata lança é o `throw new Error('Not supported')` dela.
pub trait FileChannelBehavior {
    fn source(&self) -> Vec<IOutputContentSource>;
    fn clear(&self, channel: &Rc<AbstractFileOutputChannelModel>);

    fn append(&self, _channel: &Rc<AbstractFileOutputChannelModel>, _message: &str) {
        errors::throw(Error::new("Not supported"))
    }

    fn replace(&self, _channel: &Rc<AbstractFileOutputChannelModel>, _message: &str) {
        errors::throw(Error::new("Not supported"))
    }

    fn update(&self, _channel: &Rc<AbstractFileOutputChannelModel>, _mode: OutputChannelUpdateMode, _till: Option<u64>, _immediate: bool) {
        errors::throw(Error::new("Not supported"))
    }

    fn update_channel_sources(&self, _channel: &Rc<AbstractFileOutputChannelModel>, _files: Vec<IOutputContentSource>) {
        errors::throw(Error::new("Not supported"))
    }
}

/// `AbstractFileOutputChannelModel`.
pub struct AbstractFileOutputChannelModel {
    me: Weak<AbstractFileOutputChannelModel>,
    on_dispose: Rc<Emitter<()>>,
    load_model_promise: RefCell<Option<Promise<Rc<TextModel>>>>,
    model_disposable: Rc<MutableDisposable<DisposableStore>>,
    model: RefCell<Option<Rc<TextModel>>>,
    model_update_in_progress: Cell<bool>,
    model_update_cancellation_source: RefCell<Option<Rc<CancellationTokenSource>>>,
    append_throttler: ThrottledDelayer<()>,
    replace_promise: RefCell<Option<Promise<()>>>,
    model_uri: URI,
    language: Rc<dyn ILanguageSelection>,
    output_content_provider: Rc<dyn IContentProvider>,
    services: OutputChannelModelServices,
    behavior: Box<dyn FileChannelBehavior>,
}

impl AbstractFileOutputChannelModel {
    pub fn new(
        model_uri: URI,
        language: Rc<dyn ILanguageSelection>,
        output_content_provider: Rc<dyn IContentProvider>,
        services: &OutputChannelModelServices,
        behavior: Box<dyn FileChannelBehavior>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            on_dispose: Emitter::new(),
            load_model_promise: RefCell::new(None),
            model_disposable: MutableDisposable::new(),
            model: RefCell::new(None),
            model_update_in_progress: Cell::new(false),
            model_update_cancellation_source: RefCell::new(None),
            append_throttler: ThrottledDelayer::new(300),
            replace_promise: RefCell::new(None),
            model_uri,
            language,
            output_content_provider,
            services: services.clone(),
            behavior,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o modelo de canal vive enquanto é usado")
    }

    /// `loadModelPromise` (o valor atual, se `loadModel` já foi chamado).
    fn pending_load(&self) -> Promise<()> {
        match self.load_model_promise.borrow().clone() {
            Some(promise) => promise.then(|_| ()),
            None => Promise::resolved(()),
        }
    }

    fn on_did_content_change(&self, reset: bool, append_immediately: bool) {
        if reset && !self.model_update_in_progress.get() {
            self.do_update(OutputChannelUpdateMode::Clear, true);
        }
        self.do_update(OutputChannelUpdateMode::Append, append_immediately);
    }

    pub fn do_update(&self, mode: OutputChannelUpdateMode, immediate: bool) {
        if mode == OutputChannelUpdateMode::Clear || mode == OutputChannelUpdateMode::Replace {
            self.cancel_model_update();
        }
        let Some(model) = self.model.borrow().clone() else {
            return;
        };

        self.model_update_in_progress.set(true);
        let token = self.model_update_cancellation_source.borrow_mut().get_or_insert_with(|| Rc::new(CancellationTokenSource::new(None))).token();

        if mode == OutputChannelUpdateMode::Clear {
            self.clear_content(&model);
        } else if mode == OutputChannelUpdateMode::Replace {
            let weak = self.me.clone();
            let replacing = self.replace_content(model, token).finally(move || {
                if let Some(this) = weak.upgrade() {
                    *this.replace_promise.borrow_mut() = None;
                }
            });
            *self.replace_promise.borrow_mut() = Some(replacing);
        } else {
            self.append_content(model, immediate, token);
        }
    }

    fn clear_content(&self, model: &TextModel) {
        model.apply_edits(&[EditOperation::delete(model.get_full_model_range())]);
        self.model_update_in_progress.set(false);
    }

    fn append_content(&self, model: Rc<TextModel>, immediate: bool, token: CancellationToken) {
        let weak = self.me.clone();
        let appending = self.append_throttler.trigger(
            move |_token| {
                /* Abort if operation is cancelled */
                if token.is_cancellation_requested() {
                    return Promise::resolved(());
                }
                let Some(this) = weak.upgrade() else {
                    return Promise::resolved(());
                };

                /* Wait for replace to finish */
                let replace_finished = match this.replace_promise.borrow().clone() {
                    Some(replacing) => replacing.catch(|_| { /* Ignore */ }),
                    None => Promise::resolved(()),
                };
                replace_finished.then_promise(move |()| {
                    /* Abort if operation is cancelled */
                    if token.is_cancellation_requested() {
                        return Promise::resolved(());
                    }

                    /* Get content to append */
                    this.output_content_provider.get_content().then(move |chunk| {
                        /* Abort if operation is cancelled */
                        if token.is_cancellation_requested() {
                            return;
                        }

                        /* Appned Content */
                        (chunk.consume)();
                        this.do_append_content(&model, &chunk.content);
                        this.model_update_in_progress.set(false);
                    })
                })
            },
            if immediate { Some(0) } else { None },
        );
        ignore_cancellation(appending);
    }

    fn do_append_content(&self, model: &TextModel, content: &str) {
        let last_line = CursorSimpleModel::get_line_count(model);
        let last_line_max_column = CursorSimpleModel::get_line_max_column(model, last_line);
        model.apply_edits(&[EditOperation::insert(Position::new(last_line, last_line_max_column), content)]);
    }

    fn replace_content(&self, model: Rc<TextModel>, token: CancellationToken) -> Promise<()> {
        let this = self.rc();
        /* Get content to replace */
        self.output_content_provider.get_content().then_promise(move |chunk| {
            /* Abort if operation is cancelled */
            if token.is_cancellation_requested() {
                return Promise::resolved(());
            }

            /* Compute Edits */
            this.get_replace_edits(&model, &chunk.content).then(move |edits| {
                /* Abort if operation is cancelled */
                if token.is_cancellation_requested() {
                    return;
                }

                (chunk.consume)();
                if !edits.is_empty() {
                    /* Apply Edits */
                    model.apply_edits(&edits);
                }
                this.model_update_in_progress.set(false);
            })
        })
    }

    fn get_replace_edits(&self, model: &TextModel, content_to_replace: &str) -> Promise<Vec<SingleEditOperation>> {
        if content_to_replace.is_empty() {
            return Promise::resolved(vec![EditOperation::delete(model.get_full_model_range())]);
        }
        if JsString::from(content_to_replace) != model.get_value() {
            let edit = TextEdit { range: model.get_full_model_range(), text: content_to_replace.to_owned(), eol: None, insert_as_snippet: None, keep_whitespace: None };
            return self.services.editor_worker_service.compute_more_minimal_edits(&model.uri(), Some(vec![edit])).then(|edits| match edits {
                Some(edits) if !edits.is_empty() => edits.into_iter().map(|edit| EditOperation::replace(edit.range, Some(JsString::from(edit.text)))).collect(),
                _ => Vec::new(),
            });
        }
        Promise::resolved(Vec::new())
    }

    fn cancel_model_update(&self) {
        if let Some(source) = self.model_update_cancellation_source.borrow_mut().take() {
            source.cancel();
        }
        self.append_throttler.cancel();
        *self.replace_promise.borrow_mut() = None;
        self.model_update_in_progress.set(false);
    }

    pub fn is_visible(&self) -> bool {
        self.model.borrow().is_some()
    }
}

impl IOutputChannelModel for AbstractFileOutputChannelModel {
    fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    fn source(&self) -> Vec<IOutputContentSource> {
        self.behavior.source()
    }

    fn load_model(&self) -> Promise<Rc<TextModel>> {
        let this = self.rc();
        let loading = promises::with_async_body::<Rc<TextModel>>(move |resolver| {
            let store = DisposableStore::new();
            this.model_disposable.set(Some(store.clone()));
            let model = this.services.model_service.create_model(TextSource::from(""), Some(&*this.language), Some(&this.model_uri), false);
            *this.model.borrow_mut() = Some(model.clone());
            this.output_content_provider.get_content().then_result(move |result| {
                match result {
                    Ok(chunk) => {
                        (chunk.consume)();
                        this.do_append_content(&model, &chunk.content);
                        let weak = this.me.clone();
                        store.add(this.output_content_provider.on_did_reset().listen({
                            let weak = weak.clone();
                            move |_| {
                                if let Some(this) = weak.upgrade() {
                                    this.on_did_content_change(true, true);
                                }
                            }
                        }));
                        store.add(this.output_content_provider.on_did_append().listen({
                            let weak = weak.clone();
                            move |_| {
                                if let Some(this) = weak.upgrade() {
                                    this.on_did_content_change(false, false);
                                }
                            }
                        }));
                        this.output_content_provider.watch();
                        let provider = this.output_content_provider.clone();
                        store.add(je_base::lifecycle::to_disposable(move || provider.unwatch()));
                        store.add(model.on_will_dispose().listen({
                            let provider = this.output_content_provider.clone();
                            move |_| {
                                provider.reset();
                                if let Some(this) = weak.upgrade() {
                                    this.model_disposable.set(None);
                                    this.cancel_model_update();
                                    *this.model.borrow_mut() = None;
                                }
                            }
                        }));
                        resolver.resolve(model);
                    }
                    Err(error) => resolver.reject(error),
                }
                Ok(())
            })
        });
        *self.load_model_promise.borrow_mut() = Some(loading.clone());
        loading
    }

    fn get_log_entries(&self) -> Vec<ILogEntry> {
        self.output_content_provider.get_log_entries()
    }

    fn append(&self, message: &str) {
        self.behavior.append(&self.rc(), message);
    }

    fn replace(&self, message: &str) {
        self.behavior.replace(&self.rc(), message);
    }

    fn clear(&self) {
        self.behavior.clear(&self.rc());
    }

    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>, immediate: bool) {
        self.behavior.update(&self.rc(), mode, till, immediate);
    }

    fn update_channel_sources(&self, files: Vec<IOutputContentSource>) {
        self.behavior.update_channel_sources(&self.rc(), files);
    }
}

impl IDisposable for AbstractFileOutputChannelModel {
    fn dispose(&self) {
        self.on_dispose.fire(());
        self.on_dispose.dispose();
        self.model_disposable.dispose();
        if let Some(source) = self.model_update_cancellation_source.borrow_mut().take() {
            source.dispose_with(false);
        }
        self.append_throttler.dispose();
        self.output_content_provider.dispose();
    }
}

/// O comportamento de `FileOutputChannelModel`.
struct FileBehavior {
    source: IOutputContentSource,
    file_output: Rc<FileContentProvider>,
}

impl FileChannelBehavior for FileBehavior {
    fn source(&self) -> Vec<IOutputContentSource> {
        vec![self.source.clone()]
    }

    fn clear(&self, channel: &Rc<AbstractFileOutputChannelModel>) {
        channel.update(OutputChannelUpdateMode::Clear, None, true);
    }

    fn update(&self, channel: &Rc<AbstractFileOutputChannelModel>, mode: OutputChannelUpdateMode, till: Option<u64>, immediate: bool) {
        let channel = channel.clone();
        let file_output = self.file_output.clone();
        channel.pending_load().then(move |()| {
            if mode == OutputChannelUpdateMode::Clear || mode == OutputChannelUpdateMode::Replace {
                match till {
                    Some(till) => file_output.reset_at(Some(till)),
                    None => file_output.reset_to_end(),
                }
            }
            channel.do_update(mode, immediate);
        });
    }
}

/// `FileOutputChannelModel`.
pub struct FileOutputChannelModel;

impl FileOutputChannelModel {
    pub fn create(model_uri: URI, language: Rc<dyn ILanguageSelection>, source: IOutputContentSource, services: &OutputChannelModelServices) -> Rc<AbstractFileOutputChannelModel> {
        let file_output = FileContentProvider::new(&source, services);
        AbstractFileOutputChannelModel::new(model_uri, language, file_output.clone(), services, Box::new(FileBehavior { source, file_output }))
    }
}

/// O comportamento de `MultiFileOutputChannelModel`.
struct MultiFileBehavior {
    source: Vec<IOutputContentSource>,
    multifile_output: Rc<MultiFileContentProvider>,
}

impl FileChannelBehavior for MultiFileBehavior {
    fn source(&self) -> Vec<IOutputContentSource> {
        self.source.clone()
    }

    fn update_channel_sources(&self, channel: &Rc<AbstractFileOutputChannelModel>, files: Vec<IOutputContentSource>) {
        self.multifile_output.unwatch();
        self.multifile_output.update_files(&files);
        self.multifile_output.reset();
        channel.do_update(OutputChannelUpdateMode::Replace, true);
        if channel.is_visible() {
            self.multifile_output.watch();
        }
    }

    fn clear(&self, channel: &Rc<AbstractFileOutputChannelModel>) {
        let channel = channel.clone();
        let multifile_output = self.multifile_output.clone();
        channel.pending_load().then(move |()| {
            multifile_output.reset_to_end();
            channel.do_update(OutputChannelUpdateMode::Clear, true);
        });
    }
}

/// `MultiFileOutputChannelModel`.
pub struct MultiFileOutputChannelModel;

impl MultiFileOutputChannelModel {
    pub fn create(
        model_uri: URI,
        language: Rc<dyn ILanguageSelection>,
        source: Vec<IOutputContentSource>,
        services: &OutputChannelModelServices,
    ) -> Rc<AbstractFileOutputChannelModel> {
        let multifile_output = MultiFileContentProvider::new(&source, services);
        AbstractFileOutputChannelModel::new(model_uri, language, multifile_output.clone(), services, Box::new(MultiFileBehavior { source, multifile_output }))
    }
}

/// O comportamento de `OutputChannelBackedByFile`: o de `FileOutputChannelModel` com `append` e `replace` que escrevem no log.
struct BackedByFileBehavior {
    file: FileBehavior,
    logger: LoggerRef,
    offset: Cell<u64>,
}

impl BackedByFileBehavior {
    fn write(&self, channel: &AbstractFileOutputChannelModel, content: &str) {
        self.offset.set(self.offset.get() + VSBuffer::from_string(content).byte_length() as u64);
        self.logger.info(content);
        if channel.is_visible() {
            self.logger.flush();
        }
    }
}

impl FileChannelBehavior for BackedByFileBehavior {
    fn source(&self) -> Vec<IOutputContentSource> {
        self.file.source()
    }

    fn clear(&self, channel: &Rc<AbstractFileOutputChannelModel>) {
        self.file.clear(channel);
    }

    fn update(&self, channel: &Rc<AbstractFileOutputChannelModel>, mode: OutputChannelUpdateMode, till: Option<u64>, immediate: bool) {
        self.file.update(channel, mode, till, immediate);
    }

    fn append(&self, channel: &Rc<AbstractFileOutputChannelModel>, message: &str) {
        self.write(channel, message);
        channel.update(OutputChannelUpdateMode::Append, None, channel.is_visible());
    }

    fn replace(&self, channel: &Rc<AbstractFileOutputChannelModel>, message: &str) {
        let till = self.offset.get();
        self.write(channel, message);
        channel.update(OutputChannelUpdateMode::Replace, Some(till), true);
    }
}

/// `OutputChannelBackedByFile`.
struct OutputChannelBackedByFile;

impl OutputChannelBackedByFile {
    fn create(model_uri: URI, language: Rc<dyn ILanguageSelection>, file: URI, services: &OutputChannelModelServices) -> Rc<AbstractFileOutputChannelModel> {
        let source = IOutputContentSource { resource: file.clone(), name: Some(String::new()) };
        let file_output = FileContentProvider::new(&source, services);

        // Donot rotate to check for the file reset
        let logger = services.logger_service.create_logger(
            ResourceOrId::Resource(file),
            Some(LoggerOptions { log_level: Some(LoggerLogLevel::Always), donot_rotate: true, donot_use_formatters: true, hidden: Some(true), ..Default::default() }),
        );
        let behavior = BackedByFileBehavior { file: FileBehavior { source, file_output: file_output.clone() }, logger, offset: Cell::new(0) };
        AbstractFileOutputChannelModel::new(model_uri, language, file_output, services, Box::new(behavior))
    }
}

/// `id.replace(/[\\/:\*\?"<>\|]/g, '')`.
fn sanitize_channel_id(id: &str) -> String {
    id.chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect()
}

/// `DelegatedOutputChannelModel`.
pub struct DelegatedOutputChannelModel {
    on_dispose: Rc<Emitter<()>>,
    store: Rc<DisposableStore>,
    output_channel_model: Promise<Rc<dyn IOutputChannelModel>>,
    source: IOutputContentSource,
}

impl DelegatedOutputChannelModel {
    pub fn new(
        id: &str,
        model_uri: URI,
        language: Rc<dyn ILanguageSelection>,
        output_dir: URI,
        output_dir_creation_promise: Promise<()>,
        services: &OutputChannelModelServices,
    ) -> Rc<Self> {
        let on_dispose = Emitter::new();
        let store = DisposableStore::new();
        let file_name = format!("{}.log", sanitize_channel_id(id));
        let source = IOutputContentSource { name: None, resource: join_path(&output_dir, &[&file_name]) };
        let output_channel_model = Self::create_output_channel_model(model_uri, language, source.resource.clone(), output_dir_creation_promise, services, on_dispose.clone(), store.clone());
        Rc::new(Self { on_dispose, store, output_channel_model, source })
    }

    fn create_output_channel_model(
        model_uri: URI,
        language: Rc<dyn ILanguageSelection>,
        file: URI,
        output_dir_promise: Promise<()>,
        services: &OutputChannelModelServices,
        on_dispose: Rc<Emitter<()>>,
        store: Rc<DisposableStore>,
    ) -> Promise<Rc<dyn IOutputChannelModel>> {
        let services = services.clone();
        output_dir_promise.then_promise(move |()| {
            let file_service = services.file_service.clone();
            file_service.create_file(&file, None, None).then(move |_| {
                let output_channel_model = OutputChannelBackedByFile::create(model_uri, language, file, &services);
                store.add(output_channel_model.clone());
                store.add(output_channel_model.on_dispose().listen(move |_| on_dispose.fire(())));
                let output_channel_model: Rc<dyn IOutputChannelModel> = output_channel_model;
                output_channel_model
            })
        })
    }

    /// `this.outputChannelModel.then(outputChannelModel => ...)`.
    fn with_model(&self, action: impl FnOnce(&Rc<dyn IOutputChannelModel>) + 'static) {
        self.output_channel_model.then(move |output_channel_model| action(&output_channel_model));
    }
}

impl IOutputChannelModel for DelegatedOutputChannelModel {
    fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    fn source(&self) -> Vec<IOutputContentSource> {
        vec![self.source.clone()]
    }

    fn get_log_entries(&self) -> Vec<ILogEntry> {
        Vec::new()
    }

    fn append(&self, output: &str) {
        let output = output.to_owned();
        self.with_model(move |model| model.append(&output));
    }

    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>, immediate: bool) {
        self.with_model(move |model| model.update(mode, till, immediate));
    }

    fn update_channel_sources(&self, files: Vec<IOutputContentSource>) {
        self.with_model(move |model| model.update_channel_sources(files));
    }

    fn load_model(&self) -> Promise<Rc<TextModel>> {
        self.output_channel_model.then_promise(|output_channel_model| output_channel_model.load_model())
    }

    fn clear(&self) {
        self.with_model(|model| model.clear());
    }

    fn replace(&self, value: &str) {
        let value = value.to_owned();
        self.with_model(move |model| model.replace(&value));
    }
}

impl IDisposable for DelegatedOutputChannelModel {
    fn dispose(&self) {
        self.store.dispose();
        self.on_dispose.dispose();
    }
}
