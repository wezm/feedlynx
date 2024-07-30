FROM docker.io/rust:1.80-alpine3.20 AS builder

RUN apk --update add --no-cache musl-dev

WORKDIR /usr/src/feedlynx

COPY . .

RUN cargo build --release --locked


FROM alpine:3.20

ENV FEEDLYNX_ADDRESS=0.0.0.0

RUN mkdir -p /data

#RUN apt-get update & apt-get install -y extra-runtime-dependencies & rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/feedlynx/target/release/feedlynx /usr/local/bin/feedlynx

VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/feedlynx"]
