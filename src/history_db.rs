use alloy::primitives::StorageKey;
use eyre::Result;
use reth::api::NodeTypesWithDBAdapter;
use reth::providers::providers::StaticFileProvider;
use reth::providers::AccountReader;
use alloy::primitives::U256;
use reth::providers::StateProviderBox;
use alloy::primitives::Address as AlloyAddress;
use reth::providers::{BlockNumReader, ProviderFactory};
use reth::utils::open_db_read_only;
use reth_chainspec::ChainSpecBuilder;
use reth_db::{mdbx::DatabaseArguments, ClientVersion, DatabaseEnv};
use reth_node_ethereum::EthereumNode;
use revm::primitives::KECCAK_EMPTY;
use revm::{
    primitives::{keccak256, Address, B256 as RevmB256},
    state::{AccountInfo, Bytecode},
    Database,
    DatabaseCommit, DatabaseRef,
};

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

// Main structure for the Node Database
pub struct HistoryDB {
    db_provider: StateProviderBox,
    provider_factory: ProviderFactory<NodeTypesWithDBAdapter<EthereumNode, Arc<DatabaseEnv>>>,
}

impl HistoryDB {
    // Constructor for NodeDB
    pub fn new(db_path: String, block: u64) -> Result<Self> {
        // Open the database in read-only mode
        let db_path = Path::new(&db_path);
        let db = Arc::new(open_db_read_only(
            db_path.join("db").as_path(),
            DatabaseArguments::new(ClientVersion::default()),
        )?);

        // Create a ProviderFactory
        let spec = Arc::new(ChainSpecBuilder::mainnet().build());
        let factory =
            ProviderFactory::<NodeTypesWithDBAdapter<EthereumNode, Arc<DatabaseEnv>>>::new(
                db.clone(),
                spec.clone(),
                StaticFileProvider::read_only(db_path.join("static_files"), true)?,
            );

        let provider = factory
            .history_by_block_number(block)
            .expect("Unable to create provider");

        Ok(Self {
            db_provider: provider,
            provider_factory: factory,
        })
    }
}

impl Database for HistoryDB {
    type Error = eyre::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        Self::basic_ref(self, address)
    }

    fn code_by_hash(&mut self, _code_hash: RevmB256) -> Result<Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        Self::storage_ref(self, address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<RevmB256, Self::Error> {
        Self::block_hash_ref(self, number)
    }
}

impl DatabaseRef for HistoryDB {
    type Error = eyre::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Convert revm Address to alloy Address for provider
        let alloy_address = AlloyAddress::from_slice(address.as_slice());
        
        let account = self
            .db_provider
            .basic_account(&alloy_address)
            .unwrap_or_default()
            .unwrap_or_default();
        let code = self.db_provider.account_code(&alloy_address).unwrap_or_default();
        let account_info = if let Some(code) = code {
            AccountInfo::new(
                account.balance,
                account.nonce,
                code.hash_slow(),
                Bytecode::new_raw(code.original_bytes()),
            )
        } else {
            AccountInfo::new(
                account.balance,
                account.nonce,
                KECCAK_EMPTY,
                Bytecode::new(),
            )
        };
        Ok(Some(account_info))
    }

    fn code_by_hash_ref(&self, _code_hash: RevmB256) -> Result<Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Convert revm Address to alloy Address for provider
        let alloy_address = AlloyAddress::from_slice(address.as_slice());
        let value = self.db_provider.storage(alloy_address, StorageKey::from(index))?;

        Ok(value.unwrap_or_default())
    }

    fn block_hash_ref(&self, number: u64) -> Result<RevmB256, Self::Error> {
        let blockhash = self.db_provider.block_hash(number).unwrap_or_default();

        if let Some(hash) = blockhash {
            // Convert alloy B256 to revm B256
            Ok(RevmB256::from_slice(hash.as_slice()))
        } else {
            Ok(KECCAK_EMPTY)
        }
    }
}
