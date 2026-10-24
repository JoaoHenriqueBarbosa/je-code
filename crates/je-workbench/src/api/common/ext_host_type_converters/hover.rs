//! `extHostApiCommands.ts` (`vscode.executeHoverProvider`): a ponte do `languages.Hover` com o valor que o RPC entrega, o
//! objeto `{ contents, range?, canIncreaseVerbosity?, canDecreaseVerbosity?, id? }`.
//!
//! Divergências:
//! - O `typeConverters.Hover.to` que o comando aplica ao resultado é o de
//!   [`ext_host_language_features_converters`](crate::api::common::ext_host_language_features_converters), que lê o
//!   [`HoverDto`]: [`Hover::from_dto`] monta esse DTO a partir do valor, e [`Hover::to_dto`] faz o caminho de ida.
//! - `isValid` de `getHover.ts` só deixa passar hover com `range`, então o valor do comando sempre o tem; o `id` do
//!   `HoverWithId` cruza quando existe.

use je_base::dynamic::Dynamic;
use je_editor::language_features_service::Hover as EditorHover;

use super::geometry::Range;
use super::markup::MarkdownString;
use crate::api::common::ext_host_protocol::HoverDto;

/// O `languages.Hover` como valor do RPC.
pub struct Hover;

impl Hover {
    /// O `languages.Hover` como o DTO do RPC (com o `id` do `HoverWithId` quando existe).
    pub fn to_dto(hover: &EditorHover) -> Dynamic {
        let entries = [
            ("contents", Some(Dynamic::array(hover.contents.iter().map(MarkdownString::to_dto)))),
            ("range", hover.range.as_ref().map(Range::to_dto)),
            ("canIncreaseVerbosity", hover.can_increase_verbosity.map(Dynamic::Bool)),
            ("canDecreaseVerbosity", hover.can_decrease_verbosity.map(Dynamic::Bool)),
            ("id", hover.id.map(|id| Dynamic::Number(f64::from(id)))),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    }

    /// A leitura do DTO de [`Hover::to_dto`]; `None` se o `contents` não é uma lista de `IMarkdownString`.
    pub fn from_dto(value: &Dynamic) -> Option<HoverDto> {
        let Dynamic::Array(contents) = value.get_str("contents") else {
            return None;
        };
        let contents = contents.to_vec().iter().map(MarkdownString::from_dto).collect::<Option<Vec<_>>>()?;
        let flag = |key: &str| match value.get_str(key) {
            Dynamic::Bool(flag) => Some(flag),
            _ => None,
        };
        Some(HoverDto {
            contents,
            range: Range::from_dto(&value.get_str("range")),
            can_increase_verbosity: flag("canIncreaseVerbosity"),
            can_decrease_verbosity: flag("canDecreaseVerbosity"),
        })
    }
}
