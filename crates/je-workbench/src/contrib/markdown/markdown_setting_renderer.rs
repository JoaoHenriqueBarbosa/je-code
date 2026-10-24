//! Porte de `src/vs/workbench/contrib/markdown/browser/markdownSettingRenderer.ts`.
//!
//! `SimpleSettingRenderer` transforma links `code://settings/<id>` e `` `setting(id:valor)` `` em
//! HTML de botão e monta as ações do menu de contexto (testar, restaurar, ver nas configurações,
//! copiar o id).
//!
//! Divergências:
//! - **`@IConfigurationService` e companhia** são `Rc<dyn ...>` recebidos por [`SimpleSettingRenderer::new`]; o
//!   `@IContextMenuService` é o [`IContextMenuProvider`] e o [`Ui`] acompanha, porque o `ActionViewItem` precisa dele.
//! - **`getHtmlRenderer` e `getCodeSpanRenderer`** devolvem `Fn(&str) -> String` sobre o `raw` (o `Tokens.HTML | Tokens.Tag`)
//!   e o `text` (o `Tokens.Codespan`), os únicos campos que o original lê.
//! - **`unknown` dos valores de configuração** é [`Dynamic`]; `parseInt(value, 10)` é [`parse_int`], que devolve `NaN`
//!   quando não há dígitos.
//! - **`raw.replace(regex, rendered)`** troca o casamento pelo texto literal; o original também interpretaria `$&`, `$1` e
//!   afins dentro do `rendered`, que só tem o id da configuração e texto fixo.
//! - **`new ActionViewItem(action, action, { label: true })`**: o `context` é `None`, porque o contexto do original é a própria
//!   ação e o item não o lê para desenhar o rótulo.
//! - **`showContextMenu`** recebe o `Ui` e entrega um [`IContextMenuDelegate`] com a âncora em `{ x, y }`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::layout::IAnchor;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_ui::Ui;
use je_widgets::action_view_items::{ActionViewItem, IActionViewItem, IActionViewItemOptions};
use je_widgets::context_view_api::{Anchor, IContextMenuDelegate, IContextMenuProvider};
use zyarr::RegExp;

use crate::contrib::preferences::settings_tree_models::setting_key_to_display_format;
use crate::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService, ISetting};

fn compile_reg_exp(pattern: &str) -> RegExp {
    RegExp::from_str(pattern, "").unwrap_or_else(|error| panic!("expressão regular inválida /{pattern}/: {}", error.message()))
}

/// `parseInt(value, 10)`: o prefixo inteiro (com sinal opcional) depois dos espaços iniciais, ou `NaN`.
fn parse_int(value: &str) -> f64 {
    let trimmed = value.trim_start();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return f64::NAN;
    }
    let number: f64 = digits[..end].parse().unwrap_or(f64::NAN);
    if negative { -number } else { number }
}

/// O `{ x, y }` do menu de contexto.
struct SettingMenuDelegate {
    ui: Ui,
    x: f64,
    y: f64,
    actions: Vec<ActionRef>,
}

impl IContextMenuDelegate for SettingMenuDelegate {
    fn get_anchor(&self) -> Anchor {
        Anchor::Rect(IAnchor { x: self.x, y: self.y, width: None, height: None })
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }

    fn get_action_view_item(&self, action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        let options = IActionViewItemOptions { label: Some(true), ..Default::default() };
        Some(ActionViewItem::new(&self.ui, None, action.clone(), options))
    }
}

/// `SimpleSettingRenderer`.
pub struct SimpleSettingRenderer {
    code_setting_anchor_regex: RegExp,
    code_setting_simple_regex: RegExp,

    /// setting ID to user's original setting value
    updated_settings: RefCell<HashMap<String, Dynamic>>,
    /// setting ID to setting
    encountered_settings: RefCell<HashMap<String, ISetting>>,
    /// setting ID to feature value
    featured_settings: RefCell<HashMap<String, Dynamic>>,

    configuration_service: Rc<dyn IConfigurationService>,
    context_menu_service: Rc<dyn IContextMenuProvider>,
    preferences_service: Rc<dyn IPreferencesService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    clipboard_service: Rc<dyn IClipboardService>,
    ui: Ui,
}

impl SimpleSettingRenderer {
    pub fn new(
        configuration_service: Rc<dyn IConfigurationService>,
        context_menu_service: Rc<dyn IContextMenuProvider>,
        preferences_service: Rc<dyn IPreferencesService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        clipboard_service: Rc<dyn IClipboardService>,
        ui: Ui,
    ) -> Rc<Self> {
        Rc::new(Self {
            code_setting_anchor_regex: compile_reg_exp(r#"^<a (href)=".*code.*://settings/([^\s"]+)"(?:\s*codesetting="([^"]+)")?>"#),
            code_setting_simple_regex: compile_reg_exp(r"^setting\(([^\s:)]+)(?::([^)]+))?\)$"),
            updated_settings: RefCell::new(HashMap::new()),
            encountered_settings: RefCell::new(HashMap::new()),
            featured_settings: RefCell::new(HashMap::new()),
            configuration_service,
            context_menu_service,
            preferences_service,
            telemetry_service,
            clipboard_service,
            ui,
        })
    }

    fn get_value(&self, key: &str) -> Dynamic {
        self.configuration_service.get_value(Some(key), None)
    }

    pub fn featured_setting_states(&self) -> HashMap<String, bool> {
        let mut result = HashMap::new();
        for (setting_id, value) in self.featured_settings.borrow().iter() {
            result.insert(setting_id.clone(), self.get_value(setting_id).strict_equals(value));
        }
        result
    }

    /// `raw.replace(regex, rendered)` do primeiro casamento; `groups` são os grupos 2 e 3 (ou 1 e 2) já lidos.
    fn replace_match(regex: &RegExp, raw: &str, rendered: &str, units: &[u16], start: usize, end: usize) -> String {
        let _ = regex;
        let mut out = JsString::from(&units[..start]);
        out.push_units(&rendered.encode_utf16().collect::<Vec<u16>>());
        out.push_units(&units[end..]);
        let _ = raw;
        out.to_string_lossy()
    }

    /// Lê o texto do grupo `n` do casamento (`None` quando o grupo não participou).
    fn group_text(units: &[u16], found: &zyarr::Match, n: usize) -> Option<String> {
        found.captures.get(n).and_then(|range| range.clone()).map(|range| String::from_utf16_lossy(&units[range]))
    }

    fn replace_anchor(&self, raw: &str) -> Option<String> {
        let units: Vec<u16> = raw.encode_utf16().collect();
        let found = self.code_setting_anchor_regex.exec(&units, 0)?;
        if found.captures.len() == 4 {
            let setting_id = Self::group_text(&units, &found, 2)?;
            let rendered = self.render(&setting_id, Self::group_text(&units, &found, 3).as_deref());
            if let Some(rendered) = rendered {
                return Some(Self::replace_match(&self.code_setting_anchor_regex, raw, &rendered, &units, found.start(), found.end()));
            }
        }
        None
    }

    fn replace_simple(&self, raw: &str) -> Option<String> {
        let units: Vec<u16> = raw.encode_utf16().collect();
        let found = self.code_setting_simple_regex.exec(&units, 0)?;
        if found.captures.len() == 3 {
            let setting_id = Self::group_text(&units, &found, 1)?;
            let rendered = self.render(&setting_id, Self::group_text(&units, &found, 2).as_deref());
            if let Some(rendered) = rendered {
                return Some(Self::replace_match(&self.code_setting_simple_regex, raw, &rendered, &units, found.start(), found.end()));
            }
        }
        None
    }

    /// `getHtmlRenderer()`: recebe o `raw` do token e devolve o HTML.
    pub fn get_html_renderer(self: &Rc<Self>) -> impl Fn(&str) -> String + 'static {
        let this = Rc::clone(self);
        move |raw: &str| this.replace_anchor(raw).unwrap_or_else(|| raw.to_owned())
    }

    /// `getCodeSpanRenderer()`: recebe o `text` do token e devolve o HTML.
    pub fn get_code_span_renderer(self: &Rc<Self>) -> impl Fn(&str) -> String + 'static {
        let this = Rc::clone(self);
        move |text: &str| this.replace_simple(text).unwrap_or_else(|| format!("<code>{text}</code>"))
    }

    /// `settingToUriString(settingId, value?)`: o `value ? ... : ''` do original trata o texto vazio como ausente.
    pub fn setting_to_uri_string(&self, setting_id: &str, value: Option<&str>) -> String {
        let suffix = match value {
            Some(value) if !value.is_empty() => format!("/{value}"),
            _ => String::new(),
        };
        format!("{}://{}{}", schemas::CODE_SETTING, setting_id, suffix)
    }

    fn get_setting(&self, setting_id: &str) -> Option<ISetting> {
        if let Some(setting) = self.encountered_settings.borrow().get(setting_id) {
            return Some(setting.clone());
        }
        self.preferences_service.get_setting(setting_id)
    }

    /// `parseValue(settingId, value)`: `undefined` é [`Dynamic::Undefined`].
    pub fn parse_value(&self, setting_id: &str, value: &str) -> Dynamic {
        if value == "undefined" || value.is_empty() {
            return Dynamic::Undefined;
        }
        let Some(setting) = self.get_setting(setting_id) else {
            return Dynamic::string(value);
        };

        match &setting.r#type {
            Some(kind) if kind.is("boolean") => Dynamic::Bool(value == "true"),
            Some(kind) if kind.is("number") => Dynamic::Number(parse_int(value)),
            _ => Dynamic::string(value),
        }
    }

    fn render(&self, setting_id: &str, new_value: Option<&str>) -> Option<String> {
        let Some(setting) = self.get_setting(setting_id) else {
            return Some(format!("<code>{setting_id}</code>"));
        };

        self.render_setting(&setting, new_value)
    }

    fn view_in_settings_message(&self, setting_id: &str, already_displayed: bool) -> String {
        if already_displayed {
            localize("viewInSettings", "View in Settings", &[])
        } else {
            let display_name = setting_key_to_display_format(setting_id, "", false);
            localize("viewInSettingsDetailed", "View \"{0}: {1}\" in Settings", &[&display_name.category, &display_name.label])
        }
    }

    fn restore_previous_setting_message(&self, setting_id: &str) -> String {
        let display_name = setting_key_to_display_format(setting_id, "", false);
        localize("restorePreviousValue", "Restore value of \"{0}: {1}\"", &[&display_name.category, &display_name.label])
    }

    fn is_already_set(&self, setting: &ISetting, value: &Dynamic) -> bool {
        let current_value = self.get_value(&setting.key);
        current_value.strict_equals(value) || (current_value.is_undefined() && setting.value.strict_equals(value))
    }

    fn boolean_setting_message(&self, setting: &ISetting, boolean_value: bool) -> Option<String> {
        let display_name = setting_key_to_display_format(&setting.key, "", false);
        if self.is_already_set(setting, &Dynamic::Bool(boolean_value)) {
            if boolean_value {
                return Some(localize("alreadysetBoolTrue", "\"{0}: {1}\" is already enabled", &[&display_name.category, &display_name.label]));
            } else {
                return Some(localize("alreadysetBoolFalse", "\"{0}: {1}\" is already disabled", &[&display_name.category, &display_name.label]));
            }
        }

        if boolean_value {
            Some(localize("trueMessage", "Enable \"{0}: {1}\"", &[&display_name.category, &display_name.label]))
        } else {
            Some(localize("falseMessage", "Disable \"{0}: {1}\"", &[&display_name.category, &display_name.label]))
        }
    }

    fn string_setting_message(&self, setting: &ISetting, string_value: &str) -> Option<String> {
        let display_name = setting_key_to_display_format(&setting.key, "", false);
        if self.is_already_set(setting, &Dynamic::string(string_value)) {
            return Some(localize("alreadysetString", "\"{0}: {1}\" is already set to \"{2}\"", &[&display_name.category, &display_name.label, string_value]));
        }

        Some(localize("stringValue", "Set \"{0}: {1}\" to \"{2}\"", &[&display_name.category, &display_name.label, string_value]))
    }

    fn number_setting_message(&self, setting: &ISetting, number_value: f64) -> Option<String> {
        let display_name = setting_key_to_display_format(&setting.key, "", false);
        let number_text = number_to_js_string(number_value);
        if self.is_already_set(setting, &Dynamic::Number(number_value)) {
            return Some(localize("alreadysetNum", "\"{0}: {1}\" is already set to {2}", &[&display_name.category, &display_name.label, &number_text]));
        }

        Some(localize("numberValue", "Set \"{0}: {1}\" to {2}", &[&display_name.category, &display_name.label, &number_text]))
    }

    fn render_setting(&self, setting: &ISetting, new_value: Option<&str>) -> Option<String> {
        let href = self.setting_to_uri_string(&setting.key, new_value);
        let title = localize("changeSettingTitle", "View or change setting", &[]);
        Some(format!(
            "<code tabindex=\"0\"><a href=\"{href}\" class=\"codesetting\" title=\"{title}\" aria-role=\"button\"><svg width=\"14\" height=\"14\" viewBox=\"0 0 15 15\" xmlns=\"http://www.w3.org/2000/svg\" fill=\"currentColor\"><path d=\"M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.8-1.3 2 .8.8 2-1.3.8.3.4 2.3h1.2l.5-2.4.8-.3 2 1.3.8-.8-1.3-2 .3-.8 2.3-.4V7.4l-2.4-.5-.3-.8 1.3-2-.8-.8-2 1.3-.7-.2zM9.4 1l.5 2.4L12 2.1l2 2-1.4 2.1 2.4.4v2.8l-2.4.5L14 12l-2 2-2.1-1.4-.5 2.4H6.6l-.5-2.4L4 13.9l-2-2 1.4-2.1L1 9.4V6.6l2.4-.5L2.1 4l2-2 2.1 1.4.4-2.4h2.8zm.6 7c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2zM8 9c.6 0 1-.4 1-1s-.4-1-1-1-1 .4-1 1 .4 1 1 1z\"/></svg>\n\t\t\t<span class=\"separator\"></span>\n\t\t\t<span class=\"setting-name\">{key}</span>\n\t\t</a></code>",
            key = setting.key,
        ))
    }

    fn get_setting_message(&self, setting: &ISetting, new_value: &Dynamic) -> Option<String> {
        let kind = setting.r#type.as_ref()?;
        if kind.is("boolean") {
            self.boolean_setting_message(setting, matches!(new_value, Dynamic::Bool(true)))
        } else if kind.is("string") {
            let text = match new_value {
                Dynamic::String(text) => text.to_string_lossy(),
                _ => String::new(),
            };
            self.string_setting_message(setting, &text)
        } else if kind.is("number") {
            let number = match new_value {
                Dynamic::Number(number) => *number,
                _ => f64::NAN,
            };
            self.number_setting_message(setting, number)
        } else {
            None
        }
    }

    pub fn restore_setting(&self, setting_id: &str) -> je_base::promise::Promise<()> {
        let user_original_setting_value = self.updated_settings.borrow_mut().remove(setting_id).unwrap_or(Dynamic::Undefined);
        self.configuration_service.update_value(setting_id, user_original_setting_value, None, Some(ConfigurationTarget::User))
    }

    pub fn set_setting(&self, setting_id: &str, current_setting_value: Dynamic, new_setting_value: Dynamic) -> je_base::promise::Promise<()> {
        self.updated_settings.borrow_mut().insert(setting_id.to_owned(), current_setting_value);
        self.configuration_service.update_value(setting_id, new_setting_value, None, Some(ConfigurationTarget::User))
    }

    /// `getActions(uri)`: `None` quando o esquema não é `code-setting`.
    pub fn get_actions(self: &Rc<Self>, uri: &URI) -> Option<Vec<ActionRef>> {
        if uri.scheme().to_string_lossy() != schemas::CODE_SETTING {
            return None;
        }

        let mut actions: Vec<ActionRef> = Vec::new();

        let setting_id = uri.authority().to_string_lossy();
        let path = uri.path().to_string_lossy();
        let new_setting_value = self.parse_value(&setting_id, path.get(1..).unwrap_or(""));
        let current_setting_value = self.configuration_service.inspect(&setting_id, &ConfigurationOverrides::default()).user_value();

        if !new_setting_value.is_undefined() && new_setting_value.strict_equals(&current_setting_value) && self.updated_settings.borrow().contains_key(&setting_id) {
            let restore_message = self.restore_previous_setting_message(&setting_id);
            let (this, id) = (Rc::clone(self), setting_id.clone());
            actions.push(to_action(ToActionProps {
                id: "restoreSetting".to_owned(),
                label: restore_message.clone(),
                tooltip: Some(restore_message),
                enabled: Some(true),
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| MaybePromise::Promise(this.restore_setting(&id).then(|_| Dynamic::Undefined))),
            }));
        } else if !new_setting_value.is_undefined() {
            let setting = self.get_setting(&setting_id);
            let try_setting_message = setting.as_ref().and_then(|setting| self.get_setting_message(setting, &new_setting_value));

            if let (Some(setting), Some(try_setting_message)) = (setting, try_setting_message) {
                let (this, id) = (Rc::clone(self), setting_id.clone());
                let (current, new_value) = (current_setting_value.clone(), new_setting_value.clone());
                actions.push(to_action(ToActionProps {
                    id: "trySetting".to_owned(),
                    label: try_setting_message.clone(),
                    tooltip: Some(try_setting_message),
                    enabled: Some(!self.is_already_set(&setting, &new_setting_value)),
                    checked: None,
                    class: None,
                    alt: None,
                    run: Rc::new(move |_| {
                        // O original não espera o `setSetting` nem repassa o resultado.
                        this.set_setting(&id, current.clone(), new_value.clone());
                        MaybePromise::Value(Dynamic::Undefined)
                    }),
                }));
            }
        }

        let view_in_settings_message = self.view_in_settings_message(&setting_id, !actions.is_empty());
        let (this, id) = (Rc::clone(self), setting_id.clone());
        actions.push(to_action(ToActionProps {
            id: "viewInSettings".to_owned(),
            label: view_in_settings_message.clone(),
            tooltip: Some(view_in_settings_message),
            enabled: Some(true),
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                let options = IOpenSettingsOptions { query: Some(format!("@id:{id}")), ..Default::default() };
                MaybePromise::Promise(this.preferences_service.open_application_settings(Some(&options)).then(|_| Dynamic::Undefined))
            }),
        }));

        let (this, id) = (Rc::clone(self), setting_id);
        actions.push(to_action(ToActionProps {
            id: "copySettingId".to_owned(),
            label: localize("copySettingId", "Copy Setting ID", &[]),
            tooltip: Some(localize("copySettingId", "Copy Setting ID", &[])),
            enabled: Some(true),
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                this.clipboard_service.write_text(&id);
                MaybePromise::Value(Dynamic::Undefined)
            }),
        }));

        Some(actions)
    }

    fn show_context_menu(self: &Rc<Self>, uri: &URI, x: f64, y: f64) {
        let Some(actions) = self.get_actions(uri) else {
            return;
        };

        self.context_menu_service.show_context_menu(Rc::new(SettingMenuDelegate { ui: self.ui.clone(), x, y, actions }));
    }

    pub fn update_setting(self: &Rc<Self>, uri: &URI, x: f64, y: f64) {
        if uri.scheme().to_string_lossy() == schemas::CODE_SETTING {
            self.telemetry_service.public_log2("releaseNotesSettingAction", Some(Dynamic::object([("settingId", Dynamic::string(&uri.authority().to_string_lossy()))])));
            self.show_context_menu(uri, x, y);
        }
    }
}
