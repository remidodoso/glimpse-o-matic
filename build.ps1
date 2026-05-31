# Stamp index.html
$stamp = Get-Date -Format "MMdd:HHmm"
$file  = "$PSScriptRoot\index.html"
$html  = Get-Content $file -Raw
$html  = $html -replace '<!-- Build \d{4}:\d{4} -->', "<!-- Build $stamp -->"
[System.IO.File]::WriteAllText($file, $html, (New-Object System.Text.UTF8Encoding $false))
Write-Host "Build $stamp"

# Build glimr (WASM)
Write-Host "Building glimr (WASM)..."
wasm-pack build glimr --target web --out-dir ../pkg
if ($LASTEXITCODE -ne 0) { Write-Error "wasm-pack build failed"; exit 1 }
Remove-Item -Force "$PSScriptRoot\pkg\.gitignore" -ErrorAction SilentlyContinue
Write-Host "glimr -> pkg/"

# Build Rust tools
Write-Host "Building Rust tools..."
cargo build --release -p packg -p deployg
if ($LASTEXITCODE -ne 0) { Write-Error "cargo build failed"; exit 1 }

$bin_dir = "$PSScriptRoot\tools\bin"
if (-not (Test-Path $bin_dir)) { New-Item -ItemType Directory -Path $bin_dir | Out-Null }

foreach ($tool in @("packg", "deployg")) {
    if (Test-Path "$PSScriptRoot\target\release\$tool.exe") {
        Copy-Item "$PSScriptRoot\target\release\$tool.exe" "$bin_dir\$tool.exe" -Force
    } else {
        Copy-Item "$PSScriptRoot\target\release\$tool" "$bin_dir\$tool" -Force
    }
}
Write-Host "packg, deployg -> tools/bin/"
