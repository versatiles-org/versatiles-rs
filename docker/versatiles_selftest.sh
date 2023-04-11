#!/usr/bin/env bash
cd "$(dirname "$0")"

set -e

versatiles convert --max-zoom 3 "https://storage.googleapis.com/versatiles/download/test.versatiles" test.versatiles
versatiles convert --max-zoom 3 "https://download.versatiles.org/planet-20230227.versatiles" test.versatiles
versatiles serve --auto-shutdown 1000 -p 8088 "https://download.versatiles.org/planet-20230227.versatiles"
