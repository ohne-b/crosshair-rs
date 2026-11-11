[CmdletBinding()]
param([string]$Package)
$ErrorActionPreference = 'Stop'
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') { throw 'This release requires x64 Windows.' }
if ([Environment]::OSVersion.Version.Build -lt 19045) { throw 'Windows 10 22H2 or Windows 11 is required.' }
if (!$Package) {
    $files = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter 'crosshair-rs-*-windows-x64.msix' -File)
    if ($files.Count -ne 1) { throw 'Place exactly one signed crosshair-rs MSIX beside this script, or pass -Package.' }
    $Package = $files[0].FullName
}
$Package = (Resolve-Path -LiteralPath $Package).Path
if ([IO.Path]::GetFileName($Package) -notmatch '^crosshair-rs-[0-9]+\.[0-9]+\.[0-9]+-windows-x64\.msix$') { throw 'Expected a stable signed crosshair-rs release package.' }
$framework = Join-Path $PSScriptRoot 'Microsoft.VCLibs.x64.14.00.appx'
if (!(Test-Path -LiteralPath $framework -PathType Leaf) -or (Get-FileHash -LiteralPath $framework -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'The bundled Microsoft x64 VCLibs framework is missing or damaged. Download the complete release.' }
# Windows verifies package publisher trust, signatures and dependency identity.
# This script never imports certificates, enables Developer Mode or elevates.
Add-AppxPackage -Path $Package -DependencyPath $framework -ErrorAction Stop
Write-Output 'crosshair-rs installed. Open it from Start.'
