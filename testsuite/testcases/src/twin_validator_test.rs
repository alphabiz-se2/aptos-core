// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use crate::{LoadDestination, NetworkLoadTest};
use aptos_sdk::move_types::account_address::AccountAddress;
use forge::{NetworkContext, NetworkTest, NodeExt, Test};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

pub struct TwinValidatorTest;

impl Test for TwinValidatorTest {
    fn name(&self) -> &'static str {
        "twin validator"
    }
}

impl NetworkLoadTest for TwinValidatorTest {
    fn setup(&self, _ctx: &mut NetworkContext) -> anyhow::Result<LoadDestination> {
        Ok(LoadDestination::AllFullnodes)
    }
}

impl NetworkTest for TwinValidatorTest {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> anyhow::Result<()> {
        let runtime = Runtime::new().unwrap();

        let all_validators_ids = ctx
            .swarm()
            .validators()
            .map(|v| v.peer_id())
            .collect::<Vec<_>>();
        let validator_count = all_validators_ids.len();
        let twin_count = 2;
        runtime.block_on(async {
            for i in 0..twin_count {
                let main_id: AccountAddress = all_validators_ids[i];
                let twin_id = all_validators_ids[i + validator_count - twin_count];
                ctx.swarm()
                    .validator_mut(twin_id)
                    .unwrap()
                    .clear_storage()
                    .await
                    .expect(
                        format!("Error while clearing storage and stopping {twin_id}").as_str(),
                    );
                let main_identity = ctx
                    .swarm()
                    .validator_mut(main_id)
                    .unwrap()
                    .get_identity()
                    .await
                    .expect(format!("Error while getting identity for {main_id}").as_str());
                ctx.swarm()
                    .validator_mut(main_id)
                    .unwrap()
                    .stop()
                    .await
                    .expect(format!("Error while stopping {twin_id}").as_str());
                ctx.swarm()
                    .validator_mut(twin_id)
                    .unwrap()
                    .set_identity(main_identity)
                    .await
                    .expect(format!("Error while setting identity for {twin_id}").as_str());
                ctx.swarm()
                    .validator_mut(twin_id)
                    .unwrap()
                    .start()
                    .await
                    .expect(format!("Error while starting {twin_id}").as_str());
                ctx.swarm()
                    .validator_mut(twin_id)
                    .unwrap()
                    .wait_until_healthy(Instant::now() + Duration::from_secs(300))
                    .await
                    .expect(format!("Error while waiting for {twin_id}").as_str());
                ctx.swarm()
                    .validator_mut(main_id)
                    .unwrap()
                    .start()
                    .await
                    .expect(format!("Error while starting {twin_id}").as_str());
            }
        });
        <dyn NetworkLoadTest>::run(self, ctx)
    }
}
