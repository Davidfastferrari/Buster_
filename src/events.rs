use alloy_primitives::Address;
use alloy_primitives::U256;
use alloy::rpc::types::Header;
use std::collections::HashSet;

use crate::gen_::FlashQuoter::SwapParams;
use crate::swap::SwapPath;

#[derive(Debug, Clone)]
pub enum Event {
    ArbPath((SwapPath, U256, u64)),
    ValidPath((SwapParams, U256, u64)),
    PoolsTouched(HashSet<Address>, u64),
    NewBlock(Header),
}
