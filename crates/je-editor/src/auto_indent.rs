//! Porte de `src/vs/editor/common/languages/autoIndent.ts`: o que decide a indentação automática ao
//! digitar Enter, ao colar e ao digitar um caractere (`getInheritIndentForLine`, `getGoodIndentForLine`,
//! `getIndentForEnter`, `getIndentActionForType`, `getIndentMetadata`).
//!
//! Decisões de tradução:
//! - `IVirtualModel` e `IIndentConverter` viram traits. O objeto `tokenization` do `IVirtualModel` é achatado
//!   em métodos, e o `forceTokenization` opcional tem implementação padrão que não faz nada.
//! - O `ITextModel` é o [`TextModel`] concreto, que também é um [`IVirtualModel`].
//! - As indentações são [`JsString`] (UTF-16, como o texto do modelo) e `normalizeIndentation?` do conversor
//!   devolve `Option`, porque é um membro opcional.
//! - Os parâmetros com valor padrão (`honorIntentialIndent = true`) são obrigatórios aqui.
//! - Os `/^\s*$/` e `/^\s+$/` do original viram [`is_blank`] sobre `isJsWhitespace`.

use crate::cursor_common::CursorSimpleModel;
use je_base::JsString;
use je_base::js_string::is_js_whitespace;
use je_base::strings::get_leading_whitespace;

use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::cursor_common::CursorConfiguration;
use crate::indent_rules::indent_consts;
use crate::indentation_line_processor::{
    IndentationContextProcessor, ProcessedIndentRulesSupport, is_language_different_from_line_start,
};
use crate::language_configuration::IndentAction;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::range::Range;
use crate::text_model::TextModel;

/// `IVirtualModel`: o mínimo do modelo que a avaliação da indentação lê.
pub trait IVirtualModel {
    /// `tokenization.getLineTokens`.
    fn get_line_tokens(&self, line_number: i32) -> Box<dyn IViewLineTokens>;
    /// `tokenization.getLanguageId`.
    fn get_language_id(&self) -> String;
    /// `tokenization.getLanguageIdAtPosition`.
    fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String;
    /// `tokenization.forceTokenization?`: opcional no original.
    fn force_tokenization(&self, _line_number: i32) {}
    fn get_line_content(&self, line_number: i32) -> JsString;
}

impl IVirtualModel for TextModel {
    fn get_line_tokens(&self, line_number: i32) -> Box<dyn IViewLineTokens> {
        Box::new(self.tokenization().get_line_tokens(line_number))
    }

    fn get_language_id(&self) -> String {
        self.tokenization().get_language_id()
    }

    fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        self.tokenization().get_language_id_at_position(line_number, column)
    }

    fn force_tokenization(&self, line_number: i32) {
        self.tokenization().force_tokenization(line_number);
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        <TextModel as CursorSimpleModel>::get_line_content(self, line_number)
    }
}

/// `IIndentConverter`.
pub trait IIndentConverter {
    fn shift_indent(&self, indentation: &[u16]) -> JsString;
    fn unshift_indent(&self, indentation: &[u16]) -> JsString;
    /// `normalizeIndentation?`: `None` quando o conversor não o tem.
    fn normalize_indentation(&self, _indentation: &[u16]) -> Option<JsString> {
        None
    }
}

/// O resultado de [`get_inherit_indent_for_line`]: `{ indentation, action, line? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InheritIndent {
    pub indentation: JsString,
    pub action: Option<IndentAction>,
    pub line: Option<i32>,
}

/// O resultado de [`get_indent_for_enter`]: `{ beforeEnter, afterEnter }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndentForEnter {
    pub before_enter: JsString,
    pub after_enter: JsString,
}

/// `/^\s*$/.test(text)`.
fn is_blank(text: &[u16]) -> bool {
    text.iter().all(|&unit| is_js_whitespace(unit))
}

/// `strings.getLeadingWhitespace(text)`.
fn leading_whitespace(text: &[u16]) -> JsString {
    JsString::from(get_leading_whitespace(text, 0, text.len()))
}

/// `indentation.substring(0, indentation.length - remove_text)`.
pub(crate) fn remove_trailing_units(indentation: &JsString, remove_text: i32) -> JsString {
    let len = indentation.len() as i64;
    let end = (len - i64::from(remove_text)).clamp(0, len) as usize;
    indentation.slice(0, end)
}

/// Pega a linha anterior mais próxima que não casa `unIndentedLinePattern` nem é só espaço em branco.
/// Resultado:
/// -1: esbarrou no limite de linguagens embutidas
/// 0: todas as linhas acima são inválidas
/// senão: a linha anterior mais próxima da mesma linguagem
fn get_preceding_valid_line(
    model: &dyn IVirtualModel,
    line_number: i32,
    processed_indent_rules_support: &ProcessedIndentRulesSupport,
) -> i32 {
    let language_id = model.get_language_id_at_position(line_number, 0);
    if line_number > 1 {
        let mut result_line_number = -1;

        let mut last_line_number = line_number - 1;
        while last_line_number >= 1 {
            if model.get_language_id_at_position(last_line_number, 0) != language_id {
                return result_line_number;
            }
            let text = model.get_line_content(last_line_number);
            // `/^\s+$/.test(text) || text === ''` é o mesmo que o texto ser só espaço em branco.
            if processed_indent_rules_support.should_ignore(last_line_number, None) || is_blank(text.units()) {
                result_line_number = last_line_number;
                last_line_number -= 1;
                continue;
            }

            return last_line_number;
        }
    }

    -1
}

/// A indentação herdada das linhas de cima.
/// 1. Acha a linha anterior mais próxima que não casa `unIndentedLinePattern`.
/// 2. Se ela casa `indentNextLinePattern` ou `increaseIndentPattern`, a indentação de `lineNumber` deve ser
///    um nível maior que a dela.
/// 3. Se ela não casa nenhuma regra de indentação:
///    a. confere se a linha acima dela casa `indentNextLinePattern`;
///    b. se não, a indentação desta linha é o resultado;
///    c. se sim, a indentação desta linha é *temporária*: sobe até achar uma linha cuja indentação não seja
///       temporária (o mesmo fluxo a -> b -> c).
/// 4. Senão, não há indentação herdada das de cima. Devolve `None` e a indentação de `lineNumber` não deve
///    ser mexida.
///
/// Esta função só devolve a indentação herdada das linhas de cima, não confere se a linha atual deve
/// diminuir.
pub fn get_inherit_indent_for_line(
    auto_indent: EditorAutoIndentStrategy,
    model: &dyn IVirtualModel,
    line_number: i32,
    honor_intential_indent: bool,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<InheritIndent> {
    if auto_indent < EditorAutoIndentStrategy::Full {
        return None;
    }

    let language_configuration = language_configuration_service.get_language_configuration(&model.get_language_id());
    let indent_rules_support = language_configuration.indent_rules_support.as_ref()?;
    let processed_indent_rules_support =
        ProcessedIndentRulesSupport::new(model, indent_rules_support, language_configuration_service);

    let no_indent = || InheritIndent { indentation: JsString::new(), action: None, line: None };

    if line_number <= 1 {
        return Some(no_indent());
    }

    // Sem indentação se esta é a primeira linha não vazia.
    let mut prior_line_number = line_number - 1;
    while prior_line_number > 0 {
        if !model.get_line_content(prior_line_number).is_empty() {
            break;
        }
        if prior_line_number == 1 {
            return Some(no_indent());
        }
        prior_line_number -= 1;
    }

    let preceding_un_ignored_line =
        get_preceding_valid_line(model, line_number, &processed_indent_rules_support);
    if preceding_un_ignored_line < 0 {
        return None;
    } else if preceding_un_ignored_line < 1 {
        return Some(no_indent());
    }

    let indentation_of = |line: i32| leading_whitespace(model.get_line_content(line).units());

    if processed_indent_rules_support.should_increase(preceding_un_ignored_line, None)
        || processed_indent_rules_support.should_indent_next_line(preceding_un_ignored_line, None)
    {
        Some(InheritIndent {
            indentation: indentation_of(preceding_un_ignored_line),
            action: Some(IndentAction::Indent),
            line: Some(preceding_un_ignored_line),
        })
    } else if processed_indent_rules_support.should_decrease(preceding_un_ignored_line, None) {
        Some(InheritIndent {
            indentation: indentation_of(preceding_un_ignored_line),
            action: None,
            line: Some(preceding_un_ignored_line),
        })
    } else {
        // `precedingUnIgnoredLine` não pode ser ignorada.
        // Ela não aumenta a indentação das linhas seguintes,
        // nem só a da linha seguinte,
        // então a linha atual não é afetada por ela
        // e devemos pegar a indentação herdada correta das linhas de cima.
        if preceding_un_ignored_line == 1 {
            return Some(InheritIndent {
                indentation: indentation_of(preceding_un_ignored_line),
                action: None,
                line: Some(preceding_un_ignored_line),
            });
        }

        let previous_line = preceding_un_ignored_line - 1;

        let previous_line_indent_metadata =
            indent_rules_support.get_indent_metadata(model.get_line_content(previous_line).units());
        if previous_line_indent_metadata & (indent_consts::INCREASE_MASK | indent_consts::DECREASE_MASK) == 0
            && previous_line_indent_metadata & indent_consts::INDENT_NEXTLINE_MASK != 0
        {
            let mut stop_line = 0;
            let mut i = previous_line - 1;
            while i > 0 {
                if processed_indent_rules_support.should_indent_next_line(i, None) {
                    i -= 1;
                    continue;
                }
                stop_line = i;
                break;
            }

            return Some(InheritIndent {
                indentation: indentation_of(stop_line + 1),
                action: None,
                line: Some(stop_line + 1),
            });
        }

        if honor_intential_indent {
            Some(InheritIndent {
                indentation: indentation_of(preceding_un_ignored_line),
                action: None,
                line: Some(preceding_un_ignored_line),
            })
        } else {
            // Busca a partir de `precedingUnIgnoredLine` até achar uma linha cuja indentação não seja
            // temporária.
            let mut i = preceding_un_ignored_line;
            while i > 0 {
                if processed_indent_rules_support.should_increase(i, None) {
                    return Some(InheritIndent {
                        indentation: indentation_of(i),
                        action: Some(IndentAction::Indent),
                        line: Some(i),
                    });
                } else if processed_indent_rules_support.should_indent_next_line(i, None) {
                    let mut stop_line = 0;
                    let mut j = i - 1;
                    while j > 0 {
                        // O original confere `i` (e não `j`) aqui, e o porte mantém o comportamento.
                        if processed_indent_rules_support.should_indent_next_line(i, None) {
                            j -= 1;
                            continue;
                        }
                        stop_line = j;
                        break;
                    }

                    return Some(InheritIndent {
                        indentation: indentation_of(stop_line + 1),
                        action: None,
                        line: Some(stop_line + 1),
                    });
                } else if processed_indent_rules_support.should_decrease(i, None) {
                    return Some(InheritIndent {
                        indentation: indentation_of(i),
                        action: None,
                        line: Some(i),
                    });
                }
                i -= 1;
            }

            Some(InheritIndent { indentation: indentation_of(1), action: None, line: Some(1) })
        }
    }
}

pub fn get_good_indent_for_line(
    auto_indent: EditorAutoIndentStrategy,
    virtual_model: &dyn IVirtualModel,
    language_id: &str,
    line_number: i32,
    indent_converter: &dyn IIndentConverter,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<JsString> {
    if auto_indent < EditorAutoIndentStrategy::Full {
        return None;
    }

    let rich_edit_support = language_configuration_service.get_language_configuration(language_id);

    let indent_rules_support = rich_edit_support.indent_rules_support.as_ref()?;

    let processed_indent_rules_support =
        ProcessedIndentRulesSupport::new(virtual_model, indent_rules_support, language_configuration_service);
    let indent = get_inherit_indent_for_line(
        auto_indent,
        virtual_model,
        line_number,
        true,
        language_configuration_service,
    )?;

    if let Some(inherit_line) = indent.line {
        // Aplica a regra de Enter enquanto só houver linhas em branco entre a linha herdada e esta.
        let mut should_apply_enter_rules = true;
        for in_between_line in inherit_line..line_number - 1 {
            if !is_blank(virtual_model.get_line_content(in_between_line).units()) {
                should_apply_enter_rules = false;
                break;
            }
        }
        if should_apply_enter_rules {
            let inherit_line_content = virtual_model.get_line_content(inherit_line);
            let enter_result = rich_edit_support.on_enter(auto_indent, &[], inherit_line_content.units(), &[]);

            if let Some(enter_result) = enter_result {
                let mut indentation = leading_whitespace(inherit_line_content.units());

                if let Some(remove_text) = enter_result.remove_text.filter(|&remove_text| remove_text != 0) {
                    indentation = remove_trailing_units(&indentation, remove_text);
                }

                if enter_result.indent_action == IndentAction::Indent
                    || enter_result.indent_action == IndentAction::IndentOutdent
                {
                    indentation = indent_converter.shift_indent(indentation.units());
                } else if enter_result.indent_action == IndentAction::Outdent {
                    indentation = indent_converter.unshift_indent(indentation.units());
                }

                if processed_indent_rules_support.should_decrease(line_number, None) {
                    indentation = indent_converter.unshift_indent(indentation.units());
                }

                if let Some(append_text) = enter_result.append_text.filter(|append_text| !append_text.is_empty()) {
                    let append_text: Vec<u16> = append_text.encode_utf16().collect();
                    indentation.push_units(&append_text);
                }

                return Some(leading_whitespace(indentation.units()));
            }
        }
    }

    if processed_indent_rules_support.should_decrease(line_number, None) {
        if indent.action == Some(IndentAction::Indent) {
            Some(indent.indentation)
        } else {
            Some(indent_converter.unshift_indent(indent.indentation.units()))
        }
    } else if indent.action == Some(IndentAction::Indent) {
        Some(indent_converter.shift_indent(indent.indentation.units()))
    } else {
        Some(indent.indentation)
    }
}

pub fn get_indent_for_enter(
    auto_indent: EditorAutoIndentStrategy,
    model: &TextModel,
    range: &Range,
    indent_converter: &dyn IIndentConverter,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<IndentForEnter> {
    if auto_indent < EditorAutoIndentStrategy::Full {
        return None;
    }
    let language_id = model
        .tokenization()
        .get_language_id_at_position(range.start_line_number, range.start_column);
    let language_configuration = language_configuration_service.get_language_configuration(&language_id);
    let indent_rules_support = language_configuration.indent_rules_support.as_ref()?;

    model.tokenization().force_tokenization(range.start_line_number);
    let indentation_context_processor = IndentationContextProcessor::new(model, language_configuration_service);
    let processed_context_tokens = indentation_context_processor.get_processed_token_context_around_range(range);
    let after_enter_processed_tokens = processed_context_tokens.after_range_processed_tokens;
    let before_enter_processed_tokens = processed_context_tokens.before_range_processed_tokens;
    let before_enter_indent = leading_whitespace(before_enter_processed_tokens.get_line_content().units());

    let virtual_model = VirtualModelWithModifiedTokensAtLine {
        model,
        modified_line_number: range.start_line_number,
        modified_tokens: before_enter_processed_tokens,
    };
    let language_is_different_from_line_start =
        is_language_different_from_line_start(model, range.get_start_position());
    let current_line = CursorSimpleModel::get_line_content(model, range.start_line_number);
    let current_line_indent = leading_whitespace(current_line.units());
    let after_enter_action = get_inherit_indent_for_line(
        auto_indent,
        &virtual_model,
        range.start_line_number + 1,
        true,
        language_configuration_service,
    );
    let before_enter =
        if language_is_different_from_line_start { current_line_indent.clone() } else { before_enter_indent };
    let Some(after_enter_action) = after_enter_action else {
        return Some(IndentForEnter { before_enter: before_enter.clone(), after_enter: before_enter });
    };

    let mut after_enter_indent =
        if language_is_different_from_line_start { current_line_indent } else { after_enter_action.indentation };

    if after_enter_action.action == Some(IndentAction::Indent) {
        after_enter_indent = indent_converter.shift_indent(after_enter_indent.units());
    }

    if indent_rules_support.should_decrease(after_enter_processed_tokens.get_line_content().units()) {
        after_enter_indent = indent_converter.unshift_indent(after_enter_indent.units());
    }

    Some(IndentForEnter { before_enter, after_enter: after_enter_indent })
}

/// Sempre devemos permitir a indentação intencional. Ou seja, se o usuário muda a indentação de `lineNumber`
/// e o conteúdo da linha não casa `decreaseIndentPattern`, não devemos ajustar a indentação.
pub fn get_indent_action_for_type(
    cursor_config: &CursorConfiguration,
    model: &TextModel,
    range: &Range,
    ch: &[u16],
    indent_converter: &dyn IIndentConverter,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<JsString> {
    let auto_indent = cursor_config.auto_indent;
    if auto_indent < EditorAutoIndentStrategy::Full {
        return None;
    }
    let language_is_different_from_line_start =
        is_language_different_from_line_start(model, range.get_start_position());
    if language_is_different_from_line_start {
        // Esta linha mistura linguagens e as regras de indentação não funcionam.
        return None;
    }

    let language_id = model
        .tokenization()
        .get_language_id_at_position(range.start_line_number, range.start_column);
    let language_configuration = language_configuration_service.get_language_configuration(&language_id);
    let indent_rules_support = language_configuration.indent_rules_support.as_ref()?;

    let indentation_context_processor = IndentationContextProcessor::new(model, language_configuration_service);
    let processed_context_tokens = indentation_context_processor.get_processed_token_context_around_range(range);
    let before_range_text = processed_context_tokens.before_range_processed_tokens.get_line_content();
    let after_range_text = processed_context_tokens.after_range_processed_tokens.get_line_content();
    let mut text_around_range = JsString::from(before_range_text.units());
    text_around_range.push_units(after_range_text.units());
    let mut text_around_range_with_character = JsString::from(before_range_text.units());
    text_around_range_with_character.push_units(ch);
    text_around_range_with_character.push_units(after_range_text.units());

    // Se o conteúdo anterior já casa `decreaseIndentPattern`, a indentação desta linha já deve estar ajustada.
    // O usuário pode ter mudado a indentação de propósito e devemos respeitar isso em vez de reajustar.
    if !indent_rules_support.should_decrease(text_around_range.units())
        && indent_rules_support.should_decrease(text_around_range_with_character.units())
    {
        // Depois de digitar `ch`, o conteúdo casa `decreaseIndentPattern`: ajusta a indentação para o certo.
        // 1. Pega a ação de indentação herdada
        let r = get_inherit_indent_for_line(
            auto_indent,
            model,
            range.start_line_number,
            false,
            language_configuration_service,
        )?;

        let mut indentation = r.indentation;
        if r.action != Some(IndentAction::Indent) {
            indentation = indent_converter.unshift_indent(indentation.units());
        }

        return Some(indentation);
    }

    let previous_line_number = range.start_line_number - 1;
    if range.is_empty() && previous_line_number > 0 {
        let previous_line = CursorSimpleModel::get_line_content(model, previous_line_number);
        if indent_rules_support.should_indent_next_line(previous_line.units())
            && indent_rules_support.should_increase(text_around_range_with_character.units())
        {
            let inherited_indentation_data = get_inherit_indent_for_line(
                auto_indent,
                model,
                range.start_line_number,
                false,
                language_configuration_service,
            );
            if let Some(inherited_indentation_data) = inherited_indentation_data {
                let inherited_indentation = inherited_indentation_data.indentation;
                let current_line = CursorSimpleModel::get_line_content(model, range.start_line_number);
                let actual_current_indentation = leading_whitespace(current_line.units());
                let inferred_current_indentation = indent_converter.shift_indent(inherited_indentation.units());
                // Se a indentação inferida não é igual à atual, a indentação foi mudada de propósito e fica.
                let inferred_indentation_equals_actual = inferred_current_indentation == actual_current_indentation;
                let text_around_range_contains_only_whitespace = is_blank(text_around_range.units());
                let auto_closing_pair_exists = cursor_config
                    .auto_closing_pairs
                    .auto_closing_pairs_open_by_end
                    .get(ch)
                    .is_some_and(|auto_closing_pairs| !auto_closing_pairs.is_empty());
                let is_ch_first_non_whitespace_character_and_in_auto_closing_pair =
                    auto_closing_pair_exists && text_around_range_contains_only_whitespace;
                if inferred_indentation_equals_actual && is_ch_first_non_whitespace_character_and_in_auto_closing_pair {
                    return Some(inherited_indentation);
                }
            }
        }
    }

    None
}

pub fn get_indent_metadata(
    model: &TextModel,
    line_number: i32,
    language_configuration_service: &dyn ILanguageConfigurationService,
) -> Option<u32> {
    let language_configuration =
        language_configuration_service.get_language_configuration(&model.tokenization().get_language_id());
    let indent_rules_support = language_configuration.indent_rules_support.as_ref()?;
    if line_number < 1 || line_number > model.get_line_count() {
        return None;
    }
    Some(indent_rules_support.get_indent_metadata(CursorSimpleModel::get_line_content(model, line_number).units()))
}

/// `createVirtualModelWithModifiedTokensAtLine`: o modelo com os tokens de uma linha trocados.
struct VirtualModelWithModifiedTokensAtLine<'a> {
    model: &'a TextModel,
    modified_line_number: i32,
    modified_tokens: LineTokens,
}

impl IVirtualModel for VirtualModelWithModifiedTokensAtLine<'_> {
    fn get_line_tokens(&self, line_number: i32) -> Box<dyn IViewLineTokens> {
        if line_number == self.modified_line_number {
            Box::new(self.modified_tokens.clone())
        } else {
            IVirtualModel::get_line_tokens(self.model, line_number)
        }
    }

    fn get_language_id(&self) -> String {
        IVirtualModel::get_language_id(self.model)
    }

    fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        IVirtualModel::get_language_id_at_position(self.model, line_number, column)
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        if line_number == self.modified_line_number {
            self.modified_tokens.get_line_content()
        } else {
            IVirtualModel::get_line_content(self.model, line_number)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use je_base::js_regexp::JsRegExp;

    use super::*;
    use crate::language_configuration::{CompleteEnterAction, IndentationRule, LanguageConfiguration};
    use crate::language_configuration_registry::LanguageConfigurationService;
    use crate::language_service::LanguageService;
    use crate::text_model::{TextModelServices, TextSource};
    use crate::undo_redo_service::UndoRedoService;

    fn re(pattern: &str, flags: &str) -> Rc<JsRegExp> {
        Rc::new(JsRegExp::from_str(pattern, flags).expect("regex válida"))
    }

    /// `javascriptIndentationRules` de `test/common/modes/supports/indentationRules.ts`.
    fn javascript_indentation_rules() -> IndentationRule {
        IndentationRule {
            decrease_indent_pattern: re(r##"^((?!.*?\/\*).*\*\/)?\s*[\}\]\)].*$"##, ""),
            increase_indent_pattern: re(
                r##"^((?!\/\/).)*(\{([^}"'`]*|(\t|[ ])*\/\/.*)|\([^)"'`]*|\[[^\]"'`]*)$"##,
                "",
            ),
            indent_next_line_pattern: Some(re(r##"^((.*=>\s*)|((.*[^\w]+|\s*)(if|while|for)\s*\(.*\)\s*))$"##, "")),
            un_indented_line_pattern: Some(re(
                r##"^(\t|[ ])*[ ]\*[^/]*\*\/\s*$|^(\t|[ ])*[ ]\*\/\s*$|^(\t|[ ])*[ ]\*([ ]([^\*]|\*(?!\/))*)?$"##,
                "",
            )),
        }
    }

    struct Fixture {
        service: Rc<LanguageConfigurationService>,
        model: Rc<TextModel>,
        _language_service: Rc<LanguageService>,
    }

    /// Um modelo `plaintext` com as linhas dadas e, se vierem, as regras de indentação registradas para ele.
    fn fixture(lines: &[&str], rules: Option<IndentationRule>) -> Fixture {
        let language_service = LanguageService::new(false);
        let service = LanguageConfigurationService::new(language_service.clone());
        if let Some(rules) = rules {
            let _ = service.register(
                "plaintext",
                LanguageConfiguration { indentation_rules: Some(rules), ..LanguageConfiguration::default() },
                None,
            );
        }
        let model = TextModel::new_with_language_service(
            TextSource::from(lines.join("\n").as_str()),
            "plaintext",
            TextModel::default_creation_options(),
            None,
            UndoRedoService::for_tests(),
            Some(TextModelServices {
                language_service: language_service.clone(),
                language_configuration_service: service.clone(),
            }),
        );
        Fixture { service, model, _language_service: language_service }
    }

    #[test]
    fn indent_metadata_has_one_bit_per_rule() {
        let f = fixture(
            &["function f() {", "    return 1;", "}", "if (x)", " * note"],
            Some(javascript_indentation_rules()),
        );
        let metadata = |line| get_indent_metadata(&f.model, line, &*f.service);
        assert_eq!(metadata(1), Some(indent_consts::INCREASE_MASK));
        assert_eq!(metadata(2), Some(0));
        assert_eq!(metadata(3), Some(indent_consts::DECREASE_MASK));
        assert_eq!(metadata(4), Some(indent_consts::INDENT_NEXTLINE_MASK));
        assert_eq!(metadata(5), Some(indent_consts::UNINDENT_MASK));
        assert_eq!(metadata(0), None);
        assert_eq!(metadata(6), None);
    }

    #[test]
    fn indent_metadata_is_none_without_indentation_rules() {
        let f = fixture(&["function f() {"], None);
        assert_eq!(get_indent_metadata(&f.model, 1, &*f.service), None);
    }

    #[test]
    fn processed_rules_support_applies_the_new_indentation() {
        let f = fixture(&["function f() {", "}", "* x"], Some(javascript_indentation_rules()));
        let configuration = f.service.get_language_configuration("plaintext");
        let rules = configuration.indent_rules_support.as_ref().expect("regras");
        let support = ProcessedIndentRulesSupport::new(&*f.model, rules, &*f.service);
        assert!(support.should_increase(1, None));
        assert!(!support.should_decrease(1, None));
        assert!(support.should_decrease(2, None));
        // Sem o espaço antes do `*` a linha não é ignorada; com a nova indentação ela é.
        assert!(!support.should_ignore(3, None));
        assert!(support.should_ignore(3, Some(&[0x20])));
    }

    fn text(tokens: &LineTokens) -> String {
        tokens.get_line_content().to_string_lossy()
    }

    #[test]
    fn context_processor_slices_before_after_and_previous_line() {
        let f = fixture(&["if (a) {", "  foo(bar)"], None);
        let processor = IndentationContextProcessor::new(&f.model, &*f.service);

        let context = processor.get_processed_token_context_around_range(&Range::new(2, 7, 2, 7));
        assert_eq!(text(&context.before_range_processed_tokens), "  foo(");
        assert_eq!(text(&context.after_range_processed_tokens), "bar)");
        assert_eq!(text(&context.previous_line_processed_tokens), "if (a) {");

        // Com seleção, o depois parte do fim do intervalo.
        let context = processor.get_processed_token_context_around_range(&Range::new(2, 4, 2, 7));
        assert_eq!(text(&context.before_range_processed_tokens), "  f");
        assert_eq!(text(&context.after_range_processed_tokens), "bar)");

        // Na primeira linha não há linha de cima.
        let context = processor.get_processed_token_context_around_range(&Range::new(1, 5, 1, 5));
        assert_eq!(text(&context.before_range_processed_tokens), "if (");
        assert_eq!(text(&context.after_range_processed_tokens), "a) {");
        assert_eq!(text(&context.previous_line_processed_tokens), "");
    }

    #[test]
    fn enter_action_indents_after_an_open_bracket() {
        let f = fixture(&["  foo{"], None);
        let action = crate::enter_action::get_enter_action(
            EditorAutoIndentStrategy::Advanced,
            &f.model,
            &Range::new(1, 7, 1, 7),
            &*f.service,
        );
        assert_eq!(
            action,
            Some(CompleteEnterAction {
                indent_action: IndentAction::Indent,
                append_text: "\t".to_owned(),
                remove_text: 0,
                indentation: "  ".to_owned(),
            })
        );
    }

    #[test]
    fn enter_action_indent_outdents_between_brackets() {
        let f = fixture(&["{}"], None);
        let action = crate::enter_action::get_enter_action(
            EditorAutoIndentStrategy::Advanced,
            &f.model,
            &Range::new(1, 2, 1, 2),
            &*f.service,
        );
        assert_eq!(
            action,
            Some(CompleteEnterAction {
                indent_action: IndentAction::IndentOutdent,
                append_text: "\t".to_owned(),
                remove_text: 0,
                indentation: String::new(),
            })
        );
    }
}
