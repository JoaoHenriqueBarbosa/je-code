//! Porte de `bytecompiler/Label.h`.
//!
//! `GenericBoundLabel` guarda no C++ um ponteiro para o `BytecodeGeneratorBase<Traits>` só para
//! ler `m_writer.position()`. Aqui o gerador não é guardado: quem precisa da posição a recebe como
//! `&dyn LabelGenerator` (o gerador implementa o trait), o que dispensa ponteiro cru.
//! `GenericLabel::setLocation` está em `BytecodeGeneratorBaseInlines.h` e vive com o gerador.
//! O refcount intrusivo do `Label` se mantém no objeto (`hasOneRef()` é observado pelo
//! `LabelScope`); `Ref<Label>`/`RefPtr<Label>` viram `GenericLabelRef`.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::marker::PhantomData;
use std::rc::Rc;

/// O que o `BytecodeGeneratorBase` oferece ao `GenericBoundLabel`: `m_writer.position()`.
pub trait LabelGenerator {
    fn writer_position(&self) -> i32;
}

/// `JSGeneratorTraits`.
#[derive(Debug)]
pub struct JSGeneratorTraits;

pub mod wasm {
    use super::GenericLabel;

    /// `Wasm::GeneratorTraits`.
    #[derive(Debug)]
    pub struct GeneratorTraits;

    pub type Label = GenericLabel<GeneratorTraits>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoundType {
    Offset,
    GeneratorForward,
    GeneratorBackward,
}

pub struct GenericBoundLabel<Traits> {
    type_: BoundType,
    saved_target: i32,
    /// `m_label` (união com `m_target`), válido em `GeneratorForward`.
    label: Option<GenericLabelRef<Traits>>,
    /// `m_target`, válido em `Offset` e `GeneratorBackward`.
    target: i32,
}

impl<Traits> GenericBoundLabel<Traits> {
    /// `GenericBoundLabel()`.
    pub fn new() -> Self {
        GenericBoundLabel { type_: BoundType::Offset, saved_target: 0, label: None, target: 0 }
    }

    /// `explicit GenericBoundLabel(int offset)`.
    pub fn from_offset(offset: i32) -> Self {
        GenericBoundLabel { type_: BoundType::Offset, saved_target: 0, label: None, target: offset }
    }

    /// `GenericBoundLabel(BytecodeGenerator*, Label*)`.
    pub fn forward(label: GenericLabelRef<Traits>) -> Self {
        GenericBoundLabel {
            type_: BoundType::GeneratorForward,
            saved_target: 0,
            label: Some(label),
            target: 0,
        }
    }

    /// `GenericBoundLabel(BytecodeGenerator*, int offset)`.
    pub fn backward(offset: i32) -> Self {
        GenericBoundLabel {
            type_: BoundType::GeneratorBackward,
            saved_target: 0,
            label: None,
            target: offset,
        }
    }

    pub fn target(&self, generator: &dyn LabelGenerator) -> i32 {
        match self.type_ {
            BoundType::Offset => self.target,
            BoundType::GeneratorBackward => self.target - generator.writer_position(),
            BoundType::GeneratorForward => 0,
        }
    }

    pub fn save_target(&mut self, generator: &dyn LabelGenerator) -> i32 {
        if self.type_ == BoundType::GeneratorForward {
            self.saved_target = generator.writer_position();
            return 0;
        }
        self.saved_target = self.target(generator);
        self.saved_target
    }

    pub fn commit_target(&mut self) -> i32 {
        if self.type_ == BoundType::GeneratorForward {
            if let Some(label) = &self.label {
                label.borrow_mut().unresolved_jumps.push(self.saved_target);
            }
            return 0;
        }
        self.saved_target
    }
}

impl<Traits> Default for GenericBoundLabel<Traits> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct GenericLabel<Traits> {
    ref_count: i32,
    location: u32,
    bound: Cell<bool>,
    unresolved_jumps: Vec<i32>,
    _traits: PhantomData<fn() -> Traits>,
}

impl<Traits> Default for GenericLabel<Traits> {
    fn default() -> Self {
        GenericLabel {
            ref_count: 0,
            location: Self::INVALID_LOCATION,
            bound: Cell::new(false),
            unresolved_jumps: Vec::new(),
            _traits: PhantomData,
        }
    }
}

impl<Traits> crate::wtf::ref_counted::RefCounted for GenericLabel<Traits> {
    fn ref_count(&self) -> i32 {
        self.ref_count
    }
}

impl<Traits> GenericLabel<Traits> {
    const INVALID_LOCATION: u32 = u32::MAX;

    /// Escrita de `m_location` usada por `setLocation` (em `BytecodeGeneratorBaseInlines.h`).
    pub fn set_location_raw(&mut self, location: u32) {
        self.location = location;
    }

    /// `bind(BytecodeGenerator*)`; o `label` é a referência a este mesmo rótulo.
    pub fn bind_generator(&mut self, label: &GenericLabelRef<Traits>) -> GenericBoundLabel<Traits> {
        self.bound.set(true);
        if !self.is_forward() {
            return GenericBoundLabel::backward(self.location as i32);
        }
        // O `Ref<Label>` do `GenericBoundLabel` incrementa a contagem por `ref_()` aqui mesmo: clonar o
        // `GenericLabelRef` faria `borrow_mut()` num `RefCell` que o chamador já tem emprestado.
        self.ref_();
        GenericBoundLabel::forward(GenericLabelRef { label: Rc::clone(&label.label) })
    }

    /// `bind(unsigned offset)`.
    pub fn bind_offset(&mut self, offset: u32) -> GenericBoundLabel<Traits> {
        self.bound.set(true);
        if !self.is_forward() {
            return GenericBoundLabel::from_offset((self.location as i32).wrapping_sub(offset as i32));
        }
        self.unresolved_jumps.push(offset as i32);
        GenericBoundLabel::new()
    }

    /// `bind()`.
    pub fn bind(&mut self) -> GenericBoundLabel<Traits> {
        debug_assert!(!self.is_forward());
        self.bind_offset(0)
    }

    pub fn ref_(&mut self) {
        self.ref_count += 1;
    }

    pub fn deref(&mut self) {
        self.ref_count -= 1;
        debug_assert!(self.ref_count >= 0);
    }

    pub fn has_one_ref(&self) -> bool {
        self.ref_count == 1
    }

    pub fn is_forward(&self) -> bool {
        self.location == Self::INVALID_LOCATION
    }

    pub fn is_bound(&self) -> bool {
        self.bound.get()
    }

    pub fn location(&self) -> u32 {
        debug_assert!(!self.is_forward());
        self.bound.set(true);
        self.location
    }

    pub fn unresolved_jumps(&self) -> &Vec<i32> {
        &self.unresolved_jumps
    }
}

/// `Ref<Label>`/`RefPtr<Label>`: referência contada (incrementa ao clonar, decrementa ao soltar).
pub struct GenericLabelRef<Traits> {
    label: Rc<RefCell<GenericLabel<Traits>>>,
}

impl<Traits> GenericLabelRef<Traits> {
    /// Adota o rótulo incrementando a contagem.
    pub fn new(label: &Rc<RefCell<GenericLabel<Traits>>>) -> Self {
        label.borrow_mut().ref_();
        GenericLabelRef { label: Rc::clone(label) }
    }

    pub fn get(&self) -> &Rc<RefCell<GenericLabel<Traits>>> {
        &self.label
    }

    pub fn borrow(&self) -> Ref<'_, GenericLabel<Traits>> {
        self.label.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, GenericLabel<Traits>> {
        self.label.borrow_mut()
    }

    /// `label->bind(this)` com o rótulo na forma de referência (o `Label*` do C++).
    pub fn bind_generator(&self) -> GenericBoundLabel<Traits> {
        self.label.borrow_mut().bind_generator(self)
    }

    /// `label->isForward()`.
    pub fn is_forward(&self) -> bool {
        self.label.borrow().is_forward()
    }
}

impl<Traits> Clone for GenericLabelRef<Traits> {
    fn clone(&self) -> Self {
        GenericLabelRef::new(&self.label)
    }
}

impl<Traits> Drop for GenericLabelRef<Traits> {
    fn drop(&mut self) {
        self.label.borrow_mut().deref();
    }
}

pub type Label = GenericLabel<JSGeneratorTraits>;
pub type LabelRef = GenericLabelRef<JSGeneratorTraits>;
pub type BoundLabel = GenericBoundLabel<JSGeneratorTraits>;
/// `WasmBoundLabel`.
pub type WasmBoundLabel = GenericBoundLabel<wasm::GeneratorTraits>;
