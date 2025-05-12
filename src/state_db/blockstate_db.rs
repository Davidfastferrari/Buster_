use alloy::transports::Transport;
// use alloy::network::primitives::HeaderResponse; // Unused
use alloy::network::Network;
// use alloy::network::BlockResponse; // Unused
use alloy::primitives::{Address as AlloyAddress, BlockNumber, B256 as AlloyB256, U256};
use alloy::providers::Provider;
use alloy::rpc::types::trace::geth::AccountState as AlloyGethAccountState; // Keep for original intent if necessary, but revm's will be prioritized for DB commit logic
use alloy::rpc::types::AccountInfo as AlloyAccountInfo;
use alloy::primitives::Bytes as AlloyBytes; // For constructing AlloyAccountInfo.code
// use alloy::json_abi::ContractObject::Bytecode; // This seems to be revm's Bytecode
use alloy::transports::TransportError;
// use alloy::rpc::types::Block; // Unused
use alloy::eips::BlockId;
use anyhow::Result;
use log::{debug, trace, warn};
use pool_sync::PoolInfo; // Assuming this is the correct PoolInfo
use revm::{
    primitives::{AccountInfo as RevmAccountInfo, Bytecode, Account as RevmAccount, AccountState as RevmAccountState, B256 as RevmB256, Address as RevmAddress, KECCAK_EMPTY, Log},
    Database, DatabaseCommit, DatabaseRef,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::IntoFuture;
use tokio::runtime::{Handle, Runtime}; // Added Runtime

use crate::types::{IntoRevm, IntoAlloy}; // Crucial for conversions

#[derive(Debug)]
pub enum HandleOrRuntime {
    Handle(Handle),
    Runtime(Runtime), // Added Runtime variant
}

impl HandleOrRuntime {
    #[inline]
    pub fn block_on<F>(&self, f: F) -> F::Output
    where
        F: std::future::Future + Send,
        F::Output: Send,
    {
        match self {
            Self::Handle(handle) => tokio::task::block_in_place(move || handle.block_on(f)),
            Self::Runtime(rt) => rt.block_on(f),
        }
    }
}

#[derive(Debug)]
pub struct BlockStateDB<T: Transport + Clone, N: Network, P: Provider<N>> {
    pub accounts: HashMap<AlloyAddress, BlockStateDBAccount>, // Key is AlloyAddress
    pub contracts: HashMap<RevmB256, Bytecode>, // Key is RevmB256 (code_hash from revm)
    pub _logs: Vec<Log>,
    pub block_hashes: HashMap<BlockNumber, AlloyB256>, // Value is AlloyB256
    pub pools: HashSet<AlloyAddress>,
    pub pool_info: HashMap<AlloyAddress, Pool>, // Assuming Pool comes from pool_sync or similar
    provider: P,
    runtime: HandleOrRuntime,
    _marker: std::marker::PhantomData<fn() -> (T, N)>,
}

// Corrected impl block
impl<T, N, P> BlockStateDB<T, N, P>
where
    T: Transport + Clone,
    N: Network,
    P: Provider<N>,
{
    pub fn new(provider: P) -> Option<Self> {
        debug!("Creating new BlockStateDB");
        let mut contracts = HashMap::new(); // Changed to mut
        contracts.insert(KECCAK_EMPTY, Bytecode::default());
        // contracts.insert(RevmB256::ZERO, Bytecode::default()); // RevmB256::ZERO if it exists, or use KECCAK_EMPTY for both

        let rt = match Handle::try_current() {
            Ok(handle) => HandleOrRuntime::Handle(handle),
            Err(_) => {
                // Fallback to creating a new runtime if not in tokio context
                match Runtime::new() {
                    Ok(runtime) => HandleOrRuntime::Runtime(runtime),
                    Err(_) => return None, // Failed to create runtime
                }
            }
        };

        Some(Self {
            accounts: HashMap::new(),
            contracts,
            _logs: Vec::new(),
            block_hashes: HashMap::new(),
            pools: HashSet::new(),
            pool_info: HashMap::new(),
            provider,
            runtime: rt,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn add_pool(&mut self, pool: Pool) { // Assuming pool_sync::Pool
        let pool_address = pool.address(); // Assuming pool.address() returns AlloyAddress
        trace!("Adding pool {} to database", pool_address);

        self.pools.insert(pool_address);
        self.pool_info.insert(pool_address, pool.clone()); // pool might need to be PoolInfo or share relevant parts

        // Fetch the onchain pool account and insert it into database
        // This is onchain because it has onchain state, the slots will be custom
        // basic_ref expects RevmAddress
        match <Self as DatabaseRef>::basic_ref(self, pool_address.into_revm()) {
            Ok(Some(revm_pool_account_info)) => {
                let new_db_account = BlockStateDBAccount {
                    info: revm_pool_account_info.into_alloy(), // Convert RevmAccountInfo to AlloyAccountInfo
                    insertion_type: InsertionType::OnChain,
                    state: RevmAccountState::default(), // Default state
                    storage: HashMap::new(),
                };
                self.accounts.insert(pool_address, new_db_account);
            }
            Ok(None) => {
                warn!("Could not find account info for pool {}", pool_address);
                 let new_db_account = BlockStateDBAccount {
                    info: AlloyAccountInfo::default(), // Default AlloyAccountInfo
                    insertion_type: InsertionType::OnChain,
                    state: RevmAccountState::default(), // Default state
                    storage: HashMap::new(),
                };
                self.accounts.insert(pool_address, new_db_account);
            }
            Err(e) => {
                warn!("Error fetching basic_ref for pool {}: {:?}", pool_address, e);
            }
        }
    }

    pub fn get_pool(&self, pool_address: &AlloyAddress) -> &Pool {
        self.pool_info.get(pool_address).unwrap()
    }

    #[inline]
    pub fn tracking_pool(&self, pool: &AlloyAddress) -> bool {
        self.pools.contains(pool)
    }

    #[inline]
    pub fn zero_to_one(&self, pool: &AlloyAddress, token_in: AlloyAddress) -> Option<bool> {
        self.pool_info
            .get(pool)
            .map(|info| info.token0_address() == token_in) // Assuming PoolInfo has token0_address
    }

    #[inline]
    pub fn update_all_slots(
        &mut self,
        address: AlloyAddress, // address is AlloyAddress
        account_state: AlloyGethAccountState, // This is from alloy trace
    ) -> Result<()> {
        trace!(
            "Update all slots: updating all storage slots for adddress {}",
            address
        );
        if let Some(alloy_storage) = account_state.storage { // storage is Option<HashMap<B256, B256>>
            for (slot_b256, value_b256) in alloy_storage {
                if let Some(account) = self.accounts.get_mut(&address) {
                    let new_slot_val = BlockStateDBSlot {
                        value: U256::from_be_bytes(value_b256.0), // Convert AlloyB256 to U256
                        insertion_type: InsertionType::Custom,
                    };
                    account.storage.insert(U256::from_be_bytes(slot_b256.0), new_slot_val); // Convert AlloyB256 to U256
                }
            }
        }
        Ok(())
    }

    pub fn insert_account_info(
        &mut self,
        account_address: AlloyAddress, // Consistent with HashMap key
        account_info: AlloyAccountInfo, // Takes AlloyAccountInfo
        insertion_type: InsertionType,
    ) {
        let mut new_account = BlockStateDBAccount::new(insertion_type);
        new_account.info = account_info;
        // new_account.state remains default (RevmAccountState::NoneValue or Default)
        self.accounts.insert(account_address, new_account);
    }

    pub fn insert_account_storage(
        &mut self,
        account_address: AlloyAddress, // Consistent with HashMap key
        slot: U256,
        value: U256,
        insertion_type: InsertionType,
    ) -> Result<()> {
        if let Some(account) = self.accounts.get_mut(&account_address) {
            let slot_value = BlockStateDBSlot {
                value,
                insertion_type: InsertionType::Custom,
            };
            account.storage.insert(slot, slot_value);
            return Ok(());
        }

        // The account does not exist. Fetch account information from provider and insert account.
        // basic method expects RevmAddress and returns RevmAccountInfo.
        match self.basic(account_address.into_revm()) {
            Ok(Some(revm_account_info)) => {
                self.insert_account_info(account_address, revm_account_info.into_alloy(), insertion_type); // Convert to AlloyAccountInfo

                let node_db_account = self.accounts.get_mut(&account_address).unwrap();
                let slot_value = BlockStateDBSlot {
                    value,
                    insertion_type: InsertionType::Custom,
                };
                node_db_account.storage.insert(slot, slot_value);
            }
            Ok(None) => {
                // Create a default account if not found on chain
                let default_alloy_info = AlloyAccountInfo::default();
                self.insert_account_info(account_address, default_alloy_info, insertion_type);
                let node_db_account = self.accounts.get_mut(&account_address).unwrap();
                 let slot_value = BlockStateDBSlot {
                    value,
                    insertion_type: InsertionType::Custom,
                };
                node_db_account.storage.insert(slot, slot_value);
            }
            Err(e) => {
                warn!("Failed to fetch account info for {} during insert_account_storage: {:?}", account_address, e);
                return Err(anyhow::anyhow!("Failed to fetch account info: {:?}", e));
            }
        }
        Ok(())
    }
}

impl<T: Transport + Clone, N: Network, P: Provider<N>> Database for BlockStateDB<T, N, P> {
    type Error = TransportError; // revm Database trait requires this error type

    fn basic(&mut self, address: RevmAddress) -> Result<Option<RevmAccountInfo>, Self::Error> {
        trace!("Database Basic: Looking for account {}", address);
        let alloy_address = address.into_alloy(); // Convert to AlloyAddress for HashMap lookup

        if let Some(account) = self.accounts.get(&alloy_address) {
            trace!("Database Basic: Account {} found in database cache", alloy_address);
            // Convert cached AlloyAccountInfo back to RevmAccountInfo
            return Ok(Some(account.info.clone().into_revm()));
        }

        trace!("Database Basic: Account {} not found in cache. Fetching via basic_ref", address);
        match <Self as DatabaseRef>::basic_ref(self, address) {
            Ok(Some(revm_account_info)) => {
                // Cache as AlloyAccountInfo
                self.insert_account_info(alloy_address, revm_account_info.clone().into_alloy(), InsertionType::OnChain);
                Ok(Some(revm_account_info))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn code_by_hash(&mut self, code_hash: RevmB256) -> Result<Bytecode, Self::Error> {
        trace!("Database Code By Hash: Fetching code for hash {}", code_hash);
        if let Some(code) = self.contracts.get(&code_hash) {
            trace!("Database Code By Hash: Code for hash {} found in database", code_hash);
            return Ok(code.clone());
        }

        trace!("Database Code By Hash: Code for hash {} not found in cache. Attempting fetch (should be preloaded or error).", code_hash);
        // revm expects this to be preloaded if it's not an EOA.
        // For now, we rely on the panic in code_by_hash_ref or return empty.
        let bytecode = <Self as DatabaseRef>::code_by_hash_ref(self, code_hash)?;
        self.contracts.insert(code_hash, bytecode.clone());
        Ok(bytecode)
    }

    fn storage(&mut self, address: RevmAddress, index: U256) -> Result<U256, Self::Error> {
        trace!("Database Storage: Fetching storage for address {}, slot {}", address, index);
        let alloy_address = address.into_alloy();

        if let Some(account) = self.accounts.get(&alloy_address) {
            if let Some(slot_value) = account.storage.get(&index) {
                trace!("Database Storage: Storage for address {}, slot {} found in database", alloy_address, index);
                return Ok(slot_value.value);
            }
        }

        trace!("Database Storage: Account {} found or slot {} missing. Fetching slot via storage_ref", alloy_address, index);
        let value = <Self as DatabaseRef>::storage_ref(self, address, index)?;
        
        // Ensure account exists in self.accounts before modifying storage
        if !self.accounts.contains_key(&alloy_address) {
            // If account doesn't exist, fetch its basic info and insert it
            let revm_account_info_opt = Self::basic(self, address)?; // This will populate cache if successful
            if revm_account_info_opt.is_none() {
                 // Still no account, create a default one to insert the slot
                let default_alloy_info = AlloyAccountInfo::default();
                self.insert_account_info(alloy_address, default_alloy_info, InsertionType::OnChain);
            }
        }

        let account = self.accounts.entry(alloy_address).or_insert_with(|| BlockStateDBAccount::new(InsertionType::OnChain));
        account.storage.insert(
            index,
            BlockStateDBSlot {
                value,
                insertion_type: InsertionType::OnChain,
            },
        );
        Ok(value)
    }

    fn block_hash(&mut self, number: BlockNumber) -> Result<RevmB256, Self::Error> {
        debug!("Fetching block hash for block number: {:?}", number);
        // block_hashes stores AlloyB256, convert to RevmB256 for return
        if let Some(alloy_hash) = self.block_hashes.get(&number) {
            debug!("Block hash found in database for block number: {:?}, hash: {:?}", number, alloy_hash);
            return Ok(alloy_hash.into_revm());
        }

        debug!("Block hash not found in cache, fetching from provider for block number: {:?}", number);
        let revm_hash = <Self as DatabaseRef>::block_hash_ref(self, number)?;
        self.block_hashes.insert(number, revm_hash.into_alloy()); // Store as AlloyB256
        Ok(revm_hash)
    }
}

impl<T: Transport + Clone, N: Network, P: Provider<N>> DatabaseRef for BlockStateDB<T, N, P> {
    type Error = TransportError;

    fn basic_ref(&self, address: RevmAddress) -> Result<Option<RevmAccountInfo>, Self::Error> {
        trace!("Database Basic Ref: Looking for account {}", address);
        // This method fetches directly from the provider.
        // It should return RevmAccountInfo as per revm's DatabaseRef trait.
        let alloy_address = address.into_alloy(); // For logging consistency with cache key

        trace!("Database BasicRef: Account {} not in cache for direct ref. Fetching info from provider", alloy_address);
        let f = async {
            let nonce_req = self.provider.get_transaction_count(alloy_address).block_id(BlockId::latest());
            let balance_req = self.provider.get_balance(alloy_address).block_id(BlockId::latest());
            let code_req = self.provider.get_code_at(alloy_address).block_id(BlockId::latest());
            tokio::join!(nonce_req, balance_req, code_req)
        };
        let (nonce_res, balance_res, code_res) = self.runtime.block_on(f);

        match (nonce_res, balance_res, code_res) {
            (Ok(nonce_val), Ok(balance_val), Ok(code_bytes)) => {
                trace!("Database BasicRef: Fetched account {} from provider", alloy_address);
                
                let revm_bytecode = Bytecode::new_raw(code_bytes.0.into()); // code_bytes is alloy::Bytes
                let code_hash = revm_bytecode.hash_slow(); // This is RevmB256

                Ok(Some(RevmAccountInfo::new(
                    balance_val, // U256 from alloy
                    nonce_val,   // u64 from alloy
                    code_hash,   // RevmB256
                    revm_bytecode,
                )))
            }
            (Err(e), _, _) => { trace!("Nonce error: {:?}", e); Err(TransportError::custom(e)) }
            (_, Err(e), _) => { trace!("Balance error: {:?}", e); Err(TransportError::custom(e)) }
            (_, _, Err(e)) => { trace!("Code error: {:?}", e); Err(TransportError::custom(e)) }
        }
    }

    fn code_by_hash_ref(&self, code_hash: RevmB256) -> Result<Bytecode, Self::Error> {
        trace!("Database Code By Hash Ref: Fetching code for hash {}", code_hash);
        if let Some(code) = self.contracts.get(&code_hash) {
            trace!("Database Code By Hash Ref: Code for hash {} found in cache", code_hash);
            return Ok(code.clone());
        }
        // As per revm, code should be loaded by `basic_ref` or pre-inserted.
        // If it's not found, it's typically an issue or an EOA.
        // Returning empty bytecode or an error might be options.
        // For now, return empty to avoid panic, but this might hide issues.
        warn!("Database Code By Hash Ref: Code for hash {} not found. This might be an EOA or an error.", code_hash);
        Ok(Bytecode::new()) // Or return an error: Err(TransportError::Custom(eyre!("Code not found")))
    }

    fn storage_ref(&self, address: RevmAddress, index: U256) -> Result<U256, Self::Error> {
        trace!("Database Storage Ref: Fetching storage for address {}, slot {}", address, index);
        let alloy_address = address.into_alloy();

        // Check local cache first (though DatabaseRef is typically for provider calls)
        if let Some(account) = self.accounts.get(&alloy_address) {
            if let Some(value) = account.storage.get(&index) {
                trace!("Database Storage Ref: Storage for address {}, slot {} found in *local cache*", alloy_address, index);
                return Ok(value.value);
            }
        }
        
        trace!("Database Storage Ref: Slot not in local cache for address {}. Fetching slot {} from provider", alloy_address, index);
        let f = self.provider.get_storage_at(alloy_address, index.into_alloy()).block_id(BlockId::latest()); // index might need conversion if its type differs
        
        match self.runtime.block_on(f.into_future()) {
            Ok(storage_value_b256) => { // storage_value is B256 from provider
                let u256_value = U256::from_be_bytes(storage_value_b256.0);
                trace!("Database Storage Ref: Fetched slot {} with value {} for account {} from provider", index, u256_value, alloy_address);
                Ok(u256_value)
            }
            Err(e) => {
                warn!("Database Storage Ref: Error fetching slot {} for {}: {:?}", index, alloy_address, e);
                Err(TransportError::custom(e))
            }
        }
    }

    fn block_hash_ref(&self, number: BlockNumber) -> Result<RevmB256, Self::Error> {
        debug!("Fetching block_hash_ref for block number: {:?}", number);
        // Check local cache first
        if let Some(alloy_hash) = self.block_hashes.get(&number) {
            debug!("Block hash found in *local cache* for block number: {:?}, hash: {:?}", number, alloy_hash);
            return Ok(alloy_hash.into_revm());
        }

        debug!("Block hash not found in local cache, fetching from provider for block number: {:?}", number);
        
        // Corrected: get_block_by_number takes one arg for number, and returns a request object.
        let block_request = self.provider.get_block_by_number(number.into(), false); // Second arg `false` for not full transactions
        
        match self.runtime.block_on(block_request.into_future()) {
            Ok(Some(block_data)) => { // block_data is N::BlockResponse (e.g., alloy_rpc_types::Block)
                let header = block_data.header(); // Assuming BlockResponse has header()
                if let Some(hash_bytes) = header.hash { // Assuming header has hash: Option<AlloyB256>
                    let revm_hash = RevmB256::from_slice(hash_bytes.as_slice());
                    debug!("Fetched block hash from provider for block number: {:?}, hash: {:?}" ,number, revm_hash);
                    Ok(revm_hash)
                } else {
                    warn!("Block {} header has no hash.", number);
                    Ok(RevmB256::ZERO) // Or return error
                }
            }
            Ok(None) => {
                warn!("No block found for block number: {:?}", number);
                Ok(RevmB256::ZERO) // Or return error
            }
            Err(e) => {
                 warn!("Error fetching block for number {}: {:?}", number, e);
                 Err(TransportError::custom(e))
            }
        }
    }
}

impl<T: Transport + Clone, N: Network, P: Provider<N>> DatabaseCommit for BlockStateDB<T, N, P> {
    fn commit(&mut self, changes: HashMap<RevmAddress, RevmAccount>) { // revm types
        for (revm_address, mut revm_account) in changes {
            let alloy_address = revm_address.into_alloy();

            if !revm_account.is_touched() && !revm_account.is_created() { // More robust check
                continue;
            }

            if revm_account.is_selfdestructed() {
                let db_account = self.accounts.entry(alloy_address).or_default();
                db_account.storage.clear();
                db_account.state = RevmAccountState::SelfDestructed; // Use revm's state
                db_account.info = AlloyAccountInfo::default(); // Clear to default alloy info
                continue;
            }
            
            let is_newly_created = revm_account.is_created();

            // Handle bytecode
            if let Some(code) = revm_account.info.code.take() { // Take ownership of bytecode
                if !code.is_empty() {
                    // revm_account.info.code_hash is already set by revm if code is present
                    self.contracts.insert(revm_account.info.code_hash, code);
                }
            }
            
            let db_account = self.accounts.entry(alloy_address).or_insert_with(|| BlockStateDBAccount::new(InsertionType::Custom));
            
            // Update account info (RevmAccountInfo -> AlloyAccountInfo)
            db_account.info = revm_account.info.into_alloy(); // Conversion needed

            // Update state
            db_account.state = if is_newly_created {
                db_account.storage.clear();
                RevmAccountState::StorageCleared // Or Touched/NoneValue depending on exact revm logic post-creation
            } else if revm_account.state.is_storage_cleared() { // Use method on revm_account.state
                RevmAccountState::StorageCleared
            } else if revm_account.state.is_touched() { // Check if touched
                 RevmAccountState::Touched
            } else {
                db_account.state // Preserve old state if not otherwise determined
            };
            
            // Update storage
            for (key, revm_storage_slot) in revm_account.storage.into_iter() {
                // revm_storage_slot.present_value() gives U256
                db_account.storage.insert(
                    key, // U256
                    BlockStateDBSlot {
                        value: revm_storage_slot.present_value(),
                        insertion_type: InsertionType::Custom,
                    },
                );
            }
        }
    }
}

#[derive(Default, Eq, PartialEq, Copy, Clone, Debug)]
pub enum InsertionType {
    Custom,
    #[default]
    OnChain,
}

#[derive(Default, Eq, PartialEq, Copy, Clone, Debug)]
pub struct BlockStateDBSlot {
    pub value: U256,
    pub insertion_type: InsertionType,
}

#[derive(Debug, Clone)] // Removed Default derive as state needs specific init
pub struct BlockStateDBAccount {
    pub info: AlloyAccountInfo, // Storing as Alloy's version
    pub state: RevmAccountState, // Storing revm's state enum
    pub storage: HashMap<U256, BlockStateDBSlot>,
    // #[warn(dead_code)] // no longer needed if used
    pub insertion_type: InsertionType,
}

impl BlockStateDBAccount {
    pub fn new(insertion_type: InsertionType) -> Self {
        Self {
            info: AlloyAccountInfo::default(),
            state: RevmAccountState::default(), // Default for RevmAccountState (usually NoneValue/0)
            storage: HashMap::new(),
            insertion_type,
        }
    }
}

// Default impl for BlockStateDBAccount if manual new is too verbose everywhere
impl Default for BlockStateDBAccount {
    fn default() -> Self {
        Self {
            info: AlloyAccountInfo::default(),
            state: RevmAccountState::default(),
            storage: HashMap::new(),
            insertion_type: InsertionType::OnChain, // Or Custom as a sensible default
        }
    }
}