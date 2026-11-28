# Outils de test autonome de Hearth (à dot-sourcer : . .\tools\dev.ps1)
#   Deploy          compile, arrête la console, copie les binaires installés, relance
#   Press 'g'       simule un bouton : up down left right enter back g(uide) m(enu) tri sq l1 r1
#   Fg              décrit la fenêtre au premier plan
#   Shot [nom]      capture d'écran -> C:\dev\shots\<nom>.png
#   Ask             met la fenêtre de Claude au premier plan pour poser une question

Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public class HearthDev {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int L, T, R, B; }
  public static string Info(IntPtr h) {
    var sb = new StringBuilder(256); GetWindowText(h, sb, 256); RECT r; GetWindowRect(h, out r);
    return String.Format("'{0}' style=0x{1:X8} rect={2},{3},{4},{5} iconic={6}", sb, GetWindowLong(h, -16), r.L, r.T, r.R, r.B, IsIconic(h));
  }
  public static void Focus(IntPtr h) {
    keybd_event(0x12, 0, 0, UIntPtr.Zero);
    if (IsIconic(h)) ShowWindow(h, 9);
    SetForegroundWindow(h);
    keybd_event(0x12, 0, 2, UIntPtr.Zero);
  }
}
'@ -ErrorAction SilentlyContinue
[HearthDev]::SetProcessDPIAware() | Out-Null
$script:HearthApp = "$env:LOCALAPPDATA\Hearth\app"
$script:Ws = New-Object -ComObject WScript.Shell

function Deploy {
    cargo build --release 2>&1 | Select-String '^error|-->|Finished'
    Get-Process hearth, hearth-shell -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    for ($i = 0; $i -lt 20 -and (Get-Process hearth, hearth-shell -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
    Start-Sleep 1
    # install : copie les binaires, (re)crée le raccourci « Mode console » (icône, cible) du bureau.
    & "$PSScriptRoot\..\target\release\hearth.exe" install | Out-Null
    Start-Sleep 3
    Remove-Item "$script:HearthApp\staged" -Recurse -Force -ErrorAction SilentlyContinue
    Start-Process "$script:HearthApp\hearth.exe"
    for ($i = 0; $i -lt 40 -and -not (Get-Process hearth-shell -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
    Start-Sleep 1
}

function Press([string]$key, [int]$wait = 600) {
    $idx = @{ up = 0; down = 1; left = 2; right = 3; enter = 4; back = 5; g = 6; m = 7; tri = 8; sq = 9; l1 = 10; r1 = 11 }[$key]
    # F1..F8 (0x70..) ; instance de dev : F13..F20 (0x7C..), que SendKeys ne sait pas envoyer.
    $base = 0x70; if ($env:HEARTH_DEV_TOPMOST) { $base = 0x7C }
    $vk = $base + $idx
    foreach ($m in 0x11, 0x12, 0x10) { [HearthDev]::keybd_event($m, 0, 0, [UIntPtr]::Zero) }
    [HearthDev]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40
    [HearthDev]::keybd_event($vk, 0, 2, [UIntPtr]::Zero)
    foreach ($m in 0x10, 0x12, 0x11) { [HearthDev]::keybd_event($m, 0, 2, [UIntPtr]::Zero) }
    Start-Sleep -Milliseconds $wait
}

function Fg { [HearthDev]::Info([HearthDev]::GetForegroundWindow()) }

function Shot([string]$name = 'shot') {
    New-Item -ItemType Directory -Force C:\dev\shots | Out-Null
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save("C:\dev\shots\$name.png"); $bmp.Dispose()
    "C:\dev\shots\$name.png"
}

function Ask {
    $p = Get-Process claude -ErrorAction SilentlyContinue | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
    if ($p) { [HearthDev]::Focus($p.MainWindowHandle) }
}

#   Dev             compile et lance target\release\hearth-shell.exe (ne touche pas à la copie installée,
#                   utile quand un ancien shell élevé verrouille app\hearth-shell.exe)
function Dev {
    Get-CimInstance Win32_Process -Filter "Name='hearth-shell.exe'" | Where-Object { $_.ExecutablePath -like '*\target\release\*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep 1
    cargo build --release 2>&1 | Select-String '^error|^warning|-->|Finished'
    $env:HEARTH_DEV_TOPMOST = '1'
    Start-Process C:\dev\hearth\target\release\hearth-shell.exe
    Start-Sleep 6
}
