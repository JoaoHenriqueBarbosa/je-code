//! Porte de `src/vs/editor/test/common/modes/languageSelector.test.ts` (suíte `LanguageSelector`), caso a caso e na mesma ordem.
//!
//! O `null!` do primeiro caso e o `undefined` são o mesmo `None` em Rust, os dois asserts são mantidos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::glob::RelativePattern;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::uri::URI;
use je_editor::language_selector::{self, FilterPattern, LanguageFilter, LanguageSelector, score};

const MODEL_LANGUAGE: &str = "farboo";

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn model_uri() -> URI {
    parse("file:///testbed/file.fb")
}

fn filter(language: Option<&str>, scheme: Option<&str>) -> LanguageFilter {
    LanguageFilter { language: language.map(str::to_string), scheme: scheme.map(str::to_string), ..LanguageFilter::default() }
}

fn lang_scheme(language: &str, scheme: &str) -> LanguageSelector {
    filter(Some(language), Some(scheme)).into()
}

fn lang(language: &str) -> LanguageSelector {
    filter(Some(language), None).into()
}

fn scheme_only(scheme: &str) -> LanguageSelector {
    filter(None, Some(scheme)).into()
}

fn pattern(pattern: &str, scheme: Option<&str>) -> LanguageSelector {
    LanguageFilter { pattern: Some(pattern.into()), ..filter(None, scheme) }.into()
}

/// `score(selector, uri, language, true, undefined, undefined)`.
fn sync_score(selector: &LanguageSelector, uri: &URI, language: &str) -> i32 {
    score(Some(selector), uri, language, true, None, None)
}

/// `score(selector, uri, language, false, undefined, undefined)`.
fn unsync_score(selector: &LanguageSelector, uri: &URI, language: &str) -> i32 {
    score(Some(selector), uri, language, false, None, None)
}

#[test]
fn score_invalid_selector() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = model_uri();
    assert_eq!(sync_score(&LanguageFilter::default().into(), &uri, MODEL_LANGUAGE), 0);
    assert_eq!(score(None, &uri, MODEL_LANGUAGE, true, None, None), 0);
    assert_eq!(score(None, &uri, MODEL_LANGUAGE, true, None, None), 0);
    assert_eq!(sync_score(&"".into(), &uri, MODEL_LANGUAGE), 0);
}

#[test]
fn score_any_language() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = model_uri();
    assert_eq!(sync_score(&lang("*"), &uri, MODEL_LANGUAGE), 5);
    assert_eq!(sync_score(&"*".into(), &uri, MODEL_LANGUAGE), 5);

    assert_eq!(sync_score(&"*".into(), &parse("foo:bar"), MODEL_LANGUAGE), 5);
    assert_eq!(sync_score(&"farboo".into(), &parse("foo:bar"), MODEL_LANGUAGE), 10);
}

#[test]
fn score_default_schemes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = parse("git:foo/file.txt");
    let language = "farboo";

    assert_eq!(sync_score(&"*".into(), &uri, language), 5);
    assert_eq!(sync_score(&"farboo".into(), &uri, language), 10);
    assert_eq!(sync_score(&lang_scheme("farboo", ""), &uri, language), 10);
    assert_eq!(sync_score(&lang_scheme("farboo", "git"), &uri, language), 10);
    assert_eq!(sync_score(&lang_scheme("farboo", "*"), &uri, language), 10);
    assert_eq!(sync_score(&lang("farboo"), &uri, language), 10);
    assert_eq!(sync_score(&lang("*"), &uri, language), 5);

    assert_eq!(sync_score(&scheme_only("*"), &uri, language), 5);
    assert_eq!(sync_score(&scheme_only("git"), &uri, language), 10);
}

#[test]
fn score_filter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = model_uri();
    assert_eq!(sync_score(&"farboo".into(), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&lang("farboo"), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&lang_scheme("farboo", "file"), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&lang_scheme("farboo", "http"), &uri, MODEL_LANGUAGE), 0);

    assert_eq!(sync_score(&pattern("**/*.fb", None), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&pattern("**/*.fb", Some("file")), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&pattern("**/*.fb", None), &parse("foo:bar"), MODEL_LANGUAGE), 0);
    assert_eq!(sync_score(&pattern("**/*.fb", Some("foo")), &parse("foo:bar"), MODEL_LANGUAGE), 0);

    let doc_uri = parse("git:/my/file.js");
    let doc_lang_id = "javascript";
    assert_eq!(sync_score(&"javascript".into(), &doc_uri, doc_lang_id), 10); // 0;
    assert_eq!(sync_score(&lang_scheme("javascript", "git"), &doc_uri, doc_lang_id), 10); // 10;
    assert_eq!(sync_score(&"*".into(), &doc_uri, doc_lang_id), 5); // 5
    assert_eq!(sync_score(&"fooLang".into(), &doc_uri, doc_lang_id), 0); // 0
    assert_eq!(sync_score(&LanguageSelector::array(["fooLang".into(), "*".into()]), &doc_uri, doc_lang_id), 5); // 5
}

#[test]
fn score_max_filters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = model_uri();
    let matching = lang_scheme("farboo", "file");
    let fail = lang_scheme("farboo", "http");

    assert_eq!(sync_score(&matching, &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&fail, &uri, MODEL_LANGUAGE), 0);
    assert_eq!(sync_score(&LanguageSelector::array([matching.clone(), fail.clone()]), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&LanguageSelector::array([fail.clone(), fail.clone()]), &uri, MODEL_LANGUAGE), 0);
    assert_eq!(sync_score(&LanguageSelector::array(["farboo".into(), "*".into()]), &uri, MODEL_LANGUAGE), 10);
    assert_eq!(sync_score(&LanguageSelector::array(["*".into(), "farboo".into()]), &uri, MODEL_LANGUAGE), 10);
}

#[test]
fn score_has_access_to_all_models() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let doc_uri = parse("file:/my/file.js");
    let doc_lang_id = "javascript";
    assert_eq!(unsync_score(&"javascript".into(), &doc_uri, doc_lang_id), 0);
    assert_eq!(unsync_score(&lang_scheme("javascript", "file"), &doc_uri, doc_lang_id), 0);
    assert_eq!(unsync_score(&"*".into(), &doc_uri, doc_lang_id), 0);
    assert_eq!(unsync_score(&"fooLang".into(), &doc_uri, doc_lang_id), 0);
    assert_eq!(unsync_score(&LanguageSelector::array(["fooLang".into(), "*".into()]), &doc_uri, doc_lang_id), 0);

    let with_access = |language: &str, scheme: Option<&str>| -> LanguageSelector {
        LanguageFilter { has_access_to_all_models: true, ..filter(Some(language), scheme) }.into()
    };
    assert_eq!(unsync_score(&with_access("javascript", Some("file")), &doc_uri, doc_lang_id), 10);
    assert_eq!(
        unsync_score(&LanguageSelector::array(["fooLang".into(), "*".into(), with_access("*", None)]), &doc_uri, doc_lang_id),
        5
    );
}

#[test]
fn score_notebook_type() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = parse("vscode-notebook-cell:///my/file.js#blabla");
    let lang_id = "javascript";
    let notebook_type = Some("fooBook");
    let notebook_uri = parse("file:///my/file.js");

    let notebook_score = |selector: &LanguageSelector| score(Some(selector), &uri, lang_id, true, Some(&notebook_uri), notebook_type);
    let notebook_filter = |notebook: &str, language: Option<&str>, scheme: Option<&str>| -> LanguageSelector {
        LanguageFilter { notebook_type: Some(notebook.to_string()), ..filter(language, scheme) }.into()
    };

    assert_eq!(sync_score(&"javascript".into(), &uri, lang_id), 10);
    assert_eq!(notebook_score(&"javascript".into()), 10);
    assert_eq!(notebook_score(&notebook_filter("fooBook", None, None)), 10);
    assert_eq!(notebook_score(&notebook_filter("fooBook", Some("javascript"), Some("file"))), 10);
    assert_eq!(notebook_score(&notebook_filter("fooBook", Some("*"), None)), 10);
    assert_eq!(notebook_score(&notebook_filter("*", Some("*"), None)), 5);
    assert_eq!(notebook_score(&notebook_filter("*", Some("javascript"), None)), 10);
}

#[test]
fn snippet_choices_lost_149363() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let base = LanguageFilter {
        scheme: Some("vscode-notebook-cell".to_string()),
        pattern: Some("/some/path/file.py".into()),
        language: Some("python".to_string()),
        ..LanguageFilter::default()
    };
    let selector: LanguageSelector = base.clone().into();

    let model_uri = parse("vscode-notebook-cell:///some/path/file.py");
    let nb_uri = parse("file:///some/path/file.py");
    assert_eq!(score(Some(&selector), &model_uri, "python", true, Some(&nb_uri), Some("jupyter")), 10);

    let selector2: LanguageSelector = LanguageFilter { notebook_type: Some("jupyter".to_string()), ..base }.into();

    assert_eq!(score(Some(&selector2), &model_uri, "python", true, Some(&nb_uri), Some("jupyter")), 0);
}

#[test]
fn document_selector_match_unexpected_result_value_60232() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let selector: LanguageSelector =
        LanguageFilter { pattern: Some("**/*.interface.json".into()), ..filter(Some("json"), Some("file")) }.into();
    let value = sync_score(&selector, &parse("file:///C:/Users/zlhe/Desktop/test.interface.json"), "json");
    assert_eq!(value, 10);
}

#[test]
fn document_selector_match_platform_paths_99938() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let selector: LanguageSelector = LanguageFilter {
        pattern: Some(FilterPattern::from(RelativePattern { base: "/home/user/Desktop".to_string(), pattern: "*.json".to_string() })),
        ..LanguageFilter::default()
    }
    .into();
    let value = sync_score(&selector, &URI::file("/home/user/Desktop/test.json", ProcessEnvironment.platform()), "json");
    assert_eq!(value, 10);
}

#[test]
fn notebook_type_without_notebook() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = parse("file:///my/file.bat");
    let lang_id = "bat";
    let with_notebook = |notebook: &str| -> LanguageSelector {
        LanguageFilter { notebook_type: Some(notebook.to_string()), ..filter(Some("bat"), None) }.into()
    };

    let mut value = sync_score(&with_notebook("xxx"), &uri, lang_id);
    assert_eq!(value, 0);

    value = sync_score(&with_notebook("*"), &uri, lang_id);
    assert_eq!(value, 0);
}

#[test]
fn select_language_ids() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    use language_selector::select_language_ids;
    let mut result: Vec<String> = Vec::new();

    select_language_ids(&"typescript".into(), &mut result);
    assert_eq!(result, ["typescript"]);

    result.clear();
    select_language_ids(&lang_scheme("python", "file"), &mut result);
    assert_eq!(result, ["python"]);

    result.clear();
    select_language_ids(&scheme_only("file"), &mut result);
    assert!(result.is_empty());

    result.clear();
    select_language_ids(&LanguageSelector::array(["javascript".into(), lang("css"), scheme_only("untitled")]), &mut result);
    result.sort();
    assert_eq!(result, ["css", "javascript"]);

    result.clear();
    select_language_ids(&"*".into(), &mut result);
    assert_eq!(result, ["*"]);
}
