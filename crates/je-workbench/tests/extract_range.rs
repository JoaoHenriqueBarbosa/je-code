//! Porte de `src/vs/workbench/contrib/search/test/common/extractRange.test.ts` (suíte `extractRangeFromFilter`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - As suítes `ranges` e `unless` geram um `test(...)` por especificação do `for`; aqui cada especificação é um
//!   `#[test]` de nome próprio, na ordem do vetor, e o corpo é o mesmo ([`ranges_case`] e [`unless_case`]).
//! - `assert.deepStrictEqual(x, { filter, range })` compara o [`FilterAndRange`] inteiro (o `Range` do je-editor
//!   é o `IRange` do original).
//! - `res?.range.startLineNumber` sobre `undefined` reprova a asserção; o porte compara `Option` com `Some`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::range::Range;
use je_workbench::contrib::search::search::{FilterAndRange, extract_range_from_filter};

const BASE: &str = "/some/path/file.txt";

fn filter_and_range(filter: &str, range: Range) -> Option<FilterAndRange> {
    Some(FilterAndRange { filter: filter.to_owned(), range })
}

#[test]
fn basics() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(extract_range_from_filter("", None).is_none());
    assert!(extract_range_from_filter("/some/path", None).is_none());
    assert!(extract_range_from_filter("/some/path/file.txt", None).is_none());

    for line_sep in [":", "#", "(", ":line "] {
        for col_sep in [":", "#", ","] {
            let base = "/some/path/file.txt";

            let mut res = extract_range_from_filter(&format!("{base}{line_sep}20"), None);
            assert_eq!(res.as_ref().map(|r| r.filter.as_str()), Some(base));
            assert_eq!(res.as_ref().map(|r| r.range.start_line_number), Some(20));
            assert_eq!(res.as_ref().map(|r| r.range.start_column), Some(1));

            res = extract_range_from_filter(&format!("{base}{line_sep}20{col_sep}"), None);
            assert_eq!(res.as_ref().map(|r| r.filter.as_str()), Some(base));
            assert_eq!(res.as_ref().map(|r| r.range.start_line_number), Some(20));
            assert_eq!(res.as_ref().map(|r| r.range.start_column), Some(1));

            res = extract_range_from_filter(&format!("{base}{line_sep}20{col_sep}3"), None);
            assert_eq!(res.as_ref().map(|r| r.filter.as_str()), Some(base));
            assert_eq!(res.as_ref().map(|r| r.range.start_line_number), Some(20));
            assert_eq!(res.as_ref().map(|r| r.range.start_column), Some(3));
        }
    }
}

#[test]
fn allow_space_after_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let res = extract_range_from_filter("/some/path/file.txt (19,20)", None);

    assert_eq!(res.as_ref().map(|r| r.filter.as_str()), Some("/some/path/file.txt"));
    assert_eq!(res.as_ref().map(|r| r.range.start_line_number), Some(19));
    assert_eq!(res.as_ref().map(|r| r.range.start_column), Some(20));
}

// suite('ranges')
mod ranges {
    use super::*;

    /// O corpo do `test(filter, ...)` do `for` do original.
    fn ranges_case(filter: &str, range: Range) {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(extract_range_from_filter(filter, None), filter_and_range(BASE, range));
    }

    // line range: "20-40"
    #[test]
    fn base_colon_20_40() {
        ranges_case(&format!("{BASE}:20-40"), Range::new(20, 1, 40, 1));
    }

    // line and column range: "20:3-40:5"
    #[test]
    fn base_colon_20_3_40_5() {
        ranges_case(&format!("{BASE}:20:3-40:5"), Range::new(20, 3, 40, 5));
    }

    // end column defaults to start of the end line: "20:3-40"
    #[test]
    fn base_colon_20_3_40() {
        ranges_case(&format!("{BASE}:20:3-40"), Range::new(20, 3, 40, 1));
    }

    // mixed separators: "20#3-40,5"
    #[test]
    fn base_hash_20_3_40_comma_5() {
        ranges_case(&format!("{BASE}#20#3-40,5"), Range::new(20, 3, 40, 5));
    }

    // paren style: "(20,3-40,5)"
    #[test]
    fn base_paren_20_3_40_5() {
        ranges_case(&format!("{BASE}(20,3-40,5)"), Range::new(20, 3, 40, 5));
    }

    // dangling separator falls back to single line: "20-"
    #[test]
    fn base_colon_20_dash() {
        ranges_case(&format!("{BASE}:20-"), Range::new(20, 1, 20, 1));
    }

    #[test]
    fn hyphen_in_path_is_not_treated_as_a_range() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(extract_range_from_filter("/some/path/my-file.txt", None).is_none());
        assert!(extract_range_from_filter("/some/path/file-2.txt", None).is_none());
    }
}

// suite('unless')
mod unless {
    use super::*;

    /// O corpo do `test(`${filter} - ${JSON.stringify(unless)}`, ...)` do `for` do original.
    fn unless_case(filter: &str, unless: &[&str], result: Option<FilterAndRange>) {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(extract_range_from_filter(filter, Some(unless)), result);
    }

    // alpha-only symbol after unless
    #[test]
    fn alpha_only_symbol_after_unless() {
        unless_case("/some/path/file.txt@alphasymbol", &["@"], None);
    }

    // unless as first char
    #[test]
    fn unless_as_first_char() {
        unless_case("@/some/path/file.txt (19,20)", &["@"], None);
    }

    // unless as last char
    #[test]
    fn unless_as_last_char() {
        unless_case("/some/path/file.txt (19,20)@", &["@"], None);
    }

    // unless before ,
    #[test]
    fn unless_before_comma() {
        unless_case("/some/@path/file.txt (19,20)", &["@"], filter_and_range("/some/@path/file.txt", Range::new(19, 20, 19, 20)));
    }

    // unless before :
    #[test]
    fn unless_before_colon() {
        unless_case("/some/@path/file.txt:19:20", &["@"], filter_and_range("/some/@path/file.txt", Range::new(19, 20, 19, 20)));
    }

    // unless before #
    #[test]
    fn unless_before_hash() {
        unless_case("/some/@path/file.txt#19", &["@"], filter_and_range("/some/@path/file.txt", Range::new(19, 1, 19, 1)));
    }
}
