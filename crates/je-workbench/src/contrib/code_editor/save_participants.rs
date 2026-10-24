//! Porte de `src/vs/workbench/contrib/codeEditor/browser/saveParticipants.ts`: os participantes do salvar que acertam o
//! espaço em branco e as linhas finais do arquivo.
//!
//! Recorte e divergências:
//! - **Portados**: `TrimWhitespaceParticipant`, `FinalNewLineParticipant`, `TrimFinalNewLinesParticipant` e o `findEditor`.
//!   Ficam de fora `FormatOnSaveParticipant`, `CodeActionOnSaveParticipant`, `SaveParticipantsContribution` e o registro
//!   da contribuição: dependem de `format.ts`, `codeAction.ts`, `IHostService` e do registro de contribuições do
//!   workbench, que ainda não têm porte.
//! - **`@IConfigurationService` e `@ICodeEditorService`** são os serviços recebidos pelo construtor. O `ICodeEditorService`
//!   do porte (`opener_service.rs`) é um recorte sem `listCodeEditors`, que mora no [`CodeEditorRegistry`]; o construtor
//!   recebe um `Option` dele (o `undefined!` do teste do original é `None`, e `findEditor` só consulta o serviço quando o
//!   modelo está ligado a um editor).
//! - **`getValue('files.x', { overrideIdentifier, resource })`** é `get_value` com [`ConfigurationOverrides`], e o resultado
//!   passa pelo `is_truthy` do JavaScript.
//! - **`participate(...)` async** devolve a promessa já resolvida (não há `await` no corpo).
//! - **`pushEditOperations(prevSelection, ops, () => prevSelection)`** é o `push_edit_operations` com um calculador de
//!   cursor que devolve a seleção anterior; `() => null` devolve `None`.
//! - **`strings.lastNonWhitespaceIndex(lastLine) === -1`** é `last_non_whitespace_index(units, len - 1)` devolver `None`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Truthy;
use je_base::promise::Promise;
use je_base::strings::last_non_whitespace_index;
use je_base::JsString;
use je_editor::code_editor_widget::{CodeEditorWidget, EndCursorState, ExecuteEditsSource, ICodeEditor};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::EditOperation;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::services::abstract_code_editor_service::CodeEditorRegistry;
use je_editor::snippet::snippet_controller2::SnippetController2;
use je_editor::text_model::TextModel;
use je_editor::trim_trailing_whitespace_command::trim_trailing_whitespace;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::ConfigurationOverrides;

use crate::common::editor::SaveReason;
use crate::services::textfile::collaborators::{IProgress, ProgressStep};
use crate::services::textfile::textfiles::{ITextFileEditorModel, ITextFileSaveParticipant, TextFileSaveParticipantContext};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `{ overrideIdentifier: model.textEditorModel.getLanguageId(), resource: model.resource }`.
fn overrides_of(model: &dyn ITextFileEditorModel, text_model: &TextModel) -> ConfigurationOverrides {
    ConfigurationOverrides { override_identifier: Some(JsString::from(text_model.get_language_id().as_str())), resource: Some(model.resource()) }
}

/// `findEditor(model, codeEditorService)`: o editor que mostra o modelo, preferindo o que tem o foco do texto.
fn find_editor(model: &Rc<TextModel>, code_editor_service: &Option<Rc<CodeEditorRegistry>>) -> Option<Rc<CodeEditorWidget>> {
    let mut candidate: Option<Rc<CodeEditorWidget>> = None;

    if model.is_attached_to_editor() {
        if let Some(code_editor_service) = code_editor_service {
            for editor in code_editor_service.list_code_editors() {
                if editor.has_model() && editor.get_model().is_some_and(|editor_model| Rc::ptr_eq(&editor_model, model)) {
                    if editor.has_text_focus() {
                        return Some(editor); // favour focused editor if there are multiple
                    }

                    candidate = Some(editor);
                }
            }
        }
    }

    candidate
}

/// `TrimWhitespaceParticipant`.
pub struct TrimWhitespaceParticipant {
    configuration_service: Rc<dyn IConfigurationService>,
    code_editor_service: Option<Rc<CodeEditorRegistry>>,
}

impl TrimWhitespaceParticipant {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, code_editor_service: Option<Rc<CodeEditorRegistry>>) -> Self {
        Self { configuration_service, code_editor_service }
    }

    fn do_trim_trailing_whitespace(&self, model: &Rc<TextModel>, is_auto_saved: bool, trim_in_regexes_and_strings: bool) {
        let mut prev_selection: Vec<Selection> = Vec::new();
        let mut cursors: Vec<Position> = Vec::new();

        if let Some(editor) = find_editor(model, &self.code_editor_service) {
            // Find `prevSelection` in any case do ensure a good undo stack when pushing the edit
            // Collect active cursors in `cursors` only if `isAutoSaved` to avoid having the cursors jump
            prev_selection = editor.get_selections().unwrap_or_default();
            if is_auto_saved {
                cursors = prev_selection.iter().map(|selection| selection.get_position()).collect();
                if let Some(snippets_range) = SnippetController2::get(&editor).and_then(|controller| controller.get_session_enclosing_range()) {
                    for line_number in snippets_range.start_line_number..=snippets_range.end_line_number {
                        cursors.push(Position::new(line_number, model.get_line_max_column(line_number)));
                    }
                }
            }
        }

        let ops = trim_trailing_whitespace(model, &mut cursors, trim_in_regexes_and_strings);
        if ops.is_empty() {
            return; // Nothing to do
        }

        let selection_to_restore = prev_selection.clone();
        model.push_edit_operations(Some(prev_selection), &ops, Some(&mut |_edits| Some(selection_to_restore.clone())), None, None);
    }
}

impl ITextFileSaveParticipant for TrimWhitespaceParticipant {
    fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        let Some(text_model) = model.text_editor_model() else {
            return Promise::resolved(());
        };

        let overrides = overrides_of(&*model, &text_model);
        let trim_trailing_whitespace_option = self.configuration_service.get_value(Some("files.trimTrailingWhitespace"), Some(&overrides)).is_truthy();
        let trim_in_regex_and_strings = self.configuration_service.get_value(Some("files.trimTrailingWhitespaceInRegexAndStrings"), Some(&overrides)).is_truthy();
        if trim_trailing_whitespace_option {
            self.do_trim_trailing_whitespace(&text_model, context.reason == SaveReason::Auto, trim_in_regex_and_strings);
        }
        Promise::resolved(())
    }
}

/// `FinalNewLineParticipant`.
pub struct FinalNewLineParticipant {
    configuration_service: Rc<dyn IConfigurationService>,
    code_editor_service: Option<Rc<CodeEditorRegistry>>,
}

impl FinalNewLineParticipant {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, code_editor_service: Option<Rc<CodeEditorRegistry>>) -> Self {
        Self { configuration_service, code_editor_service }
    }

    fn do_insert_final_new_line(&self, model: &Rc<TextModel>) {
        let line_count = model.get_line_count();
        let last_line = model.get_line_content(line_count);
        let units = last_line.units();
        let last_line_is_empty_or_whitespace = last_non_whitespace_index(units, units.len() as i64 - 1).is_none();

        if line_count == 0 || last_line_is_empty_or_whitespace {
            return;
        }

        let eol = String::from_utf16_lossy(model.get_eol().as_units());
        let edits = vec![EditOperation::insert(Position::new(line_count, model.get_line_max_column(line_count)), eol.as_str())];
        match find_editor(model, &self.code_editor_service) {
            Some(editor) => {
                editor.execute_edits(ExecuteEditsSource::Name(Some("insertFinalNewLine")), edits, editor.get_selections().map(EndCursorState::Selections));
            }
            None => {
                model.push_edit_operations(Some(Vec::new()), &edits, Some(&mut |_edits| None), None, None);
            }
        }
    }
}

impl ITextFileSaveParticipant for FinalNewLineParticipant {
    fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        _context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        let Some(text_model) = model.text_editor_model() else {
            return Promise::resolved(());
        };

        let overrides = overrides_of(&*model, &text_model);
        if self.configuration_service.get_value(Some("files.insertFinalNewline"), Some(&overrides)).is_truthy() {
            self.do_insert_final_new_line(&text_model);
        }
        Promise::resolved(())
    }
}

/// `TrimFinalNewLinesParticipant`.
pub struct TrimFinalNewLinesParticipant {
    configuration_service: Rc<dyn IConfigurationService>,
    code_editor_service: Option<Rc<CodeEditorRegistry>>,
}

impl TrimFinalNewLinesParticipant {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, code_editor_service: Option<Rc<CodeEditorRegistry>>) -> Self {
        Self { configuration_service, code_editor_service }
    }

    /// Devolve 0 se o arquivo inteiro está vazio.
    fn find_last_non_empty_line(model: &TextModel) -> i32 {
        for line_number in (1..=model.get_line_count()).rev() {
            let line_length = model.get_line_length(line_number);
            if line_length > 0 {
                // this line has content
                return line_number;
            }
        }
        // no line has content
        0
    }

    fn do_trim_final_new_lines(&self, model: &Rc<TextModel>, is_auto_saved: bool) {
        let line_count = model.get_line_count();

        // Do not insert new line if file does not end with new line
        if line_count == 1 {
            return;
        }

        let mut prev_selection: Vec<Selection> = Vec::new();
        let mut cannot_touch_line_number = 0;
        let editor = find_editor(model, &self.code_editor_service);
        if let Some(editor) = &editor {
            prev_selection = editor.get_selections().unwrap_or_default();
            if is_auto_saved {
                for selection in &prev_selection {
                    if selection.position_line_number > cannot_touch_line_number {
                        cannot_touch_line_number = selection.position_line_number;
                    }
                }
            }
        }

        let last_non_empty_line = Self::find_last_non_empty_line(model);
        let delete_from_line_number = (last_non_empty_line + 1).max(cannot_touch_line_number + 1);
        let deletion_range = model.validate_range(&Range::new(delete_from_line_number, 1, line_count, model.get_line_max_column(line_count)));

        if deletion_range.is_empty() {
            return;
        }

        let selection_to_restore = prev_selection.clone();
        model.push_edit_operations(
            Some(prev_selection.clone()),
            &[EditOperation::delete(deletion_range)],
            Some(&mut |_edits| Some(selection_to_restore.clone())),
            None,
            None,
        );

        if let Some(editor) = editor {
            editor.set_selections(&prev_selection, None, None);
        }
    }
}

impl ITextFileSaveParticipant for TrimFinalNewLinesParticipant {
    fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        let Some(text_model) = model.text_editor_model() else {
            return Promise::resolved(());
        };

        let overrides = overrides_of(&*model, &text_model);
        if self.configuration_service.get_value(Some("files.trimFinalNewlines"), Some(&overrides)).is_truthy() {
            self.do_trim_final_new_lines(&text_model, context.reason == SaveReason::Auto);
        }
        Promise::resolved(())
    }
}
