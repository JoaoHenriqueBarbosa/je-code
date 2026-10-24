//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/foldingModel.ts`: as regiões de dobra do notebook, calculadas
//! pelos cabeçalhos de markdown das células (`FoldingModel`).
//!
//! Divergências:
//! - `INotebookViewModel` (a interface) tem uma única implementação, então o modelo guarda o `Rc<NotebookViewModel>`.
//! - `FoldingModel` é `Rc<FoldingModel>` (os ouvintes do view model guardam um `Weak`); `regions` devolve o `Rc<FoldingRegions>`
//!   que o `FoldingRegion` precisa para virar região (`toRegion`).
//! - `getRegionsInside(region, filter)` com `filter.length === 2` (filtro com nível) é o método separado
//!   [`FoldingModel::get_regions_inside_with_level`]; sem nível é [`FoldingModel::get_regions_inside`].
//! - `getMarkdownHeadersInCell` usa o lexer `marked` do porte (`je_base::marked`) e devolve `depth` e `text`; o `text` é o
//!   `renderAsPlaintext` do `raw` do cabeçalho (`je_base::markdown_plaintext`) com `trim()`, como no original.
//! - O erro do lexer (que o JavaScript lançaria) é tratado como "sem cabeçalhos".
//! - `foldingRangeLimit` é o [`FoldingLimitReporter`] com `limit: 5000` e `update` vazio.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::marked::{Options, Token, lexer_with};
use je_base::markdown_plaintext::render_as_plaintext;
use je_editor::folding::FoldingLimitReporter;
use je_editor::folding::folding_ranges::{FoldingRegion, FoldingRegions, ILineRange};
use je_editor::folding::syntax_range_provider::{FoldingRangeData, sanitize_ranges};
use je_editor::model::TrackedRangeStickiness;

use super::notebook_view_model_impl::NotebookViewModel;
use crate::contrib::notebook::common::notebook_common::CellKind;
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_ranges_to_indexes};

struct FoldingRangeLimit;

impl FoldingLimitReporter for FoldingRangeLimit {
    fn limit(&self) -> usize {
        5000
    }

    fn update(&self, _computed: usize, _limited: Option<usize>) {}
}

/// `FoldingModel`.
pub struct FoldingModel {
    view_model: RefCell<Option<Rc<NotebookViewModel>>>,
    view_model_store: Rc<DisposableStore>,
    regions: RefCell<Rc<FoldingRegions>>,
    on_did_folding_region_changes: Rc<Emitter<()>>,
    folding_range_decoration_ids: RefCell<Vec<String>>,
}

impl FoldingModel {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            view_model: RefCell::new(None),
            view_model_store: DisposableStore::new(),
            regions: RefCell::new(Rc::new(FoldingRegions::new(Vec::new(), Vec::new(), None))),
            on_did_folding_region_changes: Emitter::new(),
            folding_range_decoration_ids: RefCell::new(Vec::new()),
        })
    }

    pub fn regions(&self) -> Rc<FoldingRegions> {
        self.regions.borrow().clone()
    }

    pub fn on_did_folding_region_changed(&self) -> Event<()> {
        self.on_did_folding_region_changes.event()
    }

    pub fn detach_view_model(&self) {
        self.view_model_store.clear();
        *self.view_model.borrow_mut() = None;
    }

    pub fn attach_view_model(self: &Rc<Self>, model: Rc<NotebookViewModel>) {
        *self.view_model.borrow_mut() = Some(model.clone());

        {
            let weak = Rc::downgrade(self);
            self.view_model_store.add(model.on_did_change_view_cells().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.recompute();
                }
            }));
        }

        {
            let weak = Rc::downgrade(self);
            self.view_model_store.add(model.on_did_change_selection().listen(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let Some(view_model) = this.view_model.borrow().clone() else { return };

                let indexes = cell_ranges_to_indexes(&view_model.get_selections());

                let mut changed = false;

                for index in indexes {
                    let regions = this.regions();
                    let mut region_index = regions.find_range((index + 1) as u32);

                    while region_index != -1 {
                        let current = region_index as usize;
                        if regions.is_collapsed(current) && index > regions.get_start_line_number(current) as i64 - 1 {
                            regions.set_collapsed(current, false);
                            changed = true;
                        }
                        region_index = regions.get_parent_index(current);
                    }
                }

                if changed {
                    this.on_did_folding_region_changes.fire(());
                }
            }));
        }

        self.recompute();
    }

    pub fn get_region_at_line(&self, line_number: u32) -> Option<FoldingRegion> {
        let regions = self.regions();
        let index = regions.find_range(line_number);
        if index >= 0 {
            return Some(regions.to_region(index as usize));
        }
        None
    }

    pub fn get_regions_inside(&self, region: Option<&FoldingRegion>, filter: Option<&dyn Fn(&FoldingRegion) -> bool>) -> Vec<FoldingRegion> {
        let regions = self.regions();
        let mut result = Vec::new();
        let index = region.map(|r| r.region_index() + 1).unwrap_or(0);
        let end_line_number = region.map(|r| r.end_line_number()).unwrap_or(u32::MAX);

        for i in index..regions.length() {
            let current = regions.to_region(i);
            if regions.get_start_line_number(i) < end_line_number {
                if filter.is_none_or(|filter| filter(&current)) {
                    result.push(current);
                }
            } else {
                break;
            }
        }
        result
    }

    /// `getRegionsInside(region, filter)` com o filtro de dois parâmetros (região e nível).
    pub fn get_regions_inside_with_level(&self, region: Option<&FoldingRegion>, filter: &dyn Fn(&FoldingRegion, usize) -> bool) -> Vec<FoldingRegion> {
        let regions = self.regions();
        let mut result = Vec::new();
        let index = region.map(|r| r.region_index() + 1).unwrap_or(0);
        let end_line_number = region.map(|r| r.end_line_number()).unwrap_or(u32::MAX);

        let mut level_stack: Vec<FoldingRegion> = Vec::new();
        for i in index..regions.length() {
            let current = regions.to_region(i);
            if regions.get_start_line_number(i) < end_line_number {
                while let Some(top) = level_stack.last() {
                    if current.contained_by(ILineRange { start_line_number: top.start_line_number(), end_line_number: top.end_line_number() }) {
                        break;
                    }
                    level_stack.pop();
                }
                level_stack.push(current.clone());
                if filter(&current, level_stack.len()) {
                    result.push(current);
                }
            } else {
                break;
            }
        }
        result
    }

    pub fn get_all_regions_at_line(&self, line_number: u32, filter: Option<&dyn Fn(&FoldingRegion, usize) -> bool>) -> Vec<FoldingRegion> {
        let regions = self.regions();
        let mut result = Vec::new();
        let mut index = regions.find_range(line_number);
        let mut level = 1;
        while index >= 0 {
            let current = regions.to_region(index as usize);
            if filter.is_none_or(|filter| filter(&current, level)) {
                result.push(current.clone());
            }
            level += 1;
            index = current.parent_index();
        }
        result
    }

    pub fn set_collapsed(&self, index: i32, new_state: bool) {
        self.regions().set_collapsed(index as usize, new_state);
    }

    pub fn recompute(&self) {
        let Some(view_model) = self.view_model.borrow().clone() else { return };

        let cells = view_model.view_cells();
        struct StackEntry {
            index: usize,
            level: u8,
        }
        let mut stack: Vec<StackEntry> = Vec::new();

        for (i, cell) in cells.iter().enumerate() {
            if cell.cell_kind() != CellKind::Markup || cell.language() != "markdown" {
                continue;
            }

            let min_depth = get_markdown_headers_in_cell(&cell.get_text()).iter().map(|header| header.depth).fold(7, u8::min);
            if min_depth < 7 {
                // header 1 to 6
                stack.push(StackEntry { index: i, level: min_depth });
            }
        }

        // calculate folding ranges
        let raw_folding_ranges: Vec<FoldingRangeData> = stack
            .iter()
            .enumerate()
            .map(|(start_index, entry)| {
                let mut end: Option<i32> = None;
                for later in &stack[start_index + 1..] {
                    if later.level <= entry.level {
                        end = Some(later.index as i32 - 1);
                        break;
                    }
                }

                let end_index = end.unwrap_or(cells.len() as i32 - 1);

                // one based
                FoldingRangeData { start: entry.index as i32 + 1, end: end_index + 1, kind: None, rank: 1 }
            })
            .filter(|range| range.start != range.end)
            .collect();

        let new_regions = sanitize_ranges(raw_folding_ranges, &FoldingRangeLimit);

        // restore collased state
        let old_regions = self.regions();
        let mut i = 0;
        let mut next_collapsed = || -> i64 {
            while i < old_regions.length() {
                let is_collapsed = old_regions.is_collapsed(i);
                i += 1;
                if is_collapsed {
                    return i as i64 - 1;
                }
            }
            -1
        };

        let mut k = 0;
        let mut collapsed_index = next_collapsed();

        while collapsed_index != -1 && k < new_regions.length() {
            // get the latest range
            let dec_range = {
                let ids = self.folding_range_decoration_ids.borrow();
                ids.get(collapsed_index as usize).and_then(|id| view_model.get_tracked_range(id))
            };
            if let Some(dec_range) = dec_range {
                let collasped_start_index = dec_range.start;

                while k < new_regions.length() {
                    let start_index = new_regions.get_start_line_number(k) as i64 - 1;
                    if collasped_start_index >= start_index {
                        new_regions.set_collapsed(k, collasped_start_index == start_index);
                        k += 1;
                    } else {
                        break;
                    }
                }
            }
            collapsed_index = next_collapsed();
        }

        while k < new_regions.length() {
            new_regions.set_collapsed(k, false);
            k += 1;
        }

        let mut cell_ranges: Vec<ICellRange> = Vec::new();
        let new_regions = Rc::new(new_regions);
        for i in 0..new_regions.length() {
            let region = new_regions.to_region(i);
            cell_ranges.push(ICellRange { start: region.start_line_number() as i64 - 1, end: region.end_line_number() as i64 - 1 });
        }

        // remove old tracked ranges and add new ones
        // TODO@rebornix, implement delta
        for id in self.folding_range_decoration_ids.borrow().iter() {
            view_model.set_tracked_range(Some(id.as_str()), None, TrackedRangeStickiness::GrowsOnlyWhenTypingAfter);
        }
        *self.folding_range_decoration_ids.borrow_mut() = cell_ranges
            .iter()
            .filter_map(|region| view_model.set_tracked_range(None, Some(*region), TrackedRangeStickiness::GrowsOnlyWhenTypingAfter))
            .collect();

        *self.regions.borrow_mut() = new_regions;
        self.on_did_folding_region_changes.fire(());
    }

    pub fn get_memento(&self) -> Vec<ICellRange> {
        let regions = self.regions();
        let mut collapsed_ranges = Vec::new();
        for i in 0..regions.length() {
            if regions.is_collapsed(i) {
                let region = regions.to_region(i);
                collapsed_ranges.push(ICellRange { start: region.start_line_number() as i64 - 1, end: region.end_line_number() as i64 - 1 });
            }
        }

        collapsed_ranges
    }

    pub fn apply_memento(&self, state: &[ICellRange]) -> bool {
        let Some(view_model) = self.view_model.borrow().clone() else { return false };
        let regions = self.regions();

        let mut i = 0;
        let mut k = 0;

        while k < state.len() && i < regions.length() {
            // get the latest range
            let dec_range = self.folding_range_decoration_ids.borrow().get(i).and_then(|id| view_model.get_tracked_range(id));
            if dec_range.is_some() {
                let collasped_start_index = state[k].start;

                while i < regions.length() {
                    let start_index = regions.get_start_line_number(i) as i64 - 1;
                    if collasped_start_index >= start_index {
                        regions.set_collapsed(i, collasped_start_index == start_index);
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            k += 1;
        }

        while i < regions.length() {
            regions.set_collapsed(i, false);
            i += 1;
        }

        true
    }
}

impl IDisposable for FoldingModel {
    fn dispose(&self) {
        self.on_did_folding_region_changes.dispose();
        self.view_model_store.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FoldingModel"
    }
}

/// `updateFoldingStateAtIndex(foldingModel, index, collapsed)`.
pub fn update_folding_state_at_index(folding_model: &FoldingModel, index: i64, collapsed: bool) {
    let range = folding_model.regions().find_range((index + 1) as u32);
    folding_model.set_collapsed(range, collapsed);
}

/// Um cabeçalho de markdown (`{ depth, text }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownHeader {
    pub depth: u8,
    pub text: String,
}

/// `getMarkdownHeadersInCell(cellContent)`.
pub fn get_markdown_headers_in_cell(cell_content: &str) -> Vec<MarkdownHeader> {
    let source = JsString::from(cell_content);
    let Ok(tokens) = lexer_with(source.units(), Options { gfm: true, ..Options::default() }) else {
        return Vec::new();
    };
    tokens
        .tokens
        .iter()
        .filter_map(|token| match token {
            Token::Heading(heading) => Some(MarkdownHeader { depth: heading.depth, text: render_as_plaintext(&heading.raw.to_string_lossy()).trim().to_string() }),
            _ => None,
        })
        .collect()
}
