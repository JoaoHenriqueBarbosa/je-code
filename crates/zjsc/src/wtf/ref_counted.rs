//! Trecho de `WTF/wtf/RefCounted.h`: o que os chamadores precisam de `RefCounted<T>`, a leitura de
//! `refCount()`.
//!
//! No C++ o contador vive na própria classe (herança de `RefCounted<T>`); aqui o tipo que mantém o
//! contador implementa o trait.

use std::cell::RefCell;
use std::rc::Rc;

/// `RefCounted<T>::refCount()`.
pub trait RefCounted {
    fn ref_count(&self) -> i32;
}

impl<T: RefCounted> RefCounted for RefCell<T> {
    fn ref_count(&self) -> i32 {
        self.borrow().ref_count()
    }
}

impl<T: RefCounted + ?Sized> RefCounted for Rc<T> {
    fn ref_count(&self) -> i32 {
        (**self).ref_count()
    }
}
