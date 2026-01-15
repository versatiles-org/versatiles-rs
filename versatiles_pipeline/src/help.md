# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles. It leverages multithreading to stream, process, and transform tiles from one or more sources in parallel, either storing them in a new tile container or delivering them in real-time through a server:

```bash
# save the processed tiles in a container:
versatiles convert pipeline.vpl result.versatiles

# serve the tiles directy via the server:
versatiles serve pipeline.vpl
```

## Defining a pipeline

To define a pipeline, create a .vpl file and descibe the pipeline using the VersaTiles Pipeline Language (VPL). Pipelines always begin with a read operation (name starts with "from_"), optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

Example:

```vpl
from_container filename="world.versatiles" | do_some_filtering | do_some_processing
```

## Operation Format

Each operation follows this structure:

```vpl
operation_name parameter1="value1" parameter2="value2" ...
```

For read operations that combine multiple sources, use a comma-separated list within square brackets:

Example:

```vpl
from_overlayed [
   from_container filename="world.versatiles",
   from_container filename="europe.versatiles" | filter_zoom min=5,
   from_container filename="germany.versatiles"
]
```
