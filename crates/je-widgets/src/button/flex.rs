//! A resolução de comprimentos flexíveis do CSS Flexbox (CSS Flexible Box Layout, seção 9.7), numa linha.
//!
//! O `je-ui` não tem motor de layout; os widgets de botão (`radio`, `ButtonWithDropdown`) dependem do
//! `display: flex` com `flex: 1 1 0`, `width: 100%` que encolhe e `min-width: auto`, e o número que
//! o navegador calcula para cada item sai desta função.

/// Um item de uma linha flex, em tamanhos externos (a caixa de borda mais margem, aqui sem margem).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlexItem {
    /// O tamanho base externo (`flex-basis` mais padding e borda).
    pub base: f64,
    /// Padding e borda no eixo principal: o tamanho interno base é `base - pad_border`.
    pub pad_border: f64,
    pub grow: f64,
    pub shrink: f64,
    /// O `min-width` externo (no `auto`, o min-content).
    pub min: f64,
}

impl FlexItem {
    fn clamp(&self, size: f64) -> f64 {
        size.max(self.min)
    }
}

/// O tamanho externo final de cada item de uma linha de `available` px (já sem os `gap`).
pub(crate) fn resolve_flexible_lengths(items: &[FlexItem], available: f64) -> Vec<f64> {
    let hypothetical: Vec<f64> = items.iter().map(|i| i.clamp(i.base)).collect();
    let growing = hypothetical.iter().sum::<f64>() < available;
    let factor = |i: &FlexItem| if growing { i.grow } else { i.shrink * (i.base - i.pad_border).max(0.0) };
    let mut target = vec![0.0; items.len()];
    let mut frozen = vec![false; items.len()];
    for (index, item) in items.iter().enumerate() {
        let inflexible = factor(item) == 0.0 || (growing && item.base > hypothetical[index]) || (!growing && item.base < hypothetical[index]);
        if inflexible {
            target[index] = hypothetical[index];
            frozen[index] = true;
        }
    }
    let free_space = |target: &[f64], frozen: &[bool]| available - items.iter().enumerate().map(|(i, item)| if frozen[i] { target[i] } else { item.base }).sum::<f64>();
    let initial_free = free_space(&target, &frozen);
    while frozen.iter().any(|f| !f) {
        let mut free = free_space(&target, &frozen);
        let factor_sum: f64 = items.iter().enumerate().filter(|(i, _)| !frozen[*i]).map(|(_, item)| factor(item)).sum();
        let flex_sum: f64 = items.iter().enumerate().filter(|(i, _)| !frozen[*i]).map(|(_, item)| if growing { item.grow } else { item.shrink }).sum();
        if flex_sum < 1.0 && (initial_free * flex_sum).abs() < free.abs() {
            free = initial_free * flex_sum;
        }
        let mut unclamped = vec![0.0; items.len()];
        for (index, item) in items.iter().enumerate().filter(|(i, _)| !frozen[*i]) {
            let share = if factor_sum > 0.0 { factor(item) / factor_sum } else { 0.0 };
            unclamped[index] = item.base + free * share;
            target[index] = item.clamp(unclamped[index]);
        }
        let violation: f64 = items.iter().enumerate().filter(|(i, _)| !frozen[*i]).map(|(i, _)| target[i] - unclamped[i]).sum();
        for index in 0..items.len() {
            if frozen[index] {
                continue;
            }
            let min_violation = target[index] > unclamped[index];
            if violation == 0.0 || (violation > 0.0 && min_violation) || (violation < 0.0 && !min_violation && target[index] < unclamped[index]) {
                frozen[index] = true;
            }
        }
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(base: f64, pad_border: f64, grow: f64, shrink: f64, min: f64) -> FlexItem {
        FlexItem { base, pad_border, grow, shrink, min }
    }

    #[test]
    fn grow_equal_with_basis_zero() {
        let items = [item(14.0, 14.0, 1.0, 1.0, 20.0); 3];
        let sizes = resolve_flexible_lengths(&items, 120.0);
        assert!(sizes.iter().all(|s| (s - 40.0).abs() < 1e-9), "{sizes:?}");
    }

    #[test]
    fn grow_respects_min_content_and_redistributes() {
        let items = [item(14.0, 14.0, 1.0, 1.0, 20.0), item(14.0, 14.0, 1.0, 1.0, 100.0), item(14.0, 14.0, 1.0, 1.0, 20.0)];
        let sizes = resolve_flexible_lengths(&items, 160.0);
        assert!((sizes[1] - 100.0).abs() < 1e-9 && (sizes[0] - 30.0).abs() < 1e-9 && (sizes[2] - 30.0).abs() < 1e-9, "{sizes:?}");
    }

    #[test]
    fn shrink_is_proportional_to_the_inner_base_size() {
        let items = [item(100.0, 0.0, 0.0, 1.0, 0.0), item(100.0, 0.0, 0.0, 1.0, 0.0)];
        let sizes = resolve_flexible_lengths(&items, 100.0);
        assert!((sizes[0] - 50.0).abs() < 1e-9 && (sizes[1] - 50.0).abs() < 1e-9, "{sizes:?}");
        let clamped = [item(100.0, 0.0, 0.0, 1.0, 80.0), item(100.0, 0.0, 0.0, 1.0, 0.0)];
        let sizes = resolve_flexible_lengths(&clamped, 100.0);
        assert!((sizes[0] - 80.0).abs() < 1e-9 && (sizes[1] - 20.0).abs() < 1e-9, "{sizes:?}");
    }
}
