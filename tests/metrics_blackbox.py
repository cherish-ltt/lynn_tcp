"""Real-process TCP + HTTP smoke test: python3 tests/metrics_blackbox.py EXAMPLE_BIN."""

import concurrent.futures
import json
import socket
import struct
import subprocess
import sys
import time
import urllib.request


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def receive(sock, length):
    data = b""
    while len(data) < length:
        chunk = sock.recv(length - len(data))
        assert chunk, "unexpected EOF"
        data += chunk
    return data


def run(binary):
    port, metrics_port = free_port(), free_port()
    process = subprocess.Popen(
        [binary, f"127.0.0.1:{port}", f"127.0.0.1:{metrics_port}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    try:
        url = f"http://127.0.0.1:{metrics_port}"

        def metrics():
            with urllib.request.urlopen(url + "/metrics", timeout=2) as response:
                assert response.status == 200
                lines = response.read().decode().splitlines()
            return {
                line.split()[0]: float(line.split()[1])
                for line in lines
                if line and not line.startswith("#")
            }

        deadline = time.monotonic() + 5
        while True:
            assert process.poll() is None, process.stderr.read().decode()
            try:
                initial = metrics()
                with socket.create_connection(("127.0.0.1", port), timeout=1):
                    pass
                break
            except OSError:
                assert time.monotonic() < deadline, "startup timeout"
                time.sleep(0.01)
        assert initial["lynn_connections_total"] == 0
        with urllib.request.urlopen(url + "/health", timeout=2) as response:
            assert response.read() == b"OK"

        def client(index):
            payload = f"client-{index}".encode()
            frame = struct.pack("<HQBH", 9177, len(payload) + 5, 1, 1) + payload + struct.pack("<H", 7719)
            with socket.create_connection(("127.0.0.1", port), timeout=3) as peer:
                for _ in range(100):
                    peer.sendall(frame)
                    assert receive(peer, len(frame)) == frame
            return len(frame) * 100

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            byte_count = sum(pool.map(client, range(4)))
        deadline = time.monotonic() + 5
        while True:
            values = metrics()
            if values["lynn_connections_active"] == 0:
                break
            assert time.monotonic() < deadline, values
            time.sleep(0.01)
        expected = {
            "lynn_connections_total": 5,
            "lynn_connections_closed_total": 5,
            "lynn_messages_received_total": 400,
            "lynn_messages_sent_total": 400,
            "lynn_network_bytes_received_total": byte_count,
            "lynn_network_bytes_sent_total": byte_count,
        }
        for name, value in expected.items():
            assert values[name] == value, (name, values[name], value)
        assert process.poll() is None
        print(json.dumps({"status": "passed", "expected_and_observed": expected}))
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        process.stderr.close()


if __name__ == "__main__":
    run(sys.argv[1])
