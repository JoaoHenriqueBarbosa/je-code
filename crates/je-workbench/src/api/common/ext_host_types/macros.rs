//! Macro das enumerações numéricas da API (`export enum X { A = 1, ... }` do TypeScript).
//!
//! Um `enum` numérico do TypeScript é também um mapa reverso (`DiagnosticSeverity[0] === 'Error'`), que
//! o `toJSON` de várias classes usa. O `enum` de Rust gerado aqui tem os mesmos valores, [`value`](#method.value),
//! [`from_value`](#method.from_value) e [`name`](#method.name) (o mapa reverso).

macro_rules! api_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(i32)]
        pub enum $name {
            $($variant = $value),+
        }

        impl $name {
            /// O valor numérico do membro (`X.A`).
            pub fn value(self) -> i32 {
                self as i32
            }

            /// O membro de um valor numérico, se existir.
            pub fn from_value(value: i32) -> Option<Self> {
                $(if value == $value {
                    return Some(Self::$variant);
                })+
                None
            }

            /// O mapa reverso do `enum` do TypeScript (`X[valor]`).
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant)),+
                }
            }
        }
    };
}
