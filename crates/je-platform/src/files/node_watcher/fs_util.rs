//! Auxiliares de sistema de arquivos dos watchers: o que o original pega do `fs`, do `Promises` e do
//! `realcase` de `base/node/pfs.ts`, mais a conversão entre caminho do sistema e `URI`.
//!
//! As chamadas de disco são bloqueantes (`std::fs`): o loop é de um thread só e as operações dos watchers
//! (`stat`, `readdir`, `realpath`) são pequenas. Onde o original faz `await promises.x()`, o código
//! chamador mantém o `await` de uma promessa já resolvida, que cede o loop como o JavaScript, então a
//! ordem em relação aos eventos e timers continua a do original.

use std::io;
use std::path::Path;

use je_base::path::{PathModule, ProcessEnvironment};
use je_base::uri::URI;

use crate::files::watcher::{is_linux, native_platform};

/// `URI.file(path)`.
pub fn file_uri(path: &str) -> URI {
    URI::file(path, native_platform())
}

/// `uri.fsPath`.
pub fn fs_path(uri: &URI) -> String {
    uri.fs_path(native_platform()).to_string_lossy()
}

/// `join` do módulo `path` da plataforma.
pub fn join(segments: &[&str]) -> String {
    PathModule::native(&ProcessEnvironment).join(segments)
}

/// `basename` do módulo `path` da plataforma.
pub fn basename(path: &str) -> String {
    PathModule::native(&ProcessEnvironment).basename(path, None)
}

/// `dirname` do módulo `path` da plataforma.
pub fn dirname(path: &str) -> String {
    PathModule::native(&ProcessEnvironment).dirname(path)
}

/// `isAbsolute` do módulo `path` da plataforma.
pub fn is_absolute(path: &str) -> bool {
    PathModule::native(&ProcessEnvironment).is_absolute(path)
}

/// `isEqual(a, b, !isLinux)` de `extpath.ts`.
pub fn is_equal(a: &str, b: &str) -> bool {
    je_base::extpath::is_equal(a, b, !is_linux())
}

/// `isEqualOrParent(base, parentCandidate, !isLinux)` de `extpath.ts`.
pub fn is_equal_or_parent(base: &str, parent_candidate: &str) -> bool {
    je_base::extpath::is_equal_or_parent(base, parent_candidate, !is_linux(), false, &ProcessEnvironment)
}

/// `pathToWatcherKey(path)`: o caminho, em minúsculas onde o sistema de arquivos ignora a caixa.
pub fn path_to_watcher_key(path: &str) -> String {
    if is_linux() { path.to_string() } else { path.to_lowercase() }
}

/// `Promises.exists(path)`.
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `Promises.realpath(path)`: o caminho canônico, sem o prefixo `\\?\` que o Windows acrescenta (o
/// `fs.realpath` do Node não o devolve).
pub fn real_path(path: &str) -> Option<String> {
    let canonical = std::fs::canonicalize(path).ok()?.to_string_lossy().into_owned();
    Some(strip_verbatim_prefix(canonical))
}

fn strip_verbatim_prefix(path: String) -> String {
    if cfg!(windows) {
        if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{unc}");
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    path
}

/// `Promises.readdir(path)`: os nomes dos filhos.
pub fn read_dir_names(path: &str) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

/// `realcase(path)` de `pfs.ts`: o caminho com a caixa que o disco guarda, ou `None` se ele não existe.
pub fn real_case(path: &str) -> Option<String> {
    if is_linux() {
        return Some(path.to_string()); // Linux has a case sensitive file system so there is no re-casing necessary
    }
    let dir = dirname(path);
    if path == dir {
        return Some(path.to_string()); // end recursion
    }
    let base = basename(path);
    let name = if base.is_empty() { path.to_lowercase() } else { base.to_lowercase() }; // can be '' for windows drive letters
    let entries = read_dir_names(&dir).ok()?;
    let found: Vec<&String> = entries.iter().filter(|e| e.to_lowercase() == name).collect();
    match found.len() {
        1 => Some(join(&[&real_case(&dir)?, found[0]])),
        0 => None,
        // must be a case sensitive file system
        _ => found.iter().find(|e| ***e == name).and_then(|e| Some(join(&[&real_case(&dir)?, e]))),
    }
}

/// O `normalizeNFC` que o macOS pede nos nomes vindos do disco, que lá ficam em NFD.
pub fn normalize_nfc(name: &str) -> String {
    let units: Vec<u16> = name.encode_utf16().collect();
    je_base::normalization::normalize_nfc(&units).to_string_lossy()
}
