//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/searchTreeCommon.ts`: os contratos da árvore de
//! resultados de busca (modelo, resultado, cabeçalho de texto, pasta, arquivo e casamento), os identificadores e os
//! predicados de tipo.
//!
//! Divergências:
//! - **Interfaces** são traits sobre `Rc<dyn ...>`; os pais são guardados como `Weak` pelas implementações (o original
//!   tem coleta de lixo), e `parent()` devolve o `Rc` vivo.
//! - **`RenderableMatch`** (a união `ITextSearchHeading | ISearchTreeFolderMatch | ISearchTreeFileMatch |
//!   ISearchTreeMatch`) é o enum [`RenderableMatch`]; as uniões menores são [`FileMatchOrMatch`],
//!   [`FileOrFolderMatch`] e [`FolderMatchParent`]. A identidade dos elementos (`===`) é [`RenderableMatch::is_same`].
//! - **Predicados de tipo** (`isSearchTreeFileMatch` e afins, que olham o prefixo do `id()` e a existência de métodos)
//!   olham a variante do enum e, para as pastas, `resource()` e [`ISearchTreeFolderMatch::has_create_and_configure_file_match`].
//!   `isPlainTextSearchHeading` (heading com `replace` e `replaceAll`) compara o `id()` do cabeçalho de texto simples.
//! - `ITextSearchHeading.add` tem a assinatura da implementação (`allRaw, searchInstanceID, silent`), porque a da interface
//!   (com `ai`) não é a que `TextSearchHeadingImpl` implementa nem a que `SearchResultImpl` chama.
//! - `ISearchTreeFolderMatch.index()` devolve `i32` (o original passa `-1` nas pastas intermediárias), `resource` nulo é
//!   `Option<URI>`, `replacingAll` (propriedade com `set`) é [`ISearchTreeFolderMatch::set_replacing_all`], e
//!   `closestRoot` de pastas e arquivos é `Option<Rc<dyn ISearchTreeFolderMatch>>` (a raiz do workspace).
//! - `IterableIterator` (`fileMatchesIterator`, `folderMatchesIterator`) é um `Vec` com os elementos naquele momento.
//! - `IMatchInNotebook.parent()` (com tipo de retorno mais estreito) é [`IMatchInNotebook::notebook_parent`](crate::contrib::search::notebook_search::notebook_search_model_base::IMatchInNotebook),
//!   porque o trait `ISearchTreeMatch` já tem `parent()`.
//! - `ISearchTreeFolderMatch::dispose_count_for_testing` existe para o caso `folder matches are not retained across
//!   queries`, que no original espiona `dispose` com o sinon (o porte não troca métodos de uma instância).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{IDisposable, id_of};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::promise::Promise;
use je_base::ternary_search_tree::{TernarySearchTree, UriPredicate};
use je_base::uri::URI;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::range_decorations::RangeHighlightDecorations;
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::search::notebook_search::notebook_search_model_base::{IMatchInNotebook, INotebookFileInstanceMatch};
use crate::services::search::replace::ReplacePattern;
use crate::services::search::search::{
    IFileMatch, ISearchComplete, ISearchProgressItem, ISearchRange, ITextQuery, ITextSearchQuery, ITextSearchResult, SearchProgressCallback,
};
use crate::services::textfile::collaborators::{IProgress, ProgressStep};

/// `FileMatchOrMatch`.
#[derive(Clone)]
pub enum FileMatchOrMatch {
    FileMatch(Rc<dyn ISearchTreeFileMatch>),
    Match(Rc<dyn ISearchTreeMatch>),
}

/// `ISearchTreeFileMatch | ISearchTreeFolderMatch` (os elementos que `remove` aceita).
#[derive(Clone)]
pub enum FileOrFolderMatch {
    File(Rc<dyn ISearchTreeFileMatch>),
    Folder(Rc<dyn ISearchTreeFolderMatch>),
}

/// `ISearchTreeFolderMatch | ITextSearchHeading`: o pai de uma pasta.
#[derive(Clone)]
pub enum FolderMatchParent {
    FolderMatch(Rc<dyn ISearchTreeFolderMatch>),
    Heading(Rc<dyn ITextSearchHeading>),
}

impl FolderMatchParent {
    /// `showHighlights` do pai.
    pub fn show_highlights(&self) -> bool {
        match self {
            FolderMatchParent::FolderMatch(folder_match) => folder_match.show_highlights(),
            FolderMatchParent::Heading(heading) => heading.show_highlights(),
        }
    }

    /// `query` do pai.
    pub fn query(&self) -> Option<ITextSearchQuery> {
        match self {
            FolderMatchParent::FolderMatch(folder_match) => folder_match.query(),
            FolderMatchParent::Heading(heading) => heading.query(),
        }
    }
}

/// `RenderableMatch`.
#[derive(Clone)]
pub enum RenderableMatch {
    Heading(Rc<dyn ITextSearchHeading>),
    FolderMatch(Rc<dyn ISearchTreeFolderMatch>),
    FileMatch(Rc<dyn ISearchTreeFileMatch>),
    Match(Rc<dyn ISearchTreeMatch>),
}

impl RenderableMatch {
    /// `element.id()`.
    pub fn id(&self) -> String {
        match self {
            RenderableMatch::Heading(heading) => heading.id(),
            RenderableMatch::FolderMatch(folder_match) => folder_match.id(),
            RenderableMatch::FileMatch(file_match) => file_match.id(),
            RenderableMatch::Match(match_) => match_.id(),
        }
    }

    /// `a === b`.
    pub fn is_same(&self, other: &RenderableMatch) -> bool {
        match (self, other) {
            (RenderableMatch::Heading(a), RenderableMatch::Heading(b)) => id_of(a) == id_of(b),
            (RenderableMatch::FolderMatch(a), RenderableMatch::FolderMatch(b)) => id_of(a) == id_of(b),
            (RenderableMatch::FileMatch(a), RenderableMatch::FileMatch(b)) => id_of(a) == id_of(b),
            (RenderableMatch::Match(a), RenderableMatch::Match(b)) => id_of(a) == id_of(b),
            _ => false,
        }
    }

    /// `element.parent()`: `None` para o cabeçalho, cujo pai é o `ISearchResult` (que não é um `RenderableMatch`).
    pub fn parent(&self) -> Option<RenderableMatch> {
        match self {
            RenderableMatch::Heading(_) => None,
            RenderableMatch::FolderMatch(folder_match) => Some(match folder_match.parent() {
                FolderMatchParent::FolderMatch(parent) => RenderableMatch::FolderMatch(parent),
                FolderMatchParent::Heading(parent) => RenderableMatch::Heading(parent),
            }),
            RenderableMatch::FileMatch(file_match) => Some(RenderableMatch::FolderMatch(file_match.parent())),
            RenderableMatch::Match(match_) => Some(RenderableMatch::FileMatch(match_.parent())),
        }
    }
}

/// `arrayContainsElementOrParent`.
pub fn array_contains_element_or_parent(element: &RenderableMatch, test_array: &[RenderableMatch]) -> bool {
    let mut element = element.clone();
    loop {
        if test_array.iter().any(|candidate| candidate.is_same(&element)) {
            return true;
        }
        match element.parent() {
            Some(parent) => element = parent,
            None => return false,
        }
    }
}

/// `IChangeEvent`: os campos opcionais do original são `Option` (um campo ausente difere de `false`).
#[derive(Clone, Default)]
pub struct IChangeEvent {
    pub elements: Vec<Rc<dyn ISearchTreeFileMatch>>,
    pub added: Option<bool>,
    pub removed: Option<bool>,
    pub clearing_all: Option<bool>,
}

/// O `{ didRemove?, forceUpdateModel? }` do `onChange` de um arquivo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileMatchChangeEvent {
    pub did_remove: Option<bool>,
    pub force_update_model: Option<bool>,
}

/// `SearchModelLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchModelLocation {
    Panel,
    QuickAccess,
}

pub const PLAIN_TEXT_SEARCH__RESULT_ID: &str = "plainTextSearch";
pub const AI_TEXT_SEARCH_RESULT_ID: &str = "aiTextSearch";

/// `createParentList`.
pub fn create_parent_list(element: &RenderableMatch) -> Vec<RenderableMatch> {
    let mut parent_array = Vec::new();
    let mut curr_element = element.clone();

    while !matches!(curr_element, RenderableMatch::Heading(_)) {
        parent_array.push(curr_element.clone());
        curr_element = curr_element.parent().expect("todo elemento abaixo do cabeçalho tem pai");
    }

    parent_array
}

pub const SEARCH_MODEL_PREFIX: &str = "SEARCH_MODEL_";
pub const SEARCH_RESULT_PREFIX: &str = "SEARCH_RESULT_";
pub const TEXT_SEARCH_HEADING_PREFIX: &str = "TEXT_SEARCH_HEADING_";
pub const FOLDER_MATCH_PREFIX: &str = "FOLDER_MATCH_";
pub const FILE_MATCH_PREFIX: &str = "FILE_MATCH_";
pub const MATCH_PREFIX: &str = "MATCH_";

/// `mergeSearchResultEvents`.
pub fn merge_search_result_events(events: Vec<IChangeEvent>) -> IChangeEvent {
    let mut ret_event = IChangeEvent { elements: Vec::new(), added: Some(false), removed: Some(false), clearing_all: None };
    for e in events {
        if e.added == Some(true) {
            ret_event.added = Some(true);
        }

        if e.removed == Some(true) {
            ret_event.removed = Some(true);
        }

        ret_event.elements.extend(e.elements);
    }

    ret_event
}

/// O `{ asyncResults, syncResults }` de `ISearchModel.search`.
pub struct SearchModelSearchResults {
    pub async_results: Promise<ISearchComplete>,
    pub sync_results: Vec<IFileMatch>,
}

/// `ISearchModel`.
pub trait ISearchModel: IDisposable {
    fn on_replace_term_changed(&self) -> Event<()>;
    fn on_search_result_changed(&self) -> Event<IChangeEvent>;
    fn location(&self) -> SearchModelLocation;
    fn set_location(&self, location: SearchModelLocation);
    fn id(&self) -> String;

    fn get_ai_text_result_provider_name(&self) -> Promise<String>;
    fn is_replace_active(&self) -> bool;
    fn set_replace_active(&self, replace_active: bool);
    fn replace_pattern(&self) -> Option<Rc<ReplacePattern>>;
    fn replace_string(&self) -> String;
    fn set_replace_string(&self, replace_string: &str);
    fn preserve_case(&self) -> bool;
    fn set_preserve_case(&self, value: bool);
    fn search_result(&self) -> Rc<dyn ISearchResult>;
    fn ai_search(&self, on_result_reported: Rc<dyn Fn(Option<&ISearchProgressItem>)>) -> Promise<ISearchComplete>;
    fn has_ai_results(&self) -> bool;
    fn has_plain_results(&self) -> bool;
    fn search(&self, query: ITextQuery, on_progress: Option<SearchProgressCallback>, caller_token: Option<CancellationToken>) -> SearchModelSearchResults;
    fn cancel_search(&self, cancelled_for_new_search: bool) -> bool;
    fn cancel_ai_search(&self, cancelled_for_new_search: bool) -> bool;
    fn clear_ai_search_results(&self);
}

/// `ISearchResult`.
pub trait ISearchResult: IDisposable {
    fn on_change(&self) -> Event<IChangeEvent>;
    fn search_model(&self) -> Rc<dyn ISearchModel>;
    fn plain_text_search_result(&self) -> Rc<dyn IPlainTextSearchHeading>;
    fn ai_text_search_result(&self) -> Rc<dyn ITextSearchHeading>;
    fn children(&self) -> Vec<Rc<dyn ITextSearchHeading>>;
    fn has_children(&self) -> bool;
    fn is_dirty(&self) -> bool;
    fn query(&self) -> Option<ITextQuery>;
    fn set_query(&self, query: Option<ITextQuery>);

    fn batch_replace(&self, elements_to_replace: Vec<RenderableMatch>) -> Promise<()>;
    fn batch_remove(&self, elements_to_remove: &[RenderableMatch]);
    fn folder_matches(&self, ai: bool) -> Vec<Rc<dyn ISearchTreeFolderMatch>>;
    fn add(&self, all_raw: &[IFileMatch], search_instance_id: &str, ai: bool, silent: bool);
    fn clear(&self);
    fn remove(&self, matches: &[FileOrFolderMatch], ai: bool);
    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()>;
    /// `matches(ai?)`: `None` junta os resultados de texto simples e de IA.
    fn matches(&self, ai: Option<bool>) -> Vec<Rc<dyn ISearchTreeFileMatch>>;
    fn is_empty(&self) -> bool;
    fn file_count(&self, ignore_semantic_search_results: bool) -> usize;
    fn count(&self, ignore_semantic_search_results: bool) -> usize;
    fn id(&self) -> String;
    fn set_cached_search_complete(&self, cached_search_complete: Option<ISearchComplete>, ai: bool);
    fn get_cached_search_complete(&self, ai: bool) -> Option<ISearchComplete>;
    fn toggle_highlights(&self, value: bool, ai: bool);
    fn get_range_highlight_decorations(&self, ai: bool) -> Rc<RangeHighlightDecorations>;
    fn replace_all(&self, progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()>;
    /// `setAIQueryUsingTextQuery(query?)`: sem consulta, usa a de texto do próprio resultado.
    fn set_ai_query_using_text_query(&self, query: Option<Option<ITextQuery>>);
}

/// O `{ byFolder, other }` de `groupFilesByFolder`.
pub struct GroupedFiles<T> {
    pub by_folder: ResourceMap<Vec<T>>,
    pub other: Vec<T>,
}

/// `ITextSearchHeading`.
pub trait ITextSearchHeading: IDisposable {
    fn on_change(&self) -> Event<IChangeEvent>;
    /// `resource: URI | null` (sempre nulo).
    fn resource(&self) -> Option<URI>;
    fn hidden(&self) -> bool;
    fn set_hidden(&self, hidden: bool);
    fn cached_search_complete(&self) -> Option<ISearchComplete>;
    fn set_cached_search_complete(&self, cached_search_complete: Option<ISearchComplete>);
    fn hide(&self);
    fn is_ai_contributed(&self) -> bool;
    fn id(&self) -> String;
    fn parent(&self) -> Rc<dyn ISearchResult>;
    fn has_children(&self) -> bool;
    fn name(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn get_folder_match(&self, resource: &URI) -> Option<Rc<dyn ISearchTreeFolderMatch>>;
    fn add(&self, all_raw: &[IFileMatch], search_instance_id: &str, silent: bool);
    fn remove(&self, matches: &[FileOrFolderMatch], ai: bool);
    fn group_files_by_folder(&self, file_matches: &[Rc<dyn ISearchTreeFileMatch>]) -> GroupedFiles<Rc<dyn ISearchTreeFileMatch>>;
    fn is_empty(&self) -> bool;
    fn find_folder_substr(&self, resource: &URI) -> Option<Rc<dyn ISearchTreeFolderMatch>>;
    fn query(&self) -> Option<ITextSearchQuery>;
    fn set_query(&self, query: Option<ITextSearchQuery>);
    fn folder_matches(&self) -> Vec<Rc<dyn ISearchTreeFolderMatch>>;
    fn matches(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>>;
    fn show_highlights(&self) -> bool;
    fn toggle_highlights(&self, value: bool);
    fn range_highlight_decorations(&self) -> Rc<RangeHighlightDecorations>;
    fn file_count(&self) -> usize;
    fn count(&self) -> usize;
    fn clear(&self, clear_all: bool);
}

/// `IPlainTextSearchHeading`.
pub trait IPlainTextSearchHeading: ITextSearchHeading {
    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()>;
    fn replace_all(&self, progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()>;
}

/// `ISearchTreeFolderMatch` (e, pelos predicados, `ISearchTreeFolderMatchWithResource`,
/// `ISearchTreeFolderMatchWorkspaceRoot` e `ISearchTreeFolderMatchNoRoot`).
pub trait ISearchTreeFolderMatch: IDisposable {
    fn on_change(&self) -> Event<IChangeEvent>;
    fn on_dispose(&self) -> Event<()>;
    fn id(&self) -> String;
    fn resource(&self) -> Option<URI>;
    fn index(&self) -> i32;
    fn name(&self) -> String;
    fn count(&self) -> usize;
    fn has_children(&self) -> bool;
    fn parent(&self) -> FolderMatchParent;
    fn matches(&self) -> Vec<FileOrFolderMatch>;
    fn all_downstream_file_matches(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>>;
    fn remove(&self, matches: &[FileOrFolderMatch]);
    fn add_file_match(&self, raw: &[IFileMatch], silent: bool, search_instance_id: &str);
    fn is_empty(&self) -> bool;
    fn clear(&self, clearing_all: bool);
    fn show_highlights(&self) -> bool;
    fn search_model(&self) -> Rc<dyn ISearchModel>;
    fn query(&self) -> Option<ITextSearchQuery>;
    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()>;
    fn set_replacing_all(&self, replacing_all: bool);
    fn bind_model(&self, model: &Rc<TextModel>);
    fn get_downstream_file_match(&self, uri: &URI) -> Option<Rc<dyn ISearchTreeFileMatch>>;
    fn replace_all(&self) -> Promise<()>;
    fn recursive_file_count(&self) -> usize;
    fn do_remove_file(&self, file_matches: &[Rc<dyn ISearchTreeFileMatch>], dispose: bool, trigger: bool, keep_readonly: bool);
    fn unbind_notebook_editor_widget(&self, editor: &Rc<dyn NotebookEditorWidget>, resource: &URI);
    fn bind_notebook_editor_widget(&self, editor: &Rc<dyn NotebookEditorWidget>, resource: &URI) -> Promise<()>;
    fn has_only_read_only_matches(&self) -> bool;
    fn file_matches_iterator(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>>;
    fn folder_matches_iterator(&self) -> Vec<Rc<dyn ISearchTreeFolderMatch>>;
    fn recursive_match_count(&self) -> usize;
    fn is_ai_contributed(&self) -> bool;
    /// `closestRoot` (`FolderMatchImpl`).
    fn closest_root(&self) -> Option<Rc<dyn ISearchTreeFolderMatch>>;
    /// Se o objeto tem o `createAndConfigureFileMatch` (`ISearchTreeFolderMatchWorkspaceRoot` e `...NoRoot`).
    fn has_create_and_configure_file_match(&self) -> bool;
    /// Quantas vezes `dispose` foi chamado (ver as divergências do módulo).
    fn dispose_count_for_testing(&self) -> usize;
}

/// `ISearchTreeFileMatch`.
pub trait ISearchTreeFileMatch: IDisposable {
    fn id(&self) -> String;
    fn resource(&self) -> URI;
    fn on_change(&self) -> Event<FileMatchChangeEvent>;
    fn has_children(&self) -> bool;
    fn on_dispose(&self) -> Event<()>;
    fn name(&self) -> String;
    fn count(&self) -> usize;
    fn has_only_read_only_matches(&self) -> bool;
    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>>;
    fn update_highlights(&self);
    fn get_selected_match(&self) -> Option<Rc<dyn ISearchTreeMatch>>;
    fn parent(&self) -> Rc<dyn ISearchTreeFolderMatch>;
    fn bind_model(&self, model: &Rc<TextModel>);
    fn has_readonly_matches(&self) -> bool;
    fn add_context(&self, results: Option<&[ITextSearchResult]>);
    fn add(&self, match_: Rc<dyn ISearchTreeMatch>, trigger: bool);
    fn replace(&self, to_replace: &Rc<dyn ISearchTreeMatch>) -> Promise<()>;
    fn remove(&self, matches: &[Rc<dyn ISearchTreeMatch>]);
    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>);
    fn file_stat(&self) -> Option<FileStatWithMetadata>;
    fn set_file_stat(&self, stat: Option<FileStatWithMetadata>);
    fn resolve_file_stat(&self, file_service: &Rc<dyn IFileService>) -> Promise<()>;
    fn text_matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>>;
    fn context(&self) -> OrderedMap<i64, JsString>;
    fn closest_root(&self) -> Option<Rc<dyn ISearchTreeFolderMatch>>;
    fn is_match_selected(&self, match_: &Rc<dyn ISearchTreeMatch>) -> bool;
    /// `isNotebookFileMatch(obj)`: o arquivo como `INotebookFileInstanceMatch`, se for um.
    fn as_notebook_file_match(&self) -> Option<Rc<dyn INotebookFileInstanceMatch>> {
        None
    }
}

/// O `{ before, fullBefore, inside, after }` de `ISearchTreeMatch.preview()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchPreview {
    pub before: JsString,
    pub full_before: JsString,
    pub inside: JsString,
    pub after: JsString,
}

/// `ISearchTreeMatch`.
pub trait ISearchTreeMatch {
    fn id(&self) -> String;
    fn parent(&self) -> Rc<dyn ISearchTreeFileMatch>;
    fn text(&self) -> JsString;
    fn range(&self) -> Range;
    fn preview(&self) -> MatchPreview;
    /// `replaceString`: lança se o modelo não tem `replacePattern`.
    fn replace_string(&self) -> JsString;
    fn full_match_text(&self, include_surrounding: bool) -> JsString;
    fn range_in_preview(&self) -> ISearchRange;
    fn full_preview_lines(&self) -> Vec<JsString>;
    fn get_match_string(&self) -> JsString;
    fn is_readonly(&self) -> bool;
    /// `isIMatchInNotebook(obj)`: o casamento como `IMatchInNotebook`, se for um.
    fn as_match_in_notebook(&self) -> Option<Rc<dyn IMatchInNotebook>> {
        None
    }
}

/// `isSearchModel`.
pub fn is_search_model(obj: &dyn ISearchModel) -> bool {
    obj.id().starts_with(SEARCH_MODEL_PREFIX)
}

/// `isSearchResult`.
pub fn is_search_result(obj: &dyn ISearchResult) -> bool {
    obj.id().starts_with(SEARCH_RESULT_PREFIX)
}

/// `isTextSearchHeading`.
pub fn is_text_search_heading(obj: &RenderableMatch) -> bool {
    matches!(obj, RenderableMatch::Heading(heading) if heading.id().starts_with(TEXT_SEARCH_HEADING_PREFIX))
}

/// `isPlainTextSearchHeading`.
pub fn is_plain_text_search_heading(obj: &RenderableMatch) -> bool {
    matches!(obj, RenderableMatch::Heading(heading) if heading.id() == format!("{TEXT_SEARCH_HEADING_PREFIX}{PLAIN_TEXT_SEARCH__RESULT_ID}"))
}

/// `isSearchTreeFolderMatch`.
pub fn is_search_tree_folder_match(obj: &dyn ISearchTreeFolderMatch) -> bool {
    obj.id().starts_with(FOLDER_MATCH_PREFIX)
}

/// `isSearchTreeFolderMatchWithResource`.
pub fn is_search_tree_folder_match_with_resource(obj: &dyn ISearchTreeFolderMatch) -> bool {
    is_search_tree_folder_match(obj) && obj.resource().is_some()
}

/// `isSearchTreeFolderMatchWorkspaceRoot`.
pub fn is_search_tree_folder_match_workspace_root(obj: &dyn ISearchTreeFolderMatch) -> bool {
    is_search_tree_folder_match_with_resource(obj) && obj.has_create_and_configure_file_match()
}

/// `isSearchTreeFolderMatchNoRoot`.
pub fn is_search_tree_folder_match_no_root(obj: &dyn ISearchTreeFolderMatch) -> bool {
    is_search_tree_folder_match(obj) && obj.has_create_and_configure_file_match()
}

/// `isSearchTreeFileMatch`.
pub fn is_search_tree_file_match(obj: &dyn ISearchTreeFileMatch) -> bool {
    obj.id().starts_with(FILE_MATCH_PREFIX)
}

/// `isSearchTreeMatch`.
pub fn is_search_tree_match(obj: &dyn ISearchTreeMatch) -> bool {
    obj.id().starts_with(MATCH_PREFIX)
}

/// `isSearchHeader`.
pub fn is_search_header(obj: &RenderableMatch) -> bool {
    is_text_search_heading(obj)
}

/// `getFileMatches`.
pub fn get_file_matches(matches: &[FileOrFolderMatch]) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
    let mut folder_matches: Vec<Rc<dyn ISearchTreeFolderMatch>> = Vec::new();
    let mut file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
    for e in matches {
        match e {
            FileOrFolderMatch::File(file_match) => file_matches.push(file_match.clone()),
            FileOrFolderMatch::Folder(folder_match) => folder_matches.push(folder_match.clone()),
        }
    }

    file_matches.extend(folder_matches.iter().flat_map(|e| e.all_downstream_file_matches()));
    file_matches
}

/// `TernarySearchTree.forUris<V>(key => uriIdentityService.extUri.ignorePathCasing(key))`, que `FolderMatchImpl` e
/// `TextSearchHeadingImpl` criam do mesmo jeito.
pub fn create_folder_matches_map<V>(uri_identity_service: &Rc<dyn IUriIdentityService>) -> TernarySearchTree<URI, V> {
    let service = uri_identity_service.clone();
    let ignore_path_casing: UriPredicate = Rc::new(move |key: &URI| service.ext_uri().ignore_path_casing(key));
    let ignore_query_and_fragment: UriPredicate = Rc::new(|_: &URI| false);
    TernarySearchTree::for_uris(ignore_path_casing, ignore_query_and_fragment)
}
