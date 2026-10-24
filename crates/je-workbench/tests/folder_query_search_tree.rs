//! Porte de `src/vs/workbench/services/search/test/common/folderQuerySearchTree.test.ts` (suíte
//! `FolderQuerySearchTree`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `deepStrictEqual` vira `assert_eq!` sobre tipos que derivam `PartialEq`; o `undefined` é `None`.
//! - `getFolderQueryInfo` devolve `{ folder, index }`, aqui a struct `FolderQueryInfo`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uri::URI;
use je_workbench::services::search::folder_query_search_tree::FolderQuerySearchTree;
use je_workbench::services::search::search::IFolderQuery;

#[derive(Clone, Debug, PartialEq)]
struct FolderQueryInfo {
    folder: URI,
    index: usize,
}

fn fq(uri: &str) -> IFolderQuery<URI> {
    IFolderQuery::new(URI::parse(uri, false))
}

fn fq1() -> IFolderQuery<URI> {
    fq("file:///folder1?query1#fragment1")
}

fn fq2() -> IFolderQuery<URI> {
    fq("file:///folder2?query2#fragment2")
}

fn fq3() -> IFolderQuery<URI> {
    fq("file:///folder3?query3#fragment3")
}

fn fq4() -> IFolderQuery<URI> {
    fq("file:///folder3?query3")
}

fn fq5() -> IFolderQuery<URI> {
    fq("file:///folder3")
}

fn folder_queries() -> Vec<IFolderQuery<URI>> {
    vec![fq1(), fq2(), fq3(), fq4(), fq5()]
}

fn get_folder_query_info(fq: &IFolderQuery<URI>, i: usize) -> FolderQueryInfo {
    FolderQueryInfo { folder: fq.folder.clone(), index: i }
}

fn info(fq: &IFolderQuery<URI>, index: usize) -> FolderQueryInfo {
    FolderQueryInfo { folder: fq.folder.clone(), index }
}

#[test]
fn find_query_fragment_aware_substr_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let tree = FolderQuerySearchTree::new(&folder_queries(), get_folder_query_info, None);
    let result = tree.find_query_fragment_aware_substr(&fq1().folder);
    let result2 = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder1/foo/bar?query1#fragment1", false));
    assert_eq!(result, Some(&info(&fq1(), 0)));
    assert_eq!(result2, Some(&info(&fq1(), 0)));
}

#[test]
fn do_not_to_uris_that_do_not_have_queries_if_the_base_has_query() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let tree = FolderQuerySearchTree::new(&folder_queries(), get_folder_query_info, None);
    let result = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder1", false));
    let result2 = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder1?query1", false));
    assert_eq!(result, None);
    assert_eq!(result2, None);
}

#[test]
fn match_correct_entry_with_query_fragment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let tree = FolderQuerySearchTree::new(&folder_queries(), get_folder_query_info, None);
    let result = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder3/file.txt?query3#fragment3", false));
    assert_eq!(result, Some(&info(&fq3(), 2)));

    let result2 = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder3/file.txt?query3", false));
    assert_eq!(result2, Some(&info(&fq4(), 3)));

    let result3 = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder3/file.txt", false));
    assert_eq!(result3, Some(&info(&fq5(), 4)));
}

#[test]
fn can_find_substr_of_non_query_fragment_uris() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let tree = FolderQuerySearchTree::new(&folder_queries(), get_folder_query_info, None);
    let result = tree.find_query_fragment_aware_substr(&fq5().folder);
    let result2 = tree.find_query_fragment_aware_substr(&URI::parse("file:///folder3/hello/world", false));
    assert_eq!(result, Some(&info(&fq5(), 4)));
    assert_eq!(result2, Some(&info(&fq5(), 4)));
}

#[test]
fn iterate_over_all_folder_query_info_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let queries = folder_queries();
    let tree = FolderQuerySearchTree::new(&queries, get_folder_query_info, None);
    let mut results: Vec<FolderQueryInfo> = Vec::new();
    tree.for_each_folder_query_info(|info| results.push(info.clone()));
    assert_eq!(results.len(), 5);
    let expected: Vec<FolderQueryInfo> =
        queries.iter().enumerate().map(|(i, fq)| get_folder_query_info(fq, i)).collect();
    assert_eq!(results, expected);
}

#[test]
fn slash_as_a_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let trie = FolderQuerySearchTree::new(&[fq("memfs:/?q=1")], get_folder_query_info, None);

    assert_eq!(
        trie.find_query_fragment_aware_substr(&URI::parse("memfs:/file.txt?q=1", false)),
        Some(&FolderQueryInfo { folder: URI::parse("memfs:/?q=1", false), index: 0 })
    );
}
