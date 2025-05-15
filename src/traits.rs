use alloy::primitives::{Address as AlloyAddress, B256 as AlloyB256, Bytes as AlloyBytes, U256 as AlloyU256};
use alloy::rpc::types::AccountInfo as AlloyAccountInfo;
// Fix the import path for AccountInfo
use reth::rpc::types::{Address as RevmAddress, B256 as RevmB256, Bytes as RevmBytes, U256 as RevmU256};
// Fix the import path for AccountInfo
use reth::rpc::types::AccountInfo as RevmAccountInfo;

/// Trait for types that can be converted to revm types
pub trait IntoRevm<T> {
    fn into_revm(self) -> T;
}

/// Trait for types that can be converted to alloy types
pub trait IntoAlloy<T> {
    fn into_alloy(self) -> T;
}

// Address conversions
impl IntoRevm<RevmAddress> for AlloyAddress {
    fn into_revm(self) -> RevmAddress {
        RevmAddress(self.into_array())
    }
}

impl IntoAlloy<AlloyAddress> for RevmAddress {
    fn into_alloy(self) -> AlloyAddress {
        AlloyAddress::from_slice(&self.0)
    }
}

// B256 conversions
impl IntoRevm<RevmB256> for AlloyB256 {
    fn into_revm(self) -> RevmB256 {
        RevmB256(self.into_array())
    }
}

impl IntoAlloy<AlloyB256> for RevmB256 {
    fn into_alloy(self) -> AlloyB256 {
        AlloyB256::from_slice(&self.0)
    }
}

// U256 conversions
impl IntoRevm<RevmU256> for AlloyU256 {
    fn into_revm(self) -> RevmU256 {
        RevmU256::from_limbs(self.as_limbs())
    }
}

impl IntoAlloy<AlloyU256> for RevmU256 {
    fn into_alloy(self) -> AlloyU256 {
        AlloyU256::from_limbs(self.as_limbs())
    }
}

// Bytes conversions
impl IntoRevm<RevmBytes> for AlloyBytes {
    fn into_revm(self) -> RevmBytes {
        RevmBytes(self.into())
    }
}

impl IntoAlloy<AlloyBytes> for RevmBytes {
    fn into_alloy(self) -> AlloyBytes {
        AlloyBytes::from(self.0)
    }
}

// AccountInfo conversions
impl IntoRevm<RevmAccountInfo> for AlloyAccountInfo {
    fn into_revm(self) -> RevmAccountInfo {
        // Check if the field exists in the struct
        // If code_hash doesn't exist in AlloyAccountInfo, use a default value
        let code_hash = match self.code {
            Some(ref code) => code.hash_slow(),
            None => RevmB256::default(),
        };
        
        RevmAccountInfo {
            nonce: self.nonce,
            balance: self.balance.into_revm(),
            code_hash,
            code: self.code.map(|code| code.into_revm()),
        }
    }
}

impl IntoAlloy<AlloyAccountInfo> for RevmAccountInfo {
    fn into_alloy(self) -> AlloyAccountInfo {
        AlloyAccountInfo {
            nonce: self.nonce,
            balance: self.balance.into_alloy(),
            // If code_hash doesn't exist in AlloyAccountInfo, don't include it
            code: self.code.map(|code| code.into_alloy()),
        }
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
}