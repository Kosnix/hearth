# Publie une version sur GitHub : incrémente `VERSION` (4 chiffres, ex. 0.1.0.3 -> 0.1.0.4), génère Hearth-Setup.exe
# puis crée la release `v<version>` avec l'installateur en pièce jointe. Les consoles installées comparent ce
# numéro (`HEARTH_VERSION`, embarqué depuis `VERSION` par build.rs) et proposent la mise à jour.
# Nécessite la CLI GitHub (`winget install GitHub.cli`, puis `gh auth login`).
param([string]$Notes = '')
$ErrorActionPreference = 'Stop'
Set-Location "$PSScriptRoot\.."
# Via cmd : sous Windows PowerShell 5.1, un simple avertissement de git/cargo sur stderr déclencherait
# `Stop` et interromprait la publication à mi-chemin ; on juge l'échec au code de retour.
function Invoke-Native([string]$cmd) { cmd /c "$cmd 2>&1" | Out-Host; if ($LASTEXITCODE) { throw "$cmd : code $LASTEXITCODE" } }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "gh introuvable : winget install GitHub.cli" }
$parts = (Get-Content VERSION -Raw).Trim().Split('.')
$parts[3] = [string]([int]$parts[3] + 1)
$version = $parts -join '.'
Set-Content VERSION $version -NoNewline
Add-Content VERSION "`n"
Invoke-Native 'git add VERSION'
Invoke-Native "git commit -m `"Version $version`""
& "$PSScriptRoot\make-installer.ps1"
# Le commit de version doit être sur GitHub avant la release : son tag est posé sur la branche distante.
Invoke-Native 'git push'
if (-not $Notes) { $Notes = "Hearth $version" }
$notesFile = New-TemporaryFile
[IO.File]::WriteAllText($notesFile, $Notes)
Invoke-Native "gh release create v$version dist\Hearth-Setup.exe --repo Kosnix/hearth --target $(git rev-parse HEAD) --title `"Hearth $version`" --notes-file `"$notesFile`""
Remove-Item $notesFile

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
