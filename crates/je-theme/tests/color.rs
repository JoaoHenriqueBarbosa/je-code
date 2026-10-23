//! Porte de `src/vs/base/test/common/color.test.ts` (suíte `Color`), caso a caso e na mesma ordem.

use je_theme::color::format::css;
use je_theme::color::{Color, Hsla, Hsva, Srgba};

#[test]
fn is_lighter_color() {
    let color1 = Color::from(Hsla::new(60, 1, 0.5, 1));
    let color2 = Color::from(Hsla::new(0, 0, 0.753, 1));

    assert!(color1.is_lighter_than(&color2));

    // Abyss theme
    assert!(Color::from_hex("#770811").is_lighter_than(&Color::from_hex("#000c18")));
}

#[test]
fn get_lighter_color() {
    let color1 = Color::from(Hsla::new(60, 1, 0.5, 1));
    let color2 = Color::from(Hsla::new(0, 0, 0.753, 1));

    assert_eq!(
        color1.hsla(),
        Color::get_lighter_color(&color1, &color2, None).hsla()
    );
    assert_eq!(
        Hsla::new(0, 0, 0.916, 1),
        Color::get_lighter_color(&color2, &color1, None).hsla()
    );
    assert_eq!(
        Hsla::new(0, 0, 0.851, 1),
        Color::get_lighter_color(&color2, &color1, Some(0.3)).hsla()
    );
    assert_eq!(
        Hsla::new(0, 0, 0.981, 1),
        Color::get_lighter_color(&color2, &color1, Some(0.7)).hsla()
    );
    assert_eq!(
        Hsla::new(0, 0, 1, 1),
        Color::get_lighter_color(&color2, &color1, Some(1.0)).hsla()
    );
}

#[test]
fn is_darker_color() {
    let color1 = Color::from(Hsla::new(60, 1, 0.5, 1));
    let color2 = Color::from(Hsla::new(0, 0, 0.753, 1));

    assert!(color2.is_darker_than(&color1));
}

#[test]
fn get_darker_color() {
    let color1 = Color::from(Hsla::new(60, 1, 0.5, 1));
    let color2 = Color::from(Hsla::new(0, 0, 0.753, 1));

    assert_eq!(
        color2.hsla(),
        Color::get_darker_color(&color2, &color1, None).hsla()
    );
    assert_eq!(
        Hsla::new(60, 1, 0.392, 1),
        Color::get_darker_color(&color1, &color2, None).hsla()
    );
    assert_eq!(
        Hsla::new(60, 1, 0.435, 1),
        Color::get_darker_color(&color1, &color2, Some(0.3)).hsla()
    );
    assert_eq!(
        Hsla::new(60, 1, 0.349, 1),
        Color::get_darker_color(&color1, &color2, Some(0.7)).hsla()
    );
    assert_eq!(
        Hsla::new(60, 1, 0.284, 1),
        Color::get_darker_color(&color1, &color2, Some(1.0)).hsla()
    );

    // Abyss theme
    assert_eq!(
        Hsla::new(355, 0.874, 0.157, 1),
        Color::get_darker_color(
            &Color::from_hex("#770811"),
            &Color::from_hex("#000c18"),
            Some(0.4)
        )
        .hsla()
    );
}

#[test]
fn luminance() {
    assert_eq!(
        Color::from(Srgba::new(0, 0, 0, 1)).get_relative_luminance(),
        0.0
    );
    assert_eq!(
        Color::from(Srgba::new(255, 255, 255, 1)).get_relative_luminance(),
        1.0
    );
    assert_eq!(
        Color::from(Srgba::new(255, 0, 0, 1)).get_relative_luminance(),
        0.2126
    );
    assert_eq!(
        Color::from(Srgba::new(0, 255, 0, 1)).get_relative_luminance(),
        0.7152
    );
    assert_eq!(
        Color::from(Srgba::new(0, 0, 255, 1)).get_relative_luminance(),
        0.0722
    );
    assert_eq!(
        Color::from(Srgba::new(255, 255, 0, 1)).get_relative_luminance(),
        0.9278
    );
    assert_eq!(
        Color::from(Srgba::new(0, 255, 255, 1)).get_relative_luminance(),
        0.7874
    );
    assert_eq!(
        Color::from(Srgba::new(255, 0, 255, 1)).get_relative_luminance(),
        0.2848
    );
    assert_eq!(
        Color::from(Srgba::new(192, 192, 192, 1)).get_relative_luminance(),
        0.5271
    );
    assert_eq!(
        Color::from(Srgba::new(128, 128, 128, 1)).get_relative_luminance(),
        0.2159
    );
    assert_eq!(
        Color::from(Srgba::new(128, 0, 0, 1)).get_relative_luminance(),
        0.0459
    );
    assert_eq!(
        Color::from(Srgba::new(128, 128, 0, 1)).get_relative_luminance(),
        0.2003
    );
    assert_eq!(
        Color::from(Srgba::new(0, 128, 0, 1)).get_relative_luminance(),
        0.1544
    );
    assert_eq!(
        Color::from(Srgba::new(128, 0, 128, 1)).get_relative_luminance(),
        0.0615
    );
    assert_eq!(
        Color::from(Srgba::new(0, 128, 128, 1)).get_relative_luminance(),
        0.17
    );
    assert_eq!(
        Color::from(Srgba::new(0, 0, 128, 1)).get_relative_luminance(),
        0.0156
    );
}

#[test]
fn blending() {
    assert_eq!(
        Color::from(Srgba::new(0, 0, 0, 0)).blend(&Color::from(Srgba::opaque(243, 34, 43))),
        Color::from(Srgba::opaque(243, 34, 43))
    );
    assert_eq!(
        Color::from(Srgba::opaque(255, 255, 255)).blend(&Color::from(Srgba::opaque(243, 34, 43))),
        Color::from(Srgba::opaque(255, 255, 255))
    );
    assert_eq!(
        Color::from(Srgba::new(122, 122, 122, 0.7)).blend(&Color::from(Srgba::opaque(243, 34, 43))),
        Color::from(Srgba::opaque(158, 95, 98))
    );
    assert_eq!(
        Color::from(Srgba::new(0, 0, 0, 0.58)).blend(&Color::from(Srgba::new(255, 255, 255, 0.33))),
        Color::from(Srgba::new(49, 49, 49, 0.719))
    );
}

mod to_string {
    use super::*;

    #[test]
    fn alpha_channel() {
        assert_eq!(Color::from_hex("#00000000").to_string(), "rgba(0, 0, 0, 0)");
        assert_eq!(
            Color::from_hex("#00000080").to_string(),
            "rgba(0, 0, 0, 0.5)"
        );
        assert_eq!(Color::from_hex("#000000FF").to_string(), "#000000");
    }

    #[test]
    fn opaque() {
        assert_eq!(
            Color::from_hex("#000000").to_string().to_uppercase(),
            "#000000".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#FFFFFF").to_string().to_uppercase(),
            "#FFFFFF".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#FF0000").to_string().to_uppercase(),
            "#FF0000".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#00FF00").to_string().to_uppercase(),
            "#00FF00".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0000FF").to_string().to_uppercase(),
            "#0000FF".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#FFFF00").to_string().to_uppercase(),
            "#FFFF00".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#00FFFF").to_string().to_uppercase(),
            "#00FFFF".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#FF00FF").to_string().to_uppercase(),
            "#FF00FF".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#C0C0C0").to_string().to_uppercase(),
            "#C0C0C0".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#808080").to_string().to_uppercase(),
            "#808080".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#800000").to_string().to_uppercase(),
            "#800000".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#808000").to_string().to_uppercase(),
            "#808000".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#008000").to_string().to_uppercase(),
            "#008000".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#800080").to_string().to_uppercase(),
            "#800080".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#008080").to_string().to_uppercase(),
            "#008080".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#000080").to_string().to_uppercase(),
            "#000080".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#010203").to_string().to_uppercase(),
            "#010203".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#040506").to_string().to_uppercase(),
            "#040506".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#070809").to_string().to_uppercase(),
            "#070809".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0a0A0a").to_string().to_uppercase(),
            "#0a0A0a".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0b0B0b").to_string().to_uppercase(),
            "#0b0B0b".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0c0C0c").to_string().to_uppercase(),
            "#0c0C0c".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0d0D0d").to_string().to_uppercase(),
            "#0d0D0d".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0e0E0e").to_string().to_uppercase(),
            "#0e0E0e".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#0f0F0f").to_string().to_uppercase(),
            "#0f0F0f".to_uppercase()
        );
        assert_eq!(
            Color::from_hex("#a0A0a0").to_string().to_uppercase(),
            "#a0A0a0".to_uppercase()
        );
    }
}

mod to_number_32bit {
    use super::*;

    #[test]
    fn alpha_channel() {
        assert_eq!(Color::from_hex("#00000000").to_number_32bit(), 0x00000000);
        assert_eq!(Color::from_hex("#00000080").to_number_32bit(), 0x00000080);
        assert_eq!(Color::from_hex("#000000FF").to_number_32bit(), 0x000000ff);
    }

    #[test]
    fn opaque() {
        assert_eq!(Color::from_hex("#000000").to_number_32bit(), 0x000000ff);
        assert_eq!(Color::from_hex("#FFFFFF").to_number_32bit(), 0xffffffff);
        assert_eq!(Color::from_hex("#FF0000").to_number_32bit(), 0xff0000ff);
        assert_eq!(Color::from_hex("#00FF00").to_number_32bit(), 0x00ff00ff);
        assert_eq!(Color::from_hex("#0000FF").to_number_32bit(), 0x0000ffff);
        assert_eq!(Color::from_hex("#FFFF00").to_number_32bit(), 0xffff00ff);
        assert_eq!(Color::from_hex("#00FFFF").to_number_32bit(), 0x00ffffff);
        assert_eq!(Color::from_hex("#FF00FF").to_number_32bit(), 0xff00ffff);
        assert_eq!(Color::from_hex("#C0C0C0").to_number_32bit(), 0xc0c0c0ff);
        assert_eq!(Color::from_hex("#808080").to_number_32bit(), 0x808080ff);
        assert_eq!(Color::from_hex("#800000").to_number_32bit(), 0x800000ff);
        assert_eq!(Color::from_hex("#808000").to_number_32bit(), 0x808000ff);
        assert_eq!(Color::from_hex("#008000").to_number_32bit(), 0x008000ff);
        assert_eq!(Color::from_hex("#800080").to_number_32bit(), 0x800080ff);
        assert_eq!(Color::from_hex("#008080").to_number_32bit(), 0x008080ff);
        assert_eq!(Color::from_hex("#000080").to_number_32bit(), 0x000080ff);
        assert_eq!(Color::from_hex("#010203").to_number_32bit(), 0x010203ff);
        assert_eq!(Color::from_hex("#040506").to_number_32bit(), 0x040506ff);
        assert_eq!(Color::from_hex("#070809").to_number_32bit(), 0x070809ff);
        assert_eq!(Color::from_hex("#0a0A0a").to_number_32bit(), 0x0a0a0aff);
        assert_eq!(Color::from_hex("#0b0B0b").to_number_32bit(), 0x0b0b0bff);
        assert_eq!(Color::from_hex("#0c0C0c").to_number_32bit(), 0x0c0c0cff);
        assert_eq!(Color::from_hex("#0d0D0d").to_number_32bit(), 0x0d0d0dff);
        assert_eq!(Color::from_hex("#0e0E0e").to_number_32bit(), 0x0e0e0eff);
        assert_eq!(Color::from_hex("#0f0F0f").to_number_32bit(), 0x0f0f0fff);
        assert_eq!(Color::from_hex("#a0A0a0").to_number_32bit(), 0xa0a0a0ff);
    }
}

mod hsla {
    use super::*;

    #[test]
    fn hsla_to_rgba() {
        assert_eq!(Hsla::new(0, 0, 0, 0).to_rgba(), Srgba::new(0, 0, 0, 0));
        assert_eq!(Hsla::new(0, 0, 0, 1).to_rgba(), Srgba::new(0, 0, 0, 1));
        assert_eq!(
            Hsla::new(0, 0, 1, 1).to_rgba(),
            Srgba::new(255, 255, 255, 1)
        );
        assert_eq!(Hsla::new(0, 1, 0.5, 1).to_rgba(), Srgba::new(255, 0, 0, 1));
        assert_eq!(
            Hsla::new(120, 1, 0.5, 1).to_rgba(),
            Srgba::new(0, 255, 0, 1)
        );
        assert_eq!(
            Hsla::new(240, 1, 0.5, 1).to_rgba(),
            Srgba::new(0, 0, 255, 1)
        );
        assert_eq!(
            Hsla::new(60, 1, 0.5, 1).to_rgba(),
            Srgba::new(255, 255, 0, 1)
        );
        assert_eq!(
            Hsla::new(180, 1, 0.5, 1).to_rgba(),
            Srgba::new(0, 255, 255, 1)
        );
        assert_eq!(
            Hsla::new(300, 1, 0.5, 1).to_rgba(),
            Srgba::new(255, 0, 255, 1)
        );
        assert_eq!(
            Hsla::new(0, 0, 0.753, 1).to_rgba(),
            Srgba::new(192, 192, 192, 1)
        );
        assert_eq!(
            Hsla::new(0, 0, 0.502, 1).to_rgba(),
            Srgba::new(128, 128, 128, 1)
        );
        assert_eq!(
            Hsla::new(0, 1, 0.251, 1).to_rgba(),
            Srgba::new(128, 0, 0, 1)
        );
        assert_eq!(
            Hsla::new(60, 1, 0.251, 1).to_rgba(),
            Srgba::new(128, 128, 0, 1)
        );
        assert_eq!(
            Hsla::new(120, 1, 0.251, 1).to_rgba(),
            Srgba::new(0, 128, 0, 1)
        );
        assert_eq!(
            Hsla::new(300, 1, 0.251, 1).to_rgba(),
            Srgba::new(128, 0, 128, 1)
        );
        assert_eq!(
            Hsla::new(180, 1, 0.251, 1).to_rgba(),
            Srgba::new(0, 128, 128, 1)
        );
        assert_eq!(
            Hsla::new(240, 1, 0.251, 1).to_rgba(),
            Srgba::new(0, 0, 128, 1)
        );
    }

    #[test]
    fn hsla_from_rgba() {
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 0, 0, 0)),
            Hsla::new(0, 0, 0, 0)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 0, 0, 1)),
            Hsla::new(0, 0, 0, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(255, 255, 255, 1)),
            Hsla::new(0, 0, 1, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(255, 0, 0, 1)),
            Hsla::new(0, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 255, 0, 1)),
            Hsla::new(120, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 0, 255, 1)),
            Hsla::new(240, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(255, 255, 0, 1)),
            Hsla::new(60, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 255, 255, 1)),
            Hsla::new(180, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(255, 0, 255, 1)),
            Hsla::new(300, 1, 0.5, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(192, 192, 192, 1)),
            Hsla::new(0, 0, 0.753, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(128, 128, 128, 1)),
            Hsla::new(0, 0, 0.502, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(128, 0, 0, 1)),
            Hsla::new(0, 1, 0.251, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(128, 128, 0, 1)),
            Hsla::new(60, 1, 0.251, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 128, 0, 1)),
            Hsla::new(120, 1, 0.251, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(128, 0, 128, 1)),
            Hsla::new(300, 1, 0.251, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 128, 128, 1)),
            Hsla::new(180, 1, 0.251, 1)
        );
        assert_eq!(
            Hsla::from_rgba(Srgba::new(0, 0, 128, 1)),
            Hsla::new(240, 1, 0.251, 1)
        );
    }
}

mod hsva {
    use super::*;

    #[test]
    fn hsva_to_rgba() {
        assert_eq!(Hsva::new(0, 0, 0, 0).to_rgba(), Srgba::new(0, 0, 0, 0));
        assert_eq!(Hsva::new(0, 0, 0, 1).to_rgba(), Srgba::new(0, 0, 0, 1));
        assert_eq!(
            Hsva::new(0, 0, 1, 1).to_rgba(),
            Srgba::new(255, 255, 255, 1)
        );
        assert_eq!(Hsva::new(0, 1, 1, 1).to_rgba(), Srgba::new(255, 0, 0, 1));
        assert_eq!(Hsva::new(120, 1, 1, 1).to_rgba(), Srgba::new(0, 255, 0, 1));
        assert_eq!(Hsva::new(240, 1, 1, 1).to_rgba(), Srgba::new(0, 0, 255, 1));
        assert_eq!(Hsva::new(60, 1, 1, 1).to_rgba(), Srgba::new(255, 255, 0, 1));
        assert_eq!(
            Hsva::new(180, 1, 1, 1).to_rgba(),
            Srgba::new(0, 255, 255, 1)
        );
        assert_eq!(
            Hsva::new(300, 1, 1, 1).to_rgba(),
            Srgba::new(255, 0, 255, 1)
        );
        assert_eq!(
            Hsva::new(0, 0, 0.753, 1).to_rgba(),
            Srgba::new(192, 192, 192, 1)
        );
        assert_eq!(
            Hsva::new(0, 0, 0.502, 1).to_rgba(),
            Srgba::new(128, 128, 128, 1)
        );
        assert_eq!(
            Hsva::new(0, 1, 0.502, 1).to_rgba(),
            Srgba::new(128, 0, 0, 1)
        );
        assert_eq!(
            Hsva::new(60, 1, 0.502, 1).to_rgba(),
            Srgba::new(128, 128, 0, 1)
        );
        assert_eq!(
            Hsva::new(120, 1, 0.502, 1).to_rgba(),
            Srgba::new(0, 128, 0, 1)
        );
        assert_eq!(
            Hsva::new(300, 1, 0.502, 1).to_rgba(),
            Srgba::new(128, 0, 128, 1)
        );
        assert_eq!(
            Hsva::new(180, 1, 0.502, 1).to_rgba(),
            Srgba::new(0, 128, 128, 1)
        );
        assert_eq!(
            Hsva::new(240, 1, 0.502, 1).to_rgba(),
            Srgba::new(0, 0, 128, 1)
        );
        assert_eq!(Hsva::new(360, 0, 0, 0).to_rgba(), Srgba::new(0, 0, 0, 0));
        assert_eq!(Hsva::new(360, 0, 0, 1).to_rgba(), Srgba::new(0, 0, 0, 1));
        assert_eq!(
            Hsva::new(360, 0, 1, 1).to_rgba(),
            Srgba::new(255, 255, 255, 1)
        );
        assert_eq!(Hsva::new(360, 1, 1, 1).to_rgba(), Srgba::new(255, 0, 0, 1));
        assert_eq!(
            Hsva::new(360, 0, 0.753, 1).to_rgba(),
            Srgba::new(192, 192, 192, 1)
        );
        assert_eq!(
            Hsva::new(360, 0, 0.502, 1).to_rgba(),
            Srgba::new(128, 128, 128, 1)
        );
        assert_eq!(
            Hsva::new(360, 1, 0.502, 1).to_rgba(),
            Srgba::new(128, 0, 0, 1)
        );
    }

    #[test]
    fn hsva_from_rgba() {
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 0, 0, 0)),
            Hsva::new(0, 0, 0, 0)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 0, 0, 1)),
            Hsva::new(0, 0, 0, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(255, 255, 255, 1)),
            Hsva::new(0, 0, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(255, 0, 0, 1)),
            Hsva::new(0, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 255, 0, 1)),
            Hsva::new(120, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 0, 255, 1)),
            Hsva::new(240, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(255, 255, 0, 1)),
            Hsva::new(60, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 255, 255, 1)),
            Hsva::new(180, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(255, 0, 255, 1)),
            Hsva::new(300, 1, 1, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(192, 192, 192, 1)),
            Hsva::new(0, 0, 0.753, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(128, 128, 128, 1)),
            Hsva::new(0, 0, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(128, 0, 0, 1)),
            Hsva::new(0, 1, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(128, 128, 0, 1)),
            Hsva::new(60, 1, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 128, 0, 1)),
            Hsva::new(120, 1, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(128, 0, 128, 1)),
            Hsva::new(300, 1, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 128, 128, 1)),
            Hsva::new(180, 1, 0.502, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(0, 0, 128, 1)),
            Hsva::new(240, 1, 0.502, 1)
        );
    }

    #[test]
    fn keep_hue_value_when_saturation_is_0() {
        assert_eq!(
            Hsva::new(10, 0, 0, 0).to_rgba(),
            Hsva::new(20, 0, 0, 0).to_rgba()
        );
        assert_eq!(
            Color::from(Hsva::new(10, 0, 0, 0)).rgba(),
            Color::from(Hsva::new(20, 0, 0, 0)).rgba()
        );
        assert_ne!(
            Color::from(Hsva::new(10, 0, 0, 0)).hsva(),
            Color::from(Hsva::new(20, 0, 0, 0)).hsva()
        );
    }

    #[test]
    fn bug_36240() {
        assert_eq!(
            Hsva::from_rgba(Srgba::new(92, 106, 196, 1)),
            Hsva::new(232, 0.531, 0.769, 1)
        );
        assert_eq!(
            Hsva::from_rgba(Srgba::new(92, 106, 196, 1)).to_rgba(),
            Srgba::new(92, 106, 196, 1)
        );
    }
}

mod format_css {
    use super::*;

    mod parse {
        use super::*;

        #[test]
        fn invalid() {
            assert_eq!(css::parse(""), Ok(None));
            assert_eq!(css::parse("#"), Ok(None));
            assert_eq!(css::parse("#0102030"), Ok(None));
        }

        #[test]
        fn transparent() {
            assert_eq!(
                css::parse("transparent").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 0)
            );
        }

        #[test]
        fn named_keyword() {
            assert_eq!(
                css::parse("aliceblue").unwrap().unwrap().rgba(),
                Srgba::new(240, 248, 255, 1)
            );
            assert_eq!(
                css::parse("antiquewhite").unwrap().unwrap().rgba(),
                Srgba::new(250, 235, 215, 1)
            );
            assert_eq!(
                css::parse("aqua").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 255, 1)
            );
            assert_eq!(
                css::parse("aquamarine").unwrap().unwrap().rgba(),
                Srgba::new(127, 255, 212, 1)
            );
            assert_eq!(
                css::parse("azure").unwrap().unwrap().rgba(),
                Srgba::new(240, 255, 255, 1)
            );
            assert_eq!(
                css::parse("beige").unwrap().unwrap().rgba(),
                Srgba::new(245, 245, 220, 1)
            );
            assert_eq!(
                css::parse("bisque").unwrap().unwrap().rgba(),
                Srgba::new(255, 228, 196, 1)
            );
            assert_eq!(
                css::parse("black").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("blanchedalmond").unwrap().unwrap().rgba(),
                Srgba::new(255, 235, 205, 1)
            );
            assert_eq!(
                css::parse("blue").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 255, 1)
            );
            assert_eq!(
                css::parse("blueviolet").unwrap().unwrap().rgba(),
                Srgba::new(138, 43, 226, 1)
            );
            assert_eq!(
                css::parse("brown").unwrap().unwrap().rgba(),
                Srgba::new(165, 42, 42, 1)
            );
            assert_eq!(
                css::parse("burlywood").unwrap().unwrap().rgba(),
                Srgba::new(222, 184, 135, 1)
            );
            assert_eq!(
                css::parse("cadetblue").unwrap().unwrap().rgba(),
                Srgba::new(95, 158, 160, 1)
            );
            assert_eq!(
                css::parse("chartreuse").unwrap().unwrap().rgba(),
                Srgba::new(127, 255, 0, 1)
            );
            assert_eq!(
                css::parse("chocolate").unwrap().unwrap().rgba(),
                Srgba::new(210, 105, 30, 1)
            );
            assert_eq!(
                css::parse("coral").unwrap().unwrap().rgba(),
                Srgba::new(255, 127, 80, 1)
            );
            assert_eq!(
                css::parse("cornflowerblue").unwrap().unwrap().rgba(),
                Srgba::new(100, 149, 237, 1)
            );
            assert_eq!(
                css::parse("cornsilk").unwrap().unwrap().rgba(),
                Srgba::new(255, 248, 220, 1)
            );
            assert_eq!(
                css::parse("crimson").unwrap().unwrap().rgba(),
                Srgba::new(220, 20, 60, 1)
            );
            assert_eq!(
                css::parse("cyan").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 255, 1)
            );
            assert_eq!(
                css::parse("darkblue").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 139, 1)
            );
            assert_eq!(
                css::parse("darkcyan").unwrap().unwrap().rgba(),
                Srgba::new(0, 139, 139, 1)
            );
            assert_eq!(
                css::parse("darkgoldenrod").unwrap().unwrap().rgba(),
                Srgba::new(184, 134, 11, 1)
            );
            assert_eq!(
                css::parse("darkgray").unwrap().unwrap().rgba(),
                Srgba::new(169, 169, 169, 1)
            );
            assert_eq!(
                css::parse("darkgreen").unwrap().unwrap().rgba(),
                Srgba::new(0, 100, 0, 1)
            );
            assert_eq!(
                css::parse("darkgrey").unwrap().unwrap().rgba(),
                Srgba::new(169, 169, 169, 1)
            );
            assert_eq!(
                css::parse("darkkhaki").unwrap().unwrap().rgba(),
                Srgba::new(189, 183, 107, 1)
            );
            assert_eq!(
                css::parse("darkmagenta").unwrap().unwrap().rgba(),
                Srgba::new(139, 0, 139, 1)
            );
            assert_eq!(
                css::parse("darkolivegreen").unwrap().unwrap().rgba(),
                Srgba::new(85, 107, 47, 1)
            );
            assert_eq!(
                css::parse("darkorange").unwrap().unwrap().rgba(),
                Srgba::new(255, 140, 0, 1)
            );
            assert_eq!(
                css::parse("darkorchid").unwrap().unwrap().rgba(),
                Srgba::new(153, 50, 204, 1)
            );
            assert_eq!(
                css::parse("darkred").unwrap().unwrap().rgba(),
                Srgba::new(139, 0, 0, 1)
            );
            assert_eq!(
                css::parse("darksalmon").unwrap().unwrap().rgba(),
                Srgba::new(233, 150, 122, 1)
            );
            assert_eq!(
                css::parse("darkseagreen").unwrap().unwrap().rgba(),
                Srgba::new(143, 188, 143, 1)
            );
            assert_eq!(
                css::parse("darkslateblue").unwrap().unwrap().rgba(),
                Srgba::new(72, 61, 139, 1)
            );
            assert_eq!(
                css::parse("darkslategray").unwrap().unwrap().rgba(),
                Srgba::new(47, 79, 79, 1)
            );
            assert_eq!(
                css::parse("darkslategrey").unwrap().unwrap().rgba(),
                Srgba::new(47, 79, 79, 1)
            );
            assert_eq!(
                css::parse("darkturquoise").unwrap().unwrap().rgba(),
                Srgba::new(0, 206, 209, 1)
            );
            assert_eq!(
                css::parse("darkviolet").unwrap().unwrap().rgba(),
                Srgba::new(148, 0, 211, 1)
            );
            assert_eq!(
                css::parse("deeppink").unwrap().unwrap().rgba(),
                Srgba::new(255, 20, 147, 1)
            );
            assert_eq!(
                css::parse("deepskyblue").unwrap().unwrap().rgba(),
                Srgba::new(0, 191, 255, 1)
            );
            assert_eq!(
                css::parse("dimgray").unwrap().unwrap().rgba(),
                Srgba::new(105, 105, 105, 1)
            );
            assert_eq!(
                css::parse("dimgrey").unwrap().unwrap().rgba(),
                Srgba::new(105, 105, 105, 1)
            );
            assert_eq!(
                css::parse("dodgerblue").unwrap().unwrap().rgba(),
                Srgba::new(30, 144, 255, 1)
            );
            assert_eq!(
                css::parse("firebrick").unwrap().unwrap().rgba(),
                Srgba::new(178, 34, 34, 1)
            );
            assert_eq!(
                css::parse("floralwhite").unwrap().unwrap().rgba(),
                Srgba::new(255, 250, 240, 1)
            );
            assert_eq!(
                css::parse("forestgreen").unwrap().unwrap().rgba(),
                Srgba::new(34, 139, 34, 1)
            );
            assert_eq!(
                css::parse("fuchsia").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 255, 1)
            );
            assert_eq!(
                css::parse("gainsboro").unwrap().unwrap().rgba(),
                Srgba::new(220, 220, 220, 1)
            );
            assert_eq!(
                css::parse("ghostwhite").unwrap().unwrap().rgba(),
                Srgba::new(248, 248, 255, 1)
            );
            assert_eq!(
                css::parse("gold").unwrap().unwrap().rgba(),
                Srgba::new(255, 215, 0, 1)
            );
            assert_eq!(
                css::parse("goldenrod").unwrap().unwrap().rgba(),
                Srgba::new(218, 165, 32, 1)
            );
            assert_eq!(
                css::parse("gray").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 128, 1)
            );
            assert_eq!(
                css::parse("green").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 0, 1)
            );
            assert_eq!(
                css::parse("greenyellow").unwrap().unwrap().rgba(),
                Srgba::new(173, 255, 47, 1)
            );
            assert_eq!(
                css::parse("grey").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 128, 1)
            );
            assert_eq!(
                css::parse("honeydew").unwrap().unwrap().rgba(),
                Srgba::new(240, 255, 240, 1)
            );
            assert_eq!(
                css::parse("hotpink").unwrap().unwrap().rgba(),
                Srgba::new(255, 105, 180, 1)
            );
            assert_eq!(
                css::parse("indianred").unwrap().unwrap().rgba(),
                Srgba::new(205, 92, 92, 1)
            );
            assert_eq!(
                css::parse("indigo").unwrap().unwrap().rgba(),
                Srgba::new(75, 0, 130, 1)
            );
            assert_eq!(
                css::parse("ivory").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 240, 1)
            );
            assert_eq!(
                css::parse("khaki").unwrap().unwrap().rgba(),
                Srgba::new(240, 230, 140, 1)
            );
            assert_eq!(
                css::parse("lavender").unwrap().unwrap().rgba(),
                Srgba::new(230, 230, 250, 1)
            );
            assert_eq!(
                css::parse("lavenderblush").unwrap().unwrap().rgba(),
                Srgba::new(255, 240, 245, 1)
            );
            assert_eq!(
                css::parse("lawngreen").unwrap().unwrap().rgba(),
                Srgba::new(124, 252, 0, 1)
            );
            assert_eq!(
                css::parse("lemonchiffon").unwrap().unwrap().rgba(),
                Srgba::new(255, 250, 205, 1)
            );
            assert_eq!(
                css::parse("lightblue").unwrap().unwrap().rgba(),
                Srgba::new(173, 216, 230, 1)
            );
            assert_eq!(
                css::parse("lightcoral").unwrap().unwrap().rgba(),
                Srgba::new(240, 128, 128, 1)
            );
            assert_eq!(
                css::parse("lightcyan").unwrap().unwrap().rgba(),
                Srgba::new(224, 255, 255, 1)
            );
            assert_eq!(
                css::parse("lightgoldenrodyellow").unwrap().unwrap().rgba(),
                Srgba::new(250, 250, 210, 1)
            );
            assert_eq!(
                css::parse("lightgray").unwrap().unwrap().rgba(),
                Srgba::new(211, 211, 211, 1)
            );
            assert_eq!(
                css::parse("lightgreen").unwrap().unwrap().rgba(),
                Srgba::new(144, 238, 144, 1)
            );
            assert_eq!(
                css::parse("lightgrey").unwrap().unwrap().rgba(),
                Srgba::new(211, 211, 211, 1)
            );
            assert_eq!(
                css::parse("lightpink").unwrap().unwrap().rgba(),
                Srgba::new(255, 182, 193, 1)
            );
            assert_eq!(
                css::parse("lightsalmon").unwrap().unwrap().rgba(),
                Srgba::new(255, 160, 122, 1)
            );
            assert_eq!(
                css::parse("lightseagreen").unwrap().unwrap().rgba(),
                Srgba::new(32, 178, 170, 1)
            );
            assert_eq!(
                css::parse("lightskyblue").unwrap().unwrap().rgba(),
                Srgba::new(135, 206, 250, 1)
            );
            assert_eq!(
                css::parse("lightslategray").unwrap().unwrap().rgba(),
                Srgba::new(119, 136, 153, 1)
            );
            assert_eq!(
                css::parse("lightslategrey").unwrap().unwrap().rgba(),
                Srgba::new(119, 136, 153, 1)
            );
            assert_eq!(
                css::parse("lightsteelblue").unwrap().unwrap().rgba(),
                Srgba::new(176, 196, 222, 1)
            );
            assert_eq!(
                css::parse("lightyellow").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 224, 1)
            );
            assert_eq!(
                css::parse("lime").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 0, 1)
            );
            assert_eq!(
                css::parse("limegreen").unwrap().unwrap().rgba(),
                Srgba::new(50, 205, 50, 1)
            );
            assert_eq!(
                css::parse("linen").unwrap().unwrap().rgba(),
                Srgba::new(250, 240, 230, 1)
            );
            assert_eq!(
                css::parse("magenta").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 255, 1)
            );
            assert_eq!(
                css::parse("maroon").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 0, 1)
            );
            assert_eq!(
                css::parse("mediumaquamarine").unwrap().unwrap().rgba(),
                Srgba::new(102, 205, 170, 1)
            );
            assert_eq!(
                css::parse("mediumblue").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 205, 1)
            );
            assert_eq!(
                css::parse("mediumorchid").unwrap().unwrap().rgba(),
                Srgba::new(186, 85, 211, 1)
            );
            assert_eq!(
                css::parse("mediumpurple").unwrap().unwrap().rgba(),
                Srgba::new(147, 112, 219, 1)
            );
            assert_eq!(
                css::parse("mediumseagreen").unwrap().unwrap().rgba(),
                Srgba::new(60, 179, 113, 1)
            );
            assert_eq!(
                css::parse("mediumslateblue").unwrap().unwrap().rgba(),
                Srgba::new(123, 104, 238, 1)
            );
            assert_eq!(
                css::parse("mediumspringgreen").unwrap().unwrap().rgba(),
                Srgba::new(0, 250, 154, 1)
            );
            assert_eq!(
                css::parse("mediumturquoise").unwrap().unwrap().rgba(),
                Srgba::new(72, 209, 204, 1)
            );
            assert_eq!(
                css::parse("mediumvioletred").unwrap().unwrap().rgba(),
                Srgba::new(199, 21, 133, 1)
            );
            assert_eq!(
                css::parse("midnightblue").unwrap().unwrap().rgba(),
                Srgba::new(25, 25, 112, 1)
            );
            assert_eq!(
                css::parse("mintcream").unwrap().unwrap().rgba(),
                Srgba::new(245, 255, 250, 1)
            );
            assert_eq!(
                css::parse("mistyrose").unwrap().unwrap().rgba(),
                Srgba::new(255, 228, 225, 1)
            );
            assert_eq!(
                css::parse("moccasin").unwrap().unwrap().rgba(),
                Srgba::new(255, 228, 181, 1)
            );
            assert_eq!(
                css::parse("navajowhite").unwrap().unwrap().rgba(),
                Srgba::new(255, 222, 173, 1)
            );
            assert_eq!(
                css::parse("navy").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 128, 1)
            );
            assert_eq!(
                css::parse("oldlace").unwrap().unwrap().rgba(),
                Srgba::new(253, 245, 230, 1)
            );
            assert_eq!(
                css::parse("olive").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 0, 1)
            );
            assert_eq!(
                css::parse("olivedrab").unwrap().unwrap().rgba(),
                Srgba::new(107, 142, 35, 1)
            );
            assert_eq!(
                css::parse("orange").unwrap().unwrap().rgba(),
                Srgba::new(255, 165, 0, 1)
            );
            assert_eq!(
                css::parse("orangered").unwrap().unwrap().rgba(),
                Srgba::new(255, 69, 0, 1)
            );
            assert_eq!(
                css::parse("orchid").unwrap().unwrap().rgba(),
                Srgba::new(218, 112, 214, 1)
            );
            assert_eq!(
                css::parse("palegoldenrod").unwrap().unwrap().rgba(),
                Srgba::new(238, 232, 170, 1)
            );
            assert_eq!(
                css::parse("palegreen").unwrap().unwrap().rgba(),
                Srgba::new(152, 251, 152, 1)
            );
            assert_eq!(
                css::parse("paleturquoise").unwrap().unwrap().rgba(),
                Srgba::new(175, 238, 238, 1)
            );
            assert_eq!(
                css::parse("palevioletred").unwrap().unwrap().rgba(),
                Srgba::new(219, 112, 147, 1)
            );
            assert_eq!(
                css::parse("papayawhip").unwrap().unwrap().rgba(),
                Srgba::new(255, 239, 213, 1)
            );
            assert_eq!(
                css::parse("peachpuff").unwrap().unwrap().rgba(),
                Srgba::new(255, 218, 185, 1)
            );
            assert_eq!(
                css::parse("peru").unwrap().unwrap().rgba(),
                Srgba::new(205, 133, 63, 1)
            );
            assert_eq!(
                css::parse("pink").unwrap().unwrap().rgba(),
                Srgba::new(255, 192, 203, 1)
            );
            assert_eq!(
                css::parse("plum").unwrap().unwrap().rgba(),
                Srgba::new(221, 160, 221, 1)
            );
            assert_eq!(
                css::parse("powderblue").unwrap().unwrap().rgba(),
                Srgba::new(176, 224, 230, 1)
            );
            assert_eq!(
                css::parse("purple").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 128, 1)
            );
            assert_eq!(
                css::parse("rebeccapurple").unwrap().unwrap().rgba(),
                Srgba::new(102, 51, 153, 1)
            );
            assert_eq!(
                css::parse("red").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rosybrown").unwrap().unwrap().rgba(),
                Srgba::new(188, 143, 143, 1)
            );
            assert_eq!(
                css::parse("royalblue").unwrap().unwrap().rgba(),
                Srgba::new(65, 105, 225, 1)
            );
            assert_eq!(
                css::parse("saddlebrown").unwrap().unwrap().rgba(),
                Srgba::new(139, 69, 19, 1)
            );
            assert_eq!(
                css::parse("salmon").unwrap().unwrap().rgba(),
                Srgba::new(250, 128, 114, 1)
            );
            assert_eq!(
                css::parse("sandybrown").unwrap().unwrap().rgba(),
                Srgba::new(244, 164, 96, 1)
            );
            assert_eq!(
                css::parse("seagreen").unwrap().unwrap().rgba(),
                Srgba::new(46, 139, 87, 1)
            );
            assert_eq!(
                css::parse("seashell").unwrap().unwrap().rgba(),
                Srgba::new(255, 245, 238, 1)
            );
            assert_eq!(
                css::parse("sienna").unwrap().unwrap().rgba(),
                Srgba::new(160, 82, 45, 1)
            );
            assert_eq!(
                css::parse("silver").unwrap().unwrap().rgba(),
                Srgba::new(192, 192, 192, 1)
            );
            assert_eq!(
                css::parse("skyblue").unwrap().unwrap().rgba(),
                Srgba::new(135, 206, 235, 1)
            );
            assert_eq!(
                css::parse("slateblue").unwrap().unwrap().rgba(),
                Srgba::new(106, 90, 205, 1)
            );
            assert_eq!(
                css::parse("slategray").unwrap().unwrap().rgba(),
                Srgba::new(112, 128, 144, 1)
            );
            assert_eq!(
                css::parse("slategrey").unwrap().unwrap().rgba(),
                Srgba::new(112, 128, 144, 1)
            );
            assert_eq!(
                css::parse("snow").unwrap().unwrap().rgba(),
                Srgba::new(255, 250, 250, 1)
            );
            assert_eq!(
                css::parse("springgreen").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 127, 1)
            );
            assert_eq!(
                css::parse("steelblue").unwrap().unwrap().rgba(),
                Srgba::new(70, 130, 180, 1)
            );
            assert_eq!(
                css::parse("tan").unwrap().unwrap().rgba(),
                Srgba::new(210, 180, 140, 1)
            );
            assert_eq!(
                css::parse("teal").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 128, 1)
            );
            assert_eq!(
                css::parse("thistle").unwrap().unwrap().rgba(),
                Srgba::new(216, 191, 216, 1)
            );
            assert_eq!(
                css::parse("tomato").unwrap().unwrap().rgba(),
                Srgba::new(255, 99, 71, 1)
            );
            assert_eq!(
                css::parse("turquoise").unwrap().unwrap().rgba(),
                Srgba::new(64, 224, 208, 1)
            );
            assert_eq!(
                css::parse("violet").unwrap().unwrap().rgba(),
                Srgba::new(238, 130, 238, 1)
            );
            assert_eq!(
                css::parse("wheat").unwrap().unwrap().rgba(),
                Srgba::new(245, 222, 179, 1)
            );
            assert_eq!(
                css::parse("white").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 255, 1)
            );
            assert_eq!(
                css::parse("whitesmoke").unwrap().unwrap().rgba(),
                Srgba::new(245, 245, 245, 1)
            );
            assert_eq!(
                css::parse("yellow").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("yellowgreen").unwrap().unwrap().rgba(),
                Srgba::new(154, 205, 50, 1)
            );
        }

        #[test]
        fn hex_color() {
            // somewhat valid

            assert_eq!(
                css::parse("#FFFFG0").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("#FFFFg0").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("#-FFF00").unwrap().unwrap().rgba(),
                Srgba::new(15, 255, 0, 1)
            );

            // valid

            assert_eq!(
                css::parse("#000000").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("#FFFFFF").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 255, 1)
            );
            assert_eq!(
                css::parse("#FF0000").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );
            assert_eq!(
                css::parse("#00FF00").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 0, 1)
            );
            assert_eq!(
                css::parse("#0000FF").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 255, 1)
            );
            assert_eq!(
                css::parse("#FFFF00").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("#00FFFF").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 255, 1)
            );
            assert_eq!(
                css::parse("#FF00FF").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 255, 1)
            );
            assert_eq!(
                css::parse("#C0C0C0").unwrap().unwrap().rgba(),
                Srgba::new(192, 192, 192, 1)
            );
            assert_eq!(
                css::parse("#808080").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 128, 1)
            );
            assert_eq!(
                css::parse("#800000").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 0, 1)
            );
            assert_eq!(
                css::parse("#808000").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 0, 1)
            );
            assert_eq!(
                css::parse("#008000").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 0, 1)
            );
            assert_eq!(
                css::parse("#800080").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 128, 1)
            );
            assert_eq!(
                css::parse("#008080").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 128, 1)
            );
            assert_eq!(
                css::parse("#000080").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 128, 1)
            );
            assert_eq!(
                css::parse("#010203").unwrap().unwrap().rgba(),
                Srgba::new(1, 2, 3, 1)
            );
            assert_eq!(
                css::parse("#040506").unwrap().unwrap().rgba(),
                Srgba::new(4, 5, 6, 1)
            );
            assert_eq!(
                css::parse("#070809").unwrap().unwrap().rgba(),
                Srgba::new(7, 8, 9, 1)
            );
            assert_eq!(
                css::parse("#0a0A0a").unwrap().unwrap().rgba(),
                Srgba::new(10, 10, 10, 1)
            );
            assert_eq!(
                css::parse("#0b0B0b").unwrap().unwrap().rgba(),
                Srgba::new(11, 11, 11, 1)
            );
            assert_eq!(
                css::parse("#0c0C0c").unwrap().unwrap().rgba(),
                Srgba::new(12, 12, 12, 1)
            );
            assert_eq!(
                css::parse("#0d0D0d").unwrap().unwrap().rgba(),
                Srgba::new(13, 13, 13, 1)
            );
            assert_eq!(
                css::parse("#0e0E0e").unwrap().unwrap().rgba(),
                Srgba::new(14, 14, 14, 1)
            );
            assert_eq!(
                css::parse("#0f0F0f").unwrap().unwrap().rgba(),
                Srgba::new(15, 15, 15, 1)
            );
            assert_eq!(
                css::parse("#a0A0a0").unwrap().unwrap().rgba(),
                Srgba::new(160, 160, 160, 1)
            );
            assert_eq!(
                css::parse("#CFA").unwrap().unwrap().rgba(),
                Srgba::new(204, 255, 170, 1)
            );
            assert_eq!(
                css::parse("#CFA8").unwrap().unwrap().rgba(),
                Srgba::new(204, 255, 170, 0.533)
            );
        }

        #[test]
        fn rgb() {
            // somewhat valid / unusual
            assert_eq!(
                css::parse("rgb(-255, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::opaque(0, 0, 0)
            );
            assert_eq!(
                css::parse("rgb(+255, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::opaque(255, 0, 0)
            );
            assert_eq!(
                css::parse("rgb(800, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::opaque(255, 0, 0)
            );

            // valid

            assert_eq!(
                css::parse("rgb(0, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(255, 255, 255)").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 255, 1)
            );
            assert_eq!(
                css::parse("rgb(255, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 255, 0)").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 0, 255)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 255, 1)
            );
            assert_eq!(
                css::parse("rgb(255, 255, 0)").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 255, 255)").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 255, 1)
            );
            assert_eq!(
                css::parse("rgb(255, 0, 255)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 255, 1)
            );
            assert_eq!(
                css::parse("rgb(192, 192, 192)").unwrap().unwrap().rgba(),
                Srgba::new(192, 192, 192, 1)
            );
            assert_eq!(
                css::parse("rgb(128, 128, 128)").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 128, 1)
            );
            assert_eq!(
                css::parse("rgb(128, 0, 0)").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(128, 128, 0)").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 128, 0)").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 0, 1)
            );
            assert_eq!(
                css::parse("rgb(128, 0, 128)").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 128, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 128, 128)").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 128, 1)
            );
            assert_eq!(
                css::parse("rgb(0, 0, 128)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 128, 1)
            );
            assert_eq!(
                css::parse("rgb(1, 2, 3)").unwrap().unwrap().rgba(),
                Srgba::new(1, 2, 3, 1)
            );
            assert_eq!(
                css::parse("rgb(4, 5, 6)").unwrap().unwrap().rgba(),
                Srgba::new(4, 5, 6, 1)
            );
            assert_eq!(
                css::parse("rgb(7, 8, 9)").unwrap().unwrap().rgba(),
                Srgba::new(7, 8, 9, 1)
            );
            assert_eq!(
                css::parse("rgb(10, 10, 10)").unwrap().unwrap().rgba(),
                Srgba::new(10, 10, 10, 1)
            );
            assert_eq!(
                css::parse("rgb(11, 11, 11)").unwrap().unwrap().rgba(),
                Srgba::new(11, 11, 11, 1)
            );
            assert_eq!(
                css::parse("rgb(12, 12, 12)").unwrap().unwrap().rgba(),
                Srgba::new(12, 12, 12, 1)
            );
            assert_eq!(
                css::parse("rgb(13, 13, 13)").unwrap().unwrap().rgba(),
                Srgba::new(13, 13, 13, 1)
            );
            assert_eq!(
                css::parse("rgb(14, 14, 14)").unwrap().unwrap().rgba(),
                Srgba::new(14, 14, 14, 1)
            );
            assert_eq!(
                css::parse("rgb(15, 15, 15)").unwrap().unwrap().rgba(),
                Srgba::new(15, 15, 15, 1)
            );
        }

        #[test]
        fn rgba() {
            // somewhat valid / unusual
            assert_eq!(
                css::parse("rgba(0, 0, 0, 255)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(-255, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(+255, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(800, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );

            // alpha values

            assert_eq!(
                css::parse("rgba(255, 0, 0, 0.2)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 0.2)
            );
            assert_eq!(
                css::parse("rgba(255, 0, 0, 0.5)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 0.5)
            );
            assert_eq!(
                css::parse("rgba(255, 0, 0, 0.75)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 0.75)
            );

            // valid

            assert_eq!(
                css::parse("rgba(0, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(255, 255, 255, 1)")
                    .unwrap()
                    .unwrap()
                    .rgba(),
                Srgba::new(255, 255, 255, 1)
            );
            assert_eq!(
                css::parse("rgba(255, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 255, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 0, 255, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 255, 1)
            );
            assert_eq!(
                css::parse("rgba(255, 255, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(255, 255, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 255, 255, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 255, 255, 1)
            );
            assert_eq!(
                css::parse("rgba(255, 0, 255, 1)").unwrap().unwrap().rgba(),
                Srgba::new(255, 0, 255, 1)
            );
            assert_eq!(
                css::parse("rgba(192, 192, 192, 1)")
                    .unwrap()
                    .unwrap()
                    .rgba(),
                Srgba::new(192, 192, 192, 1)
            );
            assert_eq!(
                css::parse("rgba(128, 128, 128, 1)")
                    .unwrap()
                    .unwrap()
                    .rgba(),
                Srgba::new(128, 128, 128, 1)
            );
            assert_eq!(
                css::parse("rgba(128, 0, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(128, 128, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(128, 128, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 128, 0, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 0, 1)
            );
            assert_eq!(
                css::parse("rgba(128, 0, 128, 1)").unwrap().unwrap().rgba(),
                Srgba::new(128, 0, 128, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 128, 128, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 128, 128, 1)
            );
            assert_eq!(
                css::parse("rgba(0, 0, 128, 1)").unwrap().unwrap().rgba(),
                Srgba::new(0, 0, 128, 1)
            );
            assert_eq!(
                css::parse("rgba(1, 2, 3, 1)").unwrap().unwrap().rgba(),
                Srgba::new(1, 2, 3, 1)
            );
            assert_eq!(
                css::parse("rgba(4, 5, 6, 1)").unwrap().unwrap().rgba(),
                Srgba::new(4, 5, 6, 1)
            );
            assert_eq!(
                css::parse("rgba(7, 8, 9, 1)").unwrap().unwrap().rgba(),
                Srgba::new(7, 8, 9, 1)
            );
            assert_eq!(
                css::parse("rgba(10, 10, 10, 1)").unwrap().unwrap().rgba(),
                Srgba::new(10, 10, 10, 1)
            );
            assert_eq!(
                css::parse("rgba(11, 11, 11, 1)").unwrap().unwrap().rgba(),
                Srgba::new(11, 11, 11, 1)
            );
            assert_eq!(
                css::parse("rgba(12, 12, 12, 1)").unwrap().unwrap().rgba(),
                Srgba::new(12, 12, 12, 1)
            );
            assert_eq!(
                css::parse("rgba(13, 13, 13, 1)").unwrap().unwrap().rgba(),
                Srgba::new(13, 13, 13, 1)
            );
            assert_eq!(
                css::parse("rgba(14, 14, 14, 1)").unwrap().unwrap().rgba(),
                Srgba::new(14, 14, 14, 1)
            );
            assert_eq!(
                css::parse("rgba(15, 15, 15, 1)").unwrap().unwrap().rgba(),
                Srgba::new(15, 15, 15, 1)
            );
        }
    }

    #[test]
    fn parse_hex() {
        // invalid
        assert!(css::parse_hex("").is_none());
        assert!(css::parse_hex("#").is_none());
        assert!(css::parse_hex("#0102030").is_none());

        // somewhat valid

        assert_eq!(
            css::parse_hex("#FFFFG0").unwrap().rgba(),
            Srgba::new(255, 255, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#FFFFg0").unwrap().rgba(),
            Srgba::new(255, 255, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#-FFF00").unwrap().rgba(),
            Srgba::new(15, 255, 0, 1)
        );

        // valid

        assert_eq!(
            css::parse_hex("#000000").unwrap().rgba(),
            Srgba::new(0, 0, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#FFFFFF").unwrap().rgba(),
            Srgba::new(255, 255, 255, 1)
        );
        assert_eq!(
            css::parse_hex("#FF0000").unwrap().rgba(),
            Srgba::new(255, 0, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#00FF00").unwrap().rgba(),
            Srgba::new(0, 255, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#0000FF").unwrap().rgba(),
            Srgba::new(0, 0, 255, 1)
        );
        assert_eq!(
            css::parse_hex("#FFFF00").unwrap().rgba(),
            Srgba::new(255, 255, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#00FFFF").unwrap().rgba(),
            Srgba::new(0, 255, 255, 1)
        );
        assert_eq!(
            css::parse_hex("#FF00FF").unwrap().rgba(),
            Srgba::new(255, 0, 255, 1)
        );
        assert_eq!(
            css::parse_hex("#C0C0C0").unwrap().rgba(),
            Srgba::new(192, 192, 192, 1)
        );
        assert_eq!(
            css::parse_hex("#808080").unwrap().rgba(),
            Srgba::new(128, 128, 128, 1)
        );
        assert_eq!(
            css::parse_hex("#800000").unwrap().rgba(),
            Srgba::new(128, 0, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#808000").unwrap().rgba(),
            Srgba::new(128, 128, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#008000").unwrap().rgba(),
            Srgba::new(0, 128, 0, 1)
        );
        assert_eq!(
            css::parse_hex("#800080").unwrap().rgba(),
            Srgba::new(128, 0, 128, 1)
        );
        assert_eq!(
            css::parse_hex("#008080").unwrap().rgba(),
            Srgba::new(0, 128, 128, 1)
        );
        assert_eq!(
            css::parse_hex("#000080").unwrap().rgba(),
            Srgba::new(0, 0, 128, 1)
        );
        assert_eq!(
            css::parse_hex("#010203").unwrap().rgba(),
            Srgba::new(1, 2, 3, 1)
        );
        assert_eq!(
            css::parse_hex("#040506").unwrap().rgba(),
            Srgba::new(4, 5, 6, 1)
        );
        assert_eq!(
            css::parse_hex("#070809").unwrap().rgba(),
            Srgba::new(7, 8, 9, 1)
        );
        assert_eq!(
            css::parse_hex("#0a0A0a").unwrap().rgba(),
            Srgba::new(10, 10, 10, 1)
        );
        assert_eq!(
            css::parse_hex("#0b0B0b").unwrap().rgba(),
            Srgba::new(11, 11, 11, 1)
        );
        assert_eq!(
            css::parse_hex("#0c0C0c").unwrap().rgba(),
            Srgba::new(12, 12, 12, 1)
        );
        assert_eq!(
            css::parse_hex("#0d0D0d").unwrap().rgba(),
            Srgba::new(13, 13, 13, 1)
        );
        assert_eq!(
            css::parse_hex("#0e0E0e").unwrap().rgba(),
            Srgba::new(14, 14, 14, 1)
        );
        assert_eq!(
            css::parse_hex("#0f0F0f").unwrap().rgba(),
            Srgba::new(15, 15, 15, 1)
        );
        assert_eq!(
            css::parse_hex("#a0A0a0").unwrap().rgba(),
            Srgba::new(160, 160, 160, 1)
        );
        assert_eq!(
            css::parse_hex("#CFA").unwrap().rgba(),
            Srgba::new(204, 255, 170, 1)
        );
        assert_eq!(
            css::parse_hex("#CFA8").unwrap().rgba(),
            Srgba::new(204, 255, 170, 0.533)
        );
    }

    mod format {
        use super::*;

        #[test]
        fn format_hsl_should_use_whole_numbers_for_percentages() {
            // Test case matching the issue: color with fractional percentages that should be rounded
            let color1 = Color::from(Hsla::new(0, 0.857, 0.437, 1)); // Should format as hsl(0, 86%, 44%)
            assert_eq!(css::format_hsl(&color1), "hsl(0, 86%, 44%)");

            // Test edge cases
            let color2 = Color::from(Hsla::new(120, 0.5, 0.75, 1)); // Should format as hsl(120, 50%, 75%)
            assert_eq!(css::format_hsl(&color2), "hsl(120, 50%, 75%)");

            // Test case with values that would round differently
            let color3 = Color::from(Hsla::new(240, 0.334, 0.666, 1)); // Should format as hsl(240, 33%, 67%)
            assert_eq!(css::format_hsl(&color3), "hsl(240, 33%, 67%)");
        }

        #[test]
        fn format_hsla_should_use_whole_numbers_for_percentages() {
            // Test case with alpha
            let color1 = Color::from(Hsla::new(0, 0.857, 0.437, 0.85)); // Should format as hsla(0, 86%, 44%, 0.85)
            assert_eq!(css::format_hsla(&color1), "hsla(0, 86%, 44%, 0.85)");

            // Test edge cases
            let color2 = Color::from(Hsla::new(180, 0.25, 0.5, 0.5)); // Should format as hsla(180, 25%, 50%, 0.50)
            assert_eq!(css::format_hsla(&color2), "hsla(180, 25%, 50%, 0.50)");
        }
    }
}

fn rgba_from_int(int: u32) -> Color {
    Color::from(Srgba::new(
        (int >> 24) & 0xff,
        (int >> 16) & 0xff,
        (int >> 8) & 0xff,
        int & 0xff,
    ))
}

fn assert_contrast_ratio(background: u32, foreground: u32, ratio: f64, expected: u32) {
    let bg_color = rgba_from_int(background);
    let fg_color = rgba_from_int(foreground);
    assert_eq!(
        bg_color.ensure_contrast(&fg_color, ratio).rgba(),
        rgba_from_int(expected).rgba()
    );
}

// https://github.com/xtermjs/xterm.js/blob/44f9fa39ae03e2ca6d28354d88a399608686770e/src/common/Color.test.ts#L355
mod ensure_contrast_ratio {
    use super::*;

    #[test]
    fn should_return_undefined_if_the_color_already_meets_the_contrast_ratio_black_bg() {
        assert_contrast_ratio(0x000000ff, 0x606060ff, 1.0, 0x606060ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 2.0, 0x606060ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 3.0, 0x606060ff);
    }

    #[test]
    fn should_return_a_color_that_meets_the_contrast_ratio_black_bg() {
        assert_contrast_ratio(0x000000ff, 0x606060ff, 4.0, 0x707070ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 5.0, 0x7f7f7fff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 6.0, 0x8c8c8cff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 7.0, 0x989898ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 8.0, 0xa3a3a3ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 9.0, 0xadadadff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 10.0, 0xb6b6b6ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 11.0, 0xbebebeff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 12.0, 0xc5c5c5ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 13.0, 0xd1d1d1ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 14.0, 0xd6d6d6ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 15.0, 0xdbdbdbff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 16.0, 0xe3e3e3ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 17.0, 0xe9e9e9ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 18.0, 0xeeeeeeff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 19.0, 0xf4f4f4ff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 20.0, 0xfafafaff);
        assert_contrast_ratio(0x000000ff, 0x606060ff, 21.0, 0xffffffff);
    }

    #[test]
    fn should_return_undefined_if_the_color_already_meets_the_contrast_ratio_white_bg() {
        assert_contrast_ratio(0xffffffff, 0x606060ff, 1.0, 0x606060ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 2.0, 0x606060ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 3.0, 0x606060ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 4.0, 0x606060ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 5.0, 0x606060ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 6.0, 0x606060ff);
    }

    #[test]
    fn should_return_a_color_that_meets_the_contrast_ratio_white_bg() {
        assert_contrast_ratio(0xffffffff, 0x606060ff, 7.0, 0x565656ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 8.0, 0x4d4d4dff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 9.0, 0x454545ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 10.0, 0x3e3e3eff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 11.0, 0x373737ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 12.0, 0x313131ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 13.0, 0x313131ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 14.0, 0x272727ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 15.0, 0x232323ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 16.0, 0x1f1f1fff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 17.0, 0x1b1b1bff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 18.0, 0x151515ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 19.0, 0x101010ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 20.0, 0x080808ff);
        assert_contrast_ratio(0xffffffff, 0x606060ff, 21.0, 0x000000ff);
    }
}
