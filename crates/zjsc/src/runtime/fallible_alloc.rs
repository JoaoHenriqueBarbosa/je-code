//! Alocação que falha sem abortar o processo.
//!
//! O `vec![x; n]` e o `Vec::with_capacity(n)` abortam o processo quando a alocação falha. No C++ o
//! `tryAllocate*` devolve `nullptr` e o chamador lança `OutOfMemoryError` (um `RangeError`); estas funções
//! são o equivalente: `None` é a falta de memória, e quem chama a converte em `Thrown::OutOfMemory`,
//! `PutError::OutOfMemory` ou `BigIntError::OutOfMemory`. Todo tamanho que vem de JS passa por aqui.

/// `Vec` com `capacity` posições reservadas, ou `None` se o alocador recusa (ou se o tamanho estoura).
pub fn try_vec_with_capacity<T>(capacity: usize) -> Option<Vec<T>> {
    let mut vector = Vec::new();
    vector.try_reserve_exact(capacity).ok()?;
    Some(vector)
}

/// O `WTF::Vector<T>::tryReserveCapacity(capacity)`: além do alocador, o `VectorBufferBase::tryAllocateBuffer` recusa
/// buffer de mais de `UINT_MAX` bytes (`newCapacity > std::numeric_limits<unsigned>::max() / sizeof(T)`), seja qual for
/// a memória da máquina.
pub fn try_wtf_vector_with_capacity<T>(capacity: usize) -> Option<Vec<T>> {
    if capacity > u32::MAX as usize / std::mem::size_of::<T>().max(1) {
        return None;
    }
    try_vec_with_capacity(capacity)
}

/// O `vec![value; length]` que devolve `None` em vez de abortar.
pub fn try_filled_vec<T: Clone>(value: T, length: usize) -> Option<Vec<T>> {
    let mut vector = try_vec_with_capacity(length)?;
    vector.resize(length, value);
    Some(vector)
}

/// `Vec<u8>` zerado de `length` bytes, ou `None` se o alocador recusa. O zero vem do `calloc` (`vec![0; n]`), cujas
/// páginas só ganham memória física quando tocadas: um `Int8Array(2 ** 32)` não pode custar 4 GiB residentes de
/// um `resize` (o `Gigacage::tryZeroedMalloc` do C++ também é preguiçoso). A sondagem `try_reserve_exact` primeiro
/// mantém a falha como `None` em vez do abort do `vec!`.
pub fn try_zeroed_bytes(length: usize) -> Option<Vec<u8>> {
    drop(try_vec_with_capacity::<u8>(length)?);
    Some(vec![0u8; length])
}

/// `String::with_capacity(capacity)` que devolve `None` em vez de abortar.
pub fn try_string_with_capacity(capacity: usize) -> Option<String> {
    let mut string = String::new();
    string.try_reserve_exact(capacity).ok()?;
    Some(string)
}

/// Estende `vector` até `new_length` com `value`, ou `false` se a memória falta (o vetor fica como estava).
pub fn try_resize<T: Clone>(vector: &mut Vec<T>, new_length: usize, value: T) -> bool {
    if new_length > vector.len() && vector.try_reserve_exact(new_length - vector.len()).is_err() {
        return false;
    }
    vector.resize(new_length, value);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_allocations_succeed() {
        assert_eq!(try_filled_vec(7u8, 3), Some(vec![7, 7, 7]));
        assert_eq!(try_vec_with_capacity::<u64>(4).map(|vector| vector.capacity() >= 4), Some(true));
        assert!(try_string_with_capacity(8).is_some());
        let mut vector = vec![1u8];
        assert!(try_resize(&mut vector, 3, 0));
        assert_eq!(vector, vec![1, 0, 0]);
    }

    #[test]
    fn impossible_allocations_fail_instead_of_aborting() {
        assert!(try_filled_vec(0u64, usize::MAX / 2).is_none());
        assert!(try_vec_with_capacity::<u64>(usize::MAX).is_none());
        assert!(try_string_with_capacity(usize::MAX).is_none());
        let mut vector = vec![1u64];
        assert!(!try_resize(&mut vector, usize::MAX / 8, 0));
        assert_eq!(vector, vec![1]);
    }
}
