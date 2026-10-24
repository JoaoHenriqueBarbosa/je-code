//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/preferencesSearch.ts`: o `SettingMatches`, que diz como
//! uma configuração casa com a busca (chave, descrição e valor) e com que relevância. O resto do arquivo
//! (`PreferencesSearchService`, `LocalSearchProvider`, os provedores de embeddings, TF-IDF, remoto e de IA) depende do
//! `ISettingsEditorModel`, do serviço de busca de IA e do injetor de instâncias, e entra com o porte do editor de
//! configurações.
//!
//! Divergências:
//! - **Texto é UTF-16 nos filtros.** Os filtros de `je_base::filters` recebem `&[u16]` e devolvem posições UTF-16, como o
//!   `IMatch` do original; as buscas convertem o `&str` na chamada, e as expressões regulares são as `RegExp` ECMAScript
//!   do `zyarr` (a de `_toAlphaNumeric` usa `\p{L}\p{N}` com a flag `u`).
//! - **Construtor.** O `SettingMatches` do original guarda `searchDescription` e `configurationService` só para o
//!   construtor, que calcula tudo; aqui [`SettingMatches::new`] recebe os dois por parâmetro e não os guarda.
//! - **`IRange`** é o [`Range`] do `je-editor`, escrito como literal (o `Range::new` reordena início e fim).
//! - **`Map<string, IRange[]>`** de palavras é um `IndexMap<String, Vec<Range>>`: o `set` de uma chave que já existe
//!   troca o valor sem mudar a posição, e `Array.from(map.values()).flat()` segue a ordem de inserção, como no `Map`.

use indexmap::IndexMap;
use je_base::JsString;
use je_base::arrays::distinct;
use je_base::dynamic::Dynamic;
use je_base::filters::{Match, matches_base_contiguous_sub_string, matches_contiguous_sub_string, matches_sub_string, matches_words};
use je_base::js_regexp::replace_all;
use je_editor::range::Range;
use je_platform::configuration::service::IConfigurationService;
use zyarr::RegExp;

use crate::contrib::preferences::common::preferences::join_first_two_groups;
use crate::services::preferences::preferences::{ISetting, SettingMatchType};
use crate::services::preferences::preferences_models::NULL_RANGE;

/// As expressões de `_keyToLabel` e `_toAlphaNumeric`, compiladas uma vez por thread.
struct SearchRegexes {
    separators: RegExp,
    lower_then_upper: RegExp,
    letters_then_digits: RegExp,
    digits_then_letters: RegExp,
    not_alphanumeric: RegExp,
}

thread_local! {
    static SEARCH_REGEXES: SearchRegexes = {
        let regex = |pattern: &str, flags: &str| RegExp::from_str(pattern, flags).expect("a regex do preferencesSearch é válida");
        SearchRegexes {
            separators: regex(r"[-._]", "g"),
            lower_then_upper: regex(r"([a-z]+)([A-Z])", "g"),
            letters_then_digits: regex(r"([A-Za-z]+)(\d+)", "g"),
            digits_then_letters: regex(r"(\d+)([A-Za-z]+)", "g"),
            not_alphanumeric: regex(r"[^\p{L}\p{N}]+", "gu"),
        }
    };
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `Array.from(map.values()).flat()`, ou `[]` quando o mapa está vazio.
fn flatten_ranges(words: &IndexMap<String, Vec<Range>>) -> Vec<Range> {
    words.values().flatten().copied().collect()
}

/// `SettingMatches`.
pub struct SettingMatches {
    pub matches: Vec<Range>,
    pub match_type: SettingMatchType,
    /// A match score for key matches to allow comparing key matches against each other.
    /// Otherwise, all key matches are treated the same, and sorting is done by ToC order.
    pub key_match_score: f64,
}

impl SettingMatches {
    pub fn new(search_string: &str, setting: &ISetting, search_description: bool, configuration_service: &dyn IConfigurationService) -> Self {
        let mut this = Self { matches: Vec::new(), match_type: SettingMatchType::None, key_match_score: 0.0 };
        let found = this.find_matches_in_setting(search_string, setting, search_description, configuration_service);
        this.matches = distinct(&found, |range| (range.start_line_number, range.start_column, range.end_line_number, range.end_column));
        this
    }

    fn find_matches_in_setting(&mut self, search_string: &str, setting: &ISetting, search_description: bool, configuration_service: &dyn IConfigurationService) -> Vec<Range> {
        self.do_find_matches_in_setting(search_string, setting, search_description, configuration_service)
    }

    fn key_to_label(setting_id: &str) -> String {
        SEARCH_REGEXES.with(|regexes| {
            let label = replace_all(&regexes.separators, &units(setting_id), |_| JsString::from(" "));
            let label = replace_all(&regexes.lower_then_upper, label.units(), join_first_two_groups);
            let label = replace_all(&regexes.letters_then_digits, label.units(), join_first_two_groups);
            let label = replace_all(&regexes.digits_then_letters, label.units(), join_first_two_groups);
            label.to_lower_case().to_string_lossy()
        })
    }

    fn to_alpha_numeric(s: &str) -> String {
        SEARCH_REGEXES.with(|regexes| replace_all(&regexes.not_alphanumeric, &units(s), |_| JsString::new()).to_string_lossy())
    }

    fn do_find_matches_in_setting(
        &mut self,
        search_string: &str,
        setting: &ISetting,
        search_description: bool,
        configuration_service: &dyn IConfigurationService,
    ) -> Vec<Range> {
        let mut description_matching_words: IndexMap<String, Vec<Range>> = IndexMap::new();
        let mut key_matching_words: IndexMap<String, Vec<Range>> = IndexMap::new();
        let mut value_matching_words: IndexMap<String, Vec<Range>> = IndexMap::new();

        // Key (ID) search
        // First, search by the setting's ID and label.
        let setting_key_as_words = units(&Self::key_to_label(&setting.key));
        let mut query_words: Vec<&str> = Vec::new();
        for word in search_string.split(' ') {
            if !query_words.contains(&word) {
                query_words.push(word);
            }
        }
        for word in &query_words {
            // Check if the key contains the word. Use contiguous search.
            let key_matches = matches_words(&units(word), &setting_key_as_words, true);
            if let Some(key_matches) = key_matches.filter(|matches| !matches.is_empty()) {
                key_matching_words.insert((*word).to_owned(), key_matches.iter().map(|m| Self::to_key_range(setting, m)).collect());
            }
        }
        if key_matching_words.len() == query_words.len() {
            // All words in the query matched with something in the setting key.
            // Matches "edit format on paste" to "editor.formatOnPaste".
            self.match_type |= SettingMatchType::AllWordsInSettingsLabel;
        } else if key_matching_words.len() >= 2 {
            // Matches "edit paste" to "editor.formatOnPaste".
            // The if statement reduces noise by preventing "editor formatonpast" from matching all editor settings.
            self.match_type |= SettingMatchType::ContiguousWordsInSettingsLabel;
            self.key_match_score = key_matching_words.len() as f64;
        }
        let search_string_alpha_numeric = units(&Self::to_alpha_numeric(search_string));
        let key_alpha_numeric = units(&Self::to_alpha_numeric(&setting.key));
        let key_id_matches = matches_contiguous_sub_string(&search_string_alpha_numeric, &key_alpha_numeric);
        if let Some(key_id_matches) = key_id_matches.filter(|matches| !matches.is_empty()) {
            // Matches "editorformatonp" to "editor.formatonpaste".
            key_matching_words.insert(setting.key.clone(), key_id_matches.iter().map(|m| Self::to_key_range(setting, m)).collect());
            self.match_type |= SettingMatchType::ContiguousQueryInSettingId;
        }

        // Fall back to non-contiguous key (ID) searches if nothing matched yet.
        if self.match_type == SettingMatchType::None {
            key_matching_words.clear();
            for word in &query_words {
                let key_matches = matches_words(&units(word), &setting_key_as_words, false);
                if let Some(key_matches) = key_matches.filter(|matches| !matches.is_empty()) {
                    key_matching_words.insert((*word).to_owned(), key_matches.iter().map(|m| Self::to_key_range(setting, m)).collect());
                }
            }
            if key_matching_words.len() >= 2 || (key_matching_words.len() == 1 && query_words.len() == 1) {
                // Matches "edforonpas" to "editor.formatOnPaste".
                // The if statement reduces noise by preventing "editor fomonpast" from matching all editor settings.
                self.match_type |= SettingMatchType::NonContiguousWordsInSettingsLabel;
                self.key_match_score = key_matching_words.len() as f64;
            } else {
                let key_id_matches = matches_sub_string(&search_string_alpha_numeric, &key_alpha_numeric);
                if let Some(key_id_matches) = key_id_matches.filter(|matches| !matches.is_empty()) {
                    // Matches "edfmonpas" to "editor.formatOnPaste".
                    key_matching_words.insert(setting.key.clone(), key_id_matches.iter().map(|m| Self::to_key_range(setting, m)).collect());
                    self.match_type |= SettingMatchType::NonContiguousQueryInSettingId;
                }
            }
        }

        // Check if the match was for a language tag group setting such as [markdown].
        // In such a case, move that setting to be last.
        if setting.overrides.as_ref().is_some_and(|overrides| !overrides.is_empty()) && self.match_type != SettingMatchType::None {
            self.match_type = SettingMatchType::LanguageTagSettingMatch;
            return flatten_ranges(&key_matching_words);
        }

        // Description search
        // Search the description if we found non-contiguous key matches at best.
        let has_contiguous_key_match_types = self.match_type >= SettingMatchType::ContiguousWordsInSettingsLabel;
        if search_description && !has_contiguous_key_match_types {
            // Search the description lines and any additional keywords.
            let searchable_lines: Vec<String> = match setting.keywords.as_ref().filter(|keywords| !keywords.is_empty()) {
                Some(keywords) => setting.description.iter().cloned().chain(std::iter::once(keywords.join(" "))).collect(),
                None => setting.description.clone(),
            };
            for word in &query_words {
                for (line_index, line) in searchable_lines.iter().enumerate() {
                    let description_matches = matches_base_contiguous_sub_string(&units(word), &units(line));
                    if let Some(description_matches) = description_matches.filter(|matches| !matches.is_empty()) {
                        description_matching_words.insert((*word).to_owned(), description_matches.iter().map(|m| Self::to_description_range(setting, m, line_index)).collect());
                    }
                }
            }
            if description_matching_words.len() == query_words.len() {
                self.match_type |= SettingMatchType::DescriptionOrValueMatch;
            } else {
                // Clear out the match for now. We want to require all words to match in the description.
                description_matching_words.clear();
            }
        }

        // Value search
        // Check if the value contains all the words.
        // Search the values if we found non-contiguous key matches at best.
        if !has_contiguous_key_match_types {
            match setting.r#enum.as_ref().filter(|options| !options.is_empty()) {
                Some(options) => {
                    // Search all string values of enums.
                    for option in options {
                        value_matching_words.clear();
                        for word in &query_words {
                            let value_matches = matches_contiguous_sub_string(&units(word), &units(option));
                            if let Some(value_matches) = value_matches.filter(|matches| !matches.is_empty()) {
                                value_matching_words.insert((*word).to_owned(), value_matches.iter().map(|m| Self::to_value_range(setting, m)).collect());
                            }
                        }
                        if value_matching_words.len() == query_words.len() {
                            self.match_type |= SettingMatchType::DescriptionOrValueMatch;
                            break;
                        }
                        // Clear out the match for now. We want to require all words to match in the value.
                        value_matching_words.clear();
                    }
                }
                None => {
                    // Search single string value.
                    if let Dynamic::String(setting_value) = configuration_service.get_value(Some(&setting.key), None) {
                        let setting_value = setting_value.to_string_lossy();
                        for word in &query_words {
                            let value_matches = matches_contiguous_sub_string(&units(word), &units(&setting_value));
                            if let Some(value_matches) = value_matches.filter(|matches| !matches.is_empty()) {
                                value_matching_words.insert((*word).to_owned(), value_matches.iter().map(|m| Self::to_value_range(setting, m)).collect());
                            }
                        }
                        if value_matching_words.len() == query_words.len() {
                            self.match_type |= SettingMatchType::DescriptionOrValueMatch;
                        } else {
                            // Clear out the match for now. We want to require all words to match in the value.
                            value_matching_words.clear();
                        }
                    }
                }
            }
        }

        let description_ranges = flatten_ranges(&description_matching_words);
        let key_ranges = flatten_ranges(&key_matching_words);
        let value_ranges = flatten_ranges(&value_matching_words);
        [description_ranges, key_ranges, value_ranges].concat()
    }

    fn to_key_range(setting: &ISetting, match_: &Match) -> Range {
        Range {
            start_line_number: setting.key_range.start_line_number,
            start_column: setting.key_range.start_column + match_.start as i32,
            end_line_number: setting.key_range.start_line_number,
            end_column: setting.key_range.start_column + match_.end as i32,
        }
    }

    fn to_description_range(setting: &ISetting, match_: &Match, line_index: usize) -> Range {
        let Some(description_range) = setting.description_ranges.get(line_index) else {
            // This case occurs with added settings such as the
            // manage extension setting.
            return NULL_RANGE;
        };
        Range {
            start_line_number: description_range.start_line_number,
            start_column: description_range.start_column + match_.start as i32,
            end_line_number: description_range.end_line_number,
            end_column: description_range.start_column + match_.end as i32,
        }
    }

    fn to_value_range(setting: &ISetting, match_: &Match) -> Range {
        Range {
            start_line_number: setting.value_range.start_line_number,
            start_column: setting.value_range.start_column + match_.start as i32 + 1,
            end_line_number: setting.value_range.start_line_number,
            end_column: setting.value_range.start_column + match_.end as i32 + 1,
        }
    }
}
