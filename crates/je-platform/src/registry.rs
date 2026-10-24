//! Porte de `src/vs/platform/registry/common/platform.ts`: o `Registry` global de contribuições, por id.
//!
//! Decisões de tradução:
//! - **`Registry` é a struct [`Registry`]** (o `RegistryImpl` e a interface `IRegistry` juntos), e o global
//!   `export const Registry` é o registro da thread, devolvido por [`registry`] (um `thread_local`, como os registros
//!   específicos do porte, p.ex. o [`configuration_registry`](crate::configuration::registry::configuration_registry): o
//!   VS Code roda num processo e numa thread de interface só).
//! - **O `data: any` é um [`Rc`] de qualquer tipo `'static`** (`Rc<dyn Any>`). Quem registra guarda o `Rc` e quem lê
//!   pede o tipo ([`Registry::as_`], o `as<T>(id)` do original) e recebe o mesmo `Rc` (a identidade do objeto, que o
//!   teste do original confere com `===`, é [`Rc::ptr_eq`]). Pedir um id com o tipo errado lança (no TypeScript o
//!   `as<T>` é um cast sem verificação, e o erro viria depois; em Rust não há como entregar o valor com outro tipo).
//! - **`Assert.ok(Types.isString(id))` e `Assert.ok(Types.isObject(data))`** são garantidos pelos tipos (`&str` e `Rc<T>`).
//!   O `Assert.ok(!this.data.has(id), 'There is already an extension with this id')` lança [`Error`] (pânico, ver
//!   `docs/design.md`).
//! - **`as` devolve `null` para o id desconhecido**: `None`. O `dispose()` de `RegistryImpl` (descarta os valores que
//!   têm `dispose`) não é alcançável pela interface `IRegistry`, que é o tipo do `Registry` exportado, e não foi portado.
//! - **`as` é palavra reservada**: o método é `as_`, como em `je_base::dynamic`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::assert;
use je_base::errors::{self, Error};

/// `RegistryImpl` / `IRegistry`: as contribuições (extensões das funções e propriedades da plataforma), por id único.
#[derive(Default)]
pub struct Registry {
    data: RefCell<HashMap<String, Rc<dyn Any>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// `add(id, data)`: adiciona a contribuição `data` sob o `id`, que precisa ser único.
    pub fn add<T: Any>(&self, id: &str, data: Rc<T>) {
        assert::ok(&!self.data.borrow().contains_key(id), Some("There is already an extension with this id"));

        self.data.borrow_mut().insert(id.to_owned(), data);
    }

    /// `knows(id)`: há uma contribuição com este id?
    pub fn knows(&self, id: &str) -> bool {
        self.data.borrow().contains_key(id)
    }

    /// `as<T>(id)`: a contribuição do `id`, ou `None` (o `null` do original) se não há. Lança se há, mas de outro tipo.
    pub fn as_<T: Any>(&self, id: &str) -> Option<Rc<T>> {
        let data = self.data.borrow().get(id).cloned()?;
        match data.downcast::<T>() {
            Ok(data) => Some(data),
            Err(_) => errors::throw(Error::new(format!("The extension with id '{id}' is not a {}", std::any::type_name::<T>()))),
        }
    }
}

thread_local! {
    static REGISTRY: Rc<Registry> = Rc::new(Registry::new());
}

/// `export const Registry: IRegistry`: o registro global da thread.
pub fn registry() -> Rc<Registry> {
    REGISTRY.with(Rc::clone)
}
