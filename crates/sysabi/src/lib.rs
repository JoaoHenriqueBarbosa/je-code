//! sysabi: a "libc" do pseudo-linus.
//!
//! - [`linux`]: `Errno` e `Signal` com números do Linux e mensagens da glibc reais.
//! - [`types`]: tipos das syscalls (`Stat`, `OFlags`, `WaitStatus`, `SpawnSpec`...).
//! - [`sys`]: a trait [`Syscalls`] que o kernel implementa por processo, e as funções livres que os
//!   programas chamam sobre o processo corrente da thread.
//! - [`sched`]: política de escalonamento, afinidade, ioprio e personality (regras do Linux 6.12 num
//!   contêiner docker padrão, compartilhadas pelo kernel e pelo testkit).
//! - [`ctx`]: o [`Ctx`] dos builtins e adaptadores `Read`/`Write` sobre fds.
//! - [`program`]: a tabela de programas embutidos.
//! - `testkit` (feature): kernel de teste em memória pra testar programas isolados.

pub mod cmsg;
pub mod ctx;
pub mod itimer;
pub mod linux;
pub mod program;
pub mod sched;
pub mod sys;
pub mod types;
pub mod util;

#[cfg(feature = "testkit")]
pub mod testkit;

pub use ctx::{Ctx, FdReader, FdWriter};
pub use itimer::{Itimer, ItimerSlot, Itimerval};
pub use linux::{DefaultAction, Errno, Signal};
pub use program::{Main, Program};
pub use sched::{SchedAttr, SchedCaller, SchedParam, SchedState};
pub use sys::{ExecUnwind, ExitUnwind, KillUnwind, SysResult, Syscalls};
pub use types::*;
