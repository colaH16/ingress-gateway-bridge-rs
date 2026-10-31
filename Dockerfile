FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/release/bridge-controller /bridge-controller
COPY LICENSE /LICENSE
USER 65532:65532
EXPOSE 8081
ENTRYPOINT ["/bridge-controller"]
