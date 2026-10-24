//! Tradução de `JavaScriptCore/parser/ResultType.h`: `ResultType` (o que se sabe do tipo do
//! resultado de uma expressão, em bits) e `OperandTypes` (o par de tipos dos operandos de uma
//! operação binária, empacotado em 16 bits).

use std::fmt;

/// `ResultType::Type`.
pub type ResultTypeBits = u8;

// FIXME do C++: considerar se isto é realmente necessário (a informação de profiling do LLInt e do
// Baseline basta?). https://bugs.webkit.org/show_bug.cgi?id=201659
/// `struct ResultType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultType {
    bits: ResultTypeBits,
}

impl ResultType {
    const TYPE_INT32: ResultTypeBits = 1 << 0;
    const TYPE_MAYBE_NUMBER: ResultTypeBits = 1 << 1;
    const TYPE_MAYBE_STRING: ResultTypeBits = 1 << 2;
    const TYPE_MAYBE_BIG_INT: ResultTypeBits = 1 << 3;
    const TYPE_MAYBE_NULL: ResultTypeBits = 1 << 4;
    const TYPE_MAYBE_BOOL: ResultTypeBits = 1 << 5;
    const TYPE_MAYBE_OTHER: ResultTypeBits = 1 << 6;

    const TYPE_BITS: ResultTypeBits = Self::TYPE_MAYBE_NUMBER
        | Self::TYPE_MAYBE_STRING
        | Self::TYPE_MAYBE_BIG_INT
        | Self::TYPE_MAYBE_NULL
        | Self::TYPE_MAYBE_BOOL
        | Self::TYPE_MAYBE_OTHER;

    pub const NUM_BITS_NEEDED: i32 = 7;

    /// `constexpr explicit ResultType(Type type)`.
    pub const fn new(bits: ResultTypeBits) -> ResultType {
        ResultType { bits }
    }

    pub const fn is_int32(&self) -> bool {
        self.bits & Self::TYPE_INT32 != 0
    }

    pub const fn definitely_is_number(&self) -> bool {
        (self.bits & Self::TYPE_BITS) == Self::TYPE_MAYBE_NUMBER
    }

    pub const fn definitely_is_string(&self) -> bool {
        (self.bits & Self::TYPE_BITS) == Self::TYPE_MAYBE_STRING
    }

    pub const fn definitely_is_boolean(&self) -> bool {
        (self.bits & Self::TYPE_BITS) == Self::TYPE_MAYBE_BOOL
    }

    pub const fn definitely_is_big_int(&self) -> bool {
        (self.bits & Self::TYPE_BITS) == Self::TYPE_MAYBE_BIG_INT
    }

    pub const fn definitely_is_null(&self) -> bool {
        (self.bits & Self::TYPE_BITS) == Self::TYPE_MAYBE_NULL
    }

    pub const fn might_be_undefined_or_null(&self) -> bool {
        self.bits & (Self::TYPE_MAYBE_NULL | Self::TYPE_MAYBE_OTHER) != 0
    }

    pub const fn might_be_number(&self) -> bool {
        self.bits & Self::TYPE_MAYBE_NUMBER != 0
    }

    pub const fn is_not_number(&self) -> bool {
        !self.might_be_number()
    }

    pub const fn might_be_big_int(&self) -> bool {
        self.bits & Self::TYPE_MAYBE_BIG_INT != 0
    }

    pub const fn is_not_big_int(&self) -> bool {
        !self.might_be_big_int()
    }

    pub const fn null_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_NULL)
    }

    pub const fn boolean_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_BOOL)
    }

    pub const fn number_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_NUMBER)
    }

    pub const fn number_type_is_int32() -> ResultType {
        ResultType::new(Self::TYPE_INT32 | Self::TYPE_MAYBE_NUMBER)
    }

    pub const fn string_or_number_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_NUMBER | Self::TYPE_MAYBE_STRING)
    }

    pub const fn add_result_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_NUMBER | Self::TYPE_MAYBE_STRING | Self::TYPE_MAYBE_BIG_INT)
    }

    pub const fn string_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_STRING)
    }

    pub const fn big_int_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_BIG_INT)
    }

    pub const fn big_int_or_int32_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_BIG_INT | Self::TYPE_INT32 | Self::TYPE_MAYBE_NUMBER)
    }

    pub const fn big_int_or_number_type() -> ResultType {
        ResultType::new(Self::TYPE_MAYBE_BIG_INT | Self::TYPE_MAYBE_NUMBER)
    }

    pub const fn unknown_type() -> ResultType {
        ResultType::new(Self::TYPE_BITS)
    }

    pub const fn for_add(op1: ResultType, op2: ResultType) -> ResultType {
        if op1.definitely_is_number() && op2.definitely_is_number() {
            return Self::number_type();
        }
        if op1.definitely_is_string() || op2.definitely_is_string() {
            return Self::string_type();
        }
        if op1.definitely_is_big_int() && op2.definitely_is_big_int() {
            return Self::big_int_type();
        }
        Self::add_result_type()
    }

    pub const fn for_non_add_arith(op1: ResultType, op2: ResultType) -> ResultType {
        if op1.definitely_is_number() && op2.definitely_is_number() {
            return Self::number_type();
        }
        if op1.definitely_is_big_int() && op2.definitely_is_big_int() {
            return Self::big_int_type();
        }
        Self::big_int_or_number_type()
    }

    pub const fn for_unary_arith(op: ResultType) -> ResultType {
        if op.definitely_is_number() {
            return Self::number_type();
        }
        if op.definitely_is_big_int() {
            return Self::big_int_type();
        }
        Self::big_int_or_number_type()
    }

    /// Ao contrário do C, um operador lógico produz o valor da última expressão avaliada (e não
    /// `true` ou `false`).
    pub const fn for_logical_op(op1: ResultType, op2: ResultType) -> ResultType {
        if op1.definitely_is_boolean() && op2.definitely_is_boolean() {
            return Self::boolean_type();
        }
        if op1.definitely_is_number() && op2.definitely_is_number() {
            return Self::number_type();
        }
        if op1.definitely_is_string() && op2.definitely_is_string() {
            return Self::string_type();
        }
        if op1.definitely_is_big_int() && op2.definitely_is_big_int() {
            return Self::big_int_type();
        }
        Self::unknown_type()
    }

    pub const fn for_coalesce(op1: ResultType, op2: ResultType) -> ResultType {
        if op1.definitely_is_null() {
            return op2;
        }
        if !op1.might_be_undefined_or_null() {
            return op1;
        }
        Self::unknown_type()
    }

    pub const fn for_bit_op() -> ResultType {
        Self::big_int_or_int32_type()
    }

    pub const fn bits(&self) -> ResultTypeBits {
        self.bits
    }

    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        // FIXME do C++: informação mais significativa. https://bugs.webkit.org/show_bug.cgi?id=190930
        write!(out, "{}", self.bits())
    }
}

/// `constexpr explicit ResultType()`: o tipo desconhecido.
impl Default for ResultType {
    fn default() -> ResultType {
        ResultType::unknown_type()
    }
}

// `static_assert((TypeBits & ((1 << numBitsNeeded) - 1)) == TypeBits)`.
const _: () = assert!((ResultType::TYPE_BITS & ((1u32 << ResultType::NUM_BITS_NEEDED) - 1) as u8) == ResultType::TYPE_BITS);

/// `struct OperandTypes`. No C++ são dois `uint8_t` seguidos, que o `std::bit_cast` lê como um
/// `uint16_t` em x86_64 (little endian): `m_first` nos 8 bits baixos, `m_second` nos altos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperandTypes {
    pub first: ResultTypeBits,
    pub second: ResultTypeBits,
}

impl OperandTypes {
    pub const fn new(first: ResultType, second: ResultType) -> OperandTypes {
        OperandTypes { first: first.bits, second: second.bits }
    }

    /// `first()`.
    pub const fn first(&self) -> ResultType {
        ResultType::new(self.first)
    }

    /// `second()`.
    pub const fn second(&self) -> ResultType {
        ResultType::new(self.second)
    }

    /// `bits()` (`std::bit_cast<uint16_t>` em little endian).
    pub const fn bits(&self) -> u16 {
        u16::from_le_bytes([self.first, self.second])
    }

    pub const fn from_bits(bits: u16) -> OperandTypes {
        let bytes = bits.to_le_bytes();
        OperandTypes { first: bytes[0], second: bytes[1] }
    }

    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str("OperandTypes(")?;
        self.first().dump(out)?;
        out.write_str(", ")?;
        self.second().dump(out)?;
        out.write_str(")")
    }
}

/// Os argumentos padrão do construtor C++ (`first = unknownType(), second = unknownType()`).
impl Default for OperandTypes {
    fn default() -> OperandTypes {
        OperandTypes::new(ResultType::unknown_type(), ResultType::unknown_type())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_match_cpp() {
        assert_eq!(ResultType::unknown_type().bits(), 0b0111_1110);
        assert_eq!(ResultType::number_type_is_int32().bits(), 0b11);
        assert_eq!(ResultType::big_int_or_int32_type().bits(), 0b1011);
    }

    #[test]
    fn for_add_rules() {
        let n = ResultType::number_type();
        let s = ResultType::string_type();
        assert_eq!(ResultType::for_add(n, n), n);
        assert_eq!(ResultType::for_add(n, s), s);
        assert_eq!(ResultType::for_add(n, ResultType::boolean_type()), ResultType::add_result_type());
    }

    #[test]
    fn coalesce_rules() {
        let n = ResultType::number_type();
        assert_eq!(ResultType::for_coalesce(ResultType::null_type(), n), n);
        assert_eq!(ResultType::for_coalesce(n, ResultType::string_type()), n);
        assert_eq!(ResultType::for_coalesce(ResultType::unknown_type(), n), ResultType::unknown_type());
    }

    #[test]
    fn operand_types_roundtrip() {
        let ops = OperandTypes::new(ResultType::number_type(), ResultType::string_type());
        assert_eq!(ops.bits(), 0x0402);
        assert_eq!(OperandTypes::from_bits(0x0402), ops);
        assert_eq!(ops.first(), ResultType::number_type());
    }
}
