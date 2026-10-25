//! Porte de `src/vs/workbench/contrib/preferences/browser/settingsWidgets.ts` (+ `media/settingsWidgets.css`), da parte
//! que é das listas de configuração: os tipos de item (`IListDataItem`, `IObjectDataItem`, `IBoolObjectDataItem`,
//! `IIncludeExcludeDataItem`, `ObjectKey`, `ObjectValue`, `IObjectEnumData`, os sugeridores), o modelo da lista
//! ([`ListSettingListModel`]), a base [`AbstractListSettingWidget`] (estado, edição de linha, navegação por teclado,
//! seleção, `onDidChangeList`) e os widgets concretos: o [`ListSettingWidget`] (linha com valor e irmão, edição com
//! `InputBox` ou `SelectBox`, botões OK e Cancelar, ações de editar e remover, dica de cada linha, arrastar para
//! reordenar), o [`IncludeSettingWidget`] e o [`ExcludeSettingWidget`] (o `ListSettingWidget` com outras classes,
//! textos, dica em Markdown e sem arrastar, que são [`ListSettingWidgetOptions`]), o [`ObjectSettingDropdownWidget`]
//! (cabeçalho, linhas de chave e valor com zebra, chave inválida, edição com campo ou lista para a chave e para o
//! valor, sugestão de valor) e o [`ObjectSettingCheckboxWidget`] (uma linha de edição por item, com a caixa de
//! marcação). `ISettingListChangeEvent` e os demais são o [`SettingListEvent`].
//!
//! O arquivo do original termina no `ObjectSettingCheckboxWidget`: não há mais nada depois dele. As regras de
//! `.settings-tabs-widget` e `.settings-header-widget` do mesmo CSS não são de lista e ficam com os widgets que usam
//! essas classes.
//!
//! Divergências da fatia 2:
//!
//! - **Caixa de marcação.** O original usa um `Toggle` com o ícone `check` e as classes `setting-value-checkbox`, e o
//!   CSS (`.setting-item-bool .setting-value-checkbox`) o deixa com `18px`, borda transparente de `1px` pintada por
//!   `!important` com `settings.checkboxBorder`, `settings.checkboxBackground` e `settings.checkboxForeground`, e o
//!   `::before` do ícone sem opacidade quando desmarcado. É exatamente o `Checkbox` do `je-widgets` (`18px`, ícone só
//!   marcado, cores do estilo), então o widget usa ele com [`SettingsWidgetStyles::checkbox`], mais a classe
//!   `setting-value-checkbox`. O `domNode` do `Toggle` também ganha `monaco-checkbox`, que o original não tem.
//! - **Item de caixa de marcação.** `IBoolObjectDataItem.key` (`IObjectStringData`) e `.value` (`IObjectBoolData`) são o
//!   texto e o `bool`; `removable: false` é fixo e não é campo. O `changedItem.value.data = newValue` do original muda
//!   o objeto compartilhado com o modelo (e com o `originalItem`): aqui o item novo vai ao modelo
//!   ([`ListSettingListModel::set_data_item`]) e é também o `original_item` do evento.
//! - **`IIncludeExcludeDataItem.elementType`** é `Option`: o `getEmptyItem` do original o deixa ausente (o cast `as
//!   TListDataItem`).
//! - **Tipo da chave de objeto.** `renderEditWidget(keyOrValue: ObjectKey | ObjectValue)` recebe um [`ObjectValue`] (a
//!   chave vira valor por `From`), e o `update` devolve um [`ObjectValue`] que a chave volta a [`ObjectKey`] com
//!   [`ObjectValue::into_key`] (a chave nunca é booleana).
//! - **TDZ.** O `okButton` e o `renderLatestValue` do `renderEdit` são `const` declarados depois do primeiro `update` da
//!   chave: se a chave começa como lista com um dado fora das opções, o original lança `ReferenceError`; aqui o `update`
//!   só não encontra o botão (ainda não existe) e segue.
//!
//! Layout da fatia 2 (sem motor de CSS, como a fatia 1):
//!
//! - **Linha de objeto.** `padding-right: 4px`; a chave tem `margin: 0 3px 0 4px` e `width: 40%`, o valor `60%` (os
//!   percentuais são da largura do conteúdo da linha) com `padding-right: 40px` (`box-sizing: border-box`) só na linha de
//!   dados; `flex: none`, então a soma passa da linha em `11px`, como no navegador. O cabeçalho é a mesma coisa em negrito,
//!   com o valor sem o `padding-right`. A zebra (`:nth-child(odd)`, contando o cabeçalho) e o `invalid-key`
//!   ([`SettingsWidgetStyles::list_error_foreground`]) ficam em [`AbstractListSettingWidget::refresh_rows`].
//! - **Linha de edição de objeto.** O `flex` da linha: o campo (ou lista) da chave com `min-width: 40%` e `flex-shrink: 1`
//!   (a margem de `4px` do `.setting-list-object-input` fica dentro dele no campo de texto e fora na lista), o contêiner do
//!   valor com `width: 100%` e `flex-shrink: 1`, os botões rígidos. `resolve_flex_shrink` é o laço de encolher do
//!   CSS Flexbox (itens congelados por `min-width`). Sem `showAddButton` a chave é texto rígido.
//! - **Linha de caixa de marcação.** Contêiner de `4px` de margem esquerda, `27px` de largura (a caixa `18px` mais a margem
//!   de `9px`) e `24px` de altura, com a caixa a `3px` do topo, e o valor ocupando o resto (`flex: 1`).
//! - NÃO MEDIDO contra o oráculo: a largura intrínseca da chave em lista (`natural_size` do `SelectBox`), a altura de
//!   `24px` imposta à lista da chave e do valor (o `select` do CSS), a largura intrínseca do campo de texto da chave
//!   (a mesma do `<input>` de `size="20"` da fatia 1, sem o teto de `320px`, que o objeto anula com `max-width: unset`) e o
//!   desenho da zebra com a cor `rgba(130, 130, 130, 0.04)` arredondada em oito bits (`10/255`).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - **Herança.** `AbstractListSettingWidget` é uma estrutura com os métodos abstratos num trait,
//!   [`ListSettingDelegate`], que cada widget concreto implementa (e registra como `Weak`, como o `ViewPane` do
//!   porte). Como o TypeScript chama os métodos abstratos já dentro do construtor da base, a base só os usa em
//!   [`AbstractListSettingWidget::init`], depois que o widget concreto existe. `getEmptyItem` virou o parâmetro
//!   `empty_item` do construtor (os dois widgets devolvem uma constante); `ListSettingWidget` herda os métodos da base
//!   por `Deref`.
//! - **Serviços.** `IThemeService` e `IConfigurationService` do construtor só serviam à `createBasicSelectBox`
//!   (`useCustomDrawn`, sem efeito no porte, ver `select_box.rs`) e às cores: as cores vêm resolvidas do tema em
//!   [`SettingsWidgetStyles::from_theme`] (`getInputBoxStyle` e `getSelectBoxStyles` com as chaves `settings.*`).
//!   `IContextViewService` é o [`IContextViewProvider`] e `IHoverService` o [`IHoverDelegate2`], em
//!   [`SettingsWidgetServices`].
//! - **Layout.** Não há motor de CSS: [`AbstractListSettingWidget::layout`] empilha as linhas (`display: flex`, `max-height:
//!   24px`, `padding: 1px` e `margin-bottom: 1px` da lista, `margin-top: 4px` do botão de adicionar) e devolve a altura
//!   total; quem muda a largura do contêiner chama `layout()` de novo. A linha de dados põe o valor a 60% e o irmão a 40% da
//!   largura (`width: 100%` do valor sem irmão), com as margens, o `padding-right` de 40px e 50px e as reticências do
//!   `text-overflow`. A linha de edição põe os campos como o flex faria (largura intrínseca do `<input>` com no máximo
//!   320px, encolhendo na falta de espaço). NÃO MEDIDO contra o oráculo: a largura intrínseca do `<input>` (vinte vezes a
//!   largura de `0`, mais padding e borda), o tamanho dos itens da barra de ações (o `ActionViewItem` dá o tamanho do
//!   `actionbar.css`, não os `16px x 20px` com `margin-right: 2px` do `settingsWidgets.css`) e a folga de linha do
//!   botão inline-block.
//! - **Estados do CSS.** `:hover`, `.selected:focus`, `.selected:not(:focus)`, `.draggable` e `.drag-hover` viram
//!   [`AbstractListSettingWidget::refresh_rows`], chamada nos ouvintes de ponteiro e de foco e nas trocas de seleção. A
//!   barra de ações da linha (`display: none`, visível em `:hover` e `.selected`) é um nó visível ou não. Uma variável de
//!   CSS sem valor no tema deixa a propriedade sem efeito (a cor do texto volta à do widget).
//! - **Atributos.** `role`, `data-index`, `tabindex`, `draggable` e `aria-label` das linhas são os atributos do nó
//!   ([`je_ui::Tree::set_attribute`]), como o original os lê e escreve.
//! - **Arrastar e soltar.** O `je-ui` não tem o arrastar e soltar do DOM. As linhas arrastáveis são tratadas com os eventos
//!   de ponteiro: o `dragstart` é o movimento de 4px (o limiar do Chromium) com o botão primário baixo numa linha
//!   arrastável, `dragenter` e `dragleave` são a linha sob o ponteiro (classe `drag-hover`), `drop` e `dragend` são o
//!   botão alto (o `move` sai se a linha de destino é outra). Os ouvintes de movimento e de soltura ficam na raiz da
//!   árvore, como o `sash`, porque o botão pode subir fora da lista.
//! - **Foco da linha de edição.** O `rowElement?.focus()` do fim do `onKeyDown` da edição não faz nada no DOM (a
//!   `div` da edição não é focável); aqui seria roubar o foco do campo, então não existe.
//! - **Texto da linha.** O `textContent` do valor e do irmão é o texto de [`je_ui::Content::Text`] do nó; ele só é
//!   encurtado com reticências quando a lista tem largura (lista sem largura mostra o texto inteiro).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::async_::disposable_timeout;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::html_content::{IMarkdownString, MarkdownString, TrustedOrOptions};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Content, Cursor, Event, EventKind, NodeId, PointerButton, Role, Tree, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::{ActionBarStyles, IActionViewItemOptions, default_action_bar_styles};
use je_widgets::button::{Button, ButtonLabel, ButtonOptions, ButtonStyles, IButton, default_button_styles};
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::dom::{StandardKeyboardEvent, create_detached_el, create_el, is_ancestor_of_active_element};
use je_widgets::hover_api::{HoverContent, HoverTarget, IHoverDelegate2, IHoverOptions};
use je_widgets::input_box::{InputBox, InputBoxOptions, InputBoxStyles, default_input_box_styles};
use je_widgets::select_box::{ISelectBoxDelegate, ISelectBoxStyles, ISelectOptionItem, SelectBox, default_select_box_styles};
use je_widgets::text::{UI_FONT_SIZE, ellipsize, measure_text, ui_font};
use je_widgets::toggle::{Checkbox, CheckboxStyles, default_checkbox_styles};

use crate::services::preferences::preferences::SettingValueType;
use crate::services::preferences::preferences_validation::validate_property_name;

/// `registerIcon('settings-edit', Codicon.edit)`: o ícone registrado é o próprio codicon padrão dele (como o
/// `view-pane-container-expanded` de `view_pane.rs`).
pub const SETTINGS_EDIT_ICON: &str = "edit";
/// `registerIcon('settings-remove', Codicon.close)`.
pub const SETTINGS_REMOVE_ICON: &str = "close";
/// `registerIcon('settings-discard', Codicon.discard)`.
pub const SETTINGS_DISCARD_ICON: &str = "discard";

/// O `line-height: 24px` (e `min-height`) das linhas, e o `height: 24px` dos campos da edição.
const ROW_HEIGHT: f64 = 24.0;
/// O `padding: 1px` da lista (e o `margin-bottom: 1px`).
const LIST_PADDING: f64 = 1.0;
const LIST_MARGIN_BOTTOM: f64 = 1.0;
/// `.setting-list-value`: `margin-left: 2px`, `margin-right: 3px`, `padding-right: 40px`.
const VALUE_MARGIN_LEFT: f64 = 2.0;
const VALUE_MARGIN_RIGHT: f64 = 3.0;
const VALUE_PADDING_RIGHT: f64 = 40.0;
/// `.setting-list-sibling`: `margin-left: 0.5em`, `font-size: 0.9em`, `padding-right: 50px`, `opacity: 0.7`.
const SIBLING_MARGIN_LEFT_EM: f64 = 0.5;
const SIBLING_FONT_SCALE: f32 = 0.9;
const SIBLING_PADDING_RIGHT: f64 = 50.0;
const SIBLING_OPACITY: f32 = 0.7;
/// O `margin-top: 4px` do botão de adicionar.
const ADD_BUTTON_MARGIN_TOP: f64 = 4.0;
/// `.setting-list-valueInput`: `max-width: 320px`, `margin-right: 4px`; `.setting-list-ok-button`: `margin: 0 4px`.
const INPUT_MAX_WIDTH: f64 = 320.0;
const INPUT_MARGIN_RIGHT: f64 = 4.0;
const OK_BUTTON_MARGIN: f64 = 4.0;
/// `.select-container` da lista: `width: 320px`.
const SELECT_WIDTH: f64 = 320.0;
/// O `padding: 4px 6px` mais a borda de 1px do `InputBox`, para a largura intrínseca do `<input>`.
const INPUT_CHROME_WIDTH: f64 = 2.0 * 6.0 + 2.0;
/// O `size="20"` padrão do `<input>`, em caracteres.
const INPUT_INTRINSIC_CHARS: usize = 20;
/// O movimento, em px, que começa um arraste.
const DRAG_THRESHOLD: f64 = 4.0;
/// O `padding-right: 4px` das linhas e do cabeçalho do widget de objeto.
const OBJECT_ROW_PADDING_RIGHT: f64 = 4.0;
/// `.setting-list-object-key`: `margin-left: 4px`, `margin-right: 3px`, `width: 40%`; o valor, `width: 60%`.
const OBJECT_KEY_MARGIN_LEFT: f64 = 4.0;
const OBJECT_KEY_MARGIN_RIGHT: f64 = 3.0;
const OBJECT_KEY_WIDTH: f64 = 0.4;
const OBJECT_VALUE_WIDTH: f64 = 0.6;
/// O `padding-right: 40px` do valor da linha de dados (`.setting-list-row .setting-list-object-value`).
const OBJECT_VALUE_PADDING_RIGHT: f64 = 40.0;
/// `.setting-list-object-input-key { min-width: 40% }`.
const OBJECT_KEY_MIN_WIDTH: f64 = 0.4;
/// O `margin: 0 4px` do botão OK, que ocupa `8px` da linha de edição além dos botões.
const OBJECT_BUTTONS_MARGIN: f64 = 2.0 * OK_BUTTON_MARGIN;
/// A caixa de marcação: `18px`, `margin-right: 9px`, `margin-top: 3px` e o `margin-left: 4px` do contêiner.
const CHECKBOX_SIZE: f64 = 18.0;
const CHECKBOX_MARGIN_RIGHT: f64 = 9.0;
const CHECKBOX_MARGIN_TOP: f64 = 3.0;
const CHECKBOX_WRAPPER_MARGIN_LEFT: f64 = 4.0;
/// `rgba(130, 130, 130, 0.04)`, a zebra e o fundo do cabeçalho: `0.04 * 255` arredondado.
const ZEBRA_ALPHA: u8 = 10;
/// `getContainerClasses()` dos dois widgets de objeto.
const OBJECT_WIDGET_CLASS: &str = "setting-list-object-widget";

// ---- tipos de item ---------------------------------------------------------------------------------------------------

/// `IObjectEnumOption`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectEnumOption {
    pub value: String,
    pub description: Option<String>,
}

/// `IObjectEnumData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectEnumData {
    pub data: String,
    pub options: Vec<ObjectEnumOption>,
}

/// `ObjectKey`: `IObjectStringData | IObjectEnumData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectKey {
    String(String),
    Enum(ObjectEnumData),
}

impl ObjectKey {
    /// `key.data`.
    pub fn data(&self) -> &str {
        match self {
            Self::String(data) => data,
            Self::Enum(enum_data) => &enum_data.data,
        }
    }
}

/// `ObjectValue`: `IObjectStringData | IObjectEnumData | IObjectBoolData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectValue {
    String(String),
    Enum(ObjectEnumData),
    Boolean(bool),
}

impl ObjectValue {
    /// `value.data.toString()`.
    pub fn data_string(&self) -> String {
        match self {
            Self::String(data) => data.clone(),
            Self::Enum(enum_data) => enum_data.data.clone(),
            Self::Boolean(data) => data.to_string(),
        }
    }

    /// A volta de [`From<ObjectKey>`]: `None` para o booleano (uma chave nunca é booleana).
    pub fn into_key(self) -> Option<ObjectKey> {
        match self {
            Self::String(data) => Some(ObjectKey::String(data)),
            Self::Enum(enum_data) => Some(ObjectKey::Enum(enum_data)),
            Self::Boolean(_) => None,
        }
    }
}

impl From<ObjectKey> for ObjectValue {
    fn from(key: ObjectKey) -> Self {
        match key {
            ObjectKey::String(data) => Self::String(data),
            ObjectKey::Enum(enum_data) => Self::Enum(enum_data),
        }
    }
}

/// `IListDataItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IListDataItem {
    pub value: ObjectKey,
    pub sibling: Option<String>,
}

/// `IObjectDataItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IObjectDataItem {
    pub key: ObjectKey,
    pub value: ObjectValue,
    pub key_description: Option<String>,
    pub source: Option<String>,
    pub removable: bool,
    pub resetable: bool,
}

/// `IBoolObjectDataItem`: `key` e `value` são o `data` de `IObjectStringData` e de `IObjectBoolData`, e `removable` é
/// sempre `false` (o tipo do original o fixa), por isso não é campo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IBoolObjectDataItem {
    pub key: String,
    pub value: bool,
    pub key_description: Option<String>,
    pub source: Option<String>,
    pub resetable: bool,
}

/// `IIncludeExcludeDataItem`. `element_type` é `Option` porque o `getEmptyItem` do original não o preenche.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IIncludeExcludeDataItem {
    pub value: ObjectKey,
    pub element_type: Option<SettingValueType>,
    pub sibling: Option<String>,
    pub source: Option<String>,
}

/// `IObjectValueSuggester`.
pub type ObjectValueSuggester = Rc<dyn Fn(&str) -> Option<ObjectValue>>;

/// `IObjectKeySuggester`: recebe as chaves já existentes e o índice da linha.
pub type ObjectKeySuggester = Rc<dyn Fn(&[String], Option<usize>) -> Option<ObjectEnumData>>;

/// `TListDataItem extends IListDataItem` do `ListSettingWidget`: o que o widget lê e escreve de um item de lista.
pub trait ListDataItem: Clone + 'static {
    /// `getEmptyItem()`.
    fn empty() -> Self;
    fn value(&self) -> &ObjectKey;
    fn sibling(&self) -> Option<&str>;
    /// `source` (os itens de inclusão e exclusão o têm; o `IListDataItem` não).
    fn source(&self) -> Option<&str> {
        None
    }
    /// O item editado: `{ ...item, value, sibling }` (a troca de valor do sugeridor) e o `{ value, sibling } as
    /// TListDataItem` do OK da edição. Os campos além de valor e irmão ficam como no original do item (o TypeScript
    /// os deixa ausentes no segundo caso, o que um tipo com campos obrigatórios não representa).
    fn with_value(&self, value: ObjectKey, sibling: Option<String>) -> Self;
}

impl ListDataItem for IIncludeExcludeDataItem {
    fn empty() -> Self {
        Self { value: ObjectKey::String(String::new()), element_type: None, sibling: None, source: None }
    }

    fn value(&self) -> &ObjectKey {
        &self.value
    }

    fn sibling(&self) -> Option<&str> {
        self.sibling.as_deref()
    }

    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    fn with_value(&self, value: ObjectKey, sibling: Option<String>) -> Self {
        Self { value, sibling, ..self.clone() }
    }
}

impl ListDataItem for IListDataItem {
    fn empty() -> Self {
        Self { value: ObjectKey::String(String::new()), sibling: None }
    }

    fn value(&self) -> &ObjectKey {
        &self.value
    }

    fn sibling(&self) -> Option<&str> {
        self.sibling.as_deref()
    }

    fn with_value(&self, value: ObjectKey, sibling: Option<String>) -> Self {
        Self { value, sibling }
    }
}

// ---- estilos e serviços ----------------------------------------------------------------------------------------------

/// As cores que o `settingsWidgets.css` e os helpers de estilo do original resolvem do tema.
#[derive(Clone)]
pub struct SettingsWidgetStyles {
    /// `getInputBoxStyle({ inputBackground: settingsTextInputBackground, ... })`.
    pub input_box: InputBoxStyles,
    /// `getSelectBoxStyles({ selectBackground: settingsSelectBackground, ... })`.
    pub select_box: ISelectBoxStyles,
    /// `defaultButtonStyles`.
    pub button: ButtonStyles,
    pub action_bar: Rc<ActionBarStyles>,
    /// O `Checkbox` do `ObjectSettingCheckboxWidget`: `.setting-value-checkbox` pinta o fundo, o texto e a borda com
    /// `settings.checkboxBackground`, `settings.checkboxForeground` e `settings.checkboxBorder`.
    pub checkbox: CheckboxStyles,
    /// O `color` herdado do texto das linhas.
    pub foreground: Rgba,
    /// `--vscode-list-errorForeground`: a chave inválida das linhas de objeto.
    pub list_error_foreground: Option<Rgba>,
    pub list_hover_background: Option<Rgba>,
    pub list_hover_foreground: Option<Rgba>,
    pub list_active_selection_background: Option<Rgba>,
    pub list_active_selection_foreground: Option<Rgba>,
    pub list_inactive_selection_background: Option<Rgba>,
    pub list_inactive_selection_foreground: Option<Rgba>,
    pub list_drop_background: Option<Rgba>,
}

impl SettingsWidgetStyles {
    pub fn from_theme(theme: &ColorTheme) -> Self {
        let color = |id: &str| theme.get_color(id);
        let mut input_box = default_input_box_styles(theme);
        input_box.input_background = color("settings.textInputBackground");
        input_box.input_foreground = color("settings.textInputForeground");
        input_box.input_border = color("settings.textInputBorder");
        let mut select_box = default_select_box_styles(theme);
        select_box.select_background = color("settings.dropdownBackground");
        select_box.select_foreground = color("settings.dropdownForeground");
        select_box.select_border = color("settings.dropdownBorder");
        select_box.select_list_border = color("settings.dropdownListBorder");
        let mut checkbox = default_checkbox_styles(theme);
        checkbox.checkbox_background = color("settings.checkboxBackground");
        checkbox.checkbox_foreground = color("settings.checkboxForeground");
        checkbox.checkbox_border = color("settings.checkboxBorder");
        Self {
            input_box,
            select_box,
            button: default_button_styles(theme),
            action_bar: Rc::new(default_action_bar_styles(theme)),
            checkbox,
            foreground: color("foreground").unwrap_or(Rgba::opaque(0xcc, 0xcc, 0xcc)),
            list_error_foreground: color("list.errorForeground"),
            list_hover_background: color("list.hoverBackground"),
            list_hover_foreground: color("list.hoverForeground"),
            list_active_selection_background: color("list.activeSelectionBackground"),
            list_active_selection_foreground: color("list.activeSelectionForeground"),
            list_inactive_selection_background: color("list.inactiveSelectionBackground"),
            list_inactive_selection_foreground: color("list.inactiveSelectionForeground"),
            list_drop_background: color("list.dropBackground"),
        }
    }
}

/// Os serviços do construtor do `AbstractListSettingWidget` (ver o `//!` do módulo).
#[derive(Clone)]
pub struct SettingsWidgetServices {
    pub ui: Ui,
    pub context_view_provider: Rc<dyn IContextViewProvider>,
    pub hover_service: Rc<dyn IHoverDelegate2>,
    pub styles: SettingsWidgetStyles,
}

// ---- modelo ----------------------------------------------------------------------------------------------------------

/// `EditKey`: `'none' | 'create' | number`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditKey {
    #[default]
    None,
    Create,
    Index(usize),
}

/// `IListViewItem<TDataItem>`: o item com os estados que o modelo soma.
#[derive(Clone, Debug)]
pub struct ListViewItem<T> {
    pub item: T,
    pub editing: bool,
    pub selected: bool,
}

/// `ListSettingListModel`.
pub struct ListSettingListModel<T> {
    data_items: Vec<T>,
    edit_key: EditKey,
    selected_idx: Option<i64>,
    new_data_item: T,
}

impl<T: Clone> ListSettingListModel<T> {
    pub fn new(new_item: T) -> Self {
        Self { data_items: Vec::new(), edit_key: EditKey::None, selected_idx: None, new_data_item: new_item }
    }

    /// `get items()`.
    pub fn items(&self) -> Vec<ListViewItem<T>> {
        let mut items: Vec<ListViewItem<T>> = self
            .data_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let editing = self.edit_key == EditKey::Index(i);
                ListViewItem { item: item.clone(), editing, selected: self.selected_idx == Some(i as i64) || editing }
            })
            .collect();
        if self.edit_key == EditKey::Create {
            items.push(ListViewItem { item: self.new_data_item.clone(), editing: true, selected: true });
        }
        items
    }

    pub fn set_edit_key(&mut self, key: EditKey) {
        self.edit_key = key;
    }

    pub fn set_value(&mut self, list_data: Vec<T>) {
        self.data_items = list_data;
    }

    /// `this._dataItems[idx] = item`: a escrita de um objeto que o original compartilha com o modelo (o
    /// `changedItem.value.data = ...` do `ObjectSettingCheckboxWidget`). Índice fora da lista não faz nada.
    pub fn set_data_item(&mut self, idx: usize, item: T) {
        if let Some(slot) = self.data_items.get_mut(idx) {
            *slot = item;
        }
    }

    pub fn select(&mut self, idx: Option<i64>) {
        self.selected_idx = idx;
    }

    pub fn get_selected(&self) -> Option<i64> {
        self.selected_idx
    }

    /// `selectNext()`: `Math.min(idx + 1, length - 1)`, que com a lista vazia vale -1.
    pub fn select_next(&mut self) {
        let last = self.data_items.len() as i64 - 1;
        self.selected_idx = Some(self.selected_idx.map_or(0, |idx| (idx + 1).min(last)));
    }

    /// `selectPrevious()`.
    pub fn select_previous(&mut self) {
        self.selected_idx = Some(self.selected_idx.map_or(0, |idx| (idx - 1).max(0)));
    }
}

// ---- eventos ---------------------------------------------------------------------------------------------------------

/// `SettingListEvent<TDataItem>`: `ISettingListChangeEvent | ISettingListAddEvent | ISettingListMoveEvent |
/// ISettingListRemoveEvent | ISettingListResetEvent` (o `type` do original é a variante).
#[derive(Clone, Debug)]
pub enum SettingListEvent<T> {
    Change { original_item: T, new_item: T, target_index: usize },
    Add { new_item: T, target_index: usize },
    Move { original_item: T, new_item: T, target_index: usize, source_index: usize },
    Remove { original_item: T, target_index: usize },
    Reset { original_item: T, target_index: usize },
}

// ---- base ------------------------------------------------------------------------------------------------------------

/// `RowElementGroup`.
#[derive(Clone, Copy, Debug)]
pub struct RowElementGroup {
    pub row_element: NodeId,
    pub key_element: NodeId,
    pub value_element: Option<NodeId>,
}

/// O que `getLocalizedStrings()` devolve e a base lê: os três textos. Os campos que cada widget soma (os `placeholder`
/// da edição do `ListSettingWidget`, o `resetActionTooltip` e os cabeçalhos do widget de objeto) ficam em
/// [`ListPlaceholderStrings`] e [`ObjectSettingStrings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListSettingStrings {
    pub delete_action_tooltip: String,
    pub edit_action_tooltip: String,
    pub add_button_label: String,
}

/// Os dois campos que o `ListSettingWidget` soma aos textos da base: os `placeholder` da edição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListPlaceholderStrings {
    pub input_placeholder: String,
    pub sibling_input_placeholder: String,
}

/// O que `getLocalizedStrings()` dos dois widgets de objeto devolve (é o mesmo nos dois).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectSettingStrings {
    pub list: ListSettingStrings,
    pub reset_action_tooltip: String,
    pub key_header_text: String,
    pub value_header_text: String,
}

fn object_strings() -> ObjectSettingStrings {
    ObjectSettingStrings {
        list: ListSettingStrings {
            delete_action_tooltip: localize("removeItem", "Remove Item", &[]),
            edit_action_tooltip: localize("editItem", "Edit Item", &[]),
            add_button_label: localize("addItem", "Add Item", &[]),
        },
        reset_action_tooltip: localize("resetItem", "Reset Item", &[]),
        key_header_text: localize("objectKeyHeader", "Item", &[]),
        value_header_text: localize("objectValueHeader", "Value", &[]),
    }
}

/// Os métodos abstratos (e os `protected` sobrescritos) do `AbstractListSettingWidget`.
pub trait ListSettingDelegate<T> {
    fn is_item_new(&self, item: &T) -> bool;
    fn get_container_classes(&self) -> Vec<String>;
    fn get_actions_for_item(&self, item: &T, idx: usize) -> Vec<ActionRef>;
    fn render_item(&self, item: &T, idx: usize) -> RowElementGroup;
    fn render_edit(&self, item: &T, idx: usize) -> NodeId;
    fn add_tooltips_to_row(&self, row_element: &RowElementGroup, item: &T);
    fn get_localized_strings(&self) -> ListSettingStrings;
    /// A altura da linha (de dados ou de edição) com a largura dada, depois de posicionar o que ela tem dentro.
    fn layout_row(&self, row: NodeId, width: f64) -> f64;
    fn render_header(&self) -> Option<NodeId> {
        None
    }
    /// A altura do cabeçalho, depois de posicionar o que ele tem dentro.
    fn layout_header(&self, _header: NodeId, _width: f64) -> f64 {
        0.0
    }
    fn is_add_button_visible(&self) -> bool {
        true
    }
    fn is_read_only(&self) -> bool {
        false
    }
    /// O `renderDataOrEditItem` sobrescrito do `ObjectSettingCheckboxWidget`: toda linha, editando ou não, é a de edição.
    fn force_edit_rows(&self) -> bool {
        false
    }
}

/// `DOM.findParentWithClass(node, class)`: o nó ou o antepassado mais próximo com a classe.
fn find_parent_with_class(tree: &Tree, node: NodeId, class: &str) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        if tree.has_class(n, class) {
            return Some(n);
        }
        current = tree.parent(n);
    }
    None
}

/// `AbstractListSettingWidget<TDataItem>`.
pub struct AbstractListSettingWidget<T: Clone + 'static> {
    base: Rc<Disposable>,
    weak_self: Weak<Self>,
    ui: Ui,
    services: SettingsWidgetServices,
    delegate: Weak<dyn ListSettingDelegate<T>>,
    container: NodeId,
    list_element: NodeId,
    new_row: Cell<Option<NodeId>>,
    add_button: RefCell<Option<Rc<Button>>>,
    header: Cell<Option<NodeId>>,
    row_elements: RefCell<Vec<NodeId>>,
    row_bars: RefCell<Vec<Option<Rc<ActionBar>>>>,
    hovered_row: Cell<Option<NodeId>>,
    on_did_change_list: Rc<Emitter<SettingListEvent<T>>>,
    model: RefCell<ListSettingListModel<T>>,
    list_disposables: Rc<DisposableStore>,
}

impl<T: Clone + 'static> AbstractListSettingWidget<T> {
    /// O construtor do original sem o que chama os métodos abstratos (ver o `//!` do módulo): cria a lista como
    /// último filho de `container` e os ouvintes dela. [`AbstractListSettingWidget::init`] termina a montagem.
    pub fn new(services: SettingsWidgetServices, container: NodeId, empty_item: T, delegate: Weak<dyn ListSettingDelegate<T>>) -> Rc<Self> {
        let ui = services.ui.clone();
        let base = Disposable::new();
        let list_element = {
            let mut tree = ui.tree_mut();
            let list = create_el(&mut tree, container, Role::List, "");
            tree.set_fill(list, true, false);
            list
        };
        let on_did_change_list: Rc<Emitter<SettingListEvent<T>>> = Emitter::new();
        base.register(on_did_change_list.clone());
        let list_disposables = DisposableStore::new();
        base.register(list_disposables.clone());
        let this = Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            ui,
            services,
            delegate,
            container,
            list_element,
            new_row: Cell::new(None),
            add_button: RefCell::new(None),
            header: Cell::new(None),
            row_elements: RefCell::new(Vec::new()),
            row_bars: RefCell::new(Vec::new()),
            hovered_row: Cell::new(None),
            on_did_change_list,
            model: RefCell::new(ListSettingListModel::new(empty_item)),
            list_disposables,
        });
        this.base.register(this.listen(list_element, EventKind::PointerDown, |this, e| this.on_list_click(e)));
        this.base.register(this.listen(list_element, EventKind::DoubleClick, |this, e| this.on_list_double_click(e)));
        this.base.register(this.listen(list_element, EventKind::KeyDown, |this, e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            if key.equals(KeyCode::UpArrow.bits()) {
                this.select_previous_row();
            } else if key.equals(KeyCode::DownArrow.bits()) {
                this.select_next_row();
            } else {
                return;
            }
            e.stop(true);
        }));
        this
    }

    /// `this._register(addDisposableListener(node, kind, ...))` com o `this` fraco.
    fn listen(&self, node: NodeId, kind: EventKind, handler: impl Fn(&AbstractListSettingWidget<T>, &mut Event) + 'static) -> DisposableRef {
        let weak = self.weak_self.clone();
        self.ui.listen(node, kind, false, move |e| {
            if let Some(this) = weak.upgrade() {
                handler(&this, e);
            }
        })
    }

    /// O resto do construtor: as classes do contêiner, o botão de adicionar e a primeira montagem da lista. Chamar
    /// uma vez, com o delegado já vivo.
    pub fn init(&self) {
        let Some(delegate) = self.delegate.upgrade() else { return };
        {
            let mut tree = self.ui.tree_mut();
            for class in delegate.get_container_classes() {
                tree.add_class(self.list_element, &class);
            }
        }
        self.render_add_button(&*delegate);
        self.render_list();
    }

    /// `get domNode()`.
    pub fn dom_node(&self) -> NodeId {
        self.list_element
    }

    /// O `container` do construtor.
    pub fn container(&self) -> NodeId {
        self.container
    }

    pub fn on_did_change_list(&self) -> BaseEvent<SettingListEvent<T>> {
        self.on_did_change_list.event()
    }

    /// `get items()`.
    pub fn items(&self) -> Vec<ListViewItem<T>> {
        self.model.borrow().items()
    }

    /// `this.model`, para os widgets concretos.
    pub fn with_model<R>(&self, f: impl FnOnce(&mut ListSettingListModel<T>) -> R) -> R {
        f(&mut self.model.borrow_mut())
    }

    /// `this.listDisposables`.
    pub fn list_disposables(&self) -> &Rc<DisposableStore> {
        &self.list_disposables
    }

    /// Os nós das linhas montadas, na ordem da lista.
    pub fn row_elements(&self) -> Vec<NodeId> {
        self.row_elements.borrow().clone()
    }

    pub fn services(&self) -> &SettingsWidgetServices {
        &self.services
    }

    /// `this._onDidChangeList.fire(event)`.
    pub fn fire_list_event(&self, event: SettingListEvent<T>) {
        self.on_did_change_list.fire(event);
    }

    /// `setValue(listData)`.
    pub fn set_value(&self, list_data: Vec<T>) {
        self.model.borrow_mut().set_value(list_data);
        self.render_list();
    }

    /// `renderList()`.
    pub fn render_list(&self) {
        let Some(delegate) = self.delegate.upgrade() else { return };
        let focused = is_ancestor_of_active_element(&self.ui, self.list_element);

        let children = self.ui.tree().children(self.list_element).to_vec();
        for child in children {
            self.ui.remove(child);
        }
        self.list_disposables.clear();
        self.row_bars.borrow_mut().clear();
        self.hovered_row.set(None);

        let items = self.model.borrow().items();
        let new_mode = items.iter().any(|item| item.editing && delegate.is_item_new(&item.item));
        let hide_add_button = !delegate.is_add_button_visible() || new_mode;
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.container, "setting-list-hide-add-button", hide_add_button);
            if let Some(new_row) = self.new_row.get() {
                tree.style_mut(new_row).visible = !hide_add_button;
            }
            if items.is_empty() {
                tree.remove_attribute(self.list_element, "tabindex");
            } else {
                tree.set_attribute(self.list_element, "tabindex", "0");
            }
        }

        let header = delegate.render_header();
        self.header.set(header);
        if let Some(header) = header {
            self.ui.tree_mut().append_child(self.list_element, header);
        }

        let rows: Vec<NodeId> = items.iter().enumerate().map(|(i, item)| self.render_data_or_edit_item(&*delegate, item, i, focused)).collect();
        {
            let mut tree = self.ui.tree_mut();
            for row in &rows {
                tree.append_child(self.list_element, *row);
            }
        }
        *self.row_elements.borrow_mut() = rows;
        self.layout();
        self.refresh_rows();
    }

    /// `createBasicSelectBox(value)`.
    pub fn create_basic_select_box(&self, value: &ObjectEnumData) -> Rc<SelectBox> {
        let options: Vec<ISelectOptionItem> =
            value.options.iter().map(|option| ISelectOptionItem { text: option.value.clone(), description: option.description.clone(), ..ISelectOptionItem::default() }).collect();
        let selected = value.options.iter().position(|option| option.value == value.data).unwrap_or(0);
        SelectBox::new(&self.ui, options, selected, self.services.context_view_provider.clone(), self.services.styles.select_box.clone(), None)
    }

    /// `editSetting(idx)`.
    pub fn edit_setting(&self, idx: usize) {
        self.model.borrow_mut().set_edit_key(EditKey::Index(idx));
        self.render_list();
    }

    /// `cancelEdit()`.
    pub fn cancel_edit(&self) {
        self.model.borrow_mut().set_edit_key(EditKey::None);
        self.render_list();
    }

    /// `handleItemChange(originalItem, changedItem, idx)`.
    pub fn handle_item_change(&self, original_item: T, changed_item: T, idx: usize) {
        self.model.borrow_mut().set_edit_key(EditKey::None);
        let Some(delegate) = self.delegate.upgrade() else { return };
        let event = if delegate.is_item_new(&original_item) {
            SettingListEvent::Add { new_item: changed_item, target_index: idx }
        } else {
            SettingListEvent::Change { original_item, new_item: changed_item, target_index: idx }
        };
        self.on_did_change_list.fire(event);
        self.render_list();
    }

    fn render_data_or_edit_item(&self, delegate: &dyn ListSettingDelegate<T>, item: &ListViewItem<T>, idx: usize, list_focused: bool) -> NodeId {
        let row = if item.editing || delegate.force_edit_rows() {
            self.row_bars.borrow_mut().push(None);
            delegate.render_edit(&item.item, idx)
        } else {
            self.render_data_item(delegate, item, idx, list_focused)
        };
        self.ui.tree_mut().set_role(row, Role::ListItem);
        row
    }

    fn render_data_item(&self, delegate: &dyn ListSettingDelegate<T>, item: &ListViewItem<T>, idx: usize, list_focused: bool) -> NodeId {
        let group = delegate.render_item(&item.item, idx);
        let row = group.row_element;
        {
            let mut tree = self.ui.tree_mut();
            tree.set_attribute(row, "data-index", idx.to_string());
            tree.set_attribute(row, "tabindex", if item.selected { "0" } else { "-1" });
            tree.toggle_class(row, "selected", item.selected);
        }

        let action_bar = ActionBar::new(&self.ui, row, IActionBarOptions { styles: Some(self.services.styles.action_bar.clone()), ..IActionBarOptions::default() });
        self.list_disposables.add_rc(action_bar.clone());
        let item_options = IActionViewItemOptions { icon: Some(true), label: Some(true), styles: Some(self.services.styles.action_bar.clone()), ..IActionViewItemOptions::default() };
        action_bar.push(&delegate.get_actions_for_item(&item.item, idx), IActionOptions { item: item_options, index: None });
        self.row_bars.borrow_mut().push(Some(action_bar));
        delegate.add_tooltips_to_row(&group, &item.item);

        if item.selected && list_focused {
            let ui = self.ui.clone();
            let _ = disposable_timeout(move || ui.focus(row), 0, Some(self.list_disposables.clone()));
        }

        // There is a parent list widget, which is the one that holds the list of settings.
        // Prevent the parent widget from trying to interpret this click event.
        self.list_disposables.add(self.ui.listen(row, EventKind::Click, false, |e| e.stop_propagation()));
        self.list_disposables.add(self.listen(row, EventKind::PointerEnter, move |this, _| {
            this.hovered_row.set(Some(row));
            this.refresh_rows();
        }));
        self.list_disposables.add(self.listen(row, EventKind::PointerLeave, move |this, _| {
            if this.hovered_row.get() == Some(row) {
                this.hovered_row.set(None);
            }
            this.refresh_rows();
        }));
        for kind in [EventKind::Focus, EventKind::Blur] {
            self.list_disposables.add(self.listen(row, kind, |this, _| this.refresh_rows()));
        }
        row
    }

    fn render_add_button(&self, delegate: &dyn ListSettingDelegate<T>) {
        let strings = delegate.get_localized_strings();
        let row = {
            let mut tree = self.ui.tree_mut();
            let row = create_el(&mut tree, self.container, Role::Generic, "setting-list-new-row");
            tree.set_fill(row, true, false);
            row
        };
        self.new_row.set(Some(row));
        let button = Button::new(&self.ui, row, ButtonOptions { styles: self.services.styles.button, ..ButtonOptions::default() });
        button.set_label(ButtonLabel::Text(strings.add_button_label));
        self.ui.tree_mut().add_class(button.element(), "setting-list-addButton");
        let weak = self.weak_self.clone();
        self.base.register(button.on_did_click().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.model.borrow_mut().set_edit_key(EditKey::Create);
                this.render_list();
            }
        }));
        self.base.register(button.clone());
        *self.add_button.borrow_mut() = Some(button);
    }

    // ---- layout e estilo ---------------------------------------------------------------------------------------------

    /// Posiciona o cabeçalho, as linhas, as barras de ações e o botão de adicionar com a largura atual da lista e devolve
    /// a altura total (a lista mais o botão de adicionar, se visível).
    pub fn layout(&self) -> f64 {
        let Some(delegate) = self.delegate.upgrade() else { return 0.0 };
        let list_width = self.ui.tree().bounds(self.list_element).width();
        let row_width = (list_width - 2.0 * LIST_PADDING).max(0.0);
        let mut y = LIST_PADDING;
        if let Some(header) = self.header.get() {
            let height = delegate.layout_header(header, row_width);
            self.ui.tree_mut().set_bounds(header, Rect::new(LIST_PADDING, y, LIST_PADDING + row_width, y + height));
            y += height;
        }
        let rows = self.row_elements.borrow().clone();
        for row in &rows {
            let height = delegate.layout_row(*row, row_width);
            self.ui.tree_mut().set_bounds(*row, Rect::new(LIST_PADDING, y, LIST_PADDING + row_width, y + height));
            y += height;
        }
        let list_height = y + LIST_PADDING;
        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(self.list_element, Rect::new(0.0, 0.0, list_width, list_height));
            tree.set_fill(self.list_element, true, false);
        }
        let bars: Vec<Rc<ActionBar>> = self.row_bars.borrow().iter().flatten().cloned().collect();
        for bar in bars {
            self.layout_action_bar(&bar);
        }
        let mut total = list_height + LIST_MARGIN_BOTTOM;
        if let Some(new_row) = self.new_row.get() {
            let visible = self.ui.tree().style(new_row).visible;
            if visible && let Some(button) = self.add_button.borrow().clone() {
                // `display: inline-block; margin-top: 4px`, e `width: initial` do `.monaco-text-button`.
                button.set_height(None);
                button.set_width(Some(button.min_content_width()));
                button.set_position(0.0, ADD_BUTTON_MARGIN_TOP);
                let button_height = button.size().1;
                let bottom = total + ADD_BUTTON_MARGIN_TOP + button_height;
                self.ui.tree_mut().set_bounds(new_row, Rect::new(0.0, total, list_width, bottom));
                total = bottom;
            }
        }
        total
    }

    // ---- peças que os widgets concretos montam e posicionam do mesmo jeito ----------------------------------------------

    /// `this.listDisposables.add(this.hoverService.setupDelayedHover(element, { content }))`.
    pub fn setup_row_hover(&self, node: NodeId, content: HoverContent) {
        let hover = self.services.hover_service.setup_delayed_hover(node, Rc::new(move || IHoverOptions::new(content.clone(), HoverTarget::Node(node))), None);
        self.list_disposables.add(hover);
    }

    /// `this.listDisposables.add(new Button(row, defaultButtonStyles))` com o rótulo e a classe.
    pub fn make_button(&self, row: NodeId, label: String, class: &str, secondary: bool) -> Rc<Button> {
        let button = Button::new(&self.ui, row, ButtonOptions { styles: self.services.styles.button, secondary, ..ButtonOptions::default() });
        button.set_label(ButtonLabel::Text(label));
        self.ui.tree_mut().add_class(button.element(), class);
        self.list_disposables.add_rc(button.clone());
        button
    }

    /// Texto numa caixa de `width` px (`overflow: hidden`, `text-overflow: ellipsis`), com a faixa de `padding_right` px
    /// à direita sem texto.
    pub fn place_text(&self, node: NodeId, text: &str, style: &TextStyle, x: f64, width: f64, padding_right: f64) {
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(node, Rect::new(x, 0.0, x + width, ROW_HEIGHT));
        tree.style_mut(node).clip = true;
        let shown = if width > 0.0 { ellipsize(text, style, (width - padding_right).max(0.0)) } else { text.to_owned() };
        let color = match &tree.style(node).content {
            Content::Text { color, .. } => *color,
            _ => self.services.styles.foreground,
        };
        tree.style_mut(node).content = if shown.is_empty() { Content::None } else { Content::Text { text: shown, style: style.clone(), color, line_height: ROW_HEIGHT } };
    }

    /// Uma caixa da altura e da largura dadas, na posição `x` do pai.
    pub fn place_box(&self, node: NodeId, x: f64, width: f64, height: f64) {
        let mut tree = self.ui.tree_mut();
        tree.set_fill(node, false, false);
        tree.set_bounds(node, Rect::new(x, 0.0, x + width, height));
    }

    /// Um `InputBox` com `height: 24px` e a largura dada, na posição `x` do pai.
    pub fn place_input(&self, input: &InputBox, x: f64, width: f64) {
        let element = input.element();
        self.place_box(element, x, width, ROW_HEIGHT);
        input.layout();
        self.ui.tree_mut().set_bounds(element, Rect::new(x, 0.0, x + width, ROW_HEIGHT));
    }

    /// Os botões OK (`margin: 0 4px`) e Cancelar a partir de `x`, com a altura mínima `height`; devolve a altura da linha
    /// (`align-items: stretch`: os botões a acompanham).
    pub fn place_edit_buttons(&self, ok: &Rc<Button>, cancel: &Rc<Button>, mut x: f64, mut height: f64) -> f64 {
        x += OK_BUTTON_MARGIN;
        for button in [ok, cancel] {
            let button_width = button.min_content_width();
            button.set_height(None);
            button.set_width(Some(button_width));
            button.set_position(x, 0.0);
            height = height.max(button.size().1);
            x += button_width + if Rc::ptr_eq(button, ok) { OK_BUTTON_MARGIN } else { 0.0 };
        }
        for button in [ok, cancel] {
            button.set_height(Some(height));
        }
        height
    }

    /// `.monaco-action-bar { position: absolute; right: 0; top: 0 }`.
    fn layout_action_bar(&self, bar: &ActionBar) {
        bar.layout();
        let count = bar.length();
        let width: f64 = (0..count).map(|i| bar.get_width(i)).sum();
        let height = (0..count).map(|i| bar.get_height(i)).fold(0.0, f64::max);
        let node = bar.dom_node();
        let mut tree = self.ui.tree_mut();
        tree.set_fill(node, false, false);
        tree.set_bounds(node, Rect::new(0.0, 0.0, width, height));
        tree.set_right(node, Some(0.0));
    }

    /// As regras de `:hover`, `.selected:focus`, `.selected:not(:focus)`, `.draggable` e `.drag-hover` das linhas de
    /// dados, e a visibilidade da barra de ações (`display: none` até `:hover` ou `.selected`). As linhas de objeto
    /// (`.setting-list-object-row`, de dados ou de edição) ganham a zebra (`:nth-child(odd)`, contando o cabeçalho; a de
    /// dados só sem `:hover`, `:focus` e `.selected`) e a de dados com `.invalid-key` pinta a chave com
    /// `list.errorForeground`.
    pub fn refresh_rows(&self) {
        let rows = self.row_elements.borrow().clone();
        let bars = self.row_bars.borrow().clone();
        let focused = self.ui.focused();
        let hovered = self.hovered_row.get();
        let styles = &self.services.styles;
        let header_offset = usize::from(self.header.get().is_some());
        let mut tree = self.ui.tree_mut();
        for (i, row) in rows.iter().enumerate() {
            let is_data_row = tree.has_class(*row, "setting-list-row");
            let is_object_row = tree.has_class(*row, "setting-list-object-row");
            if !is_data_row && !is_object_row {
                continue;
            }
            let is_hovered = hovered == Some(*row);
            let selected = tree.has_class(*row, "selected");
            let mut background = None;
            let mut foreground = styles.foreground;
            if is_data_row {
                if is_hovered {
                    background = styles.list_hover_background;
                    foreground = styles.list_hover_foreground.unwrap_or(styles.foreground);
                }
                if selected {
                    let (selection_background, selection_foreground) = if focused == Some(*row) {
                        (styles.list_active_selection_background, styles.list_active_selection_foreground)
                    } else {
                        (styles.list_inactive_selection_background, styles.list_inactive_selection_foreground)
                    };
                    background = selection_background;
                    foreground = selection_foreground.unwrap_or(styles.foreground);
                }
                if tree.has_class(*row, "drag-hover") {
                    background = styles.list_drop_background;
                }
            }
            let odd = (i + 1 + header_offset) % 2 == 1;
            if is_object_row && odd && (!is_data_row || (!is_hovered && focused != Some(*row) && !selected)) {
                background = Some(zebra_background());
            }
            tree.style_mut(*row).background = background;
            if !is_data_row {
                continue;
            }
            let draggable = tree.has_class(*row, "draggable");
            let invalid_key = tree.has_class(*row, "invalid-key");
            tree.style_mut(*row).cursor = draggable.then_some(Cursor::Pointer);
            for child in tree.children(*row).to_vec() {
                let color_for_child = if invalid_key && tree.has_class(child, "setting-list-object-key") { styles.list_error_foreground.unwrap_or(foreground) } else { foreground };
                if let Content::Text { color, .. } = &mut tree.style_mut(child).content {
                    *color = color_for_child;
                }
            }
            if let Some(Some(bar)) = bars.get(i) {
                tree.style_mut(bar.dom_node()).visible = is_hovered || selected;
            }
        }
    }

    // ---- entrada -----------------------------------------------------------------------------------------------------

    fn on_list_click(&self, e: &mut Event) {
        let target_idx = self.get_clicked_item_index(e);
        if target_idx < 0 {
            return;
        }
        e.stop(true);
        if self.model.borrow().get_selected() == Some(target_idx) {
            return;
        }
        self.select_row(target_idx);
    }

    fn on_list_double_click(&self, e: &mut Event) {
        let target_idx = self.get_clicked_item_index(e);
        if target_idx < 0 {
            return;
        }
        if self.delegate.upgrade().is_some_and(|delegate| delegate.is_read_only()) {
            return;
        }
        let exists = usize::try_from(target_idx).is_ok_and(|i| i < self.model.borrow().items().len());
        if exists {
            self.edit_setting(target_idx as usize);
            e.stop(true);
        }
    }

    fn get_clicked_item_index(&self, e: &Event) -> i64 {
        let tree = self.ui.tree();
        if find_parent_with_class(&tree, e.target, "monaco-action-bar").is_some() {
            // Don't handle doubleclicks inside the action bar
            return -1;
        }
        let Some(element) = find_parent_with_class(&tree, e.target, "setting-list-row") else { return -1 };
        tree.attribute(element, "data-index").filter(|s| !s.is_empty()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(-1)
    }

    fn select_row(&self, idx: i64) {
        self.model.borrow_mut().select(Some(idx));
        let rows = self.row_elements.borrow().clone();
        {
            let mut tree = self.ui.tree_mut();
            for row in &rows {
                tree.remove_class(*row, "selected");
            }
        }
        let selected = self.model.borrow().get_selected();
        if let Some(row) = selected.and_then(|i| usize::try_from(i).ok()).and_then(|i| rows.get(i)).copied() {
            self.ui.tree_mut().add_class(row, "selected");
            self.ui.focus(row);
        }
        self.refresh_rows();
    }

    fn select_next_row(&self) {
        self.model.borrow_mut().select_next();
        if let Some(idx) = self.model.borrow().get_selected() {
            self.select_row(idx);
        }
    }

    fn select_previous_row(&self) {
        self.model.borrow_mut().select_previous();
        if let Some(idx) = self.model.borrow().get_selected() {
            self.select_row(idx);
        }
    }
}

impl<T: Clone + 'static> IDisposable for AbstractListSettingWidget<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---- ListSettingWidget -----------------------------------------------------------------------------------------------

/// `IListSetValueOptions`.
#[derive(Clone, Default)]
pub struct ListSetValueOptions {
    pub show_add_button: Option<bool>,
    pub key_suggester: Option<ObjectKeySuggester>,
    pub is_read_only: Option<bool>,
}

/// A dica de uma linha: o texto (também o `aria-label`) e, se o conteúdo do hover não é o texto simples, o conteúdo
/// (o `MarkdownString` dos widgets de inclusão e exclusão).
#[derive(Clone, Debug)]
pub struct ListRowTooltip {
    pub title: String,
    pub hover: Option<HoverContent>,
}

/// O que as subclasses `ExcludeSettingWidget` e `IncludeSettingWidget` mudam no `ListSettingWidget`.
#[derive(Clone)]
pub struct ListSettingWidgetOptions<T> {
    /// `getContainerClasses()`.
    pub container_classes: Vec<String>,
    /// Falso onde a subclasse sobrescreve `addDragAndDrop` com um corpo vazio.
    pub drag_and_drop: bool,
    /// `getLocalizedStrings()`.
    pub strings: ListSettingStrings,
    /// Os `placeholder` da edição, também de `getLocalizedStrings()`.
    pub placeholders: ListPlaceholderStrings,
    /// O título que `addTooltipsToRow` monta para o item.
    pub tooltip: Rc<dyn Fn(&T) -> ListRowTooltip>,
    /// `font-family: var(--monaco-monospace-font)` do valor e do irmão (`.setting-list-include-exclude-widget`).
    pub monospace: bool,
}

/// `new MarkdownString().appendMarkdown(text)` como conteúdo de hover.
fn markdown_hover(text: &str) -> HoverContent {
    let mut markdown = MarkdownString::new(None, TrustedOrOptions::default());
    markdown.append_markdown(JsString::from(text).units());
    HoverContent::Markdown(IMarkdownString {
        value: markdown.value,
        is_trusted: markdown.is_trusted,
        support_theme_icons: markdown.support_theme_icons,
        support_html: markdown.support_html,
        support_alert_syntax: markdown.support_alert_syntax,
        base_uri: markdown.base_uri,
        uris: markdown.uris,
    })
}

/// Qual das duas subclasses do `ListSettingWidget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncludeExclude {
    Include,
    Exclude,
}

impl<T: ListDataItem> ListSettingWidgetOptions<T> {
    /// `ExcludeSettingWidget` e `IncludeSettingWidget`: as duas só mudam as palavras (`Exclude` e `Include`).
    fn include_exclude(kind: IncludeExclude) -> Self {
        let exclude = kind == IncludeExclude::Exclude;
        let pick = |exclude_text: String, include_text: String| if exclude { exclude_text } else { include_text };
        Self {
            container_classes: vec!["setting-list-include-exclude-widget".to_owned()],
            drag_and_drop: false,
            strings: ListSettingStrings {
                delete_action_tooltip: pick(localize("removeExcludeItem", "Remove Exclude Item", &[]), localize("removeIncludeItem", "Remove Include Item", &[])),
                edit_action_tooltip: pick(localize("editExcludeItem", "Edit Exclude Item", &[]), localize("editIncludeItem", "Edit Include Item", &[])),
                add_button_label: localize("addPattern", "Add Pattern", &[]),
            },
            placeholders: ListPlaceholderStrings {
                input_placeholder: pick(localize("excludePatternInputPlaceholder", "Exclude Pattern...", &[]), localize("includePatternInputPlaceholder", "Include Pattern...", &[])),
                sibling_input_placeholder: pick(
                    localize("excludeSiblingInputPlaceholder", "When Pattern Is Present...", &[]),
                    localize("includeSiblingInputPlaceholder", "When Pattern Is Present...", &[]),
                ),
            },
            tooltip: Rc::new(move |item: &T| {
                let value = item.value().data();
                let mut title = match (item.sibling(), exclude) {
                    (None, true) => localize("excludePatternHintLabel", "Exclude files matching `{0}`", &[value]),
                    (None, false) => localize("includePatternHintLabel", "Include files matching `{0}`", &[value]),
                    (Some(sibling), true) => localize("excludeSiblingHintLabel", "Exclude files matching `{0}`, only when a file matching `{1}` is present", &[value, sibling]),
                    (Some(sibling), false) => localize("includeSiblingHintLabel", "Include files matching `{0}`, only when a file matching `{1}` is present", &[value, sibling]),
                };
                if let Some(source) = item.source().filter(|source| !source.is_empty()) {
                    title += &localize("excludeIncludeSource", ". Default value provided by `{0}`", &[source]);
                }
                let hover = Some(markdown_hover(&title));
                ListRowTooltip { title, hover }
            }),
            monospace: true,
        }
    }
}

impl<T: ListDataItem> ListSettingWidgetOptions<T> {
    /// O `ListSettingWidget` em si.
    pub fn list() -> Self {
        Self {
            container_classes: vec!["setting-list-widget".to_owned()],
            drag_and_drop: true,
            strings: ListSettingStrings {
                delete_action_tooltip: localize("removeItem", "Remove Item", &[]),
                edit_action_tooltip: localize("editItem", "Edit Item", &[]),
                add_button_label: localize("addItem", "Add Item", &[]),
            },
            placeholders: ListPlaceholderStrings {
                input_placeholder: localize("itemInputPlaceholder", "Item...", &[]),
                sibling_input_placeholder: localize("listSiblingInputPlaceholder", "Sibling...", &[]),
            },
            tooltip: Rc::new(|item: &T| {
                let value = item.value().data();
                let title = match item.sibling() {
                    None => localize("listValueHintLabel", "List item `{0}`", &[value]),
                    Some(sibling) => localize("listSiblingHintLabel", "List item `{0}` with sibling `${1}`", &[value, sibling]),
                };
                ListRowTooltip { title, hover: None }
            }),
            monospace: false,
        }
    }
}

/// `ListSettingWidgetDragDetails` mais o ponto onde o botão baixou (o `dragstart` só vem depois do limiar).
struct PendingDrag<T> {
    element: NodeId,
    item: T,
    item_index: usize,
    start: (f64, f64),
    started: bool,
}

#[derive(Clone)]
enum EditValue {
    Input(Rc<InputBox>),
    Select { wrapper: NodeId, select: Rc<SelectBox> },
}

#[derive(Clone)]
struct EditRowLayout {
    value: EditValue,
    sibling: Option<Rc<InputBox>>,
    ok: Rc<Button>,
    cancel: Rc<Button>,
}

#[derive(Clone)]
enum RowLayout {
    Data { value_node: NodeId, sibling_node: NodeId, value_text: String, sibling_text: Option<String> },
    Edit(EditRowLayout),
}

/// `ListSettingWidget<TListDataItem extends IListDataItem>`.
pub struct ListSettingWidget<T: ListDataItem> {
    own: Rc<Disposable>,
    weak_self: Weak<Self>,
    ui: Ui,
    services: SettingsWidgetServices,
    options: ListSettingWidgetOptions<T>,
    base: Rc<AbstractListSettingWidget<T>>,
    key_value_suggester: RefCell<Option<ObjectKeySuggester>>,
    show_add_button: Cell<bool>,
    is_editable: Cell<bool>,
    pending_drag: RefCell<Option<PendingDrag<T>>>,
    row_layouts: RowLayouts<RowLayout>,
}

impl<T: ListDataItem> Deref for ListSettingWidget<T> {
    type Target = AbstractListSettingWidget<T>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl<T: ListDataItem> ListSettingWidget<T> {
    pub fn new(services: SettingsWidgetServices, container: NodeId, options: ListSettingWidgetOptions<T>) -> Rc<Self> {
        let ui = services.ui.clone();
        let own = Disposable::new();
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let delegate: Weak<dyn ListSettingDelegate<T>> = weak.clone();
            Self {
                base: AbstractListSettingWidget::new(services.clone(), container, T::empty(), delegate),
                own,
                weak_self: weak.clone(),
                ui: ui.clone(),
                services,
                options,
                key_value_suggester: RefCell::new(None),
                show_add_button: Cell::new(true),
                is_editable: Cell::new(true),
                pending_drag: RefCell::new(None),
                row_layouts: RowLayouts::default(),
            }
        });
        this.base.init();
        let root = ui.root();
        this.own.register(this.listen_root(root, EventKind::PointerMove, |this, e| this.on_drag_move(e)));
        this.own.register(this.listen_root(root, EventKind::PointerUp, |this, e| this.on_drag_end(e)));
        this
    }

    fn listen_root(&self, node: NodeId, kind: EventKind, handler: impl Fn(&ListSettingWidget<T>, &mut Event) + 'static) -> DisposableRef {
        let weak = self.weak_self.clone();
        self.ui.listen(node, kind, false, move |e| {
            if let Some(this) = weak.upgrade() {
                handler(&this, e);
            }
        })
    }

    /// `get isReadOnly()`.
    fn is_read_only_now(&self) -> bool {
        !self.is_editable.get()
    }

    /// `setValue(listData, options?)`.
    pub fn set_value(&self, list_data: Vec<T>, options: Option<ListSetValueOptions>) {
        let options = options.unwrap_or_default();
        *self.key_value_suggester.borrow_mut() = options.key_suggester;
        self.is_editable.set(options.is_read_only.is_none_or(|read_only| !read_only));
        self.show_add_button.set(if self.is_editable.get() { options.show_add_button.unwrap_or(true) } else { false });
        if self.is_read_only_now() {
            self.base.with_model(|model| model.set_edit_key(EditKey::None));
            *self.pending_drag.borrow_mut() = None;
        }
        self.base.set_value(list_data);
    }

    fn value_font(&self, scale: f32) -> TextStyle {
        if self.options.monospace { TextStyle::new("monospace", UI_FONT_SIZE * scale, 400) } else { ui_font(UI_FONT_SIZE * scale, 400) }
    }

    // ---- linha de dados ----------------------------------------------------------------------------------------------

    fn add_drag_and_drop(&self, row: NodeId, item: &T, idx: usize) {
        if self.is_read_only_now() {
            return;
        }
        let all_idle = self.base.items().iter().all(|item| !item.editing);
        {
            let mut tree = self.ui.tree_mut();
            tree.set_attribute(row, "draggable", if all_idle { "true" } else { "false" });
            tree.toggle_class(row, "draggable", all_idle);
        }
        let weak = self.weak_self.clone();
        let item = item.clone();
        self.base.list_disposables().add(self.ui.listen(row, EventKind::PointerDown, false, move |e| {
            let Some(this) = weak.upgrade() else { return };
            let Some(pointer) = e.pointer else { return };
            let tree = this.ui.tree();
            if pointer.button != PointerButton::Primary || tree.attribute(row, "draggable") != Some("true") || find_parent_with_class(&tree, e.target, "monaco-action-bar").is_some() {
                return;
            }
            drop(tree);
            *this.pending_drag.borrow_mut() = Some(PendingDrag { element: row, item: item.clone(), item_index: idx, start: (pointer.x, pointer.y), started: false });
        }));
    }

    /// A linha de dados sob o ponto, se for desta lista.
    fn row_at(&self, x: f64, y: f64) -> Option<NodeId> {
        let tree = self.ui.tree();
        let row = find_parent_with_class(&tree, tree.hit_test(x, y)?, "setting-list-row")?;
        self.base.row_elements().contains(&row).then_some(row)
    }

    fn set_drag_hover(&self, target: Option<NodeId>) {
        {
            let mut tree = self.ui.tree_mut();
            for row in self.base.row_elements() {
                tree.toggle_class(row, "drag-hover", Some(row) == target);
            }
        }
        self.base.refresh_rows();
    }

    fn on_drag_move(&self, e: &Event) {
        let Some(pointer) = e.pointer else { return };
        {
            let mut pending = self.pending_drag.borrow_mut();
            let Some(drag) = pending.as_mut() else { return };
            if !drag.started {
                if (pointer.x - drag.start.0).hypot(pointer.y - drag.start.1) < DRAG_THRESHOLD {
                    return;
                }
                drag.started = true;
            }
        }
        self.set_drag_hover(self.row_at(pointer.x, pointer.y));
    }

    fn on_drag_end(&self, e: &Event) {
        let Some(drag) = self.pending_drag.borrow_mut().take() else { return };
        if !drag.started {
            return;
        }
        let Some(pointer) = e.pointer else { return };
        let target = self.row_at(pointer.x, pointer.y);
        self.set_drag_hover(None);
        let Some(target) = target.filter(|row| *row != drag.element) else { return };
        let target_index = self.ui.tree().attribute(target, "data-index").and_then(|s| s.parse::<usize>().ok());
        let new_item = target_index.and_then(|i| self.base.items().get(i).map(|item| item.item.clone()));
        if let (Some(target_index), Some(new_item)) = (target_index, new_item) {
            self.base.fire_list_event(SettingListEvent::Move { original_item: drag.item, new_item, target_index, source_index: drag.item_index });
        }
    }

    // ---- linha de edição ---------------------------------------------------------------------------------------------

    fn render_input_box(&self, data: &str, row: NodeId) -> Rc<InputBox> {
        let mut options = InputBoxOptions::new(self.services.styles.input_box.clone());
        options.placeholder = Some(self.options.placeholders.input_placeholder.clone());
        let input = InputBox::new(&self.ui, row, Some(self.services.context_view_provider.clone()), options);
        self.ui.tree_mut().add_class(input.element(), "setting-list-valueInput");
        self.base.list_disposables().add_rc(input.clone());
        input.set_value(data);
        input
    }

    fn render_dropdown(&self, value: &ObjectEnumData, row: NodeId) -> (NodeId, Rc<SelectBox>) {
        let select = self.base.create_basic_select_box(value);
        self.base.list_disposables().add_rc(select.clone());
        let wrapper = create_detached_el(&mut self.ui.tree_mut(), Role::Generic, "setting-list-object-list-row");
        select.render(wrapper);
        self.ui.tree_mut().append_child(row, wrapper);
        (wrapper, select)
    }

    // ---- posição -----------------------------------------------------------------------------------------------------

    fn layout_data_row(&self, value_node: NodeId, sibling_node: NodeId, value_text: &str, sibling_text: Option<&str>, width: f64) -> f64 {
        let value_font = self.value_font(1.0);
        let sibling_font = self.value_font(SIBLING_FONT_SCALE);
        let value_width = if sibling_text.is_none() { width } else { 0.6 * width };
        self.place_text(value_node, value_text, &value_font, VALUE_MARGIN_LEFT, value_width, VALUE_PADDING_RIGHT);
        let sibling_x = VALUE_MARGIN_LEFT + value_width + VALUE_MARGIN_RIGHT + SIBLING_MARGIN_LEFT_EM * f64::from(sibling_font.size());
        self.place_text(sibling_node, sibling_text.unwrap_or(""), &sibling_font, sibling_x, 0.4 * width, SIBLING_PADDING_RIGHT);
        ROW_HEIGHT
    }

    /// O flex da linha de edição: valor, irmão, OK (`margin: 0 4px`) e Cancelar.
    fn layout_edit_row(&self, layout: &EditRowLayout, width: f64) -> f64 {
        let intrinsic = input_intrinsic_width(INPUT_MAX_WIDTH);
        let (ok_width, cancel_width) = (layout.ok.min_content_width(), layout.cancel.min_content_width());
        let has_sibling = layout.sibling.is_some();
        let value_flexible = matches!(layout.value, EditValue::Input(_));
        let value_basis = match layout.value {
            EditValue::Input(_) if has_sibling => intrinsic,
            // `.setting-list-valueInput.no-sibling { width: 100% }`
            EditValue::Input(_) => width,
            EditValue::Select { .. } => SELECT_WIDTH,
        };
        let flexible = if value_flexible { value_basis } else { 0.0 } + if has_sibling { intrinsic } else { 0.0 };
        let rigid = if value_flexible { INPUT_MARGIN_RIGHT } else { value_basis }
            + if has_sibling { INPUT_MARGIN_RIGHT } else { 0.0 }
            + ok_width
            + 2.0 * OK_BUTTON_MARGIN
            + cancel_width;
        let scale = if width > 0.0 && flexible > 0.0 && flexible + rigid > width { ((width - rigid) / flexible).max(0.0) } else { 1.0 };

        let mut x = 0.0;
        let mut height = ROW_HEIGHT;
        match &layout.value {
            EditValue::Input(input) => {
                let input_width = value_basis * scale;
                self.place_input(input, x, input_width);
                x += input_width + INPUT_MARGIN_RIGHT;
            }
            EditValue::Select { wrapper, select } => {
                let select_height = select.natural_size().1;
                self.place_box(*wrapper, x, SELECT_WIDTH, select_height);
                select.layout();
                height = height.max(select_height);
                x += SELECT_WIDTH;
            }
        }
        if let Some(sibling) = &layout.sibling {
            let sibling_width = intrinsic * scale;
            self.place_input(sibling, x, sibling_width);
            x += sibling_width + INPUT_MARGIN_RIGHT;
        }
        self.place_edit_buttons(&layout.ok, &layout.cancel, x, height)
    }
}

impl<T: ListDataItem> ListSettingDelegate<T> for ListSettingWidget<T> {
    fn is_item_new(&self, item: &T) -> bool {
        item.value().data().is_empty()
    }

    fn get_container_classes(&self) -> Vec<String> {
        self.options.container_classes.clone()
    }

    fn is_add_button_visible(&self) -> bool {
        self.show_add_button.get()
    }

    fn is_read_only(&self) -> bool {
        self.is_read_only_now()
    }

    fn get_localized_strings(&self) -> ListSettingStrings {
        self.options.strings.clone()
    }

    fn get_actions_for_item(&self, item: &T, idx: usize) -> Vec<ActionRef> {
        if self.is_read_only_now() {
            return Vec::new();
        }
        let strings = &self.options.strings;
        let weak = self.weak_self.clone();
        let edit = list_action("workbench.action.editListItem", &strings.edit_action_tooltip, SETTINGS_EDIT_ICON, move || {
            if let Some(this) = weak.upgrade() {
                this.edit_setting(idx);
            }
        });
        let weak = self.weak_self.clone();
        let item = item.clone();
        let remove = list_action("workbench.action.removeListItem", &strings.delete_action_tooltip, SETTINGS_REMOVE_ICON, move || {
            if let Some(this) = weak.upgrade() {
                this.fire_list_event(SettingListEvent::Remove { original_item: item.clone(), target_index: idx });
            }
        });
        vec![edit, remove]
    }

    fn render_item(&self, item: &T, idx: usize) -> RowElementGroup {
        let (row, value, sibling) = {
            let mut tree = self.ui.tree_mut();
            let row = create_detached_el(&mut tree, Role::Generic, "setting-list-row");
            let value = create_el(&mut tree, row, Role::Generic, "setting-list-value");
            let sibling = create_el(&mut tree, row, Role::Generic, "setting-list-sibling");
            tree.style_mut(sibling).opacity = SIBLING_OPACITY;
            (row, value, sibling)
        };
        // `if (item.sibling)`: o texto vazio conta como sem irmão.
        let sibling_text = item.sibling().filter(|s| !s.is_empty()).map(|s| format!("when: {s}"));
        if sibling_text.is_none() {
            self.ui.tree_mut().add_class(value, "no-sibling");
        }
        self.row_layouts.register(&self.ui, row, RowLayout::Data { value_node: value, sibling_node: sibling, value_text: item.value().data().to_owned(), sibling_text });
        if self.options.drag_and_drop {
            self.add_drag_and_drop(row, item, idx);
        }
        RowElementGroup { row_element: row, key_element: value, value_element: Some(sibling) }
    }

    fn render_edit(&self, item: &T, idx: usize) -> NodeId {
        let ui = self.ui.clone();
        let row = create_detached_el(&mut ui.tree_mut(), Role::Generic, "setting-list-edit-row");
        let mut item = item.clone();

        let suggester = self.key_value_suggester.borrow().clone();
        if let Some(suggester) = suggester {
            let existing: Vec<String> = self.base.items().iter().map(|i| i.item.value().data().to_owned()).collect();
            let options = suggester(&existing, Some(idx)).map(|enum_data| enum_data.options).unwrap_or_default();
            let value = ObjectKey::Enum(ObjectEnumData { data: item.value().data().to_owned(), options });
            let sibling = item.sibling().map(str::to_owned);
            item = item.with_value(value, sibling);
        }

        let current_display_value: Rc<RefCell<Option<String>>> = Rc::default();
        let mut current_enum_options: Vec<ObjectEnumOption> = Vec::new();
        let value_input = match item.value().clone() {
            ObjectKey::String(data) => EditValue::Input(self.render_input_box(&data, row)),
            ObjectKey::Enum(enum_data) => {
                let (wrapper, select) = self.render_dropdown(&enum_data, row);
                current_enum_options = enum_data.options.clone();
                if !enum_data.options.is_empty() {
                    let shown = if self.is_item_new(&item) { enum_data.options[0].value.clone() } else { enum_data.data.clone() };
                    *current_display_value.borrow_mut() = Some(shown);
                }
                let display = current_display_value.clone();
                self.base.list_disposables().add(select.on_did_select().listen(move |data| *display.borrow_mut() = Some(data.selected.clone())));
                EditValue::Select { wrapper, select }
            }
        };

        let sibling_input: Rc<RefCell<Option<Rc<InputBox>>>> = Rc::default();
        let updated_input_box_item: Rc<dyn Fn() -> T> = {
            let (template, value_input, sibling_input) = (item.clone(), value_input.clone(), sibling_input.clone());
            Rc::new(move || {
                let value = match &value_input {
                    EditValue::Input(input) => input.value(),
                    EditValue::Select { .. } => String::new(),
                };
                let sibling = sibling_input.borrow().as_ref().map(|input| input.value());
                template.with_value(ObjectKey::String(value), sibling)
            })
        };
        let on_key_down: Rc<dyn Fn(&mut Event)> = {
            let (weak, item, updated) = (self.weak_self.clone(), item.clone(), updated_input_box_item.clone());
            Rc::new(move |e: &mut Event| {
                let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
                let Some(this) = weak.upgrade() else { return };
                if key.equals(KeyCode::Enter.bits()) {
                    this.handle_item_change(item.clone(), updated(), idx);
                } else if key.equals(KeyCode::Escape.bits()) {
                    this.cancel_edit();
                    e.stop(true);
                }
            })
        };
        if let EditValue::Input(input) = &value_input {
            let on_key_down = on_key_down.clone();
            self.base.list_disposables().add(ui.listen(input.input_element(), EventKind::KeyDown, false, move |e| on_key_down(e)));
        }

        if let Some(sibling) = item.sibling() {
            let mut options = InputBoxOptions::new(self.services.styles.input_box.clone());
            options.placeholder = Some(self.options.placeholders.sibling_input_placeholder.clone());
            let input = InputBox::new(&ui, row, Some(self.services.context_view_provider.clone()), options);
            ui.tree_mut().add_class(input.element(), "setting-list-siblingInput");
            self.base.list_disposables().add_rc(input.clone());
            input.set_value(sibling);
            let on_key_down = on_key_down.clone();
            self.base.list_disposables().add(ui.listen(input.input_element(), EventKind::KeyDown, false, move |e| on_key_down(e)));
            *sibling_input.borrow_mut() = Some(input);
        } else if let EditValue::Input(input) = &value_input {
            ui.tree_mut().add_class(input.element(), "no-sibling");
        }

        let ok = self.make_button(row, localize("okButton", "OK", &[]), "setting-list-ok-button", false);
        {
            let (weak, item, updated, display) = (self.weak_self.clone(), item.clone(), updated_input_box_item.clone(), current_display_value.clone());
            self.base.list_disposables().add(ok.on_did_click().listen(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let changed = match item.value() {
                    ObjectKey::String(_) => updated(),
                    ObjectKey::Enum(_) => {
                        let data = display.borrow().clone().unwrap_or_default();
                        item.with_value(ObjectKey::Enum(ObjectEnumData { data, options: current_enum_options.clone() }), None)
                    }
                };
                this.handle_item_change(item.clone(), changed, idx);
            }));
        }
        let cancel = self.make_button(row, localize("cancelButton", "Cancel", &[]), "setting-list-cancel-button", true);
        {
            let weak = self.weak_self.clone();
            self.base.list_disposables().add(cancel.on_did_click().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.cancel_edit();
                }
            }));
        }

        let focus_target = value_input.clone();
        self.base.list_disposables().add(disposable_timeout(
            move || match &focus_target {
                EditValue::Input(input) => {
                    input.focus();
                    input.select(None);
                }
                EditValue::Select { select, .. } => select.focus(),
            },
            0,
            None,
        ));

        let sibling = sibling_input.borrow().clone();
        self.row_layouts.register(&self.ui, row, RowLayout::Edit(EditRowLayout { value: value_input, sibling, ok, cancel }));
        row
    }

    fn add_tooltips_to_row(&self, row_element: &RowElementGroup, item: &T) {
        let tooltip = (self.options.tooltip)(item);
        let row = row_element.row_element;
        self.base.setup_row_hover(row, tooltip.hover.clone().unwrap_or_else(|| HoverContent::Text(tooltip.title.clone())));
        self.ui.tree_mut().set_label(row, tooltip.title);
    }

    fn layout_row(&self, row: NodeId, width: f64) -> f64 {
        match self.row_layouts.get(row) {
            Some(RowLayout::Data { value_node, sibling_node, value_text, sibling_text }) => self.layout_data_row(value_node, sibling_node, &value_text, sibling_text.as_deref(), width),
            Some(RowLayout::Edit(edit)) => self.layout_edit_row(&edit, width),
            None => ROW_HEIGHT,
        }
    }
}

impl<T: ListDataItem> IDisposable for ListSettingWidget<T> {
    fn dispose(&self) {
        self.own.dispose();
        self.base.dispose();
    }
}

// ---- Include e Exclude -----------------------------------------------------------------------------------------------

/// `ExcludeSettingWidget`: o [`ListSettingWidget`] de [`IIncludeExcludeDataItem`] com o que a subclasse muda
/// ([`ListSettingWidgetOptions`]: classes, textos, dica em Markdown, sem arrastar, fonte monoespaçada).
pub struct ExcludeSettingWidget;

impl ExcludeSettingWidget {
    pub fn new(services: SettingsWidgetServices, container: NodeId) -> Rc<ListSettingWidget<IIncludeExcludeDataItem>> {
        ListSettingWidget::new(services, container, ListSettingWidgetOptions::include_exclude(IncludeExclude::Exclude))
    }
}

/// `IncludeSettingWidget`: igual ao [`ExcludeSettingWidget`], com os textos de inclusão.
pub struct IncludeSettingWidget;

impl IncludeSettingWidget {
    pub fn new(services: SettingsWidgetServices, container: NodeId) -> Rc<ListSettingWidget<IIncludeExcludeDataItem>> {
        ListSettingWidget::new(services, container, ListSettingWidgetOptions::include_exclude(IncludeExclude::Include))
    }
}

// ---- peças dos widgets concretos -------------------------------------------------------------------------------------

/// Como cada widget guarda o que a linha montada precisa para ser posicionada: por nó da linha.
struct RowLayouts<V>(RefCell<HashMap<NodeId, V>>);

impl<V> Default for RowLayouts<V> {
    fn default() -> Self {
        Self(RefCell::new(HashMap::new()))
    }
}

impl<V: Clone> RowLayouts<V> {
    /// Esquece as posições de linhas que já saíram da árvore e guarda a desta.
    fn register(&self, ui: &Ui, row: NodeId, layout: V) {
        let tree = ui.tree();
        let mut layouts = self.0.borrow_mut();
        layouts.retain(|id, _| tree.is_alive(*id));
        layouts.insert(row, layout);
    }

    fn get(&self, row: NodeId) -> Option<V> {
        self.0.borrow().get(&row).cloned()
    }
}

/// `rgba(130, 130, 130, 0.04)`: a zebra das linhas de objeto e o fundo do cabeçalho.
fn zebra_background() -> Rgba {
    Rgba::new(130, 130, 130, ZEBRA_ALPHA)
}

/// A largura intrínseca do `<input>` (vinte vezes a largura de `0`, mais padding e borda), até `max_width`.
fn input_intrinsic_width(max_width: f64) -> f64 {
    let font = ui_font(UI_FONT_SIZE, 400);
    (measure_text(&"0".repeat(INPUT_INTRINSIC_CHARS), &font) + INPUT_CHROME_WIDTH).min(max_width)
}

/// Uma ação de linha (`{ class: ThemeIcon.asClassName(icon), enabled: true, id, label: '', tooltip, run }`).
fn list_action(id: &str, tooltip: &str, icon: &str, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: String::new(),
        tooltip: Some(tooltip.to_owned()),
        enabled: Some(true),
        checked: None,
        class: Some(ThemeIcon::from_id(icon).as_class_name()),
        alt: None,
        run: Rc::new(move |_args: &[Dynamic]| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

/// O laço de encolher do CSS Flexbox para itens com `flex-shrink: 1`: `items` são o tamanho base e o `min-width` de cada
/// um, `available` é o espaço da linha sem os itens rígidos. Itens cujo base é menor que o mínimo congelam no mínimo; os
/// demais encolhem na proporção do tamanho base, e quem passar a ficar abaixo do mínimo congela e o laço repete.
fn resolve_flex_shrink(available: f64, items: &[(f64, f64)]) -> Vec<f64> {
    let mut sizes: Vec<f64> = items.iter().map(|&(base, min)| base.max(min)).collect();
    if sizes.iter().sum::<f64>() <= available {
        return sizes;
    }
    let mut frozen: Vec<bool> = items.iter().map(|&(base, min)| base < min).collect();
    while frozen.iter().any(|is_frozen| !is_frozen) {
        let frozen_total: f64 = (0..items.len()).filter(|&i| frozen[i]).map(|i| sizes[i]).sum();
        let unfrozen: Vec<usize> = (0..items.len()).filter(|&i| !frozen[i]).collect();
        let bases_total: f64 = unfrozen.iter().map(|&i| items[i].0).sum();
        let free = (available - frozen_total - bases_total).min(0.0);
        let targets: Vec<f64> = unfrozen.iter().map(|&i| if bases_total > 0.0 { items[i].0 + free * items[i].0 / bases_total } else { items[i].0 }).collect();
        let total_violation: f64 = unfrozen.iter().zip(&targets).map(|(&i, &target)| target.max(items[i].1) - target).sum();
        for (&i, &target) in unfrozen.iter().zip(&targets) {
            let clamped = target.max(items[i].1);
            if total_violation == 0.0 || clamped > target {
                sizes[i] = clamped;
                frozen[i] = true;
            }
        }
    }
    sizes
}

// ---- widgets de objeto -----------------------------------------------------------------------------------------------

/// `IObjectSetValueOptions`.
pub struct ObjectSetValueOptions {
    pub setting_key: String,
    pub show_add_button: bool,
    pub is_read_only: Option<bool>,
    pub key_suggester: Option<ObjectKeySuggester>,
    pub value_suggester: Option<ObjectValueSuggester>,
    /// `propertyNames?: IJSONSchema`, o esquema dinâmico que `validate_property_name` lê.
    pub property_names: Option<Dynamic>,
}

/// `IBoolObjectSetValueOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoolObjectSetValueOptions {
    pub setting_key: String,
}

/// `ObjectWidget`: `InputBox | SelectBox`.
#[derive(Clone)]
enum ObjectWidgetKind {
    Input(Rc<InputBox>),
    Select(Rc<SelectBox>),
}

/// O que `renderEditWidget` devolve: o `widget` e o `element` (o contêiner `.setting-list-object-input-*`).
#[derive(Clone)]
struct ObjectEditWidget {
    wrapper: NodeId,
    kind: ObjectWidgetKind,
}

impl ObjectEditWidget {
    /// `widget.focus(); if (widget instanceof InputBox) widget.select();`.
    fn focus(&self) {
        match &self.kind {
            ObjectWidgetKind::Input(input) => {
                input.focus();
                input.select(None);
            }
            ObjectWidgetKind::Select(select) => select.focus(),
        }
    }
}

/// A chave da linha de edição: o campo ou a lista (com `showAddButton`) ou só o texto.
#[derive(Clone)]
enum ObjectEditKey {
    Widget(ObjectEditWidget),
    Text { node: NodeId, text: String },
}

/// O que as linhas dos widgets de objeto guardam para ser posicionadas.
#[derive(Clone)]
enum ObjectRowLayout {
    /// A linha de dados e o cabeçalho (`.setting-list-row-header`): chave e valor em texto.
    Text { key_node: NodeId, value_node: NodeId, key_text: String, value_text: String, header: bool },
    /// A linha de edição do `ObjectSettingDropdownWidget`. `value` é o `valueWidget` mais recente.
    Edit { key: ObjectEditKey, value_container: NodeId, value: Rc<RefCell<Option<ObjectEditWidget>>>, ok: Rc<Button>, cancel: Rc<Button> },
    /// A linha do `ObjectSettingCheckboxWidget`.
    Checkbox { wrapper: NodeId, checkbox: Rc<Checkbox>, value_node: NodeId, text: String },
}

/// Posiciona uma linha (ou o cabeçalho) dos widgets de objeto com a largura dada e devolve a altura.
fn layout_object_row<T: Clone + 'static>(base: &AbstractListSettingWidget<T>, layouts: &RowLayouts<ObjectRowLayout>, row: NodeId, width: f64) -> f64 {
    let Some(layout) = layouts.get(row) else { return ROW_HEIGHT };
    match layout {
        ObjectRowLayout::Text { key_node, value_node, key_text, value_text, header } => {
            // `padding-right: 4px`; os percentuais são da largura do conteúdo. `flex: none`: nada encolhe.
            let content_width = (width - OBJECT_ROW_PADDING_RIGHT).max(0.0);
            let font = ui_font(UI_FONT_SIZE, if header { 700 } else { 400 });
            let key_width = OBJECT_KEY_WIDTH * content_width;
            base.place_text(key_node, &key_text, &font, OBJECT_KEY_MARGIN_LEFT, key_width, 0.0);
            let value_x = OBJECT_KEY_MARGIN_LEFT + key_width + OBJECT_KEY_MARGIN_RIGHT;
            base.place_text(value_node, &value_text, &font, value_x, OBJECT_VALUE_WIDTH * content_width, if header { 0.0 } else { OBJECT_VALUE_PADDING_RIGHT });
            ROW_HEIGHT
        }
        ObjectRowLayout::Edit { key, value_container, value, ok, cancel } => {
            let value = value.borrow().clone();
            layout_object_edit_row(base, &key, value_container, value, &ok, &cancel, width)
        }
        ObjectRowLayout::Checkbox { wrapper, checkbox, value_node, text } => {
            let wrapper_width = CHECKBOX_SIZE + CHECKBOX_MARGIN_RIGHT;
            base.place_box(wrapper, CHECKBOX_WRAPPER_MARGIN_LEFT, wrapper_width, ROW_HEIGHT);
            checkbox.set_position(0.0, CHECKBOX_MARGIN_TOP);
            // `.setting-list-object-input { margin-right: 4px }`; o valor ocupa o resto (`flex: 1`).
            let value_x = CHECKBOX_WRAPPER_MARGIN_LEFT + wrapper_width + INPUT_MARGIN_RIGHT;
            base.place_text(value_node, &text, &ui_font(UI_FONT_SIZE, 400), value_x, (width - value_x).max(0.0), 0.0);
            ROW_HEIGHT
        }
    }
}

/// O flex da linha de edição do `ObjectSettingDropdownWidget`: a chave, o contêiner do valor e os botões OK
/// (`margin: 0 4px`) e Cancelar. Ver o `//!` do módulo.
fn layout_object_edit_row<T: Clone + 'static>(
    base: &AbstractListSettingWidget<T>,
    key: &ObjectEditKey,
    value_container: NodeId,
    value: Option<ObjectEditWidget>,
    ok: &Rc<Button>,
    cancel: &Rc<Button>,
    width: f64,
) -> f64 {
    let buttons = OBJECT_BUTTONS_MARGIN + ok.min_content_width() + cancel.min_content_width();
    let key_min = OBJECT_KEY_MIN_WIDTH * width;
    // O item flexível da chave (tamanho base e mínimo) e o que ela tem de rígido (as margens fora do `min-width`).
    let (key_flexible, key_rigid) = match key {
        ObjectEditKey::Text { .. } => (None, OBJECT_KEY_MARGIN_LEFT + OBJECT_KEY_WIDTH * width + OBJECT_KEY_MARGIN_RIGHT),
        ObjectEditKey::Widget(ObjectEditWidget { kind: ObjectWidgetKind::Input(_), .. }) => (Some((input_intrinsic_width(f64::INFINITY) + INPUT_MARGIN_RIGHT, key_min)), 0.0),
        ObjectEditKey::Widget(ObjectEditWidget { kind: ObjectWidgetKind::Select(select), .. }) => (Some((select.natural_size().0, key_min)), INPUT_MARGIN_RIGHT),
    };
    let mut items: Vec<(f64, f64)> = key_flexible.into_iter().collect();
    // O contêiner do valor: `width: 100%`, e o mínimo de um item flex cujo conteúdo tem largura em percentual é zero.
    items.push((width, 0.0));
    let sizes = resolve_flex_shrink(width - key_rigid - buttons, &items);
    let value_width = sizes[sizes.len() - 1];

    let mut x = match key {
        ObjectEditKey::Text { node, text } => {
            let key_width = OBJECT_KEY_WIDTH * width;
            base.place_text(*node, text, &ui_font(UI_FONT_SIZE, 400), OBJECT_KEY_MARGIN_LEFT, key_width, 0.0);
            OBJECT_KEY_MARGIN_LEFT + key_width + OBJECT_KEY_MARGIN_RIGHT
        }
        ObjectEditKey::Widget(widget) => {
            let key_width = sizes[0];
            base.place_box(widget.wrapper, 0.0, key_width, ROW_HEIGHT);
            match &widget.kind {
                // A margem de `4px` do `.setting-list-object-input` é do `InputBox` e fica dentro do contêiner da chave.
                ObjectWidgetKind::Input(input) => {
                    base.place_input(input, 0.0, (key_width - INPUT_MARGIN_RIGHT).max(0.0));
                    key_width
                }
                // Na lista a margem é do próprio contêiner, fora do `min-width`.
                ObjectWidgetKind::Select(select) => {
                    select.layout();
                    key_width + INPUT_MARGIN_RIGHT
                }
            }
        }
    };

    base.place_box(value_container, x, value_width, ROW_HEIGHT);
    if let Some(widget) = value {
        base.place_box(widget.wrapper, 0.0, value_width, ROW_HEIGHT);
        match &widget.kind {
            ObjectWidgetKind::Input(input) => {
                base.place_input(input, 0.0, value_width);
            }
            ObjectWidgetKind::Select(select) => {
                select.layout();
            }
        }
    }
    x += value_width;
    base.place_edit_buttons(ok, cancel, x, ROW_HEIGHT)
}

/// `shouldUseSuggestion(originalValue, previousValue, newValue)`.
fn should_use_suggestion(original_value: &ObjectValue, previous_value: &ObjectValue, new_value: &ObjectValue) -> bool {
    let same_type = std::mem::discriminant(previous_value) == std::mem::discriminant(new_value);
    let new_is_enum = matches!(new_value, ObjectValue::Enum(_));
    // suggestion is exactly the same
    if !new_is_enum && same_type && new_value.data_string() == previous_value.data_string() {
        return false;
    }
    // item is new, use suggestion
    if matches!(original_value, ObjectValue::String(data) | ObjectValue::Enum(ObjectEnumData { data, .. }) if data.is_empty()) {
        return true;
    }
    if same_type && !new_is_enum {
        return false;
    }
    // check if all enum options are the same
    if let (ObjectValue::Enum(previous), ObjectValue::Enum(new)) = (previous_value, new_value) {
        let mut previous_enums: std::collections::HashSet<&str> = previous.options.iter().map(|option| option.value.as_str()).collect();
        for option in &new.options {
            previous_enums.remove(option.value.as_str());
        }
        // all options are the same
        if previous_enums.is_empty() {
            return false;
        }
    }
    true
}

/// O `{ ...changedKeyOrValue, data: selected }` do `onDidSelect` (e do "inválido no json"): o booleano vira `selected ===
/// 'true'`, a lista mantém as opções do molde.
fn with_enum_selection(template: &ObjectValue, selected: &str) -> ObjectValue {
    match template {
        ObjectValue::Boolean(_) => ObjectValue::Boolean(selected == "true"),
        ObjectValue::Enum(enum_data) => ObjectValue::Enum(ObjectEnumData { data: selected.to_owned(), options: enum_data.options.clone() }),
        ObjectValue::String(_) => ObjectValue::String(selected.to_owned()),
    }
}

/// `getEnumDescription`: a descrição da opção escolhida.
fn enum_description(enum_data: &ObjectEnumData) -> Option<String> {
    enum_data.options.iter().find(|option| option.value == enum_data.data).and_then(|option| option.description.clone())
}

/// `IObjectRenderEditWidgetOptions`.
struct ObjectEditContext {
    is_key: bool,
    idx: usize,
    original_item: IObjectDataItem,
    changed_item: Rc<RefCell<IObjectDataItem>>,
    update: Rc<dyn Fn(ObjectValue)>,
}

/// `ObjectSettingDropdownWidget`.
pub struct ObjectSettingDropdownWidget {
    weak_self: Weak<Self>,
    ui: Ui,
    base: Rc<AbstractListSettingWidget<IObjectDataItem>>,
    editable: Cell<bool>,
    current_setting_key: RefCell<String>,
    show_add_button: Cell<bool>,
    key_suggester: RefCell<ObjectKeySuggester>,
    value_suggester: RefCell<ObjectValueSuggester>,
    property_names: RefCell<Option<Dynamic>>,
    row_layouts: RowLayouts<ObjectRowLayout>,
}

impl Deref for ObjectSettingDropdownWidget {
    type Target = AbstractListSettingWidget<IObjectDataItem>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ObjectSettingDropdownWidget {
    pub fn new(services: SettingsWidgetServices, container: NodeId) -> Rc<Self> {
        let ui = services.ui.clone();
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let delegate: Weak<dyn ListSettingDelegate<IObjectDataItem>> = weak.clone();
            let empty_item = IObjectDataItem {
                key: ObjectKey::String(String::new()),
                value: ObjectValue::String(String::new()),
                key_description: None,
                source: None,
                removable: true,
                resetable: false,
            };
            Self {
                base: AbstractListSettingWidget::new(services, container, empty_item, delegate),
                weak_self: weak.clone(),
                ui,
                editable: Cell::new(true),
                current_setting_key: RefCell::new(String::new()),
                show_add_button: Cell::new(true),
                key_suggester: RefCell::new(Rc::new(|_: &[String], _: Option<usize>| None)),
                value_suggester: RefCell::new(Rc::new(|_: &str| None)),
                property_names: RefCell::new(None),
                row_layouts: RowLayouts::default(),
            }
        });
        this.base.init();
        this
    }

    /// `get isReadOnly()`.
    fn is_read_only_now(&self) -> bool {
        !self.editable.get()
    }

    /// `setValue(listData, options?)`.
    pub fn set_value(&self, list_data: Vec<IObjectDataItem>, options: Option<ObjectSetValueOptions>) {
        self.editable.set(!options.as_ref().and_then(|options| options.is_read_only).unwrap_or(false));
        match options {
            Some(options) => {
                self.show_add_button.set(options.show_add_button);
                if let Some(suggester) = options.key_suggester {
                    *self.key_suggester.borrow_mut() = suggester;
                }
                if let Some(suggester) = options.value_suggester {
                    *self.value_suggester.borrow_mut() = suggester;
                }
                *self.property_names.borrow_mut() = options.property_names;
                if options.setting_key != *self.current_setting_key.borrow() {
                    self.base.with_model(|model| {
                        model.set_edit_key(EditKey::None);
                        model.select(None);
                    });
                    *self.current_setting_key.borrow_mut() = options.setting_key;
                }
            }
            None => *self.property_names.borrow_mut() = None,
        }
        self.base.set_value(list_data);
    }

    // ---- linha de edição ---------------------------------------------------------------------------------------------

    fn render_edit_widget(&self, key_or_value: ObjectValue, context: &ObjectEditContext) -> ObjectEditWidget {
        match key_or_value {
            ObjectValue::String(data) => self.render_string_edit_widget(&data, context),
            ObjectValue::Enum(enum_data) => self.render_enum_edit_widget(&enum_data, context),
            ObjectValue::Boolean(data) => {
                let options = ["true", "false"].map(|value| ObjectEnumOption { value: value.to_owned(), description: None }).to_vec();
                self.render_enum_edit_widget(&ObjectEnumData { data: data.to_string(), options }, context)
            }
        }
    }

    fn render_string_edit_widget(&self, data: &str, context: &ObjectEditContext) -> ObjectEditWidget {
        let services = self.base.services();
        let wrapper = create_detached_el(&mut self.ui.tree_mut(), Role::Generic, if context.is_key { "setting-list-object-input-key" } else { "setting-list-object-input-value" });
        let mut options = InputBoxOptions::new(services.styles.input_box.clone());
        options.placeholder = Some(if context.is_key { localize("objectKeyInputPlaceholder", "Key", &[]) } else { localize("objectValueInputPlaceholder", "Value", &[]) });
        let input = InputBox::new(&self.ui, wrapper, Some(services.context_view_provider.clone()), options);
        self.ui.tree_mut().add_class(input.element(), "setting-list-object-input");
        self.base.list_disposables().add_rc(input.clone());
        input.set_value(data);

        let update = context.update.clone();
        self.base.list_disposables().add(input.on_did_change().listen(move |value| update(ObjectValue::String(value.clone()))));

        let (weak, original_item, changed_item, idx) = (self.weak_self.clone(), context.original_item.clone(), context.changed_item.clone(), context.idx);
        self.base.list_disposables().add(self.ui.listen(input.input_element(), EventKind::KeyDown, false, move |e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            let Some(this) = weak.upgrade() else { return };
            if key.equals(KeyCode::Enter.bits()) {
                let changed = changed_item.borrow().clone();
                this.handle_item_change(original_item.clone(), changed, idx);
            } else if key.equals(KeyCode::Escape.bits()) {
                this.cancel_edit();
                e.stop(true);
            }
        }));
        ObjectEditWidget { wrapper, kind: ObjectWidgetKind::Input(input) }
    }

    fn render_enum_edit_widget(&self, key_or_value: &ObjectEnumData, context: &ObjectEditContext) -> ObjectEditWidget {
        let select = self.base.create_basic_select_box(key_or_value);
        self.base.list_disposables().add_rc(select.clone());

        // `changedKeyOrValue`: o objeto que o `onDidSelect` espalha, o do momento em que o widget foi montado.
        let template = if context.is_key { ObjectValue::from(context.changed_item.borrow().key.clone()) } else { context.changed_item.borrow().value.clone() };
        {
            let (update, template) = (context.update.clone(), template.clone());
            self.base.list_disposables().add(select.on_did_select().listen(move |data| update(with_enum_selection(&template, &data.selected))));
        }

        let classes = if context.is_key { "setting-list-object-input setting-list-object-input-key" } else { "setting-list-object-input setting-list-object-input-value" };
        let wrapper = create_detached_el(&mut self.ui.tree_mut(), Role::Generic, classes);
        select.render(wrapper);

        // Switch to the first item if the user set something invalid in the json
        let selected = key_or_value.options.iter().position(|option| key_or_value.data == option.value);
        if selected.is_none() && let Some(first) = key_or_value.options.first() {
            let value = match &template {
                ObjectValue::Boolean(_) => ObjectValue::Boolean(true),
                other => with_enum_selection(other, &first.value),
            };
            (context.update)(value);
        } else if matches!(template, ObjectValue::Boolean(_)) {
            // https://github.com/microsoft/vscode/issues/129581
            (context.update)(ObjectValue::Boolean(key_or_value.data == "true"));
        }
        ObjectEditWidget { wrapper, kind: ObjectWidgetKind::Select(select) }
    }
}

impl ListSettingDelegate<IObjectDataItem> for ObjectSettingDropdownWidget {
    fn is_item_new(&self, item: &IObjectDataItem) -> bool {
        item.key.data().is_empty() && item.value.data_string().is_empty()
    }

    fn get_container_classes(&self) -> Vec<String> {
        vec![OBJECT_WIDGET_CLASS.to_owned()]
    }

    fn is_add_button_visible(&self) -> bool {
        self.show_add_button.get()
    }

    fn is_read_only(&self) -> bool {
        self.is_read_only_now()
    }

    fn get_localized_strings(&self) -> ListSettingStrings {
        object_strings().list
    }

    fn get_actions_for_item(&self, item: &IObjectDataItem, idx: usize) -> Vec<ActionRef> {
        if self.is_read_only_now() {
            return Vec::new();
        }
        let strings = object_strings();
        // Uma ação que dispara o evento da lista para o item.
        let fire = |event: fn(IObjectDataItem, usize) -> SettingListEvent<IObjectDataItem>| {
            let (weak, item) = (self.weak_self.clone(), item.clone());
            move || {
                if let Some(this) = weak.upgrade() {
                    this.fire_list_event(event(item.clone(), idx));
                }
            }
        };
        let weak = self.weak_self.clone();
        let mut actions = vec![list_action("workbench.action.editListItem", &strings.list.edit_action_tooltip, SETTINGS_EDIT_ICON, move || {
            if let Some(this) = weak.upgrade() {
                this.edit_setting(idx);
            }
        })];
        if item.resetable {
            actions.push(list_action(
                "workbench.action.resetListItem",
                &strings.reset_action_tooltip,
                SETTINGS_DISCARD_ICON,
                fire(|original_item, target_index| SettingListEvent::Reset { original_item, target_index }),
            ));
        }
        if item.removable {
            actions.push(list_action(
                "workbench.action.removeListItem",
                &strings.list.delete_action_tooltip,
                SETTINGS_REMOVE_ICON,
                fire(|original_item, target_index| SettingListEvent::Remove { original_item, target_index }),
            ));
        }
        actions
    }

    fn render_header(&self) -> Option<NodeId> {
        let strings = object_strings();
        let (header, key, value) = {
            let mut tree = self.ui.tree_mut();
            let header = create_detached_el(&mut tree, Role::Generic, "setting-list-row-header");
            let key = create_el(&mut tree, header, Role::Generic, "setting-list-object-key");
            let value = create_el(&mut tree, header, Role::Generic, "setting-list-object-value");
            tree.style_mut(header).background = Some(zebra_background());
            (header, key, value)
        };
        self.row_layouts.register(&self.ui, header, ObjectRowLayout::Text { key_node: key, value_node: value, key_text: strings.key_header_text, value_text: strings.value_header_text, header: true });
        Some(header)
    }

    fn layout_header(&self, header: NodeId, width: f64) -> f64 {
        layout_object_row(&self.base, &self.row_layouts, header, width)
    }

    fn render_item(&self, item: &IObjectDataItem, _idx: usize) -> RowElementGroup {
        // Mark row as invalid if the key doesn't match propertyNames.pattern
        let invalid_key = self.property_names.borrow().as_ref().is_some_and(|names| !item.key.data().is_empty() && !validate_property_name(names, &JsString::from(item.key.data())));
        let (row, key, value) = {
            let mut tree = self.ui.tree_mut();
            let row = create_detached_el(&mut tree, Role::Generic, "setting-list-row setting-list-object-row");
            if invalid_key {
                tree.add_class(row, "invalid-key");
            }
            let key = create_el(&mut tree, row, Role::Generic, "setting-list-object-key");
            let value = create_el(&mut tree, row, Role::Generic, "setting-list-object-value");
            (row, key, value)
        };
        let layout = ObjectRowLayout::Text { key_node: key, value_node: value, key_text: item.key.data().to_owned(), value_text: item.value.data_string(), header: false };
        self.row_layouts.register(&self.ui, row, layout);
        RowElementGroup { row_element: row, key_element: key, value_element: Some(value) }
    }

    fn render_edit(&self, item: &IObjectDataItem, idx: usize) -> NodeId {
        let ui = self.ui.clone();
        let (row, value_container) = {
            let mut tree = ui.tree_mut();
            (
                create_detached_el(&mut tree, Role::Generic, "setting-list-edit-row setting-list-object-row"),
                create_detached_el(&mut tree, Role::Generic, "setting-list-object-value-container"),
            )
        };
        let changed = Rc::new(RefCell::new(item.clone()));
        let ok_button: Rc<RefCell<Option<Rc<Button>>>> = Rc::default();
        let value_widget: Rc<RefCell<Option<ObjectEditWidget>>> = Rc::default();
        // Verdadeiro até o fim desta função: uma troca de chave durante a montagem não reposiciona a lista.
        let rendering = Rc::new(Cell::new(true));

        let on_value_change: Rc<dyn Fn(ObjectValue)> = {
            let changed = changed.clone();
            Rc::new(move |value: ObjectValue| changed.borrow_mut().value = value)
        };

        let render_latest_value: Rc<dyn Fn()> = {
            let (weak, changed, item, update, value_widget) = (self.weak_self.clone(), changed.clone(), item.clone(), on_value_change.clone(), value_widget.clone());
            Rc::new(move || {
                let Some(this) = weak.upgrade() else { return };
                let current = changed.borrow().value.clone();
                let context = ObjectEditContext { is_key: false, idx, original_item: item.clone(), changed_item: changed.clone(), update: update.clone() };
                let widget = this.render_edit_widget(current, &context);
                let old_children = this.ui.tree().children(value_container).to_vec();
                for child in old_children {
                    this.ui.remove(child);
                }
                this.ui.tree_mut().append_child(value_container, widget.wrapper);
                *value_widget.borrow_mut() = Some(widget);
            })
        };

        let on_key_change: Rc<dyn Fn(ObjectKey)> = {
            let (weak, changed, item, ok_button, on_value_change, render_latest_value, rendering) =
                (self.weak_self.clone(), changed.clone(), item.clone(), ok_button.clone(), on_value_change.clone(), render_latest_value.clone(), rendering.clone());
            Rc::new(move |key: ObjectKey| {
                let Some(this) = weak.upgrade() else { return };
                let key_is_empty = key.data().is_empty();
                let suggester = this.value_suggester.borrow().clone();
                let suggested_value = suggester(key.data()).unwrap_or_else(|| item.value.clone());
                changed.borrow_mut().key = key;
                if let Some(ok) = ok_button.borrow().as_ref() {
                    ok.set_enabled(!key_is_empty);
                }
                let previous_value = changed.borrow().value.clone();
                if should_use_suggestion(&item.value, &previous_value, &suggested_value) {
                    on_value_change(suggested_value);
                    render_latest_value();
                    if !rendering.get() {
                        this.base.layout();
                    }
                }
            })
        };

        let (key_widget, key_element) = if self.show_add_button.get() {
            if self.is_item_new(item) {
                let existing: Vec<String> = self.base.items().iter().map(|existing| existing.item.key.data().to_owned()).collect();
                let key_suggester = self.key_suggester.borrow().clone();
                if let Some(suggested_key) = key_suggester(&existing, None) {
                    let value_suggester = self.value_suggester.borrow().clone();
                    let suggested_value = value_suggester(&suggested_key.data);
                    changed.borrow_mut().key = ObjectKey::Enum(suggested_key);
                    let value = suggested_value.unwrap_or_else(|| changed.borrow().value.clone());
                    on_value_change(value);
                }
            }
            let key_update: Rc<dyn Fn(ObjectValue)> = {
                let on_key_change = on_key_change.clone();
                Rc::new(move |value: ObjectValue| {
                    if let Some(key) = value.into_key() {
                        on_key_change(key);
                    }
                })
            };
            let current_key = ObjectValue::from(changed.borrow().key.clone());
            let context = ObjectEditContext { is_key: true, idx, original_item: item.clone(), changed_item: changed.clone(), update: key_update };
            let widget = self.render_edit_widget(current_key, &context);
            (Some(widget.clone()), ObjectEditKey::Widget(widget))
        } else {
            let node = create_detached_el(&mut ui.tree_mut(), Role::Generic, "setting-list-object-key");
            (None, ObjectEditKey::Text { node, text: item.key.data().to_owned() })
        };

        render_latest_value();
        {
            let key_node = match &key_element {
                ObjectEditKey::Widget(widget) => widget.wrapper,
                ObjectEditKey::Text { node, .. } => *node,
            };
            let mut tree = ui.tree_mut();
            tree.append_child(row, key_node);
            tree.append_child(row, value_container);
        }

        let ok = self.base.make_button(row, localize("okButton", "OK", &[]), "setting-list-ok-button", false);
        ok.set_enabled(!changed.borrow().key.data().is_empty());
        *ok_button.borrow_mut() = Some(ok.clone());
        {
            let (weak, item, changed) = (self.weak_self.clone(), item.clone(), changed.clone());
            self.base.list_disposables().add(ok.on_did_click().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    let changed_item = changed.borrow().clone();
                    this.handle_item_change(item.clone(), changed_item, idx);
                }
            }));
        }

        let cancel = self.base.make_button(row, localize("cancelButton", "Cancel", &[]), "setting-list-cancel-button", true);
        {
            let weak = self.weak_self.clone();
            self.base.list_disposables().add(cancel.on_did_click().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.cancel_edit();
                }
            }));
        }

        {
            let (key_widget, value_widget) = (key_widget, value_widget.clone());
            self.base.list_disposables().add(disposable_timeout(
                move || {
                    let widget = key_widget.clone().or_else(|| value_widget.borrow().clone());
                    if let Some(widget) = widget {
                        widget.focus();
                    }
                },
                0,
                None,
            ));
        }

        rendering.set(false);
        self.row_layouts.register(&ui, row, ObjectRowLayout::Edit { key: key_element, value_container, value: value_widget, ok, cancel });
        row
    }

    fn add_tooltips_to_row(&self, row_element: &RowElementGroup, item: &IObjectDataItem) {
        let value_text = item.value.data_string();
        let key_text = item.key.data();
        let accessible_description = match item.source.as_deref().filter(|source| !source.is_empty()) {
            Some(source) => localize("objectPairHintLabelWithSource", "The property `{0}` is set to `{1}` by `{2}`.", &[key_text, &value_text, source]),
            None => localize("objectPairHintLabel", "The property `{0}` is set to `{1}`.", &[key_text, &value_text]),
        };
        let markdown = markdown_hover(&accessible_description);

        let key_enum_description = match &item.key {
            ObjectKey::Enum(enum_data) => enum_description(enum_data),
            ObjectKey::String(_) => None,
        };
        let key_description = key_enum_description.or_else(|| item.key_description.clone()).map_or_else(|| markdown.clone(), HoverContent::Text);
        self.base.setup_row_hover(row_element.key_element, key_description);

        let value_enum_description = match &item.value {
            ObjectValue::Enum(enum_data) => enum_description(enum_data),
            ObjectValue::String(_) | ObjectValue::Boolean(_) => None,
        };
        if let Some(value_element) = row_element.value_element {
            self.base.setup_row_hover(value_element, value_enum_description.map_or(markdown, HoverContent::Text));
        }
        self.ui.tree_mut().set_label(row_element.row_element, accessible_description);
    }

    fn layout_row(&self, row: NodeId, width: f64) -> f64 {
        layout_object_row(&self.base, &self.row_layouts, row, width)
    }
}

impl IDisposable for ObjectSettingDropdownWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---- ObjectSettingCheckboxWidget -------------------------------------------------------------------------------------

/// `ObjectSettingCheckboxWidget`.
pub struct ObjectSettingCheckboxWidget {
    weak_self: Weak<Self>,
    ui: Ui,
    base: Rc<AbstractListSettingWidget<IBoolObjectDataItem>>,
    current_setting_key: RefCell<String>,
    row_layouts: RowLayouts<ObjectRowLayout>,
}

impl Deref for ObjectSettingCheckboxWidget {
    type Target = AbstractListSettingWidget<IBoolObjectDataItem>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ObjectSettingCheckboxWidget {
    pub fn new(services: SettingsWidgetServices, container: NodeId) -> Rc<Self> {
        let ui = services.ui.clone();
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let delegate: Weak<dyn ListSettingDelegate<IBoolObjectDataItem>> = weak.clone();
            let empty_item = IBoolObjectDataItem { key: String::new(), value: false, key_description: None, source: None, resetable: true };
            Self {
                base: AbstractListSettingWidget::new(services, container, empty_item, delegate),
                weak_self: weak.clone(),
                ui,
                current_setting_key: RefCell::new(String::new()),
                row_layouts: RowLayouts::default(),
            }
        });
        this.base.init();
        this
    }

    /// `setValue(listData, options?)`.
    pub fn set_value(&self, list_data: Vec<IBoolObjectDataItem>, options: Option<BoolObjectSetValueOptions>) {
        if let Some(options) = options
            && options.setting_key != *self.current_setting_key.borrow()
        {
            self.base.with_model(|model| {
                model.set_edit_key(EditKey::None);
                model.select(None);
            });
            *self.current_setting_key.borrow_mut() = options.setting_key;
        }
        self.base.set_value(list_data);
    }
}

impl ListSettingDelegate<IBoolObjectDataItem> for ObjectSettingCheckboxWidget {
    fn is_item_new(&self, item: &IBoolObjectDataItem) -> bool {
        item.key.is_empty() && !item.value
    }

    fn get_container_classes(&self) -> Vec<String> {
        vec![OBJECT_WIDGET_CLASS.to_owned()]
    }

    fn get_actions_for_item(&self, _item: &IBoolObjectDataItem, _idx: usize) -> Vec<ActionRef> {
        Vec::new()
    }

    fn is_add_button_visible(&self) -> bool {
        false
    }

    fn force_edit_rows(&self) -> bool {
        true
    }

    fn get_localized_strings(&self) -> ListSettingStrings {
        object_strings().list
    }

    fn render_item(&self, _item: &IBoolObjectDataItem, _idx: usize) -> RowElementGroup {
        // Return just the containers, since we always render in edit mode anyway
        let mut tree = self.ui.tree_mut();
        let row_element = create_detached_el(&mut tree, Role::Generic, "blank-row");
        let key_element = create_detached_el(&mut tree, Role::Generic, "blank-row-key");
        RowElementGroup { row_element, key_element, value_element: None }
    }

    fn render_edit(&self, item: &IBoolObjectDataItem, idx: usize) -> NodeId {
        let row = create_detached_el(&mut self.ui.tree_mut(), Role::Generic, "setting-list-edit-row setting-list-object-row setting-item-bool");

        let on_value_change: Rc<dyn Fn(bool)> = {
            let (weak, item) = (self.weak_self.clone(), item.clone());
            Rc::new(move |new_value: bool| {
                let Some(this) = weak.upgrade() else { return };
                // `changedItem.value.data = newValue` escreve no objeto que o modelo e o `originalItem` compartilham.
                let changed = IBoolObjectDataItem { value: new_value, ..item.clone() };
                this.base.with_model(|model| model.set_data_item(idx, changed.clone()));
                this.handle_item_change(changed.clone(), changed, idx);
            })
        };

        let checkbox_description = match item.key_description.as_deref().filter(|description| !description.is_empty()) {
            Some(description) => format!("{description} ({})", item.key),
            None => item.key.clone(),
        };
        let checkbox = Checkbox::new(&self.ui, &checkbox_description, item.value, self.base.services().styles.checkbox.clone());
        self.base.list_disposables().add_rc(checkbox.clone());
        let (checkbox_wrapper, value_element) = {
            let mut tree = self.ui.tree_mut();
            let wrapper = create_detached_el(&mut tree, Role::Generic, "setting-list-object-input setting-list-object-input-key-checkbox");
            tree.add_class(checkbox.dom_node(), "setting-value-checkbox");
            tree.append_child(wrapper, checkbox.dom_node());
            tree.append_child(row, wrapper);
            let value_element = create_el(&mut tree, row, Role::Generic, "setting-list-object-value");
            tree.style_mut(value_element).cursor = Some(Cursor::Pointer);
            (wrapper, value_element)
        };

        // Without `stopPropagation`, the settings editor assumes we lost focus on this setting completely.
        {
            let (checkbox, on_value_change) = (checkbox.clone(), on_value_change.clone());
            self.base.list_disposables().add(self.ui.listen(checkbox_wrapper, EventKind::PointerDown, false, move |e| {
                checkbox.set_checked(!checkbox.checked());
                on_value_change(checkbox.checked());
                e.stop_propagation();
            }));
        }
        {
            let (checkbox, on_value_change) = (checkbox.clone(), on_value_change.clone());
            self.base.list_disposables().add(self.ui.listen(value_element, EventKind::PointerDown, false, move |e| {
                checkbox.set_checked(!checkbox.checked());
                on_value_change(checkbox.checked());
                e.stop(true);
            }));
        }

        // We add the tooltips here, because the method is not called by default for widgets in edit mode
        let row_element_group = RowElementGroup { row_element: row, key_element: value_element, value_element: Some(checkbox.dom_node()) };
        self.add_tooltips_to_row(&row_element_group, item);

        self.row_layouts.register(&self.ui, row, ObjectRowLayout::Checkbox { wrapper: checkbox_wrapper, checkbox, value_node: value_element, text: checkbox_description });
        row
    }

    fn add_tooltips_to_row(&self, row_element: &RowElementGroup, item: &IBoolObjectDataItem) {
        let accessible_description = localize("objectPairHintLabel", "The property `{0}` is set to `{1}`.", &[item.key.as_str(), &item.value.to_string()]);
        let title = item.key_description.clone().unwrap_or_else(|| accessible_description.clone());
        self.base.setup_row_hover(row_element.key_element, HoverContent::Text(title));
        let mut tree = self.ui.tree_mut();
        if let Some(value_element) = row_element.value_element {
            tree.set_label(value_element, accessible_description.clone());
        }
        tree.set_label(row_element.row_element, accessible_description);
    }

    fn layout_row(&self, row: NodeId, width: f64) -> f64 {
        layout_object_row(&self.base, &self.row_layouts, row, width)
    }
}

impl IDisposable for ObjectSettingCheckboxWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}
