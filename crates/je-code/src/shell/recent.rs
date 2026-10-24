//! O `Open Recent` com a lista do próprio VS Code: a chave `history.recentlyOpenedPathsList` do armazenamento
//! compartilhado da aplicação (`APPLICATION_SHARED`: `~/<sharedDataFolderName>/sharedStorage/state.vscdb`, com
//! `.vscode-shared` no VS Code estável) e, nas versões antigas, do `User/globalStorage/state.vscdb`, lida pelo
//! `SqliteStorageDatabase` do `je-storage` (o mesmo formato, em Rust puro). O banco é copiado antes de abrir, para
//! não disputar a trava com um VS Code aberto ao mesmo tempo; a lista é só lida.
//!
//! As pastas abertas pelo VS Code no WSL (`vscode-remote://wsl+<distro>/...`) viram caminhos locais: no Windows,
//! `\\wsl.localhost\<distro>\...`; dentro do próprio WSL, o caminho como está.

use std::path::PathBuf;

use je_base::runtime;
use je_storage::{SqliteStorageDatabase, SqliteStorageDatabaseOptions, StorageDatabase};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recent {
    Folder(PathBuf),
    File(PathBuf),
}

/// O caminho local de uma URI `file://` (com os `%XX` decodificados; `/c:/x` vira `c:/x` no Windows).
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16) {
            decoded.push(v);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let path = String::from_utf8(decoded).ok()?;
    if cfg!(windows) {
        return Some(PathBuf::from(path.trim_start_matches('/')));
    }
    // Um caminho do Windows (`/c:/x`) lido de dentro do WSL é o `/mnt/c/x`; fora do WSL não existe aqui.
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        if std::env::var_os("WSL_DISTRO_NAME").is_none() {
            return None;
        }
        return Some(PathBuf::from(format!("/mnt/{}{}", (bytes[1] as char).to_ascii_lowercase(), &path[3..])));
    }
    Some(PathBuf::from(path))
}

/// O caminho local de uma pasta do WSL (`vscode-remote://wsl%2B<distro>/caminho`).
fn wsl_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("vscode-remote://wsl%2B").or_else(|| uri.strip_prefix("vscode-remote://wsl+"))?;
    let (distro, path) = rest.split_once('/')?;
    let path = file_uri_to_path(&format!("file:///{path}"))?;
    if cfg!(windows) {
        let relative = path.to_string_lossy().trim_start_matches('/').replace('/', "\\");
        Some(PathBuf::from(format!("\\\\wsl.localhost\\{distro}\\{relative}")))
    } else if std::env::var_os("WSL_DISTRO_NAME").is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case(distro)) {
        Some(PathBuf::from("/").join(path.to_string_lossy().trim_start_matches('/')))
    } else {
        None
    }
}

/// Lê a lista (as entradas locais e as do WSL; as outras remotas e os workspaces `.code-workspace` ficam de fora).
pub fn parse(json: &str) -> Vec<Recent> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    value["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    let local = |uri: &str| file_uri_to_path(uri).or_else(|| wsl_uri_to_path(uri));
                    if let Some(uri) = e["folderUri"].as_str() {
                        local(uri).map(Recent::Folder)
                    } else {
                        e["fileUri"].as_str().and_then(local).map(Recent::File)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Os bancos onde a lista pode estar, na ordem em que o VS Code a procura.
fn databases() -> Vec<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let shared = std::env::var_os("JE_USER_DATA_DIR").map(|d| PathBuf::from(d).join("sharedStorage/state.vscdb")).or_else(|| home.map(|h| h.join(".vscode-shared/sharedStorage/state.vscdb")));
    shared.into_iter().chain(super::themes::user_data_dir().map(|d| d.join("User/globalStorage/state.vscdb"))).filter(|p| p.is_file()).collect()
}

/// A lista do VS Code do usuário, ou vazia se não existe.
pub fn load() -> Vec<Recent> {
    databases().into_iter().map(|db| load_from(&db)).find(|list| !list.is_empty()).unwrap_or_default()
}

fn load_from(db: &std::path::Path) -> Vec<Recent> {
    let copy = std::env::temp_dir().join(format!("je-code-{}-state.vscdb", std::process::id()));
    if std::fs::copy(db, &copy).is_err() {
        return Vec::new();
    }
    let database = SqliteStorageDatabase::new(&copy.to_string_lossy(), SqliteStorageDatabaseOptions { logging: None, use_wal: false, busy_timeout: None });
    let items = database.get_items();
    while !items.is_settled() {
        runtime::turn();
    }
    let list = items.result().and_then(Result::ok).and_then(|map| map.get("history.recentlyOpenedPathsList"));
    let _ = database.close(None);
    runtime::turn();
    let _ = std::fs::remove_file(&copy);
    list.map(|json| parse(&json)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_folders_and_files_and_skips_remotes() {
        let json = r#"{"entries":[{"folderUri":"file:///home/u/my%20proj"},{"folderUri":"vscode-remote://ssh-remote%2Bhost/x"},{"fileUri":"file:///home/u/a.txt"},{"workspace":{"id":"1","configPath":"file:///w.code-workspace"}}]}"#;
        let list = parse(json);
        if cfg!(windows) {
            assert_eq!(list.len(), 2);
        } else {
            assert_eq!(list, vec![Recent::Folder(PathBuf::from("/home/u/my proj")), Recent::File(PathBuf::from("/home/u/a.txt"))]);
        }
    }

    /// Lê o `state.vscdb` de verdade apontado por `JE_USER_DATA_DIR` (manual: `cargo test -- --ignored`).
    #[test]
    #[ignore = "precisa de uma pasta de dados do VS Code"]
    fn loads_the_real_list() {
        let list = load();
        eprintln!("{} itens: {:?}", list.len(), list.iter().take(5).collect::<Vec<_>>());
        assert!(!list.is_empty());
    }
}
