# Génère le logo Hearth : assets\logo.svg, assets\logo-512.png, assets\hearth.ico (16 à 256 px).
# Le logo : une arche de foyer et une flamme. Les chemins ci-dessous servent aussi dans l'interface (crates\shell\ui\app.slint, composant Logo).
Add-Type -AssemblyName System.Drawing

$arch  = 'M 14 92 L 14 52 C 14 26 30 8 50 8 C 70 8 86 26 86 52 L 86 92'
$flame = 'M 50 30 C 53 42 66 50 66 66 C 66 78 59 86 50 86 C 41 86 34 78 34 66 C 34 58 38 52 42 47 C 43 53 46 56 49 56 C 47 46 47 38 50 30 Z'
$core  = 'M 50 60 C 52 66 58 69 58 75 C 58 80 54 83 50 83 C 46 83 42 80 42 75 C 42 71 45 68 47 65 C 48 67 49 66 50 60 Z'

# Flamme agrandie de 18 % autour de sa base (centre 50, 86) pour mieux remplir l'arche.
function Scale-Path([string]$d, [float]$k, [float]$cx, [float]$cy) {
    $t = $d -split '\s+'; $out = @(); $i = 0
    while ($i -lt $t.Count) {
        if ($t[$i] -match '^[MLCZ]$') { $out += $t[$i]; $i++ }
        else { $x = [float]$t[$i]; $y = [float]$t[$i + 1]; $inv = [Globalization.CultureInfo]::InvariantCulture; $out += ([math]::Round($cx + ($x - $cx) * $k, 1)).ToString($inv); $out += ([math]::Round($cy + ($y - $cy) * $k, 1)).ToString($inv); $i += 2 }
    }
    $out -join ' '
}
$flame = Scale-Path $flame 1.18 50 86
$core  = Scale-Path $core 1.18 50 86

function New-Path([string]$d, [float]$s, [float]$ox, [float]$oy) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $t = $d -split '\s+'; $i = 0; $cur = $null
    $pt = { param($k) New-Object System.Drawing.PointF(($ox + [float]$t[$k] * $s), ($oy + [float]$t[$k + 1] * $s)) }
    while ($i -lt $t.Count) {
        switch ($t[$i]) {
            'M' { $cur = & $pt ($i + 1); $p.StartFigure(); $i += 3 }
            'L' { $n = & $pt ($i + 1); $p.AddLine($cur, $n); $cur = $n; $i += 3 }
            'C' { $a = & $pt ($i + 1); $b = & $pt ($i + 3); $c = & $pt ($i + 5); $p.AddBezier($cur, $a, $b, $c); $cur = $c; $i += 7 }
            'Z' { $p.CloseFigure(); $i += 1 }
            default { $i += 1 }
        }
    }
    $p
}

function Render([int]$size, [bool]$badge) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $s = $size / 100.0
    if ($badge) {
        $r = $size * 0.22
        $bg = New-Object System.Drawing.Drawing2D.GraphicsPath
        $bg.AddArc(0, 0, 2 * $r, 2 * $r, 180, 90); $bg.AddArc($size - 2 * $r, 0, 2 * $r, 2 * $r, 270, 90)
        $bg.AddArc($size - 2 * $r, $size - 2 * $r, 2 * $r, 2 * $r, 0, 90); $bg.AddArc(0, $size - 2 * $r, 2 * $r, 2 * $r, 90, 90); $bg.CloseFigure()
        $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Point(0, 0)), (New-Object System.Drawing.Point(0, $size)), [System.Drawing.Color]::FromArgb(255, 44, 44, 48), [System.Drawing.Color]::FromArgb(255, 20, 20, 22))
        $g.FillPath($br, $bg)
        $k = 0.72; $o = $size * (1 - $k) / 2
        $sc = $s * $k
    } else { $sc = $s; $o = 0 }
    $orange = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.PointF(0, ($o + 30 * $sc))), (New-Object System.Drawing.PointF(0, ($o + 88 * $sc))), [System.Drawing.Color]::FromArgb(255, 255, 196, 60), [System.Drawing.Color]::FromArgb(255, 255, 140, 0))
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 255, 165, 0), [float](7 * $sc)); $pen.StartCap = 'Round'; $pen.EndCap = 'Round'; $pen.LineJoin = 'Round'
    $g.DrawPath($pen, (New-Path $arch $sc $o $o))
    $g.FillPath($orange, (New-Path $flame $sc $o $o))
    $g.FillPath((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 226, 150))), (New-Path $core $sc $o $o))
    $g.Dispose(); $bmp
}

$dir = Join-Path $PSScriptRoot '..\assets'; New-Item -ItemType Directory -Force $dir | Out-Null
(Render 512 $true).Save((Join-Path $dir 'logo-512.png'), [System.Drawing.Imaging.ImageFormat]::Png)
(Render 512 $false).Save((Join-Path $dir 'logo-mark-512.png'), [System.Drawing.Imaging.ImageFormat]::Png)

# .ico : images PNG de 16 à 256 px
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = foreach ($z in $sizes) { $ms = New-Object System.IO.MemoryStream; (Render $z $true).Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); ,$ms.ToArray() }
$out = New-Object System.IO.MemoryStream; $bw = New-Object System.IO.BinaryWriter $out
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$off = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $z = $sizes[$i]; $b = if ($z -ge 256) { 0 } else { $z }
    $bw.Write([byte]$b); $bw.Write([byte]$b); $bw.Write([byte]0); $bw.Write([byte]0); $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$pngs[$i].Length); $bw.Write([uint32]$off); $off += $pngs[$i].Length
}
foreach ($p in $pngs) { $bw.Write($p) }
[IO.File]::WriteAllBytes((Join-Path $dir 'hearth.ico'), $out.ToArray())

$svg = @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="f" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffc43c"/><stop offset="1" stop-color="#ff8c00"/></linearGradient></defs>
<path d="$arch" fill="none" stroke="#ffa500" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>
<path d="$flame" fill="url(#f)"/><path d="$core" fill="#ffe296"/></svg>
"@
[IO.File]::WriteAllText((Join-Path $dir 'logo.svg'), $svg)
"ok : " + (Get-ChildItem $dir | ForEach-Object { $_.Name + ' (' + $_.Length + ' o)' }) -join ', '
