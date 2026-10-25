//! Porte de `src/vs/workbench/services/search/test/node/textSearch.integrationTest.ts`.
//!
//! Notas de tradução:
//! - A suíte `TextSearch-integration` roda o `rg` de verdade sobre `tests/fixtures/search/node/fixtures` (cópia intacta da
//!   pasta `fixtures` do oráculo). O binário é o do oráculo (`ref/oracle`) ou o `rg` do `PATH`, como no porte do
//!   `rawSearchService.integrationTest.ts`.
//! - `doSearchTest` é [`do_search_test`]: soma o `numMatches` de cada lote, junta os lotes e confere a contagem. Quando a
//!   busca rejeita, o `.then` do original não roda, e o `?` devolve o erro antes da conferência.
//! - Os casos sem `type` no original (`config: any`) são o mesmo [`ITextQuery`], porque o tipo de consulta é a própria estrutura.
//! - `makeExpression(...)` é [`make_expression`]; `{ when: '$(basename).less' }` é `ExprValue::When`.
//! - O `.then(() => { throw new Error('expected fail') }, err => ...)` dos casos de `error messages` é `expect_err`.
//! - O caso dos links simbólicos usa `tempfile::tempdir` no lugar de `getRandomTestPath`, que apaga a pasta ao sair (o
//!   `fs.promises.rm` do `finally`); o original o pula no Windows, e o porte só o compila onde há link simbólico.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/search_integration_support.rs"]
mod search_integration_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_with_real_timers;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::glob::{ExprValue, Expression};
use je_platform::resources::uri_file;
use je_workbench::services::search::search::{
    ExcludeGlobPattern, IFolderQuery, IProgressMessage, ISearchRange, ISerializedFileMatch, ITextQuery, ITextSearchContext, ITextSearchResult, PatternInfo, SearchErrorCode,
    deserialize_search_error,
};
use je_workbench::services::search::text_search_adapter::TextSearchEngineAdapter;
use search_integration_support::{make_expression, test_fixtures, use_oracle_ripgrep};

fn examples_fixtures() -> String {
    format!("{}/examples", test_fixtures())
}

fn more_fixtures() -> String {
    format!("{}/more", test_fixtures())
}

/// `ROOT_FOLDER_QUERY`.
fn root_folder_query() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&test_fixtures()))]
}

/// `MULTIROOT_QUERIES`.
fn multiroot_queries() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&examples_fixtures())), IFolderQuery::new(uri_file(&more_fixtures()))]
}

fn reg_exp(pattern: &str) -> PatternInfo {
    PatternInfo { is_reg_exp: Some(true), ..PatternInfo::new(pattern) }
}

fn root_query(content_pattern: PatternInfo) -> ITextQuery {
    ITextQuery::new(root_folder_query(), content_pattern)
}

/// `doSearchTest(query, expectedResultCount)`.
async fn do_search_test(query: ITextQuery, expected_result_count: i64) -> Result<Vec<ISerializedFileMatch>, Error> {
    let engine = TextSearchEngineAdapter::new(query, None);

    let count = Rc::new(Cell::new(0_i64));
    let results: Rc<RefCell<Vec<ISerializedFileMatch>>> = Rc::default();
    let on_result: Rc<dyn Fn(Vec<ISerializedFileMatch>)> = {
        let (count, results) = (count.clone(), results.clone());
        Rc::new(move |batch: Vec<ISerializedFileMatch>| {
            count.set(count.get() + batch.iter().map(|file_match| file_match.num_matches.expect("numMatches")).sum::<i64>());
            results.borrow_mut().extend(batch);
        })
    };
    let on_message: Rc<dyn Fn(IProgressMessage)> = Rc::new(|_| {});

    let source = CancellationTokenSource::new(None);
    engine.search(&source.token(), on_result, on_message).await?;

    assert_eq!(count.get(), expected_result_count, "rg {} !== {expected_result_count}", count.get());
    let found = results.borrow().clone();
    Ok(found)
}

/// Um caso que só confere a contagem de casamentos.
fn assert_count(query: ITextQuery, expected_result_count: i64) {
    use_oracle_ripgrep();
    run_with_real_timers(async move {
        do_search_test(query, expected_result_count).await.expect("a busca terminou");
    });
}

fn context_at(result: &ITextSearchResult) -> &ITextSearchContext {
    match result {
        ITextSearchResult::Context(context) => context,
        other => panic!("contexto esperado: {other:?}"),
    }
}

#[test]
fn text_game_of_life() {
    assert_count(root_query(PatternInfo::new("GameOfLife")), 4);
}

#[test]
fn text_game_of_life_reg_exp() {
    assert_count(root_query(reg_exp("Game.?fL\\w?fe")), 4);
}

#[test]
fn text_game_of_life_unicode_escape_sequences() {
    assert_count(root_query(reg_exp("G\\u{0061}m\\u0065OfLife")), 4);
}

#[test]
fn text_game_of_life_pcre2_lookbehind() {
    assert_count(root_query(reg_exp("(?<!a)G\\u{0061}m\\u0065OfLife")), 4);
}

#[test]
fn text_game_of_life_pcre2_reg_exp() {
    assert_count(root_query(reg_exp("Life(?!P)")), 8);
}

#[test]
fn text_game_of_life_reg_exp_to_eol() {
    assert_count(root_query(reg_exp("GameOfLife.*")), 4);
}

#[test]
fn text_game_of_life_word_match_case_sensitive() {
    assert_count(root_query(PatternInfo { is_word_match: Some(true), is_case_sensitive: Some(true), ..PatternInfo::new("GameOfLife") }), 4);
}

#[test]
fn text_game_of_life_word_match_spaces() {
    assert_count(root_query(PatternInfo { is_word_match: Some(true), ..PatternInfo::new(" GameOfLife ") }), 1);
}

#[test]
fn text_game_of_life_word_match_punctuation_and_spaces() {
    assert_count(root_query(PatternInfo { is_word_match: Some(true), ..PatternInfo::new(", as =") }), 1);
}

#[test]
fn text_helvetica_utf_16() {
    assert_count(root_query(PatternInfo::new("Helvetica")), 3);
}

#[test]
fn text_e() {
    assert_count(root_query(PatternInfo::new("e")), 785);
}

#[test]
fn text_e_with_excludes() {
    let mut query = root_query(PatternInfo::new("e"));
    query.common.exclude_pattern = Some(Expression::from_entries([("**/examples", true)]));
    assert_count(query, 391);
}

#[test]
fn text_e_with_includes() {
    let mut query = root_query(PatternInfo::new("e"));
    query.common.include_pattern = Some(Expression::from_entries([("**/examples/**", true)]));
    assert_count(query, 394);
}

// Os casos `Text: e (with absolute path excludes)` e `Text: e (with mixed absolute/relative path excludes)` estão
// comentados (TODO) no original e por isso não têm `#[test]` aqui.

#[test]
fn text_sibling_exclude() {
    let mut query = root_query(PatternInfo::new("m"));
    query.common.include_pattern = Some(make_expression(&["**/site*"]));
    query.common.exclude_pattern = Some(Expression::from_entries([("*.css", ExprValue::When("$(basename).less".to_owned()))]));
    assert_count(query, 1);
}

#[test]
fn text_e_with_includes_and_exclude() {
    let mut query = root_query(PatternInfo::new("e"));
    query.common.include_pattern = Some(Expression::from_entries([("**/examples/**", true)]));
    query.common.exclude_pattern = Some(Expression::from_entries([("**/examples/small.js", true)]));
    assert_count(query, 371);
}

#[test]
fn text_a_capped() {
    let max_results = 520;
    let mut query = root_query(PatternInfo::new("a"));
    query.common.max_results = Some(max_results);
    assert_count(query, max_results);
}

#[test]
fn text_a_no_results() {
    assert_count(root_query(PatternInfo::new("ahsogehtdas")), 0);
}

#[test]
fn text_size() {
    assert_count(root_query(PatternInfo::new("-size")), 9);
}

#[test]
fn multiroot_conway() {
    assert_count(ITextQuery::new(multiroot_queries(), PatternInfo::new("conway")), 8);
}

#[test]
fn multiroot_e_with_partial_global_exclude() {
    let mut query = ITextQuery::new(multiroot_queries(), PatternInfo::new("e"));
    query.common.exclude_pattern = Some(make_expression(&["**/*.txt"]));
    assert_count(query, 394);
}

#[test]
fn multiroot_e_with_global_excludes() {
    let mut query = ITextQuery::new(multiroot_queries(), PatternInfo::new("e"));
    query.common.exclude_pattern = Some(make_expression(&["**/*.txt", "**/*.js"]));
    assert_count(query, 0);
}

#[test]
fn multiroot_e_with_folder_exclude() {
    let mut examples = IFolderQuery::new(uri_file(&examples_fixtures()));
    examples.exclude_pattern = Some(vec![ExcludeGlobPattern { folder: None, pattern: make_expression(&["**/e*.js"]) }]);
    let query = ITextQuery::new(vec![examples, IFolderQuery::new(uri_file(&more_fixtures()))], PatternInfo::new("e"));
    assert_count(query, 298);
}

#[test]
fn text_chinese_character() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let results = do_search_test(root_query(PatternInfo::new("语")), 1).await.expect("a busca terminou");
        let ITextSearchResult::Match(first) = &results[0].results.as_ref().expect("results")[0] else {
            panic!("casamento esperado");
        };
        let match_range: Vec<ISearchRange> = first.range_locations.iter().map(|location| location.source).collect();
        assert_eq!(match_range, vec![ISearchRange::new(0, 1, 0, 2)]);
    });
}

#[test]
fn multiple_matches_on_line_h_d_comma() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let results = do_search_test(root_query(reg_exp("h\\d,")), 15).await.expect("a busca terminou");
        assert_eq!(results.len(), 3);
        let first_results = results[0].results.as_ref().expect("results");
        assert_eq!(first_results.len(), 1);
        let ITextSearchResult::Match(first) = &first_results[0] else {
            panic!("casamento esperado");
        };
        let ranges: Vec<ISearchRange> = first.range_locations.iter().map(|location| location.source).collect();
        assert_eq!(ranges.len(), 5);
    });
}

#[test]
fn search_with_context_matches() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let mut query = root_query(PatternInfo::new("compiler.typeCheck();"));
        query.surrounding_context = Some(1);

        let results = do_search_test(query, 3).await.expect("a busca terminou");
        assert_eq!(results.len(), 1);
        let first_results = results[0].results.as_ref().expect("results");
        assert_eq!(first_results.len(), 3);
        assert_eq!(context_at(&first_results[0]).line_number, 24);
        assert_eq!(context_at(&first_results[0]).text, "        compiler.addUnit(prog,\"input.ts\");");
        assert_eq!(context_at(&first_results[2]).line_number, 26);
        assert_eq!(context_at(&first_results[2]).text, "        compiler.emit();");
    });
}

/// Criar links simbólicos exige privilégio elevado no Windows, onde o original pula o caso.
#[cfg(unix)]
#[test]
fn text_stops_following_symbolic_links_that_form_too_many_loops_143060() {
    use std::collections::HashSet;

    use je_workbench::services::search::search_ext_types::TextSearchCompleteMessageType;

    use_oracle_ripgrep();
    run_with_real_timers(async {
        let root = tempfile::tempdir().expect("pasta temporária");
        let files = search_integration_support::create_cross_linked_folders(root.path(), 8, "needle");
        let query = ITextQuery::new(vec![IFolderQuery::new(uri_file(&root.path().to_string_lossy()))], PatternInfo::new("needle"));
        let engine = TextSearchEngineAdapter::new(query, None);

        let paths: Rc<RefCell<Vec<String>>> = Rc::default();
        let on_result: Rc<dyn Fn(Vec<ISerializedFileMatch>)> = {
            let paths = paths.clone();
            Rc::new(move |matches: Vec<ISerializedFileMatch>| paths.borrow_mut().extend(matches.into_iter().map(|file_match| file_match.path)))
        };
        let on_message: Rc<dyn Fn(IProgressMessage)> = Rc::new(|_| {});
        let source = CancellationTokenSource::new(None);
        let complete = engine.search(&source.token(), on_result, on_message).await.expect("a busca terminou");

        let paths = paths.borrow();
        let unique: HashSet<&String> = paths.iter().collect();
        let actual = (
            unique.len() != paths.len(),
            files.iter().filter(|file| !paths.contains(file)).cloned().collect::<Vec<String>>(),
            complete.messages.iter().map(|message| message.message_type).collect::<Vec<TextSearchCompleteMessageType>>(),
        );
        assert_eq!(actual, (false, Vec::<String>::new(), vec![TextSearchCompleteMessageType::Warning]));
    });
}

#[test]
fn error_messages_invalid_encoding() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let mut folder = IFolderQuery::new(uri_file(&test_fixtures()));
        folder.file_encoding = Some("invalidEncoding".to_owned());
        let query = ITextQuery::new(vec![folder], PatternInfo::new("test"));

        let err = do_search_test(query, 0).await.expect_err("expected fail");
        let search_error = deserialize_search_error(&err);
        assert_eq!(search_error.message, "Unknown encoding: invalidEncoding");
        assert_eq!(search_error.code, Some(SearchErrorCode::UnknownEncoding));
    });
}

#[test]
fn error_messages_invalid_regex_case_1() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let err = do_search_test(root_query(reg_exp(")")), 0).await.expect_err("expected fail");
        let search_error = deserialize_search_error(&err);
        assert_eq!(search_error.message, "Regex parse error: unmatched closing parenthesis");
        assert_eq!(search_error.code, Some(SearchErrorCode::RegexParseError));
    });
}

#[test]
fn error_messages_invalid_regex_case_2() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let err = do_search_test(root_query(reg_exp("(?<!a.*)")), 0).await.expect_err("expected fail");
        let search_error = deserialize_search_error(&err);
        assert_eq!(search_error.message, "Regex parse error: length of lookbehind assertion is not limited");
        assert_eq!(search_error.code, Some(SearchErrorCode::RegexParseError));
    });
}
