//! Porte de `src/vs/editor/common/model/guidesTextModelPart.ts`: os guias de indentação
//! (`getActiveIndentGuide`, `getLinesIndentGuides`) e os guias de pares de colchetes
//! (`getLinesBracketGuides`).
//!
//! Divergências:
//! - `TextModelPart` (a base que só guarda o estado "descartado") é o `assert_not_disposed` do modelo.
//! - O estado `-2` ("ainda não calculado") dos índices de linha com conteúdo é o do original; por isso o
//!   estado de cada direção é um [`ContentLines`] passado às funções de resolução, no lugar das variáveis
//!   capturadas pelas closures do original.

use crate::cursor_common::CursorSimpleModel;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::strings::first_non_whitespace_index;

use crate::cursor_columns::visible_column_from_column;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::model_utils::compute_indent_level;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::text_model_guides::{
    ActiveIndentGuideInfo, BracketGuideOptions, HorizontalGuidesState, IndentGuide,
    IndentGuideHorizontalLine,
};

/// O estado das linhas com conteúdo acima e abaixo, como no original (`-2`: ainda não calculado).
struct ContentLines {
    above_index: i64,
    above_indent: i32,
    below_index: i64,
    below_indent: i32,
}

impl ContentLines {
    fn new() -> Self {
        Self {
            above_index: -2,
            above_indent: -1,
            below_index: -2,
            below_indent: -1,
        }
    }
}

/// `GuidesTextModelPart`.
pub struct GuidesTextModelPart {
    text_model: Weak<TextModel>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
}

impl GuidesTextModelPart {
    pub fn new(
        text_model: Weak<TextModel>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        Self {
            text_model,
            language_configuration_service,
        }
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model
            .upgrade()
            .expect("o modelo de texto foi descartado")
    }

    fn compute_indent_level(&self, model: &TextModel, line_index: i64) -> i32 {
        compute_indent_level(
            model.get_line_content(line_index as i32 + 1).units(),
            model.get_options().tab_size,
        )
    }

    fn off_side(&self, model: &TextModel) -> bool {
        self.language_configuration_service
            .get_language_configuration(&model.get_language_id())
            .folding_rules
            .off_side
            .unwrap_or(false)
    }

    /// Acha a linha com conteúdo acima (a partir de `from_index`, descendo) e guarda no estado.
    fn find_above(&self, model: &TextModel, state: &mut ContentLines, line_number: i32) {
        state.above_index = -1;
        state.above_indent = -1;

        // tem de achar a linha anterior com conteúdo
        let mut line_index = i64::from(line_number) - 2;
        while line_index >= 0 {
            let indent = self.compute_indent_level(model, line_index);
            if indent >= 0 {
                state.above_index = line_index;
                state.above_indent = indent;
                break;
            }
            line_index -= 1;
        }
    }

    /// Acha a próxima linha com conteúdo abaixo e guarda no estado.
    fn find_below(
        &self,
        model: &TextModel,
        state: &mut ContentLines,
        line_number: i32,
        line_count: i32,
    ) {
        state.below_index = -1;
        state.below_indent = -1;

        // tem de achar a próxima linha com conteúdo
        for line_index in i64::from(line_number)..i64::from(line_count) {
            let indent = self.compute_indent_level(model, line_index);
            if indent >= 0 {
                state.below_index = line_index;
                state.below_indent = indent;
                break;
            }
        }
    }

    pub fn get_active_indent_guide(
        &self,
        line_number: i32,
        min_line_number: i32,
        max_line_number: i32,
    ) -> ActiveIndentGuideInfo {
        let model = self.model();
        model.assert_not_disposed();
        let line_count = model.get_line_count();

        if line_number < 1 || line_number > line_count {
            errors::throw(Error::bug_indicating(Some("Illegal value for lineNumber")));
        }

        let off_side = self.off_side(&model);
        let indent_size = model.get_options().indent_size;

        let mut up = ContentLines::new();
        let up_resolve_indents = |this: &Self, up: &mut ContentLines, line_number: i32| {
            if up.above_index != -1
                && (up.above_index == -2 || up.above_index > i64::from(line_number) - 1)
            {
                this.find_above(&model, up, line_number);
            }

            if up.below_index == -2 {
                this.find_below(&model, up, line_number, line_count);
            }
        };

        let mut down = ContentLines::new();
        let down_resolve_indents = |this: &Self, down: &mut ContentLines, line_number: i32| {
            if down.above_index == -2 {
                this.find_above(&model, down, line_number);
            }

            if down.below_index != -1
                && (down.below_index == -2 || down.below_index < i64::from(line_number) - 1)
            {
                this.find_below(&model, down, line_number, line_count);
            }
        };

        let mut start_line_number = 0;
        let mut go_up = true;
        let mut end_line_number = 0;
        let mut go_down = true;
        let mut indent = 0;

        let mut initial_indent = 0;

        let mut distance = 0;
        while go_up || go_down {
            let up_line_number = line_number - distance;
            let down_line_number = line_number + distance;

            if distance > 1 && (up_line_number < 1 || up_line_number < min_line_number) {
                go_up = false;
            }
            if distance > 1 && (down_line_number > line_count || down_line_number > max_line_number)
            {
                go_down = false;
            }
            if distance > 50000 {
                // para de processar
                go_up = false;
                go_down = false;
            }

            let mut up_line_indent_level: i32 = -1;
            if go_up && up_line_number >= 1 {
                // calcula o nível de indentação subindo
                let current_indent =
                    self.compute_indent_level(&model, i64::from(up_line_number) - 1);
                if current_indent >= 0 {
                    // Esta linha tem conteúdo (além de espaço em branco)
                    // Usa a indentação da linha
                    up.below_index = i64::from(up_line_number) - 1;
                    up.below_indent = current_indent;
                    up_line_indent_level = ceil_div(current_indent, indent_size);
                } else {
                    up_resolve_indents(self, &mut up, up_line_number);
                    up_line_indent_level = self.get_indent_level_for_whitespace_line(
                        &model,
                        off_side,
                        up.above_indent,
                        up.below_indent,
                    );
                }
            }

            let mut down_line_indent_level: i32 = -1;
            if go_down && down_line_number <= line_count {
                // calcula o nível de indentação descendo
                let current_indent =
                    self.compute_indent_level(&model, i64::from(down_line_number) - 1);
                if current_indent >= 0 {
                    // Esta linha tem conteúdo (além de espaço em branco)
                    // Usa a indentação da linha
                    down.above_index = i64::from(down_line_number) - 1;
                    down.above_indent = current_indent;
                    down_line_indent_level = ceil_div(current_indent, indent_size);
                } else {
                    down_resolve_indents(self, &mut down, down_line_number);
                    down_line_indent_level = self.get_indent_level_for_whitespace_line(
                        &model,
                        off_side,
                        down.above_indent,
                        down.below_indent,
                    );
                }
            }

            if distance == 0 {
                initial_indent = up_line_indent_level;
                distance += 1;
                continue;
            }

            if distance == 1 {
                if down_line_number <= line_count
                    && down_line_indent_level >= 0
                    && initial_indent + 1 == down_line_indent_level
                {
                    // Este é o começo de um escopo, tratamos de forma especial, já que queremos que a
                    // indentação do escopo filho fique ativa, não a do pai
                    go_up = false;
                    start_line_number = down_line_number;
                    end_line_number = down_line_number;
                    indent = down_line_indent_level;
                    distance += 1;
                    continue;
                }

                if up_line_number >= 1
                    && up_line_indent_level >= 0
                    && up_line_indent_level - 1 == initial_indent
                {
                    // Este é o fim de um escopo, como acima
                    go_down = false;
                    start_line_number = up_line_number;
                    end_line_number = up_line_number;
                    indent = up_line_indent_level;
                    distance += 1;
                    continue;
                }

                start_line_number = line_number;
                end_line_number = line_number;
                indent = initial_indent;
                if indent == 0 {
                    // Não precisa continuar
                    return ActiveIndentGuideInfo {
                        start_line_number,
                        end_line_number,
                        indent,
                    };
                }
            }

            if go_up {
                if up_line_indent_level >= indent {
                    start_line_number = up_line_number;
                } else {
                    go_up = false;
                }
            }
            if go_down {
                if down_line_indent_level >= indent {
                    end_line_number = down_line_number;
                } else {
                    go_down = false;
                }
            }
            distance += 1;
        }

        ActiveIndentGuideInfo {
            start_line_number,
            end_line_number,
            indent,
        }
    }

    pub fn get_lines_bracket_guides(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        active_position: Option<Position>,
        options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>> {
        let model = self.model();
        let mut result: Vec<Vec<IndentGuide>> = (start_line_number..=end_line_number)
            .map(|_| Vec::new())
            .collect();

        // Se for pedido, isto pode virar configurável.
        let include_single_line_pairs = true;

        let bracket_pairs = model
            .bracket_pairs()
            .get_bracket_pairs_in_range_with_min_indentation(Range::new(
                start_line_number,
                1,
                end_line_number,
                model.get_line_max_column(end_line_number),
            ))
            .to_array();

        let mut active_bracket_pair_range: Option<Range> = None;
        if let Some(active_position) = active_position
            && !bracket_pairs.is_empty()
        {
            let candidates = if start_line_number <= active_position.line_number
                && active_position.line_number <= end_line_number
            {
                // Não precisamos consultar os colchetes de novo se o cursor está na viewport
                bracket_pairs.clone()
            } else {
                model
                    .bracket_pairs()
                    .get_bracket_pairs_in_range(Range::from_positions(
                        active_position,
                        active_position,
                    ))
                    .to_array()
            };
            let containing: Vec<_> = candidates
                .into_iter()
                .filter(|bp| bp.range.strict_contains_position(&active_position))
                .collect();

            active_bracket_pair_range = containing
                .iter()
                .rfind(|i| {
                    include_single_line_pairs
                        || i.range.start_line_number != i.range.end_line_number
                })
                .map(|i| i.range);
        }

        let independent_color_pool_per_bracket_type = model
            .get_options()
            .bracket_pair_colorization_options
            .independent_color_pool_per_bracket_type;
        let color_provider = BracketPairGuidesClassNames;

        for pair in &bracket_pairs {
            let Some(closing_bracket_range) = pair.closing_bracket_range else {
                continue;
            };

            let is_active = active_bracket_pair_range
                .as_ref()
                .is_some_and(|r| pair.range == *r);

            if !is_active && !options.include_inactive {
                continue;
            }

            let class_name = color_provider.get_inline_class_name(
                pair.nesting_level,
                pair.nesting_level_of_equal_bracket_type,
                independent_color_pool_per_bracket_type,
            ) + &(if options.highlight_active && is_active {
                format!(" {}", color_provider.active_class_name())
            } else {
                String::new()
            });

            let start = pair.opening_bracket_range.get_start_position();
            let end = closing_bracket_range.get_start_position();

            let horizontal_guides = options.horizontal_guides == HorizontalGuidesState::Enabled
                || (options.horizontal_guides == HorizontalGuidesState::EnabledForActive
                    && is_active);

            if pair.range.start_line_number == pair.range.end_line_number {
                if include_single_line_pairs && horizontal_guides {
                    result[(pair.range.start_line_number - start_line_number) as usize].push(
                        IndentGuide::new(
                            -1,
                            pair.opening_bracket_range.get_end_position().column,
                            class_name.clone(),
                            Some(IndentGuideHorizontalLine {
                                top: false,
                                end_column: end.column,
                            }),
                            -1,
                            -1,
                        ),
                    );
                }
                continue;
            }

            let end_visible_column = self.get_visible_column_from_position(&model, end);
            let start_visible_column = self.get_visible_column_from_position(
                &model,
                pair.opening_bracket_range.get_start_position(),
            );
            let guide_visible_column = i64::from(start_visible_column)
                .min(i64::from(end_visible_column))
                .min(pair.min_visible_column_indentation + 1)
                as i32;

            let mut render_horizontal_end_line_at_the_bottom = false;

            let first_non_ws_index = first_non_whitespace_index(
                model
                    .get_line_content(closing_bracket_range.start_line_number)
                    .units(),
            );
            // `firstNonWhitespaceIndex` devolve -1 numa linha toda em branco, e o original compara com `-1`.
            let first_non_ws_index = first_non_ws_index.map_or(-1_i64, |i| i as i64);
            let has_text_before_closing_bracket =
                first_non_ws_index < i64::from(closing_bracket_range.start_column) - 1;
            if has_text_before_closing_bracket {
                render_horizontal_end_line_at_the_bottom = true;
            }

            let visible_guide_start_line_number = start.line_number.max(start_line_number);
            let visible_guide_end_line_number = end.line_number.min(end_line_number);

            let offset = i32::from(render_horizontal_end_line_at_the_bottom);

            for l in visible_guide_start_line_number..visible_guide_end_line_number + offset {
                result[(l - start_line_number) as usize].push(IndentGuide::new(
                    guide_visible_column,
                    -1,
                    class_name.clone(),
                    None,
                    if l == start.line_number {
                        start.column
                    } else {
                        -1
                    },
                    if l == end.line_number { end.column } else { -1 },
                ));
            }

            if horizontal_guides {
                if start.line_number >= start_line_number
                    && start_visible_column > guide_visible_column
                {
                    result[(start.line_number - start_line_number) as usize].push(
                        IndentGuide::new(
                            guide_visible_column,
                            -1,
                            class_name.clone(),
                            Some(IndentGuideHorizontalLine {
                                top: false,
                                end_column: start.column,
                            }),
                            -1,
                            -1,
                        ),
                    );
                }

                if end.line_number <= end_line_number && end_visible_column > guide_visible_column {
                    result[(end.line_number - start_line_number) as usize].push(IndentGuide::new(
                        guide_visible_column,
                        -1,
                        class_name.clone(),
                        Some(IndentGuideHorizontalLine {
                            top: !render_horizontal_end_line_at_the_bottom,
                            end_column: end.column,
                        }),
                        -1,
                        -1,
                    ));
                }
            }
        }

        for guides in &mut result {
            // `sort` do JavaScript é estável, como o `sort_by` do Rust
            guides.sort_by_key(|g| g.visible_column);
        }

        result
    }

    fn get_visible_column_from_position(&self, model: &TextModel, position: Position) -> i32 {
        visible_column_from_column(
            model.get_line_content(position.line_number).units(),
            position.column,
            model.get_options().tab_size,
        ) + 1
    }

    pub fn get_lines_indent_guides(
        &self,
        start_line_number: i32,
        end_line_number: i32,
    ) -> Vec<i32> {
        let model = self.model();
        model.assert_not_disposed();
        let line_count = model.get_line_count();

        if start_line_number < 1 || start_line_number > line_count {
            errors::throw(Error::new("Illegal value for startLineNumber"));
        }
        if end_line_number < 1 || end_line_number > line_count {
            errors::throw(Error::new("Illegal value for endLineNumber"));
        }

        let options = model.get_options();
        let off_side = self.off_side(&model);

        let mut result: Vec<i32> =
            vec![0; (end_line_number - start_line_number + 1).max(0) as usize];

        let mut state = ContentLines::new();

        for line_number in start_line_number..=end_line_number {
            let result_index = (line_number - start_line_number) as usize;

            let current_indent = self.compute_indent_level(&model, i64::from(line_number) - 1);
            if current_indent >= 0 {
                // Esta linha tem conteúdo (além de espaço em branco)
                // Usa a indentação da linha
                state.above_index = i64::from(line_number) - 1;
                state.above_indent = current_indent;
                result[result_index] = ceil_div(current_indent, options.indent_size);
                continue;
            }

            if state.above_index == -2 {
                self.find_above(&model, &mut state, line_number);
            }

            if state.below_index != -1
                && (state.below_index == -2 || state.below_index < i64::from(line_number) - 1)
            {
                self.find_below(&model, &mut state, line_number, line_count);
            }

            result[result_index] = self.get_indent_level_for_whitespace_line(
                &model,
                off_side,
                state.above_indent,
                state.below_indent,
            );
        }
        result
    }

    fn get_indent_level_for_whitespace_line(
        &self,
        model: &TextModel,
        off_side: bool,
        above_content_line_indent: i32,
        below_content_line_indent: i32,
    ) -> i32 {
        let options = model.get_options();

        if above_content_line_indent == -1 || below_content_line_indent == -1 {
            // No topo ou no fim do arquivo
            0
        } else if above_content_line_indent < below_content_line_indent {
            // estamos dentro da região acima
            1 + above_content_line_indent / options.indent_size
        } else if above_content_line_indent == below_content_line_indent {
            // estamos entre duas regiões
            ceil_div(below_content_line_indent, options.indent_size)
        } else if off_side {
            // mesmo nível da região abaixo
            ceil_div(below_content_line_indent, options.indent_size)
        } else {
            // estamos dentro da região que termina abaixo
            1 + below_content_line_indent / options.indent_size
        }
    }
}

/// `Math.ceil(a / b)` para `a >= 0` e `b > 0`.
fn ceil_div(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}

/// `BracketPairGuidesClassNames`.
pub struct BracketPairGuidesClassNames;

impl BracketPairGuidesClassNames {
    pub fn active_class_name(&self) -> &'static str {
        "indent-active"
    }

    pub fn get_inline_class_name(
        &self,
        nesting_level: i64,
        nesting_level_of_equal_bracket_type: i64,
        independent_color_pool_per_bracket_type: bool,
    ) -> String {
        self.get_inline_class_name_of_level(if independent_color_pool_per_bracket_type {
            nesting_level_of_equal_bracket_type
        } else {
            nesting_level
        })
    }

    pub fn get_inline_class_name_of_level(&self, level: i64) -> String {
        // Para suportar uma quantidade dinâmica de até 6 cores, usa-se um número que é o mínimo múltiplo
        // comum de 1 a 6.
        format!("bracket-indent-guide lvl-{}", level % 30)
    }
}
