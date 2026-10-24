//! Porte de `src/vs/editor/browser/controller/editContext` do VS Code: o estado da textarea de entrada do
//! editor (`textArea/textAreaEditContextState.ts`), os dados da área de transferência (`clipboardUtils.ts`) e as
//! utilidades do conteúdo para leitores de tela (`screenReaderUtils.ts`).

pub mod clipboard_utils;
pub mod screen_reader_utils;
pub mod text_area_state;
