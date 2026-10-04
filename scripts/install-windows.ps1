# PowerShell script to install versatiles on Windows
#
# Installs the latest stable release. To install a particular release — a
# release candidate, or an older version — pass its tag, or set
# VERSATILES_VERSION:
#
#   & "$env:TEMP\install-windows.ps1" -Version v5.0.0-rc.2

param(
   # Release tag to install, e.g. "v5.0.0-rc.2"; the leading "v" is optional.
   [string]$Version = $env:VERSATILES_VERSION
)

# The base URL of the release to download from. The tag ends up in a URL, so it
# is checked against the shape of a version tag first.
function Get-ReleaseUrl {
   param ([string]$version)

   if ([string]::IsNullOrWhiteSpace($version)) {
      Write-Host "Installing the latest VersaTiles release" -ForegroundColor Green
      return "https://github.com/versatiles-org/versatiles-rs/releases/latest/download"
   }
   if (-not $version.StartsWith("v")) {
      $version = "v$version"
   }
   if ($version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$') {
      Write-Host "Not a version tag: $version (expected e.g. v5.0.0 or v5.0.0-rc.2)" -ForegroundColor Red
      exit 1
   }
   Write-Host "Installing VersaTiles $version" -ForegroundColor Green
   return "https://github.com/versatiles-org/versatiles-rs/releases/download/$version"
}

# Function to detect the system architecture
function Detect-Architecture {
   $arch = (Get-WmiObject Win32_OperatingSystem).OSArchitecture
   if ($arch -eq "64-bit") {
      $cpuArch = (Get-WmiObject Win32_Processor).Architecture
      if ($cpuArch -eq 9) {
         return "x86_64"
      } elseif ($cpuArch -eq 12) {
         return "aarch64"
      } else {
         Write-Host "Unsupported CPU architecture: $cpuArch" -ForegroundColor Red
         exit 1
      }
   } else {
      Write-Host "Unsupported OS architecture: $arch" -ForegroundColor Red
      exit 1
   }
}

# Function to download and install the package
function Install-Package {
   param (
      [string]$architecture,
      [string]$releaseUrl
   )

   $packageUrl = "$releaseUrl/versatiles-windows-$architecture.tar.gz"
   $checksumUrl = "$packageUrl.sha256"
   $downloadPath = "$env:TEMP\versatiles.tar.gz"
   $checksumPath = "$env:TEMP\versatiles.tar.gz.sha256"
   $installDir = "$env:ProgramFiles\versatiles"

   # Download the package.
   #
   # try/catch, not `if (-not $?)`: Invoke-WebRequest throws on failure rather
   # than setting $?, so the old check never ran and a failed download fell
   # through to the extract step.
   Write-Host "Downloading versatiles for $architecture..." -ForegroundColor Green
   try {
      Invoke-WebRequest -Uri $packageUrl -OutFile $downloadPath
      Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath
   } catch {
      Write-Host "Failed to download the package: $_" -ForegroundColor Red
      exit 1
   }

   # Verify the download before unpacking it into ProgramFiles. The published
   # file is "<hash>  <filename>"; only the hash is compared, since the name in
   # it is the release asset's rather than this temporary copy's.
   Write-Host "Verifying checksum..." -ForegroundColor Green
   $expected = ((Get-Content -Path $checksumPath -TotalCount 1) -split '\s+')[0].ToLower()
   $actual = (Get-FileHash -Algorithm SHA256 -Path $downloadPath).Hash.ToLower()
   if ([string]::IsNullOrWhiteSpace($expected) -or ($expected -ne $actual)) {
      Write-Host "Checksum mismatch for $packageUrl" -ForegroundColor Red
      Write-Host "  expected: $expected" -ForegroundColor Red
      Write-Host "  actual:   $actual" -ForegroundColor Red
      Remove-Item $downloadPath, $checksumPath -ErrorAction SilentlyContinue
      exit 1
   }

   # Create installation directory if it doesn't exist
   if (-not (Test-Path $installDir)) {
      New-Item -ItemType Directory -Path $installDir
   }

   # Extract the tar.gz file
   Write-Host "Extracting the package..." -ForegroundColor Green
   tar -xzf $downloadPath -C $installDir
   if (-not $?) {
      Write-Host "Failed to extract the package." -ForegroundColor Red
      exit 1
   }

   # Add the directory to the PATH if not already included
   if (-not $env:Path.Contains($installDir)) {
      Write-Host "Adding $installDir to system PATH..." -ForegroundColor Green
      $env:Path += ";$installDir"
      [Environment]::SetEnvironmentVariable("Path", $env:Path, [EnvironmentVariableTarget]::Machine)

      # Reload the PATH in the current session
      $env:Path = [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::Machine)
   }

   # Clean up
   Remove-Item $downloadPath, $checksumPath -ErrorAction SilentlyContinue

   Write-Host "VersaTiles installed successfully." -ForegroundColor Green
}

# Main script execution
$releaseUrl = Get-ReleaseUrl -version $Version
$architecture = Detect-Architecture
Install-Package -architecture $architecture -releaseUrl $releaseUrl
