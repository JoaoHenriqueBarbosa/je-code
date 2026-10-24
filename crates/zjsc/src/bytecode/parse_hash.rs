//! Tradução de `bytecode/ParseHash.h` e `.cpp`.

use crate::bytecode::code_block_hash::CodeBlockHash;
use crate::parser::source_code::SourceCode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::wtf::sha1::Sha1;
use crate::wtf::text::string_view::StringView;

/// `class ParseHash`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseHash {
    hash_for_call: CodeBlockHash,
    hash_for_construct: CodeBlockHash,
}

impl ParseHash {
    /// `ParseHash(const SourceCode&)`.
    pub fn new(source_code: &SourceCode) -> ParseHash {
        let mut sha1 = Sha1::new();
        let view = source_code.view();
        sha1.add_utf8_bytes(StringView::from(&view));
        let digest = sha1.compute_hash();
        let mut hash = (digest[0] as u32)
            | ((digest[1] as u32) << 8)
            | ((digest[2] as u32) << 16)
            | ((digest[3] as u32) << 24);

        if hash == 0 || hash == 1 {
            // Ensures a non-zero hash, and gets us #Azero0 for CodeForCall and #Azero1 for CodeForConstruct.
            hash = hash.wrapping_add(0x2d5a93d0);
        }
        let hash_for_call = hash ^ (CodeSpecializationKind::CodeForCall as u32);
        let hash_for_construct = hash ^ (CodeSpecializationKind::CodeForConstruct as u32);

        debug_assert!(hash_for_call != 0);
        debug_assert!(hash_for_construct != 0);
        ParseHash {
            hash_for_call: CodeBlockHash::from_hash(hash_for_call),
            hash_for_construct: CodeBlockHash::from_hash(hash_for_construct),
        }
    }

    /// `hashForCall()`.
    pub fn hash_for_call(&self) -> CodeBlockHash {
        self.hash_for_call
    }

    /// `hashForConstruct()`.
    pub fn hash_for_construct(&self) -> CodeBlockHash {
        self.hash_for_construct
    }
}
