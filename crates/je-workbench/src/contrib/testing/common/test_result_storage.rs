//! Porte de `src/vs/workbench/contrib/testing/common/testResultStorage.ts`: a persistência dos resultados de testes por
//! workspace, com limite de quantidade e de bytes, em memória (testes) ou em arquivos.
//!
//! Divergências:
//! - **Classes abstratas.** `BaseTestResultStorage` é a struct [`BaseTestResultStorage`], que recebe os métodos abstratos
//!   (`readForResultId`, `readOutputForResultId`, `readOutputRangeForResultId`, `deleteForResultId`, `storeForResultId`,
//!   `storeOutputForResultId`) como um [`TestResultStorageBackend`]. [`InMemoryResultStorage`] e [`TestResultStorage`] são
//!   as duas implementações; cada uma é um `ITestResultStorage` que repete a base (o `override persist` do
//!   `TestResultStorage` roda a limpeza depois).
//! - **`JSON.stringify` e `JSON.parse`** de `ISerializedTestResults` são `to_json_string` e `from_json_str`
//!   (`serialized_json.rs`); o `bytes` guardado é o `byteLength` UTF-8 do texto.
//! - **`StoredValue<ReadonlyArray<{ rev, id, bytes }>>`** é `StoredValue<Vec<StoredResultRecord>>`, com a forma JSON do
//!   [`StoredJson`].
//! - **`IFileService`** é o `FileService` concreto: o contrato reduzido `IFileService` do porte ainda não tem `writeFile`,
//!   `createFile`, `del` nem `readFileStream`.
//! - **`IEnvironmentService`** é o trait [`EnvironmentService`] (`workspaceStorageHome`) e **`IWorkspaceContextService`**
//!   fornece `getWorkspace().id`.
//! - **`Math.random() < CLEANUP_PROBABILITY`** é `random_f64`.
//! - **`catch(() => undefined)`** é `.catch(|_| ())`. **`throw new Error('Method not implemented.')`** do
//!   `InMemoryResultStorage` lança na chamada (as funções não são `async`).
//! - **`async` com `try/catch`** (`read`, `readOutputRangeForResultId`, `readOutputForResultId`) são cadeias de promessas
//!   que tratam o erro na mesma reação.
//! - **`createDecorator('ITestResultStorage')`** (o original não tipa o decorador) é [`I_TEST_RESULT_STORAGE`].
//! - **`HydratedTestResult`:** `Promise<HydratedTestResult[]>` é `Promise<Vec<Rc<HydratedTestResult>>>`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream, new_writeable_buffer_stream};
use je_base::errors::{self, Error, throw};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::random::random_f64;
use je_base::stream::WriteableStreamImpl;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::{CreateFileOptions, FileReadStreamOptions, PartialFileDeleteOptions, ReadFileOptions};
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::resources::join_path;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use serde_json::Value;

use super::stored_value::{StoredJson, StoredValue, StoredValueOptions};
use super::test_result::{HydratedTestResult, ITestResult};
use super::test_types::ISerializedTestResults;

pub const RETAIN_MAX_RESULTS: usize = 128;
const RETAIN_MIN_RESULTS: usize = 16;
const RETAIN_MAX_BYTES: i64 = 1024 * 128;
const CLEANUP_PROBABILITY: f64 = 0.2;

pub trait ITestResultStorage {
    /// Retrieves the list of stored test results.
    fn read(&self) -> Promise<Vec<Rc<HydratedTestResult>>>;

    /// Persists the list of test results.
    fn persist(&self, results: &[Rc<dyn ITestResult>]) -> Promise<()>;
}

service_id!(pub I_TEST_RESULT_STORAGE: dyn ITestResultStorage = "ITestResultStorage");

/// Data revision this version of VS Code deals with. Should be bumped whenever
/// a breaking change is made to the stored results, which will cause previous
/// revisions to be discarded.
const CURRENT_REVISION: i64 = 1;

/// O `{ rev: number; id: string; bytes: number }` guardado em `storedTestResults`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredResultRecord {
    pub rev: i64,
    pub id: String,
    pub bytes: usize,
}

impl StoredJson for Vec<StoredResultRecord> {
    fn to_json(&self) -> Value {
        Value::Array(
            self.iter()
                .map(|rec| {
                    let mut object = serde_json::Map::new();
                    object.insert("rev".into(), Value::from(rec.rev));
                    object.insert("id".into(), Value::String(rec.id.clone()));
                    object.insert("bytes".into(), Value::from(rec.bytes as u64));
                    Value::Object(object)
                })
                .collect(),
        )
    }

    fn from_json(value: &Value) -> Option<Self> {
        value
            .as_array()?
            .iter()
            .map(|rec| {
                let object = rec.as_object()?;
                Some(StoredResultRecord { rev: object.get("rev")?.as_i64()?, id: object.get("id")?.as_str()?.to_string(), bytes: object.get("bytes")?.as_u64()? as usize })
            })
            .collect()
    }
}

/// Os métodos abstratos do `BaseTestResultStorage`.
pub trait TestResultStorageBackend {
    /// Reads serialized results for the test. Is allowed to throw.
    fn read_for_result_id(&self, id: &str) -> Promise<Option<Rc<ISerializedTestResults>>>;

    /// Reads output as a stream for the test.
    fn read_output_for_result_id(&self, id: &str) -> Promise<VSBufferReadableStream>;

    /// Reads an output range for the test.
    fn read_output_range_for_result_id(&self, id: &str, offset: u64, length: u64) -> Promise<VSBuffer>;

    /// Deletes serialized results for the test.
    fn delete_for_result_id(&self, id: &str) -> Promise<()>;

    /// Stores test results by ID.
    fn store_for_result_id(&self, id: &str, data: Rc<ISerializedTestResults>) -> Promise<()>;

    /// Reads serialized results for the test. Is allowed to throw.
    fn store_output_for_result_id(&self, id: &str, input: Rc<WriteableStreamImpl<VSBuffer>>) -> Promise<()>;
}

pub struct BaseTestResultStorage {
    base: Rc<Disposable>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: LogServiceRef,
    pub stored: Rc<StoredValue<Vec<StoredResultRecord>>>,
    backend: Rc<dyn TestResultStorageBackend>,
}

impl BaseTestResultStorage {
    pub fn new(
        uri_identity_service: Rc<dyn IUriIdentityService>,
        storage_service: Rc<dyn StorageService>,
        log_service: LogServiceRef,
        backend: Rc<dyn TestResultStorageBackend>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let stored = StoredValue::<Vec<StoredResultRecord>>::new(
            StoredValueOptions { key: "storedTestResults".to_string(), scope: StorageScope::Workspace, target: StorageTarget::Machine },
            storage_service,
        );
        base.register(stored.clone());
        Rc::new(Self { base, uri_identity_service, log_service, stored, backend })
    }

    pub fn read(&self) -> Promise<Vec<Rc<HydratedTestResult>>> {
        type Loaded = Option<(StoredResultRecord, Rc<HydratedTestResult>)>;

        let records = self.stored.get_or(Vec::new());
        let total = records.len();
        let promises: Vec<Promise<Loaded>> = records
            .into_iter()
            .map(|rec| {
                if rec.rev != CURRENT_REVISION {
                    return Promise::resolved(None);
                }

                let identity = self.uri_identity_service.clone();
                let log = self.log_service.clone();
                let id = rec.id.clone();
                self.backend.read_for_result_id(&id).then_result(move |result| {
                    let warn = |error: &Error| {
                        log.warn_args(&LogMessage::from(format!("Error deserializing stored test result {id}")), &[LogArg::from(error)]);
                    };
                    match result {
                        Ok(None) => Ok(None),
                        Ok(Some(contents)) => match errors::catch(|| HydratedTestResult::new(identity.as_ref(), contents)) {
                            Ok(hydrated) => Ok(Some((rec, hydrated))),
                            Err(error) => {
                                warn(&error);
                                Ok(None)
                            }
                        },
                        Err(error) => {
                            warn(&error);
                            Ok(None)
                        }
                    }
                })
            })
            .collect();

        let stored = self.stored.clone();
        Promise::all(promises).then(move |results| {
            let defined: Vec<(StoredResultRecord, Rc<HydratedTestResult>)> = results.into_iter().flatten().collect();
            if defined.len() != total {
                stored.store(defined.iter().map(|(rec, _)| rec.clone()).collect());
            }

            defined.into_iter().map(|(_, result)| result).collect()
        })
    }

    pub fn get_result_output_writer(&self, result_id: &str) -> Rc<WriteableStreamImpl<VSBuffer>> {
        let stream = new_writeable_buffer_stream(None);
        let _ = self.backend.store_output_for_result_id(result_id, stream.clone());
        stream
    }

    pub fn persist(&self, results: &[Rc<dyn ITestResult>]) -> Promise<()> {
        let mut to_delete: IndexMap<String, usize> = self.stored.get_or(Vec::new()).into_iter().map(|rec| (rec.id, rec.bytes)).collect();
        let mut to_store: Vec<StoredResultRecord> = Vec::new();
        let mut todo: Vec<Promise<()>> = Vec::new();
        let mut budget: i64 = RETAIN_MAX_BYTES;

        // Run until either:
        // 1. We store all results
        // 2. We store the max results
        // 3. We store the min results, and have no more byte budget
        let mut i = 0;
        while i < results.len() && i < RETAIN_MAX_RESULTS && (budget > 0 || to_store.len() < RETAIN_MIN_RESULTS) {
            let result = &results[i];
            i += 1;

            let existing_bytes = to_delete.shift_remove(&result.id());
            if let Some(existing_bytes) = existing_bytes {
                to_store.push(StoredResultRecord { id: result.id(), rev: CURRENT_REVISION, bytes: existing_bytes });
                budget -= existing_bytes as i64;
                continue;
            }

            let Some(obj) = result.to_json() else {
                continue;
            };

            let contents = VSBuffer::from_string(&obj.to_json_string());
            todo.push(self.backend.store_for_result_id(&result.id(), obj));
            to_store.push(StoredResultRecord { id: result.id(), rev: CURRENT_REVISION, bytes: contents.byte_length() });
            budget -= contents.byte_length() as i64;
        }

        for id in to_delete.keys() {
            todo.push(self.backend.delete_for_result_id(id).catch(|_| ()));
        }

        self.stored.store(to_store);
        Promise::all(todo).then(|_| ())
    }
}

impl IDisposable for BaseTestResultStorage {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O armazenamento em memória do `InMemoryResultStorage`.
struct InMemoryBackend {
    cache: Rc<RefCell<HashMap<String, Rc<ISerializedTestResults>>>>,
}

impl TestResultStorageBackend for InMemoryBackend {
    fn read_for_result_id(&self, id: &str) -> Promise<Option<Rc<ISerializedTestResults>>> {
        Promise::resolved(self.cache.borrow().get(id).cloned())
    }

    fn store_for_result_id(&self, id: &str, contents: Rc<ISerializedTestResults>) -> Promise<()> {
        self.cache.borrow_mut().insert(id.to_string(), contents);
        Promise::resolved(())
    }

    fn delete_for_result_id(&self, id: &str) -> Promise<()> {
        self.cache.borrow_mut().remove(id);
        Promise::resolved(())
    }

    fn read_output_for_result_id(&self, _id: &str) -> Promise<VSBufferReadableStream> {
        throw(Error::new("Method not implemented."))
    }

    fn store_output_for_result_id(&self, _id: &str, _input: Rc<WriteableStreamImpl<VSBuffer>>) -> Promise<()> {
        throw(Error::new("Method not implemented."))
    }

    fn read_output_range_for_result_id(&self, _id: &str, _offset: u64, _length: u64) -> Promise<VSBuffer> {
        throw(Error::new("Method not implemented."))
    }
}

pub struct InMemoryResultStorage {
    base: Rc<BaseTestResultStorage>,
    pub cache: Rc<RefCell<HashMap<String, Rc<ISerializedTestResults>>>>,
}

impl InMemoryResultStorage {
    pub fn new(uri_identity_service: Rc<dyn IUriIdentityService>, storage_service: Rc<dyn StorageService>, log_service: LogServiceRef) -> Rc<Self> {
        let cache = Rc::new(RefCell::new(HashMap::new()));
        let backend = Rc::new(InMemoryBackend { cache: cache.clone() });
        Rc::new(Self { base: BaseTestResultStorage::new(uri_identity_service, storage_service, log_service, backend), cache })
    }

    pub fn get_result_output_writer(&self, result_id: &str) -> Rc<WriteableStreamImpl<VSBuffer>> {
        self.base.get_result_output_writer(result_id)
    }
}

impl ITestResultStorage for InMemoryResultStorage {
    fn read(&self) -> Promise<Vec<Rc<HydratedTestResult>>> {
        self.base.read()
    }

    fn persist(&self, results: &[Rc<dyn ITestResult>]) -> Promise<()> {
        self.base.persist(results)
    }
}

impl IDisposable for InMemoryResultStorage {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O armazenamento em arquivos do `TestResultStorage`.
struct FileBackend {
    directory: URI,
    file_service: Rc<FileService>,
}

impl FileBackend {
    fn get_result_json_path(&self, id: &str) -> URI {
        join_path(&self.directory, &[&format!("{id}.json")])
    }

    fn get_result_output_path(&self, id: &str) -> URI {
        join_path(&self.directory, &[&format!("{id}.output")])
    }
}

impl TestResultStorageBackend for FileBackend {
    fn read_for_result_id(&self, id: &str) -> Promise<Option<Rc<ISerializedTestResults>>> {
        self.file_service.read_file(&self.get_result_json_path(id), None, None).then_result(|result| {
            let contents = result?;
            errors::catch(|| Some(Rc::new(ISerializedTestResults::from_json_str(&contents.value.to_string()))))
        })
    }

    fn store_for_result_id(&self, id: &str, contents: Rc<ISerializedTestResults>) -> Promise<()> {
        self.file_service.write_file(&self.get_result_json_path(id), FileContentSource::from(VSBuffer::from_string(&contents.to_json_string())), None).then(|_| ())
    }

    fn delete_for_result_id(&self, id: &str) -> Promise<()> {
        self.file_service.del(&self.get_result_json_path(id), None::<PartialFileDeleteOptions>).catch(|_| ())
    }

    fn read_output_range_for_result_id(&self, id: &str, offset: u64, length: u64) -> Promise<VSBuffer> {
        let options = ReadFileOptions { stream: FileReadStreamOptions { position: Some(offset), length: Some(length), limit_size: None }, ..ReadFileOptions::default() };
        self.file_service.read_file(&self.get_result_output_path(id), Some(options), None).then_result(|result| match result {
            Ok(content) => Ok(content.value.clone()),
            Err(_) => Ok(VSBuffer::alloc(0)),
        })
    }

    fn read_output_for_result_id(&self, id: &str) -> Promise<VSBufferReadableStream> {
        self.file_service.read_file_stream(&self.get_result_output_path(id), None, None).then_result(|result| match result {
            Ok(content) => Ok(content.value.clone()),
            Err(_) => Ok(buffer_to_stream(VSBuffer::alloc(0)) as VSBufferReadableStream),
        })
    }

    fn store_output_for_result_id(&self, id: &str, input: Rc<WriteableStreamImpl<VSBuffer>>) -> Promise<()> {
        let stream: VSBufferReadableStream = input;
        self.file_service.create_file(&self.get_result_output_path(id), Some(FileContentSource::from(stream)), None::<CreateFileOptions>).then(|_| ())
    }
}

pub struct TestResultStorage {
    base: Rc<BaseTestResultStorage>,
    backend: Rc<FileBackend>,
    this: std::rc::Weak<TestResultStorage>,
}

impl TestResultStorage {
    pub fn new(
        uri_identity_service: Rc<dyn IUriIdentityService>,
        storage_service: Rc<dyn StorageService>,
        log_service: LogServiceRef,
        workspace_context: Rc<dyn IWorkspaceContextService>,
        file_service: Rc<FileService>,
        environment_service: Rc<dyn EnvironmentService>,
    ) -> Rc<Self> {
        let directory = join_path(&environment_service.workspace_storage_home(), &[&workspace_context.get_workspace().id(), "testResults"]);
        let backend = Rc::new(FileBackend { directory, file_service });
        let base = BaseTestResultStorage::new(uri_identity_service, storage_service, log_service, backend.clone());
        Rc::new_cyclic(|me| Self { base, backend, this: me.clone() })
    }

    pub fn get_result_output_writer(&self, result_id: &str) -> Rc<WriteableStreamImpl<VSBuffer>> {
        self.base.get_result_output_writer(result_id)
    }

    /// Cleans up orphaned files. For instance, output can get orphaned if it's
    /// written but the editor is closed before the test run is complete.
    fn cleanup_dereferenced(&self) -> Promise<()> {
        let backend = self.backend.clone();
        let stored = self.base.stored.clone();
        self.backend.file_service.resolve(&self.backend.directory, None).then_promise(move |stat| {
            let Some(children) = stat.children else {
                return Promise::resolved(());
            };

            let stored: std::collections::HashSet<String> = stored.get_or(Vec::new()).into_iter().filter(|s| s.rev == CURRENT_REVISION).map(|s| s.id).collect();

            let deletions: Vec<Promise<()>> = children
                .into_iter()
                .filter(|child| !stored.contains(strip_extension(&child.name)))
                .map(|child| backend.file_service.del(&child.resource, None::<PartialFileDeleteOptions>).catch(|_| ()))
                .collect();
            Promise::all(deletions).then(|_| ())
        })
    }
}

/// `name.replace(/\.[a-z]+$/, '')`.
fn strip_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(index) if index + 1 < name.len() && name[index + 1..].bytes().all(|b| b.is_ascii_lowercase()) => &name[..index],
        _ => name,
    }
}

impl ITestResultStorage for TestResultStorage {
    fn read(&self) -> Promise<Vec<Rc<HydratedTestResult>>> {
        self.base.read()
    }

    /// @inheritdoc
    fn persist(&self, results: &[Rc<dyn ITestResult>]) -> Promise<()> {
        let this = self.this.upgrade().expect("o armazenamento está vivo enquanto é usado");
        self.base.persist(results).then_promise(move |_| if random_f64() < CLEANUP_PROBABILITY { this.cleanup_dereferenced() } else { Promise::resolved(()) })
    }
}

impl IDisposable for TestResultStorage {
    fn dispose(&self) {
        self.base.dispose();
    }
}
