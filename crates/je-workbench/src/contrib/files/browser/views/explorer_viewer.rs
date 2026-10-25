//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts`, fatia 1: a parte lógica, sem DOM.
//!
//! Nesta fatia: `ExplorerDelegate`, `ExplorerCompressionDelegate`, `FileSorter`, a árvore de destaque da busca
//! (`ExplorerFindHighlightTree`), os padrões de glob da busca (`fuzzyMatchingGlobPattern`,
//! `continousMatchingGlobPattern`, `getMatchingDirectoriesFromFiles`, `findFirstParent`), o `getFileOrFolderLabelSuffix`,
//! a decisão de `FileDragAndDrop.handleDragOver` ([`handle_drag_over`]) e o `ICompressedNavigationController` com o
//! `CompressedNavigationController`.
//!
//! Plano da fatia 2 (a seguir, nesta ordem): (a) `ExplorerDataSource` e `FilesFilter` sobre `IFileService`,
//! `IExplorerService`, `IFilesConfigurationService`, `IgnoreFile` e `TernarySearchTree`; (b) `ExplorerFindProvider` sobre
//! `ISearchService` e a árvore compressível assíncrona de je-widgets; (c) `FilesRenderer` sobre `ResourceLabels` de
//! `labels.rs` e o `ViewPane` do workbench; (d) o restante do `FileDragAndDrop` (`drop`, `handleExplorerDrop`,
//! `doHandleRootDrop`, cópia e movimento com `applyBulkEdit`); (e) `explorerView.ts` inteiro e os 16 casos da suíte
//! `context menu` de `explorer_view.rs`.
//!
//! Divergências:
//! - **DOM.** A UI é nativa: o `templateData` (`container`, `label.onDidRender`) e os `labels: HTMLElement[]` não existem. O
//!   `updateLabels` (atributos `aria-label`, `aria-level`, `aria-expanded` e a classe `active`) é só apresentação e fica
//!   com o widget que desenhar o rótulo; o controlador guarda o índice, a profundidade e o estado colapsado.
//! - **`static ID`.** Contador de instâncias: [`CompressedNavigationController::next_id`] (o `ID++` do original).
//! - **Texto.** Os comparadores de nomes recebem unidades UTF-16 (`&[u16]`), como no resto do porte.
//! - **`handleDragOver`.** O que o original lê do `DragEvent` e do `IDragAndDropData` entra em [`DragOverInput`]; a
//!   resposta é [`DragOverReaction`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::comparers::{
    compare_file_extensions_default, compare_file_extensions_lower, compare_file_extensions_unicode, compare_file_extensions_upper,
    compare_file_names_default, compare_file_names_lower, compare_file_names_unicode, compare_file_names_upper,
};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::workspace::IWorkspaceContextService;
use je_widgets::list::list_types::IListVirtualDelegate;
use je_widgets::tree::compressible_async_data_tree::ITreeCompressionDelegate;
use je_widgets::tree::tree_types::ITreeSorter;

use crate::contrib::files::common_files::{LexicographicOptions, SortOrder};
use crate::contrib::files::explorer_model::ExplorerItem;
use crate::contrib::files::browser_files::IExplorerService;

/// `ICompressedNavigationController`.
pub trait ICompressedNavigationController {
    fn current(&self) -> Rc<ExplorerItem>;
    fn current_id(&self) -> String;
    fn items(&self) -> Vec<Rc<ExplorerItem>>;
    fn index(&self) -> usize;
    fn count(&self) -> usize;
    fn on_did_change(&self) -> Event<()>;
    fn previous(&self);
    fn next(&self);
    fn first(&self);
    fn last(&self);
    fn set_index(&self, index: i64);
    fn update_collapsed(&self, collapsed: bool);
}

thread_local! {
    /// `CompressedNavigationController.ID`.
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

/// `CompressedNavigationController`.
pub struct CompressedNavigationController {
    id: String,
    items: Vec<Rc<ExplorerItem>>,
    index: Cell<usize>,
    depth: usize,
    collapsed: Cell<bool>,
    on_did_change: Rc<Emitter<()>>,
}

impl CompressedNavigationController {
    /// `CompressedNavigationController.ID++`.
    pub fn next_id() -> u64 {
        NEXT_ID.with(|id| {
            let value = id.get();
            id.set(value + 1);
            value
        })
    }

    /// `constructor(id, items, templateData, depth, collapsed)`, sem o `templateData` (ver o topo do módulo).
    pub fn new(id: impl Into<String>, items: Vec<Rc<ExplorerItem>>, depth: usize, collapsed: bool) -> Rc<Self> {
        let index = items.len().saturating_sub(1);
        Rc::new(Self { id: id.into(), items, index: Cell::new(index), depth, collapsed: Cell::new(collapsed), on_did_change: Emitter::new() })
    }

    /// `depth`.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// `collapsed`.
    pub fn collapsed(&self) -> bool {
        self.collapsed.get()
    }
}

impl ICompressedNavigationController for CompressedNavigationController {
    fn current(&self) -> Rc<ExplorerItem> {
        self.items[self.index.get()].clone()
    }

    fn current_id(&self) -> String {
        format!("{}_{}", self.id, self.index.get())
    }

    fn items(&self) -> Vec<Rc<ExplorerItem>> {
        self.items.clone()
    }

    fn index(&self) -> usize {
        self.index.get()
    }

    fn count(&self) -> usize {
        self.items.len()
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn previous(&self) {
        if self.index.get() == 0 {
            return;
        }

        self.set_index(self.index.get() as i64 - 1);
    }

    fn next(&self) {
        if self.index.get() + 1 >= self.items.len() {
            return;
        }

        self.set_index(self.index.get() as i64 + 1);
    }

    fn first(&self) {
        if self.index.get() == 0 {
            return;
        }

        self.set_index(0);
    }

    fn last(&self) {
        if self.index.get() + 1 == self.items.len() {
            return;
        }

        self.set_index(self.items.len() as i64 - 1);
    }

    fn set_index(&self, index: i64) {
        if index < 0 || index >= self.items.len() as i64 {
            return;
        }

        self.index.set(index as usize);
        self.on_did_change.fire(());
    }

    fn update_collapsed(&self, collapsed: bool) {
        self.collapsed.set(collapsed);
    }
}

impl IDisposable for CompressedNavigationController {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// `FilesRenderer.ID`.
pub const FILES_RENDERER_ID: &str = "file";

/// `ExplorerDelegate`.
pub struct ExplorerDelegate;

impl ExplorerDelegate {
    pub const ITEM_HEIGHT: f64 = 22.0;
}

impl IListVirtualDelegate<Rc<ExplorerItem>> for ExplorerDelegate {
    fn get_height(&self, _element: &Rc<ExplorerItem>) -> f64 {
        ExplorerDelegate::ITEM_HEIGHT
    }

    fn get_template_id(&self, _element: &Rc<ExplorerItem>) -> String {
        FILES_RENDERER_ID.to_string()
    }
}

/// `ExplorerCompressionDelegate`.
pub struct ExplorerCompressionDelegate;

impl ITreeCompressionDelegate<Rc<ExplorerItem>> for ExplorerCompressionDelegate {
    fn is_incompressible(&self, stat: &Rc<ExplorerItem>) -> bool {
        stat.is_root() || !stat.is_directory() || stat.is_new_explorer_item() || stat.parent().is_none_or(|parent| parent.is_root())
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `FileSorter`.
pub struct FileSorter {
    explorer_service: Rc<dyn IExplorerService>,
    context_service: Rc<dyn IWorkspaceContextService>,
}

impl FileSorter {
    pub fn new(explorer_service: Rc<dyn IExplorerService>, context_service: Rc<dyn IWorkspaceContextService>) -> Self {
        Self { explorer_service, context_service }
    }
}

impl ITreeSorter<Rc<ExplorerItem>> for FileSorter {
    fn compare(&self, stat_a: &Rc<ExplorerItem>, stat_b: &Rc<ExplorerItem>) -> i32 {
        // Do not sort roots
        if stat_a.is_root() {
            if stat_b.is_root() {
                let workspace_a = self.context_service.get_workspace_folder(&stat_a.resource());
                let workspace_b = self.context_service.get_workspace_folder(&stat_b.resource());
                return match (workspace_a, workspace_b) {
                    (Some(a), Some(b)) => a.index as i32 - b.index as i32,
                    _ => -1,
                };
            }

            return -1;
        }

        if stat_b.is_root() {
            return 1;
        }

        let configuration = self.explorer_service.sort_order_configuration();
        let sort_order = configuration.sort_order;
        let (mut stat_a, mut stat_b) = (stat_a, stat_b);
        if configuration.reverse {
            std::mem::swap(&mut stat_a, &mut stat_b);
        }

        let compare_file_names = match configuration.lexicographic_options {
            LexicographicOptions::Upper => compare_file_names_upper,
            LexicographicOptions::Lower => compare_file_names_lower,
            LexicographicOptions::Unicode => compare_file_names_unicode,
            LexicographicOptions::Default => compare_file_names_default,
        };
        let compare_file_extensions = match configuration.lexicographic_options {
            LexicographicOptions::Upper => compare_file_extensions_upper,
            LexicographicOptions::Lower => compare_file_extensions_lower,
            LexicographicOptions::Unicode => compare_file_extensions_unicode,
            LexicographicOptions::Default => compare_file_extensions_default,
        };
        let (name_a, name_b) = (utf16(&stat_a.name()), utf16(&stat_b.name()));
        let names = || compare_file_names(Some(&name_a), Some(&name_b));

        // Sort Directories
        match sort_order {
            SortOrder::Type => {
                if stat_a.is_directory() && !stat_b.is_directory() {
                    return -1;
                }

                if stat_b.is_directory() && !stat_a.is_directory() {
                    return 1;
                }

                if stat_a.is_directory() && stat_b.is_directory() {
                    return names();
                }
            }

            SortOrder::FilesFirst => {
                if stat_a.is_directory() && !stat_b.is_directory() {
                    return 1;
                }

                if stat_b.is_directory() && !stat_a.is_directory() {
                    return -1;
                }
            }

            SortOrder::FoldersNestsFiles => {
                if stat_a.is_directory() && !stat_b.is_directory() {
                    return -1;
                }

                if stat_b.is_directory() && !stat_a.is_directory() {
                    return 1;
                }

                if stat_a.has_nests() && !stat_b.has_nests() {
                    return -1;
                }

                if stat_b.has_nests() && !stat_a.has_nests() {
                    return 1;
                }
            }

            SortOrder::Mixed => {} // not sorting when "mixed" is on

            SortOrder::Default | SortOrder::Modified => {
                if stat_a.is_directory() && !stat_b.is_directory() {
                    return -1;
                }

                if stat_b.is_directory() && !stat_a.is_directory() {
                    return 1;
                }
            }
        }

        // Sort Files
        match sort_order {
            SortOrder::Type => compare_file_extensions(Some(&name_a), Some(&name_b)),

            SortOrder::Modified => {
                let (mtime_a, mtime_b) = (stat_a.mtime(), stat_b.mtime());
                if mtime_a != mtime_b {
                    // `statA.mtime && statB.mtime && statA.mtime < statB.mtime`: zero e ausente são falsos
                    return match (mtime_a, mtime_b) {
                        (Some(a), Some(b)) if a != 0.0 && b != 0.0 && a < b => 1,
                        _ => -1,
                    };
                }

                names()
            }

            SortOrder::Default | SortOrder::Mixed | SortOrder::FilesFirst | SortOrder::FoldersNestsFiles => names(),
        }
    }
}

/// `FindHighlightLayer`.
#[derive(Default)]
struct FindHighlightLayer {
    child_matches: usize,
    is_match: bool,
    stats: HashMap<String, FindHighlightLayer>,
}

/// `IExplorerFindHighlightTree`.
pub trait IExplorerFindHighlightTree {
    fn get(&self, item: &Rc<ExplorerItem>) -> usize;
    fn is_match(&self, item: &Rc<ExplorerItem>) -> bool;
}

fn not_a_child_of_the_root() -> ! {
    errors::throw(Error::new("Resource is not a child of the root"))
}

/// `ExplorerFindHighlightTree`.
pub struct ExplorerFindHighlightTree {
    ext_uri: Rc<ExtUri>,
    tree: RefCell<HashMap<String, FindHighlightLayer>>,
    highlighted_items: RefCell<HashMap<String, Rc<ExplorerItem>>>,
}

impl ExplorerFindHighlightTree {
    pub fn new(ext_uri: Rc<ExtUri>) -> Self {
        Self { ext_uri, tree: RefCell::new(HashMap::new()), highlighted_items: RefCell::new(HashMap::new()) }
    }

    /// `highlightedItems`.
    pub fn highlighted_items(&self) -> Vec<Rc<ExplorerItem>> {
        self.highlighted_items.borrow().values().cloned().collect()
    }

    /// `find`: devolve o caminho relativo e a leitura da camada (`childMatches`, `isMatch`).
    fn find(&self, item: &Rc<ExplorerItem>) -> Option<(String, usize, bool)> {
        let root = item.root();
        let tree = self.tree.borrow();
        let root_layer = tree.get(&root.name())?;

        let rel_path = self.ext_uri.relative_path(&root.resource(), &item.resource());
        let Some(rel_path) = rel_path.filter(|rel_path| !rel_path.starts_with("..")) else {
            not_a_child_of_the_root();
        };

        if rel_path.is_empty() {
            return Some((rel_path, root_layer.child_matches, root_layer.is_match));
        }

        let mut tree_layer = root_layer;
        for segment in rel_path.split('/') {
            tree_layer = tree_layer.stats.get(segment)?;
        }

        Some((rel_path, tree_layer.child_matches, tree_layer.is_match))
    }

    /// `add`.
    pub fn add(&self, resource: &URI, root: &Rc<ExplorerItem>) {
        let rel_path = self.ext_uri.relative_path(&root.resource(), resource);
        let Some(rel_path) = rel_path.filter(|rel_path| !rel_path.starts_with("..")) else {
            not_a_child_of_the_root();
        };

        let mut tree = self.tree.borrow_mut();
        let root_layer = tree.entry(root.name()).or_default();
        root_layer.child_matches += 1;

        let mut tree_layer = root_layer;
        for stat in rel_path.split('/') {
            tree_layer = tree_layer.stats.entry(stat.to_string()).or_default();
            tree_layer.child_matches += 1;
        }

        tree_layer.child_matches -= 1; // the last segment is the file itself
        tree_layer.is_match = true;
    }

    /// `clear`.
    pub fn clear(&self) {
        self.tree.borrow_mut().clear();
    }
}

impl IExplorerFindHighlightTree for ExplorerFindHighlightTree {
    fn get(&self, item: &Rc<ExplorerItem>) -> usize {
        let Some((rel_path, child_matches, _)) = self.find(item) else {
            return 0;
        };

        self.highlighted_items.borrow_mut().insert(rel_path, item.clone());

        child_matches
    }

    fn is_match(&self, item: &Rc<ExplorerItem>) -> bool {
        self.find(item).is_some_and(|(_, _, is_match)| is_match)
    }
}

/// `fuzzyMatchingGlobPattern`.
pub fn fuzzy_matching_glob_pattern(pattern: &str) -> String {
    if pattern.is_empty() {
        return "*".to_string();
    }

    let letters: Vec<String> = pattern.encode_utf16().map(|unit| String::from_utf16_lossy(&[unit])).collect();
    format!("*{}*", letters.join("*"))
}

/// `continousMatchingGlobPattern`.
pub fn continous_matching_glob_pattern(pattern: &str) -> String {
    if pattern.is_empty() {
        return "*".to_string();
    }

    format!("*{pattern}*")
}

/// `findFirstParent`: o ancestral mais profundo do recurso que já está no modelo, ou `None` quando o próprio recurso está.
pub fn find_first_parent(ext_uri: &ExtUri, resource: &URI, root: &Rc<ExplorerItem>) -> Option<Rc<ExplorerItem>> {
    let relative_path_to_root = ext_uri.relative_path(&root.resource(), resource).filter(|path| !path.is_empty());
    let Some(relative_path_to_root) = relative_path_to_root else {
        not_a_child_of_the_root();
    };

    let mut current_item = root.clone();
    for stat in relative_path_to_root.split('/') {
        let Some(child) = current_item.get_child(stat) else {
            return Some(current_item);
        };

        current_item = child;
    }

    None
}

/// `getFileOrFolderLabelSuffix`.
pub fn get_file_or_folder_label_suffix(items: &[Rc<ExplorerItem>]) -> String {
    if items.len() == 1 {
        return items[0].name();
    }

    if items.iter().all(|item| item.is_directory()) {
        return localize("numberOfFolders", "{0} folders", &[items.len().to_string()]);
    }
    if items.iter().all(|item| !item.is_directory()) {
        return localize("numberOfFiles", "{0} files", &[items.len().to_string()]);
    }

    format!("{} files and folders", items.len())
}

/// `ListDragOverEffectType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOverEffectType {
    Copy,
    Move,
}

/// `ListDragOverEffectPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOverEffectPosition {
    Over,
    Before,
    After,
}

/// `ListViewTargetSector`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetSector {
    Top,
    CenterTop,
    CenterBottom,
    Bottom,
}

/// `TreeDragOverBubble`.
pub use crate::browser::parts::views::tree_view::TreeDragOverBubble;

/// `boolean | ITreeDragOverReaction` do `FileDragAndDrop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOverReaction {
    Reject,
    Accept { bubble: Option<TreeDragOverBubble>, effect: (DragOverEffectType, Option<DragOverEffectPosition>), auto_expand: Option<bool> },
}

/// De onde vem o dado arrastado: o `data instanceof ...` do original.
#[derive(Clone)]
pub enum DragOverData {
    /// `NativeDragAndDropData`, com o resultado de `containsDragType(FILES, CodeDataTransfers.FILES, RESOURCES)`.
    Native { contains_files_type: bool },
    /// `ExternalElementsDragAndDropData`.
    ExternalElements,
    /// `ElementsDragAndDropData`, já resolvido por `getStatsFromDragAndDropData`.
    Explorer(Vec<Rc<ExplorerItem>>),
}

/// O que `FileDragAndDrop.handleDragOver` lê do evento e do contexto.
pub struct DragOverInput<'a> {
    pub data: DragOverData,
    pub target: Option<Rc<ExplorerItem>>,
    pub target_sector: Option<TargetSector>,
    /// `ctrlKey && !isMacintosh || altKey && isMacintosh`.
    pub is_copy: bool,
    pub ext_uri: &'a ExtUri,
    pub context_service: &'a dyn IWorkspaceContextService,
}

/// `FileDragAndDrop.handleDragOver`.
pub fn handle_drag_over(input: &DragOverInput) -> DragOverReaction {
    let is_native = matches!(input.data, DragOverData::Native { .. });
    let effect_type = if is_native || input.is_copy { DragOverEffectType::Copy } else { DragOverEffectType::Move };
    let effect = (effect_type, Some(DragOverEffectPosition::Over));
    let target = &input.target;

    match &input.data {
        // Native DND
        DragOverData::Native { contains_files_type } => {
            if !contains_files_type {
                return DragOverReaction::Reject;
            }
        }

        // Other-Tree DND
        DragOverData::ExternalElements => return DragOverReaction::Reject,

        // In-Explorer DND
        DragOverData::Explorer(items) => {
            let is_roots_reorder = items.iter().all(|item| item.is_root());

            let Some(target) = target else {
                // Dropping onto the empty area. Do not accept if items dragged are already
                // children of the root unless we are copying the file
                if !input.is_copy && items.iter().all(|i| i.parent().is_some_and(|parent| parent.is_root())) {
                    return DragOverReaction::Reject;
                }

                // root is added after last root folder when hovering on empty background
                if is_roots_reorder {
                    return DragOverReaction::Accept { bubble: None, effect: (DragOverEffectType::Move, Some(DragOverEffectPosition::After)), auto_expand: None };
                }

                return DragOverReaction::Accept { bubble: Some(TreeDragOverBubble::Down), effect, auto_expand: Some(false) };
            };

            if !input.is_copy && items.iter().all(|source| source.is_readonly().is_readonly()) {
                return DragOverReaction::Reject; // Cannot move readonly items unless we copy
            }

            let invalid = items.iter().any(|source| {
                if source.is_root() {
                    return false; // Root folders are handled seperately
                }

                let (source_resource, target_resource) = (source.resource(), target.resource());
                if input.ext_uri.is_equal(Some(&source_resource), Some(&target_resource), false) {
                    return true; // Can not move anything onto itself excpet for root folders
                }

                if !input.is_copy && input.ext_uri.is_equal(Some(&input.ext_uri.dirname(&source_resource)), Some(&target_resource), false) {
                    return true; // Can not move a file to the same parent unless we copy
                }

                // Can not move a parent folder into one of its children
                input.ext_uri.is_equal_or_parent(&target_resource, &source_resource, false, false)
            });
            if invalid {
                return DragOverReaction::Reject;
            }

            // reordering roots
            if is_roots_reorder {
                if !target.is_root() {
                    return DragOverReaction::Reject;
                }

                let position = match input.target_sector {
                    Some(TargetSector::Top | TargetSector::CenterTop) => Some(DragOverEffectPosition::Before),
                    Some(TargetSector::CenterBottom | TargetSector::Bottom) => Some(DragOverEffectPosition::After),
                    None => None,
                };
                return DragOverReaction::Accept { bubble: None, effect: (DragOverEffectType::Move, position), auto_expand: None };
            }
        }
    }

    // All (target = model)
    let Some(target) = target else {
        return DragOverReaction::Accept { bubble: Some(TreeDragOverBubble::Down), effect, auto_expand: None };
    };

    // All (target = file/folder)
    if target.is_directory() {
        if target.is_readonly().is_readonly() {
            return DragOverReaction::Reject;
        }

        return DragOverReaction::Accept { bubble: Some(TreeDragOverBubble::Down), effect, auto_expand: Some(true) };
    }

    let target_resource = target.resource();
    if input.context_service.get_workspace().folders().iter().all(|folder| folder.uri.to_string() != target_resource.to_string()) {
        return DragOverReaction::Accept { bubble: Some(TreeDragOverBubble::Up), effect, auto_expand: None };
    }

    DragOverReaction::Reject
}
