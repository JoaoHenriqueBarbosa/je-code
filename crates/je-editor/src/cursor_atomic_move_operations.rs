//! Porte de `src/vs/editor/common/cursor/cursorAtomicMoveOperations.ts`.
//!
//! Movimento "atômico" sobre a indentação: com tabulações atômicas, as setas e a seleção com o mouse
//! tratam um nível de indentação feito de espaços como se fosse um único caractere de tabulação.
//! Posições e retornos são sempre contados a partir de 0, e `-1` significa "não se aplica".

use crate::cursor_columns::next_render_tab_stop;
use je_base::char_code as cc;

const SPACE: u16 = cc::Space as u16;
const TAB: u16 = cc::Tab as u16;

/// `Direction`: para onde o movimento atômico vai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Nearest,
}

/// A classe estática `AtomicTabMoveOperations` do original vira funções livres neste módulo.
/// `whitespaceVisibleColumn`.
pub fn whitespace_visible_column(
    line_content: &[u16],
    position: i32,
    tab_size: i32,
) -> (i32, i32, i32) {
    let line_length = line_content.len() as i32;
    let mut visible_column = 0;
    let mut prev_tab_stop_position = -1;
    let mut prev_tab_stop_visible_column = -1;
    for i in 0..line_length {
        if i == position {
            return (
                prev_tab_stop_position,
                prev_tab_stop_visible_column,
                visible_column,
            );
        }
        if visible_column % tab_size == 0 {
            prev_tab_stop_position = i;
            prev_tab_stop_visible_column = visible_column;
        }
        match line_content[i as usize] {
            SPACE => visible_column += 1,
            // Pula para o próximo múltiplo de `tab_size`.
            TAB => visible_column = next_render_tab_stop(visible_column, tab_size),
            _ => return (-1, -1, -1),
        }
    }
    if position == line_length {
        return (
            prev_tab_stop_position,
            prev_tab_stop_visible_column,
            visible_column,
        );
    }
    (-1, -1, -1)
}

/// Avança a coluna visível sobre um espaço ou uma tabulação; `None` para qualquer outro caractere.
fn advance_whitespace(code_unit: u16, visible_column: i32, tab_size: i32) -> Option<i32> {
    match code_unit {
        SPACE => Some(visible_column + 1),
        TAB => Some(next_render_tab_stop(visible_column, tab_size)),
        _ => None,
    }
}

/// `atomicPosition`.
pub fn atomic_position(
    line_content: &[u16],
    position: i32,
    tab_size: i32,
    direction: Direction,
) -> i32 {
    let line_length = line_content.len() as i32;

    // A coluna visível (a partir de 0) da posição, ou -1 se ela não está na indentação inicial.
    let (prev_tab_stop_position, prev_tab_stop_visible_column, visible_column) =
        whitespace_visible_column(line_content, position, tab_size);

    if visible_column == -1 {
        return -1;
    }

    // O resultado fica à esquerda ou à direita da posição atual. O caso de `Nearest` em que ele
    // coincide com a posição atual é tratado aqui.
    let left = match direction {
        Direction::Left => true,
        Direction::Right => false,
        Direction::Nearest => {
            // O código abaixo supõe que a saída está à esquerda ou à direita da entrada.
            if visible_column % tab_size == 0 {
                return position;
            }
            // Vai para a indentação mais próxima. Divisão em ponto flutuante, como no JavaScript.
            f64::from(visible_column % tab_size) <= f64::from(tab_size) / 2.0
        }
    };

    // Indo para a esquerda, basta o último tab stop calculado na primeira passada.
    if left {
        if prev_tab_stop_position == -1 {
            return -1;
        }
        // Continua varrendo para a direita para garantir que `tab stop + tab_size` fica antes de
        // um caractere que não é espaço. Assim a seta para a esquerda no fim de uma indentação
        // parcial anda só um caractere: `      foo` com tabulação 4 vai da posição 6 para a 5, não
        // para a 4.
        let mut current_visible_column = prev_tab_stop_visible_column;
        for i in prev_tab_stop_position..line_length {
            if current_visible_column == prev_tab_stop_visible_column + tab_size {
                // É uma indentação completa.
                return prev_tab_stop_position;
            }
            match advance_whitespace(line_content[i as usize], current_visible_column, tab_size) {
                Some(next) => current_visible_column = next,
                None => return -1,
            }
        }
        if current_visible_column == prev_tab_stop_visible_column + tab_size {
            return prev_tab_stop_position;
        }
        // Era uma indentação parcial.
        return -1;
    }

    // Indo para a direita.
    let target_visible_column = next_render_tab_stop(visible_column, tab_size);

    // Continua de onde `whitespace_visible_column` parou.
    let mut current_visible_column = visible_column;
    for i in position..line_length {
        if current_visible_column == target_visible_column {
            return i;
        }
        match advance_whitespace(line_content[i as usize], current_visible_column, tab_size) {
            Some(next) => current_visible_column = next,
            None => return -1,
        }
    }
    // Este caso trata a coluna alvo no fim da linha.
    if current_visible_column == target_visible_column {
        return line_length;
    }
    -1
}
