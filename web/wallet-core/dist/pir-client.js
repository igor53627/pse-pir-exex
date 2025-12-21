let wasmModule = null;
let wasmInit = null;
async function ensureWasmLoaded() {
    if (wasmModule)
        return wasmModule;
    if (!wasmInit) {
        wasmInit = (async () => {
            const wasm = await import('inspire-client-wasm');
            await wasm.default();
            wasmModule = wasm;
        })();
    }
    await wasmInit;
    return wasmModule;
}
export class PirBalanceClient {
    client = null;
    metadata = null;
    serverUrl;
    lane;
    constructor(serverUrl, lane = 'balances') {
        this.serverUrl = serverUrl;
        this.lane = lane;
    }
    async init() {
        const wasm = await ensureWasmLoaded();
        const metadataRes = await fetch(`${this.serverUrl}/metadata/${this.lane}`);
        if (!metadataRes.ok) {
            throw new Error(`Failed to fetch metadata: ${metadataRes.status}`);
        }
        this.metadata = await metadataRes.json();
        this.client = new wasm.PirClient(this.serverUrl);
        await this.client.init(this.lane);
    }
    getMetadata() {
        return this.metadata;
    }
    getSnapshotBlock() {
        if (!this.metadata)
            throw new Error('Not initialized');
        return BigInt(this.metadata.snapshotBlock);
    }
    getSnapshotBlockHash() {
        if (!this.metadata)
            throw new Error('Not initialized');
        return this.metadata.snapshotBlockHash;
    }
    findAddressIndex(address) {
        if (!this.metadata)
            throw new Error('Not initialized');
        const normalized = address.toLowerCase();
        const idx = this.metadata.addresses.findIndex(a => a.toLowerCase() === normalized);
        return idx;
    }
    async queryBalance(address) {
        if (!this.client || !this.metadata) {
            throw new Error('Client not initialized');
        }
        const index = this.findAddressIndex(address);
        if (index < 0) {
            return null;
        }
        const result = await this.client.query_binary(BigInt(index));
        if (result.length < 64) {
            throw new Error(`Invalid balance record size: ${result.length}`);
        }
        const ethBytes = result.slice(0, 32);
        const usdcBytes = result.slice(32, 64);
        return {
            eth: bytesToBigInt(ethBytes),
            usdc: bytesToBigInt(usdcBytes),
        };
    }
    dispose() {
        if (this.client) {
            this.client.free();
            this.client = null;
        }
    }
}
function bytesToBigInt(bytes) {
    let result = 0n;
    for (const byte of bytes) {
        result = (result << 8n) | BigInt(byte);
    }
    return result;
}
//# sourceMappingURL=pir-client.js.map