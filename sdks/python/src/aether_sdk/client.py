from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, Optional, Union

from .builders import JobBuilder, TransferBuilder
from .transaction import Transaction
from .types import (
    ClientConfig,
    JobRequest,
    JobSubmission,
    NodeHealth,
    RpcAccountState,
    RpcBlock,
    RpcReceipt,
    SubmitResponse,
)


def _normalize_endpoint(endpoint: str) -> str:
    if not endpoint:
        raise ValueError("endpoint must be provided")
    return endpoint.rstrip("/")


def _rpc_payload(method: str, params: list[object], request_id: int) -> bytes:
    return json.dumps(
        {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": request_id,
        }
    ).encode("utf-8")


@dataclass
class AetherClient:
    endpoint: str
    config: ClientConfig = ClientConfig()
    _request_id: int = 1

    def __post_init__(self) -> None:
        self.endpoint = _normalize_endpoint(self.endpoint)

    def transfer(self) -> TransferBuilder:
        return TransferBuilder(self.config)

    def job(self) -> JobBuilder:
        return JobBuilder(self.endpoint)

    def submit(self, transaction: Transaction) -> SubmitResponse:
        tx_hash = self._rpc_call(
            "aeth_sendTransaction",
            [transaction.to_rpc_transaction()],
        )
        if not isinstance(tx_hash, str):
            raise ValueError("rpc response did not include a transaction hash")
        return SubmitResponse(tx_hash=tx_hash, accepted=True)

    def get_slot_number(self) -> int:
        slot = self._rpc_call("aeth_getSlotNumber", [])
        if not isinstance(slot, int):
            raise ValueError("rpc response did not include a slot number")
        return slot

    def get_finalized_slot(self) -> int:
        slot = self._rpc_call("aeth_getFinalizedSlot", [])
        if not isinstance(slot, int):
            raise ValueError("rpc response did not include a finalized slot")
        return slot

    def get_block_by_number(
        self,
        block_ref: Union[int, str] = "latest",
        full_tx: bool = True,
    ) -> Optional[RpcBlock]:
        result = self._rpc_call("aeth_getBlockByNumber", [str(block_ref), full_tx])
        if result is None:
            return None
        return RpcBlock.from_dict(result)

    def get_block_by_hash(
        self,
        block_hash: str,
        full_tx: bool = True,
    ) -> Optional[RpcBlock]:
        result = self._rpc_call("aeth_getBlockByHash", [block_hash, full_tx])
        if result is None:
            return None
        return RpcBlock.from_dict(result)

    def get_transaction_receipt(self, tx_hash: str) -> Optional[RpcReceipt]:
        result = self._rpc_call("aeth_getTransactionReceipt", [tx_hash])
        if result is None:
            return None
        return RpcReceipt.from_dict(result)

    def get_account(
        self,
        address: str,
        block_ref: Optional[str] = None,
    ) -> Optional[RpcAccountState]:
        params: list[object] = [address] if block_ref is None else [address, block_ref]
        return self._rpc_call("aeth_getAccount", params)

    def get_state_root(self, block_ref: Optional[str] = None) -> str:
        params: list[object] = [] if block_ref is None else [block_ref]
        result = self._rpc_call("aeth_getStateRoot", params)
        if not isinstance(result, str):
            raise ValueError("rpc response did not include a state root string")
        return result

    def get_health(self) -> NodeHealth:
        """Fetch node health from the HTTP /health endpoint (not a JSON-RPC call)."""
        request = urllib.request.Request(
            f"{self.endpoint}/health",
            headers={"accept": "application/json"},
            method="GET",
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                body = response.read().decode("utf-8")
        except urllib.error.URLError as exc:
            raise ConnectionError(
                f"failed to reach health endpoint {self.endpoint}/health"
            ) from exc
        data: Dict[str, Any] = json.loads(body)
        return NodeHealth.from_dict(data)

    def prepare_job_submission(self, job: JobRequest) -> JobSubmission:
        return JobSubmission(
            url=f"{self.endpoint}/v1/jobs",
            method="POST",
            headers={"content-type": "application/json"},
            body=job,
        )

    def _rpc_call(self, method: str, params: list[object]) -> Any:
        request_id = self._request_id
        self._request_id += 1
        request = urllib.request.Request(
            self.endpoint,
            data=_rpc_payload(method, params, request_id),
            headers={"content-type": "application/json"},
            method="POST",
        )

        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                body = response.read().decode("utf-8")
        except urllib.error.URLError as exc:
            raise ConnectionError(f"failed to reach rpc endpoint {self.endpoint}") from exc

        payload: Dict[str, Any] = json.loads(body)
        error = payload.get("error")
        if error is not None:
            code = error.get("code", "unknown")
            message = error.get("message", "unknown rpc error")
            raise ValueError(f"rpc error {code}: {message}")
        if "result" not in payload:
            raise ValueError("rpc response missing result")
        return payload["result"]
