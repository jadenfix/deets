"""Tests for Python SDK RPC methods added for TypeScript parity.

Covers: get_block_by_number, get_block_by_hash, get_transaction_receipt,
        get_account, get_state_root, get_health.
"""
from __future__ import annotations

import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, Optional

import pytest

from aether_sdk import AetherClient, NodeHealth, RpcBlock, RpcReceipt

# ─── test server helpers ──────────────────────────────────────────────────────

_SAMPLE_BLOCK: Dict[str, Any] = {
    "header": {"slot": 42, "timestamp": 1_700_000_000, "proposer": None},
    "transactions": [{"hash": "0x" + "aa" * 32}],
}

_SAMPLE_RECEIPT: Dict[str, Any] = {
    "tx_hash": "0x" + "bb" * 32,
    "block_hash": "0x" + "cc" * 32,
    "slot": 42,
    "status": "success",
}

_SAMPLE_ACCOUNT: Dict[str, Any] = {
    "address": "0x" + "11" * 20,
    "balance": "1000000",
    "nonce": 3,
}

_SAMPLE_STATE_ROOT = "0x" + "dd" * 32

_SAMPLE_HEALTH: Dict[str, Any] = {
    "status": "ok",
    "version": "0.1.0",
    "latestSlot": 99,
    "finalizedSlot": 95,
    "peerCount": 7,
    "sync": {"syncing": False},
}


@contextmanager
def rpc_server():
    """Minimal HTTP server that handles JSON-RPC POST and GET /health."""
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            if self.path == "/health":
                encoded = json.dumps(_SAMPLE_HEALTH).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)
            else:
                self.send_response(404)
                self.end_headers()

        def do_POST(self):  # noqa: N802
            content_len = int(self.headers.get("content-length", 0))
            payload = json.loads(self.rfile.read(content_len).decode())
            method = payload.get("method", "")
            req_id = payload.get("id", 1)
            params = payload.get("params", [])

            result: Any = None
            if method == "aeth_getBlockByNumber":
                result = _SAMPLE_BLOCK
            elif method == "aeth_getBlockByHash":
                result = _SAMPLE_BLOCK
            elif method == "aeth_getTransactionReceipt":
                result = _SAMPLE_RECEIPT
            elif method == "aeth_getAccount":
                result = _SAMPLE_ACCOUNT
            elif method == "aeth_getStateRoot":
                result = _SAMPLE_STATE_ROOT
            else:
                encoded = json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {"code": -32601, "message": "method not found"},
                    }
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)
                return

            encoded = json.dumps(
                {"jsonrpc": "2.0", "id": req_id, "result": result}
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, format, *args):  # noqa: A003
            return

    try:
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    except PermissionError:
        pytest.skip("socket binding is not permitted in this environment")
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=1)


# ─── null-returns ─────────────────────────────────────────────────────────────

@contextmanager
def null_server():
    """Server that always returns null result (unknown block / receipt)."""
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            content_len = int(self.headers.get("content-length", 0))
            payload = json.loads(self.rfile.read(content_len).decode())
            encoded = json.dumps(
                {"jsonrpc": "2.0", "id": payload.get("id", 1), "result": None}
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, format, *args):  # noqa: A003
            return

    try:
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    except PermissionError:
        pytest.skip("socket binding is not permitted in this environment")
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=1)


# ─── get_block_by_number ─────────────────────────────────────────────────────

def test_get_block_by_number_returns_rpc_block():
    with rpc_server() as ep:
        block = AetherClient(ep).get_block_by_number()
        assert isinstance(block, RpcBlock)
        assert block.header.slot == 42
        assert block.header.timestamp == 1_700_000_000
        assert len(block.transactions) == 1


def test_get_block_by_number_with_explicit_ref():
    with rpc_server() as ep:
        block = AetherClient(ep).get_block_by_number(10, full_tx=False)
        assert block is not None
        assert block.header.slot == 42


def test_get_block_by_number_returns_none_for_unknown():
    with null_server() as ep:
        assert AetherClient(ep).get_block_by_number(9999) is None


# ─── get_block_by_hash ───────────────────────────────────────────────────────

def test_get_block_by_hash_returns_rpc_block():
    with rpc_server() as ep:
        block = AetherClient(ep).get_block_by_hash("0x" + "ab" * 32)
        assert isinstance(block, RpcBlock)
        assert block.header.slot == 42


def test_get_block_by_hash_returns_none_for_unknown():
    with null_server() as ep:
        assert AetherClient(ep).get_block_by_hash("0x" + "ff" * 32) is None


# ─── get_transaction_receipt ─────────────────────────────────────────────────

def test_get_transaction_receipt_returns_rpc_receipt():
    with rpc_server() as ep:
        receipt = AetherClient(ep).get_transaction_receipt("0x" + "bb" * 32)
        assert isinstance(receipt, RpcReceipt)
        assert receipt.slot == 42
        assert receipt.status == "success"


def test_get_transaction_receipt_returns_none_for_unknown():
    with null_server() as ep:
        assert AetherClient(ep).get_transaction_receipt("0x" + "00" * 32) is None


# ─── get_account ─────────────────────────────────────────────────────────────

def test_get_account_returns_state_dict():
    with rpc_server() as ep:
        account = AetherClient(ep).get_account("0x" + "11" * 20)
        assert account is not None
        assert account["balance"] == "1000000"
        assert account["nonce"] == 3


def test_get_account_with_block_ref():
    with rpc_server() as ep:
        account = AetherClient(ep).get_account("0x" + "11" * 20, block_ref="latest")
        assert account is not None


def test_get_account_returns_none_for_unknown():
    with null_server() as ep:
        assert AetherClient(ep).get_account("0x" + "22" * 20) is None


# ─── get_state_root ──────────────────────────────────────────────────────────

def test_get_state_root_returns_hex_string():
    with rpc_server() as ep:
        root = AetherClient(ep).get_state_root()
        assert root == _SAMPLE_STATE_ROOT
        assert root.startswith("0x")


def test_get_state_root_with_block_ref():
    with rpc_server() as ep:
        root = AetherClient(ep).get_state_root(block_ref="latest")
        assert isinstance(root, str)


# ─── get_health ──────────────────────────────────────────────────────────────

def test_get_health_returns_node_health():
    with rpc_server() as ep:
        health = AetherClient(ep).get_health()
        assert isinstance(health, NodeHealth)
        assert health.status == "ok"
        assert health.version == "0.1.0"
        assert health.latest_slot == 99
        assert health.finalized_slot == 95
        assert health.peer_count == 7
        assert health.sync.syncing is False


def test_get_health_sync_fields_populated():
    with rpc_server() as ep:
        health = AetherClient(ep).get_health()
        assert health.sync.from_slot is None
        assert health.sync.target_slot is None


def test_get_health_connection_error():
    client = AetherClient("http://127.0.0.1:1")
    with pytest.raises(ConnectionError, match="health endpoint"):
        client.get_health()
