# Data Source Syntax

VersaTiles supports multiple ways to specify data sources for tile containers.

## Basic Usage

### File Paths

Local file paths are resolved relative to the current directory:

```text
tiles.versatiles
./data/world.mbtiles
/absolute/path/to/tiles.pmtiles
```

### HTTP and HTTPS URLs

HTTP and HTTPS URLs are supported for reading. Only formats that support
range-requests can be read remotely: `versatiles` and `pmtiles`.

```text
https://example.org/tiles.versatiles
http://download.example.org/world.pmtiles
```

Connection details:

- Connect timeout: 30 s
- TCP keepalive: 60 s (no overall timeout — large range reads can take minutes)
- Automatic retry on transient failures (up to 2 retries with backoff)
- Adaptive range splitting when the server rejects oversized requests

### HTTP and HTTPS with Basic Authentication

Embed credentials directly in the URL to access password-protected servers
(e.g., WebDAV servers or private file hosts):

```text
https://user:password@example.org/tiles.versatiles
https://admin:s3cret@webdav.example.org/data/world.versatiles
```

Credentials are extracted from the URL, percent-decoded, and sent as an
`Authorization: Basic` header. They are never passed as part of the URL to the
server.

Special characters in the username or password must be percent-encoded
(e.g., `@` → `%40`, `:` → `%3A`):

```text
https://user%40company:p%40ssw0rd@example.org/tiles.versatiles
```

### WebDAV

WebDAV servers speak HTTP/HTTPS, so no special syntax is needed.
Use a plain `https://` URL with basic auth if the server requires it:

```text
https://webdav.example.org/tiles/world.versatiles
https://user:password@webdav.example.org/tiles/world.versatiles
```

### SFTP

SFTP URLs are supported for both **reading** and **writing** when VersaTiles
is built with the `ssh2` feature. Only formats with data-reader/writer support
are available over SFTP: `versatiles` and `pmtiles`.

```text
sftp://fileserver.example.org/data/world.versatiles
sftp://user@fileserver.example.org/data/world.versatiles
sftp://user:password@fileserver.example.org/data/world.versatiles
sftp://fileserver.example.org:2222/data/world.versatiles
```

Default port is **22**. Connect timeout is 30 s; SSH keepalive fires every 60 s.

**Authentication** is tried in this order:

1. Password embedded in the URL (`sftp://user:password@host/…`)
2. Explicit identity file passed via `--ssh-identity`
3. SSH agent
4. `IdentityFile` entries in `~/.ssh/config` for the target host
5. Default key files: `~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`

Writing to SFTP:

```bash
versatiles convert world.mbtiles sftp://user@fileserver.example.org/tiles/world.versatiles
```

## Name and Type Prefixes

You can override the auto-detected name and container type using bracket notation.

### Prefix Notation

Format: `[name,type]location`

```text
[osm,mbtiles]tiles.db        # Set name to "osm", type to "mbtiles"
[,mbtiles]tiles.db           # Set only type to "mbtiles"
[osm]tiles.mbtiles           # Set only name to "osm"
[osm,vpl]pipeline.txt        # Treat file as VPL pipeline with name "osm"
```

### Postfix Notation

Format: `location[name,type]`

```text
tiles.db[osm,mbtiles]        # Same as [osm,mbtiles]tiles.db
tiles.db[,mbtiles]           # Same as [,mbtiles]tiles.db
tiles.mbtiles[osm]           # Same as [osm]tiles.mbtiles
```

## Inline VPL Pipelines

Use `[,vpl]` prefix with parentheses to define a VPL pipeline directly:

```text
[,vpl](from_mbtiles tiles.mbtiles)
[osm,vpl](from_mbtiles tiles.mbtiles | filter level_max=10)
```

The content in parentheses is treated as VPL (VersaTiles Pipeline Language).

## JSON Format

For programmatic use, data sources can be specified as JSON:

```json
{"location": "tiles.mbtiles"}
{"name": "osm", "type": "mbtiles", "location": "tiles.db"}
{"name": "inline", "type": "vpl", "content": "from_debug"}
{"name": "remote", "type": "versatiles", "location": "https://example.org/tiles.versatiles"}
```

JSON fields:

- `location`: Path or URL to the data source
- `name`: Optional name identifier (defaults to filename without extension)
- `type`: Optional container type (defaults to file extension)
- `content`: Inline content (alternative to `location`)

## Supported Container Types

- `versatiles` - VersaTiles format (*.versatiles) — supports HTTP, HTTPS, SFTP
- `pmtiles` - PMTiles format (*.pmtiles) — supports HTTP, HTTPS, SFTP
- `mbtiles` - MBTiles SQLite format (*.mbtiles) — local files only
- `tar` - Tar archive (*.tar) — local files only
- `vpl` - VersaTiles Pipeline Language (*.vpl)
- Directory containing tiles in `{z}/{x}/{y}.{ext}` structure

## Examples

```bash
# Basic file
versatiles convert tiles.mbtiles output.versatiles

# Remote VersaTiles container over HTTPS
versatiles probe https://download.versatiles.org/osm.versatiles

# Remote PMTiles container over HTTPS
versatiles probe https://example.org/world.pmtiles

# Named tile source for serving
versatiles serve [osm]tiles.versatiles [satellite]imagery.mbtiles

# Inline VPL pipeline
versatiles convert "[,vpl](from_mbtiles in.mbtiles | filter level_max=12)" out.versatiles

# Override container type
versatiles probe tiles.db[,mbtiles]

# HTTPS with basic auth (e.g., WebDAV)
versatiles probe https://user:password@webdav.example.org/tiles.versatiles

# SFTP read with password auth
versatiles probe sftp://user:password@fileserver.example.org/data/tiles.versatiles

# SFTP read with SSH agent or key (no password in URL)
versatiles probe sftp://user@fileserver.example.org/data/tiles.versatiles

# SFTP write
versatiles convert world.mbtiles sftp://user@fileserver.example.org/tiles/world.versatiles

# SFTP with non-standard port
versatiles probe sftp://user@fileserver.example.org:2222/data/tiles.versatiles
```
