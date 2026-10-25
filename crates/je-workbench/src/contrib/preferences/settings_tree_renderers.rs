//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/settingsTree.ts`, linhas 856 a 1327 (e o que o template
//! delas usa de fora dessa faixa): a base [`AbstractSettingRenderer`] (o template comum com título, categoria, descrição em
//! Markdown, indicadores, barra de ferramentas, mensagem de depreciação, `renderSettingElement`, o rastreio de foco, a ordem de
//! tabulação e os eventos), o [`SettingGroupRenderer`], o [`SettingNewExtensionsRenderer`], o [`SettingComplexRenderer`] e o
//! [`SettingComplexObjectRenderer`], com os tipos de template, os eventos (`ISettingChangeEvent`, `ISettingLinkClickEvent`,
//! `HeightChangeParams`) e os identificadores de template (`SETTINGS_*_TEMPLATE_ID`, linhas 792 a 806).
//!
//! Ficam para as fatias seguintes (cada uma entra com a sua parte de `settingsTree.ts`): os renderizadores de array, de objeto,
//! de include e exclude, de texto, de enum, de número, de bool e do toggle de extensão (`SettingArrayRenderer` em diante), o
//! delegado de alturas, o provedor de acessibilidade (o `getAriaLabel` da linha, linhas 2640 a 2665) e o `SettingsTree`. Os
//! serviços que só esses renderizadores leem (`IExtensionsWorkbenchService`, `ITelemetryService`, `IExtensionService`,
//! `IProductService`) entram no [`SettingRendererServices`] com a fatia que os usa.
//!
//! De fora da faixa, porque o template da faixa os chama: `cleanRenderedMarkdown` (em `settings_markdown_view`),
//! `fixSettingLinks` (linhas 2426 a 2435, [`fix_setting_links`]; não é o homônimo de `preferences_models.rs`, que é o de
//! `preferencesModels.ts`), `getObjectDisplayValue` com os auxiliares que ele usa (linhas 124 a 256,
//! [`get_object_display_value`]), `renderValidations` do complexo (linhas 1275 a 1284) e `addChildrenToTabOrder` com
//! `removeChildrenFromTabOrder` (linhas 821 a 849). A fatia que cobrir essas linhas reaproveita estes; os outros auxiliares
//! de valor (`getIncludeExcludeDisplayValue`, `getBoolObjectDisplayValue`, os sugeridores, `getListDisplayValue`) não entram.
//!
//! Divergências:
//! - **Herança.** `AbstractSettingRenderer` é uma estrutura concreta, e `SettingComplexRenderer` e `SettingComplexObjectRenderer`
//!   a envolvem (cada um dá `Deref` ao anterior, como o `ObjectSettingDropdownWidget` em `settings_widgets.rs`). O método
//!   abstrato `renderValue` e a chamada virtual dele dentro de `renderSettingElement` são o parâmetro `render_value` de
//!   [`AbstractSettingRenderer::render_setting_element`]; por isso o objeto complexo, que sobrescreve `renderValue`, tem o seu
//!   próprio `render_element` (o herdado chamaria o `renderValue` do pai). O `index` de `renderSettingElement` não é lido e não
//!   existe.
//! - **Serviços.** Os serviços decorados do construtor (`@IThemeService`, `@IContextViewService`, `@IOpenerService`,
//!   `@IInstantiationService`, `@ICommandService`, `@IContextMenuService`, `@IKeybindingService`, `@IConfigurationService`,
//!   `@IHoverService`, `@IMarkdownRendererService`) vão no [`SettingRendererServices`]: o tema vira os [`SettingRendererStyles`]
//!   e os estilos de `SettingsWidgetServices`, o `IContextViewService` e o `IHoverService` são os de `SettingsWidgetServices`, o
//!   `IContextMenuService` é o `IContextMenuProvider` que o `ToolBar` recebe, o `IInstantiationService.createInstance` é a
//!   chamada direta ao construtor, e o `IMarkdownRendererService` é a função `render_markdown` com o
//!   [`OwnedMarkdownEnvironment`] (a plataforma ainda não tem o serviço; ver `settings_markdown_view`).
//! - **Templates.** Um template é um [`TemplateData`] (`Rc<dyn Any>`) que cada renderizador converte de volta com
//!   `downcast_ref`. O `...common` do TypeScript é o campo `common` mais o `Deref`. `context` é um `Rc<RefCell<_>>` porque os
//!   ouvintes criados em `renderTemplate` o leem depois, como leem o objeto `template` no original.
//! - **`isDisposable(action)`.** `IAction` não tem `dispose`: uma ação do `disposableActionFactory` é descartada com o elemento
//!   quando é uma `Action` (`as_base_action`, o `instanceof Action`), o caso em que o original a descarta.
//! - **`isIOS`** é falso (os alvos são Windows, Linux, macOS e web), então `renderDropdownAsChildElement` é sempre verdadeiro.
//! - **`settingsMoreActionIcon`** (`preferencesIcons.ts`, não portado) é o ícone `gear`, o padrão que o original registra.
//! - **Eventos.** Os `Emitter` do original são [`Emitter`] de `je-base`; os protegidos (`_onDidChangeSetting`,
//!   `_onDidOpenSettings`, `_onDidFocusSetting`, `_onDidChangeSettingHeight`, `_onApplyFilter`) são `pub(crate)` para as fatias
//!   seguintes.
//! - **`ISettingChangeEvent.type`** é `SettingValueType | SettingValueType[]`, o [`SettingChangeType`]; o `value: unknown` é um
//!   [`Dynamic`] (`undefined` é reset ou desconfigurar).
//! - **`SettingsTreeGroupChild` como elemento da árvore.** O `ObjectTreeModel` exige `Eq + Hash` do elemento; o original compara
//!   os objetos por identidade, e este módulo dá ao [`SettingsTreeGroupChild`] a igualdade e o hash pelo endereço do `Rc`.
//! - **Layout.** Não há motor de CSS: [`AbstractSettingRenderer::layout_setting_item`] empilha as partes do template com as
//!   medidas de `settingsEditor2.css` (`.monaco-tl-contents` com `max-width: 1400px`, `padding: 0 24px` e `margin: auto`;
//!   `.setting-item-contents` com `padding: 12px 14px 18px` (26 px em `bool` e `list`); título com `padding-bottom: 2px`;
//!   descrição com `margin-top: -1px`; valor com `margin-top: 9px`; depreciação com `margin-top: 3px`; barra de ferramentas
//!   absoluta em `left: -22px; top: 8px`; indicador de modificação absoluto em `left: 5px; top: 15px`). O título não quebra
//!   linha (o original deixa os indicadores flutuarem para a linha de baixo), e os indicadores fluem em linha logo depois do
//!   rótulo. O renderizador de cada valor calcula o tamanho do controle e chama a função, que devolve a altura da linha e a
//!   guarda em [`ISettingItemTemplate::height`]. NÃO MEDIDO contra o oráculo: a altura de uma linha completa.
//! - **Mensagem de validação do complexo.** `position: absolute` abaixo do conteúdo (sem afetar a altura da linha), com a largura
//!   do texto mais o `padding: 5px` e a borda de `1px`.
//! - **Tabulação.** `tabindex` é um atributo do nó (o `je-ui` não modela o foco por tabulação): os elementos "focalizáveis" que
//!   `removeChildrenFromTabOrder` procura (`input`, `select`, `textarea`, `a`, `button`) são os nós de papel `Button`, `Link`,
//!   `TextInput`, `ComboBox`, `ListBox`, `Checkbox`, `Radio` e `Switch` sem `tabindex="-1"`, e os de `tabindex="0"`.
//! - **`getObjectDisplayValue`.** O `filter(item => !isUndefinedOrNull(item.value.data))` final é sempre verdadeiro (`'' + x` e
//!   `!!x` nunca são `undefined`) e não existe; `'' + data` é [`js_text`]; a `RegExp` de um padrão inválido lança `SyntaxError`,
//!   como o `new RegExp` do original.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::actions::ActionRef;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy, number_to_js_string};
use je_base::errors::{self, Error, on_unexpected_error};
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::html_content::{IMarkdownString, IsTrusted};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::markdown::{MarkdownRenderOptions, render_markdown};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_editor::language_service::ILanguageService;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::get_language_tag_setting_plain_key;
use je_platform::configuration::registry::{ConfigurationDefaultSource, ConfigurationDefaultValueSource, ConfigurationScope};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::opener::opener::{AllowCommands, IOpenerService, OpenOptions, OpenResource};
use je_platform::user_data_sync::settings_merge::get_ignored_settings;
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, get_default_ignored_settings};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, Role, Tree, Ui};
use je_widgets::button::{Button, ButtonLabel, ButtonOptions, ButtonStyles, ButtonTitle, IButton};
use je_widgets::context_view_api::IContextMenuProvider;
use je_widgets::dom::{FocusTracker, StandardKeyboardEvent, clear_node, create_el};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{HoverContent, HoverTarget, IHoverOptions};
use je_widgets::list::list_types::TemplateData;
use je_widgets::simple_icon_label::SimpleIconLabel;
use je_widgets::text::{UI_FONT_SIZE, WORKBENCH_LINE_HEIGHT, ellipsize, measure_text, ui_font};
use je_widgets::toolbar::{IToolBarOptions, ToolBar};
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{ITreeRenderer, TreeElementRenderDetails};
use zyarr::RegExp;

use crate::contrib::preferences::common::preferences::{LANGUAGE_SETTING_TAG, SETTINGS_EDITOR_COMMAND_SHOW_CONTEXT_MENU};
use crate::contrib::preferences::preferences_widgets::SettingsTarget;
use crate::contrib::preferences::settings_editor_setting_indicators::{ISettingOverrideClickEvent, SettingsTreeIndicatorsLabel};
use crate::contrib::preferences::settings_markdown_view::{MarkdownView, MarkdownViewSlot, MarkdownViewStyles, OwnedMarkdownEnvironment, clean_rendered_markdown};
use crate::contrib::preferences::settings_tree_models::{
    SettingsTreeElement, SettingsTreeGroupChild, SettingsTreeGroupElement, SettingsTreeNewExtensionsElement, SettingsTreeSettingElement,
    object_setting_supports_remove_default_value, setting_key_to_display_format,
};
use crate::contrib::preferences::settings_widgets::{
    IObjectDataItem, ObjectEnumData, ObjectEnumOption, ObjectKey, ObjectSetValueOptions, ObjectSettingDropdownWidget, ObjectValue, SettingsWidgetServices,
};
use crate::services::configuration::common::configuration::{APPLY_ALL_PROFILES_SETTING, IWorkbenchConfigurationService};
use crate::services::preferences::preferences::{ISetting, SettingSchemaType, SettingValueType};
use crate::services::preferences::preferences_validation::get_invalid_type_error;

// ---- identificadores de template (linhas 792 a 806) ------------------------------------------------------------------------

pub const SETTINGS_TEXT_TEMPLATE_ID: &str = "settings.text.template";
pub const SETTINGS_MULTILINE_TEXT_TEMPLATE_ID: &str = "settings.multilineText.template";
pub const SETTINGS_NUMBER_TEMPLATE_ID: &str = "settings.number.template";
pub const SETTINGS_ENUM_TEMPLATE_ID: &str = "settings.enum.template";
pub const SETTINGS_BOOL_TEMPLATE_ID: &str = "settings.bool.template";
pub const SETTINGS_ARRAY_TEMPLATE_ID: &str = "settings.array.template";
pub const SETTINGS_EXCLUDE_TEMPLATE_ID: &str = "settings.exclude.template";
pub const SETTINGS_INCLUDE_TEMPLATE_ID: &str = "settings.include.template";
pub const SETTINGS_OBJECT_TEMPLATE_ID: &str = "settings.object.template";
pub const SETTINGS_BOOL_OBJECT_TEMPLATE_ID: &str = "settings.boolObject.template";
pub const SETTINGS_COMPLEX_TEMPLATE_ID: &str = "settings.complex.template";
pub const SETTINGS_COMPLEX_OBJECT_TEMPLATE_ID: &str = "settings.complexObject.template";
pub const SETTINGS_NEW_EXTENSIONS_TEMPLATE_ID: &str = "settings.newExtensions.template";
pub const SETTINGS_ELEMENT_TEMPLATE_ID: &str = "settings.group.template";
pub const SETTINGS_EXTENSION_TOGGLE_TEMPLATE_ID: &str = "settings.extensionToggle.template";

// ---- medidas de settingsEditor2.css ---------------------------------------------------------------------------------------

/// `.monaco-tl-contents { max-width: min(100%, 1400px) }`.
const ROW_MAX_WIDTH: f64 = 1400.0;
/// `.monaco-tl-contents { padding-left: 24px; padding-right: 24px }`.
const ROW_PADDING_X: f64 = 24.0;
/// `.setting-item-contents { padding: 12px 14px 18px }`.
const ITEM_PADDING_TOP: f64 = 12.0;
const ITEM_PADDING_X: f64 = 14.0;
const ITEM_PADDING_BOTTOM: f64 = 18.0;
/// `.setting-item-bool .setting-item-contents, .setting-item-list .setting-item-contents { padding-bottom: 26px }`.
const ITEM_PADDING_BOTTOM_TALL: f64 = 26.0;
/// `.setting-item-title { padding-bottom: 2px }`.
const TITLE_PADDING_BOTTOM: f64 = 2.0;
/// `.setting-item-label { margin-right: 7px }`.
const LABEL_MARGIN_RIGHT: f64 = 7.0;
/// `.setting-item-description { margin-top: -1px }`.
const DESCRIPTION_MARGIN_TOP: f64 = -1.0;
/// `.setting-item-value { margin-top: 9px }`.
const VALUE_MARGIN_TOP: f64 = 9.0;
/// `.setting-item-deprecation-message { margin-top: 3px }` e a variante informativa (`--vscode-spacing-size60`).
const DEPRECATION_MARGIN_TOP: f64 = 3.0;
const DEPRECATION_INFO_MARGIN_TOP: f64 = 6.0;
/// O `margin-right: 4px` do codicon da depreciação e o tamanho dele.
const DEPRECATION_ICON_SIZE: f64 = 16.0;
const DEPRECATION_ICON_MARGIN: f64 = 4.0;
/// `.setting-toolbar-container { position: absolute; left: -22px; top: 8px; width: 22px; height: 22px }`.
const TOOLBAR_LEFT: f64 = -22.0;
const TOOLBAR_TOP: f64 = 8.0;
const TOOLBAR_SIZE: f64 = 22.0;
/// `.setting-item-modified-indicator { left: 5px; top: 15px; bottom: 18px; width: 6px; border-left-width: 2px }`.
const MODIFIED_LEFT: f64 = 5.0;
const MODIFIED_TOP: f64 = 15.0;
const MODIFIED_WIDTH: f64 = 6.0;
const MODIFIED_BORDER: f64 = 2.0;
/// `bottom: 23px` do indicador em `bool` e `list`.
const MODIFIED_BOTTOM_TALL: f64 = 23.0;
/// `data-focusable`: a marca de `removeChildrenFromTabOrder`.
const ELEMENT_FOCUSABLE_ATTR: &str = "data-focusable";
/// `.setting-item-validation-message { padding: 5px; border: 1px }`.
const VALIDATION_PADDING: f64 = 5.0;
const VALIDATION_BORDER: f64 = 1.0;
/// `.complex-object-edit-in-settings-button-container { margin-top: 9px }`.
const COMPLEX_BUTTON_MARGIN_TOP: f64 = 9.0;
/// `.settings-group-title-label { padding: 10px; padding-left: 15px }` e os tamanhos por nível.
const GROUP_PADDING: f64 = 10.0;
const GROUP_PADDING_LEFT: f64 = 15.0;
/// `.setting-item-new-extensions .settings-new-extensions-button { margin-bottom: 15px }`.
const NEW_EXTENSIONS_MARGIN_BOTTOM: f64 = 15.0;

/// O `isIOS` do original (ver o `//!`).
const IS_IOS: bool = false;

// ---- eventos e templates -----------------------------------------------------------------------------------------------------

/// `HeightChangeParams`.
#[derive(Clone)]
pub struct HeightChangeParams {
    pub element: Rc<SettingsTreeSettingElement>,
    pub height: f64,
}

/// `SettingValueType | SettingValueType[]`.
#[derive(Clone)]
pub enum SettingChangeType {
    Single(SettingValueType),
    Many(Vec<SettingValueType>),
}

/// `ISettingChangeEvent`.
#[derive(Clone)]
pub struct ISettingChangeEvent {
    pub key: String,
    /// `undefined` é reset ou desconfigurar.
    pub value: Dynamic,
    pub r#type: SettingChangeType,
    pub manual_reset: bool,
    pub scope: Option<ConfigurationScope>,
}

/// `ISettingLinkClickEvent`.
#[derive(Clone)]
pub struct ISettingLinkClickEvent {
    pub source: Rc<SettingsTreeSettingElement>,
    pub target_key: String,
}

/// `ISettingItemTemplate`, com os nós do DOM que o original deixa de fora do template mas o layout precisa (`row_element`,
/// `title_element`, `label_container_element`, `modified_indicator_element`, `value_element`, `toolbar_container`).
pub struct ISettingItemTemplate {
    pub to_dispose: Rc<DisposableStore>,
    pub element_disposables: Rc<DisposableStore>,
    /// `onChange?`.
    pub on_change: RefCell<Option<Rc<dyn Fn(Dynamic)>>>,
    pub context: Rc<RefCell<Option<Rc<SettingsTreeSettingElement>>>>,
    pub row_element: NodeId,
    pub container_element: NodeId,
    pub title_element: NodeId,
    pub category_element: NodeId,
    pub label_container_element: NodeId,
    pub label_element: Rc<SimpleIconLabel>,
    pub description_element: NodeId,
    pub modified_indicator_element: NodeId,
    pub value_element: NodeId,
    pub control_element: NodeId,
    pub deprecation_warning_element: NodeId,
    pub toolbar_container: NodeId,
    pub indicators_label: Rc<SettingsTreeIndicatorsLabel>,
    pub toolbar: Rc<ToolBar>,
    /// A classe de tipo (`setting-item-<type_class>`), que decide o `padding-bottom`.
    pub type_class: String,
    pub description_view: MarkdownViewSlot,
    pub deprecation_view: MarkdownViewSlot,
    /// A altura que o último [`AbstractSettingRenderer::layout_setting_item`] deu à linha.
    pub height: Cell<f64>,
    /// Onde o conteúdo termina (a altura menos o `padding-bottom`).
    pub content_bottom: Cell<f64>,
}

/// `ISettingComplexItemTemplate`.
pub struct ISettingComplexItemTemplate {
    pub common: ISettingItemTemplate,
    pub button: NodeId,
    pub validation_error_message_element: NodeId,
}

impl Deref for ISettingComplexItemTemplate {
    type Target = ISettingItemTemplate;

    fn deref(&self) -> &ISettingItemTemplate {
        &self.common
    }
}

/// `ISettingComplexObjectItemTemplate`.
pub struct ISettingComplexObjectItemTemplate {
    pub complex: ISettingComplexItemTemplate,
    pub object_setting_widget: Rc<ObjectSettingDropdownWidget>,
    /// O `.complex-object-edit-in-settings-button-container` que envolve o botão.
    pub button_container: NodeId,
}

impl Deref for ISettingComplexObjectItemTemplate {
    type Target = ISettingComplexItemTemplate;

    fn deref(&self) -> &ISettingComplexItemTemplate {
        &self.complex
    }
}

/// `ISettingNewExtensionsTemplate`.
pub struct ISettingNewExtensionsTemplate {
    pub to_dispose: Rc<DisposableStore>,
    pub button: Rc<Button>,
    pub context: Rc<RefCell<Option<Rc<SettingsTreeNewExtensionsElement>>>>,
}

/// `IGroupTitleTemplate`.
pub struct IGroupTitleTemplate {
    pub to_dispose: Rc<DisposableStore>,
    pub context: RefCell<Option<Rc<SettingsTreeGroupElement>>>,
    pub parent: NodeId,
}

// ---- o elemento da árvore ----------------------------------------------------------------------------------------------------

/// A identidade de um filho de grupo (o `===` do original): a variante e o endereço do `Rc`.
fn child_identity(child: &SettingsTreeGroupChild) -> (u8, usize) {
    match child {
        SettingsTreeGroupChild::Group(element) => (0, Rc::as_ptr(element) as usize),
        SettingsTreeGroupChild::Setting(element) => (1, Rc::as_ptr(element) as usize),
        SettingsTreeGroupChild::NewExtensions(element) => (2, Rc::as_ptr(element) as usize),
    }
}

impl PartialEq for SettingsTreeGroupChild {
    fn eq(&self, other: &Self) -> bool {
        child_identity(self) == child_identity(other)
    }
}

impl Eq for SettingsTreeGroupChild {}

impl Hash for SettingsTreeGroupChild {
    fn hash<H: Hasher>(&self, state: &mut H) {
        child_identity(self).hash(state);
    }
}

/// O modelo da árvore de configurações: um `ObjectTreeModel` de [`SettingsTreeGroupChild`].
pub type SettingsObjectTreeModel = ObjectTreeModel<SettingsTreeGroupChild>;
/// O nó desse modelo.
pub type SettingsTreeNode = TreeNode<Option<SettingsTreeGroupChild>>;

fn setting_of(node: &SettingsTreeNode) -> Option<&Rc<SettingsTreeSettingElement>> {
    node.element.as_ref()?.as_setting()
}

// ---- estilos e serviços ------------------------------------------------------------------------------------------------------

/// As cores do template, resolvidas do tema (o que o CSS lê com `var(--vscode-...)`).
#[derive(Clone)]
pub struct SettingRendererStyles {
    pub foreground: Rgba,
    /// `--vscode-settings-headerForeground`: o rótulo, a categoria e o título do grupo.
    pub header_foreground: Rgba,
    /// `--vscode-settings-modifiedItemIndicator`.
    pub modified_item_indicator: Option<Rgba>,
    /// `--vscode-settings-rowHoverBackground`.
    pub row_hover_background: Option<Rgba>,
    /// `--vscode-errorForeground`: a mensagem de depreciação.
    pub error_foreground: Rgba,
    /// `--vscode-editorInfo-foreground`: o ícone da depreciação informativa.
    pub info_foreground: Rgba,
    pub validation_error_background: Option<Rgba>,
    pub validation_error_foreground: Rgba,
    pub validation_error_border: Option<Rgba>,
    pub markdown: MarkdownViewStyles,
}

impl SettingRendererStyles {
    pub fn from_theme(theme: &ColorTheme) -> Self {
        let color = |id: &str| theme.get_color(id);
        let foreground = color("foreground").unwrap_or(Rgba::opaque(0xcc, 0xcc, 0xcc));
        Self {
            foreground,
            header_foreground: color("settings.headerForeground").unwrap_or(foreground),
            modified_item_indicator: color("settings.modifiedItemIndicator"),
            row_hover_background: color("settings.rowHoverBackground"),
            error_foreground: color("errorForeground").unwrap_or(Rgba::opaque(0xf4, 0x87, 0x71)),
            info_foreground: color("editorInfo.foreground").unwrap_or(Rgba::opaque(0x37, 0x94, 0xff)),
            validation_error_background: color("inputValidation.errorBackground"),
            validation_error_foreground: color("inputValidation.errorForeground").unwrap_or(foreground),
            validation_error_border: color("inputValidation.errorBorder"),
            markdown: MarkdownViewStyles::from_theme(theme),
        }
    }
}

/// O pacote de serviços do construtor do `AbstractSettingRenderer` (ver o `//!`).
#[derive(Clone)]
pub struct SettingRendererServices {
    /// A janela, o provedor de vista de contexto, o hover e os estilos dos widgets.
    pub widgets: SettingsWidgetServices,
    pub styles: SettingRendererStyles,
    pub opener_service: Rc<dyn IOpenerService>,
    pub command_service: Rc<dyn ICommandService>,
    /// O que o `IContextMenuService` dá ao `ToolBar`.
    pub context_menu_provider: Rc<dyn IContextMenuProvider>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub configuration_service: Rc<dyn IWorkbenchConfigurationService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub markdown_environment: Rc<OwnedMarkdownEnvironment>,
}

impl SettingRendererServices {
    fn ui(&self) -> &Ui {
        &self.widgets.ui
    }
}

/// `this._register(new Emitter<T>())`.
fn registered_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::<T>::new();
    base.register(emitter.clone());
    emitter
}

/// `(setting: ISetting, settingTarget: SettingsTarget) => IAction[]`.
pub type DisposableActionFactory = Rc<dyn Fn(&ISetting, &SettingsTarget) -> Vec<ActionRef>>;

// ---- auxiliares do template -----------------------------------------------------------------------------------------------------

/// A caixa de `.monaco-tl-contents` dentro da linha: o `x` e a largura do que fica depois do `padding`.
fn contents_box(row_width: f64) -> (f64, f64) {
    let content = row_width.min(ROW_MAX_WIDTH);
    ((row_width - content) / 2.0 + ROW_PADDING_X, (content - 2.0 * ROW_PADDING_X).max(0.0))
}

/// A largura do texto de um `.setting-item-contents` numa linha da largura dada.
fn item_text_width(row_width: f64) -> f64 {
    (contents_box(row_width).1 - 2.0 * ITEM_PADDING_X).max(0.0)
}

/// O quanto um nó ocupa na horizontal, pelo conteúdo dele (texto, codicon ou o fim do último filho).
fn node_extent(tree: &Tree, node: NodeId) -> f64 {
    match &tree.style(node).content {
        Content::Text { text, style, .. } => measure_text(text, style),
        Content::Codicon { size, .. } => f64::from(*size),
        _ => tree.children(node).iter().map(|child| tree.bounds(*child).x1).fold(0.0, f64::max),
    }
}

/// Põe os filhos visíveis de `container` em fila, da esquerda para a direita, e devolve a largura. O fluxo em linha do DOM
/// (`display: inline`) dos indicadores, que o `je-ui` não tem.
fn flow_inline(tree: &mut Tree, container: NodeId, line_height: f64) -> f64 {
    let mut x = 0.0;
    for child in tree.children(container).to_vec() {
        if !tree.style(child).visible {
            continue;
        }
        let width = node_extent(tree, child);
        tree.set_bounds(child, Rect::new(x, 0.0, x + width, line_height));
        x += width;
    }
    x
}

/// Põe `text` como conteúdo de uma linha do nó (a fonte da UI no peso dado) e devolve a largura; texto vazio tira o conteúdo.
fn set_text(tree: &mut Tree, node: NodeId, text: &str, weight: u16, color: Rgba) -> f64 {
    if text.is_empty() {
        tree.style_mut(node).content = Content::None;
        return 0.0;
    }
    let style = ui_font(UI_FONT_SIZE, weight);
    let width = measure_text(text, &style);
    tree.style_mut(node).content = Content::Text { text: text.to_owned(), style, color, line_height: WORKBENCH_LINE_HEIGHT };
    width
}

fn is_natively_focusable(role: Role) -> bool {
    matches!(role, Role::Button | Role::Link | Role::TextInput | Role::ComboBox | Role::ListBox | Role::Checkbox | Role::Radio | Role::Switch)
}

/// `removeChildrenFromTabOrder`.
fn remove_children_from_tab_order(tree: &mut Tree, node: NodeId) {
    for descendant in tree.subtree(node) {
        let tab_index = tree.attribute(descendant, "tabindex").map(str::to_owned);
        let focusable = tab_index.as_deref() == Some("0") || (tab_index.as_deref() != Some("-1") && is_natively_focusable(tree.role(descendant)));
        if focusable {
            tree.set_attribute(descendant, ELEMENT_FOCUSABLE_ATTR, "true");
            tree.set_attribute(descendant, "tabindex", "-1");
        }
    }
}

/// `addChildrenToTabOrder`.
fn add_children_to_tab_order(tree: &mut Tree, node: NodeId) {
    for descendant in tree.subtree(node) {
        if tree.attribute(descendant, ELEMENT_FOCUSABLE_ATTR) == Some("true") {
            tree.remove_attribute(descendant, ELEMENT_FOCUSABLE_ATTR);
            tree.set_attribute(descendant, "tabindex", "0");
        }
    }
}

/// `fixSettingLinks`: `` `#editor.fontSize#` `` e `'#editor.fontSize#'` viram um link para a configuração (ou só o nome entre
/// aspas, sem `linkify`).
pub fn fix_setting_links(text: &str, linkify: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let quote = chars[index];
        if (quote == '`' || quote == '\'') && chars.get(index + 1) == Some(&'#') {
            let start = index + 2;
            let mut end = start;
            while end < chars.len() && chars[end] != '#' && chars[end] != quote && !chars[end].is_whitespace() {
                end += 1;
            }
            if end > start && chars.get(end) == Some(&'#') && chars.get(end + 1) == Some(&quote) {
                let setting_key: String = chars[start..end].iter().collect();
                let target_display_format = setting_key_to_display_format(&setting_key, "", false);
                let target_name = format!("{}: {}", target_display_format.category, target_display_format.label);
                if linkify {
                    result.push_str(&format!("[{target_name}](#{setting_key} \"{setting_key}\")"));
                } else {
                    result.push_str(&format!("\"{target_name}\""));
                }
                index = end + 2;
                continue;
            }
        }
        result.push(quote);
        index += 1;
    }
    result
}

/// `getLanguageTagSettingPlainKey(key)` como texto.
fn plain_language_key(key: &str) -> String {
    get_language_tag_setting_plain_key(key).to_string_lossy()
}

/// `renderValidations` do `SettingComplexRenderer` (linhas 1275 a 1284), a parte de [`Dynamic`] do tipo.
fn invalid_type_error(element: &SettingsTreeSettingElement) -> Option<String> {
    if !element.is_configured() {
        return None;
    }
    let type_ = match &element.setting.r#type {
        None => Dynamic::Undefined,
        Some(SettingSchemaType::Single(name)) => Dynamic::string(name),
        Some(SettingSchemaType::List(names)) => Dynamic::array(names.iter().map(|name| Dynamic::string(name))),
    };
    get_invalid_type_error(&element.value(), &type_)
}

// ---- getObjectDisplayValue (linhas 124 a 256) ---------------------------------------------------------------------------------

/// `'' + value`: a conversão de texto do JavaScript.
pub fn js_text(value: &Dynamic) -> String {
    match value {
        Dynamic::Undefined => "undefined".to_owned(),
        Dynamic::Null => "null".to_owned(),
        Dynamic::Bool(value) => value.to_string(),
        Dynamic::Number(value) => number_to_js_string(*value),
        Dynamic::String(text) => text.to_string_lossy(),
        Dynamic::Array(items) => items.to_vec().iter().map(|item| if item.is_nullish() { String::new() } else { js_text(item) }).collect::<Vec<_>>().join(","),
        _ => "[object Object]".to_owned(),
    }
}

/// O texto de um valor que pode estar ausente (`description?: string`).
fn optional_text(value: &Dynamic) -> Option<String> {
    if value.is_nullish() { None } else { Some(js_text(value)) }
}

fn array_items(value: &Dynamic) -> Vec<Dynamic> {
    match value {
        Dynamic::Array(items) => items.to_vec(),
        _ => Vec::new(),
    }
}

fn is_text(value: &Dynamic, expected: &str) -> bool {
    matches!(value, Dynamic::String(text) if text.to_string_lossy() == expected)
}

/// `typeof value === 'object' ? value ?? {} : {}` como as entradas de `Object.entries`.
fn object_entries(value: &Dynamic) -> Vec<(JsString, Dynamic)> {
    match value {
        Dynamic::Object(_) | Dynamic::Array(_) => value.own_entries(),
        _ => Vec::new(),
    }
}

/// `{ ...defaults, ...overrides }`: a chave repetida mantém o lugar da primeira e o valor da última.
fn merge_entries(defaults: &[(JsString, Dynamic)], overrides: &[(JsString, Dynamic)]) -> Vec<(JsString, Dynamic)> {
    let mut merged: Vec<(JsString, Dynamic)> = defaults.to_vec();
    for (key, value) in overrides {
        match merged.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value.clone(),
            None => merged.push((key.clone(), value.clone())),
        }
    }
    merged
}

fn lookup(entries: &[(JsString, Dynamic)], key: &JsString) -> Dynamic {
    entries.iter().find(|(candidate, _)| candidate == key).map_or(Dynamic::Undefined, |(_, value)| value.clone())
}

/// `getEnumOptionsFromSchema`.
pub fn get_enum_options_from_schema(schema: &Dynamic) -> Vec<ObjectEnumOption> {
    let any_of = schema.get_str("anyOf");
    if any_of.is_truthy() {
        return array_items(&any_of).iter().flat_map(get_enum_options_from_schema).collect();
    }
    let descriptions = array_items(&schema.get_str("enumDescriptions"));
    array_items(&schema.get_str("enum"))
        .iter()
        .enumerate()
        .map(|(index, value)| ObjectEnumOption { value: js_text(value), description: descriptions.get(index).and_then(optional_text) })
        .collect()
}

/// `ObjectValue['type']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectValueType {
    Boolean,
    Enum,
    String,
}

/// `getObjectValueType`.
pub fn get_object_value_type(schema: &Dynamic) -> ObjectValueType {
    let any_of = schema.get_str("anyOf");
    if any_of.is_truthy() {
        let sub_types: Vec<ObjectValueType> = array_items(&any_of).iter().map(get_object_value_type).collect();
        return if sub_types.contains(&ObjectValueType::Enum) { ObjectValueType::Enum } else { ObjectValueType::String };
    }

    let type_ = schema.get_str("type");
    let enum_values = schema.get_str("enum");
    if is_text(&type_, "boolean") {
        ObjectValueType::Boolean
    } else if is_text(&type_, "string") && !enum_values.is_nullish() && !array_items(&enum_values).is_empty() {
        ObjectValueType::Enum
    } else {
        ObjectValueType::String
    }
}

/// `getObjectEntryValueDisplayValue`.
fn get_object_entry_value_display_value(type_: ObjectValueType, data: &Dynamic, options: Vec<ObjectEnumOption>) -> ObjectValue {
    match type_ {
        ObjectValueType::Boolean => ObjectValue::Boolean(data.is_truthy()),
        ObjectValueType::Enum => ObjectValue::Enum(ObjectEnumData { data: js_text(data), options }),
        ObjectValueType::String => ObjectValue::String(js_text(data)),
    }
}

/// O nome da origem de um valor padrão (`typeof source === 'string' ? source : source?.displayName`).
fn default_source_display_name(source: &ConfigurationDefaultSource) -> Option<String> {
    match source {
        ConfigurationDefaultSource::Text(text) => Some(text.clone()),
        ConfigurationDefaultSource::Extension(extension) => extension.display_name.clone(),
    }
}

/// A `RegExp` de um padrão do esquema, ou o `SyntaxError` do `new RegExp` do original.
fn compile_pattern(pattern: &str) -> RegExp {
    RegExp::from_str(pattern, "").unwrap_or_else(|error| errors::throw(Error::named("SyntaxError", format!("Invalid regular expression: /{pattern}/: {}", error.message()))))
}

fn reg_exp_test(regex: &RegExp, text: &str) -> bool {
    regex.exec(&text.encode_utf16().collect::<Vec<u16>>(), 0).is_some()
}

/// `getObjectDisplayValue`.
pub fn get_object_display_value(element: &SettingsTreeSettingElement) -> Vec<IObjectDataItem> {
    let element_default_value = object_entries(&element.default_value());
    let element_scope_value = object_entries(&element.scope_value());

    let data: Vec<(JsString, Dynamic)> = if element.is_configured() {
        merge_entries(&element_default_value, &element_scope_value)
    } else if element.has_policy_value() || element.is_agents_window_read_only() {
        element.scope_value().own_entries()
    } else {
        element_default_value.clone()
    };

    let setting = &element.setting;
    let object_properties = setting.object_properties.clone().filter(|value| !value.is_nullish());
    let object_pattern_properties = setting.object_pattern_properties.clone().unwrap_or(Dynamic::Undefined);
    let object_additional_properties = setting.object_additional_properties.clone().unwrap_or(Dynamic::Undefined);

    let patterns_and_schemas: Vec<(RegExp, Dynamic)> = object_entries(&object_pattern_properties).into_iter().map(|(pattern, schema)| (compile_pattern(&pattern.to_string_lossy()), schema)).collect();

    let well_defined_key_enum_options: Vec<ObjectEnumOption> = object_properties
        .as_ref()
        .map(|properties| {
            object_entries(properties)
                .into_iter()
                .map(|(key, schema)| ObjectEnumOption { value: key.to_string_lossy(), description: optional_text(&schema.get_str("description")) })
                .collect()
        })
        .unwrap_or_default();

    let additional_schema = match &object_additional_properties {
        Dynamic::Bool(_) | Dynamic::Undefined | Dynamic::Null => None,
        other => Some(other.clone()),
    };

    data.iter()
        .map(|(key, data_value)| {
            let key_text = key.to_string_lossy();
            let default_value = lookup(&element_default_value, key);

            // Get source if it's a default value
            let mut source: Option<String> = None;
            if default_value.strict_equals(data_value)
                && setting.r#type.as_ref().is_some_and(|type_| type_.is("object"))
                && let Some(ConfigurationDefaultValueSource::Map(map)) = element.default_value_source()
            {
                source = map.borrow().get(&JsString::from(format!("{}.{}", setting.key, key_text))).and_then(default_source_display_name);
            }

            if let Some(properties) = &object_properties
                && properties.has_property(key)
            {
                let property_schema = properties.get(key);
                let value_enum_options = get_enum_options_from_schema(&property_schema);
                return IObjectDataItem {
                    key: ObjectKey::Enum(ObjectEnumData { data: key_text, options: well_defined_key_enum_options.clone() }),
                    value: get_object_entry_value_display_value(get_object_value_type(&property_schema), data_value, value_enum_options),
                    key_description: optional_text(&property_schema.get_str("description")),
                    removable: default_value.is_nullish(),
                    resetable: !default_value.is_nullish(),
                    source,
                };
            }

            // The row is removable if it doesn't have a default value assigned or the setting supports removing the default value.
            // If a default value is assigned and the user modified the default, it can be reset back to the default.
            let removable = default_value.is_undefined() || object_setting_supports_remove_default_value(&setting.key);
            let resetable = default_value.is_truthy() && !default_value.strict_equals(data_value);
            let schema = patterns_and_schemas.iter().find(|(pattern, _)| reg_exp_test(pattern, &key_text)).map(|(_, schema)| schema);
            if let Some(schema) = schema {
                let value_enum_options = get_enum_options_from_schema(schema);
                return IObjectDataItem {
                    key: ObjectKey::String(key_text),
                    value: get_object_entry_value_display_value(get_object_value_type(schema), data_value, value_enum_options),
                    key_description: optional_text(&schema.get_str("description")),
                    removable,
                    resetable,
                    source,
                };
            }

            let additional_value_enums = get_enum_options_from_schema(&additional_schema.clone().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new())));
            let additional_type = match &additional_schema {
                Some(schema @ (Dynamic::Object(_) | Dynamic::Array(_))) => get_object_value_type(schema),
                _ => ObjectValueType::String,
            };
            IObjectDataItem {
                key: ObjectKey::String(key_text),
                value: get_object_entry_value_display_value(additional_type, data_value, additional_value_enums),
                key_description: match &additional_schema {
                    Some(schema @ (Dynamic::Object(_) | Dynamic::Array(_))) => optional_text(&schema.get_str("description")),
                    _ => None,
                },
                removable,
                resetable,
                source,
            }
        })
        .collect()
}

// ---- AbstractSettingRenderer (linhas 856 a 1137) ------------------------------------------------------------------------------

/// `AbstractSettingRenderer`.
pub struct AbstractSettingRenderer {
    base: Rc<Disposable>,
    weak_self: Weak<AbstractSettingRenderer>,
    pub(crate) services: SettingRendererServices,
    setting_actions: Vec<ActionRef>,
    disposable_action_factory: DisposableActionFactory,
    ignored_settings: RefCell<Vec<String>>,
    on_did_click_override_element: Rc<Emitter<ISettingOverrideClickEvent>>,
    pub(crate) on_did_change_setting: Rc<Emitter<ISettingChangeEvent>>,
    pub(crate) on_did_open_settings: Rc<Emitter<String>>,
    on_did_click_setting_link: Rc<Emitter<ISettingLinkClickEvent>>,
    pub(crate) on_did_focus_setting: Rc<Emitter<Rc<SettingsTreeSettingElement>>>,
    on_did_change_ignored_settings: Rc<Emitter<()>>,
    pub(crate) on_did_change_setting_height: Rc<Emitter<HeightChangeParams>>,
    pub(crate) on_apply_filter: Rc<Emitter<String>>,
}

impl AbstractSettingRenderer {
    pub const CONTROL_CLASS: &'static str = "setting-control-focus-target";
    pub const CONTENTS_CLASS: &'static str = "setting-item-contents";
    pub const SETTING_KEY_ATTR: &'static str = "data-key";
    pub const SETTING_ID_ATTR: &'static str = "data-id";
    pub const ELEMENT_FOCUSABLE_ATTR: &'static str = ELEMENT_FOCUSABLE_ATTR;

    pub fn new(services: SettingRendererServices, setting_actions: Vec<ActionRef>, disposable_action_factory: DisposableActionFactory) -> Rc<Self> {
        let base = Disposable::new();
        let ignored_settings = Self::read_ignored_settings(&*services.configuration_service);

        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            services: services.clone(),
            setting_actions,
            disposable_action_factory,
            ignored_settings: RefCell::new(ignored_settings),
            on_did_click_override_element: registered_emitter::<ISettingOverrideClickEvent>(&base),
            on_did_change_setting: registered_emitter::<ISettingChangeEvent>(&base),
            on_did_open_settings: registered_emitter::<String>(&base),
            on_did_click_setting_link: registered_emitter::<ISettingLinkClickEvent>(&base),
            on_did_focus_setting: registered_emitter::<Rc<SettingsTreeSettingElement>>(&base),
            on_did_change_ignored_settings: registered_emitter::<()>(&base),
            on_did_change_setting_height: registered_emitter::<HeightChangeParams>(&base),
            on_apply_filter: registered_emitter::<String>(&base),
            base: base.clone(),
        });

        let weak = Rc::downgrade(&this);
        base.register(services.configuration_service.on_did_change_configuration().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.ignored_settings.borrow_mut() = Self::read_ignored_settings(&*this.services.configuration_service);
                this.on_did_change_ignored_settings.fire(());
            }
        }));
        this
    }

    /// `getIgnoredSettings(getDefaultIgnoredSettings(), this._configService)`.
    fn read_ignored_settings(configuration_service: &dyn IWorkbenchConfigurationService) -> Vec<String> {
        let defaults = get_default_ignored_settings(false);
        get_ignored_settings(&defaults, configuration_service, None).iter().map(JsString::to_string_lossy).collect()
    }

    pub fn on_did_click_override_element(&self) -> BaseEvent<ISettingOverrideClickEvent> {
        self.on_did_click_override_element.event()
    }

    pub fn on_did_change_setting(&self) -> BaseEvent<ISettingChangeEvent> {
        self.on_did_change_setting.event()
    }

    pub fn on_did_open_settings(&self) -> BaseEvent<String> {
        self.on_did_open_settings.event()
    }

    pub fn on_did_click_setting_link(&self) -> BaseEvent<ISettingLinkClickEvent> {
        self.on_did_click_setting_link.event()
    }

    pub fn on_did_focus_setting(&self) -> BaseEvent<Rc<SettingsTreeSettingElement>> {
        self.on_did_focus_setting.event()
    }

    pub fn on_did_change_ignored_settings(&self) -> BaseEvent<()> {
        self.on_did_change_ignored_settings.event()
    }

    pub fn on_did_change_setting_height(&self) -> BaseEvent<HeightChangeParams> {
        self.on_did_change_setting_height.event()
    }

    pub fn on_apply_filter(&self) -> BaseEvent<String> {
        self.on_apply_filter.event()
    }

    /// `hoverService.setupDelayedHover(target, { content })`.
    fn setup_text_hover(&self, target: NodeId, content: String) -> je_base::lifecycle::DisposableRef {
        self.services.widgets.hover_service.setup_delayed_hover(target, Rc::new(move || IHoverOptions::new(HoverContent::Text(content.clone()), HoverTarget::Node(target))), None)
    }

    /// Os estilos do texto dos indicadores (`font-style: italic` do `.setting-indicators-container`).
    fn indicators_styles(&self) -> HighlightedLabelStyles {
        HighlightedLabelStyles {
            foreground: self.services.styles.foreground,
            highlight_foreground: None,
            highlight_weight: 400,
            font_size: UI_FONT_SIZE,
            font_weight: 400,
            line_height: Some(WORKBENCH_LINE_HEIGHT),
            italic: true,
            strikethrough: false,
        }
    }

    /// `renderCommonTemplate(tree, _container, typeClass)`. `row_element` é o `_container`.
    pub fn render_common_template(&self, row_element: NodeId, type_class: &str) -> ISettingItemTemplate {
        let ui = self.services.ui();
        let to_dispose = DisposableStore::new();
        let styles = &self.services.styles;

        // O título e o contêiner de indicadores (criado dentro do `SettingsTreeIndicatorsLabel`, depois do rótulo) vêm antes
        // da descrição, como no original; o indicador de modificação, o valor e o resto seguem a ordem do DOM.
        let (container, title_element, category_element, label_container_element) = {
            let mut tree = ui.tree_mut();
            tree.add_class(row_element, "setting-item");
            tree.add_class(row_element, &format!("setting-item-{type_class}"));

            let container = create_el(&mut tree, row_element, Role::Generic, "setting-item-contents settings-row-inner-container");
            let title_element = create_el(&mut tree, container, Role::Generic, "setting-item-title");
            let label_category_container = create_el(&mut tree, title_element, Role::Generic, "setting-item-cat-label-container");
            let category_element = create_el(&mut tree, label_category_container, Role::Generic, "setting-item-category");
            let label_container_element = create_el(&mut tree, label_category_container, Role::Generic, "setting-item-label");
            (container, title_element, category_element, label_container_element)
        };

        let label_styles = HighlightedLabelStyles {
            foreground: styles.header_foreground,
            highlight_foreground: None,
            highlight_weight: 600,
            font_size: UI_FONT_SIZE,
            font_weight: 600,
            line_height: Some(WORKBENCH_LINE_HEIGHT),
            italic: false,
            strikethrough: false,
        };
        let label_element = to_dispose.add_rc(Rc::new(SimpleIconLabel::new(ui, label_container_element, label_styles)));
        let indicators_label = to_dispose.add_rc(Rc::new(SettingsTreeIndicatorsLabel::new(
            ui,
            self.indicators_styles(),
            title_element,
            Some(self.services.configuration_service.clone()),
            self.services.widgets.hover_service.clone(),
            Some(self.services.user_data_sync_enablement_service.clone()),
            Some(self.services.language_service.clone()),
            Some(self.services.command_service.clone()),
        )));

        let (description_element, modified_indicator_element, value_element, control_element, deprecation_warning_element, toolbar_container) = {
            let mut tree = ui.tree_mut();
            let description_element = create_el(&mut tree, container, Role::Generic, "setting-item-description");
            let modified_indicator_element = create_el(&mut tree, container, Role::Generic, "setting-item-modified-indicator");
            let value_element = create_el(&mut tree, container, Role::Generic, "setting-item-value");
            let control_element = create_el(&mut tree, value_element, Role::Generic, "setting-item-control");
            let deprecation_warning_element = create_el(&mut tree, container, Role::Generic, "setting-item-deprecation-message");
            // `.setting-item-deprecation-message { display: none }`
            tree.style_mut(deprecation_warning_element).visible = false;
            let toolbar_container = create_el(&mut tree, container, Role::Generic, "setting-toolbar-container");
            (description_element, modified_indicator_element, value_element, control_element, deprecation_warning_element, toolbar_container)
        };

        to_dispose.add(self.setup_text_hover(modified_indicator_element, localize("modified", "The setting has been configured in the current scope.", &[])));

        let toolbar = to_dispose.add_rc(self.render_setting_toolbar(toolbar_container));

        let template = ISettingItemTemplate {
            to_dispose: to_dispose.clone(),
            element_disposables: to_dispose.add_rc(DisposableStore::new()),
            on_change: RefCell::new(None),
            context: Rc::new(RefCell::new(None)),
            row_element,
            container_element: container,
            title_element,
            category_element,
            label_container_element,
            label_element,
            description_element,
            modified_indicator_element,
            value_element,
            control_element,
            deprecation_warning_element,
            toolbar_container,
            indicators_label,
            toolbar,
            type_class: type_class.to_owned(),
            description_view: RefCell::new(None),
            deprecation_view: RefCell::new(None),
            height: Cell::new(0.0),
            content_bottom: Cell::new(0.0),
        };

        // Prevent clicks from being handled by list
        to_dispose.add(ui.listen(control_element, EventKind::PointerDown, false, |e| e.stop_propagation()));

        for (kind, over) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
            let ui_for_class = ui.clone();
            to_dispose.add(ui.listen(title_element, kind, false, move |_| {
                ui_for_class.tree_mut().toggle_class(container, "mouseover", over);
            }));
        }

        // `.monaco-list-row:not(.focused) .settings-row-inner-container:hover { background-color: settings.rowHoverBackground }`
        if let Some(hover_background) = styles.row_hover_background {
            for (kind, over) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
                let ui_for_hover = ui.clone();
                to_dispose.add(ui.listen(container, kind, false, move |_| {
                    let mut tree = ui_for_hover.tree_mut();
                    let row_is_focused = std::iter::successors(tree.parent(container), |node| tree.parent(*node)).any(|node| tree.has_class(node, "focused"));
                    tree.style_mut(container).background = (over && !row_is_focused).then_some(hover_background);
                }));
            }
        }

        template
    }

    /// `addSettingElementFocusHandler(template)`.
    pub fn add_setting_element_focus_handler(&self, template: &ISettingItemTemplate) {
        let ui = self.services.ui();
        let container = template.container_element;
        let focus_tracker = FocusTracker::new(ui, container);
        template.to_dispose.add_rc(focus_tracker.clone());

        let ui_for_blur = ui.clone();
        template.to_dispose.add(focus_tracker.on_did_blur().listen(move |_| {
            let mut tree = ui_for_blur.tree_mut();
            if tree.has_class(container, "focused") {
                tree.remove_class(container, "focused");
            }
        }));

        let (ui_for_focus, context, emitter) = (ui.clone(), template.context.clone(), self.on_did_focus_setting.clone());
        template.to_dispose.add(focus_tracker.on_did_focus().listen(move |_| {
            ui_for_focus.tree_mut().add_class(container, "focused");
            let current = context.borrow().clone();
            if let Some(current) = current {
                emitter.fire(current);
            }
        }));
    }

    /// `renderSettingToolbar(container)`.
    fn render_setting_toolbar(&self, container: NodeId) -> Rc<ToolBar> {
        let toggle_menu_title = self.services.keybinding_service.append_keybinding(
            &localize("settingsContextMenuTitle", "More Actions... ", &[]),
            Some(SETTINGS_EDITOR_COMMAND_SHOW_CONTEXT_MENU),
            None,
            false,
        );
        ToolBar::new(
            self.services.ui(),
            container,
            self.services.context_menu_provider.clone(),
            IToolBarOptions {
                toggle_menu_title: Some(toggle_menu_title),
                render_dropdown_as_child_element: !IS_IOS,
                more_icon: Some(ThemeIcon::from_id("gear")),
                styles: Some(self.services.widgets.styles.action_bar.clone()),
                ..IToolBarOptions::default()
            },
        )
    }

    /// `renderSettingElement(node, index, template)`. `render_value` é o `this.renderValue(element, template, onChange)`.
    pub fn render_setting_element(&self, element: &Rc<SettingsTreeSettingElement>, template: &ISettingItemTemplate, render_value: &dyn Fn(Rc<dyn Fn(Dynamic)>)) {
        let ui = self.services.ui();
        let styles = &self.services.styles;

        // The element must inspect itself to get information for
        // the modified indicator and the overridden Settings indicators.
        element.inspect_self();

        *template.context.borrow_mut() = Some(element.clone());
        template.toolbar.set_context(Some(Dynamic::from(ObjectRef::with_host(element.clone()))));
        let actions = (self.disposable_action_factory)(&element.setting, &element.settings_target);
        for action in &actions {
            if action.as_base_action().is_some() {
                let action = action.clone();
                template.element_disposables.add(to_disposable(move || {
                    if let Some(base) = action.as_base_action() {
                        base.dispose();
                    }
                }));
            }
        }
        let all_actions: Vec<ActionRef> = self.setting_actions.iter().chain(actions.iter()).cloned().collect();
        template.toolbar.set_actions(&[], &all_actions);

        let setting = &element.setting;
        let is_configured = element.is_configured();

        {
            let mut tree = ui.tree_mut();
            tree.toggle_class(template.container_element, "is-configured", is_configured);
            tree.set_attribute(template.container_element, Self::SETTING_KEY_ATTR, setting.key.clone());
            tree.set_attribute(template.container_element, Self::SETTING_ID_ATTR, element.id());
        }

        let title_tooltip = format!("{}{}", setting.key, if is_configured { " - Modified" } else { "" });
        let display_category = element.display_category();
        {
            let mut tree = ui.tree_mut();
            let text = if display_category.is_empty() { String::new() } else { format!("{display_category}: ") };
            set_text(&mut tree, template.category_element, &text, 600, styles.header_foreground);
            tree.style_mut(template.category_element).opacity = 0.9;
        }
        template.element_disposables.add(self.setup_text_hover(template.category_element, title_tooltip.clone()));

        template.label_element.set_text(&element.display_label());
        template.label_element.set_title(&title_tooltip);

        // `descriptionElement.innerText = ''`
        {
            let mut tree = ui.tree_mut();
            clear_node(&mut tree, template.description_element);
        }
        let description_view = if setting.description_is_markdown.unwrap_or(false) {
            self.render_setting_markdown(element, template.container_element, element.description(), &template.element_disposables)
        } else {
            template.element_disposables.add_rc(MarkdownView::from_plain_text(ui, element.description(), &styles.markdown))
        };
        ui.tree_mut().append_child(template.description_element, description_view.root());
        *template.description_view.borrow_mut() = Some(description_view);

        template.indicators_label.update_scope_overrides(&**element, &self.on_did_click_override_element, &self.on_apply_filter);
        {
            let (indicators, element, click, filter) = (template.indicators_label.clone(), element.clone(), self.on_did_click_override_element.clone(), self.on_apply_filter.clone());
            template.element_disposables.add(self.services.configuration_service.on_did_change_configuration().listen(move |e| {
                if e.affects_configuration(APPLY_ALL_PROFILES_SETTING, None) {
                    indicators.update_scope_overrides(&*element, &click, &filter);
                }
            }));
        }

        let on_change: Rc<dyn Fn(Dynamic)> = {
            let (emitter, element, context) = (self.on_did_change_setting.clone(), element.clone(), template.context.clone());
            Rc::new(move |value| {
                let value_type = context.borrow().as_ref().map_or_else(|| element.value_type(), |current| current.value_type());
                emitter.fire(ISettingChangeEvent {
                    key: element.setting.key.clone(),
                    value,
                    r#type: SettingChangeType::Single(value_type),
                    manual_reset: false,
                    scope: element.setting.scope,
                });
            })
        };

        self.render_deprecation(element, template);

        *template.on_change.borrow_mut() = Some(on_change.clone());
        render_value(on_change);

        template.indicators_label.update_workspace_trust(&**element);
        template.indicators_label.update_sync_ignored(&**element, &self.ignored_settings.borrow());
        template.indicators_label.update_default_override_indicator(&**element);
        template.indicators_label.update_preview_indicator(&**element);
        template.indicators_label.update_advanced_indicator(&**element);
        {
            let (weak, indicators, element) = (self.weak_self.clone(), template.indicators_label.clone(), element.clone());
            template.element_disposables.add(self.on_did_change_ignored_settings.event().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    indicators.update_sync_ignored(&*element, &this.ignored_settings.borrow());
                }
            }));
        }

        self.update_setting_tabbable(element, template);
        {
            let (weak, element_for_listener, container) = (self.weak_self.clone(), element.clone(), template.container_element);
            template.element_disposables.add(element.on_did_change_tabbable().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    Self::apply_tabbable(this.services.ui(), &element_for_listener, container);
                }
            }));
        }
    }

    /// A parte da depreciação de `renderSettingElement`: o texto (em Markdown ou não), o ícone e as classes.
    fn render_deprecation(&self, element: &Rc<SettingsTreeSettingElement>, template: &ISettingItemTemplate) {
        let ui = self.services.ui();
        let styles = &self.services.styles;
        let setting = &element.setting;
        let deprecation_text = setting.deprecation_message.clone().unwrap_or_default();

        // `deprecationWarningElement.innerText = ''` nos dois ramos
        {
            let mut tree = ui.tree_mut();
            clear_node(&mut tree, template.deprecation_warning_element);
        }
        let view = if !deprecation_text.is_empty() && setting.deprecation_message_is_markdown.unwrap_or(false) {
            Some(self.render_setting_markdown(element, template.container_element, &deprecation_text, &template.element_disposables))
        } else if !deprecation_text.is_empty() {
            Some(template.element_disposables.add_rc(MarkdownView::from_plain_text(ui, &deprecation_text, &styles.markdown)))
        } else {
            None
        };

        let is_informational_deprecation = setting.deprecation_message_severity.as_deref() == Some("info");
        {
            let mut tree = ui.tree_mut();
            // `prepend(deprecationIcon)`: o ícone vem antes do texto.
            let icon = tree.create(template.deprecation_warning_element, Role::Image);
            tree.set_class_name(icon, if is_informational_deprecation { "codicon codicon-info" } else { "codicon codicon-error" });
            tree.style_mut(icon).content = Content::Codicon {
                id: (if is_informational_deprecation { "info" } else { "error" }).to_owned(),
                size: DEPRECATION_ICON_SIZE as f32,
                color: if is_informational_deprecation { styles.info_foreground } else { styles.error_foreground },
            };
            tree.set_label(
                icon,
                if is_informational_deprecation { localize("setting.deprecationMessageSeverity.info", "Info", &[]) } else { localize("setting.deprecationMessageSeverity.warning", "Warning", &[]) },
            );
            if let Some(view) = &view {
                tree.append_child(template.deprecation_warning_element, view.root());
            }
            tree.toggle_class(template.container_element, "is-deprecated", !deprecation_text.is_empty());
            tree.toggle_class(template.container_element, "is-deprecated-info", !deprecation_text.is_empty() && is_informational_deprecation);
            // `.setting-item-deprecation-message { display: none }`, `display: flex` com `.is-deprecated`.
            tree.style_mut(template.deprecation_warning_element).visible = !deprecation_text.is_empty();
        }
        *template.deprecation_view.borrow_mut() = view;
    }

    /// `updateSettingTabbable(element, template)`.
    fn update_setting_tabbable(&self, element: &Rc<SettingsTreeSettingElement>, template: &ISettingItemTemplate) {
        Self::apply_tabbable(self.services.ui(), element, template.container_element);
    }

    fn apply_tabbable(ui: &Ui, element: &SettingsTreeSettingElement, container: NodeId) {
        let mut tree = ui.tree_mut();
        if element.tabbable() {
            add_children_to_tab_order(&mut tree, container);
        } else {
            remove_children_from_tab_order(&mut tree, container);
        }
    }

    /// `renderSettingMarkdown(element, container, text, disposables)`.
    fn render_setting_markdown(&self, element: &Rc<SettingsTreeSettingElement>, container: NodeId, text: &str, disposables: &DisposableStore) -> Rc<MarkdownView> {
        let ui = self.services.ui();
        // Rewrite `#editor.fontSize#` to link format
        let markdown = IMarkdownString { value: JsString::from(fix_setting_links(text, true)), is_trusted: Some(IsTrusted::Bool(true)), ..IMarkdownString::default() };

        let action_handler = {
            let (weak, element) = (self.weak_self.clone(), element.clone());
            Rc::new(move |content: &JsString, _markdown: &IMarkdownString| {
                let Some(this) = weak.upgrade() else { return };
                let content = content.to_string_lossy();
                if let Some(target_key) = content.strip_prefix('#') {
                    this.on_did_click_setting_link.fire(ISettingLinkClickEvent { source: element.clone(), target_key: target_key.to_owned() });
                } else {
                    let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..OpenOptions::default() };
                    let _ = this.services.opener_service.open(&OpenResource::Text(JsString::from(content)), Some(&options)).catch(|error| {
                        on_unexpected_error(&error);
                        false
                    });
                }
            })
        };
        let async_render_callback = {
            let (ui, weak, element) = (ui.clone(), self.weak_self.clone(), element.clone());
            Rc::new(move || {
                let height = ui.tree().bounds(container).height();
                if height > 0.0 && let Some(this) = weak.upgrade() {
                    this.on_did_change_setting_height.fire(HeightChangeParams { element: element.clone(), height });
                }
            })
        };
        let options = MarkdownRenderOptions { action_handler: Some(action_handler), async_render_callback: Some(async_render_callback), ..MarkdownRenderOptions::default() };
        let rendered = render_markdown(&markdown, &options, &self.services.markdown_environment.borrowed()).unwrap_or_else(|error| errors::throw(Error::new(error.to_string())));
        clean_rendered_markdown(&mut rendered.element.borrow_mut());
        let view = MarkdownView::from_rendered(ui, rendered, self.services.markdown_environment.clone(), &self.services.styles.markdown);
        disposables.add_rc(view)
    }

    /// `disposeElement(element, index, template)`.
    pub fn dispose_element(&self, template: &ISettingItemTemplate) {
        template.element_disposables.clear();
    }

    /// `disposeTemplate(template)`.
    pub fn dispose_template(&self, template: &ISettingItemTemplate) {
        template.to_dispose.dispose();
    }

    /// O layout do template: empilha título, descrição, valor e mensagem de depreciação (ver o `//!`), posiciona a barra de
    /// ferramentas e o indicador de modificação, e devolve a altura da linha. `control_size` é o tamanho do controle (o
    /// `.setting-item-control`) que o renderizador do valor já calculou.
    pub fn layout_setting_item(&self, template: &ISettingItemTemplate, control_size: (f64, f64)) -> f64 {
        let ui = self.services.ui();
        let styles = &self.services.styles;
        let line_height = styles.markdown.line_height;
        let row_width = ui.tree().bounds(template.row_element).width();
        let (left, inner_width) = contents_box(row_width);
        let text_width = (inner_width - 2.0 * ITEM_PADDING_X).max(0.0);

        // As vistas de Markdown medem a si mesmas (elas mexem na árvore): antes de abrir o empréstimo abaixo.
        let description_height = template.description_view.borrow().as_ref().map_or(0.0, |view| view.layout(text_width));
        let (is_deprecated, is_deprecated_info) = {
            let tree = ui.tree();
            (tree.has_class(template.container_element, "is-deprecated"), tree.has_class(template.container_element, "is-deprecated-info"))
        };
        let deprecation_text_width = (text_width - DEPRECATION_ICON_SIZE - DEPRECATION_ICON_MARGIN).max(0.0);
        let deprecation_text_height = if is_deprecated { template.deprecation_view.borrow().as_ref().map_or(0.0, |view| view.layout(deprecation_text_width)) } else { 0.0 };

        let bottom_padding = if matches!(template.type_class.as_str(), "bool" | "list") { ITEM_PADDING_BOTTOM_TALL } else { ITEM_PADDING_BOTTOM };
        let mut tree = ui.tree_mut();
        let mut y = ITEM_PADDING_TOP;

        // título: a categoria e o rótulo flutuam à esquerda, os indicadores seguem em linha
        let category_width = match &tree.style(template.category_element).content {
            Content::Text { text, style, .. } => measure_text(text, style),
            _ => 0.0,
        };
        let label_width = node_extent(&tree, template.label_container_element);
        tree.set_bounds(template.category_element, Rect::new(0.0, 0.0, category_width, line_height));
        tree.set_bounds(template.label_container_element, Rect::new(category_width, 0.0, category_width + label_width, line_height));
        let label_end = category_width + label_width + LABEL_MARGIN_RIGHT;
        let mut title_width = label_end;
        if let Some(indicators) = tree.children_with_class(template.title_element, "setting-indicators-container").first().copied() {
            let width = if tree.style(indicators).visible { flow_inline(&mut tree, indicators, line_height) } else { 0.0 };
            tree.set_bounds(indicators, Rect::new(label_end, 0.0, label_end + width, line_height));
            title_width += width;
        }
        let title_height = line_height + TITLE_PADDING_BOTTOM;
        tree.set_bounds(template.title_element, Rect::new(ITEM_PADDING_X, y, ITEM_PADDING_X + title_width.min(text_width), y + title_height));
        y += title_height + DESCRIPTION_MARGIN_TOP;

        // descrição
        tree.set_bounds(template.description_element, Rect::new(ITEM_PADDING_X, y, ITEM_PADDING_X + text_width, y + description_height));
        y += description_height;

        // valor
        y += VALUE_MARGIN_TOP;
        tree.set_bounds(template.value_element, Rect::new(ITEM_PADDING_X, y, ITEM_PADDING_X + text_width, y + control_size.1));
        tree.set_bounds(template.control_element, Rect::new(0.0, 0.0, control_size.0, control_size.1));
        y += control_size.1;

        // depreciação
        if is_deprecated {
            y += if is_deprecated_info { DEPRECATION_INFO_MARGIN_TOP } else { DEPRECATION_MARGIN_TOP };
            let height = deprecation_text_height.max(DEPRECATION_ICON_SIZE);
            tree.set_bounds(template.deprecation_warning_element, Rect::new(ITEM_PADDING_X, y, ITEM_PADDING_X + text_width, y + height));
            if let Some(icon) = tree.children(template.deprecation_warning_element).first().copied() {
                tree.set_bounds(icon, Rect::new(0.0, 0.0, DEPRECATION_ICON_SIZE, DEPRECATION_ICON_SIZE));
            }
            if let Some(view) = template.deprecation_view.borrow().as_ref() {
                let root = tree.bounds(view.root());
                let x = DEPRECATION_ICON_SIZE + DEPRECATION_ICON_MARGIN;
                tree.set_bounds(view.root(), Rect::new(x, 0.0, x + root.width(), root.height()));
            }
            y += height;
        }

        let content_bottom = y;
        y += bottom_padding;

        // a barra de ferramentas e o indicador de modificação, absolutos
        tree.set_bounds(template.toolbar_container, Rect::new(TOOLBAR_LEFT, TOOLBAR_TOP, TOOLBAR_LEFT + TOOLBAR_SIZE, TOOLBAR_TOP + TOOLBAR_SIZE));
        let configured = tree.has_class(template.container_element, "is-configured");
        tree.style_mut(template.modified_indicator_element).visible = configured;
        // `bottom: 18px` (a borda de baixo do conteúdo), ou `bottom: 23px` em `bool` e `list`
        let modified_bottom = if bottom_padding == ITEM_PADDING_BOTTOM_TALL { y - MODIFIED_BOTTOM_TALL } else { content_bottom };
        tree.set_bounds(template.modified_indicator_element, Rect::new(MODIFIED_LEFT, MODIFIED_TOP, MODIFIED_LEFT + MODIFIED_WIDTH, modified_bottom.max(MODIFIED_TOP)));
        if let Some(color) = styles.modified_item_indicator {
            let indicator = tree.style_mut(template.modified_indicator_element);
            indicator.border = Some(Border { width: MODIFIED_BORDER, color });
            indicator.border_widths = Some([0.0, 0.0, 0.0, MODIFIED_BORDER]);
        }

        tree.set_bounds(template.container_element, Rect::new(left, 0.0, left + inner_width, y));
        drop(tree);
        template.toolbar.action_bar().layout();

        template.height.set(y);
        template.content_bottom.set(content_bottom);
        y
    }
}

impl IDisposable for AbstractSettingRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "AbstractSettingRenderer"
    }
}

// ---- SettingGroupRenderer (linhas 1139 a 1167) ----------------------------------------------------------------------------------

/// `SettingGroupRenderer`.
pub struct SettingGroupRenderer {
    ui: Ui,
    styles: SettingRendererStyles,
}

impl SettingGroupRenderer {
    pub fn new(ui: Ui, styles: SettingRendererStyles) -> Self {
        Self { ui, styles }
    }

    /// `.settings-group-level-N { font-size }`: 26, 22 e 18 px nos três primeiros níveis, o padrão nos outros.
    fn font_size(level: usize) -> f32 {
        match level {
            1 => 26.0,
            2 => 22.0,
            3 => 18.0,
            _ => UI_FONT_SIZE,
        }
    }
}

impl ITreeRenderer<SettingsObjectTreeModel> for SettingGroupRenderer {
    fn template_id(&self) -> String {
        SETTINGS_ELEMENT_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        self.ui.tree_mut().add_class(container, "group-title");
        Rc::new(IGroupTitleTemplate { to_dispose: DisposableStore::new(), context: RefCell::new(None), parent: container }) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<SettingsTreeNode>, _index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        let Some(template) = template_data.downcast_ref::<IGroupTitleTemplate>() else { return };
        let Some(SettingsTreeGroupChild::Group(group)) = node.element.as_ref() else { return };
        let mut tree = self.ui.tree_mut();

        // `templateData.parent.innerText = ''`
        clear_node(&mut tree, template.parent);
        let label_element = create_el(&mut tree, template.parent, Role::Generic, "settings-group-title-label settings-row-inner-container");
        tree.add_class(label_element, &format!("settings-group-level-{}", group.level()));
        if group.is_first_group() {
            tree.add_class(label_element, "settings-group-first");
        }

        let container = tree.bounds(template.parent);
        let height = details.and_then(|details| details.height).unwrap_or_else(|| container.height());
        let (left, width) = contents_box(container.width());
        tree.set_bounds(label_element, Rect::new(left, 0.0, left + width, height));

        // `textContent = label`, com `padding: 10px 10px 10px 15px` e `text-overflow: ellipsis`.
        let text_node = tree.create(label_element, Role::Heading);
        let style = ui_font(Self::font_size(group.level()), 600);
        let text_width = (width - GROUP_PADDING_LEFT - GROUP_PADDING).max(0.0);
        let label = ellipsize(&group.label(), &style, text_width);
        tree.set_label(text_node, group.label());
        tree.set_bounds(text_node, Rect::new(GROUP_PADDING_LEFT, GROUP_PADDING, GROUP_PADDING_LEFT + text_width, GROUP_PADDING + WORKBENCH_LINE_HEIGHT));
        tree.style_mut(text_node).content = Content::Text { text: label, style, color: self.styles.header_foreground, line_height: WORKBENCH_LINE_HEIGHT };
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<IGroupTitleTemplate>() {
            template.to_dispose.dispose();
        }
    }
}

// ---- SettingNewExtensionsRenderer (linhas 1169 a 1207) -------------------------------------------------------------------------

/// `SettingNewExtensionsRenderer`.
pub struct SettingNewExtensionsRenderer {
    ui: Ui,
    command_service: Rc<dyn ICommandService>,
    button_styles: ButtonStyles,
}

impl SettingNewExtensionsRenderer {
    pub fn new(ui: Ui, command_service: Rc<dyn ICommandService>, button_styles: ButtonStyles) -> Self {
        Self { ui, command_service, button_styles }
    }
}

impl ITreeRenderer<SettingsObjectTreeModel> for SettingNewExtensionsRenderer {
    fn template_id(&self) -> String {
        SETTINGS_NEW_EXTENSIONS_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let to_dispose = DisposableStore::new();
        self.ui.tree_mut().add_class(container, "setting-item-new-extensions");

        let button = to_dispose.add_rc(Button::new(&self.ui, container, ButtonOptions { styles: self.button_styles.clone(), title: ButtonTitle::Auto, ..ButtonOptions::default() }));
        let context: Rc<RefCell<Option<Rc<SettingsTreeNewExtensionsElement>>>> = Rc::new(RefCell::new(None));
        let (command_service, shared_context) = (self.command_service.clone(), context.clone());
        to_dispose.add(button.on_did_click().listen(move |_| {
            let current = shared_context.borrow().clone();
            if let Some(current) = current {
                let extension_ids = Dynamic::array(current.extension_ids.iter().map(|id| Dynamic::string(id)));
                let _ = command_service.execute_command("workbench.extensions.action.showExtensionsWithIds", vec![extension_ids]);
            }
        }));
        button.set_label(ButtonLabel::Text(localize("newExtensionsButtonLabel", "Show matching extensions", &[])));
        self.ui.tree_mut().add_class(button.element(), "settings-new-extensions-button");

        Rc::new(ISettingNewExtensionsTemplate { to_dispose, button, context }) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<SettingsTreeNode>, _index: usize, template_data: &TemplateData, details: Option<&TreeElementRenderDetails>) {
        let Some(template) = template_data.downcast_ref::<ISettingNewExtensionsTemplate>() else { return };
        if let Some(SettingsTreeGroupChild::NewExtensions(element)) = node.element.as_ref() {
            *template.context.borrow_mut() = Some(element.clone());
        }

        // `.setting-item-new-extensions { display: flex }` e `.settings-new-extensions-button { margin: auto; margin-bottom: 15px; width: initial }`
        let container = {
            let tree = self.ui.tree();
            let parent = tree.parent(template.button.element()).unwrap_or_else(|| tree.root());
            tree.bounds(parent)
        };
        let height = details.and_then(|details| details.height).unwrap_or_else(|| container.height());
        template.button.set_height(None);
        template.button.set_width(Some(template.button.min_content_width()));
        let (width, button_height) = template.button.size();
        template.button.set_position(((container.width() - width) / 2.0).max(0.0), ((height - NEW_EXTENSIONS_MARGIN_BOTTOM - button_height) / 2.0).max(0.0));
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<ISettingNewExtensionsTemplate>() {
            template.to_dispose.dispose();
        }
    }
}

// ---- SettingComplexRenderer (linhas 1209 a 1285) ---------------------------------------------------------------------------------

/// `SettingComplexRenderer`.
pub struct SettingComplexRenderer {
    base: Rc<AbstractSettingRenderer>,
}

impl Deref for SettingComplexRenderer {
    type Target = AbstractSettingRenderer;

    fn deref(&self) -> &AbstractSettingRenderer {
        &self.base
    }
}

impl SettingComplexRenderer {
    pub fn new(services: SettingRendererServices, setting_actions: Vec<ActionRef>, disposable_action_factory: DisposableActionFactory) -> Rc<Self> {
        Rc::new(Self { base: AbstractSettingRenderer::new(services, setting_actions, disposable_action_factory) })
    }

    /// `SettingComplexRenderer.EDIT_IN_JSON_LABEL`.
    fn edit_in_json_label() -> String {
        localize("editInSettingsJson", "Edit in settings.json", &[])
    }

    /// O botão `a.edit-in-settings-button`, dentro de `parent`, com o papel de botão e a mão como cursor. `extra_classes` (com
    /// espaço no fim) vem antes das classes do botão.
    fn create_edit_button(&self, parent: NodeId, extra_classes: &str) -> NodeId {
        let mut tree = self.services.ui().tree_mut();
        let button = create_el(&mut tree, parent, Role::Button, &format!("{extra_classes}edit-in-settings-button {}", AbstractSettingRenderer::CONTROL_CLASS));
        tree.style_mut(button).cursor = Some(Cursor::Pointer);
        tree.style_mut(button).opacity = 0.9;
        button
    }

    /// O elemento da mensagem de validação (`$('.setting-item-validation-message')` no fim do contêiner), escondido.
    fn create_validation_message(&self, container: NodeId) -> NodeId {
        let mut tree = self.services.ui().tree_mut();
        let element = create_el(&mut tree, container, Role::Generic, "setting-item-validation-message");
        tree.style_mut(element).visible = false;
        element
    }

    /// `renderValue(dataElement, template, onChange)`.
    fn render_value(&self, data_element: &Rc<SettingsTreeSettingElement>, template: &ISettingComplexItemTemplate) {
        let ui = self.services.ui();
        let plain_key = plain_language_key(&data_element.setting.key);
        let edit_language_setting_label = localize("editLanguageSettingLabel", "Edit settings for {0}", &[&plain_key]);
        let is_language_tag_setting = data_element.setting.is_language_tag_setting.unwrap_or(false);
        let button_text = if is_language_tag_setting { edit_language_setting_label.clone() } else { Self::edit_in_json_label() };
        set_text(&mut ui.tree_mut(), template.button, &button_text, 400, self.services.styles.markdown.link_foreground);

        let on_click_or_keydown = {
            let (apply_filter, open_settings, key) = (self.on_apply_filter.clone(), self.on_did_open_settings.clone(), data_element.setting.key.clone());
            let plain_key = plain_key.clone();
            Rc::new(move |e: &mut je_ui::Event| {
                if is_language_tag_setting {
                    apply_filter.fire(format!("@{LANGUAGE_SETTING_TAG}{}", plain_key.replace(' ', "")));
                } else {
                    open_settings.fire(key.clone());
                }
                e.prevent_default();
                e.stop_propagation();
            })
        };
        let on_click = on_click_or_keydown.clone();
        template.element_disposables.add(ui.listen(template.button, EventKind::Click, false, move |e| on_click(e)));
        template.element_disposables.add(ui.listen(template.button, EventKind::KeyDown, false, move |e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            if key.equals(KeyCode::Space.bits()) || key.equals(KeyCode::Enter.bits()) {
                on_click_or_keydown(e);
            }
        }));

        self.render_validations(data_element, template);

        ui.tree_mut().set_label(
            template.button,
            if is_language_tag_setting { edit_language_setting_label } else { format!("{}: {}", Self::edit_in_json_label(), data_element.setting.key) },
        );
    }

    /// `renderValidations(dataElement, template)`.
    fn render_validations(&self, data_element: &Rc<SettingsTreeSettingElement>, template: &ISettingComplexItemTemplate) {
        let ui = self.services.ui();
        let mut tree = ui.tree_mut();
        match invalid_type_error(data_element) {
            Some(error_message) => {
                tree.add_class(template.container_element, "invalid-input");
                // `validationErrorMessageElement.innerText = errMsg`
                set_text(&mut tree, template.validation_error_message_element, &error_message, 400, self.services.styles.validation_error_foreground);
                tree.style_mut(template.validation_error_message_element).visible = true;
            }
            None => {
                tree.remove_class(template.container_element, "invalid-input");
                tree.style_mut(template.validation_error_message_element).visible = false;
            }
        }
    }

    /// `.setting-item-validation-message` absoluta, logo abaixo do conteúdo (sem mudar a altura da linha).
    fn place_validation_message(&self, template: &ISettingComplexItemTemplate) {
        let ui = self.services.ui();
        let mut tree = ui.tree_mut();
        let element = template.validation_error_message_element;
        if !tree.style(element).visible {
            return;
        }
        let line_height = self.services.styles.markdown.line_height;
        let text_width = node_extent(&tree, element);
        let inset = VALIDATION_PADDING + VALIDATION_BORDER;
        let top = template.content_bottom.get() - VALIDATION_BORDER;
        tree.set_bounds(element, Rect::new(ITEM_PADDING_X, top, ITEM_PADDING_X + text_width + 2.0 * inset, top + line_height + 2.0 * inset));
        let styles = &self.services.styles;
        let style = tree.style_mut(element);
        style.background = styles.validation_error_background;
        style.border = styles.validation_error_border.map(|color| Border { width: VALIDATION_BORDER, color });
        style.z_index = 1;
    }

    /// O layout da linha complexa: o botão de editar é o controle (`.setting-item-control`), e a mensagem de validação fica
    /// abaixo do conteúdo. Devolve a altura da linha.
    pub fn layout(&self, template: &ISettingComplexItemTemplate) -> f64 {
        let ui = self.services.ui();
        let line_height = self.services.styles.markdown.line_height;
        let button_width = node_extent(&ui.tree(), template.button);
        ui.tree_mut().set_bounds(template.button, Rect::new(0.0, 0.0, button_width, line_height));
        let height = self.layout_setting_item(&template.common, (button_width, line_height));
        self.place_validation_message(template);
        height
    }
}

impl ITreeRenderer<SettingsObjectTreeModel> for SettingComplexRenderer {
    fn template_id(&self) -> String {
        SETTINGS_COMPLEX_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let common = self.render_common_template(container, "complex");

        let button = self.create_edit_button(common.control_element, "");
        let validation_error_message_element = self.create_validation_message(common.container_element);

        let template = ISettingComplexItemTemplate { common, button, validation_error_message_element };
        self.add_setting_element_focus_handler(&template.common);
        Rc::new(template) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<SettingsTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let (Some(template), Some(element)) = (template_data.downcast_ref::<ISettingComplexItemTemplate>(), setting_of(node)) else { return };
        self.render_setting_element(element, &template.common, &|_on_change| self.render_value(element, template));
        self.layout(template);
    }

    fn dispose_element(&self, _node: &Rc<SettingsTreeNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let Some(template) = template_data.downcast_ref::<ISettingComplexItemTemplate>() {
            self.base.dispose_element(&template.common);
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<ISettingComplexItemTemplate>() {
            self.base.dispose_template(&template.common);
        }
    }
}

// ---- SettingComplexObjectRenderer (linhas 1287 a 1326) ---------------------------------------------------------------------------

/// `SettingComplexObjectRenderer`.
pub struct SettingComplexObjectRenderer {
    complex: Rc<SettingComplexRenderer>,
}

impl Deref for SettingComplexObjectRenderer {
    type Target = SettingComplexRenderer;

    fn deref(&self) -> &SettingComplexRenderer {
        &self.complex
    }
}

impl SettingComplexObjectRenderer {
    pub fn new(services: SettingRendererServices, setting_actions: Vec<ActionRef>, disposable_action_factory: DisposableActionFactory) -> Rc<Self> {
        Rc::new(Self { complex: SettingComplexRenderer::new(services, setting_actions, disposable_action_factory) })
    }

    /// `renderValue(dataElement, template, onChange)`.
    fn render_value(&self, data_element: &Rc<SettingsTreeSettingElement>, template: &ISettingComplexObjectItemTemplate) {
        let items = get_object_display_value(data_element);
        template.object_setting_widget.set_value(
            items,
            Some(ObjectSetValueOptions {
                setting_key: data_element.setting.key.clone(),
                show_add_button: false,
                is_read_only: Some(true),
                key_suggester: None,
                value_suggester: None,
                property_names: None,
            }),
        );
        let hide = data_element.has_policy_value() || data_element.is_agents_window_read_only();
        {
            let mut tree = self.services.ui().tree_mut();
            tree.toggle_class(template.button_container, "hide", hide);
            tree.style_mut(template.button_container).visible = !hide;
        }
        self.complex.render_value(data_element, &template.complex);
    }

    /// O layout da linha: a lista de objeto na largura do texto, depois o botão de editar (`margin-top: 9px`).
    pub fn layout(&self, template: &ISettingComplexObjectItemTemplate) -> f64 {
        let ui = self.services.ui();
        let row_width = ui.tree().bounds(template.row_element).width();
        let text_width = item_text_width(row_width);
        let dom_node = template.object_setting_widget.dom_node();
        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(template.control_element, Rect::new(0.0, 0.0, text_width, 0.0));
            let bounds = tree.bounds(dom_node);
            tree.set_bounds(dom_node, Rect::new(0.0, 0.0, text_width, bounds.height()));
        }
        let list_height = template.object_setting_widget.layout();

        let button_visible = ui.tree().style(template.button_container).visible;
        let line_height = self.services.styles.markdown.line_height;
        let button_width = node_extent(&ui.tree(), template.button);
        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(template.button, Rect::new(0.0, 0.0, button_width, line_height));
            let top = list_height + COMPLEX_BUTTON_MARGIN_TOP;
            tree.set_bounds(template.button_container, Rect::new(0.0, top, button_width, top + line_height));
        }
        let control_height = list_height + if button_visible { COMPLEX_BUTTON_MARGIN_TOP + line_height } else { 0.0 };
        let height = self.layout_setting_item(&template.common, (text_width, control_height));
        self.place_validation_message(&template.complex);
        height
    }
}

impl ITreeRenderer<SettingsObjectTreeModel> for SettingComplexObjectRenderer {
    fn template_id(&self) -> String {
        SETTINGS_COMPLEX_OBJECT_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let common = self.render_common_template(container, "list");

        let object_setting_widget = common.to_dispose.add_rc(ObjectSettingDropdownWidget::new(self.services.widgets.clone(), common.control_element));
        let ui = self.services.ui();
        ui.tree_mut().add_class(object_setting_widget.dom_node(), AbstractSettingRenderer::CONTROL_CLASS);

        let (button_container, button) = {
            let button_container = create_el(&mut ui.tree_mut(), common.control_element, Role::Generic, "complex-object-edit-in-settings-button-container");
            (button_container, self.create_edit_button(button_container, "complex-object "))
        };
        let validation_error_message_element = self.create_validation_message(common.container_element);

        let template = ISettingComplexObjectItemTemplate {
            complex: ISettingComplexItemTemplate { common, button, validation_error_message_element },
            object_setting_widget,
            button_container,
        };
        self.add_setting_element_focus_handler(&template.common);
        Rc::new(template) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<SettingsTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let (Some(template), Some(element)) = (template_data.downcast_ref::<ISettingComplexObjectItemTemplate>(), setting_of(node)) else { return };
        self.render_setting_element(element, &template.common, &|_on_change| self.render_value(element, template));
        self.layout(template);
    }

    fn dispose_element(&self, _node: &Rc<SettingsTreeNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let Some(template) = template_data.downcast_ref::<ISettingComplexObjectItemTemplate>() {
            self.complex.base.dispose_element(&template.common);
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<ISettingComplexObjectItemTemplate>() {
            self.complex.base.dispose_template(&template.common);
        }
    }
}
