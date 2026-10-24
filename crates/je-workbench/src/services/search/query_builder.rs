//! Porte de `src/vs/workbench/services/search/common/queryBuilder.ts`: o `QueryBuilder`, que transforma o que a
//! visão de busca digita (padrões de conteúdo, de arquivo, de inclusão e de exclusão) nas consultas
//! (`ITextQuery`, `IFileQuery`, `IAITextQuery`) que o serviço de busca executa.
//!
//! Notas de tradução:
//! - **Injeção.** Os parâmetros decorados são a tupla `Deps` do [`Injectable`]; `create` é o construtor.
//! - **Uniões.** `string | string[]` (`ISearchPathPatternBuilder`) é [`SearchPathPatternBuilder`];
//!   `IWorkspaceFolderData | URI` é [`WorkspaceFolderOrUri`]; os campos opcionais (`?:`) são `Option`.
//! - **Opções.** `ITextQueryBuilderOptions` e `IFileQueryBuilderOptions` estendem as comuns: a composição, com o
//!   campo `common`, como nas consultas de `search.rs`. O parâmetro `U extends UriComponents` fica fixo em [`URI`], o
//!   único que o construtor usa.
//! - **Erros.** O `throw` de `expandOneSearchPath` é [`errors::throw`] (um pânico com o `Error`), capturável com
//!   [`errors::catch`].
//! - **`text`.** O `contentPattern` entra por valor: o original reescreve as quebras de linha do padrão
//!   regular no próprio objeto recebido, o que nenhum chamador observa.
//! - **`isISearchPatternBuilder`.** Como `excludePattern` é uma lista tipada de [`SearchPatternBuilder`], o guarda
//!   existe para o `string | string[] | ISearchPatternBuilder` ([`SearchPatternOrBuilder`]) e vale em todo elemento da
//!   lista; o `uri` ausente do original (`'uri' in object` falso) é `None`.
//! - **Regex.** `^<nome da pasta>(?:/(.*)|$)` e `/\.[\/\\](.+)[\/\\]?/` são escritos à mão, com a mesma semântica
//!   (o `.` não casa terminadores de linha).
//! - **Logs.** O `JSON.stringify` dos URIs e das consultas (só para `trace`) é o JSON do [`Dynamic`] montado na mesma
//!   ordem de propriedades do original.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{self, Error};
use je_base::glob::{self, ExprValue, Expression};
use je_base::js_string::{self, JsString};
use je_base::labels::untildify;
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::strings;
use je_base::uri::{Field, URI, UriChange};
use je_editor::text_model_search::is_multiline_reg_exp_source;
use je_instantiation::Injectable;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::{LogArg, LogServiceRef};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, fs_path, host_environment, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, WorkspaceFolder, to_workspace_folder};

use super::search::{
    ExcludeGlobPattern, IAITextQuery, ICommonQueryProps, IFileQuery, IFileQueryProps, IFolderQuery, ITextQuery, ITextQueryProps, ITextSearchPreviewOptions,
    NotebookPatternInfo, PatternInfo, get_excludes, path_included_in_query,
};
use crate::services::editor::editor_groups_service::IEditorGroupsService;

/// `IOneSearchPathPattern`: uma pasta a buscar e um glob a aplicar.
struct OneSearchPathPattern {
    search_path: URI,
    pattern: Option<String>,
}

/// `ISearchPathPattern`: uma pasta a buscar e um conjunto de globs a aplicar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISearchPathPattern {
    pub search_path: URI,
    pub pattern: Option<Expression>,
}

/// `ISearchPathPatternBuilder = string | string[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchPathPatternBuilder {
    Single(String),
    List(Vec<String>),
}

impl From<&str> for SearchPathPatternBuilder {
    fn from(pattern: &str) -> Self {
        Self::Single(pattern.to_string())
    }
}

impl From<String> for SearchPathPatternBuilder {
    fn from(pattern: String) -> Self {
        Self::Single(pattern)
    }
}

impl From<Vec<String>> for SearchPathPatternBuilder {
    fn from(patterns: Vec<String>) -> Self {
        Self::List(patterns)
    }
}

/// `ISearchPatternBuilder<URI>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPatternBuilder {
    pub uri: Option<URI>,
    pub pattern: SearchPathPatternBuilder,
}

/// `ISearchPatternBuilder<U> | ISearchPathPatternBuilder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchPatternOrBuilder {
    Builder(SearchPatternBuilder),
    Pattern(SearchPathPatternBuilder),
}

/// `isISearchPatternBuilder`.
pub fn is_search_pattern_builder(object: &SearchPatternOrBuilder) -> bool {
    matches!(object, SearchPatternOrBuilder::Builder(_))
}

/// `ISearchPathsInfo`: um conjunto de pastas e um conjunto de globs a aplicar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ISearchPathsInfo {
    pub search_paths: Option<Vec<ISearchPathPattern>>,
    pub pattern: Option<Expression>,
}

/// `IWorkspaceFolderData | URI`.
#[derive(Clone, Debug)]
pub enum WorkspaceFolderOrUri {
    Folder(WorkspaceFolder),
    Uri(URI),
}

impl WorkspaceFolderOrUri {
    fn uri(&self) -> &URI {
        match self {
            Self::Folder(folder) => &folder.uri,
            Self::Uri(uri) => uri,
        }
    }
}

/// `notebookSearchConfig` das opções de consulta de texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookSearchConfig {
    pub include_markup_input: bool,
    pub include_markup_preview: bool,
    pub include_code_input: bool,
    pub include_output: bool,
}

/// `ICommonQueryBuilderOptions<URI>`.
#[derive(Clone, Debug, Default)]
pub struct ICommonQueryBuilderOptions {
    pub reason: Option<String>,
    pub exclude_pattern: Option<Vec<SearchPatternBuilder>>,
    pub include_pattern: Option<SearchPathPatternBuilder>,
    pub extra_file_resources: Option<Vec<URI>>,
    /// Parse the special ./ syntax supported by the searchview, and expand foo to **/foo
    pub expand_patterns: Option<bool>,
    pub max_results: Option<i64>,
    pub max_file_size: Option<i64>,
    pub disregard_ignore_files: Option<bool>,
    pub disregard_global_ignore_files: Option<bool>,
    pub disregard_parent_ignore_files: Option<bool>,
    pub disregard_exclude_settings: Option<bool>,
    pub disregard_search_exclude_settings: Option<bool>,
    pub ignore_symlinks: Option<bool>,
    pub ignore_glob_case: Option<bool>,
    pub only_open_editors: Option<bool>,
    pub changed_file_uris: Option<Vec<URI>>,
    pub only_file_scheme: Option<bool>,
}

/// `IFileQueryBuilderOptions<URI>`.
#[derive(Clone, Debug, Default)]
pub struct IFileQueryBuilderOptions {
    pub common: ICommonQueryBuilderOptions,
    pub file_pattern: Option<String>,
    pub exists: Option<bool>,
    pub sort_by_score: Option<bool>,
    pub cache_key: Option<String>,
    pub should_glob_search: Option<bool>,
}

/// `ITextQueryBuilderOptions<URI>`.
#[derive(Clone, Debug, Default)]
pub struct ITextQueryBuilderOptions {
    pub common: ICommonQueryBuilderOptions,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub file_encoding: Option<String>,
    pub surrounding_context: Option<i64>,
    pub is_smart_case: Option<bool>,
    pub notebook_search_config: Option<NotebookSearchConfig>,
}

/// O `a && b` do JavaScript sobre dois `boolean | undefined`.
fn and_options(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    if a.is_truthy() { b } else { a }
}

/// `QueryBuilder`.
pub struct QueryBuilder {
    configuration_service: Rc<dyn IConfigurationService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    editor_groups_service: Rc<dyn IEditorGroupsService>,
    log_service: LogServiceRef,
    path_service: Rc<dyn IPathService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl Injectable for QueryBuilder {
    type Args = ();
    type Deps = (
        Rc<dyn IConfigurationService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IEditorGroupsService>,
        LogServiceRef,
        Rc<dyn IPathService>,
        Rc<dyn IUriIdentityService>,
    );

    fn create(
        _: (),
        (configuration_service, workspace_context_service, editor_groups_service, log_service, path_service, uri_identity_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new(Self { configuration_service, workspace_context_service, editor_groups_service, log_service, path_service, uri_identity_service })
    }
}

impl QueryBuilder {
    /// `aiText`.
    pub fn ai_text(&self, content_pattern: &str, folder_resources: Option<&[URI]>, options: &ITextQueryBuilderOptions) -> IAITextQuery {
        let folders = to_folders(folder_resources);
        let common = self.common_query(&folders, &options.common);
        super::search::IAITextQueryProps {
            common,
            content_pattern: JsString::from(content_pattern),
            preview_options: None,
            max_file_size: None,
            surrounding_context: None,
            user_disabled_excludes_and_ignore_files: None,
        }
    }

    /// `text`.
    pub fn text(&self, content_pattern: PatternInfo, folder_resources: Option<&[URI]>, options: &ITextQueryBuilderOptions) -> ITextQuery {
        let content_pattern = self.get_content_pattern(content_pattern, options);

        let folders = to_folders(folder_resources);
        let common = self.common_query(&folders, &options.common);
        ITextQueryProps {
            common,
            content_pattern,
            preview_options: options.preview_options,
            max_file_size: options.common.max_file_size,
            surrounding_context: options.surrounding_context,
            user_disabled_excludes_and_ignore_files: and_options(options.common.disregard_exclude_settings, options.common.disregard_ignore_files),
        }
    }

    /// Adjusts input pattern for config.
    fn get_content_pattern(&self, mut input_pattern: PatternInfo, options: &ITextQueryBuilderOptions) -> PatternInfo {
        let search_config = self.configuration_service.get_value(None, None);

        if input_pattern.is_reg_exp.is_truthy() {
            input_pattern.pattern = JsString::from(replace_line_breaks(&input_pattern.pattern.to_string_lossy()));
        }

        let mut new_pattern = input_pattern.clone();
        new_pattern.word_separators = match search_config.get_str("editor").get_str("wordSeparators") {
            Dynamic::String(separators) => Some(separators),
            _ => None,
        };

        if self.is_case_sensitive(&input_pattern, options) {
            new_pattern.is_case_sensitive = Some(true);
        }

        if self.is_multiline(&input_pattern) {
            new_pattern.is_multiline = Some(true);
        }

        if let Some(config) = &options.notebook_search_config {
            if config.include_markup_input {
                new_pattern.notebook_info.get_or_insert_with(NotebookPatternInfo::default).is_in_notebook_markdown_input = Some(true);
            }
            if config.include_markup_preview {
                new_pattern.notebook_info.get_or_insert_with(NotebookPatternInfo::default).is_in_notebook_markdown_preview = Some(true);
            }
            if config.include_code_input {
                new_pattern.notebook_info.get_or_insert_with(NotebookPatternInfo::default).is_in_notebook_cell_input = Some(true);
            }
            if config.include_output {
                new_pattern.notebook_info.get_or_insert_with(NotebookPatternInfo::default).is_in_notebook_cell_output = Some(true);
            }
        }

        new_pattern
    }

    /// `file`.
    pub fn file(&self, folders: &[WorkspaceFolderOrUri], options: &IFileQueryBuilderOptions) -> IFileQuery {
        let common = self.common_query(folders, &options.common);
        IFileQueryProps {
            common,
            file_pattern: options.file_pattern.as_ref().map(|pattern| js_trim(pattern)),
            exists: options.exists,
            sort_by_score: options.sort_by_score,
            cache_key: options.cache_key.clone(),
            should_glob_match_file_pattern: options.should_glob_search,
        }
    }

    fn handle_include_exclude(&self, pattern: Option<&SearchPathPatternBuilder>, expand_patterns: Option<bool>) -> ISearchPathsInfo {
        let Some(pattern) = pattern else { return ISearchPathsInfo::default() };

        let pattern = match pattern {
            SearchPathPatternBuilder::List(patterns) => {
                let patterns: Vec<String> = patterns.iter().filter(|p| !p.is_empty()).map(|p| normalize_slashes(p)).collect();
                if patterns.is_empty() {
                    return ISearchPathsInfo::default();
                }
                SearchPathPatternBuilder::List(patterns)
            }
            SearchPathPatternBuilder::Single(single) => {
                if single.is_empty() {
                    return ISearchPathsInfo::default();
                }
                SearchPathPatternBuilder::Single(normalize_slashes(single))
            }
        };

        if expand_patterns.is_truthy() {
            self.parse_search_paths(&pattern)
        } else {
            let patterns = match &pattern {
                SearchPathPatternBuilder::List(patterns) => patterns.clone(),
                SearchPathPatternBuilder::Single(single) => vec![single.clone()],
            };
            ISearchPathsInfo { search_paths: None, pattern: pattern_list_to_expression(&patterns) }
        }
    }

    fn common_query(&self, folder_resources: &[WorkspaceFolderOrUri], options: &ICommonQueryBuilderOptions) -> ICommonQueryProps<URI> {
        let exclude_patterns: Option<SearchPathPatternBuilder> = options.exclude_pattern.as_ref().map(|builders| {
            let flat: Vec<String> = builders
                .iter()
                .flat_map(|builder| match &builder.pattern {
                    SearchPathPatternBuilder::Single(single) => vec![single.clone()],
                    SearchPathPatternBuilder::List(patterns) => patterns.clone(),
                })
                .collect();
            if flat.len() == 1 { SearchPathPatternBuilder::Single(flat[0].clone()) } else { SearchPathPatternBuilder::List(flat) }
        });
        let include_search_paths_info = self.handle_include_exclude(options.include_pattern.as_ref(), options.expand_patterns);
        let exclude_search_paths_info = self.handle_include_exclude(exclude_patterns.as_ref(), options.expand_patterns);

        // Build folderQueries from searchPaths, if given, otherwise folderResources
        let include_folder_name = folder_resources.len() > 1;
        let using_search_paths = include_search_paths_info.search_paths.as_ref().is_some_and(|paths| !paths.is_empty());
        let folder_queries: Vec<IFolderQuery<URI>> = if using_search_paths {
            include_search_paths_info
                .search_paths
                .iter()
                .flatten()
                .filter_map(|search_path| self.get_folder_query_for_search_path(search_path, options, &exclude_search_paths_info))
                .collect()
        } else {
            folder_resources
                .iter()
                .filter_map(|folder| self.get_folder_query_for_root(folder, options, &exclude_search_paths_info, include_folder_name))
                .collect()
        };

        let mut query_props = ICommonQueryProps {
            reason: options.reason.clone(),
            folder_queries,
            using_search_paths: Some(using_search_paths),
            extra_file_resources: options.extra_file_resources.clone(),

            exclude_pattern: exclude_search_paths_info.pattern,
            include_pattern: include_search_paths_info.pattern,
            ignore_glob_case: options.ignore_glob_case,
            only_open_editors: options.only_open_editors,
            max_results: options.max_results,
            only_file_scheme: options.only_file_scheme,
        };

        if options.only_open_editors.is_truthy() {
            let open_editors: Vec<URI> =
                self.editor_groups_service.groups().iter().flat_map(|group| group.editors()).filter_map(|editor| editor.resource()).collect();
            self.log_service.trace_args("QueryBuilder#commonQuery - openEditor URIs", &[LogArg::from(uris_to_json(&open_editors).as_str())]);
            let open_editors_in_query: Vec<URI> = open_editors.into_iter().filter(|editor| path_included_in_query(&query_props, &fs_path(editor))).collect();
            let open_editors_query_props = self.common_query_from_file_list(&open_editors_in_query);
            self.log_service.trace_args("QueryBuilder#commonQuery - openEditor Query", &[LogArg::from(query_props_to_json(&open_editors_query_props).as_str())]);
            return merge_file_list_props(query_props, open_editors_query_props);
        }

        if let Some(changed_file_uris) = &options.changed_file_uris {
            let changed_files_in_query: Vec<URI> = changed_file_uris.iter().filter(|uri| path_included_in_query(&query_props, &fs_path(uri))).cloned().collect();
            let changed_files_query_props = self.common_query_from_file_list(&changed_files_in_query);
            self.log_service.trace_args("QueryBuilder#commonQuery - changedFile Query", &[LogArg::from(query_props_to_json(&changed_files_query_props).as_str())]);
            return merge_file_list_props(query_props, changed_files_query_props);
        }

        // Filter extraFileResources against global include/exclude patterns - they are already expected to not belong to a workspace
        let extra_file_resources: Option<Vec<URI>> =
            options.extra_file_resources.as_ref().map(|resources| resources.iter().filter(|extra_file| path_included_in_query(&query_props, &fs_path(extra_file))).cloned().collect());
        query_props.extra_file_resources = extra_file_resources.filter(|resources| !resources.is_empty());

        query_props
    }

    fn common_query_from_file_list(&self, files: &[URI]) -> ICommonQueryProps<URI> {
        let mut folder_queries: Vec<IFolderQuery<URI>> = Vec::new();
        let mut folders_to_search: ResourceMap<usize> = ResourceMap::new();
        let mut include_pattern = Expression::new();
        let mut has_included_file = false;
        let env = host_environment();
        for file in files {
            if file.scheme() == schemas::WALK_THROUGH {
                continue;
            }

            let provider_exists = ext_uri().is_absolute_path(file);
            // Special case userdata as we don't have a search provider for it, but it can be searched.
            if provider_exists {
                let search_root = self
                    .workspace_context_service
                    .get_workspace_folder(file)
                    .map(|folder| folder.uri)
                    .unwrap_or_else(|| self.uri_identity_service.ext_uri().dirname(file));

                let index = match folders_to_search.get(&search_root) {
                    Some(index) => *index,
                    None => {
                        has_included_file = true;
                        let mut folder_query = IFolderQuery::new(search_root.clone());
                        folder_query.include_pattern = Some(Expression::new());
                        folder_queries.push(folder_query);
                        folders_to_search.set(search_root.clone(), folder_queries.len() - 1);
                        folder_queries.len() - 1
                    }
                };

                let rel_path = PathModule::native(env.as_ref()).relative(&fs_path(&search_root), &fs_path(file));
                folder_queries[index]
                    .include_pattern
                    .as_mut()
                    .expect("o includePattern da pasta é criado junto com ela")
                    .insert(escape_glob_pattern(&rel_path.replace('\\', "/")), true);
            } else {
                let file_fs_path = fs_path(file);
                if !file_fs_path.is_empty() {
                    has_included_file = true;
                    include_pattern.insert(escape_glob_pattern(&file_fs_path), true);
                }
            }
        }

        let mut props = ICommonQueryProps::new(folder_queries);
        props.include_pattern = Some(include_pattern);
        props.using_search_paths = Some(true);
        props.exclude_pattern = if has_included_file { None } else { Some(Expression::from_entries([("**/*", true)])) };
        props
    }

    /// Resolve isCaseSensitive flag based on the query and the isSmartCase flag, for search providers that don't support
    /// smart case natively.
    fn is_case_sensitive(&self, content_pattern: &PatternInfo, options: &ITextQueryBuilderOptions) -> bool {
        if options.is_smart_case.is_truthy() {
            if content_pattern.is_reg_exp.is_truthy() {
                // Consider it case sensitive if it contains an unescaped capital letter
                if strings::contains_uppercase_character(content_pattern.pattern.units(), true) {
                    return true;
                }
            } else if strings::contains_uppercase_character(content_pattern.pattern.units(), false) {
                return true;
            }
        }

        content_pattern.is_case_sensitive.is_truthy()
    }

    fn is_multiline(&self, content_pattern: &PatternInfo) -> bool {
        if content_pattern.is_multiline.is_truthy() {
            return true;
        }

        if content_pattern.is_reg_exp.is_truthy() && is_multiline_reg_exp_source(content_pattern.pattern.units()) {
            return true;
        }

        content_pattern.pattern.units().contains(&u16::from(b'\n'))
    }

    /// Take the includePattern as seen in the search viewlet, and split into components that look like searchPaths, and
    /// glob patterns. Glob patterns are expanded from 'foo/bar' to '{foo/bar/**, **\/foo/bar}.
    ///
    /// Public for test.
    pub fn parse_search_paths(&self, pattern: &SearchPathPatternBuilder) -> ISearchPathsInfo {
        let is_search_path = |segment: &str| {
            // A segment is a search path if it is an absolute path or starts with ./, ../, .\, or ..\
            PathModule::native(host_environment().as_ref()).is_absolute(segment) || starts_with_relative_dots(segment)
        };

        let patterns: Vec<String> = match pattern {
            SearchPathPatternBuilder::List(patterns) => patterns.clone(),
            SearchPathPatternBuilder::Single(single) => split_glob_pattern(single),
        };
        let user_home = self.path_service.resolved_user_home();
        let segments: Vec<String> = patterns
            .iter()
            .map(|segment| match &user_home {
                Some(user_home) => untildify(segment, &if user_home.scheme() == schemas::FILE { fs_path(user_home) } else { user_home.path().to_string_lossy() }),
                None => segment.clone(),
            })
            .collect();
        let (search_path_segments, expr_segments): (Vec<String>, Vec<String>) = segments.into_iter().partition(|segment| is_search_path(segment));

        let expanded_expr_segments: Vec<Vec<String>> = expr_segments
            .iter()
            .map(|segment| segment.trim_end_matches('/').trim_end_matches('\\').to_string())
            .map(|mut p| {
                if p.starts_with('.') {
                    p = format!("*{p}"); // convert ".js" to "*.js"
                }

                expand_global_glob(&p)
            })
            .collect();

        let mut result = ISearchPathsInfo::default();
        let search_paths = self.expand_search_path_patterns(&search_path_segments);
        if !search_paths.is_empty() {
            result.search_paths = Some(search_paths);
        }

        let expr_segments: Vec<String> = expanded_expr_segments.into_iter().flatten().collect();
        if let Some(include_pattern) = pattern_list_to_expression(&expr_segments) {
            result.pattern = Some(include_pattern);
        }

        result
    }

    fn get_excludes_for_folder(&self, folder_config: &Dynamic, options: &ICommonQueryBuilderOptions) -> Option<Expression> {
        if options.disregard_exclude_settings.is_truthy() { None } else { get_excludes(folder_config, !options.disregard_search_exclude_settings.is_truthy()) }
    }

    /// Split search paths (./ or ../ or absolute paths in the includePatterns) into absolute paths and globs applied to
    /// those paths.
    fn expand_search_path_patterns(&self, search_paths: &[String]) -> Vec<ISearchPathPattern> {
        if search_paths.is_empty() {
            // No workspace => ignore search paths
            return Vec::new();
        }

        let expanded_search_paths: Vec<OneSearchPathPattern> = search_paths
            .iter()
            .flat_map(|search_path| {
                // 1 open folder => just resolve the search paths to absolute paths
                let (path_portion, glob_portion) = split_glob_from_path(search_path);
                let glob_portion = glob_portion.map(|portion| normalize_glob_pattern(&portion));

                // One pathPortion to multiple expanded search paths (e.g. duplicate matching workspace folders)
                let one_expanded = self.expand_one_search_path(&path_portion);

                // Expanded search paths to multiple resolved patterns (with ** and without)
                one_expanded.into_iter().flat_map(|one_expanded_result| resolve_one_search_path_pattern(one_expanded_result, glob_portion.as_deref())).collect::<Vec<_>>()
            })
            .collect();

        // O `Map<string, ISearchPathPattern>` do original, na ordem de inserção.
        let mut search_path_pattern_map: Vec<(String, ISearchPathPattern)> = Vec::new();
        for one_search_path_pattern in expanded_search_paths {
            let key = one_search_path_pattern.search_path.to_string(false).to_string_lossy();
            match search_path_pattern_map.iter_mut().find(|(existing_key, _)| *existing_key == key) {
                Some((_, existing)) => {
                    if let Some(pattern) = &one_search_path_pattern.pattern {
                        existing.pattern.get_or_insert_with(Expression::new).insert(pattern.clone(), true);
                    }
                }
                None => {
                    let pattern = one_search_path_pattern.pattern.as_ref().and_then(|pattern| pattern_list_to_expression(std::slice::from_ref(pattern)));
                    search_path_pattern_map.push((key, ISearchPathPattern { search_path: one_search_path_pattern.search_path, pattern }));
                }
            }
        }

        search_path_pattern_map.into_iter().map(|(_, search_path_pattern)| search_path_pattern).collect()
    }

    /// Takes a searchPath like `./a/foo` or `../a/foo` and expands it to absolute paths for all the workspaces it matches.
    fn expand_one_search_path(&self, search_path: &str) -> Vec<OneSearchPathPattern> {
        let env = host_environment();
        let native = PathModule::native(env.as_ref());
        if native.is_absolute(search_path) {
            let workspace_folders = self.workspace_context_service.get_workspace().folders();
            if let Some(first) = workspace_folders.first().filter(|folder| folder.uri.scheme() != schemas::FILE) {
                return vec![OneSearchPathPattern {
                    search_path: first.uri.with(&UriChange { path: Field::Set(JsString::from(search_path)), ..UriChange::default() }),
                    pattern: None,
                }];
            }

            // Currently only local resources can be searched for with absolute search paths.
            // TODO convert this to a workspace folder + pattern, so excludes will be resolved properly for an absolute path inside a workspace folder
            return vec![OneSearchPathPattern { search_path: uri_file(&native.normalize(search_path)), pattern: None }];
        }

        if self.workspace_context_service.get_workbench_state() == WorkbenchState::Folder {
            let workspace_uri = self.workspace_context_service.get_workspace().folders()[0].uri.clone();

            let search_path = normalize_slashes(search_path);
            if search_path.starts_with("../") || search_path == ".." {
                let resolved_path = PathModule::posix(env.as_ref()).resolve(&[&workspace_uri.path().to_string_lossy(), &search_path]);
                return vec![OneSearchPathPattern {
                    search_path: workspace_uri.with(&UriChange { path: Field::Set(JsString::from(resolved_path)), ..UriChange::default() }),
                    pattern: None,
                }];
            }

            let cleaned_pattern = normalize_glob_pattern(&search_path);
            vec![OneSearchPathPattern { search_path: workspace_uri, pattern: non_empty(cleaned_pattern) }]
        } else if search_path == "./" || search_path == ".\\" {
            Vec::new() // ./ or ./**/foo makes sense for single-folder but not multi-folder workspaces
        } else {
            let search_path_without_dot_slash = strip_dot_slash(search_path);
            let folders = self.workspace_context_service.get_workspace().folders();
            let folder_matches: Vec<OneSearchPathPattern> = folders
                .iter()
                .filter_map(|folder| {
                    let pattern_match = match_folder_name(search_path_without_dot_slash, &folder.name)?;
                    Some(OneSearchPathPattern { search_path: folder.uri.clone(), pattern: pattern_match.and_then(|pattern| non_empty(normalize_glob_pattern(&pattern))) })
                })
                .collect();

            if !folder_matches.is_empty() {
                folder_matches
            } else {
                let probable_workspace_folder_name = probable_folder_name(search_path).unwrap_or_else(|| search_path.to_string());

                // No root folder with name
                let search_path_not_found_error = localize("search.noWorkspaceWithName", "Workspace folder does not exist: {0}", &[&probable_workspace_folder_name]);
                errors::throw(Error::new(search_path_not_found_error))
            }
        }
    }

    fn get_folder_query_for_search_path(&self, search_path: &ISearchPathPattern, options: &ICommonQueryBuilderOptions, search_path_excludes: &ISearchPathsInfo) -> Option<IFolderQuery<URI>> {
        let folder = WorkspaceFolderOrUri::Folder(to_folder(&search_path.search_path));
        let root_config = self.get_folder_query_for_root(&folder, options, search_path_excludes, false)?;

        Some(IFolderQuery { include_pattern: search_path.pattern.clone(), ..root_config })
    }

    fn get_folder_query_for_root(
        &self,
        folder: &WorkspaceFolderOrUri,
        options: &ICommonQueryBuilderOptions,
        search_path_excludes: &ISearchPathsInfo,
        include_folder_name: bool,
    ) -> Option<IFolderQuery<URI>> {
        let mut this_folder_exclude_search_path_pattern: Option<Expression> = None;
        let folder_uri = folder.uri().clone();

        // only use exclude root if it is different from the folder root
        let mut exclude_folder_roots: Vec<Option<URI>> = options
            .exclude_pattern
            .iter()
            .flatten()
            .map(|exclude_pattern| {
                let exclude_root = exclude_pattern.uri.clone();
                let should_use_exclude_root = match (&exclude_root, folder) {
                    (Some(root), WorkspaceFolderOrUri::Uri(folder_uri)) => !self.uri_identity_service.ext_uri().is_equal(Some(folder_uri), Some(root), false),
                    _ => true,
                };
                if should_use_exclude_root { exclude_root } else { None }
            })
            .collect();

        if exclude_folder_roots.is_empty() {
            exclude_folder_roots = vec![None];
        }

        if let Some(search_paths) = &search_path_excludes.search_paths {
            let this_folder_exclude_search_path = search_paths.iter().find(|sp| ext_uri().is_equal(Some(&sp.search_path), Some(&folder_uri), false));
            match this_folder_exclude_search_path {
                Some(exclude) if exclude.pattern.is_none() => {
                    // entire folder is excluded
                    return None;
                }
                Some(exclude) => this_folder_exclude_search_path_pattern = exclude.pattern.clone(),
                None => {}
            }
        }

        let folder_config = self.configuration_service.get_value(None, Some(&ConfigurationOverrides { resource: Some(folder_uri.clone()), override_identifier: None }));
        let setting_excludes = self.get_excludes_for_folder(&folder_config, options);
        let mut exclude_pattern = Expression::new();
        for source in [setting_excludes, this_folder_exclude_search_path_pattern].into_iter().flatten() {
            for (pattern, value) in source.entries() {
                exclude_pattern.insert(pattern.clone(), value.clone());
            }
        }

        let folder_name = match folder {
            WorkspaceFolderOrUri::Uri(uri) => ext_uri().basename(uri, None),
            WorkspaceFolderOrUri::Folder(folder) => folder.name.clone(),
        };

        let exclude_pattern_ret: Vec<ExcludeGlobPattern<URI>> = exclude_folder_roots
            .into_iter()
            .filter(|_| !exclude_pattern.is_empty())
            .map(|exclude_folder_root| ExcludeGlobPattern { folder: exclude_folder_root, pattern: exclude_pattern.clone() })
            .collect();

        let search_config = |key: &str| {
            let search = folder_config.get_str("search");
            if search.is_nullish() { Dynamic::Undefined } else { search.get_str(key) }
        };
        let files = folder_config.get_str("files");
        let file_encoding = if files.is_truthy() { files.get_str("encoding") } else { Dynamic::Undefined };

        Some(IFolderQuery {
            folder: folder_uri,
            folder_name: include_folder_name.then_some(folder_name),
            exclude_pattern: Some(exclude_pattern_ret),
            include_pattern: None,
            ignore_glob_case: options.ignore_glob_case,
            file_encoding: match file_encoding {
                Dynamic::String(encoding) => Some(encoding.to_string_lossy()),
                _ => None,
            },
            disregard_ignore_files: Some(options.disregard_ignore_files.unwrap_or_else(|| !search_config("useIgnoreFiles").is_truthy())),
            disregard_global_ignore_files: Some(options.disregard_global_ignore_files.unwrap_or_else(|| !search_config("useGlobalIgnoreFiles").is_truthy())),
            disregard_parent_ignore_files: Some(options.disregard_parent_ignore_files.unwrap_or_else(|| !search_config("useParentIgnoreFiles").is_truthy())),
            ignore_symlinks: Some(options.ignore_symlinks.unwrap_or_else(|| !search_config("followSymlinks").is_truthy())),
        })
    }
}

/// `folderResources?.map(toWorkspaceFolder)`.
fn to_folders(folder_resources: Option<&[URI]>) -> Vec<WorkspaceFolderOrUri> {
    folder_resources.unwrap_or_default().iter().map(|resource| WorkspaceFolderOrUri::Folder(to_folder(resource))).collect()
}

/// `toWorkspaceFolder(resource)`: o nome é o `basenameOrAuthority`.
fn to_folder(resource: &URI) -> WorkspaceFolder {
    to_workspace_folder(resource, ext_uri().basename_or_authority(resource))
}

/// `{ ...queryProps, ...fileListQueryProps }`: as propriedades da lista de arquivos (que existem todas, mesmo
/// `undefined`) vencem.
fn merge_file_list_props(mut query_props: ICommonQueryProps<URI>, file_list_props: ICommonQueryProps<URI>) -> ICommonQueryProps<URI> {
    query_props.folder_queries = file_list_props.folder_queries;
    query_props.include_pattern = file_list_props.include_pattern;
    query_props.using_search_paths = file_list_props.using_search_paths;
    query_props.exclude_pattern = file_list_props.exclude_pattern;
    query_props
}

/// `.replace(/\r?\n/g, '\\n')`.
fn replace_line_breaks(pattern: &str) -> String {
    pattern.replace("\r\n", "\\n").replace('\n', "\\n")
}

/// `/^\.\.?([\/\\]|$)/.test(segment)`.
fn starts_with_relative_dots(segment: &str) -> bool {
    let at_boundary = |rest: &str| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\');
    match segment.strip_prefix('.') {
        Some(rest) => at_boundary(rest) || rest.strip_prefix('.').is_some_and(at_boundary),
        None => false,
    }
}

/// `searchPath.replace(/^\.[\/\\]/, '')`.
fn strip_dot_slash(search_path: &str) -> &str {
    match search_path.strip_prefix('.') {
        Some(rest) if rest.starts_with('/') || rest.starts_with('\\') => &rest[1..],
        _ => search_path,
    }
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `searchPath.match(new RegExp(`^${escapeRegExpCharacters(name)}(?:/(.*)|$)`))`: `None` se não casa; senão o grupo
/// `(.*)` (`None` quando o casamento foi pelo `$`).
fn match_folder_name(search_path: &str, folder_name: &str) -> Option<Option<String>> {
    let rest = search_path.strip_prefix(folder_name)?;
    if rest.is_empty() {
        return Some(None);
    }
    let after_slash = rest.strip_prefix('/')?;
    Some(Some(after_slash.split(is_line_terminator).next().unwrap_or_default().to_string()))
}

/// `searchPath.match(/\.[\/\\](.+)[\/\\]?/)[1]`.
fn probable_folder_name(search_path: &str) -> Option<String> {
    let chars: Vec<char> = search_path.chars().collect();
    (0..chars.len()).find_map(|i| {
        let is_dot_slash = chars[i] == '.' && chars.get(i + 1).is_some_and(|c| *c == '/' || *c == '\\');
        if !is_dot_slash {
            return None;
        }
        let name: String = chars[i + 2..].iter().take_while(|c| !is_line_terminator(**c)).collect();
        (!name.is_empty()).then_some(name)
    })
}

/// `s.trim()` do JavaScript.
fn js_trim(text: &str) -> String {
    JsString::from(js_string::trim(JsString::from(text).units())).to_string_lossy()
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn split_glob_from_path(search_path: &str) -> (String, Option<String>) {
    let glob_char_idx = search_path.find(['*', '{', '}', '(', ')', '[', ']', '?']);
    if let Some(glob_char_idx) = glob_char_idx {
        // `searchPath.substr(0, globCharIdx).match(/[/|\\][^/\\]*$/)`: a primeira barra (ou `|`) depois da qual não há outra barra.
        let before = &search_path[..glob_char_idx];
        let last_slash_idx = before.char_indices().find(|(idx, c)| matches!(c, '/' | '|' | '\\') && !before[idx + c.len_utf8()..].contains(['/', '\\'])).map(|(idx, _)| idx);
        if let Some(last_slash_idx) = last_slash_idx {
            let mut path_portion = search_path[..last_slash_idx].to_string();
            if !path_portion.contains(['/', '\\']) {
                // If the last slash was the only slash, then we now have '' or 'C:' or '.'. Append a slash.
                path_portion.push('/');
            }

            return (path_portion, Some(search_path[last_slash_idx + 1..].to_string()));
        }
    }

    // No glob char, or malformed
    (search_path.to_string(), None)
}

fn pattern_list_to_expression(patterns: &[String]) -> Option<Expression> {
    (!patterns.is_empty()).then(|| Expression::from_entries(patterns.iter().map(|pattern| (pattern.clone(), ExprValue::Bool(true)))))
}

fn split_glob_pattern(pattern: &str) -> Vec<String> {
    glob::split_glob_aware(pattern, ',').iter().map(|s| js_trim(s)).filter(|s| !s.is_empty()).collect()
}

/// Note - we used {} here previously but ripgrep can't handle nested {} patterns. See
/// https://github.com/microsoft/vscode/issues/32761
fn expand_global_glob(pattern: &str) -> Vec<String> {
    [format!("**/{pattern}/**"), format!("**/{pattern}")].into_iter().map(|p| p.replace("**/**", "**")).collect()
}

fn normalize_slashes(pattern: &str) -> String {
    pattern.replace('\\', "/")
}

/// Normalize slashes, remove `./` and trailing slashes.
fn normalize_glob_pattern(pattern: &str) -> String {
    let normalized = normalize_slashes(pattern);
    let without_dot_slash = normalized.strip_prefix("./").unwrap_or(&normalized);
    without_dot_slash.trim_end_matches('/').to_string()
}

fn resolve_one_search_path_pattern(one_expanded_result: OneSearchPathPattern, glob_portion: Option<&str>) -> Vec<OneSearchPathPattern> {
    let glob_portion = glob_portion.filter(|portion| !portion.is_empty());
    let pattern = match (&one_expanded_result.pattern, glob_portion) {
        (Some(pattern), Some(glob_portion)) => Some(format!("{pattern}/{glob_portion}")),
        (pattern, glob_portion) => pattern.clone().or_else(|| glob_portion.map(str::to_string)),
    };

    let mut results = vec![OneSearchPathPattern { search_path: one_expanded_result.search_path.clone(), pattern: pattern.clone() }];

    if let Some(pattern) = pattern.filter(|pattern| !pattern.ends_with("**")) {
        results.push(OneSearchPathPattern { search_path: one_expanded_result.search_path, pattern: Some(format!("{pattern}/**")) });
    }

    results
}

/// Escapes a path for use as a glob pattern that would match the input precisely.
/// Characters '?', '*', '[', and ']' are escaped into character range glob syntax
/// (for example, '?' becomes '[?]').
/// NOTE: This implementation makes no special cases for UNC paths. For example,
/// given the input "//?/C:/A?.txt", this would produce output '//[?]/C:/A[?].txt',
/// which may not be desirable in some cases. Use with caution if UNC paths could be expected.
pub fn escape_glob_pattern(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '?' | '*' | '[' | ']') {
            escaped.push('[');
            escaped.push(c);
            escaped.push(']');
        } else {
            escaped.push(c);
        }
    }
    escaped
}

/// Construct an include pattern from a list of folders uris to search in.
pub fn resolve_resources_for_search_includes(resources: &[URI], context_service: &dyn IWorkspaceContextService) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let resources: Vec<&URI> = resources
        .iter()
        .filter(|resource| {
            let key = resource.to_string(false).to_string_lossy();
            let is_new = !seen.contains(&key);
            seen.push(key);
            is_new
        })
        .collect();

    let mut folder_paths: Vec<String> = Vec::new();
    let workspace = context_service.get_workspace();

    for resource in resources {
        let mut folder_path: Option<String> = None;
        if context_service.get_workbench_state() == WorkbenchState::Folder {
            // Show relative path from the root for single-root mode
            folder_path = ext_uri().relative_path(&workspace.folders()[0].uri, resource); // always uses forward slashes
            if let Some(path) = folder_path.as_mut().filter(|path| !path.is_empty() && *path != ".") {
                *path = format!("./{path}");
            }
        } else if let Some(owning_folder) = context_service.get_workspace_folder(resource) {
            let owning_root_name = &owning_folder.name;
            // If this root is the only one with its basename, use a relative ./ path. If there is another, use an absolute path
            let is_unique_folder = workspace.folders().iter().filter(|folder| folder.name == *owning_root_name).count() == 1;
            if is_unique_folder {
                let rel_path = ext_uri().relative_path(&owning_folder.uri, resource); // always uses forward slashes
                folder_path = Some(match rel_path.as_deref() {
                    Some("") => format!("./{}", owning_folder.name),
                    Some(rel_path) => format!("./{}/{rel_path}", owning_folder.name),
                    None => format!("./{}/undefined", owning_folder.name),
                });
            } else {
                folder_path = Some(fs_path(resource)); // TODO rob: handle non-file URIs
            }
        }

        if let Some(folder_path) = folder_path.filter(|path| !path.is_empty()) {
            folder_paths.push(escape_glob_pattern(&folder_path));
        }
    }
    folder_paths
}

/// O objeto que `JSON.stringify(uri)` serializa (`Uri#toJSON`), na ordem de propriedades do original.
fn uri_to_dynamic(uri: &URI) -> Dynamic {
    let state = uri.to_json();
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(mid) = state.mid {
        entries.push(("$mid", Dynamic::from(mid)));
    }
    if let Some(fs_path) = &state.fs_path {
        entries.push(("fsPath", Dynamic::from(fs_path.clone())));
    }
    if let Some(sep) = state.sep {
        entries.push(("_sep", Dynamic::from(i32::from(sep))));
    }
    if let Some(external) = &state.external {
        entries.push(("external", Dynamic::from(external.clone())));
    }
    for (key, value) in [
        ("path", &state.components.path),
        ("scheme", &state.components.scheme),
        ("authority", &state.components.authority),
        ("query", &state.components.query),
        ("fragment", &state.components.fragment),
    ] {
        if let Some(value) = value.as_ref().filter(|value| !value.is_empty()) {
            entries.push((key, Dynamic::from(value.clone())));
        }
    }
    Dynamic::object(entries)
}

fn uris_to_json(uris: &[URI]) -> String {
    json_stringify(&Dynamic::array(uris.iter().map(uri_to_dynamic))).unwrap_or_default()
}

fn expression_to_dynamic(expression: &Expression) -> Dynamic {
    Dynamic::object(expression.entries().iter().map(|(pattern, value)| {
        let value = match value {
            ExprValue::Bool(enabled) => Dynamic::Bool(*enabled),
            ExprValue::When(when) => Dynamic::object([("when", Dynamic::string(when))]),
            ExprValue::Other => Dynamic::object(Vec::<(&str, Dynamic)>::new()),
        };
        (pattern.as_str(), value)
    }))
}

fn folder_query_to_dynamic(folder_query: &IFolderQuery<URI>) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = vec![("folder", uri_to_dynamic(&folder_query.folder))];
    if let Some(name) = &folder_query.folder_name {
        entries.push(("folderName", Dynamic::string(name)));
    }
    if let Some(excludes) = &folder_query.exclude_pattern {
        let excludes = excludes.iter().map(|exclude| {
            let mut item: Vec<(&str, Dynamic)> = Vec::new();
            if let Some(folder) = &exclude.folder {
                item.push(("folder", uri_to_dynamic(folder)));
            }
            item.push(("pattern", expression_to_dynamic(&exclude.pattern)));
            Dynamic::object(item)
        });
        entries.push(("excludePattern", Dynamic::array(excludes)));
    }
    if let Some(include) = &folder_query.include_pattern {
        entries.push(("includePattern", expression_to_dynamic(include)));
    }
    if let Some(ignore_glob_case) = folder_query.ignore_glob_case {
        entries.push(("ignoreGlobCase", Dynamic::Bool(ignore_glob_case)));
    }
    if let Some(encoding) = &folder_query.file_encoding {
        entries.push(("fileEncoding", Dynamic::string(encoding)));
    }
    for (key, value) in [
        ("disregardIgnoreFiles", folder_query.disregard_ignore_files),
        ("disregardGlobalIgnoreFiles", folder_query.disregard_global_ignore_files),
        ("disregardParentIgnoreFiles", folder_query.disregard_parent_ignore_files),
        ("ignoreSymlinks", folder_query.ignore_symlinks),
    ] {
        if let Some(value) = value {
            entries.push((key, Dynamic::Bool(value)));
        }
    }
    Dynamic::object(entries)
}

/// `JSON.stringify(queryProps)`.
fn query_props_to_json(props: &ICommonQueryProps<URI>) -> String {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(reason) = &props.reason {
        entries.push(("_reason", Dynamic::string(reason)));
    }
    entries.push(("folderQueries", Dynamic::array(props.folder_queries.iter().map(folder_query_to_dynamic))));
    if let Some(include) = &props.include_pattern {
        entries.push(("includePattern", expression_to_dynamic(include)));
    }
    if let Some(exclude) = &props.exclude_pattern {
        entries.push(("excludePattern", expression_to_dynamic(exclude)));
    }
    if let Some(ignore_glob_case) = props.ignore_glob_case {
        entries.push(("ignoreGlobCase", Dynamic::Bool(ignore_glob_case)));
    }
    if let Some(extra) = &props.extra_file_resources {
        entries.push(("extraFileResources", Dynamic::array(extra.iter().map(uri_to_dynamic))));
    }
    if let Some(only_open_editors) = props.only_open_editors {
        entries.push(("onlyOpenEditors", Dynamic::Bool(only_open_editors)));
    }
    if let Some(max_results) = props.max_results {
        entries.push(("maxResults", Dynamic::Number(max_results as f64)));
    }
    if let Some(using_search_paths) = props.using_search_paths {
        entries.push(("usingSearchPaths", Dynamic::Bool(using_search_paths)));
    }
    if let Some(only_file_scheme) = props.only_file_scheme {
        entries.push(("onlyFileScheme", Dynamic::Bool(only_file_scheme)));
    }
    json_stringify(&Dynamic::object(entries)).unwrap_or_default()
}
