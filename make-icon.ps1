# make-icon.ps1 — 用官方 DeepSeek 鲸鱼 LOGO 生成多尺寸 app.ico
# 数据来源: deepseek.com 页脚官方 SVG（<g clip-path="url(#clip0_logo)"> 内路径）
# 用法: 由 build.ps1 自动调用；也可单独运行。

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$pathFile = Join-Path $here 'assets\whale-path.txt'
if (-not (Test-Path $pathFile)) { throw "缺少 $pathFile（官方鲸鱼 SVG 路径数据）" }
$d = ([System.IO.File]::ReadAllText($pathFile)).Trim()

# ---------- SVG path (M/C/V/Z 绝对坐标) → GraphicsPath ----------
function New-WhaleGraphicsPath {
    param([string]$d)
    $gp = New-Object System.Drawing.Drawing2D.GraphicsPath
    $items = @()
    foreach ($m in [regex]::Matches($d, '[A-Za-z]|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?')) { $items += $m.Value }
    $i = 0
    $curX = 0.0; $curY = 0.0; $startX = 0.0; $startY = 0.0
    while ($i -lt $items.Count) {
        $cmd = $items[$i]; $i++
        switch ($cmd) {
            'M' {
                $startX = [double]$items[$i];   $startY = [double]$items[$i + 1]; $i += 2
                $curX = $startX; $curY = $startY
            }
            'C' {
                $x1 = [double]$items[$i];   $y1 = [double]$items[$i + 1]
                $x2 = [double]$items[$i + 2]; $y2 = [double]$items[$i + 3]
                $x  = [double]$items[$i + 4]; $y  = [double]$items[$i + 5]
                $i += 6
                $gp.AddBezier($curX, $curY, $x1, $y1, $x2, $y2, $x, $y) | Out-Null
                $curX = $x; $curY = $y
            }
            'V' {
                $y = [double]$items[$i]; $i++
                $gp.AddLine($curX, $curY, $curX, $y) | Out-Null
                $curY = $y
            }
            'Z' {
                $gp.CloseFigure()
                $curX = $startX; $curY = $startY
            }
        }
    }
    # Alternate 填充模式：内部子路径（反向绕行）自动成为镂空（鲸鱼眼睛/螺旋负空间）
    $gp.FillMode = [System.Drawing.Drawing2D.FillMode]::Alternate
    return $gp
}

# ---------- 渲染鲸鱼 ----------
function New-WhaleBitmap {
    param([int]$size, [System.Drawing.Drawing2D.GraphicsPath]$gp)
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)
    # 鲸鱼包围盒: x ∈ [-0.093, 26.791], y ∈ [1.754, 21.491]
    $wx = 26.884; $wy = 19.737
    $s = ($size * 0.92) / $wx
    $offX = ($size - $wx * $s) / 2 + 0.093 * $s
    $offY = ($size - $wy * $s) / 2 + 1.754 * $s
    $g.TranslateTransform($offX, $offY)
    $g.ScaleTransform($s, $s)
    # DeepSeek 品牌蓝渐变: #7A97FE (顶) → #4D6BFE (底)，渐变范围取鲸鱼包围盒
    $c1 = [System.Drawing.Color]::FromArgb(255, 122, 151, 254)
    $c2 = [System.Drawing.Color]::FromArgb(255, 77, 107, 254)
    $rect = New-Object System.Drawing.RectangleF 0, 1.754, 26.884, 19.737
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, $c1, $c2, 90.0
    $g.FillPath($brush, $gp) | Out-Null
    $brush.Dispose()
    $g.Dispose()
    return $bmp
}

# ---------- ICO 位图条目 (BMP DIB: BITMAPINFOHEADER + XOR(BGRA) + AND 掩码) ----------
function Get-DibData {
    param([System.Drawing.Bitmap]$bmp)
    $w = $bmp.Width; $h = $bmp.Height
    $xrow = $w * 4
    $andRow = [int]([Math]::Ceiling($w / 32.0) * 4)
    $xorSize = $xrow * $h
    $andSize = $andRow * $h
    $data = New-Object byte[] (40 + $xorSize + $andSize)

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter $ms
    $bw.Write([int32]40); $bw.Write([int32]$w); $bw.Write([int32]($h * 2))
    $bw.Write([int16]1); $bw.Write([int16]32)
    $bw.Write([int32]0); $bw.Write([int32]($xorSize + $andSize))
    $bw.Write([int32]0); $bw.Write([int32]0); $bw.Write([int32]0); $bw.Write([int32]0)
    $bw.Flush()
    $hdr = $ms.ToArray()
    $bw.Close(); $ms.Close()
    [Array]::Copy($hdr, 0, $data, 0, 40)

    $rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
    $bd = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $bd.Stride
    $row = New-Object byte[] $stride
    $dst = 40
    for ($y = $h - 1; $y -ge 0; $y--) {
        [System.Runtime.InteropServices.Marshal]::Copy([IntPtr]::Add($bd.Scan0, $y * $stride), $row, 0, $stride)
        for ($x = 0; $x -lt $w; $x++) {
            $data[$dst + $x * 4]     = $row[$x * 4]       # B
            $data[$dst + $x * 4 + 1] = $row[$x * 4 + 1]   # G
            $data[$dst + $x * 4 + 2] = $row[$x * 4 + 2]   # R
            $data[$dst + $x * 4 + 3] = $row[$x * 4 + 3]   # A
        }
        $dst += $xrow
    }
    $bmp.UnlockBits($bd)

    $maskStart = 40 + $xorSize
    for ($y = 0; $y -lt $h; $y++) {
        for ($x = 0; $x -lt $w; $x++) {
            if ($bmp.GetPixel($x, $y).A -lt 128) {
                $byteIdx = $maskStart + ($h - 1 - $y) * $andRow + ($x -shr 3)
                $data[$byteIdx] = $data[$byteIdx] -bor [byte](0x80 -shr ($x % 8))
            }
        }
    }
    return ,$data
}

# ---------- 主流程 ----------
$gp = New-WhaleGraphicsPath $d
$sizes = @(16, 32, 48, 256)
$blobs = @()

foreach ($sz in $sizes) {
    $bmp = New-WhaleBitmap $sz $gp
    if ($sz -ge 256) {
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $blob = $ms.ToArray()
        $ms.Close()
        # 同时输出一张 256 PNG 参考图
        $bmp.Save((Join-Path $here 'whale-256.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        $blob = Get-DibData $bmp
    }
    $bmp.Dispose()
    $blobs += ,$blob
}
$gp.Dispose()

# 组装 ICO
$count = $sizes.Count
$bytes = New-Object System.Collections.Generic.List[byte]
$bytes.AddRange([BitConverter]::GetBytes([uint16]0))   # reserved
$bytes.AddRange([BitConverter]::GetBytes([uint16]1))   # type: icon
$bytes.AddRange([BitConverter]::GetBytes([uint16]$count))
$offset = 6 + 16 * $count
for ($k = 0; $k -lt $count; $k++) {
    $sz = $sizes[$k]
    $len = $blobs[$k].Length
    $dim = $(if ($sz -ge 256) { 0 } else { $sz })
    $bytes.Add([byte]$dim)
    $bytes.Add([byte]$dim)
    $bytes.Add([byte]0)
    $bytes.Add([byte]0)
    $bytes.AddRange([BitConverter]::GetBytes([uint16]1))    # planes
    $bytes.AddRange([BitConverter]::GetBytes([uint16]32))   # bpp
    $bytes.AddRange([BitConverter]::GetBytes([uint32]$len))
    $bytes.AddRange([BitConverter]::GetBytes([uint32]$offset))
    $offset += $len
}
foreach ($blob in $blobs) { $bytes.AddRange($blob) }

$icoPath = Join-Path $here 'app.ico'
[System.IO.File]::WriteAllBytes($icoPath, $bytes.ToArray())
Write-Host ("app.ico 已生成: {0} ({1} bytes, {2} 个尺寸)" -f $icoPath, (Get-Item $icoPath).Length, $count)
