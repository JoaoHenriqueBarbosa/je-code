//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellEditorOptions.ts`: `CellEditorOptions`, as opções do editor
//! de uma célula (as do editor base mais o padding, os números de linha da célula e as réguas), a configuração `notebook.lineNumbers`
//! e as duas ações de números de linha (`notebook.toggleLineNumbers` e `notebook.cell.toggleLineNumbers`).
//!
//! Divergências:
//! - `IEditorOptions` é um objeto [`Dynamic`] (o mesmo modelo de `je_editor::config`), e `{ ...value, ...overrides }` é
//!   [`merge_options`] (as chaves de `overrides` substituem no lugar, as novas entram no fim, como o spread).
//! - `IBaseCellEditorOptions` completo (`value`, `onDidChange`) é o contrato [`ICellEditorBaseOptions`], que estende o
//!   `IBaseCellEditorOptions` dos view models (só com `wordWrap`).
//! - `implements ITextModelUpdateOptions` é [`CellEditorOptions::text_model_update_options`], que devolve o `tabSize`, o `indentSize` e o
//!   `insertSpaces` atuais. O valor de `editor.indentSize` das customizações é um número ou o texto `tabSize`; outros valores (que o
//!   TypeScript aceitaria sem checagem) são ignorados, e o mesmo vale para `editor.tabSize` não numérico e `editor.insertSpaces` não
//!   booleano.
//! - `registerConfiguration` no carregamento do módulo é [`register_configuration`] e `registerAction2(ToggleLineNumberAction)` é
//!   [`register_actions`]; quem monta o workbench chama as duas.
//! - `ToggleActiveLineNumberAction` estende `NotebookMultiCellAction` (de `coreActions.ts`, sem porte), então o corpo de `runWithContext`
//!   é [`ToggleActiveLineNumberAction::run_with_context`] e a descrição é [`ToggleActiveLineNumberAction::desc`]; o registro fica com
//!   quem monta o contexto da ação (o mesmo molde de `layout_actions.rs`). `ActiveEditorContext.isEqualTo(NOTEBOOK_EDITOR_ID)` é
//!   `active_editor_context().is_equal_to(...)`.
//! - `INotebookCellToolbarActionContext` é o [`INotebookActionContext`] (com `cell` e `ui`).
//! - `await configurationService.updateValue(...)` da ação de alternância global é a promessa descartada (o `run` do `Action2` do porte é
//!   síncrono).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::uri::URI;
use je_editor::model::{IndentSize, TextModelUpdateOptions};
use je_instantiation::ServicesAccessor;
use je_platform::action::{Category, CommandAction, CommandActionTitle, CommandActionToggleInfo, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2MenuItem, Action2Options, register_action2};
use je_platform::actions::menu_ids;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, PropertySchema, configuration_registry};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression};

use super::cell_context_keys::{notebook_cell_line_numbers, notebook_editor_focused};
use crate::common::contextkeys::active_editor_context;
use crate::contrib::notebook::browser::contrib::layout::layout_actions::notebook_actions_category;
use crate::contrib::notebook::browser::controller::INotebookActionContext;
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::base_cell_view_model::LineNumbers;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::browser::view_model::view_context::IBaseCellEditorOptions;
use crate::contrib::notebook::common::notebook_common::{NOTEBOOK_EDITOR_ID, NotebookCellInternalMetadata};

/// A chave de configuração `notebook.lineNumbers`.
pub const NOTEBOOK_LINE_NUMBERS_SETTING: &str = "notebook.lineNumbers";

/// `IBaseCellEditorOptions` (de `notebookBrowser.ts`) completo: o valor das opções base e o aviso de mudança.
pub trait ICellEditorBaseOptions: IBaseCellEditorOptions {
    /// `value`.
    fn value(&self) -> Dynamic;
    /// `onDidChange`.
    fn on_did_change(&self) -> Event<()>;
}

/// `{ ...value, ...overrides }`: uma cópia rasa de `value` (um objeto, ou nada) com as chaves de `overrides` por cima.
pub fn merge_options(value: &Dynamic, overrides: impl IntoIterator<Item = (&'static str, Dynamic)>) -> Dynamic {
    let merged = ObjectRef::new();
    for (key, entry) in value.own_entries() {
        merged.set(&key, entry);
    }
    for (key, entry) in overrides {
        merged.set(&JsString::from(key), entry);
    }
    Dynamic::Object(merged)
}

/// `{ top, bottom }` do `padding`.
fn padding_option(top: f64, bottom: f64) -> Dynamic {
    Dynamic::object([("top", Dynamic::Number(top)), ("bottom", Dynamic::Number(bottom))])
}

fn is_string(value: &Dynamic, expected: &str) -> bool {
    matches!(value, Dynamic::String(text) if text.to_string_lossy() == expected)
}

/// `configurationService.getValue<'on' | 'off'>('notebook.lineNumbers') === 'on'`.
fn notebook_line_numbers_is_on(configuration_service: &dyn IConfigurationService) -> bool {
    is_string(&configuration_service.get_value(Some(NOTEBOOK_LINE_NUMBERS_SETTING), None), "on")
}

/// `CellEditorOptions`.
pub struct CellEditorOptions {
    state: CellPartState,
    base: Rc<dyn ICellEditorBaseOptions>,
    /// `notebookOptions`.
    pub notebook_options: Rc<NotebookOptions>,
    /// `configurationService`.
    pub configuration_service: Rc<dyn IConfigurationService>,
    line_numbers: Cell<LineNumbers>,
    tab_size: Cell<Option<i32>>,
    indent_size: Cell<Option<IndentSize>>,
    insert_spaces: Cell<Option<bool>>,
    on_did_change: Rc<Emitter<()>>,
    value: RefCell<Dynamic>,
}

impl CellEditorOptions {
    pub fn new(base: Rc<dyn ICellEditorBaseOptions>, notebook_options: Rc<NotebookOptions>, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let state = CellPartState::new();
        let on_did_change = Emitter::<()>::new();
        state.register(on_did_change.clone());

        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let me = me.clone();
            state.register(base.on_did_change().listen(move |_| {
                if let Some(this) = me.upgrade() {
                    this.recompute_options();
                }
            }));

            Self {
                state,
                base: base.clone(),
                notebook_options,
                configuration_service,
                line_numbers: Cell::new(LineNumbers::Inherit),
                tab_size: Cell::new(None),
                indent_size: Cell::new(None),
                insert_spaces: Cell::new(None),
                on_did_change,
                value: RefCell::new(Dynamic::Undefined),
            }
        });

        let value = this.compute_editor_options();
        *this.value.borrow_mut() = value;
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `get tabSize()`.
    pub fn tab_size(&self) -> Option<i32> {
        self.tab_size.get()
    }

    /// `set tabSize(value)`.
    pub fn set_tab_size(&self, value: Option<i32>) {
        if self.tab_size.get() != value {
            self.tab_size.set(value);
            self.on_did_change.fire(());
        }
    }

    /// `get indentSize()`.
    pub fn indent_size(&self) -> Option<IndentSize> {
        self.indent_size.get()
    }

    /// `set indentSize(value)`.
    pub fn set_indent_size(&self, value: Option<IndentSize>) {
        if self.indent_size.get() != value {
            self.indent_size.set(value);
            self.on_did_change.fire(());
        }
    }

    /// `get insertSpaces()`.
    pub fn insert_spaces(&self) -> Option<bool> {
        self.insert_spaces.get()
    }

    /// `set insertSpaces(value)`.
    pub fn set_insert_spaces(&self, value: Option<bool>) {
        if self.insert_spaces.get() != value {
            self.insert_spaces.set(value);
            self.on_did_change.fire(());
        }
    }

    /// O `ITextModelUpdateOptions` que a classe implementa: `tabSize`, `indentSize` e `insertSpaces`.
    pub fn text_model_update_options(&self) -> TextModelUpdateOptions {
        TextModelUpdateOptions { tab_size: self.tab_size.get(), indent_size: self.indent_size.get(), insert_spaces: self.insert_spaces.get(), ..Default::default() }
    }

    fn recompute_options(&self) {
        let value = self.compute_editor_options();
        *self.value.borrow_mut() = value;
        self.on_did_change.fire(());
    }

    fn compute_editor_options(&self) -> Dynamic {
        let value = self.base.value(); // base IEditorOptions

        // TODO @Yoyokrazy find a different way to get the editor overrides, this is not the right way
        let cell_editor_overrides_raw = self.notebook_options.get_display_options().editor_options_customizations;
        match cell_editor_overrides_raw.optional_get_str("editor.indentSize") {
            Dynamic::Number(size) => self.set_indent_size(Some(IndentSize::Size(size as i32))),
            indent_size if is_string(&indent_size, "tabSize") => self.set_indent_size(Some(IndentSize::TabSize)),
            _ => {}
        }
        if let Dynamic::Bool(insert_spaces) = cell_editor_overrides_raw.optional_get_str("editor.insertSpaces") {
            self.set_insert_spaces(Some(insert_spaces));
        }
        if let Dynamic::Number(tab_size) = cell_editor_overrides_raw.optional_get_str("editor.tabSize") {
            self.set_tab_size(Some(tab_size as i32));
        }

        let value_line_numbers = value.optional_get_str("lineNumbers");
        let mut cell_render_line_number = value_line_numbers.clone();

        match self.line_numbers.get() {
            LineNumbers::Inherit => {
                // inherit from the notebook setting
                if notebook_line_numbers_is_on(&*self.configuration_service) {
                    if is_string(&value_line_numbers, "off") {
                        cell_render_line_number = Dynamic::string("on");
                    } // otherwise just use the editor setting
                } else {
                    cell_render_line_number = Dynamic::string("off");
                }
            }
            LineNumbers::On => {
                // should turn on, ignore the editor line numbers off options
                if is_string(&value_line_numbers, "off") {
                    cell_render_line_number = Dynamic::string("on");
                } // otherwise just use the editor setting
            }
            LineNumbers::Off => {
                cell_render_line_number = Dynamic::string("off");
            }
        }

        let mut overrides: Vec<(&'static str, Dynamic)> = Vec::new();
        if !value_line_numbers.strict_equals(&cell_render_line_number) {
            overrides.push(("lineNumbers", cell_render_line_number));
        }

        if self.notebook_options.get_layout_configuration().disable_rulers == Some(true) {
            overrides.push(("rulers", Dynamic::array([])));
        }

        merge_options(&value, overrides)
    }

    /// `getUpdatedValue(internalMetadata, cellUri)`.
    pub fn get_updated_value(&self, internal_metadata: &NotebookCellInternalMetadata, cell_uri: &URI) -> Dynamic {
        let options = self.get_value(internal_metadata, cell_uri);
        if let Dynamic::Object(object) = &options {
            object.delete(&JsString::from("hover")); // This is toggled by a debug editor contribution
        }

        options
    }

    /// `getValue(internalMetadata, cellUri)`.
    pub fn get_value(&self, internal_metadata: &NotebookCellInternalMetadata, cell_uri: &URI) -> Dynamic {
        let padding = self.notebook_options.compute_editor_padding(internal_metadata, cell_uri);
        merge_options(&self.value.borrow(), [("padding", padding_option(padding.top, padding.bottom))])
    }

    /// `getDefaultValue()`.
    pub fn get_default_value(&self) -> Dynamic {
        merge_options(&self.value.borrow(), [("padding", padding_option(12.0, 12.0))])
    }

    /// `setLineNumbers(lineNumbers)`.
    pub fn set_line_numbers(&self, line_numbers: LineNumbers) {
        self.line_numbers.set(line_numbers);
        self.recompute_options();
    }
}

impl CellPart for CellEditorOptions {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn update_state(&self, element: &CellViewModel, e: &CellViewModelStateChangeEvent) {
        if e.cell_line_number_changed {
            self.set_line_numbers(element.base().line_numbers());
        }
    }
}

impl_cell_content_part!(CellEditorOptions, "CellEditorOptions");

/// O `registerConfiguration` do fim do arquivo original: a configuração `notebook.lineNumbers`.
pub fn register_configuration() {
    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData { id: Some("notebook".to_owned()), order: Some(100.0), type_: Some(Dynamic::string("object")), ..Default::default() }.with_properties([(
            NOTEBOOK_LINE_NUMBERS_SETTING,
            PropertySchema::from_entries([
                ("type", Dynamic::string("string")),
                ("enum", Dynamic::array([Dynamic::string("off"), Dynamic::string("on")])),
                ("default", Dynamic::string("off")),
                ("markdownDescription", Dynamic::string(&localize("notebook.lineNumbers", "Controls the display of line numbers in the cell editor.", &[]))),
            ]),
        )]),
    ));
}

/// `registerAction2(ToggleLineNumberAction)`.
pub fn register_actions() -> DisposableRef {
    register_action2(Rc::new(ToggleLineNumberAction))
}

fn title_of(localized: je_platform::action::LocalizedString) -> Title {
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// `ToggleLineNumberAction`.
pub struct ToggleLineNumberAction;

impl Action2 for ToggleLineNumberAction {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: CommandAction {
                short_title: Some(title_of(localize2("notebook.toggleLineNumbers.short", "Line Numbers"))),
                precondition: Some(notebook_editor_focused().expr()),
                category: Some(Category::Localized(notebook_actions_category())),
                toggled: Some(Toggled::Info(CommandActionToggleInfo {
                    condition: ContextKeyExpr::not_equals("config.notebook.lineNumbers", "off"),
                    icon: None,
                    tooltip: None,
                    title: Some(localize("notebook.showLineNumbers", "Line Numbers", &[])),
                    mnemonic_title: None,
                })),
                ..CommandAction::new("notebook.toggleLineNumbers", title_of(localize2("notebook.toggleLineNumbers", "Toggle Notebook Line Numbers")))
            },
            menu: vec![Action2MenuItem {
                id: menu_ids::notebook_toolbar(),
                null_precondition: false,
                alt: None,
                when: Some(ContextKeyExpr::equals("config.notebook.globalToolbar", true)),
                group: Some("notebookLayout".to_owned()),
                order: Some(2.0),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: true,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);
        let render_line_numbers = notebook_line_numbers_is_on(&*configuration_service);

        let new_value = if render_line_numbers { "off" } else { "on" };
        let _ = configuration_service.update_value(NOTEBOOK_LINE_NUMBERS_SETTING, Dynamic::string(new_value), None, None);
    }
}

/// `ToggleActiveLineNumberAction`.
pub struct ToggleActiveLineNumberAction;

impl ToggleActiveLineNumberAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        let toggled: ContextKeyExpression = ContextKeyExpr::or([
            notebook_cell_line_numbers().is_equal_to("on"),
            ContextKeyExpr::and([notebook_cell_line_numbers().is_equal_to("inherit"), ContextKeyExpr::equals("config.notebook.lineNumbers", "on")])
                .expect("a conjunção tem termos"),
        ])
        .expect("a disjunção tem termos");

        Action2Options {
            command: CommandAction {
                precondition: Some(active_editor_context().is_equal_to(NOTEBOOK_EDITOR_ID)),
                toggled: Some(Toggled::Expression(toggled)),
                ..CommandAction::new("notebook.cell.toggleLineNumbers", Title::Plain(localize("notebook.cell.toggleLineNumbers.title", "Show Cell Line Numbers", &[])))
            },
            menu: vec![Action2MenuItem {
                id: menu_ids::notebook_cell_title(),
                null_precondition: false,
                alt: None,
                when: None,
                group: Some("View".to_owned()),
                order: Some(1.0),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    /// `runWithContext(accessor, context)`.
    pub fn run_with_context(&self, configuration_service: &dyn IConfigurationService, context: &INotebookActionContext) {
        if context.ui == Some(true) {
            if let Some(cell) = &context.cell {
                self.update_cell(configuration_service, cell);
            }
        } else if let Some(selected_cells) = &context.selected_cells {
            for cell in selected_cells {
                self.update_cell(configuration_service, cell);
            }
        }
    }

    fn update_cell(&self, configuration_service: &dyn IConfigurationService, cell: &CellViewModel) {
        let render_line_numbers = notebook_line_numbers_is_on(configuration_service);
        let cell_line_numbers = cell.base().line_numbers();
        // 'on', 'inherit' 	-> 'on'
        // 'on', 'off'		-> 'off'
        // 'on', 'on'		-> 'on'
        // 'off', 'inherit'	-> 'off'
        // 'off', 'off'		-> 'off'
        // 'off', 'on'		-> 'on'
        let current_line_number_is_on = cell_line_numbers == LineNumbers::On || (cell_line_numbers == LineNumbers::Inherit && render_line_numbers);

        if current_line_number_is_on {
            cell.base().set_line_numbers(LineNumbers::Off);
        } else {
            cell.base().set_line_numbers(LineNumbers::On);
        }
    }
}
