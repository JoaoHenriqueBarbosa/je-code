//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/hintsWidget/inlineCompletionsHintsWidget.ts`:
//! `InlineCompletionsHintsWidget` (cria o widget de conteúdo quando `showToolbar` é `always`),
//! `InlineSuggestionHintsContentWidget` (a barra com anterior, contador e próximo, o aviso e as ações extras),
//! `ActionViewItemWithClassName`, `StatusBarViewItem` e `CustomizedMenuWorkbenchToolBar`.
//!
//! Notas de tradução:
//! - **DOM vira dado.** `h('div.inlineSuggestionsHints', ...)`, `n.div({ class: 'warningMessage', ... })` e `getDomNode()`
//!   não existem: o pintor nativo lê o [`HintsWidgetVisual`] de [`InlineSuggestionHintsContentWidget::visual`]. As medidas
//!   do `inlineCompletionsHintsWidget.css` são as constantes `HINTS_*`, `WARNING_*`, `KEYBINDING_*`, `AVAILABLE_COUNT_*` e
//!   `STATUS_BAR_LABEL_*`. O `renderMarkdown(warning.message)` do aviso também é do pintor: o dado é o
//!   [`WarningMessageContent`] (texto ou o `IMarkdownString`);
//! - **Toolbar de menu.** O `WorkbenchToolBar` do workbench (`je_workbench::platform::actions::toolbar`) estende a `ToolBar`
//!   de `je-widgets`, e o `je-editor` não depende do `je-workbench` nem de widgets. O `CustomizedMenuWorkbenchToolBar` é
//!   portado como o modelo da barra, [`CustomizedMenuWorkbenchToolBar`]: ele faz o que o original faz com o menu
//!   (`IMenuService.createMenu`, `getActionBarActions`, as ações prefixadas, adicionais primárias e adicionais secundárias) e
//!   entrega o resultado em [`ToolBarActions`] por um observável. Quem desenha (o pintor, que cria o `MenuWorkbenchToolBar` de
//!   `je-workbench` ou o desenho nativo equivalente) lê as ações, o [`ToolBarViewItem`] de cada uma (o
//!   `actionViewItemProvider`), o `resetMenu`, o `telemetrySource` e avisa a abertura do menu secundário por
//!   [`CustomizedMenuWorkbenchToolBar::set_dropdown_visible`] (o `onDidChangeDropdownVisibility`). Os serviços que só o
//!   `WorkbenchToolBar` pedia (`IContextMenuService`, `IKeybindingService`, `ICommandService` e `ITelemetryService`) ficam com
//!   o pintor. O mesmo padrão (menu do `je-platform` mais `get_action_bar_actions`) é o do `MenuWorkbenchToolBar` do
//!   `je-workbench`;
//! - `createHotClass(this)` (recarga a quente do desenvolvimento do VS Code) não existe: o `.hot.read(reader)` vira a
//!   construção direta; `registerIcon` não tem porte (ver [`INLINE_SUGGESTION_HINTS_NEXT_ICON_ID`]);
//! - `@IInstantiationService` é o [`InlineSuggestionHintsServices`], montado do `InlineEditsViewServices` que a
//!   `InlineSuggestionsView` já tem;
//! - `_dropDownVisible` e `id` (`static`) são `thread_local`: o editor inteiro roda no thread da interface;
//! - `this._register(this.createCommandAction(...))` registra a `Action` no `Disposable` do widget; o `Action` do original
//!   com `enabled = false` e `label = ''` muda as propriedades pelos setters de `IAction`, que disparam o `onDidChange`;
//! - `layoutContentWidget(this)` usa um `Weak` do widget, porque o autorun roda dentro do widget e o editor guarda o `Rc`;
//! - `IActionViewItemOptions` (o segundo argumento do `actionViewItemProvider`) é do desenho e não entra no provedor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, IAction, Separator, ToActionProps, to_action};
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, Observable, ObservableExt, SettableExt, StrictEquals, autorun, autorun_with_store, derived, derived_observable_with_cache, observable_value,
};
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_platform::actions::menu_entry_action_view_item::{PrimaryAndSecondaryActions, get_action_bar_actions};
use je_platform::actions::menu_ids::{inline_completions_actions, inline_suggestion_toolbar};
use je_platform::actions::{IMenu, IMenuService, MenuAction, MenuActionOptions, MenuCreateOptions, MenuId, MenuItemAction};
use je_platform::commands::ICommandService;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::IKeybindingService;

use crate::code_editor_widget::CodeEditorWidget;
use crate::config::editor_options::inline_suggest::{INLINE_SUGGEST, InlineSuggestShowToolbar};
use crate::editor_browser::{ContentWidgetPosition, ContentWidgetPositionPreference, IContentWidget};
use crate::inline_completions::controller::command_ids::{SHOW_NEXT_INLINE_SUGGESTION_ACTION_ID, SHOW_PREVIOUS_INLINE_SUGGESTION_ACTION_ID};
use crate::inline_completions::inline_completions_model::InlineCompletionsModel;
use crate::inline_completions::view::inline_edits::inline_edits_view::InlineEditsViewServices;
use crate::languages::{InlineCompletionCommand, InlineCompletionTriggerKind, InlineCompletionWarning, StringOrMarkdown};
use crate::model::PositionAffinity;
use crate::observable_code_editor::observable_code_editor;
use crate::position::Position;

/// O id do ícone `inline-suggestion-hints-next` (`registerIcon('inline-suggestion-hints-next', Codicon.chevronRight, ...)`).
/// O registro de ícones não tem porte: o id é o `ThemeIcon.fromId(id)` que `registerIcon` devolve, e o ícone padrão que a
/// tabela de ícones resolve para ele é o codicon `chevron-right`.
pub const INLINE_SUGGESTION_HINTS_NEXT_ICON_ID: &str = "inline-suggestion-hints-next";
/// O id do ícone `inline-suggestion-hints-previous` (ícone padrão: o codicon `chevron-left`).
pub const INLINE_SUGGESTION_HINTS_PREVIOUS_ICON_ID: &str = "inline-suggestion-hints-previous";

/// O `h('div.inlineSuggestionsHints')`: a classe do contêiner.
pub const HINTS_WIDGET_CLASS_NAME: &str = "inlineSuggestionsHints";
/// O `className` do contêiner quando `withBorder`: o visual do widget de hover.
pub const HINTS_WIDGET_BORDER_CLASS_NAME: &str = "monaco-hover monaco-hover-content";
/// `padding: 4px` de `.inlineSuggestionsHints`.
pub const HINTS_PADDING: f64 = 4.0;
/// `class: 'warningMessage'`.
pub const WARNING_MESSAGE_CLASS_NAME: &str = "warningMessage";
/// `maxWidth: 400` do `warningMessage`.
pub const WARNING_MESSAGE_MAX_WIDTH: f64 = 400.0;
/// `margin: 4` do `warningMessage`.
pub const WARNING_MESSAGE_MARGIN: f64 = 4.0;
/// `marginBottom: 4` do `warningMessage`.
pub const WARNING_MESSAGE_MARGIN_BOTTOM: f64 = 4.0;
/// `.warningMessage p { margin: 0 }`.
pub const WARNING_MESSAGE_PARAGRAPH_MARGIN: f64 = 0.0;
/// `.keybinding { margin-left: 4px }`.
pub const KEYBINDING_MARGIN_LEFT: f64 = 4.0;
/// `.keybinding { opacity: 0.6 }`.
pub const KEYBINDING_OPACITY: f64 = 0.6;
/// `.keybinding .monaco-keybinding-key { font-size: 8px }`.
pub const KEYBINDING_KEY_FONT_SIZE: f64 = 8.0;
/// `.keybinding .monaco-keybinding-key { padding: 2px 3px }`: o recuo vertical.
pub const KEYBINDING_KEY_PADDING_VERTICAL: f64 = 2.0;
/// `.keybinding .monaco-keybinding-key { padding: 2px 3px }`: o recuo horizontal.
pub const KEYBINDING_KEY_PADDING_HORIZONTAL: f64 = 3.0;
/// A classe do contador (`a.setClass('availableSuggestionCount')`).
pub const AVAILABLE_SUGGESTION_COUNT_CLASS_NAME: &str = "availableSuggestionCount";
/// `.availableSuggestionCount a { min-width: 19px }`.
pub const AVAILABLE_COUNT_MIN_WIDTH: f64 = 19.0;
/// `.inlineSuggestionStatusBarItemLabel { margin-right: 2px }`.
pub const STATUS_BAR_LABEL_MARGIN_RIGHT: f64 = 2.0;
/// A classe do rótulo do item de status (`inlineSuggestionStatusBarItemLabel`).
pub const STATUS_BAR_LABEL_CLASS_NAME: &str = "inlineSuggestionStatusBarItemLabel";
/// O `disableTitle: true` do `KeybindingLabel` do item de status (as demais opções são as `unthemedKeybindingLabelOptions`).
pub const STATUS_BAR_KEYBINDING_DISABLE_TITLE: bool = true;
/// O id da ação do contador.
pub const AVAILABLE_SUGGESTION_COUNT_ACTION_ID: &str = "inlineSuggestionHints.availableSuggestionCount";
/// O atraso dos `RunOnceScheduler` do contador e dos botões, em milissegundos.
const DEBOUNCE_MS: u64 = 100;

thread_local! {
    /// `InlineSuggestionHintsContentWidget._dropDownVisible`.
    static DROP_DOWN_VISIBLE: Cell<bool> = const { Cell::new(false) };
    /// `InlineSuggestionHintsContentWidget.id`.
    static NEXT_WIDGET_ID: Cell<u32> = const { Cell::new(0) };
}

/// Os serviços que o `@IInstantiationService` injetava no [`InlineSuggestionHintsContentWidget`]:
/// `@ICommandService`, `@IKeybindingService`, `@IContextKeyService` e `@IMenuService`.
#[derive(Clone)]
pub struct InlineSuggestionHintsServices {
    pub command_service: Rc<dyn ICommandService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub menu_service: Rc<dyn IMenuService>,
}

impl InlineSuggestionHintsServices {
    /// Os serviços do hints widget a partir dos que a `InlineSuggestionsView` já recebe.
    pub fn from_view_services(services: &InlineEditsViewServices) -> Self {
        Self {
            command_service: Rc::clone(&services.command_service),
            keybinding_service: Rc::clone(&services.keybinding_service),
            context_key_service: Rc::clone(&services.context_key_service),
            menu_service: Rc::clone(&services.menu_service),
        }
    }
}

/// `InlineCompletionsHintsWidget`: com `showToolbar: 'always'`, mostra o [`InlineSuggestionHintsContentWidget`] na linha
/// do texto fantasma e pede o disparo explícito (para o contador de sugestões ter o que mostrar).
pub struct InlineCompletionsHintsWidget {
    base: Rc<Disposable>,
    position: IObservable<Option<Position>>,
}

impl InlineCompletionsHintsWidget {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        model: IObservable<Option<Rc<InlineCompletionsModel>>>,
        services: InlineSuggestionHintsServices,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let always_show_toolbar: IObservable<bool> =
            observable_code_editor(&editor).get_option(INLINE_SUGGEST).map(|options, _| options.show_toolbar == InlineSuggestShowToolbar::Always);

        let session_position: Rc<Cell<Option<Position>>> = Rc::new(Cell::new(None));
        let position: IObservable<Option<Position>> = {
            let (model, always_show_toolbar, session_position) = (Rc::clone(&model), Rc::clone(&always_show_toolbar), Rc::clone(&session_position));
            derived(move |reader| {
                let ghost_text = model.read(reader).and_then(|model| model.primary_ghost_text.read(reader));
                let always = always_show_toolbar.read(reader);

                let Some(ghost_text) = ghost_text.filter(|ghost_text| always && !ghost_text.parts().is_empty()) else {
                    session_position.set(None);
                    return None;
                };

                let first_column = ghost_text.parts()[0].column;
                if session_position.get().is_some_and(|session| session.line_number != ghost_text.line_number()) {
                    session_position.set(None);
                }

                let position = Position::new(ghost_text.line_number(), first_column.min(session_position.get().map_or(i32::MAX, |session| session.column)));
                session_position.set(Some(position));
                Some(position)
            })
        };

        base.register(autorun_with_store({
            let (editor, model, always_show_toolbar, position) = (Rc::clone(&editor), Rc::clone(&model), always_show_toolbar, Rc::clone(&position));
            move |reader, store| {
                /* @description setup content widget */
                let Some(model) = model.read(reader) else {
                    return;
                };
                if !always_show_toolbar.read(reader) {
                    return;
                }

                let content_widget_value: IObservable<Rc<InlineSuggestionHintsContentWidget>> = {
                    let (editor, model, position, services) = (Rc::clone(&editor), Rc::clone(&model), Rc::clone(&position), services.clone());
                    derived(move |reader| {
                        let content_widget = reader.store().add_rc(InlineSuggestionHintsContentWidget::new(
                            Rc::clone(&editor),
                            true,
                            Rc::clone(&position),
                            Rc::clone(&model.selected_inline_completion_index),
                            Rc::clone(&model.inline_completions_count),
                            Rc::clone(&model.active_commands),
                            Rc::clone(&model.warning),
                            Rc::new(|| {}),
                            services.clone(),
                        ));
                        let widget: Rc<dyn IContentWidget> = Rc::clone(&content_widget) as Rc<dyn IContentWidget>;
                        editor.add_content_widget(Rc::clone(&widget));
                        reader.store().add(to_disposable({
                            let editor = Rc::clone(&editor);
                            move || editor.remove_content_widget(&widget)
                        }));

                        reader.store().add(autorun({
                            let (model, position) = (Rc::clone(&model), Rc::clone(&position));
                            move |reader| {
                                /* @description request explicit */
                                if position.read(reader).is_none() {
                                    return;
                                }
                                if model.last_trigger_kind.read(reader) != Some(InlineCompletionTriggerKind::Explicit) {
                                    let _ = model.trigger_explicitly(None, false);
                                }
                            }
                        }));
                        content_widget
                    })
                };

                let had_position: IObservable<bool> = {
                    let position = Rc::clone(&position);
                    derived_observable_with_cache::<bool>(None, move |reader, last_value| position.read(reader).is_some() || last_value.copied().unwrap_or(false))
                };
                store.add(autorun(move |reader| {
                    if had_position.read(reader) {
                        content_widget_value.read(reader);
                    }
                }));
            }
        }));

        Rc::new(Self { base, position })
    }

    /// `position`: a posição do widget (a primeira coluna do texto fantasma, sem recuar dentro da mesma linha).
    pub fn position(&self) -> IObservable<Option<Position>> {
        Rc::clone(&self.position)
    }
}

impl IDisposable for InlineCompletionsHintsWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que `_warningMessageContentNode` devolve: a mensagem em texto, ou o `IMarkdownString` que o pintor renderiza
/// (`renderMarkdown(warning.message)`).
#[derive(Clone, Debug)]
pub enum WarningMessageContent {
    Text(String),
    Markdown(Rc<je_base::html_content::IMarkdownString>),
}

impl StrictEquals for WarningMessageContent {
    /// O texto compara por valor; o markdown é um elemento novo a cada cálculo no original, e aqui a identidade do `Rc`.
    fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Markdown(a), Self::Markdown(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `this.nodes`: o que o pintor desenha do widget (o `getDomNode()` do original). Os filhos são o `warningMessage` e o
/// `toolBar`, nessa ordem.
#[derive(Clone)]
pub struct HintsWidgetVisual {
    /// `withBorder`: o contêiner leva as classes de [`HINTS_WIDGET_BORDER_CLASS_NAME`].
    pub with_border: bool,
    /// O conteúdo de `_warningMessageNode` (`_warningMessageContentNode`).
    pub warning_message: IObservable<Option<WarningMessageContent>>,
    /// `display` do `_warningMessageNode`: `block` com aviso, `none` sem.
    pub warning_message_visible: IObservable<bool>,
    pub tool_bar: Rc<CustomizedMenuWorkbenchToolBar>,
}

/// `InlineSuggestionHintsContentWidget`.
pub struct InlineSuggestionHintsContentWidget {
    base: Rc<Disposable>,
    id: String,
    editor: Rc<CodeEditorWidget>,
    with_border: bool,
    position: IObservable<Option<Position>>,
    warning_message_content_node: IObservable<Option<WarningMessageContent>>,
    warning_message_visible: IObservable<bool>,
    previous_action: Rc<Action>,
    available_suggestion_count_action: Rc<Action>,
    next_action: Rc<Action>,
    tool_bar: Rc<CustomizedMenuWorkbenchToolBar>,
}

/// `createCommandAction(commandId, label, iconClassName)`.
fn create_command_action(services: &InlineSuggestionHintsServices, command_id: &str, label: &str, icon_class_name: &str) -> Rc<Action> {
    let (command_service, id) = (Rc::clone(&services.command_service), command_id.to_owned());
    let action = Action::new(
        command_id,
        label,
        icon_class_name,
        true,
        Some(Rc::new(move |_: Dynamic| MaybePromise::Promise(command_service.execute_command(&id, Vec::new())))),
    );
    action.set_tooltip(services.keybinding_service.append_keybinding(label, Some(command_id), Some(&*services.context_key_service), false));
    action
}

/// O `IAction` literal de `extraCommands.map(c => ({ ... }))`.
fn extra_command_action(command_service: &Rc<dyn ICommandService>, c: &InlineCompletionCommand) -> ActionRef {
    let (command_service, id, arguments) = (Rc::clone(command_service), c.command.id.clone(), c.command.arguments.clone().unwrap_or_default());
    to_action(ToActionProps {
        id: c.command.id.clone(),
        label: c.command.title.clone(),
        tooltip: Some(c.command.tooltip.clone().unwrap_or_default()),
        enabled: Some(true),
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_: &[Dynamic]| MaybePromise::Promise(command_service.execute_command(&id, arguments.clone()))),
    })
}

/// `a === b` entre duas ações: o mesmo objeto.
fn same_action(a: &ActionRef, b: &ActionRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

impl InlineSuggestionHintsContentWidget {
    /// `InlineSuggestionHintsContentWidget.dropDownVisible`: o menu secundário da barra está aberto.
    pub fn drop_down_visible() -> bool {
        DROP_DOWN_VISIBLE.with(Cell::get)
    }

    /// O construtor. `relayout` é o `_relayout` (o `() => { }` do widget fixo e o `onContentsChanged` do hover).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        with_border: bool,
        position: IObservable<Option<Position>>,
        current_suggestion_idx: IObservable<usize>,
        suggestion_count: IObservable<Option<i32>>,
        extra_commands: IObservable<Rc<Vec<InlineCompletionCommand>>>,
        warning: IObservable<Option<Rc<InlineCompletionWarning>>>,
        relayout: Rc<dyn Fn()>,
        services: InlineSuggestionHintsServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let id = format!(
            "InlineSuggestionHintsContentWidget{}",
            NEXT_WIDGET_ID.with(|next| {
                let id = next.get();
                next.set(id + 1);
                id
            })
        );

        let warning_message_content_node: IObservable<Option<WarningMessageContent>> = {
            let warning = Rc::clone(&warning);
            derived(move |reader| {
                let warning = warning.read(reader)?;
                Some(match &warning.message {
                    StringOrMarkdown::String(message) => WarningMessageContent::Text(message.clone()),
                    StringOrMarkdown::Markdown(markdown) => WarningMessageContent::Markdown(Rc::new(markdown.clone())),
                })
            })
        };
        let warning_message_visible: IObservable<bool> = {
            let warning = Rc::clone(&warning);
            derived(move |reader| warning.read(reader).is_some())
        };

        let previous_action = create_command_action(
            &services,
            SHOW_PREVIOUS_INLINE_SUGGESTION_ACTION_ID,
            &localize("previous", "Previous", &[]),
            &ThemeIcon::from_id(INLINE_SUGGESTION_HINTS_PREVIOUS_ICON_ID).as_class_name(),
        );
        base.register(previous_action.clone());
        let available_suggestion_count_action = Action::new(AVAILABLE_SUGGESTION_COUNT_ACTION_ID, "", "", false, None);
        base.register(available_suggestion_count_action.clone());
        let next_action = create_command_action(
            &services,
            SHOW_NEXT_INLINE_SUGGESTION_ACTION_ID,
            &localize("next", "Next", &[]),
            &ThemeIcon::from_id(INLINE_SUGGESTION_HINTS_NEXT_ICON_ID).as_class_name(),
        );
        base.register(next_action.clone());

        // TODO@hediet: deprecate MenuId.InlineCompletionsActions
        let inline_completions_actions_menus: Rc<dyn IMenu> =
            services.menu_service.create_menu(&inline_completions_actions(), Rc::clone(&services.context_key_service), None);
        base.register(inline_completions_actions_menus.clone());

        let clear_available_suggestion_count_label_debounced = RunOnceScheduler::new(
            {
                let available_suggestion_count_action = Rc::clone(&available_suggestion_count_action);
                move || available_suggestion_count_action.set_label(String::new())
            },
            DEBOUNCE_MS,
        );
        base.register(clear_available_suggestion_count_label_debounced.clone());
        let disable_buttons_debounced = RunOnceScheduler::new(
            {
                let (previous_action, next_action) = (Rc::clone(&previous_action), Rc::clone(&next_action));
                move || {
                    previous_action.set_enabled(false);
                    next_action.set_enabled(false);
                }
            },
            DEBOUNCE_MS,
        );
        base.register(disable_buttons_debounced.clone());

        let action_view_item_provider: ActionViewItemProvider = {
            let (keybinding_service, context_key_service) = (Rc::clone(&services.keybinding_service), Rc::clone(&services.context_key_service));
            let count_action: ActionRef = available_suggestion_count_action.clone();
            Rc::new(move |action: &ActionRef| {
                if action.as_any().is::<MenuItemAction>() {
                    return Some(ToolBarViewItem::StatusBar(StatusBarViewItem::new(
                        Rc::clone(action),
                        Rc::clone(&keybinding_service),
                        Rc::clone(&context_key_service),
                    )));
                }
                if same_action(action, &count_action) {
                    let item = ActionViewItemWithClassName::new(Rc::clone(action), true, false);
                    item.set_class(Some(AVAILABLE_SUGGESTION_COUNT_CLASS_NAME.to_owned()));
                    return Some(ToolBarViewItem::ClassNamed(item));
                }
                None
            })
        };

        let tool_bar = CustomizedMenuWorkbenchToolBar::new(
            inline_suggestion_toolbar(),
            CustomizedMenuWorkbenchToolBarOptions {
                menu_options: Some(MenuActionOptions { render_short_title: true, ..MenuActionOptions::default() }),
                primary_group: Rc::new(|group: &str| group.starts_with("primary")),
                use_separators_in_primary_actions: false,
                action_view_item_provider: Some(action_view_item_provider),
                telemetry_source: Some("InlineSuggestionToolbar".to_owned()),
            },
            Rc::clone(&services.menu_service),
            Rc::clone(&services.context_key_service),
        );
        base.register(tool_bar.clone());

        let prepended_primary_actions: Vec<ActionRef> =
            vec![previous_action.clone() as ActionRef, available_suggestion_count_action.clone() as ActionRef, next_action.clone() as ActionRef];
        tool_bar.set_prepended_primary_actions(prepended_primary_actions);

        base.register(tool_bar.on_did_change_dropdown_visibility().listen(|visible| DROP_DOWN_VISIBLE.with(|cell| cell.set(*visible))));

        let this = Rc::new(Self {
            base,
            id,
            editor,
            with_border,
            position,
            warning_message_content_node,
            warning_message_visible,
            previous_action,
            available_suggestion_count_action,
            next_action,
            tool_bar,
        });

        this.base.register(autorun({
            let (content_node, visible) = (Rc::clone(&this.warning_message_content_node), Rc::clone(&this.warning_message_visible));
            move |reader| {
                content_node.read(reader);
                visible.read(reader);
                // Only update after the warning message node has been rendered
                relayout();
            }
        }));

        this.base.register(autorun({
            let (weak, position) = (Rc::downgrade(&this), Rc::clone(&this.position));
            move |reader| {
                /* @description update position */
                position.read(reader);
                if let Some(this) = weak.upgrade() {
                    let widget: Rc<dyn IContentWidget> = Rc::clone(&this) as Rc<dyn IContentWidget>;
                    this.editor.layout_content_widget(&widget);
                }
            }
        }));

        this.base.register(autorun({
            let (previous_action, next_action, count_action) =
                (Rc::clone(&this.previous_action), Rc::clone(&this.next_action), Rc::clone(&this.available_suggestion_count_action));
            move |reader| {
                /* @description counts */
                let suggestion_count = suggestion_count.read(reader);
                let current_suggestion_idx = current_suggestion_idx.read(reader);

                if let Some(suggestion_count) = suggestion_count {
                    clear_available_suggestion_count_label_debounced.cancel();
                    count_action.set_label(format!("{}/{}", current_suggestion_idx + 1, suggestion_count));
                } else {
                    clear_available_suggestion_count_label_debounced.schedule(None);
                }

                if suggestion_count.is_some_and(|count| count > 1) {
                    disable_buttons_debounced.cancel();
                    previous_action.set_enabled(true);
                    next_action.set_enabled(true);
                } else {
                    disable_buttons_debounced.schedule(None);
                }
            }
        }));

        this.base.register(autorun({
            let (command_service, tool_bar) = (Rc::clone(&services.command_service), Rc::clone(&this.tool_bar));
            move |reader| {
                /* @description extra commands */
                let extra_commands = extra_commands.read(reader);
                let mut extra_actions: Vec<ActionRef> = extra_commands.iter().map(|c| extra_command_action(&command_service, c)).collect();

                for (_, group) in inline_completions_actions_menus.get_actions(None) {
                    for action in group {
                        if let MenuAction::Item(item) = action {
                            extra_actions.push(item);
                        }
                    }
                }

                if !extra_actions.is_empty() {
                    extra_actions.insert(0, Separator::new());
                }

                tool_bar.set_additional_secondary_actions(extra_actions);
            }
        }));

        this
    }

    /// `getDomNode()`: o que o pintor desenha (ver as notas do módulo).
    pub fn visual(&self) -> HintsWidgetVisual {
        HintsWidgetVisual {
            with_border: self.with_border,
            warning_message: Rc::clone(&self.warning_message_content_node),
            warning_message_visible: Rc::clone(&self.warning_message_visible),
            tool_bar: Rc::clone(&self.tool_bar),
        }
    }

    /// `previousAction`.
    pub fn previous_action(&self) -> &Rc<Action> {
        &self.previous_action
    }

    /// `availableSuggestionCountAction`.
    pub fn available_suggestion_count_action(&self) -> &Rc<Action> {
        &self.available_suggestion_count_action
    }

    /// `nextAction`.
    pub fn next_action(&self) -> &Rc<Action> {
        &self.next_action
    }
}

impl IContentWidget for InlineSuggestionHintsContentWidget {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_position(&self) -> Option<ContentWidgetPosition> {
        Some(ContentWidgetPosition {
            position: self.position.get(),
            secondary_position: None,
            preference: vec![ContentWidgetPositionPreference::Above, ContentWidgetPositionPreference::Below],
            position_affinity: Some(PositionAffinity::LeftOfInjectedText),
        })
    }

    /// `allowEditorOverflow = true`.
    fn allow_editor_overflow(&self) -> bool {
        true
    }

    /// `suppressMouseDown = false`.
    fn suppress_mouse_down(&self) -> bool {
        false
    }
}

impl IDisposable for InlineSuggestionHintsContentWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ActionViewItemWithClassName`: um `ActionViewItem` que adiciona uma classe ao contêiner e não tem tooltip
/// (`updateTooltip` não faz nada). O pintor aplica [`ActionViewItemWithClassName::class_name`] no `render`.
pub struct ActionViewItemWithClassName {
    pub action: ActionRef,
    /// `label` das opções do `ActionViewItem`.
    pub show_label: bool,
    /// `icon` das opções do `ActionViewItem`.
    pub show_icon: bool,
    class_name: RefCell<Option<String>>,
}

impl ActionViewItemWithClassName {
    /// `new ActionViewItemWithClassName(undefined, action, { label, icon })`.
    pub fn new(action: ActionRef, show_label: bool, show_icon: bool) -> Rc<Self> {
        Rc::new(Self { action, show_label, show_icon, class_name: RefCell::new(None) })
    }

    /// `setClass(className)`.
    pub fn set_class(&self, class_name: Option<String>) {
        *self.class_name.borrow_mut() = class_name;
    }

    /// A classe que o `render` põe no contêiner.
    pub fn class_name(&self) -> Option<String> {
        self.class_name.borrow().clone()
    }

    /// `updateTooltip()` não faz nada: sem tooltip.
    pub fn shows_tooltip(&self) -> bool {
        false
    }
}

/// O que `StatusBarViewItem.updateLabel` desenha.
#[derive(Clone)]
pub enum StatusBarItemLabel {
    /// Sem atalho: `super.updateLabel()`, o rótulo padrão do `MenuEntryActionViewItem`.
    Default,
    /// Com atalho: o rótulo da ação seguido de `div.keybinding` com o `KeybindingLabel` do atalho
    /// ([`STATUS_BAR_KEYBINDING_DISABLE_TITLE`] e as `unthemedKeybindingLabelOptions`); o rótulo leva a classe
    /// [`STATUS_BAR_LABEL_CLASS_NAME`].
    WithKeybinding { text: String, keybinding: Rc<dyn ResolvedKeybinding>, label_class_name: &'static str },
}

/// `StatusBarViewItem` (um `MenuEntryActionViewItem`): mostra o atalho do comando ao lado do rótulo e não tem tooltip.
pub struct StatusBarViewItem {
    pub action: ActionRef,
    keybinding_service: Rc<dyn IKeybindingService>,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl StatusBarViewItem {
    /// `new StatusBarViewItem(action, undefined)`.
    pub fn new(action: ActionRef, keybinding_service: Rc<dyn IKeybindingService>, context_key_service: Rc<dyn IContextKeyService>) -> Rc<Self> {
        Rc::new(Self { action, keybinding_service, context_key_service })
    }

    /// `updateLabel()`: relê o atalho (`lookupKeybinding(id, contextKeyService, true)`).
    pub fn label(&self) -> StatusBarItemLabel {
        match self.keybinding_service.lookup_keybinding(&self.action.id(), Some(&*self.context_key_service), true) {
            None => StatusBarItemLabel::Default,
            Some(keybinding) => StatusBarItemLabel::WithKeybinding { text: self.action.label(), keybinding, label_class_name: STATUS_BAR_LABEL_CLASS_NAME },
        }
    }

    /// `updateTooltip()` não faz nada: sem tooltip.
    pub fn shows_tooltip(&self) -> bool {
        false
    }
}

/// O item de view que o `actionViewItemProvider` devolve para uma ação (`undefined` é o item padrão da barra).
#[derive(Clone)]
pub enum ToolBarViewItem {
    StatusBar(Rc<StatusBarViewItem>),
    ClassNamed(Rc<ActionViewItemWithClassName>),
}

/// `actionViewItemProvider`: o item de view da ação, ou `None` para o padrão da barra.
pub type ActionViewItemProvider = Rc<dyn Fn(&ActionRef) -> Option<ToolBarViewItem>>;

/// As opções do `CustomizedMenuWorkbenchToolBar` (`IMenuWorkbenchToolBarOptions`, só o que o hints widget passa).
#[derive(Clone)]
pub struct CustomizedMenuWorkbenchToolBarOptions {
    /// `menuOptions`.
    pub menu_options: Option<MenuActionOptions>,
    /// `toolbarOptions.primaryGroup` (a função; o padrão do original é o grupo `navigation`).
    pub primary_group: Rc<dyn Fn(&str) -> bool>,
    /// `toolbarOptions.useSeparatorsInPrimaryActions`.
    pub use_separators_in_primary_actions: bool,
    /// `actionViewItemProvider`.
    pub action_view_item_provider: Option<ActionViewItemProvider>,
    /// `telemetrySource`.
    pub telemetry_source: Option<String>,
}

/// As ações da barra depois do `setActions(primary, secondary)`.
#[derive(Clone, Default)]
pub struct ToolBarActions {
    pub primary: Vec<ActionRef>,
    pub secondary: Vec<ActionRef>,
}

/// `equals(a, b, (x, y) => x === y)` entre duas listas de ações.
fn same_actions(a: &[ActionRef], b: &[ActionRef]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_action(x, y))
}

/// `CustomizedMenuWorkbenchToolBar`: o modelo de uma barra movida pelo menu `menuId` mais ações prefixadas, adicionais
/// primárias e adicionais secundárias (ver as notas do módulo para o que fica com o pintor).
pub struct CustomizedMenuWorkbenchToolBar {
    base: Rc<Disposable>,
    menu_id: MenuId,
    options: CustomizedMenuWorkbenchToolBarOptions,
    menu: Rc<dyn IMenu>,
    additional_actions: RefCell<Vec<ActionRef>>,
    prepended_primary_actions: RefCell<Vec<ActionRef>>,
    additional_primary_actions: RefCell<Vec<ActionRef>>,
    actions: Rc<ObservableValue<Rc<ToolBarActions>, ()>>,
    on_did_change_dropdown_visibility: Rc<Emitter<bool>>,
}

impl CustomizedMenuWorkbenchToolBar {
    /// `new CustomizedMenuWorkbenchToolBar(container, menuId, options2, menuService, contextKeyService, ...)`.
    pub fn new(
        menu_id: MenuId,
        options: CustomizedMenuWorkbenchToolBarOptions,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let menu = menu_service.create_menu(&menu_id, context_key_service, Some(MenuCreateOptions { emit_events_for_submenu_changes: true, event_debounce_delay: None }));
        base.register(menu.clone());
        let on_did_change_dropdown_visibility = Emitter::new();
        base.register(on_did_change_dropdown_visibility.clone());

        let this = Rc::new(Self {
            base,
            menu_id,
            options,
            menu: Rc::clone(&menu),
            additional_actions: RefCell::new(Vec::new()),
            prepended_primary_actions: RefCell::new(Vec::new()),
            additional_primary_actions: RefCell::new(Vec::new()),
            actions: observable_value::<Rc<ToolBarActions>, ()>("toolBarActions", Rc::new(ToolBarActions::default())),
            on_did_change_dropdown_visibility,
        });

        let weak = Rc::downgrade(&this);
        this.base.register(menu.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_toolbar();
            }
        }));
        this.update_toolbar();
        this
    }

    /// `menuId` (e o `resetMenu: menuId` do `super`).
    pub fn menu_id(&self) -> &MenuId {
        &self.menu_id
    }

    /// As opções com que a barra foi criada (o provedor de view items, o grupo primário e o `telemetrySource`).
    pub fn options(&self) -> &CustomizedMenuWorkbenchToolBarOptions {
        &self.options
    }

    /// O resultado de `setActions(primary, secondary)`: muda a cada atualização.
    pub fn actions(&self) -> IObservable<Rc<ToolBarActions>> {
        self.actions.to_observable()
    }

    /// `onDidChangeDropdownVisibility`.
    pub fn on_did_change_dropdown_visibility(&self) -> Event<bool> {
        self.on_did_change_dropdown_visibility.event()
    }

    /// O pintor avisa que o menu secundário abriu (`true`) ou fechou (`false`).
    pub fn set_dropdown_visible(&self, visible: bool) {
        self.on_did_change_dropdown_visibility.fire(visible);
    }

    /// `updateToolbar()`.
    fn update_toolbar(&self) {
        let PrimaryAndSecondaryActions { mut primary, mut secondary } = get_action_bar_actions(
            &self.menu.get_actions(self.options.menu_options.as_ref()),
            &*self.options.primary_group,
            self.options.use_separators_in_primary_actions,
        );

        secondary.extend(self.additional_actions.borrow().iter().cloned());
        // `primary.unshift(...prepended)` e `primary.push(...additionalPrimary)`.
        let mut merged_primary: Vec<ActionRef> = self.prepended_primary_actions.borrow().clone();
        merged_primary.append(&mut primary);
        merged_primary.extend(self.additional_primary_actions.borrow().iter().cloned());
        self.actions.set(Rc::new(ToolBarActions { primary: merged_primary, secondary }), None);
    }

    /// O corpo comum dos três setters: não faz nada com a mesma lista (por identidade das ações).
    fn replace_actions(&self, slot: &RefCell<Vec<ActionRef>>, actions: Vec<ActionRef>) {
        if same_actions(&slot.borrow(), &actions) {
            return;
        }

        *slot.borrow_mut() = actions;
        self.update_toolbar();
    }

    /// `setPrependedPrimaryActions(actions)`.
    pub fn set_prepended_primary_actions(&self, actions: Vec<ActionRef>) {
        self.replace_actions(&self.prepended_primary_actions, actions);
    }

    /// `setAdditionalPrimaryActions(actions)`.
    pub fn set_additional_primary_actions(&self, actions: Vec<ActionRef>) {
        self.replace_actions(&self.additional_primary_actions, actions);
    }

    /// `setAdditionalSecondaryActions(actions)`.
    pub fn set_additional_secondary_actions(&self, actions: Vec<ActionRef>) {
        self.replace_actions(&self.additional_actions, actions);
    }
}

impl IDisposable for CustomizedMenuWorkbenchToolBar {
    fn dispose(&self) {
        self.base.dispose();
    }
}
