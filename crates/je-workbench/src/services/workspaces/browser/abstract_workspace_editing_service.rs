//! Porte de `DidEnterWorkspaceEvent` de `src/vs/workbench/services/workspaces/browser/abstractWorkspaceEditingService.ts`
//! e do contrato `IDidEnterWorkspaceEvent` de `common/workspaceEditing.ts`: o evento disparado depois de entrar num
//! workspace, ao qual os ouvintes juntam promessas (por exemplo, para migrar dados para o novo workspace).
//!
//! Notas de tradução:
//! - `join` recebe `&self`: a lista de promessas fica atrás de um `RefCell`, como o `readonly promises` mutável do original.
//! - `wait` é o `Promises.settled(this.promises)`: espera todas assentarem e rejeita só com o primeiro erro.

use std::cell::RefCell;

use je_base::async_::promises;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_platform::workspace::AnyWorkspaceIdentifier;

/// `IDidEnterWorkspaceEvent`.
pub trait IDidEnterWorkspaceEvent {
    fn old_workspace(&self) -> &AnyWorkspaceIdentifier;
    fn new_workspace(&self) -> &AnyWorkspaceIdentifier;
    fn join(&self, promise: Promise<()>);
}

/// `DidEnterWorkspaceEvent`.
pub struct DidEnterWorkspaceEvent {
    promises: RefCell<Vec<Promise<()>>>,
    pub old_workspace: AnyWorkspaceIdentifier,
    pub new_workspace: AnyWorkspaceIdentifier,
}

impl DidEnterWorkspaceEvent {
    pub fn new(old_workspace: AnyWorkspaceIdentifier, new_workspace: AnyWorkspaceIdentifier) -> Self {
        Self { promises: RefCell::new(Vec::new()), old_workspace, new_workspace }
    }

    pub fn join(&self, promise: Promise<()>) {
        self.promises.borrow_mut().push(promise);
    }

    pub async fn wait(&self) -> Result<(), Error> {
        let promises = self.promises.borrow().clone();
        promises::settled(promises).await?;
        Ok(())
    }
}

impl IDidEnterWorkspaceEvent for DidEnterWorkspaceEvent {
    fn old_workspace(&self) -> &AnyWorkspaceIdentifier {
        &self.old_workspace
    }

    fn new_workspace(&self) -> &AnyWorkspaceIdentifier {
        &self.new_workspace
    }

    fn join(&self, promise: Promise<()>) {
        DidEnterWorkspaceEvent::join(self, promise)
    }
}
