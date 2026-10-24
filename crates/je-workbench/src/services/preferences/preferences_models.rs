//! Porte parcial de `src/vs/workbench/services/preferences/common/preferencesModels.ts`: o `nullRange`, o `isNullRange`,
//! o `fixSettingLinks` e o `DefaultSettings` (os grupos de configurações do registro, com o conteúdo textual das
//! configurações padrão e as posições de cada uma nele), com o `SettingsContentBuilder` que o monta.
//!
//! Fora do porte (dependem do `ITextModel`, do `EditorModel` e dos editores de preferências, que entram com o porte das
//! telas de preferências): `AbstractSettingsModel`, `SettingsEditorModel`, `Settings2EditorModel`, o `parse` de modelo,
//! `WorkspaceConfigurationEditorModel`, `DefaultSettingsEditorModel`, `RawSettingsContentBuilder`,
//! `DefaultRawSettingsEditorModel`, `defaultKeybindingsContents` e `DefaultKeybindingsEditorModel`.
//!
//! Divergências:
//! - `IRange` é o [`Range`] do `je-editor` (o mesmo tipo), que aqui se escreve como literal, porque `Range::new` reordena
//!   início e fim e o sentinela não é um intervalo válido.
//! - **Objetos compartilhados.** As seções guardam `Rc<ISetting>`: o `SettingsContentBuilder` do original atribui
//!   `range`, `keyRange`, `valueRange` e `descriptionRanges` nas configurações do grupo, e aqui ele troca o `Rc` por uma
//!   cópia atualizada (`Rc::make_mut`). `_settingsByName` só é lido antes da montagem do conteúdo, então a troca não é
//!   observável.
//! - **Esquema de propriedade.** Os campos de `IRegisteredConfigurationPropertySchema` são lidos do [`PropertySchema`] por
//!   nome, com a mesma truthiness do JavaScript (`prop.markdownDescription || prop.description || ''`). `enum` e as listas
//!   de descrições são listas de texto (cada item pelo `String(item)`).
//! - `console.error` do `parseSetting` (configuração com campos de `enum` sem `enum`) não tem equivalente: é só um
//!   diagnóstico, e a configuração é devolvida do mesmo jeito.
//! - `Array.prototype.sort` é o TimSort do V8 de `je_base::arrays::sort_by_compare`: o `compareGroups` não é uma ordem
//!   total, e o resultado segue o do JavaScript.
//! - `onDidChangeConfiguration(e => ...)`: o `DefaultSettings` segura-se por referência fraca no ouvinte.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::sort_by_compare;
use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, Truthy, json_stringify_indented};
use je_base::event::{Emitter, Event};
use je_base::json::Value;
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_editor::range::Range;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::registry::{ConfigurationDefaultSource, ConfigurationScope, PropertySchema, configuration_registry, is_override_property};
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::{object_keys, to_js_string, to_json};

use super::preferences::{ISetting, ISettingsGroup, ISettingsSection, SettingSchemaType, SettingValidator};
use super::preferences_validation::create_validator;
use crate::services::configuration::common::configuration::{FOLDER_SCOPES, WORKSPACE_SCOPES};

/// `nullRange`.
pub const NULL_RANGE: Range = Range { start_line_number: -1, start_column: -1, end_line_number: -1, end_column: -1 };

/// `isNullRange`.
pub fn is_null_range(range: &Range) -> bool {
    range.start_line_number == -1 && range.start_column == -1 && range.end_line_number == -1 && range.end_column == -1
}

/// `fixSettingLinks`: tira a sintaxe de link `` `#settingId#` `` e deixa o código em linha `` `settingId` ``
/// (`text.replace(/`#([^#`]*)#`/g, (_, settingName) => `\`${settingName}\``)`).
pub fn fix_setting_links(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' && chars.get(i + 1) == Some(&'#') {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '#' && chars[j] != '`' {
                j += 1;
            }
            if chars.get(j) == Some(&'#') && chars.get(j + 1) == Some(&'`') {
                result.push('`');
                result.extend(&chars[i + 2..j]);
                result.push('`');
                i = j + 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// `escapeInvisibleChars`.
fn escape_invisible_chars(enum_value: &str) -> String {
    enum_value.replace('\n', "\\n").replace('\r', "\\r")
}

/// O tamanho em unidades UTF-16 (`string.length`).
fn js_length(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

/// `haystack.indexOf(needle)` em unidades UTF-16 (`-1` quando não há).
fn js_index_of(haystack: &str, needle: &str) -> i32 {
    let haystack: Vec<u16> = haystack.encode_utf16().collect();
    let needle: Vec<u16> = needle.encode_utf16().collect();
    je_base::js_string::index_of(&haystack, &needle, 0).map_or(-1, |index| index as i32)
}

/// `JSON.stringify(text)`.
fn json_quote(text: &str) -> String {
    json_stringify(&Value::String(JsString::from(text)), None)
}

/// `SettingsContentBuilder`.
struct SettingsContentBuilder {
    content_by_lines: Vec<String>,
    range_offset: i32,
}

impl SettingsContentBuilder {
    fn new(range_offset: i32) -> Self {
        Self { content_by_lines: Vec::new(), range_offset }
    }

    fn line_count_with_offset(&self) -> i32 {
        self.content_by_lines.len() as i32 + self.range_offset
    }

    fn last_line(&self) -> &str {
        self.content_by_lines.last().map_or("", String::as_str)
    }

    /// `lines[index] = lines[index].substring(0, length - 1)`: tira a vírgula do fim da linha.
    fn strip_trailing_comma(&mut self, index: i32) {
        if let Some(content) = self.content_by_lines.get_mut(index as usize) {
            content.pop();
        }
    }

    fn push_group(&mut self, settings_group: &mut ISettingsGroup, is_first: bool, is_last: bool) {
        self.content_by_lines.push(if is_first { "[{" } else { "{" }.to_owned());
        let last_setting_end_line = self.push_group_content(settings_group, "  ");

        if let Some(end_line_number) = last_setting_end_line {
            // Strip the comma from the last setting
            let line_idx = end_line_number - self.range_offset;
            self.strip_trailing_comma(line_idx - 2);
        }

        self.content_by_lines.push(if is_last { "}]" } else { "}," }.to_owned());
    }

    /// `_pushGroup`: devolve o `range.endLineNumber` da última configuração.
    fn push_group_content(&mut self, group: &mut ISettingsGroup, indent: &str) -> Option<i32> {
        let mut last_setting_end_line = None;
        let group_start = self.line_count_with_offset() + 1;
        for section in &mut group.sections {
            if let Some(title) = section.title.as_ref().filter(|title| !title.is_empty()) {
                self.add_description(std::slice::from_ref(title), indent);
            }

            for setting in &mut section.settings {
                let setting = Rc::make_mut(setting);
                self.push_setting(setting, indent);
                last_setting_end_line = Some(setting.range.end_line_number);
            }
        }
        group.range = Range {
            start_line_number: group_start,
            start_column: 1,
            end_line_number: self.line_count_with_offset(),
            end_column: js_length(self.last_line()),
        };
        last_setting_end_line
    }

    fn get_content(&self) -> String {
        self.content_by_lines.join("\n")
    }

    fn push_setting(&mut self, setting: &mut ISetting, indent: &str) {
        let setting_start = self.line_count_with_offset() + 1;

        self.push_setting_description(setting, indent);

        let mut pre_value_content = indent.to_owned();
        let key_string = json_quote(&setting.key);
        pre_value_content.push_str(&key_string);
        setting.key_range = Range {
            start_line_number: self.line_count_with_offset() + 1,
            start_column: js_index_of(&pre_value_content, &setting.key) + 1,
            end_line_number: self.line_count_with_offset() + 1,
            end_column: js_length(&setting.key),
        };

        pre_value_content.push_str(": ");
        let value_start = self.line_count_with_offset() + 1;
        self.push_value(setting, &pre_value_content, indent);

        setting.value_range = Range {
            start_line_number: value_start,
            start_column: js_length(&pre_value_content) + 1,
            end_line_number: self.line_count_with_offset(),
            end_column: js_length(self.last_line()) + 1,
        };
        if let Some(last) = self.content_by_lines.last_mut() {
            last.push(',');
        }
        self.content_by_lines.push(String::new());
        setting.range = Range {
            start_line_number: setting_start,
            start_column: 1,
            end_line_number: self.line_count_with_offset(),
            end_column: js_length(self.last_line()),
        };
    }

    fn push_setting_description(&mut self, setting: &mut ISetting, indent: &str) {
        setting.description_ranges = Vec::new();
        let description_pre_value = format!("{indent}// ");
        let deprecation_message_lines: Vec<String> = setting.deprecation_message.as_ref().map(|message| message.split('\n').map(str::to_owned).collect()).unwrap_or_default();
        for line in deprecation_message_lines.iter().chain(setting.description.iter()) {
            let line = fix_setting_links(line);

            self.content_by_lines.push(format!("{description_pre_value}{line}"));
            let range = Range {
                start_line_number: self.line_count_with_offset(),
                start_column: js_index_of(self.last_line(), &line) + 1,
                end_line_number: self.line_count_with_offset(),
                end_column: js_length(self.last_line()),
            };
            setting.description_ranges.push(range);
        }

        if let (Some(enum_values), Some(enum_descriptions)) = (&setting.r#enum, &setting.enum_descriptions)
            && enum_descriptions.iter().any(|desc| !desc.is_empty())
        {
            for (i, desc) in enum_descriptions.iter().enumerate() {
                let display_enum = escape_invisible_chars(enum_values.get(i).map_or("undefined", String::as_str));
                let line = if desc.is_empty() { display_enum } else { format!("{display_enum}: {}", fix_setting_links(desc)) };

                let mut lines: Vec<String> = line.split('\n').map(str::to_owned).collect();
                lines[0] = format!(" - {}", lines[0]);
                self.content_by_lines.extend(lines.iter().map(|l| format!("{indent}// {l}")));

                let range = Range {
                    start_line_number: self.line_count_with_offset(),
                    start_column: js_index_of(self.last_line(), &line) + 1,
                    end_line_number: self.line_count_with_offset(),
                    end_column: js_length(self.last_line()),
                };
                setting.description_ranges.push(range);
            }
        }
    }

    fn push_value(&mut self, setting: &mut ISetting, pre_value_content: &str, indent: &str) {
        let value_string = json_stringify_indented(&setting.value, indent.len());
        let is_object = matches!(setting.value, Dynamic::Object(_) | Dynamic::Array(_) | Dynamic::Null);
        match value_string {
            Some(value_string) if is_object => {
                if let Some(overrides) = setting.overrides.as_mut().filter(|overrides| !overrides.is_empty()) {
                    self.content_by_lines.push(format!("{pre_value_content} {{"));
                    let sub_indent = format!("{indent}{indent}");
                    for sub_setting in overrides.iter_mut() {
                        self.push_setting(sub_setting, &sub_indent);
                        self.content_by_lines.pop();
                    }
                    let last_end_line = overrides[overrides.len() - 1].range.end_line_number;
                    self.strip_trailing_comma(last_end_line - 2);
                    self.content_by_lines.push(format!("{indent}}}"));
                } else {
                    let mulit_line_value: Vec<&str> = value_string.split('\n').collect();
                    self.content_by_lines.push(format!("{pre_value_content}{}", mulit_line_value[0]));
                    for line in &mulit_line_value[1..] {
                        self.content_by_lines.push(format!("{indent}{line}"));
                    }
                }
            }
            Some(value_string) => self.content_by_lines.push(format!("{pre_value_content}{value_string}")),
            None => self.content_by_lines.push(format!("{pre_value_content}undefined")),
        }
    }

    fn add_description(&mut self, description: &[String], indent: &str) {
        for line in description {
            self.content_by_lines.push(format!("{indent}// {line}"));
        }
    }
}

/// O `string` de um valor dinâmico, `None` quando não é texto.
fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// O `boolean` de um valor dinâmico, `None` quando não é booleano (`undefined`).
fn bool_of(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

/// `string[]` (cada item pelo `String(item)`), `None` quando o valor não é uma lista.
fn text_list_of(value: &Dynamic) -> Option<Vec<String>> {
    match value {
        Dynamic::Array(items) => Some(items.to_vec().iter().map(|item| to_js_string(item).to_string_lossy()).collect()),
        _ => None,
    }
}

/// `value` quando definido.
fn defined(value: Dynamic) -> Option<Dynamic> {
    if value.is_undefined() { None } else { Some(value) }
}

/// O `type` de um esquema: um texto ou uma lista de textos.
fn schema_type_of(value: &Dynamic) -> Option<SettingSchemaType> {
    match value {
        Dynamic::String(text) => Some(SettingSchemaType::Single(text.to_string_lossy())),
        Dynamic::Array(_) => text_list_of(value).map(SettingSchemaType::List),
        _ => None,
    }
}

/// `DefaultSettings`.
pub struct DefaultSettings {
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<()>>,

    all_settings_groups: RefCell<Option<Vec<ISettingsGroup>>>,
    content: RefCell<Option<String>>,
    content_without_most_commonly_used: RefCell<Option<String>>,
    settings_by_name: RefCell<HashMap<String, Rc<ISetting>>>,

    most_commonly_used_settings_keys: Vec<String>,
    pub target: ConfigurationTarget,
    pub configuration_service: Rc<dyn IConfigurationService>,
}

impl DefaultSettings {
    pub fn new(most_commonly_used_settings_keys: Vec<String>, target: ConfigurationTarget, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let this = Rc::new(Self {
            base,
            store,
            on_did_change,
            all_settings_groups: RefCell::new(None),
            content: RefCell::new(None),
            content_without_most_commonly_used: RefCell::new(None),
            settings_by_name: RefCell::new(HashMap::new()),
            most_commonly_used_settings_keys,
            target,
            configuration_service: configuration_service.clone(),
        });
        let weak: Weak<Self> = Rc::downgrade(&this);
        configuration_service.on_did_change_configuration().listen_in(
            move |event| {
                if event.source.get() == Some(ConfigurationTarget::Default)
                    && let Some(this) = weak.upgrade()
                {
                    this.reset();
                    this.on_did_change.fire(());
                }
            },
            &this.store,
        );
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `getContent(forceUpdate = false)`.
    pub fn get_content(&self, force_update: bool) -> String {
        if self.content.borrow().as_deref().is_none_or(str::is_empty) || force_update {
            self.initialize();
        }

        self.content.borrow().clone().unwrap_or_default()
    }

    /// `getContentWithoutMostCommonlyUsed(forceUpdate = false)`.
    pub fn get_content_without_most_commonly_used(&self, force_update: bool) -> String {
        if self.content_without_most_commonly_used.borrow().as_deref().is_none_or(str::is_empty) || force_update {
            self.initialize();
        }

        self.content_without_most_commonly_used.borrow().clone().unwrap_or_default()
    }

    /// `getSettingsGroups(forceUpdate = false)`.
    pub fn get_settings_groups(&self, force_update: bool) -> Vec<ISettingsGroup> {
        if self.all_settings_groups.borrow().is_none() || force_update {
            self.initialize();
        }

        self.all_settings_groups.borrow().clone().unwrap_or_default()
    }

    fn initialize(&self) {
        let mut all_settings_groups = self.parse();
        let content = self.to_content(&mut all_settings_groups, 0);
        let content_without_most_commonly_used = self.to_content(&mut all_settings_groups, 1);
        *self.all_settings_groups.borrow_mut() = Some(all_settings_groups);
        *self.content.borrow_mut() = Some(content);
        *self.content_without_most_commonly_used.borrow_mut() = Some(content_without_most_commonly_used);
    }

    fn reset(&self) {
        *self.content.borrow_mut() = None;
        *self.content_without_most_commonly_used.borrow_mut() = None;
        *self.all_settings_groups.borrow_mut() = None;
    }

    fn parse(&self) -> Vec<ISettingsGroup> {
        let settings_groups = self.get_registered_groups();
        self.init_all_settings_map(&settings_groups);
        let most_commonly_used = self.get_most_commonly_used_settings();
        let mut groups = vec![most_commonly_used];
        groups.extend(settings_groups);
        groups
    }

    /// `getRegisteredGroups()`.
    pub fn get_registered_groups(&self) -> Vec<ISettingsGroup> {
        let registry = configuration_registry();
        let mut all_configurations: Vec<(JsString, PropertySchema)> = registry.get_configuration_properties().entries();
        let excluded_configurations = registry.get_excluded_configuration_properties();

        for policy_key in self.configuration_service.keys().policy {
            if let Some(policy_configuration) = excluded_configurations.get(&policy_key) {
                match all_configurations.iter_mut().find(|(key, _)| *key == policy_key) {
                    Some(slot) => slot.1 = policy_configuration,
                    None => all_configurations.push((policy_key, policy_configuration)),
                }
            }
        }

        let mut parsed = self.parse_properties(&all_configurations);
        sort_by_compare(&mut parsed, Self::compare_groups);
        let groups = self.remove_empty_settings_groups(parsed);
        self.sort_groups(groups)
    }

    fn sort_groups(&self, mut groups: Vec<ISettingsGroup>) -> Vec<ISettingsGroup> {
        for group in &mut groups {
            for section in &mut group.sections {
                sort_by_compare(&mut section.settings, |a, b| match locale_compare(&a.key, &b.key) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                });
            }
        }

        groups
    }

    fn init_all_settings_map(&self, all_settings_groups: &[ISettingsGroup]) {
        let mut settings_by_name = HashMap::new();
        for group in all_settings_groups {
            for section in &group.sections {
                for setting in &section.settings {
                    settings_by_name.insert(setting.key.clone(), setting.clone());
                }
            }
        }
        *self.settings_by_name.borrow_mut() = settings_by_name;
    }

    fn get_most_commonly_used_settings(&self) -> ISettingsGroup {
        let settings_by_name = self.settings_by_name.borrow();
        let settings: Vec<Rc<ISetting>> = self
            .most_commonly_used_settings_keys
            .iter()
            .filter_map(|key| {
                settings_by_name.get(key).map(|setting| {
                    Rc::new(ISetting {
                        description: setting.description.clone(),
                        key: setting.key.clone(),
                        value: setting.value.clone(),
                        key_range: NULL_RANGE,
                        range: NULL_RANGE,
                        value_range: NULL_RANGE,
                        overrides: Some(Vec::new()),
                        scope: Some(ConfigurationScope::Resource),
                        r#type: setting.r#type.clone(),
                        r#enum: setting.r#enum.clone(),
                        enum_descriptions: setting.enum_descriptions.clone(),
                        description_ranges: Vec::new(),
                        ..ISetting::default()
                    })
                })
            })
            .collect();

        ISettingsGroup {
            id: "mostCommonlyUsed".to_owned(),
            range: NULL_RANGE,
            title: localize("commonlyUsed", "Commonly Used", &[]),
            title_range: NULL_RANGE,
            sections: vec![ISettingsSection { title: None, settings }],
            order: None,
            extension_info: None,
        }
    }

    fn parse_properties(&self, properties: &[(JsString, PropertySchema)]) -> Vec<ISettingsGroup> {
        let mut result: Vec<ISettingsGroup> = Vec::new();
        let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_id: HashMap<String, Vec<usize>> = HashMap::new();
        for (key, property) in properties {
            let Some(section) = property.section() else {
                continue;
            };
            let extension_id = section.extension_info.as_ref().map(|info| info.id.clone());
            let title = section.title.clone().filter(|title| !title.is_empty());
            let id = section.id.clone().filter(|id| !id.is_empty());

            let mut settings_group: Option<usize> = None;

            if let Some(title) = &title
                && let Some(groups) = by_title.get(title)
            {
                settings_group = groups.iter().copied().find(|&index| result[index].extension_info.as_ref().map(|info| &info.id) == extension_id.as_ref());
            }

            if settings_group.is_none()
                && let Some(id) = &id
            {
                if let Some(groups) = by_id.get(id) {
                    settings_group = groups.iter().copied().find(|&index| result[index].extension_info.as_ref().map(|info| &info.id) == extension_id.as_ref() && result[index].title.is_empty());
                }
                if let Some(index) = settings_group
                    && result[index].title.is_empty()
                    && let Some(title) = &title
                {
                    result[index].title = title.clone();
                    by_title.entry(title.clone()).or_default().push(index);
                }
            }

            let group_index = match settings_group {
                Some(index) => index,
                None => {
                    result.push(ISettingsGroup {
                        sections: vec![ISettingsSection { title: section.title.clone(), settings: Vec::new() }],
                        id: section.id.clone().filter(|id| !id.is_empty()).unwrap_or_default(),
                        title: section.title.clone().unwrap_or_default(),
                        title_range: NULL_RANGE,
                        order: section.order,
                        range: NULL_RANGE,
                        extension_info: match property.source() {
                            Some(ConfigurationDefaultSource::Extension(info)) => Some(info),
                            _ => None,
                        },
                    });
                    let index = result.len() - 1;
                    if let Some(title) = &title {
                        by_title.entry(title.clone()).or_default().push(index);
                    }
                    if let Some(id) = &id {
                        by_id.entry(id.clone()).or_default().push(index);
                    }
                    index
                }
            };

            if let Some(setting) = self.parse_setting(key, property) {
                result[group_index].sections[0].settings.push(Rc::new(setting));
            }
        }
        result
    }

    fn remove_empty_settings_groups(&self, settings_groups: Vec<ISettingsGroup>) -> Vec<ISettingsGroup> {
        let mut result = Vec::new();
        for mut settings_group in settings_groups {
            settings_group.sections.retain(|section| !section.settings.is_empty());
            if !settings_group.sections.is_empty() {
                result.push(settings_group);
            }
        }
        result
    }

    fn parse_setting(&self, key: &JsString, prop: &PropertySchema) -> Option<ISetting> {
        if !self.matches_scope(prop) {
            return None;
        }

        let value = prop.default_value();
        let markdown_description = prop.get("markdownDescription");
        let description_value = if markdown_description.is_truthy() {
            markdown_description.clone()
        } else {
            let description = prop.get("description");
            if description.is_truthy() { description } else { Dynamic::string("") }
        };
        let description = text_of(&description_value).unwrap_or_default();
        let description_lines: Vec<String> = description.split('\n').map(str::to_owned).collect();
        let is_override = is_override_property(key);
        let overrides = if is_override { self.parse_override_settings(&prop.default_value()) } else { Vec::new() };

        let type_value = prop.get("type");
        let is_object_type = matches!(&type_value, Dynamic::String(text) if *text == "object");
        let items = prop.get("items");
        let mut list_item_type: Option<String> = None;
        if matches!(&type_value, Dynamic::String(text) if *text == "array")
            && items.is_truthy()
            && !matches!(items, Dynamic::Array(_))
        {
            let items_type = items.get_str("type");
            if items_type.is_truthy() {
                if items.get_str("enum").is_truthy() {
                    list_item_type = Some("enum".to_owned());
                } else if !matches!(items_type, Dynamic::Array(_)) {
                    list_item_type = text_of(&items_type);
                }
            }
        }

        let object_properties = if is_object_type { defined(prop.get("properties")) } else { None };
        let object_pattern_properties = if is_object_type { defined(prop.get("patternProperties")) } else { None };
        let object_additional_properties = if is_object_type { defined(prop.get("additionalProperties")) } else { None };
        let property_names = if is_object_type { defined(prop.get("propertyNames")) } else { None };

        let markdown_enum_descriptions = prop.get("markdownEnumDescriptions");
        let mut enum_to_use = text_list_of(&prop.get("enum"));
        let mut enum_descriptions = text_list_of(&if markdown_enum_descriptions.is_nullish() { prop.get("enumDescriptions") } else { markdown_enum_descriptions.clone() });
        let mut enum_descriptions_are_markdown = markdown_enum_descriptions.is_truthy();
        if list_item_type.as_deref() == Some("enum") && !matches!(items, Dynamic::Array(_)) {
            let items_markdown_enum_descriptions = items.get_str("markdownEnumDescriptions");
            enum_to_use = text_list_of(&items.get_str("enum"));
            enum_descriptions = text_list_of(&if items_markdown_enum_descriptions.is_nullish() { items.get_str("enumDescriptions") } else { items_markdown_enum_descriptions.clone() });
            enum_descriptions_are_markdown = items_markdown_enum_descriptions.is_truthy();
        }

        let mut all_keys_are_boolean = false;
        let properties = prop.get("properties");
        if is_object_type && !prop.get("additionalProperties").is_truthy() && properties.is_truthy() {
            let property_keys = object_keys(&properties);
            if !property_keys.is_empty() {
                all_keys_are_boolean = property_keys.iter().all(|property_key| matches!(properties.get(property_key).get_str("type"), Dynamic::String(text) if text == "boolean"));
            }
        }

        let is_language_tag_setting = is_override;

        let non_language_specific_default_value_source = if is_language_tag_setting { None } else { prop.default_value_source() };

        let source_extension_info = match prop.source() {
            Some(ConfigurationDefaultSource::Extension(info)) => Some(info),
            _ => None,
        };
        let markdown_deprecation_message = prop.get("markdownDeprecationMessage");
        let deprecation_message_value = if markdown_deprecation_message.is_truthy() { markdown_deprecation_message.clone() } else { prop.get("deprecationMessage") };
        let section = prop.section();
        let section_id = section.as_ref().and_then(|section| section.id.clone());
        let category_label =
            if source_extension_info.as_ref().map(|info| info.id.clone()) == section_id { text_of(&prop.get("title")) } else { section_id };

        let schema = prop.json();
        let validator = create_validator(&schema);
        let validator: SettingValidator = Rc::new(move |value| {
            let message = validator(value);
            if message.is_empty() { None } else { Some(message) }
        });

        Some(ISetting {
            key: key.to_string_lossy(),
            value,
            description: description_lines,
            description_is_markdown: Some(markdown_description.is_truthy()),
            keywords: text_list_of(&prop.get("keywords")),
            range: NULL_RANGE,
            key_range: NULL_RANGE,
            value_range: NULL_RANGE,
            description_ranges: Vec::new(),
            overrides: Some(overrides),
            scope: prop.scope(),
            r#type: schema_type_of(&type_value),
            array_item_type: list_item_type,
            object_properties,
            object_pattern_properties,
            object_additional_properties,
            property_names,
            r#enum: enum_to_use,
            enum_descriptions,
            enum_descriptions_are_markdown: Some(enum_descriptions_are_markdown),
            enum_item_labels: text_list_of(&prop.get("enumItemLabels")),
            unique_items: bool_of(&prop.get("uniqueItems")),
            tags: text_list_of(&prop.get("tags")),
            disallow_sync_ignore: bool_of(&prop.get("disallowSyncIgnore")),
            restricted: bool_of(&prop.get("restricted")),
            extension_info: source_extension_info,
            deprecation_message: text_of(&deprecation_message_value),
            deprecation_message_is_markdown: Some(markdown_deprecation_message.is_truthy()),
            deprecation_message_severity: text_of(&prop.get("deprecationMessageSeverity")),
            deprecation_message_show_in_settings: bool_of(&prop.get("deprecationMessageShowInSettings")),
            validator: Some(validator),
            all_keys_are_boolean: Some(all_keys_are_boolean),
            edit_presentation: text_of(&prop.get("editPresentation")),
            order: match prop.get("order") {
                Dynamic::Number(order) => Some(order),
                _ => None,
            },
            non_language_specific_default_value_source,
            is_language_tag_setting: Some(is_language_tag_setting),
            category_label,
            ..ISetting::default()
        })
    }

    fn parse_override_settings(&self, override_settings: &Dynamic) -> Vec<ISetting> {
        object_keys(override_settings)
            .into_iter()
            .map(|key| ISetting {
                value: override_settings.get(&key),
                key: key.to_string_lossy(),
                description: Vec::new(),
                description_is_markdown: Some(false),
                range: NULL_RANGE,
                key_range: NULL_RANGE,
                value_range: NULL_RANGE,
                description_ranges: Vec::new(),
                overrides: Some(Vec::new()),
                ..ISetting::default()
            })
            .collect()
    }

    fn matches_scope(&self, property: &PropertySchema) -> bool {
        let Some(scope) = property.scope() else {
            return true;
        };
        if self.target == ConfigurationTarget::WorkspaceFolder {
            return FOLDER_SCOPES.contains(&scope);
        }
        if self.target == ConfigurationTarget::Workspace {
            return WORKSPACE_SCOPES.contains(&scope);
        }
        true
    }

    fn compare_groups(c1: &ISettingsGroup, c2: &ISettingsGroup) -> f64 {
        let Some(order1) = c1.order else {
            return 1.0;
        };
        let Some(order2) = c2.order else {
            return -1.0;
        };
        if order1 == order2 {
            return match locale_compare(&c1.title, &c2.title) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            };
        }
        order1 - order2
    }

    fn to_content(&self, settings_groups: &mut [ISettingsGroup], start_index: usize) -> String {
        let mut builder = SettingsContentBuilder::new(0);
        let length = settings_groups.len();
        for i in start_index..length {
            builder.push_group(&mut settings_groups[i], i == start_index, i == length - 1);
        }
        builder.get_content()
    }
}

impl IDisposable for DefaultSettings {
    fn dispose(&self) {
        self.base.dispose();
    }
}
