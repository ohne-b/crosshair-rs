[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [switch]$Test,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$buildRoot = Join-Path $repoRoot 'target/gamebar'
$dependencies = Join-Path $buildRoot 'deps'
$generated = Join-Path $buildRoot 'generated'
$output = Join-Path $buildRoot $Configuration.ToLowerInvariant()
New-Item -ItemType Directory -Force $dependencies, $generated, $output | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Restore-PinnedPackage([string]$Name, [string]$Version, [string]$Sha256) {
    $archive = Join-Path $dependencies "$Name.$Version.nupkg"
    if (!(Test-Path -LiteralPath $archive)) {
        if ($Offline) { throw "Missing cached package: $Name $Version" }
        Invoke-WebRequest -UseBasicParsing -Uri "https://api.nuget.org/v3-flatcontainer/$Name/$Version/$Name.$Version.nupkg" -OutFile $archive -TimeoutSec 60
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) { throw "Package hash mismatch: $Name" }
    $directory = Join-Path $dependencies "$Name.$Version"
    if (!(Test-Path -LiteralPath $directory)) { [IO.Compression.ZipFile]::ExtractToDirectory($archive, $directory) }
    return $directory
}

$gamebar = Restore-PinnedPackage 'microsoft.gaming.xboxgamebar' '7.3.2607010' '20230e0a1544ebb391764e13cdbaa1eda86239968444b599fcc9fa1e3d748ff1'
$cppwinrt = Restore-PinnedPackage 'microsoft.windows.cppwinrt' '2.0.250303.1' '955e3051b35db1c00177ed14ab6b7b995c3b32a53fdb6c184ea53b1dba66e439'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio 2022 C++ build tools and Windows SDK 10.0.26100.' }
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vsPath) { throw 'Visual Studio C++ build tools are missing.' }
$env:Path = (Split-Path -Parent $vswhere) + ';' + $env:Path
$vcvars = Join-Path $vsPath 'VC/Auxiliary/Build/vcvars64.bat'
$environmentLines = & $env:ComSpec /d /c ('call "' + $vcvars + '" >nul && set')
if ($LASTEXITCODE -ne 0) { throw 'Could not load the x64 compiler environment.' }
foreach ($line in $environmentLines) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$sdkVersion = '10.0.26100.0'
$metadata = Join-Path $sdk "UnionMetadata/$sdkVersion/Windows.winmd"
if (!(Test-Path -LiteralPath $metadata)) { throw "Windows SDK $sdkVersion is required." }
& (Join-Path $cppwinrt 'bin/cppwinrt.exe') -input (Join-Path $gamebar 'lib/uap10.0/Microsoft.Gaming.XboxGameBar.winmd') -reference $metadata -output $generated
if ($LASTEXITCODE -ne 0) { throw 'Game Bar C++/WinRT projection failed.' }

$source = Join-Path $repoRoot 'apps/gamebar/CrosshairWidget'
$compilerArgs = @('/nologo', '/std:c++20', '/EHsc', '/MT', '/W4', '/DWIN32_LEAN_AND_MEAN', '/DNOMINMAX', '/DUNICODE', '/D_UNICODE', '/I..\generated')
if ($Configuration -eq 'Release') { $compilerArgs += @('/O2', '/DNDEBUG') } else { $compilerArgs += @('/Od', '/Zi') }
Push-Location $output
try {
    $appSource = Resolve-Path -LiteralPath (Join-Path $source 'app.cpp') -Relative
    $pipeSource = Resolve-Path -LiteralPath (Join-Path $source 'pipe.cpp') -Relative
    & cl.exe @compilerArgs $appSource $pipeSource '/Fe:crosshair-gamebar.exe' /link /APPCONTAINER /SUBSYSTEM:WINDOWS /MANIFESTUAC:NO '/PDBALTPATH:%_PDB%' windowsapp.lib advapi32.lib user32.lib
    if ($LASTEXITCODE -ne 0) { throw 'Game Bar host compilation failed.' }
    if ($Test) {
        & cl.exe /nologo /std:c++20 /EHsc /MT /W4 /O2 (Join-Path $repoRoot 'apps/gamebar/tests/wire.cpp') '/Fe:widget-wire-tests.exe'
        if ($LASTEXITCODE -ne 0) { throw 'Widget protocol test build failed.' }
        & (Join-Path $output 'widget-wire-tests.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Widget protocol tests failed.' }
    }
} finally { Pop-Location }
foreach ($name in @('Microsoft.Gaming.XboxGameBar.dll', 'Microsoft.Gaming.XboxGameBar.pri')) {
    Copy-Item -LiteralPath (Join-Path $gamebar "runtimes/win10-x64/native/$name") -Destination $output
}
Copy-Item -LiteralPath (Join-Path $gamebar 'lib/uap10.0/Microsoft.Gaming.XboxGameBar.winmd') -Destination $output
Copy-Item -LiteralPath (Join-Path $gamebar 'private/Microsoft.Gaming.XboxGameBar.Private.winmd') -Destination $output
Copy-Item -LiteralPath (Join-Path $gamebar 'license.txt') -Destination (Join-Path $output 'Microsoft.GameBar.LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $cppwinrt 'LICENSE') -Destination (Join-Path $output 'CppWinRT-LICENSE')
Write-Output "Game Bar host: $output"
