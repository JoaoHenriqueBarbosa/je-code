//! Porte de `src/vs/workbench/services/host/browser/host.ts`, só com o que os serviços portados chamam: o `IHostService`
//! com `reload`, `restart`, `hasFocus` e `onDidChangeFocus`.
//!
//! Não entram os demais membros (`focus`, `openWindow`, `toggleFullScreen`, `close`...):
//! dependem de tipos de janela que não foram portados. Cada um entra junto com o tipo de que depende.

use je_base::event::Event;
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

    /// `hasFocus`: se a janela tem o foco. O padrão (a janela tem foco) é o dos dublês que não observam o foco.
    fn has_focus(&self) -> bool {
        true
    }

    /// `onDidChangeFocus`: dispara com `true` quando a janela ganha o foco e com `false` quando o perde. O padrão
    /// nunca dispara, como o `Event.None` dos dublês.
    fn on_did_change_focus(&self) -> Event<bool> {
        Event::none()
    }
}

service_id!(pub I_HOST_SERVICE: dyn IHostService = "hostService");
