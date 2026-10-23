//! Porte de `src/vs/base/parts/contextmenu/common/contextmenu.ts`: os tipos do menu de contexto nativo.
//!
//! O `click` de um `IContextMenuItem` é uma função e não viaja em JSON; o item serializável
//! ([`ISerializableContextMenuItem`]) troca o `click` por um `id` numérico.

use serde::{Deserialize, Serialize};

/// `ICommonContextMenuItem['type']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextMenuItemType {
    Normal,
    Separator,
    Submenu,
    Checkbox,
    Radio,
}

record! {
    /// `ISerializableContextMenuItem` (com o `ICommonContextMenuItem` que ele estende).
    pub struct ISerializableContextMenuItem {
        req { id: f64 }
        opt {
            label: String,
            r#type: ContextMenuItemType,
            accelerator: String,
            enabled: bool,
            visible: bool,
            checked: bool,
            submenu: Vec<ISerializableContextMenuItem>,
        }
    }
}

/// `IContextMenuEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IContextMenuEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctrl_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_key: Option<bool>,
}

/// `click?: (event: IContextMenuEvent) => void`.
pub type ContextMenuClick = std::rc::Rc<dyn Fn(&IContextMenuEvent)>;

/// `IContextMenuItem`: o item com a função de clique.
#[derive(Clone, Default)]
pub struct IContextMenuItem {
    pub label: Option<String>,
    pub r#type: Option<ContextMenuItemType>,
    pub accelerator: Option<String>,
    pub enabled: Option<bool>,
    pub visible: Option<bool>,
    pub checked: Option<bool>,
    pub click: Option<ContextMenuClick>,
    pub submenu: Option<Vec<IContextMenuItem>>,
}

/// `IPopupOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IPopupOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positioning_item: Option<f64>,
}

pub const CONTEXT_MENU_CHANNEL: &str = "vscode:contextmenu";
pub const CONTEXT_MENU_CLOSE_CHANNEL: &str = "vscode:onCloseContextMenu";
