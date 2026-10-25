//! Porte de `src/vs/workbench/contrib/modernUI/browser/modernUI.contribution.ts`: a contribuição que liga e desliga o
//! Modern UI (as classes dos contêineres, o tamanho das barras de rolagem, a altura das linhas de notificação e o
//! cabeçalho dos panes) e o catálogo de módulos.
//!
//! Recortes:
//! - Os `import './media/*.css'` e o `import './connectedEditorTabs.js'` (que registra o `registerThemingParticipant`
//!   das abas conectadas) são a especificação visual: o `.css` é lido ao portar cada widget (CLAUDE.md do projeto) e
//!   o participante de tema entra com o serviço de temas.
//! - O `registerWorkbenchContribution2(ModernUIContribution.ID, ..., WorkbenchPhase.BlockRestore)` fica com quem monta o
//!   workbench; o `ID` é [`ModernUiContribution::ID`].
//!
//! Divergências:
//! - **O DOM.** `container.classList.toggle(...)`, `getWindow(container).vscodeWindowId` e
//!   `IAuxiliaryWindowService.getWindow(id)?.layout()` não têm equivalente: a árvore é nativa e os contêineres são
//!   [`LayoutContainerId`] opacos. O trait [`ModernUiHost`] reúne o que a contribuição pede de `IWorkbenchLayoutService`
//!   (`onDidAddContainer`, `containers`, `mainContainer`, `layout`) e o que ela faz nos contêineres (ligar ou desligar
//!   uma classe, e fazer o layout da janela auxiliar do contêiner), implementado por quem monta o workbench.
//! - `IConfigurationService` é o trait de `je-platform`, e `getValue<T>(key) === true` é `Dynamic::Bool(true)`.
//! - `MenuRegistry.appendMenuItem(MenuId.GlobalActivity, ...)` do início do módulo é
//!   [`register_layout_density_menu`] (registro explícito).
//! - `setGlobalDefaultScrollbarSize`, `setGlobalPaneHeaderSize` e `setNotificationRowHeight` são os globais por thread
//!   de `je-widgets` e de `notifications_viewer.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_platform::action::Title;
use je_platform::actions::actions::{AnyMenuItem, SubmenuItem, menu_registry};
use je_platform::actions::menu_ids;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::ContextKeyExpr;
use je_widgets::paneview::{DEFAULT_PANE_HEADER_SIZE, set_global_pane_header_size};
use je_widgets::scrollbar::scrollable_element::{DEFAULT_SCROLLBAR_SIZE, set_global_default_scrollbar_size};

use crate::browser::actions::layout_density_actions::LayoutDensityMenu;
use crate::browser::parts::editor::editor::CONNECTED_EDITOR_TABS_CLASS;
use crate::browser::parts::notifications::notifications_viewer::{
    COMPACT_NOTIFICATION_ROW_HEIGHT, DEFAULT_NOTIFICATION_ROW_HEIGHT, set_notification_row_height,
};
use crate::services::layout::layout_service::{AddedContainer, LayoutContainerId, LayoutSettings, ModernUiDensity, ModernUiEditorTabStyle};

/// Reduced scrollbar size (px) applied when Modern UI is on.
pub const MODERN_UI_SCROLLBAR_SIZE: f64 = 8.0;

/// Increased pane header size (px) applied when Modern UI is on.
pub const MODERN_UI_PANE_HEADER_SIZE: f64 = 28.0;

/// The primary class toggled when the Modern UI experiment is enabled. Modules that can be reused independently also
/// receive dedicated classes below.
pub const MODERN_UI_CLASS: &str = "modern-ui";
pub const MODERN_UI_COMPACT_CLASS: &str = "modern-ui-compact";
pub const MODERN_UI_TABS_CLASS: &str = "modern-ui-tabs";
pub const MODERN_UI_NOTIFICATIONS_DIALOGS_CLASS: &str = "modern-ui-notifications-dialogs";
pub const MODERN_UI_UPPERCASE_VIEW_HEADERS_CLASS: &str = "modern-ui-uppercase-view-headers";

/// `IModernUIModule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModernUiModule {
    pub id: &'static str,
    /// Whether this module changes layout metrics. Toggling such a module requires a workbench relayout; modules
    /// without this flag only affect appearance.
    pub layout_affecting: bool,
}

const fn module(id: &'static str) -> ModernUiModule {
    ModernUiModule { id, layout_affecting: false }
}

/// The fixed catalog of built-in Modern UI modules. The CSS for each module ships with the product, and all modules
/// are enabled together as part of the Modern UI experiment (`LayoutSettings.MODERN_UI`). This catalog tracks
/// per-module metadata such as whether a module affects layout.
pub const MODERN_UI_MODULES: [ModernUiModule; 15] = [
    module("activityBar"),
    module("commandCenter"),
    module("editorBorder"),
    module("fontRamp"),
    module("keyboardFocusOnly"),
    module("padding"),
    ModernUiModule { id: "paneHeaders", layout_affecting: true },
    module("roundedCorners"),
    module("sashHandles"),
    module("scrollShadows"),
    module("shadows"),
    module("statusBar"),
    module("tabs"),
    module("titlebar"),
    module("notificationsDialogs"),
];

/// O que a contribuição pede ao workbench (ver as divergências do módulo).
pub trait ModernUiHost {
    /// `layoutService.onDidAddContainer`.
    fn on_did_add_container(&self) -> Event<AddedContainer>;

    /// `layoutService.containers`.
    fn containers(&self) -> Vec<LayoutContainerId>;

    /// `layoutService.mainContainer`.
    fn main_container(&self) -> LayoutContainerId;

    /// `layoutService.layout()`.
    fn layout(&self);

    /// `container.classList.toggle(className, force)`.
    fn toggle_class(&self, container: LayoutContainerId, class_name: &str, force: bool);

    /// `auxiliaryWindowService.getWindow(getWindow(container).vscodeWindowId)?.layout()`.
    fn layout_auxiliary_window(&self, container: LayoutContainerId);
}

/// `MenuRegistry.appendMenuItem(MenuId.GlobalActivity, { title: 'Layout Density', submenu: LayoutDensityMenu, ... })`.
pub fn register_layout_density_menu() -> DisposableRef {
    menu_registry().append_menu_item(
        &menu_ids::global_activity(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("layoutDensity", "Layout Density", &[])),
            submenu: LayoutDensityMenu(),
            icon: None,
            when: Some(ContextKeyExpr::equals(&format!("config.{}", LayoutSettings::ModernUi.as_str()), true)),
            group: Some("2_configuration".to_string()),
            order: Some(8.0),
            is_selection: None,
            is_split_button: None,
        }),
    )
}

/// A contribution that toggles the built-in CSS Modern UI modules on or off as a group, based on the
/// `workbench.experimental.modernUI` setting. When the Modern UI Update experiment is enabled, all modules are applied
/// together; otherwise none are.
pub struct ModernUiContribution {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    host: Rc<dyn ModernUiHost>,
    has_layout_affecting_module: bool,
    layout_affecting_state: RefCell<String>,
    disposed: Cell<bool>,
}

impl ModernUiContribution {
    pub const ID: &'static str = "workbench.contrib.modernUI";

    pub fn new(configuration_service: Rc<dyn IConfigurationService>, host: Rc<dyn ModernUiHost>) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            configuration_service,
            host,
            has_layout_affecting_module: MODERN_UI_MODULES.iter().any(|module| module.layout_affecting),
            layout_affecting_state: RefCell::new("disabled".to_string()),
            disposed: Cell::new(false),
        });
        *this.layout_affecting_state.borrow_mut() = this.get_layout_affecting_state();

        // A config change re-applies to every container (the global `update()` covers all windows, including
        // auxiliary ones).
        let weak = Rc::downgrade(&this);
        this.base.register(this.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if e.affects_configuration(LayoutSettings::ModernUi.as_str(), None)
                || e.affects_configuration(LayoutSettings::ModernUiDensity.as_str(), None)
                || e.affects_configuration(LayoutSettings::ModernUiUppercaseViewHeaders.as_str(), None)
                || e.affects_configuration(LayoutSettings::ModernUiEditorTabStyle.as_str(), None)
            {
                this.update();
                // Some modules change layout metrics, so a relayout is required once their classes and corresponding
                // layout values are updated.
                let layout_affecting_state = this.get_layout_affecting_state();
                if layout_affecting_state != *this.layout_affecting_state.borrow() {
                    *this.layout_affecting_state.borrow_mut() = layout_affecting_state;
                    this.layout();
                }
            }
        }));

        // Apply the current selection to windows opened after startup (e.g. auxiliary windows). Subsequent config
        // changes are handled by `update()`.
        let weak = Rc::downgrade(&this);
        this.base.register(this.host.on_did_add_container().listen(move |added| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let enabled = this.is_enabled();
            this.apply_to(added.container, enabled, enabled && this.is_compact(), enabled && this.use_uppercase_view_headers());
        }));

        this.update();
        this
    }

    fn layout(&self) {
        self.host.layout();
        for container in self.host.containers() {
            if container != self.host.main_container() {
                self.host.layout_auxiliary_window(container);
            }
        }
    }

    fn is_true(&self, setting: LayoutSettings) -> bool {
        matches!(self.configuration_service.get_value(Some(setting.as_str()), None), Dynamic::Bool(true))
    }

    fn is_setting(&self, setting: LayoutSettings, value: &str) -> bool {
        matches!(self.configuration_service.get_value(Some(setting.as_str()), None), Dynamic::String(ref text) if text.to_string_lossy() == value)
    }

    fn is_enabled(&self) -> bool {
        self.is_true(LayoutSettings::ModernUi)
    }

    fn use_uppercase_view_headers(&self) -> bool {
        self.is_true(LayoutSettings::ModernUiUppercaseViewHeaders)
    }

    fn is_compact(&self) -> bool {
        self.is_setting(LayoutSettings::ModernUiDensity, ModernUiDensity::Compact.as_str())
    }

    fn use_connected_editor_tabs(&self) -> bool {
        self.is_setting(LayoutSettings::ModernUiEditorTabStyle, ModernUiEditorTabStyle::Connected.as_str())
    }

    fn get_layout_affecting_state(&self) -> String {
        if !self.is_enabled() || !self.has_layout_affecting_module {
            return "disabled".to_string();
        }
        let density = if self.is_compact() { ModernUiDensity::Compact } else { ModernUiDensity::Default };
        let tab_style = if self.use_connected_editor_tabs() { ModernUiEditorTabStyle::Connected } else { ModernUiEditorTabStyle::Pill };
        format!("{}/{}", density.as_str(), tab_style.as_str())
    }

    fn update(&self) {
        let enabled = self.is_enabled();
        let compact = enabled && self.is_compact();
        let use_uppercase_view_headers = enabled && self.use_uppercase_view_headers();
        self.apply_pane_header_size(enabled);
        for container in self.host.containers() {
            self.apply_to(container, enabled, compact, use_uppercase_view_headers);
        }
        self.apply_scrollbar_size(enabled);
        self.apply_notification_row_height(enabled);
    }

    fn apply_to(&self, container: LayoutContainerId, enabled: bool, compact: bool, use_uppercase_view_headers: bool) {
        self.host.toggle_class(container, MODERN_UI_CLASS, enabled);
        self.host.toggle_class(container, MODERN_UI_COMPACT_CLASS, compact);
        self.host.toggle_class(container, MODERN_UI_TABS_CLASS, enabled);
        self.host.toggle_class(container, CONNECTED_EDITOR_TABS_CLASS, enabled && self.use_connected_editor_tabs());
        self.host.toggle_class(container, MODERN_UI_NOTIFICATIONS_DIALOGS_CLASS, enabled);
        self.host.toggle_class(container, MODERN_UI_UPPERCASE_VIEW_HEADERS_CLASS, use_uppercase_view_headers);
    }

    fn apply_scrollbar_size(&self, enabled: bool) {
        set_global_default_scrollbar_size(if enabled { MODERN_UI_SCROLLBAR_SIZE } else { DEFAULT_SCROLLBAR_SIZE });
    }

    fn apply_notification_row_height(&self, enabled: bool) {
        set_notification_row_height(if enabled { COMPACT_NOTIFICATION_ROW_HEIGHT } else { DEFAULT_NOTIFICATION_ROW_HEIGHT });
    }

    fn apply_pane_header_size(&self, enabled: bool) {
        let mut pane_header_size = DEFAULT_PANE_HEADER_SIZE;
        if enabled {
            pane_header_size = MODERN_UI_PANE_HEADER_SIZE;
        }
        set_global_pane_header_size(pane_header_size);
    }
}

impl IDisposable for ModernUiContribution {
    /// Remove the class this contribution added so it leaves no DOM state behind.
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        for container in self.host.containers() {
            self.host.toggle_class(container, MODERN_UI_CLASS, false);
            self.host.toggle_class(container, MODERN_UI_COMPACT_CLASS, false);
            self.host.toggle_class(container, MODERN_UI_TABS_CLASS, false);
            self.host.toggle_class(container, CONNECTED_EDITOR_TABS_CLASS, false);
            self.host.toggle_class(container, MODERN_UI_NOTIFICATIONS_DIALOGS_CLASS, false);
            self.host.toggle_class(container, MODERN_UI_UPPERCASE_VIEW_HEADERS_CLASS, false);
        }
        set_global_default_scrollbar_size(DEFAULT_SCROLLBAR_SIZE);
        set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);
        set_global_pane_header_size(DEFAULT_PANE_HEADER_SIZE);
        self.base.dispose();
    }
}
