# Stamp index.html
$stamp = Get-Date -Format "MMdd:HHmm"
$file  = "$PSScriptRoot\index.html"
$html  = Get-Content $file -Raw
$html  = $html -replace '<!-- Build \d{4}:\d{4} -->', "<!-- Build $stamp -->"
[System.IO.File]::WriteAllText($file, $html, (New-Object System.Text.UTF8Encoding $false))
Write-Host "Build $stamp"

# Build Rust tools
Write-Host "Building packg..."
cargo build --release -p packg
if ($LASTEXITCODE -ne 0) { Write-Error "cargo build failed"; exit 1 }

$bin_dir = "$PSScriptRoot\tools\bin"
if (-not (Test-Path $bin_dir)) { New-Item -ItemType Directory -Path $bin_dir | Out-Null }

if (Test-Path "$PSScriptRoot\target\release\packg.exe") {
    Copy-Item "$PSScriptRoot\target\release\packg.exe" "$bin_dir\packg.exe" -Force
} else {
    Copy-Item "$PSScriptRoot\target\release\packg" "$bin_dir\packg" -Force
}
Write-Host "packg -> tools/bin/"
