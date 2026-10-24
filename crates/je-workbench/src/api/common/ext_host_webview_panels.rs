//! Porte de `src/vs/workbench/api/common/extHostWebviewPanels.ts`: o lado das extensões dos painéis de webview
//! (`vscode.WebviewPanel`) e o `ExtHostWebviewPanels`, que cria os painéis, guarda os serializadores por `viewType` e
//! responde ao protocolo da thread principal (`$onDidChangeWebviewPanelViewStates`, `$onDidDisposeWebviewPanel`,
//! `$deserializeWebviewPanel`).
//!
//! Divergências:
//! - Os setters e getters do `vscode.WebviewPanel` são métodos. Os que lançam `Webview is disposed` (`webview`, `view_type`,
//!   `title`, `set_title`, `icon_path`, `set_icon_path`, `view_column`, `active`, `visible`, `reveal`) devolvem [`Result`].
//! - `vscode.WebviewPanelSerializer` é o trait [`WebviewPanelSerializer`]; o serializador registrado é guardado como `Rc`, e a
//!   identidade que o original compara (`this`) é a do `Rc`. `registerWebviewPanelSerializer` lança (viewType já
//!   registrado): devolve [`Result`], com o `vscode.Disposable` de `ext_host_types`.
//! - `ViewColumn | { viewColumn; preserveFocus? }` do `showOptions` de `createWebviewPanel` é [`WebviewShowOptions`], e a
//!   interseção `WebviewPanelOptions & WebviewOptions` é [`CreateWebviewPanelOptions`] (as duas metades, separadas).
//! - `iconPath` (`vscode.IconPath`) é a [`IconPath`] de `ext_host_types`, comparada por valor: as URIs e o `ThemeIcon` têm
//!   igualdade por valor, e o `!==` do original comparava a identidade do objeto (dois objetos iguais contam como
//!   mudança no original; aqui, não).
//! - `$onDidChangeWebviewPanelViewStates` ordena os handles com o comparador do original (`active` por último, o resto
//!   por `visible`) por inserção estável, porque o comparador não é uma ordem total quando dois painéis estão ativos e o
//!   `sort_by` do Rust pode recusá-lo.
//! - `ExtHostWebviewPanel extends Disposable` é um campo [`Disposable`] com os dois emissores registrados.
//! - `ViewColumn.to(position)` lança numa posição que não é coluna: em `_updateViewState` isso é o `Err` do
//!   `$onDidChangeWebviewPanelViewStates`... que não devolve nada, então o erro vai ao [`errors::on_unexpected_error`] e o
//!   painel não é atualizado. `$deserializeWebviewPanel` devolve o erro como promessa rejeitada.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_protocol::{
    EditorGroupColumn, ExtHostWebviewPanelsShape, IMainContext, MainContext, MainThreadWebviewPanelsShape, WebviewDeserializeInitData, WebviewIconPathDto,
    WebviewInitData, WebviewPanelOptions, WebviewPanelShowOptions, WebviewPanelViewState, WebviewPanelViewStateData, WebviewSerializerOptions,
};
use super::ext_host_type_converters::ViewColumn;
use super::ext_host_types::{Disposable as ApiDisposable, IconPath};
use super::ext_host_webview::{
    ExtHostWebview, ExtHostWebviews, IExtHostWorkspaceFolders, WebviewOptions, serialize_webview_options, should_serialize_buffers_for_post_message,
    to_extension_data,
};

/// `vscode.WebviewPanelOnDidChangeViewStateEvent`: só o painel que mudou.
#[derive(Clone)]
pub struct WebviewPanelOnDidChangeViewStateEvent {
    pub webview_panel: Rc<ExtHostWebviewPanel>,
}

/// O `{ active, visible, viewColumn }` de `_updateViewState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WebviewPanelNewViewState {
    pub active: bool,
    pub visible: bool,
    pub view_column: f64,
}

pub struct ExtHostWebviewPanel {
    base: Rc<Disposable>,
    this: std::rc::Weak<ExtHostWebviewPanel>,
    handle: String,
    proxy: Rc<dyn MainThreadWebviewPanelsShape>,
    view_type: String,

    webview: Rc<ExtHostWebview>,
    options: WebviewPanelOptions,

    title: RefCell<String>,
    icon_path: RefCell<Option<IconPath>>,
    view_column: Cell<Option<f64>>,
    visible: Cell<bool>,
    active: Cell<bool>,
    is_disposed: Cell<bool>,

    on_did_dispose_emitter: Rc<Emitter<()>>,
    on_did_change_view_state_emitter: Rc<Emitter<WebviewPanelOnDidChangeViewStateEvent>>,
}

/// Os parâmetros de `new ExtHostWebviewPanel(handle, proxy, webview, params)`.
pub struct ExtHostWebviewPanelParams {
    pub view_type: String,
    pub title: String,
    pub view_column: Option<f64>,
    pub panel_options: WebviewPanelOptions,
    pub active: bool,
}

impl ExtHostWebviewPanel {
    pub fn new(handle: &str, proxy: Rc<dyn MainThreadWebviewPanelsShape>, webview: Rc<ExtHostWebview>, params: ExtHostWebviewPanelParams) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_dispose_emitter = Emitter::new();
        base.register(on_did_dispose_emitter.clone());
        let on_did_change_view_state_emitter = Emitter::new();
        base.register(on_did_change_view_state_emitter.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            handle: handle.to_owned(),
            proxy,
            view_type: params.view_type,
            webview,
            options: params.panel_options,
            title: RefCell::new(params.title),
            icon_path: RefCell::new(None),
            view_column: Cell::new(params.view_column),
            visible: Cell::new(true),
            active: Cell::new(params.active),
            is_disposed: Cell::new(false),
            on_did_dispose_emitter,
            on_did_change_view_state_emitter,
        })
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose_emitter.event()
    }

    /// `onDidChangeViewState`.
    pub fn on_did_change_view_state(&self) -> Event<WebviewPanelOnDidChangeViewStateEvent> {
        self.on_did_change_view_state_emitter.event()
    }

    /// `webview`.
    pub fn webview(&self) -> Result<Rc<ExtHostWebview>, Error> {
        self.assert_not_disposed()?;
        Ok(self.webview.clone())
    }

    /// `viewType`.
    pub fn view_type(&self) -> Result<String, Error> {
        self.assert_not_disposed()?;
        Ok(self.view_type.clone())
    }

    /// `title` (getter).
    pub fn title(&self) -> Result<String, Error> {
        self.assert_not_disposed()?;
        Ok(self.title.borrow().clone())
    }

    /// `title` (setter).
    pub fn set_title(&self, value: &str) -> Result<(), Error> {
        self.assert_not_disposed()?;
        if *self.title.borrow() != value {
            *self.title.borrow_mut() = value.to_owned();
            self.proxy.rpc_set_title(&self.handle, value);
        }
        Ok(())
    }

    /// `iconPath` (getter).
    pub fn icon_path(&self) -> Result<Option<IconPath>, Error> {
        self.assert_not_disposed()?;
        Ok(self.icon_path.borrow().clone())
    }

    /// `iconPath` (setter).
    pub fn set_icon_path(&self, value: Option<IconPath>) -> Result<(), Error> {
        self.assert_not_disposed()?;
        if !icon_path_equals(&self.icon_path.borrow(), &value) {
            *self.icon_path.borrow_mut() = value.clone();

            let dto = value.map(|value| match value {
                IconPath::Uri(uri) => WebviewIconPathDto::LightDark { light: uri.clone(), dark: uri },
                IconPath::LightDark { light, dark } => WebviewIconPathDto::LightDark { light, dark },
                IconPath::ThemeIcon(icon) => WebviewIconPathDto::ThemeIcon(icon),
            });
            self.proxy.rpc_set_icon_path(&self.handle, dto);
        }
        Ok(())
    }

    /// `options`.
    pub fn options(&self) -> WebviewPanelOptions {
        self.options
    }

    /// `viewColumn`.
    pub fn view_column(&self) -> Result<Option<f64>, Error> {
        self.assert_not_disposed()?;
        Ok(self.view_column_value())
    }

    /// O `viewColumn` sem a conferência de descarte (o `_updateViewState` lê o getter só depois de checar `#isDisposed`).
    fn view_column_value(&self) -> Option<f64> {
        match self.view_column.get() {
            // We are using a symbolic view column
            // Return undefined instead to indicate that the real view column is currently unknown but will be resolved.
            Some(view_column) if view_column < 0.0 => None,
            other => other,
        }
    }

    /// `active`.
    pub fn active(&self) -> Result<bool, Error> {
        self.assert_not_disposed()?;
        Ok(self.active.get())
    }

    /// `visible`.
    pub fn visible(&self) -> Result<bool, Error> {
        self.assert_not_disposed()?;
        Ok(self.visible.get())
    }

    /// `_updateViewState(newState)`.
    pub fn update_view_state(&self, new_state: WebviewPanelNewViewState) {
        if self.is_disposed.get() {
            return;
        }

        if self.active.get() != new_state.active || self.visible.get() != new_state.visible || self.view_column_value() != Some(new_state.view_column) {
            self.active.set(new_state.active);
            self.visible.set(new_state.visible);
            self.view_column.set(Some(new_state.view_column));
            if let Some(panel) = self.this.upgrade() {
                self.on_did_change_view_state_emitter.fire(WebviewPanelOnDidChangeViewStateEvent { webview_panel: panel });
            }
        }
    }

    /// `reveal(viewColumn?, preserveFocus?)`.
    pub fn reveal(&self, view_column: Option<f64>, preserve_focus: Option<bool>) -> Result<(), Error> {
        self.assert_not_disposed()?;
        self.proxy.rpc_reveal(
            &self.handle,
            WebviewPanelShowOptions { view_column: view_column.map(|view_column| ViewColumn::from(Some(view_column))), preserve_focus: Some(preserve_focus.unwrap_or(false)) },
        );
        Ok(())
    }

    fn assert_not_disposed(&self) -> Result<(), Error> {
        if self.is_disposed.get() {
            return Err(Error::new("Webview is disposed"));
        }
        Ok(())
    }
}

impl IDisposable for ExtHostWebviewPanel {
    fn dispose(&self) {
        if self.is_disposed.get() {
            return;
        }

        self.is_disposed.set(true);
        self.on_did_dispose_emitter.fire(());

        self.proxy.rpc_dispose_webview(&self.handle);
        self.webview.dispose();

        self.base.dispose();
    }
}

/// `vscode.WebviewPanelSerializer`.
pub trait WebviewPanelSerializer {
    /// `deserializeWebviewPanel(webviewPanel, state)`.
    fn deserialize_webview_panel(&self, webview_panel: Rc<ExtHostWebviewPanel>, state: Dynamic) -> Promise<()>;
}

/// `ViewColumn | { viewColumn: ViewColumn; preserveFocus?: boolean }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WebviewShowOptions {
    ViewColumn(f64),
    Options { view_column: f64, preserve_focus: Option<bool> },
}

/// `vscode.WebviewPanelOptions & vscode.WebviewOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreateWebviewPanelOptions {
    pub panel: WebviewPanelOptions,
    pub webview: WebviewOptions,
}

struct SerializerEntry {
    serializer: Rc<dyn WebviewPanelSerializer>,
    extension: ExtensionDescription,
}

pub struct ExtHostWebviewPanels {
    this: std::rc::Weak<ExtHostWebviewPanels>,
    base: Rc<Disposable>,
    proxy: Rc<dyn MainThreadWebviewPanelsShape>,

    webview_panels: RefCell<HashMap<String, Rc<ExtHostWebviewPanel>>>,
    serializers: RefCell<HashMap<String, Rc<SerializerEntry>>>,

    webviews: Rc<ExtHostWebviews>,
    workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>,
}

impl ExtHostWebviewPanels {
    pub fn new(main_context: &dyn IMainContext, webviews: Rc<ExtHostWebviews>, workspace: Option<Rc<dyn IExtHostWorkspaceFolders>>) -> Rc<Self> {
        let proxy = main_context.get_proxy(&MainContext::MAIN_THREAD_WEBVIEW_PANELS);
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            proxy,
            webview_panels: RefCell::new(HashMap::new()),
            serializers: RefCell::new(HashMap::new()),
            webviews,
            workspace,
        })
    }

    /// `newHandle()`.
    fn new_handle() -> String {
        generate_uuid()
    }

    /// `createWebviewPanel(extension, viewType, title, showOptions, options)`.
    pub fn create_webview_panel(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        title: &str,
        show_options: WebviewShowOptions,
        options: CreateWebviewPanelOptions,
    ) -> Rc<ExtHostWebviewPanel> {
        let (view_column, preserve_focus) = match show_options {
            WebviewShowOptions::Options { view_column, preserve_focus } => (view_column, preserve_focus.unwrap_or(false)),
            WebviewShowOptions::ViewColumn(view_column) => (view_column, false),
        };
        let webview_show_options = WebviewPanelShowOptions { view_column: Some(ViewColumn::from(Some(view_column))), preserve_focus: Some(preserve_focus) };

        let serialize_buffers_for_post_message = should_serialize_buffers_for_post_message(extension);
        let handle = Self::new_handle();
        self.proxy.rpc_create_webview_panel(
            to_extension_data(extension),
            &handle,
            view_type,
            WebviewInitData {
                title: title.to_owned(),
                panel_options: serialize_webview_panel_options(&options.panel),
                webview_options: serialize_webview_options(extension, self.workspace.as_deref(), &options.webview),
                serialize_buffers_for_post_message,
            },
            webview_show_options,
        );

        let webview = self.webviews.create_new_webview(&handle, &options.webview, extension);
        self.create_new_webview_panel(&handle, view_type, title, Some(view_column), options.panel, webview, true)
    }

    /// `registerWebviewPanelSerializer(extension, viewType, serializer)`.
    pub fn register_webview_panel_serializer(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        serializer: Rc<dyn WebviewPanelSerializer>,
    ) -> Result<ApiDisposable, Error> {
        if self.serializers.borrow().contains_key(view_type) {
            return Err(Error::new(format!("Serializer for '{view_type}' already registered")));
        }

        self.serializers.borrow_mut().insert(view_type.to_owned(), Rc::new(SerializerEntry { serializer, extension: extension.clone() }));
        self.proxy
            .rpc_register_serializer(view_type, WebviewSerializerOptions { serialize_buffers_for_post_message: should_serialize_buffers_for_post_message(extension) });

        let this = self.this.clone();
        let view_type = view_type.to_owned();
        Ok(ApiDisposable::new(move || {
            if let Some(this) = this.upgrade() {
                this.serializers.borrow_mut().remove(&view_type);
                this.proxy.rpc_unregister_serializer(&view_type);
            }
        }))
    }

    /// `createNewWebviewPanel(webviewHandle, viewType, title, position, options, webview, active)`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_new_webview_panel(
        &self,
        webview_handle: &str,
        view_type: &str,
        title: &str,
        position: Option<f64>,
        options: WebviewPanelOptions,
        webview: Rc<ExtHostWebview>,
        active: bool,
    ) -> Rc<ExtHostWebviewPanel> {
        let panel = ExtHostWebviewPanel::new(
            webview_handle,
            self.proxy.clone(),
            webview,
            ExtHostWebviewPanelParams { view_type: view_type.to_owned(), title: title.to_owned(), view_column: position, panel_options: options, active },
        );
        self.webview_panels.borrow_mut().insert(webview_handle.to_owned(), panel.clone());
        panel
    }

    /// `getWebviewPanel(handle)`.
    pub fn get_webview_panel(&self, handle: &str) -> Option<Rc<ExtHostWebviewPanel>> {
        self.webview_panels.borrow().get(handle).cloned()
    }
}

impl IDisposable for ExtHostWebviewPanels {
    fn dispose(&self) {
        self.base.dispose();

        let panels: Vec<Rc<ExtHostWebviewPanel>> = self.webview_panels.borrow().values().cloned().collect();
        for panel in panels {
            panel.dispose();
        }
        self.webview_panels.borrow_mut().clear();
    }
}

impl ExtHostWebviewPanelsShape for ExtHostWebviewPanels {
    fn rpc_on_did_change_webview_panel_view_states(&self, new_states: WebviewPanelViewStateData) {
        // Notify webviews of state changes in the following order:
        // - Non-visible
        // - Visible
        // - Active
        let mut handles: Vec<(String, WebviewPanelViewState)> = new_states;
        sort_view_states(&mut handles);

        for (handle, new_state) in handles {
            let Some(panel) = self.get_webview_panel(&handle) else {
                continue;
            };

            match errors::catch(|| ViewColumn::to(new_state.position)) {
                Ok(view_column) => panel.update_view_state(WebviewPanelNewViewState { active: new_state.active, visible: new_state.visible, view_column }),
                Err(error) => errors::on_unexpected_error(&error),
            }
        }
    }

    fn rpc_on_did_dispose_webview_panel(&self, handle: &str) -> Promise<()> {
        let panel = self.get_webview_panel(handle);
        if let Some(panel) = panel {
            panel.dispose();
        }

        self.webview_panels.borrow_mut().remove(handle);
        self.webviews.delete_webview(handle);
        Promise::resolved(())
    }

    fn rpc_deserialize_webview_panel(&self, webview_handle: &str, view_type: &str, init_data: WebviewDeserializeInitData, position: EditorGroupColumn) -> Promise<()> {
        let entry = self.serializers.borrow().get(view_type).cloned();
        let Some(entry) = entry else {
            return Promise::rejected(Error::new(format!("No serializer found for '{view_type}'")));
        };

        let webview = self.webviews.create_new_webview(webview_handle, &init_data.webview_options, &entry.extension);
        let revived_panel =
            self.create_new_webview_panel(webview_handle, view_type, &init_data.title, Some(position), init_data.panel_options, webview, init_data.active);
        let serializer = entry.serializer.clone();
        serializer.deserialize_webview_panel(revived_panel, init_data.state)
    }
}

/// O comparador de `handles.sort(...)` do original, aplicado por inserção estável.
fn sort_view_states(handles: &mut [(String, WebviewPanelViewState)]) {
    // `(stateA, stateB) => number`: positivo quando A vem depois de B.
    let compare = |state_a: &WebviewPanelViewState, state_b: &WebviewPanelViewState| -> i32 {
        if state_a.active {
            return 1;
        }
        if state_b.active {
            return -1;
        }
        i32::from(state_a.visible) - i32::from(state_b.visible)
    };

    for index in 1..handles.len() {
        let mut current = index;
        while current > 0 && compare(&handles[current - 1].1, &handles[current].1) > 0 {
            handles.swap(current - 1, current);
            current -= 1;
        }
    }
}

/// O `!==` entre dois `iconPath` (ver a nota do módulo): igualdade por valor.
fn icon_path_equals(a: &Option<IconPath>, b: &Option<IconPath>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(IconPath::Uri(a)), Some(IconPath::Uri(b))) => a == b,
        (Some(IconPath::LightDark { light: light_a, dark: dark_a }), Some(IconPath::LightDark { light: light_b, dark: dark_b })) => {
            light_a == light_b && dark_a == dark_b
        }
        (Some(IconPath::ThemeIcon(a)), Some(IconPath::ThemeIcon(b))) => a == b,
        _ => false,
    }
}

/// `serializeWebviewPanelOptions(options)`.
fn serialize_webview_panel_options(options: &WebviewPanelOptions) -> WebviewPanelOptions {
    WebviewPanelOptions { enable_find_widget: options.enable_find_widget, retain_context_when_hidden: options.retain_context_when_hidden }
}
