//! Porte de `src/vs/workbench/api/common/extHostTelemetry.ts`: o lado da extensão da telemetria
//! (`vscode.env.createTelemetryLogger`, `isTelemetryEnabled` e `onDidChangeTelemetryEnabled`).
//!
//! Divergências:
//! - `IExtHostInitDataService` (que o original recebe por injeção) é o struct [`ExtHostTelemetryInitData`], só com o
//!   que `extHostTelemetry.ts` lê (`environment.isExtensionTelemetryLoggingOnly`, `environment.appHost`,
//!   `environment.isSessionsWindow`, `telemetryInfo`, `remote`, `commit`, `version`, `date`, `uiKind`). Os campos
//!   que o original deixa `undefined` (o `mock` do teste não os preenche) são `Option`.
//! - `ILoggerService` é o [`LoggerService`] da plataforma; o `ILogger` de saída é o [`LoggerRef`] que ele cria.
//!   `localize('extensionTelemetryLog', ...)` é [`localize`].
//! - `Record<string, any>` (dados de evento, propriedades comuns) é [`Dynamic`]. `vscode.TelemetrySender` é o trait
//!   [`TelemetrySender`] (o `flush` opcional é `has_flush` mais `flush`, e o `flush` que lança devolve `Err`), e
//!   [`ExtHostTelemetryLogger::validate_sender`] faz as checagens de `typeof` do original sobre um [`Dynamic`].
//! - O objeto de API `vscode.TelemetryLogger` (frozen, com `bind`) é o próprio `Rc<ExtHostTelemetryLogger>`; o
//!   `logError(Error | string, data)` são dois métodos ([`log_error_event`](ExtHostTelemetryLogger::log_error_event) e
//!   [`log_error_exception`](ExtHostTelemetryLogger::log_error_exception)). O `Promise.resolve(flush())` da
//!   `dispose` não é aguardado. O `dispose` que relança o erro do `flush` levanta pânico com o [`Error`], depois de
//!   rodar o `finally` do original.
//! - A `cause` de um erro é limpa recursivamente com o mesmo `cleanData` (o original limpa o objeto-causa, que para
//!   uma instância de `Error` não tem propriedades enumeráveis).
//! - `Reflect.get(extensionTelemetry, '_telemetryLoggers')` do teste é [`ExtHostTelemetry::telemetry_logger_count`].
//! - O `Map` de loggers é uma lista de pares na ordem de inserção.
//! - `ExtHostTelemetry extends Disposable` é um campo [`Disposable`], e o `$` dos métodos de RPC é `rpc_`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::objects::mixin;
use je_base::promise::Promise;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use je_platform::telemetry::{TelemetryLevel, clean_data, clean_remote_authority, telemetry_log_group};

use super::ext_host_protocol::{ExtHostTelemetryShape, IExtHostTelemetry, TelemetryProductConfig};
use crate::contrib::update::update_utils::try_parse_date;

/// `UIKind` de `extensionHostProtocol.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UIKind {
    Desktop = 1,
    Web = 2,
}

/// O `environment` de `IExtHostInitDataService` que a telemetria lê.
#[derive(Clone, Debug, Default)]
pub struct ExtHostTelemetryEnvironment {
    pub is_extension_telemetry_logging_only: bool,
    pub app_host: String,
    pub is_sessions_window: bool,
}

/// O `telemetryInfo` de `IExtHostInitDataService`.
#[derive(Clone, Debug, Default)]
pub struct ExtHostTelemetryInfo {
    pub first_session_date: String,
    pub session_id: String,
    pub machine_id: String,
    pub sqm_id: Option<String>,
    pub dev_device_id: Option<String>,
}

/// O `remote` de `IExtHostInitDataService`.
#[derive(Clone, Debug, Default)]
pub struct ExtHostTelemetryRemote {
    pub authority: Option<String>,
    pub is_remote: bool,
}

/// O que `ExtHostTelemetry` lê de `IExtHostInitDataService`.
#[derive(Clone, Debug, Default)]
pub struct ExtHostTelemetryInitData {
    pub environment: ExtHostTelemetryEnvironment,
    pub telemetry_info: ExtHostTelemetryInfo,
    pub remote: ExtHostTelemetryRemote,
    pub commit: Option<String>,
    pub version: Option<String>,
    pub date: Option<String>,
    pub ui_kind: Option<UIKind>,
}

/// `vscode.TelemetryConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryConfigurationDetails {
    pub is_crash_enabled: bool,
    pub is_errors_enabled: bool,
    pub is_usage_enabled: bool,
}

/// `vscode.TelemetryLoggerOptions`.
#[derive(Clone, Default)]
pub struct TelemetryLoggerOptions {
    pub ignore_built_in_common_properties: Option<bool>,
    pub ignore_unhandled_errors: Option<bool>,
    pub additional_common_properties: Option<Dynamic>,
}

/// `vscode.TelemetrySender`.
pub trait TelemetrySender {
    fn send_event_data(&self, event_name: &str, data: &Dynamic);
    fn send_error_data(&self, error: &Error, data: &Dynamic);
    /// Se o remetente declara `flush` (opcional na API).
    fn has_flush(&self) -> bool {
        false
    }
    /// `flush()`: `Err` quando a função lança.
    fn flush(&self) -> Result<Promise<()>, Error> {
        Ok(Promise::resolved(()))
    }
}

/// O `{ isUsageEnabled, isErrorsEnabled }` guardado por logger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryEnablements {
    pub is_usage_enabled: bool,
    pub is_errors_enabled: bool,
}

pub struct ExtHostTelemetry {
    this: Weak<ExtHostTelemetry>,
    base: Rc<Disposable>,
    init_data: ExtHostTelemetryInitData,
    on_did_change_telemetry_enabled: Rc<Emitter<bool>>,
    on_did_change_telemetry_configuration: Rc<Emitter<TelemetryConfigurationDetails>>,
    product_config: Cell<TelemetryProductConfig>,
    level: Cell<TelemetryLevel>,
    in_logging_only_mode: bool,
    output_logger: LoggerRef,
    telemetry_loggers: RefCell<Vec<(String, Vec<Rc<ExtHostTelemetryLogger>>)>>,
}

impl ExtHostTelemetry {
    pub fn new(is_worker: bool, init_data: ExtHostTelemetryInitData, logger_service: &dyn LoggerService) -> Rc<Self> {
        let base = Disposable::new();
        let in_logging_only_mode = init_data.environment.is_extension_telemetry_logging_only;
        let id = if init_data.remote.is_remote {
            "remoteExtHostTelemetry"
        } else if is_worker {
            "workerExtHostTelemetry"
        } else {
            "extHostTelemetry"
        };
        let (group_id, group_name) = telemetry_log_group();
        let output_logger = logger_service.create_logger(
            ResourceOrId::Id(id.to_owned()),
            Some(LoggerOptions {
                name: Some(localize("extensionTelemetryLog", "Extension Telemetry{0}", &[if in_logging_only_mode { " (Not Sent)" } else { "" }])),
                hidden: Some(true),
                group: Some(je_platform::log::LoggerGroup { id: group_id, name: group_name }),
                ..LoggerOptions::default()
            }),
        );
        base.register(output_logger.clone());
        let on_did_change_telemetry_enabled: Rc<Emitter<bool>> = Emitter::new();
        base.register(on_did_change_telemetry_enabled.clone());
        let on_did_change_telemetry_configuration: Rc<Emitter<TelemetryConfigurationDetails>> = Emitter::new();
        base.register(on_did_change_telemetry_configuration.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            init_data,
            on_did_change_telemetry_enabled,
            on_did_change_telemetry_configuration,
            product_config: Cell::new(TelemetryProductConfig { usage: true, error: true }),
            level: Cell::new(TelemetryLevel::None),
            in_logging_only_mode,
            output_logger,
            telemetry_loggers: RefCell::new(Vec::new()),
        })
    }

    /// `onDidChangeTelemetryEnabled`.
    pub fn on_did_change_telemetry_enabled(&self) -> Event<bool> {
        self.on_did_change_telemetry_enabled.event()
    }

    /// `onDidChangeTelemetryConfiguration`.
    pub fn on_did_change_telemetry_configuration(&self) -> Event<TelemetryConfigurationDetails> {
        self.on_did_change_telemetry_configuration.event()
    }

    pub fn get_telemetry_configuration(&self) -> bool {
        self.level.get() == TelemetryLevel::Usage
    }

    pub fn get_telemetry_details(&self) -> TelemetryConfigurationDetails {
        let level = self.level.get();
        let product_config = self.product_config.get();
        TelemetryConfigurationDetails {
            is_crash_enabled: level >= TelemetryLevel::Crash,
            is_errors_enabled: if product_config.error { level >= TelemetryLevel::Error } else { false },
            is_usage_enabled: if product_config.usage { level >= TelemetryLevel::Usage } else { false },
        }
    }

    pub fn instantiate_logger(&self, extension: &ExtensionDescription, sender: Rc<dyn TelemetrySender>, options: Option<TelemetryLoggerOptions>) -> Rc<ExtHostTelemetryLogger> {
        let telemetry_details = self.get_telemetry_details();
        let extension_id = extension.identifier.value.clone();
        let host = self.this.clone();
        let id_for_removal = extension_id.clone();
        let logger = ExtHostTelemetryLogger::new(LoggerInit {
            sender,
            options,
            extension: extension.clone(),
            logger: self.output_logger.clone(),
            in_logging_only_mode: self.in_logging_only_mode,
            common_properties: self.get_built_in_common_properties(extension),
            telemetry_enablements: TelemetryEnablements {
                is_usage_enabled: telemetry_details.is_usage_enabled,
                is_errors_enabled: telemetry_details.is_errors_enabled,
            },
            on_did_dispose: Box::new(move |logger| {
                if let Some(host) = host.upgrade() {
                    host.remove_telemetry_logger(&id_for_removal, logger);
                }
            }),
        });
        let mut loggers = self.telemetry_loggers.borrow_mut();
        match loggers.iter_mut().find(|(key, _)| *key == extension_id) {
            Some((_, existing)) => existing.push(logger.clone()),
            None => loggers.push((extension_id, vec![logger.clone()])),
        }
        drop(loggers);
        logger.api_telemetry_logger()
    }

    fn remove_telemetry_logger(&self, extension_id: &str, logger: &Rc<ExtHostTelemetryLogger>) {
        let mut loggers = self.telemetry_loggers.borrow_mut();
        let Some(position) = loggers.iter().position(|(key, _)| key == extension_id) else {
            return;
        };
        let remaining: Vec<_> = loggers[position].1.iter().filter(|candidate| !Rc::ptr_eq(candidate, logger)).cloned().collect();
        if remaining.is_empty() {
            loggers.remove(position);
        } else {
            loggers[position].1 = remaining;
        }
    }

    /// Quantos loggers a extensão tem registrados, ou `None` se a chave não existe (o `get(id)?.length` e o `has(id)` do
    /// teste, que lê o campo privado `_telemetryLoggers`).
    pub fn telemetry_logger_count(&self, extension_id: &str) -> Option<usize> {
        self.telemetry_loggers.borrow().iter().find(|(key, _)| key == extension_id).map(|(_, loggers)| loggers.len())
    }

    pub fn get_built_in_common_properties(&self, extension: &ExtensionDescription) -> Dynamic {
        let init_data = &self.init_data;
        let string = |value: &str| Dynamic::string(value);
        let optional = |value: &Option<String>| value.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
        let manifest = &extension.manifest;
        // Dúvida do original: informações de sistema operacional, arquitetura etc. não existem aqui; as extensões nativas as misturam.
        let mut entries: Vec<(&str, Dynamic)> = vec![
            ("common.extname", Dynamic::string(&format!("{}.{}", manifest.publisher, manifest.name))),
            ("common.extversion", string(&manifest.version)),
            ("common.vscodemachineid", string(&init_data.telemetry_info.machine_id)),
            ("common.vscodesessionid", string(&init_data.telemetry_info.session_id)),
            ("common.vscodecommithash", optional(&init_data.commit)),
            ("common.sqmid", optional(&init_data.telemetry_info.sqm_id)),
            (
                "common.devDeviceId",
                init_data
                    .telemetry_info
                    .dev_device_id
                    .as_deref()
                    .map_or_else(|| string(&init_data.telemetry_info.machine_id), Dynamic::string),
            ),
            ("common.vscodeversion", optional(&init_data.version)),
            ("common.vscodereleasedate", optional(&init_data.date)),
            ("common.isnewappinstall", Dynamic::Bool(is_new_app_install(&init_data.telemetry_info.first_session_date))),
            ("common.product", string(&init_data.environment.app_host)),
        ];
        entries.push((
            "common.uikind",
            string(match init_data.ui_kind {
                Some(UIKind::Web) => "web",
                Some(UIKind::Desktop) => "desktop",
                None => "unknown",
            }),
        ));
        entries.push(("common.remotename", Dynamic::string(&clean_remote_authority(init_data.remote.authority.as_deref(), None))));

        if init_data.environment.is_sessions_window {
            // __GDPR__COMMON__ "common.isAgentsWindow" : { "classification": "SystemMetaData", "purpose": "FeatureInsight" }
            entries.push(("common.isAgentsWindow", Dynamic::Bool(true)));
        }

        Dynamic::object(entries)
    }

    pub fn on_extension_error(&self, extension: &ExtensionIdentifier, error: &Error) -> bool {
        let non_disposed: Option<Vec<Rc<ExtHostTelemetryLogger>>> = self
            .telemetry_loggers
            .borrow()
            .iter()
            .find(|(key, _)| *key == extension.value)
            .map(|(_, loggers)| loggers.iter().filter(|logger| !logger.is_disposed()).cloned().collect());
        let Some(non_disposed) = non_disposed else {
            self.telemetry_loggers.borrow_mut().retain(|(key, _)| *key != extension.value);
            return false;
        };
        let mut error_emitted = false;
        for logger in non_disposed {
            if logger.ignore_unhandled_ext_host_errors {
                continue;
            }
            logger.log_error_exception(error, None);
            error_emitted = true;
        }
        error_emitted
    }
}

impl IDisposable for ExtHostTelemetry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ExtHostTelemetryShape for ExtHostTelemetry {
    fn rpc_initialize_telemetry_level(&self, level: TelemetryLevel, _supports_telemetry: bool, product_config: Option<TelemetryProductConfig>) {
        self.level.set(level);
        self.product_config.set(product_config.unwrap_or(TelemetryProductConfig { usage: true, error: true }));
    }

    fn rpc_on_did_change_telemetry_level(&self, level: TelemetryLevel) {
        let old_telemetry_enablement = self.get_telemetry_configuration();
        self.level.set(level);
        let telemetry_details = self.get_telemetry_details();
        // Remove all disposed loggers
        {
            let mut loggers = self.telemetry_loggers.borrow_mut();
            for (_, list) in loggers.iter_mut() {
                list.retain(|logger| !logger.is_disposed());
            }
            loggers.retain(|(_, list)| !list.is_empty());
        }
        // Loop through all loggers and update their level. A cópia isola a iteração das trocas que os ouvintes fazem
        let snapshot: Vec<Vec<Rc<ExtHostTelemetryLogger>>> = self.telemetry_loggers.borrow().iter().map(|(_, loggers)| loggers.clone()).collect();
        for loggers in snapshot {
            for logger in loggers {
                logger.update_telemetry_enablements(telemetry_details.is_usage_enabled, telemetry_details.is_errors_enabled);
            }
        }

        if old_telemetry_enablement != self.get_telemetry_configuration() {
            self.on_did_change_telemetry_enabled.fire(self.get_telemetry_configuration());
        }
        self.on_did_change_telemetry_configuration.fire(self.get_telemetry_details());
    }
}

impl IExtHostTelemetry for ExtHostTelemetry {
    fn on_extension_error(&self, extension: &ExtensionIdentifier, error: &Error) -> bool {
        ExtHostTelemetry::on_extension_error(self, extension, error)
    }
}

/// Os argumentos do construtor de [`ExtHostTelemetryLogger`].
struct LoggerInit {
    sender: Rc<dyn TelemetrySender>,
    options: Option<TelemetryLoggerOptions>,
    extension: ExtensionDescription,
    logger: LoggerRef,
    in_logging_only_mode: bool,
    common_properties: Dynamic,
    telemetry_enablements: TelemetryEnablements,
    on_did_dispose: Box<dyn Fn(&Rc<ExtHostTelemetryLogger>)>,
}

pub struct ExtHostTelemetryLogger {
    this: Weak<ExtHostTelemetryLogger>,
    on_did_change_enable_states: Rc<Emitter<Rc<ExtHostTelemetryLogger>>>,
    ignore_builtin_common_properties: bool,
    additional_common_properties: Option<Dynamic>,
    pub ignore_unhandled_ext_host_errors: bool,
    telemetry_enablements: Cell<TelemetryEnablements>,
    api_object_created: Cell<bool>,
    sender: RefCell<Option<Rc<dyn TelemetrySender>>>,
    extension: ExtensionDescription,
    logger: LoggerRef,
    in_logging_only_mode: bool,
    common_properties: Dynamic,
    on_did_dispose: Box<dyn Fn(&Rc<ExtHostTelemetryLogger>)>,
}

/// `Error` do JavaScript sem propriedades enumeráveis: o `cleanData` das mensagens vive no [`Dynamic`] da cópia.
fn clean_error(error: &Error) -> Error {
    let mut entries = vec![("name", Dynamic::string(&error.name)), ("message", Dynamic::string(&error.message))];
    if let Some(stack) = &error.stack {
        entries.push(("stack", Dynamic::string(stack)));
    }
    let cleaned = clean_data(Some(&Dynamic::object(entries)), &[], None, &[]);
    let text = |key: &str| match cleaned.get_str(key) {
        Dynamic::String(value) => Some(value.to_string_lossy()),
        _ => None,
    };
    let mut result = Error::named(text("name").unwrap_or_else(|| "unknown".to_owned()), text("message").unwrap_or_default());
    result.stack = text("stack");
    result.cause = error.cause.as_ref().map(|cause| Box::new(clean_error(cause)));
    result
}

impl ExtHostTelemetryLogger {
    pub fn validate_sender(sender: &Dynamic) -> Result<(), Error> {
        let type_error = |message: &str| Err(Error::named("TypeError", message));
        if sender.type_of() != "object" {
            return type_error("TelemetrySender argument is invalid");
        }
        if sender.is_null() {
            return type_error("Cannot read properties of null (reading 'sendEventData')");
        }
        if sender.get_str("sendEventData").type_of() != "function" {
            return type_error("TelemetrySender.sendEventData must be a function");
        }
        if sender.get_str("sendErrorData").type_of() != "function" {
            return type_error("TelemetrySender.sendErrorData must be a function");
        }
        let flush = sender.get_str("flush");
        if !flush.is_undefined() && flush.type_of() != "function" {
            return type_error("TelemetrySender.flush must be a function or undefined");
        }
        Ok(())
    }

    fn new(init: LoggerInit) -> Rc<Self> {
        let options = init.options.unwrap_or_default();
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            on_did_change_enable_states: Emitter::new(),
            ignore_builtin_common_properties: options.ignore_built_in_common_properties.unwrap_or(false),
            additional_common_properties: options.additional_common_properties,
            ignore_unhandled_ext_host_errors: options.ignore_unhandled_errors.unwrap_or(false),
            telemetry_enablements: Cell::new(init.telemetry_enablements),
            api_object_created: Cell::new(false),
            sender: RefCell::new(Some(init.sender)),
            extension: init.extension,
            logger: init.logger,
            in_logging_only_mode: init.in_logging_only_mode,
            common_properties: init.common_properties,
            on_did_dispose: init.on_did_dispose,
        })
    }

    pub fn update_telemetry_enablements(&self, is_usage_enabled: bool, is_errors_enabled: bool) {
        if self.api_object_created.get() {
            self.telemetry_enablements.set(TelemetryEnablements { is_usage_enabled, is_errors_enabled });
            if let Some(this) = self.this.upgrade() {
                self.on_did_change_enable_states.fire(this);
            }
        }
    }

    pub fn mix_in_common_props_and_clean_data(&self, data: Dynamic) -> Dynamic {
        // Some telemetry modules prefer to break properties and measurmements up
        // We mix common properties into the properties tab.
        let properties = data.get_str("properties");
        let has_properties = properties.is_truthy();
        let mut updated_data = if has_properties { properties } else { data.clone() };

        // We don't clean measurements since they are just numbers
        updated_data = clean_data(Some(&updated_data), &[], None, &[]);

        if let Some(additional) = &self.additional_common_properties {
            updated_data = mixin(&updated_data, additional, true);
        }

        if !self.ignore_builtin_common_properties {
            updated_data = mixin(&updated_data, &self.common_properties, true);
        }

        if has_properties {
            if let Dynamic::Object(object) = &data {
                object.set(&JsString::from("properties"), updated_data);
            }
            data
        } else {
            updated_data
        }
    }

    fn log_event(&self, event_name: &str, data: Option<&Dynamic>) {
        // No sender means likely disposed of, we should no-op
        let Some(sender) = self.sender.borrow().clone() else {
            return;
        };
        // If it's a built-in extension (vscode publisher) we don't prefix the publisher and only the ext name
        let event_name = if self.extension.manifest.publisher == "vscode" {
            format!("{}/{event_name}", self.extension.manifest.name)
        } else {
            format!("{}/{event_name}", self.extension.identifier.value)
        };
        let data = self.mix_in_common_props_and_clean_data(data.cloned().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new())));
        if !self.in_logging_only_mode {
            sender.send_event_data(&event_name, &data);
        }
        self.logger.trace_args(&event_name, &[LogArg::Value(data)]);
    }

    pub fn log_usage(&self, event_name: &str, data: Option<&Dynamic>) {
        if !self.telemetry_enablements.get().is_usage_enabled {
            return;
        }
        self.log_event(event_name, data);
    }

    /// `logError(eventName: string, data?)`.
    pub fn log_error_event(&self, event_name: &str, data: Option<&Dynamic>) {
        if !self.telemetry_enablements.get().is_errors_enabled || self.sender.borrow().is_none() {
            return;
        }
        self.log_event(event_name, data);
    }

    /// `logError(exception: Error, data?)`.
    pub fn log_error_exception(&self, exception: &Error, data: Option<&Dynamic>) {
        let Some(sender) = self.sender.borrow().clone() else {
            return;
        };
        if !self.telemetry_enablements.get().is_errors_enabled {
            return;
        }
        let cleaned_error = clean_error(exception);
        let data = self.mix_in_common_props_and_clean_data(data.cloned().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new())));
        if !self.in_logging_only_mode {
            sender.send_error_data(&cleaned_error, &data);
        }
        self.logger.trace_args("exception", &[LogArg::Value(data)]);
    }

    pub fn is_usage_enabled(&self) -> bool {
        self.telemetry_enablements.get().is_usage_enabled
    }

    pub fn is_errors_enabled(&self) -> bool {
        self.telemetry_enablements.get().is_errors_enabled
    }

    /// `onDidChangeEnableStates`.
    pub fn on_did_change_enable_states(&self) -> Event<Rc<ExtHostTelemetryLogger>> {
        self.on_did_change_enable_states.event()
    }

    /// `apiTelemetryLogger`: o objeto de API (aqui o próprio logger), criado sob demanda.
    pub fn api_telemetry_logger(&self) -> Rc<ExtHostTelemetryLogger> {
        self.api_object_created.set(true);
        self.this.upgrade().expect("o logger está vivo enquanto é usado")
    }

    pub fn is_disposed(&self) -> bool {
        self.sender.borrow().is_none()
    }
}

impl IDisposable for ExtHostTelemetryLogger {
    fn dispose(&self) {
        let was_disposed = self.is_disposed();
        let mut flush_error = None;
        let sender = self.sender.borrow().clone();
        match sender {
            Some(temp_sender) if temp_sender.has_flush() => {
                *self.sender.borrow_mut() = None;
                self.api_object_created.set(false);
                // `Promise.resolve(tempSender.flush!())`: a promessa não é aguardada
                if let Err(error) = temp_sender.flush() {
                    flush_error = Some(error);
                }
            }
            _ => *self.sender.borrow_mut() = None,
        }
        // `finally`
        self.on_did_change_enable_states.dispose();
        if !was_disposed && let Some(this) = self.this.upgrade() {
            (self.on_did_dispose)(&this);
        }
        if let Some(error) = flush_error {
            errors::throw(error);
        }
    }
}

pub fn is_new_app_install(first_session_date: &str) -> bool {
    let Some(first) = try_parse_date(Some(first_session_date)) else {
        return false;
    };
    let install_age = je_base::runtime::date_now() as f64 - first;
    install_age < 1000.0 * 60.0 * 60.0 * 24.0 // install age is less than a day
}
