//! Contrato reduzido do `SuggestWidget` de `src/vs/editor/contrib/suggest/browser/suggestWidget.ts`: o que
//! `suggestAlternatives.ts` e `suggestCommitCharacters.ts` leem do widget de sugestões, que ainda não tem porte.
//!
//! Representação:
//! - `ISelectedSuggestion` (`{ item, index, model }`) é a struct [`ISelectedSuggestion`]: o `item` é o manipulador
//!   compartilhado [`CompletionItem`] e o `model` o `Rc` do [`CompletionModel`] que o original referencia.
//! - `SuggestWidget` é o trait [`ISuggestWidget`], só com os cinco membros que os dois consumidores usam
//!   (`onDidShow`, `onDidFocus`, `onDidHide`, `getFocusedItem` e `isFrozen`). O `Event<this>` de `onDidShow` e
//!   `onDidHide` é `Event<()>`: nenhum consumidor lê o argumento.
//!
//! Divergências:
//! - O widget completo (lista, detalhes, estados `Hidden`, `Loading`, `Empty`, `Open`, `Frozen`, `Details`) entra com o
//!   porte do `suggestWidget.ts`, que implementará este trait; nenhum comportamento é inventado aqui.

use std::rc::Rc;

use je_base::event::Event;

use crate::suggest::completion_model::CompletionModel;
use crate::suggest::suggest::CompletionItem;

/// `ISelectedSuggestion`.
#[derive(Clone)]
pub struct ISelectedSuggestion {
    pub item: CompletionItem,
    pub index: usize,
    pub model: Rc<CompletionModel>,
}

/// `SuggestWidget`, recorte dos membros lidos por `SuggestAlternatives` e `CommitCharacterController`.
pub trait ISuggestWidget {
    /// `onDidShow`.
    fn on_did_show(&self) -> Event<()>;

    /// `onDidFocus`.
    fn on_did_focus(&self) -> Event<ISelectedSuggestion>;

    /// `onDidHide`.
    fn on_did_hide(&self) -> Event<()>;

    /// `getFocusedItem()`.
    fn get_focused_item(&self) -> Option<ISelectedSuggestion>;

    /// `isFrozen()`.
    fn is_frozen(&self) -> bool;
}
