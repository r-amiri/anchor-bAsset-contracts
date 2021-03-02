use crate::state::{read_config, read_state, store_state, Config, State};

use crate::math::decimal_summation_in_256;
use cosmwasm_std::{log, Api, CosmosMsg, Decimal, Env, Extern, HandleResponse, Querier, StdError, StdResult, Storage, BankMsg, Coin};
use terra_cosmwasm::{create_swap_msg, TerraMsgWrapper};
use basset::{deduct_tax, compute_lido_fee};

/// Swap all native tokens to reward_denom
/// Only hub_contract is allowed to execute
pub fn handle_swap<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    let config = read_config(&deps.storage)?;
    let owner_addr = deps.api.human_address(&config.hub_contract).unwrap();

    if env.message.sender != owner_addr {
        return Err(StdError::unauthorized());
    }

    let contr_addr = env.contract.address;
    let balance = deps.querier.query_all_balances(contr_addr.clone())?;
    let mut msgs: Vec<CosmosMsg<TerraMsgWrapper>> = Vec::new();

    let reward_denom = config.reward_denom;

    for coin in balance {
        if coin.denom == reward_denom {
            continue;
        }

        msgs.push(create_swap_msg(
            contr_addr.clone(),
            coin,
            reward_denom.to_string(),
        ));
    }

    let res = HandleResponse {
        messages: msgs,
        log: vec![log("action", "swap")],
        data: None,
    };
    Ok(res)
}

/// Increase global_index according to claimed rewards amount
/// Only hub_contract is allowed to execute
pub fn handle_update_global_index<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    let config: Config = read_config(&deps.storage)?;
    let mut state: State = read_state(&deps.storage)?;

    // Permission check
    if config.hub_contract != deps.api.canonical_address(&env.message.sender)? {
        return Err(StdError::unauthorized());
    }

    // Zero staking balance check
    if state.total_balance.is_zero() {
        return Err(StdError::generic_err("No asset is bonded by Hub"));
    }

    let reward_denom = read_config(&deps.storage)?.reward_denom;

    // Load the reward contract balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), reward_denom.as_str())
        .unwrap();

    let previous_balance = state.prev_reward_balance;

    // claimed_rewards = current_balance - prev_balance;
    let mut claimed_rewards = (balance.amount - previous_balance)?;

    // subtract the Lido fee from claimed rewards and send the fee to Lido.
    let lido_fee = compute_lido_fee(claimed_rewards, config.lido_fee_rate)?;
    claimed_rewards = (claimed_rewards - lido_fee)?;

    let mut msgs: Vec<CosmosMsg<TerraMsgWrapper>> = Vec::new();
    msgs.push(BankMsg::Send {
        from_address: env.contract.address,
        to_address: config.lido_fee_address,
        amount: vec![deduct_tax(
            &deps,
            Coin {
                denom: config.reward_denom,
                amount: lido_fee,
            },
        )?],
    }.into());

    state.prev_reward_balance = (balance.amount - lido_fee)?;

    // global_index += claimed_rewards / total_balance;
    state.global_index = decimal_summation_in_256(
        state.global_index,
        Decimal::from_ratio(claimed_rewards, state.total_balance),
    );
    store_state(&mut deps.storage, &state)?;

    let res = HandleResponse {
        messages: msgs,
        log: vec![
            log("action", "update_global_index"),
            log("claimed_rewards", claimed_rewards),
            log("lido_fee", lido_fee),
        ],
        data: None,
    };

    Ok(res)
}
