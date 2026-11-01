# Builds the Linux version with Docker: target-linux/release/cordfast.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)
docker build -t cordfast-linux -f scripts/linux.Dockerfile scripts
docker run --rm -v "${PWD}:/src" -v cordfast-cargo:/usr/local/cargo/registry cordfast-linux
