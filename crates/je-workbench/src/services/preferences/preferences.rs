//! Porte parcial de `src/vs/workbench/services/preferences/common/preferences.ts`: os tipos das entradas do editor de
//! atalhos (`KeybindingMatch`, `KeybindingMatches`, `IKeybindingItemEntry`, `IKeybindingItem`), que o
//! `KeybindingsEditorModel` produz. O resto do arquivo (`ISetting`, `IPreferencesService`, `IKeybindingsEditorPane`...)
//! entra com o porte das telas de preferências.
//!
//! Divergências:
//! - **`KeybindingMatch`** guarda `bool` em vez de `boolean | undefined`: o original só grava a chave quando é `true`
//!   (`{ metaKey: true }`), e o objeto `{}` (nenhum casamento) é o valor com tudo falso, que é o `Default`.
//! - **`IKeybindingItem.keybinding`** é `Option`: o tipo do original diz `ResolvedKeybinding`, mas um comando sem atalho
//!   guarda `undefined` ali (o editor de atalhos lista comandos sem atalho). `commandDefaultLabel` é `Option<String>`,
//!   porque o original guarda `null` quando o idioma é o padrão.
//! - **`source: string | IExtensionDescription`** é o enum [`KeybindingSource`]. O `IExtensionDescription` é o
//!   `ExtensionDescription` do `je-extensions`, compartilhado por `Rc` (o descritor carrega o manifesto inteiro, e a
//!   mesma extensão aparece em todos os atalhos dela).
//! - **`IKeybindingItemEntry.keybindingItem`** é um `Rc<IKeybindingItem>`: o original compartilha o objeto entre a lista
//!   do modelo e as entradas devolvidas por `fetch`, e a identidade é parte do contrato.
//! - `IMatch` é o [`Match`] do `je_base::filters`.

use std::rc::Rc;

use je_base::filters::Match;
use je_base::keybindings::ResolvedKeybinding;
use je_extensions::scanner::ExtensionDescription;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;

/// `KeybindingMatch`: quais partes de um acorde casaram com a busca.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeybindingMatch {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub key_code: bool,
}

/// `KeybindingMatches`: o casamento do primeiro acorde e do segundo (o acorde em sequência).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeybindingMatches {
    pub first_part: KeybindingMatch,
    pub chord_part: KeybindingMatch,
}

/// `string | IExtensionDescription`: a origem de um atalho. O texto é `System`, `User` ou `Extension` (localizados);
/// a extensão é o descritor dela.
#[derive(Clone, Debug)]
pub enum KeybindingSource {
    Label(String),
    Extension(Rc<ExtensionDescription>),
}

/// `IKeybindingItem`.
#[derive(Clone)]
pub struct IKeybindingItem {
    pub keybinding: Option<Rc<dyn ResolvedKeybinding>>,
    pub keybinding_item: Rc<ResolvedKeybindingItem>,
    pub command_label: String,
    pub command_default_label: Option<String>,
    pub command: String,
    pub source: KeybindingSource,
    pub when: String,
}

/// `IKeybindingItemEntry`: uma linha do editor de atalhos, com os trechos que casaram com a busca.
#[derive(Clone)]
pub struct IKeybindingItemEntry {
    pub id: String,
    pub template_id: String,
    pub keybinding_item: Rc<IKeybindingItem>,
    pub command_id_matches: Option<Vec<Match>>,
    pub command_label_matches: Option<Vec<Match>>,
    pub command_default_label_matches: Option<Vec<Match>>,
    pub source_matches: Option<Vec<Match>>,
    pub extension_id_matches: Option<Vec<Match>>,
    pub extension_label_matches: Option<Vec<Match>>,
    pub when_matches: Option<Vec<Match>>,
    pub keybinding_matches: Option<KeybindingMatches>,
}

impl IKeybindingItemEntry {
    /// Uma entrada sem nenhum trecho casado (os campos opcionais do original ausentes).
    pub fn new(id: String, template_id: &str, keybinding_item: Rc<IKeybindingItem>) -> Self {
        Self {
            id,
            template_id: template_id.to_string(),
            keybinding_item,
            command_id_matches: None,
            command_label_matches: None,
            command_default_label_matches: None,
            source_matches: None,
            extension_id_matches: None,
            extension_label_matches: None,
            when_matches: None,
            keybinding_matches: None,
        }
    }
}
