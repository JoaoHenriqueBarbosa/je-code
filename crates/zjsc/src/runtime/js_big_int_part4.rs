// Quarta fatia do porte de `JSBigInt.cpp`: as linhas 1773 a 2624 (`multiplyToomCook`, o
// namespace `FFT` com a aritmética mod F_n e a escolha de parâmetros, a classe `FFTContainer` e
// `shouldUseFFT`). O trecho termina logo antes de `multiplyFFT` (linha 2625).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Dependências de fatias futuras (ainda inexistentes): `JSBigInt::multiply_digits_into` (linha
// 2686, já referenciada pela fatia 3 em `multiply_zero_padded`).
//
// Desvios mecânicos de Rust seguro, sem efeito observável:
// - O C++ guarda `Digit* m_parts[]` apontando para um `Vector<Digit> m_storage` e um `m_temp`
//   separado, e as rotinas do namespace `FFT` recebem ponteiros que podem se sobrepor
//   (`sumDiff(a, b, a, b)`). Aqui o container tem um único `Vec<Digit>` com as `n` partes
//   seguidas pelos `2 * length` dígitos do `m_temp`, e as rotinas recebem esse buffer mais
//   deslocamentos (`part(i) == i * length`, `temp() == n * length`). Cada dígito é lido antes de
//   gravar o mesmo índice, como no C++, então o resultado é idêntico.
// - O C++ guarda `InterruptCheck& m_interrupt` no container. Como `multiplyInner` mantém dois
//   containers vivos ao mesmo tempo, o `&mut InterruptCheck` vira primeiro parâmetro de cada
//   método que o usa.

/// `namespace FFT` de `JSBigInt.cpp`.
pub mod fft {
    use super::{digit_add, digit_add3, digit_sub, digit_sub2, Digit, DIGIT_BITS};
    use crate::wtf::math_extras::round_up_to_multiple_of;

    pub type SignedDigit = i64;
    pub const DIGIT_BITS_USIZE: usize = DIGIT_BITS as usize;
    pub const LOG2_DIGIT_BITS: u32 = DIGIT_BITS.trailing_zeros();
    const _: () = assert!((1u32 << LOG2_DIGIT_BITS) == DIGIT_BITS);

    // Ver `should_use_fft` para o que os limiares significam. Os valores de 64 bits são medidos
    // contra o Toom-3; os de 32 bits se reduzem ao limiar único do V8 no menor operando.
    pub const FFT_THRESHOLD: usize = 2300;
    pub const FFT_MIN_SMALLER_SIZE: usize = 600;
    pub const FFT_CHUNK_THRESHOLD: usize = 1150;
    pub const FFT_INNER_THRESHOLD: usize = 200;
    // Acima desta razão entre os tamanhos dos operandos, uma transformada dimensionada para os
    // dois é quase só preenchimento, então x é multiplicado em pedaços do tamanho de y.
    pub const ASYMMETRIC_CHUNKING_THRESHOLD: usize = 100;

    // Parte 1: funções da aritmética "mod F_n".
    // F_n tem a forma 2^K + 1, e por conveniência K conta dígitos em vez de bits, então F_n (ou K)
    // ficam implícitos e deduzidos do comprimento do vetor de dígitos.

    /// `modFnHelper`. O comprimento é o de `x`.
    fn mod_fn_helper(x: &mut [Digit], high: SignedDigit) {
        let length = x.len();
        if high > 0 {
            let mut borrow = high as Digit;
            x[length - 1] = 0;
            for i in 0..length {
                let mut new_borrow: Digit = 0;
                x[i] = digit_sub(x[i], borrow, &mut new_borrow);
                borrow = new_borrow;
                if borrow == 0 {
                    break;
                }
            }
        } else {
            let mut carry = high.wrapping_neg() as Digit;
            x[length - 1] = 0;
            for i in 0..length {
                let mut new_carry: Digit = 0;
                x[i] = digit_add(x[i], carry, &mut new_carry);
                carry = new_carry;
                if carry == 0 {
                    break;
                }
            }
        }
    }

    /// `modFn`: {x} := {x} mod F_n, supondo que {x} é "um pouco" maior que F_n (por exemplo, depois
    /// da soma de dois números já normalizados mod F_n).
    pub fn mod_fn(x: &mut [Digit]) {
        let k = x.len() - 1;
        let mut high = x[k] as SignedDigit;
        if high == 0 {
            return;
        }
        mod_fn_helper(x, high);
        high = x[k] as SignedDigit;
        if high == 0 {
            return;
        }
        debug_assert!(high == 1 || high == -1);
        mod_fn_helper(x, high);
        high = x[k] as SignedDigit;
        if high == -1 {
            mod_fn_helper(x, high);
        }
    }

    /// `modFnDoubleWidth`: {dest} := {src} mod F_n, supondo que {src} tem cerca do dobro do
    /// comprimento de F_n (por exemplo, depois da multiplicação de dois números normalizados mod
    /// F_n). `dest` e `src` são deslocamentos em `buf`; {length} é o comprimento de {dest} e {src}
    /// tem o dobro.
    pub fn mod_fn_double_width(buf: &mut [Digit], dest: usize, src: usize, length: usize) {
        let k = length - 1;
        let mut borrow: Digit = 0;
        for i in 0..k {
            let mut new_borrow: Digit = 0;
            buf[dest + i] = digit_sub2(buf[src + i], buf[src + i + k], borrow, &mut new_borrow);
            borrow = new_borrow;
        }
        let mut new_borrow: Digit = 0;
        buf[dest + k] = digit_sub2(0, buf[src + 2 * k], borrow, &mut new_borrow);
        // {borrow} pode ser não zero aqui, o que é aceitável, pois {mod_fn} cuida disso.
        mod_fn(&mut buf[dest..dest + length]);
    }

    /// `sumDiff`: {sum} := {a} + {b} e {diff} := {a} - {b}, mais eficiente que calcular soma e
    /// diferença em separado. Aplica a normalização "mod F_n" aos dois resultados. Todos os
    /// argumentos são deslocamentos em `buf`; entradas e saídas podem coincidir.
    pub fn sum_diff(buf: &mut [Digit], sum: usize, diff: usize, a: usize, b: usize, length: usize) {
        let mut carry: Digit = 0;
        let mut borrow: Digit = 0;
        for i in 0..length {
            // Lê os dois valores primeiro, porque entradas e saídas podem se sobrepor.
            let ai = buf[a + i];
            let bi = buf[b + i];
            let mut new_carry: Digit = 0;
            buf[sum + i] = digit_add3(ai, bi, carry, &mut new_carry);
            carry = new_carry;
            let mut new_borrow: Digit = 0;
            buf[diff + i] = digit_sub2(ai, bi, borrow, &mut new_borrow);
            borrow = new_borrow;
        }
        mod_fn(&mut buf[sum..sum + length]);
        mod_fn(&mut buf[diff..diff + length]);
    }

    /// `shiftModFnLarge`: {result} := ({input} << shift) mod F_n, com shift >= K. `result` e
    /// `input` são deslocamentos disjuntos em `buf`.
    fn shift_mod_fn_large(buf: &mut [Digit], result: usize, input: usize, mut digit_shift: usize, bits_shift: u32, k: usize) {
        // Se {digit_shift} é maior que K, usamos a transformação a seguir (onde, como tudo é mod
        // 2^K + 1, podemos somar ou subtrair qualquer múltiplo de 2^K + 1 a qualquer momento):
        //      x * 2^{K+m}   mod 2^K + 1
        //   == x * 2^K * 2^m - (2^K + 1)*(x * 2^m)   mod 2^K + 1
        //   == x * 2^K * 2^m - x * 2^K * 2^m - x * 2^m   mod 2^K + 1
        //   == -x * 2^m   mod 2^K + 1
        // Então o fluxo é o mesmo de m < K, mas invertemos os operandos da subtração. Para evitar
        // underflow, inicializamos virtualmente o resultado com 2^K + 1:
        //   input  =  [ iK ][iK-1] ....  .... [ i1 ][ i0 ]
        //   result =  [   1][0000] ....  .... [0000][0001]
        //            +                  [ iK ] .... [ iX ]
        //            -      [iX-1] .... [ i0 ]
        debug_assert!(digit_shift >= k);
        digit_shift -= k;
        let mut borrow: Digit = 0;
        if bits_shift == 0 {
            let mut carry: Digit = 1;
            for i in 0..digit_shift {
                let mut new_carry: Digit = 0;
                buf[result + i] = digit_add(buf[input + i + k - digit_shift], carry, &mut new_carry);
                carry = new_carry;
            }
            buf[result + digit_shift] = digit_sub(buf[input + k].wrapping_add(carry), buf[input], &mut borrow);
            for i in digit_shift + 1..k {
                let d = buf[input + i - digit_shift];
                let mut new_borrow: Digit = 0;
                buf[result + i] = digit_sub2(0, d, borrow, &mut new_borrow);
                borrow = new_borrow;
            }
        } else {
            let mut add_carry: Digit = 1;
            let mut input_carry: Digit = buf[input + k - digit_shift - 1] >> (DIGIT_BITS - bits_shift);
            for i in 0..digit_shift {
                let d = buf[input + i + k - digit_shift];
                let summand = (d << bits_shift) | input_carry;
                let mut new_carry: Digit = 0;
                buf[result + i] = digit_add(summand, add_carry, &mut new_carry);
                add_carry = new_carry;
                input_carry = d >> (DIGIT_BITS - bits_shift);
            }
            {
                // result[digitShift] = (addCarry + iKPart) - i0Part
                let mut d = buf[input + k];
                let ik_part = (d << bits_shift) | input_carry;
                let mut ik_carry = d >> (DIGIT_BITS - bits_shift);
                let mut new_carry: Digit = 0;
                let sum = digit_add(add_carry, ik_part, &mut new_carry);
                add_carry = new_carry;
                // {ik_carry} é menor que um dígito inteiro, então dá para fundir {add_carry} nele
                // sem overflow.
                ik_carry = ik_carry.wrapping_add(add_carry);
                d = buf[input];
                let i0_part = d << bits_shift;
                buf[result + digit_shift] = digit_sub(sum, i0_part, &mut borrow);
                input_carry = d >> (DIGIT_BITS - bits_shift);
                if digit_shift + 1 < k {
                    d = buf[input + 1];
                    let subtrahend = (d << bits_shift) | input_carry;
                    let mut new_borrow: Digit = 0;
                    buf[result + digit_shift + 1] = digit_sub2(ik_carry, subtrahend, borrow, &mut new_borrow);
                    borrow = new_borrow;
                    input_carry = d >> (DIGIT_BITS - bits_shift);
                }
            }
            for i in digit_shift + 2..k {
                let d = buf[input + i - digit_shift];
                let subtrahend = (d << bits_shift) | input_carry;
                let mut new_borrow: Digit = 0;
                buf[result + i] = digit_sub2(0, subtrahend, borrow, &mut new_borrow);
                borrow = new_borrow;
                input_carry = d >> (DIGIT_BITS - bits_shift);
            }
        }
        // O 1 virtual em result[K] deve ser eliminado por {borrow}. Se não há empréstimo, a
        // inicialização virtual foi demais. Subtrai 2^K + 1.
        buf[result + k] = 0;
        if borrow != 1 {
            borrow = 1;
            for i in 0..k {
                let mut new_borrow: Digit = 0;
                buf[result + i] = digit_sub(buf[result + i], borrow, &mut new_borrow);
                borrow = new_borrow;
                if borrow == 0 {
                    break;
                }
            }
            if borrow != 0 {
                // O resultado deve ser 2^K.
                for i in 0..k {
                    buf[result + i] = 0;
                }
                buf[result + k] = 1;
            }
        }
    }

    /// `shiftModFn`: {result} := {input} * 2^{power_of_two} mod 2^{K} + 1. `result` e `input` são
    /// deslocamentos disjuntos em `buf`; `zero_above` é `std::numeric_limits<size_t>::max()` quando
    /// o chamador não informa. Esta função é muito relevante para o desempenho geral.
    pub fn shift_mod_fn(buf: &mut [Digit], result: usize, input: usize, power_of_two: usize, k: usize, zero_above: usize) {
        // A redução do módulo é uma subtração, que combinamos com o deslocamento assim:
        //   input  =  [ iK ][iK-1] ....  .... [ i1 ][ i0 ]
        //   result =        [iX-1] .... [ i0 ] <---------- deslocado por {power_of_two}
        //            -                  [ iK ] .... [ iX ]
        // onde "X" é o índice "K - digitShift".
        let mut digit_shift = power_of_two / DIGIT_BITS_USIZE;
        let bits_shift = (power_of_two % DIGIT_BITS_USIZE) as u32;
        // Por uma construção análoga à do caso "digitShift >= K", vale:
        //    x * 2^{2K+m} == x * 2^m   mod 2^K + 1.
        while digit_shift >= 2 * k {
            digit_shift -= 2 * k; // Mais rápido que '%'!
        }
        if digit_shift >= k {
            return shift_mod_fn_large(buf, result, input, digit_shift, bits_shift, k);
        }
        let mut borrow: Digit = 0;
        if bits_shift == 0 {
            // Fazemos uma única passada em {input}, começando por copiar os dígitos [i1] a [iX-1]
            // para os índices digitShift+1 a K-1 de result.
            let mut i: usize = 1;
            // Lê os dígitos de entrada, a menos que saibamos que são zero.
            let mut cap = (k - digit_shift).min(zero_above);
            while i < cap {
                buf[result + i + digit_shift] = buf[input + i];
                i += 1;
            }
            // O trabalho restante pode assumir que input[i] == 0.
            while i < k - digit_shift {
                debug_assert!(buf[input + i] == 0);
                buf[result + i + digit_shift] = 0;
                i += 1;
            }
            // Segunda fase: subtrai os dígitos [iX] a [iK] de input dos índices 0 a digitShift-1 de
            // result (virtualmente inicializados com zero).
            cap = k.min(zero_above);
            while i < cap {
                let d = buf[input + i];
                let mut new_borrow: Digit = 0;
                buf[result + i + digit_shift - k] = digit_sub2(0, d, borrow, &mut new_borrow);
                borrow = new_borrow;
                i += 1;
            }
            // O trabalho restante pode assumir que input[i] == 0.
            while i < k {
                debug_assert!(buf[input + i] == 0);
                let mut new_borrow: Digit = 0;
                buf[result + i + digit_shift - k] = digit_sub(0, borrow, &mut new_borrow);
                borrow = new_borrow;
                i += 1;
            }
            // Último passo: subtrai [iK] de [i0] e grava no índice digitShift de result.
            let mut new_borrow: Digit = 0;
            buf[result + digit_shift] = digit_sub2(buf[input], buf[input + k], borrow, &mut new_borrow);
            borrow = new_borrow;
        } else {
            // Mesmo fluxo de antes, levando bitsShift != 0 em conta.
            // Primeira fase: índices digitShift+1 a K de result.
            let mut carry: Digit = 0;
            let mut i: usize = 0;
            // Lê os dígitos de entrada, a menos que saibamos que são zero.
            let mut cap = (k - digit_shift).min(zero_above);
            while i < cap {
                let d = buf[input + i];
                buf[result + i + digit_shift] = (d << bits_shift) | carry;
                carry = d >> (DIGIT_BITS - bits_shift);
                i += 1;
            }
            // O trabalho restante pode assumir que input[i] == 0.
            while i < k - digit_shift {
                debug_assert!(buf[input + i] == 0);
                buf[result + i + digit_shift] = carry;
                carry = 0;
                i += 1;
            }
            // Segunda fase: índices 0 a digitShift - 1 de result.
            cap = k.min(zero_above);
            while i < cap {
                let d = buf[input + i];
                let mut new_borrow: Digit = 0;
                buf[result + i + digit_shift - k] = digit_sub2(0, (d << bits_shift) | carry, borrow, &mut new_borrow);
                borrow = new_borrow;
                carry = d >> (DIGIT_BITS - bits_shift);
                i += 1;
            }
            // O trabalho restante pode assumir que input[i] == 0.
            if i < k {
                debug_assert!(buf[input + i] == 0);
                let mut new_borrow: Digit = 0;
                buf[result + i + digit_shift - k] = digit_sub2(0, carry, borrow, &mut new_borrow);
                borrow = new_borrow;
                carry = 0;
                i += 1;
            }
            while i < k {
                debug_assert!(buf[input + i] == 0);
                let mut new_borrow: Digit = 0;
                buf[result + i + digit_shift - k] = digit_sub(0, borrow, &mut new_borrow);
                borrow = new_borrow;
                i += 1;
            }
            // Último passo: calcula result[digitShift].
            let d = buf[input + k];
            let mut new_borrow: Digit = 0;
            buf[result + digit_shift] = digit_sub2(buf[result + digit_shift], (d << bits_shift) | carry, borrow, &mut new_borrow);
            borrow = new_borrow;
            // Não sobra vai-um.
            debug_assert!((d >> (DIGIT_BITS - bits_shift)) == 0);
        }
        buf[result + k] = 0;
        let mut i = digit_shift + 1;
        while i <= k && borrow != 0 {
            let mut new_borrow: Digit = 0;
            buf[result + i] = digit_sub(buf[result + i], borrow, &mut new_borrow);
            borrow = new_borrow;
            i += 1;
        }
        if borrow != 0 {
            // Underflow significa que subtraímos demais. Soma 2^K + 1.
            let mut carry: Digit = 1;
            for i in 0..=k {
                let mut new_carry: Digit = 0;
                buf[result + i] = digit_add(buf[result + i], carry, &mut new_carry);
                carry = new_carry;
                if carry == 0 {
                    break;
                }
            }
            let mut new_carry: Digit = 0;
            buf[result + k] = digit_add(buf[result + k], 1, &mut new_carry);
        }
    }

    // Parte 2: a multiplicação por FFT é muito sensível à escolha dos parâmetros. As funções a
    // seguir escolhem os parâmetros que o cálculo propriamente dito usará. Isso se baseia em parte
    // em restrições formais e em parte em heurísticas determinadas experimentalmente.

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Parameters {
        pub m: u32,
        pub k: usize,
        pub n: usize,
        pub s: usize,
        pub r: usize,
    }

    /// `computeParameters`: parâmetros do cálculo principal, dados um comprimento em dígitos {n_len}
    /// (o `N` do C++, que o código converte em bits) e um {m}. Ver o artigo para detalhes.
    pub fn compute_parameters(big_n: usize, m: u32, params: &mut Parameters) {
        let big_n = big_n * DIGIT_BITS_USIZE;
        let n = 1usize << m; // 2^m
        let nhalf = n >> 1;
        let mut s = (big_n + n - 1) >> m; // ceil(N/n)
        s = round_up_to_multiple_of(DIGIT_BITS_USIZE, s);
        let mut k = m as usize + 2 * s + 1; // K precisa ser pelo menos isto...
        k = round_up_to_multiple_of(nhalf, k); // ...e múltiplo de n/2.
        let mut r = k >> (m - 1); // Qual múltiplo?

        // Queremos que as chamadas recursivas progridam, então forçamos K a múltiplo de 8 se está
        // acima do limiar de recursão. Caso contrário, K precisa ser múltiplo de digitBits.
        let threshold: u32 = if k + 1 >= FFT_INNER_THRESHOLD * DIGIT_BITS_USIZE { 3 + LOG2_DIGIT_BITS } else { LOG2_DIGIT_BITS };
        let mut trailing_zeros = k.trailing_zeros();
        while trailing_zeros < threshold {
            k += 1usize << trailing_zeros;
            r = k >> (m - 1);
            trailing_zeros = k.trailing_zeros();
        }

        debug_assert!(k % DIGIT_BITS_USIZE == 0);
        debug_assert!(s % DIGIT_BITS_USIZE == 0);
        params.k = k / DIGIT_BITS_USIZE;
        params.s = s / DIGIT_BITS_USIZE;
        params.n = n;
        params.r = r;
    }

    /// `computeParametersInner`: parâmetros das invocações recursivas ("camada interna").
    pub fn compute_parameters_inner(big_n: usize, params: &mut Parameters) {
        let max_m = big_n.trailing_zeros();
        let n_bits = usize::BITS - big_n.leading_zeros();
        let mut m = n_bits.wrapping_sub(4); // Não deixa s ficar pequeno demais.
        m = max_m.min(m);
        let big_n = big_n * DIGIT_BITS_USIZE;
        let n = 1usize << m; // 2^m
        // Não dá para arredondar s na camada interna, porque N = n*s é fixo.
        let s = big_n >> m;
        debug_assert!(big_n == s * n);
        let mut k = m as usize + 2 * s + 1; // K precisa ser pelo menos isto...
        k = round_up_to_multiple_of(n, k); // ...e múltiplo de n e de digitBits.
        k = round_up_to_multiple_of(DIGIT_BITS_USIZE, k);
        params.r = k >> m; // Qual múltiplo?
        debug_assert!(k % DIGIT_BITS_USIZE == 0);
        debug_assert!(s % DIGIT_BITS_USIZE == 0);
        params.k = k / DIGIT_BITS_USIZE;
        params.s = s / DIGIT_BITS_USIZE;
        params.n = n;
        params.m = m;
    }

    /// `predictInnerK`
    pub fn predict_inner_k(big_n: usize) -> usize {
        let mut params = Parameters::default();
        compute_parameters_inner(big_n, &mut params);
        params.k
    }

    /// `shouldDecrementM`: aplica heurísticas para decidir se {m} deve ser decrementado, olhando o
    /// que aconteceria com {K} e {s} se {m} fosse decrementado.
    pub fn should_decrement_m(current: &Parameters, next: &Parameters, after_next: &Parameters) -> bool {
        // K == 64 parece funcionar particularmente bem.
        if current.k == 64 && next.k >= 112 {
            return false;
        }
        // Valores pequenos de s nunca são eficientes.
        if current.s < 6 {
            return true;
        }
        // O tempo é aproximadamente determinado por K * n. Quando decrementamos m, n sempre cai
        // pela metade, e K geralmente cresce, até 2x.
        // Para s não tão pequeno, olhamos quanto K cresceria: se o aumento de K é pequeno o
        // bastante, vale a pena diminuir n.
        // Empiricamente, o mais significativo é olhar o K *depois* do próximo.
        // Os valores específicos dos limiares foram escolhidos rodando muitos benchmarks em
        // entradas de muitos tamanhos e selecionando à mão os que pareciam dar bons resultados.
        let factor = after_next.k as f64 / current.k as f64;
        if (current.s == 6 && factor < 3.85)
            || (current.s == 7 && factor < 3.73)
            || (current.s == 8 && factor < 3.55)
            || (current.s == 9 && factor < 3.50)
            || factor < 3.4
        {
            return true;
        }
        // Se K está logo abaixo do limiar de recursão, garante que haja recursão, a menos que isso
        // seja particularmente ineficiente (K interno grande).
        // Se K está logo acima do limiar de recursão, dobrá-lo costuma tornar a chamada interna
        // mais eficiente.
        if current.k >= 160 && current.k < 250 && predict_inner_k(next.k) < 28 {
            return true;
        }
        // Se não achamos motivo para decrementar, mantém m o maior possível.
        false
    }

    /// `getParameters`: decide quais parâmetros usar para um comprimento de entrada {N}. Devolve o
    /// m escolhido.
    pub fn get_parameters(big_n: usize, params: &mut Parameters) -> u32 {
        let n_bits = usize::BITS - big_n.leading_zeros();
        let mut max_m = n_bits.wrapping_sub(3); // m maiores deixam s pequeno demais.
        max_m = LOG2_DIGIT_BITS.max(max_m); // m menores quebram a lógica abaixo.
        let mut m = max_m;
        let mut current = Parameters::default();
        compute_parameters(big_n, m, &mut current);
        let mut next = Parameters::default();
        compute_parameters(big_n, m - 1, &mut next);
        while m > 2 {
            let mut after_next = Parameters::default();
            compute_parameters(big_n, m - 2, &mut after_next);
            if should_decrement_m(&current, &next, &after_next) {
                m -= 1;
                current = next;
                next = after_next;
            } else {
                break;
            }
        }
        *params = current;
        m
    }
}

// Parte 3: Transformada Rápida de Fourier.

/// `JSBigInt::FFTContainer`. `n` é o número de pedaços, de comprimento `K + 1`; `K` determina
/// F_n = 2^(K * digitBits) + 1. `storage` guarda as `n` partes seguidas do `m_temp`
/// (`2 * length` dígitos).
pub struct FFTContainer {
    n: usize,      // Número de partes.
    k: usize,      // Sempre length - 1.
    length: usize, // Comprimento de cada parte, em dígitos.
    storage: Vec<Digit>,
}

/// `copyAndZeroExtend`
#[inline]
fn copy_and_zero_extend(destination: &mut [Digit], source: &[Digit]) {
    let digits_to_copy = source.len();
    destination[..digits_to_copy].copy_from_slice(source);
    destination[digits_to_copy..].fill(0);
}

/// `fftShouldBeNegative`
fn fft_should_be_negative(x: &[Digit], threshold: Digit, s: usize) -> bool {
    if x[2 * s] >= threshold {
        return true;
    }
    for &digit in &x[2 * s + 1..] {
        if digit > 0 {
            return true;
        }
    }
    false
}

impl FFTContainer {
    /// Construtor: `n` pedaços de comprimento `K + 1`.
    pub fn new(n: usize, k: usize) -> FFTContainer {
        let length = k + 1;
        FFTContainer { n, k, length, storage: vec![0; length * n + length * 2] }
    }

    /// Deslocamento de `m_parts[i]` em `storage`.
    #[inline]
    fn part_offset(&self, i: usize) -> usize {
        i * self.length
    }

    /// Deslocamento de `temp()` em `storage`.
    #[inline]
    fn temp_offset(&self) -> usize {
        self.n * self.length
    }

    /// Lê {x} no armazenamento interno do container, dividindo-o em pedaços; depois faz a FFT
    /// direta.
    pub fn start_default(&mut self, interrupt: &mut InterruptCheck, x: &[Digit], mut chunk_size: usize, theta: usize, omega: usize) {
        let mut length = x.len();
        let mut pointer: usize = 0;
        let mut current_theta: usize = 0;
        let mut i: usize = 0;
        let temp = self.temp_offset();
        while i < self.n && length > 0 {
            interrupt.add_work(self.length);
            chunk_size = chunk_size.min(length);
            // Para invocações via multiplyInner, x.size() == m_n * chunkSize + 1, porque o "K" da
            // camada externa é passado como o "N" da interna. Como x é normalizado (mod Fn) na
            // camada externa, há o raro caso de canto em que x[m_n * chunkSize] == 1. Detecta esse
            // caso e trata o bit extra como parte do último pedaço; sempre há espaço.
            if i == self.n - 1 && length == chunk_size + 1 {
                debug_assert!(x[self.n * chunk_size] <= 1);
                debug_assert!(self.length >= chunk_size + 1);
                chunk_size += 1;
            }
            let part = self.part_offset(i);
            if current_theta != 0 {
                // Multiplica por theta^i e reduz módulo 2^K + 1.
                // Passamos theta como quantidade de deslocamento; na verdade significa 2^theta.
                copy_and_zero_extend(&mut self.storage[temp..temp + self.length], &x[pointer..pointer + chunk_size]);
                fft::shift_mod_fn(&mut self.storage, part, temp, current_theta, self.k, chunk_size);
            } else {
                copy_and_zero_extend(&mut self.storage[part..part + self.length], &x[pointer..pointer + chunk_size]);
            }
            pointer += chunk_size;
            length -= chunk_size;
            i += 1;
            current_theta += theta;
        }
        debug_assert!(length == 0);
        while i < self.n {
            let part = self.part_offset(i);
            self.storage[part..part + self.length].fill(0);
            i += 1;
        }
        self.fft_return_shuffled(interrupt, 0, self.n, omega, temp);
    }

    /// Esta versão de start é otimizada para o caso em que ~metade do container será preenchida
    /// com zeros de preenchimento.
    pub fn start(&mut self, interrupt: &mut InterruptCheck, x: &[Digit], mut chunk_size: usize, theta: usize, omega: usize) {
        let mut length = x.len();
        if length > self.n * chunk_size / 2 {
            return self.start_default(interrupt, x, chunk_size, theta, omega);
        }
        debug_assert!(theta == 0);
        let mut pointer: usize = 0;
        let nhalf = self.n / 2;
        // Primeira iteração desenrolada.
        chunk_size = chunk_size.min(length);
        let part0 = self.part_offset(0);
        let part_half = self.part_offset(nhalf);
        copy_and_zero_extend(&mut self.storage[part0..part0 + self.length], &x[pointer..pointer + chunk_size]);
        copy_and_zero_extend(&mut self.storage[part_half..part_half + self.length], &x[pointer..pointer + chunk_size]);
        pointer += chunk_size;
        length -= chunk_size;
        let mut i: usize = 1;
        while i < nhalf && length > 0 {
            interrupt.add_work(self.length);
            chunk_size = chunk_size.min(length);
            let part = self.part_offset(i);
            copy_and_zero_extend(&mut self.storage[part..part + self.length], &x[pointer..pointer + chunk_size]);
            let w = omega * i;
            let part_i_half = self.part_offset(i + nhalf);
            fft::shift_mod_fn(&mut self.storage, part_i_half, part, w, self.k, chunk_size);
            pointer += chunk_size;
            length -= chunk_size;
            i += 1;
        }
        while i < nhalf {
            let part = self.part_offset(i);
            self.storage[part..part + self.length].fill(0);
            let part_i_half = self.part_offset(i + nhalf);
            self.storage[part_i_half..part_i_half + self.length].fill(0);
            i += 1;
        }
        let temp = self.temp_offset();
        self.fft_recurse(interrupt, 0, nhalf, omega, temp);
    }

    /// Transformação direta. Usamos a transformada "DIF", ou "decimation in frequency", porque ela
    /// deixa o resultado em ordem "bit reversed", que é justamente o que precisamos como entrada da
    /// transformada inversa "DIT", ou "decimation in time". `temp` é o deslocamento do `m_temp`.
    pub fn fft_return_shuffled(&mut self, interrupt: &mut InterruptCheck, start: usize, length: usize, omega: usize, temp: usize) {
        debug_assert!(length & 1 == 0); // {length} precisa ser par.
        if interrupt.interrupted() {
            return;
        }
        let half = length / 2;
        let (a, b) = (self.part_offset(start), self.part_offset(start + half));
        fft::sum_diff(&mut self.storage, a, b, a, b, self.length);
        for k in 1..half {
            interrupt.add_work(self.length);
            let part_k = self.part_offset(start + k);
            let part_half_k = self.part_offset(start + half + k);
            fft::sum_diff(&mut self.storage, part_k, temp, part_k, part_half_k, self.length);
            let w = omega * k;
            fft::shift_mod_fn(&mut self.storage, part_half_k, temp, w, self.k, usize::MAX);
        }
        self.fft_recurse(interrupt, start, half, omega, temp);
    }

    /// Passo recursivo do anterior, fatorado para chamadores adicionais.
    pub fn fft_recurse(&mut self, interrupt: &mut InterruptCheck, start: usize, half: usize, omega: usize, temp: usize) {
        if half > 1 {
            self.fft_return_shuffled(interrupt, start, half, 2 * omega, temp);
            self.fft_return_shuffled(interrupt, start + half, half, 2 * omega, temp);
        }
    }

    /// Transformação inversa. Usamos a transformada "DIT", ou "decimation in time", aqui, porque
    /// ela transforma a entrada em ordem bit reversed em saída em ordem normal.
    pub fn backward_fft(&mut self, interrupt: &mut InterruptCheck, start: usize, length: usize, omega: usize) {
        debug_assert!(length & 1 == 0); // {length} precisa ser par.
        let half = length / 2;
        // Não recursa para half == 2, pois pointwiseMultiply já fez o primeiro nível da FFT
        // inversa.
        if half > 2 {
            self.backward_fft(interrupt, start, half, 2 * omega);
            self.backward_fft(interrupt, start + half, half, 2 * omega);
        }
        if interrupt.interrupted() {
            return;
        }
        let temp = self.temp_offset();
        let (a, b) = (self.part_offset(start), self.part_offset(start + half));
        fft::sum_diff(&mut self.storage, a, b, a, b, self.length);
        for k in 1..half {
            interrupt.add_work(self.length);
            let w = omega * (length - k);
            let part_k = self.part_offset(start + k);
            let part_half_k = self.part_offset(start + half + k);
            fft::shift_mod_fn(&mut self.storage, temp, part_half_k, w, self.k, usize::MAX);
            fft::sum_diff(&mut self.storage, part_k, part_half_k, part_k, temp, self.length);
        }
    }

    /// Recombina as partes do resultado em {z}, depois da FFT inversa.
    pub fn normalize_and_recombine(&mut self, interrupt: &mut InterruptCheck, omega: usize, m: u32, z: &mut [Digit], chunk_size: usize) {
        z.fill(0);
        let mut z_index: usize = 0;
        let shift = self.n * omega - m as usize;
        let temp = self.temp_offset();
        let mut i: usize = 0;
        while i < self.n && !interrupt.interrupted() {
            interrupt.add_work(self.length);
            let part = self.part_offset(i);
            fft::shift_mod_fn(&mut self.storage, temp, part, shift, self.k, usize::MAX);
            let mut carry: Digit = 0;
            let mut zi = z_index;
            let mut j: usize = 0;
            while j < self.length && zi < z.len() {
                let mut new_carry: Digit = 0;
                z[zi] = digit_add3(z[zi], self.storage[temp + j], carry, &mut new_carry);
                carry = new_carry;
                j += 1;
                zi += 1;
            }
            while j < self.length {
                debug_assert!(self.storage[temp + j] == 0);
                j += 1;
            }
            if carry != 0 {
                z[zi] = carry;
            }
            i += 1;
            z_index += chunk_size;
        }
    }

    /// Igual a {normalize_and_recombine} acima, mas para as necessidades da invocação recursiva
    /// ("camada interna") da multiplicação por FFT, onde um passo adicional de contrapeso é
    /// necessário.
    pub fn counter_weight_and_recombine(&mut self, interrupt: &mut InterruptCheck, theta: usize, m: u32, z: &mut [Digit], s: usize) {
        z.fill(0);
        let mut z_index: usize = 0;
        let temp = self.temp_offset();
        let mut k: usize = 0;
        while k < self.n && !interrupt.interrupted() {
            interrupt.add_work(self.length);
            // shift = -theta * k - m, tomado módulo 2 * m_n * theta (a ordem de 2^theta).
            let mut shift = theta * k + m as usize;
            debug_assert!(shift <= 2 * self.n * theta);
            if shift != 0 {
                shift = 2 * self.n * theta - shift;
            }
            let input = self.part_offset(k);
            fft::shift_mod_fn(&mut self.storage, temp, input, shift, self.k, usize::MAX);
            let remaining_z = z.len() - z_index;
            if fft_should_be_negative(&self.storage[temp..temp + self.length], (k + 1) as Digit, s) {
                // Subtrai F_n de input antes de somar ao resultado. Usamos a transformação a
                // seguir (sabendo que X < F_n):
                // Z + (X - F_n) == Z - (F_n - X)
                let mut borrow_z: Digit = 0;
                let mut borrow_fn: Digit = 0;
                {
                    // i == 0:
                    let d = digit_sub(1, self.storage[temp], &mut borrow_fn);
                    z[z_index] = digit_sub(z[z_index], d, &mut borrow_z);
                }
                let mut i: usize = 1;
                while i < self.k && i < remaining_z {
                    let mut new_borrow_fn: Digit = 0;
                    let d = digit_sub2(0, self.storage[temp + i], borrow_fn, &mut new_borrow_fn);
                    borrow_fn = new_borrow_fn;
                    let mut new_borrow_z: Digit = 0;
                    z[z_index + i] = digit_sub2(z[z_index + i], d, borrow_z, &mut new_borrow_z);
                    borrow_z = new_borrow_z;
                    i += 1;
                }
                debug_assert!(i == self.k && self.k == self.length - 1);
                while i < self.length && i < remaining_z {
                    let mut new_borrow_fn: Digit = 0;
                    let d = digit_sub2(1, self.storage[temp + i], borrow_fn, &mut new_borrow_fn);
                    borrow_fn = new_borrow_fn;
                    let mut new_borrow_z: Digit = 0;
                    z[z_index + i] = digit_sub2(z[z_index + i], d, borrow_z, &mut new_borrow_z);
                    borrow_z = new_borrow_z;
                    i += 1;
                }
                debug_assert!(borrow_fn == 0);
                while borrow_z > 0 && i < remaining_z {
                    let mut new_borrow_z: Digit = 0;
                    z[z_index + i] = digit_sub(z[z_index + i], borrow_z, &mut new_borrow_z);
                    borrow_z = new_borrow_z;
                    i += 1;
                }
            } else {
                let mut carry: Digit = 0;
                let mut i: usize = 0;
                while i < self.length && i < remaining_z {
                    let mut new_carry: Digit = 0;
                    z[z_index + i] = digit_add3(z[z_index + i], self.storage[temp + i], carry, &mut new_carry);
                    carry = new_carry;
                    i += 1;
                }
                while i < self.length {
                    debug_assert!(self.storage[temp + i] == 0);
                    i += 1;
                }
                while carry > 0 && i < remaining_z {
                    let mut new_carry: Digit = 0;
                    z[z_index + i] = digit_add(z[z_index + i], carry, &mut new_carry);
                    carry = new_carry;
                    i += 1;
                }
                // {carry} pode ser != 0 aqui se z era negativo antes. Isso é aceitável.
            }
            k += 1;
            z_index += s;
        }
    }

    /// Função principal da FFT para invocações recursivas ("camada interna").
    pub fn multiply_inner(interrupt: &mut InterruptCheck, z: &mut [Digit], x: &[Digit], y: &[Digit], params: &fft::Parameters) {
        let omega = 2 * params.r; // na verdade: 2^(2r)
        let theta = params.r; // na verdade: 2^r

        let mut a = FFTContainer::new(params.n, params.k);
        a.start_default(interrupt, x, params.s, theta, omega);
        let mut b = FFTContainer::new(params.n, params.k);
        b.start_default(interrupt, y, params.s, theta, omega);

        a.pointwise_multiply(interrupt, &b);

        let c = &mut a;
        c.backward_fft(interrupt, 0, params.n, omega);

        c.counter_weight_and_recombine(interrupt, theta, params.m, z, params.s);
    }

    /// Multiplicação ponto a ponto das partes.
    pub fn pointwise_multiply(&mut self, interrupt: &mut InterruptCheck, other: &FFTContainer) {
        debug_assert!(self.n == other.n);
        // A condição (m_K & 3) != 0 garante que a FFT interna consiga dividir o trabalho em pelo
        // menos 4 pedaços.
        let use_fft = self.length >= fft::FFT_INNER_THRESHOLD && (self.k & 3) == 0;
        let mut params = fft::Parameters::default();
        if use_fft {
            fft::compute_parameters_inner(self.k, &mut params);
        }
        let length = self.length;
        let temp = self.temp_offset();
        let mut i: usize = 0;
        while i < self.n && !interrupt.interrupted() {
            {
                let (parts, temp_area) = self.storage.split_at_mut(temp);
                let result = &mut temp_area[..2 * length];
                let a = &parts[i * length..(i + 1) * length];
                let b = &other.storage[i * length..(i + 1) * length];
                if use_fft {
                    Self::multiply_inner(interrupt, result, a, b, &params);
                } else {
                    JSBigInt::multiply_zero_padded(interrupt, result, a, b);
                }
            }
            if interrupt.interrupted() {
                return;
            }
            let part = self.part_offset(i);
            fft::mod_fn_double_width(&mut self.storage, part, temp, length);
            // Para melhorar o uso do cache, fazemos aqui o primeiro nível da FFT inversa.
            if i & 1 != 0 {
                let previous = self.part_offset(i - 1);
                fft::sum_diff(&mut self.storage, previous, part, previous, part, length);
            }
            i += 1;
        }
    }
}

/// `shouldUseFFT`: uma transformada cobre x.size() + y.size() dígitos, então essa soma define o
/// ponto de cruzamento contra o Toom-3, cujo custo para um x mais longo cresce com x / y pedaços
/// de produtos do tamanho de y. O menor operando ainda precisa ser largo o bastante para o custo
/// fixo da transformada compensar, e quando x é tão longo que é dividido em pedaços, a transformada
/// de cada pedaço cobre só 2 * y.size(), o que exige o mínimo maior.
pub fn should_use_fft(larger_size: usize, smaller_size: usize) -> bool {
    use fft::*;
    if smaller_size < FFT_MIN_SMALLER_SIZE || larger_size + smaller_size < FFT_THRESHOLD {
        return false;
    }
    if larger_size > ASYMMETRIC_CHUNKING_THRESHOLD * smaller_size {
        return smaller_size >= FFT_CHUNK_THRESHOLD;
    }
    true
}

impl JSBigInt {
    /// `JSBigInt::multiplyToomCook`
    pub fn multiply_toom_cook<'a>(
        interrupt: &mut InterruptCheck,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        debug_assert!(x.len() >= y.len());
        debug_assert!(y.len() >= TOOM_THRESHOLD);
        assert!(result.len() >= x.len() + y.len());
        let z = &mut result[..x.len() + y.len()];
        // toom3Main divide os dois operandos em terços do maior, então um x moderadamente mais
        // longo custa os mesmos cinco produtos de um par balanceado e vence dividir x em pedaços do
        // tamanho de y. Além dessa razão, o preenchimento desperdiça mais que a divisão em pedaços.
        if x.len() * 3 <= y.len() * 5 {
            Self::toom3_main(interrupt, z, x, y);
            return z;
        }
        let k = y.len();
        Self::toom3_main(interrupt, z, &x[..k], y);
        let mut product: Vec<Digit> = vec![0; 2 * k];
        let mut i = k;
        while i < x.len() && !interrupt.interrupted() {
            let xi = clamped_subspan(x, i, k);
            if xi.len() < k {
                // O último pedaço é mais curto, então deixa o despacho por tamanho escolher o
                // algoritmo dele.
                Self::multiply_zero_padded(interrupt, &mut product, xi, y);
            } else {
                Self::toom3_main(interrupt, &mut product, xi, y);
            }
            Self::inplace_add_and_propagate(&mut z[i..], &product);
            i += k;
        }
        z
    }
}
