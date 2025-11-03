# VersaTiles Server Configuration

The VersaTiles server uses a YAML configuration file to control its behavior. This file lets you customize the server’s network settings, security policies, static content, and tile sources. You provide the configuration file when starting the server with the `--config` option:

```shell
versatiles serve --config server_config.yaml
```

Below is a complete example of a server configuration file with detailed explanations. All sections and fields are optional; default values are used when fields are omitted.

```yaml
# Optional HTTP server configuration
server: 
  
  # Optional IP address to bind to
  # Defaults to "0.0.0.0"
  ip: 0.0.0.0
  
  # Optional HTTP server port
  # Defaults to 8080
  port: 8080
  
  # Optional flag to prefer faster compression over smaller size
  # Defaults to false (smaller compression)
  minimal_recompression: false
  
  # Optional flag to disable the `/api` endpoints
  # Defaults to false (enabling the API)
  disable_api: false

# Optional Cross-Origin Resource Sharing (CORS) settings
cors: 
  
  # Allowed origins for CORS requests
  # Supports:
  # - Globs at the start of the domain like `*.example.com`
  # - Globs at the end of the domain like `example.*`
  # - Regular expressions enclosed in slashes like `/domain\..*$/`
  allowed_origins:  
    - "https://example.org"
    - "*.example.net"
  
  # Optional duration for preflight cache in seconds
  # Defaults to 86400 (1 day)
  max_age_seconds: 86400

# Optional extra HTTP response headers to add to every response
# For example, cache control or timing headers
extra_response_headers: 
  Cache-Control: public, max-age=86400, immutable
  CDN-Cache-Control: max-age=604800

# Optional list of static content sources
static: 
  - # Path to static files or archive (e.g., .tar.gz) containing assets
    path: ./frontend.tar
    
    # Optional URL prefix where static files will be served
    # Defaults to root ("/")
    url_prefix: /

# Optional list of tile sources
tiles: 
  - # Optional name identifier for this tile source
    # Tiles will be available under `/tiles/{name}/...`
    # Defaults to the last part of the path (e.g., "osm" for "osm.versatiles")
    name: osm
    
    # Path or URL to the tile data source
    # Can be a local file or remote URL.
    path: osm.versatiles
```
