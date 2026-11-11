[CmdletBinding()]
param([switch]$Offline)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$cache = Join-Path (Split-Path -Parent $PSScriptRoot) 'target/release-deps'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$archive = Get-PinnedFile -Uri 'https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip' -Destination (Join-Path $cache 'minisign-0.12-win64.zip') -Sha256 '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479' -Offline:$Offline
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $file = Join-Path $cache 'minisign.exe'
    [IO.Compression.ZipFileExtensions]::ExtractToFile($zip.GetEntry('minisign-win64/x86_64/minisign.exe'), $file, $true)
} finally { $zip.Dispose() }
Write-Output $file
