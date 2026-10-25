//! Porte de `src/vs/workbench/contrib/comments/browser/commentsFilterOptions.ts`: as opções do filtro do painel de comentários
//! (texto com negação por `!` e os estados resolvido e não resolvido).
//!
//! Divergências:
//! - **`_filter` e `_messageFilter`** são as constantes associadas [`FilterOptions::FILTER`] e [`FilterOptions::MESSAGE_FILTER`]. O
//!   original as declara de novo com os mesmos valores (`matchesFuzzy2` e `matchesFuzzy`) que o filtro do painel de problemas;
//!   aqui apontam para as dele, e `textFilter` é o mesmo [`TextFilter`].
//! - **`filter`** guarda o texto como chegou, sem o `trim` (o `readonly filter` do construtor é a propriedade de parâmetro, atribuída
//!   antes do `filter = filter.trim()`).
//! - **`trim` e `ltrim`** são os do JavaScript sobre unidades UTF-16 (`js_string::trim` e `strings::ltrim`).

use je_base::filters::Match;
use je_base::{js_string, strings};

use crate::contrib::markers::markers_filter_options::{FilterOptions as MarkersFilterOptions, TextFilter};

/// `FilterOptions`.
#[derive(Clone, Debug)]
pub struct FilterOptions {
    pub filter: String,
    pub show_resolved: bool,
    pub show_unresolved: bool,
    pub text_filter: TextFilter,
}

impl FilterOptions {
    /// `static _filter: IFilter = matchesFuzzy2`.
    pub const FILTER: fn(&[u16], &[u16]) -> Option<Vec<Match>> = MarkersFilterOptions::FILTER;
    /// `static _messageFilter: IFilter = matchesFuzzy`.
    pub const MESSAGE_FILTER: fn(&[u16], &[u16]) -> Option<Vec<Match>> = MarkersFilterOptions::MESSAGE_FILTER;

    pub fn new(filter: &str, show_resolved: bool, show_unresolved: bool) -> Self {
        let units: Vec<u16> = filter.encode_utf16().collect();
        let trimmed = js_string::trim(&units);

        let negate = trimmed.first() == Some(&u16::from(b'!'));
        let text = js_string::trim(if negate { strings::ltrim(trimmed, &[u16::from(b'!')]) } else { trimmed });

        Self {
            filter: filter.to_owned(),
            show_resolved,
            show_unresolved,
            text_filter: TextFilter { text: String::from_utf16_lossy(text), negate },
        }
    }
}
