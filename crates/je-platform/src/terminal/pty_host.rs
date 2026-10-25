//! Porte de `src/vs/platform/terminal/node/ptyHost.ts`: a conexão com um pty host e o contrato de quem o inicia.
//!
//! Representação: `readonly onRequestConnection?` e `readonly onWillShutdown?` (membros opcionais) são métodos que devolvem
//! `Option<Event<()>>`, com `None` como o membro ausente (o padrão do trait). `{ code: number; signal: string }` do
//! `onDidProcessExit` é [`PtyHostExit`]. `IChannelClient` é o [`IChannelClient`] do `je-base`.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::parts::ipc::IChannelClient;

/// `{ code: number; signal: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyHostExit {
    pub code: i32,
    pub signal: String,
}

/// `IPtyHostConnection`.
pub struct IPtyHostConnection {
    pub client: Rc<dyn IChannelClient>,
    pub store: Rc<DisposableStore>,
    pub on_did_process_exit: Event<PtyHostExit>,
}

/// `IPtyHostStarter`.
pub trait IPtyHostStarter: IDisposable {
    fn on_request_connection(&self) -> Option<Event<()>> {
        None
    }

    fn on_will_shutdown(&self) -> Option<Event<()>> {
        None
    }

    /// Creates a pty host and connects to it.
    fn start(&self) -> IPtyHostConnection;
}
