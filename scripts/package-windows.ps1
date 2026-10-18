param([string]$Destination, [string]$FFprobePath, [string]$TargetDirectory, [string]$RqbitPath)

$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne 'Win32NT') { throw 'This workflow packages Windows x64 on Windows.' }
$workspaceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $TargetDirectory) { $TargetDirectory = Join-Path $workspaceRoot 'target' }
$TargetDirectory = [IO.Path]::GetFullPath($TargetDirectory)
$projectMetadataText = cargo metadata --manifest-path (Join-Path $workspaceRoot 'Cargo.toml') --locked --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine project version.' }
$projectVersion = (($projectMetadataText | ConvertFrom-Json).packages | Where-Object name -eq 'torrent-search').version
if (-not $projectVersion) { throw 'Project version is missing.' }
if (-not $Destination) { $Destination = Join-Path $workspaceRoot "dist\torrentx-$projectVersion-windows-x64" }
$bundlePath = [IO.Path]::GetFullPath($Destination)
$distRoot = [IO.Path]::GetFullPath((Join-Path $workspaceRoot 'dist'))
if (-not $bundlePath.StartsWith($distRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The bundle must be inside this workspace dist directory.'
}
if ((Test-Path -LiteralPath $bundlePath) -or (Test-Path -LiteralPath ($bundlePath + '.zip'))) { throw "Output already exists: $bundlePath" }

$pythonVersion = '3.14.8'
$pythonUrl = "https://www.python.org/ftp/python/$pythonVersion/python-$pythonVersion-embed-amd64.zip"
# Published on https://www.python.org/downloads/release/python-3148/
$pythonSha256 = 'a93abe456ab01bd96d7a085b3cdb6566b3063f4241360d114142fbdb07f0a310'
$cacheRoot = Join-Path $workspaceRoot '.cache'
$pythonArchive = Join-Path $cacheRoot "python-$pythonVersion-embed-amd64.zip"
$pythonRuntime = Join-Path $cacheRoot "python-$pythonVersion-embed-amd64"
New-Item -ItemType Directory -Path $cacheRoot -Force | Out-Null
if (-not (Test-Path -LiteralPath $pythonArchive)) {
    Invoke-WebRequest -Uri $pythonUrl -OutFile $pythonArchive -TimeoutSec 60
}
if ((Get-FileHash -LiteralPath $pythonArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $pythonSha256) {
    throw "Python archive integrity check failed: $pythonArchive"
}
if (-not (Test-Path -LiteralPath $pythonRuntime)) {
    Expand-Archive -LiteralPath $pythonArchive -DestinationPath $pythonRuntime
    Set-Content -LiteralPath (Join-Path $pythonRuntime '.complete') -Value $pythonSha256 -Encoding ascii
}
if (-not (Test-Path -LiteralPath (Join-Path $pythonRuntime '.complete'))) { throw 'Python extraction is incomplete; inspect the cache before retrying.' }
$pythonExecutable = Join-Path $pythonRuntime 'python.exe'
$rqbitSha256 = '2ed683203beca628e0c45f62f99feeef4242cecc3444c8422dfe5b1b6aa38cea'
if (-not $RqbitPath) { $RqbitPath = Join-Path $cacheRoot 'rqbit-9.0.1\rqbit.exe' }
$RqbitPath = (Resolve-Path -LiteralPath $RqbitPath).Path
if ((Get-FileHash -LiteralPath $RqbitPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $rqbitSha256) { throw 'rqbit v9.0.1 executable integrity check failed.' }
$rqbitLicense = Join-Path $cacheRoot 'rqbit-LICENSE'
if (-not (Test-Path -LiteralPath $rqbitLicense)) { throw 'rqbit upstream LICENSE is required in .cache/rqbit-LICENSE.' }
& $pythonExecutable -I -B -X utf8 -c 'import ssl, json, socket; print("Embedded Python ready")'
if ($LASTEXITCODE -ne 0) { throw 'Embedded Python smoke check failed.' }

$previousTestPython = $env:TORRENT_SEARCH_TEST_PYTHON
$previousTestProbe = $env:TORRENT_SEARCH_TEST_FFPROBE
if (-not $FFprobePath) { $FFprobePath = (Get-Command ffprobe -ErrorAction Stop).Source }
$FFprobePath = (Resolve-Path -LiteralPath $FFprobePath).Path
$probeBin = [IO.Path]::GetDirectoryName($FFprobePath)
$probeRoot = [IO.Path]::GetDirectoryName($probeBin)
foreach ($notice in @('LICENSE', 'README.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $probeRoot $notice))) {
        throw "ffprobe packaging requires upstream LICENSE and README.txt beside its bin directory: $probeRoot"
    }
}
$probeVersion = & $FFprobePath -version
if ($LASTEXITCODE -ne 0) { throw 'ffprobe runtime validation failed.' }
Push-Location -LiteralPath $workspaceRoot
try {
    $env:TORRENT_SEARCH_TEST_PYTHON = $pythonExecutable
    $env:TORRENT_SEARCH_TEST_FFPROBE = $FFprobePath
    cargo test --locked --target-dir $TargetDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Rust checks failed; no bundle created.' }
    cargo build --release --locked --target-dir $TargetDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed; no bundle created.' }
    New-Item -ItemType Directory -Path $bundlePath -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $TargetDirectory 'release\torrentx.exe') -Destination $bundlePath
    # Package the pinned allowlist, never local Jackett/Rutor configuration,
    # cookies, logs, or unreviewed plugin files in the same directory.
    $bundleResources = New-Item -ItemType Directory -Path (Join-Path $bundlePath 'resources\qbit\engines')
    foreach ($file in @('sources.json', 'qbit_worker.py', 'python-dependencies.lock.json')) {
        Copy-Item -LiteralPath (Join-Path $workspaceRoot "resources\$file") -Destination (Join-Path $bundlePath 'resources')
    }
    foreach ($file in @('helpers.py', 'novaprinter.py', 'socks.py')) {
        Copy-Item -LiteralPath (Join-Path $workspaceRoot "resources\qbit\$file") -Destination (Join-Path $bundlePath 'resources\qbit')
    }
    $sourceManifest = Get-Content -LiteralPath (Join-Path $workspaceRoot 'resources\sources.json') -Raw | ConvertFrom-Json
    foreach ($source in $sourceManifest.sources) {
        $pluginPath = Join-Path $workspaceRoot "resources\qbit\engines\$($source.id).py"
        if ((Get-FileHash -LiteralPath $pluginPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $source.sha256) { throw "Plugin integrity failed: $($source.id)" }
        Copy-Item -LiteralPath $pluginPath -Destination $bundleResources.FullName
    }
    Copy-Item -LiteralPath $pythonRuntime -Destination (Join-Path $bundlePath 'runtime') -Recurse
    python -B -X utf8 (Join-Path $PSScriptRoot 'vendor-python-runtime.py') --runtime (Join-Path $bundlePath 'runtime')
    if ($LASTEXITCODE -ne 0) { throw 'Python plugin dependency packaging failed.' }
    & (Join-Path $bundlePath 'runtime\python.exe') -I -B -c 'import bs4, lxml.etree, requests; print("Plugin compatibility runtime ready")'
    if ($LASTEXITCODE -ne 0) { throw 'Bundled plugin dependency smoke check failed.' }
    $mediaRuntime = New-Item -ItemType Directory -Path (Join-Path $bundlePath 'runtime\media')
    Copy-Item -LiteralPath $FFprobePath -Destination (Join-Path $mediaRuntime.FullName 'ffprobe.exe')
    foreach ($dll in (Get-ChildItem -LiteralPath $probeBin -Filter '*.dll' -File)) {
        Copy-Item -LiteralPath $dll.FullName -Destination $mediaRuntime.FullName
    }
    foreach ($notice in @('LICENSE', 'README.txt')) {
        Copy-Item -LiteralPath (Join-Path $probeRoot $notice) -Destination $mediaRuntime.FullName
    }
    & (Join-Path $mediaRuntime.FullName 'ffprobe.exe') -version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Bundled ffprobe failed.' }
    $torrentRuntime = New-Item -ItemType Directory -Path (Join-Path $bundlePath 'runtime\torrent')
    Copy-Item -LiteralPath $RqbitPath -Destination (Join-Path $torrentRuntime.FullName 'rqbit.exe')
    Copy-Item -LiteralPath $rqbitLicense -Destination (Join-Path $torrentRuntime.FullName 'LICENSE')
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'README.md') -Destination $bundlePath
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'examples') -Destination $bundlePath -Recurse
    $bundleDocs = New-Item -ItemType Directory -Path (Join-Path $bundlePath 'docs')
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\agent-contract.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\media-inspection.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\markdown-reports.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\native-sources.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\torrent-workflow.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs\media-comparison.md') -Destination $bundleDocs.FullName
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'THIRD_PARTY_NOTICES.md') -Destination $bundlePath
    # Retain the complete corresponding local Rust source and original notices
    # alongside the executable, including ports of GPL upstream search engines.
    $bundleSource = New-Item -ItemType Directory -Path (Join-Path $bundlePath 'source')
    foreach ($directory in @('src','tests','scripts','.cargo')) { Copy-Item -LiteralPath (Join-Path $workspaceRoot $directory) -Destination $bundleSource.FullName -Recurse }
    foreach ($file in @('Cargo.toml','Cargo.lock','README.md','THIRD_PARTY_NOTICES.md')) { Copy-Item -LiteralPath (Join-Path $workspaceRoot $file) -Destination $bundleSource.FullName }
    $provenance = [ordered]@{
        main_commit = '240b57a90ca4f9e3869151b73c76c46f1b7bb59b'
        wiki_commit = 'e00fd9cd551258290213e684d522bc5e06130a99'
        public_community = @((Get-Content -LiteralPath (Join-Path $workspaceRoot 'research\qbittorrent\community-20261017\catalog.json') -Raw | ConvertFrom-Json).entries | Where-Object visibility -eq 'public')
    }
    $provenance | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $bundleDocs.FullName 'source-provenance.json') -Encoding utf8
    Copy-Item -LiteralPath (Join-Path $bundlePath 'resources') -Destination $bundleSource.FullName -Recurse
    foreach ($directory in @('docs','examples','licenses')) { Copy-Item -LiteralPath (Join-Path $workspaceRoot $directory) -Destination $bundleSource.FullName -Recurse }
    $sourceVerification = New-Item -ItemType Directory -Path (Join-Path $bundleSource.FullName 'verification\limetorrents-benchmark\fixtures')
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'verification\limetorrents-benchmark\fixtures\nirvana-in-fire-1.html') -Destination $sourceVerification.FullName
    foreach ($file in @('nirvana-in-fire-search.json','spiderman-brand-new-day-agent-response.json','media-0.3.0-portable-smoke.json')) { Copy-Item -LiteralPath (Join-Path $workspaceRoot "verification\$file") -Destination (Join-Path $bundleSource.FullName 'verification') }
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'licenses') -Destination $bundlePath -Recurse

    $cargoMetadataText = cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect dependency provenance.' }
    $cargoMetadata = $cargoMetadataText | ConvertFrom-Json
    $dependencyLicenses = Join-Path $bundlePath 'licenses\rust-dependencies'
    foreach ($dependency in $cargoMetadata.packages) {
        if ($dependency.name -eq 'torrent-search') { continue }
        $dependencyRoot = [IO.Path]::GetDirectoryName($dependency.manifest_path)
        $notices = Get-ChildItem -LiteralPath $dependencyRoot -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)' }
        $noticePath = Join-Path $dependencyLicenses "$($dependency.name)-$($dependency.version)"
        New-Item -ItemType Directory -Path $noticePath -Force | Out-Null
        foreach ($notice in $notices) { Copy-Item -LiteralPath $notice.FullName -Destination $noticePath }
    }
    $sourceList = & (Join-Path $bundlePath 'torrentx.exe') --json sources
    if ($LASTEXITCODE -ne 0) { throw 'Bundled source discovery failed.' }
    $sourceList | ConvertFrom-Json | Out-Null
    $packageReport = [ordered]@{
        version = $projectVersion
        target = 'x86_64-pc-windows-msvc'
        python_version = $pythonVersion
        python_archive_url = $pythonUrl
        python_archive_sha256 = $pythonSha256
        python_plugin_dependencies = (Get-Content -LiteralPath (Join-Path $workspaceRoot 'resources\python-dependencies.lock.json') -Raw | ConvertFrom-Json).packages
        native_source_count = $sourceManifest.sources.Count
        ffprobe_version = $probeVersion[0]
        rqbit_version = '9.0.1'
        rqbit_origin = 'https://github.com/ikatson/rqbit/releases/tag/v9.0.1'
        rqbit_sha256 = $rqbitSha256
        ffprobe_origin = (Get-Content -LiteralPath (Join-Path $probeRoot 'README.txt') -TotalCount 9) -join "`n"
        media_runtime_files = @(Get-ChildItem -LiteralPath $mediaRuntime.FullName -File | ForEach-Object {
            [ordered]@{ name = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
        })
        executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $bundlePath 'torrentx.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
        tests_passed = $true
        build_passed = $true
        source_discovery_passed = $true
    }
    $packageReport | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundlePath 'package-info.json') -Encoding utf8
    Compress-Archive -LiteralPath $bundlePath -DestinationPath ($bundlePath + '.zip')
    $archiveHash = (Get-FileHash -LiteralPath ($bundlePath + '.zip') -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath ($bundlePath + '.zip.sha256') -Value $archiveHash -Encoding ascii
    [pscustomobject]@{ Bundle=$bundlePath; Archive=($bundlePath + '.zip'); SHA256=$archiveHash; Tests='passed'; Build='passed'; Discovery='passed' } | ConvertTo-Json
} finally {
    Pop-Location
    if ($null -eq $previousTestPython) { Remove-Item Env:\TORRENT_SEARCH_TEST_PYTHON -ErrorAction SilentlyContinue }
    else { $env:TORRENT_SEARCH_TEST_PYTHON = $previousTestPython }
    if ($null -eq $previousTestProbe) { Remove-Item Env:\TORRENT_SEARCH_TEST_FFPROBE -ErrorAction SilentlyContinue }
    else { $env:TORRENT_SEARCH_TEST_FFPROBE = $previousTestProbe }
}

