//! Porte de `src/vs/workbench/browser/panecomposite.ts`: o [`PaneCompositeDescriptor`], o [`PaneCompositeRegistry`] e os três
//! registros globais ([`extensions`]), sobre o [`CompositeDescriptor`] e o [`CompositeRegistry`] genéricos de `composite.ts`.
//!
//! Divergências:
//! - **Construtor injetado.** `IConstructorSignature<PaneComposite>` e `instantiationService.createInstance(ctor)` são a
//!   [`PaneCompositeFactory`], que recebe o `InstantiationService` e devolve o [`IPaneComposite`].
//! - **`Registry.add(Extensions.X, new PaneCompositeRegistry())`** são os registros por identificador de
//!   [`pane_composite_registry`], criados no primeiro uso na thread (o carregamento do módulo faz o mesmo no original).
//! - **Herança vira composição.** `PaneCompositeDescriptor extends CompositeDescriptor<PaneComposite>` é o descritor base
//!   (`CompositeDescriptor<dyn IPaneComposite>`) mais o `iconUrl`; os campos e métodos do descritor base (`id`, `name`, `css_class`,
//!   `order`, `requested_index`, `instantiate`) valem pelo `Deref`. `PaneCompositeRegistry extends CompositeRegistry<PaneComposite>`
//!   é o registro genérico (que o `CompositePart` consulta, [`PaneCompositeRegistry::composite_registry`]) mais o `iconUrl` de cada
//!   identificador, que o descritor genérico não carrega; `registerComposite` (`protected`) e `getComposite(s)` são os do registro
//!   de dentro.
//! - **`PaneComposite`.** A `PaneComposite` abstrata (um `Composite` que hospeda um `ViewPaneContainer`) é o [`IPaneComposite`] de
//!   `services/panecomposite`, cujo `composite()` devolve o [`Composite`] que o `CompositePart` guarda; um painel sem composite
//!   não pode ser instanciado pela parte.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::uri::URI;
use je_instantiation::InstantiationService;

use crate::browser::composite::{Composite, CompositeDescriptor, CompositeInstance, CompositeRegistry};
use crate::browser::part::assert_returns_defined;
use crate::services::panecomposite::panecomposite::IPaneComposite;

/// `IConstructorSignature<PaneComposite>`: `createInstance(ctor)`.
pub type PaneCompositeFactory = Rc<dyn Fn(&Rc<InstantiationService>) -> Rc<dyn IPaneComposite>>;

impl CompositeInstance for dyn IPaneComposite {
    fn composite(&self) -> Rc<Composite> {
        assert_returns_defined(IPaneComposite::composite(self))
    }
}

/// `Extensions`: os identificadores dos registros de painéis.
pub mod extensions {
    pub const VIEWLETS: &str = "workbench.contributions.viewlets";
    pub const PANELS: &str = "workbench.contributions.panels";
    pub const AUXILIARY: &str = "workbench.contributions.auxiliary";
}

/// `PaneCompositeDescriptor`: o descritor leve de um painel do workbench.
#[derive(Clone)]
pub struct PaneCompositeDescriptor {
    base: CompositeDescriptor<dyn IPaneComposite>,
    pub icon_url: Option<URI>,
}

impl PaneCompositeDescriptor {
    /// `PaneCompositeDescriptor.create(ctor, id, name, cssClass?, order?, requestedIndex?, iconUrl?)`.
    pub fn create(
        ctor: PaneCompositeFactory,
        id: &str,
        name: &str,
        css_class: Option<&str>,
        order: Option<f64>,
        requested_index: Option<f64>,
        icon_url: Option<URI>,
    ) -> Self {
        Self { base: CompositeDescriptor::new(ctor, id, name, css_class, order, requested_index), icon_url }
    }

    /// A identidade do descritor (o `===` do original): o descritor registrado é o mesmo quando o clone devolvido pelo registro
    /// compartilha a fábrica.
    pub fn is_same(&self, other: &Self) -> bool {
        self.base.is_same(&other.base)
    }
}

impl Deref for PaneCompositeDescriptor {
    type Target = CompositeDescriptor<dyn IPaneComposite>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

/// `PaneCompositeRegistry` (sobre `CompositeRegistry<PaneComposite>`).
pub struct PaneCompositeRegistry {
    base: Rc<Disposable>,
    registry: Rc<CompositeRegistry<dyn IPaneComposite>>,
    icon_urls: Rc<RefCell<HashMap<String, Option<URI>>>>,
    on_did_register: Rc<Emitter<PaneCompositeDescriptor>>,
    on_did_deregister: Rc<Emitter<PaneCompositeDescriptor>>,
}

impl PaneCompositeRegistry {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let registry: Rc<CompositeRegistry<dyn IPaneComposite>> = CompositeRegistry::new();
        base.register(registry.clone());
        let icon_urls: Rc<RefCell<HashMap<String, Option<URI>>>> = Rc::new(RefCell::new(HashMap::new()));
        let on_did_register: Rc<Emitter<PaneCompositeDescriptor>> = Emitter::new();
        base.register(on_did_register.clone());
        let on_did_deregister: Rc<Emitter<PaneCompositeDescriptor>> = Emitter::new();
        base.register(on_did_deregister.clone());

        // Os eventos do registro de dentro viram os do painel, com o `iconUrl` do identificador.
        let (icons, emitter) = (icon_urls.clone(), on_did_register.clone());
        base.register(registry.on_did_register().listen(move |descriptor| {
            let icon_url = icons.borrow().get(&descriptor.id).cloned().flatten();
            emitter.fire(PaneCompositeDescriptor { base: descriptor.clone(), icon_url });
        }));
        let (icons, emitter) = (icon_urls.clone(), on_did_deregister.clone());
        base.register(registry.on_did_deregister().listen(move |descriptor| {
            let icon_url = icons.borrow().get(&descriptor.id).cloned().flatten();
            emitter.fire(PaneCompositeDescriptor { base: descriptor.clone(), icon_url });
        }));

        Rc::new(Self { base, registry, icon_urls, on_did_register, on_did_deregister })
    }

    /// O `CompositeRegistry<PaneComposite>` que a `CompositePart` consulta.
    pub fn composite_registry(&self) -> Rc<CompositeRegistry<dyn IPaneComposite>> {
        self.registry.clone()
    }

    /// `onDidRegister`.
    pub fn on_did_register(&self) -> Event<PaneCompositeDescriptor> {
        self.on_did_register.event()
    }

    /// `onDidDeregister`.
    pub fn on_did_deregister(&self) -> Event<PaneCompositeDescriptor> {
        self.on_did_deregister.event()
    }

    /// `registerPaneComposite(descriptor)`: registra o painel na plataforma; um identificador repetido é ignorado.
    pub fn register_pane_composite(&self, descriptor: PaneCompositeDescriptor) {
        if self.registry.get_composite(&descriptor.id).is_some() {
            return;
        }

        self.icon_urls.borrow_mut().insert(descriptor.id.clone(), descriptor.icon_url.clone());
        self.registry.register_composite(descriptor.base);
    }

    /// `deregisterPaneComposite(id)`.
    pub fn deregister_pane_composite(&self, id: &str) {
        self.registry.deregister_composite(id);
        self.icon_urls.borrow_mut().remove(id);
    }

    /// `getPaneComposite(id)`: o descritor do identificador, ou `None`.
    pub fn get_pane_composite(&self, id: &str) -> Option<PaneCompositeDescriptor> {
        self.registry.get_composite(id).map(|base| self.with_icon_url(base))
    }

    /// `getPaneComposites()`: os painéis registrados, na ordem de registro.
    pub fn get_pane_composites(&self) -> Vec<PaneCompositeDescriptor> {
        self.registry.get_composites().into_iter().map(|base| self.with_icon_url(base)).collect()
    }

    fn with_icon_url(&self, base: CompositeDescriptor<dyn IPaneComposite>) -> PaneCompositeDescriptor {
        let icon_url = self.icon_urls.borrow().get(&base.id).cloned().flatten();
        PaneCompositeDescriptor { base, icon_url }
    }
}

impl IDisposable for PaneCompositeRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

thread_local! {
    static REGISTRIES: RefCell<HashMap<String, Rc<PaneCompositeRegistry>>> = RefCell::new(HashMap::new());
}

/// `Registry.as<PaneCompositeRegistry>(id)`: o registro de [`extensions`] (ou de um identificador que o `viewContainerLocation`
/// escolha), criado no primeiro uso.
pub fn pane_composite_registry(id: &str) -> Rc<PaneCompositeRegistry> {
    REGISTRIES.with(|registries| registries.borrow_mut().entry(id.to_owned()).or_insert_with(PaneCompositeRegistry::new).clone())
}
