//! Porte de `src/vs/base/common/color.ts`: a classe `Color` com RGBA, HSLA e HSVA, as conversões, as
//! operações de cor (`darken`, `lighten`, `transparent`, `blend`, `mix`, `makeOpaque`...) e o
//! `Color.Format.CSS` (`format`, `parse`, `parseHex`).
//!
//! Correspondência de nomes (o nome em maiúsculas do original viola a convenção do Rust):
//! `RGBA` é [`Srgba`], `HSLA` é [`Hsla`], `HSVA` é [`Hsva`], `Color.Format.CSS` é [`format::css`].
//!
//! Os números seguem a aritmética do original: canais `r`, `g`, `b` inteiros truncados por `| 0`,
//! saturação, luminosidade e alfa arredondados a três casas (`roundFloat`), `Math.round` do
//! JavaScript (meio para cima) e `toFixed` do JavaScript (meio para cima sobre o valor exato).

/// `roundFloat` do original.
fn round_float(number: f64, decimal_points: i32) -> f64 {
    let decimal = 10f64.powi(decimal_points);
    js_round(number * decimal) / decimal
}

/// `Math.round` do JavaScript: arredonda o meio para cima (para `+inf`).
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `(x).toFixed(digits)` do JavaScript para `x` finito: o valor exato do `f64` é arredondado e o
/// empate vai para o maior (o `format!` do Rust desempata para o par, por isso a conta manual).
fn to_fixed(x: f64, digits: usize) -> String {
    if x < 0.0 {
        return format!("-{}", to_fixed(-x, digits));
    }
    // 1100 casas bastam para a expansão decimal exata de qualquer `f64`.
    let exact = format!("{x:.1100}");
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut kept: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes().take(digits))
        .collect();
    if frac_part.as_bytes().get(digits).is_some_and(|&d| d >= b'5') {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let int_len = kept.len() - digits;
    let mut out = String::from_utf8_lossy(&kept[..int_len]).into_owned();
    if digits > 0 {
        out.push('.');
        out.push_str(&String::from_utf8_lossy(&kept[int_len..]));
    }
    out
}

/// O `RGBA` do original: `r`, `g`, `b` inteiros em [0, 255] e `a` em [0, 1] com três casas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Srgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl Srgba {
    /// `new RGBA(r, g, b, a)`: trunca os canais (`| 0`) depois de limitá-los a [0, 255] e arredonda o
    /// alfa limitado a [0, 1] em três casas.
    pub fn new(r: impl Into<f64>, g: impl Into<f64>, b: impl Into<f64>, a: impl Into<f64>) -> Self {
        let channel = |v: f64| v.clamp(0.0, 255.0) as u8;
        Self {
            r: channel(r.into()),
            g: channel(g.into()),
            b: channel(b.into()),
            a: round_float(a.into().clamp(0.0, 1.0), 3),
        }
    }

    /// `new RGBA(r, g, b)`: alfa 1.
    pub fn opaque(r: impl Into<f64>, g: impl Into<f64>, b: impl Into<f64>) -> Self {
        Self::new(r, g, b, 1)
    }
}

/// O `HSLA` do original: matiz inteira em [0, 360], saturação, luminosidade e alfa em [0, 1].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsla {
    pub h: u16,
    pub s: f64,
    pub l: f64,
    pub a: f64,
}

impl Hsla {
    pub fn new(h: impl Into<f64>, s: impl Into<f64>, l: impl Into<f64>, a: impl Into<f64>) -> Self {
        Self {
            h: h.into().clamp(0.0, 360.0) as u16,
            s: round_float(s.into().clamp(0.0, 1.0), 3),
            l: round_float(l.into().clamp(0.0, 1.0), 3),
            a: round_float(a.into().clamp(0.0, 1.0), 3),
        }
    }

    /// Conversão de RGB para HSL, adaptada de http://en.wikipedia.org/wiki/HSL_color_space.
    pub fn from_rgba(rgba: Srgba) -> Self {
        let r = f64::from(rgba.r) / 255.0;
        let g = f64::from(rgba.g) / 255.0;
        let b = f64::from(rgba.b) / 255.0;
        let a = rgba.a;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let mut h = 0.0;
        let mut s = 0.0;
        let l = (min + max) / 2.0;
        let chroma = max - min;

        if chroma > 0.0 {
            s = (if l <= 0.5 {
                chroma / (2.0 * l)
            } else {
                chroma / (2.0 - (2.0 * l))
            })
            .min(1.0);

            h = if max == r {
                (g - b) / chroma + if g < b { 6.0 } else { 0.0 }
            } else if max == g {
                (b - r) / chroma + 2.0
            } else {
                (r - g) / chroma + 4.0
            };

            h *= 60.0;
            h = js_round(h);
        }
        Self::new(h, s, l, a)
    }

    fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    }

    /// Conversão de HSL para RGB, adaptada de http://en.wikipedia.org/wiki/HSL_color_space.
    pub fn to_rgba(self) -> Srgba {
        let h = f64::from(self.h) / 360.0;
        let Self { s, l, a, .. } = self;
        let (r, g, b);

        if s == 0.0 {
            // acromático
            (r, g, b) = (l, l, l);
        } else {
            let q = if l < 0.5 {
                l * (1.0 + s)
            } else {
                l + s - l * s
            };
            let p = 2.0 * l - q;
            r = Self::hue_to_rgb(p, q, h + 1.0 / 3.0);
            g = Self::hue_to_rgb(p, q, h);
            b = Self::hue_to_rgb(p, q, h - 1.0 / 3.0);
        }

        Srgba::new(
            js_round(r * 255.0),
            js_round(g * 255.0),
            js_round(b * 255.0),
            a,
        )
    }
}

/// O `HSVA` do original: matiz inteira em [0, 360], saturação, valor e alfa em [0, 1].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsva {
    pub h: u16,
    pub s: f64,
    pub v: f64,
    pub a: f64,
}

impl Hsva {
    pub fn new(h: impl Into<f64>, s: impl Into<f64>, v: impl Into<f64>, a: impl Into<f64>) -> Self {
        Self {
            h: h.into().clamp(0.0, 360.0) as u16,
            s: round_float(s.into().clamp(0.0, 1.0), 3),
            v: round_float(v.into().clamp(0.0, 1.0), 3),
            a: round_float(a.into().clamp(0.0, 1.0), 3),
        }
    }

    // de http://www.rapidtables.com/convert/color/rgb-to-hsv.htm
    pub fn from_rgba(rgba: Srgba) -> Self {
        let r = f64::from(rgba.r) / 255.0;
        let g = f64::from(rgba.g) / 255.0;
        let b = f64::from(rgba.b) / 255.0;
        let cmax = r.max(g).max(b);
        let cmin = r.min(g).min(b);
        let delta = cmax - cmin;
        let s = if cmax == 0.0 { 0.0 } else { delta / cmax };

        let m = if delta == 0.0 {
            0.0
        } else if cmax == r {
            ((((g - b) / delta) % 6.0) + 6.0) % 6.0
        } else if cmax == g {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };

        Self::new(js_round(m * 60.0), s, cmax, rgba.a)
    }

    // de http://www.rapidtables.com/convert/color/hsv-to-rgb.htm
    pub fn to_rgba(self) -> Srgba {
        let (h, s, v, a) = (f64::from(self.h), self.s, self.v, self.a);
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);

        if h < 60.0 {
            r = c;
            g = x;
        } else if h < 120.0 {
            r = x;
            g = c;
        } else if h < 180.0 {
            g = c;
            b = x;
        } else if h < 240.0 {
            g = x;
            b = c;
        } else if h < 300.0 {
            r = x;
            b = c;
        } else if h <= 360.0 {
            r = c;
            b = x;
        }

        Srgba::new(
            js_round((r + m) * 255.0),
            js_round((g + m) * 255.0),
            js_round((b + m) * 255.0),
            a,
        )
    }
}

/// `reduce_relative_luminace` ou `increase_relative_luminace`.
type Adjust = fn(&Color, &Color, f64) -> Color;

/// Uma cor. Guarda a forma com que foi criada (`_hsla`, `_hsva` do original): construída a partir de
/// HSLA ou HSVA, o `hsla()` ou `hsva()` devolve o valor original, que pode diferir do recalculado.
#[derive(Clone, Debug, PartialEq)]
pub struct Color {
    rgba: Srgba,
    hsla: Option<Hsla>,
    hsva: Option<Hsva>,
}

impl From<Srgba> for Color {
    fn from(rgba: Srgba) -> Self {
        Self {
            rgba,
            hsla: None,
            hsva: None,
        }
    }
}

impl From<Hsla> for Color {
    fn from(hsla: Hsla) -> Self {
        Self {
            rgba: hsla.to_rgba(),
            hsla: Some(hsla),
            hsva: None,
        }
    }
}

impl From<Hsva> for Color {
    fn from(hsva: Hsva) -> Self {
        Self {
            rgba: hsva.to_rgba(),
            hsla: None,
            hsva: Some(hsva),
        }
    }
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format::css::format(self))
    }
}

impl Color {
    /// `Color.fromHex`: o que não é um hexadecimal válido vira vermelho.
    pub fn from_hex(hex: &str) -> Color {
        format::css::parse_hex(hex).unwrap_or_else(Color::red)
    }

    /// `Color.equals`, com `null` representado por `None`.
    pub fn equals_opt(a: Option<&Color>, b: Option<&Color>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => a.equals(b),
            _ => false,
        }
    }

    pub fn rgba(&self) -> Srgba {
        self.rgba
    }

    /// O alfa em 8 bits, `Math.round(a * 255)`, como `formatHexA` o escreve.
    pub fn alpha_byte(&self) -> u8 {
        js_round(self.rgba.a * 255.0) as u8
    }

    pub fn hsla(&self) -> Hsla {
        self.hsla.unwrap_or_else(|| Hsla::from_rgba(self.rgba))
    }

    pub fn hsva(&self) -> Hsva {
        self.hsva.unwrap_or_else(|| Hsva::from_rgba(self.rgba))
    }

    pub fn equals(&self, other: &Color) -> bool {
        self.rgba == other.rgba && self.hsla() == other.hsla() && self.hsva() == other.hsva()
    }

    /// http://www.w3.org/TR/WCAG20/#relativeluminancedef
    /// Devolve um número em [0, 1]: 0 é o preto mais escuro e 1 o branco mais claro.
    pub fn get_relative_luminance(&self) -> f64 {
        let r = Self::relative_luminance_for_component(self.rgba.r);
        let g = Self::relative_luminance_for_component(self.rgba.g);
        let b = Self::relative_luminance_for_component(self.rgba.b);
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        round_float(luminance, 4)
    }

    /// Reduz o primeiro plano `foreground` sobre este fundo até passar da razão de luminância relativa.
    /// Baseado no xterm.js (`reduceRelativeLuminace`, com o erro de grafia do original).
    pub fn reduce_relative_luminace(&self, foreground: &Color, ratio: f64) -> Color {
        // Abordagem ingênua mas rápida: converter para HSL e voltar seria caro.
        let (mut fg_r, mut fg_g, mut fg_b) = (
            i32::from(foreground.rgba.r),
            i32::from(foreground.rgba.g),
            i32::from(foreground.rgba.b),
        );

        let mut cr = self.get_contrast_ratio(foreground);
        while cr < ratio && (fg_r > 0 || fg_g > 0 || fg_b > 0) {
            // Reduz em 10% até atingir a razão.
            fg_r -= 0.max((f64::from(fg_r) * 0.1).ceil() as i32);
            fg_g -= 0.max((f64::from(fg_g) * 0.1).ceil() as i32);
            fg_b -= 0.max((f64::from(fg_b) * 0.1).ceil() as i32);
            cr = self.get_contrast_ratio(&Color::from(Srgba::opaque(fg_r, fg_g, fg_b)));
        }

        Color::from(Srgba::opaque(fg_r, fg_g, fg_b))
    }

    /// Aumenta o primeiro plano `foreground` sobre este fundo até passar da razão de luminância relativa.
    pub fn increase_relative_luminace(&self, foreground: &Color, ratio: f64) -> Color {
        let (mut fg_r, mut fg_g, mut fg_b) = (
            i32::from(foreground.rgba.r),
            i32::from(foreground.rgba.g),
            i32::from(foreground.rgba.b),
        );
        let mut cr = self.get_contrast_ratio(foreground);
        while cr < ratio && (fg_r < 0xFF || fg_g < 0xFF || fg_b < 0xFF) {
            fg_r = 0xFF.min(fg_r + (f64::from(255 - fg_r) * 0.1).ceil() as i32);
            fg_g = 0xFF.min(fg_g + (f64::from(255 - fg_g) * 0.1).ceil() as i32);
            fg_b = 0xFF.min(fg_b + (f64::from(255 - fg_b) * 0.1).ceil() as i32);
            cr = self.get_contrast_ratio(&Color::from(Srgba::opaque(fg_r, fg_g, fg_b)));
        }

        Color::from(Srgba::opaque(fg_r, fg_g, fg_b))
    }

    fn relative_luminance_for_component(color: u8) -> f64 {
        let c = f64::from(color) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// http://www.w3.org/TR/WCAG20/#contrast-ratiodef
    /// Devolve a razão de contraste em [1, 21].
    pub fn get_contrast_ratio(&self, another: &Color) -> f64 {
        let lum1 = self.get_relative_luminance();
        let lum2 = another.get_relative_luminance();
        if lum1 > lum2 {
            (lum1 + 0.05) / (lum2 + 0.05)
        } else {
            (lum2 + 0.05) / (lum1 + 0.05)
        }
    }

    /// http://24ways.org/2010/calculating-color-contrast
    /// `true` se a cor é escura.
    pub fn is_darker(&self) -> bool {
        self.yiq() < 128.0
    }

    /// http://24ways.org/2010/calculating-color-contrast
    /// `true` se a cor é clara.
    pub fn is_lighter(&self) -> bool {
        self.yiq() >= 128.0
    }

    fn yiq(&self) -> f64 {
        (f64::from(self.rgba.r) * 299.0
            + f64::from(self.rgba.g) * 587.0
            + f64::from(self.rgba.b) * 114.0)
            / 1000.0
    }

    pub fn is_lighter_than(&self, another: &Color) -> bool {
        self.get_relative_luminance() > another.get_relative_luminance()
    }

    pub fn is_darker_than(&self, another: &Color) -> bool {
        self.get_relative_luminance() < another.get_relative_luminance()
    }

    /// Baseado no xterm.js: dado um primeiro plano e este fundo, aumenta ou reduz a luminância do
    /// primeiro plano até atingir a razão de contraste. Se bater no branco ou no preto puro sem atingir,
    /// vai para o outro lado e escolhe o resultado de maior contraste (`ensureConstrast` do original).
    pub fn ensure_contrast(&self, foreground: &Color, ratio: f64) -> Color {
        let bg_l = self.get_relative_luminance();
        let fg_l = foreground.get_relative_luminance();
        let cr = self.get_contrast_ratio(foreground);
        if cr < ratio {
            let (first, second): (Adjust, Adjust) = if fg_l < bg_l {
                (
                    Color::reduce_relative_luminace,
                    Color::increase_relative_luminace,
                )
            } else {
                (
                    Color::increase_relative_luminace,
                    Color::reduce_relative_luminace,
                )
            };
            let result_a = first(self, foreground, ratio);
            let result_a_ratio = self.get_contrast_ratio(&result_a);
            if result_a_ratio < ratio {
                let result_b = second(self, foreground, ratio);
                let result_b_ratio = self.get_contrast_ratio(&result_b);
                return if result_a_ratio > result_b_ratio {
                    result_a
                } else {
                    result_b
                };
            }
            return result_a;
        }

        foreground.clone()
    }

    pub fn lighten(&self, factor: f64) -> Color {
        let hsla = self.hsla();
        Color::from(Hsla::new(hsla.h, hsla.s, hsla.l + hsla.l * factor, hsla.a))
    }

    pub fn darken(&self, factor: f64) -> Color {
        let hsla = self.hsla();
        Color::from(Hsla::new(hsla.h, hsla.s, hsla.l - hsla.l * factor, hsla.a))
    }

    pub fn transparent(&self, factor: f64) -> Color {
        let Srgba { r, g, b, a } = self.rgba;
        Color::from(Srgba::new(r, g, b, a * factor))
    }

    pub fn is_transparent(&self) -> bool {
        self.rgba.a == 0.0
    }

    pub fn is_opaque(&self) -> bool {
        self.rgba.a == 1.0
    }

    pub fn opposite(&self) -> Color {
        Color::from(Srgba::new(
            255 - i32::from(self.rgba.r),
            255 - i32::from(self.rgba.g),
            255 - i32::from(self.rgba.b),
            self.rgba.a,
        ))
    }

    pub fn blend(&self, c: &Color) -> Color {
        let rgba = c.rgba;

        // Converte para opacidade 0..1.
        let this_a = self.rgba.a;
        let color_a = rgba.a;

        let a = this_a + color_a * (1.0 - this_a);
        if a < 1e-6 {
            return Color::transparent_color();
        }

        let mix = |this: u8, other: u8| {
            f64::from(this) * this_a / a + f64::from(other) * color_a * (1.0 - this_a) / a
        };
        Color::from(Srgba::new(
            mix(self.rgba.r, rgba.r),
            mix(self.rgba.g, rgba.g),
            mix(self.rgba.b, rgba.b),
            a,
        ))
    }

    /// Mistura esta cor com `color`: o fator 0 é esta cor, 1 é `color`, 0.5 é a mistura igual (padrão).
    pub fn mix(&self, color: &Color, factor: Option<f64>) -> Color {
        let normalize = factor.unwrap_or(0.5).clamp(0.0, 1.0);
        let this = self.rgba;
        let other = color.rgba;

        let lerp = |a: f64, b: f64| a + (b - a) * normalize;
        Color::from(Srgba::new(
            lerp(f64::from(this.r), f64::from(other.r)),
            lerp(f64::from(this.g), f64::from(other.g)),
            lerp(f64::from(this.b), f64::from(other.b)),
            lerp(this.a, other.a),
        ))
    }

    pub fn make_opaque(&self, opaque_background: &Color) -> Color {
        if self.is_opaque() || opaque_background.rgba.a != 1.0 {
            // só se mistura uma cor não opaca sobre uma cor opaca
            return self.clone();
        }

        let Srgba { r, g, b, a } = self.rgba;
        let bg = opaque_background.rgba;

        // https://stackoverflow.com/questions/12228548/finding-equivalent-color-with-opacity
        Color::from(Srgba::new(
            f64::from(bg.r) - a * (f64::from(bg.r) - f64::from(r)),
            f64::from(bg.g) - a * (f64::from(bg.g) - f64::from(g)),
            f64::from(bg.b) - a * (f64::from(bg.b) - f64::from(b)),
            1,
        ))
    }

    /// `flatten`: achata esta cor sobre `backgrounds` (do último para o primeiro). Como o
    /// `reduceRight` sem valor inicial do original, falha (`None`) se a lista é vazia.
    pub fn flatten(&self, backgrounds: &[Color]) -> Option<Color> {
        let (last, rest) = backgrounds.split_last()?;
        let background = rest.iter().rfold(last.clone(), |accumulator, color| {
            Self::flatten_onto(color, &accumulator)
        });
        Some(Self::flatten_onto(self, &background))
    }

    fn flatten_onto(foreground: &Color, background: &Color) -> Color {
        let background_alpha = 1.0 - foreground.rgba.a;
        let channel =
            |bg: u8, fg: u8| background_alpha * f64::from(bg) + foreground.rgba.a * f64::from(fg);
        Color::from(Srgba::new(
            channel(background.rgba.r, foreground.rgba.r),
            channel(background.rgba.g, foreground.rgba.g),
            channel(background.rgba.b, foreground.rgba.b),
            1,
        ))
    }

    pub fn to_number_32bit(&self) -> u32 {
        (u32::from(self.rgba.r) << 24)
            | (u32::from(self.rgba.g) << 16)
            | (u32::from(self.rgba.b) << 8)
            | ((self.rgba.a * 255.0) as u32)
    }

    pub fn get_lighter_color(of: &Color, relative: &Color, factor: Option<f64>) -> Color {
        if of.is_lighter_than(relative) {
            return of.clone();
        }
        let factor = factor.filter(|f| *f != 0.0 && !f.is_nan()).unwrap_or(0.5);
        let lum1 = of.get_relative_luminance();
        let lum2 = relative.get_relative_luminance();
        let factor = factor * (lum2 - lum1) / lum2;
        of.lighten(factor)
    }

    pub fn get_darker_color(of: &Color, relative: &Color, factor: Option<f64>) -> Color {
        if of.is_darker_than(relative) {
            return of.clone();
        }
        let factor = factor.filter(|f| *f != 0.0 && !f.is_nan()).unwrap_or(0.5);
        let lum1 = of.get_relative_luminance();
        let lum2 = relative.get_relative_luminance();
        let factor = factor * (lum1 - lum2) / lum1;
        of.darken(factor)
    }

    pub fn white() -> Color {
        Color::from(Srgba::opaque(255, 255, 255))
    }

    pub fn black() -> Color {
        Color::from(Srgba::opaque(0, 0, 0))
    }

    pub fn red() -> Color {
        Color::from(Srgba::opaque(255, 0, 0))
    }

    pub fn blue() -> Color {
        Color::from(Srgba::opaque(0, 0, 255))
    }

    pub fn green() -> Color {
        Color::from(Srgba::opaque(0, 255, 0))
    }

    pub fn cyan() -> Color {
        Color::from(Srgba::opaque(0, 255, 255))
    }

    pub fn lightgrey() -> Color {
        Color::from(Srgba::opaque(211, 211, 211))
    }

    /// `Color.transparent`.
    pub fn transparent_color() -> Color {
        Color::from(Srgba::new(0, 0, 0, 0))
    }
}

/// O `Color.Format` do original.
pub mod format {
    /// O `Color.Format.CSS` do original.
    pub mod css {
        use super::super::{Color, Srgba, js_round, to_fixed};

        /// O `throw new Error('Invalid color format ' + css)` de `parse`.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct InvalidColorFormat(pub String);

        impl std::fmt::Display for InvalidColorFormat {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Invalid color format {}", self.0)
            }
        }

        impl std::error::Error for InvalidColorFormat {}

        pub fn format_rgb(color: &Color) -> String {
            if color.rgba.a == 1.0 {
                return format!("rgb({}, {}, {})", color.rgba.r, color.rgba.g, color.rgba.b);
            }

            format_rgba(color)
        }

        pub fn format_rgba(color: &Color) -> String {
            // `+(a).toFixed(2)`: o número volta a texto na forma mais curta ("0.5", "0", "1").
            let alpha: f64 = to_fixed(color.rgba.a, 2).parse().unwrap_or(0.0);
            format!(
                "rgba({}, {}, {}, {})",
                color.rgba.r, color.rgba.g, color.rgba.b, alpha
            )
        }

        pub fn format_hsl(color: &Color) -> String {
            let hsla = color.hsla();
            if hsla.a == 1.0 {
                return format!(
                    "hsl({}, {}%, {}%)",
                    hsla.h,
                    js_round(hsla.s * 100.0),
                    js_round(hsla.l * 100.0)
                );
            }

            format_hsla(color)
        }

        pub fn format_hsla(color: &Color) -> String {
            let hsla = color.hsla();
            format!(
                "hsla({}, {}%, {}%, {})",
                hsla.h,
                js_round(hsla.s * 100.0),
                js_round(hsla.l * 100.0),
                to_fixed(hsla.a, 2)
            )
        }

        /// Formata como #RRGGBB.
        pub fn format_hex(color: &Color) -> String {
            format!(
                "#{:02x}{:02x}{:02x}",
                color.rgba.r, color.rgba.g, color.rgba.b
            )
        }

        /// Formata como #RRGGBBAA. Com `compact`, cores sem transparência saem como #RRGGBB.
        pub fn format_hex_a(color: &Color, compact: bool) -> String {
            if compact && color.rgba.a == 1.0 {
                return format_hex(color);
            }

            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                color.rgba.r,
                color.rgba.g,
                color.rgba.b,
                color.alpha_byte()
            )
        }

        /// O formato padrão usa hexadecimal se opaca e RGBA do contrário.
        pub fn format(color: &Color) -> String {
            if color.is_opaque() {
                return format_hex(color);
            }

            format_rgba(color)
        }

        /// Lê uma cor CSS (`transparent`, hexadecimal, `rgba()`, `rgb()` ou nome). `Ok(None)` é o
        /// `null` do original; `Err` é o `throw` de `rgba(` e `rgb(` malformados.
        pub fn parse(css: &str) -> Result<Option<Color>, InvalidColorFormat> {
            if css == "transparent" {
                return Ok(Some(Color::transparent_color()));
            }
            if css.starts_with('#') {
                return Ok(parse_hex(css));
            }
            if css.starts_with("rgba(") {
                let (r, g, b, a) = match_function(css, "rgba(", true)
                    .ok_or_else(|| InvalidColorFormat(css.to_owned()))?;
                return Ok(Some(Color::from(Srgba::new(r, g, b, a))));
            }
            if css.starts_with("rgb(") {
                let (r, g, b, _) = match_function(css, "rgb(", false)
                    .ok_or_else(|| InvalidColorFormat(css.to_owned()))?;
                return Ok(Some(Color::from(Srgba::opaque(r, g, b))));
            }
            // O original anota um TODO aqui: aceitar mais formatos conforme a necessidade.
            Ok(parse_named_keyword(css))
        }

        /// O `css.match(/rgba\((?<r>(?:\+|-)?\d+), *(?<g>...), *(?<b>...), *(?<a>(?:\+|-)?\d+(\.\d+)?)\)/)`
        /// (ou a variante `rgb`), que procura em qualquer posição da cadeia.
        fn match_function(css: &str, open: &str, with_alpha: bool) -> Option<(f64, f64, f64, f64)> {
            let bytes = css.as_bytes();
            css.match_indices(open).find_map(|(start, _)| {
                let mut pos = start + open.len();
                let mut values = [0.0; 4];
                let count = if with_alpha { 4 } else { 3 };
                for (index, value) in values.iter_mut().enumerate().take(count) {
                    let is_alpha = with_alpha && index == 3;
                    let (number, next) = match_number(bytes, pos, is_alpha)?;
                    *value = number;
                    pos = next;
                    if index + 1 < count {
                        if bytes.get(pos) != Some(&b',') {
                            return None;
                        }
                        pos += 1;
                        while bytes.get(pos) == Some(&b' ') {
                            pos += 1;
                        }
                    }
                }
                (bytes.get(pos) == Some(&b')'))
                    .then_some((values[0], values[1], values[2], values[3]))
            })
        }

        /// `(?:\+|-)?\d+` (e `(\.\d+)?` quando `fractional`), já convertido como `parseInt` ou `parseFloat`.
        fn match_number(bytes: &[u8], start: usize, fractional: bool) -> Option<(f64, usize)> {
            let mut pos = start;
            if matches!(bytes.get(pos), Some(b'+' | b'-')) {
                pos += 1;
            }
            let digits_start = pos;
            while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                pos += 1;
            }
            if pos == digits_start {
                return None;
            }
            if fractional && bytes.get(pos) == Some(&b'.') {
                let mut after_dot = pos + 1;
                while bytes.get(after_dot).is_some_and(u8::is_ascii_digit) {
                    after_dot += 1;
                }
                if after_dot > pos + 1 {
                    pos = after_dot;
                }
            }
            let text = std::str::from_utf8(&bytes[start..pos]).ok()?;
            Some((text.parse::<f64>().ok()?, pos))
        }

        fn parse_named_keyword(css: &str) -> Option<Color> {
            // https://drafts.csswg.org/css-color/#named-colors
            let (r, g, b): (u8, u8, u8) = match css {
                "aliceblue" => (240, 248, 255),
                "antiquewhite" => (250, 235, 215),
                "aqua" => (0, 255, 255),
                "aquamarine" => (127, 255, 212),
                "azure" => (240, 255, 255),
                "beige" => (245, 245, 220),
                "bisque" => (255, 228, 196),
                "black" => (0, 0, 0),
                "blanchedalmond" => (255, 235, 205),
                "blue" => (0, 0, 255),
                "blueviolet" => (138, 43, 226),
                "brown" => (165, 42, 42),
                "burlywood" => (222, 184, 135),
                "cadetblue" => (95, 158, 160),
                "chartreuse" => (127, 255, 0),
                "chocolate" => (210, 105, 30),
                "coral" => (255, 127, 80),
                "cornflowerblue" => (100, 149, 237),
                "cornsilk" => (255, 248, 220),
                "crimson" => (220, 20, 60),
                "cyan" => (0, 255, 255),
                "darkblue" => (0, 0, 139),
                "darkcyan" => (0, 139, 139),
                "darkgoldenrod" => (184, 134, 11),
                "darkgray" => (169, 169, 169),
                "darkgreen" => (0, 100, 0),
                "darkgrey" => (169, 169, 169),
                "darkkhaki" => (189, 183, 107),
                "darkmagenta" => (139, 0, 139),
                "darkolivegreen" => (85, 107, 47),
                "darkorange" => (255, 140, 0),
                "darkorchid" => (153, 50, 204),
                "darkred" => (139, 0, 0),
                "darksalmon" => (233, 150, 122),
                "darkseagreen" => (143, 188, 143),
                "darkslateblue" => (72, 61, 139),
                "darkslategray" => (47, 79, 79),
                "darkslategrey" => (47, 79, 79),
                "darkturquoise" => (0, 206, 209),
                "darkviolet" => (148, 0, 211),
                "deeppink" => (255, 20, 147),
                "deepskyblue" => (0, 191, 255),
                "dimgray" => (105, 105, 105),
                "dimgrey" => (105, 105, 105),
                "dodgerblue" => (30, 144, 255),
                "firebrick" => (178, 34, 34),
                "floralwhite" => (255, 250, 240),
                "forestgreen" => (34, 139, 34),
                "fuchsia" => (255, 0, 255),
                "gainsboro" => (220, 220, 220),
                "ghostwhite" => (248, 248, 255),
                "gold" => (255, 215, 0),
                "goldenrod" => (218, 165, 32),
                "gray" => (128, 128, 128),
                "green" => (0, 128, 0),
                "greenyellow" => (173, 255, 47),
                "grey" => (128, 128, 128),
                "honeydew" => (240, 255, 240),
                "hotpink" => (255, 105, 180),
                "indianred" => (205, 92, 92),
                "indigo" => (75, 0, 130),
                "ivory" => (255, 255, 240),
                "khaki" => (240, 230, 140),
                "lavender" => (230, 230, 250),
                "lavenderblush" => (255, 240, 245),
                "lawngreen" => (124, 252, 0),
                "lemonchiffon" => (255, 250, 205),
                "lightblue" => (173, 216, 230),
                "lightcoral" => (240, 128, 128),
                "lightcyan" => (224, 255, 255),
                "lightgoldenrodyellow" => (250, 250, 210),
                "lightgray" => (211, 211, 211),
                "lightgreen" => (144, 238, 144),
                "lightgrey" => (211, 211, 211),
                "lightpink" => (255, 182, 193),
                "lightsalmon" => (255, 160, 122),
                "lightseagreen" => (32, 178, 170),
                "lightskyblue" => (135, 206, 250),
                "lightslategray" => (119, 136, 153),
                "lightslategrey" => (119, 136, 153),
                "lightsteelblue" => (176, 196, 222),
                "lightyellow" => (255, 255, 224),
                "lime" => (0, 255, 0),
                "limegreen" => (50, 205, 50),
                "linen" => (250, 240, 230),
                "magenta" => (255, 0, 255),
                "maroon" => (128, 0, 0),
                "mediumaquamarine" => (102, 205, 170),
                "mediumblue" => (0, 0, 205),
                "mediumorchid" => (186, 85, 211),
                "mediumpurple" => (147, 112, 219),
                "mediumseagreen" => (60, 179, 113),
                "mediumslateblue" => (123, 104, 238),
                "mediumspringgreen" => (0, 250, 154),
                "mediumturquoise" => (72, 209, 204),
                "mediumvioletred" => (199, 21, 133),
                "midnightblue" => (25, 25, 112),
                "mintcream" => (245, 255, 250),
                "mistyrose" => (255, 228, 225),
                "moccasin" => (255, 228, 181),
                "navajowhite" => (255, 222, 173),
                "navy" => (0, 0, 128),
                "oldlace" => (253, 245, 230),
                "olive" => (128, 128, 0),
                "olivedrab" => (107, 142, 35),
                "orange" => (255, 165, 0),
                "orangered" => (255, 69, 0),
                "orchid" => (218, 112, 214),
                "palegoldenrod" => (238, 232, 170),
                "palegreen" => (152, 251, 152),
                "paleturquoise" => (175, 238, 238),
                "palevioletred" => (219, 112, 147),
                "papayawhip" => (255, 239, 213),
                "peachpuff" => (255, 218, 185),
                "peru" => (205, 133, 63),
                "pink" => (255, 192, 203),
                "plum" => (221, 160, 221),
                "powderblue" => (176, 224, 230),
                "purple" => (128, 0, 128),
                "rebeccapurple" => (102, 51, 153),
                "red" => (255, 0, 0),
                "rosybrown" => (188, 143, 143),
                "royalblue" => (65, 105, 225),
                "saddlebrown" => (139, 69, 19),
                "salmon" => (250, 128, 114),
                "sandybrown" => (244, 164, 96),
                "seagreen" => (46, 139, 87),
                "seashell" => (255, 245, 238),
                "sienna" => (160, 82, 45),
                "silver" => (192, 192, 192),
                "skyblue" => (135, 206, 235),
                "slateblue" => (106, 90, 205),
                "slategray" => (112, 128, 144),
                "slategrey" => (112, 128, 144),
                "snow" => (255, 250, 250),
                "springgreen" => (0, 255, 127),
                "steelblue" => (70, 130, 180),
                "tan" => (210, 180, 140),
                "teal" => (0, 128, 128),
                "thistle" => (216, 191, 216),
                "tomato" => (255, 99, 71),
                "turquoise" => (64, 224, 208),
                "violet" => (238, 130, 238),
                "wheat" => (245, 222, 179),
                "white" => (255, 255, 255),
                "whitesmoke" => (245, 245, 245),
                "yellow" => (255, 255, 0),
                "yellowgreen" => (154, 205, 50),
                _ => return None,
            };
            Some(Color::from(Srgba::opaque(r, g, b)))
        }

        /// Converte um hexadecimal (#RGB, #RGBA, #RRGGBB ou #RRGGBBAA) em [`Color`]. O comprimento é
        /// medido em unidades UTF-16 e dígito inválido vale 0, como no original.
        pub fn parse_hex(hex: &str) -> Option<Color> {
            let units: Vec<u16> = hex.encode_utf16().collect();
            let length = units.len();

            if length == 0 {
                // cor inválida
                return None;
            }

            if units[0] != u16::from(b'#') {
                // não começa com #
                return None;
            }

            let digit = |i: usize| f64::from(parse_hex_digit(units[i]));

            match length {
                7 => {
                    // formato #RRGGBB
                    Some(Color::from(Srgba::new(
                        16.0 * digit(1) + digit(2),
                        16.0 * digit(3) + digit(4),
                        16.0 * digit(5) + digit(6),
                        1,
                    )))
                }
                9 => {
                    // formato #RRGGBBAA
                    let a = 16.0 * digit(7) + digit(8);
                    Some(Color::from(Srgba::new(
                        16.0 * digit(1) + digit(2),
                        16.0 * digit(3) + digit(4),
                        16.0 * digit(5) + digit(6),
                        a / 255.0,
                    )))
                }
                4 => {
                    // formato #RGB
                    let (r, g, b) = (digit(1), digit(2), digit(3));
                    Some(Color::from(Srgba::opaque(
                        16.0 * r + r,
                        16.0 * g + g,
                        16.0 * b + b,
                    )))
                }
                5 => {
                    // formato #RGBA
                    let (r, g, b, a) = (digit(1), digit(2), digit(3), digit(4));
                    Some(Color::from(Srgba::new(
                        16.0 * r + r,
                        16.0 * g + g,
                        16.0 * b + b,
                        (16.0 * a + a) / 255.0,
                    )))
                }
                // cor inválida
                _ => None,
            }
        }

        fn parse_hex_digit(char_code: u16) -> u8 {
            match u8::try_from(char_code) {
                Ok(c @ b'0'..=b'9') => c - b'0',
                Ok(c @ b'a'..=b'f') => c - b'a' + 10,
                Ok(c @ b'A'..=b'F') => c - b'A' + 10,
                _ => 0,
            }
        }
    }
}
