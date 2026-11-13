[CmdletBinding()]
param([Parameter(Mandatory)][string]$Directory, [Parameter(Mandatory)][string]$Version,
      [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedPackageHash, [switch]$Offline)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
& node (Join-Path $PSScriptRoot 'release.mjs') verify-assets $Directory $Version
if ($LASTEXITCODE) { throw 'Release asset verification failed.' }
if (!$env:CROSSHAIR_UPDATE_PUBLIC_KEY -or !$env:CROSSHAIR_PACKAGE_PUBLISHER) { throw 'Release verification requires the public key and package publisher.' }
$manifest = Get-Content -LiteralPath (Join-Path $Directory 'latest.json') -Raw | ConvertFrom-Json
$artifact = $manifest.platforms.'windows-x86_64-nsis'
$minisign = & (Join-Path $PSScriptRoot 'get-minisign.ps1') -Offline:$Offline
$root = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $PSScriptRoot) 'target/release-checks'))
$temporary = Join-Path $root ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary -Force | Out-Null
$setup = Join-Path $temporary 'setup.exe'
$locked = $null
try {
    Copy-Item -LiteralPath (Join-Path $Directory "crosshair-rs-$Version-windows-x64-setup.exe") -Destination $setup
    $locked = [IO.File]::Open($setup, 'Open', 'Read', 'Read')
    foreach ($kind in 'setup','package') {
        $encoded = if ($kind -eq 'setup') { $artifact.signature } else { $artifact.package_signature }
        [IO.File]::WriteAllBytes((Join-Path $temporary "$kind.minisig"), [Convert]::FromBase64String($encoded))
    }
    & $minisign -V -H -P $env:CROSSHAIR_UPDATE_PUBLIC_KEY -m $setup -x (Join-Path $temporary 'setup.minisig')
    if ($LASTEXITCODE) { throw 'Setup signature verification failed.' }
    foreach ($mode in '/VERIFYONLY','/EXTRACT') {
        $process = Start-Process -FilePath $setup -ArgumentList "/S $mode" -WindowStyle Hidden -PassThru
        try {
            if (!$process.WaitForExit(60000)) { $process.Kill(); $process.WaitForExit(); throw 'Setup verification timed out.' }
            if ($process.ExitCode) { throw "Setup $mode failed." }
        } finally { $process.Dispose() }
    }
    $package = Join-Path $temporary 'package.msix'
    & $minisign -V -H -P $env:CROSSHAIR_UPDATE_PUBLIC_KEY -m $package -x (Join-Path $temporary 'package.minisig')
    if ($LASTEXITCODE) { throw 'Embedded package signature verification failed.' }
    if ((Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedPackageHash) { throw 'Embedded package differs from Validation.' }
    $identity = Read-AppxIdentity $package
    $mapped = & node (Join-Path $PSScriptRoot 'release.mjs') package-version $Version
    if ($LASTEXITCODE -or $identity.Name -cne 'OhneB.crosshair-rs' -or $identity.Version -ne $mapped -or $identity.Publisher -cne $env:CROSSHAIR_PACKAGE_PUBLISHER -or $identity.ProcessorArchitecture -cne 'x64') { throw 'Released package identity mismatch.' }
    Write-Output 'Setup, embedded package signatures, version, identity and exact validated bytes verified; no installation occurred.'
} finally {
    if ($locked) { $locked.Dispose() }
    $resolved = [IO.Path]::GetFullPath($temporary)
    if (!$resolved.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe release-verification cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
