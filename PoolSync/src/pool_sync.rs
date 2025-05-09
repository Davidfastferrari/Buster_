//! PoolSync Core Implementation
//!
//! This module contains the core functionality for synchronizing pools across different
//! blockchain networks and protocols. It includes the main `PoolSync` struct and its
//! associated methods for configuring and executing the synchronization process.

use crate::errors::PoolSyncError;
use crate::pool_database::PoolDatabase;
use crate::PoolInfo;
use alloy_primitives::Address;
use std::collections::HashMap;
use std::sync::Arc;

use crate::builder::PoolSyncBuilder;
use crate::sync_rpc::pool_fetchers::PoolFetcher;
use crate::Pool;
use crate::{Chain, PoolType, Syncer};
use tracing::info;

/// The main struct for pool synchronization
pub struct PoolSync {
    /// Map of pool types to their fetcher implementations
    fetchers: HashMap<PoolType, Arc<dyn PoolFetcher>>,
    /// Underlying syncer
    syncer: Box<dyn Syncer>,
    /// Database connection for persisting data
    database: Arc<PoolDatabase>,
    /// The cahin to sync on
    chain: Chain,
}

impl PoolSync {
    /// Construct a new instance of PoolSync
    pub(crate) fn new(
        fetchers: HashMap<PoolType, Arc<dyn PoolFetcher>>,
        syncer: Box<dyn Syncer>,
        database: Arc<PoolDatabase>,
        chain: Chain,
    ) -> Self {
        Self {
            fetchers,
            syncer,
            database,
            chain,
        }
    }

    /// Construct a new builder to configure sync parameters
    pub fn builder() -> PoolSyncBuilder {
        PoolSyncBuilder::default()
    }

    /// Sync all of the pools from the chain using a round-robin approach
    pub async fn sync_pools(&self) -> Result<(Vec<Pool>, u64), PoolSyncError> {
        // Load in all of the pools that have already been synced for the registered pool types and
        // chain from the database
        let mut loaded_pools = self.load_existing_pools(self.chain)?;

        // Alongside loading in all of the pools, also load in the block that they have been synced
        // up to
        let mut last_processed_blocks = HashMap::new();
        for pool_type in self.fetchers.keys() {
            let last_block = self
                .database
                .get_last_processed_block(self.chain, *pool_type)?
                .unwrap_or_default();
            last_processed_blocks.insert(*pool_type, last_block);
        }

        // Round-robin sync until all pools are up to date
        let mut current_block = self.syncer.block_number().await?;
        let mut has_more_to_sync = true;
        while has_more_to_sync {
            has_more_to_sync = false;
            current_block = self.syncer.block_number().await?;

            // Sync all of the pool types up to the currenet block
            for (pool_type, fetcher) in &self.fetchers {
                let last_processed_block = last_processed_blocks.get_mut(pool_type).unwrap();

                // If we have blocks to sync
                if *last_processed_block < current_block {
                    has_more_to_sync = true;

                    // Get existing pools for this type
                    let existing_pools = loaded_pools.get_mut(pool_type).unwrap();

                    // Sync this pool type
                    self.sync_pool_type(
                        *pool_type,
                        fetcher,
                        *last_processed_block,
                        current_block,
                        existing_pools,
                        self.chain,
                    )
                    .await?;

                    // Update last processed block
                    *last_processed_block = current_block;
                }
            }
        }

        // Collect all pools into a single vector
        let all_pools: Vec<Pool> = loaded_pools
            .into_iter()
            .flat_map(|(_, inner_map)| inner_map.into_values())
            .collect();
        info!("Synced {} pools", all_pools.len());

        Ok((all_pools, current_block))
    }

    /// Sync a single pool type from its last processed block to the current block
    async fn sync_pool_type(
        &self,
        pool_type: PoolType,
        fetcher: &Arc<dyn PoolFetcher>,
        last_block: u64,
        current_block: u64,
        existing_pools: &mut HashMap<Address, Pool>,
        chain: Chain,
    ) -> Result<(), PoolSyncError> {
        let sync_start = last_block + 1;
        let sync_end = current_block;

        info!(
            "Syncing {} from block {} to {}",
            pool_type, sync_start, sync_end
        );

        // Fetch new pool addresses
        let new_addresses = self
            .syncer
            .fetch_addresses(sync_start, sync_end, fetcher.clone())
            .await?;

        info!(
            "Fetched {} new addresses for {}",
            new_addresses.len(),
            pool_type
        );

        // If we have new addresses that we have not seen before, build the pools,populate
        // their liquidity from genesis..last_processed_block
        if !new_addresses.is_empty() {
            // Build and populate new pools
            let mut new_pools = self
                .syncer
                .populate_pool_info(new_addresses, &pool_type, sync_end)
                .await?;

            // Populate liquidity for new pools from genesis
            // Do not care about return here since all of these pools are new and must be saved
            // into the database.
            let _ = self
                .syncer
                .populate_liquidity(&mut new_pools, &pool_type, 0, last_block - 1, true)
                .await?;

            // Save all of these pools into the database and add them to our working set
            let new_pools_vec: Vec<Pool> = new_pools.values().cloned().collect();
            self.database.save_pools(&new_pools_vec, chain)?;
            existing_pools.extend(new_pools);
        }

        // We now have a set of pools that have liquditiy upto last_processed_block. Update
        // liquidity for all of the new blocks and save the state to database
        let pools_to_save = self
            .syncer
            .populate_liquidity(existing_pools, &pool_type, last_block, current_block, false)
            .await?;
        let pools_to_save: Vec<Pool> = pools_to_save
            .iter()
            .map(|addr| existing_pools.get(addr).expect("Pool exist").clone())
            .collect();
        info!("Fully processed {} pools", existing_pools.len());

        // Update the database state
        self.database.save_pools(&pools_to_save, chain)?;
        self.database
            .update_last_processed_block(chain, pool_type, sync_end)?;

        Ok(())
    }

    /// Load existing pools from the database for all pool types and the chain
    fn load_existing_pools(
        &self,
        chain: Chain,
    ) -> Result<HashMap<PoolType, HashMap<Address, Pool>>, PoolSyncError> {
        let mut loaded_pools = HashMap::new();

        for pool_type in self.fetchers.keys() {
            let pools = self.database.load_pools(chain, Some(&[*pool_type]))?;
            info!("Loaded {} existing pools for {}", pools.len(), pool_type);
            let pool_map = pools
                .into_iter()
                .map(|pool| (pool.address(), pool))
                .collect();

            loaded_pools.insert(*pool_type, pool_map);
        }

        Ok(loaded_pools)
    }
}
