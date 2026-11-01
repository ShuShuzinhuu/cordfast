# Builds a signed CordFast release in dist/, the way the updater expects it:
#   cordfast-v<version>-x86_64-pc-windows-msvc.zip  (folder with cordfast.exe)
#   checksums.txt  (sha256sum format)  and  checksums.txt.sig  (Ed25519)
# With -Publish, also creates the GitHub release v<version> with them.
param([switch]$Publish)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
$name = "cordfast-v$version-x86_64-pc-windows-msvc"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build falhou" }

Remove-Item dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "dist\$name" | Out-Null
Copy-Item target\release\cordfast.exe "dist\$name\cordfast.exe"
Compress-Archive -Path "dist\$name" -DestinationPath "dist\$name.zip"
Remove-Item "dist\$name" -Recurse

cargo run --quiet --example release -- checksums "dist\$name.zip" | Out-File -Encoding ascii -NoNewline dist\checksums.txt
Add-Content -Encoding ascii -Path dist\checksums.txt -Value ""
cargo run --quiet --example release -- sign dist\checksums.txt
Get-ChildItem dist | Format-Table Name, Length

if ($Publish) {
    gh release create "v$version" "dist\$name.zip" dist\checksums.txt dist\checksums.txt.sig `
        --repo ShuShuzinhuu/cordfast --title "CordFast $version" --notes "CordFast $version"
}
