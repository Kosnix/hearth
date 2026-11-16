param([string]$Exe, [string]$Icon)
# Icône Hearth (carré bleu arrondi, flamme blanche) générée à l'installation, puis raccourci « Mode console ».
Add-Type -AssemblyName System.Drawing
if (-not (Test-Path $Icon)) {
    $size = 256
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $r = 56; $d = $r * 2
    $path.AddArc(8, 8, $d, $d, 180, 90); $path.AddArc($size - 8 - $d, 8, $d, $d, 270, 90)
    $path.AddArc($size - 8 - $d, $size - 8 - $d, $d, $d, 0, 90); $path.AddArc(8, $size - 8 - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Point]::new(0, 0)), ([System.Drawing.Point]::new(0, $size)), ([System.Drawing.Color]::FromArgb(61, 90, 153)), ([System.Drawing.Color]::FromArgb(42, 33, 102))
    $g.FillPath($brush, $path)
    $flame = New-Object System.Drawing.Drawing2D.GraphicsPath
    $flame.AddBezier(128, 40, 190, 100, 196, 150, 176, 190)
    $flame.AddBezier(176, 190, 160, 222, 96, 222, 80, 190)
    $flame.AddBezier(80, 190, 62, 154, 100, 120, 108, 90)
    $flame.AddBezier(108, 90, 120, 112, 134, 106, 128, 40)
    $g.FillPath([System.Drawing.Brushes]::White, $flame)
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $png = $ms.ToArray()
    # ICO : en-tête + une entrée 256x256 contenant le PNG.
    $fs = [System.IO.File]::Create($Icon)
    $w = New-Object System.IO.BinaryWriter $fs
    $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]1)
    $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$png.Length); $w.Write([uint32]22)
    $w.Write($png)
    $w.Close()
}
$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop') + '\Mode console.lnk')
$s.TargetPath = $Exe
$s.Arguments = 'console'
$s.IconLocation = $Icon
$s.Description = 'Revenir à la console Hearth'
$s.Save()
