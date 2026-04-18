param(
    [string]$DataDir = ".\.bench-5k",
    [int]$StartupIterations = 50,
    [int]$PerfIterations = 50,
    [int]$PerfLimit = 200,
    [string]$PerfQuery = "rust"
)

$ErrorActionPreference = "Stop"

Write-Host "[1/4] cargo check"
cargo check

Write-Host "[2/4] cargo test --lib"
cargo test --lib

Write-Host "[3/4] startup perf"
cargo run -- --data-dir $DataDir perf-startup --iterations $StartupIterations

Write-Host "[4/4] search/list perf"
cargo run -- --data-dir $DataDir perf $PerfQuery --iterations $PerfIterations --limit $PerfLimit

Write-Host "release-check finished"
