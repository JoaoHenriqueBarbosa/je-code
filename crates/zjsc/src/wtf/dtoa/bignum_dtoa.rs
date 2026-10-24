// Tradução de WTF/wtf/dtoa/bignum-dtoa.h e bignum-dtoa.cc (double-conversion).

use crate::wtf::dtoa::bignum::Bignum;
use crate::wtf::dtoa::ieee::{Double, Single};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BignumDtoaMode {
    /// Devolve a representação correta mais curta. Por exemplo, a saída de
    /// 0.299999999999999988897 é (a menos exata, mas correta) 0.3.
    BIGNUM_DTOA_SHORTEST,
    /// Igual a `BIGNUM_DTOA_SHORTEST`, mas para floats de precisão simples.
    BIGNUM_DTOA_SHORTEST_SINGLE,
    /// Devolve um número fixo de dígitos depois da vírgula decimal.
    /// Por exemplo, fixed(0.1, 4) vira 0.1000. Se o número é grande, a saída é grande.
    BIGNUM_DTOA_FIXED,
    /// Devolve um número fixo de dígitos, qualquer que seja o expoente.
    BIGNUM_DTOA_PRECISION,
}

fn normalized_exponent(mut significand: u64, mut exponent: i32) -> i32 {
    debug_assert!(significand != 0);
    while (significand & Double::K_HIDDEN_BIT) == 0 {
        significand <<= 1;
        exponent -= 1;
    }
    exponent
}

/// Converte o double `v` para ASCII. O resultado se interpreta como
/// `buffer * 10^(point-length)`. O buffer termina em zero.
///
/// A entrada `v` precisa ser > 0 e diferente de NaN e de infinito.
///
/// A saída depende do modo:
///  - SHORTEST: produz o menor número de dígitos para o qual a identidade interna ainda vale.
///    O parâmetro `requested_digits` é ignorado.
///  - FIXED: produz os dígitos necessários para imprimir o número com `requested_digits`
///    dígitos depois da vírgula. Meios exatos arredondam para cima.
///  - PRECISION: produz `requested_digits` dígitos, o primeiro diferente de '0'. Meios exatos
///    arredondam para cima.
/// `bignum_dtoa` espera um buffer grande o bastante para todos os dígitos e o zero final.
pub fn bignum_dtoa(
    v: f64,
    mode: BignumDtoaMode,
    requested_digits: i32,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_point: &mut i32,
) {
    debug_assert!(v > 0.0);
    debug_assert!(!Double::from_f64(v).is_special());
    let significand: u64;
    let exponent: i32;
    let lower_boundary_is_closer: bool;
    if mode == BignumDtoaMode::BIGNUM_DTOA_SHORTEST_SINGLE {
        let f: f32 = v as f32;
        debug_assert!(f as f64 == v);
        significand = Single::from_f32(f).significand() as u64;
        exponent = Single::from_f32(f).exponent();
        lower_boundary_is_closer = Single::from_f32(f).lower_boundary_is_closer();
    } else {
        significand = Double::from_f64(v).significand();
        exponent = Double::from_f64(v).exponent();
        lower_boundary_is_closer = Double::from_f64(v).lower_boundary_is_closer();
    }
    let need_boundary_deltas = mode == BignumDtoaMode::BIGNUM_DTOA_SHORTEST
        || mode == BignumDtoaMode::BIGNUM_DTOA_SHORTEST_SINGLE;

    let is_even = (significand & 1) == 0;
    let normalized_exponent = normalized_exponent(significand, exponent);
    // estimated_power pode estar baixo por 1.
    let estimated_power = estimate_power(normalized_exponent);

    // Atalho para Fixed.
    // Os dígitos pedidos correspondem aos dígitos depois da vírgula. Se o número é pequeno
    // demais, não há por que tentar gerar dígitos.
    if mode == BignumDtoaMode::BIGNUM_DTOA_FIXED && -estimated_power - 1 > requested_digits {
        buffer[0] = 0;
        *length = 0;
        // Põe decimal_point em -requested_digits. É o que Gay faz. Não deve ter efeito algum,
        // pois a string é vazia.
        *decimal_point = -requested_digits;
        return;
    }

    let mut numerator = Bignum::new();
    let mut denominator = Bignum::new();
    let mut delta_minus = Bignum::new();
    let mut delta_plus = Bignum::new();
    // Garante que o bignum cresce o bastante. O menor double vale 4e-324; o denominador
    // precisa de menos de 324*4 dígitos binários. O maior double, 1.7976931348623157e308,
    // precisa de menos de 308*4.
    debug_assert!(Bignum::K_MAX_SIGNIFICANT_BITS >= 324 * 4);
    initial_scaled_start_values(
        significand,
        exponent,
        lower_boundary_is_closer,
        estimated_power,
        need_boundary_deltas,
        &mut numerator,
        &mut denominator,
        &mut delta_minus,
        &mut delta_plus,
    );
    // Agora v = (numerator / denominator) * 10^estimated_power.
    fixup_multiply_10(
        estimated_power,
        is_even,
        decimal_point,
        &mut numerator,
        &mut denominator,
        &mut delta_minus,
        &mut delta_plus,
    );
    // Agora v = (numerator / denominator) * 10^(decimal_point-1), e
    //  1 <= (numerator + delta_plus) / denominator < 10
    match mode {
        BignumDtoaMode::BIGNUM_DTOA_SHORTEST | BignumDtoaMode::BIGNUM_DTOA_SHORTEST_SINGLE => {
            generate_shortest_digits(
                &mut numerator,
                &denominator,
                &mut delta_minus,
                &mut delta_plus,
                is_even,
                buffer,
                length,
            );
        }
        BignumDtoaMode::BIGNUM_DTOA_FIXED => {
            bignum_to_fixed(
                requested_digits,
                decimal_point,
                &mut numerator,
                &mut denominator,
                buffer,
                length,
            );
        }
        BignumDtoaMode::BIGNUM_DTOA_PRECISION => {
            generate_counted_digits(
                requested_digits,
                decimal_point,
                &mut numerator,
                &denominator,
                buffer,
                length,
            );
        }
    }
    buffer[*length as usize] = 0;
}

/// O procedimento gera dígitos da esquerda para a direita e para quando os dígitos gerados
/// dão a menor representação decimal de v. Uma representação decimal de v é um número mais
/// próximo de v que de qualquer outro double, de modo que converte para v quando lida.
///
/// Isso vale se d, a representação decimal, está entre m- e m+, os limites inferior e
/// superior. d precisa estar estritamente entre eles se `!is_even`.
///           m- := (numerator - delta_minus) / denominator
///           m+ := (numerator + delta_plus) / denominator
///
/// Pré-condição: 0 <= (numerator+delta_plus) / denominator < 10.
///   Se 1 <= (numerator+delta_plus) / denominator < 10, nenhum dígito 0 à esquerda é gerado.
fn generate_shortest_digits(
    numerator: &mut Bignum,
    denominator: &Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
    is_even: bool,
    buffer: &mut [u8],
    length: &mut i32,
) {
    // Pequena otimização: se delta_minus e delta_plus são iguais, reaproveita um dos dois
    // bignums. No C++ delta_plus vira uma referência para delta_minus; aqui `same_deltas`
    // faz o mesmo papel e delta_plus não é mais lido nem escrito nesse caso.
    let same_deltas = Bignum::equal(delta_minus, delta_plus);
    *length = 0;
    loop {
        let digit: u16 = numerator.divide_modulo_int_bignum(denominator);
        debug_assert!(digit <= 9); // digit é uint16_t e portanto sempre positivo.
        // digit = numerator / denominator (divisão inteira).
        // numerator = numerator % denominator.
        buffer[*length as usize] = (digit as u8).wrapping_add(b'0');
        *length += 1;

        // Já dá para parar?
        // Se o resto da divisão é menor que a distância ao limite inferior, paramos
        // arredondando para baixo (descartando o resto). Igual para o arredondamento para
        // cima (com o limite superior).
        let in_delta_room_minus: bool;
        let in_delta_room_plus: bool;
        {
            let plus: &Bignum = if same_deltas { &*delta_minus } else { &*delta_plus };
            if is_even {
                in_delta_room_minus = Bignum::less_equal(numerator, delta_minus);
            } else {
                in_delta_room_minus = Bignum::less(numerator, delta_minus);
            }
            if is_even {
                in_delta_room_plus = Bignum::plus_compare(numerator, plus, denominator) >= 0;
            } else {
                in_delta_room_plus = Bignum::plus_compare(numerator, plus, denominator) > 0;
            }
        }
        if !in_delta_room_minus && !in_delta_room_plus {
            // Prepara a próxima iteração.
            numerator.times10();
            delta_minus.times10();
            // delta_plus foi otimizado para ser igual a delta_minus (se dividem o mesmo
            // valor). Então não se multiplica delta_plus se apontam para o mesmo objeto.
            if !same_deltas {
                delta_plus.times10();
            }
        } else if in_delta_room_minus && in_delta_room_plus {
            // Vê se 2*numerator < denominator. Se sim, o próximo dígito seria < 5 e dá para
            // arredondar para baixo.
            let compare = Bignum::plus_compare(numerator, numerator, denominator);
            let last = (*length - 1) as usize;
            if compare < 0 {
                // Os dígitos restantes são menores que .5. Arredonda para baixo (nada a fazer).
            } else if compare > 0 {
                // Os dígitos restantes passam de .5 do denominador. Arredonda para cima.
                // O último dígito não pode ser '9', senão o laço teria parado antes.
                debug_assert!(buffer[last] != b'9');
                buffer[last] = buffer[last].wrapping_add(1);
            } else {
                // Caso de meio exato. Arredonda para o par (é o que Gay parece fazer).
                if (buffer[last] - b'0') % 2 == 0 {
                    // Arredonda para baixo: nada a fazer.
                } else {
                    debug_assert!(buffer[last] != b'9');
                    buffer[last] = buffer[last].wrapping_add(1);
                }
            }
            return;
        } else if in_delta_room_minus {
            // Arredonda para baixo (nada a fazer).
            return;
        } else {
            // in_delta_room_plus: arredonda para cima. O último dígito não pode ser '9'.
            let last = (*length - 1) as usize;
            debug_assert!(buffer[last] != b'9');
            buffer[last] = buffer[last].wrapping_add(1);
            return;
        }
    }
}

/// Seja v = numerator / denominator < 10. Geramos `count` dígitos de d = x.xxxxx... (sem a
/// vírgula) da esquerda para a direita. Gerados os `count` dígitos, decide-se se arredonda
/// para cima ou para baixo. Restos de exatamente .5 arredondam para cima. Números como
/// 9.999999 propagam o vai-um até o fim e mudam o expoente (decimal_point) ao arredondar
/// para cima.
fn generate_counted_digits(
    count: i32,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &Bignum,
    buffer: &mut [u8],
    length: &mut i32,
) {
    debug_assert!(count >= 0);
    let mut i: i32 = 0;
    while i < count - 1 {
        let digit: u16 = numerator.divide_modulo_int_bignum(denominator);
        debug_assert!(digit <= 9);
        // digit = numerator / denominator (divisão inteira).
        // numerator = numerator % denominator.
        buffer[i as usize] = (digit as u8).wrapping_add(b'0');
        // Prepara a próxima iteração.
        numerator.times10();
        i += 1;
    }
    // Gera o último dígito.
    let mut digit: u16 = numerator.divide_modulo_int_bignum(denominator);
    if Bignum::plus_compare(numerator, numerator, denominator) >= 0 {
        digit += 1;
    }
    debug_assert!(digit <= 10);
    buffer[(count - 1) as usize] = (digit as u8).wrapping_add(b'0');
    // Corrige dígitos ruins (no caso de uma sequência de '9'). Propaga o vai-um até achar um
    // não '9' ou chegar ao primeiro dígito.
    let mut i: i32 = count - 1;
    while i > 0 {
        if buffer[i as usize] != b'0' + 10 {
            break;
        }
        buffer[i as usize] = b'0';
        buffer[(i - 1) as usize] = buffer[(i - 1) as usize].wrapping_add(1);
        i -= 1;
    }
    if buffer[0] == b'0' + 10 {
        // Propaga um vai-um acima da casa mais alta.
        buffer[0] = b'1';
        *decimal_point += 1;
    }
    *length = count;
}

/// Gera `requested_digits` depois da vírgula decimal. Pode omitir '0' finais. Se o número de
/// entrada é pequeno demais, nenhum dígito é gerado (ex.: 2 dígitos fixos para 0.00001).
///
/// A entrada satisfaz: 1 <= (numerator + delta) / denominator < 10.
fn bignum_to_fixed(
    requested_digits: i32,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    buffer: &mut [u8],
    length: &mut i32,
) {
    // Precisamos olhar mais que só os requested_digits, pois um número pode arredondar para
    // cima. Exemplo: v=0.5 com requested_digits=0. Mesmo que a potência de v seja 0, não dá
    // para parar aqui.
    if -*decimal_point > requested_digits {
        // O número é definitivamente pequeno demais. Ex: 0.001 com requested_digits == 1.
        // Põe decimal_point em -requested_digits. É o que Gay faz. Não deve ter efeito,
        // pois a string é vazia.
        *decimal_point = -requested_digits;
        *length = 0;
    } else if -*decimal_point == requested_digits {
        // Só precisamos conferir se o número arredonda para baixo ou para cima.
        // Ex: 0.04 e 0.06 com requested_digits == 1.
        debug_assert!(*decimal_point == -requested_digits);
        // De início a fração está no intervalo (1, 10]. Multiplica o denominador por 10 para
        // comparar com mais facilidade.
        denominator.times10();
        if Bignum::plus_compare(numerator, numerator, denominator) >= 0 {
            // Se a fração é >= 0.5, incluímos o dígito arredondado.
            buffer[0] = b'1';
            *length = 1;
            *decimal_point += 1;
        } else {
            // Já pegamos a maioria dos casos parecidos antes.
            *length = 0;
        }
    } else {
        // Os dígitos pedidos correspondem aos dígitos depois da vírgula. A variável
        // `needed_digits` inclui os dígitos antes da vírgula.
        let needed_digits = *decimal_point + requested_digits;
        generate_counted_digits(
            needed_digits,
            decimal_point,
            numerator,
            denominator,
            buffer,
            length,
        );
    }
}

/// Devolve uma estimativa de k tal que 10^(k-1) <= v < 10^k, onde v = f * 2^exponent e
/// 2^52 <= f < 2^53. v é portanto um double normalizado com o expoente dado. A saída é uma
/// aproximação do expoente da aproximação decimal .digits * 10^k.
///
/// O resultado pode errar para baixo por 1, e então 10^k <= v < 10^k+1. Isso vale também para
/// o limite superior m+ de v: 10^k <= m+ < 10^k+1.
///
/// Exemplos:
///  estimate_power(0)   => 16
///  estimate_power(-52) => 0
///
/// Nota: e >= 0 => estimate_power(e) > 0. Nada semelhante vale para e < 0.
fn estimate_power(exponent: i32) -> i32 {
    // Esta função estima log10 de v, onde v = f*2^e (com e == exponent).
    // Note que 10^floor(log10(v)) <= v, mas v <= 10^ceil(log10(v)).
    // f é limitado pelo seu contêiner. Com p = 53 (o tamanho do significando do double),
    // 2^(p-1) <= f < 2^p.
    //
    // Como log10(v) == log2(v)/log2(10) e e+(len(f)-1) é bem próximo de log2(v), a função se
    // simplifica para (e+(len(f)-1)/log2(10)). O número calculado fica abaixo por menos
    // de 0.631.
    //
    // Para evitar errar para cima, subtraímos 1e-10 para que imprecisões de ponto flutuante
    // não nos afetem.
    //
    // Explicação para o limite m+ de v: a conta aproveita que 2^(p-1) <= f < 2^p. Os limites
    // ainda satisfazem isso (mesmo para denormais, onde o delta pode ser bem maior).

    const K1_LOG10: f64 = 0.30102999566398114; // 1/lg(10)

    // Para doubles len(f) == 53 (sem esquecer o bit escondido).
    const K_SIGNIFICAND_SIZE: i32 = Double::K_SIGNIFICAND_SIZE;
    let estimate: f64 =
        (((exponent + K_SIGNIFICAND_SIZE - 1) as f64) * K1_LOG10 - 1e-10).ceil();
    estimate as i32
}

/// Ver os comentários de `initial_scaled_start_values`.
fn initial_scaled_start_values_positive_exponent(
    significand: u64,
    exponent: i32,
    estimated_power: i32,
    need_boundary_deltas: bool,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    // Um expoente positivo implica uma potência positiva.
    debug_assert!(estimated_power >= 0);
    // Como estimated_power é positivo, basta multiplicar o denominador por
    // 10^estimated_power.

    // numerator = v.
    numerator.assign_uint64(significand);
    numerator.shift_left(exponent);
    // denominator = 10^estimated_power.
    denominator.assign_power_uint16(10, estimated_power);

    if need_boundary_deltas {
        // Introduz um denominador comum para que os deltas até os limites sejam inteiros.
        denominator.shift_left(1);
        numerator.shift_left(1);
        // Seja v = f * 2^e; então m+ - v = 1/2 * 2^e. Com o denominador comum (2),
        // delta_plus vale 2^e.
        delta_plus.assign_uint16(1);
        delta_plus.shift_left(exponent);
        // Igual para delta_minus. Os ajustes se f == 2^p-1 ficam para depois.
        delta_minus.assign_uint16(1);
        delta_minus.shift_left(exponent);
    }
}

/// Ver os comentários de `initial_scaled_start_values`.
fn initial_scaled_start_values_negative_exponent_positive_power(
    significand: u64,
    exponent: i32,
    estimated_power: i32,
    need_boundary_deltas: bool,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    // v = f * 2^e com e < 0, e com estimated_power >= 0. Isso significa que e está perto de 0
    // (ver como estimated_power é calculado).

    // numerator = significand
    //  como v = significand * 2^exponent, isso equivale a numerator = v * / 2^-exponent
    numerator.assign_uint64(significand);
    // denominator = 10^estimated_power * 2^-exponent (com exponent < 0)
    denominator.assign_power_uint16(10, estimated_power);
    denominator.shift_left(-exponent);

    if need_boundary_deltas {
        // Introduz um denominador comum para que os deltas até os limites sejam inteiros.
        denominator.shift_left(1);
        numerator.shift_left(1);
        // Seja v = f * 2^e; então m+ - v = 1/2 * 2^e. Com o denominador comum (2),
        // delta_plus vale 2^e. Como o denominador já inclui o expoente de v, a distância
        // até os limites é simplesmente 1.
        delta_plus.assign_uint16(1);
        // Igual para delta_minus. Os ajustes se f == 2^p-1 ficam para depois.
        delta_minus.assign_uint16(1);
    }
}

/// Ver os comentários de `initial_scaled_start_values`.
fn initial_scaled_start_values_negative_exponent_negative_power(
    significand: u64,
    exponent: i32,
    estimated_power: i32,
    need_boundary_deltas: bool,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    // Em vez de multiplicar o denominador por 10^estimated_power, multiplicamos todos os
    // valores (numerador e deltas) por 10^-estimated_power.

    // Usa o numerator como contêiner temporário para power_ten.
    numerator.assign_power_uint16(10, -estimated_power);

    if need_boundary_deltas {
        // Como power_ten == numerator, precisamos copiar 10^estimated_power antes de
        // completar o cálculo do numerador.
        // delta_plus = delta_minus = 10^estimated_power
        delta_plus.assign_bignum(numerator);
        delta_minus.assign_bignum(numerator);
    }

    // numerator = significand * 2 * 10^-estimated_power
    //  como v = significand * 2^exponent, isso equivale a
    // numerator = v * 10^-estimated_power * 2 * 2^-exponent.
    // Lembre: o numerador foi usado como power_ten. Não precisa atribuí-lo a si mesmo.
    numerator.multiply_by_uint64(significand);

    // denominator = 2 * 2^-exponent com exponent < 0.
    denominator.assign_uint16(1);
    denominator.shift_left(-exponent);

    if need_boundary_deltas {
        // Introduz um denominador comum para que os deltas até os limites sejam inteiros.
        numerator.shift_left(1);
        denominator.shift_left(1);
        // Com este deslocamento os limites têm o valor correto, pois
        // delta_plus = 10^-estimated_power e delta_minus = 10^-estimated_power.
        // Essas atribuições foram feitas antes. Os ajustes se f == 2^p-1 (o limite inferior é
        // mais próximo) ficam para depois.
    }
}

/// Seja v = significand * 2^exponent.
/// Calcula v / 10^estimated_power exatamente, como a razão de dois bignums, numerator e
/// denominator. As funções `generate_shortest_digits` e `generate_counted_digits` convertem
/// essa razão para a representação decimal d, com a precisão exigida.
/// Então d * 10^estimated_power é a representação de v.
/// (A fração e o estimated_power podem ser ajustados antes de gerar a representação decimal.)
///
/// Os valores iniciais são:
///  - um numerador escalado: tal que numerator/denominator == v / 10^estimated_power.
///  - um denominador (comum) escalado.
///  opcionalmente (usados por `generate_shortest_digits` para decidir se tem o decimal mais
///  curto que converte de volta para v):
///  - v - m-: a distância até o limite inferior.
///  - m+ - v: a distância até o limite superior.
///
/// v, m+, m-, e portanto v - m- e m+ - v, compartilham o mesmo denominador.
///
/// Seja ep == estimated_power; os valores devolvidos satisfazem:
///  v / 10^ep = numerator / denominator.
///  os limites m- e m+ de v:
///    m- / 10^ep == v / 10^ep - delta_minus / denominator
///    m+ / 10^ep == v / 10^ep + delta_plus / denominator
///
/// Como 10^(k-1) <= v < 10^k (com k == estimated_power) ou 10^k <= v < 10^(k+1), temos
///           0.1 <= numerator/denominator < 1
///      ou     1 <= numerator/denominator < 10
///
/// Fica fácil dar a partida na rotina de geração de dígitos.
///
/// Os deltas dos limites só são preenchidos se o modo é `BIGNUM_DTOA_SHORTEST` ou
/// `BIGNUM_DTOA_SHORTEST_SINGLE`.
fn initial_scaled_start_values(
    significand: u64,
    exponent: i32,
    lower_boundary_is_closer: bool,
    estimated_power: i32,
    need_boundary_deltas: bool,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    if exponent >= 0 {
        initial_scaled_start_values_positive_exponent(
            significand,
            exponent,
            estimated_power,
            need_boundary_deltas,
            numerator,
            denominator,
            delta_minus,
            delta_plus,
        );
    } else if estimated_power >= 0 {
        initial_scaled_start_values_negative_exponent_positive_power(
            significand,
            exponent,
            estimated_power,
            need_boundary_deltas,
            numerator,
            denominator,
            delta_minus,
            delta_plus,
        );
    } else {
        initial_scaled_start_values_negative_exponent_negative_power(
            significand,
            exponent,
            estimated_power,
            need_boundary_deltas,
            numerator,
            denominator,
            delta_minus,
            delta_plus,
        );
    }

    if need_boundary_deltas && lower_boundary_is_closer {
        // O limite inferior está à metade da distância dos números "normais". Aumenta o
        // denominador comum e ajusta tudo menos delta_minus.
        denominator.shift_left(1); // *2
        numerator.shift_left(1); // *2
        delta_plus.shift_left(1); // *2
    }
}

/// Esta rotina multiplica numerator/denominator para que os valores fiquem no intervalo
/// 1-10. Depois da chamada temos:
///    1 <= (numerator + delta_plus) /denominator < 10.
/// Seja numerator a entrada antes da modificação e numerator' o argumento depois; o parâmetro
/// de saída decimal_point é tal que
///  numerator / denominator * 10^estimated_power ==
///    numerator' / denominator' * 10^(decimal_point - 1)
/// Em alguns casos estimated_power estava baixo, e isso já está valendo. Então só ajustamos a
/// potência para 10^(k-1) <= v < 10^k (com k == estimated_power), sem tocar no numerador nem
/// no denominador. Do contrário a rotina multiplica o numerador e os deltas por 10.
fn fixup_multiply_10(
    estimated_power: i32,
    is_even: bool,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    let in_range: bool;
    if is_even {
        // Para doubles IEEE os casos de meio exato (no sistema decimal, números terminados em
        // 5) arredondam para o ponto flutuante mais próximo com significando par.
        in_range = Bignum::plus_compare(numerator, delta_plus, denominator) >= 0;
    } else {
        in_range = Bignum::plus_compare(numerator, delta_plus, denominator) > 0;
    }
    if in_range {
        // Como numerator + delta_plus >= denominator, já temos 1 <= numerator/denominator < 10.
        // Só atualiza o estimated_power.
        *decimal_point = estimated_power + 1;
    } else {
        *decimal_point = estimated_power;
        numerator.times10();
        if Bignum::equal(delta_minus, delta_plus) {
            delta_minus.times10();
            delta_plus.assign_bignum(delta_minus);
        } else {
            delta_minus.times10();
            delta_plus.times10();
        }
    }
}
