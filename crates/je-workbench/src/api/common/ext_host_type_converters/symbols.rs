//! `extHostTypeConverters.ts`: `SymbolKind`, `SymbolTag`, `DocumentSymbol`, `DocumentHighlight`, e as
//! hierarquias de chamada e de tipo (`CallHierarchyItem`, `CallHierarchyIncomingCall`,
//! `CallHierarchyOutgoingCall`, `TypeHierarchyItem`).
//!
//! Divergências:
//! - Os `languages.SymbolKind`, `SymbolTag`, `DocumentSymbol` e `DocumentHighlight` são os de
//!   `je_editor::language_providers`. O mapeamento de `SymbolKind` é total (o `enum` da API e o do editor têm
//!   os mesmos 26 membros), então o `default` do original (`Property`) não existe.
//! - Os DTOs `ICallHierarchyItemDto` e `ITypeHierarchyItemDto` têm o mesmo formato: um único
//!   [`HierarchyItemDto`] serve aos dois, e os dois conversores saem de uma macro (a única diferença está no
//!   `detail` de `from`: o de chamada repassa, o de tipo troca `undefined` por `''`).
//! - `DocumentSymbol.to` devolve [`Result`], porque o construtor de `DocumentSymbol` valida (nome vazio,
//!   `selectionRange` fora de `range`).
//! - `DocumentSymbol.to` do original só copia `tags` quando não está vazio (`isNonEmptyArray`), e `children`
//!   quando existe (`info.children`): como a classe da API sempre tem `children`, o resultado sempre o recebe.

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::uri::URI;
use je_editor::language_providers::{
    DocumentHighlight as EditorDocumentHighlight, DocumentHighlightKind as EditorHighlightKind, DocumentSymbol as EditorDocumentSymbol,
    SymbolKind as EditorSymbolKind, SymbolTag as EditorSymbolTag,
};
use je_editor::range::Range as EditorRange;

use super::geometry::Range;
use crate::api::common::ext_host_values::ApiValue;
use crate::api::common::ext_host_types as types;

/// `typeConverters.SymbolKind`.
pub struct SymbolKind;

/// Os 26 membros iguais nos dois enums: gera `from` e `to` com `match` exaustivo.
macro_rules! symbol_kind_mapping {
    ($($variant:ident),+ $(,)?) => {
        pub fn from(kind: types::SymbolKind) -> EditorSymbolKind {
            match kind { $(types::SymbolKind::$variant => EditorSymbolKind::$variant),+ }
        }

        pub fn to(kind: EditorSymbolKind) -> types::SymbolKind {
            match kind { $(EditorSymbolKind::$variant => types::SymbolKind::$variant),+ }
        }
    };
}

impl SymbolKind {
    symbol_kind_mapping!(
        File, Module, Namespace, Package, Class, Method, Property, Field, Constructor, Enum, Interface, Function, Variable, Constant, String, Number,
        Boolean, Array, Object, Key, Null, EnumMember, Struct, Event, Operator, TypeParameter
    );
}

/// `typeConverters.SymbolTag`.
pub struct SymbolTag;

impl SymbolTag {
    pub fn from(tag: types::SymbolTag) -> EditorSymbolTag {
        match tag {
            types::SymbolTag::Deprecated => EditorSymbolTag::Deprecated,
        }
    }

    pub fn to(tag: EditorSymbolTag) -> types::SymbolTag {
        match tag {
            EditorSymbolTag::Deprecated => types::SymbolTag::Deprecated,
        }
    }
}

/// `typeConverters.DocumentSymbol`.
pub struct DocumentSymbol;

impl DocumentSymbol {
    pub fn from(info: &types::DocumentSymbol) -> EditorDocumentSymbol {
        EditorDocumentSymbol {
            name: if info.name.is_empty() { "!!MISSING: name!!".to_owned() } else { info.name.clone() },
            detail: info.detail.clone(),
            kind: SymbolKind::from(info.kind),
            tags: info.tags.as_ref().map(|tags| tags.iter().copied().map(SymbolTag::from).collect()).unwrap_or_default(),
            container_name: None,
            range: Range::from(&info.range),
            selection_range: Range::from(&info.selection_range),
            children: Some(info.children.iter().map(DocumentSymbol::from).collect()),
        }
    }

    pub fn to(info: &EditorDocumentSymbol) -> Result<types::DocumentSymbol, Error> {
        let mut result = types::DocumentSymbol::new(
            &info.name,
            &info.detail,
            SymbolKind::to(info.kind),
            Range::to(&info.range)?,
            Range::to(&info.selection_range)?,
        )?;
        if !info.tags.is_empty() {
            result.tags = Some(info.tags.iter().copied().map(SymbolTag::to).collect());
        }
        if let Some(children) = &info.children {
            result.children = children.iter().map(DocumentSymbol::to).collect::<Result<Vec<_>, _>>()?;
        }
        Ok(result)
    }
}

/// `typeConverters.DocumentHighlight`.
pub struct DocumentHighlight;

impl DocumentHighlight {
    pub fn from(highlight: &types::DocumentHighlight) -> EditorDocumentHighlight {
        let kind = match highlight.kind {
            types::DocumentHighlightKind::Text => EditorHighlightKind::Text,
            types::DocumentHighlightKind::Read => EditorHighlightKind::Read,
            types::DocumentHighlightKind::Write => EditorHighlightKind::Write,
        };
        EditorDocumentHighlight { range: Range::from(&highlight.range), kind: Some(kind) }
    }

    pub fn to(occurrence: &EditorDocumentHighlight) -> Result<types::DocumentHighlight, Error> {
        let kind = occurrence.kind.map(|kind| match kind {
            EditorHighlightKind::Text => types::DocumentHighlightKind::Text,
            EditorHighlightKind::Read => types::DocumentHighlightKind::Read,
            EditorHighlightKind::Write => types::DocumentHighlightKind::Write,
        });
        Ok(types::DocumentHighlight::new(Range::to(&occurrence.range)?, kind))
    }
}

/// `ICallHierarchyItemDto` e `ITypeHierarchyItemDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct HierarchyItemDto {
    pub session_id: String,
    pub item_id: String,
    pub kind: EditorSymbolKind,
    pub name: String,
    pub detail: Option<String>,
    pub uri: URI,
    pub range: EditorRange,
    pub selection_range: EditorRange,
    pub tags: Option<Vec<EditorSymbolTag>>,
}

impl HierarchyItemDto {
    /// O DTO como objeto, com as propriedades do original (`_sessionId`, `_itemId`, `kind`, `name`, `detail`,
    /// `uri`, `range`, `selectionRange`, `tags`); o `uri` é um valor `Uri`.
    pub fn to_dynamic(&self) -> Dynamic {
        let entries = [
            ("_sessionId", Some(Dynamic::string(&self.session_id))),
            ("_itemId", Some(Dynamic::string(&self.item_id))),
            ("kind", Some(Dynamic::from(self.kind as i32))),
            ("name", Some(Dynamic::string(&self.name))),
            ("detail", self.detail.as_deref().map(Dynamic::string)),
            ("uri", Some(self.uri.to_dynamic())),
            ("range", Some(Range::to_dto(&self.range))),
            ("selectionRange", Some(Range::to_dto(&self.selection_range))),
            ("tags", self.tags.as_ref().map(|tags| Dynamic::array(tags.iter().map(|tag| Dynamic::from(*tag as i32))))),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
    }

    /// A leitura do objeto de [`HierarchyItemDto::to_dynamic`] (ou do item que a extensão devolveu pelo RPC); `None` se faltar o
    /// `_sessionId`, o `_itemId`, o `name`, o `kind`, o `uri` ou um dos dois intervalos.
    pub fn from_dynamic(value: &Dynamic) -> Option<HierarchyItemDto> {
        let text = |key: &str| match value.get_str(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        let kind = match value.get_str("kind") {
            Dynamic::Number(kind) => SymbolKind::from(types::SymbolKind::from_value(kind as i32)?),
            _ => return None,
        };
        let tags = match value.get_str("tags") {
            Dynamic::Array(tags) => Some(
                tags.to_vec()
                    .iter()
                    .filter_map(|tag| match tag {
                        Dynamic::Number(tag) if *tag as i32 == 1 => Some(EditorSymbolTag::Deprecated),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        };
        Some(HierarchyItemDto {
            session_id: text("_sessionId")?,
            item_id: text("_itemId")?,
            kind,
            name: text("name")?,
            detail: text("detail"),
            uri: URI::from_dynamic(&value.get_str("uri")).ok()?,
            range: Range::from_dto(&value.get_str("range"))?,
            selection_range: Range::from_dto(&value.get_str("selectionRange"))?,
            tags,
        })
    }
}

/// As chamadas de [`IncomingCallDto`] e [`OutgoingCallDto`] como objeto: `{ from | to, fromRanges }`.
fn call_to_dynamic(key: &str, item: &HierarchyItemDto, from_ranges: &[EditorRange]) -> Dynamic {
    Dynamic::object([(key, item.to_dynamic()), ("fromRanges", Dynamic::array(from_ranges.iter().map(Range::to_dto)))])
}

impl IncomingCallDto {
    /// O DTO como objeto (`from`, `fromRanges`).
    pub fn to_dynamic(&self) -> Dynamic {
        call_to_dynamic("from", &self.from, &self.from_ranges)
    }

    /// A leitura do objeto de [`IncomingCallDto::to_dynamic`].
    pub fn from_dynamic(value: &Dynamic) -> Option<IncomingCallDto> {
        let Dynamic::Array(ranges) = value.get_str("fromRanges") else {
            return None;
        };
        Some(IncomingCallDto {
            from: HierarchyItemDto::from_dynamic(&value.get_str("from"))?,
            from_ranges: ranges.to_vec().iter().map(Range::from_dto).collect::<Option<Vec<_>>>()?,
        })
    }
}

impl OutgoingCallDto {
    /// O DTO como objeto (`to`, `fromRanges`).
    pub fn to_dynamic(&self) -> Dynamic {
        call_to_dynamic("to", &self.to, &self.from_ranges)
    }

    /// A leitura do objeto de [`OutgoingCallDto::to_dynamic`].
    pub fn from_dynamic(value: &Dynamic) -> Option<OutgoingCallDto> {
        let Dynamic::Array(ranges) = value.get_str("fromRanges") else {
            return None;
        };
        Some(OutgoingCallDto {
            to: HierarchyItemDto::from_dynamic(&value.get_str("to"))?,
            from_ranges: ranges.to_vec().iter().map(Range::from_dto).collect::<Option<Vec<_>>>()?,
        })
    }
}

/// O conversor de um dos dois itens de hierarquia. `$detail_defaults_to_empty` é o `item.detail ?? ''` do
/// `from` da hierarquia de tipos.
macro_rules! hierarchy_converter {
    ($converter:ident, $item:ident, $detail_defaults_to_empty:expr) => {
        pub struct $converter;

        impl $converter {
            pub fn to(item: &HierarchyItemDto) -> Result<types::$item, Error> {
                let mut result = types::$item::new(
                    SymbolKind::to(item.kind),
                    &item.name,
                    item.detail.as_deref().unwrap_or_default(),
                    item.uri.clone(),
                    Range::to(&item.range)?,
                    Range::to(&item.selection_range)?,
                );
                result.session_id = Some(item.session_id.clone());
                result.item_id = Some(item.item_id.clone());
                Ok(result)
            }

            /// `from(item, sessionId?, itemId?)`: sem os identificadores nem no argumento nem no item, lança
            /// `invalid item`.
            pub fn from(item: &types::$item, session_id: Option<&str>, item_id: Option<&str>) -> Result<HierarchyItemDto, Error> {
                let session_id = session_id.map(str::to_owned).or_else(|| item.session_id.clone());
                let item_id = item_id.map(str::to_owned).or_else(|| item.item_id.clone());
                let (Some(session_id), Some(item_id)) = (session_id, item_id) else {
                    return Err(Error::new("invalid item"));
                };
                Ok(HierarchyItemDto {
                    session_id,
                    item_id,
                    kind: SymbolKind::from(item.kind),
                    name: item.name.clone(),
                    detail: if $detail_defaults_to_empty { Some(item.detail.clone().unwrap_or_default()) } else { item.detail.clone() },
                    uri: item.uri.clone(),
                    range: Range::from(&item.range),
                    selection_range: Range::from(&item.selection_range),
                    tags: item.tags.as_ref().map(|tags| tags.iter().copied().map(SymbolTag::from).collect()),
                })
            }
        }
    };
}

hierarchy_converter!(CallHierarchyItem, CallHierarchyItem, false);
hierarchy_converter!(TypeHierarchyItem, TypeHierarchyItem, true);

/// `IIncomingCallDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct IncomingCallDto {
    pub from: HierarchyItemDto,
    pub from_ranges: Vec<EditorRange>,
}

/// `IOutgoingCallDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct OutgoingCallDto {
    pub to: HierarchyItemDto,
    pub from_ranges: Vec<EditorRange>,
}

/// `typeConverters.CallHierarchyIncomingCall`.
pub struct CallHierarchyIncomingCall;

impl CallHierarchyIncomingCall {
    pub fn to(item: &IncomingCallDto) -> Result<types::CallHierarchyIncomingCall, Error> {
        Ok(types::CallHierarchyIncomingCall::new(
            CallHierarchyItem::to(&item.from)?,
            item.from_ranges.iter().map(Range::to).collect::<Result<Vec<_>, _>>()?,
        ))
    }
}

/// `typeConverters.CallHierarchyOutgoingCall`.
pub struct CallHierarchyOutgoingCall;

impl CallHierarchyOutgoingCall {
    pub fn to(item: &OutgoingCallDto) -> Result<types::CallHierarchyOutgoingCall, Error> {
        Ok(types::CallHierarchyOutgoingCall::new(
            CallHierarchyItem::to(&item.to)?,
            item.from_ranges.iter().map(Range::to).collect::<Result<Vec<_>, _>>()?,
        ))
    }
}
