@echo off
rem Active/repare OpenSSH Server, autorise la cle de dev, desactive la veille sur secteur. Double-clic (auto-elevation).
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process '%~f0' -Verb RunAs" & exit /b)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "$cap='OpenSSH.Server~~~~0.0.1.0';" ^
 "if (-not (Get-Service sshd -EA SilentlyContinue)) { Write-Host 'Reinstallation OpenSSH (capacite Windows)...'; Remove-WindowsCapability -Online -Name $cap -EA SilentlyContinue | Out-Null; Add-WindowsCapability -Online -Name $cap | Out-Null };" ^
 "if (-not (Get-Service sshd -EA SilentlyContinue)) { Write-Host 'Installation OpenSSH (MSI Microsoft via winget)...'; $w=Get-ChildItem \"$env:ProgramFiles\WindowsApps\Microsoft.DesktopAppInstaller_*_x64__8wekyb3d8bbwe\winget.exe\" | Select -Last 1 -Expand FullName; & $w install -e --id Microsoft.OpenSSH.Preview --source winget --silent --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null };" ^
 "Write-Host ('Capacite Windows : ' + (Get-WindowsCapability -Online -Name $cap).State);" ^
 "Get-Process sshd -EA SilentlyContinue | Stop-Process -Force;" ^
 "Set-Service sshd -StartupType Automatic; Start-Service sshd;" ^
 "Remove-NetFirewallRule -Name sshd-lan -EA SilentlyContinue; New-NetFirewallRule -Name sshd-lan -DisplayName 'OpenSSH LAN' -Protocol TCP -LocalPort 22 -Action Allow -Profile Any | Out-Null;" ^
 "New-Item -ItemType Directory C:\ProgramData\ssh -Force | Out-Null;" ^
 "Set-Content C:\ProgramData\ssh\administrators_authorized_keys 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILXMFmJrQyEFNXp3bPueCxoaWSCbtjtoExwkx6Vk+Cbg hearth-dev';" ^
 "icacls C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant '*S-1-5-32-544:F' /grant 'SYSTEM:F' | Out-Null;" ^
 "powercfg /change standby-timeout-ac 0; powercfg /change hibernate-timeout-ac 0;" ^
 "$sv=Get-Service sshd -EA SilentlyContinue; $s=if($sv){$sv.Status}else{'introuvable'}; $p=[bool](Get-NetTCPConnection -LocalPort 22 -State Listen -EA SilentlyContinue);" ^
 "Write-Host ''; Write-Host ('Service SSH : ' + $s + '   Port 22 a l''ecoute : ' + $p) -ForegroundColor Yellow;" ^
 "if ($s -ne 'Running') { Get-WinEvent -LogName 'OpenSSH/Operational' -MaxEvents 3 -EA SilentlyContinue | ForEach-Object { Write-Host $_.Message -ForegroundColor Red } }"
echo.
pause
