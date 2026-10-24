//! Os tipos do formato binário que `wasm/WasmParser.h` e `WasmSectionParser.cpp` usam: o que o
//! `generateWasmOpsHeader.py` produz a partir de `wasm.json` (`TypeKind`, `PackedType`,
//! `DefinedTypeKind`) e as peças de `WasmFormat.h` e `WasmTypeDefinition.h` (`Type`,
//! `StorageType`, `FieldType`, `ExternalKind`).
//!
//! `TypeIndex` no C++ é um `uintptr_t` sobrecarregado (tag de tipo heap abstrato, ponteiro de RTT
//! canônico, ponteiro de `Projection`). Aqui é um enum com as mesmas quatro variantes. `Concrete`
//! carrega o identificador canônico do tipo (o que no C++ é o ponteiro do RTT canônico): dois tipos
//! definidos iguais pela canonicalização isorecursiva têm o mesmo identificador, então a igualdade
//! derivada de `Type` é a mesma do C++. Quem dá o identificador é `ModuleInformation`, que
//! canonicaliza dentro do módulo; o interning entre módulos (`TypeInformation`) é fatia própria.

/// `enum class TypeKind : int8_t`, os valores vêm de `wasm.json` (`"type"`).
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeKind {
    I32 = -1,
    I64 = -2,
    F32 = -3,
    F64 = -4,
    V128 = -5,
    Noexnref = -12,
    Nofuncref = -13,
    Noexternref = -14,
    Noneref = -15,
    Funcref = -16,
    Externref = -17,
    Anyref = -18,
    Eqref = -19,
    I31ref = -20,
    Structref = -21,
    Arrayref = -22,
    Exnref = -23,
    Ref = -28,
    RefNull = -29,
    Void = -64,
}

impl TypeKind {
    /// `makeString(TypeKind)`: o nome do variante no C++ (`ref_null` é `RefNull`).
    pub fn name(self) -> &'static str {
        match self {
            TypeKind::I32 => "I32",
            TypeKind::I64 => "I64",
            TypeKind::F32 => "F32",
            TypeKind::F64 => "F64",
            TypeKind::V128 => "V128",
            TypeKind::Noexnref => "Noexnref",
            TypeKind::Nofuncref => "Nofuncref",
            TypeKind::Noexternref => "Noexternref",
            TypeKind::Noneref => "Noneref",
            TypeKind::Funcref => "Funcref",
            TypeKind::Externref => "Externref",
            TypeKind::Anyref => "Anyref",
            TypeKind::Eqref => "Eqref",
            TypeKind::I31ref => "I31ref",
            TypeKind::Structref => "Structref",
            TypeKind::Arrayref => "Arrayref",
            TypeKind::Exnref => "Exnref",
            TypeKind::Ref => "Ref",
            TypeKind::RefNull => "RefNull",
            TypeKind::Void => "Void",
        }
    }

    /// `isValidTypeKind` seguido do `static_cast<TypeKind>`.
    pub fn from_i8(kind: i8) -> Option<TypeKind> {
        Some(match kind {
            -1 => TypeKind::I32,
            -2 => TypeKind::I64,
            -3 => TypeKind::F32,
            -4 => TypeKind::F64,
            -5 => TypeKind::V128,
            -12 => TypeKind::Noexnref,
            -13 => TypeKind::Nofuncref,
            -14 => TypeKind::Noexternref,
            -15 => TypeKind::Noneref,
            -16 => TypeKind::Funcref,
            -17 => TypeKind::Externref,
            -18 => TypeKind::Anyref,
            -19 => TypeKind::Eqref,
            -20 => TypeKind::I31ref,
            -21 => TypeKind::Structref,
            -22 => TypeKind::Arrayref,
            -23 => TypeKind::Exnref,
            -28 => TypeKind::Ref,
            -29 => TypeKind::RefNull,
            -64 => TypeKind::Void,
            _ => return None,
        })
    }

    /// `isAbstractHeapTypeKind` (a lista de `FOR_EACH_WASM_ABSTRACT_HEAP_TYPE_INDEX_TAG`), que é
    /// também o conjunto de `isValidHeapTypeKind`.
    pub fn is_abstract_heap_type_kind(self) -> bool {
        matches!(
            self,
            TypeKind::Noexnref
                | TypeKind::Nofuncref
                | TypeKind::Noexternref
                | TypeKind::Noneref
                | TypeKind::Funcref
                | TypeKind::Externref
                | TypeKind::Anyref
                | TypeKind::Eqref
                | TypeKind::I31ref
                | TypeKind::Structref
                | TypeKind::Arrayref
                | TypeKind::Exnref
        )
    }
}

/// `isValidTypeKind(int8_t)`.
pub fn is_valid_type_kind(kind: i8) -> bool {
    TypeKind::from_i8(kind).is_some()
}

/// `isValidHeapTypeKind(intptr_t)`.
pub fn is_valid_heap_type_kind(kind: i64) -> bool {
    i8::try_from(kind).ok().and_then(TypeKind::from_i8).is_some_and(TypeKind::is_abstract_heap_type_kind)
}

/// `enum class PackedType : int8_t`.
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PackedType {
    I8 = -8,
    I16 = -9,
}

impl PackedType {
    /// `makeString(PackedType)`.
    pub fn name(self) -> &'static str {
        match self {
            PackedType::I8 => "I8",
            PackedType::I16 => "I16",
        }
    }

    /// `isValidPackedType` seguido do `static_cast<PackedType>`.
    pub fn from_i8(kind: i8) -> Option<PackedType> {
        match kind {
            -8 => Some(PackedType::I8),
            -9 => Some(PackedType::I16),
            _ => None,
        }
    }

    /// `typeSizeInBytes(StorageType)` para um tipo empacotado.
    pub fn size_in_bytes(self) -> usize {
        match self {
            PackedType::I8 => 1,
            PackedType::I16 => 2,
        }
    }
}

/// `enum class DefinedTypeKind : int8_t`.
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinedTypeKind {
    Func = -32,
    Struct = -33,
    Array = -34,
    Sub = -48,
    Subfinal = -49,
    Rec = -50,
}

impl DefinedTypeKind {
    /// `static_cast<DefinedTypeKind>(typeKind)`; `None` cai no `default:` do `switch` do C++.
    pub fn from_i8(kind: i8) -> Option<DefinedTypeKind> {
        Some(match kind {
            -32 => DefinedTypeKind::Func,
            -33 => DefinedTypeKind::Struct,
            -34 => DefinedTypeKind::Array,
            -48 => DefinedTypeKind::Sub,
            -49 => DefinedTypeKind::Subfinal,
            -50 => DefinedTypeKind::Rec,
            _ => return None,
        })
    }
}

/// `enum class ExternalKind : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalKind {
    Function = 0,
    Table = 1,
    Memory = 2,
    Global = 3,
    Exception = 4,
}

impl ExternalKind {
    /// `isValidExternalKind` seguido do `static_cast<ExternalKind>`.
    pub fn from_u8(value: u8) -> Option<ExternalKind> {
        match value {
            0 => Some(ExternalKind::Function),
            1 => Some(ExternalKind::Table),
            2 => Some(ExternalKind::Memory),
            3 => Some(ExternalKind::Global),
            4 => Some(ExternalKind::Exception),
            _ => None,
        }
    }
}

/// `TypeIndex` (`uintptr_t` no C++), ver o comentário do módulo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeIndex {
    /// `invalidTypeIndex` (zero): sem tipo.
    Invalid,
    /// Tipo heap abstrato (`func`, `extern`, `any`...), o que o C++ guarda como tag.
    Abstract(TypeKind),
    /// Tipo definido na seção de tipos, pelo identificador canônico do seu RTT.
    Concrete(u32),
    /// `Projection` placeholder: referência a um membro do grupo recursivo em andamento (índice
    /// relativo ao começo do grupo), a ser trocada por um índice real em `expand()`.
    Projection(u32),
}

/// `typeIndexFromTypeKind`: só vale para tipo heap abstrato (`RELEASE_ASSERT` no C++).
pub fn type_index_from_type_kind(kind: TypeKind) -> TypeIndex {
    assert!(kind.is_abstract_heap_type_kind());
    TypeIndex::Abstract(kind)
}

/// `Wasm::Type`: par (kind, index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Type {
    pub kind: TypeKind,
    pub index: TypeIndex,
}

impl Type {
    pub const fn new(kind: TypeKind, index: TypeIndex) -> Type {
        Type { kind, index }
    }

    /// `Type::isRef`.
    pub fn is_ref(self) -> bool {
        self.kind == TypeKind::Ref
    }

    /// `Type::isRefNull`.
    pub fn is_ref_null(self) -> bool {
        self.kind == TypeKind::RefNull
    }

    /// `Type::isNullable`.
    pub fn is_nullable(self) -> bool {
        matches!(self.kind, TypeKind::RefNull | TypeKind::Externref | TypeKind::Funcref)
    }

    /// `Type::isI32`.
    pub fn is_i32(self) -> bool {
        self.kind == TypeKind::I32
    }

    /// `Type::isI64`.
    pub fn is_i64(self) -> bool {
        self.kind == TypeKind::I64
    }

    /// `Type::isV128`.
    pub fn is_v128(self) -> bool {
        self.kind == TypeKind::V128
    }
}

/// `Types::I32` e as demais constantes de `namespace Types`.
pub const TYPE_I32: Type = Type::new(TypeKind::I32, TypeIndex::Invalid);
pub const TYPE_I64: Type = Type::new(TypeKind::I64, TypeIndex::Invalid);
pub const TYPE_F32: Type = Type::new(TypeKind::F32, TypeIndex::Invalid);
pub const TYPE_F64: Type = Type::new(TypeKind::F64, TypeIndex::Invalid);
pub const TYPE_V128: Type = Type::new(TypeKind::V128, TypeIndex::Invalid);

/// `funcrefType()`: `(ref null func)`.
pub const fn funcref_type() -> Type {
    Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Funcref))
}

/// `nonNullFuncrefType()`: `(ref func)`.
pub const fn non_null_funcref_type() -> Type {
    Type::new(TypeKind::Ref, TypeIndex::Abstract(TypeKind::Funcref))
}

/// `isFuncref`, `isExternref`, `isAnyref`...: um tipo de referência cujo tipo heap é o abstrato
/// `heap_kind` (o `type.index() == typeIndexFromTypeKind(...)` do C++).
pub fn is_ref_to_abstract(ty: Type, heap_kind: TypeKind) -> bool {
    is_ref_type(ty) && ty.index == TypeIndex::Abstract(heap_kind)
}

/// `isRefWithTypeIndex`: uma referência cujo tipo heap é um tipo definido, não um abstrato.
pub fn is_ref_with_type_index(ty: Type) -> bool {
    is_ref_type(ty) && !matches!(ty.index, TypeIndex::Abstract(_))
}

/// `isDefaultableType(Type)`.
pub fn is_defaultable_type(ty: Type) -> bool {
    !ty.is_ref()
}

/// `isRefType(Type)`.
pub fn is_ref_type(ty: Type) -> bool {
    ty.is_ref() || ty.is_ref_null()
}

/// `isValueType`. `V128` depende de `Options::useWasmSIMD()`.
pub fn is_value_type(ty: Type, use_wasm_simd: bool) -> bool {
    match ty.kind {
        TypeKind::I32 | TypeKind::I64 | TypeKind::F32 | TypeKind::F64 => true,
        TypeKind::Exnref | TypeKind::Externref | TypeKind::Funcref => false,
        TypeKind::Ref | TypeKind::RefNull => ty.index != TypeIndex::Invalid,
        TypeKind::V128 => use_wasm_simd,
        _ => false,
    }
}

/// `StorageType`: um `Type` ou um `PackedType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageType {
    Type(Type),
    Packed(PackedType),
}

impl StorageType {
    /// `typeSizeInBytes(StorageType)`; `typeKindSizeInBytes` só aceita os tipos de valor
    /// armazenáveis (o `RELEASE_ASSERT_NOT_REACHED` do C++ cobre o resto).
    pub fn size_in_bytes(self) -> usize {
        match self {
            StorageType::Packed(packed) => packed.size_in_bytes(),
            StorageType::Type(ty) => match ty.kind {
                TypeKind::I32 | TypeKind::F32 => 4,
                TypeKind::I64 | TypeKind::F64 | TypeKind::Ref | TypeKind::RefNull => 8,
                TypeKind::V128 => 16,
                _ => unreachable!("tipo sem tamanho de armazenamento"),
            },
        }
    }
}

/// `enum Mutability : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutability {
    Immutable = 0,
    Mutable = 1,
}

/// `FieldType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldType {
    pub ty: StorageType,
    pub mutability: Mutability,
}

/// `v128_t`: os 16 bytes de um `v128`, na ordem do binário (little-endian).
pub type V128 = [u8; 16];

/// `Name` (`Vector<char8_t>`): o texto UTF-8 de um import, export ou seção customizada. O
/// `consumeUTF8String` já garantiu que é UTF-8 válido, então vira `String`.
pub type Name = String;

/// `struct Import`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub module: Name,
    pub field: Name,
    pub kind: ExternalKind,
    /// Índice no vetor do tipo correspondente.
    pub kind_index: u32,
}

/// `struct Export`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub field: Name,
    pub kind: ExternalKind,
    /// Índice no vetor do tipo correspondente.
    pub kind_index: u32,
}

/// `GlobalInformation::InitializationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalInitializationType {
    IsImport,
    FromGlobalImport,
    FromRefFunc,
    FromExpression,
    FromVector,
    FromExtendedExpression,
}

/// `GlobalInformation::BindingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalBindingMode {
    EmbeddedInInstance,
    Portable,
}

/// A `union { uint64_t initialBitsOrImportNumber; v128_t initialVector; }` de `GlobalInformation`.
/// Ler os bits de um vetor dá os 8 bytes de baixo, como o `union` do C++ em little-endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalInitialBits {
    BitsOrImportNumber(u64),
    Vector(V128),
}

impl GlobalInitialBits {
    /// `initialBits.initialBitsOrImportNumber`.
    pub fn bits_or_import_number(self) -> u64 {
        match self {
            GlobalInitialBits::BitsOrImportNumber(bits) => bits,
            GlobalInitialBits::Vector(vector) => {
                u64::from_le_bytes([vector[0], vector[1], vector[2], vector[3], vector[4], vector[5], vector[6], vector[7]])
            }
        }
    }
}

/// `struct GlobalInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalInformation {
    pub mutability: Mutability,
    pub ty: Type,
    pub initialization_type: GlobalInitializationType,
    pub binding_mode: GlobalBindingMode,
    pub initial_bits: GlobalInitialBits,
}

impl GlobalInformation {
    /// O `GlobalInformation global;` de `parseGlobalType`, já com o tipo e a mutabilidade lidos e
    /// os demais campos nos padrões do C++ (`IsImport`, `EmbeddedInInstance`, bits zerados).
    pub fn new(ty: Type, mutability: Mutability) -> GlobalInformation {
        GlobalInformation {
            mutability,
            ty,
            initialization_type: GlobalInitializationType::IsImport,
            binding_mode: GlobalBindingMode::EmbeddedInInstance,
            initial_bits: GlobalInitialBits::BitsOrImportNumber(0),
        }
    }
}

/// `struct FunctionData`: o corpo de uma função, como a seção `Code` o enquadra.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionData {
    /// Posição do primeiro byte do corpo no módulo.
    pub start: usize,
    /// Posição do byte seguinte ao último do corpo.
    pub end: usize,
    pub data: Vec<u8>,
    pub uses_simd: bool,
    pub uses_exceptions: bool,
    pub uses_atomics: bool,
    pub finished_validating: bool,
}

/// `I32InitExpr`, que também serve de `I64InitExpr` (`using I64InitExpr = I32InitExpr`): a
/// expressão de deslocamento de um segmento de elementos ou de dados.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I32InitExpr {
    /// `globalImport(globalImportNumber)`.
    Global(u64),
    /// `constValue(constValue)`.
    Const(u64),
    /// `extendedExpression(constantExpressionNumber)`.
    ExtendedExpression(u64),
}

/// `Segment::Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Active,
    Passive,
}

/// `class Segment`: um segmento de dados.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub offset_if_active: Option<I32InitExpr>,
    pub memory_index: u32,
    pub bytes: Vec<u8>,
}

impl Segment {
    pub fn is_active(&self) -> bool {
        self.kind == SegmentKind::Active
    }

    pub fn is_passive(&self) -> bool {
        self.kind == SegmentKind::Passive
    }

    /// `sizeInBytes`.
    pub fn size_in_bytes(&self) -> u32 {
        self.bytes.len() as u32
    }
}

/// `Element::Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Active,
    Passive,
    Declared,
}

/// `Element::InitializationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementInitializationType {
    FromGlobal,
    FromRefFunc,
    FromRefNull,
    FromExtendedExpression,
}

/// `struct Element`: um segmento de elementos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub kind: ElementKind,
    pub element_type: Type,
    pub table_index_if_active: Option<u32>,
    pub offset_if_active: Option<I32InitExpr>,
    pub init_types: Vec<ElementInitializationType>,
    pub initial_bits_or_indices: Vec<u64>,
}

impl Element {
    /// Os dois construtores de `Element` (o segundo, sem tabela nem deslocamento, é o de
    /// segmentos passivos e declarados).
    pub fn new(
        kind: ElementKind,
        element_type: Type,
        table_index_if_active: Option<u32>,
        offset_if_active: Option<I32InitExpr>,
    ) -> Element {
        Element {
            kind,
            element_type,
            table_index_if_active,
            offset_if_active,
            init_types: Vec::new(),
            initial_bits_or_indices: Vec::new(),
        }
    }

    /// `length`.
    pub fn length(&self) -> u32 {
        self.init_types.len() as u32
    }

    pub fn is_active(&self) -> bool {
        self.kind == ElementKind::Active
    }

    pub fn is_passive(&self) -> bool {
        self.kind == ElementKind::Passive
    }
}

/// `enum class TableElementType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableElementType {
    Externref,
    Funcref,
}

/// `TableInformation::InitializationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableInitializationType {
    Default,
    FromGlobalImport,
    FromRefFunc,
    FromRefNull,
    FromExtendedExpression,
}

/// `class TableInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInformation {
    /// O tamanho que o módulo declarou, que pode passar de qualquer tabela que esta implementação
    /// crie; é conferido na criação da tabela, não no parse da declaração.
    pub initial: u64,
    pub maximum: Option<u64>,
    pub is_import: bool,
    pub element_type: TableElementType,
    pub wasm_type: Type,
    pub init_type: TableInitializationType,
    pub initial_bits_or_import_number: u64,
    pub address_type: crate::wasm::wasm_address_type::AddressType,
}
