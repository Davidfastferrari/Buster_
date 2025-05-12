use alloy::transports::Transport;
use alloy_network::Network;
use alloy_primitives::Address;
use alloy_provider::ext::DebugApi;
use alloy_provider::Provider;
use alloy::rpc::types::trace::common::TraceResult;
use alloy::rpc::types::trace::geth::*;
use alloy::rpc::types::trace::geth::GethDebugBuiltInTracerType::PreStateTracer;
use alloy::rpc::types::trace::geth::GethDebugTracerType::BuiltInTracer;
use alloy::eips::BlockNumberOrTag;
use log::warn;
use std::collections::BTreeMap;
use std::sync::Arc;

// Trace the block to get all addresses with storage changes
pub async fn debug_trace_block<T: Transport + Clone, N: Network, P: Provider<N>>(
    client: Arc<P>,
    block_tag: BlockNumberOrTag,
    diff_mode: bool,
) -> Vec<BTreeMap<Address, AccountState>> {
    let tracer_opts = GethDebugTracingOptions {
        config: GethDefaultTracingOptions::default(),
        ..GethDebugTracingOptions::default()
    }
    .with_tracer(BuiltInTracer(PreStateTracer))
    .with_prestate_config(PreStateConfig {
        diff_mode: Some(diff_mode),
        disable_code: Some(false),
        disable_storage: Some(false),
    });
    let results = client
        .debug_trace_block_by_number(block_tag, tracer_opts)
        .await
        .unwrap();

    let mut post: Vec<BTreeMap<Address, AccountState>> = Vec::new();

    for trace_result in results.into_iter() {
        if let TraceResult::Success { result, .. } = trace_result {
            match result {
                GethTrace::PreStateTracer(PreStateFrame::Diff(diff_frame)) => {
                    post.push(diff_frame.post)
                }
                _ => warn!("Invalid trace"),
            }
        }
    }
    post
}
