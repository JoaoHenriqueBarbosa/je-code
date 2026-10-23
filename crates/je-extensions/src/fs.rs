//! O sistema de arquivos que o scanner, o perfil e o instalador usam: uma interface síncrona e pequena em vez do
//! `IFileService` do original (que é assíncrono, com URIs e provedores por esquema).
//!
//! Os caminhos são os da URI (`/` como separador, ver [`crate::paths`]). Há duas implementações: [`DiskFs`], sobre
//! `std::fs` (o disco real, o que o `FileService` com o provedor de disco faz no VS Code), e [`MemFs`], em memória,
//! que faz o papel do `InMemoryFileSystemProvider` nos testes.
//!
//! TODO(integração): ligar ao `je_platform::files::FileService` quando o laço de eventos do produto estiver de pé
//! (hoje ele é baseado em `Promise` e não tem consumidor síncrono).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::paths;

/// `FileStat`, reduzido ao que o gerenciamento lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub is_directory: bool,
    /// `mtime` em milissegundos desde a época.
    pub mtime: u64,
    /// O tamanho em bytes (0 para pastas).
    pub size: u64,
}

/// Uma entrada de pasta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_directory: bool,
}

/// `FileOperationResult.FILE_NOT_FOUND` e companhia, vistos pelo `io::ErrorKind`.
pub fn is_not_found(error: &io::Error) -> bool {
    error.kind() == ErrorKind::NotFound
}

/// As operações usadas. Todas dão `io::ErrorKind::NotFound` quando o caminho não existe.
pub trait ExtFs {
    fn read(&self, path: &str) -> io::Result<Vec<u8>>;
    /// `writeFile`: cria as pastas que faltam e sobrescreve.
    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()>;
    /// `createFile(..., { overwrite: false })`: `AlreadyExists` quando o arquivo já existe.
    fn create(&self, path: &str, contents: &[u8]) -> io::Result<()>;
    fn stat(&self, path: &str) -> io::Result<Stat>;
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;
    fn create_dir_all(&self, path: &str) -> io::Result<()>;
    /// `del` (com `recursive`): apaga arquivo ou pasta.
    fn remove(&self, path: &str, recursive: bool) -> io::Result<()>;
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;

    /// Define as permissões (os 9 bits `rwx`), para o executável de um VSIX continuar executável. O padrão não faz
    /// nada (a memória e o Windows não têm bits de execução).
    fn set_mode(&self, _path: &str, _mode: u32) -> io::Result<()> {
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.stat(path).is_ok()
    }

    fn read_to_string(&self, path: &str) -> io::Result<String> {
        String::from_utf8(self.read(path)?).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }
}

/// O disco, por `std::fs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiskFs;

fn disk(path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(paths::to_fs_path(path))
}

fn mtime_ms(metadata: &std::fs::Metadata) -> u64 {
    metadata.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

impl ExtFs for DiskFs {
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        std::fs::read(disk(path))
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let target = disk(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, contents)
    }

    fn create(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let target = disk(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(target)?;
        file.write_all(contents)
    }

    fn stat(&self, path: &str) -> io::Result<Stat> {
        let metadata = std::fs::metadata(disk(path))?;
        Ok(Stat { is_directory: metadata.is_dir(), mtime: mtime_ms(&metadata), size: if metadata.is_dir() { 0 } else { metadata.len() } })
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(disk(path))? {
            let entry = entry?;
            // Um link simbólico para pasta conta como pasta, como o `stat` do `FileService` resolve.
            let is_directory = std::fs::metadata(entry.path()).map(|m| m.is_dir()).unwrap_or(false);
            entries.push(DirEntry { name: entry.file_name().to_string_lossy().into_owned(), is_directory });
        }
        Ok(entries)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(disk(path))
    }

    fn remove(&self, path: &str, recursive: bool) -> io::Result<()> {
        let target = disk(path);
        let metadata = std::fs::symlink_metadata(&target)?;
        if metadata.is_dir() {
            if recursive { std::fs::remove_dir_all(&target) } else { std::fs::remove_dir(&target) }
        } else {
            std::fs::remove_file(&target)
        }
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        std::fs::rename(disk(from), disk(to))
    }

    #[cfg(unix)]
    fn set_mode(&self, path: &str, mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(disk(path), std::fs::Permissions::from_mode(mode & 0o777))
    }
}

#[derive(Clone, Debug)]
enum Node {
    File { contents: Vec<u8>, mtime: u64 },
    Directory { mtime: u64 },
}

/// Em memória, para os testes. A raiz `/` sempre existe.
#[derive(Debug, Default)]
pub struct MemFs {
    nodes: RefCell<BTreeMap<String, Node>>,
    clock: RefCell<u64>,
}

fn normalize(path: &str) -> String {
    let joined = paths::join_path("/", &[path]);
    if joined.len() > 1 { joined.trim_end_matches('/').to_owned() } else { joined }
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    fn tick(&self) -> u64 {
        let mut clock = self.clock.borrow_mut();
        *clock += 1;
        *clock
    }

    /// Quantos nós (arquivos e pastas) existem, a raiz não conta. Útil para os testes de limpeza.
    pub fn node_count(&self) -> usize {
        self.nodes.borrow().len()
    }

    /// Criar ou remover um filho muda o `mtime` da pasta, como no disco e no `InMemoryFileSystemProvider`.
    fn touch_parent(&self, path: &str) {
        let parent = paths::dirname(path);
        let mtime = self.tick();
        if let Some(Node::Directory { mtime: slot }) = self.nodes.borrow_mut().get_mut(&parent) {
            *slot = mtime;
        }
    }

    fn ensure_parents(&self, path: &str) {
        let mut ancestors = Vec::new();
        let mut current = paths::dirname(path);
        while current != "/" && current != "." && !current.is_empty() {
            ancestors.push(current.clone());
            current = paths::dirname(&current);
        }
        for ancestor in ancestors.into_iter().rev() {
            if !self.nodes.borrow().contains_key(&ancestor) {
                let mtime = self.tick();
                self.nodes.borrow_mut().insert(ancestor.clone(), Node::Directory { mtime });
                self.touch_parent(&ancestor);
            }
        }
    }
}

impl ExtFs for MemFs {
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        match self.nodes.borrow().get(&normalize(path)) {
            Some(Node::File { contents, .. }) => Ok(contents.clone()),
            Some(Node::Directory { .. }) => Err(io::Error::new(ErrorKind::IsADirectory, "é uma pasta")),
            None => Err(io::Error::new(ErrorKind::NotFound, format!("Unable to read file '{path}' (Error: Unable to resolve nonexistent file '{path}')"))),
        }
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let path = normalize(path);
        self.ensure_parents(&path);
        let mtime = self.tick();
        let existed = self.nodes.borrow_mut().insert(path.clone(), Node::File { contents: contents.to_vec(), mtime }).is_some();
        if !existed {
            self.touch_parent(&path);
        }
        Ok(())
    }

    fn create(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        if self.nodes.borrow().contains_key(&normalize(path)) {
            return Err(io::Error::new(ErrorKind::AlreadyExists, format!("File already exists: {path}")));
        }
        self.write(path, contents)
    }

    fn stat(&self, path: &str) -> io::Result<Stat> {
        let path = normalize(path);
        if path == "/" {
            return Ok(Stat { is_directory: true, mtime: 0, size: 0 });
        }
        match self.nodes.borrow().get(&path) {
            Some(Node::File { mtime, contents }) => Ok(Stat { is_directory: false, mtime: *mtime, size: contents.len() as u64 }),
            Some(Node::Directory { mtime }) => Ok(Stat { is_directory: true, mtime: *mtime, size: 0 }),
            None => Err(io::Error::new(ErrorKind::NotFound, format!("Unable to resolve nonexistent file '{path}'"))),
        }
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let path = normalize(path);
        if !self.stat(&path)?.is_directory {
            return Err(io::Error::new(ErrorKind::NotADirectory, "não é uma pasta"));
        }
        let prefix = if path == "/" { "/".to_owned() } else { format!("{path}/") };
        let nodes = self.nodes.borrow();
        let mut entries = Vec::new();
        for (key, node) in nodes.range(prefix.clone()..) {
            let Some(rest) = key.strip_prefix(&prefix) else { break };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            entries.push(DirEntry { name: rest.to_owned(), is_directory: matches!(node, Node::Directory { .. }) });
        }
        Ok(entries)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        let path = normalize(path);
        if path == "/" {
            return Ok(());
        }
        self.ensure_parents(&path);
        if !self.nodes.borrow().contains_key(&path) {
            let mtime = self.tick();
            self.nodes.borrow_mut().insert(path.clone(), Node::Directory { mtime });
            self.touch_parent(&path);
        }
        Ok(())
    }

    fn remove(&self, path: &str, recursive: bool) -> io::Result<()> {
        let path = normalize(path);
        let stat = self.stat(&path)?;
        let prefix = format!("{path}/");
        {
            let mut nodes = self.nodes.borrow_mut();
            let has_children = nodes.keys().any(|k| k.starts_with(&prefix));
            if stat.is_directory && has_children && !recursive {
                return Err(io::Error::new(ErrorKind::DirectoryNotEmpty, "a pasta não está vazia"));
            }
            nodes.retain(|key, _| key != &path && !key.starts_with(&prefix));
        }
        self.touch_parent(&path);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let (from, to) = (normalize(from), normalize(to));
        self.stat(&from)?;
        self.ensure_parents(&to);
        let prefix = format!("{from}/");
        let mut nodes = self.nodes.borrow_mut();
        let moved: Vec<(String, Node)> = nodes
            .iter()
            .filter(|(key, _)| **key == from || key.starts_with(&prefix))
            .map(|(key, node)| (key.clone(), node.clone()))
            .collect();
        for (key, _) in &moved {
            nodes.remove(key);
        }
        for (key, node) in moved {
            nodes.insert(format!("{to}{}", &key[from.len()..]), node);
        }
        drop(nodes);
        self.touch_parent(&from);
        self.touch_parent(&to);
        Ok(())
    }
}

/// `computeSize`: a soma dos tamanhos dos arquivos sob `path`, recursivamente.
pub fn compute_size(fs: &dyn ExtFs, path: &str) -> io::Result<u64> {
    let stat = fs.stat(path)?;
    if !stat.is_directory {
        return Ok(stat.size);
    }
    let mut total = 0;
    for entry in fs.read_dir(path)? {
        total += compute_size(fs, &paths::join_path(path, &[&entry.name]))?;
    }
    Ok(total)
}

/// Copia uma pasta do disco para um [`ExtFs`], recursivamente. Só para montar cenários de teste a partir de uma
/// pasta real (o teste de retrocompatibilidade).
pub fn copy_from_disk(disk_path: &Path, fs: &dyn ExtFs, target: &str) -> io::Result<()> {
    if disk_path.is_dir() {
        fs.create_dir_all(target)?;
        for entry in std::fs::read_dir(disk_path)? {
            let entry = entry?;
            copy_from_disk(&entry.path(), fs, &paths::join_path(target, &[&entry.file_name().to_string_lossy()]))?;
        }
        Ok(())
    } else {
        fs.write(target, &std::fs::read(disk_path)?)
    }
}
