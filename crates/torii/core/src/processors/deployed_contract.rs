use anyhow::{Error, Ok, Result};
use async_trait::async_trait;
use dojo_world::contracts::world::WorldContractReader;
use dojo_world::manifest::Manifest;
use starknet::core::types::{BlockWithTxs, Event, InvokeTransactionReceipt};
use starknet::providers::Provider;
use tracing::info;

use super::EventProcessor;
use crate::sql::Sql;

#[derive(Default)]
pub struct DeployedContractProcessor {
    pub manifest: Option<Manifest>,
}

#[async_trait]
impl<P> EventProcessor<P> for DeployedContractProcessor
where
    P: Provider + Send + Sync,
{
    fn event_key(&self) -> String {
        "ContractDeployed".to_string()
    }

    fn validate(&self, event: &Event) -> bool {
        if event.keys.len() > 1 {
            info!(
                "invalid keys for event {}: {}",
                <DeployedContractProcessor as EventProcessor<P>>::event_key(self),
                <DeployedContractProcessor as EventProcessor<P>>::event_keys_as_string(self, event),
            );
            return false;
        }
        true
    }

    async fn process(
        &self,
        world: &WorldContractReader<P>,
        _db: &mut Sql,
        _block: &BlockWithTxs,
        _invoke_receipt: &InvokeTransactionReceipt,
        _event_id: &str,
        event: &Event,
    ) -> Result<(), Error> {
        info!("Deployed contract: {:?}", event);

        Ok(())
    }
}
