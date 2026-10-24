//! Porte de `src/vs/workbench/api/common/extHostApiDeprecationService.ts`: o aviso (uma vez por API e extensão) de uso de API
//! descontinuada, no log (só para extensões em desenvolvimento) e na telemetria (`extHostDeprecatedApiUsage`).
//!
//! Divergências:
//! - `IExtHostApiDeprecationService` (a interface com o decorator) é o trait [`IExtHostApiDeprecationService`]; `NullApiDeprecationService`
//!   é o [`NullApiDeprecationService`]. A `IExtensionDescription` é o [`ExtensionDescription`].
//! - O `$publicLog2` (com os tipos de classificação, que só o TypeScript lê) é `rpc_public_log2` com o objeto de dados.
//! - `options?: { usageId?: string }` é `Option<&str>`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::LogServiceRef;

use super::ext_host_protocol::{IMainContext, MainContext, MainThreadTelemetryShape};

/// `IExtHostApiDeprecationService`.
pub trait IExtHostApiDeprecationService {
    fn report(&self, api_id: &str, extension: &ExtensionDescription, migration_suggestion: &str, usage_id: Option<&str>);
}

/// `ExtHostApiDeprecationService`.
pub struct ExtHostApiDeprecationService {
    reported_usages: RefCell<HashSet<String>>,
    telemetry_shape: Rc<dyn MainThreadTelemetryShape>,
    ext_host_log_service: LogServiceRef,
}

impl ExtHostApiDeprecationService {
    pub fn new(rpc: &dyn IMainContext, ext_host_log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { reported_usages: RefCell::new(HashSet::new()), telemetry_shape: rpc.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY), ext_host_log_service })
    }

    fn get_usage_key(api_id: &str, extension: &ExtensionDescription, usage_id: Option<&str>) -> String {
        let root_key = format!("{api_id}-{}", extension.identifier.value);
        match usage_id.filter(|usage_id| !usage_id.is_empty()) {
            Some(usage_id) => format!("{root_key}-{usage_id}"),
            None => root_key,
        }
    }
}

impl IExtHostApiDeprecationService for ExtHostApiDeprecationService {
    fn report(&self, api_id: &str, extension: &ExtensionDescription, migration_suggestion: &str, usage_id: Option<&str>) {
        let key = Self::get_usage_key(api_id, extension, usage_id);
        if !self.reported_usages.borrow_mut().insert(key) {
            return;
        }

        if extension.is_under_development {
            self.ext_host_log_service.warn(&format!("[Deprecation Warning] '{api_id}' is deprecated. {migration_suggestion}"));
        }

        self.telemetry_shape.rpc_public_log2(
            "extHostDeprecatedApiUsage",
            Some(Dynamic::object([
                ("extensionId", Dynamic::string(&extension.identifier.value)),
                ("apiId", Dynamic::string(api_id)),
                ("usageId", Dynamic::string(usage_id.unwrap_or(""))),
            ])),
        );
    }
}

/// `NullApiDeprecationService`.
pub struct NullApiDeprecationService;

impl IExtHostApiDeprecationService for NullApiDeprecationService {
    fn report(&self, _api_id: &str, _extension: &ExtensionDescription, _migration_suggestion: &str, _usage_id: Option<&str>) {
        // noop
    }
}
