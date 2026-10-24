//! Porte parcial de `src/vs/workbench/contrib/snippets/browser/commands/configureSnippets.ts`: `ISnippetPick` e
//! `promoteActiveLanguage` (a reordenação dos itens do seletor de snippets que põe a linguagem do editor ativo na frente).
//!
//! Fora do porte: o restante do arquivo (a ação `ConfigureSnippets`, `computePicks`, `createSnippetFile`, `createLanguageSnippetFile`
//! e `openSnippetSource`) depende do `IQuickInputService`, do `ITextFileService` e do abridor de editores, que ainda não têm
//! contrato completo no porte do `contrib/snippets`.
//!
//! Notas de tradução:
//! - **`ISnippetPick extends IQuickPickItem`** é composição: o campo `base` guarda o item do seletor.
//! - **`basename(pick.filepath)`** é o `basename` do `extUri`.

use je_base::js_string::JsString;
use je_base::nls::localize;
use je_base::uri::URI;
use je_platform::quickinput::quick_input::{IQuickPickItem, QuickPickTooltip};
use je_platform::resources::ext_uri;

/// `ISnippetPick`.
#[derive(Clone, Debug)]
pub struct ISnippetPick {
    pub base: IQuickPickItem,
    pub filepath: URI,
    pub hint: Option<bool>,
}

/// Move the active editor's language to the front of each group and mark it as detected.
pub fn promote_active_language(existing: &mut Vec<ISnippetPick>, future: &mut Vec<ISnippetPick>, active_language_id: Option<&str>) {
    let Some(active_language_id) = active_language_id.filter(|id| !id.is_empty()) else {
        return;
    };
    let detected_hint = localize("detected", "This file type was detected from the current file.", &[]);
    let ext = ext_uri();
    let json_name = format!("{active_language_id}.json");
    let existing_index = existing.iter().position(|pick| ext.basename(&pick.filepath, None) == json_name);
    promote(existing, existing_index, &detected_hint);
    let future_index = future.iter().position(|pick| pick.base.base.label == active_language_id);
    promote(future, future_index, &detected_hint);
}

fn promote(picks: &mut Vec<ISnippetPick>, index: Option<usize>, hint: &str) {
    let Some(index) = index else {
        return;
    };
    let pick = &mut picks[index];
    pick.base.tooltip = Some(QuickPickTooltip::Text(hint.to_string()));
    let description = match pick.base.base.description.as_ref().filter(|description| !description.is_empty()) {
        Some(description) => {
            let mut text = description.clone();
            text.push_units(&" *".encode_utf16().collect::<Vec<u16>>());
            text
        }
        None => JsString::from("*"),
    };
    pick.base.base.description = Some(description);
    if index > 0 {
        let moved = picks.remove(index);
        picks.insert(0, moved);
    }
}
