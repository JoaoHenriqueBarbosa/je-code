//! O mínimo de `src/vs/platform/log/common/log.ts` de que a configuração precisa: o serviço que
//! recebe mensagens e o `NullLogService` que as descarta.
//!
//! TODO(integração): substituir pelo porte completo de `vs/platform/log` quando ele existir (níveis,
//! `Error` como argumento, `flush`).

use std::rc::Rc;

use je_base::event::Event;

/// `LogLevel` de `log.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Off,
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

/// `ILogService`, reduzido a mensagens de texto.
pub trait LogService {
    fn trace(&self, message: &str);
    fn debug(&self, message: &str);
    fn info(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);

    /// `getLevel()`.
    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    /// `onDidChangeLogLevel`.
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }
}

/// O handle compartilhado do serviço.
pub type LogServiceRef = Rc<dyn LogService>;

/// `NullLogService`: descarta tudo.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullLogService;

impl NullLogService {
    pub fn shared() -> LogServiceRef {
        Rc::new(Self)
    }
}

impl LogService for NullLogService {
    fn trace(&self, _message: &str) {}
    fn debug(&self, _message: &str) {}
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
}
