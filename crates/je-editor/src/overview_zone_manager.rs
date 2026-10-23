//! Porte de `src/vs/editor/common/viewModel/overviewZoneManager.ts`: converte as zonas da régua de visão geral
//! (linhas e cores) em zonas de cor em pixels de canvas, com altura mínima e cache por zona.

use std::cmp::Ordering;
use std::collections::HashMap;

/// `Constants.MINIMUM_HEIGHT`.
const MINIMUM_HEIGHT: f64 = 4.0;

/// Equivalente ao `x | 0` do JavaScript (ToInt32): não finito vira 0, o resto trunca e dá a volta em 32 bits.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let truncated = x.trunc();
    let modulus = 4_294_967_296.0_f64;
    let wrapped = truncated.rem_euclid(modulus);
    (wrapped as u32) as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorZone {
    pub from: i32,
    pub to: i32,
    pub color_id: i32,
}

impl ColorZone {
    pub fn new(from: i32, to: i32, color_id: i32) -> ColorZone {
        ColorZone { from, to, color_id }
    }

    /// `ColorZone.compare`: por `color_id`, depois `from`, depois `to`.
    pub fn compare(a: &ColorZone, b: &ColorZone) -> Ordering {
        if a.color_id == b.color_id {
            if a.from == b.from {
                return a.to.wrapping_sub(b.to).cmp(&0);
            }
            return a.from.wrapping_sub(b.from).cmp(&0);
        }
        a.color_id.wrapping_sub(b.color_id).cmp(&0)
    }
}

/// Uma zona na régua de visão geral.
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewRulerZone {
    pub start_line_number: u32,
    pub end_line_number: u32,
    /// Se for 0, a altura em linhas é determinada a partir de `end_line_number`.
    pub height_in_lines: f64,
    pub color: String,
    color_zone: Option<ColorZone>,
}

impl OverviewRulerZone {
    pub fn new(
        start_line_number: u32,
        end_line_number: u32,
        height_in_lines: f64,
        color: &str,
    ) -> OverviewRulerZone {
        OverviewRulerZone {
            start_line_number,
            end_line_number,
            height_in_lines,
            color: color.to_string(),
            color_zone: None,
        }
    }

    /// `OverviewRulerZone.compare`. A cor compara por unidades UTF-16, como `<` entre strings do JavaScript.
    pub fn compare(a: &OverviewRulerZone, b: &OverviewRulerZone) -> Ordering {
        if a.color == b.color {
            if a.start_line_number == b.start_line_number {
                if a.height_in_lines == b.height_in_lines {
                    return a.end_line_number.cmp(&b.end_line_number);
                }
                // Diferença NaN no JavaScript conta como empate no sort.
                return (a.height_in_lines - b.height_in_lines)
                    .partial_cmp(&0.0)
                    .unwrap_or(Ordering::Equal);
            }
            return a.start_line_number.cmp(&b.start_line_number);
        }
        if a.color.encode_utf16().cmp(b.color.encode_utf16()) == Ordering::Less {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    pub fn set_color_zone(&mut self, color_zone: ColorZone) {
        self.color_zone = Some(color_zone);
    }

    pub fn get_color_zones(&self) -> Option<ColorZone> {
        self.color_zone
    }
}

pub struct OverviewZoneManager {
    get_vertical_offset_for_line: Box<dyn Fn(u32) -> f64>,
    zones: Vec<OverviewRulerZone>,
    color_zones_invalid: bool,
    line_height: f64,
    dom_width: f64,
    dom_height: f64,
    outer_height: f64,
    pixel_ratio: f64,

    last_assigned_id: i32,
    color_2_id: HashMap<String, i32>,
    id_2_color: Vec<String>,
}

#[allow(clippy::float_cmp)]
impl OverviewZoneManager {
    pub fn new(get_vertical_offset_for_line: impl Fn(u32) -> f64 + 'static) -> OverviewZoneManager {
        OverviewZoneManager {
            get_vertical_offset_for_line: Box::new(get_vertical_offset_for_line),
            zones: Vec::new(),
            color_zones_invalid: false,
            line_height: 0.0,
            dom_width: 0.0,
            dom_height: 0.0,
            outer_height: 0.0,
            pixel_ratio: 1.0,
            last_assigned_id: 0,
            color_2_id: HashMap::new(),
            id_2_color: Vec::new(),
        }
    }

    /// Cores por id (o índice 0 nunca é atribuído e fica vazio).
    pub fn get_id_2_color(&self) -> &[String] {
        &self.id_2_color
    }

    pub fn set_zones(&mut self, new_zones: Vec<OverviewRulerZone>) {
        self.zones = new_zones;
        self.zones.sort_by(OverviewRulerZone::compare);
    }

    pub fn set_line_height(&mut self, line_height: f64) -> bool {
        if self.line_height == line_height {
            return false;
        }
        self.line_height = line_height;
        self.color_zones_invalid = true;
        true
    }

    pub fn set_pixel_ratio(&mut self, pixel_ratio: f64) {
        self.pixel_ratio = pixel_ratio;
        self.color_zones_invalid = true;
    }

    pub fn get_dom_width(&self) -> f64 {
        self.dom_width
    }

    pub fn get_canvas_width(&self) -> f64 {
        self.dom_width * self.pixel_ratio
    }

    pub fn set_dom_width(&mut self, width: f64) -> bool {
        if self.dom_width == width {
            return false;
        }
        self.dom_width = width;
        self.color_zones_invalid = true;
        true
    }

    pub fn get_dom_height(&self) -> f64 {
        self.dom_height
    }

    pub fn get_canvas_height(&self) -> f64 {
        self.dom_height * self.pixel_ratio
    }

    pub fn set_dom_height(&mut self, height: f64) -> bool {
        if self.dom_height == height {
            return false;
        }
        self.dom_height = height;
        self.color_zones_invalid = true;
        true
    }

    pub fn get_outer_height(&self) -> f64 {
        self.outer_height
    }

    pub fn set_outer_height(&mut self, outer_height: f64) -> bool {
        if self.outer_height == outer_height {
            return false;
        }
        self.outer_height = outer_height;
        self.color_zones_invalid = true;
        true
    }

    pub fn resolve_color_zones(&mut self) -> Vec<ColorZone> {
        let color_zones_invalid = self.color_zones_invalid;
        let line_height = self.line_height.floor();
        let total_height = self.get_canvas_height().floor();
        let outer_height = self.outer_height.floor();
        let height_ratio = total_height / outer_height;
        let half_minimum_height = (MINIMUM_HEIGHT * self.pixel_ratio / 2.0).floor();

        let mut all_color_zones: Vec<ColorZone> = Vec::new();
        for zone in self.zones.iter_mut() {
            if !color_zones_invalid {
                if let Some(color_zone) = zone.get_color_zones() {
                    all_color_zones.push(color_zone);
                    continue;
                }
            }

            let offset1 = (self.get_vertical_offset_for_line)(zone.start_line_number);
            let offset2 = if zone.height_in_lines == 0.0 {
                (self.get_vertical_offset_for_line)(zone.end_line_number) + line_height
            } else {
                offset1 + zone.height_in_lines * line_height
            };

            let y1 = (height_ratio * offset1).floor();
            let y2 = (height_ratio * offset2).floor();

            let mut ycenter = ((y1 + y2) / 2.0).floor();
            let mut half_height = y2 - ycenter;

            if half_height < half_minimum_height {
                half_height = half_minimum_height;
            }

            if ycenter - half_height < 0.0 {
                ycenter = half_height;
            }
            if ycenter + half_height > total_height {
                ycenter = total_height - half_height;
            }

            let color = &zone.color;
            // `!colorId` do JavaScript: ausente ou 0.
            let mut color_id = self.color_2_id.get(color).copied().unwrap_or(0);
            if color_id == 0 {
                self.last_assigned_id += 1;
                color_id = self.last_assigned_id;
                self.color_2_id.insert(color.clone(), color_id);
                let index = color_id as usize;
                if self.id_2_color.len() <= index {
                    self.id_2_color.resize(index + 1, String::new());
                }
                self.id_2_color[index] = color.clone();
            }
            let color_zone = ColorZone::new(
                to_int32(ycenter - half_height),
                to_int32(ycenter + half_height),
                color_id,
            );

            zone.set_color_zone(color_zone);
            all_color_zones.push(color_zone);
        }

        self.color_zones_invalid = false;

        all_color_zones.sort_by(ColorZone::compare);
        all_color_zones
    }
}
