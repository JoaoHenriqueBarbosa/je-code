//! Porte de `src/vs/workbench/browser/parts/notifications/notificationsViewer.ts` (e das regras de
//! `media/notificationsList.css` que o viewer usa): a altura global de uma linha de notificação
//! (`DEFAULT_NOTIFICATION_ROW_HEIGHT`, `COMPACT_NOTIFICATION_ROW_HEIGHT`, `setNotificationRowHeight` e
//! `onDidChangeNotificationRowHeight`, que o Modern UI troca), o [`NotificationsListDelegate`], o
//! [`NotificationRenderer`] (o `IListRenderer` da lista de [`je_widgets`]) e o [`NotificationTemplateRenderer`].
//!
//! Notas de tradução:
//! - **DOM e CSS viram árvore do `je-ui` com layout calculado.** O `flex` de `.notification-list-item` (coluna
//!   invertida: linha de detalhes embaixo, linha principal em cima, `padding: 10px 5px`), da linha principal (ícone
//!   de 16px com margens de 4px, mensagem que cresce, barra de ações de 22px) e da linha de detalhes (fonte à
//!   esquerda, botões à direita, `padding-left: 5px`) está em [`NotificationTemplateRenderer::layout`], com as
//!   mesmas medidas do CSS. A barra de progresso fica no fim da linha (`bottom: 0`), como no original.
//! - **`offsetHelper` do delegate.** O elemento escondido que o original mede (`offsetHeight`) é a quebra de
//!   linha da mensagem por palavra (`word-wrap: break-word`, `line-height: 22px`) na largura calculada: aqui é
//!   [`wrap_message`], que mede com o mesmo sistema de texto da UI. O construtor não recebe o contêiner.
//! - **`NotificationMessageRenderer`** devolve nós de texto por trecho (um `a` por link, com o clique, o Enter e o
//!   Space abrindo o `href`), posicionados por [`wrap_message`]. O `title` do link é o rótulo de acessibilidade do
//!   nó. O foco por `tabIndex: 0` do link não existe no `je-ui` (o nó não é focável por Tab).
//! - **As classes de ação** (`ClearNotificationAction`, `ExpandNotificationAction`, `CollapseNotificationAction`,
//!   `ConfigureNotificationAction`) dependem do `ICommandService` e entram com o centro de notificações; o viewer
//!   as recebe pelo contrato [`NotificationActionProvider`], com o que o original lê delas (o `id`, o rótulo, a
//!   classe e, no `ConfigureNotificationAction`, a notificação).
//! - **`@IInstantiationService`/`@IContextMenuService`...** são os serviços de [`NotificationRendererServices`],
//!   passados à mão. Os dois provedores de menu de contexto existem porque o `ButtonWithDropdown` e o
//!   `DropdownMenuActionViewItem` do `je-widgets` usam contratos diferentes.
//! - **`:focus-within` do `.monaco-list`** (mostrar a barra de ações de todas as linhas quando a lista tem o foco)
//!   fica no `focused` da linha e no ponteiro sobre ela: o renderer não enxerga o nó da lista.
//! - **Estado global.** O `let notificationRowHeight` do módulo é um estado por thread, como os outros globais do
//!   porte (`set_global_pane_header_size`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{Action, ActionRef, ActionRunner, IAction, IActionRunner, Separator, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::linked_text::LinkedTextNode;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::notification_telemetry::{NotificationActionTelemetry, NotificationActionTelemetryId, with_notification_action_telemetry};
use je_platform::notification::{INotificationService, NotificationFilterSetting, NotificationSource, NotificationSourceFilter, NotificationsFilter, Severity};
use je_platform::opener::opener::{AllowCommands, IOpenerService, OpenOptions};
use je_platform::telemetry::ITelemetryService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, NodeId, PointerButton, Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::button::{ButtonBar, ButtonDropdownActions, ButtonWithDropdownOptions, IContextMenuProvider as IButtonContextMenuProvider};
use je_widgets::button::{ButtonLabel, ButtonOptions, ButtonStyles, ButtonTitle, IButton};
use je_widgets::context_view_api::IContextMenuProvider;
use je_widgets::dom::{StandardKeyboardEvent, clear_node, create_el};
use je_widgets::dropdown::IActionProvider;
use je_widgets::dropdown_action_view_item::{DropdownMenuActionViewItem, IDropdownMenuActionViewItemOptions, MenuActions};
use je_widgets::hover_api::{HoverPlacement, IManagedHover, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::list::list_types::{IListRenderer, IListVirtualDelegate, ListElementRenderDetails, TemplateData};
use je_widgets::progress_bar::{IProgressBarStyles, ProgressBar};
use je_widgets::text::{UI_FONT_SIZE, clip_text, measure_text, set_node_text, ui_font};

use crate::browser::parts::notifications::notifications_actions::{get_notification_collapse_icon, get_notification_expand_icon};
use crate::browser::parts::notifications::notifications_commands::{NotificationActionRunner, notification_view_item_context};
use crate::common::notification_telemetry::{NotificationActionRole, NotificationInteraction, NotificationTelemetrySurface, log_notification_interaction};
use crate::common::notifications::{ChoiceAction, NotificationViewItem, NotificationViewItemContentChangeKind, NotificationsSettings, get_notifications_position};

/// Default height (px) of a single notification row.
pub const DEFAULT_NOTIFICATION_ROW_HEIGHT: f64 = 42.0;

/// Compact height (px) of a single notification row.
pub const COMPACT_NOTIFICATION_ROW_HEIGHT: f64 = 34.0;

thread_local! {
    static NOTIFICATION_ROW_HEIGHT: Cell<f64> = const { Cell::new(DEFAULT_NOTIFICATION_ROW_HEIGHT) };
    static ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT: Rc<Emitter<f64>> = Emitter::new();
}

/// `onDidChangeNotificationRowHeight`.
pub fn on_did_change_notification_row_height() -> Event<f64> {
    ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT.with(|emitter| emitter.event())
}

/// Current height (px) of a single notification row.
pub fn notification_row_height() -> f64 {
    NOTIFICATION_ROW_HEIGHT.with(Cell::get)
}

/// Overrides the height (px) of a single notification row and notifies listeners when it changes.
pub fn set_notification_row_height(height: f64) {
    if height != notification_row_height() {
        NOTIFICATION_ROW_HEIGHT.with(|current| current.set(height));
        ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT.with(|emitter| emitter.fire(height));
    }
}

/// `line-height: 22px` da mensagem e do `offsetHelper`.
const LINE_HEIGHT: f64 = 22.0;

/// A largura do contêiner de notificações (o `450` do `computePreferredHeight`).
const NOTIFICATIONS_CONTAINER_WIDTH: f64 = 450.0;

/// O padding horizontal do `.notification-list-item` (`padding: 10px 5px`).
const ITEM_PADDING_X: f64 = 5.0;

/// O padding vertical do `.notification-list-item`.
const ITEM_PADDING_Y: f64 = 10.0;

/// Ícone da severidade: `flex: 0 0 16px`, `margin: 0 4px`, `font-size: 18px`.
const ICON_WIDTH: f64 = 16.0;
const ICON_MARGIN: f64 = 4.0;
const ICON_FONT_SIZE: f32 = 18.0;

/// A largura de um item da barra de ações (`.action-label` de 16px mais `padding: 3px` de cada lado).
const TOOLBAR_ITEM_WIDTH: f64 = 22.0;

/// `font-size: 12px` da fonte (`.notification-list-item-source`).
const SOURCE_FONT_SIZE: f32 = 12.0;

/// As cores que o CSS do viewer lê do tema (não estão no original, que as herda pelo DOM).
#[derive(Clone)]
pub struct NotificationStyles {
    /// `--vscode-notifications-foreground`.
    pub foreground: Rgba,
    /// `--vscode-notificationLink-foreground`.
    pub link_foreground: Rgba,
    /// `--vscode-notificationsInfoIcon-foreground`.
    pub info_icon_foreground: Rgba,
    /// `--vscode-notificationsWarningIcon-foreground`.
    pub warning_icon_foreground: Rgba,
    /// `--vscode-notificationsErrorIcon-foreground`.
    pub error_icon_foreground: Rgba,
    /// `defaultButtonStyles`.
    pub button_styles: ButtonStyles,
    /// `defaultProgressBarStyles`.
    pub progress_bar_styles: IProgressBarStyles,
}

impl NotificationStyles {
    fn severity_color(&self, severity: Severity) -> Rgba {
        match severity {
            Severity::Warning => self.warning_icon_foreground,
            Severity::Error => self.error_icon_foreground,
            _ => self.info_icon_foreground,
        }
    }
}

/// O que o viewer lê das classes de ação do `notificationsActions.ts` (`ClearNotificationAction`,
/// `ExpandNotificationAction`, `CollapseNotificationAction`, `ConfigureNotificationAction`).
pub trait NotificationActionProvider {
    /// `new ClearNotificationAction(ClearNotificationAction.ID, ClearNotificationAction.LABEL)`.
    fn create_close_action(&self) -> ActionRef;
    /// `new ExpandNotificationAction(ExpandNotificationAction.ID, ExpandNotificationAction.LABEL)`.
    fn create_expand_action(&self) -> ActionRef;
    /// `new CollapseNotificationAction(CollapseNotificationAction.ID, CollapseNotificationAction.LABEL)`.
    fn create_collapse_action(&self) -> ActionRef;
    /// `new ConfigureNotificationAction(ConfigureNotificationAction.ID, ConfigureNotificationAction.LABEL, notification)`:
    /// a ação e o handle que a descarta (o `this.inputDisposables.add(configureNotificationAction)` do original).
    fn create_configure_action(&self, notification: &Rc<NotificationViewItem>) -> (ActionRef, DisposableRef);
    /// `action instanceof ConfigureNotificationAction ? action.notification : undefined`.
    fn configure_action_notification(&self, action: &ActionRef) -> Option<Rc<NotificationViewItem>>;
    /// `new CopyNotificationMessageAction(CopyNotificationMessageAction.ID, CopyNotificationMessageAction.LABEL)`
    /// (o menu de contexto da lista, em `notificationsList.ts`).
    fn create_copy_message_action(&self) -> ActionRef;
}

/// Os serviços que o `NotificationRenderer` e o `NotificationTemplateRenderer` recebem por injeção no original.
#[derive(Clone)]
pub struct NotificationRendererServices {
    pub ui: Ui,
    pub opener_service: Rc<dyn IOpenerService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub notification_service: Rc<dyn INotificationService>,
    /// O `IContextMenuService` do `DropdownMenuActionViewItem` (a ação de configurar).
    pub dropdown_context_menu: Rc<dyn IContextMenuProvider>,
    /// O `IContextMenuService` do `ButtonWithDropdown` (os botões com menu).
    pub button_context_menu: Rc<dyn IButtonContextMenuProvider>,
    pub actions: Rc<dyn NotificationActionProvider>,
    pub styles: NotificationStyles,
}

/// Um trecho de texto da mensagem já posicionado: o nó de origem (índice em `linked_text.nodes`), o texto, o `x`
/// (px, em relação ao início da mensagem), a linha e a largura medida.
#[derive(Clone, Debug, PartialEq)]
pub struct MessagePiece {
    pub node: usize,
    pub text: String,
    pub x: f64,
    pub line: usize,
    pub width: f64,
}

/// O resultado de [`wrap_message`]: os trechos, o número de linhas e a largura da linha mais larga (o
/// `scrollWidth` quando não há quebra).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WrappedMessage {
    pub pieces: Vec<MessagePiece>,
    pub lines: usize,
    pub content_width: f64,
}

/// Os textos dos nós de uma mensagem (`linkedText.nodes`), com o rótulo dos links.
fn message_texts(notification: &NotificationViewItem) -> Vec<String> {
    notification
        .message()
        .linked_text
        .nodes
        .iter()
        .map(|node| match node {
            LinkedTextNode::Text(text) => text.to_string_lossy(),
            LinkedTextNode::Link(link) => link.label.to_string_lossy(),
        })
        .collect()
}

/// O texto da mensagem na fonte da UI (13px), como o `.notification-list-item-message` o desenha.
fn message_font() -> je_text::TextStyle {
    ui_font(UI_FONT_SIZE, 400)
}

/// Quebra a mensagem em linhas de `width` px por palavra (`word-wrap: break-word`: a palavra maior que a linha
/// quebra por caractere) quando `wrap` é verdadeiro; sem quebra (`white-space: nowrap`, a linha única da
/// notificação recolhida) todos os trechos ficam na linha 0.
pub fn wrap_message(texts: &[String], width: f64, wrap: bool) -> WrappedMessage {
    let font = message_font();
    let mut out = WrappedMessage::default();
    let (mut x, mut line) = (0.0_f64, 0_usize);
    let mut any = false;

    for (node, text) in texts.iter().enumerate() {
        for word in text.split_inclusive(' ') {
            let trimmed = word.trim_end_matches(' ');
            let trimmed_width = measure_text(trimmed, &font);
            let full_width = measure_text(word, &font);
            if wrap && x > 0.0 && x + trimmed_width > width {
                out.content_width = out.content_width.max(x);
                line += 1;
                x = 0.0;
            }
            let chunks: Vec<String> = if wrap && trimmed_width > width && width > 0.0 {
                // A palavra maior que a linha quebra por caractere; o primeiro trecho parte do `x` atual e os
                // seguintes do início da linha.
                let mut chunks = Vec::new();
                let mut current = String::new();
                let mut start = x;
                for ch in word.chars() {
                    let mut candidate = current.clone();
                    candidate.push(ch);
                    if !current.is_empty() && start + measure_text(&candidate, &font) > width {
                        chunks.push(std::mem::take(&mut current));
                        start = 0.0;
                    }
                    current.push(ch);
                }
                if !current.is_empty() {
                    chunks.push(current);
                }
                chunks
            } else {
                vec![word.to_owned()]
            };
            let last = chunks.len().saturating_sub(1);
            for (index, chunk) in chunks.into_iter().enumerate() {
                let chunk_width = if last == 0 { full_width } else { measure_text(&chunk, &font) };
                out.pieces.push(MessagePiece { node, text: chunk, x, line, width: chunk_width });
                any = true;
                if index < last {
                    out.content_width = out.content_width.max(x + chunk_width);
                    line += 1;
                    x = 0.0;
                } else {
                    x += chunk_width;
                }
            }
        }
    }
    out.content_width = out.content_width.max(x);
    out.lines = if any { line + 1 } else { 0 };
    out
}

/// `NotificationsListDelegate`.
pub struct NotificationsListDelegate;

impl NotificationsListDelegate {
    pub fn new() -> Self {
        Self
    }

    fn row_height() -> f64 {
        notification_row_height()
    }

    fn compute_preferred_height(notification: &NotificationViewItem) -> f64 {
        // Prepare offset helper depending on toolbar actions count
        let mut actions = 0_usize;
        if !notification.has_active_progress() {
            actions += 1; // close
        }
        if notification.can_collapse() {
            actions += 1; // expand/collapse
        }
        if !notification.actions().secondary.is_empty() {
            actions += 1; // secondary actions
        }
        let width = NOTIFICATIONS_CONTAINER_WIDTH /* notifications container width */
            - (10.0 /* padding */ + 30.0 /* severity icon */ + (actions as f64 * 30.0) /* actions */ - (actions.saturating_sub(1) as f64 * 4.0) /* less padding for actions > 1 */);

        // Render message into offset helper and compute height
        let wrapped = wrap_message(&message_texts(notification), width, true);
        wrapped.lines as f64 * LINE_HEIGHT
    }
}

impl Default for NotificationsListDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl IListVirtualDelegate<Rc<NotificationViewItem>> for NotificationsListDelegate {
    fn get_height(&self, notification: &Rc<NotificationViewItem>) -> f64 {
        if !notification.expanded() {
            return Self::row_height(); // return early if there are no more rows to show
        }

        // First row: message and actions
        let mut expanded_height = Self::row_height();

        // Dynamic height: if message overflows
        let preferred_message_height = Self::compute_preferred_height(notification);
        let message_overflows = LINE_HEIGHT < preferred_message_height;
        if message_overflows {
            let overflow = preferred_message_height - LINE_HEIGHT;
            expanded_height += overflow;
        }

        // Last row: source and buttons if we have any
        if notification.source().is_some() || !notification.actions().primary.is_empty() {
            expanded_height += Self::row_height();
        }

        // If the expanded height is same as collapsed, unset the expanded state
        // but skip events because there is no change that has visual impact
        if expanded_height == Self::row_height() {
            notification.collapse(true /* skip events, no change in height */);
        }

        expanded_height
    }

    fn get_template_id(&self, _element: &Rc<NotificationViewItem>) -> String {
        NotificationRenderer::TEMPLATE_ID.to_owned()
    }
}

/// `IMessageActionHandler`.
pub struct MessageActionHandler {
    pub to_dispose: Rc<DisposableStore>,
    pub callback: Rc<dyn Fn(&str)>,
}

/// `NotificationMessageRenderer`: monta o texto da mensagem como nós filhos de `container` (um por trecho; um `a`
/// por link) e devolve as linhas e a largura que ocupam.
pub struct NotificationMessageRenderer;

impl NotificationMessageRenderer {
    pub fn render(ui: &Ui, container: NodeId, notification: &NotificationViewItem, styles: &NotificationStyles, width: f64, wrap: bool, action_handler: Option<&MessageActionHandler>) -> WrappedMessage {
        let texts = message_texts(notification);
        let wrapped = wrap_message(&texts, width, wrap);
        let message = notification.message();
        let font = message_font();

        let mut tree = ui.tree_mut();
        let mut start = 0;
        while start < wrapped.pieces.len() {
            // Os trechos de um mesmo nó do texto são seguidos: um link vira um único `a`, mesmo quebrado em várias palavras.
            let node_index = wrapped.pieces[start].node;
            let end = wrapped.pieces[start..].iter().position(|piece| piece.node != node_index).map_or(wrapped.pieces.len(), |offset| start + offset);
            let link = match &message.linked_text.nodes[node_index] {
                LinkedTextNode::Link(link) => Some(link.clone()),
                LinkedTextNode::Text(_) => None,
            };
            let color = if link.is_some() { styles.link_foreground } else { styles.foreground };

            let placed: Vec<(String, Rect)> = wrapped.pieces[start..end]
                .iter()
                .map(|piece| {
                    let top = piece.line as f64 * LINE_HEIGHT;
                    let mut text = piece.text.clone();
                    let mut piece_width = piece.width;
                    if !wrap && piece.x + piece_width > width {
                        // `text-overflow: ellipsis` na linha única recolhida
                        text = clip_text(&text, &font, (width - piece.x).max(0.0));
                        piece_width = (width - piece.x).max(0.0);
                    }
                    (text, Rect::new(piece.x, top, piece.x + piece_width, top + LINE_HEIGHT))
                })
                .collect();

            let (parent, origin) = match &link {
                Some(link) => {
                    let union = placed.iter().skip(1).fold(placed[0].1, |acc, (_, rect)| acc.union(*rect));
                    let anchor = create_el(&mut tree, container, Role::Link, "");
                    tree.set_bounds(anchor, union);

                    let href = link.href.to_string_lossy();
                    let mut title = link.title.as_ref().map(|t| t.to_string_lossy()).unwrap_or_default();
                    if title.is_empty() && href.starts_with("command:") {
                        title = localize("executeCommand", "Click to execute command '{0}'", &[&href["command:".len()..]]);
                    } else if title.is_empty() {
                        title = href.clone();
                    }
                    tree.set_label(anchor, title);

                    if let Some(handler) = action_handler {
                        let open = {
                            let callback = handler.callback.clone();
                            let href = href.clone();
                            move |e: &mut je_ui::Event| {
                                e.stop(true);
                                callback(&href);
                            }
                        };
                        handler.to_dispose.add(ui.listen(anchor, EventKind::Click, false, open.clone()));
                        handler.to_dispose.add(ui.listen(anchor, EventKind::KeyDown, false, move |e| {
                            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
                            if event.equals(KeyCode::Space.bits()) || event.equals(KeyCode::Enter.bits()) {
                                open(e);
                            }
                        }));
                    }
                    (anchor, (union.x0, union.y0))
                }
                None => (container, (0.0, 0.0)),
            };

            for (text, rect) in placed {
                let node = create_el(&mut tree, parent, Role::Generic, "");
                set_node_text(&mut tree, node, &text, &font, color, LINE_HEIGHT);
                tree.set_bounds(node, Rect::new(rect.x0 - origin.0, rect.y0 - origin.1, rect.x1 - origin.0, rect.y1 - origin.1));
            }
            start = end;
        }
        wrapped
    }
}

/// `INotificationTemplateData`.
pub struct NotificationTemplateData {
    pub container: NodeId,
    pub to_dispose: Rc<DisposableStore>,

    pub item: NodeId,
    pub main_row: NodeId,
    pub icon: NodeId,
    pub message: NodeId,
    pub toolbar_container: NodeId,
    pub toolbar: Rc<ActionBar>,

    pub details_row: NodeId,
    pub source: NodeId,
    pub buttons_container: NodeId,
    pub progress: Rc<ProgressBar>,

    pub renderer: Rc<NotificationTemplateRenderer>,
}

/// `NotificationRenderer`.
pub struct NotificationRenderer {
    action_runner: Rc<dyn IActionRunner>,
    telemetry_surface: Option<NotificationTelemetrySurface>,
    services: NotificationRendererServices,
    /// `NotificationTemplateRenderer.closeNotificationAction` e companhia: estáticos do original, um por renderer aqui.
    shared_actions: Rc<SharedActions>,
}

impl NotificationRenderer {
    pub const TEMPLATE_ID: &'static str = "notification";

    /// `new NotificationRenderer(actionRunner)`; `telemetry_surface` é o `actionRunner.telemetrySurface` quando o
    /// runner é um `NotificationActionRunner`.
    pub fn new(action_runner: Rc<dyn IActionRunner>, telemetry_surface: Option<NotificationTelemetrySurface>, services: NotificationRendererServices) -> Rc<Self> {
        let shared_actions = SharedActions::get_or_create(&services);
        Rc::new(Self { action_runner, telemetry_surface, services, shared_actions })
    }

    /// O `NotificationActionRunner` do original passa a superfície; qualquer outro runner não tem.
    pub fn with_notification_action_runner(action_runner: Rc<NotificationActionRunner>, services: NotificationRendererServices) -> Rc<Self> {
        let telemetry_surface = action_runner.telemetry_surface;
        Self::new(action_runner, telemetry_surface, services)
    }

    fn template_of(template_data: &TemplateData) -> Rc<NotificationTemplateData> {
        let any: Rc<dyn Any> = template_data.clone();
        any.downcast::<NotificationTemplateData>().unwrap_or_else(|_| je_base::errors::throw(je_base::errors::Error::new("template de notificação inesperado")))
    }
}

impl IListRenderer<Rc<NotificationViewItem>> for NotificationRenderer {
    fn template_id(&self) -> String {
        Self::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = self.services.ui.clone();
        let to_dispose = DisposableStore::new();

        let (item, details_row, source, buttons_container, main_row, icon, message, toolbar_container) = {
            let mut tree = ui.tree_mut();
            // Container
            let item = create_el(&mut tree, container, Role::Generic, "notification-list-item");
            // the details row appears first in order for better keyboard access to notification buttons
            let details_row = create_el(&mut tree, item, Role::Generic, "notification-list-item-details-row");
            let source = create_el(&mut tree, details_row, Role::Generic, "notification-list-item-source");
            let buttons_container = create_el(&mut tree, details_row, Role::Generic, "notification-list-item-buttons-container");
            // main row
            let main_row = create_el(&mut tree, item, Role::Generic, "notification-list-item-main-row");
            let icon = create_el(&mut tree, main_row, Role::Generic, "notification-list-item-icon codicon");
            let message = create_el(&mut tree, main_row, Role::Generic, "notification-list-item-message");
            tree.style_mut(message).clip = true;
            let toolbar_container = create_el(&mut tree, main_row, Role::Generic, "notification-list-item-toolbar-container");
            tree.style_mut(details_row).clip = true;
            tree.style_mut(buttons_container).clip = true;
            (item, details_row, source, buttons_container, main_row, icon, message, toolbar_container)
        };

        // Toolbar
        let toolbar = {
            let services = self.services.clone();
            let action_runner = self.action_runner.clone();
            let provider = Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| -> Option<Rc<dyn IActionViewItem>> {
                let notification = services.actions.configure_action_notification(action)?;
                let menu_notification = notification.clone();
                let menu_services = services.clone();
                let menu = Rc::new(move || configure_menu_actions(&menu_services, &menu_notification));
                let class_names: Vec<String> = action.class().map(|c| c.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
                Some(DropdownMenuActionViewItem::new(
                    &services.ui,
                    action.clone(),
                    MenuActions::Provider(menu as Rc<dyn IActionProvider>),
                    services.dropdown_context_menu.clone(),
                    IDropdownMenuActionViewItemOptions { base: options.clone(), action_runner: Some(action_runner.clone()), class_names, ..Default::default() },
                ) as Rc<dyn IActionViewItem>)
            });
            ActionBar::new(
                &ui,
                toolbar_container,
                IActionBarOptions {
                    aria_label: Some(localize("notificationActions", "Notification Actions", &[])),
                    action_view_item_provider: Some(provider),
                    action_runner: Some(self.action_runner.clone()),
                    ..Default::default()
                },
            )
        };
        to_dispose.add_rc(toolbar.clone());

        // Progress: below the rows to span the entire width of the item
        let progress = ProgressBar::new(&ui, container, Some(&je_widgets::progress_bar::IProgressBarOptions { styles: self.services.styles.progress_bar_styles.clone(), ..je_widgets::progress_bar::unthemed_progress_bar_options() }));
        to_dispose.add_rc(progress.clone());

        // Renderer
        let renderer = NotificationTemplateRenderer::new(
            NotificationTemplateNodes { container, item, main_row, icon, message, toolbar_container, details_row, source, buttons_container },
            toolbar.clone(),
            progress.clone(),
            self.action_runner.clone(),
            self.telemetry_surface,
            self.services.clone(),
            self.shared_actions.clone(),
        );
        to_dispose.add_rc(renderer.clone());

        Rc::new(NotificationTemplateData { container, to_dispose, item, main_row, icon, message, toolbar_container, toolbar, details_row, source, buttons_container, progress, renderer })
    }

    fn render_element(&self, notification: &Rc<NotificationViewItem>, _index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        Self::template_of(template_data).renderer.set_input(notification);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        Self::template_of(template_data).to_dispose.dispose();
    }
}

/// As ações compartilhadas entre as linhas (os `static` do `NotificationTemplateRenderer`): nascem no primeiro renderer
/// do fio e nunca são descartadas, como os `static` do original.
struct SharedActions {
    close: ActionRef,
    expand: ActionRef,
    collapse: ActionRef,
}

thread_local! {
    static SHARED_ACTIONS: RefCell<Option<Rc<SharedActions>>> = const { RefCell::new(None) };
}

impl SharedActions {
    /// `if (!NotificationTemplateRenderer.closeNotificationAction) { ...create...; updateExpandCollapseIcons(...) }`.
    fn get_or_create(services: &NotificationRendererServices) -> Rc<Self> {
        if let Some(shared) = SHARED_ACTIONS.with(|cell| cell.borrow().clone()) {
            return shared;
        }
        let shared = Rc::new(Self {
            close: services.actions.create_close_action(),
            expand: services.actions.create_expand_action(),
            collapse: services.actions.create_collapse_action(),
        });
        SHARED_ACTIONS.with(|cell| *cell.borrow_mut() = Some(shared.clone()));
        shared.update_expand_collapse_icons(&*services.configuration_service);
        shared
    }

    fn update_expand_collapse_icons(&self, configuration_service: &dyn IConfigurationService) {
        let position = get_notifications_position(configuration_service);
        self.expand.set_class(Some(ThemeIcon::as_class_name(&get_notification_expand_icon(position).to_theme_icon())));
        self.collapse.set_class(Some(ThemeIcon::as_class_name(&get_notification_collapse_icon(position).to_theme_icon())));
    }
}

/// O `getActions()` do `DropdownMenuActionViewItem` da ação de configurar.
fn configure_menu_actions(services: &NotificationRendererServices, notification: &Rc<NotificationViewItem>) -> Vec<ActionRef> {
    let mut actions: Vec<ActionRef> = Vec::new();

    if let (Some(id), Some(label)) = (notification.source_id(), notification.source()) {
        let source = NotificationSource { id, label };
        let is_source_filtered = services.notification_service.get_filter(Some(&source)) == NotificationsFilter::Error;
        let notification_service = services.notification_service.clone();
        let run_source = source.clone();
        actions.push(with_notification_action_telemetry(
            to_action(ToActionProps {
                id: source.id.clone(),
                label: if is_source_filtered {
                    localize("turnOnNotifications", "Turn On All Notifications from '{0}'", &[&source.label])
                } else {
                    localize("turnOffNotifications", "Turn Off Info and Warning Notifications from '{0}'", &[&source.label])
                },
                tooltip: None,
                enabled: None,
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    notification_service.set_filter(NotificationFilterSetting::Source(NotificationSourceFilter {
                        source: run_source.clone(),
                        filter: if is_source_filtered { NotificationsFilter::Off } else { NotificationsFilter::Error },
                    }));
                    MaybePromise::from(Dynamic::Undefined)
                }),
            }),
            Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Configure)),
        ));

        if !notification.actions().secondary.is_empty() {
            actions.push(Separator::new());
        }
    }

    actions.extend(notification.actions().secondary);
    actions
}

/// Os nós que o `renderTemplate` cria e o [`NotificationTemplateRenderer`] preenche.
#[derive(Clone, Copy)]
pub struct NotificationTemplateNodes {
    pub container: NodeId,
    pub item: NodeId,
    pub main_row: NodeId,
    pub icon: NodeId,
    pub message: NodeId,
    pub toolbar_container: NodeId,
    pub details_row: NodeId,
    pub source: NodeId,
    pub buttons_container: NodeId,
}

/// `NotificationTemplateRenderer`.
pub struct NotificationTemplateRenderer {
    base: Rc<Disposable>,
    weak_self: std::rc::Weak<NotificationTemplateRenderer>,
    nodes: NotificationTemplateNodes,
    toolbar: Rc<ActionBar>,
    progress: Rc<ProgressBar>,
    action_runner: Rc<dyn IActionRunner>,
    telemetry_surface: Option<NotificationTelemetrySurface>,
    services: NotificationRendererServices,
    shared: Rc<SharedActions>,
    input_disposables: Rc<DisposableStore>,
    message_disposables: Rc<DisposableStore>,
    current: RefCell<Option<Rc<NotificationViewItem>>>,
    hovered: Cell<bool>,
    message_overflows: Cell<bool>,
    toolbar_actions: Cell<usize>,
}

impl NotificationTemplateRenderer {
    fn new(
        nodes: NotificationTemplateNodes,
        toolbar: Rc<ActionBar>,
        progress: Rc<ProgressBar>,
        action_runner: Rc<dyn IActionRunner>,
        telemetry_surface: Option<NotificationTelemetrySurface>,
        services: NotificationRendererServices,
        shared: Rc<SharedActions>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let input_disposables = DisposableStore::new();
        let message_disposables = DisposableStore::new();
        base.register(input_disposables.clone());
        base.register(message_disposables.clone());

        let this = Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            nodes,
            toolbar,
            progress,
            action_runner,
            telemetry_surface,
            services,
            shared,
            input_disposables,
            message_disposables,
            current: RefCell::new(None),
            hovered: Cell::new(false),
            message_overflows: Cell::new(false),
            toolbar_actions: Cell::new(0),
        });

        // O `:hover` do `.notification-list-item` mostra a barra de ações.
        for (kind, hovered) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
            let weak = this.weak_self.clone();
            this.base.register(this.services.ui.listen(nodes.item, kind, false, move |_| {
                if let Some(renderer) = weak.upgrade()
                    && renderer.hovered.replace(hovered) != hovered
                {
                    renderer.layout();
                }
            }));
        }
        {
            let shared = this.shared.clone();
            let configuration_service = this.services.configuration_service.clone();
            this.base.register(this.services.configuration_service.on_did_change_configuration().listen(move |event| {
                if event.affects_configuration(NotificationsSettings::NotificationsPosition.as_str(), None) {
                    shared.update_expand_collapse_icons(&*configuration_service);
                }
            }));
        }
        this
    }

    /// `setInput`.
    pub fn set_input(&self, notification: &Rc<NotificationViewItem>) {
        self.input_disposables.clear();
        self.message_disposables.clear();
        *self.current.borrow_mut() = Some(notification.clone());
        self.hovered.set(false);

        self.render(notification);
    }

    fn log_interaction(&self, notification: &Rc<NotificationViewItem>, interaction: NotificationInteraction) {
        log_notification_interaction(&*self.services.telemetry_service, notification, interaction, self.telemetry_surface, None, NotificationActionRole::None);
    }

    fn render(&self, notification: &Rc<NotificationViewItem>) {
        let ui = self.services.ui.clone();

        // Container
        ui.tree_mut().toggle_class(self.nodes.item, "expanded", notification.expanded());
        {
            let notification = notification.clone();
            let weak = self.weak_self.clone();
            // O `mouseup` do botão do meio é parado (#109322, o `paste` do editor) e o `auxclick` fecha a notificação.
            self.input_disposables.add(ui.listen(self.nodes.item, EventKind::PointerUp, false, move |e| {
                if e.pointer.is_some_and(|p| p.button == PointerButton::Auxiliary) {
                    e.stop(true);
                    if !notification.has_active_progress()
                        && let Some(renderer) = weak.upgrade()
                    {
                        renderer.log_interaction(&notification, NotificationInteraction::Dismiss);
                        notification.close();
                    }
                }
            }));
        }

        // Severity Icon
        self.render_severity(notification);

        // Message
        let message_hover = self.setup_hover(self.nodes.message);
        let message_overflows = self.render_message(notification, &message_hover);

        // Secondary Actions
        self.render_secondary_actions(notification, message_overflows);

        // Source
        let source_hover = self.setup_hover(self.nodes.source);
        self.render_source(notification, &source_hover);

        // Buttons
        self.render_buttons(notification);

        // Progress
        self.render_progress(notification);

        // Label Change Events that we can handle directly
        // (changes to actions require an entire redraw of
        // the notification because it has an impact on
        // epxansion state)
        {
            let weak = self.weak_self.clone();
            let item = notification.clone();
            self.input_disposables.add(notification.on_did_change_content().listen(move |event| {
                let Some(renderer) = weak.upgrade() else { return };
                match event.kind {
                    NotificationViewItemContentChangeKind::Severity => renderer.render_severity(&item),
                    NotificationViewItemContentChangeKind::Progress => renderer.render_progress(&item),
                    NotificationViewItemContentChangeKind::Message => {
                        renderer.render_message(&item, &message_hover);
                    }
                    NotificationViewItemContentChangeKind::Actions => {}
                }
            }));
        }
        self.layout();
    }

    /// `hoverService.setupManagedHover(getDefaultHoverDelegate('mouse'), element, '')`.
    fn setup_hover(&self, node: NodeId) -> Rc<dyn IManagedHover> {
        let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), node, "".into(), None);
        let disposable = hover.clone();
        self.input_disposables.add(to_disposable(move || disposable.dispose()));
        hover
    }

    fn render_severity(&self, notification: &NotificationViewItem) {
        // first remove, then set as the codicon class names overlap
        let severity = notification.severity();
        let mut classes = vec!["notification-list-item-icon".to_owned(), "codicon".to_owned()];
        classes.extend(ThemeIcon::as_class_name_array(&Self::severity_icon(severity)));
        let mut tree = self.services.ui.tree_mut();
        tree.set_class_name(self.nodes.icon, &classes.join(" "));
        tree.style_mut(self.nodes.icon).content = Content::Codicon { id: Self::severity_icon(severity).id, size: ICON_FONT_SIZE, color: self.services.styles.severity_color(severity) };
    }

    /// A largura da mensagem sem a barra de ações (`display: none` fora do hover e do foco).
    fn message_width(&self, with_toolbar: bool) -> f64 {
        let bounds = self.services.ui.tree().bounds(self.nodes.container);
        let row_width = if bounds.width() > 0.0 { bounds.width() } else { NOTIFICATIONS_CONTAINER_WIDTH };
        let toolbar = if with_toolbar { self.toolbar_actions.get() as f64 * TOOLBAR_ITEM_WIDTH } else { 0.0 };
        (row_width - 2.0 * ITEM_PADDING_X - (ICON_MARGIN + ICON_WIDTH + ICON_MARGIN) - toolbar).max(0.0)
    }

    fn toolbar_visible(&self, notification: &NotificationViewItem) -> bool {
        notification.expanded() || self.hovered.get() || self.services.ui.tree().has_class(self.nodes.container, "focused")
    }

    /// `renderMessage`: devolve se a mensagem transborda (só vale recolhida).
    fn render_message(&self, notification: &Rc<NotificationViewItem>, custom_hover: &Rc<dyn IManagedHover>) -> bool {
        self.message_disposables.clear();
        clear_node(&mut self.services.ui.tree_mut(), self.nodes.message);

        let wrap = notification.expanded();
        let width = self.message_width(self.toolbar_visible(notification) && self.toolbar_actions.get() > 0);
        let weak = self.weak_self.clone();
        let item = notification.clone();
        let handler = MessageActionHandler {
            to_dispose: self.message_disposables.clone(),
            callback: Rc::new(move |link: &str| {
                let Some(renderer) = weak.upgrade() else { return };
                renderer.log_interaction(&item, NotificationInteraction::Link);
                let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..Default::default() };
                let _ = renderer.services.opener_service.open(&URI::parse(link, false).into(), Some(&options));
            }),
        };
        let wrapped = NotificationMessageRenderer::render(&self.services.ui, self.nodes.message, notification, &self.services.styles, width, wrap, Some(&handler));

        // `messageOverflows = canCollapse && !expanded && scrollWidth > clientWidth`, medido sem a barra de ações
        let full_width = self.message_width(false);
        let message_overflows = notification.can_collapse() && !notification.expanded() && wrapped.content_width > full_width;
        self.message_overflows.set(message_overflows);

        custom_hover.update(if message_overflows { message_texts(notification).concat().as_str().into() } else { "".into() }, None);

        message_overflows
    }

    fn render_secondary_actions(&self, notification: &Rc<NotificationViewItem>, message_overflows: bool) {
        let mut actions: Vec<ActionRef> = Vec::new();

        // Secondary Actions
        if !notification.actions().secondary.is_empty() {
            let (configure_notification_action, disposable) = self.services.actions.create_configure_action(notification);
            actions.push(configure_notification_action);
            self.input_disposables.add(disposable);
        }

        // Expand / Collapse
        let mut show_expand_collapse_action = false;
        if notification.can_collapse() {
            if notification.expanded() {
                show_expand_collapse_action = true; // allow to collapse an expanded message
            } else if notification.source().is_some() {
                show_expand_collapse_action = true; // allow to expand to details row
            } else if message_overflows {
                show_expand_collapse_action = true; // allow to expand if message overflows
            }
        }

        if show_expand_collapse_action {
            actions.push(if notification.expanded() { self.shared.collapse.clone() } else { self.shared.expand.clone() });
        }

        // Close (unless progress is showing)
        if !notification.has_active_progress() {
            actions.push(self.shared.close.clone());
        }

        self.toolbar.clear();
        self.toolbar.set_context(Some(notification_view_item_context(notification)));
        self.toolbar_actions.set(actions.len());
        for action in actions {
            let keybinding = self.get_keybinding_label(&action);
            self.toolbar.push_action(action, IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), keybinding, ..Default::default() }, index: None });
        }
    }

    fn render_source(&self, notification: &NotificationViewItem, source_custom_hover: &Rc<dyn IManagedHover>) {
        let source = notification.source().filter(|_| notification.expanded());
        let mut tree = self.services.ui.tree_mut();
        match source {
            Some(source) => {
                let text = localize("notificationSource", "Source: {0}", &[&source]);
                let font = ui_font(SOURCE_FONT_SIZE, 400);
                set_node_text(&mut tree, self.nodes.source, &text, &font, self.services.styles.foreground, LINE_HEIGHT);
                drop(tree);
                source_custom_hover.update(source.as_str().into(), None);
            }
            None => {
                je_widgets::text::clear_node_content(&mut tree, self.nodes.source);
                drop(tree);
                source_custom_hover.update("".into(), None);
            }
        }
    }

    fn render_buttons(&self, notification: &Rc<NotificationViewItem>) {
        clear_node(&mut self.services.ui.tree_mut(), self.nodes.buttons_container);

        let primary_actions = notification.actions().primary;
        if !(notification.expanded() && !primary_actions.is_empty()) {
            return;
        }

        let item = notification.clone();
        let action_runner = self.action_runner.clone();
        let inner_runner = ActionRunner::with_run_action(Rc::new(move |action: &ActionRef, _context: Option<Dynamic>| {
            // Run action
            let _ = action_runner.run(action, Some(notification_view_item_context(&item)));

            // Hide notification (unless explicitly prevented)
            let keep_open = action.as_any().downcast_ref::<ChoiceAction>().is_some_and(ChoiceAction::keep_open);
            if !keep_open {
                item.close();
            }
            je_base::promise::Promise::resolved(())
        }));
        self.input_disposables.add_rc(inner_runner.clone());

        let button_toolbar = Rc::new(ButtonBar::new(&self.services.ui, self.nodes.buttons_container, None));
        self.input_disposables.add_rc(button_toolbar.clone());
        for (i, action) in primary_actions.iter().enumerate() {
            let options = ButtonOptions {
                title: ButtonTitle::Auto, // assign titles to buttons in case they overflow
                secondary: i > 0,
                styles: self.services.styles.button_styles.clone(),
                ..Default::default()
            };

            let dropdown_actions = action.as_any().downcast_ref::<ChoiceAction>().and_then(ChoiceAction::menu).map(|menu| menu.iter().map(|choice| choice.clone() as ActionRef).collect::<Vec<_>>());
            let button: Rc<dyn IButton> = match dropdown_actions {
                Some(dropdown_actions) => button_toolbar.add_button_with_dropdown(ButtonWithDropdownOptions {
                    button: options,
                    context_menu_provider: self.services.button_context_menu.clone(),
                    actions: ButtonDropdownActions::Actions(dropdown_actions),
                    action_runner: Some(inner_runner.clone() as Rc<dyn IActionRunner>),
                    add_primary_action_to_dropdown: None,
                    dropdown_layer: None,
                }),
                None => button_toolbar.add_button(options),
            };

            button.set_label(ButtonLabel::Text(action.label()));
            button.set_enabled(action.enabled());
            // `action instanceof Action` (o `ChoiceAction` estende `Action`, aqui é composição)
            let on_did_change = match action.as_any().downcast_ref::<ChoiceAction>() {
                Some(choice) => Some(choice.on_did_change()),
                None => action.as_base_action().map(Action::on_did_change),
            };
            if let Some(on_did_change) = on_did_change {
                let button = button.clone();
                self.input_disposables.add(on_did_change.listen(move |change| {
                    if let Some(enabled) = change.enabled {
                        button.set_enabled(enabled);
                    }
                }));
            }

            let (runner, action) = (inner_runner.clone(), action.clone());
            self.input_disposables.add(button.on_did_click().listen(move |e| {
                let _ = e; // `EventHelper.stop(e, true)`: o botão já consome o evento
                let _ = runner.run(&action, None);
            }));
        }
    }

    fn render_progress(&self, notification: &NotificationViewItem) {
        // Return early if the item has no progress
        if !notification.has_active_progress() {
            self.progress.stop().hide();

            return;
        }

        // Infinite
        let state = notification.progress().state();
        if state.infinite == Some(true) {
            self.progress.infinite().show(None);
        }
        // Total / Worked
        else if state.total.is_some() || state.worked.is_some() {
            if let Some(total) = state.total
                && !self.progress.has_total()
            {
                self.progress.total(total);
            }

            if let Some(worked) = state.worked {
                self.progress.set_worked(worked).show(None);
            }
        }
        // Done
        else {
            self.progress.done().hide();
        }
    }

    fn severity_icon(severity: Severity) -> ThemeIcon {
        match severity {
            Severity::Warning => ThemeIcon::from_id("warning"),
            Severity::Error => ThemeIcon::from_id("error"),
            _ => ThemeIcon::from_id("info"),
        }
    }

    fn get_keybinding_label(&self, action: &ActionRef) -> Option<String> {
        self.services.keybinding_service.lookup_keybinding(&action.id(), None, false).and_then(|keybinding| keybinding.get_label())
    }

    /// As regras de CSS da linha: o `flex` de `.notification-list-item` e das duas linhas internas, com as medidas
    /// de `notificationsList.css`.
    pub fn layout(&self) {
        let Some(notification) = self.current.borrow().clone() else { return };
        let ui = &self.services.ui;
        let bounds = ui.tree().bounds(self.nodes.container);
        let width = if bounds.width() > 0.0 { bounds.width() } else { NOTIFICATIONS_CONTAINER_WIDTH };
        let height = if bounds.height() > 0.0 { bounds.height() } else { notification_row_height() };

        let has_details = notification.expanded() && (notification.source().is_some() || !notification.actions().primary.is_empty());
        let details_height = if has_details { notification_row_height() } else { 0.0 };
        let inner_height = (height - 2.0 * ITEM_PADDING_Y).max(0.0);
        let main_height = (inner_height - details_height).max(0.0);
        let inner_width = (width - 2.0 * ITEM_PADDING_X).max(0.0);

        let toolbar_visible = self.toolbar_visible(&notification) && self.toolbar_actions.get() > 0;
        let toolbar_width = if toolbar_visible { self.toolbar_actions.get() as f64 * TOOLBAR_ITEM_WIDTH } else { 0.0 };
        let message_left = ICON_MARGIN + ICON_WIDTH + ICON_MARGIN;
        let message_width = (inner_width - message_left - toolbar_width).max(0.0);

        {
            let mut tree = ui.tree_mut();
            tree.set_bounds(self.nodes.item, Rect::new(0.0, 0.0, width, height));
            let main_top = ITEM_PADDING_Y;
            tree.set_bounds(self.nodes.main_row, Rect::new(ITEM_PADDING_X, main_top, ITEM_PADDING_X + inner_width, main_top + main_height));
            tree.set_bounds(self.nodes.icon, Rect::new(ICON_MARGIN, 0.0, ICON_MARGIN + ICON_WIDTH, LINE_HEIGHT));
            tree.set_bounds(self.nodes.message, Rect::new(message_left, 0.0, message_left + message_width, main_height));
            tree.style_mut(self.nodes.toolbar_container).visible = toolbar_visible;
            let toolbar_left = inner_width - toolbar_width;
            tree.set_bounds(self.nodes.toolbar_container, Rect::new(toolbar_left, 0.0, inner_width, LINE_HEIGHT));

            tree.style_mut(self.nodes.details_row).visible = has_details;
            let details_top = height - ITEM_PADDING_Y - details_height;
            tree.set_bounds(self.nodes.details_row, Rect::new(ITEM_PADDING_X, details_top, ITEM_PADDING_X + inner_width, details_top + details_height));
        }

        // A mensagem muda de largura quando a barra de ações aparece ou some.
        if notification.expanded() || self.message_overflows.get() || toolbar_visible {
            self.rerender_message_in_place(&notification, message_width);
        }
        self.toolbar.layout();
        if has_details {
            self.layout_details(inner_width, details_height);
        }
    }

    fn rerender_message_in_place(&self, notification: &Rc<NotificationViewItem>, width: f64) {
        self.message_disposables.clear();
        clear_node(&mut self.services.ui.tree_mut(), self.nodes.message);
        let weak = self.weak_self.clone();
        let item = notification.clone();
        let handler = MessageActionHandler {
            to_dispose: self.message_disposables.clone(),
            callback: Rc::new(move |link: &str| {
                let Some(renderer) = weak.upgrade() else { return };
                renderer.log_interaction(&item, NotificationInteraction::Link);
                let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..Default::default() };
                let _ = renderer.services.opener_service.open(&URI::parse(link, false).into(), Some(&options));
            }),
        };
        NotificationMessageRenderer::render(&self.services.ui, self.nodes.message, notification, &self.services.styles, width, notification.expanded(), Some(&handler));
    }

    /// A linha de detalhes: a fonte (`flex: 1`, elipse) e, à direita, os botões com `margin: 4px 5px`.
    fn layout_details(&self, inner_width: f64, details_height: f64) {
        let ui = &self.services.ui;
        let button_nodes: Vec<NodeId> = ui.tree().children(self.nodes.buttons_container).to_vec();

        let mut tree = ui.tree_mut();
        let mut x = 0.0;
        for node in &button_nodes {
            let b = tree.bounds(*node);
            let (w, h) = (b.width(), b.height());
            tree.set_bounds(*node, Rect::new(x + 5.0, 4.0, x + 5.0 + w, 4.0 + h));
            x += w + 10.0;
        }
        let buttons_width = x.min(inner_width);
        // `padding-left: 5px` da linha, `flex: 1` da fonte
        let source_left = 5.0;
        let source_width = (inner_width - source_left - buttons_width).max(0.0);
        tree.set_bounds(self.nodes.source, Rect::new(source_left, 0.0, source_left + source_width, details_height));
        tree.set_bounds(self.nodes.buttons_container, Rect::new(inner_width - buttons_width, 0.0, inner_width, details_height));
    }
}

impl IDisposable for NotificationTemplateRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationTemplateRenderer"
    }
}
