
use crate::events::Event;
use crate::gas_station::GasStation;
use crate::gen_::FlashSwap;
use crate::traits::*;
use crate::types::*;
use alloy::hex;
use alloy::signers::k256::SecretKey;
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::http::Client;
// use alloy::transports::http::Http; // Not directly used, RootProvider u
ses it.
use alloy::eips::Encodable2718;
use alloy::network::{EthereumWallet, Ethereum, Network, TransactionBuilder};
use alloy::primitives::{Address, FixedBytes, Bytes};
use alloy::providers::Provider;
use alloy::providers::ProviderBuilder;
use alloy::providers::RootProvider; // Already imported
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::SolCall;
use log::info;
//use reqwest::Client; // alloy's Client is used
use serde_json::Value;
use std::str::FromStr;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Handles sending transactions
pub struct TransactionSender {
    wallet: EthereumWallet,
    gas_station: Arc<GasStation>,
    contract_address: Address,
    client: Arc<Client>, // This is alloy::transports::http::Client
    provider: Arc<RootProvider<Http<Client>, Ethereum>>, // Corrected RootProvider type
    nonce: u64,
}

impl TransactionSender {
    pub async fn new(gas_station: Arc<GasStation>) -> Self {
        // construct a wallet
        let key = std::env::var("PRIVATE_KEY").unwrap();
        let key_hex = hex::decode(key).unwrap();
        let key_secret = SecretKey::from_bytes((&key_hex[..]).into()).unwrap();
        let signer = PrivateKeySigner::from(key_secret);
        let wallet = EthereumWallet::from(signer);

        // Create persisent http client for custom requests
        let http_client = Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(None)
            .tcp_keepalive(Duration::from_secs(10))
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client");
        
        // Warm up connection by sending a simple eth_blockNumber request
        let warmup_json = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });
        let _ = http_client // Use the created http_client
            .post("https://mainnet-sequencer.base.org")
            .json(&warmup_json)
            .send()
            .await
            .unwrap();

        // construct a provider for tx receipts and nonce
        let provider_url_str = std::env::var("FULL").unwrap();
        let provider_url = provider_url_str.parse().expect("Failed to parse provider URL");

        let provider = Arc::new(
            ProviderBuilder::new() // Removed .with_recommended_fillers()
                // The actual transport (Http<Client>) will be part of the RootProvider's type
                .on_http(provider_url),
        );

        let nonce = provider
            .get_transaction_count(std::env::var("ACCOUNT").unwrap().parse().unwrap(), None) // Added None for block_id
            .await
            .unwrap();

        Self {
            wallet,
            gas_station,
            contract_address: std::env::var("SWAP_CONTRACT").unwrap().parse().unwrap(),
            client: Arc::new(http_client), // Store the http_client
            provider,
            nonce,
        }
    }

    // Receive a path that has passed simulation to be sent to the sequencer
    pub async fn send_transactions(&mut self, tx_receiver: Receiver<Event>) {
        // wait for a new transaction that has passed simulation
        while let Ok(Event::ValidPath((arb_path, profit, block_number))) = tx_receiver.recv() {
            info!("Sending path...");

            // Setup the calldata
            let converted_path: FlashSwap::SwapParams = arb_path.clone().into();
            let calldata = FlashSwap::executeArbitrageCall {
                arb: converted_path,
            }
            .abi_encode();

            // Construct, sign, and encode transaction
            let (max_fee, priority_fee) = self.gas_station.get_gas_fees(profit);
            let tx = TransactionRequest::default()
                .with_to(self.contract_address)
                .with_nonce(self.nonce)
                .with_gas_limit(2_000_000)
                .with_chain_id(8453)
                .with_max_fee_per_gas(max_fee.into()) // Ensure U256
                .with_max_priority_fee_per_gas(priority_fee.into()) // Ensure U256
                .with_transaction_type(2) // EIP-1559
                .with_input(Bytes::from(calldata));
            self.nonce += 1;
            
            // Build the transaction envelope using the wallet
            // The build method might not be directly on TransactionRequest, but through a Provider or Wallet.
            // Assuming Network::build_transaction is the way or similar.
            // For Alloy, you often build it with a signer associated with the provider or a wallet.
            // Since self.provider is a RootProvider, it might not have direct signing if not configured with a wallet.
            // self.wallet is EthereumWallet.
            
            // Let's sign the transaction with the wallet
            let signature = self.wallet.sign_transaction(&tx.clone().into()).await.unwrap(); // tx might need to be into Network::TransactionRequest
            let tx_signed = tx.clone().into_signed(signature); // This creates a Signed<TransactionRequest> or similar EIP-2718 envelope
            
            let mut encoded_tx = Vec::new();
            tx_signed.encode_2718(&mut encoded_tx);
            let rlp_hex = hex::encode_prefixed(encoded_tx);

            let tx_data = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_sendRawTransaction",
                "params": [rlp_hex],
                "id": 1
            });

            // Send the transaciton off and monitor its status
            info!("Sending on block {}", block_number);
            let start = Instant::now();

            // construct the request and send it
            let req = self
                .client
                .post("https://mainnet-sequencer.base.org")
                .json(&tx_data)
                .send()
                .await
                .unwrap();
            let req_response: Value = req.json().await.unwrap();
            info!("Took {:?} to send tx and receive response", start.elapsed());
            
            if let Some(tx_hash_str) = req_response["result"].as_str() {
                let tx_hash = FixedBytes::<32>::from_str(tx_hash_str).unwrap();
                let provider_clone = self.provider.clone();
                tokio::spawn(async move {
                    Self::send_and_monitor(provider_clone, tx_hash, block_number).await;
                });
            } else {
                log::error!("eth_sendRawTransaction returned no result or error: {:?}", req_response);
            }
        }
    }

    // Send the transaction and monitor its status
    pub async fn send_and_monitor(
        provider: Arc<RootProvider<Http<Client>, Ethereum>>, // Corrected RootProvider type
        tx_hash: FixedBytes<32>,
        block_number: u64,
    ) {
        // loop while waiting for tx receipt
        let mut attempts = 0;
        while attempts < 10 {
            // try to fetch the receipt
            let receipt_result = provider.get_transaction_receipt(tx_hash).await;
            match receipt_result {
                Ok(Some(inner_receipt)) => {
                    info!(
                        "Send on block {:?}, Landed on block {:?}",
                        block_number,
                        inner_receipt.block_number.unwrap_or_default()
                    );
                    return;
                }
                Ok(None) => {
                    info!("Tx receipt not yet available for {:?}, attempt {}", tx_hash, attempts + 1);
                }
                Err(e) => {
                    log::error!("Error fetching tx receipt for {:?}: {:?}", tx_hash, e);
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
            attempts += 1;
        }
        log::warn!("Gave up waiting for tx receipt for {:?}", tx_hash);
    }
}

// Test transaction sending functionality
// #[cfg(test)]
// mod tx_signing_tests {
//     use super::*;
//     use crate::gen_::FlashQuoter;
//     use crate::AMOUNT;
//     use alloy::primitives::{address, U256};
//     use alloy::providers::{ProviderBuilder, RootProvider}; // Provider already imported via super::*
//     use alloy::transports::http::Http; // For provider type

//     // use env_logger; // Already imported via super::* if it's there, or directly here
//     // use pool_sync::PoolType; // Not used in this snippet
//     use std::time::Instant;

//     // Create mock swap params
//     fn dummy_swap_params() -> FlashQuoter::SwapParams {
//         let p1 = address!("4C36388bE6F416A29C8d8Eee81C771cE6bE14B18");
//         let p2 = address!("9A834b70C07C81a9FCB695573D9008d0eF23A998");
//         FlashQuoter::SwapParams {
//             pools: vec![p1, p2],
//             pool_versions: vec![0.into(), 0.into()], // Assuming poolVersions are U256 or similar
//             amount_in: *AMOUNT,
//         }
//     }

//     // Test the time it takes to create a transaction
//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_sign() {
//         // init and get all dummy state
//         dotenv::dotenv().ok();
//         let key_str = std::env::var("PRIVATE_KEY").unwrap();
//         let key_hex = hex::decode(key_str).unwrap();
//         let key_secret = SecretKey::from_bytes((&key_hex[..]).into()).unwrap();
//         let signer = PrivateKeySigner::from(key_secret);
//         let wallet = EthereumWallet::from(signer);
        
//         let url_str = std::env::var("FULL").unwrap();
//         let url_parsed = url_str.parse().expect("Failed to parse provider URL for test");

//         let wallet_provider: Arc<RootProvider<Http<Client>, Ethereum>> = Arc::new(
//             ProviderBuilder::new() // Removed .with_recommended_fillers()
//                 .wallet(wallet.clone()) // wallet may need to be cloneable or this takes ownership
//                 .on_http(url_parsed),
//         );
        
//         let contract_address_str = std::env::var("SWAP_CONTRACT").unwrap();
//         let contract_address_parsed = contract_address_str.parse().unwrap();
//         let contract = FlashSwap::new(contract_address_parsed, wallet_provider.clone());
//         let path: FlashSwap::SwapParams = dummy_swap_params().into();

//         // benchmark tx construction
//         let gas = wallet_provider.estimate_eip1559_fees(None).await.unwrap(); // Added None for block
//         let tx_time = Instant::now();
//         let max_fee = gas.max_fee_per_gas * U256::from(5); 
//         let priority_fee = gas.max_priority_fee_per_gas * U256::from(30); 

//         let _ = contract
//             .executeArbitrage(path)
//             .max_fee_per_gas(max_fee)
//             .max_priority_fee_per_gas(priority_fee)
//             .chain_id(8453)
//             .gas(U256::from(4_000_000)) // Ensure U256
//             .into_transaction_request(); // This creates a TransactionRequest, not sending it
//         println!("Tx construction took {:?}", tx_time.elapsed());
//     }

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_send_tx() {
//         // init environment
//         // env_logger::builder().filter_level(log::LevelFilter::Info).init(); // Call .init()
//         let _ = env_logger::builder().filter_level(log::LevelFilter::Info).try_init();
//         dotenv::dotenv().ok();

//         // Create gas station
//         let gas_station = Arc::new(GasStation::new());

//         // Create transaction sender
//         let mut tx_sender = TransactionSender::new(gas_station).await;

//         // Create a channel for sending events
//         let (tx, rx) = std::sync::mpsc::channel();

//         // Create and send a test event
//         let swap_path = dummy_swap_params();
//         let test_event = Event::ValidPath((
//             swap_path,
//             alloy::primitives::U256::from(10000000u128), 
//             100u64,                                  
//         ));

//         tx.send(test_event).unwrap();

//         // Send the transaction (this will only process one transaction and then exit if channel closes)
//         // For robust testing, might need a way to signal completion or use a timeout.
//         let send_fut = tx_sender.send_transactions(rx);
//         // Allow some time for the transaction to be processed in a real test scenario
//         // For this example, we'll just await it. If it hangs, the test will hang.
//         // Consider tokio::time::timeout for tests that might not complete.
//         tokio::time::timeout(Duration::from_secs(30), send_fut).await.expect("send_transactions timed out");

//     }

// use crate::events::Event;
// use crate::gas_station::GasStation;
// use crate::gen_::FlashSwap;
// use crate::traits::*;
// use crate::types::*;
// use alloy::hex;
// use alloy::signers::k256::SecretKey;
// use alloy::signers::local::PrivateKeySigner;
// use alloy::transports::http::Client;
// use alloy::transports::http::Http;
// use alloy::eips::Encodable2718;
// use alloy::network::{EthereumWallet, Ethereum, Network, TransactionBuilder};
// use alloy::primitives::{Address, FixedBytes, Bytes};
// use alloy::providers::Provider;
// use alloy::providers::ProviderBuilder;
// use alloy::providers::RootProvider;
// use alloy::rpc::types::TransactionRequest;
// use alloy::sol_types::SolCall;
// use log::info;
// //use reqwest::Client;
// use serde_json::Value;
// use std::str::FromStr;
// use std::sync::mpsc::Receiver;
// use std::sync::Arc;
// use std::time::{Duration, Instant};

// // Handles sending transactions
// pub struct TransactionSender {
//     wallet: EthereumWallet,
//     gas_station: Arc<GasStation>,
//     contract_address: Address,
//     client: Arc<Client>,
//     provider: Arc<RootProvider<Ethereum>>,
//     nonce: u64,
// }

// impl TransactionSender {
//     pub async fn new(gas_station: Arc<GasStation>) -> Self {
//         // construct a wallet
//         let key = std::env::var("PRIVATE_KEY").unwrap();
//         let key_hex = hex::decode(key).unwrap();
//         let key = SecretKey::from_bytes((&key_hex[..]).into()).unwrap();
//         let signer = PrivateKeySigner::from(key);
//         let wallet = EthereumWallet::from(signer);

//         // Create persisent http client
//         let client = Client::builder()
//             .pool_max_idle_per_host(10)
//             .pool_idle_timeout(None)
//             .tcp_keepalive(Duration::from_secs(10))
//             .tcp_nodelay(true)
//             .timeout(Duration::from_secs(10))
//             .connect_timeout(Duration::from_secs(5))
//             .build()
//             .expect("Failed to create HTTP client");
//         // Warm up connection by sending a simple eth_blockNumber request
//         let warmup_json = serde_json::json!({
//             "jsonrpc": "2.0",
//             "method": "eth_blockNumber",
//             "params": [],
//             "id": 1
//         });
//         let _ = client
//             .post("https://mainnet-sequencer.base.org")
//             .json(&warmup_json)
//             .send()
//             .await
//             .unwrap();

//         // construct a provider for tx receipts and nonce
//         let provider = Arc::new(
//             ProviderBuilder::new()
//                 .with_recommended_fillers()
//                 .on_http(std::env::var("FULL").unwrap().parse().unwrap()),
//         );
//         let nonce = provider
//             .get_transaction_count(std::env::var("ACCOUNT").unwrap().parse().unwrap())
//             .await
//             .unwrap();

//         Self {
//             wallet,
//             gas_station,
//             contract_address: std::env::var("SWAP_CONTRACT").unwrap().parse().unwrap(),
//             client: Arc::new(client),
//             provider,
//             nonce,
//         }
//     }

//     // Receive a path that has passed simulation to be sent to the sequencer
//     pub async fn send_transactions(&mut self, tx_receiver: Receiver<Event>) {
//         // wait for a new transaction that has passed simulation
//         while let Ok(Event::ValidPath((arb_path, profit, block_number))) = tx_receiver.recv() {
//             info!("Sending path...");

//             // Setup the calldata
//             let converted_path: FlashSwap::SwapParams = arb_path.clone().into();
//             let calldata = FlashSwap::executeArbitrageCall {
//                 arb: converted_path,
//             }
//             .abi_encode();

//             // Construct, sign, and encode transaction
//             let (max_fee, priority_fee) = self.gas_station.get_gas_fees(profit);
//             let tx = TransactionRequest::default()
//                 .with_to(self.contract_address)
//                 .with_nonce(self.nonce)
//                 .with_gas_limit(2_000_000)
//                 .with_chain_id(8453)
//                 .with_max_fee_per_gas(max_fee)
//                 .with_max_priority_fee_per_gas(priority_fee)
//                 .transaction_type(2)
//                 .with_input(Bytes::from(calldata));
//             self.nonce += 1;
//             let tx_envelope = tx.build(&self.wallet).await.unwrap();
//             let mut encoded_tx = vec![];
//             tx_envelope.encode_2718(&mut encoded_tx);
//             let rlp_hex = hex::encode_prefixed(encoded_tx);

//             let tx_data = serde_json::json!({
//                 "jsonrpc": "2.0",
//                 "method": "eth_sendRawTransaction",
//                 "params": [rlp_hex],
//                 "id": 1
//             });

//             // Send the transaciton off and monitor its status
//             info!("Sending on block {}", block_number);
//             let start = Instant::now();

//             // construct the request and send it
//             let req = self
//                 .client
//                 .post("https://mainnet-sequencer.base.org")
//                 .json(&tx_data)
//                 .send()
//                 .await
//                 .unwrap();
//             let req_response: Value = req.json().await.unwrap();
//             info!("Took {:?} to send tx and receive response", start.elapsed());
//             let tx_hash =
//                 FixedBytes::<32>::from_str(req_response["result"].as_str().unwrap()).unwrap();

//             let provider = self.provider.clone();
//             tokio::spawn(async move {
//                 Self::send_and_monitor(provider, tx_hash, block_number).await;
//             });
//         }
//     }

//     // Send the transaction and monitor its status
//     pub async fn send_and_monitor(
//         provider: Arc<RootProvider<Ethereum>>,
//         tx_hash: FixedBytes<32>,
//         block_number: u64,
//     ) {
//         // loop while waiting for tx receipt
//         let mut attempts = 0;
//         while attempts < 10 {
//             // try to fetch the receipt
//             let receipt = provider.get_transaction_receipt(tx_hash).await;
//             if let Ok(Some(inner)) = receipt {
//                 info!(
//                     "Send on block {:?}, Landed on block {:?}",
//                     block_number,
//                     inner.block_number.unwrap()
//                 );
//                 return;
//             }

//             tokio::time::sleep(Duration::from_secs(2)).await;
//             attempts += 1;
//         }
//     }
// }

// // Test transaction sending functionality
// #[cfg(test)]
// mod tx_signing_tests {
//     use super::*;
//     use crate::gen_::FlashQuoter;
//     use crate::AMOUNT;
//     use alloy::primitives::{address, U256};
//     use alloy::providers::{Provider, ProviderBuilder, RootProvider};

//     use env_logger;
//     use pool_sync::PoolType;
//     use std::time::Instant;

//     // Create mock swap params
//     fn dummy_swap_params() -> FlashQuoter::SwapParams {
//         let p1 = address!("4C36388bE6F416A29C8d8Eee81C771cE6bE14B18");
//         let p2 = address!("9A834b70C07C81a9FCB695573D9008d0eF23A998");
//         FlashQuoter::SwapParams {
//             pools: vec![p1, p2],
//             poolVersions: vec![0, 0],
//             amountIn: *AMOUNT,
//         }
//     }

//     // Test the time it takes to create a transaction
//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_sign() {
//         // init and get all dummy state
//         dotenv::dotenv().ok();
//         let key = std::env::var("PRIVATE_KEY").unwrap();
//         let key_hex = hex::decode(key).unwrap();
//         let key = SecretKey::from_bytes((&key_hex[..]).into()).unwrap();
//         let signer = PrivateKeySigner::from(key);
//         let wallet = EthereumWallet::from(signer);
//         let url = std::env::var("FULL").unwrap();
//         let wallet_provider = Arc::new(
//             ProviderBuilder::new()
//                 .with_recommended_fillers()
//                 .wallet(wallet)
//                 .on_http(url),
//         );
//         let contract_address = std::env::var("SWAP_CONTRACT").unwrap();
//         let contract = FlashSwap::new(contract_address.parse().unwrap(), wallet_provider.clone());
//         let path: FlashSwap::SwapParams = dummy_swap_params().into();

//         // benchmark tx construction
//         let gas = wallet_provider.estimate_eip1559_fees().await.unwrap();
//         let tx_time = Instant::now();
//         let max_fee = gas.max_fee_per_gas * 5; // 3x the suggested max fee
//         let priority_fee = gas.max_priority_fee_per_gas * 30; // 20x the suggested priority fee

//         let _ = contract
//             .executeArbitrage(path)
//             .max_fee_per_gas(max_fee)
//             .max_priority_fee_per_gas(priority_fee)
//             .chain_id(8453)
//             .gas(4_000_000)
//             .into_transaction_request();
//         println!("Tx construction took {:?}", tx_time.elapsed());
//     }

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_send_tx() {
//         // init environment
//         env_logger::builder().filter_level(log::LevelFilter::Info);
//         dotenv::dotenv().ok();

//         // Create gas station
//         let gas_station = Arc::new(GasStation::new());

//         // Create transaction sender
//         let mut tx_sender = TransactionSender::new(gas_station).await;

//         // Create a channel for sending events
//         let (tx, rx) = std::sync::mpsc::channel();

//         // Create and send a test event
//         let swap_path = dummy_swap_params();
//         let test_event = Event::ValidPath((
//             swap_path,
//             alloy::primitives::U256::from(10000000), // test input amount
//             100u64,                                  // dummy block number
//         ));

//         tx.send(test_event).unwrap();

//         // Send the transaction (this will only process one transaction and then exit)
//         tx_sender.send_transactions(rx).await;
//     }
// }

