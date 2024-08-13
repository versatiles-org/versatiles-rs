# PowerShell script to install versatiles on Windows

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
      [string]$architecture
   )

   $packageUrl = "https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-windows-$architecture.tar.gz"
   $downloadPath = "$env:TEMP\versatiles.tar.gz"
   $installDir = "$env:ProgramFiles\versatiles"

   # Download the package
   Write-Host "Downloading versatiles for $architecture..." -ForegroundColor Green
   Invoke-WebRequest -Uri $packageUrl -OutFile $downloadPath
   if (-not $?) {
      Write-Host "Failed to download the package." -ForegroundColor Red
      exit 1
   }

   # Create installation directory if it doesn't exist
   if (-not (Test-Path $installDir)) {
      New-Item -ItemType Directory -Path $installDir
   }

   # Extract the tar.gz file
   Write-Host "Extracting the package..." -ForegroundColor Green
   Add-Type -AssemblyName System.IO.Compression.FileSystem
   function Extract-TarGz {
      param([string]$gzipFileName, [string]$targetDir)
      $gzipFileStream = [System.IO.File]::OpenRead($gzipFileName)
      $gzipStream = New-Object System.IO.Compression.GZipStream($gzipFileStream, [System.IO.Compression.CompressionMode]::Decompress)
      $tarFileName = [System.IO.Path]::ChangeExtension($gzipFileName, ".tar")
      $tarFileStream = [System.IO.File]::Create($tarFileName)
      $gzipStream.CopyTo($tarFileStream)
      $gzipStream.Close()
      $tarFileStream.Close()

      $tarFileStream = [System.IO.File]::OpenRead($tarFileName)
      $reader = New-Object IO.StreamReader($tarFileStream)
      $tarArchive = New-Object Collections.ArrayList
      while ($tarFileStream.Position -lt $tarFileStream.Length) {
         $headerBuffer = New-Object byte[] 512
         $tarFileStream.Read($headerBuffer, 0, 512) | Out-Null
         if ($headerBuffer[0] -eq 0) { break }
         $header = [System.Text.Encoding]::ASCII.GetString($headerBuffer).Trim([char]0)
         $fileName = $header.Substring(0, 100).Trim()
         $fileSize = [Convert]::ToInt64($header.Substring(124, 12).Trim(), 8)
         $fileBuffer = New-Object byte[] $fileSize
         $tarFileStream.Read($fileBuffer, 0, $fileSize) | Out-Null
         if ($fileName) {
            $outputFile = Join-Path $targetDir $fileName
            [System.IO.File]::WriteAllBytes($outputFile, $fileBuffer)
         }
         $tarFileStream.Seek([Math]::Ceiling($fileSize / 512) * 512, 'Current') | Out-Null
      }
      $tarFileStream.Close()
      Remove-Item $tarFileName
   }

   Extract-TarGz -gzipFileName $downloadPath -targetDir $installDir
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
   Remove-Item $downloadPath

   Write-Host "Versatiles installed successfully." -ForegroundColor Green
}

# Main script execution
$architecture = Detect-Architecture
Install-Package -architecture $architecture
