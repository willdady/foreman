FROM denoland/deno:2.1.5

HEALTHCHECK --interval=5s --timeout=2s --start-period=3s --retries=3 CMD /usr/bin/true

WORKDIR /app

USER deno

COPY *.ts .

RUN deno cache index.ts

CMD ["run", "--allow-net", "--allow-env", "index.ts"]
