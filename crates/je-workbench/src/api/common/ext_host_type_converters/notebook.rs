//! `extHostTypeConverters.ts`: `NotebookCellKind`, `NotebookCellExecutionSummary`, `NotebookCellOutputItem`,
//! `NotebookCellOutput`, `NotebookCellData` e `NotebookData`, entre as classes da API e os DTOs do protocolo.
//!
//! Divergências:
//! - `vscode.NotebookCellExecutionSummary` (`{ timing?: { startTime, endTime }, executionOrder?, success? }`) é um
//!   [`Dynamic`], como o `execution_summary` de `ext_host_types::NotebookCellData`; `to` o devolve com só as propriedades
//!   definidas (o original também deixa `undefined` as ausentes, o que é a mesma coisa para quem lê).
//! - `NotebookCellData.validate(cell)` não lança aqui (o tipo já garante o que ele confere), então `NotebookData.from`
//!   não devolve `Result`. `NotebookCellOutputItem.to` e as conversões que constroem um item devolvem `Result`: o
//!   construtor lança com um tipo MIME inválido.
//! - `Object.create(null)` (os metadados vazios) é um [`Dynamic`] objeto sem chaves.

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::glob::{PatternItem, RelativePattern};
use je_base::lifecycle::DisposableStore;

use crate::api::common::ext_host_protocol::{INotebookCellStatusBarEntryDto, NotebookCellDataDto, NotebookDataDto, NotebookOutputDto, NotebookOutputItemDto};
use crate::api::common::ext_host_type_converters::ICommandsConverter;
use crate::api::common::ext_host_types as types;
use crate::api::common::ext_host_types::IdOrMetadata;
use crate::api::common::ext_host_workspace::{GlobPattern, GlobPatternDto, glob_pattern_from};
use crate::contrib::notebook::common::notebook_common::{
    CellKind, CellStatusbarAlignment, INotebookExclusiveDocumentFilter, NotebookCellInternalMetadata, NotebookFilenamePattern, TransientOptions,
};

/// `Object.create(null)`.
fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

/// `typeConverters.NotebookCellKind`.
pub struct NotebookCellKind;

impl NotebookCellKind {
    pub fn from(data: types::NotebookCellKind) -> CellKind {
        match data {
            types::NotebookCellKind::Markup => CellKind::Markup,
            types::NotebookCellKind::Code => CellKind::Code,
        }
    }

    pub fn to(data: CellKind) -> types::NotebookCellKind {
        match data {
            CellKind::Markup => types::NotebookCellKind::Markup,
            CellKind::Code => types::NotebookCellKind::Code,
        }
    }
}

/// `typeConverters.NotebookCellExecutionSummary`.
pub struct NotebookCellExecutionSummary;

impl NotebookCellExecutionSummary {
    pub fn to(data: &NotebookCellInternalMetadata) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        if let (Some(start_time), Some(end_time)) = (data.run_start_time, data.run_end_time) {
            entries.push(("timing", Dynamic::object([("startTime", Dynamic::Number(start_time)), ("endTime", Dynamic::Number(end_time))])));
        }
        if let Some(execution_order) = data.execution_order {
            entries.push(("executionOrder", Dynamic::Number(execution_order)));
        }
        if let Some(success) = data.last_run_success {
            entries.push(("success", Dynamic::Bool(success)));
        }
        Dynamic::object(entries)
    }

    pub fn from(data: &Dynamic) -> NotebookCellInternalMetadata {
        let number = |value: Dynamic| if let Dynamic::Number(value) = value { Some(value) } else { None };
        let timing = data.get_str("timing");
        NotebookCellInternalMetadata {
            last_run_success: if let Dynamic::Bool(success) = data.get_str("success") { Some(success) } else { None },
            run_start_time: if timing.is_nullish() { None } else { number(timing.get_str("startTime")) },
            run_end_time: if timing.is_nullish() { None } else { number(timing.get_str("endTime")) },
            execution_order: number(data.get_str("executionOrder")),
            ..NotebookCellInternalMetadata::default()
        }
    }
}

/// `typeConverters.NotebookCellOutputItem`.
pub struct NotebookCellOutputItem;

impl NotebookCellOutputItem {
    pub fn from(item: &types::NotebookCellOutputItem) -> NotebookOutputItemDto {
        NotebookOutputItemDto { mime: item.mime().to_owned(), value_bytes: VSBuffer::wrap(item.data.clone()) }
    }

    pub fn to(item: &NotebookOutputItemDto) -> Result<types::NotebookCellOutputItem, Error> {
        types::NotebookCellOutputItem::new(item.value_bytes.to_vec(), &item.mime)
    }
}

/// `typeConverters.NotebookCellOutput`.
pub struct NotebookCellOutput;

impl NotebookCellOutput {
    pub fn from(output: &types::NotebookCellOutput) -> NotebookOutputDto {
        NotebookOutputDto { output_id: output.id.clone(), items: output.items.iter().map(NotebookCellOutputItem::from).collect(), metadata: output.metadata.clone() }
    }

    pub fn to(output: &NotebookOutputDto) -> Result<types::NotebookCellOutput, Error> {
        let items = output.items.iter().map(NotebookCellOutputItem::to).collect::<Result<Vec<_>, _>>()?;
        Ok(types::NotebookCellOutput::new(items, Some(IdOrMetadata::Id(output.output_id.clone())), output.metadata.clone()))
    }
}

/// `typeConverters.NotebookCellData`.
pub struct NotebookCellData;

impl NotebookCellData {
    pub fn from(data: &types::NotebookCellData) -> NotebookCellDataDto {
        NotebookCellDataDto {
            cell_kind: NotebookCellKind::from(data.kind),
            language: data.language_id.clone(),
            mime: data.mime.clone(),
            source: data.value.clone(),
            metadata: data.metadata.clone(),
            internal_metadata: Some(NotebookCellExecutionSummary::from(data.execution_summary.as_ref().unwrap_or(&empty_object()))),
            outputs: data.outputs.iter().map(NotebookCellOutput::from).collect(),
        }
    }

    pub fn to(data: &NotebookCellDataDto) -> Result<types::NotebookCellData, Error> {
        let outputs = data.outputs.iter().map(NotebookCellOutput::to).collect::<Result<Vec<_>, _>>()?;
        Ok(types::NotebookCellData::new(
            NotebookCellKind::to(data.cell_kind),
            data.source.clone(),
            data.language.clone(),
            data.mime.clone(),
            Some(outputs),
            data.metadata.clone(),
            data.internal_metadata.as_ref().map(NotebookCellExecutionSummary::to),
        ))
    }
}

/// `typeConverters.NotebookData`.
pub struct NotebookData;

/// `vscode.NotebookRegistrationData.filenamePattern[number]`: `GlobPattern | { include: GlobPattern; exclude: GlobPattern }`.
#[derive(Clone, Debug)]
pub enum NotebookFilenamePatternInput {
    Glob(GlobPattern),
    Exclusive { include: GlobPattern, exclude: GlobPattern },
}

/// `typeConverters.NotebookExclusiveDocumentPattern`.
///
/// Divergência: o `isExclusivePattern` (ter `include` e `exclude`) é a variante do tipo de entrada; o `?? undefined` e o
/// `.filter(pattern => pattern !== undefined)` do chamador não existem, porque um padrão tipado nunca é `undefined`.
/// O `IRelativePatternDto` volta como [`RelativePattern`] (`base` e `pattern`), que é o que o `INotebookContributionData` guarda.
pub struct NotebookExclusiveDocumentPattern;

impl NotebookExclusiveDocumentPattern {
    pub fn from(pattern: &NotebookFilenamePatternInput) -> NotebookFilenamePattern {
        match pattern {
            NotebookFilenamePatternInput::Exclusive { include, exclude } => NotebookFilenamePattern::Exclusive(INotebookExclusiveDocumentFilter {
                include: Some(Self::item(include)),
                exclude: Some(Self::item(exclude)),
            }),
            NotebookFilenamePatternInput::Glob(glob) => match Self::item(glob) {
                PatternItem::Str(text) => NotebookFilenamePattern::Pattern(text),
                PatternItem::Relative(relative) => NotebookFilenamePattern::Relative(relative),
            },
        }
    }

    /// `GlobPattern.from(pattern)` como item de padrão.
    fn item(pattern: &GlobPattern) -> PatternItem {
        match glob_pattern_from(pattern) {
            GlobPatternDto::Pattern(text) => PatternItem::Str(text),
            GlobPatternDto::Relative(dto) => PatternItem::Relative(RelativePattern { base: dto.base, pattern: dto.pattern }),
        }
    }
}

/// `typeConverters.NotebookStatusBarItem`.
pub struct NotebookStatusBarItem;

impl NotebookStatusBarItem {
    pub fn from(
        item: &types::NotebookCellStatusBarItem,
        commands_converter: &dyn ICommandsConverter,
        disposables: &DisposableStore,
    ) -> Result<INotebookCellStatusBarEntryDto, Error> {
        let command = match &item.command {
            Some(types::NotebookCellStatusBarCommand::Id(id)) => Some(types::Command::new("", id.as_str())),
            Some(types::NotebookCellStatusBarCommand::Command(command)) => Some(command.clone()),
            None => None,
        };
        Ok(INotebookCellStatusBarEntryDto {
            alignment: if item.alignment == types::NotebookCellStatusBarAlignment::Left { CellStatusbarAlignment::Left } else { CellStatusbarAlignment::Right },
            command: commands_converter.to_internal(command.as_ref(), disposables)?,
            text: item.text.clone(),
            tooltip: item.tooltip.clone(),
            accessibility_information: item.accessibility_information.clone(),
            priority: item.priority,
        })
    }
}

/// `typeConverters.NotebookDocumentContentOptions`.
pub struct NotebookDocumentContentOptions;

impl NotebookDocumentContentOptions {
    pub fn from(options: Option<&types::NotebookDocumentContentOptions>) -> TransientOptions {
        TransientOptions {
            transient_outputs: options.and_then(|options| options.transient_outputs).unwrap_or(false),
            transient_cell_metadata: options.and_then(|options| options.transient_cell_metadata.clone()).unwrap_or_default(),
            transient_document_metadata: options.and_then(|options| options.transient_document_metadata.clone()).unwrap_or_default(),
            cell_content_metadata: options.and_then(|options| options.cell_content_metadata.clone()).unwrap_or_default(),
        }
    }
}

impl NotebookData {
    pub fn from(data: &types::NotebookData) -> NotebookDataDto {
        NotebookDataDto { metadata: data.metadata.clone().unwrap_or_else(empty_object), cells: data.cells.iter().map(NotebookCellData::from).collect() }
    }

    pub fn to(data: &NotebookDataDto) -> Result<types::NotebookData, Error> {
        let cells = data.cells.iter().map(NotebookCellData::to).collect::<Result<Vec<_>, _>>()?;
        let mut res = types::NotebookData::new(cells);
        if !data.metadata.own_keys().is_empty() {
            res.metadata = Some(data.metadata.clone());
        }
        Ok(res)
    }
}
