# Publie une version sur GitHub : génère Hearth-Setup.exe puis crée la release `v<version>` avec l'installateur en pièce jointe.
# La version est celle de `crates/shell/Cargo.toml` (à augmenter avant) ; les consoles installées la détectent et proposent la mise à jour.
# Nécessite la CLI GitHub (`winget install GitHub.cli`, puis `gh auth login`).
param([string]$Notes = '')
$ErrorActionPreference = 'Stop'
Set-Location "$PSScriptRoot\.."
$version = (Select-String -Path crates\shell\Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
& "$PSScriptRoot\make-installer.ps1"
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "gh introuvable : winget install GitHub.cli" }
if (-not $Notes) { $Notes = "Hearth $version" }
gh release create "v$version" dist\Hearth-Setup.exe --repo Kosnix/hearth --title "Hearth $version" --notes $Notes

# Archive de chaque release sur le NAS (en plus de la copie « dernière version » faite par make-installer.ps1),
# pour installer/réinstaller une version précise sans dépendre de GitHub.
$share = '\\192.168.1.254\fusion_storage\Drive\Hearth\releases'
if (Test-Path (Split-Path $share)) {
    New-Item -ItemType Directory -Force $share | Out-Null
    Copy-Item dist\Hearth-Setup.exe "$share\Hearth-Setup-v$version.exe" -Force
    Write-Host "Archivé sur le NAS : $share\Hearth-Setup-v$version.exe"
} else {
    Write-Warning "Partage inaccessible : $share"
}
