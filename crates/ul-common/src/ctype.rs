//! Classificação de bytes e conversão de números como a libc faz no locale C.UTF-8: byte acima de
//! 0x7f nunca é classe nenhuma, `strtol`/`strtoull` aceitam espaço inicial, sinal e base 0 com `0x`
//! e `0`, e param no primeiro byte inválido; `strtod` entende decimal, `inf`, `nan` e hexadecimal.
//!
//! As conversões devolvem quantos bytes consumiram em vez de um ponteiro, e as cadeias C (terminadas
//! em NUL) são tratadas com [`at`] e [`cstr`].

/// `isspace`: espaço, `\t`, `\n`, `\v`, `\f`, `\r`.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isblank`: espaço e `\t`.
pub fn is_blank(c: u8) -> bool {
    matches!(c, b' ' | b'\t')
}

/// `isprint` por byte (no C.UTF-8 só o ASCII imprimível conta).
pub fn is_print(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

/// A cadeia C que começa em `s`: até o primeiro NUL (ou o fim).
pub fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

/// A cadeia C que começa em `off` dentro de uma tabela de strings (ELF, por exemplo): até o
/// primeiro NUL. Deslocamento fora da tabela dá a cadeia vazia.
pub fn cstr_at(table: &[u8], off: usize) -> &[u8] {
    cstr(table.get(off..).unwrap_or(&[]))
}

/// Byte na posição `i`, ou NUL depois do fim (o terminador implícito das cadeias C).
pub fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// Valor de um dígito em qualquer base até 36 (`0`-`9`, `a`-`z`, `A`-`Z`).
pub fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

/// Valor de um dígito hexadecimal.
pub fn hex_value(c: u8) -> Option<u32> {
    digit_value(c).filter(|&d| d < 16)
}

/// Resultado de uma conversão `strto*`: valor, bytes consumidos (0 = nada convertido, como
/// `endptr == nptr`) e se houve estouro (`ERANGE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conv<T> {
    pub value: T,
    pub used: usize,
    pub overflow: bool,
}

/// Dígitos de `s` a partir de `i` na base dada, sem espaço, sinal nem prefixo. Devolve a magnitude
/// (saturada em `u64::MAX`), o índice onde parou e se estourou.
fn scan_digits(s: &[u8], mut i: usize, base: u32) -> (u64, usize, bool) {
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit_value(at(s, i)) {
        if d >= base {
            break;
        }
        match value.checked_mul(u64::from(base)).and_then(|v| v.checked_add(u64::from(d))) {
            Some(v) => value = v,
            None => {
                overflow = true;
                value = u64::MAX;
            }
        }
        i += 1;
    }
    (value, i, overflow)
}

/// Núcleo comum: espaços, sinal, prefixo da base e dígitos. Devolve (negativo, magnitude saturada em
/// `u64::MAX`, consumido, estourou).
fn scan_integer(s: &[u8], base: u32) -> (bool, u64, usize, bool) {
    let mut i = 0;
    while is_space(at(s, i)) {
        i += 1;
    }
    let mut neg = false;
    if at(s, i) == b'-' || at(s, i) == b'+' {
        neg = at(s, i) == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16) && at(s, i) == b'0' && (at(s, i + 1) | 0x20) == b'x' {
        // `0x` só vale como prefixo se vier um dígito hexadecimal depois.
        if hex_value(at(s, i + 2)).is_some() {
            i += 2;
            base = 16;
        } else if base == 0 {
            base = 8;
        }
    } else if base == 0 {
        base = if at(s, i) == b'0' { 8 } else { 10 };
    }
    let (value, end, overflow) = scan_digits(s, i, base);
    if end == i {
        return (false, 0, 0, false);
    }
    (neg, value, end, overflow)
}

/// `strtoull(s, &end, base)`: sinal negativo inverte (com volta) a magnitude.
pub fn strtoull(s: &[u8], base: u32) -> Conv<u64> {
    let (neg, mag, used, overflow) = scan_integer(s, base);
    if overflow {
        return Conv { value: u64::MAX, used, overflow };
    }
    let value = if neg { mag.wrapping_neg() } else { mag };
    Conv { value, used, overflow }
}

/// Como [`strtoull`] com a base já decidida pelo chamador: não reconhece o prefixo `0x`, só espaço,
/// sinal e dígitos.
pub fn strtoull_fixed_base(s: &[u8], base: u32) -> Conv<u64> {
    let mut i = 0;
    while is_space(at(s, i)) {
        i += 1;
    }
    let neg = at(s, i) == b'-';
    if neg || at(s, i) == b'+' {
        i += 1;
    }
    let (mag, end, overflow) = scan_digits(s, i, base);
    if end == i {
        return Conv { value: 0, used: 0, overflow: false };
    }
    if overflow {
        return Conv { value: u64::MAX, used: end, overflow };
    }
    Conv { value: if neg { mag.wrapping_neg() } else { mag }, used: end, overflow }
}

/// `strtol` (long de 64 bits), com saturação em `i64::MIN`/`i64::MAX`.
pub fn strtol(s: &[u8], base: u32) -> Conv<i64> {
    let (neg, mag, used, overflow) = scan_integer(s, base);
    if neg {
        if overflow || mag > (i64::MAX as u64) + 1 {
            return Conv { value: i64::MIN, used, overflow: true };
        }
        Conv { value: (mag as i64).wrapping_neg(), used, overflow: false }
    } else {
        if overflow || mag > i64::MAX as u64 {
            return Conv { value: i64::MAX, used, overflow: true };
        }
        Conv { value: mag as i64, used, overflow: false }
    }
}

/// Resultado de [`strtol_whole`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WholeLong {
    Ok(i64),
    /// Estourou (o `strtol` satura e marca ERANGE).
    Range(i64),
    /// Não é número inteiro do começo ao fim.
    Invalid,
}

/// `strtol(s, &end, base)` com o teste `*end == '\0'` de quem exige a cadeia inteira: espaço à
/// esquerda e sinal valem, nenhum dígito ou lixo depois é inválido.
pub fn strtol_whole(s: &[u8], base: u32) -> WholeLong {
    let c = strtol(s, base);
    if c.used == 0 || c.used != s.len() {
        WholeLong::Invalid
    } else if c.overflow {
        WholeLong::Range(c.value)
    } else {
        WholeLong::Ok(c.value)
    }
}

/// Número decimal sem sinal, sem espaço e sem lixo: a cadeia inteira (não vazia) é dígito e cabe em
/// `u64`.
pub fn parse_decimal(s: &[u8]) -> Option<u64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let c = strtoull_fixed_base(s, 10);
    (!c.overflow).then_some(c.value)
}

/// Como [`parse_decimal`], para quem guarda o número num `usize`.
pub fn parse_decimal_usize(s: &[u8]) -> Option<usize> {
    parse_decimal(s).and_then(|n| usize::try_from(n).ok())
}

/// Inteiro decimal com sinal (`-` ou `+`) opcional, sem espaço e sem lixo: a cadeia inteira tem que
/// ser dígito depois do sinal e caber em `i64` (estouro vira `None`, sem saturar).
pub fn parse_i64(s: &[u8]) -> Option<i64> {
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let magnitude = parse_decimal(digits)?;
    if neg { 0i64.checked_sub_unsigned(magnitude) } else { i64::try_from(magnitude).ok() }
}

/// `strtod` no locale C: decimal com expoente, `inf`/`infinity`/`nan` e hexadecimal (`0x1p3`).
pub fn strtod(s: &[u8]) -> Conv<f64> {
    let mut i = 0;
    while is_space(at(s, i)) {
        i += 1;
    }
    let mut neg = false;
    if at(s, i) == b'-' || at(s, i) == b'+' {
        neg = at(s, i) == b'-';
        i += 1;
    }
    let lower = |k: usize| at(s, k).to_ascii_lowercase();
    // inf / infinity / nan
    if lower(i) == b'i' && lower(i + 1) == b'n' && lower(i + 2) == b'f' {
        let mut used = i + 3;
        if (0..5).all(|k| lower(i + 3 + k) == b"inity"[k]) {
            used = i + 8;
        }
        let v = if neg { f64::NEG_INFINITY } else { f64::INFINITY };
        return Conv { value: v, used, overflow: false };
    }
    if lower(i) == b'n' && lower(i + 1) == b'a' && lower(i + 2) == b'n' {
        let mut used = i + 3;
        if at(s, used) == b'(' {
            let mut k = used + 1;
            while at(s, k).is_ascii_alphanumeric() || at(s, k) == b'_' {
                k += 1;
            }
            if at(s, k) == b')' {
                used = k + 1;
            }
        }
        let v = if neg { -f64::NAN } else { f64::NAN };
        return Conv { value: v, used, overflow: false };
    }
    if at(s, i) == b'0'
        && lower(i + 1) == b'x'
        && let Some((v, used)) = scan_hex_float(s, i + 2)
    {
        return Conv { value: if neg { -v } else { v }, used, overflow: v.is_infinite() };
    }
    let digits_start = i;
    let mut saw_digit = false;
    while at(s, i).is_ascii_digit() {
        i += 1;
        saw_digit = true;
    }
    if at(s, i) == b'.' {
        i += 1;
        while at(s, i).is_ascii_digit() {
            i += 1;
            saw_digit = true;
        }
    }
    if !saw_digit {
        return Conv { value: 0.0, used: 0, overflow: false };
    }
    let mut end = i;
    if lower(i) == b'e' {
        let mut k = i + 1;
        if at(s, k) == b'-' || at(s, k) == b'+' {
            k += 1;
        }
        if at(s, k).is_ascii_digit() {
            while at(s, k).is_ascii_digit() {
                k += 1;
            }
            end = k;
        }
    }
    let text = String::from_utf8_lossy(&s[digits_start..end]).into_owned();
    let text = if text.starts_with('.') { format!("0{text}") } else { text };
    let v: f64 = text.parse().unwrap_or(0.0);
    Conv { value: if neg { -v } else { v }, used: end, overflow: v.is_infinite() }
}

fn scan_hex_float(s: &[u8], mut i: usize) -> Option<(f64, usize)> {
    let mut mant: f64 = 0.0;
    let mut exp: i32 = 0;
    let mut saw = false;
    while let Some(d) = hex_value(at(s, i)) {
        mant = mant * 16.0 + f64::from(d);
        i += 1;
        saw = true;
    }
    if at(s, i) == b'.' {
        i += 1;
        while let Some(d) = hex_value(at(s, i)) {
            mant = mant * 16.0 + f64::from(d);
            exp -= 4;
            i += 1;
            saw = true;
        }
    }
    if !saw {
        return None;
    }
    if (at(s, i) | 0x20) == b'p' {
        let c = strtol(&s[i + 1..], 10);
        if c.used > 0 && !at(s, i + 1).is_ascii_whitespace() {
            exp = exp.saturating_add(c.value.clamp(-100_000, 100_000) as i32);
            i += 1 + c.used;
        }
    }
    Some((mant * 2f64.powi(exp), i))
}

/// `strtof`: o `strtod` arredondado pra `float`.
pub fn strtof(s: &[u8]) -> Conv<f32> {
    let c = strtod(s);
    Conv {
        value: c.value as f32,
        used: c.used,
        overflow: c.overflow || (c.value.is_finite() && (c.value as f32).is_infinite()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        for c in [b' ', b'\t', b'\n', 0x0b, 0x0c, b'\r'] {
            assert!(is_space(c));
        }
        assert!(!is_space(0xa0));
        assert!(is_blank(b' ') && is_blank(b'\t') && !is_blank(b'\n'));
        assert!(is_print(b'~') && !is_print(0x7f) && !is_print(0xc3));
        assert_eq!(hex_value(b'f'), Some(15));
        assert_eq!(hex_value(b'g'), None);
        assert_eq!(digit_value(b'z'), Some(35));
    }

    #[test]
    fn cstrings() {
        assert_eq!(cstr(b"ab\0cd"), b"ab");
        assert_eq!(cstr(b"abc"), b"abc");
        assert_eq!(cstr(b""), b"");
        assert_eq!(cstr(b"\0x"), b"");
        assert_eq!(cstr_at(b"\0ab\0cd", 1), b"ab");
        assert_eq!(cstr_at(b"\0ab\0cd", 4), b"cd");
        assert_eq!(cstr_at(b"ab", 2), b"");
        assert_eq!(cstr_at(b"ab", 9), b"");
        assert_eq!(at(b"ab", 1), b'b');
        assert_eq!(at(b"ab", 2), 0);
    }

    #[test]
    fn strtol_bases_and_prefixes() {
        assert_eq!(strtol(b"0x1f rest", 0), Conv { value: 31, used: 4, overflow: false });
        assert_eq!(strtol(b"017", 0).value, 15);
        assert_eq!(strtol(b"-12x", 0), Conv { value: -12, used: 3, overflow: false });
        assert_eq!(strtol(b"  +7", 0).value, 7);
        assert_eq!(strtol(b"x", 0).used, 0);
        // "0x" sem dígito hexadecimal: converte só o zero.
        assert_eq!(strtol(b"0xg", 0), Conv { value: 0, used: 1, overflow: false });
        assert!(strtol(b"99999999999999999999", 10).overflow);
        assert_eq!(strtol(b"-9223372036854775808", 10), Conv { value: i64::MIN, used: 20, overflow: false });
        assert_eq!(strtol(b"9223372036854775808", 10), Conv { value: i64::MAX, used: 19, overflow: true });
        assert_eq!(strtol(b"ff", 16).value, 255);
        assert_eq!(strtol(b"0xff", 16).value, 255);
    }

    #[test]
    fn strtoull_wraps_negative() {
        assert_eq!(strtoull(b"-1", 0).value, u64::MAX);
        assert_eq!(strtoull(b"0xffffffffffffffff", 0).value, u64::MAX);
        assert!(strtoull(b"0x1ffffffffffffffff", 0).overflow);
    }

    #[test]
    fn fixed_base_ignores_prefix() {
        assert_eq!(strtoull_fixed_base(b"0x1", 16), Conv { value: 0, used: 1, overflow: false });
        assert_eq!(strtoull_fixed_base(b"  -7", 10).value, 7u64.wrapping_neg());
        assert_eq!(strtoull_fixed_base(b"z", 10), Conv { value: 0, used: 0, overflow: false });
        assert!(strtoull_fixed_base(b"99999999999999999999", 10).overflow);
    }

    #[test]
    fn whole_strings() {
        assert_eq!(strtol_whole(b" -12", 10), WholeLong::Ok(-12));
        assert_eq!(strtol_whole(b"+5", 10), WholeLong::Ok(5));
        assert_eq!(strtol_whole(b"5x", 10), WholeLong::Invalid);
        assert_eq!(strtol_whole(b"", 10), WholeLong::Invalid);
        assert_eq!(strtol_whole(b"+", 10), WholeLong::Invalid);
        assert_eq!(strtol_whole(b"99999999999999999999", 10), WholeLong::Range(i64::MAX));
        assert_eq!(strtol_whole(b"-99999999999999999999", 10), WholeLong::Range(i64::MIN));
        assert_eq!(strtol_whole(b"-9223372036854775808", 10), WholeLong::Ok(i64::MIN));
    }

    #[test]
    fn decimal_strict() {
        assert_eq!(parse_decimal(b"42"), Some(42));
        assert_eq!(parse_decimal(b"007"), Some(7));
        assert_eq!(parse_decimal(b""), None);
        assert_eq!(parse_decimal(b"+1"), None);
        assert_eq!(parse_decimal(b" 1"), None);
        assert_eq!(parse_decimal(b"1 "), None);
        assert_eq!(parse_decimal(b"18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_decimal(b"18446744073709551616"), None);
    }

    #[test]
    fn signed_decimal_strict() {
        assert_eq!(parse_i64(b"-5"), Some(-5));
        assert_eq!(parse_i64(b"+5"), Some(5));
        assert_eq!(parse_i64(b"5"), Some(5));
        assert_eq!(parse_i64(b"-"), None);
        assert_eq!(parse_i64(b"--5"), None);
        assert_eq!(parse_i64(b" 5"), None);
        assert_eq!(parse_i64(b"-9223372036854775808"), Some(i64::MIN));
        assert_eq!(parse_i64(b"9223372036854775808"), None);
        assert_eq!(parse_i64(b"-9223372036854775809"), None);
        assert_eq!(parse_decimal_usize(b"12"), Some(12));
        assert_eq!(parse_decimal_usize(b"x"), None);
    }

    #[test]
    fn strtod_forms() {
        assert_eq!(strtod(b"1.5e2x").value, 150.0);
        assert_eq!(strtod(b"1.5e2x").used, 5);
        assert_eq!(strtod(b".5").value, 0.5);
        assert_eq!(strtod(b"1e").used, 1);
        assert!(strtod(b"inf").value.is_infinite());
        assert!(strtod(b"nan").value.is_nan());
        assert_eq!(strtod(b"0x1p4").value, 16.0);
        assert_eq!(strtod(b"abc").used, 0);
    }
}
