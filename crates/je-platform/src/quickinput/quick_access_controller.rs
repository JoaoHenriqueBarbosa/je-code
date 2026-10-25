//! Porte de `src/vs/platform/quickinput/browser/quickAccess.ts`: o `QuickAccessController`, que escolhe o provedor pelo
//! prefixo do valor, monta o seletor, repassa o valor entre provedores e resolve o `pick`.
//!
//! Notas de tradução:
//! - **`@IQuickInputService`, `@IInstantiationService` e `@IContextKeyService`.** São as dependências do [`Injectable`], na
//!   ordem do construtor. O registro é o `Registry.as<IQuickAccessRegistry>(Extensions.Quickaccess)` da thread.
//! - **`Map<IQuickAccessProviderDescriptor, ...>`.** As chaves são os descritores compartilhados por identidade (`Rc`), então
//!   os dois mapas são listas de pares comparados com [`Rc::ptr_eq`], na ordem de inserção (a do `Map`).
//! - **`createInstance(providerDescriptor.ctor)`.** É o `create_instance_from_descriptor` do serviço de instanciação.
//! - **`[provider, descriptor]`.** O par `[undefined, undefined]` é `None`; os dois existem juntos.
//! - **`pick()` sem seletor novo.** Quando o acesso rápido já mostra o mesmo prefixo, o original retorna `undefined` (sem
//!   promessa); o porte devolve uma promessa já resolvida com `None`, que o `await` do original lê do mesmo jeito.
//! - **`substr`, `substring` e `length`.** Contam unidades UTF-16, como o JavaScript.
//! - **`Event.once(picker.onDidHide)(...)`.** O ouvinte entra no `DisposableStore` do mostrar (o original não guarda o retorno,
//!   e o ouvinte sai quando o evento dispara ou o seletor é descartado); o `once` o solta no disparo do mesmo jeito.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, Promise};
use je_instantiation::{Injectable, InstantiationService};

use super::quick_access::{
    DefaultQuickAccessFilterValue, IQuickAccessController, IQuickAccessOptions, IQuickAccessProvider, IQuickAccessProviderDescriptor, IQuickAccessRegistry,
    quick_access_registry,
};
use super::quick_input::{IQuickInputService, IQuickPick, IQuickPickItem, ItemActivation, QuickPickCreateOptions};
use crate::contextkey::contextkey::IContextKeyService;

type Provider = Rc<dyn IQuickAccessProvider<dyn Any>>;
type Descriptor = Rc<IQuickAccessProviderDescriptor>;

/// O `.length` de JavaScript (unidades UTF-16).
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text.substring(start)` / `text.substr(start)` do JavaScript.
fn js_substring_from(text: &str, start: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[start.min(units.len())..])
}

/// `visibleQuickAccess`.
struct VisibleQuickAccess {
    picker: Rc<dyn IQuickPick>,
    descriptor: Option<Descriptor>,
    value: RefCell<String>,
}

fn same_provider(a: &Option<Provider>, b: &Option<Provider>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `QuickAccessController`.
pub struct QuickAccessController {
    this: Weak<QuickAccessController>,
    base: Rc<Disposable>,
    quick_input_service: Rc<dyn IQuickInputService>,
    instantiation_service: Rc<InstantiationService>,
    context_key_service: Rc<dyn IContextKeyService>,
    registry: Rc<dyn IQuickAccessRegistry>,
    map_provider_to_descriptor: RefCell<Vec<(Descriptor, Provider)>>,
    last_accepted_picker_values: RefCell<Vec<(Descriptor, String)>>,
    visible_quick_access: RefCell<Option<Rc<VisibleQuickAccess>>>,
}

impl Injectable for QuickAccessController {
    type Args = ();
    type Deps = (Rc<dyn IQuickInputService>, Rc<InstantiationService>, Rc<dyn IContextKeyService>);

    fn create(_: (), (quick_input_service, instantiation_service, context_key_service): Self::Deps) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            quick_input_service,
            instantiation_service,
            context_key_service,
            registry: quick_access_registry(),
            map_provider_to_descriptor: RefCell::new(Vec::new()),
            last_accepted_picker_values: RefCell::new(Vec::new()),
            visible_quick_access: RefCell::new(None),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.dispose_providers_and_picker();
            }
        }));

        this
    }
}

impl IDisposable for QuickAccessController {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IQuickAccessController for QuickAccessController {
    fn pick(&self, value: &str, options: Option<IQuickAccessOptions>) -> Promise<Option<Vec<IQuickPickItem>>> {
        self.do_show_or_pick(value, true, options).unwrap_or_else(|| Promise::resolved(None))
    }

    fn show(&self, value: &str, options: Option<IQuickAccessOptions>) {
        self.do_show_or_pick(value, false, options);
    }
}

impl QuickAccessController {
    /// O que o `toDisposable` do construtor faz: descarta os provedores que têm `dispose` e o seletor visível.
    fn dispose_providers_and_picker(&self) {
        let providers: Vec<Provider> = self.map_provider_to_descriptor.borrow().iter().map(|(_, provider)| provider.clone()).collect();
        for provider in providers {
            if let Some(disposable) = provider.as_disposable() {
                disposable.dispose();
            }
        }

        let visible = self.visible_quick_access.borrow().clone();
        if let Some(visible) = visible {
            visible.picker.dispose();
        }
    }

    fn do_show_or_pick(&self, value: &str, pick: bool, options: Option<IQuickAccessOptions>) -> Option<Promise<Option<Vec<IQuickPickItem>>>> {
        let options = options.unwrap_or_default();
        let mut value = value.to_string();

        // Find provider for the value to show
        let found = self.get_or_instantiate_provider(&value, options.enabled_provider_prefixes.as_deref(), options.provider_prefix.as_deref());
        let (provider, descriptor) = match found {
            Some((provider, descriptor)) => (Some(provider), Some(descriptor)),
            None => (None, None),
        };

        // Return early if quick access is already showing on that same prefix
        let visible_quick_access = self.visible_quick_access.borrow().clone();
        let visible_descriptor = visible_quick_access.as_ref().and_then(|visible| visible.descriptor.clone());
        if let (Some(visible_quick_access), Some(descriptor)) = (&visible_quick_access, &descriptor) {
            if visible_descriptor.as_ref().is_some_and(|visible_descriptor| Rc::ptr_eq(visible_descriptor, descriptor)) {
                // Apply value only if it is more specific than the prefix
                // from the provider and we are not instructed to preserve
                if value != descriptor.prefix && !options.preserve_value.unwrap_or(false) {
                    visible_quick_access.picker.set_value(value);
                }

                // Always adjust selection
                self.adjust_value_selection(&visible_quick_access.picker, Some(descriptor), &options);

                return None;
            }
        }

        // Rewrite the filter value based on certain rules unless disabled
        if let (Some(descriptor), Some(provider)) = (&descriptor, &provider) {
            if !options.preserve_value.unwrap_or(false) {
                let mut new_value: Option<String> = None;

                // If we have a visible provider with a value, take it's filter value but
                // rewrite to new provider prefix in case they differ
                if let (Some(visible_quick_access), Some(visible_descriptor)) = (&visible_quick_access, &visible_descriptor) {
                    if !Rc::ptr_eq(visible_descriptor, descriptor) {
                        let new_value_candidate_without_prefix = js_substring_from(&visible_quick_access.value.borrow(), js_length(&visible_descriptor.prefix));
                        if !new_value_candidate_without_prefix.is_empty() {
                            new_value = Some(format!("{}{}", descriptor.prefix, new_value_candidate_without_prefix));
                        }
                    }
                }

                // Otherwise, take a default value as instructed
                if new_value.is_none() {
                    if provider.default_filter_value() == Some(DefaultQuickAccessFilterValue::Last) {
                        new_value = self
                            .last_accepted_picker_values
                            .borrow()
                            .iter()
                            .find(|(candidate, _)| Rc::ptr_eq(candidate, descriptor))
                            .map(|(_, last_value)| last_value.clone());
                    } else if let Some(default_filter_text) = provider.default_filter_text() {
                        new_value = Some(format!("{}{}", descriptor.prefix, default_filter_text));
                    }
                }

                if let Some(new_value) = new_value {
                    value = new_value;
                }
            }
        }

        // Store the existing selection if there was one.
        let visible_selection = visible_quick_access.as_ref().and_then(|visible| visible.picker.value_selection());
        let visible_value = visible_quick_access.as_ref().map(|visible| visible.picker.value());

        // Create a picker for the provider to use with the initial value
        // and adjust the filtering to exclude the prefix from filtering
        let disposables = DisposableStore::new();
        let picker = self.quick_input_service.create_quick_pick(Some(QuickPickCreateOptions { use_separators: true }));
        let picker_disposable: DisposableRef = picker.clone();
        disposables.add(picker_disposable);
        picker.set_value(value.clone());
        self.adjust_value_selection(&picker, descriptor.as_ref(), &options);
        picker.set_placeholder(options.placeholder.clone().or_else(|| descriptor.as_ref().and_then(|descriptor| descriptor.placeholder.clone())));
        picker.set_anchor(options.anchor);
        picker.set_anchor_position(options.anchor_position);
        picker.set_quick_navigate(options.quick_navigate_configuration.clone());
        picker.set_hide_input(picker.quick_navigate().is_some() && visible_quick_access.is_none()); // only hide input if there was no picker opened already
        if options.item_activation.is_some() || options.quick_navigate_configuration.is_some() {
            picker.set_item_activation(options.item_activation.unwrap_or(ItemActivation::Second /* quick nav is always second */));
        }
        picker.set_context_key(descriptor.as_ref().and_then(|descriptor| descriptor.context_key.clone()));
        let prefix_length = descriptor.as_ref().map_or(0, |descriptor| js_length(&descriptor.prefix));
        picker.set_filter_value(Rc::new(move |value: &str| js_substring_from(value, prefix_length)));

        // Pick mode: setup a promise that can be resolved
        // with the selected items and prevent execution
        let pick_promise: Option<Rc<DeferredPromise<Option<Vec<IQuickPickItem>>>>> = pick.then(|| Rc::new(DeferredPromise::new()));
        if pick_promise.is_some() {
            let weak_picker = Rc::downgrade(&picker);
            disposables.add(picker.on_will_accept().once().listen(move |event| {
                event.veto();
                if let Some(picker) = weak_picker.upgrade() {
                    picker.hide();
                }
            }));
        }

        // Register listeners
        disposables.add(self.register_picker_listeners(&picker, provider.clone(), descriptor.clone(), &value, &options));

        // Ask provider to fill the picker as needed if we have one
        // and pass over a cancellation token that will indicate when
        // the picker is hiding without a pick being made.
        let cts = Rc::new(CancellationTokenSource::new(None));
        disposables.add_rc(cts.clone());
        if let Some(provider) = &provider {
            let token: CancellationToken = cts.token();
            disposables.add(provider.provide(picker.clone(), token, options.provider_options.clone()));
        }

        // Finally, trigger disposal and cancellation when the picker
        // hides depending on items selected or not.
        let hidden_picker = picker.clone();
        let hidden_pick_promise = pick_promise.clone();
        let hidden_disposables = disposables.clone();
        let hide_listener = picker.on_did_hide().once().listen(move |_| {
            if hidden_picker.selected_items().is_empty() {
                cts.cancel();
            }

            // Start to dispose once picker hides
            hidden_disposables.dispose();

            // Resolve pick promise with selected items
            if let Some(pick_promise) = &hidden_pick_promise {
                let _ = pick_promise.complete(Some(hidden_picker.selected_items()));
            }
        });
        disposables.add(hide_listener);

        // Finally, show the picker. This is important because a provider
        // may not call this and then our disposables would leak that rely
        // on the onDidHide event.
        picker.show();

        // If the previous picker had a selection and the value is unchanged, we should set that in the new picker.
        if let (Some(visible_selection), Some(visible_value)) = (visible_selection, &visible_value) {
            if *visible_value == value {
                picker.set_value_selection(Some(visible_selection));
            }
        }

        // Pick mode: return with promise
        pick_promise.map(|pick_promise| pick_promise.promise.clone())
    }

    fn adjust_value_selection(&self, picker: &Rc<dyn IQuickPick>, descriptor: Option<&Descriptor>, options: &IQuickAccessOptions) {
        let value_length = u32::try_from(js_length(&picker.value())).unwrap_or(u32::MAX);

        let value_selection = if options.preserve_value.unwrap_or(false) {
            // Preserve: just always put the cursor at the end
            (value_length, value_length)
        } else {
            // Otherwise: select the value up until the prefix
            (descriptor.map_or(0, |descriptor| u32::try_from(js_length(&descriptor.prefix)).unwrap_or(u32::MAX)), value_length)
        };

        picker.set_value_selection(Some(value_selection));
    }

    fn register_picker_listeners(
        &self,
        picker: &Rc<dyn IQuickPick>,
        provider: Option<Provider>,
        descriptor: Option<Descriptor>,
        value: &str,
        options: &IQuickAccessOptions,
    ) -> DisposableRef {
        let disposables = DisposableStore::new();

        // Remember as last visible picker and clean up once picker get's disposed
        let visible_quick_access = Rc::new(VisibleQuickAccess { picker: picker.clone(), descriptor: descriptor.clone(), value: RefCell::new(value.to_string()) });
        *self.visible_quick_access.borrow_mut() = Some(visible_quick_access.clone());
        let weak_this = self.this.clone();
        let visible_to_clear = visible_quick_access.clone();
        disposables.add(to_disposable(move || {
            if let Some(this) = weak_this.upgrade() {
                let mut current = this.visible_quick_access.borrow_mut();
                if current.as_ref().is_some_and(|current| Rc::ptr_eq(current, &visible_to_clear)) {
                    *current = None;
                }
            }
        }));

        // Whenever the value changes, check if the provider has
        // changed and if so - re-create the picker from the beginning
        let weak_this = self.this.clone();
        let options = options.clone();
        disposables.add(picker.on_did_change_value().listen(move |value| {
            let Some(this) = weak_this.upgrade() else {
                return;
            };
            let provider_for_value =
                this.get_or_instantiate_provider(value, options.enabled_provider_prefixes.as_deref(), options.provider_prefix.as_deref()).map(|(provider, _)| provider);
            if !same_provider(&provider_for_value, &provider) {
                this.show(
                    value,
                    Some(IQuickAccessOptions {
                        enabled_provider_prefixes: options.enabled_provider_prefixes.clone(),
                        provider_prefix: options.provider_prefix.clone(),
                        // do not rewrite value from user typing!
                        preserve_value: Some(true),
                        // persist the value of the providerOptions from the original showing
                        provider_options: options.provider_options.clone(),
                        ..IQuickAccessOptions::default()
                    }),
                );
            } else {
                *visible_quick_access.value.borrow_mut() = value.clone(); // remember the value in our visible one
            }
        }));

        // Remember picker input for future use when accepting
        if let Some(descriptor) = descriptor {
            let weak_this = self.this.clone();
            let weak_picker = Rc::downgrade(picker);
            disposables.add(picker.on_did_accept().listen(move |_| {
                let (Some(this), Some(picker)) = (weak_this.upgrade(), weak_picker.upgrade()) else {
                    return;
                };
                let mut last_accepted_picker_values = this.last_accepted_picker_values.borrow_mut();
                match last_accepted_picker_values.iter_mut().find(|(candidate, _)| Rc::ptr_eq(candidate, &descriptor)) {
                    Some((_, last_value)) => *last_value = picker.value(),
                    None => last_accepted_picker_values.push((descriptor.clone(), picker.value())),
                }
            }));
        }

        disposables
    }

    fn get_or_instantiate_provider(&self, value: &str, enabled_provider_prefixes: Option<&[String]>, provider_prefix: Option<&str>) -> Option<(Provider, Descriptor)> {
        let provider_descriptor = self.registry.get_quick_access_provider(provider_prefix.unwrap_or(value), &*self.context_key_service)?;
        if enabled_provider_prefixes.is_some_and(|prefixes| !prefixes.contains(&provider_descriptor.prefix)) {
            return None;
        }

        let existing =
            self.map_provider_to_descriptor.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, &provider_descriptor)).map(|(_, provider)| provider.clone());
        let provider = match existing {
            Some(provider) => provider,
            None => {
                let provider = self.instantiation_service.create_instance_from_descriptor(&provider_descriptor.ctor);
                self.map_provider_to_descriptor.borrow_mut().push((provider_descriptor.clone(), provider.clone()));
                provider
            }
        };

        Some((provider, provider_descriptor))
    }
}
