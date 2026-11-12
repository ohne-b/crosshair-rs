$ErrorActionPreference = 'Stop'

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
