//! Porte de `src/vs/workbench/services/extensions/common/extensionDevOptions.ts`.
//!
//! Divergências:
//! - `IEnvironmentService` é o trait [`EnvironmentService`] da plataforma (o supertrait do `IWorkbenchEnvironmentService`).
//! - `typeof environmentService.debugExtensionHost.port === 'number'` é `port.is_some()` (o `port` do porte é
//!   `Option<f64>`); `!!debugExtensionHost.break` é o `break_` e `!debugExtensionHost.debugId` é `debug_id.is_none()` (o
//!   original também trata o texto vazio como ausente).

use je_base::network::schemas;
use je_platform::environment::environment::EnvironmentService;

/// `IExtensionDevOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IExtensionDevOptions {
    pub is_extension_dev_host: bool,
    pub is_extension_dev_debug: bool,
    pub is_extension_dev_debug_brk: bool,
    pub is_extension_dev_test_from_cli: bool,
}

/// `parseExtensionDevOptions(environmentService)`.
pub fn parse_extension_dev_options(environment_service: &dyn EnvironmentService) -> IExtensionDevOptions {
    // handle extension host lifecycle a bit special when we know we are developing an extension that runs inside
    let is_extension_dev_host = environment_service.is_extension_development();

    let mut debug_ok = true;
    if let Some(ext_dev_locs) = environment_service.extension_development_location_uri() {
        for location in &ext_dev_locs {
            if location.scheme().to_string_lossy() != schemas::FILE {
                debug_ok = false;
            }
        }
    }

    let debug_extension_host = environment_service.debug_extension_host();
    let is_extension_dev_debug = debug_ok && debug_extension_host.port.is_some();
    let is_extension_dev_debug_brk = debug_ok && debug_extension_host.break_;
    let is_extension_dev_test_from_cli =
        is_extension_dev_host && environment_service.extension_tests_location_uri().is_some() && debug_extension_host.debug_id.as_deref().is_none_or(str::is_empty);
    IExtensionDevOptions { is_extension_dev_host, is_extension_dev_debug, is_extension_dev_debug_brk, is_extension_dev_test_from_cli }
}
