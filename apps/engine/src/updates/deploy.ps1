# Embedded in the updater and installer; inputs are quoted literals, never executable paths.
$ErrorActionPreference = 'Stop'
$directory = $null
$locked = $null
$frameworkLock = $null
$exitCode = 0
try {
    if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ExpectedUser) { throw 'Use the same Windows account to approve the update.' }
    $administrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (!$administrator.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Unsigned MSIX updates require administrator approval.' }
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    $directory = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) ('crosshair-rs-update-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory, $acl) | Out-Null
    $copy = Join-Path $directory 'package.msix'
    [IO.File]::Copy($Package, $copy, $false)
    $locked = [IO.File]::Open($copy, 'Open', 'Read', 'Read')
    if ((Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedHash) { throw 'The verified package changed before deployment.' }
    $parameters = @{ Path=$copy; AllowUnsigned=$true; ErrorAction='Stop' }
    if ($Framework) {
        $frameworkCopy = Join-Path $directory 'framework.appx'
        [IO.File]::Copy($Framework, $frameworkCopy, $false)
        $frameworkLock = [IO.File]::Open($frameworkCopy, 'Open', 'Read', 'Read')
        if ((Get-FileHash -LiteralPath $frameworkCopy -Algorithm SHA256).Hash.ToLowerInvariant() -cne '9c17b521f9d690a1f504da5108ed6eec5669eb3a8fd1331eef43e40d84e74283') { throw 'The verified framework changed before deployment.' }
        $parameters.DependencyPath = $frameworkCopy
    }
    # The elevated operation consumes only this verified copy in an administrator-owned directory.
    Add-AppxPackage @parameters
} catch {
    $exitCode = $_.Exception.HResult
    if (!$exitCode) { $exitCode = 1 }
    Write-Error -Message $_ -ErrorAction Continue
} finally {
    if ($locked) { $locked.Dispose() }
    if ($frameworkLock) { $frameworkLock.Dispose() }
    if ($directory) {
        foreach ($name in @('package.msix', 'framework.appx')) {
            $copy = Join-Path $directory $name
            if (Test-Path -LiteralPath $copy) { Remove-Item -LiteralPath $copy -ErrorAction SilentlyContinue }
        }
        Remove-Item -LiteralPath $directory -ErrorAction SilentlyContinue
    }
}
exit $exitCode
