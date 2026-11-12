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
$package = Join-Path $OutputDirectory "crosshair-rs-$Version-windows-x64.msix"
# Publish the unsigned MSIX byte-for-byte from Validation; only its detached signature is new.
Copy-Item -LiteralPath $source -Destination $package
foreach ($name in @('Install.ps1', 'Microsoft.VCLibs.x64.14.00.appx', 'minisign.exe', 'Minisign-LICENSE.txt')) {
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory $name) -Destination $OutputDirectory
}
$minisign = & (Join-Path $PSScriptRoot 'get-minisign.ps1')
$privateDirectory = Join-Path $env:RUNNER_TEMP ('crosshair-signing-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $privateDirectory | Out-Null
$key = Join-Path $privateDirectory 'minisign.key'
try {
    [IO.File]::WriteAllText($key, $env:MINISIGN_PRIVATE_KEY.TrimEnd() + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    & $minisign -S -W -s $key -m $package -t "version:$Version" -c 'crosshair-rs release signature'
    if ($LASTEXITCODE) { throw 'Release signing failed; provide the project-specific unencrypted Minisign CI key.' }
    & $minisign -V -H -P $env:CROSSHAIR_UPDATE_PUBLIC_KEY -m $package
    if ($LASTEXITCODE) { throw 'Release signature does not match the compiled updater key.' }
    & node (Join-Path $PSScriptRoot 'release.mjs') metadata $OutputDirectory $Version
    if ($LASTEXITCODE) { throw 'Release metadata generation failed.' }
    & node (Join-Path $PSScriptRoot 'release.mjs') verify-assets $OutputDirectory $Version
    if ($LASTEXITCODE) { throw 'Release asset verification failed.' }
    & (Join-Path $OutputDirectory 'Install.ps1') -VerifyOnly
} finally {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key }
    Remove-Item -LiteralPath $privateDirectory -ErrorAction SilentlyContinue
}
