//! Porte de `src/vs/platform/terminal/common/capabilities/shellEnvDetectionCapability.ts`: a capacidade que
//! guarda o ambiente mais recente do shell do usuário, enviado pela integração com o shell.
//!
//! Diferenças do original:
//! - `{ [key: string]: string | undefined }` é `IndexMap<String, Option<String>>` (a chave com valor `None` é
//!   a que o original deixa `undefined`). `Object.entries` e `Object.fromEntries` mantêm a ordem de inserção
//!   do [`IndexMap`]; a comparação `equals(this.env.value, env)` do original ignora a ordem das chaves, como o
//!   `==` do [`IndexMap`].
//! - `setEnvironment(env)` sem ambiente (`None`, o que o contrato do trait admite) vale como objeto vazio; o
//!   original lançaria `TypeError` em `Object.entries(undefined)`.
//! - `key !== undefined` das variáveis avulsas é sempre verdade (a chave é `&str`).
//! - `Disposable` com o `Emitter` registrado: `dispose` descarta o emissor.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::capabilities::{IShellEnvDetectionCapability, TerminalCapability, TerminalShellIntegrationEnvironment};

/// `IShellEnv`.
#[derive(Clone, Debug, PartialEq)]
pub struct IShellEnv {
    pub value: IndexMap<String, String>,
    pub is_trusted: bool,
}

/// `ShellEnvDetectionCapability`.
pub struct ShellEnvDetectionCapability {
    base: Rc<Disposable>,
    pending_env: RefCell<Option<IShellEnv>>,
    env: RefCell<IShellEnv>,
    on_did_change_env: Rc<Emitter<TerminalShellIntegrationEnvironment>>,
}

impl ShellEnvDetectionCapability {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_env: Rc<Emitter<TerminalShellIntegrationEnvironment>> = Emitter::new();
        base.register(on_did_change_env.clone());

        Rc::new(Self {
            base,
            pending_env: RefCell::new(None),
            env: RefCell::new(IShellEnv { value: IndexMap::new(), is_trusted: true }),
            on_did_change_env,
        })
    }

    fn fire_env_change(&self) {
        self.on_did_change_env.fire(self.create_state_object());
    }

    fn create_state_object(&self) -> TerminalShellIntegrationEnvironment {
        let env = self.env.borrow();
        TerminalShellIntegrationEnvironment {
            value: Some(env.value.iter().map(|(key, value)| (key.clone(), Some(value.clone()))).collect()),
            is_trusted: env.is_trusted,
        }
    }
}

impl IDisposable for ShellEnvDetectionCapability {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IShellEnvDetectionCapability for ShellEnvDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::ShellEnvDetection
    }

    fn on_did_change_env(&self) -> Event<TerminalShellIntegrationEnvironment> {
        self.on_did_change_env.event()
    }

    fn env(&self) -> TerminalShellIntegrationEnvironment {
        self.create_state_object()
    }

    fn set_environment(&self, envs: Option<&IndexMap<String, Option<String>>>, is_trusted: bool) {
        let empty = IndexMap::new();
        let envs = envs.unwrap_or(&empty);
        if self.create_state_object().value.as_ref() == Some(envs) {
            return;
        }

        {
            let mut env = self.env.borrow_mut();
            env.value.clear();
            for (key, value) in envs {
                if let Some(value) = value {
                    env.value.insert(key.clone(), value.clone());
                }
            }
            env.is_trusted = is_trusted;
        }

        self.fire_env_change();
    }

    fn start_environment_single_var(&self, clear: bool, is_trusted: bool) {
        let pending = if clear {
            IShellEnv { value: IndexMap::new(), is_trusted }
        } else {
            let env = self.env.borrow();
            IShellEnv { value: env.value.clone(), is_trusted: env.is_trusted && is_trusted }
        };
        *self.pending_env.borrow_mut() = Some(pending);
    }

    fn set_environment_single_var(&self, key: &str, value: Option<&str>, is_trusted: bool) {
        let mut pending = self.pending_env.borrow_mut();
        let Some(pending) = pending.as_mut() else {
            return;
        };
        if let Some(value) = value {
            pending.value.insert(key.to_string(), value.to_string());
            pending.is_trusted &= is_trusted;
        }
    }

    fn end_environment_single_var(&self, is_trusted: bool) {
        let Some(mut pending) = self.pending_env.borrow_mut().take() else {
            return;
        };
        pending.is_trusted &= is_trusted;
        // `mapsStrictEqualIgnoreOrder`
        let env_differs = self.env.borrow().value != pending.value;
        if env_differs {
            *self.env.borrow_mut() = pending;
            self.fire_env_change();
        }
    }

    fn delete_environment_single_var(&self, key: &str, value: Option<&str>, is_trusted: bool) {
        let mut pending = self.pending_env.borrow_mut();
        let Some(pending) = pending.as_mut() else {
            return;
        };
        if value.is_some() {
            pending.value.shift_remove(key);
            pending.is_trusted &= is_trusted;
        }
    }
}
