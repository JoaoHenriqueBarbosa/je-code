//! Porte de `src/vs/workbench/contrib/webviewPanel/browser/webviewEditorInput.ts`: a entrada de editor de um painel de
//! webview (`WebviewInput`).
//!
//! Divergências:
//! - `IThemeService` (de `platform/theme`) não foi portado: o trait [`IWebviewThemeService`] traz só o que a entrada lê dele
//!   (`onDidColorThemeChange` e o tipo de `getColorTheme()`), e `isDark(type)` é a conferência de [`ThemeType`].
//! - `WebviewInput extends EditorInput` é o [`EditorInputBase`] mais o trait [`EditorInput`]; `instanceof WebviewInput` é o
//!   `downcast_ref` de [`EditorInput::as_any`]. A entrada é um `Rc` (nasce em `Rc::new_cyclic`), e `transfer` recebe e devolve
//!   `Rc<WebviewInput>`.
//! - `_webview` é um `RefCell`, porque `transfer(other)` troca o webview da outra entrada. `iconPath`, `group` e o título
//!   são `RefCell`/`Cell`; o `iconPath` é lido e escrito por [`WebviewInput::icon_path`] e [`WebviewInput::set_icon_path`].
//! - `resource` com `providedId` ausente escreve `undefined` no caminho, como a interpolação do original.
//! - `claim(claimant, targetWindow, scopedContextKeyService)` repassa a um `CodeWindow` e a um serviço de chaves de
//!   contexto de um elemento DOM: é recorte da Fase 6 (o webview em si), junto do `claim` de [`IOverlayWebview`].
//! - `getSelection()` devolve sempre `None`, e `applyOptions(options, token)` não faz nada (a promessa já resolvida).

use std::cell::{Cell, RefCell};
use std::any::Any;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_base::js_string::JsString;
use je_platform::opener::editor_types::EditorOptions;
use je_theme::ThemeType;

use crate::common::editor::editor_input::{EditorInput, EditorInputBase, EditorInputIcon, matches_by_default};
use crate::common::editor::{EditorInputCapabilities, EditorInputOrUntyped, GroupIdentifier, IEditorPaneSelection, Verbosity};
use crate::contrib::webview::browser_webview::{IOverlayWebview, IWebview, WebviewExtensionDescription};

/// `WebviewIconPath`: `ThemeIcon | { readonly light: URI; readonly dark: URI }`.
#[derive(Clone, Debug, PartialEq)]
pub enum WebviewIconPath {
    ThemeIcon(ThemeIcon),
    LightDark { light: URI, dark: URI },
}

/// `IThemeService`, só o que o [`WebviewInput`] lê (ver a nota do módulo).
pub trait IWebviewThemeService {
    /// `onDidColorThemeChange`.
    fn on_did_color_theme_change(&self) -> Event<()>;

    /// `getColorTheme().type`.
    fn color_theme_type(&self) -> ThemeType;
}

/// `WebviewInputInitInfo`.
#[derive(Clone, Debug)]
pub struct WebviewInputInitInfo {
    pub view_type: String,
    pub provided_id: Option<String>,
    pub name: String,
    pub icon_path: Option<WebviewIconPath>,
}

/// `isDark(type)`.
fn is_dark(theme_type: ThemeType) -> bool {
    matches!(theme_type, ThemeType::Dark | ThemeType::HighContrastDark)
}

pub struct WebviewInput {
    base: EditorInputBase,

    resource_id: String,

    webview_title: RefCell<String>,
    icon_path: RefCell<Option<WebviewIconPath>>,
    group: Cell<Option<GroupIdentifier>>,

    webview: RefCell<Rc<dyn IOverlayWebview>>,

    has_transfered: Cell<bool>,

    pub view_type: String,
    pub provider_id: Option<String>,

    theme_service: Rc<dyn IWebviewThemeService>,
}

impl WebviewInput {
    /// `WebviewInput.typeId`.
    pub const TYPE_ID: &'static str = "workbench.editors.webviewInput";

    pub fn new(init: WebviewInputInitInfo, webview: Rc<dyn IOverlayWebview>, theme_service: Rc<dyn IWebviewThemeService>) -> Rc<Self> {
        let input = Rc::new_cyclic(|this| Self {
            base: EditorInputBase::new(this.clone()),
            resource_id: generate_uuid(),
            webview_title: RefCell::new(init.name),
            icon_path: RefCell::new(init.icon_path),
            group: Cell::new(None),
            webview: RefCell::new(webview),
            has_transfered: Cell::new(false),
            view_type: init.view_type,
            provider_id: init.provided_id,
            theme_service: theme_service.clone(),
        });

        let weak = Rc::downgrade(&input);
        input.base.register(theme_service.on_did_color_theme_change().listen(move |_| {
            // Potentially update icon
            if let Some(input) = weak.upgrade() {
                input.base.fire_did_change_label();
            }
        }));

        input
    }

    /// `applyOptions(options, token)`.
    pub fn apply_options(&self, _options: &EditorOptions, _token: CancellationToken) -> Promise<()> {
        Promise::resolved(())
    }

    /// `getSelection()`.
    pub fn get_selection(&self) -> Option<Rc<dyn IEditorPaneSelection>> {
        None
    }

    /// `setWebviewTitle(value)`.
    pub fn set_webview_title(&self, value: &str) {
        *self.webview_title.borrow_mut() = value.to_owned();
        self.webview().set_title(value);
        self.base.fire_did_change_label();
    }

    /// `getWebviewTitle()`.
    pub fn get_webview_title(&self) -> Option<String> {
        Some(self.webview_title.borrow().clone())
    }

    /// `webview`.
    pub fn webview(&self) -> Rc<dyn IOverlayWebview> {
        self.webview.borrow().clone()
    }

    /// `extension`.
    pub fn extension(&self) -> Option<WebviewExtensionDescription> {
        self.webview().extension()
    }

    /// `iconPath` (getter).
    pub fn icon_path(&self) -> Option<WebviewIconPath> {
        self.icon_path.borrow().clone()
    }

    /// `iconPath` (setter).
    pub fn set_icon_path(&self, value: Option<WebviewIconPath>) {
        *self.icon_path.borrow_mut() = value;
        self.base.fire_did_change_label();
    }

    /// `group`.
    pub fn group(&self) -> Option<GroupIdentifier> {
        self.group.get()
    }

    /// `updateGroup(group)`.
    pub fn update_group(&self, group: GroupIdentifier) {
        self.group.set(Some(group));
    }

    /// `transfer(other)`: passa o webview desta entrada para `other` (uma única vez).
    pub fn transfer(&self, other: &Rc<WebviewInput>) -> Option<Rc<WebviewInput>> {
        if self.has_transfered.get() {
            return None;
        }
        self.has_transfered.set(true);
        *other.webview.borrow_mut() = self.webview();
        Some(other.clone())
    }
}

impl IDisposable for WebviewInput {
    fn dispose(&self) {
        if !self.base.is_disposed() && !self.has_transfered.get() {
            self.webview().dispose();
        }
        self.base.dispose();
    }
}

impl EditorInput for WebviewInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::TYPE_ID.to_owned()
    }

    fn editor_id(&self) -> Option<String> {
        Some(self.view_type.clone())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY | EditorInputCapabilities::SINGLETON | EditorInputCapabilities::CAN_DROP_INTO_EDITOR
    }

    fn resource(&self) -> Option<URI> {
        let provider_id = self.provider_id.as_deref().unwrap_or("undefined");
        Some(URI::from(
            &UriComponents {
                scheme: Some(JsString::from(schemas::WEBVIEW_PANEL)),
                path: Some(JsString::from(format!("webview-panel/webview-{provider_id}-{}", self.resource_id))),
                ..UriComponents::default()
            },
            false,
        ))
    }

    fn get_name(&self) -> String {
        self.webview_title.borrow().clone()
    }

    fn get_title(&self, _verbosity: Option<Verbosity>) -> String {
        self.get_name()
    }

    fn get_description(&self, _verbosity: Option<Verbosity>) -> Option<String> {
        None
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        match self.icon_path.borrow().as_ref()? {
            WebviewIconPath::ThemeIcon(icon) => Some(EditorInputIcon::Theme(icon.clone())),
            WebviewIconPath::LightDark { light, dark } => {
                Some(EditorInputIcon::Uri(if is_dark(self.theme_service.color_theme_type()) { dark.clone() } else { light.clone() }))
            }
        }
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        matches_by_default(self, other_input)
    }
}
