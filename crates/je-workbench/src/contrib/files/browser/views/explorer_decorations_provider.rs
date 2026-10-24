//! Porte de `provideDecorations` de `src/vs/workbench/contrib/files/browser/views/explorerDecorationsProvider.ts`
//! (o `ExplorerDecorationsProvider`, que depende do serviço de decorações e do emissor de erro de raiz do viewer de DOM,
//! não entra).

use je_base::error_message::to_error_message_of;
use je_base::nls;

use crate::contrib::files::explorer_model::ExplorerItem;
use crate::services::decorations::decorations::{DecorationData, DecorationLetter};

/// `listInvalidItemForeground`.
pub const LIST_INVALID_ITEM_FOREGROUND: &str = "list.invalidItemForeground";

/// `listDeemphasizedForeground`.
pub const LIST_DEEMPHASIZED_FOREGROUND: &str = "list.deemphasizedForeground";

/// `provideDecorations(fileStat)`.
pub fn provide_decorations(file_stat: &ExplorerItem) -> Option<DecorationData> {
    if let (true, Some(error)) = (file_stat.is_root(), file_stat.error()) {
        return Some(DecorationData {
            tooltip: Some(nls::localize("canNotResolve", "Unable to resolve workspace folder ({0})", &[&to_error_message_of(&error, false)])),
            letter: Some(DecorationLetter::Text("!".to_string())),
            color: Some(LIST_INVALID_ITEM_FOREGROUND.to_string()),
            ..DecorationData::default()
        });
    }
    if file_stat.is_symbolic_link() {
        return Some(DecorationData {
            tooltip: Some(nls::localize("symbolicLlink", "Symbolic Link", &[])),
            letter: Some(DecorationLetter::Text("\u{2937}".to_string())),
            ..DecorationData::default()
        });
    }
    if file_stat.is_unknown() {
        return Some(DecorationData {
            tooltip: Some(nls::localize("unknown", "Unknown File Type", &[])),
            letter: Some(DecorationLetter::Text("?".to_string())),
            ..DecorationData::default()
        });
    }
    if file_stat.is_excluded() {
        return Some(DecorationData { color: Some(LIST_DEEMPHASIZED_FOREGROUND.to_string()), ..DecorationData::default() });
    }

    None
}
