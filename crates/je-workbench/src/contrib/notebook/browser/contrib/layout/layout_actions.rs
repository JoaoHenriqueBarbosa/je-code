//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/layout/layoutActions.ts`: a ação que alterna a posição da barra de
//! ferramentas das células (`ToggleCellToolbarPositionAction`).
//!
//! Divergências:
//! - `Action2` passa o contexto da ação como argumentos `Dynamic`, que não carregam o `INotebookActionContext` tipado (o editor): o
//!   corpo de `run` é [`ToggleCellToolbarPositionAction::run_with_context`], que recebe o contexto e o serviço de configuração, e a
//!   descrição (`super({...})`) é [`ToggleCellToolbarPositionAction::desc`]. O registro (`registerAction2`) fica com quem monta o
//!   contexto da ação.
//! - `NOTEBOOK_ACTIONS_CATEGORY` (de `coreActions.ts`, não portado) é [`notebook_actions_category`].
//! - `string | { [key: string]: string }` é o [`ToolbarPosition`], e `{ [key: string]: string }` é um `Vec` de pares na ordem de
//!   inserção do objeto.
//! - `await configurationService.updateValue(...)` é a promessa devolvida.

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_platform::action::{Category, CommandAction, CommandActionTitle, LocalizedString, Title, localize2};
use je_platform::actions::actions::{Action2MenuItem, Action2Options};
use je_platform::actions::menu_ids;
use je_platform::configuration::service::IConfigurationService;

use crate::contrib::notebook::browser::controller::INotebookActionContext;
use crate::contrib::notebook::common::notebook_common::NotebookSetting;

const TOGGLE_CELL_TOOLBAR_POSITION: &str = "notebook.toggleCellToolbarPosition";

/// `NOTEBOOK_ACTIONS_CATEGORY`.
pub fn notebook_actions_category() -> LocalizedString {
    localize2("notebookActions.category", "Notebook")
}

/// `{ [key: string]: string }`, na ordem de inserção.
pub type ToolbarPositionMap = Vec<(String, String)>;

/// `string | { [key: string]: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarPosition {
    Legacy(String),
    PerViewType(ToolbarPositionMap),
}

impl ToolbarPosition {
    /// O valor da configuração como as duas formas do original (`undefined` e outros tipos não ocorrem na configuração validada).
    pub fn from_dynamic(value: &Dynamic) -> ToolbarPosition {
        match value {
            Dynamic::Object(object) => ToolbarPosition::PerViewType(
                object
                    .entries()
                    .into_iter()
                    .filter_map(|(key, value)| match value {
                        Dynamic::String(value) => Some((key.to_string_lossy(), value.to_string_lossy())),
                        _ => None,
                    })
                    .collect(),
            ),
            Dynamic::String(value) => ToolbarPosition::Legacy(value.to_string_lossy()),
            _ => ToolbarPosition::Legacy(String::new()),
        }
    }
}

fn set(config: &mut ToolbarPositionMap, key: &str, value: &str) {
    match config.iter_mut().find(|(existing, _)| existing == key) {
        Some(slot) => slot.1 = value.to_string(),
        None => config.push((key.to_string(), value.to_string())),
    }
}

/// `ToggleCellToolbarPositionAction`.
pub struct ToggleCellToolbarPositionAction;

impl ToggleCellToolbarPositionAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        let title = notebook_title();
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(notebook_actions_category())),
                ..CommandAction::new(TOGGLE_CELL_TOOLBAR_POSITION, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }))
            },
            menu: vec![Action2MenuItem {
                id: menu_ids::notebook_cell_title(),
                null_precondition: false,
                alt: None,
                when: None,
                group: Some("View".to_string()),
                order: Some(1.0),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    /// `run(accessor, context)`.
    pub fn run_with_context(&self, configuration_service: &dyn IConfigurationService, context: Option<&INotebookActionContext>) -> Promise<()> {
        let editor = context.filter(|context| context.ui == Some(true)).map(|context| &context.notebook_editor);
        if let Some(editor) = editor {
            // from toolbar
            let view_type = editor.text_model().notebook_type();
            let toolbar_position = ToolbarPosition::from_dynamic(&configuration_service.get_value(Some(NotebookSetting::CELL_TOOLBAR_LOCATION), None));
            let new_config = self.toggle_position(&view_type, &toolbar_position);
            let value = Dynamic::object(new_config.into_iter().map(|(key, value)| (key, Dynamic::string(&value))));
            return configuration_service.update_value(NotebookSetting::CELL_TOOLBAR_LOCATION, value, None, None);
        }
        Promise::resolved(())
    }

    /// `togglePosition(viewType, toolbarPosition)`.
    pub fn toggle_position(&self, view_type: &str, toolbar_position: &ToolbarPosition) -> ToolbarPositionMap {
        match toolbar_position {
            ToolbarPosition::Legacy(toolbar_position) => {
                // legacy
                if ["left", "right", "hidden"].contains(&toolbar_position.as_str()) {
                    // valid position
                    let new_view_value = if toolbar_position == "right" { "left" } else { "right" };
                    let mut config: ToolbarPositionMap = vec![("default".to_string(), toolbar_position.clone())];
                    set(&mut config, view_type, new_view_value);
                    config
                } else {
                    // invalid position
                    let mut config: ToolbarPositionMap = vec![("default".to_string(), "right".to_string())];
                    set(&mut config, view_type, "left");
                    config
                }
            }
            ToolbarPosition::PerViewType(toolbar_position) => {
                let lookup = |key: &str| toolbar_position.iter().find(|(existing, _)| existing == key).map(|(_, value)| value.as_str());
                let old_value = lookup(view_type).or_else(|| lookup("default")).unwrap_or("right");
                let new_view_value = if old_value == "right" { "left" } else { "right" };
                let mut new_config = toolbar_position.clone();
                set(&mut new_config, view_type, new_view_value);
                new_config
            }
        }
    }
}

/// `localize2('notebook.toggleCellToolbarPosition', 'Toggle Cell Toolbar Position')`.
fn notebook_title() -> LocalizedString {
    localize2("notebook.toggleCellToolbarPosition", "Toggle Cell Toolbar Position")
}
