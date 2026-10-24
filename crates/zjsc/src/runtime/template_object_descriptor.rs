//! Tradução de `runtime/TemplateObjectDescriptor.h` e `.cpp`.
//!
//! Divergência documentada: o C++ tem os marcadores `DeletedValue`/`EmptyValue` para a tabela de
//! hash aberta da WTF; o `HashSet` do Rust não os usa, então não existem aqui.

use std::rc::Rc;

use crate::wtf::hash_functions::pair_int_hash;
use crate::wtf::text::string_hasher::STRING_HASHING_START_VALUE;
use crate::wtf::text::wtf_string::String as WtfString;

/// `Vector<String, 4>`.
pub type StringVector = Vec<WtfString>;
/// `Vector<std::optional<String>, 4>`.
pub type OptionalStringVector = Vec<Option<WtfString>>;

/// `TemplateObjectDescriptor` (`RefCounted`, daí o `Rc` em `create`).
#[derive(Debug)]
pub struct TemplateObjectDescriptor {
    raw_strings: StringVector,
    cooked_strings: OptionalStringVector,
    hash: u32,
}

impl TemplateObjectDescriptor {
    /// `create(StringVector&&, OptionalStringVector&&)`.
    pub fn create(raw_strings: StringVector, cooked_strings: OptionalStringVector) -> Rc<TemplateObjectDescriptor> {
        let hash = Self::calculate_hash(&raw_strings);
        Rc::new(TemplateObjectDescriptor { raw_strings, cooked_strings, hash })
    }

    pub fn hash(&self) -> u32 {
        self.hash
    }

    pub fn raw_strings(&self) -> &StringVector {
        &self.raw_strings
    }

    pub fn cooked_strings(&self) -> &OptionalStringVector {
        &self.cooked_strings
    }

    /// `calculateHash(const StringVector&)`.
    pub fn calculate_hash(raw_strings: &StringVector) -> u32 {
        let mut hash = STRING_HASHING_START_VALUE;
        for string in raw_strings {
            hash = pair_int_hash(hash, string.hash());
        }
        hash
    }
}

impl PartialEq for TemplateObjectDescriptor {
    /// `m_hash == other.m_hash && m_rawStrings == other.m_rawStrings`.
    fn eq(&self, other: &TemplateObjectDescriptor) -> bool {
        self.hash == other.hash && self.raw_strings == other.raw_strings
    }
}

impl Eq for TemplateObjectDescriptor {}

impl std::hash::Hash for TemplateObjectDescriptor {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.hash);
    }
}
