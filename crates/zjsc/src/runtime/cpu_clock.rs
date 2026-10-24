//! Tempo virtual de CPU (o navegador sob `--virtual-time-budget`): no Chromium o relógio virtual anda também com o
//! trabalho do renderer, não só com os timers. Com o custo ligado, cada instrução de bytecode executada vale
//! `ns_per_instruction` de relógio virtual, e cada byte de fonte que o hospedeiro entrega para compilar vale
//! `ns_per_source_byte`. A cobrança é preguiçosa: o trabalho acumulado entra no relógio na próxima leitura
//! (`virtual_now`) ou no próximo salto dele, então um timer que vence durante uma tarefa só dispara depois dela, como
//! num navegador ocupado. O contador é determinístico (o mesmo programa conta as mesmas instruções).

use std::cell::Cell;

thread_local! {
    /// Instruções de bytecode executadas pelo programa.
    static EXECUTED: Cell<u64> = const { Cell::new(0) };
    /// Quantas delas já entraram no relógio.
    static CHARGED: Cell<u64> = const { Cell::new(0) };
    /// Nanossegundos virtuais por instrução e por byte de fonte; `None` é o relógio só dos timers.
    static COST: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
    /// Milissegundos de compilação ainda não somados ao relógio.
    static PENDING_MS: Cell<f64> = const { Cell::new(0.0) };
}

/// Uma instrução executada (o laço do interpretador chama a cada instrução).
#[inline(always)]
pub(crate) fn count_instruction() {
    EXECUTED.with(|executed| executed.set(executed.get() + 1));
}

/// Liga o tempo virtual de CPU do programa com os custos dados.
pub fn set_cpu_cost(ns_per_instruction: f64, ns_per_source_byte: f64) {
    let executed = EXECUTED.with(Cell::get);
    CHARGED.with(|charged| charged.set(executed));
    COST.with(|cost| cost.set(Some((ns_per_instruction, ns_per_source_byte))));
}

/// O hospedeiro vai compilar `bytes` de fonte (script ou módulo): o custo entra na próxima cobrança.
pub fn charge_source_bytes(bytes: usize) {
    if let Some((_, per_byte)) = COST.with(Cell::get) {
        PENDING_MS.with(|pending| pending.set(pending.get() + bytes as f64 * per_byte / 1_000_000.0));
    }
}

/// O trabalho acumulado desde a última cobrança, em milissegundos virtuais (zero sem custo ligado).
pub(crate) fn take_pending_ms() -> f64 {
    let Some((per_instruction, _)) = COST.with(Cell::get) else { return 0.0 };
    let executed = EXECUTED.with(Cell::get);
    let instructions = executed - CHARGED.with(|charged| charged.replace(executed));
    instructions as f64 * per_instruction / 1_000_000.0 + PENDING_MS.with(|pending| pending.replace(0.0))
}

/// Fim do programa.
pub(crate) fn reset_for_program() {
    let _ = EXECUTED.try_with(|executed| executed.set(0));
    let _ = CHARGED.try_with(|charged| charged.set(0));
    let _ = COST.try_with(|cost| cost.set(None));
    let _ = PENDING_MS.try_with(|pending| pending.set(0.0));
}
