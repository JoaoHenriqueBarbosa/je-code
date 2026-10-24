//! Porte de `src/vs/workbench/contrib/search/browser/search.common.contribution.ts`: o registro das definições de
//! configuração de busca (`search.exclude`, `search.useIgnoreFiles`...), compartilhado entre o workbench principal e a janela de
//! agentes.
//!
//! Notas de tradução:
//! - **Efeito de importação.** O original registra ao ser importado (`import '../../browser/search.common.contribution.js'`);
//!   aqui o registro é a função [`register_search_common_configuration`], que o consumidor (o teste, ou o ponto de partida do
//!   workbench) chama. O registro global é por fio, como o `Registry` de plataforma do porte.
//! - **`replaceContributions()`, `notebookSearchContributions()` e `registerSingleton(ISearchHistoryService, ...)`.** Registram
//!   comandos, ações e o serviço de histórico, que dependem do registro de ações e de serviços de plataforma ainda não ligados
//!   a este módulo; entram com os consumidores que os leem. Aqui fica o registro de configuração, que é o que o teste de
//!   configuração compartilhada exercita.
//! - **`...searchConfigurationNode`.** O nó vem de [`search_configuration_node`] com as propriedades preenchidas.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationScope, PropertySchema, configuration_registry};

use crate::contrib::search::search::search_configuration_node;
use crate::services::search::search::{DEFAULT_MAX_SEARCH_RESULTS, SEARCH_EXCLUDE_CONFIG, SearchSortOrder};

/// `{ type, description|markdownDescription, default, scope?, ...extra }`, com os campos na ordem em que o original os escreve.
fn property(entries: Vec<(&str, Dynamic)>) -> PropertySchema {
    PropertySchema::from_entries(entries)
}

/// O registro de configuração do fim do arquivo original: `registerConfiguration({ ...searchConfigurationNode, properties })`.
pub fn register_search_common_configuration() {
    let exclude = property(vec![
        ("type", Dynamic::string("object")),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "exclude",
                "Configure [glob patterns](https://code.visualstudio.com/docs/editor/codebasics#_advanced-search-options) for excluding files and folders in fulltext searches and file search in quick open. To exclude files from the recently opened list in quick open, patterns must be absolute (for example `**/node_modules/**`). Inherits all glob patterns from the `#files.exclude#` setting.",
                &[],
            )),
        ),
        (
            "default",
            Dynamic::object([("**/node_modules", Dynamic::Bool(true)), ("**/bower_components", Dynamic::Bool(true)), ("**/*.code-search", Dynamic::Bool(true))]),
        ),
        (
            "additionalProperties",
            Dynamic::object([(
                "anyOf",
                Dynamic::array([
                    Dynamic::object([
                        ("type", Dynamic::string("boolean")),
                        (
                            "description",
                            Dynamic::string(&localize(
                                "exclude.boolean",
                                "The glob pattern to match file paths against. Set to true or false to enable or disable the pattern.",
                                &[],
                            )),
                        ),
                    ]),
                    Dynamic::object([
                        ("type", Dynamic::string("object")),
                        (
                            "properties",
                            Dynamic::object([(
                                "when",
                                Dynamic::object([
                                    // expression ({ "**/*.js": { "when": "$(basename).js" } })
                                    ("type", Dynamic::string("string")),
                                    ("pattern", Dynamic::string("\\w*\\$\\(basename\\)\\w*")),
                                    ("default", Dynamic::string("$(basename).ext")),
                                    (
                                        "markdownDescription",
                                        Dynamic::string(&localize(
                                            "exclude.when",
                                            "Additional check on the siblings of a matching file. Use \\$(basename) as variable for the matching file name.",
                                            &[],
                                        )),
                                    ),
                                ]),
                            )]),
                        ),
                    ]),
                ]),
            )]),
        ),
        ("scope", ConfigurationScope::Resource.to_dynamic()),
    ]);

    let properties: Vec<(&str, PropertySchema)> = vec![
        (SEARCH_EXCLUDE_CONFIG, exclude),
        (
            "search.useIgnoreFiles",
            property(vec![
                ("type", Dynamic::string("boolean")),
                (
                    "markdownDescription",
                    Dynamic::string(&localize("useIgnoreFiles", "Controls whether to use `.gitignore` and `.ignore` files when searching for files.", &[])),
                ),
                ("default", Dynamic::Bool(true)),
                ("scope", ConfigurationScope::Resource.to_dynamic()),
            ]),
        ),
        (
            "search.useGlobalIgnoreFiles",
            property(vec![
                ("type", Dynamic::string("boolean")),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "useGlobalIgnoreFiles",
                        "Controls whether to use your global gitignore file (for example, from `$HOME/.config/git/ignore`) when searching for files. Requires {0} to be enabled.",
                        &["`#search.useIgnoreFiles#`"],
                    )),
                ),
                ("default", Dynamic::Bool(false)),
                ("scope", ConfigurationScope::Resource.to_dynamic()),
            ]),
        ),
        (
            "search.useParentIgnoreFiles",
            property(vec![
                ("type", Dynamic::string("boolean")),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "useParentIgnoreFiles",
                        "Controls whether to use `.gitignore` and `.ignore` files in parent directories when searching for files. Requires {0} to be enabled.",
                        &["`#search.useIgnoreFiles#`"],
                    )),
                ),
                ("default", Dynamic::Bool(false)),
                ("scope", ConfigurationScope::Resource.to_dynamic()),
            ]),
        ),
        (
            "search.ripgrep.maxThreads",
            property(vec![
                ("type", Dynamic::string("number")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "search.ripgrep.maxThreads",
                        "Number of threads to use for searching. When set to 0, the engine automatically determines this value.",
                        &[],
                    )),
                ),
                ("default", Dynamic::Number(0.0)),
            ]),
        ),
        (
            "search.followSymlinks",
            property(vec![
                ("type", Dynamic::string("boolean")),
                ("description", Dynamic::string(&localize("search.followSymlinks", "Controls whether to follow symlinks while searching.", &[]))),
                ("default", Dynamic::Bool(true)),
            ]),
        ),
        (
            "search.smartCase",
            property(vec![
                ("type", Dynamic::string("boolean")),
                (
                    "description",
                    Dynamic::string(&localize(
                        "search.smartCase",
                        "Search case-insensitively if the pattern is all lowercase, otherwise, search case-sensitively.",
                        &[],
                    )),
                ),
                ("default", Dynamic::Bool(false)),
            ]),
        ),
        (
            "search.maxResults",
            property(vec![
                ("type", Dynamic::array([Dynamic::string("number"), Dynamic::string("null")])),
                ("default", Dynamic::Number(DEFAULT_MAX_SEARCH_RESULTS as f64)),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "search.maxResults",
                        "Controls the maximum number of search results, this can be set to `null` (empty) to return unlimited results.",
                        &[],
                    )),
                ),
            ]),
        ),
        (
            "search.sortOrder",
            property(vec![
                ("type", Dynamic::string("string")),
                (
                    "enum",
                    Dynamic::array(
                        [
                            SearchSortOrder::Default,
                            SearchSortOrder::FileNames,
                            SearchSortOrder::Type,
                            SearchSortOrder::Modified,
                            SearchSortOrder::CountDescending,
                            SearchSortOrder::CountAscending,
                        ]
                        .map(|sort_order| Dynamic::string(sort_order.as_str())),
                    ),
                ),
                ("default", Dynamic::string(SearchSortOrder::Default.as_str())),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string(&localize("searchSortOrder.default", "Results are sorted by folder and file names, in alphabetical order.", &[])),
                        Dynamic::string(&localize("searchSortOrder.filesOnly", "Results are sorted by file names ignoring folder order, in alphabetical order.", &[])),
                        Dynamic::string(&localize("searchSortOrder.type", "Results are sorted by file extensions, in alphabetical order.", &[])),
                        Dynamic::string(&localize("searchSortOrder.modified", "Results are sorted by file last modified date, in descending order.", &[])),
                        Dynamic::string(&localize("searchSortOrder.countDescending", "Results are sorted by count per file, in descending order.", &[])),
                        Dynamic::string(&localize("searchSortOrder.countAscending", "Results are sorted by count per file, in ascending order.", &[])),
                    ]),
                ),
                ("description", Dynamic::string(&localize("search.sortOrder", "Controls sorting order of search results.", &[]))),
            ]),
        ),
        (
            "search.searchOnType",
            property(vec![
                ("type", Dynamic::string("boolean")),
                ("default", Dynamic::Bool(true)),
                ("description", Dynamic::string(&localize("search.searchOnType", "Search all files as you type.", &[]))),
            ]),
        ),
        (
            "search.searchOnTypeDebouncePeriod",
            property(vec![
                ("type", Dynamic::string("number")),
                ("default", Dynamic::Number(300.0)),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "search.searchOnTypeDebouncePeriod",
                        "When {0} is enabled, controls the timeout in milliseconds between a character being typed and the search starting. Has no effect when {0} is disabled.",
                        &["`#search.searchOnType#`"],
                    )),
                ),
            ]),
        ),
    ];

    configuration_registry().register_configuration(ConfigurationNode::new(search_configuration_node().with_properties(properties)));
}
