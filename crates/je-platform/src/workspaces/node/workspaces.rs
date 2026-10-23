//! Porte de `src/vs/platform/workspaces/node/workspaces.ts`: os identificadores de workspace estáveis do Node.
//!
//! NOTE: DO NOT CHANGE. IDENTIFIERS HAVE TO REMAIN STABLE.
//!
//! O `Stats` do Node é a [`FolderStat`] com os três campos que o original lê (`ino`, `birthtime`, `birthtimeMs`).

use je_base::network::schemas;
use je_base::platform::{OperatingSystem, Platform};
use je_base::resources::original_fs_path;
use je_base::uri::URI;

use super::md5::md5_hex;
use crate::workspace::{EmptyWorkspaceIdentifier, SingleFolderWorkspaceIdentifier, WorkspaceIdentifier};

/// Tamanho dos identificadores que não são vazios: hashes MD5 (128 bits / 4 por causa do hexadecimal).
pub const NON_EMPTY_WORKSPACE_ID_LENGTH: usize = 128 / 4;

/// Os campos de `fs.Stats` que o identificador de pasta lê.
#[derive(Clone, Copy, Debug, Default)]
pub struct FolderStat {
    pub ino: f64,
    pub birthtime_ms: f64,
}

/// `getWorkspaceIdentifier`. `platform` é a plataforma que o original lê de `platform.ts`.
pub fn get_workspace_identifier(config_path: &URI, platform: &Platform) -> WorkspaceIdentifier {
    let mut config_path_str =
        if config_path.scheme() == schemas::FILE { original_fs_path(config_path, platform.path_flavor()).to_string_lossy() } else { config_path.to_string(false).to_string_lossy() };
    if !platform.is_linux {
        config_path_str = config_path_str.to_lowercase(); // sanitize for platform file system
    }
    WorkspaceIdentifier { id: md5_hex(config_path_str.as_bytes()), config_path: config_path.clone() }
}

/// `String(number)` para os inteiros que o `ino` e o `birthtime` são.
fn js_number_string(value: f64) -> String {
    je_base::dynamic::number_to_js_string(value)
}

/// `getSingleFolderWorkspaceIdentifier`: `None` para uma pasta local sem `stat` (para o identificador ser estável
/// para o mesmo URI).
pub fn get_single_folder_workspace_identifier(folder_uri: &URI, folder_stat: Option<&FolderStat>, platform: &Platform) -> Option<SingleFolderWorkspaceIdentifier> {
    let folder_id = (|| -> Option<String> {
        // Remote: produce a hash from the entire URI
        if folder_uri.scheme() != schemas::FILE {
            return Some(md5_hex(folder_uri.to_string(false).to_string_lossy().as_bytes()));
        }

        // Local: we use the ctime as extra salt to the identifier so that folders getting recreated result in a
        // different identifier. However, if the stat is not provided we return `undefined` to ensure identifiers
        // are stable for the given URI.
        let folder_stat = folder_stat?;

        let ctime: Option<f64> = match platform.os {
            // Linux: birthtime is ctime, so we cannot use it! We use the ino instead!
            OperatingSystem::Linux => Some(folder_stat.ino),
            // macOS: birthtime is fine to use as is
            OperatingSystem::Macintosh => Some(folder_stat.birthtime_ms),
            // Windows: fix precision issue in node.js 8.x to get 7.x results
            OperatingSystem::Windows => Some(folder_stat.birthtime_ms.floor()),
        };

        let mut data = folder_uri.fs_path(platform.path_flavor()).to_string_lossy().into_bytes();
        if let Some(ctime) = ctime.filter(|c| *c != 0.0 && !c.is_nan()) {
            data.extend_from_slice(js_number_string(ctime).as_bytes());
        }
        Some(md5_hex(&data))
    })();

    folder_id.map(|id| SingleFolderWorkspaceIdentifier { id, uri: folder_uri.clone() })
}

/// `createEmptyWorkspaceIdentifier`.
pub fn create_empty_workspace_identifier() -> EmptyWorkspaceIdentifier {
    let random = (js_random() * 1000.0).round();
    EmptyWorkspaceIdentifier { id: js_number_string(je_base::runtime::date_now() as f64 + random) }
}

fn js_random() -> f64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    f64::from(nanos % 1_000_000) / 1_000_000.0
}
