//! Porte de `src/vs/editor/common/viewModel/glyphLanesModel.ts`: `GlyphMarginLanesModel`, o mapa de bits das
//! faixas (lanes) da margem de glifos por linha.
//!
//! Divergências de tradução:
//! - `IGlyphMarginLanesModel` (`model.ts`) ainda não tinha porte em `model`; o trait entra aqui, porque o
//!   `IViewModel` o espera como tipo associado (`GlyphLanes`).
//! - O modelo é compartilhado por `Rc` e mutado por trás da interface (`reset`, `push`), então o estado fica
//!   num `RefCell` e os métodos são `&self`, como no resto do crate.
//! - `Uint8Array` é `Vec<u8>`. Um acesso fora do tamanho num `Uint8Array` lê `undefined` (que o `&` do
//!   original trata como 0) e a escrita é ignorada: a leitura aqui usa `get` com 0 e a escrita só acontece
//!   dentro do vetor. `new Uint8Array(n)` e o `fill(0)` são `vec![0; n]` e `fill(0)`.
//! - `persist?: boolean` é `bool` (ausente é `false`). `persist` (máscara de bits das faixas sempre visíveis)
//!   é `u8`: cabe nas 3 faixas. `reset` não zera `persist`, como o original.
//! - Os pares `getLanesAtLine`/`countAtLine` do original percorrem o mesmo laço; aqui as duas vêm do mesmo
//!   iterador de faixas ativas (`active_lanes`), e a contagem é o número de itens dele.

use std::cell::RefCell;

use crate::model::GlyphMarginLane;
use crate::range::Range;

/// As faixas em ordem de índice: a faixa `i + 1` do original é `LANES[i]`.
const LANES: [GlyphMarginLane; 3] = [GlyphMarginLane::Left, GlyphMarginLane::Center, GlyphMarginLane::Right];

/// `MAX_LANE = GlyphMarginLane.Right`.
const MAX_LANE: i32 = GlyphMarginLane::Right as i32;

/// `IGlyphMarginLanesModel` (`model.ts`).
pub trait IGlyphMarginLanesModel {
    /// O número de faixas que devem ser desenhadas no editor.
    fn required_lanes(&self) -> i32;

    /// As faixas que devem ser desenhadas numa dada linha.
    fn get_lanes_at_line(&self, line_number: i32) -> Vec<GlyphMarginLane>;

    /// Reinicia o modelo e garante que ele caiba ao menos `max_line` linhas.
    fn reset(&self, max_line: i32);

    /// Registra que uma faixa deve ficar visível no intervalo do modelo. Com `persist`, a faixa fica sempre
    /// visível, mesmo nas linhas sem pedido específico dela.
    fn push(&self, lane: GlyphMarginLane, range: Range, persist: bool);
}

/// O estado mutável do modelo.
struct LanesState {
    lanes: Vec<u8>,
    persist: u8,
    /// Sempre desenha ao menos uma faixa.
    required_lanes: i32,
}

/// Quantos bytes cobrem as faixas das linhas `0..=max_line`: `Math.ceil(((maxLine + 1) * MAX_LANE) / 8)`.
fn byte_count(max_line: i32) -> usize {
    (((max_line + 1) * MAX_LANE).max(0) as usize).div_ceil(8)
}

/// O bit da faixa de índice `lane_index` na linha `line_number`.
fn bit_of(line_number: i32, lane_index: usize) -> u32 {
    (MAX_LANE * line_number) as u32 + lane_index as u32
}

impl LanesState {
    fn is_bit_set(&self, bit: u32) -> bool {
        self.lanes.get((bit >> 3) as usize).is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
    }

    /// As faixas visíveis na linha: as persistentes e as pedidas para ela.
    fn active_lanes(&self, line_number: i32) -> impl Iterator<Item = GlyphMarginLane> + '_ {
        LANES.iter().enumerate().filter_map(move |(index, lane)| {
            (self.persist & (1 << index) != 0 || self.is_bit_set(bit_of(line_number, index))).then_some(*lane)
        })
    }
}

/// `GlyphMarginLanesModel`.
pub struct GlyphMarginLanesModel {
    state: RefCell<LanesState>,
}

impl GlyphMarginLanesModel {
    pub fn new(max_line: i32) -> Self {
        Self { state: RefCell::new(LanesState { lanes: vec![0; byte_count(max_line)], persist: 0, required_lanes: 1 }) }
    }
}

impl IGlyphMarginLanesModel for GlyphMarginLanesModel {
    fn required_lanes(&self) -> i32 {
        self.state.borrow().required_lanes
    }

    fn get_lanes_at_line(&self, line_number: i32) -> Vec<GlyphMarginLane> {
        let lanes: Vec<GlyphMarginLane> = self.state.borrow().active_lanes(line_number).collect();
        if lanes.is_empty() { vec![GlyphMarginLane::Center] } else { lanes }
    }

    fn reset(&self, max_line: i32) {
        let mut state = self.state.borrow_mut();
        let bytes = byte_count(max_line);
        if state.lanes.len() < bytes {
            state.lanes = vec![0; bytes];
        } else {
            state.lanes.fill(0);
        }
        state.required_lanes = 1;
    }

    fn push(&self, lane: GlyphMarginLane, range: Range, persist: bool) {
        let mut state = self.state.borrow_mut();
        let lane_index = lane as usize - 1;
        if persist {
            state.persist |= 1 << lane_index;
        }
        for line_number in range.start_line_number..=range.end_line_number {
            let bit = bit_of(line_number, lane_index);
            if let Some(byte) = state.lanes.get_mut((bit >> 3) as usize) {
                *byte |= 1 << (bit % 8);
            }
            let count = state.active_lanes(line_number).count() as i32;
            state.required_lanes = state.required_lanes.max(count);
        }
    }
}
