//! Tradução de `wasm/WasmSectionParser.cpp`/`.h`: um parser por seção (Type, Import, Function,
//! Table, Memory, Global, Export, Start, Element, DataCount, Exception, Data) e a seção
//! customizada. A seção `Code` não passa por aqui: o `StreamingParser` enquadra os corpos (o
//! `parseCode` do C++ é `RELEASE_ASSERT_NOT_REACHED`).
//!
//! Ainda não portado (cada item é uma fatia do `wip-notes/wasm-plan.md`):
//!
//! - os parsers de nome, de branch hints e de sourceMappingURL da seção customizada.

use std::collections::HashSet;

use crate::runtime::js_value::js_null;
use crate::runtime::options::Options;
use crate::wasm::wasm_name_section::parse_name_section;
use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_address_type::AddressType;
use crate::wasm::wasm_format::{
    DefinedTypeKind, Element, ElementInitializationType, ElementKind, Export, ExternalKind, FieldType, GlobalBindingMode,
    GlobalInformation, GlobalInitialBits, GlobalInitializationType, I32InitExpr, Import, Mutability, PackedType, Segment,
    SegmentKind, StorageType, TYPE_I32, TYPE_I64, TYPE_V128, TableElementType, TableInformation, TableInitializationType,
    Type, TypeIndex, TypeKind, V128, funcref_type, is_defaultable_type, is_ref_type, is_valid_type_kind,
    non_null_funcref_type, type_index_from_type_kind,
};
use crate::wasm::wasm_const_expr_generator::parse_extended_const_expr;
use crate::wasm::wasm_limits::{
    MAX_DATA_SEGMENTS, MAX_EXCEPTIONS, MAX_EXPORTS, MAX_FUNCTIONS, MAX_FUNCTION_PARAMS, MAX_FUNCTION_RETURNS, MAX_GLOBALS,
    MAX_IMPORTS, MAX_MEMORIES, MAX_MODULE_SIZE, MAX_NUMBER_OF_RECURSION_GROUPS, MAX_RECURSION_GROUP_COUNT,
    MAX_STRUCT_FIELD_COUNT, MAX_SUBTYPE_DEPTH, MAX_SUBTYPE_SUPERTYPE_COUNT, MAX_TABLES, MAX_TABLE_ENTRIES, MAX_TYPES,
    max_declarable_pages,
};
use crate::wasm::wasm_memory_information::MemoryInformation;
use crate::wasm::wasm_module_information::{BranchHint, BranchHintMap, CustomSection, ModuleInformation, RttKind, StructuralType, Subtype};
use crate::wasm::wasm_ops::{EXT_SIMD_V128_CONST, OpType};
use crate::wasm::wasm_parser::{ParserBase, RecursionGroupInformation};
use crate::wasm::wasm_sections::{Section, section_name};

/// `PartialResult`: o texto de erro que a API JS devolve num `CompileError`.
type PartialResult = Result<(), String>;

/// `ParsedDef` sem a parte de ponteiros do C++: a estrutura mais o `Subtype`, quando houver.
struct ParsedDef {
    structural: StructuralType,
    subtype: Option<Subtype>,
}

/// `SectionParser::LimitsType`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LimitsType {
    Memory,
    Table,
}

/// Os quatro valores que `parseResizableLimits` devolve por parâmetro.
struct ResizableLimits {
    initial: u64,
    maximum: Option<u64>,
    is_shared: bool,
    is_64_bit: bool,
}

/// Os cinco valores que `parseInitExpr` devolve por parâmetro (`opcode`,
/// `isExtendedConstantExpression`, `bitsOrImportNumber`, `vectorBits` e `resultType`).
struct InitExpr {
    opcode: u8,
    is_extended: bool,
    bits_or_import_number: u64,
    vector_bits: V128,
    result_type: Type,
}

/// `limitsFlagIsValid`.
fn limits_flag_is_valid(flags: u8) -> bool {
    const INVALID_FLAGS_MASK: u8 = !0x07;
    flags & INVALID_FLAGS_MASK == 0
}

/// `makeI32InitExpr` e `makeI64InitExpr` (a diferença é só a constante aceita).
fn make_init_expr(opcode: u8, is_extended: bool, bits: u64, const_opcode: OpType) -> I32InitExpr {
    assert!(opcode == const_opcode.value() || opcode == OpType::GetGlobal.value());
    if is_extended {
        return I32InitExpr::ExtendedExpression(bits);
    }
    if opcode == const_opcode.value() {
        return I32InitExpr::Const(bits);
    }
    I32InitExpr::Global(bits)
}

/// `isSubtype` de um campo na checagem de subtipo: igual na mutabilidade, e o tipo igual se
/// mutável ou subtipo se imutável (o mesmo para campo de struct e elemento de array).
fn field_is_subtype(info: &ModuleInformation, sub: FieldType, expanded: FieldType) -> bool {
    if sub.mutability != expanded.mutability {
        return false;
    }
    match sub.mutability {
        Mutability::Mutable => sub.ty == expanded.ty,
        Mutability::Immutable => info.is_subtype_storage(sub.ty, expanded.ty),
    }
}

/// `checkStructuralSubtype`: a relação de subtipagem estrutural da proposta GC
/// (https://github.com/WebAssembly/gc/blob/main/proposals/gc/MVP.md#structural-types).
fn check_structural_subtype(info: &ModuleInformation, sub: &StructuralType, expanded: &StructuralType) -> bool {
    match (sub, expanded) {
        (
            StructuralType::Function { arguments: sub_arguments, returns: sub_returns },
            StructuralType::Function { arguments: super_arguments, returns: super_returns },
        ) => {
            if sub_arguments.len() == super_arguments.len() && sub_returns.len() == super_returns.len() {
                // Contravariante nos argumentos, covariante nos retornos.
                for (sub_argument, super_argument) in sub_arguments.iter().zip(super_arguments) {
                    if !info.is_subtype(*super_argument, *sub_argument) {
                        return false;
                    }
                }
                for (sub_return, super_return) in sub_returns.iter().zip(super_returns) {
                    if !info.is_subtype(*sub_return, *super_return) {
                        return false;
                    }
                }
                return true;
            }
        }
        (StructuralType::Struct { fields: sub_fields }, StructuralType::Struct { fields: super_fields }) => {
            if sub_fields.len() >= super_fields.len() {
                return sub_fields.iter().zip(super_fields).all(|(sub, expanded)| field_is_subtype(info, *sub, *expanded));
            }
        }
        (StructuralType::Array { element: sub_element }, StructuralType::Array { element: super_element }) => {
            return field_is_subtype(info, *sub_element, *super_element);
        }
        _ => {}
    }
    false
}

/// `SectionParser`.
pub struct SectionParser<'s, 'i> {
    parser: ParserBase<'s>,
    offset_in_source: usize,
    info: &'i mut ModuleInformation,
}

impl<'s, 'i> SectionParser<'s, 'i> {
    pub fn new(data: &'s [u8], offset_in_source: usize, info: &'i mut ModuleInformation, use_wasm_simd: bool) -> Self {
        SectionParser { parser: ParserBase::new(data, use_wasm_simd), offset_in_source, info }
    }

    /// `source()` de `ParserBase`.
    pub fn source(&self) -> &'s [u8] {
        self.parser.source()
    }

    /// `offset()` de `ParserBase`.
    pub fn offset(&self) -> usize {
        self.parser.offset()
    }

    /// `SectionParser::fail`: o offset é relativo ao módulo inteiro.
    fn fail(&self, message: String) -> String {
        format!(
            "WebAssembly.Module doesn't parse at byte {}: {}",
            self.parser.offset() + self.offset_in_source,
            message
        )
    }

    /// `consumeUTF8String` com o texto já como `String` (o `Name` do C++ é UTF-8 válido).
    fn consume_name(&mut self, length: usize) -> Option<String> {
        self.parser.consume_utf8_string(length).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    /// O despacho de `StreamingParser::parseSectionPayload` (`FOR_EACH_KNOWN_WASM_SECTION`).
    pub fn parse_section(&mut self, section: Section) -> PartialResult {
        match section {
            Section::Type => self.parse_type(),
            Section::Import => self.parse_import(),
            Section::Function => self.parse_function(),
            Section::Table => self.parse_table(),
            Section::Memory => self.parse_memory(),
            Section::Global => self.parse_global(),
            Section::Export => self.parse_export(),
            Section::Start => self.parse_start(),
            Section::Element => self.parse_element(),
            Section::Code => unreachable!("a seção Code é enquadrada pelo StreamingParser"),
            Section::Data => self.parse_data(),
            Section::DataCount => self.parse_data_count(),
            Section::Exception => self.parse_exception(),
            Section::Custom => self.parse_custom(),
            Section::Begin => unreachable!("Begin não é uma seção real"),
        }
    }

    /// `parseType`.
    pub fn parse_type(&mut self) -> PartialResult {
        let count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Type section's count".to_string()))?;
        if count as usize > MAX_TYPES {
            return Err(self.fail(format!("Type section's count is too big {} maximum {}", count, MAX_TYPES)));
        }
        self.info.types.try_reserve_exact(count as usize).map_err(|_| {
            self.fail(format!("can't allocate enough memory for Type section's {} canonical RTT entries", count))
        })?;

        let result = self.parse_type_entries(count);
        // `clearState`: fora da seção de tipos não há grupo recursivo em andamento.
        self.parser.recursion_group_information = RecursionGroupInformation::default();
        result
    }

    fn parse_type_entries(&mut self, count: u32) -> PartialResult {
        let mut recursion_group_count = 0usize;
        for i in 0..count {
            let type_kind = self
                .parser
                .parse_int7()
                .ok_or_else(|| self.fail(format!("can't get {}th Type's type", i)))?;
            let type_count = self.info.type_count() as u32;
            // Com GC, uma referência recursiva pode aparecer em qualquer um dos casos abaixo.
            self.parser.recursion_group_information =
                RecursionGroupInformation { in_recursion_group: true, start: type_count, end: type_count + 1 };

            let signature = match DefinedTypeKind::from_i8(type_kind) {
                Some(DefinedTypeKind::Func) => self.parse_function_type(i)?,
                Some(DefinedTypeKind::Struct) => self.parse_struct_type(i)?,
                Some(DefinedTypeKind::Array) => self.parse_array_type(i)?,
                Some(DefinedTypeKind::Rec) => {
                    self.parse_recursion_group(i)?;
                    recursion_group_count += 1;
                    if recursion_group_count > MAX_NUMBER_OF_RECURSION_GROUPS {
                        return Err(self.fail(format!(
                            "number of recursion groups exceeded the limit of {}",
                            MAX_NUMBER_OF_RECURSION_GROUPS
                        )));
                    }
                    // O parse do grupo já acrescentou os tipos.
                    continue;
                }
                Some(kind @ (DefinedTypeKind::Sub | DefinedTypeKind::Subfinal)) => {
                    self.parse_subtype(i, 0, kind == DefinedTypeKind::Subfinal)?
                }
                None => {
                    return Err(self.fail(format!("{}th Type is non-Func, non-Struct, and non-Array {}", i, type_kind)));
                }
            };

            // Um tipo avulso é a forma abreviada de um grupo recursivo de um tipo só. A checagem de
            // subtipagem vem depois de canonicalizar (e a profundidade antes dela).
            let is_subtype = signature.subtype.is_some();
            self.info.append_recursion_group(vec![(signature.structural, signature.subtype)]);
            if is_subtype {
                if self.info.display_size_excluding_this(type_count as usize) as usize > MAX_SUBTYPE_DEPTH {
                    return Err(self.fail(format!(
                        "subtype depth for Type section's {}th signature exceeded the limits of {}",
                        i, MAX_SUBTYPE_DEPTH
                    )));
                }
                self.check_subtype_validity(type_count as usize)?;
            }
        }
        Ok(())
    }

    /// `parseImport`.
    pub fn parse_import(&mut self) -> PartialResult {
        let import_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Import section's count".to_string()))?;
        if import_count as usize > MAX_IMPORTS {
            return Err(self.fail(format!("Import section's count is too big {} maximum {}", import_count, MAX_IMPORTS)));
        }
        // FIXME do C++: isto reserva a mais, porque nem todo import é global, função ou exceção.
        let reserved = import_count as usize;
        self.info
            .globals
            .try_reserve_exact(reserved)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} globals", import_count)))?;
        self.info
            .imports
            .try_reserve_exact(reserved)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} imports", import_count)))?;
        self.info
            .import_function_type_signature_indices
            .try_reserve_exact(reserved)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} import function signatures", import_count)))?;
        self.info
            .import_exception_type_signature_indices
            .try_reserve_exact(reserved)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} import exception signatures", import_count)))?;

        for import_number in 0..import_count {
            let module_len = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail(format!("can't get {}th Import's module name length", import_number)))?;
            let module_string = self.consume_name(module_len as usize).ok_or_else(|| {
                self.fail(format!("can't get {}th Import's module name of length {}", import_number, module_len))
            })?;

            let field_len = self.parser.parse_var_uint32().ok_or_else(|| {
                self.fail(format!("can't get {}th Import's field name length in module '{}'", import_number, module_string))
            })?;
            // O texto do C++ imprime `moduleLen` aqui, não `fieldLen`.
            let field_string = self.consume_name(field_len as usize).ok_or_else(|| {
                self.fail(format!(
                    "can't get {}th Import's field name of length {} in module '{}'",
                    import_number, module_len, module_string
                ))
            })?;

            let kind = self.parser.parse_external_kind().ok_or_else(|| {
                self.fail(format!(
                    "can't get {}th Import's kind in module '{}' field '{}'",
                    import_number, module_string, field_string
                ))
            })?;
            let kind_index: usize;
            match kind {
                ExternalKind::Function => {
                    let function_type_index = self.parser.parse_var_uint32().ok_or_else(|| {
                        self.fail(format!(
                            "can't get {}th Import's function signature in module '{}' field '{}'",
                            import_number, module_string, field_string
                        ))
                    })?;
                    if function_type_index as usize >= self.info.type_count() {
                        return Err(self.fail(format!(
                            "invalid function signature for {}th Import, {} is out of range of {} in module '{}' field '{}'",
                            import_number,
                            function_type_index,
                            self.info.type_count(),
                            module_string,
                            field_string
                        )));
                    }
                    kind_index = self.info.import_function_type_signature_indices.len();
                    self.validate_function_type(import_number, function_type_index)?;
                    self.info.import_function_type_signature_indices.push(function_type_index);
                }
                ExternalKind::Table => {
                    kind_index = self.info.tables.len();
                    self.parse_table_helper(true)?;
                }
                ExternalKind::Memory => {
                    kind_index = self.info.memories.len();
                    self.parse_memory_helper(true)?;
                }
                ExternalKind::Global => {
                    let mut global = self.parse_global_type()?;
                    // Only mutable globals need floating bindings.
                    if global.mutability == Mutability::Mutable {
                        global.binding_mode = GlobalBindingMode::Portable;
                    }
                    kind_index = self.info.globals.len();
                    self.info.globals.push(global);
                }
                ExternalKind::Exception => {
                    let tag_type = self
                        .parser
                        .parse_uint8()
                        .ok_or_else(|| self.fail(format!("can't get {}th Import exception's tag type", import_number)))?;
                    if tag_type != 0 {
                        return Err(self.fail(format!(
                            "{}th Import exception has tag type {} but the only supported tag type is 0",
                            import_number, tag_type
                        )));
                    }

                    let exception_signature_index = self.parser.parse_var_uint32().ok_or_else(|| {
                        self.fail(format!(
                            "can't get {}th Import's exception signature in module '{}' field '{}'",
                            import_number, module_string, field_string
                        ))
                    })?;
                    if exception_signature_index as usize >= self.info.type_count() {
                        return Err(self.fail(format!(
                            "invalid exception signature for {}th Import, {} is out of range of {} in module '{}' field '{}'",
                            import_number,
                            exception_signature_index,
                            self.info.type_count(),
                            module_string,
                            field_string
                        )));
                    }
                    kind_index = self.info.import_exception_type_signature_indices.len();
                    self.validate_exception_type(import_number, exception_signature_index)?;
                    self.info.import_exception_type_signature_indices.push(exception_signature_index);
                }
            }

            self.info.imports.push(Import {
                module: module_string,
                field: field_string,
                kind,
                kind_index: kind_index as u32,
            });
        }

        self.info.first_internal_global = self.info.globals.len();
        Ok(())
    }

    /// O trecho de `parseImport` e `parseFunction` que confere que o tipo é de função.
    fn validate_function_type(&self, number: u32, type_index: u32) -> PartialResult {
        if self.info.rtt(type_index as usize).structural.kind() != RttKind::Function {
            return Err(self.fail(format!("{}th Function type {} doesn't have a function signature", number, type_index)));
        }
        Ok(())
    }

    /// O trecho de `parseImport` e `parseException` que confere que o tipo da exceção é de função
    /// sem retorno.
    fn validate_exception_type(&self, number: u32, type_index: u32) -> PartialResult {
        match &self.info.rtt(type_index as usize).structural {
            StructuralType::Function { returns, .. } => {
                if !returns.is_empty() {
                    return Err(self.fail(format!(
                        "{}th Exception type cannot have a non-void return type {}",
                        number, type_index
                    )));
                }
                Ok(())
            }
            _ => Err(self.fail(format!("{}th Exception type {} doesn't have a function signature", number, type_index))),
        }
    }

    /// `parseFunction`.
    pub fn parse_function(&mut self) -> PartialResult {
        let count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Function section's count".to_string()))?;
        if count as usize > MAX_FUNCTIONS {
            return Err(self.fail(format!("Function section's count is too big {} maximum {}", count, MAX_FUNCTIONS)));
        }
        self.info
            .internal_function_type_signature_indices
            .try_reserve_exact(count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} Function signatures", count)))?;
        // O texto do C++ não tem espaço entre o número e `Function`.
        self.info
            .functions
            .try_reserve_exact(count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {}Function locations", count)))?;

        for i in 0..count {
            let type_number = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail(format!("can't get {}th Function's type number", i)))?;
            if type_number as usize >= self.info.type_count() {
                return Err(self.fail(format!("{}th Function type number is invalid {}", i, type_number)));
            }
            self.validate_function_type(i, type_number)?;
            // The Code section fixes up start and end.
            self.info.internal_function_type_signature_indices.push(type_number);
            self.info.functions.push(Default::default());
        }

        Ok(())
    }

    /// `parseResizableLimits`.
    fn parse_resizable_limits(&mut self, limits_type: LimitsType) -> Result<ResizableLimits, String> {
        const HAS_MAX_MASK: u8 = 1;
        const IS_SHARED_MASK: u8 = 1 << 1;
        const IS_64_BIT_MASK: u8 = 1 << 2;

        let flags = self
            .parser
            .parse_uint8()
            .ok_or_else(|| self.fail("can't parse resizable limits flags".to_string()))?;
        if !limits_flag_is_valid(flags) {
            return Err(self.fail(format!("resizable limits flag are not valid {:02x}", flags)));
        }
        if limits_type != LimitsType::Memory && flags & IS_SHARED_MASK != 0 {
            return Err(self.fail("can't use shared limits for non memory".to_string()));
        }

        let is_shared = flags & IS_SHARED_MASK != 0;
        let is_64_bit = flags & IS_64_BIT_MASK != 0;
        if is_shared && !Options::with(|options| options.use_wasm_fault_signal_handler) {
            return Err(self.fail("shared memory is not enabled".to_string()));
        }
        if is_64_bit && !Options::with(|options| options.use_wasm_memory64) {
            return Err(self.fail("Memory64 is not enabled".to_string()));
        }

        let initial = if is_64_bit {
            self.parser
                .parse_var_uint64()
                .ok_or_else(|| self.fail("can't parse resizable limits initial page count".to_string()))?
        } else {
            u64::from(
                self.parser
                    .parse_var_uint32()
                    .ok_or_else(|| self.fail("can't parse resizable limits initial page count".to_string()))?,
            )
        };

        let mut maximum = None;
        if flags & HAS_MAX_MASK != 0 {
            let maximum_int = if is_64_bit {
                self.parser
                    .parse_var_uint64()
                    .ok_or_else(|| self.fail("can't parse resizable limits maximum page count".to_string()))?
            } else {
                u64::from(
                    self.parser
                        .parse_var_uint32()
                        .ok_or_else(|| self.fail("can't parse resizable limits maximum page count".to_string()))?,
                )
            };
            if initial > maximum_int {
                return Err(self.fail(format!(
                    "resizable limits has an initial page count of {} which is greater than its maximum {}",
                    initial, maximum_int
                )));
            }
            maximum = Some(maximum_int);
        }

        if is_shared && maximum.is_none() {
            return Err(self.fail("shared memory must have a maximum size".to_string()));
        }
        Ok(ResizableLimits { initial, maximum, is_shared, is_64_bit })
    }

    /// `parseTableHelper`.
    fn parse_table_helper(&mut self, is_import: bool) -> PartialResult {
        if self.info.table_count() >= MAX_TABLES as usize {
            return Err(self.fail(format!(
                "Table count of {} is too big, maximum {}",
                self.info.table_count(),
                MAX_TABLES
            )));
        }

        let mut has_init_expr = false;
        let mut table_init_type = TableInitializationType::Default;
        let mut initial_bits_or_import_number = 0u64;

        let first_byte = self
            .parser
            .peek_int7()
            .ok_or_else(|| self.fail("can't parse Table information".to_string()))?;
        if !is_import && first_byte == TypeKind::Void as i8 {
            has_init_expr = true;
            self.parser.offset += 1;
            let reserved_byte = self.parser.parse_uint8();
            if reserved_byte != Some(0) {
                return Err(self.fail("can't parse explicitly initialized Table's reserved byte".to_string()));
            }
        }

        let ty = self
            .parser
            .parse_value_type(self.info)
            .ok_or_else(|| self.fail("can't parse Table type".to_string()))?;
        if !is_ref_type(ty) {
            return Err(self.fail(format!("Table type should be a ref type, got {}", self.info.type_to_string(ty))));
        }
        if !has_init_expr && !is_import && !is_defaultable_type(ty) {
            return Err(self.fail("Table's type must be defaultable".to_string()));
        }

        let limits = self.parse_resizable_limits(LimitsType::Table)?;
        debug_assert!(!limits.is_shared);
        debug_assert!(!limits.maximum.is_some_and(|maximum| maximum < limits.initial));

        if has_init_expr {
            let init = self.parse_init_expr(ty)?;
            if !self.info.is_subtype(init.result_type, ty) {
                return Err(self.fail(format!(
                    "Table init_expr opcode of type {} doesn't match table's type {}",
                    init.result_type.kind.name(),
                    ty.kind.name()
                )));
            }

            table_init_type = if init.is_extended {
                TableInitializationType::FromExtendedExpression
            } else if init.opcode == OpType::GetGlobal.value() {
                TableInitializationType::FromGlobalImport
            } else if init.opcode == OpType::RefFunc.value() {
                TableInitializationType::FromRefFunc
            } else if init.opcode == OpType::RefNull.value() {
                TableInitializationType::FromRefNull
            } else {
                unreachable!("init_expr de tabela com o opcode {}", init.opcode)
            };
            initial_bits_or_import_number = init.bits_or_import_number;
        }

        let table_type = if self.info.is_subtype(ty, funcref_type()) {
            TableElementType::Funcref
        } else {
            TableElementType::Externref
        };
        self.info.tables.push(TableInformation {
            initial: limits.initial,
            maximum: limits.maximum,
            is_import,
            element_type: table_type,
            wasm_type: ty,
            init_type: table_init_type,
            initial_bits_or_import_number,
            address_type: AddressType::new(limits.is_64_bit),
        });

        Ok(())
    }

    /// `parseTable`.
    pub fn parse_table(&mut self) -> PartialResult {
        let count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Table's count".to_string()))?;

        for _ in 0..count {
            self.parse_table_helper(false)?;
        }

        Ok(())
    }

    /// `parseMemoryHelper`.
    fn parse_memory_helper(&mut self, is_import: bool) -> PartialResult {
        // This test is here in order to handle the case of a single imported memory and a single
        // specified memory in its own section, which is legal iff multimemory is enabled.
        if !Options::with(|options| options.use_wasm_multi_memory) && self.info.memory_count() != 0 {
            return Err(self.fail("there can at most be one Memory section for now".to_string()));
        }

        if self.info.memory_count() >= MAX_MEMORIES {
            return Err(self.fail(format!("there can be at most {} memories", MAX_MEMORIES)));
        }

        let limits = self.parse_resizable_limits(LimitsType::Memory)?;
        debug_assert!(!limits.maximum.is_some_and(|maximum| maximum < limits.initial));

        let max_declarable_page_count = max_declarable_pages(AddressType::new(limits.is_64_bit));
        if limits.initial > max_declarable_page_count {
            return Err(self.fail(format!("Memory's initial page count of {} is invalid", limits.initial)));
        }

        let initial_page_count = PageCount::new(limits.initial);
        let mut maximum_page_count = PageCount::default();
        if let Some(maximum) = limits.maximum {
            if maximum > max_declarable_page_count {
                return Err(self.fail(format!("Memory's maximum page count of {} is invalid", maximum)));
            }
            maximum_page_count = PageCount::new(maximum);
        }

        self.info.memories.push(MemoryInformation::new(
            initial_page_count,
            maximum_page_count,
            limits.is_shared,
            is_import,
            limits.is_64_bit,
        ));

        Ok(())
    }

    /// `parseMemory`.
    pub fn parse_memory(&mut self) -> PartialResult {
        let count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't parse Memory section's count".to_string()))?;

        if count == 0 {
            return Ok(());
        }

        if !Options::with(|options| options.use_wasm_multi_memory) && count != 1 {
            return Err(self.fail(
                "Memory section has more than one memory, WebAssembly currently only allows zero or one".to_string(),
            ));
        }

        for _ in 0..count {
            self.parse_memory_helper(false)?;
        }

        Ok(())
    }

    /// `parseGlobalType`. O `GlobalInformation` volta com os campos de inicialização nos padrões do
    /// C++ (`IsImport`, `EmbeddedInInstance`).
    fn parse_global_type(&mut self) -> Result<GlobalInformation, String> {
        let ty = self
            .parser
            .parse_value_type(self.info)
            .ok_or_else(|| self.fail("can't get Global's value type".to_string()))?;
        let mutability = self.parse_mutability("can't get Global type's mutability".to_string(), "Global's")?;
        Ok(GlobalInformation::new(ty, mutability))
    }

    /// `parseGlobal`.
    pub fn parse_global(&mut self) -> PartialResult {
        let global_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Global section's count".to_string()))?;
        if global_count as usize > MAX_GLOBALS {
            return Err(self.fail(format!("Global section's count is too big {} maximum {}", global_count, MAX_GLOBALS)));
        }
        let total_globals = global_count as usize + self.info.first_internal_global;
        debug_assert_eq!(self.info.first_internal_global, self.info.globals.len());
        self.info
            .globals
            .try_reserve(global_count as usize)
            .map_err(|_| self.fail(format!("can't allocate memory for {} globals", total_globals)))?;

        for _ in 0..global_count {
            let mut global = self.parse_global_type()?;
            let init = self.parse_init_expr(global.ty)?;
            if init.opcode == OpType::ExtSIMD.value() && !init.is_extended {
                assert!(init.result_type.is_v128());
                global.initial_bits = GlobalInitialBits::Vector(init.vector_bits);
            } else {
                global.initial_bits = GlobalInitialBits::BitsOrImportNumber(init.bits_or_import_number);
            }

            global.initialization_type = if init.is_extended {
                GlobalInitializationType::FromExtendedExpression
            } else if init.opcode == OpType::GetGlobal.value() {
                GlobalInitializationType::FromGlobalImport
            } else if init.opcode == OpType::RefFunc.value() {
                GlobalInitializationType::FromRefFunc
            } else {
                GlobalInitializationType::FromExpression
            };
            if !self.info.is_subtype(init.result_type, global.ty) {
                return Err(self.fail(format!(
                    "Global init_expr opcode of type {} doesn't match global's type {}",
                    init.result_type.kind.name(),
                    global.ty.kind.name()
                )));
            }

            if init.opcode == OpType::RefFunc.value() {
                debug_assert!(global.initialization_type != GlobalInitializationType::FromVector);
                self.info.add_declared_function(global.initial_bits.bits_or_import_number() as u32 as usize);
            }

            self.info.globals.push(global);
        }

        Ok(())
    }

    /// `parseExport`.
    pub fn parse_export(&mut self) -> PartialResult {
        let export_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Export section's count".to_string()))?;
        if export_count as usize > MAX_EXPORTS {
            return Err(self.fail(format!("Export section's count is too big {} maximum {}", export_count, MAX_EXPORTS)));
        }
        self.info
            .exports
            .try_reserve_exact(export_count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} exports", export_count)))?;

        let mut export_names: HashSet<String> = HashSet::new();
        for export_number in 0..export_count {
            let field_len = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail(format!("can't get {}th Export's field name length", export_number)))?;
            let field_string = self.consume_name(field_len as usize).ok_or_else(|| {
                self.fail(format!("can't get {}th Export's field name of length {}", export_number, field_len))
            })?;
            if export_names.contains(&field_string) {
                return Err(self.fail(format!("duplicate export: '{}'", field_string)));
            }
            export_names.insert(field_string.clone());

            let kind = self.parser.parse_external_kind().ok_or_else(|| {
                self.fail(format!("can't get {}th Export's kind, named '{}'", export_number, field_string))
            })?;
            let kind_index = self.parser.parse_var_uint32().ok_or_else(|| {
                self.fail(format!("can't get {}th Export's kind index, named '{}'", export_number, field_string))
            })?;
            match kind {
                ExternalKind::Function => {
                    if kind_index as usize >= self.info.function_index_space_size() {
                        return Err(self.fail(format!(
                            "{}th Export has invalid function number {} it exceeds the function index space {}, named '{}'",
                            export_number,
                            kind_index,
                            self.info.function_index_space_size(),
                            field_string
                        )));
                    }
                    self.info.add_declared_function(kind_index as usize);
                }
                ExternalKind::Table => {
                    if kind_index as usize >= self.info.table_count() {
                        return Err(self.fail(format!(
                            "can't export Table {} there are {} Tables",
                            kind_index,
                            self.info.table_count()
                        )));
                    }
                }
                ExternalKind::Memory => {
                    if kind_index as usize >= self.info.memory_count() {
                        return Err(self.fail(format!(
                            "can't export Memory {} there are {} Memories",
                            kind_index,
                            self.info.memory_count()
                        )));
                    }
                }
                ExternalKind::Global => {
                    if kind_index as usize >= self.info.globals.len() {
                        return Err(self.fail(format!(
                            "{}th Export has invalid global number {} it exceeds the globals count {}, named '{}'",
                            export_number,
                            kind_index,
                            self.info.globals.len(),
                            field_string
                        )));
                    }
                    // Only mutable globals need floating bindings.
                    let global = &mut self.info.globals[kind_index as usize];
                    if global.mutability == Mutability::Mutable {
                        global.binding_mode = GlobalBindingMode::Portable;
                    }
                }
                ExternalKind::Exception => {
                    if kind_index as usize >= self.info.exception_index_space_size() {
                        return Err(self.fail(format!(
                            "{}th Export has invalid exception number {} it exceeds the exception index space {}, named '{}'",
                            export_number,
                            kind_index,
                            self.info.exception_index_space_size(),
                            field_string
                        )));
                    }
                    self.info.add_declared_exception(kind_index as usize);
                }
            }

            self.info.exports.push(Export { field: field_string, kind, kind_index });
        }

        Ok(())
    }

    /// `parseStart`.
    pub fn parse_start(&mut self) -> PartialResult {
        let start_function_index = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Start index".to_string()))?;
        if start_function_index as usize >= self.info.function_index_space_size() {
            return Err(self.fail(format!(
                "Start index {} exceeds function index space {}",
                start_function_index,
                self.info.function_index_space_size()
            )));
        }
        match &self.info.rtt_from_function_index_space(start_function_index as usize).structural {
            StructuralType::Function { arguments, returns } => {
                if !arguments.is_empty() {
                    return Err(self.fail("Start function can't have arguments".to_string()));
                }
                if !returns.is_empty() {
                    return Err(self.fail("Start function can't return a value".to_string()));
                }
            }
            _ => {
                return Err(self.fail(format!("Start function index {} is not a function", start_function_index)));
            }
        }
        self.info.start_function_index_space = Some(start_function_index);
        Ok(())
    }

    /// `parseElement`.
    pub fn parse_element(&mut self) -> PartialResult {
        let element_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Element section's count".to_string()))?;
        if element_count as usize > MAX_TABLE_ENTRIES {
            return Err(self.fail(format!(
                "Element section's count is too big {} maximum {}",
                element_count, MAX_TABLE_ENTRIES
            )));
        }
        self.info
            .elements
            .try_reserve_exact(element_count as usize)
            .map_err(|_| self.fail(format!("can't allocate memory for {} Elements", element_count)))?;
        for element_num in 0..element_count {
            let element_flags = self.parser.parse_var_uint32().ok_or_else(|| {
                self.fail(format!("can't get {}th Element reserved byte, which should be element flags", element_num))
            })?;

            // A forma do segmento: o tipo dos elementos, a tabela e o deslocamento (só nos ativos), e
            // se os elementos são expressões (`true`) ou índices de função (`false`).
            let (kind, element_type, table_index, offset, uses_expressions) = match element_flags {
                0x00 => {
                    const TABLE_INDEX: u32 = 0;
                    self.validate_element_table_idx(TABLE_INDEX, non_null_funcref_type())?;
                    let offset = self.parse_element_offset(TABLE_INDEX)?;
                    (ElementKind::Active, non_null_funcref_type(), Some(TABLE_INDEX), Some(offset), false)
                }
                0x01 => {
                    self.parse_element_kind()?;
                    (ElementKind::Passive, non_null_funcref_type(), None, None, false)
                }
                0x02 => {
                    let table_index = self.parse_element_table_index(element_num)?;
                    self.validate_element_table_idx(table_index, non_null_funcref_type())?;
                    let offset = self.parse_element_offset(table_index)?;
                    self.parse_element_kind()?;
                    (ElementKind::Active, non_null_funcref_type(), Some(table_index), Some(offset), false)
                }
                0x03 => {
                    self.parse_element_kind()?;
                    (ElementKind::Declared, non_null_funcref_type(), None, None, false)
                }
                0x04 => {
                    const TABLE_INDEX: u32 = 0;
                    self.validate_element_table_idx(TABLE_INDEX, funcref_type())?;
                    let offset = self.parse_element_offset(TABLE_INDEX)?;
                    (ElementKind::Active, funcref_type(), Some(TABLE_INDEX), Some(offset), true)
                }
                0x05 => {
                    let ref_type = self.parse_element_ref_type()?;
                    (ElementKind::Passive, ref_type, None, None, true)
                }
                0x06 => {
                    let table_index = self.parse_element_table_index(element_num)?;
                    if table_index as usize >= self.info.table_count() {
                        return Err(self.fail(format!(
                            "Element section for Table {} exceeds available Table {}",
                            table_index,
                            self.info.table_count()
                        )));
                    }
                    let offset = self.parse_element_offset(table_index)?;
                    let ref_type = self.parse_element_ref_type()?;
                    self.validate_element_table_idx(table_index, ref_type)?;
                    (ElementKind::Active, ref_type, Some(table_index), Some(offset), true)
                }
                0x07 => {
                    let ref_type = self.parse_element_ref_type()?;
                    (ElementKind::Declared, ref_type, None, None, true)
                }
                _ => {
                    return Err(self.fail(format!("can't get {}th Element reserved byte", element_num)));
                }
            };

            let index_count = self.parse_index_count_for_element_section(element_num)?;
            if let Some(table_index) = table_index {
                debug_assert!((table_index as usize) < self.info.tables.len());
            }

            let mut element = Element::new(kind, element_type, table_index, offset);
            element
                .init_types
                .try_reserve_exact(index_count)
                .map_err(|_| self.fail(format!("can't allocate memory for {} Element init_exprs", index_count)))?;
            element
                .initial_bits_or_indices
                .try_reserve_exact(index_count)
                .map_err(|_| self.fail(format!("can't allocate memory for {} Element init_exprs", index_count)))?;

            if uses_expressions {
                self.parse_element_segment_vector_of_expressions(&mut element, index_count, element_num)?;
            } else {
                self.parse_element_segment_vector_of_indexes(&mut element, index_count, element_num)?;
            }
            self.info.elements.push(element);
        }

        Ok(())
    }

    /// O `parseVarUInt32` do índice de tabela dos segmentos de elementos com tabela explícita.
    fn parse_element_table_index(&mut self, element_num: u32) -> Result<u32, String> {
        self.parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th Element table index", element_num)))
    }

    /// O `parseRefType` dos segmentos de elementos com tipo explícito.
    fn parse_element_ref_type(&mut self) -> Result<Type, String> {
        self.parser
            .parse_ref_type(self.info)
            .ok_or_else(|| self.fail("can't parse reftype in elem section".to_string()))
    }

    // `parseCode` não existe aqui: o `StreamingParser` enquadra a seção `Code`, e no C++ ele é
    // `RELEASE_ASSERT_NOT_REACHED`.

    /// `parseInitExpr`. Só a forma de uma constante isolada seguida de `end` é validada aqui; o resto
    /// é a expressão constante estendida (`parse_extended_const_expr`).
    fn parse_init_expr(&mut self, expected_type: Type) -> Result<InitExpr, String> {
        let initial_offset = self.parser.offset();
        let opcode = self
            .parser
            .parse_uint8()
            .ok_or_else(|| self.fail("can't get init_expr's opcode".to_string()))?;

        let mut bits_or_import_number = 0u64;
        let mut vector_bits: V128 = [0; 16];
        // `resultType` só é lido depois de cada ramo o escrever, ou de o ramo estendido o trocar
        // por `expectedType`.
        let mut result_type = expected_type;

        match OpType::from_value(opcode) {
            Some(OpType::I32Const) => {
                let constant = self
                    .parser
                    .parse_var_int32()
                    .ok_or_else(|| self.fail("can't get constant value for init_expr's i32.const".to_string()))?;
                bits_or_import_number = constant as i64 as u64;
                result_type = TYPE_I32;
            }
            Some(OpType::I64Const) => {
                let constant = self
                    .parser
                    .parse_var_int64()
                    .ok_or_else(|| self.fail("can't get constant value for init_expr's i64.const".to_string()))?;
                bits_or_import_number = constant as u64;
                result_type = TYPE_I64;
            }
            Some(OpType::F32Const) => {
                let constant = self
                    .parser
                    .parse_uint32()
                    .ok_or_else(|| self.fail("can't get constant value for init_expr's f32.const".to_string()))?;
                bits_or_import_number = u64::from(constant);
                result_type = Type::new(TypeKind::F32, TypeIndex::Invalid);
            }
            Some(OpType::F64Const) => {
                let constant = self
                    .parser
                    .parse_uint64()
                    .ok_or_else(|| self.fail("can't get constant value for init_expr's f64.const".to_string()))?;
                bits_or_import_number = constant;
                result_type = Type::new(TypeKind::F64, TypeIndex::Invalid);
            }
            // O C++ só tem este ramo com `ENABLE(B3_JIT)`, que o build de referência liga.
            Some(OpType::ExtSIMD) => {
                if !self.parser.use_wasm_simd {
                    return Err(self.fail("SIMD must be enabled".to_string()));
                }
                let simd_opcode = self
                    .parser
                    .parse_uint8()
                    .ok_or_else(|| self.fail("can't get init_expr's simd opcode".to_string()))?;
                if u32::from(simd_opcode) != EXT_SIMD_V128_CONST {
                    // O texto do C++ imprime `opcode` (o prefixo), não `simdOpcode`.
                    return Err(self.fail(format!("unknown init_expr simd opcode {}", opcode)));
                }
                vector_bits = self
                    .parser
                    .parse_imm_byte_array16()
                    .ok_or_else(|| self.fail("get constant value for init_expr's v128.const".to_string()))?;
                result_type = TYPE_V128;
            }
            Some(OpType::GetGlobal) => {
                let index = self
                    .parser
                    .parse_var_uint32()
                    .ok_or_else(|| self.fail("can't get get_global's index".to_string()))?;
                if index as usize >= self.info.globals.len() {
                    return Err(self.fail(format!(
                        "get_global's index {} exceeds the number of globals {}",
                        index,
                        self.info.globals.len()
                    )));
                }
                if self.info.globals[index as usize].mutability != Mutability::Immutable {
                    return Err(self.fail(format!("get_global import kind index {} is mutable ", index)));
                }
                result_type = self.info.globals[index as usize].ty;
                bits_or_import_number = u64::from(index);
            }
            Some(OpType::RefNull) => {
                let heap_type = self.parser.parse_heap_type(self.info).ok_or_else(|| {
                    self.fail("ref.null heaptype must be funcref, externref or type_idx".to_string())
                })?;
                let type_of_null = if heap_type >= 0 {
                    Type::new(TypeKind::RefNull, self.info.type_index_of(heap_type as usize))
                } else {
                    let heap_kind = TypeKind::from_i8(heap_type as i8).expect("o tipo heap abstrato já foi validado");
                    Type::new(TypeKind::RefNull, type_index_from_type_kind(heap_kind))
                };
                result_type = type_of_null;
                bits_or_import_number = js_null().encode() as u64;
            }
            Some(OpType::RefFunc) => {
                let index = self
                    .parser
                    .parse_var_uint32()
                    .ok_or_else(|| self.fail("can't get ref.func index".to_string()))?;
                if index as usize >= self.info.function_index_space_size() {
                    return Err(self.fail(format!(
                        "ref.func index {} exceeds the number of functions {}",
                        index,
                        self.info.function_index_space_size()
                    )));
                }
                let type_signature_index = self.info.type_signature_index_from_function_index_space(index as usize);
                result_type = Type::new(TypeKind::Ref, self.info.type_index_of(type_signature_index as usize));
                bits_or_import_number = u64::from(index);
            }
            Some(OpType::ExtGC) => {}
            _ => {
                return Err(self.fail(format!("unknown init_expr opcode {}", opcode)));
            }
        }

        // Don't consume the opcode byte unless it's an End so that the extended parsing mode below
        // can consume it if needed.
        let source = self.parser.source();
        if self.parser.offset() >= source.len() {
            return Err(self.fail("can't get init_expr's end opcode".to_string()));
        }
        let end_opcode = source[self.parser.offset()];

        if end_opcode == OpType::End.value() && opcode != OpType::ExtGC.value() {
            self.parser.offset += 1;
            return Ok(InitExpr { opcode, is_extended: false, bits_or_import_number, vector_bits, result_type });
        }

        // If an End doesn't appear, we have to assume it's an extended constant expression and use
        // the full Wasm expression parser to validate.
        let offset_of_expr_in_source = initial_offset + self.offset_in_source;
        let init_expr_length = self.parse_extended_const_expr(initial_offset, expected_type)?;
        self.parser.offset = initial_offset + init_expr_length;
        self.info
            .constant_expressions
            .push((source[initial_offset..initial_offset + init_expr_length].to_vec(), offset_of_expr_in_source));
        Ok(InitExpr {
            opcode,
            is_extended: true,
            bits_or_import_number: (self.info.constant_expressions.len() - 1) as u64,
            vector_bits,
            result_type: expected_type,
        })
    }

    /// `parseExtendedConstExpr` (`WasmConstExprGenerator`): devolve quantos bytes a expressão ocupa a
    /// partir de `initial_offset`. As funções que `ref.func` cita passam a declaradas no módulo.
    fn parse_extended_const_expr(&mut self, initial_offset: usize, expected_type: Type) -> Result<usize, String> {
        let source = self.parser.source();
        let offset_of_expr_in_source = initial_offset + self.offset_in_source;
        parse_extended_const_expr(&source[initial_offset..], offset_of_expr_in_source, self.info, expected_type)
    }

    /// `validateElementTableIdx`.
    fn validate_element_table_idx(&self, table_index: u32, ty: Type) -> PartialResult {
        if table_index as usize >= self.info.table_count() {
            return Err(self.fail(format!(
                "Element section for Table {} exceeds available Table {}",
                table_index,
                self.info.table_count()
            )));
        }
        if !self.info.is_subtype(ty, self.info.tables[table_index as usize].wasm_type) {
            return Err(self.fail(format!(
                "Table {} must have type '{}' to have an element section",
                table_index,
                self.info.type_to_string(ty)
            )));
        }

        Ok(())
    }

    /// `parseI32InitExpr` e `parseI64InitExpr` (e os quatro `...ForElementSection` e
    /// `...ForDataSection`): a expressão de deslocamento de um segmento, do tipo do endereço da
    /// tabela ou da memória. `section` é o prefixo da mensagem (`Element` ou `Data`).
    fn parse_offset_init_expr(&mut self, address_type: AddressType, section: &str) -> Result<I32InitExpr, String> {
        let (expected_type, const_opcode) = if address_type.is_64_bit() {
            (TYPE_I64, OpType::I64Const)
        } else {
            (TYPE_I32, OpType::I32Const)
        };
        let init = self.parse_init_expr(expected_type)?;
        if init.result_type.kind != expected_type.kind {
            return Err(self.fail(format!(
                "{} init_expr must produce an {}",
                section,
                if address_type.is_64_bit() { "i64" } else { "i32" }
            )));
        }
        Ok(make_init_expr(init.opcode, init.is_extended, init.bits_or_import_number, const_opcode))
    }

    /// O `parseI32InitExprForElementSection` ou `parseI64InitExprForElementSection` conforme o
    /// tipo de endereço da tabela.
    fn parse_element_offset(&mut self, table_index: u32) -> Result<I32InitExpr, String> {
        let address_type = self.info.tables[table_index as usize].address_type;
        self.parse_offset_init_expr(address_type, "Element")
    }

    /// `parseElementKind`.
    fn parse_element_kind(&mut self) -> PartialResult {
        let element_kind = self
            .parser
            .parse_uint8()
            .ok_or_else(|| self.fail("can't get element kind".to_string()))?;
        if element_kind != 0 {
            return Err(self.fail("element kind must be zero".to_string()));
        }

        Ok(())
    }

    /// `parseIndexCountForElementSection`.
    fn parse_index_count_for_element_section(&mut self, element_num: u32) -> Result<usize, String> {
        let index_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th index count for Element section", element_num)))?;
        if index_count as usize > MAX_TABLE_ENTRIES {
            return Err(self.fail(format!(
                "Element section's {}th index count of {} is too big, maximum {}",
                element_num, index_count, MAX_TABLE_ENTRIES
            )));
        }

        Ok(index_count as usize)
    }

    /// `parseElementSegmentVectorOfExpressions`.
    fn parse_element_segment_vector_of_expressions(
        &mut self,
        element: &mut Element,
        index_count: usize,
        element_num: u32,
    ) -> PartialResult {
        let element_type = element.element_type;
        for _ in 0..index_count {
            let init = self.parse_init_expr(element_type)?;
            if !self.info.is_subtype(init.result_type, element_type) {
                return Err(self.fail(format!(
                    "Element section's {}th element's init_expr opcode of type {} doesn't match element's type {}",
                    element_num,
                    init.result_type.kind.name(),
                    element_type.kind.name()
                )));
            }

            let init_type = if init.is_extended {
                ElementInitializationType::FromExtendedExpression
            } else if init.opcode == OpType::GetGlobal.value() {
                ElementInitializationType::FromGlobal
            } else if init.opcode == OpType::RefFunc.value() {
                self.info.add_declared_function(init.bits_or_import_number as u32 as usize);
                ElementInitializationType::FromRefFunc
            } else if init.opcode == OpType::RefNull.value() {
                ElementInitializationType::FromRefNull
            } else {
                unreachable!("init_expr de elemento com o opcode {}", init.opcode)
            };

            element.init_types.push(init_type);
            element.initial_bits_or_indices.push(init.bits_or_import_number);
        }

        Ok(())
    }

    /// `parseElementSegmentVectorOfIndexes`.
    fn parse_element_segment_vector_of_indexes(
        &mut self,
        element: &mut Element,
        index_count: usize,
        element_num: u32,
    ) -> PartialResult {
        for index in 0..index_count {
            let function_index = self.parser.parse_var_uint32().ok_or_else(|| {
                self.fail(format!("can't get Element section's {}th element's {}th index", element_num, index))
            })?;
            if function_index as usize >= self.info.function_index_space_size() {
                return Err(self.fail(format!(
                    "Element section's {}th element's {}th index is {} which exceeds the function index space size of {}",
                    element_num,
                    index,
                    function_index,
                    self.info.function_index_space_size()
                )));
            }

            self.info.add_declared_function(function_index as usize);
            element.init_types.push(ElementInitializationType::FromRefFunc);
            element.initial_bits_or_indices.push(u64::from(function_index));
        }

        Ok(())
    }

    /// `parseFunctionType`.
    fn parse_function_type(&mut self, position: u32) -> Result<ParsedDef, String> {
        let argument_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get Type's argument count at index {}", position)))?;
        if argument_count as usize > MAX_FUNCTION_PARAMS {
            return Err(self.fail(format!(
                "argument count of Type at index {} is too big {} maximum {}",
                position, argument_count, MAX_FUNCTION_PARAMS
            )));
        }
        let mut arguments = self.reserve_signature(argument_count, position)?;
        for i in 0..argument_count {
            let argument_type = self
                .parser
                .parse_value_type(self.info)
                .ok_or_else(|| self.fail(format!("can't get {}th argument Type", i)))?;
            arguments.push(argument_type);
        }

        let return_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get Type's return count at index {}", position)))?;
        if return_count as usize > MAX_FUNCTION_RETURNS {
            return Err(self.fail(format!(
                "return count of Type at index {} is too big {} maximum {}",
                position, return_count, MAX_FUNCTION_RETURNS
            )));
        }
        let mut returns = self.reserve_signature(return_count, position)?;
        for i in 0..return_count {
            let value = self
                .parser
                .parse_value_type(self.info)
                .ok_or_else(|| self.fail(format!("can't get {}th Type's return value", i)))?;
            returns.push(value);
        }

        Ok(ParsedDef { structural: StructuralType::Function { arguments, returns }, subtype: None })
    }

    /// O `tryReserveInitialCapacity` dos vetores de argumentos e retornos.
    fn reserve_signature(&self, count: u32, position: u32) -> Result<Vec<Type>, String> {
        let mut types = Vec::new();
        types
            .try_reserve_exact(count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for Type section's {}th signature", position)))?;
        Ok(types)
    }

    /// `parsePackedType`.
    fn parse_packed_type(&mut self) -> Result<PackedType, String> {
        let kind = self
            .parser
            .parse_int7()
            .ok_or_else(|| self.fail("invalid type in struct field or array element".to_string()))?;
        PackedType::from_i8(kind).ok_or_else(|| self.fail(format!("expected a packed type but got {}", kind)))
    }

    /// `parseStorageType`.
    fn parse_storage_type(&mut self) -> Result<StorageType, String> {
        let kind = self
            .parser
            .peek_int7()
            .ok_or_else(|| self.fail("invalid type in struct field or array element".to_string()))?;
        if is_valid_type_kind(kind) {
            let element_type = self
                .parser
                .parse_value_type(self.info)
                .ok_or_else(|| self.fail("invalid type in struct field or array element".to_string()))?;
            return Ok(StorageType::Type(element_type));
        }
        Ok(StorageType::Packed(self.parse_packed_type()?))
    }

    /// A leitura do byte de mutabilidade, a mesma em struct, array e global. `missing_message` é o
    /// texto de quando o byte não existe, e `invalid_label` o prefixo do texto de valor inválido.
    fn parse_mutability(&mut self, missing_message: String, invalid_label: &str) -> Result<Mutability, String> {
        let mutability = self.parser.parse_uint8().ok_or_else(|| self.fail(missing_message))?;
        match mutability {
            0 => Ok(Mutability::Immutable),
            1 => Ok(Mutability::Mutable),
            _ => Err(self.fail(format!("invalid {} mutability: 0x{:02x}", invalid_label, mutability))),
        }
    }

    /// `parseStructType`.
    fn parse_struct_type(&mut self, position: u32) -> Result<ParsedDef, String> {
        let field_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th struct type's field count", position)))?;
        if field_count as usize > MAX_STRUCT_FIELD_COUNT {
            return Err(self.fail(format!(
                "number of fields for struct type at position {} is too big {} maximum {}",
                position, field_count, MAX_STRUCT_FIELD_COUNT
            )));
        }
        let mut fields: Vec<FieldType> = Vec::new();
        fields
            .try_reserve_exact(field_count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for struct fields {} entries", field_count)))?;

        let mut struct_instance_payload_size: u32 = 0;
        for field_index in 0..field_count {
            let field_type = self
                .parse_storage_type()
                .map_err(|_| self.fail(format!("can't get {}th field Type", field_index)))?;
            // O C++ escreve `position` antes do texto, sem separador; a mensagem herda isso.
            let mutability = self.parse_mutability(
                format!("{}can't get {}th field mutability", position, field_index),
                "Field's",
            )?;
            fields.push(FieldType { ty: field_type, mutability });
            struct_instance_payload_size = struct_instance_payload_size
                .checked_add(field_type.size_in_bytes() as u32)
                .ok_or_else(|| self.fail("struct layout is too big".to_string()))?;
        }

        self.info.has_gc_object_types = true;
        Ok(ParsedDef { structural: StructuralType::Struct { fields }, subtype: None })
    }

    /// `parseArrayType`.
    fn parse_array_type(&mut self, position: u32) -> Result<ParsedDef, String> {
        let element_type = self
            .parse_storage_type()
            .map_err(|_| self.fail("can't get array's element Type".to_string()))?;
        let mutability = self.parse_mutability(format!("{}can't get array's mutability", position), "array")?;

        self.info.has_gc_object_types = true;
        Ok(ParsedDef {
            structural: StructuralType::Array { element: FieldType { ty: element_type, mutability } },
            subtype: None,
        })
    }

    /// `parseRecursionGroup`.
    fn parse_recursion_group(&mut self, position: u32) -> PartialResult {
        let type_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th recursion group's type count", position)))?;
        if type_count as usize > MAX_RECURSION_GROUP_COUNT {
            return Err(self.fail(format!(
                "number of types for recursion group at position {} is too big {} maximum {}",
                position, type_count, MAX_RECURSION_GROUP_COUNT
            )));
        }

        // Um `(rec)` vazio é permitido: não há membros a registrar.
        if type_count == 0 {
            return Ok(());
        }

        let mut signatures: Vec<ParsedDef> = Vec::new();
        signatures.try_reserve_exact(type_count as usize).map_err(|_| {
            self.fail(format!("can't allocate enough memory for recursion group {} entries", type_count))
        })?;

        let start = self.info.type_count() as u32;
        let saved = self.parser.recursion_group_information;
        self.parser.recursion_group_information =
            RecursionGroupInformation { in_recursion_group: true, start, end: start + type_count };

        for i in 0..type_count {
            let type_kind = self
                .parser
                .parse_int7()
                .ok_or_else(|| self.fail(format!("can't get recursion group's {}th Type's type", i)))?;
            let signature = match DefinedTypeKind::from_i8(type_kind) {
                Some(DefinedTypeKind::Func) => self.parse_function_type(i)?,
                Some(DefinedTypeKind::Struct) => self.parse_struct_type(i)?,
                Some(DefinedTypeKind::Array) => self.parse_array_type(i)?,
                Some(kind @ (DefinedTypeKind::Sub | DefinedTypeKind::Subfinal)) => {
                    self.parse_subtype(i, signatures.len(), kind == DefinedTypeKind::Subfinal)?
                }
                Some(DefinedTypeKind::Rec) | None => {
                    return Err(self.fail(format!("{}th Type is non-Func, non-Struct, and non-Array {}", i, type_kind)));
                }
            };
            signatures.push(signature);
        }

        let members = signatures.into_iter().map(|signature| (signature.structural, signature.subtype)).collect();
        self.info.append_recursion_group(members);
        // Checking subtyping requirements has to be deferred until the group is canonicalized, in
        // case recursive references show up in the type.
        for member in 0..type_count as usize {
            let position_in_module = start as usize + member;
            if self.info.rtt(position_in_module).subtype.is_some() {
                self.check_subtype_validity(position_in_module)?;
            }
        }
        self.parser.recursion_group_information = saved;
        Ok(())
    }

    /// `checkSubtypeValidity`: o supertipo imediato (o último do display do RTT canônico) não pode
    /// ser final, e a estrutura tem de ser subtipo da dele.
    fn check_subtype_validity(&self, position: usize) -> PartialResult {
        let definition = self.info.rtt(position);
        let Some(subtype) = &definition.subtype else {
            return Ok(());
        };
        if subtype.super_types.is_empty() {
            return Ok(());
        }

        let super_rtt = self
            .info
            .direct_super_rtt(self.info.canonical_type_id(position))
            .expect("um subtipo com supertipo tem display não vazio");
        if super_rtt.is_final_type() {
            return Err(self.fail("cannot declare subtype of final supertype".to_string()));
        }
        if !check_structural_subtype(self.info, &definition.structural, &super_rtt.structural) {
            return Err(self.fail("structural type is not a subtype of the specified supertype".to_string()));
        }

        Ok(())
    }

    /// `parseSubtype`. `group_member_count` é `recursionGroupTypes.size()`: quantos membros do
    /// grupo recursivo já foram lidos (zero fora de um grupo).
    fn parse_subtype(&mut self, position: u32, group_member_count: usize, is_final: bool) -> Result<ParsedDef, String> {
        let supertype_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th subtype's supertype count", position)))?;
        if supertype_count as usize > MAX_SUBTYPE_SUPERTYPE_COUNT {
            return Err(self.fail(format!(
                "number of supertypes for subtype at position {} is too big {} maximum {}",
                position, supertype_count, MAX_SUBTYPE_SUPERTYPE_COUNT
            )));
        }

        // Vale a restrição do MVP: no máximo um supertipo.
        let mut super_types = Vec::new();
        if supertype_count > 0 {
            let type_index = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail("can't get subtype's supertype index".to_string()))?;
            let type_count = self.info.type_count();
            if type_index as usize >= type_count + group_member_count {
                return Err(self.fail("supertype index is a forward reference".to_string()));
            }
            if (type_index as usize) < type_count {
                super_types.push(self.info.type_index_of(type_index as usize));
            } else {
                // Um supertipo no mesmo grupo recursivo vira o placeholder (índice relativo ao grupo).
                debug_assert!(self.parser.recursion_group_information.in_recursion_group);
                super_types.push(TypeIndex::Projection(type_index - type_count as u32));
            }
        }

        let type_kind = self
            .parser
            .parse_int7()
            .ok_or_else(|| self.fail("can't get subtype's underlying Type's type".to_string()))?;
        let underlying = match DefinedTypeKind::from_i8(type_kind) {
            Some(DefinedTypeKind::Func) => self.parse_function_type(position)?,
            Some(DefinedTypeKind::Struct) => self.parse_struct_type(position)?,
            Some(DefinedTypeKind::Array) => self.parse_array_type(position)?,
            _ => return Err(self.fail(format!("invalid structural type definition for subtype {}", type_kind))),
        };

        // Sem supertipo e final, a definição é normalizada para não ter o subtipo: a forma
        // abreviada e a completa ficam representadas do mesmo jeito.
        if supertype_count == 0 && is_final {
            return Ok(underlying);
        }
        Ok(ParsedDef { structural: underlying.structural, subtype: Some(Subtype { super_types, is_final }) })
    }

    /// `parseData`.
    pub fn parse_data(&mut self) -> PartialResult {
        let segment_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Data section's count".to_string()))?;
        if segment_count as usize > MAX_DATA_SEGMENTS {
            return Err(self.fail(format!(
                "Data section's count is too big {} maximum {}",
                segment_count, MAX_DATA_SEGMENTS
            )));
        }
        if let Some(number_of_data_segments) = self.info.number_of_data_segments {
            if segment_count != number_of_data_segments {
                return Err(self.fail(format!(
                    "Data section's count {} is different from Data Count section's count {}",
                    segment_count, number_of_data_segments
                )));
            }
        }
        self.info
            .data
            .try_reserve_exact(segment_count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for Data section's {} segments", segment_count)))?;

        for segment_number in 0..segment_count {
            let memory_index_or_data_flag = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail(format!("can't get {}th Data segment's flag", segment_number)))?;

            let segment = match memory_index_or_data_flag {
                // Ativo, na memória 0.
                0 => {
                    let memory_index = 0;
                    self.validate_data_memory_index(segment_number, memory_index)?;
                    let address_type = self.info.memories[memory_index as usize].address_type;
                    let offset = self.parse_offset_init_expr(address_type, "Data")?;
                    self.parse_data_segment_bytes(segment_number, SegmentKind::Active, Some(offset), memory_index)?
                }
                // Passivo.
                0x01 => self.parse_data_segment_bytes(segment_number, SegmentKind::Passive, None, 0)?,
                // Ativo, com o índice da memória explícito.
                0x02 => {
                    let memory_index = self
                        .parser
                        .parse_var_uint32()
                        .ok_or_else(|| self.fail(format!("can't get {}th Data segment's index", segment_number)))?;
                    self.validate_data_memory_index(segment_number, memory_index)?;
                    let address_type = self.info.memories[memory_index as usize].address_type;
                    let offset = self.parse_offset_init_expr(address_type, "Data")?;
                    self.parse_data_segment_bytes(segment_number, SegmentKind::Active, Some(offset), memory_index)?
                }
                _ => return Err(self.fail(format!("unknown {}th Data segment's flag", segment_number))),
            };
            self.info.data.push(segment);
        }

        Ok(())
    }

    /// O trecho de `parseData` que confere o índice da memória de um segmento ativo.
    fn validate_data_memory_index(&self, segment_number: u32, memory_index: u32) -> PartialResult {
        if memory_index as usize >= self.info.memory_count() {
            return Err(self.fail(format!(
                "{}th Data segment has index {} which exceeds the number of Memories {}",
                segment_number,
                memory_index,
                self.info.memory_count()
            )));
        }

        Ok(())
    }

    /// O trecho de `parseData` que lê o tamanho e os bytes de um segmento (`Segment::tryCreate` e o
    /// `memcpySpan`).
    fn parse_data_segment_bytes(
        &mut self,
        segment_number: u32,
        kind: SegmentKind,
        offset_if_active: Option<I32InitExpr>,
        memory_index: u32,
    ) -> Result<Segment, String> {
        let data_byte_length = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail(format!("can't get {}th Data segment's data byte length", segment_number)))?;
        if data_byte_length as usize > MAX_MODULE_SIZE {
            return Err(self.fail(format!(
                "{}th Data segment's data byte length is too big {} maximum {}",
                segment_number, data_byte_length, MAX_MODULE_SIZE
            )));
        }

        let mut bytes: Vec<u8> = Vec::new();
        bytes.try_reserve_exact(data_byte_length as usize).map_err(|_| {
            self.fail(format!(
                "can't allocate enough memory for {}th Data segment of size {}",
                segment_number, data_byte_length
            ))
        })?;

        let source = self.parser.source();
        let length = data_byte_length as usize;
        if source.len() < length || source.len() - length < self.parser.offset() {
            return Err(self.fail(format!("can't get data bytes from {}th Data segment", segment_number)));
        }

        bytes.extend_from_slice(&source[self.parser.offset()..self.parser.offset() + length]);
        self.parser.offset += length;
        Ok(Segment { kind, offset_if_active, memory_index, bytes })
    }

    /// `parseDataCount`.
    pub fn parse_data_count(&mut self) -> PartialResult {
        let number_of_data_segments = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Data Count section's count".to_string()))?;
        if number_of_data_segments as usize > MAX_DATA_SEGMENTS {
            return Err(self.fail(format!(
                "Data Count section's count is too big {} maximum {}",
                number_of_data_segments, MAX_DATA_SEGMENTS
            )));
        }

        self.info.number_of_data_segments = Some(number_of_data_segments);
        Ok(())
    }

    /// `parseException`.
    pub fn parse_exception(&mut self) -> PartialResult {
        let exception_count = self
            .parser
            .parse_var_uint32()
            .ok_or_else(|| self.fail("can't get Exception section's count".to_string()))?;
        if exception_count as usize > MAX_EXCEPTIONS {
            return Err(self.fail(format!(
                "Exception section's count is too big {} maximum {}",
                exception_count, MAX_EXCEPTIONS
            )));
        }
        self.info
            .internal_exception_type_signature_indices
            .try_reserve_exact(exception_count as usize)
            .map_err(|_| self.fail(format!("can't allocate enough memory for {} exceptions", exception_count)))?;

        for exception_number in 0..exception_count {
            let tag_type = self
                .parser
                .parse_uint8()
                .ok_or_else(|| self.fail(format!("can't get {}th Exception tag type", exception_number)))?;
            if tag_type != 0 {
                return Err(self.fail(format!(
                    "{}th Exception has tag type {} but the only supported tag type is 0",
                    exception_number, tag_type
                )));
            }

            let type_number = self
                .parser
                .parse_var_uint32()
                .ok_or_else(|| self.fail(format!("can't get {}th Exception's type number", exception_number)))?;
            if type_number as usize >= self.info.type_count() {
                return Err(self.fail(format!("{}th Exception type number is invalid {}", exception_number, type_number)));
            }
            self.validate_exception_type(exception_number, type_number)?;
            self.info.internal_exception_type_signature_indices.push(type_number);
        }

        Ok(())
    }

    /// `parseCustom`. O parser da seção `name` entra em fatia própria; o C++ ignora a falha dos
    /// de branch hints e de `sourceMappingURL`, então a seção é guardada do mesmo jeito.
    pub fn parse_custom(&mut self) -> PartialResult {
        let custom_section_number = self.info.custom_sections.len() + 1;
        self.info.custom_sections.try_reserve(1).map_err(|_| {
            self.fail(format!("can't allocate enough memory for {}th custom section", custom_section_number))
        })?;
        let name_length = self.parser.parse_var_uint32().ok_or_else(|| {
            self.fail(format!("can't get {}th custom section's name length", custom_section_number))
        })?;
        // O texto de erro vem do C++ tal qual ("nameLen get ...").
        let name = self.consume_name(name_length as usize).ok_or_else(|| {
            self.fail(format!(
                "nameLen get {}th custom section's name of length {}",
                custom_section_number, name_length
            ))
        })?;

        let payload_bytes = self.parser.source().len() - self.parser.offset();
        let mut payload = Vec::new();
        payload.try_reserve_exact(payload_bytes).map_err(|_| {
            self.fail(format!(
                "can't allocate enough memory for {}th custom section's {} bytes",
                custom_section_number, payload_bytes
            ))
        })?;
        payload.extend_from_slice(&self.parser.source()[self.parser.offset()..]);
        self.parser.offset += payload_bytes;

        if name == "name" {
            // Falha na seção `name` não falha o módulo (o C++ só loga com dumpWasmWarnings).
            match parse_name_section(&payload, self.info, self.parser.use_wasm_simd) {
                Ok(name_section) => self.info.set_name_section(name_section),
                Err(message) => {
                    if Options::with(|options| options.dump_wasm_warnings) {
                        crate::runtime::host::eprintln(&format!("Could not parse name section: {}", message));
                    }
                }
            }
        } else if name == "metadata.code.branch_hint" {
            let _ = parse_branch_hints_section(&payload, self.info);
        } else if name == "sourceMappingURL" {
            let _ = parse_source_mapping_url_section(&payload, self.info);
        }

        self.info.custom_sections.push(CustomSection { name, payload });
        Ok(())
    }
}

/// `BranchHintsSectionParser::parse`. Os erros levam o offset dentro do payload da seção.
pub fn parse_branch_hints_section(payload: &[u8], info: &mut ModuleInformation) -> PartialResult {
    let mut parser = ParserBase::new(payload, false);
    let function_count =
        parser.parse_var_uint32().ok_or_else(|| parser.fail("can't get function count"))?;
    let mut previous_function_index: i64 = -1;

    for i in 0..function_count {
        let function_index = parser
            .parse_var_uint32()
            .ok_or_else(|| parser.fail(&format!("can't get function index for function {}", i)))?;
        if i64::from(function_index) < previous_function_index {
            return Err(parser.fail(&format!("invalid function index {} for function {}", function_index, i)));
        }
        previous_function_index = i64::from(function_index);

        let hint_count = parser
            .parse_var_uint32()
            .ok_or_else(|| parser.fail(&format!("can't get number of hints for function {}", i)))?;
        if hint_count == 0 {
            continue;
        }

        let mut previous_branch_offset: i64 = -1;
        let mut branch_hints_for_function = BranchHintMap::default();
        for j in 0..hint_count {
            let branch_offset = parser
                .parse_var_uint32()
                .ok_or_else(|| parser.fail(&format!("can't get branch offset for hint {}", j)))?;
            if i64::from(branch_offset) < previous_branch_offset || !branch_hints_for_function.is_valid_key(branch_offset) {
                return Err(parser.fail(&format!("invalid branch offset {} for hint {}", branch_offset, j)));
            }
            previous_branch_offset = i64::from(branch_offset);

            let payload_size = parser
                .parse_var_uint32()
                .ok_or_else(|| parser.fail(&format!("can't get payload size for hint {}", j)))?;
            if payload_size != 0x1 {
                return Err(parser.fail(&format!("invalid payload size for hint {}", j)));
            }

            let parsed_branch_hint = parser
                .parse_var_uint1()
                .ok_or_else(|| parser.fail(&format!("can't get or invalid branch hint value for hint {}", j)))?;
            let branch_hint = if parsed_branch_hint == 0 { BranchHint::Unlikely } else { BranchHint::Likely };
            debug_assert!(crate::wasm::wasm_module_information::is_valid_branch_hint(branch_hint));

            branch_hints_for_function.add(branch_offset, branch_hint);
        }
        info.branch_hints.entry(function_index).or_insert(branch_hints_for_function);
    }
    Ok(())
}

/// `SourceMappingURLSectionParser::parse`.
pub fn parse_source_mapping_url_section(payload: &[u8], info: &mut ModuleInformation) -> PartialResult {
    let mut parser = ParserBase::new(payload, false);
    let length = parser.parse_var_uint32().ok_or_else(|| parser.fail("can't get source mapping URL length"))?;
    let name = parser
        .consume_utf8_string(length as usize)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .ok_or_else(|| parser.fail(&format!("can't get source mapping URL of length {} for payload ", length)))?;
    info.source_mapping_url = name;
    Ok(())
}
