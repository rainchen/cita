// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

// This program is free software: you can redistribute it
// and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any
// later version.

// This program is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

extern crate mktemp;
extern crate rustc_serialize;

use self::mktemp::Temp;
use self::rustc_serialize::hex::FromHex;
use cita_crypto::KeyPair;
use cita_types::{Address, U256};
use cita_types::traits::LowerHex;
use core::libchain::chain;
use db;
use journaldb;
use libexecutor::block::{Block, BlockBody};
use libexecutor::executor::{Config, Executor};
use libexecutor::genesis::Genesis;
use libexecutor::genesis::Spec;
use libproto::blockchain;
use serde_json;
use state::State;
use state_db::*;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use types::transaction::SignedTransaction;
use util::AsMillis;
use util::KeyValueDB;
use util::crypto::CreateKey;
use util::kvdb::{Database, DatabaseConfig};

const EXECUTOR_CONFIG: &str = "executor.toml";
const CHAIN_CONFIG: &str = "chain.toml";
const GENESIS_CONFIG: &str = include_str!("../../genesis.json");
pub fn get_temp_state() -> State<StateDB> {
    let journal_db = get_temp_state_db();
    State::new(journal_db, 0.into(), Default::default())
}

fn new_db() -> Arc<KeyValueDB> {
    Arc::new(::util::kvdb::in_memory(8))
}

pub fn get_temp_state_db() -> StateDB {
    let db = new_db();
    let journal_db = journaldb::new(db, journaldb::Algorithm::Archive, ::db::COL_STATE);
    StateDB::new(journal_db, 5 * 1024 * 1024)
}

pub fn solc(name: &str, source: &str) -> (Vec<u8>, Vec<u8>) {
    // input and output of solc command
    let contract_file = Temp::new_file().unwrap().to_path_buf();
    let output_dir = Temp::new_dir().unwrap().to_path_buf();
    let deploy_code_file = output_dir.clone().join([name, ".bin"].join(""));
    let runtime_code_file = output_dir.clone().join([name, ".bin-runtime"].join(""));

    // prepare contract file
    let mut file = File::create(contract_file.clone()).unwrap();
    let mut content = String::new();
    file.write_all(source.as_ref()).expect("failed to write");

    // execute solc command
    Command::new("solc")
        .arg(contract_file.clone())
        .arg("--bin")
        .arg("--bin-runtime")
        .arg("-o")
        .arg(output_dir)
        .output()
        .expect("failed to execute solc");

    // read deploy code
    File::open(deploy_code_file)
        .expect("failed to open deploy code file!")
        .read_to_string(&mut content)
        .expect("failed to read binary");
    trace!("deploy code: {}", content);
    let deploy_code = content.as_str().from_hex().unwrap();

    // read runtime code
    let mut content = String::new();
    File::open(runtime_code_file)
        .expect("failed to open deploy code file!")
        .read_to_string(&mut content)
        .expect("failed to read binary");
    trace!("runtime code: {}", content);
    let runtime_code = content.from_hex().unwrap();
    (deploy_code, runtime_code)
}

pub fn init_executor() -> Arc<Executor> {
    let tempdir = mktemp::Temp::new_dir().unwrap().to_path_buf();
    let config = DatabaseConfig::with_columns(db::NUM_COLUMNS);
    let db = Database::open(&config, &tempdir.to_str().unwrap()).unwrap();
    // Load from genesis json file
    let spec: Spec = serde_json::from_reader::<&[u8], _>(GENESIS_CONFIG.as_ref()).expect("Failed to load genesis.");
    let genesis = Genesis {
        spec: spec,
        block: Block::default(),
    };

    let executor_config = Config::new(EXECUTOR_CONFIG);
    Arc::new(Executor::init_executor(
        Arc::new(db),
        genesis,
        executor_config,
    ))
}

pub fn init_chain() -> Arc<chain::Chain> {
    let tempdir = mktemp::Temp::new_dir().unwrap().to_path_buf();
    let config = DatabaseConfig::with_columns(db::NUM_COLUMNS);
    let db = Database::open(&config, &tempdir.to_str().unwrap()).unwrap();
    let chain_config = chain::Config::new(CHAIN_CONFIG);
    Arc::new(chain::Chain::init_chain(Arc::new(db), chain_config))
}

pub fn create_block(executor: &Executor, to: Address, data: &Vec<u8>, nonce: (u32, u32)) -> Block {
    let mut block = Block::new();

    block.set_parent_hash(executor.get_current_hash());
    block.set_timestamp(UNIX_EPOCH.elapsed().unwrap().as_millis());
    block.set_number(executor.get_current_height() + 1);
    // header.proof= ?;

    let mut body = BlockBody::new();
    let mut txs = Vec::new();
    let keypair = KeyPair::gen_keypair();
    let privkey = keypair.privkey();

    for i in nonce.0..nonce.1 {
        let mut tx = blockchain::Transaction::new();
        if to == Address::from(0) {
            tx.set_to(String::from(""));
        } else {
            tx.set_to(to.lower_hex());
        }
        tx.set_nonce(U256::from(i).lower_hex());
        tx.set_data(data.clone());
        tx.set_valid_until_block(100);
        tx.set_quota(1844674);

        let stx = tx.sign(*privkey);
        let new_tx = SignedTransaction::new(&stx).unwrap();
        txs.push(new_tx);
    }
    body.set_transactions(txs);
    block.set_body(body);
    block
}
