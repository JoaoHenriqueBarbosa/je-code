//! Porte de `src/vs/workbench/contrib/editSessions/browser/editSessionsFileSystemProvider.ts`: o provedor de arquivos somente
//! leitura do esquema `vscode-edit-sessions`, que serve o conteúdo de cada mudança guardada numa edit session (a vista Cloud
//! Changes abre os arquivos e os compara com a cópia local por ele).
//!
//! Divergências:
//! - `static readonly SCHEMA` é a constante associada [`EditSessionsFileSystemProvider::SCHEMA`], e o `IFileSystemProviderWithFileReadWriteCapability` é o
//!   trait [`FileSystemProvider`] do porte (uma só interface, com as operações opcionais devolvendo "não suportado" por padrão).
//! - `throw FileSystemProviderErrorCode.FileNotFound` lança o texto do código (`EntryNotFound`), que não é um
//!   `FileSystemProviderError`; o porte rejeita com um [`Error`] cuja mensagem é esse texto, e o `toFileSystemProviderErrorCode` do
//!   serviço de arquivos o lê como `Unknown`, como lê o texto cru do original.
//! - O `/(?<ref>[^/]+)\/(?<folderName>[^/]+)\/(?<filePath>.*)/.exec(...)` não é ancorado: [`match_resource_path`] procura a
//!   primeira posição em que ele casa (e o `.*` não atravessa quebra de linha), como o motor de expressões regulares.
//! - `Date.now()` é o relógio do `runtime`; `Event.None` é [`Event::none`] e `Disposable.None` é [`none`].
//! - `NotSupportedError` do `writeFile` é o `Error` com a mensagem `NotSupported`, que o construtor do original usa.

use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, none};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_platform::files::files::{
    FileChange, FileDeleteOptions, FileOverwriteOptions, FilePermission, FileSystemProvider, FileSystemProviderCapabilities, FileSystemProviderErrorCode, FileType as FsFileType,
    FileWriteOptions, Stat, WatchOptions,
};

use super::common::{Change, EDIT_SESSIONS_SCHEME, EditSession, IEditSessionsStorageService, SyncResource, decode_edit_session_file_content};

/// `EditSessionsFileSystemProvider`.
pub struct EditSessionsFileSystemProvider {
    edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
}

impl EditSessionsFileSystemProvider {
    /// `static readonly SCHEMA`.
    pub const SCHEMA: &'static str = EDIT_SESSIONS_SCHEME;

    pub fn new(edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>) -> Rc<Self> {
        Rc::new(Self { edit_sessions_storage_service })
    }
}

/// O `throw FileSystemProviderErrorCode.FileNotFound`.
fn file_not_found() -> Error {
    Error::new(FileSystemProviderErrorCode::FileNotFound.as_str())
}

/// Quebras de linha que o `.` de uma expressão regular sem a flag `s` não atravessa.
fn is_line_terminator(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/(?<ref>[^/]+)\/(?<folderName>[^/]+)\/(?<filePath>.*)/.exec(path)` sem âncora: `(ref, folderName, filePath)` da primeira
/// posição em que o padrão casa.
fn match_resource_path(path: &str) -> Option<(String, String, String)> {
    let characters: Vec<char> = path.chars().collect();
    let run_end = |from: usize| characters[from..].iter().position(|character| *character == '/').map_or(characters.len(), |offset| from + offset);

    for start in 0..characters.len() {
        // `[^/]+`
        let reference_end = run_end(start);
        if reference_end == start || reference_end >= characters.len() {
            continue;
        }
        // `\/` e `[^/]+`
        let folder_start = reference_end + 1;
        let folder_end = run_end(folder_start);
        if folder_end == folder_start || folder_end >= characters.len() {
            continue;
        }
        // `\/` e `.*`
        let file_start = folder_end + 1;
        let file_end = characters[file_start..].iter().position(|character| is_line_terminator(*character)).map_or(characters.len(), |offset| file_start + offset);
        return Some((
            characters[start..reference_end].iter().collect(),
            characters[folder_start..folder_end].iter().collect(),
            characters[file_start..file_end].iter().collect(),
        ));
    }
    None
}

/// `readFile(resource)`.
fn read_edit_session_file(edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>, resource: &URI) -> Promise<VSBuffer> {
    // `resource.path.substring(1)`
    let path: String = resource.path().to_string_lossy().chars().skip(1).collect();
    async_fn(async move {
        let Some((r#ref, folder_name, file_path)) = match_resource_path(&path) else {
            return Err(file_not_found());
        };
        let Some(data) = edit_sessions_storage_service.read(SyncResource::EditSessions, Some(r#ref)).await? else {
            return Err(file_not_found());
        };
        let content = EditSession::parse(&data.content)?;
        let change = content.folders.iter().find(|folder| folder.name == folder_name).and_then(|folder| folder.working_changes.iter().find(|change| change.relative_file_path() == file_path));
        match change {
            Some(Change::Addition { contents, .. }) => decode_edit_session_file_content(content.version, contents),
            _ => Err(file_not_found()),
        }
    })
}

impl FileSystemProvider for EditSessionsFileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        FileSystemProviderCapabilities::READONLY | FileSystemProviderCapabilities::FILE_READ_WRITE
    }

    fn read_file(&self, resource: &URI, _atomic: bool) -> Promise<VSBuffer> {
        read_edit_session_file(self.edit_sessions_storage_service.clone(), resource)
    }

    fn stat(&self, resource: &URI) -> Promise<Stat> {
        read_edit_session_file(self.edit_sessions_storage_service.clone(), resource).then(|content| {
            let current_time = i64::try_from(date_now()).unwrap_or(i64::MAX);
            Stat { file_type: FsFileType::FILE, permissions: Some(FilePermission::READONLY), mtime: current_time, ctime: current_time, size: content.byte_length() as u64 }
        })
    }

    // region: Unsupported file operations

    fn on_did_change_capabilities(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        Event::none()
    }

    fn watch(&self, _resource: &URI, _opts: &WatchOptions) -> DisposableRef {
        none()
    }

    fn mkdir(&self, _resource: &URI) -> Promise<()> {
        Promise::resolved(())
    }

    fn readdir(&self, _resource: &URI) -> Promise<Vec<(String, FsFileType)>> {
        Promise::resolved(Vec::new())
    }

    fn rename(&self, _from: &URI, _to: &URI, _opts: &FileOverwriteOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn delete(&self, _resource: &URI, _opts: &FileDeleteOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn write_file(&self, _resource: &URI, _content: VSBuffer, _opts: &FileWriteOptions) -> Promise<()> {
        Promise::rejected(Error::new("NotSupported"))
    }

    // endregion
}
