// Sexta fatia do porte de `JSBigInt.cpp`: as linhas 431 a 470 (`parseInt` sobre `StringView`,
// `stringToBigInt`, `toString`, `tryGetString`) e as linhas 6948 a 7213 (`parseInt` sobre span com
// `CharType` genérico), mais o que esse `parseInt` chama e ainda não existia: `multiplyAdd` (5517),
// `copyZeroPadded` (3165), a tabela `maxBitsPerCharTable` (6128), `charValueTable` e
// `digitCharValue` (6662), `fromStringLargeThreshold`, `charactersPerDigitTable` (6694 a 6726),
// `fromStringLarge` (6770), `parseDigitsLarge` (6888) e `parseDigitsPowerOfTwo` (6920).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Dependências de fatias futuras (ainda inexistentes): `JSBigInt::to_string_base_power_of_two` e
// `JSBigInt::to_string_generic` (linhas 6167 a 6660), chamadas por `to_string` e `try_get_string`
// com a assinatura `(vm: &VM, global_object: Option<&dyn BigIntGlobalObject>, big_int: &JSBigInt,
// radix: u32) -> String`. Quem portar `calculateMaximumCharactersRequired` (6143) e as demais usuárias
// de `maxBitsPerCharTable` reaproveita `MAX_BITS_PER_CHAR_TABLE`, `BITS_PER_CHAR_TABLE_SHIFT` e
// `BITS_PER_CHAR_TABLE_MULTIPLIER` daqui; `copy_zero_padded` também serve às demais usuárias.
//
// Desvios mecânicos de Rust seguro, sem efeito observável:
// - `JSGlobalObject` ainda não existe. O `JSGlobalObject*` do C++ vira `&dyn BigIntGlobalObject`
//   (`Option<..>` onde o C++ aceita nulo), com só o que o `JSBigInt.cpp` pede dele nesta fatia:
//   `vm()`, `throwVMError(createSyntaxError(..))` e `throwOutOfMemoryError(..)`.
// - O `JSValue` de retorno vira `ImplResult` (`Empty` é o `JSValue()`). Com `USE(BIGINT32)` igual a
//   0 (`PlatformUse.h`), só aparecem `Empty` e `Heap`.
// - O `InterruptCheck` recebe `vm->hasExceptionsAfterHandlingTraps()` como `vm.exception().is_some()`
//   (o `VMTraps` ainda não existe) e só quando há objeto global, como o `m_vm` nulo do C++.
// - O `StringView` do `parseInt` com radix chega como `&AtomString` (o chamador, o `ParserArena`,
//   passa `identifier.string()`); a conversão para `StringView` é feita na entrada.
// - `fromStringLarge` recebe `parts` por valor: os três buffers do C++ (`parts`, `multipliers`,
//   `temp`) giram de papel a cada iteração, e a rotação vira troca de `Vec`.
// - `Checked<uint64_t, CrashOnOverflow>` vira `checked_mul`/`checked_add` com `expect`, pois o
//   limite de caracteres por bloco garante que não estoura.

use crate::runtime::parse_int::is_str_white_space;
use crate::wtf::ascii_ctype::{is_ascii_alpha_caseless_equal, AsciiChar};
use crate::wtf::text::atom_string::AtomString;
use crate::wtf::text::string_impl::CharType;
use crate::wtf::text::string_view::StringView;

/// O que o `JSBigInt.cpp` usa do `JSGlobalObject`: o `VM` e os `throw*` com a mensagem exata.
pub trait BigIntGlobalObject {
    /// `globalObject->vm()`
    fn vm(&self) -> &crate::runtime::vm::VM;

    /// `throwVMError(globalObject, scope, createSyntaxError(globalObject, message))`
    fn throw_syntax_error(&self, vm: &crate::runtime::vm::VM, message: &str);

    /// `throwOutOfMemoryError(globalObject, scope)` (sem mensagem, `None`) e
    /// `throwOutOfMemoryError(globalObject, scope, message)`.
    fn throw_out_of_memory_error(&self, vm: &crate::runtime::vm::VM, message: Option<&str>);
}

impl ImplResult {
    /// `JSValue::operator bool` negado: `JSValue()` é a falha (exceção pendente ou `nullptr`).
    pub fn is_empty(&self) -> bool {
        matches!(self, ImplResult::Empty)
    }

    /// Ponte para o `JSValue` que o C++ devolve de `ImplResult` (`JSValue()`, `JSBigInt*` ou BigInt32):
    /// o BigInt do heap vira célula do registro central (`CellEntry::BigInt`). Com `USE(BIGINT32)`
    /// igual a 0 o `BigInt32` não existe como valor; o resíduo (`zero_impl`, conversões) vira célula.
    pub fn into_js_value(self, vm: &crate::runtime::vm::VM) -> crate::runtime::js_value::JSValue {
        use crate::runtime::cell_registry::{insert, CellEntry};
        use crate::runtime::js_value::JSValue;
        let big_int = match self {
            ImplResult::Empty => return JSValue::empty(),
            ImplResult::Heap(big_int) => big_int,
            ImplResult::BigInt32(value) => {
                JSBigInt::create_from_i32(value).expect("BigInt de um dígito sempre cabe")
            }
        };
        JSValue::from_cell(insert(CellEntry::BigInt(std::rc::Rc::new(big_int.with_structure(vm)))))
    }

    /// `JSValue::asHeapBigInt()`. Só vale para `Heap` (com `USE(BIGINT32)` igual a 0 é o único caso
    /// não vazio).
    pub fn as_heap_big_int(&self) -> &JSBigInt {
        match self {
            ImplResult::Heap(big_int) => big_int,
            _ => unreachable!("as_heap_big_int sobre valor que não é um BigInt do heap"),
        }
    }
}

/// `maxBitsPerCharTable`: o máximo de bits por caractere de uma representação em base N, vezes 32.
pub const MAX_BITS_PER_CHAR_TABLE: [u8; 37] = [
    0, 0, 32, 51, 64, 75, 83, 90, 96, // 0..8
    102, 107, 111, 115, 119, 122, 126, 128, // 9..16
    131, 134, 136, 139, 141, 143, 145, 147, // 17..24
    149, 151, 153, 154, 156, 158, 159, 160, // 25..32
    162, 163, 165, 166, // 33..36
];

pub const BITS_PER_CHAR_TABLE_SHIFT: u32 = 5;
pub const BITS_PER_CHAR_TABLE_MULTIPLIER: usize = 1usize << BITS_PER_CHAR_TABLE_SHIFT;

/// `charValueTable`: o valor numérico dos 128 primeiros caracteres ASCII, 255 para "inválido".
const CHAR_VALUE_TABLE: [u8; 128] = {
    let mut table = [255u8; 128];
    let mut c = 0usize;
    while c < 128 {
        table[c] = match c as u8 {
            b'0'..=b'9' => c as u8 - b'0',
            b'A'..=b'Z' => c as u8 - b'A' + 10,
            b'a'..=b'z' => c as u8 - b'a' + 10,
            _ => 255,
        };
        c += 1;
    }
    table
};

/// `digitCharValue`
#[inline(always)]
fn digit_char_value<C: CharType>(character: C) -> u32 {
    let character: u32 = character.into();
    if character as usize >= CHAR_VALUE_TABLE.len() {
        return 255;
    }
    CHAR_VALUE_TABLE[character as usize] as u32
}

/// `fromStringLargeThreshold` com `CPU(REGISTER64)`.
pub const FROM_STRING_LARGE_THRESHOLD: usize = 4;
const _: () = assert!(FROM_STRING_LARGE_THRESHOLD >= 3);

/// `CharactersPerDigit`: quantos caracteres da base cabem num dígito, e essa potência da base.
#[derive(Clone, Copy)]
pub struct CharactersPerDigit {
    pub count: u32,
    pub multiplier: Digit,
}

/// `computeCharactersPerDigit`
const fn compute_characters_per_digit(radix: u32) -> CharactersPerDigit {
    let mut multiplier = radix as Digit;
    let mut count = 1u32;
    while multiplier <= Digit::MAX / radix as Digit {
        multiplier *= radix as Digit;
        count += 1;
    }
    CharactersPerDigit { count, multiplier }
}

/// `charactersPerDigitTable`
pub const CHARACTERS_PER_DIGIT_TABLE: [CharactersPerDigit; 37] = {
    let mut table = [CharactersPerDigit { count: 0, multiplier: 0 }; 37];
    let mut radix = 2u32;
    while radix <= 36 {
        table[radix as usize] = compute_characters_per_digit(radix);
        radix += 1;
    }
    table
};

/// `copyZeroPadded`: Z := X, preenchendo Z com zeros. Só os dígitos de X que cabem são lidos.
fn copy_zero_padded(z: &mut [Digit], x: &[Digit]) {
    let count = x.len().min(z.len());
    z[..count].copy_from_slice(&x[..count]);
    z[count..].fill(0);
}

impl JSBigInt {
    /// `JSBigInt::parseInt(JSGlobalObject*, StringView, ErrorParseMode)`
    pub fn parse_int_string_view(global_object: &dyn BigIntGlobalObject, s: StringView, parser_mode: ErrorParseMode) -> ImplResult {
        if s.is_8bit() {
            return Self::parse_int_span(global_object, s.span8(), parser_mode);
        }
        Self::parse_int_span(global_object, s.span16(), parser_mode)
    }

    /// `JSBigInt::parseInt(JSGlobalObject* nullOrGlobalObjectForOOM, VM&, StringView, uint8_t radix,
    /// ErrorParseMode, ParseIntSign)`
    pub fn parse_int(
        null_or_global_object_for_oom: Option<&dyn BigIntGlobalObject>,
        vm: &crate::runtime::vm::VM,
        s: &AtomString,
        radix: u8,
        parser_mode: ErrorParseMode,
        sign: ParseIntSign,
    ) -> ImplResult {
        let s = StringView::from(s);
        if s.is_8bit() {
            return Self::parse_int_span_with_radix(
                null_or_global_object_for_oom,
                vm,
                s.span8(),
                0,
                radix as u32,
                parser_mode,
                sign,
                ParseIntMode::DisallowEmptyString,
            );
        }
        Self::parse_int_span_with_radix(
            null_or_global_object_for_oom,
            vm,
            s.span16(),
            0,
            radix as u32,
            parser_mode,
            sign,
            ParseIntMode::DisallowEmptyString,
        )
    }

    /// `JSBigInt::stringToBigInt`
    pub fn string_to_big_int(global_object: &dyn BigIntGlobalObject, s: StringView) -> ImplResult {
        Self::parse_int_string_view(global_object, s, ErrorParseMode::IgnoreExceptions)
    }

    /// `JSBigInt::toString(JSGlobalObject*, unsigned radix)`
    pub fn to_string(&self, global_object: &dyn BigIntGlobalObject, radix: u32) -> crate::wtf::text::wtf_string::String {
        if self.is_zero() {
            return crate::wtf::text::wtf_string::String::from_latin1(b"0");
        }

        if crate::wtf::math_extras::has_one_bit_set(radix) {
            return Self::to_string_base_power_of_two(global_object.vm(), Some(global_object), self, radix);
        }

        Self::to_string_generic(global_object.vm(), Some(global_object), self, radix)
    }

    /// `JSBigInt::tryGetString`
    pub fn try_get_string(vm: &crate::runtime::vm::VM, big_int: &JSBigInt, radix: u32) -> crate::wtf::text::wtf_string::String {
        if big_int.is_zero() {
            return crate::wtf::text::wtf_string::String::from_latin1(b"0");
        }

        if crate::wtf::math_extras::has_one_bit_set(radix) {
            return Self::to_string_base_power_of_two(vm, None, big_int, radix);
        }

        Self::to_string_generic(vm, None, big_int, radix)
    }

    /// O trio `throwVMError(.., createSyntaxError(.., "Failed to parse String to BigInt"_s))`, só
    /// com `ErrorParseMode::ThrowExceptions`.
    fn throw_parse_failure(null_or_global_object_for_oom: Option<&dyn BigIntGlobalObject>, vm: &crate::runtime::vm::VM, error_parse_mode: ErrorParseMode) {
        if error_parse_mode == ErrorParseMode::ThrowExceptions {
            debug_assert!(null_or_global_object_for_oom.is_some());
            if let Some(global_object) = null_or_global_object_for_oom {
                global_object.throw_syntax_error(vm, "Failed to parse String to BigInt");
            }
        }
    }

    /// Os `throwOutOfMemoryError` de `createWithLength`, só quando há objeto global.
    fn throw_big_int_error(null_or_global_object_for_oom: Option<&dyn BigIntGlobalObject>, vm: &crate::runtime::vm::VM, error: &BigIntError) {
        let Some(global_object) = null_or_global_object_for_oom else {
            return;
        };
        match error {
            BigIntError::OutOfMemory => global_object.throw_out_of_memory_error(vm, None),
            other => global_object.throw_out_of_memory_error(vm, Some(other.message())),
        }
    }

    /// `JSBigInt::tryCreateFromImpl(JSGlobalObject*, VM&, bool sign, std::span<const Digit>)`, com a
    /// falha lançada quando há objeto global e `ImplResult::Empty` no lugar do `nullptr`.
    fn try_create_from_impl_or_throw(
        null_or_global_object_for_oom: Option<&dyn BigIntGlobalObject>,
        vm: &crate::runtime::vm::VM,
        sign: bool,
        digits: &[Digit],
    ) -> ImplResult {
        match Self::try_create_from_impl(sign, digits) {
            Ok(big_int) => ImplResult::Heap(big_int),
            Err(error) => {
                Self::throw_big_int_error(null_or_global_object_for_oom, vm, &error);
                ImplResult::Empty
            }
        }
    }

    /// `JSBigInt::multiplyAdd`: multiplica {source} por {factor} e soma {summand}. {result} e
    /// {source} podem ser o mesmo BigInt (`Operand::in_place`) para modificação no lugar.
    fn multiply_add(source: Operand, factor: Digit, summand: Digit, result: &mut [Digit]) {
        assert!(result.len() >= source.len);

        let mut carry = summand;
        let mut high: Digit = 0;
        let mut i = 0;
        while i < source.len {
            // Compute this round's multiplication.
            let (mut current, new_high) = digit_mul(source.get(result, i), factor);

            // Add last round's carryovers.
            let mut new_carry: Digit = 0;
            current = digit_add(current, high, &mut new_carry);
            current = digit_add(current, carry, &mut new_carry);

            // Store result and prepare for next round.
            result[i] = current;
            carry = new_carry;
            high = new_high;
            i += 1;
        }

        if result.len() > i {
            result[i] = carry.wrapping_add(high);
            i += 1;

            // Current callers don't pass in such large results, but let's be robust.
            while i < result.len() {
                result[i] = 0;
                i += 1;
            }
        } else {
            debug_assert!(carry.wrapping_add(high) == 0);
        }
    }

    /// `JSBigInt::fromStringLarge`: combina as partes em ordem de árvore binária balanceada, portada
    /// do V8. `parts` entra por valor (ver o cabeçalho): é o `partsStorage` do chamador.
    pub fn from_string_large(interrupt: &mut InterruptCheck, z: &mut [Digit], mut parts: Vec<Digit>, max_multiplier: Digit, last_multiplier: Digit) {
        let mut num_parts = parts.len();
        // The first round below never writes to z, and the loop after it only runs once there are at
        // least two parts left, so two parts would leave z untouched.
        debug_assert!(num_parts >= 3);
        debug_assert!(z.len() >= num_parts);
        let mut multipliers: Vec<Digit> = vec![0; num_parts];
        let mut temp: Vec<Digit> = vec![0; num_parts];
        // Unrolled and specialized first iteration: partLength == 1, so instead of digit sub-vectors
        // we have individual digit values, and the multipliers are known up front. Aqui
        // `newParts` é `temp` e `newMultipliers` é `parts`.
        {
            let mut i = 0;
            while i + 1 < num_parts {
                let p_in = parts[i];
                let p_in2 = parts[i + 1];
                let m_in = max_multiplier;
                let m_in2 = if i == num_parts - 2 { last_multiplier } else { max_multiplier };
                // p[j] = p[i] * m[i+1] + p[i+1]
                let (p_low, p_high) = digit_mul(p_in, m_in2);
                let mut carry: Digit = 0;
                temp[i] = digit_add(p_low, p_in2, &mut carry);
                temp[i + 1] = p_high.wrapping_add(carry);
                // m[j] = m[i] * m[i+1]
                if i > 0 {
                    if i > 2 && m_in2 != last_multiplier {
                        parts[i] = parts[i - 2];
                        parts[i + 1] = parts[i - 1];
                    } else {
                        let (m_low, m_high) = digit_mul(m_in, m_in2);
                        parts[i] = m_low;
                        parts[i + 1] = m_high;
                    }
                }
                i += 2;
            }
            // Trailing last part (if {numParts} was odd).
            if i < num_parts {
                temp[i] = parts[i];
                parts[i] = last_multiplier;
                i += 2;
            }
            num_parts = i >> 1;
            // newTemp = multipliers; parts = newParts; multipliers = newMultipliers; temp = newTemp.
            core::mem::swap(&mut parts, &mut temp);
            core::mem::swap(&mut temp, &mut multipliers);
        }
        let mut part_length: usize = 2;

        // Remaining iterations.
        while num_parts > 1 {
            // In the very last iteration, write into {z}.
            let last_round = num_parts == 2;
            let new_part_length = part_length * 2;
            let mut i = 0;
            {
                let new_parts: &mut [Digit] = if last_round { &mut *z } else { &mut temp[..] };
                // `newMultipliers` é o buffer `parts`.
                while i + 1 < num_parts {
                    let start = i * part_length;
                    let p_in = clamped_subspan(&parts, start, part_length);
                    let p_in2 = clamped_subspan(&parts, start + part_length, part_length);
                    let m_in = clamped_subspan(&multipliers, start, part_length);
                    let m_in2 = clamped_subspan(&multipliers, start + part_length, part_length);
                    let p_out = clamped_subspan_mut(&mut *new_parts, start, new_part_length);
                    // p[j] = p[i] * m[i+1] + p[i+1]
                    Self::multiply_zero_padded(interrupt, p_out, p_in, m_in2);
                    if interrupt.interrupted() {
                        return;
                    }
                    let overflow = Self::inplace_add_and_propagate(p_out, p_in2);
                    debug_assert!(overflow == 0);
                    // m[j] = m[i] * m[i+1]
                    if i > 0 {
                        let mut copied = false;
                        if i > 2 {
                            let previous_start = (i - 2) * part_length;
                            let m_in_previous = clamped_subspan(&multipliers, previous_start, part_length);
                            let m_in2_previous = clamped_subspan(&multipliers, previous_start + part_length, part_length);
                            if Self::compare_digits(m_in, m_in_previous) == ComparisonResult::Equal
                                && Self::compare_digits(m_in2, m_in2_previous) == ComparisonResult::Equal
                            {
                                copied = true;
                                let m_out_length = clamped_subspan(&parts, start, new_part_length).len();
                                if m_out_length > 0 {
                                    parts.copy_within(previous_start..previous_start + m_out_length, start);
                                }
                            }
                        }
                        if !copied {
                            let m_out = clamped_subspan_mut(&mut parts, start, new_part_length);
                            Self::multiply_zero_padded(interrupt, m_out, m_in, m_in2);
                            if interrupt.interrupted() {
                                return;
                            }
                        }
                    }
                    i += 2;
                }
                // Trailing last part (if {numParts} was odd).
                if i < num_parts {
                    let p_in = clamped_subspan(&parts, i * part_length, part_length);
                    let m_in = clamped_subspan(&multipliers, i * part_length, part_length);
                    let p_out = clamped_subspan_mut(&mut *new_parts, i * part_length, new_part_length);
                    copy_zero_padded(p_out, p_in);
                    let m_out = clamped_subspan_mut(&mut parts, i * part_length, new_part_length);
                    copy_zero_padded(m_out, m_in);
                    i += 2;
                }
            }
            num_parts = i >> 1;
            part_length = new_part_length;
            // newTemp = multipliers; parts = newParts; multipliers = newMultipliers; temp = newTemp.
            core::mem::swap(&mut parts, &mut temp);
            core::mem::swap(&mut temp, &mut multipliers);
        }
        // z might be bigger than we requested; be robust towards that.
        let z_length = z.len();
        z[part_length.min(z_length)..].fill(0);
    }

    /// `JSBigInt::parseDigitsLarge`: os dígitos de uma string numa base que não é potência de dois,
    /// dado quantos caracteres cabem num dígito. Devolve falso diante de um caractere inválido.
    pub fn parse_digits_large<C: CharType>(
        interrupt: &mut InterruptCheck,
        result: &mut [Digit],
        characters: &[C],
        radix: u32,
        chars_per_part: u32,
        max_multiplier: Digit,
    ) -> bool {
        let chars_per_part = chars_per_part as usize;
        let num_parts = (characters.len() + chars_per_part - 1) / chars_per_part;
        debug_assert!(result.len() >= num_parts);
        debug_assert!(num_parts >= 3);
        let mut parts: Vec<Digit> = vec![0; num_parts];
        let mut position = 0usize;
        let mut last_multiplier = max_multiplier;
        for part_slot in parts.iter_mut() {
            let count = chars_per_part.min(characters.len() - position);
            let mut part: Digit = 0;
            let mut multiplier: Digit = 1;
            for j in 0..count {
                let value = digit_char_value(characters[position + j]);
                if value >= radix {
                    return false;
                }
                part = part.wrapping_mul(radix as Digit).wrapping_add(value as Digit);
                multiplier = multiplier.wrapping_mul(radix as Digit);
            }
            *part_slot = part;
            last_multiplier = multiplier;
            position += count;
        }
        Self::from_string_large(interrupt, result, parts, max_multiplier, last_multiplier);
        true
    }

    /// `JSBigInt::parseDigitsPowerOfTwo`: cada caractere contribui exatamente `ctz(radix)` bits, que
    /// são empacotados do caractere menos significativo para cima. Devolve falso diante de um
    /// caractere inválido.
    pub fn parse_digits_power_of_two<C: CharType>(result: &mut [Digit], characters: &[C], radix: u32) -> bool {
        debug_assert!(crate::wtf::math_extras::has_one_bit_set(radix));
        let bits_per_char = radix.trailing_zeros();
        debug_assert!(result.len() * DIGIT_BITS as usize >= characters.len() * bits_per_char as usize);
        let mut digit_index = 0usize;
        let mut digit: Digit = 0;
        let mut bits_in_digit: u32 = 0;
        for i in (0..characters.len()).rev() {
            let value = digit_char_value(characters[i]);
            if value >= radix {
                return false;
            }
            digit |= (value as Digit) << bits_in_digit;
            bits_in_digit += bits_per_char;
            if bits_in_digit >= DIGIT_BITS {
                result[digit_index] = digit;
                digit_index += 1;
                bits_in_digit -= DIGIT_BITS;
                // The bits of this character that did not fit, if any.
                digit = if bits_in_digit != 0 { (value as Digit) >> (bits_per_char - bits_in_digit) } else { 0 };
            }
        }
        if bits_in_digit != 0 {
            result[digit_index] = digit;
            digit_index += 1;
        }
        result[digit_index..].fill(0);
        true
    }

    /// `JSBigInt::parseInt<CharType>(JSGlobalObject*, std::span<const CharType>, ErrorParseMode)`
    pub fn parse_int_span<C: CharType + AsciiChar>(global_object: &dyn BigIntGlobalObject, data: &[C], error_parse_mode: ErrorParseMode) -> ImplResult {
        let vm = global_object.vm();
        let unit = |index: usize| -> u32 { data[index].into() };

        let mut p = 0usize;
        while p < data.len() && is_str_white_space(data[p]) {
            p += 1;
        }

        // Check Radix from first characters
        if p + 1 < data.len() && unit(p) == '0' as u32 {
            if is_ascii_alpha_caseless_equal(data[p + 1], b'b') {
                return Self::parse_int_span_with_radix(
                    Some(global_object),
                    vm,
                    data,
                    (p + 2) as u32,
                    2,
                    error_parse_mode,
                    ParseIntSign::Unsigned,
                    ParseIntMode::DisallowEmptyString,
                );
            }

            if is_ascii_alpha_caseless_equal(data[p + 1], b'x') {
                return Self::parse_int_span_with_radix(
                    Some(global_object),
                    vm,
                    data,
                    (p + 2) as u32,
                    16,
                    error_parse_mode,
                    ParseIntSign::Unsigned,
                    ParseIntMode::DisallowEmptyString,
                );
            }

            if is_ascii_alpha_caseless_equal(data[p + 1], b'o') {
                return Self::parse_int_span_with_radix(
                    Some(global_object),
                    vm,
                    data,
                    (p + 2) as u32,
                    8,
                    error_parse_mode,
                    ParseIntSign::Unsigned,
                    ParseIntMode::DisallowEmptyString,
                );
            }
        }

        let mut sign = ParseIntSign::Unsigned;
        if p < data.len() {
            if unit(p) == '-' as u32 {
                sign = ParseIntSign::Signed;
                p += 1;
            } else if unit(p) == '+' as u32 {
                p += 1;
            }
        }

        // Os argumentos padrão do C++ (`ParseIntMode::AllowEmptyString`) valem aqui.
        Self::parse_int_span_with_radix(Some(global_object), vm, data, p as u32, 10, error_parse_mode, sign, ParseIntMode::AllowEmptyString)
    }

    /// `JSBigInt::parseInt<CharType>(JSGlobalObject* nullOrGlobalObjectForOOM, VM&, std::span<const
    /// CharType>, unsigned startIndex, unsigned radix, ErrorParseMode, ParseIntSign, ParseIntMode)`
    pub fn parse_int_span_with_radix<C: CharType>(
        null_or_global_object_for_oom: Option<&dyn BigIntGlobalObject>,
        vm: &crate::runtime::vm::VM,
        data: &[C],
        start_index: u32,
        radix: u32,
        error_parse_mode: ErrorParseMode,
        sign: ParseIntSign,
        parse_mode: ParseIntMode,
    ) -> ImplResult {
        let unit = |index: usize| -> u32 { data[index].into() };
        let mut p = start_index as usize;

        if parse_mode != ParseIntMode::AllowEmptyString && start_index as usize == data.len() {
            debug_assert!(null_or_global_object_for_oom.is_some());
            Self::throw_parse_failure(null_or_global_object_for_oom, vm, error_parse_mode);
            return ImplResult::Empty;
        }

        // Skipping leading zeros
        while p < data.len() && unit(p) == '0' as u32 {
            p += 1;
        }

        let mut end_index: i64 = data.len() as i64 - 1;
        // Removing trailing spaces
        while end_index >= p as i64 && is_str_white_space(data[end_index as usize]) {
            end_index -= 1;
        }

        let length = (end_index + 1) as usize;

        if p == length {
            // `USE(BIGINT32)` é 0: `createZero(vm)`.
            return ImplResult::Heap(JSBigInt::default());
        }

        // The idea is to pick the largest limit such that:
        // radix ** lengthLimitForBigInt32 <= INT32_MAX
        let length_limit_for_big_int32: u32 = match radix {
            2 => 30,
            8 => 10,
            10 => 9,
            16 => 7,
            _ => 1,
        };

        fn compute_length(radix: u32, charcount: u32) -> Option<u32> {
            debug_assert!((2..=36).contains(&radix));

            let bits_per_char = MAX_BITS_PER_CHAR_TABLE[radix as usize] as usize;
            let chars = charcount as usize;
            let roundup = BITS_PER_CHAR_TABLE_MULTIPLIER - 1;
            if chars <= (usize::MAX - roundup) / bits_per_char {
                let mut bits_min = bits_per_char * chars;

                // Divide by 32 (see table), rounding up.
                bits_min = (bits_min + roundup) >> BITS_PER_CHAR_TABLE_SHIFT;
                if bits_min <= MAX_INT as usize {
                    // Divide by kDigitsBits, rounding up.
                    let length = (bits_min + DIGIT_BITS as usize - 1) / DIGIT_BITS as usize;
                    if length <= MAX_LENGTH as usize {
                        return Some(length as u32);
                    }
                }
            }

            None
        }

        let initial_length = (length - p) as u32;

        // Inputs too long for the multiplyAdd loop below to stay cheap are parsed in one of the
        // linear-time ways: packing bits for a power-of-two radix, or combining digit-sized parts
        // in a balanced tree otherwise. Inputs that may fit a BigInt32 keep the loop. The comparisons
        // spell out ceil(initialLength / charsPerPart) >= fromStringLargeThreshold without the
        // division, since they run on every parse; the length check in front of the table load is
        // implied by the part count (over 30 characters for every radix) and only short-circuits it.
        {
            let is_power_of_two_radix = crate::wtf::math_extras::has_one_bit_set(radix);
            let use_linear_parse = if is_power_of_two_radix {
                initial_length > length_limit_for_big_int32
            } else {
                initial_length > 30
                    && initial_length as usize > (FROM_STRING_LARGE_THRESHOLD - 1) * CHARACTERS_PER_DIGIT_TABLE[radix as usize].count as usize
            };
            if use_linear_parse {
                let CharactersPerDigit { count: chars_per_part, multiplier: max_multiplier } = CHARACTERS_PER_DIGIT_TABLE[radix as usize];
                let num_parts = (initial_length as usize + chars_per_part as usize - 1) / chars_per_part as usize;
                let characters = &data[p..p + initial_length as usize];
                let Some(result_length) = compute_length(radix, initial_length) else {
                    Self::throw_big_int_error(null_or_global_object_for_oom, vm, &BigIntError::TooBig);
                    return ImplResult::Empty;
                };
                // The parts can outnumber the digits of the result by one: the last part is short,
                // and the bit estimate above is tighter than a digit per part.
                let mut result_vector: Vec<Digit> = match try_zeroed_digits((result_length as usize).max(num_parts)) {
                    Ok(vector) => vector,
                    Err(error) => {
                        Self::throw_big_int_error(null_or_global_object_for_oom, vm, &error);
                        return ImplResult::Empty;
                    }
                };
                let mut vm_check = || vm.exception().is_some();
                let mut interrupt = InterruptCheck::new(if null_or_global_object_for_oom.is_some() {
                    Some(&mut vm_check as &mut dyn FnMut() -> bool)
                } else {
                    None
                });
                let valid = if is_power_of_two_radix {
                    Self::parse_digits_power_of_two(&mut result_vector, characters, radix)
                } else {
                    Self::parse_digits_large(&mut interrupt, &mut result_vector, characters, radix, chars_per_part, max_multiplier)
                };
                if interrupt.interrupted() {
                    return ImplResult::Empty;
                }
                if !valid {
                    Self::throw_parse_failure(null_or_global_object_for_oom, vm, error_parse_mode);
                    return ImplResult::Empty;
                }
                return Self::try_create_from_impl_or_throw(null_or_global_object_for_oom, vm, sign == ParseIntSign::Signed, &result_vector);
            }
        }

        let limit0: u32 = '0' as u32 + if radix < 10 { radix } else { 10 };
        // Aritmética em `int` como no C++ (`'a' + (static_cast<int32_t>(radix) - 10)`): com radix < 10 a
        // soma em `u32` estouraria. O resultado fica abaixo de 'a' e 'A', então nenhuma letra casa.
        let limita: u32 = ('a' as i32 + (radix as i32 - 10)) as u32;
        let limit_upper_a: u32 = ('A' as i32 + (radix as i32 - 10)) as u32;
        let mut result_vector: Vec<Digit> = Vec::new();
        while p < length {
            let mut digit: u64 = 0;
            let mut multiplier: u64 = 1;
            let mut i = 0;
            while i < length_limit_for_big_int32 && p < length {
                digit = digit.checked_mul(radix as u64).expect("estouro em parseInt do BigInt");
                multiplier = multiplier.checked_mul(radix as u64).expect("estouro em parseInt do BigInt");
                let character = unit(p);
                let value = if character >= '0' as u32 && character < limit0 {
                    (character - '0' as u32) as u64
                } else if character >= 'a' as u32 && character < limita {
                    (character - 'a' as u32 + 10) as u64
                } else if character >= 'A' as u32 && character < limit_upper_a {
                    (character - 'A' as u32 + 10) as u64
                } else {
                    Self::throw_parse_failure(null_or_global_object_for_oom, vm, error_parse_mode);
                    return ImplResult::Empty;
                };
                digit = digit.checked_add(value).expect("estouro em parseInt do BigInt");
                i += 1;
                p += 1;
            }

            if result_vector.is_empty() {
                if p == length {
                    debug_assert!(digit <= i64::MAX as u64);
                    let mut maybe_result = digit as i64;
                    debug_assert!(maybe_result >= 0);
                    if sign == ParseIntSign::Signed {
                        maybe_result *= -1;
                    }

                    if maybe_result as i32 as i64 == maybe_result {
                        // `USE(BIGINT32)` é 0.
                        return match JSBigInt::create_from_i32(maybe_result as i32) {
                            Ok(big_int) => ImplResult::Heap(big_int),
                            Err(error) => {
                                Self::throw_big_int_error(null_or_global_object_for_oom, vm, &error);
                                ImplResult::Empty
                            }
                        };
                    }
                }

                let Some(result_length) = compute_length(radix, initial_length) else {
                    Self::throw_big_int_error(null_or_global_object_for_oom, vm, &BigIntError::TooBig);
                    return ImplResult::Empty;
                };

                result_vector.resize(result_length as usize, 0);
            }

            debug_assert!(multiplier as Digit as u64 == multiplier);
            debug_assert!(digit as Digit as u64 == digit);
            let source_length = result_vector.len();
            Self::multiply_add(Operand::in_place(source_length), multiplier as Digit, digit as Digit, &mut result_vector);
        }

        Self::try_create_from_impl_or_throw(null_or_global_object_for_oom, vm, sign == ParseIntSign::Signed, &result_vector)
    }
}
