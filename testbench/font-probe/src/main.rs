//! Sonda H10: largura de linha medida pelo parley contra a do Chromium do oráculo.
//! Uso: font-probe <arquivo de fonte> <tamanho> <texto>...

use std::sync::Arc;

use parley::fontique::Blob;
use parley::{FontContext, LayoutContext, StyleProperty};
use skrifa::MetadataProvider;

/// Modo `system <lista de famílias CSS> <tamanho> <texto>`: resolve pelas fontes do sistema e diz qual
/// fonte cobriu cada trecho (H09).
fn system(families: &str, size: f32, text: &str) {
    let mut fcx = FontContext::new();
    let mut lcx: LayoutContext<[u8; 4]> = LayoutContext::new();
    let mut builder = lcx.ranged_builder(&mut fcx, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(families.into()));
    builder.push_default(StyleProperty::FontSize(size));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    for line in layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                let r = run.run();
                let font = r.font();
                let name = skrifa::FontRef::from_index(font.data.as_ref(), font.index)
                    .ok()
                    .and_then(|f| f.localized_strings(skrifa::string::StringId::FAMILY_NAME).english_or_first().map(|s| s.chars().collect::<String>()))
                    .unwrap_or_default();
                let range = r.text_range();
                println!("{:?}\t{}\tavanço {:.4}", &text[range], name, run.advance());
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("system") {
        system(&args[1], args[2].parse().expect("tamanho"), &args[3]);
        return;
    }
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
