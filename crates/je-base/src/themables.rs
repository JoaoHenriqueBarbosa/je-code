//! Porte de `src/vs/base/common/themables.ts`: `ThemeColor` e `ThemeIcon`.
//!
//! Os identificadores dos codicons (`Codicon.error`, `Codicon.file`, `Codicon.folder`) são os ids do
//! `je-codicons` (`error`, `file` e `folder`); o `je-base` não depende dele, então os três ids ficam
//! como constantes aqui.

use std::cell::LazyCell;

use zyarr::RegExp;

pub type ColorIdentifier = String;

pub type IconIdentifier = String;

/// `ThemeColor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColor {
    pub id: String,
}

impl ThemeColor {
    pub fn new(id: impl Into<ColorIdentifier>) -> Self {
        Self { id: id.into() }
    }
}

/// `themeColorFromId`.
pub fn theme_color_from_id(id: impl Into<ColorIdentifier>) -> ThemeColor {
    ThemeColor::new(id)
}

/// `ThemeIcon`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeIcon {
    pub id: String,
    pub color: Option<ThemeColor>,
}

pub const ICON_NAME_SEGMENT: &str = "[A-Za-z0-9]+";
pub const ICON_NAME_EXPRESSION: &str = "[A-Za-z0-9-]+";
pub const ICON_MODIFIER_EXPRESSION: &str = "~[A-Za-z]+";
pub const ICON_NAME_CHARACTER: &str = "[A-Za-z0-9~-]";

/// `Codicon.error.id`, `Codicon.file.id` e `Codicon.folder.id`.
const CODICON_ERROR: &str = "error";
const CODICON_FILE: &str = "file";
const CODICON_FOLDER: &str = "folder";

thread_local! {
    static THEME_ICON_ID_REGEX: LazyCell<RegExp> = LazyCell::new(|| {
        RegExp::from_str(&format!("^({ICON_NAME_EXPRESSION})({ICON_MODIFIER_EXPRESSION})?$"), "").expect("regex do id de ícone válida")
    });
    static REGEX_FROM_STRING: LazyCell<RegExp> = LazyCell::new(|| {
        RegExp::from_str(&format!("^\\$\\(({ICON_NAME_EXPRESSION}(?:{ICON_MODIFIER_EXPRESSION})?)\\)$"), "").expect("regex de ícone válida")
    });
}

/// O `modifier` de `ThemeIcon.modify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconModifier {
    Disabled,
    Spin,
}

impl IconModifier {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Spin => "spin",
        }
    }
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

impl ThemeIcon {
    pub fn from_id(id: impl Into<String>) -> ThemeIcon {
        ThemeIcon { id: id.into(), color: None }
    }

    pub fn as_class_name_array(&self) -> Vec<String> {
        let id_units = units(&self.id);
        let matched = THEME_ICON_ID_REGEX.with(|regex| regex.exec(&id_units, 0));
        let Some(matched) = matched else {
            return ThemeIcon::from_id(CODICON_ERROR).as_class_name_array();
        };
        let text = |n: usize| matched.captures.get(n).cloned().flatten().map(|r| String::from_utf16_lossy(&id_units[r]));
        let id = text(1).unwrap_or_default();
        let modifier = text(2);
        let mut class_names = vec!["codicon".to_string(), format!("codicon-{id}")];
        if let Some(modifier) = modifier.filter(|m| !m.is_empty()) {
            class_names.push(format!("codicon-modifier-{}", &modifier[1..]));
        }
        class_names
    }

    pub fn as_class_name(&self) -> String {
        self.as_class_name_array().join(" ")
    }

    pub fn as_css_selector(&self) -> String {
        format!(".{}", self.as_class_name_array().join("."))
    }

    /// `ThemeIcon.fromString('$(name)')`.
    pub fn from_string(str: &str) -> Option<ThemeIcon> {
        let str_units = units(str);
        let matched = REGEX_FROM_STRING.with(|regex| regex.exec(&str_units, 0))?;
        let range = matched.captures.get(1).cloned().flatten()?;
        Some(ThemeIcon::from_id(String::from_utf16_lossy(&str_units[range])))
    }

    pub fn modify(icon: &ThemeIcon, modifier: Option<IconModifier>) -> ThemeIcon {
        let mut id = icon.id.as_str();
        if let Some(tilde_index) = id.rfind('~') {
            id = &id[..tilde_index];
        }
        match modifier {
            Some(modifier) => ThemeIcon::from_id(format!("{id}~{}", modifier.as_str())),
            None => ThemeIcon::from_id(id),
        }
    }

    pub fn get_modifier(icon: &ThemeIcon) -> Option<&str> {
        icon.id.rfind('~').map(|tilde_index| &icon.id[tilde_index + 1..])
    }

    pub fn is_equal(ti1: &ThemeIcon, ti2: &ThemeIcon) -> bool {
        ti1.id == ti2.id && ti1.color.as_ref().map(|c| &c.id) == ti2.color.as_ref().map(|c| &c.id)
    }

    /// Returns whether specified icon is defined and has 'file' ID.
    pub fn is_file(icon: Option<&ThemeIcon>) -> bool {
        icon.is_some_and(|icon| icon.id == CODICON_FILE)
    }

    /// Returns whether specified icon is defined and has 'folder' ID.
    pub fn is_folder(icon: Option<&ThemeIcon>) -> bool {
        icon.is_some_and(|icon| icon.id == CODICON_FOLDER)
    }
}
