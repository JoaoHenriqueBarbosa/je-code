//! Porte de `src/vs/workbench/services/keybinding/browser/keyboardLayouts`: os layouts de teclado que o serviço conhece
//! (`_.contribution.ts` e um arquivo por layout e sistema).
//!
//! Portados: o registro ([`contribution`]) e os dois layouts do macOS que o teste do mapeador de teclado importa
//! ([`en_darwin`] e [`de_darwin`]).
//!
//! Notas de tradução:
//! - O arquivo de layout do original registra o layout ao ser importado (`KeyboardLayoutContribution.INSTANCE
//!   .registerKeyboardLayout(...)`), efeito colateral da carga do módulo. Rust não tem isso: cada módulo de layout expõe
//!   `register()`, que quem monta o serviço (ou o teste, no lugar do `import`) chama. A chamada repetida na mesma thread
//!   não registra duas vezes, como o módulo ES carregado uma vez.
//! - O mapa serializado é uma tabela `&[(escancode, Option<(value, withShift, withAltGr, withShiftAltGr, mask)>)]`, na
//!   ordem do literal do original (`Enter: []` é `None`). O travessão e o traço médio que aparecem no literal do original
//!   (teclas `Minus` e `Slash`) entram como escape Unicode (`\u{2013}` e `\u{2014}`).

pub mod contribution;
pub mod de_darwin;
pub mod en_darwin;

use super::keymap_info::{SerializedKey, SerializedMapping};

/// `[value, withShift, withAltGr, withShiftAltGr, mask]`.
pub(crate) type KeyRow = (&'static str, &'static str, &'static str, &'static str, u32);

/// A tabela do literal `mapping` do original como [`SerializedMapping`].
pub(crate) fn serialized_mapping(rows: &[(&'static str, Option<KeyRow>)]) -> SerializedMapping {
    rows.iter()
        .map(|(key, row)| {
            let entry = row.map(|(value, with_shift, with_alt_gr, with_shift_alt_gr, mask)| SerializedKey::new(value, with_shift, with_alt_gr, with_shift_alt_gr, mask));
            ((*key).to_string(), entry)
        })
        .collect()
}
