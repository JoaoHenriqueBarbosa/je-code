//! Porte de `src/vs/platform/action/common/actionCommonCategories.ts`.

use super::action::{LocalizedString, localize2};

/// `Categories`: o objeto congelado com as categorias de comando.
pub struct Categories {
    pub view: LocalizedString,
    pub help: LocalizedString,
    pub test: LocalizedString,
    pub file: LocalizedString,
    pub preferences: LocalizedString,
    pub developer: LocalizedString,
}

impl Categories {
    pub fn new() -> Self {
        Self {
            view: localize2("view", "View"),
            help: localize2("help", "Help"),
            test: localize2("test", "Test"),
            file: localize2("file", "File"),
            preferences: localize2("preferences", "Preferences"),
            developer: localize2("developer", "Developer"),
        }
    }

    /// `Categories[key]` para `keyof typeof Categories`.
    pub fn get(&self, key: &str) -> Option<&LocalizedString> {
        match key {
            "View" => Some(&self.view),
            "Help" => Some(&self.help),
            "Test" => Some(&self.test),
            "File" => Some(&self.file),
            "Preferences" => Some(&self.preferences),
            "Developer" => Some(&self.developer),
            _ => None,
        }
    }
}

impl Default for Categories {
    fn default() -> Self {
        Self::new()
    }
}
