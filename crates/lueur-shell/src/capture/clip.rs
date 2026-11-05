//! The pass that brings an HDR desktop down to the SDR white on the GPU,
//! before the mip chain shrinks it.
//!
//! The shrink averages blocks of pixels. Left to average the raw floats, a
//! highlight five times brighter than the SDR white would weigh five times
//! as much in its block, and lift a dark scene with a few bright lights well
//! above what the same picture shows in SDR. This pass draws the acquired
//! desktop into the top of the mip chain with every pixel already divided by
//! the SDR white and brought down by its brightest channel, the way
//! [`ToSdr`](super::hdr::ToSdr) does after the shrink, so the averages are
//! those of the picture as an SDR screen would show it.

use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::core::{PCSTR, s};

/// One triangle that covers the target, and a pixel shader that reads the
/// desktop pixel under it. NaNs become 0 and infinities the largest half
/// float, so neither spreads through the averages. A colour outside the sRGB gamut is
/// brought into it the way [`ToSdr`](super::hdr::ToSdr) does.
const SHADER: &str = r"
Texture2D<float4> desktop : register(t0);
cbuffer Clip : register(b0) { float scale; };

float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}

float4 ps(float4 position : SV_Position) : SV_Target {
    float3 raw = desktop.Load(int3(position.xy, 0)).rgb * scale;
    // min and max return the other operand for a NaN: this reads one as 0.
    float3 c = clamp(max(raw, 0) + min(raw, 0), -65504, 65504);
    float y = dot(c, float3(0.2126, 0.7152, 0.0722));
    float low = min(c.r, min(c.g, c.b));
    if (low < 0) {
        c = y > 0 ? y + (c - y) * (y / (y - low)) : float3(0, 0, 0);
    }
    float peak = max(c.r, max(c.g, c.b));
    return float4(peak > 1 ? c / peak : c, 1);
}
";

pub struct Clip {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    /// Holds one over the SDR white, padded to the 16 bytes a constant
    /// buffer is made of.
    constants: ID3D11Buffer,
    /// The white `constants` holds now, so it is only written when it moves.
    white: f32,
}

/// Label a Windows error with the step that failed.
fn err(what: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |e| format!("{what}: {e}")
}

/// Compile one entry point of [`SHADER`].
fn compile(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob, String> {
    let mut code = None;
    let mut errors = None;
    let compiled = unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            s!("clip.hlsl"),
            None,
            None,
            entry,
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = compiled {
        let detail = errors.map(|b| String::from_utf8_lossy(bytes(&b)).into_owned());
        return Err(format!("clip shader: {e} {}", detail.unwrap_or_default()));
    }
    code.ok_or_else(|| "clip shader: no code".into())
}

/// The bytes a blob holds.
fn bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: a blob is `GetBufferSize` bytes at `GetBufferPointer`, alive as
    // long as the blob it is borrowed from.
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    }
}

impl Clip {
    pub fn new(device: &ID3D11Device) -> Result<Self, String> {
        let vs = compile(s!("vs"), s!("vs_4_0"))?;
        let ps = compile(s!("ps"), s!("ps_4_0"))?;
        let mut vertex = None;
        let mut pixel = None;
        let mut constants = None;
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        unsafe {
            device
                .CreateVertexShader(bytes(&vs), None, Some(&mut vertex))
                .map_err(err("clip vertex shader"))?;
            device
                .CreatePixelShader(bytes(&ps), None, Some(&mut pixel))
                .map_err(err("clip pixel shader"))?;
            device
                .CreateBuffer(&desc, None, Some(&mut constants))
                .map_err(err("clip constants"))?;
        }
        Ok(Self {
            vertex: vertex.ok_or("no clip vertex shader")?,
            pixel: pixel.ok_or("no clip pixel shader")?,
            constants: constants.ok_or("no clip constants")?,
            // Not a white anyone has: the first draw writes the real one.
            white: 0.0,
        })
    }

    /// Queue drawing `desktop` into `target`, a texture of the same size,
    /// with every pixel brought down to `white` (in scRGB units).
    pub fn draw(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        desktop: &ID3D11Texture2D,
        target: &ID3D11RenderTargetView,
        white: f32,
    ) -> Result<(), String> {
        let mut size = D3D11_TEXTURE2D_DESC::default();
        unsafe { desktop.GetDesc(&mut size) };
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            device
                .CreateShaderResourceView(desktop, None, Some(&mut view))
                .map_err(err("desktop view"))?;
        }
        if white != self.white {
            let data = [1.0 / white, 0.0, 0.0, 0.0f32];
            unsafe {
                context.UpdateSubresource(&self.constants, 0, None, data.as_ptr().cast(), 0, 0);
            }
            self.white = white;
        }
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: size.Width as f32,
            Height: size.Height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[view]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            context.RSSetViewports(Some(&[viewport]));
            context.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            context.Draw(3, 0);
            // Unbound again, so the mip chain can be built from the target
            // and the desktop image handed back without them still attached.
            context.OMSetRenderTargets(None, None);
            context.PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::hdr::ToSdr;
    use super::*;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
        D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
        D3D11_USAGE_STAGING, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
    };

    /// The halves the test pixels are made of: SDR greys, highlights above
    /// the white, a colour outside sRGB, an infinity and a NaN.
    const PIXELS: [[u16; 4]; 8] = [
        [0x0000, 0x0000, 0x0000, 0x3c00], // black
        [0x3c00, 0x3c00, 0x3c00, 0x3c00], // 1.0: half the white of 2.0
        [0x4000, 0x4000, 0x4000, 0x3c00], // 2.0: the white
        [0x4800, 0x4400, 0x0000, 0x3c00], // 8, 4, 0: an orange four times too bright
        [0xb400, 0x4000, 0x0000, 0x3c00], // -0.25, 2, 0: green outside sRGB
        [0x4900, 0x3800, 0x3400, 0x3c00], // 10, 0.5, 0.25
        [0x7c00, 0x3c00, 0x0000, 0x3c00], // an infinity
        [0x7e00, 0x3c00, 0x3c00, 0x3c00], // a NaN
    ];
    const WHITE: f32 = 2.0;

    fn row_bytes(pixels: &[[u16; 4]]) -> Vec<u8> {
        pixels
            .iter()
            .flatten()
            .flat_map(|h| h.to_le_bytes())
            .collect()
    }

    /// Run the pass on WARP, the software rasterizer every Windows has, so
    /// the test needs no graphics card.
    fn gpu_pass(pixels: &[[u16; 4]], white: f32) -> Vec<u8> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                Default::default(),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .expect("WARP device");
        let device: ID3D11Device = device.expect("device");
        let context = context.expect("context");

        let width = pixels.len() as u32;
        let base = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = row_bytes(pixels);
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            SysMemPitch: data.len() as u32,
            SysMemSlicePitch: 0,
        };
        let target_desc = D3D11_TEXTURE2D_DESC {
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..base
        };
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..base
        };
        let (mut desktop, mut target, mut staging, mut view) = (None, None, None, None);
        unsafe {
            device
                .CreateTexture2D(&base, Some(&initial), Some(&mut desktop))
                .expect("desktop");
            device
                .CreateTexture2D(&target_desc, None, Some(&mut target))
                .expect("target");
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .expect("staging");
        }
        let (desktop, target, staging) = (
            desktop.expect("desktop"),
            target.expect("target"),
            staging.expect("staging"),
        );
        unsafe {
            device
                .CreateRenderTargetView(&target, None, Some(&mut view))
                .expect("view");
        }

        let mut clip = Clip::new(&device).expect("clip");
        clip.draw(&device, &context, &desktop, &view.expect("view"), white)
            .expect("draw");
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context.CopyResource(&staging, &target);
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .expect("map");
            let out = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), data.len()).to_vec();
            context.Unmap(&staging, 0);
            out
        }
    }

    #[test]
    fn the_gpu_pass_gives_the_colours_the_cpu_does() {
        // Clipped on the GPU then encoded at a white of 1.0, or encoded
        // straight from the desktop at the real white: the same bytes.
        let mut cpu = Vec::new();
        ToSdr::new(WHITE).convert_row(&row_bytes(&PIXELS), &mut cpu);
        let mut gpu = Vec::new();
        ToSdr::new(1.0).convert_row(&gpu_pass(&PIXELS, WHITE), &mut gpu);
        let close = cpu.iter().zip(&gpu).all(|(a, b)| a.abs_diff(*b) <= 1);
        assert!(close, "cpu {cpu:?}\ngpu {gpu:?}");
    }

    #[test]
    fn nothing_leaves_the_pass_above_the_white() {
        let out = gpu_pass(&PIXELS, WHITE);
        for px in out.chunks_exact(8) {
            for c in px[..6].chunks_exact(2) {
                let bits = u16::from_le_bytes([c[0], c[1]]);
                // Below or at 1.0 (0x3c00), and not negative.
                assert!(bits <= 0x3c00, "{bits:#06x}");
            }
        }
    }
}
