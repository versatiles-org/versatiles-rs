#!/usr/bin/env pwsh

# Enable strict error handling
$ErrorActionPreference = "Stop"

# Input arguments
$FOLDER = $args[0]
$FILENAME = "versatiles-" + $args[1]
$TAG = $args[2]

# Change to the specified directory
Set-Location -Path $FOLDER

# Create a tarball and gzip it
tar -cf "$FILENAME.tar" "versatiles.exe"
gzip -9 "$FILENAME.tar"

# Determine OS and calculate checksums
switch ($env:OS) {
    "Windows_NT" {
        Write-Host "Windows does not support the same tools natively; use Git Bash or another solution for checksum calculation."
    }
    default {
        $uname = &uname -s
        switch ($uname) {
            "Linux" {
                &sha256sum "$FILENAME.tar.gz" > "$FILENAME.tar.gz.sha256"
                &md5sum "$FILENAME.tar.gz" > "$FILENAME.tar.gz.md5"
            }
            "Darwin" {
                &shasum -a 256 "$FILENAME.tar.gz" > "$FILENAME.tar.gz.sha256"
                &md5 "$FILENAME.tar.gz" > "$FILENAME.tar.gz.md5"
            }
            default {
                Write-Host "Unknown OS: $uname"
            }
        }
    }
}

# Upload tarball and checksums to GitHub release
&gh release upload $TAG "$FILENAME.tar.gz" "$FILENAME.tar.gz.sha256" "$FILENAME.tar.gz.md5" --clobber

# Check for .deb files and upload them if they exist
if (Get-ChildItem -Filter *.deb) {
    &gh release upload $TAG *.deb --clobber
}
