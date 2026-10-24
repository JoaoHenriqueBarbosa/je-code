//! Porte de `src/vs/workbench/services/telemetry/browser/workbenchCommonProperties.ts`: as propriedades comuns
//! anexadas a todo evento de telemetria do navegador.
//!
//! Notas de tradução:
//! - `ICommonProperties` é um objeto JavaScript com três propriedades de leitura dinâmica (`timestamp`,
//!   `common.timesincesessionstart` e `common.sequence`, definidas com `Object.defineProperties` e `get`).
//!   Aqui é [`CommonProperties`]: uma lista ordenada de pares (as propriedades estáticas e as mescladas) mais o
//!   estado das três dinâmicas, que [`CommonProperties::get`] recalcula a cada leitura. `undefined` é
//!   `Dynamic::Undefined` e a chave continua presente, como no objeto original.
//! - `resolveAdditionalProperties` devolve um objeto: aqui, a lista de pares. O `mixin` do original lê as
//!   propriedades uma vez (uma propriedade com `get` é avaliada nessa hora), como o laço de [`mix_in`].
//! - `productService` pode ser `undefined` (o teste do original passa `undefined!`): é `Option`.
//! - `Gesture.isTouchDevice()` (`'ontouchstart' in window || navigator.maxTouchPoints > 0`) não tem janela fora
//!   do navegador, então é falso; o ramo web de `Platform::detect` não carrega `maxTouchPoints`.

use std::cell::Cell;

use je_base::dynamic::Dynamic;
use je_base::JsString;
use je_base::platform::Platform;
use je_base::runtime;
use je_base::uuid::generate_uuid;
use je_platform::product::IProductService;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::{FIRST_SESSION_DATE_STORAGE_KEY, LAST_SESSION_DATE_STORAGE_KEY, MACHINE_ID_KEY};
use je_platform::telemetry::telemetry_utils::clean_remote_authority;

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// `ICommonProperties`.
pub struct CommonProperties {
    entries: Vec<(String, Dynamic)>,
    /// `seq`: o contador de `common.sequence`.
    sequence: Cell<u32>,
    /// `startTime`.
    start_time: u64,
}

/// As propriedades dinâmicas, na ordem de definição do original.
const DYNAMIC_KEYS: [&str; 3] = ["timestamp", "common.timesincesessionstart", "common.sequence"];

impl CommonProperties {
    fn set(&mut self, key: &str, value: Dynamic) {
        match self.entries.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    /// `key in props`.
    pub fn has_own(&self, key: &str) -> bool {
        DYNAMIC_KEYS.contains(&key) || self.entries.iter().any(|(name, _)| name == key)
    }

    /// `props[key]`: as propriedades dinâmicas são recalculadas a cada chamada; uma chave ausente é `undefined`.
    pub fn get(&self, key: &str) -> Dynamic {
        if let Some((_, value)) = self.entries.iter().find(|(name, _)| name == key) {
            return value.clone();
        }
        match key {
            "timestamp" => Dynamic::date(runtime::date_now() as f64),
            "common.timesincesessionstart" => Dynamic::Number(runtime::date_now().saturating_sub(self.start_time) as f64),
            "common.sequence" => {
                let current = self.sequence.get();
                self.sequence.set(current + 1);
                Dynamic::Number(current as f64)
            }
            _ => Dynamic::Undefined,
        }
    }

    /// `Object.keys(props)`: as estáticas e as mescladas na ordem de inserção, e as dinâmicas depois das que
    /// existiam quando elas foram definidas.
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.entries.iter().map(|(name, _)| name.clone()).collect();
        for key in DYNAMIC_KEYS {
            if !keys.iter().any(|name| name == key) {
                keys.push(key.to_string());
            }
        }
        keys
    }
}

/// `cleanUserAgent`: `userAgent.replace(/(\d+\.\d+)(\.\d+)+/g, '$1')`.
fn clean_user_agent(user_agent: &str) -> String {
    let chars: Vec<char> = user_agent.chars().collect();
    let digits_end = |from: usize| {
        let mut end = from;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        end
    };
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            // `(\d+\.\d+)`
            let major_end = digits_end(index);
            if chars.get(major_end) == Some(&'.') && digits_end(major_end + 1) > major_end + 1 {
                let group_end = digits_end(major_end + 1);
                // `(\.\d+)+`
                let mut end = group_end;
                while chars.get(end) == Some(&'.') && digits_end(end + 1) > end + 1 {
                    end = digits_end(end + 1);
                }
                if end > group_end {
                    result.extend(&chars[index..group_end]);
                    index = end;
                    continue;
                }
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn text(value: &str) -> Dynamic {
    Dynamic::String(JsString::from(value))
}

fn optional_text(value: Option<&str>) -> Dynamic {
    value.map_or(Dynamic::Undefined, text)
}

/// `resolveWorkbenchCommonProperties`.
pub fn resolve_workbench_common_properties(
    storage_service: &dyn StorageService,
    product_service: Option<&dyn IProductService>,
    environment_service: &dyn IWorkbenchEnvironmentService,
    is_internal_telemetry: bool,
    resolve_additional_properties: Option<&dyn Fn() -> Vec<(String, Dynamic)>>,
) -> CommonProperties {
    let commit = product_service.and_then(|product| product.commit());
    let version = product_service.map(|product| product.version());
    let product_identifier = product_service.and_then(|product| product.embedder_identifier());
    let remove_machine_id = product_service.is_some_and(|product| product.remove_telemetry_machine_id());
    let first_session_date = storage_service.get(FIRST_SESSION_DATE_STORAGE_KEY, StorageScope::Application, None);
    let last_session_date = storage_service.get(LAST_SESSION_DATE_STORAGE_KEY, StorageScope::Application, None);

    let machine_id = if !remove_machine_id {
        match storage_service.get(MACHINE_ID_KEY, StorageScope::Application, None).filter(|id| !id.is_empty()) {
            Some(id) => id,
            None => {
                let id = generate_uuid();
                storage_service.store(MACHINE_ID_KEY, id.as_str().into(), StorageScope::Application, StorageTarget::Machine);
                id
            }
        }
    } else {
        format!("Redacted-{}", product_identifier.unwrap_or("web"))
    };

    let platform = Platform::detect();
    let mut result = CommonProperties { entries: Vec::new(), sequence: Cell::new(0), start_time: runtime::date_now() };
    let last_session_date = last_session_date.filter(|date| !date.is_empty());

    // Nota do original: na web as datas de sessão vêm do armazenamento do navegador, então valem para um
    // navegador e não para a máquina inteira.
    result.set("common.firstSessionDate", optional_text(first_session_date.as_deref()));
    result.set("common.lastSessionDate", text(last_session_date.as_deref().unwrap_or("")));
    result.set("common.isNewSession", text(if last_session_date.is_none() { "1" } else { "0" }));
    let remote_authority = environment_service.remote_authority();
    let authoring_config = product_service.and_then(|product| product.remote_authoring_config());
    result.set("common.remoteAuthority", text(&clean_remote_authority(remote_authority.as_deref(), authoring_config.as_ref())));
    result.set("common.machineId", text(&machine_id));
    result.set("sessionID", text(&format!("{}{}", generate_uuid(), runtime::date_now())));
    result.set("commitHash", optional_text(commit));
    result.set("version", optional_text(version));
    result.set("common.platform", text(platform.kind.name()));
    result.set("common.product", text(product_identifier.unwrap_or("web")));
    result.set("common.userAgent", platform.user_agent.as_deref().map_or(Dynamic::Undefined, |agent| text(&clean_user_agent(agent))));
    result.set("common.isTouchDevice", text("false"));

    if is_internal_telemetry {
        result.set("common.msftInternal", Dynamic::Bool(is_internal_telemetry));
    }

    if let Some(resolve) = resolve_additional_properties {
        for (key, value) in resolve() {
            result.set(&key, value);
        }
    }

    if environment_service.is_sessions_window() {
        result.set("common.isAgentsWindow", Dynamic::Bool(true));
    }

    result
}
