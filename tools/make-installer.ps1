# Génère dist\Hearth-Setup.exe : un seul fichier qui embarque hearth.exe et hearth-shell.exe.
$ErrorActionPreference = 'Stop'
Set-Location "$PSScriptRoot\.."
cargo build --release
cargo build --release -p hearth-setup
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\hearth-setup.exe dist\Hearth-Setup.exe -Force
# Dossier de diffusion (partage réseau) : chaque nouvelle version y est déposée.
$share = '\\192.168.1.254\fusion_storage\Drive'
if (Test-Path $share) { Copy-Item dist\Hearth-Setup.exe $share -Force } else { Write-Warning "Partage inaccessible : $share" }
Get-Item dist\Hearth-Setup.exe | Select-Object FullName, @{n = 'Mo'; e = { [math]::Round($_.Length / 1MB, 1) } }
