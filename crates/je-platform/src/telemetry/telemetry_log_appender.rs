//! Porte de `src/vs/platform/telemetry/common/telemetryLogAppender.ts`: o appender que só escreve os eventos de
//! telemetria num logger (no nível `trace`), sem enviar nada a lugar algum. É o único appender do je-code.

use std::rc::Rc;

use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;

use super::telemetry_utils::{ITelemetryAppender, TELEMETRY_LOG_ID, is_logging_only, validate_telemetry_data};
use crate::environment::environment::EnvironmentService;
use crate::log::{LogArg, LoggerGroup, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use crate::product::IProductService;

/// O `string | { prefix, loggerId }` do primeiro argumento do construtor.
#[derive(Clone, Debug)]
pub enum TelemetryLogAppenderOptions {
    /// Só o prefixo: o id do logger é `telemetry`, ou `remoteTelemetry` quando `remote`.
    Prefix(String),
    Custom { prefix: String, logger_id: String },
}

impl From<&str> for TelemetryLogAppenderOptions {
    fn from(prefix: &str) -> Self {
        Self::Prefix(prefix.to_owned())
    }
}

/// `TelemetryLogAppender`.
pub struct TelemetryLogAppender {
    base: Rc<Disposable>,
    logger: LoggerRef,
    prefix: String,
}

impl TelemetryLogAppender {
    pub fn new(
        prefix_or_options: TelemetryLogAppenderOptions,
        remote: bool,
        logger_service: &dyn LoggerService,
        environment_service: &dyn EnvironmentService,
        product_service: &dyn IProductService,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let (prefix, id) = match prefix_or_options {
            TelemetryLogAppenderOptions::Prefix(prefix) => (prefix, if remote { "remoteTelemetry" } else { TELEMETRY_LOG_ID }.to_owned()),
            TelemetryLogAppenderOptions::Custom { prefix, logger_id } => (prefix, logger_id),
        };
        let resource_or_id = ResourceOrId::Id(id.clone());
        let logger = match logger_service.get_logger(&resource_or_id) {
            Some(logger) => logger,
            None => {
                // Not a perfect check, but a nice way to indicate if we only have logging enabled for debug purposes and nothing is actually being sent
                let just_logging_and_not_sending = is_logging_only(product_service, environment_service);
                let log_suffix = if just_logging_and_not_sending { " (Not Sent)" } else { "" };
                logger_service.create_logger(
                    resource_or_id,
                    Some(LoggerOptions {
                        name: Some(localize("telemetryLog", "Telemetry{0}", &[log_suffix])),
                        group: Some(LoggerGroup { id: TELEMETRY_LOG_ID.to_owned(), name: localize("telemetryLogName", "Telemetry", &[]) }),
                        hidden: Some(true),
                        ..LoggerOptions::default()
                    }),
                )
            }
        };
        // `this._register(logger)`: o appender descarta o logger com ele.
        base.register(logger_as_disposable(&logger));
        Rc::new(Self { base, logger, prefix })
    }
}

fn logger_as_disposable(logger: &LoggerRef) -> DisposableRef {
    let logger = logger.clone();
    je_base::lifecycle::to_disposable(move || logger.dispose())
}

impl ITelemetryAppender for TelemetryLogAppender {
    fn flush(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn log(&self, event_name: &str, data: &je_base::dynamic::Dynamic) {
        self.logger.trace_args(&format!("{}telemetry/{}", self.prefix, event_name), &[LogArg::Value(validate_telemetry_data(data).to_dynamic())]);
    }
}

impl IDisposable for TelemetryLogAppender {
    fn dispose(&self) {
        self.base.dispose();
    }
}
