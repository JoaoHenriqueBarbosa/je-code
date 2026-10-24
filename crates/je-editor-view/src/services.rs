//! Os serviços que o `CodeEditorWidget` recebe do workbench (`INotificationService`, `ICommandService`,
//! `IThemeService`) e o `CommonFindController` do editor, em versões mínimas: o workbench de verdade os
//! substitui quando ele existir (Fase 4 do `roadmap.md`).

use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor, IEditorContribution};
use je_editor::multicursor::{ICommonFindController, ICommonFindControllerSource, IDocumentHighlightProviderRegistry, IFindReplaceState};
use je_editor::multicursor_find_state::FindReplaceState;
use je_editor::text_model::TextModel;
use je_editor::view_model_impl::IThemeService;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
    NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_theme::ColorTheme;

struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

/// O `INotificationService` sem interface: descarta tudo (o painel de notificações é do workbench).
pub struct SilentNotifications;

impl INotificationService for SilentNotifications {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {}

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        NotificationsFilter::Off
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }

    fn remove_filter(&self, _source_id: &str) {}

    fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn info(&self, _message: Vec<NotificationMessage>) {}

    fn warn(&self, _message: Vec<NotificationMessage>) {}

    fn error(&self, _message: Vec<NotificationMessage>) {}

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}

/// O `ICommandService` sem comandos de plataforma: o editor só dispara os próprios (`trigger`).
pub struct NoCommands;

impl ICommandService for NoCommands {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        Promise::rejected(Error::new(format!("command '{command_id}' not found")))
    }
}

/// O `IThemeService` que só avisa a troca de tema (o tema do editor é o `EditorColors` da view).
pub struct StaticTheme {
    changed: Rc<Emitter<Rc<ColorTheme>>>,
}

impl StaticTheme {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { changed: Emitter::new() })
    }
}

impl IThemeService for StaticTheme {
    fn on_did_color_theme_change(&self) -> Event<Rc<ColorTheme>> {
        self.changed.event()
    }
}

/// O `ILanguageFeaturesService.documentHighlightProvider` sem provedores (as extensões de linguagem entram com o
/// extension host).
pub struct NoHighlightProviders;

impl IDocumentHighlightProviderRegistry for NoHighlightProviders {
    fn has(&self, _model: &TextModel) -> bool {
        false
    }
}

/// O `CommonFindController` do editor reduzido ao estado de busca e ao destaque das opções, o que o multicursor
/// (`Ctrl+D`) lê e escreve. O widget de busca da janela é outro objeto, do shell.
pub struct FindController {
    me: Weak<FindController>,
    state: Rc<FindReplaceState>,
}

impl FindController {
    pub const ID: &'static str = "editor.contrib.findController";

    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), state: FindReplaceState::new() })
    }

    /// O texto da última busca por `Ctrl+D`, para quem quiser mostrá-lo.
    pub fn search_string(&self) -> JsString {
        self.state.search_string()
    }
}

impl ICommonFindController for FindController {
    fn get_state(&self) -> Rc<dyn IFindReplaceState> {
        self.state.clone()
    }

    fn highlight_find_options(&self, _ignore_when_visible: bool) {}
}

impl IDisposable for FindController {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl IEditorContribution for FindController {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// `CommonFindController.get(editor)` e `createInstance(CommonFindController, editor)`.
pub struct FindControllers;

impl ICommonFindControllerSource for FindControllers {
    fn get(&self, editor: &CodeEditorWidget) -> Option<Rc<dyn ICommonFindController>> {
        let contribution = editor.get_contribution(FindController::ID)?;
        let controller: Rc<dyn ICommonFindController> = contribution.as_any()?.downcast_ref::<FindController>()?.me.upgrade()?;
        Some(controller)
    }

    fn create_instance(&self, _editor: &Rc<CodeEditorWidget>) -> Rc<dyn ICommonFindController> {
        FindController::new()
    }
}
