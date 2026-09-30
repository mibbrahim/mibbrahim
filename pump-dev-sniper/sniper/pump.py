"""pump.fun bonding-curve primitives: constants, PDAs, curve math, event decoding
and instruction builders.

Account layouts follow the official IDL (sniper/pump_idl.json, from
github.com/pump-fun/pump-public-docs) plus the April 2026 breaking change that
appends `bonding-curve-v2` and one of 8 new fee recipients to every buy/sell.
"""
from __future__ import annotations

import base64
import hashlib
import random
import struct
from dataclasses import dataclass

from solders.instruction import AccountMeta, Instruction
from solders.pubkey import Pubkey

PUMP_PROGRAM = Pubkey.from_string("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
FEE_PROGRAM = Pubkey.from_string("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ")
SYSTEM_PROGRAM = Pubkey.from_string("11111111111111111111111111111111")
TOKEN_PROGRAM = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
TOKEN_2022_PROGRAM = Pubkey.from_string("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
ATA_PROGRAM = Pubkey.from_string("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
WSOL_MINT = "So11111111111111111111111111111111111111112"
SOL_QUOTE_MINTS = {None, "", WSOL_MINT, "11111111111111111111111111111111"}

GLOBAL = Pubkey.find_program_address([b"global"], PUMP_PROGRAM)[0]
EVENT_AUTHORITY = Pubkey.find_program_address([b"__event_authority"], PUMP_PROGRAM)[0]
GLOBAL_VOLUME_ACCUMULATOR = Pubkey.find_program_address([b"global_volume_accumulator"], PUMP_PROGRAM)[0]
FEE_CONFIG = Pubkey.find_program_address([b"fee_config", bytes(PUMP_PROGRAM)], FEE_PROGRAM)[0]

# Global::fee_recipient + Global::fee_recipients (any one may be passed as `fee_recipient`).
FEE_RECIPIENTS = [Pubkey.from_string(k) for k in (
    "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV",
    "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ",
    "7hTckgnGnLQR6sdH7YkqFTAA7VwTfYFaZ6EhEsU3saCX",
    "9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz",
    "AVmoTthdrX6tKt4nDjco2D775W2YK3sDhxPcMmzUAmTY",
    "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM",
    "FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz",
    "G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP",
)]
# The 8 fee recipients appended after bonding-curve-v2 (BREAKING_FEE_RECIPIENT.md).
TRAILING_FEE_RECIPIENTS = [Pubkey.from_string(k) for k in (
    "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
    "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
    "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
    "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR",
    "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6",
    "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
    "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
    "A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW",
)]

# Initial curve for SOL-quoted, non-mayhem coins (Global account).
INIT_V_SOL = 30_000_000_000            # lamports
INIT_V_TOK = 1_073_000_000_000_000     # base units (6 decimals)
CURVE_K = INIT_V_SOL * INIT_V_TOK
TOKEN_DECIMALS = 6
LAMPORTS = 1_000_000_000

# Bonding-curve fees: protocol 95 bps + creator fee (default 30, devs may set up to 300).
PROTOCOL_FEE_BPS = 95
DEFAULT_CREATOR_FEE_BPS = 30

# Rent for a Token-2022 ATA with the pump extensions; refunded when closed on sell.
ATA_RENT_LAMPORTS = 2_074_080


def _disc(prefix: str, name: str) -> bytes:
    return hashlib.sha256(f"{prefix}:{name}".encode()).digest()[:8]


IX_BUY = _disc("global", "buy")
IX_BUY_EXACT_SOL_IN = _disc("global", "buy_exact_sol_in")
IX_SELL = _disc("global", "sell")
IX_CREATE = _disc("global", "create")
IX_CREATE_V2 = _disc("global", "create_v2")
EV_TRADE = _disc("event", "TradeEvent")
EV_CREATE = _disc("event", "CreateEvent")
EV_COMPLETE = _disc("event", "CompleteEvent")


# ---------------------------------------------------------------- PDAs

def bonding_curve_pda(mint: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([b"bonding-curve", bytes(mint)], PUMP_PROGRAM)[0]


def bonding_curve_v2_pda(mint: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([b"bonding-curve-v2", bytes(mint)], PUMP_PROGRAM)[0]


def creator_vault_pda(creator: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([b"creator-vault", bytes(creator)], PUMP_PROGRAM)[0]


def user_volume_accumulator_pda(user: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([b"user_volume_accumulator", bytes(user)], PUMP_PROGRAM)[0]


def ata(owner: Pubkey, mint: Pubkey, token_program: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([bytes(owner), bytes(token_program), bytes(mint)], ATA_PROGRAM)[0]


# ---------------------------------------------------------------- curve math

@dataclass
class Curve:
    v_sol: int   # lamports
    v_tok: int   # base units

    @classmethod
    def initial(cls) -> "Curve":
        return cls(INIT_V_SOL, INIT_V_TOK)

    @classmethod
    def from_price(cls, price_sol_per_token: float) -> "Curve":
        """Reconstruct reserves from a UI price, using the constant k of a standard curve."""
        p = price_sol_per_token * LAMPORTS / 10 ** TOKEN_DECIMALS  # lamports per base unit
        v_sol = int((CURVE_K * p) ** 0.5)
        return cls(v_sol, CURVE_K // max(v_sol, 1))

    @property
    def price(self) -> float:
        """SOL per whole token."""
        return (self.v_sol / LAMPORTS) / (self.v_tok / 10 ** TOKEN_DECIMALS)

    @property
    def mcap_sol(self) -> float:
        return self.price * 1_000_000_000

    def tokens_for_sol(self, sol_in_lamports: int) -> int:
        """Tokens out for SOL that reaches the curve (after fees)."""
        k = self.v_sol * self.v_tok
        return self.v_tok - (k // (self.v_sol + sol_in_lamports) + 1)

    def sol_for_tokens(self, tokens: int) -> int:
        """Gross lamports out of the curve for selling `tokens` (before fees)."""
        k = self.v_sol * self.v_tok
        return self.v_sol - (k // (self.v_tok + tokens) + 1)

    def apply_buy(self, sol_in: int, tokens_out: int) -> None:
        self.v_sol += sol_in
        self.v_tok -= tokens_out

    def apply_sell(self, sol_out: int, tokens_in: int) -> None:
        self.v_sol -= sol_out
        self.v_tok += tokens_in


def quote_buy(curve: Curve, spend_lamports: int, fee_bps: int) -> int:
    """Tokens received when spending `spend_lamports` in total (fee included)."""
    net = spend_lamports * 10_000 // (10_000 + fee_bps)
    return curve.tokens_for_sol(net)


def quote_sell(curve: Curve, tokens: int, fee_bps: int) -> int:
    """Net lamports received for selling `tokens`."""
    gross = curve.sol_for_tokens(tokens)
    return gross - (gross * fee_bps + 9_999) // 10_000


# ---------------------------------------------------------------- borsh decoding

class Reader:
    def __init__(self, b: bytes, off: int = 0):
        self.b, self.o = b, off

    def u8(self) -> int:
        v = self.b[self.o]; self.o += 1; return v

    def bool(self) -> bool:
        return self.u8() != 0

    def u64(self) -> int:
        v = struct.unpack_from("<Q", self.b, self.o)[0]; self.o += 8; return v

    def i64(self) -> int:
        v = struct.unpack_from("<q", self.b, self.o)[0]; self.o += 8; return v

    def u128(self) -> int:
        lo, hi = struct.unpack_from("<QQ", self.b, self.o); self.o += 16; return lo | (hi << 64)

    def pubkey(self) -> str:
        v = Pubkey.from_bytes(self.b[self.o:self.o + 32]); self.o += 32; return str(v)

    def string(self) -> str:
        n = struct.unpack_from("<I", self.b, self.o)[0]; self.o += 4
        v = self.b[self.o:self.o + n].decode(errors="replace"); self.o += n; return v

    def remaining(self) -> int:
        return len(self.b) - self.o


@dataclass
class TradeEvent:
    mint: str
    sol_amount: int
    token_amount: int
    is_buy: bool
    user: str
    timestamp: int
    v_sol: int
    v_tok: int
    real_sol: int
    real_tok: int
    fee_bps: int
    creator: str
    creator_fee_bps: int


@dataclass
class CreateEvent:
    name: str
    symbol: str
    uri: str
    mint: str
    bonding_curve: str
    user: str
    creator: str
    timestamp: int
    v_tok: int
    v_sol: int
    real_tok: int
    token_program: str
    is_mayhem: bool
    quote_mint: str
    creator_fee_bps: int = DEFAULT_CREATOR_FEE_BPS

    @property
    def fee_bps(self) -> int:
        return PROTOCOL_FEE_BPS + (self.creator_fee_bps or DEFAULT_CREATOR_FEE_BPS)


def decode_trade(b: bytes) -> TradeEvent:
    r = Reader(b, 8)
    mint, sol, tok, is_buy, user, ts = r.pubkey(), r.u64(), r.u64(), r.bool(), r.pubkey(), r.i64()
    vs, vt, rs, rt = r.u64(), r.u64(), r.u64(), r.u64()
    r.pubkey()  # fee_recipient
    fee_bps = r.u64(); r.u64()
    creator = r.pubkey(); cfee_bps = r.u64()
    return TradeEvent(mint, sol, tok, is_buy, user, ts, vs, vt, rs, rt, fee_bps, creator, cfee_bps)


def curve_account_fee_bps(data: bytes) -> tuple[int, bool, str]:
    """(total fee bps per side, is_mayhem, quote_mint) from a BondingCurve account."""
    creator_fee = struct.unpack_from("<Q", data, 115)[0] if len(data) >= 123 else 0
    is_mayhem = len(data) > 81 and data[81] == 1
    quote = str(Pubkey.from_bytes(data[83:115])) if len(data) >= 115 else WSOL_MINT
    return PROTOCOL_FEE_BPS + (creator_fee or DEFAULT_CREATOR_FEE_BPS), is_mayhem, quote


def decode_create(b: bytes) -> CreateEvent:
    r = Reader(b, 8)
    name, symbol, uri = r.string(), r.string(), r.string()
    mint, bc, user, creator = r.pubkey(), r.pubkey(), r.pubkey(), r.pubkey()
    ts, vt, vs, rt = r.i64(), r.u64(), r.u64(), r.u64()
    r.u64()  # total supply
    token_program = str(TOKEN_PROGRAM)
    is_mayhem, quote_mint, creator_fee = False, WSOL_MINT, 0
    if r.remaining() >= 32:
        token_program = r.pubkey()
    if r.remaining() >= 1:
        is_mayhem = r.bool()
    if r.remaining() >= 33:
        r.bool()  # is_cashback_enabled
        quote_mint = r.pubkey()
    if r.remaining() >= 16:
        r.u64()  # virtual_quote_reserves
        creator_fee = r.u64()
    return CreateEvent(name, symbol, uri, mint, bc, user, creator, ts, vt, vs, rt, token_program, is_mayhem,
                       quote_mint, creator_fee)


def decode_program_data(log_line: str):
    """Decode an Anchor `Program data:` log line into a Trade/Create event (or None)."""
    if not log_line.startswith("Program data: "):
        return None
    try:
        b = base64.b64decode(log_line[14:])
    except Exception:
        return None
    try:
        if b[:8] == EV_TRADE:
            return decode_trade(b)
        if b[:8] == EV_CREATE:
            return decode_create(b)
    except (struct.error, IndexError, ValueError):
        return None
    return None


# ---------------------------------------------------------------- fees

@dataclass
class FeeTier:
    mcap_threshold_lamports: int
    total_bps: int


def decode_fee_config(data: bytes) -> list[FeeTier]:
    """Parse the FeeConfig account into tiers of total (protocol+creator+lp) bps."""
    r = Reader(data, 8)
    r.u8(); r.pubkey()
    flat = r.u64() + r.u64() + r.u64()
    tiers = []
    n = struct.unpack_from("<I", r.b, r.o)[0]; r.o += 4
    for _ in range(n):
        thr = r.u128()
        tiers.append(FeeTier(thr, r.u64() + r.u64() + r.u64()))
    return tiers or [FeeTier(0, flat)]


def fee_bps_for_mcap(tiers: list[FeeTier], mcap_lamports: int) -> int:
    bps = tiers[0].total_bps
    for t in tiers:
        if mcap_lamports >= t.mcap_threshold_lamports:
            bps = t.total_bps
    return bps


# ---------------------------------------------------------------- instructions

@dataclass
class CoinKeys:
    mint: Pubkey
    creator: Pubkey
    token_program: Pubkey

    @property
    def bonding_curve(self) -> Pubkey:
        return bonding_curve_pda(self.mint)

    @property
    def associated_bonding_curve(self) -> Pubkey:
        return ata(self.bonding_curve, self.mint, self.token_program)


def _trade_accounts(coin: CoinKeys, user: Pubkey, sell: bool, cashback: bool) -> list[AccountMeta]:
    fee_recipient = random.choice(FEE_RECIPIENTS)
    base = [
        AccountMeta(GLOBAL, False, False),
        AccountMeta(fee_recipient, False, True),
        AccountMeta(coin.mint, False, False),
        AccountMeta(coin.bonding_curve, False, True),
        AccountMeta(coin.associated_bonding_curve, False, True),
        AccountMeta(ata(user, coin.mint, coin.token_program), False, True),
        AccountMeta(user, True, True),
        AccountMeta(SYSTEM_PROGRAM, False, False),
    ]
    vault = AccountMeta(creator_vault_pda(coin.creator), False, True)
    tp = AccountMeta(coin.token_program, False, False)
    base += [vault, tp] if sell else [tp, vault]
    base += [AccountMeta(EVENT_AUTHORITY, False, False), AccountMeta(PUMP_PROGRAM, False, False)]
    if not sell:
        base += [
            AccountMeta(GLOBAL_VOLUME_ACCUMULATOR, False, False),
            AccountMeta(user_volume_accumulator_pda(user), False, True),
        ]
    base += [AccountMeta(FEE_CONFIG, False, False), AccountMeta(FEE_PROGRAM, False, False)]
    if sell and cashback:
        base.append(AccountMeta(user_volume_accumulator_pda(user), False, True))
    base += [
        AccountMeta(bonding_curve_v2_pda(coin.mint), False, False),
        AccountMeta(random.choice(TRAILING_FEE_RECIPIENTS), False, True),
    ]
    return base


def ix_buy_exact_sol_in(coin: CoinKeys, user: Pubkey, spend_lamports: int, min_tokens_out: int) -> Instruction:
    # args: spendable_sol_in u64, min_tokens_out u64, track_volume OptionBool(bool)
    data = IX_BUY_EXACT_SOL_IN + struct.pack("<QQB", spend_lamports, min_tokens_out, 1)
    return Instruction(PUMP_PROGRAM, data, _trade_accounts(coin, user, sell=False, cashback=False))


def ix_sell(coin: CoinKeys, user: Pubkey, tokens: int, min_sol_out: int, cashback: bool = False) -> Instruction:
    data = IX_SELL + struct.pack("<QQ", tokens, min_sol_out)
    return Instruction(PUMP_PROGRAM, data, _trade_accounts(coin, user, sell=True, cashback=cashback))


def ix_create_ata_idempotent(payer: Pubkey, owner: Pubkey, mint: Pubkey, token_program: Pubkey) -> Instruction:
    return Instruction(ATA_PROGRAM, bytes([1]), [
        AccountMeta(payer, True, True),
        AccountMeta(ata(owner, mint, token_program), False, True),
        AccountMeta(owner, False, False),
        AccountMeta(mint, False, False),
        AccountMeta(SYSTEM_PROGRAM, False, False),
        AccountMeta(token_program, False, False),
    ])


def ix_close_account(account: Pubkey, dest: Pubkey, owner: Pubkey, token_program: Pubkey) -> Instruction:
    return Instruction(token_program, bytes([9]), [
        AccountMeta(account, False, True),
        AccountMeta(dest, False, True),
        AccountMeta(owner, True, False),
    ])
