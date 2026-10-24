//! Porte de `runtime/JSType.h`: o enum `JSType` (valores na ordem de `FOR_EACH_JS_TYPE`), as
//! constantes de faixa e os predicados puros.

/// `enum JSType : uint8_t`. Os valores são os ordinais de `FOR_EACH_JS_TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum JSType {
    /// O `CellType` vem antes de qualquer `JSType` que seja um `JSCell`.
    CellType = 0,
    StructureType,

    // Células que exigem comparação de identidade sem ser por ponteiro (valor de string).
    StringType,
    HeapBigIntType,

    SymbolType,

    GetterSetterType,
    CustomGetterSetterType,
    APIValueWrapperType,

    NativeExecutableType,

    ProgramExecutableType,
    ModuleProgramExecutableType,
    EvalExecutableType,
    FunctionExecutableType,

    UnlinkedFunctionExecutableType,

    UnlinkedProgramCodeBlockType,
    UnlinkedModuleProgramCodeBlockType,
    UnlinkedEvalCodeBlockType,
    UnlinkedFunctionCodeBlockType,

    CodeBlockType,

    JSCellButterflyType,
    JSSourceCodeType,
    JSSlimPromiseReactionType,
    JSFullPromiseReactionType,
    JSPromiseCombinatorsContextType,
    JSPromiseCombinatorsGlobalContextType,
    JSWebAssemblyStreamingContextType,
    JSMicrotaskDispatcherType,
    ModuleRegistryEntryType,
    ModuleLoadingContextType,
    ModuleLoaderPayloadType,
    ModuleGraphLoadingStateType,
    JSModuleLoaderType,
    SentinelType,

    /// O `ObjectType` vem antes de qualquer `JSType` que seja subclasse de `JSObject`.
    ObjectType,
    FinalObjectType,
    JSCalleeType,
    JSFunctionType,
    InternalFunctionType,
    NullSetterFunctionType,
    BooleanObjectType,
    NumberObjectType,
    ErrorInstanceType,
    GlobalProxyType,
    DirectArgumentsType,
    ScopedArgumentsType,
    ClonedArgumentsType,

    // Tipos de `JSArray`.
    ArrayType,
    DerivedArrayType,

    ArrayBufferType,

    // Tipos de `JSArrayBufferView`, na ordem de `FOR_EACH_TYPED_ARRAY_TYPE_EXCLUDING_DATA_VIEW`.
    Int8ArrayType,
    Uint8ArrayType,
    Uint8ClampedArrayType,
    Int16ArrayType,
    Uint16ArrayType,
    Int32ArrayType,
    Uint32ArrayType,
    Float16ArrayType,
    Float32ArrayType,
    Float64ArrayType,
    BigInt64ArrayType,
    BigUint64ArrayType,
    DataViewType,

    // Tipos de `JSScope`: registros de ambiente primeiro, depois `WithScopeType`.
    GlobalObjectType,
    GlobalLexicalEnvironmentType,
    LexicalEnvironmentType,
    ModuleEnvironmentType,
    StrictEvalActivationType,
    WithScopeType,

    AsyncDisposableStackType,
    DisposableStackType,
    ModuleNamespaceObjectType,
    ShadowRealmType,
    RegExpObjectType,
    JSDateType,
    ProxyObjectType,
    JSGeneratorType,
    JSAsyncFunctionGeneratorType,
    JSAsyncGeneratorType,
    JSArrayIteratorType,
    JSIteratorType,
    JSIteratorHelperType,
    JSMapIteratorType,
    JSSetIteratorType,
    JSStringIteratorType,
    JSWrapForValidIteratorType,
    JSRegExpStringIteratorType,
    JSAsyncFromSyncIteratorType,
    JSPromiseType,
    JSMapType,
    JSSetType,
    JSWeakMapType,
    JSWeakSetType,
    WebAssemblyModuleType,
    WebAssemblyInstanceType,
    WebAssemblyGCObjectType,
    // Tipos de `StringObject`.
    StringObjectType,
    /// Não entra em `SpecStringObject`, para `StringObjectUse` não aceitar `String.prototype`.
    DerivedStringObjectType,
    InternalFieldTupleType,

    MaxJSType = 0b1111_1111,
}

impl JSType {
    /// `LastJSCObjectType`: o último tipo "JSC"; depois dele ficam os tipos do embedder.
    pub const LAST_JSC_OBJECT_TYPE: JSType = JSType::InternalFieldTupleType;
}

/// `struct JSTypeRange`: faixa inclusiva de `JSType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JSTypeRange {
    pub first: JSType,
    pub last: JSType,
}

impl JSTypeRange {
    /// `fromRawValue`. O byte baixo é `first`, o alto é `last`; só vale para bytes que sejam um
    /// `JSType` conhecido (o C++ faz `static_cast` de um `uint8_t` qualquer, que aqui não existe
    /// em Rust seguro), por isso devolve `None` fora do enum.
    pub fn from_raw_value(value: u16) -> Option<JSTypeRange> {
        Some(JSTypeRange {
            first: js_type_from_u8((value & 0xff) as u8)?,
            last: js_type_from_u8((value >> 8) as u8)?,
        })
    }

    pub fn contains(&self, type_: JSType) -> bool {
        self.first <= type_ && type_ <= self.last
    }

    pub const fn raw_value(&self) -> u16 {
        (self.first as u16) | ((self.last as u16) << 8)
    }
}

/// O inverso de `static_cast<uint8_t>(JSType)`: `None` para o byte que não é de nenhum `JSType`
/// (os tipos do embedder não existem aqui).
pub const fn js_type_from_u8(value: u8) -> Option<JSType> {
    if value <= JSType::LAST_JSC_OBJECT_TYPE as u8 {
        // Os ordinais de 0 a `LastJSCObjectType` são todos variantes, sem lacunas.
        Some(JS_TYPES_BY_ORDINAL[value as usize])
    } else if value == JSType::MaxJSType as u8 {
        Some(JSType::MaxJSType)
    } else {
        None
    }
}

/// `EmbedderArrayLikeType`.
pub const EMBEDDER_ARRAY_LIKE_TYPE: u8 = 0b1110_1101;

pub const LAST_VALUE_COMPARE_CELL_TYPE: u32 = JSType::HeapBigIntType as u32;

pub const FIRST_TYPED_ARRAY_TYPE: u32 = JSType::Int8ArrayType as u32;
pub const LAST_TYPED_ARRAY_TYPE: u32 = JSType::DataViewType as u32;
pub const LAST_TYPED_ARRAY_TYPE_EXCLUDING_DATA_VIEW: u32 = LAST_TYPED_ARRAY_TYPE - 1;

/// `LastObjectType` é `MaxJSType` (não `LastJSCObjectType`), porque o embedder acrescenta tipos
/// de objeto depois dos listados em `JSType`.
pub const FIRST_OBJECT_TYPE: u32 = JSType::ObjectType as u32;
pub const LAST_OBJECT_TYPE: u32 = JSType::MaxJSType as u32;

pub const FIRST_SCOPE_TYPE: u32 = JSType::GlobalObjectType as u32;
pub const LAST_SCOPE_TYPE: u32 = JSType::WithScopeType as u32;

pub const NUMBER_OF_TYPED_ARRAY_TYPES: u32 = LAST_TYPED_ARRAY_TYPE - FIRST_TYPED_ARRAY_TYPE + 1;
pub const NUMBER_OF_TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW: u32 = NUMBER_OF_TYPED_ARRAY_TYPES - 1;
pub const NUMBER_OF_TYPED_ARRAY_TYPES_EXCLUDING_BIG_INT_ARRAYS_AND_DATA_VIEW: u32 =
    NUMBER_OF_TYPED_ARRAY_TYPES - 3;

// `static_assert(LastJSCObjectType < 0b11100000, "Embedder can use 0b11100000 or upper.")`.
const _: () = assert!((JSType::LAST_JSC_OBJECT_TYPE as u32) < 0b1110_0000);

/// `isTypedArrayType`: a subtração em `uint32_t` dá a volta, como no C++.
pub const fn is_typed_array_type(type_: JSType) -> bool {
    (type_ as u32).wrapping_sub(FIRST_TYPED_ARRAY_TYPE) < NUMBER_OF_TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW
}

pub const fn is_typed_array_type_including_data_view(type_: JSType) -> bool {
    (type_ as u32).wrapping_sub(FIRST_TYPED_ARRAY_TYPE) < NUMBER_OF_TYPED_ARRAY_TYPES
}

pub const fn is_object_type(type_: JSType) -> bool {
    type_ as u8 >= JSType::ObjectType as u8
}

/// Todas as variantes de 0 até `LastJSCObjectType`, indexadas pelo ordinal.
const JS_TYPES_BY_ORDINAL: [JSType; JSType::LAST_JSC_OBJECT_TYPE as usize + 1] = [
    JSType::CellType,
    JSType::StructureType,
    JSType::StringType,
    JSType::HeapBigIntType,
    JSType::SymbolType,
    JSType::GetterSetterType,
    JSType::CustomGetterSetterType,
    JSType::APIValueWrapperType,
    JSType::NativeExecutableType,
    JSType::ProgramExecutableType,
    JSType::ModuleProgramExecutableType,
    JSType::EvalExecutableType,
    JSType::FunctionExecutableType,
    JSType::UnlinkedFunctionExecutableType,
    JSType::UnlinkedProgramCodeBlockType,
    JSType::UnlinkedModuleProgramCodeBlockType,
    JSType::UnlinkedEvalCodeBlockType,
    JSType::UnlinkedFunctionCodeBlockType,
    JSType::CodeBlockType,
    JSType::JSCellButterflyType,
    JSType::JSSourceCodeType,
    JSType::JSSlimPromiseReactionType,
    JSType::JSFullPromiseReactionType,
    JSType::JSPromiseCombinatorsContextType,
    JSType::JSPromiseCombinatorsGlobalContextType,
    JSType::JSWebAssemblyStreamingContextType,
    JSType::JSMicrotaskDispatcherType,
    JSType::ModuleRegistryEntryType,
    JSType::ModuleLoadingContextType,
    JSType::ModuleLoaderPayloadType,
    JSType::ModuleGraphLoadingStateType,
    JSType::JSModuleLoaderType,
    JSType::SentinelType,
    JSType::ObjectType,
    JSType::FinalObjectType,
    JSType::JSCalleeType,
    JSType::JSFunctionType,
    JSType::InternalFunctionType,
    JSType::NullSetterFunctionType,
    JSType::BooleanObjectType,
    JSType::NumberObjectType,
    JSType::ErrorInstanceType,
    JSType::GlobalProxyType,
    JSType::DirectArgumentsType,
    JSType::ScopedArgumentsType,
    JSType::ClonedArgumentsType,
    JSType::ArrayType,
    JSType::DerivedArrayType,
    JSType::ArrayBufferType,
    JSType::Int8ArrayType,
    JSType::Uint8ArrayType,
    JSType::Uint8ClampedArrayType,
    JSType::Int16ArrayType,
    JSType::Uint16ArrayType,
    JSType::Int32ArrayType,
    JSType::Uint32ArrayType,
    JSType::Float16ArrayType,
    JSType::Float32ArrayType,
    JSType::Float64ArrayType,
    JSType::BigInt64ArrayType,
    JSType::BigUint64ArrayType,
    JSType::DataViewType,
    JSType::GlobalObjectType,
    JSType::GlobalLexicalEnvironmentType,
    JSType::LexicalEnvironmentType,
    JSType::ModuleEnvironmentType,
    JSType::StrictEvalActivationType,
    JSType::WithScopeType,
    JSType::AsyncDisposableStackType,
    JSType::DisposableStackType,
    JSType::ModuleNamespaceObjectType,
    JSType::ShadowRealmType,
    JSType::RegExpObjectType,
    JSType::JSDateType,
    JSType::ProxyObjectType,
    JSType::JSGeneratorType,
    JSType::JSAsyncFunctionGeneratorType,
    JSType::JSAsyncGeneratorType,
    JSType::JSArrayIteratorType,
    JSType::JSIteratorType,
    JSType::JSIteratorHelperType,
    JSType::JSMapIteratorType,
    JSType::JSSetIteratorType,
    JSType::JSStringIteratorType,
    JSType::JSWrapForValidIteratorType,
    JSType::JSRegExpStringIteratorType,
    JSType::JSAsyncFromSyncIteratorType,
    JSType::JSPromiseType,
    JSType::JSMapType,
    JSType::JSSetType,
    JSType::JSWeakMapType,
    JSType::JSWeakSetType,
    JSType::WebAssemblyModuleType,
    JSType::WebAssemblyInstanceType,
    JSType::WebAssemblyGCObjectType,
    JSType::StringObjectType,
    JSType::DerivedStringObjectType,
    JSType::InternalFieldTupleType,
];

// A tabela tem de bater com os ordinais do enum, posição a posição.
const _: () = {
    let mut i = 0;
    while i < JS_TYPES_BY_ORDINAL.len() {
        assert!(JS_TYPES_BY_ORDINAL[i] as usize == i);
        i += 1;
    }
};
