//! `resources.ts` e `uri.ts` ligados ao processo hospedeiro: o `joinPath`, o `extUriBiasedIgnorePathCase`
//! e os caminhos do sistema de arquivos, sem repetir em cada módulo a montagem do [`ExtUri`].

use std::rc::Rc;

use je_base::path::{Environment, Platform, ProcessEnvironment};
use je_base::resources::ExtUri;
use je_base::uri::URI;

/// O ambiente do processo (plataforma, diretório atual e variáveis).
pub fn host_environment() -> Rc<dyn Environment> {
    Rc::new(ProcessEnvironment)
}

/// A plataforma do hospedeiro.
pub fn host_platform() -> Platform {
    ProcessEnvironment.platform()
}

/// `extUri`: o utilitário de URIs sem viés, que toma os URIs como estão (`basename`, `dirname`, `isEqual`...).
pub fn ext_uri() -> ExtUri {
    ExtUri::unbiased(host_environment())
}

/// `joinPath(resource, ...pathFragments)` (o `extUri` sem viés).
pub fn join_path(resource: &URI, path_fragments: &[&str]) -> URI {
    ExtUri::unbiased(host_environment()).join_path(resource, path_fragments)
}

/// `extUriBiasedIgnorePathCase`.
pub fn ext_uri_biased_ignore_path_case() -> ExtUri {
    ExtUri::biased_ignore_path_case(host_environment(), cfg!(target_os = "linux"))
}

/// `URI.file(path)` na plataforma do hospedeiro.
pub fn uri_file(path: &str) -> URI {
    URI::file(path, host_platform())
}

/// `uri.fsPath` na plataforma do hospedeiro.
pub fn fs_path(uri: &URI) -> String {
    uri.fs_path(host_platform()).to_string_lossy()
}
