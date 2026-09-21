# syntax=docker/dockerfile:1.7
FROM --platform=linux/amd64 rust:1.93.0-bookworm@sha256:7274e0edb5b47eda8053b350ebf3d489f7e0f65d2d7e77b16076299c7c047c28 AS build
WORKDIR /source
COPY . .
RUN cargo build --locked --release -p tect-cli --bins

FROM scratch AS artifacts
COPY --from=build /source/target/release/tectd /tectd
COPY --from=build /source/target/release/tectd-mcp /tectd-mcp
COPY --from=build /source/target/release/tect-admin /tect-admin

FROM --platform=linux/amd64 debian:bookworm-slim@sha256:5ae3c39ebd15e229dcedd5cee596b2497182493d41ff162e824ba13fc1b2b867 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /source/target/release/tectd /usr/local/bin/tectd
COPY --from=build /source/target/release/tectd-mcp /usr/local/bin/tectd-mcp
COPY --from=build /source/target/release/tect-admin /usr/local/bin/tect-admin
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/tectd"]
