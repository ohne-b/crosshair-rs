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
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vsPath) { throw 'Visual Studio C++ build tools are required.' }
$vcvars = Join-Path $vsPath 'VC/Auxiliary/Build/vcvars64.bat'
$env:Path = (Split-Path -Parent $vswhere) + ';' + $env:Path
$environmentLines = & $env:ComSpec /d /c ('call "' + $vcvars + '" >nul && set')
if ($LASTEXITCODE) { throw 'Could not load the x64 compiler environment.' }
foreach ($line in $environmentLines) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
[IO.File]::WriteAllText((Join-Path $stage 'payload.h'), ('constexpr wchar_t payload_hash[] = L"' + $hash + '";'), [Text.UTF8Encoding]::new($false))
$installer = Join-Path $stage 'install.exe'
Push-Location $stage
try {
    $source = Resolve-Path -LiteralPath (Join-Path $repoRoot 'packaging/windows/install.cpp') -Relative
    & cl.exe /nologo /std:c++20 /EHsc /MT /W4 /WX /O2 /DWIN32_LEAN_AND_MEAN /DNOMINMAX /DUNICODE /D_UNICODE /DWINRT_NO_SOURCE_LOCATION "/I$stage" "/I$(Join-Path $repoRoot 'target/gamebar/generated')" $source /Fe:install.exe /link /SUBSYSTEM:WINDOWS '/MANIFESTUAC:level="asInvoker" uiAccess="false"' /DEPENDENTLOADFLAG:0x800 '/PDBALTPATH:%_PDB%' windowsapp.lib advapi32.lib shell32.lib shlwapi.lib bcrypt.lib user32.lib
    if ($LASTEXITCODE) { throw 'Native installer compilation failed.' }
} finally { Pop-Location }
$compiler = & (Join-Path $PSScriptRoot 'get-nsis.ps1') -Offline:$Offline
$definitions = [ordered]@{ SETUP_OUT=$Output; PAYLOAD=$Package; FRAMEWORK=$Framework; INSTALL_HELPER=$installer; VERSION=$Version; PACKAGE_VERSION=$packageVersion }
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
