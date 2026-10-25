//! Porte de `src/vs/workbench/browser/parts/visibleViewContainersTracker.ts`: o [`VisibleViewContainersTracker`], que conta os
//! contêineres de visões visíveis de uma localização (os que têm visões ativas) e avisa quando a contagem muda.
//!
//! Divergências:
//! - **`createInstance(VisibleViewContainersTracker, location)`** é [`VisibleViewContainersTracker::new`], que recebe o
//!   `IViewDescriptorService` que a injeção entregaria.
//! - **`DisposableMap<string>`** é o `DisposableMap<String, dyn IDisposable>` de `je-base`.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable};

use crate::common::views::{IViewDescriptorService, ViewContainerLocation};

/// O `{ before: number; after: number }` de `onDidChange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleCountChange {
    pub before: usize,
    pub after: usize,
}

/// Tracks the number of visible view containers at a given location. A view container is considered visible if it has active
/// views (`activeViewDescriptors.length > 0`). Fires an event when the number of visible containers changes.
pub struct VisibleViewContainersTracker {
    base: Rc<Disposable>,
    location: ViewContainerLocation,
    view_descriptor_service: Rc<dyn IViewDescriptorService>,
    view_container_model_listeners: Rc<DisposableMap<String, dyn IDisposable>>,
    on_did_change: Rc<Emitter<VisibleCountChange>>,
    visible_count: Cell<usize>,
    weak_self: Weak<VisibleViewContainersTracker>,
}

impl VisibleViewContainersTracker {
    pub fn new(location: ViewContainerLocation, view_descriptor_service: Rc<dyn IViewDescriptorService>) -> Rc<Self> {
        let base = Disposable::new();
        let view_container_model_listeners: Rc<DisposableMap<String, dyn IDisposable>> = DisposableMap::new();
        base.register(view_container_model_listeners.clone());
        let on_did_change: Rc<Emitter<VisibleCountChange>> = Emitter::new();
        base.register(on_did_change.clone());

        let this = Rc::new_cyclic(|weak| Self {
            base,
            location,
            view_descriptor_service,
            view_container_model_listeners,
            on_did_change,
            visible_count: Cell::new(0),
            weak_self: weak.clone(),
        });

        this.register_listeners();
        this.initialize_view_container_listeners();
        this.update_visible_count();
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<VisibleCountChange> {
        self.on_did_change.event()
    }

    /// Returns the current number of visible view containers at this location.
    pub fn visible_count(&self) -> usize {
        self.visible_count.get()
    }

    fn register_listeners(&self) {
        // Track view container additions/removals
        let weak = self.weak_self.clone();
        self.base.register(self.view_descriptor_service.on_did_change_view_containers().listen(move |change| {
            let Some(this) = weak.upgrade() else { return };

            // Add listeners for new view containers
            for added in &change.added {
                if added.location == this.location {
                    this.add_view_container_model_listener(&added.container.id);
                }
            }
            // Remove listeners for removed view containers
            for removed in &change.removed {
                if removed.location == this.location {
                    this.view_container_model_listeners.delete_and_dispose(&removed.container.id);
                }
            }

            let relevant_change = change.added.iter().chain(change.removed.iter()).any(|entry| entry.location == this.location);
            if relevant_change {
                this.update_visible_count();
            }
        }));

        // Track container location changes
        let weak = self.weak_self.clone();
        self.base.register(self.view_descriptor_service.on_did_change_container_location().listen(move |change| {
            let Some(this) = weak.upgrade() else { return };

            // Update listeners when container moves
            if change.from == this.location {
                this.view_container_model_listeners.delete_and_dispose(&change.view_container.id);
            }
            if change.to == this.location {
                this.add_view_container_model_listener(&change.view_container.id);
            }

            if change.from == this.location || change.to == this.location {
                this.update_visible_count();
            }
        }));
    }

    fn initialize_view_container_listeners(&self) {
        // Initialize listeners for existing view containers
        for container in self.view_descriptor_service.get_view_containers_by_location(self.location) {
            self.add_view_container_model_listener(&container.id);
        }
    }

    fn add_view_container_model_listener(&self, container_id: &str) {
        if let Some(container) = self.view_descriptor_service.get_view_container_by_id(container_id) {
            let model = self.view_descriptor_service.get_view_container_model(&container);
            let weak = self.weak_self.clone();
            let listener = model.on_did_change_active_view_descriptors().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_visible_count();
                }
            });
            self.view_container_model_listeners.set(container_id.to_owned(), listener, false);
        }
    }

    fn update_visible_count(&self) {
        let view_containers = self.view_descriptor_service.get_view_containers_by_location(self.location);
        let new_count = view_containers
            .iter()
            .filter(|container| !self.view_descriptor_service.get_view_container_model(container).active_view_descriptors().is_empty())
            .count();

        let before = self.visible_count.get();
        if before != new_count {
            self.visible_count.set(new_count);
            self.on_did_change.fire(VisibleCountChange { before, after: new_count });
        }
    }
}

impl IDisposable for VisibleViewContainersTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}
