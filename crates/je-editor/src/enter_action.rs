//! Porte de `src/vs/editor/common/languages/enterAction.ts`: a ação de Enter completa de uma posição
//! (`getEnterAction`).
//!
//! O `ITextModel` é o [`TextModel`] concreto. O `!richEditSupport` do original não tem equivalente, porque o
//! serviço sempre devolve uma configuração.

use crate::auto_indent::remove_trailing_units;
use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::indentation_line_processor::IndentationContextProcessor;
use crate::language_configuration::{CompleteEnterAction, IndentAction};
use crate::language_configuration_registry::{ILanguageConfigurationService, get_indentation_at_position};
use crate::line_tokens::IViewLineTokens;
use crate::range::Range;
use crate::text_model::TextModel;

pub fn get_enter_action(
    auto_indent: EditorAutoIndentStrategy,
    model: &TextModel,
    range: &Range,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<CompleteEnterAction> {
    let tokenization = model.tokenization();
    tokenization.force_tokenization(range.start_line_number);
    let language_id = tokenization.get_language_id_at_position(range.start_line_number, range.start_column);
    let rich_edit_support = language_configuration_service.get_language_configuration(&language_id);
    let indentation_context_processor = IndentationContextProcessor::new(model, language_configuration_service);
    let processed_context_tokens = indentation_context_processor.get_processed_token_context_around_range(range);
    let previous_line_text = processed_context_tokens.previous_line_processed_tokens.get_line_content();
    let before_enter_text = processed_context_tokens.before_range_processed_tokens.get_line_content();
    let after_enter_text = processed_context_tokens.after_range_processed_tokens.get_line_content();

    let enter_result = rich_edit_support.on_enter(
        auto_indent,
        previous_line_text.units(),
        before_enter_text.units(),
        after_enter_text.units(),
    )?;

    let indent_action = enter_result.indent_action;
    let remove_text = enter_result.remove_text.unwrap_or(0);

    // Aqui o `\t` entra primeiro no `appendText`, porque a ação de Enter usa `appendText` e `removeText`
    // para mudar a indentação.
    let append_text = match enter_result.append_text.filter(|append_text| !append_text.is_empty()) {
        None => {
            if indent_action == IndentAction::Indent || indent_action == IndentAction::IndentOutdent {
                "\t".to_owned()
            } else {
                String::new()
            }
        }
        Some(append_text) => {
            if indent_action == IndentAction::Indent {
                format!("\t{append_text}")
            } else {
                append_text
            }
        }
    };

    let mut indentation = get_indentation_at_position(model, range.start_line_number, range.start_column);
    if remove_text != 0 {
        indentation = remove_trailing_units(&indentation, remove_text);
    }

    Some(CompleteEnterAction {
        indent_action,
        append_text,
        remove_text,
        indentation: indentation.to_string_lossy(),
    })
}
