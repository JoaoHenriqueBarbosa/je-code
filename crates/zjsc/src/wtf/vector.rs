//! `shrinkToFit(T& segmentedVector)` de `bytecompiler/BytecodeGeneratorBaseInlines.h:37`.
//!
//! O C++ guarda os `LabelScope`/`Label`/registradores num `SegmentedVector` e, antes de alocar um
//! novo, remove do fim os elementos cujo `refCount()` chegou a zero. O `SegmentedVector` vira `Vec`.

use crate::wtf::ref_counted::RefCounted;

/// `while (segmentedVector.size() && !segmentedVector.last().refCount()) segmentedVector.removeLast();`
pub fn shrink_to_fit<T: RefCounted>(segmented_vector: &mut Vec<T>) {
    while segmented_vector.last().is_some_and(|last| last.ref_count() == 0) {
        segmented_vector.pop();
    }
}
