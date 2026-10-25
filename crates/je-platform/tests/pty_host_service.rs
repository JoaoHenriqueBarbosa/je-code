//! Porte de `src/vs/platform/terminal/test/node/ptyHostService.test.ts`.
//!
//! Suíte `PtyHostService`, o caso da suíte.
//!
//! Divergências:
//! - O canal falso (`IChannel`) e o cliente falso (`IChannelClient`) são structs: `call` devolve `[]` e `listen` devolve um
//!   evento que conta os ouvintes ativos por nome. O `IPtyHostStarter` falso devolve a conexão do caso.
//! - O `new TestConfigurationService()` é `TestConfigurationService::new(None)` e o `NullLoggerService` é
//!   `null_logger_service()`.
//! - O caso é `async` sem relógio virtual: roda em [`run_with_real_timers`]. O timer de batimento (20 s) do último pty host
//!   só some no descarte do serviço, que no mocha acontece no `teardown`; aqui o corpo descarta o serviço antes de conferir
//!   (o descarte repetido do `LeakGuard` é inofensivo), senão o laço esperaria o timer de verdade.
//! - O `[...listenerCounts.entries()].sort()` é a lista de pares ordenada pelo nome do evento.

mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::async_::timeout;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::{IChannel, IChannelClient, IServerChannel};
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LoggerService, NullLogService, null_logger_service};
use je_platform::terminal::local_pty_channel::create_local_pty_channel;
use je_platform::terminal::pty_host::{IPtyHostConnection, IPtyHostStarter};
use je_platform::terminal::pty_host_service::PtyHostService;
use je_platform::terminal::pty_service::IPtyHostController;

type ListenerCounts = Rc<RefCell<HashMap<String, i64>>>;

/// `makeEvent(name)`: o evento que conta os ouvintes ativos de `name`.
fn make_event(listener_counts: &ListenerCounts, name: &str) -> Event<Marshalled> {
    let (listener_counts, name) = (listener_counts.clone(), name.to_owned());
    Event::new(move |_listener| {
        *listener_counts.borrow_mut().entry(name.clone()).or_insert(0) += 1;
        let (listener_counts, name) = (listener_counts.clone(), name.clone());
        to_disposable(move || {
            *listener_counts.borrow_mut().entry(name).or_insert(0) -= 1;
        })
    })
}

struct MockChannel {
    listener_counts: ListenerCounts,
}

impl IChannel for MockChannel {
    fn call(&self, _command: &str, _arg: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
        Promise::resolved(Marshalled::Array(Vec::new()))
    }

    fn listen(&self, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        make_event(&self.listener_counts, event)
    }
}

struct MockChannelClient {
    channel: Rc<dyn IChannel>,
}

impl IChannelClient for MockChannelClient {
    fn get_channel(&self, _channel_name: &str) -> Rc<dyn IChannel> {
        self.channel.clone()
    }
}

struct MockPtyHostStarter {
    client: Rc<dyn IChannelClient>,
}

impl IPtyHostStarter for MockPtyHostStarter {
    fn start(&self) -> IPtyHostConnection {
        IPtyHostConnection { client: self.client.clone(), store: DisposableStore::new(), on_did_process_exit: Event::none() }
    }
}

impl IDisposable for MockPtyHostStarter {
    fn dispose(&self) {}
}

fn sorted(listener_counts: &ListenerCounts) -> Vec<(String, i64)> {
    let mut entries: Vec<(String, i64)> = listener_counts.borrow().iter().map(|(name, count)| (name.clone(), *count)).collect();
    entries.sort();
    entries
}

#[test]
fn restart_pty_host_disposes_listeners_registered_during_pty_host_startup() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    // Track active listener counts per event across pty host restarts. Without the
    // fix, each restart would leak the listeners registered in _startPtyHost.
    let listener_counts: ListenerCounts = Rc::default();

    let channel: Rc<dyn IChannel> = Rc::new(MockChannel { listener_counts: listener_counts.clone() });
    let client: Rc<dyn IChannelClient> = Rc::new(MockChannelClient { channel });
    let starter: Rc<dyn IPtyHostStarter> = Rc::new(MockPtyHostStarter { client });

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let logger_service = store.add_rc(null_logger_service());
    let service = store.add_rc(PtyHostService::new(starter, configuration_service, NullLogService::shared(), logger_service as Rc<dyn LoggerService>));
    let disposables = DisposableStore::new();
    store.add(disposables.clone());
    let local_channel = create_local_pty_channel(service.clone(), &disposables);
    let starts = Rc::new(Cell::new(0u32));

    run_with_real_timers({
        let (listener_counts, starts) = (listener_counts.clone(), starts.clone());
        let store = store.store();
        async move {
            // _startPtyHost runs lazily on first use, so trigger one restart to spin up the
            // initial host and capture the listener counts after a single startup as the baseline.
            service.restart_pty_host().await.unwrap();
            let baseline = listener_counts.borrow().clone();
            store.add(
                local_channel
                    .listen(&Marshalled::String("window".into()), "onPtyHostStart", Marshalled::Undefined)
                    .listen({
                        let starts = starts.clone();
                        move |_| starts.set(starts.get() + 1)
                    }),
            );
            timeout(0).await.unwrap();

            for _ in 0..5 {
                service.restart_pty_host().await.unwrap();
            }

            let actual = (sorted(&listener_counts), starts.get());
            let mut expected_listeners: Vec<(String, i64)> = baseline.into_iter().collect();
            expected_listeners.sort();
            service.dispose();

            assert_eq!(actual, (expected_listeners, 6), "restarts should notify the local channel without accumulating startup listeners");
        }
    });
}
