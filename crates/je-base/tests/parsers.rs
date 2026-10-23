//! Testes próprios de `src/vs/base/common/parsers.ts` (o VS Code não tem `parsers.test.ts`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::parsers::{Parser, ProblemReporter, ValidationState, ValidationStatus};

#[test]
fn status_state_only_goes_up() {
    let status = ValidationStatus::new();
    assert!(status.is_ok());
    status.set_state(ValidationState::Warning);
    assert_eq!(status.state(), ValidationState::Warning);
    status.set_state(ValidationState::Info);
    assert_eq!(status.state(), ValidationState::Warning);
    status.set_state(ValidationState::Ok);
    assert_eq!(status.state(), ValidationState::Warning);
    assert!(!status.is_ok() && !status.is_fatal());
    status.set_state(ValidationState::Fatal);
    assert!(status.is_fatal());
}

#[derive(Default)]
struct Reporter {
    log: RefCell<Vec<String>>,
    status: ValidationStatus,
}

impl ProblemReporter for Reporter {
    fn info(&self, message: &str) {
        self.log.borrow_mut().push(format!("info:{message}"));
        self.status.set_state(ValidationState::Info);
    }
    fn warn(&self, message: &str) {
        self.log.borrow_mut().push(format!("warn:{message}"));
        self.status.set_state(ValidationState::Warning);
    }
    fn error(&self, message: &str) {
        self.log.borrow_mut().push(format!("error:{message}"));
        self.status.set_state(ValidationState::Error);
    }
    fn fatal(&self, message: &str) {
        self.log.borrow_mut().push(format!("fatal:{message}"));
        self.status.set_state(ValidationState::Fatal);
    }
    fn status(&self) -> &ValidationStatus {
        &self.status
    }
}

#[test]
fn parser_forwards_to_the_reporter() {
    let reporter = Rc::new(Reporter::default());
    let parser = Parser::new(reporter.clone());
    parser.info("a");
    parser.warn("b");
    parser.error("c");
    parser.fatal("d");
    assert_eq!(*reporter.log.borrow(), vec!["info:a", "warn:b", "error:c", "fatal:d"]);
    assert!(reporter.status().is_fatal());
    // O `reset` do original passa pelo setter que nunca desce: o estado fica.
    parser.reset();
    assert!(reporter.status().is_fatal());
}
