//! Porte de `src/vs/platform/telemetry/common/telemetryService.ts`: o `TelemetryService`, que limpa os dados
//! pessoais dos eventos, acrescenta as propriedades comuns e entrega o resultado aos appenders. No je-code o único
//! appender é o [`TelemetryLogAppender`](super::TelemetryLogAppender): nada é enviado a servidor algum.
//!
//! Divergências:
//! - `meteredConnectionService` não existe (`platform/meteredConnection` não está portado): nenhum evento é descartado
//!   por conexão cobrada.
//! - O registro das configurações `telemetry.telemetryLevel`, `telemetry.feedback.enabled` e `telemetry.enableTelemetry`
//!   (o fim de `telemetryService.ts`, com a política `TelemetryLevel` e o texto de ajuda em markdown) não é portado
//!   aqui: depende de `product.privacyStatementUrl`, do registro de política por categoria e de textos da Microsoft.
//! - `commonProperties` é um objeto dinâmico: o getter do original (`get bar()`) é uma leitura única do valor.
//! - O `setTimeout` de `waitForExperimentProperties` usa o relógio de [`je_base::runtime`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::objects;
use je_base::runtime::{self, TimerId};
use je_base::strings::escape_reg_exp_characters;
use zyarr::RegExp;

use super::telemetry::{
    CommonPropertyValue, ITelemetryData, ITelemetryService, TELEMETRY_CRASH_REPORTER_SETTING_ID, TELEMETRY_OLD_SETTING_ID, TELEMETRY_SETTING_ID, TelemetryLevel,
};
use super::telemetry_utils::{ITelemetryAppender, TelemetryTrustedValue, clean_data, get_telemetry_level_of};
use crate::configuration::service::IConfigurationService;
use crate::product::IProductService;

/// `ITelemetryServiceConfig`.
#[derive(Default)]
pub struct TelemetryServiceConfig {
    pub appenders: Vec<Rc<dyn ITelemetryAppender>>,
    pub send_error_telemetry: bool,
    /// `commonProperties`: um objeto dinâmico (`sessionID`, `common.machineId`...).
    pub common_properties: Option<Dynamic>,
    pub pii_paths: Vec<String>,
    /// Caminhos removidos como os de `pii_paths`, mas cujos descendentes são redigidos por inteiro em vez de ficarem
    /// relativos a eles.
    pub user_content_paths: Vec<String>,
    /// Um nível fixo, para processos que recebem o nível já resolvido do lançador. Com ele, o serviço não lê nem observa
    /// as configurações de telemetria.
    pub telemetry_level: Option<TelemetryLevel>,
    /// Se `true`, os eventos ficam em fila até `set_experiment_property` ser chamado (até 10 segundos), para que o
    /// contexto de experimento chegue em todos.
    pub wait_for_experiment_properties: bool,
}

struct PendingEvent {
    event_name: String,
    event_level: TelemetryLevel,
    data: Option<Dynamic>,
}

/// `TelemetryService`.
pub struct TelemetryService {
    session_id: String,
    machine_id: String,
    sqm_id: String,
    dev_device_id: String,
    first_session_date: String,
    appenders: Vec<Rc<dyn ITelemetryAppender>>,
    common_properties: Dynamic,
    experiment_properties: Dynamic,
    telemetry_level: Cell<TelemetryLevel>,
    send_error_telemetry: Cell<bool>,
    product_service: Rc<dyn IProductService>,
    pending_events: RefCell<Vec<PendingEvent>>,
    is_experiment_property_set: Cell<bool>,
    flush_timeout: Cell<Option<TimerId>>,
    disposables: Rc<DisposableStore>,
    cleanup_patterns: Vec<RegExp>,
    relative_path_root_patterns: Vec<RegExp>,
    user_content_root_patterns: Vec<RegExp>,
}

impl TelemetryService {
    pub const IDLE_START_EVENT_NAME: &'static str = "UserIdleStart";
    pub const IDLE_STOP_EVENT_NAME: &'static str = "UserIdleStop";
    const BUFFER_FLUSH_TIMEOUT: u64 = 10_000; // 10 seconds
    const MAX_BUFFER_SIZE: usize = 1000;

    /// `createWithLevel`: o serviço com nível fixo, sem serviço de configuração.
    pub fn create_with_level(config: TelemetryServiceConfig, level: TelemetryLevel, product_service: Rc<dyn IProductService>) -> Rc<Self> {
        Self::build(TelemetryServiceConfig { telemetry_level: Some(level), ..config }, None, product_service).expect("o nível fixo dispensa a configuração")
    }

    /// `new TelemetryService(config, configurationService, productService)`. Sem `telemetry_level` na configuração o
    /// serviço de configuração é obrigatório (o original lança).
    pub fn new(config: TelemetryServiceConfig, configuration_service: Option<Rc<dyn IConfigurationService>>, product_service: Rc<dyn IProductService>) -> Result<Rc<Self>, Error> {
        Self::build(config, configuration_service, product_service)
    }

    fn build(config: TelemetryServiceConfig, configuration_service: Option<Rc<dyn IConfigurationService>>, product_service: Rc<dyn IProductService>) -> Result<Rc<Self>, Error> {
        let common_properties = config.common_properties.clone().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()));
        let read = |key: &str| match common_properties.get(&JsString::from(key)) {
            Dynamic::String(text) => text.to_string_lossy(),
            _ => String::new(),
        };

        // static cleanup pattern for: `vscode-file:///DANGEROUS/PATH/resources/app/Useful/Information`
        let app_resources_pattern = || RegExp::from_str(r"(vscode-)?file:\/\/.*?\/resources\/app\/", "gi").expect("expressão fixa");
        let mut cleanup_patterns = vec![app_resources_pattern()];
        let mut relative_path_root_patterns = vec![app_resources_pattern()];
        let mut user_content_root_patterns = Vec::new();
        let mut distinct: Vec<&String> = Vec::new();
        for path in config.pii_paths.iter().chain(config.user_content_paths.iter()) {
            if !distinct.contains(&path) {
                distinct.push(path);
            }
        }
        for pii_path in distinct {
            let pattern = |text: &str| {
                let escaped = escape_reg_exp_characters(&text.encode_utf16().collect::<Vec<_>>()).to_string_lossy();
                RegExp::from_str(&escaped, "gi").expect("o texto escapado é uma expressão válida")
            };
            let make_patterns = || {
                let mut patterns = vec![pattern(pii_path)];
                if pii_path.contains('\\') {
                    patterns.push(pattern(&pii_path.replace('\\', "/")));
                }
                patterns
            };
            cleanup_patterns.extend(make_patterns());
            if config.user_content_paths.contains(pii_path) {
                user_content_root_patterns.extend(make_patterns());
            } else {
                relative_path_root_patterns.extend(make_patterns());
            }
        }

        let service = Rc::new(Self {
            session_id: read("sessionID"),
            machine_id: read("common.machineId"),
            sqm_id: read("common.sqmId"),
            dev_device_id: read("common.devDeviceId"),
            first_session_date: read("common.firstSessionDate"),
            appenders: config.appenders,
            common_properties,
            experiment_properties: Dynamic::object(Vec::<(&str, Dynamic)>::new()),
            telemetry_level: Cell::new(TelemetryLevel::Usage),
            send_error_telemetry: Cell::new(config.send_error_telemetry),
            product_service,
            pending_events: RefCell::new(Vec::new()),
            is_experiment_property_set: Cell::new(false),
            flush_timeout: Cell::new(None),
            disposables: DisposableStore::new(),
            cleanup_patterns,
            relative_path_root_patterns,
            user_content_root_patterns,
        });

        if let Some(level) = config.telemetry_level {
            service.update_telemetry_level(level);
        } else {
            let Some(configuration_service) = configuration_service else {
                return Err(Error::new("TelemetryService requires a configuration service or a fixed telemetry level."));
            };
            service.update_telemetry_level(get_telemetry_level_of(&*configuration_service));
            let weak = Rc::downgrade(&service);
            let observed = configuration_service.clone();
            service.disposables.add(configuration_service.on_did_change_configuration().listen(move |e| {
                // Check on the telemetry settings and update the state if changed
                let affects_telemetry_config = e.affects_configuration(TELEMETRY_SETTING_ID, None)
                    || e.affects_configuration(TELEMETRY_OLD_SETTING_ID, None)
                    || e.affects_configuration(TELEMETRY_CRASH_REPORTER_SETTING_ID, None);
                if affects_telemetry_config && let Some(this) = weak.upgrade() {
                    this.update_telemetry_level(get_telemetry_level_of(&*observed));
                }
            }));
        }

        // Buffer events until experiment properties are set (or timeout expires).
        // This ensures early events include experiment context when available.
        if config.wait_for_experiment_properties {
            let weak = Rc::downgrade(&service);
            service.flush_timeout.set(Some(runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.flush_pending_events();
                    }
                },
                Self::BUFFER_FLUSH_TIMEOUT,
            )));
        } else {
            service.is_experiment_property_set.set(true);
        }
        Ok(service)
    }

    fn flush_pending_events(&self) {
        if self.is_experiment_property_set.get() {
            return;
        }
        self.is_experiment_property_set.set(true);
        if let Some(timeout) = self.flush_timeout.take() {
            runtime::clear_timeout(timeout);
        }
        // Send all buffered events now that experiment properties are available
        let events = std::mem::take(&mut *self.pending_events.borrow_mut());
        for event in events {
            self.do_log(&event.event_name, event.event_level, event.data);
        }
    }

    fn update_telemetry_level(&self, level: TelemetryLevel) {
        let mut level = level;
        // Also ensure that error telemetry is respecting the product configuration for collectable telemetry
        if let Some(collectable) = self.product_service.enabled_telemetry_levels() {
            self.send_error_telemetry.set(if self.send_error_telemetry.get() { collectable.error } else { false });
            // Make sure the telemetry level from the service is the minimum of the config and product
            let max_collectable = if collectable.usage {
                TelemetryLevel::Usage
            } else if collectable.error {
                TelemetryLevel::Error
            } else {
                TelemetryLevel::None
            };
            level = level.min(max_collectable);
        }
        self.telemetry_level.set(level);
    }

    fn log(&self, event_name: &str, event_level: TelemetryLevel, data: Option<Dynamic>) {
        // don't send events when the user is optout
        if self.telemetry_level.get() < event_level {
            return;
        }

        // Buffer events until experiment properties are set (or timeout expires)
        if !self.is_experiment_property_set.get() {
            let mut pending = self.pending_events.borrow_mut();
            if pending.len() < Self::MAX_BUFFER_SIZE {
                pending.push(PendingEvent { event_name: event_name.to_owned(), event_level, data });
            }
            return;
        }

        self.do_log(event_name, event_level, data);
    }

    fn do_log(&self, event_name: &str, event_level: TelemetryLevel, data: Option<Dynamic>) {
        // add experiment properties
        let data = data.map(|d| objects::mixin(&d, &self.experiment_properties, true)).unwrap_or_else(|| objects::mixin(&Dynamic::Undefined, &self.experiment_properties, true));

        // remove all PII from data
        let data = clean_data(Some(&data), &self.cleanup_patterns, Some(&self.relative_path_root_patterns), &self.user_content_root_patterns);

        // add common properties
        let mut data = objects::mixin(&data, &self.common_properties, true);

        // tag error-level events so the backend can identify them generically
        if event_level == TelemetryLevel::Error {
            data = Dynamic::object(data.own_keys().into_iter().map(|key| (key.clone(), data.get(&key))).chain([(JsString::from("isError"), Dynamic::Bool(true))]));
        }

        // Log to the appenders of sufficient level
        for appender in &self.appenders {
            appender.log(event_name, &data);
        }
    }
}

/// O `ITelemetryData` como o objeto que o original manipula.
pub fn telemetry_data_to_dynamic(data: &ITelemetryData) -> Dynamic {
    let mut entries: Vec<(String, Dynamic)> = Vec::new();
    if let Some(from) = &data.from {
        entries.push(("from".to_owned(), Dynamic::string(from)));
    }
    if let Some(target) = &data.target {
        entries.push(("target".to_owned(), Dynamic::string(target)));
    }
    entries.extend(data.extra.iter().cloned());
    Dynamic::object(entries)
}

impl ITelemetryService for TelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        self.telemetry_level.get()
    }
    fn session_id(&self) -> String {
        self.session_id.clone()
    }
    fn machine_id(&self) -> String {
        self.machine_id.clone()
    }
    fn sqm_id(&self) -> String {
        self.sqm_id.clone()
    }
    fn dev_device_id(&self) -> String {
        self.dev_device_id.clone()
    }
    fn first_session_date(&self) -> String {
        self.first_session_date.clone()
    }
    /// Lido na hora: uma conta interna pode entrar depois de o serviço ser criado.
    fn msft_internal(&self) -> Option<bool> {
        match self.common_properties.get(&JsString::from("common.msftInternal")) {
            Dynamic::Bool(value) => Some(value),
            _ => None,
        }
    }
    fn send_error_telemetry(&self) -> bool {
        self.send_error_telemetry.get()
    }

    fn public_log(&self, event_name: &str, data: Option<ITelemetryData>) {
        self.log(event_name, TelemetryLevel::Usage, data.as_ref().map(telemetry_data_to_dynamic));
    }
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        self.log(event_name, TelemetryLevel::Usage, data);
    }
    fn public_log_error(&self, error_event_name: &str, data: Option<ITelemetryData>) {
        if !self.send_error_telemetry.get() {
            return;
        }
        // Send error event and anonymize paths
        self.log(error_event_name, TelemetryLevel::Error, data.as_ref().map(telemetry_data_to_dynamic));
    }
    fn public_log_error2(&self, event_name: &str, data: Option<Dynamic>) {
        if !self.send_error_telemetry.get() {
            return;
        }
        self.log(event_name, TelemetryLevel::Error, data);
    }

    fn set_experiment_property(&self, name: &str, value: &str) {
        if let Dynamic::Object(properties) = &self.experiment_properties {
            properties.set(&JsString::from(name), TelemetryTrustedValue { value: Dynamic::string(value) }.to_dynamic());
        }
        // On first call, flush all pending events that were buffered waiting for experiment properties
        if !self.is_experiment_property_set.get() {
            self.flush_pending_events();
        }
    }

    fn set_common_property(&self, name: &str, value: Option<CommonPropertyValue>) {
        let Dynamic::Object(properties) = &self.common_properties else { return };
        let key = JsString::from(name);
        match value {
            None => {
                properties.delete(&key);
            }
            Some(CommonPropertyValue::Text(text)) => properties.set(&key, Dynamic::string(&text)),
            Some(CommonPropertyValue::Bool(flag)) => properties.set(&key, Dynamic::Bool(flag)),
        }
    }
}

impl IDisposable for TelemetryService {
    fn dispose(&self) {
        // Flush any remaining pending events before disposing
        self.flush_pending_events();
        self.disposables.dispose();
    }
}
