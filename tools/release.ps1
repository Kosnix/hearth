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
