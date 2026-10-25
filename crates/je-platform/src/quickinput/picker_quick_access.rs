//! Porte de `src/vs/platform/quickinput/browser/pickerQuickAccess.ts`: o provedor base dos acessos rápidos que são um
//! seletor com itens calculados pelo próprio provedor (`PickerQuickAccessProvider`), com as ações de gatilho
//! (`TriggerAction`), os resultados rápidos e lentos (`FastAndSlowPicks`) e o aceite do item.
//!
//! Notas de tradução:
//! - **Itens com métodos.** `IPickerQuickAccessItem extends IQuickPickItem` acrescenta `accept`, `trigger` e `attach`. O item
//!   do seletor do porte é o `IQuickPickItem` de dados, então esses métodos vão num [`PickerItemHandlers`] guardado no `extra`
//!   do item (o lugar que o contrato reserva aos campos do tipo derivado), com [`PickerItemHandlers::attach_to`] e
//!   [`PickerItemHandlers::of`]. O `data` do mesmo objeto leva os campos que um provedor derivado acrescenta (o `resource`, o
//!   `range`, o `editor` do `IAnythingQuickPickItem`).
//! - **Identidade de item.** `picks.indexOf(item)` e `activeItems.filter(i => i !== removed)` comparam objetos. Os itens do
//!   porte são valores clonados, então [`same_item`] compara os `PickerItemHandlers` por identidade quando existem e, senão, o
//!   `id` ou o rótulo, a descrição e o detalhe.
//! - **Herança.** O provedor abstrato é [`PickerQuickAccessProvider`], e `_getPicks` abstrato é o trait
//!   [`IPickerQuickAccessSource`], que o provedor derivado implementa e passa a `provide`.
//! - **Tipos de resultado.** `Picks<T> | Promise<Picks<T> | FastAndSlowPicks<T>> | FastAndSlowPicks<T> | null` é
//!   [`ProvidedPicks`]; `Picks<T>` (lista ou `PicksWithActive`) é [`Picks`].
//! - **Separadores com gatilho.** `IPickerQuickAccessSeparator.trigger` não é portado: o separador do porte não tem onde
//!   guardar o método, e o único provedor portado (`AnythingQuickAccessProvider`) não os usa.
//! - **`async`.** `updatePickerItems` e `buttonTrigger` são [`async_fn`]: o trecho até o primeiro `await` roda já, como no
//!   JavaScript.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{async_fn, timeout};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::js_string::trim;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::promise::Promise;

use super::quick_access::{AsQuickAccessRunOptions, IQuickAccessProvider};
use super::quick_input::{
    is_key_modified, IKeyMods, IQuickPick, IQuickPickDidAcceptEvent, IQuickPickItem, IQuickPickItemButtonEvent, QuickInputButtonRef, QuickPickInput,
};

/// `TriggerAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerAction {
    /// Do nothing after the button was clicked.
    NoAction,
    /// Close the picker.
    ClosePicker,
    /// Update the results of the picker.
    RefreshPicker,
    /// Remove the item from the picker.
    RemoveItem,
}

/// `TriggerAction | Promise<TriggerAction>`, o retorno de `trigger`.
#[derive(Clone)]
pub enum TriggerResult {
    Now(TriggerAction),
    Later(Promise<TriggerAction>),
}

/// `accept(keyMods, event)`.
pub type AcceptFn = Rc<dyn Fn(&IKeyMods, &IQuickPickDidAcceptEvent)>;
/// `trigger(buttonIndex, keyMods)`.
pub type TriggerFn = Rc<dyn Fn(usize, &IKeyMods) -> TriggerResult>;

/// Os métodos de `IPickerQuickAccessItem` (ver as notas do módulo).
#[derive(Clone, Default)]
pub struct PickerItemHandlers {
    /// A method that will be executed when the pick item is accepted from the picker. The picker will close automatically
    /// before running this.
    pub accept: Option<AcceptFn>,
    /// A method that will be executed when a button of the pick item was clicked on.
    pub trigger: Option<TriggerFn>,
    /// When set, this will be invoked instead of `accept` if modifier keys are held down.
    pub attach: Option<AcceptFn>,
    /// Os campos do tipo derivado do item.
    pub data: Option<Rc<dyn Any>>,
}

impl PickerItemHandlers {
    /// Grava os métodos no `extra` do item.
    pub fn attach_to(self, item: &mut IQuickPickItem) {
        item.extra = Dynamic::Object(ObjectRef::with_host(Rc::new(self)));
    }

    /// Os métodos do item, se ele os tem.
    pub fn of(item: &IQuickPickItem) -> Option<Rc<PickerItemHandlers>> {
        match &item.extra {
            Dynamic::Object(object) => object.host::<PickerItemHandlers>(),
            _ => None,
        }
    }

    /// Os dados do tipo derivado, se forem do tipo `T`.
    pub fn data_of<T: 'static>(item: &IQuickPickItem) -> Option<Rc<T>> {
        Self::of(item)?.data.clone()?.downcast::<T>().ok()
    }
}

/// O `===` entre dois itens do seletor (ver as notas do módulo).
pub fn same_item(a: &IQuickPickItem, b: &IQuickPickItem) -> bool {
    match (PickerItemHandlers::of(a), PickerItemHandlers::of(b)) {
        (Some(handlers_a), Some(handlers_b)) => return Rc::ptr_eq(&handlers_a, &handlers_b),
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (&a.base.id, &b.base.id) {
        (Some(id_a), Some(id_b)) => id_a == id_b,
        _ => a.base.label == b.base.label && a.base.description == b.base.description && a.detail == b.detail,
    }
}

/// `IPickerQuickAccessProviderOptions.noResultsPick`: `T | ((filter: string) => T)`.
#[derive(Clone)]
pub enum NoResultsPick {
    Item(IQuickPickItem),
    Func(Rc<dyn Fn(&str) -> IQuickPickItem>),
}

/// `IPickerQuickAccessProviderOptions<T>`.
#[derive(Clone, Default)]
pub struct IPickerQuickAccessProviderOptions {
    /// Enables support for opening picks in the background via gesture.
    pub can_accept_in_background: bool,
    /// Enables to show a pick entry when no results are returned from a search.
    pub no_results_pick: Option<NoResultsPick>,
    /// Whether to skip trimming the pick filter string.
    pub should_skip_trim_pick_filter: bool,
}

/// `PicksWithActive<T>`.
#[derive(Clone, Default)]
pub struct PicksWithActive {
    pub items: Vec<QuickPickInput>,
    pub active: Option<IQuickPickItem>,
}

/// `Picks<T>`: a lista ou a lista com o item ativo.
#[derive(Clone)]
pub enum Picks {
    Items(Vec<QuickPickInput>),
    WithActive(PicksWithActive),
}

impl Picks {
    fn split(&self) -> (Vec<QuickPickInput>, Option<IQuickPickItem>) {
        match self {
            Picks::Items(items) => (items.clone(), None),
            Picks::WithActive(with_active) => (with_active.items.clone(), with_active.active.clone()),
        }
    }
}

/// `FastAndSlowPicks<T>`.
#[derive(Clone)]
pub struct FastAndSlowPicks {
    /// Picks that will show instantly or after a short delay based on the `mergeDelay` property to reduce flicker.
    pub picks: Picks,
    /// Picks that will show after they have been resolved.
    pub additional_picks: Promise<Picks>,
    /// A delay in milliseconds to wait before showing the `picks` to give a chance to merge with `additionalPicks` for
    /// reduced flicker.
    pub merge_delay: Option<u64>,
}

/// `Picks<T> | FastAndSlowPicks<T>`, o valor de uma promessa de `_getPicks`.
#[derive(Clone)]
pub enum PromisedPicks {
    Picks(Picks),
    FastAndSlow(FastAndSlowPicks),
}

/// O retorno de `_getPicks`.
#[derive(Clone)]
pub enum ProvidedPicks {
    /// Pickers can return `null` to signal that no change in picks is needed.
    None,
    Picks(Picks),
    FastAndSlow(FastAndSlowPicks),
    Promised(Promise<PromisedPicks>),
}

/// `_getPicks`, o método abstrato que o provedor derivado implementa.
pub trait IPickerQuickAccessSource<O> {
    /// Returns an array of picks and separators as needed. The implementor is responsible for filtering and sorting the picks
    /// given the provided `filter`.
    fn get_picks(&self, filter: &str, disposables: &Rc<DisposableStore>, token: CancellationToken, run_options: Option<&Rc<O>>) -> ProvidedPicks;
}

/// `PickerQuickAccessProvider`.
pub struct PickerQuickAccessProvider {
    base: Rc<Disposable>,
    prefix: String,
    options: Option<IPickerQuickAccessProviderOptions>,
}

/// `substring(start)` do JavaScript (em unidades UTF-16) e `trim`.
fn picks_filter_of(value: &str, prefix: &str, skip_trim: bool) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    let start = prefix.encode_utf16().count().min(units.len());
    let rest = &units[start..];
    String::from_utf16_lossy(if skip_trim { rest } else { trim(rest) })
}

/// O que `updatePickerItems` compartilha com os trechos assíncronos de uma rodada.
struct ApplyContext<O> {
    picker: Rc<dyn IQuickPick>,
    picks_filter: String,
    picks_token: CancellationToken,
    options: Option<IPickerQuickAccessProviderOptions>,
    _run_options: Option<Rc<O>>,
}

impl<O> ApplyContext<O> {
    fn apply_picks(&self, picks: &Picks, skip_empty: bool) -> bool {
        let (mut items, active_item) = picks.split();

        if items.is_empty() {
            if skip_empty {
                return false;
            }

            // We show the no results pick if we have no input to prevent completely empty pickers #172613
            if self.picks_filter.encode_utf16().count() > 0 || self.picker.hide_input() {
                if let Some(no_results_pick) = self.options.as_ref().and_then(|options| options.no_results_pick.as_ref()) {
                    items = vec![QuickPickInput::Item(match no_results_pick {
                        NoResultsPick::Func(func) => func(&self.picks_filter),
                        NoResultsPick::Item(item) => item.clone(),
                    })];
                }
            }
        }

        self.picker.set_items(items);
        if let Some(active_item) = active_item {
            self.picker.set_active_items(vec![active_item]);
        }

        true
    }
}

fn apply_fast_and_slow_picks<O: 'static>(ctx: Rc<ApplyContext<O>>, fast_and_slow_picks: FastAndSlowPicks) -> Promise<()> {
    let fast_picks_applied = Rc::new(Cell::new(false));
    let slow_picks_applied = Rc::new(Cell::new(false));

    // Fast Picks: if `mergeDelay` is configured, in order to reduce amount of flicker, we race against the slow picks over
    // some delay and then set the fast picks. If the slow picks are faster, we reduce the flicker by only setting the items
    // once.
    let fast = {
        let (ctx, fast_picks_applied, slow_picks_applied, fast_and_slow_picks) =
            (ctx.clone(), fast_picks_applied.clone(), slow_picks_applied.clone(), fast_and_slow_picks.clone());
        async_fn(async move {
            if let Some(merge_delay) = fast_and_slow_picks.merge_delay {
                timeout(merge_delay).await?;
                if ctx.picks_token.is_cancellation_requested() {
                    return Ok(());
                }
            }

            if !slow_picks_applied.get() {
                // skip over empty to reduce flicker
                fast_picks_applied.set(ctx.apply_picks(&fast_and_slow_picks.picks, true));
            }
            Ok(())
        })
    };

    // Slow Picks: we await the slow picks and then set them at once together with the fast picks, but only if we actually
    // have additional results.
    let slow = {
        let (ctx, fast_picks_applied, slow_picks_applied) = (ctx.clone(), fast_picks_applied.clone(), slow_picks_applied.clone());
        async_fn(async move {
            ctx.picker.set_busy(true);
            let awaited_additional_picks = fast_and_slow_picks.additional_picks.clone().await;
            let outcome = (|| -> Result<(), Error> {
                let awaited_additional_picks = awaited_additional_picks?;
                if ctx.picks_token.is_cancellation_requested() {
                    return Ok(());
                }

                let (picks, active_pick) = fast_and_slow_picks.picks.split();
                let (additional_picks, additional_active_pick) = awaited_additional_picks.split();

                if !additional_picks.is_empty() || !fast_picks_applied.get() {
                    // If we do not have any activePick or additionalActivePick we try to preserve the currently active pick
                    // from the fast results. This fixes an issue where the user might have made a pick active before the
                    // additional results kick in. See https://github.com/microsoft/vscode/issues/102480
                    let mut fallback_active_pick: Option<IQuickPickItem> = None;
                    if active_pick.is_none() && additional_active_pick.is_none() {
                        let fallback_active_pick_candidate = ctx.picker.active_items().into_iter().next();
                        if let Some(candidate) = fallback_active_pick_candidate {
                            if picks.iter().filter_map(QuickPickInput::as_item).any(|pick| same_item(pick, &candidate)) {
                                fallback_active_pick = Some(candidate);
                            }
                        }
                    }

                    let mut items = picks;
                    items.extend(additional_picks);
                    ctx.apply_picks(&Picks::WithActive(PicksWithActive { items, active: active_pick.or(additional_active_pick).or(fallback_active_pick) }), false);
                }
                Ok(())
            })();

            if !ctx.picks_token.is_cancellation_requested() {
                ctx.picker.set_busy(false);
            }

            slow_picks_applied.set(true);
            outcome
        })
    };

    async_fn(async move {
        Promise::all(vec![fast, slow]).await?;
        Ok(())
    })
}

/// O estado de uma exibição do seletor (`provide`).
struct Session<O: AsQuickAccessRunOptions + 'static> {
    prefix: String,
    options: Option<IPickerQuickAccessProviderOptions>,
    source: Rc<dyn IPickerQuickAccessSource<O>>,
    picker: Rc<dyn IQuickPick>,
    token: CancellationToken,
    run_options: Option<Rc<O>>,
    picks_cts: RefCell<Option<CancellationTokenSource>>,
    picks_disposable: Rc<MutableDisposable<DisposableStore>>,
}

impl<O: AsQuickAccessRunOptions + 'static> Session<O> {
    fn update_picker_items(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // Cancel any previous ask for picks and busy
            let previous = this.picks_cts.borrow().clone();
            if let Some(previous) = previous {
                previous.dispose_with(true);
            }
            this.picker.set_busy(false);

            // Setting the .value will call dispose() on the previous value, so we need to do this AFTER cancelling with
            // dispose(true).
            let picks_disposables = DisposableStore::new();
            this.picks_disposable.set(Some(picks_disposables.clone()));

            // Create new cancellation source for this run
            let cts = CancellationTokenSource::new(Some(&this.token));
            picks_disposables.add_rc(Rc::new(cts.clone()));
            *this.picks_cts.borrow_mut() = Some(cts.clone());

            // Collect picks and support both long running and short or combined
            let picks_token = cts.token();
            let picks_filter = picks_filter_of(
                &this.picker.value(),
                &this.prefix,
                this.options.as_ref().is_some_and(|options| options.should_skip_trim_pick_filter),
            );

            let provided_picks = this.source.get_picks(&picks_filter, &picks_disposables, picks_token.clone(), this.run_options.as_ref());

            let ctx = Rc::new(ApplyContext {
                picker: this.picker.clone(),
                picks_filter,
                picks_token: picks_token.clone(),
                options: this.options.clone(),
                _run_options: this.run_options.clone(),
            });

            match provided_picks {
                // No Picks
                ProvidedPicks::None => {}

                // Fast and Slow Picks
                ProvidedPicks::FastAndSlow(fast_and_slow_picks) => {
                    apply_fast_and_slow_picks(ctx, fast_and_slow_picks).await?;
                }

                // Fast Picks
                ProvidedPicks::Picks(picks) => {
                    ctx.apply_picks(&picks, false);
                }

                // Slow Picks
                ProvidedPicks::Promised(promised) => {
                    this.picker.set_busy(true);
                    let awaited_picks = promised.await;
                    let outcome = async {
                        let awaited_picks = awaited_picks?;
                        if picks_token.is_cancellation_requested() {
                            return Ok(());
                        }

                        match awaited_picks {
                            PromisedPicks::FastAndSlow(fast_and_slow_picks) => apply_fast_and_slow_picks(ctx.clone(), fast_and_slow_picks).await,
                            PromisedPicks::Picks(picks) => {
                                ctx.apply_picks(&picks, false);
                                Ok(())
                            }
                        }
                    }
                    .await;
                    if !picks_token.is_cancellation_requested() {
                        this.picker.set_busy(false);
                    }
                    outcome?;
                }
            }
            Ok(())
        })
    }

    fn button_trigger(self: &Rc<Self>, button: QuickInputButtonRef, item: IQuickPickItem) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(trigger) = PickerItemHandlers::of(&item).and_then(|handlers| handlers.trigger.clone()) else {
                return Ok(());
            };

            let button_index = item.base.buttons.as_ref().and_then(|buttons| buttons.iter().position(|candidate| Rc::ptr_eq(candidate, &button)));
            if let Some(button_index) = button_index {
                let result = trigger(button_index, &this.picker.key_mods());
                let action = match result {
                    TriggerResult::Now(action) => action,
                    TriggerResult::Later(promise) => promise.await?,
                };

                if this.token.is_cancellation_requested() {
                    return Ok(());
                }

                match action {
                    TriggerAction::NoAction => {}
                    TriggerAction::ClosePicker => this.picker.hide(),
                    TriggerAction::RefreshPicker => {
                        let _ = this.update_picker_items();
                    }
                    TriggerAction::RemoveItem => {
                        let mut items = this.picker.items();
                        let index = items.iter().position(|candidate| candidate.as_item().is_some_and(|candidate| same_item(candidate, &item)));
                        if let Some(index) = index {
                            items.remove(index);
                            let active_items: Vec<IQuickPickItem> =
                                this.picker.active_items().into_iter().filter(|active_item| !same_item(active_item, &item)).collect();
                            let keep_scroll_position_before = this.picker.keep_scroll_position();
                            this.picker.set_keep_scroll_position(true);
                            this.picker.set_items(items);
                            this.picker.set_active_items(active_items);
                            this.picker.set_keep_scroll_position(keep_scroll_position_before);
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

impl PickerQuickAccessProvider {
    pub fn new(prefix: impl Into<String>, options: Option<IPickerQuickAccessProviderOptions>) -> Self {
        Self { base: Disposable::new(), prefix: prefix.into(), options }
    }

    pub fn options(&self) -> Option<&IPickerQuickAccessProviderOptions> {
        self.options.as_ref()
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// O `Disposable` base do provedor, para o derivado registrar o que ele cria.
    pub fn disposable(&self) -> &Rc<Disposable> {
        &self.base
    }

    /// `provide(picker, token, runOptions)`.
    pub fn provide<O: AsQuickAccessRunOptions + 'static>(
        &self,
        source: Rc<dyn IPickerQuickAccessSource<O>>,
        picker: Rc<dyn IQuickPick>,
        token: CancellationToken,
        run_options: Option<Rc<O>>,
    ) -> DisposableRef {
        let disposables = DisposableStore::new();

        // Apply options if any
        picker.set_can_accept_in_background(self.options.as_ref().is_some_and(|options| options.can_accept_in_background));

        // Disable filtering & sorting, we control the results
        picker.set_match_on_label(false);
        picker.set_match_on_description(false);
        picker.set_match_on_detail(false);
        picker.set_sort_by_label(false);

        // Set initial picks and update on type
        let picks_disposable = MutableDisposable::<DisposableStore>::new();
        disposables.add_rc(picks_disposable.clone());
        let session = Rc::new(Session {
            prefix: self.prefix.clone(),
            options: self.options.clone(),
            source,
            picker: picker.clone(),
            token: token.clone(),
            run_options: run_options.clone(),
            picks_cts: RefCell::new(None),
            picks_disposable,
        });
        let on_change = session.clone();
        disposables.add(picker.on_did_change_value().listen(move |_| {
            let _ = on_change.update_picker_items();
        }));
        let _ = session.update_picker_items();

        // Accept the pick on accept and hide picker
        let accept_picker = picker.clone();
        disposables.add(picker.on_did_accept().listen(move |event| {
            let handle_accept = run_options.as_ref().and_then(|options| options.base().handle_accept.clone());
            if let Some(handle_accept) = handle_accept {
                if !event.in_background {
                    accept_picker.hide(); // hide picker unless we accept in background
                }
                if let Some(active_item) = accept_picker.active_items().first() {
                    handle_accept(active_item, event.in_background);
                }
                return;
            }

            let Some(item) = accept_picker.selected_items().into_iter().next() else {
                return;
            };
            let Some(handlers) = PickerItemHandlers::of(&item) else {
                return;
            };
            if let Some(accept) = handlers.accept.clone() {
                let key_mods = accept_picker.key_mods();
                let is_attach_action = is_key_modified(&key_mods) && handlers.attach.is_some();
                if is_attach_action {
                    if let Some(attach) = &handlers.attach {
                        attach(&key_mods, event);
                    }
                    return;
                }
                if !event.in_background {
                    accept_picker.hide(); // hide picker unless we accept in background
                }

                accept(&key_mods, event);
            }
        }));

        // Trigger the pick with button index if button triggered
        let trigger_session = session.clone();
        disposables.add(picker.on_did_trigger_item_button().listen(move |IQuickPickItemButtonEvent { button, item }| {
            let _ = trigger_session.button_trigger(button.clone(), item.clone());
        }));

        disposables
    }
}

impl IDisposable for PickerQuickAccessProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O contrato `IQuickAccessProvider` de um provedor que só delega ao [`PickerQuickAccessProvider`].
pub struct PickerProviderAdapter<O: AsQuickAccessRunOptions + 'static> {
    pub provider: PickerQuickAccessProvider,
    pub source: Rc<dyn IPickerQuickAccessSource<O>>,
}

impl<O: AsQuickAccessRunOptions + 'static> IQuickAccessProvider<O> for PickerProviderAdapter<O> {
    fn as_disposable(&self) -> Option<&dyn IDisposable> {
        Some(&self.provider)
    }

    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, options: Option<Rc<O>>) -> DisposableRef {
        self.provider.provide(self.source.clone(), picker, token, options)
    }
}
