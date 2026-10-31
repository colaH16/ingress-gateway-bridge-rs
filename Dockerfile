FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/release/bridge-controller /bridge-controller
COPY LICENSE /LICENSE
USER 65532:65532
ENV BRIDGE_CONFIG=/etc/bridge/bridge.yaml \
    BRIDGE_DRY_RUN=true \
    BRIDGE_SINGLE_WRITER=false \
    BRIDGE_HEALTH_PROBE_BIND_ADDRESS=0.0.0.0:8081 \
    BRIDGE_ONCE=false
EXPOSE 8081
ENTRYPOINT ["/bridge-controller"]
