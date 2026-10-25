//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts`, fatia 2a: a camada de dados da árvore do
//! explorador, sem DOM. `ExplorerDataSource`, `FilesFilter`, `PhantomExplorerItem` (por identidade, em
//! [`PhantomItems`]), `ExplorerFindProvider` e o `getMatchingDirectoriesFromFiles`.
//!
//! Divergências:
//! - **`PhantomExplorerItem`.** A classe não acrescenta nada ao `ExplorerItem`; só o `instanceof` importa. O provedor
//!   guarda a identidade (`Rc::as_ptr`) dos itens que criou em [`PhantomItems`], sem tocar no modelo.
//! - **Árvore.** `treeProvider()` devolve uma `WorkbenchCompressibleAsyncDataTree`; o provedor só usa seis operações dela
//!   (`getInput`, `getViewState`, `setInput`, `hasNode`, `rerender`, `isCollapsed`), reunidas em [`ExplorerFindTree`], que
//!   o `ExplorerView` implementa sobre a árvore real. O `IAsyncDataTreeViewState` é opaco para o provedor (`V`).
//! - **Contexto.** `ExplorerFindProviderActive.bindTo(contextKeyService)` vira o [`ExplorerFindActiveKey`], que o
//!   `ExplorerView` liga à chave de contexto.
//! - **URIs de fantasmas.** `currentResource.with({ path: `${path}/${stat}` })` é `ExtUri::join_path`.
//! - **Texto.** O `split('')` do padrão fuzzy opera em unidades UTF-16 (ver [`super::explorer_viewer`]).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::glob::{self, Expression, GlobOptions, ParsedGlob, Sibling};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::map::ResourceSet;
use je_base::nls::localize;
use je_base::objects;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileChangeType, FileSystemProviderCapabilities};
use je_platform::files::i_file_service::IFileService;
use je_platform::notification::notification::INotificationService;
use je_platform::progress::progress::{IProgressService, ProgressLocation, ProgressOptions};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};
use je_widgets::tree::tree_types::{IAsyncDataSource, ITreeFilter, InputOrElement, TreeFilterResult, TreeVisibility};

use crate::contrib::files::browser_files::IExplorerService;
use crate::contrib::files::explorer_model::ExplorerItem;
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::layout::layout_service::IWorkbenchLayoutService;
use crate::services::search::ignore_file::IgnoreFile;
use crate::services::search::search::{get_excludes, IFileQuery, IFolderQuery, ISearchService};

use super::explorer_viewer::{continous_matching_glob_pattern, find_first_parent, fuzzy_matching_glob_pattern, ExplorerFindHighlightTree, IExplorerFindHighlightTree};

/// `ExplorerItem | ExplorerItem[]`: a entrada da árvore é a lista de raízes.
pub type ExplorerInput = Vec<Rc<ExplorerItem>>;

fn not_a_child_of_the_root() -> Error {
    Error::new("Resource is not a child of the root")
}

/// `explorerRootErrorEmitter`.
pub fn explorer_root_error_emitter() -> Rc<Emitter<URI>> {
    thread_local! {
        static EMITTER: Rc<Emitter<URI>> = Emitter::new();
    }
    EMITTER.with(|emitter| emitter.clone())
}

// ---------------------------------------------------------------------------------------------------------------------
// ExplorerDataSource
// ---------------------------------------------------------------------------------------------------------------------

/// `ExplorerDataSource`. `V` é o estado de vista da árvore que o `ExplorerFindProvider` guarda (o `IAsyncDataTreeViewState`).
pub struct ExplorerDataSource<V: Clone + 'static = ()> {
    file_filter: Rc<FilesFilter>,
    find_provider: Rc<ExplorerFindProvider<V>>,
    progress_service: Rc<dyn IProgressService>,
    config_service: Rc<dyn IConfigurationService>,
    notification_service: Rc<dyn INotificationService>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    file_service: Rc<dyn IFileService>,
    explorer_service: Rc<dyn IExplorerService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
}

impl<V: Clone + 'static> ExplorerDataSource<V> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        file_filter: Rc<FilesFilter>,
        find_provider: Rc<ExplorerFindProvider<V>>,
        progress_service: Rc<dyn IProgressService>,
        config_service: Rc<dyn IConfigurationService>,
        notification_service: Rc<dyn INotificationService>,
        layout_service: Rc<dyn IWorkbenchLayoutService>,
        file_service: Rc<dyn IFileService>,
        explorer_service: Rc<dyn IExplorerService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        files_config_service: Rc<dyn IFilesConfigurationService>,
    ) -> Self {
        Self {
            file_filter,
            find_provider,
            progress_service,
            config_service,
            notification_service,
            layout_service,
            file_service,
            explorer_service,
            context_service,
            files_config_service,
        }
    }

    /// `getParent`: só vale para pais em cache.
    pub fn get_parent(&self, element: &Rc<ExplorerItem>) -> Rc<ExplorerItem> {
        match element.parent() {
            Some(parent) => parent,
            None => je_base::errors::throw(Error::new("getParent only supported for cached parents")),
        }
    }
}

impl<V: Clone + 'static> IAsyncDataSource<ExplorerInput, Rc<ExplorerItem>> for ExplorerDataSource<V> {
    fn has_children(&self, element: &InputOrElement<ExplorerInput, Rc<ExplorerItem>>) -> bool {
        match element {
            // don't render nest parents as containing children when all the children are filtered out
            InputOrElement::Input(_) => true,
            InputOrElement::Element(element) => {
                element.has_children(|stat| matches!(self.file_filter.filter_stat(stat, TreeVisibility::Visible), TreeFilterResult::Bool(true)))
            }
        }
    }

    fn get_children(&self, element: &InputOrElement<ExplorerInput, Rc<ExplorerItem>>) -> MaybePromise<Vec<Rc<ExplorerItem>>> {
        let element = match element {
            InputOrElement::Input(roots) => return MaybePromise::Value(roots.clone()),
            InputOrElement::Element(element) => element.clone(),
        };

        if self.find_provider.is_showing_filter_results() {
            return MaybePromise::Value(element.children());
        }

        let has_error = element.error().is_some();
        let sort_order = self.explorer_service.sort_order_configuration().sort_order;
        let children = match element.fetch_children(sort_order) {
            // fast path when children are known sync (i.e. nested children)
            MaybePromise::Value(children) => return MaybePromise::Value(children),
            MaybePromise::Promise(children) => children,
        };

        let context_service = self.context_service.clone();
        let notification_service = self.notification_service.clone();
        let file_service = self.file_service.clone();
        let config_service = self.config_service.clone();
        let files_config_service = self.files_config_service.clone();
        let promise: Promise<Vec<Rc<ExplorerItem>>> = async_fn(async move {
            match children.await {
                Ok(children) => {
                    // Clear previous error decoration on root folder
                    if element.is_root() && element.error().is_none() && has_error && context_service.get_workbench_state() != WorkbenchState::Folder {
                        explorer_root_error_emitter().fire(element.resource());
                    }
                    Ok(children)
                }
                Err(error) => {
                    if element.is_root() {
                        if context_service.get_workbench_state() == WorkbenchState::Folder {
                            // Single folder create a dummy explorer item to show error
                            let placeholder = ExplorerItem::new(
                                element.resource(),
                                file_service,
                                config_service,
                                files_config_service,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                false,
                            );
                            placeholder.set_error(Some(error));
                            return Ok(vec![placeholder]);
                        }

                        explorer_root_error_emitter().fire(element.resource());
                    } else {
                        // Do not show error for roots since we already use an explorer decoration to notify user
                        notification_service.error(vec![error.into()]);
                    }

                    Ok(Vec::new()) // we could not resolve any children because of an error
                }
            }
        });

        let mut options = ProgressOptions::new(ProgressLocation::Explorer);
        // reduce progress visibility when still restoring
        options.delay = Some(if self.layout_service.is_restored() { 800.0 } else { 1500.0 });
        let tracked = promise.clone();
        let _ = self.progress_service.with_progress(options, Box::new(move |_progress| tracked.then(|_| ())), None);

        MaybePromise::Promise(promise)
    }

    fn get_parent(&self, element: &Rc<ExplorerItem>) -> Option<InputOrElement<ExplorerInput, Rc<ExplorerItem>>> {
        Some(InputOrElement::Element(ExplorerDataSource::get_parent(self, element)))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FilesFilter
// ---------------------------------------------------------------------------------------------------------------------

/// `CachedParsedExpression`.
struct CachedParsedExpression {
    original: Dynamic,
    parsed: ParsedGlob,
}

/// A identidade de um editor visível (o `Set<EditorInput>` do original compara por referência).
fn editor_identity(editor: &crate::common::editor::EditorInputRef) -> usize {
    Rc::as_ptr(editor).cast::<()>() as usize
}

/// Respects files.exclude setting in filtering out content from the explorer.
/// Makes sure that visible editors are always shown in the explorer even if they are filtered out by settings.
///
/// `FilesFilter`.
pub struct FilesFilter {
    this: Weak<FilesFilter>,
    context_service: Rc<dyn IWorkspaceContextService>,
    configuration_service: Rc<dyn IConfigurationService>,
    explorer_service: Rc<dyn IExplorerService>,
    editor_service: Rc<dyn IEditorService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    file_service: Rc<dyn IFileService>,
    hidden_expression_per_root: RefCell<HashMap<String, CachedParsedExpression>>,
    editors_affecting_filter: RefCell<HashSet<usize>>,
    on_did_change: Rc<Emitter<()>>,
    to_dispose: RefCell<Vec<DisposableRef>>,
    // List of ignoreFile resources. Used to detect changes to the ignoreFiles.
    ignore_file_resources_per_root: RefCell<HashMap<String, ResourceSet>>,
    // Ignore tree per root. Similar to `hiddenExpressionPerRoot`
    // Note: URI in the ternary search tree is the URI of the folder containing the ignore file
    // It is not the ignore file itself. This is because of the way the IgnoreFile works and nested paths
    ignore_trees_per_root: RefCell<HashMap<String, TernarySearchTree<URI, IgnoreFile>>>,
}

impl FilesFilter {
    pub fn new(
        context_service: Rc<dyn IWorkspaceContextService>,
        configuration_service: Rc<dyn IConfigurationService>,
        explorer_service: Rc<dyn IExplorerService>,
        editor_service: Rc<dyn IEditorService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        file_service: Rc<dyn IFileService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            context_service,
            configuration_service,
            explorer_service,
            editor_service,
            uri_identity_service,
            file_service,
            hidden_expression_per_root: RefCell::new(HashMap::new()),
            editors_affecting_filter: RefCell::new(HashSet::new()),
            on_did_change: Emitter::new(),
            to_dispose: RefCell::new(Vec::new()),
            ignore_file_resources_per_root: RefCell::new(HashMap::new()),
            ignore_trees_per_root: RefCell::new(HashMap::new()),
        });

        let mut listeners: Vec<DisposableRef> = Vec::new();

        let weak = this.this.clone();
        listeners.push(this.context_service.on_did_change_workspace_folders().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_configuration();
            }
        }));

        let weak = this.this.clone();
        listeners.push(this.configuration_service.on_did_change_configuration().listen(move |event| {
            if let Some(this) = weak.upgrade()
                && (event.affects_configuration("files.exclude", None) || event.affects_configuration("explorer.excludeGitIgnore", None))
            {
                this.update_configuration();
            }
        }));

        let weak = this.this.clone();
        listeners.push(this.file_service.on_did_files_change().listen(move |event| {
            let Some(this) = weak.upgrade() else {
                return;
            };

            // Check to see if the update contains any of the ignoreFileResources
            let candidates: Vec<(String, Vec<URI>)> = this
                .ignore_file_resources_per_root
                .borrow()
                .iter()
                .map(|(root, resources)| (root.clone(), resources.iter().cloned().collect()))
                .collect();
            for (root, resources) in candidates {
                for ignore_resource in resources {
                    if event.contains(&ignore_resource, &[FileChangeType::Updated]) {
                        let _ = this.process_ignore_file(root.clone(), ignore_resource.clone(), true);
                    }

                    if event.contains(&ignore_resource, &[FileChangeType::Deleted]) {
                        let dirname = this.uri_identity_service.ext_uri().dirname(&ignore_resource);
                        if let Some(tree) = this.ignore_trees_per_root.borrow_mut().get_mut(&root) {
                            tree.delete(&dirname);
                        }
                        if let Some(set) = this.ignore_file_resources_per_root.borrow_mut().get_mut(&root) {
                            set.delete(&ignore_resource);
                        }
                        this.on_did_change.fire(());
                    }
                }
            }
        }));

        let weak = this.this.clone();
        listeners.push(this.editor_service.on_did_visible_editors_change().listen(move |_| {
            let Some(this) = weak.upgrade() else {
                return;
            };

            let editors = this.editor_service.visible_editors();
            let mut should_fire = false;

            for editor in &editors {
                let Some(resource) = editor.resource() else {
                    continue;
                };

                if this.explorer_service.find_closest(&resource).is_some_and(|stat| stat.is_excluded()) {
                    // A filtered resource suddenly became visible since user opened an editor
                    should_fire = true;
                    break;
                }
            }

            if !should_fire {
                let visible: HashSet<usize> = editors.iter().map(editor_identity).collect();
                // Editor that was affecting filtering is no longer visible
                should_fire = this.editors_affecting_filter.borrow().iter().any(|editor| !visible.contains(editor));
            }

            if should_fire {
                this.editors_affecting_filter.borrow_mut().clear();
                this.on_did_change.fire(());
            }
        }));

        *this.to_dispose.borrow_mut() = listeners;
        this.update_configuration();
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn files_exclude(&self, resource: URI) -> (Dynamic, bool) {
        let configuration = self.configuration_service.get_value(None, Some(&ConfigurationOverrides::with_resource(resource)));
        let exclude = configuration.optional_get_str("files").optional_get_str("exclude");
        let exclude = if exclude.is_truthy() { exclude } else { Dynamic::object(Vec::<(&str, Dynamic)>::new()) };
        let parse_ignore_file = configuration.get_str("explorer").get_str("excludeGitIgnore").is_truthy();
        (exclude, parse_ignore_file)
    }

    fn update_configuration(&self) {
        let mut should_fire = false;
        let mut updated_git_ignore_setting = false;
        for folder in self.context_service.get_workspace().folders() {
            let key = folder.uri.to_string(false).to_string_lossy();
            let (excludes_config, parse_ignore_file) = self.files_exclude(folder.uri.clone());

            // If we should be parsing ignoreFiles for this workspace and don't have an ignore tree initialize one
            let has_tree = self.ignore_trees_per_root.borrow().contains_key(&key);
            if parse_ignore_file && !has_tree {
                updated_git_ignore_setting = true;
                self.ignore_file_resources_per_root.borrow_mut().insert(key.clone(), ResourceSet::new(None));
                let uri_identity_service = self.uri_identity_service.clone();
                let tree = TernarySearchTree::for_uris(
                    Rc::new(move |uri: &URI| uri_identity_service.ext_uri().ignore_path_casing(uri)),
                    Rc::new(|_: &URI| false),
                );
                self.ignore_trees_per_root.borrow_mut().insert(key.clone(), tree);
            }

            // If we shouldn't be parsing ignore files but have an ignore tree, clear the ignore tree
            if !parse_ignore_file && has_tree {
                updated_git_ignore_setting = true;
                self.ignore_file_resources_per_root.borrow_mut().remove(&key);
                self.ignore_trees_per_root.borrow_mut().remove(&key);
            }

            if !should_fire {
                let cached = self.hidden_expression_per_root.borrow();
                should_fire = cached.get(&key).is_none_or(|cached| !objects::equals(&cached.original, &excludes_config));
            }

            // do not keep the config, as it gets mutated under our hoods
            let excludes_config_copy = objects::deep_clone(&excludes_config);
            let parsed = glob::parse(glob::Glob::Expression(&Expression::from_dynamic(&excludes_config_copy)), &GlobOptions::default());
            self.hidden_expression_per_root.borrow_mut().insert(key, CachedParsedExpression { original: excludes_config_copy, parsed });
        }

        if should_fire || updated_git_ignore_setting {
            self.editors_affecting_filter.borrow_mut().clear();
            self.on_did_change.fire(());
        }
    }

    /// Given a .gitignore file resource, processes the resource and adds it to the ignore tree which hides explorer items.
    ///
    /// `root` é a raiz do workspace como texto (chave da árvore e da lista de recursos); `update` diz se estamos
    /// atualizando um arquivo de ignore existente, caso em que o conteúdo é trocado mantendo as referências.
    fn process_ignore_file(&self, root: String, ignore_file_resource: URI, update: bool) -> Promise<()> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(());
        };

        async_fn(async move {
            // Get the name of the directory which the ignore file is in
            let dir_uri = this.uri_identity_service.ext_uri().dirname(&ignore_file_resource);
            if !this.ignore_trees_per_root.borrow().contains_key(&root) {
                return Ok(());
            }

            // Don't process a directory if we already have it in the tree
            if !update && this.ignore_trees_per_root.borrow().get(&root).is_some_and(|tree| tree.has(&dir_uri)) {
                return Ok(());
            }

            let content = this.file_service.read_file(&ignore_file_resource, None, None).await?;
            let text = content.value.to_string();

            {
                let mut trees = this.ignore_trees_per_root.borrow_mut();
                let Some(ignore_tree) = trees.get_mut(&root) else {
                    return Ok(());
                };

                // If it's just an update we update the contents keeping all references the same
                if update {
                    if let Some(ignore_file) = ignore_tree.get(&dir_uri) {
                        ignore_file.update_contents(&text);
                    }
                } else {
                    // Otherwise we create a new ignore file and add it to the tree
                    let ignore_case = !this.file_service.has_capability(&ignore_file_resource, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
                    let location = dir_uri.path().to_string_lossy();
                    let ignore_file = {
                        let ignore_parent = ignore_tree.find_substr(&dir_uri);
                        IgnoreFile::new(&text, &location, ignore_parent, ignore_case).map_err(|error| je_base::errors::Error::new(error.0))?
                    };
                    ignore_tree.set(dir_uri.clone(), ignore_file);

                    // If we haven't seen this resource before then we need to add it to the list of resources we're tracking
                    if let Some(set) = this.ignore_file_resources_per_root.borrow_mut().get_mut(&root)
                        && !set.has(&ignore_file_resource)
                    {
                        set.add(ignore_file_resource.clone());
                    }
                }
            }

            // Notify the explorer of the change so we may ignore these files
            this.on_did_change.fire(());
            Ok(())
        })
    }

    /// `filter`, sem o `FuzzyScore` (o filtro só responde booleano).
    pub fn filter_stat(&self, stat: &Rc<ExplorerItem>, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        // Add newly visited .gitignore files to the ignore tree
        let root_key = stat.root().resource().to_string(false).to_string_lossy();
        if stat.name() == ".gitignore" && self.ignore_trees_per_root.borrow().contains_key(&root_key) {
            let _ = self.process_ignore_file(root_key, stat.resource(), false);
            return TreeFilterResult::Bool(true);
        }

        TreeFilterResult::Bool(self.is_visible(stat, parent_visibility))
    }

    fn is_visible(&self, stat: &Rc<ExplorerItem>, parent_visibility: TreeVisibility) -> bool {
        stat.set_excluded(false);
        if parent_visibility == TreeVisibility::Hidden {
            stat.set_excluded(true);
            return false;
        }
        if self.explorer_service.get_editable_data(stat).is_some() {
            return true; // always visible
        }

        let ext_uri = self.uri_identity_service.ext_uri();

        // Hide those that match Hidden Patterns
        let root = stat.root();
        let root_resource = root.resource();
        let glob_match = {
            let cached = self.hidden_expression_per_root.borrow();
            cached.get(&root_resource.to_string(false).to_string_lossy()).is_some_and(|cached| match &cached.parsed {
                ParsedGlob::Expression(expression) => {
                    let relative = je_base::path::PathModule::posix(&je_base::path::ProcessEnvironment).relative(&root_resource.path().to_string_lossy(), &stat.resource().path().to_string_lossy());
                    let parent = stat.parent();
                    let has_sibling = move |name: &str| -> Sibling { Sibling::Ready(parent.as_ref().is_some_and(|parent| parent.get_child(name).is_some())) };
                    !expression.matches(&relative, Some(&stat.name()), Some(&has_sibling)).is_none()
                }
                ParsedGlob::Pattern(pattern) => pattern.matches(&stat.resource().path().to_string_lossy(), Some(&stat.name())),
            })
        };

        // Small optimization to only run isHiddenResource (traverse gitIgnore) if the globMatch from fileExclude returned nothing
        let is_hidden_resource = glob_match || self.is_ignored(&stat.resource(), &root_resource, stat.is_directory());
        if is_hidden_resource || stat.parent().is_some_and(|parent| parent.is_excluded()) {
            stat.set_excluded(true);
            let editors = self.editor_service.visible_editors();
            let editor = editors.iter().find(|editor| editor.resource().is_some_and(|resource| ext_uri.is_equal_or_parent(&resource, &stat.resource(), false, false)));
            if let Some(editor) = editor
                && self.explorer_service.find_closest_root(&stat.resource()).is_some_and(|closest| Rc::ptr_eq(&root, &closest))
            {
                self.editors_affecting_filter.borrow_mut().insert(editor_identity(editor));
                return true; // Show all opened files and their parents
            }

            return false; // hidden through pattern
        }

        true
    }

    /// `isIgnored`.
    pub fn is_ignored(&self, resource: &URI, root_resource: &URI, is_directory: bool) -> bool {
        let trees = self.ignore_trees_per_root.borrow();
        let ignore_file = trees.get(&root_resource.to_string(false).to_string_lossy()).and_then(|tree| tree.find_substr(resource));
        let is_included_in_traversal = ignore_file.and_then(|ignore_file| ignore_file.is_path_included_in_traversal(&resource.path().to_string_lossy(), is_directory).ok());

        // Doing !undefined returns true and we want it to be false when undefined because that means it's not included in the ignore file
        // When ignore_file is undefined, is_included_in_traversal is also undefined, so we handle that case explicitly
        is_included_in_traversal.is_some_and(|included| !included)
    }

    /// `dispose`.
    pub fn dispose(&self) {
        for listener in self.to_dispose.borrow_mut().drain(..) {
            listener.dispose();
        }
        self.on_did_change.dispose();
    }
}

impl ITreeFilter<Rc<ExplorerItem>, ()> for FilesFilter {
    fn filter(&self, element: &Rc<ExplorerItem>, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        self.filter_stat(element, parent_visibility)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ExplorerFindProvider
// ---------------------------------------------------------------------------------------------------------------------

/// O que o `ExplorerFindProvider` usa da `WorkbenchCompressibleAsyncDataTree` (ver o topo do módulo). `V` é o
/// `IAsyncDataTreeViewState`.
pub trait ExplorerFindTree<V: Clone + 'static> {
    /// `getInput`.
    fn get_input(&self) -> Option<ExplorerInput>;
    /// `getViewState`.
    fn get_view_state(&self) -> V;
    /// `setInput(input, viewState?)`.
    fn set_input(&self, input: ExplorerInput, view_state: Option<V>) -> Promise<()>;
    /// `hasNode`.
    fn has_node(&self, item: &Rc<ExplorerItem>) -> bool;
    /// `rerender`.
    fn rerender(&self, item: &Rc<ExplorerItem>);
    /// `isCollapsed`.
    fn is_collapsed(&self, item: &Rc<ExplorerItem>) -> bool;
}

/// A chave de contexto `ExplorerFindProviderActive`.
pub trait ExplorerFindActiveKey {
    fn set(&self, value: bool);
}

/// `TreeFindMatchType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindMatchType {
    Fuzzy,
    Contiguous,
}

/// `TreeFindMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindMode {
    Highlight,
    Filter,
}

/// `IAsyncFindToggles`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindToggles {
    pub match_type: FindMatchType,
    pub find_mode: FindMode,
}

/// `IAsyncFindResult<ExplorerItem>`.
#[derive(Clone)]
pub struct ExplorerFindResult {
    pub is_match: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>,
    pub match_count: usize,
    pub warning_message: Option<String>,
}

/// O resultado de `searchInWorkspace`.
#[derive(Clone)]
struct SearchResults {
    explorer_root: Rc<ExplorerItem>,
    files: Vec<URI>,
    directories: Vec<URI>,
    hit_max_results: bool,
}

/// A identidade dos `PhantomExplorerItem` criados (o `instanceof PhantomExplorerItem`).
#[derive(Default)]
pub struct PhantomItems(RefCell<HashSet<usize>>);

impl PhantomItems {
    fn key(item: &Rc<ExplorerItem>) -> usize {
        Rc::as_ptr(item) as usize
    }

    fn add(&self, item: &Rc<ExplorerItem>) {
        self.0.borrow_mut().insert(Self::key(item));
    }

    /// `item instanceof PhantomExplorerItem`.
    pub fn contains(&self, item: &Rc<ExplorerItem>) -> bool {
        self.0.borrow().contains(&Self::key(item))
    }
}

struct FilterSessionStartState<V> {
    view_state: V,
    input: ExplorerInput,
    roots_with_providers: Vec<Rc<ExplorerItem>>,
}

/// `ExplorerFindProvider`.
pub struct ExplorerFindProvider<V: Clone + 'static = ()> {
    this: Weak<ExplorerFindProvider<V>>,
    files_filter: Rc<FilesFilter>,
    tree_provider: Box<dyn Fn() -> Rc<dyn ExplorerFindTree<V>>>,
    search_service: Rc<dyn ISearchService>,
    file_service: Rc<dyn IFileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
    progress_service: Rc<dyn IProgressService>,
    explorer_service: Rc<dyn IExplorerService>,
    session_id: Cell<u64>,
    filter_session_start_state: RefCell<Option<FilterSessionStartState<V>>>,
    highlight_session_start_state: RefCell<Option<Vec<Rc<ExplorerItem>>>>,
    explorer_find_active_context_key: Rc<dyn ExplorerFindActiveKey>,
    phantom_parents: RefCell<Vec<Rc<ExplorerItem>>>,
    phantoms: PhantomItems,
    find_highlight_tree: ExplorerFindHighlightTree,
    ext_uri: Rc<ExtUri>,
}

impl<V: Clone + 'static> ExplorerFindProvider<V> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        files_filter: Rc<FilesFilter>,
        tree_provider: Box<dyn Fn() -> Rc<dyn ExplorerFindTree<V>>>,
        search_service: Rc<dyn ISearchService>,
        file_service: Rc<dyn IFileService>,
        configuration_service: Rc<dyn IConfigurationService>,
        files_config_service: Rc<dyn IFilesConfigurationService>,
        progress_service: Rc<dyn IProgressService>,
        explorer_service: Rc<dyn IExplorerService>,
        explorer_find_active_context_key: Rc<dyn ExplorerFindActiveKey>,
        ext_uri: Rc<ExtUri>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            files_filter,
            tree_provider,
            search_service,
            file_service,
            configuration_service,
            files_config_service,
            progress_service,
            explorer_service,
            session_id: Cell::new(0),
            filter_session_start_state: RefCell::new(None),
            highlight_session_start_state: RefCell::new(None),
            explorer_find_active_context_key,
            phantom_parents: RefCell::new(Vec::new()),
            phantoms: PhantomItems::default(),
            find_highlight_tree: ExplorerFindHighlightTree::new(ext_uri.clone()),
            ext_uri,
        })
    }

    fn ext_uri(&self) -> Rc<ExtUri> {
        self.ext_uri.clone()
    }

    /// `highlightTree`.
    pub fn highlight_tree(&self) -> &dyn IExplorerFindHighlightTree {
        &self.find_highlight_tree
    }

    /// `isShowingFilterResults`.
    pub fn is_showing_filter_results(&self) -> bool {
        self.filter_session_start_state.borrow().is_some()
    }

    /// `isVisible`.
    pub fn is_visible(&self, element: &Rc<ExplorerItem>) -> bool {
        let state = self.filter_session_start_state.borrow();
        let Some(state) = state.as_ref() else {
            return true;
        };

        if self.explorer_service.is_editable(Some(element)) {
            return true;
        }

        if state.roots_with_providers.iter().any(|root| Rc::ptr_eq(root, &element.root())) { element.is_marked_as_filtered() } else { true }
    }

    /// `startSession`.
    pub fn start_session(&self) {
        self.session_id.set(self.session_id.get() + 1);
    }

    /// `endSession`.
    pub fn end_session(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(());
        };

        async_fn(async move {
            // Restore view state
            if this.filter_session_start_state.borrow().is_some() {
                this.end_filter_session().await?;
            }

            if this.highlight_session_start_state.borrow().is_some() {
                this.end_highlight_session();
            }

            Ok(())
        })
    }

    /// `find`.
    pub fn find(&self, pattern: String, toggles: FindToggles, token: CancellationToken) -> Promise<Option<ExplorerFindResult>> {
        let promise = self.do_find(pattern, toggles, token);

        let mut options = ProgressOptions::new(ProgressLocation::Explorer);
        options.delay = Some(750.0);
        let tracked = promise.clone();
        let _ = self.progress_service.with_progress(options, Box::new(move |_progress| tracked.then(|_| ())), None);

        promise
    }

    /// `doFind`.
    pub fn do_find(&self, pattern: String, toggles: FindToggles, token: CancellationToken) -> Promise<Option<ExplorerFindResult>> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(None);
        };

        async_fn(async move {
            if toggles.find_mode == FindMode::Highlight {
                if this.filter_session_start_state.borrow().is_some() {
                    this.end_filter_session().await?;
                }

                if this.highlight_session_start_state.borrow().is_none() {
                    this.start_highlight_session();
                }

                return this.do_highlight_find(pattern, toggles.match_type, token).await;
            }

            if this.highlight_session_start_state.borrow().is_some() {
                this.end_highlight_session();
            }

            if this.filter_session_start_state.borrow().is_none() {
                this.start_filter_session();
            }

            this.do_filter_find(pattern, toggles.match_type, token).await
        })
    }

    // Filter

    fn start_filter_session(&self) {
        let tree = (self.tree_provider)();
        let Some(input) = tree.get_input() else {
            return;
        };

        let roots: Vec<Rc<ExplorerItem>> = self.explorer_service.roots().into_iter().filter(|root| self.search_supports_scheme(&root.resource().scheme().to_string_lossy())).collect();
        *self.filter_session_start_state.borrow_mut() = Some(FilterSessionStartState { view_state: tree.get_view_state(), input, roots_with_providers: roots });

        self.explorer_find_active_context_key.set(true);
    }

    /// `doFilterFind`.
    pub fn do_filter_find(&self, pattern: String, match_type: FindMatchType, token: CancellationToken) -> Promise<Option<ExplorerFindResult>> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(None);
        };

        async_fn(async move {
            let (roots, input) = {
                let state = this.filter_session_start_state.borrow();
                let Some(state) = state.as_ref() else {
                    return Err(Error::new("ExplorerFindProvider: no session state"));
                };
                (state.roots_with_providers.clone(), state.input.clone())
            };
            let search_results = this.get_search_results(&pattern, &roots, match_type, &token).await?;

            if token.is_cancellation_requested() {
                return Ok(None);
            }

            this.clear_phantom_elements();
            for SearchResults { explorer_root, files, directories, .. } in &search_results {
                this.add_workspace_filter_results(explorer_root, files, directories)?;
            }

            let tree = (this.tree_provider)();
            tree.set_input(input, None).await?;

            Ok(Some(this.find_result(&search_results, Rc::new(|item: &Rc<ExplorerItem>| item.is_marked_as_filtered()))))
        })
    }

    fn find_result(&self, search_results: &[SearchResults], is_match: Rc<dyn Fn(&Rc<ExplorerItem>) -> bool>) -> ExplorerFindResult {
        let hit_max_results = search_results.iter().any(|result| result.hit_max_results);
        ExplorerFindResult {
            is_match,
            match_count: search_results.iter().map(|result| result.files.len() + result.directories.len()).sum(),
            warning_message: hit_max_results.then(|| {
                localize("searchMaxResultsWarning", "The result set only contains a subset of all matches. Be more specific in your search to narrow down the results.", &[])
            }),
        }
    }

    fn add_workspace_filter_results(&self, root: &Rc<ExplorerItem>, files: &[URI], directories: &[URI]) -> Result<(), Error> {
        let results = files.iter().map(|file| (file, false)).chain(directories.iter().map(|directory| (directory, true)));

        for (resource, is_directory) in results {
            if let Some(element) = root.find(resource)
                && Rc::ptr_eq(&element.root(), root)
            {
                // File is already in the model
                element.mark_item_and_parents_as_filtered();
                continue;
            }

            // File is not in the model, create phantom items for the file and it's parents
            let phantom_elements = self.create_phantom_items(resource, root, is_directory)?;
            let (Some(first), Some(last)) = (phantom_elements.first(), phantom_elements.last()) else {
                return Err(Error::new("Phantom item was not created even though it is not in the model"));
            };

            // Store the first ancestor of the file which is already present in the model
            if let Some(first_phantom_parent) = first.parent()
                && !self.phantoms.contains(&first_phantom_parent)
            {
                let mut parents = self.phantom_parents.borrow_mut();
                if !parents.iter().any(|parent| Rc::ptr_eq(parent, &first_phantom_parent)) {
                    parents.push(first_phantom_parent);
                }
            }

            last.mark_item_and_parents_as_filtered();
        }

        Ok(())
    }

    fn create_phantom_items(&self, resource: &URI, root: &Rc<ExplorerItem>, resource_is_directory: bool) -> Result<Vec<Rc<ExplorerItem>>, Error> {
        let ext_uri = self.ext_uri();
        let relative_path_to_root = ext_uri.relative_path(&root.resource(), resource).filter(|path| !path.is_empty()).ok_or_else(not_a_child_of_the_root)?;

        let mut phantom_elements: Vec<Rc<ExplorerItem>> = Vec::new();

        let mut current_item = root.clone();
        let mut current_resource = root.resource();
        let path_segments: Vec<&str> = relative_path_to_root.split('/').collect();
        let last_segment = path_segments.last().copied();
        for stat in &path_segments {
            current_resource = ext_uri.join_path(&current_resource, &[stat]);

            let child = match current_item.get_child(stat) {
                Some(child) => child,
                None => {
                    let is_directory = if last_segment == Some(*stat) { resource_is_directory } else { true };
                    // PhantomExplorerItem
                    let child = ExplorerItem::new(
                        current_resource.clone(),
                        self.file_service.clone(),
                        self.configuration_service.clone(),
                        self.files_config_service.clone(),
                        Some(&current_item),
                        Some(is_directory),
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                    );
                    self.phantoms.add(&child);
                    current_item.add_child(&child);
                    phantom_elements.push(child.clone());
                    child
                }
            };

            current_item = child;
        }

        Ok(phantom_elements)
    }

    /// `endFilterSession`.
    pub fn end_filter_session(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(());
        };

        async_fn(async move {
            this.clear_phantom_elements();

            this.explorer_find_active_context_key.set(false);

            // Restore view state
            let Some(state) = this.filter_session_start_state.borrow().as_ref().map(|state| (state.input.clone(), state.view_state.clone())) else {
                return Err(Error::new("ExplorerFindProvider: no session state to restore"));
            };

            let tree = (this.tree_provider)();
            tree.set_input(state.0, Some(state.1)).await?;

            *this.filter_session_start_state.borrow_mut() = None;
            let _ = this.explorer_service.refresh();
            Ok(())
        })
    }

    fn clear_phantom_elements(&self) {
        let parents: Vec<Rc<ExplorerItem>> = self.phantom_parents.borrow_mut().drain(..).collect();
        for phantom_parent in parents {
            // Clear phantom nodes from model
            phantom_parent.forget_children();
        }
        for root in self.explorer_service.roots() {
            root.unmark_item_and_children();
        }
    }

    // Highlight

    fn start_highlight_session(&self) {
        let roots: Vec<Rc<ExplorerItem>> = self.explorer_service.roots().into_iter().filter(|root| self.search_supports_scheme(&root.resource().scheme().to_string_lossy())).collect();
        *self.highlight_session_start_state.borrow_mut() = Some(roots);
    }

    /// `doHighlightFind`.
    pub fn do_highlight_find(&self, pattern: String, match_type: FindMatchType, token: CancellationToken) -> Promise<Option<ExplorerFindResult>> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(None);
        };

        async_fn(async move {
            let Some(roots) = this.highlight_session_start_state.borrow().clone() else {
                return Err(Error::new("ExplorerFindProvider: no highlight session state"));
            };
            let search_results = this.get_search_results(&pattern, &roots, match_type, &token).await?;

            if token.is_cancellation_requested() {
                return Ok(None);
            }

            this.clear_highlights();
            for SearchResults { explorer_root, files, directories, .. } in &search_results {
                let resources: Vec<URI> = files.iter().chain(directories.iter()).cloned().collect();
                this.add_workspace_highlight_results(explorer_root, &resources)?;
            }

            let weak = this.this.clone();
            let is_match = Rc::new(move |item: &Rc<ExplorerItem>| {
                let Some(this) = weak.upgrade() else {
                    return false;
                };
                this.find_highlight_tree.is_match(item) || (this.find_highlight_tree.get(item) > 0 && (this.tree_provider)().is_collapsed(item))
            });
            Ok(Some(this.find_result(&search_results, is_match)))
        })
    }

    fn add_workspace_highlight_results(&self, root: &Rc<ExplorerItem>, resources: &[URI]) -> Result<(), Error> {
        let mut highlighted_directories: Vec<Rc<ExplorerItem>> = Vec::new();
        let mut store_directories = |mut item: Option<Rc<ExplorerItem>>| {
            while let Some(current) = item {
                if !highlighted_directories.iter().any(|known| Rc::ptr_eq(known, &current)) {
                    highlighted_directories.push(current.clone());
                }
                item = current.parent();
            }
        };

        let ext_uri = self.ext_uri();
        for resource in resources {
            if let Some(element) = root.find(resource)
                && Rc::ptr_eq(&element.root(), root)
            {
                // File is already in the model
                self.find_highlight_tree.add(resource, root);
                store_directories(element.parent());
                continue;
            }

            if let Some(first_parent) = find_first_parent(&ext_uri, resource, root) {
                self.find_highlight_tree.add(resource, root);
                store_directories(first_parent.parent());
            }
        }

        let tree = (self.tree_provider)();
        for directory in &highlighted_directories {
            if tree.has_node(directory) {
                tree.rerender(directory);
            }
        }

        Ok(())
    }

    fn end_highlight_session(&self) {
        *self.highlight_session_start_state.borrow_mut() = None;
        self.clear_highlights();
    }

    fn clear_highlights(&self) {
        let tree = (self.tree_provider)();
        for item in self.find_highlight_tree.highlighted_items() {
            if tree.has_node(&item) {
                tree.rerender(&item);
            }
        }
        self.find_highlight_tree.clear();
    }

    // Search

    fn search_supports_scheme(&self, scheme: &str) -> bool {
        // Limited by the search API
        if scheme != "file" && scheme != "vscode-remote" {
            return false;
        }
        self.search_service.scheme_has_file_search_provider(scheme)
    }

    fn get_search_results(&self, pattern: &str, roots: &[Rc<ExplorerItem>], match_type: FindMatchType, token: &CancellationToken) -> Promise<Vec<SearchResults>> {
        let pattern_lowercase = pattern.to_lowercase();
        let is_fuzzy_match = match_type == FindMatchType::Fuzzy;
        // as promessas partem já; esperar em ordem preserva o `Promise.all`
        let searches: Vec<Promise<SearchResults>> =
            roots.iter().enumerate().map(|(index, root)| self.search_in_workspace(pattern_lowercase.clone(), root.clone(), index, is_fuzzy_match, token.clone())).collect();

        async_fn(async move {
            let mut results = Vec::with_capacity(searches.len());
            for search in searches {
                results.push(search.await?);
            }
            Ok(results)
        })
    }

    fn search_in_workspace(&self, pattern_lowercase: String, root: Rc<ExplorerItem>, root_index: usize, is_fuzzy_match: bool, token: CancellationToken) -> Promise<SearchResults> {
        let Some(this) = self.this.upgrade() else {
            return Promise::resolved(SearchResults { explorer_root: root, files: Vec::new(), directories: Vec::new(), hit_max_results: false });
        };

        async_fn(async move {
            let segment_match_pattern =
                if is_fuzzy_match { fuzzy_matching_glob_pattern(&pattern_lowercase) } else { continous_matching_glob_pattern(&pattern_lowercase) };

            let root_resource = root.resource();
            let configuration = this.configuration_service.get_value(None, Some(&ConfigurationOverrides::with_resource(root_resource.clone())));
            let search_exclude_pattern = get_excludes(&configuration, true).unwrap_or_default();
            let disregard_ignore_files = !this.configuration_service.get_value(Some("explorer.excludeGitIgnore"), None).is_truthy();
            let build_query = |file_pattern: String, max_results: Option<i64>| -> IFileQuery {
                let mut query = IFileQuery::new(vec![IFolderQuery {
                    folder: root_resource.clone(),
                    folder_name: None,
                    exclude_pattern: None,
                    include_pattern: None,
                    ignore_glob_case: None,
                    file_encoding: None,
                    disregard_ignore_files: Some(disregard_ignore_files),
                    disregard_global_ignore_files: None,
                    disregard_parent_ignore_files: None,
                    ignore_symlinks: None,
                }]);
                query.should_glob_match_file_pattern = Some(true);
                query.cache_key = Some(format!("explorerfindprovider:{}:{}:{}", root.name(), root_index, this.session_id.get()));
                query.common.exclude_pattern = Some(search_exclude_pattern.clone());
                query.common.ignore_glob_case = Some(true);
                query.common.max_results = max_results;
                query.file_pattern = Some(file_pattern);
                query
            };

            // As duas buscas partem juntas (o `Promise.all` do original).
            let file_search = this.search_service.file_search(&build_query(format!("**/{segment_match_pattern}"), Some(512)), Some(token.clone()));
            let folder_search = this.search_service.file_search(&build_query(format!("**/{segment_match_pattern}/**"), None), Some(token.clone()));
            let (file_results, folder_results) = match (file_search.await, folder_search.await) {
                (Ok(files), Ok(folders)) => (files, folders),
                (Err(error), _) | (_, Err(error)) => {
                    if !je_base::errors::is_cancellation_error(&error) {
                        return Err(error);
                    }
                    return Ok(SearchResults { explorer_root: root, files: Vec::new(), directories: Vec::new(), hit_max_results: false });
                }
            };

            if token.is_cancellation_requested() {
                return Ok(SearchResults { explorer_root: root, files: Vec::new(), directories: Vec::new(), hit_max_results: false });
            }

            let ext_uri = this.ext_uri();
            let file_result_resources: Vec<URI> = file_results.results.iter().map(|result| result.resource.clone()).collect();
            let folder_resources: Vec<URI> = folder_results.results.iter().map(|result| result.resource.clone()).collect();
            let directory_resources = get_matching_directories_from_files(&ext_uri, &folder_resources, &root, &segment_match_pattern)?;

            let filtered_file_resources: Vec<URI> = file_result_resources.into_iter().filter(|resource| !this.files_filter.is_ignored(resource, &root_resource, false)).collect();
            let filtered_directory_resources: Vec<URI> = directory_resources.into_iter().filter(|resource| !this.files_filter.is_ignored(resource, &root_resource, true)).collect();

            Ok(SearchResults {
                explorer_root: root,
                files: filtered_file_resources,
                directories: filtered_directory_resources,
                hit_max_results: file_results.limit_hit.unwrap_or(false) || folder_results.limit_hit.unwrap_or(false),
            })
        })
    }
}

/// `getMatchingDirectoriesFromFiles`: os diretórios no caminho dos recursos cujo último segmento casa com o padrão.
pub fn get_matching_directories_from_files(ext_uri: &ExtUri, resources: &[URI], root: &Rc<ExplorerItem>, segment_match_pattern: &str) -> Result<Vec<URI>, Error> {
    let mut unique_directories = ResourceSet::new(None);
    for resource in resources {
        let relative_path_to_root = ext_uri.relative_path(&root.resource(), resource).filter(|path| !path.is_empty()).ok_or_else(not_a_child_of_the_root)?;

        let mut dir_resource = root.resource();
        let segments: Vec<&str> = relative_path_to_root.split('/').collect();
        for stat in &segments[..segments.len() - 1] {
            dir_resource = ext_uri.join_path(&dir_resource, &[stat]);
            unique_directories.add(dir_resource.clone());
        }
    }

    let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
    let mut matching_directories: Vec<URI> = Vec::new();
    for dir_resource in unique_directories.iter() {
        let path = dir_resource.path().to_string_lossy();
        let dir_stat = path.rsplit('/').next().unwrap_or_default();
        if dir_stat.is_empty() || !glob::match_pattern(segment_match_pattern, dir_stat, &options) {
            continue;
        }

        matching_directories.push(dir_resource.clone());
    }

    Ok(matching_directories)
}
