[CmdletBinding()]
param([switch]$VerifyOnly)
$ErrorActionPreference = 'Stop'
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64' -or [Environment]::OSVersion.Version.Build -lt 22000) { throw 'This release requires x64 Windows 11.' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$packageName = '@PACKAGE_NAME@'
$packageVersion = '@PACKAGE_VERSION@'
$expectedHash = '@PACKAGE_SHA256@'
$publisher = '@PACKAGE_PUBLISHER@'
$deployment = {
    param($Package, $ExpectedHash, $ExpectedUser, $Framework)
    # @DEPLOY_SCRIPT@
}
if ($expectedHash -notmatch '^[a-f0-9]{64}$') { throw 'The installer payload hash is missing.' }
if (!('CrosshairReleaseFilePath' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CrosshairReleaseFilePath {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern uint GetFinalPathNameByHandleW(SafeFileHandle file, StringBuilder path, uint capacity, uint flags);
}
'@
}
function Lock-ReleaseFile([string]$Path) {
    $file = [IO.File]::Open($Path, 'Open', 'Read', 'Read')
    $locks.Add($file)
    $size = [CrosshairReleaseFilePath]::GetFinalPathNameByHandleW($file.SafeFileHandle, $null, 0, 0)
    if (!$size -or $size -gt 32768) { throw 'Cannot resolve the opened release file.' }
    $buffer = [Text.StringBuilder]::new([int]$size)
    $length = [CrosshairReleaseFilePath]::GetFinalPathNameByHandleW($file.SafeFileHandle, $buffer, $size, 0)
    if (!$length -or $length -ge $size) { throw 'The opened release path changed.' }
    $final = $buffer.ToString()
    if (!$final.StartsWith('\\?\') -or $final.Length -lt 7 -or $final[5] -ne ':' -or $final[6] -ne '\') { throw 'Copy the release download to a local Windows drive before installing.' }
    return $final.Substring(4)
}
$Package = Join-Path $PSScriptRoot 'package.msix'
$framework = Join-Path $PSScriptRoot 'Microsoft.VCLibs.x64.14.00.appx'
$locks = [Collections.Generic.List[IO.FileStream]]::new()
try {
    # Bind every later path-based read to its opened file, resolving junction ancestors.
    $Package = Lock-ReleaseFile $Package
    $framework = Lock-ReleaseFile $framework
    if ((Get-FileHash -LiteralPath $Package -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expectedHash) { throw 'The application package is damaged. Download the installer again.' }
    if ((Get-FileHash -LiteralPath $framework -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'The bundled Microsoft x64 framework is damaged. Download the installer again.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Package)
    try {
        $entry = $archive.GetEntry('AppxManifest.xml')
        if (!$entry -or $entry.Length -gt 65536) { throw 'Invalid Windows package manifest.' }
        $reader = [IO.StreamReader]::new($entry.Open())
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $packageIdentity = $manifest.Package.Identity
        if ($packageIdentity.Name -cne $packageName -or $packageIdentity.Publisher -cne $publisher -or $packageIdentity.Version -ne $packageVersion -or $packageIdentity.ProcessorArchitecture -ne 'x64') { throw 'The Windows package identity does not match this release.' }
    } finally { $archive.Dispose() }
    if ($VerifyOnly) { Write-Output 'Embedded package hash, version, identity and dependency verified.'; return }
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
