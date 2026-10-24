//! Porte de `src/vs/platform/externalServices/common/marketplace.ts`: os cabeçalhos comuns das requisições ao
//! marketplace.
//!
//! Divergências: `IStorageService | undefined` é `Option<Rc<dyn StorageService>>`, e o `async` do original é um
//! [`Promise`] cujo trecho síncrono (`supportsTelemetry` e `getTelemetryLevel`, lidos antes do primeiro `await`) roda na
//! chamada, como no JavaScript. `telemetryService.machineId || serviceMachineId` trata o identificador vazio como ausente.

use crate::files::i_file_service::IFileService;
use std::rc::Rc;

use je_base::parts::request::{HeaderValue, IHeaders};
use je_base::promise::Promise;

use super::service_machine_id::get_service_machine_id;
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::FileService;
use crate::product::IProductService;
use crate::storage::StorageService;
use crate::telemetry::telemetry::{ITelemetryService, TelemetryLevel};
use crate::telemetry::telemetry_utils::{get_telemetry_level_of, supports_telemetry};

/// `resolveMarketplaceHeaders`.
pub fn resolve_marketplace_headers(
    version: &str,
    product_service: &dyn IProductService,
    environment_service: &dyn EnvironmentService,
    configuration_service: &dyn IConfigurationService,
    file_service: &Rc<dyn IFileService>,
    storage_service: Option<Rc<dyn StorageService>>,
    telemetry_service: Rc<dyn ITelemetryService>,
) -> Promise<IHeaders> {
    let mut headers = IHeaders::new();
    headers.insert("X-Market-Client-Id".to_owned(), HeaderValue::One(format!("VSCode {version}")));
    headers.insert("User-Agent".to_owned(), HeaderValue::One(format!("VSCode {version} ({})", product_service.name_short())));

    if supports_telemetry(product_service, environment_service) && get_telemetry_level_of(configuration_service) == TelemetryLevel::Usage {
        return get_service_machine_id(environment_service, file_service, storage_service).then(move |service_machine_id| {
            headers.insert("X-Market-User-Id".to_owned(), HeaderValue::One(service_machine_id.clone()));
            // Send machineId as VSCode-SessionId so we can correlate telemetry events across different services
            // machineId can be undefined sometimes (eg: when launching from CLI), so send serviceMachineId instead otherwise
            // Marketplace will reject the request if there is no VSCode-SessionId header
            let machine_id = telemetry_service.machine_id();
            let session_id = if machine_id.is_empty() { service_machine_id } else { machine_id };
            headers.insert("VSCode-SessionId".to_owned(), HeaderValue::One(session_id));
            headers
        });
    }

    Promise::resolved(headers)
}
