//! Porte de `src/vs/platform/terminal/node/terminalContrib/autoReplies/autoRepliesContribController.ts`: a contribuição
//! do pty service que instala respostas automáticas (um [`TerminalAutoResponder`] por processo e por texto).
//!
//! Representação: os `Map` do original viram [`IndexMap`] (a ordem de inserção é observável, como no `Map`). O
//! `ITerminalChildProcess` guardado é o `Rc<dyn ITerminalChildProcess>` que `handleProcessReady` recebe. `async
//! installAutoReply` e `async uninstallAllAutoReplies` não têm `await`: o corpo roda inteiro na chamada e devolve a
//! [`Promise`] já resolvida. `uninstallAllAutoReplies` não esvazia `_autoReplies`, como o original.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;

use super::pty_service::IPtyServiceContribution;
use super::terminal::ITerminalChildProcess;
use super::terminal_auto_responder::TerminalAutoResponder;
use crate::log::LogServiceRef;

pub struct AutoRepliesPtyServiceContribution {
    log_service: LogServiceRef,
    auto_replies: RefCell<IndexMap<String, String>>,
    terminal_processes: RefCell<IndexMap<u32, Rc<dyn ITerminalChildProcess>>>,
    auto_responders: RefCell<IndexMap<u32, IndexMap<String, Rc<TerminalAutoResponder>>>>,
}

impl AutoRepliesPtyServiceContribution {
    pub fn new(log_service: LogServiceRef) -> Self {
        Self { log_service, auto_replies: RefCell::default(), terminal_processes: RefCell::default(), auto_responders: RefCell::default() }
    }

    pub fn install_auto_reply(&self, match_: &str, reply: &str) -> Promise<()> {
        self.auto_replies.borrow_mut().insert(match_.to_owned(), reply.to_owned());
        // If the auto reply exists on any existing terminals it will be overridden
        let persistent_process_ids: Vec<u32> = self.auto_responders.borrow().keys().copied().collect();
        for persistent_process_id in persistent_process_ids {
            let process = self.terminal_processes.borrow().get(&persistent_process_id).cloned();
            let Some(process) = process else {
                self.log_service.error("Could not find terminal process to install auto reply");
                continue;
            };
            self.process_install_auto_reply(persistent_process_id, process, match_, reply);
        }
        Promise::resolved(())
    }

    pub fn uninstall_all_auto_replies(&self) -> Promise<()> {
        let matches: Vec<String> = self.auto_replies.borrow().keys().cloned().collect();
        for match_ in matches {
            let mut auto_responders = self.auto_responders.borrow_mut();
            for process_auto_responders in auto_responders.values_mut() {
                if let Some(responder) = process_auto_responders.shift_remove(&match_) {
                    responder.dispose();
                }
            }
        }
        Promise::resolved(())
    }

    fn process_install_auto_reply(&self, persistent_process_id: u32, terminal_process: Rc<dyn ITerminalChildProcess>, match_: &str, reply: &str) {
        if !self.auto_responders.borrow().contains_key(&persistent_process_id) {
            return;
        }
        let existing = self.auto_responders.borrow().get(&persistent_process_id).and_then(|responders| responders.get(match_).cloned());
        if let Some(existing) = existing {
            existing.dispose();
        }
        let responder = TerminalAutoResponder::new(terminal_process, match_, reply, self.log_service.clone());
        if let Some(process_auto_responders) = self.auto_responders.borrow_mut().get_mut(&persistent_process_id) {
            process_auto_responders.insert(match_.to_owned(), responder);
        }
    }
}

impl IPtyServiceContribution for AutoRepliesPtyServiceContribution {
    fn handle_process_ready(&self, persistent_process_id: u32, process: Rc<dyn ITerminalChildProcess>) {
        // Ready fires again when a persistent process is reattached, dispose the old responders
        let existing_auto_responders = self.auto_responders.borrow().get(&persistent_process_id).cloned();
        if let Some(existing_auto_responders) = existing_auto_responders {
            for responder in existing_auto_responders.values() {
                responder.dispose();
            }
        }
        self.terminal_processes.borrow_mut().insert(persistent_process_id, process.clone());
        self.auto_responders.borrow_mut().insert(persistent_process_id, IndexMap::new());
        let auto_replies: Vec<(String, String)> = self.auto_replies.borrow().iter().map(|(match_, reply)| (match_.clone(), reply.clone())).collect();
        for (match_, reply) in auto_replies {
            self.process_install_auto_reply(persistent_process_id, process.clone(), &match_, &reply);
        }
    }

    fn handle_process_dispose(&self, persistent_process_id: u32) {
        let process_auto_responders = self.auto_responders.borrow_mut().shift_remove(&persistent_process_id);
        if let Some(process_auto_responders) = process_auto_responders {
            for responder in process_auto_responders.values() {
                responder.dispose();
            }
        }
        self.terminal_processes.borrow_mut().shift_remove(&persistent_process_id);
    }

    fn handle_process_input(&self, persistent_process_id: u32, _data: &str) {
        if let Some(process_auto_responders) = self.auto_responders.borrow().get(&persistent_process_id) {
            for listener in process_auto_responders.values() {
                listener.handle_input();
            }
        }
    }

    fn handle_process_resize(&self, persistent_process_id: u32, _cols: i32, _rows: i32, _pixel_width: Option<i32>, _pixel_height: Option<i32>) {
        if let Some(process_auto_responders) = self.auto_responders.borrow().get(&persistent_process_id) {
            for listener in process_auto_responders.values() {
                listener.handle_resize();
            }
        }
    }
}
