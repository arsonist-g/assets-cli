# 从美术母版 assets-gui-source.png 生成应用图标 assets-gui.ico（多尺寸）。
# 母版是 1024x1024 的应用图标稿（圆角方块铺满画布、方块外全透明），换图标时替换这张图再跑本脚本。
# 每个尺寸都从母版逐级折半后再收到目标尺寸：一步从 1024 缩到 16 会把结构糊掉。
# 用法：powershell -File make-icon.ps1 [-Preview <目录>]
param(
    [string]$Source = (Join-Path $PSScriptRoot 'assets-gui-source.png'),
    [string]$Out = (Join-Path $PSScriptRoot 'assets-gui.ico'),
    [string]$Preview
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $Source)) {
    throw "找不到母版图 $Source：图标由它生成，不能只改 .ico"
}

function New-ScaledBitmap([System.Drawing.Bitmap]$src, [int]$size) {
    $cur = $src
    while ($cur.Width -gt $size * 2) {
        $n = [int]($cur.Width / 2)
        $step = New-Object System.Drawing.Bitmap($n, $n, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $gs = [System.Drawing.Graphics]::FromImage($step)
        $gs.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $gs.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $gs.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $gs.DrawImage($cur, 0, 0, $n, $n)
        $gs.Dispose()
        if (-not [object]::ReferenceEquals($cur, $src)) { $cur.Dispose() }
        $cur = $step
    }
    $result = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($result)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($cur, 0, 0, $size, $size)
    $g.Dispose()
    if (-not [object]::ReferenceEquals($cur, $src)) { $cur.Dispose() }
    return $result
}

# 取 32bpp 位图里的一份 DIB 字节：BITMAPINFOHEADER + 自下而上的 BGRA 像素 + 全零 AND 掩码。
function Get-DibBytes([System.Drawing.Bitmap]$bmp) {
    $w = $bmp.Width
    $h = $bmp.Height
    $rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $stride = $data.Stride
        $raw = New-Object byte[] ($stride * $h)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $raw, 0, $raw.Length)
    } finally {
        $bmp.UnlockBits($data)
    }

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)
    $bw.Write([uint32]40)
    $bw.Write([int32]$w)
    $bw.Write([int32]($h * 2))          # DIB 高度含掩码，写两倍
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]0)                 # BI_RGB
    $bw.Write([uint32]($w * $h * 4))
    $bw.Write([int32]0); $bw.Write([int32]0)
    $bw.Write([uint32]0); $bw.Write([uint32]0)
    for ($y = $h - 1; $y -ge 0; $y--) { $bw.Write($raw, $y * $stride, $w * 4) }
    $maskStride = [int](([Math]::Floor(($w + 31) / 32)) * 4)
    $bw.Write((New-Object byte[] ($maskStride * $h)))
    $bw.Flush()
    return ,$ms.ToArray()
}

$master = [System.Drawing.Bitmap]::FromFile($Source)
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$entries = @()
try {
    foreach ($size in $sizes) {
        $bmp = New-ScaledBitmap $master $size
        try {
            $entries += [pscustomobject]@{ size = $size; bytes = (Get-DibBytes $bmp) }
            if ($Preview) {
                New-Item -ItemType Directory -Force -Path $Preview | Out-Null
                $bmp.Save((Join-Path $Preview "icon-$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
            }
        } finally {
            $bmp.Dispose()
        }
    }
} finally {
    $master.Dispose()
}

$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$entries.Count)
$offset = 6 + 16 * $entries.Count
foreach ($entry in $entries) {
    $dim = if ($entry.size -ge 256) { 0 } else { $entry.size }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim)
    $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$entry.bytes.Length); $bw.Write([uint32]$offset)
    $offset += $entry.bytes.Length
}
foreach ($entry in $entries) { $bw.Write([byte[]]$entry.bytes) }
$bw.Flush()
[IO.File]::WriteAllBytes($Out, $ms.ToArray())
Write-Host "已生成 $Out（$($entries.Count) 个尺寸 / $((Get-Item $Out).Length) 字节）"