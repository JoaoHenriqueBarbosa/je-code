//! Porte de `WTF/wtf/text/ConversionMode.h`.

/// `ConversionMode`. O padrão do C++ é `LenientConversion`; o chamador o passa explicitamente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionMode {
    LenientConversion,
    StrictConversion,
    StrictConversionReplacingUnpairedSurrogatesWithFFFD,
}
