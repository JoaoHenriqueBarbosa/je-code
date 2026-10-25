//! Porte parcial de `src/vs/workbench/contrib/files/browser/fileImportExport.ts`: os auxiliares de confirmação de
//! sobrescrita (`getFileOverwriteConfirm`, `getMultipleFilesOverwriteConfirm`) que o arrastar e soltar do explorador usa.
//!
//! Pendente neste módulo: `BrowserFileUpload` e `ExternalFileImport` (a importação de arquivos soltos de fora do
//! aplicativo), que dependem do `DataTransfer` nativo da janela; o `FileDragAndDrop` os recebe como um
//! [`ExternalFileDropHandler`](super::views::explorer_viewer_dnd::ExternalFileDropHandler).

use je_base::nls::localize;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogTypeOrSeverity, FileNameOrResource, get_file_names_message};
use je_platform::resources::ext_uri;

/// `getFileOverwriteConfirm`.
pub fn get_file_overwrite_confirm(name: &str) -> Confirmation {
    Confirmation {
        base: BaseDialogOptions {
            dialog_type: Some(DialogTypeOrSeverity::Severity(Severity::Warning)),
            detail: Some(DialogDetail::Text(localize("irreversible", "This action is irreversible!", &[]))),
            ..BaseDialogOptions::new(localize(
                "confirmOverwrite",
                "A file or folder with the name '{0}' already exists in the destination folder. Do you want to replace it?",
                &[name],
            ))
        },
        primary_button: Some(localize("replaceButtonLabel", "&&Replace", &[])),
        cancel_button: None,
    }
}

/// `getMultipleFilesOverwriteConfirm`.
pub fn get_multiple_files_overwrite_confirm(files: &[URI]) -> Confirmation {
    if files.len() > 1 {
        let names: Vec<FileNameOrResource> = files.iter().cloned().map(FileNameOrResource::Resource).collect();
        return Confirmation {
            base: BaseDialogOptions {
                dialog_type: Some(DialogTypeOrSeverity::Severity(Severity::Warning)),
                detail: Some(DialogDetail::Text(format!("{}\n{}", get_file_names_message(&names), localize("irreversible", "This action is irreversible!", &[])))),
                ..BaseDialogOptions::new(localize(
                    "confirmManyOverwrites",
                    "The following {0} files and/or folders already exist in the destination folder. Do you want to replace them?",
                    &[&files.len().to_string()],
                ))
            },
            primary_button: Some(localize("replaceButtonLabel", "&&Replace", &[])),
            cancel_button: None,
        };
    }

    get_file_overwrite_confirm(&ext_uri().basename(&files[0], None))
}
