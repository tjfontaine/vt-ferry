#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)"

WORKDIR="${WORKDIR:-${REPO_ROOT}/artifacts/docker-pipe-overhead-$$}"
DOCKER_IMAGE="${DOCKER_IMAGE:-python:3.12-slim}"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/arm64}"
DOCKER_HOST_NAME="${DOCKER_HOST_NAME:-host.docker.internal}"
HOST_BIND_ADDR="${HOST_BIND_ADDR:-127.0.0.1}"
LATENCY_ITERS="${LATENCY_ITERS:-10000}"
LATENCY_WARMUP_ITERS="${LATENCY_WARMUP_ITERS:-200}"
LATENCY_PAYLOAD_BYTES="${LATENCY_PAYLOAD_BYTES:-64}"
BULK_BYTES="${BULK_BYTES:-67108864}"
BULK_CHUNK_BYTES="${BULK_CHUNK_BYTES:-262144}"
REPEATS="${REPEATS:-3}"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_cmd python3
require_cmd docker
docker info >/dev/null 2>&1 || die "docker is installed, but the Docker daemon is not reachable"

mkdir -p "${WORKDIR}/results"

BENCH_PY="${WORKDIR}/docker_pipe_bench.py"
READY_JSON="${WORKDIR}/server-ready.json"
SERVER_LOG="${WORKDIR}/server.log"
UNIX_SOCKET="${WORKDIR}/host-worker.sock"
BRIDGE_READY_JSON="${WORKDIR}/bridge-ready.json"
BRIDGE_LOG="${WORKDIR}/bridge.log"
SUMMARY_JSON="${WORKDIR}/summary.json"

cat >"${BENCH_PY}" <<'PY'
#!/usr/bin/env python3
import argparse
import json
import os
import socket
import struct
import threading
import time


def read_exact(sock, size):
    data = bytearray(size)
    view = memoryview(data)
    offset = 0
    while offset < size:
        n = sock.recv_into(view[offset:])
        if n == 0:
            raise EOFError("socket closed")
        offset += n
    return data


def discard_exact(sock, size):
    scratch = bytearray(1024 * 1024)
    view = memoryview(scratch)
    remaining = size
    while remaining:
        n = sock.recv_into(view[: min(len(scratch), remaining)])
        if n == 0:
            raise EOFError("socket closed")
        remaining -= n


def handle_client(conn):
    with conn:
        while True:
            op = conn.recv(1)
            if not op:
                return
            if op == b"P":
                payload_len = struct.unpack("!I", read_exact(conn, 4))[0]
                payload = read_exact(conn, payload_len)
                conn.sendall(b"p")
                if payload:
                    conn.sendall(payload)
            elif op == b"U":
                total = struct.unpack("!Q", read_exact(conn, 8))[0]
                discard_exact(conn, total)
                conn.sendall(b"u")
            elif op == b"D":
                total = struct.unpack("!Q", read_exact(conn, 8))[0]
                chunk = b"x" * (256 * 1024)
                remaining = total
                while remaining:
                    part = chunk[: min(len(chunk), remaining)]
                    conn.sendall(part)
                    remaining -= len(part)
            else:
                raise RuntimeError(f"unknown op {op!r}")


def accept_loop(listener):
    while True:
        try:
            conn, _ = listener.accept()
        except OSError:
            return
        thread = threading.Thread(target=handle_client, args=(conn,), daemon=True)
        thread.start()


def open_outbound(kind, host=None, port=None, unix_socket=None):
    if kind == "unix":
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(unix_socket)
        return sock
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    sock.connect((host, port))
    return sock


def relay_one_way(src, dst):
    try:
        while True:
            data = src.recv(256 * 1024)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle_proxy_client(inbound, args):
    outbound = open_outbound(
        args.connect_kind,
        host=args.connect_host,
        port=args.connect_port,
        unix_socket=args.connect_unix,
    )
    with inbound, outbound:
        left = threading.Thread(target=relay_one_way, args=(inbound, outbound), daemon=True)
        right = threading.Thread(target=relay_one_way, args=(outbound, inbound), daemon=True)
        left.start()
        right.start()
        left.join()
        right.join()


def proxy_accept_loop(listener, args):
    while True:
        try:
            conn, _ = listener.accept()
        except OSError:
            return
        thread = threading.Thread(target=handle_proxy_client, args=(conn, args), daemon=True)
        thread.start()


def server(args):
    listeners = []
    ready = {}

    if args.tcp_bind:
        tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        tcp.bind((args.tcp_bind, args.tcp_port))
        tcp.listen(64)
        ready["tcp_bind"] = args.tcp_bind
        ready["tcp_port"] = tcp.getsockname()[1]
        listeners.append(tcp)

    if args.unix_socket:
        try:
            os.unlink(args.unix_socket)
        except FileNotFoundError:
            pass
        unix = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        unix.bind(args.unix_socket)
        unix.listen(64)
        ready["unix_socket"] = args.unix_socket
        listeners.append(unix)

    if not listeners:
        raise SystemExit("server needs at least one listener")

    for listener in listeners:
        thread = threading.Thread(target=accept_loop, args=(listener,), daemon=True)
        thread.start()

    with open(args.ready_json, "w", encoding="utf-8") as f:
        json.dump(ready, f, sort_keys=True)
        f.write("\n")

    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return


def proxy(args):
    if args.listen_kind == "unix":
        try:
            os.unlink(args.listen_unix)
        except FileNotFoundError:
            pass
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(args.listen_unix)
        listener.listen(64)
        ready = {"listen_kind": "unix", "listen_unix": args.listen_unix}
    else:
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.listen_host, args.listen_port))
        listener.listen(64)
        ready = {
            "listen_kind": "tcp",
            "listen_host": args.listen_host,
            "listen_port": listener.getsockname()[1],
        }

    thread = threading.Thread(target=proxy_accept_loop, args=(listener, args), daemon=True)
    thread.start()

    with open(args.ready_json, "w", encoding="utf-8") as f:
        json.dump(ready, f, sort_keys=True)
        f.write("\n")

    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return


def connect_client(args):
    if args.kind == "unix":
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(args.unix_socket)
        return sock

    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    sock.connect((args.host, args.port))
    return sock


def percentile(samples, pct):
    if not samples:
        return 0.0
    ordered = sorted(samples)
    index = int(round((pct / 100.0) * (len(ordered) - 1)))
    return ordered[index] / 1000.0


def client(args):
    payload = b"a" * args.latency_payload_bytes
    ping = b"P" + struct.pack("!I", len(payload)) + payload
    expected = 1 + len(payload)
    upload_chunk = b"u" * args.bulk_chunk_bytes

    sock = connect_client(args)
    with sock:
        for _ in range(args.latency_warmup_iters):
            sock.sendall(ping)
            read_exact(sock, expected)

        samples_ns = []
        latency_start = time.perf_counter_ns()
        for _ in range(args.latency_iters):
            start = time.perf_counter_ns()
            sock.sendall(ping)
            read_exact(sock, expected)
            samples_ns.append(time.perf_counter_ns() - start)
        latency_total_ns = time.perf_counter_ns() - latency_start

        upload_start = time.perf_counter_ns()
        sock.sendall(b"U" + struct.pack("!Q", args.bulk_bytes))
        remaining = args.bulk_bytes
        while remaining:
            part = upload_chunk[: min(len(upload_chunk), remaining)]
            sock.sendall(part)
            remaining -= len(part)
        read_exact(sock, 1)
        upload_ns = time.perf_counter_ns() - upload_start

        download_start = time.perf_counter_ns()
        sock.sendall(b"D" + struct.pack("!Q", args.bulk_bytes))
        discard_exact(sock, args.bulk_bytes)
        download_ns = time.perf_counter_ns() - download_start

    mib = args.bulk_bytes / (1024 * 1024)
    result = {
        "label": args.label,
        "kind": args.kind,
        "repeat": args.repeat,
        "latency_iters": args.latency_iters,
        "latency_payload_bytes": args.latency_payload_bytes,
        "latency_total_ms": latency_total_ns / 1_000_000,
        "latency_avg_us": (latency_total_ns / args.latency_iters) / 1000,
        "latency_p50_us": percentile(samples_ns, 50),
        "latency_p95_us": percentile(samples_ns, 95),
        "latency_p99_us": percentile(samples_ns, 99),
        "bulk_bytes": args.bulk_bytes,
        "bulk_chunk_bytes": args.bulk_chunk_bytes,
        "upload_ms": upload_ns / 1_000_000,
        "upload_mib_s": mib / (upload_ns / 1_000_000_000),
        "download_ms": download_ns / 1_000_000,
        "download_mib_s": mib / (download_ns / 1_000_000_000),
    }

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as f:
            json.dump(result, f, indent=2, sort_keys=True)
            f.write("\n")
    print(json.dumps(result, sort_keys=True))


def mean(values):
    return sum(values) / len(values)


def summary(args):
    rows = []
    for path in args.results:
        with open(path, "r", encoding="utf-8") as f:
            rows.append(json.load(f))
    by_label = {}
    for row in rows:
        by_label.setdefault(row["label"], []).append(row)

    summary_rows = []
    for label, label_rows in sorted(by_label.items()):
        summary_rows.append(
            {
                "label": label,
                "samples": len(label_rows),
                "latency_avg_us": mean([r["latency_avg_us"] for r in label_rows]),
                "latency_p50_us": mean([r["latency_p50_us"] for r in label_rows]),
                "latency_p95_us": mean([r["latency_p95_us"] for r in label_rows]),
                "latency_p99_us": mean([r["latency_p99_us"] for r in label_rows]),
                "upload_mib_s": mean([r["upload_mib_s"] for r in label_rows]),
                "download_mib_s": mean([r["download_mib_s"] for r in label_rows]),
            }
        )

    doc = {
        "results": rows,
        "summary": summary_rows,
    }
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as f:
            json.dump(doc, f, indent=2, sort_keys=True)
            f.write("\n")

    label_width = max(12, *(len(row["label"]) for row in summary_rows))
    print(
        f"{'label':<{label_width}} samples  rtt_avg_us  rtt_p50_us  "
        "rtt_p95_us  rtt_p99_us  upload_mib_s  download_mib_s"
    )
    for row in summary_rows:
        print(
            f"{row['label']:<{label_width}} {row['samples']:>7} "
            f"{row['latency_avg_us']:>11.2f} {row['latency_p50_us']:>11.2f} "
            f"{row['latency_p95_us']:>11.2f} {row['latency_p99_us']:>11.2f} "
            f"{row['upload_mib_s']:>13.1f} {row['download_mib_s']:>15.1f}"
        )

    labels = {row["label"]: row for row in summary_rows}
    if "docker-tcp" in labels and "host-tcp" in labels:
        docker = labels["docker-tcp"]
        host = labels["host-tcp"]
        print("")
        print(f"docker_tcp_rtt_over_host_tcp: {docker['latency_avg_us'] / host['latency_avg_us']:.3f}x")
        print(f"docker_tcp_upload_vs_host_tcp: {docker['upload_mib_s'] / host['upload_mib_s']:.3f}x")
        print(f"docker_tcp_download_vs_host_tcp: {docker['download_mib_s'] / host['download_mib_s']:.3f}x")
    if "docker-tcp" in labels and "host-uds" in labels:
        docker = labels["docker-tcp"]
        host = labels["host-uds"]
        print(f"docker_tcp_rtt_over_host_uds: {docker['latency_avg_us'] / host['latency_avg_us']:.3f}x")
    if "docker-tcp-uds" in labels and "host-uds" in labels:
        docker = labels["docker-tcp-uds"]
        host = labels["host-uds"]
        print(f"docker_tcp_uds_rtt_over_host_uds: {docker['latency_avg_us'] / host['latency_avg_us']:.3f}x")
    if "docker-uds-tcp-uds" in labels and "docker-tcp-uds" in labels:
        uds = labels["docker-uds-tcp-uds"]
        tcp = labels["docker-tcp-uds"]
        print(f"docker_inner_uds_proxy_rtt_over_docker_tcp_uds: {uds['latency_avg_us'] / tcp['latency_avg_us']:.3f}x")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    server_parser = sub.add_parser("server")
    server_parser.add_argument("--tcp-bind", default=None)
    server_parser.add_argument("--tcp-port", type=int, default=0)
    server_parser.add_argument("--unix-socket", default=None)
    server_parser.add_argument("--ready-json", required=True)
    server_parser.set_defaults(func=server)

    proxy_parser = sub.add_parser("proxy")
    proxy_parser.add_argument("--listen-kind", choices=["tcp", "unix"], required=True)
    proxy_parser.add_argument("--listen-host", default="127.0.0.1")
    proxy_parser.add_argument("--listen-port", type=int, default=0)
    proxy_parser.add_argument("--listen-unix", default=None)
    proxy_parser.add_argument("--connect-kind", choices=["tcp", "unix"], required=True)
    proxy_parser.add_argument("--connect-host", default="127.0.0.1")
    proxy_parser.add_argument("--connect-port", type=int, default=0)
    proxy_parser.add_argument("--connect-unix", default=None)
    proxy_parser.add_argument("--ready-json", required=True)
    proxy_parser.set_defaults(func=proxy)

    client_parser = sub.add_parser("client")
    client_parser.add_argument("--kind", choices=["tcp", "unix"], required=True)
    client_parser.add_argument("--host", default="127.0.0.1")
    client_parser.add_argument("--port", type=int, default=0)
    client_parser.add_argument("--unix-socket", default=None)
    client_parser.add_argument("--label", required=True)
    client_parser.add_argument("--repeat", type=int, default=1)
    client_parser.add_argument("--latency-iters", type=int, required=True)
    client_parser.add_argument("--latency-warmup-iters", type=int, required=True)
    client_parser.add_argument("--latency-payload-bytes", type=int, required=True)
    client_parser.add_argument("--bulk-bytes", type=int, required=True)
    client_parser.add_argument("--bulk-chunk-bytes", type=int, required=True)
    client_parser.add_argument("--json-out", default=None)
    client_parser.set_defaults(func=client)

    summary_parser = sub.add_parser("summary")
    summary_parser.add_argument("--json-out", default=None)
    summary_parser.add_argument("results", nargs="+")
    summary_parser.set_defaults(func=summary)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
PY
chmod +x "${BENCH_PY}"

python3 "${BENCH_PY}" server \
  --tcp-bind "${HOST_BIND_ADDR}" \
  --tcp-port 0 \
  --unix-socket "${UNIX_SOCKET}" \
  --ready-json "${READY_JSON}" \
  >"${SERVER_LOG}" 2>&1 &
SERVER_PID="$!"
BRIDGE_PID=""

cleanup() {
  if [ -n "${BRIDGE_PID}" ] && kill -0 "${BRIDGE_PID}" 2>/dev/null; then
    kill "${BRIDGE_PID}" 2>/dev/null || true
    wait "${BRIDGE_PID}" 2>/dev/null || true
  fi
  if kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

i=0
while [ ! -f "${READY_JSON}" ]; do
  i=$((i + 1))
  if [ "${i}" -gt 100 ]; then
    cat "${SERVER_LOG}" >&2 || true
    die "server did not become ready"
  fi
  sleep 0.05
done

TCP_PORT="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["tcp_port"])' "${READY_JSON}")"

python3 "${BENCH_PY}" proxy \
  --listen-kind tcp \
  --listen-host "${HOST_BIND_ADDR}" \
  --listen-port 0 \
  --connect-kind unix \
  --connect-unix "${UNIX_SOCKET}" \
  --ready-json "${BRIDGE_READY_JSON}" \
  >"${BRIDGE_LOG}" 2>&1 &
BRIDGE_PID="$!"

i=0
while [ ! -f "${BRIDGE_READY_JSON}" ]; do
  i=$((i + 1))
  if [ "${i}" -gt 100 ]; then
    cat "${BRIDGE_LOG}" >&2 || true
    die "bridge did not become ready"
  fi
  sleep 0.05
done

BRIDGE_PORT="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["listen_port"])' "${BRIDGE_READY_JSON}")"

run_host_client() {
  label="$1"
  kind="$2"
  repeat="$3"
  out="${WORKDIR}/results/${label}.${repeat}.json"
  if [ "${kind}" = "unix" ]; then
    python3 "${BENCH_PY}" client \
      --kind unix \
      --unix-socket "${UNIX_SOCKET}" \
      --label "${label}" \
      --repeat "${repeat}" \
      --latency-iters "${LATENCY_ITERS}" \
      --latency-warmup-iters "${LATENCY_WARMUP_ITERS}" \
      --latency-payload-bytes "${LATENCY_PAYLOAD_BYTES}" \
      --bulk-bytes "${BULK_BYTES}" \
      --bulk-chunk-bytes "${BULK_CHUNK_BYTES}" \
      --json-out "${out}" >/dev/null
  else
    python3 "${BENCH_PY}" client \
      --kind tcp \
      --host "${HOST_BIND_ADDR}" \
      --port "${TCP_PORT}" \
      --label "${label}" \
      --repeat "${repeat}" \
      --latency-iters "${LATENCY_ITERS}" \
      --latency-warmup-iters "${LATENCY_WARMUP_ITERS}" \
      --latency-payload-bytes "${LATENCY_PAYLOAD_BYTES}" \
      --bulk-bytes "${BULK_BYTES}" \
      --bulk-chunk-bytes "${BULK_CHUNK_BYTES}" \
      --json-out "${out}" >/dev/null
  fi
}

run_host_tcp_client() {
  label="$1"
  port="$2"
  repeat="$3"
  out="${WORKDIR}/results/${label}.${repeat}.json"
  python3 "${BENCH_PY}" client \
    --kind tcp \
    --host "${HOST_BIND_ADDR}" \
    --port "${port}" \
    --label "${label}" \
    --repeat "${repeat}" \
    --latency-iters "${LATENCY_ITERS}" \
    --latency-warmup-iters "${LATENCY_WARMUP_ITERS}" \
    --latency-payload-bytes "${LATENCY_PAYLOAD_BYTES}" \
    --bulk-bytes "${BULK_BYTES}" \
    --bulk-chunk-bytes "${BULK_CHUNK_BYTES}" \
    --json-out "${out}" >/dev/null
}

run_docker_client() {
  label="$1"
  port="$2"
  repeat="$3"
  out="${WORKDIR}/results/${label}.${repeat}.json"
  docker run --rm \
    --platform "${DOCKER_PLATFORM}" \
    -v "${BENCH_PY}:/bench/docker_pipe_bench.py:ro" \
    -v "${WORKDIR}/results:/bench/results" \
    "${DOCKER_IMAGE}" \
    python3 /bench/docker_pipe_bench.py client \
      --kind tcp \
      --host "${DOCKER_HOST_NAME}" \
      --port "${port}" \
      --label "${label}" \
      --repeat "${repeat}" \
      --latency-iters "${LATENCY_ITERS}" \
      --latency-warmup-iters "${LATENCY_WARMUP_ITERS}" \
      --latency-payload-bytes "${LATENCY_PAYLOAD_BYTES}" \
      --bulk-bytes "${BULK_BYTES}" \
      --bulk-chunk-bytes "${BULK_CHUNK_BYTES}" \
      --json-out "/bench/results/${label}.${repeat}.json" >/dev/null
}

run_docker_uds_proxy_client() {
  repeat="$1"
  docker run --rm \
    --platform "${DOCKER_PLATFORM}" \
    -v "${BENCH_PY}:/bench/docker_pipe_bench.py:ro" \
    -v "${WORKDIR}/results:/bench/results" \
    "${DOCKER_IMAGE}" \
    sh -c '
      set -eu
      sock="/tmp/vt-ferry-docker-pipe.sock"
      ready="/tmp/vt-ferry-docker-pipe-ready.json"
      log="/tmp/vt-ferry-docker-pipe-proxy.log"
      rm -f "$sock" "$ready"
      python3 /bench/docker_pipe_bench.py proxy \
        --listen-kind unix \
        --listen-unix "$sock" \
        --connect-kind tcp \
        --connect-host "$0" \
        --connect-port "$1" \
        --ready-json "$ready" >"$log" 2>&1 &
      pid="$!"
      i=0
      while [ ! -f "$ready" ]; do
        i=$((i + 1))
        if [ "$i" -gt 100 ]; then
          cat "$log" >&2 || true
          exit 1
        fi
        sleep 0.05
      done
      set +e
      python3 /bench/docker_pipe_bench.py client \
        --kind unix \
        --unix-socket "$sock" \
        --label docker-uds-tcp-uds \
        --repeat "$2" \
        --latency-iters "$3" \
        --latency-warmup-iters "$4" \
        --latency-payload-bytes "$5" \
        --bulk-bytes "$6" \
        --bulk-chunk-bytes "$7" \
        --json-out "/bench/results/docker-uds-tcp-uds.$2.json" >/dev/null
      rc="$?"
      set -e
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      exit "$rc"
    ' "${DOCKER_HOST_NAME}" "${BRIDGE_PORT}" "${repeat}" "${LATENCY_ITERS}" "${LATENCY_WARMUP_ITERS}" "${LATENCY_PAYLOAD_BYTES}" "${BULK_BYTES}" "${BULK_CHUNK_BYTES}"
}

echo "workdir: ${WORKDIR}"
echo "docker image: ${DOCKER_IMAGE} (${DOCKER_PLATFORM})"
echo "host tcp: ${HOST_BIND_ADDR}:${TCP_PORT}"
echo "host tcp->uds bridge: ${HOST_BIND_ADDR}:${BRIDGE_PORT} -> ${UNIX_SOCKET}"
echo "docker target: ${DOCKER_HOST_NAME}:${TCP_PORT}"
echo "docker bridge target: ${DOCKER_HOST_NAME}:${BRIDGE_PORT}"
echo "latency: ${LATENCY_ITERS} iterations, ${LATENCY_PAYLOAD_BYTES} byte payload"
echo "bulk: ${BULK_BYTES} bytes, ${BULK_CHUNK_BYTES} byte chunks"
echo ""

repeat=1
while [ "${repeat}" -le "${REPEATS}" ]; do
  echo "repeat ${repeat}/${REPEATS}: host UDS"
  run_host_client host-uds unix "${repeat}"
  echo "repeat ${repeat}/${REPEATS}: host TCP"
  run_host_tcp_client host-tcp "${TCP_PORT}" "${repeat}"
  echo "repeat ${repeat}/${REPEATS}: docker TCP"
  run_docker_client docker-tcp "${TCP_PORT}" "${repeat}"
  echo "repeat ${repeat}/${REPEATS}: host TCP -> host UDS bridge"
  run_host_tcp_client host-tcp-uds "${BRIDGE_PORT}" "${repeat}"
  echo "repeat ${repeat}/${REPEATS}: docker TCP -> host UDS bridge"
  run_docker_client docker-tcp-uds "${BRIDGE_PORT}" "${repeat}"
  echo "repeat ${repeat}/${REPEATS}: docker UDS -> TCP -> host UDS bridge"
  run_docker_uds_proxy_client "${repeat}"
  repeat=$((repeat + 1))
done

python3 "${BENCH_PY}" summary \
  --json-out "${SUMMARY_JSON}" \
  "${WORKDIR}"/results/*.json

echo ""
echo "summary: ${SUMMARY_JSON}"
echo "server log: ${SERVER_LOG}"
echo "bridge log: ${BRIDGE_LOG}"
