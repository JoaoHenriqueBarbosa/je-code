//! Porte de `src/vs/workbench/services/preferences/browser/keybindingsEditorModel.ts`: o modelo do editor de atalhos.
//! Junta os atalhos do serviço de atalhos com os comandos sem atalho, ordena a lista e filtra pela busca (`@command:`,
//! `@source:`, `@ext:`, `@keybinding:` ou texto livre), devolvendo os trechos que casaram para a interface destacar.
//!
//! Divergências:
//! - **Herança.** `KeybindingsEditorModel extends EditorModel` guarda um [`EditorModel`] em `base` e implementa
//!   [`IEditorModel`] (ver `diff_editor_model.rs`). O `resolve(actionLabels = new Map())` do original, que sobrescreve o
//!   `resolve()` da base, é [`KeybindingsEditorModel::resolve_with_action_labels`]; o `resolve()` do trait chama esse com o
//!   mapa vazio (o valor padrão do parâmetro). O trabalho do `async` roda inteiro antes de a promessa voltar, como no
//!   original (a função não tem `await`).
//! - **Injeção.** `@IKeybindingService` e `@IExtensionService` são os `Rc<dyn ...>` do construtor
//!   ([`KeybindingsEditorModel::new`]); o `os` vem primeiro, como no original.
//! - **Texto é UTF-16 nos filtros.** Os filtros de `je_base::filters` recebem `&[u16]` e devolvem posições UTF-16, como o
//!   `IMatch` do original; as buscas convertem o `&str` na chamada. `RegExp` é o `zyarr::RegExp` (ECMAScript, flag `i`).
//! - **`isEmptyObject(chordPartMatch)`** é a comparação com o [`KeybindingMatch`] padrão (tudo falso).
//! - **`coalesce(word.split('+'))`** é descartar os trechos vazios (o `coalesce` do original tira os valores falsos, e o
//!   único falso possível ali é a string vazia).
//! - **Ordenação.** `sort` do original (TimSort do V8, com o comparador que não é uma ordem total quando dois atalhos
//!   padrão têm o mesmo comando) é [`sort_by_compare`] de `je_base::arrays`, que reproduz o V8 nas listas curtas.
//! - **`getAllUnboundCommands`** vive em `services/keybinding/unbound_commands.rs`, o lugar do original.
//! - **`localize('cat.title', ...)`, `Language.isDefaultVariant()`** são o `localize` e o `language::is_default_variant`
//!   de `je_base` sobre o idioma da plataforma detectada.
//! - `ModifierLabels` do arquivo (a tupla `ui`/`aria`/`user`) é [`KeybindingModifierLabels`], para não colidir com o
//!   `ModifierLabels` de `keybindingLabels.ts`, que aqui tem o nome original.

use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::arrays::{CompareResult, distinct, sort_by_compare};
use je_base::collator::locale_compare;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::filters::{FilterFn, Match, matches_base_contiguous_sub_string, matches_camel_case, matches_contiguous_sub_string, matches_words, or};
use je_base::js_string::trim;
use je_base::keybinding_labels::{ModifierLabelProvider, ModifierLabels};
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::platform::{OperatingSystem, Platform, language};
use je_base::promise::Promise;
use je_base::strings::{compare_ignore_case, equals_ignore_case};
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap};
use je_extensions::scanner::ExtensionDescription;
use je_platform::action::{Category, CommandAction, Title};
use je_platform::actions::menu_registry;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use zyarr::RegExp;

use crate::common::editor::editor_model::{EditorModel, IEditorModel};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::keybinding::unbound_commands::get_all_unbound_commands;
use crate::services::preferences::preferences::{IKeybindingItem, IKeybindingItemEntry, KeybindingMatch, KeybindingMatches, KeybindingSource};

pub const KEYBINDING_ENTRY_TEMPLATE_ID: &str = "keybinding.entry.template";

fn source_system() -> String {
    localize("default", "System", &[])
}

fn source_extension() -> String {
    localize("extension", "Extension", &[])
}

fn source_user() -> String {
    localize("user", "User", &[])
}

/// `ModifierLabels` do arquivo: os rótulos dos modificadores nos três formatos que a busca aceita.
struct KeybindingModifierLabels {
    ui: ModifierLabels,
    aria: ModifierLabels,
    user: ModifierLabels,
}

/// `createKeybindingCommandQuery`.
pub fn create_keybinding_command_query(command_id: &str, when: Option<&str>) -> String {
    let when_part = when.filter(|when| !when.is_empty()).map(|when| format!(" +when:{when}")).unwrap_or_default();
    format!("@command:{command_id}{when_part}")
}

/// Um filtro de palavra (`IFilter`).
type TextFilter<'a> = &'a dyn Fn(&[u16], &[u16]) -> Option<Vec<Match>>;

/// `or(matchesBaseContiguousSubString, matchesWords)`.
fn word_filter() -> impl Fn(&[u16], &[u16]) -> Option<Vec<Match>> {
    let filters: Vec<Box<FilterFn>> = vec![Box::new(matches_base_contiguous_sub_string), Box::new(|word: &[u16], target: &[u16]| matches_words(word, target, false))];
    or(filters)
}

/// `or(matchesWords, matchesCamelCase)`.
fn words_or_camel_case() -> impl Fn(&[u16], &[u16]) -> Option<Vec<Match>> {
    let filters: Vec<Box<FilterFn>> = vec![Box::new(|word: &[u16], target: &[u16]| matches_words(word, target, false)), Box::new(matches_camel_case)];
    or(filters)
}

/// `(word, wordToMatchAgainst) => matchesWords(word, target, true)`: o alvo é fixo e o segundo argumento é ignorado.
fn contiguous_words_filter(target: &str) -> impl Fn(&[u16], &[u16]) -> Option<Vec<Match>> {
    let target = units(target);
    move |word, _| matches_words(word, &target, true)
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `text.trim()`: o espaço em branco do `String.prototype.trim`.
fn js_trim(text: &str) -> String {
    String::from_utf16_lossy(trim(&units(text)))
}

/// As expressões regulares da busca (`COMMAND_REGEX`, `WHEN_REGEX`, `SOURCE_REGEX`, `EXTENSION_REGEX`,
/// `KEYBINDING_REGEX`) e as quatro de `filterBySource`, todas sem distinguir a caixa.
struct SearchRegexes {
    command: RegExp,
    when: RegExp,
    source: RegExp,
    source_default: RegExp,
    source_system: RegExp,
    source_user: RegExp,
    source_extension: RegExp,
    extension: RegExp,
    keybinding: RegExp,
}

impl SearchRegexes {
    fn new() -> Self {
        let regex = |pattern: &str| RegExp::from_str(pattern, "i").expect("a regex do keybindingsEditorModel é válida");
        Self {
            command: regex(r"@command:\s*([^\+]+)"),
            when: regex(r"\+when:\s*(.+)"),
            source: regex(r"@source:\s*(user|default|system|extension)"),
            source_default: regex(r"@source:\s*default"),
            source_system: regex(r"@source:\s*system"),
            source_user: regex(r"@source:\s*user"),
            source_extension: regex(r"@source:\s*extension"),
            extension: regex(r#"@ext:\s*((".+")|([^\s]+))"#),
            keybinding: regex(r#"@keybinding:\s*((".+")|(\S+))"#),
        }
    }
}

thread_local! {
    static SEARCH_REGEXES: Rc<SearchRegexes> = Rc::new(SearchRegexes::new());
}

fn search_regexes() -> Rc<SearchRegexes> {
    SEARCH_REGEXES.with(Rc::clone)
}

/// O resultado de `regex.exec(text)`: onde o casamento está (em unidades UTF-16) e o texto de cada grupo.
struct FoundMatch {
    start: usize,
    end: usize,
    groups: Vec<Option<String>>,
}

impl FoundMatch {
    /// `match[index]` usado como condição: o grupo, se participou e não é vazio (os dois são falsos em JavaScript).
    fn group(&self, index: usize) -> Option<&str> {
        self.groups.get(index).and_then(Option::as_deref).filter(|group| !group.is_empty())
    }
}

/// `regex.exec(text)`.
fn exec(regex: &RegExp, text: &str) -> Option<FoundMatch> {
    let text_units = units(text);
    let found = regex.exec(&text_units, 0)?;
    let groups = found.captures.iter().map(|range| range.as_ref().map(|range| String::from_utf16_lossy(&text_units[range.clone()]))).collect();
    Some(FoundMatch { start: found.start(), end: found.end(), groups })
}

/// `regex.test(text)`.
fn test(regex: &RegExp, text: &str) -> bool {
    regex.exec(&units(text), 0).is_some()
}

/// `text.replace(regex, '')` de uma regex sem a flag global: tira só o primeiro casamento.
fn remove_match(text: &str, found: &FoundMatch) -> String {
    let text_units = units(text);
    String::from_utf16_lossy(&[&text_units[..found.start], &text_units[found.end..]].concat())
}

fn ordering_to_compare_result(ordering: Ordering) -> CompareResult {
    match ordering {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

/// `KeybindingsEditorModel`.
pub struct KeybindingsEditorModel {
    base: EditorModel,
    keybindings_service: Rc<dyn IKeybindingService>,
    extension_service: Rc<dyn IExtensionService>,
    keybinding_items: RefCell<Vec<Rc<IKeybindingItem>>>,
    keybinding_items_sorted_by_precedence: RefCell<Vec<Rc<IKeybindingItem>>>,
    modifier_labels: KeybindingModifierLabels,
}

impl KeybindingsEditorModel {
    pub fn new(os: OperatingSystem, keybindings_service: Rc<dyn IKeybindingService>, extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        Rc::new(Self {
            base: EditorModel::new_inner(),
            keybindings_service,
            extension_service,
            keybinding_items: RefCell::new(Vec::new()),
            keybinding_items_sorted_by_precedence: RefCell::new(Vec::new()),
            modifier_labels: KeybindingModifierLabels {
                ui: ModifierLabelProvider::ui().modifier_labels(os).clone(),
                aria: ModifierLabelProvider::aria().modifier_labels(os).clone(),
                user: ModifierLabelProvider::user_settings().modifier_labels(os).clone(),
            },
        })
    }

    /// `fetch(searchValue, sortByPrecedence = false)`.
    pub fn fetch(&self, search_value: &str, sort_by_precedence: bool) -> Vec<IKeybindingItemEntry> {
        let mut keybinding_items = if sort_by_precedence { self.keybinding_items_sorted_by_precedence.borrow().clone() } else { self.keybinding_items.borrow().clone() };
        let mut search_value = search_value.to_string();
        let regexes = search_regexes();

        // @command:COMMAND_ID
        if let Some(command_id_matches) = exec(&regexes.command, &search_value)
            && let Some(command) = command_id_matches.group(1)
        {
            let command = js_trim(command);
            let mut filtered_keybinding_items: Vec<Rc<IKeybindingItem>> = keybinding_items.iter().filter(|k| k.command == command).cloned().collect();

            // +when:WHEN_EXPRESSION
            if !filtered_keybinding_items.is_empty()
                && let Some(when_matches) = exec(&regexes.when, &search_value)
                && let Some(when_value) = when_matches.group(1)
            {
                let when_value = js_trim(when_value);
                filtered_keybinding_items = self.filter_by_when(&filtered_keybinding_items, &command, &when_value);
            }

            return filtered_keybinding_items.into_iter().map(Self::to_entry).collect();
        }

        // @source:SOURCE
        if test(&regexes.source, &search_value) {
            keybinding_items = Self::filter_by_source(&keybinding_items, &search_value);
            if let Some(found) = exec(&regexes.source, &search_value) {
                search_value = remove_match(&search_value, &found);
            }
        } else if let Some(extension_matches) = exec(&regexes.extension, &search_value)
            && (extension_matches.group(2).is_some() || extension_matches.group(3).is_some())
        {
            // @ext:EXTENSION_ID
            let extension_id = match extension_matches.group(2) {
                Some(quoted) => quoted[1..quoted.len() - 1].to_string(),
                None => extension_matches.group(3).unwrap_or_default().to_string(),
            };
            keybinding_items = Self::filter_by_extension(&keybinding_items, &extension_id);
            search_value = remove_match(&search_value, &extension_matches);
        } else if let Some(keybinding_matches) = exec(&regexes.keybinding, &search_value)
            && (keybinding_matches.group(2).is_some() || keybinding_matches.group(3).is_some())
        {
            // @keybinding:KEYBINDING
            search_value = match keybinding_matches.group(2) {
                Some(quoted) => quoted.to_string(),
                None => format!("\"{}\"", keybinding_matches.group(3).unwrap_or_default()),
            };
        }

        let search_value = js_trim(&search_value);
        if search_value.is_empty() {
            return keybinding_items.into_iter().map(Self::to_entry).collect();
        }

        self.filter_by_text(&keybinding_items, &search_value)
    }

    /// `({ id: KeybindingsEditorModel.getId(keybindingItem), keybindingItem, templateId: KEYBINDING_ENTRY_TEMPLATE_ID })`.
    fn to_entry(keybinding_item: Rc<IKeybindingItem>) -> IKeybindingItemEntry {
        IKeybindingItemEntry::new(Self::get_id(&keybinding_item), KEYBINDING_ENTRY_TEMPLATE_ID, keybinding_item)
    }

    fn filter_by_source(keybinding_items: &[Rc<IKeybindingItem>], search_value: &str) -> Vec<Rc<IKeybindingItem>> {
        let regexes = search_regexes();
        let is_label = |keybinding_item: &IKeybindingItem, label: &str| matches!(&keybinding_item.source, KeybindingSource::Label(source) if source == label);
        if test(&regexes.source_default, search_value) || test(&regexes.source_system, search_value) {
            let system = source_system();
            return keybinding_items.iter().filter(|k| is_label(k, &system)).cloned().collect();
        }
        if test(&regexes.source_user, search_value) {
            let user = source_user();
            return keybinding_items.iter().filter(|k| is_label(k, &user)).cloned().collect();
        }
        if test(&regexes.source_extension, search_value) {
            let extension = source_extension();
            return keybinding_items.iter().filter(|k| !matches!(k.source, KeybindingSource::Label(_)) || is_label(k, &extension)).cloned().collect();
        }
        keybinding_items.to_vec()
    }

    fn filter_by_extension(keybinding_items: &[Rc<IKeybindingItem>], extension: &str) -> Vec<Rc<IKeybindingItem>> {
        let extension = js_trim(&extension.to_lowercase());
        keybinding_items
            .iter()
            .filter(|k| match &k.source {
                KeybindingSource::Label(_) => false,
                KeybindingSource::Extension(description) => {
                    ExtensionIdentifier::equals(Some(&description.identifier.value), Some(&extension))
                        || description.manifest.display_name.as_ref().is_some_and(|display_name| display_name.to_lowercase() == extension.to_lowercase())
                }
            })
            .cloned()
            .collect()
    }

    fn filter_by_text(&self, keybinding_items: &[Rc<IKeybindingItem>], search_value: &str) -> Vec<IKeybindingItemEntry> {
        let quote_at_first_char = search_value.starts_with('"');
        let quote_at_last_char = search_value.ends_with('"');
        let complete_match = quote_at_first_char && quote_at_last_char;
        let mut search_value = search_value.to_string();
        if quote_at_first_char {
            search_value.remove(0);
        }
        if quote_at_last_char {
            search_value.pop();
        }
        let search_value = js_trim(&search_value);

        let mut result: Vec<IKeybindingItemEntry> = Vec::new();
        let words: Vec<String> = search_value.split(' ').map(str::to_string).collect();
        let keybinding_words = Self::split_keybinding_words(&words);
        for keybinding_item in keybinding_items {
            let keybinding_matches = KeybindingItemMatches::new(&self.modifier_labels, keybinding_item, &search_value, &words, &keybinding_words, complete_match);
            if keybinding_matches.command_id_matches.is_some()
                || keybinding_matches.command_label_matches.is_some()
                || keybinding_matches.command_default_label_matches.is_some()
                || keybinding_matches.source_matches.is_some()
                || keybinding_matches.when_matches.is_some()
                || keybinding_matches.keybinding_matches.is_some()
                || keybinding_matches.extension_id_matches.is_some()
                || keybinding_matches.extension_label_matches.is_some()
            {
                result.push(IKeybindingItemEntry {
                    command_label_matches: keybinding_matches.command_label_matches,
                    command_default_label_matches: keybinding_matches.command_default_label_matches,
                    keybinding_matches: keybinding_matches.keybinding_matches,
                    command_id_matches: keybinding_matches.command_id_matches,
                    source_matches: keybinding_matches.source_matches,
                    when_matches: keybinding_matches.when_matches,
                    extension_id_matches: keybinding_matches.extension_id_matches,
                    extension_label_matches: keybinding_matches.extension_label_matches,
                    ..Self::to_entry(keybinding_item.clone())
                });
            }
        }
        result
    }

    fn filter_by_when(&self, keybinding_items: &[Rc<IKeybindingItem>], command: &str, when: &str) -> Vec<Rc<IKeybindingItem>> {
        if keybinding_items.is_empty() {
            return Vec::new();
        }

        // Check if a keybinding with the same command id and when clause exists
        let keybinding_items_with_when: Vec<Rc<IKeybindingItem>> = keybinding_items.iter().filter(|k| k.when == when).cloned().collect();
        if !keybinding_items_with_when.is_empty() {
            return keybinding_items_with_when;
        }

        // Create a new entry with the when clause which does not live in the model
        // We can reuse some of the properties from the same command with different when clause
        let command_label = keybinding_items[0].command_label.clone();

        let keybinding_item = ResolvedKeybindingItem::new(None, Some(command), Dynamic::Null, ContextKeyExpr::deserialize(Some(when)), false, None, false, false);
        let action_labels = HashMap::from([(command.to_string(), command_label)]);
        vec![Self::to_keybinding_entry(command, &keybinding_item, &action_labels, &self.get_extensions_mapping())]
    }

    fn split_keybinding_words(words_separated_by_spaces: &[String]) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        for word in words_separated_by_spaces {
            result.extend(word.split('+').filter(|part| !part.is_empty()).map(str::to_string));
        }
        result
    }

    /// `resolve(actionLabels = new Map<string, string>())`.
    pub fn resolve_with_action_labels(&self, action_labels: &HashMap<String, String>) -> Promise<()> {
        let extensions = self.get_extensions_mapping();

        let mut sorted_by_precedence: Vec<Rc<IKeybindingItem>> = Vec::new();
        let mut bound_commands: HashMap<String, bool> = HashMap::new();
        for keybinding in self.keybindings_service.get_keybindings() {
            if let Some(command) = keybinding.command.as_deref().filter(|command| !command.is_empty()) {
                // Skip keybindings without commands
                sorted_by_precedence.push(Self::to_keybinding_entry(command, &keybinding, action_labels, &extensions));
                bound_commands.insert(command.to_string(), true);
            }
        }

        let commands_with_default_keybindings: Vec<Option<String>> = self.keybindings_service.get_default_keybindings().iter().map(|keybinding| keybinding.command.clone()).collect();
        for command in get_all_unbound_commands(&bound_commands) {
            let is_default = !commands_with_default_keybindings.contains(&Some(command.clone()));
            let keybinding_item = ResolvedKeybindingItem::new(None, Some(&command), Dynamic::Null, None, is_default, None, false, false);
            sorted_by_precedence.push(Self::to_keybinding_entry(&command, &keybinding_item, action_labels, &extensions));
        }
        let sorted_by_precedence = distinct(&sorted_by_precedence, |keybinding_item| Self::get_id(keybinding_item));
        let mut keybinding_items = sorted_by_precedence.clone();
        sort_by_compare(&mut keybinding_items, |a, b| Self::compare_keybinding_data(a, b));
        *self.keybinding_items_sorted_by_precedence.borrow_mut() = sorted_by_precedence;
        *self.keybinding_items.borrow_mut() = keybinding_items;

        self.base.resolve()
    }

    fn get_id(keybinding_item: &IKeybindingItem) -> String {
        let aria_label = keybinding_item.keybinding.as_ref().and_then(|keybinding| keybinding.get_aria_label()).unwrap_or_default();
        let source = match &keybinding_item.source {
            KeybindingSource::Label(source) => source.clone(),
            KeybindingSource::Extension(extension) => extension.identifier.value.clone(),
        };
        format!("{}{}{}{}", keybinding_item.command, aria_label, keybinding_item.when, source)
    }

    fn get_extensions_mapping(&self) -> ExtensionIdentifierMap<Rc<ExtensionDescription>> {
        let mut extensions = ExtensionIdentifierMap::new();
        for extension in self.extension_service.extensions() {
            let identifier = extension.identifier.value.clone();
            extensions.set(&identifier, Rc::new(extension));
        }
        extensions
    }

    fn compare_keybinding_data(a: &IKeybindingItem, b: &IKeybindingItem) -> CompareResult {
        if a.keybinding.is_some() && b.keybinding.is_none() {
            return -1.0;
        }
        if b.keybinding.is_some() && a.keybinding.is_none() {
            return 1.0;
        }
        let a_has_label = !a.command_label.is_empty();
        let b_has_label = !b.command_label.is_empty();
        if a_has_label && !b_has_label {
            return -1.0;
        }
        if b_has_label && !a_has_label {
            return 1.0;
        }
        if a_has_label && b_has_label && a.command_label != b.command_label {
            return ordering_to_compare_result(locale_compare(&a.command_label, &b.command_label));
        }
        if a.command == b.command {
            return if a.keybinding_item.is_default { 1.0 } else { -1.0 };
        }
        ordering_to_compare_result(locale_compare(&a.command, &b.command))
    }

    fn to_keybinding_entry(command: &str, keybinding_item: &Rc<ResolvedKeybindingItem>, actions: &HashMap<String, String>, extensions: &ExtensionIdentifierMap<Rc<ExtensionDescription>>) -> Rc<IKeybindingItem> {
        let menu_command = menu_registry().get_command(command);
        let editor_action_label = actions.get(command);
        let mut source = KeybindingSource::Label(source_user());
        if keybinding_item.is_default {
            let extension_id = keybinding_item.extension_id.clone().or_else(|| {
                if keybinding_item.resolved_keybinding.is_some() { None } else { menu_command.as_ref().and_then(|menu_command| menu_command.source.as_ref()).map(|source| source.id.clone()) }
            });
            source = match extension_id.filter(|extension_id| !extension_id.is_empty()) {
                Some(extension_id) => match extensions.get(&extension_id) {
                    Some(extension) => KeybindingSource::Extension(extension.clone()),
                    None => KeybindingSource::Label(source_extension()),
                },
                None => KeybindingSource::Label(source_system()),
            };
        }
        Rc::new(IKeybindingItem {
            keybinding: keybinding_item.resolved_keybinding.clone(),
            keybinding_item: keybinding_item.clone(),
            command: command.to_string(),
            command_label: Self::get_command_label(menu_command.as_ref(), editor_action_label.map(String::as_str)),
            command_default_label: Self::get_command_default_label(menu_command.as_ref()),
            when: keybinding_item.when.as_ref().map(|when| when.serialize()).unwrap_or_default(),
            source,
        })
    }

    fn get_command_default_label(menu_command: Option<&CommandAction>) -> Option<String> {
        if !language::is_default_variant(&Platform::detect().language)
            && let Some(menu_command) = menu_command
            && let Title::Localized(title) = &menu_command.title
            && !title.original.is_empty()
        {
            let category = match &menu_command.category {
                Some(Category::Localized(category)) if !category.original.is_empty() => Some(category.original.as_str()),
                _ => None,
            };
            return Some(match category {
                Some(category) => localize("cat.title", "{0}: {1}", &[category, title.original.as_str()]),
                None => title.original.clone(),
            });
        }
        None
    }

    fn get_command_label(menu_command: Option<&CommandAction>, editor_action_label: Option<&str>) -> String {
        if let Some(menu_command) = menu_command {
            let category = match &menu_command.category {
                Some(Category::Text(category)) => Some(category.as_str()),
                Some(Category::Localized(category)) => Some(category.value.as_str()),
                None => None,
            }
            .filter(|category| !category.is_empty());
            let title = match &menu_command.title {
                Title::Plain(title) => title.as_str(),
                Title::Localized(title) => title.value.as_str(),
            };
            return match category {
                Some(category) => localize("cat.title", "{0}: {1}", &[category, title]),
                None => title.to_string(),
            };
        }

        if let Some(editor_action_label) = editor_action_label.filter(|label| !label.is_empty()) {
            return editor_action_label.to_string();
        }

        String::new()
    }
}

impl IDisposable for KeybindingsEditorModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorModel for KeybindingsEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        self.resolve_with_action_labels(&HashMap::new())
    }

    fn is_resolved(&self) -> bool {
        self.base.is_resolved()
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}

/// `KeybindingItemMatches`: onde a busca casou em cada campo de um atalho.
struct KeybindingItemMatches<'a> {
    modifier_labels: &'a KeybindingModifierLabels,
    command_id_matches: Option<Vec<Match>>,
    command_label_matches: Option<Vec<Match>>,
    command_default_label_matches: Option<Vec<Match>>,
    source_matches: Option<Vec<Match>>,
    when_matches: Option<Vec<Match>>,
    keybinding_matches: Option<KeybindingMatches>,
    extension_id_matches: Option<Vec<Match>>,
    extension_label_matches: Option<Vec<Match>>,
}

impl<'a> KeybindingItemMatches<'a> {
    fn new(modifier_labels: &'a KeybindingModifierLabels, keybinding_item: &IKeybindingItem, search_value: &str, words: &[String], keybinding_words: &[String], complete_match: bool) -> Self {
        let mut this = Self {
            modifier_labels,
            command_id_matches: None,
            command_label_matches: None,
            command_default_label_matches: None,
            source_matches: None,
            when_matches: None,
            keybinding_matches: None,
            extension_id_matches: None,
            extension_label_matches: None,
        };
        if !complete_match {
            let word_filter = word_filter();
            let words_or_camel_case = words_or_camel_case();
            this.command_id_matches = Self::matches(Some(search_value), &keybinding_item.command, &words_or_camel_case, words, &word_filter);
            this.command_label_matches = if keybinding_item.command_label.is_empty() {
                None
            } else {
                Self::matches(Some(search_value), &keybinding_item.command_label, &contiguous_words_filter(&keybinding_item.command_label), words, &word_filter)
            };
            this.command_default_label_matches = keybinding_item
                .command_default_label
                .as_deref()
                .filter(|label| !label.is_empty())
                .and_then(|label| Self::matches(Some(search_value), label, &contiguous_words_filter(label), words, &word_filter));
            this.when_matches = if keybinding_item.when.is_empty() { None } else { Self::matches(None, &keybinding_item.when, &words_or_camel_case, words, &word_filter) };
            match &keybinding_item.source {
                KeybindingSource::Label(source) => {
                    this.source_matches = Self::matches(Some(search_value), source, &contiguous_words_filter(source), words, &word_filter);
                }
                KeybindingSource::Extension(extension) => {
                    // O filtro do rótulo da extensão usa o `commandLabel` como alvo, como o original.
                    this.extension_label_matches = extension.manifest.display_name.as_deref().filter(|display_name| !display_name.is_empty()).and_then(|display_name| {
                        Self::matches(Some(search_value), display_name, &contiguous_words_filter(&keybinding_item.command_label), words, &word_filter)
                    });
                }
            }
        }
        let keybinding_matches = keybinding_item.keybinding.as_ref().and_then(|keybinding| this.matches_keybinding(keybinding.as_ref(), search_value, keybinding_words, complete_match));
        this.keybinding_matches = keybinding_matches;
        this
    }

    fn matches(search_value: Option<&str>, word_to_match_against: &str, word_matches_filter: TextFilter, words: &[String], word_filter: TextFilter) -> Option<Vec<Match>> {
        let target = units(word_to_match_against);
        let mut matches = search_value.filter(|search_value| !search_value.is_empty()).and_then(|search_value| word_filter(&units(search_value), &target));
        if matches.is_none() {
            matches = Self::matches_words(words, &target, word_matches_filter);
        }
        matches.map(Self::filter_and_sort)
    }

    fn matches_words(words: &[String], word_to_match_against: &[u16], word_matches_filter: TextFilter) -> Option<Vec<Match>> {
        let mut matches: Vec<Match> = Vec::new();
        for word in words {
            matches.extend(word_matches_filter(&units(word), word_to_match_against)?);
        }
        Some(matches)
    }

    fn filter_and_sort(matches: Vec<Match>) -> Vec<Match> {
        let mut result: Vec<Match> = distinct(&matches, |a| (a.start, a.end))
            .into_iter()
            .filter(|match_| !matches.iter().any(|m| !(m.start == match_.start && m.end == match_.end) && (m.start <= match_.start && m.end >= match_.end)))
            .collect();
        result.sort_by_key(|match_| match_.start);
        result
    }

    fn matches_keybinding(&self, keybinding: &dyn ResolvedKeybinding, search_value: &str, words: &[String], complete_match: bool) -> Option<KeybindingMatches> {
        let chords = keybinding.get_chords();
        let (first_part, chord_part) = (chords.first(), chords.get(1));

        let user_settings_label = keybinding.get_user_settings_label();
        let aria_label = keybinding.get_aria_label();
        let label = keybinding.get_label();
        let search_value_units = units(search_value);
        let equals_search_value = |label: &Option<String>| label.as_deref().filter(|label| !label.is_empty()).is_some_and(|label| compare_ignore_case(&search_value_units, &units(label)) == 0);
        if equals_search_value(&user_settings_label) || equals_search_value(&aria_label) || equals_search_value(&label) {
            return Some(KeybindingMatches { first_part: Self::create_complete_match(first_part), chord_part: Self::create_complete_match(chord_part) });
        }

        let mut first_part_match = KeybindingMatch::default();
        let mut chord_part_match = KeybindingMatch::default();

        let mut matched_words: Vec<usize> = Vec::new();
        let mut first_part_matched_words: Vec<usize> = Vec::new();
        let mut chord_part_matched_words: Vec<usize> = Vec::new();
        let mut match_first_part = true;
        for (index, word) in words.iter().enumerate() {
            let mut first_part_matched = false;
            let mut chord_part_matched = false;

            match_first_part = match_first_part && !first_part_match.key_code;
            let mut match_chord_part = !chord_part_match.key_code;

            if match_first_part {
                first_part_matched = self.match_part(first_part, &mut first_part_match, word, complete_match);
                if first_part_match.key_code {
                    for cord_part_matched_word_index in &chord_part_matched_words {
                        if !first_part_matched_words.contains(cord_part_matched_word_index) {
                            // `matchedWords.splice(matchedWords.indexOf(i), 1)`: um índice ausente (-1) tira o último.
                            match matched_words.iter().position(|matched_word| matched_word == cord_part_matched_word_index) {
                                Some(position) => {
                                    matched_words.remove(position);
                                }
                                None => {
                                    matched_words.pop();
                                }
                            }
                        }
                    }
                    chord_part_match = KeybindingMatch::default();
                    chord_part_matched_words = Vec::new();
                    match_chord_part = false;
                }
            }

            if match_chord_part {
                chord_part_matched = self.match_part(chord_part, &mut chord_part_match, word, complete_match);
            }

            if first_part_matched {
                first_part_matched_words.push(index);
            }
            if chord_part_matched {
                chord_part_matched_words.push(index);
            }
            if first_part_matched || chord_part_matched {
                matched_words.push(index);
            }

            match_first_part = match_first_part && self.is_modifier(word);
        }
        if matched_words.len() != words.len() {
            return None;
        }
        if complete_match {
            if !Self::is_complete_match(first_part, &first_part_match) {
                return None;
            }
            if chord_part_match != KeybindingMatch::default() && !Self::is_complete_match(chord_part, &chord_part_match) {
                return None;
            }
        }
        if Self::has_any_match(&first_part_match) || Self::has_any_match(&chord_part_match) { Some(KeybindingMatches { first_part: first_part_match, chord_part: chord_part_match }) } else { None }
    }

    fn match_part(&self, chord: Option<&ResolvedChord>, match_: &mut KeybindingMatch, word: &str, complete_match: bool) -> bool {
        let mut matched = false;
        if chord.is_some_and(|chord| chord.meta_key) && self.word_matches_meta_modifier(word) {
            matched = true;
            match_.meta_key = true;
        }
        if chord.is_some_and(|chord| chord.ctrl_key) && self.word_matches_ctrl_modifier(word) {
            matched = true;
            match_.ctrl_key = true;
        }
        if chord.is_some_and(|chord| chord.shift_key) && self.word_matches_shift_modifier(word) {
            matched = true;
            match_.shift_key = true;
        }
        if chord.is_some_and(|chord| chord.alt_key) && self.word_matches_alt_modifier(word) {
            matched = true;
            match_.alt_key = true;
        }
        if Self::matches_key_code(chord, word, complete_match) {
            match_.key_code = true;
            matched = true;
        }
        matched
    }

    fn matches_key_code(chord: Option<&ResolvedChord>, word: &str, complete_match: bool) -> bool {
        let Some(chord) = chord else {
            return false;
        };
        let aria_label = chord.key_aria_label.as_deref().unwrap_or_default();
        let (aria_label, word) = (units(aria_label), units(word));
        if complete_match || aria_label.len() == 1 || word.len() == 1 {
            compare_ignore_case(&aria_label, &word) == 0
        } else {
            matches_contiguous_sub_string(&word, &aria_label).is_some()
        }
    }

    fn has_any_match(keybinding_match: &KeybindingMatch) -> bool {
        keybinding_match.alt_key || keybinding_match.ctrl_key || keybinding_match.meta_key || keybinding_match.shift_key || keybinding_match.key_code
    }

    fn is_complete_match(chord: Option<&ResolvedChord>, match_: &KeybindingMatch) -> bool {
        let Some(chord) = chord else {
            return true;
        };
        if !match_.key_code {
            return false;
        }
        if chord.meta_key && !match_.meta_key {
            return false;
        }
        if chord.alt_key && !match_.alt_key {
            return false;
        }
        if chord.ctrl_key && !match_.ctrl_key {
            return false;
        }
        if chord.shift_key && !match_.shift_key {
            return false;
        }
        true
    }

    fn create_complete_match(chord: Option<&ResolvedChord>) -> KeybindingMatch {
        let mut match_ = KeybindingMatch::default();
        if let Some(chord) = chord {
            match_.key_code = true;
            match_.meta_key = chord.meta_key;
            match_.alt_key = chord.alt_key;
            match_.ctrl_key = chord.ctrl_key;
            match_.shift_key = chord.shift_key;
        }
        match_
    }

    fn is_modifier(&self, word: &str) -> bool {
        self.word_matches_alt_modifier(word) || self.word_matches_ctrl_modifier(word) || self.word_matches_meta_modifier(word) || self.word_matches_shift_modifier(word)
    }

    /// A palavra é igual (sem distinguir a caixa) ao rótulo do modificador em algum dos três formatos, ou ao texto extra.
    fn word_matches_modifier(&self, word: &str, label_of: fn(&ModifierLabels) -> &str, extra_label: Option<String>) -> bool {
        let word = units(word);
        let matches_label = |label: &str| equals_ignore_case(&units(label), &word);
        [&self.modifier_labels.ui, &self.modifier_labels.aria, &self.modifier_labels.user].into_iter().any(|labels| matches_label(label_of(labels)))
            || extra_label.is_some_and(|label| matches_label(&label))
    }

    fn word_matches_alt_modifier(&self, word: &str) -> bool {
        self.word_matches_modifier(word, |labels| &labels.alt_key, Some(localize("option", "option", &[])))
    }

    fn word_matches_ctrl_modifier(&self, word: &str) -> bool {
        self.word_matches_modifier(word, |labels| &labels.ctrl_key, None)
    }

    fn word_matches_meta_modifier(&self, word: &str) -> bool {
        self.word_matches_modifier(word, |labels| &labels.meta_key, Some(localize("meta", "meta", &[])))
    }

    fn word_matches_shift_modifier(&self, word: &str) -> bool {
        self.word_matches_modifier(word, |labels| &labels.shift_key, None)
    }
}
