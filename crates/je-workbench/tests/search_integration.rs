//! Porte de `src/vs/workbench/services/search/test/node/search.integrationTest.ts`.
//!
//! Notas de tradução:
//! - As suítes `FileSearchEngine` e `FileWalker` rodam sobre `tests/fixtures/search/node/fixtures` e `fixtures2` (cópias
//!   intactas das pastas do oráculo). O `rg` é o do oráculo (`ref/oracle`) ou o do `PATH`, como no porte do
//!   `rawSearchService.integrationTest.ts`; o `find` do `FileWalker` é o do sistema.
//! - `new FileSearchEngine(config)` é o [`Engine`] sobre `spawn_ripgrep_cmd(rg_disk_path())`, o mesmo que o `SearchService`
//!   monta, e o `engine.search(onResult, onProgress, done)` é [`run_engine`], que entrega o `done` como uma promessa. O
//!   `if (result) { count++ }` do original conta todo resultado, porque o `onResult` do porte nunca recebe `null`.
//! - `res` (o último resultado em que o `res = result` roda) é o último elemento de `results`.
//! - `path.basename(res.relativePath)` é [`basename`]; `assert.ok(!error)` é a conferência de `error` ser `None`.
//! - `walker.readStdout(cmd, 'utf8', cb)` é [`find_stdout`]: o `spawnFindCmd` e o `readStdout` públicos do `FileWalker`, com o
//!   `err` nulo conferido por `expect`. Os casos do `FileWalker` e o dos links simbólicos são pulados no Windows no
//!   original, e o porte só os compila fora dele.
//! - O caso dos links simbólicos usa `tempfile::tempdir` no lugar de `getRandomTestPath`, que apaga a pasta ao sair (o
//!   `fs.promises.rm` do `finally`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/search_integration_support.rs"]
mod search_integration_support;

use std::cell::RefCell;
use std::path::{MAIN_SEPARATOR_STR, Path};
use std::rc::Rc;

use common::run_with_real_timers;
use je_base::errors::Error;
use je_base::glob::{ExprValue, Expression};
use je_base::promise::Promise;
use je_platform::resources::{join_path, uri_file};
use je_workbench::services::search::file_walker::{Engine, FileWalker, ISearchEngine, ISearchEngineSuccess, OnMessage, OnResult, spawn_ripgrep_cmd};
use je_workbench::services::search::ripgrep_disk_path::rg_disk_path;
use je_workbench::services::search::search::{ExcludeGlobPattern, IFileQuery, IFolderQuery, IRawFileMatch};
use search_integration_support::{make_expression, test_fixtures, test_fixtures2, use_oracle_ripgrep};

fn examples_fixtures() -> String {
    format!("{}/examples", test_fixtures())
}

fn more_fixtures() -> String {
    format!("{}/more", test_fixtures())
}

/// `TEST_ROOT_FOLDER`.
fn test_root_folder() -> IFolderQuery {
    IFolderQuery::new(uri_file(&test_fixtures()))
}

/// `ROOT_FOLDER_QUERY`.
fn root_folder_query() -> Vec<IFolderQuery> {
    vec![test_root_folder()]
}

/// `ROOT_FOLDER_QUERY_36438`.
fn root_folder_query_36438() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&format!("{}/36438", test_fixtures2())))]
}

/// `MULTIROOT_QUERIES`.
fn multiroot_queries() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&examples_fixtures())), IFolderQuery::new(uri_file(&more_fixtures()))]
}

/// O que o `done` do `engine.search` recebe, mais os resultados que o `onResult` viu.
#[derive(Clone)]
struct EngineRun {
    results: Vec<IRawFileMatch>,
    error: Option<Error>,
    complete: ISearchEngineSuccess,
}

impl EngineRun {
    fn basenames(&self) -> Vec<String> {
        self.results.iter().map(|result| basename(&result.relative_path)).collect()
    }
}

/// `new FileSearchEngine(query).search(...)`.
fn run_engine(query: IFileQuery) -> Promise<EngineRun> {
    let (promise, resolver) = Promise::new();
    let engine = Rc::new(Engine::new(&query, None, spawn_ripgrep_cmd(rg_disk_path())));
    let results: Rc<RefCell<Vec<IRawFileMatch>>> = Rc::default();
    let on_result: OnResult = {
        let results = results.clone();
        Rc::new(move |result: &IRawFileMatch| results.borrow_mut().push(result.clone()))
    };
    let on_progress: OnMessage = Rc::new(|_| {});
    let keep_alive = engine.clone();
    engine.search(
        on_result,
        on_progress,
        Box::new(move |error, complete| {
            drop(keep_alive);
            let found = results.borrow().clone();
            resolver.resolve(EngineRun { results: found, error, complete });
        }),
    );
    promise
}

/// A busca terminada sem erro (`assert.ok(!error)`).
async fn search_engine(query: IFileQuery) -> EngineRun {
    let run = run_engine(query).await.expect("o motor terminou");
    assert!(run.error.is_none(), "{:?}", run.error);
    run
}

fn basename(path: &str) -> String {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn file_query(folder_queries: Vec<IFolderQuery>) -> IFileQuery {
    IFileQuery::new(folder_queries)
}

fn file_pattern_query(folder_queries: Vec<IFolderQuery>, file_pattern: &str) -> IFileQuery {
    IFileQuery { file_pattern: Some(file_pattern.to_owned()), ..file_query(folder_queries) }
}

/// Um caso que só confere quantos resultados chegaram.
fn assert_count(query: IFileQuery, expected: usize) {
    use_oracle_ripgrep();
    run_with_real_timers(async move {
        let run = search_engine(query).await;
        assert_eq!(run.results.len(), expected);
    });
}

/// Um caso de `exists`: nenhum resultado, e `limitHit` diz se o arquivo existe.
fn assert_exists(query: IFileQuery, expected_limit_hit: bool) {
    use_oracle_ripgrep();
    run_with_real_timers(async move {
        let run = search_engine(query).await;
        assert_eq!(run.results.len(), 0);
        assert_eq!(run.complete.limit_hit, expected_limit_hit);
    });
}

fn include_pattern(patterns: &[&str]) -> Expression {
    make_expression(patterns)
}

fn extra_file_resources() -> Vec<je_base::uri::URI> {
    vec![
        uri_file(&format!("{}/site.css", test_fixtures())),
        uri_file(&format!("{}/examples/company.js", test_fixtures())),
        uri_file(&format!("{}/index.html", test_fixtures())),
    ]
}

fn extra_files_query(file_pattern: &str) -> IFileQuery {
    let mut query = file_pattern_query(Vec::new(), file_pattern);
    query.common.extra_file_resources = Some(extra_file_resources());
    query
}

#[test]
fn files_star_js() {
    assert_count(file_pattern_query(root_folder_query(), "*.js"), 4);
}

#[test]
fn files_max_results() {
    let mut query = file_query(root_folder_query());
    query.common.max_results = Some(1);
    assert_count(query, 1);
}

#[test]
fn files_max_results_without_ripgrep() {
    let mut query = file_query(root_folder_query());
    query.common.max_results = Some(1);
    assert_count(query, 1);
}

#[test]
fn files_exists() {
    let mut query = file_query(root_folder_query());
    query.common.include_pattern = Some(include_pattern(&["**/file.txt"]));
    query.exists = Some(true);
    assert_exists(query, true);
}

#[test]
fn files_not_exists() {
    let mut query = file_query(root_folder_query());
    query.common.include_pattern = Some(include_pattern(&["**/nofile.txt"]));
    query.exists = Some(true);
    assert_exists(query, false);
}

#[test]
fn files_exists_without_ripgrep() {
    let mut query = file_query(root_folder_query());
    query.common.include_pattern = Some(include_pattern(&["**/file.txt"]));
    query.exists = Some(true);
    assert_exists(query, true);
}

#[test]
fn files_not_exists_without_ripgrep() {
    let mut query = file_query(root_folder_query());
    query.common.include_pattern = Some(include_pattern(&["**/nofile.txt"]));
    query.exists = Some(true);
    assert_exists(query, false);
}

#[test]
fn files_examples_com_star() {
    assert_count(file_pattern_query(root_folder_query(), &format!("examples{MAIN_SEPARATOR_STR}com*")), 1);
}

#[test]
fn files_examples_fuzzy() {
    assert_count(file_pattern_query(root_folder_query(), "xl"), 7);
}

#[test]
fn files_multiroot() {
    assert_count(file_pattern_query(multiroot_queries(), "file"), 3);
}

#[test]
fn files_multiroot_with_include_pattern_and_max_results() {
    let mut query = file_query(multiroot_queries());
    query.common.max_results = Some(1);
    query.common.include_pattern = Some(include_pattern(&["*.txt", "*.js"]));
    assert_count(query, 1);
}

#[test]
fn files_multiroot_with_include_pattern_and_exists() {
    let mut query = file_query(multiroot_queries());
    query.exists = Some(true);
    query.common.include_pattern = Some(include_pattern(&["*.txt", "*.js"]));
    assert_exists(query, true);
}

#[test]
fn files_npe_camel_case() {
    assert_count(file_pattern_query(root_folder_query(), "NullPE"), 1);
}

#[test]
fn files_star_dot_star() {
    assert_count(file_pattern_query(root_folder_query(), "*.*"), 14);
}

#[test]
fn files_star_dot_as() {
    assert_count(file_pattern_query(root_folder_query(), "*.as"), 0);
}

#[test]
fn files_star_dot_star_without_derived() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let mut query = file_pattern_query(root_folder_query(), "site.*");
        query.common.exclude_pattern = Some(Expression::from_entries([("**/*.css", ExprValue::When("$(basename).less".to_owned()))]));

        let run = search_engine(query).await;
        assert_eq!(run.results.len(), 1);
        assert_eq!(basename(&run.results.last().expect("res").relative_path), "site.less");
    });
}

#[test]
fn files_star_dot_star_exclude_folder_without_wildcard() {
    let mut query = file_pattern_query(root_folder_query(), "*.*");
    query.common.exclude_pattern = Some(include_pattern(&["examples"]));
    assert_count(query, 8);
}

#[test]
fn files_exclude_folder_without_wildcard_36438() {
    let mut query = file_query(root_folder_query_36438());
    query.common.exclude_pattern = Some(include_pattern(&["modules"]));
    assert_count(query, 1);
}

#[test]
fn files_include_folder_without_wildcard_36438() {
    let mut query = file_query(root_folder_query_36438());
    query.common.include_pattern = Some(include_pattern(&["modules/**"]));
    assert_count(query, 1);
}

/// Criar links simbólicos exige privilégio elevado no Windows, onde o original pula o caso.
#[cfg(unix)]
#[test]
fn files_stops_following_symbolic_links_that_form_too_many_loops_143060() {
    use std::collections::HashSet;

    use_oracle_ripgrep();
    run_with_real_timers(async {
        let root = tempfile::tempdir().expect("pasta temporária");
        let files = search_integration_support::create_cross_linked_folders(root.path(), 8, "");
        let query = file_query(vec![IFolderQuery::new(uri_file(&root.path().to_string_lossy()))]);

        let run = search_engine(query).await;
        let paths: Vec<String> = run.results.iter().map(|result| root.path().join(&result.relative_path).to_string_lossy().into_owned()).collect();
        let unique: HashSet<&String> = paths.iter().collect();
        let actual = (unique.len() != paths.len(), files.iter().filter(|file| !paths.contains(file)).cloned().collect::<Vec<String>>());
        assert_eq!(actual, (false, Vec::<String>::new()));
    });
}

#[test]
fn files_star_dot_star_exclude_folder_with_leading_wildcard() {
    let mut query = file_pattern_query(root_folder_query(), "*.*");
    query.common.exclude_pattern = Some(include_pattern(&["**/examples"]));
    assert_count(query, 8);
}

#[test]
fn files_star_dot_star_exclude_folder_with_trailing_wildcard() {
    let mut query = file_pattern_query(root_folder_query(), "*.*");
    query.common.exclude_pattern = Some(include_pattern(&["examples/**"]));
    assert_count(query, 8);
}

#[test]
fn files_star_dot_star_exclude_with_unicode() {
    let mut query = file_pattern_query(root_folder_query(), "*.*");
    query.common.exclude_pattern = Some(include_pattern(&["**/üm laut汉语"]));
    assert_count(query, 13);
}

#[test]
fn files_star_dot_star_include_with_unicode() {
    let mut query = file_pattern_query(root_folder_query(), "*.*");
    query.common.include_pattern = Some(include_pattern(&["**/üm laut汉语/*"]));
    assert_count(query, 1);
}

#[test]
fn files_multiroot_with_exclude() {
    let mut examples = IFolderQuery::new(uri_file(&examples_fixtures()));
    examples.exclude_pattern = Some(vec![ExcludeGlobPattern { folder: None, pattern: include_pattern(&["**/anotherfile.txt"]) }]);
    let mut more = IFolderQuery::new(uri_file(&more_fixtures()));
    more.exclude_pattern = Some(vec![ExcludeGlobPattern { folder: None, pattern: include_pattern(&["**/file.txt"]) }]);

    assert_count(file_pattern_query(vec![examples, more], "*"), 5);
}

#[test]
fn files_unicode_and_spaces() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let run = search_engine(file_pattern_query(root_folder_query(), "汉语")).await;
        assert_eq!(run.results.len(), 1);
        assert_eq!(basename(&run.results.last().expect("res").relative_path), "汉语.txt");
    });
}

#[test]
fn files_no_results() {
    assert_count(file_pattern_query(root_folder_query(), "nofilematch"), 0);
}

#[test]
fn files_relative_path_matched_once() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let run = search_engine(file_pattern_query(root_folder_query(), &format!("examples{MAIN_SEPARATOR_STR}company.js"))).await;
        assert_eq!(run.results.len(), 1);
        assert_eq!(basename(&run.results.last().expect("res").relative_path), "company.js");
    });
}

#[test]
fn files_include_pattern_single_files() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let mut query = file_query(root_folder_query());
        query.common.include_pattern = Some(include_pattern(&["site.css", "examples/company.js", "examples/subfolder/subfile.txt"]));

        let basenames = search_engine(query).await.basenames();
        for expected in ["site.css", "company.js", "subfile.txt"] {
            assert!(basenames.iter().any(|name| name == expected), "{expected} missing in {basenames:?}");
        }
    });
}

#[test]
fn files_extra_files_only() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let run = search_engine(extra_files_query("*.js")).await;
        assert_eq!(run.results.len(), 1);
        assert_eq!(basename(&run.results.last().expect("res").relative_path), "company.js");
    });
}

#[test]
fn files_extra_files_only_with_include() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let mut query = extra_files_query("*.*");
        query.common.include_pattern = Some(include_pattern(&["**/*.css"]));

        let run = search_engine(query).await;
        assert_eq!(run.results.len(), 1);
        assert_eq!(basename(&run.results.last().expect("res").relative_path), "site.css");
    });
}

#[test]
fn files_extra_files_only_with_exclude() {
    let mut query = extra_files_query("*.*");
    query.common.exclude_pattern = Some(include_pattern(&["**/*.css"]));
    assert_count(query, 2);
}

#[test]
fn files_no_dupes_in_nested_folders() {
    let examples = uri_file(&examples_fixtures());
    let folder_queries = vec![IFolderQuery::new(examples.clone()), IFolderQuery::new(join_path(&examples, &["subfolder"]))];
    assert_count(file_pattern_query(folder_queries, "subfile.txt"), 1);
}

// ---------------------------------------------------------------------------------------------------------
// FileWalker
// ---------------------------------------------------------------------------------------------------------

/// `walker.spawnFindCmd(folderQuery)` seguido de `walker.readStdout(cmd, 'utf8', cb)`: a saída do `find`, com `err` nulo.
async fn find_stdout(query: IFileQuery, folder_query: &IFolderQuery) -> String {
    let walker = FileWalker::new(&query, spawn_ripgrep_cmd(rg_disk_path()));
    let cmd = walker.spawn_find_cmd(folder_query);
    let (promise, resolver) = Promise::new();
    let keep_alive = walker.clone();
    walker.read_stdout(&cmd, move |result| {
        drop(keep_alive);
        match result {
            Ok(stdout) => resolver.resolve(stdout),
            Err(error) => resolver.reject(error),
        }
    });
    promise.await.expect("err nulo")
}

/// O `find` sobre a pasta de fixtures com `excludePattern` global.
async fn find_stdout_excluding(patterns: &[&str]) -> String {
    let mut query = file_query(root_folder_query());
    query.common.exclude_pattern = Some(make_expression(patterns));
    find_stdout(query, &test_root_folder()).await
}

fn output_contains(stdout: &str, file: &str) -> bool {
    stdout.split('\n').any(|line| line == file)
}

#[cfg(not(windows))]
#[test]
fn find_exclude_subfolder() {
    run_with_real_timers(async {
        let file0 = "./more/file.txt";
        let file1 = "./examples/subfolder/subfile.txt";

        let stdout1 = find_stdout_excluding(&["**/something"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(output_contains(&stdout1, file1), "{stdout1}");

        let stdout2 = find_stdout_excluding(&["**/subfolder"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout2, file1), "{stdout2}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_folder_excludes() {
    run_with_real_timers(async {
        let mut folder = test_root_folder();
        folder.exclude_pattern = Some(vec![ExcludeGlobPattern { folder: None, pattern: make_expression(&["**/subfolder"]) }]);
        let folder_queries = vec![folder];

        let file0 = "./more/file.txt";
        let file1 = "./examples/subfolder/subfile.txt";

        let stdout1 = find_stdout(file_query(folder_queries.clone()), &folder_queries[0]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout1, file1), "{stdout1}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_exclude_multiple_folders() {
    run_with_real_timers(async {
        let file0 = "./index.html";
        let file1 = "./examples/small.js";
        let file2 = "./more/file.txt";

        let stdout1 = find_stdout_excluding(&["**/something"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(output_contains(&stdout1, file1), "{stdout1}");
        assert!(output_contains(&stdout1, file2), "{stdout1}");

        let stdout2 = find_stdout_excluding(&["{**/examples,**/more}"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout2, file1), "{stdout2}");
        assert!(!output_contains(&stdout2, file2), "{stdout2}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_exclude_folder_path_suffix() {
    run_with_real_timers(async {
        let file0 = "./examples/company.js";
        let file1 = "./examples/subfolder/subfile.txt";

        let stdout1 = find_stdout_excluding(&["**/examples/something"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(output_contains(&stdout1, file1), "{stdout1}");

        let stdout2 = find_stdout_excluding(&["**/examples/subfolder"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout2, file1), "{stdout2}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_exclude_subfolder_path_suffix() {
    run_with_real_timers(async {
        let file0 = "./examples/subfolder/subfile.txt";
        let file1 = "./examples/subfolder/anotherfolder/anotherfile.txt";

        let stdout1 = find_stdout_excluding(&["**/subfolder/something"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(output_contains(&stdout1, file1), "{stdout1}");

        let stdout2 = find_stdout_excluding(&["**/subfolder/anotherfolder"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout2, file1), "{stdout2}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_exclude_folder_path() {
    run_with_real_timers(async {
        let file0 = "./examples/company.js";
        let file1 = "./examples/subfolder/subfile.txt";

        let stdout1 = find_stdout_excluding(&["examples/something"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(output_contains(&stdout1, file1), "{stdout1}");

        let stdout2 = find_stdout_excluding(&["examples/subfolder"]).await;
        assert!(output_contains(&stdout1, file0), "{stdout1}");
        assert!(!output_contains(&stdout2, file1), "{stdout2}");
    });
}

#[cfg(not(windows))]
#[test]
fn find_exclude_combination_of_paths() {
    run_with_real_timers(async {
        let files_in = ["./examples/subfolder/subfile.txt", "./examples/company.js", "./index.html"];
        let files_out = ["./examples/subfolder/anotherfolder/anotherfile.txt", "./more/file.txt"];

        let stdout1 = find_stdout_excluding(&["**/subfolder/anotherfolder", "**/something/else", "**/more", "**/andmore"]).await;
        for file_in in files_in {
            assert!(output_contains(&stdout1, file_in), "{stdout1}");
        }
        for file_out in files_out {
            assert!(!output_contains(&stdout1, file_out), "{stdout1}");
        }
    });
}
