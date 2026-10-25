//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/controller/common.ts`: o ponto de acesso ao
//! `InlineCompletionsController` de um editor sem depender do tipo do controller.
//!
//! Notas de tradução:
//! - o `import type { InlineCompletionsController }` do original existe para quebrar o ciclo de módulos do
//!   TypeScript; o tipo do controller (fatia 4c) ainda não tem porte, então o getter devolve `Rc<dyn Any>` e quem lê o
//!   controller escolhe o tipo em [`get_inline_completions_controller`] (`get_inline_completions_controller::<T>(editor)`);
//! - o getter é uma variável de módulo (`let _getInlineCompletionsController`), que aqui é um `thread_local`: o editor
//!   inteiro roda no thread da UI e os `Rc` não cruzam threads;
//! - o `ICodeEditor` do original é o `Rc<CodeEditorWidget>`, que é o editor que o porte tem.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::code_editor_widget::CodeEditorWidget;

/// O getter registrado: o controller do editor, sem tipo.
type ControllerGetter = Rc<dyn Fn(&Rc<CodeEditorWidget>) -> Option<Rc<dyn Any>>>;

thread_local! {
    /// `_getInlineCompletionsController`.
    static GETTER: RefCell<Option<ControllerGetter>> = const { RefCell::new(None) };
}

/// `getInlineCompletionsController(editor)`: `T` é o tipo do controller; `None` se não há getter, se o editor não tem
/// controller ou se o controller não é um `T`.
pub fn get_inline_completions_controller<T: 'static>(editor: &Rc<CodeEditorWidget>) -> Option<Rc<T>> {
    // O getter roda fora do empréstimo, porque ele pode registrar outro getter ou pedir outro controller.
    let getter = GETTER.with(|getter| getter.borrow().clone())?;
    getter(editor)?.downcast::<T>().ok()
}

/// `setInlineCompletionsControllerGetter(getter)`.
pub fn set_inline_completions_controller_getter(getter: impl Fn(&Rc<CodeEditorWidget>) -> Option<Rc<dyn Any>> + 'static) {
    GETTER.with(|slot| *slot.borrow_mut() = Some(Rc::new(getter)));
}
