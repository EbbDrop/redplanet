FROM rust:buster AS build

WORKDIR /rp

RUN --mount=type=bind,source=.,target=/rp \
    --mount=type=cache,target=/rp/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
cargo build --locked --release
cp ./target/release/red-planet-test /usr/bin/redplanet-test
EOF


FROM registry.gitlab.com/incoresemi/docker-images/compliance AS run

RUN pip install riscof

WORKDIR /rp

COPY --from=build /usr/bin/redplanet-test /usr/bin/
