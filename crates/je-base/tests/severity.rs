//! Testes próprios de `src/vs/base/common/severity.ts` (o VS Code não tem `severity.test.ts`).

use je_base::severity::Severity;

#[test]
fn from_value_parses_any_casing_and_falls_back_to_ignore() {
    assert_eq!(Severity::from_value("error"), Severity::Error);
    assert_eq!(Severity::from_value("ERROR"), Severity::Error);
    assert_eq!(Severity::from_value("Warning"), Severity::Warning);
    assert_eq!(Severity::from_value("warn"), Severity::Warning);
    assert_eq!(Severity::from_value("INFO"), Severity::Info);
    assert_eq!(Severity::from_value("ignore"), Severity::Ignore);
    assert_eq!(Severity::from_value("whatever"), Severity::Ignore);
    assert_eq!(Severity::from_value(""), Severity::Ignore);
}

#[test]
fn as_str_and_order() {
    assert_eq!(Severity::Error.as_str(), "error");
    assert_eq!(Severity::Warning.as_str(), "warning");
    assert_eq!(Severity::Info.as_str(), "info");
    assert_eq!(Severity::Ignore.as_str(), "ignore");
    assert!(Severity::Error > Severity::Warning && Severity::Warning > Severity::Info && Severity::Info > Severity::Ignore);
    assert_eq!(Severity::Error as i32, 3);
}
