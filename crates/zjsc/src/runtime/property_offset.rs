//! Tradução de `runtime/PropertyOffset.h`.
//!
//! `checkOffset`/`validateOffset` só têm `ASSERT` (sob `ASSERT_ENABLED`), então viram `debug_assert!`
//! de invariante, como o resto do porte.

/// `typedef int PropertyOffset`.
pub type PropertyOffset = i32;

pub const INVALID_OFFSET: PropertyOffset = -1;
pub const FIRST_OUT_OF_LINE_OFFSET: PropertyOffset = 64;
pub const KNOWN_POLY_PROTO_OFFSET: PropertyOffset = 0;

/// `checkOffset(PropertyOffset)`.
pub fn check_offset(offset: PropertyOffset) {
    debug_assert!(offset >= INVALID_OFFSET);
}

/// `checkOffset(PropertyOffset, int inlineCapacity)`.
pub fn check_offset_with_inline_capacity(offset: PropertyOffset, inline_capacity: i32) {
    debug_assert!(offset >= INVALID_OFFSET);
    debug_assert!(offset == INVALID_OFFSET || offset < inline_capacity || is_out_of_line_offset(offset));
}

/// `validateOffset(PropertyOffset)`.
pub fn validate_offset(offset: PropertyOffset) {
    check_offset(offset);
    debug_assert!(is_valid_offset(offset));
}

/// `validateOffset(PropertyOffset, int inlineCapacity)`.
pub fn validate_offset_with_inline_capacity(offset: PropertyOffset, inline_capacity: i32) {
    check_offset_with_inline_capacity(offset, inline_capacity);
    debug_assert!(is_valid_offset(offset));
}

/// `isValidOffset`.
pub fn is_valid_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    offset != INVALID_OFFSET
}

/// `isInlineOffset`.
pub fn is_inline_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    offset < FIRST_OUT_OF_LINE_OFFSET
}

/// `isOutOfLineOffset`.
pub fn is_out_of_line_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    !is_inline_offset(offset)
}

/// `offsetInInlineStorage`.
pub fn offset_in_inline_storage(offset: PropertyOffset) -> isize {
    validate_offset(offset);
    debug_assert!(is_inline_offset(offset));
    offset as isize
}

/// `offsetInOutOfLineStorage`: o índice negativo a partir do butterfly. O porte guarda o armazenamento
/// fora de linha num `Vec` em que a posição é `-índice - 1`, veja `out_of_line_index`.
pub fn offset_in_out_of_line_storage(offset: PropertyOffset) -> isize {
    validate_offset(offset);
    debug_assert!(is_out_of_line_offset(offset));
    -((offset - FIRST_OUT_OF_LINE_OFFSET) as isize) - 1
}

/// `offsetInRespectiveStorage`.
pub fn offset_in_respective_storage(offset: PropertyOffset) -> isize {
    if is_inline_offset(offset) {
        return offset_in_inline_storage(offset);
    }
    offset_in_out_of_line_storage(offset)
}

/// Posição no `Vec` do armazenamento fora de linha: `offset - firstOutOfLineOffset`. É o mesmo que
/// `-offsetInOutOfLineStorage(offset) - 1`; o C++ cresce o butterfly para índices negativos, o `Vec`
/// cresce para os positivos.
pub fn out_of_line_index(offset: PropertyOffset) -> usize {
    validate_offset(offset);
    debug_assert!(is_out_of_line_offset(offset));
    (offset - FIRST_OUT_OF_LINE_OFFSET) as usize
}

/// `numberOfOutOfLineSlotsForMaxOffset`.
pub fn number_of_out_of_line_slots_for_max_offset(offset: PropertyOffset) -> usize {
    check_offset(offset);
    if offset < FIRST_OUT_OF_LINE_OFFSET {
        return 0;
    }
    (offset - FIRST_OUT_OF_LINE_OFFSET + 1) as usize
}

/// `numberOfSlotsForMaxOffset`.
pub fn number_of_slots_for_max_offset(offset: PropertyOffset, inline_capacity: i32) -> usize {
    check_offset_with_inline_capacity(offset, inline_capacity);
    if offset < inline_capacity {
        return (offset + 1) as usize;
    }
    inline_capacity as usize + number_of_out_of_line_slots_for_max_offset(offset)
}

/// `offsetForPropertyNumber`.
pub fn offset_for_property_number(property_number: i32, inline_capacity: i32) -> PropertyOffset {
    let mut offset: PropertyOffset = property_number;
    if offset >= inline_capacity {
        offset += FIRST_OUT_OF_LINE_OFFSET;
        offset -= inline_capacity;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets() {
        assert!(!is_valid_offset(INVALID_OFFSET));
        assert!(is_inline_offset(63));
        assert!(is_out_of_line_offset(64));
        assert_eq!(offset_in_out_of_line_storage(64), -1);
        assert_eq!(offset_in_out_of_line_storage(65), -2);
        assert_eq!(out_of_line_index(65), 1);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(63), 0);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(64), 1);
        assert_eq!(number_of_slots_for_max_offset(5, 6), 6);
        assert_eq!(number_of_slots_for_max_offset(64, 6), 7);
        // Com capacidade inline 6, a propriedade número 6 é a primeira fora de linha.
        assert_eq!(offset_for_property_number(5, 6), 5);
        assert_eq!(offset_for_property_number(6, 6), 64);
        assert_eq!(offset_for_property_number(7, 6), 65);
        assert_eq!(offset_for_property_number(0, 0), 64);
    }
}
