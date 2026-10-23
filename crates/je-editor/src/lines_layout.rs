//! Porte de `src/vs/editor/common/viewLayout/linesLayout.ts`: o layout vertical das linhas e das zonas de
//! visão (whitespaces), objetos que ocupam altura e empurram os de baixo.
//!
//! Representação:
//! - Números de linha, ordinal, altura e largura mínima de whitespace são `i32` (o original os reduz com
//!   `| 0`); posições verticais, somas de altura e a altura de linha são `f64`. Os índices de whitespace são
//!   `i32` porque as buscas devolvem `-1`; um índice negativo usado para acessar o vetor dá pânico, como o
//!   `TypeError` do JavaScript.
//! - Os parâmetros de offset vertical aceitam qualquer `Into<f64>` e passam por `| 0` (`to_int32`) onde o
//!   original o faz.
//! - Os parâmetros com valor padrão (`includeViewZones`) são obrigatórios.
//! - As consultas de altura de linha e a soma de prefixo dos whitespaces são preguiçosas, então quase toda
//!   consulta pede `&mut self`.
//! - `changeWhitespace` e `changeLineHeights` recebem o acessor como `&mut dyn`; o `finally` do original vira
//!   um `catch_unwind` que confirma as mudanças pendentes e relança o pânico.
//! - `getWhitespaces()` devolve cópias do estado, não os objetos internos.
//!
//! TODO(integração): `IEditorWhitespace` e companhia estão em `view_model_types`, mover quando o viewModel for
//! portado.

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use je_base::strings::single_letter_hash;

use crate::line_heights::{CustomLineHeightData, LineHeightsManager};
use crate::view_model_types::{
    ILineHeightChangeAccessor, IPartialViewLinesViewportData, IViewWhitespaceViewportData, IWhitespaceChangeAccessor,
    IEditorWhitespace,
};

/// `x | 0` do JavaScript sobre um `number`: `ToInt32` (trunca, e dá a volta em 2^32).
fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(4294967296.0) as u32 as i32
}

struct PendingChange {
    id: String,
    new_after_line_number: i32,
    new_height: i32,
}

struct PendingRemove {
    id: String,
}

type Drained = (Vec<EditorWhitespace>, Vec<PendingChange>, Vec<PendingRemove>);

struct PendingChanges {
    has_pending: bool,
    inserts: Vec<EditorWhitespace>,
    changes: Vec<PendingChange>,
    removes: Vec<PendingRemove>,
}

impl PendingChanges {
    fn new() -> Self {
        Self { has_pending: false, inserts: Vec::new(), changes: Vec::new(), removes: Vec::new() }
    }

    fn insert(&mut self, x: EditorWhitespace) {
        self.has_pending = true;
        self.inserts.push(x);
    }

    fn change(&mut self, x: PendingChange) {
        self.has_pending = true;
        self.changes.push(x);
    }

    fn remove(&mut self, x: PendingRemove) {
        self.has_pending = true;
        self.removes.push(x);
    }

    /// A metade do `commit` que esvazia a fila: quem aplica é o [`LinesLayout`], que contém a fila e por isso
    /// não pode ser passado a ela.
    fn drain(&mut self) -> Option<Drained> {
        if !self.has_pending {
            return None;
        }
        self.has_pending = false;
        Some((
            std::mem::take(&mut self.inserts),
            std::mem::take(&mut self.changes),
            std::mem::take(&mut self.removes),
        ))
    }
}

/// `EditorWhitespace`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorWhitespace {
    pub id: String,
    pub after_line_number: i32,
    pub ordinal: i32,
    pub height: i32,
    pub min_width: i32,
    pub prefix_sum: f64,
}

impl EditorWhitespace {
    pub fn new(id: impl Into<String>, after_line_number: i32, ordinal: i32, height: i32, min_width: i32) -> Self {
        Self { id: id.into(), after_line_number, ordinal, height, min_width, prefix_sum: 0.0 }
    }
}

impl From<&EditorWhitespace> for IEditorWhitespace {
    fn from(whitespace: &EditorWhitespace) -> Self {
        Self { id: whitespace.id.clone(), after_line_number: whitespace.after_line_number, height: whitespace.height }
    }
}

static INSTANCE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// O acessor de `changeWhitespace`.
struct WhitespaceChangeAccessor<'a> {
    instance_id: &'a str,
    last_whitespace_id: &'a mut usize,
    pending_changes: &'a mut PendingChanges,
    had_a_change: bool,
}

impl IWhitespaceChangeAccessor for WhitespaceChangeAccessor<'_> {
    fn insert_whitespace(&mut self, after_line_number: i32, ordinal: i32, height_in_px: i32, min_width: i32) -> String {
        self.had_a_change = true;
        *self.last_whitespace_id += 1;
        let id = format!("{}{}", self.instance_id, self.last_whitespace_id);
        self.pending_changes.insert(EditorWhitespace::new(id.as_str(), after_line_number, ordinal, height_in_px, min_width));
        id
    }

    fn change_one_whitespace(&mut self, id: &str, new_after_line_number: i32, new_height: i32) {
        self.had_a_change = true;
        self.pending_changes.change(PendingChange { id: id.to_string(), new_after_line_number, new_height });
    }

    fn remove_whitespace(&mut self, id: &str) {
        self.had_a_change = true;
        self.pending_changes.remove(PendingRemove { id: id.to_string() });
    }
}

/// O acessor de `changeLineHeights`.
struct LineHeightChangeAccessor<'a> {
    manager: &'a mut LineHeightsManager,
    had_a_change: bool,
}

impl ILineHeightChangeAccessor for LineHeightChangeAccessor<'_> {
    fn insert_or_change_custom_line_height(
        &mut self,
        decoration_id: &str,
        start_line_number: i32,
        end_line_number: i32,
        line_height: f64,
    ) {
        self.had_a_change = true;
        self.manager.insert_or_change_custom_line_height(decoration_id, start_line_number, end_line_number, line_height);
    }

    fn remove_custom_line_height(&mut self, decoration_id: &str) {
        self.had_a_change = true;
        self.manager.remove_custom_line_height(decoration_id);
    }
}

/// Layout de objetos que ocupam espaço vertical (têm altura) e empurram os outros.
///
/// Esses objetos são linhas de texto ou os espaços entre elas (whitespaces). Oferece operações para trabalhar
/// com linhas que contêm whitespace que empurra as linhas para baixo.
pub struct LinesLayout {
    instance_id: String,
    pending_changes: PendingChanges,
    last_whitespace_id: usize,
    arr: Vec<EditorWhitespace>,
    prefix_sum_valid_index: i32,
    /// `-1` é o marcador de "não calculado".
    min_width: i32,
    line_count: i32,
    padding_top: f64,
    padding_bottom: f64,
    line_heights_manager: LineHeightsManager,
}

impl LinesLayout {
    pub fn new(
        line_count: i32,
        default_line_height: f64,
        padding_top: f64,
        padding_bottom: f64,
        custom_line_height_data: &[CustomLineHeightData],
    ) -> Self {
        let count = INSTANCE_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        Self {
            instance_id: single_letter_hash(count).to_string(),
            pending_changes: PendingChanges::new(),
            last_whitespace_id: 0,
            arr: Vec::new(),
            prefix_sum_valid_index: -1,
            min_width: -1,
            line_count,
            padding_top,
            padding_bottom,
            line_heights_manager: LineHeightsManager::new(default_line_height, custom_line_height_data),
        }
    }

    /// Acha o índice de inserção de um valor novo num vetor ordenado. Se já existe, o índice fica depois do
    /// valor existente.
    pub fn find_insertion_index(arr: &[EditorWhitespace], after_line_number: i32, ordinal: i32) -> usize {
        let mut low = 0usize;
        let mut high = arr.len();

        while low < high {
            let mid = (low + high) >> 1;

            if after_line_number == arr[mid].after_line_number {
                if ordinal < arr[mid].ordinal {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            } else if after_line_number < arr[mid].after_line_number {
                high = mid;
            } else {
                low = mid + 1;
            }
        }

        low
    }

    /// Muda a altura de uma linha, em pixels.
    pub fn set_default_line_height(&mut self, line_height: f64) {
        self.line_heights_manager.set_default_line_height(line_height);
    }

    /// Muda o padding usado para calcular os offsets verticais.
    pub fn set_padding(&mut self, padding_top: f64, padding_bottom: f64) {
        self.padding_top = padding_top;
        self.padding_bottom = padding_bottom;
    }

    /// Define o número de linhas.
    pub fn on_flushed(&mut self, line_count: i32, custom_line_height_data: &[CustomLineHeightData]) {
        self.line_count = line_count;
        self.line_heights_manager =
            LineHeightsManager::new(self.line_heights_manager.default_line_height(), custom_line_height_data);
    }

    pub fn change_line_heights(&mut self, callback: impl FnOnce(&mut dyn ILineHeightChangeAccessor)) -> bool {
        let mut accessor = LineHeightChangeAccessor { manager: &mut self.line_heights_manager, had_a_change: false };
        callback(&mut accessor);
        accessor.had_a_change
    }

    pub fn change_whitespace(&mut self, callback: impl FnOnce(&mut dyn IWhitespaceChangeAccessor)) -> bool {
        let mut accessor = WhitespaceChangeAccessor {
            instance_id: &self.instance_id,
            last_whitespace_id: &mut self.last_whitespace_id,
            pending_changes: &mut self.pending_changes,
            had_a_change: false,
        };
        // `try { ... } finally { commit }`: confirma o que ficou pendente mesmo se o callback entrar em pânico.
        let outcome = catch_unwind(AssertUnwindSafe(|| callback(&mut accessor)));
        let had_a_change = accessor.had_a_change;
        if let Some((inserts, changes, removes)) = self.pending_changes.drain() {
            self.commit_pending_changes(inserts, changes, removes);
        }
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
        had_a_change
    }

    /// `_commitPendingChanges`.
    fn commit_pending_changes(
        &mut self,
        inserts: Vec<EditorWhitespace>,
        changes: Vec<PendingChange>,
        removes: Vec<PendingRemove>,
    ) {
        if !inserts.is_empty() || !removes.is_empty() {
            self.min_width = -1; // marcador de "não calculado"
        }

        if inserts.len() + changes.len() + removes.len() <= 1 {
            // Quando só uma coisa aconteceu, trata com delicadeza.
            for insert in inserts {
                self.insert_whitespace_internal(insert);
            }
            for change in changes {
                self.change_one_whitespace_internal(&change.id, change.new_after_line_number, change.new_height);
            }
            for remove in removes {
                let Some(index) = self.find_whitespace_index(&remove.id) else {
                    continue;
                };
                self.remove_whitespace_at(index);
            }
            return;
        }

        // Reconstrói a estrutura inteira.
        let to_remove: HashSet<String> = removes.into_iter().map(|remove| remove.id).collect();
        let mut to_change: HashMap<String, PendingChange> = HashMap::new();
        for change in changes {
            to_change.insert(change.id.clone(), change);
        }

        let apply_remove_and_change = |whitespaces: Vec<EditorWhitespace>| -> Vec<EditorWhitespace> {
            let mut result = Vec::new();
            for mut whitespace in whitespaces {
                if to_remove.contains(&whitespace.id) {
                    continue;
                }
                if let Some(change) = to_change.get(&whitespace.id) {
                    whitespace.after_line_number = change.new_after_line_number;
                    whitespace.height = change.new_height;
                }
                result.push(whitespace);
            }
            result
        };

        let current = std::mem::take(&mut self.arr);
        let mut result = apply_remove_and_change(current);
        result.extend(apply_remove_and_change(inserts));
        result.sort_by(|a, b| a.after_line_number.cmp(&b.after_line_number).then(a.ordinal.cmp(&b.ordinal)));

        self.arr = result;
        self.prefix_sum_valid_index = -1;
    }

    /// `_insertWhitespace`.
    fn insert_whitespace_internal(&mut self, whitespace: EditorWhitespace) {
        let insert_index = Self::find_insertion_index(&self.arr, whitespace.after_line_number, whitespace.ordinal);
        self.arr.insert(insert_index, whitespace);
        self.prefix_sum_valid_index = self.prefix_sum_valid_index.min(insert_index as i32 - 1);
    }

    fn find_whitespace_index(&self, id: &str) -> Option<usize> {
        self.arr.iter().position(|whitespace| whitespace.id == id)
    }

    /// `_changeOneWhitespace`.
    fn change_one_whitespace_internal(&mut self, id: &str, new_after_line_number: i32, new_height: i32) {
        let Some(index) = self.find_whitespace_index(id) else {
            return;
        };
        if self.arr[index].height != new_height {
            self.arr[index].height = new_height;
            self.prefix_sum_valid_index = self.prefix_sum_valid_index.min(index as i32 - 1);
        }
        if self.arr[index].after_line_number != new_after_line_number {
            // `afterLineNumber` mudou para este whitespace. Como isso pode reordenar, remove e insere de novo.
            let mut whitespace = self.arr[index].clone();
            self.remove_whitespace_at(index);
            whitespace.after_line_number = new_after_line_number;
            self.insert_whitespace_internal(whitespace);
        }
    }

    /// `_removeWhitespace`.
    fn remove_whitespace_at(&mut self, remove_index: usize) {
        self.arr.remove(remove_index);
        self.prefix_sum_valid_index = self.prefix_sum_valid_index.min(remove_index as i32 - 1);
    }

    /// Avisa que linhas foram apagadas (uma zona contínua).
    ///
    /// `from_line_number` e `to_line_number` são inclusivos.
    pub fn on_lines_deleted(&mut self, from_line_number: i32, to_line_number: i32) {
        self.line_count -= to_line_number - from_line_number + 1;
        for whitespace in &mut self.arr {
            let after_line_number = whitespace.after_line_number;

            if from_line_number <= after_line_number && after_line_number <= to_line_number {
                // A linha em que este whitespace estava depois foi apagada: vai para antes da primeira apagada.
                whitespace.after_line_number = from_line_number - 1;
            } else if after_line_number > to_line_number {
                // A linha subiu: o whitespace sobe junto.
                whitespace.after_line_number -= to_line_number - from_line_number + 1;
            }
        }
        self.line_heights_manager.on_lines_deleted(from_line_number, to_line_number);
    }

    /// Avisa que linhas foram inseridas (uma zona contínua).
    ///
    /// `from_line_number` e `to_line_number` são inclusivos.
    pub fn on_lines_inserted(&mut self, from_line_number: i32, to_line_number: i32) {
        self.line_count += to_line_number - from_line_number + 1;
        for whitespace in &mut self.arr {
            if from_line_number <= whitespace.after_line_number {
                whitespace.after_line_number += to_line_number - from_line_number + 1;
            }
        }
        self.line_heights_manager.on_lines_inserted(from_line_number, to_line_number);
    }

    /// A soma de todos os whitespaces.
    pub fn get_whitespaces_total_height(&mut self) -> f64 {
        if self.arr.is_empty() {
            return 0.0;
        }
        self.get_whitespaces_accumulated_height(self.arr.len() as i32 - 1)
    }

    /// A soma das alturas dos whitespaces em `[0..index]`, inclusive o de `index`.
    pub fn get_whitespaces_accumulated_height(&mut self, index: i32) -> f64 {
        let mut start_index = (self.prefix_sum_valid_index + 1).max(0);
        if start_index == 0 {
            self.arr[0].prefix_sum = f64::from(self.arr[0].height);
            start_index += 1;
        }

        for i in start_index..=index {
            let i = i as usize;
            self.arr[i].prefix_sum = self.arr[i - 1].prefix_sum + f64::from(self.arr[i].height);
        }
        self.prefix_sum_valid_index = self.prefix_sum_valid_index.max(index);
        self.arr[index as usize].prefix_sum
    }

    /// A soma das alturas de todos os objetos.
    pub fn get_lines_total_height(&mut self) -> f64 {
        let lines_height = self.line_heights_manager.get_accumulated_line_heights_including_line_number(self.line_count);
        let whitespaces_height = self.get_whitespaces_total_height();

        lines_height + whitespaces_height + self.padding_top + self.padding_bottom
    }

    /// A altura acumulada dos whitespaces antes do número de linha dado.
    pub fn get_whitespace_accumulated_height_before_line_number(&mut self, line_number: i32) -> f64 {
        let last_whitespace_before_line_number = self.find_last_whitespace_before_line_number(line_number);

        if last_whitespace_before_line_number == -1 {
            return 0.0;
        }

        self.get_whitespaces_accumulated_height(last_whitespace_before_line_number)
    }

    fn find_last_whitespace_before_line_number(&self, line_number: i32) -> i32 {
        // Acha o whitespace antes do número de linha.
        let arr = &self.arr;
        let mut low: i32 = 0;
        let mut high: i32 = arr.len() as i32 - 1;

        while low <= high {
            let delta = high - low;
            let half_delta = delta / 2;
            let mid = low + half_delta;

            if arr[mid as usize].after_line_number < line_number {
                if mid as usize + 1 >= arr.len() || arr[mid as usize + 1].after_line_number >= line_number {
                    return mid;
                } else {
                    low = mid + 1;
                }
            } else {
                high = mid - 1;
            }
        }

        -1
    }

    fn find_first_whitespace_after_line_number(&self, line_number: i32) -> i32 {
        let last_whitespace_before_line_number = self.find_last_whitespace_before_line_number(line_number);
        let first_whitespace_after_line_number = last_whitespace_before_line_number + 1;

        if (first_whitespace_after_line_number as usize) < self.arr.len() {
            return first_whitespace_after_line_number;
        }

        -1
    }

    /// O índice do primeiro whitespace com `after_line_number >= line_number`, ou `-1` se não houver.
    pub fn get_first_whitespace_index_after_line_number(&self, line_number: i32) -> i32 {
        self.find_first_whitespace_after_line_number(line_number)
    }

    /// O offset vertical (a soma das alturas de tudo que está acima) de um número de linha.
    pub fn get_vertical_offset_for_line_number(&mut self, line_number: i32, include_view_zones: bool) -> f64 {
        let previous_lines_height = if line_number > 1 {
            self.line_heights_manager.get_accumulated_line_heights_including_line_number(line_number - 1)
        } else {
            0.0
        };

        let previous_whitespaces_height = self
            .get_whitespace_accumulated_height_before_line_number(line_number - if include_view_zones { 1 } else { 0 });

        previous_lines_height + previous_whitespaces_height + self.padding_top
    }

    pub fn get_line_height_for_line_number(&mut self, line_number: i32) -> f64 {
        self.line_heights_manager.height_for_line_number(line_number)
    }

    /// O offset vertical logo depois de um número de linha.
    pub fn get_vertical_offset_after_line_number(&mut self, line_number: i32, include_view_zones: bool) -> f64 {
        let previous_lines_height =
            self.line_heights_manager.get_accumulated_line_heights_including_line_number(line_number);
        let previous_whitespaces_height = self
            .get_whitespace_accumulated_height_before_line_number(line_number + if include_view_zones { 1 } else { 0 });
        previous_lines_height + previous_whitespaces_height + self.padding_top
    }

    /// Se há algum whitespace no documento.
    pub fn has_whitespace(&self) -> bool {
        self.get_whitespaces_count() > 0
    }

    /// A maior largura mínima entre todos os whitespaces.
    pub fn get_whitespace_min_width(&mut self) -> i32 {
        if self.min_width == -1 {
            let mut min_width = 0;
            for whitespace in &self.arr {
                min_width = min_width.max(whitespace.min_width);
            }
            self.min_width = min_width;
        }
        self.min_width
    }

    /// Se `vertical_offset` está abaixo de todas as linhas.
    pub fn is_after_lines(&mut self, vertical_offset: impl Into<f64>) -> bool {
        let total_height = self.get_lines_total_height();
        vertical_offset.into() > total_height
    }

    pub fn is_in_top_padding(&self, vertical_offset: impl Into<f64>) -> bool {
        if self.padding_top == 0.0 {
            return false;
        }
        vertical_offset.into() < self.padding_top
    }

    pub fn is_in_bottom_padding(&mut self, vertical_offset: impl Into<f64>) -> bool {
        if self.padding_bottom == 0.0 {
            return false;
        }
        let total_height = self.get_lines_total_height();
        vertical_offset.into() >= total_height - self.padding_bottom
    }

    /// Acha o primeiro número de linha que está em `vertical_offset` ou depois dele.
    ///
    /// Se `get_vertical_offset_for_line_number(line)` é `x` e o da `line + 1` é `y`, então
    /// `get_line_number_at_or_after_vertical_offset(i) = line` para `x <= i < y`.
    pub fn get_line_number_at_or_after_vertical_offset(&mut self, vertical_offset: impl Into<f64>) -> i32 {
        let vertical_offset = f64::from(to_int32(vertical_offset.into()));

        if vertical_offset < 0.0 {
            return 1;
        }

        let lines_count = self.line_count;
        let mut min_line_number = 1;
        let mut max_line_number = lines_count;

        while min_line_number < max_line_number {
            let mid_line_number = (min_line_number + max_line_number) / 2;

            let line_height = self.get_line_height_for_line_number(mid_line_number);
            let mid_line_number_vertical_offset =
                f64::from(to_int32(self.get_vertical_offset_for_line_number(mid_line_number, false)));

            if vertical_offset >= mid_line_number_vertical_offset + line_height {
                // O offset está depois da linha do meio.
                min_line_number = mid_line_number + 1;
            } else if vertical_offset >= mid_line_number_vertical_offset {
                // Acertou.
                return mid_line_number;
            } else {
                // O offset está antes da linha do meio, mas ela ainda pode ser a procurada.
                max_line_number = mid_line_number;
            }
        }

        if min_line_number > lines_count {
            return lines_count;
        }

        min_line_number
    }

    /// Todas as linhas e seus offsets verticais relativos que estão entre `vertical_offset1` e `vertical_offset2`.
    pub fn get_lines_viewport_data(
        &mut self,
        vertical_offset1: impl Into<f64>,
        vertical_offset2: impl Into<f64>,
    ) -> IPartialViewLinesViewportData {
        let vertical_offset1 = f64::from(to_int32(vertical_offset1.into()));
        let vertical_offset2 = f64::from(to_int32(vertical_offset2.into()));

        // Acha a primeira linha. O mundo não é perfeito: ela pode começar antes ou depois de `vertical_offset1`.
        let start_line_number = self.get_line_number_at_or_after_vertical_offset(vertical_offset1);
        let start_line_number_vertical_offset =
            f64::from(to_int32(self.get_vertical_offset_for_line_number(start_line_number, false)));

        let mut end_line_number = self.line_count;

        // Acompanha também o whitespace corrente.
        let mut whitespace_index = self.get_first_whitespace_index_after_line_number(start_line_number);
        let whitespace_count = self.get_whitespaces_count() as i32;
        let mut current_whitespace_height: i32;
        let mut current_whitespace_after_line_number: i32;

        if whitespace_index == -1 {
            whitespace_index = whitespace_count;
            current_whitespace_after_line_number = end_line_number + 1;
            current_whitespace_height = 0;
        } else {
            current_whitespace_after_line_number = self.get_after_line_number_for_whitespace_index(whitespace_index);
            current_whitespace_height = self.get_height_for_whitespace_index(whitespace_index);
        }

        let mut current_vertical_offset = start_line_number_vertical_offset;
        let mut current_line_relative_offset = current_vertical_offset;

        // O IE (todas as versões) não aguenta unidades acima de ~1.533.908 px: a cada 500k px baixa os números.
        const STEP_SIZE: f64 = 500000.0;
        let mut big_numbers_delta = 0.0;
        if start_line_number_vertical_offset >= STEP_SIZE {
            // Calcula um delta que garante linhas posicionadas em incrementos de `lineHeight`.
            let default_line_height = self.line_heights_manager.default_line_height();
            big_numbers_delta = (start_line_number_vertical_offset / STEP_SIZE).floor() * STEP_SIZE;
            big_numbers_delta = (big_numbers_delta / default_line_height).floor() * default_line_height;

            current_line_relative_offset -= big_numbers_delta;
        }

        let mut lines_offsets: Vec<f64> = Vec::new();

        let vertical_center = vertical_offset1 + (vertical_offset2 - vertical_offset1) / 2.0;
        let mut centered_line_number = -1;

        // Descobre até onde vão as linhas.
        for line_number in start_line_number..=end_line_number {
            let line_height = self.get_line_height_for_line_number(line_number);
            if centered_line_number == -1 {
                let current_line_top = current_vertical_offset;
                let current_line_bottom = current_vertical_offset + line_height;
                if (current_line_top <= vertical_center && vertical_center < current_line_bottom)
                    || current_line_top > vertical_center
                {
                    centered_line_number = line_number;
                }
            }

            // Conta a altura da linha corrente nos offsets verticais.
            current_vertical_offset += line_height;
            lines_offsets.push(current_line_relative_offset);

            // A próxima linha começa logo depois desta.
            current_line_relative_offset += line_height;
            while current_whitespace_after_line_number == line_number {
                // Empurra a próxima linha com a altura do whitespace corrente.
                current_line_relative_offset += f64::from(current_whitespace_height);

                // Conta o whitespace corrente nos offsets verticais.
                current_vertical_offset += f64::from(current_whitespace_height);
                whitespace_index += 1;

                if whitespace_index >= whitespace_count {
                    current_whitespace_after_line_number = end_line_number + 1;
                } else {
                    current_whitespace_after_line_number = self.get_after_line_number_for_whitespace_index(whitespace_index);
                    current_whitespace_height = self.get_height_for_whitespace_index(whitespace_index);
                }
            }

            if current_vertical_offset >= vertical_offset2 {
                // Cobriu toda a área do viewport, hora de parar.
                end_line_number = line_number;
                break;
            }
        }

        if centered_line_number == -1 {
            centered_line_number = end_line_number;
        }

        let end_line_number_vertical_offset =
            f64::from(to_int32(self.get_vertical_offset_for_line_number(end_line_number, false)));

        let mut completely_visible_start_line_number = start_line_number;
        let mut completely_visible_end_line_number = end_line_number;

        if completely_visible_start_line_number < completely_visible_end_line_number
            && start_line_number_vertical_offset < vertical_offset1
        {
            completely_visible_start_line_number += 1;
        }
        if completely_visible_start_line_number < completely_visible_end_line_number {
            let end_line_height = self.get_line_height_for_line_number(end_line_number);
            if end_line_number_vertical_offset + end_line_height > vertical_offset2 {
                completely_visible_end_line_number -= 1;
            }
        }

        IPartialViewLinesViewportData {
            big_numbers_delta,
            start_line_number,
            end_line_number,
            relative_vertical_offset: lines_offsets,
            centered_line_number,
            completely_visible_start_line_number,
            completely_visible_end_line_number,
            line_height: self.line_heights_manager.default_line_height(),
        }
    }

    pub fn get_vertical_offset_for_whitespace_index(&mut self, whitespace_index: i32) -> f64 {
        let after_line_number = self.get_after_line_number_for_whitespace_index(whitespace_index);

        let previous_lines_height = if after_line_number >= 1 {
            self.line_heights_manager.get_accumulated_line_heights_including_line_number(after_line_number)
        } else {
            0.0
        };

        let previous_whitespaces_height = if whitespace_index > 0 {
            self.get_whitespaces_accumulated_height(whitespace_index - 1)
        } else {
            0.0
        };
        previous_lines_height + previous_whitespaces_height + self.padding_top
    }

    /// `getWhitespaceIndexAtOrAfterVerticallOffset` (o nome do original tem o `l` a mais).
    pub fn get_whitespace_index_at_or_after_verticall_offset(&mut self, vertical_offset: impl Into<f64>) -> i32 {
        let vertical_offset = f64::from(to_int32(vertical_offset.into()));

        let mut min_whitespace_index = 0;
        let mut max_whitespace_index = self.get_whitespaces_count() as i32 - 1;

        if max_whitespace_index < 0 {
            return -1;
        }

        // Caso especial: não há o que achar.
        let max_whitespace_vertical_offset = self.get_vertical_offset_for_whitespace_index(max_whitespace_index);
        let max_whitespace_height = self.get_height_for_whitespace_index(max_whitespace_index);
        if vertical_offset >= max_whitespace_vertical_offset + f64::from(max_whitespace_height) {
            return -1;
        }

        while min_whitespace_index < max_whitespace_index {
            let mid_whitespace_index = (min_whitespace_index + max_whitespace_index) / 2;

            let mid_whitespace_vertical_offset = self.get_vertical_offset_for_whitespace_index(mid_whitespace_index);
            let mid_whitespace_height = self.get_height_for_whitespace_index(mid_whitespace_index);

            if vertical_offset >= mid_whitespace_vertical_offset + f64::from(mid_whitespace_height) {
                // O offset está depois do whitespace.
                min_whitespace_index = mid_whitespace_index + 1;
            } else if vertical_offset >= mid_whitespace_vertical_offset {
                // Acertou.
                return mid_whitespace_index;
            } else {
                // O offset está antes do whitespace, mas o do meio ainda pode ser o procurado.
                max_whitespace_index = mid_whitespace_index;
            }
        }
        min_whitespace_index
    }

    /// Exatamente o whitespace que está em `vertical_offset`, ou `None`.
    pub fn get_whitespace_at_vertical_offset(
        &mut self,
        vertical_offset: impl Into<f64>,
    ) -> Option<IViewWhitespaceViewportData> {
        let vertical_offset = f64::from(to_int32(vertical_offset.into()));

        let candidate_index = self.get_whitespace_index_at_or_after_verticall_offset(vertical_offset);

        if candidate_index < 0 {
            return None;
        }

        if candidate_index as usize >= self.get_whitespaces_count() {
            return None;
        }

        let candidate_top = self.get_vertical_offset_for_whitespace_index(candidate_index);

        if candidate_top > vertical_offset {
            return None;
        }

        let candidate_height = self.get_height_for_whitespace_index(candidate_index);
        let candidate_id = self.get_id_for_whitespace_index(candidate_index).to_string();
        let candidate_after_line_number = self.get_after_line_number_for_whitespace_index(candidate_index);

        Some(IViewWhitespaceViewportData {
            id: candidate_id,
            after_line_number: candidate_after_line_number,
            vertical_offset: candidate_top,
            height: candidate_height,
        })
    }

    /// Os whitespaces posicionados entre `vertical_offset1` e `vertical_offset2`.
    pub fn get_whitespace_viewport_data(
        &mut self,
        vertical_offset1: impl Into<f64>,
        vertical_offset2: impl Into<f64>,
    ) -> Vec<IViewWhitespaceViewportData> {
        let vertical_offset1 = f64::from(to_int32(vertical_offset1.into()));
        let vertical_offset2 = f64::from(to_int32(vertical_offset2.into()));

        let start_index = self.get_whitespace_index_at_or_after_verticall_offset(vertical_offset1);
        let end_index = self.get_whitespaces_count() as i32 - 1;

        if start_index < 0 {
            return Vec::new();
        }

        let mut result = Vec::new();
        for i in start_index..=end_index {
            let top = self.get_vertical_offset_for_whitespace_index(i);
            let height = self.get_height_for_whitespace_index(i);
            if top >= vertical_offset2 {
                break;
            }

            result.push(IViewWhitespaceViewportData {
                id: self.get_id_for_whitespace_index(i).to_string(),
                after_line_number: self.get_after_line_number_for_whitespace_index(i),
                vertical_offset: top,
                height,
            });
        }

        result
    }

    /// Todos os whitespaces.
    pub fn get_whitespaces(&self) -> Vec<IEditorWhitespace> {
        self.arr.iter().map(IEditorWhitespace::from).collect()
    }

    /// O número de whitespaces.
    pub fn get_whitespaces_count(&self) -> usize {
        self.arr.len()
    }

    /// O `id` do whitespace no índice `index`.
    pub fn get_id_for_whitespace_index(&self, index: i32) -> &str {
        &self.arr[index as usize].id
    }

    /// O `after_line_number` do whitespace no índice `index`.
    pub fn get_after_line_number_for_whitespace_index(&self, index: i32) -> i32 {
        self.arr[index as usize].after_line_number
    }

    /// O `height` do whitespace no índice `index`.
    pub fn get_height_for_whitespace_index(&self, index: i32) -> i32 {
        self.arr[index as usize].height
    }
}
