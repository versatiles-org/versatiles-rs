set -e

# VERSATILES_URL="https://storage.googleapis.com/versatiles/download/test.versatiles"
VERSATILES_URL="https://download.versatiles.org/planet-20230227.versatiles"
versatiles convert --max-zoom 3 "$VERSATILES_URL" test.versatiles
versatiles serve --auto-shutdown 1000 -p 8088 "$VERSATILES_URL"
