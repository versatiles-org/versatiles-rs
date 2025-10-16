#!/usr/bin/env pwsh

# Enable strict error handling
$ErrorActionPreference = "Stop"

# Input arguments
$FOLDER = $args[0]
$FILENAME = "versatiles-" + $args[1]
$TAG = $args[2]

#Change to the specified directory
Set-Location -Path $FOLDER

Write-Host "Create a tarball ..."
tar -cf "$FILENAME.tar" "versatiles.exe"

Write-Host "... and gzip it"
gzip -9 "$FILENAME.tar"

# Write-Host "Calculate SHA256 checksum"
# $sha256 = Get-FileHash -Algorithm SHA256 -Path "$FILENAME.tar.gz"
# $sha256.Hash | Out-File -FilePath "$FILENAME.tar.gz.sha256" -Encoding ASCII
# 
# Write-Host "Calculate MD5 checksum"
# $md5 = Get-FileHash -Algorithm MD5 -Path "$FILENAME.tar.gz"
# $md5.Hash | Out-File -FilePath "$FILENAME.tar.gz.md5" -Encoding ASCII

Write-Host "Upload tarball and checksums to GitHub release"
#&gh release upload $TAG "$FILENAME.tar.gz" "$FILENAME.tar.gz.sha256" "$FILENAME.tar.gz.md5" --clobber
&gh release upload $TAG "$FILENAME.tar.gz" --clobber
