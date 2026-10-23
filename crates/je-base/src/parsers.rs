//! Porte de `src/vs/base/common/parsers.ts`: estado de validação e relator de problemas.

use std::cell::Cell;
use std::rc::Rc;

/// `ValidationState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationState {
    Ok = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
    Fatal = 4,
}

/// `ValidationStatus`: o estado só sobe (um valor menor que o atual é ignorado).
#[derive(Debug)]
pub struct ValidationStatus {
    state: Cell<ValidationState>,
}

impl Default for ValidationStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationStatus {
    pub fn new() -> Self {
        Self { state: Cell::new(ValidationState::Ok) }
    }

    pub fn state(&self) -> ValidationState {
        self.state.get()
    }

    /// O `set state` do original: só aceita um estado maior que o atual.
    pub fn set_state(&self, value: ValidationState) {
        if value > self.state.get() {
            self.state.set(value);
        }
    }

    pub fn is_ok(&self) -> bool {
        self.state.get() == ValidationState::Ok
    }

    pub fn is_fatal(&self) -> bool {
        self.state.get() == ValidationState::Fatal
    }
}

/// `IProblemReporter`.
pub trait ProblemReporter {
    fn info(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
    fn fatal(&self, message: &str);
    fn status(&self) -> &ValidationStatus;
}

/// `Parser`: a classe base abstrata, que repassa tudo ao relator de problemas.
pub struct Parser {
    problem_reporter: Rc<dyn ProblemReporter>,
}

impl Parser {
    pub fn new(problem_reporter: Rc<dyn ProblemReporter>) -> Self {
        Self { problem_reporter }
    }

    /// `reset`: o `status.state = OK` do original passa pelo setter, que nunca desce, então é um no-op
    /// (comportamento do original preservado).
    pub fn reset(&self) {
        self.problem_reporter.status().set_state(ValidationState::Ok);
    }

    pub fn problem_reporter(&self) -> &Rc<dyn ProblemReporter> {
        &self.problem_reporter
    }

    pub fn info(&self, message: &str) {
        self.problem_reporter.info(message);
    }

    pub fn warn(&self, message: &str) {
        self.problem_reporter.warn(message);
    }

    pub fn error(&self, message: &str) {
        self.problem_reporter.error(message);
    }

    pub fn fatal(&self, message: &str) {
        self.problem_reporter.fatal(message);
    }
}
