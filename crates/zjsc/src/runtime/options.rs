//! Porte de `JavaScriptCore/runtime/Options.h` e `Options.cpp` (a parte dos valores padrão).
//!
//! A lista de opções (`OptionsList.h`) é gerada por `scripts/gen-options.py` em
//! `crate::runtime::options_list`: a struct `Options`, o `Default` com os padrões do C++, um
//! acessor e um setter por opção, e a tabela `OPTIONS_TABLE`. Este módulo tem o que o gerado
//! chama: as funções de valor padrão calculado (`computeNumberOfWorkerThreads` e as irmãs), os
//! tipos `OptionRange`, `OSLogType` e `GCLogLevel`, e o acesso global.
//!
//! Modelo (CONVENTIONS, item 1): as opções são por thread (`thread_local!` com `RefCell`), como a
//! tabela de átomos. No C++ elas vivem no `JSC::Config` do processo; aqui cada thread que roda o
//! motor começa com os padrões e `Options::set_*` altera a cópia da thread.
//!
//! Não portado ainda: `Options::initialize` (variáveis de ambiente `JSC_*`, `configFile`,
//! correções de sanidade) e a proteção de escrita depois do primeiro `VM`.

use std::cell::RefCell;
use std::sync::OnceLock;

pub use super::options_list::{Options, OPTIONS_ALIASES, OPTIONS_TABLE, NUMBER_OF_OPTIONS};

/// `MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS`.
pub const MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS: i32 = 8;

/// `Options::Type`: o tipo C++ de cada opção, para quem lista a tabela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionType {
    Bool,
    Unsigned,
    Double,
    Int32,
    Size,
    OptionRange,
    OptionString,
    GCLogLevel,
    OSLogType,
}

/// `Options::Availability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Normal,
    Restricted,
    Configurable,
}

/// Uma linha da tabela de opções (o que o `Options::Metadata` do C++ descreve).
#[derive(Clone, Copy, Debug)]
pub struct OptionInfo {
    /// Nome original do C++ (`useJIT`), o que vai depois de `JSC_` no ambiente.
    pub name: &'static str,
    pub option_type: OptionType,
    pub availability: Availability,
    /// `nullptr` no C++ vira `None`.
    pub description: Option<&'static str>,
}

/// Uma entrada do `FOR_EACH_JSC_ALIASED_OPTION`: nome antigo, nome novo e se o valor se inverte.
#[derive(Clone, Copy, Debug)]
pub struct OptionAlias {
    pub name: &'static str,
    pub target: &'static str,
    /// `InvertedOption` (`SameOption` é `false`).
    pub inverted: bool,
}

/// `GCLogging::Level` (heap/GCLogging.h). Sobe para `crate::heap::gc_logging` quando o heap existir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum GCLogLevel {
    None = 0,
    Basic,
    Verbose,
}

/// `enum class OSLogType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OSLogType {
    None,
    Default,
    Info,
    Debug,
    Error,
    Fault,
}

/// `OptionRange::RangeState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RangeState {
    Uninitialized,
    InitError,
    Normal,
    Inverted,
}

/// `OptionRange`: faixa `[!]<low>[:<high>]` de contagens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionRange {
    state: RangeState,
    range_string: Option<String>,
    low_limit: u32,
    high_limit: u32,
}

impl Default for OptionRange {
    fn default() -> Self {
        OptionRange { state: RangeState::Uninitialized, range_string: None, low_limit: 0, high_limit: 0 }
    }
}

/// O `sscanf("%u")`: pula espaços, aceita sinal opcional e dígitos, e devolve o valor (com a
/// negação módulo 2^32 do `strtoul`) e o resto da entrada.
fn scan_unsigned(input: &[u8]) -> Option<(u32, &[u8])> {
    let mut i = 0;
    while i < input.len() && matches!(input[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut negative = false;
    if i < input.len() && (input[i] == b'+' || input[i] == b'-') {
        negative = input[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut value: u64 = 0;
    while i < input.len() && input[i].is_ascii_digit() {
        value = value.saturating_mul(10).saturating_add(u64::from(input[i] - b'0'));
        i += 1;
    }
    if i == start {
        return None;
    }
    let value = if value > u64::from(u32::MAX) { u32::MAX } else { value as u32 };
    Some((if negative { value.wrapping_neg() } else { value }, &input[i..]))
}

impl OptionRange {
    /// `OptionRange::s_nullRangeStr`.
    pub const NULL_RANGE_STR: &'static str = "<null>";

    /// `OptionRange::init`: devolve `false` quando a faixa é inválida.
    pub fn init(&mut self, range_string: Option<&str>) -> bool {
        let Some(range_string) = range_string else {
            self.state = RangeState::InitError;
            return false;
        };
        if range_string == Self::NULL_RANGE_STR {
            self.state = RangeState::Uninitialized;
            return true;
        }
        let mut bytes = range_string.as_bytes();
        let mut invert = false;
        if bytes.first() == Some(&b'!') {
            invert = true;
            bytes = &bytes[1..];
        }
        // sscanf(p, " %u:%u", &low, &high)
        let Some((low, rest)) = scan_unsigned(bytes) else {
            self.state = RangeState::InitError;
            return false;
        };
        self.low_limit = low;
        let second = match rest.split_first() {
            Some((b':', after)) => scan_unsigned(after),
            _ => None,
        };
        match second {
            Some((high, _)) => self.high_limit = high,
            None => self.high_limit = self.low_limit,
        }
        if self.low_limit > self.high_limit {
            self.state = RangeState::InitError;
            return false;
        }
        self.range_string = Some(range_string.to_owned());
        self.state = if invert { RangeState::Inverted } else { RangeState::Normal };
        true
    }

    /// `OptionRange::isInRange`.
    pub fn is_in_range(&self, count: u32) -> bool {
        if self.state < RangeState::Normal {
            return true;
        }
        if self.low_limit <= count && count <= self.high_limit {
            return self.state == RangeState::Normal;
        }
        self.state != RangeState::Normal
    }

    /// `OptionRange::rangeString`.
    pub fn range_string(&self) -> &str {
        match (&self.range_string, self.state > RangeState::InitError) {
            (Some(text), true) => text,
            _ => Self::NULL_RANGE_STR,
        }
    }
}

/// `kernTCSMAwareNumberOfProcessorCores` (assembler/CPU.cpp): no Linux `isKernTCSMAvailable` não
/// existe, então é o `WTF::numberOfProcessorCores`: o ambiente `WTF_numberOfProcessorCores` ou o
/// número de CPUs disponíveis ao processo (afinidade e cgroup, como o `sysconf` mais o
/// `sched_getaffinity` do C++).
fn kern_tcsm_aware_number_of_processor_cores() -> i32 {
    static CORES: OnceLock<i32> = OnceLock::new();
    *CORES.get_or_init(|| {
        if let Some(text) = crate::runtime::host::host().getenv("WTF_numberOfProcessorCores") {
            if let Ok(parsed) = text.parse::<u32>() {
                return parsed as i32;
            }
            crate::runtime::host::eprintln(&format!("WARNING: failed to parse WTF_numberOfProcessorCores={text}"));
        }
        crate::runtime::host::host().cpus() as i32
    })
}

/// `ASSERT_ENABLED`: o porte compila sem as asserções do C++ (CONVENTIONS, regras gerais).
pub const fn assert_enabled() -> bool {
    false
}

/// `Options::defaultTCSMValue`.
pub fn default_tcsm_value() -> bool {
    true
}

/// `Options::computeNumberOfWorkerThreads`.
pub fn compute_number_of_worker_threads(max_number_of_worker_threads: i32, minimum: i32) -> u32 {
    let cpus_to_use = kern_tcsm_aware_number_of_processor_cores().min(max_number_of_worker_threads);
    let cpus_to_use = cpus_to_use.max(minimum);
    // `if constexpr (!isDarwin())`: Linux limita a 32.
    cpus_to_use.min(32) as u32
}

/// `Options::computePriorityDeltaOfWorkerThreads`.
pub fn compute_priority_delta_of_worker_threads(two_core_priority_delta: i32, multi_core_priority_delta: i32) -> i32 {
    if kern_tcsm_aware_number_of_processor_cores() <= 2 {
        return two_core_priority_delta;
    }
    multi_core_priority_delta
}

/// `Options::computeNumberOfGCMarkers`.
pub fn compute_number_of_gc_markers(max_number_of_gc_markers: u32) -> u32 {
    compute_number_of_worker_threads(max_number_of_gc_markers as i32, 1)
}

/// `Options::jitEnabledByDefault`: `isAddress64Bit()`.
pub const fn jit_enabled_by_default() -> bool {
    cfg!(target_pointer_width = "64")
}

/// `Options::ipintEnabledByDefault`: `isARM64() || isARM64E() || isX86_64()`.
pub const fn ipint_enabled_by_default() -> bool {
    cfg!(any(target_arch = "aarch64", target_arch = "x86_64"))
}

/// `Options::defaultQuickDFGTierUpThresholdFactor` (fora do `PLATFORM(MAC)`).
pub fn default_quick_dfg_tier_up_threshold_factor() -> f64 {
    0.2
}

/// `Options::defaultRelaxedProfileCoverageFactorForQuickDFGTierUp` (fora do `PLATFORM(MAC)`).
pub fn default_relaxed_profile_coverage_factor_for_quick_dfg_tier_up() -> f64 {
    1.0
}

/// `Options::defaultQuickFTLTierUpThresholdFactor` (fora do `PLATFORM(MAC)`).
pub fn default_quick_ftl_tier_up_threshold_factor() -> f64 {
    1.0
}

/// `canUseWasm`: `ENABLE(WEBASSEMBLY)` vale no `cmakeconfig.h`.
pub fn can_use_wasm() -> bool {
    true
}

/// `canUseJITCage`: sem `ENABLE(JIT_CAGE)` no Linux.
pub fn can_use_jit_cage() -> bool {
    false
}

/// `hasCapacityToUseLargeGigacage`: `Gigacage::hasCapacityToUseLargeGigacage` é `true` fora de
/// iOS e de CPU de 32 bits.
pub fn has_capacity_to_use_large_gigacage() -> bool {
    cfg!(target_pointer_width = "64")
}

thread_local! {
    /// As opções desta thread, começando nos padrões do `FOR_EACH_JSC_OPTION`.
    static OPTIONS: RefCell<Options> = RefCell::new(Options::default());
}

impl Options {
    /// Lê as opções da thread atual.
    pub fn with<R>(reader: impl FnOnce(&Options) -> R) -> R {
        OPTIONS.with(|options| reader(&options.borrow()))
    }

    /// Altera as opções da thread atual (o `Options::useJIT() = false;` do C++).
    pub fn with_mut<R>(writer: impl FnOnce(&mut Options) -> R) -> R {
        OPTIONS.with(|options| writer(&mut options.borrow_mut()))
    }

    /// Volta todas as opções da thread aos padrões.
    pub fn reset() {
        Options::with_mut(|options| *options = Options::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_range_parses_like_the_cpp() {
        let mut range = OptionRange::default();
        assert!(range.is_in_range(7));
        assert!(range.init(Some("2:5")));
        assert!(range.is_in_range(2) && range.is_in_range(5) && !range.is_in_range(6));
        assert!(range.init(Some("!3")));
        assert!(!range.is_in_range(3) && range.is_in_range(4));
        assert!(!range.init(Some("5:2")));
        assert!(!range.init(Some("x")));
        assert!(!range.init(None));
        assert_eq!(range.range_string(), "<null>");
    }

    #[test]
    fn defaults_come_from_the_option_list() {
        assert!(Options::use_ll_int());
        assert!(!Options::expose_private_identifiers());
        assert_eq!(Options::reserved_zone_size(), 64 * 1024);
        Options::set_use_ll_int(false);
        assert!(!Options::use_ll_int());
        Options::reset();
        assert!(Options::use_ll_int());
    }
}
