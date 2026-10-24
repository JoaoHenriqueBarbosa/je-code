//! Porte parcial de `WTF/wtf/StdLibExtras.h`: a busca binária (`BinarySearchMode`,
//! `binarySearchImpl`, `binarySearch`, `tryBinarySearch`, `approximateBinarySearch`).
//!
//! O C++ devolve um ponteiro para o elemento; aqui devolve o índice dele (ou `None` onde o C++
//! devolve nulo). `extractKey` é o fechamento `Fn(&T) -> K`. O resto do header (`bitwise_cast`,
//! `roundUpToMultipleOf` etc.) fica onde já foi portado (`math_extras`) ou ainda não tem chamador.

/// `enum BinarySearchMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinarySearchMode {
    KeyMustBePresentInArray,
    KeyMightNotBePresentInArray,
    ReturnAdjacentElementIfKeyIsNotPresent,
}

/// `binarySearchImpl`.
pub fn binary_search_impl<T, K: PartialOrd + Copy>(
    array: &[T],
    size: usize,
    key: K,
    extract_key: impl Fn(&T) -> K,
    mode: BinarySearchMode,
) -> Option<usize> {
    let mut size = size;
    let mut offset = 0usize;
    while size > 1 {
        let pos = (size - 1) >> 1;
        let val = extract_key(&array[offset + pos]);

        if val == key {
            return Some(offset + pos);
        }
        // The item we are looking for is smaller than the item being check; reduce the value of 'size',
        // chopping off the right hand half of the array.
        if key < val {
            size = pos;
        } else {
            // Discard all values in the left hand half of the array, up to and including the item at pos.
            size -= pos + 1;
            offset += pos + 1;
        }

        debug_assert!(mode != BinarySearchMode::KeyMustBePresentInArray || size != 0);
    }

    if mode == BinarySearchMode::KeyMightNotBePresentInArray && size == 0 {
        return None;
    }

    let result = offset;

    if mode == BinarySearchMode::KeyMightNotBePresentInArray && key != extract_key(&array[result]) {
        return None;
    }

    if mode == BinarySearchMode::KeyMustBePresentInArray {
        debug_assert!(size == 1);
        debug_assert!(key == extract_key(&array[result]));
    }

    Some(result)
}

/// `binarySearch`: a chave tem de estar no vetor.
pub fn binary_search<T, K: PartialOrd + Copy>(array: &[T], size: usize, key: K, extract_key: impl Fn(&T) -> K) -> Option<usize> {
    binary_search_impl(array, size, key, extract_key, BinarySearchMode::KeyMustBePresentInArray)
}

/// `tryBinarySearch`: `None` se a chave não estiver no vetor.
pub fn try_binary_search<T, K: PartialOrd + Copy>(array: &[T], size: usize, key: K, extract_key: impl Fn(&T) -> K) -> Option<usize> {
    binary_search_impl(array, size, key, extract_key, BinarySearchMode::KeyMightNotBePresentInArray)
}

/// `approximateBinarySearch`: o elemento à esquerda ou à direita de onde a chave estaria.
pub fn approximate_binary_search<T, K: PartialOrd + Copy>(array: &[T], size: usize, key: K, extract_key: impl Fn(&T) -> K) -> Option<usize> {
    binary_search_impl(array, size, key, extract_key, BinarySearchMode::ReturnAdjacentElementIfKeyIsNotPresent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_present_and_adjacent() {
        let array = [1u32, 4, 9, 16, 25];
        assert_eq!(try_binary_search(&array, array.len(), 9, |x| *x), Some(2));
        assert_eq!(try_binary_search(&array, array.len(), 10, |x| *x), None);
        let near = approximate_binary_search(&array, array.len(), 10, |x| *x).unwrap();
        assert!(near == 2 || near == 3);
    }
}
