//! Porte de `src/vs/workbench/services/telemetry/common/workbenchTelemetryUtils.ts`: a decisão de se as propriedades de
//! experimento entram nos eventos de telemetria.

use je_base::dynamic::Dynamic;
use je_platform::configuration::service::IConfigurationService;
use je_platform::product::IProductService;
use je_platform::telemetry::TelemetryLevel;
use je_platform::telemetry::telemetry_utils::get_telemetry_level_of;

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// Determines if experiment properties will be set on telemetry events.
/// When true, TelemetryService should buffer events until setExperimentProperty is called.
pub fn experiments_enabled(
    configuration_service: &dyn IConfigurationService,
    product_service: &dyn IProductService,
    environment_service: &dyn IWorkbenchEnvironmentService,
) -> bool {
    get_telemetry_level_of(configuration_service) == TelemetryLevel::Usage
        && product_service.tas_config().is_some()
        && !environment_service.disable_experiments()
        && environment_service.extension_tests_location_uri().is_none()
        && !environment_service.enable_smoke_test_driver().unwrap_or(false)
        && matches!(configuration_service.get_value(Some("workbench.enableExperiments"), None), Dynamic::Bool(true))
}
