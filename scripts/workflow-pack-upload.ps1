#!/usr/bin/env pwsh

# Enable strict error handling
$ErrorActionPreference = "Stop"

# Input arguments
$FOLDER = $args[0]
$FILENAME = "versatiles-" + $args[1]
$TAG = $args[2]

#Change to the specified directory
Set-Location -Path "$FOLDER\cli"

Write-Host "Create a tarball ..."
tar -cf "$FILENAME.tar" "versatiles.exe"

Write-Host "... and gzip it"
gzip -9 "$FILENAME.tar"

# Publish a SHA-256 next to the tarball, so the install script can refuse an
# archive it cannot verify. See workflow-pack-upload.sh for the reasoning.
#
# Written by hand in coreutils format ("<hash>  <filename>", lowercase, LF) to
# match what the Unix packer produces: `Get-FileHash` returns a bare uppercase
# hash, and `Out-File` would end the line with CRLF, neither of which
# `sha256sum -c` accepts.
Write-Host "Calculate SHA256 checksum"
$sha256 = (Get-FileHash -Algorithm SHA256 -Path "$FILENAME.tar.gz").Hash.ToLower()
[System.IO.File]::WriteAllText(
   (Join-Path (Get-Location) "$FILENAME.tar.gz.sha256"),
   "$sha256  $FILENAME.tar.gz`n"
)

Write-Host "Upload tarball and checksum to GitHub release"
&gh release upload $TAG "$FILENAME.tar.gz" "$FILENAME.tar.gz.sha256" --clobber
