//! Porte de `src/vs/workbench/contrib/search/test/browser/searchConfiguration.test.ts` (suíte `Shared search configuration`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`import '../../browser/search.common.contribution.js'` e `sharedSearchConfiguration`.** O original registra a contribuição
//!   ao importar o módulo e guarda uma cópia profunda do nó antes de os testes de configuração limparem o registro global. Aqui o
//!   registro é a chamada de [`register_search_common_configuration`] no `setup` de cada caso (o registro de configuração é por
//!   fio e cada `#[test]` roda no seu), e o nó é o que tem `search.searchOnType`, lido do registro.
//! - **`setup`.** `defaults.setValue(key, deepClone(schema.default))` para cada propriedade do nó, e `getValue('search')` dá o
//!   `searchConfiguration`. O `TestInstantiationService` (que registra só `IConfigurationService` e `IWorkspaceContextService`)
//!   é a chamada direta de `QueryBuilder::create`, com os serviços que o original deixa sem registro como dublês vazios (grupos
//!   de editores, log, caminho) e o `IUriIdentityService` com o `ExtUri` sem viés, como no `queryBuilder.test.ts` portado.
//!   `store.add(configurationService.onDidChangeConfigurationEmitter)` é o `add_rc` do emissor no guarda de vazamentos.
//! - **`await configurationService.setUserConfiguration(...)`.** O `setUserConfiguration` do porte aplica o valor na hora e
//!   devolve a promessa já resolvida, então o caso não precisa esperar.
//! - **`assert.deepStrictEqual` sobre objetos literais.** Os valores de configuração, que são `Dynamic`, são comparados por
//!   `assert_deep_strict_equal`; as consultas são comparadas por uma visão com os campos lidos (`FolderQueryView`), e as
//!   expressões de exclusão sem considerar a ordem das chaves, como o `deepStrictEqual` do JavaScript.
//! - **`folder: undefined`.** O `ExcludeGlobPattern.folder` é `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::glob::{ExprValue, Expression};
use je_base::objects::deep_clone;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::configuration::models::ConfigurationModel;
use je_platform::configuration::registry::configuration_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogServiceRef, NullLogger};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_instantiation::Injectable;
use je_workbench::contrib::search::browser::search_common_contribution::register_search_common_configuration;
use je_workbench::services::editor::editor_groups_service::IEditorGroupsService;
use je_workbench::services::search::query_builder::{ICommonQueryBuilderOptions, IFileQueryBuilderOptions, ITextQueryBuilderOptions, QueryBuilder, WorkspaceFolderOrUri};
use je_workbench::services::search::search::{DEFAULT_MAX_SEARCH_RESULTS, ExcludeGlobPattern, IFolderQuery, PatternInfo, SearchSortOrder};
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

/// O que o `setup` monta: `searchConfiguration`, `configurationService` e `queryBuilder`.
struct Fixture {
    search_configuration: Dynamic,
    configuration_service: Rc<TestConfigurationService>,
    query_builder: Rc<QueryBuilder>,
}

/// `const folder = URI.file('/workspace')`.
fn workspace_folder() -> URI {
    uri_file("/workspace")
}

fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// `setup`.
fn setup(guard: &LeakGuard) -> Fixture {
    register_search_common_configuration();
    let shared_search_configuration = configuration_registry()
        .get_configurations()
        .into_iter()
        .find(|node| node.property("search.searchOnType").is_some())
        .expect("a contribuição comum de busca registra search.searchOnType");

    let log_service: LogServiceRef = Rc::new(NullLogger::new());
    let defaults = ConfigurationModel::create_empty_model(log_service.clone());
    for key in shared_search_configuration.property_keys() {
        let schema = shared_search_configuration.property(&key).expect("a chave vem do próprio nó");
        defaults.set_value(key, deep_clone(&schema.default_value()));
    }
    let search_configuration = defaults.get_value("search");
    assert!(!search_configuration.is_nullish(), "os padrões da seção search existem");

    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([("search", search_configuration.clone()), ("editor", empty_object())])));
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());

    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let context_service: Rc<dyn IWorkspaceContextService> = TestContextService::new(None, None);
    let editor_groups_service: Rc<dyn IEditorGroupsService> = TestEditorGroupsService::new(None);
    let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
    let uri_identity_service: Rc<dyn IUriIdentityService> = Rc::new(TestUriIdentityService { ext_uri: ext_uri() });
    let query_builder = QueryBuilder::create((), (configuration, context_service, editor_groups_service, log_service, path_service, uri_identity_service));

    Fixture { search_configuration, configuration_service, query_builder }
}

/// A expressão com as chaves em ordem (o `deepStrictEqual` do JavaScript não olha a ordem das chaves).
fn sorted_expression(expression: &Expression) -> Expression {
    let mut entries: Vec<(String, ExprValue)> = expression.entries().to_vec();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Expression::from_entries(entries)
}

/// `{ '**/node_modules': true, '**/bower_components': true, '**/*.code-search': true }`.
fn default_exclude_expression() -> Expression {
    sorted_expression(&Expression::from_entries(["**/node_modules", "**/bower_components", "**/*.code-search"].map(|pattern| (pattern, true))))
}

/// Os campos de `folderQueries[0]` que os casos leem.
#[derive(Debug, PartialEq)]
struct FolderQueryView {
    disregard_ignore_files: Option<bool>,
    disregard_global_ignore_files: Option<bool>,
    disregard_parent_ignore_files: Option<bool>,
    ignore_symlinks: Option<bool>,
    exclude_pattern: Option<Vec<ExcludeGlobPattern>>,
}

fn view_of(folder_query: &IFolderQuery) -> FolderQueryView {
    FolderQueryView {
        disregard_ignore_files: folder_query.disregard_ignore_files,
        disregard_global_ignore_files: folder_query.disregard_global_ignore_files,
        disregard_parent_ignore_files: folder_query.disregard_parent_ignore_files,
        ignore_symlinks: folder_query.ignore_symlinks,
        exclude_pattern: folder_query
            .exclude_pattern
            .as_ref()
            .map(|excludes| excludes.iter().map(|exclude| ExcludeGlobPattern { folder: exclude.folder.clone(), pattern: sorted_expression(&exclude.pattern) }).collect()),
    }
}

/// `{ ...searchConfiguration, ...overrides }`.
fn with_overrides(search_configuration: &Dynamic, overrides: &[(&str, Dynamic)]) -> Dynamic {
    let mut entries: Vec<(String, Dynamic)> = search_configuration.own_entries().into_iter().map(|(key, value)| (key.to_string_lossy(), value)).collect();
    for (name, value) in overrides {
        match entries.iter_mut().find(|(key, _)| key == name) {
            Some(entry) => entry.1 = value.clone(),
            None => entries.push(((*name).to_owned(), value.clone())),
        }
    }

    Dynamic::object(entries)
}

#[test]
fn registers_shared_search_defaults_without_the_search_view() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);
    let search_configuration = &fixture.search_configuration;

    let actual = Dynamic::object([
        ("exclude", Dynamic::object(search_configuration.get_str("exclude").own_entries())),
        ("useIgnoreFiles", search_configuration.get_str("useIgnoreFiles")),
        ("useGlobalIgnoreFiles", search_configuration.get_str("useGlobalIgnoreFiles")),
        ("useParentIgnoreFiles", search_configuration.get_str("useParentIgnoreFiles")),
        ("followSymlinks", search_configuration.get_str("followSymlinks")),
        ("smartCase", search_configuration.get_str("smartCase")),
        ("maxResults", search_configuration.get_str("maxResults")),
        ("sortOrder", search_configuration.get_str("sortOrder")),
        ("maxThreads", fixture.configuration_service.get_value(Some("search.ripgrep.maxThreads"), None)),
    ]);
    let expected = Dynamic::object([
        (
            "exclude",
            Dynamic::object([("**/node_modules", Dynamic::Bool(true)), ("**/bower_components", Dynamic::Bool(true)), ("**/*.code-search", Dynamic::Bool(true))]),
        ),
        ("useIgnoreFiles", Dynamic::Bool(true)),
        ("useGlobalIgnoreFiles", Dynamic::Bool(false)),
        ("useParentIgnoreFiles", Dynamic::Bool(false)),
        ("followSymlinks", Dynamic::Bool(true)),
        ("smartCase", Dynamic::Bool(false)),
        ("maxResults", Dynamic::Number(DEFAULT_MAX_SEARCH_RESULTS as f64)),
        ("sortOrder", Dynamic::string(SearchSortOrder::Default.as_str())),
        ("maxThreads", Dynamic::Number(0.0)),
    ]);

    assert_deep_strict_equal(&actual, &expected);
}

#[test]
fn text_and_file_searches_respect_ignore_files_and_excludes_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);
    let folder = workspace_folder();

    let text_query = fixture.query_builder.text(PatternInfo::new("needle"), Some(std::slice::from_ref(&folder)), &ITextQueryBuilderOptions::default());
    let file_query = fixture.query_builder.file(&[WorkspaceFolderOrUri::Uri(folder.clone())], &IFileQueryBuilderOptions::default());
    let queries = [&text_query.common, &file_query.common];

    let actual: Vec<FolderQueryView> = queries.iter().map(|query| view_of(&query.folder_queries[0])).collect();
    let expected: Vec<FolderQueryView> = (0..2)
        .map(|_| FolderQueryView {
            disregard_ignore_files: Some(false),
            disregard_global_ignore_files: Some(true),
            disregard_parent_ignore_files: Some(true),
            ignore_symlinks: Some(false),
            exclude_pattern: Some(vec![ExcludeGlobPattern { folder: None, pattern: default_exclude_expression() }]),
        })
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn preserves_explicit_ignore_file_settings() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);

    let _ = fixture.configuration_service.set_user_configuration(
        "search",
        with_overrides(
            &fixture.search_configuration,
            &[("useIgnoreFiles", Dynamic::Bool(false)), ("useGlobalIgnoreFiles", Dynamic::Bool(true)), ("useParentIgnoreFiles", Dynamic::Bool(true))],
        ),
        None,
    );
    let folder = workspace_folder();
    let text_query = fixture.query_builder.text(PatternInfo::new("needle"), Some(std::slice::from_ref(&folder)), &ITextQueryBuilderOptions::default());
    let folder_query = &text_query.common.folder_queries[0];

    assert_eq!(
        (folder_query.disregard_ignore_files, folder_query.disregard_global_ignore_files, folder_query.disregard_parent_ignore_files),
        (Some(true), Some(false), Some(false))
    );
}

#[test]
fn can_disable_exclude_settings_and_ignore_files_for_a_search() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&guard);

    let options = ITextQueryBuilderOptions {
        common: ICommonQueryBuilderOptions { disregard_ignore_files: Some(true), disregard_exclude_settings: Some(true), ..Default::default() },
        ..Default::default()
    };
    let folder = workspace_folder();
    let text_query = fixture.query_builder.text(PatternInfo::new("needle"), Some(std::slice::from_ref(&folder)), &options);
    let folder_query = &text_query.common.folder_queries[0];

    assert_eq!((folder_query.disregard_ignore_files, folder_query.exclude_pattern.clone()), (Some(true), Some(Vec::new())));
}
