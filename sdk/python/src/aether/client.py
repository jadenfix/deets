"""
Aether RPC Client

Main client for interacting with Aether blockchain nodes.
"""

import httpx
from typing import Optional, Union, Any, Dict
from .types import (
    Block,
    Transaction,
    TransactionReceipt,
    Account,
    Address,
    Hash,
)


class AetherClient:
    """Async HTTP client for Aether JSON-RPC"""

    def __init__(
        self,
        rpc_url: str,
        chain_id: int = 1,
        timeout: float = 30.0,
    ):
        self.rpc_url = rpc_url
        self.chain_id = chain_id
        self.timeout = timeout
        self._request_id = 0
        self._client = httpx.AsyncClient(timeout=timeout)

    async def __aenter__(self) -> "AetherClient":
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    async def close(self) -> None:
        """Close the HTTP client"""
        await self._client.aclose()

    async def _call(self, method: str, params: Optional[list] = None) -> Any:
        """Low-level RPC call"""
        self._request_id += 1
        
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params or [],
            "id": self._request_id,
        }

        try:
            response = await self._client.post(self.rpc_url, json=payload)
            response.raise_for_status()
            data = response.json()

            if "error" in data:
                error = data["error"]
                raise Exception(f"RPC Error: {error['message']} (code: {error['code']})")

            return data.get("result")

        except httpx.HTTPError as e:
            raise Exception(f"HTTP error: {e}")

    async def get_slot(self) -> int:
        """Get current chain slot number"""
        return await self._call("getSlot")

    async def get_block(
        self, slot: int, include_transactions: bool = False
    ) -> Block:
        """Get block by slot number"""
        data = await self._call("getBlock", [slot, include_transactions])
        return Block(**data)

    async def get_block_by_hash(
        self, hash: Hash, include_transactions: bool = False
    ) -> Block:
        """Get block by hash"""
        data = await self._call("getBlockByHash", [hash, include_transactions])
        return Block(**data)

    async def get_latest_block(self) -> Block:
        """Get latest finalized block"""
        data = await self._call("getLatestBlock")
        return Block(**data)

    async def get_transaction(self, hash: Hash) -> Optional[Transaction]:
        """Get transaction by hash"""
        data = await self._call("getTransaction", [hash])
        if data is None:
            return None
        return Transaction(**data)

    async def get_account(self, address: Address) -> Account:
        """Get account information"""
        data = await self._call("getAccount", [address])
        return Account(**data)

    async def get_balance(self, address: Address) -> int:
        """Get account balance"""
        account = await self.get_account(address)
        return account.balance

    async def get_nonce(self, address: Address) -> int:
        """Get account nonce"""
        account = await self.get_account(address)
        return account.nonce

    async def send_transaction(self, tx: Transaction) -> Hash:
        """Send signed transaction"""
        tx_dict = {
            "from": tx.from_addr,
            "to": tx.to,
            "value": tx.value,
            "data": tx.data.hex() if tx.data else None,
            "nonce": tx.nonce,
            "signature": tx.signature,
        }
        return await self._call("sendTransaction", [tx_dict])

    async def send_raw_transaction(self, raw_tx: str) -> Hash:
        """Send raw transaction (hex-encoded)"""
        return await self._call("sendRawTransaction", [raw_tx])

    async def get_transaction_receipt(
        self, hash: Hash
    ) -> Optional[TransactionReceipt]:
        """Get transaction receipt"""
        data = await self._call("getTransactionReceipt", [hash])
        if data is None:
            return None
        return TransactionReceipt(**data)

    async def estimate_gas(self, tx: Dict[str, Any]) -> int:
        """Estimate gas for transaction"""
        return await self._call("estimateGas", [tx])

    async def is_healthy(self) -> bool:
        """Check node health"""
        try:
            await self.get_slot()
            return True
        except:
            return False

    async def wait_for_transaction(
        self,
        hash: Hash,
        timeout: float = 30.0,
        poll_interval: float = 1.0,
    ) -> TransactionReceipt:
        """Wait for transaction confirmation"""
        import asyncio

        start_time = asyncio.get_event_loop().time()

        while asyncio.get_event_loop().time() - start_time < timeout:
            receipt = await self.get_transaction_receipt(hash)

            if receipt:
                return receipt

            await asyncio.sleep(poll_interval)

        raise TimeoutError(f"Transaction {hash} not confirmed within {timeout}s")

