//! Porte de `src/vs/platform/externalServices/common/marketplace.ts` e `serviceMachineId.ts`: os cabeçalhos comuns
//! das requisições ao marketplace e o identificador de máquina do serviço.

use je_base::uuid::{generate_uuid, is_uuid};

use crate::fs::ExtFs;

/// `getServiceMachineId`: o UUID guardado em `machine_id_resource`, criado quando não existe (ou não é UUID).
pub fn get_service_machine_id(fs: &dyn ExtFs, machine_id_resource: &str) -> String {
    let stored = fs.read_to_string(machine_id_resource).ok().filter(|value| is_uuid(value));
    stored.unwrap_or_else(|| {
        let uuid = generate_uuid();
        // noop em caso de erro de escrita
        let _ = fs.write(machine_id_resource, uuid.as_bytes());
        uuid
    })
}

/// `resolveMarketplaceHeaders`. `send_usage_identifiers` é o "suporta telemetria e o nível é uso" do original: só aí vão
/// `X-Market-User-Id` e `VSCode-SessionId`. O je-code não tem telemetria própria, então o padrão do chamador é `false`.
pub fn resolve_marketplace_headers(
    version: &str,
    name_short: &str,
    send_usage_identifiers: bool,
    fs: &dyn ExtFs,
    machine_id_resource: &str,
    telemetry_machine_id: Option<&str>,
) -> Vec<(String, String)> {
    let mut headers = vec![("X-Market-Client-Id".to_owned(), format!("VSCode {version}")), ("User-Agent".to_owned(), format!("VSCode {version} ({name_short})"))];
    if send_usage_identifiers {
        let service_machine_id = get_service_machine_id(fs, machine_id_resource);
        headers.push(("X-Market-User-Id".to_owned(), service_machine_id.clone()));
        // Send machineId as VSCode-SessionId so we can correlate telemetry events across different services
        headers.push(("VSCode-SessionId".to_owned(), telemetry_machine_id.map_or(service_machine_id, str::to_owned)));
    }
    headers
}
