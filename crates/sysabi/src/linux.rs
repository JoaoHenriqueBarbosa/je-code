//! `Errno` e `Signal` com os números do Linux x86_64 e as mensagens da glibc 2.41, gerados de
//! `linux_facts.json` pelo `build.rs`.

use std::fmt;

/// Um errno do Linux. `Errno::ENOENT`, `Errno::EACCES`... são gerados a partir do sistema real.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Errno(pub i32);

/// Um sinal do Linux. `Signal::SIGKILL`, `Signal::SIGPIPE`... gerados a partir do sistema real.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Signal(pub i32);

include!(concat!(env!("OUT_DIR"), "/linux_tables.rs"));

/// Aliases do Linux que a tabela gerada não traz (ela guarda um nome por número).
impl Errno {
    pub const EWOULDBLOCK: Errno = Errno(11);
    pub const EDEADLK: Errno = Errno(35);
    pub const EOPNOTSUPP: Errno = Errno(95);
}

impl Errno {
    /// Nome simbólico ("ENOENT"), quando o número tem nome.
    pub fn name(self) -> Option<&'static str> {
        ERRNO_TABLE.iter().find(|e| e.0 == self.0).and_then(|e| e.1)
    }

    /// Mensagem exata do `strerror` da glibc. Número desconhecido sai como a glibc: "Unknown error N".
    pub fn message(self) -> String {
        match ERRNO_TABLE.iter().find(|e| e.0 == self.0) {
            Some(e) => e.2.to_string(),
            None => format!("Unknown error {}", self.0),
        }
    }

    pub fn from_name(name: &str) -> Option<Errno> {
        ERRNO_TABLE.iter().find(|e| e.1 == Some(name)).map(|e| Errno(e.0))
    }

    /// Conversão pra `std::io::Error` preservando o número (quem formata a mensagem deve usar
    /// [`Errno::message`], nunca o `Display` do `io::Error`, que acrescenta " (os error N)").
    pub fn to_io(self) -> std::io::Error {
        std::io::Error::from_raw_os_error(self.0)
    }

    /// Volta de `std::io::Error`; erros sem errno viram EIO.
    pub fn from_io(e: &std::io::Error) -> Errno {
        match e.raw_os_error() {
            Some(n) => Errno(n),
            None => match e.kind() {
                std::io::ErrorKind::NotFound => Errno::ENOENT,
                std::io::ErrorKind::PermissionDenied => Errno::EACCES,
                std::io::ErrorKind::AlreadyExists => Errno::EEXIST,
                std::io::ErrorKind::InvalidInput => Errno::EINVAL,
                std::io::ErrorKind::BrokenPipe => Errno::EPIPE,
                std::io::ErrorKind::WouldBlock => Errno::EAGAIN,
                std::io::ErrorKind::Interrupted => Errno::EINTR,
                std::io::ErrorKind::UnexpectedEof => Errno::EIO,
                _ => Errno::EIO,
            },
        }
    }
}

impl fmt::Debug for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => write!(f, "{n}"),
            None => write!(f, "Errno({})", self.0),
        }
    }
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Errno {}

impl From<Errno> for std::io::Error {
    fn from(e: Errno) -> Self {
        e.to_io()
    }
}

/// Primeiro sinal de tempo real (como o glibc expõe `SIGRTMIN`).
pub const SIGRTMIN: i32 = 34;
/// Último sinal válido.
pub const SIGRTMAX: i32 = 64;

/// O que o kernel faz quando o sinal chega e a disposição é a padrão.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DefaultAction {
    Terminate,
    CoreDump,
    Ignore,
    Stop,
    Continue,
}

impl Signal {
    /// Nome canônico sem o prefixo, como o `kill -l` imprime: "HUP", "INT"... Tempo real sai como
    /// "RTMIN+N" / "RTMAX-N", igual ao bash.
    pub fn name(self) -> Option<String> {
        if let Some(e) = SIGNAL_TABLE.iter().find(|e| e.0 == self.0) {
            return Some(e.1.trim_start_matches("SIG").to_string());
        }
        let n = self.0;
        if (SIGRTMIN..=SIGRTMAX).contains(&n) {
            let mid = (SIGRTMIN + SIGRTMAX) / 2;
            return Some(if n <= mid { format!("RTMIN+{}", n - SIGRTMIN) } else { format!("RTMAX-{}", SIGRTMAX - n) });
        }
        None
    }

    /// Descrição do `strsignal` ("Killed", "Terminated", "Broken pipe"...).
    pub fn description(self) -> String {
        match SIGNAL_TABLE.iter().find(|e| e.0 == self.0) {
            Some(e) => e.2.to_string(),
            None if (SIGRTMIN..=SIGRTMAX).contains(&self.0) => format!("Real-time signal {}", self.0 - SIGRTMIN),
            None => format!("Unknown signal {}", self.0),
        }
    }

    /// Aceita "TERM", "SIGTERM", "term" ou o número.
    pub fn parse(s: &str) -> Option<Signal> {
        if let Ok(n) = s.parse::<i32>() {
            return (0..=SIGRTMAX).contains(&n).then_some(Signal(n));
        }
        let upper = s.to_ascii_uppercase();
        // um `SIG` só: o bash recusa `SIGSIGHUP`
        let bare = upper.strip_prefix("SIG").unwrap_or(&upper);
        for n in 1..=SIGRTMAX {
            if Signal(n).name().as_deref() == Some(bare) {
                return Some(Signal(n));
            }
        }
        match bare {
            "IOT" => Some(Signal(6)),
            "POLL" => Some(Signal(29)),
            "CLD" => Some(Signal(17)),
            _ => None,
        }
    }

    pub fn is_valid(self) -> bool {
        (1..=SIGRTMAX).contains(&self.0)
    }

    /// Ação padrão segundo signal(7).
    pub fn default_action(self) -> DefaultAction {
        match self.0 {
            // SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGSEGV, SIGXCPU, SIGXFSZ, SIGSYS
            3 | 4 | 5 | 6 | 7 | 8 | 11 | 24 | 25 | 31 => DefaultAction::CoreDump,
            // SIGCHLD, SIGURG, SIGWINCH
            17 | 23 | 28 => DefaultAction::Ignore,
            // SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU
            19..=22 => DefaultAction::Stop,
            18 => DefaultAction::Continue,
            _ => DefaultAction::Terminate,
        }
    }

    /// SIGKILL e SIGSTOP não podem ser capturados nem ignorados.
    pub fn is_uncatchable(self) -> bool {
        self.0 == 9 || self.0 == 19
    }
}

impl fmt::Debug for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => write!(f, "SIG{n}"),
            None => write!(f, "Signal({})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_and_signal_tables_come_from_the_real_system() {
        assert_eq!(Errno::ENOENT.0, 2);
        assert_eq!(Errno::ENOENT.message(), "No such file or directory");
        assert_eq!(Errno(2).name(), Some("ENOENT"));
        assert_eq!(Errno(9999).message(), "Unknown error 9999");
        assert_eq!(Signal::SIGKILL.0, 9);
        assert_eq!(Signal::SIGKILL.description(), "Killed");
        assert_eq!(Signal::SIGPIPE.name().as_deref(), Some("PIPE"));
        assert_eq!(Signal::parse("term"), Some(Signal::SIGTERM));
        assert_eq!(Signal::parse("SIGINT"), Some(Signal::SIGINT));
        assert_eq!(Signal(36).name().as_deref(), Some("RTMIN+2"));
        assert_eq!(Signal::SIGCHLD.default_action(), DefaultAction::Ignore);
    }
}
