//! Porte de `src/vs/editor/contrib/comment/browser`: os comandos de comentar e descomentar
//! (`blockCommentCommand.ts` e `lineCommentCommand.ts`). `comment.ts`, com as ações do editor (Ctrl+/ e
//! Shift+Alt+A), é a camada de contribuição e fica de fora.
//!
//! Notas de tradução comuns aos dois comandos:
//! - Os textos de linha são UTF-16 ([`je_base::JsString`]) e os tokens de comentário da configuração de
//!   linguagem são `String`; os comandos convertem os tokens para `Vec<u16>` uma vez e trabalham em unidades
//!   UTF-16, como o original.

pub mod block_comment_command;
pub mod line_comment_command;

/// O índice do JavaScript (que pode ser negativo) como o `from` de `indexOf`/`lastIndexOf`, que o
/// JavaScript satura em zero.
pub(crate) fn from_index(index: i32) -> usize {
    index.max(0) as usize
}

/// O resultado de `indexOf`/`lastIndexOf` como o número do original (`-1` quando não acha).
pub(crate) fn js_index(found: Option<usize>) -> i32 {
    found.map_or(-1, |index| index as i32)
}
