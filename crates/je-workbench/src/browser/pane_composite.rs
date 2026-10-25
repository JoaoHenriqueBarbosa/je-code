//! Porte de `src/vs/workbench/browser/panecomposite.ts` (e do `CompositeDescriptor`/`CompositeRegistry` de `composite.ts`
//! que ele especializa): o [`PaneCompositeDescriptor`], o [`PaneCompositeRegistry`] e os três registros globais
//! ([`extensions`]).
//!
//! Divergências:
//! - **Construtor injetado.** `IConstructorSignature<PaneComposite>` e `instantiationService.createInstance(ctor)` são a
//!   [`PaneCompositeFactory`], que recebe o `InstantiationService` e devolve o [`IPaneComposite`].
//! - **`Registry.add(Extensions.X, new PaneCompositeRegistry())`** são os registros por identificador de
//!   [`pane_composite_registry`], criados no primeiro uso na thread (o carregamento do módulo faz o mesmo no original).
//! - **`CompositeDescriptor<PaneComposite>`** é um tipo só: a base genérica vira os campos do próprio descritor e a
//!   `CompositeRegistry` vira o armazenamento do [`PaneCompositeRegistry`]; `registerComposite` protegido é o interno.
//! - **Fica de fora** a classe abstrata `PaneComposite` (depende de `Composite`, `ViewPaneContainer` e do DOM da parte),
//!   que entra com o porte das partes de painéis.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::uri::URI;
use je_instantiation::InstantiationService;

use crate::services::panecomposite::panecomposite::IPaneComposite;

/// `IConstructorSignature<PaneComposite>`: `createInstance(ctor)`.
pub type PaneCompositeFactory = Rc<dyn Fn(&Rc<InstantiationService>) -> Rc<dyn IPaneComposite>>;

/// `Extensions`: os identificadores dos registros de painéis.
pub mod extensions {
    pub const VIEWLETS: &str = "workbench.contributions.viewlets";
    pub const PANELS: &str = "workbench.contributions.panels";
    pub const AUXILIARY: &str = "workbench.contributions.auxiliary";
}

/// `PaneCompositeDescriptor`: o descritor leve de um painel do workbench.
#[derive(Clone)]
pub struct PaneCompositeDescriptor {
    ctor: PaneCompositeFactory,
    pub id: String,
    pub name: String,
    pub css_class: Option<String>,
    pub order: Option<f64>,
    pub requested_index: Option<f64>,
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
        Self { ctor, id: id.to_owned(), name: name.to_owned(), css_class: css_class.map(str::to_owned), order, requested_index, icon_url }
    }

    /// `instantiate(instantiationService)`.
    pub fn instantiate(&self, instantiation_service: &Rc<InstantiationService>) -> Rc<dyn IPaneComposite> {
        (self.ctor)(instantiation_service)
    }
}

/// `PaneCompositeRegistry` (sobre `CompositeRegistry<PaneComposite>`).
pub struct PaneCompositeRegistry {
    base: Rc<Disposable>,
    on_did_register: Rc<Emitter<PaneCompositeDescriptor>>,
    on_did_deregister: Rc<Emitter<PaneCompositeDescriptor>>,
    composites: RefCell<Vec<PaneCompositeDescriptor>>,
}

impl PaneCompositeRegistry {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register = Emitter::new();
        base.register(on_did_register.clone());
        let on_did_deregister = Emitter::new();
        base.register(on_did_deregister.clone());
        Rc::new(Self { base, on_did_register, on_did_deregister, composites: RefCell::new(Vec::new()) })
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
        if self.composite_by_id(&descriptor.id).is_some() {
            return;
        }
        self.composites.borrow_mut().push(descriptor.clone());
        self.on_did_register.fire(descriptor);
    }

    /// `deregisterPaneComposite(id)`.
    pub fn deregister_pane_composite(&self, id: &str) {
        let removed = {
            let mut composites = self.composites.borrow_mut();
            composites.iter().position(|composite| composite.id == id).map(|index| composites.remove(index))
        };
        if let Some(descriptor) = removed {
            self.on_did_deregister.fire(descriptor);
        }
    }

    /// `getPaneComposite(id)`: o descritor do identificador, ou `None`.
    pub fn get_pane_composite(&self, id: &str) -> Option<PaneCompositeDescriptor> {
        self.composite_by_id(id)
    }

    /// `getPaneComposites()`: os painéis registrados, na ordem de registro.
    pub fn get_pane_composites(&self) -> Vec<PaneCompositeDescriptor> {
        self.composites.borrow().clone()
    }

    fn composite_by_id(&self, id: &str) -> Option<PaneCompositeDescriptor> {
        self.composites.borrow().iter().find(|composite| composite.id == id).cloned()
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
