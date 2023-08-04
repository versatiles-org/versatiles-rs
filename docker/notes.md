

```bash
docker buildx build --platform="linux/arm64" --progress="plain" --file="docker/aarch64-unknown-linux-musl.Dockerfile" --tag="arm64-musl-versatiles" .
docker run --platform="linux/arm64" "arm64-musl-versatiles"
```