// Quinta fatia do porte de `JSBigInt.cpp`: as linhas 2625 a 2815 (`multiplyFFT`,
// `multiplyDigitsInto`, `multiplyImpl` e os `multiply` públicos), mais as definições vizinhas que
// as fatias 2 a 4 já chamam: `addAndReturnCarry`, `subtractAndReturnBorrow`, `inplaceAdd`,
// `inplaceSub` (linhas 2919 a 2953), `copy`, `unaryMinusImpl`, `unaryMinus`, `compareDigits`,
// `multiplyDigits` (linhas 4137 a 4205), `subSchoolbook` (linha 5768) e
// `tryConvertToBigInt32(JSBigInt*)` (de `JSBigInt.h`, linha 743).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Dependências de fatias futuras (ainda inexistentes): `JSBigInt::add_digits_into` (usada por
// `addDigits`, linha 4185, que por isso fica para a fatia que a trouxer).
//
// Desvios mecânicos de Rust seguro, sem efeito observável:
// - Sem `JSGlobalObject`/`VM`, os `throwOutOfMemoryError`/`throwRangeError` viram `BigIntError` e
//   o `JSValue` vira `ImplResult`. O `InterruptCheck` nasce sem `vm_check`, então nunca interrompe;
//   o ramo de interrupção de `multiplyImpl` devolve `ImplResult::Empty` como o C++ devolve `nullptr`.
// - `tryAllocateCell` aceita `resultLength == maxLength + 1` (o C++ valida o tamanho só no fim),
//   então a célula é montada direto, sem passar pelo limite de `create_with_length`.
// - O quadrado da FFT (`a.pointwiseMultiply(a)`) usa uma cópia do container como segundo operando,
//   pois o empréstimo mutável de `self` não pode coexistir com `other`. O C++ lê cada parte `i`
//   antes de gravá-la e só modifica as partes `i - 1` e `i` depois do produto `i`, então a cópia
//   dá o mesmo resultado.
// - `addAndReturnCarry(z, z, x)` do `inplaceAdd` (Z alias de X) vira um único laço que lê `x` da
//   própria `z` quando o chamador não passa uma fonte separada.

/// `tryConvertToBigInt32(JSBigInt*)` de `JSBigInt.h`. Com `USE(BIGINT32)` em 0
/// (`PlatformUse.h:141`) o corpo de conversão some e a célula volta como veio.
pub fn try_convert_to_big_int32_heap(big_int: JSBigInt) -> ImplResult {
    ImplResult::Heap(big_int)
}

impl JSBigInt {
    /// `JSBigInt::multiplyFFT`
    pub fn multiply_fft<'a>(
        interrupt: &mut InterruptCheck,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        use fft::{get_parameters, Parameters, ASYMMETRIC_CHUNKING_THRESHOLD};
        debug_assert!(x.len() >= y.len());
        debug_assert!(should_use_fft(x.len(), y.len()));
        assert!(result.len() >= x.len() + y.len());
        let z = &mut result[..x.len() + y.len()];

        let mut params = Parameters::default();
        if core::ptr::eq(x.as_ptr(), y.as_ptr()) && x.len() == y.len() {
            // Squaring.
            let m = get_parameters(x.len() * 2, &mut params);
            let omega = params.r; // na verdade: 2^r
            let mut a = FFTContainer::new(params.n, params.k);
            a.start(interrupt, x, params.s, 0, omega);
            let copy = FFTContainer { n: a.n, k: a.k, length: a.length, storage: a.storage.clone() };
            a.pointwise_multiply(interrupt, &copy);
            a.backward_fft(interrupt, 0, params.n, omega);
            a.normalize_and_recombine(interrupt, omega, m, z, params.s);
        } else if x.len() > y.len() * ASYMMETRIC_CHUNKING_THRESHOLD {
            // Asymmetric input sizes. Proceed in chunks. See multiplyToomCook.
            let k = y.len();
            let m = get_parameters(k * 2, &mut params);
            let omega = params.r; // na verdade: 2^r
            // The container {b} only needs to be initialized once, whereas {a} will be reused for
            // each chunk.
            let mut b = FFTContainer::new(params.n, params.k);
            b.start(interrupt, y, params.s, 0, omega);
            let mut a = FFTContainer::new(params.n, params.k);
            // Unroll the first iteration to initialize {z}.
            let x0 = clamped_subspan(x, 0, k);
            a.start(interrupt, x0, params.s, 0, omega);
            a.pointwise_multiply(interrupt, &b);
            a.backward_fft(interrupt, 0, params.n, omega);
            a.normalize_and_recombine(interrupt, omega, m, z, params.s);
            // Then loop for the remaining chunks.
            let mut product: Vec<Digit> = vec![0; 2 * k];
            let mut i = k;
            while i < x.len() && !interrupt.interrupted() {
                let xi = clamped_subspan(x, i, k);
                a.start(interrupt, xi, params.s, 0, omega);
                a.pointwise_multiply(interrupt, &b);
                a.backward_fft(interrupt, 0, params.n, omega);
                a.normalize_and_recombine(interrupt, omega, m, &mut product, params.s);
                Self::inplace_add_and_propagate(&mut z[i..], &product);
                i += k;
            }
        } else {
            // Similar-ish sized inputs. Handle them in one go.
            let m = get_parameters(x.len() + y.len(), &mut params);
            let omega = params.r; // na verdade: 2^r

            let mut a = FFTContainer::new(params.n, params.k);
            a.start(interrupt, x, params.s, 0, omega);
            let mut b = FFTContainer::new(params.n, params.k);
            b.start(interrupt, y, params.s, 0, omega);
            a.pointwise_multiply(interrupt, &b);
            a.backward_fft(interrupt, 0, params.n, omega);
            a.normalize_and_recombine(interrupt, omega, m, z, params.s);
        }
        z
    }

    /// `JSBigInt::multiplyDigitsInto`
    pub fn multiply_digits_into<'a>(
        interrupt: &mut InterruptCheck,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        debug_assert!(!y.is_empty());
        debug_assert!(x.len() >= y.len());
        debug_assert!(result.len() >= x.len() + y.len());

        if x.len() == y.len() {
            // Aliased operands mean squaring, which needs only half of the digit multiplies.
            let is_square = core::ptr::eq(x.as_ptr(), y.as_ptr());
            match y.len() {
                1 => return Self::comba_fixed_or_square::<1>(is_square, x, y, result),
                2 => return Self::comba_fixed_or_square::<2>(is_square, x, y, result),
                4 => return Self::comba_fixed_or_square::<4>(is_square, x, y, result),
                8 => return Self::comba_fixed_or_square::<8>(is_square, x, y, result),
                16 => return Self::comba_fixed_or_square::<16>(is_square, x, y, result),
                _ => {}
            }
        }
        if y.len() == 1 {
            return Self::multiply_single(x, y[0], result);
        }
        if y.len() >= KARATSUBA_THRESHOLD {
            if y.len() < TOOM_THRESHOLD {
                return Self::multiply_karatsuba(interrupt, x, y, result);
            }
            if should_use_fft(x.len(), y.len()) {
                return Self::multiply_fft(interrupt, x, y, result);
            }
            return Self::multiply_toom_cook(interrupt, x, y, result);
        }
        if should_use_comba(x.len(), y.len()) {
            return Self::multiply_comba(x, y, result);
        }
        Self::multiply_schoolbook(x, y, result)
    }

    /// O corpo comum dos cinco `case` do `switch` de `multiplyDigitsInto` (`squareCombaFixed<N>`
    /// ou `multiplyCombaFixed<N>`, com `first<N>()` e `first<2N>()`).
    fn comba_fixed_or_square<'a, const N: usize>(
        is_square: bool,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        let fixed_x: &[Digit; N] = x[..N].try_into().unwrap();
        let out = &mut result[..2 * N];
        if is_square {
            return Self::square_comba_fixed::<N>(fixed_x, out);
        }
        let fixed_y: &[Digit; N] = y[..N].try_into().unwrap();
        Self::multiply_comba_fixed::<N>(fixed_x, fixed_y, out)
    }

    /// `JSBigInt::multiplyImpl`
    pub fn multiply_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        x: &B1,
        y: &B2,
    ) -> Result<ImplResult, BigIntError> {
        if x.length() < y.length() {
            return JSBigInt::multiply_impl(y, x);
        }

        debug_assert!(x.length() >= y.length());

        if y.is_zero() {
            return Ok(y.to_impl_result());
        }

        let result_length = x.length() + y.length();
        let result_sign = x.sign() != y.sign();

        if y.length() == 1 && y.digit(0) == 1 {
            if result_sign == x.sign() {
                return Ok(x.to_impl_result());
            }
            return JSBigInt::unary_minus_impl(x);
        }

        // N * M result with non-zero digits N / M is guaranteed to be (N + M - 1) or (N + M) length.
        // It is not so wasteful if we just allocate JSBigInt with N + M here.
        // It is possible that we will hit the JSBigInt size limit, so let's validate it after all the computation.
        if result_length - 1 > MAX_LENGTH {
            return Err(BigIntError::TooBig);
        }

        let x_span = x.digits();
        let y_span = y.digits();
        debug_assert!(!x_span.is_empty());
        debug_assert!(!y_span.is_empty());

        // Note that resultLength can be one-larger than maxLength.
        // We still accept. And if the adjusted result is still larger, we will throw an OOM error.
        let mut digits: Vec<Digit> = Vec::new();
        if digits.try_reserve_exact(result_length as usize).is_err() {
            return Err(BigIntError::TooBig);
        }
        digits.resize(result_length as usize, 0);
        let mut big_int = JSBigInt { sign: result_sign, digits, hash: 0, structure: BigIntStructure::default() };

        let mut interrupt = InterruptCheck::new(None);
        let mut length = JSBigInt::multiply_digits_into(&mut interrupt, x_span, y_span, &mut big_int.digits).len();
        if interrupt.interrupted() {
            // The digits were never finished, so leave the cell as a zero rather than a value.
            return Ok(ImplResult::Empty);
        }
        debug_assert!(length != 0);
        if big_int.digits[length - 1] == 0 {
            length -= 1;
        }

        if length > MAX_LENGTH as usize {
            return Err(BigIntError::TooBig);
        }
        big_int.set_length(length as u32);

        Ok(ImplResult::Heap(big_int))
    }

    /// `JSBigInt::multiply(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn multiply(x: &JSBigInt, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        JSBigInt::multiply_impl(&HeapBigIntImpl::new(x), &HeapBigIntImpl::new(y)).map(try_convert_to_big_int32)
    }

    /// `JSBigInt::multiply(JSGlobalObject*, int32_t, JSBigInt*)`
    pub fn multiply_int32_heap(x: i32, y: &JSBigInt) -> Result<ImplResult, BigIntError> {
        JSBigInt::multiply_impl(&Int32BigIntImpl::new(x), &HeapBigIntImpl::new(y)).map(try_convert_to_big_int32)
    }

    /// `JSBigInt::multiply(JSGlobalObject*, JSBigInt*, int32_t)`
    pub fn multiply_heap_int32(x: &JSBigInt, y: i32) -> Result<ImplResult, BigIntError> {
        JSBigInt::multiply_impl(&HeapBigIntImpl::new(x), &Int32BigIntImpl::new(y)).map(try_convert_to_big_int32)
    }

    /// Laço de `addAndReturnCarry`. `source` é o `x` do C++; `None` significa `x == z`
    /// (o `inplaceAdd`).
    fn add_carry_loop(z: &mut [Digit], source: Option<&[Digit]>, y: &[Digit]) -> Digit {
        assert!(z.len() >= y.len() && source.is_none_or(|x| x.len() >= y.len()));
        let mut carry: Digit = 0;
        for i in 0..y.len() {
            let mut new_carry: Digit = 0;
            let xi = source.map_or(z[i], |x| x[i]);
            z[i] = digit_add3(xi, y[i], carry, &mut new_carry);
            carry = new_carry;
        }
        carry
    }

    /// Laço de `subtractAndReturnBorrow`, com `source` como em `add_carry_loop`.
    fn sub_borrow_loop(z: &mut [Digit], source: Option<&[Digit]>, y: &[Digit]) -> Digit {
        assert!(z.len() >= y.len() && source.is_none_or(|x| x.len() >= y.len()));
        let mut borrow: Digit = 0;
        for i in 0..y.len() {
            let mut borrow_out: Digit = 0;
            let xi = source.map_or(z[i], |x| x[i]);
            z[i] = digit_sub2(xi, y[i], borrow, &mut borrow_out);
            borrow = borrow_out;
        }
        borrow
    }

    /// `JSBigInt::addAndReturnCarry`
    pub fn add_and_return_carry(z: &mut [Digit], x: &[Digit], y: &[Digit]) -> Digit {
        Self::add_carry_loop(z, Some(x), y)
    }

    /// `JSBigInt::subtractAndReturnBorrow`
    pub fn subtract_and_return_borrow(z: &mut [Digit], x: &[Digit], y: &[Digit]) -> Digit {
        Self::sub_borrow_loop(z, Some(x), y)
    }

    /// Z += X. Returns the "carry" (0 or 1) after adding all of X's digits.
    pub fn inplace_add(z: &mut [Digit], x: &[Digit]) -> Digit {
        Self::add_carry_loop(z, None, x)
    }

    /// Z -= X. Returns the "borrow" (0 or 1) after subtracting all of X's digits.
    pub fn inplace_sub(z: &mut [Digit], x: &[Digit]) -> Digit {
        Self::sub_borrow_loop(z, None, x)
    }

    /// `JSBigInt::copy`
    pub fn copy<B: BigIntImpl>(x: &B) -> Result<JSBigInt, BigIntError> {
        debug_assert!(!x.is_zero());

        let mut result = JSBigInt::create_with_length(x.length())?;
        result.digits.copy_from_slice(x.digits());
        result.set_sign(x.sign());
        Ok(result)
    }

    /// `JSBigInt::unaryMinusImpl`
    pub fn unary_minus_impl<B: BigIntImpl>(x: &B) -> Result<ImplResult, BigIntError> {
        if x.is_zero() {
            return Ok(zero_impl());
        }

        let mut result = JSBigInt::copy(x)?;

        result.set_sign(!x.sign());
        Ok(ImplResult::Heap(result))
    }

    /// `JSBigInt::unaryMinus(JSGlobalObject*, JSBigInt*)`
    pub fn unary_minus(x: &JSBigInt) -> Result<ImplResult, BigIntError> {
        JSBigInt::unary_minus_impl(&HeapBigIntImpl::new(x)).map(try_convert_to_big_int32)
    }

    /// `JSBigInt::compareDigits`
    pub fn compare_digits(x: &[Digit], y: &[Digit]) -> ComparisonResult {
        let x = normalize(x);
        let y = normalize(y);
        if x.len() != y.len() {
            return if x.len() < y.len() { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
        }
        for i in (0..x.len()).rev() {
            if x[i] != y[i] {
                return if x[i] < y[i] { ComparisonResult::LessThan } else { ComparisonResult::GreaterThan };
            }
        }
        ComparisonResult::Equal
    }

    /// `JSBigInt::multiplyDigits`
    pub fn multiply_digits<'a>(
        interrupt: &mut InterruptCheck,
        x: &[Digit],
        y: &[Digit],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        let mut x = normalize(x);
        let mut y = normalize(y);
        if x.is_empty() || y.is_empty() {
            return &mut [];
        }
        if x.len() < y.len() {
            core::mem::swap(&mut x, &mut y);
        }
        assert!(result.len() >= x.len() + y.len());
        let length = normalize(Self::multiply_digits_into(interrupt, x, y, &mut *result)).len();
        &mut result[..length]
    }

    /// `JSBigInt::subSchoolbook`
    pub fn sub_schoolbook<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(x.len() >= y.len());
        assert!(result.len() >= x.len());
        let mut borrow: Digit = 0;
        let mut i = 0;
        while i < y.len() {
            let mut new_borrow: Digit = 0;
            result[i] = digit_sub2(x[i], y[i], borrow, &mut new_borrow);
            borrow = new_borrow;
            i += 1;
        }

        while i < x.len() {
            let mut new_borrow: Digit = 0;
            result[i] = digit_sub(x[i], borrow, &mut new_borrow);
            borrow = new_borrow;
            i += 1;
        }

        debug_assert!(borrow == 0);
        &mut result[..x.len()]
    }
}
