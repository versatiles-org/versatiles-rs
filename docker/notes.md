# .Dockerfile

These .Dockerfiles are used in a GitHub Workflow to build VersaTiles for different Architectures.

Here are some Bash lines for testing:

```bash
docker buildx build --platform="linux/amd64" --progress="plain" --file="docker/build-linux.Dockerfile" --tag="amd64-musl-versatiles" --build-arg="ARCH=x86_64" --build-arg="LIBC=musl" --output="type=local,dest=output/" .
docker run --platform="linux/arm64" "arm64-musl-versatiles"
```
