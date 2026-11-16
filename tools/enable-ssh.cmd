@echo off
rem Active OpenSSH Server et autorise la cle de dev. Double-clic suffit (auto-elevation).
net session >nul 2>&1 || (powershell -NoProfile -Command "Start-Process '%~f0' -Verb RunAs" & exit /b)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
 "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0 | Out-Null;" ^
 "Set-Service sshd -StartupType Automatic; Start-Service sshd;" ^
 "if (-not (Get-NetFirewallRule -Name sshd-lan -EA SilentlyContinue)) { New-NetFirewallRule -Name sshd-lan -DisplayName 'OpenSSH LAN' -Protocol TCP -LocalPort 22 -Action Allow -Profile Any | Out-Null };" ^
 "Set-Content C:\ProgramData\ssh\administrators_authorized_keys 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILXMFmJrQyEFNXp3bPueCxoaWSCbtjtoExwkx6Vk+Cbg hearth-dev';" ^
 "icacls C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant '*S-1-5-32-544:F' /grant 'SYSTEM:F' | Out-Null"
echo.
echo SSH actif. Utilisateur : %USERNAME%
pause
