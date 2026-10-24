//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_FAST_FLOAT_H` (linhas 1606 a 1693
//! do amálgama): a API pública.
//!
//! Esta seção do C++ só traz declarações (protótipos de template com a documentação); os corpos
//! vivem na seção `FASTFLOAT_PARSE_NUMBER_H` e portanto no módulo `parse_number`:
//!
//! - `from_chars(first, last, value, fmt = chars_format::general)` para tipos de ponto flutuante;
//! - `from_chars_advanced(first, last, value, options)`;
//! - `integer_times_pow10(mantissa, decimal_exponent)` (`u64`/`i64`, e as formas genéricas em `T`);
//! - `from_chars(first, last, value, base = 10)` para tipos inteiros.
//!
//! Em Rust não existe declaração sem corpo, então nada se repete aqui. Os tipos que a API expõe
//! (`chars_format`, `from_chars_result_t`, `parse_options_t`) nascem em `float_common`, como no
//! amálgama, e são reexportados para quem chama pelo nome da seção da API.
//!
//! Documentação original de `from_chars`: lê a sequência de caracteres `[first, last)` à procura de
//! um número de ponto flutuante, num formato independente de localidade equivalente ao de
//! `std::strtod` na localidade "C". O valor é o ponto flutuante mais próximo (`float` ou `double`),
//! com arredondamento para o par nos empates, ou seja, a análise é exata segundo o padrão IEEE.
//! Em caso de sucesso, `ptr` aponta logo depois do número lido e `value` recebe o valor; em caso de
//! erro, `ec` traz o erro. O último argumento, `chars_format`, é um conjunto de bits: `fixed` e
//! `scientific` dizem se a notação de ponto fixo e a científica são aceitas.
//!
//! `integer_times_pow10` multiplica um inteiro por uma potência de 10 e devolve o resultado como
//! `double` corretamente arredondado (para o par nos empates). No estouro devolve infinito, no
//! subfluxo devolve zero.

pub use super::float_common::{
    chars_format, from_chars_result, from_chars_result_t, parse_options, parse_options_t,
};
