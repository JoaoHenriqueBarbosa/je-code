//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/components/gutterIndicatorMenu.ts`:
//! `GutterIndicatorMenuContent`, o conteúdo do menu que o indicador da margem abre (o cabeçalho, as ações, os atalhos
//! exibidos e o rodapé com a barra de ações).
//!
//! Notas de tradução:
//! - o DOM do original (`hoverContent`, `header`, `option`, `separator` e `actionBar`, montados com `n.div`) é o dado
//!   [`GutterMenuContentVisual`], uma lista de [`GutterMenuNode`], que o pintor nativo lê. `toDisposableLiveElement()` é
//!   [`GutterIndicatorMenuContent::to_visual`]. As medidas do CSS inline são as constantes `MENU_*`, `HEADER_*`, `OPTION_*`,
//!   `SEPARATOR_*` e `ACTION_BAR_*`; as cores são ids do tema ([`DESCRIPTION_FOREGROUND`], [`EDITOR_ACTION_LIST_FOREGROUND`]
//!   e [`EDITOR_HOVER_BORDER`], os `asCssVariable` do original). O `LiveElement` guardava o `store` dos `KeybindingLabel` e do
//!   `ActionBar`; aqui esses widgets são do pintor (que cria o `KeybindingLabel` com [`KEYBINDING_LABEL_FOREGROUND`], fundo e
//!   borda transparentes, sem sombra, sem borda inferior e sem título) e o conteúdo não guarda recurso algum: os observáveis
//!   só assinam o evento de contexto enquanto são observados;
//! - `option(...)` devolve um `derived` sem dependências, que no original só recria o elemento. Aqui é o
//!   [`GutterMenuOption`] dentro de um observável constante, e o `onmouseenter`, o `onmouseleave`, o `onclick` e o `onkeydown`
//!   (`Enter`) são [`GutterMenuOption::notify_mouse_enter`], [`GutterMenuOption::notify_mouse_leave`],
//!   [`GutterMenuOption::notify_click`] e [`GutterMenuOption::notify_key_down`], que o pintor chama. A classe `active` é o
//!   [`GutterMenuOption::is_active`]; o `tabIndex: 0` é o item ser focável (o pintor navega pelas opções);
//! - o `icon` e o `commandId` de `createOptionArgs` aceitam `IObservable<...>` no tipo, mas toda chamada do original passa um
//!   `ThemeIcon` e um `string`: só esses ramos existem. Pelo mesmo motivo o `title` do `header` é só texto. `FirstFnArg` é
//!   um tipo auxiliar de TypeScript (o argumento de `option`) e é o [`OptionSpec`] privado;
//! - `commandArgs ? undefined : commandId` do atalho vale para qualquer lista, inclusive a vazia (no JavaScript `[]` é
//!   verdadeiro): é `command_args.is_some()`. `this._commandService.executeCommand(...)` devolve a promessa que o `onAction`
//!   (`() => void`) descarta, como no original;
//! - `const showModelEnabled = false` fica como [`SHOW_MODEL_ENABLED`]: as opções de modelo são montadas só com ele ligado;
//! - `@IContextKeyService`, `@IKeybindingService` e `@ICommandService` são os `Rc<dyn ...>` de
//!   [`GutterIndicatorMenuServices`]. `_getKeybinding` é a `lookup_keybinding_observable` do `jumpToView`, que tem o mesmo
//!   corpo (`observableFromEvent(onDidChangeContext, () => lookupKeybinding(id))`);
//! - `nativeHoverDelegate` (sem hover dentro de hover) é [`GutterMenuActionBar::native_hover`], e `{ icon: false, label: true }`
//!   do `push` são [`GutterMenuActionBar::show_icon`] e [`GutterMenuActionBar::show_label`]. As ações da barra são `IAction`
//!   reais ([`ActionRef`]), prontas para o `ActionBar::push` do pintor;
//! - **ponto de ligação com o indicador.** O `_showHover` do `gutterIndicatorView` cria o conteúdo, mede o foco com
//!   `createFocusTracker` e chama `IHoverService.showInstantHover({ target, content })`. O hover e o rastreador de foco são
//!   do pintor nativo (o `HoverService` do `je-platform` recebe um elemento do DOM como conteúdo): entram pelo
//!   [`IGutterMenuHoverPresenter`]. O [`GutterIndicatorMenuHost`] implementa o `IGutterIndicatorHost::show_menu` do indicador
//!   (cria o conteúdo com o `onClose` do pedido, monta o visual e o entrega ao apresentador). O descarte do hover, o foco no
//!   menu e o `_hoverVisible` ficam no indicador, como no original.

use std::rc::Rc;

use je_base::observable::Observable;
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::keybindings::ResolvedKeybinding;
use je_base::nls::localize;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, ObservableExt, SettableExt, const_observable, observable_value};
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_platform::commands::ICommandService;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::IKeybindingService;

use crate::config::editor_options::inline_suggest::INLINE_SUGGEST;
use crate::core_2d::rect::Rect;
use crate::inline_completions::controller::command_ids::{
    HIDE_INLINE_COMPLETION_ID, INLINE_SUGGEST_COMMIT_ALTERNATIVE_ACTION_ID, INLINE_SUGGEST_COMMIT_ID, TOGGLE_SHOW_COLLAPSED_ID,
};
use crate::observable_code_editor::ObservableCodeEditor;

use super::super::inline_edits_views::jump_to_view::lookup_keybinding_observable;
use super::gutter_indicator_view::{
    GutterIndicatorMenuRequest, GutterMenuCloseCallback, IGutterIndicatorHost, IGutterIndicatorMenuSession, InlineSuggestionGutterMenuData,
};

/// `descriptionForeground`: a cor do cabeçalho e do atalho de cada opção.
pub const DESCRIPTION_FOREGROUND: &str = "descriptionForeground";
/// `editorActionListForeground` (`editorActionList.foreground`): a cor do separador.
pub const EDITOR_ACTION_LIST_FOREGROUND: &str = "editorActionList.foreground";
/// `editorHoverBorder` (`editorHoverWidget.border`): a cor da linha do separador.
pub const EDITOR_HOVER_BORDER: &str = "editorHoverWidget.border";
/// `keybindingLabelForeground` que o menu passa ao `KeybindingLabel` (fundo e borda são transparentes).
pub const KEYBINDING_LABEL_FOREGROUND: &str = DESCRIPTION_FOREGROUND;

/// `margin: 4` do `div.content`.
pub const MENU_CONTENT_MARGIN: f64 = 4.0;
/// `minWidth: 180` do `div.content`.
pub const MENU_CONTENT_MIN_WIDTH: f64 = 180.0;
/// `fontSize: '13px'` do cabeçalho.
pub const HEADER_FONT_SIZE: f64 = 13.0;
/// `fontWeight: '600'` do cabeçalho.
pub const HEADER_FONT_WEIGHT: u16 = 600;
/// `padding: '0 4px'` do cabeçalho: o recuo horizontal.
pub const HEADER_PADDING_HORIZONTAL: f64 = 4.0;
/// `lineHeight: 28` do cabeçalho.
pub const HEADER_LINE_HEIGHT: f64 = 28.0;
/// `borderRadius: 3` da opção (o mesmo do widget de hover).
pub const OPTION_BORDER_RADIUS: f64 = 3.0;
/// `fontSize: 16` do ícone da opção.
pub const OPTION_ICON_FONT_SIZE: f64 = 16.0;
/// `padding: '2px 0'` do separador: o recuo vertical.
pub const SEPARATOR_PADDING_VERTICAL: f64 = 2.0;
/// O `borderBottom: '1px solid ...'` do separador.
pub const SEPARATOR_BORDER_WIDTH: f64 = 1.0;
/// O `id` do separador.
pub const SEPARATOR_ID: &str = "inline-edit-gutter-indicator-menu-separator";
/// `padding: '3px 24px'` da barra de ações: o recuo vertical.
pub const ACTION_BAR_PADDING_VERTICAL: f64 = 3.0;
/// `padding: '3px 24px'` da barra de ações: o recuo horizontal.
pub const ACTION_BAR_PADDING_HORIZONTAL: f64 = 24.0;

/// `const showModelEnabled = false`.
const SHOW_MODEL_ENABLED: bool = false;

/// O atalho de um comando, relido quando o contexto muda.
type KeybindingObservable = IObservable<Option<Rc<dyn ResolvedKeybinding>>>;

/// Os serviços que o construtor do conteúdo recebe (`@IContextKeyService`, `@IKeybindingService` e `@ICommandService`).
#[derive(Clone)]
pub struct GutterIndicatorMenuServices {
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub command_service: Rc<dyn ICommandService>,
}

/// O que `option(props)` recebe e o pintor lê: uma linha do menu.
pub struct GutterMenuOption {
    pub title: String,
    pub icon: Rc<ThemeIcon>,
    pub keybinding: KeybindingObservable,
    /// A classe `active` (o ponteiro está sobre a opção).
    pub is_active: IObservable<bool>,
    on_hover_change: Rc<dyn Fn(bool)>,
    on_action: Rc<dyn Fn()>,
}

impl GutterMenuOption {
    pub fn new(
        title: String,
        icon: ThemeIcon,
        keybinding: KeybindingObservable,
        is_active: IObservable<bool>,
        on_hover_change: impl Fn(bool) + 'static,
        on_action: impl Fn() + 'static,
    ) -> Self {
        Self { title, icon: Rc::new(icon), keybinding, is_active, on_hover_change: Rc::new(on_hover_change), on_action: Rc::new(on_action) }
    }

    /// O `onmouseenter`.
    pub fn notify_mouse_enter(&self) {
        (self.on_hover_change)(true);
    }

    /// O `onmouseleave`.
    pub fn notify_mouse_leave(&self) {
        (self.on_hover_change)(false);
    }

    /// O `onclick`.
    pub fn notify_click(&self) {
        (self.on_action)();
    }

    /// O `onkeydown`: `Enter` roda a ação (`e.key`).
    pub fn notify_key_down(&self, key: &str) {
        if key == "Enter" {
            (self.on_action)();
        }
    }
}

/// `actionBar(actions, options)`: o rodapé com as ações da sugestão.
pub struct GutterMenuActionBar {
    pub actions: Vec<ActionRef>,
    /// `hoverDelegate: nativeHoverDelegate`: não dá para mostrar hover dentro de outro hover.
    pub native_hover: bool,
    /// O `icon: false` do `push`.
    pub show_icon: bool,
    /// O `label: true` do `push`.
    pub show_label: bool,
}

/// Um filho de `div.content`.
pub enum GutterMenuNode {
    /// `header(title)`.
    Header(String),
    /// `option(...)`: observável porque o `toggleCollapsedMode` troca de opção com `showCollapsed`.
    Option(IObservable<Rc<GutterMenuOption>>),
    /// `separator()`.
    Separator,
    /// `actionBar(...)`.
    ActionBar(Rc<GutterMenuActionBar>),
}

/// `hoverContent(content)`: o que o pintor nativo lê. As medidas do `div.content` são [`MENU_CONTENT_MARGIN`] e
/// [`MENU_CONTENT_MIN_WIDTH`].
pub struct GutterMenuContentVisual {
    pub nodes: Vec<GutterMenuNode>,
}

/// `FirstFnArg<typeof option>` de `createOptionArgs`: o que descreve uma opção que roda um comando.
struct OptionSpec {
    id: String,
    title: String,
    icon: ThemeIcon,
    command_id: String,
    command_args: Option<Vec<Dynamic>>,
}

/// O que as opções compartilham: o `_close`, os serviços e o `activeElement`.
struct MenuShared {
    close: GutterMenuCloseCallback,
    services: GutterIndicatorMenuServices,
}

impl MenuShared {
    /// `_getKeybinding(commandId)`.
    fn get_keybinding(&self, command_id: Option<&str>) -> KeybindingObservable {
        match command_id {
            None => const_observable(None),
            Some(command_id) => {
                lookup_keybinding_observable(command_id, Rc::clone(&self.services.keybinding_service), &self.services.context_key_service)
            }
        }
    }

    /// `option(createOptionArgs(options))`.
    fn create_option(self: &Rc<Self>, active_element: &Rc<ObservableValue<Option<String>, ()>>, spec: OptionSpec) -> Rc<GutterMenuOption> {
        let OptionSpec { id, title, icon, command_id, command_args } = spec;
        let keybinding = self.get_keybinding(if command_args.is_some() { None } else { Some(command_id.as_str()) });

        let hover_change = on_hover_change(active_element, id.clone());
        let this = Rc::clone(self);
        let on_action = move || {
            (this.close)(true, Some(command_id.clone()));
            let _ = this.services.command_service.execute_command(&command_id, command_args.clone().unwrap_or_default());
        };

        Rc::new(GutterMenuOption::new(title, icon, keybinding, is_active(active_element, id), hover_change, on_action))
    }
}

/// `isActive: activeElement.map(v => v === id)`.
fn is_active(active_element: &Rc<ObservableValue<Option<String>, ()>>, id: String) -> IObservable<bool> {
    active_element.to_observable().map(move |value, _| value.as_deref() == Some(id.as_str()))
}

/// `onHoverChange: v => activeElement.set(v ? id : undefined, undefined)`.
fn on_hover_change(active_element: &Rc<ObservableValue<Option<String>, ()>>, id: String) -> impl Fn(bool) + 'static {
    let active_element = Rc::clone(active_element);
    move |hovered| active_element.set(if hovered { Some(id.clone()) } else { None }, None)
}

/// `GutterIndicatorMenuContent`.
pub struct GutterIndicatorMenuContent {
    data: Rc<InlineSuggestionGutterMenuData>,
    shared: Rc<MenuShared>,
    inline_edits_show_collapsed: IObservable<bool>,
}

impl GutterIndicatorMenuContent {
    pub fn new(
        editor_obs: &Rc<ObservableCodeEditor>,
        data: Rc<InlineSuggestionGutterMenuData>,
        close: GutterMenuCloseCallback,
        services: GutterIndicatorMenuServices,
    ) -> Self {
        let inline_edits_show_collapsed = editor_obs.get_option(INLINE_SUGGEST).map(|options, _| options.edits.show_collapsed);
        Self { data, shared: Rc::new(MenuShared { close, services }), inline_edits_show_collapsed }
    }

    /// `toDisposableLiveElement()`: o conteúdo do hover como dado.
    pub fn to_visual(&self) -> Rc<GutterMenuContentVisual> {
        Rc::new(GutterMenuContentVisual { nodes: self.create_hover_content() })
    }

    /// `_createHoverContent()`.
    fn create_hover_content(&self) -> Vec<GutterMenuNode> {
        let shared = &self.shared;
        let active_element = observable_value::<Option<String>, ()>("active", None);

        let extension_command_groups: Vec<Vec<GutterMenuNode>> = self
            .data
            .extension_commands
            .iter()
            .map(|group| {
                group
                    .iter()
                    .enumerate()
                    .map(|(idx, c)| {
                        GutterMenuNode::Option(const_observable(shared.create_option(
                            &active_element,
                            OptionSpec {
                                id: format!("{}_{}", c.command.id, idx),
                                title: c.command.title.clone(),
                                icon: c.icon.clone().unwrap_or_else(|| ThemeIcon::from_id("symbol-event")),
                                command_id: c.command.id.clone(),
                                command_args: c.command.arguments.clone(),
                            },
                        )))
                    })
                    .collect()
            })
            .collect();

        let mut extension_command_nodes: Vec<GutterMenuNode> = Vec::new();
        for group in extension_command_groups {
            if !group.is_empty() {
                extension_command_nodes.push(GutterMenuNode::Separator);
                extension_command_nodes.extend(group);
            }
        }

        if self.data.extension_commands_only {
            // drop leading separator
            return extension_command_nodes.into_iter().skip(1).collect();
        }

        let command_option = |id: &str, title: String, icon: ThemeIcon, command_id: &str, command_args: Option<Vec<Dynamic>>| {
            GutterMenuNode::Option(const_observable(shared.create_option(
                &active_element,
                OptionSpec { id: id.to_owned(), title, icon, command_id: command_id.to_owned(), command_args },
            )))
        };

        let title = GutterMenuNode::Header(self.data.display_name.clone());

        let goto_and_accept = command_option(
            "gotoAndAccept",
            localize("gotoAndAccept", "Go To / Accept", &[]),
            ThemeIcon::from_id("check"),
            INLINE_SUGGEST_COMMIT_ID,
            None,
        );

        let reject = command_option("reject", localize("reject", "Reject", &[]), ThemeIcon::from_id("close"), HIDE_INLINE_COMPLETION_ID, None);

        let alternative_command = self.data.alternative_action.as_ref().map(|action| {
            command_option(
                "alternativeCommand",
                action.command.title.clone(),
                action.icon.clone(),
                INLINE_SUGGEST_COMMIT_ALTERNATIVE_ACTION_ID,
                None,
            )
        });

        let model_options: Vec<GutterMenuNode> = if SHOW_MODEL_ENABLED { self.create_model_options(&active_element) } else { Vec::new() };

        let toggle_collapsed_mode = {
            let (shared, active_element) = (Rc::clone(shared), Rc::clone(&active_element));
            self.inline_edits_show_collapsed.map(move |show_collapsed, _| {
                let (id, title, icon) = if show_collapsed {
                    ("showExpanded", localize("showExpanded", "Show Expanded", &[]), ThemeIcon::from_id("expand-all"))
                } else {
                    ("showCollapsed", localize("showCollapsed", "Show Collapsed", &[]), ThemeIcon::from_id("collapse-all"))
                };
                shared.create_option(
                    &active_element,
                    OptionSpec { id: id.to_owned(), title, icon, command_id: TOGGLE_SHOW_COLLAPSED_ID.to_owned(), command_args: None },
                )
            })
        };

        let snooze = command_option(
            "snooze",
            localize("snooze", "Snooze", &[]),
            ThemeIcon::from_id("bell-slash"),
            "editor.action.inlineSuggest.snooze",
            None,
        );

        let settings = command_option(
            "settings",
            localize("settings", "Settings", &[]),
            ThemeIcon::from_id("gear"),
            "workbench.action.openSettings",
            Some(vec![Dynamic::string("@tag:nextEditSuggestions")]),
        );

        let action_bar_footer = self.create_action_bar_footer();

        let mut nodes = vec![title, goto_and_accept];
        nodes.extend(alternative_command);
        nodes.push(reject);
        nodes.push(GutterMenuNode::Option(toggle_collapsed_mode));
        if !model_options.is_empty() {
            nodes.push(GutterMenuNode::Separator);
        }
        nodes.extend(model_options);
        nodes.push(snooze);
        nodes.push(settings);
        nodes.extend(extension_command_nodes);
        if let Some(action_bar_footer) = action_bar_footer {
            nodes.push(GutterMenuNode::Separator);
            nodes.push(GutterMenuNode::ActionBar(action_bar_footer));
        }
        nodes
    }

    /// `modelOptions` (só com [`SHOW_MODEL_ENABLED`]).
    fn create_model_options(&self, active_element: &Rc<ObservableValue<Option<String>, ()>>) -> Vec<GutterMenuNode> {
        let Some(model_info) = &self.data.model_info else {
            return Vec::new();
        };
        model_info
            .models
            .iter()
            .map(|m| {
                let active_id = format!("model_{}", m.id);
                let icon = ThemeIcon::from_id(if m.id == model_info.current_model_id { "check" } else { "circle" });
                let (shared, set_model_id, model_id) = (Rc::clone(&self.shared), self.data.set_model_id.clone(), m.id.clone());
                GutterMenuNode::Option(const_observable(Rc::new(GutterMenuOption::new(
                    m.name.clone(),
                    icon,
                    const_observable(None),
                    is_active(active_element, active_id.clone()),
                    on_hover_change(active_element, active_id),
                    move || {
                        (shared.close)(true, None);
                        if let Some(set_model_id) = &set_model_id {
                            let _ = set_model_id(&model_id);
                        }
                    },
                ))))
            })
            .collect()
    }

    /// `actionBarFooter`: `undefined` sem a ação da sugestão.
    fn create_action_bar_footer(&self) -> Option<Rc<GutterMenuActionBar>> {
        let command_service = &self.shared.services.command_service;
        let actions: Vec<ActionRef> = self
            .data
            .action
            .iter()
            .map(|action| {
                let (command_service, id, arguments) = (Rc::clone(command_service), action.id.clone(), action.arguments.clone());
                to_action(ToActionProps {
                    id: action.id.clone(),
                    label: format!("{}...", action.title),
                    tooltip: Some(action.tooltip.clone().unwrap_or_else(|| action.title.clone())),
                    enabled: Some(true),
                    checked: None,
                    class: None,
                    alt: None,
                    run: Rc::new(move |_| MaybePromise::Promise(command_service.execute_command(&id, arguments.clone().unwrap_or_default()))),
                })
            })
            .collect();
        if actions.is_empty() {
            return None;
        }
        Some(Rc::new(GutterMenuActionBar { actions, native_hover: true, show_icon: false, show_label: true }))
    }
}

/// O que o hover (`IHoverService.showInstantHover`) recebe do menu.
pub struct GutterMenuHoverRequest {
    /// O `target: this._iconRef.element`: o retângulo da pílula, nas coordenadas do overlay do editor.
    pub target: Rect,
    /// O `content: content.element`.
    pub content: Rc<GutterMenuContentVisual>,
}

/// `IHoverService.showInstantHover` com o `createFocusTracker` do conteúdo: o pintor nativo mostra o hover, mede o foco
/// dentro dele e devolve a sessão (`None` quando o hover não aparece, como o `undefined` do original).
pub trait IGutterMenuHoverPresenter {
    fn show_instant_hover(&self, request: GutterMenuHoverRequest) -> Option<Rc<dyn IGutterIndicatorMenuSession>>;
}

/// O `IGutterIndicatorHost` do indicador da margem: abre o menu pelo [`IGutterMenuHoverPresenter`].
pub struct GutterIndicatorMenuHost {
    services: GutterIndicatorMenuServices,
    presenter: Rc<dyn IGutterMenuHoverPresenter>,
}

impl GutterIndicatorMenuHost {
    pub fn new(services: GutterIndicatorMenuServices, presenter: Rc<dyn IGutterMenuHoverPresenter>) -> Rc<Self> {
        Rc::new(Self { services, presenter })
    }
}

impl IGutterIndicatorHost for GutterIndicatorMenuHost {
    fn show_menu(&self, request: GutterIndicatorMenuRequest) -> Option<Rc<dyn IGutterIndicatorMenuSession>> {
        let GutterIndicatorMenuRequest { editor, data, target, on_close } = request;
        let content = GutterIndicatorMenuContent::new(&editor, data, on_close, self.services.clone());
        self.presenter.show_instant_hover(GutterMenuHoverRequest { target, content: content.to_visual() })
    }
}
