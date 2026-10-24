//! Tradução do `ConstExprInterpreter` e de `evaluateExtendedConstExpr`
//! (`wasm/WasmConstExprGenerator.cpp`): a avaliação, numa instância, de uma expressão constante que
//! o `ConstExprGenerator` já validou (valor inicial de global, índice de elemento, deslocamento de
//! segmento de dados e de elementos).
//!
//! O C++ lê a instância direto (`JSWebAssemblyInstance`: `loadI64Global`, `loadV128Global`,
//! `ensureFunctionWrapper`, `gcObjectStructure`, `structNew`, `arrayNew`...). Aqui tudo o que toca a
//! instância passa pelo trait `ConstExprHost`, que a fatia da instância implementa: o interpretador
//! não conhece `JSValue` além da codificação de `null` e de `i31`, e não aloca nada. O que o C++
//! guarda em `m_keepAlive` (objetos alocados pela expressão, que o GC precisa enxergar) fica a cargo
//! do host, que é quem tem o `MarkedArgumentBuffer`.
//!
//! O interpretador decodifica os imediatos sem as conferências do `LEBDecoder`, porque a validação
//! já os aceitou: um byte só em quase todos os casos.

use crate::runtime::js_value::{js_null, js_number_i32};
use crate::wasm::wasm_format::{StorageType, TypeKind, V128};
use crate::wasm::wasm_function_parser::unpacked;
use crate::wasm::wasm_module_information::{ModuleInformation, StructuralType};
use crate::wasm::wasm_ops::{ExtGCOpType, OpType, EXT_SIMD_V128_CONST};

/// `ConstExprValue`: o que uma expressão constante pode produzir. O `Invalid` do C++ (alocação que
/// falhou) vira `None` nos métodos do host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstExprValue {
    /// `Numeric`: i32 (com os 32 bits altos sem significado), i64, f32 e f64 em bits.
    Numeric(u64),
    /// `Vector`.
    Vector(V128),
    /// `Ref`: um `JSValue` codificado.
    Ref(u64),
}

impl ConstExprValue {
    /// `getValue`.
    pub fn value(&self) -> u64 {
        match self {
            ConstExprValue::Numeric(bits) | ConstExprValue::Ref(bits) => *bits,
            ConstExprValue::Vector(_) => unreachable!("getValue em vetor"),
        }
    }

    /// `getVector`.
    pub fn vector(&self) -> V128 {
        match self {
            ConstExprValue::Vector(vector) => *vector,
            _ => unreachable!("getVector em valor que não é vetor"),
        }
    }

    fn is_vector(&self) -> bool {
        matches!(self, ConstExprValue::Vector(_))
    }
}

/// O que a expressão precisa da instância. Os métodos que alocam devolvem `None` quando a alocação
/// falha (o `isNull()` do resultado de `structNew`/`arrayNew` no C++).
pub trait ConstExprHost {
    /// `loadI64Global` e `loadV128Global`: `is_v128` escolhe qual, como `loadGlobal`.
    fn load_global(&mut self, index: u32, is_v128: bool) -> ConstExprValue;
    /// `ensureFunctionWrapper`: o `JSValue` codificado do wrapper da função.
    fn ref_func(&mut self, function_index_space: u32) -> u64;
    /// `externInternalize`.
    fn extern_internalize(&mut self, reference: u64) -> u64;
    /// `structNew` com `UseDefaultValue::Yes` (`fields` vazio e `use_default`) ou com os campos
    /// que estavam na pilha, na ordem de declaração.
    fn struct_new(&mut self, type_index: u32, fields: Option<&[ConstExprValue]>) -> Option<u64>;
    /// `arrayNew`: `size` elementos iguais a `value`.
    fn array_new(&mut self, type_index: u32, size: u32, value: &ConstExprValue) -> Option<u64>;
    /// `createNewArray(typeIndex, elements)`: um elemento por entrada da pilha.
    fn array_new_fixed(&mut self, type_index: u32, elements: &[ConstExprValue]) -> Option<u64>;
}

/// `ConstExprInterpreter`.
pub struct ConstExprInterpreter<'a, H: ConstExprHost> {
    offset_in_source: usize,
    info: &'a ModuleInformation,
    host: &'a mut H,
}

impl<'a, H: ConstExprHost> ConstExprInterpreter<'a, H> {
    pub fn new(offset_in_source: usize, info: &'a ModuleInformation, host: &'a mut H) -> Self {
        ConstExprInterpreter { offset_in_source, info, host }
    }

    /// `failedToAllocate`: o offset é o da instrução em curso, como o parser do módulo o reportaria.
    fn failed_to_allocate(&self, kind: &str, offset: usize) -> String {
        format!(
            "WebAssembly.Module doesn't parse at byte {}: Failed to allocate new {}",
            self.offset_in_source + offset,
            kind
        )
    }

    /// `createNewDefaultArray`: o valor com que `array.new_default` enche o vetor.
    fn default_element(&self, type_index: u32) -> ConstExprValue {
        let StructuralType::Array { element } = &self.info.rtt(type_index as usize).structural else {
            unreachable!("array.new_default de um tipo que não é array");
        };
        let element_type = match element.ty {
            StorageType::Type(ty) => ty,
            packed => unpacked(packed),
        };
        if crate::wasm::wasm_format::is_ref_type(element_type) {
            return ConstExprValue::Numeric(js_null().encode() as u64);
        }
        if element_type.kind == TypeKind::V128 {
            return ConstExprValue::Vector([0; 16]);
        }
        ConstExprValue::Numeric(0)
    }

    /// `run`.
    pub fn run(&mut self, bytes: &[u8]) -> Result<u64, String> {
        let mut stack: Vec<ConstExprValue> = Vec::with_capacity(16);
        let mut offset = 0usize;

        macro_rules! var_uint32 {
            () => {{
                let mut byte = bytes[offset];
                offset += 1;
                if byte & 0x80 == 0 {
                    u32::from(byte)
                } else {
                    let mut value = u32::from(byte & 0x7f);
                    let mut shift = 7u32;
                    loop {
                        byte = bytes[offset];
                        offset += 1;
                        value |= u32::from(byte & 0x7f) << shift;
                        shift += 7;
                        if byte & 0x80 == 0 {
                            break;
                        }
                    }
                    value
                }
            }};
        }
        macro_rules! var_int64 {
            () => {{
                let mut byte = bytes[offset];
                offset += 1;
                if byte & 0x80 == 0 {
                    i64::from(((byte << 1) as i8) >> 1)
                } else {
                    let mut value = i64::from(byte & 0x7f);
                    let mut shift = 7u32;
                    loop {
                        byte = bytes[offset];
                        offset += 1;
                        value |= i64::from(byte & 0x7f) << shift;
                        shift += 7;
                        if byte & 0x80 == 0 {
                            break;
                        }
                    }
                    if shift < 64 && byte & 0x40 != 0 {
                        value |= (-1i64).wrapping_shl(shift);
                    }
                    value
                }
            }};
        }

        loop {
            assert!(offset < bytes.len());
            let op = bytes[offset];
            offset += 1;
            let result: ConstExprValue = match OpType::from_value(op) {
                Some(OpType::I32Const) => {
                    // `static_cast<uint64_t>(varInt32())`: o i32 com sinal estendido.
                    let value = var_int64!() as i32;
                    stack.push(ConstExprValue::Numeric(i64::from(value) as u64));
                    continue;
                }
                Some(OpType::I64Const) => {
                    stack.push(ConstExprValue::Numeric(var_int64!() as u64));
                    continue;
                }
                Some(OpType::F32Const) => {
                    // Invariante: os bytes já passaram pela validação da expressão constante (mesmo ASSERT do C++).
                    let value = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                    offset += 4;
                    stack.push(ConstExprValue::Numeric(u64::from(value)));
                    continue;
                }
                Some(OpType::F64Const) => {
                    let value = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
                    offset += 8;
                    stack.push(ConstExprValue::Numeric(value));
                    continue;
                }
                Some(OpType::RefNull) => {
                    // Heap type, which only affects the static type of the null.
                    let _ = var_int64!();
                    stack.push(ConstExprValue::Numeric(js_null().encode() as u64));
                    continue;
                }
                Some(OpType::RefFunc) => {
                    let index = var_uint32!();
                    ConstExprValue::Ref(self.host.ref_func(index))
                }
                Some(OpType::GetGlobal) => {
                    let index = var_uint32!();
                    let is_v128 = self.info.globals[index as usize].ty.kind == TypeKind::V128;
                    self.host.load_global(index, is_v128)
                }
                Some(OpType::I32Add | OpType::I64Add) => {
                    let rhs = stack.pop().expect("pilha vazia");
                    let lhs = stack.pop().expect("pilha vazia");
                    ConstExprValue::Numeric(lhs.value().wrapping_add(rhs.value()))
                }
                Some(OpType::I32Sub | OpType::I64Sub) => {
                    let rhs = stack.pop().expect("pilha vazia");
                    let lhs = stack.pop().expect("pilha vazia");
                    ConstExprValue::Numeric(lhs.value().wrapping_sub(rhs.value()))
                }
                Some(OpType::I32Mul | OpType::I64Mul) => {
                    let rhs = stack.pop().expect("pilha vazia");
                    let lhs = stack.pop().expect("pilha vazia");
                    ConstExprValue::Numeric(lhs.value().wrapping_mul(rhs.value()))
                }
                Some(OpType::ExtSIMD) => {
                    let extended = var_uint32!();
                    assert_eq!(extended, EXT_SIMD_V128_CONST);
                    let vector: V128 = bytes[offset..offset + 16].try_into().unwrap();
                    offset += 16;
                    stack.push(ConstExprValue::Vector(vector));
                    continue;
                }
                Some(OpType::ExtGC) => {
                    let extended = var_uint32!();
                    match ExtGCOpType::from_value(extended) {
                        Some(ExtGCOpType::RefI31) => {
                            let value = stack.pop().expect("pilha vazia");
                            let i31 = ((value.value() as i32 & 0x7fff_ffff).wrapping_shl(1)) >> 1;
                            ConstExprValue::Ref(js_number_i32(i31).encode() as u64)
                        }
                        Some(ExtGCOpType::AnyConvertExtern) => {
                            let reference = stack.pop().expect("pilha vazia");
                            // extern.internalize only rewrites doubles that fit an i31, so anything
                            // already known to be an object reference passes through unchanged.
                            match reference {
                                ConstExprValue::Numeric(bits) => ConstExprValue::Ref(self.host.extern_internalize(bits)),
                                other => other,
                            }
                        }
                        // extern.convert_any leaves the operand exactly as it is.
                        Some(ExtGCOpType::ExternConvertAny) => continue,
                        Some(ExtGCOpType::StructNewDefault) => {
                            let type_index = var_uint32!();
                            match self.host.struct_new(type_index, None) {
                                Some(value) => ConstExprValue::Ref(value),
                                None => return Err(self.failed_to_allocate("struct", offset)),
                            }
                        }
                        Some(ExtGCOpType::StructNew) => {
                            let type_index = var_uint32!();
                            let StructuralType::Struct { fields } = &self.info.rtt(type_index as usize).structural
                            else {
                                unreachable!("struct.new de um tipo que não é struct");
                            };
                            let field_count = fields.len();
                            assert!(stack.len() >= field_count);
                            let first = stack.len() - field_count;
                            match self.host.struct_new(type_index, Some(&stack[first..])) {
                                Some(value) => {
                                    stack.truncate(first);
                                    ConstExprValue::Ref(value)
                                }
                                None => return Err(self.failed_to_allocate("struct", offset)),
                            }
                        }
                        Some(ExtGCOpType::ArrayNew) => {
                            let type_index = var_uint32!();
                            // array.new pushes the element value before the size.
                            let size = stack.pop().expect("pilha vazia");
                            let value = stack.pop().expect("pilha vazia");
                            match self.host.array_new(type_index, size.value() as u32, &value) {
                                Some(array) => ConstExprValue::Ref(array),
                                None => return Err(self.failed_to_allocate("array", offset)),
                            }
                        }
                        Some(ExtGCOpType::ArrayNewDefault) => {
                            let type_index = var_uint32!();
                            let size = stack.pop().expect("pilha vazia");
                            let value = self.default_element(type_index);
                            match self.host.array_new(type_index, size.value() as u32, &value) {
                                Some(array) => ConstExprValue::Ref(array),
                                None => return Err(self.failed_to_allocate("array", offset)),
                            }
                        }
                        Some(ExtGCOpType::ArrayNewFixed) => {
                            let type_index = var_uint32!();
                            let element_count = var_uint32!() as usize;
                            assert!(stack.len() >= element_count);
                            let first = stack.len() - element_count;
                            match self.host.array_new_fixed(type_index, &stack[first..]) {
                                Some(array) => {
                                    stack.truncate(first);
                                    ConstExprValue::Ref(array)
                                }
                                None => return Err(self.failed_to_allocate("array", offset)),
                            }
                        }
                        _ => unreachable!("opcode GC fora da lista de expressões constantes"),
                    }
                }
                Some(OpType::End) => {
                    assert_eq!(stack.len(), 1);
                    assert!(!stack[0].is_vector());
                    return Ok(stack[0].value());
                }
                _ => unreachable!("opcode fora da lista de expressões constantes"),
            };
            stack.push(result);
        }
    }
}

/// `evaluateExtendedConstExpr`: `expression` é o par (bytes, offset no módulo) de
/// `ModuleInformation::constantExpressions`.
pub fn evaluate_extended_const_expr<H: ConstExprHost>(
    expression: &(Vec<u8>, usize),
    host: &mut H,
    info: &ModuleInformation,
) -> Result<u64, String> {
    ConstExprInterpreter::new(expression.1, info, host).run(&expression.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::wasm_format::{FieldType, GlobalInformation, Mutability, TYPE_I32, TYPE_I64, TYPE_V128};

    #[derive(Default)]
    struct Host {
        globals: Vec<ConstExprValue>,
        fail_alloc: bool,
        last_struct_fields: Vec<ConstExprValue>,
        last_array: Option<(u32, u32, ConstExprValue)>,
        last_fixed: Vec<ConstExprValue>,
    }

    impl ConstExprHost for Host {
        fn load_global(&mut self, index: u32, _is_v128: bool) -> ConstExprValue {
            self.globals[index as usize]
        }
        fn ref_func(&mut self, function_index_space: u32) -> u64 {
            0x1000 + u64::from(function_index_space)
        }
        fn extern_internalize(&mut self, reference: u64) -> u64 {
            reference | 0x100
        }
        fn struct_new(&mut self, _type_index: u32, fields: Option<&[ConstExprValue]>) -> Option<u64> {
            if self.fail_alloc {
                return None;
            }
            self.last_struct_fields = fields.map(|f| f.to_vec()).unwrap_or_default();
            Some(0x2000)
        }
        fn array_new(&mut self, type_index: u32, size: u32, value: &ConstExprValue) -> Option<u64> {
            if self.fail_alloc {
                return None;
            }
            self.last_array = Some((type_index, size, *value));
            Some(0x3000)
        }
        fn array_new_fixed(&mut self, _type_index: u32, elements: &[ConstExprValue]) -> Option<u64> {
            if self.fail_alloc {
                return None;
            }
            self.last_fixed = elements.to_vec();
            Some(0x4000)
        }
    }

    fn info() -> ModuleInformation {
        let mut info = ModuleInformation::default();
        info.globals.push(GlobalInformation::new(TYPE_I32, Mutability::Immutable));
        info.globals.push(GlobalInformation::new(TYPE_V128, Mutability::Immutable));
        info.append_recursion_group(vec![(
            StructuralType::Struct {
                fields: vec![
                    FieldType { ty: StorageType::Type(TYPE_I32), mutability: Mutability::Immutable },
                    FieldType { ty: StorageType::Type(TYPE_I64), mutability: Mutability::Immutable },
                ],
            },
            None,
        )]);
        info.append_recursion_group(vec![(
            StructuralType::Array { element: FieldType { ty: StorageType::Type(TYPE_V128), mutability: Mutability::Mutable } },
            None,
        )]);
        info.append_recursion_group(vec![(
            StructuralType::Array { element: FieldType { ty: StorageType::Type(TYPE_I32), mutability: Mutability::Mutable } },
            None,
        )]);
        info
    }

    fn run(bytes: &[u8], host: &mut Host) -> Result<u64, String> {
        evaluate_extended_const_expr(&(bytes.to_vec(), 40), host, &info())
    }

    #[test]
    fn constants_and_negative_leb() {
        let mut host = Host::default();
        // i32.const -1; end: o i32 com sinal estendido.
        assert_eq!(run(&[0x41, 0x7f, 0x0b], &mut host), Ok(u64::MAX));
        // i64.const 300; end
        assert_eq!(run(&[0x42, 0xac, 0x02, 0x0b], &mut host), Ok(300));
        // f32.const 1.0; end
        assert_eq!(run(&[0x43, 0x00, 0x00, 0x80, 0x3f, 0x0b], &mut host), Ok(0x3f80_0000));
    }

    #[test]
    fn arithmetic_wraps() {
        let mut host = Host::default();
        // i64.const 6; i64.const 7; i64.mul; i64.const 2; i64.sub; end
        assert_eq!(run(&[0x42, 0x06, 0x42, 0x07, 0x7e, 0x42, 0x02, 0x7d, 0x0b], &mut host), Ok(40));
        // i64.const 0; i64.const 1; i64.sub; end
        assert_eq!(run(&[0x42, 0x00, 0x42, 0x01, 0x7d, 0x0b], &mut host), Ok(u64::MAX));
    }

    #[test]
    fn globals_and_ref_func() {
        let mut host = Host { globals: vec![ConstExprValue::Numeric(9), ConstExprValue::Vector([1; 16])], ..Host::default() };
        // global.get 0; i32.const 1; i32.add; end
        assert_eq!(run(&[0x23, 0x00, 0x41, 0x01, 0x6a, 0x0b], &mut host), Ok(10));
        assert_eq!(run(&[0xd2, 0x05, 0x0b], &mut host), Ok(0x1005));
    }

    #[test]
    fn ref_null_and_i31() {
        let mut host = Host::default();
        // ref.null func (heap type 0x70 = -16 em sleb); end
        assert_eq!(run(&[0xd0, 0x70, 0x0b], &mut host), Ok(js_null().encode() as u64));
        // i32.const -1; ref.i31; end: -1 cabe em 31 bits com sinal.
        assert_eq!(run(&[0x41, 0x7f, 0xfb, 0x1c, 0x0b], &mut host), Ok(js_number_i32(-1).encode() as u64));
        // i32.const 0x40000000; ref.i31; end: o bit 30 vira o sinal.
        assert_eq!(
            run(&[0x41, 0x80, 0x80, 0x80, 0x80, 0x04, 0xfb, 0x1c, 0x0b], &mut host),
            Ok(js_number_i32(-0x4000_0000).encode() as u64)
        );
    }

    #[test]
    fn extern_conversions() {
        let mut host = Host::default();
        // i32.const 5; any.convert_extern; end: o número passa pelo `externInternalize` do host.
        assert_eq!(run(&[0x41, 0x05, 0xfb, 0x1a, 0x0b], &mut host), Ok(5 | 0x100));
        // ref.func 1; any.convert_extern; end: a referência já é objeto, passa direto.
        assert_eq!(run(&[0xd2, 0x01, 0xfb, 0x1a, 0x0b], &mut host), Ok(0x1001));
        // i32.const 5; extern.convert_any; end: o operando fica como está.
        assert_eq!(run(&[0x41, 0x05, 0xfb, 0x1b, 0x0b], &mut host), Ok(5));
    }

    #[test]
    fn struct_and_array_allocation() {
        let mut host = Host::default();
        // i32.const 3; i64.const 4; struct.new 0; end
        assert_eq!(run(&[0x41, 0x03, 0x42, 0x04, 0xfb, 0x00, 0x00, 0x0b], &mut host), Ok(0x2000));
        assert_eq!(host.last_struct_fields, vec![ConstExprValue::Numeric(3), ConstExprValue::Numeric(4)]);
        // struct.new_default 0; end
        assert_eq!(run(&[0xfb, 0x01, 0x00, 0x0b], &mut host), Ok(0x2000));
        assert!(host.last_struct_fields.is_empty());
        // i32.const 7; i32.const 2; array.new 2; end: valor antes do tamanho.
        assert_eq!(run(&[0x41, 0x07, 0x41, 0x02, 0xfb, 0x06, 0x02, 0x0b], &mut host), Ok(0x3000));
        assert_eq!(host.last_array, Some((2, 2, ConstExprValue::Numeric(7))));
        // i32.const 3; array.new_default 1; end: v128 enche com zeros.
        assert_eq!(run(&[0x41, 0x03, 0xfb, 0x07, 0x01, 0x0b], &mut host), Ok(0x3000));
        assert_eq!(host.last_array, Some((1, 3, ConstExprValue::Vector([0; 16]))));
        // i32.const 1; i32.const 2; array.new_fixed 2 2; end
        assert_eq!(run(&[0x41, 0x01, 0x41, 0x02, 0xfb, 0x08, 0x02, 0x02, 0x0b], &mut host), Ok(0x4000));
        assert_eq!(host.last_fixed, vec![ConstExprValue::Numeric(1), ConstExprValue::Numeric(2)]);
    }

    #[test]
    fn allocation_failure_names_the_byte_after_the_immediates() {
        let mut host = Host { fail_alloc: true, ..Host::default() };
        // struct.new_default 0: opcode em 0, imediato em 3, offset 40 + 4.
        assert_eq!(
            run(&[0xfb, 0x01, 0x00, 0x0b], &mut host),
            Err("WebAssembly.Module doesn't parse at byte 43: Failed to allocate new struct".to_string())
        );
        // i32.const 3; array.new_default 2; end
        assert_eq!(
            run(&[0x41, 0x03, 0xfb, 0x07, 0x02, 0x0b], &mut host),
            Err("WebAssembly.Module doesn't parse at byte 45: Failed to allocate new array".to_string())
        );
    }

    #[test]
    fn v128_constant_feeds_array_new_fixed() {
        let mut host = Host::default();
        // v128.const 7..; array.new_fixed 1 1; end
        let mut bytes = vec![0xfd, 0x0c];
        bytes.extend_from_slice(&[7; 16]);
        bytes.extend_from_slice(&[0xfb, 0x08, 0x01, 0x01, 0x0b]);
        assert_eq!(run(&bytes, &mut host), Ok(0x4000));
        assert_eq!(host.last_fixed, vec![ConstExprValue::Vector([7; 16])]);
    }
}
