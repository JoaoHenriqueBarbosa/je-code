//! Porte de `src/vs/workbench/contrib/editSessions/common/editSessionsLogService.ts`: o log "Cloud Changes".
//!
//! Divergência: `class EditSessionsLogService extends AbstractLogger` só repassa cada chamada ao logger que o `ILoggerService` cria
//! (`logs/editSessions.log`, grupo da janela). O `delegate_logger!` faz o repasse, inclusive do nível e do evento de mudança de nível,
//! que são os do logger interno (o `AbstractLogger` do original guardaria um nível próprio, que nada consulta, porque o logger interno
//! decide o que escreve), e do `dispose`, que descarta o logger (o `_register` do original).

use std::rc::Rc;

use je_base::nls::localize;
use je_platform::environment::environment::EnvironmentService;
use je_platform::log::{LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use je_platform::resources::join_path;

use super::common::{EDIT_SESSIONS_LOG_ID, IEditSessionsLogService};
use crate::services::log::log_constants::window_log_group;

/// `EditSessionsLogService`.
pub struct EditSessionsLogService {
    logger: LoggerRef,
}

impl EditSessionsLogService {
    pub fn new(logger_service: &dyn LoggerService, environment_service: &dyn EnvironmentService) -> Rc<Self> {
        let logger = logger_service.create_logger(
            ResourceOrId::from(join_path(&environment_service.logs_home(), &[&format!("{EDIT_SESSIONS_LOG_ID}.log")])),
            Some(LoggerOptions {
                id: Some(EDIT_SESSIONS_LOG_ID.to_string()),
                name: Some(localize("cloudChangesLog", "Cloud Changes", &[])),
                group: Some(window_log_group()),
                ..LoggerOptions::default()
            }),
        );
        Rc::new(Self { logger })
    }
}

je_platform::delegate_logger!(EditSessionsLogService, logger);

impl IEditSessionsLogService for EditSessionsLogService {}
