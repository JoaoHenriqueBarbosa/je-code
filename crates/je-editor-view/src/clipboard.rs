//! A área de transferência: a do sistema pelo `arboard` (Rust puro em Linux, Windows e macOS, conferido por
//! `scripts/pure-rust-check.sh`) com uma cópia interna de reserva, para quando o sistema não tem clipboard
//! (sessão sem display, teste, CI) e para o alvo web, onde o `arboard` não compila.
// TODO(integração): no alvo web, ligar ao `navigator.clipboard` quando a ponte com o navegador existir.

#[cfg(not(target_arch = "wasm32"))]
type System = arboard::Clipboard;
/// No web não há clipboard de sistema por enquanto: o tipo nunca é construído.
#[cfg(target_arch = "wasm32")]
enum System {}

pub struct Clipboard {
    system: Option<System>,
    internal: String,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard {
    /// Abre o clipboard do sistema; sem ele, só a cópia interna funciona.
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let system = System::new().ok();
        #[cfg(target_arch = "wasm32")]
        let system = None;
        Self { system, internal: String::new() }
    }

    /// Só a cópia interna (testes que não podem tocar o clipboard real do desenvolvedor).
    pub fn internal_only() -> Self {
        Self { system: None, internal: String::new() }
    }

    pub fn set_text(&mut self, text: &str) {
        self.internal = text.to_owned();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(system) = &mut self.system
            && system.set_text(text).is_err()
        {
            // O sistema recusou (display morreu): fica a cópia interna.
            self.system = None;
        }
    }

    pub fn text(&mut self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(system) = &mut self.system
            && let Ok(text) = system.get_text()
        {
            return text;
        }
        self.internal.clone()
    }
}
