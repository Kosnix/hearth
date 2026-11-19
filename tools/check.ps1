# Contrôle de non-régression : tests, compilation, puis mesures sur le shell installé (CPU au repos, mémoire, son) et captures d'écran.
# Usage : . .\tools\check.ps1 ; Check          (sans entrée simulée : ne touche pas à une session en cours)
. "$PSScriptRoot\dev.ps1"

function Check {
    $root = Resolve-Path "$PSScriptRoot\.."
    Push-Location $root
    "== tests ==";  cargo test --release 2>&1 | Select-String 'test result|FAILED|error' | ForEach-Object { $_.Line }
    "== compilation ==";  $w = (cargo build --release 2>&1 | Select-String '^warning|^error').Count; "avertissements/erreurs : $w"
    Pop-Location

    $p = Get-CimInstance Win32_Process -Filter "Name='hearth-shell.exe'" | Select-Object -First 1
    if (-not $p) { "shell non lancé : lancez Deploy d'abord"; return }
    $pr = Get-Process -Id $p.ProcessId
    $c1 = $pr.CPU; $t0 = Get-Date; Start-Sleep 10; $pr.Refresh()
    "== repos ==";  "CPU : {0:N1} % d'un coeur (cible < 5 %)" -f (($pr.CPU - $c1) / ((Get-Date) - $t0).TotalSeconds * 100)
    "mémoire : {0} Mo ; threads : {1}" -f [int]($pr.WorkingSet64 / 1MB), $pr.Threads.Count

    "== captures =="; Shot check_home | Out-Null; "C:\dev\shots\check_home.png"
}
