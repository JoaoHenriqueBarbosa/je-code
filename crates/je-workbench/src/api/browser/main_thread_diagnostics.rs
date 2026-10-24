//! Porte de `src/vs/workbench/api/browser/mainThreadDiagnostics.ts`: a ponte entre o `IMarkerService` do workbench e as
//! coleções de diagnósticos que as extensões publicam no host.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadDiagnostics)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostDiagnostics`.
//! - `static ExtHostCounter` é um contador atômico do módulo.
//! - `URI.revive` (de `relatedInformation.resource`, de `code.target` e do `uri`) não tem o que fazer: o DTO já carrega
//!   [`URI`]. O `markers ?? []` é o `unwrap_or_default` do `Option<Vec<MarkerData>>`.
//! - `_activeOwners` é um `Set<string>` que se percorre na ordem de inserção: aqui é um `Vec<String>` sem repetição.
//! - `dispose` converte cada [`Marker`] lido (que carrega `owner` e `resource`) no [`MarkerData`] que `changeOne` recebe.
//! - O `ResourceMap<IMarker[]>` do `dispose` é o `ResourceMap` do `je_base::map`.
//! - O ouvinte de `onMarkerChanged` guarda uma referência fraca ao objeto (o `this` do original).

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::map::ResourceMap;
use je_base::uri::URI;
use je_platform::markers::markers::{IMarkerService, Marker, MarkerChangeOptions, MarkerData, MarkerReadOptions};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostDiagnosticsShape, IMainContext, IMarkerChange, MainThreadDiagnosticsShape};

/// `MainThreadDiagnostics.ExtHostCounter`.
static EXT_HOST_COUNTER: AtomicU32 = AtomicU32::new(1);

/// O `IMarker` sem o `owner` e o `resource`, que é o que `changeOne` recebe de volta.
fn marker_to_data(marker: Marker) -> MarkerData {
    MarkerData {
        code: marker.code,
        severity: marker.severity,
        message: marker.message,
        source: marker.source,
        start_line_number: marker.start_line_number,
        start_column: marker.start_column,
        end_line_number: marker.end_line_number,
        end_column: marker.end_column,
        model_version_id: marker.model_version_id,
        related_information: marker.related_information,
        tags: marker.tags,
        origin: marker.origin,
    }
}

/// `MainThreadDiagnostics`.
pub struct MainThreadDiagnostics {
    active_owners: RefCell<Vec<String>>,
    proxy: Rc<dyn ExtHostDiagnosticsShape>,
    marker_service: Rc<dyn IMarkerService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    marker_listener: DisposableRef,
    ext_host_id: String,
}

impl MainThreadDiagnostics {
    pub fn new(ext_host_context: &dyn IMainContext, marker_service: Rc<dyn IMarkerService>, uri_identity_service: Rc<dyn IUriIdentityService>) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_DIAGNOSTICS);
        let ext_host_id = format!("extHost{}", EXT_HOST_COUNTER.fetch_add(1, Ordering::SeqCst));
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            let marker_listener = marker_service.on_marker_changed().listen(move |resources| {
                if let Some(this) = weak.upgrade() {
                    this.forward_markers(resources);
                }
            });
            Self { active_owners: RefCell::new(Vec::new()), proxy, marker_service, uri_identity_service, marker_listener, ext_host_id }
        })
    }

    /// `_forwardMarkers`.
    fn forward_markers(&self, resources: &[URI]) {
        let mut data: Vec<(URI, Vec<MarkerData>)> = Vec::new();
        for resource in resources {
            let all_marker_data =
                self.marker_service.read(Some(&MarkerReadOptions { resource: Some(resource.clone()), ignore_resource_filters: true, ..Default::default() }));
            let marker_data: Vec<MarkerData> =
                all_marker_data.into_iter().filter(|marker| marker.origin.as_deref() != Some(self.ext_host_id.as_str())).map(marker_to_data).collect();
            data.push((resource.clone(), marker_data));
        }
        if !data.is_empty() {
            self.proxy.rpc_accept_markers_change(data);
        }
    }
}

impl IDisposable for MainThreadDiagnostics {
    fn dispose(&self) {
        self.marker_listener.dispose();
        let owners: Vec<String> = std::mem::take(&mut *self.active_owners.borrow_mut());
        for owner in owners {
            let mut markers_data: ResourceMap<Vec<MarkerData>> = ResourceMap::new();
            let markers = self.marker_service.read(Some(&MarkerReadOptions { owner: Some(owner.clone()), ..Default::default() }));
            for marker in markers {
                let resource = marker.resource.clone();
                if !markers_data.has(&resource) {
                    markers_data.set(resource.clone(), Vec::new());
                }
                let is_own = marker.origin.as_deref() == Some(self.ext_host_id.as_str());
                if !is_own {
                    let mut current = markers_data.get(&resource).cloned().unwrap_or_default();
                    current.push(marker_to_data(marker));
                    markers_data.set(resource, current);
                }
            }
            let entries: Vec<(URI, Vec<MarkerData>)> = markers_data.entries().map(|(resource, local)| (resource.clone(), local.clone())).collect();
            for (resource, local) in entries {
                self.marker_service.change_one(&owner, &resource, local, None);
            }
        }
    }
}

impl MainThreadDiagnosticsShape for MainThreadDiagnostics {
    fn rpc_change_many(&self, owner: &str, entries: Vec<IMarkerChange>) {
        for entry in entries {
            let IMarkerChange { resource, markers, is_truncated } = entry;
            let markers = markers.map(|markers| {
                markers
                    .into_iter()
                    .map(|mut marker| {
                        if marker.origin.is_none() {
                            marker.origin = Some(self.ext_host_id.clone());
                        }
                        marker
                    })
                    .collect::<Vec<_>>()
            });
            self.marker_service.change_one(
                owner,
                &self.uri_identity_service.as_canonical_uri(&resource),
                markers.unwrap_or_default(),
                Some(MarkerChangeOptions { is_truncated: is_truncated.unwrap_or(false) }),
            );
        }
        let mut active_owners = self.active_owners.borrow_mut();
        if !active_owners.iter().any(|active| active == owner) {
            active_owners.push(owner.to_owned());
        }
    }

    fn rpc_clear(&self, owner: &str) {
        self.marker_service.change_all(owner, Vec::new());
        self.active_owners.borrow_mut().retain(|active| active != owner);
    }
}
