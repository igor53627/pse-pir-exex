use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_sol_types::{sol, SolCall};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_eips::eip2718::Encodable2718;
use alloy_consensus::{SignableTransaction, TxEip7702};
use alloy_rlp::Encodable;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

sol! {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
}

#[derive(Serialize, Deserialize)]
pub struct WalletInfo {
    pub private_key: String,
    pub address: String,
}

#[derive(Serialize, Deserialize)]
pub struct AuthorizationRequest {
    pub chain_id: u64,
    pub contract_address: String,
    pub nonce: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SignedAuthorizationResult {
    pub chain_id: u64,
    pub address: String,
    pub nonce: u64,
    pub y_parity: u8,
    pub r: String,
    pub s: String,
    pub rlp_encoded: String,
}

#[derive(Serialize, Deserialize)]
pub struct Eip7702TxRequest {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: String,
    pub max_fee_per_gas: String,
    pub gas_limit: u64,
    pub to: String,
    pub value: String,
    pub data: String,
    pub authorization_list: Vec<SignedAuthorizationInput>,
}

#[derive(Serialize, Deserialize)]
pub struct SignedAuthorizationInput {
    pub chain_id: u64,
    pub address: String,
    pub nonce: u64,
    pub y_parity: u8,
    pub r: String,
    pub s: String,
}

#[derive(Serialize, Deserialize)]
pub struct SignedTxResult {
    pub tx_hash: String,
    pub raw_tx: String,
}

fn parse_hex(s: &str) -> Result<Vec<u8>, JsError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(|e| JsError::new(&format!("Invalid hex: {}", e)))
}

fn to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn get_signer(private_key: &str) -> Result<PrivateKeySigner, JsError> {
    let key_bytes: [u8; 32] = parse_hex(private_key)?
        .try_into()
        .map_err(|_| JsError::new("Invalid key length, expected 32 bytes"))?;
    
    PrivateKeySigner::from_bytes(&B256::from(key_bytes))
        .map_err(|e| JsError::new(&format!("Invalid private key: {}", e)))
}

#[wasm_bindgen]
pub fn generate_wallet() -> Result<String, JsError> {
    let signer = PrivateKeySigner::random();
    let info = WalletInfo {
        private_key: to_hex(signer.to_bytes().as_slice()),
        address: format!("{:?}", signer.address()),
    };
    serde_json::to_string(&info).map_err(|e| JsError::new(&format!("{}", e)))
}

#[wasm_bindgen]
pub fn get_address(private_key: &str) -> Result<String, JsError> {
    let signer = get_signer(private_key)?;
    Ok(format!("{:?}", signer.address()))
}

#[wasm_bindgen]
pub fn sign_authorization(
    private_key: &str,
    auth_request_json: &str,
) -> Result<String, JsError> {
    let signer = get_signer(private_key)?;
    let req: AuthorizationRequest = serde_json::from_str(auth_request_json)
        .map_err(|e| JsError::new(&format!("Invalid request: {}", e)))?;
    
    let contract_addr: Address = req.contract_address.parse()
        .map_err(|e| JsError::new(&format!("Invalid contract address: {}", e)))?;
    
    let auth = Authorization {
        chain_id: U256::from(req.chain_id),
        address: contract_addr,
        nonce: req.nonce,
    };
    
    let sig_hash = auth.signature_hash();
    let signed = auth.into_signed(signer.sign_hash_sync(&sig_hash)
        .map_err(|e| JsError::new(&format!("Signing failed: {}", e)))?);
    
    let mut rlp_buf = Vec::new();
    signed.encode(&mut rlp_buf);
    
    let r_bytes: [u8; 32] = signed.r().to_be_bytes();
    let s_bytes: [u8; 32] = signed.s().to_be_bytes();
    
    let result = SignedAuthorizationResult {
        chain_id: signed.chain_id().try_into().unwrap_or(0),
        address: format!("{:?}", signed.address()),
        nonce: signed.nonce(),
        y_parity: signed.y_parity(),
        r: to_hex(&r_bytes),
        s: to_hex(&s_bytes),
        rlp_encoded: to_hex(&rlp_buf),
    };
    
    serde_json::to_string(&result).map_err(|e| JsError::new(&format!("{}", e)))
}

#[wasm_bindgen]
pub fn sign_message(private_key: &str, message: &str) -> Result<String, JsError> {
    let signer = get_signer(private_key)?;
    let message_bytes = message.as_bytes();
    
    let prefixed = format!("\x19Ethereum Signed Message:\n{}{}", message_bytes.len(), message);
    let hash = alloy_primitives::keccak256(prefixed.as_bytes());
    
    let sig = signer.sign_hash_sync(&hash)
        .map_err(|e| JsError::new(&format!("Signing failed: {}", e)))?;
    
    Ok(to_hex(&sig.as_bytes()))
}

#[wasm_bindgen]
pub fn sign_typed_data_hash(private_key: &str, hash_hex: &str) -> Result<String, JsError> {
    let signer = get_signer(private_key)?;
    let hash_bytes: [u8; 32] = parse_hex(hash_hex)?
        .try_into()
        .map_err(|_| JsError::new("Invalid hash length"))?;
    
    let sig = signer.sign_hash_sync(&B256::from(hash_bytes))
        .map_err(|e| JsError::new(&format!("Signing failed: {}", e)))?;
    
    Ok(to_hex(&sig.as_bytes()))
}

#[wasm_bindgen]
pub fn encode_balance_of(address: &str) -> Result<Vec<u8>, JsError> {
    let addr: Address = address.parse()
        .map_err(|e| JsError::new(&format!("Invalid address: {}", e)))?;
    let call = balanceOfCall { account: addr };
    Ok(call.abi_encode().to_vec())
}

#[wasm_bindgen]
pub fn encode_transfer(to: &str, amount: &str) -> Result<Vec<u8>, JsError> {
    let to_addr: Address = to.parse()
        .map_err(|e| JsError::new(&format!("Invalid address: {}", e)))?;
    let amount_u256: U256 = amount.parse()
        .map_err(|e| JsError::new(&format!("Invalid amount: {}", e)))?;
    
    let call = transferCall { to: to_addr, amount: amount_u256 };
    Ok(call.abi_encode().to_vec())
}

#[wasm_bindgen]
pub fn keccak256(data: &[u8]) -> Vec<u8> {
    alloy_primitives::keccak256(data).to_vec()
}

#[wasm_bindgen]
pub fn parse_address(address: &str) -> Result<String, JsError> {
    let addr: Address = address.parse()
        .map_err(|e| JsError::new(&format!("Invalid address: {}", e)))?;
    Ok(format!("{:?}", addr))
}

#[wasm_bindgen]
pub fn format_units(value: &str, decimals: u8) -> Result<String, JsError> {
    let val: U256 = value.parse()
        .map_err(|e| JsError::new(&format!("Invalid value: {}", e)))?;
    
    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = val / divisor;
    let frac = val % divisor;
    
    if frac.is_zero() {
        Ok(whole.to_string())
    } else {
        let frac_str = format!("{:0>width$}", frac, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        Ok(format!("{}.{}", whole, trimmed))
    }
}

#[wasm_bindgen]
pub fn parse_units(value: &str, decimals: u8) -> Result<String, JsError> {
    let parts: Vec<&str> = value.split('.').collect();
    
    let (whole, frac) = match parts.len() {
        1 => (parts[0], ""),
        2 => (parts[0], parts[1]),
        _ => return Err(JsError::new("Invalid number format")),
    };
    
    let whole_val: U256 = if whole.is_empty() { U256::ZERO } else {
        whole.parse().map_err(|e| JsError::new(&format!("Invalid whole part: {}", e)))?
    };
    
    let frac_padded = format!("{:0<width$}", frac, width = decimals as usize);
    let frac_trimmed = &frac_padded[..decimals as usize];
    let frac_val: U256 = if frac_trimmed.is_empty() { U256::ZERO } else {
        frac_trimmed.parse().map_err(|e| JsError::new(&format!("Invalid frac part: {}", e)))?
    };
    
    let multiplier = U256::from(10u64).pow(U256::from(decimals));
    let result = whole_val * multiplier + frac_val;
    
    Ok(result.to_string())
}

#[wasm_bindgen]
pub fn sign_eip7702_tx(
    private_key: &str,
    tx_request_json: &str,
) -> Result<String, JsError> {
    let signer = get_signer(private_key)?;
    let req: Eip7702TxRequest = serde_json::from_str(tx_request_json)
        .map_err(|e| JsError::new(&format!("Invalid tx request: {}", e)))?;
    
    let to_addr: Address = req.to.parse()
        .map_err(|e| JsError::new(&format!("Invalid to address: {}", e)))?;
    
    let value: U256 = req.value.parse()
        .map_err(|e| JsError::new(&format!("Invalid value: {}", e)))?;
    
    let max_priority_fee: u128 = req.max_priority_fee_per_gas.parse()
        .map_err(|e| JsError::new(&format!("Invalid max_priority_fee_per_gas: {}", e)))?;
    
    let max_fee: u128 = req.max_fee_per_gas.parse()
        .map_err(|e| JsError::new(&format!("Invalid max_fee_per_gas: {}", e)))?;
    
    let data = if req.data.is_empty() || req.data == "0x" {
        Bytes::new()
    } else {
        Bytes::from(parse_hex(&req.data)?)
    };
    
    let mut auth_list: Vec<SignedAuthorization> = Vec::new();
    for auth_input in &req.authorization_list {
        let addr: Address = auth_input.address.parse()
            .map_err(|e| JsError::new(&format!("Invalid auth address: {}", e)))?;
        
        let r_bytes: [u8; 32] = parse_hex(&auth_input.r)?
            .try_into()
            .map_err(|_| JsError::new("Invalid R length"))?;
        let s_bytes: [u8; 32] = parse_hex(&auth_input.s)?
            .try_into()
            .map_err(|_| JsError::new("Invalid S length"))?;
        
        let auth = Authorization {
            chain_id: U256::from(auth_input.chain_id),
            address: addr,
            nonce: auth_input.nonce,
        };
        
        let sig = alloy_primitives::Signature::from_scalars_and_parity(
            B256::from(r_bytes),
            B256::from(s_bytes),
            auth_input.y_parity != 0,
        );
        
        auth_list.push(auth.into_signed(sig));
    }
    
    let tx = TxEip7702 {
        chain_id: req.chain_id,
        nonce: req.nonce,
        max_priority_fee_per_gas: max_priority_fee,
        max_fee_per_gas: max_fee,
        gas_limit: req.gas_limit,
        to: to_addr,
        value,
        input: data,
        access_list: Default::default(),
        authorization_list: auth_list,
    };
    
    let sig_hash = tx.signature_hash();
    let sig = signer.sign_hash_sync(&sig_hash)
        .map_err(|e| JsError::new(&format!("Signing failed: {}", e)))?;
    
    let signed_tx = tx.into_signed(sig);
    
    let mut rlp_buf = Vec::new();
    signed_tx.encode_2718(&mut rlp_buf);
    
    let tx_hash = signed_tx.hash();
    
    let result = SignedTxResult {
        tx_hash: to_hex(tx_hash.as_slice()),
        raw_tx: to_hex(&rlp_buf),
    };
    
    serde_json::to_string(&result).map_err(|e| JsError::new(&format!("{}", e)))
}

mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, std::fmt::Error> {
        if s.len() % 2 != 0 {
            return Err(std::fmt::Error);
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| std::fmt::Error))
            .collect()
    }

    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
