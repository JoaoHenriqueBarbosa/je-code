//! Porte de `src/vs/workbench/api/browser/mainThreadOutputService.ts`: a ponte entre os canais de saída que as extensões criam
//! (`window.createOutputChannel`) e o `IOutputService` do workbench (registro, atualização, revelar, fechar e descartar), com o
//! indicador na barra de status quando o painel de saída está configurado para abrir em silêncio.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadOutputService)` e o `IExtHostContext` do construtor: o construtor recebe o
//!   [`IMainContext`] e pede o proxy de `ExtHostOutputService`.
//! - `IViewsService` (`isViewVisible`, `closeView`, `onDidChangeViewVisibility`) e `IStatusbarService` (`addEntry`) ainda não existem no
//!   porte com esses membros (o [`IViewsService`](crate::services::views::views_service::IViewsService) só tem `openView`, e o serviço
//!   de barra de status não está portado): ficam como os traits estreitos [`IOutputViews`] e [`IOutputStatusbar`], com o que este
//!   arquivo lê, e o aplicativo os implementa sobre os serviços reais.
//! - O `static _extensionIdPool` é um mapa `thread_local` (um por thread, o processo do host é uma só).
//! - `Registry.as<IOutputChannelRegistry>(...)` é [`output_channel_registry`](crate::services::output::output::output_channel_registry).
//! - `async` de `$register`, `$update`, `$reveal`, `$close` e `$dispose` são promessas já resolvidas (nenhum corpo espera algo).
//! - A leitura de `workbench.view.showQuietly` devolve um [`Dynamic`] (objeto de chave `string` para `boolean`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostOutputServiceShape, IMainContext, MainThreadOutputServiceShape};
use crate::services::output::output::{
    output_channel_registry, IOutputChannel, IOutputChannelDescriptor, IOutputChannelRegistry, IOutputContentSource, IOutputService, OutputChannelSource, OutputChannelUpdateMode,
    OUTPUT_VIEW_ID,
};

/// O que `MainThreadOutputService` lê do `IViewsService`.
pub trait IOutputViews {
    fn is_view_visible(&self, id: &str) -> bool;
    fn close_view(&self, id: &str);
    /// `onDidChangeViewVisibility`: o id da vista cuja visibilidade mudou.
    fn on_did_change_view_visibility(&self) -> Event<String>;
}

/// O `IStatusbarEntry` do indicador (`kind: 'prominent'`).
#[derive(Clone, Debug, PartialEq)]
pub struct OutputStatusEntry {
    pub name: String,
    pub text: String,
    pub aria_label: String,
    pub command: String,
    pub tooltip: String,
}

/// `IStatusbarEntryAccessor`.
pub trait IOutputStatusEntryAccessor: IDisposable {
    fn update(&self, entry: OutputStatusEntry);
}

/// O que `MainThreadOutputService` usa do `IStatusbarService`: `addEntry(entry, 'status.view.showQuietly', RIGHT, { location:
/// { id: 'status.notifications', priority: -Infinity }, alignment: LEFT })`.
pub trait IOutputStatusbar {
    fn add_quiet_output_entry(&self, entry: OutputStatusEntry) -> Rc<dyn IOutputStatusEntryAccessor>;
}

thread_local! {
    /// `MainThreadOutputService._extensionIdPool`.
    static EXTENSION_ID_POOL: RefCell<HashMap<String, u32>> = RefCell::new(HashMap::new());
}

/// `MainThreadOutputService`.
pub struct MainThreadOutputService {
    base: Rc<Disposable>,
    output_service: Rc<dyn IOutputService>,
    views_service: Rc<dyn IOutputViews>,
    configuration_service: Rc<dyn IConfigurationService>,
    statusbar_service: Rc<dyn IOutputStatusbar>,
    output_status_item: RefCell<Option<Rc<dyn IOutputStatusEntryAccessor>>>,
}

impl MainThreadOutputService {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        output_service: Rc<dyn IOutputService>,
        views_service: Rc<dyn IOutputViews>,
        configuration_service: Rc<dyn IConfigurationService>,
        statusbar_service: Rc<dyn IOutputStatusbar>,
    ) -> Rc<Self> {
        let proxy: Rc<dyn ExtHostOutputServiceShape> = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_OUTPUT_SERVICE);
        let this = Rc::new(Self { base: Disposable::new(), output_service, views_service, configuration_service, statusbar_service, output_status_item: RefCell::new(None) });

        let set_visible_channel: Rc<dyn Fn()> = {
            let (weak, proxy) = (Rc::downgrade(&this), proxy.clone());
            Rc::new(move || {
                let Some(this) = weak.upgrade() else { return };
                let visible_channel = if this.views_service.is_view_visible(OUTPUT_VIEW_ID) { this.output_service.get_active_channel() } else { None };
                proxy.rpc_set_visible_channel(visible_channel.map(|channel| channel.id()));
                if let Some(item) = this.output_status_item.borrow_mut().take() {
                    item.dispose();
                }
            })
        };
        // `Event.any(onActiveOutputChannel, Event.filter(onDidChangeViewVisibility, ({ id }) => id === OUTPUT_VIEW_ID))`
        {
            let callback = set_visible_channel.clone();
            this.base.register(this.output_service.on_active_output_channel().listen(move |_| callback()));
        }
        {
            let callback = set_visible_channel.clone();
            this.base.register(this.views_service.on_did_change_view_visibility().listen(move |id| {
                if id == OUTPUT_VIEW_ID {
                    callback();
                }
            }));
        }
        set_visible_channel();
        this
    }

    fn get_channel(&self, channel_id: &str) -> Option<Rc<dyn IOutputChannel>> {
        self.output_service.get_channel(channel_id)
    }

    /// Show status bar indicator.
    fn show_channel_quietly(&self, channel: &dyn IOutputChannel) {
        let label = channel.label();
        let status_properties = OutputStatusEntry {
            name: localize("status.showOutput", "Show Output", &[]),
            text: "$(output)".to_owned(),
            aria_label: localize("status.showOutputAria", "Show {0} Output Channel", &[label.as_str()]),
            command: format!("workbench.action.output.show.{}", channel.id()),
            tooltip: localize("status.showOutputTooltip", "Show {0} Output Channel", &[label.as_str()]),
        };
        let existing = self.output_status_item.borrow().clone();
        match existing {
            None => *self.output_status_item.borrow_mut() = Some(self.statusbar_service.add_quiet_output_entry(status_properties)),
            Some(item) => item.update(status_properties),
        }
    }
}

impl IDisposable for MainThreadOutputService {
    fn dispose(&self) {
        if let Some(item) = self.output_status_item.borrow_mut().take() {
            item.dispose();
        }
        self.base.dispose();
    }
}

impl MainThreadOutputServiceShape for MainThreadOutputService {
    fn rpc_register(&self, label: &str, file: URI, language_id: Option<String>, extension_id: &str) -> Promise<String> {
        let id_counter = EXTENSION_ID_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            let next = pool.get(extension_id).copied().unwrap_or(0) + 1;
            pool.insert(extension_id.to_owned(), next);
            next
        });
        let id = format!("extension-output-{extension_id}-#{id_counter}-{label}");

        output_channel_registry().register_channel(IOutputChannelDescriptor {
            id: id.clone(),
            label: label.to_owned(),
            log: false,
            language_id,
            source: Some(OutputChannelSource::Single(IOutputContentSource { name: None, resource: file })),
            extension_id: Some(extension_id.to_owned()),
            user: None,
        });
        let output_service = self.output_service.clone();
        let to_release = id.clone();
        let release: DisposableRef = to_disposable(move || {
            if let Some(channel) = output_service.get_channel(&to_release) {
                channel.dispose();
            }
        });
        self.base.register(release);
        Promise::resolved(id)
    }

    fn rpc_update(&self, channel_id: &str, mode: OutputChannelUpdateMode, till: Option<u64>) -> Promise<()> {
        if let Some(channel) = self.get_channel(channel_id) {
            if mode == OutputChannelUpdateMode::Append {
                channel.update(mode, None);
            } else if till.is_some() {
                channel.update(mode, till);
            }
        }
        Promise::resolved(())
    }

    fn rpc_reveal(&self, channel_id: &str, preserve_focus: bool) -> Promise<()> {
        let Some(channel) = self.get_channel(channel_id) else { return Promise::resolved(()) };

        let views_to_show_quietly = self.configuration_service.get_value(Some("workbench.view.showQuietly"), None);
        let quiet = matches!(views_to_show_quietly, Dynamic::Object(_)) && matches!(views_to_show_quietly.get_str(OUTPUT_VIEW_ID), Dynamic::Bool(true));
        if !self.views_service.is_view_visible(OUTPUT_VIEW_ID) && quiet {
            self.show_channel_quietly(&*channel);
            return Promise::resolved(());
        }

        let _ = self.output_service.show_channel(&channel.id(), preserve_focus);
        Promise::resolved(())
    }

    fn rpc_close(&self, channel_id: &str) -> Promise<()> {
        if self.views_service.is_view_visible(OUTPUT_VIEW_ID)
            && let Some(active_channel) = self.output_service.get_active_channel()
            && channel_id == active_channel.id()
        {
            self.views_service.close_view(OUTPUT_VIEW_ID);
        }
        Promise::resolved(())
    }

    fn rpc_dispose(&self, channel_id: &str) -> Promise<()> {
        if let Some(channel) = self.get_channel(channel_id) {
            channel.dispose();
        }
        Promise::resolved(())
    }
}
