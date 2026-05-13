#!/usr/bin/env python3
"""
Mock LLM upstream for load testing kooix-gate.

Returns instant (or configurable-latency) OpenAI-compatible chat completion
responses. Supports both non-streaming and SSE streaming modes.

Usage:
    python3 mock_upstream.py --port 9999 --latency 50
    python3 mock_upstream.py --port 9999 --latency 50 --workers 4
"""

import argparse
import json
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn

MOCK_RESPONSE_CONTENT = "This is a mock response from the load testing upstream."

MOCK_STREAM_CHUNKS = [
    "This is ",
    "a mock ",
    "streaming ",
    "response ",
    "from the ",
    "load testing ",
    "upstream.",
]


class Handler(BaseHTTPRequestHandler):
    """Handles POST /v1/chat/completions (and any other POST)."""

    latency_ms: int = 50

    def do_POST(self):
        content_len = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_len)

        try:
            req = json.loads(body) if body else {}
        except json.JSONDecodeError:
            req = {}

        model = req.get("model", "mock-model")
        stream = req.get("stream", False)

        # Simulate upstream latency
        if self.latency_ms > 0:
            time.sleep(self.latency_ms / 1000.0)

        if stream:
            self._handle_stream(model)
        else:
            self._handle_non_stream(model)

    def do_GET(self):
        """Health check endpoint."""
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok"}).encode())
        else:
            self.send_response(404)
            self.end_headers()

    def _handle_non_stream(self, model: str):
        completion_id = f"chatcmpl-mock-{uuid.uuid4().hex[:12]}"
        response = {
            "id": completion_id,
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": MOCK_RESPONSE_CONTENT,
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 25,
                "completion_tokens": 12,
                "total_tokens": 37,
            },
        }

        payload = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _handle_stream(self, model: str):
        completion_id = f"chatcmpl-mock-{uuid.uuid4().hex[:12]}"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.end_headers()

        # Send role chunk
        role_chunk = {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "delta": {"role": "assistant", "content": ""},
                    "finish_reason": None,
                }
            ],
        }
        self.wfile.write(f"data: {json.dumps(role_chunk)}\n\n".encode())
        self.wfile.flush()

        # Send content chunks
        for chunk_text in MOCK_STREAM_CHUNKS:
            chunk = {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": int(time.time()),
                "model": model,
                "choices": [
                    {
                        "index": 0,
                        "delta": {"content": chunk_text},
                        "finish_reason": None,
                    }
                ],
            }
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()

        # Send finish chunk with usage
        finish_chunk = {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 25,
                "completion_tokens": 12,
                "total_tokens": 37,
            },
        }
        self.wfile.write(f"data: {json.dumps(finish_chunk)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def log_message(self, format, *args):
        """Suppress per-request logging for perf."""
        pass


class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    """Handle each request in a separate thread for concurrency."""

    daemon_threads = True
    allow_reuse_address = True


def main():
    parser = argparse.ArgumentParser(description="Mock LLM upstream for load testing")
    parser.add_argument("--port", type=int, default=9999, help="Listen port (default: 9999)")
    parser.add_argument(
        "--latency",
        type=int,
        default=50,
        help="Simulated response latency in ms (default: 50)",
    )
    parser.add_argument("--host", default="0.0.0.0", help="Bind address (default: 0.0.0.0)")
    args = parser.parse_args()

    Handler.latency_ms = args.latency
    server = ThreadedHTTPServer((args.host, args.port), Handler)

    print(f"Mock LLM upstream listening on {args.host}:{args.port}")
    print(f"  latency: {args.latency}ms")
    print(f"  streaming: supported")
    print(f"  health: GET /health")
    print()

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nShutting down.")
        server.server_close()


if __name__ == "__main__":
    main()
