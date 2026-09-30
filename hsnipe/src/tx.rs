//! Minimal legacy-transaction builder: compile instructions into a message, sign with
//! ed25519, and serialize to wire format. Plus the few system/token/ATA instructions
//! the bot needs and the PDA/ATA derivations used for account substitution.

use crate::pump::{ATA_PROGRAM, COMPUTE_BUDGET, SYSTEM};
use crate::types::Pk;
use solana_keypair::Keypair;
use solana_signer::Signer;

#[derive(Clone, Debug)]
pub struct Meta {
    pub pk: Pk,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct Ix {
    pub program: Pk,
    pub accounts: Vec<Meta>,
    pub data: Vec<u8>,
}

pub fn m(pk: Pk, signer: bool, writable: bool) -> Meta {
    Meta { pk, signer, writable }
}

pub fn ata(owner: &Pk, mint: &Pk, token_program: &Pk) -> Pk {
    pda(&[&owner.0, &token_program.0, &mint.0], &ATA_PROGRAM)
}

pub fn pda(seeds: &[&[u8]], program: &Pk) -> Pk {
    Pk::from_sol(&solana_pubkey::Pubkey::find_program_address(seeds, &program.sol()).0)
}

pub fn set_cu_limit(units: u32) -> Ix {
    let mut d = vec![2u8];
    d.extend_from_slice(&units.to_le_bytes());
    Ix { program: *COMPUTE_BUDGET, accounts: vec![], data: d }
}

pub fn set_cu_price(micro_lamports: u64) -> Ix {
    let mut d = vec![3u8];
    d.extend_from_slice(&micro_lamports.to_le_bytes());
    Ix { program: *COMPUTE_BUDGET, accounts: vec![], data: d }
}

pub fn transfer(from: Pk, to: Pk, lamports: u64) -> Ix {
    let mut d = vec![2u8, 0, 0, 0];
    d.extend_from_slice(&lamports.to_le_bytes());
    Ix { program: *SYSTEM, accounts: vec![m(from, true, true), m(to, false, true)], data: d }
}

pub fn create_ata_idempotent(payer: Pk, owner: Pk, mint: Pk, token_program: Pk) -> Ix {
    Ix {
        program: *ATA_PROGRAM,
        accounts: vec![
            m(payer, true, true),
            m(ata(&owner, &mint, &token_program), false, true),
            m(owner, false, false),
            m(mint, false, false),
            m(*SYSTEM, false, false),
            m(token_program, false, false),
        ],
        data: vec![1],
    }
}

pub fn close_account(token_program: Pk, account: Pk, dest: Pk, owner: Pk) -> Ix {
    Ix {
        program: token_program,
        accounts: vec![m(account, false, true), m(dest, false, true), m(owner, true, false)],
        data: vec![9],
    }
}

fn shortvec(out: &mut Vec<u8>, mut n: usize) {
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

/// Compile + serialize a legacy message. Returns the message bytes.
pub fn compile(payer: &Pk, ixs: &[Ix], blockhash: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    // Merge account flags; payer first.
    let mut keys: Vec<Meta> = vec![m(*payer, true, true)];
    let mut upsert = |pk: Pk, signer: bool, writable: bool| {
        if let Some(k) = keys.iter_mut().find(|k| k.pk == pk) {
            k.signer |= signer;
            k.writable |= writable;
        } else {
            keys.push(m(pk, signer, writable));
        }
    };
    for ix in ixs {
        for a in &ix.accounts {
            upsert(a.pk, a.signer, a.writable);
        }
        upsert(ix.program, false, false);
    }
    // Order: writable signers, readonly signers, writable non-signers, readonly non-signers.
    let rank = |k: &Meta| match (k.signer, k.writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    let payer_meta = keys.remove(0);
    keys.sort_by_key(rank); // stable: keeps first-seen order inside each class
    keys.insert(0, payer_meta);
    if keys.len() > 256 {
        anyhow::bail!("too many accounts");
    }
    let n_sig = keys.iter().filter(|k| k.signer).count();
    let n_ro_sig = keys.iter().filter(|k| k.signer && !k.writable).count();
    let n_ro_unsig = keys.iter().filter(|k| !k.signer && !k.writable).count();
    let index = |pk: &Pk| keys.iter().position(|k| k.pk == *pk).unwrap() as u8;

    let mut out = Vec::with_capacity(1024);
    out.push(n_sig as u8);
    out.push(n_ro_sig as u8);
    out.push(n_ro_unsig as u8);
    shortvec(&mut out, keys.len());
    for k in &keys {
        out.extend_from_slice(&k.pk.0);
    }
    out.extend_from_slice(blockhash);
    shortvec(&mut out, ixs.len());
    for ix in ixs {
        out.push(index(&ix.program));
        shortvec(&mut out, ix.accounts.len());
        for a in &ix.accounts {
            out.push(index(&a.pk));
        }
        shortvec(&mut out, ix.data.len());
        out.extend_from_slice(&ix.data);
    }
    Ok(out)
}

/// Sign and serialize (single signer = payer). Returns (signature, wire bytes).
pub fn sign(kp: &Keypair, msg: &[u8]) -> ([u8; 64], Vec<u8>) {
    let sig = kp.sign_message(msg);
    let sig_bytes: [u8; 64] = sig.into();
    let mut tx = Vec::with_capacity(1 + 64 + msg.len());
    shortvec(&mut tx, 1);
    tx.extend_from_slice(&sig_bytes);
    tx.extend_from_slice(msg);
    (sig_bytes, tx)
}

pub const MAX_TX_SIZE: usize = 1232;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortvec_encoding() {
        let mut v = vec![];
        shortvec(&mut v, 0x7f);
        assert_eq!(v, vec![0x7f]);
        v.clear();
        shortvec(&mut v, 0x80);
        assert_eq!(v, vec![0x80, 0x01]);
        v.clear();
        shortvec(&mut v, 0x3fff);
        assert_eq!(v, vec![0xff, 0x7f]);
    }

    #[test]
    fn known_ata_derivation() {
        // Pool 6ZG6... base ATA taken from a mainnet PumpSwap sell (owner=pool account).
        let wsol = *crate::pump::WSOL;
        let token = *crate::pump::TOKEN;
        // ATA of the PumpSwap GlobalConfig PDA check: global_config = PDA("global_config").
        let gc = pda(&[b"global_config"], &crate::pump::AMM);
        assert_eq!(gc.b58(), "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw");
        let _ = ata(&gc, &wsol, &token);
    }

    #[test]
    fn compile_orders_and_signs() {
        let kp = Keypair::new();
        let payer = Pk(kp.pubkey().to_bytes());
        let to = Pk([7u8; 32]);
        let ixs = vec![set_cu_limit(100_000), set_cu_price(1000), transfer(payer, to, 5000)];
        let msg = compile(&payer, &ixs, &[1u8; 32]).unwrap();
        assert_eq!(&msg[..3], &[1, 0, 2]);
        assert_eq!(msg[3], 4); // payer, to, compute budget, system
        assert_eq!(&msg[4..36], &payer.0);
        assert_eq!(&msg[36..68], &to.0);
        let (sig, wire) = sign(&kp, &msg);
        assert_eq!(wire.len(), 1 + 64 + msg.len());
        let s = solana_signature::Signature::from(sig);
        assert!(s.verify(&payer.0, &msg));
    }
}
