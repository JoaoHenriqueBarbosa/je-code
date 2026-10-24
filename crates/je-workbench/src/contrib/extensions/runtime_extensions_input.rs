//! Porte de `src/vs/workbench/contrib/extensions/common/runtimeExtensionsInput.ts`: a entrada de editor singleton e modal
//! da lista de extensões em execução (`RuntimeExtensionsInput`).
//!
//! Divergências:
//! - `RuntimeExtensionsInput extends EditorInput` é o [`EditorInputBase`] mais o trait [`EditorInput`]; a entrada nasce em
//!   `Rc::new_cyclic`.
//! - `static _instance` é um `thread_local` com o `Rc` da instância corrente: [`RuntimeExtensionsInput::instance`] cria
//!   outra quando não há nenhuma ou quando a guardada já foi descartada.
//! - `registerIcon('runtime-extensions-editor-label-icon', Codicon.extensions, ...)`: o registro de ícones não foi portado,
//!   então [`runtime_extensions_editor_icon`] devolve o `ThemeIcon` (o que `registerIcon` devolve) sem registrar o padrão e
//!   a descrição.
//! - `matches` (`other instanceof RuntimeExtensionsInput`) é o `downcast_ref` de [`EditorInput::as_any`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};

use crate::common::editor::editor_input::{EditorInput, EditorInputBase, EditorInputIcon, matches_by_default};
use crate::common::editor::{EditorInputCapabilities, EditorInputOrUntyped};

/// `RuntimeExtensionsEditorIcon`.
pub fn runtime_extensions_editor_icon() -> ThemeIcon {
    ThemeIcon::from_id("runtime-extensions-editor-label-icon")
}

thread_local! {
    /// `RuntimeExtensionsInput._instance`.
    static INSTANCE: RefCell<Option<Rc<RuntimeExtensionsInput>>> = const { RefCell::new(None) };
}

pub struct RuntimeExtensionsInput {
    base: EditorInputBase,
    resource: URI,
}

impl RuntimeExtensionsInput {
    /// `RuntimeExtensionsInput.ID`.
    pub const ID: &'static str = "workbench.runtimeExtensions.input";

    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let this: std::rc::Weak<dyn EditorInput> = this.clone();
            Self {
                base: EditorInputBase::new(this),
                resource: URI::from(
                    &UriComponents {
                        scheme: Some(JsString::from("runtime-extensions")),
                        path: Some(JsString::from("default")),
                        ..UriComponents::default()
                    },
                    false,
                ),
            }
        })
    }

    /// `RuntimeExtensionsInput.instance`.
    pub fn instance() -> Rc<RuntimeExtensionsInput> {
        INSTANCE.with(|cell| {
            let mut slot = cell.borrow_mut();
            match slot.as_ref() {
                Some(current) if !current.is_disposed() => current.clone(),
                _ => {
                    let input = RuntimeExtensionsInput::new();
                    *slot = Some(input.clone());
                    input
                }
            }
        })
    }
}

impl IDisposable for RuntimeExtensionsInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for RuntimeExtensionsInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_owned()
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY | EditorInputCapabilities::SINGLETON | EditorInputCapabilities::REQUIRES_MODAL
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn get_name(&self) -> String {
        "Running Extensions".to_owned()
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        Some(EditorInputIcon::Theme(runtime_extensions_editor_icon()))
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        if matches_by_default(self, other) {
            return true;
        }
        match other {
            EditorInputOrUntyped::Typed(other) => other.as_any().is::<RuntimeExtensionsInput>(),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}
