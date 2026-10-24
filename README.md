# Prometheus NUT Exporter

[![GitHub release](https://img.shields.io/github/v/release/SeiyaGame/prometheus-nut-exporter?label=Version)](https://github.com/SeiyaGame/prometheus-nut-exporter/releases)
[![CI](https://github.com/SeiyaGame/prometheus-nut-exporter/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/SeiyaGame/prometheus-nut-exporter/actions/workflows/ci.yml)

> [!NOTE]
> This is a maintained fork of [HON95/prometheus-nut-exporter](https://github.com/HON95/prometheus-nut-exporter),
> which is no longer maintained. Dependencies are kept up to date, and images and binaries are published on
> GitHub only: `ghcr.io/seiyagame/prometheus-nut-exporter` and the
> [releases](https://github.com/SeiyaGame/prometheus-nut-exporter/releases) (nothing on Docker Hub).
> The metrics and the configuration are unchanged: replace `hon95/prometheus-nut-exporter` with
> `ghcr.io/seiyagame/prometheus-nut-exporter` to switch.

![Dashboard](https://grafana.com/api/dashboards/14371/images/10335/image)

A Prometheus/OpenMetrics exporter for uninterruptable power supplies (UPSes) using Network UPS Tools (NUT).

## Usage

### NUT

Set up NUT in server mode and make sure the TCP port (3493 by default) is accessible (without authentication).

If you want to test that it's working, run `telnet <nut-server> 3493` and then `VER`, `LIST UPS` and `LIST VAR <ups>`.

### Docker

[`docker-compose.yml`](docker-compose.yml):

```bash
curl -fsSLO https://raw.githubusercontent.com/SeiyaGame/prometheus-nut-exporter/master/docker-compose.yml
docker compose up -d
```

The container runs as an unprivileged user (`5000:5000`) and has a health check on the home page.

#### Hardened mode

The exporter only opens TCP connections to the NUT servers and answers HTTP, so it runs without anything else:
read-only filesystem, all capabilities dropped, `no-new-privileges`, memory, CPU and process limits, and the
port bound to `127.0.0.1` only. Example: [`docker-compose.hardened.yml`](docker-compose.hardened.yml).

```bash
curl -fsSLO https://raw.githubusercontent.com/SeiyaGame/prometheus-nut-exporter/master/docker-compose.hardened.yml
docker compose -f docker-compose.hardened.yml up -d
```

- When Prometheus runs on another machine, remove `127.0.0.1` from `ports:` (or put both containers on
  the same Docker network and drop `ports:`).
- `TOKIO_WORKER_THREADS=2` keeps the thread count under `pids_limit` on hosts with many CPUs.
- The health check calls `127.0.0.1`: keep `HTTP_ADDRESS` at its default (`::`) or set it to `0.0.0.0`.

### Binary

Static Linux binaries (no dependencies) for `amd64`, `arm64`, `armv7`, `armv6` and `386` are attached to each
[release](https://github.com/SeiyaGame/prometheus-nut-exporter/releases), with their SHA-256:

```bash
curl -fsSLO https://github.com/SeiyaGame/prometheus-nut-exporter/releases/latest/download/prometheus-nut-exporter-linux-amd64
chmod +x prometheus-nut-exporter-linux-amd64
HTTP_PATH=/metrics ./prometheus-nut-exporter-linux-amd64
```

### Prometheus

Example `prometheus.yml`:

```yaml
global:
    scrape_interval: 15s
    scrape_timeout: 10s

scrape_configs:
  - job_name: "nut"
    static_configs:
      # Insert NUT server address here
      - targets: ["nut-server:3493"]
    relabel_configs:
      - source_labels: [__address__]
        target_label: __param_target
      - source_labels: [__param_target]
        target_label: instance
      - target_label: __address__
        # Insert NUT exporter address here
        replacement: nut-exporter:9995
```

In the above example, `nut-exporter:9995` is the address and port of the NUT _exporter_ while `nut-server:3493` is the address and port of the NUT _server_ to query through the exporter. Add `metrics_path: /metrics` if the exporter runs with `HTTP_PATH=/metrics`.

A scrape gives up after the `scrape_timeout` sent by Prometheus (10 seconds without it), so an unreachable NUT server makes the scrape fail instead of hanging.

### Kubernetes Resource Usage

Example container resources requests and limits.
This was done by scraping one NUT server with two UPSes.
Resource usage was observed across 7 days period.
This can be lowered even more but should be sufficient as a starting point.

```yaml
resources:
  limits:
    cpu: "10m"
    memory: "16Mi"
  requests:
    cpu: "1m"
    memory: "8Mi"
```

### Grafana

[Example dashboard](https://grafana.com/grafana/dashboards/14371)

## Configuration

### Docker Image Versions

Images are published on `ghcr.io/seiyagame/prometheus-nut-exporter` for `linux/amd64`, `linux/arm64`,
`linux/arm/v7`, `linux/arm/v6` and `linux/386`:

- `1`, `1.Y` or `1.Y.Z` for releases (`1` follows the stable v1 releases), `latest` for the last release.
- `dev` for the master branch, rebuilt on every change (dependency updates included). The `dev`
  [pre-release](https://github.com/SeiyaGame/prometheus-nut-exporter/releases/tag/dev) has the same binaries.

### Environment Variables

- `RUST_LOG` (defaults to `info`): The log level used by the console/STDOUT. Set to `debug` to show HTTP requests and failed scrapes, and `trace` to show the NUT protocol exchanges.
- `HTTP_ADDRESS` (defaults to `::`): The HTTP server will listen on this IP. Set to `127.0.0.1` or `::1` to only allow local access.
- `HTTP_PORT` (defaults to `9995`): The HTTP server port.
- `HTTP_PATH` (defaults to `/nut`): The HTTP server metrics path. You may want to set it to `/metrics` on new setups to avoid extra Prometheus configuration (not changed here due to compatibility).
- `PRINT_METRICS_AND_EXIT` (defaults to `false`): Print a Markdown-formatted table consisting of all metrics and then immediately exit. Used to update [metrics.md](metrics.md).

Invalid values are ignored with a warning in the log, and the default value is used.

## Metrics

See [metrics](metrics.md).

## Development

```bash
cargo run                        # listens on :9995, e.g. curl 'localhost:9995/nut?target=nut-server'
cargo fmt && cargo clippy --all-targets -- -D warnings
cargo test                       # unit tests and tests/exporter.rs
docker build -t prometheus-nut-exporter .
```

```
src/
  main.rs         startup, shutdown on SIGINT/SIGTERM
  config.rs       environment variables
  server.rs       HTTP endpoints (axum), target parsing, scrape timeout
  nut.rs          NUT protocol client
  metrics.rs      definitions of all metrics (add new ones here, then update metrics.md)
  openmetrics.rs  OpenMetrics output
tests/
  exporter.rs     end-to-end: fake NUT server serving tests/fixtures/, output compared to expected.txt
```

`tests/fixtures/` holds answers of real NUT servers. After an intended change of the output,
`UPDATE_SNAPSHOTS=1 cargo test` rewrites `tests/fixtures/expected.txt`: check its diff before committing.

The CI (`.github/workflows/ci.yml`) runs the checks and the tests, checks the minimum Rust version (`rust-version`), builds the static musl binaries
for every architecture and packages them in the multi-arch Docker image. Each push to master replaces the `dev`
pre-release and the `:dev` image. Pushing a `v*` tag publishes the release (binaries, release notes,
`X.Y.Z`, `X.Y`, `X` and `latest` images).

Dependabot opens weekly updates (crates, GitHub Actions, base images). Minor and patch updates of the crates
and the base images are merged automatically once the CI passes, which publishes a new `dev` build; majors and
GitHub Actions updates (pinned to a commit SHA) wait for a review.

## License

GNU General Public License version 3 (GPLv3).
