//! Windows capture through DXGI Desktop Duplication, shrunk on the GPU.
//!
//! `xcap`'s one-shot `capture_image` goes through GDI on Windows: a full-screen
//! `BitBlt`, a full-resolution copy into system memory and a per-pixel channel
//! swap, every single frame. That costs tens of milliseconds at 4K and cannot
//! see exclusive-fullscreen games. The ambilight only needs a handful of
//! averages, so this source keeps one duplication session open, lets the GPU
//! box-filter the desktop down a mip chain, and reads back a frame a few hundred
//! pixels wide, a few hundred kilobytes instead of tens of megabytes.
//!
//! The mip chain is built in an sRGB view of the desktop, so the GPU averages
//! in linear light; that keeps the averages as vivid as the engine's own
//! squared-value accumulation would.

use lueur_core::engine::{Frame, PixelFormat};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
    D3D11_FORMAT_SUPPORT_MIP_AUTOGEN, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RESOURCE_MISC_GENERATE_MIPS, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

/// The readback is the smallest mip level at least this wide: plenty for zone
/// averages, and small enough that copying it out is negligible.
const MIN_READBACK_WIDTH: u32 = 256;

/// The mip level whose width is the smallest one still `>= MIN_READBACK_WIDTH`.
fn readback_level(width: u32) -> u32 {
    let mut level = 0;
    while (width >> (level + 1)) >= MIN_READBACK_WIDTH {
        level += 1;
    }
    level
}

/// Label a Windows error with the step that failed.
fn err(what: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |e| format!("{what}: {e}")
}

/// A duplicated monitor. Reopens itself after the session is lost (display
/// mode change, a game switching to exclusive fullscreen, the UAC desktop).
pub struct DxgiSource {
    /// `xcap`'s monitor id: the `HMONITOR`, truncated to 32 bits.
    monitor: u32,
    session: Option<Session>,
    frame: Option<Frame>,
}

impl DxgiSource {
    /// Open a duplication session for the monitor `xcap` calls `monitor`.
    pub fn open(monitor: u32) -> Result<Self, String> {
        let session = Session::open(monitor)?;
        Ok(Self {
            monitor,
            session: Some(session),
            frame: None,
        })
    }

    /// Return the newest desktop image, waiting up to `timeout` for one. When
    /// nothing was presented in that time the previous image is still current
    /// and is returned again.
    pub fn grab(&mut self, timeout: Duration) -> Result<&Frame, String> {
        let session = match self.session.as_mut() {
            Some(s) => s,
            None => self.session.insert(Session::open(self.monitor)?),
        };
        if let Err(e) = session.next(timeout, &mut self.frame) {
            self.session = None;
            return Err(e);
        }
        self.frame
            .as_ref()
            .ok_or_else(|| "no desktop image yet".to_string())
    }
}

/// GPU objects sized for one desktop resolution.
struct Target {
    source_width: u32,
    source_height: u32,
    mips: ID3D11Resource,
    view: ID3D11ShaderResourceView,
    staging: ID3D11Resource,
    level: u32,
    width: u32,
    height: u32,
}

struct Session {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    format: DXGI_FORMAT,
    target: Option<Target>,
}

impl Session {
    fn open(monitor: u32) -> Result<Self, String> {
        let factory: IDXGIFactory1 =
            unsafe { CreateDXGIFactory1() }.map_err(err("DXGI factory"))?;

        // The duplication has to come from a device on the adapter that drives
        // the output, which matters on laptops with two GPUs.
        let mut a = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
            a += 1;
            let mut o = 0;
            while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
                o += 1;
                let desc = unsafe { output.GetDesc() }.map_err(err("output description"))?;
                if desc.Monitor.0 as usize as u32 == monitor {
                    let output: IDXGIOutput1 = output.cast().map_err(err("IDXGIOutput1"))?;
                    return Self::on_output(&adapter, &output);
                }
            }
        }
        Err("monitor not found on any graphics adapter".into())
    }

    fn on_output(adapter: &IDXGIAdapter1, output: &IDXGIOutput1) -> Result<Self, String> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(err("D3D11 device"))?;
        let device: ID3D11Device = device.ok_or("no D3D11 device")?;
        let context = context.ok_or("no D3D11 context")?;

        let duplication =
            unsafe { output.DuplicateOutput(&device) }.map_err(err("desktop duplication"))?;

        // A rotated desktop arrives in panel orientation. GDI hands it back the
        // way the user sees it, so leave those setups to the fallback.
        let rotation = unsafe { duplication.GetDesc() }.Rotation;
        if rotation != DXGI_MODE_ROTATION_IDENTITY && rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
            return Err("rotated display".into());
        }

        // Average in linear light where the hardware can build sRGB mips.
        let srgb = unsafe { device.CheckFormatSupport(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB) }
            .map(|bits| bits & D3D11_FORMAT_SUPPORT_MIP_AUTOGEN.0 as u32 != 0)
            .unwrap_or(false);
        let format = if srgb {
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        } else {
            DXGI_FORMAT_B8G8R8A8_UNORM
        };

        Ok(Self {
            device,
            context,
            duplication,
            format,
            target: None,
        })
    }

    /// Wait for a presented frame (up to `timeout`) and shrink it into `frame`.
    /// Leaves `frame` untouched when the desktop did not change.
    fn next(&mut self, timeout: Duration, frame: &mut Option<Frame>) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let wait = left.as_millis().min(u32::MAX as u128) as u32;
            match unsafe {
                self.duplication
                    .AcquireNextFrame(wait, &mut info, &mut resource)
            } {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
                Err(e) => return Err(format!("next desktop frame: {e}")),
            }

            // A zero present time means only the mouse pointer moved.
            let copied = match resource {
                Some(r) if info.LastPresentTime != 0 => self.copy(&r).map(|()| true),
                _ => Ok(false),
            };
            // Handed back as soon as the copy is queued: the shrink and the
            // readback below wait on the GPU, which a game can keep busy, and
            // the desktop image should not stay held for that long.
            unsafe { self.duplication.ReleaseFrame() }
                .map_err(|e| format!("release desktop frame: {e}"))?;
            if copied? {
                return self.shrink(frame);
            }
            if left.is_zero() {
                return Ok(());
            }
        }
    }

    /// Queue a copy of the acquired desktop image into the mip chain's top
    /// level.
    fn copy(&mut self, resource: &IDXGIResource) -> Result<(), String> {
        let texture: ID3D11Texture2D = resource.cast().map_err(err("desktop texture"))?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let stale = self
            .target
            .as_ref()
            .is_none_or(|t| t.source_width != desc.Width || t.source_height != desc.Height);
        if stale {
            self.target = Some(self.make_target(desc.Width, desc.Height)?);
        }
        let Some(t) = self.target.as_ref() else {
            return Err("no capture target".into());
        };
        let source: ID3D11Resource = texture.cast().map_err(err("desktop resource"))?;
        unsafe {
            self.context
                .CopySubresourceRegion(Some(&t.mips), 0, 0, 0, 0, Some(&source), 0, None);
        }
        Ok(())
    }

    /// Shrink the copied desktop down the mip chain and read the small level
    /// back into `frame`.
    fn shrink(&mut self, frame: &mut Option<Frame>) -> Result<(), String> {
        let Some(t) = self.target.as_ref() else {
            return Err("no capture target".into());
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context.GenerateMips(Some(&t.view));
            self.context.CopySubresourceRegion(
                Some(&t.staging),
                0,
                0,
                0,
                0,
                Some(&t.mips),
                t.level,
                None,
            );
            self.context
                .Map(Some(&t.staging), 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(err("map readback"))?;
        }

        let row = t.width as usize * 4;
        let out = frame.get_or_insert_with(|| Frame::new(0, 0, PixelFormat::Bgra, Vec::new()));
        out.width = t.width;
        out.height = t.height;
        out.format = PixelFormat::Bgra;
        out.data.clear();
        out.data.reserve(row * t.height as usize);
        for y in 0..t.height as usize {
            // SAFETY: the mapped staging texture is `height` rows of `RowPitch`
            // bytes, each at least `width * 4` long, and stays mapped until the
            // `Unmap` below.
            let line = unsafe {
                std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    row,
                )
            };
            out.data.extend_from_slice(line);
        }
        unsafe { self.context.Unmap(Some(&t.staging), 0) };
        Ok(())
    }

    fn make_target(&self, width: u32, height: u32) -> Result<Target, String> {
        let level = readback_level(width);
        let base = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: level + 1,
            ArraySize: 1,
            Format: self.format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
        };
        let small_width = (width >> level).max(1);
        let small_height = (height >> level).max(1);
        let readback = D3D11_TEXTURE2D_DESC {
            Width: small_width,
            Height: small_height,
            MipLevels: 1,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..base
        };

        let mut mips = None;
        let mut staging = None;
        let mut view = None;
        unsafe {
            self.device
                .CreateTexture2D(&base, None, Some(&mut mips))
                .map_err(err("mip texture"))?;
            self.device
                .CreateTexture2D(&readback, None, Some(&mut staging))
                .map_err(err("readback texture"))?;
        }
        let mips: ID3D11Texture2D = mips.ok_or("no mip texture")?;
        let staging: ID3D11Texture2D = staging.ok_or("no readback texture")?;
        unsafe {
            self.device
                .CreateShaderResourceView(&mips, None, Some(&mut view))
                .map_err(err("mip view"))?;
        }

        Ok(Target {
            source_width: width,
            source_height: height,
            mips: mips.cast().map_err(err("mip resource"))?,
            view: view.ok_or("no mip view")?,
            staging: staging.cast().map_err(err("readback resource"))?,
            level,
            width: small_width,
            height: small_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Time the shrink-and-read path on the real primary display, per
    /// presented frame. Needs a desktop session and something moving on
    /// screen, so it only runs on request:
    /// `cargo test -p lueur capture_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dxgi_capture_cost() {
        let id = xcap::Monitor::all().expect("monitors")[0].id().expect("id");
        let mut session = Session::open(id).expect("duplication");
        let mut frame = None;
        let (mut reads, mut spent) = (0u32, Duration::ZERO);
        let started = Instant::now();
        while reads < 60 && started.elapsed() < Duration::from_secs(20) {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            if unsafe {
                session
                    .duplication
                    .AcquireNextFrame(500, &mut info, &mut resource)
            }
            .is_err()
            {
                continue;
            }
            let t = Instant::now();
            let copied = resource
                .filter(|_| info.LastPresentTime != 0)
                .map(|r| session.copy(&r).expect("copy"))
                .is_some();
            unsafe { session.duplication.ReleaseFrame() }.expect("release");
            if copied {
                session.shrink(&mut frame).expect("shrink");
                spent += t.elapsed();
                reads += 1;
            }
        }
        let f = frame.expect("no frame presented");
        let whole = lueur_core::geometry::SampleZone::new(0.0, 0.0, 1.0, 1.0);
        println!("dxgi: screen average {:?}", f.average_zone(&whole, 1));
        println!(
            "dxgi: {}x{} readback, {:.3} ms per frame over {reads} frames (srgb mips: {})",
            f.width,
            f.height,
            spent.as_secs_f64() * 1000.0 / reads.max(1) as f64,
            session.format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        );
    }

    #[test]
    fn readback_stays_a_few_hundred_pixels_wide() {
        assert_eq!(1920 >> readback_level(1920), 480);
        assert_eq!(2560 >> readback_level(2560), 320);
        assert_eq!(3840 >> readback_level(3840), 480);
        assert_eq!(5120 >> readback_level(5120), 320);
        // Never shrinks a small display below the floor.
        assert_eq!(readback_level(300), 0);
    }
}
