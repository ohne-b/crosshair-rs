[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Drawing
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$root = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $PSScriptRoot) 'target/installer-tests'))
$directory = Join-Path $root ("quoted ' `$ path-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$package = Join-Path $directory 'package.msix'
$framework = Join-Path $directory 'Microsoft.VCLibs.x64.14.00.appx'
function Run-Check([string]$Executable, [string]$Arguments, [bool]$Success = $true) {
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    try {
        if (!$process.WaitForExit(60000)) { $process.Kill(); $process.WaitForExit(); throw 'Installer check timed out.' }
        if (($process.ExitCode -eq 0) -ne $Success) { throw "Unexpected result for $Arguments (exit $($process.ExitCode))." }
    } finally { $process.Dispose() }
}
try {
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Destination $framework
    $archive = [IO.Compression.ZipFile]::Open($package, 'Create')
    try {
        $entry = $archive.CreateEntry('AppxManifest.xml')
        $writer = [IO.StreamWriter]::new($entry.Open())
        try { $writer.Write("<Package><Identity Name='OhneB.crosshair-rs' Publisher='CN=fixture' Version='0.1.0.1' ProcessorArchitecture='x64'/></Package>") } finally { $writer.Dispose() }
    } finally { $archive.Dispose() }
    $fixture = Join-Path $directory 'fixture.exe'
    & (Join-Path $PSScriptRoot 'build-setup.ps1') -Package $package -Framework $framework -Output $fixture -Version '0.1.0-alpha.1' -Offline | Out-Null
    Run-Check $fixture '/S /VERIFYONLY'
    $helpers = @(Get-ChildItem -LiteralPath $directory -Filter 'setup-*' -Directory | ForEach-Object { Join-Path $_.FullName 'install.exe' })
    if ($helpers.Count -ne 1) { throw 'Expected exactly one fixture helper.' }
    $helper = Join-Path $directory 'install.exe'
    Copy-Item -LiteralPath $helpers[0] -Destination $helper
    Run-Check $helper '--verify'
    $original = [IO.File]::ReadAllBytes($package)
    [IO.File]::AppendAllText($package, 'tampered')
    Run-Check $helper '--verify' $false
    [IO.File]::WriteAllBytes($package, $original)
    [IO.File]::AppendAllText($framework, 'tampered')
    Run-Check $helper '--verify' $false
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Destination $framework -Force
    Run-Check $helper '--verify'
    $wrongVersionRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'build-setup.ps1') -Package $package -Framework $framework -Output (Join-Path $directory 'wrong.exe') -Version '0.1.0-alpha.2' -Offline | Out-Null
    } catch { $wrongVersionRejected = $true }
    if (!$wrongVersionRejected) { throw 'Installer accepted mismatched package metadata.' }
    Remove-Item -LiteralPath $package
    $build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
    $setup = Join-Path $directory 'installer.exe'
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory $build.setup) -Destination $setup
    Run-Check $setup '/S /VERIFYONLY'
    Run-Check $setup '/S /EXTRACT'
    Run-Check $setup '/S /EXTRACT' $false
    if ((Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant() -cne $build.sha256) { throw 'Setup does not contain the validated MSIX bytes.' }
    $archive = [IO.Compression.ZipFile]::OpenRead($package)
    try {
        $reader = [IO.StreamReader]::new($archive.GetEntry('AppxManifest.xml').Open())
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $visuals = $manifest.SelectSingleNode("//*[local-name()='Application' and @Id='Editor']/*[local-name()='VisualElements']")
        if ($visuals.BackgroundColor -ne 'transparent') { throw 'Editor icon background must be transparent.' }
        $pri = Join-Path $directory 'resources.pri'
        $entry = $archive.GetEntry('resources.pri')
        if (!$entry) { throw 'Package resource index is missing.' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $pri)
        $dump = Join-Path $directory 'resources.xml'
        & (Get-WindowsSdkTool 'makepri') dump /if $pri /of $dump /dt detailed /o | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect package resource index.' }
        [xml]$index = Get-Content -LiteralPath $dump -Raw
        foreach ($size in @(16, 24, 32, 48, 256)) {
            foreach ($form in @('UNPLATED', 'LIGHTUNPLATED')) {
                $candidate = $index.SelectSingleNode("//NamedResource[@name='Square44x44Logo.png']/Candidate[QualifierSet/Qualifier[@name='TargetSize' and @value='$size'] and QualifierSet/Qualifier[@name='AlternateForm' and @value='$form']]")
                if (!$candidate) { throw "Missing indexed taskbar icon: $size $form" }
                $entry = $archive.GetEntry($candidate.Value.Replace('\', '/'))
                if (!$entry) { throw 'Indexed taskbar icon is missing from the package.' }
                $stream = $entry.Open()
                try {
                    $image = [Drawing.Bitmap]::new($stream)
                    try {
                        if ($image.Width -ne $size -or $image.Height -ne $size -or $image.GetPixel(0, 0).A -ne 0 -or $image.GetPixel([int]($size / 2), [int]($size / 2)).A -ne 0) { throw 'Taskbar icon size or transparency is incorrect.' }
                    } finally { $image.Dispose() }
                } finally { $stream.Dispose() }
            }
        }
    } finally { $archive.Dispose() }
    Write-Output 'Packaged taskbar icon backgrounds, indexed light/dark size variants and PNG transparency passed.'
    Write-Output 'Native setup verification, quoted paths, tampered payload/dependency rejection, version checks, exact extraction and overwrite rejection passed. No installation or elevation occurred.'
} finally {
    $resolved = [IO.Path]::GetFullPath($directory)
    if (!$resolved.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
