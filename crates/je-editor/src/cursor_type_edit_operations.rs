//! Porte de `src/vs/editor/common/cursor/cursorTypeEditOperations.ts`: as edições que o editor faz ao digitar
//! (autofechamento de pares, sobrescrita do fechamento, envolver a seleção, Enter com indentação, caracteres
//! elétricos, composição de IME, colar e Tab).
//!
//! Notas de tradução:
//!
//! - As classes estáticas do original (`AutoIndentOperation`, `PasteOperation`, ...) são structs sem campos com as
//!   funções associadas, como `DeleteOperations`; as funções soltas (`shiftIndent`, `unshiftIndent`,
//!   `shouldSurroundChar`) são funções do módulo.
//! - `ITextModel` é o [`TextModel`] concreto, `ICursorSimpleModel` é `&dyn CursorSimpleModel`, `ICommand` é o
//!   [`Command`] e `ch: string` é `&[u16]` (uma unidade UTF-16, ou vazio); os textos que viram edição são
//!   [`JsString`]. As estratégias de opção ficam como `&str` (ver `cursor_common.rs`).
//! - Os pares de fechamento (`open` e `close` de `StandardAutoClosingPairConditional`) são `String`; tudo que é
//!   comparado com o texto do modelo é recodificado em UTF-16 (`utf16`), para o comprimento e os índices
//!   contarem unidades como no JavaScript.
//! - `autoClosingPair: { open, close } | null` é `Option<`[`AutoClosingPairText`]`>`.
//! - O `followUp` de `typeAndAutoCloseWithUndoStop` precisa viver mais que a chamada, então guarda o `Weak` do
//!   modelo (`TextModel::downgrade`) e devolve `None` se o modelo já foi descartado.
//! - `BaseTypeWithAutoClosingCommand` estende `ReplaceCommandWithOffsetCursorState`: aqui ela guarda um e delega a
//!   ele, e as duas subclasses guardam a base. `closeCharacterRange` e `enclosingRange` são campos públicos da base
//!   (o `Cursor` os lê dos comandos executados).
//! - `new ChosenReplaceCommand(...)` escolhe entre dois comandos pelo modo de sobrescrita: [`replace_or_overtype`].
//! - `PasteOperation.getEdits` ordena `selections` no lugar no original; aqui a ordenação é numa cópia, e o
//!   chamador não vê as seleções reordenadas (nenhum chamador do original lê o vetor depois).
//! - `EnterOperation._enter` é a função associada `enter`. `firstNonWhitespaceIndex` devolve `-1` no original e
//!   `Option` no porte; `enter` converte para `i32` porque o `-1` entra na comparação com `oldEndColumn`.
//! - `getEdits` com parâmetro que o original não lê (`config` e `model` de `CompositionOperation`/`PasteOperation`)
//!   mantém o parâmetro, com `_`, para a chamada seguir a assinatura de lá.
//! - `try { ... } catch (e) { onUnexpectedError(e) }` de `onElectricCharacter` é `errors::catch` e
//!   `errors::on_unexpected_error`.
//! - `findMatchingBracketUp` recebe o colchete como `&str` (ver `bracket_pairs_text_model_part`), então o
//!   `matchOpenBracket` é convertido com perda (colchetes de linguagem são texto válido).

use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::errors::{self, Error};
use je_base::js_string::{index_of, is_js_whitespace, substring};
use je_base::strings::{
    first_non_whitespace_index, get_leading_whitespace, last_non_whitespace_index, split_lines,
};

use crate::auto_indent::{
    IIndentConverter, get_indent_action_for_type, get_indent_for_enter, get_inherit_indent_for_line,
};
use crate::column_selection_paste_command::ColumnSelectionPasteCommand;
use crate::config::editor_options::{EditorAutoIndentStrategy, InputMode};
use crate::cursor_common::{
    CursorConfiguration, CursorSimpleModel, EditOperationResult, EditOperationResultOptions, EditOperationType,
    ShouldAutoClose, is_quote,
};
use crate::cursor_type_operations::CompositionOutcome;
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::enter_action::get_enter_action;
use crate::language_configuration::{AutoClosingPairsMap, IndentAction, StandardAutoClosingPairConditional};
use crate::language_configuration_registry::get_indentation_at_position;
use crate::line_tokens::IViewLineTokens;
use crate::position::Position;
use crate::range::Range;
use crate::replace_command::{
    ReplaceCommand, ReplaceCommandThatPreservesSelection, ReplaceCommandWithOffsetCursorState,
    ReplaceCommandWithoutChangingPosition, ReplaceOvertypeCommand, ReplaceOvertypeCommandOnCompositionEnd,
};
use crate::selection::Selection;
use crate::shift_command::{ShiftCommand, ShiftCommandOpts};
use crate::supports::create_scoped_line_tokens;
use crate::surround_selection_command::SurroundSelectionCommand;
use crate::text_model::TextModel;
use crate::word_character_classifier::{WordCharacterClass, get_map_for_word_separators};

/// `\n`.
const NEWLINE: &[u16] = &[cc::LineFeed];

type CommandList = Vec<Option<Box<dyn Command>>>;

/// O par de fechamento escolhido: `{ open: string; close: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoClosingPairText {
    pub open: JsString,
    pub close: JsString,
}

/// `{ range: Range; text: string }`, a edição de indentação e a de fechamento automático.
#[derive(Clone, Debug)]
pub struct RangeEdit {
    pub range: Range,
    pub text: JsString,
}

/// O texto em UTF-16, a forma em que o modelo compara e mede.
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A concatenação de vários trechos de texto.
fn concat(parts: &[&[u16]]) -> JsString {
    let mut out = JsString::new();
    for part in parts {
        out.push_units(part);
    }
    out
}

/// `'\n' + indentation`.
fn newline_and(indentation: &JsString) -> JsString {
    concat(&[NEWLINE, indentation.units()])
}

/// `/^\s*$/.test(text)`.
fn is_blank(text: &[u16]) -> bool {
    text.iter().all(|&unit| is_js_whitespace(unit))
}

/// `str.lastIndexOf(needle)`: `-1` se não achar (e o tamanho do texto com a agulha vazia).
fn last_index_of(haystack: &[u16], needle: &[u16]) -> i32 {
    if needle.len() > haystack.len() {
        return -1;
    }
    (0..=haystack.len() - needle.len())
        .rev()
        .find(|&index| &haystack[index..index + needle.len()] == needle)
        .map_or(-1, |index| index as i32)
}

/// Os pares do mapa para a chave (`map.get(key) || []`).
fn pairs_for<'a>(map: &'a AutoClosingPairsMap, key: &[u16]) -> &'a [Rc<StandardAutoClosingPairConditional>] {
    match map.get(key) {
        Some(pairs) => pairs.as_slice(),
        None => &[],
    }
}

/// `Box<dyn Command>` de um comando.
fn boxed(command: impl Command + 'static) -> Box<dyn Command> {
    Box::new(command)
}

/// `Some(Box<dyn Command>)`: o elemento de `EditOperationResult.commands`.
fn some_command(command: impl Command + 'static) -> Option<Box<dyn Command>> {
    Some(Box::new(command))
}

/// `new EditOperationResult(type, commands, { shouldPushStackElementBefore, shouldPushStackElementAfter })`, sem
/// `followUp`.
fn edit_result(
    r#type: EditOperationType,
    commands: CommandList,
    should_push_stack_element_before: bool,
    should_push_stack_element_after: bool,
) -> EditOperationResult {
    EditOperationResult::new(
        r#type,
        commands,
        EditOperationResultOptions { should_push_stack_element_before, should_push_stack_element_after, follow_up: None },
    )
}

/// `new ChosenReplaceCommand(range, text)`: `ReplaceOvertypeCommand` no modo de sobrescrita, senão `ReplaceCommand`.
fn replace_or_overtype(overtype: bool, range: Range, text: JsString) -> Box<dyn Command> {
    if overtype {
        boxed(ReplaceOvertypeCommand::new(range, text, None))
    } else {
        boxed(ReplaceCommand::new(range, text, None))
    }
}

/// `config.overtypeOnPaste && config.inputMode === 'overtype'`.
fn should_overtype_on_paste(config: &CursorConfiguration) -> bool {
    config.overtype_on_paste && config.input_mode() == InputMode::Overtype
}

/// Os dois conversores de indentação do original: o de `AutoIndentOperation` (só `shiftIndent` e `unshiftIndent`)
/// e o de `EnterOperation` (com `normalizeIndentation`).
struct IndentConverter<'a> {
    config: &'a CursorConfiguration,
    normalizes: bool,
}

impl IIndentConverter for IndentConverter<'_> {
    fn shift_indent(&self, indentation: &[u16]) -> JsString {
        shift_indent(self.config, indentation, None)
    }

    fn unshift_indent(&self, indentation: &[u16]) -> JsString {
        unshift_indent(self.config, indentation, None)
    }

    fn normalize_indentation(&self, indentation: &[u16]) -> Option<JsString> {
        self.normalizes.then(|| self.config.normalize_indentation(indentation))
    }
}

pub struct AutoIndentOperation;

impl AutoIndentOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        is_doing_composition: bool,
    ) -> Option<EditOperationResult> {
        if is_doing_composition || !Self::is_auto_indent_type(config, model, selections) {
            return None;
        }
        let mut indentation_for_selections: Vec<(Selection, JsString)> = Vec::with_capacity(selections.len());
        for selection in selections {
            // `None` aqui é a indentação automática que falhou.
            let indentation = Self::find_actual_indentation_for_selection(config, model, selection, ch)?;
            indentation_for_selections.push((*selection, indentation));
        }
        let auto_closing_pair =
            AutoClosingOpenCharTypeOperation::get_auto_closing_pair(config, model, selections, ch, false);
        Some(Self::get_indentation_and_auto_closing_pair_edits(
            config,
            model,
            &indentation_for_selections,
            ch,
            auto_closing_pair,
        ))
    }

    fn is_auto_indent_type(config: &CursorConfiguration, model: &TextModel, selections: &[Selection]) -> bool {
        if config.auto_indent < EditorAutoIndentStrategy::Full {
            return false;
        }
        selections
            .iter()
            .all(|selection| model.tokenization().is_cheap_to_tokenize(selection.get_end_position().line_number))
    }

    fn find_actual_indentation_for_selection(
        config: &CursorConfiguration,
        model: &TextModel,
        selection: &Selection,
        ch: &[u16],
    ) -> Option<JsString> {
        let actual_indentation = get_indent_action_for_type(
            config,
            model,
            selection,
            ch,
            &IndentConverter { config, normalizes: false },
            &*config.language_configuration_service,
        )?;

        let current_indentation =
            get_indentation_at_position(model, selection.start_line_number, selection.start_column);
        if actual_indentation == config.normalize_indentation(current_indentation.units()) {
            return None;
        }
        Some(actual_indentation)
    }

    fn get_indentation_and_auto_closing_pair_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        indentation_for_selections: &[(Selection, JsString)],
        ch: &[u16],
        auto_closing_pair: Option<AutoClosingPairText>,
    ) -> EditOperationResult {
        let auto_close_with_undo_stop = auto_closing_pair.as_ref().is_some_and(|pair| pair.open.len() > ch.len());
        let commands: CommandList = indentation_for_selections
            .iter()
            .map(|(selection, indentation)| match &auto_closing_pair {
                Some(pair) if !auto_close_with_undo_stop => {
                    // Aplica as edições do par de fechamento e as da indentação automática.
                    let indentation_edit =
                        Self::get_edit_from_indentation_and_selection(config, model, indentation, selection, ch, false);
                    some_command(TypeWithIndentationAndAutoClosingCommand::new(
                        indentation_edit,
                        *selection,
                        ch,
                        pair.close.units(),
                    ))
                }
                _ => {
                    // Aplica só as edições da indentação automática.
                    let indentation_edit =
                        Self::get_edit_from_indentation_and_selection(config, model, indentation, selection, ch, true);
                    Some(type_command(indentation_edit.range, indentation_edit.text, false))
                }
            })
            .collect();
        if let Some(pair) = &auto_closing_pair
            && auto_close_with_undo_stop
        {
            return AutoClosingOpenCharTypeOperation::type_and_auto_close_with_undo_stop(
                model,
                commands,
                ch,
                pair.close.units(),
            );
        }
        edit_result(EditOperationType::TypingOther, commands, true, false)
    }

    fn get_edit_from_indentation_and_selection(
        config: &CursorConfiguration,
        model: &TextModel,
        indentation: &JsString,
        selection: &Selection,
        ch: &[u16],
        include_ch_in_edit: bool,
    ) -> RangeEdit {
        let start_line_number = selection.start_line_number;
        let first_non_whitespace_column = model.get_line_first_non_whitespace_column(start_line_number);
        let mut text = config.normalize_indentation(indentation.units());
        if first_non_whitespace_column != 0 {
            let start_line = model.get_line_content(start_line_number);
            text.push_units(substring(
                start_line.units(),
                i64::from(first_non_whitespace_column - 1),
                i64::from(selection.start_column - 1),
            ));
        }
        if include_ch_in_edit {
            text.push_units(ch);
        }
        let range = Range::new(start_line_number, 1, selection.end_line_number, selection.end_column);
        RangeEdit { range, text }
    }
}

pub struct AutoClosingOvertypeOperation;

impl AutoClosingOvertypeOperation {
    pub fn get_edits(
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        auto_closed_characters: &[Range],
        ch: &[u16],
    ) -> Option<EditOperationResult> {
        if is_auto_closing_overtype(config, model, selections, auto_closed_characters, ch) {
            return Some(Self::run_auto_closing_overtype(prev_edit_operation_type, selections, ch));
        }
        None
    }

    fn run_auto_closing_overtype(
        prev_edit_operation_type: EditOperationType,
        selections: &[Selection],
        ch: &[u16],
    ) -> EditOperationResult {
        let commands: CommandList = selections
            .iter()
            .map(|selection| {
                let position = selection.get_position();
                let type_selection =
                    Range::new(position.line_number, position.column, position.line_number, position.column + 1);
                some_command(ReplaceCommand::new(type_selection, JsString::from(ch), None))
            })
            .collect();
        edit_result(
            EditOperationType::TypingOther,
            commands,
            should_push_stack_element_between(prev_edit_operation_type, EditOperationType::TypingOther),
            false,
        )
    }
}

pub struct AutoClosingOvertypeWithInterceptorsOperation;

impl AutoClosingOvertypeWithInterceptorsOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        auto_closed_characters: &[Range],
        ch: &[u16],
    ) -> Option<EditOperationResult> {
        if is_auto_closing_overtype(config, model, selections, auto_closed_characters, ch) {
            // Infelizmente, o caractere de fechamento está, a esta altura, "dobrado", então é preciso apagá-lo...
            let commands: CommandList = selections
                .iter()
                .map(|s| {
                    some_command(ReplaceCommand::new(
                        Range::new(s.position_line_number, s.position_column, s.position_line_number, s.position_column + 1),
                        JsString::new(),
                        Some(false),
                    ))
                })
                .collect();
            return Some(edit_result(EditOperationType::TypingOther, commands, true, false));
        }
        None
    }
}

/// `{ lineNumber, beforeColumn, afterColumn }`: a posição antes e depois de `ch`.
struct TypedPosition {
    line_number: i32,
    before_column: i32,
    after_column: i32,
}

pub struct AutoClosingOpenCharTypeOperation;

impl AutoClosingOpenCharTypeOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        ch_is_already_typed: bool,
        is_doing_composition: bool,
    ) -> Option<EditOperationResult> {
        if !is_doing_composition {
            let auto_closing_pair = Self::get_auto_closing_pair(config, model, selections, ch, ch_is_already_typed)?;
            return Some(Self::run_auto_closing_open_char_type(
                model,
                selections,
                ch,
                ch_is_already_typed,
                &auto_closing_pair,
            ));
        }
        None
    }

    fn run_auto_closing_open_char_type(
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        ch_is_already_typed: bool,
        auto_closing_pair: &AutoClosingPairText,
    ) -> EditOperationResult {
        if !ch_is_already_typed && auto_closing_pair.open.len() > ch.len() {
            let type_commands: CommandList = selections
                .iter()
                .map(|selection| Some(type_command(**selection, JsString::from(ch), false)))
                .collect();
            return Self::type_and_auto_close_with_undo_stop(model, type_commands, ch, auto_closing_pair.close.units());
        }
        let commands: CommandList = selections
            .iter()
            .map(|selection| {
                some_command(TypeWithAutoClosingCommand::new(
                    *selection,
                    ch,
                    !ch_is_already_typed,
                    auto_closing_pair.close.units(),
                ))
            })
            .collect();
        edit_result(EditOperationType::TypingOther, commands, true, false)
    }

    /// Executa `type_commands`, que digitam `ch`, e depois insere `auto_closing_pair_close` depois de `ch` com uma
    /// parada de desfazer no meio. Assim, desfazer remove primeiro só os caracteres fechados automaticamente e
    /// mantém uma abertura de vários caracteres digitada (por exemplo `begin`).
    pub fn type_and_auto_close_with_undo_stop(
        model: &TextModel,
        type_commands: CommandList,
        ch: &[u16],
        auto_closing_pair_close: &[u16],
    ) -> EditOperationResult {
        let expected_version_id = model.get_version_id() + 1;
        let command_count = type_commands.len();
        let model_handle = model.downgrade();
        let ch = ch.to_vec();
        let auto_closing_pair_close = auto_closing_pair_close.to_vec();
        EditOperationResult::new(
            EditOperationType::TypingOther,
            type_commands,
            EditOperationResultOptions {
                should_push_stack_element_before: true,
                should_push_stack_element_after: true,
                follow_up: Some(Box::new(move |selections: &[Selection]| {
                    let model = model_handle.upgrade()?;
                    let can_auto_close = model.get_version_id() == expected_version_id
                        && selections.len() == command_count
                        && selections.iter().all(|s| {
                            s.is_empty()
                                && s.position_column > ch.len() as i32
                                && model
                                    .get_value_in_range(&Range::new(
                                        s.position_line_number,
                                        s.position_column - ch.len() as i32,
                                        s.position_line_number,
                                        s.position_column,
                                    ))
                                    .units()
                                    == ch.as_slice()
                        });
                    if !can_auto_close {
                        return None;
                    }
                    let commands: CommandList = selections
                        .iter()
                        .map(|selection| {
                            some_command(TypeWithAutoClosingCommand::new(*selection, &ch, false, &auto_closing_pair_close))
                        })
                        .collect();
                    Some(edit_result(EditOperationType::TypingOther, commands, false, false))
                })),
            },
        )
    }

    /// O par de fechamento automático cuja abertura termina em `ch`, junto dos caracteres de fechamento a inserir.
    pub fn get_auto_closing_pair(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        ch_is_already_typed: bool,
    ) -> Option<AutoClosingPairText> {
        if selections.iter().any(|selection| !selection.is_empty()) {
            return None;
        }
        // Este método é chamado ao digitar (normalmente) e ao terminar a composição. Isso quer dizer que o buffer
        // de texto às vezes não tem `ch` (que está sendo digitado agora) e às vezes já o tem.
        //
        // Para não espalhar testes de `chIsAlreadyTyped`, trabalhamos com duas posições conceituais: a posição
        // antes de `ch` e a posição depois dele.
        let ch_length = ch.len() as i32;
        let positions: Vec<TypedPosition> = selections
            .iter()
            .map(|selection| {
                let position = selection.get_position();
                if ch_is_already_typed {
                    TypedPosition {
                        line_number: position.line_number,
                        before_column: position.column - ch_length,
                        after_column: position.column,
                    }
                } else {
                    TypedPosition {
                        line_number: position.line_number,
                        before_column: position.column,
                        after_column: position.column,
                    }
                }
            })
            .collect();
        // Acha o par de abertura mais longo, caso vários terminem em `ch`: com [f","] e [","], escolhe [f","] se o
        // caractere antes for f.
        let before_positions: Vec<Position> =
            positions.iter().map(|p| Position::new(p.line_number, p.before_column)).collect();
        let pair = Self::find_auto_closing_pair_open(config, model, &before_positions, ch)?;
        let pair_open = utf16(&pair.open);
        let pair_close = utf16(&pair.close);

        let ch_is_quote = is_quote(ch);
        let (auto_close_config, should_auto_close_before, should_check_bracket_balance): (&str, &ShouldAutoClose, bool) =
            if ch_is_quote {
                (config.auto_closing_quotes.as_str(), &config.should_auto_close_before.quote, false)
            } else {
                let pair_is_for_comments = config
                    .block_comment_start_token
                    .as_deref()
                    .filter(|token| !token.is_empty())
                    .is_some_and(|token| pair.open.contains(token));
                if pair_is_for_comments {
                    (config.auto_closing_comments.as_str(), &config.should_auto_close_before.comment, false)
                } else {
                    (config.auto_closing_brackets.as_str(), &config.should_auto_close_before.bracket, true)
                }
            };
        if auto_close_config == "never" {
            return None;
        }
        // Às vezes dois pares de fechamento têm relação de contenção, como [(,)] e [(*,*)]:
        // - ao digitar (, o estado resultante é (|)
        // - ao digitar *, o estado desejado é (*|*), não (*|*))
        let contained_pair = Self::find_contained_auto_closing_pair(config, &pair);
        let contained_pair_close = contained_pair.map_or_else(Vec::new, |contained| utf16(&contained.close));
        let mut is_contained_pair_present = true;

        for position in &positions {
            let TypedPosition { line_number, before_column, after_column } = *position;
            let line_text = model.get_line_content(line_number);
            let line_before = substring(line_text.units(), 0, i64::from(before_column - 1));
            let line_after = substring(line_text.units(), i64::from(after_column - 1), line_text.len() as i64);

            if !line_after.starts_with(&contained_pair_close) {
                is_contained_pair_present = false;
            }
            // Só considera fechar o par se vier um caractere permitido ou o fechamento de outro par já fechado
            if !line_after.is_empty() {
                let character_after = &line_after[..1];
                let is_before_close_brace = Self::is_before_closing_brace(config, line_after);
                if !is_before_close_brace && !should_auto_close_before(character_after) {
                    return None;
                }
            }
            if should_check_bracket_balance
                // Com 'always', sempre insere o colchete de fechamento
                && auto_close_config != "always"
                // O caractere ainda não está digitado, então os colchetes continuam desbalanceados
                && !ch_is_already_typed
                && model
                    .bracket_pairs()
                    .has_unmatched_closing_bracket_after(Position::new(line_number, before_column), &pair.open)
            {
                return None;
            }
            // Não fecha ' nem " depois de um caractere de palavra
            if pair_open.len() == 1
                && (ch == [u16::from(b'\'')] || ch == [u16::from(b'"')])
                && auto_close_config != "always"
            {
                let word_separators = get_map_for_word_separators(&utf16(&config.word_separators), &[]);
                if let Some(&character_before) = line_before.last()
                    && word_separators.get(i32::from(character_before)) == WordCharacterClass::Regular
                {
                    return None;
                }
            }
            if !model.tokenization().is_cheap_to_tokenize(line_number) {
                // Não força a tokenização
                return None;
            }
            model.tokenization().force_tokenization(line_number);
            let line_tokens = Rc::new(model.tokenization().get_line_tokens(line_number));
            let scoped_line_tokens = create_scoped_line_tokens(line_tokens, before_column - 1);
            if !pair.should_auto_close(&scoped_line_tokens, before_column - scoped_line_tokens.first_char_offset) {
                return None;
            }
            // Digitar, por exemplo, uma aspa pode abrir uma string (e fechar é desejável) ou fechar uma string já
            // aberta (e fechar não é desejável).
            //
            // Em certos casos não dá para decidir olhando o token anterior. Por isso fazemos algo incomum: fingimos
            // digitar outro caractere e perguntamos ao tokenizador o resultado: depois de digitar um caractere
            // neutro, estamos numa string (ou seja, a aspa provavelmente fecharia uma string) ou não?
            if let Some(neutral_character) = pair.find_neutral_character() {
                let token_type = model.tokenization().get_token_type_if_inserting_character(
                    line_number,
                    before_column,
                    &JsString::from(neutral_character),
                );
                if !pair.is_ok(token_type) {
                    return None;
                }
            }
        }
        let close_length = if is_contained_pair_present {
            pair_close.len() - contained_pair_close.len()
        } else {
            pair_close.len()
        };
        Some(AutoClosingPairText {
            open: JsString::from_units(pair_open),
            close: JsString::from(&pair_close[..close_length]),
        })
    }

    /// Acha outro par de fechamento contido no que foi passado: com [(,)] e [(*,*)], o contido de [(*,*)] é [(,)].
    fn find_contained_auto_closing_pair(
        config: &CursorConfiguration,
        pair: &StandardAutoClosingPairConditional,
    ) -> Option<Rc<StandardAutoClosingPairConditional>> {
        let pair_open_length = pair.open.encode_utf16().count();
        if pair_open_length <= 1 {
            return None;
        }
        let close = utf16(&pair.close);
        // Os candidatos têm o mesmo último caractere de `close`.
        let last_char = &close[close.len().saturating_sub(1)..];
        let mut result: Option<&Rc<StandardAutoClosingPairConditional>> = None;
        for candidate in pairs_for(&config.auto_closing_pairs.auto_closing_pairs_close_by_end, last_char) {
            if candidate.open != pair.open
                && pair.open.contains(candidate.open.as_str())
                && pair.close.ends_with(candidate.close.as_str())
                && result.is_none_or(|found| {
                    candidate.open.encode_utf16().count() > found.open.encode_utf16().count()
                })
            {
                result = Some(candidate);
            }
        }
        result.cloned()
    }

    /// Determina se digitar `ch` em todas as `positions` do modelo resulta na digitação de uma abertura de par de
    /// fechamento automático.
    ///
    /// Essas aberturas podem ter vários caracteres, o que gera ambiguidade. Nesse caso, devolve a mais longa.
    fn find_auto_closing_pair_open(
        config: &CursorConfiguration,
        model: &TextModel,
        positions: &[Position],
        ch: &[u16],
    ) -> Option<Rc<StandardAutoClosingPairConditional>> {
        // Determina qual é o par de fechamento automático
        let mut result: Option<&Rc<StandardAutoClosingPairConditional>> = None;
        for candidate in pairs_for(&config.auto_closing_pairs.auto_closing_pairs_open_by_end, ch) {
            let candidate_open = utf16(&candidate.open);
            if result.is_none_or(|found| candidate_open.len() > found.open.encode_utf16().count()) {
                let candidate_is_match = positions.iter().all(|position| {
                    let relevant_text = model.get_value_in_range(&Range::new(
                        position.line_number,
                        position.column - candidate_open.len() as i32 + 1,
                        position.line_number,
                        position.column,
                    ));
                    if concat(&[relevant_text.units(), ch]).units() != candidate_open.as_slice() {
                        return false;
                    }
                    if candidate.before_text.is_some() {
                        let prefix = model.get_value_in_range(&Range::new(
                            position.line_number,
                            1,
                            position.line_number,
                            position.column,
                        ));
                        return candidate.should_auto_close_before(concat(&[prefix.units(), ch]).units());
                    }
                    true
                });
                if candidate_is_match {
                    result = Some(candidate);
                }
            }
        }
        result.cloned()
    }

    fn is_before_closing_brace(config: &CursorConfiguration, line_after: &[u16]) -> bool {
        // Se o começo de `lineAfter` pode ser lido como abertura ou fechamento de colchete, devolve falso
        let next_char = line_after.get(..1).unwrap_or(&[]);
        let potential_starting_braces = pairs_for(&config.auto_closing_pairs.auto_closing_pairs_open_by_start, next_char);
        let potential_closing_braces = pairs_for(&config.auto_closing_pairs.auto_closing_pairs_close_by_start, next_char);

        let is_before_starting_brace =
            potential_starting_braces.iter().any(|pair| line_after.starts_with(&utf16(&pair.open)));
        let is_before_closing_brace =
            potential_closing_braces.iter().any(|pair| line_after.starts_with(&utf16(&pair.close)));

        !is_before_starting_brace && is_before_closing_brace
    }
}

pub struct CompositionEndOvertypeOperation;

impl CompositionEndOvertypeOperation {
    pub fn get_edits(config: &CursorConfiguration, compositions: &[CompositionOutcome]) -> Option<EditOperationResult> {
        let is_overtype_mode = config.input_mode() == InputMode::Overtype;
        if !is_overtype_mode {
            return None;
        }
        let commands: CommandList = compositions
            .iter()
            .map(|composition| some_command(ReplaceOvertypeCommandOnCompositionEnd::new(composition.inserted_text_range)))
            .collect();
        Some(edit_result(EditOperationType::TypingOther, commands, true, false))
    }
}

pub struct SurroundSelectionOperation;

impl SurroundSelectionOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        is_doing_composition: bool,
    ) -> Option<EditOperationResult> {
        if !is_doing_composition && Self::is_surround_selection_type(config, model, selections, ch) {
            return Some(Self::run_surround_selection_type(config, selections, ch));
        }
        None
    }

    fn run_surround_selection_type(
        config: &CursorConfiguration,
        selections: &[Selection],
        ch: &[u16],
    ) -> EditOperationResult {
        let close_character = config.surrounding_pairs.get(ch).cloned().unwrap_or_default();
        let commands: CommandList = selections
            .iter()
            .map(|selection| {
                some_command(SurroundSelectionCommand::new(*selection, JsString::from(ch), close_character.clone()))
            })
            .collect();
        edit_result(EditOperationType::Other, commands, true, true)
    }

    fn is_surround_selection_type(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
    ) -> bool {
        if !should_surround_char(config, ch) || !config.surrounding_pairs.contains_key(ch) {
            return false;
        }
        let is_typing_a_quote_character = is_quote(ch);
        for selection in selections {
            if selection.is_empty() {
                return false;
            }
            let mut selection_contains_only_whitespace = true;
            for line_number in selection.start_line_number..=selection.end_line_number {
                let line_text = model.get_line_content(line_number);
                let start_index = if line_number == selection.start_line_number { selection.start_column - 1 } else { 0 };
                let end_index =
                    if line_number == selection.end_line_number { selection.end_column - 1 } else { line_text.len() as i32 };
                let selected_text = substring(line_text.units(), i64::from(start_index), i64::from(end_index));
                if selected_text.iter().any(|&unit| unit != cc::Space && unit != cc::Tab) {
                    // este texto selecionado tem algo além de espaço em branco
                    selection_contains_only_whitespace = false;
                    break;
                }
            }
            if selection_contains_only_whitespace {
                return false;
            }
            if is_typing_a_quote_character
                && selection.start_line_number == selection.end_line_number
                && selection.start_column + 1 == selection.end_column
            {
                let selection_text = model.get_value_in_range(selection);
                if is_quote(selection_text.units()) {
                    // Digitar uma aspa sobre outra aspa => desliga o envolver a seleção
                    return false;
                }
            }
        }
        true
    }
}

pub struct InterceptorElectricCharOperation;

impl InterceptorElectricCharOperation {
    pub fn get_edits(
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        is_doing_composition: bool,
    ) -> Option<EditOperationResult> {
        // Caracteres elétricos só fazem sentido com um cursor: vários cursores digitando colchetes, por exemplo,
        // atrapalhariam o casamento de colchetes.
        if !is_doing_composition && Self::is_type_interceptor_electric_char(model, selections) {
            return Self::type_interceptor_electric_char(prev_edit_operation_type, config, model, &selections[0], ch);
        }
        None
    }

    fn is_type_interceptor_electric_char(model: &TextModel, selections: &[Selection]) -> bool {
        selections.len() == 1
            && model.tokenization().is_cheap_to_tokenize(selections[0].get_end_position().line_number)
    }

    fn type_interceptor_electric_char(
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &TextModel,
        selection: &Selection,
        ch: &[u16],
    ) -> Option<EditOperationResult> {
        if !config.electric_chars().contains(ch) || !selection.is_empty() {
            return None;
        }
        let position = selection.get_position();
        model.tokenization().force_tokenization(position.line_number);
        let line_tokens = Rc::new(model.tokenization().get_line_tokens(position.line_number));
        let electric_action = match errors::catch(|| config.on_electric_character(ch, &line_tokens, position.column)) {
            Ok(electric_action) => electric_action?,
            Err(error) => {
                errors::on_unexpected_error(&error);
                return None;
            }
        };
        if electric_action.match_open_bracket.is_empty() {
            return None;
        }
        let end_column = last_index_of(
            concat(&[line_tokens.get_line_content().units(), ch]).units(),
            electric_action.match_open_bracket.units(),
        ) + 1;
        let matched = model.bracket_pairs().find_matching_bracket_up(
            &electric_action.match_open_bracket.to_string_lossy(),
            Position::new(position.line_number, end_column),
            Some(500), // dá no máximo 500 ms para calcular
        )?;
        if matched.start_line_number == position.line_number {
            // casou algo na mesma linha => a indentação não muda
            return None;
        }
        let match_line = model.get_line_content(matched.start_line_number);
        let match_line_indentation = get_leading_whitespace(match_line.units(), 0, match_line.len());
        let new_indentation = config.normalize_indentation(match_line_indentation);
        let line_text = model.get_line_content(position.line_number);
        let line_first_non_blank_column = match model.get_line_first_non_whitespace_column(position.line_number) {
            0 => position.column,
            column => column,
        };
        let prefix = substring(
            line_text.units(),
            i64::from(line_first_non_blank_column - 1),
            i64::from(position.column - 1),
        );
        let type_text = concat(&[new_indentation.units(), prefix, ch]);
        let type_selection = Range::new(position.line_number, 1, position.line_number, position.column);
        let operation_type = get_typing_operation(type_text.units(), prev_edit_operation_type);
        let command = some_command(ReplaceCommand::new(type_selection, type_text, None));
        Some(edit_result(operation_type, vec![command], false, true))
    }
}

pub struct SimpleCharacterTypeOperation;

impl SimpleCharacterTypeOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        prev_edit_operation_type: EditOperationType,
        selections: &[Selection],
        ch: &[u16],
        is_doing_composition: bool,
    ) -> EditOperationResult {
        // Digitar um caractere simples
        let overtype = config.input_mode() == InputMode::Overtype && !is_doing_composition;
        let commands: CommandList = selections
            .iter()
            .map(|selection| Some(replace_or_overtype(overtype, **selection, JsString::from(ch))))
            .collect();

        let operation_type = get_typing_operation(ch, prev_edit_operation_type);
        edit_result(
            operation_type,
            commands,
            should_push_stack_element_between(prev_edit_operation_type, operation_type),
            false,
        )
    }
}

pub struct EnterOperation;

impl EnterOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        ch: &[u16],
        is_doing_composition: bool,
    ) -> Option<EditOperationResult> {
        if !is_doing_composition && ch == NEWLINE {
            let commands: CommandList =
                selections.iter().map(|selection| Some(Self::enter(config, model, false, **selection))).collect();
            return Some(edit_result(EditOperationType::TypingOther, commands, true, false));
        }
        None
    }

    /// A indentação da linha do começo de `range`, até a coluna do começo: `getLeadingWhitespace(line).substring(0,
    /// range.startColumn - 1)`.
    fn leading_indentation_before_start(model: &TextModel, range: &Range) -> JsString {
        let line_text = model.get_line_content(range.start_line_number);
        let leading = get_leading_whitespace(line_text.units(), 0, line_text.len());
        JsString::from(substring(leading, 0, i64::from(range.start_column - 1)))
    }

    fn enter(config: &CursorConfiguration, model: &TextModel, keep_position: bool, range: Range) -> Box<dyn Command> {
        let mut range = range;
        if config.auto_indent == EditorAutoIndentStrategy::None {
            return type_command(range, JsString::from(NEWLINE), keep_position);
        }
        if !model.tokenization().is_cheap_to_tokenize(range.start_line_number)
            || config.auto_indent == EditorAutoIndentStrategy::Keep
        {
            let indentation = Self::leading_indentation_before_start(model, &range);
            return type_command(
                range,
                newline_and(&config.normalize_indentation(indentation.units())),
                keep_position,
            );
        }
        if let Some(r) = get_enter_action(config.auto_indent, model, &range, &*config.language_configuration_service) {
            let indentation = utf16(&r.indentation);
            let append_text = utf16(&r.append_text);
            match r.indent_action {
                // Nada de especial, ou indenta uma vez
                IndentAction::None | IndentAction::Indent => {
                    return type_command(
                        range,
                        newline_and(&config.normalize_indentation(concat(&[&indentation, &append_text]).units())),
                        keep_position,
                    );
                }
                // Ultra especial
                IndentAction::IndentOutdent => {
                    let normal_indent = config.normalize_indentation(&indentation);
                    let increased_indent =
                        config.normalize_indentation(concat(&[&indentation, &append_text]).units());
                    let type_text =
                        concat(&[NEWLINE, increased_indent.units(), NEWLINE, normal_indent.units()]);
                    return if keep_position {
                        boxed(ReplaceCommandWithoutChangingPosition::new(range, type_text, Some(true)))
                    } else {
                        boxed(ReplaceCommandWithOffsetCursorState::new(
                            range,
                            type_text,
                            -1,
                            increased_indent.len() as i32 - normal_indent.len() as i32,
                            Some(true),
                        ))
                    };
                }
                IndentAction::Outdent => {
                    let actual_indentation = unshift_indent(config, &indentation, None);
                    return type_command(
                        range,
                        newline_and(
                            &config.normalize_indentation(concat(&[actual_indentation.units(), &append_text]).units()),
                        ),
                        keep_position,
                    );
                }
            }
        }

        let indentation = Self::leading_indentation_before_start(model, &range);

        if config.auto_indent >= EditorAutoIndentStrategy::Full {
            let indent_for_enter = get_indent_for_enter(
                config.auto_indent,
                model,
                &range,
                &IndentConverter { config, normalizes: true },
                &*config.language_configuration_service,
            );

            if let Some(ir) = indent_for_enter {
                let mut old_end_view_column = config.visible_column_from_column(model, range.get_end_position());
                let old_end_column = range.end_column;
                let new_line_content = model.get_line_content(range.end_line_number);
                let first_non_whitespace =
                    first_non_whitespace_index(new_line_content.units()).map_or(-1, |index| index as i32);
                if first_non_whitespace >= 0 {
                    range = range.set_end_position(range.end_line_number, range.end_column.max(first_non_whitespace + 1));
                } else {
                    range = range.set_end_position(range.end_line_number, model.get_line_max_column(range.end_line_number));
                }
                let after_enter = newline_and(&config.normalize_indentation(ir.after_enter.units()));
                if keep_position {
                    return boxed(ReplaceCommandWithoutChangingPosition::new(range, after_enter, Some(true)));
                }
                let mut offset = 0;
                if old_end_column <= first_non_whitespace + 1 {
                    if !config.insert_spaces {
                        old_end_view_column =
                            (f64::from(old_end_view_column) / f64::from(config.indent_size)).ceil() as i32;
                    }
                    offset = (old_end_view_column + 1
                        - config.normalize_indentation(ir.after_enter.units()).len() as i32
                        - 1)
                        .min(0);
                }
                return boxed(ReplaceCommandWithOffsetCursorState::new(range, after_enter, 0, offset, Some(true)));
            }
        }
        type_command(range, newline_and(&config.normalize_indentation(indentation.units())), keep_position)
    }

    pub fn line_insert_before(
        config: &CursorConfiguration,
        model: Option<&TextModel>,
        selections: Option<&[Selection]>,
    ) -> Vec<Box<dyn Command>> {
        let (Some(model), Some(selections)) = (model, selections) else {
            return Vec::new();
        };
        selections
            .iter()
            .map(|selection| {
                let mut line_number = selection.position_line_number;
                if line_number == 1 {
                    boxed(ReplaceCommandWithoutChangingPosition::new(Range::new(1, 1, 1, 1), JsString::from(NEWLINE), None))
                } else {
                    line_number -= 1;
                    let column = model.get_line_max_column(line_number);
                    Self::enter(config, model, false, Range::new(line_number, column, line_number, column))
                }
            })
            .collect()
    }

    pub fn line_insert_after(
        config: &CursorConfiguration,
        model: Option<&TextModel>,
        selections: Option<&[Selection]>,
    ) -> Vec<Box<dyn Command>> {
        let (Some(model), Some(selections)) = (model, selections) else {
            return Vec::new();
        };
        selections
            .iter()
            .map(|selection| {
                let line_number = selection.position_line_number;
                let column = model.get_line_max_column(line_number);
                Self::enter(config, model, false, Range::new(line_number, column, line_number, column))
            })
            .collect()
    }

    pub fn line_break_insert(
        config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
    ) -> Vec<Box<dyn Command>> {
        selections.iter().map(|selection| Self::enter(config, model, true, **selection)).collect()
    }
}

pub struct PasteOperation;

impl PasteOperation {
    pub fn get_edits(
        config: &CursorConfiguration,
        _model: &dyn CursorSimpleModel,
        selections: &[Selection],
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: &[JsString],
        is_block: bool,
    ) -> EditOperationResult {
        let distribute_block_to_cursor = is_block && config.multi_cursor_paste == "spread" && selections.len() == 1;
        if distribute_block_to_cursor {
            return Self::block_paste(config, &selections[0], text);
        }
        match Self::distribute_paste_to_cursors(config, selections, text, paste_on_new_line, multicursor_text) {
            Some(distributed_paste) => {
                let mut sorted_selections = selections.to_vec();
                sorted_selections
                    .sort_by(|a, b| Range::compare_ranges_using_starts(Some(&**a), Some(&**b)).cmp(&0));
                Self::distributed_paste(config, &sorted_selections, &distributed_paste)
            }
            None => Self::simple_paste(config, selections, text, paste_on_new_line),
        }
    }

    fn block_paste(config: &CursorConfiguration, selection: &Selection, text: &[u16]) -> EditOperationResult {
        let lines: Vec<JsString> = split_lines(text).into_iter().map(JsString::from).collect();
        let should_overtype_on_paste = should_overtype_on_paste(config);
        let command = ColumnSelectionPasteCommand::new(*selection, lines, config.tab_size, should_overtype_on_paste);
        edit_result(EditOperationType::Other, vec![some_command(command)], true, true)
    }

    fn distribute_paste_to_cursors(
        config: &CursorConfiguration,
        selections: &[Selection],
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: &[JsString],
    ) -> Option<Vec<JsString>> {
        if selections.len() == 1 {
            return None;
        }
        if multicursor_text.len() == selections.len() {
            return Some(multicursor_text.to_vec());
        }
        if paste_on_new_line {
            return None;
        }
        if config.multi_cursor_paste == "spread" {
            // Tenta espalhar o texto colado se o número de linhas for o de cursores
            let mut text = text;
            // Tira o \n do fim, se houver
            if text.last() == Some(&cc::LineFeed) {
                text = &text[..text.len() - 1];
            }
            // Tira o \r do fim, se houver
            if text.last() == Some(&cc::CarriageReturn) {
                text = &text[..text.len() - 1];
            }
            let lines: Vec<JsString> = split_lines(text).into_iter().map(JsString::from).collect();
            if lines.len() == selections.len() {
                return Some(lines);
            }
        }
        None
    }

    fn distributed_paste(config: &CursorConfiguration, selections: &[Selection], text: &[JsString]) -> EditOperationResult {
        let overtype = should_overtype_on_paste(config);
        let commands: CommandList = selections
            .iter()
            .zip(text)
            .map(|(selection, text)| Some(replace_or_overtype(overtype, **selection, text.clone())))
            .collect();
        edit_result(EditOperationType::Other, commands, true, true)
    }

    fn simple_paste(
        config: &CursorConfiguration,
        selections: &[Selection],
        text: &[u16],
        paste_on_new_line: bool,
    ) -> EditOperationResult {
        let mut paste_on_new_line = paste_on_new_line;
        let commands: CommandList = selections
            .iter()
            .map(|selection| {
                let position = selection.get_position();
                if paste_on_new_line && !selection.is_empty() {
                    paste_on_new_line = false;
                }
                // `text.indexOf('\n') !== text.length - 1`, com o `-1` do `indexOf` que não achou.
                let last_index = index_of(text, NEWLINE, 0).map_or(-1, |index| index as i64);
                if paste_on_new_line && last_index != text.len() as i64 - 1 {
                    paste_on_new_line = false;
                }
                if paste_on_new_line {
                    // Cola a linha inteira no começo da linha
                    let type_selection = Range::new(position.line_number, 1, position.line_number, 1);
                    some_command(ReplaceCommandThatPreservesSelection::new(
                        type_selection,
                        JsString::from(text),
                        *selection,
                        Some(true),
                    ))
                } else {
                    Some(replace_or_overtype(should_overtype_on_paste(config), **selection, JsString::from(text)))
                }
            })
            .collect();
        edit_result(EditOperationType::Other, commands, true, true)
    }
}

pub struct CompositionOperation;

impl CompositionOperation {
    #[allow(clippy::too_many_arguments)]
    pub fn get_edits(
        prev_edit_operation_type: EditOperationType,
        _config: &CursorConfiguration,
        model: &TextModel,
        selections: &[Selection],
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
    ) -> EditOperationResult {
        let commands: CommandList = selections
            .iter()
            .map(|selection| {
                Self::composition_type(model, selection, text, replace_prev_char_cnt, replace_next_char_cnt, position_delta)
            })
            .collect();
        edit_result(
            EditOperationType::TypingOther,
            commands,
            should_push_stack_element_between(prev_edit_operation_type, EditOperationType::TypingOther),
            false,
        )
    }

    fn composition_type(
        model: &TextModel,
        selection: &Selection,
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
    ) -> Option<Box<dyn Command>> {
        if !selection.is_empty() {
            // parece https://github.com/microsoft/vscode/issues/2773
            // em que uma operação do cursor aconteceu antes de uma composição cancelada
            // => ignora a composição
            return None;
        }
        let pos = selection.get_position();
        let start_column = (pos.column - replace_prev_char_cnt).max(1);
        let end_column = model.get_line_max_column(pos.line_number).min(pos.column + replace_next_char_cnt);
        let range = Range::new(pos.line_number, start_column, pos.line_number, end_column);
        some_command(ReplaceCommandWithOffsetCursorState::new(range, JsString::from(text), 0, position_delta, None))
    }
}

pub struct TypeWithoutInterceptorsOperation;

impl TypeWithoutInterceptorsOperation {
    pub fn get_edits(prev_edit_operation_type: EditOperationType, selections: &[Selection], str: &[u16]) -> EditOperationResult {
        let commands: CommandList = selections
            .iter()
            .map(|selection| some_command(ReplaceCommand::new(**selection, JsString::from(str), None)))
            .collect();
        let operation_type = get_typing_operation(str, prev_edit_operation_type);
        edit_result(
            operation_type,
            commands,
            should_push_stack_element_between(prev_edit_operation_type, operation_type),
            false,
        )
    }
}

pub struct TabOperation;

impl TabOperation {
    pub fn get_commands(config: &CursorConfiguration, model: &TextModel, selections: &[Selection]) -> Vec<Box<dyn Command>> {
        let mut commands: Vec<Box<dyn Command>> = Vec::with_capacity(selections.len());
        for selection in selections {
            if selection.is_empty() {
                let line_text = model.get_line_content(selection.start_line_number);
                if is_blank(line_text.units()) && model.tokenization().is_cheap_to_tokenize(selection.start_line_number) {
                    let good_indent = Self::good_indent_for_line(config, model, selection.start_line_number)
                        .unwrap_or_else(|| JsString::from("\t"));
                    let possible_type_text = config.normalize_indentation(good_indent.units());
                    if !line_text.units().starts_with(possible_type_text.units()) {
                        commands.push(boxed(ReplaceCommand::new(
                            Range::new(selection.start_line_number, 1, selection.start_line_number, line_text.len() as i32 + 1),
                            possible_type_text,
                            Some(true),
                        )));
                        continue;
                    }
                }
                commands.push(boxed(Self::replace_jump_to_next_indent(config, model, selection, true)));
            } else {
                if selection.start_line_number == selection.end_line_number {
                    let line_max_column = model.get_line_max_column(selection.start_line_number);
                    if selection.start_column != 1 || selection.end_column != line_max_column {
                        // Esta é uma seleção de uma linha que não é a linha inteira
                        commands.push(boxed(Self::replace_jump_to_next_indent(config, model, selection, false)));
                        continue;
                    }
                }
                commands.push(boxed(ShiftCommand::new(
                    *selection,
                    ShiftCommandOpts {
                        is_unshift: false,
                        tab_size: config.tab_size,
                        indent_size: config.indent_size,
                        insert_spaces: config.insert_spaces,
                        use_tab_stops: config.use_tab_stops,
                        auto_indent: config.auto_indent,
                    },
                    config.language_configuration_service.clone(),
                )));
            }
        }
        commands
    }

    fn good_indent_for_line(config: &CursorConfiguration, model: &TextModel, line_number: i32) -> Option<JsString> {
        let mut action: Option<IndentAction> = None;
        let mut indentation = JsString::new();
        let expected_indent_action = get_inherit_indent_for_line(
            config.auto_indent,
            model,
            line_number,
            false,
            &*config.language_configuration_service,
        );
        if let Some(expected_indent_action) = expected_indent_action {
            action = expected_indent_action.action;
            indentation = expected_indent_action.indentation;
        } else if line_number > 1 {
            let mut last_line_number = line_number - 1;
            while last_line_number >= 1 {
                let line_text = model.get_line_content(last_line_number);
                if last_non_whitespace_index(line_text.units(), line_text.len() as i64 - 1).is_some() {
                    break;
                }
                last_line_number -= 1;
            }
            if last_line_number < 1 {
                // Não achou uma linha anterior com conteúdo
                return None;
            }
            let max_column = model.get_line_max_column(last_line_number);
            let expected_enter_action = get_enter_action(
                config.auto_indent,
                model,
                &Range::new(last_line_number, max_column, last_line_number, max_column),
                &*config.language_configuration_service,
            );
            if let Some(expected_enter_action) = expected_enter_action {
                indentation = JsString::from(expected_enter_action.indentation + &expected_enter_action.append_text);
            }
        }
        // `if (action)`: `IndentAction.None` vale 0 e é falso.
        match action {
            Some(IndentAction::Indent) => {
                indentation = shift_indent(config, indentation.units(), None);
                indentation = config.normalize_indentation(indentation.units());
            }
            Some(IndentAction::Outdent) => {
                indentation = unshift_indent(config, indentation.units(), None);
                indentation = config.normalize_indentation(indentation.units());
            }
            Some(IndentAction::IndentOutdent) => {
                indentation = config.normalize_indentation(indentation.units());
            }
            Some(IndentAction::None) | None => {}
        }
        if indentation.is_empty() {
            return None;
        }
        Some(indentation)
    }

    fn replace_jump_to_next_indent(
        config: &CursorConfiguration,
        model: &dyn CursorSimpleModel,
        selection: &Selection,
        inserts_auto_whitespace: bool,
    ) -> ReplaceCommand {
        let type_text = if config.insert_spaces {
            let position = selection.get_start_position();
            let visible_column_from_column = config.visible_column_from_column(model, position);
            let indent_size = config.indent_size;
            let spaces_cnt = indent_size - (visible_column_from_column % indent_size);
            JsString::from_units(vec![cc::Space; spaces_cnt.max(0) as usize])
        } else {
            JsString::from("\t")
        };
        ReplaceCommand::new(**selection, type_text, Some(inserts_auto_whitespace))
    }
}

/// `BaseTypeWithAutoClosingCommand`: um `ReplaceCommandWithOffsetCursorState` que, ao calcular o estado do cursor,
/// guarda o intervalo do caractere de fechamento e o do par inteiro (`enclosingRange`).
pub struct BaseTypeWithAutoClosingCommand {
    inner: ReplaceCommandWithOffsetCursorState,
    open_character_length: i32,
    close_character_length: i32,
    pub close_character_range: Option<Range>,
    pub enclosing_range: Option<Range>,
}

impl BaseTypeWithAutoClosingCommand {
    pub fn new(
        selection: Selection,
        text: JsString,
        line_number_delta_offset: i32,
        column_delta_offset: i32,
        open_character: &[u16],
        close_character: &[u16],
    ) -> Self {
        Self {
            inner: ReplaceCommandWithOffsetCursorState::new(
                *selection,
                text,
                line_number_delta_offset,
                column_delta_offset,
                None,
            ),
            open_character_length: open_character.len() as i32,
            close_character_length: close_character.len() as i32,
            close_character_range: None,
            enclosing_range: None,
        }
    }

    fn compute_cursor_state_with_range(
        &mut self,
        model: &TextModel,
        range: Range,
        helper: &dyn CursorStateComputerData,
    ) -> Selection {
        self.close_character_range = Some(Range::new(
            range.start_line_number,
            range.end_column - self.close_character_length,
            range.end_line_number,
            range.end_column,
        ));
        self.enclosing_range = Some(Range::new(
            range.start_line_number,
            range.end_column - self.open_character_length - self.close_character_length,
            range.end_line_number,
            range.end_column,
        ));
        self.inner.compute_cursor_state(model, helper)
    }
}

pub struct TypeWithAutoClosingCommand {
    pub base: BaseTypeWithAutoClosingCommand,
}

impl TypeWithAutoClosingCommand {
    pub fn new(selection: Selection, open_character: &[u16], insert_open_character: bool, close_character: &[u16]) -> Self {
        let open_text: &[u16] = if insert_open_character { open_character } else { &[] };
        let text = concat(&[open_text, close_character]);
        let line_number_delta_offset = 0;
        let column_delta_offset = -(close_character.len() as i32);
        Self {
            base: BaseTypeWithAutoClosingCommand::new(
                selection,
                text,
                line_number_delta_offset,
                column_delta_offset,
                open_character,
                close_character,
            ),
        }
    }
}

impl Command for TypeWithAutoClosingCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        self.base.inner.get_edit_operations(model, builder);
    }

    fn compute_cursor_state(&mut self, model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let range = helper.get_inverse_edit_operations()[0].range;
        self.base.compute_cursor_state_with_range(model, range, helper)
    }

    fn as_type_with_auto_closing(&self) -> Option<&BaseTypeWithAutoClosingCommand> {
        Some(&self.base)
    }
}

pub struct TypeWithIndentationAndAutoClosingCommand {
    pub base: BaseTypeWithAutoClosingCommand,
    auto_indentation_edit: RangeEdit,
    auto_closing_edit: RangeEdit,
}

impl TypeWithIndentationAndAutoClosingCommand {
    pub fn new(
        auto_indentation_edit: RangeEdit,
        selection: Selection,
        open_character: &[u16],
        close_character: &[u16],
    ) -> Self {
        let text = concat(&[open_character, close_character]);
        let line_number_delta_offset = 0;
        let column_delta_offset = open_character.len() as i32;
        Self {
            base: BaseTypeWithAutoClosingCommand::new(
                selection,
                text.clone(),
                line_number_delta_offset,
                column_delta_offset,
                open_character,
                close_character,
            ),
            auto_indentation_edit,
            auto_closing_edit: RangeEdit { range: *selection, text },
        }
    }
}

impl Command for TypeWithIndentationAndAutoClosingCommand {
    fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        builder.add_tracked_edit_operation(
            self.auto_indentation_edit.range,
            Some(self.auto_indentation_edit.text.clone()),
            false,
        );
        builder.add_tracked_edit_operation(
            self.auto_closing_edit.range,
            Some(self.auto_closing_edit.text.clone()),
            false,
        );
    }

    fn compute_cursor_state(&mut self, model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let inverse_edit_operations = helper.get_inverse_edit_operations();
        if inverse_edit_operations.len() != 2 {
            errors::throw(Error::new("There should be two inverse edit operations!"));
        }
        let range1 = inverse_edit_operations[0].range;
        let range2 = inverse_edit_operations[1].range;
        let range = range1.plus_range(&range2);
        self.base.compute_cursor_state_with_range(model, range, helper)
    }

    fn as_type_with_auto_closing(&self) -> Option<&BaseTypeWithAutoClosingCommand> {
        Some(&self.base)
    }
}

fn get_typing_operation(typed_text: &[u16], previous_typing_operation: EditOperationType) -> EditOperationType {
    if typed_text == [cc::Space] {
        return if previous_typing_operation == EditOperationType::TypingFirstSpace
            || previous_typing_operation == EditOperationType::TypingConsecutiveSpace
        {
            EditOperationType::TypingConsecutiveSpace
        } else {
            EditOperationType::TypingFirstSpace
        };
    }

    EditOperationType::TypingOther
}

fn should_push_stack_element_between(
    previous_typing_operation: EditOperationType,
    typing_operation: EditOperationType,
) -> bool {
    if is_typing_operation(previous_typing_operation) && !is_typing_operation(typing_operation) {
        // Sempre abre uma parada de desfazer antes de operações que não são de digitação
        return true;
    }
    if previous_typing_operation == EditOperationType::TypingFirstSpace {
        // `abc |d`: sem parada de desfazer
        // `abc  |d`: com parada de desfazer
        return false;
    }
    // Abre uma parada de desfazer entre tipos de operação diferentes
    normalize_operation_type(previous_typing_operation) != normalize_operation_type(typing_operation)
}

/// `EditOperationType | 'space'`: os dois tipos de espaço viram o mesmo.
#[derive(PartialEq, Eq)]
enum NormalizedOperationType {
    Space,
    Type(EditOperationType),
}

fn normalize_operation_type(r#type: EditOperationType) -> NormalizedOperationType {
    if r#type == EditOperationType::TypingConsecutiveSpace || r#type == EditOperationType::TypingFirstSpace {
        NormalizedOperationType::Space
    } else {
        NormalizedOperationType::Type(r#type)
    }
}

fn is_typing_operation(r#type: EditOperationType) -> bool {
    r#type == EditOperationType::TypingOther
        || r#type == EditOperationType::TypingFirstSpace
        || r#type == EditOperationType::TypingConsecutiveSpace
}

fn is_auto_closing_overtype(
    config: &CursorConfiguration,
    model: &TextModel,
    selections: &[Selection],
    auto_closed_characters: &[Range],
    ch: &[u16],
) -> bool {
    if config.auto_closing_overtype == "never" {
        return false;
    }
    if !config.auto_closing_pairs.auto_closing_pairs_close_single_char.contains_key(ch) {
        return false;
    }
    for selection in selections {
        if !selection.is_empty() {
            return false;
        }
        let position = selection.get_position();
        let line_text = model.get_line_content(position.line_number);
        let after_character_index = (position.column - 1) as usize;
        let after_character = line_text.units().get(after_character_index..after_character_index + 1).unwrap_or(&[]);
        if after_character != ch {
            return false;
        }
        // Não sobrescreve aspas depois de uma barra invertida
        let ch_is_quote = is_quote(ch);
        let before_character = if position.column > 2 { line_text.units()[(position.column - 2) as usize] } else { 0 };
        if before_character == cc::Backslash && ch_is_quote {
            return false;
        }
        // Só sobrescreve um caractere de fechamento digitado pelo editor
        if config.auto_closing_overtype == "auto" {
            let found = auto_closed_characters.iter().any(|auto_closed_character| {
                position.line_number == auto_closed_character.start_line_number
                    && position.column == auto_closed_character.start_column
            });
            if !found {
                return false;
            }
        }
    }
    true
}

fn type_command(range: Range, text: JsString, keep_position: bool) -> Box<dyn Command> {
    if keep_position {
        boxed(ReplaceCommandWithoutChangingPosition::new(range, text, Some(true)))
    } else {
        boxed(ReplaceCommand::new(range, text, Some(true)))
    }
}

/// `ShiftCommand.shiftIndent` e `ShiftCommand.unshiftIndent`.
type ShiftIndentFn = fn(&[u16], i32, i32, i32, bool) -> JsString;

/// O corpo comum de `shiftIndent` e `unshiftIndent`: `count = count || 1`.
fn apply_shift(config: &CursorConfiguration, indentation: &[u16], count: Option<i32>, shift: ShiftIndentFn) -> JsString {
    let count = count.filter(|&count| count != 0).unwrap_or(1);
    shift(indentation, indentation.len() as i32 + count, config.tab_size, config.indent_size, config.insert_spaces)
}

pub fn shift_indent(config: &CursorConfiguration, indentation: &[u16], count: Option<i32>) -> JsString {
    apply_shift(config, indentation, count, ShiftCommand::shift_indent)
}

pub fn unshift_indent(config: &CursorConfiguration, indentation: &[u16], count: Option<i32>) -> JsString {
    apply_shift(config, indentation, count, ShiftCommand::unshift_indent)
}

pub fn should_surround_char(config: &CursorConfiguration, ch: &[u16]) -> bool {
    if is_quote(ch) {
        config.auto_surround == "quotes" || config.auto_surround == "languageDefined"
    } else {
        // O caractere é um colchete
        config.auto_surround == "brackets" || config.auto_surround == "languageDefined"
    }
}
