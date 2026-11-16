$ErrorActionPreference = 'Stop'

function Initialize-X64Compiler {
    # Repeated vcvars calls grow PATH enough to exceed cmd.exe's limit in packaging tests.
    if ($env:VSCMD_ARG_HOST_ARCH -eq 'x64' -and $env:VSCMD_ARG_TGT_ARCH -eq 'x64' -and
        (Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue)) { return }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio C++ build tools and Windows SDK 10.0.26100.' }
    $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$vsPath) { throw 'Visual Studio C++ build tools are required.' }
    $vcvars = Join-Path $vsPath 'VC/Auxiliary/Build/vcvars64.bat'
    $env:Path = (Split-Path -Parent $vswhere) + ';' + $env:Path
    $environmentLines = & $env:ComSpec /d /c ('call "' + $vcvars + '" >nul && set')
    if ($LASTEXITCODE) { throw 'Could not load the x64 compiler environment.' }
    foreach ($line in $environmentLines) {
        if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
    }
}

function Get-WindowsSdkTool([string]$Name) {
    $tool = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin/10.0.26100.0/x64/$Name.exe"
    if (!(Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Windows SDK 10.0.26100 tool is missing: $Name" }
    return $tool
}

function Get-PinnedFile([string]$Uri, [string]$Destination, [string]$Sha256, [switch]$Offline) {
    if (!(Test-Path -LiteralPath $Destination)) {
        if ($Offline) { throw "Required cached dependency is missing: $([IO.Path]::GetFileName($Destination))" }
        $temporary = "$Destination.download"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $temporary -TimeoutSec 60
            if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) { throw 'Downloaded dependency hash mismatch.' }
            Move-Item -LiteralPath $temporary -Destination $Destination
        } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
    }
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) { throw "Cached dependency hash mismatch: $([IO.Path]::GetFileName($Destination))" }
    return $Destination
}

function Read-AppxIdentity([string]$Path) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entry = $archive.GetEntry('AppxManifest.xml')
        if (!$entry -or $entry.Length -gt 65536) { throw 'Missing or oversized Windows package manifest.' }
        $reader = [IO.StreamReader]::new($entry.Open())
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
        return $manifest.Package.Identity
    } finally { $archive.Dispose() }
}
