#!/usr/bin/env bash
dfx stop
# set -e
# trap 'dfx stop' EXIT

echo "===========SETUP========="
dfx start --background --clean
dfx identity new alice_crowdfunding --storage-mode plaintext --force
dfx identity new bob_crowdfunding --storage-mode plaintext --force
export MINTER=$(dfx --identity anonymous identity get-principal)
export DEFAULT=$(dfx identity get-principal)
dfx deploy icrc1_ledger_canister --argument "(variant { Init =
record {
     token_symbol = \"ICRC1\";
     token_name = \"L-ICRC1\";
     minting_account = record { owner = principal \"${MINTER}\" };
     transfer_fee = 10_000;
     metadata = vec {};
     initial_balances = vec { record { record { owner = principal \"${DEFAULT}\"; }; 10_000_000_000; }; };
     archive_options = record {
         num_blocks_to_archive = 1000;
         trigger_threshold = 2000;
         controller_id = principal \"${MINTER}\";
     };
 }
})"
dfx canister call icrc1_ledger_canister icrc1_balance_of "(record {
  owner = principal \"${DEFAULT}\";
  }
)"
echo "===========SETUP DONE========="

# Deploy backend canister
dfx deploy crowdfunding_backend

# Send some tokens to the canister
dfx canister call icrc1_ledger_canister icrc1_transfer "(record {
  to = record {
    owner = principal \"$(dfx canister id crowdfunding_backend)\";
  };
  amount = 1_000_000_000;
})"

# Create a project as the default user
echo "===========CREATING PROJECT========="
dfx canister call crowdfunding_backend create_project "(
  \"My First Crowdfunding Project\", 
  \"This is a test project for the ICP hackathon. It demonstrates how to create crowdfunding projects on the Internet Computer.\", 
  5_000_000_000, 
  7
)"

# Switch to Bob and fund him some tokens
dfx identity use alice_crowdfunding
export ALICE=$(dfx identity get-principal)
dfx identity use default
dfx canister call icrc1_ledger_canister icrc1_transfer "(record {
  to = record {
    owner = principal \"${ALICE}\";
  };
  amount = 2_000_000_000;
})"

# Make a contribution as Alice
dfx identity use alice_crowdfunding
dfx canister call icrc1_ledger_canister icrc1_transfer "(record {
  to = record {
    owner = principal \"$(dfx canister id crowdfunding_backend)\";
  };
  amount = 1_000_000_000;
})"

echo "===========MAKING CONTRIBUTION========="
dfx canister call crowdfunding_backend contribute "(\"project-0\", 1_000_000_000)"

# List all projects
echo "===========LISTING PROJECTS========="
dfx canister call crowdfunding_backend list_projects

# List user contributions
echo "===========LISTING USER CONTRIBUTIONS========="
dfx canister call crowdfunding_backend get_user_contributions "(principal \"${ALICE}\")"

# Switch back to default identity
dfx identity use default

echo "DEMO COMPLETED"