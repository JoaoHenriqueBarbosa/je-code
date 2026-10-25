//! Porte de `src/vs/workbench/services/search/test/node/fileSearch.integrationTest.ts`.
//!
//! Notas de tradução:
//! - A suíte `FileSearch-integration` roda o `SearchService` com o `rg` de verdade sobre `tests/fixtures/search/node/fixtures` e
//!   `fixtures2` (cópias intactas das pastas do oráculo). O binário é o do oráculo (`ref/oracle`) ou o `rg` do `PATH`, como no
//!   porte do `rawSearchService.integrationTest.ts`.
//! - `doSearchTest` é [`do_search_test`]: junta o que o `progressCallback` entrega, descartando as mensagens de progresso
//!   (`isProgressMessage`), seja um resultado só ou um lote (`Array.isArray`), e confere a contagem. `numThreads` indefinido é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/search_integration_support.rs"]
mod search_integration_support;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_with_real_timers;
use je_base::glob::{ExprValue, Expression};
use je_platform::resources::uri_file;
use je_workbench::services::search::raw_search_service::{IProgressCallback, SearchService};
use je_workbench::services::search::search::{FileSearchStatsType, IFileQuery, IFolderQuery, ISerializedSearchProgressItem};
use search_integration_support::{make_expression, test_fixtures, test_fixtures2, use_oracle_ripgrep};

/// `ROOT_FOLDER_QUERY`.
fn root_folder_query() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&test_fixtures()))]
}

/// `MULTIROOT_QUERIES`.
fn multiroot_queries() -> Vec<IFolderQuery> {
    let mut examples = IFolderQuery::new(uri_file(&format!("{}/examples", test_fixtures())));
    examples.folder_name = Some("examples_folder".to_owned());
    vec![examples, IFolderQuery::new(uri_file(&format!("{}/more", test_fixtures())))]
}

/// `doSearchTest(query, expectedResultCount)`.
fn do_search_test(query: IFileQuery, expected_result_count: usize) {
    use_oracle_ripgrep();
    run_with_real_timers(async move {
        let svc = SearchService::new(FileSearchStatsType::SearchProcess, None);

        let count = Rc::new(RefCell::new(0_usize));
        let progress_callback: IProgressCallback = {
            let count = count.clone();
            Rc::new(move |item: ISerializedSearchProgressItem| match item {
                ISerializedSearchProgressItem::Message(_) => {}
                ISerializedSearchProgressItem::FileMatches(matches) => *count.borrow_mut() += matches.len(),
                ISerializedSearchProgressItem::FileMatch(_) => *count.borrow_mut() += 1,
            })
        };
        svc.do_file_search(&query, None, progress_callback, None).await.expect("a busca terminou");

        let results = *count.borrow();
        assert_eq!(results, expected_result_count, "rg {results} !== {expected_result_count}");
    });
}

#[test]
fn file_simple() {
    do_search_test(IFileQuery::new(root_folder_query()), 14);
}

#[test]
fn file_filepattern() {
    let mut query = IFileQuery::new(root_folder_query());
    query.file_pattern = Some("anotherfile".to_owned());
    do_search_test(query, 1);
}

#[test]
fn file_exclude() {
    let mut query = IFileQuery::new(root_folder_query());
    query.file_pattern = Some("file".to_owned());
    query.common.exclude_pattern = Some(make_expression(&["**/anotherfolder/**"]));
    do_search_test(query, 2);
}

#[test]
fn file_multiroot() {
    let mut query = IFileQuery::new(multiroot_queries());
    query.file_pattern = Some("file".to_owned());
    query.common.exclude_pattern = Some(make_expression(&["**/anotherfolder/**"]));
    do_search_test(query, 2);
}

#[test]
fn file_multiroot_with_folder_name() {
    let mut query = IFileQuery::new(multiroot_queries());
    query.file_pattern = Some("examples_folder anotherfile".to_owned());
    do_search_test(query, 1);
}

#[test]
fn file_multiroot_with_folder_name_and_sibling_exclude() {
    let mut folder1 = IFolderQuery::new(uri_file(&test_fixtures()));
    folder1.folder_name = Some("folder1".to_owned());
    let mut query = IFileQuery::new(vec![folder1, IFolderQuery::new(uri_file(&test_fixtures2()))]);
    query.file_pattern = Some("folder1 site".to_owned());
    query.common.exclude_pattern = Some(Expression::from_entries([("*.css", ExprValue::When("$(basename).less".to_owned()))]));
    do_search_test(query, 1);
}
