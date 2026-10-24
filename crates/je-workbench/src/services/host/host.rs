//! Porte de `src/vs/workbench/services/host/browser/host.ts`, só com o que os serviços portados chamam: o `IHostService`
//! com `reload` e `restart`.
//!
//! Não entram os demais membros (`onDidChangeFocus`, `hasFocus`, `focus`, `openWindow`, `toggleFullScreen`, `close`...):
//! dependem de tipos de janela que não foram portados. Cada um entra junto com o tipo de que depende.

use je_base::promise::Promise;
use je_instantiation::service_id;

/// O `{ disableExtensions?: boolean }` de `IReloadOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadOptions {
    pub disable_extensions: Option<bool>,
}

/// `IHostService`, reduzido a `reload` e `restart`.
pub trait IHostService {
    /// `reload(options?)`.
    fn reload(&self, options: Option<ReloadOptions>) -> Promise<()>;

    /// `restart()`: reinicia a aplicação.
    fn restart(&self) -> Promise<()>;
}

service_id!(pub I_HOST_SERVICE: dyn IHostService = "hostService");
