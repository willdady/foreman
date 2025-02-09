FROM rust:1.84.1 AS builder
WORKDIR /usr/src/foreman
COPY . .
RUN cargo install --path .

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3
COPY --from=builder /usr/local/cargo/bin/foreman /usr/local/bin/foreman
CMD ["foreman"]
