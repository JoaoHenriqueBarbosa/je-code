//! Porte de `src/vs/workbench/contrib/search/test/browser/searchViewlet.test.ts` (suíte `Search - Viewlet`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O guarda de vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`) é o [`LeakGuard`] criado no
//!   começo de cada caso, e o `TestInstantiationService` é o [`Fixture`] de `support/search_support.rs` (os mesmos serviços do
//!   `setup` da suíte `SearchResult`, que reaproveita o mesmo arquivo). O `FileService`, o `UriIdentityService` e o
//!   `MockLabelService` do `setup` do original estão nele.
//! - **`Data Source`.** O resultado do caso tem o `previewText` e o `rangeLocations` escritos à mão no original; aqui o
//!   [`TextSearchMatch`] é montado com os mesmos campos (`preview` e `source`), sem passar pelo construtor.
//! - **`aFileMatch(path, parentFolder?)`.** É [`Fixture::a_file_match_in_folder`], com o `aFolderMatch('', 0)` como pasta padrão.
//! - **`aFolderMatch(path, index, parent?)`.** É [`Fixture::a_folder_match`] (ver as notas do módulo de suporte).
//! - **`new MatchImpl(fileMatch, ['bar'], range, range, false)`** é [`MatchImpl::new`] sobre o arquivo como `ISearchTreeFileMatch`.
//! - **`sortOrder` padrão.** O argumento omitido do original é [`SearchSortOrder::Default`] em cada chamada.

mod support;

use std::rc::Rc;

use je_base::js_string::JsString;
use je_platform::resources::uri_file;
use je_workbench::contrib::search::browser::search_compare::{search_comparer, search_match_comparer};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::INotebookFileInstanceMatch;
use je_workbench::contrib::search::search_tree_model::r#match::MatchImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{
    FILE_MATCH_PREFIX, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch, ITextSearchHeading, MATCH_PREFIX, RenderableMatch,
};
use je_workbench::services::search::search::{
    IFileMatch, ITextSearchResult, OneLineRange, SearchRange, SearchRangeSetPairing, SearchSortOrder, TextSearchMatch,
};
use support::search_support::{
    Fixture, LeakGuard, create_file_uri_from_path_from_root, ensure_no_disposables_are_leaked_in_test_suite, get_root_name, root_query_with_pattern, setup,
};

/// `aFileMatch(path, parentFolder?)` da suíte `Search - Viewlet`.
fn a_file_match(t: &Fixture, guard: &LeakGuard, path: &str, parent_folder: Option<Rc<dyn ISearchTreeFolderMatch>>) -> Rc<dyn INotebookFileInstanceMatch> {
    let parent_folder = parent_folder.unwrap_or_else(|| t.a_folder_match(guard, "", 0, None));
    t.a_file_match_in_folder(guard, path, parent_folder, Vec::new())
}

fn file_element(file_match: &Rc<dyn INotebookFileInstanceMatch>) -> RenderableMatch {
    RenderableMatch::FileMatch(file_match.clone())
}

/// `new MatchImpl(fileMatch, [text], previewRange, documentRange, false)`.
fn a_line_match(file_match: &Rc<dyn INotebookFileInstanceMatch>, text: &str, preview_range: SearchRange, document_range: SearchRange) -> RenderableMatch {
    let parent: Rc<dyn ISearchTreeFileMatch> = file_match.clone();
    let line_match: Rc<dyn ISearchTreeMatch> = MatchImpl::new(&parent, vec![JsString::from(text)], preview_range, document_range, false);
    RenderableMatch::Match(line_match)
}

#[test]
fn data_source() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let result = t.a_search_result(&guard);
    result.set_query(Some(root_query_with_pattern("foo")));

    let raw_result = TextSearchMatch {
        preview_text: JsString::from("bar"),
        range_locations: vec![SearchRangeSetPairing {
            preview: SearchRange::new(0, 0, 0, 1),
            source: SearchRange::new(1, 0, 1, 1),
        }],
        webview_index: None,
    };
    result.add(
        &[IFileMatch::new(create_file_uri_from_path_from_root(Some("/foo")), Some(vec![ITextSearchResult::Match(raw_result)]))],
        "",
        false,
        false,
    );

    let file_match = result.matches(None)[0].clone();
    let line_match = file_match.matches()[0].clone();

    let foo_uri = uri_file(&format!("{}/foo", get_root_name())).to_string(false);
    assert_eq!(file_match.id(), format!("{FILE_MATCH_PREFIX}{foo_uri}"));
    assert_eq!(line_match.id(), format!("{MATCH_PREFIX}{foo_uri}>[2,1 -> 2,2]b"));
}

#[test]
fn comparer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let file_match1 = a_file_match(&t, &guard, "/foo", None);
    let file_match2 = a_file_match(&t, &guard, "/with/path", None);
    let file_match3 = a_file_match(&t, &guard, "/with/path/foo", None);
    let line_match1 = a_line_match(&file_match1, "bar", OneLineRange::new(0, 1, 1), OneLineRange::new(0, 1, 1));
    let line_match2 = a_line_match(&file_match1, "bar", OneLineRange::new(0, 1, 1), OneLineRange::new(2, 1, 1));
    let line_match3 = a_line_match(&file_match1, "bar", OneLineRange::new(0, 1, 1), OneLineRange::new(2, 1, 1));

    let default = SearchSortOrder::Default;
    assert!(search_match_comparer(&file_element(&file_match1), &file_element(&file_match2), default) < 0);
    assert!(search_match_comparer(&file_element(&file_match2), &file_element(&file_match1), default) > 0);
    assert!(search_match_comparer(&file_element(&file_match1), &file_element(&file_match1), default) == 0);
    assert!(search_match_comparer(&file_element(&file_match2), &file_element(&file_match3), default) < 0);

    assert!(search_match_comparer(&line_match1, &line_match2, default) < 0);
    assert!(search_match_comparer(&line_match2, &line_match1, default) > 0);
    assert!(search_match_comparer(&line_match2, &line_match3, default) == 0);
}

#[test]
fn advanced_comparer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let file_match1 = file_element(&a_file_match(&t, &guard, "/with/path/foo10", None));
    let file_match2 = file_element(&a_file_match(&t, &guard, "/with/path2/foo1", None));
    let file_match3 = file_element(&a_file_match(&t, &guard, "/with/path/bar.a", None));
    let file_match4 = file_element(&a_file_match(&t, &guard, "/with/path/bar.b", None));

    // By default, path < path2
    assert!(search_match_comparer(&file_match1, &file_match2, SearchSortOrder::Default) < 0);
    // By filenames, foo10 > foo1
    assert!(search_match_comparer(&file_match1, &file_match2, SearchSortOrder::FileNames) > 0);
    // By type, bar.a < bar.b
    assert!(search_match_comparer(&file_match3, &file_match4, SearchSortOrder::Type) < 0);
}

#[test]
fn cross_type_comparer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);

    let search_result = t.a_search_result(&guard);
    let heading: Rc<dyn ITextSearchHeading> = search_result.plain_text_search_result();
    let folder_match1 = t.a_folder_match(&guard, "/voo", 0, Some(heading.clone()));
    let folder_match2 = t.a_folder_match(&guard, "/with", 1, Some(heading));

    let file_match1 = a_file_match(&t, &guard, "/voo/foo.a", Some(folder_match1.clone()));
    let file_match2 = a_file_match(&t, &guard, "/with/path.c", Some(folder_match2.clone()));
    let file_match3 = a_file_match(&t, &guard, "/with/path/bar.b", Some(folder_match2.clone()));

    let line_match1 = a_line_match(&file_match1, "bar", OneLineRange::new(0, 1, 1), OneLineRange::new(0, 1, 1));
    let line_match2 = a_line_match(&file_match1, "bar", OneLineRange::new(0, 1, 1), OneLineRange::new(2, 1, 1));

    let line_match3 = a_line_match(&file_match2, "barfoo", OneLineRange::new(0, 1, 1), OneLineRange::new(0, 1, 1));
    let line_match4 = a_line_match(&file_match2, "fooooo", OneLineRange::new(0, 1, 1), OneLineRange::new(2, 1, 1));

    let line_match5 = a_line_match(&file_match3, "foobar", OneLineRange::new(0, 1, 1), OneLineRange::new(2, 1, 1));

    /***
     * Structure would take the following form:
     *
     *	folderMatch1 (voo)
     *		> fileMatch1 (/foo.a)
     *			>> lineMatch1
     *			>> lineMatch2
     *	folderMatch2 (with)
     *		> fileMatch2 (/path.c)
     *			>> lineMatch4
     *			>> lineMatch5
     *		> fileMatch3 (/path/bar.b)
     *			>> lineMatch3
     *
     */

    let folder_element2 = RenderableMatch::FolderMatch(folder_match2);
    let (file_element1, file_element2, file_element3) = (file_element(&file_match1), file_element(&file_match2), file_element(&file_match3));
    let default = SearchSortOrder::Default;

    // for these, refer to diagram above
    assert!(search_comparer(&file_element1, &file_element3, default) < 0);
    assert!(search_comparer(&file_element2, &file_element3, default) < 0);
    assert!(search_comparer(&folder_element2, &file_element2, default) < 0);
    assert!(search_comparer(&line_match4, &line_match5, default) < 0);
    assert!(search_comparer(&line_match1, &line_match3, default) < 0);
    assert!(search_comparer(&line_match2, &folder_element2, default) < 0);

    // travel up hierarchy and order of folders take precedence. "voo < with" in indices
    assert!(search_comparer(&file_element1, &file_element3, SearchSortOrder::FileNames) < 0);
    // bar.b < path.c
    assert!(search_comparer(&file_element3, &file_element2, SearchSortOrder::FileNames) < 0);
    // lineMatch4's parent is fileMatch2, "bar.b < path.c"
    assert!(search_comparer(&file_element3, &line_match4, SearchSortOrder::FileNames) < 0);

    // bar.b < path.c
    assert!(search_comparer(&file_element3, &file_element2, SearchSortOrder::Type) < 0);
    // lineMatch4's parent is fileMatch2, "bar.b < path.c"
    assert!(search_comparer(&file_element3, &line_match4, SearchSortOrder::Type) < 0);
}
