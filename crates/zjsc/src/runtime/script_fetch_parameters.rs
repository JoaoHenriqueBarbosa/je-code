//! Tradução de `runtime/ScriptFetchParameters.h` (com `USE(BUN_JSC_ADDITIONS)` ligado pelo
//! `cmakeconfig.h`, como em `script_fetcher.rs`).
//!
//! DIVERGÊNCIAS: `RefCounted<ScriptFetchParameters>` vira `Rc<ScriptFetchParameters>`. Os virtuais
//! `integrity()` e `isTopLevelModule()` só têm a implementação da base aqui (a subclasse que os
//! sobrescreve, `ScriptFetchParameters` do `JSModuleLoader`, entra com o loader): `integrity()`
//! devolve a string nula e `isTopLevelModule()` falso.

use std::collections::HashMap;
use std::rc::Rc;

use crate::wtf::text::string_impl::UniquedKey;
use crate::wtf::text::wtf_string::String as WtfString;

/// `ScriptFetchParameters::Type`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptFetchParametersType {
    None = 0,
    JavaScript = 1,
    WebAssembly = 2,
    JSON = 3,
    Text = 4,
    HostDefined = 5,
}

/// `ScriptFetchParameters::AttributesMap`.
pub type AttributesMap = HashMap<Option<UniquedKey>, WtfString>;

/// `class ScriptFetchParameters`.
#[derive(Debug)]
pub struct ScriptFetchParameters {
    m_type: ScriptFetchParametersType,
    m_host_defined_import_type: WtfString,
    m_attributes: std::cell::RefCell<AttributesMap>,
}

/// `Ref<ScriptFetchParameters>` / `RefPtr<ScriptFetchParameters>`.
pub type ScriptFetchParametersRef = Rc<ScriptFetchParameters>;

impl ScriptFetchParameters {
    /// `create(Type)`.
    pub fn create(type_: ScriptFetchParametersType) -> ScriptFetchParametersRef {
        assert!(
            type_ != ScriptFetchParametersType::HostDefined,
            "HostDefined type requires a hostDefinedImportType"
        );
        Rc::new(ScriptFetchParameters {
            m_type: type_,
            m_host_defined_import_type: WtfString::default(),
            m_attributes: std::cell::RefCell::new(AttributesMap::new()),
        })
    }

    /// `create(const String& hostDefinedImportType)`.
    pub fn create_host_defined(host_defined_import_type: &WtfString) -> ScriptFetchParametersRef {
        Rc::new(ScriptFetchParameters {
            m_type: ScriptFetchParametersType::HostDefined,
            m_host_defined_import_type: host_defined_import_type.clone(),
            m_attributes: std::cell::RefCell::new(AttributesMap::new()),
        })
    }

    /// `type()`.
    pub fn type_(&self) -> ScriptFetchParametersType {
        self.m_type
    }

    /// `integrity()`: a da base é a string nula.
    pub fn integrity(&self) -> WtfString {
        WtfString::default()
    }

    /// `isTopLevelModule()`: o da base é falso.
    pub fn is_top_level_module(&self) -> bool {
        false
    }

    /// `hostDefinedImportType()`.
    pub fn host_defined_import_type(&self) -> &WtfString {
        &self.m_host_defined_import_type
    }

    /// `attributes()`.
    pub fn attributes(&self) -> std::cell::Ref<'_, AttributesMap> {
        self.m_attributes.borrow()
    }

    /// `setAttributes(AttributesMap&&)`.
    pub fn set_attributes(&self, attributes: AttributesMap) {
        *self.m_attributes.borrow_mut() = attributes;
    }

    /// `parseType(StringView)`. Com os acréscimos do Bun, `"text"` não é tipo próprio (cai em
    /// `HostDefined`) e qualquer string não vazia desconhecida também.
    pub fn parse_type(string: &str) -> Option<ScriptFetchParametersType> {
        if string == "json" {
            return Some(ScriptFetchParametersType::JSON);
        }
        if string == "webassembly" {
            return Some(ScriptFetchParametersType::WebAssembly);
        }
        if !string.is_empty() {
            return Some(ScriptFetchParametersType::HostDefined);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_follows_bun_additions() {
        assert_eq!(ScriptFetchParameters::parse_type("json"), Some(ScriptFetchParametersType::JSON));
        assert_eq!(ScriptFetchParameters::parse_type("webassembly"), Some(ScriptFetchParametersType::WebAssembly));
        assert_eq!(ScriptFetchParameters::parse_type("text"), Some(ScriptFetchParametersType::HostDefined));
        assert_eq!(ScriptFetchParameters::parse_type(""), None);
    }

    #[test]
    fn create_keeps_type() {
        let parameters = ScriptFetchParameters::create(ScriptFetchParametersType::JSON);
        assert_eq!(parameters.type_(), ScriptFetchParametersType::JSON);
        assert!(!parameters.is_top_level_module());
    }
}
