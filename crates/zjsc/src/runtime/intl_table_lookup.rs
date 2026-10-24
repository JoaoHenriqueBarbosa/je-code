//! A busca por chave nas tabelas estáticas dos módulos `intl_*` (gerados por `scripts/gen-*.js` ou à mão),
//! num lugar só: ordenadas pela chave (busca binária) ou na ordem de declaração (busca linear).

use std::cmp::Ordering;

/// Se `key` está na lista ordenada de chaves `sorted`.
pub fn contains_sorted(sorted: &[&str], key: &str) -> bool {
    sorted.binary_search_by(|candidate| (*candidate).cmp(key)).is_ok()
}

/// A posição da linha cuja chave (`key` da linha) é `code`, numa lista de linhas ordenada pela chave.
pub fn sorted_position_by<T>(rows: &[T], key: impl Fn(&T) -> &str, code: &str) -> Option<usize> {
    rows.binary_search_by(|row| key(row).cmp(code)).ok()
}

/// O índice da chave `key` na tabela de pares `(chave, valor)` ordenada pela chave.
pub fn sorted_index<V>(table: &[(&str, V)], key: &str) -> Option<usize> {
    sorted_position_by(table, |row| row.0, key)
}

/// O valor da chave `key` na tabela de pares ordenada pela chave.
pub fn sorted_value<V: Copy>(table: &[(&str, V)], key: &str) -> Option<V> {
    sorted_index(table, key).map(|index| table[index].1)
}

/// O valor da chave `key` na tabela de pares em ordem qualquer.
pub fn linear_value<V: Copy>(table: &[(&str, V)], key: &str) -> Option<V> {
    table.iter().find(|(candidate, _)| *candidate == key).map(|(_, value)| *value)
}

/// Se `table` está ordenada pela chave de forma estritamente crescente (para o teste de cada tabela
/// que a busca binária pressupõe ordenada).
pub fn is_strictly_sorted<V>(table: &[(&str, V)]) -> bool {
    table.windows(2).all(|pair| pair[0].0.cmp(pair[1].0) == Ordering::Less)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_in_sorted_and_linear_tables() {
        let table = [("a", 1), ("c", 3), ("e", 5)];
        assert!(is_strictly_sorted(&table));
        assert_eq!(sorted_value(&table, "c"), Some(3));
        assert_eq!(sorted_value(&table, "d"), None);
        assert_eq!(linear_value(&table, "e"), Some(5));
        assert_eq!(sorted_index(&table, "a"), Some(0));
        assert!(contains_sorted(&["en", "pt"], "pt"));
        assert!(!contains_sorted(&["en", "pt"], "fr"));
    }
}
