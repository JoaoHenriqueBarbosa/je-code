//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/virtualizedItemManager.ts`: o gerenciador que liga itens
//! virtualizados a modelos (`template`) reaproveitados de um pool por identificador de modelo, entregando à
//! [`crate::compressed_virtualized_scroll_view::CompressedVirtualizedScrollView`] itens que desfazem a ligação (`binding`)
//! quando saem da tela.
//!
//! Divergências:
//! - **Sem herança.** As classes abstratas `VirtualizedItemBinding` e `VirtualizedItemTemplate` viram structs base que a
//!   "subclasse" guarda (`base`) e consulta: [`VirtualizedItemBinding`] traz o `dispose` com o evento `onDidDispose`, e
//!   [`VirtualizedItemTemplate::bind`] recebe como argumento o `createBinding` (o método abstrato da subclasse). As
//!   interfaces `IVirtualizedItemBinding` e `IVirtualizedItemTemplate` são traits, e `IVirtualizedItemDelegate` (um objeto
//!   literal de funções no original) é uma struct de closures.
//! - `TBinding` é entregue como `Rc<TBinding>` (a ligação é compartilhada entre o template e o item). O `TItem` é
//!   comparado por identidade ([`StrictEquals`], o `===` do original; para `Rc<T>` é a identidade do ponteiro).
//! - O `getId(item): unknown` devolve um [`Dynamic`] (o `Map` do original compara por `SameValueZero`, que é o
//!   `PartialEq` do [`Dynamic`]), e a lista de itens é um `Rc<Vec<TItem>>` (um array do JavaScript é comparado por
//!   identidade).
//! - O contexto da vista ([`ICompressedVirtualizedScrollViewContext`]) não tem nós do DOM (ver a nota do módulo da vista).
//!   O `context?` do `ManagedVirtualizedItem.render` é um `Option` na função própria [`ManagedVirtualizedItem::render`]; a
//!   implementação do trait da vista sempre o entrega.
//! - O item gerenciado guarda um `Weak` do gerenciador (o gerenciador guarda os itens no cache de
//!   `mapObservableArrayCached`, o que fecharia um ciclo de `Rc`); o `acquire` falha com bug se o gerenciador já caiu.
//! - O `maxScroll` do item é um `derived` cujo valor era um objeto novo a cada cálculo (comparado por identidade no
//!   original); aqui [`MaxScroll`] é comparado por valor (`StrictEquals` definido neste módulo), então um cálculo que
//!   devolve o mesmo número não notifica de novo.
//! - O `IReference<TTemplate>` é um [`Reference`] de `Rc<TTemplate>` e o seu `dispose` é de chamada única
//!   (`plain_disposable`), como o sinalizador `disposed` do original.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, Reference, plain_disposable, to_disposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    IObservable, ITransaction, Observable, ObservableExt, SettableExt, StrictEquals, autorun, derived, map_observable_array_cached_by, observable_value, transaction,
};

use crate::compressed_virtualized_scroll_view::{
    ICompressedVirtualizedScrollItem, ICompressedVirtualizedScrollItemContext, ICompressedVirtualizedScrollViewContext, MaxScroll,
};
use crate::offset_range::OffsetRange;

/// Quantos templates ociosos o pool guarda; os que sobram são descartados ao voltar (o `5` do `release`).
const MAX_UNUSED_TEMPLATES: usize = 5;

/// O `MaxScroll` é um objeto no original (comparado por identidade); aqui é um valor, comparado por valor.
impl StrictEquals for MaxScroll {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

fn throw_bug(message: &str) -> ! {
    errors::throw(Error::bug_indicating(Some(message)))
}

/// O identificador de um item (`getId(item): unknown`).
pub type VirtualizedItemId = Dynamic;

/// `(getItemOffset, update) => void` do `runWithScrollAnchor`.
type RunWithScrollAnchor = dyn Fn(Rc<dyn Fn() -> i32>, &dyn Fn(&dyn ITransaction));

/// `IVirtualizedItemBindingContext`.
#[derive(Clone)]
pub struct IVirtualizedItemBindingContext {
    pub initial_size: i32,
    run_with_scroll_anchor: Rc<RunWithScrollAnchor>,
}

impl IVirtualizedItemBindingContext {
    pub fn new(initial_size: i32, run_with_scroll_anchor: impl Fn(Rc<dyn Fn() -> i32>, &dyn Fn(&dyn ITransaction)) + 'static) -> Self {
        Self { initial_size, run_with_scroll_anchor: Rc::new(run_with_scroll_anchor) }
    }

    /// `runWithScrollAnchor(getItemOffset, update)`.
    pub fn run_with_scroll_anchor(&self, get_item_offset: Rc<dyn Fn() -> i32>, update: &dyn Fn(&dyn ITransaction)) {
        (self.run_with_scroll_anchor)(get_item_offset, update);
    }
}

/// `IVirtualizedItemBinding<TItem>`.
pub trait IVirtualizedItemBinding<TItem>: IDisposable {
    fn item(&self) -> &TItem;
    fn size(&self) -> IObservable<i32>;
    fn max_scroll(&self) -> IObservable<MaxScroll>;
    fn should_keep_alive(&self) -> IObservable<bool>;
    fn on_did_dispose(&self) -> Event<()>;
    fn render(&self, rendered_range: OffsetRange, scroll_offset: i32, width: f64, rendered_viewport: OffsetRange);
    fn hide(&self);
}

/// `VirtualizedItemBinding<TItem>` (a classe abstrata): o item, o evento `onDidDispose` e o `Disposable` das subclasses.
pub struct VirtualizedItemBinding<TItem> {
    base: Rc<Disposable>,
    on_did_dispose: Rc<Emitter<()>>,
    pub item: TItem,
}

impl<TItem> VirtualizedItemBinding<TItem> {
    pub fn new(item: TItem) -> Self {
        let base = Disposable::new();
        let on_did_dispose = Emitter::new();
        base.register(on_did_dispose.clone());
        Self { base, on_did_dispose, item }
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    /// `_register(o)`, para as subclasses.
    pub fn base(&self) -> &Rc<Disposable> {
        &self.base
    }
}

impl<TItem> IDisposable for VirtualizedItemBinding<TItem> {
    /// `dispose()`: dispara `onDidDispose` e só então descarta os filhos; uma segunda chamada não faz nada.
    fn dispose(&self) {
        if self.base.store().is_disposed() {
            return;
        }
        self.on_did_dispose.fire(());
        self.base.dispose();
    }
}

/// `IVirtualizedItemTemplate<TItem, TBinding>`.
pub trait IVirtualizedItemTemplate<TItem, TBinding>: IDisposable {
    fn current_binding(&self) -> IObservable<Option<Rc<TBinding>>>;
    fn bind(&self, item: TItem, context: IVirtualizedItemBindingContext) -> Rc<TBinding>;
}

/// `VirtualizedItemTemplate<TItem, TBinding>` (a classe abstrata): a ligação atual e a regra de posse do template.
pub struct VirtualizedItemTemplate<TItem: 'static, TBinding: 'static> {
    base: Rc<Disposable>,
    current_binding_value: Rc<ObservableValue<Option<Rc<TBinding>>, ()>>,
    binding_store: Rc<DisposableStore>,
    _item: PhantomData<fn(TItem)>,
}

impl<TItem: 'static, TBinding: IVirtualizedItemBinding<TItem> + 'static> VirtualizedItemTemplate<TItem, TBinding> {
    pub fn new() -> Self {
        let base = Disposable::new();
        let binding_store = DisposableStore::new();
        base.register(binding_store.clone());
        Self { base, current_binding_value: observable_value::<Option<Rc<TBinding>>, ()>("currentBinding", None), binding_store, _item: PhantomData }
    }

    /// `currentBinding`.
    pub fn current_binding(&self) -> IObservable<Option<Rc<TBinding>>> {
        self.current_binding_value.to_observable()
    }

    /// `_store`, para as subclasses.
    pub fn store(&self) -> &Rc<DisposableStore> {
        self.base.store()
    }

    /// `bind(item, context)`; `create_binding` é o `createBinding` abstrato da subclasse.
    pub fn bind(
        &self,
        item: TItem,
        context: IVirtualizedItemBindingContext,
        create_binding: impl FnOnce(TItem, IVirtualizedItemBindingContext) -> Rc<TBinding>,
    ) -> Rc<TBinding> {
        if self.current_binding_value.get().is_some() {
            throw_bug("Cannot rebind a virtualized template before disposing its current binding");
        }
        let binding = create_binding(item, context);
        self.binding_store.clear();
        let current_binding = self.current_binding_value.clone();
        let binding_store = Rc::downgrade(&self.binding_store);
        let bound = Rc::downgrade(&binding);
        self.binding_store.add(binding.on_did_dispose().once().listen(move |_| {
            if !current_binding.get().is_some_and(|current| std::ptr::eq(Rc::as_ptr(&current), bound.as_ptr())) {
                throw_bug("Disposed binding does not own its virtualized template");
            }
            if let Some(binding_store) = binding_store.upgrade() {
                binding_store.clear();
            }
            current_binding.set(None, None);
        }));
        self.current_binding_value.set(Some(binding.clone()), None);
        binding
    }
}

impl<TItem: 'static, TBinding: IVirtualizedItemBinding<TItem> + 'static> Default for VirtualizedItemTemplate<TItem, TBinding> {
    fn default() -> Self {
        Self::new()
    }
}

impl<TItem: 'static, TBinding: IVirtualizedItemBinding<TItem> + 'static> IDisposable for VirtualizedItemTemplate<TItem, TBinding> {
    fn dispose(&self) {
        if let Some(binding) = self.current_binding_value.get() {
            binding.dispose();
        }
        self.base.dispose();
    }
}

/// `IVirtualizedItemDelegate<TItem, TBinding, TTemplate>`.
pub struct IVirtualizedItemDelegate<TItem: 'static, TBinding: 'static, TTemplate: 'static> {
    pub get_id: Box<dyn Fn(&TItem) -> VirtualizedItemId>,
    pub get_template_id: Box<dyn Fn(&TItem) -> String>,
    pub get_unbound_size: Box<dyn Fn(&TItem) -> IObservable<i32>>,
    pub create_template: Box<dyn Fn(&str, &ICompressedVirtualizedScrollViewContext) -> Rc<TTemplate>>,
    /// `onDidBind?`.
    pub on_did_bind: Option<Box<dyn Fn(&Rc<TBinding>, &dyn ITransaction)>>,
    /// `onWillUnbind?`.
    pub on_will_unbind: Option<Box<dyn Fn(&Rc<TBinding>, &dyn ITransaction)>>,
}

impl<TItem: 'static, TBinding: 'static, TTemplate: 'static> IVirtualizedItemDelegate<TItem, TBinding, TTemplate> {
    /// O delegado com os quatro membros obrigatórios; `on_did_bind` e `on_will_unbind` (opcionais) começam vazios.
    pub fn new(
        get_id: impl Fn(&TItem) -> VirtualizedItemId + 'static,
        get_template_id: impl Fn(&TItem) -> String + 'static,
        get_unbound_size: impl Fn(&TItem) -> IObservable<i32> + 'static,
        create_template: impl Fn(&str, &ICompressedVirtualizedScrollViewContext) -> Rc<TTemplate> + 'static,
    ) -> Self {
        Self {
            get_id: Box::new(get_id),
            get_template_id: Box::new(get_template_id),
            get_unbound_size: Box::new(get_unbound_size),
            create_template: Box::new(create_template),
            on_did_bind: None,
            on_will_unbind: None,
        }
    }
}

/// `VirtualizedItemManager<TItem, TBinding, TTemplate>`.
pub struct VirtualizedItemManager<TItem: 'static, TBinding: 'static, TTemplate: 'static> {
    base: Rc<Disposable>,
    context: ICompressedVirtualizedScrollViewContext,
    delegate: Rc<IVirtualizedItemDelegate<TItem, TBinding, TTemplate>>,
    pools: Rc<RefCell<Vec<(String, Rc<VirtualizedTemplatePool<TItem, TBinding, TTemplate>>)>>>,
    pub virtualized_items: IObservable<Rc<Vec<Rc<ManagedVirtualizedItem<TItem, TBinding, TTemplate>>>>>,
}

impl<TItem, TBinding, TTemplate> VirtualizedItemManager<TItem, TBinding, TTemplate>
where
    TItem: Clone + StrictEquals + 'static,
    TBinding: IVirtualizedItemBinding<TItem> + 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    pub fn new(
        items: IObservable<Rc<Vec<TItem>>>,
        context: ICompressedVirtualizedScrollViewContext,
        delegate: IVirtualizedItemDelegate<TItem, TBinding, TTemplate>,
    ) -> Rc<Self> {
        let delegate = Rc::new(delegate);
        let this = Rc::new_cyclic(|manager: &Weak<Self>| {
            let (manager, item_delegate, id_delegate) = (manager.clone(), delegate.clone(), delegate.clone());
            let virtualized_items = map_observable_array_cached_by(
                None,
                items,
                move |item: &TItem, store| {
                    let managed = ManagedVirtualizedItem::new(item.clone(), manager.clone(), item_delegate.clone());
                    store.add_rc(managed.clone());
                    managed
                },
                move |item: &TItem| (id_delegate.get_id)(item),
            );
            Self { base: Disposable::new(), context, delegate, pools: Rc::default(), virtualized_items }
        });
        this.virtualized_items.recompute_initially_and_on_change(this.base.store(), None);
        let pools = this.pools.clone();
        this.base.register(to_disposable(move || {
            let all: Vec<_> = pools.borrow().iter().map(|(_, pool)| pool.clone()).collect();
            for pool in all {
                pool.dispose();
            }
            pools.borrow_mut().clear();
        }));
        this
    }

    /// `acquire(item)`: um template ocioso do pool do `templateId` do item, ou um novo.
    pub fn acquire(&self, item: &TItem) -> Reference<Rc<TTemplate>> {
        let template_id = (self.delegate.get_template_id)(item);
        let existing = self.pools.borrow().iter().find(|(id, _)| *id == template_id).map(|(_, pool)| pool.clone());
        let pool = existing.unwrap_or_else(|| {
            let (delegate, context, id) = (self.delegate.clone(), self.context.clone(), template_id.clone());
            let pool = Rc::new(VirtualizedTemplatePool::new(Box::new(move || (delegate.create_template)(&id, &context))));
            self.pools.borrow_mut().push((template_id.clone(), pool.clone()));
            pool
        });
        pool.acquire()
    }
}

impl<TItem, TBinding, TTemplate> IDisposable for VirtualizedItemManager<TItem, TBinding, TTemplate>
where
    TItem: Clone + StrictEquals + 'static,
    TBinding: IVirtualizedItemBinding<TItem> + 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `IReference<TTemplate>` guardada num observável (comparada por identidade).
type TemplateReference<TTemplate> = Rc<Reference<Rc<TTemplate>>>;

/// O último `render` do item, para o `rebind`.
#[derive(Clone)]
struct LastRender {
    rendered_range: OffsetRange,
    scroll_offset: i32,
    width: f64,
    rendered_viewport: OffsetRange,
    context: Option<Rc<dyn ICompressedVirtualizedScrollItemContext>>,
}

/// `ManagedVirtualizedItem<TItem, TBinding, TTemplate>`.
pub struct ManagedVirtualizedItem<TItem: 'static, TBinding: 'static, TTemplate: 'static> {
    base: Rc<Disposable>,
    pub item: TItem,
    manager: Weak<VirtualizedItemManager<TItem, TBinding, TTemplate>>,
    delegate: Rc<IVirtualizedItemDelegate<TItem, TBinding, TTemplate>>,
    template_reference: Rc<ObservableValue<Option<TemplateReference<TTemplate>>, ()>>,
    is_hidden: Rc<ObservableValue<bool, ()>>,
    last_render: RefCell<Option<LastRender>>,
    did_render_fail: Cell<bool>,
    pub template: IObservable<Option<Rc<TTemplate>>>,
    pub binding: IObservable<Option<Rc<TBinding>>>,
    pub size: IObservable<i32>,
    pub max_scroll: IObservable<MaxScroll>,
}

impl<TItem, TBinding, TTemplate> ManagedVirtualizedItem<TItem, TBinding, TTemplate>
where
    TItem: Clone + StrictEquals + 'static,
    TBinding: IVirtualizedItemBinding<TItem> + 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    pub fn new(
        item: TItem,
        manager: Weak<VirtualizedItemManager<TItem, TBinding, TTemplate>>,
        delegate: Rc<IVirtualizedItemDelegate<TItem, TBinding, TTemplate>>,
    ) -> Rc<Self> {
        let template_reference = observable_value::<Option<TemplateReference<TTemplate>>, ()>("templateReference", None);
        let is_hidden = observable_value::<bool, ()>("isHidden", false);
        let template: IObservable<Option<Rc<TTemplate>>> = {
            let template_reference = template_reference.to_observable();
            derived(move |reader| template_reference.read(reader).map(|reference| reference.object.clone()))
        };
        let binding: IObservable<Option<Rc<TBinding>>> = {
            let template = template.clone();
            derived(move |reader| template.read(reader).and_then(|template| template.current_binding().read(reader)))
        };
        let unbound_size = (delegate.get_unbound_size)(&item);
        let size: IObservable<i32> = {
            let binding = binding.clone();
            derived(move |reader| match binding.read(reader) {
                Some(binding) => binding.size().read(reader),
                None => unbound_size.read(reader),
            })
        };
        let max_scroll: IObservable<MaxScroll> = {
            let binding = binding.clone();
            derived(move |reader| match binding.read(reader) {
                Some(binding) => binding.max_scroll().read(reader),
                None => MaxScroll { max_scroll: 0.0 },
            })
        };
        let this = Rc::new(Self {
            base: Disposable::new(),
            item,
            manager,
            delegate,
            template_reference,
            is_hidden,
            last_render: RefCell::new(None),
            did_render_fail: Cell::new(false),
            template,
            binding,
            size,
            max_scroll,
        });
        let weak = Rc::downgrade(&this);
        let (binding, is_hidden) = (this.binding.clone(), this.is_hidden.to_observable());
        this.base.register(autorun(move |reader| {
            let Some(binding) = binding.read(reader) else { return };
            if !is_hidden.read(reader) || binding.should_keep_alive().read(reader) {
                return;
            }
            if let Some(this) = weak.upgrade() {
                this.clear_binding();
            }
        }));
        this
    }

    /// `render(renderedRange, scrollOffset, width, renderedViewport, context?)`. Um erro na ligação ou no `render` da
    /// ligação é reportado (`onUnexpectedError`) e o item deixa de renderizar até o `rebind`.
    pub fn render(
        &self,
        rendered_range: OffsetRange,
        scroll_offset: i32,
        width: f64,
        rendered_viewport: OffsetRange,
        context: Option<Rc<dyn ICompressedVirtualizedScrollItemContext>>,
    ) {
        *self.last_render.borrow_mut() = Some(LastRender { rendered_range, scroll_offset, width, rendered_viewport, context: context.clone() });
        self.is_hidden.set(false, None);
        if self.did_render_fail.get() {
            return;
        }
        if let Err(error) = errors::catch(|| self.render_bound(rendered_range, scroll_offset, width, rendered_viewport, context)) {
            self.did_render_fail.set(true);
            errors::on_unexpected_error(&error);
        }
    }

    /// `_render`.
    fn render_bound(
        &self,
        rendered_range: OffsetRange,
        scroll_offset: i32,
        width: f64,
        rendered_viewport: OffsetRange,
        context: Option<Rc<dyn ICompressedVirtualizedScrollItemContext>>,
    ) {
        let binding = match self.binding.get() {
            Some(binding) => binding,
            None => self.bind_new(context),
        };
        binding.render(rendered_range, scroll_offset, width, rendered_viewport);
    }

    /// O ramo `if (!binding)` do `_render`: adquire um template e liga o item a ele.
    fn bind_new(&self, context: Option<Rc<dyn ICompressedVirtualizedScrollItemContext>>) -> Rc<TBinding> {
        let manager = self.manager.upgrade().unwrap_or_else(|| throw_bug("The virtualized item manager was disposed"));
        let template_reference: TemplateReference<TTemplate> = Rc::new(manager.acquire(&self.item));
        let template = template_reference.object.clone();
        if template.current_binding().get().is_some() {
            template_reference.dispose();
            throw_bug("Virtualized template pool returned a bound template");
        }
        let binding_context = IVirtualizedItemBindingContext::new(self.size.get(), move |get_item_offset, update| match &context {
            Some(context) => context.run_with_scroll_anchor(get_item_offset, update),
            None => throw_bug("Cannot preserve a virtualized item scroll anchor without a render context"),
        });
        let mut new_binding: Option<Rc<TBinding>> = None;
        let attempt = errors::catch(|| {
            let bound = template.bind(self.item.clone(), binding_context);
            new_binding = Some(bound.clone());
            if !bound.item().strict_equals(&self.item) || !template.current_binding().get().is_some_and(|current| Rc::ptr_eq(&current, &bound)) {
                throw_bug("Virtualized template returned a binding for a different item");
            }
            transaction(|tx| {
                if let Some(on_did_bind) = &self.delegate.on_did_bind {
                    on_did_bind(&bound, tx);
                }
                self.template_reference.set(Some(template_reference.clone()), Some(tx));
            });
            bound
        });
        match attempt {
            Ok(bound) => bound,
            Err(error) => {
                if let Some(stale) = template.current_binding().get().or(new_binding) {
                    stale.dispose();
                }
                template_reference.dispose();
                errors::throw(error)
            }
        }
    }

    /// `rebind()`: desliga a ligação atual e renderiza de novo com os últimos argumentos.
    pub fn rebind(&self) {
        if self.binding.get().is_none() {
            return;
        }
        let Some(last_render) = self.last_render.borrow().clone() else { return };
        self.clear_binding();
        self.did_render_fail.set(false);
        self.render(last_render.rendered_range, last_render.scroll_offset, last_render.width, last_render.rendered_viewport, last_render.context);
    }

    /// `_clearBinding()`.
    fn clear_binding(&self) {
        let template_reference = self.template_reference.get();
        let binding = template_reference.as_ref().and_then(|reference| reference.object.current_binding().get());
        let (Some(template_reference), Some(binding)) = (template_reference, binding) else { return };
        transaction(|tx| {
            if let Some(on_will_unbind) = &self.delegate.on_will_unbind {
                on_will_unbind(&binding, tx);
            }
        });
        binding.hide();
        binding.dispose();
        if template_reference.object.current_binding().get().is_some() {
            throw_bug("Virtualized binding did not release its template when disposed");
        }
        transaction(|tx| self.template_reference.set(None, Some(tx)));
        template_reference.dispose();
    }
}

impl<TItem, TBinding, TTemplate> ICompressedVirtualizedScrollItem for ManagedVirtualizedItem<TItem, TBinding, TTemplate>
where
    TItem: Clone + StrictEquals + 'static,
    TBinding: IVirtualizedItemBinding<TItem> + 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    fn size(&self) -> IObservable<i32> {
        self.size.clone()
    }

    fn max_scroll(&self) -> IObservable<MaxScroll> {
        self.max_scroll.clone()
    }

    fn render(&self, rendered_range: OffsetRange, scroll_offset: i32, width: f64, rendered_viewport: OffsetRange, context: Rc<dyn ICompressedVirtualizedScrollItemContext>) {
        ManagedVirtualizedItem::render(self, rendered_range, scroll_offset, width, rendered_viewport, Some(context));
    }

    /// `hide()`.
    fn hide(&self) {
        self.is_hidden.set(true, None);
        if let Some(binding) = self.binding.get() {
            binding.hide();
        }
    }
}

impl<TItem, TBinding, TTemplate> IDisposable for ManagedVirtualizedItem<TItem, TBinding, TTemplate>
where
    TItem: Clone + StrictEquals + 'static,
    TBinding: IVirtualizedItemBinding<TItem> + 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    fn dispose(&self) {
        self.clear_binding();
        self.base.dispose();
    }
}

/// `VirtualizedTemplatePool<TItem, TBinding, TTemplate>`.
struct VirtualizedTemplatePool<TItem: 'static, TBinding: 'static, TTemplate: 'static> {
    create: Box<dyn Fn() -> Rc<TTemplate>>,
    unused: Rc<RefCell<Vec<Rc<TTemplate>>>>,
    used: Rc<RefCell<Vec<Rc<TTemplate>>>>,
    _types: PhantomData<fn() -> (TItem, TBinding)>,
}

impl<TItem, TBinding, TTemplate> VirtualizedTemplatePool<TItem, TBinding, TTemplate>
where
    TItem: 'static,
    TBinding: 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    fn new(create: Box<dyn Fn() -> Rc<TTemplate>>) -> Self {
        Self { create, unused: Rc::default(), used: Rc::default(), _types: PhantomData }
    }

    /// `acquire()`: o primeiro template ocioso, ou um novo; o `dispose` da referência o devolve ao pool.
    fn acquire(&self) -> Reference<Rc<TTemplate>> {
        let idle = {
            let mut unused = self.unused.borrow_mut();
            if unused.is_empty() { None } else { Some(unused.remove(0)) }
        };
        let template = idle.unwrap_or_else(|| (self.create)());
        if template.current_binding().get().is_some() {
            throw_bug("Cannot acquire a bound virtualized template");
        }
        self.used.borrow_mut().push(template.clone());
        let (used, unused, released) = (self.used.clone(), self.unused.clone(), template.clone());
        Reference::new(
            template,
            plain_disposable(move || {
                if released.current_binding().get().is_some() {
                    throw_bug("Cannot pool a virtualized template with a current binding");
                }
                used.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &released));
                if unused.borrow().len() >= MAX_UNUSED_TEMPLATES {
                    released.dispose();
                } else {
                    unused.borrow_mut().push(released);
                }
            }),
        )
    }
}

impl<TItem, TBinding, TTemplate> IDisposable for VirtualizedTemplatePool<TItem, TBinding, TTemplate>
where
    TItem: 'static,
    TBinding: 'static,
    TTemplate: IVirtualizedItemTemplate<TItem, TBinding> + 'static,
{
    fn dispose(&self) {
        let used = self.used.borrow().clone();
        for template in used {
            template.dispose();
        }
        let unused = self.unused.borrow().clone();
        for template in unused {
            template.dispose();
        }
        self.used.borrow_mut().clear();
        self.unused.borrow_mut().clear();
    }
}
