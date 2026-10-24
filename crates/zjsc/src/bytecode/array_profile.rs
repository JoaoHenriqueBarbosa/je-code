//! Porte de `bytecode/ArrayProfile.h` e `ArrayProfile.cpp`: o comportamento do `ArrayProfile`. O struct
//! (`ArrayProfile`, os quatro campos de 32 bits) vive em `bytecode/op_metadata.rs`, que o metadata do
//! bytecode precisa; este módulo acrescenta os `ArrayModes`, as `ArrayProfileFlag` e os métodos no
//! mesmo tipo.
//!
//! Divergências:
//!
//! - `StructureID` é o `u32` do `Structure::id()`; o 0 é o `StructureID()` vazio.
//! - `OptionSet<ArrayProfileFlag>` é o `u32` de bits do campo `array_profile_flags`, e cada flag é uma
//!   constante de [`ArrayProfileFlag`].
//! - `computeUpdatedPrediction`, `briefDescription*` e `dumpArrayModes` só alimentam o DFG e o dump do
//!   JIT (leem `JSGlobalObject::isOriginalTypedArrayStructure` e `regExpMatchesArrayStructure`, que o
//!   porte não tem). O LLInt só escreve no perfil (`observeStructureID`, `setOutOfBounds`,
//!   `observeIndexedRead`), então o consumidor fica de fora até o DFG existir. O mesmo vale para
//!   `UnlinkedArrayProfile::update`, que fica em `unlinked_code_block.rs`.

use crate::bytecode::op_metadata::ArrayProfile;
use crate::runtime::indexing_type::{
    has_any_array_storage, has_contiguous, has_double, has_int32, IndexingType, ARRAY_CLASS, ARRAY_STORAGE_SHAPE,
    ARRAY_WITH_ARRAY_STORAGE, ARRAY_WITH_CONTIGUOUS, ARRAY_WITH_DOUBLE, ARRAY_WITH_INT32,
    ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, ARRAY_WITH_UNDECIDED, CONTIGUOUS_SHAPE, COPY_ON_WRITE,
    COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS, COPY_ON_WRITE_ARRAY_WITH_DOUBLE, COPY_ON_WRITE_ARRAY_WITH_INT32,
    DOUBLE_SHAPE, INT32_SHAPE, NON_ARRAY, NON_ARRAY_WITH_ARRAY_STORAGE, NON_ARRAY_WITH_CONTIGUOUS,
    NON_ARRAY_WITH_DOUBLE, NON_ARRAY_WITH_INT32, NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, SLOW_PUT_ARRAY_STORAGE_SHAPE,
};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::JSString;
use crate::runtime::js_type::{is_typed_array_type, FIRST_TYPED_ARRAY_TYPE};
use crate::runtime::options::Options;
use crate::runtime::structure::Structure;

/// `typedef unsigned ArrayModes`: um bit por tipo de acesso a array visto.
pub type ArrayModes = u32;

// `static_assert(CopyOnWriteArrayWithInt32 == 21)` e as duas seguintes.
const _: () = {
    assert!(COPY_ON_WRITE_ARRAY_WITH_INT32 == 21);
    assert!(COPY_ON_WRITE_ARRAY_WITH_DOUBLE == 23);
    assert!(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS == 25);
};

pub const COPY_ON_WRITE_ARRAY_WITH_INT32_ARRAY_MODE: ArrayModes = 1 << COPY_ON_WRITE_ARRAY_WITH_INT32;
pub const COPY_ON_WRITE_ARRAY_WITH_DOUBLE_ARRAY_MODE: ArrayModes = 1 << COPY_ON_WRITE_ARRAY_WITH_DOUBLE;
pub const COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS_ARRAY_MODE: ArrayModes = 1 << COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS;

pub const INT8_ARRAY_MODE: ArrayModes = 1 << 16;
pub const INT16_ARRAY_MODE: ArrayModes = 1 << 17;
pub const INT32_ARRAY_MODE: ArrayModes = 1 << 18;
pub const UINT8_ARRAY_MODE: ArrayModes = 1 << 19;
pub const UINT8_CLAMPED_ARRAY_MODE: ArrayModes = 1 << 20;
// 21, 23 e 25 são os arrays copy-on-write.
pub const FLOAT16_ARRAY_MODE: ArrayModes = 1 << 22;
pub const UINT16_ARRAY_MODE: ArrayModes = 1 << 26;
pub const UINT32_ARRAY_MODE: ArrayModes = 1 << 27;
pub const FLOAT32_ARRAY_MODE: ArrayModes = 1 << 28;
pub const FLOAT64_ARRAY_MODE: ArrayModes = 1 << 29;
pub const BIG_INT64_ARRAY_MODE: ArrayModes = 1 << 30;
pub const BIG_UINT64_ARRAY_MODE: ArrayModes = 1 << 31;

/// `typedArrayModes`: na ordem do `TypedArrayType` (Int8, Uint8, Uint8Clamped, Int16, Uint16, Int32, Uint32,
/// Float16, Float32, Float64, BigInt64, BigUint64).
pub const TYPED_ARRAY_MODES: [ArrayModes; 12] = [
    INT8_ARRAY_MODE,
    UINT8_ARRAY_MODE,
    UINT8_CLAMPED_ARRAY_MODE,
    INT16_ARRAY_MODE,
    UINT16_ARRAY_MODE,
    INT32_ARRAY_MODE,
    UINT32_ARRAY_MODE,
    FLOAT16_ARRAY_MODE,
    FLOAT32_ARRAY_MODE,
    FLOAT64_ARRAY_MODE,
    BIG_INT64_ARRAY_MODE,
    BIG_UINT64_ARRAY_MODE,
];

/// `asArrayModesIgnoringTypedArrays`.
pub const fn as_array_modes_ignoring_typed_arrays(indexing_mode: IndexingType) -> ArrayModes {
    1u32 << indexing_mode as u32
}

/// `ALL_TYPED_ARRAY_MODES`.
pub const ALL_TYPED_ARRAY_MODES: ArrayModes = INT8_ARRAY_MODE
    | INT16_ARRAY_MODE
    | INT32_ARRAY_MODE
    | UINT8_ARRAY_MODE
    | UINT8_CLAMPED_ARRAY_MODE
    | UINT16_ARRAY_MODE
    | UINT32_ARRAY_MODE
    | FLOAT16_ARRAY_MODE
    | FLOAT32_ARRAY_MODE
    | FLOAT64_ARRAY_MODE
    | BIG_INT64_ARRAY_MODE
    | BIG_UINT64_ARRAY_MODE;

/// `ALL_NON_ARRAY_ARRAY_MODES`.
pub const ALL_NON_ARRAY_ARRAY_MODES: ArrayModes = as_array_modes_ignoring_typed_arrays(NON_ARRAY)
    | as_array_modes_ignoring_typed_arrays(NON_ARRAY_WITH_INT32)
    | as_array_modes_ignoring_typed_arrays(NON_ARRAY_WITH_DOUBLE)
    | as_array_modes_ignoring_typed_arrays(NON_ARRAY_WITH_CONTIGUOUS)
    | as_array_modes_ignoring_typed_arrays(NON_ARRAY_WITH_ARRAY_STORAGE)
    | as_array_modes_ignoring_typed_arrays(NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE)
    | ALL_TYPED_ARRAY_MODES;

/// `ALL_COPY_ON_WRITE_ARRAY_MODES`.
pub const ALL_COPY_ON_WRITE_ARRAY_MODES: ArrayModes = COPY_ON_WRITE_ARRAY_WITH_INT32_ARRAY_MODE
    | COPY_ON_WRITE_ARRAY_WITH_DOUBLE_ARRAY_MODE
    | COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS_ARRAY_MODE;

/// `ALL_WRITABLE_ARRAY_ARRAY_MODES`.
pub const ALL_WRITABLE_ARRAY_ARRAY_MODES: ArrayModes = as_array_modes_ignoring_typed_arrays(ARRAY_CLASS)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_UNDECIDED)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_INT32)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_DOUBLE)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_CONTIGUOUS)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_ARRAY_STORAGE)
    | as_array_modes_ignoring_typed_arrays(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE);

/// `ALL_ARRAY_ARRAY_MODES`.
pub const ALL_ARRAY_ARRAY_MODES: ArrayModes = ALL_WRITABLE_ARRAY_ARRAY_MODES | ALL_COPY_ON_WRITE_ARRAY_MODES;

/// `ALL_ARRAY_MODES`.
pub const ALL_ARRAY_MODES: ArrayModes = ALL_NON_ARRAY_ARRAY_MODES | ALL_ARRAY_ARRAY_MODES;

/// `arrayModesFromStructure`.
pub fn array_modes_from_structure(structure: &Structure) -> ArrayModes {
    let type_ = structure.type_info().type_();
    if is_typed_array_type(type_) {
        return TYPED_ARRAY_MODES[(type_ as u32 - FIRST_TYPED_ARRAY_TYPE) as usize];
    }
    as_array_modes_ignoring_typed_arrays(structure.indexing_mode())
}

/// `mergeArrayModes`: `true` se `left` mudou.
pub fn merge_array_modes(left: &mut ArrayModes, right: ArrayModes) -> bool {
    let new_modes = *left | right;
    if new_modes == *left {
        return false;
    }
    *left = new_modes;
    true
}

/// `arrayModesAreClearOrTop`.
pub fn array_modes_are_clear_or_top(modes: ArrayModes) -> bool {
    modes == 0 || modes == ALL_ARRAY_MODES
}

/// `arrayModesAlreadyChecked`: `proven` é subconjunto de `expected`.
pub fn array_modes_already_checked(proven: ArrayModes, expected: ArrayModes) -> bool {
    (expected | proven) == expected
}

/// `arrayModesIncludeIgnoringTypedArrays`.
pub fn array_modes_include_ignoring_typed_arrays(array_modes: ArrayModes, shape: IndexingType) -> bool {
    let mut modes = as_array_modes_ignoring_typed_arrays(NON_ARRAY | shape)
        | as_array_modes_ignoring_typed_arrays(ARRAY_CLASS | shape);
    if has_int32(shape) || has_double(shape) || has_contiguous(shape) {
        modes |= as_array_modes_ignoring_typed_arrays(ARRAY_CLASS | shape | COPY_ON_WRITE);
    }
    array_modes & modes != 0
}

/// `shouldUseSlowPutArrayStorage`.
pub fn should_use_slow_put_array_storage(array_modes: ArrayModes) -> bool {
    array_modes_include_ignoring_typed_arrays(array_modes, SLOW_PUT_ARRAY_STORAGE_SHAPE)
}

/// `shouldUseFastArrayStorage`.
pub fn should_use_fast_array_storage(array_modes: ArrayModes) -> bool {
    array_modes_include_ignoring_typed_arrays(array_modes, ARRAY_STORAGE_SHAPE)
}

/// `shouldUseContiguous`.
pub fn should_use_contiguous(array_modes: ArrayModes) -> bool {
    array_modes_include_ignoring_typed_arrays(array_modes, CONTIGUOUS_SHAPE)
}

/// `shouldUseDouble`: `ASSERT(Options::allowDoubleShape())`.
pub fn should_use_double(array_modes: ArrayModes) -> bool {
    debug_assert!(Options::with(|options| options.allow_double_shape));
    array_modes_include_ignoring_typed_arrays(array_modes, DOUBLE_SHAPE)
}

/// `shouldUseInt32`.
pub fn should_use_int32(array_modes: ArrayModes) -> bool {
    array_modes_include_ignoring_typed_arrays(array_modes, INT32_SHAPE)
}

/// `hasSeenArray`.
pub fn has_seen_array(array_modes: ArrayModes) -> bool {
    array_modes & ALL_ARRAY_ARRAY_MODES != 0
}

/// `hasSeenNonArray`.
pub fn has_seen_non_array(array_modes: ArrayModes) -> bool {
    array_modes & ALL_NON_ARRAY_ARRAY_MODES != 0
}

/// `hasSeenWritableArray`.
pub fn has_seen_writable_array(array_modes: ArrayModes) -> bool {
    array_modes & ALL_WRITABLE_ARRAY_ARRAY_MODES != 0
}

/// `hasSeenCopyOnWriteArray`.
pub fn has_seen_copy_on_write_array(array_modes: ArrayModes) -> bool {
    array_modes & ALL_COPY_ON_WRITE_ARRAY_MODES != 0
}

/// `enum class ArrayProfileFlag : uint32_t`: os bits do `OptionSet` de `array_profile_flags`.
pub struct ArrayProfileFlag;

impl ArrayProfileFlag {
    pub const MAY_STORE_HOLE: u32 = 1 << 0;
    pub const OUT_OF_BOUNDS: u32 = 1 << 1;
    pub const MAY_BE_LARGE_TYPED_ARRAY: u32 = 1 << 2;
    pub const MAY_INTERCEPT_INDEXED_ACCESSES: u32 = 1 << 3;
    pub const USES_NON_ORIGINAL_ARRAY_STRUCTURES: u32 = 1 << 4;
    pub const MAY_BE_RESIZABLE_OR_GROWABLE_SHARED_TYPED_ARRAY: u32 = 1 << 5;
    pub const DID_PERFORM_FIRST_RUN_PRUNING: u32 = 1 << 6;
    pub const MAY_BE_REG_EXP_MATCHES_ARRAY: u32 = 1 << 7;
}

impl ArrayProfile {
    /// `s_smallTypedArrayMaxLength`.
    pub const SMALL_TYPED_ARRAY_MAX_LENGTH: u64 = i32::MAX as u64;

    /// `clear()`.
    pub fn clear(&mut self) {
        *self = ArrayProfile::default();
    }

    fn contains(&self, flag: u32) -> bool {
        self.array_profile_flags & flag != 0
    }

    /// `setMayBeLargeTypedArray`.
    pub fn set_may_be_large_typed_array(&mut self) {
        self.array_profile_flags |= ArrayProfileFlag::MAY_BE_LARGE_TYPED_ARRAY;
    }

    /// `mayBeLargeTypedArray`.
    pub fn may_be_large_typed_array(&self) -> bool {
        self.contains(ArrayProfileFlag::MAY_BE_LARGE_TYPED_ARRAY)
    }

    /// `mayBeResizableOrGrowableSharedTypedArray`.
    pub fn may_be_resizable_or_growable_shared_typed_array(&self) -> bool {
        self.contains(ArrayProfileFlag::MAY_BE_RESIZABLE_OR_GROWABLE_SHARED_TYPED_ARRAY)
    }

    /// `setOutOfBounds`.
    pub fn set_out_of_bounds(&mut self) {
        self.array_profile_flags |= ArrayProfileFlag::OUT_OF_BOUNDS;
    }

    /// `setMayStoreHole`.
    pub fn set_may_store_hole(&mut self) {
        self.array_profile_flags |= ArrayProfileFlag::MAY_STORE_HOLE;
    }

    /// `observeStructureID`.
    pub fn observe_structure_id(&mut self, structure_id: u32) {
        self.last_seen_structure_id = structure_id;
    }

    /// `observeStructure`.
    pub fn observe_structure(&mut self, structure: &Structure) {
        self.last_seen_structure_id = structure.id();
    }

    /// `observeArrayMode`.
    pub fn observe_array_mode(&mut self, mode: ArrayModes) {
        self.observed_array_modes |= mode;
    }

    /// `observedArrayModes`.
    pub fn observed_array_modes(&self) -> ArrayModes {
        self.observed_array_modes
    }

    /// `mayInterceptIndexedAccesses`.
    pub fn may_intercept_indexed_accesses(&self) -> bool {
        self.contains(ArrayProfileFlag::MAY_INTERCEPT_INDEXED_ACCESSES)
    }

    /// `mayStoreToHole`.
    pub fn may_store_to_hole(&self) -> bool {
        self.contains(ArrayProfileFlag::MAY_STORE_HOLE)
    }

    /// `outOfBounds`.
    pub fn out_of_bounds(&self) -> bool {
        self.contains(ArrayProfileFlag::OUT_OF_BOUNDS)
    }

    /// `usesOriginalArrayStructures`.
    pub fn uses_original_array_structures(&self) -> bool {
        !self.contains(ArrayProfileFlag::USES_NON_ORIGINAL_ARRAY_STRUCTURES)
    }

    /// `mayBeRegExpMatchesArray`.
    pub fn may_be_reg_exp_matches_array(&self) -> bool {
        self.contains(ArrayProfileFlag::MAY_BE_REG_EXP_MATCHES_ARRAY)
    }

    /// `observeIndexedRead(JSCell*, index)` para um `JSObject`: grava a estrutura vista e marca
    /// `OutOfBounds` quando o índice passa do vetor (formas `ArrayStorage`) ou do comprimento.
    pub fn observe_indexed_read_object(&mut self, object: &JSObject, index: u32) {
        self.last_seen_structure_id = object.cell().structure_id();
        if has_any_array_storage(object.cell().indexing_type()) && index >= object.vector_length() {
            self.set_out_of_bounds();
        } else if index >= object.public_length() {
            self.set_out_of_bounds();
        }
    }

    /// `observeIndexedRead(JSCell*, index)` para um `JSString`: `OutOfBounds` quando o índice passa do
    /// comprimento.
    pub fn observe_indexed_read_string(&mut self, string: &JSString, index: u32) {
        if index >= string.length() {
            self.set_out_of_bounds();
        }
    }
}
