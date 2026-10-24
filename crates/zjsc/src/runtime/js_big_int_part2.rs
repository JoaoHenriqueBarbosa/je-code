// Segunda fatia do porte de `JSBigInt.cpp`, linhas 632 a 1335: `exponentiateImpl`, `exponentiate`,
// a multiplicação por colunas (Comba), `multiplySingle`, `multiplySchoolbook`, `multiplySpecialLow`
// e `multiplySpecialHigh` (mais as variantes `Fixed`), `shouldUseComba` e o preâmbulo do Karatsuba
// (`InterruptCheck`, `karatsubaLength`, `clampedSubspan`).
//
// Esta fatia é incluída por `include!` em `js_big_int.rs` e compartilha o escopo dele.
//
// Dependências de fatias futuras (referenciadas pelo nome em snake_case, ainda inexistentes):
//   `JSBigInt::multiply_impl`, `JSBigInt::unary_minus_impl`, `try_convert_to_big_int32_heap`
//   (o `tryConvertToBigInt32(JSBigInt*)` do C++), `digit_add`, `digit_add3`, `digit_mul`.

/// Adaptadores de operando de `exponentiateImpl` (os `BigIntImpl1`/`BigIntImpl2` do template que
/// também sabem virar célula e `ImplResult`: `HeapBigIntImpl` e `Int32BigIntImpl`).
pub trait ExponentiateOperand: BigIntImpl {
    /// `toHeapBigInt(JSGlobalObject*)`
    fn to_heap_big_int(&self) -> Result<JSBigInt, BigIntError>;
    /// `ImplResult { impl }`
    fn to_impl_result(&self) -> ImplResult;
}

impl ExponentiateOperand for HeapBigIntImpl<'_> {
    fn to_heap_big_int(&self) -> Result<JSBigInt, BigIntError> {
        HeapBigIntImpl::to_heap_big_int(self)
    }
    fn to_impl_result(&self) -> ImplResult {
        ImplResult::Heap(self.big_int.clone())
    }
}

impl ExponentiateOperand for Int32BigIntImpl {
    fn to_heap_big_int(&self) -> Result<JSBigInt, BigIntError> {
        Int32BigIntImpl::to_heap_big_int(self)
    }
    fn to_impl_result(&self) -> ImplResult {
        ImplResult::from(self)
    }
}

/// `tryConvertToBigInt32(JSBigInt::ImplResult)` (linha 613).
pub fn try_convert_to_big_int32(impl_result: ImplResult) -> ImplResult {
    match impl_result {
        ImplResult::Empty => ImplResult::Empty,
        ImplResult::BigInt32(_) => impl_result,
        ImplResult::Heap(big_int) => try_convert_to_big_int32_heap(big_int),
    }
}

impl JSBigInt {
    /// `JSBigInt::exponentiateImpl`
    pub fn exponentiate_impl<B1: ExponentiateOperand, B2: ExponentiateOperand>(
        base: &B1,
        exponent: &B2,
    ) -> Result<ImplResult, BigIntError> {
        if exponent.sign() {
            return Err(BigIntError::NegativeExponent);
        }

        // 2. If base is 0n and exponent is 0n, return 1n.
        if exponent.is_zero() {
            return Ok(ImplResult::Heap(JSBigInt::create_from_i32(1)?));
        }

        // 3. Return a BigInt representing the mathematical value of base raised
        //    to the power exponent.
        if base.is_zero() {
            return Ok(base.to_impl_result());
        }

        if base.length() == 1 && base.digit(0) == 1 {
            // (-1) ** even_number == 1.
            if base.sign() && (exponent.digit(0) & 1) == 0 {
                return JSBigInt::unary_minus_impl(base);
            }

            // (-1) ** odd_number == -1; 1 ** anything == 1.
            return Ok(base.to_impl_result());
        }

        // For all bases >= 2, very large exponents would lead to unrepresentable
        // results.
        const { assert!((MAX_LENGTH_BITS as u64) < Digit::MAX) };
        if exponent.length() > 1 {
            return Err(BigIntError::TooBig);
        }

        let exp_value: Digit = exponent.digit(0);
        if exp_value == 1 {
            return Ok(base.to_impl_result());
        }
        if exp_value >= MAX_LENGTH_BITS as Digit {
            return Err(BigIntError::TooBig);
        }

        const { assert!(MAX_LENGTH_BITS <= MAX_INT as u32) };
        let mut n: i32 = exp_value as i32;
        if base.length() == 1 && base.digit(0) == 2 {
            // Fast path for 2^n.
            let needed_digits = 1 + (n / DIGIT_BITS as i32);

            let mut result: Vec<Digit> = try_zeroed_digits(needed_digits as usize)?;

            // All bits are zero. Now set the n-th bit.
            let msd: Digit = (1 as Digit) << (n % DIGIT_BITS as i32);
            result[(needed_digits - 1) as usize] = msd;

            // Result is negative for odd powers of -2n.
            let mut sign = false;
            if base.sign() {
                sign = (n & 1) != 0;
            }
            return Ok(ImplResult::Heap(JSBigInt::try_create_from_impl(sign, &result)?));
        }

        let mut result: Option<JSBigInt> = None;
        let mut running_square: JSBigInt = base.to_heap_big_int()?;

        // This implicitly sets the result's sign correctly.
        if (n & 1) != 0 {
            result = Some(base.to_heap_big_int()?);
        }

        n >>= 1;
        while n != 0 {
            let temp = JSBigInt::multiply_impl(
                &HeapBigIntImpl::new(&running_square),
                &HeapBigIntImpl::new(&running_square),
            )?;
            let ImplResult::Heap(maybe_result) = temp else {
                unreachable!("multiplyImpl de dois BigInt de heap devolve BigInt de heap");
            };
            running_square = maybe_result;
            if (n & 1) != 0 {
                match &result {
                    None => result = Some(running_square.clone()),
                    Some(current) => {
                        let temp = JSBigInt::multiply_impl(
                            &HeapBigIntImpl::new(current),
                            &HeapBigIntImpl::new(&running_square),
                        )?;
                        let ImplResult::Heap(maybe_result) = temp else {
                            unreachable!("multiplyImpl de dois BigInt de heap devolve BigInt de heap");
                        };
                        result = Some(maybe_result);
                    }
                }
            }
            n >>= 1;
        }

        // `{ result }`: o laço sempre roda ao menos uma vez aqui (expValue >= 2), então há resultado.
        Ok(match result {
            Some(big_int) => ImplResult::Heap(big_int),
            None => ImplResult::Empty,
        })
    }

    /// `JSBigInt::exponentiate(JSGlobalObject*, JSBigInt*, JSBigInt*)`
    pub fn exponentiate(base: &JSBigInt, exponent: &JSBigInt) -> Result<ImplResult, BigIntError> {
        JSBigInt::exponentiate_impl(&HeapBigIntImpl::new(base), &HeapBigIntImpl::new(exponent))
            .map(try_convert_to_big_int32)
    }

    /// `JSBigInt::exponentiate(JSGlobalObject*, JSBigInt*, int32_t)`
    pub fn exponentiate_heap_i32(base: &JSBigInt, exponent: i32) -> Result<ImplResult, BigIntError> {
        JSBigInt::exponentiate_impl(&HeapBigIntImpl::new(base), &Int32BigIntImpl::new(exponent))
            .map(try_convert_to_big_int32)
    }

    /// `JSBigInt::exponentiate(JSGlobalObject*, int32_t, JSBigInt*)`
    pub fn exponentiate_i32_heap(base: i32, exponent: &JSBigInt) -> Result<ImplResult, BigIntError> {
        JSBigInt::exponentiate_impl(&Int32BigIntImpl::new(base), &HeapBigIntImpl::new(exponent))
            .map(try_convert_to_big_int32)
    }

    /// `JSBigInt::exponentiate(JSGlobalObject*, int32_t, int32_t)`
    pub fn exponentiate_i32_i32(base: i32, exponent: i32) -> Result<ImplResult, BigIntError> {
        JSBigInt::exponentiate_impl(&Int32BigIntImpl::new(base), &Int32BigIntImpl::new(exponent))
            .map(try_convert_to_big_int32)
    }
}

/// `TwoDigit` (`UInt128` em `CPU(REGISTER64)`).
type TwoDigit = u128;

/// `DigitColumnAccumulator<carryForm>`: `CARRY_VALUE == false` é `CarryForm::Flags` e `true` é
/// `CarryForm::Value` (o enum do C++ vira `const bool`, porque parâmetro const de enum é instável).
/// As duas formas calculam o mesmo valor; a diferença no C++ é só a sequência de instruções.
#[derive(Default)]
struct DigitColumnAccumulator<const CARRY_VALUE: bool> {
    t0: Digit,
    t1: Digit,
    t2: Digit,
}

impl<const CARRY_VALUE: bool> DigitColumnAccumulator<CARRY_VALUE> {
    /// `addCarrying`: devolve a soma e o vai-um de saída.
    #[inline(always)]
    fn add_carrying(a: Digit, b: Digit, carry_in: Digit) -> (Digit, Digit) {
        let (sum, carry0) = a.overflowing_add(b);
        let (result, carry1) = sum.overflowing_add(carry_in);
        (result, (carry0 | carry1) as Digit)
    }

    #[inline(always)]
    fn mac(&mut self, a: Digit, b: Digit) {
        let prod: TwoDigit = (a as TwoDigit) * (b as TwoDigit);
        if CARRY_VALUE {
            let (t0, carry) = Self::add_carrying(self.t0, prod as Digit, 0);
            self.t0 = t0;
            let (t1, carry) = Self::add_carrying(self.t1, (prod >> DIGIT_BITS) as Digit, carry);
            self.t1 = t1;
            self.t2 = self.t2.wrapping_add(carry);
            return;
        }
        let sum0: TwoDigit = (self.t0 as TwoDigit) + (prod as Digit) as TwoDigit;
        self.t0 = sum0 as Digit;
        let sum1: TwoDigit = (self.t1 as TwoDigit)
            + ((prod >> DIGIT_BITS) as Digit) as TwoDigit
            + ((sum0 >> DIGIT_BITS) as Digit) as TwoDigit;
        self.t1 = sum1 as Digit;
        self.t2 = self.t2.wrapping_add((sum1 >> DIGIT_BITS) as Digit);
    }

    /// Acumula `2 * a * b`. O produto dobrado precisa de um bit a mais que os dois dígitos de
    /// `prod`, e esse bit cai em `t2`, o dígito acima do par.
    #[inline(always)]
    fn mac_doubled(&mut self, a: Digit, b: Digit) {
        let prod: TwoDigit = (a as TwoDigit) * (b as TwoDigit);
        let high = (prod >> (DIGIT_BITS * 2 - 1)) as Digit;
        let doubled: TwoDigit = prod << 1;
        if CARRY_VALUE {
            let (t0, carry) = Self::add_carrying(self.t0, doubled as Digit, 0);
            self.t0 = t0;
            let (t1, carry) = Self::add_carrying(self.t1, (doubled >> DIGIT_BITS) as Digit, carry);
            self.t1 = t1;
            self.t2 = self.t2.wrapping_add(carry.wrapping_add(high));
            return;
        }
        let sum0: TwoDigit = (self.t0 as TwoDigit) + (doubled as Digit) as TwoDigit;
        self.t0 = sum0 as Digit;
        let sum1: TwoDigit = (self.t1 as TwoDigit)
            + ((doubled >> DIGIT_BITS) as Digit) as TwoDigit
            + ((sum0 >> DIGIT_BITS) as Digit) as TwoDigit;
        self.t1 = sum1 as Digit;
        self.t2 = self.t2.wrapping_add(((sum1 >> DIGIT_BITS) as Digit).wrapping_add(high));
    }

    #[inline(always)]
    fn store_and_shift(&mut self) -> Digit {
        let result = self.t0;
        self.t0 = self.t1;
        self.t1 = self.t2;
        self.t2 = 0;
        result
    }

    #[inline(always)]
    fn low(&self) -> Digit {
        self.t0
    }

    /// Verdadeiro quando a soma corrente cabe no único dígito que `low()` devolve, de que os
    /// chamadores dependem na última coluna.
    #[inline(always)]
    fn fits_in_low(&self) -> bool {
        self.t1 == 0 && self.t2 == 0
    }
}

/// `CombaAccumulator<N>`. O C++ desenrola as colunas em tempo de compilação com `if constexpr`;
/// aqui os mesmos limites viram laços sobre `N` constante.
#[derive(Default)]
struct CombaAccumulator<const N: usize> {
    accumulator: DigitColumnAccumulator<true>,
}

impl<const N: usize> CombaAccumulator<N> {
    #[inline(always)]
    fn compute_column(&mut self, k: usize, a: &[Digit; N], b: &[Digit; N]) {
        for i in 0..N {
            let j = k as isize - i as isize;
            if j >= 0 && j < N as isize {
                self.accumulator.mac(a[i], b[j as usize]);
            }
        }
    }

    #[inline(always)]
    fn pass(&mut self, r: &mut [Digit], a: &[Digit; N], b: &[Digit; N]) {
        debug_assert!(r.len() == N * 2);
        for k in 0..(2 * N - 1) {
            self.compute_column(k, a, b);
            r[k] = self.accumulator.store_and_shift();
        }
        debug_assert!(self.accumulator.fits_in_low());
        r[N * 2 - 1] = self.accumulator.low();
    }

    /// Coluna `K` de `a * a`. Os pares `(I, J)` e `(J, I)` contribuem o mesmo produto, então ele é
    /// acumulado uma vez com o dobro do peso, reduzindo as multiplicações a `N * (N + 1) / 2`.
    #[inline(always)]
    fn compute_square_column(&mut self, k: usize, a: &[Digit; N]) {
        for i in 0..N {
            let j = k as isize - i as isize;
            if j >= i as isize && j < N as isize {
                if j == i as isize {
                    self.accumulator.mac(a[i], a[i]);
                } else {
                    self.accumulator.mac_doubled(a[i], a[j as usize]);
                }
            }
        }
    }

    #[inline(always)]
    fn square_pass(&mut self, r: &mut [Digit], a: &[Digit; N]) {
        debug_assert!(r.len() == N * 2);
        for k in 0..(2 * N - 1) {
            self.compute_square_column(k, a);
            r[k] = self.accumulator.store_and_shift();
        }
        debug_assert!(self.accumulator.fits_in_low());
        r[N * 2 - 1] = self.accumulator.low();
    }
}

impl JSBigInt {
    /// `JSBigInt::multiplyCombaFixed<N>`; `result` tem `N * 2` dígitos.
    pub fn multiply_comba_fixed<'a, const N: usize>(
        x: &[Digit; N],
        y: &[Digit; N],
        result: &'a mut [Digit],
    ) -> &'a mut [Digit] {
        const { assert!(N == 1 || N == 2 || N == 4 || N == 8 || N == MAX_COMBA_FIXED_SIZE) };
        assert!(result.len() == N * 2);
        // Ensure that all loads are done before entering to computation.
        let a: [Digit; N] = *x;
        let b: [Digit; N] = *y;

        let mut acc = CombaAccumulator::<N>::default();
        acc.pass(result, &a, &b);
        result
    }

    /// `JSBigInt::squareCombaFixed<N>`; `result` tem `N * 2` dígitos.
    pub fn square_comba_fixed<'a, const N: usize>(x: &[Digit; N], result: &'a mut [Digit]) -> &'a mut [Digit] {
        const { assert!(N == 1 || N == 2 || N == 4 || N == 8 || N == MAX_COMBA_FIXED_SIZE) };
        assert!(result.len() == N * 2);
        // Ensure that all loads are done before entering to computation.
        let a: [Digit; N] = *x;

        let mut acc = CombaAccumulator::<N>::default();
        acc.square_pass(result, &a);
        result
    }

    /// `JSBigInt::multiplySingle`
    pub fn multiply_single<'a>(multiplicand: &[Digit], multiplier: Digit, result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(result.len() > multiplicand.len());
        let mut carry: Digit = 0;
        let mut high: Digit = 0;
        let mut i = 0usize;
        while i < multiplicand.len() {
            let (low, new_high) = digit_mul(multiplicand[i], multiplier);
            let mut new_carry: Digit = 0;
            result[i] = digit_add3(low, high, carry, &mut new_carry);
            high = new_high;
            carry = new_carry;
            i += 1;
        }
        result[i] = carry.wrapping_add(high);
        i += 1;
        &mut result[..i]
    }

    /// `MULTIPLY_BODY(min, max)`: soma em `zi` e `next` os produtos dos dígitos relevantes de
    /// `x` e `y` e grava `result[i] = zi`.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn multiply_body(
        x: &[Digit],
        y: &[Digit],
        result: &mut [Digit],
        i: usize,
        min: usize,
        max: usize,
        zi: &mut Digit,
        next: &mut Digit,
        carry: &mut Digit,
        next_carry: &mut Digit,
    ) {
        for j in min..=max {
            let (low, high) = digit_mul(x[j], y[i - j]);
            *zi = digit_add(*zi, low, carry);
            *next = digit_add(*next, high, next_carry);
        }
        result[i] = *zi;
    }

    /// `JSBigInt::multiplySchoolbook`: Z := X * Y.
    /// Multiplicação "schoolbook" O(n²), otimizada para minimizar as checagens de limite e de
    /// estouro: em vez de percorrer X para cada dígito de Y, percorre-se Z. Esta função é
    /// *muito* sensível a desempenho, pois é o caso base das recursões dos algoritmos avançados.
    pub fn multiply_schoolbook<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(x.len() >= y.len());
        assert!(result.len() >= x.len() + y.len());
        assert!(!x.is_empty());
        assert!(!y.is_empty());

        let mut next: Digit = 0;
        let mut next_carry: Digit = 0;
        let mut carry: Digit = 0;
        // Unrolled first iteration: it's trivial.
        {
            let (low, high) = digit_mul(x[0], y[0]);
            result[0] = low;
            next = high;
        }
        let mut i = 1usize;
        // Unrolled second iteration: a little less setup.
        if i < y.len() {
            let mut zi: Digit = next;
            next = 0;
            JSBigInt::multiply_body(x, y, result, i, 0, 1, &mut zi, &mut next, &mut carry, &mut next_carry);
            i += 1;
        }

        // Main part: since xSpan.size() >= ySpan.size() > i, no bounds checks are needed.
        while i < y.len() {
            let mut temp: Digit = 0;
            let mut zi = digit_add(next, carry, &mut temp);
            next = next_carry.wrapping_add(temp);
            carry = 0;
            next_carry = 0;
            JSBigInt::multiply_body(x, y, result, i, 0, i, &mut zi, &mut next, &mut carry, &mut next_carry);
            i += 1;
        }

        // Last part: i exceeds y now, we have to be careful about bounds.
        let loop_end = x.len() + y.len() - 2;
        while i <= loop_end {
            let max_x_index = i.min(x.len() - 1);
            let max_y_index = y.len() - 1;
            let min_x_index = i - max_y_index;
            let mut temp: Digit = 0;
            let mut zi = digit_add(next, carry, &mut temp);
            next = next_carry.wrapping_add(temp);
            carry = 0;
            next_carry = 0;
            JSBigInt::multiply_body(
                x, y, result, i, min_x_index, max_x_index, &mut zi, &mut next, &mut carry, &mut next_carry,
            );
            i += 1;
        }

        // Write the last digit.
        let mut temp: Digit = 0;
        result[i] = digit_add(next, carry, &mut temp);
        i += 1;
        debug_assert!(temp == 0);
        &mut result[..i]
    }

    /// `JSBigInt::multiplySpecialLow`: para o `cachedMod`, calcula só os `result.len()` dígitos
    /// baixos de X * Y.
    pub fn multiply_special_low(x: &[Digit], y: &[Digit], result: &mut [Digit]) {
        assert!(y.len() >= 1);
        assert!(x.len() >= 2);
        assert!(x.len() >= y.len() - 1);
        assert!(!result.is_empty());

        let mut accumulator = DigitColumnAccumulator::<false>::default();
        let last_column = result.len() - 1;
        let main_end = x.len().min(y.len()).min(last_column);
        let mut column = 0usize;

        // Expanding phase: both operands still cover the whole column, so the term range is exactly
        // [0, column] and needs no clamping.
        while column < main_end {
            for j in 0..=column {
                accumulator.mac(x[j], y[column - j]);
            }
            result[column] = accumulator.store_and_shift();
            column += 1;
        }

        // Shrinking phase: the term range is clipped at both ends.
        while column <= last_column {
            let max_y_index = column.min(y.len() - 1);
            let min_x_index = column - max_y_index;
            let max_x_index = column.min(x.len() - 1);
            for j in min_x_index..=max_x_index {
                accumulator.mac(x[j], y[column - j]);
            }
            result[column] = accumulator.store_and_shift();
            column += 1;
        }
    }

    /// `JSBigInt::multiplySpecialHigh`: para o `cachedMod`, calcula só os dígitos do produto de
    /// `start_position` em diante. `result[start_position]` corresponde ao dígito `start_position`
    /// do produto. O estado do acumulador das posições abaixo se perde, então os dígitos são um
    /// valor *aproximado*.
    pub fn multiply_special_high(x: &[Digit], y: &[Digit], result: &mut [Digit], start_position: usize) {
        assert!(x.len() >= y.len());
        assert!(y.len() >= 1);
        let full_size = x.len() + y.len();
        assert!(start_position < full_size);
        assert!(result.len() >= full_size);

        let mut accumulator = DigitColumnAccumulator::<false>::default();
        let mut column = start_position;

        // Expanding phase: column < ySpan.size(), so the term range starts at 0.
        while column < y.len() {
            for j in 0..=column {
                accumulator.mac(x[j], y[column - j]);
            }
            result[column] = accumulator.store_and_shift();
            column += 1;
        }

        // Shrinking phase: the term range is clipped at both ends.
        let last_column = full_size - 2;
        while column <= last_column {
            let min_x_index = column - (y.len() - 1);
            let max_x_index = column.min(x.len() - 1);
            for j in min_x_index..=max_x_index {
                accumulator.mac(x[j], y[column - j]);
            }
            result[column] = accumulator.store_and_shift();
            column += 1;
        }

        debug_assert!(accumulator.fits_in_low());
        result[column] = accumulator.low();
    }

    /// `JSBigInt::multiplyComba`: Z := X * Y por varredura de produtos. A soma corrente de cada
    /// coluna vive em três dígitos, então cada produto alimenta uma única cadeia de soma com
    /// vai-um. Dividir a caminhada em rampa de subida, regime e rampa de descida torna todo limite
    /// de laço exato. Ver `should_use_comba`.
    pub fn multiply_comba<'a>(x: &[Digit], y: &[Digit], result: &'a mut [Digit]) -> &'a mut [Digit] {
        assert!(x.len() >= y.len());
        assert!(result.len() >= x.len() + y.len());
        assert!(!y.is_empty());

        let x_size = x.len();
        let y_size = y.len();

        let mut accumulator = DigitColumnAccumulator::<false>::default();
        for i in 0..y_size {
            for j in 0..=i {
                accumulator.mac(x[j], y[i - j]);
            }
            result[i] = accumulator.store_and_shift();
        }
        for i in y_size..x_size {
            for j in (i - y_size + 1)..=i {
                accumulator.mac(x[j], y[i - j]);
            }
            result[i] = accumulator.store_and_shift();
        }
        for i in x_size..(x_size + y_size - 1) {
            for j in (i - y_size + 1)..=(x_size - 1) {
                accumulator.mac(x[j], y[i - j]);
            }
            result[i] = accumulator.store_and_shift();
        }
        debug_assert!(accumulator.fits_in_low());
        result[x_size + y_size - 1] = accumulator.low();
        &mut result[..x_size + y_size]
    }

    /// `JSBigInt::multiplySpecialHighFixed<XSize, YSize, StartPosition>`: forma especializada em
    /// tempo de compilação de `multiply_special_high` para o `cachedMod`. Precisa acumular
    /// exatamente na mesma ordem da genérica (a cota de erro do laço corretivo do `cachedMod`
    /// depende dessa ordem). `result` tem `XSIZE + YSIZE` dígitos.
    #[inline(always)]
    pub fn multiply_special_high_fixed<const XSIZE: usize, const YSIZE: usize, const START_POSITION: usize>(
        x: &[Digit; XSIZE],
        y: &[Digit; YSIZE],
        result: &mut [Digit],
    ) {
        const { assert!(XSIZE >= YSIZE && YSIZE >= 1) };
        const { assert!(START_POSITION < XSIZE + YSIZE) };
        assert!(result.len() == XSIZE + YSIZE);
        let loop_end: usize = XSIZE + YSIZE - 2;

        let mut acc = DigitColumnAccumulator::<false>::default();
        for i in START_POSITION..=loop_end {
            let min_x_index = if i < YSIZE { 0 } else { i - (YSIZE - 1) };
            multiply_special_column(x, y, i, min_x_index, i.min(XSIZE - 1), &mut acc);
            result[i] = acc.store_and_shift();
        }

        debug_assert!(acc.fits_in_low());
        result[loop_end + 1] = acc.low();
    }

    /// `JSBigInt::multiplySpecialLowFixed<XSize, YSize, RSize>`; `result` tem `RSIZE` dígitos.
    #[inline(always)]
    pub fn multiply_special_low_fixed<const XSIZE: usize, const YSIZE: usize, const RSIZE: usize>(
        x: &[Digit; XSIZE],
        y: &[Digit; YSIZE],
        result: &mut [Digit],
    ) {
        const { assert!(XSIZE >= 2 && YSIZE >= 1 && RSIZE >= 2) };
        const { assert!(XSIZE + 1 >= YSIZE) };
        assert!(result.len() == RSIZE);
        let loop_end: usize = RSIZE - 1;
        let main_end: usize = XSIZE.min(YSIZE).min(loop_end);

        let mut acc = DigitColumnAccumulator::<false>::default();
        let mut i = 0usize;

        // Expanding phase: both operands still cover the whole column, so the term range is exactly
        // [0, i] and needs no clamping.
        while i < main_end {
            multiply_special_column(x, y, i, 0, i, &mut acc);
            result[i] = acc.store_and_shift();
            i += 1;
        }

        // Shrinking phase: the term range is clipped at both ends.
        while i <= loop_end {
            let max_y_index = i.min(YSIZE - 1);
            multiply_special_column(x, y, i, i - max_y_index, i.min(XSIZE - 1), &mut acc);
            result[i] = acc.store_and_shift();
            i += 1;
        }
    }
}

/// `minCombaSmallerSize`, `maxCombaThinSmallerSize`, `minCombaThinLargerSize`. Os limites são
/// grosseiros de propósito (ver o comentário longo do C++): só valem os que mantiveram o sinal nas
/// quatro medições, então não se estreitam sem medir de novo do mesmo jeito.
const MIN_COMBA_SMALLER_SIZE: usize = 8;
const MAX_COMBA_THIN_SMALLER_SIZE: usize = 2;
const MIN_COMBA_THIN_LARGER_SIZE: usize = 16;

/// `shouldUseComba`
pub const fn should_use_comba(larger_size: usize, smaller_size: usize) -> bool {
    smaller_size >= MIN_COMBA_SMALLER_SIZE
        || (smaller_size <= MAX_COMBA_THIN_SMALLER_SIZE && larger_size >= MIN_COMBA_THIN_LARGER_SIZE)
}

/// `multiplySpecialColumn`: acumula `x[j] * y[i - j]` para `j` em `[min, max]` em `acc`.
#[inline(always)]
fn multiply_special_column(
    x: &[Digit],
    y: &[Digit],
    i: usize,
    min: usize,
    max: usize,
    acc: &mut DigitColumnAccumulator<false>,
) {
    for j in min..=max {
        acc.mac(x[j], y[i - j]);
    }
}

/// `JSBigInt::InterruptCheck`. Os algoritmos sub-quadráticos podem rodar por segundos nas maiores
/// entradas, então atendem a um pedido de término a cada alguns milhões de multiplicações de
/// dígito. Quem não tem objeto global (não pode lançar) nunca interrompe.
/// FATIA3: o `VM*` ainda não existe; `vm_check` faz o papel de
/// `vm->hasExceptionsAfterHandlingTraps()` (`None` equivale a `m_vm == nullptr`).
pub struct InterruptCheck<'a> {
    vm_check: Option<&'a mut dyn FnMut() -> bool>,
    work: usize,
    interrupted: bool,
}

impl<'a> InterruptCheck<'a> {
    const WORK_THRESHOLD: usize = 5_000_000;

    pub fn new(vm_check: Option<&'a mut dyn FnMut() -> bool>) -> InterruptCheck<'a> {
        InterruptCheck { vm_check, work: 0, interrupted: false }
    }

    #[inline(always)]
    pub fn add_work(&mut self, units: usize) {
        self.work += units;
        if self.work >= Self::WORK_THRESHOLD {
            self.check_slow();
        }
    }

    #[inline(always)]
    pub fn interrupted(&self) -> bool {
        self.interrupted
    }

    fn check_slow(&mut self) {
        self.work = 0;
        let Some(vm_check) = self.vm_check.as_mut() else {
            return;
        };
        // Isto trata cada trap assíncrono como um RETURN_IF_EXCEPTION, então um pedido de término
        // deixa a TerminationException pendente.
        if vm_check() {
            self.interrupted = true;
        }
    }
}

// Multiplicação de Karatsuba, portada do V8, que por sua vez se baseia no math/big do Go.
//
// O limiar é o tamanho do menor operando, medido contra o caso base Comba (o schoolbook do V8
// cruza em 34). Formas balanceadas ganham a partir de 40 dígitos, mas para um x longo
// `karatsuba_length` arredonda um menor ímpar para cima em um dígito, e em 41 ou 43 dígitos esse
// preenchimento custa os 2-3% que o Karatsuba ganharia. Em 44 nenhuma forma medida regride.
pub const KARATSUBA_THRESHOLD: usize = 44;

/// `karatsubaRoundUpLength`
fn karatsuba_round_up_length(length: usize) -> usize {
    if length <= 36 {
        // `roundUpToMultipleOf<2>`
        return (length + 1) & !1usize;
    }
    let mut shift: u32 = usize::BITS - length.leading_zeros() - 5;
    if (length >> shift) >= 0x18 {
        shift += 1;
    }
    let additive: usize = (1usize << shift) - 1;
    if shift >= 2 && (length & additive) < (1usize << (shift - 2)) {
        return length;
    }
    ((length + additive) >> shift) << shift
}

/// `karatsubaLength`
pub fn karatsuba_length(n: usize) -> usize {
    let mut n = karatsuba_round_up_length(n);
    let mut i: u32 = 0;
    while n > KARATSUBA_THRESHOLD {
        n >>= 1;
        i += 1;
    }
    n << i
}

/// `clampedSubspan` (fatia de leitura).
pub fn clamped_subspan<D>(x: &[D], offset: usize, length: usize) -> &[D] {
    if offset >= x.len() {
        return &[];
    }
    &x[offset..offset + length.min(x.len() - offset)]
}

/// `clampedSubspan` (fatia mutável).
pub fn clamped_subspan_mut<D>(x: &mut [D], offset: usize, length: usize) -> &mut [D] {
    if offset >= x.len() {
        return &mut [];
    }
    let end = offset + length.min(x.len() - offset);
    &mut x[offset..end]
}
