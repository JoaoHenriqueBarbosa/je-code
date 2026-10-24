//! `WTF/wtf/text/UniquedStringImpl.h`.
//!
//! O tipo vive em `atom_string_impl` (`UniquedStringImpl` é o `StringImpl` internado); este módulo
//! só dá o caminho do cabeçalho do C++. `UniquedStringImplRef` é o `UniquedStringImpl*` do C++: a
//! alça comparada e espalhada por identidade, que é o `UniquedKey` e o que `Identifier::impl_`
//! devolve (o ponteiro nulo é `Option<UniquedStringImplRef>`).

pub use crate::wtf::text::atom_string_impl::UniquedStringImpl;
pub use crate::wtf::text::string_impl::UniquedKey as UniquedStringImplRef;
