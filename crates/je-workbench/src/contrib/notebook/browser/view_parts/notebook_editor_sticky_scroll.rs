//! Porte da lógica de `src/vs/workbench/contrib/notebook/browser/viewParts/notebookEditorStickyScroll.ts`: quais cabeçalhos de
//! seção ficam "grudados" no topo do notebook para uma posição de rolagem (`computeContent`) e as regras de altura e de
//! recolhimento que a decidem (`computeStickyHeight`, `checkCollapsedStickyLines`, `getVisibleOutlineEntry`).
//!
//! Divergências:
//! - O widget `NotebookStickyScroll` (nó DOM, `StickyFoldingIcon`, `scheduleAtNextAnimationFrame`, o evento de mudança de altura,
//!   a assinatura do outline) e os ouvintes de clique do `NotebookStickyLine` são pintura e eventos do DOM e não entram.
//! - `NotebookStickyLine` guarda a `entry` e o texto do cabeçalho (`header.innerText = entry.label`); o `isCollapsed` do ícone
//!   de dobra (`foldingState === Collapsed` de uma célula de markdown) é [`NotebookStickyLine::is_collapsed`].
//! - O `Map<OutlineEntry, { line, rendered }>` é um `Vec` na ordem de inserção do `Map` (uma entrada por `OutlineEntry`; ver
//!   [`StickyLines`]).
//! - `INotebookEditor` e `INotebookCellList` são os traits [`IStickyScrollEditor`] e [`IStickyScrollCellList`], só com o que
//!   `computeContent` lê.
//! - As constantes `22` (altura de uma linha) e `7` (nível sem cabeçalho) são [`STICKY_LINE_HEIGHT`] e
//!   [`NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL`].

use std::rc::Rc;

use crate::contrib::notebook::browser::view::notebook_cell_list::NotebookCellList;
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFoldingState;
use crate::contrib::notebook::browser::view_model::notebook_outline_entry_factory::NotebookOutlineConstants;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use crate::contrib::notebook::common::notebook_common::CellKind;
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// A altura de uma linha de cabeçalho grudado (`22`).
pub const STICKY_LINE_HEIGHT: f64 = 22.0;

/// O que `computeContent` lê do `INotebookEditor`.
pub trait IStickyScrollEditor {
    /// `scrollTop`.
    fn scroll_top(&self) -> f64;
    /// `visibleRanges`.
    fn visible_ranges(&self) -> Vec<ICellRange>;
    /// `cellAt(index)`.
    fn cell_at(&self, index: i64) -> Option<CellViewModel>;
    /// `getLayoutInfo().scrollHeight`.
    fn layout_scroll_height(&self) -> f64;
}

/// O que `computeContent` lê do `INotebookCellList`.
pub trait IStickyScrollCellList {
    /// `getCellViewScrollTop(cell)`.
    fn get_cell_view_scroll_top(&self, cell: &CellViewModel) -> f64;
}

impl IStickyScrollCellList for NotebookCellList {
    fn get_cell_view_scroll_top(&self, cell: &CellViewModel) -> f64 {
        NotebookCellList::get_cell_view_scroll_top(self, cell)
    }
}

/// `NotebookStickyLine`.
pub struct NotebookStickyLine {
    pub entry: Rc<OutlineEntry>,
    /// O `header.innerText`.
    pub header_text: String,
    /// O `isCollapsed` do `StickyFoldingIcon`.
    pub is_collapsed: bool,
}

impl NotebookStickyLine {
    /// `static getParentCount(entry)`.
    pub fn get_parent_count(entry: &Rc<OutlineEntry>) -> usize {
        let mut count = 0;
        let mut current = entry.parent();
        while let Some(parent) = current {
            count += 1;
            current = parent.parent();
        }
        count
    }
}

/// Uma entrada de `Map<OutlineEntry, { line: NotebookStickyLine; rendered: boolean }>`.
pub struct StickyLineEntry {
    pub line: Rc<NotebookStickyLine>,
    pub rendered: bool,
}

/// O `Map<OutlineEntry, { line, rendered }>` na ordem de inserção.
#[derive(Default)]
pub struct StickyLines {
    entries: Vec<(Rc<OutlineEntry>, StickyLineEntry)>,
}

impl StickyLines {
    /// `map.set(entry, value)`: reatribuir uma chave existente mantém a posição original.
    fn set(&mut self, entry: Rc<OutlineEntry>, value: StickyLineEntry) {
        match self.entries.iter_mut().find(|(candidate, _)| Rc::ptr_eq(candidate, &entry)) {
            Some(existing) => existing.1 = value,
            None => self.entries.push((entry, value)),
        }
    }

    /// `map.values()`.
    pub fn values(&self) -> impl Iterator<Item = &StickyLineEntry> {
        self.entries.iter().map(|(_, value)| value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `NotebookStickyScroll.getVisibleOutlineEntry(visibleIndex, notebookOutlineEntries)`.
pub fn get_visible_outline_entry(visible_index: i64, notebook_outline_entries: &[Rc<OutlineEntry>]) -> Option<Rc<OutlineEntry>> {
    let find_in = |root_entry: &Rc<OutlineEntry>| {
        let mut flat_list: Vec<Rc<OutlineEntry>> = Vec::new();
        root_entry.as_flat_list(&mut flat_list);
        flat_list.into_iter().find(|entry| entry.index == visible_index)
    };

    let mut left: i64 = 0;
    let mut right: i64 = notebook_outline_entries.len() as i64 - 1;

    while left <= right {
        let mid = (left + right) / 2;
        let mid_entry = &notebook_outline_entries[mid as usize];
        if mid_entry.index == visible_index {
            // Exact match found
            return find_in(mid_entry);
        } else if mid_entry.index < visible_index {
            left = mid + 1;
        } else {
            right = mid - 1;
        }
    }

    // No exact match found - get the closest smaller entry
    if right >= 0 {
        return find_in(&notebook_outline_entries[right as usize]);
    }

    None
}

/// `NotebookStickyScroll.computeStickyHeight(entry)`.
pub fn compute_sticky_height(entry: &Rc<OutlineEntry>) -> f64 {
    let mut height = 0.0;
    if entry.cell.cell_kind() == CellKind::Markup && entry.level < NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL {
        height += STICKY_LINE_HEIGHT;
    }
    let mut current = entry.parent();
    while let Some(parent) = current {
        height += STICKY_LINE_HEIGHT;
        current = parent.parent();
    }
    height
}

/// `NotebookStickyScroll.createStickyElement(entry, notebookEditor)`, sem o DOM.
fn create_sticky_element(entry: &Rc<OutlineEntry>) -> Rc<NotebookStickyLine> {
    let mut is_collapsed = false;
    if entry.cell.cell_kind() == CellKind::Markup
        && let Some(markup) = entry.cell.as_markup()
    {
        is_collapsed = markup.folding_state(&entry.cell) == CellFoldingState::Collapsed;
    }

    Rc::new(NotebookStickyLine { entry: entry.clone(), header_text: entry.label.clone(), is_collapsed })
}

/// `NotebookStickyScroll.checkCollapsedStickyLines(entry, numLinesToRender, notebookEditor)`.
pub fn check_collapsed_sticky_lines(entry: Option<&Rc<OutlineEntry>>, num_lines_to_render: f64) -> StickyLines {
    let mut current_entry = entry.cloned();
    let mut new_map = StickyLines::default();

    let mut elements_to_render: Vec<Rc<NotebookStickyLine>> = Vec::new();
    while let Some(current) = current_entry {
        if current.level >= NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL {
            // level 7+ represents a non-header entry, which we don't want to render
            current_entry = current.parent();
            continue;
        }
        let line_to_render = create_sticky_element(&current);
        new_map.set(current.clone(), StickyLineEntry { line: line_to_render.clone(), rendered: false });
        elements_to_render.insert(0, line_to_render);
        current_entry = current.parent();
    }

    // iterate over elements to render, and append to container
    // break when we reach numLinesToRender
    for (i, element) in elements_to_render.iter().enumerate() {
        if i as f64 >= num_lines_to_render {
            break;
        }
        new_map.set(element.entry.clone(), StickyLineEntry { line: element.clone(), rendered: true });
    }
    new_map
}

/// `computeContent(notebookEditor, notebookCellList, notebookOutlineEntries, renderedStickyHeight)`.
pub fn compute_content(
    notebook_editor: &dyn IStickyScrollEditor,
    notebook_cell_list: &dyn IStickyScrollCellList,
    notebook_outline_entries: &[Rc<OutlineEntry>],
    rendered_sticky_height: f64,
) -> StickyLines {
    // get data about the cell list within viewport ----------------------------------------------------------------------------------------
    let editor_scroll_top = notebook_editor.scroll_top() - rendered_sticky_height;
    let Some(visible_range) = notebook_editor.visible_ranges().first().copied() else {
        return StickyLines::default();
    };

    // edge case for cell 0 in the notebook is a header ------------------------------------------------------------------------------------
    if visible_range.start == 0 {
        let first_cell = notebook_editor.cell_at(0);
        let first_cell_entry = get_visible_outline_entry(0, notebook_outline_entries);
        if let (Some(first_cell), Some(first_cell_entry)) = (first_cell, first_cell_entry.as_ref())
            && first_cell.cell_kind() == CellKind::Markup
            && first_cell_entry.level < 7
            && notebook_editor.scroll_top() > 22.0
        {
            return check_collapsed_sticky_lines(Some(first_cell_entry), 100.0);
        }
    }

    // iterate over cells in viewport ------------------------------------------------------------------------------------------------------
    let mut cell_entry: Option<Rc<OutlineEntry>> = None;
    let start_index = visible_range.start - 1; // -1 to account for cells hidden "under" sticky lines.
    let mut current_index = start_index;
    while current_index < visible_range.end {
        // store data for current cell, and next cell
        let Some(_cell) = notebook_editor.cell_at(current_index) else {
            return StickyLines::default();
        };
        cell_entry = get_visible_outline_entry(current_index, notebook_outline_entries);
        let Some(current_entry) = cell_entry.clone() else {
            current_index += 1;
            continue;
        };

        let Some(next_cell) = notebook_editor.cell_at(current_index + 1) else {
            let section_bottom = notebook_editor.layout_scroll_height();
            let lines_to_render = (section_bottom / STICKY_LINE_HEIGHT).floor();
            return check_collapsed_sticky_lines(Some(&current_entry), lines_to_render);
        };
        let Some(next_cell_entry) = get_visible_outline_entry(current_index + 1, notebook_outline_entries) else {
            current_index += 1;
            continue;
        };

        // check next cell, if markdown with non level 7 entry, that means this is the end of the section (new header) ---------------------
        if next_cell.cell_kind() == CellKind::Markup && next_cell_entry.level < 7 {
            let section_bottom = notebook_cell_list.get_cell_view_scroll_top(&next_cell);
            let current_section_sticky_height = compute_sticky_height(&current_entry);
            let next_section_sticky_height = compute_sticky_height(&next_cell_entry);

            // case: we can render the all sticky lines for the current section ------------------------------------------------------------
            if editor_scroll_top + current_section_sticky_height < section_bottom {
                let lines_to_render = ((section_bottom - editor_scroll_top) / STICKY_LINE_HEIGHT).floor();
                return check_collapsed_sticky_lines(Some(&current_entry), lines_to_render);
            }
            // case: next section is the same size or bigger, render next entry -----------------------------------------------------------
            else if next_section_sticky_height >= current_section_sticky_height {
                return check_collapsed_sticky_lines(Some(&next_cell_entry), 100.0);
            }
            // case: next section is the smaller, shrink until next section height is greater than the available space ---------------------
            else if next_section_sticky_height < current_section_sticky_height {
                let available_space = section_bottom - editor_scroll_top;

                if available_space >= next_section_sticky_height {
                    let lines_to_render = (available_space / STICKY_LINE_HEIGHT).floor();
                    return check_collapsed_sticky_lines(Some(&current_entry), lines_to_render);
                } else {
                    return check_collapsed_sticky_lines(Some(&next_cell_entry), 100.0);
                }
            }
        }
        current_index += 1;
    } // visible range loop close

    // case: all visible cells were non-header cells, so render any headers relevant to their section --------------------------------------
    let section_bottom = notebook_editor.layout_scroll_height();
    let lines_to_render = ((section_bottom - editor_scroll_top) / STICKY_LINE_HEIGHT).floor();
    check_collapsed_sticky_lines(cell_entry.as_ref(), lines_to_render)
}
