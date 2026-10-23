//! O trecho de `src/vs/editor/common/textModelEditSource.ts` que o `TextModel` usa: a origem de uma edição
//! (`TextModelEditSource`) e as fábricas `EditSources.setValue`, `eolChange`, `applyEdits` e `unknown`.
//!
//! Pendente: as demais fábricas de `EditSources` (chat, inline chat, completions, rename, cursor, snippet,
//! suggest, code action) dependem de tipos do `languages.ts` e de telemetria, e só os consumidores de
//! cada uma precisam delas; entram quando o consumidor for portado.

/// `TextModelEditSource`: os metadados são pares chave/valor na ordem de criação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextModelEditSource {
    pub source: String,
    pub name: Option<String>,
}

impl TextModelEditSource {
    /// `toString()`.
    pub fn to_string_value(&self) -> &str {
        &self.source
    }

    /// `getType()`.
    pub fn get_type(&self) -> String {
        if self.source == "unknown" {
            return self.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "unknown".to_owned());
        }
        if self.source == "cursor" {
            // `case 'cursor': return metadata.kind`
            return self.name.clone().unwrap_or_default();
        }
        self.source.clone()
    }
}

/// `EditSources`.
pub struct EditSources;

impl EditSources {
    fn simple(source: &str) -> TextModelEditSource {
        TextModelEditSource {
            source: source.to_owned(),
            name: None,
        }
    }

    pub fn unknown(name: Option<&str>) -> TextModelEditSource {
        TextModelEditSource {
            source: "unknown".to_owned(),
            name: name.map(str::to_owned),
        }
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

    /// `EditSources.cursor({ kind, detailedSource })`: `kind` fica em `name` (é o que `getType()` devolve para
    /// a origem `cursor`). O `detailedSource` não tem campo nesta representação simplificada e não é guardado.
    pub fn cursor(kind: &str, _detailed_source: Option<&str>) -> TextModelEditSource {
        TextModelEditSource {
            source: "cursor".to_owned(),
            name: Some(kind.to_owned()),
        }
    }
}
