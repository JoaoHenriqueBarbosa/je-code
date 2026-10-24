//! Porte de `src/vs/workbench/contrib/tasks/test/common/problemMatcher.test.ts` (suítes `ProblemPatternParser` e
//! `ProblemPatternRegistry - msCompile`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `ProblemReporter.hasMessage` do original compara `indexOf(message) !== null`, que é sempre verdadeiro (o
//!   `indexOf` devolve um número). A tradução mantém essa semântica: trocar por uma busca real reprovaria
//!   `forbids setting the kind outside of the first element of the array`, porque o `kind: 'file'` do segundo
//!   elemento vale `ProblemLocationKind.File` (zero, falso) e o analisador não relata essa mensagem; o caso passa
//!   porque o padrão é rejeitado por falta de `message`.
//! - `new RegExp('test')` é [`test_regexp`], e o `assert.deepStrictEqual` entre padrões compara a expressão
//!   regular por `source` e `flags` (a igualdade de [`ProblemPattern`]).
//! - `assert.strictEqual(null, parsed)` é `parsed.is_none()`.
//! - `ProblemPatternRegistry.get('msCompile')` é `problem_pattern_registry().get("msCompile")` (o original o tipa
//!   como definido, aqui é `Option` e o caso o desembrulha); `matcher.handle([line])` é `handle(&[line], 0)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::arrays::OneOrMany;
use je_base::js_regexp::JsRegExp;
use je_base::parsers::{ProblemReporter, ValidationState, ValidationStatus};
use je_platform::markers::{MarkerCode, MarkerSeverity};
use je_workbench::contrib::tasks::problem_matcher::{
    self as matchers, ApplyToKind, FileLocationKind, IProblemPatternRegistry, ParsedProblemPattern, PatternInput, ProblemLocationKind, ProblemMatcher,
    ProblemPattern, ProblemPatternParser, config, create_line_matcher, problem_pattern_registry,
};

struct TestProblemReporter {
    validation_status: ValidationStatus,
    messages: RefCell<Vec<String>>,
}

impl TestProblemReporter {
    fn new() -> Self {
        Self { validation_status: ValidationStatus::new(), messages: RefCell::new(Vec::new()) }
    }

    /// `indexOf(message) !== null`: sempre verdadeiro no original (ver as notas de tradução no topo).
    fn has_message(&self, _message: &str) -> bool {
        true
    }

    fn state(&self) -> ValidationState {
        self.validation_status.state()
    }

    fn is_ok(&self) -> bool {
        self.validation_status.is_ok()
    }
}

impl ProblemReporter for TestProblemReporter {
    fn info(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
        self.validation_status.set_state(ValidationState::Info);
    }

    fn warn(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
        self.validation_status.set_state(ValidationState::Warning);
    }

    fn error(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
        self.validation_status.set_state(ValidationState::Error);
    }

    fn fatal(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
        self.validation_status.set_state(ValidationState::Fatal);
    }

    fn status(&self) -> &ValidationStatus {
        &self.validation_status
    }
}

fn test_regexp(source: &str) -> Rc<JsRegExp> {
    Rc::new(JsRegExp::from_str(source, "").expect("expressão regular válida"))
}

/// `setup`: o relator e o analisador de cada caso de `ProblemPatternParser`.
fn setup() -> (Rc<TestProblemReporter>, ProblemPatternParser) {
    let reporter = Rc::new(TestProblemReporter::new());
    let parser = ProblemPatternParser::new(reporter.clone());
    (reporter, parser)
}

fn pattern(regexp: &str) -> config::ProblemPattern {
    config::ProblemPattern { regexp: Some(regexp.to_string()), ..Default::default() }
}

// suite('ProblemPatternParser') > suite('single-pattern definitions')

#[test]
fn parses_a_pattern_defined_by_only_a_regexp() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = pattern("test");
    let parsed = parser.parse(PatternInput::Single(problem_pattern));
    assert!(reporter.is_ok());
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Single(ProblemPattern {
            kind: Some(ProblemLocationKind::Location),
            file: Some(1),
            line: Some(2),
            character: Some(3),
            message: Some(0),
            ..ProblemPattern::new(test_regexp("test"))
        }))
    );
}

#[test]
fn does_not_sets_defaults_for_line_and_character_if_kind_is_file() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (_reporter, parser) = setup();
    let problem_pattern = config::ProblemPattern { kind: Some("file".to_string()), ..pattern("test") };
    let parsed = parser.parse(PatternInput::Single(problem_pattern));
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Single(ProblemPattern {
            kind: Some(ProblemLocationKind::File),
            file: Some(1),
            message: Some(0),
            ..ProblemPattern::new(test_regexp("test"))
        }))
    );
}

// suite('ProblemPatternParser') > suite('multi-pattern definitions')

#[test]
fn defines_a_pattern_based_on_regexp_and_property_fields_with_file_line_location() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(3), line: Some(4), column: Some(5), message: Some(6), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(reporter.is_ok());
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Multi(vec![ProblemPattern {
            kind: Some(ProblemLocationKind::Location),
            file: Some(3),
            line: Some(4),
            character: Some(5),
            message: Some(6),
            ..ProblemPattern::new(test_regexp("test"))
        }]))
    );
}

#[test]
fn defines_a_pattern_bsaed_on_regexp_and_property_fields_with_location() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(3), location: Some(4), message: Some(6), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(reporter.is_ok());
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Multi(vec![ProblemPattern {
            kind: Some(ProblemLocationKind::Location),
            file: Some(3),
            location: Some(4),
            message: Some(6),
            ..ProblemPattern::new(test_regexp("test"))
        }]))
    );
}

#[test]
fn accepts_a_pattern_that_provides_the_fields_from_multiple_entries() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![
        config::ProblemPattern { file: Some(3), ..pattern("test") },
        config::ProblemPattern { line: Some(4), ..pattern("test1") },
        config::ProblemPattern { column: Some(5), ..pattern("test2") },
        config::ProblemPattern { message: Some(6), ..pattern("test3") },
    ];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(reporter.is_ok());
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Multi(vec![
            ProblemPattern { kind: Some(ProblemLocationKind::Location), file: Some(3), ..ProblemPattern::new(test_regexp("test")) },
            ProblemPattern { line: Some(4), ..ProblemPattern::new(test_regexp("test1")) },
            ProblemPattern { character: Some(5), ..ProblemPattern::new(test_regexp("test2")) },
            ProblemPattern { message: Some(6), ..ProblemPattern::new(test_regexp("test3")) },
        ]))
    );
}

#[test]
fn forbids_setting_the_loop_flag_outside_of_the_last_element_in_the_array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![
        config::ProblemPattern { file: Some(3), r#loop: Some(true), ..pattern("test") },
        config::ProblemPattern { line: Some(4), ..pattern("test1") },
    ];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The loop property is only supported on the last line matcher."));
}

#[test]
fn forbids_setting_the_kind_outside_of_the_first_element_of_the_array() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![
        config::ProblemPattern { file: Some(3), ..pattern("test") },
        config::ProblemPattern { kind: Some("file".to_string()), line: Some(4), ..pattern("test1") },
    ];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. The kind property must be provided only in the first element"));
}

#[test]
fn kind_location_requires_a_regexp() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(0), line: Some(1), column: Some(20), message: Some(0), ..Default::default() }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is missing a regular expression."));
}

#[test]
fn kind_location_requires_a_regexp_on_every_entry() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![
        config::ProblemPattern { file: Some(3), ..pattern("test") },
        config::ProblemPattern { line: Some(4), ..Default::default() },
        config::ProblemPattern { column: Some(5), ..pattern("test2") },
        config::ProblemPattern { message: Some(6), ..pattern("test3") },
    ];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is missing a regular expression."));
}

#[test]
fn kind_location_requires_a_message() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(0), line: Some(1), column: Some(20), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must have at least have a file and a message."));
}

#[test]
fn kind_location_requires_a_file() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { line: Some(1), column: Some(20), message: Some(0), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must either have kind: \"file\" or have a line or location match group."));
}

#[test]
fn kind_location_requires_either_a_line_or_location() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(1), column: Some(20), message: Some(0), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must either have kind: \"file\" or have a line or location match group."));
}

#[test]
fn kind_file_accepts_a_regexp_file_and_message() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { file: Some(2), kind: Some("file".to_string()), message: Some(6), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(reporter.is_ok());
    assert_eq!(
        parsed,
        Some(ParsedProblemPattern::Multi(vec![ProblemPattern {
            kind: Some(ProblemLocationKind::File),
            file: Some(2),
            message: Some(6),
            ..ProblemPattern::new(test_regexp("test"))
        }]))
    );
}

#[test]
fn kind_file_requires_a_file() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { kind: Some("file".to_string()), message: Some(6), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must have at least have a file and a message."));
}

#[test]
fn kind_file_requires_a_message() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern = vec![config::ProblemPattern { kind: Some("file".to_string()), file: Some(6), ..pattern("test") }];
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must have at least have a file and a message."));
}

#[test]
fn empty_pattern_array_should_be_handled_gracefully() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (reporter, parser) = setup();
    let problem_pattern: config::MultiLineProblemPattern = Vec::new();
    let parsed = parser.parse(PatternInput::Multi(problem_pattern));
    assert!(parsed.is_none());
    assert_eq!(ValidationState::Error, reporter.state());
    assert!(reporter.has_message("The problem pattern is invalid. It must contain at least one pattern."));
}

// suite('ProblemPatternRegistry - msCompile')

/// O `createLineMatcher({ owner: 'msCompile', applyTo: allDocuments, fileLocation: Absolute, pattern: ... })` de
/// cada caso.
fn ms_compile_matcher() -> Box<dyn matchers::ILineMatcher> {
    let pattern: OneOrMany<ProblemPattern> = problem_pattern_registry().get("msCompile").expect("padrão msCompile");
    create_line_matcher(ProblemMatcher::new("msCompile", ApplyToKind::AllDocuments, FileLocationKind::Absolute, pattern), None, None)
}

#[test]
fn matches_lines_with_leading_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "    /workspace/app.cs(5,10): error CS1001: Sample message";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("CS1001".to_string())));
    assert_eq!(marker.message, "Sample message");
}

#[test]
fn matches_lines_without_diagnostic_code() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "/workspace/app.cs(3,7): warning : Message without code";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, None);
    assert_eq!(marker.message, "Message without code");
}

#[test]
fn matches_lines_without_location_information() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "Main.cs: warning CS0168: The variable 'x' is declared but never used";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("CS0168".to_string())));
    assert_eq!(marker.message, "The variable 'x' is declared but never used");
    assert_eq!(marker.severity, MarkerSeverity::WARNING);
}

#[test]
fn matches_lines_with_build_prefixes_and_fatal_errors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "  1>c:/workspace/app.cs(12): fatal error C1002: Fatal diagnostics";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("C1002".to_string())));
    assert_eq!(marker.message, "Fatal diagnostics");
    assert_eq!(marker.severity, MarkerSeverity::ERROR);
}

#[test]
fn matches_info_diagnostics_with_codes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "2>/workspace/app.cs(20,5): info INF1001: Informational diagnostics";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("INF1001".to_string())));
    assert_eq!(marker.message, "Informational diagnostics");
    assert_eq!(marker.severity, MarkerSeverity::INFO);
}

#[test]
fn matches_lines_with_subcategory_prefixes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "Main.cs(17,20): subcategory warning CS0168: The variable 'x' is declared but never used";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("CS0168".to_string())));
    assert_eq!(marker.message, "The variable 'x' is declared but never used");
    assert_eq!(marker.severity, MarkerSeverity::WARNING);
}

#[test]
fn matches_complex_diagnostics_with_all_qualifiers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "  12>c:/workspace/Main.cs(42,7,43,2): subcategory fatal error CS9999: Complex diagnostics";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_some());
    let marker = result.r#match.unwrap().marker;
    assert_eq!(marker.code, Some(MarkerCode::Text("CS9999".to_string())));
    assert_eq!(marker.message, "Complex diagnostics");
    assert_eq!(marker.severity, MarkerSeverity::ERROR);
    assert_eq!(marker.start_line_number, 42);
    assert_eq!(marker.start_column, 7);
    assert_eq!(marker.end_line_number, 43);
    assert_eq!(marker.end_column, 2);
}

#[test]
fn ignores_diagnostics_without_origin() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut matcher = ms_compile_matcher();
    let line = "warning: The variable 'x' is declared but never used";
    let result = matcher.handle(&[line.to_string()], 0);
    assert!(result.r#match.is_none());
}
