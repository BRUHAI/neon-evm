use solana_program::pubkey::Pubkey;

use crate::account::{AccountsDB, AllocateResult};
use crate::account_storage::ProgramAccountStorage;
use crate::error::{Error, Result};
use crate::evm::Machine;
use crate::executor::ExecutorState;
use crate::gasometer::Gasometer;
use crate::instruction::transaction_step::log_return_value;
use crate::types::{Address, Transaction};

pub fn validate(program_id: &Pubkey, accounts: &AccountsDB) -> Result<()> {
    for account in accounts {
        if account.owner != program_id {
            continue;
        }

        if crate::account::is_blocked(program_id, account)? {
            return Err(Error::AccountBlocked(*account.key));
        }
    }

    Ok(())
}

pub fn execute(
    accounts: AccountsDB<'_>,
    mut gasometer: Gasometer,
    trx: Transaction,
    origin: Address,
) -> Result<()> {
    let chain_id = trx.chain_id().unwrap_or(crate::config::DEFAULT_CHAIN_ID);
    let gas_limit = trx.gas_limit();
    let gas_price = trx.gas_price();

    let mut account_storage = ProgramAccountStorage::new(accounts)?;

    let (exit_reason, apply_state) = {
        let mut backend = ExecutorState::new(&account_storage);

        let mut evm = Machine::new(trx, origin, &mut backend)?;
        let (result, _) = evm.execute(u64::MAX, &mut backend)?;

        let actions = backend.into_actions();

        (result, actions)
    };

    let allocate_result = account_storage.allocate(&apply_state)?;
    if allocate_result != AllocateResult::Ready {
        return Err(Error::AccountSpaceAllocationFailure);
    }

    account_storage.apply_state_change(apply_state)?;
    account_storage.transfer_treasury_payment()?;

    gasometer.record_operator_expenses(account_storage.operator());
    let used_gas = gasometer.used_gas();
    if used_gas > gas_limit {
        return Err(Error::OutOfGas(gas_limit, used_gas));
    }

    solana_program::log::sol_log_data(&[b"GAS", &used_gas.to_le_bytes(), &used_gas.to_le_bytes()]);

    let gas_cost = used_gas.saturating_mul(gas_price);
    account_storage.transfer_gas_payment(origin, chain_id, gas_cost)?;

    log_return_value(&exit_reason);

    Ok(())
}
