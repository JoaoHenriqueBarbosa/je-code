//! Tradução parcial de `wasm/WasmStreamingParser.cpp`: o enquadramento do módulo
//! (`[cabeçalho][seção]*`, com a seção sendo `[id][tamanho][payload]`) e a validação da ordem das
//! seções.
//!
//! O C++ é uma máquina de estados que recebe pedaços de bytes (`addBytes`) e termina em
//! `finalize()`. Aqui entra o módulo inteiro de uma vez, que é o `addBytes` de todos os bytes
//! seguido de `finalize()`: os mesmos estados, as mesmas mensagens e os mesmos offsets de erro
//! (o `m_offset` do C++ é o começo da unidade em leitura). A seção `Code` não passa pelo
//! `SectionParser`: o enquadramento dos corpos (contagem, tamanho de cada função, `FunctionData`)
//! é daqui. O `didReceiveFunctionData` do cliente (`IPIntPlan`) não faz nada: a validação dos corpos
//! é do `IPIntPlan::compileFunction`, que `validate_module` reproduz depois do enquadramento.

use crate::runtime::options::Options;
use crate::wasm::wasm_function_validator::validate_function;
use crate::wasm::wasm_limits::{MAX_FUNCTION_SIZE, MAX_MODULE_SIZE};
use crate::wasm::wasm_module_information::ModuleInformation;
use crate::wasm::wasm_section_parser::SectionParser;
use crate::wasm::wasm_sections::{Section, decode_section, is_known_section, section_name, validate_order};
use crate::wtf::leb_decoder;
use crate::wtf::sha1::Sha1;

/// `moduleHeaderSize`.
const MODULE_HEADER_SIZE: usize = 8;
/// `expectedVersionNumber` (`wasm.json`, preâmbulo `version`).
const EXPECTED_VERSION_NUMBER: u32 = 1;

/// O texto de `StreamingParser::fail` no offset `offset`.
fn fail_at(offset: usize, message: &str) -> String {
    format!("WebAssembly.Module doesn't parse at byte {}: {}", offset, message)
}

/// `consumeVarUInt32`: um LEB de 32 bits que começa em `offset`, lido numa janela de no máximo
/// `maxByteLength<uint32_t>()` bytes. Devolve o valor e quantos bytes ocupou.
fn consume_var_uint32(bytes: &[u8], offset: usize) -> Option<(u32, usize)> {
    let max_size = leb_decoder::max_byte_length(32);
    let window = &bytes[offset..bytes.len().min(offset + max_size)];
    let mut consumed = 0usize;
    let value = leb_decoder::decode_uint32(window, &mut consumed)?;
    Some((value, consumed))
}

/// Os estados `CodeSectionSize`, `FunctionSize` e `FunctionPayload` do `StreamingParser`, de uma
/// vez: lê a contagem de funções e o corpo de cada uma, preenche `functions` e devolve o offset do
/// byte depois da seção e a contagem lida (`m_functionCount`). O tamanho da seção só é conferido no
/// fim, como no C++: os corpos podem passar do fim declarado e a falha vem depois.
fn parse_code_section(
    bytes: &[u8],
    payload_start: usize,
    section_length: u32,
    info: &mut ModuleInformation,
) -> Result<(usize, u32), String> {
    let (function_count, consumed) = consume_var_uint32(bytes, payload_start)
        .ok_or_else(|| fail_at(payload_start, "can't get Code section's count"))?;
    let mut next_offset = payload_start + consumed;
    info.code_section_size = section_length;
    let code_offset = payload_start;

    if function_count == u32::MAX {
        return Err(fail_at(payload_start, &format!("Code section's count is too big {}", function_count)));
    }
    if function_count as usize != info.functions.len() {
        return Err(fail_at(
            payload_start,
            &format!(
                "Code section count {} exceeds the declared number of functions {}",
                function_count,
                info.functions.len()
            ),
        ));
    }

    let ended_early = || fail_at(payload_start, "parsing ended before the end of Code section");
    if function_count == 0 {
        if code_offset + section_length as usize != next_offset {
            return Err(ended_early());
        }
        return Ok((next_offset, function_count));
    }

    let small_function_threshold = Options::with(|options| options.wasm_inlining_small_function_threshold);
    let mut total_function_size = 0usize;
    for function_index in 0..function_count as usize {
        // State::FunctionSize.
        let size_offset = next_offset;
        let (function_size, consumed) = consume_var_uint32(bytes, size_offset)
            .ok_or_else(|| fail_at(size_offset, &format!("can't get {}th Code function's size", function_index)))?;
        if function_size as usize > MAX_FUNCTION_SIZE {
            return Err(fail_at(size_offset, &format!("Code function's size {} is too big", function_size)));
        }
        if function_size < small_function_threshold {
            info.num_small_functions += 1;
        }

        // State::FunctionPayload.
        let function_start = size_offset + consumed;
        let function_size = function_size as usize;
        if bytes.len() - function_start < function_size {
            return Err(fail_at(
                function_start,
                &format!("Code function's size {} exceeds the module's remaining size", function_size),
            ));
        }
        let function = &mut info.functions[function_index];
        function.start = function_start;
        function.end = function_start + function_size;
        function.data = bytes[function_start..function_start + function_size].to_vec();
        total_function_size += function_size;
        next_offset = function_start + function_size;

        if function_index + 1 == function_count as usize {
            info.total_function_size = total_function_size;
            if code_offset + section_length as usize != next_offset {
                return Err(fail_at(function_start, "parsing ended before the end of Code section"));
            }
        }
    }
    Ok((next_offset, function_count))
}

/// `StreamingParser` alimentado com o módulo inteiro e finalizado.
pub fn parse_module(bytes: &[u8], use_wasm_simd: bool) -> Result<ModuleInformation, String> {
    let mut info = ModuleInformation::default();

    if bytes.len() > MAX_MODULE_SIZE {
        return Err(fail_at(0, &format!("module size is too large, maximum {}", MAX_MODULE_SIZE)));
    }

    // State::ModuleHeader
    if bytes.len() < MODULE_HEADER_SIZE {
        return Err(fail_at(0, &format!("expected a module of at least {} bytes", MODULE_HEADER_SIZE)));
    }
    if &bytes[..4] != b"\0asm" {
        return Err(fail_at(0, "module doesn't start with '\\0asm'"));
    }
    let version_number = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version_number != EXPECTED_VERSION_NUMBER {
        return Err(fail_at(
            0,
            &format!("unexpected version number {} expected {}", version_number, EXPECTED_VERSION_NUMBER),
        ));
    }

    // `m_hasher.addBytes(bytes)` em `addBytes`, só com `useEagerWasmModuleHashing`. O módulo
    // inteiro entra de uma vez; o SHA-1 incremental dá o mesmo resumo para qualquer fatiamento.
    let mut hasher = Sha1::new();
    if Options::with(|options| options.use_eager_wasm_module_hashing) {
        hasher.add_bytes(bytes);
    }

    let mut offset = MODULE_HEADER_SIZE;
    let mut previous_known_section = Section::Begin;
    // `m_functionCount`/`m_functionIndex` depois da seção `Code`: sem ela, ficam em zero.
    let mut functions_parsed = 0u32;
    // Sem mais bytes depois de uma seção completa, o estado é `SectionID` com `m_remaining` vazio:
    // `finalize()` encerra o módulo.
    while offset < bytes.len() {
        // State::SectionID. O byte lido por `parseUInt7`; o bit 7 ligado não é id de seção.
        let id_byte = bytes[offset];
        if id_byte >= 0x80 {
            return Err(fail_at(offset, "can't get section byte"));
        }
        let Some(section) = decode_section(id_byte) else {
            return Err(fail_at(offset, "invalid section"));
        };
        debug_assert!(section != Section::Begin);
        if !validate_order(previous_known_section, section) {
            return Err(fail_at(
                offset,
                &format!(
                    "invalid section order, {} followed by {}",
                    section_name(previous_known_section),
                    section_name(section)
                ),
            ));
        }
        if is_known_section(i64::from(section as u8)) {
            previous_known_section = section;
        }
        offset += 1;

        // State::SectionSize.
        let Some((section_length, consumed)) = consume_var_uint32(bytes, offset) else {
            return Err(fail_at(offset, &format!("can't get {} section's length", section_name(section))));
        };
        offset += consumed;

        // `parseSectionSize`: a seção `Code` segue para `CodeSectionSize`, sem o estado
        // `SectionPayload` (o tamanho dela só é conferido contra o fim dos corpos).
        if section == Section::Code {
            let (next_offset, function_count) = parse_code_section(bytes, offset, section_length, &mut info)?;
            offset = next_offset;
            functions_parsed = function_count;
            continue;
        }

        // State::SectionPayload.
        let payload_start = offset;
        let section_length = section_length as usize;
        if section_length > bytes.len() - payload_start {
            return Err(fail_at(
                payload_start,
                &format!("{} section of size {} would overflow Module's size", section_name(section), section_length),
            ));
        }
        let payload = &bytes[payload_start..payload_start + section_length];
        let mut parser = SectionParser::new(payload, payload_start, &mut info, use_wasm_simd);
        parser.parse_section(section)?;
        if parser.source().len() != parser.offset() {
            return Err(fail_at(
                payload_start,
                &format!("parsing ended before the end of {} section", section_name(section)),
            ));
        }
        offset = payload_start + section_length;
    }

    // `finalize()` no estado `SectionID`, com os bytes todos consumidos.
    if functions_parsed as usize != info.functions.len() {
        return Err(fail_at(
            offset,
            &format!(
                "Number of functions parsed ({}) does not match the number of declared functions ({})",
                functions_parsed,
                info.functions.len()
            ),
        ));
    }
    if let Some(number_of_data_segments) = info.number_of_data_segments {
        if info.data.len() != number_of_data_segments as usize {
            return Err(fail_at(
                offset,
                &format!(
                    "Data section's count {} is different from Data Count section's count {}",
                    info.data.len(),
                    number_of_data_segments
                ),
            ));
        }
    }
    if Options::with(|options| options.use_eager_wasm_module_hashing) {
        info.name_section.set_hash(Some(&hasher.compute_hex_digest()));
    }
    info.import_should_be_hidden = vec![false; info.imports.len()];

    Ok(info)
}

/// `Module::validateSync`: o `StreamingParser` alimentado com o módulo inteiro e, em seguida, o
/// `IPIntPlan` que valida o corpo de cada função (`IPIntPlan::compileFunction`, que no C++ roda o
/// `FunctionParser<IPIntGenerator>`; aqui o `FunctionValidator`, que aceita o que o parser conferiu).
/// `IPIntPlan::didReceiveFunctionData` não faz nada ("Validation is done inline by the parser"): a
/// validação dos corpos começa depois do enquadramento inteiro, em partes de tamanho limitado por
/// `EntryPlan::compileFunctions`.
///
/// O erro de função leva o sufixo `, in function at index N` (índice no espaço de código). Dentro de
/// uma parte todas as funções são validadas e vale o menor índice com erro (`Plan::failAtFunction`);
/// depois de cada parte vem `failIfMixedExceptionHandlingProposals`, que só falha se nenhuma função
/// tinha falhado (`Plan::fail` não sobrescreve).
pub fn validate_module(bytes: &[u8], use_wasm_simd: bool) -> Result<ModuleInformation, String> {
    let mut info = parse_module(bytes, use_wasm_simd)?;

    let function_count = info.functions.len();
    let mut compile_limit = Options::with(|options| options.wasm_small_partial_compile_limit);
    if Options::with(|options| options.use_concurrent_jit) {
        // When the size of wasm binary requires 3 loops, use large limit.
        let threads = Options::with(|options| options.number_of_wasm_compiler_threads) as usize;
        if info.total_function_size > 3usize.saturating_mul(compile_limit).saturating_mul(threads) {
            compile_limit = Options::with(|options| options.wasm_large_partial_compile_limit);
        }
    }

    let mut uses_legacy_exceptions = false;
    let mut uses_modern_exceptions = false;
    let mut current_index = 0usize;
    while current_index < function_count {
        // A parte que `compileFunctions` reserva para esta rodada.
        let function_index = current_index;
        let mut function_index_end = function_count;
        let mut bytes_compiled = 0usize;
        for index in function_index..function_count {
            let byte_size = info.functions[index].data.len();
            // If One function's size is larger than the limit itself, we compile it separately from
            // the current sequence, so that we can distribute compilation tasks more uniformly.
            if bytes_compiled != 0 && byte_size >= compile_limit {
                function_index_end = index;
                break;
            }
            bytes_compiled += byte_size;
            if bytes_compiled >= compile_limit {
                function_index_end = index + 1;
                break;
            }
        }
        current_index = function_index_end;

        let mut first_error: Option<String> = None;
        for index in function_index..function_index_end {
            let signature_position = info.internal_function_type_signature_indices[index];
            let body = std::mem::take(&mut info.functions[index].data);
            let result = validate_function(&info, &body, signature_position);
            info.functions[index].data = body;
            match result {
                Ok(validation) => {
                    if validation.uses_simd {
                        info.mark_uses_simd(index);
                    }
                    uses_legacy_exceptions |= validation.uses_legacy_exceptions;
                    uses_modern_exceptions |= validation.uses_modern_exceptions;
                    if validation.uses_legacy_exceptions || validation.uses_modern_exceptions {
                        info.mark_uses_exceptions(index);
                    }
                    info.done_seeing_function(index);
                }
                Err(message) => {
                    if first_error.is_none() {
                        first_error = Some(format!("{}, in function at index {}", message, index));
                    }
                }
            }
        }
        if let Some(message) = first_error {
            return Err(message);
        }
        if uses_legacy_exceptions && uses_modern_exceptions {
            return Err("Module uses both legacy exceptions and try_table".to_string());
        }
    }

    info.uses_legacy_exceptions = uses_legacy_exceptions;
    info.uses_modern_exceptions = uses_modern_exceptions;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::{Type, TypeIndex, TypeKind};
    use crate::wasm::wasm_module_information::StructuralType;

    const HEADER: [u8; 8] = [0, b'a', b's', b'm', 1, 0, 0, 0];

    fn module(sections: &[&[u8]]) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        for section in sections {
            bytes.extend_from_slice(section);
        }
        bytes
    }

    #[test]
    fn empty_module() {
        let info = parse_module(&HEADER, true).unwrap();
        assert_eq!(info.type_count(), 0);
    }

    #[test]
    fn header_errors() {
        assert_eq!(
            parse_module(&[0, b'a', b's'], true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 0: expected a module of at least 8 bytes")
        );
        assert_eq!(
            parse_module(&[1, b'a', b's', b'm', 1, 0, 0, 0], true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 0: module doesn't start with '\\0asm'")
        );
        assert_eq!(
            parse_module(&[0, b'a', b's', b'm', 2, 0, 0, 0], true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 0: unexpected version number 2 expected 1")
        );
    }

    #[test]
    fn function_types() {
        // (type (func (param i32 i64) (result f32))) (type (func))
        let type_section: &[u8] = &[1, 10, 2, 0x60, 2, 0x7f, 0x7e, 1, 0x7d, 0x60, 0, 0];
        let info = parse_module(&module(&[type_section]), true).unwrap();
        assert_eq!(info.type_count(), 2);
        let i32_type = Type::new(TypeKind::I32, TypeIndex::Invalid);
        let i64_type = Type::new(TypeKind::I64, TypeIndex::Invalid);
        let f32_type = Type::new(TypeKind::F32, TypeIndex::Invalid);
        assert_eq!(
            info.types[0].structural,
            StructuralType::Function { arguments: vec![i32_type, i64_type], returns: vec![f32_type] }
        );
        assert_eq!(info.types[0].subtype, None);
        assert_eq!(info.types[1].structural, StructuralType::Function { arguments: vec![], returns: vec![] });
    }

    #[test]
    fn recursive_group_with_struct() {
        // (rec (type $a (struct (field (ref null $b)))) (type $b (func)))
        let type_section: &[u8] = &[1, 11, 1, 0x4e, 2, 0x5f, 1, 0x63, 1, 0x01, 0x60, 0, 0];
        let info = parse_module(&module(&[type_section]), true).unwrap();
        assert_eq!(info.type_count(), 2);
        assert!(info.has_gc_object_types);
        assert_eq!(info.types[1].recursion_group_start, 0);
        assert_eq!(info.types[1].recursion_group_len, 2);
        match &info.types[0].structural {
            StructuralType::Struct { fields } => {
                assert_eq!(fields.len(), 1);
            }
            other => panic!("esperava struct, veio {other:?}"),
        }
    }

    #[test]
    fn custom_section_keeps_name_and_payload() {
        let custom: &[u8] = &[0, 6, 3, b'a', b'b', b'c', 9, 8];
        let info = parse_module(&module(&[custom]), true).unwrap();
        assert_eq!(info.custom_sections.len(), 1);
        assert_eq!(info.custom_sections[0].name, "abc");
        assert_eq!(info.custom_sections[0].payload, vec![9, 8]);
    }

    #[test]
    fn section_framing_errors() {
        // Dois Type seguidos: ordem inválida.
        let type_section: &[u8] = &[1, 1, 0];
        assert_eq!(
            parse_module(&module(&[type_section, type_section]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 11: invalid section order, Type followed by Type")
        );
        // Id desconhecido.
        assert_eq!(
            parse_module(&module(&[&[0x50, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 8: invalid section")
        );
        // Payload maior que o módulo.
        assert_eq!(
            parse_module(&module(&[&[1, 5, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 10: Type section of size 5 would overflow Module's size")
        );
        // Comprimento de seção truncado.
        assert_eq!(
            parse_module(&module(&[&[1]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 9: can't get Type section's length")
        );
        // Sobra de bytes dentro do payload da seção.
        assert_eq!(
            parse_module(&module(&[&[1, 2, 0, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 10: parsing ended before the end of Type section")
        );
    }

    #[test]
    fn type_section_error_offsets_are_relative_to_the_module() {
        // Tipo de função com argumento de tipo inválido (0x40 = void): o byte lido é o 14 do módulo
        // e o offset reportado é o seguinte ao byte consumido pelo `parseInt7`.
        let type_section: &[u8] = &[1, 5, 1, 0x60, 1, 0x40, 0];
        assert_eq!(
            parse_module(&module(&[type_section]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 14: can't get 0th argument Type")
        );
    }

    // (type (func)), (func (type 0)).
    const TYPE_SECTION: &[u8] = &[1, 4, 1, 0x60, 0, 0];
    const FUNCTION_SECTION: &[u8] = &[3, 2, 1, 0];

    #[test]
    fn module_with_every_section() {
        let memory: &[u8] = &[5, 3, 1, 0, 1];
        let export: &[u8] = &[7, 5, 1, 1, b'f', 0, 0];
        let code: &[u8] = &[10, 4, 1, 2, 0, 0x0b];
        // Um segmento ativo na memória 0, deslocamento `i32.const 0`, dois bytes.
        let data: &[u8] = &[11, 8, 1, 0, 0x41, 0, 0x0b, 2, 0xaa, 0xbb];
        let info = parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, memory, export, code, data]), true).unwrap();
        assert_eq!(info.functions.len(), 1);
        assert_eq!((info.functions[0].start, info.functions[0].end), (34, 36));
        assert_eq!(info.functions[0].data, vec![0, 0x0b]);
        assert_eq!(info.code_section_size, 4);
        assert_eq!(info.total_function_size, 2);
        assert_eq!(info.memories[0].initial.page_count(), 1);
        assert!(!info.memories[0].maximum.has_value());
        assert_eq!(info.exports[0].field, "f");
        assert_eq!(info.exports[0].kind_index, 0);
        assert!(info.is_declared_function(0));
        assert_eq!(info.data.len(), 1);
        assert_eq!(info.data[0].bytes, vec![0xaa, 0xbb]);
        assert_eq!(info.data[0].offset_if_active, Some(crate::wasm::wasm_format::I32InitExpr::Const(0)));
        assert_eq!(info.import_should_be_hidden.len(), 0);
    }

    #[test]
    fn code_section_framing_errors() {
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, &[10, 1, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 20: Code section count 0 exceeds the declared number of functions 1")
        );
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, &[10, 3, 1, 5, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 22: Code function's size 5 exceeds the module's remaining size")
        );
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, &[10, 9, 1, 2, 0, 0x0b]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 22: parsing ended before the end of Code section")
        );
        // Declarou uma função e não mandou a seção `Code`.
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 18: Number of functions parsed (0) does not match the number of declared functions (1)")
        );
    }

    #[test]
    fn data_count_must_match_the_data_section() {
        assert_eq!(
            parse_module(&module(&[&[12, 1, 2]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 11: Data section's count 0 is different from Data Count section's count 2")
        );
        assert!(parse_module(&module(&[&[12, 1, 0]]), true).is_ok());
    }

    #[test]
    fn section_errors_use_the_cpp_messages() {
        // Export de uma função que não existe.
        assert_eq!(
            parse_module(&module(&[&[7, 5, 1, 1, b'f', 0, 0]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 15: 0th Export has invalid function number 0 it exceeds the function index space 0, named 'f'")
        );
        // Dois exports com o mesmo nome.
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, &[7, 9, 2, 1, b'f', 0, 0, 1, b'f', 0, 0]]), true)
                .err()
                .as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 27: duplicate export: 'f'")
        );
        // Função com índice de tipo fora da seção de tipos.
        assert_eq!(
            parse_module(&module(&[TYPE_SECTION, &[3, 2, 1, 5]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 18: 0th Function type number is invalid 5")
        );
        // Limites de memória com o inicial maior que o máximo.
        assert_eq!(
            parse_module(&module(&[&[5, 4, 1, 1, 2, 1]]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 14: resizable limits has an initial page count of 2 which is greater than its maximum 1")
        );
    }

    #[test]
    fn subtype_validation_follows_the_gc_rules() {
        // (type $a (sub (func))), (type $b (sub final $a (func (param i32)))): os parâmetros diferem.
        let invalid: &[u8] = &[1, 13, 2, 0x50, 0, 0x60, 0, 0, 0x4f, 1, 0, 0x60, 1, 0x7f, 0];
        assert_eq!(
            parse_module(&module(&[invalid]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 23: structural type is not a subtype of the specified supertype")
        );
        // Mesma estrutura, supertipo aberto: válido.
        let valid: &[u8] = &[1, 12, 2, 0x50, 0, 0x60, 0, 0, 0x4f, 1, 0, 0x60, 0, 0];
        let info = parse_module(&module(&[valid]), true).unwrap();
        assert_eq!(info.display_size_excluding_this(1), 1);
        // Subtipo de um tipo final.
        let final_super: &[u8] = &[1, 10, 2, 0x60, 0, 0, 0x50, 1, 0, 0x60, 0, 0];
        assert_eq!(
            parse_module(&module(&[final_super]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 20: cannot declare subtype of final supertype")
        );
    }

    const TWO_FUNCTIONS: &[u8] = &[3, 3, 2, 0, 0];

    #[test]
    fn validate_module_accepts_valid_bodies_and_records_what_they_use() {
        let code: &[u8] = &[10, 4, 1, 2, 0, 0x0b];
        let info = validate_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, code]), true).unwrap();
        assert!(info.functions[0].finished_validating);
        assert!(!info.uses_simd(0));
        assert!(!info.uses_legacy_exceptions && !info.uses_modern_exceptions);

        // v128.const; drop; end: 21 bytes de corpo.
        let mut body = vec![0x00, 0xfd, 0x0c];
        body.extend_from_slice(&[0; 16]);
        body.extend_from_slice(&[0x1a, 0x0b]);
        let mut code = vec![10, 23, 1, 21];
        code.extend_from_slice(&body);
        let info = validate_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, &code]), true).unwrap();
        assert!(info.uses_simd(0));
    }

    #[test]
    fn validate_module_names_the_failing_function() {
        // i32.add sem operandos na função 0.
        let code: &[u8] = &[10, 5, 1, 3, 0, 0x6a, 0x0b];
        assert_eq!(
            validate_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, code]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 2: can't pop empty stack in binary right, in function at index 0")
        );
        // A função 0 é válida e a 1 não.
        let code: &[u8] = &[10, 8, 2, 2, 0, 0x0b, 3, 0, 0x6a, 0x0b];
        assert_eq!(
            validate_module(&module(&[TYPE_SECTION, TWO_FUNCTIONS, code]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 2: can't pop empty stack in binary right, in function at index 1")
        );
        // Erro de enquadramento continua vindo do StreamingParser, sem o sufixo.
        let code: &[u8] = &[10, 3, 1, 5, 0];
        assert_eq!(
            validate_module(&module(&[TYPE_SECTION, FUNCTION_SECTION, code]), true).err().as_deref(),
            Some("WebAssembly.Module doesn't parse at byte 22: Code function's size 5 exceeds the module's remaining size")
        );
    }

    #[test]
    fn validate_module_rejects_mixing_legacy_and_modern_exceptions() {
        // Função 0: try (void); catch_all; end; end. Função 1: try_table (void) sem catches; end; end.
        let code: &[u8] = &[10, 15, 2, 6, 0, 0x06, 0x40, 0x19, 0x0b, 0x0b, 6, 0, 0x1f, 0x40, 0x00, 0x0b, 0x0b];
        assert_eq!(
            validate_module(&module(&[TYPE_SECTION, TWO_FUNCTIONS, code]), true).err().as_deref(),
            Some("Module uses both legacy exceptions and try_table")
        );
    }
}
