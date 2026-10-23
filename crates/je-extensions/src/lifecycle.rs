//! Porte de `src/vs/platform/extensionManagement/node/extensionLifecycle.ts`: o que roda depois de desinstalar uma
//! extensão: o script `vscode:uninstall` do manifesto e a remoção da pasta de armazenamento global dela.
//!
//! TODO(integração): o script é um `node <arquivo> [args]`, que o original executa com `fork` do Node e 5 s de limite.
//! O je-code só terá um runtime JS quando o extension host (zjsc) estiver integrado; até lá o script é reconhecido
//! ([`parse_script`]) e registrado, mas não executado.

use je_platform::log::Logger;

use crate::fs::{ExtFs, is_not_found};
use crate::management::LocalExtension;
use crate::paths;

/// O script `vscode:<type>` do manifesto e seus argumentos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleScript {
    /// O caminho (da URI) do arquivo do script, dentro da pasta da extensão.
    pub script: String,
    pub args: Vec<String>,
}

/// `parseScript`: só scripts `node <arquivo>` valem.
pub fn parse_script(extension: &LocalExtension, lifecycle_type: &str, logger: &dyn Logger) -> Option<LifecycleScript> {
    let key = format!("vscode:{lifecycle_type}");
    let command = extension.manifest.scripts.as_ref()?.get(&key)?;
    let parts: Vec<&str> = command.split(' ').collect();
    if parts.len() < 2 || parts[0] != "node" || parts[1].is_empty() {
        logger.warn(&format!("{} {} {key} should be a node script", extension.identifier.id, extension.manifest.version));
        return None;
    }
    Some(LifecycleScript { script: paths::join_path(&extension.location, &[parts[1]]), args: parts[2..].iter().map(|a| (*a).to_owned()).collect() })
}

/// `postUninstall`: devolve o script que deveria rodar (para o chamador com runtime executar) e apaga a pasta de
/// armazenamento global da extensão (`<globalStorageHome>/<id em minúscula>`).
pub fn post_uninstall(extension: &LocalExtension, global_storage_home: &str, fs: &dyn ExtFs, logger: &dyn Logger) -> Option<LifecycleScript> {
    let script = parse_script(extension, "uninstall", logger);
    if script.is_some() {
        logger.info(&format!("{} {} Running post uninstall script", extension.identifier.id, extension.manifest.version));
    }
    let storage_path = paths::join_path(global_storage_home, &[&extension.identifier.id.to_lowercase()]);
    if let Err(error) = fs.remove(&storage_path, true)
        && !is_not_found(&error)
    {
        logger.error(&format!("Error while removing extension storage path {} {error}", extension.identifier.id));
    }
    script
}
