# VersaTiles Server Configuration

The VersaTiles server uses a YAML configuration file to control its behavior. This file lets you customize the server's network settings, security policies, static content, and tile sources. You provide the configuration file when starting the server with the `--config` option:

```shell
versatiles serve --config server_config.yaml
```

Below is a complete example of a server configuration file with detailed explanations. All sections and fields are optional; default values are used when fields are omitted.

## Tile Sources

Tile sources can be specified as:

- **Local files**: `.versatiles`, `.mbtiles`, `.pmtiles`, or `.tar` containers
- **Remote URLs**: HTTP/HTTPS URLs to tile containers
- **VPL pipelines**: `.vpl` files for advanced tile processing (see `versatiles help pipeline`)

Example with different source types:

```yaml
tiles:
  - name: osm
    src: osm.versatiles
  - name: remote
    src: https://example.org/tiles.pmtiles
  - name: processed
    src: pipeline.vpl
```

VPL (VersaTiles Pipeline Language) allows you to define complex tile processing pipelines that can merge, filter, transform, and recompress tiles from multiple sources. For detailed VPL documentation, run `versatiles help pipeline`.
