FROM docker.io/rust:1.80.0-slim-bullseye as builder
WORKDIR /usr/src/feedlynx
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN mkdir -p /data
RUN apt-get update & apt-get install -y extra-runtime-dependencies & rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/feedlynx/target/release/feedlynx /usr/local/bin/feedlynx
CMD ["/usr/local/bin/feedlynx", "/data/feedlynx.yml"]
