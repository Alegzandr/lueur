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
//!
//! On a display with HDR on, the desktop is read in the 16-bit floats Windows
//! composes it in, which are already linear. A first pass brings each pixel
//! down to the SDR white (see [`clip`](super::clip)), the floats are shrunk,
//! and the small image is encoded as sRGB (see [`hdr`](super::hdr)).

use super::clip::Clip;
use super::hdr::{self, ToSdr};
use lueur_core::engine::{Frame, PixelFormat};
use std::time::{Duration, Instant};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
    D3D11_FORMAT_SUPPORT_MIP_AUTOGEN, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RESOURCE_MISC_GENERATE_MIPS, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11RenderTargetView, ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_MODE_ROTATION_IDENTITY,
    DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput1, IDXGIOutput5, IDXGIOutput6, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::core::Interface;

/// How long a lost session waits between attempts to open again. A display
/// mode change refuses duplication for a few seconds, and each attempt
/// creates a D3D11 device while the driver is busy switching.
const REOPEN_EVERY: Duration = Duration::from_millis(250);

/// The readback is the smallest mip level at least this wide: plenty for zone
/// averages, and small enough that copying it out is negligible.
const MIN_READBACK_WIDTH: u32 = 256;

/// How often a session makes sure it still matches the display, and an HDR
/// one asks Windows for the SDR white again: moving the brightness slider
/// does not always interrupt the duplication, so nothing else would tell.
const CHECK_EVERY: Duration = Duration::from_secs(2);

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
    /// When a lost session may next try to open again.
    reopen_at: Instant,
    /// Whether the display is in HDR and read in its floats. Set when a
    /// session opens and kept while it is lost, so a lock screen or a mode
    /// change does not make the window say otherwise for a moment.
    hdr: bool,
    /// How the display was read when the logs last said so: whether it was
    /// in HDR, and whether its floats were read.
    told: Option<(bool, bool)>,
}

impl DxgiSource {
    /// Open a duplication session for the monitor `xcap` calls `monitor`.
    pub fn open(monitor: u32) -> Result<Self, String> {
        let session = Session::open(monitor)?;
        let mut source = Self {
            monitor,
            session: Some(session),
            frame: None,
            reopen_at: Instant::now(),
            hdr: false,
            told: None,
        };
        source.opened();
        Ok(source)
    }

    /// Take in a session that just opened: what the window shows, and what
    /// the logs say when it differs from the last session.
    fn opened(&mut self) {
        let Some(s) = self.session.as_ref() else {
            return;
        };
        let read = (s.hdr, s.refused.is_none());
        self.hdr = read == (true, true);
        if self.told == Some(read) {
            return;
        }
        match (read, s.refused.as_deref()) {
            ((true, true), _) => log::info!(
                "HDR display, read in 16-bit floats, SDR white at {:.0} nits{}; highlights \
                 brought down {}",
                s.white * 80.0,
                if s.white_known { "" } else { " (assumed)" },
                if s.clip.is_some() {
                    "before the shrink"
                } else {
                    "after the shrink"
                }
            ),
            ((true, false), refused) => log::warn!(
                "HDR display read in 8 bits, colours may look washed out: {}",
                refused.unwrap_or_default()
            ),
            ((false, _), _) if self.told.is_some_and(|(hdr, _)| hdr) => {
                log::info!("display back in SDR");
            }
            _ => {}
        }
        self.told = Some(read);
    }

    /// Return the newest desktop image, waiting up to `timeout` for one. When
    /// nothing was presented in that time the previous image is still current
    /// and is returned again.
    pub fn grab(&mut self, timeout: Duration) -> Result<&Frame, String> {
        let session = match self.session.as_mut() {
            Some(s) => s,
            None if Instant::now() < self.reopen_at => {
                return Err("desktop duplication lost, waiting to open it again".into());
            }
            None => {
                self.reopen_at = Instant::now() + REOPEN_EVERY;
                self.session = Some(Session::open(self.monitor)?);
                self.opened();
                match self.session.as_mut() {
                    Some(s) => s,
                    None => return Err("no capture session".into()),
                }
            }
        };
        if let Err(e) = session.next(timeout, &mut self.frame) {
            self.session = None;
            self.reopen_at = Instant::now() + REOPEN_EVERY;
            return Err(e);
        }
        self.frame
            .as_ref()
            .ok_or_else(|| "no desktop image yet".to_string())
    }

    /// The image the last successful grab returned.
    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }

    /// Whether the display is in HDR, read in its floats and brought back to
    /// SDR colours.
    pub fn hdr(&self) -> bool {
        self.hdr
    }
}

/// GPU objects sized for one desktop resolution and format.
struct Target {
    source_width: u32,
    source_height: u32,
    /// The mip chain's format: 8-bit BGRA, or 16-bit floats on an HDR desktop.
    format: DXGI_FORMAT,
    mips: ID3D11Resource,
    view: ID3D11ShaderResourceView,
    /// The top of the mip chain as a render target, for the clip pass: only
    /// on an HDR desktop whose session has one.
    top: Option<ID3D11RenderTargetView>,
    staging: ID3D11Resource,
    level: u32,
    width: u32,
    height: u32,
}

struct Session {
    /// The factory the output was found through, which says when its view of
    /// the displays is out of date.
    factory: IDXGIFactory1,
    /// Whether the display was in HDR when the session opened.
    hdr: bool,
    /// Why an HDR display is read in 8 bits rather than its floats.
    refused: Option<String>,
    /// The SDR white, in scRGB units, and whether Windows said it or it is
    /// assumed.
    white: f32,
    white_known: bool,
    /// The pass that brings an HDR desktop down to the SDR white before the
    /// shrink. Without it (no shader compiler), the shrunk image is.
    clip: Option<Clip>,
    /// Whether the image being shrunk went through `clip`.
    clipped: bool,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    /// The mip chain's format for an 8-bit desktop.
    format: DXGI_FORMAT,
    target: Option<Target>,
    /// The GDI name of the display, which is how Windows is asked for its
    /// SDR white.
    display: [u16; 32],
    to_sdr: ToSdr,
    /// When the session next checks it still matches the display.
    check_at: Instant,
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
                    return Self::on_output(&factory, &adapter, output);
                }
            }
        }
        Err("monitor not found on any graphics adapter".into())
    }

    fn on_output(
        factory: &IDXGIFactory1,
        adapter: &IDXGIAdapter1,
        output: IDXGIOutput1,
    ) -> Result<Self, String> {
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

        let name = unsafe { output.GetDesc() }
            .map_err(err("output description"))?
            .DeviceName;
        let hdr_on = is_hdr(&output);
        let white = if hdr_on { sdr_white(&name) } else { None };
        // Any card that drives an HDR display builds float mips; asked all
        // the same, since the shrink relies on it.
        let (duplication, refused) = if hdr_on
            && can_shrink(&device, DXGI_FORMAT_R16G16B16A16_FLOAT)
        {
            duplicate_hdr(&output, &device)?
        } else {
            let duplication =
                unsafe { output.DuplicateOutput(&device) }.map_err(err("desktop duplication"))?;
            let refused = hdr_on.then(|| "the graphics card cannot shrink 16-bit floats".into());
            (duplication, refused)
        };
        let clip = if hdr_on && refused.is_none() {
            Clip::new(&device)
                .inspect_err(|e| log::warn!("HDR highlights brought down after the shrink: {e}"))
                .ok()
        } else {
            None
        };

        // A rotated desktop arrives in panel orientation. GDI hands it back the
        // way the user sees it, so leave those setups to the fallback.
        let rotation = unsafe { duplication.GetDesc() }.Rotation;
        if rotation != DXGI_MODE_ROTATION_IDENTITY && rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
            return Err("rotated display".into());
        }

        // Average in linear light where the hardware can build sRGB mips.
        let format = if can_shrink(&device, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB) {
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        } else {
            DXGI_FORMAT_B8G8R8A8_UNORM
        };

        Ok(Self {
            factory: factory.clone(),
            hdr: hdr_on,
            refused,
            white: white.unwrap_or(hdr::DEFAULT_WHITE),
            white_known: white.is_some(),
            clip,
            clipped: false,
            device,
            context,
            duplication,
            format,
            target: None,
            display: name,
            to_sdr: ToSdr::new(white.unwrap_or(hdr::DEFAULT_WHITE)),
            check_at: Instant::now() + CHECK_EVERY,
        })
    }

    /// Make sure the session still reads the display as it is set up now.
    /// Turning HDR on or off normally interrupts the duplication, but a
    /// session opened while Windows was still switching can be left reading
    /// the desktop the way it was, washed out. The factory then says its
    /// view of the displays is out of date (an output found through it keeps
    /// the colour space it had), and the session is opened again through a
    /// new one.
    fn check(&mut self) -> Result<(), String> {
        if !unsafe { self.factory.IsCurrent() }.as_bool() {
            return Err("display settings changed".into());
        }
        if self.hdr
            && let Some(white) = sdr_white(&self.display)
        {
            self.white = white;
            self.white_known = true;
        }
        Ok(())
    }

    /// Wait for a presented frame (up to `timeout`) and shrink it into `frame`.
    /// Leaves `frame` untouched when the desktop did not change.
    fn next(&mut self, timeout: Duration, frame: &mut Option<Frame>) -> Result<(), String> {
        if Instant::now() >= self.check_at {
            self.check_at = Instant::now() + CHECK_EVERY;
            self.check()?;
        }
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
    /// level: through the clip pass on an HDR desktop, as it is otherwise.
    fn copy(&mut self, resource: &IDXGIResource) -> Result<(), String> {
        let texture: ID3D11Texture2D = resource.cast().map_err(err("desktop texture"))?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        // An HDR desktop arrives in 16-bit floats, which are already linear
        // and stay floats down the mip chain; anything else is 8-bit BGRA.
        let format = if desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT {
            DXGI_FORMAT_R16G16B16A16_FLOAT
        } else {
            self.format
        };
        let stale = self.target.as_ref().is_none_or(|t| {
            t.source_width != desc.Width || t.source_height != desc.Height || t.format != format
        });
        if stale {
            self.target = Some(self.make_target(desc.Width, desc.Height, format)?);
        }
        let Some(t) = self.target.as_ref() else {
            return Err("no capture target".into());
        };
        // The pass reads the desktop image as a shader resource, which
        // duplication allows; checked all the same, the copy works without.
        let readable = desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0;
        self.clipped = match (self.clip.as_mut(), t.top.as_ref()) {
            (Some(clip), Some(top)) if readable => {
                clip.draw(&self.device, &self.context, &texture, top, self.white)?;
                true
            }
            _ => {
                let source: ID3D11Resource = texture.cast().map_err(err("desktop resource"))?;
                unsafe {
                    self.context.CopySubresourceRegion(
                        Some(&t.mips),
                        0,
                        0,
                        0,
                        0,
                        Some(&source),
                        0,
                        None,
                    );
                }
                false
            }
        };
        Ok(())
    }

    /// Shrink the copied desktop down the mip chain and read the small level
    /// back into `frame`.
    fn shrink(&mut self, frame: &mut Option<Frame>) -> Result<(), String> {
        let Some(t) = self.target.as_ref() else {
            return Err("no capture target".into());
        };
        let floats = t.format == DXGI_FORMAT_R16G16B16A16_FLOAT;
        // A clipped image is already at the SDR white; one that was copied
        // as it is still has to be divided by it.
        self.to_sdr
            .set_white(if self.clipped { 1.0 } else { self.white });
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

        let pixel = if floats { 8 } else { 4 };
        let row = t.width as usize * pixel;
        let out = frame.get_or_insert_with(|| Frame::new(0, 0, PixelFormat::Bgra, Vec::new()));
        out.width = t.width;
        out.height = t.height;
        out.format = PixelFormat::Bgra;
        out.data.clear();
        out.data.reserve(t.width as usize * 4 * t.height as usize);
        for y in 0..t.height as usize {
            // SAFETY: the mapped staging texture is `height` rows of `RowPitch`
            // bytes, each at least `width * pixel` long, and stays mapped until
            // the `Unmap` below.
            let line = unsafe {
                std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    row,
                )
            };
            if floats {
                self.to_sdr.convert_row(line, &mut out.data);
            } else {
                out.data.extend_from_slice(line);
            }
        }
        unsafe { self.context.Unmap(Some(&t.staging), 0) };
        Ok(())
    }

    fn make_target(&self, width: u32, height: u32, format: DXGI_FORMAT) -> Result<Target, String> {
        let level = readback_level(width);
        let base = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: level + 1,
            ArraySize: 1,
            Format: format,
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
        // A render target view with no description is the top mip level.
        let mut top = None;
        if format == DXGI_FORMAT_R16G16B16A16_FLOAT && self.clip.is_some() {
            unsafe {
                self.device
                    .CreateRenderTargetView(&mips, None, Some(&mut top))
                    .map_err(err("mip render target"))?;
            }
        }

        Ok(Target {
            source_width: width,
            source_height: height,
            format,
            mips: mips.cast().map_err(err("mip resource"))?,
            view: view.ok_or("no mip view")?,
            top,
            staging: staging.cast().map_err(err("readback resource"))?,
            level,
            width: small_width,
            height: small_height,
        })
    }
}

/// Whether `device` can build the mip chain in `format`.
fn can_shrink(device: &ID3D11Device, format: DXGI_FORMAT) -> bool {
    unsafe { device.CheckFormatSupport(format) }
        .is_ok_and(|bits| bits & D3D11_FORMAT_SUPPORT_MIP_AUTOGEN.0 as u32 != 0)
}

/// Whether HDR is on for this output: Windows then drives it in the BT.2100
/// PQ colour space. Before Windows 10 1803 there is no `IDXGIOutput6`, and no
/// way to ask, so the output is taken as SDR.
fn is_hdr(output: &IDXGIOutput1) -> bool {
    output
        .cast::<IDXGIOutput6>()
        .and_then(|o| unsafe { o.GetDesc1() })
        .is_ok_and(|d| d.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020)
}

/// Duplicate an HDR output in the 16-bit floats it is composed in. Where
/// Windows refuses, the 8-bit image still works, only washed out, and the
/// reason comes with it.
fn duplicate_hdr(
    output: &IDXGIOutput1,
    device: &ID3D11Device,
) -> Result<(IDXGIOutputDuplication, Option<String>), String> {
    // The 8-bit format stays on the list, as the documentation asks: DXGI
    // falls back to it when the floats are not available.
    let formats = [DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM];
    // `DuplicateOutput1` refuses a thread that does not follow each
    // monitor's own DPI (DXGI_ERROR_UNSUPPORTED). The window makes the
    // whole process do so, the capture benchmark and the tests do not: the
    // thread is made to for the call, and put back as it was.
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let duplicated = output
        .cast::<IDXGIOutput5>()
        .and_then(|o| unsafe { o.DuplicateOutput1(device, 0, &formats) });
    if !previous.0.is_null() {
        unsafe { SetThreadDpiAwarenessContext(previous) };
    }
    match duplicated {
        Ok(duplication) => Ok((duplication, None)),
        Err(e) => {
            let duplication =
                unsafe { output.DuplicateOutput(device) }.map_err(err("desktop duplication"))?;
            Ok((duplication, Some(e.to_string())))
        }
    }
}

/// The SDR white Windows uses on the display named `device`, in scRGB units
/// (1.0 is 80 nits): what the "SDR content brightness" slider is set to.
/// None where Windows does not say: before Windows 10 1709, or for a display
/// it does not list.
fn sdr_white(device: &[u16; 32]) -> Option<f32> {
    let (mut path_count, mut mode_count) = (0u32, 0u32);
    let sized = unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    };
    if sized != ERROR_SUCCESS {
        return None;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    let queried = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if queried != ERROR_SUCCESS {
        return None;
    }
    paths.truncate(path_count as usize);

    // A path joins the source DXGI names to the target, the panel, whose
    // white it is. A cloned display has one source and several panels,
    // each with its own white; the image is one, so the first panel Windows
    // lists sets it.
    paths.iter().find_map(|path| {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            viewGdiDeviceName: [0; 32],
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0
            || !same_name(&source.viewGdiDeviceName, device)
        {
            return None;
        }
        let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                size: size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            SDRWhiteLevel: 0,
        };
        // Windows gives the level in thousandths of 80 nits.
        (unsafe { DisplayConfigGetDeviceInfo(&mut white.header) } == 0 && white.SDRWhiteLevel > 0)
            .then(|| white.SDRWhiteLevel as f32 / 1000.0)
    })
}

/// Two NUL-terminated wide strings, compared up to their terminators.
fn same_name(a: &[u16; 32], b: &[u16; 32]) -> bool {
    let end = |s: &[u16; 32]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
    a[..end(a)] == b[..end(b)]
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
            "dxgi: {}x{} readback, {:.3} ms per frame over {reads} frames (srgb mips: {}, hdr: {}, sdr white: {:?})",
            f.width,
            f.height,
            spent.as_secs_f64() * 1000.0 / reads.max(1) as f64,
            session.format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
            session
                .target
                .as_ref()
                .is_some_and(|t| t.format == DXGI_FORMAT_R16G16B16A16_FLOAT),
            sdr_white(&session.display).map(|w| w * 80.0)
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
