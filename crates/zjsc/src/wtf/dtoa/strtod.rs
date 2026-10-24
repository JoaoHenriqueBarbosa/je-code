// Tradução de WTF/wtf/dtoa/strtod.h e strtod.cc (double-conversion).

use crate::wtf::dtoa::bignum::Bignum;
use crate::wtf::dtoa::cached_powers::PowersOfTenCache;
use crate::wtf::dtoa::diy_fp::DiyFp;
use crate::wtf::dtoa::ieee::{Double, Single};

// `DOUBLE_CONVERSION_CORRECT_DOUBLE_OPERATIONS` vale em x86_64 (ver utils.h), então os ramos
// sob essa macro são os que existem aqui.

// 2^53 = 9007199254740992.
// Qualquer inteiro com no máximo 15 dígitos decimais cabe num double (que tem significando de
// 53 bits) sem perda de precisão.
const K_MAX_EXACT_DOUBLE_INTEGER_DECIMAL_DIGITS: i32 = 15;
// 2^64 = 18446744073709551616 > 10^19
const K_MAX_UINT64_DECIMAL_DIGITS: i32 = 19;

// Double máximo: 1.7976931348623157 x 10^308
// Menor double não nulo: 4.9406564584124654 x 10^-324
// Qualquer x >= 10^309 é lido como +infinito.
// Qualquer x <= 10^-324 é lido como 0.
// Note que 2.5e-324 (apesar de menor que o menor double) é lido como não nulo (igual ao
// menor double não nulo).
const K_MAX_DECIMAL_POWER: i32 = 309;
const K_MIN_DECIMAL_POWER: i32 = -324;

// 2^64 = 18446744073709551616
const K_MAX_UINT64: u64 = 0xFFFF_FFFF_FFFF_FFFF;

static EXACT_POWERS_OF_TEN: [f64; 23] = [
    1.0, // 10^0
    10.0,
    100.0,
    1000.0,
    10000.0,
    100000.0,
    1000000.0,
    10000000.0,
    100000000.0,
    1000000000.0,
    10000000000.0, // 10^10
    100000000000.0,
    1000000000000.0,
    10000000000000.0,
    100000000000000.0,
    1000000000000000.0,
    10000000000000000.0,
    100000000000000000.0,
    1000000000000000000.0,
    10000000000000000000.0,
    100000000000000000000.0, // 10^20
    1000000000000000000000.0,
    // 10^22 = 0x21e19e0c9bab2400000 = 0x878678326eac9 * 2^22
    10000000000000000000000.0,
];
const K_EXACT_POWERS_OF_TEN_SIZE: i32 = EXACT_POWERS_OF_TEN.len() as i32;

// Número máximo de dígitos significativos na representação decimal.
// Na verdade o valor é 772 (ver conversions.cc), mas para dar margem arredondamos para 780.
const K_MAX_SIGNIFICANT_DECIMAL_DIGITS: usize = 780;

fn trim_leading_zeros(buffer: &[u8]) -> &[u8] {
    for i in 0..buffer.len() {
        if buffer[i] != b'0' {
            return &buffer[i..buffer.len()];
        }
    }
    &buffer[..0]
}

fn trim_trailing_zeros(buffer: &[u8]) -> &[u8] {
    let mut i: i32 = buffer.len() as i32 - 1;
    while i >= 0 {
        if buffer[i as usize] != b'0' {
            return &buffer[0..(i + 1) as usize];
        }
        i -= 1;
    }
    &buffer[..0]
}

fn cut_to_max_significant_digits(
    buffer: &[u8],
    exponent: i32,
    significant_buffer: &mut [u8],
    significant_exponent: &mut i32,
) {
    for i in 0..(K_MAX_SIGNIFICANT_DECIMAL_DIGITS - 1) {
        significant_buffer[i] = buffer[i];
    }
    // O buffer de entrada foi aparado. Portanto o último dígito é diferente de '0'.
    debug_assert!(buffer[buffer.len() - 1] != b'0');
    // Põe o último dígito como não nulo. Isso basta para garantir o arredondamento correto.
    significant_buffer[K_MAX_SIGNIFICANT_DECIMAL_DIGITS - 1] = b'1';
    *significant_exponent =
        exponent + (buffer.len() as i32 - K_MAX_SIGNIFICANT_DECIMAL_DIGITS as i32);
}

/// Apara o buffer e o corta em no máximo `K_MAX_SIGNIFICANT_DECIMAL_DIGITS`. Se possível, o
/// buffer de entrada é reaproveitado; se precisa ser modificado (por causa do corte), a
/// entrada é copiada para `buffer_copy_space`.
fn trim_and_cut<'a>(
    buffer: &'a [u8],
    mut exponent: i32,
    buffer_copy_space: &'a mut [u8],
    space_size: i32,
    trimmed: &mut &'a [u8],
    updated_exponent: &mut i32,
) {
    let left_trimmed = trim_leading_zeros(buffer);
    let right_trimmed = trim_trailing_zeros(left_trimmed);
    exponent += (left_trimmed.len() - right_trimmed.len()) as i32;
    if right_trimmed.len() > K_MAX_SIGNIFICANT_DECIMAL_DIGITS {
        let _ = space_size; // Marca a variável como usada.
        debug_assert!(space_size >= K_MAX_SIGNIFICANT_DECIMAL_DIGITS as i32);
        cut_to_max_significant_digits(
            right_trimmed,
            exponent,
            &mut *buffer_copy_space,
            updated_exponent,
        );
        let shared: &'a [u8] = buffer_copy_space;
        *trimmed = &shared[..K_MAX_SIGNIFICANT_DECIMAL_DIGITS];
    } else {
        *trimmed = right_trimmed;
        *updated_exponent = exponent;
    }
}

/// Lê dígitos do buffer e os converte para um uint64. Lê quantos dígitos couberem num uint64.
/// Quando a string começa com "1844674407370955161" nenhum outro dígito é lido. Como
/// 2^64 = 18446744073709551616, ainda seria possível ler mais um dígito se ele fosse <= 6,
/// mas isso complicaria o código.
fn read_uint64(buffer: &[u8], number_of_read_digits: &mut usize) -> u64 {
    let mut result: u64 = 0;
    let mut i: usize = 0;
    while i < buffer.len() && result <= (K_MAX_UINT64 / 10 - 1) {
        let digit: i32 = buffer[i] as i32 - b'0' as i32;
        i += 1;
        debug_assert!(0 <= digit && digit <= 9);
        result = result.wrapping_mul(10).wrapping_add(digit as u64);
    }
    *number_of_read_digits = i;
    result
}

/// Lê um `DiyFp` do buffer. O `DiyFp` devolvido não é necessariamente normalizado.
/// Se `remaining_decimals` é zero, o `DiyFp` devolvido é exato. Do contrário foi arredondado
/// e tem erro de no máximo 1/2 ulp.
fn read_diy_fp(buffer: &[u8], result: &mut DiyFp, remaining_decimals: &mut i32) {
    let mut read_digits: usize = 0;
    let mut significand: u64 = read_uint64(buffer, &mut read_digits);
    if buffer.len() == read_digits {
        *result = DiyFp::new(significand, 0);
        *remaining_decimals = 0;
    } else {
        // Arredonda o significando.
        if buffer[read_digits] >= b'5' {
            significand = significand.wrapping_add(1);
        }
        // Calcula o expoente binário.
        let exponent: i32 = 0;
        *result = DiyFp::new(significand, exponent);
        *remaining_decimals = (buffer.len() - read_digits) as i32;
    }
}

fn double_strtod(trimmed: &[u8], exponent: i32, result: &mut f64) -> bool {
    if trimmed.len() as i32 <= K_MAX_EXACT_DOUBLE_INTEGER_DECIMAL_DIGITS {
        let mut read_digits: usize = 0;
        // O trimmed de entrada cabe num double. Se 10^exponent (resp. 10^-exponent) também
        // cabe num double, o double do resultado sai simplesmente multiplicando (resp.
        // dividindo) os dois números. Isso é possível porque o IEEE garante que as operações
        // de ponto flutuante devolvem a melhor aproximação possível.
        if exponent < 0 && -exponent < K_EXACT_POWERS_OF_TEN_SIZE {
            // 10^-exponent cabe num double.
            *result = read_uint64(trimmed, &mut read_digits) as f64;
            debug_assert!(read_digits == trimmed.len());
            *result /= EXACT_POWERS_OF_TEN[(-exponent) as usize];
            return true;
        }
        if 0 <= exponent && exponent < K_EXACT_POWERS_OF_TEN_SIZE {
            // 10^exponent cabe num double.
            *result = read_uint64(trimmed, &mut read_digits) as f64;
            debug_assert!(read_digits == trimmed.len());
            *result *= EXACT_POWERS_OF_TEN[exponent as usize];
            return true;
        }
        let remaining_digits: i32 = K_MAX_EXACT_DOUBLE_INTEGER_DECIMAL_DIGITS - trimmed.len() as i32;
        if 0 <= exponent && exponent - remaining_digits < K_EXACT_POWERS_OF_TEN_SIZE {
            // A string aparada era curta e podemos multiplicá-la por 10^remaining_digits.
            // Com isso o expoente restante também cabe num double.
            *result = read_uint64(trimmed, &mut read_digits) as f64;
            debug_assert!(read_digits == trimmed.len());
            *result *= EXACT_POWERS_OF_TEN[remaining_digits as usize];
            *result *= EXACT_POWERS_OF_TEN[(exponent - remaining_digits) as usize];
            return true;
        }
    }
    false
}

/// Devolve 10^exponent como um `DiyFp` exato.
/// O expoente dado precisa estar no intervalo [1; K_DECIMAL_EXPONENT_DISTANCE[.
fn adjustment_power_of_ten(exponent: i32) -> DiyFp {
    debug_assert!(0 < exponent);
    debug_assert!(exponent < PowersOfTenCache::K_DECIMAL_EXPONENT_DISTANCE);
    // Fixa as potências restantes para a distância de expoente decimal dada.
    debug_assert!(PowersOfTenCache::K_DECIMAL_EXPONENT_DISTANCE == 8);
    match exponent {
        1 => DiyFp::new(0xa000_0000_0000_0000, -60),
        2 => DiyFp::new(0xc800_0000_0000_0000, -57),
        3 => DiyFp::new(0xfa00_0000_0000_0000, -54),
        4 => DiyFp::new(0x9c40_0000_0000_0000, -50),
        5 => DiyFp::new(0xc350_0000_0000_0000, -47),
        6 => DiyFp::new(0xf424_0000_0000_0000, -44),
        7 => DiyFp::new(0x9896_8000_0000_0000, -40),
        _ => unreachable!(),
    }
}

/// Se a função devolve true, o resultado é o double correto. Do contrário é o double correto
/// ou o double logo abaixo do correto.
fn diy_fp_strtod(buffer: &[u8], mut exponent: i32, result: &mut f64) -> bool {
    let mut input = DiyFp::default();
    let mut remaining_decimals: i32 = 0;
    read_diy_fp(buffer, &mut input, &mut remaining_decimals);
    // Como podemos ter descartado dígitos, a entrada não é exata. Se remaining_decimals é
    // diferente de 0, o erro é de no máximo .5 ulp (unidade na última casa). Não queremos
    // lidar com frações, então mantemos um denominador comum.
    const K_DENOMINATOR_LOG: i32 = 3;
    const K_DENOMINATOR: i32 = 1 << K_DENOMINATOR_LOG;
    // Move as casas decimais restantes para o expoente.
    exponent += remaining_decimals;
    let mut error: u64 = if remaining_decimals == 0 {
        0
    } else {
        (K_DENOMINATOR / 2) as u64
    };

    let mut old_e: i32 = input.e();
    input.normalize();
    error <<= (old_e - input.e()) as u32;

    debug_assert!(exponent <= PowersOfTenCache::K_MAX_DECIMAL_EXPONENT);
    if exponent < PowersOfTenCache::K_MIN_DECIMAL_EXPONENT {
        *result = 0.0;
        return true;
    }
    let mut cached_power = DiyFp::default();
    let mut cached_decimal_exponent: i32 = 0;
    PowersOfTenCache::get_cached_power_for_decimal_exponent(
        exponent,
        &mut cached_power,
        &mut cached_decimal_exponent,
    );

    if cached_decimal_exponent != exponent {
        let adjustment_exponent: i32 = exponent - cached_decimal_exponent;
        let adjustment_power: DiyFp = adjustment_power_of_ten(adjustment_exponent);
        input.multiply(&adjustment_power);
        if K_MAX_UINT64_DECIMAL_DIGITS - buffer.len() as i32 >= adjustment_exponent {
            // O produto de input com a potência de ajuste cabe num inteiro de 64 bits.
            debug_assert!(DiyFp::K_SIGNIFICAND_SIZE == 64);
        } else {
            // A potência de ajuste é exata. Há portanto só um erro de 0.5.
            error = error.wrapping_add((K_DENOMINATOR / 2) as u64);
        }
    }

    input.multiply(&cached_power);
    // O erro introduzido por uma multiplicação a*b é
    //   error_a + error_b + error_a*error_b/2^64 + 0.5
    // Substituindo a por 'input' e b por 'cached_power':
    //   error_b = 0.5  (todas as potências em cache têm erro menor que 0.5 ulp),
    //   error_ab = 0 ou 1 / kDenominator > error_a*error_b/ 2^64
    let error_b: i32 = K_DENOMINATOR / 2;
    let error_ab: i32 = if error == 0 { 0 } else { 1 }; // Arredondamos para 1.
    let fixed_error: i32 = K_DENOMINATOR / 2;
    error = error.wrapping_add((error_b + error_ab + fixed_error) as u64);

    old_e = input.e();
    input.normalize();
    error <<= (old_e - input.e()) as u32;

    // Vê se o significando do double muda se somarmos/subtrairmos o erro.
    let order_of_magnitude: i32 = DiyFp::K_SIGNIFICAND_SIZE + input.e();
    let effective_significand_size: i32 =
        Double::significand_size_for_order_of_magnitude(order_of_magnitude);
    let mut precision_digits_count: i32 = DiyFp::K_SIGNIFICAND_SIZE - effective_significand_size;
    if precision_digits_count + K_DENOMINATOR_LOG >= DiyFp::K_SIGNIFICAND_SIZE {
        // Isso só acontece com denormais muito pequenos. Nesse caso o meio-caminho
        // multiplicado pelo denominador excede o intervalo de um uint64. Simplesmente
        // desloca tudo para a direita.
        let shift_amount: i32 =
            (precision_digits_count + K_DENOMINATOR_LOG) - DiyFp::K_SIGNIFICAND_SIZE + 1;
        input.set_f(input.f() >> shift_amount as u32);
        input.set_e(input.e() + shift_amount);
        // Somamos 1 pela precisão perdida do erro, e kDenominator pela precisão perdida de
        // input.f().
        error = (error >> shift_amount as u32)
            .wrapping_add(1)
            .wrapping_add(K_DENOMINATOR as u64);
        precision_digits_count -= shift_amount;
    }
    // Agora usamos uint64_ts. Isso só funciona se o `DiyFp` também usa uint64_ts.
    debug_assert!(DiyFp::K_SIGNIFICAND_SIZE == 64);
    debug_assert!(precision_digits_count < 64);
    let one64: u64 = 1;
    let precision_bits_mask: u64 = (one64 << precision_digits_count as u32).wrapping_sub(1);
    let mut precision_bits: u64 = input.f() & precision_bits_mask;
    let mut half_way: u64 = one64 << (precision_digits_count - 1) as u32;
    precision_bits = precision_bits.wrapping_mul(K_DENOMINATOR as u64);
    half_way = half_way.wrapping_mul(K_DENOMINATOR as u64);
    let mut rounded_input = DiyFp::new(
        input.f() >> precision_digits_count as u32,
        input.e() + precision_digits_count,
    );
    if precision_bits >= half_way.wrapping_add(error) {
        rounded_input.set_f(rounded_input.f().wrapping_add(1));
    }
    // Se os últimos bits estão perto demais do caso de meio-caminho, somos imprecisos demais
    // e arredondamos para baixo. Nesse caso devolvemos false para recorrer a um algoritmo
    // mais preciso.

    *result = Double::from_diy_fp(rounded_input).value();
    if half_way.wrapping_sub(error) < precision_bits
        && precision_bits < half_way.wrapping_add(error)
    {
        // Impreciso demais. O chamador terá de recorrer a uma versão mais lenta. Porém o
        // número devolvido é garantidamente o double correto ou o double logo abaixo.
        false
    } else {
        true
    }
}

/// Devolve
///   - -1 se buffer*10^exponent < diy_fp.
///   -  0 se buffer*10^exponent == diy_fp.
///   - +1 se buffer*10^exponent > diy_fp.
/// Pré-condições:
///   buffer.length() + exponent <= K_MAX_DECIMAL_POWER + 1
///   buffer.length() + exponent > K_MIN_DECIMAL_POWER
///   buffer.length() <= K_MAX_SIGNIFICANT_DECIMAL_DIGITS
fn compare_buffer_with_diy_fp(buffer: &[u8], exponent: i32, diy_fp: DiyFp) -> i32 {
    debug_assert!(buffer.len() as i32 + exponent <= K_MAX_DECIMAL_POWER + 1);
    debug_assert!(buffer.len() as i32 + exponent > K_MIN_DECIMAL_POWER);
    debug_assert!(buffer.len() <= K_MAX_SIGNIFICANT_DECIMAL_DIGITS);
    // Garante que o Bignum comporta todos os nossos números. Nosso Bignum tem um campo
    // separado para expoentes. Os deslocamentos consomem no máximo um bigit (< 64 bits).
    // ln(10) == 3.3219...
    debug_assert!(((K_MAX_DECIMAL_POWER + 1) * 333 / 100) < Bignum::K_MAX_SIGNIFICANT_BITS);
    let mut buffer_bignum = Bignum::new();
    let mut diy_fp_bignum = Bignum::new();
    buffer_bignum.assign_decimal_string(buffer);
    diy_fp_bignum.assign_uint64(diy_fp.f());
    if exponent >= 0 {
        buffer_bignum.multiply_by_power_of_ten(exponent);
    } else {
        diy_fp_bignum.multiply_by_power_of_ten(-exponent);
    }
    if diy_fp.e() > 0 {
        diy_fp_bignum.shift_left(diy_fp.e());
    } else {
        buffer_bignum.shift_left(-diy_fp.e());
    }
    Bignum::compare(&buffer_bignum, &diy_fp_bignum)
}

/// Devolve true se o palpite é o double correto. Devolve false quando o palpite é o correto
/// ou o double logo abaixo.
fn compute_guess(trimmed: &[u8], exponent: i32, guess: &mut f64) -> bool {
    if trimmed.is_empty() {
        *guess = 0.0;
        return true;
    }
    // No C++ `exponent + trimmed.length() - 1` é aritmética sem sinal de size_t (length() é
    // size_t): o expoente é estendido com sinal para 64 bits e a conta dá a volta.
    if (exponent as i64 as u64)
        .wrapping_add(trimmed.len() as u64)
        .wrapping_sub(1)
        >= K_MAX_DECIMAL_POWER as u64
    {
        *guess = Double::infinity();
        return true;
    }
    if exponent + trimmed.len() as i32 <= K_MIN_DECIMAL_POWER {
        *guess = 0.0;
        return true;
    }

    if double_strtod(trimmed, exponent, guess) || diy_fp_strtod(trimmed, exponent, guess) {
        return true;
    }
    if *guess == Double::infinity() {
        return true;
    }
    false
}

/// O buffer só pode conter dígitos no intervalo [0-9]. Não pode conter ponto nem sinal.
/// Não pode começar com '0' nem ser vazio.
pub fn strtod(buffer: &[u8], mut exponent: i32) -> f64 {
    let mut copy_buffer = [0u8; K_MAX_SIGNIFICANT_DECIMAL_DIGITS];
    let mut trimmed: &[u8] = &[];
    let mut updated_exponent: i32 = 0;
    trim_and_cut(
        buffer,
        exponent,
        &mut copy_buffer,
        K_MAX_SIGNIFICANT_DECIMAL_DIGITS as i32,
        &mut trimmed,
        &mut updated_exponent,
    );
    exponent = updated_exponent;

    let mut guess: f64 = 0.0;
    let is_correct = compute_guess(trimmed, exponent, &mut guess);
    if is_correct {
        return guess;
    }

    let upper_boundary: DiyFp = Double::from_f64(guess).upper_boundary();
    let comparison = compare_buffer_with_diy_fp(trimmed, exponent, upper_boundary);
    if comparison < 0 {
        guess
    } else if comparison > 0 {
        Double::from_f64(guess).next_double()
    } else if (Double::from_f64(guess).significand() & 1) == 0 {
        // Arredonda para o par.
        guess
    } else {
        Double::from_f64(guess).next_double()
    }
}

fn sanitized_doubletof(d: f64) -> f32 {
    debug_assert!(d >= 0.0);
    // O ASAN tem uma checagem que proíbe converter doubles em floats se forem grandes demais.
    // O comportamento é coberto pelo IEEE 754, mas alguns projetos usam essa flag.
    let max_finite: f32 = 3.4028234663852885981170418348451692544e+38_f32;
    // O ponto médio entre o maior finito e o infinito. Como o infinito tem significando par,
    // tudo igual ou maior que esse valor vira infinito.
    let half_max_finite_infinity: f64 = 3.40282356779733661637539395458142568448e+38_f64;
    if d >= max_finite as f64 {
        if d >= half_max_finite_infinity {
            return Single::infinity();
        }
        return max_finite;
    }
    d as f32
}

/// O buffer só pode conter dígitos no intervalo [0-9]. Não pode conter ponto nem sinal.
/// Não pode começar com '0' nem ser vazio.
pub fn strtof(buffer: &[u8], mut exponent: i32) -> f32 {
    let mut copy_buffer = [0u8; K_MAX_SIGNIFICANT_DECIMAL_DIGITS];
    let mut trimmed: &[u8] = &[];
    let mut updated_exponent: i32 = 0;
    trim_and_cut(
        buffer,
        exponent,
        &mut copy_buffer,
        K_MAX_SIGNIFICANT_DECIMAL_DIGITS as i32,
        &mut trimmed,
        &mut updated_exponent,
    );
    exponent = updated_exponent;

    let mut double_guess: f64 = 0.0;
    let is_correct = compute_guess(trimmed, exponent, &mut double_guess);

    let float_guess: f32 = sanitized_doubletof(double_guess);
    if float_guess as f64 == double_guess {
        // Este atalho vale para valores inteiros.
        return float_guess;
    }

    // Precisamos pegar o arredondamento duplo. Digamos que o double foi arredondado para
    // cima, virou o limite de um float, e arredonda para cima de novo. Por isso olhamos
    // também o anterior.
    // Exemplo (em números decimais):
    //    entrada: 12349
    //    alta precisão (4 dígitos): 1235
    //    baixa precisão (3 dígitos):
    //       lido da entrada: 123
    //       arredondado da alta precisão: 124.
    // Para isso olhamos os vizinhos do resultado correto e vemos se arredondariam para o
    // mesmo float. Se o palpite não é correto, olhamos quatro valores (pois dois doubles
    // diferentes poderiam ser o correto).

    let double_next: f64 = Double::from_f64(double_guess).next_double();
    let double_previous: f64 = Double::from_f64(double_guess).previous_double();

    let f1: f32 = sanitized_doubletof(double_previous);
    let f2: f32 = float_guess;
    let f3: f32 = sanitized_doubletof(double_next);
    let f4: f32;
    if is_correct {
        f4 = f3;
    } else {
        let double_next2: f64 = Double::from_f64(double_next).next_double();
        f4 = sanitized_doubletof(double_next2);
    }
    let _ = f2; // Marca a variável como usada.
    debug_assert!(f1 <= f2 && f2 <= f3 && f3 <= f4);

    // Se o palpite não está perto de um limite de precisão simples, basta devolver o seu
    // valor float.
    if f1 == f4 {
        return float_guess;
    }

    debug_assert!(
        (f1 != f2 && f2 == f3 && f3 == f4)
            || (f1 == f2 && f2 != f3 && f3 == f4)
            || (f1 == f2 && f2 == f3 && f3 != f4)
    );

    // guess e next são os dois candidatos possíveis (do mesmo modo que double_guess era o
    // candidato inferior para um palpite de precisão dupla).
    let guess: f32 = f1;
    let next: f32 = f4;
    let upper_boundary: DiyFp;
    if guess == 0.0f32 {
        let min_float: f32 = 1e-45_f32;
        upper_boundary = Double::from_f64(min_float as f64 / 2.0).as_diy_fp();
    } else {
        upper_boundary = Single::from_f32(guess).upper_boundary();
    }
    let comparison = compare_buffer_with_diy_fp(trimmed, exponent, upper_boundary);
    if comparison < 0 {
        guess
    } else if comparison > 0 {
        next
    } else if (Single::from_f32(guess).significand() & 1) == 0 {
        // Arredonda para o par.
        guess
    } else {
        next
    }
}
