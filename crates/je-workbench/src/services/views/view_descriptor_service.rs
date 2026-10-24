//! Porte de `src/vs/workbench/services/views/browser/viewDescriptorService.ts`: o `ViewDescriptorService`, que sabe em que
//! contêiner e em que localização (barra lateral, painel, barra auxiliar) cada view e cada contêiner de views está,
//! persiste as customizações do usuário (`views.customizations`) e mantém os modelos dos contêineres.
//!
//! Divergências:
//! - **Injeção.** O construtor é um [`Injectable`] com os sete serviços do original em `Deps`. `registerSingleton(...,
//!   InstantiationType.Delayed)` é [`register`], e o proxy da instanciação atrasada está em `common/views.rs`.
//! - **Ações de visibilidade e de reset.** `registerViewsVisibilityActions`, `registerViewsVisibilityActionsForContainer`
//!   e `registerResetViewContainerAction` (`registerAction2` com `ViewPaneContainerAction`, `ViewsSubMenu`, `MenuId` e
//!   `IViewsService`) pertencem à camada de interface (`viewPaneContainer.ts`, `viewsService.ts`), que ainda não foi
//!   portada, e ficam de fora com `canRegisterViewsVisibilityActions` e o `viewsVisibilityActionDisposables`. Nenhum
//!   estado do serviço depende delas.
//! - **Contêiner gerado.** `registerGeneratedViewContainer` passa `new SyncDescriptor(ViewPaneContainer, [id, {
//!   mergeViewWithContainerWhenSingleView: true }])` como `ctorDescriptor`; o `ViewPaneContainer` ainda não existe, então
//!   o `ctor_descriptor` do contêiner gerado é `None`.
//! - **Localização.** `getViewContainerLocation` e `getDefaultViewContainerLocation` devolvem `Option` (ver `views.rs`).
//!   Quando o contêiner não tem localização (não está no registro), `moveViewContainerToLocationWithoutSaving` e
//!   `moveViewsWithoutSaving` não disparam os eventos de localização (o original dispararia com `undefined`).
//! - **Estado em `Rc<RefCell<...>>`.** Os mapas do original (`Map` com ordem de inserção) são [`OrderedMap`]; as chaves
//!   por objeto (`viewContainerModels`) são [`ById`]. As chaves de contexto do mesmo tipo (`.active`, `.visible`,
//!   `.canMove`, `.defaultViewLocation`, `.defaultViewContainerLocation`) compartilham [`get_or_create_context_key`].
//! - **JSON** das customizações: `serde_json` com `preserve_order`; um JSON inválido lança como o `JSON.parse`.
//! - **Telemetria.** `publicLog2('viewDescriptorService.moveViews', ...)` leva o objeto como [`Dynamic`]; a classificação
//!   (`ViewDescriptorServiceMoveViewsClassification`) é só tipo no original e não tem equivalente.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{ById, Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::nls::{LocalizedString, localize};
use je_base::uuid::generate_uuid;
use je_instantiation::{Injectable, InstantiationService, InstantiationType, register_singleton, service_impl};
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::log::{LoggerRef, LoggerService};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_storage::StorageValue;
use serde_json::{Map, Value};

use crate::common::views::{
    IViewContainerDescriptor, IViewContainerModel, IViewDescriptorService, RegisterViewContainerOptions, ViewContainer, ViewContainerAndLocation,
    ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc, ViewContainersChange, ViewContainersRegistry, ViewDescriptorRc, ViewIcon,
    ViewVisibilityState, ViewsContainerChange, ViewsLocationChange, ViewsRegistration, ViewsRegistry, WindowEnablement, default_view_icon,
    view_container_location_to_string, view_containers_registry, views_registry, I_VIEW_DESCRIPTOR_SERVICE,
};
use crate::common::views::IAddedViewDescriptorState;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::views::view_container_model::{ViewContainerModel, create_views_logger, get_views_state_storage_id, parse_json};

/// `IViewsCustomizations`.
#[derive(Clone, Default)]
struct ViewsCustomizations {
    view_container_locations: OrderedMap<String, ViewContainerLocation>,
    view_locations: OrderedMap<String, String>,
    view_container_badge_enablement_states: OrderedMap<String, bool>,
}

impl ViewsCustomizations {
    /// `JSON.parse(value) as IViewsCustomizations`, com os três dicionários que faltam preenchidos (`?? {}`).
    fn parse(value: &str) -> Self {
        let parsed = parse_json(value);
        let mut customizations = Self::default();
        if let Some(object) = parsed.get("viewContainerLocations").and_then(Value::as_object) {
            for (id, location) in object {
                if let Some(location) = location.as_i64().and_then(ViewContainerLocation::from_value) {
                    customizations.view_container_locations.insert(id.clone(), location);
                }
            }
        }
        if let Some(object) = parsed.get("viewLocations").and_then(Value::as_object) {
            for (view_id, container_id) in object {
                if let Some(container_id) = container_id.as_str() {
                    customizations.view_locations.insert(view_id.clone(), container_id.to_string());
                }
            }
        }
        if let Some(object) = parsed.get("viewContainerBadgeEnablementStates").and_then(Value::as_object) {
            for (id, enabled) in object {
                if let Some(enabled) = enabled.as_bool() {
                    customizations.view_container_badge_enablement_states.insert(id.clone(), enabled);
                }
            }
        }
        customizations
    }

    /// `JSON.stringify(viewCustomizations)`.
    fn stringify(&self) -> String {
        let mut view_container_locations = Map::new();
        for (id, location) in self.view_container_locations.iter() {
            view_container_locations.insert(id.clone(), Value::from(location.value()));
        }
        let mut view_locations = Map::new();
        for (view_id, container_id) in self.view_locations.iter() {
            view_locations.insert(view_id.clone(), Value::String(container_id.clone()));
        }
        let mut badge_enablement_states = Map::new();
        for (id, enabled) in self.view_container_badge_enablement_states.iter() {
            badge_enablement_states.insert(id.clone(), Value::Bool(*enabled));
        }
        let mut root = Map::new();
        root.insert("viewContainerLocations".to_string(), Value::Object(view_container_locations));
        root.insert("viewLocations".to_string(), Value::Object(view_locations));
        root.insert("viewContainerBadgeEnablementStates".to_string(), Value::Object(badge_enablement_states));
        Value::Object(root).to_string()
    }
}

/// `getViewContainerStorageId(viewContainerId)`.
fn get_view_container_storage_id(view_container_id: &str) -> String {
    format!("{view_container_id}.state")
}

/// O `{ viewContainerModel, disposables } & IDisposable` do mapa `viewContainerModels`.
struct ViewContainerModelEntry {
    view_container_model: Rc<ViewContainerModel>,
    disposables: Rc<DisposableStore>,
}

impl IDisposable for ViewContainerModelEntry {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// O `Map<string, IContextKey<boolean>>` de chaves de contexto por id.
type ContextKeys = RefCell<HashMap<String, Rc<dyn IContextKey>>>;

/// `getOrCreate...ContextKey`: a chave `id` (`new RawContextKey(id, false).bindTo(contextKeyService)`), criada uma vez.
fn get_or_create_context_key(context_key_service: &dyn IContextKeyService, context_keys: &ContextKeys, id: String) -> Rc<dyn IContextKey> {
    let existing = context_keys.borrow().get(&id).cloned();
    match existing {
        Some(context_key) => context_key,
        None => {
            let context_key = RawContextKey::new(&id, false, ContextKeyMeta::None).bind_to(context_key_service);
            context_keys.borrow_mut().insert(id, context_key.clone());
            context_key
        }
    }
}

/// `ViewDescriptorService`.
pub struct ViewDescriptorService {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    this: Weak<ViewDescriptorService>,

    on_did_change_container: Rc<Emitter<ViewsContainerChange>>,
    on_did_change_location: Rc<Emitter<ViewsLocationChange>>,
    on_did_change_container_location: Rc<Emitter<ViewContainerLocationChange>>,
    on_did_change_view_containers: Rc<Emitter<ViewContainersChange>>,

    view_container_models: Rc<DisposableMap<ById<ViewContainer>, ViewContainerModelEntry>>,
    active_view_context_keys: ContextKeys,
    movable_view_context_keys: ContextKeys,
    default_view_location_context_keys: ContextKeys,
    default_view_container_location_context_keys: ContextKeys,

    views_registry: Rc<ViewsRegistry>,
    view_containers_registry: Rc<ViewContainersRegistry>,

    view_containers_custom_locations: RefCell<OrderedMap<String, ViewContainerLocation>>,
    view_descriptors_custom_locations: RefCell<OrderedMap<String, String>>,
    view_container_badge_enablement_states: RefCell<OrderedMap<String, bool>>,

    /// `_viewCustomizations`.
    view_customizations_cache: RefCell<Option<ViewsCustomizations>>,

    logger: Lazy<LoggerRef>,
    is_sessions_window: bool,

    instantiation_service: Rc<InstantiationService>,
    context_key_service: Rc<dyn IContextKeyService>,
    storage_service: Rc<dyn StorageService>,
    extension_service: Rc<dyn IExtensionService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

const VIEWS_CUSTOMIZATIONS: &str = "views.customizations";
const COMMON_CONTAINER_ID_PREFIX: &str = "workbench.views.service";

impl Injectable for ViewDescriptorService {
    type Args = ();
    type Deps = (
        Rc<InstantiationService>,
        Rc<dyn IContextKeyService>,
        Rc<dyn StorageService>,
        Rc<dyn IExtensionService>,
        Rc<dyn ITelemetryService>,
        Rc<dyn LoggerService>,
        Rc<dyn IWorkbenchEnvironmentService>,
    );

    fn create(
        _: (),
        (instantiation_service, context_key_service, storage_service, extension_service, telemetry_service, logger_service, environment_service): Self::Deps,
    ) -> Rc<Self> {
        let is_sessions_window = environment_service.is_sessions_window();

        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change_container = Emitter::new();
        base.register(on_did_change_container.clone());
        let on_did_change_location = Emitter::new();
        base.register(on_did_change_location.clone());
        let on_did_change_container_location = Emitter::new();
        base.register(on_did_change_container_location.clone());
        let on_did_change_view_containers = Emitter::new();
        base.register(on_did_change_view_containers.clone());
        let view_container_models = DisposableMap::new();
        base.register(view_container_models.clone());

        Self::migrate_to_views_customizations_storage(storage_service.as_ref());
        let view_customizations = Self::read_view_customizations(storage_service.as_ref(), is_sessions_window);

        let this = Rc::new_cyclic(|this| Self {
            base,
            store,
            this: this.clone(),
            on_did_change_container,
            on_did_change_location,
            on_did_change_container_location,
            on_did_change_view_containers,
            view_container_models,
            active_view_context_keys: RefCell::new(HashMap::new()),
            movable_view_context_keys: RefCell::new(HashMap::new()),
            default_view_location_context_keys: RefCell::new(HashMap::new()),
            default_view_container_location_context_keys: RefCell::new(HashMap::new()),
            views_registry: views_registry(),
            view_containers_registry: view_containers_registry(),
            view_containers_custom_locations: RefCell::new(view_customizations.view_container_locations.clone()),
            view_descriptors_custom_locations: RefCell::new(view_customizations.view_locations.clone()),
            view_container_badge_enablement_states: RefCell::new(view_customizations.view_container_badge_enablement_states.clone()),
            view_customizations_cache: RefCell::new(Some(view_customizations)),
            logger: Lazy::new(move || create_views_logger(logger_service.as_ref())),
            is_sessions_window,
            instantiation_service,
            context_key_service,
            storage_service,
            extension_service,
            telemetry_service,
        });

        // Register all containers that were registered before this ctor
        for view_container in this.view_containers() {
            this.on_did_register_view_container(&view_container);
        }

        let weak = this.this.clone();
        this.base.register(this.views_registry.on_views_registered().listen(move |views| {
            if let Some(this) = weak.upgrade() {
                this.on_did_register_views(views);
            }
        }));
        let weak = this.this.clone();
        this.base.register(this.views_registry.on_views_deregistered().listen(move |registration| {
            if let Some(this) = weak.upgrade() {
                this.on_did_deregister_views(&registration.views, &registration.view_container);
            }
        }));

        let weak = this.this.clone();
        this.base.register(this.views_registry.on_did_change_container().listen(move |change| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_default_container(&change.views, &change.from, &change.to);
            }
        }));

        let weak = this.this.clone();
        this.base.register(this.view_containers_registry.on_did_register().listen(move |registration| {
            let Some(this) = weak.upgrade() else { return };
            let view_container = &registration.view_container;
            if !this.is_view_container_enabled(view_container) {
                return;
            }
            this.on_did_register_view_container(view_container);
            let location = this.get_view_container_location(view_container).unwrap_or(registration.view_container_location);
            this.on_did_change_view_containers
                .fire(ViewContainersChange { added: vec![ViewContainerAndLocation { container: view_container.clone(), location }], removed: Vec::new() });
        }));

        let weak = this.this.clone();
        this.base.register(this.view_containers_registry.on_did_deregister().listen(move |registration| {
            let Some(this) = weak.upgrade() else { return };
            let view_container = &registration.view_container;
            if !this.is_view_container_enabled(view_container) {
                return;
            }
            this.on_did_deregister_view_container(view_container);
            this.on_did_change_view_containers.fire(ViewContainersChange {
                removed: vec![ViewContainerAndLocation { container: view_container.clone(), location: registration.view_container_location }],
                added: Vec::new(),
            });
        }));

        let weak = this.this.clone();
        this.base.register(this.storage_service.on_did_change_value(StorageScope::Profile, Some(VIEWS_CUSTOMIZATIONS), &this.store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_change();
            }
        }));

        let weak = this.this.clone();
        this.extension_service.when_installed_extensions_registered().then(move |_| {
            if let Some(this) = weak.upgrade() {
                this.when_extensions_registered();
            }
        });

        this
    }
}

service_impl!(ViewDescriptorService => dyn IViewDescriptorService, disposable);

/// `registerSingleton(IViewDescriptorService, ViewDescriptorService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<ViewDescriptorService>(I_VIEW_DESCRIPTOR_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for ViewDescriptorService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ViewDescriptorService {
    /// `migrateToViewsCustomizationsStorage()`.
    fn migrate_to_views_customizations_storage(storage_service: &dyn StorageService) {
        if storage_service.get(VIEWS_CUSTOMIZATIONS, StorageScope::Profile, None).is_some_and(|value| !value.is_empty()) {
            return;
        }

        let view_container_locations_value = storage_service.get("views.cachedViewContainerLocations", StorageScope::Profile, None).filter(|value| !value.is_empty());
        let view_descriptor_locations_value = storage_service.get("views.cachedViewPositions", StorageScope::Profile, None).filter(|value| !value.is_empty());
        if view_container_locations_value.is_none() && view_descriptor_locations_value.is_none() {
            return;
        }

        let mut views_customizations = ViewsCustomizations::default();
        // `[string, ViewContainerLocation][]`
        if let Some(value) = &view_container_locations_value
            && let Value::Array(entries) = parse_json(value)
        {
            for entry in &entries {
                let id = entry.get(0).and_then(Value::as_str);
                let location = entry.get(1).and_then(Value::as_i64).and_then(ViewContainerLocation::from_value);
                if let (Some(id), Some(location)) = (id, location) {
                    views_customizations.view_container_locations.insert(id.to_string(), location);
                }
            }
        }
        // `[string, { containerId: string }][]`
        if let Some(value) = &view_descriptor_locations_value
            && let Value::Array(entries) = parse_json(value)
        {
            for entry in &entries {
                let id = entry.get(0).and_then(Value::as_str);
                let container_id = entry.get(1).and_then(|position| position.get("containerId")).and_then(Value::as_str);
                if let (Some(id), Some(container_id)) = (id, container_id) {
                    views_customizations.view_locations.insert(id.to_string(), container_id.to_string());
                }
            }
        }
        storage_service.store(VIEWS_CUSTOMIZATIONS, StorageValue::String(views_customizations.stringify()), StorageScope::Profile, StorageTarget::User);
        storage_service.remove("views.cachedViewContainerLocations", StorageScope::Profile);
        storage_service.remove("views.cachedViewPositions", StorageScope::Profile);
    }

    fn register_grouped_views(&self, grouped_views: &OrderedMap<String, Vec<ViewDescriptorRc>>) {
        for (container_id, views) in grouped_views.iter() {
            let view_container = self.get_view_container_by_id(container_id);

            // The container has not been registered yet
            let registered = view_container.as_ref().filter(|view_container| self.view_container_models.has(&ById((*view_container).clone())));
            let Some(view_container) = registered else {
                // Register if the container is a genarated container
                if self.is_generated_container_id(container_id) {
                    let view_container_location = self.view_containers_custom_locations.borrow().get(container_id).copied();
                    if let Some(view_container_location) = view_container_location {
                        self.register_generated_view_container(view_container_location, Some(container_id));
                    }
                }
                // Registration of the container handles registration of its views
                continue;
            };

            // Filter out views that have already been added to the view container model
            // This is needed when statically-registered views are moved to
            // other statically registered containers as they will both try to add on startup
            let existing = self.get_or_register_view_container_model(view_container).all_view_descriptors();
            let views_to_add: Vec<ViewDescriptorRc> = views.iter().filter(|view| !existing.iter().any(|vd| vd.id == view.id)).cloned().collect();
            self.add_views(view_container, &views_to_add, ViewVisibilityState::Default);
        }
    }

    fn deregister_grouped_views(&self, grouped_views: &OrderedMap<String, Vec<ViewDescriptorRc>>) {
        for (view_container_id, views) in grouped_views.iter() {
            let view_container = self.get_view_container_by_id(view_container_id);

            // The container has not been registered yet
            let Some(view_container) = view_container.filter(|view_container| self.view_container_models.has(&ById(view_container.clone()))) else {
                continue;
            };

            self.remove_views(&view_container, views);
        }
    }

    fn move_orphan_views_to_default_location(&self) {
        let custom_locations: Vec<(String, String)> = self.view_descriptors_custom_locations.borrow().iter().map(|(view_id, container_id)| (view_id.clone(), container_id.clone())).collect();
        for (view_id, container_id) in custom_locations {
            // check if the view container exists
            if self.get_view_container_by_id(&container_id).is_some() {
                continue;
            }

            // check if view has been registered to default location
            let view_container = self.views_registry.get_view_container(&view_id);
            let view_descriptor = self.get_view_descriptor_by_id(&view_id);
            if let (Some(view_container), Some(view_descriptor)) = (view_container, view_descriptor) {
                self.add_views(&view_container, &[view_descriptor], ViewVisibilityState::Default);
            }
        }
    }

    fn when_extensions_registered(&self) {
        // Handle those views whose custom parent view container does not exist anymore
        // May be the extension contributing this view container is no longer installed
        // Or the parent view container is generated and no longer available.
        self.move_orphan_views_to_default_location();

        // Clean up empty generated view containers
        let custom_location_ids: Vec<String> = self.view_containers_custom_locations.borrow().keys().cloned().collect();
        for view_container_id in custom_location_ids {
            self.clean_up_generated_view_container(&view_container_id);
        }

        // Save updated view customizations after cleanup
        self.save_view_customizations();
    }

    fn on_did_register_views(&self, views: &[ViewsRegistration]) {
        self.context_key_service.buffer_change_events(&|| {
            for ViewsRegistration { views, view_container } in views {
                // When views are registered, we need to regroup them based on the customizations
                let regrouped_views = self.regroup_views(&view_container.id, views);

                // Once they are grouped, try registering them which occurs
                // if the container has already been registered within this service
                // or we can generate the container from the source view id
                self.register_grouped_views(&regrouped_views);

                for view_descriptor in views {
                    self.get_or_create_movable_view_context_key(view_descriptor).set(Dynamic::Bool(view_descriptor.can_move_view));
                }
            }
        });
    }

    fn is_generated_container_id(&self, id: &str) -> bool {
        id.starts_with(COMMON_CONTAINER_ID_PREFIX)
    }

    fn on_did_deregister_views(&self, views: &[ViewDescriptorRc], view_container: &ViewContainerRc) {
        // When views are registered, we need to regroup them based on the customizations
        let regrouped_views = self.regroup_views(&view_container.id, views);
        self.deregister_grouped_views(&regrouped_views);
        self.context_key_service.buffer_change_events(&|| {
            for view_descriptor in views {
                self.get_or_create_movable_view_context_key(view_descriptor).set(Dynamic::Bool(false));
            }
        });
    }

    fn regroup_views(&self, container_id: &str, views: &[ViewDescriptorRc]) -> OrderedMap<String, Vec<ViewDescriptorRc>> {
        let mut views_by_container: OrderedMap<String, Vec<ViewDescriptorRc>> = OrderedMap::new();

        for view_descriptor in views {
            let correct_container_id = self.view_descriptors_custom_locations.borrow().get(&view_descriptor.id).cloned().unwrap_or_else(|| container_id.to_string());
            match views_by_container.get_mut(&correct_container_id) {
                Some(container_views) => container_views.push(view_descriptor.clone()),
                None => {
                    views_by_container.insert(correct_container_id, vec![view_descriptor.clone()]);
                }
            }
        }

        views_by_container
    }

    /// O `viewContainerModel` do contêiner (`getViewContainerModel`, com o tipo concreto).
    pub fn get_or_register_view_container_model(&self, view_container: &ViewContainerRc) -> Rc<ViewContainerModel> {
        let key = ById(view_container.clone());
        if let Some(entry) = self.view_container_models.get(&key) {
            return entry.view_container_model.clone();
        }

        let disposables = DisposableStore::new();
        let view_container_model = self.instantiation_service.create_instance::<ViewContainerModel>(view_container.clone());
        disposables.add_rc(view_container_model.clone());

        self.on_did_change_active_views(&view_container_model.active_view_descriptors(), &[]);
        let weak = self.this.clone();
        disposables.add(view_container_model.on_did_change_active_view_descriptors().listen(move |changed| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_active_views(&changed.added, &changed.removed);
            }
        }));

        self.on_did_change_visible_views(&view_container_model.visible_view_descriptors(), &[]);
        let weak = self.this.clone();
        disposables.add(view_container_model.on_did_add_visible_view_descriptors().listen(move |added| {
            if let Some(this) = weak.upgrade() {
                let added: Vec<ViewDescriptorRc> = added.iter().map(|added| added.view_descriptor.clone()).collect();
                this.on_did_change_visible_views(&added, &[]);
            }
        }));
        let weak = self.this.clone();
        disposables.add(view_container_model.on_did_remove_visible_view_descriptors().listen(move |removed| {
            if let Some(this) = weak.upgrade() {
                let removed: Vec<ViewDescriptorRc> = removed.iter().map(|removed| removed.view_descriptor.clone()).collect();
                this.on_did_change_visible_views(&[], &removed);
            }
        }));

        self.view_container_models.set(key, Rc::new(ViewContainerModelEntry { view_container_model: view_container_model.clone(), disposables }), false);

        // Register all views that were statically registered to this container
        // Potentially, this is registering something that was handled by another container
        // addViews() handles this by filtering views that are already registered
        self.on_did_register_views(&[ViewsRegistration { views: self.views_registry.get_views(view_container), view_container: view_container.clone() }]);

        // Add views that were registered prior to this view container
        let views_to_register: Vec<ViewDescriptorRc> = self
            .get_views_by_container(view_container)
            .into_iter()
            .filter(|view| !self.get_default_container_by_id(&view.id).is_some_and(|default_container| Rc::ptr_eq(&default_container, view_container)))
            .collect();
        if !views_to_register.is_empty() {
            self.add_views(view_container, &views_to_register, ViewVisibilityState::Default);
            self.context_key_service.buffer_change_events(&|| {
                for view_descriptor in &views_to_register {
                    self.get_or_create_movable_view_context_key(view_descriptor).set(Dynamic::Bool(view_descriptor.can_move_view));
                }
            });
        }

        view_container_model
    }

    fn is_view_container_enabled(&self, view_container: &ViewContainerRc) -> bool {
        self.is_enabled(view_container.window_enablement)
    }

    fn is_view_enabled(&self, view: &ViewDescriptorRc) -> bool {
        self.is_enabled(view.window_enablement)
    }

    fn is_enabled(&self, enablement: Option<WindowEnablement>) -> bool {
        if self.is_sessions_window {
            return matches!(enablement, Some(WindowEnablement::Sessions | WindowEnablement::Both));
        }
        matches!(enablement, None | Some(WindowEnablement::Editor | WindowEnablement::Both))
    }

    fn move_view_container_to_location_without_saving(&self, view_container: &ViewContainerRc, location: ViewContainerLocation, requested_index: Option<usize>) {
        // O contêiner sem localização não está no registro (o original seguiria com `from` indefinido).
        let Some(from) = self.get_view_container_location(view_container) else { return };
        let to = location;
        if from != to {
            let is_generated_view_container = self.is_generated_container_id(&view_container.id);
            let is_default_view_container_location = Some(to) == self.get_default_view_container_location(view_container);
            if is_generated_view_container || !is_default_view_container_location {
                self.view_containers_custom_locations.borrow_mut().insert(view_container.id.clone(), to);
            } else {
                self.view_containers_custom_locations.borrow_mut().remove(&view_container.id);
            }
            self.get_or_create_default_view_container_location_context_key(view_container).set(Dynamic::Bool(is_generated_view_container || is_default_view_container_location));

            view_container.set_requested_index(requested_index);
            self.on_did_change_container_location.fire(ViewContainerLocationChange { view_container: view_container.clone(), from, to });

            let views = self.get_views_by_container(view_container);
            self.on_did_change_location.fire(ViewsLocationChange { views, from, to });
        }
    }

    fn clean_up_generated_view_container(&self, view_container_id: &str) {
        // Skip if container is not generated
        if !self.is_generated_container_id(view_container_id) {
            return;
        }

        // Skip if container has views registered
        let view_container = self.get_view_container_by_id(view_container_id);
        if let Some(view_container) = &view_container
            && !self.get_or_register_view_container_model(view_container).all_view_descriptors().is_empty()
        {
            return;
        }

        // Skip if container has moved views
        if self.view_descriptors_custom_locations.borrow().values().any(|container_id| container_id == view_container_id) {
            return;
        }

        // Deregister the container
        if let Some(view_container) = &view_container {
            self.view_containers_registry.deregister_view_container(view_container);
        }

        self.view_containers_custom_locations.borrow_mut().remove(&view_container_id.to_string());
        self.view_container_badge_enablement_states.borrow_mut().remove(&view_container_id.to_string());

        // Clean up caches of container
        let storage_id = view_container.as_ref().and_then(|view_container| view_container.storage_id.clone()).filter(|storage_id| !storage_id.is_empty());
        self.storage_service.remove(&get_views_state_storage_id(&storage_id.unwrap_or_else(|| get_view_container_storage_id(view_container_id))), StorageScope::Profile);
    }

    fn register_generated_view_container(&self, location: ViewContainerLocation, existing_id: Option<&str>) -> ViewContainerRc {
        let id = existing_id.filter(|id| !id.is_empty()).map(str::to_string).unwrap_or_else(|| self.generate_container_id(location));

        let title = LocalizedString { value: localize("user", "User View Container", &[]), original: "User View Container".to_string() }; // having a placeholder title - this should not be shown anywhere
        let container = self.view_containers_registry.register_view_container(
            IViewContainerDescriptor {
                icon: if location == ViewContainerLocation::Sidebar { Some(ViewIcon::Theme(default_view_icon())) } else { None },
                storage_id: Some(get_view_container_storage_id(&id)),
                hide_if_empty: true,
                ..IViewContainerDescriptor::new(&id, title)
            },
            location,
            Some(RegisterViewContainerOptions { do_not_register_open_command: true, ..RegisterViewContainerOptions::default() }),
        );

        if self.view_containers_custom_locations.borrow().get(&container.id).copied() != Some(location) {
            self.view_containers_custom_locations.borrow_mut().insert(container.id.clone(), location);
        }

        self.get_or_create_default_view_container_location_context_key(&container).set(Dynamic::Bool(true));

        container
    }

    fn on_did_storage_change(&self) {
        if self.view_customizations().stringify() != self.get_stored_view_customizations_value() /* This checks if current window changed the value or not */ {
            self.on_did_view_customizations_storage_change();
        }
    }

    fn on_did_view_customizations_storage_change(&self) {
        *self.view_customizations_cache.borrow_mut() = None;

        let customizations = self.view_customizations();
        let new_view_container_customizations = customizations.view_container_locations;
        let new_view_descriptor_customizations = customizations.view_locations;
        let mut view_containers_to_move: Vec<(ViewContainerRc, ViewContainerLocation)> = Vec::new();
        let mut views_to_move: Vec<ViewsContainerChange> = Vec::new();

        for (container_id, location) in new_view_container_customizations.iter() {
            let container = self.get_view_container_by_id(container_id);
            if let Some(container) = container {
                if Some(*location) != self.get_view_container_location(&container) {
                    view_containers_to_move.push((container, *location));
                }
            }
            // If the container is generated and not registered, we register it now
            else if self.is_generated_container_id(container_id) {
                self.register_generated_view_container(*location, Some(container_id));
            }
        }

        for view_container in self.view_containers() {
            if !new_view_container_customizations.contains_key(&view_container.id) {
                let current_location = self.get_view_container_location(&view_container);
                let default_location = self.get_default_view_container_location(&view_container);
                if current_location != default_location
                    && let Some(default_location) = default_location
                {
                    view_containers_to_move.push((view_container, default_location));
                }
            }
        }

        for (view_id, view_container_id) in new_view_descriptor_customizations.iter() {
            if let Some(view_descriptor) = self.get_view_descriptor_by_id(view_id) {
                let prev_view_container = self.get_view_container_by_view_id(view_id);
                let new_view_container = self.get_view_container_by_id(view_container_id);
                if let (Some(prev_view_container), Some(new_view_container)) = (prev_view_container, new_view_container)
                    && !Rc::ptr_eq(&new_view_container, &prev_view_container)
                {
                    views_to_move.push(ViewsContainerChange { views: vec![view_descriptor], from: prev_view_container, to: new_view_container });
                }
            }
        }

        // If a value is not present in the cache, it must be reset to default
        for view_container in self.view_containers() {
            let view_container_model = self.get_or_register_view_container_model(&view_container);
            for view_descriptor in view_container_model.all_view_descriptors() {
                if !new_view_descriptor_customizations.contains_key(&view_descriptor.id) {
                    let current_container = self.get_view_container_by_view_id(&view_descriptor.id);
                    let default_container = self.get_default_container_by_id(&view_descriptor.id);
                    if let (Some(current_container), Some(default_container)) = (current_container, default_container)
                        && !Rc::ptr_eq(&current_container, &default_container)
                    {
                        views_to_move.push(ViewsContainerChange { views: vec![view_descriptor], from: current_container, to: default_container });
                    }
                }
            }
        }

        // Execute View Container Movements
        for (container, location) in &view_containers_to_move {
            self.move_view_container_to_location_without_saving(container, *location, None);
        }
        // Execute View Movements
        for ViewsContainerChange { views, from, to } in &views_to_move {
            self.move_views_without_saving(views, from, to, ViewVisibilityState::Default);
        }

        *self.view_containers_custom_locations.borrow_mut() = new_view_container_customizations;
        *self.view_descriptors_custom_locations.borrow_mut() = new_view_descriptor_customizations;
    }

    // Generated Container Id Format
    // {Common Prefix}.{Location}.{Uniqueness Id}
    // Old Format (deprecated)
    // {Common Prefix}.{Uniqueness Id}.{Source View Id}
    fn generate_container_id(&self, location: ViewContainerLocation) -> String {
        format!("{COMMON_CONTAINER_ID_PREFIX}.{}.{}", view_container_location_to_string(location), generate_uuid())
    }

    fn save_view_customizations(&self) {
        let mut view_customizations = ViewsCustomizations::default();

        let custom_locations: Vec<(String, ViewContainerLocation)> = self.view_containers_custom_locations.borrow().iter().map(|(id, location)| (id.clone(), *location)).collect();
        for (container_id, location) in custom_locations {
            let container = self.get_view_container_by_id(&container_id);
            // Skip if the view container is not a generated container and in default location
            if let Some(container) = container
                && !self.is_generated_container_id(&container_id)
                && Some(location) == self.get_default_view_container_location(&container)
            {
                continue;
            }
            view_customizations.view_container_locations.insert(container_id, location);
        }

        let view_locations: Vec<(String, String)> = self.view_descriptors_custom_locations.borrow().iter().map(|(view_id, container_id)| (view_id.clone(), container_id.clone())).collect();
        for (view_id, view_container_id) in view_locations {
            if let Some(view_container) = self.get_view_container_by_id(&view_container_id) {
                let default_container = self.get_default_container_by_id(&view_id);
                // Skip if the view is at default location
                // https://github.com/microsoft/vscode/issues/90414
                if default_container.is_some_and(|default_container| default_container.id == view_container.id) {
                    continue;
                }
            }
            view_customizations.view_locations.insert(view_id, view_container_id);
        }

        // Loop through viewContainerBadgeEnablementStates and save only the ones that are disabled
        let badge_enablement_states: Vec<(String, bool)> = self.view_container_badge_enablement_states.borrow().iter().map(|(id, state)| (id.clone(), *state)).collect();
        for (view_container_id, badge_enablement_state) in badge_enablement_states {
            if !badge_enablement_state {
                view_customizations.view_container_badge_enablement_states.insert(view_container_id, badge_enablement_state);
            }
        }
        self.set_view_customizations(view_customizations);
    }

    /// O getter `viewCustomizations`.
    fn view_customizations(&self) -> ViewsCustomizations {
        let cached = self.view_customizations_cache.borrow().clone();
        match cached {
            Some(view_customizations) => view_customizations,
            None => {
                let view_customizations = ViewsCustomizations::parse(&self.get_stored_view_customizations_value());
                *self.view_customizations_cache.borrow_mut() = Some(view_customizations.clone());
                view_customizations
            }
        }
    }

    /// O setter `viewCustomizations`.
    fn set_view_customizations(&self, view_customizations: ViewsCustomizations) {
        let value = view_customizations.stringify();
        if self.view_customizations().stringify() != value {
            *self.view_customizations_cache.borrow_mut() = Some(view_customizations);
            self.set_stored_view_customizations_value(&value);
        }
    }

    /// O `viewCustomizations` guardado, para a inicialização (o getter `viewCustomizations` antes do objeto existir).
    fn read_view_customizations(storage_service: &dyn StorageService, is_sessions_window: bool) -> ViewsCustomizations {
        ViewsCustomizations::parse(&Self::stored_view_customizations_value(storage_service, is_sessions_window))
    }

    fn stored_view_customizations_value(storage_service: &dyn StorageService, is_sessions_window: bool) -> String {
        if is_sessions_window {
            return "{}".to_string();
        }
        storage_service.get(VIEWS_CUSTOMIZATIONS, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_string())
    }

    fn get_stored_view_customizations_value(&self) -> String {
        Self::stored_view_customizations_value(self.storage_service.as_ref(), self.is_sessions_window)
    }

    fn set_stored_view_customizations_value(&self, value: &str) {
        if self.is_sessions_window {
            return;
        }
        self.storage_service.store(VIEWS_CUSTOMIZATIONS, StorageValue::String(value.to_string()), StorageScope::Profile, StorageTarget::User);
    }

    fn get_views_by_container(&self, view_container: &ViewContainerRc) -> Vec<ViewDescriptorRc> {
        let custom_locations: Vec<(String, String)> = self.view_descriptors_custom_locations.borrow().iter().map(|(view_id, container_id)| (view_id.clone(), container_id.clone())).collect();
        let mut result: Vec<ViewDescriptorRc> = self
            .views_registry
            .get_views(view_container)
            .into_iter()
            .filter(|view_descriptor| {
                let view_descriptor_view_container_id =
                    custom_locations.iter().find(|(view_id, _)| *view_id == view_descriptor.id).map(|(_, container_id)| container_id.as_str()).unwrap_or(view_container.id.as_str());
                view_descriptor_view_container_id == view_container.id
            })
            .collect();

        for (view_id, view_container_id) in &custom_locations {
            if *view_container_id != view_container.id {
                continue;
            }

            if self.views_registry.get_view_container(view_id).is_some_and(|container| Rc::ptr_eq(&container, view_container)) {
                continue;
            }

            if let Some(view_descriptor) = self.get_view_descriptor_by_id(view_id) {
                result.push(view_descriptor);
            }
        }

        result
    }

    fn on_did_register_view_container(&self, view_container: &ViewContainerRc) {
        let default_location = if self.is_generated_container_id(&view_container.id) {
            true
        } else {
            self.get_view_container_location(view_container) == self.get_default_view_container_location(view_container)
        };
        self.get_or_create_default_view_container_location_context_key(view_container).set(Dynamic::Bool(default_location));
        self.get_or_register_view_container_model(view_container);
    }

    fn on_did_deregister_view_container(&self, view_container: &ViewContainerRc) {
        self.view_container_models.delete_and_dispose(&ById(view_container.clone()));
    }

    fn on_did_change_active_views(&self, added: &[ViewDescriptorRc], removed: &[ViewDescriptorRc]) {
        self.context_key_service.buffer_change_events(&|| {
            for view_descriptor in added {
                self.get_or_create_active_view_context_key(view_descriptor).set(Dynamic::Bool(true));
            }
            for view_descriptor in removed {
                self.get_or_create_active_view_context_key(view_descriptor).set(Dynamic::Bool(false));
            }
        });
    }

    fn on_did_change_visible_views(&self, added: &[ViewDescriptorRc], removed: &[ViewDescriptorRc]) {
        self.context_key_service.buffer_change_events(&|| {
            for view_descriptor in added {
                self.get_or_create_visible_view_context_key(view_descriptor).set(Dynamic::Bool(true));
            }
            for view_descriptor in removed {
                self.get_or_create_visible_view_context_key(view_descriptor).set(Dynamic::Bool(false));
            }
        });
    }

    fn add_views(&self, container: &ViewContainerRc, views: &[ViewDescriptorRc], visibility_state: ViewVisibilityState) {
        self.context_key_service.buffer_change_events(&|| {
            for view in views {
                let is_default_container = self.get_default_container_by_id(&view.id).is_some_and(|default_container| Rc::ptr_eq(&default_container, container));
                self.get_or_create_default_view_location_context_key(view).set(Dynamic::Bool(is_default_container));
                if is_default_container {
                    self.view_descriptors_custom_locations.borrow_mut().remove(&view.id);
                } else {
                    self.view_descriptors_custom_locations.borrow_mut().insert(view.id.clone(), container.id.clone());
                }
            }
        });

        self.get_or_register_view_container_model(container).add(
            views
                .iter()
                .map(|view| IAddedViewDescriptorState {
                    view_descriptor: view.clone(),
                    collapsed: if visibility_state == ViewVisibilityState::Default { None } else { Some(false) },
                    visible: if visibility_state == ViewVisibilityState::Default { None } else { Some(true) },
                })
                .collect(),
        );
    }

    fn remove_views(&self, container: &ViewContainerRc, views: &[ViewDescriptorRc]) {
        // Set view default location keys to false
        self.context_key_service.buffer_change_events(&|| {
            for view in views {
                let custom_container_id = self.view_descriptors_custom_locations.borrow().get(&view.id).cloned();
                if custom_container_id.as_deref() == Some(container.id.as_str()) {
                    self.view_descriptors_custom_locations.borrow_mut().remove(&view.id);
                }
                self.get_or_create_default_view_location_context_key(view).set(Dynamic::Bool(false));
            }
        });

        // Remove the views
        self.get_or_register_view_container_model(container).remove(views);
    }

    fn on_did_change_default_container(&self, views: &[ViewDescriptorRc], from: &ViewContainerRc, to: &ViewContainerRc) {
        let from_is_registered = self.view_containers().iter().any(|view_container| Rc::ptr_eq(view_container, from));
        let views_to_move: Vec<ViewDescriptorRc> = {
            let custom_locations = self.view_descriptors_custom_locations.borrow();
            views
                .iter()
                .filter(|view| {
                    !custom_locations.contains_key(&view.id) // Move views which are not already moved
                        || (!from_is_registered && custom_locations.get(&view.id).is_some_and(|container_id| *container_id == from.id)) // Move views which are moved from a removed container
                })
                .cloned()
                .collect()
        };
        if !views_to_move.is_empty() {
            self.move_views_without_saving(&views_to_move, from, to, ViewVisibilityState::Expand);
        }
    }

    fn report_moved_views(&self, views: &[ViewDescriptorRc], from: &ViewContainerRc, to: &ViewContainerRc) {
        let container_to_string = |container: &ViewContainerRc| -> String {
            if container.id.starts_with(COMMON_CONTAINER_ID_PREFIX) {
                return "custom".to_string();
            }

            if container.extension_id.is_none() {
                return container.id.clone();
            }

            "extension".to_string()
        };

        let old_location = self.get_view_container_location(from);
        let new_location = self.get_view_container_location(to);
        let view_count = views.len();
        let from_container = container_to_string(from);
        let to_container = container_to_string(to);
        let from_location = if old_location == Some(ViewContainerLocation::Panel) { "panel" } else { "sidebar" };
        let to_location = if new_location == Some(ViewContainerLocation::Panel) { "panel" } else { "sidebar" };

        self.telemetry_service.public_log2(
            "viewDescriptorService.moveViews",
            Some(Dynamic::object([
                ("viewCount", Dynamic::Number(view_count as f64)),
                ("fromContainer", Dynamic::string(&from_container)),
                ("toContainer", Dynamic::string(&to_container)),
                ("fromLocation", Dynamic::string(from_location)),
                ("toLocation", Dynamic::string(to_location)),
            ])),
        );
    }

    fn move_views_without_saving(&self, views: &[ViewDescriptorRc], from: &ViewContainerRc, to: &ViewContainerRc, visibility_state: ViewVisibilityState) {
        self.remove_views(from, views);
        self.add_views(to, views, visibility_state);

        let old_location = self.get_view_container_location(from);
        let new_location = self.get_view_container_location(to);

        if old_location != new_location
            && let (Some(old_location), Some(new_location)) = (old_location, new_location)
        {
            self.on_did_change_location.fire(ViewsLocationChange { views: views.to_vec(), from: old_location, to: new_location });
        }

        self.on_did_change_container.fire(ViewsContainerChange { views: views.to_vec(), from: from.clone(), to: to.clone() });
    }

    fn get_or_create_active_view_context_key(&self, view_descriptor: &ViewDescriptorRc) -> Rc<dyn IContextKey> {
        get_or_create_context_key(self.context_key_service.as_ref(), &self.active_view_context_keys, format!("{}.active", view_descriptor.id))
    }

    fn get_or_create_visible_view_context_key(&self, view_descriptor: &ViewDescriptorRc) -> Rc<dyn IContextKey> {
        get_or_create_context_key(self.context_key_service.as_ref(), &self.active_view_context_keys, format!("{}.visible", view_descriptor.id))
    }

    fn get_or_create_movable_view_context_key(&self, view_descriptor: &ViewDescriptorRc) -> Rc<dyn IContextKey> {
        get_or_create_context_key(self.context_key_service.as_ref(), &self.movable_view_context_keys, format!("{}.canMove", view_descriptor.id))
    }

    fn get_or_create_default_view_location_context_key(&self, view_descriptor: &ViewDescriptorRc) -> Rc<dyn IContextKey> {
        get_or_create_context_key(self.context_key_service.as_ref(), &self.default_view_location_context_keys, format!("{}.defaultViewLocation", view_descriptor.id))
    }

    fn get_or_create_default_view_container_location_context_key(&self, view_container: &ViewContainerRc) -> Rc<dyn IContextKey> {
        get_or_create_context_key(
            self.context_key_service.as_ref(),
            &self.default_view_container_location_context_keys,
            format!("{}.defaultViewContainerLocation", view_container.id),
        )
    }
}

impl IViewDescriptorService for ViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        self.view_containers_registry.all().into_iter().filter(|view_container| self.is_view_container_enabled(view_container)).collect()
    }

    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        self.on_did_change_view_containers.event()
    }

    fn get_view_descriptor_by_id(&self, view_id: &str) -> Option<ViewDescriptorRc> {
        let view = self.views_registry.get_view(view_id);
        if let Some(view) = &view
            && !self.is_view_enabled(view)
        {
            return None;
        }
        view
    }

    fn get_view_location_by_id(&self, view_id: &str) -> Option<ViewContainerLocation> {
        let container = self.get_view_container_by_view_id(view_id)?;

        self.get_view_container_location(&container)
    }

    fn get_view_container_by_view_id(&self, view_id: &str) -> Option<ViewContainerRc> {
        // Check if the view itself should be visible in current workspace
        let view = self.views_registry.get_view(view_id);
        if let Some(view) = &view
            && !self.is_view_enabled(view)
        {
            return None;
        }

        let container_id = self.view_descriptors_custom_locations.borrow().get(&view_id.to_string()).cloned().filter(|container_id| !container_id.is_empty());

        match container_id {
            Some(container_id) => self.get_view_container_by_id(&container_id),
            None => self.get_default_container_by_id(view_id),
        }
    }

    fn get_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        let custom_location = self.view_containers_custom_locations.borrow().get(&view_container.id).copied();
        custom_location.or_else(|| self.get_default_view_container_location(view_container))
    }

    fn get_default_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        self.view_containers_registry.get_view_container_location(view_container)
    }

    fn get_default_container_by_id(&self, view_id: &str) -> Option<ViewContainerRc> {
        self.views_registry.get_view_container(view_id)
    }

    fn get_view_container_model(&self, view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        self.get_or_register_view_container_model(view_container)
    }

    fn get_view_container_by_id(&self, id: &str) -> Option<ViewContainerRc> {
        self.view_containers().into_iter().find(|view_container| view_container.id == id)
    }

    fn get_view_containers_by_location(&self, location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        self.view_containers().into_iter().filter(|view_container| self.get_view_container_location(view_container) == Some(location)).collect()
    }

    fn get_default_view_container(&self, location: ViewContainerLocation) -> Option<ViewContainerRc> {
        let view_containers = self.view_containers_registry.get_default_view_containers(location);
        view_containers.into_iter().find(|view_container| self.is_view_container_enabled(view_container))
    }

    fn can_move_views(&self) -> bool {
        !self.is_sessions_window
    }

    fn move_view_container_to_location(&self, view_container: &ViewContainerRc, location: ViewContainerLocation, requested_index: Option<usize>, reason: Option<&str>) {
        if !self.can_move_views() {
            return;
        }
        self.logger.value().trace(&format!("moveViewContainerToLocation: viewContainer:{} location:{} reason:{}", view_container.id, location.value(), reason.unwrap_or("undefined")));
        self.move_view_container_to_location_without_saving(view_container, location, requested_index);
        self.save_view_customizations();
    }

    fn get_view_container_badge_enablement_state(&self, id: &str) -> bool {
        self.view_container_badge_enablement_states.borrow().get(&id.to_string()).copied().unwrap_or(true)
    }

    fn set_view_container_badge_enablement_state(&self, id: &str, badges_enabled: bool) {
        self.view_container_badge_enablement_states.borrow_mut().insert(id.to_string(), badges_enabled);
        self.save_view_customizations();
    }

    fn move_view_to_location(&self, view: &ViewDescriptorRc, location: ViewContainerLocation, reason: Option<&str>) {
        if !self.can_move_views() {
            return;
        }
        self.logger.value().trace(&format!("moveViewToLocation: view:{} location:{} reason:{}", view.id, location.value(), reason.unwrap_or("undefined")));
        let container = self.register_generated_view_container(location, None);
        self.move_views_to_container(&[view.clone()], &container, None, None);
    }

    fn move_views_to_container(&self, views: &[ViewDescriptorRc], view_container: &ViewContainerRc, visibility_state: Option<ViewVisibilityState>, reason: Option<&str>) {
        if views.is_empty() {
            return;
        }

        if !self.can_move_views() {
            return;
        }

        self.logger.value().trace(&format!(
            "moveViewsToContainer: views:{} viewContainer:{} reason:{}",
            views.iter().map(|view| view.id.as_str()).collect::<Vec<_>>().join(","),
            view_container.id,
            reason.unwrap_or("undefined")
        ));

        let from = self.get_view_container_by_view_id(&views[0].id);
        let to = view_container;

        if let Some(from) = from
            && !Rc::ptr_eq(&from, to)
        {
            // Move views
            self.move_views_without_saving(views, &from, to, visibility_state.unwrap_or(ViewVisibilityState::Expand));
            self.clean_up_generated_view_container(&from.id);

            // Save new locations
            self.save_view_customizations();

            // Log to telemetry
            self.report_moved_views(views, &from, to);
        }
    }

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        self.on_did_change_container_location.event()
    }

    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        self.on_did_change_container.event()
    }

    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        self.on_did_change_location.event()
    }

    fn reset(&self) {
        for view_container in self.view_containers() {
            let view_container_model = self.get_or_register_view_container_model(&view_container);

            for view_descriptor in view_container_model.all_view_descriptors() {
                let default_container = self.get_default_container_by_id(&view_descriptor.id);
                let current_container = self.get_view_container_by_view_id(&view_descriptor.id);
                if let (Some(current_container), Some(default_container)) = (current_container, default_container)
                    && !Rc::ptr_eq(&current_container, &default_container)
                {
                    self.move_views_without_saving(&[view_descriptor], &current_container, &default_container, ViewVisibilityState::Expand);
                }
            }

            let default_container_location = self.get_default_view_container_location(&view_container);
            let current_container_location = self.get_view_container_location(&view_container);
            if let Some(default_container_location) = default_container_location
                && current_container_location != Some(default_container_location)
            {
                self.move_view_container_to_location_without_saving(&view_container, default_container_location, None);
            }

            self.clean_up_generated_view_container(&view_container.id);
        }

        self.view_containers_custom_locations.borrow_mut().clear();
        self.view_descriptors_custom_locations.borrow_mut().clear();
        self.save_view_customizations();
    }

    fn is_view_container_removed_permanently(&self, view_container_id: &str) -> bool {
        self.is_generated_container_id(view_container_id) && !self.view_containers_custom_locations.borrow().contains_key(&view_container_id.to_string())
    }
}
