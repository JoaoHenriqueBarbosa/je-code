//! Porte de `src/vs/platform/markers/common/markerService.ts`.
//!
//! Divergências: `getStatistics` devolve uma cópia dos contadores (no original, o próprio objeto vivo, que as
//! atualizações seguintes alteram); o `MarkerStats` não é uma classe à parte, é o estado do serviço atualizado
//! pelo mesmo ouvinte de `onMarkerChanged`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::event::{Event, MicrotaskEmitter};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::uri::URI;

use super::markers::{IMarkerService, Marker, MarkerChangeOptions, MarkerInstance, MarkerData, MarkerReadOptions, MarkerSeverity, MarkerStatistics, ResourceMarker};

/// `unsupportedSchemas`.
pub fn unsupported_schemas() -> HashSet<&'static str> {
    HashSet::from([
        schemas::IN_MEMORY,
        schemas::VSCODE_SOURCE_CONTROL,
        schemas::WALK_THROUGH,
        schemas::WALK_THROUGH_SNIPPET,
        schemas::VSCODE_CHAT_CODE_BLOCK,
        schemas::VSCODE_TERMINAL,
    ])
}

/// `IMarkers`.
#[derive(Clone)]
struct Markers {
    markers: Vec<Marker>,
    is_truncated: bool,
}

type OwnerEntries<V> = Rc<RefCell<Vec<(String, V)>>>;

/// `DoubleResourceMap<V>`: por recurso e por dono, cada um na ordem de inserção.
struct DoubleResourceMap<V: Clone> {
    by_resource: ResourceMap<OwnerEntries<V>>,
    by_owner: Vec<(String, ResourceMap<V>)>,
}

impl<V: Clone> DoubleResourceMap<V> {
    fn new() -> Self {
        Self { by_resource: ResourceMap::new(), by_owner: Vec::new() }
    }

    fn set(&mut self, resource: &URI, owner: &str, value: V) {
        let owner_entries = match self.by_resource.get(resource) {
            Some(entries) => entries.clone(),
            None => {
                let entries: OwnerEntries<V> = Rc::new(RefCell::new(Vec::new()));
                self.by_resource.set(resource.clone(), entries.clone());
                entries
            }
        };
        {
            let mut entries = owner_entries.borrow_mut();
            match entries.iter_mut().find(|(o, _)| o == owner) {
                Some(entry) => entry.1 = value.clone(),
                None => entries.push((owner.to_string(), value.clone())),
            }
        }

        let position = match self.by_owner.iter().position(|(o, _)| o == owner) {
            Some(position) => position,
            None => {
                self.by_owner.push((owner.to_string(), ResourceMap::new()));
                self.by_owner.len() - 1
            }
        };
        self.by_owner[position].1.set(resource.clone(), value);
    }

    fn get(&self, resource: &URI, owner: &str) -> Option<V> {
        let entries = self.by_resource.get(resource)?;
        let entries = entries.borrow();
        entries.iter().find(|(o, _)| o == owner).map(|(_, v)| v.clone())
    }

    fn delete(&mut self, resource: &URI, owner: &str) -> bool {
        let mut removed_a = false;
        if let Some(entries) = self.by_resource.get(resource) {
            let mut entries = entries.borrow_mut();
            if let Some(position) = entries.iter().position(|(o, _)| o == owner) {
                entries.remove(position);
                removed_a = true;
            }
        }
        let mut removed_b = false;
        if let Some((_, resource_map)) = self.by_owner.iter_mut().find(|(o, _)| o == owner) {
            removed_b = resource_map.delete(resource);
        }
        if removed_a != removed_b {
            je_base::errors::throw(je_base::errors::Error::new("illegal state"));
        }
        removed_a && removed_b
    }

    fn values_of_owner(&self, owner: &str) -> Vec<V> {
        self.by_owner.iter().find(|(o, _)| o == owner).map(|(_, map)| map.values().cloned().collect()).unwrap_or_default()
    }

    fn values_of_resource(&self, resource: &URI) -> Vec<V> {
        self.by_resource.get(resource).map(|entries| entries.borrow().iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default()
    }

    fn values_all(&self) -> Vec<V> {
        self.by_owner.iter().flat_map(|(_, map)| map.values().cloned().collect::<Vec<_>>()).collect()
    }
}

struct State {
    data: DoubleResourceMap<Markers>,
    filtered_resources: ResourceMap<Vec<String>>,
    stats: MarkerStatistics,
    stats_data: ResourceMap<MarkerStatistics>,
}

/// `MarkerService`.
pub struct MarkerService {
    on_marker_changed: Rc<MicrotaskEmitter<Vec<URI>>>,
    state: Rc<RefCell<State>>,
    subscription: RefCell<Option<DisposableRef>>,
}

impl MarkerService {
    pub fn new() -> Rc<Self> {
        let merge: Rc<dyn Fn(Vec<Vec<URI>>) -> Vec<URI>> = Rc::new(Self::merge);
        let on_marker_changed = MicrotaskEmitter::with_options(Default::default(), Some(merge));
        let state = Rc::new(RefCell::new(State {
            data: DoubleResourceMap::new(),
            filtered_resources: ResourceMap::new(),
            stats: MarkerStatistics::default(),
            stats_data: ResourceMap::new(),
        }));
        let this = Rc::new(Self { on_marker_changed, state, subscription: RefCell::new(None) });

        // `MarkerStats`: acompanha os contadores a cada mudança.
        let weak = Rc::downgrade(&this);
        let subscription = this.on_marker_changed.event().listen(move |resources| {
            if let Some(service) = weak.upgrade() {
                service.update_stats(resources);
            }
        });
        *this.subscription.borrow_mut() = Some(subscription);
        this
    }

    fn update_stats(&self, resources: &[URI]) {
        let unsupported = unsupported_schemas();
        for resource in resources {
            let new_stats = {
                let mut result = MarkerStatistics::default();
                // TODO this is a hack
                if !unsupported.contains(resource.scheme().to_string_lossy().as_str()) {
                    for marker in self.read(Some(&MarkerReadOptions { resource: Some(resource.clone()), ..Default::default() })) {
                        match marker.severity {
                            MarkerSeverity::ERROR => result.errors += 1,
                            MarkerSeverity::WARNING => result.warnings += 1,
                            MarkerSeverity::INFO => result.infos += 1,
                            _ => result.unknowns += 1,
                        }
                    }
                }
                result
            };
            let mut state = self.state.borrow_mut();
            if let Some(old) = state.stats_data.get(resource).copied() {
                state.stats.errors -= old.errors;
                state.stats.warnings -= old.warnings;
                state.stats.infos -= old.infos;
                state.stats.unknowns -= old.unknowns;
            }
            state.stats.errors += new_stats.errors;
            state.stats.warnings += new_stats.warnings;
            state.stats.infos += new_stats.infos;
            state.stats.unknowns += new_stats.unknowns;
            state.stats_data.set(resource.clone(), new_stats);
        }
    }

    fn to_marker(owner: &str, resource: &URI, data: &MarkerData) -> Option<Marker> {
        if data.message.is_empty() {
            return None;
        }
        // santize data
        let start_line_number = if data.start_line_number > 0 { data.start_line_number } else { 1 };
        let start_column = if data.start_column > 0 { data.start_column } else { 1 };
        let end_line_number = if data.end_line_number >= start_line_number { data.end_line_number } else { start_line_number };
        let end_column = if data.end_column > 0 { data.end_column } else { start_column };
        Some(Marker {
            instance: MarkerInstance::new(),
            resource: resource.clone(),
            owner: owner.to_string(),
            code: data.code.clone(),
            severity: data.severity,
            message: data.message.clone(),
            source: data.source.clone(),
            start_line_number,
            start_column,
            end_line_number,
            end_column,
            related_information: data.related_information.clone(),
            model_version_id: data.model_version_id,
            tags: data.tags.clone(),
            origin: data.origin.clone(),
        })
    }

    /// Cria um marcador de informação para um recurso filtrado.
    fn create_filtered_marker(resource: &URI, reasons: &[String]) -> Marker {
        let message = if reasons.len() == 1 {
            localize("filtered", "Problems are paused because: \"{0}\"", &[&reasons[0]])
        } else {
            localize("filtered.network", "Problems are paused because: \"{0}\" and {1} more", &[&reasons[0], &(reasons.len() - 1).to_string()])
        };
        Marker {
            instance: MarkerInstance::new(),
            owner: "markersFilter".to_string(),
            resource: resource.clone(),
            severity: MarkerSeverity::INFO,
            code: None,
            message,
            source: None,
            start_line_number: 1,
            start_column: 1,
            end_line_number: 1,
            end_column: 1,
            model_version_id: None,
            related_information: None,
            tags: None,
            origin: None,
        }
    }

    fn accept(marker: &Marker, severities: Option<u32>) -> bool {
        severities.is_none_or(|s| (s & marker.severity.0) == marker.severity.0)
    }

    /// `_merge`: a lista sem repetição de todos os recursos, na ordem de chegada.
    fn merge(all: Vec<Vec<URI>>) -> Vec<URI> {
        let mut set: ResourceMap<bool> = ResourceMap::new();
        for array in all {
            for item in array {
                set.set(item, true);
            }
        }
        set.keys().cloned().collect()
    }

    fn reasons_for(&self, resource: &URI, ignore_resource_filters: bool) -> Option<Vec<String>> {
        if ignore_resource_filters {
            return None;
        }
        self.state.borrow().filtered_resources.get(resource).filter(|r| !r.is_empty()).cloned()
    }
}

impl IDisposable for MarkerService {
    fn dispose(&self) {
        if let Some(subscription) = self.subscription.borrow_mut().take() {
            subscription.dispose();
        }
        self.on_marker_changed.dispose();
    }
}

impl IMarkerService for MarkerService {
    fn get_statistics(&self) -> MarkerStatistics {
        self.state.borrow().stats
    }

    fn is_source_truncated(&self, resource: &URI) -> bool {
        self.state.borrow().data.values_of_resource(resource).iter().any(|markers| markers.is_truncated)
    }

    fn remove(&self, owner: &str, resources: &[URI]) {
        for resource in resources {
            self.change_one(owner, resource, Vec::new(), None);
        }
    }

    fn change_one(&self, owner: &str, resource: &URI, marker_data: Vec<MarkerData>, options: Option<MarkerChangeOptions>) {
        if marker_data.is_empty() {
            // remove marker for this (owner,resource)-tuple
            let removed = self.state.borrow_mut().data.delete(resource, owner);
            if removed {
                self.on_marker_changed.fire(vec![resource.clone()]);
            }
        } else {
            // insert marker for this (owner,resource)-tuple
            let markers: Vec<Marker> = marker_data.iter().filter_map(|data| Self::to_marker(owner, resource, data)).collect();
            self.state.borrow_mut().data.set(resource, owner, Markers { markers, is_truncated: options.is_some_and(|o| o.is_truncated) });
            self.on_marker_changed.fire(vec![resource.clone()]);
        }
    }

    fn install_resource_filter(&self, resource: &URI, reason: &str) -> DisposableRef {
        {
            let mut state = self.state.borrow_mut();
            if state.filtered_resources.get(resource).is_none() {
                state.filtered_resources.set(resource.clone(), Vec::new());
            }
            let mut reasons = state.filtered_resources.get(resource).cloned().unwrap_or_default();
            reasons.push(reason.to_string());
            state.filtered_resources.set(resource.clone(), reasons);
        }
        self.on_marker_changed.fire(vec![resource.clone()]);

        let state = self.state.clone();
        let emitter = self.on_marker_changed.clone();
        let resource = resource.clone();
        let reason = reason.to_string();
        to_disposable(move || {
            let changed = {
                let mut state = state.borrow_mut();
                let Some(mut reasons) = state.filtered_resources.get(&resource).cloned() else { return };
                match reasons.iter().position(|r| *r == reason) {
                    Some(index) => {
                        reasons.remove(index);
                        if reasons.is_empty() {
                            state.filtered_resources.delete(&resource);
                        } else {
                            state.filtered_resources.set(resource.clone(), reasons);
                        }
                        true
                    }
                    None => false,
                }
            };
            if changed {
                emitter.fire(vec![resource.clone()]);
            }
        })
    }

    fn change_all(&self, owner: &str, data: Vec<ResourceMarker>) {
        let mut changes: Vec<URI> = Vec::new();

        // remove old marker
        let existing = self.state.borrow().data.values_of_owner(owner);
        for entry in existing {
            if let Some(first) = entry.markers.first() {
                changes.push(first.resource.clone());
                self.state.borrow_mut().data.delete(&first.resource, owner);
            }
        }

        // add new markers
        if !data.is_empty() {
            // group by resource
            let mut groups: ResourceMap<Vec<Marker>> = ResourceMap::new();
            for ResourceMarker { resource, marker: marker_data } in &data {
                let Some(marker) = Self::to_marker(owner, resource, marker_data) else {
                    // filter bad markers
                    continue;
                };
                match groups.get(resource) {
                    None => {
                        groups.set(resource.clone(), vec![marker]);
                        changes.push(resource.clone());
                    }
                    Some(array) => {
                        let mut array = array.clone();
                        array.push(marker);
                        groups.set(resource.clone(), array);
                    }
                }
            }

            // insert all
            for (resource, value) in groups.entries() {
                self.state.borrow_mut().data.set(resource, owner, Markers { markers: value.clone(), is_truncated: false });
            }
        }

        if !changes.is_empty() {
            self.on_marker_changed.fire(changes);
        }
    }

    fn read(&self, filter: Option<&MarkerReadOptions>) -> Vec<Marker> {
        let default_filter = MarkerReadOptions::default();
        let filter = filter.unwrap_or(&default_filter);
        let take = filter.take.filter(|t| *t > 0).unwrap_or(-1);
        let owner = filter.owner.as_deref().filter(|o| !o.is_empty());

        if let (Some(owner), Some(resource)) = (owner, filter.resource.as_ref()) {
            // exactly one owner AND resource
            if let Some(reasons) = self.reasons_for(resource, filter.ignore_resource_filters) {
                return vec![Self::create_filtered_marker(resource, &reasons)];
            }
            let Some(data) = self.state.borrow().data.get(resource, owner) else {
                return Vec::new();
            };
            let mut result: Vec<Marker> = Vec::new();
            for marker in &data.markers {
                if take > 0 && result.len() as i64 == take {
                    break;
                }
                if let Some(reasons) = self.reasons_for(resource, filter.ignore_resource_filters) {
                    result.push(Self::create_filtered_marker(resource, &reasons));
                } else if Self::accept(marker, filter.severities) {
                    result.push(marker.clone());
                }
            }
            result
        } else {
            // of one resource OR owner
            let iterable: Vec<Markers> = {
                let state = self.state.borrow();
                match (owner, filter.resource.as_ref()) {
                    (None, None) => state.data.values_all(),
                    (_, Some(resource)) => state.data.values_of_resource(resource),
                    (Some(owner), None) => state.data.values_of_owner(owner),
                }
            };
            let mut result: Vec<Marker> = Vec::new();
            let mut filtered: ResourceMap<()> = ResourceMap::new();
            for markers in &iterable {
                for data in &markers.markers {
                    if filtered.has(&data.resource) {
                        continue;
                    }
                    if take > 0 && result.len() as i64 == take {
                        break;
                    }
                    if let Some(reasons) = self.reasons_for(&data.resource, filter.ignore_resource_filters) {
                        result.push(Self::create_filtered_marker(&data.resource, &reasons));
                        filtered.set(data.resource.clone(), ());
                    } else if Self::accept(data, filter.severities) {
                        result.push(data.clone());
                    }
                }
            }
            result
        }
    }

    fn on_marker_changed(&self) -> Event<Vec<URI>> {
        self.on_marker_changed.event()
    }
}
