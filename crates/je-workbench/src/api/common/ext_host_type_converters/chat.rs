//! `extHostTypeConverters.ts`: os conversores de chat que não dependem do resto da camada de IA:
//! `ChatResponseVoiceProgressPart`, `ChatLanguageModelToolReference`/`ChatLanguageModelToolReferences`,
//! `ChatRequestModeInstructions` e `ChatAgentResult`.
//!
//! Divergências:
//! - `ChatAgentResult.to` revive as partes de modelo de linguagem do `metadata` (`reviveMetadata`): como um [`Dynamic`] não guarda
//!   uma instância de classe, a parte revivida é um `Dynamic` objeto cuja carga nativa é a parte (`ObjectRef::with_host`), que o
//!   chamador recupera com `ObjectRef::host::<LanguageModelDataPart>()` (o `instanceof` do original). Os itens de um
//!   `LanguageModelToolResult` revivido que não são partes de texto, `PromptTsx` ou dados são descartados (o tipo não os tem).
//!   `IChatAgentResult` e `vscode.ChatResult` têm só os campos que o conversor lê.
//! - A camada de chat (`contrib/chat`) está fora do objetivo atual, então os tipos de que estes conversores dependem
//!   ficam definidos aqui, só com os campos que os conversores leem: `IChatVoiceProgressPart` é
//!   [`ChatVoiceProgressPartDto`], `IChatRequestModeInstructions` é [`IChatRequestModeInstructions`] e
//!   `ChatRequestToolReferenceEntry` é [`ChatRequestToolReferenceEntry`] (`IChatRequestToolEntry` e
//!   `IChatRequestToolSetEntry` sem os campos de ícone, de origem e de `fullName`, que nenhum conversor lê).
//! - A classe `ChatResponseVoiceProgressPart` de `extHostTypes.ts` é [`ExtHostChatResponseVoiceProgressPart`] (o nome que o
//!   teste do original lhe dá ao importá-la), porque o espaço de nomes conversor tem o mesmo nome dela.
//! - `Dto<IChatRequestModeInstructions>` (os `UriComponents` no lugar de `URI`) é o mesmo tipo que
//!   [`IChatRequestModeInstructions`], pois aqui um `UriComponents` já é um [`URI`]: `to` serve aos dois casos, e o
//!   `revive` das referências de ferramenta é a identidade.
//! - `metadata?: Record<string, boolean | string | number>` é [`Dynamic`]; `value?` de uma referência de ferramenta é
//!   [`Dynamic`] (sempre `undefined` no uso real, mas `ChatLanguageModelToolReference.to` lança quando ele é verdadeiro).
//! - `toolReferences` do conversor `to` lança `Error('Invalid tool reference')` e `Error('Invalid tool reference in prompt
//!   variables')`: o resultado é `Result`.

use std::rc::Rc;

use je_base::buffer::decode_base64;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::marshalling_ids::MarshalledId;
use je_base::objects::clone_and_change;
use je_base::uri::URI;
use je_editor::offset_range::OffsetRange;

use crate::api::common::ext_host_types::{
    LanguageModelDataPart, LanguageModelPartAudience, LanguageModelPromptTsxPart, LanguageModelTextPart, LanguageModelThinkingPart, LanguageModelToolResult,
    LanguageModelToolResultContent, ThinkingValue,
};

/// `vscode.ChatResponseVoiceProgressPart` (a classe de `extHostTypes.ts`). O `ChatResponseVoiceProgressStage` do `id` é um texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtHostChatResponseVoiceProgressPart {
    pub id: String,
    pub value: String,
}

impl ExtHostChatResponseVoiceProgressPart {
    pub fn new(id: impl Into<String>, value: impl Into<String>) -> Self {
        Self { id: id.into(), value: value.into() }
    }
}

/// `Dto<IChatVoiceProgressPart>`: `{ kind: 'voiceProgress', id, value }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatVoiceProgressPartDto {
    pub kind: &'static str,
    pub id: String,
    pub value: String,
}

/// `typeConverters.ChatResponseVoiceProgressPart`.
pub struct ChatResponseVoiceProgressPart;

impl ChatResponseVoiceProgressPart {
    pub fn from(part: &ExtHostChatResponseVoiceProgressPart) -> ChatVoiceProgressPartDto {
        ChatVoiceProgressPartDto { kind: "voiceProgress", id: part.id.clone(), value: part.value.clone() }
    }
}

/// `IChatRequestToolEntry` (só o que os conversores leem).
#[derive(Clone, Debug)]
pub struct ChatRequestToolEntry {
    pub id: String,
    pub name: String,
    pub value: Option<Dynamic>,
    pub range: Option<OffsetRange>,
}

/// `IChatRequestToolSetEntry` (só o que os conversores leem): o `value` é a lista de ferramentas do conjunto.
#[derive(Clone, Debug)]
pub struct ChatRequestToolSetEntry {
    pub id: String,
    pub name: String,
    pub value: Vec<ChatRequestToolEntry>,
    pub range: Option<OffsetRange>,
}

/// `ChatRequestToolReferenceEntry` (`kind: 'tool' | 'toolset'`).
#[derive(Clone, Debug)]
pub enum ChatRequestToolReferenceEntry {
    Tool(ChatRequestToolEntry),
    ToolSet(ChatRequestToolSetEntry),
}

/// `IChatRequestModeInstructions`.
#[derive(Clone, Debug)]
pub struct IChatRequestModeInstructions {
    pub uri: Option<URI>,
    pub name: String,
    pub content: String,
    pub tool_references: Vec<ChatRequestToolReferenceEntry>,
    pub allowed_subagents: Option<Vec<String>>,
    pub metadata: Option<Dynamic>,
    pub is_builtin: Option<bool>,
}

/// `vscode.ChatLanguageModelToolReference`: `{ name, range?: [number, number] }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLanguageModelToolReferenceApi {
    pub name: String,
    pub range: Option<(i32, i32)>,
}

/// `vscode.ChatRequestModeInstructions`.
#[derive(Clone, Debug)]
pub struct ChatRequestModeInstructionsApi {
    pub uri: Option<URI>,
    pub name: String,
    pub content: String,
    pub tool_references: Option<Vec<ChatLanguageModelToolReferenceApi>>,
    pub allowed_subagents: Option<Vec<String>>,
    pub metadata: Option<Dynamic>,
    pub is_builtin: Option<bool>,
}

/// A verdade de `if (value)` num valor JavaScript.
fn is_truthy(value: &Dynamic) -> bool {
    match value {
        Dynamic::Undefined | Dynamic::Null => false,
        Dynamic::Bool(value) => *value,
        Dynamic::Number(value) => *value != 0.0 && !value.is_nan(),
        Dynamic::BigInt(value) => *value != 0,
        Dynamic::String(value) => !value.units().is_empty(),
        _ => true,
    }
}

fn range_pair(range: Option<OffsetRange>) -> Option<(i32, i32)> {
    range.map(|range| (range.start, range.end_exclusive))
}

/// `typeConverters.ChatLanguageModelToolReference.to(variable)`: lança quando a entrada tem `value`.
fn tool_reference_to(id: &str, value: Option<&Dynamic>, range: Option<OffsetRange>) -> Result<ChatLanguageModelToolReferenceApi, Error> {
    if value.is_some_and(is_truthy) {
        return Err(Error::new("Invalid tool reference"));
    }

    Ok(ChatLanguageModelToolReferenceApi { name: id.to_owned(), range: range_pair(range) })
}

/// `typeConverters.ChatLanguageModelToolReference`.
pub struct ChatLanguageModelToolReference;

impl ChatLanguageModelToolReference {
    pub fn to(variable: &ChatRequestToolEntry) -> Result<ChatLanguageModelToolReferenceApi, Error> {
        tool_reference_to(&variable.id, variable.value.as_ref(), variable.range)
    }
}

/// `ChatLanguageModelToolReferences` (privado no original).
struct ChatLanguageModelToolReferences;

impl ChatLanguageModelToolReferences {
    fn to(variables: &[ChatRequestToolReferenceEntry]) -> Result<Vec<ChatLanguageModelToolReferenceApi>, Error> {
        let mut tool_references = Vec::new();
        for variable in variables {
            match variable {
                ChatRequestToolReferenceEntry::Tool(tool) => tool_references.push(ChatLanguageModelToolReference::to(tool)?),
                ChatRequestToolReferenceEntry::ToolSet(tool_set) => {
                    for tool in &tool_set.value {
                        tool_references.push(ChatLanguageModelToolReference::to(tool)?);
                    }
                }
            }
        }
        Ok(tool_references)
    }
}

/// `typeConverters.ChatRequestModeInstructions`.
pub struct ChatRequestModeInstructions;

impl ChatRequestModeInstructions {
    pub fn to(mode: Option<&IChatRequestModeInstructions>) -> Result<Option<ChatRequestModeInstructionsApi>, Error> {
        let Some(mode) = mode else {
            return Ok(None);
        };

        Ok(Some(ChatRequestModeInstructionsApi {
            uri: mode.uri.clone(),
            name: mode.name.clone(),
            content: mode.content.clone(),
            tool_references: Some(ChatLanguageModelToolReferences::to(&mode.tool_references)?),
            allowed_subagents: mode.allowed_subagents.clone(),
            metadata: mode.metadata.clone(),
            is_builtin: mode.is_builtin,
        }))
    }

    pub fn from(mode: Option<&ChatRequestModeInstructionsApi>) -> Option<IChatRequestModeInstructions> {
        let mode = mode?;
        Some(IChatRequestModeInstructions {
            uri: mode.uri.clone(),
            name: mode.name.clone(),
            content: mode.content.clone(),
            tool_references: mode
                .tool_references
                .as_ref()
                .map(|references| {
                    references
                        .iter()
                        .map(|reference| {
                            ChatRequestToolReferenceEntry::Tool(ChatRequestToolEntry {
                                id: reference.name.clone(),
                                name: reference.name.clone(),
                                value: None,
                                range: reference.range.map(|(start, end_exclusive)| OffsetRange { start, end_exclusive }),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            allowed_subagents: mode.allowed_subagents.clone(),
            metadata: mode.metadata.clone(),
            is_builtin: mode.is_builtin,
        })
    }
}

// ---- ChatAgentResult ------------------------------------------------------------------------------------

/// `IChatAgentResult` (só os campos que o conversor lê).
#[derive(Clone, Debug, Default)]
pub struct IChatAgentResult {
    pub error_details: Option<Dynamic>,
    pub metadata: Option<Dynamic>,
    pub next_question: Option<Dynamic>,
    pub details: Option<String>,
}

/// `vscode.ChatResult`.
#[derive(Clone, Debug, Default)]
pub struct ChatResult {
    pub error_details: Option<Dynamic>,
    pub metadata: Option<Dynamic>,
    pub next_question: Option<Dynamic>,
    pub details: Option<String>,
}

/// O objeto revivido de uma parte de modelo de linguagem: um [`Dynamic`] objeto cuja carga nativa é a parte (recuperada com
/// `ObjectRef::host::<T>()`), que faz o papel do `instanceof` do original.
fn host_object<T: 'static>(part: T) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(part)))
}

fn string_of(value: &Dynamic) -> Option<String> {
    if let Dynamic::String(text) = value { Some(text.to_string_lossy()) } else { None }
}

/// O `Uint8Array` de um `LanguageModelDataPart` serializado: o formato antigo (`{ type: 'Buffer', data: number[] }`, de antes
/// do #303151), o texto base64, ou vazio se for outra coisa ou se o base64 for inválido.
fn revived_data_bytes(data: &Dynamic) -> Vec<u8> {
    match data {
        Dynamic::Object(_) if string_of(&data.get_str("type")).as_deref() == Some("Buffer") && data.get_str("data").is_array() => {
            let Dynamic::Array(items) = data.get_str("data") else {
                return Vec::new();
            };
            items.to_vec().iter().map(|item| if let Dynamic::Number(byte) = item { *byte as u8 } else { 0 }).collect()
        }
        Dynamic::String(encoded) => errors::catch(|| decode_base64(&encoded.to_string_lossy()).to_vec()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// O `value => ...` de `reviveMetadata`: `undefined` quando o valor não é uma parte marcada com `$mid`.
fn revive_part(value: &Dynamic) -> Dynamic {
    let is_mid = |id: MarshalledId| matches!(value.get_str("$mid"), Dynamic::Number(mid) if mid == f64::from(id as i32));

    if is_mid(MarshalledId::LanguageModelToolResult) {
        let content = revive_metadata(&value.get_str("content"));
        let items = if let Dynamic::Array(items) = &content { items.to_vec() } else { Vec::new() };
        host_object(LanguageModelToolResult {
            content: items
                .iter()
                .filter_map(|item| {
                    let Dynamic::Object(object) = item else {
                        return None;
                    };
                    object
                        .host::<LanguageModelTextPart>()
                        .map(|part| LanguageModelToolResultContent::Text((*part).clone()))
                        .or_else(|| object.host::<LanguageModelPromptTsxPart>().map(|part| LanguageModelToolResultContent::PromptTsx((*part).clone())))
                        .or_else(|| object.host::<LanguageModelDataPart>().map(|part| LanguageModelToolResultContent::Data((*part).clone())))
                })
                .collect(),
        })
    } else if is_mid(MarshalledId::LanguageModelTextPart) {
        host_object(LanguageModelTextPart::new(string_of(&value.get_str("value")).unwrap_or_default(), None))
    } else if is_mid(MarshalledId::LanguageModelThinkingPart) {
        let thinking = match value.get_str("value") {
            Dynamic::Array(lines) => ThinkingValue::Lines(lines.to_vec().iter().filter_map(string_of).collect()),
            other => ThinkingValue::Text(string_of(&other).unwrap_or_default()),
        };
        let metadata = value.get_str("metadata");
        host_object(LanguageModelThinkingPart::new(thinking, string_of(&value.get_str("id")), if metadata.is_undefined() { None } else { Some(metadata) }))
    } else if is_mid(MarshalledId::LanguageModelPromptTsxPart) {
        host_object(LanguageModelPromptTsxPart::new(value.get_str("value")))
    } else if is_mid(MarshalledId::LanguageModelDataPart) {
        let audience = match value.get_str("audience") {
            Dynamic::Array(items) => Some(
                items
                    .to_vec()
                    .iter()
                    .filter_map(|item| if let Dynamic::Number(number) = item { LanguageModelPartAudience::from_value(*number as i32) } else { None })
                    .collect(),
            ),
            _ => None,
        };
        host_object(LanguageModelDataPart::new(revived_data_bytes(&value.get_str("data")), string_of(&value.get_str("mimeType")).unwrap_or_default(), audience))
    } else {
        Dynamic::Undefined
    }
}

/// `reviveMetadata(metadata)`.
fn revive_metadata(metadata: &Dynamic) -> Dynamic {
    clone_and_change(metadata, &revive_part)
}

/// `typeConverters.ChatAgentResult`.
pub struct ChatAgentResult;

impl ChatAgentResult {
    pub fn to(result: &IChatAgentResult) -> ChatResult {
        ChatResult {
            error_details: result.error_details.clone(),
            metadata: result.metadata.as_ref().map(revive_metadata),
            next_question: result.next_question.clone(),
            details: result.details.clone(),
        }
    }

    pub fn from(result: &ChatResult) -> IChatAgentResult {
        IChatAgentResult {
            error_details: result.error_details.clone(),
            metadata: result.metadata.clone(),
            next_question: result.next_question.clone(),
            details: result.details.clone(),
        }
    }
}
