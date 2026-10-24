//! Porte de `src/vs/workbench/common/comments.ts`: `MarshalledCommentThread`, a forma com que uma thread de
//! comentários atravessa o protocolo como argumento de comando.
//!
//! Divergências:
//! - O argumento chega como [`Dynamic`] (um objeto JSON), então o struct tem [`MarshalledCommentThread::from_dynamic`]
//!   (a leitura dos três campos, `None` se o valor não tem a forma) e [`MarshalledCommentThread::to_dynamic`]. Os
//!   handles (`number`) são `u32`, e um número que não é um handle possível (negativo, fracionário) lê como ausente:
//!   nenhum controlador ou thread existe com ele, que é o que o `Map.get` do original concluiria.
//! - `MarshalledCommentThreadInternal` (o `thread: CommentThread` do modelo da thread principal) não é portado: só
//!   a thread principal o cria e o host de extensões lê dele os mesmos dois handles.

use je_base::dynamic::Dynamic;
use je_base::marshalling_ids::MarshalledId;

/// O `number` de uma propriedade como handle: um inteiro não negativo que cabe em `u32`.
pub fn handle_of(value: &Dynamic) -> Option<u32> {
    match value {
        Dynamic::Number(number) if number.fract() == 0.0 && *number >= 0.0 && *number <= f64::from(u32::MAX) => Some(*number as u32),
        _ => None,
    }
}

/// `MarshalledCommentThread`: `{ $mid: MarshalledId.CommentThread, commentControlHandle, commentThreadHandle }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarshalledCommentThread {
    pub comment_control_handle: u32,
    pub comment_thread_handle: u32,
}

impl MarshalledCommentThread {
    /// Lê os dois handles de um objeto (o `$mid` é conferido por quem chama, porque as formas aninhadas em `thread`
    /// do argumento não o têm).
    pub fn from_dynamic(value: &Dynamic) -> Option<MarshalledCommentThread> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        Some(MarshalledCommentThread {
            comment_control_handle: handle_of(&value.get_str("commentControlHandle"))?,
            comment_thread_handle: handle_of(&value.get_str("commentThreadHandle"))?,
        })
    }

    /// O argumento de comando com o `$mid` da thread.
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("$mid", Dynamic::from(MarshalledId::CommentThread as i32)),
            ("commentControlHandle", Dynamic::from(f64::from(self.comment_control_handle))),
            ("commentThreadHandle", Dynamic::from(f64::from(self.comment_thread_handle))),
        ])
    }
}
