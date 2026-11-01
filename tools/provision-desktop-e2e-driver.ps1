param([Parameter(Mandatory=$true)][string]$ReportDirectory)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'desktop-e2e-driver.psm1') -Force -DisableNameChecking
$report = Assert-ConfinedPath $ReportDirectory (Join-Path $PSScriptRoot '../.ai-bridge/desktop-lifecycle')
New-Item -ItemType Directory -Path $report -Force | Out-Null
$evidence = [ordered]@{schemaVersion=1;status='BLOCKED';source=$null;driver=$null}
try {
    Assert-DesktopCiAuthorization @{GITHUB_ACTIONS=$env:GITHUB_ACTIONS;RUNNER_ENVIRONMENT=$env:RUNNER_ENVIRONMENT;QBIT_E2E_ISOLATED_DESKTOP=$env:QBIT_E2E_ISOLATED_DESKTOP}
    if ([Environment]::OSVersion.Platform -ne 'Win32NT' -or -not $env:GITHUB_ENV) { throw 'BLOCKED_E2E_ENVIRONMENT: Windows CI environment file required' }
    $spec = Get-Content (Join-Path $PSScriptRoot 'desktop-e2e-driver.json') -Raw | ConvertFrom-Json
    Assert-DriverSpec $spec
    $evidence.source = $spec
    $toolRoot = Join-Path $PSScriptRoot '../.ai-bridge/e2e-tools'
    $directory = Assert-ConfinedPath (Join-Path $toolRoot "edgedriver/$($spec.version)") $toolRoot
    # No reuse or replacement of a pre-existing driver location, global or local.
    if (Test-Path -LiteralPath $directory) { throw 'BLOCKED_CI_SETUP: private driver directory already exists' }
    New-Item -ItemType Directory -Path $directory | Out-Null
    Add-Type -AssemblyName System.Net.Http
    $handler = [Net.Http.HttpClientHandler]::new(); $handler.AllowAutoRedirect = $false
    $client = [Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(90); $client.MaxResponseContentBufferSize = 33554432
    try {
        $response = $client.GetAsync($spec.url).GetAwaiter().GetResult()
        try {
            if ([int]$response.StatusCode -ne 200 -or $response.RequestMessage.RequestUri.AbsoluteUri -cne $spec.url) { throw 'BLOCKED_CI_SETUP: official driver download failed or redirected' }
            $bytes = $response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
            if ($bytes.Length -gt 33554432) { throw 'BLOCKED_CI_SETUP: oversized driver archive' }
            $archive = Join-Path $directory 'edgedriver_win64.zip'
            [IO.File]::WriteAllBytes($archive, $bytes)
        } finally { $response.Dispose() }
    } finally { $client.Dispose(); $handler.Dispose() }
    $archiveDigest = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-ArchiveDigest $archiveDigest $spec.archiveSha256
    $evidence.archiveSha256 = $archiveDigest
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $entries = @($zip.Entries | Where-Object FullName -CEQ 'msedgedriver.exe')
        if ($entries.Count -ne 1 -or $entries[0].Length -gt 33554432 -or $entries[0].Length -le 0) { throw 'BLOCKED_CI_SETUP: missing, duplicate or oversized driver entry' }
        # Extract only the exact root entry; never expand arbitrary archive paths.
        $driver = Join-Path $directory 'msedgedriver.exe'
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $driver)
    } finally { $zip.Dispose() }
    $record = Get-MicrosoftBinaryRecord $driver
    if ($record.version -cne $spec.version -or $record.sha256 -cne $spec.binarySha256) { throw 'BLOCKED_CI_SETUP: provisioned driver version/SHA256 mismatch' }
    $evidence.driver = $record
    "QBIT_E2E_NATIVE_DRIVER=$driver" | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    $evidence.status = 'PASS'
} catch {
    $evidence.error = ConvertTo-DriverDiagnostic $_.Exception.Message
    throw
} finally {
    $json = $evidence | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText((Join-Path $report 'provisioning.json'), $json)
    Write-Output $json
}
