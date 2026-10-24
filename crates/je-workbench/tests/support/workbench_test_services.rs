//! Porte do que o `extensionManifestPropertiesService.test.ts` usa de
//! `src/vs/workbench/test/common/workbenchTestServices.ts`: o `TestProductService` e o
//! `TestWorkspaceTrustEnablementService`.
//!
//! Notas de tradução:
//! - `TestProductService` (`{ _serviceBrand: undefined, ...product }`) é o [`ProductService`] padrão do porte.
//!   O `product` do original, nos testes, não traz `extensionKind`, `extensionPointExtensionKind`,
//!   `extensionUntrustedWorkspaceSupport` nem `extensionVirtualWorkspacesSupport`, e o padrão do porte também não.
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
