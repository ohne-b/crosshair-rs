[CmdletBinding()]
param([switch]$Offline)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$cache = Join-Path (Split-Path -Parent $PSScriptRoot) 'target/release-deps'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
# Tauri's pinned portable toolchain; NSIS is never a separate release asset.
$archive = Get-PinnedFile -Uri 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip' -Destination (Join-Path $cache 'nsis-3.13.zip') -Sha256 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8' -Offline:$Offline
Expand-Archive -LiteralPath $archive -DestinationPath $cache -Force
Write-Output (Join-Path $cache 'nsis-3.13/makensis.exe')
