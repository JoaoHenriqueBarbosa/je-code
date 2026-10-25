//! Porte de `src/vs/base/node/ripgrep.ts`: o caminho do binário do ripgrep (`rgDiskPath`).
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento da busca:
//! - O original resolve `@vscode/ripgrep-universal` por `import()` (o que o torna assíncrono) e troca
//!   `node_modules.asar` por `node_modules.asar.unpacked`. Aqui não há `node_modules`: o caminho é o do binário que o
//!   produto empacota na mesma estrutura (`node_modules.asar.unpacked/@vscode/ripgrep-universal/bin/<so>-<arquitetura>/rg`)
//!   ao lado do executável, ou o que o hospedeiro registrar em [`set_rg_disk_path`]. Sem nenhum dos dois, vale o nome
//!   `rg`, que o sistema resolve pelo `PATH` ao lançar o processo. Como não há `import()` a esperar, [`rg_disk_path`] é
//!   síncrona e o `await rgDiskPath()` dos chamadores é uma chamada comum.
//! - O `Lazy` do original é um valor por thread (o laço de eventos do porte é de uma thread só).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

thread_local! {
    static RG_DISK_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// O diretório `<so>-<arquitetura>` do `@vscode/ripgrep-universal` desta plataforma.
fn platform_directory() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// O binário empacotado ao lado do executável, se existir.
fn bundled_rg_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    let binary = if cfg!(windows) { "rg.exe" } else { "rg" };
    let relative: PathBuf = ["node_modules.asar.unpacked", "@vscode", "ripgrep-universal", "bin", platform_directory().as_str(), binary].iter().collect();
    [directory.join("resources").join("app"), directory.to_path_buf()]
        .iter()
        .map(|root| root.join(&relative))
        .find(|candidate| Path::new(candidate).is_file())
}

/// `rgDiskPath()`.
pub fn rg_disk_path() -> String {
    if let Some(registered) = RG_DISK_PATH.with(|slot| slot.borrow().clone()) {
        return registered;
    }
    bundled_rg_path().map_or_else(|| "rg".to_owned(), |path| path.to_string_lossy().into_owned())
}

/// Registra o caminho do binário do ripgrep desta thread (o hospedeiro sabe onde empacotou o `rg`).
pub fn set_rg_disk_path(path: impl Into<String>) {
    let path = path.into();
    RG_DISK_PATH.with(|slot| *slot.borrow_mut() = Some(path));
}
