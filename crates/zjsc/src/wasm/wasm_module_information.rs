//! Tradução de `wasm/WasmModuleInformation.h`, com as peças de `WasmFormat.h` e
//! `WasmTypeDefinition.h` de que o parse das seções precisa.
//!
//! `m_rtts` (`Vector<Ref<const RTT>>`, os tipos canônicos) vira `types`, a lista dos tipos na ordem
//! da seção, cada um com a sua estrutura. A canonicalização isorecursiva
//! (`TypeInformation::canonicalize*`, que faz tipos iguais compartilharem o mesmo RTT) é feita
//! aqui dentro do módulo, em `append_recursion_group`: dois tipos são o mesmo RTT quando o grupo
//! recursivo inteiro é igual membro a membro, com a referência a um membro do próprio grupo contada
//! pela posição relativa e a referência a um tipo anterior contada pelo RTT canônico dele (as
//! mesmas regras de `hashRTTForRecGroup` e `equalRTTsForRecGroup`). O identificador canônico (o
//! ponteiro do RTT no C++) vai em `TypeIndex::Concrete`. O interning entre módulos, que
//! `TypeInformation` faz num registro do processo, entra com a fatia própria dele.
//!
//! Fica de fora, até a fatia da seção `name` e das customizadas: `sourceURL`,
//! `nameSection`, as listas de builtins de string e `debugInfo`.

use std::collections::HashMap;
use crate::wasm::wasm_name_section::NameSection;

use crate::runtime::options::Options;
use crate::wasm::wasm_format::{
    Element, Export, FieldType, FunctionData, GlobalInformation, Import, Mutability, PackedType, Segment, StorageType,
    Name, TableInformation, Type, TypeIndex, TypeKind, funcref_type, is_ref_to_abstract, is_ref_type, is_ref_with_type_index,
};
use crate::wasm::wasm_memory_information::MemoryInformation;
use crate::wtf::bit_vector::BitVector;

/// `RTTKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RttKind {
    Function,
    Struct,
    Array,
}

/// A estrutura de um tipo definido (o que o `RTT` do C++ descreve): função, struct ou array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructuralType {
    Function { arguments: Vec<Type>, returns: Vec<Type> },
    Struct { fields: Vec<FieldType> },
    Array { element: FieldType },
}

impl StructuralType {
    /// `RTT::kind`.
    pub fn kind(&self) -> RttKind {
        match self {
            StructuralType::Function { .. } => RttKind::Function,
            StructuralType::Struct { .. } => RttKind::Struct,
            StructuralType::Array { .. } => RttKind::Array,
        }
    }
}

/// `Subtype`: o tipo declarado com `sub` ou `sub final`. Um tipo `final` sem supertipo é
/// normalizado para a estrutura sozinha (ver `SectionParser::parseSubtype`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subtype {
    pub super_types: Vec<TypeIndex>,
    pub is_final: bool,
}

/// Um item da seção de tipos, na posição do seu índice de tipo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDefinition {
    pub structural: StructuralType,
    pub subtype: Option<Subtype>,
    /// Posição do primeiro membro do grupo recursivo a que o tipo pertence.
    pub recursion_group_start: u32,
    /// Quantos tipos o grupo recursivo tem (1 para um tipo avulso).
    pub recursion_group_len: u32,
}

impl TypeDefinition {
    /// `RTT::isFinalType`: um tipo sem `sub` é a forma abreviada de `sub final`.
    pub fn is_final_type(&self) -> bool {
        self.subtype.as_ref().map_or(true, |subtype| subtype.is_final)
    }
}

/// `CustomSection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomSection {
    pub name: String,
    pub payload: Vec<u8>,
}

/// Para onde uma referência aponta, na chave de canonicalização de um grupo recursivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CanonTarget {
    /// Um tipo sem tipo heap (`i32`, `f64`...).
    None,
    Abstract(TypeKind),
    /// Um RTT canônico de um grupo anterior (`EncodedRef` com o ponteiro do RTT).
    External(u32),
    /// Um membro do próprio grupo, pela posição relativa (`EncodedRef` com o `ProjectionIndex`).
    Intra(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CanonType {
    kind: TypeKind,
    target: CanonTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CanonStorage {
    Packed(PackedType),
    Type(CanonType),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CanonField {
    mutability: Mutability,
    ty: CanonStorage,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CanonShape {
    Function { arguments: Vec<CanonType>, returns: Vec<CanonType> },
    Struct { fields: Vec<CanonField> },
    Array { element: CanonField },
}

/// Um membro de grupo na chave: o que `equalRTTsForRecGroup` compara.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CanonMember {
    is_final: bool,
    super_type: Option<CanonTarget>,
    shape: CanonShape,
}

fn canon_target(index: TypeIndex) -> CanonTarget {
    match index {
        TypeIndex::Invalid => CanonTarget::None,
        TypeIndex::Abstract(kind) => CanonTarget::Abstract(kind),
        TypeIndex::Concrete(id) => CanonTarget::External(id),
        TypeIndex::Projection(relative) => CanonTarget::Intra(relative),
    }
}

fn canon_type(ty: Type) -> CanonType {
    CanonType { kind: ty.kind, target: canon_target(ty.index) }
}

fn canon_field(field: FieldType) -> CanonField {
    let ty = match field.ty {
        StorageType::Packed(packed) => CanonStorage::Packed(packed),
        StorageType::Type(ty) => CanonStorage::Type(canon_type(ty)),
    };
    CanonField { mutability: field.mutability, ty }
}

fn canon_member(definition: &TypeDefinition) -> CanonMember {
    let shape = match &definition.structural {
        StructuralType::Function { arguments, returns } => CanonShape::Function {
            arguments: arguments.iter().copied().map(canon_type).collect(),
            returns: returns.iter().copied().map(canon_type).collect(),
        },
        StructuralType::Struct { fields } => CanonShape::Struct { fields: fields.iter().copied().map(canon_field).collect() },
        StructuralType::Array { element } => CanonShape::Array { element: canon_field(*element) },
    };
    let super_type = definition
        .subtype
        .as_ref()
        .and_then(|subtype| subtype.super_types.first())
        .map(|index| canon_target(*index));
    CanonMember { is_final: definition.is_final_type(), super_type, shape }
}

/// A troca do `expand()` do C++: o placeholder `Projection(relativo)` vira o RTT canônico do
/// membro `base + relativo`.
fn expand_index(index: &mut TypeIndex, base: u32) {
    if let TypeIndex::Projection(relative) = *index {
        *index = TypeIndex::Concrete(base + relative);
    }
}

fn expand_storage(ty: &mut StorageType, base: u32) {
    if let StorageType::Type(inner) = ty {
        expand_index(&mut inner.index, base);
    }
}

fn expand_projections(definition: &mut TypeDefinition, base: u32) {
    match &mut definition.structural {
        StructuralType::Function { arguments, returns } => {
            for ty in arguments.iter_mut().chain(returns.iter_mut()) {
                expand_index(&mut ty.index, base);
            }
        }
        StructuralType::Struct { fields } => {
            for field in fields {
                expand_storage(&mut field.ty, base);
            }
        }
        StructuralType::Array { element } => expand_storage(&mut element.ty, base),
    }
    if let Some(subtype) = &mut definition.subtype {
        for index in &mut subtype.super_types {
            expand_index(index, base);
        }
    }
}

/// Um RTT canônico do registro do thread (`TypeInformation::singleton()`).
#[derive(Clone, Copy, Debug)]
struct CanonicalEntry {
    /// O pai imediato na cadeia de supertipos (a última entrada do display).
    super_type: Option<u32>,
    /// `displaySizeExcludingThis`: quantos ancestrais o RTT tem.
    depth: u32,
    kind: RttKind,
}

/// O registro canônico por thread (o `TypeInformation` do processo): os grupos recursivos de todos os módulos são
/// internados aqui por estrutura (iso-recursivo), então o mesmo tipo tem o mesmo identificador em módulos diferentes.
#[derive(Default)]
struct TypeRegistry {
    /// A tabela de `RTTGroup`: a chave do grupo e o identificador canônico do primeiro membro.
    groups: HashMap<Vec<CanonMember>, u32>,
    entries: Vec<CanonicalEntry>,
}

thread_local! {
    static TYPE_REGISTRY: std::cell::RefCell<TypeRegistry> = std::cell::RefCell::new(TypeRegistry::default());
}

fn registry_entry(id: u32) -> CanonicalEntry {
    TYPE_REGISTRY.with(|registry| registry.borrow().entries[id as usize])
}

/// `isStrictSubRTT`: o pai está na cadeia de ancestrais, e não é o próprio RTT.
fn is_strict_sub_rtt(sub: u32, parent: u32) -> bool {
    let parent_depth = registry_entry(parent).depth;
    if registry_entry(sub).depth <= parent_depth {
        return false;
    }
    let mut current = sub;
    while registry_entry(current).depth > parent_depth {
        current = registry_entry(current).super_type.expect("um RTT com profundidade positiva tem pai");
    }
    current == parent
}

/// `isSubtypeIndex`: os dois são tipos definidos. Os RTTs canônicos são globais, então a checagem não precisa de
/// módulo nem de instância (Table e Global soltas a usam).
pub fn is_subtype_index(sub: TypeIndex, parent: TypeIndex) -> bool {
    if sub == parent {
        return true;
    }
    match (sub, parent) {
        (TypeIndex::Concrete(sub_id), TypeIndex::Concrete(parent_id)) => is_strict_sub_rtt(sub_id, parent_id),
        other => unreachable!("isSubtypeIndex pede tipos definidos: {other:?}"),
    }
}

/// Os RTTs canônicos do módulo: as posições da seção de tipos e a definição que representa cada id (ids são globais).
#[derive(Clone, Debug, Default)]
struct CanonicalTypes {
    /// O RTT canônico de cada posição da seção de tipos.
    position_ids: Vec<u32>,
    /// A primeira posição da seção de tipos deste módulo que tem o RTT de cada id.
    representatives: HashMap<u32, u32>,
}

/// `BranchHint` (`WasmBranchHints.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchHint {
    Unlikely,
    Likely,
    Invalid,
}

/// `isValidBranchHint`.
pub const fn is_valid_branch_hint(hint: BranchHint) -> bool {
    matches!(hint, BranchHint::Likely | BranchHint::Unlikely)
}

/// `BranchHintMap`.
#[derive(Clone, Debug, Default)]
pub struct BranchHintMap {
    map: HashMap<u32, BranchHint>,
}

impl BranchHintMap {
    /// `add`: como o `HashMap::add` do WTF, não troca o valor de uma chave que já existe.
    pub fn add(&mut self, branch_offset: u32, hint: BranchHint) {
        self.map.entry(branch_offset).or_insert(hint);
    }

    /// `getBranchHint`.
    pub fn get_branch_hint(&self, branch_offset: u32) -> BranchHint {
        self.map.get(&branch_offset).copied().unwrap_or(BranchHint::Invalid)
    }

    /// `isValidKey` de `UnsignedWithZeroKeyHashTraits`: o valor vazio (`max`) e o apagado (`max - 1`) não servem de chave.
    pub fn is_valid_key(&self, branch_offset: u32) -> bool {
        branch_offset < u32::MAX - 1
    }
}

/// `ModuleInformation`.
#[derive(Clone, Debug, Default)]
pub struct ModuleInformation {
    pub imports: Vec<Import>,
    /// `importShouldBeHidden` (um `FixedBitVector`), do tamanho de `imports` depois do parse.
    pub import_should_be_hidden: Vec<bool>,
    /// `Vector<TypeSignatureIndex>`: posições na seção de tipos.
    pub import_function_type_signature_indices: Vec<u32>,
    pub internal_function_type_signature_indices: Vec<u32>,
    pub import_exception_type_signature_indices: Vec<u32>,
    pub internal_exception_type_signature_indices: Vec<u32>,
    pub memories: Vec<MemoryInformation>,
    /// `m_hasGCObjectTypes`.
    pub has_gc_object_types: bool,
    pub functions: Vec<FunctionData>,
    pub exports: Vec<Export>,
    /// `startFunctionIndexSpace`.
    pub start_function_index_space: Option<u32>,
    pub data: Vec<Segment>,
    pub elements: Vec<Element>,
    pub tables: Vec<TableInformation>,
    pub globals: Vec<GlobalInformation>,
    pub first_internal_global: usize,
    pub code_section_size: u32,
    pub custom_sections: Vec<CustomSection>,
    /// `branchHints` (`metadata.code.branch_hint`): por índice de função, os hints por offset.
    pub branch_hints: HashMap<u32, BranchHintMap>,
    /// `sourceMappingURL`.
    pub source_mapping_url: Name,
    pub number_of_data_segments: Option<u32>,
    /// `nameSection()`.
    pub name_section: NameSection,
    /// `m_hasCustomNameSection`.
    has_custom_name_section: bool,
    /// `constantExpressions`: o texto da expressão e o offset dela no módulo.
    pub constant_expressions: Vec<(Vec<u8>, usize)>,
    /// `m_declaredFunctions`.
    declared_functions: BitVector,
    /// `m_declaredExceptions`.
    declared_exceptions: BitVector,
    /// `m_totalFunctionSize`.
    pub total_function_size: usize,
    /// `m_numSmallFunctions`.
    pub num_small_functions: u32,
    /// `m_usesLegacyExceptions`: algum corpo usa `try`/`catch`/`delegate`.
    pub uses_legacy_exceptions: bool,
    /// `m_usesModernExceptions`: algum corpo usa `try_table`.
    pub uses_modern_exceptions: bool,
    /// `m_rtts`: os tipos da seção, na ordem.
    pub types: Vec<TypeDefinition>,
    canonical: CanonicalTypes,
}

impl ModuleInformation {
    /// `setNameSection`: só a primeira seção `name` vale, as seguintes são ignoradas (a nota
    /// editorial da especificação).
    pub fn set_name_section(&mut self, section: NameSection) {
        if self.has_custom_name_section {
            return;
        }
        self.has_custom_name_section = true;
        self.name_section = section;
    }

    /// `getBranchHint`.
    pub fn get_branch_hint(&self, function_offset: u32, branch_offset: u32) -> BranchHint {
        self.branch_hints.get(&function_offset).map_or(BranchHint::Invalid, |map| map.get_branch_hint(branch_offset))
    }

    /// `typeCount()`.
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// `rtt(TypeSignatureIndex)`.
    pub fn rtt(&self, position: usize) -> &TypeDefinition {
        &self.types[position]
    }

    /// `functionIndexSpaceSize`.
    pub fn function_index_space_size(&self) -> usize {
        self.import_function_type_signature_indices.len() + self.internal_function_type_signature_indices.len()
    }

    /// `typeSignatureIndexFromFunctionIndexSpace`.
    pub fn type_signature_index_from_function_index_space(&self, function_index: usize) -> u32 {
        let import_count = self.import_function_type_signature_indices.len();
        if function_index < import_count {
            self.import_function_type_signature_indices[function_index]
        } else {
            self.internal_function_type_signature_indices[function_index - import_count]
        }
    }

    /// `rtt(FunctionSpaceIndex)`.
    pub fn rtt_from_function_index_space(&self, function_index: usize) -> &TypeDefinition {
        self.rtt(self.type_signature_index_from_function_index_space(function_index) as usize)
    }

    /// `exceptionIndexSpaceSize`.
    pub fn exception_index_space_size(&self) -> usize {
        self.import_exception_type_signature_indices.len() + self.internal_exception_type_signature_indices.len()
    }

    /// `typeSignatureIndexFromExceptionIndexSpace`.
    pub fn type_signature_index_from_exception_index_space(&self, exception_index: usize) -> u32 {
        let import_count = self.import_exception_type_signature_indices.len();
        if exception_index < import_count {
            self.import_exception_type_signature_indices[exception_index]
        } else {
            self.internal_exception_type_signature_indices[exception_index - import_count]
        }
    }

    /// `rttFromExceptionIndexSpace`.
    pub fn rtt_from_exception_index_space(&self, exception_index: usize) -> &TypeDefinition {
        self.rtt(self.type_signature_index_from_exception_index_space(exception_index) as usize)
    }

    /// `toCodeIndex`: tira os imports do índice no espaço de funções.
    pub fn to_code_index(&self, space_index: usize) -> usize {
        let import_count = self.import_function_type_signature_indices.len();
        assert!(import_count <= space_index && space_index < self.function_index_space_size());
        space_index - import_count
    }

    /// `toSpaceIndex`.
    pub fn to_space_index(&self, code_index: usize) -> usize {
        assert!(code_index < self.internal_function_type_signature_indices.len());
        code_index + self.import_function_type_signature_indices.len()
    }

    /// `memoryCount`.
    pub fn memory_count(&self) -> usize {
        self.memories.len()
    }

    /// `tableCount`.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// `elementCount`.
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// `globalCount`.
    pub fn global_count(&self) -> usize {
        self.globals.len()
    }

    /// `dataSegmentsCount`.
    pub fn data_segments_count(&self) -> u32 {
        self.number_of_data_segments.unwrap_or(0)
    }

    /// `isDeclaredFunction`.
    pub fn is_declared_function(&self, function_index: usize) -> bool {
        self.declared_functions.contains(function_index)
    }

    /// `addDeclaredFunction`.
    pub fn add_declared_function(&mut self, function_index: usize) {
        self.declared_functions.set_bit(function_index);
    }

    /// `isDeclaredException`.
    pub fn is_declared_exception(&self, exception_index: usize) -> bool {
        self.declared_exceptions.contains(exception_index)
    }

    /// `addDeclaredException`.
    pub fn add_declared_exception(&mut self, exception_index: usize) {
        self.declared_exceptions.set_bit(exception_index);
    }

    /// `functionWasmSize`.
    pub fn function_wasm_size(&self, code_index: usize) -> usize {
        let function = &self.functions[code_index];
        assert!(function.finished_validating);
        function.end - function.start
    }

    /// `usesSIMD`.
    pub fn uses_simd(&self, code_index: usize) -> bool {
        assert!(self.functions[code_index].finished_validating);
        // See also: B3Procedure::usesSIMD().
        if !Options::with(|options| options.use_wasm_simd) {
            return false;
        }
        if Options::with(|options| options.force_all_functions_to_use_simd) {
            return true;
        }
        self.functions[code_index].uses_simd
    }

    /// `markUsesSIMD`.
    pub fn mark_uses_simd(&mut self, code_index: usize) {
        assert!(!self.functions[code_index].finished_validating);
        self.functions[code_index].uses_simd = true;
    }

    /// `usesExceptions`.
    pub fn uses_exceptions(&self, code_index: usize) -> bool {
        assert!(self.functions[code_index].finished_validating);
        self.functions[code_index].uses_exceptions
    }

    /// `markUsesExceptions`.
    pub fn mark_uses_exceptions(&mut self, code_index: usize) {
        assert!(!self.functions[code_index].finished_validating);
        self.functions[code_index].uses_exceptions = true;
    }

    /// `usesAtomics`.
    pub fn uses_atomics(&self, code_index: usize) -> bool {
        assert!(self.functions[code_index].finished_validating);
        self.functions[code_index].uses_atomics
    }

    /// `markUsesAtomics`.
    pub fn mark_uses_atomics(&mut self, code_index: usize) {
        assert!(!self.functions[code_index].finished_validating);
        self.functions[code_index].uses_atomics = true;
    }

    /// `doneSeeingFunction`.
    pub fn done_seeing_function(&mut self, code_index: usize) {
        assert!(!self.functions[code_index].finished_validating);
        self.functions[code_index].finished_validating = true;
    }

    /// `hasMemoryImport`.
    pub fn has_memory_import(&self) -> bool {
        self.memories.iter().any(|memory| memory.is_import)
    }

    /// O `m_info->m_rtts.append` de um grupo recursivo (um tipo avulso é o grupo de um membro só):
    /// acrescenta os membros, acha ou cria o RTT canônico do grupo e troca os placeholders
    /// `Projection` pelos RTTs canônicos.
    pub fn append_recursion_group(&mut self, members: Vec<(StructuralType, Option<Subtype>)>) {
        let start = self.types.len();
        let len = members.len();
        assert!(len > 0);
        for (structural, subtype) in members {
            self.types.push(TypeDefinition {
                structural,
                subtype,
                recursion_group_start: start as u32,
                recursion_group_len: len as u32,
            });
        }

        let key: Vec<CanonMember> = self.types[start..].iter().map(canon_member).collect();
        let existing = TYPE_REGISTRY.with(|registry| registry.borrow().groups.get(&key).copied());
        let base = existing.unwrap_or_else(|| TYPE_REGISTRY.with(|registry| registry.borrow().entries.len() as u32));
        if existing.is_none() {
            TYPE_REGISTRY.with(|registry| registry.borrow_mut().groups.insert(key, base));
        }

        for definition in &mut self.types[start..] {
            expand_projections(definition, base);
        }

        for member in 0..len {
            self.canonical.representatives.entry(base + member as u32).or_insert((start + member) as u32);
        }
        if existing.is_none() {
            for member in 0..len {
                // Um supertipo do mesmo grupo é sempre um membro anterior (`parseSubtype` recusa a
                // referência para frente), então a entrada dele já existe.
                let super_type = self.types[start + member]
                    .subtype
                    .as_ref()
                    .and_then(|subtype| subtype.super_types.first())
                    .map(|index| match *index {
                        TypeIndex::Concrete(id) => id,
                        other => unreachable!("supertipo sem RTT canônico depois do expand: {other:?}"),
                    });
                let depth = super_type.map_or(0, |id| registry_entry(id).depth + 1);
                let kind = self.types[start + member].structural.kind();
                TYPE_REGISTRY.with(|registry| registry.borrow_mut().entries.push(CanonicalEntry { super_type, depth, kind }));
            }
        }
        for member in 0..len {
            self.canonical.position_ids.push(base + member as u32);
        }
    }

    /// O RTT canônico do tipo na posição dada.
    pub fn canonical_type_id(&self, position: usize) -> u32 {
        self.canonical.position_ids[position]
    }

    /// `rtt(...).asTypeIndex()`.
    pub fn type_index_of(&self, position: usize) -> TypeIndex {
        TypeIndex::Concrete(self.canonical_type_id(position))
    }

    /// O RTT canônico pelo identificador: a definição do primeiro tipo do módulo que o tem.
    pub fn canonical_rtt(&self, id: u32) -> &TypeDefinition {
        &self.types[self.canonical.representatives[&id] as usize]
    }

    /// `displaySizeExcludingThis` do RTT do tipo na posição dada.
    pub fn display_size_excluding_this(&self, position: usize) -> u32 {
        registry_entry(self.canonical_type_id(position)).depth
    }

    /// A última entrada do display do RTT (`displayEntry(displaySizeExcludingThis() - 1)`): o pai
    /// imediato, `None` sem supertipo.
    pub fn direct_super_rtt(&self, id: u32) -> Option<&TypeDefinition> {
        registry_entry(id).super_type.map(|super_id| self.canonical_rtt(super_id))
    }

    /// O `getCanonicalRTT(index)->kind()` de um tipo heap que é tipo definido.
    fn defined_kind(&self, index: TypeIndex) -> RttKind {
        match index {
            TypeIndex::Concrete(id) => registry_entry(id).kind,
            other => unreachable!("tipo heap sem RTT canônico: {other:?}"),
        }
    }

    /// `isInternalref`.
    pub fn is_internalref(&self, ty: Type) -> bool {
        if !is_ref_type(ty) {
            return false;
        }
        match ty.index {
            TypeIndex::Abstract(kind) => matches!(
                kind,
                TypeKind::I31ref
                    | TypeKind::Arrayref
                    | TypeKind::Structref
                    | TypeKind::Eqref
                    | TypeKind::Anyref
                    | TypeKind::Noneref
            ),
            index => self.defined_kind(index) != RttKind::Function,
        }
    }

    /// `isSubtype(Type, Type)` com o `isSubtypeSlow`. Ver a hierarquia no comentário de
    /// `WasmFormat.h`: `any > eq > {i31, array, struct} > none`, `func > nofunc`,
    /// `extern > noextern`, `exn > noexn`.
    pub fn is_subtype(&self, sub: Type, parent: Type) -> bool {
        // Fast path.
        if sub == parent {
            return true;
        }
        // Before the typed funcref proposal there is no non-trivial subtyping.
        if sub.is_nullable() && !parent.is_nullable() {
            return false;
        }

        let parent_is_any_or_eq =
            is_ref_to_abstract(parent, TypeKind::Anyref) || is_ref_to_abstract(parent, TypeKind::Eqref);

        if is_ref_with_type_index(sub) {
            if is_ref_with_type_index(parent) {
                return is_subtype_index(sub.index, parent.index);
            }
            let sub_kind = self.defined_kind(sub.index);
            if parent_is_any_or_eq {
                return sub_kind != RttKind::Function;
            }
            if is_ref_to_abstract(parent, TypeKind::Arrayref) {
                return sub_kind == RttKind::Array;
            }
            if is_ref_to_abstract(parent, TypeKind::Structref) {
                return sub_kind == RttKind::Struct;
            }
            if is_ref_to_abstract(parent, TypeKind::Funcref) {
                return sub_kind == RttKind::Function;
            }
        }

        let sub_is_i31_struct_or_array = is_ref_to_abstract(sub, TypeKind::I31ref)
            || is_ref_to_abstract(sub, TypeKind::Structref)
            || is_ref_to_abstract(sub, TypeKind::Arrayref);
        if sub_is_i31_struct_or_array && parent_is_any_or_eq {
            return true;
        }

        if is_ref_to_abstract(sub, TypeKind::Eqref) && is_ref_to_abstract(parent, TypeKind::Anyref) {
            return true;
        }

        if is_ref_to_abstract(sub, TypeKind::Noneref) {
            return self.is_internalref(parent);
        }

        if is_ref_to_abstract(sub, TypeKind::Nofuncref) {
            return self.is_subtype(parent, funcref_type());
        }

        if is_ref_to_abstract(sub, TypeKind::Noexternref) && is_ref_to_abstract(parent, TypeKind::Externref) {
            return true;
        }

        if is_ref_to_abstract(sub, TypeKind::Noexnref) && is_ref_to_abstract(parent, TypeKind::Exnref) {
            return true;
        }

        if sub.is_ref() && parent.is_ref_null() {
            return sub.index == parent.index;
        }

        false
    }

    /// `isSubtype(StorageType, StorageType)`.
    pub fn is_subtype_storage(&self, sub: StorageType, parent: StorageType) -> bool {
        match (sub, parent) {
            (StorageType::Type(sub), StorageType::Type(parent)) => self.is_subtype(sub, parent),
            (sub, parent) => sub == parent,
        }
    }

    /// `Type::dump`: o texto que as mensagens de erro usam para um tipo.
    pub fn type_to_string(&self, ty: Type) -> String {
        let kind_to_print = match ty.index {
            TypeIndex::Invalid => ty.kind,
            TypeIndex::Abstract(kind) => kind,
            TypeIndex::Concrete(id) => return rtt_to_string(self.canonical_rtt(id)),
            TypeIndex::Projection(relative) => unreachable!("Type::dump de um placeholder {relative}"),
        };
        kind_to_print.name().to_string()
    }
}

/// `makeString(StorageType)`.
fn storage_type_name(ty: StorageType) -> &'static str {
    match ty {
        StorageType::Type(ty) => ty.kind.name(),
        StorageType::Packed(packed) => packed.name(),
    }
}

/// `RTT::dump`. O prefixo de mutabilidade sai invertido (`mutability ? "immutable " : "mutable "`
/// com `Mutable = 1`): é o que o C++ imprime, e a mensagem de erro herda.
pub(crate) fn rtt_to_string(definition: &TypeDefinition) -> String {
    fn field_to_string(field: &FieldType) -> String {
        let prefix = if field.mutability == Mutability::Mutable { "immutable " } else { "mutable " };
        format!("{}{}", prefix, storage_type_name(field.ty))
    }
    match &definition.structural {
        StructuralType::Function { arguments, returns } => {
            let arguments: Vec<&str> = arguments.iter().map(|ty| ty.kind.name()).collect();
            let returns: Vec<&str> = returns.iter().map(|ty| ty.kind.name()).collect();
            format!("({}) -> [{}]", arguments.join(", "), returns.join(", "))
        }
        StructuralType::Struct { fields } => {
            let fields: Vec<String> = fields.iter().map(field_to_string).collect();
            format!("({})", fields.join(", "))
        }
        StructuralType::Array { element } => format!("({})", field_to_string(element)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::{TYPE_I32, TYPE_I64, non_null_funcref_type};

    fn function(arguments: Vec<Type>, returns: Vec<Type>) -> StructuralType {
        StructuralType::Function { arguments, returns }
    }

    fn struct_of(fields: Vec<FieldType>) -> StructuralType {
        StructuralType::Struct { fields }
    }

    fn field(ty: Type, mutability: Mutability) -> FieldType {
        FieldType { ty: StorageType::Type(ty), mutability }
    }

    fn reference(kind: TypeKind, index: TypeIndex) -> Type {
        Type::new(kind, index)
    }

    #[test]
    fn equal_standalone_types_share_the_canonical_rtt() {
        let mut info = ModuleInformation::default();
        info.append_recursion_group(vec![(function(vec![TYPE_I32], vec![]), None)]);
        info.append_recursion_group(vec![(function(vec![TYPE_I64], vec![]), None)]);
        info.append_recursion_group(vec![(function(vec![TYPE_I32], vec![]), None)]);
        assert_eq!(info.canonical_type_id(0), info.canonical_type_id(2));
        assert_ne!(info.canonical_type_id(0), info.canonical_type_id(1));
        assert_eq!(info.type_index_of(0), info.type_index_of(2));
        let ref_to_first = reference(TypeKind::Ref, info.type_index_of(0));
        let ref_to_third = reference(TypeKind::Ref, info.type_index_of(2));
        assert!(info.is_subtype(ref_to_first, ref_to_third));
    }

    #[test]
    fn finality_and_supertype_are_part_of_the_identity() {
        let mut info = ModuleInformation::default();
        // O tipo 0 é aberto (`sub`), o 1 é final: a mesma estrutura, RTTs diferentes.
        info.append_recursion_group(vec![(function(vec![], vec![]), Some(Subtype { super_types: vec![], is_final: false }))]);
        info.append_recursion_group(vec![(function(vec![], vec![]), None)]);
        assert_ne!(info.canonical_type_id(0), info.canonical_type_id(1));
        // O tipo 2 é filho do 0: um subtipo estrito, com profundidade 1.
        info.append_recursion_group(vec![(
            function(vec![], vec![]),
            Some(Subtype { super_types: vec![info.type_index_of(0)], is_final: true }),
        )]);
        assert_eq!(info.display_size_excluding_this(0), 0);
        assert_eq!(info.display_size_excluding_this(2), 1);
        let sub = reference(TypeKind::Ref, info.type_index_of(2));
        let parent = reference(TypeKind::RefNull, info.type_index_of(0));
        let unrelated = reference(TypeKind::Ref, info.type_index_of(1));
        assert!(info.is_subtype(sub, parent));
        assert!(!info.is_subtype(parent, sub));
        assert!(!info.is_subtype(sub, unrelated));
        assert!(info.direct_super_rtt(info.canonical_type_id(2)).is_some());
    }

    #[test]
    fn recursive_group_members_are_canonicalized_with_relative_references() {
        let build = || {
            let mut info = ModuleInformation::default();
            // (rec (type $a (struct (field (ref null $b)))) (type $b (func)))
            let a = struct_of(vec![field(reference(TypeKind::RefNull, TypeIndex::Projection(1)), Mutability::Immutable)]);
            info.append_recursion_group(vec![(a, None), (function(vec![], vec![]), None)]);
            info
        };
        let mut info = build();
        // O mesmo grupo de novo é o mesmo RTT, membro a membro.
        let a_again = struct_of(vec![field(reference(TypeKind::RefNull, TypeIndex::Projection(1)), Mutability::Immutable)]);
        info.append_recursion_group(vec![(a_again, None), (function(vec![], vec![]), None)]);
        assert_eq!(info.canonical_type_id(0), info.canonical_type_id(2));
        assert_eq!(info.canonical_type_id(1), info.canonical_type_id(3));
        // O placeholder virou o RTT canônico do membro 1.
        let expected = TypeIndex::Concrete(info.canonical_type_id(1));
        match &info.types[0].structural {
            StructuralType::Struct { fields } => match fields[0].ty {
                StorageType::Type(ty) => assert_eq!(ty.index, expected),
                other => panic!("esperava um tipo, veio {other:?}"),
            },
            other => panic!("esperava struct, veio {other:?}"),
        }
        // E um grupo com a referência trocada de lugar é outro.
        let mut other_info = ModuleInformation::default();
        let swapped = struct_of(vec![field(reference(TypeKind::RefNull, TypeIndex::Projection(0)), Mutability::Immutable)]);
        other_info.append_recursion_group(vec![(swapped, None), (function(vec![], vec![]), None)]);
        assert_eq!(other_info.type_count(), 2);
    }

    #[test]
    fn abstract_heap_hierarchy() {
        let info = ModuleInformation::default();
        let any = |kind| reference(TypeKind::RefNull, TypeIndex::Abstract(kind));
        assert!(info.is_subtype(any(TypeKind::Eqref), any(TypeKind::Anyref)));
        assert!(info.is_subtype(any(TypeKind::I31ref), any(TypeKind::Eqref)));
        assert!(info.is_subtype(any(TypeKind::Noneref), any(TypeKind::Structref)));
        assert!(info.is_subtype(any(TypeKind::Nofuncref), funcref_type()));
        assert!(info.is_subtype(any(TypeKind::Noexternref), any(TypeKind::Externref)));
        assert!(!info.is_subtype(any(TypeKind::Externref), any(TypeKind::Anyref)));
        assert!(!info.is_subtype(any(TypeKind::Anyref), any(TypeKind::Eqref)));
        // Nulável nunca é subtipo de não nulável.
        assert!(!info.is_subtype(funcref_type(), non_null_funcref_type()));
        assert!(info.is_subtype(non_null_funcref_type(), funcref_type()));
    }

    #[test]
    fn type_to_string_matches_type_dump() {
        let mut info = ModuleInformation::default();
        info.append_recursion_group(vec![(function(vec![TYPE_I32, funcref_type()], vec![TYPE_I64]), None)]);
        info.append_recursion_group(vec![(
            struct_of(vec![field(TYPE_I32, Mutability::Mutable), field(TYPE_I64, Mutability::Immutable)]),
            None,
        )]);
        assert_eq!(info.type_to_string(TYPE_I32), "I32");
        assert_eq!(info.type_to_string(funcref_type()), "Funcref");
        assert_eq!(info.type_to_string(reference(TypeKind::Ref, info.type_index_of(0))), "(I32, RefNull) -> [I64]");
        // O C++ imprime o prefixo de mutabilidade invertido.
        assert_eq!(info.type_to_string(reference(TypeKind::Ref, info.type_index_of(1))), "(immutable I32, mutable I64)");
    }

    #[test]
    fn function_index_space_puts_imports_first() {
        let mut info = ModuleInformation::default();
        info.import_function_type_signature_indices = vec![3, 4];
        info.internal_function_type_signature_indices = vec![5];
        assert_eq!(info.function_index_space_size(), 3);
        assert_eq!(info.type_signature_index_from_function_index_space(1), 4);
        assert_eq!(info.type_signature_index_from_function_index_space(2), 5);
        assert_eq!(info.to_code_index(2), 0);
        assert_eq!(info.to_space_index(0), 2);
        info.add_declared_function(2);
        assert!(info.is_declared_function(2));
        assert!(!info.is_declared_function(1));
    }
}
