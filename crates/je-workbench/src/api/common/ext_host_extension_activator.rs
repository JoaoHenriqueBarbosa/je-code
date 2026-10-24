//! Porte de `src/vs/workbench/api/common/extHostExtensionActivator.ts`: o ativador de extensões do host
//! (`ExtensionsActivator`), com a resolução de dependências, os tempos de ativação e os resultados de ativação.
//!
//! Divergências:
//! - **Valores da fronteira com o JavaScript** são [`Dynamic`]: `vscode.ExtensionContext` (argumento de `activate`) e
//!   `IExtensionAPI` (o retorno de `activate`, interface vazia no original, e `exports`) cruzam como `Dynamic`. O
//!   `IExtensionModule` é o struct [`ExtensionModule`] com `activate` e `deactivate` opcionais.
//! - **Subclasses de `ActivatedExtension`**: `EmptyExtension`, `HostExtension` e `FailedExtension` só fixam parâmetros do
//!   construtor, então são as funções associadas [`ActivatedExtension::empty`], [`ActivatedExtension::host`] e
//!   [`ActivatedExtension::failed`] (a última é privada, como a classe do original).
//! - **`ExtensionActivationTimes`** guarda os tempos como `f64` (o `number` do JavaScript). `ExtensionActivationTimesFragment`
//!   é só um tipo de classificação de telemetria do TypeScript e não tem correspondente.
//! - **`Error | null`** é `Option<Error>`. O `(<any>error).detail = ...` do erro de dependência que falhou é a propriedade
//!   `detail` do `Error` do porte, que guarda texto (o `Error` de origem em texto, ou `null`).
//! - **`activateById`** lança de forma síncrona quando a extensão é desconhecida; o `throw` é o `Err` do resultado e, no
//!   `Ok`, vem a promessa. `getActivatedExtension` também devolve `Result`.
//! - **O que `actualActivateExtension` lança de forma síncrona** (o `throw new Error('Unexpected!')` de um dublê) é capturado
//!   com [`errors::catch`], como o `try` do original, que cobre a chamada e o `await`.
//! - **Registros**: o ativador recebe os [`ExtensionDescriptionRegistry`] concretos (o original chama `isHostExtension`
//!   estático com eles).
//! - **`async`**: cada método `async` do original é um [`async_fn`], que roda de forma síncrona até o primeiro `await`,
//!   como o motor JS (o teste do problema 144518 depende disso).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::{Barrier, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{self, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap};
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use crate::services::extensions::extension_activation::{ExtensionActivationReason, MissingExtensionDependency};
use crate::services::extensions::extension_description_registry::{
    ExtensionDescriptionRegistry, IReadOnlyExtensionDescriptionRegistry, has_main_or_browser,
};

/// `IExtensionModule.activate`: recebe o `ExtensionContext` e devolve a promessa da API da extensão.
pub type ExtensionActivateFn = Rc<dyn Fn(Dynamic) -> Promise<Dynamic>>;

/// `IExtensionModule.deactivate`.
pub type ExtensionDeactivateFn = Rc<dyn Fn()>;

/// `IExtensionModule`: o código-fonte (módulo) de uma extensão.
#[derive(Clone, Default)]
pub struct ExtensionModule {
    pub activate: Option<ExtensionActivateFn>,
    pub deactivate: Option<ExtensionDeactivateFn>,
}

/// `ExtensionActivationTimes`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtensionActivationTimes {
    pub startup: bool,
    pub code_loading_time: f64,
    pub activate_call_time: f64,
    pub activate_resolved_time: f64,
}

impl ExtensionActivationTimes {
    /// `ExtensionActivationTimes.NONE`.
    pub const NONE: Self = Self { startup: false, code_loading_time: -1.0, activate_call_time: -1.0, activate_resolved_time: -1.0 };

    pub fn new(startup: bool, code_loading_time: f64, activate_call_time: f64, activate_resolved_time: f64) -> Self {
        Self { startup, code_loading_time, activate_call_time, activate_resolved_time }
    }
}

/// `ExtensionActivationTimesBuilder`.
pub struct ExtensionActivationTimesBuilder {
    startup: bool,
    code_loading_start: f64,
    code_loading_stop: f64,
    activate_call_start: f64,
    activate_call_stop: f64,
    activate_resolve_start: f64,
    activate_resolve_stop: f64,
}

impl ExtensionActivationTimesBuilder {
    pub fn new(startup: bool) -> Self {
        Self {
            startup,
            code_loading_start: -1.0,
            code_loading_stop: -1.0,
            activate_call_start: -1.0,
            activate_call_stop: -1.0,
            activate_resolve_start: -1.0,
            activate_resolve_stop: -1.0,
        }
    }

    fn delta(start: f64, stop: f64) -> f64 {
        if start == -1.0 || stop == -1.0 {
            return -1.0;
        }
        stop - start
    }

    pub fn build(&self) -> ExtensionActivationTimes {
        ExtensionActivationTimes::new(
            self.startup,
            Self::delta(self.code_loading_start, self.code_loading_stop),
            Self::delta(self.activate_call_start, self.activate_call_stop),
            Self::delta(self.activate_resolve_start, self.activate_resolve_stop),
        )
    }

    fn now() -> f64 {
        runtime::date_now() as f64
    }

    pub fn code_loading_start(&mut self) {
        self.code_loading_start = Self::now();
    }

    pub fn code_loading_stop(&mut self) {
        self.code_loading_stop = Self::now();
    }

    pub fn activate_call_start(&mut self) {
        self.activate_call_start = Self::now();
    }

    pub fn activate_call_stop(&mut self) {
        self.activate_call_stop = Self::now();
    }

    pub fn activate_resolve_start(&mut self) {
        self.activate_resolve_start = Self::now();
    }

    pub fn activate_resolve_stop(&mut self) {
        self.activate_resolve_stop = Self::now();
    }
}

/// `ActivatedExtension`.
#[derive(Clone)]
pub struct ActivatedExtension {
    pub activation_failed: bool,
    pub activation_failed_error: Option<Error>,
    pub activation_times: ExtensionActivationTimes,
    pub module: ExtensionModule,
    pub exports: Option<Dynamic>,
    pub disposable: DisposableRef,
}

impl ActivatedExtension {
    pub fn new(
        activation_failed: bool,
        activation_failed_error: Option<Error>,
        activation_times: ExtensionActivationTimes,
        module: ExtensionModule,
        exports: Option<Dynamic>,
        disposable: DisposableRef,
    ) -> Self {
        Self { activation_failed, activation_failed_error, activation_times, module, exports, disposable }
    }

    /// `new EmptyExtension(activationTimes)`.
    pub fn empty(activation_times: ExtensionActivationTimes) -> Self {
        Self::new(false, None, activation_times, ExtensionModule::default(), None, lifecycle::none())
    }

    /// `new HostExtension()`.
    pub fn host() -> Self {
        Self::new(false, None, ExtensionActivationTimes::NONE, ExtensionModule::default(), None, lifecycle::none())
    }

    /// `new FailedExtension(activationError)`.
    fn failed(activation_error: Error) -> Self {
        Self::new(true, Some(activation_error), ExtensionActivationTimes::NONE, ExtensionModule::default(), None, lifecycle::none())
    }
}

/// `IExtensionsActivatorHost`.
pub trait IExtensionsActivatorHost {
    fn on_extension_activation_error(
        &self,
        extension_id: &ExtensionIdentifier,
        error: Option<&Error>,
        missing_extension_dependency: Option<&MissingExtensionDependency>,
    );

    fn actual_activate_extension(&self, extension_id: &ExtensionIdentifier, reason: &ExtensionActivationReason) -> Promise<Rc<ActivatedExtension>>;
}

/// `ActivationIdAndReason`.
#[derive(Clone)]
struct ActivationIdAndReason {
    id: ExtensionIdentifier,
    reason: ExtensionActivationReason,
}

/// `ExtensionsActivator`.
pub struct ExtensionsActivator {
    registry: Rc<ExtensionDescriptionRegistry>,
    global_registry: Rc<ExtensionDescriptionRegistry>,
    host: Rc<dyn IExtensionsActivatorHost>,
    log_service: LogServiceRef,
    operations: RefCell<ExtensionIdentifierMap<Rc<ActivationOperation>>>,
    /// A map of already activated events to speed things up if the same activation event is triggered multiple times.
    already_activated_events: RefCell<HashSet<String>>,
}

impl ExtensionsActivator {
    pub fn new(
        registry: Rc<ExtensionDescriptionRegistry>,
        global_registry: Rc<ExtensionDescriptionRegistry>,
        host: Rc<dyn IExtensionsActivatorHost>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        Rc::new(Self {
            registry,
            global_registry,
            host,
            log_service,
            operations: RefCell::new(ExtensionIdentifierMap::new()),
            already_activated_events: RefCell::new(HashSet::new()),
        })
    }

    /// `waitForActivatingExtensions()`.
    pub fn wait_for_activating_extensions(&self) -> Promise<()> {
        let res: Vec<Promise<bool>> = self.operations.borrow().values().map(|op| op.wait()).collect();
        async_fn(async move {
            Promise::all(res).await?;
            Ok(())
        })
    }

    /// `isActivated(extensionId)`.
    pub fn is_activated(&self, extension_id: &str) -> bool {
        self.operations.borrow().get(extension_id).is_some_and(|op| op.value().is_some())
    }

    /// `getActivatedExtension(extensionId)`: o `throw` do original é o `Err`.
    pub fn get_activated_extension(&self, extension_id: &str) -> Result<Rc<ActivatedExtension>, Error> {
        let op = self.operations.borrow().get(extension_id).cloned();
        match op.and_then(|op| op.value()) {
            Some(value) => Ok(value),
            None => Err(Error::new(format!("Extension '{extension_id}' is not known or not activated"))),
        }
    }

    /// `activateByEvent(activationEvent, startup)`.
    pub fn activate_by_event(self: &Rc<Self>, activation_event: &str, startup: bool) -> Promise<()> {
        let this = self.clone();
        let activation_event = activation_event.to_owned();
        async_fn(async move {
            if this.already_activated_events.borrow().contains(&activation_event) {
                return Ok(());
            }

            let activate_extensions = this.registry.get_extension_descriptions_for_activation_event(&activation_event);
            let requests: Vec<ActivationIdAndReason> = activate_extensions
                .iter()
                .map(|e| ActivationIdAndReason {
                    id: e.identifier.clone(),
                    reason: ExtensionActivationReason { startup, extension_id: e.identifier.clone(), activation_event: activation_event.clone() },
                })
                .collect();
            this.activate_extensions(requests).await?;

            this.already_activated_events.borrow_mut().insert(activation_event);
            Ok(())
        })
    }

    /// `activateById(extensionId, reason)`: a extensão desconhecida lança de forma síncrona (o `Err`).
    pub fn activate_by_id(self: &Rc<Self>, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Result<Promise<()>, Error> {
        let Some(desc) = self.registry.get_extension_description(&extension_id.value) else {
            return Err(Error::new(format!("Extension '{}' is not known", extension_id.value)));
        };
        Ok(self.activate_extensions(vec![ActivationIdAndReason { id: desc.identifier.clone(), reason }]))
    }

    fn activate_extensions(self: &Rc<Self>, extensions: Vec<ActivationIdAndReason>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let pending: Vec<&ActivationIdAndReason> = extensions.iter().filter(|p| !this.is_activated(&p.id.value)).collect();
            let operations: Vec<Rc<ActivationOperation>> = pending.into_iter().map(|ext| this.handle_activation_request(ext)).collect();
            Promise::all(operations.iter().map(|op| op.wait()).collect()).await?;
            Ok(())
        })
    }

    /// Handle semantics related to dependencies for `currentExtension`.
    /// We don't need to worry about dependency loops because they are handled by the registry.
    fn handle_activation_request(&self, current_activation: &ActivationIdAndReason) -> Rc<ActivationOperation> {
        let existing = self.operations.borrow().get(&current_activation.id.value).cloned();
        if let Some(existing) = existing {
            return existing;
        }

        if self.is_host_extension(&current_activation.id.value) {
            return self.create_and_save_operation(current_activation, None, Vec::new(), None);
        }

        let Some(current_extension) = self.registry.get_extension_description(&current_activation.id.value) else {
            // Error condition 0: unknown extension
            let error = Error::new(format!("Cannot activate unknown extension '{}'", current_activation.id.value));
            let result = self.create_and_save_operation(
                current_activation,
                None,
                Vec::new(),
                Some(Rc::new(ActivatedExtension::failed(error.clone()))),
            );
            self.host.on_extension_activation_error(
                &current_activation.id,
                Some(&error),
                Some(&MissingExtensionDependency::new(current_activation.id.value.clone())),
            );
            return result;
        };

        let mut deps: Vec<Rc<ActivationOperation>> = Vec::new();
        let dep_ids: Vec<String> = current_extension.manifest.extension_dependencies.clone().unwrap_or_default();
        for dep_id in &dep_ids {
            if self.is_resolved_extension(dep_id) {
                // This dependency is already resolved
                continue;
            }

            let dep = self.operations.borrow().get(dep_id).cloned();
            if let Some(dep) = dep {
                deps.push(dep);
                continue;
            }

            if self.is_host_extension(dep_id) {
                // must first wait for the dependency to activate
                let host_extension = self.global_registry.get_extension_description(dep_id).expect("uma extensão de host está no registro global");
                deps.push(self.handle_activation_request(&ActivationIdAndReason {
                    id: host_extension.identifier.clone(),
                    reason: current_activation.reason.clone(),
                }));
                continue;
            }

            if let Some(dep_desc) = self.registry.get_extension_description(dep_id) {
                if !has_main_or_browser(&dep_desc) {
                    // this dependency does not need to activate because it is descriptive only
                    continue;
                }

                // must first wait for the dependency to activate
                deps.push(self.handle_activation_request(&ActivationIdAndReason {
                    id: dep_desc.identifier.clone(),
                    reason: current_activation.reason.clone(),
                }));
                continue;
            }

            // Error condition 1: unknown dependency
            let current_extension_friendly_name = friendly_name_of(current_extension.manifest.display_name.as_deref(), &current_extension.identifier);
            let error = Error::new(format!(
                "Cannot activate the '{current_extension_friendly_name}' extension because it depends on unknown extension '{dep_id}'"
            ));
            let result = self.create_and_save_operation(
                current_activation,
                current_extension.manifest.display_name.clone(),
                Vec::new(),
                Some(Rc::new(ActivatedExtension::failed(error.clone()))),
            );
            self.host.on_extension_activation_error(&current_extension.identifier, Some(&error), Some(&MissingExtensionDependency::new(dep_id.clone())));
            return result;
        }

        self.create_and_save_operation(current_activation, current_extension.manifest.display_name.clone(), deps, None)
    }

    fn create_and_save_operation(
        &self,
        activation: &ActivationIdAndReason,
        display_name: Option<String>,
        deps: Vec<Rc<ActivationOperation>>,
        value: Option<Rc<ActivatedExtension>>,
    ) -> Rc<ActivationOperation> {
        let operation = ActivationOperation::new(
            activation.id.clone(),
            display_name,
            activation.reason.clone(),
            deps,
            value,
            self.host.clone(),
            self.log_service.clone(),
        );
        self.operations.borrow_mut().set(&activation.id.value, operation.clone());
        operation
    }

    fn is_host_extension(&self, extension_id: &str) -> bool {
        ExtensionDescriptionRegistry::is_host_extension(extension_id, &self.registry, &self.global_registry)
    }

    fn is_resolved_extension(&self, extension_id: &str) -> bool {
        match self.global_registry.get_extension_description(extension_id) {
            // unknown extension
            None => false,
            Some(extension_description) => !has_main_or_browser(&extension_description),
        }
    }
}

impl IDisposable for ExtensionsActivator {
    fn dispose(&self) {
        let operations: Vec<Rc<ActivationOperation>> = self.operations.borrow().values().cloned().collect();
        for op in operations {
            op.dispose();
        }
    }
}

/// `displayName || identifier.value`.
fn friendly_name_of(display_name: Option<&str>, identifier: &ExtensionIdentifier) -> String {
    match display_name {
        Some(display_name) if !display_name.is_empty() => display_name.to_owned(),
        _ => identifier.value.clone(),
    }
}

/// `ActivationOperation`.
struct ActivationOperation {
    barrier: Barrier,
    is_disposed: Cell<bool>,
    id: ExtensionIdentifier,
    display_name: Option<String>,
    reason: ExtensionActivationReason,
    deps: RefCell<Vec<Rc<ActivationOperation>>>,
    value: RefCell<Option<Rc<ActivatedExtension>>>,
    host: Rc<dyn IExtensionsActivatorHost>,
    log_service: LogServiceRef,
}

impl ActivationOperation {
    fn new(
        id: ExtensionIdentifier,
        display_name: Option<String>,
        reason: ExtensionActivationReason,
        deps: Vec<Rc<ActivationOperation>>,
        value: Option<Rc<ActivatedExtension>>,
        host: Rc<dyn IExtensionsActivatorHost>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let operation = Rc::new(Self {
            barrier: Barrier::new(),
            is_disposed: Cell::new(false),
            id,
            display_name,
            reason,
            deps: RefCell::new(deps),
            value: RefCell::new(value),
            host,
            log_service,
        });
        operation.initialize();
        operation
    }

    fn value(&self) -> Option<Rc<ActivatedExtension>> {
        self.value.borrow().clone()
    }

    fn friendly_name(&self) -> String {
        friendly_name_of(self.display_name.as_deref(), &self.id)
    }

    fn dispose(&self) {
        self.is_disposed.set(true);
    }

    fn wait(&self) -> Promise<bool> {
        self.barrier.wait()
    }

    /// `_initialize()`: a promessa devolvida não é usada pelo original (a rejeição fica sem tratador).
    fn initialize(self: &Rc<Self>) {
        let this = self.clone();
        let _ = async_fn(async move {
            this.wait_for_deps_then_activate().await?;
            this.barrier.open();
            Ok(())
        });
    }

    fn wait_for_deps_then_activate(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.value().is_some() {
                // this operation is already finished
                return Ok(());
            }

            while !this.deps.borrow().is_empty() {
                // remove completed deps
                let mut i = 0;
                loop {
                    let dep = {
                        let deps = this.deps.borrow();
                        if i >= deps.len() {
                            break;
                        }
                        deps[i].clone()
                    };

                    match dep.value() {
                        Some(dep_value) if !dep_value.activation_failed => {
                            // the dependency is already activated OK
                            this.deps.borrow_mut().remove(i);
                            continue;
                        }
                        Some(dep_value) => {
                            // Error condition 2: a dependency has already failed activation
                            let detail = dep_value.activation_failed_error.as_ref().map_or_else(|| "null".to_owned(), ToString::to_string);
                            let error = Error::new(format!(
                                "Cannot activate the '{}' extension because its dependency '{}' failed to activate",
                                this.friendly_name(),
                                dep.friendly_name()
                            ))
                            .with_prop("detail", detail);
                            *this.value.borrow_mut() = Some(Rc::new(ActivatedExtension::failed(error.clone())));
                            this.host.on_extension_activation_error(&this.id, Some(&error), None);
                            return Ok(());
                        }
                        None => {}
                    }
                    i += 1;
                }

                let waits: Vec<Promise<bool>> = this.deps.borrow().iter().map(|dep| dep.wait()).collect();
                if !waits.is_empty() {
                    // wait for one dependency
                    Promise::race(waits).await?;
                }
            }

            this.activate().await
        })
    }

    fn activate(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let activation = errors::catch(|| this.host.actual_activate_extension(&this.id, &this.reason));
            let outcome = match activation {
                Ok(promise) => promise.await,
                Err(err) => Err(err),
            };

            match outcome {
                Ok(value) => {
                    *this.value.borrow_mut() = Some(value);
                }
                Err(err) => {
                    let mut error = Error::new("");
                    if !err.name.is_empty() {
                        error.name = err.name.clone();
                    }
                    if !err.message.is_empty() {
                        error.message = format!("Activating extension '{}' failed: {}.", this.id.value, err.message);
                    } else {
                        error.message = format!("Activating extension '{}' failed: {}.", this.id.value, err);
                    }
                    if let Some(stack) = &err.stack {
                        error.stack = Some(stack.clone());
                    }

                    // Treat the extension as being empty
                    *this.value.borrow_mut() = Some(Rc::new(ActivatedExtension::failed(error.clone())));

                    if this.is_disposed.get() && errors::is_cancellation_error(&err) {
                        // It is expected for ongoing activations to fail if the extension host is going down
                        // So simply ignore and don't log canceled errors in this case
                        return Ok(());
                    }

                    this.host.on_extension_activation_error(&this.id, Some(&error), None);
                    this.log_service.error(&format!("Activating extension {} failed due to an error:", this.id.value));
                    this.log_service.error_args(&LogMessage::from(&err), &[]);
                }
            }
            Ok(())
        })
    }
}
