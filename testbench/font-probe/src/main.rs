//! Sonda H10: largura de linha medida pelo parley contra a do Chromium do oráculo.
//! Uso: font-probe <arquivo de fonte> <tamanho> <texto>...

use std::sync::Arc;

use parley::fontique::Blob;
use parley::{FontContext, LayoutContext, StyleProperty};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = std::fs::read(&args[0]).expect("fonte");
    let size: f32 = args[1].parse().expect("tamanho");
    let mut fcx = FontContext::new();
    let families = fcx.collection.register_fonts(Blob::new(Arc::new(data)), None);
    let family = fcx.collection.family_name(families[0].0).expect("família").to_owned();
    let mut lcx: LayoutContext<[u8; 4]> = LayoutContext::new();
    for text in &args[2..] {
        let text = text.replace("\\t", "    ");
        let mut builder = lcx.ranged_builder(&mut fcx, &text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(family.as_str().into()));
        builder.push_default(StyleProperty::FontSize(size));
        let mut layout = builder.build(&text);
        layout.break_all_lines(None);
        let width = layout.lines().map(|l| l.metrics().advance).sum::<f32>();
        let lh = layout.lines().next().map(|l| l.metrics().line_height).unwrap_or(0.0);
        println!("{width:.6}\t{lh:.3}\t{text}");
    }
}
