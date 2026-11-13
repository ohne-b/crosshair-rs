[CmdletBinding()]
param([Parameter(Mandatory)][string]$ArtifactDirectory)
$ErrorActionPreference = 'Stop'
# This test changes package registration only on the disposable hosted Windows runner.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Deployment acceptance runs only on a disposable GitHub-hosted runner.' }
$administrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (!$administrator.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'The deployment runner must already be elevated.' }
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$build = Get-Content -LiteralPath (Join-Path $ArtifactDirectory 'build.json') -Raw | ConvertFrom-Json
$identity = Read-AppxIdentity (Join-Path $ArtifactDirectory $build.package)
if ($identity.Name -notin @('OhneB.crosshair-rs', 'OhneB.crosshair-rs.Preview')) { throw 'Unexpected test package identity.' }
if (Get-AppxPackage -Name $identity.Name) { throw 'A package already exists; refusing to alter an existing installation.' }
$installed = $null
try {
    $setup = (Resolve-Path -LiteralPath (Join-Path $ArtifactDirectory $build.setup)).Path
    $process = Start-Process -FilePath $setup -ArgumentList '/S' -WindowStyle Hidden -PassThru
    try {
        if (!$process.WaitForExit(180000)) { $process.Kill(); $process.WaitForExit(); throw 'Setup installation timed out.' }
        if ($process.ExitCode) { throw 'Setup could not install its bundled package and dependency.' }
    } finally { $process.Dispose() }
    $installed = Get-AppxPackage -Name $identity.Name
    if (!$installed -or $installed.Version -ne $build.packageVersion -or $installed.Publisher -cne $identity.Publisher -or $installed.Status -ne 'Ok') { throw 'Windows did not register the expected package.' }
    $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $installed.InstallLocation 'crosshair-engine.exe'), '--build-info')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $engine = [Diagnostics.Process]::Start($start)
    try {
        if (!$engine.WaitForExit(15000)) { $engine.Kill(); $engine.WaitForExit(); throw 'Installed engine did not start.' }
        $info = $engine.StandardOutput.ReadToEnd() | ConvertFrom-Json
        if ($engine.ExitCode -or $info.version -ne $build.version) { throw 'Installed engine version mismatch.' }
    } finally { $engine.Dispose() }
    Write-Output 'Self-contained setup installed the expected MSIX and its dependency; installed engine startup passed.'
} finally {
    $installed = Get-AppxPackage -Name $identity.Name
    if ($installed -and $installed.Publisher -ceq $identity.Publisher) { Remove-AppxPackage -Package $installed.PackageFullName }
}
