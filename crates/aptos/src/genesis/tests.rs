// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use crate::common::types::OptionalPoolAddressArgs;
use crate::common::utils::read_from_file;
use crate::genesis::git::FRAMEWORK_NAME;
use crate::genesis::git::{from_yaml, BALANCES_FILE, EMPLOYEE_VESTING_ACCOUNTS_FILE};
use crate::genesis::keys::{GenerateLayoutTemplate, PUBLIC_KEYS_FILE};
use crate::{
    common::{
        types::{PromptOptions, RngArgs},
        utils::write_to_file,
    },
    genesis::{
        git::{GitOptions, SetupGit},
        keys::{GenerateKeys, SetValidatorConfiguration},
        GenerateGenesis,
    },
    CliCommand,
};
use aptos_crypto::{
    ed25519::{Ed25519PrivateKey, Ed25519PublicKey},
    PrivateKey,
};
use aptos_genesis::config::{HostAndPort, Layout};
use aptos_keygen::KeyGen;
use aptos_temppath::TempPath;
use aptos_types::account_address::AccountAddress;
use aptos_types::chain_id::ChainId;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};
use vm_genesis::{AccountMap, EmployeeAccountMap};

/// Test the E2E genesis flow since it doesn't require a node to run
#[tokio::test]
async fn test_genesis_e2e_flow() {
    let dir = TempPath::new();
    dir.create_as_dir().unwrap();
    let git_options = create_users(2, &dir).await;

    // Now generate genesis
    let output_dir = TempPath::new();
    output_dir.create_as_dir().unwrap();
    let output_dir = PathBuf::from(output_dir.path());
    generate_genesis(git_options, output_dir.clone(), false).await;

    // TODO: Verify that these are good
    let waypoint_file = output_dir.join("waypoint.txt");
    assert!(waypoint_file.exists());
    let genesis_file = output_dir.join("genesis.blob");
    assert!(genesis_file.exists());
}

#[tokio::test]
async fn test_mainnet_genesis_e2e_flow() {
    let dir = TempPath::new();
    dir.create_as_dir().unwrap();
    let git_options = create_users(2, &dir).await;

    // Create initial balances and employee vesting account files.
    let git_dir = git_options.local_repository_dir.as_ref().unwrap().as_path();
    create_account_balances_file(
        PathBuf::from(git_dir),
        vec![
            AccountAddress::from_hex_literal("0x123").unwrap(),
            AccountAddress::from_hex_literal("0x234").unwrap(),
        ],
        vec![1, 2],
    )
    .await;
    create_employee_vesting_accounts_file(PathBuf::from(git_dir)).await;

    // Now generate genesis
    let output_dir = TempPath::new();
    output_dir.create_as_dir().unwrap();
    let output_dir = PathBuf::from(output_dir.path());
    generate_genesis(git_options, output_dir.clone(), true).await;

    // TODO: Verify that these are good
    let waypoint_file = output_dir.join("waypoint.txt");
    assert!(waypoint_file.exists());
    let genesis_file = output_dir.join("genesis.blob");
    assert!(genesis_file.exists());
}

async fn create_users(num_users: u8, dir: &TempPath) -> GitOptions {
    let mut users: HashMap<String, PathBuf> = HashMap::new();
    for i in 0..num_users {
        let name = format!("user-{}", i);
        let output_dir = generate_keys(dir.path(), i).await;
        users.insert(name, output_dir);
    }

    let names = users.keys().map(|key| key.to_string()).collect();
    let mut key_gen = KeyGen::from_seed([num_users.saturating_add(1); 32]);
    // First step is setup the local git repo
    let root_private_key = key_gen.generate_ed25519_private_key();
    let git_options = setup_git_dir(&root_private_key, names, ChainId::test()).await;

    for (name, user_dir) in users.iter() {
        set_validator_config(name.to_string(), git_options.clone(), user_dir.as_path()).await;
    }
    git_options
}

/// Generate genesis and waypoint
async fn generate_genesis(git_options: GitOptions, output_dir: PathBuf, mainnet: bool) {
    let command = GenerateGenesis {
        prompt_options: PromptOptions::yes(),
        git_options,
        output_dir: Some(output_dir),
        mainnet: Some(mainnet),
    };
    let _ = command.execute().await.unwrap();
}

/// Setup a temporary repo location and add all required pieces
async fn setup_git_dir(
    root_private_key: &Ed25519PrivateKey,
    users: Vec<String>,
    chain_id: ChainId,
) -> GitOptions {
    let git_options = git_options();
    let layout_file = TempPath::new();
    layout_file.create_as_file().unwrap();
    let layout_file = layout_file.path();

    create_layout_file(layout_file, root_private_key.public_key(), users, chain_id).await;
    let setup_command = SetupGit {
        git_options: git_options.clone(),
        layout_file: PathBuf::from(layout_file),
    };

    setup_command
        .execute()
        .await
        .expect("Should not fail creating repo folder");

    // Add framework
    add_framework_to_dir(git_options.local_repository_dir.as_ref().unwrap().as_path());
    git_options
}

/// Add framework to git directory
fn add_framework_to_dir(git_dir: &Path) {
    cached_packages::head_release_bundle()
        .write(git_dir.join(FRAMEWORK_NAME))
        .unwrap()
}

/// Local git options for testing
fn git_options() -> GitOptions {
    let temp_path = TempPath::new();
    let path = PathBuf::from(temp_path.path());
    GitOptions {
        local_repository_dir: Some(path),
        ..Default::default()
    }
}

/// Create a layout file for the repo
async fn create_layout_file(
    file: &Path,
    root_public_key: Ed25519PublicKey,
    users: Vec<String>,
    chain_id: ChainId,
) {
    GenerateLayoutTemplate {
        output_file: PathBuf::from(file),
        prompt_options: PromptOptions::yes(),
    }
    .execute()
    .await
    .expect("Expected to create layout template");

    // Update layout file
    let mut layout: Layout =
        from_yaml(&String::from_utf8(read_from_file(file).unwrap()).unwrap()).unwrap();
    layout.root_key = Some(root_public_key);
    layout.users = users;
    layout.chain_id = chain_id;
    layout.is_test = true;

    write_to_file(
        file,
        "Layout file",
        serde_yaml::to_string(&layout).unwrap().as_bytes(),
    )
    .unwrap();
}

/// Generate keys for a "user"
async fn generate_keys(dir: &Path, index: u8) -> PathBuf {
    let output_dir = dir.join(index.to_string());
    let command = GenerateKeys {
        pool_address_args: OptionalPoolAddressArgs { pool_address: None },
        rng_args: RngArgs::from_seed([index; 32]),
        prompt_options: PromptOptions::yes(),
        output_dir: Some(output_dir.clone()),
    };
    let _ = command.execute().await.unwrap();

    output_dir
}

/// Set validator configuration for a user
async fn set_validator_config(username: String, git_options: GitOptions, keys_dir: &Path) {
    let command = SetValidatorConfiguration {
        username,
        git_options,
        owner_public_identity_file: Some(PathBuf::from(keys_dir).join(PUBLIC_KEYS_FILE)),
        validator_host: HostAndPort::from_str("localhost:6180").unwrap(),
        stake_amount: 100_000_000_000_000,
        full_node_host: None,
        operator_public_identity_file: None,
        voter_public_identity_file: None,
        commission_percentage: 0,
    };

    command.execute().await.unwrap()
}

async fn create_account_balances_file(
    path: PathBuf,
    addresses: Vec<AccountAddress>,
    balances: Vec<u64>,
) {
    let account_balances: &Vec<AccountMap> = &addresses
        .iter()
        .zip(balances)
        .map(|(account_address, balance)| AccountMap {
            account_address: *account_address,
            balance,
        })
        .collect();

    write_to_file(
        &path.join(BALANCES_FILE),
        BALANCES_FILE,
        serde_yaml::to_string(&account_balances).unwrap().as_bytes(),
    )
    .unwrap();
}

// TODO: Update to generate real employee vesting account details.
async fn create_employee_vesting_accounts_file(path: PathBuf) {
    let employee_vesting_accounts: Vec<EmployeeAccountMap> = vec![];

    write_to_file(
        &path.join(EMPLOYEE_VESTING_ACCOUNTS_FILE),
        EMPLOYEE_VESTING_ACCOUNTS_FILE,
        serde_yaml::to_string(&employee_vesting_accounts)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
}
