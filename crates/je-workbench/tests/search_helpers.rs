//! Porte de `src/vs/workbench/services/search/test/common/searchHelpers.test.ts` (suíte `SearchHelpers`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - os `ITextModel` simulados do original (objetos com `getLineContent` e, na segunda suíte,
//!   `getLineCount`) são structs que implementam [`SearchTextModel`]. O método que o original não define
//!   (`getLineCount` na primeira suíte) dá pânico, como o `TypeError` de chamar `undefined`.
//! - os `Range` do editor que o original usa como `ISearchRange` são [`SearchRange`] (nenhum caso inverte
//!   início e fim, então a normalização do `Range` não faz diferença).
//! - `deepStrictEqual` vira `assert_eq!`; `{ text, lineNumber }` é [`ITextSearchContext`] dentro de
//!   [`ITextSearchResult::Context`], e os casamentos viram [`ITextSearchResult::Match`].
//! - `getQuery(surroundingContext?)` não precisa do `type: QueryType.Text`: ele é o tipo `ITextQuery`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_editor::model::FindMatch;
use je_editor::range::Range;
use je_workbench::services::search::search::{
    ITextQuery, ITextQueryProps, ITextSearchContext, ITextSearchQuery, ITextSearchResult, PatternInfo,
    SearchRange, SearchRangeSetPairing, TextSearchMatch,
};
use je_workbench::services::search::search_helpers::{
    SearchTextModel, editor_matches_to_text_search_results, get_text_search_match_with_model_context,
};

mod editor_matches_to_text_search_results_suite {
    use super::*;

    struct MockTextModel;

    impl SearchTextModel for MockTextModel {
        fn get_line_content(&self, line_number: i32) -> JsString {
            JsString::from(line_number.to_string())
        }

        fn get_line_count(&self) -> i32 {
            panic!("model.getLineCount is not a function")
        }
    }

    fn assert_ranges_equal(actual: Vec<SearchRange>, expected: Vec<SearchRange>) {
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }

    #[test]
    fn simple() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let results = editor_matches_to_text_search_results(
            &[FindMatch { range: Range::new(6, 1, 6, 2), matches: None }],
            &MockTextModel,
            None,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].preview_text, "6\n");
        assert_ranges_equal(
            results[0].range_locations.iter().map(|e| e.preview).collect(),
            vec![SearchRange::new(0, 0, 0, 1)],
        );
        assert_ranges_equal(
            results[0].range_locations.iter().map(|e| e.source).collect(),
            vec![SearchRange::new(5, 0, 5, 1)],
        );
    }

    #[test]
    fn multiple() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let results = editor_matches_to_text_search_results(
            &[
                FindMatch { range: Range::new(6, 1, 6, 2), matches: None },
                FindMatch { range: Range::new(6, 4, 8, 2), matches: None },
                FindMatch { range: Range::new(9, 1, 10, 3), matches: None },
            ],
            &MockTextModel,
            None,
        );
        assert_eq!(results.len(), 2);
        assert_ranges_equal(
            results[0].range_locations.iter().map(|e| e.preview).collect(),
            vec![SearchRange::new(0, 0, 0, 1), SearchRange::new(0, 3, 2, 1)],
        );
        assert_ranges_equal(
            results[0].range_locations.iter().map(|e| e.source).collect(),
            vec![SearchRange::new(5, 0, 5, 1), SearchRange::new(5, 3, 7, 1)],
        );
        assert_eq!(results[0].preview_text, "6\n7\n8\n");

        assert_ranges_equal(
            results[1].range_locations.iter().map(|e| e.preview).collect(),
            vec![SearchRange::new(0, 0, 1, 2)],
        );
        assert_ranges_equal(
            results[1].range_locations.iter().map(|e| e.source).collect(),
            vec![SearchRange::new(8, 0, 9, 2)],
        );
        assert_eq!(results[1].preview_text, "9\n10\n");
    }
}

mod add_context_to_editor_matches_suite {
    use super::*;

    const MOCK_LINE_COUNT: i32 = 100;

    struct MockTextModel;

    impl SearchTextModel for MockTextModel {
        fn get_line_content(&self, line_number: i32) -> JsString {
            if line_number < 1 || line_number > MOCK_LINE_COUNT {
                panic!("invalid line count: {line_number}");
            }

            JsString::from(line_number.to_string())
        }

        fn get_line_count(&self) -> i32 {
            MOCK_LINE_COUNT
        }
    }

    fn get_query(surrounding_context: Option<i64>) -> ITextSearchQuery {
        let mut query: ITextQuery = ITextQueryProps::new(Vec::new(), PatternInfo::new("test"));
        query.surrounding_context = surrounding_context;
        ITextSearchQuery::Text(query)
    }

    fn text_match(preview_text: &str, preview: SearchRange, source: SearchRange) -> TextSearchMatch {
        TextSearchMatch {
            range_locations: vec![SearchRangeSetPairing { preview, source }],
            preview_text: JsString::from(preview_text),
            webview_index: None,
        }
    }

    fn context(text: &str, line_number: i64) -> ITextSearchResult {
        ITextSearchResult::Context(ITextSearchContext { text: JsString::from(text), line_number })
    }

    fn as_results(matches: &[TextSearchMatch]) -> Vec<ITextSearchResult> {
        matches.iter().cloned().map(ITextSearchResult::Match).collect()
    }

    #[test]
    fn no_context() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let matches = [text_match("foo", SearchRange::new(0, 0, 0, 10), SearchRange::new(0, 0, 0, 10))];

        assert_eq!(
            get_text_search_match_with_model_context(&matches, &MockTextModel, &get_query(None)),
            as_results(&matches)
        );
    }

    #[test]
    fn simple() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let matches = [text_match("foo", SearchRange::new(0, 0, 0, 10), SearchRange::new(1, 0, 1, 10))];

        let mut expected = vec![context("1", 1)];
        expected.extend(as_results(&matches));
        expected.push(context("3", 3));
        assert_eq!(get_text_search_match_with_model_context(&matches, &MockTextModel, &get_query(Some(1))), expected);
    }

    #[test]
    fn multiple_matches_next_to_each_other() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let matches = [
            text_match("foo", SearchRange::new(0, 0, 0, 10), SearchRange::new(1, 0, 1, 10)),
            text_match("bar", SearchRange::new(0, 0, 0, 10), SearchRange::new(2, 0, 2, 10)),
        ];

        let mut expected = vec![context("1", 1)];
        expected.extend(as_results(&matches));
        expected.push(context("4", 4));
        assert_eq!(get_text_search_match_with_model_context(&matches, &MockTextModel, &get_query(Some(1))), expected);
    }

    #[test]
    fn boundaries() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let last_line = i64::from(MOCK_LINE_COUNT - 1);
        let matches = [
            text_match("foo", SearchRange::new(0, 0, 0, 10), SearchRange::new(0, 0, 0, 10)),
            text_match("bar", SearchRange::new(0, 0, 0, 10), SearchRange::new(last_line, 0, last_line, 10)),
        ];

        let expected = vec![
            ITextSearchResult::Match(matches[0].clone()),
            context("2", 2),
            context(&last_line.to_string(), last_line),
            ITextSearchResult::Match(matches[1].clone()),
        ];
        assert_eq!(get_text_search_match_with_model_context(&matches, &MockTextModel, &get_query(Some(1))), expected);
    }
}
