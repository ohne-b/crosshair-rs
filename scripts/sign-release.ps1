[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory, [Parameter(Mandatory)][string]$OutputDirectory,
      [Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Commit)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
# Private-key import is confined to an explicitly dispatched ephemeral runner.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:GITHUB_REPOSITORY -ne 'ohne-b/crosshair-rs' -or $env:GITHUB_REF -ne 'refs/heads/main' -or $env:GITHUB_EVENT_NAME -ne 'workflow_dispatch') { throw 'Release signing runs only in the manual canonical-main GitHub workflow.' }
foreach ($name in @('WINDOWS_CERTIFICATE_PFX_BASE64', 'WINDOWS_CERTIFICATE_PASSWORD', 'MINISIGN_PRIVATE_KEY', 'CROSSHAIR_UPDATE_PUBLIC_KEY', 'CROSSHAIR_PACKAGE_PUBLISHER')) {
    if (![Environment]::GetEnvironmentVariable($name)) { throw "Required release configuration is missing: $name" }
}
& node (Join-Path $PSScriptRoot 'release.mjs') verify-build $ArtifactDirectory $Version $Commit
if ($LASTEXITCODE -ne 0) { throw 'Build provenance validation failed.' }
$build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
$source = Join-Path $ArtifactDirectory $build.package
if ((Get-Item -LiteralPath $source).Length -gt 512MB) { throw 'Package exceeds the updater size limit.' }
$identity = Read-AppxIdentity $source
if ($identity.Name -ne 'OhneB.crosshair-rs' -or $identity.Version -ne "$Version.0" -or $identity.Publisher -ne $env:CROSSHAIR_PACKAGE_PUBLISHER -or $identity.ProcessorArchitecture -ne 'x64') { throw 'Validated package identity differs from release configuration.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Release output must be a new directory.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$package = Join-Path $OutputDirectory "crosshair-rs-$Version-windows-x64.msix"
Copy-Item -LiteralPath $source -Destination $package
Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Install.ps1') -Destination $OutputDirectory
Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Destination $OutputDirectory
if ((Get-FileHash -LiteralPath (Join-Path $OutputDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'Microsoft framework hash mismatch.' }
$signTool = Get-WindowsSdkTool 'signtool'
$minisign = & (Join-Path $PSScriptRoot 'get-minisign.ps1')
$privateDirectory = Join-Path $env:RUNNER_TEMP ('crosshair-signing-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $privateDirectory | Out-Null
$pfx = Join-Path $privateDirectory 'publisher.pfx'
$key = Join-Path $privateDirectory 'minisign.key'
$existing = @(Get-ChildItem Cert:\CurrentUser\My | ForEach-Object Thumbprint)
$imported = @()
try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE_PFX_BASE64))
    $password = ConvertTo-SecureString $env:WINDOWS_CERTIFICATE_PASSWORD -AsPlainText -Force
    $imported = @(Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\CurrentUser\My -Password $password)
    $certificate = @($imported | Where-Object { $_.HasPrivateKey -and $_.Subject -eq $env:CROSSHAIR_PACKAGE_PUBLISHER })
    if ($certificate.Count -ne 1 -or $certificate[0].NotAfter -le [DateTime]::UtcNow -or $certificate[0].NotBefore -gt [DateTime]::UtcNow -or '1.3.6.1.5.5.7.3.3' -notin @($certificate[0].EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value })) { throw 'A current code-signing certificate with the exact configured publisher is required.' }
    & $signTool sign /fd SHA256 /sha1 $certificate[0].Thumbprint /tr https://timestamp.digicert.com /td SHA256 $package
    if ($LASTEXITCODE -ne 0) { throw 'Timestamped MSIX signing failed.' }
    & $signTool verify /pa /all /v $package
    if ($LASTEXITCODE -ne 0) { throw 'Windows did not trust the signed package.' }
    [IO.File]::WriteAllText($key, $env:MINISIGN_PRIVATE_KEY.TrimEnd() + "`n", [Text.UTF8Encoding]::new($false))
    # The environment secret holds an unencrypted CI-specific key; -W prevents prompts.
    & $minisign -S -W -s $key -m $package -t "version:$Version" -c 'crosshair-rs release signature'
    if ($LASTEXITCODE -ne 0) { throw 'Release signature failed; provide an unencrypted project-specific Minisign CI key.' }
    & $minisign -V -H -P $env:CROSSHAIR_UPDATE_PUBLIC_KEY -m $package
    if ($LASTEXITCODE -ne 0) { throw 'Release signature does not match the compiled updater key.' }
    & node (Join-Path $PSScriptRoot 'release.mjs') metadata $OutputDirectory $Version
    if ($LASTEXITCODE -ne 0) { throw 'Release metadata generation failed.' }
    & node (Join-Path $PSScriptRoot 'release.mjs') verify-assets $OutputDirectory $Version
    if ($LASTEXITCODE -ne 0) { throw 'Release asset verification failed.' }
} finally {
    foreach ($certificate in $imported) {
        if ($certificate.Thumbprint -notin $existing) { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -ErrorAction SilentlyContinue }
    }
    foreach ($file in @($pfx, $key)) { if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file } }
    Remove-Item -LiteralPath $privateDirectory -ErrorAction SilentlyContinue
}
