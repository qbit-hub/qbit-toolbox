# Deterministic in-memory validation. No downloads, registry reads, native
# executables, drivers, GUI or impersonated hosted-runner environment.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '../../tools/desktop-e2e-driver.psm1') -Force -DisableNameChecking
$spec = Get-Content (Join-Path $PSScriptRoot '../../tools/desktop-e2e-driver.json') -Raw | ConvertFrom-Json
$script:cases = @()
function Check([string]$Name, [scriptblock]$Action) {
    & $Action
    $script:cases += @{name=$Name;status='PASS'}
}
function Equal($Actual, $Expected) { if ($Actual -cne $Expected) { throw "Expected [$Expected], got [$Actual]" } }
function Reject([scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch {
        if ($_.Exception.Message -notmatch '^BLOCKED_(CI_SETUP|ISOLATION|E2E_ENVIRONMENT):') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'Unsafe fixture unexpectedly accepted' }
}
function Driver { return @{path='owned/msedgedriver.exe';version=$spec.version;signature='Valid';signer='CN=Microsoft Corporation, O=Microsoft Corporation, C=US';sha256=$spec.binarySha256} }
function Runtime { return @{version='131.0.2903.86';signature='Valid';signer='CN=Microsoft Corporation, O=Microsoft Corporation, C=US'} }
Check 'legacy MSEdgeDriver output' { Equal (Get-EdgeDriverVersion 'MSEdgeDriver 131.0.2903.86 (abcdef)' 0) $spec.version }
Check 'Microsoft Edge WebDriver output' { Equal (Get-EdgeDriverVersion "Microsoft Edge WebDriver 131.0.2903.86 (abcdef)`r`n" 0) $spec.version }
Check 'case, whitespace and ANSI formatting' { Equal (Get-EdgeDriverVersion "$([char]27)[32mMicrosoft Edge WebDriver 131.0.2903.86$([char]27)[0m`r`n" 0) $spec.version }
Check 'unknown tool output rejected' { Reject { Get-EdgeDriverVersion 'ChromeDriver 131.0.2903.86' 0 } }
Check 'empty version rejected' { Reject { Get-EdgeDriverVersion '' 0 } }
Check 'ambiguous output rejected' { Reject { Get-EdgeDriverVersion "MSEdgeDriver 131.0.2903.86`nMSEdgeDriver 156.0.4314.42" 0 } }
Check 'nonzero probe exit rejected' { Reject { Get-EdgeDriverVersion 'MSEdgeDriver 131.0.2903.86' 1 } }
Check 'oversized native output rejected' { Reject { Get-EdgeDriverVersion ('x' * 4097) 0 } }
Check 'diagnostics bounded and control sanitized' {
    $value = ConvertTo-DriverDiagnostic ("$([char]0)$([char]27)[31m" + ('a' * 9000))
    if ($value.Length -gt 4096 -or $value -match '[\x00-\x1f\x7f]') { throw 'Unsafe diagnostic' }
}
Check 'first three components match with different patch' { Assert-CompatibleDriver '131.0.2903.112' $spec.version }
Check 'verified 131 versus 156 mismatch rejected' { Reject { Assert-CompatibleDriver '131.0.2903.86' '156.0.4314.42' } }
Check 'malformed runtime rejected' { Reject { Assert-CompatibleDriver '131.0.2903' $spec.version } }
Check 'pinned spec accepted' { Assert-DriverSpec $spec }
Check 'unofficial acquisition rejected' {
    $other = $spec | ConvertTo-Json | ConvertFrom-Json; $other.url='https://example.com/driver.zip'
    Reject { Assert-DriverSpec $other }
}
Check 'invalid archive digest rejected' { Reject { Assert-ArchiveDigest ('0' * 64) $spec.archiveSha256 } }
Check 'pinned archive digest accepted' { Assert-ArchiveDigest $spec.archiveSha256 $spec.archiveSha256 }
Check 'valid Microsoft publisher accepted' { Assert-MicrosoftBinary (Driver) }
Check 'invalid signature rejected' { $other=Driver; $other.signature='HashMismatch'; Reject { Assert-MicrosoftBinary $other } }
Check 'unsigned binary rejected' { $other=Driver; $other.signature='NotSigned'; $other.signer=''; Reject { Assert-MicrosoftBinary $other } }
Check 'lookalike publisher rejected' { $other=Driver; $other.signer='O=Microsoft Corporation Fake'; Reject { Assert-MicrosoftBinary $other } }
Check 'null binary rejected' { Reject { Assert-MicrosoftBinary $null } }
Check 'missing tool rejected before signature/probe' { Reject { Get-MicrosoftBinaryRecord (Join-Path $PSScriptRoot 'does-not-exist/msedgedriver.exe') } }
Check 'trusted source-pinned selection accepted' { Assert-DriverSelection (Runtime) (Driver) $spec 'owned/msedgedriver.exe' }
Check 'tampered binary digest rejected' { $other=Driver; $other.sha256='0' * 64; Reject { Assert-DriverSelection (Runtime) $other $spec 'owned/msedgedriver.exe' } }
Check 'global driver fallback rejected' { $other=Driver; $other.path='global/msedgedriver.exe'; Reject { Assert-DriverSelection (Runtime) $other $spec 'owned/msedgedriver.exe' } }
Check 'wrong pinned file version rejected' { $other=Driver; $other.version='131.0.2903.112'; Reject { Assert-DriverSelection (Runtime) $other $spec 'owned/msedgedriver.exe' } }
Check 'unsigned runtime rejected' { $other=Runtime; $other.signature='NotSigned'; Reject { Assert-DriverSelection $other (Driver) $spec 'owned/msedgedriver.exe' } }
Check 'workstation and self-hosted authorization rejected' {
    Reject { Assert-DesktopCiAuthorization @{GITHUB_ACTIONS='';RUNNER_ENVIRONMENT='';QBIT_E2E_ISOLATED_DESKTOP=''} }
    Reject { Assert-DesktopCiAuthorization @{GITHUB_ACTIONS='true';RUNNER_ENVIRONMENT='self-hosted';QBIT_E2E_ISOLATED_DESKTOP='1'} }
}
Check 'path traversal outside private root rejected' { Reject { Assert-ConfinedPath (Join-Path $PSScriptRoot '../../outside') (Join-Path $PSScriptRoot '../../.ai-bridge/e2e-tools') } }
ConvertTo-Json -InputObject $script:cases -Compress
