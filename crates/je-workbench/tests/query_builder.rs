//! Porte de `src/vs/workbench/services/search/test/browser/queryBuilder.test.ts` (suíte `QueryBuilder`, com as
//! subsuítes `parseSearchPaths 1`, `parseSearchPaths 2`, `smartCase`, `file` e `pattern processing`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`.** Monta o `QueryBuilder` como o `TestInstantiationService` do original: o serviço de configuração, o de
//!   contexto de workspace e o de caminho são os registrados; os que o original deixa sem registro (`undefined`) são
//!   dublês vazios (grupos de editores, log) e o `IUriIdentityService` com o `ExtUri` sem viés.
//! - **`assertEqualQueries`.** Compara as pastas por `{ fsPath, excludePattern, includePattern, fileEncoding }`, o
//!   `extraFileResources` por `fsPath`, e o resto da consulta com `usingSearchPaths` apagado e as expressões
//!   normalizadas (barras e ordem das chaves, que o `deepStrictEqual` do JavaScript ignora).
//! - **`IExpression`.** `patternsToIExpression` é [`patterns_to_expression`]; as expressões são comparadas sem
//!   considerar a ordem das chaves.
//! - **`try`/`catch`.** O `throw` do `QueryBuilder` é um pânico com o `Error`, capturado com `errors::catch`.
//! - **`test.skip`.** Os dois casos `test.skip` do original ficam `#[ignore]`. O que eles esperam (uma lista de
//!   expressões e um `uri` no `excludePattern`) o próprio original marca como "TODO: Fix", e o tipo
//!   `ICommonQueryProps.excludePattern` (uma só expressão) não os representa.
//! - **`isWindows`.** O `fixPath` põe `c:` na frente no Windows; o caso `includes with tilde` é pulado lá.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::glob::{ExprValue, Expression};
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogServiceRef, NullLogger};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, ext_uri_biased_ignore_path_case, fs_path, host_environment, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, RawWorkspaceFolder, Workspace, WorkspaceFolder, new_test_workspace, to_workspace_folder};
use je_platform::workspaces::workspaces::to_workspace_folders;
use je_instantiation::Injectable;
use je_workbench::services::editor::editor_groups_service::IEditorGroupsService;
use je_workbench::services::search::query_builder::{
    ICommonQueryBuilderOptions, IFileQueryBuilderOptions, ISearchPathsInfo, ITextQueryBuilderOptions, QueryBuilder, SearchPathPatternBuilder,
    SearchPatternBuilder, WorkspaceFolderOrUri,
};
use je_workbench::services::search::search::{ExcludeGlobPattern, ICommonQueryProps, IFileQueryProps, IFolderQuery, ITextQuery, ITextQueryProps, PatternInfo};
use je_workbench::test::browser::workbench_test_services::{TestEditorGroupsService, TestPathService};
use je_workbench::test::common::workbench_test_services::TestContextService;

/// Um `IUriIdentityService` com o `ExtUri` sem viés e `asCanonicalUri` identidade.
struct TestUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `fixPath(...slashPathParts)`.
fn fix_path(parts: &[&str]) -> String {
    let env = host_environment();
    let native = PathModule::native(env.as_ref());
    if is_windows() && !parts.is_empty() && !parts[0].to_lowercase().starts_with("c:") {
        let mut with_drive = vec!["c:"];
        with_drive.extend_from_slice(parts);
        return native.join(&with_drive);
    }
    native.join(parts)
}

/// `getUri(...slashPathParts)`.
fn get_uri(parts: &[&str]) -> URI {
    uri_file(&fix_path(parts))
}

fn flag(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

/// `DEFAULT_USER_CONFIG`, com o `exclude` opcional.
fn user_config(exclude: Option<Dynamic>) -> Dynamic {
    let mut entries = vec![("useIgnoreFiles", flag(true)), ("useGlobalIgnoreFiles", flag(true)), ("useParentIgnoreFiles", flag(true))];
    if let Some(exclude) = exclude {
        entries.push(("exclude", exclude));
    }
    Dynamic::object(entries)
}

/// `patternsToIExpression(...patterns)`: o `undefined` do original quando não há padrões é `None`.
fn patterns_to_expression(patterns: &[&str]) -> Option<Expression> {
    (!patterns.is_empty()).then(|| Expression::from_entries(patterns.iter().map(|pattern| (*pattern, true))))
}

/// `patternsToIExpression(...patterns)` com ao menos um padrão.
fn expr(patterns: &[&str]) -> Expression {
    patterns_to_expression(patterns).expect("ao menos um padrão")
}

/// `globalGlob(pattern)`.
fn global_glob(pattern: &str) -> Vec<String> {
    vec![format!("**/{pattern}/**"), format!("**/{pattern}")]
}

fn expr_of_global_glob(pattern: &str) -> Expression {
    let globs = global_glob(pattern);
    expr(&globs.iter().map(String::as_str).collect::<Vec<_>>())
}

/// `normalizeExpression`: as barras invertidas viram barras e as chaves ficam em ordem (o `deepStrictEqual` do
/// JavaScript não olha a ordem das chaves).
fn normalize_expression(expression: Option<&Expression>) -> Option<Expression> {
    expression.map(|expression| {
        let mut entries: Vec<(String, ExprValue)> = expression.entries().iter().map(|(key, value)| (key.replace('\\', "/"), value.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Expression::from_entries(entries)
    })
}

/// A expressão sem a ordem das chaves, para os `deepStrictEqual` de `{ ...expression }`.
fn sorted_expression(expression: Option<&Expression>) -> Expression {
    let mut entries: Vec<(String, ExprValue)> = expression.map(|e| e.entries().to_vec()).unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Expression::from_entries(entries)
}

fn folder_for(uri: &URI) -> WorkspaceFolder {
    to_workspace_folder(uri, ext_uri().basename_or_authority(uri))
}

/// `new IFolderQuery` esperado: `{ folder }`.
fn fq(folder: &URI) -> IFolderQuery<URI> {
    IFolderQuery::new(folder.clone())
}

/// `{ folder, includePattern, excludePattern: [{ pattern }] }`.
fn fq_with(folder: &URI, include_pattern: Option<Expression>, exclude_pattern: Option<Expression>) -> IFolderQuery<URI> {
    let mut query = fq(folder);
    query.include_pattern = include_pattern;
    query.exclude_pattern = exclude_pattern.map(|pattern| vec![ExcludeGlobPattern { folder: None, pattern }]);
    query
}

/// `makeExcludePatternFromPatterns(...patterns)`.
fn make_exclude_pattern_from_patterns(patterns: &[&str]) -> Option<Expression> {
    patterns_to_expression(patterns)
}

#[derive(Debug, PartialEq)]
struct FolderCompare {
    path: String,
    exclude_pattern: Option<Vec<Option<Expression>>>,
    include_pattern: Option<Expression>,
    file_encoding: Option<String>,
}

/// `folderQueryToCompareObject`.
fn folder_query_to_compare_object(folder_query: &IFolderQuery<URI>) -> FolderCompare {
    let exclude_pattern: Option<Vec<Option<Expression>>> =
        folder_query.exclude_pattern.as_ref().map(|excludes| excludes.iter().map(|exclude| normalize_expression(Some(&exclude.pattern))).collect());
    FolderCompare {
        path: fs_path(&folder_query.folder),
        exclude_pattern: exclude_pattern.filter(|excludes| !excludes.is_empty()),
        include_pattern: normalize_expression(folder_query.include_pattern.as_ref()),
        file_encoding: folder_query.file_encoding.clone(),
    }
}

/// A parte comum de `assertEqualQueries`: compara as pastas e o `extraFileResources` e devolve o resto de cada lado,
/// já normalizado, para o chamador comparar junto com os campos do tipo da consulta.
fn compare_common(actual: &ICommonQueryProps<URI>, expected: &ICommonQueryProps<URI>) -> (ICommonQueryProps<URI>, ICommonQueryProps<URI>) {
    // Avoid comparing URI objects, not a good idea
    let actual_folders: Vec<FolderCompare> = actual.folder_queries.iter().map(folder_query_to_compare_object).collect();
    let expected_folders: Vec<FolderCompare> = expected.folder_queries.iter().map(folder_query_to_compare_object).collect();
    assert_eq!(actual_folders, expected_folders);

    let mut actual = actual.clone();
    let mut expected = expected.clone();
    actual.folder_queries = Vec::new();
    expected.folder_queries = Vec::new();

    if let Some(expected_extra) = &expected.extra_file_resources {
        let actual_paths: Vec<String> = actual.extra_file_resources.as_ref().expect("extraFileResources").iter().map(fs_path).collect();
        let expected_paths: Vec<String> = expected_extra.iter().map(fs_path).collect();
        assert_eq!(actual_paths, expected_paths);
        expected.extra_file_resources = None;
        actual.extra_file_resources = None;
    }

    actual.using_search_paths = None;
    actual.include_pattern = normalize_expression(actual.include_pattern.as_ref());
    actual.exclude_pattern = normalize_expression(actual.exclude_pattern.as_ref());
    expected.include_pattern = normalize_expression(expected.include_pattern.as_ref());
    expected.exclude_pattern = normalize_expression(expected.exclude_pattern.as_ref());
    (actual, expected)
}

/// `assertEqualTextQueries`.
fn assert_equal_text_queries(actual: &ITextQuery, expected: &ITextQuery) {
    let (actual_common, expected_common) = compare_common(&actual.common, &expected.common);
    assert_eq!(ITextQueryProps { common: actual_common, ..actual.clone() }, ITextQueryProps { common: expected_common, ..expected.clone() });
}

/// `assertEqualQueries` para uma consulta de arquivo.
fn assert_equal_file_queries(actual: &IFileQueryProps<URI>, expected: &IFileQueryProps<URI>) {
    let (actual_common, expected_common) = compare_common(&actual.common, &expected.common);
    assert_eq!(IFileQueryProps { common: actual_common, ..actual.clone() }, IFileQueryProps { common: expected_common, ..expected.clone() });
}

/// `assertEqualSearchPathResults`.
fn assert_equal_search_path_results(actual: &ISearchPathsInfo, expected: &ISearchPathsInfo, message: &str) {
    assert_eq!(sorted_expression(actual.pattern.as_ref()), sorted_expression(expected.pattern.as_ref()), "{message}");

    assert_eq!(actual.search_paths.as_ref().map(Vec::len), expected.search_paths.as_ref().map(Vec::len), "{message}");
    if let (Some(actual_paths), Some(expected_paths)) = (&actual.search_paths, &expected.search_paths) {
        for (search_path, expected_search_path) in actual_paths.iter().zip(expected_paths) {
            assert_eq!(search_path.pattern.as_ref().map(|p| sorted_expression(Some(p))), expected_search_path.pattern.as_ref().map(|p| sorted_expression(Some(p))), "{message}");
            assert_eq!(search_path.search_path.to_string(false).to_string_lossy(), expected_search_path.search_path.to_string(false).to_string_lossy(), "{message}");
        }
    }
}

fn search_path(search_path: URI, pattern: Option<Expression>) -> je_workbench::services::search::query_builder::ISearchPathPattern {
    je_workbench::services::search::query_builder::ISearchPathPattern { search_path, pattern }
}

fn info(search_paths: Option<Vec<je_workbench::services::search::query_builder::ISearchPathPattern>>, pattern: Option<Expression>) -> ISearchPathsInfo {
    ISearchPathsInfo { search_paths, pattern }
}

fn common_options() -> ICommonQueryBuilderOptions {
    ICommonQueryBuilderOptions::default()
}

fn text_options(common: ICommonQueryBuilderOptions) -> ITextQueryBuilderOptions {
    ITextQueryBuilderOptions { common, ..ITextQueryBuilderOptions::default() }
}

fn file_options(common: ICommonQueryBuilderOptions) -> IFileQueryBuilderOptions {
    IFileQueryBuilderOptions { common, ..IFileQueryBuilderOptions::default() }
}

fn exclude_builder(pattern: &str) -> Option<Vec<SearchPatternBuilder>> {
    Some(vec![SearchPatternBuilder { uri: None, pattern: SearchPathPatternBuilder::from(pattern) }])
}

fn pattern_info() -> PatternInfo {
    PatternInfo::new("a")
}

fn root_1() -> String {
    fix_path(&["/foo/root1"])
}

fn root_1_uri() -> URI {
    uri_file(&root_1())
}

fn root_1_named_folder() -> WorkspaceFolderOrUri {
    WorkspaceFolderOrUri::Folder(folder_for(&root_1_uri()))
}

/// `WS_CONFIG_PATH`: o local do arquivo de workspace.
fn ws_config_path() -> URI {
    get_uri(&["/bar/test.code-workspace"])
}

/// `toWorkspaceFolders([{ path }...], WS_CONFIG_PATH, extUriBiasedIgnorePathCase)`.
fn to_folders(raw: Vec<(String, Option<&str>)>) -> Vec<WorkspaceFolder> {
    let raw: Vec<RawWorkspaceFolder> = raw.into_iter().map(|(path, name)| RawWorkspaceFolder::Path { path, name: name.map(str::to_string) }).collect();
    to_workspace_folders(&raw, &ws_config_path(), &ext_uri_biased_ignore_path_case())
}

struct Fixture {
    mock_config_service: Rc<TestConfigurationService>,
    mock_workspace: Rc<Workspace>,
    query_builder: Rc<QueryBuilder>,
}

fn setup() -> Fixture {
    let mock_config_service = TestConfigurationService::new(None);
    let _ = mock_config_service.set_user_configuration("search", user_config(None), None);
    let _ = mock_config_service.set_user_configuration("editor", Dynamic::object(Vec::<(&str, Dynamic)>::new()), None);

    let mock_workspace = new_test_workspace("workspace", vec![folder_for(&root_1_uri())], None, None);
    let mock_context_service = TestContextService::new(Some(mock_workspace.clone()), None);

    let configuration_service: Rc<dyn IConfigurationService> = mock_config_service.clone();
    let context_service: Rc<dyn IWorkspaceContextService> = mock_context_service;
    let editor_groups_service: Rc<dyn IEditorGroupsService> = TestEditorGroupsService::new(None);
    let log_service: LogServiceRef = Rc::new(NullLogger::new());
    let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
    let uri_identity_service: Rc<dyn IUriIdentityService> = Rc::new(TestUriIdentityService { ext_uri: ext_uri() });

    let query_builder = QueryBuilder::create((), (configuration_service, context_service, editor_groups_service, log_service, path_service, uri_identity_service));
    Fixture { mock_config_service, mock_workspace, query_builder }
}

/// `queryBuilder.parseSearchPaths(string)`.
fn parse_search_paths(fixture: &Fixture, include_pattern: &str) -> ISearchPathsInfo {
    fixture.query_builder.parse_search_paths(&SearchPathPatternBuilder::from(include_pattern))
}

#[test]
fn simple_text_pattern() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), None, &ITextQueryBuilderOptions::default()), &ITextQueryProps::new(Vec::new(), pattern_info()));
}

#[test]
fn normalize_literal_newlines() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut input = PatternInfo::new("foo\nbar");
    input.is_reg_exp = Some(true);
    let mut expected_pattern = PatternInfo::new("foo\\nbar");
    expected_pattern.is_reg_exp = Some(true);
    expected_pattern.is_multiline = Some(true);
    assert_equal_text_queries(&t.query_builder.text(input, None, &ITextQueryBuilderOptions::default()), &ITextQueryProps::new(Vec::new(), expected_pattern));

    let mut input = PatternInfo::new("foo\nbar");
    input.is_reg_exp = Some(false);
    let mut expected_pattern = PatternInfo::new("foo\nbar");
    expected_pattern.is_reg_exp = Some(false);
    expected_pattern.is_multiline = Some(true);
    assert_equal_text_queries(&t.query_builder.text(input, None, &ITextQueryBuilderOptions::default()), &ITextQueryProps::new(Vec::new(), expected_pattern));
}

#[test]
fn splits_include_pattern_when_expand_patterns_enabled() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = file_options(ICommonQueryBuilderOptions { include_pattern: Some("**/foo, **/bar".into()), expand_patterns: Some(true), ..common_options() });
    let mut expected = IFileQueryProps::new(vec![fq(&root_1_uri())]);
    expected.common.include_pattern = Some(expr(&["**/foo", "**/foo/**", "**/bar", "**/bar/**"]));
    assert_equal_file_queries(&t.query_builder.file(&[root_1_named_folder()], &options), &expected);
}

#[test]
fn does_not_split_include_pattern_when_expand_patterns_disabled() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = file_options(ICommonQueryBuilderOptions { include_pattern: Some("**/foo, **/bar".into()), ..common_options() });
    let mut expected = IFileQueryProps::new(vec![fq(&root_1_uri())]);
    expected.common.include_pattern = Some(expr(&["**/foo, **/bar"]));
    assert_equal_file_queries(&t.query_builder.file(&[root_1_named_folder()], &options), &expected);
}

#[test]
fn include_pattern_array() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = file_options(ICommonQueryBuilderOptions { include_pattern: Some(vec!["**/foo".to_string(), "**/bar".to_string()].into()), ..common_options() });
    let mut expected = IFileQueryProps::new(vec![fq(&root_1_uri())]);
    expected.common.include_pattern = Some(expr(&["**/foo", "**/bar"]));
    assert_equal_file_queries(&t.query_builder.file(&[root_1_named_folder()], &options), &expected);
}

#[test]
fn include_pattern_array_with_expand_patterns() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = file_options(ICommonQueryBuilderOptions {
        include_pattern: Some(vec!["**/foo".to_string(), "**/bar".to_string()].into()),
        expand_patterns: Some(true),
        ..common_options()
    });
    let mut expected = IFileQueryProps::new(vec![fq(&root_1_uri())]);
    expected.common.include_pattern = Some(expr(&["**/foo", "**/foo/**", "**/bar", "**/bar/**"]));
    assert_equal_file_queries(&t.query_builder.file(&[root_1_named_folder()], &options), &expected);
}

#[test]
fn folder_resources() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_equal_text_queries(
        &t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &ITextQueryBuilderOptions::default()),
        &ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info()),
    );
}

#[test]
fn simple_exclude_setting() {
    let t = setup();
    let exclude = Dynamic::object([("bar/**", flag(true)), ("foo/**", Dynamic::object([("when", Dynamic::string("$(basename).ts"))]))]);
    let _ = t.mock_config_service.set_user_configuration("search", user_config(Some(exclude)), None);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let options = text_options(ICommonQueryBuilderOptions { expand_patterns: Some(true), ..common_options() }); // verify that this doesn't affect patterns from configuration
    let expected_exclude = Expression::from_entries([("bar/**", ExprValue::Bool(true)), ("foo/**", ExprValue::When("$(basename).ts".to_string()))]);
    assert_equal_text_queries(
        &t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options),
        &ITextQueryProps::new(vec![fq_with(&root_1_uri(), None, Some(expected_exclude))], pattern_info()),
    );
}

#[test]
fn simple_include() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("bar".into()), expand_patterns: Some(true), ..common_options() });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.include_pattern = Some(expr(&["**/bar", "**/bar/**"]));
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);

    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("bar".into()), ..common_options() });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.include_pattern = Some(expr(&["bar"]));
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);
}

#[test]
fn simple_include_with_dot_slash_syntax() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for include in ["./bar", ".\\bar"] {
        let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some(include.into()), expand_patterns: Some(true), ..common_options() });
        assert_equal_text_queries(
            &t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options),
            &ITextQueryProps::new(vec![fq_with(&root_1_uri(), Some(expr(&["bar", "bar/**"])), None)], pattern_info()),
        );
    }
}

#[test]
fn exclude_setting_and_search_path() {
    let t = setup();
    let exclude = Dynamic::object([("foo/**/*.js", flag(true)), ("bar/**", Dynamic::object([("when", Dynamic::string("$(basename).ts"))]))]);
    let _ = t.mock_config_service.set_user_configuration("search", user_config(Some(exclude)), None);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("./foo".into()), expand_patterns: Some(true), ..common_options() });
    let expected_exclude = Expression::from_entries([("foo/**/*.js", ExprValue::Bool(true)), ("bar/**", ExprValue::When("$(basename).ts".to_string()))]);
    assert_equal_text_queries(
        &t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options),
        &ITextQueryProps::new(vec![fq_with(&root_1_uri(), Some(expr(&["foo", "foo/**"])), Some(expected_exclude))], pattern_info()),
    );
}

#[test]
fn multiroot_exclude_settings() {
    let t = setup();
    let root_2 = fix_path(&["/project/root2"]);
    let root_2_uri = uri_file(&root_2);
    let root_3 = fix_path(&["/project/root3"]);
    let root_3_uri = uri_file(&root_3);
    t.mock_workspace.set_folders(to_folders(vec![(fs_path(&root_1_uri()), None), (fs_path(&root_2_uri), None), (fs_path(&root_3_uri), None)]));
    t.mock_workspace.set_configuration(Some(uri_file(&fix_path(&["/config"]))));

    let _ = t.mock_config_service.set_user_configuration("search", user_config(Some(Dynamic::object([("foo/**/*.js", flag(true))]))), Some(&root_1_uri()));
    let _ = t.mock_config_service.set_user_configuration("search", user_config(Some(Dynamic::object([("bar", flag(true))]))), Some(&root_2_uri));
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // There are 3 roots, the first two have search.exclude settings, test that the correct basic query is returned
    assert_equal_text_queries(
        &t.query_builder.text(pattern_info(), Some(&[root_1_uri(), root_2_uri.clone(), root_3_uri.clone()]), &ITextQueryBuilderOptions::default()),
        &ITextQueryProps::new(
            vec![
                fq_with(&root_1_uri(), None, make_exclude_pattern_from_patterns(&["foo/**/*.js"])),
                fq_with(&root_2_uri, None, make_exclude_pattern_from_patterns(&["bar"])),
                fq(&root_3_uri),
            ],
            pattern_info(),
        ),
    );

    // Now test that it merges the root excludes when an 'include' is used
    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("./root2/src".into()), expand_patterns: Some(true), ..common_options() });
    assert_equal_text_queries(
        &t.query_builder.text(pattern_info(), Some(&[root_1_uri(), root_2_uri.clone(), root_3_uri.clone()]), &options),
        &ITextQueryProps::new(vec![fq_with(&root_2_uri, Some(expr(&["src", "src/**"])), Some(expr(&["bar"])))], pattern_info()),
    );
}

#[test]
fn simple_exclude_input_pattern() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { exclude_pattern: exclude_builder("foo"), expand_patterns: Some(true), ..common_options() });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.exclude_pattern = Some(expr_of_global_glob("foo"));
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);
}

#[test]
fn file_pattern_trimming() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = "content";
    let options = IFileQueryBuilderOptions { file_pattern: Some(format!(" {content} ")), ..IFileQueryBuilderOptions::default() };
    let mut expected = IFileQueryProps::new(Vec::new());
    expected.file_pattern = Some(content.to_string());
    assert_equal_file_queries(&t.query_builder.file(&[], &options), &expected);
}

#[test]
fn exclude_dot_slash_syntax() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, [&str; 2]); 3] = [("./bar", ["bar", "bar/**"]), ("./bar/**/*.ts", ["bar/**/*.ts", "bar/**/*.ts/**"]), (".\\bar\\**\\*.ts", ["bar/**/*.ts", "bar/**/*.ts/**"])];
    for (exclude, expected_patterns) in cases {
        let options = text_options(ICommonQueryBuilderOptions { exclude_pattern: exclude_builder(exclude), expand_patterns: Some(true), ..common_options() });
        assert_equal_text_queries(
            &t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options),
            &ITextQueryProps::new(vec![fq_with(&root_1_uri(), None, make_exclude_pattern_from_patterns(&expected_patterns))], pattern_info()),
        );
    }
}

#[test]
fn extra_file_resources() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { extra_file_resources: Some(vec![get_uri(&["/foo/bar.js"])]), ..common_options() });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.extra_file_resources = Some(vec![get_uri(&["/foo/bar.js"])]);
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);

    let options = text_options(ICommonQueryBuilderOptions {
        extra_file_resources: Some(vec![get_uri(&["/foo/bar.js"])]),
        exclude_pattern: exclude_builder("*.js"),
        expand_patterns: Some(true),
        ..common_options()
    });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.exclude_pattern = Some(expr_of_global_glob("*.js"));
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);

    let options = text_options(ICommonQueryBuilderOptions {
        extra_file_resources: Some(vec![get_uri(&["/foo/bar.js"])]),
        include_pattern: Some("*.txt".into()),
        expand_patterns: Some(true),
        ..common_options()
    });
    let mut expected = ITextQueryProps::new(vec![fq(&root_1_uri())], pattern_info());
    expected.common.include_pattern = Some(expr_of_global_glob("*.txt"));
    assert_equal_text_queries(&t.query_builder.text(pattern_info(), Some(&[root_1_uri()]), &options), &expected);
}

// ---- parseSearchPaths 1 ---------------------------------------------------------------------------------------

#[test]
fn parse_search_paths_1_simple_includes() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, &[&str]); 6] = [
        ("a", &["**/a/**", "**/a"]),
        ("a/b", &["**/a/b", "**/a/b/**"]),
        ("a/b,  c", &["**/a/b", "**/c", "**/a/b/**", "**/c/**"]),
        ("a,.txt", &["**/a", "**/a/**", "**/*.txt", "**/*.txt/**"]),
        ("a,,,b", &["**/a", "**/a/**", "**/b", "**/b/**"]),
        ("**/a,b/**", &["**/a", "**/a/**", "**/b/**"]),
    ];
    for (include_pattern, expected_patterns) in cases {
        let result = parse_search_paths(&t, include_pattern);
        assert_eq!(sorted_expression(result.pattern.as_ref()), sorted_expression(patterns_to_expression(expected_patterns).as_ref()), "{include_pattern}");
        assert_eq!(result.search_paths, None);
    }
}

/// `testIncludes` da subsuíte 1: um erro do `parseSearchPaths` vira `{ searchPaths: [] }`.
fn test_includes_catching(t: &Fixture, include_pattern: &str, expected_result: &ISearchPathsInfo) {
    let actual = errors::catch(|| parse_search_paths(t, include_pattern)).unwrap_or_else(|_| info(Some(Vec::new()), None));
    assert_equal_search_path_results(&actual, expected_result, include_pattern);
}

#[test]
fn parse_search_paths_1_absolute_includes() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let foo_bar = fix_path(&["/foo/bar"]);
    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        (foo_bar.clone(), info(Some(vec![search_path(get_uri(&["/foo/bar"]), None)]), None)),
        (format!("{foo_bar},a"), info(Some(vec![search_path(get_uri(&["/foo/bar"]), None)]), Some(expr_of_global_glob("a")))),
        (format!("{foo_bar},{}", fix_path(&["/1/2"])), info(Some(vec![search_path(get_uri(&["/foo/bar"]), None), search_path(get_uri(&["/1/2"]), None)]), None)),
        (format!("{foo_bar},{}", fix_path(&["/foo/../foo/bar/fooar/.."])), info(Some(vec![search_path(get_uri(&["/foo/bar"]), None)]), None)),
        (fix_path(&["/foo/bar/**/*.ts"]), info(Some(vec![search_path(get_uri(&["/foo/bar"]), patterns_to_expression(&["**/*.ts", "**/*.ts/**"]))]), None)),
        (fix_path(&["/foo/bar/*a/b/c"]), info(Some(vec![search_path(get_uri(&["/foo/bar"]), patterns_to_expression(&["*a/b/c", "*a/b/c/**"]))]), None)),
        (fix_path(&["/*a/b/c"]), info(Some(vec![search_path(get_uri(&["/"]), patterns_to_expression(&["*a/b/c", "*a/b/c/**"]))]), None)),
        (fix_path(&["/foo/{b,c}ar"]), info(Some(vec![search_path(get_uri(&["/foo"]), patterns_to_expression(&["{b,c}ar", "{b,c}ar/**"]))]), None)),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

#[test]
fn parse_search_paths_1_relative_includes_with_single_root_folder() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        ("./a".to_string(), info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["a", "a/**"]))]), None)),
        ("./a/".to_string(), info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["a", "a/**"]))]), None)),
        ("./a/*b/c".to_string(), info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["a/*b/c", "a/*b/c/**"]))]), None)),
        (
            format!("./a/*b/c, {}", fix_path(&["/project/foo"])),
            info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["a/*b/c", "a/*b/c/**"])), search_path(get_uri(&["/project/foo"]), None)]), None),
        ),
        ("./a/b/,./c/d".to_string(), info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["a/b", "a/b/**", "c/d", "c/d/**"]))]), None)),
        ("../".to_string(), info(Some(vec![search_path(get_uri(&["/foo"]), None)]), None)),
        ("..".to_string(), info(Some(vec![search_path(get_uri(&["/foo"]), None)]), None)),
        ("..\\bar".to_string(), info(Some(vec![search_path(get_uri(&["/foo/bar"]), None)]), None)),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

#[test]
fn parse_search_paths_1_relative_includes_with_two_root_folders() {
    let t = setup();
    let root_2 = "/project/root2";
    t.mock_workspace.set_folders(to_folders(vec![(fs_path(&root_1_uri()), None), (fs_path(&get_uri(&[root_2])), None)]));
    t.mock_workspace.set_configuration(Some(uri_file(&fix_path(&["config"]))));
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        ("./root1".to_string(), info(Some(vec![search_path(get_uri(&[&root_1()]), None)]), None)),
        ("./root2".to_string(), info(Some(vec![search_path(get_uri(&[root_2]), None)]), None)),
        (
            "./root1/a/**/b, ./root2/**/*.txt".to_string(),
            info(
                Some(vec![
                    search_path(root_1_uri(), patterns_to_expression(&["a/**/b", "a/**/b/**"])),
                    search_path(get_uri(&[root_2]), patterns_to_expression(&["**/*.txt", "**/*.txt/**"])),
                ]),
                None,
            ),
        ),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

#[test]
fn include_dot_slash_foldername() {
    let t = setup();
    let root_2 = "/project/root2";
    let root_1_foldername = "foldername";
    t.mock_workspace.set_folders(to_folders(vec![(fs_path(&root_1_uri()), Some(root_1_foldername)), (fs_path(&get_uri(&[root_2])), None)]));
    t.mock_workspace.set_configuration(Some(uri_file(&fix_path(&["config"]))));
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        ("./foldername".to_string(), info(Some(vec![search_path(root_1_uri(), None)]), None)),
        ("./foldername/foo".to_string(), info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["foo", "foo/**"]))]), None)),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

#[test]
fn folder_with_slash_in_the_name() {
    let t = setup();
    let root_2 = "/project/root2";
    let root_2_uri = get_uri(&[root_2]);
    let root_1_foldername = "folder/one";
    let root_2_foldername = "folder/two+"; // And another regex character, #126003
    t.mock_workspace.set_folders(to_folders(vec![(fs_path(&root_1_uri()), Some(root_1_foldername)), (fs_path(&root_2_uri), Some(root_2_foldername))]));
    t.mock_workspace.set_configuration(Some(uri_file(&fix_path(&["config"]))));
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        ("./folder/one".to_string(), info(Some(vec![search_path(root_1_uri(), None)]), None)),
        ("./folder/two+/foo/".to_string(), info(Some(vec![search_path(root_2_uri.clone(), patterns_to_expression(&["foo", "foo/**"]))]), None)),
        ("./folder/onesomethingelse".to_string(), info(Some(Vec::new()), None)),
        ("./folder/onesomethingelse/foo".to_string(), info(Some(Vec::new()), None)),
        ("./folder".to_string(), info(Some(Vec::new()), None)),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

#[test]
fn relative_includes_with_multiple_ambiguous_root_folders() {
    let t = setup();
    let root_2 = "/project/rootB";
    let root_3 = "/otherproject/rootB";
    t.mock_workspace.set_folders(to_folders(vec![(fs_path(&root_1_uri()), None), (fs_path(&get_uri(&[root_2])), None), (fs_path(&get_uri(&[root_3])), None)]));
    t.mock_workspace.set_configuration(Some(uri_file(&fix_path(&["/config"]))));
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        (String::new(), info(None, None)),
        ("./".to_string(), info(None, None)),
        ("./root1".to_string(), info(Some(vec![search_path(get_uri(&[&root_1()]), None)]), None)),
        ("./root1,./".to_string(), info(Some(vec![search_path(get_uri(&[&root_1()]), None)]), None)),
        ("./rootB".to_string(), info(Some(vec![search_path(get_uri(&[root_2]), None), search_path(get_uri(&[root_3]), None)]), None)),
        (
            "./rootB/a/**/b, ./rootB/b/**/*.txt".to_string(),
            info(
                Some(vec![
                    search_path(get_uri(&[root_2]), patterns_to_expression(&["a/**/b", "a/**/b/**", "b/**/*.txt", "b/**/*.txt/**"])),
                    search_path(get_uri(&[root_3]), patterns_to_expression(&["a/**/b", "a/**/b/**", "b/**/*.txt", "b/**/*.txt/**"])),
                ]),
                None,
            ),
        ),
        (
            "./root1/**/foo/, bar/".to_string(),
            info(Some(vec![search_path(root_1_uri(), patterns_to_expression(&["**/foo", "**/foo/**"]))]), patterns_to_expression(&["**/bar", "**/bar/**"])),
        ),
    ];
    for (include_pattern, expected_result) in &cases {
        test_includes_catching(&t, include_pattern, expected_result);
    }
}

// ---- parseSearchPaths 2 ---------------------------------------------------------------------------------------

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows, como no original (isWindows ? test.skip : test)")]
fn includes_with_tilde() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let user_home = uri_file("/");
    let user_home_path = fs_path(&user_home);
    let cases: Vec<(String, ISearchPathsInfo)> = vec![
        ("~/foo/bar".to_string(), info(Some(vec![search_path(get_uri(&[&user_home_path, "/foo/bar"]), None)]), None)),
        ("~/foo/bar, a".to_string(), info(Some(vec![search_path(get_uri(&[&user_home_path, "/foo/bar"]), None)]), Some(expr_of_global_glob("a")))),
        (fix_path(&["/foo/~/bar"]), info(Some(vec![search_path(get_uri(&["/foo/~/bar"]), None)]), None)),
    ];
    for (include_pattern, expected_result) in &cases {
        assert_equal_search_path_results(&parse_search_paths(&t, include_pattern), expected_result, include_pattern);
    }
}

// ---- smartCase ------------------------------------------------------------------------------------------------

#[test]
fn smart_case_no_flags_no_change() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let query = t.query_builder.text(PatternInfo::new("a"), Some(&[]), &ITextQueryBuilderOptions::default());
    assert!(!query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_maintains_is_case_sensitive_when_smart_case_not_set() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut pattern = PatternInfo::new("a");
    pattern.is_case_sensitive = Some(true);
    let query = t.query_builder.text(pattern, Some(&[]), &ITextQueryBuilderOptions::default());
    assert!(query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_maintains_is_case_sensitive_when_smart_case_set() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut pattern = PatternInfo::new("a");
    pattern.is_case_sensitive = Some(true);
    let options = ITextQueryBuilderOptions { is_smart_case: Some(true), ..ITextQueryBuilderOptions::default() };
    let query = t.query_builder.text(pattern, Some(&[]), &options);
    assert!(query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_determines_not_case_sensitive() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = ITextQueryBuilderOptions { is_smart_case: Some(true), ..ITextQueryBuilderOptions::default() };
    let query = t.query_builder.text(PatternInfo::new("abcd"), Some(&[]), &options);
    assert!(!query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_determines_case_sensitive() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = ITextQueryBuilderOptions { is_smart_case: Some(true), ..ITextQueryBuilderOptions::default() };
    let query = t.query_builder.text(PatternInfo::new("abCd"), Some(&[]), &options);
    assert!(query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_determines_not_case_sensitive_regex() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut pattern = PatternInfo::new("ab\\Sd");
    pattern.is_reg_exp = Some(true);
    let options = ITextQueryBuilderOptions { is_smart_case: Some(true), ..ITextQueryBuilderOptions::default() };
    let query = t.query_builder.text(pattern, Some(&[]), &options);
    assert!(!query.content_pattern.is_case_sensitive.unwrap_or(false));
}

#[test]
fn smart_case_determines_case_sensitive_regex() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut pattern = PatternInfo::new("ab[A-Z]d");
    pattern.is_reg_exp = Some(true);
    let options = ITextQueryBuilderOptions { is_smart_case: Some(true), ..ITextQueryBuilderOptions::default() };
    let query = t.query_builder.text(pattern, Some(&[]), &options);
    assert!(query.content_pattern.is_case_sensitive.unwrap_or(false));
}

// ---- file -----------------------------------------------------------------------------------------------------

#[test]
fn simple_file_query() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cache_key = "asdf";
    let options = IFileQueryBuilderOptions { cache_key: Some(cache_key.to_string()), sort_by_score: Some(true), ..IFileQueryBuilderOptions::default() };
    let query = t.query_builder.file(&[root_1_named_folder()], &options);

    assert_eq!(query.common.folder_queries.len(), 1);
    assert_eq!(query.cache_key.as_deref(), Some(cache_key));
    assert_eq!(query.sort_by_score, Some(true));
}

// ---- pattern processing ---------------------------------------------------------------------------------------

fn comma_separated_globs() -> Expression {
    expr(&["**/*.js/**", "**/*.js", "**/*.ts/**", "**/*.ts"])
}

#[test]
fn text_query_with_comma_separated_includes_with_no_workspace() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("*.js,*.ts".into()), expand_patterns: Some(true), ..common_options() });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[]), &options);
    assert_eq!(sorted_expression(query.common.include_pattern.as_ref()), sorted_expression(Some(&comma_separated_globs())));
    assert_eq!(query.common.folder_queries.len(), 0);
}

#[test]
fn text_query_with_comma_separated_includes_with_workspace() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { include_pattern: Some("*.js,*.ts".into()), expand_patterns: Some(true), ..common_options() });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[root_1_uri()]), &options);
    assert_eq!(sorted_expression(query.common.include_pattern.as_ref()), sorted_expression(Some(&comma_separated_globs())));
    assert_eq!(query.common.folder_queries.len(), 1);
}

#[test]
fn text_query_with_comma_separated_excludes_globally() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { exclude_pattern: exclude_builder("*.js,*.ts"), expand_patterns: Some(true), ..common_options() });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[]), &options);
    assert_eq!(sorted_expression(query.common.exclude_pattern.as_ref()), sorted_expression(Some(&comma_separated_globs())));
    assert_eq!(query.common.folder_queries.len(), 0);
}

#[test]
fn text_query_with_comma_separated_excludes_globally_in_a_workspace() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions { exclude_pattern: exclude_builder("*.js,*.ts"), expand_patterns: Some(true), ..common_options() });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[folder_for(&root_1_uri()).uri]), &options);
    assert_eq!(sorted_expression(query.common.exclude_pattern.as_ref()), sorted_expression(Some(&comma_separated_globs())));
    assert_eq!(query.common.folder_queries.len(), 1);
}

#[test]
#[ignore = "test.skip no original: TODO: Fix. Exigiria que `ICommonQueryProps.excludePattern` aceitasse uma lista"]
fn text_query_with_multiple_comma_separated_excludes() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions {
        exclude_pattern: Some(vec![
            SearchPatternBuilder { uri: None, pattern: "*.js,*.ts".into() },
            SearchPatternBuilder { uri: None, pattern: "foo/*,bar/*".into() },
        ]),
        expand_patterns: Some(true),
        ..common_options()
    });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[folder_for(&root_1_uri()).uri]), &options);
    // O original espera uma lista com duas expressões; o tipo só tem uma (por isso o caso é pulado lá).
    assert_eq!(query.common.folder_queries.len(), 1);
}

#[test]
#[ignore = "test.skip no original: TODO: Fix. Exigiria que `ICommonQueryProps.excludePattern` aceitasse um URI base"]
fn text_query_with_base_uri_on_exclud() {
    let t = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let options = text_options(ICommonQueryBuilderOptions {
        exclude_pattern: Some(vec![SearchPatternBuilder { uri: Some(root_1_uri()), pattern: "*.js,*.ts".into() }]),
        expand_patterns: Some(true),
        ..common_options()
    });
    let query = t.query_builder.text(PatternInfo::new(""), Some(&[folder_for(&root_1_uri()).uri]), &options);
    // todo: incorporate the base URI into the pattern
    assert_eq!(query.common.folder_queries.len(), 1);
}
