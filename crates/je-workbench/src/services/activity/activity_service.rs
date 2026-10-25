//! Porte de `src/vs/workbench/services/activity/browser/activityService.ts`: o [`ActivityService`], que guarda as
//! atividades (selos) dos contêineres de views e das ações globais, e o `ViewContainerActivityByView`, que leva o selo de
//! uma view para o contêiner onde ela está.
//!
//! Divergências:
//! - Os `@IViewDescriptorService`, `@IActivityService` e `@IInstantiationService` do construtor são as dependências do
//!   [`Injectable`], e o `registerSingleton(..., Delayed)` do fim do módulo é [`register_activity_service`], chamada por
//!   quem monta o workbench (Rust não tem efeito colateral na carga do módulo).
//! - `Disposable.None` é [`lifecycle::none`]; a herança de `Disposable` vira o campo `base`.
//! - `activities.indexOf(activity)` compara a identidade do `Rc` do selo (ver [`crate::services::activity::activity`]).
//! - O objeto `{ id, activity }` de `viewActivities` é o `ViewActivity`, com o `id` num `Cell`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, to_disposable, Disposable, DisposableRef, IDisposable};
use je_instantiation::{register_singleton, service_impl, Injectable, InstantiationService, InstantiationType};

use crate::common::activity::{ACCOUNTS_ACTIVITY_ID, GLOBAL_ACTIVITY_ID};
use crate::common::views::IViewDescriptorService;
use crate::services::activity::activity::{ActivityChangeTarget, IActivity, IActivityService, I_ACTIVITY_SERVICE};

/// A identidade de uma atividade: a do `Rc` do selo (o `indexOf` do original compara o objeto).
fn same_activity(a: &IActivity, b: &IActivity) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(&a.badge), Rc::as_ptr(&b.badge))
}

/// `ViewContainerActivityByView`: mostra o selo da view no contêiner onde ela está, e o move quando o contêiner ou a
/// localização da view mudam.
struct ViewContainerActivityByView {
    base: Rc<Disposable>,
    view_id: String,
    view_descriptor_service: Rc<dyn IViewDescriptorService>,
    activity_service: Rc<dyn IActivityService>,
    activity: RefCell<Option<IActivity>>,
    activity_disposable: RefCell<DisposableRef>,
}

impl ViewContainerActivityByView {
    /// `setActivity(activity)`.
    fn set_activity(&self, activity: IActivity) {
        *self.activity.borrow_mut() = Some(activity);
        self.update();
    }

    /// `clearActivity()`. Nenhum chamador do original o usa, mas ele faz parte da classe.
    #[allow(dead_code)]
    fn clear_activity(&self) {
        *self.activity.borrow_mut() = None;
        self.update();
    }

    /// O `() => this.update()` que o construtor liga aos eventos de contêiner e de localização.
    fn update_on_change<T: 'static>(weak: Weak<Self>) -> impl Fn(&T) {
        move |_| {
            if let Some(this) = weak.upgrade() {
                this.update();
            }
        }
    }

    /// `update()`.
    fn update(&self) {
        let previous = self.activity_disposable.replace(lifecycle::none());
        previous.dispose();
        let container = self.view_descriptor_service.get_view_container_by_view_id(&self.view_id);
        let activity = self.activity.borrow().clone();
        if let (Some(container), Some(activity)) = (container, activity) {
            let shown = self.activity_service.show_view_container_activity(&container.id, activity);
            *self.activity_disposable.borrow_mut() = shown;
        }
    }
}

impl Injectable for ViewContainerActivityByView {
    /// O `viewId`.
    type Args = String;
    type Deps = (Rc<dyn IViewDescriptorService>, Rc<dyn IActivityService>);

    fn create(view_id: Self::Args, (view_descriptor_service, activity_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = Disposable::new();
            let id = view_id.clone();
            base.register(view_descriptor_service.on_did_change_container().filter(move |e| e.views.iter().any(|view| view.id == id), None).listen(Self::update_on_change(weak.clone())));
            let id = view_id.clone();
            base.register(view_descriptor_service.on_did_change_location().filter(move |e| e.views.iter().any(|view| view.id == id), None).listen(Self::update_on_change(weak.clone())));
            Self { base, view_id, view_descriptor_service, activity_service, activity: RefCell::new(None), activity_disposable: RefCell::new(lifecycle::none()) }
        })
    }
}

impl IDisposable for ViewContainerActivityByView {
    fn dispose(&self) {
        let previous = self.activity_disposable.replace(lifecycle::none());
        previous.dispose();
        self.base.dispose();
    }
}

/// `IViewActivity`: o número da última chamada de `showViewActivity` da view e o seu `ViewContainerActivityByView`.
struct ViewActivity {
    id: Cell<u32>,
    activity: Rc<ViewContainerActivityByView>,
}

/// `ActivityService`.
pub struct ActivityService {
    base: Rc<Disposable>,
    view_descriptor_service: Rc<dyn IViewDescriptorService>,
    instantiation_service: Rc<InstantiationService>,
    view_activities: RefCell<HashMap<String, Rc<ViewActivity>>>,
    on_did_change_activity: Rc<Emitter<ActivityChangeTarget>>,
    view_container_activities: RefCell<HashMap<String, Vec<IActivity>>>,
    global_activities: RefCell<HashMap<String, Vec<IActivity>>>,
    this: Weak<ActivityService>,
}

impl ActivityService {
    fn this(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    /// `showActivity(id, activity)`: as atividades das ações globais.
    fn show_activity(&self, id: &str, activity: IActivity) -> DisposableRef {
        self.global_activities.borrow_mut().entry(id.to_owned()).or_default().push(activity.clone());
        self.on_did_change_activity.fire(ActivityChangeTarget::Id(id.to_owned()));
        let (this, id) = (self.this(), id.to_owned());
        to_disposable(move || {
            {
                let mut global_activities = this.global_activities.borrow_mut();
                if let Some(activities) = global_activities.get_mut(&id) {
                    if let Some(index) = activities.iter().position(|candidate| same_activity(candidate, &activity)) {
                        activities.remove(index);
                    }
                    if activities.is_empty() {
                        global_activities.remove(&id);
                    }
                }
            }
            this.on_did_change_activity.fire(ActivityChangeTarget::Id(id));
        })
    }
}

impl Injectable for ActivityService {
    type Args = ();
    type Deps = (Rc<dyn IViewDescriptorService>, Rc<InstantiationService>);

    fn create((): (), (view_descriptor_service, instantiation_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = Disposable::new();
            let on_did_change_activity = Emitter::<ActivityChangeTarget>::new();
            base.register(on_did_change_activity.clone());
            Self {
                base,
                view_descriptor_service,
                instantiation_service,
                view_activities: RefCell::new(HashMap::new()),
                on_did_change_activity,
                view_container_activities: RefCell::new(HashMap::new()),
                global_activities: RefCell::new(HashMap::new()),
                this: weak.clone(),
            }
        })
    }
}

service_impl!(ActivityService => dyn IActivityService, disposable);

impl IDisposable for ActivityService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IActivityService for ActivityService {
    fn on_did_change_activity(&self) -> Event<ActivityChangeTarget> {
        self.on_did_change_activity.event()
    }

    fn show_view_container_activity(&self, view_container_id: &str, activity: IActivity) -> DisposableRef {
        let Some(view_container) = self.view_descriptor_service.get_view_container_by_id(view_container_id) else {
            return lifecycle::none();
        };

        // add activity
        self.view_container_activities.borrow_mut().entry(view_container_id.to_owned()).or_default().push(activity.clone());

        self.on_did_change_activity.fire(ActivityChangeTarget::ViewContainer(view_container.clone()));

        let (this, view_container_id) = (self.this(), view_container_id.to_owned());
        to_disposable(move || {
            {
                let mut view_container_activities = this.view_container_activities.borrow_mut();
                if let Some(activities) = view_container_activities.get_mut(&view_container_id) {
                    if let Some(index) = activities.iter().position(|candidate| same_activity(candidate, &activity)) {
                        activities.remove(index);
                    }
                    if activities.is_empty() {
                        view_container_activities.remove(&view_container_id);
                    }
                }
            }
            this.on_did_change_activity.fire(ActivityChangeTarget::ViewContainer(view_container));
        })
    }

    fn get_view_container_activities(&self, view_container_id: &str) -> Vec<IActivity> {
        if self.view_descriptor_service.get_view_container_by_id(view_container_id).is_some() {
            return self.view_container_activities.borrow().get(view_container_id).cloned().unwrap_or_default();
        }
        Vec::new()
    }

    fn show_view_activity(&self, view_id: &str, activity: IActivity) -> DisposableRef {
        let existing = self.view_activities.borrow().get(view_id).cloned();
        let item = match existing {
            Some(item) => {
                item.id.set(item.id.get() + 1);
                item
            }
            None => {
                let item = Rc::new(ViewActivity { id: Cell::new(1), activity: self.instantiation_service.create_instance::<ViewContainerActivityByView>(view_id.to_owned()) });
                self.view_activities.borrow_mut().insert(view_id.to_owned(), item.clone());
                item
            }
        };

        let id = item.id.get();
        item.activity.set_activity(activity);

        let (this, view_id) = (self.this(), view_id.to_owned());
        to_disposable(move || {
            if item.id.get() == id {
                item.activity.dispose();
                this.view_activities.borrow_mut().remove(&view_id);
            }
        })
    }

    fn show_accounts_activity(&self, activity: IActivity) -> DisposableRef {
        self.show_activity(ACCOUNTS_ACTIVITY_ID, activity)
    }

    fn show_global_activity(&self, activity: IActivity) -> DisposableRef {
        self.show_activity(GLOBAL_ACTIVITY_ID, activity)
    }

    fn get_activity(&self, id: &str) -> Vec<IActivity> {
        self.global_activities.borrow().get(id).cloned().unwrap_or_default()
    }
}

/// `registerSingleton(IActivityService, ActivityService, InstantiationType.Delayed)`.
pub fn register_activity_service() {
    register_singleton::<ActivityService>(I_ACTIVITY_SERVICE, InstantiationType::Delayed);
}
