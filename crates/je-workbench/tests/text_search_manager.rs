//! Porte de `src/vs/workbench/services/search/test/node/textSearchManager.test.ts` (suíte `NativeTextSearchManager`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Cada caso é `async` e roda em [`run_with_real_timers`]; `await m.search(...)` é `.await.unwrap()` sobre a promessa.
//! - `TextSearchProvider2` é o trait do porte: os provedores literais são structs, e o `ProviderResult` que devolve
//!   `null` é `Promise::resolved(None)`. O `progress: Progress<TextSearchResult2>` é o `Rc<dyn IProgress<...>>`.
//! - `let correctEncoding` e `let receivedResults` mutados dentro do provedor e do callback são `Rc<Cell<...>>`.
//! - `new Range(0, 0, 0, 5)` é `Range::new(0, 0, 0, 5)`, e `[{ sourceRange, previewRange }]` é `TextSearchMatchRanges`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::progress::progress::IProgress;
use je_platform::resources::uri_file;
use je_workbench::services::search::search::{IFileMatch, IFolderQuery, ITextQuery, PatternInfo};
use je_workbench::services::search::search_ext_types::{
    ProviderResult, Range, TextSearchComplete2, TextSearchMatch2, TextSearchMatchRanges, TextSearchProvider2, TextSearchProviderOptions, TextSearchQuery2, TextSearchResult2,
};
use je_workbench::services::search::text_search_manager::NativeTextSearchManager;

struct EncodingProvider {
    correct_encoding: Rc<Cell<bool>>,
}

impl TextSearchProvider2 for EncodingProvider {
    fn provide_text_search_results(
        &self,
        _query: &TextSearchQuery2,
        options: &TextSearchProviderOptions,
        _progress: Rc<dyn IProgress<TextSearchResult2>>,
        _token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        self.correct_encoding.set(options.folder_options[0].encoding == "windows-1252");

        Promise::resolved(None)
    }
}

struct UnmatchedFolderProvider;

impl TextSearchProvider2 for UnmatchedFolderProvider {
    fn provide_text_search_results(
        &self,
        _query: &TextSearchQuery2,
        _options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        _token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        let range = Range::new(0, 0, 0, 5);

        // Report a result from a folder that IS in the query - should be received
        progress.report(TextSearchResult2::Match(TextSearchMatch2::new(
            uri_file("/folder1/test.txt"),
            vec![TextSearchMatchRanges { source_range: range, preview_range: range }],
            "test match",
        )));

        // Report a result from a folder that is NOT in the query
        // This exercises: folderQuery?.folder?.scheme where folderQuery is undefined
        // The optional chaining should handle this gracefully without throwing
        progress.report(TextSearchResult2::Match(TextSearchMatch2::new(
            uri_file("/unknown/folder/file.txt"),
            vec![TextSearchMatchRanges { source_range: range, preview_range: range }],
            "unmatched result",
        )));

        Promise::resolved(None)
    }
}

#[test]
fn fixes_encoding() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        let correct_encoding = Rc::new(Cell::new(false));
        let provider = Rc::new(EncodingProvider { correct_encoding: correct_encoding.clone() });

        let query: ITextQuery = ITextQuery::new(
            vec![IFolderQuery { file_encoding: Some("windows1252".to_owned()), ..IFolderQuery::<URI>::new(uri_file("/some/folder")) }],
            PatternInfo::new("a"),
        );

        let m = NativeTextSearchManager::new(query, provider);
        m.search(Rc::new(|_: Vec<IFileMatch>| {}), &CancellationToken::None, None).await.unwrap();

        assert!(correct_encoding.get());
    });
}

#[test]
fn handles_result_from_unmatched_folder_gracefully_via_optional_chaining() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        let received_results = Rc::new(Cell::new(0usize));

        let query: ITextQuery = ITextQuery::new(vec![IFolderQuery::<URI>::new(uri_file("/folder1"))], PatternInfo::new("a"));

        let m = NativeTextSearchManager::new(query, Rc::new(UnmatchedFolderProvider));
        // This should not throw even though a result from an unmatched folder was reported
        let received = received_results.clone();
        m.search(Rc::new(move |results: Vec<IFileMatch>| received.set(received.get() + results.len())), &CancellationToken::None, None).await.unwrap();

        // Should only receive 1 result (the one from /folder1)
        // The result from /unknown/folder should be silently ignored
        assert_eq!(received_results.get(), 1);
    });
}
