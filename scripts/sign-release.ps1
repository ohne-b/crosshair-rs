[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory, [Parameter(Mandatory)][string]$OutputDirectory,
      [Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Commit)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:GITHUB_REPOSITORY -ne 'ohne-b/crosshair-rs' -or $env:GITHUB_REF -ne 'refs/heads/main' -or $env:GITHUB_EVENT_NAME -ne 'workflow_dispatch') { throw 'Release signing runs only in the manual canonical-main GitHub workflow.' }
foreach ($name in @('MINISIGN_PRIVATE_KEY', 'CROSSHAIR_UPDATE_PUBLIC_KEY', 'CROSSHAIR_PACKAGE_PUBLISHER')) {
    if (![Environment]::GetEnvironmentVariable($name)) { throw "Required release configuration is missing: $name" }
}
& node (Join-Path $PSScriptRoot 'release.mjs') verify-build $ArtifactDirectory $Version $Commit
if ($LASTEXITCODE) { throw 'Build provenance validation failed.' }
$build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
$source = Join-Path $ArtifactDirectory $build.package
if ((Get-Item -LiteralPath $source).Length -gt 512MB) { throw 'Package exceeds the updater size limit.' }
$identity = Read-AppxIdentity $source
if ($identity.Name -ne 'OhneB.crosshair-rs' -or $identity.Version -ne $build.packageVersion -or $identity.Publisher -ne $env:CROSSHAIR_PACKAGE_PUBLISHER -or $identity.ProcessorArchitecture -ne 'x64') { throw 'Validated package identity differs from release configuration.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Release output must be a new directory.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$setup = Join-Path $OutputDirectory $build.setup
# Both the installer and its embedded MSIX come byte-for-byte from Validation.
Copy-Item -LiteralPath (Join-Path $ArtifactDirectory $build.setup) -Destination $setup
if ((Get-Item -LiteralPath $setup).Length -gt 512MB) { throw 'Setup exceeds the updater size limit.' }
$minisign = & (Join-Path $PSScriptRoot 'get-minisign.ps1')
$privateDirectory = Join-Path $env:RUNNER_TEMP ('crosshair-signing-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $privateDirectory | Out-Null
$key = Join-Path $privateDirectory 'minisign.key'
try {
    [IO.File]::WriteAllText($key, $env:MINISIGN_PRIVATE_KEY.TrimEnd() + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    $setupSignature = Join-Path $privateDirectory 'setup.minisig'
    $packageSignature = Join-Path $privateDirectory 'package.minisig'
    foreach ($item in @(@($setup, $setupSignature), @($source, $packageSignature))) {
        & $minisign -S -W -s $key -m $item[0] -x $item[1] -t "version:$Version" -c 'crosshair-rs release signature'
        if ($LASTEXITCODE) { throw 'Release signing failed; provide the project-specific unencrypted Minisign CI key.' }
        & $minisign -V -H -P $env:CROSSHAIR_UPDATE_PUBLIC_KEY -m $item[0] -x $item[1]
        if ($LASTEXITCODE) { throw 'Release signature does not match the compiled updater key.' }
    }
    & node (Join-Path $PSScriptRoot 'release.mjs') metadata $OutputDirectory $Version $setupSignature $packageSignature
    if ($LASTEXITCODE) { throw 'Release metadata generation failed.' }
    & node (Join-Path $PSScriptRoot 'release.mjs') verify-assets $OutputDirectory $Version
    if ($LASTEXITCODE) { throw 'Release asset verification failed.' }
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -Directory $OutputDirectory -Version $Version -ExpectedPackageHash $build.sha256 -Offline
} finally {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key }
    foreach ($signature in @($setupSignature, $packageSignature)) {
        if ($signature -and (Test-Path -LiteralPath $signature)) { Remove-Item -LiteralPath $signature }
    }
    Remove-Item -LiteralPath $privateDirectory -ErrorAction SilentlyContinue
}
