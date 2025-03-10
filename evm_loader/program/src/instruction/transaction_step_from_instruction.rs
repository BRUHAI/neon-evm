use crate::account::legacy::{TAG_HOLDER_DEPRECATED, TAG_STATE_FINALIZED_DEPRECATED};
use crate::account::{
    program, AccountsDB, BalanceAccount, Operator, StateAccount, Treasury, TAG_HOLDER, TAG_STATE,
    TAG_STATE_FINALIZED,
};
use crate::error::{Error, Result};
use crate::gasometer::Gasometer;
use crate::instruction::transaction_step::{do_begin, do_continue};
use crate::types::Transaction;
use arrayref::array_ref;
use ethnum::U256;
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};

pub fn process<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    instruction: &[u8],
) -> Result<()> {
    solana_program::msg!("Instruction: Begin or Continue Transaction from Instruction");

    let treasury_index = u32::from_le_bytes(*array_ref![instruction, 0, 4]);
    let step_count = u64::from(u32::from_le_bytes(*array_ref![instruction, 4, 4]));
    // skip let unique_index = u32::from_le_bytes(*array_ref![instruction, 8, 4]);
    let message = &instruction[4 + 4 + 4..];

    let storage_info = accounts[0].clone();

    let operator = Operator::from_account(&accounts[1])?;
    let treasury = Treasury::from_account(program_id, treasury_index, &accounts[2])?;
    let operator_balance = BalanceAccount::from_account(program_id, accounts[3].clone())?;
    let system = program::System::from_account(&accounts[4])?;

    let miner_address = operator_balance.address();

    let accounts_db = AccountsDB::new(
        &accounts[5..],
        operator.clone(),
        Some(operator_balance),
        Some(system),
        Some(treasury),
    );

    let mut excessive_lamports = 0_u64;

    let mut tag = crate::account::tag(program_id, &storage_info)?;
    if (tag == TAG_HOLDER_DEPRECATED) || (tag == TAG_STATE_FINALIZED_DEPRECATED) {
        tag = crate::account::legacy::update_holder_account(&storage_info)?;
    }

    match tag {
        TAG_HOLDER | TAG_STATE_FINALIZED => {
            let trx = Transaction::from_rlp(message)?;
            let origin = trx.recover_caller_address()?;

            solana_program::log::sol_log_data(&[b"HASH", &trx.hash()]);
            solana_program::log::sol_log_data(&[b"MINER", miner_address.as_bytes()]);

            let mut gasometer = Gasometer::new(U256::ZERO, accounts_db.operator())?;
            gasometer.record_solana_transaction_cost();
            gasometer.record_address_lookup_table(accounts);

            excessive_lamports += crate::account::legacy::update_legacy_accounts(&accounts_db)?;
            gasometer.refund_lamports(excessive_lamports);

            let storage = StateAccount::new(program_id, storage_info, &accounts_db, origin, &trx)?;

            do_begin(accounts_db, storage, gasometer, trx, origin)
        }
        TAG_STATE => {
            let storage = StateAccount::restore(program_id, storage_info, &accounts_db, false)?;

            solana_program::log::sol_log_data(&[b"HASH", &storage.trx_hash()]);
            solana_program::log::sol_log_data(&[b"MINER", miner_address.as_bytes()]);

            let mut gasometer = Gasometer::new(storage.gas_used(), accounts_db.operator())?;
            gasometer.record_solana_transaction_cost();

            do_continue(step_count, accounts_db, storage, gasometer)
        }
        _ => Err(Error::AccountInvalidTag(*storage_info.key, TAG_HOLDER)),
    }?;

    **operator.try_borrow_mut_lamports()? += excessive_lamports;

    Ok(())
}
