//! Porte de `src/vs/editor/common/viewLayout/lineHeights.ts`.
//!
//! Representação:
//! - Alturas e somas de prefixo são `f64` (o `number` do JavaScript, pois `lineHeight * defaultLineHeight` pode
//!   ser fracionário); números de linha são `i32`, como no resto do crate.
//! - No original, `_decorationIDToCustomLine` e `_orderedCustomLines` (e os `staged*` do `_commit`) guardam os
//!   MESMOS objetos `CustomLine`, e uma mutação num é vista nos outros. Aqui cada linha é um
//!   `Rc<RefCell<CustomLine>>` compartilhado pelas coleções, o que preserva essa identidade.
//! - `Infinity` do `_invalidIndex` é `usize::MAX`; o `index` de uma linha ainda não gravada (`-1`) é `-1_i64`.
//! - `intersection` do `collections.ts` recebe `Set`s: os dois conjuntos de decorações viram vetores sem
//!   repetição na ordem de inserção, que é a ordem que o `Set` do JavaScript itera.
//! - O setter e o getter `defaultLineHeight` são [`LineHeightsManager::set_default_line_height`] e
//!   [`LineHeightsManager::default_line_height`]. As consultas (`heightForLineNumber`,
//!   `getAccumulatedLineHeightsIncludingLineNumber`) fazem o `_commit` e por isso pedem `&mut self`.
//!
//! Pendente: `CustomLineHeightData.fromDecorations`, que depende de `IModelDecoration` (opções com `lineHeight`),
//! de `ICoordinatesConverter` e de `IEditorConfiguration`/`EditorOption.lineHeight`, ainda não portados.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::arrays::binary_search2;
use je_base::collections::intersection;

/// `Math.round` do JavaScript: arredonda para o inteiro mais próximo, e o meio para cima (`+Infinity`).
fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 { floor + 1.0 } else { floor }
}

const INFINITY_INDEX: usize = usize::MAX;

/// `PendingChange` (com o `PendingChangeKind` embutido nas variantes).
enum PendingChange {
    InsertOrChange { decoration_id: String, start_line_number: i32, end_line_number: i32, line_height: f64 },
    Remove { decoration_id: String },
    LinesDeleted { from_line_number: i32, to_line_number: i32 },
    LinesInserted { from_line_number: i32, to_line_number: i32 },
}

/// `CustomLine`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomLine {
    pub index: i64,
    pub line_number: i32,
    pub special_height: f64,
    pub prefix_sum: f64,
    pub maximum_special_height: f64,
    pub decoration_id: String,
    pub deleted: bool,
}

impl CustomLine {
    pub fn new(decoration_id: &str, index: i64, line_number: i32, special_height: f64, prefix_sum: f64) -> Self {
        Self {
            decoration_id: decoration_id.to_string(),
            index,
            line_number,
            special_height: js_round(special_height),
            prefix_sum,
            maximum_special_height: js_round(special_height),
            deleted: false,
        }
    }
}

type CustomLineRef = Rc<RefCell<CustomLine>>;

/// `ArrayMap<K, T>`: um mapa de chave para a lista de valores acrescentados a ela.
struct ArrayMap<K, T> {
    map: HashMap<K, Vec<T>>,
}

impl<K: std::hash::Hash + Eq, T> ArrayMap<K, T> {
    fn new() -> Self {
        Self { map: HashMap::new() }
    }

    fn add(&mut self, key: K, value: T) {
        self.map.entry(key).or_default().push(value);
    }

    fn get(&self, key: &K) -> Option<&Vec<T>> {
        self.map.get(key)
    }

    fn delete(&mut self, key: &K) {
        self.map.remove(key);
    }

    fn clear(&mut self) {
        self.map.clear();
    }
}

/// `LineHeightsManager`: as alturas de linha do editor, com alturas customizadas vindas de decorações.
///
/// Cada linha com altura customizada é uma [`CustomLine`] num vetor ordenado por número de linha (busca binária),
/// com a soma de prefixo para consultar a altura acumulada em O(log n). As mudanças ficam pendentes e são
/// aplicadas em lote (`_commit`) antes de qualquer consulta.
pub struct LineHeightsManager {
    decoration_id_to_custom_line: ArrayMap<String, CustomLineRef>,
    ordered_custom_lines: Vec<CustomLineRef>,
    pending_changes: Vec<PendingChange>,
    invalid_index: usize,
    default_line_height: f64,
    has_pending: bool,
}

impl LineHeightsManager {
    pub fn new(default_line_height: f64, custom_line_height_data: &[CustomLineHeightData]) -> Self {
        let mut manager = Self {
            decoration_id_to_custom_line: ArrayMap::new(),
            ordered_custom_lines: Vec::new(),
            pending_changes: Vec::new(),
            invalid_index: INFINITY_INDEX,
            default_line_height,
            has_pending: false,
        };
        for data in custom_line_height_data {
            manager.insert_or_change_custom_line_height(
                &data.decoration_id,
                data.start_line_number,
                data.end_line_number,
                data.line_height,
            );
        }
        manager
    }

    pub fn set_default_line_height(&mut self, default_line_height: f64) {
        self.default_line_height = default_line_height;
    }

    pub fn default_line_height(&self) -> f64 {
        self.default_line_height
    }

    pub fn remove_custom_line_height(&mut self, decoration_id: &str) {
        self.pending_changes.push(PendingChange::Remove { decoration_id: decoration_id.to_string() });
        self.has_pending = true;
    }

    pub fn insert_or_change_custom_line_height(
        &mut self,
        decoration_id: &str,
        start_line_number: i32,
        end_line_number: i32,
        line_height: f64,
    ) {
        self.pending_changes.push(PendingChange::InsertOrChange {
            decoration_id: decoration_id.to_string(),
            start_line_number,
            end_line_number,
            line_height,
        });
        self.has_pending = true;
    }

    pub fn height_for_line_number(&mut self, line_number: i32) -> f64 {
        self.commit();
        let search_index = self.binary_search_over_ordered_custom_lines_array(line_number);
        if search_index >= 0 {
            return self.ordered_custom_lines[search_index as usize].borrow().maximum_special_height;
        }
        self.default_line_height
    }

    pub fn get_accumulated_line_heights_including_line_number(&mut self, line_number: i32) -> f64 {
        self.commit();
        let search_index = self.binary_search_over_ordered_custom_lines_array(line_number);
        if search_index >= 0 {
            let line = self.ordered_custom_lines[search_index as usize].borrow();
            return line.prefix_sum + line.maximum_special_height;
        }
        if search_index == -1 {
            return self.default_line_height * f64::from(line_number);
        }
        let modified_index = (-(search_index + 1)) as usize;
        let previous_special_line = self.ordered_custom_lines[modified_index - 1].borrow();
        previous_special_line.prefix_sum
            + previous_special_line.maximum_special_height
            + self.default_line_height * f64::from(line_number - previous_special_line.line_number)
    }

    pub fn on_lines_deleted(&mut self, from_line_number: i32, to_line_number: i32) {
        self.pending_changes.push(PendingChange::LinesDeleted { from_line_number, to_line_number });
        self.has_pending = true;
    }

    pub fn on_lines_inserted(&mut self, from_line_number: i32, to_line_number: i32) {
        self.pending_changes.push(PendingChange::LinesInserted { from_line_number, to_line_number });
        self.has_pending = true;
    }

    /// `_commit`: aplica as mudanças pendentes na ordem em que chegaram. Uma mudança de linhas descarrega
    /// antes as inserções de decoração em estágio, porque elas valem para a numeração anterior.
    fn commit(&mut self) {
        if !self.has_pending {
            return;
        }
        let changes = std::mem::take(&mut self.pending_changes);
        self.has_pending = false;

        let mut staged_inserts: Vec<CustomLineRef> = Vec::new();
        let mut staged_id_map: ArrayMap<String, CustomLineRef> = ArrayMap::new();
        for change in changes {
            match change {
                PendingChange::Remove { decoration_id } => {
                    self.do_remove_custom_line_height(&decoration_id, &mut staged_id_map);
                }
                PendingChange::InsertOrChange { decoration_id, start_line_number, end_line_number, line_height } => {
                    self.do_insert_or_change_custom_line_height(
                        &decoration_id,
                        start_line_number,
                        end_line_number,
                        line_height,
                        &mut staged_inserts,
                        &mut staged_id_map,
                    );
                }
                PendingChange::LinesDeleted { from_line_number, to_line_number } => {
                    self.flush_staged_decoration_changes(&mut staged_inserts, &mut staged_id_map);
                    self.do_lines_deleted(from_line_number, to_line_number);
                }
                PendingChange::LinesInserted { from_line_number, to_line_number } => {
                    self.flush_staged_decoration_changes(&mut staged_inserts, &mut staged_id_map);
                    self.do_lines_inserted(from_line_number, to_line_number, &mut staged_inserts, &mut staged_id_map);
                }
            }
        }
        self.flush_staged_decoration_changes(&mut staged_inserts, &mut staged_id_map);
    }

    fn do_remove_custom_line_height(&mut self, decoration_id: &str, staged_id_map: &mut ArrayMap<String, CustomLineRef>) {
        let key = decoration_id.to_string();
        if let Some(custom_lines) = self.decoration_id_to_custom_line.get(&key).cloned() {
            self.decoration_id_to_custom_line.delete(&key);
            for custom_line in custom_lines {
                let mut line = custom_line.borrow_mut();
                line.deleted = true;
                self.invalid_index = self.invalid_index.min(line.index as usize);
            }
        }
        if let Some(staged_lines) = staged_id_map.get(&key).cloned() {
            staged_id_map.delete(&key);
            for line in staged_lines {
                line.borrow_mut().deleted = true;
            }
        }
    }

    fn do_insert_or_change_custom_line_height(
        &mut self,
        decoration_id: &str,
        start_line_number: i32,
        end_line_number: i32,
        line_height: f64,
        staged_inserts: &mut Vec<CustomLineRef>,
        staged_id_map: &mut ArrayMap<String, CustomLineRef>,
    ) {
        self.do_remove_custom_line_height(decoration_id, staged_id_map);
        for line_number in start_line_number..=end_line_number {
            let custom_line = Rc::new(RefCell::new(CustomLine::new(decoration_id, -1, line_number, line_height, 0.0)));
            staged_inserts.push(Rc::clone(&custom_line));
            staged_id_map.add(decoration_id.to_string(), custom_line);
        }
    }

    fn flush_staged_decoration_changes(
        &mut self,
        staged_inserts: &mut Vec<CustomLineRef>,
        staged_id_map: &mut ArrayMap<String, CustomLineRef>,
    ) {
        if staged_inserts.is_empty() && self.invalid_index == INFINITY_INDEX {
            return;
        }
        for pending_change in staged_inserts.iter() {
            if pending_change.borrow().deleted {
                continue;
            }
            let line_number = pending_change.borrow().line_number;
            let candidate_insertion_index = self.binary_search_over_ordered_custom_lines_array(line_number);
            let insertion_index = if candidate_insertion_index >= 0 {
                candidate_insertion_index
            } else {
                -(candidate_insertion_index + 1)
            } as usize;
            self.ordered_custom_lines.insert(insertion_index, Rc::clone(pending_change));
            self.invalid_index = self.invalid_index.min(insertion_index);
        }
        staged_inserts.clear();
        staged_id_map.clear();
        if self.invalid_index == INFINITY_INDEX {
            return;
        }
        let mut new_decoration_id_to_special_line: ArrayMap<String, CustomLineRef> = ArrayMap::new();
        let mut new_ordered_special_lines: Vec<CustomLineRef> = Vec::new();

        for custom_line in &self.ordered_custom_lines[..self.invalid_index] {
            new_ordered_special_lines.push(Rc::clone(custom_line));
            new_decoration_id_to_special_line.add(custom_line.borrow().decoration_id.clone(), Rc::clone(custom_line));
        }

        let mut number_of_deletions = 0usize;
        let mut previous_special_line: Option<CustomLineRef> =
            if self.invalid_index > 0 { Some(Rc::clone(&new_ordered_special_lines[self.invalid_index - 1])) } else { None };
        for i in self.invalid_index..self.ordered_custom_lines.len() {
            let custom_line = Rc::clone(&self.ordered_custom_lines[i]);
            if custom_line.borrow().deleted {
                number_of_deletions += 1;
                continue;
            }
            custom_line.borrow_mut().index = (i - number_of_deletions) as i64;
            let line_number = custom_line.borrow().line_number;
            let same_line_previous = previous_special_line.as_ref().filter(|p| p.borrow().line_number == line_number);
            if let Some(previous) = same_line_previous {
                let (maximum_special_height, prefix_sum) = {
                    let previous = previous.borrow();
                    (previous.maximum_special_height, previous.prefix_sum)
                };
                let mut line = custom_line.borrow_mut();
                line.maximum_special_height = maximum_special_height;
                line.prefix_sum = prefix_sum;
            } else {
                let mut maximum_special_height = custom_line.borrow().special_height;
                for next_special_line in &self.ordered_custom_lines[i..] {
                    let next = next_special_line.borrow();
                    if next.deleted {
                        continue;
                    }
                    if next.line_number != line_number {
                        break;
                    }
                    maximum_special_height = maximum_special_height.max(next.special_height);
                }
                custom_line.borrow_mut().maximum_special_height = maximum_special_height;

                let prefix_sum = match &previous_special_line {
                    Some(previous) => {
                        let previous = previous.borrow();
                        previous.prefix_sum
                            + previous.maximum_special_height
                            + self.default_line_height * f64::from(line_number - previous.line_number - 1)
                    }
                    None => self.default_line_height * f64::from(line_number - 1),
                };
                custom_line.borrow_mut().prefix_sum = prefix_sum;
            }
            previous_special_line = Some(Rc::clone(&custom_line));
            new_ordered_special_lines.push(Rc::clone(&custom_line));
            let decoration_id = custom_line.borrow().decoration_id.clone();
            new_decoration_id_to_special_line.add(decoration_id, custom_line);
        }
        self.ordered_custom_lines = new_ordered_special_lines;
        self.decoration_id_to_custom_line = new_decoration_id_to_special_line;
        self.invalid_index = INFINITY_INDEX;
    }

    fn do_lines_deleted(&mut self, from_line_number: i32, to_line_number: i32) {
        let delete_count = to_line_number - from_line_number + 1;
        let number_of_custom_lines = self.ordered_custom_lines.len();
        let line_number_at = |lines: &Vec<CustomLineRef>, i: usize| lines[i].borrow().line_number;

        let candidate_start_index_of_deletion = self.binary_search_over_ordered_custom_lines_array(from_line_number);
        let start_index_of_deletion: usize = if candidate_start_index_of_deletion >= 0 {
            let mut start = candidate_start_index_of_deletion as usize;
            let mut i = candidate_start_index_of_deletion - 1;
            while i >= 0 {
                if line_number_at(&self.ordered_custom_lines, i as usize) == from_line_number {
                    start -= 1;
                } else {
                    break;
                }
                i -= 1;
            }
            start
        } else if candidate_start_index_of_deletion == -(number_of_custom_lines as i64 + 1)
            && candidate_start_index_of_deletion != -1
        {
            number_of_custom_lines - 1
        } else {
            (-(candidate_start_index_of_deletion + 1)) as usize
        };
        let candidate_end_index_of_deletion = self.binary_search_over_ordered_custom_lines_array(to_line_number);
        let end_index_of_deletion: usize = if candidate_end_index_of_deletion >= 0 {
            let mut end = candidate_end_index_of_deletion as usize;
            for i in (candidate_end_index_of_deletion as usize + 1)..number_of_custom_lines {
                if line_number_at(&self.ordered_custom_lines, i) == to_line_number {
                    end += 1;
                } else {
                    break;
                }
            }
            end
        } else if candidate_end_index_of_deletion == -(number_of_custom_lines as i64 + 1)
            && candidate_end_index_of_deletion != -1
        {
            number_of_custom_lines - 1
        } else {
            (-(candidate_end_index_of_deletion + 1)) as usize
        };
        let is_end_index_bigger_than_start_index = end_index_of_deletion > start_index_of_deletion;
        let is_end_index_equal_to_start_index_and_covers_custom_line = end_index_of_deletion == start_index_of_deletion
            && self.ordered_custom_lines.get(start_index_of_deletion).is_some_and(|line| {
                let line = line.borrow();
                line.line_number >= from_line_number && line.line_number <= to_line_number
            });

        if is_end_index_bigger_than_start_index || is_end_index_equal_to_start_index_and_covers_custom_line {
            let mut maximum_special_height_on_deleted_interval = 0.0_f64;
            for i in start_index_of_deletion..=end_index_of_deletion {
                maximum_special_height_on_deleted_interval =
                    maximum_special_height_on_deleted_interval.max(self.ordered_custom_lines[i].borrow().maximum_special_height);
            }
            let prefix_sum_on_deleted_interval = if start_index_of_deletion > 0 {
                let previous_special_line = self.ordered_custom_lines[start_index_of_deletion - 1].borrow();
                previous_special_line.prefix_sum
                    + previous_special_line.maximum_special_height
                    + self.default_line_height * f64::from(from_line_number - previous_special_line.line_number - 1)
            } else if from_line_number > 0 {
                f64::from(from_line_number - 1) * self.default_line_height
            } else {
                0.0
            };
            let total_height_deleted = {
                let first_special_line_deleted = self.ordered_custom_lines[start_index_of_deletion].borrow();
                let last_special_line_deleted = self.ordered_custom_lines[end_index_of_deletion].borrow();
                let height_of_first_line_after_deletion = match self.ordered_custom_lines.get(end_index_of_deletion + 1) {
                    Some(line) if line.borrow().line_number == to_line_number + 1 => line.borrow().maximum_special_height,
                    _ => self.default_line_height,
                };
                last_special_line_deleted.prefix_sum + last_special_line_deleted.maximum_special_height
                    - first_special_line_deleted.prefix_sum
                    + self.default_line_height * f64::from(to_line_number - last_special_line_deleted.line_number)
                    + self.default_line_height * f64::from(first_special_line_deleted.line_number - from_line_number)
                    + height_of_first_line_after_deletion
                    - maximum_special_height_on_deleted_interval
            };

            let mut decoration_ids_seen: HashSet<String> = HashSet::new();
            let mut new_ordered_custom_lines: Vec<CustomLineRef> = Vec::new();
            let mut new_decoration_id_to_special_line: ArrayMap<String, CustomLineRef> = ArrayMap::new();
            let mut number_of_deletions = 0i64;
            for i in 0..self.ordered_custom_lines.len() {
                let custom_line = Rc::clone(&self.ordered_custom_lines[i]);
                if i < start_index_of_deletion {
                    new_ordered_custom_lines.push(Rc::clone(&custom_line));
                    new_decoration_id_to_special_line.add(custom_line.borrow().decoration_id.clone(), Rc::clone(&custom_line));
                } else if i <= end_index_of_deletion {
                    let decoration_id = custom_line.borrow().decoration_id.clone();
                    if !decoration_ids_seen.contains(&decoration_id) {
                        {
                            let mut line = custom_line.borrow_mut();
                            line.index -= number_of_deletions;
                            line.line_number = from_line_number;
                            line.prefix_sum = prefix_sum_on_deleted_interval;
                            line.maximum_special_height = maximum_special_height_on_deleted_interval;
                        }
                        new_ordered_custom_lines.push(Rc::clone(&custom_line));
                        new_decoration_id_to_special_line.add(decoration_id, Rc::clone(&custom_line));
                    } else {
                        number_of_deletions += 1;
                    }
                } else {
                    {
                        let mut line = custom_line.borrow_mut();
                        line.index -= number_of_deletions;
                        line.line_number -= delete_count;
                        line.prefix_sum -= total_height_deleted;
                    }
                    new_ordered_custom_lines.push(Rc::clone(&custom_line));
                    new_decoration_id_to_special_line.add(custom_line.borrow().decoration_id.clone(), Rc::clone(&custom_line));
                }
                decoration_ids_seen.insert(custom_line.borrow().decoration_id.clone());
            }
            self.ordered_custom_lines = new_ordered_custom_lines;
            self.decoration_id_to_custom_line = new_decoration_id_to_special_line;
        } else {
            let total_height_deleted = f64::from(delete_count) * self.default_line_height;
            for i in end_index_of_deletion..self.ordered_custom_lines.len() {
                let mut custom_line = self.ordered_custom_lines[i].borrow_mut();
                if custom_line.line_number > to_line_number {
                    custom_line.line_number -= delete_count;
                    custom_line.prefix_sum -= total_height_deleted;
                }
            }
        }
    }

    fn do_lines_inserted(
        &mut self,
        from_line_number: i32,
        to_line_number: i32,
        staged_inserts: &mut Vec<CustomLineRef>,
        staged_id_map: &mut ArrayMap<String, CustomLineRef>,
    ) {
        let insert_count = to_line_number - from_line_number + 1;
        let candidate_start_index_of_insertion = self.binary_search_over_ordered_custom_lines_array(from_line_number);
        let start_index_of_insertion: usize = if candidate_start_index_of_insertion >= 0 {
            let mut start = candidate_start_index_of_insertion as usize;
            let mut i = candidate_start_index_of_insertion - 1;
            while i >= 0 {
                if self.ordered_custom_lines[i as usize].borrow().line_number == from_line_number {
                    start -= 1;
                } else {
                    break;
                }
                i -= 1;
            }
            start
        } else {
            (-(candidate_start_index_of_insertion + 1)) as usize
        };
        let mut to_re_add: Vec<CustomLineHeightData> = Vec::new();
        let mut decorations_immediately_after: Vec<String> = Vec::new();
        for line in &self.ordered_custom_lines[start_index_of_insertion..] {
            let line = line.borrow();
            if line.line_number == from_line_number && !decorations_immediately_after.contains(&line.decoration_id) {
                decorations_immediately_after.push(line.decoration_id.clone());
            }
        }
        let mut decorations_immediately_before: Vec<String> = Vec::new();
        for line in self.ordered_custom_lines[..start_index_of_insertion].iter().rev() {
            let line = line.borrow();
            if line.line_number == from_line_number - 1 && !decorations_immediately_before.contains(&line.decoration_id) {
                decorations_immediately_before.push(line.decoration_id.clone());
            }
        }
        let decorations_with_gaps = intersection(decorations_immediately_before.as_slice(), decorations_immediately_after);
        let prefix_sum_to_add = f64::from(insert_count) * self.default_line_height;
        for line in &self.ordered_custom_lines[start_index_of_insertion..] {
            let mut line = line.borrow_mut();
            line.line_number += insert_count;
            line.prefix_sum += prefix_sum_to_add;
        }

        if !decorations_with_gaps.is_empty() {
            for decoration_id in decorations_with_gaps {
                if let Some(decoration) = self.decoration_id_to_custom_line.get(&decoration_id) {
                    let start_line_number =
                        decoration.iter().fold(from_line_number, |min, l| min.min(l.borrow().line_number));
                    let end_line_number =
                        decoration.iter().fold(from_line_number, |max, l| max.max(l.borrow().line_number));
                    let line_height = decoration.iter().fold(0.0_f64, |max, l| max.max(l.borrow().special_height));
                    to_re_add.push(CustomLineHeightData { decoration_id, start_line_number, end_line_number, line_height });
                }
            }

            for dec in to_re_add {
                self.do_insert_or_change_custom_line_height(
                    &dec.decoration_id,
                    dec.start_line_number,
                    dec.end_line_number,
                    dec.line_height,
                    staged_inserts,
                    staged_id_map,
                );
            }
        }
    }

    fn binary_search_over_ordered_custom_lines_array(&self, line_number: i32) -> i64 {
        binary_search2(self.ordered_custom_lines.len(), |index| {
            let line_number_at_index = self.ordered_custom_lines[index].borrow().line_number;
            if line_number_at_index == line_number {
                0.0
            } else if line_number_at_index < line_number {
                -1.0
            } else {
                1.0
            }
        })
    }
}

/// `CustomLineHeightData`: a altura de uma decoração, para as linhas `[start_line_number, end_line_number]`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomLineHeightData {
    pub decoration_id: String,
    pub start_line_number: i32,
    pub end_line_number: i32,
    pub line_height: f64,
}

impl CustomLineHeightData {
    pub fn new(decoration_id: &str, start_line_number: i32, end_line_number: i32, line_height: f64) -> Self {
        Self { decoration_id: decoration_id.to_string(), start_line_number, end_line_number, line_height }
    }
}
