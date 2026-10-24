//! Porte de `src/vs/editor/contrib/find` do VS Code: a busca e a substituição do editor.
//!
//! - [`find_state`]: `findState.ts` (o estado da busca e da substituição);
//! - [`find_decorations`]: `findDecorations.ts` (as decorações das ocorrências);
//! - [`replace_pattern`]: `replacePattern.ts` (a cadeia de substituição);
//! - [`replace_all_command`]: `replaceAllCommand.ts` (o comando de trocar tudo);
//! - [`find_model`]: `findModel.ts` (o modelo de busca ligado ao editor, as chaves de contexto e os ids);
//! - [`find_controller`]: `findController.ts` (o controlador, as ações e os comandos).
//!
//! `findWidget.ts` e `findOptionsWidget.ts` (e o histórico do widget) são o widget DOM do original: viram
//! especificação (medidas, cores e estados), lida ao portar o widget nativo.

pub mod find_controller;
pub mod find_decorations;
pub mod find_model;
pub mod find_state;
pub mod replace_all_command;
pub mod replace_pattern;

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;

/// `this._register(event(listener))` para um ouvinte que usa o dono (o `this` do original): o `Weak` do dono sobe a cada
/// chamada, e a chamada some se o dono já foi solto. O dono e o evento são do mesmo ciclo de vida, então o dono não
/// fica preso pelo ouvinte.
pub(crate) fn listen_weak<S: 'static, E: 'static>(event: Event<E>, owner: &Rc<S>, handler: impl Fn(&Rc<S>, &E) + 'static) -> DisposableRef {
    let owner = Rc::downgrade(owner);
    event.listen(move |e| {
        if let Some(owner) = owner.upgrade() {
            handler(&owner, e);
        }
    })
}
