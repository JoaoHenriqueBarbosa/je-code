//! Porte de `src/vs/platform/files/common/files.ts`: o contrato do provider de arquivos (com as
//! capacidades), os tipos de stat, de evento e de erro, e as funções puras do módulo.
//!
//! Onde o TypeScript tem uma interface por capacidade (`IFileSystemProviderWithFileReadWriteCapability`
//! e as outras), o Rust tem um único trait [`FileSystemProvider`] com as operações opcionais devolvendo
//! o erro "não suportado" por padrão; quem decide o que vale é a máscara de [`FileSystemProviderCapabilities`],
//! como no original (as funções `has*Capability` só olham a máscara).
//!
//! Números do JavaScript: `mtime` e `ctime` são milissegundos (`i64`), `size` e posições são `u64`/`usize`.

use std::cell::OnceCell;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::glob::PatternItem;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::strings;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::{URI, UriChange};

use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::stream::WriteableStream;

/// Define um tipo de máscara de bits com os operadores que o original usa sobre `enum` numérico.
macro_rules! flags {
    ($(#[$meta:meta])* $name:ident { $($(#[$cmeta:meta])* $constant:ident = $value:expr),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);

        impl $name {
            $($(#[$cmeta])* pub const $constant: $name = $name($value);)*

            pub fn bits(self) -> u32 {
                self.0
            }

            /// `(self & other) !== 0`.
            pub fn intersects(self, other: $name) -> bool {
                self.0 & other.0 != 0
            }
        }

        impl BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }

        impl BitAnd for $name {
            type Output = $name;
            fn bitand(self, rhs: $name) -> $name {
                $name(self.0 & rhs.0)
            }
        }

        impl Not for $name {
            type Output = $name;
            fn not(self) -> $name {
                $name(!self.0)
            }
        }
    };
}

flags! {
    /// `FileType`.
    FileType {
        UNKNOWN = 0,
        FILE = 1,
        DIRECTORY = 2,
        SYMBOLIC_LINK = 64,
    }
}

flags! {
    /// `FilePermission`.
    FilePermission {
        NONE = 0,
        READONLY = 1,
        LOCKED = 2,
        EXECUTABLE = 4,
    }
}

flags! {
    /// `FileChangeFilter`.
    FileChangeFilter {
        UPDATED = 1 << 1,
        ADDED = 1 << 2,
        DELETED = 1 << 3,
    }
}

flags! {
    /// `FileSystemProviderCapabilities`.
    FileSystemProviderCapabilities {
        NONE = 0,
        FILE_READ_WRITE = 1 << 1,
        FILE_OPEN_READ_WRITE_CLOSE = 1 << 2,
        FILE_READ_STREAM = 1 << 4,
        FILE_FOLDER_COPY = 1 << 3,
        PATH_CASE_SENSITIVE = 1 << 10,
        READONLY = 1 << 11,
        TRASH = 1 << 12,
        FILE_WRITE_UNLOCK = 1 << 13,
        FILE_ATOMIC_READ = 1 << 14,
        FILE_ATOMIC_WRITE = 1 << 15,
        FILE_ATOMIC_DELETE = 1 << 16,
        FILE_CLONE = 1 << 17,
        FILE_REALPATH = 1 << 18,
        FILE_APPEND = 1 << 19,
    }
}

/// `FileChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileChangeType {
    Updated = 0,
    Added = 1,
    Deleted = 2,
}

/// `FileOperation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Create,
    Delete,
    Move,
    Copy,
    Write,
}

/// `FileOperationResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOperationResult {
    FileIsDirectory = 0,
    FileNotFound,
    FileNotModifiedSince,
    FileModifiedSince,
    FileMoveConflict,
    FileWriteLocked,
    FilePermissionDenied,
    FileTooLarge,
    FileInvalidPath,
    FileNotDirectory,
    FileOtherError,
}

impl FileOperationResult {
    fn from_index(index: usize) -> Option<Self> {
        use FileOperationResult::*;
        [FileIsDirectory, FileNotFound, FileNotModifiedSince, FileModifiedSince, FileMoveConflict, FileWriteLocked, FilePermissionDenied, FileTooLarge, FileInvalidPath, FileNotDirectory, FileOtherError]
            .get(index)
            .copied()
    }
}

/// `FileKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Folder,
    RootFolder,
}

/// `FILES_ASSOCIATIONS_CONFIG`: a chave de configuração das associações de arquivo a idioma.
pub const FILES_ASSOCIATIONS_CONFIG: &str = "files.associations";

// region: opções

/// `IFileAtomicOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileAtomicOptions {
    pub postfix: String,
}

/// `IFileReadLimits`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileReadLimits {
    /// If the file exceeds the given size, an error of kind `FILE_TOO_LARGE` will be thrown.
    pub size: Option<u64>,
}

/// `IFileReadStreamOptions` (com `IFileReadLimits`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileReadStreamOptions {
    pub position: Option<u64>,
    pub length: Option<u64>,
    pub limit_size: Option<u64>,
}

/// `IFileWriteOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileWriteOptions {
    pub create: bool,
    pub overwrite: bool,
    pub unlock: bool,
    pub atomic: Option<FileAtomicOptions>,
    pub append: bool,
}

/// `IFileOpenOptions`: `create: false` é leitura; `create: true` é escrita.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileOpenOptions {
    pub create: bool,
    pub unlock: bool,
    pub append: bool,
}

impl FileOpenOptions {
    pub fn for_read() -> Self {
        Self::default()
    }

    pub fn for_write(unlock: bool, append: bool) -> Self {
        Self { create: true, unlock, append }
    }

    /// `isFileOpenForWriteOptions`.
    pub fn is_for_write(&self) -> bool {
        self.create
    }
}

/// `IFileDeleteOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDeleteOptions {
    pub recursive: bool,
    pub use_trash: bool,
    pub atomic: Option<FileAtomicOptions>,
}

/// `Partial<IFileDeleteOptions>`: o que quem chama o serviço passa.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartialFileDeleteOptions {
    pub recursive: Option<bool>,
    pub use_trash: Option<bool>,
    pub atomic: Option<FileAtomicOptions>,
}

/// `IFileOverwriteOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileOverwriteOptions {
    pub overwrite: bool,
}

/// `IWatchOptionsWithoutCorrelation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchOptions {
    pub recursive: bool,
    pub excludes: Vec<String>,
    pub includes: Option<Vec<PatternItem>>,
    pub filter: Option<FileChangeFilter>,
    /// `IWatchOptions.correlationId`.
    pub correlation_id: Option<i64>,
}

/// `IReadFileOptions` e `IReadFileStreamOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadFileOptions {
    pub stream: FileReadStreamOptions,
    pub etag: Option<String>,
    pub atomic: bool,
}

/// `IWriteFileOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteFileOptions {
    pub mtime: Option<i64>,
    pub etag: Option<String>,
    pub unlock: bool,
    pub atomic: Option<FileAtomicOptions>,
    pub append: bool,
}

/// `IResolveFileOptions`.
#[derive(Clone, Debug, Default)]
pub struct ResolveFileOptions {
    pub resolve_to: Vec<URI>,
    pub resolve_single_child_descendants: bool,
    pub resolve_metadata: bool,
}

/// `ICreateFileOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CreateFileOptions {
    pub overwrite: bool,
}

// endregion

// region: stat

/// `IStat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub file_type: FileType,
    pub mtime: i64,
    pub ctime: i64,
    pub size: u64,
    pub permissions: Option<FilePermission>,
}

/// `IFileStat`. Os campos de metadados são `Option` porque o `resolve` sem `resolveMetadata` não os
/// preenche; [`FileStatWithMetadata`] é o mesmo valor com a garantia de que estão todos.
#[derive(Clone, Debug, PartialEq)]
pub struct FileStat {
    pub resource: URI,
    pub name: String,
    pub size: Option<u64>,
    pub mtime: Option<i64>,
    pub ctime: Option<i64>,
    pub etag: Option<String>,
    pub readonly: Option<bool>,
    pub locked: Option<bool>,
    pub executable: Option<bool>,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub children: Option<Vec<FileStat>>,
}

/// `IFileStatWithMetadata` (e `IFileStatWithPartialMetadata`, que só difere por não ter filhos).
#[derive(Clone, Debug, PartialEq)]
pub struct FileStatWithMetadata(FileStat);

impl FileStatWithMetadata {
    /// Afirma que o stat tem os metadados todos (o `resolve` com `resolveMetadata: true`).
    pub fn new(stat: FileStat) -> Self {
        assert!(
            stat.size.is_some() && stat.mtime.is_some() && stat.ctime.is_some() && stat.etag.is_some() && stat.readonly.is_some() && stat.locked.is_some() && stat.executable.is_some(),
            "stat sem metadados"
        );
        Self(stat)
    }

    pub fn size(&self) -> u64 {
        self.0.size.unwrap_or_default()
    }

    pub fn mtime(&self) -> i64 {
        self.0.mtime.unwrap_or_default()
    }

    pub fn ctime(&self) -> i64 {
        self.0.ctime.unwrap_or_default()
    }

    pub fn etag(&self) -> &str {
        self.0.etag.as_deref().unwrap_or_default()
    }

    pub fn readonly(&self) -> bool {
        self.0.readonly.unwrap_or_default()
    }

    pub fn locked(&self) -> bool {
        self.0.locked.unwrap_or_default()
    }

    pub fn executable(&self) -> bool {
        self.0.executable.unwrap_or_default()
    }

    pub fn into_stat(self) -> FileStat {
        self.0
    }
}

impl std::ops::Deref for FileStatWithMetadata {
    type Target = FileStat;

    fn deref(&self) -> &FileStat {
        &self.0
    }
}

/// `IFileStatResult` (e `IFileStatResultWithMetadata`).
#[derive(Clone, Debug, PartialEq)]
pub struct FileStatResult {
    pub stat: Option<FileStat>,
    pub success: bool,
}

/// `IFileContent`: os metadados do arquivo e o conteúdo.
#[derive(Clone, Debug)]
pub struct FileContent {
    pub stat: FileStatWithMetadata,
    pub value: VSBuffer,
}

impl std::ops::Deref for FileContent {
    type Target = FileStatWithMetadata;

    fn deref(&self) -> &FileStatWithMetadata {
        &self.stat
    }
}

/// `IFileStreamContent`.
#[derive(Clone)]
pub struct FileStreamContent {
    pub stat: FileStatWithMetadata,
    pub value: VSBufferReadableStream,
}

impl std::ops::Deref for FileStreamContent {
    type Target = FileStatWithMetadata;

    fn deref(&self) -> &FileStatWithMetadata {
        &self.stat
    }
}

/// `ETAG_DISABLED`.
pub const ETAG_DISABLED: &str = "";

/// O `Number.prototype.toString(radix)` para inteiros.
pub fn to_radix(value: i64, radix: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let negative = value < 0;
    let mut n = value.unsigned_abs();
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from_digit((n % u64::from(radix)) as u32, radix).unwrap_or('0'));
        n /= u64::from(radix);
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// `etag(stat)`: `mtime` em base 29 e `size` em base 31, o que só muda se um dos dois mudar.
pub fn etag(mtime: i64, size: u64) -> String {
    format!("{}{}", to_radix(mtime, 29), to_radix(i64::try_from(size).unwrap_or(i64::MAX), 31))
}

// endregion

// region: erros

/// `FileSystemProviderErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileSystemProviderErrorCode {
    FileExists,
    FileNotFound,
    FileNotADirectory,
    FileIsADirectory,
    FileExceedsStorageQuota,
    FileTooLarge,
    FileWriteLocked,
    NoPermissions,
    Unavailable,
    Unknown,
}

impl FileSystemProviderErrorCode {
    /// O valor de string do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileExists => "EntryExists",
            Self::FileNotFound => "EntryNotFound",
            Self::FileNotADirectory => "EntryNotADirectory",
            Self::FileIsADirectory => "EntryIsADirectory",
            Self::FileExceedsStorageQuota => "EntryExceedsStorageQuota",
            Self::FileTooLarge => "EntryTooLarge",
            Self::FileWriteLocked => "EntryWriteLocked",
            Self::NoPermissions => "NoPermissions",
            Self::Unavailable => "Unavailable",
            Self::Unknown => "Unknown",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::FileExists, Self::FileNotFound, Self::FileNotADirectory, Self::FileIsADirectory, Self::FileExceedsStorageQuota, Self::FileTooLarge, Self::FileWriteLocked, Self::NoPermissions, Self::Unavailable, Self::Unknown]
            .into_iter()
            .find(|code| code.as_str() == text)
    }
}

/// `createFileSystemProviderError(error, code)`: a mensagem é o `toString()` do erro de origem e o nome
/// é `<código> (FileSystemError)`. A propriedade `code` marca o que no original é `instanceof
/// FileSystemProviderError`.
pub fn create_file_system_provider_error(error: &Error, code: FileSystemProviderErrorCode) -> Error {
    create_file_system_provider_error_from_text(&error.to_string(), code)
}

/// O mesmo, a partir de um texto (o `error: Error | string` do original quando é string).
pub fn create_file_system_provider_error_from_text(message: &str, code: FileSystemProviderErrorCode) -> Error {
    Error::named(format!("{} (FileSystemError)", code.as_str()), message).with_prop("code", code.as_str())
}

/// `toFileSystemProviderErrorCode(error)`.
pub fn to_file_system_provider_error_code(error: Option<&Error>) -> FileSystemProviderErrorCode {
    let Some(error) = error else {
        return FileSystemProviderErrorCode::Unknown;
    };
    if let Some(code) = error.prop("code").and_then(FileSystemProviderErrorCode::parse) {
        return code;
    }
    let Some(code) = error.name.strip_suffix(" (FileSystemError)") else {
        return FileSystemProviderErrorCode::Unknown;
    };
    match FileSystemProviderErrorCode::parse(code) {
        Some(
            code @ (FileSystemProviderErrorCode::FileExists
            | FileSystemProviderErrorCode::FileIsADirectory
            | FileSystemProviderErrorCode::FileNotADirectory
            | FileSystemProviderErrorCode::FileNotFound
            | FileSystemProviderErrorCode::FileTooLarge
            | FileSystemProviderErrorCode::FileWriteLocked
            | FileSystemProviderErrorCode::NoPermissions
            | FileSystemProviderErrorCode::Unavailable),
        ) => code,
        _ => FileSystemProviderErrorCode::Unknown,
    }
}

/// `ensureFileSystemProviderError(error)`: o erro como veio, ou o "Unknown Error" do original.
pub fn ensure_file_system_provider_error(error: Option<Error>) -> Error {
    error.unwrap_or_else(|| {
        create_file_system_provider_error_from_text(&localize("unknownError", "Unknown Error", &[]), FileSystemProviderErrorCode::Unknown)
    })
}

/// `FileOperationError` e as duas subclasses (`TooLargeFileOperationError` e
/// `NotModifiedSinceFileOperationError`). Como um [`Error`] não carrega dados tipados, o resultado
/// e os extras viajam em propriedades; [`FileOperationError::from_error`] lê de volta.
#[derive(Clone, Debug)]
pub struct FileOperationError {
    pub message: String,
    pub file_operation_result: FileOperationResult,
    /// `TooLargeFileOperationError.size`.
    pub size: Option<u64>,
    /// `NotModifiedSinceFileOperationError.stat`.
    pub stat: Option<FileStatWithMetadata>,
}

impl FileOperationError {
    pub fn new(message: impl Into<String>, file_operation_result: FileOperationResult) -> Self {
        Self { message: message.into(), file_operation_result, size: None, stat: None }
    }

    pub fn too_large(message: impl Into<String>, size: u64) -> Self {
        Self { size: Some(size), ..Self::new(message, FileOperationResult::FileTooLarge) }
    }

    pub fn not_modified_since(message: impl Into<String>, stat: FileStatWithMetadata) -> Self {
        Self { stat: Some(stat), ..Self::new(message, FileOperationResult::FileNotModifiedSince) }
    }

    pub fn into_error(self) -> Error {
        let mut error = Error::new(self.message).with_prop("fileOperationResult", self.file_operation_result as usize);
        if let Some(size) = self.size {
            error = error.with_prop("size", size);
        }
        if let Some(stat) = self.stat {
            error = error.with_prop("stat.resource", stat.resource.to_string(false).to_string_lossy());
            for (name, value) in stat_props(&stat) {
                error = error.with_prop(format!("stat.{name}"), value);
            }
        }
        error
    }

    pub fn from_error(error: &Error) -> Option<Self> {
        let result = error.prop("fileOperationResult")?.parse::<usize>().ok().and_then(FileOperationResult::from_index)?;
        let stat = error.prop("stat.resource").map(|resource| stat_from_props(error, resource));
        Some(Self { message: error.message.clone(), file_operation_result: result, size: error.prop("size").and_then(|s| s.parse().ok()), stat })
    }
}

fn stat_props(stat: &FileStatWithMetadata) -> Vec<(&'static str, String)> {
    vec![
        ("name", stat.name.clone()),
        ("size", stat.size().to_string()),
        ("mtime", stat.mtime().to_string()),
        ("ctime", stat.ctime().to_string()),
        ("etag", stat.etag().to_string()),
        ("readonly", stat.readonly().to_string()),
        ("locked", stat.locked().to_string()),
        ("executable", stat.executable().to_string()),
        ("isFile", stat.is_file.to_string()),
        ("isDirectory", stat.is_directory.to_string()),
        ("isSymbolicLink", stat.is_symbolic_link.to_string()),
    ]
}

fn stat_from_props(error: &Error, resource: &str) -> FileStatWithMetadata {
    let get = |name: &str| error.prop(&format!("stat.{name}")).unwrap_or_default().to_string();
    let flag = |name: &str| get(name) == "true";
    FileStatWithMetadata::new(FileStat {
        resource: URI::parse(resource, false),
        name: get("name"),
        size: get("size").parse().ok(),
        mtime: get("mtime").parse().ok(),
        ctime: get("ctime").parse().ok(),
        etag: Some(get("etag")),
        readonly: Some(flag("readonly")),
        locked: Some(flag("locked")),
        executable: Some(flag("executable")),
        is_file: flag("isFile"),
        is_directory: flag("isDirectory"),
        is_symbolic_link: flag("isSymbolicLink"),
        children: None,
    })
}

impl From<FileOperationError> for Error {
    fn from(error: FileOperationError) -> Error {
        error.into_error()
    }
}

/// `toFileOperationResult(error)`.
pub fn to_file_operation_result(error: &Error) -> FileOperationResult {
    if let Some(error) = FileOperationError::from_error(error) {
        return error.file_operation_result;
    }
    match to_file_system_provider_error_code(Some(error)) {
        FileSystemProviderErrorCode::FileNotFound => FileOperationResult::FileNotFound,
        FileSystemProviderErrorCode::FileIsADirectory => FileOperationResult::FileIsDirectory,
        FileSystemProviderErrorCode::FileNotADirectory => FileOperationResult::FileNotDirectory,
        FileSystemProviderErrorCode::FileWriteLocked => FileOperationResult::FileWriteLocked,
        FileSystemProviderErrorCode::NoPermissions => FileOperationResult::FilePermissionDenied,
        FileSystemProviderErrorCode::FileExists => FileOperationResult::FileMoveConflict,
        FileSystemProviderErrorCode::FileTooLarge => FileOperationResult::FileTooLarge,
        _ => FileOperationResult::FileOtherError,
    }
}

/// O erro de uma operação que o provider não suporta (no original, chamar um método ausente).
pub fn unsupported(operation: &str) -> Error {
    Error::new(format!("provider does not support '{operation}'"))
}

// endregion

// region: eventos

/// `IFileChange`.
#[derive(Clone, Debug, PartialEq)]
pub struct FileChange {
    pub change_type: FileChangeType,
    pub resource: URI,
    pub c_id: Option<i64>,
}

impl FileChange {
    pub fn new(change_type: FileChangeType, resource: URI) -> Self {
        Self { change_type, resource, c_id: None }
    }
}

/// `FileOperationEvent`.
#[derive(Clone, Debug)]
pub struct FileOperationEvent {
    pub resource: URI,
    pub operation: FileOperation,
    pub target: Option<FileStatWithMetadata>,
}

impl FileOperationEvent {
    pub fn new(resource: URI, operation: FileOperation, target: Option<FileStatWithMetadata>) -> Self {
        Self { resource, operation, target }
    }

    pub fn is_operation(&self, operation: FileOperation) -> bool {
        self.operation == operation
    }
}

/// `IFileSystemProviderRegistrationEvent`.
#[derive(Clone)]
pub struct FileSystemProviderRegistrationEvent {
    pub added: bool,
    pub scheme: String,
    pub provider: Option<Rc<dyn FileSystemProvider>>,
}

/// `IFileSystemProviderCapabilitiesChangeEvent`.
#[derive(Clone)]
pub struct FileSystemProviderCapabilitiesChangeEvent {
    pub provider: Rc<dyn FileSystemProvider>,
    pub scheme: String,
}

/// `IFileSystemProviderActivationEvent`: `join` junta uma promessa que o serviço espera.
#[derive(Clone)]
pub struct FileSystemProviderActivationEvent {
    pub scheme: String,
    joiners: Rc<std::cell::RefCell<Vec<Promise<()>>>>,
}

impl FileSystemProviderActivationEvent {
    pub fn new(scheme: String) -> Self {
        Self { scheme, joiners: Rc::default() }
    }

    pub fn join(&self, promise: Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }

    pub fn take_joiners(&self) -> Vec<Promise<()>> {
        std::mem::take(&mut self.joiners.borrow_mut())
    }
}

/// `FileChangesEvent`.
pub struct FileChangesEvent {
    ignore_path_casing: bool,
    /// `None` é o `MIXED_CORRELATION` do original (`null`); `Some(None)` é "sem correlação ainda".
    correlation_id: Option<Option<i64>>,
    added: OnceCell<TernarySearchTree<URI, bool>>,
    updated: OnceCell<TernarySearchTree<URI, bool>>,
    deleted: OnceCell<TernarySearchTree<URI, bool>>,
    pub raw_added: Vec<URI>,
    pub raw_updated: Vec<URI>,
    pub raw_deleted: Vec<URI>,
}

impl FileChangesEvent {
    pub fn new(changes: &[FileChange], ignore_path_casing: bool) -> Self {
        let mut event = Self {
            ignore_path_casing,
            correlation_id: Some(None),
            added: OnceCell::new(),
            updated: OnceCell::new(),
            deleted: OnceCell::new(),
            raw_added: Vec::new(),
            raw_updated: Vec::new(),
            raw_deleted: Vec::new(),
        };
        for change in changes {
            match change.change_type {
                FileChangeType::Added => event.raw_added.push(change.resource.clone()),
                FileChangeType::Updated => event.raw_updated.push(change.resource.clone()),
                FileChangeType::Deleted => event.raw_deleted.push(change.resource.clone()),
            }
            if let Some(current) = event.correlation_id {
                event.correlation_id = match (change.c_id, current) {
                    (Some(c_id), None) => Some(Some(c_id)),
                    (Some(c_id), Some(existing)) if c_id == existing => Some(Some(existing)),
                    (None, None) => Some(None),
                    _ => None,
                };
            }
        }
        event
    }

    fn tree(&self, resources: &[URI]) -> TernarySearchTree<URI, bool> {
        let ignore = self.ignore_path_casing;
        let mut tree = TernarySearchTree::for_uris(Rc::new(move |_| ignore), Rc::new(|_| false));
        tree.fill(&resources.iter().map(|resource| (resource.clone(), true)).collect::<Vec<_>>());
        tree
    }

    /// `contains(resource, ...types)`.
    pub fn contains(&self, resource: &URI, types: &[FileChangeType]) -> bool {
        self.do_contains(resource, false, types)
    }

    /// `affects(resource, ...types)`.
    pub fn affects(&self, resource: &URI, types: &[FileChangeType]) -> bool {
        self.do_contains(resource, true, types)
    }

    fn do_contains(&self, resource: &URI, include_children: bool, types: &[FileChangeType]) -> bool {
        let has_types_filter = !types.is_empty();
        let wants = |t: FileChangeType| !has_types_filter || types.contains(&t);
        if wants(FileChangeType::Added) {
            let added = self.added.get_or_init(|| self.tree(&self.raw_added));
            if added.get(resource).is_some_and(|v| *v) || (include_children && added.find_superstr(resource).is_some()) {
                return true;
            }
        }
        if wants(FileChangeType::Updated) {
            let updated = self.updated.get_or_init(|| self.tree(&self.raw_updated));
            if updated.get(resource).is_some_and(|v| *v) || (include_children && updated.find_superstr(resource).is_some()) {
                return true;
            }
        }
        if wants(FileChangeType::Deleted) {
            let deleted = self.deleted.get_or_init(|| self.tree(&self.raw_deleted));
            // deleted também considera as pastas pai
            if deleted.find_substr(resource).is_some() || (include_children && deleted.find_superstr(resource).is_some()) {
                return true;
            }
        }
        false
    }

    pub fn got_added(&self) -> bool {
        !self.raw_added.is_empty()
    }

    pub fn got_deleted(&self) -> bool {
        !self.raw_deleted.is_empty()
    }

    pub fn got_updated(&self) -> bool {
        !self.raw_updated.is_empty()
    }

    /// Se todos os eventos do lote pertencem a este watcher correlacionado.
    pub fn correlates(&self, correlation_id: i64) -> bool {
        self.correlation_id == Some(Some(correlation_id))
    }

    pub fn has_correlation(&self) -> bool {
        matches!(self.correlation_id, Some(Some(_)))
    }
}

// endregion

/// `isParent(path, candidate, ignoreCase)`.
pub fn is_parent(path: &str, candidate: &str, ignore_case: bool) -> bool {
    if path.is_empty() || candidate.is_empty() || path == candidate {
        return false;
    }
    if candidate.encode_utf16().count() > path.encode_utf16().count() {
        return false;
    }
    let sep = if cfg!(windows) { "\\" } else { "/" };
    let mut candidate = candidate.to_string();
    if !candidate.ends_with(sep) {
        candidate.push_str(sep);
    }
    if ignore_case {
        let (p, c): (Vec<u16>, Vec<u16>) = (path.encode_utf16().collect(), candidate.encode_utf16().collect());
        return strings::starts_with_ignore_case(&p, &c);
    }
    path.starts_with(&candidate)
}

/// `ByteSize.formatSize`.
pub fn format_byte_size(size: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * KB;
    const GB: f64 = MB * KB;
    const TB: f64 = GB * KB;
    if size.is_nan() {
        return localize("sizeB", "{0}B", &["0"]);
    }
    if size < KB {
        return localize("sizeB", "{0}B", &[&format!("{size:.0}")]);
    }
    if size < MB {
        return localize("sizeKB", "{0}KB", &[&format!("{:.2}", size / KB)]);
    }
    if size < GB {
        return localize("sizeMB", "{0}MB", &[&format!("{:.2}", size / MB)]);
    }
    if size < TB {
        return localize("sizeGB", "{0}GB", &[&format!("{:.2}", size / GB)]);
    }
    localize("sizeTB", "{0}TB", &[&format!("{:.2}", size / TB)])
}

/// `ByteSize.KB`, `MB`, `GB` e `TB`.
pub mod byte_size {
    pub const KB: u64 = 1024;
    pub const MB: u64 = KB * KB;
    pub const GB: u64 = MB * KB;
    pub const TB: u64 = GB * KB;
}

/// `getLargeFileConfirmationLimit(uri)`: o limite para a confirmação de abrir arquivo grande.
pub fn get_large_file_confirmation_limit(uri: Option<&URI>) -> u64 {
    let is_remote = uri.is_some_and(|u| u.scheme().to_string_lossy() == je_base::network::schemas::VSCODE_REMOTE);
    let is_local = uri.is_some_and(|u| u.scheme().to_string_lossy() == je_base::network::schemas::FILE);
    if is_local {
        return 1024 * byte_size::MB;
    }
    if is_remote {
        return 10 * byte_size::MB;
    }
    if cfg!(target_arch = "wasm32") {
        return 50 * byte_size::MB;
    }
    1024 * byte_size::MB
}

/// `resource.with({ path })`.
pub fn with_path(resource: &URI, path: &str) -> URI {
    resource.with(&UriChange { path: je_base::uri::Field::Set(path.into()), ..UriChange::default() })
}

// region: o provider

/// `IFileSystemProvider` junto das interfaces de capacidade. As operações opcionais têm implementação
/// padrão que falha com [`unsupported`]; o serviço só as chama quando a máscara de
/// [`capabilities`](Self::capabilities) as anuncia.
pub trait FileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities;
    fn on_did_change_capabilities(&self) -> Event<()>;
    fn on_did_change_file(&self) -> Event<Vec<FileChange>>;
    /// `onDidWatchError?`.
    fn on_did_watch_error(&self) -> Option<Event<String>> {
        None
    }
    fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef;
    fn stat(&self, resource: &URI) -> Promise<Stat>;
    fn mkdir(&self, resource: &URI) -> Promise<()>;
    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>>;
    fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()>;
    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()>;

    fn copy(&self, _from: &URI, _to: &URI, _opts: &FileOverwriteOptions) -> Promise<()> {
        Promise::rejected(unsupported("copy"))
    }

    /// `readFile(resource, opts?: { atomic })`.
    fn read_file(&self, _resource: &URI, _atomic: bool) -> Promise<VSBuffer> {
        Promise::rejected(unsupported("readFile"))
    }

    fn write_file(&self, _resource: &URI, _content: VSBuffer, _opts: &FileWriteOptions) -> Promise<()> {
        Promise::rejected(unsupported("writeFile"))
    }

    fn read_file_stream(&self, _resource: &URI, _opts: &FileReadStreamOptions, _token: &CancellationToken) -> VSBufferReadableStream {
        let stream = je_base::buffer::new_writeable_buffer_stream(None);
        stream.error(unsupported("readFileStream"));
        stream.end(None);
        stream
    }

    fn open(&self, _resource: &URI, _opts: &FileOpenOptions) -> Promise<i64> {
        Promise::rejected(unsupported("open"))
    }

    fn close(&self, _fd: i64) -> Promise<()> {
        Promise::rejected(unsupported("close"))
    }

    /// `read(fd, pos, data, offset, length)`: preenche `data` a partir de `offset` e devolve os bytes lidos.
    fn read(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        Promise::rejected(unsupported("read"))
    }

    /// `write(fd, pos, data, offset, length)`: devolve os bytes escritos.
    fn write(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        Promise::rejected(unsupported("write"))
    }

    fn clone_file(&self, _from: &URI, _to: &URI) -> Promise<()> {
        Promise::rejected(unsupported("cloneFile"))
    }

    fn realpath(&self, _resource: &URI) -> Promise<String> {
        Promise::rejected(unsupported("realpath"))
    }

    /// `enforceAtomicReadFile?(resource)`.
    fn enforce_atomic_read_file(&self, _resource: &URI) -> bool {
        false
    }

    /// `enforceAtomicWriteFile?(resource)`.
    fn enforce_atomic_write_file(&self, _resource: &URI) -> Option<FileAtomicOptions> {
        None
    }

    /// `enforceAtomicDelete?(resource)`.
    fn enforce_atomic_delete(&self, _resource: &URI) -> Option<FileAtomicOptions> {
        None
    }

    /// `readOnlyMessage` (o markdown do original, como texto).
    fn read_only_message(&self) -> Option<String> {
        None
    }
}

/// O handle compartilhado de um provider.
pub type FileSystemProviderRef = Rc<dyn FileSystemProvider>;

macro_rules! capability_check {
    ($(#[$meta:meta])* $name:ident, $cap:ident) => {
        $(#[$meta])*
        pub fn $name(provider: &dyn FileSystemProvider) -> bool {
            provider.capabilities().intersects(FileSystemProviderCapabilities::$cap)
        }
    };
}

capability_check!(
    /// `hasReadWriteCapability`.
    has_read_write_capability, FILE_READ_WRITE
);
capability_check!(
    /// `hasFileAppendCapability`.
    has_file_append_capability, FILE_APPEND
);
capability_check!(
    /// `hasFileFolderCopyCapability`.
    has_file_folder_copy_capability, FILE_FOLDER_COPY
);
capability_check!(
    /// `hasFileCloneCapability`.
    has_file_clone_capability, FILE_CLONE
);
capability_check!(
    /// `hasFileRealpathCapability`.
    has_file_realpath_capability, FILE_REALPATH
);
capability_check!(
    /// `hasOpenReadWriteCloseCapability`.
    has_open_read_write_close_capability, FILE_OPEN_READ_WRITE_CLOSE
);
capability_check!(
    /// `hasFileReadStreamCapability`.
    has_file_read_stream_capability, FILE_READ_STREAM
);
capability_check!(
    /// `hasFileAtomicDeleteCapability`.
    has_file_atomic_delete_capability, FILE_ATOMIC_DELETE
);
capability_check!(
    /// `hasReadonlyCapability`.
    has_readonly_capability, READONLY
);

/// `hasFileAtomicReadCapability`: exige também `FileReadWrite`.
pub fn has_file_atomic_read_capability(provider: &dyn FileSystemProvider) -> bool {
    has_read_write_capability(provider) && provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_ATOMIC_READ)
}

/// `hasFileAtomicWriteCapability`: exige também `FileReadWrite`.
pub fn has_file_atomic_write_capability(provider: &dyn FileSystemProvider) -> bool {
    has_read_write_capability(provider) && provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_ATOMIC_WRITE)
}

// endregion

/// `HotExitConfiguration`: os valores da configuração `files.hotExit`.
pub struct HotExitConfiguration;

impl HotExitConfiguration {
    pub const OFF: &'static str = "off";
    pub const ON_EXIT: &'static str = "onExit";
    pub const ON_EXIT_AND_WINDOW_CLOSE: &'static str = "onExitAndWindowClose";
}
