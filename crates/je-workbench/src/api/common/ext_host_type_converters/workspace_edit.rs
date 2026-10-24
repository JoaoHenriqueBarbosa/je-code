//! `extHostTypeConverters.ts`: `WorkspaceEdit`, a conversão entre o `vscode.WorkspaceEdit` e o `IWorkspaceEditDto` do protocolo.
//!
//! Divergências:
//! - `from(value, versionInfo?)` recebe um [`Option`] de [`IVersionInformationProvider`]; o `value instanceof types.WorkspaceEdit`
//!   do original é garantido pelo tipo. O `contents` de uma operação de arquivo que o original diferencia (`ArrayBuffer.isView` contra
//!   `DataTransferFile`) é sempre os bytes de `FileOperationOptions.contents` aqui, que viram `base64`: a variante
//!   `dataTransferItem` não tem como nascer de uma [`types::FileOperationOptions`].
//! - Os metadados `vscode.WorkspaceEditEntryMetadata` viram o `WorkspaceEditMetadata` do editor ([`workspace_edit_metadata`]); o
//!   `iconPath` que o original repassa como veio é convertido para o [`languages::IconPath`](EditorIconPath) de mesma forma.
//! - O `cellEdit` de uma edição `Cell` é o `ICellPartialMetadataEdit` (de célula) ou o `IDocumentMetadataEdit` (do documento) de
//!   `notebook_common`; o de `CellReplace` leva os `ICellDto2` (o `NotebookCellDataDto` mais o estado de colapso, ausente aqui):
//!   [`cell_dto2_from`]. O `index` e o `count` viram `usize`.
//! - `to` reconstrói só as edições de texto (ou snippet) e as operações de arquivo, como o original: a operação sem `oldResource`
//!   ou sem `newResource` (que o original passaria a `renameFile` como `undefined`, e que a classe guarda como criação ou exclusão) usa
//!   `createFile` ou `deleteFile`, que guardam a mesma entrada; as edições de célula de notebook são ignoradas, pois o original só
//!   entende `textEdit` e trata o resto como operação de arquivo (uma `IWorkspaceNotebookCellEdit` não tem `oldResource`).
//!   Os `contents` e os metadados não passam por `to`, como no original.

use je_base::JsString;
use je_base::buffer::{VSBuffer, decode_base64, encode_base64};
use je_base::errors::Error;
use je_base::map::ResourceSet;
use je_base::uri::URI;
use je_editor::code_action::languages::{IWorkspaceTextEdit, IconPath as EditorIconPath, TextEdit as EditorTextEdit, WorkspaceEditMetadata};

use super::edits::TextEdit;
use super::geometry::Range;
use super::notebook::NotebookCellData;
use crate::api::common::ext_host_protocol::{
    IWorkspaceEditDto, IWorkspaceFileEditDto, NotebookCellDataDto, WorkspaceEditEntryDto, WorkspaceFileEditContentsDto, WorkspaceFileEditOptionsDto,
};
use crate::api::common::ext_host_types as types;
use crate::api::common::ext_host_types::{SetEditItem, SnippetString, SnippetTextEdit, WorkspaceEditEntry};
use crate::contrib::notebook::common::notebook_common::{
    ICellDto2, ICellPartialMetadataEdit, ICellReplaceEdit, IDocumentMetadataEdit, IOutputDto, IOutputItemDto, IWorkspaceNotebookCellEdit,
    WorkspaceNotebookCellEditOperation,
};

/// `WorkspaceEdit.IVersionInformationProvider`.
pub trait IVersionInformationProvider {
    fn get_text_document_version(&self, uri: &URI) -> Option<i64>;
    fn get_notebook_document_version(&self, uri: &URI) -> Option<i64>;
}

/// `vscode.WorkspaceEditEntryMetadata` para o `WorkspaceEditMetadata` do editor.
pub fn workspace_edit_metadata(metadata: &Option<types::WorkspaceEditEntryMetadata>) -> Option<WorkspaceEditMetadata> {
    metadata.as_ref().map(|metadata| WorkspaceEditMetadata {
        needs_confirmation: metadata.needs_confirmation,
        label: metadata.label.clone(),
        description: metadata.description.clone(),
        icon_path: metadata.icon_path.as_ref().map(|icon_path| match icon_path {
            types::IconPath::Uri(uri) => EditorIconPath::Uri(uri.clone()),
            types::IconPath::LightDark { light, dark } => EditorIconPath::LightDark { light: light.clone(), dark: dark.clone() },
            types::IconPath::ThemeIcon(icon) => EditorIconPath::ThemeIcon(icon.clone()),
        }),
    })
}

/// O `NotebookCellDataDto` como o `ICellDto2` que o `ICellReplaceEdit` carrega.
pub fn cell_dto2_from(cell: NotebookCellDataDto) -> ICellDto2 {
    ICellDto2 {
        source: JsString::from(cell.source),
        language: cell.language,
        mime: cell.mime,
        cell_kind: cell.cell_kind,
        outputs: cell
            .outputs
            .into_iter()
            .map(|output| IOutputDto {
                outputs: output.items.into_iter().map(|item| IOutputItemDto { mime: item.mime, data: item.value_bytes }).collect(),
                output_id: output.output_id,
                metadata: output.metadata,
            })
            .collect(),
        metadata: cell.metadata,
        internal_metadata: cell.internal_metadata,
        collapse_state: None,
    }
}

/// `typeConverters.WorkspaceEdit`.
pub struct WorkspaceEdit;

impl WorkspaceEdit {
    pub fn from(value: &types::WorkspaceEdit, version_info: Option<&dyn IVersionInformationProvider>) -> IWorkspaceEditDto {
        let mut result = IWorkspaceEditDto { edits: Vec::new() };
        let text_version = |uri: &URI| version_info.and_then(|info| info.get_text_document_version(uri));
        let notebook_version = |uri: &URI| version_info.and_then(|info| info.get_notebook_document_version(uri));

        // collect all files that are to be created so that their version
        // information (in case they exist as text model already) can be ignored
        let mut to_create = ResourceSet::new(None);
        for entry in value.all_entries() {
            if let WorkspaceEditEntry::File(file) = entry
                && let (Some(to), None) = (&file.to, &file.from)
            {
                to_create.add(to.clone());
            }
        }

        for entry in value.all_entries() {
            match entry {
                WorkspaceEditEntry::File(file) => {
                    let options = file.options.clone().unwrap_or_default();
                    let contents = options.contents.map(|bytes| WorkspaceFileEditContentsDto::Base64(encode_base64(&VSBuffer::wrap(bytes), true, false)));

                    // file operation
                    result.edits.push(WorkspaceEditEntryDto::File(IWorkspaceFileEditDto {
                        old_resource: file.from.clone(),
                        new_resource: file.to.clone(),
                        options: Some(WorkspaceFileEditOptionsDto {
                            overwrite: options.overwrite,
                            ignore_if_not_exists: options.ignore_if_not_exists,
                            ignore_if_exists: options.ignore_if_exists,
                            recursive: options.recursive,
                            contents,
                            ..WorkspaceFileEditOptionsDto::default()
                        }),
                        metadata: workspace_edit_metadata(&file.metadata),
                    }));
                }
                WorkspaceEditEntry::Text(text) => {
                    // text edits
                    result.edits.push(WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
                        resource: text.uri.clone(),
                        text_edit: TextEdit::from(&text.edit),
                        version_id: if to_create.has(&text.uri) { None } else { text_version(&text.uri) },
                        metadata: workspace_edit_metadata(&text.metadata),
                    }));
                }
                WorkspaceEditEntry::Snippet(snippet) => {
                    result.edits.push(WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
                        resource: snippet.uri.clone(),
                        text_edit: EditorTextEdit {
                            range: Range::from(&snippet.range),
                            text: snippet.edit.value.clone(),
                            eol: None,
                            insert_as_snippet: Some(true),
                            keep_whitespace: snippet.keep_whitespace,
                        },
                        version_id: if to_create.has(&snippet.uri) { None } else { text_version(&snippet.uri) },
                        metadata: workspace_edit_metadata(&snippet.metadata),
                    }));
                }
                WorkspaceEditEntry::Cell(cell) => {
                    // cell edit
                    let cell_edit = match &cell.edit {
                        types::CellMetadataEdit::Document { metadata } => WorkspaceNotebookCellEditOperation::DocumentMetadata(IDocumentMetadataEdit { metadata: metadata.clone() }),
                        types::CellMetadataEdit::Cell { index, metadata } => {
                            WorkspaceNotebookCellEditOperation::PartialMetadata(ICellPartialMetadataEdit { index: *index as usize, metadata: metadata.clone() })
                        }
                    };
                    result.edits.push(WorkspaceEditEntryDto::Cell(IWorkspaceNotebookCellEdit {
                        metadata: workspace_edit_metadata(&cell.metadata),
                        resource: cell.uri.clone(),
                        notebook_version_id: notebook_version(&cell.uri),
                        cell_edit,
                    }));
                }
                WorkspaceEditEntry::CellReplace(replace) => {
                    // cell replace
                    result.edits.push(WorkspaceEditEntryDto::Cell(IWorkspaceNotebookCellEdit {
                        metadata: workspace_edit_metadata(&replace.metadata),
                        resource: replace.uri.clone(),
                        notebook_version_id: notebook_version(&replace.uri),
                        cell_edit: WorkspaceNotebookCellEditOperation::Replace(ICellReplaceEdit {
                            index: replace.index as usize,
                            count: replace.count as usize,
                            cells: replace.cells.iter().map(|cell| cell_dto2_from(NotebookCellData::from(cell))).collect(),
                        }),
                    }));
                }
            }
        }
        result
    }

    pub fn to(value: &IWorkspaceEditDto) -> Result<types::WorkspaceEdit, Error> {
        let mut result = types::WorkspaceEdit::new();
        let mut edits: Vec<(URI, Vec<SetEditItem>)> = Vec::new();
        for edit in &value.edits {
            match edit {
                WorkspaceEditEntryDto::Text(item) => {
                    let uri = item.resource.clone();
                    let range = Range::to(&item.text_edit.range)?;
                    let text = &item.text_edit.text;

                    let edit_or_snippet: SetEditItem = if item.text_edit.insert_as_snippet == Some(true) {
                        let mut snippet = SnippetTextEdit::new(range, SnippetString::new(Some(text.as_str())));
                        snippet.keep_whitespace = item.text_edit.keep_whitespace;
                        snippet.into()
                    } else {
                        types::TextEdit::replace(range, text).into()
                    };

                    match edits.iter_mut().find(|(candidate, _)| candidate.to_string(false) == uri.to_string(false)) {
                        Some((_, array)) => array.push(edit_or_snippet),
                        None => edits.push((uri, vec![edit_or_snippet])),
                    }
                }
                WorkspaceEditEntryDto::File(file) => {
                    let options = file.options.as_ref().map(|options| types::FileOperationOptions {
                        overwrite: options.overwrite,
                        ignore_if_exists: options.ignore_if_exists,
                        ignore_if_not_exists: options.ignore_if_not_exists,
                        recursive: options.recursive,
                        contents: match &options.contents {
                            Some(WorkspaceFileEditContentsDto::Base64(value)) => Some(decode_base64(value).to_vec()),
                            _ => None,
                        },
                    });
                    match (&file.old_resource, &file.new_resource) {
                        (Some(old), Some(new)) => result.rename_file(old.clone(), new.clone(), options, None),
                        (None, Some(new)) => result.create_file(new.clone(), options, None),
                        (Some(old), None) => result.delete_file(old.clone(), options, None),
                        (None, None) => return Err(Error::type_error("WorkspaceEdit.to: a operação de arquivo não tem oldResource nem newResource")),
                    }
                }
                WorkspaceEditEntryDto::Cell(_) => {}
            }
        }

        for (uri, array) in edits {
            result.set(&uri, array);
        }
        Ok(result)
    }
}
