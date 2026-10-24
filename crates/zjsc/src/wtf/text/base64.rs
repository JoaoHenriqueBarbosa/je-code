//! Porte de `WTF/wtf/text/Base64.{h,cpp}`: a codificação base64 (`base64EncodeToString`) e a decodificação do
//! proposal `Uint8Array` base64 (`fromBase64`, `maxLengthFromBase64`).
//!
//! DIVERGÊNCIAS:
//!
//! - O C++ decodifica com `simdutf::base64_to_binary_safe(..., decodeUpToBadChar = true)`. Aqui está o caminho
//!   escalar do próprio simdutf (`scalar::base64::find_end`, `base64_tail_decode_impl`, `patch_tail_result`,
//!   `base64_to_binary_details_impl` e `base64_to_binary_safe_impl` com `slow_base64_to_binary_safe_impl`),
//!   que é a especificação de que as implementações SIMD são otimização. Os modos `*_accept_garbage` e
//!   `base64_default_or_url` do simdutf não são usados pelo WTF e não entram.
//! - Só a codificação e a decodificação de `fromBase64` existem. `base64Decode` com `Base64DecodeOption`
//!   (`ValidatePadding`, `IgnoreWhitespace`), usado por `atob` e fetch, não foi pedido por nenhum chamador
//!   portado e não entra.
//! - A codificação reusa `ul_common::codec::base64_encode`, de semântica idêntica (RFC 4648, com `=` ou sem).

use ul_common::codec::{base64_encode, BASE64_STANDARD, BASE64_URL};

use crate::wtf::text::wtf_string::String as WtfString;

/// `enum class Alphabet : uint8_t { Base64, Base64URL }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alphabet {
    Base64,
    Base64URL,
}

/// `enum class LastChunkHandling : uint8_t { Loose, Strict, StopBeforePartial }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastChunkHandling {
    Loose,
    Strict,
    StopBeforePartial,
}

/// `enum class FromBase64ShouldThrowError : bool { No, Yes }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FromBase64ShouldThrowError {
    No,
    Yes,
}

/// `enum class OutputSizeIsMaxLength : bool { No, Yes }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputSizeIsMaxLength {
    No,
    Yes,
}

/// `maximumBase64EncoderInputBufferSize`: acima disso a entrada é patológica e o resultado é nulo.
pub const MAXIMUM_BASE64_ENCODER_INPUT_BUFFER_SIZE: usize = (u32::MAX as usize) / 77 * 76 / 4 * 3 - 2;

/// `base64EncodeToStringReturnNullIfOverflow(input, options)` com `Base64EncodeOption::URL` (`url`) e
/// `OmitPadding` (`omit_padding`): `None` é a `String` nula do C++ (entrada acima do limite do codificador ou
/// texto acima de `String::MAX_LENGTH`).
pub fn base64_encode_to_string_return_none_if_overflow(input: &[u8], url: bool, omit_padding: bool) -> Option<WtfString> {
    if input.len() > MAXIMUM_BASE64_ENCODER_INPUT_BUFFER_SIZE {
        return None;
    }
    if base64_length_from_binary(input.len(), !omit_padding) > WtfString::MAX_LENGTH as usize {
        return None;
    }
    let alphabet = if url { BASE64_URL } else { BASE64_STANDARD };
    Some(WtfString::from_latin1(&base64_encode(input, alphabet, !omit_padding)))
}

/// `simdutf::base64_length_from_binary(length, options)`. O `use_padding` do simdutf é
/// `((options & base64_url) == 0) ^ reverse_padding`: com as opções que o WTF monta (`toSIMDUTFEncodeOptions`
/// e os `base64_default`/`base64_url` da decodificação) ele é verdadeiro quando o texto leva `=`.
fn base64_length_from_binary(length: usize, use_padding: bool) -> usize {
    if !use_padding {
        return length / 3 * 4 + if length % 3 != 0 { length % 3 + 1 } else { 0 };
    }
    length.div_ceil(3) * 4
}

/// `simdutf::error_code`, só os valores que o decodificador devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorCode {
    Success,
    InvalidBase64Character,
    Base64InputRemainder,
    Base64ExtraBits,
    OutputBufferTooSmall,
}

/// `simdutf::last_chunk_handling_options`: os três do WTF mais `only_full_chunks`, que o decodificador
/// seguro usa na primeira passada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkMode {
    Loose,
    Strict,
    StopBeforePartial,
    OnlyFullChunks,
}

impl ChunkMode {
    /// `is_partial(options)`.
    fn is_partial(self) -> bool {
        matches!(self, ChunkMode::StopBeforePartial | ChunkMode::OnlyFullChunks)
    }
}

impl From<LastChunkHandling> for ChunkMode {
    /// `toSIMDUTFLastChunkHandling`.
    fn from(handling: LastChunkHandling) -> ChunkMode {
        match handling {
            LastChunkHandling::Loose => ChunkMode::Loose,
            LastChunkHandling::Strict => ChunkMode::Strict,
            LastChunkHandling::StopBeforePartial => ChunkMode::StopBeforePartial,
        }
    }
}

/// `simdutf::full_result`.
#[derive(Clone, Copy, Debug)]
struct FullResult {
    error: ErrorCode,
    input_count: usize,
    output_count: usize,
    /// "true if the error is due to padding".
    padding_error: bool,
}

impl FullResult {
    fn new(error: ErrorCode, input_count: usize, output_count: usize) -> FullResult {
        FullResult { error, input_count, output_count, padding_error: false }
    }

    fn padding(error: ErrorCode, input_count: usize, output_count: usize) -> FullResult {
        FullResult { error, input_count, output_count, padding_error: true }
    }
}

/// `tables::base64::to_base64_value` e `to_base64_url_value`: o valor de 0 a 63, 64 para o espaço ASCII
/// (` `, `\t`, `\n`, `\r`, `\f`) e 255 para o que não é do alfabeto. Unidade acima de 255 (UTF-16) nunca é do
/// alfabeto, o que o `is_eight_byte` do simdutf garante.
fn base64_value(unit: u32, url: bool) -> u8 {
    match unit {
        0x41..=0x5A => (unit - 0x41) as u8,
        0x61..=0x7A => (unit - 0x61 + 26) as u8,
        0x30..=0x39 => (unit - 0x30 + 52) as u8,
        0x2B if !url => 62,
        0x2F if !url => 63,
        0x2D if url => 62,
        0x5F if url => 63,
        0x09 | 0x0A | 0x0C | 0x0D | 0x20 => 64,
        _ => 255,
    }
}

/// `base64_ignorable(c, options)` sem `accept_garbage`: só o espaço ASCII.
fn is_ignorable<T: Copy + Into<u32>>(unit: T, url: bool) -> bool {
    base64_value(unit.into(), url) == 64
}

/// `scalar::base64::reduced_input`.
struct ReducedInput {
    equal_signs: usize,
    equal_location: usize,
    src_len: usize,
    full_input_length: usize,
}

/// `scalar::base64::find_end`: o fim da entrada sem o `=` do preenchimento (no máximo dois) e sem o espaço de
/// cada lado deles. O espaço final entra no `full_input_length`.
fn find_end<T: Copy + Into<u32>>(src: &[T], url: bool) -> ReducedInput {
    let mut src_len = src.len();
    let full_input_length = src_len;
    let mut equal_signs = 0;
    while src_len > 0 && is_ignorable(src[src_len - 1], url) {
        src_len -= 1;
    }
    let mut equal_location = src_len;
    if src_len > 0 && Into::<u32>::into(src[src_len - 1]) == u32::from(b'=') {
        equal_location = src_len - 1;
        src_len -= 1;
        equal_signs = 1;
        while src_len > 0 && is_ignorable(src[src_len - 1], url) {
            src_len -= 1;
        }
        if src_len > 0 && Into::<u32>::into(src[src_len - 1]) == u32::from(b'=') {
            equal_location = src_len - 1;
            src_len -= 1;
            equal_signs = 2;
        }
    }
    ReducedInput { equal_signs, equal_location, src_len, full_input_length }
}

/// `scalar::base64::base64_tail_decode_impl<check_capacity>`: o `=` já foi retirado da entrada
/// (`padding_characters` os conta). Com `CHECK_CAPACITY` não escreve além de `dst` e devolve
/// `OutputBufferTooSmall`.
///
/// O laço rápido de quatro caracteres do C++ (as tabelas `d0` a `d3`) é só uma otimização do laço comum e não
/// existe aqui: os dois param nos mesmos pontos e devolvem os mesmos valores.
fn base64_tail_decode<T: Copy + Into<u32>, const CHECK_CAPACITY: bool>(
    dst: &mut [u8],
    src: &[T],
    padding_characters: usize,
    url: bool,
    mode: ChunkMode,
) -> FullResult {
    let mut s = 0usize;
    let mut d = 0usize;
    loop {
        let src_cur = s;
        let mut idx = 0usize;
        let mut buffer = [0u8; 4];
        while idx < 4 && s < src.len() {
            let code = base64_value(src[s].into(), url);
            buffer[idx] = code;
            if code <= 63 {
                idx += 1;
            } else if code > 64 {
                return FullResult::new(ErrorCode::InvalidBase64Character, s, d);
            }
            // O espaço (código 64) é ignorado.
            s += 1;
        }
        if idx != 4 {
            debug_assert!(idx < 4);
            // O número de caracteres base64 mais o de `=` nunca passa de 4.
            if idx + padding_characters > 4 {
                return FullResult::padding(ErrorCode::InvalidBase64Character, s, d);
            }

            // No modo `loose`, se há preenchimento ele tem de fechar o bloco de 4; sem nenhum é aceito.
            if mode == ChunkMode::Loose && idx >= 2 && padding_characters > 0 && ((idx + padding_characters) & 3) != 0 {
                return FullResult::padding(ErrorCode::InvalidBase64Character, s, d);
            } else if mode == ChunkMode::Strict && idx >= 2 && ((idx + padding_characters) & 3) != 0 {
                // No modo `strict` o bloco incompleto é `BASE64_INPUT_REMAINDER`.
                return FullResult::padding(ErrorCode::Base64InputRemainder, s, d);
            } else if (mode == ChunkMode::StopBeforePartial
                && padding_characters + idx < 4
                && idx != 0
                && (idx >= 2 || padding_characters == 0))
                || (mode == ChunkMode::OnlyFullChunks && (idx >= 2 || padding_characters == 0))
            {
                // O bloco parcial não é consumido: volta ao começo dele.
                return FullResult::new(ErrorCode::Success, src_cur, d);
            } else {
                if idx == 2 {
                    let triple = (u32::from(buffer[0]) << 18) + (u32::from(buffer[1]) << 12);
                    if mode == ChunkMode::Strict && (triple & 0xffff) != 0 {
                        return FullResult::new(ErrorCode::Base64ExtraBits, s, d);
                    }
                    if CHECK_CAPACITY && dst.len() - d < 1 {
                        return FullResult::new(ErrorCode::OutputBufferTooSmall, src_cur, d);
                    }
                    dst[d] = ((triple >> 16) & 0xFF) as u8;
                    d += 1;
                } else if idx == 3 {
                    let triple = (u32::from(buffer[0]) << 18) + (u32::from(buffer[1]) << 12) + (u32::from(buffer[2]) << 6);
                    if mode == ChunkMode::Strict && (triple & 0xff) != 0 {
                        return FullResult::new(ErrorCode::Base64ExtraBits, s, d);
                    }
                    if CHECK_CAPACITY && dst.len() - d < 2 {
                        return FullResult::new(ErrorCode::OutputBufferTooSmall, src_cur, d);
                    }
                    dst[d] = ((triple >> 16) & 0xFF) as u8;
                    dst[d + 1] = ((triple >> 8) & 0xFF) as u8;
                    d += 2;
                } else if idx == 1 && (!mode.is_partial() || padding_characters > 0) {
                    return FullResult::new(ErrorCode::Base64InputRemainder, s, d);
                } else if idx == 0 && padding_characters > 0 {
                    return FullResult::padding(ErrorCode::InvalidBase64Character, s, d);
                }
                return FullResult::new(ErrorCode::Success, s, d);
            }
        }
        if CHECK_CAPACITY && dst.len() - d < 3 {
            return FullResult::new(ErrorCode::OutputBufferTooSmall, src_cur, d);
        }
        let triple = (u32::from(buffer[0]) << 18) + (u32::from(buffer[1]) << 12) + (u32::from(buffer[2]) << 6) + u32::from(buffer[3]);
        dst[d] = ((triple >> 16) & 0xFF) as u8;
        dst[d + 1] = ((triple >> 8) & 0xFF) as u8;
        dst[d + 2] = (triple & 0xFF) as u8;
        d += 3;
    }
}

/// `scalar::base64::patch_tail_result(r, 0, 0, equallocation, full_input_length, mode)`.
fn patch_tail_result(mut r: FullResult, equal_location: usize, full_input_length: usize, mode: ChunkMode) -> FullResult {
    if r.padding_error {
        r.input_count = equal_location;
    }
    if r.error == ErrorCode::Success {
        if !mode.is_partial() {
            // Sucesso fora de `stop_before_partial` é a entrada inteira consumida.
            r.input_count = full_input_length;
        } else if r.output_count % 3 != 0 {
            r.input_count = full_input_length;
        }
    }
    r
}

/// `scalar::base64::base64_to_binary_details_impl` (`CHECK_CAPACITY` falso) e
/// `base64_to_binary_details_safe_impl` (verdadeiro).
fn base64_to_binary_details<T: Copy + Into<u32>, const CHECK_CAPACITY: bool>(
    input: &[T],
    output: &mut [u8],
    url: bool,
    mode: ChunkMode,
) -> FullResult {
    let ri = find_end(input, url);
    if ri.src_len == 0 {
        if ri.equal_signs > 0 {
            return FullResult::padding(ErrorCode::InvalidBase64Character, ri.equal_location, 0);
        }
        return FullResult::new(ErrorCode::Success, ri.full_input_length, 0);
    }
    let mut r = base64_tail_decode::<T, CHECK_CAPACITY>(output, &input[..ri.src_len], ri.equal_signs, url, mode);
    r = patch_tail_result(r, ri.equal_location, ri.full_input_length, mode);
    if !mode.is_partial() && r.error == ErrorCode::Success && ri.equal_signs > 0 {
        // Verificações adicionais do preenchimento.
        if r.output_count % 3 == 0 || (r.output_count % 3) + 1 + ri.equal_signs != 4 {
            return FullResult::padding(ErrorCode::InvalidBase64Character, ri.equal_location, r.output_count);
        }
    }
    // Com `is_partial`, a entrada consumida termina no fim do fluxo (depois do espaço) ou logo após um caractere
    // que não é ignorável. https://tc39.es/proposal-arraybuffer-base64/spec/#sec-frombase64
    if mode.is_partial() && r.error == ErrorCode::Success && r.input_count < ri.full_input_length {
        while r.input_count < ri.full_input_length && is_ignorable(input[r.input_count], url) {
            r.input_count += 1;
        }
        if r.input_count < ri.full_input_length {
            while r.input_count > 0 && is_ignorable(input[r.input_count - 1], url) {
                r.input_count -= 1;
            }
        }
    }
    r
}

/// `slow_base64_to_binary_safe_impl`: o resultado `(erro, entrada consumida, saída escrita)`.
fn slow_base64_to_binary_safe<T: Copy + Into<u32>>(
    input: &[T],
    output: &mut [u8],
    url: bool,
    mode: ChunkMode,
) -> (ErrorCode, usize, usize) {
    let ri = find_end(input, url);
    if ri.src_len == 0 {
        if ri.equal_signs > 0 {
            return (ErrorCode::InvalidBase64Character, ri.equal_location, 0);
        }
        return (ErrorCode::Success, 0, 0);
    }
    let mut r = base64_tail_decode::<T, true>(output, &input[..ri.src_len], ri.equal_signs, url, mode);
    r = patch_tail_result(r, ri.equal_location, ri.full_input_length, mode);
    let out_length = r.output_count;
    let mut error = r.error;
    if !mode.is_partial() && error == ErrorCode::Success && ri.equal_signs > 0 && (out_length % 3 == 0 || (out_length % 3) + 1 + ri.equal_signs != 4)
    {
        error = ErrorCode::InvalidBase64Character;
    }
    (error, r.input_count, out_length)
}

/// `base64_to_binary_safe_impl(..., decode_up_to_bad_char = true)`: `(erro, entrada lida, saída escrita)`.
fn base64_to_binary_safe<T: Copy + Into<u32>>(
    input: &[T],
    output: &mut [u8],
    url: bool,
    last_chunk_handling: ChunkMode,
) -> (ErrorCode, usize, usize) {
    let length = input.len();
    let out_capacity = output.len();

    // Uma primeira passada pelo caminho rápido decodifica o que cabe com folga (`base64_length_from_binary`
    // com as opções de decodificação: o alfabeto padrão leva `=` e o de URL não).
    let safe_input = length.min(base64_length_from_binary(out_capacity / 3 * 3, !url));
    let done_with_partial = safe_input == length;
    let mode = if done_with_partial { last_chunk_handling } else { ChunkMode::OnlyFullChunks };
    let r = base64_to_binary_details::<T, false>(&input[..safe_input], output, url, mode);
    debug_assert!(r.input_count <= safe_input);
    debug_assert!(r.output_count <= out_capacity);
    let mut input_position = r.input_count;
    let mut output_position = r.output_count;
    if r.error != ErrorCode::Success {
        if r.error == ErrorCode::InvalidBase64Character {
            return slow_base64_to_binary_safe(input, output, url, last_chunk_handling);
        }
        return (r.error, input_position, output_position);
    }

    if done_with_partial {
        return (ErrorCode::Success, input_position, output_position);
    }
    // O resto da entrada, agora conferindo a capacidade.
    let r = base64_to_binary_details::<T, true>(&input[input_position..], &mut output[output_position..], url, last_chunk_handling);
    input_position += r.input_count;
    output_position += r.output_count;

    if r.error != ErrorCode::Success {
        if r.error == ErrorCode::InvalidBase64Character {
            return slow_base64_to_binary_safe(input, output, url, last_chunk_handling);
        }
        return (r.error, input_position, output_position);
    }
    if input_position < length {
        // A passada rápida pode ter "comido" o espaço final, mas pelo padrão do JavaScript, com só espaço
        // seguido de um caractere base64, nenhum caractere é consumido.
        while input_position > 0 && is_ignorable(input[input_position - 1], url) {
            input_position -= 1;
        }
    }
    (ErrorCode::Success, input_position, output_position)
}

/// `fromBase64Impl`.
fn from_base64_impl<T: Copy + Into<u32>>(
    span: &[T],
    output: &mut [u8],
    alphabet: Alphabet,
    last_chunk_handling: LastChunkHandling,
    output_size_is_max_length: OutputSizeIsMaxLength,
) -> (FromBase64ShouldThrowError, usize, usize) {
    // O passo 3 de https://tc39.es/proposal-arraybuffer-base64/spec/#sec-frombase64 volta antes de olhar um
    // caractere quando `maxLength` é 0, então até a entrada inválida é aceita.
    if output_size_is_max_length == OutputSizeIsMaxLength::Yes && output.is_empty() {
        return (FromBase64ShouldThrowError::No, 0, 0);
    }

    let (error, read_length, write_length) =
        base64_to_binary_safe(span, output, alphabet == Alphabet::Base64URL, ChunkMode::from(last_chunk_handling));
    match error {
        ErrorCode::OutputBufferTooSmall | ErrorCode::Success => (FromBase64ShouldThrowError::No, read_length, write_length),
        ErrorCode::InvalidBase64Character | ErrorCode::Base64InputRemainder | ErrorCode::Base64ExtraBits => {
            (FromBase64ShouldThrowError::Yes, read_length, write_length)
        }
    }
}

/// `fromBase64(string, output, alphabet, lastChunkHandling, outputSizeIsMaxLength)`: `(deve lançar, caracteres
/// lidos, bytes escritos)`.
pub fn from_base64(
    string: &WtfString,
    output: &mut [u8],
    alphabet: Alphabet,
    last_chunk_handling: LastChunkHandling,
    output_size_is_max_length: OutputSizeIsMaxLength,
) -> (FromBase64ShouldThrowError, usize, usize) {
    if string.is_8bit() {
        return from_base64_impl(string.span8(), output, alphabet, last_chunk_handling, output_size_is_max_length);
    }
    from_base64_impl(string.span16(), output, alphabet, last_chunk_handling, output_size_is_max_length)
}

/// `simdutf::scalar::base64::maximal_binary_length_from_base64`.
fn maximal_binary_length<T: Copy + Into<u32>>(input: &[T]) -> usize {
    let length = input.len();
    let equals = u32::from(b'=');
    let mut padding = 0;
    if length > 0 && Into::<u32>::into(input[length - 1]) == equals {
        padding += 1;
        if length > 1 && Into::<u32>::into(input[length - 2]) == equals {
            padding += 1;
        }
    }
    let actual_length = length - padding;
    if actual_length % 4 <= 1 {
        return actual_length / 4 * 3;
    }
    // Numa entrada válida o resto é 2 ou 3, que somam um ou dois bytes.
    actual_length / 4 * 3 + (actual_length % 4) - 1
}

/// `maxLengthFromBase64(string)`.
pub fn max_length_from_base64(string: &WtfString) -> usize {
    if string.is_8bit() {
        return maximal_binary_length(string.span8());
    }
    maximal_binary_length(string.span16())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(text: &str, alphabet: Alphabet, mode: LastChunkHandling, capacity: Option<usize>) -> (bool, usize, Vec<u8>) {
        let string = WtfString::from_latin1(text.as_bytes());
        let (max, kind) = match capacity {
            Some(capacity) => (capacity, OutputSizeIsMaxLength::Yes),
            None => (max_length_from_base64(&string), OutputSizeIsMaxLength::No),
        };
        let mut output = vec![0u8; max];
        let (error, read, written) = from_base64(&string, &mut output, alphabet, mode, kind);
        output.truncate(written);
        (error == FromBase64ShouldThrowError::Yes, read, output)
    }

    #[test]
    fn decodes_loose() {
        assert_eq!(decode("Zm9vYmFy", Alphabet::Base64, LastChunkHandling::Loose, None), (false, 8, b"foobar".to_vec()));
        assert_eq!(decode("Zm8", Alphabet::Base64, LastChunkHandling::Loose, None), (false, 3, b"fo".to_vec()));
        assert_eq!(decode(" Zm 9v ", Alphabet::Base64, LastChunkHandling::Loose, None), (false, 7, b"foo".to_vec()));
        assert_eq!(decode("Zm8=", Alphabet::Base64, LastChunkHandling::Loose, None), (false, 4, b"fo".to_vec()));
        assert!(decode("Zm8==", Alphabet::Base64, LastChunkHandling::Loose, None).0);
        assert!(decode("Z", Alphabet::Base64, LastChunkHandling::Loose, None).0);
        assert!(decode("Zm9v!", Alphabet::Base64, LastChunkHandling::Loose, None).0);
        assert!(decode("-_", Alphabet::Base64, LastChunkHandling::Loose, None).0);
        assert_eq!(decode("-_8", Alphabet::Base64URL, LastChunkHandling::Loose, None).2, vec![0xfb, 0xff]);
    }

    #[test]
    fn strict_and_stop_before_partial() {
        assert!(decode("Zm8", Alphabet::Base64, LastChunkHandling::Strict, None).0);
        assert!(decode("Zm9=", Alphabet::Base64, LastChunkHandling::Strict, None).0);
        assert_eq!(decode("Zm8=", Alphabet::Base64, LastChunkHandling::Strict, None), (false, 4, b"fo".to_vec()));
        assert_eq!(decode("Zm9vYg", Alphabet::Base64, LastChunkHandling::StopBeforePartial, None), (false, 4, b"foo".to_vec()));
    }

    #[test]
    fn set_from_base64_stops_when_full() {
        assert_eq!(decode("Zm9vYmFy", Alphabet::Base64, LastChunkHandling::Loose, Some(4)), (false, 4, b"foo".to_vec()));
        assert_eq!(decode("Zm9vYmFy", Alphabet::Base64, LastChunkHandling::Loose, Some(0)), (false, 0, Vec::new()));
        assert_eq!(decode("%", Alphabet::Base64, LastChunkHandling::Loose, Some(0)), (false, 0, Vec::new()));
    }

    #[test]
    fn encodes() {
        let encode = |bytes: &[u8], url, omit| {
            base64_encode_to_string_return_none_if_overflow(bytes, url, omit).unwrap().span8().to_vec()
        };
        assert_eq!(encode(b"fo", false, false), b"Zm8=");
        assert_eq!(encode(b"fo", false, true), b"Zm8");
        assert_eq!(encode(b"\xfb\xff", true, false), b"-_8=");
        assert_eq!(encode(b"\xfb\xff", true, true), b"-_8");
    }
}
