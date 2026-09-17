# 生成 assets-gui 的应用图标（多尺寸 .ico）。
# 图形只用几何形状、不依赖字体：圆角方块底 + 三段逐级缩进的短杠，
# 对应界面左侧「平台 -> 账号 -> 变量」的三段缩进树。色值取自 tokens.slint 的强调色。
# 用法：powershell -File make-icon.ps1 [-Preview <目录>]
param(
    [string]$Out = (Join-Path $PSScriptRoot 'assets-gui.ico'),
    [string]$Preview
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$accent = [System.Drawing.Color]::FromArgb(255, 0x02, 0x78, 0x7d)
$ink = [System.Drawing.Color]::FromArgb(255, 0xf8, 0xfd, 0xfd)

function New-RoundedPath([double]$x, [double]$y, [double]$w, [double]$h, [double]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    if ($r -le 0) {
        $path.AddRectangle((New-Object System.Drawing.RectangleF($x, $y, $w, $h)))
        return $path
    }
    $d = 2 * $r
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.Clear([System.Drawing.Color]::Transparent)

        $bg = New-RoundedPath 0 0 $size $size ($size * 0.18)
        $bgBrush = New-Object System.Drawing.SolidBrush($accent)
        $g.FillPath($bgBrush, $bg)
        $bg.Dispose(); $bgBrush.Dispose()

        $thickness = [Math]::Max(2.0, [Math]::Round($size * 0.105))
        $gap = [Math]::Max(1.0, [Math]::Round($thickness * 0.82))
        $lefts = @(0.20, 0.30, 0.40)
        $widths = @(0.60, 0.42, 0.24)
        $blockTop = ($size - (3 * $thickness + 2 * $gap)) / 2
        $barBrush = New-Object System.Drawing.SolidBrush($ink)
        for ($i = 0; $i -lt 3; $i++) {
            $bar = New-RoundedPath ($size * $lefts[$i]) ($blockTop + $i * ($thickness + $gap)) `
                ([Math]::Max($thickness * 1.2, $size * $widths[$i])) $thickness ($thickness / 2)
            $g.FillPath($barBrush, $bar)
            $bar.Dispose()
        }
        $barBrush.Dispose()
    } finally {
        $g.Dispose()
    }
    return $bmp
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

$sizes = @(16, 24, 32, 48, 64, 128, 256)
$entries = @()
foreach ($size in $sizes) {
    $bmp = New-IconBitmap $size
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
