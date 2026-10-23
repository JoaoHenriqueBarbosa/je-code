//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/utils/towersLayout.ts`.
//!
//! O `OffsetRange` do crate é `i32` e as medidas (`Size2D`) são `f64`, como os números do JavaScript.

use crate::core_2d::size::Size2D;
use crate::offset_range::OffsetRange;

/// As áreas das torres ficam da esquerda para a direita, se tocam e alinham na base.
/// Qual a altura máxima de uma torre colocada na faixa horizontal pedida, de modo que o tamanho dela caiba
/// na união das `available_tower_areas` empilhadas?
pub fn get_max_tower_height_in_available_area(tower_horizontal_range: OffsetRange, available_tower_areas: &[Size2D]) -> f64 {
    let tower_left_offset = f64::from(tower_horizontal_range.start);
    let tower_right_offset = f64::from(tower_horizontal_range.end_exclusive);

    let mut min_height = f64::MAX;

    // A largura acumulada mostra com quais áreas a torre pedida se sobrepõe.
    let mut current_left_offset = 0.0_f64;
    for available_area in available_tower_areas {
        let current_right_offset = current_left_offset + available_area.width;

        let overlap_left = tower_left_offset.max(current_left_offset);
        let overlap_right = tower_right_offset.min(current_right_offset);

        if overlap_left < overlap_right {
            // Há sobreposição: guarda a menor altura.
            min_height = min_height.min(available_area.height);
        }

        current_left_offset = current_right_offset;
    }

    if tower_right_offset > current_left_offset {
        return 0.0;
    }

    // Sem sobreposição, devolve 0.
    if min_height == f64::MAX { 0.0 } else { min_height }
}
