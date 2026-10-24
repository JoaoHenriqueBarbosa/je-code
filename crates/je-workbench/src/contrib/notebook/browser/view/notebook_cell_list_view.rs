//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/view/notebookCellListView.ts`: só o
//! `NotebookCellsLayout` (o `IRangeMap` da lista de células, com o espaço dos view zones) e o `IWhitespace`. O
//! `NotebookCellListView` (a subclasse de `ListView`, com DOM) é widget de notebook e fica para a Fase 6.
//!
//! Divergências:
//! - Os tamanhos são `f64` (o `number` do original). Os valores do `ConstantTimePrefixSumComputer`, que no original é um
//!   `Uint32Array`, são `u32`: o tamanho inteiro (com o espaço dos view zones) é truncado, como a conversão do array
//!   tipado (para tamanhos negativos o JavaScript dá a volta, o porte satura em 0; nenhum chamador passa negativo).
//! - `splice(index, deleteCount, items?)` é [`IRangeMap::splice`] com uma fatia (a ausência de `items` é a fatia
//!   vazia); `get paddingTop`/`set paddingTop` são [`IRangeMap::padding_top`] e [`IRangeMap::set_padding_top`]. Os índices
//!   que podem ser `-1` (`indexAt`, `positionAt`) são `i64`; `afterPosition` é `usize` e `priority` é `i64`.
//! - `getWhitespacePosition` de um id que não existe lança (`errors::throw`), como o `throw new Error`.
//! - `Array.prototype.splice` limita o índice e a contagem ao tamanho do array; o porte limita igual.
//! - `protected _prefixSumComputer` (lido pelas subclasses) não tem acessor: nenhuma subclasse foi portada.

use je_base::errors::{self, Error};
use je_editor::prefix_sum_computer::ConstantTimePrefixSumComputer;
use je_widgets::list::range_map::{IItem, IRangeMap};

/// `IWhitespace`.
#[derive(Clone, Debug, PartialEq)]
pub struct IWhitespace {
    pub id: String,
    /// To insert whitespace before the first item, use afterPosition 0.
    /// In other cases, afterPosition is 1-based.
    pub after_position: usize,
    pub size: f64,
    pub priority: i64,
}

/// `NotebookCellsLayout`.
pub struct NotebookCellsLayout {
    items: Vec<IItem>,
    whitespace: Vec<IWhitespace>,
    prefix_sum_computer: ConstantTimePrefixSumComputer,
    size: f64,
    padding_top: f64,
}

impl NotebookCellsLayout {
    /// `new NotebookCellsLayout(topPadding?)`.
    pub fn new(top_padding: Option<f64>) -> Self {
        let padding_top = top_padding.unwrap_or(0.0);
        Self { items: Vec::new(), whitespace: Vec::new(), prefix_sum_computer: ConstantTimePrefixSumComputer::new(Vec::new()), size: padding_top, padding_top }
    }

    pub fn get_whitespaces(&self) -> &[IWhitespace] {
        &self.whitespace
    }

    /// `this.items[index].size` do `ListView` (o tamanho do item, sem o espaço dos view zones depois dele).
    pub fn item_size(&self, index: usize) -> f64 {
        self.items[index].size
    }

    pub fn restore_whitespace(&mut self, items: Vec<IWhitespace>) {
        self.whitespace = items;
        self.size = self.total_size();
    }

    /// `this._paddingTop + items + whitespace`.
    fn total_size(&self) -> f64 {
        self.padding_top + self.items.iter().map(|item| item.size).sum::<f64>() + self.whitespace.iter().map(|ws| ws.size).sum::<f64>()
    }

    /// A soma dos tamanhos dos espaços `afterPosition === after_position`.
    fn whitespace_size_after(&self, after_position: usize) -> f64 {
        self.whitespace.iter().filter(|ws| ws.after_position == after_position).map(|ws| ws.size).sum()
    }

    /// O tamanho do item `index` somado ao dos espaços que vêm logo depois dele (`afterPosition === index + 1`).
    fn item_size_with_whitespace(&self, index: usize) -> f64 {
        self.items[index].size + self.whitespace_size_after(index + 1)
    }

    /// `whitespace.filter(ws => ws.afterPosition === 0).reduce(...)`.
    fn whitespace_before_first_item(&self) -> f64 {
        self.whitespace_size_after(0)
    }

    pub fn insert_whitespace(&mut self, id: &str, after_position: usize, size: f64) {
        let mut priority = 0;
        let existing_priority = self.whitespace.iter().filter(|ws| ws.after_position == after_position).map(|ws| ws.priority).max();
        if let Some(max) = existing_priority {
            priority = max + 1;
        }

        self.whitespace.push(IWhitespace { id: id.to_owned(), after_position, size, priority });
        self.size += size; // Update the total size to include the whitespace
        self.whitespace.sort_by(|a, b| a.after_position.cmp(&b.after_position).then(a.priority.cmp(&b.priority)));

        // find item size of index
        if after_position > 0 {
            let index = after_position - 1;
            let item_size = self.items[index].size;
            let acc_size = item_size + size;
            self.prefix_sum_computer.set_value(index, acc_size as u32);
        }
    }

    pub fn change_one_whitespace(&mut self, id: &str, after_position: usize, size: f64) {
        let Some(whitespace_index) = self.whitespace.iter().position(|ws| ws.id == id) else {
            return;
        };
        let whitespace = &mut self.whitespace[whitespace_index];
        let old_after_position = whitespace.after_position;
        whitespace.after_position = after_position;
        let old_size = whitespace.size;
        let delta = size - old_size;
        whitespace.size = size;
        self.size += delta;

        if old_after_position > 0 && old_after_position <= self.items.len() {
            let index = old_after_position - 1;
            let item_size = self.items[index].size;
            let acc_size = item_size;
            self.prefix_sum_computer.set_value(index, acc_size as u32);
        }

        if after_position > 0 && after_position <= self.items.len() {
            let index = after_position - 1;
            let item_size = self.items[index].size;
            let acc_size = item_size + size;
            self.prefix_sum_computer.set_value(index, acc_size as u32);
        }
    }

    pub fn remove_whitespace(&mut self, id: &str) {
        let Some(whitespace_index) = self.whitespace.iter().position(|ws| ws.id == id) else {
            return;
        };
        let whitespace = self.whitespace.remove(whitespace_index);
        self.size -= whitespace.size; // Reduce the total size by the size of the removed whitespace

        if whitespace.after_position > 0 {
            let index = whitespace.after_position - 1;
            let item_size = self.items[index].size;
            let acc_size = item_size + self.whitespace_size_after(whitespace.after_position);
            self.prefix_sum_computer.set_value(index, acc_size as u32);
        }
    }

    /// find position of whitespace
    /// - `id`: id of the whitespace
    /// - devolve: position in the list view
    pub fn get_whitespace_position(&self, id: &str) -> f64 {
        let Some(whitespace) = self.whitespace.iter().find(|ws| ws.id == id) else {
            errors::throw(Error::new("Whitespace not found"));
        };

        let after_position = whitespace.after_position;
        if after_position == 0 {
            // find all whitespaces at the same position but with higher priority (smaller number)
            let whitespaces_size: f64 = self.whitespace.iter().filter(|ws| ws.after_position == after_position && ws.priority < whitespace.priority).map(|ws| ws.size).sum();
            return whitespaces_size + self.padding_top;
        }

        let whitespace_before_first_item = self.whitespace_before_first_item();

        // previous item index
        let index = after_position - 1;
        let previous_item_position = self.prefix_sum_computer.get_prefix_sum(index);
        let previous_item_size = self.items[index].size;
        previous_item_position as f64 + previous_item_size + whitespace_before_first_item + self.padding_top
    }
}

impl IRangeMap for NotebookCellsLayout {
    fn padding_top(&self) -> f64 {
        self.padding_top
    }

    fn set_padding_top(&mut self, padding_top: f64) {
        self.size = self.size + padding_top - self.padding_top;
        self.padding_top = padding_top;
    }

    fn count(&self) -> usize {
        self.items.len()
    }

    /// Returns the sum of the sizes of all items in the range map.
    fn size(&self) -> f64 {
        self.size
    }

    fn splice(&mut self, index: usize, delete_count: usize, inserts: &[IItem]) {
        // Perform the splice operation on the items array.
        let start = index.min(self.items.len());
        let end = start.saturating_add(delete_count).min(self.items.len());
        drop(self.items.splice(start..end, inserts.iter().copied()));

        self.size = self.total_size();
        self.prefix_sum_computer.remove_values(index, delete_count);

        // inserts should also include whitespaces
        let new_sizes: Vec<u32> = inserts
            .iter()
            .enumerate()
            .map(|(i, insert)| {
                let insert_index = i + index;
                (insert.size + self.whitespace_size_after(insert_index + 1)) as u32
            })
            .collect();
        self.prefix_sum_computer.insert_values(index, &new_sizes);

        // Now that the items array has been updated, and the whitespaces are updated elsewhere, if an item is removed/inserted, the accumlated size of the items are all updated.
        // Loop through all items from the index where the splice started, to the end
        for i in index..self.items.len() {
            let size = self.item_size_with_whitespace(i);
            self.prefix_sum_computer.set_value(i, size as u32);
        }
    }

    fn index_at(&self, position: f64) -> i64 {
        if position < 0.0 {
            return -1;
        }

        let whitespace_before_first_item = self.whitespace_before_first_item();

        let offset = position - (self.padding_top + whitespace_before_first_item);
        if offset <= 0.0 {
            return 0;
        }

        if offset >= self.size - self.padding_top - whitespace_before_first_item {
            return self.count() as i64;
        }

        self.prefix_sum_computer.get_index_of(offset.trunc() as i64).index as i64
    }

    fn index_after(&self, position: f64) -> i64 {
        let index = self.index_at(position);
        (index + 1).min(self.items.len() as i64)
    }

    fn position_at(&self, index: i64) -> f64 {
        if index < 0 {
            return -1.0;
        }

        if self.count() == 0 {
            return -1.0;
        }

        // index is zero based, if index+1 > this.count, then it points to the fictitious element after the last element of this array.
        if index >= self.count() as i64 {
            return -1.0;
        }

        let whitespace_before_first_item = self.whitespace_before_first_item();
        self.prefix_sum_computer.get_prefix_sum(index as usize /* count */) as f64 + self.padding_top + whitespace_before_first_item
    }
}
