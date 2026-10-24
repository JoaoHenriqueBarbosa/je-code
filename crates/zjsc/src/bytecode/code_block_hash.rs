//! Tradução de `bytecode/CodeBlockHash.h` e `.cpp`.
//!
//! Hash informal de um code block: os 32 bits baixos do SHA-1 do código-fonte, com o bit baixo
//! virado conforme o papel (chamada ou construção).

use std::fmt;

use crate::parser::source_code::SourceCode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::wtf::sha1::{Digest, Sha1};
use crate::wtf::six_character_hash::{integer_to_six_character_hash_string, six_character_hash_string_to_integer};
use crate::wtf::text::string_view::StringView;

/// `CodeBlockHash::stringLength`.
pub const STRING_LENGTH: usize = 6;

/// `class CodeBlockHash` (a comparação padrão do C++ é a ordem do `unsigned`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeBlockHash {
    hash: u32,
}

impl CodeBlockHash {
    /// `CodeBlockHash(unsigned)`.
    pub fn from_hash(hash: u32) -> CodeBlockHash {
        CodeBlockHash { hash }
    }

    /// `CodeBlockHash(std::span<const char, stringLength>)`.
    pub fn from_string(string: &[u8; STRING_LENGTH]) -> CodeBlockHash {
        CodeBlockHash { hash: six_character_hash_string_to_integer(string) }
    }

    /// `CodeBlockHash(StringView, StringView, CodeSpecializationKind)`.
    pub fn from_views(
        code_block_source_code: StringView,
        entire_source_code: StringView,
        kind: CodeSpecializationKind,
    ) -> CodeBlockHash {
        let mut sha1 = Sha1::new();

        // The maxSourceCodeLengthToHash is a heuristic to avoid crashing fuzzers
        // due to resource exhaustion. This is OK to do because:
        // 1. CodeBlockHash is not a critical hash.
        // 2. In practice, reasonable source code are not 500 MB or more long.
        // 3. And if they are that long, then we are still diversifying the hash on
        //    their length. But if they do collide, it's OK.
        // The only invariant here is that we should always produce the same hash
        // for the same source string. The algorithm below achieves that.
        const MAX_SOURCE_CODE_LENGTH_TO_HASH: u32 = 500 * 1024 * 1024;
        if code_block_source_code.length() < MAX_SOURCE_CODE_LENGTH_TO_HASH {
            sha1.add_utf8_bytes(code_block_source_code);
        } else {
            // Just hash with the length and samples of the source string instead.
            let mut index: u32 = 0;
            let length: u32 = entire_source_code.length();
            let step: u32 = (length >> 10) + 1;

            sha1.add_bytes(&length.to_ne_bytes());
            loop {
                let character: u16 = entire_source_code.code_unit_at(index);
                sha1.add_bytes(&character.to_ne_bytes());
                let old_index = index;
                index = index.wrapping_add(step);
                if !(index > old_index && index < length) {
                    break;
                }
            }
        }

        let digest: Digest = sha1.compute_hash();
        let mut hash = (digest[0] as u32)
            | ((digest[1] as u32) << 8)
            | ((digest[2] as u32) << 16)
            | ((digest[3] as u32) << 24);

        if hash == 0 || hash == 1 {
            // Ensures a non-zero hash, and gets us #Azero0 for CodeForCall and #Azero1 for CodeForConstruct.
            hash = hash.wrapping_add(0x2d5a93d0);
        }
        hash ^= kind as u32;
        debug_assert!(hash != 0);
        CodeBlockHash { hash }
    }

    /// `CodeBlockHash(const SourceCode&, CodeSpecializationKind)`: o provedor nulo é desreferência
    /// nula no C++, aqui uma violação de invariante.
    pub fn from_source_code(source_code: &SourceCode, kind: CodeSpecializationKind) -> CodeBlockHash {
        let view = source_code.view();
        let entire = match source_code.provider() {
            Some(provider) => provider.source(),
            None => panic!("CodeBlockHash: SourceCode sem provedor"),
        };
        CodeBlockHash::from_views(StringView::from(&view), StringView::from(&entire), kind)
    }

    /// `isSet()` (e `operator bool`).
    pub fn is_set(&self) -> bool {
        self.hash != 0
    }

    /// `hash()`.
    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// `dump(PrintStream&)`: o `PrintStream` vira `fmt::Write`. O `ASSERT_ENABLED` some.
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        let buffer = integer_to_six_character_hash_string(self.hash);
        out.write_str(&String::from_utf8_lossy(&buffer))
    }
}

/// O `StringTypeAdapter<CodeBlockHash>`: o texto de seis caracteres que o `makeString` escreve.
impl CodeBlockHash {
    pub fn to_six_character_string(&self) -> [u8; STRING_LENGTH] {
        integer_to_six_character_hash_string(self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_and_construct_differ_in_low_bit() {
        let source = b"function f() {}";
        let call = CodeBlockHash::from_views(StringView::from(&source[..]), StringView::from(&source[..]), CodeSpecializationKind::CodeForCall);
        let construct = CodeBlockHash::from_views(StringView::from(&source[..]), StringView::from(&source[..]), CodeSpecializationKind::CodeForConstruct);
        assert_eq!(call.hash() ^ construct.hash(), 1);
        assert_eq!(CodeBlockHash::from_string(&call.to_six_character_string()), call);
    }
}

impl fmt::Display for CodeBlockHash {
    /// `CodeBlockHash::dump` (o `operator<<` do `PrintStream`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.dump(f)
    }
}
