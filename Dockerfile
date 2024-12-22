FROM alpine:latest
HEALTHCHECK --interval=15s --timeout=5s --start-period=3s --retries=3 CMD [ "true" ]