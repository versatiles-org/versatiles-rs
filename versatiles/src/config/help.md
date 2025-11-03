# VersaTiles Server Configuration

The VersaTiles server uses a YAML configuration file to control its behavior. This file lets you customize the server’s network settings, security policies, static content, and tile sources. You provide the configuration file when starting the server with the `--config` option:

```shell
versatiles serve --config server_config.yaml
```

Below is a complete example of a server configuration file with detailed explanations. All sections and fields are optional; default values are used when fields are omitted.