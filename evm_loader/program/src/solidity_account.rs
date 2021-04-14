use crate::{
    account_data::AccountData,
    hamt::Hamt,
    utils::{keccak256_digest, u256_to_h256},
};
use evm::backend::Basic;
use solana_program::{
    account_info::AccountInfo,
    pubkey::Pubkey,
    program_error::ProgramError,
};
use primitive_types::{H160, H256, U256};
use std::cell::RefCell;
use std::rc::Rc;
use std::convert::TryInto;


#[derive(Debug, Clone)]
pub struct SolidityAccount<'a> {
    account_data: AccountData,
    solana_address: &'a Pubkey,
    code_data: Option<Rc<RefCell<&'a mut [u8]>>>,
    lamports: u64,
}

impl<'a> SolidityAccount<'a> {
    pub fn new(solana_address: &'a Pubkey, data: &[u8], lamports: u64, code_data: Option<Rc<RefCell<&'a mut [u8]>>>) -> Result<Self, ProgramError> {
        debug_print!("  SolidityAccount::new");
        debug_print!("  Get data with length {}", &data.len());
        let (account_data, _) = AccountData::unpack(&data)?;
        if account_data.code_size > 0 {
            if code_data.is_none() {
                debug_print!("code_data.is_none()");
                return Err(ProgramError::InvalidAccountData);
            }
            if code_data.as_ref().unwrap().borrow().len() < account_data.code_size.try_into().unwrap() {
                debug_print!("code_data.as_ref().unwrap().borrow().len() < account_data.code_size.try_into().unwrap()");
                return Err(ProgramError::InvalidAccountData);
            }
            if account_data.code_account == Pubkey::new_from_array([0u8; 32]) {
                debug_print!("account_data.code_account == Pubkey::new_from_array([0u8; 32]");
                return Err(ProgramError::InvalidAccountData);
            }
        }
        Ok(Self{account_data, solana_address, code_data: code_data, lamports})
    }

    pub fn get_code_account(data: &[u8]) -> Result<Pubkey, ProgramError> {
        debug_print!("  SolidityAccount::get_code_account");
        debug_print!("  Get data with length {}", &data.len());
        AccountData::get_code_account(&data)
    }

    pub fn get_signer(&self) -> Pubkey {self.account_data.signer}

    pub fn get_ether(&self) -> H160 {self.account_data.ether}

    pub fn get_nonce(&self) -> u64 {self.account_data.trx_count}

    pub fn get_solana_address(&self) -> Pubkey {
        *self.solana_address
    }

    pub fn get_seeds(&self) -> (H160, u8) {
        (self.account_data.ether, self.account_data.nonce)
    }
    
    pub fn basic(&self) -> Basic {
        Basic {
            balance: self.lamports.into(),
            nonce: U256::from(self.account_data.trx_count),
        }
    }
    
    pub fn code_hash(&self) -> H256 {
        self.code(|d| {
            debug_print!("{}", &hex::encode(&d[0..32]));
            keccak256_digest(d)
        })
    }
    
    pub fn code_size(&self) -> usize {
        self.code(|d| d.len())
    }
    
    pub fn get_code(&self) -> Vec<u8> {
        self.code(|d| d.into())
    }
    
    pub fn get_storage(&self, index: &H256) -> H256 {
        let index = index.as_fixed_bytes().into();
        let value = self.storage(|storage| storage.find(index)).unwrap_or_default();
        if let Some(v) = value {u256_to_h256(v)} else {H256::default()}
    }

    pub fn update<I>(
        &mut self,
        account_info: &'a AccountInfo<'a>,
        solidity_address: H160,
        nonce: U256,
        lamports: u64,
        code: &Option<Vec<u8>>,
        storage_items: I,
        reset_storage: bool,
    ) -> Result<(), ProgramError>
    where I: IntoIterator<Item = (H256, H256)> 
    {
        println!("Update: {}, {}, {}, {:?} for {:?}", solidity_address, nonce, lamports, if let Some(_) = code {"Exist"} else {"Empty"}, self);
        let mut data = (*account_info.data).borrow_mut();
        **(*account_info.lamports).borrow_mut() = lamports;

        /*let mut current_code_size = match self.account_data {
            AccountData::Empty => 0,
            AccountData::Foreign => 0,
            AccountData::Account{code_size, ..} => code_size as usize,
        };*/
        self.account_data.trx_count = nonce.as_u64();
        if let Some(code) = code {
            if self.account_data.code_size != 0 {
                return Err(ProgramError::AccountAlreadyInitialized);
            };
            if self.code_data.is_none() {
                debug_print!("Expected code account");
                return Err(ProgramError::NotEnoughAccountKeys);
            };
            self.account_data.code_size = code.len().try_into().map_err(|_| ProgramError::AccountDataTooSmall)?;
            debug_print!("Write code");
            let mut code_data = self.code_data.as_ref().unwrap().borrow_mut();
            code_data[32..32+code.len()].copy_from_slice(&code);
            debug_print!("Code written");
        }

        debug_print!("Write account data");
        self.account_data.pack(&mut data)?;

        let mut storage_iter = storage_items.into_iter().peekable();
        let exist_items = if let Some(_) = storage_iter.peek() {true} else {false};
        if reset_storage || exist_items {
            debug_print!("Update storage");
            let code_size = self.account_data.code_size as usize;
            if code_size == 0 {return Err(ProgramError::UninitializedAccount);};

            let mut code_data = self.code_data.as_ref().unwrap().borrow_mut();
            let mut storage = Hamt::new(&mut code_data[32+code_size..], reset_storage)?;
            debug_print!("Storage initialized");
            for (key, value) in storage_iter {
                debug_print!("Storage value: {} = {}", &key.to_string(), &value.to_string());
                storage.insert(key.as_fixed_bytes().into(), value.as_fixed_bytes().into())?;
            }
        }

        debug_print!("Account updated");
        Ok(())
    }

    fn code<U, F>(&self, f: F) -> U
    where F: FnOnce(&[u8]) -> U {
        /*if let AccountData::Account{code_size,..} = self.account_data {
            if code_size > 0 {
                let data = self.account_info.data.borrow();
                let offset = AccountData::size();
                return f(&data[offset..offset+code_size as usize])
            }
        }*/
        if self.account_data.code_size > 0 && self.code_data.is_some() {
            let some_data = self.code_data.as_ref().unwrap();
            let data = some_data.borrow();
            let code_size = self.account_data.code_size as usize;
            f(&data[32..32+code_size])
        } else {
            f(&[])
        }
    }

    fn storage<U, F>(&self, f: F) -> Result<U, ProgramError>
    where F: FnOnce(&mut Hamt) -> U {
        /*if let AccountData::Account{code_size,..} = self.account_data {
            if code_size > 0 {
                let mut data = self.account_info.data.borrow_mut();
                debug_print!("Storage data borrowed");
                let offset = AccountData::size() + code_size as usize;
                let mut hamt = Hamt::new(&mut data[offset..], false)?;
                return Ok(f(&mut hamt));
            }
        }
        Err(ProgramError::UninitializedAccount)*/
        if self.account_data.code_size > 0 && self.code_data.is_some() {
            let some_data = self.code_data.as_ref().unwrap();
            let mut data = some_data.borrow_mut();
            debug_print!("Storage data borrowed");
            let code_size = self.account_data.code_size as usize;
            let mut hamt = Hamt::new(&mut data[32+code_size..], false)?;
            Ok(f(&mut hamt))
        } else {
            Err(ProgramError::UninitializedAccount)
        }
    }
}
