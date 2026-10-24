//! Escalonamento, afinidade, prioridade de E/S e personality: as regras do Linux 6.12 num contêiner
//! docker padrão (root sem `CAP_SYS_NICE` nem `CAP_SYS_ADMIN`, seccomp padrão do docker), em funções
//! puras que o kernel e o testkit compartilham. Quem chama guarda o estado por processo e decide de
//! quem são os rlimits e o dono; aqui ficam só as regras e os errnos.
//!
//! Fontes: `kernel/sched/syscalls.c` (`__sched_setscheduler`, `user_check_sched_setscheduler`,
//! `sched_getattr`), `kernel/sched/core.c` (`sched_fork`), `block/ioprio.c`, `kernel/sys.c`
//! (`override_release`) e o `default.json` do seccomp do docker.

use std::time::Duration;

use crate::linux::Errno;
use crate::types::Utsname;

// ---- política de escalonamento ----

pub const SCHED_OTHER: i32 = 0;
pub const SCHED_FIFO: i32 = 1;
pub const SCHED_RR: i32 = 2;
pub const SCHED_BATCH: i32 = 3;
pub const SCHED_IDLE: i32 = 5;
pub const SCHED_DEADLINE: i32 = 6;
pub const SCHED_EXT: i32 = 7;
/// Bit que o `sched_setscheduler` aceita junto da política (e o `sched_getscheduler` devolve).
pub const SCHED_RESET_ON_FORK: i32 = 0x4000_0000;

/// `MAX_RT_PRIO`: as prioridades de tempo real vão de 1 a `MAX_RT_PRIO - 1`.
pub const MAX_RT_PRIO: u32 = 100;

/// `SCHED_FLAG_*` do `sched_attr.sched_flags`.
pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
pub const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u64 = 0x20;
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;
pub const SCHED_FLAG_UTIL_CLAMP: u64 = SCHED_FLAG_UTIL_CLAMP_MIN | SCHED_FLAG_UTIL_CLAMP_MAX;
/// `SCHED_FLAG_ALL`: o que o usuário pode pedir (o `SUGOV` é interno do kernel).
pub const SCHED_FLAG_ALL: u64 = 0x7f;

/// `SCHED_ATTR_SIZE_VER0` e `VER1` (com `util_min` e `util_max`).
pub const SCHED_ATTR_SIZE_VER0: u32 = 48;
pub const SCHED_ATTR_SIZE_VER1: u32 = 56;
/// `SCHED_CAPACITY_SCALE`: o teto do `uclamp`.
pub const SCHED_CAPACITY_SCALE: u32 = 1024;

/// `struct sched_param`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedParam {
    pub priority: i32,
}

/// `struct sched_attr` (VER1). `size` é o tamanho que o programa declara (0 vale `VER0`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedAttr {
    pub size: u32,
    pub policy: u32,
    pub flags: u64,
    pub nice: i32,
    pub priority: u32,
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub util_min: u32,
    pub util_max: u32,
}

/// Quem pede a mudança e o alvo, nos termos do `user_check_sched_setscheduler`. O contêiner padrão não
/// tem `CAP_SYS_NICE`, então não há campo pra ela: todo `req_priv` vira EPERM.
#[derive(Copy, Clone, Debug)]
pub struct SchedCaller {
    /// O alvo é do mesmo dono (`check_same_owner`).
    pub same_owner: bool,
    /// `RLIMIT_RTPRIO` do alvo (`task_rlimit(p, RLIMIT_RTPRIO)`), o valor atual.
    pub rlim_rtprio: u64,
    /// `RLIMIT_NICE` do alvo, o valor atual.
    pub rlim_nice: u64,
}

/// A parte do estado de escalonamento de um processo que não é a nice.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SchedState {
    pub policy: i32,
    pub rt_priority: u32,
    pub reset_on_fork: bool,
    /// Fatia pedida por `sched_setattr` (`sched_runtime` de política justa); 0 é a padrão.
    pub slice_ns: u64,
    pub util_min: u32,
    pub util_max: u32,
}

impl Default for SchedState {
    fn default() -> Self {
        SchedState { policy: SCHED_OTHER, rt_priority: 0, reset_on_fork: false, slice_ns: 0, util_min: 0, util_max: SCHED_CAPACITY_SCALE }
    }
}

fn is_rt(policy: i32) -> bool {
    policy == SCHED_FIFO || policy == SCHED_RR
}

fn is_fair(policy: i32) -> bool {
    policy == SCHED_OTHER || policy == SCHED_BATCH
}

fn valid_policy(policy: i32) -> bool {
    matches!(policy, SCHED_OTHER | SCHED_FIFO | SCHED_RR | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE)
}

/// `is_nice_reduction`: o rlimit `RLIMIT_NICE` cobre (20 - nice).
fn nice_reduction(nice: i32, rlim_nice: u64) -> bool {
    u64::try_from(20 - nice.clamp(-20, 19)).is_ok_and(|r| r <= rlim_nice)
}

/// `__checkparam_dl`.
fn checkparam_dl(a: &SchedAttr) -> bool {
    if a.deadline == 0 || a.runtime < (1 << 10) {
        return false;
    }
    if a.deadline & (1 << 63) != 0 || a.period & (1 << 63) != 0 {
        return false;
    }
    !((a.period != 0 && a.period < a.deadline) || a.deadline < a.runtime)
}

/// `sched_get_priority_max`.
pub fn priority_max(policy: i32) -> Result<i32, Errno> {
    match policy {
        SCHED_FIFO | SCHED_RR => Ok(MAX_RT_PRIO as i32 - 1),
        SCHED_DEADLINE | SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_EXT => Ok(0),
        _ => Err(Errno::EINVAL),
    }
}

/// `sched_get_priority_min`.
pub fn priority_min(policy: i32) -> Result<i32, Errno> {
    match policy {
        SCHED_FIFO | SCHED_RR => Ok(1),
        SCHED_DEADLINE | SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_EXT => Ok(0),
        _ => Err(Errno::EINVAL),
    }
}

/// `sysctl_sched_base_slice`: 0,70 ms (o `700000ULL` do 6.12) vezes (1 + log2 das CPUs, no máximo 8).
pub fn default_slice_ns(ncpus: usize) -> u64 {
    700_000 * (1 + u64::from(ncpus.clamp(1, 8).ilog2()))
}

impl SchedState {
    /// `sched_fork`: o estado do filho. Com `reset_on_fork`, política de tempo real volta a `SCHED_OTHER`
    /// com nice 0, nice negativa vira 0, o `uclamp` volta ao padrão e a flag se desliga.
    pub fn fork(&self, nice: i32) -> (SchedState, i32) {
        let mut s = *self;
        let mut n = nice;
        if s.reset_on_fork {
            if is_rt(s.policy) || s.policy == SCHED_DEADLINE {
                s.policy = SCHED_OTHER;
                s.rt_priority = 0;
                n = 0;
            } else if n < 0 {
                n = 0;
            }
            s.reset_on_fork = false;
            s.util_min = 0;
            s.util_max = SCHED_CAPACITY_SCALE;
        }
        (s, n)
    }

    /// `sched_setscheduler`, `sched_setparam` e `sched_setattr` por baixo: o `__sched_setscheduler` do
    /// 6.12 sem as capabilities. `nice` é a nice atual do alvo. `keep_policy` é o `SETPARAM_POLICY`
    /// (`sched_setparam`): a política e o `reset_on_fork` ficam como estão. Devolve o estado novo e a
    /// nice nova; os erros saem na ordem do kernel (EINVAL de forma, depois EPERM, depois o `uclamp`).
    pub fn set(&self, nice: i32, attr: &SchedAttr, keep_policy: bool, cx: &SchedCaller) -> Result<(SchedState, i32), Errno> {
        let keep_policy = keep_policy || attr.flags & SCHED_FLAG_KEEP_POLICY != 0;
        let (policy, reset_on_fork) = if keep_policy {
            (self.policy, self.reset_on_fork)
        } else {
            let p = attr.policy as i32;
            if !valid_policy(p) {
                return Err(Errno::EINVAL);
            }
            (p, attr.flags & SCHED_FLAG_RESET_ON_FORK != 0)
        };
        if attr.flags & !SCHED_FLAG_ALL != 0 {
            return Err(Errno::EINVAL);
        }
        // `get_params` (KEEP_PARAMS): os parâmetros de agora no lugar dos do pedido.
        let (priority, want_nice, runtime) = if attr.flags & SCHED_FLAG_KEEP_PARAMS != 0 {
            (self.rt_priority, nice, self.slice_ns)
        } else {
            (attr.priority, attr.nice.clamp(-20, 19), attr.runtime)
        };
        if priority > MAX_RT_PRIO - 1 {
            return Err(Errno::EINVAL);
        }
        if (policy == SCHED_DEADLINE && !checkparam_dl(attr)) || (is_rt(policy) != (priority != 0)) {
            return Err(Errno::EINVAL);
        }

        // `user_check_sched_setscheduler`: tudo que pede privilégio dá EPERM sem CAP_SYS_NICE.
        let mut req_priv = false;
        if is_fair(policy) && want_nice < nice && !nice_reduction(want_nice, cx.rlim_nice) {
            req_priv = true;
        }
        if is_rt(policy) {
            if policy != self.policy && cx.rlim_rtprio == 0 {
                req_priv = true;
            }
            if priority > self.rt_priority && u64::from(priority) > cx.rlim_rtprio {
                req_priv = true;
            }
        }
        if policy == SCHED_DEADLINE {
            req_priv = true;
        }
        // SCHED_IDLE conta como nice 20: sair dele só passa se o RLIMIT_NICE permitiria a nice atual.
        if self.policy == SCHED_IDLE && policy != SCHED_IDLE && !nice_reduction(nice, cx.rlim_nice) {
            req_priv = true;
        }
        if !cx.same_owner || (self.reset_on_fork && !reset_on_fork) {
            req_priv = true;
        }
        if req_priv {
            return Err(Errno::EPERM);
        }

        let mut s = *self;
        if attr.flags & SCHED_FLAG_UTIL_CLAMP != 0 {
            // `uclamp_validate`: -1 volta ao padrão; os dois pedidos juntos precisam de min <= max.
            let pick = |flag: u64, raw: u32| -> Result<Option<i64>, Errno> {
                if attr.flags & flag == 0 {
                    return Ok(None);
                }
                let v = i64::from(raw as i32);
                if !(-1..=i64::from(SCHED_CAPACITY_SCALE)).contains(&v) {
                    return Err(Errno::EINVAL);
                }
                Ok(Some(v))
            };
            let min = pick(SCHED_FLAG_UTIL_CLAMP_MIN, attr.util_min)?;
            let max = pick(SCHED_FLAG_UTIL_CLAMP_MAX, attr.util_max)?;
            if let (Some(a), Some(b)) = (min, max)
                && a != -1
                && b != -1
                && a > b
            {
                return Err(Errno::EINVAL);
            }
            if let Some(v) = min {
                s.util_min = if v == -1 { 0 } else { v as u32 };
            }
            if let Some(v) = max {
                s.util_max = if v == -1 { SCHED_CAPACITY_SCALE } else { v as u32 };
            }
        }
        s.policy = policy;
        s.rt_priority = priority;
        s.reset_on_fork = reset_on_fork;
        let mut out_nice = nice;
        if is_fair(policy) {
            out_nice = want_nice;
            s.slice_ns = if runtime != 0 { runtime.clamp(100_000, 100_000_000) } else { 0 };
        }
        Ok((s, out_nice))
    }

    /// `sched_getscheduler`: a política com o bit de `reset_on_fork`.
    pub fn scheduler(&self) -> i32 {
        self.policy | if self.reset_on_fork { SCHED_RESET_ON_FORK } else { 0 }
    }

    /// `sched_getattr`. `size` é o tamanho que o programa pediu (já validado por [`check_getattr`]).
    pub fn attr(&self, nice: i32, size: u32, ncpus: usize) -> SchedAttr {
        let mut a = SchedAttr {
            size: size.min(SCHED_ATTR_SIZE_VER1),
            policy: self.policy as u32,
            flags: if self.reset_on_fork { SCHED_FLAG_RESET_ON_FORK } else { 0 },
            util_min: self.util_min,
            util_max: self.util_max,
            ..SchedAttr::default()
        };
        if is_rt(self.policy) {
            a.priority = self.rt_priority;
        } else {
            a.nice = nice;
            a.runtime = if self.slice_ns != 0 { self.slice_ns } else { default_slice_ns(ncpus) };
        }
        a
    }

    /// `sched_rr_get_interval`: FIFO não tem fatia, RR tem 100 ms, e a classe justa devolve a fatia em
    /// jiffies (`HZ` = 250 no Debian, 4 ms), então a fatia padrão de menos de 4 ms sai 0.
    pub fn rr_interval(&self, ncpus: usize) -> Duration {
        const JIFFY_NS: u64 = 4_000_000;
        match self.policy {
            SCHED_FIFO | SCHED_DEADLINE => Duration::ZERO,
            SCHED_RR => Duration::from_millis(100),
            _ => {
                let slice = if self.slice_ns != 0 { self.slice_ns } else { default_slice_ns(ncpus) };
                Duration::from_nanos(slice / JIFFY_NS * JIFFY_NS)
            }
        }
    }
}

/// As checagens de forma do `sched_setscheduler` (pid, política) e o `SchedAttr` que ele monta: o bit
/// `SCHED_RESET_ON_FORK` sai da política e vira a flag, e os parâmetros são os atuais (`nice`).
pub fn attr_for_setscheduler(policy: i32, param: SchedParam, nice: i32) -> Result<SchedAttr, Errno> {
    if policy < 0 {
        return Err(Errno::EINVAL);
    }
    let reset = policy & SCHED_RESET_ON_FORK != 0;
    let priority = u32::try_from(param.priority).unwrap_or(u32::MAX);
    Ok(SchedAttr {
        policy: (policy & !SCHED_RESET_ON_FORK) as u32,
        flags: if reset { SCHED_FLAG_RESET_ON_FORK } else { 0 },
        nice,
        priority,
        ..SchedAttr::default()
    })
}

/// O `SchedAttr` do `sched_setparam`: só a prioridade, a política fica.
pub fn attr_for_setparam(param: SchedParam, nice: i32) -> SchedAttr {
    SchedAttr { nice, priority: u32::try_from(param.priority).unwrap_or(u32::MAX), ..SchedAttr::default() }
}

/// Valida o `SchedAttr` de `sched_setattr` como o `sched_copy_attr` e o syscall: `flags` do syscall
/// tem de ser 0, `size` 0 vale `VER0` e abaixo disso (ou acima de uma página) é E2BIG, e uma política
/// com o bit alto ligado (negativa como `int`) é EINVAL. Com `VER0` os campos do `uclamp` são zero.
pub fn check_setattr(attr: &SchedAttr, syscall_flags: u32) -> Result<SchedAttr, Errno> {
    if syscall_flags != 0 {
        return Err(Errno::EINVAL);
    }
    let size = if attr.size == 0 { SCHED_ATTR_SIZE_VER0 } else { attr.size };
    if !(SCHED_ATTR_SIZE_VER0..=4096).contains(&size) {
        return Err(Errno::E2BIG);
    }
    let mut a = *attr;
    a.size = size;
    if size < SCHED_ATTR_SIZE_VER1 {
        a.util_min = 0;
        a.util_max = 0;
    }
    if (a.policy as i32) < 0 {
        return Err(Errno::EINVAL);
    }
    Ok(a)
}

/// Valida `sched_getattr(pid, size, flags)`: tamanho entre `VER0` e uma página, flags 0.
pub fn check_getattr(size: u32, syscall_flags: u32) -> Result<(), Errno> {
    if syscall_flags != 0 || !(SCHED_ATTR_SIZE_VER0..=4096).contains(&size) {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// ---- afinidade ----

/// A máscara que `sched_setaffinity` grava: só CPUs online, sem repetição e em ordem. EINVAL se não
/// sobra nenhuma (`__set_cpus_allowed_ptr` exige interseção com as CPUs ativas).
pub fn normalize_affinity(ncpus: usize, requested: &[usize]) -> Result<Vec<usize>, Errno> {
    let mut v: Vec<usize> = requested.iter().copied().filter(|c| *c < ncpus).collect();
    v.sort_unstable();
    v.dedup();
    if v.is_empty() { Err(Errno::EINVAL) } else { Ok(v) }
}

/// A máscara efetiva: a gravada (ou todas, se nunca mudou) interseccionada com as CPUs online.
pub fn effective_affinity(ncpus: usize, stored: Option<&[usize]>) -> Vec<usize> {
    match stored {
        None => (0..ncpus).collect(),
        Some(s) => s.iter().copied().filter(|c| *c < ncpus).collect(),
    }
}

// ---- prioridade de E/S ----

pub const IOPRIO_CLASS_SHIFT: u32 = 13;
pub const IOPRIO_CLASS_NONE: i32 = 0;
pub const IOPRIO_CLASS_RT: i32 = 1;
pub const IOPRIO_CLASS_BE: i32 = 2;
pub const IOPRIO_CLASS_IDLE: i32 = 3;
/// Níveis de RT e BE (`IOPRIO_NR_LEVELS`) e a máscara do nível (`IOPRIO_LEVEL_MASK`).
pub const IOPRIO_NR_LEVELS: i32 = 8;
pub const IOPRIO_LEVEL_MASK: i32 = 7;
pub const IOPRIO_WHO_PROCESS: i32 = 1;
pub const IOPRIO_WHO_PGRP: i32 = 2;
pub const IOPRIO_WHO_USER: i32 = 3;

/// `IOPRIO_PRIO_VALUE(class, data)`.
pub const fn ioprio_value(class: i32, data: i32) -> i32 {
    (class << IOPRIO_CLASS_SHIFT) | data
}

pub const fn ioprio_class(ioprio: i32) -> i32 {
    ioprio >> IOPRIO_CLASS_SHIFT
}

pub const fn ioprio_level(ioprio: i32) -> i32 {
    ioprio & IOPRIO_LEVEL_MASK
}

/// `ioprio_check_cap`: o que `ioprio_set` valida antes de procurar o alvo. Sem `CAP_SYS_NICE` nem
/// `CAP_SYS_ADMIN`, a classe RT dá EPERM.
pub fn ioprio_check(ioprio: i32) -> Result<(), Errno> {
    let level = ioprio_level(ioprio);
    match ioprio_class(ioprio) {
        IOPRIO_CLASS_RT => Err(Errno::EPERM),
        IOPRIO_CLASS_BE => {
            if level >= IOPRIO_NR_LEVELS {
                Err(Errno::EINVAL)
            } else {
                Ok(())
            }
        }
        IOPRIO_CLASS_IDLE => Ok(()),
        IOPRIO_CLASS_NONE => {
            if level != 0 {
                Err(Errno::EINVAL)
            } else {
                Ok(())
            }
        }
        // No oráculo (root sem capacidades), classe fora do intervalo cai em EPERM.
        _ => Err(Errno::EPERM),
    }
}

/// `__get_task_ioprio` de um processo com contexto de E/S: o valor gravado, ou, se a classe é NONE, a
/// derivada da política e da nice (`task_nice_ioclass` e `(nice + 20) / 5`).
pub fn ioprio_effective(stored: i32, policy: i32, nice: i32) -> i32 {
    if ioprio_class(stored) != IOPRIO_CLASS_NONE {
        return stored;
    }
    let class = if policy == SCHED_IDLE {
        IOPRIO_CLASS_IDLE
    } else if is_rt(policy) {
        IOPRIO_CLASS_RT
    } else {
        IOPRIO_CLASS_BE
    };
    ioprio_value(class, (nice + 20) / 5)
}

/// `ioprio_best`: de duas, a melhor (o menor valor; classe NONE conta como 0).
pub fn ioprio_best(a: i32, b: i32) -> i32 {
    let norm = |x: i32| if ioprio_class(x) == IOPRIO_CLASS_NONE { 0 } else { x };
    norm(a).min(norm(b))
}

// ---- personality ----

/// `personality(2)`: os bits e as regras que o `uname` e o `exec` usam.
pub mod personality {
    use super::{Errno, Utsname};

    /// Argumento que só lê a personality atual.
    pub const QUERY: u32 = 0xffff_ffff;
    pub const PER_LINUX: u32 = 0;
    pub const PER_LINUX32: u32 = 0x0008;
    /// Máscara do domínio de execução (`PER_MASK`).
    pub const PER_MASK: u32 = 0x00ff;
    pub const UNAME26: u32 = 0x002_0000;
    pub const ADDR_NO_RANDOMIZE: u32 = 0x004_0000;
    pub const FDPIC_FUNCPTRS: u32 = 0x008_0000;
    pub const MMAP_PAGE_ZERO: u32 = 0x010_0000;
    pub const ADDR_COMPAT_LAYOUT: u32 = 0x020_0000;
    pub const READ_IMPLIES_EXEC: u32 = 0x040_0000;
    pub const ADDR_LIMIT_32BIT: u32 = 0x080_0000;
    pub const SHORT_INODE: u32 = 0x100_0000;
    pub const WHOLE_SECONDS: u32 = 0x200_0000;
    pub const STICKY_TIMEOUTS: u32 = 0x400_0000;
    pub const ADDR_LIMIT_3GB: u32 = 0x800_0000;
    /// `PER_CLEAR_ON_SETID`: os bits que um exec com troca de credenciais apaga.
    pub const PER_CLEAR_ON_SETID: u32 = READ_IMPLIES_EXEC | ADDR_NO_RANDOMIZE | ADDR_COMPAT_LAYOUT | MMAP_PAGE_ZERO;

    /// O filtro `personality` do seccomp padrão do docker: só 0x0, 0x0008, 0x20000, 0x20008 e a leitura
    /// (`0xffffffff`) passam; qualquer outro valor volta EPERM antes de chegar ao kernel.
    pub fn docker_seccomp_allows(persona: u32) -> bool {
        matches!(persona, 0x0 | 0x0008 | 0x2_0000 | 0x2_0008 | QUERY)
    }

    /// O `personality(2)` do contêiner: o filtro do seccomp e, passando, o valor novo (ou o atual, na
    /// leitura). Devolve `(antiga, nova)`.
    pub fn change(current: u32, persona: u32) -> Result<(u32, u32), Errno> {
        if !docker_seccomp_allows(persona) {
            return Err(Errno::EPERM);
        }
        Ok((current, if persona == QUERY { current } else { persona }))
    }

    /// A personality depois de um `execve`: ELF de 64 bits apaga `READ_IMPLIES_EXEC`
    /// (`set_personality_64bit`), e um exec com troca de credenciais (`secure`: setuid ou setgid que
    /// muda o dono efetivo) apaga todos os bits de [`PER_CLEAR_ON_SETID`]. O resto passa, inclusive
    /// `PER_LINUX32` e `UNAME26` (é assim que `setarch i686 prog` funciona).
    pub fn after_exec(persona: u32, secure: bool) -> u32 {
        let mut p = persona & !READ_IMPLIES_EXEC;
        if secure {
            p &= !PER_CLEAR_ON_SETID;
        }
        p
    }

    /// O `release` do `UNAME26` (`override_release`): `2.6.<minor + 60>` mais o que vem depois dos três
    /// primeiros números do release real, até 64 bytes. `6.12.101+deb13-amd64` vira `2.6.72+deb13-amd64`.
    pub fn uname26_release(release: &[u8]) -> Vec<u8> {
        let mut ndots = 0;
        let mut i = 0;
        while i < release.len() {
            let c = release[i];
            if c == b'.' {
                ndots += 1;
                if ndots >= 3 {
                    break;
                }
            } else if !c.is_ascii_digit() {
                break;
            }
            i += 1;
        }
        let minor: u32 = release
            .split(|b| *b == b'.')
            .nth(1)
            .map(|m| m.iter().take_while(|b| b.is_ascii_digit()).fold(0u32, |a, b| a.saturating_mul(10).saturating_add(u32::from(b - b'0'))))
            .unwrap_or(0);
        let mut out = format!("2.6.{}", minor + 60).into_bytes();
        out.extend_from_slice(&release[i..]);
        out.truncate(64);
        out
    }

    /// O que o `uname` mostra com a personality: `PER_LINUX32` troca o `machine` `x86_64` por `i686`, e
    /// `UNAME26` reescreve o `release`.
    pub fn apply_to_uname(persona: u32, u: &mut Utsname) {
        if persona & UNAME26 != 0 {
            u.release = uname26_release(&u.release);
        }
        if persona & PER_MASK == PER_LINUX32 && u.machine.starts_with(b"x86_64") {
            u.machine = b"i686".to_vec();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::personality as per;
    use super::*;

    fn cx() -> SchedCaller {
        SchedCaller { same_owner: true, rlim_rtprio: 0, rlim_nice: 0 }
    }

    fn setsched(s: &SchedState, nice: i32, policy: i32, prio: i32) -> Result<(SchedState, i32), Errno> {
        let a = attr_for_setscheduler(policy, SchedParam { priority: prio }, nice)?;
        s.set(nice, &a, false, &cx())
    }

    #[test]
    fn container_default_blocks_realtime_but_allows_idle_and_batch() {
        let s = SchedState::default();
        assert_eq!(setsched(&s, 0, SCHED_FIFO, 10).unwrap_err(), Errno::EPERM);
        assert_eq!(setsched(&s, 0, SCHED_RR, 1).unwrap_err(), Errno::EPERM);
        let (b, _) = setsched(&s, 0, SCHED_BATCH, 0).unwrap();
        assert_eq!(b.policy, SCHED_BATCH);
        let (i, _) = setsched(&s, 0, SCHED_IDLE, 0).unwrap();
        assert_eq!(i.scheduler(), SCHED_IDLE);
        let (o, _) = setsched(&b, 0, SCHED_OTHER, 0).unwrap();
        assert_eq!(o.policy, SCHED_OTHER);
    }

    #[test]
    fn invalid_shapes_are_einval_before_eperm() {
        let s = SchedState::default();
        assert_eq!(setsched(&s, 0, SCHED_FIFO, 0).unwrap_err(), Errno::EINVAL, "FIFO pede prioridade");
        assert_eq!(setsched(&s, 0, SCHED_OTHER, 3).unwrap_err(), Errno::EINVAL, "OTHER não tem prioridade");
        assert_eq!(setsched(&s, 0, SCHED_FIFO, 100).unwrap_err(), Errno::EINVAL);
        assert_eq!(setsched(&s, 0, SCHED_FIFO, -1).unwrap_err(), Errno::EINVAL);
        assert_eq!(setsched(&s, 0, 4, 0).unwrap_err(), Errno::EINVAL, "4 não existe");
        assert_eq!(setsched(&s, 0, 99, 0).unwrap_err(), Errno::EINVAL);
        assert_eq!(attr_for_setscheduler(-1, SchedParam::default(), 0).unwrap_err(), Errno::EINVAL);
        assert_eq!(setsched(&s, 0, SCHED_DEADLINE, 0).unwrap_err(), Errno::EINVAL, "sem runtime nem deadline");
    }

    #[test]
    fn rtprio_rlimit_opens_realtime() {
        let s = SchedState::default();
        let open = SchedCaller { rlim_rtprio: 50, ..cx() };
        let a = attr_for_setscheduler(SCHED_FIFO, SchedParam { priority: 50 }, 0).unwrap();
        let (f, _) = s.set(0, &a, false, &open).unwrap();
        assert_eq!((f.policy, f.rt_priority), (SCHED_FIFO, 50));
        let a = attr_for_setscheduler(SCHED_FIFO, SchedParam { priority: 51 }, 0).unwrap();
        assert_eq!(s.set(0, &a, false, &open).unwrap_err(), Errno::EPERM);
        // sched_setparam mantém a política e só mexe na prioridade.
        let p = attr_for_setparam(SchedParam { priority: 10 }, 0);
        let (g, _) = f.set(0, &p, true, &open).unwrap();
        assert_eq!((g.policy, g.rt_priority), (SCHED_FIFO, 10));
        let p = attr_for_setparam(SchedParam { priority: 0 }, 0);
        assert_eq!(f.set(0, &p, true, &open).unwrap_err(), Errno::EINVAL);
    }

    #[test]
    fn idle_cannot_go_back_without_nice_rlimit() {
        let (i, _) = setsched(&SchedState::default(), 0, SCHED_IDLE, 0).unwrap();
        assert_eq!(setsched(&i, 0, SCHED_OTHER, 0).unwrap_err(), Errno::EPERM);
        let open = SchedCaller { rlim_nice: 20, ..cx() };
        let a = attr_for_setscheduler(SCHED_OTHER, SchedParam::default(), 0).unwrap();
        assert!(i.set(0, &a, false, &open).is_ok());
    }

    #[test]
    fn reset_on_fork_rules() {
        let s = SchedState::default();
        let (r, _) = setsched(&s, 0, SCHED_BATCH | SCHED_RESET_ON_FORK, 0).unwrap();
        assert_eq!(r.scheduler(), SCHED_BATCH | SCHED_RESET_ON_FORK);
        // Quem tem a flag não a desliga sem privilégio.
        assert_eq!(setsched(&r, 0, SCHED_BATCH, 0).unwrap_err(), Errno::EPERM);
        let (child, nice) = r.fork(-3);
        assert_eq!((child.policy, child.reset_on_fork, nice), (SCHED_BATCH, false, 0));
        let rt = SchedState { policy: SCHED_RR, rt_priority: 5, reset_on_fork: true, ..SchedState::default() };
        let (child, nice) = rt.fork(-5);
        assert_eq!((child.policy, child.rt_priority, nice), (SCHED_OTHER, 0, 0));
    }

    #[test]
    fn setattr_checks_size_flags_and_nice() {
        let mut a = SchedAttr { size: 47, ..SchedAttr::default() };
        assert_eq!(check_setattr(&a, 0).unwrap_err(), Errno::E2BIG);
        a.size = 0;
        assert_eq!(check_setattr(&a, 1).unwrap_err(), Errno::EINVAL);
        let a = check_setattr(&a, 0).unwrap();
        assert_eq!(a.size, SCHED_ATTR_SIZE_VER0);
        let s = SchedState::default();
        // nice mais baixa que a atual precisa de RLIMIT_NICE.
        let low = SchedAttr { policy: SCHED_OTHER as u32, nice: -5, ..a };
        assert_eq!(s.set(0, &low, false, &cx()).unwrap_err(), Errno::EPERM);
        // nice mais alta passa e vira a nice do processo, com o clamp de 19.
        let high = SchedAttr { policy: SCHED_BATCH as u32, nice: 99, ..a };
        let (b, n) = s.set(0, &high, false, &cx()).unwrap();
        assert_eq!((b.policy, n), (SCHED_BATCH, 19));
        let bad = SchedAttr { flags: 0x8000, ..a };
        assert_eq!(s.set(0, &bad, false, &cx()).unwrap_err(), Errno::EINVAL);
        assert_eq!(check_getattr(47, 0).unwrap_err(), Errno::EINVAL);
        assert_eq!(check_getattr(56, 1).unwrap_err(), Errno::EINVAL);
        assert!(check_getattr(56, 0).is_ok());
    }

    #[test]
    fn getattr_reports_policy_nice_and_default_slice() {
        let s = SchedState::default();
        let a = s.attr(4, 56, 8);
        assert_eq!((a.size, a.policy, a.nice, a.priority, a.runtime), (56, 0, 4, 0, 2_800_000));
        assert_eq!((a.util_min, a.util_max), (0, 1024));
        assert_eq!(s.attr(0, 1000, 1).size, 56);
        assert_eq!(default_slice_ns(1), 700_000);
        assert_eq!(default_slice_ns(2), 1_400_000);
        assert_eq!(default_slice_ns(64), 2_800_000);
    }

    #[test]
    fn uclamp_validation() {
        let s = SchedState::default();
        let base = SchedAttr { policy: SCHED_OTHER as u32, size: 56, ..SchedAttr::default() };
        let ok = SchedAttr { flags: SCHED_FLAG_UTIL_CLAMP, util_min: 100, util_max: 500, ..base };
        let (n, _) = s.set(0, &ok, false, &cx()).unwrap();
        assert_eq!((n.util_min, n.util_max), (100, 500));
        let bad = SchedAttr { flags: SCHED_FLAG_UTIL_CLAMP, util_min: 600, util_max: 500, ..base };
        assert_eq!(s.set(0, &bad, false, &cx()).unwrap_err(), Errno::EINVAL);
        let big = SchedAttr { flags: SCHED_FLAG_UTIL_CLAMP_MAX, util_max: 1025, ..base };
        assert_eq!(s.set(0, &big, false, &cx()).unwrap_err(), Errno::EINVAL);
        let reset = SchedAttr { flags: SCHED_FLAG_UTIL_CLAMP, util_min: u32::MAX, util_max: u32::MAX, ..base };
        let (r, _) = n.set(0, &reset, false, &cx()).unwrap();
        assert_eq!((r.util_min, r.util_max), (0, 1024));
    }

    #[test]
    fn priority_ranges_and_rr_interval() {
        assert_eq!((priority_min(SCHED_FIFO).unwrap(), priority_max(SCHED_FIFO).unwrap()), (1, 99));
        assert_eq!((priority_min(SCHED_RR).unwrap(), priority_max(SCHED_RR).unwrap()), (1, 99));
        for p in [SCHED_OTHER, SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE] {
            assert_eq!((priority_min(p).unwrap(), priority_max(p).unwrap()), (0, 0));
        }
        assert_eq!(priority_max(4).unwrap_err(), Errno::EINVAL);
        assert_eq!(priority_min(-1).unwrap_err(), Errno::EINVAL);
        let rr = SchedState { policy: SCHED_RR, rt_priority: 1, ..SchedState::default() };
        assert_eq!(rr.rr_interval(1), Duration::from_millis(100));
        let fifo = SchedState { policy: SCHED_FIFO, rt_priority: 1, ..SchedState::default() };
        assert_eq!(fifo.rr_interval(1), Duration::ZERO);
        assert_eq!(SchedState::default().rr_interval(8), Duration::ZERO);
        let long = SchedState { slice_ns: 100_000_000, ..SchedState::default() };
        assert_eq!(long.rr_interval(8), Duration::from_millis(100));
    }

    #[test]
    fn affinity_masks() {
        assert_eq!(normalize_affinity(4, &[3, 1, 1, 9]).unwrap(), vec![1, 3]);
        assert_eq!(normalize_affinity(4, &[]).unwrap_err(), Errno::EINVAL);
        assert_eq!(normalize_affinity(4, &[4, 5]).unwrap_err(), Errno::EINVAL);
        assert_eq!(effective_affinity(3, None), vec![0, 1, 2]);
        assert_eq!(effective_affinity(2, Some(&[0, 1, 5])), vec![0, 1]);
    }

    #[test]
    fn ioprio_rules() {
        assert!(ioprio_check(ioprio_value(IOPRIO_CLASS_BE, 7)).is_ok());
        // O oitavo valor já é o bit de dica (hint) do kernel 6.x, não um nível inválido.
        assert!(ioprio_check(ioprio_value(IOPRIO_CLASS_BE, 8)).is_ok());
        assert_eq!(ioprio_check(ioprio_value(IOPRIO_CLASS_RT, 0)).unwrap_err(), Errno::EPERM);
        assert!(ioprio_check(ioprio_value(IOPRIO_CLASS_IDLE, 0)).is_ok());
        assert!(ioprio_check(0).is_ok());
        assert_eq!(ioprio_check(ioprio_value(IOPRIO_CLASS_NONE, 3)).unwrap_err(), Errno::EINVAL);
        assert_eq!(ioprio_check(ioprio_value(4, 0)).unwrap_err(), Errno::EPERM);
        assert_eq!(ioprio_check(-1).unwrap_err(), Errno::EPERM);
        // Padrão: BE com nível (nice + 20) / 5.
        assert_eq!(ioprio_effective(0, SCHED_OTHER, 0), ioprio_value(IOPRIO_CLASS_BE, 4));
        assert_eq!(ioprio_effective(0, SCHED_OTHER, 19), ioprio_value(IOPRIO_CLASS_BE, 7));
        assert_eq!(ioprio_effective(0, SCHED_OTHER, -20), ioprio_value(IOPRIO_CLASS_BE, 0));
        assert_eq!(ioprio_effective(0, SCHED_IDLE, 0), ioprio_value(IOPRIO_CLASS_IDLE, 4));
        assert_eq!(ioprio_effective(0, SCHED_RR, 0), ioprio_value(IOPRIO_CLASS_RT, 4));
        let set = ioprio_value(IOPRIO_CLASS_BE, 1);
        assert_eq!(ioprio_effective(set, SCHED_OTHER, 10), set);
        assert_eq!(ioprio_best(ioprio_value(IOPRIO_CLASS_BE, 5), ioprio_value(IOPRIO_CLASS_BE, 2)), ioprio_value(IOPRIO_CLASS_BE, 2));
    }

    #[test]
    fn personality_seccomp_and_bits() {
        for ok in [0, 8, 0x20000, 0x20008, 0xffff_ffff] {
            assert!(per::docker_seccomp_allows(ok), "{ok:#x}");
        }
        for bad in [0x40000, 0x40008, 1, 0x0000_0009, 0x0004_0000 | 0x0002_0000] {
            assert_eq!(per::change(0, bad).unwrap_err(), Errno::EPERM, "{bad:#x}");
        }
        assert_eq!(per::change(8, per::QUERY).unwrap(), (8, 8));
        assert_eq!(per::change(8, 0).unwrap(), (8, 0));
        assert_eq!(per::change(0, 0x20008).unwrap(), (0, 0x20008));
        let all = per::ADDR_NO_RANDOMIZE | per::READ_IMPLIES_EXEC | per::MMAP_PAGE_ZERO | per::ADDR_COMPAT_LAYOUT | per::PER_LINUX32 | per::UNAME26;
        assert_eq!(per::after_exec(all, false), all & !per::READ_IMPLIES_EXEC);
        assert_eq!(per::after_exec(all, true), per::PER_LINUX32 | per::UNAME26);
    }

    #[test]
    fn personality_uname() {
        let base = Utsname {
            sysname: b"Linux".to_vec(),
            nodename: b"h".to_vec(),
            release: b"6.12.101+deb13-amd64".to_vec(),
            version: b"#1".to_vec(),
            machine: b"x86_64".to_vec(),
            domainname: b"(none)".to_vec(),
        };
        let mut u = base.clone();
        per::apply_to_uname(0, &mut u);
        assert_eq!(u, base);
        per::apply_to_uname(per::PER_LINUX32, &mut u);
        assert_eq!(u.machine, b"i686");
        assert_eq!(u.release, base.release);
        let mut u = base.clone();
        per::apply_to_uname(per::UNAME26, &mut u);
        assert_eq!(u.release, b"2.6.72+deb13-amd64");
        assert_eq!(u.machine, b"x86_64");
        let mut u = base;
        per::apply_to_uname(per::UNAME26 | per::PER_LINUX32 | per::ADDR_NO_RANDOMIZE, &mut u);
        assert_eq!((u.release.as_slice(), u.machine.as_slice()), (&b"2.6.72+deb13-amd64"[..], &b"i686"[..]));
        assert_eq!(per::uname26_release(b"5.10.0"), b"2.6.70");
        assert_eq!(per::uname26_release(b"6.1"), b"2.6.61");
    }
}
