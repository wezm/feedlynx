FROM docker.io/rust:1.80-alpine3.20 AS builder

RUN apk --update add --no-cache musl-dev

WORKDIR /usr/src/feedlynx

COPY . .

RUN cargo build --release --locked


FROM alpine:3.20

ENV FEEDLYNX_ADDRESS=0.0.0.0

HEALTHCHECK --interval=5m --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q \
    --header 'Content-Type: application/x-www-form-urlencoded' \
    --post-data "token=$FEEDLYNX_PRIVATE_TOKEN" \
    -O - \
    "${FEEDLYNX_ADDRESS}:${FEEDLYNX_PORT:-8001}/info" | grep -q '"status":"ok"' || exit 1

RUN mkdir -p /data

COPY --from=builder /usr/src/feedlynx/target/release/feedlynx /usr/local/bin/feedlynx

VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/feedlynx"]
