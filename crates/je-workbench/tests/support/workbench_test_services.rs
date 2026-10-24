//! Porte do que o teste do `AuthenticationAccessService` usa de `src/vs/workbench/test/common/workbenchTestServices.ts`.
//!
//! - `TestProductService` (`{ _serviceBrand: undefined, ...product }`) é [`TestProductService`]: o `ProductService`
//!   padrão (o `product.json` do VS Code estável) mais `trustedExtensionAuthAccess` mutável, porque o teste
//!   reatribui a propriedade depois de o serviço existir e o serviço a relê a cada chamada. Os demais campos
//!   repassam ao produto padrão.
//! - `TestStorageService` (`InMemoryStorageService` mais `testEmitWillSaveState`) não tem tipo próprio: este
//!   teste só usa o que é do `InMemoryStorageService`, e `testEmitWillSaveState` é o
//!   `AbstractStorageService::emit_will_save_state`, que já é público.

use std::cell::RefCell;

use je_base::product::StringsOrDictionary;
use je_platform::product::{EnabledTelemetryLevels, IProductService, ProductService};

#[derive(Default)]
pub struct TestProductService {
    product: ProductService,
    /// `productService.trustedExtensionAuthAccess`.
    trusted_extension_auth_access: RefCell<Option<StringsOrDictionary>>,
}

impl TestProductService {
    /// `productService.trustedExtensionAuthAccess = value`.
    pub fn set_trusted_extension_auth_access(&self, value: Option<StringsOrDictionary>) {
        *self.trusted_extension_auth_access.borrow_mut() = value;
    }
}

/// Os métodos de [`IProductService`] que só repassam ao produto padrão, escritos uma vez.
macro_rules! forward_to_product {
    ($($name:ident -> $ret:ty),* $(,)?) => {
        $(fn $name(&self) -> $ret {
            self.product.$name()
        })*
    };
}

impl IProductService for TestProductService {
    forward_to_product! {
        version -> &str,
        name_short -> &str,
        name_long -> &str,
        application_name -> &str,
        url_protocol -> &str,
        data_folder_name -> &str,
        shared_data_folder_name -> &str,
        quality -> Option<&str>,
        commit -> Option<&str>,
        enable_telemetry -> bool,
        ai_config_aria_key -> Option<&str>,
        msft_internal_domains -> &[String],
        enabled_telemetry_levels -> Option<EnabledTelemetryLevels>,
    }

    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_extension_auth_access.borrow().clone()
    }
}
