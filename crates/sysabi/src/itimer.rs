//! Timers de intervalo (`alarm(2)`, `setitimer(2)`, `getitimer(2)`): as regras de `kernel/time/itimer.c`
//! em funções puras que o kernel e o testkit compartilham. Quem chama guarda um [`ItimerSlot`] por
//! relógio e por processo, diz quanto tempo o relógio marca e entrega o sinal quando [`ItimerSlot::fire`]
//! avisa que venceu.

use crate::linux::{Errno, Signal};

/// Duração de um tick (`TICK_NSEC` com HZ=250): o que `getitimer` devolve de um timer de CPU armado que já
/// passou do prazo, e o passo com que o kernel confere os timers de CPU.
pub const TICK_NS: u64 = 4_000_000;

/// Os três relógios de `setitimer`, com os números do Linux.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Itimer {
    /// Tempo de parede; vence com `SIGALRM`.
    Real = 0,
    /// Tempo de CPU do processo em modo usuário; vence com `SIGVTALRM`.
    Virtual = 1,
    /// Tempo de CPU do processo (usuário mais sistema); vence com `SIGPROF`.
    Prof = 2,
}

impl Itimer {
    pub const ALL: [Itimer; 3] = [Itimer::Real, Itimer::Virtual, Itimer::Prof];

    /// O `which` da syscall; qualquer outro número é `EINVAL`.
    pub fn from_raw(which: i32) -> Result<Itimer, Errno> {
        Itimer::ALL.into_iter().find(|t| *t as i32 == which).ok_or(Errno::EINVAL)
    }

    pub fn signal(self) -> Signal {
        match self {
            Itimer::Real => Signal::SIGALRM,
            Itimer::Virtual => Signal::SIGVTALRM,
            Itimer::Prof => Signal::SIGPROF,
        }
    }

    /// O relógio do timer é de CPU (conferido a cada tick) e não de parede.
    pub fn is_cpu(self) -> bool {
        self != Itimer::Real
    }
}

/// `struct itimerval`: tempo até o próximo vencimento e intervalo de repetição, em segundos e microssegundos.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Itimerval {
    pub value_sec: i64,
    pub value_usec: i64,
    pub interval_sec: i64,
    pub interval_usec: i64,
}

/// O que `timeval_valid` aceita: segundos não negativos e microssegundos em `0..1_000_000`.
fn timeval_ns(sec: i64, usec: i64) -> Result<u64, Errno> {
    if sec < 0 || !(0..1_000_000).contains(&usec) {
        return Err(Errno::EINVAL);
    }
    Ok((sec as u64).saturating_mul(1_000_000_000).saturating_add(usec as u64 * 1000).min(i64::MAX as u64))
}

impl Itimerval {
    /// Um disparo só daqui a `seconds` (o que `alarm` pede).
    pub fn oneshot(seconds: i64) -> Itimerval {
        Itimerval { value_sec: seconds, ..Itimerval::default() }
    }

    /// `(valor, intervalo)` em nanossegundos; `EINVAL` se algum dos dois `timeval` é inválido.
    pub fn to_ns(&self) -> Result<(u64, u64), Errno> {
        Ok((timeval_ns(self.value_sec, self.value_usec)?, timeval_ns(self.interval_sec, self.interval_usec)?))
    }

    pub fn from_ns(value_ns: u64, interval_ns: u64) -> Itimerval {
        let split = |ns: u64| ((ns / 1_000_000_000) as i64, (ns % 1_000_000_000 / 1000) as i64);
        let (value_sec, value_usec) = split(value_ns);
        let (interval_sec, interval_usec) = split(interval_ns);
        Itimerval { value_sec, value_usec, interval_sec, interval_usec }
    }

    /// O que `alarm` devolve do alarme anterior: os segundos que faltavam, com 1 a mais quando sobra meio
    /// segundo ou mais, ou quando o alarme está armado e falta menos de um segundo (nunca 0 com alarme
    /// pendente).
    pub fn alarm_remaining(&self) -> u32 {
        let mut secs = self.value_sec;
        if (secs == 0 && self.value_usec != 0) || self.value_usec >= 500_000 {
            secs += 1;
        }
        secs.clamp(0, i64::from(u32::MAX)) as u32
    }
}

/// O estado de um relógio de `setitimer` de um processo: o prazo no relógio dele e o intervalo, em ns.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ItimerSlot {
    expiry: Option<u64>,
    interval: u64,
}

impl ItimerSlot {
    /// O prazo, se está armado.
    pub fn expiry(&self) -> Option<u64> {
        self.expiry
    }

    /// `getitimer`: o que falta (de um timer periódico que passou do prazo, até o próximo) e o intervalo. Armado
    /// que já venceu e ainda não foi tratado devolve o mínimo (1 us no relógio de parede, um tick nos de CPU),
    /// nunca zero.
    pub fn get(&self, which: Itimer, now: u64) -> (u64, u64) {
        let Some(expiry) = self.expiry else { return (0, self.interval) };
        let left = if now < expiry {
            expiry - now
        } else if self.interval > 0 {
            self.interval - (now - expiry) % self.interval
        } else {
            0
        };
        let floor = if which.is_cpu() { TICK_NS } else { 1000 };
        (if left == 0 { floor } else { left }, self.interval)
    }

    /// `setitimer`: rearma com `value` a partir de `now` (zero desarma, mas o intervalo fica guardado, como no
    /// Linux). Devolve o que o `getitimer` devolveria antes.
    pub fn set(&mut self, which: Itimer, now: u64, value: u64, interval: u64) -> (u64, u64) {
        let old = self.get(which, now);
        self.expiry = (value > 0).then(|| now.saturating_add(value));
        self.interval = interval;
        old
    }

    /// Desarma sem mexer no intervalo (o processo acabou).
    pub fn disarm(&mut self) {
        self.expiry = None;
    }

    /// Venceu em `now`? Se sim, avança o prazo (um disparo só desarma; periódico pula os vencimentos perdidos,
    /// porque sinal comum não se acumula) e devolve `true`: o chamador entrega o sinal.
    pub fn fire(&mut self, now: u64) -> bool {
        let Some(expiry) = self.expiry else { return false };
        if now < expiry {
            return false;
        }
        self.expiry = (self.interval > 0).then(|| expiry + self.interval * (1 + (now - expiry) / self.interval));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        assert_eq!(Itimerval { value_usec: 1_000_000, ..Default::default() }.to_ns(), Err(Errno::EINVAL));
        assert_eq!(Itimerval { interval_sec: -1, ..Default::default() }.to_ns(), Err(Errno::EINVAL));
        assert_eq!(Itimerval { value_sec: 1, value_usec: 5, ..Default::default() }.to_ns(), Ok((1_000_005_000, 0)));
        assert_eq!(Itimer::from_raw(3), Err(Errno::EINVAL));
        assert_eq!(Itimer::from_raw(2), Ok(Itimer::Prof));
    }

    #[test]
    fn alarm_rounding() {
        assert_eq!(Itimerval::from_ns(0, 0).alarm_remaining(), 0);
        assert_eq!(Itimerval::from_ns(1000, 0).alarm_remaining(), 1);
        assert_eq!(Itimerval::from_ns(2_400_000_000, 0).alarm_remaining(), 2);
        assert_eq!(Itimerval::from_ns(2_500_000_000, 0).alarm_remaining(), 3);
    }

    #[test]
    fn slot_cycle() {
        let mut s = ItimerSlot::default();
        assert_eq!(s.set(Itimer::Real, 100, 50, 20), (0, 0));
        assert_eq!(s.get(Itimer::Real, 120), (30, 20));
        assert!(!s.fire(149));
        assert!(s.fire(150));
        assert_eq!(s.expiry(), Some(170));
        assert!(s.fire(205));
        assert_eq!(s.expiry(), Some(210));
        assert_eq!(s.get(Itimer::Real, 205), (5, 20));
        assert_eq!(s.set(Itimer::Real, 205, 0, 0), (5, 20));
        assert!(!s.fire(1000));
    }

    #[test]
    fn one_shot_expired_reports_minimum() {
        let mut s = ItimerSlot::default();
        s.set(Itimer::Prof, 0, 10, 0);
        assert_eq!(s.get(Itimer::Prof, 10), (TICK_NS, 0));
        assert!(s.fire(10));
        assert_eq!(s.expiry(), None);
    }
}
