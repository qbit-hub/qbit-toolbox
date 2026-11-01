param([Parameter(Mandatory=$true)][string]$ReportDirectory, [switch]$Configure)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'desktop-e2e-driver.psm1') -Force -DisableNameChecking
$report = Assert-ConfinedPath $ReportDirectory (Join-Path $PSScriptRoot '../.ai-bridge/desktop-lifecycle')
New-Item -ItemType Directory -Path $report -Force | Out-Null
$evidence = [ordered]@{ schemaVersion=1; status='BLOCKED'; imageOS=$env:ImageOS; imageVersion=$env:ImageVersion; selectedImage=$env:QBIT_E2E_IMAGE; runnerEnvironment=$env:RUNNER_ENVIRONMENT; sourceHead=$null; os=$null; runtime=$null; edgeDriver=$null }
try {
    Assert-DesktopCiAuthorization @{GITHUB_ACTIONS=$env:GITHUB_ACTIONS;RUNNER_ENVIRONMENT=$env:RUNNER_ENVIRONMENT;QBIT_E2E_ISOLATED_DESKTOP=$env:QBIT_E2E_ISOLATED_DESKTOP}
    $spec = Get-Content (Join-Path $PSScriptRoot 'desktop-e2e-driver.json') -Raw | ConvertFrom-Json
    Assert-DriverSpec $spec
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
    $expectedDriver = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.ai-bridge/e2e-tools/edgedriver/$($spec.version)/msedgedriver.exe"))
    if (-not $env:QBIT_E2E_NATIVE_DRIVER -or [IO.Path]::GetFullPath($env:QBIT_E2E_NATIVE_DRIVER) -ine $expectedDriver) { throw 'BLOCKED_CI_SETUP: explicit private driver is missing or outside its pinned location' }
    $driver = Assert-ConfinedPath $env:QBIT_E2E_NATIVE_DRIVER (Join-Path $PSScriptRoot '../.ai-bridge/e2e-tools')
    foreach ($entry in @(@{name='runtime';path=$runtime},@{name='edgeDriver';path=$driver})) {
        $record = Get-MicrosoftBinaryRecord $entry.path
        $evidence[$entry.name] = $record
    }
    if ([regex]::Match([string]$evidence.runtime.version, '^\d+\.\d+\.\d+\.\d+').Value -ne $version) { throw 'BLOCKED_CI_SETUP: registered and executable WebView2 versions disagree' }
    $evidence.runtime.version = $version
    # Verify publisher + pinned file bytes + compatible build BEFORE execution.
    Assert-DriverSelection $evidence.runtime $evidence.edgeDriver $spec $expectedDriver
    $evidence.driverProbe = @{}
    $probe = Invoke-EdgeDriverVersionProbe $driver $evidence.driverProbe
    if ($probe.version -cne $evidence.edgeDriver.version) { throw 'BLOCKED_CI_SETUP: driver file and command versions disagree' }
    $driverVersion = $probe.version
    $evidence.status = 'PASS'
    if ($Configure) {
        if (-not $env:GITHUB_ENV) { throw 'BLOCKED_CI_SETUP: no GitHub environment file' }
        @("QBIT_E2E_NATIVE_DRIVER=$driver", "EDGEDRIVER_VERSION=$driverVersion", "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=$([IO.Path]::GetDirectoryName($runtime))") | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    }
} catch {
    $evidence.status = 'BLOCKED'
    $evidence.error = ConvertTo-DriverDiagnostic $_.Exception.Message
    throw
} finally {
    $json = $evidence | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText((Join-Path $report 'environment.json'), $json)
    Write-Output $json
}
