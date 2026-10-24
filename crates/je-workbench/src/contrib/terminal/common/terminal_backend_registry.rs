//! Porte do registro de backends de `src/vs/platform/terminal/common/terminal.ts` (`TerminalExtensions.Backend`,
//! `ITerminalBackendRegistry` e a classe `TerminalBackendRegistry`): o mapa dos backends de terminal por autoridade remota.
//!
//! Fica no workbench, junto do [`ITerminalBackend`], que não pode viver em `je_platform` (herda o `ITerminalBackendWslPath`).
//!
//! Divergências:
//! - **`Registry.add(TerminalExtensions.Backend, ...)` e `Registry.as<ITerminalBackendRegistry>(...)`.** O registro é um único
//!   `TerminalBackendRegistry` por thread (o do grafo de objetos num thread só, como em `docs/design.md`), com as funções
//!   [`register_terminal_backend`], [`get_terminal_backend`] e [`terminal_backends`].
//! - **`ReadonlyMap<string, ITerminalBackend>`.** [`terminal_backends`] devolve a lista dos backends na ordem de registro (o
//!   `IterableIterator` de `backends.values()` do original).
//! - **`_clearForTesting`.** [`clear_terminal_backends_for_testing`] esvazia o registro, para os casos que o populam.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};

use super::terminal_backend::ITerminalBackend;

thread_local! {
    /// `TerminalBackendRegistry._backends`: a chave é a autoridade remota (vazia para o backend local), na ordem de registro.
    static BACKENDS: RefCell<Vec<(String, Rc<dyn ITerminalBackend>)>> = const { RefCell::new(Vec::new()) };
}

/// `_sanitizeRemoteAuthority`.
fn sanitize_remote_authority(remote_authority: Option<&str>) -> String {
    remote_authority.unwrap_or_default().to_owned()
}

/// `registerTerminalBackend(backend)`.
pub fn register_terminal_backend(backend: Rc<dyn ITerminalBackend>) {
    let key = sanitize_remote_authority(backend.remote_authority().as_deref());
    BACKENDS.with(|backends| {
        let mut backends = backends.borrow_mut();
        if backends.iter().any(|(existing, _)| *existing == key) {
            errors::throw(Error::new(format!("A terminal backend with remote authority '{key}' was already registered.")));
        }
        backends.push((key, backend));
    });
}

/// `getTerminalBackend(remoteAuthority)`.
pub fn get_terminal_backend(remote_authority: Option<&str>) -> Option<Rc<dyn ITerminalBackend>> {
    let key = sanitize_remote_authority(remote_authority);
    BACKENDS.with(|backends| backends.borrow().iter().find(|(existing, _)| *existing == key).map(|(_, backend)| backend.clone()))
}

/// `backends.values()`.
pub fn terminal_backends() -> Vec<Rc<dyn ITerminalBackend>> {
    BACKENDS.with(|backends| backends.borrow().iter().map(|(_, backend)| backend.clone()).collect())
}

/// Esvazia o registro (para os casos de teste que registram backends).
pub fn clear_terminal_backends_for_testing() {
    BACKENDS.with(|backends| backends.borrow_mut().clear());
}
