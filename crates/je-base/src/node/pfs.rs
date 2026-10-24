//! Porte parcial de `src/vs/base/node/pfs.ts`: só o que o `processes.ts`, o `powershell.ts` e o
//! `terminalProfiles.ts` usam (`SymlinkSupport.existsFile`, `SymlinkSupport.existsDirectory`,
//! `Promises.exists` e `Promises.readdir`).
//!
//! Divergências: as funções do original são `async` sobre o `fs/promises` do Node; aqui continuam `async fn`,
//! sobre o `std::fs` síncrono do Rust (o futuro nunca cede a execução). Um erro de E/S vira o ramo do `catch`
//! do original (`false`, ou o `Err` de `readdir`).

use std::future::Future;
use std::pin::Pin;

use crate::errors::Error;

/// Um futuro que fica na thread atual: o retorno dos métodos de traits de objeto (`IFsProvider`).
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// `SymlinkSupport`: as consultas que seguem o link simbólico até o alvo.
pub struct SymlinkSupport;

impl SymlinkSupport {
    /// `SymlinkSupport.existsFile(path)`: o caminho existe, é um arquivo e não é um link quebrado (o
    /// `metadata` segue o link, então um link quebrado falha e cai no `false`).
    pub async fn exists_file(path: &str) -> bool {
        std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
    }

    /// `SymlinkSupport.existsDirectory(path)`.
    pub async fn exists_directory(path: &str) -> bool {
        std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
    }
}

/// `Promises.exists(path)`: o caminho existe (sem seguir o link, como o `access` do Node que ele usa para
/// decidir; um link quebrado ainda "existe" para o `lstat`).
pub async fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `Promises.readdir(path)`: os nomes das entradas de um diretório.
pub async fn readdir(path: &str) -> Result<Vec<String>, Error> {
    let entries = std::fs::read_dir(path).map_err(|error| Error::new(error.to_string()))?;
    let mut names = Vec::new();
    for entry in entries {
        names.push(entry.map_err(|error| Error::new(error.to_string()))?.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

/// `fs.promises.readFile(path)`.
pub async fn read_file(path: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|error| Error::new(error.to_string()))
}
