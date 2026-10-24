//! O pedaço de `src/vs/platform/workspace/common/workspaceTrust.ts` de que os serviços portados precisam: o
//! `IWorkspaceTrustEnablementService` (se o recurso de confiança no workspace está ligado).
//!
//! Divergência: o arquivo original também declara `IWorkspaceTrustManagementService`,
//! `IWorkspaceTrustRequestService` e os tipos de pedido de confiança; eles entram quando um serviço portado
//! pedir.

use je_instantiation::service_id;

/// `IWorkspaceTrustEnablementService`.
pub trait IWorkspaceTrustEnablementService {
    /// `isWorkspaceTrustEnabled`.
    fn is_workspace_trust_enabled(&self) -> bool;
}

service_id!(pub I_WORKSPACE_TRUST_ENABLEMENT_SERVICE: dyn IWorkspaceTrustEnablementService = "workspaceTrustEnablementService");
