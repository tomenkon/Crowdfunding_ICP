use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::time;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, NumTokens, TransferArg, TransferError};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;

// Data structures
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Project {
    id: String,
    owner: Principal,
    title: String,
    description: String, 
    funding_goal: NumTokens,
    current_amount: NumTokens,
    deadline: u64,  // Timestamp when funding ends
    contributors: Vec<Contribution>,
    status: ProjectStatus,
    created_at: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Contribution {
    contributor: Principal,
    amount: NumTokens,
    timestamp: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum ProjectStatus {
    Active,
    Funded,
    Expired,
}

#[derive(CandidType, Deserialize, Serialize)]
pub struct TransferArgs {
    amount: NumTokens,
    to_account: Account,
}

// Store project data
thread_local! {
    static PROJECTS: RefCell<HashMap<String, Project>> = RefCell::new(HashMap::new());
    static PROJECT_COUNTER: RefCell<u64> = RefCell::new(0);
}

// Project management functions
#[ic_cdk::update]
fn create_project(title: String, description: String, funding_goal: NumTokens, duration_days: u64) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("Title cannot be empty".to_string());
    }
    
    if description.trim().is_empty() {
        return Err("Description cannot be empty".to_string());
    }
    
    if funding_goal == 0 {
        return Err("Funding goal must be greater than zero".to_string());
    }
    
    if duration_days == 0 {
        return Err("Duration must be greater than zero days".to_string());
    }
    
    let caller = ic_cdk::caller();
    
    // Generate project ID
    let project_id = PROJECT_COUNTER.with(|counter| {
        let current = *counter.borrow();
        *counter.borrow_mut() = current + 1;
        format!("project-{}", current)
    });
    
    let now = time();
    let deadline = now + (duration_days * 24 * 60 * 60 * 1_000_000_000); // Convert days to nanoseconds
    
    let project = Project {
        id: project_id.clone(),
        owner: caller,
        title,
        description,
        funding_goal,
        current_amount: 0,
        deadline,
        contributors: Vec::new(),
        status: ProjectStatus::Active,
        created_at: now,
    };
    
    PROJECTS.with(|projects| {
        projects.borrow_mut().insert(project_id.clone(), project);
    });
    
    ic_cdk::println!("Created project: {}", &project_id);
    Ok(project_id)
}

#[ic_cdk::query]
fn get_project(id: String) -> Result<Project, String> {
    PROJECTS.with(|projects| {
        projects
            .borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| "Project not found".to_string())
    })
}

#[ic_cdk::query]
fn list_projects() -> Vec<Project> {
    PROJECTS.with(|projects| {
        projects.borrow().values().cloned().collect()
    })
}

#[ic_cdk::query]
fn get_user_projects(user: Principal) -> Vec<Project> {
    PROJECTS.with(|projects| {
        projects
            .borrow()
            .values()
            .filter(|p| p.owner == user)
            .cloned()
            .collect()
    })
}

#[ic_cdk::query]
fn get_user_contributions(user: Principal) -> Vec<(String, Project, NumTokens)> {
    PROJECTS.with(|projects| {
        projects
            .borrow()
            .iter()
            .filter_map(|(id, project)| {
                let total_contribution: NumTokens = project
                    .contributors
                    .iter()
                    .filter(|c| c.contributor == user)
                    .map(|c| c.amount)
                    .sum();
                
                if total_contribution > 0 {
                    Some((id.clone(), project.clone(), total_contribution))
                } else {
                    None
                }
            })
            .collect()
    })
}

// Transfer functions
#[ic_cdk::update]
async fn contribute(project_id: String, amount: NumTokens) -> Result<BlockIndex, String> {
    if amount == 0 {
        return Err("Contribution amount must be greater than zero".to_string());
    }
    
    let caller = ic_cdk::caller();
    let anonymous_principal = Principal::anonymous();
    
    if caller == anonymous_principal {
        return Err("Anonymous principals cannot contribute".to_string());
    }
    
    // Check if project exists and is active
    let mut project = PROJECTS.with(|projects| {
        projects
            .borrow()
            .get(&project_id)
            .cloned()
            .ok_or_else(|| "Project not found".to_string())
    })?;
    
    if project.status != ProjectStatus::Active {
        return Err(format!("Project is not active. Current status: {:?}", project.status));
    }
    
    let now = time();
    if now > project.deadline {
        project.status = ProjectStatus::Expired;
        PROJECTS.with(|projects| {
            projects.borrow_mut().insert(project_id.clone(), project.clone());
        });
        return Err("Project funding period has ended".to_string());
    }
    
    // Transfer tokens to this canister
    let transfer_args = TransferArg {
        memo: Some(Memo::from(project_id.as_bytes().to_vec())),
        amount,
        from_subaccount: None,
        fee: None,
        to: Account {
            owner: ic_cdk::id(),
            subaccount: None,
        },
        created_at_time: None,
    };
    
    // Call the ledger to transfer tokens from the contributor to this canister
    let result = ic_cdk::call::<(TransferArg,), (Result<BlockIndex, TransferError>,)>(
        Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai")
            .expect("Could not decode the principal."),
        "icrc1_transfer",
        (transfer_args,),
    )
    .await
    .map_err(|e| format!("Failed to call ledger: {:?}", e))?
    .0
    .map_err(|e| format!("Ledger transfer error: {:?}", e))?;
    
    // Update project status
    project.current_amount += amount;
    project.contributors.push(Contribution {
        contributor: caller,
        amount,
        timestamp: now,
    });
    
    // Check if funding goal has been reached
    if project.current_amount >= project.funding_goal {
        project.status = ProjectStatus::Funded;
    }
    
    // Update project in storage
    PROJECTS.with(|projects| {
        projects.borrow_mut().insert(project_id, project);
    });
    
    Ok(result)
}

#[ic_cdk::update]
async fn release_funds(project_id: String) -> Result<BlockIndex, String> {
    let caller = ic_cdk::caller();
    
    // Verify project exists and is funded
    let project = PROJECTS.with(|projects| {
        projects
            .borrow()
            .get(&project_id)
            .cloned()
            .ok_or_else(|| "Project not found".to_string())
    })?;
    
    if project.status != ProjectStatus::Funded {
        return Err("Project is not funded yet".to_string());
    }
    
    if project.owner != caller {
        return Err("Only the project owner can release funds".to_string());
    }
    
    // Transfer tokens to the project owner
    let transfer_args = TransferArgs {
        amount: project.current_amount,
        to_account: Account {
            owner: project.owner,
            subaccount: None,
        },
    };
    
    let result = transfer(transfer_args).await?;
    
    // Update project to mark funds as released
    let mut updated_project = project.clone();
    updated_project.current_amount = 0; // Funds have been transferred
    
    PROJECTS.with(|projects| {
        projects.borrow_mut().insert(project_id, updated_project);
    });
    
    Ok(result)
}

#[ic_cdk::update]
async fn claim_refund(project_id: String) -> Result<BlockIndex, String> {
    let caller = ic_cdk::caller();
    
    // Verify project exists and has expired without reaching goal
    let project = PROJECTS.with(|projects| {
        projects
            .borrow()
            .get(&project_id)
            .cloned()
            .ok_or_else(|| "Project not found".to_string())
    })?;
    
    if project.status != ProjectStatus::Expired {
        return Err("Refunds only available for expired projects".to_string());
    }
    
    // Find all contributions by this caller
    let mut total_contribution: NumTokens = 0;
    let mut indices_to_remove = Vec::new();
    
    for (i, contribution) in project.contributors.iter().enumerate() {
        if contribution.contributor == caller {
            total_contribution += contribution.amount;
            indices_to_remove.push(i);
        }
    }
    
    if total_contribution == 0 {
        return Err("No contribution found for this user".to_string());
    }
    
    // Transfer tokens back to the contributor
    let transfer_args = TransferArgs {
        amount: total_contribution,
        to_account: Account {
            owner: caller,
            subaccount: None,
        },
    };
    
    let result = transfer(transfer_args).await?;
    
    // Update project state to remove the contribution
    let mut updated_project = project.clone();
    // Remove contributions in reverse order to avoid index issues
    for i in indices_to_remove.iter().rev() {
        updated_project.contributors.remove(*i);
    }
    updated_project.current_amount -= total_contribution;
    
    PROJECTS.with(|projects| {
        projects.borrow_mut().insert(project_id, updated_project);
    });
    
    Ok(result)
}

#[ic_cdk::update]
async fn transfer(args: TransferArgs) -> Result<BlockIndex, String> {
    ic_cdk::println!(
        "Transferring {} tokens to account {}",
        &args.amount,
        &args.to_account,
    );

    let transfer_args: TransferArg = TransferArg {
        memo: None,
        amount: args.amount,
        from_subaccount: None,
        fee: None,
        to: args.to_account,
        created_at_time: None,
    };

    ic_cdk::call::<(TransferArg,), (Result<BlockIndex, TransferError>,)>(
        Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai")
            .expect("Could not decode the principal."),
        "icrc1_transfer",
        (transfer_args,),
    )
    .await
    .map_err(|e| format!("failed to call ledger: {:?}", e))?
    .0
    .map_err(|e| format!("ledger transfer error {:?}", e))
}

// Initialize the canister
#[ic_cdk::init]
fn init() {
    // Set up timer to check for expired projects every hour
    ic_cdk_timers::set_timer_interval(std::time::Duration::from_secs(3600), || {
        update_project_statuses();
    });
}

fn update_project_statuses() {
    let now = time();
    
    PROJECTS.with(|projects| {
        let mut projects_ref = projects.borrow_mut();
        
        for (_, project) in projects_ref.iter_mut() {
            if project.status == ProjectStatus::Active && now > project.deadline {
                project.status = ProjectStatus::Expired;
            }
        }
    });
}

// Candid export
ic_cdk::export_candid!();