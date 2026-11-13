[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $PSScriptRoot) 'target/installer-tests'))
$directory = Join-Path $root ("quoted ' `$ path-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$package = Join-Path $directory 'package.msix'
$installer = Join-Path $directory 'Install.ps1'
$alias = Join-Path $root ('junction-' + [Guid]::NewGuid().ToString('N'))
$publisher = 'CN=fixture, OID.2.25.311729368913984317654407730594956997722=1'
function Write-Package([string]$version = '0.1.0.1') {
    if (Test-Path -LiteralPath $package) { Remove-Item -LiteralPath $package }
    $archive = [IO.Compression.ZipFile]::Open($package, 'Create')
    try {
        $writer = [IO.StreamWriter]::new($archive.CreateEntry('AppxManifest.xml').Open())
        try { $writer.Write("<Package><Identity Name='OhneB.crosshair-rs' Publisher='$publisher' Version='$version' ProcessorArchitecture='x64'/></Package>") } finally { $writer.Dispose() }
    } finally { $archive.Dispose() }
}
function Write-Installer {
    $hash = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
    $script = $source.Replace('@PACKAGE_NAME@', 'OhneB.crosshair-rs').Replace('@PACKAGE_VERSION@', '0.1.0.1').Replace('@PACKAGE_SHA256@', $hash).Replace('@PACKAGE_PUBLISHER@', $publisher)
    [IO.File]::WriteAllText($installer, $script)
}
function Expect-Rejection([string]$label) {
    $rejected = $false
    try { & $installer -VerifyOnly 2>$null | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw "Installer accepted $label." }
}
# Capture the elevation boundary without launching a child or requesting UAC.
function Start-Process {
    param($FilePath, $ArgumentList, $WorkingDirectory, $WindowStyle, [switch]$Wait, [switch]$PassThru, $Verb)
    $global:crosshairInstallerInvocation = $PSBoundParameters
    [PSCustomObject]@{ ExitCode=0 }
}
try {
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Destination $directory
    $source = [IO.File]::ReadAllText((Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging/windows/Install.ps1'))
    $source = $source.Replace('# @DEPLOY_SCRIPT@', [IO.File]::ReadAllText((Join-Path (Split-Path -Parent $PSScriptRoot) 'apps/engine/src/updates/deploy.ps1')))
    Write-Package
    Write-Installer
    & $installer -VerifyOnly | Out-Null
    $fixtureSetup = Join-Path $directory 'fixture-setup.exe'
    & (Join-Path $PSScriptRoot 'build-setup.ps1') -Package $package -Framework (Join-Path $directory 'Microsoft.VCLibs.x64.14.00.appx') -Output $fixtureSetup -Version '0.1.0-alpha.1' -Offline | Out-Null
    $fixtureProcess = Microsoft.PowerShell.Management\Start-Process -FilePath $fixtureSetup -ArgumentList '/S /VERIFYONLY' -WindowStyle Hidden -PassThru
    try {
        if (!$fixtureProcess.WaitForExit(60000)) { $fixtureProcess.Kill(); $fixtureProcess.WaitForExit(); throw 'Quoted-path installer verification timed out.' }
        if ($fixtureProcess.ExitCode) { throw 'Installer built from quoted paths failed verification.' }
    } finally { $fixtureProcess.Dispose() }
    New-Item -ItemType Junction -Path $alias -Target $directory | Out-Null
    & (Join-Path $alias 'Install.ps1') | Out-Null
    $invocation = $global:crosshairInstallerInvocation
    if (!$invocation -or $invocation.ArgumentList[2] -ne '-EncodedCommand' -or $invocation.WorkingDirectory -ne [Environment]::GetFolderPath('System')) { throw 'Installer did not transfer captured code with a system working directory.' }
    $command = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($invocation.ArgumentList[3]))
    $tokens=$null; $errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseInput($command,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw 'The elevated command is not valid PowerShell.'}
    $arguments=$ast.EndBlock.Statements[0].PipelineElements[0].CommandElements
    if($arguments[0] -isnot [Management.Automation.Language.ScriptBlockExpressionAst] -or $arguments[2].Value -cne $package -or $arguments[8].Value -cne (Join-Path $directory 'Microsoft.VCLibs.x64.14.00.appx')) { throw 'Elevation altered a literal path or referenced a writable script.' }
    if(!$arguments[0].ScriptBlock.ToString().Contains('Add-AppxPackage @parameters')) { throw 'The shared deployment code was not embedded.' }
    [IO.File]::AppendAllText($package, 'tampered')
    Expect-Rejection 'changed package bytes'
    Write-Package '0.1.0.2'
    Write-Installer
    Expect-Rejection 'a different MSIX version'
    Write-Package
    Write-Installer
    $framework = Join-Path $directory 'Microsoft.VCLibs.x64.14.00.appx'
    [IO.File]::AppendAllText($framework, 'tampered')
    Expect-Rejection 'a changed dependency'
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'Microsoft.VCLibs.x64.14.00.appx') -Destination $framework -Force
    Remove-Item -LiteralPath $package
    $build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
    $setup = Join-Path $directory 'setup.exe'
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory $build.setup) -Destination $setup
    foreach ($mode in '/VERIFYONLY','/EXTRACT','/EXTRACT') {
        $alreadyExtracted = Test-Path -LiteralPath $package
        $process = Microsoft.PowerShell.Management\Start-Process -FilePath $setup -ArgumentList "/S $mode" -WindowStyle Hidden -PassThru
        try {
            if (!$process.WaitForExit(60000)) { $process.Kill(); $process.WaitForExit(); throw 'Setup check timed out.' }
            if ($mode -eq '/EXTRACT' -and $alreadyExtracted) {
                if (!$process.ExitCode) { throw 'Setup overwrote an existing extraction target.' }
            } elseif ($process.ExitCode) { throw "Setup $mode failed." }
        } finally { $process.Dispose() }
    }
    if ((Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant() -cne $build.sha256) { throw 'Setup does not contain the validated MSIX bytes.' }
    Write-Output 'Setup verification, exact-byte extraction, overwrite rejection and captured-code handoff passed. Tampered payload, version and dependency were rejected. No installation or elevation occurred.'
} finally {
    Remove-Variable -Scope Global -Name crosshairInstallerInvocation -ErrorAction SilentlyContinue
    # Remove only the known junction, never traverse it during recursive fixture cleanup.
    if (Test-Path -LiteralPath $alias) { [IO.Directory]::Delete($alias) }
    $resolved = [IO.Path]::GetFullPath($directory)
    if (!$resolved.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
