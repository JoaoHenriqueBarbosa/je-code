//! Porte de `bytecode/SpeculatedType.h`: os bits de `SpeculatedType` e os predicados puros.
//! O que depende de `ClassInfo`, `Structure`, `JSValue`, `TypedArrayType` ou de `PrintStream`
//! (`speculationFromCell`, `dumpSpeculation`, `typeOfDouble*` e afins, do `.cpp`) não entra aqui.

/// `typedef uint64_t SpeculatedType`.
pub type SpeculatedType = u64;

/// Não sabemos nada ainda.
pub const SPEC_NONE: SpeculatedType = 0;
pub const SPEC_FINAL_OBJECT: SpeculatedType = 1 << 0;
pub const SPEC_ARRAY: SpeculatedType = 1 << 1;
pub const SPEC_FUNCTION: SpeculatedType = 1 << 2;
pub const SPEC_INT8_ARRAY: SpeculatedType = 1 << 4;
pub const SPEC_INT16_ARRAY: SpeculatedType = 1 << 5;
pub const SPEC_INT32_ARRAY: SpeculatedType = 1 << 6;
pub const SPEC_UINT8_ARRAY: SpeculatedType = 1 << 7;
pub const SPEC_UINT8_CLAMPED_ARRAY: SpeculatedType = 1 << 8;
pub const SPEC_UINT16_ARRAY: SpeculatedType = 1 << 9;
pub const SPEC_UINT32_ARRAY: SpeculatedType = 1 << 10;
pub const SPEC_FLOAT16_ARRAY: SpeculatedType = 1 << 11;
pub const SPEC_FLOAT32_ARRAY: SpeculatedType = 1 << 12;
pub const SPEC_FLOAT64_ARRAY: SpeculatedType = 1 << 13;
pub const SPEC_BIG_INT64_ARRAY: SpeculatedType = 1 << 14;
pub const SPEC_BIG_UINT64_ARRAY: SpeculatedType = 1 << 15;
pub const SPEC_TYPED_ARRAY_VIEW: SpeculatedType = SPEC_INT8_ARRAY
    | SPEC_INT16_ARRAY
    | SPEC_INT32_ARRAY
    | SPEC_UINT8_ARRAY
    | SPEC_UINT8_CLAMPED_ARRAY
    | SPEC_UINT16_ARRAY
    | SPEC_UINT32_ARRAY
    | SPEC_FLOAT16_ARRAY
    | SPEC_FLOAT32_ARRAY
    | SPEC_FLOAT64_ARRAY
    | SPEC_BIG_INT64_ARRAY
    | SPEC_BIG_UINT64_ARRAY;
pub const SPEC_DIRECT_ARGUMENTS: SpeculatedType = 1 << 16;
pub const SPEC_SCOPED_ARGUMENTS: SpeculatedType = 1 << 17;
pub const SPEC_STRING_OBJECT: SpeculatedType = 1 << 18;
/// `RegExpObject`, e não uma subclasse dele.
pub const SPEC_REG_EXP_OBJECT: SpeculatedType = 1 << 19;
pub const SPEC_DATE_OBJECT: SpeculatedType = 1 << 20;
pub const SPEC_PROMISE_OBJECT: SpeculatedType = 1 << 21;
pub const SPEC_MAP_OBJECT: SpeculatedType = 1 << 22;
pub const SPEC_SET_OBJECT: SpeculatedType = 1 << 23;
pub const SPEC_MAP_ITERATOR_OBJECT: SpeculatedType = 1 << 24;
pub const SPEC_SET_ITERATOR_OBJECT: SpeculatedType = 1 << 25;
pub const SPEC_WEAK_MAP_OBJECT: SpeculatedType = 1 << 26;
pub const SPEC_WEAK_SET_OBJECT: SpeculatedType = 1 << 27;
pub const SPEC_PROXY_OBJECT: SpeculatedType = 1 << 28;
pub const SPEC_GLOBAL_PROXY: SpeculatedType = 1 << 29;
pub const SPEC_DERIVED_ARRAY: SpeculatedType = 1 << 30;
/// Objeto, mas não `JSFinalObject`, `JSArray` nem `JSFunction`.
pub const SPEC_OBJECT_OTHER: SpeculatedType = 1 << 31;
pub const SPEC_STRING_IDENT: SpeculatedType = 1 << 32;
pub const SPEC_STRING_RESOLVED_VAR: SpeculatedType = 1 << 33;
pub const SPEC_STRING_UNRESOLVED_VAR: SpeculatedType = 1 << 34;
pub const SPEC_STRING_VAR: SpeculatedType = SPEC_STRING_UNRESOLVED_VAR | SPEC_STRING_RESOLVED_VAR;
pub const SPEC_STRING_RESOLVED: SpeculatedType = SPEC_STRING_IDENT | SPEC_STRING_RESOLVED_VAR;
pub const SPEC_STRING: SpeculatedType = SPEC_STRING_IDENT | SPEC_STRING_VAR;
pub const SPEC_SYMBOL: SpeculatedType = 1 << 35;
/// `JSCell` que não é `JSObject`, nem `JSString`, `BigInt` ou `Symbol`.
pub const SPEC_CELL_OTHER: SpeculatedType = 1 << 36;
pub const SPEC_BOOL_INT32: SpeculatedType = 1 << 37;
pub const SPEC_NON_BOOL_INT32: SpeculatedType = 1 << 38;
pub const SPEC_INT32_ONLY: SpeculatedType = SPEC_BOOL_INT32 | SPEC_NON_BOOL_INT32;

pub const SPEC_INT32_AS_INT52: SpeculatedType = 1 << 39;
pub const SPEC_NON_INT32_AS_INT52: SpeculatedType = 1 << 40;
pub const SPEC_INT52_ANY: SpeculatedType = SPEC_INT32_AS_INT52 | SPEC_NON_INT32_AS_INT52;

pub const SPEC_ANY_INT_AS_DOUBLE: SpeculatedType = 1 << 41;
pub const SPEC_NON_INT_AS_DOUBLE: SpeculatedType = 1 << 42;
pub const SPEC_DOUBLE_REAL: SpeculatedType = SPEC_NON_INT_AS_DOUBLE | SPEC_ANY_INT_AS_DOUBLE;
pub const SPEC_DOUBLE_PURE_NAN: SpeculatedType = 1 << 43;
pub const SPEC_DOUBLE_IMPURE_NAN: SpeculatedType = 1 << 44;
pub const SPEC_DOUBLE_NAN: SpeculatedType = SPEC_DOUBLE_PURE_NAN | SPEC_DOUBLE_IMPURE_NAN;
pub const SPEC_BYTECODE_DOUBLE: SpeculatedType = SPEC_DOUBLE_REAL | SPEC_DOUBLE_PURE_NAN;
pub const SPEC_FULL_DOUBLE: SpeculatedType = SPEC_DOUBLE_REAL | SPEC_DOUBLE_NAN;
pub const SPEC_BYTECODE_REAL_NUMBER: SpeculatedType = SPEC_INT32_ONLY | SPEC_DOUBLE_REAL;
pub const SPEC_FULL_REAL_NUMBER: SpeculatedType = SPEC_INT32_ONLY | SPEC_INT52_ANY | SPEC_DOUBLE_REAL;
pub const SPEC_BYTECODE_NUMBER: SpeculatedType = SPEC_INT32_ONLY | SPEC_BYTECODE_DOUBLE;
pub const SPEC_INT_ANY_FORMAT: SpeculatedType = SPEC_INT52_ANY | SPEC_INT32_ONLY | SPEC_ANY_INT_AS_DOUBLE;

pub const SPEC_FULL_NUMBER: SpeculatedType = SPEC_INT_ANY_FORMAT | SPEC_FULL_DOUBLE;
pub const SPEC_BOOLEAN: SpeculatedType = 1 << 45;
/// `Null` ou `Undefined`.
pub const SPEC_OTHER: SpeculatedType = 1 << 46;
pub const SPEC_MISC: SpeculatedType = SPEC_BOOLEAN | SPEC_OTHER;
pub const SPEC_EMPTY: SpeculatedType = 1 << 47;
pub const SPEC_HEAP_BIG_INT: SpeculatedType = 1 << 48;
pub const SPEC_BIG_INT32: SpeculatedType = 1 << 49;
/// `USE(BIGINT32)` é 0 em `PlatformUse.h`: o `SpecBigInt` não inclui o `SpecBigInt32`.
pub const SPEC_BIG_INT: SpeculatedType = SPEC_HEAP_BIG_INT;
pub const SPEC_DATA_VIEW_OBJECT: SpeculatedType = 1 << 50;
pub const SPEC_PRIMITIVE: SpeculatedType =
    SPEC_STRING | SPEC_SYMBOL | SPEC_BYTECODE_NUMBER | SPEC_MISC | SPEC_BIG_INT;
pub const SPEC_OBJECT: SpeculatedType = SPEC_FINAL_OBJECT
    | SPEC_ARRAY
    | SPEC_FUNCTION
    | SPEC_TYPED_ARRAY_VIEW
    | SPEC_DIRECT_ARGUMENTS
    | SPEC_SCOPED_ARGUMENTS
    | SPEC_STRING_OBJECT
    | SPEC_REG_EXP_OBJECT
    | SPEC_DATE_OBJECT
    | SPEC_PROMISE_OBJECT
    | SPEC_MAP_OBJECT
    | SPEC_SET_OBJECT
    | SPEC_MAP_ITERATOR_OBJECT
    | SPEC_SET_ITERATOR_OBJECT
    | SPEC_WEAK_MAP_OBJECT
    | SPEC_WEAK_SET_OBJECT
    | SPEC_PROXY_OBJECT
    | SPEC_GLOBAL_PROXY
    | SPEC_DERIVED_ARRAY
    | SPEC_OBJECT_OTHER
    | SPEC_DATA_VIEW_OBJECT;
pub const SPEC_CELL: SpeculatedType =
    SPEC_OBJECT | SPEC_STRING | SPEC_SYMBOL | SPEC_CELL_OTHER | SPEC_HEAP_BIG_INT;
pub const SPEC_HEAP_TOP: SpeculatedType = SPEC_CELL | SPEC_BIG_INT32 | SPEC_BYTECODE_NUMBER | SPEC_MISC;
pub const SPEC_BYTECODE_TOP: SpeculatedType = SPEC_HEAP_TOP | SPEC_EMPTY;
pub const SPEC_FULL_TOP: SpeculatedType = SPEC_BYTECODE_TOP | SPEC_FULL_NUMBER;

pub const SPEC_TYPEOF_MIGHT_BE_FUNCTION: SpeculatedType =
    SPEC_FUNCTION | SPEC_OBJECT_OTHER | SPEC_PROXY_OBJECT;

/// O conjunto de tipos que passa por um teste de célula. Em 64 bits o valor vazio passa; o
/// complemento é o que passa no teste de "não é célula".
pub const SPEC_CELL_CHECK: SpeculatedType = SPEC_CELL | SPEC_EMPTY;

/// `typedef bool (*SpeculatedTypeChecker)(SpeculatedType)`.
pub type SpeculatedTypeChecker = fn(SpeculatedType) -> bool;

/// Verificador de predição que aceita tudo, para quem insiste em exigir um.
pub fn is_any_speculation(_: SpeculatedType) -> bool {
    true
}

pub fn is_subtype_speculation(value: SpeculatedType, category: SpeculatedType) -> bool {
    value & !category == 0 && value != 0
}

pub fn speculation_contains(value: SpeculatedType, category: SpeculatedType) -> bool {
    value & category != 0 && value != 0
}

pub fn is_cell_speculation(value: SpeculatedType) -> bool {
    value & SPEC_CELL != 0 && value & !SPEC_CELL == 0
}

pub fn is_cell_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !(SPEC_CELL | SPEC_OTHER) == 0
}

pub fn is_not_cell_speculation(value: SpeculatedType) -> bool {
    value & SPEC_CELL_CHECK == 0 && value != 0
}

pub fn is_not_cell_nor_big_int_speculation(value: SpeculatedType) -> bool {
    value & (SPEC_CELL_CHECK | SPEC_BIG_INT) == 0 && value != 0
}

pub fn is_object_speculation(value: SpeculatedType) -> bool {
    value & SPEC_OBJECT != 0 && value & !SPEC_OBJECT == 0
}

pub fn is_object_or_other_speculation(value: SpeculatedType) -> bool {
    value & (SPEC_OBJECT | SPEC_OTHER) != 0 && value & !(SPEC_OBJECT | SPEC_OTHER) == 0
}

pub fn is_final_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FINAL_OBJECT
}

pub fn is_final_object_or_other_speculation(value: SpeculatedType) -> bool {
    value & (SPEC_FINAL_OBJECT | SPEC_OTHER) != 0
        && value & !(SPEC_FINAL_OBJECT | SPEC_OTHER) == 0
}

pub fn is_string_ident_speculation(value: SpeculatedType) -> bool {
    value == SPEC_STRING_IDENT
}

pub fn is_not_string_var_speculation(value: SpeculatedType) -> bool {
    value & SPEC_STRING_VAR == 0
}

pub fn is_string_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_STRING == value
}

pub fn is_not_string_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_STRING == 0
}

pub fn is_string_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & (SPEC_STRING | SPEC_OTHER) == value
}

pub fn is_symbol_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SYMBOL
}

pub fn is_big_int32_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_INT32
}

pub fn is_heap_big_int_speculation(value: SpeculatedType) -> bool {
    value == SPEC_HEAP_BIG_INT
}

pub fn is_big_int_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_BIG_INT == value
}

pub fn is_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_ARRAY
}

pub fn is_function_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FUNCTION
}

pub fn is_proxy_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_PROXY_OBJECT
}

pub fn is_set_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SET_OBJECT
}

pub fn is_global_proxy_speculation(value: SpeculatedType) -> bool {
    value == SPEC_GLOBAL_PROXY
}

pub fn is_derived_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_DERIVED_ARRAY
}

pub fn is_int8_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT8_ARRAY
}

pub fn is_int16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT16_ARRAY
}

pub fn is_int32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT32_ARRAY
}

pub fn is_uint8_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT8_ARRAY
}

pub fn is_uint8_clamped_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT8_CLAMPED_ARRAY
}

pub fn is_uint16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT16_ARRAY
}

pub fn is_uint32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT32_ARRAY
}

pub fn is_float16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT16_ARRAY
}

pub fn is_float32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT32_ARRAY
}

pub fn is_float64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT64_ARRAY
}

pub fn is_big_int64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_INT64_ARRAY
}

pub fn is_big_uint64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_UINT64_ARRAY
}

pub fn is_direct_arguments_speculation(value: SpeculatedType) -> bool {
    value == SPEC_DIRECT_ARGUMENTS
}

pub fn is_scoped_arguments_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SCOPED_ARGUMENTS
}

pub fn is_array_or_other_speculation(value: SpeculatedType) -> bool {
    value & (SPEC_ARRAY | SPEC_OTHER) != 0 && value & !(SPEC_ARRAY | SPEC_OTHER) == 0
}

pub fn is_string_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_STRING_OBJECT
}

pub fn is_string_or_string_object_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !(SPEC_STRING | SPEC_STRING_OBJECT) == 0
}

pub fn is_reg_exp_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_REG_EXP_OBJECT
}

pub fn is_bool_int32_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BOOL_INT32
}

pub fn is_int32_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !SPEC_INT32_ONLY == 0
}

pub fn is_not_int32_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_INT32_ONLY == 0
}

pub fn is_int32_or_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !(SPEC_BOOLEAN | SPEC_INT32_ONLY) == 0
}

pub fn is_int32_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    value & (SPEC_FULL_DOUBLE | SPEC_NON_INT32_AS_INT52 | SPEC_BIG_INT) == 0
}

pub fn is_int32_or_boolean_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    value & (SPEC_FULL_DOUBLE | SPEC_NON_INT32_AS_INT52 | SPEC_BIG_INT) == 0
}

pub fn is_int32_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !(SPEC_INT32_ONLY | SPEC_OTHER) == 0
}

pub fn is_int32_or_boolean_speculation_expecting_defined(value: SpeculatedType) -> bool {
    is_int32_or_boolean_speculation(value & !SPEC_OTHER)
}

pub fn is_any_int52_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_INT52_ANY == value
}

pub fn is_int32_or_int52_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & (SPEC_INT32_ONLY | SPEC_INT52_ANY) == value
}

pub fn is_int32_or_int52_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & (SPEC_INT32_ONLY | SPEC_INT52_ANY | SPEC_OTHER) == value
}

pub fn is_int_any_format(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_INT_ANY_FORMAT == value
}

pub fn is_any_int_as_double_speculation(value: SpeculatedType) -> bool {
    value == SPEC_ANY_INT_AS_DOUBLE
}

pub fn is_double_real_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_DOUBLE_REAL == value
}

pub fn is_double_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_FULL_DOUBLE == value
}

pub fn is_double_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    value & SPEC_FULL_DOUBLE != 0
}

pub fn is_bytecode_real_number_speculation(value: SpeculatedType) -> bool {
    value & SPEC_BYTECODE_REAL_NUMBER != 0 && value & !SPEC_BYTECODE_REAL_NUMBER == 0
}

pub fn is_full_real_number_speculation(value: SpeculatedType) -> bool {
    value & SPEC_FULL_REAL_NUMBER != 0 && value & !SPEC_FULL_REAL_NUMBER == 0
}

pub fn is_bytecode_number_speculation(value: SpeculatedType) -> bool {
    value & SPEC_BYTECODE_NUMBER != 0 && value & !SPEC_BYTECODE_NUMBER == 0
}

pub fn is_full_number_speculation(value: SpeculatedType) -> bool {
    value & SPEC_FULL_NUMBER != 0 && value & !SPEC_FULL_NUMBER == 0
}

pub fn is_full_number_or_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN) == 0
}

pub fn is_full_number_or_boolean_speculation_expecting_defined(value: SpeculatedType) -> bool {
    is_full_number_or_boolean_speculation(value & !SPEC_OTHER)
}

pub fn is_boolean_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BOOLEAN
}

pub fn is_not_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & SPEC_BOOLEAN == 0
}

pub fn is_not_double_speculation(type_: SpeculatedType) -> bool {
    type_ & SPEC_FULL_DOUBLE == 0
}

pub fn is_neither_double_nor_heap_big_int_nor_string_speculation(type_: SpeculatedType) -> bool {
    type_ & (SPEC_FULL_DOUBLE | SPEC_HEAP_BIG_INT | SPEC_STRING) == 0
}

pub fn is_neither_double_nor_heap_big_int_speculation(type_: SpeculatedType) -> bool {
    type_ & (SPEC_FULL_DOUBLE | SPEC_HEAP_BIG_INT) == 0
}

pub fn is_other_speculation(value: SpeculatedType) -> bool {
    value == SPEC_OTHER
}

pub fn is_misc_speculation(value: SpeculatedType) -> bool {
    value != 0 && value & !SPEC_MISC == 0
}

pub fn is_other_or_empty_speculation(value: SpeculatedType) -> bool {
    value == 0 || value == SPEC_OTHER
}

pub fn is_empty_speculation(value: SpeculatedType) -> bool {
    value == SPEC_EMPTY
}

pub fn is_untyped_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN) != 0
}

pub fn is_untyped_speculation_for_bit_ops(value: SpeculatedType) -> bool {
    value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN | SPEC_OTHER) != 0
}

/// Funde duas predições. Hoje é só `left | right`, mas o protocolo de fusão pode mudar, por isso
/// ninguém deve fazer o `|` direto.
pub fn merge_speculations(left: SpeculatedType, right: SpeculatedType) -> SpeculatedType {
    left | right
}

/// `mergeSpeculation<T>`: o `T` do C++ é um `SpeculatedType` guardado em outro tipo inteiro
/// (`std::atomic`, campo de perfil); aqui o chamador passa o valor lido e guarda o resultado.
pub fn merge_speculation(left: &mut SpeculatedType, right: SpeculatedType) -> bool {
    let new_speculation = merge_speculations(*left, right);
    let result = new_speculation != *left;
    *left = new_speculation;
    result
}

pub fn speculation_checked(actual: SpeculatedType, desired: SpeculatedType) -> bool {
    actual | desired == desired
}

/// `speculationFromString` (SpeculatedType.cpp:967). O C++ compara por prefixo (`strncmp` com o
/// `strlen` do nome), na ordem da tabela: `SpecObjectOther` vem antes de `SpecObject`,
/// `SpecStringIdent`/`SpecStringVar` antes de `SpecString` etc. A ordem abaixo é a do C++.
pub fn speculation_from_string(speculation: &str) -> SpeculatedType {
    const TABLE: &[(&str, SpeculatedType)] = &[
        ("SpecNone", SPEC_NONE),
        ("SpecFinalObject", SPEC_FINAL_OBJECT),
        ("SpecArray", SPEC_ARRAY),
        ("SpecFunction", SPEC_FUNCTION),
        ("SpecInt8Array", SPEC_INT8_ARRAY),
        ("SpecInt16Array", SPEC_INT16_ARRAY),
        ("SpecInt32Array", SPEC_INT32_ARRAY),
        ("SpecUint8Array", SPEC_UINT8_ARRAY),
        ("SpecUint8ClampedArray", SPEC_UINT8_CLAMPED_ARRAY),
        ("SpecUint16Array", SPEC_UINT16_ARRAY),
        ("SpecUint32Array", SPEC_UINT32_ARRAY),
        ("SpecFloat16Array", SPEC_FLOAT16_ARRAY),
        ("SpecFloat32Array", SPEC_FLOAT32_ARRAY),
        ("SpecFloat64Array", SPEC_FLOAT64_ARRAY),
        ("SpecBigInt64Array", SPEC_BIG_INT64_ARRAY),
        ("SpecBigUint64Array", SPEC_BIG_UINT64_ARRAY),
        ("SpecTypedArrayView", SPEC_TYPED_ARRAY_VIEW),
        ("SpecDirectArguments", SPEC_DIRECT_ARGUMENTS),
        ("SpecScopedArguments", SPEC_SCOPED_ARGUMENTS),
        ("SpecStringObject", SPEC_STRING_OBJECT),
        ("SpecRegExpObject", SPEC_REG_EXP_OBJECT),
        ("SpecDateObject", SPEC_DATE_OBJECT),
        ("SpecPromiseObject", SPEC_PROMISE_OBJECT),
        ("SpecMapObject", SPEC_MAP_OBJECT),
        ("SpecSetObject", SPEC_SET_OBJECT),
        ("SpecWeakMapObject", SPEC_WEAK_MAP_OBJECT),
        ("SpecWeakSetObject", SPEC_WEAK_SET_OBJECT),
        ("SpecProxyObject", SPEC_PROXY_OBJECT),
        ("SpecGlobalProxy", SPEC_GLOBAL_PROXY),
        ("SpecDerivedArray", SPEC_DERIVED_ARRAY),
        ("SpecDataViewObject", SPEC_DATA_VIEW_OBJECT),
        ("SpecObjectOther", SPEC_OBJECT_OTHER),
        ("SpecObject", SPEC_OBJECT),
        ("SpecStringIdent", SPEC_STRING_IDENT),
        ("SpecStringVar", SPEC_STRING_VAR),
        ("SpecString", SPEC_STRING),
        ("SpecSymbol", SPEC_SYMBOL),
        ("SpecBigInt", SPEC_BIG_INT),
        ("SpecCellOther", SPEC_CELL_OTHER),
        ("SpecCell", SPEC_CELL),
        ("SpecBoolInt32", SPEC_BOOL_INT32),
        ("SpecNonBoolInt32", SPEC_NON_BOOL_INT32),
        ("SpecInt32Only", SPEC_INT32_ONLY),
        ("SpecInt32AsInt52", SPEC_INT32_AS_INT52),
        ("SpecNonInt32AsInt52", SPEC_NON_INT32_AS_INT52),
        ("SpecInt52Any", SPEC_INT52_ANY),
        ("SpecIntAnyFormat", SPEC_INT_ANY_FORMAT),
        ("SpecAnyIntAsDouble", SPEC_ANY_INT_AS_DOUBLE),
        ("SpecNonIntAsDouble", SPEC_NON_INT_AS_DOUBLE),
        ("SpecDoubleReal", SPEC_DOUBLE_REAL),
        ("SpecDoublePureNaN", SPEC_DOUBLE_PURE_NAN),
        ("SpecDoubleImpureNaN", SPEC_DOUBLE_IMPURE_NAN),
        ("SpecDoubleNaN", SPEC_DOUBLE_NAN),
        ("SpecBytecodeDouble", SPEC_BYTECODE_DOUBLE),
        ("SpecFullDouble", SPEC_FULL_DOUBLE),
        ("SpecBytecodeRealNumber", SPEC_BYTECODE_REAL_NUMBER),
        ("SpecFullRealNumber", SPEC_FULL_REAL_NUMBER),
        ("SpecBytecodeNumber", SPEC_BYTECODE_NUMBER),
        ("SpecFullNumber", SPEC_FULL_NUMBER),
        ("SpecBoolean", SPEC_BOOLEAN),
        ("SpecOther", SPEC_OTHER),
        ("SpecMisc", SPEC_MISC),
        ("SpecHeapTop", SPEC_HEAP_TOP),
        ("SpecPrimitive", SPEC_PRIMITIVE),
        ("SpecEmpty", SPEC_EMPTY),
        ("SpecBytecodeTop", SPEC_BYTECODE_TOP),
        ("SpecFullTop", SPEC_FULL_TOP),
        ("SpecCellCheck", SPEC_CELL_CHECK),
    ];
    for (name, value) in TABLE {
        if speculation.starts_with(name) {
            return *value;
        }
    }
    // RELEASE_ASSERT_NOT_REACHED()
    unreachable!("speculationFromString: nome de especulação desconhecido")
}
