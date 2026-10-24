//! Parâmetros de compressão: tabela `ZSTD_defaultCParameters` (clevels.h) e `ZSTD_adjustCParams_internal`.

/// Estratégias do libzstd (só as que as fatias já cobrem são consumidas; o resto existe na tabela).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strategy {
    Fast,
    DFast,
    Greedy,
    Lazy,
    Lazy2,
    BtLazy2,
    BtOpt,
    BtUltra,
    BtUltra2,
}

/// `ZSTD_compressionParameters`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CParams {
    pub window_log: u32,
    pub chain_log: u32,
    pub hash_log: u32,
    pub search_log: u32,
    pub min_match: u32,
    pub target_length: u32,
    pub strategy: Strategy,
}

const fn cp(w: u32, c: u32, h: u32, s: u32, l: u32, t: u32, strategy: Strategy) -> CParams {
    CParams { window_log: w, chain_log: c, hash_log: h, search_log: s, min_match: l, target_length: t, strategy }
}

/// Linha do nível 3 em cada uma das quatro tabelas de `ZSTD_defaultCParameters`
/// (índice 0: entrada > 256 KiB ou desconhecida; 1: até 256 KiB; 2: até 128 KiB; 3: até 16 KiB).
const LEVEL3_BY_TABLE: [CParams; 4] = [
    cp(21, 16, 17, 1, 5, 0, Strategy::DFast),
    cp(18, 16, 16, 1, 4, 0, Strategy::DFast),
    cp(17, 15, 16, 2, 5, 0, Strategy::DFast),
    cp(14, 14, 15, 2, 4, 0, Strategy::DFast),
];

pub const ZSTD_HASHLOG_MIN: u32 = 6;
pub const ZSTD_WINDOWLOG_ABSOLUTE_MIN: u32 = 10;
pub const ZSTD_CHAINLOG_MIN: u32 = 6;

/// Nível de compressão que o bun usa (`ZSTD_CLEVEL_DEFAULT`).
pub const DEFAULT_LEVEL: i32 = 3;

/// Índice da tabela por tamanho de entrada (`tableID` de `ZSTD_getCParams_internal`); `None` é tamanho
/// desconhecido (`ZSTD_CONTENTSIZE_UNKNOWN`), que cai na tabela 0.
fn table_id(src_size: Option<u64>) -> usize {
    match src_size {
        None => 0,
        Some(n) => usize::from(n <= 256 * 1024) + usize::from(n <= 128 * 1024) + usize::from(n <= 16 * 1024),
    }
}

pub(super) fn highbit32(v: u32) -> u32 {
    31 - v.leading_zeros()
}

/// `ZSTD_getCParams(3, src_size, 0)`: tabela do nível 3 e ajuste ao tamanho da entrada.
/// `src_size` é o tamanho prometido (`None` quando desconhecido, como no streaming do bun).
pub fn level3_cparams(src_size: Option<u64>) -> CParams {
    adjust(LEVEL3_BY_TABLE[table_id(src_size)], src_size)
}

/// `ZSTD_adjustCParams_internal` com `dictSize == 0` e modo `ZSTD_cpm_unknown`.
fn adjust(mut c: CParams, src_size: Option<u64>) -> CParams {
    let max_window_resize: u64 = 1u64 << 30;
    if let Some(size) = src_size {
        if size <= max_window_resize {
            let src_log = if size < (1u64 << ZSTD_HASHLOG_MIN) { ZSTD_HASHLOG_MIN } else { highbit32((size - 1) as u32) + 1 };
            if c.window_log > src_log {
                c.window_log = src_log;
            }
        }
    }
    // Só com tamanho conhecido (`srcSize != ZSTD_CONTENTSIZE_UNKNOWN`); `dictAndWindowLog` é o `window_log` sem dicionário.
    if src_size.is_some() {
        if c.hash_log > c.window_log + 1 {
            c.hash_log = c.window_log + 1;
        }
        let cycle_log = c.chain_log - u32::from(matches!(c.strategy, Strategy::BtLazy2 | Strategy::BtOpt | Strategy::BtUltra | Strategy::BtUltra2));
        if cycle_log > c.window_log {
            c.chain_log -= cycle_log - c.window_log;
        }
    }
    if c.window_log < ZSTD_WINDOWLOG_ABSOLUTE_MIN {
        c.window_log = ZSTD_WINDOWLOG_ABSOLUTE_MIN;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_size_uses_two_mib_window() {
        let c = level3_cparams(None);
        assert_eq!((c.window_log, c.chain_log, c.hash_log, c.min_match), (21, 16, 17, 5));
    }
}
