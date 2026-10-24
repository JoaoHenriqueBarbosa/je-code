//! Porte de `src/vs/platform/workspace/common/workspaceTrust.ts`: os três serviços de confiança no workspace
//! (`IWorkspaceTrustEnablementService`, `IWorkspaceTrustManagementService` e `IWorkspaceTrustRequestService`) e os
//! tipos dos pedidos de confiança.
//!
//! Divergências:
//! - **Propriedades** do original viram métodos: `readonly workspaceResolved: Promise<void>` é
//!   [`IWorkspaceTrustManagementService::workspace_resolved`] e a propriedade `acceptsOutOfWorkspaceFiles`
//!   (leitura e escrita) são [`IWorkspaceTrustManagementService::accepts_out_of_workspace_files`] e
//!   [`IWorkspaceTrustManagementService::set_accepts_out_of_workspace_files`]. Os eventos são métodos que devolvem o
//!   [`Event`].
//! - `completeOpenFilesTrustRequest(result, saveResponse?)` e `requestWorkspaceTrust(options?)` recebem `Option`.
//! - `requestResourcesTrust` e `requestWorkspaceTrust` devolvem `Promise<boolean | undefined>`, que é
//!   `Promise<Option<bool>>`.
//! - `WorkspaceTrustRequestButton.type` é o campo `button_type` (`type` é palavra reservada).
//! - `_serviceBrand` não existe: a identidade do serviço é o [`ServiceId`](je_instantiation::ServiceId).

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `WorkspaceTrustScope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTrustScope {
    Local = 0,
    Remote = 1,
}

/// O `type` de `WorkspaceTrustRequestButton`: `'ContinueWithTrust' | 'ContinueWithoutTrust' | 'Manage' | 'Cancel'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTrustRequestButtonType {
    ContinueWithTrust,
    ContinueWithoutTrust,
    Manage,
    Cancel,
}

/// `WorkspaceTrustRequestButton`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceTrustRequestButton {
    pub label: String,
    pub button_type: WorkspaceTrustRequestButtonType,
}

/// `ResourceTrustRequestOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceTrustRequestOptions {
    pub uri: URI,
    pub message: Option<String>,
}

/// `WorkspaceTrustRequestOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceTrustRequestOptions {
    pub buttons: Option<Vec<WorkspaceTrustRequestButton>>,
    pub message: Option<String>,
}

/// `IWorkspaceTrustEnablementService`.
pub trait IWorkspaceTrustEnablementService {
    /// `isWorkspaceTrustEnabled`.
    fn is_workspace_trust_enabled(&self) -> bool;
}

service_id!(pub I_WORKSPACE_TRUST_ENABLEMENT_SERVICE: dyn IWorkspaceTrustEnablementService = "workspaceTrustEnablementService");

/// `IWorkspaceTrustUriInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceTrustUriInfo {
    pub uri: URI,
    pub trusted: bool,
}

/// `IWorkspaceTrustInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceTrustInfo {
    pub uri_trust_info: Vec<WorkspaceTrustUriInfo>,
}

/// `IWorkspaceTrustTransitionParticipant`.
pub trait IWorkspaceTrustTransitionParticipant {
    fn participate(&self, trusted: bool) -> Promise<()>;
}

/// `IWorkspaceTrustManagementService`.
pub trait IWorkspaceTrustManagementService {
    fn on_did_change_trust(&self) -> Event<bool>;
    fn on_did_change_trusted_folders(&self) -> Event<()>;

    fn workspace_resolved(&self) -> Promise<()>;
    fn workspace_trust_initialized(&self) -> Promise<()>;

    /// `acceptsOutOfWorkspaceFiles` (leitura).
    fn accepts_out_of_workspace_files(&self) -> bool;
    /// `acceptsOutOfWorkspaceFiles` (escrita).
    fn set_accepts_out_of_workspace_files(&self, value: bool);

    fn is_workspace_trusted(&self) -> bool;
    fn is_workspace_trust_forced(&self) -> bool;

    fn can_set_parent_folder_trust(&self) -> bool;
    fn set_parent_folder_trust(&self, trusted: bool) -> Promise<()>;

    fn can_set_workspace_trust(&self) -> bool;
    fn set_workspace_trust(&self, trusted: bool) -> Promise<()>;

    fn get_uri_trust_info(&self, uri: &URI) -> Promise<WorkspaceTrustUriInfo>;
    fn set_uris_trust(&self, uris: &[URI], trusted: bool) -> Promise<()>;

    /// Automatically trusts an exact scheme and authority for the registration's lifetime, without persisting user trust.
    fn register_trusted_authority(&self, scheme: &str, authority: &str) -> DisposableRef;

    fn get_trusted_uris(&self) -> Vec<URI>;
    fn set_trusted_uris(&self, uris: &[URI]) -> Promise<()>;

    fn add_workspace_trust_transition_participant(&self, participant: Rc<dyn IWorkspaceTrustTransitionParticipant>) -> DisposableRef;
}

service_id!(pub I_WORKSPACE_TRUST_MANAGEMENT_SERVICE: dyn IWorkspaceTrustManagementService = "workspaceTrustManagementService");

/// `WorkspaceTrustUriResponse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTrustUriResponse {
    Open = 1,
    OpenInNewWindow = 2,
    Cancel = 3,
}

/// `IWorkspaceTrustRequestService`.
pub trait IWorkspaceTrustRequestService {
    fn on_did_initiate_open_files_trust_request(&self) -> Event<()>;
    fn on_did_initiate_workspace_trust_request(&self) -> Event<Option<WorkspaceTrustRequestOptions>>;
    fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()>;
    fn on_did_initiate_resources_trust_request(&self) -> Event<ResourceTrustRequestOptions>;

    fn complete_open_files_trust_request(&self, result: WorkspaceTrustUriResponse, save_response: Option<bool>) -> Promise<()>;
    fn request_open_files_trust(&self, open_files: &[URI]) -> Promise<WorkspaceTrustUriResponse>;

    fn complete_resources_trust_request(&self, uri: &URI, result: WorkspaceTrustUriResponse) -> Promise<()>;
    fn request_resources_trust(&self, options: ResourceTrustRequestOptions) -> Promise<Option<bool>>;

    fn cancel_workspace_trust_request(&self);
    fn complete_workspace_trust_request(&self, trusted: Option<bool>) -> Promise<()>;
    fn request_workspace_trust(&self, options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>>;
    fn request_workspace_trust_on_startup(&self);
}

service_id!(pub I_WORKSPACE_TRUST_REQUEST_SERVICE: dyn IWorkspaceTrustRequestService = "workspaceTrustRequestService");
