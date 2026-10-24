//! Porte do que os testes usam de `src/vs/workbench/test/common/workbenchTestServices.ts`: o
//! `TestProductService` e o `TestWorkspaceTrustEnablementService`.
//!
//! Notas de tradução:
//! - `TestProductService` (`{ _serviceBrand: undefined, ...product }`) é o [`ProductService`] padrão do porte.
//!   O `product` do original, nos testes, não traz `extensionKind`, `extensionPointExtensionKind`,
//!   `extensionUntrustedWorkspaceSupport` nem `extensionVirtualWorkspacesSupport`, e o padrão do porte também não.
//!   O teste do `AuthenticationAccessService` reatribui `trustedExtensionAuthAccess` com
//!   `ProductService::set_trusted_extension_auth_access`.
//! - `TestStorageService` (`InMemoryStorageService` mais `testEmitWillSaveState`) não tem tipo próprio: os testes só
//!   usam o que é do `InMemoryStorageService`, e `testEmitWillSaveState` é o
//!   `AbstractStorageService::emit_will_save_state`, que já é público.
#![allow(dead_code)]

use std::rc::Rc;

use je_platform::product::ProductService;
use je_platform::workspace_trust::IWorkspaceTrustEnablementService;

/// `TestProductService`.
pub fn test_product_service() -> Rc<ProductService> {
    Rc::new(ProductService::default())
}

/// `TestWorkspaceTrustEnablementService`.
pub struct TestWorkspaceTrustEnablementService {
    is_enabled: bool,
}

impl TestWorkspaceTrustEnablementService {
    /// `constructor(isEnabled = true)`.
    pub fn new(is_enabled: bool) -> Rc<Self> {
        Rc::new(Self { is_enabled })
    }
}

impl IWorkspaceTrustEnablementService for TestWorkspaceTrustEnablementService {
    fn is_workspace_trust_enabled(&self) -> bool {
        self.is_enabled
    }
}
