# syntax=docker/dockerfile:1
FROM rust:1.88-bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
      protobuf-compiler {% if database == "postgres" %}libpq-dev{% else %}libsqlite3-dev{% endif %} \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Cargo.lock is committed, so --locked builds exactly what was tested. A freshly
# generated project has none until `cargo build` writes one: run that and commit
# the result, or this COPY fails and the image cannot build at all.
COPY Cargo.toml Cargo.lock ./
COPY grpc ./grpc
{% if gateway %}COPY web ./web
{% endif %}RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates {% if database == "postgres" %}libpq5{% else %}libsqlite3-0{% endif %} \
    && rm -rf /var/lib/apt/lists/*

# Not root. A container that runs as root is one container escape away from
# being a host compromise, and nothing here needs the privilege.
RUN useradd --system --uid 10001 --create-home app
USER 10001

WORKDIR /app
COPY --from=build /build/target/release/{{project-name}} /usr/local/bin/{{project-name}}
{% if gateway %}COPY --from=build /build/target/release/gateway /usr/local/bin/gateway{% endif %}

ENV LISTEN_ADDR=0.0.0.0:50051
EXPOSE 50051

ENTRYPOINT ["{{project-name}}"]
