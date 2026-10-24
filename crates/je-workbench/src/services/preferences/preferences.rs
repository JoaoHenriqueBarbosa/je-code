//! Porte parcial de `src/vs/workbench/services/preferences/common/preferences.ts`: os tipos das entradas do editor de
//! atalhos (`KeybindingMatch`, `KeybindingMatches`, `IKeybindingItemEntry`, `IKeybindingItem`), que o
//! `KeybindingsEditorModel` produz, e os tipos das configurações que o editor de configurações mostra e busca
//! (`SettingValueType`, `ISetting`, `IExtensionSetting`, `ISettingsGroup`, `ISettingsSection`, `ISearchResult`,
//! `SettingMatchType`, `SettingKeyMatchTypes`, `ISettingMatch`) e o recorte do `IPreferencesService` que o
//! `ExternalUriOpenerService` usa (`openUserSettings`, `IOpenSettingsOptions`). O resto do arquivo (o `IPreferencesService`
//! completo, `IKeybindingsEditorPane`, `ISettingsEditorModel`...) entra com o porte das telas de preferências.
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
//! - **`SettingValueType`** (um `enum` de strings) é o enum [`SettingValueType`] com [`SettingValueType::as_str`] para o
//!   texto do original.
//! - **`ISetting`** é a struct [`ISetting`], com `Default` (o equivalente do `mock<ISetting>()` dos testes, em que só
//!   os campos usados são preenchidos): cada campo opcional é `Option`, os esquemas JSON soltos (`objectProperties`,
//!   `objectPatternProperties`, `objectAdditionalProperties`, `propertyNames`) são [`Dynamic`], como nas validações, e
//!   `type: string | string[]` é o [`SettingSchemaType`]. `IRange` é o [`Range`] do `je-editor` (o mesmo tipo). O
//!   `internalOrder`, que o modelo de resultados da busca grava no objeto que várias listas compartilham, é um
//!   [`Cell`], e as configurações compartilhadas entre listas e elementos da árvore são `Rc<ISetting>`.
//! - **`IExtensionSetting extends ISetting`**: o original só acrescenta `extensionName` e `extensionPublisher`, e o
//!   modelo de resultados converte um `ISetting` em `IExtensionSetting` com um cast. Os dois campos vivem no
//!   [`ISetting`] e [`IExtensionSetting`] é o mesmo tipo.
//! - **`ISettingMatch.matches`** é `Option<Vec<Range>>` (o `IRange[] | null` do original) e `setting` é `Rc<ISetting>`.
//!   A igualdade de dois [`ISettingMatch`] compara o `setting` por identidade (o `deepStrictEqual` do original, nos
//!   testes, compara o mesmo objeto), e os demais campos por valor.
//! - **`SettingMatchType`** (um `enum` de bits) é o newtype [`SettingMatchType`] com as constantes do original e os
//!   operadores `|` e `&`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::filters::Match;
use je_base::keybindings::ResolvedKeybinding;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::range::Range;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::service_id;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::registry::{ConfigurationDefaultValueSource, ConfigurationScope, ExtensionInfo};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::opener::editor_types::EditorOptions;

use super::preferences_models::NULL_RANGE;

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

/// `SettingValueType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingValueType {
    Null,
    Enum,
    String,
    MultilineString,
    Integer,
    Number,
    Boolean,
    Array,
    Exclude,
    Include,
    Complex,
    NullableInteger,
    NullableNumber,
    Object,
    BooleanObject,
    LanguageTag,
    ExtensionToggle,
    ComplexObject,
}

impl SettingValueType {
    /// O valor de texto do membro do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Enum => "enum",
            Self::String => "string",
            Self::MultilineString => "multiline-string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Exclude => "exclude",
            Self::Include => "include",
            Self::Complex => "complex",
            Self::NullableInteger => "nullable-integer",
            Self::NullableNumber => "nullable-number",
            Self::Object => "object",
            Self::BooleanObject => "boolean-object",
            Self::LanguageTag => "language-tag",
            Self::ExtensionToggle => "extension-toggle",
            Self::ComplexObject => "complex-object",
        }
    }
}

/// `ISettingsGroup`.
#[derive(Clone)]
pub struct ISettingsGroup {
    pub id: String,
    pub range: Range,
    pub title: String,
    pub title_range: Range,
    pub sections: Vec<ISettingsSection>,
    pub order: Option<f64>,
    pub extension_info: Option<ExtensionInfo>,
}

/// `ISettingsSection`.
#[derive(Clone)]
pub struct ISettingsSection {
    pub title: Option<String>,
    pub settings: Vec<Rc<ISetting>>,
}

/// `string | string[]`: o `type` de uma configuração.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingSchemaType {
    Single(String),
    List(Vec<String>),
}

impl SettingSchemaType {
    /// `type === name` (uma lista nunca é igual a um texto).
    pub fn is(&self, name: &str) -> bool {
        matches!(self, Self::Single(single) if single == name)
    }

    /// `Array.isArray(type)`.
    pub fn is_array(&self) -> bool {
        matches!(self, Self::List(_))
    }

    /// `[].concat(type)`: os tipos como lista (o texto único é uma lista de um elemento).
    pub fn as_list(&self) -> Vec<&str> {
        match self {
            Self::Single(single) => vec![single.as_str()],
            Self::List(list) => list.iter().map(String::as_str).collect(),
        }
    }
}

/// `validator?: (value: any) => string | null`.
pub type SettingValidator = Rc<dyn Fn(&Dynamic) -> Option<String>>;

/// `ISetting`.
#[derive(Clone)]
pub struct ISetting {
    pub range: Range,
    pub key: String,
    pub key_range: Range,
    pub value: Dynamic,
    pub value_range: Range,
    pub description: Vec<String>,
    pub keywords: Option<Vec<String>>,
    pub description_is_markdown: Option<bool>,
    pub description_ranges: Vec<Range>,
    pub overrides: Option<Vec<ISetting>>,
    pub override_of: Option<Box<ISetting>>,
    pub deprecation_message: Option<String>,
    pub deprecation_message_is_markdown: Option<bool>,
    /// `'warning' | 'info'`.
    pub deprecation_message_severity: Option<String>,
    pub deprecation_message_show_in_settings: Option<bool>,

    pub scope: Option<ConfigurationScope>,
    pub r#type: Option<SettingSchemaType>,
    pub order: Option<f64>,
    pub array_item_type: Option<String>,
    /// `IJSONSchemaMap`.
    pub object_properties: Option<Dynamic>,
    /// `IJSONSchemaMap`.
    pub object_pattern_properties: Option<Dynamic>,
    /// `boolean | IJSONSchema`.
    pub object_additional_properties: Option<Dynamic>,
    /// `IJSONSchema`.
    pub property_names: Option<Dynamic>,
    pub r#enum: Option<Vec<String>>,
    pub enum_descriptions: Option<Vec<String>>,
    pub enum_descriptions_are_markdown: Option<bool>,
    pub unique_items: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub disallow_sync_ignore: Option<bool>,
    pub restricted: Option<bool>,
    pub extension_info: Option<ExtensionInfo>,
    pub validator: Option<SettingValidator>,
    pub enum_item_labels: Option<Vec<String>>,
    /// `EditPresentationTypes` (`multilineText` ou `singlelineText`).
    pub edit_presentation: Option<String>,
    pub non_language_specific_default_value_source: Option<ConfigurationDefaultValueSource>,
    pub is_language_tag_setting: Option<bool>,
    pub category_label: Option<String>,

    // Internal properties
    pub all_keys_are_boolean: Option<bool>,
    pub display_extension_id: Option<String>,
    pub title: Option<String>,
    pub extension_group_title: Option<String>,
    pub internal_order: Cell<Option<f64>>,

    // `IExtensionSetting`
    pub extension_name: Option<String>,
    pub extension_publisher: Option<String>,
}

/// `IExtensionSetting`: ver a divergência no topo do arquivo.
pub type IExtensionSetting = ISetting;

impl Default for ISetting {
    /// Uma configuração com os campos obrigatórios vazios (`nullRange`, `key` vazia) e nenhum opcional.
    fn default() -> Self {
        Self {
            range: NULL_RANGE,
            key: String::new(),
            key_range: NULL_RANGE,
            value: Dynamic::Undefined,
            value_range: NULL_RANGE,
            description: Vec::new(),
            keywords: None,
            description_is_markdown: None,
            description_ranges: Vec::new(),
            overrides: None,
            override_of: None,
            deprecation_message: None,
            deprecation_message_is_markdown: None,
            deprecation_message_severity: None,
            deprecation_message_show_in_settings: None,
            scope: None,
            r#type: None,
            order: None,
            array_item_type: None,
            object_properties: None,
            object_pattern_properties: None,
            object_additional_properties: None,
            property_names: None,
            r#enum: None,
            enum_descriptions: None,
            enum_descriptions_are_markdown: None,
            unique_items: None,
            tags: None,
            disallow_sync_ignore: None,
            restricted: None,
            extension_info: None,
            validator: None,
            enum_item_labels: None,
            edit_presentation: None,
            non_language_specific_default_value_source: None,
            is_language_tag_setting: None,
            category_label: None,
            all_keys_are_boolean: None,
            display_extension_id: None,
            title: None,
            extension_group_title: None,
            internal_order: Cell::new(None),
            extension_name: None,
            extension_publisher: None,
        }
    }
}

/// `ISearchResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISearchResult {
    pub filter_matches: Vec<ISettingMatch>,
    pub exact_match: bool,
}

/// `SettingMatchType`: as maneiras de uma configuração casar com a busca, em ordem crescente de relevância. É um
/// conjunto de bits no original (`1 << n`), por isso o newtype com `|` e `&`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SettingMatchType(pub u32);

#[allow(non_upper_case_globals)] // os membros do `enum` do original
impl SettingMatchType {
    pub const None: Self = Self(0);
    pub const LanguageTagSettingMatch: Self = Self(1 << 0);
    pub const RemoteMatch: Self = Self(1 << 1);
    pub const NonContiguousQueryInSettingId: Self = Self(1 << 2);
    pub const DescriptionOrValueMatch: Self = Self(1 << 3);
    pub const NonContiguousWordsInSettingsLabel: Self = Self(1 << 4);
    pub const ContiguousWordsInSettingsLabel: Self = Self(1 << 5);
    pub const ContiguousQueryInSettingId: Self = Self(1 << 6);
    pub const AllWordsInSettingsLabel: Self = Self(1 << 7);
    pub const ExactMatch: Self = Self(1 << 8);

    /// `(this & other) !== 0`: o teste de verdade de `a.matchType & X` do original.
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for SettingMatchType {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for SettingMatchType {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for SettingMatchType {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// `SettingKeyMatchTypes`.
pub const SETTING_KEY_MATCH_TYPES: SettingMatchType = SettingMatchType(
    SettingMatchType::AllWordsInSettingsLabel.0
        | SettingMatchType::ContiguousWordsInSettingsLabel.0
        | SettingMatchType::NonContiguousWordsInSettingsLabel.0
        | SettingMatchType::NonContiguousQueryInSettingId.0
        | SettingMatchType::ContiguousQueryInSettingId.0,
);

/// `ISettingMatch`.
#[derive(Clone)]
pub struct ISettingMatch {
    pub setting: Rc<ISetting>,
    pub matches: Option<Vec<Range>>,
    pub match_type: SettingMatchType,
    pub key_match_score: f64,
    pub score: f64,
    pub provider_name: Option<String>,
}

impl std::fmt::Debug for ISettingMatch {
    /// Mostra a chave da configuração (o `ISetting` carrega funções e esquemas, e não tem `Debug`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ISettingMatch")
            .field("setting", &self.setting.key)
            .field("matches", &self.matches)
            .field("match_type", &self.match_type)
            .field("key_match_score", &self.key_match_score)
            .field("score", &self.score)
            .field("provider_name", &self.provider_name)
            .finish()
    }
}

impl PartialEq for ISettingMatch {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.setting, &other.setting)
            && self.matches == other.matches
            && self.match_type == other.match_type
            && self.key_match_score == other.key_match_score
            && self.score == other.score
            && self.provider_name == other.provider_name
    }
}

impl ISettingMatch {
    /// Um casamento construído como os literais `{ setting, matches, matchType, keyMatchScore, score }`.
    pub fn new(setting: Rc<ISetting>, matches: Option<Vec<Range>>, match_type: SettingMatchType, key_match_score: f64, score: f64) -> Self {
        Self { setting, matches, match_type, key_match_score, score, provider_name: None }
    }
}

// ---- `IPreferencesService` (reduzido) ---------------------------------------------------------------------------------
//
// O `IPreferencesService` completo (as aberturas de configurações por escopo, os editores de atalhos, os modelos) entra
// com o porte das telas de preferências. Este recorte é o que o `ExternalUriOpenerService` lê dele: `openUserSettings`.
// `Promise<IEditorPane | undefined>` é `Promise<()>`, porque o `IEditorPane` não foi portado e nenhum chamador usa o painel.

/// O `revealSetting` de `ISettingsEditorOptions`: `{ key: string; edit?: boolean }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealSetting {
    pub key: String,
    pub edit: Option<bool>,
}

/// `IOpenSettingsOptions` (`extends ISettingsEditorOptions extends IEditorOptions`). Os campos de `IEditorOptions` são o
/// [`EditorOptions`] em `editor_options`.
#[derive(Clone, Default)]
pub struct IOpenSettingsOptions {
    pub editor_options: EditorOptions,
    pub target: Option<ConfigurationTarget>,
    pub folder_uri: Option<URI>,
    pub query: Option<String>,
    /// Only works when opening the json settings file. Use `query` for settings editor.
    pub reveal_setting: Option<RevealSetting>,
    pub focus_search: Option<bool>,
    pub json_editor: Option<bool>,
    pub open_to_side: Option<bool>,
    pub group_id: Option<f64>,
}

/// `IPreferencesService`, reduzido ao `openUserSettings` (ver a nota acima).
pub trait IPreferencesService {
    /// `openUserSettings(options?)`.
    fn open_user_settings(&self, options: Option<&IOpenSettingsOptions>) -> Promise<()>;
}

service_id!(pub I_PREFERENCES_SERVICE: dyn IPreferencesService = "preferencesService");
