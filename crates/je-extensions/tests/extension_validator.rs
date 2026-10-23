//! Porte de `src/vs/platform/extensions/test/common/extensionValidator.test.ts` (suíte `Extension Version Validator`), caso a caso e na mesma ordem.

use je_extensions::validator::{self, NormalizedVersion, ParsedVersion};
use serde_json::json;

const PRODUCT_VERSION: &str = "2021-05-11T21:54:30.577Z";

fn date_ms(text: &str) -> f64 {
    je_extensions::validator::parse_date_ms(text).expect("data válida")
}

#[test]
fn is_valid_version_str() {
    assert!(validator::is_valid_version_str("0.10.0-dev"));
    assert!(validator::is_valid_version_str("0.10.0"));
    assert!(validator::is_valid_version_str("0.10.1"));
    assert!(validator::is_valid_version_str("0.10.100"));
    assert!(validator::is_valid_version_str("0.11.0"));

    assert!(validator::is_valid_version_str("x.x.x"));
    assert!(validator::is_valid_version_str("0.x.x"));
    assert!(validator::is_valid_version_str("0.10.0"));
    assert!(validator::is_valid_version_str("0.10.x"));
    assert!(validator::is_valid_version_str("^0.10.0"));
    assert!(validator::is_valid_version_str("*"));

    assert!(!validator::is_valid_version_str("0.x.x.x"));
    assert!(!validator::is_valid_version_str("0.10"));
    assert!(!validator::is_valid_version_str("0.10."));
}

#[test]
fn parse_version() {
    #[allow(clippy::too_many_arguments)]
    fn assert_parse_version(
        version: &str,
        has_caret: bool,
        has_greater_equals: bool,
        major_base: u64,
        major_must_equal: bool,
        minor_base: u64,
        minor_must_equal: bool,
        patch_base: u64,
        patch_must_equal: bool,
        pre_release: Option<&str>,
    ) {
        let actual = validator::parse_version(version);
        let expected = ParsedVersion {
            has_caret,
            has_greater_equals,
            major_base,
            major_must_equal,
            minor_base,
            minor_must_equal,
            patch_base,
            patch_must_equal,
            pre_release: pre_release.map(str::to_owned),
        };
        assert_eq!(actual, Some(expected), "parseVersion for {version}");
    }



    assert_parse_version("0.10.0-dev", false, false, 0, true, 10, true, 0, true, Some("-dev"));
    assert_parse_version("0.10.0", false, false, 0, true, 10, true, 0, true, None);
    assert_parse_version("0.10.1", false, false, 0, true, 10, true, 1, true, None);
    assert_parse_version("0.10.100", false, false, 0, true, 10, true, 100, true, None);
    assert_parse_version("0.11.0", false, false, 0, true, 11, true, 0, true, None);

    assert_parse_version("x.x.x", false, false, 0, false, 0, false, 0, false, None);
    assert_parse_version("0.x.x", false, false, 0, true, 0, false, 0, false, None);
    assert_parse_version("0.10.x", false, false, 0, true, 10, true, 0, false, None);
    assert_parse_version("^0.10.0", true, false, 0, true, 10, true, 0, true, None);
    assert_parse_version("^0.10.2", true, false, 0, true, 10, true, 2, true, None);
    assert_parse_version("^1.10.2", true, false, 1, true, 10, true, 2, true, None);
    assert_parse_version("*", false, false, 0, false, 0, false, 0, false, None);

    assert_parse_version(">=0.0.1", false, true, 0, true, 0, true, 1, true, None);
    assert_parse_version(">=2.4.3", false, true, 2, true, 4, true, 3, true, None);

    // Parse versions with HHMM date format
    assert_parse_version("1.10.0-202105111430", false, false, 1, true, 10, true, 0, true, Some("-202105111430"));
    assert_parse_version("^1.10.0-202105112359", true, false, 1, true, 10, true, 0, true, Some("-202105112359"));

}

#[test]
fn normalize_version() {
    #[allow(clippy::too_many_arguments)]
    fn assert_normalize_version(
        version: &str,
        major_base: u64,
        major_must_equal: bool,
        minor_base: u64,
        minor_must_equal: bool,
        patch_base: u64,
        patch_must_equal: bool,
        is_minimum: bool,
        not_before: f64,
    ) {
        let actual = validator::normalize_version(validator::parse_version(version).as_ref());
        let expected = NormalizedVersion { major_base, major_must_equal, minor_base, minor_must_equal, patch_base, patch_must_equal, is_minimum, not_before };
        assert_eq!(actual, Some(expected), "parseVersion for {version}");
    }


    assert_normalize_version("0.10.0-dev", 0, true, 10, true, 0, true, false, 0.0);
    assert_normalize_version("0.10.0-222222222", 0, true, 10, true, 0, true, false, 0.0);
    assert_normalize_version("0.10.0-20210511", 0, true, 10, true, 0, true, false, date_ms("2021-05-11T00:00:00Z"));

    // Normalize versions with HHMM date format
    assert_normalize_version("1.10.0-202105111430", 1, true, 10, true, 0, true, false, date_ms("2021-05-11T14:30:00Z"));
    assert_normalize_version("1.10.0-202105112359", 1, true, 10, true, 0, true, false, date_ms("2021-05-11T23:59:00Z"));
    assert_normalize_version("1.10.0-202105110000", 1, true, 10, true, 0, true, false, date_ms("2021-05-11T00:00:00Z"));

    assert_normalize_version("0.10.0", 0, true, 10, true, 0, true, false, 0.0);
    assert_normalize_version("0.10.1", 0, true, 10, true, 1, true, false, 0.0);
    assert_normalize_version("0.10.100", 0, true, 10, true, 100, true, false, 0.0);
    assert_normalize_version("0.11.0", 0, true, 11, true, 0, true, false, 0.0);

    assert_normalize_version("x.x.x", 0, false, 0, false, 0, false, false, 0.0);
    assert_normalize_version("0.x.x", 0, true, 0, false, 0, false, false, 0.0);
    assert_normalize_version("0.10.x", 0, true, 10, true, 0, false, false, 0.0);
    assert_normalize_version("^0.10.0", 0, true, 10, true, 0, false, false, 0.0);
    assert_normalize_version("^0.10.2", 0, true, 10, true, 2, false, false, 0.0);
    assert_normalize_version("^1.10.2", 1, true, 10, false, 2, false, false, 0.0);
    assert_normalize_version("*", 0, false, 0, false, 0, false, false, 0.0);

    assert_normalize_version(">=0.0.1", 0, true, 0, true, 1, true, true, 0.0);
    assert_normalize_version(">=2.4.3", 2, true, 4, true, 3, true, true, 0.0);
    assert_normalize_version(">=2.4.3", 2, true, 4, true, 3, true, true, 0.0);

}

#[test]
fn is_valid_version() {
    fn test_is_valid_version(version: &str, desired_version: &str, expected_result: bool) {
        let actual = validator::is_valid_version(version, Some(PRODUCT_VERSION), desired_version);
        assert_eq!(actual, expected_result, "extension - vscode: {version}, desiredVersion: {desired_version} should be {expected_result}");
    }

    test_is_valid_version("0.10.0-dev", "0.x.x", true);
    test_is_valid_version("0.10.0-dev", "0.10.0", true);
    test_is_valid_version("0.10.0-dev", "0.10.2", false);
    test_is_valid_version("0.10.0-dev", "^0.10.2", false);
    test_is_valid_version("0.10.0-dev", "0.10.x", true);
    test_is_valid_version("0.10.0-dev", "^0.10.0", true);
    test_is_valid_version("0.10.0-dev", "*", true);
    test_is_valid_version("0.10.0-dev", ">=0.0.1", true);
    test_is_valid_version("0.10.0-dev", ">=0.0.10", true);
    test_is_valid_version("0.10.0-dev", ">=0.10.0", true);
    test_is_valid_version("0.10.0-dev", ">=0.10.1", false);
    test_is_valid_version("0.10.0-dev", ">=1.0.0", false);

    test_is_valid_version("0.10.0", "x.x.x", true);
    test_is_valid_version("0.10.0", "0.x.x", true);
    test_is_valid_version("0.10.0", "0.10.0", true);
    test_is_valid_version("0.10.0", "0.10.2", false);
    test_is_valid_version("0.10.0", "^0.10.2", false);
    test_is_valid_version("0.10.0", "0.10.x", true);
    test_is_valid_version("0.10.0", "^0.10.0", true);
    test_is_valid_version("0.10.0", "*", true);

    test_is_valid_version("0.10.1", "x.x.x", true);
    test_is_valid_version("0.10.1", "0.x.x", true);
    test_is_valid_version("0.10.1", "0.10.0", false);
    test_is_valid_version("0.10.1", "0.10.2", false);
    test_is_valid_version("0.10.1", "^0.10.2", false);
    test_is_valid_version("0.10.1", "0.10.x", true);
    test_is_valid_version("0.10.1", "^0.10.0", true);
    test_is_valid_version("0.10.1", "*", true);

    test_is_valid_version("0.10.100", "x.x.x", true);
    test_is_valid_version("0.10.100", "0.x.x", true);
    test_is_valid_version("0.10.100", "0.10.0", false);
    test_is_valid_version("0.10.100", "0.10.2", false);
    test_is_valid_version("0.10.100", "^0.10.2", true);
    test_is_valid_version("0.10.100", "0.10.x", true);
    test_is_valid_version("0.10.100", "^0.10.0", true);
    test_is_valid_version("0.10.100", "*", true);

    test_is_valid_version("0.11.0", "x.x.x", true);
    test_is_valid_version("0.11.0", "0.x.x", true);
    test_is_valid_version("0.11.0", "0.10.0", false);
    test_is_valid_version("0.11.0", "0.10.2", false);
    test_is_valid_version("0.11.0", "^0.10.2", false);
    test_is_valid_version("0.11.0", "0.10.x", false);
    test_is_valid_version("0.11.0", "^0.10.0", false);
    test_is_valid_version("0.11.0", "*", true);

    // Anything < 1.0.0 is compatible

    test_is_valid_version("1.0.0", "x.x.x", true);
    test_is_valid_version("1.0.0", "0.x.x", true);
    test_is_valid_version("1.0.0", "0.10.0", false);
    test_is_valid_version("1.0.0", "0.10.2", false);
    test_is_valid_version("1.0.0", "^0.10.2", true);
    test_is_valid_version("1.0.0", "0.10.x", true);
    test_is_valid_version("1.0.0", "^0.10.0", true);
    test_is_valid_version("1.0.0", "1.0.0", true);
    test_is_valid_version("1.0.0", "^1.0.0", true);
    test_is_valid_version("1.0.0", "^2.0.0", false);
    test_is_valid_version("1.0.0", "*", true);
    test_is_valid_version("1.0.0", ">=0.0.1", true);
    test_is_valid_version("1.0.0", ">=0.0.10", true);
    test_is_valid_version("1.0.0", ">=0.10.0", true);
    test_is_valid_version("1.0.0", ">=0.10.1", true);
    test_is_valid_version("1.0.0", ">=1.0.0", true);
    test_is_valid_version("1.0.0", ">=1.1.0", false);
    test_is_valid_version("1.0.0", ">=1.0.1", false);
    test_is_valid_version("1.0.0", ">=2.0.0", false);

    test_is_valid_version("1.0.100", "x.x.x", true);
    test_is_valid_version("1.0.100", "0.x.x", true);
    test_is_valid_version("1.0.100", "0.10.0", false);
    test_is_valid_version("1.0.100", "0.10.2", false);
    test_is_valid_version("1.0.100", "^0.10.2", true);
    test_is_valid_version("1.0.100", "0.10.x", true);
    test_is_valid_version("1.0.100", "^0.10.0", true);
    test_is_valid_version("1.0.100", "1.0.0", false);
    test_is_valid_version("1.0.100", "^1.0.0", true);
    test_is_valid_version("1.0.100", "^1.0.1", true);
    test_is_valid_version("1.0.100", "^2.0.0", false);
    test_is_valid_version("1.0.100", "*", true);

    test_is_valid_version("1.100.0", "x.x.x", true);
    test_is_valid_version("1.100.0", "0.x.x", true);
    test_is_valid_version("1.100.0", "0.10.0", false);
    test_is_valid_version("1.100.0", "0.10.2", false);
    test_is_valid_version("1.100.0", "^0.10.2", true);
    test_is_valid_version("1.100.0", "0.10.x", true);
    test_is_valid_version("1.100.0", "^0.10.0", true);
    test_is_valid_version("1.100.0", "1.0.0", false);
    test_is_valid_version("1.100.0", "^1.0.0", true);
    test_is_valid_version("1.100.0", "^1.1.0", true);
    test_is_valid_version("1.100.0", "^1.100.0", true);
    test_is_valid_version("1.100.0", "^2.0.0", false);
    test_is_valid_version("1.100.0", "*", true);
    test_is_valid_version("1.100.0", ">=1.99.0", true);
    test_is_valid_version("1.100.0", ">=1.100.0", true);
    test_is_valid_version("1.100.0", ">=1.101.0", false);

    test_is_valid_version("2.0.0", "x.x.x", true);
    test_is_valid_version("2.0.0", "0.x.x", false);
    test_is_valid_version("2.0.0", "0.10.0", false);
    test_is_valid_version("2.0.0", "0.10.2", false);
    test_is_valid_version("2.0.0", "^0.10.2", false);
    test_is_valid_version("2.0.0", "0.10.x", false);
    test_is_valid_version("2.0.0", "^0.10.0", false);
    test_is_valid_version("2.0.0", "1.0.0", false);
    test_is_valid_version("2.0.0", "^1.0.0", false);
    test_is_valid_version("2.0.0", "^1.1.0", false);
    test_is_valid_version("2.0.0", "^1.100.0", false);
    test_is_valid_version("2.0.0", "^2.0.0", true);
    test_is_valid_version("2.0.0", "*", true);
}

#[test]
fn is_valid_extension_version() {
    fn test_extension_version(version: &str, desired_version: &str, is_builtin: bool, has_main: bool, expected_result: bool) {
        let mut manifest = json!({ "name": "test", "publisher": "test", "version": "0.0.0", "engines": { "vscode": desired_version } });
        if has_main {
            manifest["main"] = json!("something");
        }
        let mut reasons = Vec::new();
        let actual = validator::is_valid_extension_version(version, Some(PRODUCT_VERSION), &manifest, is_builtin, &mut reasons);
        assert_eq!(actual, expected_result, "version: {version}, desiredVersion: {desired_version}, desc: {manifest}, reasons: {reasons:?}");
    }
    fn test_is_invalid_extension_version(version: &str, desired_version: &str, is_builtin: bool, has_main: bool) {
        test_extension_version(version, desired_version, is_builtin, has_main, false);
    }
    fn test_is_valid_extension_version(version: &str, desired_version: &str, is_builtin: bool, has_main: bool) {
        test_extension_version(version, desired_version, is_builtin, has_main, true);
    }
    fn test_is_valid_version(version: &str, desired_version: &str, expected_result: bool) {
        test_extension_version(version, desired_version, false, true, expected_result);
    }



    // builtin are allowed to use * or x.x.x
    test_is_valid_extension_version("0.10.0-dev", "*", true, true);
    test_is_valid_extension_version("0.10.0-dev", "x.x.x", true, true);
    test_is_valid_extension_version("0.10.0-dev", "0.x.x", true, true);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", true, true);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", true, true);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", true, true);
    test_is_valid_extension_version("0.10.0-dev", "*", true, false);
    test_is_valid_extension_version("0.10.0-dev", "x.x.x", true, false);
    test_is_valid_extension_version("0.10.0-dev", "0.x.x", true, false);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", true, false);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", true, false);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", true, false);

    // normal extensions are allowed to use * or x.x.x only if they have no main
    test_is_invalid_extension_version("0.10.0-dev", "*", false, true);
    test_is_invalid_extension_version("0.10.0-dev", "x.x.x", false, true);
    test_is_invalid_extension_version("0.10.0-dev", "0.x.x", false, true);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", false, true);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", false, true);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", false, true);
    test_is_valid_extension_version("0.10.0-dev", "*", false, false);
    test_is_valid_extension_version("0.10.0-dev", "x.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", false, false);

    // extensions without "main" get no version check
    test_is_valid_extension_version("0.10.0-dev", ">=0.9.1-pre.1", false, false);
    test_is_valid_extension_version("0.10.0-dev", "*", false, false);
    test_is_valid_extension_version("0.10.0-dev", "x.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "*", false, false);
    test_is_valid_extension_version("0.10.0-dev", "x.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.x.x", false, false);
    test_is_valid_extension_version("0.10.0-dev", "0.10.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.x.x", false, false);
    test_is_valid_extension_version("1.10.0-dev", "1.10.x", false, false);

    // normal extensions with code
    test_is_valid_version("0.10.0-dev", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.0-dev", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.0-dev", "0.10.0", true);
    test_is_valid_version("0.10.0-dev", "0.10.2", false);
    test_is_valid_version("0.10.0-dev", "^0.10.2", false);
    test_is_valid_version("0.10.0-dev", "0.10.x", true);
    test_is_valid_version("0.10.0-dev", "^0.10.0", true);
    test_is_valid_version("0.10.0-dev", "*", false); // fails due to lack of specificity

    test_is_valid_version("0.10.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.0", "0.10.0", true);
    test_is_valid_version("0.10.0", "0.10.2", false);
    test_is_valid_version("0.10.0", "^0.10.2", false);
    test_is_valid_version("0.10.0", "0.10.x", true);
    test_is_valid_version("0.10.0", "^0.10.0", true);
    test_is_valid_version("0.10.0", "*", false); // fails due to lack of specificity

    test_is_valid_version("0.10.1", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.1", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.1", "0.10.0", false);
    test_is_valid_version("0.10.1", "0.10.2", false);
    test_is_valid_version("0.10.1", "^0.10.2", false);
    test_is_valid_version("0.10.1", "0.10.x", true);
    test_is_valid_version("0.10.1", "^0.10.0", true);
    test_is_valid_version("0.10.1", "*", false); // fails due to lack of specificity

    test_is_valid_version("0.10.100", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.100", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.10.100", "0.10.0", false);
    test_is_valid_version("0.10.100", "0.10.2", false);
    test_is_valid_version("0.10.100", "^0.10.2", true);
    test_is_valid_version("0.10.100", "0.10.x", true);
    test_is_valid_version("0.10.100", "^0.10.0", true);
    test_is_valid_version("0.10.100", "*", false); // fails due to lack of specificity

    test_is_valid_version("0.11.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.11.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("0.11.0", "0.10.0", false);
    test_is_valid_version("0.11.0", "0.10.2", false);
    test_is_valid_version("0.11.0", "^0.10.2", false);
    test_is_valid_version("0.11.0", "0.10.x", false);
    test_is_valid_version("0.11.0", "^0.10.0", false);
    test_is_valid_version("0.11.0", "*", false); // fails due to lack of specificity

    test_is_valid_version("1.0.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.0", "0.10.0", false);
    test_is_valid_version("1.0.0", "0.10.2", false);
    test_is_valid_version("1.0.0", "^0.10.2", true);
    test_is_valid_version("1.0.0", "0.10.x", true);
    test_is_valid_version("1.0.0", "^0.10.0", true);
    test_is_valid_version("1.0.0", "*", false); // fails due to lack of specificity

    test_is_valid_version("1.10.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.10.0", "1.x.x", true);
    test_is_valid_version("1.10.0", "1.10.0", true);
    test_is_valid_version("1.10.0", "1.10.2", false);
    test_is_valid_version("1.10.0", "^1.10.2", false);
    test_is_valid_version("1.10.0", "1.10.x", true);
    test_is_valid_version("1.10.0", "^1.10.0", true);
    test_is_valid_version("1.10.0", "*", false); // fails due to lack of specificity


    // Anything < 1.0.0 is compatible

    test_is_valid_version("1.0.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.0", "0.10.0", false);
    test_is_valid_version("1.0.0", "0.10.2", false);
    test_is_valid_version("1.0.0", "^0.10.2", true);
    test_is_valid_version("1.0.0", "0.10.x", true);
    test_is_valid_version("1.0.0", "^0.10.0", true);
    test_is_valid_version("1.0.0", "1.0.0", true);
    test_is_valid_version("1.0.0", "^1.0.0", true);
    test_is_valid_version("1.0.0", "^2.0.0", false);
    test_is_valid_version("1.0.0", "*", false); // fails due to lack of specificity

    test_is_valid_version("1.0.100", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.100", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.0.100", "0.10.0", false);
    test_is_valid_version("1.0.100", "0.10.2", false);
    test_is_valid_version("1.0.100", "^0.10.2", true);
    test_is_valid_version("1.0.100", "0.10.x", true);
    test_is_valid_version("1.0.100", "^0.10.0", true);
    test_is_valid_version("1.0.100", "1.0.0", false);
    test_is_valid_version("1.0.100", "^1.0.0", true);
    test_is_valid_version("1.0.100", "^1.0.1", true);
    test_is_valid_version("1.0.100", "^2.0.0", false);
    test_is_valid_version("1.0.100", "*", false); // fails due to lack of specificity

    test_is_valid_version("1.100.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.100.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("1.100.0", "0.10.0", false);
    test_is_valid_version("1.100.0", "0.10.2", false);
    test_is_valid_version("1.100.0", "^0.10.2", true);
    test_is_valid_version("1.100.0", "0.10.x", true);
    test_is_valid_version("1.100.0", "^0.10.0", true);
    test_is_valid_version("1.100.0", "1.0.0", false);
    test_is_valid_version("1.100.0", "^1.0.0", true);
    test_is_valid_version("1.100.0", "^1.1.0", true);
    test_is_valid_version("1.100.0", "^1.100.0", true);
    test_is_valid_version("1.100.0", "^2.0.0", false);
    test_is_valid_version("1.100.0", "*", false); // fails due to lack of specificity

    test_is_valid_version("2.0.0", "x.x.x", false); // fails due to lack of specificity
    test_is_valid_version("2.0.0", "0.x.x", false); // fails due to lack of specificity
    test_is_valid_version("2.0.0", "0.10.0", false);
    test_is_valid_version("2.0.0", "0.10.2", false);
    test_is_valid_version("2.0.0", "^0.10.2", false);
    test_is_valid_version("2.0.0", "0.10.x", false);
    test_is_valid_version("2.0.0", "^0.10.0", false);
    test_is_valid_version("2.0.0", "1.0.0", false);
    test_is_valid_version("2.0.0", "^1.0.0", false);
    test_is_valid_version("2.0.0", "^1.1.0", false);
    test_is_valid_version("2.0.0", "^1.100.0", false);
    test_is_valid_version("2.0.0", "^2.0.0", true);
    test_is_valid_version("2.0.0", "*", false); // fails due to lack of specificity

    // date tags
    test_is_valid_version("1.10.0", "^1.10.0-20210511", true); // current date
    test_is_valid_version("1.10.0", "^1.10.0-20210510", true); // before date
    test_is_valid_version("1.10.0", "^1.10.0-20210512", false); // future date
    test_is_valid_version("1.10.1", "^1.10.0-20200101", true); // before date, but ahead version
    test_is_valid_version("1.11.0", "^1.10.0-20200101", true);

    // Test with HHMM date format
    test_is_valid_version("1.10.0", "^1.10.0-202105111400", true); // product at beginning of day, required time at 14:00
    test_is_valid_version("1.10.0", "^1.10.0-202105112359", false); // product at beginning of day, required time at 23:59
    test_is_valid_version("1.10.0", "^1.10.0-202105110000", true); // product at beginning of day, required time at 00:00
}

#[test]
fn is_valid_extension_version_checks_browser_only_extensions() {
    let manifest = json!({ "name": "test", "publisher": "test", "version": "0.0.0", "engines": { "vscode": "^1.45.0" }, "browser": "something" });
    assert!(!validator::is_valid_extension_version("1.44.0", None, &manifest, false, &mut Vec::new()));
}
