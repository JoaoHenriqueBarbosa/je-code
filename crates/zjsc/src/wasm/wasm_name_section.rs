//! Tradução de `wasm/WasmNameSection.h`, `WasmNameSectionParser.{h,cpp}` e
//! `WasmIndexOrName.{h,cpp}`: a seção customizada `name` (módulo, funções e locais) e o nome que
//! aparece no stack trace (`wasm-function[3]` ou o nome da função).

use std::sync::Arc;

use crate::runtime::options::Options;
use crate::wasm::wasm_format::Name;
use crate::wasm::wasm_module_information::ModuleInformation;
use crate::wasm::wasm_parser::ParserBase;

/// `NameType` (`WasmFormat.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NameType {
    Module = 0,
    Function = 1,
    Local = 2,
}

/// `isValidNameType` + o `static_cast<NameType>`.
fn decode_name_type(value: u8) -> Option<NameType> {
    match value {
        0 => Some(NameType::Module),
        1 => Some(NameType::Function),
        2 => Some(NameType::Local),
        _ => None,
    }
}

/// `NameSection`. O `ThreadSafeRefCounted` do C++ aparece só no `IndexOrName` (um `Arc` com a
/// cópia da seção no momento da consulta); o `ModuleInformation` é o dono do valor.
#[derive(Clone, Debug)]
pub struct NameSection {
    pub module_name: Name,
    pub module_hash: Name,
    pub function_names: Vec<Name>,
}

impl NameSection {
    /// `NameSection::create()`: com `useEagerWasmModuleHashing`, o hash começa em `<?>`.
    pub fn create() -> NameSection {
        let mut section = NameSection { module_name: Name::new(), module_hash: Name::new(), function_names: Vec::new() };
        if Options::with(|options| options.use_eager_wasm_module_hashing) {
            section.set_hash(None);
        }
        section
    }

    /// `setHash`: sem hash, o módulo se chama `<?>`.
    pub fn set_hash(&mut self, hash: Option<&[u8]>) {
        self.module_hash = match hash {
            Some(hash) => hash.iter().map(|&byte| byte as char).collect(),
            None => "<?>".to_string(),
        };
    }

    /// `get(functionIndexSpace)`: o nome da função (se há) e a própria seção.
    pub fn get(&self, function_index_space: usize) -> (Option<usize>, Arc<NameSection>) {
        let found = (function_index_space < self.function_names.len()).then_some(function_index_space);
        (found, Arc::new(self.clone()))
    }
}

impl Default for NameSection {
    fn default() -> NameSection {
        NameSection::create()
    }
}

/// `NameSectionParser::parse`. `payload` é o conteúdo da seção customizada `name`.
pub fn parse_name_section(payload: &[u8], info: &ModuleInformation, use_wasm_simd: bool) -> Result<NameSection, String> {
    let mut parser = ParserBase::new(payload, use_wasm_simd);
    let mut name_section = NameSection::create();
    let space_size = info.function_index_space_size();
    name_section
        .function_names
        .try_reserve_exact(space_size)
        .map_err(|_| parser.fail("can't allocate enough memory for function names"))?;
    name_section.function_names.resize(space_size, Name::new());

    let mut payload_number = 0usize;
    while parser.offset() < payload.len() {
        let name_type = parser
            .parse_uint7()
            .ok_or_else(|| parser.fail(&format!("can't get name type for payload {}", payload_number)))?;
        let payload_length = parser
            .parse_var_uint32()
            .ok_or_else(|| parser.fail(&format!("can't get payload length for payload {}", payload_number)))?
            as usize;
        if payload_length > payload.len() - parser.offset() {
            return Err(parser.fail(&format!("payload length is too big for payload {}", payload_number)));
        }
        let payload_start = parser.offset();

        let Some(name_type) = decode_name_type(name_type) else {
            // Entradas desconhecidas são ignoradas, para aceitar toolchains mais novas.
            parser.offset += payload_length;
            payload_number += 1;
            continue;
        };

        match name_type {
            NameType::Module => {
                let name_len = parser
                    .parse_var_uint32()
                    .ok_or_else(|| parser.fail(&format!("can't get module's name length for payload {}", payload_number)))?;
                name_section.module_name = consume_name(&mut parser, name_len).ok_or_else(|| {
                    parser.fail(&format!("can't get module's name of length {} for payload {}", name_len, payload_number))
                })?;
            }
            NameType::Function => {
                let count = parser
                    .parse_var_uint32()
                    .ok_or_else(|| parser.fail(&format!("can't get function count for payload {}", payload_number)))?;
                for function in 0..count {
                    let index = parser.parse_var_uint32().ok_or_else(|| {
                        parser.fail(&format!("can't get function {} index for payload {}", function, payload_number))
                    })?;
                    if space_size <= index as usize {
                        return Err(parser.fail(&format!(
                            "function {} index {} is larger than function index space {} for payload {}",
                            function, index, space_size, payload_number
                        )));
                    }
                    let name_len = parser.parse_var_uint32().ok_or_else(|| {
                        parser.fail(&format!("can't get functions {}'s name length for payload {}", function, payload_number))
                    })?;
                    let name = consume_name(&mut parser, name_len).ok_or_else(|| {
                        parser.fail(&format!(
                            "can't get function {}'s name of length {} for payload {}",
                            function, name_len, payload_number
                        ))
                    })?;
                    name_section.function_names[index as usize] = name;
                }
            }
            NameType::Local => {
                // Os nomes de locais não são usados, mas precisam ser lidos para serem ignorados.
                let function_count = parser.parse_var_uint32().ok_or_else(|| {
                    parser.fail(&format!("can't get function count for local name payload {}", payload_number))
                })?;
                for _ in 0..function_count {
                    parser.parse_var_uint32().ok_or_else(|| {
                        parser.fail(&format!("can't get local's function index for payload {}", payload_number))
                    })?;
                    let count = parser
                        .parse_var_uint32()
                        .ok_or_else(|| parser.fail(&format!("can't get local count for payload {}", payload_number)))?;
                    for local in 0..count {
                        parser.parse_var_uint32().ok_or_else(|| {
                            parser.fail(&format!("can't get local {} index for payload {}", local, payload_number))
                        })?;
                        let name_len = parser.parse_var_uint32().ok_or_else(|| {
                            parser.fail(&format!("can't get local {}'s name length for payload {}", local, payload_number))
                        })?;
                        consume_name(&mut parser, name_len).ok_or_else(|| {
                            parser.fail(&format!(
                                "can't get local {}'s name of length {} for payload {}",
                                local, name_len, payload_number
                            ))
                        })?;
                    }
                }
            }
        }
        if payload_start + payload_length != parser.offset() {
            return Err(parser.fail(&format!(
                "payload for name section is not correct size, expected {} got {}",
                payload_length,
                parser.offset().wrapping_sub(payload_start)
            )));
        }
        payload_number += 1;
    }
    Ok(name_section)
}

/// `consumeUTF8String` para `Name`.
fn consume_name(parser: &mut ParserBase<'_>, length: u32) -> Option<Name> {
    parser.consume_utf8_string(length as usize).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// `IndexOrName`. O C++ guarda uma união com tags nos bits altos; aqui é um enum, com a mesma
/// semântica de `isEmpty`/`isIndex`/`isName`.
#[derive(Clone, Debug, Default)]
pub struct IndexOrName {
    kind: IndexOrNameKind,
    name_section: Option<Arc<NameSection>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum IndexOrNameKind {
    #[default]
    Empty,
    Index(usize),
    /// A posição em `function_names`.
    Name(usize),
}

impl IndexOrName {
    /// `IndexOrName(Index, std::pair<const Name*, RefPtr<NameSection>>&&)`, com o resultado de
    /// `NameSection::get`.
    pub fn new(index: usize, name: (Option<usize>, Arc<NameSection>)) -> IndexOrName {
        let kind = match name.0 {
            Some(position) => IndexOrNameKind::Name(position),
            None => IndexOrNameKind::Index(index),
        };
        IndexOrName { kind, name_section: Some(name.1) }
    }

    pub fn is_empty(&self) -> bool {
        self.kind == IndexOrNameKind::Empty
    }

    pub fn is_index(&self) -> bool {
        matches!(self.kind, IndexOrNameKind::Index(_))
    }

    pub fn is_name(&self) -> bool {
        matches!(self.kind, IndexOrNameKind::Name(_))
    }

    pub fn index(&self) -> usize {
        match self.kind {
            IndexOrNameKind::Index(index) => index,
            _ => panic!("IndexOrName::index em valor que não é índice"),
        }
    }

    pub fn name(&self) -> &Name {
        match (self.kind, &self.name_section) {
            (IndexOrNameKind::Name(position), Some(section)) => &section.function_names[position],
            _ => panic!("IndexOrName::name em valor que não é nome"),
        }
    }

    pub fn name_section(&self) -> Option<&Arc<NameSection>> {
        self.name_section.as_ref()
    }

    /// `moduleName()`: o nome do módulo, ou o hash, ou vazio.
    pub fn module_name(&self) -> &str {
        let section = self.name_section.as_ref().expect("IndexOrName sem NameSection");
        if !section.module_name.is_empty() {
            &section.module_name
        } else if !section.module_hash.is_empty() {
            &section.module_hash
        } else {
            ""
        }
    }

    /// `dump(PrintStream&)`.
    pub fn dump(&self) -> String {
        let Some(section) = self.name_section.as_ref().filter(|_| !self.is_empty()) else {
            let mut out = "wasm-stub".to_string();
            if self.is_index() {
                out.push_str(&format!("[{}]", self.index()));
            }
            return out;
        };
        let module_name = if section.module_name.is_empty() { &section.module_hash } else { &section.module_name };
        if self.is_index() {
            format!("{}.wasm-function[{}]", module_name, self.index())
        } else {
            format!("{}.wasm-function[{}]", module_name, self.name())
        }
    }
}

/// `makeString(const IndexOrName&)`: o nome da função no stack trace.
pub fn make_string(ion: &IndexOrName) -> String {
    if ion.is_empty() || ion.name_section().is_none() {
        if ion.is_index() {
            return format!("wasm-stub[{}]", ion.index());
        }
        return "wasm-stub".to_string();
    }
    if ion.is_index() {
        format!("{}.wasm-function[{}]", ion.module_name(), ion.index())
    } else {
        format!("{}.wasm-function[{}]", ion.module_name(), ion.name())
    }
}
