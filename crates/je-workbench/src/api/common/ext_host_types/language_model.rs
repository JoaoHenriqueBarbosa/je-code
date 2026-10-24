//! `LanguageModel*` e `Mcp*` de `extHostTypes.ts` (a seção `Chat`/`MCP` que o teste cobre e as classes de
//! dados do modelo de linguagem).
//!
//! Divergências:
//! - `LanguageModelChatMessage` e `LanguageModelChatMessage2` têm o mesmo comportamento em tempo de
//!   execução (a segunda só aceita também `LanguageModelThinkingPart` no tipo), então [`LanguageModelChatMessage`]
//!   é um tipo só, com [`LanguageModelChatMessage2`] como apelido. O mesmo vale para `LanguageModelToolResult`
//!   e `LanguageModelToolResult2`. As partes são o `enum` [`LanguageModelChatMessagePart`] (o `instanceof`
//!   do original).
//! - `content = string | parts[]` é [`ChatMessageContent`]; `content2` é [`ChatMessageContent2Item`].
//! - O construtor de `LanguageModelTextPart` faz `audience = audience;` (atribui o parâmetro a ele mesmo) e
//!   por isso nunca grava `this.audience`; o porte mantém o comportamento: `audience` fica `None`.
//! - `LanguageModelError` envolve um [`Error`] de `je-base` (nome `LanguageModelError`, propriedade
//!   `code`); [`serialize_language_model_error`] e [`LanguageModelError::try_deserialize`] usam o
//!   [`SerializedError`] de `je-base` mais `retryAfter`.
//! - Os `any`/`unknown` (`input`, `metadata`, `value`) são [`Dynamic`]; `Record<string, string | number |
//!   null>` do `env` de MCP é uma lista de pares.
//! - `toJSON` devolve [`Value`].

use je_base::buffer::{VSBuffer, encode_base64};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, SerializedError, transform_error_for_serialization, transform_error_from_serialization};
use je_base::json::Value;
use je_base::marshalling_ids::MarshalledId;
use je_base::mime::mimes;
use je_base::uri::URI;

use super::enums::{LanguageModelChatMessageRole, LanguageModelPartAudience};
use super::position_range::Location;
use super::{Documentation, dynamic_to_json, json_object, json_stringify_tabs};

fn mid(id: MarshalledId) -> Value {
    Value::Number(f64::from(id as i32))
}

fn audience_to_json(audience: &Option<Vec<LanguageModelPartAudience>>) -> Option<Value> {
    audience.as_ref().map(|items| Value::Array(items.iter().map(|item| Value::Number(f64::from(item.value()))).collect()))
}

/// `LanguageModelTextPart`.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageModelTextPart {
    pub value: String,
    pub audience: Option<Vec<LanguageModelPartAudience>>,
}

impl LanguageModelTextPart {
    /// `new LanguageModelTextPart(value, audience?)`: o `audience` é ignorado (ver o topo do módulo).
    pub fn new(value: impl Into<String>, _audience: Option<Vec<LanguageModelPartAudience>>) -> LanguageModelTextPart {
        LanguageModelTextPart { value: value.into(), audience: None }
    }

    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("$mid", Some(mid(MarshalledId::LanguageModelTextPart))),
            ("value", Some(Value::from(self.value.as_str()))),
            ("audience", audience_to_json(&self.audience)),
        ])
    }
}

/// `LanguageModelDataPart`.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageModelDataPart {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub audience: Option<Vec<LanguageModelPartAudience>>,
}

impl LanguageModelDataPart {
    pub fn new(data: Vec<u8>, mime_type: impl Into<String>, audience: Option<Vec<LanguageModelPartAudience>>) -> LanguageModelDataPart {
        LanguageModelDataPart { mime_type: mime_type.into(), data, audience }
    }

    pub fn image(data: Vec<u8>, mime_type: impl Into<String>) -> LanguageModelDataPart {
        LanguageModelDataPart::new(data, mime_type, None)
    }

    /// `json(value, mime = 'text/x-json')`.
    pub fn json(value: &Value, mime: Option<&str>) -> LanguageModelDataPart {
        LanguageModelDataPart::new(json_stringify_tabs(value).into_bytes(), mime.unwrap_or("text/x-json"), None)
    }

    /// `text(value, mime = Mimes.text)`.
    pub fn text(value: &str, mime: Option<&str>) -> LanguageModelDataPart {
        LanguageModelDataPart::new(value.as_bytes().to_vec(), mime.unwrap_or(mimes::TEXT), None)
    }

    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("$mid", Some(mid(MarshalledId::LanguageModelDataPart))),
            ("mimeType", Some(Value::from(self.mime_type.as_str()))),
            ("data", Some(Value::from(encode_base64(&VSBuffer::wrap(self.data.clone()), true, false).as_str()))),
            ("audience", audience_to_json(&self.audience)),
        ])
    }
}

/// `string | string[]`: o valor de um `LanguageModelThinkingPart`.
#[derive(Clone, Debug, PartialEq)]
pub enum ThinkingValue {
    Text(String),
    Lines(Vec<String>),
}

/// `LanguageModelThinkingPart`.
#[derive(Clone)]
pub struct LanguageModelThinkingPart {
    pub value: ThinkingValue,
    pub id: Option<String>,
    pub metadata: Option<Dynamic>,
}

impl LanguageModelThinkingPart {
    pub fn new(value: ThinkingValue, id: Option<String>, metadata: Option<Dynamic>) -> LanguageModelThinkingPart {
        LanguageModelThinkingPart { value, id, metadata }
    }

    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("$mid", Some(mid(MarshalledId::LanguageModelThinkingPart))),
            (
                "value",
                Some(match &self.value {
                    ThinkingValue::Text(text) => Value::from(text.as_str()),
                    ThinkingValue::Lines(lines) => Value::Array(lines.iter().map(|line| Value::from(line.as_str())).collect()),
                }),
            ),
            ("id", self.id.as_deref().map(Value::from)),
            ("metadata", self.metadata.as_ref().and_then(dynamic_to_json)),
        ])
    }
}

/// `LanguageModelPromptTsxPart`.
#[derive(Clone)]
pub struct LanguageModelPromptTsxPart {
    pub value: Dynamic,
}

impl LanguageModelPromptTsxPart {
    pub fn new(value: Dynamic) -> LanguageModelPromptTsxPart {
        LanguageModelPromptTsxPart { value }
    }

    pub fn to_json(&self) -> Value {
        json_object(vec![("$mid", Some(mid(MarshalledId::LanguageModelPromptTsxPart))), ("value", dynamic_to_json(&self.value))])
    }
}

/// `LanguageModelToolCallPart`.
#[derive(Clone)]
pub struct LanguageModelToolCallPart {
    pub call_id: String,
    pub name: String,
    pub input: Dynamic,
}

impl LanguageModelToolCallPart {
    pub fn new(call_id: impl Into<String>, name: impl Into<String>, input: Dynamic) -> LanguageModelToolCallPart {
        LanguageModelToolCallPart { call_id: call_id.into(), name: name.into(), input }
    }
}

/// `LanguageModelToolResultPart`.
#[derive(Clone)]
pub struct LanguageModelToolResultPart {
    pub call_id: String,
    pub content: Vec<Dynamic>,
    pub is_error: bool,
}

impl LanguageModelToolResultPart {
    /// `new LanguageModelToolResultPart(callId, content, isError = false)`.
    pub fn new(call_id: impl Into<String>, content: Vec<Dynamic>, is_error: Option<bool>) -> LanguageModelToolResultPart {
        LanguageModelToolResultPart { call_id: call_id.into(), content, is_error: is_error.unwrap_or(false) }
    }
}

/// As classes de parte que `LanguageModelChatMessage#content` aceita (o `instanceof` do original).
#[derive(Clone)]
pub enum LanguageModelChatMessagePart {
    Text(LanguageModelTextPart),
    ToolResult(LanguageModelToolResultPart),
    ToolCall(LanguageModelToolCallPart),
    Data(LanguageModelDataPart),
    Thinking(LanguageModelThinkingPart),
}

/// `string | parts[]`: o que o setter de `content` aceita.
#[derive(Clone)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<LanguageModelChatMessagePart>),
}

/// Um item de `content2`: `string | parts`.
#[derive(Clone)]
pub enum ChatMessageContent2Item {
    Text(String),
    Part(LanguageModelChatMessagePart),
}

/// `LanguageModelChatMessage`.
#[derive(Clone)]
pub struct LanguageModelChatMessage {
    pub role: LanguageModelChatMessageRole,
    content: Vec<LanguageModelChatMessagePart>,
    pub name: Option<String>,
}

/// `LanguageModelChatMessage2` (o mesmo comportamento em tempo de execução).
pub type LanguageModelChatMessage2 = LanguageModelChatMessage;

impl LanguageModelChatMessage {
    pub fn user(content: ChatMessageContent, name: Option<String>) -> LanguageModelChatMessage {
        LanguageModelChatMessage::new(LanguageModelChatMessageRole::User, content, name)
    }

    pub fn assistant(content: ChatMessageContent, name: Option<String>) -> LanguageModelChatMessage {
        LanguageModelChatMessage::new(LanguageModelChatMessageRole::Assistant, content, name)
    }

    pub fn new(role: LanguageModelChatMessageRole, content: ChatMessageContent, name: Option<String>) -> LanguageModelChatMessage {
        let mut message = LanguageModelChatMessage { role, content: Vec::new(), name };
        message.set_content(content);
        message
    }

    pub fn content(&self) -> &[LanguageModelChatMessagePart] {
        &self.content
    }

    /// O setter de `content`: uma `string` vira uma parte de texto (o tempo de execução continua estável,
    /// apesar da mudança de tipo na definição).
    pub fn set_content(&mut self, value: ChatMessageContent) {
        self.content = match value {
            ChatMessageContent::Text(text) => vec![LanguageModelChatMessagePart::Text(LanguageModelTextPart::new(text, None))],
            ChatMessageContent::Parts(parts) => parts,
        };
    }

    /// `content2`: as partes de texto voltam como `string`.
    pub fn content2(&self) -> Vec<ChatMessageContent2Item> {
        self.content
            .iter()
            .map(|part| match part {
                LanguageModelChatMessagePart::Text(text) => ChatMessageContent2Item::Text(text.value.clone()),
                other => ChatMessageContent2Item::Part(other.clone()),
            })
            .collect()
    }

    /// O setter de `content2`; `None` não faz nada.
    pub fn set_content2(&mut self, value: Option<Vec<ChatMessageContent2Item>>) {
        if let Some(items) = value {
            self.content = items
                .into_iter()
                .map(|item| match item {
                    ChatMessageContent2Item::Text(text) => LanguageModelChatMessagePart::Text(LanguageModelTextPart::new(text, None)),
                    ChatMessageContent2Item::Part(part) => part,
                })
                .collect();
        }
    }
}

/// `LanguageModelChatSystemMessage` (descontinuada).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageModelChatSystemMessage {
    pub content: String,
}

/// `LanguageModelChatUserMessage` (descontinuada).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageModelChatUserMessage {
    pub content: String,
    pub name: Option<String>,
}

/// `LanguageModelChatAssistantMessage` (descontinuada).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageModelChatAssistantMessage {
    pub content: String,
    pub name: Option<String>,
}

/// `LanguageModelError`.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageModelError {
    error: Error,
    code: String,
    retry_after: Option<f64>,
}

/// `SerializedLanguageModelError`: o erro serializado mais o `retryAfter`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedLanguageModelError {
    pub error: SerializedError,
    pub retry_after: Option<f64>,
}

impl LanguageModelError {
    const NAME: &'static str = "LanguageModelError";

    pub fn not_found(message: Option<&str>) -> LanguageModelError {
        LanguageModelError::new(message, Some("NotFound"), None, None)
    }

    pub fn no_permissions(message: Option<&str>) -> LanguageModelError {
        LanguageModelError::new(message, Some("NoPermissions"), None, None)
    }

    pub fn blocked(message: Option<&str>) -> LanguageModelError {
        LanguageModelError::new(message, Some("Blocked"), None, None)
    }

    pub fn rate_limited(message: Option<&str>, retry_after: Option<f64>) -> LanguageModelError {
        LanguageModelError::new(message, Some("RateLimited"), None, retry_after)
    }

    /// `tryDeserialize(data)`: `None` quando o nome não é `LanguageModelError`.
    pub fn try_deserialize(data: &SerializedLanguageModelError) -> Option<LanguageModelError> {
        if data.error.name != LanguageModelError::NAME {
            return None;
        }
        let cause = data.error.cause.as_deref().map(transform_error_from_serialization);
        Some(LanguageModelError::new(Some(&data.error.message), data.error.code.as_deref(), cause, data.retry_after))
    }

    /// `new LanguageModelError(message?, code?, cause?, retryAfter?)`: o `retryAfter` só vale se for `>= 0`.
    pub fn new(message: Option<&str>, code: Option<&str>, cause: Option<Error>, retry_after: Option<f64>) -> LanguageModelError {
        let code = code.unwrap_or("").to_owned();
        let mut error = Error::named(LanguageModelError::NAME, message.unwrap_or("")).with_prop("code", &code);
        if let Some(cause) = cause {
            error = error.with_cause(cause);
        }
        LanguageModelError { error, code, retry_after: retry_after.filter(|value| *value >= 0.0) }
    }

    /// O `Error` do JavaScript que a classe estende (`name`, `message`, `cause`).
    pub fn error(&self) -> &Error {
        &self.error
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn retry_after(&self) -> Option<f64> {
        self.retry_after
    }
}

/// `serializeLanguageModelError`: como `transformErrorForSerialization`, mas guarda o `retryAfter`.
pub fn serialize_language_model_error(error: &LanguageModelError) -> SerializedLanguageModelError {
    SerializedLanguageModelError { error: transform_error_for_serialization(&error.error), retry_after: error.retry_after }
}

/// Um item de `LanguageModelToolResult#content`.
#[derive(Clone)]
pub enum LanguageModelToolResultContent {
    Text(LanguageModelTextPart),
    PromptTsx(LanguageModelPromptTsxPart),
    Data(LanguageModelDataPart),
}

/// `LanguageModelToolResult` (e `LanguageModelToolResult2`).
#[derive(Clone)]
pub struct LanguageModelToolResult {
    pub content: Vec<LanguageModelToolResultContent>,
}

/// `LanguageModelToolResult2`.
pub type LanguageModelToolResult2 = LanguageModelToolResult;

impl LanguageModelToolResult {
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("$mid", Some(mid(MarshalledId::LanguageModelToolResult))),
            (
                "content",
                Some(Value::Array(
                    self.content
                        .iter()
                        .map(|part| match part {
                            LanguageModelToolResultContent::Text(part) => part.to_json(),
                            LanguageModelToolResultContent::PromptTsx(part) => part.to_json(),
                            LanguageModelToolResultContent::Data(part) => part.to_json(),
                        })
                        .collect(),
                )),
            ),
        ])
    }
}

/// `URI | Location`: um item de `toolResultDetails`.
#[derive(Clone)]
pub enum ToolResultDetail {
    Uri(URI),
    Location(Location),
}

/// `ExtendedLanguageModelToolResult`: o resultado base mais os campos extras.
#[derive(Clone)]
pub struct ExtendedLanguageModelToolResult {
    pub base: LanguageModelToolResult,
    pub tool_result_message: Option<Documentation>,
    pub tool_result_details: Option<Vec<ToolResultDetail>>,
    pub tool_metadata: Option<Dynamic>,
    pub has_error: Option<bool>,
}

impl ExtendedLanguageModelToolResult {
    pub fn new(content: Vec<LanguageModelToolResultContent>) -> ExtendedLanguageModelToolResult {
        ExtendedLanguageModelToolResult {
            base: LanguageModelToolResult { content },
            tool_result_message: None,
            tool_result_details: None,
            tool_metadata: None,
            has_error: None,
        }
    }
}

/// `LanguageModelToolExtensionSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageModelToolExtensionSource {
    pub id: String,
    pub label: String,
}

/// `LanguageModelToolMCPSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageModelToolMcpSource {
    pub label: String,
    pub name: String,
    pub instructions: Option<String>,
}

/// `string | number | null`: um valor do `env` de um servidor MCP.
#[derive(Clone, Debug, PartialEq)]
pub enum McpEnvValue {
    Text(String),
    Number(f64),
    Null,
}

/// `McpStdioServerDefinition`.
#[derive(Clone)]
pub struct McpStdioServerDefinition {
    pub cwd: Option<URI>,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, McpEnvValue)>,
    pub version: Option<String>,
    pub metadata: Option<Dynamic>,
}

impl McpStdioServerDefinition {
    /// `new McpStdioServerDefinition(label, command, args, env = {}, version?, metadata?)`.
    pub fn new(
        label: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        env: Option<Vec<(String, McpEnvValue)>>,
        version: Option<String>,
        metadata: Option<Dynamic>,
    ) -> McpStdioServerDefinition {
        McpStdioServerDefinition {
            cwd: None,
            label: label.into(),
            command: command.into(),
            args,
            env: env.unwrap_or_default(),
            version,
            metadata,
        }
    }
}

/// `{ providerId, scopes }`: a autenticação de um servidor MCP HTTP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpAuthentication {
    pub provider_id: String,
    pub scopes: Vec<String>,
}

/// `McpHttpServerDefinition`.
#[derive(Clone)]
pub struct McpHttpServerDefinition {
    pub label: String,
    pub uri: URI,
    pub headers: Vec<(String, String)>,
    pub version: Option<String>,
    pub metadata: Option<Dynamic>,
    pub authentication: Option<McpAuthentication>,
}

impl McpHttpServerDefinition {
    /// `new McpHttpServerDefinition(label, uri, headers = {}, version?, metadata?, authentication?)`.
    pub fn new(
        label: impl Into<String>,
        uri: URI,
        headers: Option<Vec<(String, String)>>,
        version: Option<String>,
        metadata: Option<Dynamic>,
        authentication: Option<McpAuthentication>,
    ) -> McpHttpServerDefinition {
        McpHttpServerDefinition { label: label.into(), uri, headers: headers.unwrap_or_default(), version, metadata, authentication }
    }
}
