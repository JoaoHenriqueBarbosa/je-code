//! Porte do trecho de `src/vs/editor/common/editorCommon.ts` que os comandos do editor usam: `ICommand`,
//! `IEditOperationBuilder` e `ICursorStateComputerData`.
//!
//! Notas de tradução:
//! - `ITextModel` é o [`TextModel`] concreto.
//! - Argumento opcional do TypeScript vira parâmetro explícito: `forceMoveMarkers?: boolean` é
//!   `force_move_markers: bool` (o padrão do original é `false`) e `trackPreviousOnEmpty?: boolean` é
//!   `Option<bool>`.
//! - `text: string | null` é `Option<JsString>`, como nas demais operações de edição do crate.
//! - `IRange` e `ISelection` chegam como [`Range`] e [`Selection`].
//! - Os métodos de [`Command`] recebem `&mut self` porque os comandos do original guardam estado entre
//!   `getEditOperations` e `computeCursorState` (por exemplo o id da seleção rastreada).
//!
//! TODO(integração): `editorCommon.ts` tem muito mais (`IEditor`, `ICodeEditor`, `IEditorAction`, eventos,
//! `Handler`, `ScrollType`, ...). Este módulo traz só o que os comandos precisam hoje, e deve ser fundido ao
//! restante quando o resto do arquivo for portado.

use je_base::JsString;

use crate::cursor_type_edit_operations::BaseTypeWithAutoClosingCommand;
use crate::model::ValidEditOperation;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `IEditOperationBuilder`: o que um comando usa para pedir edições e rastrear seleções.
pub trait EditOperationBuilder {
    /// `addEditOperation(range, text, forceMoveMarkers)`.
    fn add_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool);

    /// `addTrackedEditOperation(range, text, forceMoveMarkers)`: como [`Self::add_edit_operation`], mas
    /// avisa o modelo de que a edição é rastreada.
    fn add_tracked_edit_operation(&mut self, range: Range, text: Option<JsString>, force_move_markers: bool);

    /// `trackSelection(selection, trackPreviousOnEmpty)`: devolve o id para
    /// [`CursorStateComputerData::get_tracked_selection`].
    fn track_selection(&mut self, selection: Selection, track_previous_on_empty: Option<bool>) -> String;
}

/// `ICursorStateComputerData`: o que o comando recebe ao calcular o estado do cursor depois da edição.
pub trait CursorStateComputerData {
    /// `getInverseEditOperations()`: as operações inversas deste comando, ordenadas pelo `minor`.
    fn get_inverse_edit_operations(&self) -> Vec<ValidEditOperation>;

    /// `getTrackedSelection(id)`: a seleção rastreada com o id devolvido por `trackSelection`.
    fn get_tracked_selection(&self, id: &str) -> Selection;
}

/// `ICommand`.
pub trait Command {
    /// `insertsAutoWhitespace`: as edições deste comando inserem espaço em branco automático.
    fn inserts_auto_whitespace(&self) -> bool {
        false
    }

    /// `getEditOperations(model, builder)`.
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder);

    /// `computeCursorState(model, helper)`.
    fn compute_cursor_state(&mut self, model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection;

    /// O `command instanceof BaseTypeWithAutoClosingCommand` do controlador de cursores: os comandos que
    /// digitam e fecham um par automaticamente devolvem a base (com `close_character_range` e
    /// `enclosing_range`, preenchidos depois de `compute_cursor_state`); os demais devolvem `None`.
    fn as_type_with_auto_closing(&self) -> Option<&BaseTypeWithAutoClosingCommand> {
        None
    }
}

/// `ScrollType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollType {
    Smooth = 0,
    Immediate = 1,
}
