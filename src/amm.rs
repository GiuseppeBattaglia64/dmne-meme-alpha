// DMNE QUANT // IN-MEMORY AMM & BONDING CURVE DECODER
// Sub-Microsecond Local Mathematical State Machine for Pump.fun and Raydium Pools

use serde::{Deserialize, Serialize};

pub const PUMP_CURVE_DISCRIMINATOR: [u8; 8] = [0x17, 0xb7, 0xf8, 0x37, 0x45, 0x61, 0xac, 0x6e];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpBondingCurveState {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}

impl PumpBondingCurveState {
    /// Decodes raw account bytes from Solana AccountInfo data
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 49 {
            return Err("Payload insufficient for Pump.fun curve layout");
        }
        
        let virtual_token_reserves = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let virtual_sol_reserves = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let real_token_reserves = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let real_sol_reserves = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let token_total_supply = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        let complete = bytes[48] != 0;

        Ok(Self {
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves,
            real_sol_reserves,
            token_total_supply,
            complete,
        })
    }

    /// Computes instantaneous price in SOL per Token directly from virtual reserves
    pub fn calculate_price_sol(&self) -> f64 {
        if self.virtual_token_reserves == 0 {
            return 0.0;
        }
        let sol_amount = self.virtual_sol_reserves as f64 / 1_000_000_000.0;
        let token_amount = self.virtual_token_reserves as f64 / 1_000_000.0;
        sol_amount / token_amount
    }

    /// Computes instantaneous price impact percentage for an incoming swap
    pub fn calculate_price_impact(&self, swap_sol_lamports: u64) -> f64 {
        if self.virtual_sol_reserves == 0 {
            return 1.0;
        }
        (swap_sol_lamports as f64) / ((self.virtual_sol_reserves + swap_sol_lamports) as f64)
    }
}

