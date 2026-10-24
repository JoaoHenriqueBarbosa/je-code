//! Porte do trecho de `src/vs/workbench/common/editor.ts` que guarda os serializadores de editor: `IEditorSerializer`,
//! `IEditorFactoryRegistry` e a classe `EditorFactoryRegistry` (o registro de `EditorExtensions.EditorFactory`), que o
//! `EditorGroupModel` consulta para serializar e restaurar os editores de um grupo.
//!
//! Divergências:
//! - **Registro global.** `Registry.add(EditorExtensions.EditorFactory, new EditorFactoryRegistry())` roda na carga do módulo
//!   do original. Aqui o registro é criado na primeira chamada de [`editor_factory_registry`], que o grava no `Registry` de
//!   plataforma (`je_platform::registry`) sob o mesmo id e devolve o objeto guardado, como o
//!   `Registry.as<IEditorFactoryRegistry>(EditorExtensions.EditorFactory)` dos consumidores.
//! - **Construtores de serializador.** `registerEditorSerializer(editorTypeId, ctor)` recebe a classe (`new(...Services)`)
//!   e a instancia com `instantiationService.createInstance(ctor)`. Aqui `ctor` é o tipo `S` (um [`Injectable`] sem
//!   argumentos estáticos que implementa [`IEditorSerializer`]): [`EditorFactoryRegistry::register_editor_serializer`] guarda
//!   a função que o instancia (`createInstance::<S>(())`) até o `start` ou, se o registro já começou, instancia na hora.
//! - **`IInstantiationService`.** É o tipo concreto [`InstantiationService`] (ver `je_instantiation`).
//! - **Sobrecargas de `getEditorSerializer`.** `(editor: EditorInput)` é [`EditorFactoryRegistry::get_editor_serializer`] e
//!   `(editorTypeId: string)` é [`EditorFactoryRegistry::get_editor_serializer_by_type_id`].
//! - **`IFileEditorFactory`** (`registerFileEditorFactory` e `getFileEditorFactory`) fica fora: depende do `IFileEditorInput`,
//!   que ainda não foi portado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_instantiation::{I_INSTANTIATION_SERVICE, Injectable, InstantiationService, ServicesAccessor};
use je_platform::registry::registry;

use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorExtensions, EditorInputRef};

/// `IEditorSerializer`.
pub trait IEditorSerializer {
    /// Determines whether the given editor can be serialized by the serializer.
    fn can_serialize(&self, editor: &EditorInputRef) -> bool;

    /// Returns a string representation of the provided editor that contains enough information to deserialize back to the
    /// original editor from the [`IEditorSerializer::deserialize`] method.
    fn serialize(&self, editor: &EditorInputRef) -> Option<String>;

    /// Returns an editor from the provided serialized form of the editor. This form matches the value returned from the
    /// [`IEditorSerializer::serialize`] method.
    fn deserialize(&self, instantiation_service: &InstantiationService, serialized_editor: &str) -> Option<EditorInputRef>;
}

/// `IConstructorSignature<IEditorSerializer>`: a função que instancia o serializador pelo serviço de instanciação.
type SerializerConstructor = Rc<dyn Fn(&InstantiationService) -> Rc<dyn IEditorSerializer>>;

/// O corpo de `instantiationService.createInstance(ctor)` para o serializador `S`.
fn create_serializer<S: Injectable<Args = ()> + IEditorSerializer>(instantiation_service: &InstantiationService) -> Rc<dyn IEditorSerializer> {
    instantiation_service.create_instance::<S>(())
}

/// `EditorFactoryRegistry` (`IEditorFactoryRegistry`, sem o editor de arquivos).
pub struct EditorFactoryRegistry {
    this: Weak<EditorFactoryRegistry>,
    instantiation_service: RefCell<Option<Rc<InstantiationService>>>,
    /// `editorSerializerConstructors`, na ordem do registro (o `Map` do original guarda a ordem de inserção).
    editor_serializer_constructors: RefCell<Vec<(String, SerializerConstructor)>>,
    editor_serializer_instances: RefCell<HashMap<String, Rc<dyn IEditorSerializer>>>,
}

impl EditorFactoryRegistry {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            instantiation_service: RefCell::new(None),
            editor_serializer_constructors: RefCell::new(Vec::new()),
            editor_serializer_instances: RefCell::new(HashMap::new()),
        })
    }

    /// `start(accessor)`: starts the registry by providing the required services.
    pub fn start(&self, accessor: &ServicesAccessor) {
        let instantiation_service = accessor.get(I_INSTANTIATION_SERVICE);
        *self.instantiation_service.borrow_mut() = Some(instantiation_service.clone());

        let constructors = std::mem::take(&mut *self.editor_serializer_constructors.borrow_mut());
        for (key, ctor) in &constructors {
            self.create_editor_serializer(key, ctor, &instantiation_service);
        }
    }

    fn create_editor_serializer(&self, editor_type_id: &str, ctor: &SerializerConstructor, instantiation_service: &InstantiationService) {
        let instance = ctor(instantiation_service);
        self.editor_serializer_instances.borrow_mut().insert(editor_type_id.to_string(), instance);
    }

    /// `registerEditorSerializer(editorTypeId, ctor)`: registers a editor serializer for the given editor to the registry. An
    /// editor serializer is capable of serializing and deserializing editor from string data.
    pub fn register_editor_serializer<S: Injectable<Args = ()> + IEditorSerializer>(&self, editor_type_id: &str) -> DisposableRef {
        let already_registered = self.editor_serializer_constructors.borrow().iter().any(|(key, _)| key == editor_type_id)
            || self.editor_serializer_instances.borrow().contains_key(editor_type_id);
        if already_registered {
            errors::throw(Error::new(format!("A editor serializer with type ID '{editor_type_id}' was already registered.")));
        }

        let ctor: SerializerConstructor = Rc::new(create_serializer::<S>);
        let instantiation_service = self.instantiation_service.borrow().clone();
        match instantiation_service {
            None => self.editor_serializer_constructors.borrow_mut().push((editor_type_id.to_string(), ctor)),
            Some(instantiation_service) => self.create_editor_serializer(editor_type_id, &ctor, &instantiation_service),
        }

        let this = self.this.clone();
        let editor_type_id = editor_type_id.to_string();
        to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.editor_serializer_constructors.borrow_mut().retain(|(key, _)| *key != editor_type_id);
                this.editor_serializer_instances.borrow_mut().remove(&editor_type_id);
            }
        })
    }

    /// `getEditorSerializer(editor: EditorInput)`: returns the editor serializer for the given editor.
    pub fn get_editor_serializer(&self, editor: &EditorInputRef) -> Option<Rc<dyn IEditorSerializer>> {
        self.get_editor_serializer_by_type_id(&editor.editor_type_id())
    }

    /// `getEditorSerializer(editorTypeId: string)`.
    pub fn get_editor_serializer_by_type_id(&self, editor_type_id: &str) -> Option<Rc<dyn IEditorSerializer>> {
        self.editor_serializer_instances.borrow().get(editor_type_id).cloned()
    }
}

/// `Registry.as<IEditorFactoryRegistry>(EditorExtensions.EditorFactory)`: o registro de fábricas de editor da thread.
pub fn editor_factory_registry() -> Rc<EditorFactoryRegistry> {
    let registry = registry();
    if !registry.knows(EditorExtensions::EDITOR_FACTORY) {
        registry.add(EditorExtensions::EDITOR_FACTORY, EditorFactoryRegistry::new());
    }

    registry.as_::<EditorFactoryRegistry>(EditorExtensions::EDITOR_FACTORY).expect("o registro de fábricas de editor foi adicionado acima")
}
