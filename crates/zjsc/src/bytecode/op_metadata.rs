//! Os `Op*::Metadata` de `derived/JavaScriptCore/BytecodeStructs.h` (gerados do `metadata:` de
//! `bytecode/BytecodeList.rb`), um struct por opcode com metadata, mais o `sizeof`/`alignof` que o
//! `UnlinkedMetadataTable::finalize` usa (`FOR_EACH_BYTECODE_METADATA_SIZE` e `_ALIGNMENT` de
//! `Bytecodes.h`).
//!
//! São 50 opcodes com metadata (`NUMBER_OF_BYTECODE_WITH_METADATA`), na ordem do `OpcodeID`.
//!
//! Divergências:
//!
//! - O layout em bytes do C++ (x86_64, ABI Itanium) não é o do Rust. Cada struct carrega, além dos
//!   campos de valor, a lista de layouts C++ dos seus membros (`CppLayout`), e `LAYOUT` calcula o
//!   `sizeof`/`alignof` do struct pela regra do C++ (membros em ordem, cada um alinhado, tamanho
//!   final arredondado ao maior alinhamento; struct sem membros tem tamanho 1). `union` do C++ entra
//!   na lista como um membro só, com o maior tamanho e o maior alinhamento dos seus braços.
//! - Os tipos que dependem do heap (`WriteBarrier<T>`, `StructureID`, `JSArray*`, `CodeBlock*`,
//!   `InlineWatchpointSet*`, `TypeLocation*`, `PolymorphicCallStubRoutine`) entram como `u32` (índice
//!   de célula, 0 é nulo) ou `u64` (endereço de código ou ponteiro de sistema) até o `Heap` existir.
//! - `DataOnlyCallLinkInfo` guarda só os campos de valor que o interpretador lê e escreve; o
//!   cabeçalho de lista (`BasicRawSentinelNode`), o `CodePtr` e o `RefPtr<PolymorphicCallStubRoutine>`
//!   existem só no `sizeof` (80 bytes: 16 do nó, 1 do `m_callSiteType`, bitfields e
//!   `m_maxArgumentCountIncludingThisForVarargs` em 17..20, e sete ponteiros de 8 bytes a partir
//!   de 24).
//! - Os value profiles não fazem parte do `Metadata` (vivem antes da tabela, no `MetadataTable`);
//!   aqui só entra o `sizeof(ValueProfile)` (`VALUE_PROFILE_SIZE`).

use crate::bytecode::opcode::OpcodeID;
use crate::runtime::get_put_info::{GetPutInfo, ResolveType};
use crate::runtime::indexing_type::{IndexingType, ARRAY_WITH_UNDECIDED};
use crate::runtime::options::Options;

/// `sizeof` e `alignof` de um tipo C++ em x86_64 Linux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CppLayout {
    pub size: u32,
    pub align: u32,
}

const fn round_up(value: u32, align: u32) -> u32 {
    (value + align - 1) / align * align
}

/// Layout de um `struct` do C++: membros em ordem, cada um no seu alinhamento; sem membros,
/// `sizeof == 1`.
pub const fn struct_layout(fields: &[CppLayout]) -> CppLayout {
    if fields.is_empty() {
        return CppLayout { size: 1, align: 1 };
    }
    let mut offset = 0;
    let mut align = 1;
    let mut index = 0;
    while index < fields.len() {
        let field = fields[index];
        offset = round_up(offset, field.align) + field.size;
        if field.align > align {
            align = field.align;
        }
        index += 1;
    }
    CppLayout { size: round_up(offset, align), align }
}

const fn scalar(size: u32) -> CppLayout {
    CppLayout { size, align: size }
}

pub const BOOL: CppLayout = scalar(1);
pub const UNSIGNED: CppLayout = scalar(4);
/// `StructureID`, `WriteBarrierStructureID`, `ToThisStatus` (enum sem tipo fixo), `ResolveType`
/// (`enum : unsigned`) e `GetPutInfo` (um `unsigned`).
pub const STRUCTURE_ID: CppLayout = scalar(4);
pub const TO_THIS_STATUS: CppLayout = scalar(4);
pub const RESOLVE_TYPE: CppLayout = scalar(4);
pub const GET_PUT_INFO: CppLayout = scalar(4);
/// Ponteiros, `WriteBarrier<T>`, `WriteBarrierBase<T>` e `uintptr_t`.
pub const POINTER: CppLayout = scalar(8);
pub const UINTPTR: CppLayout = scalar(8);
/// `IterationModeMetadata`: um `uint16_t`.
pub const ITERATION_MODE_METADATA: CppLayout = scalar(2);
/// `EnumeratorMetadata`: o tipo subjacente de `JSPropertyNameEnumerator::Flag` (`uint8_t`).
pub const ENUMERATOR_METADATA: CppLayout = scalar(1);
/// `ArrayProfile`: quatro campos de 32 bits (`static_assert(sizeof(ArrayProfile) == 16)`).
pub const ARRAY_PROFILE: CppLayout = CppLayout { size: 16, align: 4 };
/// `GetByIdModeMetadata`: união com `uint64_t cachedSlot` (`static_assert(... == 16)`).
pub const GET_BY_ID_MODE_METADATA: CppLayout = CppLayout { size: 16, align: 8 };
/// `ArrayAllocationProfile`: um `CompactPointerTuple<JSArray*, uint16_t>` de 64 bits.
pub const ARRAY_ALLOCATION_PROFILE: CppLayout = scalar(8);
/// `ObjectAllocationProfile`: `Allocator` (um ponteiro) mais `WriteBarrier<Structure>`.
pub const OBJECT_ALLOCATION_PROFILE: CppLayout = CppLayout { size: 16, align: 8 };
/// `DataOnlyCallLinkInfo`, ver o cabeçalho do módulo.
pub const DATA_ONLY_CALL_LINK_INFO: CppLayout = CppLayout { size: 80, align: 8 };
/// `ValueProfile` (`ValueProfileBase<1, 0>`): um bucket de `EncodedJSValue` e um `SpeculatedType`
/// (`uint64_t`).
pub const VALUE_PROFILE_SIZE: u32 = 16;

const _: () = {
    assert!(struct_layout(&[POINTER, UINTPTR]).size == 16);
    assert!(struct_layout(&[]).size == 1);
};

/// Referência a célula do heap (`WriteBarrier<T>`): índice de célula, 0 é nulo.
pub type HeapRef = u32;
/// `StructureID`.
pub type StructureId = u32;

/// `enum class GetByIdMode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GetByIdMode {
    ProtoLoad = 0,
    Default = 1,
    Unset = 2,
    ArrayLength = 3,
}

/// `union GetByIdModeMetadata`: os braços compartilham os mesmos 16 bytes no C++; o porte guarda os
/// campos separados (o `mode` e o `hitCountForLLIntCaching` ficam fora do `cachedSlot` porque ele
/// só vale no modo `ProtoLoad`, onde os dois são zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GetByIdModeMetadata {
    pub structure_id: StructureId,
    pub cached_offset: i32,
    pub cached_slot: u64,
    pub mode: GetByIdMode,
    pub hit_count_for_llint_caching: u8,
}

impl Default for GetByIdModeMetadata {
    /// `GetByIdModeMetadata()`.
    fn default() -> GetByIdModeMetadata {
        GetByIdModeMetadata {
            structure_id: 0,
            cached_offset: 0,
            cached_slot: 0,
            mode: GetByIdMode::Default,
            hit_count_for_llint_caching: Options::prototype_hit_count_for_ll_int_caching() as u8,
        }
    }
}

/// `ArrayProfile`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArrayProfile {
    pub last_seen_structure_id: StructureId,
    pub speculation_failure_structure_id: StructureId,
    pub array_profile_flags: u32,
    pub observed_array_modes: u32,
}

/// `enum class IterationMode : uint16_t` (`IterationModeMetadata.h`).
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationMode {
    Generic = 1 << 0,
    FastArray = 1 << 1,
    FastMap = 1 << 2,
    FastSet = 1 << 3,
    FastString = 1 << 4,
    FastArrayValues = 1 << 5,
    FastArrayKeys = 1 << 6,
    FastArrayEntries = 1 << 7,
    FastMapKeys = 1 << 8,
    FastMapValues = 1 << 9,
    FastMapEntries = 1 << 10,
    FastSetValues = 1 << 11,
    FastSetEntries = 1 << 12,
    FastAsyncGenerator = 1 << 13,
    AsyncFromSync = 1 << 14,
}

/// `IterationModeMetadata`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IterationModeMetadata {
    pub seen_modes: u16,
}

/// `ArrayAllocationProfile`: o `JSArray*` e o `IndexingTypeAndVectorLength` do `CompactPointerTuple`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrayAllocationProfile {
    pub last_array: HeapRef,
    pub indexing_type_and_vector_length: u16,
}

impl ArrayAllocationProfile {
    const INDEXING_TYPE_SHIFT: u16 = 8;

    /// `ArrayAllocationProfile(IndexingType)`: `initializeIndexingMode`.
    pub fn with_indexing_type(indexing_type: IndexingType) -> ArrayAllocationProfile {
        ArrayAllocationProfile {
            last_array: 0,
            indexing_type_and_vector_length: (indexing_type as u16) << Self::INDEXING_TYPE_SHIFT,
        }
    }
}

impl Default for ArrayAllocationProfile {
    /// `ArrayAllocationProfile()`: `initializeIndexingMode(ArrayWithUndecided)`.
    fn default() -> ArrayAllocationProfile {
        ArrayAllocationProfile::with_indexing_type(ARRAY_WITH_UNDECIDED)
    }
}

/// `ObjectAllocationProfile`: `m_allocator` (o `LocalAllocator*` do `Allocator`) e `m_structure`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectAllocationProfile {
    pub allocator: u64,
    pub structure: HeapRef,
}

/// `DataOnlyCallLinkInfo`, só os campos de valor (ver o cabeçalho do módulo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DataOnlyCallLinkInfo {
    pub has_seen_should_repatch: bool,
    pub has_seen_closure: bool,
    pub cleared_by_gc: bool,
    pub cleared_by_virtual: bool,
    pub call_type: u8,
    pub mode: u8,
    pub max_argument_count_including_this_for_varargs: u8,
    pub code_block: HeapRef,
    pub callee: HeapRef,
    pub last_seen_callee: HeapRef,
    pub owner: HeapRef,
}

/// `ToThisStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ToThisStatus {
    #[default]
    ToThisOK = 0,
    ToThisConflicted = 1,
    ToThisClearedByGC = 2,
}

/// Declara os structs `Metadata` e as tabelas de layout a partir de uma lista só: cada entrada é
/// `opcode, Nome { campos do porte } layout [membros do C++ em ordem]`.
macro_rules! define_op_metadata {
    ($( $opcode:ident, $name:ident { $( $field:ident : $ty:ty ),* $(,)? } layout [ $( $layout:expr ),* $(,)? ]; )*) => {
        $(
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct $name {
                $( pub $field: $ty, )*
            }

            impl $name {
                /// `sizeof` e `alignof` do `Op::Metadata` correspondente no C++.
                pub const LAYOUT: CppLayout = struct_layout(&[ $( $layout ),* ]);
            }
        )*

        /// Layout de cada `Op::Metadata`, indexado pelo `OpcodeID`.
        pub const METADATA_LAYOUTS: [CppLayout; NUMBER_OF_OPCODES_WITH_METADATA] = [ $( $name::LAYOUT ),* ];

        const _: () = {
            let mut index = 0;
            $(
                assert!(OpcodeID::$opcode as usize == index);
                index += 1;
            )*
            assert!(index == NUMBER_OF_OPCODES_WITH_METADATA);
        };
    };
}

const NUMBER_OF_OPCODES_WITH_METADATA: usize = 50;

/// `metadataSize(OpcodeID)`: o `sizeof(Op::Metadata)`.
pub fn metadata_size(opcode_id: OpcodeID) -> u32 {
    METADATA_LAYOUTS[opcode_id as usize].size
}

/// `metadataAlignment(OpcodeID)`: o `alignof(Op::Metadata)`.
pub fn metadata_alignment(opcode_id: OpcodeID) -> u32 {
    METADATA_LAYOUTS[opcode_id as usize].align
}

define_op_metadata! {
    op_tail_call_varargs, OpTailCallVarargsMetadata { call_link_info: DataOnlyCallLinkInfo } layout [DATA_ONLY_CALL_LINK_INFO];
    op_call_varargs, OpCallVarargsMetadata { call_link_info: DataOnlyCallLinkInfo } layout [DATA_ONLY_CALL_LINK_INFO];
    op_iterator_next, OpIteratorNextMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        done_mode_metadata: GetByIdModeMetadata,
        value_mode_metadata: GetByIdModeMetadata,
        iterable_profile: ArrayProfile,
        iteration_metadata: IterationModeMetadata,
    } layout [DATA_ONLY_CALL_LINK_INFO, GET_BY_ID_MODE_METADATA, GET_BY_ID_MODE_METADATA, ARRAY_PROFILE, ITERATION_MODE_METADATA];
    op_construct_varargs, OpConstructVarargsMetadata { call_link_info: DataOnlyCallLinkInfo } layout [DATA_ONLY_CALL_LINK_INFO];
    op_super_construct_varargs, OpSuperConstructVarargsMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        cached_callee: HeapRef,
    } layout [DATA_ONLY_CALL_LINK_INFO, POINTER];
    op_iterator_open, OpIteratorOpenMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        mode_metadata: GetByIdModeMetadata,
        array_profile: ArrayProfile,
        iteration_metadata: IterationModeMetadata,
    } layout [DATA_ONLY_CALL_LINK_INFO, GET_BY_ID_MODE_METADATA, ARRAY_PROFILE, ITERATION_MODE_METADATA];
    op_async_iterator_open, OpAsyncIteratorOpenMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        mode_metadata: GetByIdModeMetadata,
        iteration_metadata: IterationModeMetadata,
    } layout [DATA_ONLY_CALL_LINK_INFO, GET_BY_ID_MODE_METADATA, ITERATION_MODE_METADATA];
    op_instanceof, OpInstanceofMetadata {
        has_instance_mode_metadata: GetByIdModeMetadata,
        prototype_mode_metadata: GetByIdModeMetadata,
    } layout [GET_BY_ID_MODE_METADATA, GET_BY_ID_MODE_METADATA];
    op_set_private_brand, OpSetPrivateBrandMetadata {
        old_structure_id: StructureId,
        new_structure_id: StructureId,
        brand: HeapRef,
    } layout [STRUCTURE_ID, STRUCTURE_ID, POINTER];
    op_check_private_brand, OpCheckPrivateBrandMetadata {
        structure_id: StructureId,
        brand: HeapRef,
    } layout [STRUCTURE_ID, POINTER];
    op_put_by_id, OpPutByIdMetadata {
        old_structure_id: StructureId,
        offset: u32,
        new_structure_id: StructureId,
        structure_chain: HeapRef,
    } layout [STRUCTURE_ID, UNSIGNED, STRUCTURE_ID, POINTER];
    op_construct, OpConstructMetadata { call_link_info: DataOnlyCallLinkInfo } layout [DATA_ONLY_CALL_LINK_INFO];
    op_super_construct, OpSuperConstructMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        cached_callee: HeapRef,
    } layout [DATA_ONLY_CALL_LINK_INFO, POINTER];
    op_tail_call, OpTailCallMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        array_profile: ArrayProfile,
    } layout [DATA_ONLY_CALL_LINK_INFO, ARRAY_PROFILE];
    op_call_direct_eval, OpCallDirectEvalMetadata { call_link_info: DataOnlyCallLinkInfo } layout [DATA_ONLY_CALL_LINK_INFO];
    op_create_generator, OpCreateGeneratorMetadata { cached_callee: HeapRef } layout [POINTER];
    op_create_async_generator, OpCreateAsyncGeneratorMetadata { cached_callee: HeapRef } layout [POINTER];
    op_create_promise, OpCreatePromiseMetadata { cached_callee: HeapRef } layout [POINTER];
    op_catch, OpCatchMetadata { buffer: u64 } layout [POINTER];
    op_new_array_with_size, OpNewArrayWithSizeMetadata { array_allocation_profile: ArrayAllocationProfile } layout [ARRAY_ALLOCATION_PROFILE];
    op_new_array_buffer, OpNewArrayBufferMetadata { array_allocation_profile: ArrayAllocationProfile } layout [ARRAY_ALLOCATION_PROFILE];
    op_get_by_id, OpGetByIdMetadata { mode_metadata: GetByIdModeMetadata } layout [GET_BY_ID_MODE_METADATA];
    op_get_length, OpGetLengthMetadata {
        mode_metadata: GetByIdModeMetadata,
        array_profile: ArrayProfile,
    } layout [GET_BY_ID_MODE_METADATA, ARRAY_PROFILE];
    op_profile_type, OpProfileTypeMetadata { type_location: u64 } layout [POINTER];
    op_profile_control_flow, OpProfileControlFlowMetadata { } layout [];
    op_new_array_with_species, OpNewArrayWithSpeciesMetadata {
        array_allocation_profile: ArrayAllocationProfile,
        array_profile: ArrayProfile,
    } layout [ARRAY_ALLOCATION_PROFILE, ARRAY_PROFILE];
    op_call, OpCallMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        array_profile: ArrayProfile,
    } layout [DATA_ONLY_CALL_LINK_INFO, ARRAY_PROFILE];
    op_call_ignore_result, OpCallIgnoreResultMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        array_profile: ArrayProfile,
    } layout [DATA_ONLY_CALL_LINK_INFO, ARRAY_PROFILE];
    op_async_iterator_next, OpAsyncIteratorNextMetadata {
        call_link_info: DataOnlyCallLinkInfo,
        iteration_metadata: IterationModeMetadata,
    } layout [DATA_ONLY_CALL_LINK_INFO, ITERATION_MODE_METADATA];
    // `m_localScopeDepth`/`m_globalLexicalBindingEpoch` e os cinco `WriteBarrierBase` são uniões.
    op_resolve_scope, OpResolveScopeMetadata {
        resolve_type: ResolveTypeField,
        local_scope_depth_or_global_lexical_binding_epoch: u32,
        lexical_environment_or_symbol_table_or_constant_scope_or_global_object: HeapRef,
    } layout [RESOLVE_TYPE, UNSIGNED, POINTER];
    // `InlineWatchpointSet*` e `WriteBarrierStructureID` formam uma união de 8 bytes.
    op_get_from_scope, OpGetFromScopeMetadata {
        get_put_info: GetPutInfo,
        watchpoint_set_or_structure_id: u64,
        operand: u64,
    } layout [GET_PUT_INFO, POINTER, UINTPTR];
    op_put_to_scope, OpPutToScopeMetadata {
        get_put_info: GetPutInfo,
        watchpoint_set_or_structure_id: u64,
        operand: u64,
    } layout [GET_PUT_INFO, POINTER, UINTPTR];
    op_create_this, OpCreateThisMetadata { cached_callee: HeapRef } layout [POINTER];
    op_new_object, OpNewObjectMetadata { object_allocation_profile: ObjectAllocationProfile } layout [OBJECT_ALLOCATION_PROFILE];
    op_new_array, OpNewArrayMetadata { array_allocation_profile: ArrayAllocationProfile } layout [ARRAY_ALLOCATION_PROFILE];
    op_put_private_name, OpPutPrivateNameMetadata {
        property: HeapRef,
        old_structure_id: StructureId,
        offset: u32,
        new_structure_id: StructureId,
    } layout [POINTER, STRUCTURE_ID, UNSIGNED, STRUCTURE_ID];
    op_get_private_name, OpGetPrivateNameMetadata {
        structure_id: StructureId,
        offset: u32,
        property: HeapRef,
    } layout [STRUCTURE_ID, UNSIGNED, POINTER];
    op_get_by_val_with_this, OpGetByValWithThisMetadata { array_profile: ArrayProfile } layout [ARRAY_PROFILE];
    op_get_by_val, OpGetByValMetadata { array_profile: ArrayProfile } layout [ARRAY_PROFILE];
    op_put_by_val, OpPutByValMetadata { array_profile: ArrayProfile } layout [ARRAY_PROFILE];
    op_put_by_val_direct, OpPutByValDirectMetadata { array_profile: ArrayProfile } layout [ARRAY_PROFILE];
    op_in_by_val, OpInByValMetadata { array_profile: ArrayProfile } layout [ARRAY_PROFILE];
    op_enumerator_next, OpEnumeratorNextMetadata {
        array_profile: ArrayProfile,
        enumerator_metadata: u8,
    } layout [ARRAY_PROFILE, ENUMERATOR_METADATA];
    op_enumerator_in_by_val, OpEnumeratorInByValMetadata {
        array_profile: ArrayProfile,
        enumerator_metadata: u8,
    } layout [ARRAY_PROFILE, ENUMERATOR_METADATA];
    op_enumerator_has_own_property, OpEnumeratorHasOwnPropertyMetadata {
        array_profile: ArrayProfile,
        enumerator_metadata: u8,
    } layout [ARRAY_PROFILE, ENUMERATOR_METADATA];
    op_enumerator_put_by_val, OpEnumeratorPutByValMetadata {
        array_profile: ArrayProfile,
        enumerator_metadata: u8,
    } layout [ARRAY_PROFILE, ENUMERATOR_METADATA];
    op_to_this, OpToThisMetadata {
        cached_structure_id: StructureId,
        to_this_status: ToThisStatus,
    } layout [STRUCTURE_ID, TO_THIS_STATUS];
    op_enumerator_get_by_val, OpEnumeratorGetByValMetadata {
        array_profile: ArrayProfile,
        enumerator_metadata: u8,
    } layout [ARRAY_PROFILE, ENUMERATOR_METADATA];
    op_get_by_id_direct, OpGetByIdDirectMetadata {
        structure_id: StructureId,
        offset: u32,
    } layout [STRUCTURE_ID, UNSIGNED];
    op_jneq_ptr, OpJneqPtrMetadata { has_jumped: bool } layout [BOOL];
}

/// `m_resolveType` do `OpResolveScope::Metadata`: o `ResolveType` com o valor inicial do C++
/// (zero, `GlobalProperty`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveTypeField(pub ResolveType);

impl Default for ResolveTypeField {
    fn default() -> ResolveTypeField {
        ResolveTypeField(ResolveType::GlobalProperty)
    }
}

impl OpGetFromScopeMetadata {
    /// `Metadata(const OpGetFromScope&)`: `m_getPutInfo` e `m_operand` vêm do operando do opcode.
    pub fn new(get_put_info: GetPutInfo, offset: u32) -> OpGetFromScopeMetadata {
        OpGetFromScopeMetadata { get_put_info, watchpoint_set_or_structure_id: 0, operand: offset as u64 }
    }
}

impl OpPutToScopeMetadata {
    /// `Metadata(const OpPutToScope&)`.
    pub fn new(get_put_info: GetPutInfo, offset: u32) -> OpPutToScopeMetadata {
        OpPutToScopeMetadata { get_put_info, watchpoint_set_or_structure_id: 0, operand: offset as u64 }
    }
}
