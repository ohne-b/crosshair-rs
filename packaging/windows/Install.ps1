[CmdletBinding()]
param([string]$Package, [switch]$VerifyOnly, [string]$ExpectedUser)
$ErrorActionPreference = 'Stop'
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64' -or [Environment]::OSVersion.Version.Build -lt 22000) { throw 'This release requires x64 Windows 11.' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if ($ExpectedUser -and $identity.User.Value -ne $ExpectedUser) { throw 'Install using the same Windows account that requested installation.' }
$publicKey = '@UPDATE_PUBLIC_KEY@'
$publisher = '@PACKAGE_PUBLISHER@'
$deployment = {
    param($Package, $ExpectedHash, $ExpectedUser, $Framework)
    # @DEPLOY_SCRIPT@
}
if ($publicKey -notmatch '^[A-Za-z0-9+/]{56}$') { throw 'This development package has no release verification key.' }
if (!$Package) {
    $files = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter 'crosshair-rs-*-windows-x64.msix' -File)
    if ($files.Count -ne 1) { throw 'Place exactly one release MSIX beside this script, or pass -Package.' }
    $Package = $files[0].FullName
}
$Package = (Resolve-Path -LiteralPath $Package).Path
$version = [regex]::Match([IO.Path]::GetFileName($Package), '^crosshair-rs-(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta|rc)\.(0|[1-9][0-9]*))?-windows-x64\.msix$')
if (!$version.Success) { throw 'Expected a crosshair-rs release package.' }
$parts = @(1,2,3 | ForEach-Object { [uint32]$version.Groups[$_].Value })
if (@($parts | Where-Object { $_ -gt 65535 }).Count) { throw 'Package version exceeds Windows limits.' }
$releaseVersion = "$($parts[0]).$($parts[1]).$($parts[2])"
$revision = 65535
if ($version.Groups[4].Success) {
    $number = [uint32]$version.Groups[5].Value
    if ($number -ge 20000) { throw 'Invalid prerelease version.' }
    $stage = $version.Groups[4].Value
    $revision = @{alpha=0; beta=20000; rc=40000}[$stage] + $number
    $releaseVersion += "-$stage.$number"
}
$packageVersion = "$($parts[0]).$($parts[1]).$($parts[2]).$revision"
$framework = Join-Path $PSScriptRoot 'Microsoft.VCLibs.x64.14.00.appx'
$minisign = Join-Path $PSScriptRoot 'minisign.exe'
$locks = [Collections.Generic.List[IO.FileStream]]::new()
try {
    foreach ($file in @($Package, "$Package.minisig", $minisign, $framework)) {
        $locks.Add([IO.File]::Open($file, 'Open', 'Read', 'Read'))
    }
    if ((Get-FileHash -LiteralPath $framework -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'The bundled Microsoft x64 framework is damaged. Download the complete release.' }
    if ((Get-FileHash -LiteralPath $minisign -Algorithm SHA256).Hash.ToLowerInvariant() -ne '5535be9e4e123831ebe6ef324aafe9dde507015c176191f9e20c3ad60567f9e1') { throw 'The release verifier is damaged. Download the complete release.' }
    & $minisign -V -H -P $publicKey -m $Package
    if ($LASTEXITCODE) { throw 'The package was not signed by the crosshair-rs updater key.' }
    $signature = (Get-Content -LiteralPath "$Package.minisig" -Raw).TrimEnd() -split '\r?\n'
    if ($signature.Count -ne 4 -or $signature[2] -cne "trusted comment: version:$releaseVersion") { throw 'The signed version does not match the download name.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Package)
    try {
        $entry = $archive.GetEntry('AppxManifest.xml')
        if (!$entry -or $entry.Length -gt 65536) { throw 'Invalid Windows package manifest.' }
        $reader = [IO.StreamReader]::new($entry.Open())
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $packageIdentity = $manifest.Package.Identity
        if ($packageIdentity.Name -cne 'OhneB.crosshair-rs' -or $packageIdentity.Publisher -cne $publisher -or $packageIdentity.Version -ne $packageVersion -or $packageIdentity.ProcessorArchitecture -ne 'x64') { throw 'The Windows package identity does not match this release.' }
    } finally { $archive.Dispose() }
    if ($VerifyOnly) { Write-Output 'Release signature, version, identity and dependencies verified.'; return }
    $administrator = [Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $hash = (Get-FileHash -LiteralPath $Package -Algorithm SHA256).Hash.ToLowerInvariant()
    # Transfer the already parsed deployment code, never reopen a writable script after elevation.
    $command = "& { $deployment } -Package '" + $Package.Replace("'", "''") + "' -ExpectedHash '$hash' -ExpectedUser '" + $identity.User.Value + "' -Framework '" + $framework.Replace("'", "''") + "'"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $system = [Environment]::GetFolderPath('System')
    $start = @{ FilePath=(Join-Path $system 'WindowsPowerShell/v1.0/powershell.exe'); ArgumentList=@('-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded); WorkingDirectory=$system; WindowStyle='Hidden'; Wait=$true; PassThru=$true }
    if (!$administrator) { $start.Verb = 'RunAs' }
    $process = Start-Process @start
    if ($process.ExitCode) { throw 'Installation was cancelled or failed. The existing application was preserved.' }
    Write-Output 'crosshair-rs installed. Open it from Start.'
} finally {
    foreach ($locked in $locks) { $locked.Dispose() }
}
