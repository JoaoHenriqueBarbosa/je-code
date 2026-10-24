//! Tradução de `wasm/WasmParser.h`: `ParserBase`, o leitor de bytes com LEB128, tipos de valor e
//! tipos heap que as seções usam.
//!
//! O C++ devolve `bool` e preenche um parâmetro de saída; aqui cada leitura devolve `Option<T>`
//! (`None` é o `false` do C++). O `offset` avança como no C++, inclusive em leitura que falha.

use crate::wasm::wasm_format::{
    ExternalKind, Type, TypeIndex, TypeKind, is_ref_type, is_valid_heap_type_kind, is_valid_type_kind, is_value_type,
    type_index_from_type_kind,
};
use crate::wasm::wasm_module_information::ModuleInformation;
use crate::wtf::leb_decoder;
use crate::wtf::unicode::utf8_conversion::check_utf8_without_utf16_length;

/// `RecursionGroupInformation` (de `WasmTypeSectionState.h`): o grupo recursivo em andamento, em
/// posições absolutas do espaço de tipos, `[start, end)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecursionGroupInformation {
    pub in_recursion_group: bool,
    pub start: u32,
    pub end: u32,
}

/// `ParserBase`. `type_section_state` do C++ (ponteiro nulo fora da seção de tipos) vira
/// `recursion_group_information`, cujo valor padrão (`in_recursion_group == false`) se comporta
/// como o ponteiro nulo em todas as leituras.
pub struct ParserBase<'a> {
    source: &'a [u8],
    pub(crate) offset: usize,
    pub(crate) recursion_group_information: RecursionGroupInformation,
    /// `Options::useWasmSIMD()`, que decide se `v128` é tipo de valor.
    pub(crate) use_wasm_simd: bool,
}

impl<'a> ParserBase<'a> {
    pub fn new(source: &'a [u8], use_wasm_simd: bool) -> ParserBase<'a> {
        ParserBase { source, offset: 0, recursion_group_information: RecursionGroupInformation::default(), use_wasm_simd }
    }

    pub fn source(&self) -> &'a [u8] {
        self.source
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// `ParserBase::fail`: o texto que a API JS devolve num `CompileError`.
    pub fn fail(&self, message: &str) -> String {
        format!("WebAssembly.Module doesn't parse at byte {}: {}", self.offset, message)
    }

    /// `consumeCharacter`.
    pub fn consume_character(&mut self, c: u8) -> bool {
        if self.offset >= self.source.len() {
            return false;
        }
        if c == self.source[self.offset] {
            self.offset += 1;
            return true;
        }
        false
    }

    /// `consumeString`.
    pub fn consume_string(&mut self, string: &[u8]) -> bool {
        let start = self.offset;
        if self.offset >= self.source.len() {
            return false;
        }
        for &c in string {
            if !self.consume_character(c) {
                self.offset = start;
                return false;
            }
        }
        true
    }

    /// `consumeUTF8String`: lê `string_length` bytes que precisam ser UTF-8 válido.
    pub fn consume_utf8_string(&mut self, string_length: usize) -> Option<&'a [u8]> {
        if string_length == 0 {
            return Some(&[]);
        }
        if self.source.len() < string_length || self.offset > self.source.len() - string_length {
            return None;
        }
        let string = &self.source[self.offset..self.offset + string_length];
        if check_utf8_without_utf16_length(string).len() != string.len() {
            return None;
        }
        self.offset += string.len();
        Some(string)
    }

    /// `parseVarUInt32`.
    pub fn parse_var_uint32(&mut self) -> Option<u32> {
        leb_decoder::decode_uint32(self.source, &mut self.offset)
    }

    /// `peekVarUInt32`.
    pub fn peek_var_uint32(&mut self) -> Option<u32> {
        let saved_offset = self.offset;
        let result = self.parse_var_uint32();
        self.offset = saved_offset;
        result
    }

    /// `parseVarUInt64`.
    pub fn parse_var_uint64(&mut self) -> Option<u64> {
        leb_decoder::decode_uint64(self.source, &mut self.offset)
    }

    /// `parseVarInt32`.
    pub fn parse_var_int32(&mut self) -> Option<i32> {
        leb_decoder::decode_int32(self.source, &mut self.offset)
    }

    /// `parseVarInt64`.
    pub fn parse_var_int64(&mut self) -> Option<i64> {
        leb_decoder::decode_int64(self.source, &mut self.offset)
    }

    /// `parseUInt32`: 4 bytes little-endian, sem exigir alinhamento.
    pub fn parse_uint32(&mut self) -> Option<u32> {
        let bytes = self.source.get(self.offset..)?.first_chunk::<4>()?;
        self.offset += 4;
        Some(u32::from_le_bytes(*bytes))
    }

    /// `parseUInt64`: 8 bytes little-endian.
    pub fn parse_uint64(&mut self) -> Option<u64> {
        let bytes = self.source.get(self.offset..)?.first_chunk::<8>()?;
        self.offset += 8;
        Some(u64::from_le_bytes(*bytes))
    }

    /// `parseImmByteArray16`: os 16 bytes de um `v128`.
    pub fn parse_imm_byte_array16(&mut self) -> Option<[u8; 16]> {
        let bytes = self.source.get(self.offset..)?.first_chunk::<16>()?;
        self.offset += 16;
        Some(*bytes)
    }

    /// `peekUInt8`.
    pub fn peek_uint8(&self) -> Option<u8> {
        self.source.get(self.offset).copied()
    }

    /// `parseUInt8`.
    pub fn parse_uint8(&mut self) -> Option<u8> {
        let result = self.peek_uint8()?;
        self.offset += 1;
        Some(result)
    }

    /// `int7Of(byte)`: o byte com o bit 6 estendido para o 7.
    fn int7_of(byte: u8) -> i8 {
        if byte & 0x40 != 0 { (byte | 0x80) as i8 } else { byte as i8 }
    }

    /// `parseInt7`: o offset avança mesmo quando o bit de continuação está ligado (falha).
    pub fn parse_int7(&mut self) -> Option<i8> {
        let byte = self.parse_uint8()?;
        (byte & 0x80 == 0).then(|| Self::int7_of(byte))
    }

    /// `peekInt7`.
    pub fn peek_int7(&self) -> Option<i8> {
        let byte = self.peek_uint8()?;
        (byte & 0x80 == 0).then(|| Self::int7_of(byte))
    }

    /// `parseUInt7`.
    pub fn parse_uint7(&mut self) -> Option<u8> {
        let result = self.parse_uint8()?;
        (result < 0x80).then_some(result)
    }

    /// `parseVarUInt1`.
    pub fn parse_var_uint1(&mut self) -> Option<u8> {
        let temp = self.parse_var_uint32()?;
        (temp <= 1).then_some(temp as u8)
    }

    /// `parseHeapType`: um tipo heap abstrato (negativo) ou o índice de um tipo definido.
    pub fn parse_heap_type(&mut self, info: &ModuleInformation) -> Option<i32> {
        let heap_type = self.parse_var_int32()?;
        if heap_type < 0 {
            return is_valid_heap_type_kind(i64::from(heap_type)).then_some(heap_type);
        }
        let group = self.recursion_group_information;
        let in_current_group = group.in_recursion_group
            && heap_type as u32 >= group.start
            && (heap_type as u32) < group.end;
        if heap_type as usize >= info.type_count() && !in_current_group {
            return None;
        }
        Some(heap_type)
    }

    /// `parseValueType`.
    pub fn parse_value_type(&mut self, info: &ModuleInformation) -> Option<Type> {
        let kind = self.parse_int7()?;
        if !is_valid_type_kind(kind) {
            return None;
        }
        let mut type_kind = TypeKind::from_i8(kind)?;
        let mut type_index = TypeIndex::Invalid;
        if is_valid_heap_type_kind(i64::from(kind)) {
            // Uma forma abreviada: `funcref` é `(ref null func)`.
            type_index = type_index_from_type_kind(type_kind);
            type_kind = TypeKind::RefNull;
        } else if type_kind == TypeKind::Ref || type_kind == TypeKind::RefNull {
            let heap_type = self.parse_heap_type(info)?;
            if heap_type < 0 {
                type_index = type_index_from_type_kind(TypeKind::from_i8(heap_type as i8)?);
            } else {
                let group = self.recursion_group_information;
                if group.in_recursion_group && heap_type as u32 >= group.start {
                    // Referência recursiva dentro de um grupo: um placeholder com o índice relativo
                    // ao grupo, trocado por um índice real antes do uso.
                    debug_assert!(heap_type as usize >= info.type_count() && (heap_type as u32) < group.end);
                    type_index = TypeIndex::Projection(heap_type as u32 - group.start);
                } else {
                    debug_assert!((heap_type as usize) < info.type_count());
                    // `info.rtt(typeSignatureIndexFromHeapType(heapType)).asTypeIndex()`.
                    type_index = info.type_index_of(heap_type as usize);
                }
            }
        }
        let ty = Type::new(type_kind, type_index);
        is_value_type(ty, self.use_wasm_simd).then_some(ty)
    }

    /// `parseRefType`.
    pub fn parse_ref_type(&mut self, info: &ModuleInformation) -> Option<Type> {
        self.parse_value_type(info).filter(|ty| is_ref_type(*ty))
    }

    /// `parseExternalKind`.
    pub fn parse_external_kind(&mut self) -> Option<ExternalKind> {
        self.parse_uint7().and_then(ExternalKind::from_u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_module_information::ModuleInformation;

    #[test]
    fn int7_sign_extends_bit_six() {
        let mut parser = ParserBase::new(&[0x7f, 0x40, 0x3f, 0x80], true);
        assert_eq!(parser.parse_int7(), Some(-1));
        assert_eq!(parser.parse_int7(), Some(-64));
        assert_eq!(parser.parse_int7(), Some(63));
        // Bit de continuação ligado: falha, mas o offset avançou.
        assert_eq!(parser.parse_int7(), None);
        assert_eq!(parser.offset(), 4);
    }

    #[test]
    fn value_types() {
        let info = ModuleInformation::default();
        // i32, funcref (0x70 = -16), (ref null extern) por 0x63 0x6f, v128 (0x7b).
        let mut parser = ParserBase::new(&[0x7f, 0x70, 0x63, 0x6f, 0x7b, 0x40], true);
        assert_eq!(parser.parse_value_type(&info), Some(Type::new(TypeKind::I32, TypeIndex::Invalid)));
        assert_eq!(
            parser.parse_value_type(&info),
            Some(Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Funcref)))
        );
        assert_eq!(
            parser.parse_value_type(&info),
            Some(Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Externref)))
        );
        assert_eq!(parser.parse_value_type(&info), Some(Type::new(TypeKind::V128, TypeIndex::Invalid)));
        // void (-64) é tipo válido de bloco, mas não de valor.
        assert_eq!(parser.parse_value_type(&info), None);
        let mut without_simd = ParserBase::new(&[0x7b], false);
        assert_eq!(without_simd.parse_value_type(&info), None);
    }

    #[test]
    fn concrete_heap_type_needs_a_declared_type() {
        let info = ModuleInformation::default();
        // (ref 0) sem nenhum tipo declarado e fora de grupo recursivo.
        let mut parser = ParserBase::new(&[0x64, 0x00], true);
        assert_eq!(parser.parse_value_type(&info), None);
        // Dentro de um grupo [0, 1), o índice 0 é um placeholder.
        let mut parser = ParserBase::new(&[0x64, 0x00], true);
        parser.recursion_group_information = RecursionGroupInformation { in_recursion_group: true, start: 0, end: 1 };
        assert_eq!(parser.parse_value_type(&info), Some(Type::new(TypeKind::Ref, TypeIndex::Projection(0))));
    }

    #[test]
    fn fixed_width_reads() {
        let mut parser = ParserBase::new(&[1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0], true);
        assert_eq!(parser.parse_uint32(), Some(1));
        assert_eq!(parser.parse_uint64(), Some(2));
        assert_eq!(parser.parse_uint32(), None);
        let mut parser = ParserBase::new(b"\0asm", true);
        assert!(parser.consume_string(b"\0as"));
        assert!(!parser.consume_string(b"mx"));
        assert_eq!(parser.offset(), 3);
    }
}
