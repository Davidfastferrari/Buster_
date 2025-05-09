use crate::errors::PoolSyncError;
use crate::{Chain, Pool, PoolInfo, PoolType, Syncer};
use alloy_network::Ethereum;
use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_types::{Filter, Log};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use pool_builder::build_pools;
use pool_fetchers::PoolFetcher;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use tracing::{debug, error};

pub mod pool_builder;
pub mod pool_fetchers;

// Batch steps size for address syncing
const ADDRESS_BATCH_SIZE: u64 = 10_000;
const INFO_BATCH_SIZE: u64 = 50;
const RETRY_LIMIT: usize = 10;

// Sync pools via rpc
pub(crate) struct RpcSyncer {
    provider: Arc<RootProvider>,
    chain: Chain,
}

#[async_trait]
impl Syncer for RpcSyncer {
    // Query factory even logs to extract all pool adddresses for the pool type
    async fn fetch_addresses(
        &self,
        start_block: u64,
        end_block: u64,
        pool_fetcher: Arc<dyn PoolFetcher>,
    ) -> Result<Vec<Address>, PoolSyncError> {
        let filter = Filter::new()
            .address(pool_fetcher.factory_address(self.chain))
            .event(pool_fetcher.pair_created_signature());

        // Chunk up block range into fetching futures and join them all
        let tasks = self.build_fetch_tasks(start_block, end_block, filter);

        // Buffer the futures to not overwhelm the provider
        let logs: Vec<_> = stream::iter(tasks).buffer_unordered(100).collect().await;
        let logs: Vec<Log> = logs
            .into_iter()
            .filter_map(|result| match result {
                Ok(logs) => Some(logs),
                Err(e) => {
                    error!("Fetching failed: {}", e);
                    None
                }
            })
            .flatten()
            .collect();

        // Parse the logs into their pool address via the fetcher
        Ok(logs
            .iter()
            .map(|log| pool_fetcher.log_to_address(&log.inner))
            .collect())
    }

    async fn populate_pool_info(
        &self,
        addresses: Vec<Address>,
        pool_type: &PoolType,
        block_num: u64,
    ) -> Result<HashMap<Address, Pool>, PoolSyncError> {
        // Chunk up addresses into info fetching futures
        let futures: Vec<_> = addresses
            .chunks(INFO_BATCH_SIZE as usize)
            .map(|addr_chunk| async move {
                build_pools(addr_chunk, pool_type, self.provider.clone(), block_num).await
            })
            .collect();

        let results: Vec<_> = stream::iter(futures).buffer_unordered(100).collect().await;
        Ok(results
            .into_iter()
            .filter_map(|result| match result {
                Ok(pools) => Some(pools),
                Err(e) => {
                    error!("building failed: {}", e);
                    None
                }
            })
            .flatten()
            .map(|pool| (pool.address(), pool))
            .collect())
    }

    async fn populate_liquidity(
        &self,
        pools: &mut HashMap<Address, Pool>,
        pool_type: &PoolType,
        start_block: u64,
        end_block: u64,
        is_initial_sync: bool,
    ) -> Result<Vec<Address>, PoolSyncError> {
        // Construct proper liquidity filter based on pool type
        /*
        let filter: Filter = if pool_type.is_v2() {
            todo!()
        } else if pool_type.is_v3() {
            todo!()
        } else {
            todo!()
        };
        */
        let filter = Filter::new();

        // Chunk up block range into fetching futures and join them all
        let tasks = self.build_fetch_tasks(start_block, end_block, filter);

        // Buffer the futures to not overwhelm the provider
        let logs: Vec<_> = stream::iter(tasks).buffer_unordered(100).collect().await;
        let logs: Vec<Log> = logs
            .into_iter()
            .filter_map(|result| match result {
                Ok(logs) => Some(logs),
                Err(e) => {
                    error!("Fetching failed: {}", e);
                    None
                }
            })
            .flatten()
            .collect();

        let mut ordered_logs: BTreeMap<u64, Vec<Log>> = BTreeMap::new();
        for log in logs {
            if let Some(block_number) = log.block_number {
                ordered_logs.entry(block_number).or_default().push(log);
            }
        }

        // Process all of the logs
        let mut touched_pools = Vec::new();
        for (_, log_group) in ordered_logs {
            for log in log_group {
                let address = log.address();
                touched_pools.push(address);
                if let Some(pool) = pools.get_mut(&address) {
                    if pool_type.is_v3() {
                        let pool = pool.get_v3_mut().unwrap();
                        pool.process_tick_data(log, pool_type, is_initial_sync);
                    } else if pool_type.is_balancer() {
                        //process_balance_data(pool.get_balancer_mut().unwrap(), log);
                    } else {
                        let pool = pool.get_v2_mut().unwrap();
                        pool.process_sync_data(log, pool_type);
                    }
                }
            }
        }
        Ok(touched_pools)
    }

    async fn block_number(&self) -> Result<u64, PoolSyncError> {
        self.provider
            .get_block_number()
            .await
            .map_err(|_| PoolSyncError::ProviderError("failed to get block".to_string()))
    }
}

impl RpcSyncer {
    // Construct a new Rpc Syncer to sync pools via RPC
    pub fn new(chain: Chain) -> Result<Self, PoolSyncError> {
        let endpoint = std::env::var("ARCHIVE").map_err(|_e| PoolSyncError::EndpointNotSet)?;

        let provider = Arc::new(
            ProviderBuilder::<_, _, Ethereum>::default().on_http(
                endpoint
                    .parse()
                    .map_err(|_e| PoolSyncError::ParseEndpointError)?,
            ),
        );
        Ok(Self { provider, chain })
    }

    // Fetch logs from start_block..end_block for the provided filter
    fn fetch_logs(
        &self,
        start_block: u64,
        end_block: u64,
        filter: Filter,
    ) -> impl Future<Output = Result<Vec<Log>, PoolSyncError>> {
        let filter = filter.from_block(start_block).to_block(end_block);
        let client = self.provider.clone();
        async move {
            let mut fetch_cnt = 0;
            loop {
                // Fetch the logs w/ a backoff retry
                match client.get_logs(&filter).await {
                    Ok(logs) => {
                        debug!("Fetched logs from block {} to {}", start_block, end_block);
                        return Ok(logs);
                    }
                    Err(_) => {
                        fetch_cnt += 1;
                        if fetch_cnt == RETRY_LIMIT {
                            return Err(PoolSyncError::ProviderError(
                                "Reached rety limit".to_string(),
                            ));
                        }

                        // Jitter for some retry sleep duration
                        let jitter = fastrand::u64(0..=1000);
                        tokio::time::sleep(std::time::Duration::from_millis(jitter)).await
                    }
                }
            }
        }
    }

    // Build a set of log fetching futures for the filter
    fn build_fetch_tasks(
        &self,
        start_block: u64,
        end_block: u64,
        filter: Filter,
    ) -> Vec<impl Future<Output = Result<Vec<Log>, PoolSyncError>>> {
        (start_block..=end_block)
            .step_by(ADDRESS_BATCH_SIZE as usize)
            // Map each starting block to a task
            .map(|start| {
                let end = std::cmp::min(start + ADDRESS_BATCH_SIZE - 1, end_block);
                self.fetch_logs(start, end, filter.clone())
            })
            .collect::<Vec<_>>()
    }
}
