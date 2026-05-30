$stamp = Get-Date -Format "MMdd:HHmm"
$file  = "$PSScriptRoot\index.html"
$html  = Get-Content $file -Raw
$html  = $html -replace '<!-- Build \d{4}:\d{4} -->', "<!-- Build $stamp -->"
[System.IO.File]::WriteAllText($file, $html, (New-Object System.Text.UTF8Encoding $false))
Write-Host "Build $stamp"
