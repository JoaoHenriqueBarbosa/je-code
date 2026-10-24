//! Porte de `JavaScriptCore/debugger/DebuggerParseData.h` e `.cpp`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::nodes::{ModuleProgramNode, ProgramNode};
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{
    JSParserBuiltinMode, JSParserScriptMode, LexicallyScopedFeatures, SourceParseMode,
    NO_LEXICALLY_SCOPED_FEATURES, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::parser::{parse_root_node, ParsedNode};
use crate::parser::parser_tokens::JSTextPosition;
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::{SourceProvider, SourceProviderSourceType};
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;

/// `DebuggerPausePositionType`. A ordem das constantes importa: ela desempata posições com o mesmo
/// offset na ordenação.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum DebuggerPausePositionType {
    #[default]
    Invalid,
    Enter,
    Pause,
    Leave,
}

/// `DebuggerPausePosition`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DebuggerPausePosition {
    pub type_: DebuggerPausePositionType,
    pub position: JSTextPosition,
}

/// `DebuggerPausePositions`.
#[derive(Default)]
pub struct DebuggerPausePositions {
    positions: Vec<DebuggerPausePosition>,
}

impl DebuggerPausePositions {
    pub fn append_pause(&mut self, position: JSTextPosition) {
        self.positions.push(DebuggerPausePosition { type_: DebuggerPausePositionType::Pause, position });
    }

    pub fn append_entry(&mut self, position: JSTextPosition) {
        self.positions.push(DebuggerPausePosition { type_: DebuggerPausePositionType::Enter, position });
    }

    pub fn append_leave(&mut self, position: JSTextPosition) {
        self.positions.push(DebuggerPausePosition { type_: DebuggerPausePositionType::Leave, position });
    }

    pub fn for_each_breakpoint_location(
        &self,
        start_line: i32,
        start_column: i32,
        end_line: i32,
        end_column: i32,
        mut callback: impl FnMut(&JSTextPosition),
    ) {
        let is_after_end = |line: i32, column: i32| (line == end_line && column >= end_column) || line > end_line;

        let mut unique_positions: Vec<JSTextPosition> = Vec::new();
        let mut index = self.first_position_after(start_line, start_column);
        while index < self.positions.len() {
            let line = self.positions[index].position.line;
            let column = self.positions[index].position.column();

            if is_after_end(line, column) {
                break;
            }

            if let Some(resolved) = self.breakpoint_location_for_line_column_from(line, column, index) {
                if !is_after_end(resolved.line, resolved.column()) && !unique_positions.contains(&resolved) {
                    unique_positions.push(resolved);
                }
            }
            index += 1;
        }
        unique_positions.sort_by(|a, b| {
            if a.line == b.line {
                return a.column().cmp(&b.column());
            }
            a.line.cmp(&b.line)
        });
        for position in &unique_positions {
            callback(position);
        }
    }

    /// `firstPositionAfter`: o `lower_bound` por (linha, coluna); devolve o índice (`end()` é `len()`).
    fn first_position_after(&self, line: i32, column: i32) -> usize {
        self.positions.partition_point(|p| {
            if p.position.line == line {
                return p.position.column() < column;
            }
            p.position.line < line
        })
    }

    pub fn breakpoint_location_for_line_column(&self, line: i32, column: i32) -> Option<JSTextPosition> {
        self.breakpoint_location_for_line_column_from(line, column, self.first_position_after(line, column))
    }

    fn breakpoint_location_for_line_column_from(&self, line: i32, column: i32, mut it: usize) -> Option<JSTextPosition> {
        if it == self.positions.len() {
            return None;
        }

        debug_assert!(line <= self.positions[it].position.line);
        debug_assert!(line != self.positions[it].position.line || column <= self.positions[it].position.column());

        if line == self.positions[it].position.line && column == self.positions[it].position.column() {
            // Posição exata. Avança se for um Enter de função: há sempre um Leave correspondente,
            // então não precisa checar o limite.
            while self.positions[it].type_ == DebuggerPausePositionType::Enter {
                it += 1;
            }
            return Some(self.positions[it].position);
        }

        // Se a próxima posição é o Enter de uma função, decide-se entrar nela ou passar por cima:
        // entra se a entrada estiver na mesma linha do pedido.
        let first_slide_position = self.positions[it];
        if first_slide_position.type_ != DebuggerPausePositionType::Enter {
            return Some(first_slide_position.position);
        }

        // Se `entry_stack_size` > 0, está pulando funções.
        let should_enter_function = first_slide_position.position.line == line;
        let mut entry_stack_size: i32 = if should_enter_function { 0 } else { 1 };
        it += 1;
        while it < self.positions.len() {
            let slide_position = &self.positions[it];
            debug_assert!(entry_stack_size >= 0);

            // Já está pulando funções.
            if entry_stack_size != 0 {
                if slide_position.type_ == DebuggerPausePositionType::Enter {
                    entry_stack_size += 1;
                } else if slide_position.type_ == DebuggerPausePositionType::Leave {
                    entry_stack_size -= 1;
                }
                it += 1;
                continue;
            }

            // Começa a pular funções.
            if slide_position.type_ == DebuggerPausePositionType::Enter {
                entry_stack_size += 1;
                it += 1;
                continue;
            }

            // Achou a posição de pausa.
            return Some(slide_position.position);
        }

        // Nenhuma posição de pausa.
        None
    }

    pub fn sort(&mut self) {
        self.positions.sort_by(|a, b| {
            if a.position.offset == b.position.offset {
                return a.type_.cmp(&b.type_);
            }
            a.position.offset.cmp(&b.position.offset)
        });
    }
}

/// `DebuggerParseData`.
#[derive(Default)]
pub struct DebuggerParseData {
    pub pause_positions: DebuggerPausePositions,
}

/// `DebuggerParseInfo<T>`: os parâmetros de parse de `Program` e `Module`.
trait DebuggerParseInfo: ParsedNode {
    const LEXICALLY_SCOPED_FEATURES: LexicallyScopedFeatures;
    const PARSE_MODE: SourceParseMode;
    const SCRIPT_MODE: JSParserScriptMode;
}

impl DebuggerParseInfo for ProgramNode {
    const LEXICALLY_SCOPED_FEATURES: LexicallyScopedFeatures = NO_LEXICALLY_SCOPED_FEATURES;
    const PARSE_MODE: SourceParseMode = SourceParseMode::ProgramMode;
    const SCRIPT_MODE: JSParserScriptMode = JSParserScriptMode::Classic;
}

impl DebuggerParseInfo for ModuleProgramNode {
    const LEXICALLY_SCOPED_FEATURES: LexicallyScopedFeatures = STRICT_MODE_LEXICALLY_SCOPED_FEATURE;
    const PARSE_MODE: SourceParseMode = SourceParseMode::ModuleEvaluateMode;
    const SCRIPT_MODE: JSParserScriptMode = JSParserScriptMode::Module;
}

/// `gatherDebuggerParseData<T>`.
fn gather_debugger_parse_data<T: DebuggerParseInfo>(
    vm: &Rc<VM>,
    source: &SourceCode,
    debugger_parse_data: &Rc<RefCell<DebuggerParseData>>,
) -> bool {
    let mut error = ParserError::new();
    let root_node = parse_root_node::<T>(
        vm,
        source,
        ImplementationVisibility::Public,
        JSParserBuiltinMode::NotBuiltin,
        T::LEXICALLY_SCOPED_FEATURES,
        T::SCRIPT_MODE,
        T::PARSE_MODE,
        &mut error,
        ConstructorKind::None,
        None,
        Some(debugger_parse_data.clone()),
    );
    if root_node.is_none() {
        return false;
    }

    debugger_parse_data.borrow_mut().pause_positions.sort();

    true
}

/// `gatherDebuggerParseDataForSource`. O `DebuggerParseData&` do C++ vira `Rc<RefCell<..>>`, que é o
/// que o parser guarda.
pub fn gather_debugger_parse_data_for_source(
    vm: &Rc<VM>,
    provider: &Rc<dyn SourceProvider>,
    debugger_parse_data: &Rc<RefCell<DebuggerParseData>>,
) -> bool {
    let start_line = provider.start_position().line.one_based_int();
    let start_column = provider.start_position().column.one_based_int();
    let complete_source = SourceCode::with_position(provider.clone(), start_line, start_column);

    match provider.source_type() {
        SourceProviderSourceType::Program => {
            gather_debugger_parse_data::<ProgramNode>(vm, &complete_source, debugger_parse_data)
        }
        SourceProviderSourceType::Module | SourceProviderSourceType::BunTranspiledModule => {
            gather_debugger_parse_data::<ModuleProgramNode>(vm, &complete_source, debugger_parse_data)
        }
        _ => false,
    }
}
