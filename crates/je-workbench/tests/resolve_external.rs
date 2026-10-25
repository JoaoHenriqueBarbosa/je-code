//! Porte de `src/vs/workbench/test/electron-browser/resolveExternal.test.ts`.
//!
//! Notas de tradução:
//! - A suíte é `suite.skip` no original; os oito casos viram `#[ignore]` com o corpo mantido, como o resto do porte faz com
//!   `test.skip`.
//! - `TestNativeWindow` (que anula `create`, `registerListeners` e `enableMultiWindowAwareTimeout`) é o
//!   [`NativeWindowUriResolver`], a fatia da `NativeWindow` que resolve URIs externas e que não faz nada ao nascer.
//! - `TunnelMock implements Partial<ITunnelService>` é o `TunnelMock` abaixo, sobre o [`ITunnelServiceApi`] real. O `Partial`
//!   vira os membros que nenhum caso chama falhando com "is not a function", como o objeto parcial do original faria.
//!   O estado (`assignedPorts`, `expectedDispose`) fica em células compartilhadas porque o `dispose` do túnel o lê depois que
//!   `openTunnel` voltou.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não entra: o `dispose` do resultado é um objeto literal no original, que o
//!   rastreador de vazamentos não vê; aqui o resultado nunca é descartado pelo teste, como no original.
//! - `workbenchInstantiationService` é substituído pelos três serviços que o caminho dos casos lê (arquivos, produto e
//!   resolvedor de autoridade remota); nenhum caso chega neles, porque todos passam `openExternal: true` e não há autoridade
//!   remota. O serviço de arquivos é o `FileService` real (sem provedores) e o duplo de autoridade falha se for chamado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::run_with_real_timers;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::file_service::FileService;
use je_platform::log::NullLogService;
use je_platform::opener::opener::OpenOptions;
use je_platform::product::ProductService;
use je_platform::remote::remote_agent_connection::IAddressProvider;
use je_platform::remote::remote_authority_resolver::{IRemoteAuthorityResolverService, ResolverResult, TunnelPrivacy};
use je_platform::tunnel::{ITunnelProvider, ITunnelService, ITunnelServiceApi, RemoteTunnel, TunnelClosedEvent, TunnelOpenResult, TunnelProviderFeatures};
use je_workbench::electron_browser::window::{NativeWindowUriResolver, NativeWindowUriResolverServices};

type PortMap = HashMap<u32, u32>;

/// O objeto literal que o `openTunnel` do `TunnelMock` devolve no original.
struct MockTunnel {
    local_address: String,
    tunnel_remote_port: u32,
    expected_dispose: Rc<Cell<bool>>,
}

impl RemoteTunnel for MockTunnel {
    fn tunnel_remote_port(&self) -> u32 {
        self.tunnel_remote_port
    }

    fn tunnel_remote_host(&self) -> String {
        "4.3.2.1".to_owned()
    }

    fn tunnel_local_port(&self) -> Option<u32> {
        Some(self.tunnel_remote_port)
    }

    fn local_address(&self) -> String {
        self.local_address.clone()
    }

    fn privacy(&self) -> String {
        String::new()
    }

    fn protocol(&self) -> Option<String> {
        None
    }

    fn dispose(&self, _silent: bool) -> Promise<()> {
        assert!(self.expected_dispose.get(), "Unexpected dispose");
        self.expected_dispose.set(false);
        Promise::resolved(())
    }
}

struct TunnelMock {
    assigned_ports: RefCell<PortMap>,
    expected_dispose: Rc<Cell<bool>>,
}

impl TunnelMock {
    fn new() -> Rc<Self> {
        Rc::new(Self { assigned_ports: RefCell::new(PortMap::new()), expected_dispose: Rc::new(Cell::new(false)) })
    }

    fn reset(&self, ports: PortMap) {
        *self.assigned_ports.borrow_mut() = ports;
    }

    fn expect_dispose(&self) {
        self.expected_dispose.set(true);
    }

    fn validate(&self) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert!(self.assigned_ports.borrow().is_empty(), "Expected tunnel to be used");
            assert!(!self.expected_dispose.get(), "Expected dispose to be called");
        }));
        self.expected_dispose.set(false);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

impl ITunnelService for TunnelMock {
    fn can_tunnel(&self, _uri: &URI) -> bool {
        true
    }
}

impl ITunnelServiceApi for TunnelMock {
    fn tunnels(&self) -> Promise<Vec<Rc<dyn RemoteTunnel>>> {
        panic!("tunnels is not a function");
    }

    fn can_change_privacy(&self) -> bool {
        panic!("canChangePrivacy is not a function");
    }

    fn privacy_options(&self) -> Vec<TunnelPrivacy> {
        panic!("privacyOptions is not a function");
    }

    fn on_tunnel_opened(&self) -> Event<Rc<dyn RemoteTunnel>> {
        panic!("onTunnelOpened is not a function");
    }

    fn on_tunnel_closed(&self) -> Event<TunnelClosedEvent> {
        panic!("onTunnelClosed is not a function");
    }

    fn can_elevate(&self) -> bool {
        panic!("canElevate is not a function");
    }

    fn can_change_protocol(&self) -> bool {
        panic!("canChangeProtocol is not a function");
    }

    fn has_tunnel_provider(&self) -> bool {
        panic!("hasTunnelProvider is not a function");
    }

    fn on_added_tunnel_provider(&self) -> Event<()> {
        panic!("onAddedTunnelProvider is not a function");
    }

    fn open_tunnel(
        &self,
        _address_provider: Option<Rc<dyn IAddressProvider>>,
        _remote_host: Option<&str>,
        remote_port: u32,
        _local_host: Option<&str>,
        _local_port: Option<u32>,
        _elevate_if_needed: bool,
        _privacy: Option<&str>,
        _protocol: Option<&str>,
    ) -> Option<Promise<TunnelOpenResult>> {
        let assigned = self.assigned_ports.borrow().get(&remote_port).copied().unwrap_or(0);
        if assigned == 0 {
            return Some(Promise::rejected(Error::new("Unexpected tunnel request")));
        }
        let tunnel: Rc<dyn RemoteTunnel> =
            Rc::new(MockTunnel { local_address: format!("localhost:{assigned}"), tunnel_remote_port: assigned, expected_dispose: self.expected_dispose.clone() });
        self.assigned_ports.borrow_mut().remove(&remote_port);
        Some(Promise::resolved(TunnelOpenResult::Tunnel(tunnel)))
    }

    fn get_existing_tunnel(&self, _remote_host: &str, _remote_port: u32) -> Promise<TunnelOpenResult> {
        Promise::resolved(TunnelOpenResult::Undefined)
    }

    fn set_environment_tunnel(&self, _remote_host: &str, _remote_port: u32, _local_address: &str, _privacy: &str, _protocol: &str) {
        panic!("setEnvironmentTunnel is not a function");
    }

    fn close_tunnel(&self, _remote_host: &str, _remote_port: u32) -> Promise<()> {
        panic!("closeTunnel is not a function");
    }

    fn set_tunnel_provider(&self, _provider: Option<Rc<dyn ITunnelProvider>>) -> DisposableRef {
        panic!("setTunnelProvider is not a function");
    }

    fn set_tunnel_features(&self, _features: &TunnelProviderFeatures) {
        panic!("setTunnelFeatures is not a function");
    }

    fn is_port_privileged(&self, _port: u32) -> bool {
        panic!("isPortPrivileged is not a function");
    }
}

/// O `IRemoteAuthorityResolverService` que nenhum caso consulta (não há autoridade remota).
struct UnusedRemoteAuthorityResolver;

impl IRemoteAuthorityResolverService for UnusedRemoteAuthorityResolver {
    fn resolve_authority(&self, _authority: &str) -> Promise<ResolverResult> {
        panic!("resolveAuthority is not a function");
    }

    fn get_canonical_uri(&self, _uri: &URI) -> Promise<URI> {
        panic!("getCanonicalURI is not a function");
    }
}

struct Fixture {
    tunnel_mock: Rc<TunnelMock>,
    window: Rc<NativeWindowUriResolver>,
}

/// O `setup` do `suite`.
fn setup() -> Fixture {
    let tunnel_mock = TunnelMock::new();
    let window = NativeWindowUriResolver::new(NativeWindowUriResolverServices {
        tunnel_service: tunnel_mock.clone(),
        file_service: FileService::new(NullLogService::shared()),
        product_service: Rc::new(ProductService::default()),
        remote_authority_resolver_service: Rc::new(UnusedRemoteAuthorityResolver),
        remote_authority: None,
    });
    Fixture { tunnel_mock, window }
}

async fn do_test(fixture: &Fixture, uri: &str, ports: PortMap, expected_uri: Option<&str>) {
    fixture.tunnel_mock.reset(ports);
    let options = OpenOptions { allow_tunneling: Some(true), open_external: Some(true), ..OpenOptions::default() };
    let res = fixture.window.resolve_external_uri(&URI::parse(uri, false), Some(&options)).await.unwrap();
    assert_eq!(expected_uri.is_none(), res.is_none(), "Expected URI {expected_uri:?} but got {:?}", res.as_ref().map(|res| res.resolved.to_string(false).to_string_lossy()));
    if let (Some(expected_uri), Some(res)) = (expected_uri, res) {
        assert_eq!(res.resolved.to_string(false).to_string_lossy(), URI::parse(expected_uri, false).to_string(false).to_string_lossy());
    }
    fixture.tunnel_mock.validate();
}

fn ports(entries: &[(u32, u32)]) -> PortMap {
    entries.iter().copied().collect()
}

#[test]
#[ignore = "suite.skip no original"]
fn invalid() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(&fixture, "file:///foo.bar/baz", PortMap::new(), None).await;
        do_test(&fixture, "http://foo.bar/path", PortMap::new(), None).await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn simple() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(&fixture, "http://localhost:1234/path", ports(&[(1234, 1234)]), Some("http://localhost:1234/path")).await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn all_interfaces() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(&fixture, "http://0.0.0.0:1234/path", ports(&[(1234, 1234)]), Some("http://localhost:1234/path")).await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn changed_port() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(&fixture, "http://localhost:1234/path", ports(&[(1234, 1235)]), Some("http://localhost:1235/path")).await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn query() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(
            &fixture,
            "http://foo.bar/path?a=b&c=http%3a%2f%2flocalhost%3a4455",
            ports(&[(4455, 4455)]),
            Some("http://foo.bar/path?a=b&c=http%3a%2f%2flocalhost%3a4455"),
        )
        .await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn query_with_different_port() {
    run_with_real_timers(async {
        let fixture = setup();
        fixture.tunnel_mock.expect_dispose();
        do_test(&fixture, "http://foo.bar/path?a=b&c=http%3a%2f%2flocalhost%3a4455", ports(&[(4455, 4567)]), None).await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn both_url_and_query() {
    run_with_real_timers(async {
        let fixture = setup();
        do_test(
            &fixture,
            "http://localhost:1234/path?a=b&c=http%3a%2f%2flocalhost%3a4455",
            ports(&[(1234, 4321), (4455, 4455)]),
            Some("http://localhost:4321/path?a=b&c=http%3a%2f%2flocalhost%3a4455"),
        )
        .await;
    });
}

#[test]
#[ignore = "suite.skip no original"]
fn both_url_and_query_query_rejected() {
    run_with_real_timers(async {
        let fixture = setup();
        fixture.tunnel_mock.expect_dispose();
        do_test(
            &fixture,
            "http://localhost:1234/path?a=b&c=http%3a%2f%2flocalhost%3a4455",
            ports(&[(1234, 4321), (4455, 5544)]),
            Some("http://localhost:4321/path?a=b&c=http%3a%2f%2flocalhost%3a4455"),
        )
        .await;
    });
}
