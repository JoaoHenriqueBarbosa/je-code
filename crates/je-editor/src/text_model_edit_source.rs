//! O trecho de `src/vs/editor/common/textModelEditSource.ts` que o `TextModel` e o `editTelemetry` usam: a origem de
//! uma edição (`TextModelEditSource`) e as fábricas `EditSources`.
//!
//! Representação: o `metadata` do original é um objeto literal; aqui é a lista de pares chave/valor na ordem de
//! criação (a ordem importa em `toKey`). `undefined` e `null` são `None`; números e booleanos entram como o texto
//! que o template literal do JavaScript produziria (`true`, `false`).
//!
//! A identidade de uma origem (o `===` do original, que o `EditSourceData.join` do `editTelemetry` usa) é a do
//! `Rc<TextModelEditSource>` que o chamador guarda.
//!
//! Pendente: as fábricas `rename`, `chatUndoEdits`, `chatReset`, `codeAction` com `ProviderId` completo e o
//! `EditDeltaInfo`/`EditSuggestionId.newId` entram quando o consumidor for portado.

use crate::code_action::languages::ProviderId;

/// `ITextModelEditSourceMetadata` de uma origem: pares chave/valor na ordem de criação.
pub type EditSourceMetadata = Vec<(String, Option<String>)>;

/// `TextModelEditSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextModelEditSource {
    /// `metadata.source`.
    pub source: String,
    /// `metadata.name` da origem `unknown` e `metadata.kind` da origem `cursor`.
    pub name: Option<String>,
    /// Todo o `metadata`, começando por `source`.
    pub metadata: EditSourceMetadata,
}

impl TextModelEditSource {
    /// Monta uma origem a partir do `metadata`; a primeira entrada é o `source`.
    pub fn from_metadata(name: Option<String>, metadata: Vec<(&str, Option<String>)>) -> Self {
        let metadata: EditSourceMetadata = metadata.into_iter().map(|(key, value)| (key.to_owned(), value)).collect();
        let source = metadata.first().and_then(|(_, value)| value.clone()).unwrap_or_default();
        Self { source, name, metadata }
    }

    /// `toString()`.
    pub fn to_string_value(&self) -> &str {
        &self.source
    }

    /// O valor de `metadata[key]`; `None` se a chave não existe ou é `undefined`/`null`.
    pub fn metadata_value(&self, key: &str) -> Option<&str> {
        self.metadata.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.as_deref())
    }

    /// `key in metadata`.
    pub fn has_metadata_key(&self, key: &str) -> bool {
        self.metadata.iter().any(|(k, _)| k == key)
    }

    /// `getType()`.
    pub fn get_type(&self) -> String {
        match self.source.as_str() {
            "cursor" => self.name.clone().unwrap_or_default(),
            "inlineCompletionAccept" => {
                let nes = self.metadata_value("$nes").is_some_and(|v| v == "true");
                format!("{}{}", self.source, if nes { ":nes" } else { "" })
            }
            "unknown" => self.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "unknown".to_owned()),
            _ => self.source.clone(),
        }
    }

    /// `toKey(level, filter)`: só inclui as propriedades com `level` prefixos `$` ou menos (e valor definido e não
    /// vazio); uma entrada em `filter` decide a chave sozinha.
    pub fn to_key(&self, level: usize, filter: &[(&str, bool)]) -> String {
        self.metadata
            .iter()
            .filter(|(key, value)| {
                if let Some((_, filter_val)) = filter.iter().find(|(k, _)| k == key) {
                    return *filter_val;
                }
                let prefix_count = key.matches('$').count();
                prefix_count <= level && value.as_deref().is_some_and(|v| !v.is_empty())
            })
            .map(|(key, value)| format!("{key}:{}", value.as_deref().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("-")
    }
}

/// `isAiEdit(source)`.
pub fn is_ai_edit(source: &TextModelEditSource) -> bool {
    matches!(source.source.as_str(), "inlineCompletionAccept" | "inlineCompletionPartialAccept" | "inlineChat.applyEdits" | "Chat.applyEdits")
}

/// `isUserEdit(source)`.
pub fn is_user_edit(source: &TextModelEditSource) -> bool {
    source.source == "cursor" && source.metadata_value("kind") == Some("type")
}

/// Os dados de `EditSources.inlineCompletionAccept`.
pub struct InlineCompletionAcceptData {
    pub nes: bool,
    pub request_uuid: String,
    pub language_id: String,
    pub provider_id: Option<ProviderId>,
    pub correlation_id: Option<String>,
}

/// Os dados de `EditSources.inlineCompletionPartialAccept`.
pub struct InlineCompletionPartialAcceptData {
    pub nes: bool,
    pub request_uuid: String,
    pub language_id: String,
    pub provider_id: Option<ProviderId>,
    pub correlation_id: Option<String>,
    /// `'word' | 'line'`.
    pub kind: String,
}

/// `VersionedExtensionId` (`languages.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedExtensionId {
    pub extension_id: String,
    pub version: String,
}

/// Os dados de `EditSources.chatApplyEdits`.
#[derive(Default)]
pub struct ChatApplyEditsData {
    pub model_id: Option<String>,
    pub auto_tier: Option<String>,
    pub fusion_model: Option<String>,
    pub session_id: Option<String>,
    pub request_id: Option<String>,
    pub language_id: String,
    pub mode: Option<String>,
    pub extension_id: Option<VersionedExtensionId>,
    pub code_block_suggestion_id: Option<String>,
    /// `'agentHost'`.
    pub origin: Option<String>,
    pub harness: Option<String>,
    pub agent_session_id: Option<String>,
    pub chat_session_id: Option<String>,
}

/// Os dados de `EditSources.agentHostChatApplyEdits`.
pub struct AgentHostChatApplyEditsData {
    pub model_id: Option<String>,
    pub auto_tier: Option<String>,
    pub fusion_model: Option<String>,
    pub session_id: String,
    pub agent_session_id: Option<String>,
    pub chat_session_id: Option<String>,
    pub request_id: String,
    pub harness: String,
}

/// Os dados de `EditSources.inlineChatApplyEdit`.
pub struct InlineChatApplyEditData {
    pub model_id: Option<String>,
    pub auto_tier: Option<String>,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub language_id: String,
    pub extension_id: Option<VersionedExtensionId>,
}

/// `EditSources`.
pub struct EditSources;

/// `avoidPathRedaction`: evita a detecção falsa de caminho de arquivo.
fn avoid_path_redaction(value: Option<&String>) -> Option<String> {
    value.map(|v| v.replace('/', "|"))
}

/// `toProperties(version)`: as chaves `$extensionId`, `$extensionVersion` e `$providerId`.
fn provider_properties(version: Option<&ProviderId>) -> Vec<(&'static str, Option<String>)> {
    match version {
        None => Vec::new(),
        Some(v) => vec![
            ("$extensionId", v.extension_id.clone()),
            ("$extensionVersion", v.extension_version.clone()),
            ("$providerId", v.provider_id.clone()),
        ],
    }
}

impl EditSources {
    fn simple(source: &str) -> TextModelEditSource {
        TextModelEditSource::from_metadata(None, vec![("source", Some(source.to_owned()))])
    }

    pub fn unknown(name: Option<&str>) -> TextModelEditSource {
        TextModelEditSource::from_metadata(
            name.map(str::to_owned),
            vec![("source", Some("unknown".to_owned())), ("name", name.map(str::to_owned))],
        )
    }

    pub fn set_value() -> TextModelEditSource {
        Self::simple("setValue")
    }

    pub fn eol_change() -> TextModelEditSource {
        Self::simple("eolChange")
    }

    pub fn apply_edits() -> TextModelEditSource {
        Self::simple("applyEdits")
    }

    /// `EditSources.reloadFromDisk()`.
    pub fn reload_from_disk() -> TextModelEditSource {
        Self::simple("reloadFromDisk")
    }

    /// `EditSources.snippet()`.
    pub fn snippet() -> TextModelEditSource {
        Self::simple("snippet")
    }

    /// `EditSources.suggest({ providerId })`.
    pub fn suggest(provider_id: &ProviderId) -> TextModelEditSource {
        let mut metadata = vec![("source", Some("suggest".to_owned()))];
        metadata.extend(provider_properties(Some(provider_id)));
        TextModelEditSource::from_metadata(None, metadata)
    }

    /// `EditSources.cursor({ kind, detailedSource })`: `kind` fica também em `name` (é o que `getType()` devolve para
    /// a origem `cursor`).
    pub fn cursor(kind: &str, detailed_source: Option<&str>) -> TextModelEditSource {
        TextModelEditSource::from_metadata(
            Some(kind.to_owned()),
            vec![
                ("source", Some("cursor".to_owned())),
                ("kind", Some(kind.to_owned())),
                ("detailedSource", detailed_source.map(str::to_owned)),
            ],
        )
    }

    /// `EditSources.codeAction({ kind, providerId })`: `kind` fica também em `name`.
    pub fn code_action(kind: Option<&str>, provider_id: Option<&ProviderId>) -> TextModelEditSource {
        let mut metadata = vec![("source", Some("codeAction".to_owned())), ("$kind", kind.map(str::to_owned))];
        metadata.extend(provider_properties(provider_id));
        TextModelEditSource::from_metadata(kind.map(str::to_owned), metadata)
    }

    /// `EditSources.inlineCompletionAccept(data)`.
    pub fn inline_completion_accept(data: &InlineCompletionAcceptData) -> TextModelEditSource {
        let mut metadata = vec![("source", Some("inlineCompletionAccept".to_owned())), ("$nes", Some(data.nes.to_string()))];
        metadata.extend(provider_properties(data.provider_id.as_ref()));
        metadata.push(("$$correlationId", data.correlation_id.clone()));
        metadata.push(("$$requestUuid", Some(data.request_uuid.clone())));
        metadata.push(("$$languageId", Some(data.language_id.clone())));
        TextModelEditSource::from_metadata(None, metadata)
    }

    /// `EditSources.inlineCompletionPartialAccept(data)`.
    pub fn inline_completion_partial_accept(data: &InlineCompletionPartialAcceptData) -> TextModelEditSource {
        let mut metadata = vec![
            ("source", Some("inlineCompletionPartialAccept".to_owned())),
            ("type", Some(data.kind.clone())),
            ("$nes", Some(data.nes.to_string())),
        ];
        metadata.extend(provider_properties(data.provider_id.as_ref()));
        metadata.push(("$$correlationId", data.correlation_id.clone()));
        metadata.push(("$$requestUuid", Some(data.request_uuid.clone())));
        metadata.push(("$$languageId", Some(data.language_id.clone())));
        TextModelEditSource::from_metadata(None, metadata)
    }

    /// `EditSources.chatApplyEdits(data)`.
    pub fn chat_apply_edits(data: &ChatApplyEditsData) -> TextModelEditSource {
        let mut metadata = vec![
            ("source", Some("Chat.applyEdits".to_owned())),
            ("$modelId", avoid_path_redaction(data.model_id.as_ref())),
            ("$autoTier", data.auto_tier.clone()),
            ("$fusionModel", data.fusion_model.clone()),
            ("$extensionId", data.extension_id.as_ref().map(|e| e.extension_id.clone())),
            ("$extensionVersion", data.extension_id.as_ref().map(|e| e.version.clone())),
            ("$harness", data.harness.clone()),
            ("$origin", data.origin.clone()),
            ("$$languageId", Some(data.language_id.clone())),
            ("$$sessionId", data.session_id.clone()),
        ];
        if data.agent_session_id.is_some() {
            metadata.push(("$$agentSessionId", data.agent_session_id.clone()));
        }
        if data.chat_session_id.is_some() {
            metadata.push(("$$chatSessionId", data.chat_session_id.clone()));
        }
        metadata.push(("$$requestId", data.request_id.clone()));
        metadata.push(("$$mode", data.mode.clone()));
        metadata.push(("$$codeBlockSuggestionId", data.code_block_suggestion_id.clone()));
        TextModelEditSource::from_metadata(None, metadata)
    }

    /// `EditSources.agentHostChatApplyEdits(data)`.
    pub fn agent_host_chat_apply_edits(data: &AgentHostChatApplyEditsData) -> TextModelEditSource {
        Self::chat_apply_edits(&ChatApplyEditsData {
            model_id: data.model_id.clone(),
            auto_tier: data.auto_tier.clone(),
            fusion_model: data.fusion_model.clone(),
            session_id: Some(data.session_id.clone()),
            agent_session_id: data.agent_session_id.clone(),
            chat_session_id: data.chat_session_id.clone(),
            request_id: Some(data.request_id.clone()),
            language_id: String::new(),
            mode: None,
            extension_id: None,
            code_block_suggestion_id: None,
            origin: Some("agentHost".to_owned()),
            harness: Some(data.harness.clone()),
        })
    }

    /// `EditSources.inlineChatApplyEdit(data)`.
    pub fn inline_chat_apply_edit(data: &InlineChatApplyEditData) -> TextModelEditSource {
        TextModelEditSource::from_metadata(
            None,
            vec![
                ("source", Some("inlineChat.applyEdits".to_owned())),
                ("$modelId", avoid_path_redaction(data.model_id.as_ref())),
                ("$autoTier", data.auto_tier.clone()),
                ("$extensionId", data.extension_id.as_ref().map(|e| e.extension_id.clone())),
                ("$extensionVersion", data.extension_id.as_ref().map(|e| e.version.clone())),
                ("$$sessionId", data.session_id.clone()),
                ("$$requestId", data.request_id.clone()),
                ("$$languageId", Some(data.language_id.clone())),
            ],
        )
    }
}
