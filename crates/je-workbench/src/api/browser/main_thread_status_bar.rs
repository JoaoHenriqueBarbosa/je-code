//! Porte de `src/vs/workbench/api/browser/mainThreadStatusBar.ts`: o lado do workbench do RPC `ExtHostStatusBar`, que
//! repassa as entradas que as extensões definem ao [`IExtensionStatusBarItemService`] e envia ao host de extensões as
//! entradas estáticas (as contribuídas pelo ponto de extensão `statusBarItems`).
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IExtensionStatusBarItemService)` é [`MainThreadStatusBar::new`], porque o
//!   serviço ainda não tem identificador registrado; `@extHostNamedCustomer(MainContext.MainThreadStatusBar)` é o registro
//!   do `extHostCustomers.ts`, que não foi portado.
//! - **`IExtensionStatusBarItemService`.** O serviço de `statusBarExtensionPoint.ts` (a implementação sobre
//!   `IStatusbarService`) não foi portado; aqui entram o trait, o [`StatusBarUpdateKind`] e o evento. O `IStatusbarEntry`
//!   (ainda não portado) é reduzido ao que o `MainThreadStatusBar` lê ([`ExtensionStatusBarEntry`]: `name`, `text`,
//!   `tooltip` em texto, o id do `command`, `ariaLabel` e `role`). `Command` é o [`ICommandDto`] (os mesmos campos).
//! - **`IManagedHoverTooltipMarkdownString`** (`{ markdown: (token) => Promise<...>, markdownNotSupportedFallback }`) é
//!   [`StatusBarTooltip::Provider`], sem o `markdownNotSupportedFallback` (sempre `undefined` aqui).
//! - O `Iterable<[string, {...}]>` de `getEntries` é um `Vec` de pares na ordem de inserção.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::themables::ThemeColor;
use je_editor::languages::StringOrMarkdown;
use je_platform::accessibility::IAccessibilityInformation;

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostStatusBarShape, ICommandDto, IMainContext, MainThreadStatusBarShape, StatusBarColor, StatusBarItemDto,
};
use crate::services::statusbar::statusbar::StatusbarAlignment;

/// `StatusBarUpdateKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarUpdateKind {
    DidDefine,
    DidUpdate,
}

/// O `tooltip` de `setOrUpdateEntry`: `IMarkdownString | string | undefined | IManagedHoverTooltipMarkdownString` (o
/// `undefined` é o `None` do `Option` que o envolve).
#[derive(Clone)]
pub enum StatusBarTooltip {
    Value(StringOrMarkdown),
    Provider(Rc<dyn Fn(CancellationToken) -> Promise<Option<StringOrMarkdown>>>),
}

/// O que `MainThreadStatusBar` lê de `IStatusbarEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionStatusBarEntry {
    pub name: String,
    pub text: String,
    pub tooltip: Option<String>,
    /// O id do comando (`typeof command === 'string' ? command : command.id`).
    pub command: Option<String>,
    pub aria_label: Option<String>,
    pub role: Option<String>,
}

/// O valor de `[entryId, { entry, alignment, priority }]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionStatusBarItem {
    pub entry: ExtensionStatusBarEntry,
    pub alignment: StatusbarAlignment,
    pub priority: f64,
}

/// `IExtensionStatusBarItemChangeEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionStatusBarItemChangeEvent {
    pub added: Option<(String, ExtensionStatusBarItem)>,
}

/// `IExtensionStatusBarItemService`.
pub trait IExtensionStatusBarItemService {
    fn on_did_change(&self) -> Event<ExtensionStatusBarItemChangeEvent>;

    #[allow(clippy::too_many_arguments)]
    fn set_or_update_entry(
        &self,
        id: &str,
        status_id: &str,
        extension_id: Option<&str>,
        name: &str,
        text: &str,
        tooltip: Option<StatusBarTooltip>,
        command: Option<ICommandDto>,
        color: Option<StatusBarColor>,
        background_color: Option<ThemeColor>,
        align_left: bool,
        priority: Option<f64>,
        accessibility_information: Option<IAccessibilityInformation>,
    ) -> StatusBarUpdateKind;

    fn unset_entry(&self, id: &str);

    fn get_entries(&self) -> Vec<(String, ExtensionStatusBarItem)>;
}

fn as_dto(entry_id: &str, item: &ExtensionStatusBarItem) -> StatusBarItemDto {
    StatusBarItemDto {
        entry_id: entry_id.to_owned(),
        name: item.entry.name.clone(),
        text: item.entry.text.clone(),
        tooltip: item.entry.tooltip.clone(),
        command: item.entry.command.clone(),
        priority: Some(item.priority),
        align_left: item.alignment == StatusbarAlignment::Left,
        accessibility_information: item.entry.aria_label.clone().map(|label| IAccessibilityInformation { label, role: item.entry.role.clone() }),
    }
}

/// `MainThreadStatusBar`.
pub struct MainThreadStatusBar {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostStatusBarShape>,
    entry_disposables: Rc<DisposableMap<String, dyn IDisposable>>,
    statusbar_service: Rc<dyn IExtensionStatusBarItemService>,
}

impl MainThreadStatusBar {
    pub fn new(ext_host_context: &dyn IMainContext, statusbar_service: Rc<dyn IExtensionStatusBarItemService>) -> Rc<Self> {
        let base = Disposable::new();
        let entry_disposables = DisposableMap::<String, dyn IDisposable>::new();
        base.register(entry_disposables.clone());
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_STATUS_BAR);

        // once, at startup read existing items and send them over
        let entries: Vec<StatusBarItemDto> = statusbar_service.get_entries().iter().map(|(entry_id, item)| as_dto(entry_id, item)).collect();

        proxy.rpc_accept_static_entries(entries);

        let listener_proxy = proxy.clone();
        base.register(statusbar_service.on_did_change().listen(move |event| {
            if let Some((entry_id, item)) = &event.added {
                listener_proxy.rpc_accept_static_entries(vec![as_dto(entry_id, item)]);
            }
        }));

        Rc::new(Self { base, proxy, entry_disposables, statusbar_service })
    }
}

impl IDisposable for MainThreadStatusBar {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadStatusBarShape for MainThreadStatusBar {
    #[allow(clippy::too_many_arguments)]
    fn rpc_set_entry(
        &self,
        entry_id: &str,
        id: &str,
        extension_id: Option<&str>,
        name: &str,
        text: &str,
        tooltip: Option<StringOrMarkdown>,
        has_tooltip_provider: bool,
        command: Option<ICommandDto>,
        color: Option<StatusBarColor>,
        background_color: Option<ThemeColor>,
        align_left: bool,
        priority: Option<f64>,
        accessibility_information: Option<IAccessibilityInformation>,
    ) {
        let tooltip_or_tooltip_provider = if has_tooltip_provider {
            let proxy = self.proxy.clone();
            let provider_entry_id = entry_id.to_owned();
            Some(StatusBarTooltip::Provider(Rc::new(move |cancellation| proxy.rpc_provide_tooltip(&provider_entry_id, cancellation))))
        } else {
            tooltip.map(StatusBarTooltip::Value)
        };

        let kind = self.statusbar_service.set_or_update_entry(
            entry_id,
            id,
            extension_id,
            name,
            text,
            tooltip_or_tooltip_provider,
            command,
            color,
            background_color,
            align_left,
            priority,
            accessibility_information,
        );
        if kind == StatusBarUpdateKind::DidDefine {
            let statusbar_service = self.statusbar_service.clone();
            let entry_to_unset = entry_id.to_owned();
            let disposable = to_disposable(move || statusbar_service.unset_entry(&entry_to_unset));
            self.entry_disposables.set(entry_id.to_owned(), disposable, false);
        }
    }

    fn rpc_dispose_entry(&self, entry_id: &str) {
        self.entry_disposables.delete_and_dispose(&entry_id.to_owned());
    }
}
