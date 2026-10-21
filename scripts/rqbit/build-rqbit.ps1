param([string]$SourceDirectory, [string]$Destination)

# Build rqbit v9.0.1 with stream-join.patch: peers join a piece a stream is
# waiting on instead of leaving it to one slow peer. Without the web UI,
# Postgres or Prometheus, which torrentx does not use.
$ErrorActionPreference = 'Stop'
$workspaceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$tag = 'v9.0.1'
$commit = 'a499d2f243d124e144aef137afe7cb304a6e3f36'
if (-not $SourceDirectory) { $SourceDirectory = Join-Path $workspaceRoot '.cache\rqbit-src' }
if (-not $Destination) { $Destination = Join-Path $workspaceRoot '.cache\rqbit-9.0.1-stream-join' }
$patch = Join-Path $PSScriptRoot 'stream-join.patch'

if (-not (Test-Path -LiteralPath $SourceDirectory)) {
    git clone --quiet --depth 1 --branch $tag https://github.com/ikatson/rqbit.git $SourceDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Cannot clone rqbit.' }
}
$head = git -C $SourceDirectory rev-parse HEAD
if ($head -ne $commit) { throw "rqbit source is at $head, expected $commit ($tag)." }
git -C $SourceDirectory checkout --quiet -- .
git -C $SourceDirectory apply --whitespace=nowarn $patch
if ($LASTEXITCODE -ne 0) { throw 'stream-join.patch does not apply.' }

Push-Location -LiteralPath $SourceDirectory
try {
    cargo test -p librqbit --lib --locked --no-default-features --features default-tls
    if ($LASTEXITCODE -ne 0) { throw 'librqbit tests failed.' }
    # Static C runtime, and no builder paths in the panic locations.
    $previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        '-Ctarget-feature=+crt-static',
        "--remap-path-prefix=$($env:USERPROFILE)=~",
        "--remap-path-prefix=$([IO.Path]::GetFullPath($SourceDirectory))=rqbit"
    ) -join [char]0x1f
    # aws-lc's C sources embed __FILE__; MSVC trims the prefix with /d1trimfile.
    $previousCFlags = $env:CFLAGS
    $env:CFLAGS = "/d1trimfile:$($env:USERPROFILE)\"
    try {
        cargo build --release -p rqbit --locked --no-default-features --features default-tls
        if ($LASTEXITCODE -ne 0) { throw 'rqbit build failed.' }
    } finally {
        $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
        $env:CFLAGS = $previousCFlags
    }
} finally {
    Pop-Location
}

$built = Join-Path $SourceDirectory 'target\release\rqbit.exe'
$text = [Text.Encoding]::Latin1.GetString([IO.File]::ReadAllBytes($built))
if ($text.IndexOf($env:USERNAME, [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw 'rqbit.exe still contains the user name.' }
if ($text.Contains('VCRUNTIME140.dll')) { throw 'rqbit.exe needs the Visual C++ runtime.' }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$exe = Join-Path $Destination 'rqbit.exe'
Copy-Item -LiteralPath (Join-Path $SourceDirectory 'target\release\rqbit.exe') -Destination $exe -Force
Copy-Item -LiteralPath (Join-Path $SourceDirectory 'LICENSE') -Destination (Join-Path $Destination 'LICENSE') -Force
Copy-Item -LiteralPath $patch -Destination (Join-Path $Destination 'stream-join.patch') -Force
[pscustomobject]@{
    Executable = $exe
    SHA256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    Source = "https://github.com/ikatson/rqbit/tree/$tag ($commit) + stream-join.patch"
} | ConvertTo-Json
