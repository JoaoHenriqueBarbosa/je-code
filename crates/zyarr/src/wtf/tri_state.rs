//! Tradução de `WTF/wtf/TriState.h`.

/// `enum class TriState : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriState {
    False = 0,
    True = 1,
    Indeterminate = 2,
}

impl TriState {
    /// `triState(bool)`: `static_cast<TriState>(boolean)`.
    pub const fn from_bool(boolean: bool) -> TriState {
        if boolean {
            TriState::True
        } else {
            TriState::False
        }
    }

    /// `static_cast<TriState>(bits)` para o valor já mascarado em 2 bits; o 3 não ocorre em uso
    /// válido e cai em `Indeterminate`.
    pub const fn from_bits(bits: u32) -> TriState {
        match bits {
            0 => TriState::False,
            1 => TriState::True,
            _ => TriState::Indeterminate,
        }
    }

    /// `invert(TriState)`.
    pub const fn invert(self) -> TriState {
        match self {
            TriState::True => TriState::False,
            TriState::False => TriState::True,
            TriState::Indeterminate => TriState::Indeterminate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(TriState::from_bool(true), TriState::True);
        assert_eq!(TriState::True.invert(), TriState::False);
        assert_eq!(TriState::Indeterminate.invert(), TriState::Indeterminate);
        assert_eq!(TriState::Indeterminate as u32, 2);
    }
}
