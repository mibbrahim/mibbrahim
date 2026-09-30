//! Small value types shared by every module: 32-byte pubkeys and 64-byte signatures
//! that serialize as base58 strings, plus wall-clock helpers.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Pk(pub [u8; 32]);

impl Pk {
    pub const ZERO: Pk = Pk([0u8; 32]);

    pub fn from_slice(b: &[u8]) -> Option<Pk> {
        if b.len() < 32 {
            return None;
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&b[..32]);
        Some(Pk(a))
    }

    pub fn parse(s: &str) -> anyhow::Result<Pk> {
        let v = bs58::decode(s.trim()).into_vec()?;
        if v.len() != 32 {
            anyhow::bail!("bad pubkey length for {s}");
        }
        Ok(Pk::from_slice(&v).unwrap())
    }

    /// For compile-time-known addresses.
    pub fn lit(s: &str) -> Pk {
        Pk::parse(s).expect("bad pubkey literal")
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }

    pub fn b58(&self) -> String {
        bs58::encode(self.0).into_string()
    }

    pub fn sol(&self) -> solana_pubkey::Pubkey {
        solana_pubkey::Pubkey::new_from_array(self.0)
    }

    pub fn from_sol(p: &solana_pubkey::Pubkey) -> Pk {
        Pk(p.to_bytes())
    }
}

impl fmt::Display for Pk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.b58())
    }
}
impl fmt::Debug for Pk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.b58())
    }
}
impl Serialize for Pk {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.b58())
    }
}
impl<'de> Deserialize<'de> for Pk {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Pk::parse(&s).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig(pub [u8; 64]);

impl Default for Sig {
    fn default() -> Self {
        Sig([0u8; 64])
    }
}

impl Sig {
    pub fn from_slice(b: &[u8]) -> Sig {
        let mut a = [0u8; 64];
        let n = b.len().min(64);
        a[..n].copy_from_slice(&b[..n]);
        Sig(a)
    }
    pub fn parse(s: &str) -> anyhow::Result<Sig> {
        let v = bs58::decode(s.trim()).into_vec()?;
        Ok(Sig::from_slice(&v))
    }
    pub fn b58(&self) -> String {
        bs58::encode(self.0).into_string()
    }
    /// First 8 bytes as an integer, used as a cheap dedup key.
    pub fn key(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().unwrap())
    }
}

impl fmt::Display for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.b58())
    }
}
impl fmt::Debug for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.b58())
    }
}
impl Serialize for Sig {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.b58())
    }
}
impl<'de> Deserialize<'de> for Sig {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Sig::parse(&s).map_err(D::Error::custom)
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}


pub const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

pub fn sol(l: u64) -> f64 {
    l as f64 / LAMPORTS_PER_SOL
}

pub fn sol_i(l: i64) -> f64 {
    l as f64 / LAMPORTS_PER_SOL
}

pub fn lamports(s: f64) -> u64 {
    if s <= 0.0 {
        0
    } else {
        (s * LAMPORTS_PER_SOL).round() as u64
    }
}

/// Round to 6 decimals so JSON logs stay readable.
pub fn r6(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    (x * 1e6).round() / 1e6
}
