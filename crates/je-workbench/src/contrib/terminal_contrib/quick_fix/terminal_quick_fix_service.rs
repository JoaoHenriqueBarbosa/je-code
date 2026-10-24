//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/browser/terminalQuickFixService.ts`: o serviço que guarda os
//! seletores contribuídos pelas extensões (`terminalQuickFixes`) e os provedores que elas registram.
//!
//! Decisões de tradução:
//! - `quickFixExtensionPoint` (o `ExtensionsRegistry.registerExtensionPoint` do módulo) é [`register_quick_fix_extension_point`];
//!   o serviço recebe o ponto no construtor, como o `AuthenticationService` recebe o dele. O `jsonSchema` e o
//!   `activationEventsGenerator` não ficam no descritor do porte (ver `extensions_registry`); o gerador é
//!   [`quick_fix_activation_events`].
//! - Os valores contribuídos chegam como JSON ([`Value`]). Um seletor sem `id`, sem `commandLineMatcher` ou com
//!   `commandExitResult` fora de `success`/`error` (que o esquema do original rejeita) é descartado. `kind: 'default'` do esquema
//!   vale `fix`.
//! - `new Promise(r => setHandler(...))` é `Promise::with_executor`; o tratador pode rodar mais de uma vez e só a primeira
//!   resolução vale, como no original.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_platform::terminal::terminal::{ITerminalOutputMatcher, LineMatcher, OutputMatcherAnchor};
use serde_json::Value;

use super::quick_fix::{
    CommandExitResult, ITerminalCommandSelector, ITerminalQuickFixProvider, ITerminalQuickFixProviderSelector, ITerminalQuickFixService, QuickFixKind,
};
use crate::services::extensions::extensions::is_proposed_api_enabled;
use crate::services::extensions::extensions_registry::{ExtensionPoint, ExtensionPointDescriptor, ExtensionPointUser, extensions_registry};

/// `quickFixExtensionPoint = ExtensionsRegistry.registerExtensionPoint({ extensionPoint: 'terminalQuickFixes', defaultExtensionKind: ['workspace'], ... })`.
pub fn register_quick_fix_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "terminalQuickFixes".to_owned(),
        default_extension_kind: Some(vec![je_extensions::manifest::ExtensionKind::Workspace]),
        can_handle_resolver: None,
    })
}

/// O `activationEventsGenerator` do ponto de extensão: `onTerminalQuickFixRequest:<id>` para cada seletor contribuído.
pub fn quick_fix_activation_events(terminal_quick_fixes: &[ITerminalCommandSelector]) -> Vec<String> {
    terminal_quick_fixes.iter().map(|quick_fix_contrib| format!("onTerminalQuickFixRequest:{}", quick_fix_contrib.id)).collect()
}

fn parse_matcher(value: &Value) -> Option<LineMatcher> {
    Some(LineMatcher::Text(value.as_str()?.to_owned()))
}

fn parse_output_matcher(value: &Value) -> Option<ITerminalOutputMatcher> {
    let object = value.as_object()?;
    Some(ITerminalOutputMatcher {
        line_matcher: parse_matcher(object.get("lineMatcher")?)?,
        anchor: match object.get("anchor")?.as_str()? {
            "top" => OutputMatcherAnchor::Top,
            "bottom" => OutputMatcherAnchor::Bottom,
            _ => return None,
        },
        offset: object.get("offset")?.as_f64()?,
        length: object.get("length")?.as_f64()?,
        multiple_matches: object.get("multipleMatches").and_then(Value::as_bool),
    })
}

fn parse_selector(value: &Value, extension_identifier: &str) -> Option<ITerminalCommandSelector> {
    let object = value.as_object()?;
    let command_exit_result = match object.get("commandExitResult")?.as_str()? {
        "success" => CommandExitResult::Success,
        "error" => CommandExitResult::Error,
        _ => return None,
    };
    let kind = match object.get("kind").and_then(Value::as_str) {
        Some("explain") => Some(QuickFixKind::Explain),
        Some("fix" | "default") => Some(QuickFixKind::Fix),
        _ => None,
    };
    Some(ITerminalCommandSelector {
        id: object.get("id")?.as_str()?.to_owned(),
        command_line_matcher: parse_matcher(object.get("commandLineMatcher")?)?,
        output_matcher: object.get("outputMatcher").and_then(parse_output_matcher),
        command_exit_result,
        kind,
        extension_identifier: Some(extension_identifier.to_owned()),
    })
}

type Providers = Rc<RefCell<HashMap<String, Rc<dyn ITerminalQuickFixProvider>>>>;

struct Inner {
    /// `_selectors`.
    selectors: RefCell<HashMap<String, ITerminalCommandSelector>>,
    /// `_providers`.
    providers: Providers,
    /// `_pendingProviders`.
    pending_providers: RefCell<HashMap<String, Rc<dyn ITerminalQuickFixProvider>>>,
    on_did_register_provider: Rc<Emitter<ITerminalQuickFixProviderSelector>>,
    on_did_register_command_selector: Rc<Emitter<ITerminalCommandSelector>>,
    on_did_unregister_provider: Rc<Emitter<String>>,
    extension_quick_fixes: Promise<Vec<ITerminalCommandSelector>>,
}

impl Inner {
    fn register_command_selector(&self, selector: ITerminalCommandSelector) {
        self.selectors.borrow_mut().insert(selector.id.clone(), selector.clone());
        self.on_did_register_command_selector.fire(selector.clone());

        // Check if there's a pending provider for this selector
        let pending_provider = self.pending_providers.borrow_mut().remove(&selector.id);
        if let Some(pending_provider) = pending_provider {
            self.providers.borrow_mut().insert(selector.id.clone(), pending_provider.clone());
            self.on_did_register_provider.fire(ITerminalQuickFixProviderSelector { selector, provider: pending_provider });
        }
    }
}

/// `TerminalQuickFixService`.
pub struct TerminalQuickFixService {
    inner: Rc<Inner>,
    /// O registro do tratador do ponto de extensão (o original o deixa vivo para sempre).
    _handler: DisposableRef,
}

impl TerminalQuickFixService {
    pub fn new(quick_fix_extension_point: &ExtensionPoint) -> Self {
        let mut handler_registration: Option<DisposableRef> = None;
        let extension_quick_fixes = Promise::with_executor(|resolver| {
            handler_registration = Some(quick_fix_extension_point.set_handler(Rc::new(move |users: &[ExtensionPointUser<Value>], _delta| {
                let selectors = users
                    .iter()
                    .filter(|user| is_proposed_api_enabled(&user.description, "terminalQuickFixProvider"))
                    .flat_map(|user| match user.value.as_array() {
                        Some(fixes) => fixes.iter().filter_map(|fix| parse_selector(fix, &user.description.identifier.value)).collect::<Vec<_>>(),
                        None => Vec::new(),
                    })
                    .collect();
                resolver.resolve(selectors);
            })));
        });
        let inner = Rc::new(Inner {
            selectors: RefCell::new(HashMap::new()),
            providers: Rc::new(RefCell::new(HashMap::new())),
            pending_providers: RefCell::new(HashMap::new()),
            on_did_register_provider: Emitter::new(),
            on_did_register_command_selector: Emitter::new(),
            on_did_unregister_provider: Emitter::new(),
            extension_quick_fixes: extension_quick_fixes.clone(),
        });
        {
            let inner = inner.clone();
            extension_quick_fixes.then(move |selectors| {
                for selector in selectors {
                    inner.register_command_selector(selector);
                }
            });
        }
        Self { inner, _handler: handler_registration.expect("Promise::with_executor roda o executor na hora") }
    }
}

impl ITerminalQuickFixService for TerminalQuickFixService {
    fn on_did_register_provider(&self) -> Event<ITerminalQuickFixProviderSelector> {
        self.inner.on_did_register_provider.event()
    }

    fn on_did_register_command_selector(&self) -> Event<ITerminalCommandSelector> {
        self.inner.on_did_register_command_selector.event()
    }

    fn on_did_unregister_provider(&self) -> Event<String> {
        self.inner.on_did_unregister_provider.event()
    }

    fn extension_quick_fixes(&self) -> Promise<Vec<ITerminalCommandSelector>> {
        self.inner.extension_quick_fixes.clone()
    }

    fn providers(&self) -> Providers {
        self.inner.providers.clone()
    }

    fn register_command_selector(&self, selector: ITerminalCommandSelector) {
        self.inner.register_command_selector(selector);
    }

    fn register_quick_fix_provider(&self, id: &str, provider: Rc<dyn ITerminalQuickFixProvider>) -> DisposableRef {
        // This is more complicated than it looks like it should be because we need to return an
        // IDisposable synchronously but we must await ITerminalContributionService.quickFixes
        // asynchronously before actually registering the provider.
        let disposed = Rc::new(Cell::new(false));
        {
            let inner = self.inner.clone();
            let disposed = disposed.clone();
            let id = id.to_owned();
            self.inner.extension_quick_fixes.then(move |_| {
                if disposed.get() {
                    return;
                }
                let selector = inner.selectors.borrow().get(&id).cloned();
                if let Some(selector) = selector {
                    // Selector is already available, register immediately
                    inner.providers.borrow_mut().insert(id, provider.clone());
                    inner.on_did_register_provider.fire(ITerminalQuickFixProviderSelector { selector, provider });
                } else {
                    // Selector not yet available, store provider as pending
                    inner.pending_providers.borrow_mut().insert(id, provider);
                }
            });
        }
        let inner = self.inner.clone();
        let id = id.to_owned();
        to_disposable(move || {
            disposed.set(true);
            inner.providers.borrow_mut().remove(&id);
            inner.pending_providers.borrow_mut().remove(&id);
            let selector = inner.selectors.borrow_mut().remove(&id);
            if let Some(selector) = selector {
                inner.on_did_unregister_provider.fire(selector.id);
            }
        })
    }
}

impl IDisposable for TerminalQuickFixService {
    fn dispose(&self) {
        self._handler.dispose();
    }
}
