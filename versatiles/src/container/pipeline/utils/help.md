# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles, from thousands to millions. It leverages multithreading to stream, process, and transform tiles from one or more sources in parallel, either storing them in a new tile container or delivering them in real-time through a server.

## Defining a pipeline

To define a pipeline, use the VersaTiles Pipeline Language (VPL). Pipelines begin with a read operation, optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

Example:
```
get_tiles filename="world.versatiles" | do_some_filtering | do_some_processing
```

## Operation Format

Each operation follows this structure:
```
operation_name parameter1="value1" parameter2="value2" ...
```

For read operations that combine multiple sources, use a comma-separated list within square brackets:

Example:
```
get_overlayed [
   get_tiles filename="world.versatiles",
   get_tiles filename="europe.versatiles" | do_some_filtering,
   get_tiles filename="germany.versatiles"
]
```