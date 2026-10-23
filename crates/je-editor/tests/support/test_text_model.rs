//! Porte de `src/vs/editor/test/common/testTextModel.ts`: a criação de modelos para os testes.
//!
//! `createModelServices` do original monta o contêiner de serviços; aqui o único serviço de que o
//! `TextModel` depende é o [`UndoRedoService`], então `create_model_services` devolve um serviço novo
//! (cada modelo de teste tem o seu).

use std::rc::Rc;

use je_base::uri::URI;
use je_editor::model::{RelaxedTextModelCreationOptions, TextModelCreationOptions};
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;

/// `PLAINTEXT_LANGUAGE_ID`.
pub const PLAINTEXT_LANGUAGE_ID: &str = "plaintext";

/// `withEditorModel(text, callback)`.
pub fn with_editor_model(text: &[&str], callback: impl FnOnce(&TextModel)) {
    let model = create_text_model(&text.join("\n"));
    callback(&model);
    model.dispose();
}

/// `resolveOptions(_options)`: cada campo ausente cai no padrão do modelo.
pub fn resolve_options(options: &RelaxedTextModelCreationOptions) -> TextModelCreationOptions {
    let defaults = TextModel::default_creation_options();
    TextModelCreationOptions {
        tab_size: options.tab_size.unwrap_or(defaults.tab_size),
        indent_size: options.indent_size.unwrap_or(defaults.indent_size),
        insert_spaces: options.insert_spaces.unwrap_or(defaults.insert_spaces),
        detect_indentation: options.detect_indentation.unwrap_or(defaults.detect_indentation),
        trim_auto_whitespace: options.trim_auto_whitespace.unwrap_or(defaults.trim_auto_whitespace),
        default_eol: options.default_eol.unwrap_or(defaults.default_eol),
        is_for_simple_widget: options.is_for_simple_widget.unwrap_or(defaults.is_for_simple_widget),
        large_file_optimizations: options.large_file_optimizations.unwrap_or(defaults.large_file_optimizations),
        bracket_pair_colorization_options: options
            .bracket_colorization_options
            .unwrap_or(defaults.bracket_pair_colorization_options),
    }
}

/// `createTextModel(text)`: opções padrão, sem linguagem e sem URI.
pub fn create_text_model(text: &str) -> Rc<TextModel> {
    create_text_model_with(text, None, RelaxedTextModelCreationOptions::default(), None)
}

/// `createTextModel(text, languageId, options, uri)`.
pub fn create_text_model_with(
    text: impl Into<TextSource>,
    language_id: Option<&str>,
    options: RelaxedTextModelCreationOptions,
    uri: Option<URI>,
) -> Rc<TextModel> {
    instantiate_text_model(create_model_services(), text, language_id, options, uri)
}

/// `instantiateTextModel(instantiationService, text, languageId, options, uri)`.
pub fn instantiate_text_model(
    undo_redo_service: Rc<UndoRedoService>,
    text: impl Into<TextSource>,
    language_id: Option<&str>,
    options: RelaxedTextModelCreationOptions,
    uri: Option<URI>,
) -> Rc<TextModel> {
    TextModel::new(
        text.into(),
        language_id.unwrap_or(PLAINTEXT_LANGUAGE_ID),
        resolve_options(&options),
        uri,
        undo_redo_service,
    )
}

/// `createModelServices(disposables)`.
pub fn create_model_services() -> Rc<UndoRedoService> {
    UndoRedoService::for_tests()
}
