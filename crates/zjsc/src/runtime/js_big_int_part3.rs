// Terceira fatia do porte de `JSBigInt.cpp`: as primitivas de dígito (`digitAdd`, `digitAdd3`,
// `digitSub`, `digitSub2`, `digitMul`, `digitPow`, `digitDiv`, definidas perto da linha 5360 do .cpp) e
// as linhas 1336 a 1772 (`inplaceAddAndPropagate`, `inplaceSubAndPropagate`,
// `karatsubaAbsoluteDifference`, `multiplyZeroPadded`, `karatsubaMain`, `karatsubaChunk`,
// `karatsubaStart`, `multiplyKaratsuba`, o limiar e os auxiliares de Toom-3 e `toom3Main`).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Dependências de fatias futuras (ainda inexistentes, em snake_case, como funções associadas de
// `JSBigInt`): `inplace_add`, `inplace_sub` (JSBigInt.cpp:2944), `compare_digits`,
// `sub_schoolbook`, `multiply_digits_into`.
//
// Aliasing: onde o C++ chama as operações com Z alias de X ou de Y (`addSigned(R3, R3, ...)`), os
// spans viram `Operand`, que lê da própria fonte ou do buffer de destino, sempre antes de gravar o
// mesmo índice. O resultado é o do C++, sem `unsafe`.

/// `JSBigInt::digitAdd`. `carry` é incrementado de um ou deixado como está.
#[inline]
pub fn digit_add(a: Digit, b: Digit, carry: &mut Digit) -> Digit {
    let result = (a as TwoDigit) + (b as TwoDigit);
    *carry = carry.wrapping_add((result >> DIGIT_BITS) as Digit);
    result as Digit
}

/// `JSBigInt::digitAdd3`. `carry` sai 0 ou 1; `c` deve ser 0 ou 1.
#[inline]
pub fn digit_add3(a: Digit, b: Digit, c: Digit, carry: &mut Digit) -> Digit {
    debug_assert!(c <= 1);
    let (partial, carry_from_partial) = a.overflowing_add(b);
    let (result, carry_from_c) = partial.overflowing_add(c);
    // a + b <= 2^digitBits * 2 - 2, então no máximo uma das duas somas gera vai-um.
    *carry = (carry_from_partial as Digit) | (carry_from_c as Digit);
    result
}

/// `JSBigInt::digitSub`. `borrow` é incrementado de um ou deixado como está.
#[inline]
pub fn digit_sub(a: Digit, b: Digit, borrow: &mut Digit) -> Digit {
    let result = (a as TwoDigit).wrapping_sub(b as TwoDigit);
    *borrow = borrow.wrapping_add(((result >> DIGIT_BITS) as Digit) & 1);
    result as Digit
}

/// `JSBigInt::digitSub2`. `borrow_out` sai 0 ou 1; `borrow_in` deve ser 0 ou 1.
#[inline]
pub fn digit_sub2(a: Digit, b: Digit, borrow_in: Digit, borrow_out: &mut Digit) -> Digit {
    debug_assert!(borrow_in <= 1);
    let (partial, borrow_from_partial) = a.overflowing_sub(b);
    let (result, borrow_from_borrow_in) = partial.overflowing_sub(borrow_in);
    // b + borrowIn <= 2^digitBits, então no máximo uma das duas subtrações gera empréstimo.
    *borrow_out = (borrow_from_partial as Digit) | (borrow_from_borrow_in as Digit);
    result
}

/// `JSBigInt::digitMul`: devolve `(low, high)`.
#[inline]
pub fn digit_mul(a: Digit, b: Digit) -> (Digit, Digit) {
    let result = (a as TwoDigit) * (b as TwoDigit);
    let high = (result >> DIGIT_BITS) as Digit;
    let low = result as Digit;
    (low, high)
}

/// `JSBigInt::digitPow`: eleva `base` a `exponent`, sem checar estouro.
pub fn digit_pow(mut base: Digit, mut exponent: Digit) -> Digit {
    let mut result: Digit = 1;
    while exponent > 0 {
        if exponent & 1 != 0 {
            result = result.wrapping_mul(base);
        }
        exponent >>= 1;
        base = base.wrapping_mul(base);
    }
    result
}

/// `JSBigInt::digitDiv`: devolve o quociente de `(high << digitBits + low) / divisor`, com o resto
/// em `remainder`. Invariante: `high < divisor` (o quociente cabe em um `Digit`). No x86_64 o C++
/// usa `divq`, que dá exatamente o quociente e o resto da divisão de 128 por 64 bits.
#[inline]
pub fn digit_div(high: Digit, low: Digit, divisor: Digit, remainder: &mut Digit) -> Digit {
    debug_assert!(high < divisor);
    let dividend = ((high as TwoDigit) << DIGIT_BITS) | (low as TwoDigit);
    let divisor = divisor as TwoDigit;
    *remainder = (dividend % divisor) as Digit;
    (dividend / divisor) as Digit
}

/// Operando de uma operação elemento a elemento que pode ser o próprio buffer de destino
/// (`data == None`), o caso `Z may alias either operand` do C++. `len` é o tamanho lógico do span.
#[derive(Clone, Copy)]
struct Operand<'a> {
    data: Option<&'a [Digit]>,
    len: usize,
}

impl<'a> Operand<'a> {
    fn slice(data: &'a [Digit]) -> Operand<'a> {
        Operand { data: Some(data), len: data.len() }
    }

    /// Operando que é o próprio destino (mesmo span que Z), com `len` dígitos.
    fn in_place(len: usize) -> Operand<'a> {
        Operand { data: None, len }
    }

    #[inline(always)]
    fn get(&self, z: &[Digit], i: usize) -> Digit {
        match self.data {
            Some(data) => data[i],
            None => z[i],
        }
    }

    /// `normalize(x)`.
    fn normalized(self, z: &[Digit]) -> Operand<'a> {
        let mut len = self.len;
        while len > 0 && self.get(z, len - 1) == 0 {
            len -= 1;
        }
        Operand { data: self.data, len }
    }
}

/// `toomThreshold`: Toom-3 vence a partir de 480 dígitos do menor operando.
pub const TOOM_THRESHOLD: usize = 480;

/// `addZeroPadded`: Z := X + Y, com Z preenchido por zeros.
fn add_zero_padded(z: &mut [Digit], x: Operand, y: Operand) {
    let (x, y) = if x.len < y.len { (y, x) } else { (x, y) };
    debug_assert!(z.len() >= x.len);
    let mut carry: Digit = 0;
    let mut i = 0usize;
    while i < y.len {
        let mut new_carry: Digit = 0;
        let value = digit_add3(x.get(z, i), y.get(z, i), carry, &mut new_carry);
        z[i] = value;
        carry = new_carry;
        i += 1;
    }
    while i < x.len {
        let mut new_carry: Digit = 0;
        let value = digit_add(x.get(z, i), carry, &mut new_carry);
        z[i] = value;
        carry = new_carry;
        i += 1;
    }
    while i < z.len() {
        z[i] = carry;
        carry = 0;
        i += 1;
    }
}

/// `subZeroPadded`: Z := X - Y para X >= Y normalizados, com Z preenchido por zeros.
fn sub_zero_padded(z: &mut [Digit], x: Operand, y: Operand) {
    debug_assert!(z.len() >= x.len && x.len >= y.len);
    let mut borrow: Digit = 0;
    let mut i = 0usize;
    while i < y.len {
        let mut new_borrow: Digit = 0;
        let value = digit_sub2(x.get(z, i), y.get(z, i), borrow, &mut new_borrow);
        z[i] = value;
        borrow = new_borrow;
        i += 1;
    }
    while i < x.len {
        let mut new_borrow: Digit = 0;
        let value = digit_sub(x.get(z, i), borrow, &mut new_borrow);
        z[i] = value;
        borrow = new_borrow;
        i += 1;
    }
    debug_assert!(borrow == 0);
    while i < z.len() {
        z[i] = 0;
        i += 1;
    }
}

/// `lessThanNormalized`: os dois operandos já normalizados.
fn less_than_normalized(z: &[Digit], x: Operand, y: Operand) -> bool {
    if x.len != y.len {
        return x.len < y.len;
    }
    let mut i = x.len;
    while i > 0 {
        i -= 1;
        let (xi, yi) = (x.get(z, i), y.get(z, i));
        if xi != yi {
            return xi < yi;
        }
    }
    false
}

/// `addSigned`: Z := X + Y em sinal e magnitude, devolvendo o sinal de Z.
fn add_signed(z: &mut [Digit], x: Operand, x_negative: bool, y: Operand, y_negative: bool) -> bool {
    if x_negative == y_negative {
        add_zero_padded(z, x, y);
        return x_negative;
    }
    let x = x.normalized(z);
    let y = y.normalized(z);
    if !less_than_normalized(z, x, y) {
        sub_zero_padded(z, x, y);
        return x_negative;
    }
    sub_zero_padded(z, y, x);
    !x_negative
}

/// `subtractSigned`: Z := X - Y em sinal e magnitude, devolvendo o sinal de Z.
fn subtract_signed(z: &mut [Digit], x: Operand, x_negative: bool, y: Operand, y_negative: bool) -> bool {
    if x_negative != y_negative {
        add_zero_padded(z, x, y);
        return x_negative;
    }
    let x = x.normalized(z);
    let y = y.normalized(z);
    if !less_than_normalized(z, x, y) {
        sub_zero_padded(z, x, y);
        return x_negative;
    }
    sub_zero_padded(z, y, x);
    !x_negative
}

/// `timesTwo`
fn times_two(x: &mut [Digit]) {
    let mut carry: Digit = 0;
    for digit in x.iter_mut() {
        let d = *digit;
        *digit = (d << 1) | carry;
        carry = d >> (DIGIT_BITS - 1);
    }
}

/// `divideByTwo`
fn divide_by_two(x: &mut [Digit]) {
    let mut carry: Digit = 0;
    let mut i = x.len();
    while i > 0 {
        i -= 1;
        let d = x[i];
        x[i] = (d >> 1) | carry;
        carry = d << (DIGIT_BITS - 1);
    }
}

/// `divideByThree`
fn divide_by_three(x: &mut [Digit]) {
    let mut remainder: Digit = 0;
    let mut i = x.len();
    while i > 0 {
        i -= 1;
        let d = x[i];
        let upper = (remainder << HALF_DIGIT_BITS) | (d >> HALF_DIGIT_BITS);
        let upper_result = upper / 3;
        remainder = upper - 3 * upper_result;
        let lower = (remainder << HALF_DIGIT_BITS) | (d & HALF_DIGIT_MASK);
        let lower_result = lower / 3;
        remainder = lower - 3 * lower_result;
        x[i] = (upper_result << HALF_DIGIT_BITS) | lower_result;
    }
}

impl JSBigInt {
    /// `JSBigInt::inplaceAddAndPropagate`
    pub fn inplace_add_and_propagate(z: &mut [Digit], x: &[Digit]) -> Digit {
        let x = normalize(x);
        assert!(z.len() >= x.len());
        let mut carry = Self::inplace_add(z, x);
        let mut i = x.len();
        while i < z.len() && carry != 0 {
            let mut new_carry: Digit = 0;
            z[i] = digit_add(z[i], carry, &mut new_carry);
            carry = new_carry;
            i += 1;
        }
        carry
    }

    /// `JSBigInt::inplaceSubAndPropagate`
    pub fn inplace_sub_and_propagate(z: &mut [Digit], x: &[Digit]) -> Digit {
        let x = normalize(x);
        assert!(z.len() >= x.len());
        let mut borrow = Self::inplace_sub(z, x);
        let mut i = x.len();
        while i < z.len() && borrow != 0 {
            let mut new_borrow: Digit = 0;
            z[i] = digit_sub(z[i], borrow, &mut new_borrow);
            borrow = new_borrow;
            i += 1;
        }
        borrow
    }

    /// `JSBigInt::karatsubaAbsoluteDifference`
    pub fn karatsuba_absolute_difference(result: &mut [Digit], x: &[Digit], y: &[Digit], negative: &mut bool) {
        let mut x = normalize(x);
        let mut y = normalize(y);
        if matches!(Self::compare_digits(x, y), ComparisonResult::LessThan) {
            *negative = !*negative;
            core::mem::swap(&mut x, &mut y);
        }
        let difference_len = Self::sub_schoolbook(x, y, &mut *result).len();
        result[difference_len..].fill(0);
    }

    /// `JSBigInt::multiplyZeroPadded`
    pub fn multiply_zero_padded(interrupt: &mut InterruptCheck, result: &mut [Digit], x: &[Digit], y: &[Digit]) {
        let mut x = normalize(x);
        let mut y = normalize(y);
        if x.len() < y.len() {
            core::mem::swap(&mut x, &mut y);
        }
        if y.is_empty() {
            result.fill(0);
            return;
        }
        let product_len = Self::multiply_digits_into(interrupt, x, y, &mut *result).len();
        result[product_len..].fill(0);
        // Os algoritmos sub-quadráticos contam o trabalho nos casos base que despacham para cá,
        // pois as próprias passadas sobre os dígitos são lineares em comparação.
        if y.len() < KARATSUBA_THRESHOLD {
            interrupt.add_work(x.len() * y.len());
        }
    }

    /// `JSBigInt::karatsubaMain`
    pub fn karatsuba_main(
        interrupt: &mut InterruptCheck,
        z: &mut [Digit],
        x: &[Digit],
        y: &[Digit],
        scratch: &mut [Digit],
        n: usize,
    ) {
        if n < KARATSUBA_THRESHOLD {
            let end = z.len().min(2 * n);
            Self::multiply_zero_padded(interrupt, &mut z[..end], x, y);
            return;
        }
        debug_assert!(scratch.len() >= 4 * n);
        debug_assert!(n & 1 == 0);
        let n2 = n >> 1;
        let x0 = clamped_subspan(x, 0, n2);
        let x1 = clamped_subspan(x, n2, n2);
        let y0 = clamped_subspan(y, 0, n2);
        let y1 = clamped_subspan(y, n2, n2);
        // `scratch.first(2n)` guarda p0/p2/p1 e as diferenças; `scratch.subspan(2n, 2n)` é a recursão.
        let (own, recursion) = scratch.split_at_mut(2 * n);
        let recursion = &mut recursion[..2 * n];

        Self::karatsuba_main(interrupt, &mut own[..n], x0, y0, recursion, n2);
        if interrupt.interrupted() {
            return;
        }
        z[..n].copy_from_slice(&own[..n]);

        Self::karatsuba_main(interrupt, &mut own[n..2 * n], x1, y1, recursion, n2);
        if interrupt.interrupted() {
            return;
        }
        {
            let z2 = &mut z[n..];
            let end = z2.len().min(n);
            z2[..end].copy_from_slice(&own[n..n + end]);
        }

        let mut overflow = Self::inplace_add_and_propagate(&mut z[n2..], &own[..n]);
        overflow = overflow.wrapping_add(Self::inplace_add_and_propagate(&mut z[n2..], &own[n..2 * n]));

        // xDifference e yDifference reaproveitam o lugar de p0; p1 reaproveita o de p2.
        let (low, p1) = own.split_at_mut(n);
        let (x_difference, y_difference) = low.split_at_mut(n2);
        let mut negative = false;
        Self::karatsuba_absolute_difference(x_difference, x1, x0, &mut negative);
        Self::karatsuba_absolute_difference(y_difference, y0, y1, &mut negative);
        Self::karatsuba_main(interrupt, p1, x_difference, y_difference, recursion, n2);
        if interrupt.interrupted() {
            return;
        }
        if negative {
            overflow = overflow.wrapping_sub(Self::inplace_sub_and_propagate(&mut z[n2..], p1));
        } else {
            overflow = overflow.wrapping_add(Self::inplace_add_and_propagate(&mut z[n2..], p1));
        }
        debug_assert!(overflow == 0);
        let _ = overflow;
    }

    /// `JSBigInt::karatsubaChunk`
    pub fn karatsuba_chunk(interrupt: &mut InterruptCheck, z: &mut [Digit], x: &[Digit], y: &[Digit], scratch: &mut [Digit]) {
        let mut x = normalize(x);
        let mut y = normalize(y);
        if x.len() < y.len() {
            core::mem::swap(&mut x, &mut y);
        }
        if y.len() < KARATSUBA_THRESHOLD {
            Self::multiply_zero_padded(interrupt, z, x, y);
            return;
        }
        let k = karatsuba_length(y.len());
        debug_assert!(scratch.len() >= 4 * k);
        Self::karatsuba_start(interrupt, z, x, y, scratch, k);
    }

    /// `JSBigInt::karatsubaStart`
    pub fn karatsuba_start(
        interrupt: &mut InterruptCheck,
        z: &mut [Digit],
        x: &[Digit],
        y: &[Digit],
        scratch: &mut [Digit],
        k: usize,
    ) {
        Self::karatsuba_main(interrupt, z, x, y, scratch, k);
        if interrupt.interrupted() {
            return;
        }
        if z.len() > 2 * k {
            z[2 * k..].fill(0);
        }
        if k >= y.len() && x.len() == y.len() {
            return;
        }

        let mut product: Vec<Digit> = vec![0; 2 * k];
        let x0 = clamped_subspan(x, 0, k);
        let y0 = clamped_subspan(y, 0, k);
        let y1 = clamped_subspan(y, k, y.len());
        if !y1.is_empty() {
            Self::karatsuba_chunk(interrupt, &mut product, x0, y1, scratch);
            if interrupt.interrupted() {
                return;
            }
            Self::inplace_add_and_propagate(&mut z[k..], &product);
        }
        let mut i = k;
        while i < x.len() {
            let xi = clamped_subspan(x, i, k);
            Self::karatsuba_chunk(interrupt, &mut product, xi, y0, scratch);
            if interrupt.interrupted() {
                return;
            }
            Self::inplace_add_and_propagate(&mut z[i..], &product);
            if !y1.is_empty() {
                Self::karatsuba_chunk(interrupt, &mut product, xi, y1, scratch);
                if interrupt.interrupted() {
                    return;
                }
                Self::inplace_add_and_propagate(&mut z[i + k..], &product);
            }
            i += k;
        }
    }

    /// `JSBigInt::multiplyKaratsuba`
    pub fn multiply_karatsuba<'a>(
        interrupt: &mut InterruptCheck,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        debug_assert!(x.len() >= y.len());
        debug_assert!(y.len() >= KARATSUBA_THRESHOLD);
        assert!(result.len() >= x.len() + y.len());
        let k = karatsuba_length(y.len());
        let mut scratch: Vec<Digit> = vec![0; 4 * k];
        let z = &mut result[..x.len() + y.len()];
        Self::karatsuba_start(interrupt, z, x, y, &mut scratch, k);
        z
    }

    /// `JSBigInt::toom3Main`
    pub fn toom3_main(interrupt: &mut InterruptCheck, z: &mut [Digit], x: &[Digit], y: &[Digit]) {
        debug_assert!(z.len() >= x.len() + y.len());
        // Fase 1: divisão em três partes.
        let i = (x.len().max(y.len()) + 2) / 3;
        let x0 = clamped_subspan(x, 0, i);
        let x1 = clamped_subspan(x, i, i);
        let x2 = clamped_subspan(x, 2 * i, i);
        let y0 = clamped_subspan(y, 0, i);
        let y1 = clamped_subspan(y, i, i);
        let y2 = clamped_subspan(y, 2 * i, i);

        // Armazenamento temporário.
        let p_length = i + 1; // Para todos os px, qx abaixo.
        let r_length = 2 * p_length; // Para todos os r_x, Rx abaixo.
        let mut temp_storage: Vec<Digit> = vec![0; 4 * r_length];
        // Reuso do armazenamento temporário (as quatro faixas de `r_length` dígitos):
        //
        //   a [0 .. rL)      | po, qo -> pm1, qm1 -> rm2 (R3)
        //   b [rL .. 2rL)    | p1, q1 -> pm2, qm2 -> rinf (R4)
        //   r1 [2rL .. 3rL)  | r1 (R1)
        //   rm1 [3rL .. 4rL) | rm1 (R2)
        //
        // O C++ intercala as fases 2 e 3: depois de r1 = p1 * q1, o lugar de p1 vira pm2.
        let (a, rest) = temp_storage.split_at_mut(r_length);
        let (b, rest) = rest.split_at_mut(r_length);
        let (r1, rm1) = rest.split_at_mut(r_length);

        // Fase 2a: avaliação, passos 0, 1, m1.
        // po = X0 + X2
        add_zero_padded(&mut a[..p_length], Operand::slice(x0), Operand::slice(x2));
        // p1 = po + X1
        add_zero_padded(&mut b[..p_length], Operand::slice(&a[..p_length]), Operand::slice(x1));
        // pm1 = po - X1
        let pm1_sign =
            subtract_signed(&mut a[..p_length], Operand::in_place(p_length), false, Operand::slice(x1), false);

        // qo = Y0 + Y2
        add_zero_padded(&mut a[p_length..2 * p_length], Operand::slice(y0), Operand::slice(y2));
        // q1 = qo + Y1
        add_zero_padded(
            &mut b[p_length..2 * p_length],
            Operand::slice(&a[p_length..2 * p_length]),
            Operand::slice(y1),
        );
        // qm1 = qo - Y1
        let qm1_sign = subtract_signed(
            &mut a[p_length..2 * p_length],
            Operand::in_place(p_length),
            false,
            Operand::slice(y1),
            false,
        );

        // Fase 3a: multiplicação ponto a ponto, passos 0, 1, m1.
        Self::multiply_zero_padded(interrupt, &mut z[..r_length], x0, y0);
        Self::multiply_zero_padded(interrupt, r1, &b[..p_length], &b[p_length..2 * p_length]);
        Self::multiply_zero_padded(interrupt, rm1, &a[..p_length], &a[p_length..2 * p_length]);
        let rm1_sign = pm1_sign != qm1_sign;

        // Fase 2b: avaliação, passos m2 e inf.
        // pm2 = (pm1 + X2) * 2 - X0
        let mut pm2_sign = add_signed(
            &mut b[..p_length],
            Operand::slice(&a[..p_length]),
            pm1_sign,
            Operand::slice(x2),
            false,
        );
        times_two(&mut b[..p_length]);
        pm2_sign = subtract_signed(
            &mut b[..p_length],
            Operand::in_place(p_length),
            pm2_sign,
            Operand::slice(x0),
            false,
        );

        // qm2 = (qm1 + Y2) * 2 - Y0
        let mut qm2_sign = add_signed(
            &mut b[p_length..2 * p_length],
            Operand::slice(&a[p_length..2 * p_length]),
            qm1_sign,
            Operand::slice(y2),
            false,
        );
        times_two(&mut b[p_length..2 * p_length]);
        qm2_sign = subtract_signed(
            &mut b[p_length..2 * p_length],
            Operand::in_place(p_length),
            qm2_sign,
            Operand::slice(y0),
            false,
        );

        // Fase 3b: multiplicação ponto a ponto, passos m2 e inf.
        Self::multiply_zero_padded(interrupt, a, &b[..p_length], &b[p_length..2 * p_length]);
        let rm2_sign = pm2_sign != qm2_sign;

        Self::multiply_zero_padded(interrupt, b, x2, y2);
        if interrupt.interrupted() {
            return;
        }

        // Fase 4: interpolação. R0 = z[..rL], R1 = r1, R2 = rm1, R3 = a (rm2), R4 = b (rinf).
        // R3 <- (rm2 - r1) / 3
        let mut r3_sign = subtract_signed(a, Operand::in_place(r_length), rm2_sign, Operand::slice(r1), false);
        divide_by_three(a);
        // R1 <- (r1 - rm1) / 2
        let mut r1_sign = subtract_signed(r1, Operand::in_place(r_length), false, Operand::slice(rm1), rm1_sign);
        divide_by_two(r1);
        // R2 <- rm1 - r0
        let mut r2_sign =
            subtract_signed(rm1, Operand::in_place(r_length), rm1_sign, Operand::slice(&z[..r_length]), false);
        // R3 <- (R2 - R3) / 2 + 2 * rinf
        r3_sign = subtract_signed(a, Operand::slice(rm1), r2_sign, Operand::in_place(r_length), r3_sign);
        divide_by_two(a);
        r3_sign = add_signed(a, Operand::in_place(r_length), r3_sign, Operand::slice(b), false);
        r3_sign = add_signed(a, Operand::in_place(r_length), r3_sign, Operand::slice(b), false);
        // R2 <- R2 + R1 - R4
        r2_sign = add_signed(rm1, Operand::in_place(r_length), r2_sign, Operand::slice(r1), r1_sign);
        r2_sign = subtract_signed(rm1, Operand::in_place(r_length), r2_sign, Operand::slice(b), false);
        // R1 <- R1 - R3
        r1_sign = subtract_signed(r1, Operand::in_place(r_length), r1_sign, Operand::slice(a), r3_sign);

        debug_assert!(!r1_sign || normalize(&*r1).is_empty());
        debug_assert!(!r2_sign || normalize(&*rm1).is_empty());
        debug_assert!(!r3_sign || normalize(&*a).is_empty());
        let _ = (r1_sign, r2_sign, r3_sign);

        // Fase 5: recomposição. R0 já está no lugar. Não há estouro possível.
        z[r_length..].fill(0);
        Self::inplace_add_and_propagate(&mut z[i..], r1);
        Self::inplace_add_and_propagate(&mut z[2 * i..], rm1);
        Self::inplace_add_and_propagate(&mut z[3 * i..], a);
        Self::inplace_add_and_propagate(&mut z[4 * i..], b);
    }
}
