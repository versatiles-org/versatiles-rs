# .Dockerfile

These .Dockerfiles are used in a GitHub Workflow to build VersaTiles for different Architectures.

The final stage is `FROM scratch` and only carries the build results, so the
image is not meant to be run: export it to a folder instead, as the release
workflow does. To try a build locally:

```bash
docker buildx build --platform="linux/amd64" --progress="plain" --file="docker/build-linux.Dockerfile" --build-arg="ARCH=x86_64" --build-arg="LIBC=musl" --output="type=local,dest=output/" .

# The CLI binary and the Node.js library land in output/cli/ and output/node/
ls -lh output/cli output/node
docker run --rm --platform="linux/amd64" -v "$PWD/output/cli:/cli" alpine /cli/versatiles --version
```

For `linux/arm64`, use `--platform="linux/arm64"` and `ARCH=aarch64` in both commands.
