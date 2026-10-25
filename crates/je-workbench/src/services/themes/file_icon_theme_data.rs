//! Porte de `src/vs/workbench/services/themes/browser/fileIconThemeData.ts`: o `FileIconThemeData`, o tema de ícones
//! de arquivo do workbench (identidade, flags, folha de estilo gerada, persistência) e o `FileIconThemeLoader`, que
//! lê o documento do tema (`icon-theme.json`) e o converte nas regras CSS (`.show-file-icons ... ::before`) de
//! ícones de imagem (`background-image`, ou máscara com `currentColor`) e de fonte (`@font-face`).
//!
//! Reaproveitado, não duplicado: o contrato de leitura de recursos é o [`IExtensionResourceLoaderService`] de
//! `contrib::snippets::snippets_file`; o `asCSSUrl`, o `CSS.escape` e os valores seguros de CSS são de
//! `je_editor::services::css_value`; `fileIconSelectorEscape` é de `je_editor::get_icon_classes`; o JSON do tema é lido
//! com o `parse` de `je_base::json` e convertido com o `to_serde` de `color_theme_data`.
//!
//! Divergências:
//! - `fromExtensionTheme` e o campo `extensionData` (inclusive a gravação e a leitura no armazenamento) dependem de
//!   `IThemeExtensionPoint` e `ExtensionData`, ainda sem porte.
//! - `noIconTheme` é uma função que devolve um tema novo a cada chamada (o original guarda um singleton mutável).
//! - `FileAccess.uriToBrowserUri` é o campo `uri_to_browser_uri` do carregador (identidade por padrão; o chamador troca
//!   com [`FileIconThemeLoader::with_uri_to_browser_uri`]), como no `as_css_url` de `css_value`.
//! - O `console.warn` dos valores seguros de CSS não existe.
//! - `load` e `ensureLoaded` são `async fn` que devolvem `Result` (a rejeição da `Promise` do original). Propriedades
//!   que o original leria de `undefined` (um `fonts` vazio, uma fonte sem `src`, `weight` ou `style`) lançariam
//!   `TypeError` e viram um [`Error`] de mesma mensagem.
//! - Associações com valores que não são texto são tratadas como definição vazia (não geram seletor, mas ligam as
//!   flags `hasFileIcons` e `hasFolderIcons`, como o `for ... in` do original).

use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::errors::Error;
use je_base::json::{self as js_json, NodeType, ParseOptions};
use je_base::json_error_messages::parse_error_message;
use je_base::nls::localize;
use je_base::uri::URI;
use je_editor::get_icon_classes::file_icon_selector_escape;
use je_editor::language_service::ILanguageService;
use je_editor::services::css_value::{
    as_css_url, class_name, hex_color_value, ident_value, size_value, string_value,
};
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use serde_json::{Map, Value};

use super::color_theme_data::{is_truthy, to_serde};
use crate::contrib::snippets::snippets_file::IExtensionResourceLoaderService;

/// `FileIconThemeData`.
#[derive(Clone, Debug)]
pub struct FileIconThemeData {
    pub id: String,
    pub label: String,
    pub settings_id: Option<String>,
    pub description: Option<String>,
    pub has_file_icons: bool,
    pub has_folder_icons: bool,
    pub hides_explorer_arrows: bool,
    pub is_loaded: bool,
    pub location: Option<URI>,
    pub watch: Option<bool>,
    pub style_sheet_content: Option<String>,
}

impl FileIconThemeData {
    pub const STORAGE_KEY: &'static str = "iconThemeData";

    fn new(id: &str, label: &str, settings_id: Option<String>) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            settings_id,
            description: None,
            has_file_icons: false,
            has_folder_icons: false,
            hides_explorer_arrows: false,
            is_loaded: false,
            location: None,
            watch: None,
            style_sheet_content: None,
        }
    }

    /// `ensureLoaded`.
    pub async fn ensure_loaded(&mut self, theme_loader: &FileIconThemeLoader) -> Result<Option<String>, Error> {
        if self.is_loaded { Ok(self.style_sheet_content.clone()) } else { theme_loader.load(self).await }
    }

    /// `reload`.
    pub async fn reload(&mut self, theme_loader: &FileIconThemeLoader) -> Result<Option<String>, Error> {
        theme_loader.load(self).await
    }

    /// `noIconTheme` (ver as divergências do módulo).
    pub fn no_icon_theme() -> Self {
        let mut theme_data = Self::new("", "", None);
        theme_data.is_loaded = true;
        theme_data.watch = Some(false);
        theme_data
    }

    /// `createUnloadedTheme`.
    pub fn create_unloaded_theme(id: &str) -> Self {
        let mut theme_data = Self::new(id, "", Some(format!("__{id}")));
        theme_data.watch = Some(false);
        theme_data
    }

    /// `fromStorageData`: o tema gravado por [`FileIconThemeData::to_storage`], ou `None` se não há ou não é válido.
    pub fn from_storage_data(storage_service: &dyn StorageService) -> Option<Self> {
        let input = storage_service.get(Self::STORAGE_KEY, StorageScope::Profile, None).filter(|input| !input.is_empty())?;
        let data: Value = serde_json::from_str(&input).ok()?;
        let data = data.as_object()?;
        let mut theme = Self::new("", "", None);
        for (key, value) in data {
            let text = || value.as_str().map(str::to_owned);
            let flag = || value.as_bool();
            match key.as_str() {
                "id" => theme.id = text().unwrap_or_default(),
                "label" => theme.label = text().unwrap_or_default(),
                "description" => theme.description = text(),
                "settingsId" => theme.settings_id = text(),
                "styleSheetContent" => theme.style_sheet_content = text(),
                "hasFileIcons" => theme.has_file_icons = flag().unwrap_or_default(),
                "hidesExplorerArrows" => theme.hides_explorer_arrows = flag().unwrap_or_default(),
                "hasFolderIcons" => theme.has_folder_icons = flag().unwrap_or_default(),
                "watch" => theme.watch = flag(),
                // `location`: ignorado, não é mais restaurado.
                _ => {}
            }
        }
        Some(theme)
    }

    /// `toStorage`: as chaves sem valor (`undefined` no original) ficam de fora do JSON.
    pub fn to_storage(&self, storage_service: &dyn StorageService) {
        let mut data = Map::new();
        data.insert("id".into(), Value::String(self.id.clone()));
        data.insert("label".into(), Value::String(self.label.clone()));
        if let Some(description) = &self.description {
            data.insert("description".into(), Value::String(description.clone()));
        }
        data.insert("settingsId".into(), self.settings_id.clone().map_or(Value::Null, Value::String));
        if let Some(content) = &self.style_sheet_content {
            data.insert("styleSheetContent".into(), Value::String(content.clone()));
        }
        data.insert("hasFileIcons".into(), Value::Bool(self.has_file_icons));
        data.insert("hasFolderIcons".into(), Value::Bool(self.has_folder_icons));
        data.insert("hidesExplorerArrows".into(), Value::Bool(self.hides_explorer_arrows));
        if let Some(watch) = self.watch {
            data.insert("watch".into(), Value::Bool(watch));
        }
        storage_service.store(
            Self::STORAGE_KEY,
            StorageValue::String(Value::Object(data).to_string()),
            StorageScope::Profile,
            StorageTarget::Machine,
        );
    }
}

/// O que `processIconThemeDocument` devolve.
struct ProcessedTheme {
    content: String,
    has_file_icons: bool,
    has_folder_icons: bool,
    hides_explorer_arrows: bool,
}

/// `FileIconThemeLoader`.
pub struct FileIconThemeLoader {
    file_service: Rc<dyn IExtensionResourceLoaderService>,
    language_service: Rc<dyn ILanguageService>,
    uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>,
}

impl FileIconThemeLoader {
    pub fn new(file_service: Rc<dyn IExtensionResourceLoaderService>, language_service: Rc<dyn ILanguageService>) -> Self {
        Self { file_service, language_service, uri_to_browser_uri: Rc::new(URI::clone) }
    }

    /// Troca o `FileAccess.uriToBrowserUri` (ver as divergências do módulo).
    pub fn with_uri_to_browser_uri(mut self, uri_to_browser_uri: Rc<dyn Fn(&URI) -> URI>) -> Self {
        self.uri_to_browser_uri = uri_to_browser_uri;
        self
    }

    /// `load`: lê o documento do tema, grava o resultado em `data` e devolve a folha de estilo.
    pub async fn load(&self, data: &mut FileIconThemeData) -> Result<Option<String>, Error> {
        let Some(location) = data.location.clone() else {
            return Ok(data.style_sheet_content.clone());
        };
        let icon_theme_document = self.load_icon_theme_document(&location).await?;
        let result = self.process_icon_theme_document(&location, &icon_theme_document)?;
        data.style_sheet_content = Some(result.content);
        data.has_file_icons = result.has_file_icons;
        data.has_folder_icons = result.has_folder_icons;
        data.hides_explorer_arrows = result.hides_explorer_arrows;
        data.is_loaded = true;
        Ok(data.style_sheet_content.clone())
    }

    async fn load_icon_theme_document(&self, location: &URI) -> Result<Value, Error> {
        let content = self.file_service.read_extension_resource(location).await?;
        let mut errors = Vec::new();
        let units: Vec<u16> = content.encode_utf16().collect();
        let parsed = js_json::parse(&units, &mut errors, ParseOptions::default());
        if !errors.is_empty() {
            let messages: Vec<String> = errors.iter().map(|e| parse_error_message(e.error)).collect();
            return Err(Error::new(localize(
                "error.cannotparseicontheme",
                "Problems parsing file icons file: {0}",
                &[&messages.join(", ")],
            )));
        }
        match parsed.as_ref().map(|value| (js_json::node_type(value), value)) {
            Some((NodeType::Object, value)) => Ok(to_serde(value)),
            _ => Err(Error::new(localize(
                "error.invalidformat",
                "Invalid format for file icons theme file: Object expected.",
                &[],
            ))),
        }
    }

    fn process_icon_theme_document(&self, location: &URI, document: &Value) -> Result<ProcessedTheme, Error> {
        let mut result = ProcessedTheme {
            content: String::new(),
            has_file_icons: false,
            has_folder_icons: false,
            hides_explorer_arrows: document.get("hidesExplorerArrows").is_some_and(is_truthy),
        };

        if !document.get("iconDefinitions").is_some_and(is_truthy) {
            return Ok(result);
        }

        let mut collector = SelectorCollector::default();
        collector.collect(Some(document), None);
        collector.collect(document.get("light"), Some(".vs"));
        collector.collect(document.get("highContrast"), Some(".hc-black"));
        collector.collect(document.get("highContrast"), Some(".hc-light"));
        result.has_file_icons = collector.has_file_icons;
        result.has_folder_icons = collector.has_folder_icons;

        if !result.has_file_icons && !result.has_folder_icons {
            return Ok(result);
        }

        let show_language_mode_icons = document.get("showLanguageModeIcons") == Some(&Value::Bool(true))
            || (collector.has_specific_file_icons && document.get("showLanguageModeIcons") != Some(&Value::Bool(false)));

        let dirname = ext_uri().dirname(location);
        let resolve_path = |path: &str| ext_uri().join_path(&dirname, &[path]);
        let css_url = |uri: &URI| as_css_url(Some(uri), &*self.uri_to_browser_uri);

        let mut css_rules: Vec<String> = Vec::new();

        let mut font_sizes: IndexMap<String, String> = IndexMap::new();
        if let Some(fonts) = document.get("fonts").and_then(Value::as_array) {
            let first_font = fonts.first().ok_or_else(|| type_error("size"))?;
            let default_font_size = try_normalize_font_size(first_font.get("size").and_then(Value::as_str))
                .filter(|size| !size.is_empty())
                .unwrap_or_else(|| "150%".to_owned());
            for font in fonts {
                let sources = font.get("src").and_then(Value::as_array).ok_or_else(|| type_error("map"))?;
                let font_srcs: Vec<String> = sources
                    .iter()
                    .map(|source| {
                        let path = source.get("path").and_then(Value::as_str).unwrap_or_default();
                        let format = source.get("format").and_then(Value::as_str).unwrap_or_default();
                        format!("{} format({})", css_url(&resolve_path(path)), string_value(format))
                    })
                    .collect();
                let id = text_of(font, "id");
                css_rules.push(format!(
                    "@font-face {{ src: {}; font-family: {}; font-weight: {}; font-style: {}; font-display: block; }}",
                    font_srcs.join(", "),
                    string_value(&id),
                    ident_value(&required_text(font, "weight")?),
                    ident_value(&required_text(font, "style")?),
                ));

                let font_size = try_normalize_font_size(font.get("size").and_then(Value::as_str));
                if let Some(font_size) = font_size.filter(|size| *size != default_font_size) {
                    font_sizes.insert(id, font_size);
                }
            }
            css_rules.push(format!(
                ".show-file-icons .file-icon::before, .show-file-icons .folder-icon::before, .show-file-icons .rootfolder-icon::before {{ font-family: {}; font-size: {}; }}",
                string_value(&text_of(first_font, "id")),
                size_value(&default_font_size),
            ));
        }

        // Use emQuads to prevent the icon from collapsing to zero height for image icons
        let em_quad = string_value("\\2001");

        // When usesCurrentColor is set, image icons are rendered as masks filled with the current text color
        let uses_current_color = document.get("usesCurrentColor").is_some_and(is_truthy);
        let image_icon_style = |icon_path: &str| -> String {
            if uses_current_color {
                format!(
                    "background-color: currentColor; background-image: none; mask: {icon_path} no-repeat var(--file-icon-mask-position, left center); mask-size: var(--file-icon-mask-size, 16px); -webkit-mask: {icon_path} no-repeat var(--file-icon-mask-position, left center); -webkit-mask-size: var(--file-icon-mask-size, 16px);"
                )
            } else {
                format!("background-image: {icon_path};")
            }
        };

        let icon_definitions = document.get("iconDefinitions");
        for (def_id, selectors) in &collector.selector_by_definition_id {
            let Some(definition) = icon_definitions.and_then(|definitions| definitions.get(def_id)).filter(|d| is_truthy(d)) else {
                continue;
            };
            if let Some(icon_path) = truthy_text(definition, "iconPath") {
                let icon_path = css_url(&resolve_path(icon_path));
                css_rules.push(format!("{} {{ content: {em_quad}; {} }}", selectors.join(", "), image_icon_style(&icon_path)));
            } else if truthy_text(definition, "fontCharacter").is_some() || truthy_text(definition, "fontColor").is_some() {
                let mut body: Vec<String> = Vec::new();
                if let Some(font_color) = truthy_text(definition, "fontColor").filter(|color| matches_font_color(color)) {
                    body.push(format!("color: {};", hex_color_value(font_color)));
                }
                if let Some(font_character) = truthy_text(definition, "fontCharacter") {
                    body.push(format!("content: {};", string_value(font_character)));
                }
                let font_id = truthy_text(definition, "fontId");
                // `??`: um `fontSize` presente (mesmo vazio) vence o tamanho da fonte.
                let font_size = match definition.get("fontSize") {
                    Some(Value::String(size)) => Some(size.as_str()),
                    _ => font_id.and_then(|id| font_sizes.get(id)).map(String::as_str),
                };
                if let Some(font_size) = font_size.filter(|size| matches_font_size(size)) {
                    body.push(format!("font-size: {};", size_value(font_size)));
                }
                if let Some(font_id) = font_id {
                    body.push(format!("font-family: {};", string_value(font_id)));
                }
                if show_language_mode_icons {
                    body.push("background-image: unset;".to_owned()); // potentially set by the language default
                }
                css_rules.push(format!("{} {{ {} }}", selectors.join(", "), body.join(" ")));
            }
        }

        if show_language_mode_icons {
            for language_id in self.language_service.get_registered_language_ids() {
                if collector.covered_languages.contains(&language_id) {
                    continue;
                }
                if let Some(icon) = self.language_service.get_icon(&language_id) {
                    let selector = format!(".show-file-icons .{}-lang-file-icon.file-icon::before", class_selector_part(&language_id));
                    css_rules.push(format!("{selector} {{ content: {em_quad}; {} }}", image_icon_style(&css_url(&icon.dark))));
                    css_rules.push(format!(".vs {selector} {{ content: {em_quad}; {} }}", image_icon_style(&css_url(&icon.light))));
                }
            }
        }

        result.content = css_rules.join("\n");
        Ok(result)
    }
}

/// O `TypeError` de ler uma propriedade de `undefined`.
fn type_error(property: &str) -> Error {
    Error::new(format!("Cannot read properties of undefined (reading '{property}')"))
}

/// Uma propriedade de texto de um objeto do documento (o que o original passaria adiante como `undefined` vira vazio).
fn text_of(object: &Value, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Uma propriedade de texto que o original usaria sem checar (`undefined` lançaria `TypeError`).
fn required_text(object: &Value, key: &str) -> Result<String, Error> {
    object.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| type_error("replaceAll"))
}

/// Uma propriedade de texto verdadeira (não vazia), o `if (object.key)` do original.
fn truthy_text<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// `fontColorRegex`: `/^#[0-9a-fA-F]{0,6}$/`.
fn matches_font_color(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|digits| digits.len() <= 6 && digits.chars().all(|ch| ch.is_ascii_hexdigit()))
}

/// `fontSizeRegex`: `/^([\w_.%+-]+)$/`.
fn matches_font_size(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '%' | '+' | '-'))
}

/// `tryNormalizeFontSize`: tenta converter tamanhos absolutos em valores relativos, que escalam melhor onde são usados.
fn try_normalize_font_size(size: Option<&str>) -> Option<String> {
    let size = size.filter(|size| !size.is_empty())?;
    let default_font_size_in_px = 13.0;
    if size.ends_with("px") {
        if let Some(value) = parse_int_prefix(size) {
            return Some(format!("{}%", ((value / default_font_size_in_px) * 100.0 + 0.5).floor()));
        }
    }
    Some(size.to_owned())
}

/// `parseInt(text, 10)`: o prefixo inteiro (com sinal) depois dos espaços, ou `None` (o `NaN`).
fn parse_int_prefix(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = digits.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<f64>().ok().map(|value| sign * value)
}

/// `classSelectorPart`.
fn class_selector_part(text: &str) -> String {
    class_name(&file_icon_selector_escape(text))
}

/// `handleParentFolder`: a pasta-pai de uma chave `pasta/nome` vira um seletor `-name-dir-icon`; devolve o nome.
fn handle_parent_folder(key: &str, selectors: &mut Vec<String>) -> String {
    match key.rfind('/') {
        Some(last_index_of_slash) => {
            selectors.push(format!(".{}-name-dir-icon", class_selector_part(&key[..last_index_of_slash])));
            key[last_index_of_slash + 1..].to_owned()
        }
        None => key.to_owned(),
    }
}

/// Os pares `chave: definição` de uma associação (objeto), na ordem do documento.
fn entries(associations: &Value, key: &str) -> Vec<(String, String)> {
    associations
        .get(key)
        .and_then(Value::as_object)
        .map(|object| object.iter().map(|(name, def)| (name.clone(), def.as_str().unwrap_or_default().to_owned())).collect())
        .unwrap_or_default()
}

/// O `collectSelectors` do original, com o estado que ele compartilha por fechamento.
#[derive(Default)]
struct SelectorCollector {
    selector_by_definition_id: IndexMap<String, Vec<String>>,
    covered_languages: HashSet<String>,
    has_file_icons: bool,
    has_folder_icons: bool,
    has_specific_file_icons: bool,
}

impl SelectorCollector {
    fn add_selector(&mut self, selector: String, def_id: &str) {
        if !def_id.is_empty() {
            self.selector_by_definition_id.entry(def_id.to_owned()).or_default().push(selector);
        }
    }

    fn collect(&mut self, associations: Option<&Value>, base_theme_class_name: Option<&str>) {
        let Some(associations) = associations.filter(|value| value.is_object()) else {
            return;
        };
        let mut qualifier = ".show-file-icons".to_owned();
        if let Some(base) = base_theme_class_name {
            qualifier = format!("{base} {qualifier}");
        }

        let expanded = ".monaco-tl-twistie.collapsible:not(.collapsed) + .monaco-tl-contents";

        if let Some(folder) = truthy_text(associations, "folder") {
            self.add_selector(format!("{qualifier} .folder-icon::before"), folder);
            self.has_folder_icons = true;
        }

        if let Some(folder_expanded) = truthy_text(associations, "folderExpanded") {
            self.add_selector(format!("{qualifier} {expanded} .folder-icon::before"), folder_expanded);
            self.has_folder_icons = true;
        }

        let root_folder = truthy_text(associations, "rootFolder").or_else(|| truthy_text(associations, "folder"));
        let root_folder_expanded =
            truthy_text(associations, "rootFolderExpanded").or_else(|| truthy_text(associations, "folderExpanded"));

        if let Some(root_folder) = root_folder {
            self.add_selector(format!("{qualifier} .rootfolder-icon::before"), root_folder);
            self.has_folder_icons = true;
        }

        if let Some(root_folder_expanded) = root_folder_expanded {
            self.add_selector(format!("{qualifier} {expanded} .rootfolder-icon::before"), root_folder_expanded);
            self.has_folder_icons = true;
        }

        if let Some(file) = truthy_text(associations, "file") {
            self.add_selector(format!("{qualifier} .file-icon::before"), file);
            self.has_file_icons = true;
        }

        for (key, def_id) in entries(associations, "folderNames") {
            let mut selectors = Vec::new();
            let name = handle_parent_folder(&key.to_lowercase(), &mut selectors);
            selectors.push(format!(".{}-name-folder-icon", class_selector_part(&name)));
            self.add_selector(format!("{qualifier} {}.folder-icon::before", selectors.concat()), &def_id);
            self.has_folder_icons = true;
        }
        for (key, def_id) in entries(associations, "folderNamesExpanded") {
            let mut selectors = Vec::new();
            let name = handle_parent_folder(&key.to_lowercase(), &mut selectors);
            selectors.push(format!(".{}-name-folder-icon", class_selector_part(&name)));
            self.add_selector(format!("{qualifier} {expanded} {}.folder-icon::before", selectors.concat()), &def_id);
            self.has_folder_icons = true;
        }

        for (key, def_id) in entries(associations, "rootFolderNames") {
            let name = key.to_lowercase();
            self.add_selector(format!("{qualifier} .{}-root-name-folder-icon.rootfolder-icon::before", class_selector_part(&name)), &def_id);
            self.has_folder_icons = true;
        }
        for (key, def_id) in entries(associations, "rootFolderNamesExpanded") {
            let name = key.to_lowercase();
            self.add_selector(
                format!("{qualifier} {expanded} .{}-root-name-folder-icon.rootfolder-icon::before", class_selector_part(&name)),
                &def_id,
            );
            self.has_folder_icons = true;
        }

        if associations.get("languageIds").is_some_and(Value::is_object) {
            let mut language_ids = entries(associations, "languageIds");
            let find = |ids: &[(String, String)], name: &str| ids.iter().find(|(key, _)| key == name).map(|(_, def)| def.clone());
            if find(&language_ids, "jsonc").is_none_or(|def| def.is_empty()) {
                if let Some(json) = find(&language_ids, "json").filter(|def| !def.is_empty()) {
                    // `languageIds.jsonc = languageIds.json`: substitui o valor no lugar, ou acrescenta ao fim.
                    match language_ids.iter_mut().find(|(key, _)| key == "jsonc") {
                        Some(entry) => entry.1 = json,
                        None => language_ids.push(("jsonc".to_owned(), json)),
                    }
                }
            }
            for (language_id, def_id) in language_ids {
                self.add_selector(format!("{qualifier} .{}-lang-file-icon.file-icon::before", class_selector_part(&language_id)), &def_id);
                self.has_file_icons = true;
                self.has_specific_file_icons = true;
                self.covered_languages.insert(language_id);
            }
        }

        for (key, def_id) in entries(associations, "fileExtensions") {
            let mut selectors = Vec::new();
            let name = handle_parent_folder(&key.to_lowercase(), &mut selectors);
            let segments: Vec<&str> = name.split('.').collect();
            for index in 0..segments.len() {
                selectors.push(format!(".{}-ext-file-icon", class_selector_part(&segments[index..].join("."))));
            }
            selectors.push(".ext-file-icon".to_owned()); // extra segment to increase file-ext score
            self.add_selector(format!("{qualifier} {}.file-icon::before", selectors.concat()), &def_id);
            self.has_file_icons = true;
            self.has_specific_file_icons = true;
        }

        for (key, def_id) in entries(associations, "fileNames") {
            let mut selectors = Vec::new();
            let file_name = handle_parent_folder(&key.to_lowercase(), &mut selectors);
            selectors.push(format!(".{}-name-file-icon", class_selector_part(&file_name)));
            selectors.push(".name-file-icon".to_owned()); // extra segment to increase file-name score
            let segments: Vec<&str> = file_name.split('.').collect();
            for index in 1..segments.len() {
                selectors.push(format!(".{}-ext-file-icon", class_selector_part(&segments[index..].join("."))));
            }
            selectors.push(".ext-file-icon".to_owned()); // extra segment to increase file-ext score
            self.add_selector(format!("{qualifier} {}.file-icon::before", selectors.concat()), &def_id);
            self.has_file_icons = true;
            self.has_specific_file_icons = true;
        }
    }
}
