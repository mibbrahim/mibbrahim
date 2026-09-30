//! Thin JSON-RPC client (reqwest) for the few calls the bot needs.

use anyhow::{anyhow, Context};
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Rpc {
    pub url: String,
    http: reqwest::Client,
}

impl Rpc {
    pub fn new(url: &str) -> Rpc {
        let http = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Rpc { url: url.to_string(), http }
    }

    pub async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp = self.http.post(&self.url).json(&body).send().await.with_context(|| format!("{method} request"))?;
        let status = resp.status();
        let v: Value = resp.json().await.with_context(|| format!("{method} decode (http {status})"))?;
        if let Some(e) = v.get("error") {
            return Err(anyhow!("{method}: {e}"));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn get_slot(&self, commitment: &str) -> anyhow::Result<u64> {
        let v = self.call("getSlot", json!([{"commitment": commitment}])).await?;
        v.as_u64().ok_or_else(|| anyhow!("bad getSlot"))
    }

    /// (blockhash bytes, context slot)
    pub async fn latest_blockhash(&self) -> anyhow::Result<([u8; 32], u64)> {
        let v = self.call("getLatestBlockhash", json!([{"commitment": "processed"}])).await?;
        let h = v["value"]["blockhash"].as_str().ok_or_else(|| anyhow!("no blockhash"))?;
        let slot = v["context"]["slot"].as_u64().unwrap_or(0);
        Ok((decode_hash(h)?, slot))
    }

    pub async fn balance(&self, pk: &str) -> anyhow::Result<u64> {
        let v = self.call("getBalance", json!([pk, {"commitment": "processed"}])).await?;
        v["value"].as_u64().ok_or_else(|| anyhow!("bad getBalance"))
    }

    pub async fn send_tx(&self, wire: &[u8]) -> anyhow::Result<String> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(wire);
        let v = self
            .call("sendTransaction", json!([b64, {"encoding": "base64", "skipPreflight": true, "maxRetries": 0}]))
            .await?;
        Ok(v.as_str().unwrap_or_default().to_string())
    }

    /// Simulate without a valid signature or blockhash: validates accounts/instructions.
    pub async fn simulate(&self, wire: &[u8]) -> anyhow::Result<Value> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(wire);
        self.call(
            "simulateTransaction",
            json!([b64, {"encoding": "base64", "sigVerify": false, "replaceRecentBlockhash": true, "commitment": "processed"}]),
        )
        .await
    }

    pub async fn get_block(&self, slot: u64) -> anyhow::Result<Option<String>> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"getBlock","params":[slot, {
            "encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"full","rewards":false,"commitment":"confirmed"}]});
        let resp = self.http.post(&self.url).json(&body).send().await?;
        let text = resp.text().await?;
        Ok(Some(text))
    }
}

pub fn decode_hash(s: &str) -> anyhow::Result<[u8; 32]> {
    let v = bs58::decode(s).into_vec()?;
    v.try_into().map_err(|_| anyhow!("bad hash length"))
}

/// Jito block-engine `sendTransaction` (the tx must already contain a tip transfer).
pub async fn jito_send(http: &reqwest::Client, url: &str, wire: &[u8]) -> anyhow::Result<String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(wire);
    let body = json!({"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[b64, {"encoding":"base64"}]});
    let v: Value = http.post(url).json(&body).send().await?.json().await?;
    if let Some(e) = v.get("error") {
        return Err(anyhow!("jito: {e}"));
    }
    Ok(v["result"].as_str().unwrap_or_default().to_string())
}
