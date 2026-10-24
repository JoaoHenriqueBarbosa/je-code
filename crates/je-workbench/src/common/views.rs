//! Porte parcial de `src/vs/workbench/common/views.ts`: os tipos da árvore de visões que o host de extensões
//! (`extHostTreeViews.ts`) e o protocolo (`extHost.protocol.ts`) usam: `ITreeItem`, `TreeItemCollapsibleState`,
//! `ITreeItemLabel`, `TreeCommand`, `ITreeItemCheckboxState`, `IRevealOptions`, `IViewBadge` e o `NoTreeViewError`.
//!
//! Divergências:
//! - É um porte parcial: o resto de `views.ts` (descritores, containers, `ViewContainerLocation`, `ITreeView`,
//!   `ResolvableTreeItem`, `ITreeViewDataProvider`, `ITreeViewDragAndDropController`...) é da camada de interface e
//!   entra junto do porte dela. `TreeViewPaneHandleArg` e `TreeViewItemHandleArg` (os `$treeViewId` e
//!   `$treeItemHandle` que o `processArgument` do host lê) chegam como [`Dynamic`], então não têm struct aqui.
//! - `UriComponents` é [`URI`], `ThemeIcon` é o de `je-base`, `IMarkdownString | string` é [`StringOrMarkdown`] e o
//!   `Command & { originalId?: string }` de `TreeCommand` é o [`ICommandDto`] do protocolo mais `original_id`.
//! - `IAccessibilityInformation` (`platform/accessibility/common/accessibility.ts`) é definido aqui, como
//!   [`AccessibilityInformation`], até esse arquivo ser portado.
//! - `ITreeItem.collapsibleState` é o enum [`TreeItemCollapsibleState`]. O original aceita qualquer número que a
//!   extensão devolver; o host de Rust lê um valor que não seja `0`, `1` ou `2` como `None`.
//! - `ITreeItem.description` é `string | boolean` ([`TreeItemDescription`]) e `IRevealOptions.expand` é
//!   `boolean | number` ([`RevealExpand`]).
//! - [`ITreeItem::to_dynamic`] não existe no original: é o objeto JavaScript do item sem as propriedades
//!   `undefined`, que é o que o `removeUnsetKeys` da suíte de testes calcula (e o que o RPC serializaria). Os
//!   `UriComponents` e o `ThemeIcon` saem como os valores de classe de `ext_host_values`.

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::html_content::{IMarkdownString, IsTrusted};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;

use crate::api::common::ext_host_protocol::ICommandDto;
use crate::api::common::ext_host_values::ApiValue;

/// `IViewBadge`.
#[derive(Clone, Debug, PartialEq)]
pub struct IViewBadge {
    pub tooltip: String,
    pub value: f64,
}

/// `IAccessibilityInformation`: `{ label: string; role?: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityInformation {
    pub label: String,
    pub role: Option<String>,
}

/// `IRevealOptions.expand`: `boolean | number`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RevealExpand {
    Bool(bool),
    Depth(f64),
}

/// `IRevealOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IRevealOptions {
    pub select: Option<bool>,
    pub focus: Option<bool>,
    pub expand: Option<RevealExpand>,
}

/// `TreeItemCollapsibleState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TreeItemCollapsibleState {
    #[default]
    None = 0,
    Collapsed = 1,
    Expanded = 2,
}

impl TreeItemCollapsibleState {
    /// O valor numérico do membro.
    pub fn value(self) -> i32 {
        self as i32
    }

    /// O membro de um valor numérico, se existir.
    pub fn from_value(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Collapsed),
            2 => Some(Self::Expanded),
            _ => None,
        }
    }
}

/// `ITreeItemLabel`.
#[derive(Clone, Debug)]
pub struct ITreeItemLabel {
    pub label: StringOrMarkdown,
    pub highlights: Option<Vec<(f64, f64)>>,
}

/// `TreeCommand`: `Command & { originalId?: string }`.
#[derive(Clone, Debug)]
pub struct TreeCommand {
    pub command: ICommandDto,
    pub original_id: Option<String>,
}

/// `ITreeItemCheckboxState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITreeItemCheckboxState {
    pub is_checked: bool,
    pub tooltip: Option<String>,
    pub accessibility_information: Option<AccessibilityInformation>,
}

/// `ITreeItem.description`: `string | boolean`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeItemDescription {
    Text(String),
    Bool(bool),
}

/// `ITreeItem`.
#[derive(Clone, Debug, Default)]
pub struct ITreeItem {
    pub handle: String,
    pub parent_handle: Option<String>,
    pub collapsible_state: TreeItemCollapsibleState,
    pub label: Option<ITreeItemLabel>,
    pub description: Option<TreeItemDescription>,
    pub icon: Option<URI>,
    pub icon_dark: Option<URI>,
    pub theme_icon: Option<ThemeIcon>,
    pub resource_uri: Option<URI>,
    pub tooltip: Option<StringOrMarkdown>,
    pub context_value: Option<String>,
    pub command: Option<TreeCommand>,
    pub children: Option<Vec<ITreeItem>>,
    pub parent: Option<Box<ITreeItem>>,
    pub accessibility_information: Option<AccessibilityInformation>,
    pub checkbox: Option<ITreeItemCheckboxState>,
}

/// O objeto JavaScript só com as propriedades definidas (uma propriedade `undefined` não existe).
fn object_of(entries: Vec<(&str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
}

fn markdown_to_dynamic(markdown: &IMarkdownString) -> Dynamic {
    let is_trusted = markdown.is_trusted.as_ref().map(|trusted| match trusted {
        IsTrusted::Bool(flag) => Dynamic::Bool(*flag),
        IsTrusted::Options(options) => Dynamic::object([("enabledCommands", Dynamic::array(options.enabled_commands.iter().map(|command| Dynamic::string(command))))]),
    });
    object_of(vec![
        ("value", Some(Dynamic::String(markdown.value.clone()))),
        ("isTrusted", is_trusted),
        ("supportThemeIcons", markdown.support_theme_icons.map(Dynamic::Bool)),
        ("supportHtml", markdown.support_html.map(Dynamic::Bool)),
        ("supportAlertSyntax", markdown.support_alert_syntax.map(Dynamic::Bool)),
        ("baseUri", markdown.base_uri.as_ref().map(ApiValue::to_dynamic)),
    ])
}

fn string_or_markdown_to_dynamic(value: &StringOrMarkdown) -> Dynamic {
    match value {
        StringOrMarkdown::String(text) => Dynamic::string(text),
        StringOrMarkdown::Markdown(markdown) => markdown_to_dynamic(markdown),
    }
}

fn accessibility_to_dynamic(information: &AccessibilityInformation) -> Dynamic {
    object_of(vec![("label", Some(Dynamic::string(&information.label))), ("role", information.role.as_deref().map(Dynamic::string))])
}

impl TreeCommand {
    /// O objeto do comando: o [`ICommandDto`] (com o `$ident`) e o `originalId`.
    pub fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("$ident", self.command.ident.as_deref().map(Dynamic::string)),
            ("id", Some(Dynamic::string(&self.command.id))),
            ("title", Some(Dynamic::string(&self.command.title))),
            ("tooltip", self.command.tooltip.as_deref().map(Dynamic::string)),
            ("arguments", self.command.arguments.as_ref().map(|arguments| Dynamic::array(arguments.iter().cloned()))),
            ("originalId", self.original_id.as_deref().map(Dynamic::string)),
        ])
    }
}

impl ITreeItemLabel {
    fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("label", Some(string_or_markdown_to_dynamic(&self.label))),
            (
                "highlights",
                self.highlights.as_ref().map(|highlights| {
                    Dynamic::array(highlights.iter().map(|(start, end)| Dynamic::array([Dynamic::Number(*start), Dynamic::Number(*end)])))
                }),
            ),
        ])
    }
}

impl ITreeItem {
    /// O item como objeto JavaScript, sem as propriedades `undefined` (ver as divergências do módulo).
    pub fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("handle", Some(Dynamic::string(&self.handle))),
            ("parentHandle", self.parent_handle.as_deref().map(Dynamic::string)),
            ("collapsibleState", Some(Dynamic::from(self.collapsible_state.value()))),
            ("label", self.label.as_ref().map(ITreeItemLabel::to_dynamic)),
            (
                "description",
                self.description.as_ref().map(|description| match description {
                    TreeItemDescription::Text(text) => Dynamic::string(text),
                    TreeItemDescription::Bool(flag) => Dynamic::Bool(*flag),
                }),
            ),
            ("icon", self.icon.as_ref().map(ApiValue::to_dynamic)),
            ("iconDark", self.icon_dark.as_ref().map(ApiValue::to_dynamic)),
            ("themeIcon", self.theme_icon.as_ref().map(ApiValue::to_dynamic)),
            ("resourceUri", self.resource_uri.as_ref().map(ApiValue::to_dynamic)),
            ("tooltip", self.tooltip.as_ref().map(string_or_markdown_to_dynamic)),
            ("contextValue", self.context_value.as_deref().map(Dynamic::string)),
            ("command", self.command.as_ref().map(TreeCommand::to_dynamic)),
            ("children", self.children.as_ref().map(|children| Dynamic::array(children.iter().map(ITreeItem::to_dynamic)))),
            ("parent", self.parent.as_ref().map(|parent| parent.to_dynamic())),
            ("accessibilityInformation", self.accessibility_information.as_ref().map(accessibility_to_dynamic)),
            (
                "checkbox",
                self.checkbox.as_ref().map(|checkbox| {
                    object_of(vec![
                        ("isChecked", Some(Dynamic::Bool(checkbox.is_checked))),
                        ("tooltip", checkbox.tooltip.as_deref().map(Dynamic::string)),
                        ("accessibilityInformation", checkbox.accessibility_information.as_ref().map(accessibility_to_dynamic)),
                    ])
                }),
            ),
        ])
    }
}

/// O nome do `NoTreeViewError`.
pub const NO_TREE_VIEW_ERROR_NAME: &str = "NoTreeViewError";

/// `new NoTreeViewError(treeViewId)`.
pub fn no_tree_view_error(tree_view_id: &str) -> Error {
    Error::named(NO_TREE_VIEW_ERROR_NAME, localize("treeView.notRegistered", "No tree view with id '{0}' registered.", &[tree_view_id]))
}

/// `NoTreeViewError.is(err)`.
pub fn is_no_tree_view_error(error: &Error) -> bool {
    error.name == NO_TREE_VIEW_ERROR_NAME
}
