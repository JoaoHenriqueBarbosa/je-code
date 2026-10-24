//! Porte dos comandos internos que a API `vscode.prepareCallHierarchy`, `vscode.provideIncomingCalls`,
//! `vscode.provideOutgoingCalls`, `vscode.prepareTypeHierarchy`, `vscode.provideSupertypes` e `vscode.provideSubtypes` chamam (ver
//! [`ext_host_api_commands`](super::ext_host_api_commands)): `_executePrepareCallHierarchy`, `_executeProvideIncomingCalls` e
//! `_executeProvideOutgoingCalls` (`workbench/contrib/callHierarchy/common/callHierarchy.ts`) e `_executePrepareTypeHierarchy`,
//! `_executeProvideSupertypes` e `_executeProvideSubtypes` (`workbench/contrib/typeHierarchy/common/typeHierarchy.ts`).
//!
//! Decisões de tradução:
//! - `CallHierarchyModel` e `TypeHierarchyModel` são duas cópias de uma mesma classe no original (só mudam os nomes dos métodos do
//!   provedor); aqui são um só [`HierarchyModel`], genérico no provedor, e o registro dos comandos de cada hierarquia é o mesmo
//!   código ([`register_prepare_command`] e [`register_resolve_command`]) com a diferença virando parâmetro. O `fork(item)` e o
//!   `ref.acquire()` não têm chamador no porte e não foram portados.
//! - `_models` (o `Map` do módulo, que o `forEach` poda quando passa de dez modelos) é o [`ModelStore`], um vetor de pares na ordem
//!   de inserção: `set` de uma chave que já existe troca o valor e mantém a posição, como o `Map`. Como o estado do módulo, ele
//!   vive tanto quanto o registro dos comandos (cada registro tem o seu).
//! - `CallHierarchyItem` e `TypeHierarchyItem` são o [`HierarchyItemDto`] (o formato é o mesmo). O resultado de cada comando é o
//!   [`Dynamic`] que o RPC entregaria: o item é o objeto de [`HierarchyItemDto::to_dynamic`] e a chamada é `{ from | to, fromRanges }`.
//! - `isCallHierarchyItemDto` devolve sempre `true` no original; `isTypeHierarchyItemDto` confere `name`, `kind`, `uri` e os dois
//!   intervalos. O item que o comando repassa ao provedor é lido com [`HierarchyItemDto::from_dynamic`]: um item sem os
//!   identificadores não acha modelo e o resultado é a lista vazia, e um item que acha modelo mas não tem a forma rejeita a
//!   promessa (o provedor do original receberia o objeto torto).
//! - `_models.forEach(...)` com `delete` durante a iteração visita cada entrada uma vez, na ordem: é o laço sobre a cópia das
//!   chaves de [`ModelStore::prune`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self as errors, Error, on_unexpected_external_error};
use je_base::lifecycle::{DisposableRef, RefCountedDisposable};
use je_base::promise::Promise;
use je_base::types::assert_type;
use je_editor::editor_extensions::I_TEXT_MODEL_SERVICE;
use je_editor::language_providers::ProviderResult;
use je_editor::position::Position;
use je_editor::services_model::I_MODEL_SERVICE;
use je_editor::text_model::TextModel;
use je_platform::commands::commands::{CommandHandler, commands_registry};

use super::editor_execute_commands::{position_argument, resource_argument};
use super::ext_host_type_converters::HierarchyItemDto;
use super::ext_host_type_converters::symbols::{IncomingCallDto, OutgoingCallDto};
use super::ext_host_values::is_instance_of;
use crate::api::browser::main_thread_commands::promise_to_dynamic;
use crate::api::browser::main_thread_language_features::registries::{
    CallHierarchyProvider, TypeHierarchyProvider, call_hierarchy_provider_registry, type_hierarchy_provider_registry,
};

/// A fatia de [`HierarchyItemDto`] que o `prepare*` do provedor devolve: as raízes e o que descarta a sessão.
type PreparedSession = (Vec<HierarchyItemDto>, DisposableRef);

/// `CallHierarchyModel` e `TypeHierarchyModel`.
struct HierarchyModel<P: ?Sized> {
    id: String,
    provider: Rc<P>,
    roots: Vec<HierarchyItemDto>,
    reference: RefCountedDisposable,
}

impl<P: ?Sized + 'static> HierarchyModel<P> {
    /// `static async create(model, position, token)`: o primeiro provedor da lista (ordenada) prepara a sessão; sem provedor ou
    /// sem sessão não há modelo.
    fn create(providers: Vec<Rc<P>>, prepare: impl FnOnce(&Rc<P>) -> ProviderResult<PreparedSession> + 'static) -> Promise<Option<Rc<HierarchyModel<P>>>> {
        let Some(provider) = providers.into_iter().next() else {
            return Promise::resolved(None);
        };
        let session = prepare(&provider);
        async_fn(async move {
            let Some((roots, session)) = session.await? else {
                return Ok(None);
            };
            let id = roots.iter().fold(String::new(), |id, root| id + &root.session_id);
            Ok(Some(Rc::new(HierarchyModel { id, provider, roots, reference: RefCountedDisposable::new(session) })))
        })
    }

    /// `dispose()`.
    fn dispose(&self) {
        self.reference.release();
    }
}

/// `_models`: o `Map<string, Model>` do módulo.
struct ModelStore<P: ?Sized> {
    models: RefCell<Vec<(String, Rc<HierarchyModel<P>>)>>,
}

impl<P: ?Sized + 'static> ModelStore<P> {
    fn new() -> Rc<Self> {
        Rc::new(Self { models: RefCell::new(Vec::new()) })
    }

    /// `_models.set(key, model)`.
    fn set(&self, key: &str, model: &Rc<HierarchyModel<P>>) {
        let mut models = self.models.borrow_mut();
        match models.iter_mut().find(|(candidate, _)| candidate == key) {
            Some(entry) => entry.1 = Rc::clone(model),
            None => models.push((key.to_owned(), Rc::clone(model))),
        }
    }

    /// `_models.get(key)`.
    fn get(&self, key: &str) -> Option<Rc<HierarchyModel<P>>> {
        self.models.borrow().iter().find(|(candidate, _)| candidate == key).map(|(_, model)| Rc::clone(model))
    }

    /// `_models.forEach((value, key, map) => { if (map.size > 10) { value.dispose(); _models.delete(key); } })`.
    fn prune(&self) {
        let keys: Vec<String> = self.models.borrow().iter().map(|(key, _)| key.clone()).collect();
        for key in keys {
            let removed = {
                let mut models = self.models.borrow_mut();
                if models.len() > 10 { models.iter().position(|(candidate, _)| *candidate == key).map(|index| models.remove(index).1) } else { None }
            };
            if let Some(model) = removed {
                model.dispose();
            }
        }
    }
}

/// `{ ... } as IncomingCall[] | OutgoingCall[] | TypeHierarchyItem[]` do resultado do provedor: a lista só vale não vazia
/// (`isNonEmptyArray`), e um erro do provedor é reportado e vira a lista vazia.
fn resolve_items<T: Clone + 'static>(result: ProviderResult<Vec<T>>) -> Promise<Vec<T>> {
    async_fn(async move {
        match result.await {
            Ok(Some(items)) if !items.is_empty() => Ok(items),
            Ok(_) => Ok(Vec::new()),
            Err(error) => {
                on_unexpected_external_error(&error);
                Ok(Vec::new())
            }
        }
    })
}

/// `CommandsRegistry.registerCommand('_executePrepare*Hierarchy', ...)`: `providers` é a lista ordenada do registro para o modelo,
/// `prepare` pede a sessão ao provedor e `publish` guarda o modelo no `_models` (cada hierarquia o faz de um jeito) e devolve o
/// resultado do comando.
fn register_prepare_command<P: ?Sized + 'static>(
    id: &str,
    store: Rc<ModelStore<P>>,
    providers: impl Fn(&TextModel) -> Vec<Rc<P>> + 'static,
    prepare: impl Fn(&Rc<P>, &Rc<TextModel>, Position) -> ProviderResult<PreparedSession> + 'static,
    publish: impl Fn(&ModelStore<P>, &Rc<HierarchyModel<P>>) -> Vec<Dynamic> + 'static,
) {
    let (providers, prepare, publish) = (Rc::new(providers), Rc::new(prepare), Rc::new(publish));
    let handler: CommandHandler = Rc::new(move |accessor, args| {
        let resource = resource_argument(args);
        let position = position_argument(args.get(1));

        let model_service = accessor.get(I_MODEL_SERVICE);
        let text_model_service = accessor.get(I_TEXT_MODEL_SERVICE);
        let (store, providers, prepare, publish) = (Rc::clone(&store), Rc::clone(&providers), Rc::clone(&prepare), Rc::clone(&publish));

        promise_to_dynamic(async_fn(async move {
            let mut text_model_reference = None;
            let text_model = match model_service.get_model(&resource) {
                Some(text_model) => text_model,
                None => {
                    let reference = text_model_service.create_model_reference(&resource).await?;
                    let text_model = Rc::clone(&reference.object);
                    text_model_reference = Some(reference);
                    text_model
                }
            };

            let providers = providers(&text_model);
            let model = {
                let text_model = Rc::clone(&text_model);
                let prepare = Rc::clone(&prepare);
                HierarchyModel::create(providers, move |provider| prepare(provider, &text_model, position)).await
            };
            let result = model.map(|model| match model {
                Some(model) => Dynamic::array(publish(&store, &model)),
                None => Dynamic::array(Vec::<Dynamic>::new()),
            });
            if let Some(reference) = text_model_reference {
                reference.dispose();
            }
            result
        }))
    });
    commands_registry().register_command_handler(id, Some(handler));
}

/// `CommandsRegistry.registerCommand('_executeProvide*', ...)`: acha o modelo pelo `_sessionId` do item (sem modelo, a lista vazia)
/// e pede ao provedor, por `resolve`, o que o comando entrega. `is_item_dto` é o `assertType(isXItemDto(item))`.
fn register_resolve_command<P: ?Sized + 'static>(
    id: &str,
    store: Rc<ModelStore<P>>,
    is_item_dto: fn(&Dynamic) -> bool,
    resolve: impl Fn(&Rc<HierarchyModel<P>>, &HierarchyItemDto) -> Promise<Vec<Dynamic>> + 'static,
) {
    let resolve = Rc::new(resolve);
    let handler: CommandHandler = Rc::new(move |_accessor, args| {
        let item = args.first().cloned().unwrap_or(Dynamic::Undefined);
        assert_type(&is_item_dto(&item), None);

        // find model: `item._sessionId` de um item ausente lança
        if item.is_nullish() {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading '_sessionId')"));
        }
        let session_id = match item.get_str("_sessionId") {
            Dynamic::String(session_id) => session_id.to_string_lossy(),
            _ => String::new(),
        };
        let Some(model) = store.get(&session_id) else {
            return promise_to_dynamic(Promise::resolved(Dynamic::array(Vec::<Dynamic>::new())));
        };
        let resolve = Rc::clone(&resolve);
        promise_to_dynamic(async_fn(async move {
            let Some(item) = HierarchyItemDto::from_dynamic(&item) else {
                return Err(Error::type_error("expected a hierarchy item"));
            };
            Ok(Dynamic::array(resolve(&model, &item).await?))
        }))
    });
    commands_registry().register_command_handler(id, Some(handler));
}

/// `isTypeHierarchyItemDto(obj)`.
fn is_type_hierarchy_item_dto(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Object(_))
        && matches!(value.get_str("name"), Dynamic::String(_))
        && matches!(value.get_str("kind"), Dynamic::Number(_))
        && is_instance_of(&value.get_str("uri"), "Uri")
        && super::ext_host_type_converters::Range::from_dto(&value.get_str("range")).is_some()
        && super::ext_host_type_converters::Range::from_dto(&value.get_str("selectionRange")).is_some()
}

/// `isCallHierarchyItemDto(obj)`: sempre verdadeiro.
fn is_call_hierarchy_item_dto(_value: &Dynamic) -> bool {
    true
}

/// O `HierarchyItemDto` como o `CallHierarchyItem`/`TypeHierarchyItem` do resultado do comando.
fn item_to_dynamic(item: &HierarchyItemDto) -> Dynamic {
    item.to_dynamic()
}

/// `callHierarchy.ts`: os três comandos de hierarquia de chamadas.
fn register_call_hierarchy_commands() {
    let store: Rc<ModelStore<dyn CallHierarchyProvider>> = ModelStore::new();

    register_prepare_command(
        "_executePrepareCallHierarchy",
        Rc::clone(&store),
        |model| call_hierarchy_provider_registry().ordered(model, false),
        |provider, model, position| {
            provider.prepare_call_hierarchy(model, position, &CancellationToken::None).then(|session| session.map(|session| (session.roots.clone(), session as DisposableRef)))
        },
        |store, model| {
            // `_models.set(model.id, model); _models.forEach(...); return [model.root]`
            store.set(&model.id, model);
            store.prune();
            vec![model.roots.first().map_or(Dynamic::Undefined, item_to_dynamic)]
        },
    );
    register_resolve_command(
        "_executeProvideIncomingCalls",
        Rc::clone(&store),
        is_call_hierarchy_item_dto,
        |model, item| {
            let calls = resolve_items(model.provider.provide_incoming_calls(item, &CancellationToken::None));
            calls.then(|calls: Vec<IncomingCallDto>| calls.iter().map(IncomingCallDto::to_dynamic).collect())
        },
    );
    register_resolve_command(
        "_executeProvideOutgoingCalls",
        store,
        is_call_hierarchy_item_dto,
        |model, item| {
            let calls = resolve_items(model.provider.provide_outgoing_calls(item, &CancellationToken::None));
            calls.then(|calls: Vec<OutgoingCallDto>| calls.iter().map(OutgoingCallDto::to_dynamic).collect())
        },
    );
}

/// `typeHierarchy.ts`: os três comandos de hierarquia de tipos.
fn register_type_hierarchy_commands() {
    let store: Rc<ModelStore<dyn TypeHierarchyProvider>> = ModelStore::new();

    register_prepare_command(
        "_executePrepareTypeHierarchy",
        Rc::clone(&store),
        |model| type_hierarchy_provider_registry().ordered(model, false),
        |provider, model, position| {
            provider.prepare_type_hierarchy(model, position, &CancellationToken::None).then(|session| session.map(|session| (session.roots.clone(), session as DisposableRef)))
        },
        |store, model| {
            // `_models.forEach(...); for (const root of model.roots) _models.set(root._sessionId, model); return model.roots`
            store.prune();
            for root in &model.roots {
                store.set(&root.session_id, model);
            }
            model.roots.iter().map(item_to_dynamic).collect()
        },
    );
    register_resolve_command(
        "_executeProvideSupertypes",
        Rc::clone(&store),
        is_type_hierarchy_item_dto,
        |model, item| {
            resolve_items(model.provider.provide_supertypes(item, &CancellationToken::None)).then(|items| items.iter().map(item_to_dynamic).collect())
        },
    );
    register_resolve_command(
        "_executeProvideSubtypes",
        store,
        is_type_hierarchy_item_dto,
        |model, item| resolve_items(model.provider.provide_subtypes(item, &CancellationToken::None)).then(|items| items.iter().map(item_to_dynamic).collect()),
    );
}

/// A carga dos módulos `callHierarchy.ts` e `typeHierarchy.ts`, que registram os comandos `_execute*` das duas hierarquias.
pub fn register_hierarchy_execute_commands() {
    register_call_hierarchy_commands();
    register_type_hierarchy_commands();
}
