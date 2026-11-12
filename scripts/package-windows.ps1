[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [string]$BinaryDirectory,
    [string]$OutputDirectory,
    [string]$Publisher = $(if ($env:CROSSHAIR_PACKAGE_PUBLISHER) { $env:CROSSHAIR_PACKAGE_PUBLISHER } else { 'CN=ohne-b, OID.2.25.311729368913984317654407730594956997722=1' }),
    [switch]$ReleaseCandidate,
    [switch]$SkipBuild,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$repoRoot = Split-Path -Parent $PSScriptRoot
$packageRoot = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else { Join-Path $repoRoot 'target/packages' }
New-Item -ItemType Directory -Force $packageRoot | Out-Null
$cargo = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
$packageSection = [regex]::Match($cargo, '(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\z)').Groups[1].Value
$productVersion = [regex]::Match($packageSection, '(?m)^version\s*=\s*"([^"\r\n]+)"\s*$').Groups[1].Value
if ($Publisher -notmatch ', OID\.2\.25\.311729368913984317654407730594956997722=1$') { throw 'The publisher must use the Windows unsigned-package namespace.' }
$msixVersion = & node (Join-Path $PSScriptRoot 'release.mjs') package-version $productVersion
if ($LASTEXITCODE) { throw 'The workspace version cannot be mapped to MSIX.' }
if ($ReleaseCandidate -and (!$env:CROSSHAIR_UPDATE_PUBLIC_KEY -or !$env:CROSSHAIR_PACKAGE_PUBLISHER -or $Publisher -ne $env:CROSSHAIR_PACKAGE_PUBLISHER)) { throw 'Release candidates require the configured update public key and exact package publisher.' }
if (!$ReleaseCandidate -and ($env:CROSSHAIR_UPDATE_PUBLIC_KEY -or $env:CROSSHAIR_PACKAGE_PUBLISHER)) { throw 'Clear production build variables for development packages, or explicitly select ReleaseCandidate.' }
if (!$BinaryDirectory) { $BinaryDirectory = Join-Path $repoRoot $(if ($Configuration -eq 'Release') { 'target/release' } else { 'target/debug' }) }
$profilePath = [Environment]::GetFolderPath('UserProfile')
$cargoPath = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $profilePath '.cargo' }
$rustupPath = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $profilePath '.rustup' }
$pathMappings = @(@($profilePath, '/build-user'), @($cargoPath, '/cargo'), @($rustupPath, '/rustup'), @($repoRoot, '.'))
if (!$SkipBuild) {
    Push-Location $repoRoot
    $previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
    try {
        $flags = @(if ($previousFlags) { $previousFlags.Split([char]31) } elseif ($env:RUSTFLAGS) { $env:RUSTFLAGS -split '\s+' | Where-Object { $_ } })
        foreach ($mapping in $pathMappings) {
            $path = [IO.Path]::GetFullPath($mapping[0]).TrimEnd('\')
            $flags += "--remap-path-prefix=$path=$($mapping[1])"
            $flags += "--remap-path-prefix=$($path.Replace('\', '/'))=$($mapping[1])"
        }
        $flags += '-Clink-arg=/PDBALTPATH:%_PDB%'
        $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
        $cargoArgs = @('build', '--locked', '--workspace')
        if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
        if ($Offline) { $cargoArgs += '--offline' }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    } finally { $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags; Pop-Location }
    & (Join-Path $PSScriptRoot 'build-gamebar.ps1') -Configuration $Configuration -Test -Offline:$Offline
}
$gamebarOutput = Join-Path $repoRoot "target/gamebar/$($Configuration.ToLowerInvariant())"
$required = @(
    (Join-Path $BinaryDirectory 'crosshair-engine.exe'), (Join-Path $BinaryDirectory 'crosshair-editor.exe'),
    (Join-Path $gamebarOutput 'crosshair-gamebar.exe'), (Join-Path $gamebarOutput 'Microsoft.Gaming.XboxGameBar.dll'),
    (Join-Path $gamebarOutput 'Microsoft.Gaming.XboxGameBar.winmd'), (Join-Path $gamebarOutput 'Microsoft.Gaming.XboxGameBar.Private.winmd'),
    (Join-Path $gamebarOutput 'Microsoft.Gaming.XboxGameBar.pri'), (Join-Path $gamebarOutput 'Microsoft.GameBar.LICENSE.txt')
)
foreach ($file in $required) { if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing package input: $file" } }
foreach ($file in @($required | Where-Object { [IO.Path]::GetExtension($_) -eq '.exe' })) {
    $bytes = [IO.File]::ReadAllBytes($file)
    foreach ($encoding in @([Text.Encoding]::UTF8, [Text.Encoding]::Unicode)) {
        $text = $encoding.GetString($bytes)
        foreach ($mapping in $pathMappings) {
            $path = [IO.Path]::GetFullPath($mapping[0]).TrimEnd('\')
            if ($text.IndexOf($path, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $text.IndexOf($path.Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw "Executable contains a local build path; rebuild through this packaging script: $([IO.Path]::GetFileName($file))" }
        }
    }
}
if (!(Test-Path -LiteralPath (Join-Path $gamebarOutput 'CppWinRT-LICENSE') -PathType Leaf)) { throw 'C++/WinRT license is missing; rebuild the Game Bar host.' }
$start = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath((Join-Path $BinaryDirectory 'crosshair-engine.exe')), '--build-info')
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$process = [Diagnostics.Process]::Start($start)
try {
    if (!$process.WaitForExit(15000)) { $process.Kill(); throw 'Engine build-information command did not complete.' }
    if ($process.ExitCode -ne 0) { throw 'Engine build-information command failed.' }
    $buildConfiguration = $process.StandardOutput.ReadToEnd() | ConvertFrom-Json
} finally { $process.Dispose() }
if ($buildConfiguration.version -ne $productVersion -or $buildConfiguration.packageName -ne 'OhneB.crosshair-rs') { throw 'Engine build does not match the workspace version or package identity.' }
if ($ReleaseCandidate) {
    if ($buildConfiguration.updatePublicKey -notmatch '^[A-Za-z0-9+/]{56}$') { throw 'Invalid updater public key.' }
    if (!$buildConfiguration.updatesConfigured -or $buildConfiguration.updatePublicKey -ne $env:CROSSHAIR_UPDATE_PUBLIC_KEY -or $buildConfiguration.packagePublisher -ne $Publisher) { throw 'Engine was not compiled with the configured release trust. Rebuild before packaging.' }
} elseif ($buildConfiguration.updatesConfigured -or $buildConfiguration.updatePublicKey -or $buildConfiguration.packagePublisher) { throw 'Development packages must contain an updater-disabled engine. Rebuild with production variables unset.' }
$stage = Join-Path $packageRoot ('stage-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
foreach ($file in $required) { Copy-Item -LiteralPath $file -Destination $stage }
Copy-Item -LiteralPath (Join-Path $repoRoot 'packaging/windows/Assets') -Destination $stage -Recurse
Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE.md') -Destination $stage
Copy-Item -LiteralPath (Join-Path $repoRoot 'README.md') -Destination $stage
Copy-Item -LiteralPath (Join-Path $repoRoot 'THIRD_PARTY_NOTICES.md') -Destination $stage
$licenses = Join-Path $stage 'Licenses'
New-Item -ItemType Directory -Path $licenses | Out-Null
Copy-Item -LiteralPath (Join-Path $repoRoot 'assets/fonts/OFL.txt') -Destination (Join-Path $licenses 'Manrope-OFL.txt')
Copy-Item -LiteralPath (Join-Path $repoRoot 'assets/icons/LICENSE') -Destination (Join-Path $licenses 'Tabler-MIT.txt')
Copy-Item -LiteralPath (Join-Path $gamebarOutput 'CppWinRT-LICENSE') -Destination $licenses
& node (Join-Path $PSScriptRoot 'generate-notices.mjs') (Join-Path $licenses 'third-party-rust.md')
if ($LASTEXITCODE -ne 0) { throw 'Dependency license collection failed; no installer was produced.' }
[xml]$manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/AppxManifest.xml') -Raw
$manifest.Package.Identity.SetAttribute('Publisher', $Publisher)
$manifest.Package.Identity.SetAttribute('Version', [string]$msixVersion)
if (!$ReleaseCandidate) { $manifest.Package.Identity.Name += '.Preview' }
$sdkReadme = Get-Content -LiteralPath (Join-Path $repoRoot 'target/gamebar/deps/microsoft.gaming.xboxgamebar.7.3.2607010/readme.txt') -Raw
$proxyXml = [regex]::Match($sdkReadme, '(?s)<Extension Category="windows\.activatableClass\.proxyStub">.*?</Extension>').Value
if (!$proxyXml) { throw 'Pinned SDK proxy registration is missing.' }
$fragment = $manifest.CreateDocumentFragment()
$fragment.InnerXml = $proxyXml.Replace('<Extension ', '<Extension xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" ')
# SDK 7.3.2607010's readme repeats WidgetHost10's IID for NotificationHost.
# This IID matches the pinned Private.winmd and Microsoft's manifest reference.
$notification = $fragment.FirstChild.SelectSingleNode(".//*[local-name()='Interface' and @Name='Microsoft.Gaming.XboxGameBar.Private.IXboxGameBarWidgetNotificationHost']")
if (!$notification) { throw 'Game Bar notification interface registration is missing.' }
$notification.SetAttribute('InterfaceId', '6F68D392-E4A9-46F7-A024-5275BC2FE7BA')
$interfaceIds = @($fragment.FirstChild.SelectNodes(".//*[local-name()='Interface']") | ForEach-Object { $_.InterfaceId })
if (@($interfaceIds | Select-Object -Unique).Count -ne $interfaceIds.Count) { throw 'Duplicate Game Bar interface IDs.' }
[void]$manifest.Package.Extensions.AppendChild($fragment)
$manifest.Save((Join-Path $stage 'AppxManifest.xml'))
$sdkTools = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin/10.0.26100.0/x64'
$makeappx = Join-Path $sdkTools 'makeappx.exe'
if (!(Test-Path -LiteralPath $makeappx)) { throw 'Windows SDK 10.0.26100 packaging tools are required.' }
$packageFile = Join-Path $packageRoot "crosshair-rs-$productVersion-windows-x64.unsigned.msix"
if (Test-Path -LiteralPath $packageFile) { throw "Output already exists; choose a new version or remove the known local build first: $packageFile" }
& $makeappx pack /d $stage /p $packageFile /o /h SHA256
if ($LASTEXITCODE -ne 0) { throw 'MSIX validation or packaging failed.' }
$commit = & git -C $repoRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine package source revision.' }
$dirty = [bool](& git -C $repoRoot status --porcelain)
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine package source state.' }
$hash = (Get-FileHash -LiteralPath $packageFile -Algorithm SHA256).Hash.ToLowerInvariant()
$build = [ordered]@{ schemaVersion=1; version=$productVersion; packageVersion=$msixVersion; commit=$commit; dirty=$dirty; platform='windows-x86_64-msix'; package=[IO.Path]::GetFileName($packageFile); sha256=$hash; signed=$false; releaseCandidate=[bool]$ReleaseCandidate; configuration=$buildConfiguration }
[IO.File]::WriteAllText((Join-Path $packageRoot 'build.json'), ($build | ConvertTo-Json -Depth 4) + "`n", [Text.UTF8Encoding]::new($false))
& (Join-Path $PSScriptRoot 'get-vclibs.ps1') -OutputDirectory $packageRoot -Offline:$Offline | Out-Host
$installer = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/Install.ps1') -Raw
$installer = $installer.Replace('@UPDATE_PUBLIC_KEY@', [string]$buildConfiguration.updatePublicKey).Replace('@PACKAGE_PUBLISHER@', $Publisher.Replace("'", "''"))
$installer = $installer.Replace('# @DEPLOY_SCRIPT@', [IO.File]::ReadAllText((Join-Path $repoRoot 'apps/engine/src/updates/deploy.ps1')))
[IO.File]::WriteAllText((Join-Path $packageRoot 'Install.ps1'), $installer, [Text.UTF8Encoding]::new($false))
$minisign = & (Join-Path $PSScriptRoot 'get-minisign.ps1') -Offline:$Offline
Copy-Item -LiteralPath $minisign -Destination $packageRoot
Copy-Item -LiteralPath (Join-Path $repoRoot 'assets/licenses/Minisign-LICENSE.txt') -Destination $packageRoot
Write-Output "Package: $packageFile"
Write-Output "Stage: $stage"
if ($ReleaseCandidate) { Write-Output 'Unsigned release candidate. Manual publication must verify provenance and add the updater signature.' }
else { Write-Output 'Unsigned .Preview development artifact with updates disabled. No installation or trust settings were changed.' }
