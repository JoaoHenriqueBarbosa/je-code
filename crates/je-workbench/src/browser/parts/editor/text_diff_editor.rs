//! Porte parcial de `src/vs/workbench/browser/parts/editor/textDiffEditor.ts`: o painel `TextDiffEditor` no recorte que os
//! comandos de editor de diff leem (o controle `IDiffEditor`, a entrada e o grupo), mais o contrato `IDiffEditor` de
//! `editor/browser/editorBrowser.ts` nesse mesmo recorte.
//!
//! Divergências:
//! - **`IDiffEditor`.** O widget de diff do `je-editor` não é um `ICodeEditor`/`IDiffEditor` completo (ver `editor_browser`):
//!   o contrato aqui tem só os quatro membros que `diffEditorCommandsService.ts` chama (`getOriginalEditor`,
//!   `getModifiedEditor`, `goToDiff` e `resetWidthBasedLayout`). Cada lado é o [`IActiveCodeEditor`] que o `IEditorService`
//!   já devolve para o `ICodeEditor`.
//! - **`goToDiff('next' | 'previous')`** é o [`GoToDiffTarget`].
//! - **`TextDiffEditor`.** O painel de widgets (`create`, `setInput`, `layout`, a configuração, o estado de visão) não entra:
//!   a árvore de widgets dele vem com o editor de diff nativo. O painel guarda o controle, a entrada e o grupo que o original
//!   expõe por `getControl()`, `input` e `group`, e é reconhecido pelos consumidores como o `instanceof TextDiffEditor` do
//!   original (o `downcast_ref` de [`IEditorPane::as_any`]).
//! - **`resetDiffEditorWidthBasedLayout`.** O `hasDiffEditorWidthBasedLayoutReset` do original testa a propriedade por forma
//!   (`'resetDiffEditorWidthBasedLayout' in pane`); aqui é o trait [`IDiffEditorWidthBasedLayoutReset`], que o painel
//!   devolve em [`IEditorPane::as_diff_editor_width_based_layout_reset`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;

use crate::common::editor::{EditorInputRef, GroupIdentifier, IEditorPane, TEXT_DIFF_EDITOR_ID};
use crate::services::editor::editor_service::IActiveCodeEditor;

/// O argumento de `goToDiff(target: 'next' | 'previous')`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoToDiffTarget {
    Next,
    Previous,
}

/// `IDiffEditor`, no recorte que os comandos de diff usam.
pub trait IDiffEditor {
    /// `getOriginalEditor()`: o editor do lado original.
    fn get_original_editor(&self) -> Rc<dyn IActiveCodeEditor>;

    /// `getModifiedEditor()`: o editor do lado modificado.
    fn get_modified_editor(&self) -> Rc<dyn IActiveCodeEditor>;

    /// `goToDiff(target)`: vai para a próxima ou a anterior mudança.
    fn go_to_diff(&self, target: GoToDiffTarget);

    /// `resetWidthBasedLayout()`: volta ao layout que depende da largura (o modo automático).
    fn reset_width_based_layout(&self);
}

/// `IDiffEditorWidthBasedLayoutReset` (`diffEditorCommandsService.ts`): o painel que reinicia o layout por largura de todos os
/// editores de diff embutidos nele.
pub trait IDiffEditorWidthBasedLayoutReset {
    /// `resetDiffEditorWidthBasedLayout()`.
    fn reset_diff_editor_width_based_layout(&self);
}

/// `TextDiffEditor`, no recorte do controle, da entrada e do grupo (ver a nota do módulo).
pub struct TextDiffEditor {
    control: RefCell<Option<Rc<dyn IDiffEditor>>>,
    input: RefCell<Option<EditorInputRef>>,
    group_id: Cell<Option<GroupIdentifier>>,
}

impl TextDiffEditor {
    /// Um painel com o controle dado, sem entrada nem grupo atribuídos (o `Object.create(TextDiffEditor.prototype)` do
    /// original, que não passa pelo construtor).
    pub fn new(control: Option<Rc<dyn IDiffEditor>>) -> Rc<Self> {
        Rc::new(Self { control: RefCell::new(control), input: RefCell::new(None), group_id: Cell::new(None) })
    }

    /// `setInput(input)`: a entrada atribuída ao painel.
    pub fn set_input(&self, input: Option<EditorInputRef>) {
        *self.input.borrow_mut() = input;
    }

    /// O grupo em que o painel aparece (`group`).
    pub fn set_group_id(&self, group_id: Option<GroupIdentifier>) {
        self.group_id.set(group_id);
    }

    /// `group?.id`: `None` enquanto o painel não tem grupo atribuído (o `group` indefinido do original).
    pub fn group(&self) -> Option<GroupIdentifier> {
        self.group_id.get()
    }
}

impl IEditorPane for TextDiffEditor {
    fn input(&self) -> Option<EditorInputRef> {
        self.input.borrow().clone()
    }

    fn group_id(&self) -> GroupIdentifier {
        match self.group_id.get() {
            Some(group_id) => group_id,
            None => je_base::errors::throw(je_base::errors::Error::new("O painel de diff de texto não tem grupo atribuído")),
        }
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        None
    }

    fn on_did_change_control(&self) -> Event<()> {
        Event::none()
    }

    fn get_id(&self) -> String {
        TEXT_DIFF_EDITOR_ID.to_owned()
    }

    fn get_diff_editor_control(&self) -> Option<Rc<dyn IDiffEditor>> {
        self.control.borrow().clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
