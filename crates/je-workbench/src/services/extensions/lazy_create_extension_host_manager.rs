//! Porte de `src/vs/workbench/services/extensions/common/lazyCreateExtensionHostManager.ts`: o `LazyCreateExtensionHostManager`, que
//! espera o `start()` e só então, se houver extensões para o host, cria o `ExtensionHostManager` de verdade.
//!
//! Divergências:
//! - `@IInstantiationService` e `@ILogService` são os serviços de [`ExtensionHostManagerServices`] (o `createInstance(ExtensionHostManager,
//!   ...)` é [`ExtensionHostManager::new`]).
//! - **`extends Disposable`.** O gerenciador guarda um [`Disposable`] e os métodos são `&self` com o `Weak` do próprio objeto; os
//!   `async` do original são [`async_fn`].
//! - `this._actual?.getInspectPort(...)` resolve `undefined` (`None`) quando ainda não há gerenciador de verdade.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{Barrier, async_fn};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::Logger;
use je_platform::remote::remote_authority_resolver::RemoteAuthorityResolverErrorCode;

use super::extension_activation::ExtensionActivationReason;
use super::extension_description_registry::SharedExtensionDescription;
use super::extension_host_kind::ExtensionHostKind;
use super::extension_host_manager::{ExtensionHostManager, ExtensionHostManagerServices, friendly_ext_host_name};
use super::extension_host_managers::IExtensionHostManager;
use super::extension_host_protocol::IExtensionDescriptionDelta;
use super::extension_host_proxy::{IResolveAuthorityResult, ResolveAuthorityError};
use super::extension_running_location::ExtensionRunningLocation;
use super::extension_running_location_tracker::IRepresentsRunningLocation;
use super::extensions::{ActivationKind, ExtensionHostStartup, IExtensionHost, IExtensionInspectInfo, IInternalExtensionService};
use super::rpc_protocol::ResponsiveState;

/// Waits until `start()` and only if it has extensions proceeds to really start.
pub struct LazyCreateExtensionHostManager {
    me: Weak<LazyCreateExtensionHostManager>,
    base: Rc<Disposable>,
    on_did_change_responsive_state: Rc<Emitter<ResponsiveState>>,

    extension_host: Rc<dyn IExtensionHost>,
    initial_activation_events: Vec<String>,
    internal_extension_service: Rc<dyn IInternalExtensionService>,
    services: ExtensionHostManagerServices,
    start_called: Barrier,
    actual: RefCell<Option<Rc<ExtensionHostManager>>>,
}

fn join_ids(ids: &[ExtensionIdentifier]) -> String {
    ids.iter().map(|id| id.value.as_str()).collect::<Vec<_>>().join(",")
}

impl LazyCreateExtensionHostManager {
    pub fn new(
        extension_host: Rc<dyn IExtensionHost>,
        initial_activation_events: Vec<String>,
        internal_extension_service: Rc<dyn IInternalExtensionService>,
        services: ExtensionHostManagerServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_responsive_state = Emitter::new();
        base.register(on_did_change_responsive_state.clone());
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_did_change_responsive_state,
            extension_host,
            initial_activation_events,
            internal_extension_service,
            services,
            start_called: Barrier::new(),
            actual: RefCell::new(None),
        })
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o gerenciador está vivo enquanto um método dele roda")
    }

    fn actual(&self) -> Option<Rc<ExtensionHostManager>> {
        self.actual.borrow().clone()
    }

    fn create_actual(&self, reason: &str) -> Rc<ExtensionHostManager> {
        self.services.log_service.info(&format!("Creating lazy extension host ({}). Reason: {reason}", self.friendly_name()));
        let actual = ExtensionHostManager::new(
            self.extension_host.clone(),
            self.initial_activation_events.clone(),
            self.internal_extension_service.clone(),
            self.services.clone(),
        );
        *self.actual.borrow_mut() = Some(actual.clone());
        self.base.register(actual.clone());
        let weak = self.me.clone();
        self.base.register(actual.on_did_change_responsive_state().listen(move |state: &ResponsiveState| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_responsive_state.fire(*state);
            }
        }));
        actual
    }

    fn get_or_create_actual_and_start(&self, reason: &str) -> Promise<Rc<ExtensionHostManager>> {
        if let Some(actual) = self.actual() {
            // already created/started
            return Promise::resolved(actual);
        }
        let actual = self.create_actual(reason);
        async_fn(async move {
            actual.ready().await?;
            Ok(actual)
        })
    }
}

impl IRepresentsRunningLocation for LazyCreateExtensionHostManager {
    fn represents_running_location(&self, running_location: &ExtensionRunningLocation) -> bool {
        self.extension_host.running_location().equals(running_location)
    }
}

impl IDisposable for LazyCreateExtensionHostManager {
    fn dispose(&self) {
        if self.actual().is_none() {
            self.extension_host.dispose();
        }
        self.base.dispose();
    }
}

impl IExtensionHostManager for LazyCreateExtensionHostManager {
    fn pid(&self) -> Option<u32> {
        self.actual().and_then(|actual| actual.pid())
    }

    fn kind(&self) -> ExtensionHostKind {
        self.extension_host.running_location().kind()
    }

    fn startup(&self) -> ExtensionHostStartup {
        self.extension_host.startup()
    }

    fn friendly_name(&self) -> String {
        friendly_ext_host_name(self.kind(), self.pid())
    }

    fn on_did_exit(&self) -> Event<(i32, Option<String>)> {
        self.extension_host.on_exit()
    }

    fn on_did_change_responsive_state(&self) -> Event<ResponsiveState> {
        self.on_did_change_responsive_state.event()
    }

    fn disconnect(&self) -> Promise<()> {
        match self.actual() {
            Some(actual) => actual.disconnect(),
            None => Promise::resolved(()),
        }
    }

    fn is_ready(&self) -> bool {
        self.start_called.is_open() && self.actual().is_some_and(|actual| actual.is_ready())
    }

    fn ready(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                actual.ready().await?;
            }
            Ok(())
        })
    }

    fn delta_extensions(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                return actual.delta_extensions(extensions_delta).await;
            }
            if !extensions_delta.my_to_add.is_empty() {
                let actual = this.create_actual(&format!(
                    "contains {} new extension(s) (installed or enabled): {}",
                    extensions_delta.my_to_add.len(),
                    join_ids(&extensions_delta.my_to_add)
                ));
                actual.ready().await?;
            }
            Ok(())
        })
    }

    fn contains_extension(&self, extension_id: &ExtensionIdentifier) -> bool {
        self.extension_host.extensions().is_some_and(|extensions| extensions.contains_extension(extension_id))
    }

    fn activate(&self, extension: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool> {
        let this = self.this();
        let extension = extension.clone();
        async_fn(async move {
            this.start_called.wait().await?;
            match this.actual() {
                Some(actual) => actual.activate(&extension, reason).await,
                None => Ok(false),
            }
        })
    }

    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()> {
        let this = self.this();
        let activation_event = activation_event.to_owned();
        async_fn(async move {
            if activation_kind == ActivationKind::Immediate {
                // this is an immediate request, so we cannot wait for start to be called
                if let Some(actual) = this.actual() {
                    return actual.activate_by_event(&activation_event, activation_kind).await;
                }
                return Ok(());
            }
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                return actual.activate_by_event(&activation_event, activation_kind).await;
            }
            Ok(())
        })
    }

    fn activation_event_is_done(&self, activation_event: &str) -> bool {
        if !self.start_called.is_open() {
            return false;
        }
        match self.actual() {
            Some(actual) => actual.activation_event_is_done(activation_event),
            None => true,
        }
    }

    fn get_inspect_port(&self, try_enable_inspector: bool) -> Promise<Option<IExtensionInspectInfo>> {
        let this = self.this();
        async_fn(async move {
            this.start_called.wait().await?;
            match this.actual() {
                Some(actual) => actual.get_inspect_port(try_enable_inspector).await,
                None => Ok(None),
            }
        })
    }

    fn resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult> {
        let this = self.this();
        let remote_authority = remote_authority.to_owned();
        async_fn(async move {
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                return actual.resolve_authority(&remote_authority, resolve_attempt).await;
            }
            Ok(IResolveAuthorityResult::Error(ResolveAuthorityError {
                message: Some("Cannot resolve authority".to_owned()),
                code: RemoteAuthorityResolverErrorCode::Unknown,
                detail: None,
            }))
        })
    }

    fn get_canonical_uri(&self, remote_authority: &str, uri: &URI) -> Promise<Option<URI>> {
        let this = self.this();
        let (remote_authority, uri) = (remote_authority.to_owned(), uri.clone());
        async_fn(async move {
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                return actual.get_canonical_uri(&remote_authority, &uri).await;
            }
            Err(Error::new("Cannot resolve canonical URI"))
        })
    }

    fn start(&self, _extension_registry_version_id: i64, _all_extensions: Vec<SharedExtensionDescription>, my_extensions: Vec<ExtensionIdentifier>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if !my_extensions.is_empty() {
                // there are actual extensions, so let's launch the extension host (auto-start)
                let actual = this.create_actual(&format!("contains {} extension(s): {}.", my_extensions.len(), join_ids(&my_extensions)));
                let result = actual.ready();
                this.start_called.open();
                return result.await;
            }
            // there are no actual extensions running
            this.start_called.open();
            Ok(())
        })
    }

    fn extension_tests_execute(&self) -> Promise<i32> {
        let this = self.this();
        async_fn(async move {
            this.start_called.wait().await?;
            let actual = this.get_or_create_actual_and_start("execute tests.").await?;
            actual.extension_tests_execute().await
        })
    }

    fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            this.start_called.wait().await?;
            if let Some(actual) = this.actual() {
                return actual.set_remote_environment(env).await;
            }
            Ok(())
        })
    }
}
