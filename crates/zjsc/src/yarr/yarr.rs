//! Porte de `yarr/Yarr.h`: constantes e enums do Yarr.

pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PATTERN_CHARACTER: u32 = 2; // Só para quantificadores não fixos.
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_CHARACTER_CLASS: u32 = 2; // Greedy/NonGreedy, ou FixedCount com a flag unicode/unicodeSets.
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_BACK_REFERENCE: u32 = 4;
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_ALTERNATIVE: u32 = 1; // Um por alternativa.
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHETICAL_ASSERTION: u32 = 1;
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE: u32 = 3;
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL: u32 = 2;
pub const YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES: u32 = 4;
pub const YARR_STACK_SPACE_FOR_DOT_STAR_ENCLOSURE: u32 = 2; // O deslocamento inicial e o fim do trecho sem quebra de linha que o segue.

// O despacho pelo primeiro caractere (YarrJIT) só compensa a leitura extra a partir de poucas
// alternativas e caracteres; emite um stub de entrada por par (cadeia, alternativa) e desiste além
// dos limites superiores. O YarrPattern molda os grupos que quer despachados para caberem aqui.
// Os limites superiores foram dimensionados para a expansão plana de \p{RGI_Emoji} (~2.800
// alternativas, ~1.400 primeiros pontos de código distintos).
pub const ALTERNATION_DISPATCH_MIN_ALTERNATIVES: u32 = 4;
pub const ALTERNATION_DISPATCH_MIN_TOTAL_SIZE: u32 = 12;
pub const ALTERNATION_DISPATCH_MAX_CHAINS: u32 = 2048;
pub const ALTERNATION_DISPATCH_MAX_STUBS: u32 = 4096;

pub const QUANTIFY_INFINITE: u32 = u32::MAX;
pub const QUANTIFY_INFINITE64: u64 = u64::MAX;
pub const OFFSET_NO_MATCH: u32 = u32::MAX;

/// O limite abaixo restringe o número de chamadas de match "recursivas" para evitar gastar tempo
/// exponencial em expressões regulares complexas.
pub const MATCH_LIMIT: u32 = 100000000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchFrom {
    VMThread,
    CompilerThread,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum JSRegExpResult {
    Match = 1,
    NoMatch = 0,
    ErrorNoMatch = -1,
    JITCodeFailure = -2,
    ErrorHitLimit = -3,
    ErrorNoMemory = -4,
    ErrorInternal = -5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CharSize {
    Char8,
    Char16,
}

/// `enum class BuiltInCharacterClassID : unsigned`. O C++ soma o índice da propriedade Unicode a
/// `BaseUnicodePropertyID`, valor que nenhum enumerador nomeia; por isso é um newtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BuiltInCharacterClassID(pub u32);

#[allow(non_upper_case_globals)]
impl BuiltInCharacterClassID {
    pub const DigitClassID: Self = Self(0);
    pub const SpaceClassID: Self = Self(1);
    pub const WordClassID: Self = Self(2);
    pub const DotClassID: Self = Self(3);
    pub const BaseUnicodePropertyID: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SpecificPattern {
    None,
    Atom,
    LeadingSpacesStar,
    LeadingSpacesPlus,
    TrailingSpacesStar,
    TrailingSpacesPlus,
    Newlines,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExecutionMode {
    MatchOnly,
    IncludeSubpatterns,
    InlineTest,
}

// `struct BytecodePattern;` (declaração antecipada) vive no módulo do interpretador.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_cpp() {
        assert_eq!(QUANTIFY_INFINITE, 4294967295);
        assert_eq!(MATCH_LIMIT, 100_000_000);
        assert_eq!(JSRegExpResult::ErrorInternal as i32, -5);
        assert_eq!(BuiltInCharacterClassID::BaseUnicodePropertyID.0, 4);
    }
}
