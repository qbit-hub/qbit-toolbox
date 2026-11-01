# Pure validation and bounded probes shared by explicit CI provisioning and the
# read-only pre-launch diagnostic. Importing this module performs no I/O.
Set-StrictMode -Version Latest

function Assert-DesktopCiAuthorization {
    param($Environment)
    if ($Environment.GITHUB_ACTIONS -ne 'true' -or $Environment.RUNNER_ENVIRONMENT -ne 'github-hosted' -or $Environment.QBIT_E2E_ISOLATED_DESKTOP -ne '1') {
        throw 'BLOCKED_E2E_ENVIRONMENT: requires explicitly selected GitHub-hosted Windows CI'
    }
}

function ConvertTo-DriverDiagnostic {
    param([AllowNull()][string]$Text)
    # Never store terminal escapes/control characters or unbounded native output.
    if ($null -eq $Text) { return '' }
    $bounded = $Text.Substring(0, [Math]::Min(4096, $Text.Length))
    return [regex]::Replace([regex]::Replace($bounded, '\x1b\[[0-?]*[ -/]*[@-~]', ''), '[\x00-\x08\x0b-\x1f\x7f]', '')
}

function Get-EdgeDriverVersion {
    param([string]$Output, [int]$ExitCode)
    if ($ExitCode -ne 0 -or $Output.Length -gt 4096) { throw 'BLOCKED_CI_SETUP: driver version probe failed or exceeded output bound' }
    $clean = ConvertTo-DriverDiagnostic $Output
    $versions = [regex]::Matches($clean, '(?im)^\s*(?:MSEdgeDriver|Microsoft Edge WebDriver)\s+(\d+\.\d+\.\d+\.\d+)(?:\s+\([^\r\n]*\))?\s*$')
    if ($versions.Count -ne 1) { throw 'BLOCKED_CI_SETUP: unsupported or ambiguous Microsoft driver version output' }
    return $versions[0].Groups[1].Value
}

function Assert-DriverSpec {
    param($Spec)
    if ($Spec.version -notmatch '^\d+\.\d+\.\d+\.\d+$' -or $Spec.runtimeBuild -ne ($Spec.version.Split('.')[0..2] -join '.') -or
        $Spec.url -cne "https://msedgedriver.microsoft.com/$($Spec.version)/edgedriver_win64.zip" -or
        $Spec.archiveSha256 -cnotmatch '^[a-f0-9]{64}$' -or $Spec.binarySha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'BLOCKED_CI_SETUP: invalid pinned Microsoft driver specification'
    }
}

function Assert-MicrosoftBinary {
    param($Record)
    if ($null -eq $Record -or $Record.signature -ne 'Valid' -or $Record.signer -notmatch '(?:^|,\s*)O=Microsoft Corporation(?:,|$)') {
        throw 'BLOCKED_CI_SETUP: binary lacks a valid Microsoft publisher signature'
    }
}

function Assert-CompatibleDriver {
    param([string]$RuntimeVersion, [string]$DriverVersion)
    if ($RuntimeVersion -notmatch '^\d+\.\d+\.\d+\.\d+$' -or $DriverVersion -notmatch '^\d+\.\d+\.\d+\.\d+$' -or
        ($RuntimeVersion.Split('.')[0..2] -join '.') -ne ($DriverVersion.Split('.')[0..2] -join '.')) {
        throw 'BLOCKED_CI_SETUP: WebView2/EdgeDriver first three version components must match'
    }
}

function Assert-DriverSelection {
    param($Runtime, $Driver, $Spec, [string]$ExpectedPath)
    Assert-DriverSpec $Spec
    Assert-MicrosoftBinary $Runtime
    Assert-MicrosoftBinary $Driver
    Assert-CompatibleDriver $Runtime.version $Driver.version
    if ($Driver.path -ine $ExpectedPath -or $Driver.version -cne $Spec.version -or $Driver.sha256 -cne $Spec.binarySha256 -or
        ($Runtime.version.Split('.')[0..2] -join '.') -ne $Spec.runtimeBuild) {
        throw 'BLOCKED_CI_SETUP: driver path, version or SHA256 differs from source pin'
    }
}

function Assert-ConfinedPath {
    param([string]$Path, [string]$Root)
    $full = [IO.Path]::GetFullPath($Path)
    $base = [IO.Path]::GetFullPath($Root).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if (-not $full.StartsWith($base + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'BLOCKED_ISOLATION: path outside CI-owned root' }
    # Check existing ancestors before creating anything or trusting an existing file.
    $cursor = $full
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'BLOCKED_ISOLATION: reparse point in CI-owned path' }
        }
        $parent = [IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    return $full
}

function Get-MicrosoftBinaryRecord {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'BLOCKED_CI_SETUP: required binary is missing' }
    $item = Get-Item -LiteralPath $Path
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    $subject = if ($null -ne $signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { '' }
    $record = @{path=$item.FullName; version=$item.VersionInfo.ProductVersion; sha256=(Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant(); signature=[string]$signature.Status; signer=$subject}
    Assert-MicrosoftBinary $record
    return $record
}

function Invoke-EdgeDriverVersionProbe {
    param([string]$Path, [hashtable]$Evidence)
    # Caller verifies signature + pinned binary digest before starting this probe.
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Path; $start.Arguments = '--version'; $start.UseShellExecute = $false
    $start.CreateNoWindow = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $child = [Diagnostics.Process]::Start($start)
    try {
        # Read at most 4097 chars from each stream; excess/blocked output times out.
        $outBuffer = [char[]]::new(4097); $errBuffer = [char[]]::new(4097)
        $outTask = $child.StandardOutput.ReadBlockAsync($outBuffer, 0, $outBuffer.Length)
        $errTask = $child.StandardError.ReadBlockAsync($errBuffer, 0, $errBuffer.Length)
        if (-not $child.WaitForExit(10000)) { $Evidence.timedOut = $true; throw 'BLOCKED_CI_SETUP: driver version probe timed out' }
        if (-not $outTask.Wait(1000) -or -not $errTask.Wait(1000)) { throw 'BLOCKED_CI_SETUP: driver output did not close' }
        $stdout = [string]::new($outBuffer, 0, $outTask.Result)
        $stderr = [string]::new($errBuffer, 0, $errTask.Result)
        $Evidence.stdout = ConvertTo-DriverDiagnostic $stdout
        $Evidence.stderr = ConvertTo-DriverDiagnostic $stderr
        $Evidence.exitCode = $child.ExitCode
        $combined = ($stdout + "`n" + $stderr).Trim()
        return @{version=(Get-EdgeDriverVersion $combined $child.ExitCode); exitCode=$child.ExitCode; stdout=(ConvertTo-DriverDiagnostic $stdout); stderr=(ConvertTo-DriverDiagnostic $stderr)}
    } finally {
        if (-not $child.HasExited) { $child.Kill(); [void]$child.WaitForExit(1000) }
        $child.Dispose()
    }
}

function Assert-ArchiveDigest {
    param([string]$Actual, [string]$Expected)
    if ($Actual -cne $Expected -or $Expected -cnotmatch '^[a-f0-9]{64}$') { throw 'BLOCKED_CI_SETUP: driver archive SHA256 mismatch' }
}

Export-ModuleMember -Function Assert-DesktopCiAuthorization, ConvertTo-DriverDiagnostic, Get-EdgeDriverVersion, Assert-DriverSpec, Assert-MicrosoftBinary, Assert-CompatibleDriver, Assert-DriverSelection, Assert-ConfinedPath, Get-MicrosoftBinaryRecord, Invoke-EdgeDriverVersionProbe, Assert-ArchiveDigest
