# Development aid for the GPUI interface: runs a debug build on a settings
# folder of its own (never the real one, never the real bulbs) and saves a
# picture of its window, to check a change by eye. Windows only.
# Usage: snap.ps1 -Exe target/debug/lueur-gpui.exe -DataDir <folder> -Out shot.png
param([string]$Exe, [string]$DataDir, [string]$Out, [int]$Wait = 5, [string]$Lang = "")
# Starts the app on an empty data folder, captures only its window, stops it.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[W]::SetProcessDPIAware() | Out-Null
$env:LUEUR_DATA_DIR = $DataDir
$p = Start-Process -FilePath $Exe -PassThru
try {
  Start-Sleep -Seconds $Wait
  $p.Refresh()
  $h = $p.MainWindowHandle
  if ($h -eq [IntPtr]::Zero) { Write-Output "no window"; return }
  [W]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 600
  $r = New-Object W+RECT
  [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $hh = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Output "captured ${w}x${hh}"
} finally {
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
