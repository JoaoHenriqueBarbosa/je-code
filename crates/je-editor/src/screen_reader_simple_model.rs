//! Porte de `src/vs/editor/common/viewModel/screenReaderSimpleModel.ts`: `ISimpleModel`, o recorte do modelo
//! que a área de entrada de texto lê para montar o estado do leitor de tela.
//!
//! Divergências: os textos de linha e de intervalo são [`JsString`] (UTF-16), os números de linha e coluna são
//! `i32`, e `getValueLengthInRange` devolve `usize` e `modifyPosition` recebe o deslocamento em `i32`, como
//! `IViewModel` em `view_model_types.rs`. Os parâmetros por valor seguem a convenção do crate: `Range` e
//! `Position` são `Copy`.

use je_base::JsString;

use crate::model::EndOfLinePreference;
use crate::position::Position;
use crate::range::Range;

/// `ISimpleModel`.
pub trait ISimpleModel {
    fn get_line_content(&self, line_number: i32) -> JsString;
    fn get_line_count(&self) -> i32;
    fn get_line_max_column(&self, line_number: i32) -> i32;
    fn get_value_in_range(&self, range: Range, eol: EndOfLinePreference) -> JsString;
    fn get_value_length_in_range(&self, range: Range, eol: EndOfLinePreference) -> usize;
    fn modify_position(&self, position: Position, offset: i32) -> Position;
}
