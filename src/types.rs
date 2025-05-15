use alloy::primitives::{Address as AlloyAddress, B256 as AlloyB256, Bytes as AlloyBytes, U256 as AlloyU256, I256 as AlloyI256, StorageKey as AlloyStorageKey, FixedBytes as AlloyFixedBytes};
use alloy::rpc::types::{AccountInfo as AlloyAccountInfo, BlockId, BlockNumberOrTag};
// Fix imports for primitive types
use revm_primitives::{Address as RevmAddress, B256 as RevmB256, Bytes as RevmBytes, U256 as RevmU256, I256 as RevmI256};
// Fix imports for AccountInfo
use cast::revm::interpreter::primitives::AccountInfo as RevmAccountInfo;
use revm::context_interface::TransactTo;
use revm_primitives::Log as RevmLog;
use reth::revm::db::StorageSlot;
use reth::revm::db::DatabaseRef;
// AccountState might be in a different location or renamed
// // Commented out as it's causing an error - need to find the correct path
// use revm::primitives::state::AccountState;
use revm_primitives::hardfork::SpecId;
use alloy::network::Network;
use alloy::primitives::Log as AlloyLog;

/// Trait for converting types from alloy to revmw1
pub trait IntoRevm<T> {
    fn into_revm(self) -> T;
}

/// Trait for converting types from revm to alloy
pub trait IntoAlloy<T> {
    fn into_alloy(self) -> T;
}

// Address conversions
impl IntoRevm<RevmAddress> for AlloyAddress {
    fn into_revm(self) -> RevmAddress {
        RevmAddress::from_slice(self.as_slice())
    }
}

impl IntoAlloy<AlloyAddress> for RevmAddress {
    fn into_alloy(self) -> AlloyAddress {
        AlloyAddress::from_slice(self.as_slice())
    }
}

// B256 conversions
impl IntoRevm<RevmB256> for AlloyB256 {
    fn into_revm(self) -> RevmB256 {
        RevmB256::from_slice(self.as_slice())
    }
}

impl IntoAlloy<AlloyB256> for RevmB256 {
    fn into_alloy(self) -> AlloyB256 {
        AlloyB256::from_slice(self.as_slice())
    }
}

// U256 conversions
impl IntoRevm<RevmU256> for AlloyU256 {
    fn into_revm(self) -> RevmU256 {
        // Convert using the byte representation to avoid limbs issues
        RevmU256::from_be_bytes(self.to_be_bytes())
    }
}

impl IntoAlloy<AlloyU256> for RevmU256 {
    fn into_alloy(self) -> AlloyU256 {
        // Convert using the byte representation to avoid limbs issues
        AlloyU256::from_be_bytes(self.to_be_bytes())
    }
}

// I256 conversions
impl IntoRevm<RevmI256> for AlloyI256 {
    fn into_revm(self) -> RevmI256 {
        let (sign, abs) = self.into_sign_and_abs();
        let abs_revm = abs.into_revm();
        if !sign.is_positive() {
            -RevmI256::from_raw(abs_revm)
        } else {
            RevmI256::from_raw(abs_revm)
        }
    }
}

impl IntoAlloy<AlloyI256> for RevmI256 {
    fn into_alloy(self) -> AlloyI256 {
        let (sign, abs) = self.into_sign_and_abs();
        let abs_alloy = abs.into_alloy();
        if sign {
            -AlloyI256::from_raw(abs_alloy)
        } else {
            AlloyI256::from_raw(abs_alloy)
        }
    }
}

// StorageKey conversions
impl IntoRevm<RevmB256> for AlloyStorageKey {
    fn into_revm(self) -> RevmB256 {
        RevmB256::from_slice(self.as_slice())
    }
}

impl IntoAlloy<AlloyStorageKey> for RevmB256 {
    fn into_alloy(self) -> AlloyStorageKey {
        AlloyStorageKey::from_slice(self.as_slice())
    }
}

// We're removing these implementations to avoid conflicts with B256 conversions
// FixedBytes<32> is essentially the same as B256, so we can use those conversions instead

// Bytes conversions
impl IntoRevm<RevmBytes> for AlloyBytes {
    fn into_revm(self) -> RevmBytes {
        RevmBytes::from(self.to_vec())
    }
}

impl IntoAlloy<AlloyBytes> for RevmBytes {
    fn into_alloy(self) -> AlloyBytes {
        AlloyBytes::from(self.to_vec())
    }
}

// AccountInfo conversions
impl IntoRevm<RevmAccountInfo> for AlloyAccountInfo {
    fn into_revm(self) -> RevmAccountInfo {
        // We don't need code_hash as it's not a field in RevmAccountInfo
        RevmAccountInfo {
            nonce: self.nonce,
            balance: self.balance.into_revm(),
            code: self.code.map(|code| code.into_revm()),
        }
    }
}

impl IntoAlloy<AlloyAccountInfo> for RevmAccountInfo {
    fn into_alloy(self) -> AlloyAccountInfo {
        AlloyAccountInfo {
            nonce: self.nonce,
            balance: self.balance.into_alloy(),
            code: self.code.map(|code| code.into_alloy()),
        }
    }
}

// Log conversions
impl IntoRevm<RevmLog> for AlloyLog {
    fn into_revm(self) -> RevmLog {
        RevmLog {
            address: self.address.into_revm(),
            data: self.data.into_revm(),
        }
    }
}

impl IntoAlloy<AlloyLog> for RevmLog {
    fn into_alloy(self) -> AlloyLog {
        AlloyLog::new(
            self.address.into_alloy(),
            vec![],  // Empty topics since RevmLog doesn't have topics field
            self.data.into_alloy(),
        )
    }
}

// BlockId to u64 conversion helper
pub fn block_id_to_number(block_id: BlockId) -> Option<u64> {
    match block_id {
        BlockId::Number(num) => match num {
            BlockNumberOrTag::Number(n) => Some(n.into()), // Changed to() to into()
            BlockNumberOrTag::Latest => None, // Latest block, would need to be fetched
            BlockNumberOrTag::Pending => None, // Pending block
            BlockNumberOrTag::Earliest => Some(0), // Genesis block
            BlockNumberOrTag::Safe => None, // Safe block
            BlockNumberOrTag::Finalized => None, // Finalized block
        },
        BlockId::Hash(_) => None, // Would need to look up the number from the hash
    }
}

// Chain ID conversions
pub fn chain_id_to_spec_id(network: Network) -> SpecId {
    match network.chain_id() {
        1 => SpecId::SHANGHAI,    // Mainnet is on Shanghai
        5 => SpecId::SHANGHAI,    // Goerli is on Shanghai
        11155111 => SpecId::SHANGHAI, // Sepolia is on Shanghai
        _ => SpecId::SHANGHAI,    // Default to Shanghai for unknown chains
    }
}

// TransactTo helper
pub fn address_to_transact_to(address: Option<AlloyAddress>) -> TransactTo {
    match address {
        Some(addr) => TransactTo::Call(addr.into_revm()),
        None => TransactTo::Create,
    }
}

// StorageSlot helper
pub fn create_storage_slot(key: AlloyB256, value: AlloyU256) -> StorageSlot {
    StorageSlot {
        key: key.into_revm(),
        value: value.into_revm(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_conversions() {
        let alloy_addr = AlloyAddress::from([1u8; 20]);
        let revm_addr = alloy_addr.into_revm();
        let back_to_alloy = revm_addr.into_alloy();
        assert_eq!(alloy_addr, back_to_alloy);
    }

    #[test]
    fn test_b256_conversions() {
        let alloy_b256 = AlloyB256::from([1u8; 32]);
        let revm_b256 = alloy_b256.into_revm();
        let back_to_alloy = revm_b256.into_alloy();
        assert_eq!(alloy_b256, back_to_alloy);
    }

    #[test]
    fn test_u256_conversions() {
        let alloy_u256 = AlloyU256::from(1234u64);
        let revm_u256 = alloy_u256.into_revm();
        let back_to_alloy = revm_u256.into_alloy();
        assert_eq!(alloy_u256, back_to_alloy);
    }

    #[test]
    fn test_bytes_conversions() {
        let data = vec![1u8, 2u8, 3u8];
        let alloy_bytes = AlloyBytes::from(data.clone());
        let revm_bytes = alloy_bytes.clone().into_revm();
        let back_to_alloy = revm_bytes.into_alloy();
        assert_eq!(alloy_bytes, back_to_alloy);
    }
    
    #[test]
    fn test_i256_conversions() {
        // Test positive number
        let alloy_i256_pos = AlloyI256::from_raw(AlloyU256::from(1234u64));
        let revm_i256_pos = alloy_i256_pos.into_revm();
        let back_to_alloy_pos = revm_i256_pos.into_alloy();
        assert_eq!(alloy_i256_pos, back_to_alloy_pos);
        
        // Test negative number
        let alloy_i256_neg = -AlloyI256::from_raw(AlloyU256::from(5678u64));
        let revm_i256_neg = alloy_i256_neg.into_revm();
        let back_to_alloy_neg = revm_i256_neg.into_alloy();
        assert_eq!(alloy_i256_neg, back_to_alloy_neg);
    }
    
    #[test]
    fn test_storage_key_conversions() {
        let alloy_storage_key = AlloyStorageKey::from([1u8; 32]);
        let revm_b256 = alloy_storage_key.into_revm();
        let back_to_alloy = revm_b256.into_alloy();
        assert_eq!(alloy_storage_key, back_to_alloy);
    }
    
    #[test]
    fn test_fixed_bytes_conversions() {
        let alloy_fixed_bytes = AlloyFixedBytes::<32>::from([1u8; 32]);
        let revm_b256 = alloy_fixed_bytes.into_revm();
        let back_to_alloy = revm_b256.into_alloy();
        assert_eq!(alloy_fixed_bytes, back_to_alloy);
    }

    #[test]
    fn test_log_conversions() {
        let alloy_log = AlloyLog {
            address: AlloyAddress::from([1u8; 20]),
            topics: vec![AlloyB256::from([2u8; 32]), AlloyB256::from([3u8; 32])],
            data: AlloyBytes::from(vec![4u8, 5u8, 6u8]),
        };
        let revm_log = alloy_log.clone().into_revm();
        let back_to_alloy = revm_log.into_alloy();
        assert_eq!(alloy_log.address, back_to_alloy.address);
        assert_eq!(alloy_log.topics, back_to_alloy.topics);
        assert_eq!(alloy_log.data, back_to_alloy.data);
    }

    #[test]
    fn test_block_id_to_number() {
        assert_eq!(block_id_to_number(BlockId::Number(BlockNumberOrTag::Number(42.into()))), Some(42));
        assert_eq!(block_id_to_number(BlockId::Number(BlockNumberOrTag::Earliest)), Some(0));
        assert_eq!(block_id_to_number(BlockId::Number(BlockNumberOrTag::Latest)), None);
    }

    #[test]
    fn test_address_to_transact_to() {
        let addr = AlloyAddress::from([1u8; 20]);
        match address_to_transact_to(Some(addr)) {
            TransactTo::Call(revm_addr) => assert_eq!(revm_addr, addr.into_revm()),
            _ => panic!("Expected TransactTo::Call"),
        }
        
        match address_to_transact_to(None) {
            TransactTo::Create => {},
            _ => panic!("Expected TransactTo::Create"),
        }
    }
}
