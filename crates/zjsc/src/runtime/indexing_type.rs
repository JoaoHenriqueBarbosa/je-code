//! Constantes e predicados puros de `runtime/IndexingType.h`, mais `indexingTypeForValue`,
//! `leastUpperBoundOfIndexingTypes` e `leastUpperBoundOfIndexingTypeAndValue`, as três que o
//! bytecompiler usa. `dumpIndexingType` e as variantes de especulação (DFG) não entram aqui.

/// `typedef uint8_t IndexingType`.
pub type IndexingType = u8;

pub const IS_ARRAY: IndexingType = 0x01;

pub const NO_INDEXING_SHAPE: IndexingType = 0x00;
pub const UNDECIDED_SHAPE: IndexingType = 0x02;
pub const INT32_SHAPE: IndexingType = 0x04;
pub const DOUBLE_SHAPE: IndexingType = 0x06;
pub const CONTIGUOUS_SHAPE: IndexingType = 0x08;
pub const ARRAY_STORAGE_SHAPE: IndexingType = 0x0A;
pub const SLOW_PUT_ARRAY_STORAGE_SHAPE: IndexingType = 0x0C;

pub const INDEXING_SHAPE_MASK: IndexingType = 0x0E;
pub const INDEXING_SHAPE_SHIFT: IndexingType = 1;
pub const NUMBER_OF_INDEXING_SHAPES: IndexingType = 7;
pub const INDEXING_TYPE_MASK: IndexingType = INDEXING_SHAPE_MASK | IS_ARRAY;

pub const COPY_ON_WRITE: IndexingType = 0x10;
pub const INDEXING_SHAPE_AND_WRITABILITY_MASK: IndexingType = COPY_ON_WRITE | INDEXING_SHAPE_MASK;
pub const INDEXING_MODE_MASK: IndexingType = COPY_ON_WRITE | INDEXING_TYPE_MASK;
pub const NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES: IndexingType = 3;
pub const NUMBER_OF_ARRAY_INDEXING_MODES: IndexingType =
    NUMBER_OF_INDEXING_SHAPES + NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES;

pub const MAY_HAVE_INDEXED_ACCESSORS: IndexingType = 0x20;

pub const INDEXING_TYPE_LOCK_IS_HELD: IndexingType = 0x40;
pub const INDEXING_TYPE_LOCK_HAS_PARKED: IndexingType = 0x80;

pub const NON_ARRAY: IndexingType = 0x0;
pub const NON_ARRAY_WITH_INT32: IndexingType = INT32_SHAPE;
pub const NON_ARRAY_WITH_DOUBLE: IndexingType = DOUBLE_SHAPE;
pub const NON_ARRAY_WITH_CONTIGUOUS: IndexingType = CONTIGUOUS_SHAPE;
pub const NON_ARRAY_WITH_ARRAY_STORAGE: IndexingType = ARRAY_STORAGE_SHAPE;
pub const NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = SLOW_PUT_ARRAY_STORAGE_SHAPE;
/// `ArrayClass`.
pub const ARRAY_CLASS: IndexingType = IS_ARRAY;
pub const ARRAY_WITH_UNDECIDED: IndexingType = IS_ARRAY | UNDECIDED_SHAPE;
pub const ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE;
pub const ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE;
pub const ARRAY_WITH_CONTIGUOUS: IndexingType = IS_ARRAY | CONTIGUOUS_SHAPE;
pub const ARRAY_WITH_ARRAY_STORAGE: IndexingType = IS_ARRAY | ARRAY_STORAGE_SHAPE;
pub const ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = IS_ARRAY | SLOW_PUT_ARRAY_STORAGE_SHAPE;
pub const COPY_ON_WRITE_ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE | COPY_ON_WRITE;
pub const COPY_ON_WRITE_ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE | COPY_ON_WRITE;
pub const COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS: IndexingType = IS_ARRAY | CONTIGUOUS_SHAPE | COPY_ON_WRITE;

pub const ALL_WRITABLE_ARRAY_TYPES: IndexingType = INDEXING_SHAPE_MASK | IS_ARRAY;
pub const ALL_ARRAY_TYPES: IndexingType = ALL_WRITABLE_ARRAY_TYPES | COPY_ON_WRITE;
pub const ALL_WRITABLE_ARRAY_TYPES_AND_HISTORY: IndexingType =
    ALL_WRITABLE_ARRAY_TYPES | MAY_HAVE_INDEXED_ACCESSORS;
pub const ALL_ARRAY_TYPES_AND_HISTORY: IndexingType = ALL_ARRAY_TYPES | MAY_HAVE_INDEXED_ACCESSORS;

pub const fn has_indexed_properties(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) != NO_INDEXING_SHAPE
}

pub const fn has_undecided(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == UNDECIDED_SHAPE
}

pub const fn has_int32(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == INT32_SHAPE
}

pub const fn has_double(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == DOUBLE_SHAPE
}

pub const fn has_contiguous(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == CONTIGUOUS_SHAPE
}

pub const fn has_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == ARRAY_STORAGE_SHAPE
}

pub const fn has_any_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) >= ARRAY_STORAGE_SHAPE
}

pub const fn has_slow_put_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == SLOW_PUT_ARRAY_STORAGE_SHAPE
}

/// `shouldUseSlowPut` é só `hasSlowPutArrayStorage`.
pub use self::has_slow_put_array_storage as should_use_slow_put;

pub const fn is_copy_on_write(indexing_mode: IndexingType) -> bool {
    indexing_mode & COPY_ON_WRITE != 0
}

pub const fn array_index_from_indexing_type(indexing_type: IndexingType) -> u32 {
    if is_copy_on_write(indexing_type) {
        return (((indexing_type & INDEXING_SHAPE_MASK) as u32) - UNDECIDED_SHAPE as u32
            + SLOW_PUT_ARRAY_STORAGE_SHAPE as u32)
            >> INDEXING_SHAPE_SHIFT;
    }
    ((indexing_type & INDEXING_SHAPE_MASK) >> INDEXING_SHAPE_SHIFT) as u32
}

/// `indexingTypeForValue(JSValue)` de `IndexingTypeInlines.h`.
pub fn indexing_type_for_value(value: crate::runtime::js_value::JSValue) -> IndexingType {
    if value.is_int32() {
        return INT32_SHAPE;
    }

    if value.is_number()
        && value.as_number() == value.as_number()
        && crate::runtime::options::Options::with(|options| options.allow_double_shape)
    {
        return DOUBLE_SHAPE;
    }

    CONTIGUOUS_SHAPE
}

/// `leastUpperBoundOfIndexingTypes(IndexingType, IndexingType)` de `IndexingType.cpp`.
pub fn least_upper_bound_of_indexing_types(a: IndexingType, b: IndexingType) -> IndexingType {
    // It doesn't make sense to LUB something that is an array with something that isn't.
    debug_assert!((a & IS_ARRAY) == (b & IS_ARRAY));

    // Boy, this sure is easy right now.
    a.max(b)
}

/// `dumpIndexingType` de `IndexingType.cpp` (o `IndexingTypeDump` do `Structure::dump`).
pub fn dump_indexing_type(indexing_type: IndexingType) -> String {
    let basic_name = match indexing_type & ALL_ARRAY_TYPES {
        NON_ARRAY => "NonArray",
        NON_ARRAY_WITH_INT32 => "NonArrayWithInt32",
        NON_ARRAY_WITH_DOUBLE => "NonArrayWithDouble",
        NON_ARRAY_WITH_CONTIGUOUS => "NonArrayWithContiguous",
        NON_ARRAY_WITH_ARRAY_STORAGE => "NonArrayWithArrayStorage",
        NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE => "NonArrayWithSlowPutArrayStorage",
        ARRAY_CLASS => "ArrayClass",
        ARRAY_WITH_UNDECIDED => "ArrayWithUndecided",
        ARRAY_WITH_INT32 => "ArrayWithInt32",
        ARRAY_WITH_DOUBLE => "ArrayWithDouble",
        ARRAY_WITH_CONTIGUOUS => "ArrayWithContiguous",
        ARRAY_WITH_ARRAY_STORAGE => "ArrayWithArrayStorage",
        ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE => "ArrayWithSlowPutArrayStorage",
        COPY_ON_WRITE_ARRAY_WITH_INT32 => "CopyOnWriteArrayWithInt32",
        COPY_ON_WRITE_ARRAY_WITH_DOUBLE => "CopyOnWriteArrayWithDouble",
        COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS => "CopyOnWriteArrayWithContiguous",
        _ => "Unknown!",
    };
    let suffix = if indexing_type & MAY_HAVE_INDEXED_ACCESSORS != 0 { "|MayHaveIndexedAccessors" } else { "" };
    format!("{basic_name}{suffix}")
}

/// `leastUpperBoundOfIndexingTypeAndValue(IndexingType, JSValue)` de `IndexingType.cpp`.
pub fn least_upper_bound_of_indexing_type_and_value(
    indexing_type: IndexingType,
    value: crate::runtime::js_value::JSValue,
) -> IndexingType {
    least_upper_bound_of_indexing_types(indexing_type, indexing_type_for_value(value) | (indexing_type & IS_ARRAY))
}
