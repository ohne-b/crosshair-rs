[CmdletBinding()]
param([string]$OutputDirectory, [switch]$Offline)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$repoRoot = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $repoRoot 'target/release-deps'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
if (!$OutputDirectory) { $OutputDirectory = $cache }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
# Microsoft.VisualCpp.CRT.Appx.Msi 14.44.35208, from Microsoft's VS2022 catalog.
# Extract only the redistributable framework; do not run its MSI or change trust.
$cab = Get-PinnedFile -Uri 'https://download.visualstudio.microsoft.com/download/pr/d2fa077f-a814-4fb2-b903-1fca7658d17e/dd4dbe036d235dd902ef41f2cb3ec7ce7ab1ebf93e6cc26fb6d7283301188e5a/cab1.cab' -Destination (Join-Path $cache 'vclibs-appx-14.44.35208.cab') -Sha256 'dd4dbe036d235dd902ef41f2cb3ec7ce7ab1ebf93e6cc26fb6d7283301188e5a' -Offline:$Offline
$file = Join-Path $OutputDirectory 'Microsoft.VCLibs.x64.14.00.appx'
if (!(Test-Path -LiteralPath $file)) {
    & (Join-Path $env:SystemRoot 'System32/expand.exe') '-F:Microsoft.VCLibs.x64.14.00.appx' $cab $OutputDirectory | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'VCLibs extraction failed.' }
}
if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'VCLibs package hash mismatch.' }
$identity = Read-AppxIdentity $file
if ($identity.Name -ne 'Microsoft.VCLibs.140.00' -or $identity.ProcessorArchitecture -ne 'x64' -or $identity.Version -ne '14.0.33519.0' -or $identity.Publisher -ne 'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US') { throw 'Unexpected VCLibs identity, architecture, publisher or version.' }
& (Get-WindowsSdkTool 'signtool') verify /pa /all $file | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Microsoft framework signature verification failed.' }
Write-Output $file
