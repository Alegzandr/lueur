//! Bringing an HDR desktop back to the SDR colours the bulbs are given.
//!
//! With HDR on, Windows composes the desktop in scRGB: linear light in 16-bit
//! floats, where 1.0 is 80 nits and the white of ordinary SDR content sits
//! wherever the user put the "SDR content brightness" slider, usually two or
//! three times higher. The 8-bit image the older duplication call hands back
//! does not take that white into account, which is why an HDR desktop looks
//! washed out in screen sharing. [`ToSdr`] reads the floats instead, divides
//! by the SDR white so a white page is white again, and encodes the result as
//! sRGB, the same bytes an SDR desktop would have given.

/// The SDR white, in scRGB units, when Windows does not say: 200 nits, a
/// common setting of the slider.
pub const DEFAULT_WHITE: f32 = 200.0 / 80.0;

/// Entries in the linear-to-sRGB table: fine enough that neighbouring dark
/// values still land on the right byte.
const ENCODE_STEPS: usize = 1 << 14;

/// Turns rows of `R16G16B16A16_FLOAT` scRGB pixels into BGRA sRGB bytes.
pub struct ToSdr {
    /// One over the SDR white, so a pixel at that white becomes 1.0.
    scale: f32,
    /// Linear light in `[0, 1]`, in `ENCODE_STEPS` steps, to its sRGB byte.
    encode: Vec<u8>,
}

impl ToSdr {
    pub fn new(white: f32) -> Self {
        let last = (ENCODE_STEPS - 1) as f32;
        let encode = (0..ENCODE_STEPS)
            .map(|i| (srgb(i as f32 / last) * 255.0).round() as u8)
            .collect();
        let mut to_sdr = Self { scale: 1.0, encode };
        to_sdr.set_white(white);
        to_sdr
    }

    /// Follow a change of the SDR white, in scRGB units. Nonsense values
    /// (zero, negative, not a number) keep the white there was.
    pub fn set_white(&mut self, white: f32) {
        if white.is_finite() && white > 0.0 {
            self.scale = 1.0 / white;
        }
    }

    /// Convert one row of 8-byte pixels and append it to `out` as BGRA.
    ///
    /// Colours outside the sRGB gamut have negative channels in scRGB; those
    /// are cut to zero. Light brighter than the SDR white is brought down by
    /// its brightest channel rather than clipped channel by channel, so a
    /// bright orange stays orange instead of turning yellow.
    pub fn convert_row(&self, row: &[u8], out: &mut Vec<u8>) {
        for px in row.chunks_exact(8) {
            let channel = |i: usize| {
                let v = half(u16::from_le_bytes([px[i], px[i + 1]])) * self.scale;
                // `max` also turns a NaN into zero.
                v.max(0.0)
            };
            let (r, g, b) = (channel(0), channel(2), channel(4));
            let peak = r.max(g).max(b);
            let k = if peak > 1.0 { 1.0 / peak } else { 1.0 };
            out.extend_from_slice(&[
                self.encode(b * k),
                self.encode(g * k),
                self.encode(r * k),
                255,
            ]);
        }
    }

    fn encode(&self, linear: f32) -> u8 {
        // A float-to-integer cast saturates, and turns a NaN into zero.
        let i = (linear * (ENCODE_STEPS - 1) as f32 + 0.5) as usize;
        self.encode[i.min(ENCODE_STEPS - 1)]
    }
}

/// The sRGB transfer function, linear light to the encoded value.
fn srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// An IEEE 754 half-precision float, from its bits.
fn half(bits: u16) -> f32 {
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let magnitude = match exponent {
        // Subnormal: the mantissa counts steps of 2^-24.
        0 => mantissa as f32 / 16_777_216.0,
        31 if mantissa == 0 => f32::INFINITY,
        31 => f32::NAN,
        // Rebias the exponent from 15 to 127 and widen the mantissa.
        _ => f32::from_bits(((exponent + 112) << 23) | (mantissa << 13)),
    };
    if bits & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bits of a half that holds `value`, for the values these tests use
    /// (all exact in half precision).
    fn bits(value: f32) -> u16 {
        if value == 0.0 {
            return 0;
        }
        let f = value.to_bits();
        let sign = ((f >> 16) & 0x8000) as u16;
        let exponent = ((f >> 23) & 0xff) as u16 - 112;
        let mantissa = ((f >> 13) & 0x3ff) as u16;
        sign | (exponent << 10) | mantissa
    }

    fn pixel(r: f32, g: f32, b: f32) -> Vec<u8> {
        [r, g, b, 1.0]
            .iter()
            .flat_map(|&c| bits(c).to_le_bytes())
            .collect()
    }

    fn convert(to_sdr: &ToSdr, r: f32, g: f32, b: f32) -> [u8; 4] {
        let mut out = Vec::new();
        to_sdr.convert_row(&pixel(r, g, b), &mut out);
        out.try_into().expect("one BGRA pixel")
    }

    #[test]
    fn reads_half_floats() {
        assert_eq!(half(0x3c00), 1.0);
        assert_eq!(half(0xc000), -2.0);
        assert_eq!(half(0x3800), 0.5);
        assert_eq!(half(0x7bff), 65504.0);
        assert_eq!(half(0x0001), 2f32.powi(-24));
        assert_eq!(half(0x7c00), f32::INFINITY);
        assert!(half(0x7e00).is_nan());
        assert_eq!(half(0x8000), 0.0);
    }

    #[test]
    fn sdr_white_is_white_and_black_is_black() {
        let to_sdr = ToSdr::new(2.5);
        assert_eq!(convert(&to_sdr, 2.5, 2.5, 2.5), [255, 255, 255, 255]);
        assert_eq!(convert(&to_sdr, 0.0, 0.0, 0.0), [0, 0, 0, 255]);
    }

    #[test]
    fn mid_grey_is_encoded_as_srgb() {
        // Half the white in linear light is 188 in sRGB, not 128: the
        // washed-out look is exactly this step going missing.
        let to_sdr = ToSdr::new(2.0);
        assert_eq!(convert(&to_sdr, 1.0, 1.0, 1.0), [188, 188, 188, 255]);
    }

    #[test]
    fn bright_highlights_keep_their_hue() {
        // Four and two times the white: clipping each channel would give
        // yellow (255, 255, 0); scaling by the peak keeps the orange.
        let to_sdr = ToSdr::new(1.0);
        assert_eq!(convert(&to_sdr, 4.0, 2.0, 0.0), [0, 188, 255, 255]);
    }

    #[test]
    fn colours_outside_srgb_are_cut_to_the_gamut() {
        let to_sdr = ToSdr::new(1.0);
        assert_eq!(convert(&to_sdr, -0.25, 1.0, 0.0), [0, 255, 0, 255]);
    }

    #[test]
    fn a_nonsense_white_is_ignored() {
        let mut to_sdr = ToSdr::new(3.0);
        to_sdr.set_white(0.0);
        to_sdr.set_white(f32::NAN);
        assert_eq!(convert(&to_sdr, 3.0, 3.0, 3.0), [255, 255, 255, 255]);
        assert_eq!(convert(&to_sdr, 1.5, 1.5, 1.5), [188, 188, 188, 255]);
    }
}
