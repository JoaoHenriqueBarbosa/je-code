//! Porte de `src/vs/platform/workspace/common/editSessions.ts`: o serviço que liga a identidade de uma pasta do
//! workspace (um identificador estável, como a URL remota do repositório) aos provedores que as extensões registram.
//!
//! Divergências:
//! - Os `readonly _serviceBrand: undefined` não existem: o identificador do serviço é o [`I_EDIT_SESSION_IDENTITY_SERVICE`].
//! - `IWorkspaceFolder` é o [`WorkspaceFolder`] da plataforma.
//! - `IEditSessionIdentityProvider` e `IEditSessionIdentityCreateParticipant` são traits, registrados como `Rc<dyn ...>`.
//! - `getEditSessionIdentifier` e os métodos irmãos devolvem [`Promise`]; o `string | undefined` é `Option<String>`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::workspace::WorkspaceFolder;

/// `IEditSessionIdentityProvider`.
pub trait IEditSessionIdentityProvider {
    /// O esquema que o provedor atende.
    fn scheme(&self) -> String;

    fn get_edit_session_identifier(&self, workspace_folder: &WorkspaceFolder, token: CancellationToken) -> Promise<Option<String>>;

    fn provide_edit_session_identity_match(
        &self,
        workspace_folder: &WorkspaceFolder,
        identity1: &str,
        identity2: &str,
        token: CancellationToken,
    ) -> Promise<Option<EditSessionIdentityMatch>>;
}

/// `IEditSessionIdentityCreateParticipant`.
pub trait IEditSessionIdentityCreateParticipant {
    fn participate(&self, workspace_folder: &WorkspaceFolder, cancellation_token: CancellationToken) -> Promise<()>;
}

/// `IEditSessionIdentityService`.
pub trait IEditSessionIdentityService {
    fn register_edit_session_identity_provider(&self, provider: Rc<dyn IEditSessionIdentityProvider>) -> DisposableRef;

    fn get_edit_session_identifier(&self, workspace_folder: &WorkspaceFolder, cancellation_token: CancellationToken) -> Promise<Option<String>>;

    fn provide_edit_session_identity_match(
        &self,
        workspace_folder: &WorkspaceFolder,
        identity1: &str,
        identity2: &str,
        cancellation_token: CancellationToken,
    ) -> Promise<Option<EditSessionIdentityMatch>>;

    fn add_edit_session_identity_create_participant(&self, participants: Rc<dyn IEditSessionIdentityCreateParticipant>) -> DisposableRef;

    fn on_will_create_edit_session_identity(&self, workspace_folder: &WorkspaceFolder, cancellation_token: CancellationToken) -> Promise<()>;
}

service_id!(pub I_EDIT_SESSION_IDENTITY_SERVICE: dyn IEditSessionIdentityService = "editSessionIdentityService");

/// `EditSessionIdentityMatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSessionIdentityMatch {
    Complete = 100,
    Partial = 50,
    None = 0,
}
