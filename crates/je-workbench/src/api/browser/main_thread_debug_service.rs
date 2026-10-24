//! Porte de `src/vs/workbench/api/browser/mainThreadDebugService.ts`: a ponte entre o `IDebugService` do workbench e o host
//! de extensões (sessões, breakpoints, provedores de configuração, fábricas de adaptador, visualizadores) e o
//! `ExtensionHostDebugAdapter` que fala com o adaptador pelo RPC.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadDebugService)` e o `IExtHostContext` do construtor: o registro de clientes
//!   não existe ainda; o construtor recebe o [`IMainContext`] (que dá o `getProxy`) e as duas dependências decoradas
//!   (`IDebugService` e `IDebugVisualizerService`). O `remoteAuthority` e o `extensionHostKind` do contexto não são lidos
//!   por esta classe.
//! - Os métodos de RPC mantêm o `$` como `rpc_` (ver `ext_host_debug_protocol`). O `IDebugAdapterFactory` é o trait de
//!   `contrib/debug/debug.rs`, implementado por esta classe (que se passa a si mesma como `this` em
//!   `registerDebugAdapterFactory`, por isso o `Weak` do `Rc`).
//! - O `DisposableMap<IDebugSession, DisposableStore>` das sessões é indexado pelo id da sessão (o `getId()` é único), e
//!   `value.session === session` do `_debugAdapters` compara os ids.
//! - `ExtensionHostDebugAdapter` guarda a classe `MainThreadDebugService` por `Weak` (`_ds`) e entrega o próprio
//!   [`DebugAdapterState`] para o contrato `AbstractDebugAdapter`; `fireError` e `fireExit` disparam os emissores do estado.
//! - `onDidCustomEvent` entrega a `DebugProtocol.Event`; o `event: any` do `$acceptDebugSessionCustomEvent` é o objeto JSON da
//!   mensagem (`seq`, `type`, `event`, `body`).
//! - O `try/catch` de `$startDebugging` (só pega o que o serviço lança de forma síncrona, já que o `return` não espera a
//!   promessa) é um `errors::catch`; o `async` do original adota a promessa devolvida (`async_fn_maybe`).
//! - `$acceptDAError` com `stack` ausente escreve `undefined` na mensagem, como o template do original.
//! - O `Event.any(viewModel.onDidFocusStackFrame, viewModel.onDidFocusThread)(...)` é o `any` dos dois eventos reduzidos a sinal.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::any;
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::workspace::WorkspaceFolder as IWorkspaceFolder;
use serde_json::Value;

use crate::api::common::ext_host_debug_protocol::{
    BreakpointDto, DebugSessionUuid, ExtHostDebugServiceShape, IBreakpointDto, IBreakpointsDeltaDto, IDataBreakpointDto, IDebugSessionDto, IDebugSessionFullDto, IFunctionBreakpointDto,
    ISourceBreakpointDto, IStackFrameFocusDto, IStartDebuggingOptions, IThreadFocusDto, MainThreadDebugServiceShape, NameOrConfig, RegisterBreakpointDto, StackFocusDto,
};
use crate::api::common::ext_host_protocol::{ExtHostContext, IMainContext};
use crate::contrib::debug::abstract_debug_adapter::{AbstractDebugAdapter, DebugAdapterState};
use crate::contrib::debug::debug::{
    AnyBreakpoint, BreakpointIds, ConfigOrName, DataBreakpointSource, DebugConfigurationProviderTriggerKind, IConfig, IDebugAdapter, IDebugAdapterDescriptorFactory,
    IDebugAdapterFactory, IDebugConfigurationProvider, IDebugService, IDebugSession, IDebugSessionOptions, IBreakpointData, IDataBreakpointOptions, IFunctionBreakpointOptions,
    INewReplElementData, IDebugVisualization, IDebugVisualizationTreeItem,
};
use crate::contrib::debug::debug_protocol::{Event as ProtocolEvent, ProtocolMessage};
use crate::contrib::debug::debug_utils::{convert_to_da_paths, convert_to_vsc_paths, is_session_attach};
use crate::contrib::debug::debug_visualizers::{IDebugVisualizerService, VisualizerHandle, VisualizerTreeHandle};

/// `MainThreadDebugService`.
pub struct MainThreadDebugService {
    this: Weak<MainThreadDebugService>,
    proxy: Rc<dyn ExtHostDebugServiceShape>,
    to_dispose: Rc<DisposableStore>,
    debug_adapters: Rc<RefCell<IndexMap<u32, Rc<ExtensionHostDebugAdapter>>>>,
    debug_adapters_handle_counter: Cell<u32>,
    debug_configuration_providers: Rc<DisposableMap<u32, dyn IDisposable>>,
    debug_adapter_descriptor_factories: Rc<DisposableMap<u32, dyn IDisposable>>,
    ext_host_known_sessions: RefCell<HashSet<DebugSessionUuid>>,
    visualizer_handles: RefCell<HashMap<String, DisposableRef>>,
    visualizer_tree_handles: RefCell<HashMap<String, DisposableRef>>,
    debug_service: Rc<dyn IDebugService>,
    visualizer_service: Rc<dyn IDebugVisualizerService>,
}

/// O objeto JSON de uma `DebugProtocol.Event` (o `event: any` de `$acceptDebugSessionCustomEvent`).
fn event_to_value(event: &ProtocolEvent) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("seq".to_owned(), Value::from(event.seq));
    object.insert("type".to_owned(), Value::from("event"));
    object.insert("event".to_owned(), Value::from(event.event.clone()));
    if let Some(body) = &event.body {
        object.insert("body".to_owned(), body.clone());
    }
    Value::Object(object)
}

impl MainThreadDebugService {
    pub fn new(ext_host_context: &dyn IMainContext, debug_service: Rc<dyn IDebugService>, visualizer_service: Rc<dyn IDebugVisualizerService>) -> Rc<MainThreadDebugService> {
        let to_dispose = DisposableStore::new();
        let debug_configuration_providers: Rc<DisposableMap<u32, dyn IDisposable>> = DisposableMap::new();
        to_dispose.add(debug_configuration_providers.clone());
        let debug_adapter_descriptor_factories: Rc<DisposableMap<u32, dyn IDisposable>> = DisposableMap::new();
        to_dispose.add(debug_adapter_descriptor_factories.clone());
        let service = Rc::new_cyclic(|this| MainThreadDebugService {
            this: this.clone(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_DEBUG_SERVICE),
            to_dispose,
            debug_adapters: Rc::new(RefCell::new(IndexMap::new())),
            debug_adapters_handle_counter: Cell::new(1),
            debug_configuration_providers,
            debug_adapter_descriptor_factories,
            ext_host_known_sessions: RefCell::new(HashSet::new()),
            visualizer_handles: RefCell::new(HashMap::new()),
            visualizer_tree_handles: RefCell::new(HashMap::new()),
            debug_service,
            visualizer_service,
        });
        service.listen();
        service
    }

    /// O corpo do construtor que registra os ouvintes (precisa do `Rc` pronto).
    fn listen(&self) {
        let session_listeners: Rc<DisposableMap<String, DisposableStore>> = DisposableMap::new();
        self.to_dispose.add(session_listeners.clone());

        let this = self.this.clone();
        let listeners = session_listeners.clone();
        self.to_dispose.add(self.debug_service.on_did_new_session().listen(move |session: &Rc<dyn IDebugSession>| {
            let Some(this) = this.upgrade() else { return };
            let _ = this.proxy.rpc_accept_debug_session_started(this.get_session_dto(session));
            let store = listeners.get(&session.get_id());
            if let Some(store) = store {
                let (weak, session) = (this.this.clone(), session.clone());
                store.add(session.on_did_change_name().listen(move |name: &String| {
                    if let Some(this) = weak.upgrade() {
                        let _ = this.proxy.rpc_accept_debug_session_name_changed(this.get_session_dto(&session), name);
                    }
                }));
            }
        }));

        // Need to start listening early to new session events because a custom event can come while a session is initialising
        let this = self.this.clone();
        let listeners = session_listeners.clone();
        self.to_dispose.add(self.debug_service.on_will_new_session().listen(move |session: &Rc<dyn IDebugSession>| {
            let Some(this) = this.upgrade() else { return };
            let store = match listeners.get(&session.get_id()) {
                Some(store) => store,
                None => {
                    let store = DisposableStore::new();
                    listeners.set(session.get_id(), store.clone(), false);
                    store
                }
            };
            let (weak, session) = (this.this.clone(), session.clone());
            store.add(session.on_did_custom_event().listen(move |event: &ProtocolEvent| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.proxy.rpc_accept_debug_session_custom_event(this.get_session_dto(&session), event_to_value(event));
                }
            }));
        }));

        let this = self.this.clone();
        let listeners = session_listeners;
        self.to_dispose.add(self.debug_service.on_did_end_session().listen(move |event| {
            let Some(this) = this.upgrade() else { return };
            let session = &event.session;
            let _ = this.proxy.rpc_accept_debug_session_terminated(this.get_session_dto(session));
            this.ext_host_known_sessions.borrow_mut().remove(&session.get_id());

            // keep the session listeners around since we still will get events after they restart
            if !event.restart {
                listeners.delete_and_dispose(&session.get_id());
            }

            // any restarted session will create a new DA, so always throw the old one away.
            this.debug_adapters.borrow_mut().retain(|_, adapter| adapter.session.get_id() != session.get_id());
        }));

        let this = self.this.clone();
        self.to_dispose.add(self.debug_service.get_view_model().on_did_focus_session().listen(move |session: &Option<Rc<dyn IDebugSession>>| {
            if let Some(this) = this.upgrade() {
                let dto = session.as_ref().map(|session| this.get_session_dto(session));
                let _ = this.proxy.rpc_accept_debug_session_active_changed(dto);
            }
        }));

        let adapters = self.debug_adapters.clone();
        self.to_dispose.add(to_disposable(move || {
            let adapters: Vec<(u32, Rc<ExtensionHostDebugAdapter>)> = adapters.borrow().iter().map(|(handle, adapter)| (*handle, adapter.clone())).collect();
            for (handle, da) in adapters {
                da.fire_error(handle, Error::new("Extension host shut down"));
            }
        }));

        let view_model = self.debug_service.get_view_model();
        let this = self.this.clone();
        let focus_view_model = view_model.clone();
        self.to_dispose.add(any(vec![view_model.on_did_focus_stack_frame().signal(), view_model.on_did_focus_thread().signal()]).listen(move |_| {
            let Some(this) = this.upgrade() else { return };
            let stack_frame = focus_view_model.focused_stack_frame();
            let thread = focus_view_model.focused_thread();
            if let Some(stack_frame) = stack_frame {
                let thread = stack_frame.thread();
                let _ = this.proxy.rpc_accept_stack_frame_focus(Some(StackFocusDto::StackFrame(IStackFrameFocusDto {
                    thread_id: thread.thread_id(),
                    frame_id: stack_frame.frame_id(),
                    session_id: thread.session().get_id(),
                })));
            } else if let Some(thread) = thread {
                let _ = this.proxy.rpc_accept_stack_frame_focus(Some(StackFocusDto::Thread(IThreadFocusDto { thread_id: thread.thread_id(), session_id: thread.session().get_id() })));
            } else {
                let _ = this.proxy.rpc_accept_stack_frame_focus(None);
            }
        }));

        self.send_breakpoints_and_listen();
    }

    fn send_breakpoints_and_listen(&self) {
        // set up a handler to send more
        let this = self.this.clone();
        self.to_dispose.add(self.debug_service.get_model().on_did_change_breakpoints().listen(move |e| {
            let Some(this) = this.upgrade() else { return };
            // Ignore session only breakpoint events since they should only reflect in the UI
            if let Some(e) = e
                && !e.session_only
            {
                let mut delta = IBreakpointsDeltaDto::default();
                if let Some(added) = &e.added {
                    delta.added = Some(Self::convert_to_dto(added));
                }
                if let Some(removed) = &e.removed {
                    delta.removed = Some(removed.iter().map(|x| any_breakpoint_id(x)).collect());
                }
                if let Some(changed) = &e.changed {
                    delta.changed = Some(Self::convert_to_dto(changed));
                }

                if delta.added.is_some() || delta.removed.is_some() || delta.changed.is_some() {
                    this.proxy.rpc_accept_breakpoints_delta(delta);
                }
            }
        }));

        // send all breakpoints
        let model = self.debug_service.get_model();
        let bps: Vec<AnyBreakpoint> = model.get_breakpoints().into_iter().map(AnyBreakpoint::Breakpoint).collect();
        let fbps: Vec<AnyBreakpoint> = model.get_function_breakpoints().into_iter().map(AnyBreakpoint::Function).collect();
        let dbps: Vec<AnyBreakpoint> = model.get_data_breakpoints().into_iter().map(AnyBreakpoint::Data).collect();
        if !bps.is_empty() || !fbps.is_empty() {
            let mut added = Self::convert_to_dto(&bps);
            added.extend(Self::convert_to_dto(&fbps));
            added.extend(Self::convert_to_dto(&dbps));
            self.proxy.rpc_accept_breakpoints_delta(IBreakpointsDeltaDto { added: Some(added), ..IBreakpointsDeltaDto::default() });
        }
    }

    /// O `Rc` deste serviço, para passá-lo como `IDebugAdapterFactory`.
    fn rc(&self) -> Rc<MainThreadDebugService> {
        self.this.upgrade().expect("o MainThreadDebugService está vivo enquanto um método dele roda")
    }

    fn get_session(&self, session_id: Option<&str>) -> Option<Rc<dyn IDebugSession>> {
        match session_id {
            Some(session_id) if !session_id.is_empty() => self.debug_service.get_model().get_session(session_id, true),
            _ => None,
        }
    }

    /// `getSessionDto(session)`.
    pub fn get_session_dto(&self, session: &Rc<dyn IDebugSession>) -> IDebugSessionDto {
        let session_id = session.get_id();
        if self.ext_host_known_sessions.borrow().contains(&session_id) {
            IDebugSessionDto::Id(session_id)
        } else {
            // this._sessions.add(sessionID); 	// #69534: see $sessionCached above
            let configuration = session.configuration();
            IDebugSessionDto::Full(IDebugSessionFullDto {
                id: session_id,
                r#type: configuration.get("type").and_then(Value::as_str).unwrap_or_default().to_owned(),
                name: session.name(),
                folder_uri: session.root().map(|root| root.uri),
                configuration,
                parent: session.parent_session().map(|parent| parent.get_id()),
            })
        }
    }

    /// `convertToDto(bps)`: os breakpoints de instrução não têm DTO e saem da lista.
    fn convert_to_dto(bps: &[AnyBreakpoint]) -> Vec<BreakpointDto> {
        bps.iter()
            .filter_map(|bp| match bp {
                AnyBreakpoint::Function(fbp) => Some(BreakpointDto::Function(IFunctionBreakpointDto {
                    base: IBreakpointDto {
                        id: Some(fbp.base.get_id()),
                        enabled: fbp.base.enabled,
                        condition: fbp.base.condition.clone(),
                        hit_condition: fbp.base.hit_condition.clone(),
                        log_message: fbp.base.log_message.clone(),
                        mode: None,
                    },
                    function_name: fbp.name.clone(),
                })),
                AnyBreakpoint::Data(dbp) => Some(BreakpointDto::Data(IDataBreakpointDto {
                    base: IBreakpointDto {
                        id: Some(dbp.base.get_id()),
                        enabled: dbp.base.enabled,
                        condition: dbp.base.condition.clone(),
                        hit_condition: dbp.base.hit_condition.clone(),
                        log_message: dbp.base.log_message.clone(),
                        mode: None,
                    },
                    data_id: match &dbp.src {
                        DataBreakpointSource::Variable { data_id } => data_id.clone(),
                        DataBreakpointSource::Address { address, .. } => address.clone(),
                    },
                    access_type: dbp.access_type,
                    label: dbp.description.clone(),
                    can_persist: dbp.can_persist,
                    access_types: None,
                })),
                AnyBreakpoint::Breakpoint(sbp) => Some(BreakpointDto::Source(ISourceBreakpointDto {
                    base: IBreakpointDto {
                        id: Some(sbp.base.get_id()),
                        enabled: sbp.base.enabled,
                        condition: sbp.base.condition.clone(),
                        hit_condition: sbp.base.hit_condition.clone(),
                        log_message: sbp.base.log_message.clone(),
                        mode: None,
                    },
                    uri: sbp.uri.clone(),
                    line: if sbp.line_number > 0 { sbp.line_number - 1 } else { 0 },
                    character: match sbp.column {
                        Some(column) if column > 0 => column - 1,
                        _ => 0,
                    },
                })),
                AnyBreakpoint::Instruction(_) => None,
            })
            .collect()
    }
}

/// O `getId()` de qualquer breakpoint.
fn any_breakpoint_id(bp: &AnyBreakpoint) -> String {
    match bp {
        AnyBreakpoint::Breakpoint(bp) => bp.base.get_id(),
        AnyBreakpoint::Function(bp) => bp.base.get_id(),
        AnyBreakpoint::Data(bp) => bp.base.get_id(),
        AnyBreakpoint::Instruction(bp) => bp.base.get_id(),
    }
}

impl IDisposable for MainThreadDebugService {
    fn dispose(&self) {
        self.to_dispose.dispose();
    }
}

// ---- interface IDebugAdapterProvider

impl IDebugAdapterFactory for MainThreadDebugService {
    fn create_debug_adapter(&self, session: Rc<dyn IDebugSession>) -> Rc<dyn IDebugAdapter> {
        let handle = self.debug_adapters_handle_counter.get();
        self.debug_adapters_handle_counter.set(handle + 1);
        let da = Rc::new(ExtensionHostDebugAdapter { state: DebugAdapterState::new(), ds: self.this.clone(), handle, proxy: self.proxy.clone(), session });
        self.debug_adapters.borrow_mut().insert(handle, da.clone());
        da
    }

    fn substitute_variables(&self, folder: Option<IWorkspaceFolder>, config: IConfig) -> Promise<IConfig> {
        self.proxy.rpc_substitute_variables(folder.map(|folder| folder.uri), config)
    }

    fn run_in_terminal(&self, args: Value, session_id: &str) -> Promise<Option<i64>> {
        self.proxy.rpc_run_in_terminal(args, session_id)
    }
}

// ---- RPC methods (MainThreadDebugServiceShape)

impl MainThreadDebugServiceShape for MainThreadDebugService {
    fn rpc_register_debug_visualizer_tree(&self, tree_id: &str, can_edit: bool) {
        let (dispose_proxy, children_proxy, item_proxy, edit_proxy) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        let (children_tree_id, item_tree_id) = (tree_id.to_owned(), tree_id.to_owned());
        let handle = VisualizerTreeHandle {
            dispose_item: Rc::new(move |id| dispose_proxy.rpc_dispose_visualized_tree(id)),
            get_children: Rc::new(move |e| children_proxy.rpc_get_visualizer_tree_item_children(&children_tree_id, e)),
            get_tree_item: Rc::new(move |e| item_proxy.rpc_get_visualizer_tree_item(&item_tree_id, e)),
            edit_item: can_edit.then(|| {
                let edit: Rc<dyn Fn(u64, &str) -> Promise<Option<IDebugVisualizationTreeItem>>> = Rc::new(move |e, v| edit_proxy.rpc_edit_visualizer_tree_item(e, v));
                edit
            }),
        };
        let registration = self.visualizer_service.register_tree(tree_id, handle);
        self.visualizer_tree_handles.borrow_mut().insert(tree_id.to_owned(), registration);
    }

    fn rpc_unregister_debug_visualizer_tree(&self, tree_id: &str) {
        let handle = self.visualizer_tree_handles.borrow_mut().remove(tree_id);
        if let Some(handle) = handle {
            handle.dispose();
        }
    }

    fn rpc_register_debug_visualizer(&self, extension_id: &str, id: &str) {
        let (dispose_proxy, execute_proxy, provide_proxy, resolve_proxy) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        let (provide_extension_id, provide_id) = (extension_id.to_owned(), id.to_owned());
        let handle = VisualizerHandle {
            extension_id: ExtensionIdentifier::new(extension_id),
            id: id.to_owned(),
            dispose_debug_visualizers: Rc::new(move |ids| dispose_proxy.rpc_dispose_debug_visualizers(ids)),
            execute_debug_visualizer_command: Rc::new(move |id| execute_proxy.rpc_execute_debug_visualizer_command(id)),
            provide_debug_visualizers: Rc::new(move |context, token: CancellationToken| {
                // O `.then(r => r.map(IDebugVisualization.deserialize))` do original é a identidade.
                let provided: Promise<Vec<IDebugVisualization>> = provide_proxy.rpc_provide_debug_visualizers(&provide_extension_id, &provide_id, context, token);
                provided
            }),
            resolve_debug_visualizer: Rc::new(move |viz, token| resolve_proxy.rpc_resolve_debug_visualizer(viz.id, token)),
        };
        let registration = self.visualizer_service.register(handle);
        self.visualizer_handles.borrow_mut().insert(format!("{extension_id}/{id}"), registration);
    }

    fn rpc_unregister_debug_visualizer(&self, extension_id: &str, id: &str) {
        let key = format!("{extension_id}/{id}");
        let handle = self.visualizer_handles.borrow_mut().remove(&key);
        if let Some(handle) = handle {
            handle.dispose();
        }
    }

    fn rpc_register_debug_types(&self, debug_types: Vec<String>) {
        let factory: Rc<dyn IDebugAdapterFactory> = self.rc();
        self.to_dispose.add(self.debug_service.get_adapter_manager().register_debug_adapter_factory(debug_types, factory));
    }

    fn rpc_register_breakpoints(&self, dtos: Vec<RegisterBreakpointDto>) -> Promise<()> {
        for dto in dtos {
            match dto {
                RegisterBreakpointDto::SourceMulti(dto) => {
                    let rawbps: Vec<IBreakpointData> = dto
                        .lines
                        .iter()
                        .map(|l| IBreakpointData {
                            id: Some(l.id.clone()),
                            enabled: Some(l.enabled),
                            line_number: l.line + 1,
                            column: if l.character > 0 { Some(l.character + 1) } else { None }, // a column value of 0 results in an omitted column attribute; see #46784
                            condition: l.condition.clone(),
                            hit_condition: l.hit_condition.clone(),
                            log_message: l.log_message.clone(),
                            mode: l.mode.clone(),
                        })
                        .collect();
                    let _ = self.debug_service.add_breakpoints(dto.uri, rawbps);
                }
                RegisterBreakpointDto::Function(dto) => {
                    self.debug_service.add_function_breakpoint(
                        Some(IFunctionBreakpointOptions {
                            name: dto.function_name,
                            mode: dto.base.mode,
                            condition: dto.base.condition,
                            hit_condition: dto.base.hit_condition,
                            enabled: Some(dto.base.enabled),
                            log_message: dto.base.log_message,
                        }),
                        dto.base.id,
                    );
                }
                RegisterBreakpointDto::Data(dto) => {
                    let _ = self.debug_service.add_data_breakpoint(IDataBreakpointOptions {
                        description: dto.label,
                        src: DataBreakpointSource::Variable { data_id: dto.data_id },
                        can_persist: dto.can_persist,
                        access_types: dto.access_types,
                        access_type: dto.access_type,
                        mode: dto.base.mode,
                    });
                }
            }
        }
        Promise::resolved(())
    }

    fn rpc_unregister_breakpoints(&self, breakpoint_ids: Vec<String>, function_breakpoint_ids: Vec<String>, data_breakpoint_ids: Vec<String>) -> Promise<()> {
        breakpoint_ids.into_iter().for_each(|id| {
            let _ = self.debug_service.remove_breakpoints(Some(BreakpointIds::One(id)));
        });
        function_breakpoint_ids.iter().for_each(|id| {
            let _ = self.debug_service.remove_function_breakpoints(Some(id));
        });
        data_breakpoint_ids.iter().for_each(|id| {
            let _ = self.debug_service.remove_data_breakpoints(Some(id));
        });
        Promise::resolved(())
    }

    fn rpc_register_debug_configuration_provider(
        &self,
        debug_type: &str,
        provider_trigger_kind: DebugConfigurationProviderTriggerKind,
        has_provide: bool,
        has_resolve: bool,
        has_resolve2: bool,
        handle: u32,
    ) -> Promise<()> {
        let mut provider = IDebugConfigurationProvider {
            r#type: debug_type.to_owned(),
            trigger_kind: provider_trigger_kind,
            resolve_debug_configuration: None,
            resolve_debug_configuration_with_substituted_variables: None,
            provide_debug_configurations: None,
        };
        if has_provide {
            let proxy = self.proxy.clone();
            provider.provide_debug_configurations = Some(Rc::new(move |folder, token| proxy.rpc_provide_debug_configurations(handle, folder, token)));
        }
        if has_resolve {
            let proxy = self.proxy.clone();
            provider.resolve_debug_configuration = Some(Rc::new(move |folder, config, token| proxy.rpc_resolve_debug_configuration(handle, folder, config, token)));
        }
        if has_resolve2 {
            let proxy = self.proxy.clone();
            provider.resolve_debug_configuration_with_substituted_variables =
                Some(Rc::new(move |folder, config, token| proxy.rpc_resolve_debug_configuration_with_substituted_variables(handle, folder, config, token)));
        }
        let registration = self.debug_service.get_configuration_manager().register_debug_configuration_provider(Rc::new(provider));
        self.debug_configuration_providers.set(handle, registration, false);

        Promise::resolved(())
    }

    fn rpc_unregister_debug_configuration_provider(&self, handle: u32) {
        self.debug_configuration_providers.delete_and_dispose(&handle);
    }

    fn rpc_register_debug_adapter_descriptor_factory(&self, debug_type: &str, handle: u32) -> Promise<()> {
        let (proxy, this) = (self.proxy.clone(), self.this.clone());
        let provider = IDebugAdapterDescriptorFactory {
            r#type: debug_type.to_owned(),
            create_debug_adapter_descriptor: Rc::new(move |session| {
                let this = this.upgrade().expect("o MainThreadDebugService está vivo enquanto registra o descritor");
                proxy.rpc_provide_debug_adapter(handle, this.get_session_dto(&session))
            }),
        };
        let registration = self.debug_service.get_adapter_manager().register_debug_adapter_descriptor_factory(Rc::new(provider));
        self.debug_adapter_descriptor_factories.set(handle, registration, false);

        Promise::resolved(())
    }

    fn rpc_unregister_debug_adapter_descriptor_factory(&self, handle: u32) {
        self.debug_adapter_descriptor_factories.delete_and_dispose(&handle);
    }

    fn rpc_start_debugging(&self, folder: Option<URI>, name_or_config: NameOrConfig, options: IStartDebuggingOptions) -> Promise<bool> {
        let launch = self.debug_service.get_configuration_manager().get_launch(folder.as_ref());
        let parent_session = self.get_session(options.parent_session_id.as_deref());
        let save_before_start = options.suppress_save_before_start.map(|suppress| !suppress);
        let debug_options = IDebugSessionOptions {
            no_debug: options.no_debug,
            parent_session: parent_session.clone(),
            lifecycle_managed_by_parent: options.lifecycle_managed_by_parent,
            repl: options.repl,
            compound_root: parent_session.as_ref().and_then(|parent| parent.compound_root()),
            compact: options.compact,
            started_by_user: None,
            save_before_restart: save_before_start,
            test_run: options.test_run,

            suppress_debug_statusbar: options.suppress_debug_statusbar,
            suppress_debug_toolbar: options.suppress_debug_toolbar,
            suppress_debug_view: options.suppress_debug_view,
        };
        let config_or_name = match name_or_config {
            NameOrConfig::Name(name) => ConfigOrName::Name(name),
            NameOrConfig::Config(config) => ConfigOrName::Config(config),
        };
        let started = errors::catch(|| self.debug_service.start_debugging(launch, Some(config_or_name), Some(debug_options), save_before_start));
        je_base::async_::async_fn_maybe(async move {
            match started {
                Ok(promise) => Ok(MaybePromise::Promise(promise)),
                Err(err) => Err(Error::no_telemetry(if err.message.is_empty() { "cannot start debugging".to_owned() } else { err.message })),
            }
        })
    }

    fn rpc_set_debug_session_name(&self, session_id: &str, name: &str) {
        let session = self.debug_service.get_model().get_session(session_id, false);
        if let Some(session) = session {
            session.set_name(name);
        }
    }

    fn rpc_custom_debug_adapter_request(&self, session_id: &str, request: &str, args: Option<Value>) -> Promise<Option<Value>> {
        let session = self.debug_service.get_model().get_session(session_id, true);
        match session {
            Some(session) => session.custom_request(request, args).then_result_promise(|result| match result {
                Ok(Some(response)) if response.success => Promise::resolved(response.body),
                Ok(Some(response)) => Promise::rejected(Error::no_telemetry(response.message.unwrap_or_default())),
                Ok(None) => Promise::rejected(Error::no_telemetry("custom request failed")),
                Err(error) => Promise::rejected(error),
            }),
            None => Promise::rejected(Error::no_telemetry("debug session not found")),
        }
    }

    fn rpc_get_debug_protocol_breakpoint(&self, session_id: &str, breakpoint_id: &str) -> Promise<Option<Value>> {
        let session = self.debug_service.get_model().get_session(session_id, true);
        match session {
            Some(session) => Promise::resolved(session.get_debug_protocol_breakpoint(breakpoint_id)),
            None => Promise::rejected(Error::no_telemetry("debug session not found")),
        }
    }

    fn rpc_stop_debugging(&self, session_id: Option<&str>) -> Promise<()> {
        if let Some(session_id) = session_id.filter(|session_id| !session_id.is_empty()) {
            let session = self.get_session(Some(session_id));
            if let Some(session) = session {
                let disconnect_only = is_session_attach(&session);
                return self.debug_service.stop_session(Some(session), disconnect_only);
            }
        } else {
            // stop all
            return self.debug_service.stop_session(None, false);
        }
        Promise::rejected(Error::no_telemetry("debug session not found"))
    }

    fn rpc_append_debug_console(&self, value: &str) {
        // Use warning as severity to get the orange color for messages coming from the debug extension
        let session = self.debug_service.get_view_model().focused_session();
        if let Some(session) = session {
            session.append_to_repl(INewReplElementData { output: value.to_owned(), sev: Severity::Warning });
        }
    }

    fn rpc_accept_da_message(&self, handle: u32, message: ProtocolMessage) {
        // don't use getDebugAdapter since a message can still be in-flight after the adapter was removed on session end
        let adapter = self.debug_adapters.borrow().get(&handle).cloned();
        if let Some(adapter) = adapter {
            adapter.accept_message(convert_to_vsc_paths(&message, false));
        }
    }

    fn rpc_accept_da_error(&self, handle: u32, name: &str, message: &str, stack: Option<&str>) {
        // don't use getDebugAdapter since an error can be expected on a post-close
        let adapter = self.debug_adapters.borrow().get(&handle).cloned();
        if let Some(adapter) = adapter {
            adapter.fire_error(handle, Error::new(format!("{name}: {message}\n{}", stack.unwrap_or("undefined"))));
        }
    }

    fn rpc_accept_da_exit(&self, handle: u32, code: Option<i32>, _signal: Option<&str>) {
        // don't use getDebugAdapter since an error can be expected on a post-close
        let adapter = self.debug_adapters.borrow().get(&handle).cloned();
        if let Some(adapter) = adapter {
            adapter.fire_exit(handle, code);
        }
    }

    // dto helpers

    fn rpc_session_cached(&self, session_id: &str) {
        // remember that the EH has cached the session and we do not have to send it again
        self.ext_host_known_sessions.borrow_mut().insert(session_id.to_owned());
    }
}

/// `ExtensionHostDebugAdapter`: DebugAdapter that communicates via extension protocol with another debug adapter.
pub struct ExtensionHostDebugAdapter {
    state: Rc<DebugAdapterState>,
    ds: Weak<MainThreadDebugService>,
    handle: u32,
    proxy: Rc<dyn ExtHostDebugServiceShape>,
    pub session: Rc<dyn IDebugSession>,
}

impl ExtensionHostDebugAdapter {
    /// `fireError(handle, err)`.
    pub fn fire_error(&self, _handle: u32, err: Error) {
        self.state.on_error_emitter.fire(err);
    }

    /// `fireExit(handle, code, signal)`.
    pub fn fire_exit(&self, _handle: u32, code: Option<i32>) {
        self.state.on_exit_emitter.fire(code);
    }
}

impl IDisposable for ExtensionHostDebugAdapter {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for ExtensionHostDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        let ds = self.ds.upgrade().expect("o MainThreadDebugService está vivo enquanto a sessão começa");
        self.proxy.rpc_start_da_session(self.handle, ds.get_session_dto(&self.session))
    }

    fn send_message(&self, message: &ProtocolMessage) {
        self.proxy.rpc_send_da_message(self.handle, convert_to_da_paths(message, true));
    }

    fn stop_session(&self) -> Promise<()> {
        let cancelled = self.cancel_pending_requests();
        let (proxy, handle) = (self.proxy.clone(), self.handle);
        async_fn(async move {
            cancelled.await?;
            proxy.rpc_stop_da_session(handle).await
        })
    }
}
