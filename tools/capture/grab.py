#!/usr/bin/env python3
"""Send a fixed set of raw HTTP probes to a server and save each response
byte for byte. Used to capture stock output of real software (for the
templates) and to compare a trap persona against the real thing.

    grab.py HOST PORT OUTDIR [--host-header NAME] [--extra PATH ...]

Run it only against servers you run yourself.
"""
import argparse
import os
import socket
import time

PROBES = {
    "get-root": b"GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Mozilla/5.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
    "head-root": b"HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "get-root-http10": b"GET / HTTP/1.0\r\n\r\n",
    "get-missing": b"GET /no-such-file-7f3a.php HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "get-missing-dir": b"GET /no-such-dir-7f3a/ HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "get-dotfile": b"GET /.ht-probe HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "no-host": b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n",
    "garbage": b"\x16\x03\x01\x00\xa5\x01\x00\x00\xa1\x03\x03garbage\r\n\r\n",
    "bad-version": b"GET / HTTP/2.0\r\nHost: {host}\r\n\r\n",
    "traversal": b"GET /../../etc/passwd HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "long-uri": b"GET /" + b"a" * 20000 + b" HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "options": b"OPTIONS / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "options-star": b"OPTIONS * HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "propfind": b"PROPFIND / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "unknown-method": b"FOO / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
    "post-root": b"POST / HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 3\r\nConnection: close\r\n\r\na=1",
    "keepalive-pair": b"GET / HTTP/1.1\r\nHost: {host}\r\n\r\nGET /no-such-file-7f3a.php HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
}


def send(addr, payload, timeout=10.0):
    s = socket.create_connection(addr, timeout=timeout)
    try:
        try:
            s.sendall(payload)
        except OSError:
            pass
        out = b""
        start = time.time()
        while time.time() - start < timeout:
            try:
                chunk = s.recv(65536)
            except socket.timeout:
                break
            except OSError:
                break
            if not chunk:
                break
            out += chunk
        return out
    finally:
        s.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("host")
    ap.add_argument("port", type=int)
    ap.add_argument("outdir")
    ap.add_argument("--host-header", default="localhost")
    ap.add_argument("--extra", nargs="*", default=[], help="more GET paths to save")
    a = ap.parse_args()
    os.makedirs(a.outdir, exist_ok=True)
    addr = (a.host, a.port)
    probes = dict(PROBES)
    for i, path in enumerate(a.extra):
        probes[f"extra-{i:02d}" + path.replace("/", "_")[:60]] = (
            f"GET {path} HTTP/1.1\r\nHost: {{host}}\r\nUser-Agent: Mozilla/5.0\r\nConnection: close\r\n\r\n".encode()
        )
    for name, raw in probes.items():
        payload = raw.replace(b"{host}", a.host_header.encode())
        with open(os.path.join(a.outdir, name + ".raw"), "wb") as f:
            f.write(send(addr, payload))
    print(f"saved {len(probes)} responses to {a.outdir}")


if __name__ == "__main__":
    main()
