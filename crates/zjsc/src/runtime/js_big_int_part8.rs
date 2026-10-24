// Oitava fatia do porte de `JSBigInt.cpp`: as linhas 2825 a 3770 (o `DigitDiv`, `divideSingle`,
// `leftShift`, `rightShift`, `divideSchoolbook`, `rightShiftZeroPadded`, `addDigit`, `subtractDigit`,
// a divisão de Burnikel-Ziegler, a inversão (base e Newton) e `divideBarrett` de sete argumentos).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Desvios mecânicos de Rust seguro, sem efeito observável:
// - Onde o C++ admite `Z` e `X` sobrepostos em `leftShift`/`rightShift`/`divideSingle` (em lugar),
//   o chamador copia a origem antes (a fatia 7 já faz isso); aqui os dois sempre são disjuntos.
// - Em `divideBurnikelZiegler` o `temp` único de `n * 5` dígitos vira quatro `Vec` (B, Z, Ri, Qi),
//   pois o empréstimo seguro não admite várias subfatias mutáveis vivas do mesmo buffer.
// - Em `invertNewton` o scratch é dividido em dois com `split_at_mut(uOffset)`: S e W vivem antes
//   de `uOffset`, U depois dele (o C++ garante que não se sobrepõem).
// - `divideBarrett` de cinco argumentos, `divideDigitsInto` e os demais ainda não são portados aqui.

/// `barrettThreshold` com `CPU(REGISTER64)`.
#[allow(dead_code)]
const BARRETT_THRESHOLD: usize = 13000;
/// `newtonInversionThreshold`
const NEWTON_INVERSION_THRESHOLD: usize = 25;
/// `burnikelThreshold`
const BURNIKEL_THRESHOLD: usize = 57;
/// `invertNewtonExtraSpace`
const INVERT_NEWTON_EXTRA_SPACE: usize = 5;

/// `divideBarrettScratchSpace`
pub const fn divide_barrett_scratch_space(n: usize) -> usize {
    n + 2
}

/// `invertNewtonScratchSpace`
pub const fn invert_newton_scratch_space(n: usize) -> usize {
    3 * n + 2 * INVERT_NEWTON_EXTRA_SPACE
}

/// `invertScratchSpace`
pub const fn invert_scratch_space(n: usize) -> usize {
    if n < NEWTON_INVERSION_THRESHOLD {
        2 * n
    } else {
        invert_newton_scratch_space(n)
    }
}

/// `DigitDiv`: divisão de dois dígitos por um dígito normalizado com o inverso pré-calculado.
struct DigitDiv {
    divisor: Digit,
    inverse: Digit,
}

impl DigitDiv {
    fn new(d: Digit) -> DigitDiv {
        // `d` já está normalizado.
        debug_assert!(d & (1 << (DIGIT_BITS - 1)) != 0);
        let limit: TwoDigit = !0;
        DigitDiv { divisor: d, inverse: (limit / d as TwoDigit) as Digit }
    }

    #[inline(always)]
    fn div(&self, high: Digit, low: Digit, remainder: &mut Digit) -> Digit {
        debug_assert!(high < self.divisor);
        let (u1, u0, v, d) = (high, low, self.inverse, self.divisor);
        // 1. q = ((v * u1) / beta) + u1
        let mut q: Digit = u1.wrapping_add((((u1 as TwoDigit) * (v as TwoDigit)) >> DIGIT_BITS) as Digit);
        // 2. <p1, p0> = q * d
        let p: TwoDigit = (q as TwoDigit).wrapping_mul(d as TwoDigit);
        // 3. <r1, r0> = <u1, u0> - <p1, p0>
        let u: TwoDigit = ((u1 as TwoDigit) << DIGIT_BITS) | u0 as TwoDigit;
        let mut rem: TwoDigit = u.wrapping_sub(p);
        // 4. while (r1 > 0 || r0 >= d) { q++; <r1, r0> = <r1, r0> - d; }
        while rem >= d as TwoDigit {
            q = q.wrapping_add(1);
            rem -= d as TwoDigit;
        }
        *remainder = rem as Digit;
        q
    }
}

/// `JSBigInt::productGreaterThan`: se `(factor1 * factor2) > (high << digitBits) + low`.
#[inline]
fn product_greater_than(factor1: Digit, factor2: Digit, high: Digit, low: Digit) -> bool {
    let (result_low, result_high) = digit_mul(factor1, factor2);
    result_high > high || (result_high == high && result_low > low)
}

/// `JSBigInt::greaterThanOrEqual`
fn greater_than_or_equal(a: &[Digit], b: &[Digit]) -> bool {
    debug_assert!(a.len() == b.len());
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

/// `addDigit`: X += y para um dígito y. X precisa ter espaço para o carry.
fn add_digit(x: &mut [Digit], y: Digit) {
    let mut carry = y;
    let mut i = 0usize;
    while carry != 0 {
        let mut new_carry: Digit = 0;
        x[i] = digit_add(x[i], carry, &mut new_carry);
        carry = new_carry;
        i += 1;
    }
}

/// `subtractDigit`: X -= y para um dígito y <= X.
fn subtract_digit(x: &mut [Digit], y: Digit) {
    let mut borrow = y;
    let mut i = 0usize;
    while borrow != 0 {
        let mut new_borrow: Digit = 0;
        x[i] = digit_sub(x[i], borrow, &mut new_borrow);
        borrow = new_borrow;
        i += 1;
    }
}

/// `compareWithHighDigit`: compara `[aHigh, A]` com B, devolvendo o sinal da diferença.
fn compare_with_high_digit(a_high: Digit, a: &[Digit], b: &[Digit]) -> i32 {
    let b = normalize(b);
    let (a, a_length) = if a_high == 0 {
        let a = normalize(a);
        (a, a.len())
    } else {
        (a, a.len() + 1)
    };
    if a_length != b.len() {
        return if a_length < b.len() { -1 } else { 1 };
    }
    let mut i = a_length;
    if a_high != 0 {
        i -= 1;
        if a_high != b[i] {
            return if a_high < b[i] { -1 } else { 1 };
        }
    }
    while i > 0 {
        i -= 1;
        if a[i] != b[i] {
            return if a[i] < b[i] { -1 } else { 1 };
        }
    }
    0
}

/// `assertIntegerPartRange`
fn assert_integer_part_range(x: &[Digit], min: Digit, max: Digit) {
    let integer_part = *x.last().unwrap();
    debug_assert!(integer_part >= min);
    debug_assert!(integer_part <= max);
}

/// `JSBigInt::BurnikelZiegler`: a recursão guarda os dados que não mudam neste objeto.
struct BurnikelZiegler<'a, 'b> {
    interrupt: &'b mut InterruptCheck<'a>,
    scratch: Vec<Digit>,
}

impl<'a, 'b> BurnikelZiegler<'a, 'b> {
    fn new(interrupt: &'b mut InterruptCheck<'a>, scratch_space: usize) -> Self {
        let scratch = vec![0; if scratch_space >= BURNIKEL_THRESHOLD { scratch_space } else { 0 }];
        BurnikelZiegler { interrupt, scratch }
    }

    /// `BurnikelZiegler::divideBasecase`
    fn divide_basecase(&mut self, q: &mut [Digit], r: &mut [Digit], a: &[Digit], b: &[Digit]) {
        let a = normalize(a);
        let b = normalize(b);
        debug_assert!(!b.is_empty());
        let comparison = JSBigInt::compare_digits(a, b);
        if !matches!(comparison, ComparisonResult::GreaterThan) {
            q.fill(0);
            if matches!(comparison, ComparisonResult::Equal) {
                // If A == B, then Q=1, R=0.
                r.fill(0);
                q[0] = 1;
            } else {
                // If A < B, then Q=0, R=A.
                copy_zero_padded(r, a);
            }
            return;
        }
        if b.len() == 1 {
            let mut remainder: Digit = 0;
            let quotient_len = JSBigInt::divide_single(&mut *q, &mut remainder, a, b[0]).len();
            q[quotient_len..].fill(0);
            r[0] = remainder;
            r[1..].fill(0);
            self.interrupt.add_work(a.len());
            return;
        }
        let (quotient_len, remainder_len) = {
            let (quotient, remainder) = JSBigInt::divide_schoolbook(&mut *q, &mut *r, a, b, Some(&mut *self.interrupt));
            (quotient.len(), remainder.len())
        };
        q[quotient_len..].fill(0);
        r[remainder_len..].fill(0);
    }

    /// `BurnikelZiegler::d3n2n`: algoritmo 2 do artigo.
    /// Devolve Q e R para A/B, com B tendo dois terços do tamanho de A = [A1, A2, A3].
    fn d3n2n(&mut self, q: &mut [Digit], r: &mut [Digit], a1a2: &[Digit], a3: &[Digit], b: &[Digit]) {
        debug_assert!(b.len() & 1 == 0);
        let n = b.len() / 2;
        debug_assert!(a1a2.len() == 2 * n);
        debug_assert!(matches!(JSBigInt::compare_digits(a1a2, b), ComparisonResult::LessThan));
        debug_assert!(a3.len() == n);
        debug_assert!(q.len() == n);
        debug_assert!(r.len() == 2 * n);
        // 1. Split A into three parts A = [A1, A2, A3] with Ai < 2^(digitBits * n).
        let a1 = &a1a2[n..2 * n];
        // 2. Split B into two parts B = [B1, B2] with Bi < 2^(digitBits * n).
        let b1 = &b[n..2 * n];
        let b2 = &b[..n];
        // 3. Distinguish the cases A1 < B1 or A1 >= B1.
        let mut r1_high: Digit = 0;
        {
            let r1 = &mut r[n..2 * n];
            if matches!(JSBigInt::compare_digits(a1, b1), ComparisonResult::LessThan) {
                // 3a. If A1 < B1, compute Qhat = floor([A1, A2] / B1) with remainder R1 using
                //     algorithm D2n1n.
                self.d2n1n(&mut *q, r1, a1a2, b1);
                if self.interrupt.interrupted() {
                    return;
                }
            } else {
                // 3b. If A1 >= B1, set Qhat = 2^(digitBits * n) - 1 and set
                //     R1 = [A1, A2] - [B1, 0] + [0, B1]
                q.fill(!0);
                // Step 1: compute A1 - B1, which can't underflow because of the comparison guarding
                // this else-branch, and always has a one-digit result because of this function's
                // preconditions.
                sub_zero_padded(&mut *r1, Operand::slice(normalize(a1)), Operand::slice(normalize(b1)));
                let difference = normalize(&r1[..]);
                debug_assert!(difference.len() <= 1);
                if !difference.is_empty() {
                    r1_high = difference[0];
                }
                // Step 2: compute A2 + B1.
                let a2 = &a1a2[..n];
                r1_high = r1_high.wrapping_add(JSBigInt::add_and_return_carry(r1, a2, b1));
            }
        }
        // 4. Compute D = Qhat * B2 using (Karatsuba) multiplication.
        let d = &mut self.scratch[..2 * n];
        JSBigInt::multiply_zero_padded(&mut *self.interrupt, &mut *d, q, b2);
        if self.interrupt.interrupted() {
            return;
        }

        // 5. Compute Rhat = R1*2^(digitBits * n) + A3 - D = [R1, A3] - D.
        copy_zero_padded(&mut r[..n], a3);
        // 6. As long as Rhat < 0, repeat:
        while compare_with_high_digit(r1_high, r, d) < 0 {
            // 6a. Rhat = Rhat + B
            r1_high = r1_high.wrapping_add(JSBigInt::inplace_add(r, b));
            // 6b. Qhat = Qhat - 1
            subtract_digit(q, 1);
        }
        // 5. Compute Rhat = R1*2^(digitBits * n) + A3 - D = [R1, A3] - D.
        let borrow = JSBigInt::inplace_sub(r, d);
        debug_assert!(borrow == r1_high);
        let _ = borrow;
        debug_assert!(matches!(JSBigInt::compare_digits(r, b), ComparisonResult::LessThan));
        // 7. Return R = Rhat, Q = Qhat.
    }

    /// `BurnikelZiegler::d2n1n`: algoritmo 1 do artigo.
    /// Devolve Q e R para A/B, com A tendo o dobro do tamanho de B.
    fn d2n1n(&mut self, q: &mut [Digit], r: &mut [Digit], a: &[Digit], b: &[Digit]) {
        let n = b.len();
        debug_assert!(a.len() <= 2 * n);
        debug_assert!(matches!(JSBigInt::compare_digits(clamped_subspan(a, n, n), b), ComparisonResult::LessThan));
        debug_assert!(q.len() == n);
        debug_assert!(r.len() == n);
        // 1. If n is odd or smaller than some convenient constant, compute Q and R by school
        //    division and return.
        if (n & 1) != 0 || n < BURNIKEL_THRESHOLD {
            return self.divide_basecase(q, r, a, b);
        }
        // 2. Split A into four parts A = [A1, ..., A4] with Ai < 2^(digitBits * n / 2). Split B
        //    into two parts [B2, B1] with Bi < 2^(digitBits * n / 2).
        let a1a2 = clamped_subspan(a, n, n);
        let a3 = clamped_subspan(a, n / 2, n / 2);
        let a4 = clamped_subspan(a, 0, n / 2);
        // 3. Compute the high part Q1 of floor(A/B) as Q1 = floor([A1, A2, A3] / [B1, B2]) with
        //    remainder R1 = [R11, R12], using algorithm D3n2n.
        let (q2, q1) = q.split_at_mut(n / 2);
        let mut r1: Vec<Digit> = vec![0; n];
        self.d3n2n(q1, &mut r1, a1a2, a3, b);
        if self.interrupt.interrupted() {
            return;
        }
        // 4. Compute the low part Q2 of floor(A/B) as Q2 = floor([R11, R12, A4] / [B1, B2]) with
        //    remainder R, using algorithm D3n2n.
        self.d3n2n(q2, r, &r1, a4, b);
        // 5. Return Q = [Q1, Q2] and R.
    }
}

impl JSBigInt {
    /// `JSBigInt::divideSingle`: computa Q(uociente) e o resto de A/b, tais que
    /// Q = (A - remainder) / b, com 0 <= remainder < b. Se `q` é vazio, só o resto é devolvido.
    pub fn divide_single<'a>(q: &'a mut [Digit], remainder: &mut Digit, a: &[Digit], b: Digit) -> &'a mut [Digit] {
        assert!(b != 0);
        assert!(!a.is_empty());
        *remainder = 0;
        let length = a.len();
        if !q.is_empty() {
            if a[length - 1] >= b {
                assert!(q.len() >= a.len());
                for i in (0..length).rev() {
                    let mut new_remainder: Digit = 0;
                    q[i] = digit_div(*remainder, a[i], b, &mut new_remainder);
                    *remainder = new_remainder;
                }
                return &mut q[..length];
            }

            assert!(q.len() >= a.len() - 1);
            *remainder = a[length - 1];
            for i in (0..length - 1).rev() {
                let mut new_remainder: Digit = 0;
                q[i] = digit_div(*remainder, a[i], b, &mut new_remainder);
                *remainder = new_remainder;
            }
            return &mut q[..length - 1];
        }

        for i in (0..length).rev() {
            let mut new_remainder: Digit = 0;
            digit_div(*remainder, a[i], b, &mut new_remainder);
            *remainder = new_remainder;
        }
        q
    }

    /// `spanCopy`: Z := X (sem sobreposição em Rust seguro), devolvendo `z.first(x.size())`.
    fn span_copy<'a>(z: &'a mut [Digit], x: &[Digit]) -> &'a mut [Digit] {
        z[..x.len()].copy_from_slice(x);
        &mut z[..x.len()]
    }

    /// `JSBigInt::leftShift(span, span, unsigned)`: Z := X << shift.
    pub fn left_shift<'a>(z: &'a mut [Digit], x: &[Digit], shift: u32) -> &'a mut [Digit] {
        debug_assert!(shift < DIGIT_BITS);
        debug_assert!(z.len() >= x.len());
        if shift == 0 {
            return Self::span_copy(z, x);
        }

        let mut carry: Digit = 0;
        let mut i = 0usize;
        while i < x.len() {
            let d = x[i];
            z[i] = (d << shift) | carry;
            carry = d >> (DIGIT_BITS - shift);
            i += 1;
        }

        if i < z.len() {
            z[i] = carry;
            i += 1;
        } else {
            debug_assert!(carry == 0);
        }
        &mut z[..i]
    }

    /// `JSBigInt::rightShift(span, span, unsigned)`: Z := X >> shift.
    pub fn right_shift<'a>(z: &'a mut [Digit], x: &[Digit], shift: u32) -> &'a mut [Digit] {
        debug_assert!(shift < DIGIT_BITS);
        let x = normalize(x);
        if shift == 0 {
            return Self::span_copy(z, x);
        }

        if x.is_empty() {
            return &mut z[..0];
        }

        assert!(z.len() >= x.len());
        let mut carry: Digit = x[0] >> shift;
        let last = x.len() - 1;
        let mut i = 0usize;
        while i < last {
            let d = x[i + 1];
            z[i] = (d << (DIGIT_BITS - shift)) | carry;
            carry = d >> shift;
            i += 1;
        }
        z[i] = carry;
        &mut z[..x.len()]
    }

    /// `JSBigInt::divideSchoolbook`: computa Q(uociente) e R(esto) para A/B, tais que
    /// Q = (A - R) / B, com 0 <= R < B. Q e R são opcionais (vazios quando não interessam).
    /// Knuth, volume 2, seção 4.3.1, algoritmo D.
    pub fn divide_schoolbook<'q, 'r>(
        q: &'q mut [Digit],
        r: &'r mut [Digit],
        a: &[Digit],
        b: &[Digit],
        mut interrupt: Option<&mut InterruptCheck<'_>>,
    ) -> (&'q mut [Digit], &'r mut [Digit]) {
        assert!(b.len() >= 2); // Use divideSingle otherwise.
        assert!(a.len() >= b.len()); // No-op otherwise.
        // The quotient has a.size() - b.size() + 1 digits unless a's top b.size() digits are below
        // b, in which case the top one is zero and q may omit it; the loop below asserts that.
        assert!(q.is_empty() || q.len() >= a.len() - b.len());
        assert!(r.is_empty() || r.len() >= b.len());

        let n = b.len();
        let m = a.len() - n;

        // D1. Left-shift inputs so that the divisor's MSB is set.
        let shift = b[n - 1].leading_zeros();

        let mut normalized_divisor_storage: Vec<Digit> = vec![0; if shift > 0 { n } else { 0 }];
        if shift > 0 {
            let filled = Self::left_shift(&mut normalized_divisor_storage, b, shift).len();
            debug_assert!(filled == n);
            let _ = filled;
        }
        let normalized_divisor: &[Digit] = if shift > 0 { &normalized_divisor_storage } else { b };
        assert!(normalized_divisor.len() == b.len());

        // U holds the (continuously updated) remaining part of the dividend, which eventually
        // becomes the remainder.
        let mut u: Vec<Digit> = vec![0; a.len() + 1];
        {
            let filled = Self::left_shift(&mut u, a, shift).len();
            u[filled..].fill(0);
        }
        assert!(u.len() == a.len() + 1);

        // In each iteration, {qhatv} holds {divisor} * {current quotient digit}.
        let mut qhatv: Vec<Digit> = vec![0; n + 1];

        // D2. Iterate over the dividend's digits (like the "grad school" algorithm).
        let vn1 = normalized_divisor[n - 1];
        let vn2 = normalized_divisor[n - 2];
        let digit_div = DigitDiv::new(vn1);
        for j in (0..=m).rev() {
            // A long divisor makes each quotient digit a long row, so the termination check is per
            // row.
            if let Some(interrupt) = interrupt.as_mut() {
                interrupt.add_work(n);
                if interrupt.interrupted() {
                    break;
                }
            }
            // D3. Estimate the current iteration's quotient digit (see Knuth for details).
            let mut qhat: Digit = Digit::MAX;

            // {ujn} is the dividend's most significant remaining digit.
            let ujn = u[j + n];
            if ujn != vn1 {
                // {rhat} is the current iteration's remainder.
                let mut rhat: Digit = 0;
                // Estimate the current quotient digit by dividing the most significant digits of
                // dividend and divisor. The result will not be too small, but could be a bit too
                // large.
                qhat = digit_div.div(ujn, u[j + n - 1], &mut rhat);

                // Decrement the quotient estimate as needed by looking at the next digit, i.e. by
                // testing whether qhat * v_{n-2} > (rhat << digitBits) + u_{j+n-2}.
                let ujn2 = u[j + n - 2];
                while product_greater_than(qhat, vn2, rhat, ujn2) {
                    qhat = qhat.wrapping_sub(1);
                    let prev_rhat = rhat;
                    rhat = rhat.wrapping_add(vn1);
                    // v[n-1] >= 0, so this tests for overflow.
                    if rhat < prev_rhat {
                        break;
                    }
                }
            }

            // D4. Multiply the divisor with the current quotient digit, and subtract it from the
            // dividend. If there was "borrow", then the quotient digit was one too high, so we
            // must correct it and undo one subtraction of the (shifted) divisor.
            if qhat != 0 {
                let filled = Self::multiply_single(normalized_divisor, qhat, &mut qhatv).len();
                qhatv[filled..].fill(0);

                let mut c = Self::inplace_sub(&mut u[j..], &qhatv);
                if c != 0 {
                    c = Self::inplace_add(&mut u[j..], normalized_divisor);
                    u[j + n] = u[j + n].wrapping_add(c);
                    qhat = qhat.wrapping_sub(1);
                }
            }

            if !q.is_empty() {
                if j >= q.len() {
                    assert!(qhat == 0);
                } else {
                    q[j] = qhat;
                }
            }
        }

        // Determine the actual quotient length: it's m+1 if q[m] is non-zero, otherwise m.
        let q_length = (m + 1).min(q.len());
        let r_result = if r.is_empty() { r } else { Self::right_shift(r, &u, shift) };

        (&mut q[..q_length], r_result)
    }

    /// `JSBigInt::rightShiftZeroPadded`: Z := X >> shift, preenchendo Z com zeros.
    pub fn right_shift_zero_padded(z: &mut [Digit], x: &[Digit], shift: u32) {
        let shifted_len = Self::right_shift(&mut *z, x, shift).len();
        z[shifted_len..].fill(0);
    }

    /// `JSBigInt::divideBurnikelZiegler`: algoritmo 3 do artigo. Devolve Q e R para A/B (sem
    /// restrição de tamanho). R é opcional, Q não. Todo dígito de Q e de R é escrito.
    pub fn divide_burnikel_ziegler<'q, 'r>(
        interrupt: &mut InterruptCheck<'_>,
        q: &'q mut [Digit],
        r: &'r mut [Digit],
        a: &[Digit],
        b: &[Digit],
    ) -> (&'q mut [Digit], &'r mut [Digit]) {
        assert!(a.len() >= b.len());
        assert!(r.is_empty() || r.len() >= b.len());
        assert!(q.len() > a.len() - b.len());
        let quotient_length = a.len() - b.len() + 1;
        let mut a_length = a.len();
        let s = b.len();
        // The requirements are: n >= s, n as small as possible; m must be a power of two.
        // 1. Set m = min {2^k | 2^k * burnikelThreshold > s}.
        let m: usize = 1usize << (usize::BITS - (s / BURNIKEL_THRESHOLD).leading_zeros());
        // 2. Set j = roundup(s/m) and n = j * m.
        let j = (s + m - 1) / m;
        let n = j * m;
        // 3. Set sigma = max{tao | 2^tao * B < 2^(digitBits * n)}.
        let sigma = b[s - 1].leading_zeros();
        let digit_shift = n - s;
        // 4. Set B = B * 2^sigma to normalize B. Shift A by the same amount.
        let mut b_shifted: Vec<Digit> = vec![0; n];
        let shifted_divisor_len = Self::left_shift(&mut b_shifted[digit_shift..], b, sigma).len();
        debug_assert!(shifted_divisor_len == s);
        let _ = shifted_divisor_len;
        // We need an extra digit if A's top digit does not have enough space for the left-shift by
        // {sigma}. Additionally, the top bit of A must be 0 (see "-1" in step 5 below).
        let extra_digit = if a[a_length - 1].leading_zeros() < sigma + 1 { 1 } else { 0 };
        a_length = a.len() + digit_shift + extra_digit;
        let mut a_shifted: Vec<Digit> = vec![0; a_length];
        let shifted_dividend_len = Self::left_shift(&mut a_shifted[digit_shift..], a, sigma).len();
        // A shift of zero copies a's digits without the carry digit.
        a_shifted[digit_shift + shifted_dividend_len..].fill(0);
        // 5. Set t = min{t >= 2 | A < 2^(digitBits * t * n - 1)}.
        let t = ((a_length + n - 1) / n).max(2);
        // 6. Split A conceptually into t blocks.
        // 7. Set Z_(t-2) = [A_(t-1), A_(t-2)].
        let z_length = n * 2;
        let mut z: Vec<Digit> = vec![0; z_length];
        copy_zero_padded(&mut z, clamped_subspan(&a_shifted, n * (t - 2), z_length));
        // 8. For i from t-2 downto 0 do:
        let mut bz = BurnikelZiegler::new(interrupt, n);
        let mut ri: Vec<Digit> = vec![0; n];
        {
            // First iteration unrolled and specialized.
            // We might not have n digits at the top of Q, so use temporary storage for Qi...
            let mut qi: Vec<Digit> = vec![0; n];
            bz.d2n1n(&mut qi, &mut ri, &z, &b_shifted);
            if bz.interrupt.interrupted() {
                return (q, r);
            }
            // ...but there *will* be enough space for any non-zero result digits!
            let quotient_chunk = normalize(&qi[..]);
            let target = &mut q[n * (t - 2)..];
            debug_assert!(quotient_chunk.len() <= target.len());
            copy_zero_padded(target, quotient_chunk);
        }
        // Now loop over any remaining iterations.
        for i in (0..t - 2).rev() {
            // 8b. If i > 0, set Z_(i-1) = [Ri, A_(i-1)].
            // (De-duped with unrolled first iteration, hence reading A_(i).)
            copy_zero_padded(&mut z[n..], &ri);
            copy_zero_padded(&mut z[..n], clamped_subspan(&a_shifted, n * i, n));
            // 8a. Using algorithm D2n1n compute Qi, Ri such that Zi = B*Qi + Ri.
            bz.d2n1n(&mut q[i * n..(i + 1) * n], &mut ri, &z, &b_shifted);
            if bz.interrupt.interrupted() {
                return (q, r);
            }
        }
        // 9. Return Q = [Q_(t-2), ..., Q_0] and R = R_0 * 2^(-sigma).
        debug_assert!(ri[..digit_shift].iter().all(|&d| d == 0));
        if !r.is_empty() {
            let remainder = normalize(&ri[digit_shift..]);
            debug_assert!(remainder.len() <= r.len());
            Self::right_shift_zero_padded(r, remainder, sigma);
            return (&mut q[..quotient_length], &mut r[..s]);
        }
        (&mut q[..quotient_length], r)
    }

    /// `JSBigInt::invertBasecase`: Z := (a parte fracionária de) 1/V, por divisão ingênua.
    fn invert_basecase(interrupt: &mut InterruptCheck<'_>, z: &mut [Digit], v: &[Digit], scratch: &mut [Digit]) {
        debug_assert!(z.len() > v.len());
        debug_assert!(!v.is_empty());
        debug_assert!(scratch.len() >= 2 * v.len());
        let n = v.len();
        let x = &mut scratch[..2 * n];
        let mut borrow: Digit = 0;
        let mut i = 0usize;
        while i < n {
            x[i] = 0;
            i += 1;
        }
        while i < 2 * n {
            let mut new_borrow: Digit = 0;
            x[i] = digit_sub2(0, v[i - n], borrow, &mut new_borrow);
            borrow = new_borrow;
            i += 1;
        }
        debug_assert!(borrow == 1);
        // We don't need the remainder.
        let quotient_len = if n < BURNIKEL_THRESHOLD {
            Self::divide_schoolbook(&mut *z, &mut [], x, v, Some(&mut *interrupt)).0.len()
        } else {
            Self::divide_burnikel_ziegler(interrupt, &mut *z, &mut [], x, v).0.len()
        };
        z[quotient_len..].fill(0);
    }

    /// `JSBigInt::invertNewton`: algoritmo 4.2 do artigo. Computa o inverso de V, deslocado por
    /// `digitBits * 2 * V.size()`, preciso até V.size()+1 dígitos. Os V.size() dígitos baixos vão
    /// para Z, mais um dígito alto implícito de valor 1. Precisa de `invertNewtonScratchSpace`.
    fn invert_newton(interrupt: &mut InterruptCheck<'_>, z: &mut [Digit], v: &[Digit], scratch: &mut [Digit]) {
        let vn = v.len();
        debug_assert!(z.len() >= vn);
        debug_assert!(scratch.len() >= invert_newton_scratch_space(vn));
        let s_offset = 0usize;
        let w_offset = 0usize; // S and W can share their scratch space.
        let u_offset = vn + INVERT_NEWTON_EXTRA_SPACE;
        debug_assert!(s_offset == 0 && w_offset == 0);

        // The base case won't work otherwise.
        debug_assert!(v.len() >= 3);

        let basecase_precision = (NEWTON_INVERSION_THRESHOLD - 1).min((vn + 1) / 2);
        // V must have more digits than the basecase.
        debug_assert!(v.len() > basecase_precision);
        debug_assert!(v[vn - 1] >> (DIGIT_BITS - 1) != 0);

        // Step (1): Setup.
        // Calculate precision required at each step.
        // {k} is the number of fraction bits for the current iteration.
        let digit_bits = DIGIT_BITS as usize;
        let mut k = vn * digit_bits;
        let mut target_fraction_bits = [0usize; usize::BITS as usize];
        let mut iterations = 0usize; // "i" in the paper, except inverted to run downwards.
        while k > basecase_precision * digit_bits {
            target_fraction_bits[iterations] = k;
            iterations += 1;
            k = (k + 1) / 2;
        }
        // At this point, k <= basecasePrecision * digitBits is the number of fraction bits to use in
        // the base case. {iterations} is one past the highest index in use for targetFractionBits.

        // Step (2): Initial approximation.
        let initial_digits = (k + 1 + digit_bits - 1) / digit_bits;
        let top_part_of_v = &v[vn - initial_digits..];
        Self::invert_basecase(interrupt, z, top_part_of_v, scratch);
        z[initial_digits] = z[initial_digits].wrapping_add(1); // Implicit top digit.
        // From now on, we'll keep zLength updated to the part that's already computed.
        let mut z_length = initial_digits + 1;

        // Step (3): Precision doubling loop.
        loop {
            assert_integer_part_range(&z[..z_length], 1, 2);

            let (low, high) = scratch.split_at_mut(u_offset);

            // (3b): S = Z^2
            let s = &mut low[s_offset..s_offset + 2 * z_length];
            Self::multiply_zero_padded(interrupt, s, &z[..z_length], &z[..z_length]);
            if interrupt.interrupted() {
                return;
            }
            let s_length = 2 * z_length - 1; // Top digit of S is unused.
            debug_assert!(low[s_length] == 0);
            let s = &low[s_offset..s_offset + s_length];
            assert_integer_part_range(s, 1, 4);

            // (3c): T = V, truncated so that at least 2k+3 fraction bits remain.
            let fraction_digits = (2 * k + 3 + digit_bits - 1) / digit_bits;
            let t_length = vn.min(fraction_digits);
            let t = &v[vn - t_length..];

            // (3d): U = T * S, truncated so that at least 2k+1 fraction bits remain (U has one
            // integer digit, which might be zero).
            let fraction_digits = (2 * k + 1 + digit_bits - 1) / digit_bits;
            let u_full_length = s.len() + t.len();
            let u_full = &mut high[..u_full_length];
            debug_assert!(u_full.len() > fraction_digits);
            Self::multiply_zero_padded(interrupt, &mut *u_full, s, t);
            if interrupt.interrupted() {
                return;
            }
            let u = &u_full[u_full_length - (1 + fraction_digits)..];
            assert_integer_part_range(u, 0, 3);

            // (3e): W = 2 * Z, padded with "0" fraction bits so that it has the same number of
            // fraction bits as U.
            debug_assert!(u.len() >= z_length);
            let w = &mut low[w_offset..w_offset + u.len()];
            let padding_digits = u.len() - z_length;
            w[..padding_digits].fill(0);
            let doubled = Self::left_shift(&mut w[padding_digits..], &z[..z_length], 1).len();
            debug_assert!(doubled == z_length);
            let _ = doubled;
            let w: &[Digit] = w;
            assert_integer_part_range(w, 2, 4);

            // (3f): Z = W - U.
            // This check is '<=' instead of '<' because U's top digit is its integer part, and we
            // want vn fraction digits.
            if u.len() <= vn {
                // Normal subtraction.
                // This is not the last iteration.
                debug_assert!(iterations > 1);
                z_length = u.len();
                let borrow = Self::subtract_and_return_borrow(&mut z[..z_length], w, u);
                debug_assert!(borrow == 0);
                let _ = borrow;
                assert_integer_part_range(&z[..z_length], 1, 2);
            } else {
                // Truncate some least significant digits so that we get vn fraction digits, and
                // compute the integer digit separately.
                // This is the last iteration.
                debug_assert!(iterations == 1);
                z_length = vn;
                let w_part = &w[w.len() - vn - 1..w.len() - 1];
                let u_part = &u[u.len() - vn - 1..u.len() - 1];
                let borrow = Self::subtract_and_return_borrow(&mut z[..vn], w_part, u_part);
                let integer_part = w[w.len() - 1].wrapping_sub(u[u.len() - 1]).wrapping_sub(borrow);
                debug_assert!(integer_part == 1 || integer_part == 2);
                if integer_part == 2 {
                    // This is the rare case where the correct result would be 2.0, but since we
                    // can't express that by returning only the fractional part with an implicit
                    // 1-digit, we have to return [1.]9999... instead.
                    z[..vn].fill(!0);
                }
                break;
            }
            // (3g, 3h): Update local variables and loop.
            iterations -= 1;
            k = target_fraction_bits[iterations];
        }
        let _ = z_length;
    }

    /// `JSBigInt::invert`: computa o inverso de V, deslocado por `digitBits * 2 * V.size()`, preciso
    /// até V.size()+1 dígitos. Os V.size() dígitos baixos vão para Z, mais um dígito alto implícito
    /// de valor 1. (Caso de canto: se V é mínimo o dígito implícito deveria ser 2; devolvemos um a
    /// menos, e `divideBarrett` lida com isso.) Precisa de `invertScratchSpace(V.size())`.
    pub fn invert(interrupt: &mut InterruptCheck<'_>, z: &mut [Digit], v: &[Digit], scratch: &mut [Digit]) {
        debug_assert!(z.len() > v.len());
        debug_assert!(!v.is_empty());
        debug_assert!(v[v.len() - 1] >> (DIGIT_BITS - 1) != 0);
        debug_assert!(scratch.len() >= invert_scratch_space(v.len()));

        let vn = v.len();
        if vn >= NEWTON_INVERSION_THRESHOLD {
            return Self::invert_newton(interrupt, z, v, scratch);
        }
        if vn == 1 {
            let d = v[0];
            let mut dummy_remainder: Digit = 0;
            z[0] = digit_div(!d, !0, d, &mut dummy_remainder);
            z[1] = 0;
        } else {
            Self::invert_basecase(interrupt, z, v, scratch);
            if z[vn] == 1 {
                z[..vn].fill(!0);
                z[vn] = 0;
            }
        }
    }

    /// `JSBigInt::divideBarrett` (sete argumentos): algoritmo 3.5 do artigo. Computa Q(uociente) e
    /// R(esto) para A/B usando I, uma aproximação pré-computada de 1/B (por exemplo com `invert`).
    /// Precisa de `divideBarrettScratchSpace(A.size())` de scratch.
    pub fn divide_barrett(
        interrupt: &mut InterruptCheck<'_>,
        q: &mut [Digit],
        r: &mut [Digit],
        a: &[Digit],
        b: &[Digit],
        inverse: &[Digit],
        scratch: &mut [Digit],
    ) {
        debug_assert!(q.len() > a.len() - b.len());
        debug_assert!(r.len() >= b.len());
        debug_assert!(a.len() > b.len()); // Careful: This is *not* '>='!
        debug_assert!(a.len() <= 2 * b.len());
        debug_assert!(!b.is_empty());
        debug_assert!(b[b.len() - 1] >> (DIGIT_BITS - 1) != 0);
        debug_assert!(inverse.len() == a.len() - b.len());
        debug_assert!(scratch.len() >= divide_barrett_scratch_space(a.len()));

        let n = b.len();

        // (1): A1 = A with B.size() fewer digits.
        let a1 = &a[n..];
        debug_assert!(a1.len() == inverse.len());

        // (2): Q = A1*I with I.size() fewer digits.
        // {inverse} has an implicit high digit with value 1, so we add {A1} to the high part of the
        // multiplication result.
        let (q, full_quotient_rest) = q.split_at_mut(inverse.len() + 1);
        {
            let k = &mut scratch[..2 * inverse.len()];
            Self::multiply_zero_padded(interrupt, &mut *k, a1, inverse);
            if interrupt.interrupted() {
                return;
            }
            add_zero_padded(&mut *q, Operand::slice(&k[inverse.len()..]), Operand::slice(a1));
        }
        // K is no longer used, can reuse {scratch} for P.

        // (3): R = A - B*Q (approximate remainder).
        let p = &mut scratch[..a.len() + 1];
        Self::multiply_zero_padded(interrupt, &mut *p, b, q);
        if interrupt.interrupted() {
            return;
        }
        let borrow = {
            let remainder = &mut r[..n];
            Self::subtract_and_return_borrow(remainder, a, &p[..n])
        };
        // R may be allocated wider than B, zero out any extra digits if so.
        r[n..].fill(0);
        let mut r_high: Digit = a[n].wrapping_sub(p[n]).wrapping_sub(borrow);

        // Adjust R and Q so that they become the correct remainder and quotient.
        // The number of iterations is guaranteed to be at most some very small constant, unless the
        // caller gave us a bad approximate quotient.
        let remainder = &mut r[..n];
        if (r_high >> (DIGIT_BITS - 1)) != 0 {
            // (5b): R < 0, so R += B
            let mut q_sub: Digit = 0;
            loop {
                r_high = r_high.wrapping_add(Self::inplace_add(remainder, b));
                q_sub += 1;
                debug_assert!(q_sub <= 5);
                if r_high == 0 {
                    break;
                }
            }
            subtract_digit(q, q_sub);
        } else {
            let mut q_add: Digit = 0;
            while r_high != 0 || greater_than_or_equal(remainder, b) {
                // (5c): R >= B, so R -= B
                r_high = r_high.wrapping_sub(Self::inplace_sub(remainder, b));
                q_add += 1;
                debug_assert!(q_add <= 5);
            }
            add_digit(q, q_add);
        }
        // (5a): Return.
        full_quotient_rest.fill(0);
    }
}
