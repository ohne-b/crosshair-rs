[CmdletBinding()]
param([Parameter(Mandatory)][string]$Package, [Parameter(Mandatory)][string]$Framework,
      [Parameter(Mandatory)][string]$Output, [Parameter(Mandatory)][string]$Version, [switch]$Offline)
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$repoRoot = Split-Path -Parent $PSScriptRoot
$Package = (Resolve-Path -LiteralPath $Package).Path
$Framework = (Resolve-Path -LiteralPath $Framework).Path
$Output = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $Output) { throw 'Setup output already exists.' }
$packageVersion = & node (Join-Path $PSScriptRoot 'release.mjs') package-version $Version
if ($LASTEXITCODE) { throw 'Invalid installer version.' }
$identity = Read-AppxIdentity $Package
if ($identity.Name -notin @('OhneB.crosshair-rs', 'OhneB.crosshair-rs.Preview') -or $identity.Version -ne $packageVersion -or $identity.ProcessorArchitecture -ne 'x64') { throw 'Installer payload identity does not match the requested version.' }
$hash = (Get-FileHash -LiteralPath $Package -Algorithm SHA256).Hash.ToLowerInvariant()
$stage = Join-Path (Split-Path -Parent $Output) ('setup-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
$script = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/Install.ps1') -Raw
$script = $script.Replace('@PACKAGE_NAME@', $identity.Name).Replace('@PACKAGE_VERSION@', $packageVersion).Replace('@PACKAGE_SHA256@', $hash).Replace('@PACKAGE_PUBLISHER@', $identity.Publisher.Replace("'", "''"))
$script = $script.Replace('# @DEPLOY_SCRIPT@', [IO.File]::ReadAllText((Join-Path $repoRoot 'apps/engine/src/updates/deploy.ps1')))
$installer = Join-Path $stage 'Install.ps1'
[IO.File]::WriteAllText($installer, $script, [Text.UTF8Encoding]::new($false))
$compiler = & (Join-Path $PSScriptRoot 'get-nsis.ps1') -Offline:$Offline
$definitions = [ordered]@{ SETUP_OUT=$Output; PAYLOAD=$Package; FRAMEWORK=$Framework; INSTALL_SCRIPT=$installer; VERSION=$Version; PACKAGE_VERSION=$packageVersion }
$config = foreach ($entry in $definitions.GetEnumerator()) {
    $literal = [string]$entry.Value
    if ($literal.Contains('"') -or $literal.Contains('${') -or $literal.Contains('$%') -or $literal.Contains("`r") -or $literal.Contains("`n")) { throw 'Unsupported characters in an installer build path.' }
    '!define ' + $entry.Key + ' "' + $literal + '"'
}
$configPath = Join-Path $stage 'config.nsh'
[IO.File]::WriteAllLines($configPath, $config, [Text.UTF8Encoding]::new($false))
& $compiler /V2 "/DCONFIG=$configPath" (Join-Path $repoRoot 'packaging/windows/setup.nsi')
if ($LASTEXITCODE -or !(Test-Path -LiteralPath $Output -PathType Leaf)) { throw 'Windows setup build failed.' }
Write-Output $Output
