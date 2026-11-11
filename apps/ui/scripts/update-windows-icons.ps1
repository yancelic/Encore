$ErrorActionPreference = 'Stop'
$uiRoot = Split-Path $PSScriptRoot -Parent
$repoRoot = Split-Path (Split-Path $uiRoot -Parent) -Parent
$generated = Join-Path $repoRoot '.local-backups/generated-windows-icons'
$iconDir = Join-Path $uiRoot 'src-tauri/icons'
& node (Join-Path $uiRoot 'node_modules/@tauri-apps/cli/tauri.js') icon (Join-Path $uiRoot 'src/assets/encore-mark.svg') --output $generated
if ($LASTEXITCODE -ne 0) { throw 'Icon generation failed' }
foreach ($name in @('icon.ico','icon.png','32x32.png','64x64.png','128x128.png','128x128@2x.png')) {
    Copy-Item -LiteralPath (Join-Path $generated $name) -Destination (Join-Path $iconDir $name)
}
Copy-Item -LiteralPath (Join-Path $generated 'icon.png') -Destination (Join-Path $uiRoot 'src/assets/encore-icon.png')

# The Win32 recorder and Tauri tray share a premultiplied BGRA base.
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new((Join-Path $generated '32x32.png'))
try {
    $bytes = [byte[]]::new(32 * 32 * 4)
    for ($y = 0; $y -lt 32; $y++) {
        for ($x = 0; $x -lt 32; $x++) {
            $pixel = $bitmap.GetPixel($x, $y)
            $offset = ($y * 32 + $x) * 4
            $bytes[$offset] = [Math]::Floor(($pixel.B * $pixel.A + 127) / 255)
            $bytes[$offset + 1] = [Math]::Floor(($pixel.G * $pixel.A + 127) / 255)
            $bytes[$offset + 2] = [Math]::Floor(($pixel.R * $pixel.A + 127) / 255)
            $bytes[$offset + 3] = $pixel.A
        }
    }
    [System.IO.File]::WriteAllBytes((Join-Path $repoRoot 'crates/common/src/tray_icon.bgra'), $bytes)
} finally { $bitmap.Dispose() }
