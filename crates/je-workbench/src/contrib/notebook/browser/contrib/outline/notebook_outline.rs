//! Porte dos provedores de `src/vs/workbench/contrib/notebook/browser/contrib/outline/notebookOutline.ts`:
//! [`NotebookQuickPickProvider`], [`NotebookOutlinePaneProvider`] (com `filterEntry`) e [`NotebookBreadcrumbsProvider`], que
//! filtram as entradas do outline conforme as configurações de cada visão (painel, seletor rápido e breadcrumbs).
//!
//! Divergências:
//! - `NotebookOutlineRenderer`, `NotebookOutlineTemplate`, `NotebookOutlineAccessibility`, `NotebookNavigationLabelProvider`,
//!   `NotebookOutlineVirtualDelegate` (renderização da árvore), `NotebookComparator` (`Intl.Collator` numérico, sem porte),
//!   `NotebookCellOutline`, `NotebookOutlineCreator` e o contexto de ação (`NotebookOutlineContext`, menus e ações
//!   registradas) dependem de widgets, do `IEditorPane`/`NotebookEditor` e do registro de ações, e não entram.
//! - `IReference<INotebookCellOutlineDataSource> | undefined` é `Option<Reference<Rc<dyn INotebookCellOutlineDataSource>>>`.
//! - `IThemeService.getFileIconTheme()` (o `IThemeService` do porte não tem tema de ícones de arquivo) é o `Rc<dyn Fn() ->
//!   IFileIconTheme>` do construtor do seletor rápido.
//! - O `NotebookCellOutline | OutlineEntry` de `getChildren(element)` é o [`OutlinePaneParent`]; o `element: OutlineEntry`
//!   genérico dos contratos de `outline.ts` é o `Rc<dyn Any>` de `OutlineElement` (um `Rc<OutlineEntry>`).
//! - A leitura das configurações e o `onDidChangeConfiguration` ficam em `Cell<bool>` compartilhados com o ouvinte.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, Reference};
use je_editor::get_icon_classes::get_icon_classes_for_language_id;
use je_platform::configuration::service::IConfigurationService;

use crate::contrib::notebook::browser::view_model::notebook_outline_data_source::INotebookCellOutlineDataSource;
use crate::contrib::notebook::browser::view_model::notebook_outline_entry_factory::NotebookOutlineConstants;
use crate::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use crate::contrib::notebook::common::notebook_common::{CellKind, NotebookSetting};
use crate::services::outline::outline::{BreadcrumbsOutlineElement, IBreadcrumbsDataSource, IQuickPickDataSource, QuickPickOutlineElement};

/// `IFileIconTheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IFileIconTheme {
    pub has_file_icons: bool,
    pub has_folder_icons: bool,
    pub hides_explorer_arrows: bool,
}

/// `IReference<INotebookCellOutlineDataSource> | undefined`.
pub type OutlineDataSourceRef = Option<Reference<Rc<dyn INotebookCellOutlineDataSource>>>;

pub(super) fn read_bool(configuration_service: &dyn IConfigurationService, key: &str) -> bool {
    matches!(configuration_service.get_value(Some(key), None), Dynamic::Bool(true))
}

/// Mantém um `Cell<bool>` igual à configuração `key` (lê agora e a cada `onDidChangeConfiguration` que a afeta).
fn track_setting(configuration_service: &Rc<dyn IConfigurationService>, disposables: &DisposableStore, key: &'static str) -> Rc<Cell<bool>> {
    let value = Rc::new(Cell::new(read_bool(configuration_service.as_ref(), key)));
    let tracked = value.clone();
    let service = configuration_service.clone();
    let listener: DisposableRef = configuration_service.on_did_change_configuration().listen(move |e| {
        if e.affects_configuration(key, None) {
            tracked.set(read_bool(service.as_ref(), key));
        }
    });
    disposables.add(listener);
    value
}

/// `NotebookQuickPickProvider`.
pub struct NotebookQuickPickProvider {
    disposables: Rc<DisposableStore>,
    notebook_cell_outline_data_source_ref: OutlineDataSourceRef,
    goto_show_code_cell_symbols: Rc<Cell<bool>>,
    file_icon_theme: Rc<dyn Fn() -> IFileIconTheme>,
}

impl NotebookQuickPickProvider {
    pub fn new(
        notebook_cell_outline_data_source_ref: OutlineDataSourceRef,
        configuration_service: &Rc<dyn IConfigurationService>,
        file_icon_theme: Rc<dyn Fn() -> IFileIconTheme>,
    ) -> Self {
        let disposables = DisposableStore::new();
        let goto_show_code_cell_symbols = track_setting(configuration_service, &disposables, NotebookSetting::GOTO_SYMBOLS_ALL_SYMBOLS);
        Self { disposables, notebook_cell_outline_data_source_ref, goto_show_code_cell_symbols, file_icon_theme }
    }
}

impl IQuickPickDataSource for NotebookQuickPickProvider {
    fn get_quick_pick_elements(&self) -> Vec<QuickPickOutlineElement> {
        let mut bucket: Vec<Rc<OutlineEntry>> = Vec::new();
        if let Some(reference) = &self.notebook_cell_outline_data_source_ref {
            for entry in reference.object.entries() {
                entry.as_flat_list(&mut bucket);
            }
        }
        let mut result: Vec<QuickPickOutlineElement> = Vec::new();
        let has_file_icons = (self.file_icon_theme)().has_file_icons;
        let goto_show_code_cell_symbols = self.goto_show_code_cell_symbols.get();

        let is_symbol = |element: &OutlineEntry| element.has_symbol_kind();
        // code cell entries are exactly level 7 by this constant
        let is_code_cell = |element: &OutlineEntry| element.cell.cell_kind() == CellKind::Code && element.level == NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL;
        for i in 0..bucket.len() {
            let element = &bucket[i];
            let next_element = bucket.get(i + 1); // can be undefined

            if !goto_show_code_cell_symbols && is_symbol(element) {
                continue;
            }

            if goto_show_code_cell_symbols && is_code_cell(element) && next_element.is_some_and(|next| is_symbol(next)) {
                continue;
            }

            let use_file_icon = has_file_icons && !element.has_symbol_kind();
            // todo@jrieken it is fishy that codicons cannot be used with iconClasses
            // but file icons can...
            result.push(QuickPickOutlineElement {
                element: element.clone() as Rc<dyn Any>,
                label: if use_file_icon { element.label.clone() } else { format!("$({}) {}", element.icon().id, element.label) },
                aria_label: Some(element.label.clone()),
                icon_classes: if use_file_icon { Some(get_icon_classes_for_language_id(&element.cell.language())) } else { None },
                description: None,
            });
        }
        result
    }
}

impl IDisposable for NotebookQuickPickProvider {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// Checks if the given outline entry should be filtered out of the outlinePane.
///
/// Devolve `true` se a entrada deve ficar fora do painel, `false` se ela fica visível.
pub fn filter_entry(entry: &OutlineEntry, show_markdown_headers_only: bool, show_code_cells: bool, show_code_cell_symbols: bool) -> bool {
    let kind = entry.cell.cell_kind();
    // if any are true, return true, this entry should NOT be included in the outline
    // show headers only   + cell is mkdn + is level 7 (not header)
    (show_markdown_headers_only && kind == CellKind::Markup && entry.level == NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL)
        // show code cells off + cell is code
        || (!show_code_cells && kind == CellKind::Code)
        // show symbols off    + cell is code + is level >7 (nb symbol levels)
        || (!show_code_cell_symbols && kind == CellKind::Code && entry.level > NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL)
}

/// O `NotebookCellOutline | OutlineEntry` do `element` de `getChildren`.
pub enum OutlinePaneParent {
    /// O próprio outline: os filhos são as entradas da fonte de dados.
    Outline,
    Entry(Rc<OutlineEntry>),
}

/// `NotebookOutlinePaneProvider`.
pub struct NotebookOutlinePaneProvider {
    disposables: Rc<DisposableStore>,
    outline_data_source_ref: OutlineDataSourceRef,
    show_code_cells: Rc<Cell<bool>>,
    show_code_cell_symbols: Rc<Cell<bool>>,
    show_markdown_headers_only: Rc<Cell<bool>>,
}

impl NotebookOutlinePaneProvider {
    pub fn new(outline_data_source_ref: OutlineDataSourceRef, configuration_service: &Rc<dyn IConfigurationService>) -> Self {
        let disposables = DisposableStore::new();
        let show_code_cells = track_setting(configuration_service, &disposables, NotebookSetting::OUTLINE_SHOW_CODE_CELLS);
        let show_code_cell_symbols = track_setting(configuration_service, &disposables, NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS);
        let show_markdown_headers_only = track_setting(configuration_service, &disposables, NotebookSetting::OUTLINE_SHOW_MARKDOWN_HEADERS_ONLY);
        Self { disposables, outline_data_source_ref, show_code_cells, show_code_cell_symbols, show_markdown_headers_only }
    }

    fn filtered(&self, entry: &OutlineEntry) -> bool {
        filter_entry(entry, self.show_markdown_headers_only.get(), self.show_code_cells.get(), self.show_code_cell_symbols.get())
    }

    /// `getActiveEntry()`.
    pub fn get_active_entry(&self) -> Option<Rc<OutlineEntry>> {
        let new_active = self.outline_data_source_ref.as_ref()?.object.active_element()?;

        if !self.filtered(&new_active) {
            return Some(new_active);
        }

        // find a valid parent
        let mut parent = new_active.parent();
        while let Some(candidate) = parent {
            if self.filtered(&candidate) {
                parent = candidate.parent();
            } else {
                return Some(candidate);
            }
        }

        // no valid parent found, return undefined
        None
    }

    /// `getChildren(element)`.
    pub fn get_children(&self, element: &OutlinePaneParent) -> Vec<Rc<OutlineEntry>> {
        let entries: Vec<Rc<OutlineEntry>> = match element {
            OutlinePaneParent::Outline => self.outline_data_source_ref.as_ref().map(|reference| reference.object.entries()).unwrap_or_default(),
            OutlinePaneParent::Entry(entry) => entry.children(),
        };

        let show_markdown_headers_only = self.show_markdown_headers_only.get();
        let show_code_cells = self.show_code_cells.get();
        let show_code_cell_symbols = self.show_code_cell_symbols.get();

        let mut result: Vec<Rc<OutlineEntry>> = Vec::new();
        for entry in entries {
            let kind = entry.cell.cell_kind();
            if kind == CellKind::Markup {
                if !show_markdown_headers_only || entry.level < NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL {
                    result.push(entry);
                }
            } else if show_code_cells && kind == CellKind::Code {
                if show_code_cell_symbols || entry.level == NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL {
                    result.push(entry);
                }
            }
        }
        result
    }
}

impl IDisposable for NotebookOutlinePaneProvider {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `NotebookBreadcrumbsProvider`.
pub struct NotebookBreadcrumbsProvider {
    disposables: Rc<DisposableStore>,
    outline_data_source_ref: OutlineDataSourceRef,
    show_code_cells: Rc<Cell<bool>>,
}

impl NotebookBreadcrumbsProvider {
    pub fn new(outline_data_source_ref: OutlineDataSourceRef, configuration_service: &Rc<dyn IConfigurationService>) -> Self {
        let disposables = DisposableStore::new();
        let show_code_cells = track_setting(configuration_service, &disposables, NotebookSetting::BREADCRUMBS_SHOW_CODE_CELLS);
        Self { disposables, outline_data_source_ref, show_code_cells }
    }
}

impl IBreadcrumbsDataSource for NotebookBreadcrumbsProvider {
    fn get_breadcrumb_elements(&self) -> Vec<BreadcrumbsOutlineElement> {
        let mut result: Vec<BreadcrumbsOutlineElement> = Vec::new();
        let mut candidate = self.outline_data_source_ref.as_ref().and_then(|reference| reference.object.active_element());
        while let Some(entry) = candidate {
            if self.show_code_cells.get() || entry.cell.cell_kind() != CellKind::Code {
                result.insert(0, BreadcrumbsOutlineElement { element: entry.clone() as Rc<dyn Any>, label: entry.label.clone() });
            }
            candidate = entry.parent();
        }
        result
    }
}

impl IDisposable for NotebookBreadcrumbsProvider {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
