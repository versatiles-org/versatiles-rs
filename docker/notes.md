
These .Dockerfiles are used in a GitHub Workflow to build VersaTiles for different Architectures.

Here are some Bash lines for testing:

```bash
docker buildx build --platform="linux/arm64" --progress="plain" --file="docker/linux-musl-arm64.Dockerfile" --tag="arm64-musl-versatiles" .
docker run --platform="linux/arm64" "arm64-musl-versatiles"
```