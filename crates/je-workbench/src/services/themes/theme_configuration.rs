//! Porte de `src/vs/workbench/services/themes/common/themeConfiguration.ts`: o `ThemeConfiguration`, a leitura das
//! configurações de tema (tema de cor, de ícones, de produto e as customizações), a escolha do tema pelo esquema de
//! cores do sistema e a gravação da configuração no alvo certo.
//!
//! Fora deste porte (dependem de módulos ainda sem porte: `colorThemeSchema`, o esquema JSON do registro de cores e
//! o de classificação de tokens, e `notifyConfigurationSchemaUpdated`):
//! - O registro das configurações de tema (`themeSettingsConfiguration`, `themeSettingsWindowConfiguration` e
//!   `tokenColorCustomizationConfiguration`) e os esquemas associados.
//! - `updateColorThemeConfigurationSchemas`, `updateFileIconThemeConfigurationSchemas` e
//!   `updateProductIconThemeConfigurationSchemas`.
//! - `formatSettingAsLink` e `COLOR_THEME_CONFIGURATION_SETTINGS_TAG` (só servem aos esquemas).
//!
//! Divergências:
//! - Os valores lidos da configuração são [`Dynamic`]. `colorTheme`, `fileIconTheme` e `productIconTheme` devolvem
//!   `Option<String>` (`None` para `undefined`, `null` ou valor que não é texto). As customizações são objetos JSON
//!   (`serde_json::Map`); um valor que não é objeto vale como `{}` (no original, um valor de outro tipo chegaria ao
//!   consumidor sem validação).
//! - `tokenColorCustomizations` só reescreve `textMateRules` quando é um array, e só as regras que são objetos.
//! - `setColorTheme`, `setFileIconTheme` e `setProductIconTheme` aceitam qualquer [`IWorkbenchTheme`] e o devolvem
//!   ao fim (a `Promise<T>` do original).
//! - `isDefaultColorTheme` compara o `default.value` e o `value` com `===` ([`Dynamic::strict_equals`]).

use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget};
use je_theme::ThemeType;
use serde_json::{Map, Value};

use super::host_color_scheme_service::{IColorScheme, IHostColorSchemeService};
use super::workbench_theme_service::{IWorkbenchTheme, ThemeSettingTarget, ThemeSettings};
use crate::api::common::ext_host_types::dynamic_to_json;
use super::color_theme_data::to_serde;

/// `colorSchemeToPreferred`: a configuração do tema preferido de cada esquema de cores.
fn color_scheme_to_preferred(scheme: ThemeType) -> ThemeSettings {
    match scheme {
        ThemeType::Dark => ThemeSettings::PreferredDarkTheme,
        ThemeType::Light => ThemeSettings::PreferredLightTheme,
        ThemeType::HighContrastDark => ThemeSettings::PreferredHcDarkTheme,
        ThemeType::HighContrastLight => ThemeSettings::PreferredHcLightTheme,
    }
}

/// O texto de um valor da configuração (`None` se não é texto).
fn as_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// O objeto JSON de um valor da configuração (`getValue(...) || {}`).
fn as_object(value: &Dynamic) -> Map<String, Value> {
    match dynamic_to_json(value).map(|json| to_serde(&json)) {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    }
}

pub struct ThemeConfiguration {
    configuration_service: Rc<dyn IConfigurationService>,
    host_color_service: Rc<dyn IHostColorSchemeService>,
}

impl ThemeConfiguration {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, host_color_service: Rc<dyn IHostColorSchemeService>) -> Self {
        Self { configuration_service, host_color_service }
    }

    fn get_value(&self, setting: ThemeSettings) -> Dynamic {
        self.configuration_service.get_value(Some(setting.as_str()), None)
    }

    fn inspect(&self, key: &str) -> Rc<dyn je_platform::configuration::ConfigurationValue> {
        self.configuration_service.inspect(key, &ConfigurationOverrides::default())
    }

    /// `colorTheme`.
    pub fn color_theme(&self) -> Option<String> {
        as_text(&self.configuration_service.get_value(Some(self.get_color_theme_setting_id().as_str()), None))
    }

    /// `fileIconTheme`.
    pub fn file_icon_theme(&self) -> Option<String> {
        as_text(&self.get_value(ThemeSettings::FileIconTheme))
    }

    /// `productIconTheme`.
    pub fn product_icon_theme(&self) -> Option<String> {
        as_text(&self.get_value(ThemeSettings::ProductIconTheme))
    }

    /// `colorCustomizations`.
    pub fn color_customizations(&self) -> Map<String, Value> {
        as_object(&self.get_value(ThemeSettings::ColorCustomizations))
    }

    /// `tokenColorCustomizations`: uma regra de `textMateRules` com `fontSize` e sem `lineHeight` ganha o `lineHeight`
    /// igual ao `fontSize`.
    pub fn token_color_customizations(&self) -> Map<String, Value> {
        let mut token_color_customization = as_object(&self.get_value(ThemeSettings::TokenColorCustomizations));
        let Some(Value::Array(text_mate_rules)) = token_color_customization.get("textMateRules") else {
            return token_color_customization;
        };
        let updated_rules: Vec<Value> = text_mate_rules
            .iter()
            .map(|rule| {
                let Some(settings) = rule.get("settings").and_then(Value::as_object) else { return rule.clone() };
                let (Some(font_size), None) = (settings.get("fontSize"), settings.get("lineHeight")) else {
                    return rule.clone();
                };
                let mut updated_settings = settings.clone();
                updated_settings.insert("lineHeight".into(), font_size.clone());
                let mut updated_rule = rule.as_object().cloned().unwrap_or_default();
                updated_rule.insert("settings".into(), Value::Object(updated_settings));
                Value::Object(updated_rule)
            })
            .collect();
        token_color_customization.insert("textMateRules".into(), Value::Array(updated_rules));
        token_color_customization
    }

    /// `semanticTokenColorCustomizations`.
    pub fn semantic_token_color_customizations(&self) -> Option<Map<String, Value>> {
        match dynamic_to_json(&self.get_value(ThemeSettings::SemanticTokenColorCustomizations)).map(|json| to_serde(&json)) {
            Some(Value::Object(object)) => Some(object),
            _ => None,
        }
    }

    /// `getPreferredColorScheme`: o alto contraste do sistema vence, depois o esquema claro ou escuro.
    pub fn get_preferred_color_scheme(&self) -> Option<ThemeType> {
        if self.is_detecting_high_contrast() && self.host_color_service.high_contrast() {
            return Some(if self.host_color_service.dark() { ThemeType::HighContrastDark } else { ThemeType::HighContrastLight });
        }
        if self.is_detecting_color_scheme() {
            return Some(if self.host_color_service.dark() { ThemeType::Dark } else { ThemeType::Light });
        }
        None
    }

    /// `isDetectingHighContrast`.
    pub fn is_detecting_high_contrast(&self) -> bool {
        self.get_value(ThemeSettings::DetectHc).is_truthy()
    }

    /// `isDetectingColorScheme`.
    pub fn is_detecting_color_scheme(&self) -> bool {
        self.get_value(ThemeSettings::DetectColorScheme).is_truthy()
    }

    /// `isPreferredColorSchemeChange`: se a mudança do esquema do sistema (em relação ao `previous`) altera o tema
    /// preferido.
    pub fn is_preferred_color_scheme_change(&self, previous: &IColorScheme) -> bool {
        let dark_changed = previous.dark != self.host_color_service.dark();
        if self.is_detecting_color_scheme() && dark_changed {
            return true;
        }
        if self.is_detecting_high_contrast() {
            return previous.high_contrast != self.host_color_service.high_contrast()
                || (self.host_color_service.high_contrast() && dark_changed);
        }
        false
    }

    /// `getColorThemeSettingId`.
    pub fn get_color_theme_setting_id(&self) -> ThemeSettings {
        match self.get_preferred_color_scheme() {
            Some(scheme) => color_scheme_to_preferred(scheme),
            None => ThemeSettings::ColorTheme,
        }
    }

    /// `setColorTheme`.
    pub async fn set_color_theme<T: IWorkbenchTheme>(&self, theme: T, settings_target: ThemeSettingTarget) -> Result<T, Error> {
        self.write_configuration(self.get_color_theme_setting_id().as_str(), settings_id_value(&theme), settings_target).await?;
        Ok(theme)
    }

    /// `setFileIconTheme`.
    pub async fn set_file_icon_theme<T: IWorkbenchTheme>(&self, theme: T, settings_target: ThemeSettingTarget) -> Result<T, Error> {
        self.write_configuration(ThemeSettings::FileIconTheme.as_str(), settings_id_value(&theme), settings_target).await?;
        Ok(theme)
    }

    /// `setProductIconTheme`.
    pub async fn set_product_icon_theme<T: IWorkbenchTheme>(&self, theme: T, settings_target: ThemeSettingTarget) -> Result<T, Error> {
        self.write_configuration(ThemeSettings::ProductIconTheme.as_str(), settings_id_value(&theme), settings_target).await?;
        Ok(theme)
    }

    /// `isDefaultColorTheme`: o valor efetivo é o padrão do registro.
    pub fn is_default_color_theme(&self) -> bool {
        let settings = self.inspect(self.get_color_theme_setting_id().as_str());
        let default_value = settings.default_inspect().map_or(Dynamic::Undefined, |default| default.value());
        default_value.strict_equals(&settings.value())
    }

    /// `findAutoConfigurationTarget`.
    pub fn find_auto_configuration_target(&self, key: &str) -> ConfigurationTarget {
        let settings = self.inspect(key);
        if !settings.workspace_folder_value().is_undefined() {
            ConfigurationTarget::WorkspaceFolder
        } else if !settings.workspace_value().is_undefined() {
            ConfigurationTarget::Workspace
        } else if !settings.user_remote_value().is_undefined() {
            ConfigurationTarget::UserRemote
        } else {
            ConfigurationTarget::User
        }
    }

    /// `writeConfiguration`: grava no alvo pedido, sem mexer quando nada muda; no `USER`, o valor padrão remove a
    /// chave das configurações do usuário.
    async fn write_configuration(&self, key: &str, mut value: Dynamic, settings_target: ThemeSettingTarget) -> Result<(), Error> {
        let target = match settings_target {
            ThemeSettingTarget::Undefined | ThemeSettingTarget::Preview => return Ok(()),
            ThemeSettingTarget::Auto => {
                return self.configuration_service.update_value(key, value, None, None).await;
            }
            ThemeSettingTarget::Target(target) => target,
        };

        let settings = self.inspect(key);
        match target {
            ConfigurationTarget::User => {
                if value.strict_equals(&settings.user_value()) {
                    return Ok(()); // nothing to do
                } else if value.strict_equals(&settings.default_value()) {
                    if settings.user_value().is_undefined() {
                        return Ok(()); // nothing to do
                    }
                    value = Dynamic::Undefined; // remove configuration from user settings
                }
            }
            ConfigurationTarget::Workspace | ConfigurationTarget::WorkspaceFolder | ConfigurationTarget::UserRemote => {
                if value.strict_equals(&settings.value()) {
                    return Ok(()); // nothing to do
                }
            }
            _ => {}
        }
        self.configuration_service.update_value(key, value, None, Some(target)).await
    }
}

/// O `theme.settingsId` como valor de configuração (`null` quando o tema não tem).
fn settings_id_value(theme: &impl IWorkbenchTheme) -> Dynamic {
    theme.settings_id().map_or(Dynamic::Null, Dynamic::from)
}
