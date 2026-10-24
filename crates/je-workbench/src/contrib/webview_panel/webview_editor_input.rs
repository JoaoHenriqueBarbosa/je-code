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
//! - `getSelection()` devolve `None` e `applyOptions(options, token)` não faz nada (a promessa já resolvida) na entrada
//!   simples; as subclasses sobrescrevem os dois pelo [`WebviewInputKind`].
//! - **Subclasses (`LazilyResolvedWebviewEditorInput`, `CustomEditorInput`, `CustomEditorDiffInput`...).** As interfaces do
//!   porte (`IWebviewWorkbenchService`, `WebviewResolver`, `MainThreadWebviewPanels`) trabalham com `Rc<WebviewInput>`, e o
//!   `instanceof` das subclasses precisa devolver a mesma entrada (a identidade `===` que o workbench compara). A herança
//!   é o [`WebviewInputKind`]: o objeto que a entrada guarda e para o qual encaminha cada método de [`EditorInput`] que uma
//!   subclasse pode sobrescrever (a tabela virtual); o padrão de cada método é o comportamento desta classe (o
//!   `super.método()` da subclasse é o método `base_*` correspondente de [`WebviewInput`] ou o padrão do trait).
//!   `instanceof Subclasse` é [`WebviewInput::kind_as`], e `new Subclasse(...)` é [`WebviewInput::with_kind`]. O
//!   `Rc<WebviewInput>` da própria entrada (o `this` das continuações assíncronas) é [`WebviewInput::rc`].
//! - `transfer(other)` chama [`WebviewInputKind::on_transfer`] depois de passar o webview (o que as subclasses sobrescrevem
//!   para levar o próprio estado junto) e `dispose()` chama [`WebviewInputKind::on_dispose`] depois do descarte da base.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};
use je_base::uuid::generate_uuid;
use je_theme::ThemeType;

use crate::common::editor::editor_input::{CanMove, EditorInput, EditorInputBase, EditorInputIcon, UntypedEditorOptions, matches_by_default};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, GroupIdentifier, IEditorPaneSelection, MoveResult, RevertOptions, SaveOptions, TextEditorOptions,
    UntypedEditorInput, Verbosity,
};
use crate::contrib::webview::browser_webview::{IOverlayWebview, IWebview, WebviewExtensionDescription};
use crate::services::files_configuration::files_configuration_service::Readonly;

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

/// A tabela virtual de [`WebviewInput`] (ver a nota do módulo): cada método tem como padrão o comportamento da classe
/// `WebviewInput`, e a subclasse sobrescreve os que quiser. `input` é a entrada que recebeu a chamada.
pub trait WebviewInputKind {
    /// Para o `instanceof` ([`WebviewInput::kind_as`]).
    fn as_any(&self) -> &dyn Any;

    /// Chamado ao fim do construtor, com a entrada pronta (o que a subclasse registra nos eventos de serviços).
    fn attach(&self, _input: &Rc<WebviewInput>) {}

    /// `typeId`.
    fn editor_type_id(&self, _input: &WebviewInput) -> String {
        WebviewInput::TYPE_ID.to_owned()
    }

    /// `editorId`.
    fn editor_id(&self, input: &WebviewInput) -> Option<String> {
        input.base_editor_id()
    }

    /// `capabilities`.
    fn capabilities(&self, input: &WebviewInput) -> EditorInputCapabilities {
        input.base_capabilities()
    }

    /// `resource`.
    fn resource(&self, input: &WebviewInput) -> Option<URI> {
        input.base_resource()
    }

    /// `getName()`.
    fn get_name(&self, input: &WebviewInput) -> String {
        input.base_name()
    }

    /// `getTitle(verbosity)`.
    fn get_title(&self, input: &WebviewInput, _verbosity: Option<Verbosity>) -> String {
        EditorInput::get_name(input)
    }

    /// `getDescription(verbosity)`.
    fn get_description(&self, _input: &WebviewInput, _verbosity: Option<Verbosity>) -> Option<String> {
        None
    }

    /// `isReadonly()`.
    fn is_readonly(&self, input: &WebviewInput) -> Readonly {
        Readonly::Bool(input.has_capability(EditorInputCapabilities::READONLY))
    }

    /// `isDirty()`.
    fn is_dirty(&self, _input: &WebviewInput) -> bool {
        false
    }

    /// `matches(otherInput)`.
    fn matches(&self, input: &WebviewInput, other_input: &EditorInputOrUntyped) -> bool {
        matches_by_default(input, other_input)
    }

    /// `resolve()`.
    fn resolve(&self, _input: &WebviewInput) -> Promise<Option<Rc<dyn IEditorModel>>> {
        Promise::resolved(None)
    }

    /// `save(group, options)`.
    fn save(&self, input: &WebviewInput, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        Promise::resolved(Some(EditorInputOrUntyped::Typed(input.this())))
    }

    /// `saveAs(group, options)`.
    fn save_as(&self, input: &WebviewInput, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        Promise::resolved(Some(EditorInputOrUntyped::Typed(input.this())))
    }

    /// `revert(group, options)`.
    fn revert(&self, _input: &WebviewInput, _group: GroupIdentifier, _options: Option<RevertOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    /// `rename(group, target)`.
    fn rename(&self, _input: &WebviewInput, _group: GroupIdentifier, _target: &URI) -> Promise<Option<MoveResult>> {
        Promise::resolved(None)
    }

    /// `copy()`.
    fn copy(&self, input: &WebviewInput) -> Rc<dyn EditorInput> {
        input.this()
    }

    /// `canMove(sourceGroup, targetGroup)`.
    fn can_move(&self, _input: &WebviewInput, _source_group: GroupIdentifier, _target_group: GroupIdentifier) -> CanMove {
        CanMove::Yes
    }

    /// `toUntyped(options)`.
    fn to_untyped(&self, _input: &WebviewInput, _options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        None
    }

    /// `diffResources` (de `IEditorInputWithDiffResources`).
    fn diff_resources(&self, _input: &WebviewInput) -> Option<(URI, URI)> {
        None
    }

    /// `getSelection()`.
    fn get_selection(&self, _input: &WebviewInput) -> Option<Rc<dyn IEditorPaneSelection>> {
        None
    }

    /// `applyOptions(options, token)`.
    fn apply_options(&self, _input: &WebviewInput, _options: &TextEditorOptions, _token: CancellationToken) -> Promise<()> {
        Promise::resolved(())
    }

    /// O que a subclasse faz depois do `dispose()` da base (`super.dispose()` e em seguida o resto).
    fn on_dispose(&self, _input: &WebviewInput) {}

    /// O que a subclasse leva para `other` depois de `transfer(other)` passar o webview.
    fn on_transfer(&self, _input: &WebviewInput, _other: &Rc<WebviewInput>) {}
}

/// A tabela virtual de `WebviewInput` sem sobrescritas (o `new WebviewInput(...)`).
struct PlainWebviewInputKind;

impl WebviewInputKind for PlainWebviewInputKind {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct WebviewInput {
    me: Weak<WebviewInput>,
    kind: Rc<dyn WebviewInputKind>,
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
        Self::with_kind(init, webview, theme_service, Rc::new(PlainWebviewInputKind))
    }

    /// `new Subclasse(init, webview, themeService, ...)`: a entrada com a tabela virtual da subclasse.
    pub fn with_kind(
        init: WebviewInputInitInfo,
        webview: Rc<dyn IOverlayWebview>,
        theme_service: Rc<dyn IWebviewThemeService>,
        kind: Rc<dyn WebviewInputKind>,
    ) -> Rc<Self> {
        let input = Rc::new_cyclic(|this| Self {
            me: this.clone(),
            kind: kind.clone(),
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

        kind.attach(&input);

        input
    }

    /// O `Rc` da própria entrada (o `this` das continuações assíncronas).
    pub fn rc(&self) -> Rc<WebviewInput> {
        self.me.upgrade().expect("a entrada de webview já foi liberada")
    }

    /// `this instanceof Subclasse`: a tabela virtual da entrada, quando é a da subclasse pedida.
    pub fn kind_as<T: 'static>(&self) -> Option<&T> {
        self.kind.as_any().downcast_ref::<T>()
    }

    /// `applyOptions(options, token)`.
    pub fn apply_options(&self, options: &TextEditorOptions, token: CancellationToken) -> Promise<()> {
        self.kind.apply_options(self, options, token)
    }

    /// `getSelection()`.
    pub fn get_selection(&self) -> Option<Rc<dyn IEditorPaneSelection>> {
        self.kind.get_selection(self)
    }

    /// `editorId` de `WebviewInput`.
    pub fn base_editor_id(&self) -> Option<String> {
        Some(self.view_type.clone())
    }

    /// `capabilities` de `WebviewInput`.
    pub fn base_capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY | EditorInputCapabilities::SINGLETON | EditorInputCapabilities::CAN_DROP_INTO_EDITOR
    }

    /// `resource` de `WebviewInput`.
    pub fn base_resource(&self) -> Option<URI> {
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

    /// `getName()` de `WebviewInput`.
    pub fn base_name(&self) -> String {
        self.webview_title.borrow().clone()
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
        self.kind.on_transfer(self, other);
        Some(other.clone())
    }
}

impl IDisposable for WebviewInput {
    fn dispose(&self) {
        if !self.base.is_disposed() && !self.has_transfered.get() {
            self.webview().dispose();
        }
        self.base.dispose();
        self.kind.on_dispose(self);
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
        self.kind.editor_type_id(self)
    }

    fn editor_id(&self) -> Option<String> {
        self.kind.editor_id(self)
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        self.kind.capabilities(self)
    }

    fn resource(&self) -> Option<URI> {
        self.kind.resource(self)
    }

    fn get_name(&self) -> String {
        self.kind.get_name(self)
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        self.kind.get_title(self, verbosity)
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        self.kind.get_description(self, verbosity)
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        match self.icon_path.borrow().as_ref()? {
            WebviewIconPath::ThemeIcon(icon) => Some(EditorInputIcon::Theme(icon.clone())),
            WebviewIconPath::LightDark { light, dark } => {
                Some(EditorInputIcon::Uri(if is_dark(self.theme_service.color_theme_type()) { dark.clone() } else { light.clone() }))
            }
        }
    }

    fn is_readonly(&self) -> Readonly {
        self.kind.is_readonly(self)
    }

    fn is_dirty(&self) -> bool {
        self.kind.is_dirty(self)
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        self.kind.matches(self, other_input)
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        self.kind.resolve(self)
    }

    fn save(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        self.kind.save(self, group, options)
    }

    fn save_as(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        self.kind.save_as(self, group, options)
    }

    fn revert(&self, group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.kind.revert(self, group, options)
    }

    fn rename(&self, group: GroupIdentifier, target: &URI) -> Promise<Option<MoveResult>> {
        self.kind.rename(self, group, target)
    }

    fn copy(&self) -> Rc<dyn EditorInput> {
        self.kind.copy(self)
    }

    fn can_move(&self, source_group: GroupIdentifier, target_group: GroupIdentifier) -> CanMove {
        self.kind.can_move(self, source_group, target_group)
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        self.kind.to_untyped(self, options)
    }

    fn diff_resources(&self) -> Option<(URI, URI)> {
        self.kind.diff_resources(self)
    }
}
