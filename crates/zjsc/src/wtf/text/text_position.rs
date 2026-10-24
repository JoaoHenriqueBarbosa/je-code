//! Tradução de `WTF/wtf/text/OrdinalNumber.h` e `WTF/wtf/text/TextPosition.h`.
//!
//! Por pedido, o `OrdinalNumber` mora aqui junto com o `TextPosition`. As especializações de
//! `DefaultHash` e `HashTraits` (valor "deletado" da tabela do WTF) não existem: o `HashMap` do Rust
//! dispensa o marcador. O `Hash` derivado cobre o papel do `DefaultHash`.

/// `OrdinalNumber`: um número de elemento numa sequência que tem primeiro elemento. Evita o inteiro
/// ambíguo entre as tradições de contar a partir de 0 ou de 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrdinalNumber {
    zero_based_value: i32,
}

impl OrdinalNumber {
    /// `OrdinalNumber::beforeFirst()`.
    pub fn before_first() -> OrdinalNumber {
        OrdinalNumber { zero_based_value: -1 }
    }

    /// `OrdinalNumber::fromZeroBasedInt(int)`.
    pub fn from_zero_based_int(zero_based_int: i32) -> OrdinalNumber {
        OrdinalNumber { zero_based_value: zero_based_int }
    }

    /// `OrdinalNumber::fromOneBasedInt(int)`.
    pub fn from_one_based_int(one_based_int: i32) -> OrdinalNumber {
        OrdinalNumber { zero_based_value: one_based_int.wrapping_sub(1) }
    }

    /// `zeroBasedInt()`.
    pub fn zero_based_int(&self) -> i32 {
        self.zero_based_value
    }

    /// `oneBasedInt()`.
    pub fn one_based_int(&self) -> i32 {
        self.zero_based_value.wrapping_add(1)
    }
}

impl Default for OrdinalNumber {
    /// `OrdinalNumber() : m_zeroBasedValue(0)`.
    fn default() -> OrdinalNumber {
        OrdinalNumber { zero_based_value: 0 }
    }
}

/// `TextPosition`: coordenadas dentro de um recurso de texto, usada sobretudo para guardar a posição
/// de um script. A ordem derivada compara a linha e, em empate, a coluna, como o `operator<=>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextPosition {
    pub line: OrdinalNumber,
    pub column: OrdinalNumber,
}

impl TextPosition {
    /// `TextPosition(OrdinalNumber line, OrdinalNumber column)`.
    pub fn new(line: OrdinalNumber, column: OrdinalNumber) -> TextPosition {
        TextPosition { line, column }
    }

    /// `TextPosition::belowRangePosition()`: valor com a linha abaixo do mínimo, posição impossível.
    pub fn below_range_position() -> TextPosition {
        TextPosition::new(OrdinalNumber::before_first(), OrdinalNumber::before_first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinal_bases() {
        assert_eq!(OrdinalNumber::before_first().zero_based_int(), -1);
        assert_eq!(OrdinalNumber::before_first().one_based_int(), 0);
        assert_eq!(OrdinalNumber::from_one_based_int(1).zero_based_int(), 0);
        assert_eq!(OrdinalNumber::from_zero_based_int(4).one_based_int(), 5);
        assert_eq!(OrdinalNumber::default().one_based_int(), 1);
    }

    #[test]
    fn text_position_order() {
        let a = TextPosition::new(OrdinalNumber::from_one_based_int(1), OrdinalNumber::from_one_based_int(9));
        let b = TextPosition::new(OrdinalNumber::from_one_based_int(2), OrdinalNumber::from_one_based_int(1));
        assert!(a < b);
        assert!(TextPosition::below_range_position() < TextPosition::default());
    }
}
