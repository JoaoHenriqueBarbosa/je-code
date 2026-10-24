//! Porte de `src/vs/base/test/common/virtualScheduling/embedding.ts`: o contrato entre a máquina de
//! estados pura do processador e o laço de eventos do host.
//!
//! Um [`Embedding`] é chamado uma vez por passo virtual que produziu progresso, e decide *como* o
//! processador chega ao host antes do próximo passo.
//!
//! # Contrato
//!
//! A cada chamada o embedding DEVE fazer exatamente uma destas:
//!
//! 1. Devolver [`EmbeddingChoice::ContinueSync`] **sem** chamar `then`: o processador repete o laço no
//!    mesmo quadro da pilha.
//! 2. Agendar `then` numa primitiva do host (microtarefa, macrotarefa, quadro de pintura) e devolver
//!    [`EmbeddingChoice::CbScheduled`]: o processador retorna e espera o callback reentrar no trampolim.
//!
//! O embedding NÃO PODE chamar `then` de forma síncrona e também devolver `CbScheduled` (reentraria no
//! trampolim antes desta chamada terminar), nem devolver `ContinueSync` tendo agendado `then` de forma
//! assíncrona (`then` dispararia depois de o trampolim já ter repetido o laço).
//!
//! Notas de tradução:
//! - `'continueSync' | 'cbScheduled'` é [`EmbeddingChoice`]; o `Embedding` é um `Rc<dyn Fn>` e o `then`
//!   um `Rc<dyn Fn()>` (o trampolim do processador é chamado várias vezes).
//! - Divergência: o ramo `setTimeout0IsFaster` de `nextMacrotask` (o `postMessage` do navegador) não
//!   existe: o `platform` do porte não tem `setTimeout0` (ele é do laço de eventos do JavaScript). Fora do
//!   navegador esse valor é `false` no original, e vale o ramo de `setImmediate` (ou `setTimeout(cb, 0)`).

use std::rc::Rc;

use super::time_api::TimeApi;
use super::virtual_clock::VirtualEvent;

/// O que o embedding escolheu fazer (`'continueSync' | 'cbScheduled'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingChoice {
    ContinueSync,
    CbScheduled,
}

pub type Embedding = Rc<dyn Fn(&VirtualEvent, Rc<dyn Fn()>) -> EmbeddingChoice>;

/// As tarefas nunca agendam por cadeias de promessa. O processador roda os eventos virtuais em
/// sequência num único quadro da pilha: o mais rápido possível, mas deixa o laço do host sem vez
/// enquanto a execução dura.
///
/// Use só em testes sem `await`/`then` entre o agendamento e a execução dos eventos virtuais.
pub fn sync_embedding() -> Embedding {
    Rc::new(|_: &VirtualEvent, _: Rc<dyn Fn()>| EmbeddingChoice::ContinueSync)
}

/// As tarefas podem agendar por `await`/`then`. Entre eventos virtuais cede ao host, para que o *fecho
/// de microtarefas* (a microtarefa atual mais todas as que ela enfileira) esvazie antes do próximo evento.
///
/// É o embedding para quase todos os testes de integração.
pub fn drain_microtasks_embedding(real_api: Rc<dyn TimeApi>) -> Embedding {
    Rc::new(move |next: &VirtualEvent, then: Rc<dyn Fn()>| {
        if next.prefer_real_animation_frame {
            let frame = then.clone();
            if real_api.request_animation_frame(Box::new(move |_| frame())).is_ok() {
                return EmbeddingChoice::CbScheduled;
            }
        }
        next_macrotask(&*real_api, move || then());
        EmbeddingChoice::CbScheduled
    })
}

/// Agenda `cb` depois do fecho da fila de microtarefas atual: `cb` só dispara quando a microtarefa
/// atual E todas as que ela enfileira (recursiva e transitivamente) terminaram.
///
/// Pela especificação do HTML uma macrotarefa só roda com a fila de microtarefas vazia, então qualquer
/// primitiva de macrotarefa serve. Escolhe a mais rápida disponível no host.
pub fn next_macrotask(api: &dyn TimeApi, cb: impl FnOnce() + 'static) {
    if let Err(cb) = api.set_immediate(Box::new(cb)) {
        api.set_timeout(cb, 0);
    }
}
