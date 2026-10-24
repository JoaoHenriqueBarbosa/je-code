//! Porte de `src/vs/editor/contrib/linesOperations/browser/moveLinesCommand.ts`: mover as linhas da seleção
//! uma linha para cima ou para baixo, ajustando a indentação pelas regras de Enter e de indentação da linguagem.
//!
//! Notas de tradução:
//! - `@ILanguageConfigurationService` é o parâmetro `language_configuration_service` do construtor.
//! - O `IVirtualModel` de `createVirtualModel` é o [`MovedLinesVirtualModel`]: o mapeador de linhas e o
//!   conteúdo substituto são closures.
//! - `previousLineText?` de `matchEnterRule` é `Option<&[u16]>`, como nos demais argumentos opcionais do crate.
//! - `_moveEndPositionDown?: boolean` é `bool`, porque o original só o lê como condição (`undefined` é falso).

use std::rc::Rc;

use je_base::JsString;
use je_base::js_string::{index_of, trim_start};
use je_base::strings::last_non_whitespace_index;

use crate::auto_indent::{IIndentConverter, get_good_indent_for_line, get_indent_metadata, leading_whitespace};
use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::enter_action::get_enter_action;
use crate::indent_rules::indent_consts;
use crate::indent_utils::{generate_indent, get_space_cnt};
use crate::language_configuration::{CompleteEnterAction, IndentAction};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_tokens::IViewLineTokens;
use crate::range::Range;
use crate::selection::Selection;
use crate::shift_command::ShiftCommand;
use crate::text_model::TextModel;

/// O `IVirtualModel` de `createVirtualModel`: o modelo com as linhas remapeadas e, opcionalmente, o conteúdo de
/// algumas linhas trocado.
struct MovedLinesVirtualModel<'a> {
    model: &'a TextModel,
    line_number_mapper: Box<dyn Fn(i32) -> i32 + 'a>,
    content_override: Option<Box<dyn Fn(i32) -> Option<JsString> + 'a>>,
}

impl<'a> MovedLinesVirtualModel<'a> {
    /// `createVirtualModel(model, lineNumberMapper, contentOverride?)`.
    fn new(
        model: &'a TextModel,
        line_number_mapper: impl Fn(i32) -> i32 + 'a,
        content_override: Option<Box<dyn Fn(i32) -> Option<JsString> + 'a>>,
    ) -> Self {
        Self { model, line_number_mapper: Box::new(line_number_mapper), content_override }
    }
}

impl crate::auto_indent::IVirtualModel for MovedLinesVirtualModel<'_> {
    fn get_line_tokens(&self, line_number: i32) -> Box<dyn IViewLineTokens> {
        Box::new(self.model.tokenization().get_line_tokens((self.line_number_mapper)(line_number)))
    }

    fn get_language_id(&self) -> String {
        self.model.tokenization().get_language_id()
    }

    fn get_language_id_at_position(&self, line_number: i32, column: i32) -> String {
        self.model.tokenization().get_language_id_at_position(line_number, column)
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        if let Some(custom_content) = self.content_override.as_ref().and_then(|content_override| content_override(line_number)) {
            return custom_content;
        }
        CursorSimpleModel::get_line_content(&*self.model, (self.line_number_mapper)(line_number))
    }
}

/// O `IIndentConverter` de `buildIndentConverter`.
pub(crate) struct MoveLinesIndentConverter {
    pub(crate) tab_size: i32,
    pub(crate) indent_size: i32,
    pub(crate) insert_spaces: bool,
}

impl IIndentConverter for MoveLinesIndentConverter {
    fn shift_indent(&self, indentation: &[u16]) -> JsString {
        ShiftCommand::shift_indent(indentation, indentation.len() as i32 + 1, self.tab_size, self.indent_size, self.insert_spaces)
    }

    fn unshift_indent(&self, indentation: &[u16]) -> JsString {
        ShiftCommand::unshift_indent(indentation, indentation.len() as i32 + 1, self.tab_size, self.indent_size, self.insert_spaces)
    }
}

/// `str.replace(/^\s+/, '')` como [`JsString`].
fn trimmed_start(str: &[u16]) -> JsString {
    JsString::from(trim_start(str))
}

/// `a + b`.
fn concat(a: &JsString, b: &[u16]) -> JsString {
    let mut result = a.clone();
    result.push_units(b);
    result
}

/// `MoveLinesCommand`.
pub struct MoveLinesCommand {
    selection: Selection,
    is_moving_down: bool,
    auto_indent: EditorAutoIndentStrategy,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,

    selection_id: Option<String>,
    move_end_position_down: bool,
    move_end_line_selection_shrink: bool,
}

impl MoveLinesCommand {
    /// `new MoveLinesCommand(selection, isMovingDown, autoIndent, languageConfigurationService)`.
    pub fn new(
        selection: Selection,
        is_moving_down: bool,
        auto_indent: EditorAutoIndentStrategy,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        Self {
            selection,
            is_moving_down,
            auto_indent,
            language_configuration_service,
            selection_id: None,
            move_end_position_down: false,
            move_end_line_selection_shrink: false,
        }
    }

    /// `parseEnterResult`.
    fn parse_enter_result(
        &self,
        model: &TextModel,
        indent_converter: &dyn IIndentConverter,
        tab_size: i32,
        line: i32,
        enter: Option<CompleteEnterAction>,
    ) -> Option<i32> {
        let enter = enter?;
        let enter_indentation: Vec<u16> = enter.indentation.encode_utf16().collect();
        let enter_prefix: JsString = match enter.indent_action {
            IndentAction::None | IndentAction::Indent => JsString::from(format!("{}{}", enter.indentation, enter.append_text).as_str()),
            IndentAction::IndentOutdent => JsString::from(enter_indentation.as_slice()),
            IndentAction::Outdent => {
                concat(&indent_converter.unshift_indent(&enter_indentation), &enter.append_text.encode_utf16().collect::<Vec<u16>>())
            }
        };
        let moving_line_text = model.get_line_content(line);
        if index_of(trim_start(moving_line_text.units()), trim_start(enter_prefix.units()), 0).is_some() {
            let old_indentation = leading_whitespace(moving_line_text.units());
            let mut new_indentation = leading_whitespace(enter_prefix.units());
            let indent_metadata_of_moveling_line = get_indent_metadata(model, line, &*self.language_configuration_service);
            if indent_metadata_of_moveling_line.is_some_and(|metadata| metadata & indent_consts::DECREASE_MASK != 0) {
                new_indentation = indent_converter.unshift_indent(new_indentation.units());
            }
            let new_space_cnt = get_space_cnt(new_indentation.units(), tab_size);
            let old_space_cnt = get_space_cnt(old_indentation.units(), tab_size);
            return Some(new_space_cnt - old_space_cnt);
        }

        None
    }

    /// `getEnterAction` na posição final de `line_number`, que `matchEnterRuleMovingDown` e `matchEnterRule` fazem
    /// com a linha válida que acharam.
    fn enter_action_at_end_of_line(&self, model: &TextModel, line_number: i32) -> Option<CompleteEnterAction> {
        let max_column = model.get_line_max_column(line_number);
        get_enter_action(
            self.auto_indent,
            model,
            &Range::new(line_number, max_column, line_number, max_column),
            &*self.language_configuration_service,
        )
    }

    /// `matchEnterRuleMovingDown`.
    ///
    /// - `line`: a linha que desce.
    /// - `future_above_line_number`: a linha que ficará na posição `line`.
    /// - `future_above_line_text`: o texto dela.
    fn match_enter_rule_moving_down(
        &self,
        model: &TextModel,
        indent_converter: &dyn IIndentConverter,
        tab_size: i32,
        line: i32,
        future_above_line_number: i32,
        future_above_line_text: &JsString,
    ) -> Option<i32> {
        let text_units = future_above_line_text.units();
        if last_non_whitespace_index(text_units, text_units.len() as i64 - 1).is_some() {
            // break
            let enter = self.enter_action_at_end_of_line(model, future_above_line_number);
            self.parse_enter_result(model, indent_converter, tab_size, line, enter)
        } else {
            // sobe, a partir de `line - 1`
            let mut valid_preceding_line = line - 1;
            while valid_preceding_line >= 1 {
                let line_content = model.get_line_content(valid_preceding_line);
                let non_whitespace_idx = last_non_whitespace_index(line_content.units(), line_content.len() as i64 - 1);

                if non_whitespace_idx.is_some() {
                    break;
                }

                valid_preceding_line -= 1;
            }

            if valid_preceding_line < 1 || line > model.get_line_count() {
                return None;
            }

            let enter = self.enter_action_at_end_of_line(model, valid_preceding_line);
            self.parse_enter_result(model, indent_converter, tab_size, line, enter)
        }
    }

    /// `matchEnterRule`.
    fn match_enter_rule(
        &self,
        model: &TextModel,
        indent_converter: &dyn IIndentConverter,
        tab_size: i32,
        line: i32,
        one_line_above: i32,
        previous_line_text: Option<&JsString>,
    ) -> Option<i32> {
        let mut valid_preceding_line = one_line_above;
        while valid_preceding_line >= 1 {
            // linhas vazias são puladas, porque só herdam a indentação
            let line_content = match previous_line_text {
                Some(previous_line_text) if valid_preceding_line == one_line_above => previous_line_text.clone(),
                _ => model.get_line_content(valid_preceding_line),
            };

            let non_whitespace_idx = last_non_whitespace_index(line_content.units(), line_content.len() as i64 - 1);
            if non_whitespace_idx.is_some() {
                break;
            }
            valid_preceding_line -= 1;
        }

        if valid_preceding_line < 1 || line > model.get_line_count() {
            return None;
        }

        let enter = self.enter_action_at_end_of_line(model, valid_preceding_line);
        self.parse_enter_result(model, indent_converter, tab_size, line, enter)
    }

    /// `shouldAutoIndent`.
    fn should_auto_indent(&self, model: &TextModel, selection: &Selection) -> bool {
        if self.auto_indent < EditorAutoIndentStrategy::Full {
            return false;
        }
        // se não é barato tokenizar, a indentação automática para
        if !model.tokenization().is_cheap_to_tokenize(selection.start_line_number) {
            return false;
        }
        let language_at_selection_start = model.get_language_id_at_position(selection.start_line_number, 1);
        let language_at_selection_end = model.get_language_id_at_position(selection.end_line_number, 1);

        if language_at_selection_start != language_at_selection_end {
            return false;
        }

        if self
            .language_configuration_service
            .get_language_configuration(&language_at_selection_start)
            .indent_rules_support
            .is_none()
        {
            return false;
        }

        true
    }

    /// `getIndentEditsOfMovingBlock`.
    fn get_indent_edits_of_moving_block(
        &mut self,
        model: &TextModel,
        builder: &mut dyn EditOperationBuilder,
        s: &Selection,
        tab_size: i32,
        insert_spaces: bool,
        offset: i32,
    ) {
        for i in s.start_line_number..=s.end_line_number {
            let line_content = model.get_line_content(i);
            let original_indent = leading_whitespace(line_content.units());
            let original_spaces_cnt = get_space_cnt(original_indent.units(), tab_size);
            let new_spaces_cnt = original_spaces_cnt + offset;
            let new_indent = generate_indent(new_spaces_cnt, tab_size, insert_spaces);

            if new_indent != original_indent {
                builder.add_edit_operation(Range::new(i, 1, i, original_indent.len() as i32 + 1), Some(new_indent.clone()), false);

                if i == s.end_line_number && s.end_column <= original_indent.len() as i32 + 1 && new_indent.is_empty() {
                    // como o usuário selecionou parte dos espaços da indentação original,
                    // ao ajustar a indentação da última linha o cursor também deve ser ajustado
                    self.move_end_line_selection_shrink = true;
                }
            }
        }
    }

    /// O trecho de `getEditOperations` que move o bloco `s` para baixo.
    fn move_block_down(
        &mut self,
        model: &TextModel,
        builder: &mut dyn EditOperationBuilder,
        s: &Selection,
        indent_converter: &MoveLinesIndentConverter,
    ) {
        let MoveLinesIndentConverter { tab_size, insert_spaces, .. } = *indent_converter;
        let start_line_number = s.start_line_number;
        let end_line_number = s.end_line_number;

        let moving_line_number = end_line_number + 1;
        let moving_line_text = model.get_line_content(moving_line_number);
        // Apaga a linha que será movida
        builder.add_edit_operation(
            Range::new(
                moving_line_number - 1,
                model.get_line_max_column(moving_line_number - 1),
                moving_line_number,
                model.get_line_max_column(moving_line_number),
            ),
            None,
            false,
        );

        let mut inserting_text = moving_line_text.clone();

        if self.should_auto_indent(model, s) {
            let moving_line_match_result =
                self.match_enter_rule(model, indent_converter, tab_size, moving_line_number, start_line_number - 1, None);
            // se `s.startLineNumber - 1` casa uma regra de Enter, ela continua valendo
            if let Some(moving_line_match_result) = moving_line_match_result {
                let old_indentation = leading_whitespace(model.get_line_content(moving_line_number).units());
                let new_space_cnt = moving_line_match_result + get_space_cnt(old_indentation.units(), tab_size);
                let new_indentation = generate_indent(new_space_cnt, tab_size, insert_spaces);
                inserting_text = concat(&new_indentation, trimmed_start(moving_line_text.units()).units());
            } else {
                // nenhuma regra de Enter casa, então confere as regras de indentação
                let virtual_model = MovedLinesVirtualModel::new(
                    model,
                    move |line_number| if line_number == start_line_number { moving_line_number } else { line_number },
                    None,
                );
                let indent_of_moving_line = get_good_indent_for_line(
                    self.auto_indent,
                    &virtual_model,
                    &model.get_language_id_at_position(moving_line_number, 1),
                    start_line_number,
                    indent_converter,
                    &*self.language_configuration_service,
                );
                if let Some(indent_of_moving_line) = indent_of_moving_line {
                    let old_indentation = leading_whitespace(model.get_line_content(moving_line_number).units());
                    let new_space_cnt = get_space_cnt(indent_of_moving_line.units(), tab_size);
                    let old_space_cnt = get_space_cnt(old_indentation.units(), tab_size);
                    if new_space_cnt != old_space_cnt {
                        let new_indentation = generate_indent(new_space_cnt, tab_size, insert_spaces);
                        inserting_text = concat(&new_indentation, trimmed_start(moving_line_text.units()).units());
                    }
                }
            }

            // adiciona primeiro a edição da linha movida, para que rode depois da mudança de indentação
            // de `s.startLineNumber`
            builder.add_edit_operation(
                Range::new(start_line_number, 1, start_line_number, 1),
                Some(concat(&inserting_text, &[10])),
                false,
            );

            let ret =
                self.match_enter_rule_moving_down(model, indent_converter, tab_size, start_line_number, moving_line_number, &inserting_text);

            // se a linha movida casa regras de Enter, ajusta a indentação por elas
            if let Some(ret) = ret {
                if ret != 0 {
                    self.get_indent_edits_of_moving_block(model, builder, s, tab_size, insert_spaces, ret);
                }
            } else {
                // não casa regras de Enter, então confere as regras de indentação
                let overriding_text = inserting_text.clone();
                let virtual_model = MovedLinesVirtualModel::new(
                    model,
                    move |line_number| {
                        if line_number == start_line_number {
                            // TODO@aiday-mar do original: os tokens aqui não correspondem exatamente ao conteúdo
                            // (depois do ajuste de indentação).
                            moving_line_number
                        } else if line_number > start_line_number && line_number <= end_line_number + 1 {
                            line_number - 1
                        } else {
                            line_number
                        }
                    },
                    Some(Box::new(move |line_number| (line_number == start_line_number).then(|| overriding_text.clone()))),
                );

                let new_indent_at_moving_block = get_good_indent_for_line(
                    self.auto_indent,
                    &virtual_model,
                    &model.get_language_id_at_position(moving_line_number, 1),
                    start_line_number + 1,
                    indent_converter,
                    &*self.language_configuration_service,
                );

                if let Some(new_indent_at_moving_block) = new_indent_at_moving_block {
                    let old_indentation = leading_whitespace(model.get_line_content(start_line_number).units());
                    let new_space_cnt = get_space_cnt(new_indent_at_moving_block.units(), tab_size);
                    let old_space_cnt = get_space_cnt(old_indentation.units(), tab_size);
                    if new_space_cnt != old_space_cnt {
                        let space_cnt_offset = new_space_cnt - old_space_cnt;

                        self.get_indent_edits_of_moving_block(model, builder, s, tab_size, insert_spaces, space_cnt_offset);
                    }
                }
            }
        } else {
            // Insere antes a linha que será movida
            builder.add_edit_operation(
                Range::new(start_line_number, 1, start_line_number, 1),
                Some(concat(&inserting_text, &[10])),
                false,
            );
        }
    }

    /// O trecho de `getEditOperations` que move o bloco `s` para cima.
    fn move_block_up(
        &mut self,
        model: &TextModel,
        builder: &mut dyn EditOperationBuilder,
        s: &Selection,
        indent_converter: &MoveLinesIndentConverter,
    ) {
        let MoveLinesIndentConverter { tab_size, insert_spaces, .. } = *indent_converter;
        let start_line_number = s.start_line_number;
        let end_line_number = s.end_line_number;

        let moving_line_number = start_line_number - 1;
        let moving_line_text = model.get_line_content(moving_line_number);

        // Apaga a linha que será movida
        builder.add_edit_operation(Range::new(moving_line_number, 1, moving_line_number + 1, 1), None, false);

        // Insere depois a linha que será movida
        let end_max_column = model.get_line_max_column(end_line_number);
        builder.add_edit_operation(
            Range::new(end_line_number, end_max_column, end_line_number, end_max_column),
            Some(concat(&JsString::from("\n"), moving_line_text.units())),
            false,
        );

        if self.should_auto_indent(model, s) {
            let virtual_model = MovedLinesVirtualModel::new(
                model,
                move |line_number| if line_number == moving_line_number { start_line_number } else { line_number },
                None,
            );

            let ret = self.match_enter_rule(model, indent_converter, tab_size, start_line_number, start_line_number - 2, None);
            // se `s.startLineNumber - 2` casa regras de Enter, ajusta o bloco movido por elas
            if let Some(ret) = ret {
                if ret != 0 {
                    self.get_indent_edits_of_moving_block(model, builder, s, tab_size, insert_spaces, ret);
                }
            } else {
                // nenhuma regra de Enter casa, então confere as regras de indentação
                let indent_of_first_line = get_good_indent_for_line(
                    self.auto_indent,
                    &virtual_model,
                    &model.get_language_id_at_position(start_line_number, 1),
                    moving_line_number,
                    indent_converter,
                    &*self.language_configuration_service,
                );
                if let Some(indent_of_first_line) = indent_of_first_line {
                    // ajusta a indentação do bloco movido
                    let old_indent = leading_whitespace(model.get_line_content(start_line_number).units());
                    let new_space_cnt = get_space_cnt(indent_of_first_line.units(), tab_size);
                    let old_space_cnt = get_space_cnt(old_indent.units(), tab_size);
                    if new_space_cnt != old_space_cnt {
                        let space_cnt_offset = new_space_cnt - old_space_cnt;

                        self.get_indent_edits_of_moving_block(model, builder, s, tab_size, insert_spaces, space_cnt_offset);
                    }
                }
            }
        }
    }
}

impl Command for MoveLinesCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let model_line_count = model.get_line_count();

        if self.is_moving_down && self.selection.end_line_number == model_line_count {
            self.selection_id = Some(builder.track_selection(self.selection, None));
            return;
        }
        if !self.is_moving_down && self.selection.start_line_number == 1 {
            self.selection_id = Some(builder.track_selection(self.selection, None));
            return;
        }

        self.move_end_position_down = false;
        let mut s = self.selection;

        if s.start_line_number < s.end_line_number && s.end_column == 1 {
            self.move_end_position_down = true;
            s = s.set_end_position(s.end_line_number - 1, model.get_line_max_column(s.end_line_number - 1));
        }

        let options = model.get_options();
        let indent_converter = MoveLinesIndentConverter {
            tab_size: options.tab_size,
            indent_size: options.indent_size,
            insert_spaces: options.insert_spaces,
        };

        if s.start_line_number == s.end_line_number && model.get_line_max_column(s.start_line_number) == 1 {
            // A linha atual está vazia
            let line_number = s.start_line_number;
            let other_line_number = if self.is_moving_down { line_number + 1 } else { line_number - 1 };

            if model.get_line_max_column(other_line_number) == 1 {
                // A outra linha também está vazia, então não há o que editar.
                // Adiciona uma operação nula para forçar a execução pelo modelo
                builder.add_edit_operation(Range::new(1, 1, 1, 1), None, false);
            } else {
                // Digita na linha atual o conteúdo da outra
                builder.add_edit_operation(Range::new(line_number, 1, line_number, 1), Some(model.get_line_content(other_line_number)), false);

                // Remove o conteúdo da outra linha
                builder.add_edit_operation(
                    Range::new(other_line_number, 1, other_line_number, model.get_line_max_column(other_line_number)),
                    None,
                    false,
                );
            }
            // Rastreia a seleção na outra linha
            s = Selection::new(other_line_number, 1, other_line_number, 1);
        } else if self.is_moving_down {
            self.move_block_down(model, builder, &s, &indent_converter);
        } else {
            self.move_block_up(model, builder, &s, &indent_converter);
        }

        self.selection_id = Some(builder.track_selection(s, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        let mut result =
            helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"));

        if self.move_end_position_down {
            result = result.set_end_position(result.end_line_number + 1, 1);
        }

        if self.move_end_line_selection_shrink && result.start_line_number < result.end_line_number {
            result = result.set_end_position(result.end_line_number, 2);
        }

        result
    }
}
