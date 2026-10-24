// Nona fatia do porte de `JSBigInt.cpp`: as linhas 3757 a 4135 (`divideBarrett` de cinco argumentos,
// `quotientLength`, `divideDigitsInto`, `estimateQhat`, `divideSameSize`, `remainderSameSize`,
// `divideImpl` e `divide`), 4185 a 4290 (`addDigits`, `divideDigits`, `oneShiftedLeft`, `sqrt`),
// 4290 a 4338 (`cbrt`), 4804 a 4915 (`remainderImpl`, `remainder`), 4916 a 5357 (`absoluteAddOne`,
// `absoluteSubOne`, `inc`, `dec`, `add`, `sub`, o `&`, `|` e `^`, os deslocamentos e `bitwiseNot`),
// 5549 a 5667 (`equals`, `absoluteCompare`, `compare`), 5668 a 6126 (`addSchoolbook`,
// `absoluteAdd`, `absoluteSub`, as operações absolutas bit a bit, `leftShiftByAbsolute`,
// `rightShiftByAbsolute`, `rightShiftByMaximum`), 6653 (a mensagem do `toNumber`), 7215 a 7778
// (`equalsToNumber`, `equalsToInt32`, `compareToDouble`, `toShiftAmount`, `decideRounding`,
// `toNumberHeap`, `asIntN`, `asUintN`, `toBigUInt64Heap`).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Desvios mecânicos de Rust seguro, sem efeito observável:
// - Sem `USE(BIGINT32)` (`PlatformUse.h:141`) as sobrecargas com `int32_t` e o `BigInt32` não
//   existem; `Int32BigIntImpl` continua servindo de operando genérico onde o C++ o instancia.
// - Sem sobrecarga em Rust, os nomes que o C++ repete ganham sufixo: `divide_barrett_digits`
//   (cinco argumentos), `absolute_add_one_digits`/`absolute_sub_one_digits` (as de `span`).
// - `addSchoolbookFixed`, `subSchoolbookFixed`, `addDigitsInto` e `subDigitsInto` só despacham para
//   versões de tamanho fixo que fazem a mesma conta; `absoluteAdd` e `absoluteSub` chamam direto
//   `add_schoolbook` e `sub_schoolbook`.
// - O cache de divisor de `remainderImpl` (`m_cachedBigIntDivisor`, `cachedMod*`, 4340 a 4800) é só
//   uma otimização do VM: o resto que ele produz é o mesmo do caminho geral, que é o único aqui.
// - `tryAllocateCell` e o `setLength` de encurtar a célula viram `create_with_length` e
//   `set_length`.
// - `hash`/`hashSlow` (7780 a 7792) e o `toObject` (precisa do `BigIntObject`, em `bigint_object.rs`)
//   ficam fora desta fatia.

/// `andLength`
fn and_length(x: &[Digit], y: &[Digit]) -> usize {
    x.len().min(y.len())
}

/// `orLength` e `xorLength` (os dois são o maior dos comprimentos).
fn or_length(x: &[Digit], y: &[Digit]) -> usize {
    x.len().max(y.len())
}

/// `addOneLength`
fn add_one_length(x: &[Digit]) -> usize {
    x.len() + 1
}

/// `subOneLength`
fn sub_one_length(x: &[Digit]) -> usize {
    x.len()
}

/// Aloca `length` dígitos, deixa `operation` escrevê-los e devolve só os dígitos normalizados. É o
/// `Vector<Digit, 16> resultVector(length); auto result = normalize(op(..., resultVector))` que o
/// C++ repete em cada operação bit a bit.
fn normalized_digits(length: usize, operation: impl FnOnce(&mut [Digit]) -> &mut [Digit]) -> Vec<Digit> {
    let mut storage = vec![0; length];
    let normalized_length = normalize(&*operation(&mut storage)).len();
    storage.truncate(normalized_length);
    storage
}

/// `shouldUseSchoolbookDivision`
fn should_use_schoolbook_division(dividend_size: usize, divisor_size: usize) -> bool {
    divisor_size < BURNIKEL_THRESHOLD || dividend_size - divisor_size < BURNIKEL_THRESHOLD
}

/// `estimateQhat`
fn estimate_qhat(a: &[Digit], b: &[Digit]) -> Digit {
    debug_assert!(a.len() == b.len());
    let n = a.len();
    debug_assert!(n > 1); // one digit case is already handled via divideSingle.
    debug_assert!(b[n - 1] != 0); // b should be normalized
    debug_assert!(a[n - 1] > b[n - 1]);

    // a.back() > b.back(), so a > b and quotient is at least 1. Since a and b have the same number
    // of digits, the quotient fits in one digit. Only the top 2-3 digits are normalized (no vector
    // allocation); the caller verifies qhat by computing qhat * b and comparing with a.
    let shift = b[n - 1].leading_zeros();

    let (vn1, vn2, un, un1, un2): (Digit, Digit, Digit, Digit, Digit);
    if shift == 0 {
        vn1 = b[n - 1];
        vn2 = b[n - 2];
        un = 0;
        un1 = a[n - 1];
        un2 = a[n - 2];
    } else {
        // Left-shift by 'shift' bits to normalize
        vn1 = (b[n - 1] << shift) | (b[n - 2] >> (DIGIT_BITS - shift));
        vn2 = (b[n - 2] << shift) | (if n >= 3 { b[n - 3] >> (DIGIT_BITS - shift) } else { 0 });
        un = a[n - 1] >> (DIGIT_BITS - shift);
        un1 = (a[n - 1] << shift) | (a[n - 2] >> (DIGIT_BITS - shift));
        un2 = (a[n - 2] << shift) | (if n >= 3 { a[n - 3] >> (DIGIT_BITS - shift) } else { 0 });
    }

    // Since a and b have the same number of digits with a.back() > b.back(), after normalization
    // un < vn1 is guaranteed.
    debug_assert!(un < vn1);

    let mut rhat: Digit = 0;
    let mut qhat = digit_div(un, un1, vn1, &mut rhat);

    // Refine qhat using the second most significant digit of divisor.
    while product_greater_than(qhat, vn2, rhat, un2) {
        qhat -= 1;
        let prev_rhat = rhat;
        rhat = rhat.wrapping_add(vn1);
        if rhat < prev_rhat {
            break;
        }
    }

    qhat
}

/// `ImplResult { x }` seguido de `tryConvertToBigInt32`: o resultado público das operações.
fn public_result(result: Result<ImplResult, BigIntError>) -> Result<ImplResult, BigIntError> {
    result.map(try_convert_to_big_int32)
}

/// `absoluteCompare`
pub fn absolute_compare<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> ComparisonResult {
    debug_assert!(x.length() == 0 || x.digit(x.length() - 1) != 0);
    debug_assert!(y.length() == 0 || y.digit(y.length() - 1) != 0);

    if x.length() != y.length() {
        return if x.length() < y.length() { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    let mut i = x.length() as i64 - 1;
    while i >= 0 && x.digit(i as u32) == y.digit(i as u32) {
        i -= 1;
    }

    if i < 0 {
        return ComparisonResult::Equal;
    }

    if x.digit(i as u32) > y.digit(i as u32) {
        ComparisonResult::GreaterThan
    } else {
        ComparisonResult::LessThan
    }
}

/// `compareImpl`
pub fn compare_impl<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> ComparisonResult {
    let x_sign = x.sign();

    if x_sign != y.sign() {
        return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    match absolute_compare(x, y) {
        ComparisonResult::GreaterThan => {
            if x_sign {
                ComparisonResult::LessThan
            } else {
                ComparisonResult::GreaterThan
            }
        }
        ComparisonResult::LessThan => {
            if x_sign {
                ComparisonResult::GreaterThan
            } else {
                ComparisonResult::LessThan
            }
        }
        _ => ComparisonResult::Equal,
    }
}

/// `JSBigInt::compare(JSBigInt*, JSBigInt*)`
pub fn compare(x: &JSBigInt, y: &JSBigInt) -> ComparisonResult {
    compare_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y))
}

/// `JSBigInt::compare(JSBigInt*, int64_t)`
pub fn compare_i64(x: &JSBigInt, y: i64) -> ComparisonResult {
    compare_impl(&HeapBigIntImpl::new(x), &Int64BigIntImpl::from_i64(y))
}

/// `JSBigInt::compare(JSBigInt*, uint64_t)`
pub fn compare_u64(x: &JSBigInt, y: u64) -> ComparisonResult {
    compare_impl(&HeapBigIntImpl::new(x), &Int64BigIntImpl::from_u64(y))
}

/// `JSBigInt::toShiftAmount`
fn to_shift_amount<B: BigIntImpl>(x: &B) -> Option<Digit> {
    if x.length() > 1 {
        return None;
    }

    let value = x.digit(0);
    const _: () = assert!((MAX_LENGTH_BITS as u64) < Digit::MAX);

    if value > MAX_LENGTH_BITS as Digit {
        return None;
    }

    Some(value)
}

/// `JSBigInt::compareToDouble(BigIntImpl, double)`
pub fn compare_to_double<B: BigIntImpl>(x: &B, y: f64) -> ComparisonResult {
    // This algorithm expect that the double format is IEEE 754

    let double_bits = y.to_bits();
    let raw_exponent = ((double_bits >> 52) & 0x7FF) as i32;

    // Handle finite doubles for {y}.
    if raw_exponent == 0x7FF {
        if y.is_nan() {
            return ComparisonResult::Undefined;
        }

        return if y == f64::INFINITY { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    let x_sign = x.sign();

    // Note that this is different from the double's sign bit for -0. That's
    // intentional because -0 must be treated like 0.
    let y_sign = y < 0.0;
    if x_sign != y_sign {
        return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    if y == 0.0 {
        // If {y} is zero, then ySign is false and xSign must be false.
        debug_assert!(!x_sign);
        return if x.is_zero() { ComparisonResult::Equal } else { ComparisonResult::GreaterThan };
    }

    if x.is_zero() {
        // If {x} is zero, then xSign is false and ySign must be false which indicates that {y} is greater than zero.
        debug_assert!(!y_sign && y > 0.0);
        return ComparisonResult::LessThan;
    }

    // Right now, only two cases left:
    //     {x} >= 1 and {y} > 0
    //     {x} <= -1 and {y} < 0

    // Non-finite doubles are handled above.
    debug_assert!(raw_exponent != 0x7FF);
    let exponent = raw_exponent - 0x3FF;
    if exponent < 0 {
        // The absolute value of the double is less than 1. Only 0n has an
        // absolute value smaller than that, but we've already covered that case.
        // Note that this also handles denormal doubles for {y}.
        return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    let x_length = x.length() as i32;
    let x_msd = x.digit((x_length - 1) as u32);
    let msd_leading_zeros = x_msd.leading_zeros() as i32;

    let x_bit_length = x_length * DIGIT_BITS as i32 - msd_leading_zeros;
    let y_bit_length = exponent + 1;
    if x_bit_length < y_bit_length {
        return if x_sign { ComparisonResult::GreaterThan } else { ComparisonResult::LessThan };
    }

    if x_bit_length > y_bit_length {
        return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    // At this point, we know that signs and bit lengths (i.e. position of
    // the most significant bit in exponent-free representation) are identical.
    // {x} is not zero, {y} is finite and not denormal.
    // Now we virtually convert the double to an integer by shifting its
    // mantissa according to its exponent, so it will align with the BigInt {x},
    // and then we compare them bit for bit until we find a difference or the
    // least significant bit.
    let mut mantissa: u64 = double_bits & 0x000F_FFFF_FFFF_FFFF;
    mantissa |= 0x0010_0000_0000_0000;
    const MANTISSA_TOP_BIT: i32 = 52; // 0-indexed.

    // 0-indexed position of {x}'s most significant bit within the {msd}.
    let msd_top_bit = DIGIT_BITS as i32 - 1 - msd_leading_zeros;
    debug_assert!(msd_top_bit == (x_bit_length - 1) % DIGIT_BITS as i32);

    // Shifted chunk of {mantissa} for comparing with {digit}.
    let mut compare_mantissa: Digit;

    // Number of unprocessed bits in {mantissa}. We'll keep them shifted to
    // the left (i.e. most significant part) of the underlying uint64_t.
    let mut remaining_mantissa_bits: i32 = 0;

    // First, compare the most significant digit against the beginning of
    // the mantissa and then we align them.
    if msd_top_bit < MANTISSA_TOP_BIT {
        remaining_mantissa_bits = MANTISSA_TOP_BIT - msd_top_bit;
        compare_mantissa = (mantissa >> remaining_mantissa_bits) as Digit;
        mantissa <<= 64 - remaining_mantissa_bits;
    } else {
        compare_mantissa = (mantissa << (msd_top_bit - MANTISSA_TOP_BIT)) as Digit;
        mantissa = 0;
    }

    if x_msd > compare_mantissa {
        return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
    }

    if x_msd < compare_mantissa {
        return if x_sign { ComparisonResult::GreaterThan } else { ComparisonResult::LessThan };
    }

    // Then, compare additional digits against any remaining mantissa bits.
    let mut digit_index = x_length - 2;
    while digit_index >= 0 {
        if remaining_mantissa_bits > 0 {
            remaining_mantissa_bits -= DIGIT_BITS as i32;
            // `sizeof(mantissa) == sizeof(xMSD)` em `CPU(REGISTER64)`.
            compare_mantissa = mantissa as Digit;
            mantissa = 0;
        } else {
            compare_mantissa = 0;
        }

        let digit = x.digit(digit_index as u32);
        if digit > compare_mantissa {
            return if x_sign { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
        }
        if digit < compare_mantissa {
            return if x_sign { ComparisonResult::GreaterThan } else { ComparisonResult::LessThan };
        }
        digit_index -= 1;
    }

    // Integer parts are equal; check whether {y} has a fractional part.
    if mantissa != 0 {
        debug_assert!(remaining_mantissa_bits > 0);
        return if x_sign { ComparisonResult::GreaterThan } else { ComparisonResult::LessThan };
    }

    ComparisonResult::Equal
}

/// `JSBigInt::compareToDouble(double, BigIntImpl)` (`JSBigIntInlines.h`): `flip(compareToDouble(y, x))`.
pub fn compare_double_to_big_int<B: BigIntImpl>(x: f64, y: &B) -> ComparisonResult {
    flip(compare_to_double(y, x))
}

impl JSBigInt {
    /// `JSBigInt::equals`
    pub fn equals(x: &JSBigInt, y: &JSBigInt) -> bool {
        x.sign() == y.sign() && x.digits() == y.digits()
    }

    /// `JSBigInt::equalsToInt32`
    pub fn equals_to_int32(&self, value: i32) -> bool {
        if value == 0 {
            return self.is_zero();
        }
        self.length() == 1 && self.sign() == (value < 0) && self.digit(0) == (value as i64).unsigned_abs()
    }

    /// `JSBigInt::equalsToNumber`: `num_value` é um número (`isNumber()`).
    pub fn equals_to_number(&self, num_value: crate::runtime::js_value::JSValue) -> bool {
        debug_assert!(num_value.is_number());

        if num_value.is_int32() {
            return self.equals_to_int32(num_value.as_int32());
        }

        compare_to_double(&HeapBigIntImpl::new(self), num_value.as_double()) == ComparisonResult::Equal
    }

    /// `JSBigInt::toNumber(JSGlobalObject*)`: o `TypeError` que o C++ lança (a conversão implícita
    /// de BigInt para número não é permitida).
    pub const TO_NUMBER_ERROR_MESSAGE: &'static str = "Conversion from 'BigInt' to 'number' is not allowed.";

    /// `JSBigInt::divideBarrett` de cinco argumentos: computa Q(uociente) e R(esto) para A/B com a
    /// divisão de Barrett. Todo dígito de Q e de R é escrito.
    pub fn divide_barrett_digits<'q, 'r>(
        interrupt: &mut InterruptCheck<'_>,
        q: &'q mut [Digit],
        r: &'r mut [Digit],
        a: &[Digit],
        b: &[Digit],
    ) -> (&'q mut [Digit], &'r mut [Digit]) {
        assert!(q.len() > a.len() - b.len() + 1);
        assert!(r.len() >= b.len());
        assert!(a.len() > b.len()); // Careful: This is *not* '>=' !
        assert!(!b.is_empty());
        let quotient_length = a.len() - b.len() + 1;
        let remainder_length = b.len();

        // Normalize B, and shift A by the same amount.
        let shift = b[b.len() - 1].leading_zeros();
        let mut b_normalized_storage: Vec<Digit> = Vec::new();
        let mut a_normalized_storage: Vec<Digit> = Vec::new();
        if shift != 0 {
            b_normalized_storage.resize(b.len(), 0);
            let shifted = Self::left_shift(&mut b_normalized_storage, b, shift).len();
            debug_assert!(shifted == b.len());
            // A gains a digit if its top digit has no room for the shift.
            a_normalized_storage.resize(a.len() + usize::from(a[a.len() - 1].leading_zeros() < shift), 0);
            let shifted_dividend = Self::left_shift(&mut a_normalized_storage, a, shift).len();
            debug_assert!(shifted_dividend == a_normalized_storage.len());
        }
        let b: &[Digit] = if shift != 0 { &b_normalized_storage } else { b };
        let a: &[Digit] = if shift != 0 { &a_normalized_storage } else { a };

        // The core divideBarrett function above only supports A having at most twice as many digits
        // as B. We generalize this to arbitrary inputs similar to Burnikel-Ziegler division by
        // performing a t-by-1 division of B-sized chunks. It's easy to special-case the situation
        // where we don't need to bother.
        let barrett_dividend_length = if a.len() <= 2 * b.len() { a.len() } else { 2 * b.len() };
        let inverse_length = barrett_dividend_length - b.len();
        // +1 is for temporary use by invert().
        let mut inverse_storage: Vec<Digit> = vec![0; inverse_length + 1];
        let scratch_length = invert_scratch_space(inverse_length).max(divide_barrett_scratch_space(barrett_dividend_length));
        let mut scratch: Vec<Digit> = vec![0; scratch_length];
        Self::invert(interrupt, &mut inverse_storage, &b[b.len() - inverse_length..], &mut scratch);
        if interrupt.interrupted() {
            return (q, r);
        }
        debug_assert!(inverse_storage[inverse_length] == 0);
        let inverse = &inverse_storage[..inverse_length];
        if a.len() > 2 * b.len() {
            // This follows the variable names and and algorithmic steps of divideBurnikelZiegler().
            let n = b.len(); // Chunk length.
            // (5): {t} is the number of B-sized chunks of A.
            let t = a.len().div_ceil(n);
            debug_assert!(t >= 3);
            // (6)/(7): Z is used for the current 2-chunk block to be divided by B, initialized to the
            // two topmost chunks of A.
            let z_length = n * 2;
            let mut z: Vec<Digit> = vec![0; z_length];
            copy_zero_padded(&mut z, clamped_subspan(a, n * (t - 2), z_length));
            // (8): For i from t-2 downto 0 do
            let qi_length = n + 1;
            let mut qi: Vec<Digit> = vec![0; qi_length];
            let mut ri: Vec<Digit> = vec![0; n];
            // First iteration unrolled and specialized.
            {
                let i = t - 2;
                Self::divide_barrett(interrupt, &mut qi, &mut ri, &z, b, inverse, &mut scratch);
                if interrupt.interrupted() {
                    return (q, r);
                }
                let target = &mut q[n * i..];
                // In the first iteration, all qiLength = n + 1 digits may be used.
                copy_zero_padded(target, &qi);
                debug_assert!(qi.iter().skip(target.len()).all(|digit| *digit == 0));
            }
            // Now loop over any remaining iterations.
            for i in (0..t - 2).rev() {
                // (8b): If i > 0, set Z_(i-1) = [Ri, A_(i-1)].
                // (De-duped with unrolled first iteration, hence reading A_(i).)
                copy_zero_padded(&mut z[n..], &ri);
                copy_zero_padded(&mut z[..n], clamped_subspan(a, n * i, n));
                // (8a): Compute Qi, Ri such that Zi = B*Qi + Ri.
                Self::divide_barrett(interrupt, &mut qi, &mut ri, &z, b, inverse, &mut scratch);
                if interrupt.interrupted() {
                    return (q, r);
                }
                debug_assert!(qi[qi_length - 1] == 0);
                // (9): Return Q = [Q_(t-2), ..., Q_0]...
                copy_zero_padded(&mut q[n * i..n * i + n], &qi);
            }
            let remainder = normalize(&ri[..]);
            debug_assert!(remainder.len() <= r.len());
            // (9): ...and R = R_0 * 2^(-leading_zeros).
            Self::right_shift_zero_padded(r, remainder, shift);
        } else {
            Self::divide_barrett(interrupt, q, r, a, b, inverse, &mut scratch);
            if interrupt.interrupted() {
                return (q, r);
            }
            let remainder = r.to_vec();
            Self::right_shift_zero_padded(r, &remainder, shift);
        }
        (&mut q[..quotient_length], &mut r[..remainder_length])
    }

    /// `JSBigInt::quotientLength`: o número de dígitos de quociente que quem chama `divideDigitsInto`
    /// precisa fornecer.
    pub fn quotient_length(a: &[Digit], b: &[Digit]) -> usize {
        debug_assert!(a.len() >= b.len());
        let mut length = a.len() - b.len() + 1;
        // Barrett division normalizes the dividend itself, which can grow it by a digit.
        if b.len() >= BARRETT_THRESHOLD {
            length += 1;
        }
        length
    }

    /// `JSBigInt::divideDigitsInto`: computa Q(uociente) e R(esto) para A/B com o algoritmo que serve
    /// ao tamanho dos operandos. Q ou R pode ser vazio; Q, quando presente, precisa ter
    /// `quotient_length(a, b)` dígitos. Os spans devolvidos não são normalizados.
    pub fn divide_digits_into<'q, 'r>(
        interrupt: &mut InterruptCheck<'_>,
        q: &'q mut [Digit],
        r: &'r mut [Digit],
        a: &[Digit],
        b: &[Digit],
    ) -> (&'q mut [Digit], &'r mut [Digit]) {
        debug_assert!(b.len() >= 2);
        debug_assert!(a.len() >= b.len());
        debug_assert!(q.is_empty() || q.len() >= Self::quotient_length(a, b));
        debug_assert!(r.is_empty() || r.len() >= b.len());
        if should_use_schoolbook_division(a.len(), b.len()) {
            return Self::divide_schoolbook(q, r, a, b, Some(interrupt));
        }
        if b.len() < BARRETT_THRESHOLD {
            if !q.is_empty() {
                return Self::divide_burnikel_ziegler(interrupt, q, r, a, b);
            }
            let mut quotient_storage: Vec<Digit> = vec![0; Self::quotient_length(a, b)];
            let (_, remainder) = Self::divide_burnikel_ziegler(interrupt, &mut quotient_storage, r, a, b);
            return (q, remainder);
        }
        if q.is_empty() {
            let mut quotient_storage: Vec<Digit> = vec![0; Self::quotient_length(a, b)];
            let (_, remainder) = Self::divide_barrett_digits(interrupt, &mut quotient_storage, r, a, b);
            return (q, remainder);
        }
        if r.is_empty() {
            let mut remainder_storage: Vec<Digit> = vec![0; b.len()];
            let (quotient, _) = Self::divide_barrett_digits(interrupt, q, &mut remainder_storage, a, b);
            return (quotient, r);
        }
        Self::divide_barrett_digits(interrupt, q, r, a, b)
    }

    /// `JSBigInt::divideSameSize`
    pub fn divide_same_size(a: &[Digit], b: &[Digit]) -> Digit {
        assert!(a.len() == b.len());
        let n = a.len();
        assert!(n >= 2); // Use divideSingle otherwise.
        debug_assert!(b[n - 1] != 0); // b should be normalized

        // absoluteCompare ensures that a > b.

        // MSB is the same. Thus result is 1 or 0.
        if a[n - 1] == b[n - 1] {
            return 1; // a >= b
        }

        let mut qhat = estimate_qhat(a, b);

        // Verify qhat by computing qhat * b and comparing with a inline.
        // If qhat * b > a, decrement qhat.
        let mut mul_carry: Digit = 0;
        let mut sub_borrow: Digit = 0;
        for i in 0..n {
            // Compute qhat * b[i] + mulCarry
            let (low, high) = digit_mul(qhat, b[i]);
            let product = low.wrapping_add(mul_carry);
            mul_carry = high + Digit::from(product < low);

            // Subtract product from a[i] to check if qhat * b > a
            let mut borrow_out: Digit = 0;
            digit_sub2(a[i], product, sub_borrow, &mut borrow_out);
            sub_borrow = borrow_out;
        }

        // If there's overflow from multiplication or borrow from subtraction,
        // qhat * b > a, so decrement qhat.
        if mul_carry != 0 || sub_borrow != 0 {
            qhat -= 1;
        }

        qhat
    }

    /// `JSBigInt::remainderSameSize`
    pub fn remainder_same_size<'a>(r: &'a mut [Digit], a: &[Digit], b: &[Digit]) -> &'a mut [Digit] {
        assert!(a.len() == b.len());
        let n = a.len();
        assert!(n >= 2); // Use divideSingle otherwise.
        debug_assert!(b[n - 1] != 0); // b should be normalized
        debug_assert!(r.len() >= n);

        // absoluteCompare ensures that a > b.

        // a.back() == b.back(): quotient is 0 or 1
        if a[n - 1] == b[n - 1] {
            return Self::sub_schoolbook(a, b, r);
        }

        let qhat = estimate_qhat(a, b);

        // Compute remainder = a - qhat * b inline without allocating a vector.
        // We compute qhat * b and subtract from a in a single pass.
        let mut mul_carry: Digit = 0;
        let mut sub_borrow: Digit = 0;
        for i in 0..n {
            // Compute qhat * b[i] + mulCarry
            let (low, high) = digit_mul(qhat, b[i]);
            let product = low.wrapping_add(mul_carry);
            mul_carry = high + Digit::from(product < low);

            // Compute r[i] = a[i] - product - subBorrow
            let mut borrow_out: Digit = 0;
            r[i] = digit_sub2(a[i], product, sub_borrow, &mut borrow_out);
            sub_borrow = borrow_out;
        }

        // If there's overflow from multiplication or borrow from subtraction,
        // qhat was too large, add back b.
        if mul_carry != 0 || sub_borrow != 0 {
            Self::inplace_add(&mut r[..n], b);
        }

        &mut r[..n]
    }

    /// `JSBigInt::divideImpl`
    pub fn divide_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        // 1. If y is 0n, throw a RangeError exception.
        if y.is_zero() {
            return Err(BigIntError::InvalidDivisor);
        }

        // 2. Let quotient be the mathematical value of x divided by y.
        // 3. Return a BigInt representing quotient rounded towards 0 to the next
        //    integral value.
        let result_sign = x.sign() != y.sign();
        match absolute_compare(x, y) {
            ComparisonResult::LessThan => return Ok(zero_impl()),
            ComparisonResult::Equal => {
                return JSBigInt::create_from_i32(if result_sign { -1 } else { 1 }).map(ImplResult::Heap);
            }
            ComparisonResult::GreaterThan | ComparisonResult::Undefined => {}
        }

        let x_span = x.digits();
        let y_span = y.digits();
        let q_length = x_span.len() - y_span.len() + 1;
        if y_span.len() == 1 {
            let divisor = y_span[0];
            if divisor == 1 {
                if result_sign == x.sign() {
                    return Ok(x.to_impl_result());
                }
                return JSBigInt::unary_minus_impl(x);
            }

            let mut q: Vec<Digit> = vec![0; q_length];
            let mut remainder: Digit = 0;
            let quotient = Self::divide_single(&mut q, &mut remainder, x_span, divisor);
            return JSBigInt::try_create_from_impl(result_sign, quotient).map(ImplResult::Heap);
        }

        if x_span.len() == y_span.len() {
            let quotient_digit = Self::divide_same_size(x_span, y_span);
            if quotient_digit == 0 {
                return Ok(zero_impl());
            }

            let mut quotient = JSBigInt::create_with_length(1)?;
            quotient.set_digit(0, quotient_digit);
            quotient.set_sign(result_sign);
            return Ok(ImplResult::Heap(quotient));
        }

        let mut q: Vec<Digit> = vec![0; Self::quotient_length(x_span, y_span)];
        let mut interrupt = InterruptCheck::new(None);
        let (q_span, _) = Self::divide_digits_into(&mut interrupt, &mut q, &mut [], x_span, y_span);
        if interrupt.interrupted() {
            return Ok(ImplResult::Empty);
        }
        JSBigInt::try_create_from_impl(result_sign, q_span).map(ImplResult::Heap)
    }

    /// `JSBigInt::divide(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn divide(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::divide_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::addDigits`
    pub fn add_digits<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        let mut x = normalize(x);
        let mut y = normalize(y);
        if x.len() < y.len() {
            core::mem::swap(&mut x, &mut y);
        }
        assert!(result.len() > x.len());
        let length = normalize(&*Self::add_schoolbook(x, y, result)).len();
        &mut result[..length]
    }

    /// `JSBigInt::divideDigits`
    pub fn divide_digits<'a>(
        interrupt: &mut InterruptCheck<'_>,
        quotient: &'a mut [Digit],
        x: &[Digit],
        y: &[Digit],
    ) -> &'a mut [Digit] {
        let x = normalize(x);
        let y = normalize(y);
        assert!(!y.is_empty());

        let comparison_result = Self::compare_digits(x, y);
        if comparison_result == ComparisonResult::LessThan {
            return &mut quotient[..0];
        }

        assert!(quotient.len() >= x.len());
        if comparison_result == ComparisonResult::Equal {
            quotient[0] = 1;
            return &mut quotient[..1];
        }

        // x > y, thus x.size() >= y.size().
        if y.len() == 1 {
            let mut remainder: Digit = 0;
            let length = normalize(&*Self::divide_single(quotient, &mut remainder, x, y[0])).len();
            return &mut quotient[..length];
        }

        if x.len() == y.len() {
            let quotient_digit = Self::divide_same_size(x, y);
            if quotient_digit == 0 {
                return &mut quotient[..0];
            }
            quotient[0] = quotient_digit;
            return &mut quotient[..1];
        }

        let length = {
            let (quotient_span, _) = Self::divide_digits_into(interrupt, &mut *quotient, &mut [], x, y);
            normalize(&*quotient_span).len()
        };
        &mut quotient[..length]
    }

    /// `JSBigInt::oneShiftedLeft`
    pub fn one_shifted_left(result: &mut [Digit], bit_index: u32) -> &mut [Digit] {
        let digit_index = (bit_index / DIGIT_BITS) as usize;
        assert!(result.len() > digit_index);
        let result = &mut result[..digit_index + 1];
        result.fill(0);
        result[digit_index] = 1 << (bit_index % DIGIT_BITS);
        result
    }

    /// `JSBigInt::sqrt`: https://tc39.es/proposal-bigint-math/#sec-bigint.sqrt
    pub fn sqrt(big_int: &JSBigInt) -> Result<ImplResult, BigIntError> {
        debug_assert!(!big_int.sign());

        if big_int.is_zero() {
            return Ok(ImplResult::Heap(big_int.clone()));
        }

        let value = big_int.digits();
        let mut result_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut quotient_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut sum_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut next_storage: Vec<Digit> = vec![0; value.len() + 2];

        // 2^floor(floor(log2(value)) / 2)
        let mut result_length = Self::one_shifted_left(&mut result_storage, (big_int.bit_length() - 1) >> 1).len();
        let mut interrupt = InterruptCheck::new(None);
        let mut iteration = 0;
        loop {
            // result = ((value / result) + result) >> 1
            let quotient_length =
                Self::divide_digits(&mut interrupt, &mut quotient_storage, value, &result_storage[..result_length]).len();
            if interrupt.interrupted() {
                return Ok(ImplResult::Empty);
            }
            let sum_length =
                Self::add_digits(&quotient_storage[..quotient_length], &result_storage[..result_length], &mut sum_storage).len();
            let next_length = normalize(&*Self::right_shift(&mut next_storage, &sum_storage[..sum_length], 1)).len();
            if iteration != 0 {
                let comparison_result = Self::compare_digits(&next_storage[..next_length], &result_storage[..result_length]);
                if comparison_result == ComparisonResult::Equal || comparison_result == ComparisonResult::GreaterThan {
                    break;
                }
            }

            result_storage[..next_length].copy_from_slice(&next_storage[..next_length]);
            result_length = next_length;
            iteration += 1;
        }

        JSBigInt::try_create_from_impl(false, &result_storage[..result_length]).map(ImplResult::Heap)
    }

    /// `JSBigInt::cbrt`: https://tc39.es/proposal-bigint-math/#sec-bigint.cbrt
    pub fn cbrt(big_int: &JSBigInt) -> Result<ImplResult, BigIntError> {
        if big_int.is_zero() {
            return Ok(ImplResult::Heap(big_int.clone()));
        }

        const THREE: [Digit; 1] = [3];

        let sign = big_int.sign();
        let value = big_int.digits();
        let mut result_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut squared_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut quotient_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut doubled_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut sum_storage: Vec<Digit> = vec![0; value.len() + 2];
        let mut next_storage: Vec<Digit> = vec![0; value.len() + 2];

        // 2^floor(floor(log2(value)) / 3)
        let mut result_length = Self::one_shifted_left(&mut result_storage, (big_int.bit_length() - 1) / 3).len();
        let mut interrupt = InterruptCheck::new(None);
        let mut iteration = 0;
        loop {
            // result = ((2 * result) + (value / (result * result))) / 3
            let squared_length = Self::multiply_digits(
                &mut interrupt,
                &result_storage[..result_length],
                &result_storage[..result_length],
                &mut squared_storage,
            )
            .len();
            if interrupt.interrupted() {
                return Ok(ImplResult::Empty);
            }
            let quotient_length =
                Self::divide_digits(&mut interrupt, &mut quotient_storage, value, &squared_storage[..squared_length]).len();
            if interrupt.interrupted() {
                return Ok(ImplResult::Empty);
            }
            let doubled_length =
                normalize(&*Self::left_shift(&mut doubled_storage, &result_storage[..result_length], 1)).len();
            let sum_length =
                Self::add_digits(&doubled_storage[..doubled_length], &quotient_storage[..quotient_length], &mut sum_storage)
                    .len();
            let next_length = Self::divide_digits(&mut interrupt, &mut next_storage, &sum_storage[..sum_length], &THREE).len();
            if iteration != 0 {
                let comparison_result = Self::compare_digits(&next_storage[..next_length], &result_storage[..result_length]);
                if comparison_result == ComparisonResult::Equal || comparison_result == ComparisonResult::GreaterThan {
                    break;
                }
            }

            result_storage[..next_length].copy_from_slice(&next_storage[..next_length]);
            result_length = next_length;
            iteration += 1;
        }

        JSBigInt::try_create_from_impl(sign, &result_storage[..result_length]).map(ImplResult::Heap)
    }

    /// `JSBigInt::remainderImpl`
    pub fn remainder_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        // 1. If y is 0n, throw a RangeError exception.
        if y.is_zero() {
            return Err(BigIntError::InvalidDivisor);
        }

        // 2. Return the JSBigInt representing x modulo y.
        // See https://github.com/tc39/proposal-bigint/issues/84 though.
        match absolute_compare(x, y) {
            ComparisonResult::LessThan => return Ok(x.to_impl_result()),
            ComparisonResult::Equal => return Ok(zero_impl()),
            ComparisonResult::GreaterThan | ComparisonResult::Undefined => {}
        }

        let x_span = x.digits();
        let y_span = y.digits();
        if y_span.len() == 1 {
            let divisor = y_span[0];
            if divisor == 1 {
                return Ok(zero_impl());
            }

            let mut remainder_digit: Digit = 0;
            Self::divide_single(&mut [], &mut remainder_digit, x_span, divisor);
            if remainder_digit == 0 {
                return Ok(zero_impl());
            }

            let mut remainder = JSBigInt::create_with_length(1)?;
            remainder.set_digit(0, remainder_digit);
            remainder.set_sign(x.sign());
            return Ok(ImplResult::Heap(remainder));
        }

        let mut r: Vec<Digit> = vec![0; y_span.len()];
        let r_span: &[Digit] = if x_span.len() == y_span.len() {
            Self::remainder_same_size(&mut r, x_span, y_span)
        } else {
            let mut interrupt = InterruptCheck::new(None);
            let (_, r_span) = Self::divide_digits_into(&mut interrupt, &mut [], &mut r, x_span, y_span);
            if interrupt.interrupted() {
                return Ok(ImplResult::Empty);
            }
            r_span
        };
        JSBigInt::try_create_from_impl(x.sign(), r_span).map(ImplResult::Heap)
    }

    /// `JSBigInt::remainder(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn remainder(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::remainder_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::absoluteAddOne(std::span<const Digit>, std::span<Digit>)`
    pub fn absolute_add_one_digits<'a>(x: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        debug_assert!(result.len() >= add_one_length(x));
        let mut carry: Digit = 1;
        let mut i = 0;
        while i < x.len() {
            let mut new_carry: Digit = 0;
            result[i] = digit_add(x[i], carry, &mut new_carry);
            carry = new_carry;
            i += 1;
        }
        if carry != 0 {
            result[i] = carry;
            i += 1;
        }
        &mut result[..i]
    }

    /// `JSBigInt::absoluteSubOne(std::span<const Digit>, std::span<Digit>)`
    pub fn absolute_sub_one_digits<'a>(x: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        debug_assert!(!x.is_empty());
        debug_assert!(result.len() >= sub_one_length(x));
        let mut borrow: Digit = 1;
        for i in 0..x.len() {
            let mut new_borrow: Digit = 0;
            result[i] = digit_sub(x[i], borrow, &mut new_borrow);
            borrow = new_borrow;
        }
        debug_assert!(borrow == 0);
        &mut result[..x.len()]
    }

    /// `|x| + 1` como dígitos normalizados (o `finalResult` de cada operação bit a bit).
    fn absolute_add_one_vec(x: &[Digit]) -> Vec<Digit> {
        normalized_digits(add_one_length(x), |result| Self::absolute_add_one_digits(x, result))
    }

    /// `normalize(|x| - 1)` como dígitos (o `resultX` de cada operação bit a bit).
    fn absolute_sub_one_vec(x: &[Digit]) -> Vec<Digit> {
        normalized_digits(sub_one_length(x), |result| Self::absolute_sub_one_digits(x, result))
    }

    /// `JSBigInt::absoluteAddOne(JSGlobalObject*, std::span<const Digit>, bool resultSign)`
    pub fn absolute_add_one(x: &[Digit], result_sign: bool) -> Result<ImplResult, BigIntError> {
        let result_length = add_one_length(x);
        if result_length > MAX_LENGTH as usize {
            let mut scratch: Vec<Digit> = try_zeroed_digits(result_length)?;
            let result = Self::absolute_add_one_digits(x, &mut scratch);
            return JSBigInt::try_create_from_impl(result_sign, result).map(ImplResult::Heap);
        }

        let mut big_int = JSBigInt::create_with_length(result_length as u32)?;
        big_int.set_sign(result_sign);

        let length = Self::absolute_add_one_digits(x, big_int.digits_mut()).len();
        debug_assert!(length != 0);
        debug_assert!(big_int.digit(length as u32 - 1) != 0);
        if length < result_length {
            big_int.set_length(length as u32);
        }
        Ok(ImplResult::Heap(big_int))
    }

    /// `JSBigInt::absoluteSubOne(JSGlobalObject*, std::span<const Digit>, bool resultSign)`
    pub fn absolute_sub_one(x: &[Digit], result_sign: bool) -> Result<ImplResult, BigIntError> {
        debug_assert!(!x.is_empty());
        let result_length = sub_one_length(x);
        let mut big_int = JSBigInt::create_with_length(result_length as u32)?;
        big_int.set_sign(result_sign);

        let length = normalize(&*Self::absolute_sub_one_digits(x, big_int.digits_mut())).len();
        if length == 0 {
            return Ok(zero_impl());
        }
        if length < result_length {
            big_int.set_length(length as u32);
        }
        Ok(ImplResult::Heap(big_int))
    }

    /// `JSBigInt::incImpl`
    pub fn inc_impl<B: BigIntImpl>(x: &B) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        if !x.sign() {
            return Self::absolute_add_one(x_span, false);
        }
        Self::absolute_sub_one(x_span, true)
    }

    /// `JSBigInt::inc(JSGlobalObject*, JSBigInt*)`
    pub fn inc(x: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::inc_impl(&HeapBigIntImpl::new(x)))
    }

    /// `JSBigInt::decImpl`
    pub fn dec_impl<B: BigIntImpl>(x: &B) -> Result<ImplResult, BigIntError> {
        if x.is_zero() {
            return JSBigInt::create_from_i32(-1).map(ImplResult::Heap);
        }

        let x_span = x.digits();
        if !x.sign() {
            return Self::absolute_sub_one(x_span, false);
        }
        Self::absolute_add_one(x_span, true)
    }

    /// `JSBigInt::dec(JSGlobalObject*, JSBigInt*)`
    pub fn dec(x: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::dec_impl(&HeapBigIntImpl::new(x)))
    }

    /// `JSBigInt::addSchoolbook`
    pub fn add_schoolbook<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(x.len() >= y.len());
        assert!(result.len() > x.len());
        let mut carry: Digit = 0;
        let mut i = 0;
        while i < y.len() {
            let mut new_carry: Digit = 0;
            result[i] = digit_add3(x[i], y[i], carry, &mut new_carry);
            carry = new_carry;
            i += 1;
        }

        while i < x.len() {
            let mut new_carry: Digit = 0;
            result[i] = digit_add(x[i], carry, &mut new_carry);
            carry = new_carry;
            i += 1;
        }

        result[i] = carry;
        i += 1;
        &mut result[..i]
    }

    /// `JSBigInt::absoluteAdd`
    pub fn absolute_add<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
        result_sign: bool,
    ) -> Result<ImplResult, BigIntError> {
        if x.length() < y.length() {
            return Self::absolute_add(y, x, result_sign);
        }

        if x.is_zero() {
            debug_assert!(y.is_zero());
            return Ok(x.to_impl_result());
        }

        if y.is_zero() {
            if result_sign == x.sign() {
                return Ok(x.to_impl_result());
            }
            return JSBigInt::unary_minus_impl(x);
        }

        let result_length = x.length() + 1;
        if result_length > MAX_LENGTH {
            let mut scratch: Vec<Digit> = try_zeroed_digits(result_length as usize)?;
            let span = Self::add_schoolbook(x.digits(), y.digits(), &mut scratch);
            return JSBigInt::try_create_from_impl(result_sign, span).map(ImplResult::Heap);
        }

        let mut big_int = JSBigInt::create_with_length(result_length)?;
        big_int.set_sign(result_sign);

        let span = Self::add_schoolbook(x.digits(), y.digits(), big_int.digits_mut());
        debug_assert!(!span.is_empty());
        let length = span.len();
        if span[length - 1] == 0 {
            big_int.set_length(length as u32 - 1);
        }

        Ok(ImplResult::Heap(big_int))
    }

    /// `JSBigInt::absoluteSub`
    pub fn absolute_sub<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
        result_sign: bool,
    ) -> Result<ImplResult, BigIntError> {
        // Callers fold the equal case into zero, so |x| > |y| here and neither x nor the difference is
        // zero.
        debug_assert!(absolute_compare(x, y) == ComparisonResult::GreaterThan);
        debug_assert!(!x.is_zero());

        if y.is_zero() {
            if result_sign == x.sign() {
                return Ok(x.to_impl_result());
            }
            return JSBigInt::unary_minus_impl(x);
        }

        let result_length = x.length();
        if result_length > MAX_IN_PLACE_SUB_SIZE {
            let mut scratch: Vec<Digit> = try_zeroed_digits(result_length as usize)?;
            let span = Self::sub_schoolbook(x.digits(), y.digits(), &mut scratch);
            return JSBigInt::try_create_from_impl(result_sign, span).map(ImplResult::Heap);
        }

        let mut big_int = JSBigInt::create_with_length(result_length)?;
        big_int.set_sign(result_sign);

        let length = normalize(&*Self::sub_schoolbook(x.digits(), y.digits(), big_int.digits_mut())).len();
        debug_assert!(length != 0);
        big_int.set_length(length as u32);
        Ok(ImplResult::Heap(big_int))
    }

    /// `JSBigInt::addImpl`
    pub fn add_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        let x_sign = x.sign();

        // x + y == x + y
        // -x + -y == -(x + y)
        if x_sign == y.sign() {
            return Self::absolute_add(x, y, x_sign);
        }

        // x + -y == x - y == -(y - x)
        // -x + y == y - x == -(x - y)
        match absolute_compare(x, y) {
            ComparisonResult::Equal => Ok(zero_impl()),
            ComparisonResult::GreaterThan => Self::absolute_sub(x, y, x_sign),
            _ => Self::absolute_sub(y, x, !x_sign),
        }
    }

    /// `JSBigInt::add(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn add(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::add_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::subImpl`
    pub fn sub_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        let x_sign = x.sign();
        if x_sign != y.sign() {
            // x - (-y) == x + y
            // (-x) - y == -(x + y)
            return Self::absolute_add(x, y, x_sign);
        }
        // x - y == -(y - x)
        // (-x) - (-y) == y - x == -(x - y)
        match absolute_compare(x, y) {
            ComparisonResult::Equal => Ok(zero_impl()),
            ComparisonResult::GreaterThan => Self::absolute_sub(x, y, x_sign),
            _ => Self::absolute_sub(y, x, !x_sign),
        }
    }

    /// `JSBigInt::sub(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn sub(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::sub_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::absoluteBitwiseOp`: aplica `op` aos pares de dígitos de `x` e `y`; quando o menor
    /// termina, `extra_digits` decide se os dígitos restantes do maior são copiados ou ignorados.
    fn absolute_bitwise_op<'a>(
        x: &[Digit],
        y: &[Digit],
        extra_digits: ExtraDigitsHandling,
        op: impl Fn(Digit, Digit) -> Digit,
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        let (x, y) = if x.len() < y.len() { (y, x) } else { (x, y) };

        debug_assert!(x.len() >= y.len());

        let num_pairs = y.len();
        let max_length = x.len();

        let result_length = if extra_digits == ExtraDigitsHandling::Copy { max_length } else { num_pairs };
        assert!(result.len() >= result_length);

        for i in 0..num_pairs {
            result[i] = op(x[i], y[i]);
        }

        if extra_digits == ExtraDigitsHandling::Copy && num_pairs != max_length {
            result[num_pairs..max_length].copy_from_slice(&x[num_pairs..]);
        }

        &mut result[..result_length]
    }

    /// `JSBigInt::absoluteAnd`
    pub fn absolute_and<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        debug_assert!(result.len() >= and_length(x, y));
        Self::absolute_bitwise_op(x, y, ExtraDigitsHandling::Skip, |a, b| a & b, result)
    }

    /// `JSBigInt::absoluteOr`
    pub fn absolute_or<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        debug_assert!(result.len() >= or_length(x, y));
        Self::absolute_bitwise_op(x, y, ExtraDigitsHandling::Copy, |a, b| a | b, result)
    }

    /// `JSBigInt::absoluteAndNot`: x & ~y
    pub fn absolute_and_not<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(result.len() >= x.len());

        let mut i = 0;
        while i < x.len().min(y.len()) {
            result[i] = x[i] & !y[i];
            i += 1;
        }

        while i < x.len() {
            result[i] = x[i];
            i += 1;
        }

        &mut result[..x.len()]
    }

    /// `JSBigInt::absoluteXor`
    pub fn absolute_xor<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        debug_assert!(result.len() >= or_length(x, y));
        Self::absolute_bitwise_op(x, y, ExtraDigitsHandling::Copy, |a, b| a ^ b, result)
    }

    /// Separa os operandos de sinais diferentes em `(positivo, negativo)`, que é o `computeResult`
    /// que o C++ chama com os dois na ordem certa.
    fn split_mixed_signs<'a, B1: BigIntImpl, B2: BigIntImpl>(x: &'a B1, y: &'a B2) -> (&'a [Digit], &'a [Digit]) {
        debug_assert!(x.sign() != y.sign());
        if x.sign() {
            (y.digits(), x.digits())
        } else {
            (x.digits(), y.digits())
        }
    }

    /// `JSBigInt::bitwiseAndImpl`
    pub fn bitwise_and_impl<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        let y_span = y.digits();
        if !x.sign() && !y.sign() {
            let result = normalized_digits(and_length(x_span, y_span), |r| Self::absolute_and(x_span, y_span, r));
            return JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap);
        }

        if x.sign() && y.sign() {
            // (-x) & (-y) == ~(x-1) & ~(y-1) == ~((x-1) | (y-1))
            // == -(((x-1) | (y-1)) + 1)
            let result_x = Self::absolute_sub_one_vec(x_span);
            let result_y = Self::absolute_sub_one_vec(y_span);
            let result = normalized_digits(or_length(&result_x, &result_y), |r| Self::absolute_or(&result_x, &result_y, r));
            let final_result = Self::absolute_add_one_vec(&result);
            return JSBigInt::try_create_from_impl(true, &final_result).map(ImplResult::Heap);
        }

        // x & (-y) == x & ~(y-1)
        let (positive, negative) = Self::split_mixed_signs(x, y);
        let result_y = Self::absolute_sub_one_vec(negative);
        let result = normalized_digits(positive.len(), |r| Self::absolute_and_not(positive, &result_y, r));
        JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::bitwiseAnd(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn bitwise_and(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::bitwise_and_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::bitwiseOrImpl`
    pub fn bitwise_or_impl<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        let y_span = y.digits();
        if !x.sign() && !y.sign() {
            let result = normalized_digits(or_length(x_span, y_span), |r| Self::absolute_or(x_span, y_span, r));
            return JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap);
        }

        if x.sign() && y.sign() {
            // (-x) | (-y) == ~(x-1) | ~(y-1) == ~((x-1) & (y-1))
            // == -(((x-1) & (y-1)) + 1)
            let result_x = Self::absolute_sub_one_vec(x_span);
            let result_y = Self::absolute_sub_one_vec(y_span);
            let result = normalized_digits(and_length(&result_x, &result_y), |r| Self::absolute_and(&result_x, &result_y, r));
            let final_result = Self::absolute_add_one_vec(&result);
            return JSBigInt::try_create_from_impl(true, &final_result).map(ImplResult::Heap);
        }

        // x | (-y) == x | ~(y-1) == ~((y-1) &~ x) == -(((y-1) &~ x) + 1)
        let (positive, negative) = Self::split_mixed_signs(x, y);
        let result_y = Self::absolute_sub_one_vec(negative);
        let result = normalized_digits(result_y.len(), |r| Self::absolute_and_not(&result_y, positive, r));
        let final_result = Self::absolute_add_one_vec(&result);
        JSBigInt::try_create_from_impl(true, &final_result).map(ImplResult::Heap)
    }

    /// `JSBigInt::bitwiseOr(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn bitwise_or(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::bitwise_or_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::bitwiseXorImpl`
    pub fn bitwise_xor_impl<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        let y_span = y.digits();
        if !x.sign() && !y.sign() {
            let result = normalized_digits(or_length(x_span, y_span), |r| Self::absolute_xor(x_span, y_span, r));
            return JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap);
        }

        if x.sign() && y.sign() {
            // (-x) ^ (-y) == ~(x-1) ^ ~(y-1) == (x-1) ^ (y-1)
            let result_x = Self::absolute_sub_one_vec(x_span);
            let result_y = Self::absolute_sub_one_vec(y_span);
            let result = normalized_digits(or_length(&result_x, &result_y), |r| Self::absolute_xor(&result_x, &result_y, r));
            return JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap);
        }

        // x ^ (-y) == x ^ ~(y-1) == ~(x ^ (y-1)) == -((x ^ (y-1)) + 1)
        let (positive, negative) = Self::split_mixed_signs(x, y);
        let result_y = Self::absolute_sub_one_vec(negative);
        let result = normalized_digits(or_length(&result_y, positive), |r| Self::absolute_xor(&result_y, positive, r));
        let final_result = Self::absolute_add_one_vec(&result);
        JSBigInt::try_create_from_impl(true, &final_result).map(ImplResult::Heap)
    }

    /// `JSBigInt::bitwiseXor(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn bitwise_xor(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::bitwise_xor_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::bitwiseNotImpl`
    pub fn bitwise_not_impl<B: BigIntImpl>(x: &B) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        if x.sign() {
            // ~(-x) == ~(~(x-1)) == x-1
            let result = Self::absolute_sub_one_vec(x_span);
            return JSBigInt::try_create_from_impl(false, &result).map(ImplResult::Heap);
        }
        // ~x == -x-1 == -(x+1)
        let result = Self::absolute_add_one_vec(x_span);
        JSBigInt::try_create_from_impl(true, &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::bitwiseNot(JSGlobalObject*, JSBigInt*)`
    pub fn bitwise_not(x: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::bitwise_not_impl(&HeapBigIntImpl::new(x)))
    }

    /// `JSBigInt::leftShiftImpl`
    pub fn left_shift_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        if x.is_zero() || y.is_zero() {
            return Ok(x.to_impl_result());
        }

        if y.sign() {
            return Self::right_shift_by_absolute(x, y);
        }

        Self::left_shift_by_absolute(x, y)
    }

    /// `JSBigInt::leftShift(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn left_shift_big_int(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::left_shift_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::signedRightShiftImpl`
    pub fn signed_right_shift_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        if x.is_zero() || y.is_zero() {
            return Ok(x.to_impl_result());
        }

        if y.sign() {
            return Self::left_shift_by_absolute(x, y);
        }

        Self::right_shift_by_absolute(x, y)
    }

    /// `JSBigInt::signedRightShift(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn signed_right_shift(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::signed_right_shift_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)))
    }

    /// `JSBigInt::leftShiftByAbsolute`
    pub fn left_shift_by_absolute<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> Result<ImplResult, BigIntError> {
        let Some(shift) = to_shift_amount(y) else {
            return Err(BigIntError::TooBig);
        };

        let digit_shift = (shift / DIGIT_BITS as Digit) as usize;
        let bits_shift = (shift % DIGIT_BITS as Digit) as u32;
        let x_span = x.digits();
        let length = x_span.len();
        let grow = bits_shift != 0 && (x_span[length - 1] >> (DIGIT_BITS - bits_shift)) != 0;
        let result_length = length + digit_shift + usize::from(grow);
        if result_length > MAX_LENGTH as usize {
            return Err(BigIntError::TooBig);
        }

        let mut result: Vec<Digit> = try_zeroed_digits(result_length)?;
        if bits_shift == 0 {
            for i in digit_shift..result_length {
                result[i] = x_span[i - digit_shift];
            }
        } else {
            let mut carry: Digit = 0;
            for i in 0..length {
                let d = x_span[i];
                result[i + digit_shift] = (d << bits_shift) | carry;
                carry = d >> (DIGIT_BITS - bits_shift);
            }

            if grow {
                result[length + digit_shift] = carry;
            } else {
                debug_assert!(carry == 0);
            }
        }

        JSBigInt::try_create_from_impl(x.sign(), &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::rightShiftByAbsolute`
    pub fn right_shift_by_absolute<B1: BigIntImpl, B2: BigIntImpl>(x: &B1, y: &B2) -> Result<ImplResult, BigIntError> {
        let x_span = x.digits();
        let length = x_span.len();
        let sign = x.sign();
        let Some(shift) = to_shift_amount(y) else {
            return Self::right_shift_by_maximum(sign);
        };

        let digital_shift = (shift / DIGIT_BITS as Digit) as usize;
        let bits_shift = (shift % DIGIT_BITS as Digit) as u32;
        if length <= digital_shift {
            return Self::right_shift_by_maximum(sign);
        }

        let mut result_length = length - digital_shift;

        // For negative numbers, round down if any bit was shifted out (so that e.g.
        // -5n >> 1n == -3n and not -2n). Check now whether this will happen and
        // whether it can cause overflow into a new digit. If we allocate the result
        // large enough up front, it avoids having to do a second allocation later.
        let mut must_round_down = false;
        if sign {
            let mask: Digit = (1 << bits_shift) - 1;
            if x_span[digital_shift] & mask != 0 {
                must_round_down = true;
            } else if x_span[..digital_shift].iter().any(|digit| *digit != 0) {
                must_round_down = true;
            }
        }

        // If bitsShift is non-zero, it frees up bits, preventing overflow.
        if must_round_down && bits_shift == 0 {
            // Overflow cannot happen if the most significant digit has unset bits.
            let msd = x_span[length - 1];
            let rounding_can_overflow = !msd == 0;
            if rounding_can_overflow {
                result_length += 1;
            }
        }

        debug_assert!(result_length <= length);
        let mut result: Vec<Digit> = try_zeroed_digits(result_length)?;

        if bits_shift == 0 {
            result[result_length - 1] = 0;
            for i in digital_shift..length {
                result[i - digital_shift] = x_span[i];
            }
        } else {
            let mut carry: Digit = x_span[digital_shift] >> bits_shift;
            let last = length - digital_shift - 1;
            for i in 0..last {
                let d = x_span[i + digital_shift + 1];
                result[i] = (d << (DIGIT_BITS - bits_shift)) | carry;
                carry = d >> bits_shift;
            }
            result[last] = carry;
        }

        if sign && must_round_down {
            // Since the result is negative, rounding down means adding one to
            // its absolute value. This cannot overflow.
            let final_result = Self::absolute_add_one_vec(normalize(&result));
            return JSBigInt::try_create_from_impl(sign, &final_result).map(ImplResult::Heap);
        }

        JSBigInt::try_create_from_impl(sign, &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::rightShiftByMaximum`
    pub fn right_shift_by_maximum(sign: bool) -> Result<ImplResult, BigIntError> {
        if sign {
            return JSBigInt::create_from_i32(-1).map(ImplResult::Heap);
        }

        Ok(zero_impl())
    }

    /// `JSBigInt::decideRounding`
    pub fn decide_rounding(
        big_int: &JSBigInt,
        mantissa_bits_unset: i32,
        mut digit_index: i32,
        mut current_digit: u64,
    ) -> RoundingResult {
        if mantissa_bits_unset > 0 {
            return RoundingResult::RoundDown;
        }
        let top_unconsumed_bit: i32;
        if mantissa_bits_unset < 0 {
            // There are unconsumed bits in currentDigit.
            top_unconsumed_bit = -mantissa_bits_unset - 1;
        } else {
            debug_assert!(mantissa_bits_unset == 0);
            // currentDigit fit the mantissa exactly; look at the next digit.
            if digit_index == 0 {
                return RoundingResult::RoundDown;
            }
            digit_index -= 1;
            current_digit = big_int.digit(digit_index as u32);
            top_unconsumed_bit = DIGIT_BITS as i32 - 1;
        }
        // If the most significant remaining bit is 0, round down.
        let mut bitmask: u64 = 1 << top_unconsumed_bit;
        if current_digit & bitmask == 0 {
            return RoundingResult::RoundDown;
        }
        // If any other remaining bit is set, round up.
        bitmask -= 1;
        if current_digit & bitmask != 0 {
            return RoundingResult::RoundUp;
        }
        while digit_index > 0 {
            digit_index -= 1;
            if big_int.digit(digit_index as u32) != 0 {
                return RoundingResult::RoundUp;
            }
        }
        RoundingResult::Tie
    }

    /// `JSBigInt::toNumber(JSValue)` (`JSBigIntInlines.h`): `value` é um `BigInt` (sem `BigInt32`, só o
    /// caminho de heap).
    pub fn to_number(value: crate::runtime::js_value::JSValue) -> f64 {
        debug_assert!(value.is_big_int());
        match crate::runtime::cell_registry::get(value.as_cell()) {
            Some(crate::runtime::cell_registry::CellEntry::BigInt(big_int)) => Self::to_number_heap(&big_int),
            _ => unreachable!("célula HeapBigIntType fora do registro"),
        }
    }

    /// `JSBigInt::toNumberHeap`: o `double` que o `jsNumber` do C++ embrulha.
    pub fn to_number_heap(big_int: &JSBigInt) -> f64 {
        if big_int.is_zero() {
            return 0.0;
        }
        debug_assert!(big_int.length() != 0);

        // Conversion mechanism is the following.
        //
        // 1. Get exponent bits.
        // 2. Collect mantissa 52 bits.
        // 3. Add rounding result of unused bits to mantissa and adjust mantissa & exponent bits.
        // 4. Generate double by combining (1) and (3).

        let length = big_int.length();
        let sign = big_int.sign();
        let msd = big_int.digit(length - 1);
        let msd_leading_zeros = msd.leading_zeros();
        let bit_length = length as usize * DIGIT_BITS as usize - msd_leading_zeros as usize;
        let infinity = if sign { f64::NEG_INFINITY } else { f64::INFINITY };
        // Double's exponent bits overflow.
        if bit_length > 1024 {
            return infinity;
        }
        let mut exponent: u64 = bit_length as u64 - 1;
        let mut current_digit: u64 = msd;
        let mut digit_index = length as i32 - 1;
        let shift_amount = msd_leading_zeros as i32 + 1 + (64 - DIGIT_BITS as i32);
        debug_assert!((1..=64).contains(&shift_amount));
        let mut mantissa: u64 = if shift_amount == 64 { 0 } else { current_digit << shift_amount };

        // unsetBits = 64 - setBits - 12 // 12 for non-mantissa bits
        //     setBits = 64 - (msdLeadingZeros + 1 + bitsNotAvailableDueToDigitSize);  // 1 for hidden mantissa bit.
        //                 = 64 - (msdLeadingZeros + 1 + (64 - digitBits))
        //                 = 64 - shiftAmount
        // Hence, unsetBits = 64 - (64 - shiftAmount) - 12 = shiftAmount - 12

        mantissa >>= 12; // (12 = 64 - 52), we shift 12 bits to put 12 zeros in uint64_t mantissa.
        let mut mantissa_bits_unset = shift_amount - 12;

        // If not all mantissa bits are defined yet, get more digits as needed.
        // Collect mantissa 52bits from several digits.

        // `digitBits < 64` é falso em `CPU(REGISTER64)`.
        if mantissa_bits_unset > 0 && digit_index > 0 {
            debug_assert!(mantissa_bits_unset < DIGIT_BITS as i32);
            digit_index -= 1;
            current_digit = big_int.digit(digit_index as u32);
            mantissa |= current_digit >> (DIGIT_BITS as i32 - mantissa_bits_unset);
            mantissa_bits_unset -= DIGIT_BITS as i32;
        }

        // If there are unconsumed digits left, we may have to round.
        let rounding = Self::decide_rounding(big_int, mantissa_bits_unset, digit_index, current_digit);
        if rounding == RoundingResult::RoundUp || (rounding == RoundingResult::Tie && (mantissa & 1) == 1) {
            mantissa += 1;
            // Incrementing the mantissa can overflow the mantissa bits. In that case the new mantissa will be all zero (plus hidden bit).
            if (mantissa >> DOUBLE_PHYSICAL_MANTISSA_SIZE) != 0 {
                mantissa = 0;
                exponent += 1;
                // Incrementing the exponent can overflow too.
                if exponent > 1023 {
                    return infinity;
                }
            }
        }

        let sign_bit: u64 = if sign { 1 << 63 } else { 0 };
        exponent = (exponent + 0x3ff) << DOUBLE_PHYSICAL_MANTISSA_SIZE; // 0x3ff is double exponent bias.
        f64::from_bits(sign_bit | exponent | mantissa)
    }

    /// `JSBigInt::toBigUInt64Heap`
    pub fn to_big_uint64_heap(big_int: &JSBigInt) -> u64 {
        let length = big_int.length();
        if length == 0 {
            return 0;
        }
        // `sizeof(Digit) == 8`
        let value: u64 = big_int.digit(0);
        if !big_int.sign() {
            return value;
        }
        !(value.wrapping_sub(1)) // To avoid undefined behavior, we compute two's compliment by hand in C while this is simply `-value`.
    }

    /// `JSBigInt::asIntNImpl`
    pub fn as_int_n_impl<B: ExponentiateOperand>(n: u64, big_int: &B) -> Result<ImplResult, BigIntError> {
        if big_int.is_zero() {
            return Ok(big_int.to_impl_result());
        }
        if n == 0 {
            return Ok(zero_impl());
        }

        let needed_length: u64 = (n + DIGIT_BITS as u64 - 1) / DIGIT_BITS as u64;
        let length = big_int.length() as u64;
        // If bigInt has less than n bits, return it directly.
        if length < needed_length {
            return Ok(big_int.to_impl_result());
        }
        debug_assert!(needed_length <= i32::MAX as u64);
        let top_digit = big_int.digit(needed_length as u32 - 1);
        let compare_digit: Digit = 1 << ((n - 1) % DIGIT_BITS as u64);
        if length == needed_length && top_digit < compare_digit {
            return Ok(big_int.to_impl_result());
        }

        // Otherwise we have to truncate (which is a no-op in the special case
        // of bigInt == -2^(n-1)), and determine the right sign. We also might have
        // to subtract from 2^n to simulate having two's complement representation.
        // In most cases, the result's sign is bigInt.sign() xor "(n-1)th bit present".
        // The only exception is when bigInt is negative, has the (n-1)th bit, and all
        // its bits below (n-1) are zero. In that case, the result is the minimum
        // n-bit integer (example: asIntN(3, -12n) => -4n).
        let has_bit = (top_digit & compare_digit) == compare_digit;
        debug_assert!(n <= i32::MAX as u64);
        let n = n as i32;
        if !has_bit {
            return Self::truncate_to_n_bits(n, big_int);
        }
        if !big_int.sign() {
            return Self::truncate_and_sub_from_power_of_two(n, big_int, true);
        }

        // Negative numbers must subtract from 2^n, except for the special case
        // described above.
        if (top_digit & (compare_digit - 1)) == 0 {
            for i in (0..needed_length as i64 - 1).rev() {
                if big_int.digit(i as u32) != 0 {
                    return Self::truncate_and_sub_from_power_of_two(n, big_int, false);
                }
            }
            // Truncation is no-op if bigInt == -2^(n-1).
            if length == needed_length && top_digit == compare_digit {
                return Ok(big_int.to_impl_result());
            }
            return Self::truncate_to_n_bits(n, big_int);
        }
        Self::truncate_and_sub_from_power_of_two(n, big_int, false)
    }

    /// `JSBigInt::asUintNImpl`
    pub fn as_uint_n_impl<B: ExponentiateOperand>(n: u64, big_int: &B) -> Result<ImplResult, BigIntError> {
        if big_int.is_zero() {
            return Ok(big_int.to_impl_result());
        }
        if n == 0 {
            return Ok(zero_impl());
        }

        // If bigInt is negative, simulate two's complement representation.
        if big_int.sign() {
            if n > MAX_LENGTH_BITS as u64 {
                return Err(BigIntError::TooBig);
            }
            return Self::truncate_and_sub_from_power_of_two(n as i32, big_int, false);
        }

        // If bigInt is positive and has up to n bits, return it directly.
        if n >= MAX_LENGTH_BITS as u64 {
            return Ok(big_int.to_impl_result());
        }
        const _: () = assert!(MAX_LENGTH_BITS < (i32::MAX as u32) - DIGIT_BITS);
        let needed_length = ((n + DIGIT_BITS as u64 - 1) / DIGIT_BITS as u64) as i32;
        if (big_int.length() as i32) < needed_length {
            return Ok(big_int.to_impl_result());
        }

        let bits_in_top_digit = (n % DIGIT_BITS as u64) as u32;
        if big_int.length() as i32 == needed_length {
            if bits_in_top_digit == 0 {
                return Ok(big_int.to_impl_result());
            }
            let top_digit = big_int.digit(needed_length as u32 - 1);
            if (top_digit >> bits_in_top_digit) == 0 {
                return Ok(big_int.to_impl_result());
            }
        }

        // Otherwise, truncate.
        debug_assert!(n <= i32::MAX as u64);
        Self::truncate_to_n_bits(n as i32, big_int)
    }

    /// `JSBigInt::truncateToNBits`
    pub fn truncate_to_n_bits<B: BigIntImpl>(n: i32, big_int: &B) -> Result<ImplResult, BigIntError> {
        let span = big_int.digits();

        debug_assert!(n != 0);
        debug_assert!(span.len() as i64 > (n / DIGIT_BITS as i32) as i64);

        let needed_digits = (n + (DIGIT_BITS as i32 - 1)) / DIGIT_BITS as i32;
        debug_assert!(needed_digits <= span.len() as i32);

        let mut result: Vec<Digit> = try_zeroed_digits(needed_digits as usize)?;

        // Copy all digits except the MSD.
        let last = (needed_digits - 1) as usize;
        result[..last].copy_from_slice(&span[..last]);

        // The MSD might contain extra bits that we don't want.
        let mut msd = span[last];
        if n % DIGIT_BITS as i32 != 0 {
            let drop = DIGIT_BITS - (n as u32 % DIGIT_BITS);
            msd = (msd << drop) >> drop;
        }
        result[last] = msd;
        JSBigInt::try_create_from_impl(big_int.sign(), &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::truncateAndSubFromPowerOfTwo`: subtrai os `n` bits menos significativos de
    /// `abs(bigInt)` de 2^n.
    pub fn truncate_and_sub_from_power_of_two<B: BigIntImpl>(
        n: i32,
        big_int: &B,
        result_sign: bool,
    ) -> Result<ImplResult, BigIntError> {
        debug_assert!(n != 0);
        debug_assert!(n <= MAX_LENGTH_BITS as i32);

        let span = big_int.digits();

        let needed_digits = (n + (DIGIT_BITS as i32 - 1)) / DIGIT_BITS as i32;
        debug_assert!(needed_digits <= MAX_LENGTH as i32); // Follows from n <= maxLengthBits.

        let mut result: Vec<Digit> = try_zeroed_digits(needed_digits as usize)?;

        // Process all digits except the MSD.
        let mut i: i32 = 0;
        let last = needed_digits - 1;
        let length = span.len() as i32;
        let mut borrow: Digit = 0;
        // Take digits from bigInt unless its length is exhausted.
        let limit = last.min(length);
        while i < limit {
            let mut new_borrow: Digit = 0;
            let mut difference = digit_sub(0, span[i as usize], &mut new_borrow);
            difference = digit_sub(difference, borrow, &mut new_borrow);
            result[i as usize] = difference;
            borrow = new_borrow;
            i += 1;
        }
        // Then simulate leading zeroes in {bigInt} as needed.
        while i < last {
            let mut new_borrow: Digit = 0;
            let difference = digit_sub(0, borrow, &mut new_borrow);
            result[i as usize] = difference;
            borrow = new_borrow;
            i += 1;
        }

        // The MSD might contain extra bits that we don't want.
        let mut msd: Digit = if last < length { span[last as usize] } else { 0 };
        let msd_bits_consumed = n % DIGIT_BITS as i32;
        let mut result_msd: Digit;
        if msd_bits_consumed == 0 {
            let mut new_borrow: Digit = 0;
            result_msd = digit_sub(0, msd, &mut new_borrow);
            result_msd = digit_sub(result_msd, borrow, &mut new_borrow);
        } else {
            let drop = DIGIT_BITS - msd_bits_consumed as u32;
            msd = (msd << drop) >> drop;
            let minuend_msd: Digit = 1 << (DIGIT_BITS - drop);
            let mut new_borrow: Digit = 0;
            result_msd = digit_sub(minuend_msd, msd, &mut new_borrow);
            result_msd = digit_sub(result_msd, borrow, &mut new_borrow);
            debug_assert!(new_borrow == 0); // result < 2^n.
            // If all subtracted bits were zero, we have to get rid of the
            // materialized minuendMSD again.
            result_msd &= minuend_msd - 1;
        }
        result[last as usize] = result_msd;
        JSBigInt::try_create_from_impl(result_sign, &result).map(ImplResult::Heap)
    }

    /// `JSBigInt::asIntN(JSGlobalObject*, uint64_t, JSBigInt*)`
    pub fn as_int_n(n: u64, big_int: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::as_int_n_impl(n, &HeapBigIntImpl::new(big_int)))
    }

    /// `JSBigInt::asUintN(JSGlobalObject*, uint64_t, JSBigInt*)`
    pub fn as_uint_n(n: u64, big_int: &JSBigInt) -> Result<ImplResult, BigIntError> {
        public_result(JSBigInt::as_uint_n_impl(n, &HeapBigIntImpl::new(big_int)))
    }
}
