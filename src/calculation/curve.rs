use super::Calculator;
use alloy::sol;
use alloy::network::Network;
use alloy::primitives::U256;
use alloy::primitives::{address, Address};
use alloy::sol_types::SolValue;
use alloy::sol_types::SolCall;
use alloy::transports::Transport;
use alloy::providers::Provider;
use alloy::dyn_abi::SolType;
use revm::{
    context::{ ContextTr, Evm},
    context_interface::{
        result::{ ExecutionResult},
        TransactTo, JournalTr,
    },
    primitives::{keccak256, KECCAK_EMPTY, Log},
    handler::{
        instructions::{EthInstructions, InstructionProvider},
        EthPrecompiles, EvmTr,
    },
    database::InMemoryDB,
    inspector::{inspect_instructions, InspectorEvmTr, JournalExt},
    interpreter::{interpreter::EthInterpreter, Interpreter, InterpreterTypes},
    Inspector,
};


sol!(
    #[sol(rpc)]
    contract CurveOut {
        function get_dy(uint256 i, uint256 j, uint256 dx) external view returns (uint256);
    }
);
impl<T, N, P> Calculator<T, N, P>
where
    T: Transport + Clone,
    N: Network,
    P: Provider<N>,
{
    pub fn curve_out(
        &self,
        index_in: U256,
        index_out: U256,
        amount_in: U256,
        pool: Address,
    ) -> U256 {
        // the function calldata
        let calldata = CurveOut::get_dyCall {
            i: index_in,
            j: index_out,
            dx: amount_in,
        }
        .abi_encode();

        // get the db and construct our evm
        let mut db = self.db.write().unwrap();
        let mut evm = Evm::builder()
            .with_db(&mut *db)
            .modify_tx_env(|tx| {
                tx.caller = address!("0000000000000000000000000000000000000001");
                tx.transact_to = TransactTo::Call(pool);
                tx.data = calldata.into();
                tx.value = U256::ZERO;
            })
            .build();

        // do the transaction
        let ref_tx = evm.transact().unwrap();
        let result = ref_tx.result;
        println!("{:#?}", result);

        match result {
            ExecutionResult::Success { output: value, .. } => {
                let a = match <U256>::abi_decode(&value.data(), false) {
                    Ok(a) => a,
                    Err(_) => U256::ZERO,
                };
                a
            }
            _ => U256::ZERO,
        }
    }
}
