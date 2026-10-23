//! `Number.prototype.toFixed` e `Number.prototype.toPrecision` do JavaScript, para as estatísticas de uso
//! do atlas saírem com o mesmo texto do original.
//!
//! O arredondamento do `format!` do Rust desempata para o par (12.5 vira "12"), e o do JavaScript
//! desempata para o maior `n` ("pick the larger n": 12.5 vira "13"). Por isso as duas funções
//! arredondam sobre a expansão decimal exata do valor (60 casas), com "meio para cima".

/// Quantas casas da expansão decimal exata entram no arredondamento. Um `f64` normal tem a expansão exata
/// bem mais curta nos casos de empate, que são os únicos em que as casas além do corte decidem.
const EXACT_DIGITS: usize = 60;

fn non_finite(value: f64) -> Option<String> {
    if value.is_nan() {
        Some("NaN".to_owned())
    } else if value.is_infinite() {
        Some(if value < 0.0 { "-Infinity" } else { "Infinity" }.to_owned())
    } else {
        None
    }
}

/// Soma 1 na posição `keep - 1` dos dígitos e propaga o vai-um. Devolve `true` se sobrou um vai-um para
/// um dígito novo à esquerda (que é inserido).
fn round_half_up(digits: &mut Vec<u8>, keep: usize) -> bool {
    let round_up = digits.get(keep).is_some_and(|digit| *digit >= 5);
    digits.truncate(keep);
    if !round_up {
        return false;
    }
    for digit in digits.iter_mut().rev() {
        if *digit == 9 {
            *digit = 0;
        } else {
            *digit += 1;
            return false;
        }
    }
    digits.insert(0, 1);
    true
}

fn digits_to_string(digits: &[u8]) -> String {
    digits.iter().map(|digit| char::from(b'0' + digit)).collect()
}

/// `value.toFixed(fraction_digits)`.
pub fn to_fixed(value: f64, fraction_digits: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let exact = format!("{:.*}", EXACT_DIGITS, value.abs());
    let (integer, fraction) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut digits: Vec<u8> = integer.bytes().chain(fraction.bytes()).map(|byte| byte - b'0').collect();
    let mut integer_len = integer.len();
    if round_half_up(&mut digits, integer_len + fraction_digits) {
        integer_len += 1;
    }
    let (integer_digits, fraction_part) = digits.split_at(integer_len);
    if fraction_digits == 0 {
        format!("{sign}{}", digits_to_string(integer_digits))
    } else {
        format!("{sign}{}.{}", digits_to_string(integer_digits), digits_to_string(fraction_part))
    }
}

/// `value.toPrecision(precision)`, com `precision` entre 1 e 100.
pub fn to_precision(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }
    let sign = if value < 0.0 { "-" } else { "" };
    if value == 0.0 {
        return if precision == 1 { "0".to_owned() } else { format!("0.{}", "0".repeat(precision - 1)) };
    }
    let scientific = format!("{:.*e}", EXACT_DIGITS, value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let mut exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(|byte| *byte != b'.').map(|byte| byte - b'0').collect();
    if round_half_up(&mut digits, precision) {
        digits.truncate(precision);
        exponent += 1;
    }
    let text = digits_to_string(&digits);
    let precision_i32 = i32::try_from(precision).unwrap_or(i32::MAX);
    if exponent < -6 || exponent >= precision_i32 {
        let point = if precision == 1 { String::new() } else { format!(".{}", &text[1..]) };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{}{point}e{exponent_sign}{}", &text[..1], exponent.abs());
    }
    if exponent == precision_i32 - 1 {
        return format!("{sign}{text}");
    }
    if exponent >= 0 {
        let split = usize::try_from(exponent).unwrap_or(0) + 1;
        return format!("{sign}{}.{}", &text[..split], &text[split..]);
    }
    format!("{sign}0.{}{text}", "0".repeat(usize::try_from(-(exponent + 1)).unwrap_or(0)))
}
