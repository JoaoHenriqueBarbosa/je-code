//! Porte de `src/vs/workbench/services/themes/common/workbenchThemeService.ts`: os identificadores das configurações
//! de tema (`ThemeSettings`), os padrões (`ThemeSettingDefaults`), `migrateThemeSettingsId`, o alvo de gravação
//! (`ThemeSettingTarget`), `IWorkbenchTheme`, `ExtensionData` e `IThemeExtensionPoint`.
//!
//! Fora deste porte:
//! - `IWorkbenchThemeService` (o serviço completo) e as interfaces dos temas de ícone, que dependem de módulos ainda
//!   sem porte.
//! - `COLOR_THEME_DARK_INITIAL_COLORS` e `COLOR_THEME_LIGHT_INITIAL_COLORS` (nenhum consumidor portado ainda).
//! - As interfaces estruturais de customização (`IColorCustomizations`, `ITokenColorCustomizations`,
//!   `ISemanticTokenColorCustomizations` e afins): são objetos JSON, representados por `serde_json::Map`.
//!
//! Divergências:
//! - `ThemeSettings` é um `enum` com [`ThemeSettings::as_str`] (o valor de texto do `enum` do original).
//! - `ThemeSettingTarget` (`ConfigurationTarget | undefined | 'auto' | 'preview'`) é um `enum` de quatro casos.
//! - `ThemeSettingDefaults` é o módulo [`theme_setting_defaults`].

use je_platform::configuration::ConfigurationTarget;
use serde_json::{Map, Value};

pub use crate::services::themes::host_color_scheme_service::IColorScheme;

pub const THEME_SCOPE_OPEN_PAREN: char = '[';
pub const THEME_SCOPE_CLOSE_PAREN: char = ']';
pub const THEME_SCOPE_WILDCARD: char = '*';

/// `ThemeSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeSettings {
    ColorTheme,
    FileIconTheme,
    ProductIconTheme,
    ColorCustomizations,
    TokenColorCustomizations,
    SemanticTokenColorCustomizations,

    PreferredDarkTheme,
    PreferredLightTheme,
    PreferredHcDarkTheme,
    PreferredHcLightTheme,
    DetectColorScheme,
    DetectHc,

    SystemColorTheme,
}

impl ThemeSettings {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ColorTheme => "workbench.colorTheme",
            Self::FileIconTheme => "workbench.iconTheme",
            Self::ProductIconTheme => "workbench.productIconTheme",
            Self::ColorCustomizations => "workbench.colorCustomizations",
            Self::TokenColorCustomizations => "editor.tokenColorCustomizations",
            Self::SemanticTokenColorCustomizations => "editor.semanticTokenColorCustomizations",
            Self::PreferredDarkTheme => "workbench.preferredDarkColorTheme",
            Self::PreferredLightTheme => "workbench.preferredLightColorTheme",
            // id kept for compatibility reasons
            Self::PreferredHcDarkTheme => "workbench.preferredHighContrastColorTheme",
            Self::PreferredHcLightTheme => "workbench.preferredHighContrastLightColorTheme",
            Self::DetectColorScheme => "window.autoDetectColorScheme",
            Self::DetectHc => "window.autoDetectHighContrast",
            Self::SystemColorTheme => "window.systemColorTheme",
        }
    }
}

/// `ThemeSettingDefaults`.
pub mod theme_setting_defaults {
    pub const COLOR_THEME_DARK: &str = "Dark 2026";
    pub const COLOR_THEME_LIGHT: &str = "Light 2026";
    pub const COLOR_THEME_HC_DARK: &str = "Default High Contrast";
    pub const COLOR_THEME_HC_LIGHT: &str = "Default High Contrast Light";

    pub const FILE_ICON_THEME: &str = "vs-seti";
    pub const PRODUCT_ICON_THEME: &str = "Default";
}

/// `migrateThemeSettingsId`: migra os ids legados das configurações de tema para os atuais. O prefixo "Default" saiu
/// dos temas embutidos e o "Experimental" foi trocado quando os temas do VS Code ficaram disponíveis.
pub fn migrate_theme_settings_id(settings_id: &str) -> String {
    match settings_id {
        "Default Dark Modern" => "Dark Modern",
        "Default Light Modern" => "Light Modern",
        "Default Dark+" => "Dark+",
        "Default Light+" => "Light+",
        "Experimental Dark" | "VS Code Dark" => theme_setting_defaults::COLOR_THEME_DARK,
        "Experimental Light" | "VS Code Light" => theme_setting_defaults::COLOR_THEME_LIGHT,
        other => other,
    }
    .to_owned()
}

/// `ThemeSettingTarget`: `ConfigurationTarget | undefined | 'auto' | 'preview'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSettingTarget {
    Undefined,
    Auto,
    Preview,
    Target(ConfigurationTarget),
}

/// `IWorkbenchTheme`. O `settingsId` é `string | null`: `None` é o `null`.
pub trait IWorkbenchTheme {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn extension_data(&self) -> Option<&ExtensionData>;
    fn description(&self) -> Option<&str>;
    fn settings_id(&self) -> Option<&str>;
}

/// `ExtensionData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionData {
    pub extension_id: String,
    pub extension_publisher: String,
    pub extension_name: String,
    pub extension_is_builtin: bool,
}

impl ExtensionData {
    /// `ExtensionData.toJSONObject`: `None` é o `undefined` (que o `JSON.stringify` omite).
    pub fn to_json_object(data: Option<&ExtensionData>) -> Option<Value> {
        let data = data?;
        let mut object = Map::new();
        object.insert("_extensionId".into(), Value::String(data.extension_id.clone()));
        object.insert("_extensionIsBuiltin".into(), Value::Bool(data.extension_is_builtin));
        object.insert("_extensionName".into(), Value::String(data.extension_name.clone()));
        object.insert("_extensionPublisher".into(), Value::String(data.extension_publisher.clone()));
        Some(Value::Object(object))
    }

    /// `ExtensionData.fromJSONObject`: `None` se falta algum dos quatro campos ou o tipo não confere.
    pub fn from_json_object(value: &Value) -> Option<ExtensionData> {
        let object = value.as_object()?;
        Some(ExtensionData {
            extension_id: object.get("_extensionId")?.as_str()?.to_owned(),
            extension_is_builtin: object.get("_extensionIsBuiltin")?.as_bool()?,
            extension_name: object.get("_extensionName")?.as_str()?.to_owned(),
            extension_publisher: object.get("_extensionPublisher")?.as_str()?.to_owned(),
        })
    }

    /// `ExtensionData.fromName` (`isBuiltin` é `false` por padrão).
    pub fn from_name(publisher: &str, name: &str, is_builtin: bool) -> ExtensionData {
        ExtensionData {
            extension_publisher: publisher.to_owned(),
            extension_id: format!("{publisher}.{name}"),
            extension_name: name.to_owned(),
            extension_is_builtin: is_builtin,
        }
    }
}

/// `IThemeExtensionPoint`. O `uiTheme` é um `ThemeTypeSelector` (`vs`, `vs-dark`, `hc-black`, `hc-light`) e o `_watch`
/// é o campo `watch` (opção não suportada para observar o arquivo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IThemeExtensionPoint {
    pub id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub path: String,
    pub ui_theme: Option<String>,
    pub watch: bool,
}
