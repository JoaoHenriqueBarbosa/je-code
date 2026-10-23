//! Porte de `src/vs/editor/common/languages/supports/indentationLineProcessor.ts`: o processamento das
//! linhas para a avaliação das regras de indentação (`ProcessedIndentRulesSupport`,
//! `IndentationContextProcessor`, `isLanguageDifferentFromLineStart`).
//!
//! Decisões de tradução:
//! - `IVirtualModel` é o trait [`IVirtualModel`] de `auto_indent.rs`, com o objeto `tokenization` do original
//!   achatado em métodos (`get_line_tokens`, `get_language_id`, `get_language_id_at_position` e o
//!   `forceTokenization` opcional, que por padrão não faz nada).
//! - O `ITextModel` é o [`TextModel`] concreto.
//! - Os tokens processados são [`LineTokens`] (o que `LineTokens.createFromTextAndMetadata` devolve), que
//!   implementa `IViewLineTokens`.
//! - `text.replace(bracketsRegExp, '')` com a regex global vira [`remove_matches`], sobre `exec` e `lastIndex`.

use crate::cursor_common::CursorSimpleModel;
use std::rc::Rc;

use je_base::JsString;
use je_base::js_regexp::JsRegExp;
use je_base::strings::{RegExpOptions, get_leading_whitespace};

use crate::auto_indent::IVirtualModel;
use crate::encoded_token_attributes::standard_token_type;
use crate::indent_rules::IndentRulesSupport;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::position::Position;
use crate::range::Range;
use crate::supports::{ScopedLineTokens, create_scoped_line_tokens};
use crate::text_model::TextModel;

/// `ProcessedIndentRulesSupport`: um envoltório de [`IndentRulesSupport`] que processa as linhas, tirando os
/// colchetes da configuração de linguagem dos tokens de regex, string e comentário, antes de validar as
/// condições de indentação.
pub struct ProcessedIndentRulesSupport<'a> {
    indent_rules_support: &'a IndentRulesSupport,
    indentation_line_processor: IndentationLineProcessor<'a>,
}

impl<'a> ProcessedIndentRulesSupport<'a> {
    pub fn new(
        model: &'a dyn IVirtualModel,
        indent_rules_support: &'a IndentRulesSupport,
        language_configuration_service: &'a dyn ILanguageConfigurationService,
    ) -> Self {
        Self {
            indent_rules_support,
            indentation_line_processor: IndentationLineProcessor::new(model, language_configuration_service),
        }
    }

    /// Aplica a nova indentação e diz se o nível deve aumentar depois da linha.
    pub fn should_increase(&self, line_number: i32, new_indentation: Option<&[u16]>) -> bool {
        let processed_line = self.indentation_line_processor.get_processed_line(line_number, new_indentation);
        self.indent_rules_support.should_increase(processed_line.units())
    }

    /// Aplica a nova indentação e diz se o nível deve diminuir depois da linha.
    pub fn should_decrease(&self, line_number: i32, new_indentation: Option<&[u16]>) -> bool {
        let processed_line = self.indentation_line_processor.get_processed_line(line_number, new_indentation);
        self.indent_rules_support.should_decrease(processed_line.units())
    }

    /// Aplica a nova indentação e diz se o nível deve ficar como está na linha.
    pub fn should_ignore(&self, line_number: i32, new_indentation: Option<&[u16]>) -> bool {
        let processed_line = self.indentation_line_processor.get_processed_line(line_number, new_indentation);
        self.indent_rules_support.should_ignore(processed_line.units())
    }

    /// Aplica a nova indentação e diz se o nível deve aumentar na linha depois da linha.
    pub fn should_indent_next_line(&self, line_number: i32, new_indentation: Option<&[u16]>) -> bool {
        let processed_line = self.indentation_line_processor.get_processed_line(line_number, new_indentation);
        self.indent_rules_support.should_indent_next_line(processed_line.units())
    }
}

/// O que `getProcessedTokenContextAroundRange` devolve.
pub struct ProcessedTokenContext {
    /// O texto processado antes do intervalo, na mesma linha do começo.
    pub before_range_processed_tokens: LineTokens,
    /// O texto processado depois do intervalo, na mesma linha do fim.
    pub after_range_processed_tokens: LineTokens,
    /// O texto processado da linha de cima.
    pub previous_line_processed_tokens: LineTokens,
}

/// `IndentationContextProcessor`: busca o texto processado em volta de um intervalo, para a avaliação da
/// indentação.
pub struct IndentationContextProcessor<'a> {
    model: &'a TextModel,
    indentation_line_processor: IndentationLineProcessor<'a>,
}

impl<'a> IndentationContextProcessor<'a> {
    pub fn new(model: &'a TextModel, language_configuration_service: &'a dyn ILanguageConfigurationService) -> Self {
        Self {
            model,
            indentation_line_processor: IndentationLineProcessor::new(model, language_configuration_service),
        }
    }

    /// O texto processado, sem os colchetes da configuração de linguagem dentro dos tokens de string,
    /// comentário e regex, em volta do intervalo.
    pub fn get_processed_token_context_around_range(&self, range: &Range) -> ProcessedTokenContext {
        let before_range_processed_tokens = self.get_processed_tokens_before_range(range);
        let after_range_processed_tokens = self.get_processed_tokens_after_range(range);
        let previous_line_processed_tokens = self.get_processed_previous_line_tokens(range);
        ProcessedTokenContext {
            before_range_processed_tokens,
            after_range_processed_tokens,
            previous_line_processed_tokens,
        }
    }

    fn get_processed_tokens_before_range(&self, range: &Range) -> LineTokens {
        let tokenization = self.model.tokenization();
        tokenization.force_tokenization(range.start_line_number);
        let line_tokens = Rc::new(tokenization.get_line_tokens(range.start_line_number));
        let scoped_line_tokens = create_scoped_line_tokens(line_tokens.clone(), range.start_column - 1);
        let sliced_tokens = if is_language_different_from_line_start(self.model, range.get_start_position()) {
            let column_index_within_scope = (range.start_column - 1) - scoped_line_tokens.first_char_offset;
            let first_character_offset = scoped_line_tokens.first_char_offset;
            let last_character_offset = first_character_offset + column_index_within_scope;
            line_tokens.slice_and_inflate(first_character_offset, last_character_offset, 0)
        } else {
            let column_within_line = range.start_column - 1;
            line_tokens.slice_and_inflate(0, column_within_line, 0)
        };
        self.indentation_line_processor.get_processed_tokens(&sliced_tokens)
    }

    fn get_processed_tokens_after_range(&self, range: &Range) -> LineTokens {
        let position: Position = if range.is_empty() { range.get_start_position() } else { range.get_end_position() };
        let tokenization = self.model.tokenization();
        tokenization.force_tokenization(position.line_number);
        let line_tokens = Rc::new(tokenization.get_line_tokens(position.line_number));
        let scoped_line_tokens = create_scoped_line_tokens(line_tokens.clone(), position.column - 1);
        let column_index_within_scope = position.column - 1 - scoped_line_tokens.first_char_offset;
        let first_character_offset = scoped_line_tokens.first_char_offset + column_index_within_scope;
        let last_character_offset = scoped_line_tokens.first_char_offset + scoped_line_tokens.get_line_length();
        let sliced_tokens = line_tokens.slice_and_inflate(first_character_offset, last_character_offset, 0);
        self.indentation_line_processor.get_processed_tokens(&sliced_tokens)
    }

    fn get_processed_previous_line_tokens(&self, range: &Range) -> LineTokens {
        let tokenization = self.model.tokenization();
        let get_scoped_line_tokens_at_end_column_of_line = |line_number: i32| -> ScopedLineTokens {
            tokenization.force_tokenization(line_number);
            let line_tokens = Rc::new(tokenization.get_line_tokens(line_number));
            let end_column_of_line = self.model.get_line_max_column(line_number) - 1;
            create_scoped_line_tokens(line_tokens, end_column_of_line)
        };

        tokenization.force_tokenization(range.start_line_number);
        let line_tokens = Rc::new(tokenization.get_line_tokens(range.start_line_number));
        let scoped_line_tokens = create_scoped_line_tokens(line_tokens, range.start_column - 1);
        let empty_tokens = LineTokens::create_empty(JsString::new(), scoped_line_tokens.language_id_codec.clone());
        let previous_line_number = range.start_line_number - 1;
        let is_first_line = previous_line_number == 0;
        if is_first_line {
            return empty_tokens;
        }
        let can_scope_extend_on_previous_line = scoped_line_tokens.first_char_offset == 0;
        if !can_scope_extend_on_previous_line {
            return empty_tokens;
        }
        let scoped_line_tokens_at_end_column_of_previous_line =
            get_scoped_line_tokens_at_end_column_of_line(previous_line_number);
        let does_language_continue_on_previous_line =
            scoped_line_tokens.language_id == scoped_line_tokens_at_end_column_of_previous_line.language_id;
        if !does_language_continue_on_previous_line {
            return empty_tokens;
        }
        let previous_sliced_line_tokens = scoped_line_tokens_at_end_column_of_previous_line.to_i_view_line_tokens();
        self.indentation_line_processor.get_processed_tokens(&previous_sliced_line_tokens)
    }
}

/// `IndentationLineProcessor`: o processamento das linhas em si, que tira os colchetes da configuração de
/// linguagem dos tokens de regex, string e comentário.
struct IndentationLineProcessor<'a> {
    model: &'a dyn IVirtualModel,
    language_configuration_service: &'a dyn ILanguageConfigurationService,
}

/// `text.replace(regExp, '')` para uma regex global: tira todos os casamentos de `text`.
fn remove_matches(reg_exp: &JsRegExp, text: &[u16]) -> JsString {
    reg_exp.set_last_index(0);
    let mut out = JsString::new();
    let mut copied = 0;
    while let Some(found) = reg_exp.exec(text) {
        out.push_units(&text[copied..found.start()]);
        copied = found.end();
        if found.end() == found.start() {
            // Um casamento vazio avança o `lastIndex` em uma unidade, como o `replace` global.
            reg_exp.set_last_index(found.end() + 1);
        }
    }
    out.push_units(&text[copied..]);
    out
}

impl<'a> IndentationLineProcessor<'a> {
    fn new(model: &'a dyn IVirtualModel, language_configuration_service: &'a dyn ILanguageConfigurationService) -> Self {
        Self { model, language_configuration_service }
    }

    /// A linha processada, com a indentação trocada se `new_indentation` vier. Tira os colchetes da
    /// configuração de linguagem dos tokens de regex, string e comentário.
    fn get_processed_line(&self, line_number: i32, new_indentation: Option<&[u16]>) -> JsString {
        self.model.force_tokenization(line_number);
        let tokens = self.model.get_line_tokens(line_number);
        let processed_line = self.get_processed_tokens(&*tokens).get_line_content();
        match new_indentation {
            Some(new_indentation) => {
                let line = processed_line.units();
                let current_indentation = get_leading_whitespace(line, 0, line.len());
                let mut adjusted_line = JsString::from(new_indentation);
                adjusted_line.push_units(&line[current_indentation.len()..]);
                adjusted_line
            }
            None => processed_line,
        }
    }

    /// Processa a linha com os tokens dados: tira os colchetes da configuração de linguagem dos tokens de
    /// regex, string e comentário.
    fn get_processed_tokens(&self, tokens: &dyn IViewLineTokens) -> LineTokens {
        let should_remove_brackets_from_token_type = |token_type: u32| -> bool {
            token_type == standard_token_type::STRING
                || token_type == standard_token_type::REG_EX
                || token_type == standard_token_type::COMMENT
        };

        let language_id = tokens.get_language_id(0);
        let brackets_configuration = self.language_configuration_service.get_language_configuration(&language_id);
        let options = RegExpOptions { global: true, ..RegExpOptions::default() };
        let brackets_reg_exp = brackets_configuration.brackets_new.get_bracket_reg_exp(Some(&options));
        let mut text_and_metadata: Vec<(JsString, u32)> = Vec::new();
        for token_index in 0..tokens.get_count() {
            let token_type = tokens.get_standard_token_type(token_index);
            let mut text = tokens.get_token_text(token_index);
            if should_remove_brackets_from_token_type(token_type) {
                text = remove_matches(&brackets_reg_exp, text.units());
            }
            let metadata = tokens.get_metadata(token_index);
            text_and_metadata.push((text, metadata));
        }
        LineTokens::create_from_text_and_metadata(&text_and_metadata, tokens.language_id_codec().clone())
    }
}

/// `isLanguageDifferentFromLineStart`.
pub fn is_language_different_from_line_start(model: &TextModel, position: Position) -> bool {
    let tokenization = model.tokenization();
    tokenization.force_tokenization(position.line_number);
    let line_tokens = Rc::new(tokenization.get_line_tokens(position.line_number));
    let scoped_line_tokens = create_scoped_line_tokens(line_tokens.clone(), position.column - 1);
    let does_scope_start_at_offset_zero = scoped_line_tokens.first_char_offset == 0;
    let is_scoped_language_equal_to_first_language_on_line =
        line_tokens.get_language_id(0) == scoped_line_tokens.language_id;
    !does_scope_start_at_offset_zero && !is_scoped_language_equal_to_first_language_on_line
}
