//! Porte de `src/vs/workbench/contrib/accessibility/browser/accessibleViewKeybindingResolver.ts`.
//!
//! Notas de tradução:
//! - **`value.matchAll(/(\<keybinding:(?<commandId>[^\<]*)\>)/gm)`** é [`keybinding_matches`]: depois de `<keybinding:`, o
//!   `[^<]*` guloso engole tudo até o próximo `<` (ou o fim) e recua até o último `>` desse trecho, que fecha a marcação.
//! - **`value.replace(match[0], kbLabel)`** troca cada marcação, em ordem, pelo rótulo: como o laço percorre os resultados do
//!   texto original, a troca da primeira ocorrência de cada um equivale a montar o texto numa passada só.
//! - **`MarkdownString`** (`content.value`, `isTrusted`) é o texto: a visão acessível só lê o `value`.
//! - **`IPickerQuickAccessItem`** (`{ label, id }`) é o [`IQuickPickItem`] com `base.label` e `base.id`.

use je_base::JsString;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::quickinput::quick_input::IQuickPickItem;

const MARKER_PREFIX: &str = "<keybinding:";

/// O retorno de `resolveContentAndKeybindingItems`.
pub struct ResolvedContentAndKeybindingItems {
    /// `content.value`.
    pub content: String,
    pub configure_keybinding_items: Option<Vec<IQuickPickItem>>,
    pub configured_keybinding_items: Option<Vec<IQuickPickItem>>,
}

/// Uma marcação `<keybinding:id>` achada: os índices em bytes do começo e do fim (exclusivo) e o `commandId`.
struct KeybindingMatch<'a> {
    start: usize,
    end: usize,
    command_id: &'a str,
}

/// Os resultados de `matchAll` sobre o texto.
fn keybinding_matches(value: &str) -> Vec<KeybindingMatch<'_>> {
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(relative) = value[from..].find(MARKER_PREFIX) {
        let start = from + relative;
        let id_start = start + MARKER_PREFIX.len();
        let run_end = value[id_start..].find('<').map_or(value.len(), |offset| id_start + offset);
        match value[id_start..run_end].rfind('>') {
            Some(last_close) => {
                let id_end = id_start + last_close;
                matches.push(KeybindingMatch { start, end: id_end + 1, command_id: &value[id_start..id_end] });
                from = id_end + 1;
            }
            None => from = id_start,
        }
    }
    matches
}

/// `IPickerQuickAccessItem` (`{ label: commandId, id: commandId }`).
fn command_item(command_id: &str) -> IQuickPickItem {
    let mut item = IQuickPickItem::default();
    item.base.label = JsString::from(command_id);
    item.base.id = Some(command_id.to_owned());
    item
}

/// `resolveContentAndKeybindingItems(keybindingService, value)`.
pub fn resolve_content_and_keybinding_items(keybinding_service: &dyn IKeybindingService, value: Option<&str>) -> Option<ResolvedContentAndKeybindingItems> {
    let value = value.filter(|value| !value.is_empty())?;
    let mut configure_keybinding_items = Vec::new();
    let mut configured_keybinding_items = Vec::new();
    let mut content = String::with_capacity(value.len());
    let mut copied_up_to = 0;
    for found in keybinding_matches(value) {
        if found.command_id.is_empty() {
            continue;
        }
        let keybinding = keybinding_service.lookup_keybinding(found.command_id, None, false).and_then(|keybinding| keybinding.get_aria_label());
        let label = match keybinding.filter(|label| !label.is_empty()) {
            None => {
                configure_keybinding_items.push(command_item(found.command_id));
                " (unassigned keybinding)".to_owned()
            }
            Some(keybinding) => {
                configured_keybinding_items.push(command_item(found.command_id));
                format!(" ({keybinding})")
            }
        };
        content.push_str(&value[copied_up_to..found.start]);
        content.push_str(&label);
        copied_up_to = found.end;
    }
    content.push_str(&value[copied_up_to..]);
    Some(ResolvedContentAndKeybindingItems {
        content,
        configure_keybinding_items: (!configure_keybinding_items.is_empty()).then_some(configure_keybinding_items),
        configured_keybinding_items: (!configured_keybinding_items.is_empty()).then_some(configured_keybinding_items),
    })
}
