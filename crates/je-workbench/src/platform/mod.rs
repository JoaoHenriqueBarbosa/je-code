//! Porte do que de `src/vs/platform` (parte `browser`) estende os widgets nativos e por isso vive no workbench: a
//! plataforma (`je-platform`) não depende de `je-widgets` (ela também serve ao servidor e ao host de extensões, sem
//! interface). Os contratos ficam na plataforma; aqui entram as implementações que montam widgets.

pub mod actions;
pub mod contextview;
pub mod theme;
pub mod user_interaction;
