/// sRGB-encoded colour with straight alpha, 0..1. Blending happens in sRGB space (like CSS), so
/// the rare.market tokens and the web launcher's gradients and opacities look the same.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    /// The same colour at `a` times its alpha (Tailwind's `/60`).
    pub const fn alpha(self, a: f32) -> Color {
        Color { a: self.a * a, ..self }
    }

    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        // Interpolate premultiplied, like CSS transitions and gradients.
        let (pa, pb) = (a.premul(), b.premul());
        let p = [0, 1, 2, 3].map(|i| pa[i] + (pb[i] - pa[i]) * t);
        if p[3] <= 1e-6 {
            return Color::TRANSPARENT;
        }
        Color { r: p[0] / p[3], g: p[1] / p[3], b: p[2] / p[3], a: p[3] }
    }

    pub fn premul(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
    }

    pub fn arr(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn to_wgpu(self) -> wgpu::Color {
        wgpu::Color { r: self.r as f64, g: self.g as f64, b: self.b as f64, a: self.a as f64 }
    }
}
