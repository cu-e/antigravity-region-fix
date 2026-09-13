$ErrorActionPreference = 'Stop'
$action = 'install'
$forward = @($args)
if ($forward.Count -gt 0 -and $forward[0] -eq '--uninstall') {
    $action = 'uninstall'
    $forward = @($forward | Select-Object -Skip 1)
}
$binary = Join-Path $PSScriptRoot 'bin/antigravity-region-fix.exe'
if (-not (Test-Path $binary)) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Rust 1.88+ is required to build from source; native release bundles need no Rust or Python.'
    }
    & cargo build --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml') --target-dir (Join-Path $PSScriptRoot 'target') --release --locked --bins
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $binary = Join-Path $PSScriptRoot 'target/release/antigravity-region-fix.exe'
}
& $binary $action @forward
exit $LASTEXITCODE
