//! Porte de `src/vs/editor/common/textModelEvents.ts`: os eventos do `TextModel` (conteúdo, decorações,
//! opções) e os eventos crus que o view model consome.
//!
//! Decisões: `IModelContentChange` do original vira [`ModelContentChange`] (os quatro campos que o
//! `mirrorTextModel.ts` declara; o `forceMoveMarkers` que os objetos reais carregam fica só no
//! [`crate::model::InternalModelContentChange`]); `eol: string` vira [`Eol`]; `ModelRawChange` vira um
//! `enum`.
//!
//! Pendente: os eventos de tokenização (`IModelTokensChangedEvent`, `IModelFontTokensChangedEvent`,
//! `FontTokensUpdate`), de linguagem (`IModelLanguageChangedEvent`, `IModelLanguageConfigurationChangedEvent`),
//! `ISerializedModelContentChangedEvent` (depende do renderizador de linhas).

use je_base::JsString;

use crate::model::{Eol, InjectedTextOptions, InternalModelContentChange, ModelDecoration};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model_edit_source::TextModelEditSource;

/// `IModelContentChange`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelContentChange {
    /// O intervalo antigo que foi trocado.
    pub range: Range,
    /// O deslocamento do intervalo trocado.
    pub range_offset: usize,
    /// O comprimento do intervalo trocado.
    pub range_length: usize,
    /// O texto novo.
    pub text: JsString,
}

impl From<&InternalModelContentChange> for ModelContentChange {
    fn from(change: &InternalModelContentChange) -> Self {
        Self {
            range: change.range,
            range_offset: change.range_offset,
            range_length: change.range_length,
            text: change.text.clone(),
        }
    }
}

/// `IModelContentChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelContentChangedEvent {
    /// As mudanças ordenadas do fim do documento para o começo, seguras para aplicar em sequência.
    pub changes: Vec<ModelContentChange>,
    /// O novo fim de linha.
    pub eol: Eol,
    /// A nova versão do modelo.
    pub version_id: i64,
    pub is_undoing: bool,
    pub is_redoing: bool,
    /// Todas as decorações se perderam: o modelo foi reiniciado com um valor novo.
    pub is_flush: bool,
    /// O evento descreve uma troca de fim de linha.
    pub is_eol_change: bool,
    pub detailed_reasons: Vec<TextModelEditSource>,
    /// A soma dos comprimentos é `changes.len()`; o vetor tem o mesmo tamanho de `detailed_reasons`.
    pub detailed_reasons_change_lengths: Vec<usize>,
}

/// `IModelDecorationsChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelDecorationsChangedEvent {
    pub affects_minimap: bool,
    pub affects_overview_ruler: bool,
    pub affects_glyph_margin: bool,
    pub affects_line_number: bool,
}

pub use crate::model::ModelOptionsChangedEvent;

/// `RawContentChangedType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawContentChangedType {
    Flush = 1,
    LineChanged = 2,
    LinesDeleted = 3,
    LinesInserted = 4,
    EolChanged = 5,
}

/// `ModelRawLineChanged`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelRawLineChanged {
    /// A linha que mudou (antes da edição).
    pub line_number: i32,
    /// O número novo da linha (depois da edição).
    pub line_number_post_edit: i32,
}

/// `ModelRawLinesDeleted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelRawLinesDeleted {
    pub from_line_number: i32,
    pub to_line_number: i32,
    pub last_untouched_line_post_edit: i32,
}

/// `ModelRawLinesInserted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelRawLinesInserted {
    pub from_line_number: i32,
    pub from_line_number_post_edit: i32,
    pub count: i32,
}

impl ModelRawLinesInserted {
    pub fn to_line_number(&self) -> i32 {
        self.from_line_number + self.count - 1
    }

    pub fn to_line_number_post_edit(&self) -> i32 {
        self.from_line_number_post_edit + self.count - 1
    }
}

/// `ModelRawChange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRawChange {
    /// `ModelRawFlush`: o modelo foi reiniciado com um valor novo.
    Flush,
    LineChanged(ModelRawLineChanged),
    LinesDeleted(ModelRawLinesDeleted),
    LinesInserted(ModelRawLinesInserted),
    /// `ModelRawEOLChanged`.
    EolChanged,
}

impl ModelRawChange {
    pub fn change_type(&self) -> RawContentChangedType {
        match self {
            ModelRawChange::Flush => RawContentChangedType::Flush,
            ModelRawChange::LineChanged(_) => RawContentChangedType::LineChanged,
            ModelRawChange::LinesDeleted(_) => RawContentChangedType::LinesDeleted,
            ModelRawChange::LinesInserted(_) => RawContentChangedType::LinesInserted,
            ModelRawChange::EolChanged => RawContentChangedType::EolChanged,
        }
    }
}

/// `ModelRawContentChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRawContentChangedEvent {
    pub changes: Vec<ModelRawChange>,
    pub version_id: i64,
    pub is_undoing: bool,
    pub is_redoing: bool,
    pub resulting_selection: Option<Vec<Selection>>,
}

impl ModelRawContentChangedEvent {
    pub fn new(changes: Vec<ModelRawChange>, version_id: i64, is_undoing: bool, is_redoing: bool) -> Self {
        Self {
            changes,
            version_id,
            is_undoing,
            is_redoing,
            resulting_selection: None,
        }
    }

    pub fn contains_event(&self, change_type: RawContentChangedType) -> bool {
        self.changes.iter().any(|change| change.change_type() == change_type)
    }

    pub fn merge(a: &ModelRawContentChangedEvent, b: &ModelRawContentChangedEvent) -> ModelRawContentChangedEvent {
        let mut changes = a.changes.clone();
        changes.extend_from_slice(&b.changes);
        ModelRawContentChangedEvent::new(changes, b.version_id, a.is_undoing || b.is_undoing, a.is_redoing || b.is_redoing)
    }
}

/// `ModelInjectedTextChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInjectedTextChangedEvent {
    pub changes: Vec<ModelRawLineChanged>,
}

/// `ModelLineHeightChanged`.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLineHeightChanged {
    pub owner_id: i32,
    pub decoration_id: String,
    pub line_number: i32,
    pub line_height_multiplier: Option<f64>,
}

/// `ModelLineHeightChangedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLineHeightChangedEvent {
    pub changes: Vec<ModelLineHeightChanged>,
}

impl ModelLineHeightChangedEvent {
    /// `affects(range)`.
    pub fn affects_range(&self, range: &Range) -> bool {
        self.changes
            .iter()
            .any(|change| change.line_number >= range.start_line_number && change.line_number <= range.end_line_number)
    }

    /// `affects(position)`.
    pub fn affects_position(&self, position: &Position) -> bool {
        self.changes.iter().any(|change| change.line_number == position.line_number)
    }
}

/// `ModelFontChanged`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelFontChanged {
    pub owner_id: i32,
    pub line_number: i32,
}

/// `ModelFontChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelFontChangedEvent {
    pub changes: Vec<ModelFontChanged>,
}

/// `InternalModelContentChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalModelContentChangeEvent {
    pub raw_content_changed_event: ModelRawContentChangedEvent,
    pub content_changed_event: ModelContentChangedEvent,
}

impl InternalModelContentChangeEvent {
    pub fn merge(&self, other: &InternalModelContentChangeEvent) -> InternalModelContentChangeEvent {
        InternalModelContentChangeEvent {
            raw_content_changed_event: ModelRawContentChangedEvent::merge(
                &self.raw_content_changed_event,
                &other.raw_content_changed_event,
            ),
            content_changed_event: Self::merge_change_events(&self.content_changed_event, &other.content_changed_event),
        }
    }

    fn merge_change_events(a: &ModelContentChangedEvent, b: &ModelContentChangedEvent) -> ModelContentChangedEvent {
        let mut changes = a.changes.clone();
        changes.extend(b.changes.iter().cloned());
        let mut detailed_reasons = a.detailed_reasons.clone();
        detailed_reasons.extend(b.detailed_reasons.iter().cloned());
        let mut detailed_reasons_change_lengths = a.detailed_reasons_change_lengths.clone();
        detailed_reasons_change_lengths.extend(b.detailed_reasons_change_lengths.iter().copied());
        ModelContentChangedEvent {
            changes,
            eol: b.eol,
            version_id: b.version_id,
            is_undoing: a.is_undoing || b.is_undoing,
            is_redoing: a.is_redoing || b.is_redoing,
            is_flush: a.is_flush || b.is_flush,
            // Os dois precisam ser verdadeiros para não confundir quem ignora esse tipo de edição.
            is_eol_change: a.is_eol_change && b.is_eol_change,
            detailed_reasons,
            detailed_reasons_change_lengths,
        }
    }
}

/// `occupiesHorizontalSpace`: o texto injetado ocupa espaço pelo conteúdo ou, se for só largura, pelo
/// espaço horizontal que reserva.
fn occupies_horizontal_space(options: &InjectedTextOptions) -> bool {
    !options.content.is_empty() || options.width_in_em.is_some_and(|width| width > 0.0)
}

/// `FixedWidthInjectedTextRange`: um trecho do texto com os textos injetados aplicados que é desenhado
/// com largura fixa (`widthInEm` vezes o tamanho da fonte).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedWidthInjectedTextRange {
    pub start_offset: usize,
    pub end_offset: usize,
    pub width_in_em: f64,
}

/// `LineInjectedText`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineInjectedText {
    pub owner_id: i32,
    pub line_number: i32,
    pub column: i32,
    pub options: InjectedTextOptions,
    pub order: i32,
}

impl LineInjectedText {
    /// `applyInjectedText(lineText, injectedTexts)`. As colunas são unidades UTF-16.
    pub fn apply_injected_text(line_text: &JsString, injected_texts: Option<&[LineInjectedText]>) -> JsString {
        let Some(injected_texts) = injected_texts.filter(|texts| !texts.is_empty()) else {
            return line_text.clone();
        };
        let units = line_text.units();
        let mut result = JsString::default();
        let mut last_original_offset = 0usize;
        for injected_text in injected_texts {
            let end = ((injected_text.column - 1).max(0) as usize).min(units.len());
            let start = last_original_offset.min(end);
            result.push_units(&units[start..end]);
            last_original_offset = (injected_text.column - 1).max(0) as usize;
            result.push_units(&injected_text.options.content.encode_utf16().collect::<Vec<u16>>());
        }
        result.push_units(&units[last_original_offset.min(units.len())..]);
        result
    }

    /// `getFixedWidthInjectedTextRanges(injectedTexts)`: os trechos de `applyInjectedText(...)` desenhados
    /// com largura fixa. Um texto injetado só de largura produz um trecho vazio
    /// (`start_offset == end_offset`), que reserva espaço horizontal sem cobrir caractere algum.
    ///
    /// `injected_texts` precisa estar ordenado por coluna, como `fromDecorations` produz e
    /// `applyInjectedText` já exige. O resultado fica ordenado por `start_offset` e sem sobreposição.
    pub fn get_fixed_width_injected_text_ranges(
        injected_texts: Option<&[LineInjectedText]>,
    ) -> Vec<FixedWidthInjectedTextRange> {
        let mut result = Vec::new();
        let mut injected_text_length = 0usize;
        for injected_text in injected_texts.unwrap_or_default() {
            let length = injected_text.options.content.encode_utf16().count();
            let start_offset = (injected_text.column - 1).max(0) as usize + injected_text_length;
            let end_offset = start_offset + length;
            if let Some(width_in_em) = injected_text.options.width_in_em {
                result.push(FixedWidthInjectedTextRange { start_offset, end_offset, width_in_em });
            }
            injected_text_length += length;
        }
        result
    }

    /// `fromDecorations(decorations)`.
    pub fn from_decorations(decorations: &[ModelDecoration]) -> Vec<LineInjectedText> {
        let mut result = Vec::new();
        for decoration in decorations {
            if let Some(before) = decoration.options.before.as_ref().filter(|b| occupies_horizontal_space(b)) {
                result.push(LineInjectedText {
                    owner_id: decoration.owner_id,
                    line_number: decoration.range.start_line_number,
                    column: decoration.range.start_column,
                    options: before.as_ref().clone(),
                    order: 0,
                });
            }
            if let Some(after) = decoration.options.after.as_ref().filter(|a| occupies_horizontal_space(a)) {
                result.push(LineInjectedText {
                    owner_id: decoration.owner_id,
                    line_number: decoration.range.end_line_number,
                    column: decoration.range.end_column,
                    options: after.as_ref().clone(),
                    order: 1,
                });
            }
        }
        result.sort_by(|a, b| {
            a.line_number
                .cmp(&b.line_number)
                .then(a.column.cmp(&b.column))
                .then(a.order.cmp(&b.order))
        });
        result
    }

    /// `withText(text)`.
    pub fn with_text(&self, text: &str) -> LineInjectedText {
        let mut options = self.options.clone();
        options.content = text.to_owned();
        LineInjectedText {
            options,
            ..self.clone()
        }
    }
}

/// `IModelLanguageChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLanguageChangedEvent {
    /// Previous language
    pub old_language: String,
    /// New language
    pub new_language: String,
    /// Source of the call that caused the event.
    pub source: String,
}

/// `IModelLanguageConfigurationChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelLanguageConfigurationChangedEvent;

/// `IModelTokensChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTokensChangedEvent {
    pub semantic_tokens_applied: bool,
    pub ranges: Vec<ModelTokensChangedRange>,
}

/// Um intervalo de linhas de [`ModelTokensChangedEvent`] (`{ fromLineNumber, toLineNumber }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelTokensChangedRange {
    /// The start of the range (inclusive)
    pub from_line_number: i32,
    /// The end of the range (inclusive)
    pub to_line_number: i32,
}

/// `IModelFontTokensChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelFontTokensChangedEvent {
    pub changes: crate::languages::FontTokensUpdate,
}
