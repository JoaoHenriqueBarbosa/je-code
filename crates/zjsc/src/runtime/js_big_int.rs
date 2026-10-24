//! Porte de `JavaScriptCore/runtime/JSBigInt.h` e das linhas 1 a 631 de `JSBigInt.cpp`
//! (primeira fatia: tipos, constantes, criação, conversões de e para palavras, `bitLength`,
//! os adaptadores `HeapBigIntImpl`/`Int32BigIntImpl`/`Int64BigIntImpl`).
//!
//! Modelo de dados provisório: o `JSBigInt` do C++ é uma célula do heap; aqui é uma struct com
//! `sign` e `digits` (o `perCellBit` do C++ vira o campo `sign`, e `m_length` é `digits.len()`).
//! A integração com o heap (`CellId`, `JSValue`, `VM`) vem depois. Onde o C++ lança exceção por
//! `JSGlobalObject*`/`VM&`, devolve-se `Result<_, BigIntError>`; onde devolve `nullptr` sem
//! `JSGlobalObject*` (família `try*`), devolve-se `Option`.

use crate::wtf::math_extras::negate;

/// `JSBigInt::Digit` (`UCPURegister`, 64 bits em Linux x86_64).
pub type Digit = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JSBigIntComparisonMode {
    LessThan,
    LessThanOrEqual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JSBigIntComparisonResult {
    Equal,
    Undefined,
    GreaterThan,
    LessThan,
}

/// `JSBigInt::ComparisonMode` e `JSBigInt::ComparisonResult` (aliases do C++).
pub type ComparisonMode = JSBigIntComparisonMode;
pub type ComparisonResult = JSBigIntComparisonResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationType {
    None,
    WithZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorParseMode {
    ThrowExceptions,
    IgnoreExceptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseIntMode {
    DisallowEmptyString,
    AllowEmptyString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseIntSign {
    Unsigned,
    Signed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingResult {
    RoundDown,
    Tie,
    RoundUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtraDigitsHandling {
    Copy,
    Skip,
}

/// Erros que o C++ lança por `throwOutOfMemoryError`/`throwRangeError` em `JSBigInt.cpp`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BigIntError {
    /// `throwOutOfMemoryError(globalObject, scope)` sem mensagem (o padrão de
    /// `createOutOfMemoryError` é "Out of memory"; conferido em `Error.cpp:417`, um RangeError).
    OutOfMemory,
    /// `throwOutOfMemoryError(..., "BigInt generated from this operation is too big"_s)`.
    TooBig,
    /// `throwRangeError(..., "Negative exponent is not allowed"_s)` (usado a partir da linha 638).
    NegativeExponent,
    /// `throwRangeError(..., "0 is an invalid divisor value."_s)` de `divideImpl` e `remainderImpl`.
    InvalidDivisor,
}

impl BigIntError {
    /// Mensagem exata do C++.
    pub fn message(&self) -> &'static str {
        match self {
            BigIntError::OutOfMemory => "Out of memory",
            BigIntError::TooBig => "BigInt generated from this operation is too big",
            BigIntError::NegativeExponent => "Negative exponent is not allowed",
            BigIntError::InvalidDivisor => "0 is an invalid divisor value.",
        }
    }
}

pub const BITS_PER_BYTE: u32 = 8;
pub const DIGIT_BITS: u32 = (core::mem::size_of::<Digit>() as u32) * BITS_PER_BYTE;
pub const HALF_DIGIT_BITS: u32 = DIGIT_BITS / 2;
pub const HALF_DIGIT_MASK: Digit = (1u64 << HALF_DIGIT_BITS) - 1;

pub const MAX_INT: i32 = 0x7FFF_FFFF;

pub const DOUBLE_MANTISSA_SIZE: u32 = 53;
/// Excluindo o hidden-bit.
pub const DOUBLE_PHYSICAL_MANTISSA_SIZE: u32 = 52;
pub const DOUBLE_PHYSICAL_MANTISSA_MASK: u64 = (1u64 << DOUBLE_PHYSICAL_MANTISSA_SIZE) - 1;
pub const DOUBLE_MANTISSA_HIDDEN_BIT: u64 = 1u64 << DOUBLE_PHYSICAL_MANTISSA_SIZE;

/// Até 1 << 30 bits (128MB de dígitos), o mesmo limite do V8.
pub const MAX_LENGTH_BITS: u32 = 1 << 30;
pub const MAX_LENGTH: u32 = MAX_LENGTH_BITS / DIGIT_BITS;
const _: () = assert!(MAX_LENGTH_BITS % DIGIT_BITS == 0);

pub const MAX_COMBA_FIXED_SIZE: usize = 16;

/// `Vec<Digit>` zerado de `length` dígitos: a falta de memória é `OutOfMemory`, nunca um abort. Todo
/// buffer de resultado (e de rascunho) de tamanho derivado de expoente, deslocamento ou texto passa aqui.
pub(crate) fn try_zeroed_digits(length: usize) -> Result<Vec<Digit>, BigIntError> {
    crate::runtime::fallible_alloc::try_filled_vec(0, length).ok_or(BigIntError::OutOfMemory)
}

pub const MAX_CACHED_MOD_DIVISOR_SIZE: u32 = 32;
pub const MAX_FIXED_CACHED_MOD_DIVISOR_SIZE: u32 = 4;
pub const MAX_IN_PLACE_SUB_SIZE: u32 = 16;
pub const MAX_IN_PLACE_CACHED_MOD_SIZE: u32 = 8;
const _: () = assert!(MAX_IN_PLACE_CACHED_MOD_SIZE <= MAX_CACHED_MOD_DIVISOR_SIZE);
const _: () = assert!(MAX_FIXED_CACHED_MOD_DIVISOR_SIZE <= MAX_CACHED_MOD_DIVISOR_SIZE);

/// `JSBigInt::flip`.
pub fn flip(result: ComparisonResult) -> ComparisonResult {
    match result {
        ComparisonResult::LessThan => ComparisonResult::GreaterThan,
        ComparisonResult::GreaterThan => ComparisonResult::LessThan,
        ComparisonResult::Equal | ComparisonResult::Undefined => result,
    }
}

/// `invertBigIntCompareResult`.
pub fn invert_big_int_compare_result(result: ComparisonResult) -> ComparisonResult {
    match result {
        ComparisonResult::GreaterThan => ComparisonResult::LessThan,
        ComparisonResult::LessThan => ComparisonResult::GreaterThan,
        other => other,
    }
}

/// `normalize<D>(std::span<D>)`: remove os dígitos zero mais significativos.
pub fn normalize<D: Copy + PartialEq + Default>(mut x: &[D]) -> &[D] {
    while let Some(last) = x.last() {
        if *last != D::default() {
            break;
        }
        x = &x[..x.len() - 1];
    }
    x
}

/// Referência de célula do registro central (`CellEntry::BigInt`); o `JSBigInt` é imutável depois de criado.
pub type JSBigIntRef = std::rc::Rc<JSBigInt>;

/// `JSBigInt` (célula do heap no C++). `digits.len()` é o `m_length`; `sign` é o `perCellBit`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct JSBigInt {
    sign: bool,
    digits: Vec<Digit>,
    /// `m_hash`
    hash: u32,
    /// `JSCell::m_structureID`: a `vm.bigIntStructure`; `None` até a célula entrar no registro.
    structure: BigIntStructure,
}

/// `const ClassInfo JSBigInt::s_info`.
pub static BIG_INT_S_INFO: crate::runtime::class_info::ClassInfo =
    crate::runtime::class_info::ClassInfo { class_name: "BigInt", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// A `Structure` de um `JSBigInt`. Fora da igualdade: dois BigInts valem o mesmo pelos dígitos e pelo sinal.
#[derive(Clone, Debug, Default)]
pub struct BigIntStructure(Option<crate::runtime::structure::StructureRef>);

impl PartialEq for BigIntStructure {
    fn eq(&self, _other: &BigIntStructure) -> bool {
        true
    }
}

impl Eq for BigIntStructure {}

impl JSBigInt {
    /// `JSBigInt::createStructure(vm, globalObject, prototype)`: `TypeInfo(HeapBigIntType, StructureFlags)`.
    pub fn create_structure(
        vm: &crate::runtime::vm::VM,
        global_object: Option<&crate::runtime::js_global_object::JSGlobalObject>,
        prototype: crate::runtime::js_value::JSValue,
    ) -> crate::runtime::structure::StructureRef {
        use crate::runtime::js_type::JSType;
        use crate::runtime::js_type_info::{TypeInfo, STRUCTURE_IS_IMMORTAL};
        crate::runtime::structure::Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::HeapBigIntType, STRUCTURE_IS_IMMORTAL),
            &BIG_INT_S_INFO,
        )
    }

    /// `JSCell::structure()`.
    pub fn structure(&self) -> Option<&crate::runtime::structure::StructureRef> {
        self.structure.0.as_ref()
    }

    /// Dá à célula a `vm.bigIntStructure` (o C++ a grava no `JSCell` ao alocar).
    pub fn with_structure(mut self, vm: &crate::runtime::vm::VM) -> JSBigInt {
        self.structure = BigIntStructure(Some(vm.big_int_structure()));
        self
    }
}

impl JSBigInt {
    /// `JSBigInt::allocationSize`, sem o cabeçalho da célula (`offsetOfData` depende do layout do
    /// heap, que não existe neste modelo). FATIA2: ajustar quando o heap por índice entrar.
    pub const fn allocation_size(length: u32) -> usize {
        length as usize * core::mem::size_of::<Digit>()
    }

    /// `JSBigInt::initialize`
    pub fn initialize(&mut self, init_type: InitializationType) {
        if init_type == InitializationType::WithZero {
            self.digits.fill(0);
        }
    }

    // FATIA2: `createStructure` (Structure, TypeInfo(HeapBigIntType, StructureFlags), info()) e
    // `s_info` (ClassInfo) dependem de Structure/VM, ainda não portados.

    // `createZero(VM&)` e `tryCreateZero(VM&)` são `JSBigInt::default()`. O C++ devolve a célula
    // cacheada em `vm.heapBigIntConstantZero`; FATIA2: apontar para o zero do VM quando o heap existir.

    /// `createWithLength(JSGlobalObject* nullOrGlobalObjectForOOM, VM&, unsigned)`. O C++ lança
    /// só se `nullOrGlobalObjectForOOM` existe; as variantes `try_*` descartam o erro.
    pub fn create_with_length(length: u32) -> Result<JSBigInt, BigIntError> {
        if length > MAX_LENGTH {
            return Err(BigIntError::TooBig);
        }
        // O C++ deixa os dígitos sem inicializar; aqui nascem zerados (`initialize` os zera de novo).
        Ok(JSBigInt { sign: false, digits: try_zeroed_digits(length as usize)?, hash: 0, structure: BigIntStructure::default() })
    }

    /// `JSBigInt::tryCreateWithLength(VM&, unsigned)`
    pub fn try_create_with_length(length: u32) -> Option<JSBigInt> {
        JSBigInt::create_with_length(length).ok()
    }

    /// `createFrom(JSGlobalObject*, VM&, int32_t)` (o `nullOrGlobalObjectForOOM` vira o `Result`).
    /// Cobre também `createFrom(JSGlobalObject*, int32_t)` e `tryCreateFrom(VM&, int32_t)`.
    pub fn create_from_i32(value: i32) -> Result<JSBigInt, BigIntError> {
        if value == 0 {
            return Ok(JSBigInt::default());
        }
        let mut big_int = JSBigInt::create_with_length(1)?;
        if value < 0 {
            big_int.set_digit(0, (-1 * (value as i64)) as Digit);
            big_int.set_sign(true);
        } else {
            big_int.set_digit(0, value as Digit);
        }
        Ok(big_int)
    }

    /// `JSBigInt::tryCreateFrom(VM&, int32_t)`
    pub fn try_create_from_i32(value: i32) -> Option<JSBigInt> {
        JSBigInt::create_from_i32(value).ok()
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, uint32_t)`
    pub fn create_from_u32(value: u32) -> Result<JSBigInt, BigIntError> {
        if value == 0 {
            return Ok(JSBigInt::default());
        }
        let mut big_int = JSBigInt::create_with_length(1)?;
        big_int.set_digit(0, value as Digit);
        Ok(big_int)
    }

    /// `JSBigInt::tryCreateFromImpl(JSGlobalObject*, uint64_t value, bool sign)`
    fn try_create_from_u64_impl(value: u64, sign: bool) -> Result<JSBigInt, BigIntError> {
        if value == 0 {
            return Ok(JSBigInt::default());
        }
        // `sizeof(Digit) == 8`: o ramo de dois dígitos de 32 bits não existe neste alvo.
        let mut big_int = JSBigInt::create_with_length(1)?;
        big_int.set_digit(0, value as Digit);
        big_int.set_sign(sign);
        Ok(big_int)
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, uint64_t)`
    pub fn create_from_u64(value: u64) -> Result<JSBigInt, BigIntError> {
        JSBigInt::try_create_from_u64_impl(value, false)
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, int64_t)`
    pub fn create_from_i64(value: i64) -> Result<JSBigInt, BigIntError> {
        let unsigned_value: u64;
        let mut sign = false;
        if value < 0 {
            unsigned_value = ((-(value + 1)) as u64) + 1;
            sign = true;
        } else {
            unsigned_value = value as u64;
        }
        JSBigInt::try_create_from_u64_impl(unsigned_value, sign)
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, Int128)`
    pub fn create_from_i128(value: i128) -> Result<JSBigInt, BigIntError> {
        if value == 0 {
            return Ok(JSBigInt::default());
        }

        let unsigned_value: u128;
        let mut sign = false;
        if value < 0 {
            unsigned_value = ((-(value + 1)) as u128) + 1;
            sign = true;
        } else {
            unsigned_value = value as u128;
        }

        if unsigned_value <= u64::MAX as u128 {
            return JSBigInt::try_create_from_u64_impl(unsigned_value as u64, sign);
        }

        // `sizeof(Digit) == 8`
        let mut big_int = JSBigInt::create_with_length(2)?;
        let low_bits = unsigned_value as u64 as Digit;
        let high_bits = (unsigned_value >> 64) as u64 as Digit;
        debug_assert!(high_bits != 0);
        big_int.set_digit(0, low_bits);
        big_int.set_digit(1, high_bits);
        big_int.set_sign(sign);
        Ok(big_int)
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, bool)`
    pub fn create_from_bool(value: bool) -> Result<JSBigInt, BigIntError> {
        if !value {
            return Ok(JSBigInt::default());
        }
        let mut big_int = JSBigInt::create_with_length(1)?;
        big_int.set_digit(0, value as Digit);
        Ok(big_int)
    }

    /// `JSBigInt::createFrom(JSGlobalObject*, double)`. O valor precisa ser inteiro (`isInteger`).
    pub fn create_from_f64(value: f64) -> Result<JSBigInt, BigIntError> {
        if value == 0.0 {
            return Ok(JSBigInt::default());
        }

        let sign = value < 0.0; // -0 já foi tratado acima.
        let double_bits = value.to_bits();
        let raw_exponent = ((double_bits >> DOUBLE_PHYSICAL_MANTISSA_SIZE) as i32) & 0x7ff;
        debug_assert!(raw_exponent != 0x7ff);
        debug_assert!(raw_exponent >= 0x3ff);
        let exponent = raw_exponent - 0x3ff;
        let digits = exponent / DIGIT_BITS as i32 + 1;
        let mut result: Vec<Digit> = vec![0; digits as usize];

        // Constrói-se o BigInt deslocando a mantissa conforme o expoente e mapeando o padrão de
        // bits nos dígitos.
        let mut mantissa: u64 = (double_bits & DOUBLE_PHYSICAL_MANTISSA_MASK) | DOUBLE_MANTISSA_HIDDEN_BIT;

        let mantissa_top_bit = (DOUBLE_MANTISSA_SIZE - 1) as i32; // indexado em 0.
        // Posição (indexada em 0) do bit mais significativo no dígito mais significativo.
        let msd_top_bit = exponent % DIGIT_BITS as i32;
        // Bits da mantissa ainda não usados, mantidos deslocados à esquerda do `u64`.
        let mut remaining_mantissa_bits: i32 = 0;
        let mut digit: Digit;

        // Primeiro, o MSD, deslocando a mantissa de acordo.
        if msd_top_bit < mantissa_top_bit {
            remaining_mantissa_bits = mantissa_top_bit - msd_top_bit;
            digit = (mantissa >> remaining_mantissa_bits) as Digit;
            mantissa <<= 64 - remaining_mantissa_bits;
        } else {
            debug_assert!(msd_top_bit >= mantissa_top_bit);
            digit = (mantissa << (msd_top_bit - mantissa_top_bit)) as Digit;
            mantissa = 0;
        }
        result[(digits - 1) as usize] = digit;
        // Depois, o resto dos dígitos.
        let mut digit_index = digits - 2;
        while digit_index >= 0 {
            if remaining_mantissa_bits > 0 {
                remaining_mantissa_bits -= DIGIT_BITS as i32;
                // `sizeof(Digit) == 8`
                digit = mantissa;
                mantissa = 0;
            } else {
                digit = 0;
            }
            result[digit_index as usize] = digit;
            digit_index -= 1;
        }
        JSBigInt::try_create_from_impl(sign, &result)
    }

    /// `JSBigInt::tryCreateFromImpl(JSGlobalObject*, VM&, bool sign, std::span<const Digit>)`
    /// (definida na linha 7794 do `.cpp`, usada pela fatia). Também é `tryCreateFrom(..., sign, span)`.
    pub fn try_create_from_impl(sign: bool, digits: &[Digit]) -> Result<JSBigInt, BigIntError> {
        let digits = normalize(digits);
        if digits.is_empty() {
            return Ok(JSBigInt::default());
        }

        let mut result = JSBigInt::create_with_length(digits.len() as u32)?;
        result.digits.copy_from_slice(digits);
        result.set_sign(sign);
        Ok(result)
    }

    /// `JSBigInt::tryCreateFromWords(VM&, std::span<const uint64_t>, bool sign)`
    pub fn try_create_from_words(words: &[u64], sign: bool) -> Option<JSBigInt> {
        // Remove zeros à esquerda.
        let mut word_count = words.len();
        while word_count > 0 && words[word_count - 1] == 0 {
            word_count -= 1;
        }

        if word_count == 0 {
            return Some(JSBigInt::default());
        }

        // Confere o limite de tamanho.
        if word_count > MAX_LENGTH as usize {
            return None;
        }

        let mut big_int = JSBigInt::try_create_with_length(word_count as u32)?;
        big_int.set_sign(sign);
        big_int.digits.copy_from_slice(&words[..word_count]);
        Some(big_int)
    }

    /// `JSBigInt::createFromWords(JSGlobalObject*, std::span<const uint64_t>, bool sign)`
    pub fn create_from_words(words: &[u64], sign: bool) -> Result<JSBigInt, BigIntError> {
        JSBigInt::try_create_from_words(words, sign).ok_or(BigIntError::TooBig)
    }

    /// `JSBigInt::toWordsArray(std::span<uint64_t>)`
    pub fn to_words_array(&self, words: &mut [u64]) -> usize {
        let copy_count = words.len().min(self.length() as usize);
        if copy_count > 0 {
            words[..copy_count].copy_from_slice(&self.digits[..copy_count]);
        }
        copy_count
    }

    // FATIA2: `toPrimitive` devolve o próprio JSValue (precisa de JSValue/CellId do heap).

    pub fn set_sign(&mut self, sign: bool) {
        self.sign = sign;
    }

    pub fn sign(&self) -> bool {
        self.sign
    }

    pub fn length(&self) -> u32 {
        self.digits.len() as u32
    }

    /// `JSBigInt::bitLength`
    pub fn bit_length(&self) -> u32 {
        if self.is_zero() {
            return 1;
        }
        self.length() * DIGIT_BITS - self.digit(self.length() - 1).leading_zeros()
    }

    /// `JSBigInt::digit`
    pub fn digit(&self, n: u32) -> Digit {
        debug_assert!(n < self.length());
        self.digits[n as usize]
    }

    /// `JSBigInt::setDigit` (só para inicialização).
    pub fn set_digit(&mut self, n: u32, value: Digit) {
        debug_assert!(n < self.length());
        self.digits[n as usize] = value;
    }

    /// `JSBigInt::digits() const`
    pub fn digits(&self) -> &[Digit] {
        &self.digits
    }

    /// `JSBigInt::digits()` mutável.
    pub fn digits_mut(&mut self) -> &mut [Digit] {
        &mut self.digits
    }

    /// `JSBigInt::setLength`
    pub fn set_length(&mut self, length: u32) {
        self.digits.resize(length as usize, 0);
    }

    pub fn is_zero(&self) -> bool {
        debug_assert!(self.length() != 0 || !self.sign());
        self.length() == 0
    }

    /// `JSBigInt::toBoolean`
    pub fn to_boolean(&self) -> bool {
        !self.is_zero()
    }

    /// `m_hash` (0 significa ainda não calculado).
    pub fn cached_hash(&self) -> u32 {
        self.hash
    }

    // FATIA2: `hash()`/`hashSlow()` (linha 7787 do .cpp, `computeHash`) e `concurrentHash()`.
    // FATIA2: `equals`, `equalsToNumber`, `equalsToInt32`, `compare*`, `compareToDouble*`,
    //   `toNumber`, `toObject`, `toNumberHeap`, `toBigUInt64`/`toBigInt64`, `tryExtractDouble`
    //   (JSValue; implementações depois da linha 700).
    // FATIA2: `exponentiateImpl` começa na linha 632 e segue além da 700; fica inteira para a
    //   próxima fatia, assim como multiply/inc/dec/add/sub/divide/remainder/shift/bitwise/asIntN.
    // FATIA2: `makeHeapBigIntOrBigInt32`, `tryConvertToBigInt32`, `asHeapBigInt` precisam do
    //   `JSValue` (`jsBigInt32`) ainda não portado; `USE(BIGINT32)` é 0 (`PlatformUse.h:141`).
}

/// Interface comum de `HeapBigIntImpl`, `Int32BigIntImpl` e `Int64BigIntImpl` (os `BigIntImpl`
/// dos templates do C++).
pub trait BigIntImpl {
    fn is_zero(&self) -> bool;
    fn sign(&self) -> bool;
    fn length(&self) -> u32;
    fn digit(&self, i: u32) -> Digit;
    fn digits(&self) -> &[Digit];
}

/// `HeapBigIntImpl`: no C++ guarda `JSBigInt*`; aqui empresta a referência.
pub struct HeapBigIntImpl<'a> {
    big_int: &'a JSBigInt,
}

impl<'a> HeapBigIntImpl<'a> {
    pub fn new(big_int: &'a JSBigInt) -> HeapBigIntImpl<'a> {
        HeapBigIntImpl { big_int }
    }

    /// `toHeapBigInt(JSGlobalObject*, VM&)` e `toHeapBigInt(JSGlobalObject*)`. No C++ devolve o
    /// mesmo ponteiro; aqui, sem GC, clona. FATIA2: devolver o `CellId` quando o heap existir.
    pub fn to_heap_big_int(&self) -> Result<JSBigInt, BigIntError> {
        Ok(self.big_int.clone())
    }
}

impl BigIntImpl for HeapBigIntImpl<'_> {
    fn is_zero(&self) -> bool {
        self.big_int.is_zero()
    }
    fn sign(&self) -> bool {
        self.big_int.sign()
    }
    fn length(&self) -> u32 {
        self.big_int.length()
    }
    fn digit(&self, i: u32) -> Digit {
        self.big_int.digit(i)
    }
    fn digits(&self) -> &[Digit] {
        self.big_int.digits()
    }
}

/// `Int32BigIntImpl`
pub struct Int32BigIntImpl {
    value: i32,
    digit_storage: Digit,
}

impl Int32BigIntImpl {
    pub fn new(value: i32) -> Int32BigIntImpl {
        let mut this = Int32BigIntImpl { value, digit_storage: 0 };
        if !this.is_zero() {
            this.digit_storage = this.digit(0);
        }
        this
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    /// `toHeapBigInt(JSGlobalObject*, VM&)` e `toHeapBigInt(JSGlobalObject*)`
    pub fn to_heap_big_int(&self) -> Result<JSBigInt, BigIntError> {
        JSBigInt::create_from_i32(self.value)
    }
}

impl BigIntImpl for Int32BigIntImpl {
    fn is_zero(&self) -> bool {
        self.value == 0
    }
    fn sign(&self) -> bool {
        self.value < 0
    }
    fn length(&self) -> u32 {
        if self.is_zero() {
            0
        } else {
            1
        }
    }
    fn digit(&self, i: u32) -> Digit {
        debug_assert!(self.length() != 0);
        debug_assert!(i == 0);
        if self.sign() {
            return negate(self.value as i64) as Digit;
        }
        self.value as Digit
    }
    fn digits(&self) -> &[Digit] {
        &core::slice::from_ref(&self.digit_storage)[..self.length() as usize]
    }
}

/// `Int64BigIntImpl` (`numDigits == 1` em `CPU(REGISTER64)`).
pub struct Int64BigIntImpl {
    value: u64,
    digit_storage: Digit,
    sign: bool,
}

impl Int64BigIntImpl {
    pub const NUM_DIGITS: u32 = 1;

    /// `Int64BigIntImpl(int64_t)`
    pub fn from_i64(value: i64) -> Int64BigIntImpl {
        let mut this = Int64BigIntImpl { value: value as u64, digit_storage: 0, sign: value < 0 };
        if !this.is_zero() {
            this.digit_storage = this.digit(0);
        }
        this
    }

    /// `Int64BigIntImpl(uint64_t)`
    pub fn from_u64(value: u64) -> Int64BigIntImpl {
        let mut this = Int64BigIntImpl { value, digit_storage: 0, sign: false };
        if !this.is_zero() {
            this.digit_storage = this.digit(0);
        }
        this
    }
}

impl BigIntImpl for Int64BigIntImpl {
    fn is_zero(&self) -> bool {
        self.value == 0
    }
    fn sign(&self) -> bool {
        self.sign
    }
    fn length(&self) -> u32 {
        if self.is_zero() {
            0
        } else {
            Int64BigIntImpl::NUM_DIGITS
        }
    }
    fn digit(&self, i: u32) -> Digit {
        debug_assert!(i < self.length());
        if self.sign() {
            return negate(self.value as i64) as Digit;
        }
        self.value
    }
    fn digits(&self) -> &[Digit] {
        &core::slice::from_ref(&self.digit_storage)[..self.length() as usize]
    }
}

/// `JSBigInt::ImplResult`: no C++ é um `JSValue` (nulo, `JSBigInt*` ou BigInt32). FATIA2: trocar
/// por `JSValue` quando ele existir; o enum cobre os três casos que a fatia produz.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImplResult {
    /// `JSValue()` (exceção pendente, `nullptr` no C++).
    Empty,
    Heap(JSBigInt),
    BigInt32(i32),
}

impl From<JSBigInt> for ImplResult {
    fn from(value: JSBigInt) -> ImplResult {
        ImplResult::Heap(value)
    }
}

impl From<&Int32BigIntImpl> for ImplResult {
    fn from(value: &Int32BigIntImpl) -> ImplResult {
        ImplResult::BigInt32(value.value)
    }
}

/// `zeroImpl(VM&)`: com `USE(BIGINT32)` devolve `jsBigInt32(0)`.
pub fn zero_impl() -> ImplResult {
    ImplResult::BigInt32(0)
}

include!("js_big_int_part2.rs");
include!("js_big_int_part3.rs");
include!("js_big_int_part4.rs");
include!("js_big_int_part5.rs");
include!("js_big_int_part6.rs");
include!("js_big_int_part7.rs");
include!("js_big_int_part8.rs");
include!("js_big_int_part9.rs");
