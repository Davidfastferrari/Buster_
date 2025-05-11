use super::Calculator;
use alloy::sol;
use alloy_network::Network;
use alloy_primitives::U256;
use alloy_primitives::{address, Address};
use alloy_provider::Provider;
use alloy_sol_types::{SolCall, SolValue};
use alloy_transport::Transport;
use revm::interpreter::Evm;
use revm::primitives::ExecutionResult;
use revm::primitives::TransactTo;

sol!(
    #[sol(rpc)]
    contract MaverickOut {
        function calculateSwap(
            address pool,
            uint128 amount,
            bool tokenAIn,
            bool exactOutput,
            int32 tickLimit
        ) external returns (uint256 amountIn, uint256 amountOut, uint256 gasEstimate);
    }
);

impl<T, N, P> Calculator<T, N, P>
where
    T: Transport + Clone,
    N: Network,
    P: Provider<N>,
{
    pub fn maverick_v2_out(
        &self,
        amount_in: U256,
        pool: Address,
        zero_for_one: bool,
        tick_limit: i32,
    ) -> U256 {
        // the function calldata
        let calldata = MaverickOut::calculateSwapCall {
            pool,
            amount: amount_in.to::<u128>(),
            tokenAIn: zero_for_one,
            exactOutput: false,
            tickLimit: tick_limit,
        }
        .abi_encode();

        // get the db and construct our evm
        let mut db = self.db.write().unwrap();
        let mut evm = Evm::builder()
            .with_db(&mut *db)
            .modify_tx_env(|tx| {
                tx.caller = address!("0000000000000000000000000000000000000001");
                tx.transact_to =
                    TransactTo::Call(address!("b40AfdB85a07f37aE217E7D6462e609900dD8D7A"));
                tx.data = calldata.into();
                tx.value = U256::ZERO;
            })
            .build();

        // do the transaction
        let ref_tx = evm.transact().unwrap();
        let result = ref_tx.result;

        match result {
            ExecutionResult::Success { output: value, .. } => {
                let out = match <(U256, U256, U256)>::abi_decode(&value.data(), false) {
                    Ok(out) => out.1,
                    Err(_) => U256::ZERO,
                };
                out
            }
            _ => U256::ZERO,
        }
    }
}
