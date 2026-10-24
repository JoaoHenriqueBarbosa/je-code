//! Porte de `src/vs/base/test/common/virtualScheduling/globalTimeApi.ts`: troca os globais de tempo do
//! thread pelos de uma [`TimeApi`].
//!
//! Notas de tradução:
//! - Os "globais" do original (`globalThis.setTimeout`, `setInterval`, `Date`, `performance.now`,
//!   `performance.timeOrigin` e, se a API os tem, `requestAnimationFrame`) são a `TimeApi` instalada no
//!   thread (`runtime::time_api`), para a qual `runtime::set_timeout`, `runtime::date_now` e companhia
//!   delegam. Instalar uma API é um só `set_time_api`: o `Object.defineProperty` de `performance.now`
//!   e `performance.timeOrigin`, e a restauração dos descritores anteriores, não têm o que fazer aqui,
//!   porque o que estava instalado volta inteiro.
//! - A API anterior é capturada na instalação ([`capture_global_time_api`]), então instalações aninhadas
//!   se compõem: o descartável restaura o que estava corrente quando `push_global_time_api` foi
//!   chamada, não os valores reais originais.
//! - `originalFn` (o atalho no `setTimeout` instalado que escapa do tempo virtual) e o
//!   `ensureSetTimeoutOriginalFn` não são portados: a API real do loop ([`real_time_api`]) está sempre
//!   à mão e nunca é encoberta, então não há atalho a preservar.
//!
//! [`real_time_api`]: super::time_api::real_time_api

use std::rc::Rc;

use super::time_api::{TimeApi, capture_global_time_api};
use crate::lifecycle::{DisposableRef, plain_disposable};
use crate::runtime;

/// Troca os tempos globais (`setTimeout`, `setInterval`, ..., `Date`, `performance.now`, e
/// `requestAnimationFrame` se a API o tem) pelos de `api`. Devolve um descartável que restaura os
/// anteriores.
pub fn push_global_time_api(api: Rc<dyn TimeApi>) -> DisposableRef {
    let previous = capture_global_time_api();
    runtime::set_time_api(api);
    plain_disposable(move || runtime::set_time_api(previous))
}
