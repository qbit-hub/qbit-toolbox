param([Parameter(Mandatory=$true)][string]$ReportDirectory, [switch]$Configure)
$ErrorActionPreference = 'Stop'
$report = [IO.Path]::GetFullPath($ReportDirectory)
$allowed = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../.ai-bridge/desktop-lifecycle'))
if (-not $report.StartsWith($allowed + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'BLOCKED_ISOLATION: diagnostic report path is outside the owned artifact root' }
New-Item -ItemType Directory -Path $report -Force | Out-Null
$evidence = [ordered]@{ schemaVersion=1; status='BLOCKED'; imageOS=$env:ImageOS; imageVersion=$env:ImageVersion; selectedImage=$env:QBIT_E2E_IMAGE; runnerEnvironment=$env:RUNNER_ENVIRONMENT; sourceHead=$null; os=$null; runtime=$null; edgeDriver=$null }
try {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:QBIT_E2E_ISOLATED_DESKTOP -ne '1') { throw 'BLOCKED_E2E_ENVIRONMENT: diagnostics require selected hosted Windows CI' }
    $evidence.sourceHead = (git rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'BLOCKED_CI_SETUP: cannot identify candidate HEAD' }
    $os = Get-CimInstance Win32_OperatingSystem
    $evidence.os = @{ caption=$os.Caption; version=$os.Version; build=$os.BuildNumber; architecture=$os.OSArchitecture }
    $versions = @('HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}') | ForEach-Object {
        if (Test-Path -LiteralPath $_) { (Get-ItemProperty -LiteralPath $_).pv }
    } | Where-Object { $_ -match '^\d+\.\d+\.\d+\.\d+$' } | Sort-Object { [version]$_ } -Descending -Unique
    $version = @($versions) | Select-Object -First 1
    if (-not $version) { throw 'BLOCKED_CI_SETUP: installed WebView2 version is unknown' }
    $evidence.registeredRuntimeVersion = $version
    $runtime = @(${env:ProgramFiles(x86)}, $env:ProgramFiles) | ForEach-Object { Join-Path $_ "Microsoft/EdgeWebView/Application/$version/msedgewebview2.exe" } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if (-not $runtime) { throw 'BLOCKED_CI_SETUP: registered WebView2 binary is unavailable' }
    $driver = (Get-Command msedgedriver.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    foreach ($entry in @(@{name='runtime';path=$runtime},@{name='edgeDriver';path=$driver})) {
        $item = Get-Item -LiteralPath $entry.path
        $signature = Get-AuthenticodeSignature -LiteralPath $entry.path
        $record = @{ path=$item.FullName; version=$item.VersionInfo.ProductVersion; sha256=(Get-FileHash -LiteralPath $entry.path -Algorithm SHA256).Hash.ToLowerInvariant(); signature=[string]$signature.Status; signer=$signature.SignerCertificate.Subject }
        $evidence[$entry.name] = $record
        if ($signature.Status -ne 'Valid' -or $record.signer -notmatch 'O=Microsoft Corporation') { throw 'BLOCKED_CI_SETUP: runtime/driver is not verified Microsoft-signed software' }
    }
    $driverText = (& $driver --version)
    $driverMatch = [regex]::Match([string]$driverText, 'MSEdgeDriver (\d+\.\d+\.\d+\.\d+)')
    if ($LASTEXITCODE -ne 0 -or -not $driverMatch.Success) { throw 'BLOCKED_CI_SETUP: driver version probe failed' }
    $driverVersion = $driverMatch.Groups[1].Value
    $evidence.edgeDriver.version = $driverVersion
    if ([regex]::Match([string]$evidence.runtime.version, '^\d+\.\d+\.\d+\.\d+').Value -ne $version) { throw 'BLOCKED_CI_SETUP: registered and executable WebView2 versions disagree' }
    if (($driverVersion.Split('.')[0..2] -join '.') -ne ($version.Split('.')[0..2] -join '.')) { throw 'BLOCKED_CI_SETUP: installed WebView2/EdgeDriver build mismatch; no automatic download or replacement is permitted' }
    $evidence.runtime.version = $version
    $evidence.status = 'PASS'
    if ($Configure) {
        if (-not $env:GITHUB_ENV) { throw 'BLOCKED_CI_SETUP: no GitHub environment file' }
        @("QBIT_E2E_NATIVE_DRIVER=$driver", "EDGEDRIVER_VERSION=$driverVersion", "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=$([IO.Path]::GetDirectoryName($runtime))") | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    }
} catch {
    $evidence.status = 'BLOCKED'
    $evidence.error = $_.Exception.Message
    throw
} finally {
    $json = $evidence | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText((Join-Path $report 'environment.json'), $json)
    Write-Output $json
}
