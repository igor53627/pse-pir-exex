# InsPIRe PIR Protocol Specification

This document specifies the **InsPIRe PIR** protocol for private Ethereum
state queries, as implemented in [`inspire-rs`] and extended by `inspire-exex`.

[`inspire-rs`]: https://github.com/igor53627/inspire-rs

## Version Note

> **Initial Version**: The initial implementation uses a **single lane**
> containing full Ethereum state (~2.7B entries). This provides complete
> coverage with no lane-level privacy leakage.
>
> **Future Optimization**: A two-lane (hot/cold) architecture is specified
> below for future optimization. This trades ~1 bit of lane-level leakage
> for 10x faster server response on popular contracts.

---

## 1. System Model

### 1.1 Entities

- **Client**  
  A wallet or application that wishes to privately query Ethereum state
  (account balances, positions, contract storage slots).

- **PIR Server**  
  A single honest-but-curious server that:
  - Preprocesses Ethereum state into two PIR databases (**hot** and **cold**
    lanes).
  - Serves PIR queries for each lane over HTTP.
  - Publishes lane manifests and cryptographic reference strings (CRS).

- **Lane Builder**  
  An offline component (e.g. `lane-builder` ExEx) that:
  - Analyzes recent chain activity and curated lists.
  - Selects hot contracts and assigns them to the hot lane.
  - Encodes Ethereum state into fixed-size entries and generates per-lane
    databases and CRS.

These three roles may be operated by the same organization, but the security
goal is to protect client query indices even if the server is honest-but-curious.

### 1.2 Data Model

- **Ethereum State Space**  
  Let `U` denote the universe of state items that can be queried (e.g. a tuple
  `(contract_address, storage_slot)` or another canonical key).

- **Lane Partitioning**  

  **Initial Version (Single Lane)**:
  - All of `U` is served from a single database.
  - Encoded as a PIR database with approximately **2.7B entries** and size ~87 GB.
  - No lane-level privacy leakage.

  **Future Optimization (Two Lanes)**:
  `U` is partitioned into two disjoint subsets:
  - **Hot lane** `U_hot`  
    - Contains state for approximately the top **1,000** contracts by query
      frequency.
    - Encoded as a PIR database with approximately **1M entries** and size
      ~32 MB.
  - **Cold lane** `U_cold`  
    - Contains state for all remaining contracts/accounts.
    - Encoded as a PIR database with approximately **2.7B entries** and size
      ~87 GB.

  By construction, `U = U_hot ∪ U_cold` and `U_hot ∩ U_cold = ∅`.

- **Lane Databases**

  For each lane `ℓ ∈ {hot, cold}`:
  - The lane builder outputs a database `DB_ℓ` of `N_ℓ` fixed-size entries:
    ```text
    DB_ℓ[0], DB_ℓ[1], ..., DB_ℓ[N_ℓ - 1]
    ```
  - Entries have fixed byte size `entry_size` (e.g. 32 bytes), recorded in CRS
    metadata.
  - A public **manifest** maps each Ethereum state key `u ∈ U_ℓ` to a lane-local
    index `i ∈ {0, ..., N_ℓ - 1}`:
    ```text
    index_ℓ : U_ℓ -> {0, ..., N_ℓ - 1}
    ```

- **Sharding (Implementation Detail)**  
  For scalability, each lane's database may be sharded internally. The server
  exposes a `ShardConfig` describing this layout (see `/crs/:lane`). Sharding is
  treated as an implementation detail: from the protocol's perspective, each
  lane is a single logical array of `N_ℓ` entries.

### 1.3 Cryptographic Primitives

The protocol uses an RLWE-based PIR scheme as implemented in `inspire-rs`,
parameterized by `PirParams`. The production parameters are:

- **RLWE ring**  
  - Polynomial ring: `R_q = Z_q[x]/(x^d + 1)`
  - Ring dimension: `d = ring_dim = 2048`
  - Ciphertext modulus:  
    `q = 1152921504606830593 = 2^60 - 2^14 + 1`
  - Plaintext modulus:  
    `p = 65536 = 2^16`

- **Noise Distribution**  
  - Discrete Gaussian with standard deviation:  
    `σ = 6.4` (updated from 3.2 per InsPIRe paper recommendations).

- **Gadget Decomposition**  
  - Gadget base: `B = 2^20`
  - Gadget length: `ℓ = 3`

- **Security Level**

  The parameters target ~128-bit classical security based on:
  - Ring dimension `d = 2048` with power-of-two cyclotomic.
  - Modulus `q ≈ 2^60` with noise ratio `q/σ ≈ 2^56`.

  **Caveat**: This security estimate has NOT been formally verified with
  state-of-the-art LWE estimators (e.g., lattice-estimator) for this exact
  parameter set. Deployments SHOULD run security analysis tools before
  production use. The InsPIRe paper reference is incomplete (placeholder).

  **Decryption failure**: No formal analysis of decryption failure probability
  is provided. Implementations MUST ensure failure rates are negligible
  (< 2^-40) and independent of queried indices to avoid leakage.

- **PIR Parameter Versioning**

  The PIR parameters are versioned to avoid client/server mismatch:

  ```rust
  pub const PIR_PARAMS_VERSION: u16 = 2;

  pub const PIR_PARAMS: PirParams = PirParams {
      version: PIR_PARAMS_VERSION,
      ring_dim: 2048,
      sigma: 6.4,
      q: 1152921504606830593,
      p: 65537,  // Fermat prime F4, ensures gcd(d, p) = 1 for mod_inverse
      gadget_base: 1 << 20,
      gadget_len: 3,
  };
  ```

  Each lane's CRS metadata (`CrsMetadata`) includes:
  - `pir_params_version` (must equal `PIR_PARAMS_VERSION`)
  - The full `pir_params`
  - `entry_size`, `entry_count`, and `lane` name

  Clients MUST check `pir_params_version` for compatibility.

### 1.4 Protocol Variants

The underlying InsPIRe PIR scheme supports multiple communication variants:

- **InsPIRe^0 (baseline)**: full query, unseeded packing.
- **InsPIRe^1 (OnePacking)**: full query, better server packing (32 KB response).
- **InsPIRe^2 (Seeded+Packed)**: seeded queries (client sends seeds; server
  expands) and packed response.
- **InsPIRe^2+ (Switched+Packed)**: seeded queries + key switching + 2-matrix
  packing (smallest query size; default).

The **Two-Lane InsPIRe** system is protocol-agnostic over these variants, but
deployments SHOULD default to **InsPIRe^2+ (Switched+Packed)** for best
bandwidth/latency.

---

## 2. Threat Model

### 2.1 Adversary Model

- **Server Model: Single-Server, Honest-But-Curious**
  - The server correctly follows the protocol:
    - Uses the advertised PIR parameters and CRS.
    - Encodes DBs correctly and responds to queries as specified.
  - The server may record and analyze:
    - Lane identifier (hot vs cold).
    - Query and response ciphertexts.
    - Timing, size, and metadata (IP, headers).
  - The server attempts to infer which state entry the client queried.

- **Network Adversary**
  - May observe and modify traffic on the network.
  - Confidentiality and integrity on the wire are provided by HTTPS (TLS) and
    are out of scope for this cryptographic specification.

- **Client**
  - Assumed honest in the security analysis (does not try to exploit server
    side channels). Malicious client behavior is not covered.

### 2.2 Security Goals

The primary security goal is:

- **Query Index Confidentiality (Within a Lane)**  
  For any lane `ℓ`, and any two valid indices `i, j ∈ {0, ..., N_ℓ - 1}`, the
  distribution of transcripts of queries targeting `i` is
  computationally indistinguishable from those targeting `j`, under standard
  RLWE hardness assumptions.

More concretely, for the server:

- The **target index within a lane** MUST be hidden.
- The **target contract and storage slot**, which map to that index,
  MUST be hidden.

### 2.3 Non-Goals

The protocol does **not** attempt to:

- Hide which **lane** (hot vs cold) is queried in a given request.
- Hide which **user**, IP address, or account is querying.
- Hide that a user is using this service at all.
- Protect data integrity or availability (e.g. no authenticated PIR, no DoS
  resistance).
- Mitigate side-channel attacks (timing beyond lane-level, microarchitectural,
  etc.).

These are explicitly out of scope and must be addressed by higher-level
infrastructure or deployment practices if needed.

---

## 3. Lane Selection Algorithm

This section defines how Ethereum state items are assigned to the hot and cold
lanes. The goal is to place the most frequently queried contracts into the hot
lane to reduce server computation time for typical wallet workloads, while
preserving full PIR privacy within each lane.

### 3.1 High-Level Overview

- **Hot Lane Capacity**
  - `K ~ 1000` contracts are selected for the hot lane.
  - The hot lane holds approximately `N_hot ~ 1,000,000` entries
    (~32 MB database).

- **Cold Lane**
  - All remaining contracts (on the order of `2.7M` contracts, with
    `N_cold ~ 2.7B` entries, ~87 GB) remain in the cold lane.

- **Public Manifest**
  - The hot lane contract list and per-contract metadata (categories) are
    **publicly available** and intentionally non-sensitive.

### 3.2 Data Sources

Lane selection uses **hybrid scoring** based on:

1. **Gas Backfill (On-Chain Analytics)**
   - Scan the latest `B` blocks (default: `B = 100,000`) from an Ethereum
     node.
   - Compute aggregate gas usage and/or call frequency per contract.
   - Implementation example:

     ```bash
     cargo run --bin lane-backfill --features backfill -- \
         --rpc-url http://localhost:8545 \
         --blocks 100000
     ```

2. **Curated Contract List**
   - Include a manually curated list of known important DeFi, bridge, and
     privacy protocols (e.g., USDC, WETH, Aave, Uniswap, Tornado Cash,
     Arbitrum bridge).
   - Curated entries may receive a baseline score boost.

3. **Category Weights**
   - Each contract is assigned a category with a multiplicative weight:

     | Category       | Weight | Example Contracts                   |
     |----------------|--------|-------------------------------------|
     | Privacy        | 3.0x   | Tornado Cash, Railgun               |
     | Bridges        | 2.0x   | Arbitrum, Optimism, Polygon         |
     | Stablecoins    | 1.5x   | USDC, USDT, DAI, FRAX, LUSD         |
     | Wrapped assets | 1.0x   | WETH, WBTC, stETH, rETH             |
     | DEX            | 1.5x   | Uniswap V2/V3, Curve, Balancer      |
     | Lending        | 1.5x   | Aave V2/V3, Compound, Maker         |

### 3.3 Scoring and Selection

The precise scoring formula is an implementation detail, but the following
requirements MUST hold:

1. **Monotonicity in Usage**
   - Contracts with higher recent gas usage or call frequency SHOULD receive
     higher scores, all else equal.

2. **Category Weighting**
   - Category weights MUST be applied multiplicatively or additively such that
     a contract in a higher-weight category has a strictly higher score than an
     otherwise identical contract in a lower-weight category.

3. **Curated Priority**
   - Curated contracts MAY receive an additional fixed bonus to ensure
     inclusion even if short-term on-chain activity is low.

4. **Top-K Selection**
   - Let `score(c)` be the final score for contract `c`. The hot lane contract
     set `C_hot` MUST be the set of `K` contracts with highest scores:
     ```text
     C_hot = TopK_{c} score(c)
     C_cold = AllContracts \ C_hot
     ```

5. **Determinism**
   - Given the same input block range, curated list, and weights, the
     lane-builder MUST produce the same `C_hot` set and associated manifest.

### 3.4 Lane Update Frequency

- The hot lane selection SHOULD be recomputed on a regular cadence (e.g.
  weekly), using the latest block data and updated curated lists.
- New manifests and databases are **hot-swapped** on the server using atomic
  snapshot updates; in-flight queries continue against the old snapshot.

Lane update frequency is a deployment parameter and does not affect the
cryptographic guarantees, as long as clients and server use a consistent
manifest for index computation.

---

## 4. Query Protocol

This section defines the protocol for a single PIR query against one lane,
independent of whether the target is in the hot or cold lane.

### 4.1 Notation

- `ℓ ∈ {hot, cold}`: lane identifier.
- `N_ℓ`: number of entries in lane `ℓ`.
- `DB_ℓ`: PIR-encoded database for lane `ℓ`.
- `CRS_ℓ`: public cryptographic reference string for lane `ℓ`.
- `sk`: client's RLWE secret key.
- `i`: target index in lane `ℓ` (0-based).

### 4.2 Setup Phase (Lane Builder and Server)

For each lane `ℓ ∈ {hot, cold}`:

1. **Parameter Agreement**
   - Use `PIR_PARAMS` as defined in Section 1.3.
   - Generate or load `CRS_ℓ` consistent with `PIR_PARAMS`.

2. **Database Construction**
   - Encode each entry in `U_ℓ` into fixed-size `entry_size` bytes.
   - Arrange entries into an array `DB_ℓ[0..N_ℓ-1]`.

3. **CRS Metadata**
   - Create `CrsMetadata`:
     - `pir_params_version = PIR_PARAMS_VERSION`
     - `pir_params = PIR_PARAMS`
     - `entry_size`, `entry_count = N_ℓ`
     - `lane = "hot"` or `"cold"`
   - Persist CRS and metadata.

4. **Server Initialization**
   - Load `DB_ℓ`, `CRS_ℓ`, and metadata into memory (or mmap).
   - Expose HTTP endpoints:
     - `GET /crs/:lane`
     - `POST /query/:lane`
     - `POST /query/:lane/seeded`
     - Binary variants: `.../binary`.

### 4.3 Client Initialization

For each lane `ℓ` the client wishes to query:

1. **Fetch CRS and ShardConfig**
   - Request: `GET /crs/hot` or `GET /crs/cold`.
   - Response (JSON):
     ```json
     {
       "crs": "<base64 or JSON CRS>",
       "lane": "hot" | "cold",
       "entry_count": N_ℓ,
       "shard_config": { ... }
     }
     ```
   - The client MUST:
     - Parse `crs` and metadata.
     - Verify `pir_params_version == PIR_PARAMS_VERSION`.

2. **Key Generation**
   - Locally generate `sk` using a CSPRNG (e.g. browser WebCrypto).
   - In WASM, `SecureSecretKey` MUST be used to ensure key zeroization on drop.

3. **Variant Selection**
   - The client chooses an InsPIRe variant (recommended: `query_switched` /
     InsPIRe^2+).
   - The chosen variant determines:
     - Query structure (`ClientQuery` vs `SeededClientQuery`).
     - Endpoint (`/query/:lane` vs `/query/:lane/seeded`).
     - Response packing (OnePacking vs InspiRING 2-matrix).

### 4.4 Single-Lane Query (Default Mode)

Given target state item `u`, the client performs:

1. **Lane and Index Resolution**
   - Use the public manifest to determine:
     - Lane `ℓ = lane(u) ∈ {hot, cold}`
     - Index `i = index_ℓ(u) ∈ {0, ..., N_ℓ - 1}`

2. **Query Generation**
   - Compute a PIR query for index `i` using the selected variant.
   - Example API (Rust):

     ```rust
     use inspire_pir::pir::{query, query_seeded, query_switched};

     // Baseline (full query, 192 KB)
     let q = query(&crs, i, &config, &sk, &mut sampler);

     // Seeded (96 KB)
     let q = query_seeded(&crs, i, &config, &sk, &mut sampler);

     // Switched (48 KB, InsPIRe^2+)
     let q = query_switched(&crs, i, &config, &sk, &mut sampler);
     ```

   - For seeded variants, `q` is a `SeededClientQuery` which encodes seeds for
     the RLWE `a` polynomials rather than sending them explicitly.

3. **Client -> Server Request**

   Over HTTPS:

   - **Full query (JSON)**

     ```http
     POST /query/{lane}
     Content-Type: application/json

     {
       "query": <ClientQuery>
     }
     ```

   - **Seeded query (JSON)**

     ```http
     POST /query/{lane}/seeded
     Content-Type: application/json

     {
       "query": <SeededClientQuery>
     }
     ```

   - **Binary responses** can be requested via `/binary` suffix:

     - `/query/:lane/binary`
     - `/query/:lane/seeded/binary`

4. **Server-Side Processing**

   For a request to lane `ℓ`:

   1. **Lane Parsing**
      - Extract `ℓ` from the path `:lane` in {`"hot"`, `"cold"`}.
      - Reject invalid lane strings.

   2. **Seeded Expansion (if applicable)**

      ```rust
      // routes.rs
      let expanded_query = req.query.expand();
      ```

      - `SeededClientQuery::expand()` regenerates full RLWE `a` polynomials
        from seeds, yielding a `ClientQuery`.

   3. **PIR Response Computation**

      - Load a full snapshot (`load_snapshot_full()`).
      - Evaluate the PIR response using `DB_ℓ` and the chosen packing strategy:

        ```rust
        use inspire_pir::pir::{respond_with_variant, respond_inspiring};
        use inspire_pir::InspireVariant;

        // OnePacking variant (32 KB response)
        let resp = respond_with_variant(&crs, db, query, InspireVariant::OnePacking);

        // InspiRING 2-matrix (InsPIRe^2+, fastest packing)
        let resp = respond_inspiring(&crs, db, query);
        ```

      - The result is a `ServerResponse` object.

   4. **Server -> Client Response**

      - **JSON response**:

        ```json
        {
          "response": <ServerResponse>,
          "lane": "hot" | "cold"
        }
        ```

      - **Binary response** (`application/octet-stream`):

        ```rust
        let binary = response.to_binary()?;
        ```

5. **Client Extraction**

   Upon receiving `ServerResponse`:

   ```rust
   use inspire_pir::pir::extract_with_variant;
   use inspire_pir::InspireVariant;

   let data = extract_with_variant(
       &crs,
       &state,        // client-side state, includes sk
       &response,
       entry_size,    // e.g., 32
       InspireVariant::OnePacking, // or variant in use
   );
   ```

   - `data` is the decoded plaintext entry corresponding to `DB_ℓ[i]`, which
     encodes the requested Ethereum state item.

### 4.5 Maximum Privacy Mode (Dual-Lane Queries)

For applications requiring that the server not learn **which lane** contains the
true target, the client MAY operate in **maximum privacy mode**:

For each logical query:

1. **Select true lane `ℓ_true` and index `i_true`** as in Section 4.4.
2. **Sample decoy lane `ℓ_decoy != ℓ_true`**:
   - If `ℓ_true = hot`, `ℓ_decoy = cold`, and vice versa.
3. **Sample decoy index `i_decoy` uniformly at random from the decoy lane**
   - `i_decoy <- {0, ..., N_{ℓ_decoy} - 1}`.
4. **Generate two PIR queries**:
   - Real query: `(ℓ_true, i_true)`.
   - Decoy query: `(ℓ_decoy, i_decoy)`.
5. **Send both queries** (ideally in parallel, with similar timing).
6. **Discard the decoy response** and only use data from the real lane.

This hides which lane holds the true target at the cost of approximately 2x
communication and computation.

---

## 5. Privacy Analysis

### 5.1 What the Server Learns

Per query, the server's knowledge can be summarized as:

| Information                | Server Knowledge                           |
|---------------------------|--------------------------------------------|
| Query lane (hot/cold)     | **YES** - visible via HTTP path           |
| Target contract           | NO - hidden by PIR                         |
| Target storage slot       | NO - hidden by PIR                         |
| Target index within lane  | NO - PIR index is computationally hidden   |
| Query ciphertext contents | Encrypted under RLWE                       |
| Query timing              | YES - observable network metadata          |
| Client identity / IP      | YES - from network layer (out of scope)   |

Thus, the server learns **which popularity tier** (hot vs cold) the target
belongs to, but neither the specific contract nor the specific storage slot.

### 5.2 Within-Lane Privacy

For a fixed lane `ℓ`, the underlying InsPIRe PIR scheme guarantees:

- **Semantic Security of Ciphertexts**
  - For a fixed secret key `sk`, each query ciphertext is semantically secure
    under the RLWE assumption; adding fresh noise and randomness ensures that
    multiple queries for the same index are unlinkable at the ciphertext level.

- **Index Indistinguishability**
  - For any two indices `i, j` and any probabilistic polynomial-time server,
    the view of the server when interacting with a client querying index `i`
    is computationally indistinguishable from when the client queries index
    `j`.

Intuitively:

- The query ciphertexts leak no information about `i` beyond what is implied
  by public parameters.
- The PIR evaluation uses homomorphic operations on encrypted selectors;
  the server never sees the plaintext selector or index.

Therefore, within a lane:

- **Cryptographic anonymity set** equals `N_ℓ` (the server cannot
  cryptographically distinguish which of the `N_ℓ` indices was queried).

**Important caveat**: The *effective* anonymity set depends on the adversary's
prior distribution over indices. Given:
- The public manifest mapping contracts to indices.
- Real-world query patterns are highly skewed (popular contracts dominate).
- Category weights are public.

The server can assign non-uniform priors. Under realistic workload models, the
effective anonymity set (e.g., measured by min-entropy) may be significantly
smaller than `N_ℓ`. The cryptographic guarantee is that PIR does not *increase*
the server's information beyond its prior beliefs.

### 5.3 Lane-Level Leakage

The two-lane design **intentionally leaks** lane membership per query:

- The server observes which lane (hot/cold) receives each query via the HTTP path.

This corresponds to "popularity tier" leakage: the server learns whether the
target contract is among the top ~1,000 most-used contracts or not.

**Quantifying this leakage**: While often described as "one bit," the actual
information content depends on:
- The public hot-lane contract list and category weights.
- Correlation between lane and sensitive contract types (e.g., privacy protocols
  may be over-represented in the hot lane due to 3x category weight).

The information learned is better characterized as: "the query targets a contract
drawn from the publicly-known hot-lane distribution" vs "cold-lane distribution."
This may encode more than 1 bit of semantic information in practice.

This leakage is considered acceptable for most use cases because:

1. The exact contract remains cryptographically hidden within the lane.
2. For users requiring stronger privacy, maximum privacy mode is available.
3. The hot lane's contract list is intentionally public.

**Maximum privacy mode** (Section 4.5) reduces lane-level leakage by querying
both lanes, but does NOT fully eliminate it due to:
- Timing differences: hot-lane queries typically complete faster than cold-lane.
- Resource usage differences observable by the server.

Applications requiring strong lane privacy MUST implement additional mitigations
(e.g., artificial delays, traffic shaping) beyond dual-lane querying.

### 5.4 Cross-Query and Metadata Leakage

The protocol does not address higher-level metadata leakage:

- Timing patterns and frequency of queries.
- Long-lived client identifiers (cookies, IPs, TLS sessions).
- Correlation across lanes at the application layer.

Applications that require stronger privacy MUST consider:

- Tor or other network-layer anonymity systems.
- Traffic padding and batch scheduling of queries.
- Always-on dual-lane querying.

---

## 6. Performance Analysis

This section summarizes expected performance characteristics, based on
benchmarks with `d = 2048` and ~128-bit security.

### 6.1 Communication Complexity

- **Asymptotic**:  
  InsPIRe communication complexity is `O(d)` where `d` is the RLWE ring
  dimension. It does **not** scale as `O(sqrt(N))` with the database size. Therefore:
  - Query size is independent of `N_ℓ`.
  - Response size is independent of `N_ℓ`.

- **Variant-Specific Sizes** (per query):

  | Variant                      | Query (upload) | Response (download) | Total |
  |------------------------------|----------------|---------------------|-------|
  | InsPIRe^0 (baseline)         | 192 KB         | 545 KB              | 737 KB |
  | InsPIRe^1 (OnePacking)       | 192 KB         | 32 KB               | 224 KB |
  | InsPIRe^2 (Seeded+Packed)    | 96 KB          | 32 KB               | 128 KB |
  | **InsPIRe^2+ (Switched+Packed)** | **48 KB**  | **32 KB**           | **80 KB** |

These sizes apply equally to both lanes; lane size affects **server computation
time**, not client communication.

### 6.2 Server Computation

Measured on an AMD/Intel x64 server with `d = 2048` and 128-bit security:

- **Server Response Time vs Database Size**

  | Database Size | Shards | Server Respond Time |
  |---------------|--------|---------------------|
  | 256K entries (8 MB)  | 128 | 3.8 ms          |
  | 512K entries (16 MB) | 256 | 3.1 ms          |
  | 1M entries (32 MB)   | 512 | 3.3 ms          |

For the hot lane (~1M entries, 32 MB), response time is ~3-4 ms.

**Cold lane performance**: The cold lane (~2.7B entries, ~87 GB) has NOT been
benchmarked at full scale. Expected behavior:
- Query/response sizes remain constant (same as hot lane).
- Server computation scales with shard count and internal layout.
- Memory pressure, cache misses, and I/O may significantly increase latency.
- Realistic cold-lane latencies could be 10-100x higher than hot-lane.

Deployments MUST benchmark cold-lane performance under realistic conditions
before making latency guarantees.

The **primary benefit** of the hot lane is thus **lower server-side latency**
for the majority of queries (those targeting popular contracts).

### 6.3 End-to-End Latency

Benchmarked end-to-end latency (single query, InsPIRe^2+, hot-lane-sized DB):

| Phase                          | Time   |
|--------------------------------|--------|
| Client: query generation (switched) | ~4 ms  |
| Server: expand + respond       | ~3-4 ms |
| Client: extract                | ~5 ms  |
| **Total**                      | **~12 ms** |

This is sufficient for interactive wallet UX.

### 6.4 Multi-Query Workloads

For a typical "wallet open" scenario involving ~14 independent queries:

| Approach                 | Upload | Download | Total | Privacy |
|--------------------------|--------|----------|-------|---------|
| Clearnet RPC             | 2 KB   | 2 KB     | 4 KB  | None    |
| InsPIRe^0 (baseline)     | 2.7 MB | 7.6 MB   | 10.3 MB | Full  |
| InsPIRe^1 (OnePacking)   | 2.7 MB | 0.4 MB   | 3.1 MB | Full   |
| InsPIRe^2 (Seeded+Packed)| 1.3 MB | 0.4 MB   | 1.8 MB | Full   |
| **InsPIRe^2+ (Switched+Packed)** | **0.7 MB** | **0.4 MB** | **1.1 MB** | **Full** |

With Two-Lane InsPIRe, and assuming most wallet queries hit the hot lane,
the **average per-query latency** approaches the hot-lane numbers above, with
cold-lane queries incurring higher server processing costs but identical
communication overhead.

### 6.5 Operational Considerations

- **Live Database Updates**
  - The server supports **lock-free hot reloading** of lane databases:
    - `ArcSwap` for lock-free reads.
    - `mmap` mode (default) for O(1) swap time (~1-5 ms).
    - Atomic snapshot swaps ensure in-flight queries complete on old data.

- **Observability**
  - Metrics include:
    - `pir_requests_total`, `pir_request_duration_seconds` (by lane and outcome).
    - `pir_lane_loaded`, `pir_lane_block_number`, `pir_lane_mmap_mode`.
    - `pir_reload_total`, `pir_reload_duration_seconds`.
  - Metrics are designed to be privacy-safe: labels include only **lane** and
    **outcome**, never query contents.

Overall, the two-lane architecture trades **one bit of privacy leakage**
(hot vs cold) for substantial performance gains:
- Significant reduction in average server compute time.
- Constant-size queries and responses, independent of database size.
- End-to-end latencies on the order of ~10-20 ms for hot-lane workloads.

---

## 7. Limitations and Open Issues

This section documents known limitations and areas requiring further work.

### 7.1 Unverified Security Claims

- **RLWE parameters**: The ~128-bit security claim has not been verified with
  lattice-estimator or equivalent tools for this exact parameter set.
- **Decryption failure**: No formal bound on failure probability is provided.
- **Implementation correctness**: `inspire-rs` has not been formally verified
  or audited for side-channel resistance.

### 7.2 Privacy Limitations

- **Effective anonymity**: Under realistic workload priors, effective anonymity
  sets are much smaller than `N_ℓ`.
- **Lane leakage**: Lane membership leaks more than "one bit" of semantic
  information due to public manifest and category weights.
- **Maximum privacy mode**: Does not fully hide lane due to timing side-channels.
- **Metadata**: IP addresses, timing patterns, and session correlation are
  explicitly out of scope but critical in practice.

### 7.3 Performance Limitations

- **Cold lane**: Not benchmarked at full 2.7B entry scale.
- **Concurrency**: No analysis of performance under realistic concurrent load.
- **Network latency**: End-to-end estimates ignore network RTT.

### 7.4 Specification Gaps

- **Lane selection**: No formal scoring function or optimality criterion defined.
- **Entry encoding**: No specification of how 32-byte entries map to plaintext
  modulus p=2^16.
- **Error handling**: No specification of behavior on decryption failures or
  malformed queries.

---

## References

- [InsPIRe Paper](https://eprint.iacr.org/2023/297) - Original PIR construction
- [inspire-rs](https://github.com/igor53627/inspire-rs) - Rust PIR implementation
- [inspire-exex](https://github.com/igor53627/inspire-exex) - Two-lane extension
- [Lattice Estimator](https://github.com/malb/lattice-estimator) - LWE security analysis
