//! Porte de `src/vs/platform/terminal/test/node/autoRepliesContribController.test.ts`.
//!
//! Suíte `AutoRepliesPtyServiceContribution`, o caso da suíte.
//!
//! Divergências: `mock<ITerminalChildProcess>()` é uma struct que implementa o contrato inteiro com os membros que o
//! caso define (`onProcessData` e `input`) e o resto falhando se for usado, como o `mock` do original. O
//! `runWithFakedTimers({ useFakeTimers: true }, async () => ...)` é [`run_with_faked_timers`]; o `try/finally` é o
//! descarte (`handleProcessDispose` e a espera do `timeout(1100)`) antes de conferir o resultado, o que reprova do mesmo
//! jeito e deixa a conferência de vazamentos depois do dreno.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::timeout;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_platform::log::NullLogService;
use je_platform::terminal::auto_replies_contrib_controller::AutoRepliesPtyServiceContribution;
use je_platform::terminal::pty_service::IPtyServiceContribution;
use je_platform::terminal::terminal::{IProcessProperty, IProcessReadyEvent, ITerminalChildProcess, ITerminalLaunchOutcome, ProcessData, ProcessPropertyType, UnicodeVersion};

const NOT_MOCKED: &str = "membro do mock não definido";

struct MockTerminalChildProcess {
    data: Rc<Emitter<ProcessData>>,
    replies: Rc<RefCell<Vec<String>>>,
}

impl ITerminalChildProcess for MockTerminalChildProcess {
    fn id(&self) -> u32 {
        unreachable!("{NOT_MOCKED}")
    }

    fn should_persist(&self) -> bool {
        unreachable!("{NOT_MOCKED}")
    }

    fn on_process_data(&self) -> Event<ProcessData> {
        self.data.event()
    }

    fn on_process_ready(&self) -> Event<IProcessReadyEvent> {
        unreachable!("{NOT_MOCKED}")
    }

    fn on_did_change_property(&self) -> Event<IProcessProperty> {
        unreachable!("{NOT_MOCKED}")
    }

    fn on_process_exit(&self) -> Event<Option<i32>> {
        unreachable!("{NOT_MOCKED}")
    }

    fn start(&self) -> Promise<Option<ITerminalLaunchOutcome>> {
        unreachable!("{NOT_MOCKED}")
    }

    fn shutdown(&self, _immediate: bool) {
        unreachable!("{NOT_MOCKED}")
    }

    fn input(&self, value: &str) {
        self.replies.borrow_mut().push(value.to_owned());
    }

    fn send_signal(&self, _signal: &str) {
        unreachable!("{NOT_MOCKED}")
    }

    fn process_binary(&self, _data: &str) -> Promise<()> {
        unreachable!("{NOT_MOCKED}")
    }

    fn resize(&self, _cols: i32, _rows: i32, _pixel_width: Option<i32>, _pixel_height: Option<i32>) -> Result<(), Error> {
        unreachable!("{NOT_MOCKED}")
    }

    fn clear_buffer(&self) -> Promise<()> {
        unreachable!("{NOT_MOCKED}")
    }

    fn acknowledge_data_event(&self, _char_count: usize) {
        unreachable!("{NOT_MOCKED}")
    }

    fn set_unicode_version(&self, _version: UnicodeVersion) -> Promise<()> {
        unreachable!("{NOT_MOCKED}")
    }

    fn get_initial_cwd(&self) -> Promise<String> {
        unreachable!("{NOT_MOCKED}")
    }

    fn get_cwd(&self) -> Promise<String> {
        unreachable!("{NOT_MOCKED}")
    }

    fn refresh_property(&self, _property: ProcessPropertyType) -> Promise<IProcessProperty> {
        unreachable!("{NOT_MOCKED}")
    }

    fn update_property(&self, _property: IProcessProperty) -> Promise<()> {
        unreachable!("{NOT_MOCKED}")
    }
}

#[test]
fn does_not_duplicate_replies_when_a_persistent_process_becomes_ready_again() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let data: Rc<Emitter<ProcessData>> = store.add_rc(Emitter::new());
    let replies: Rc<RefCell<Vec<String>>> = Rc::default();
    run_with_faked_timers({
        let (data, replies) = (data.clone(), replies.clone());
        async move {
            let process: Rc<dyn ITerminalChildProcess> = Rc::new(MockTerminalChildProcess { data: data.clone(), replies: replies.clone() });
            let contribution = AutoRepliesPtyServiceContribution::new(NullLogService::shared());
            contribution.install_auto_reply("prompt", "reply").await.unwrap();
            contribution.handle_process_ready(1, process.clone());
            // Simulates the replay that happens when the process is reattached
            contribution.handle_process_ready(1, process.clone());
            data.fire(ProcessData::Text("prompt".to_owned()));
            let observed = replies.borrow().clone();

            contribution.handle_process_dispose(1);
            // Let the response throttle finish (in virtual time) before checking disposable ownership.
            timeout(1100).await.unwrap();

            assert_eq!(observed, vec!["reply".to_owned()]);
        }
    });
}
