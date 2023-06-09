# create builder system
FROM alpine as builder

# install dependencies
RUN apk add curl gcc musl-dev openssl-dev pkgconfig sqlite-dev

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable

# install versatiles
RUN $HOME/.cargo/bin/cargo install versatiles

# download frontend
RUN curl -L "https://github.com/versatiles-org/versatiles-frontend/releases/latest/download/frontend.br.tar" > frontend.br.tar

# create production system
FROM alpine

# install dependencies
RUN apk add --no-cache curl sqlite

# copy versatiles, frontend and selftest
COPY --from=builder /root/.cargo/bin/versatiles /usr/bin/
COPY --from=builder frontend.br.tar .
COPY helpers/versatiles_selftest.sh .
