//! Tradução de `runtime/EnumerationMode.h`.

/// `enum class PropertyNameMode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyNameMode {
    Symbols = 1 << 0,
    Strings = 1 << 1,
    StringsAndSymbols = (1 << 0) | (1 << 1),
}

impl PropertyNameMode {
    /// `propertyNameMode & PropertyNameMode::Symbols`.
    pub fn includes_symbols(self) -> bool {
        self as u8 & PropertyNameMode::Symbols as u8 != 0
    }

    /// `propertyNameMode & PropertyNameMode::Strings`.
    pub fn includes_strings(self) -> bool {
        self as u8 & PropertyNameMode::Strings as u8 != 0
    }
}

/// `enum class PrivateSymbolMode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateSymbolMode {
    Include,
    Exclude,
}

/// `enum class DontEnumPropertiesMode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DontEnumPropertiesMode {
    Include,
    Exclude,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_mode_bits() {
        assert!(PropertyNameMode::StringsAndSymbols.includes_symbols());
        assert!(PropertyNameMode::StringsAndSymbols.includes_strings());
        assert!(PropertyNameMode::Symbols.includes_symbols());
        assert!(!PropertyNameMode::Symbols.includes_strings());
        assert!(!PropertyNameMode::Strings.includes_symbols());
    }
}
