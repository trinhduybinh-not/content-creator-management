#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    Address, Env, String,
    symbol_short,
    token,
};

// ============================================================
//  STORAGE KEYS
// ============================================================

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    // Channel
    Channel(u64),
    ChannelCount,
    ChannelByOwner(Address),

    // Content / Copyright
    Content(u64),
    ContentCount,
    ContentsByChannel(u64),
    OwnershipClaim(u64),       // content_id -> disputer Address

    // Viewer Stats
    ViewCount(u64),            // content_id -> total views
    UniqueViewers(u64),        // content_id -> viewer count
    ViewerRecord(u64, Address),// (content_id, viewer) -> viewed bool
    ChannelTotalViews(u64),    // channel_id -> aggregate views
    ChannelSubscribers(u64),   // channel_id -> subscriber count
    IsSubscribed(u64, Address),// (channel_id, user) -> bool

    // Jobs / Plans
    Job(u64),
    JobCount,
    JobsByChannel(u64),
}

// ============================================================
//  DATA STRUCTURES
// ============================================================

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelStatus {
    Active,
    Suspended,
    Closed,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Channel {
    pub id: u64,
    pub owner: Address,
    pub name: String,
    pub description: String,
    pub category: String,         // e.g. "music", "gaming", "education"
    pub subscriber_count: u64,
    pub total_views: u64,
    pub content_count: u64,
    pub created_at: u64,
    pub status: ChannelStatus,
    pub royalty_address: Address, // where royalties are sent
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LicenseType {
    AllRightsReserved,
    CreativeCommons,
    OpenSource,
    CommercialAllowed,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CopyrightStatus {
    Registered,
    Disputed,
    Resolved,
    Revoked,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Content {
    pub id: u64,
    pub channel_id: u64,
    pub owner: Address,
    pub title: String,
    pub description: String,
    pub content_hash: String,     // IPFS CID or SHA256 fingerprint
    pub content_type: String,     // "video", "audio", "image", "text"
    pub license: LicenseType,
    pub royalty_bps: u32,         // basis points, e.g. 500 = 5%
    pub copyright_status: CopyrightStatus,
    pub registered_at: u64,
    pub view_count: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum JobStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
    Failed,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum JobType {
    LiveStream,
    VideoUpload,
    AudioUpload,
    MarketingCampaign,
    Collaboration,
    Maintenance,
    Other,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub channel_id: u64,
    pub creator: Address,
    pub title: String,
    pub description: String,
    pub job_type: JobType,
    pub status: JobStatus,
    pub scheduled_at: u64,   // unix timestamp
    pub deadline_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub reward_xlm: i128,    // optional reward in stroops (1 XLM = 10^7 stroops)
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ViewerStats {
    pub content_id: u64,
    pub total_views: u64,
    pub unique_viewers: u64,
    pub last_viewed_at: u64,
}

// ============================================================
//  ERROR CODES
// ============================================================

#[contracterror]
#[derive(Clone, Debug, PartialEq)]
pub enum ContractError {
    NotFound          = 1,
    Unauthorized      = 2,
    AlreadyExists     = 3,
    InvalidInput      = 4,
    InsufficientFunds = 5,
    ContentDisputed   = 6,
    ChannelSuspended  = 7,
    JobNotPending     = 8,
}

// ============================================================
//  CONTRACT
// ============================================================

#[contract]
pub struct CreatorPlatformContract;

#[contractimpl]
impl CreatorPlatformContract {

    // ============================================================
    //  CHANNEL MODULE
    // ============================================================

    /// Create a new channel. Returns the new channel ID.
    pub fn create_channel(
        env: Env,
        owner: Address,
        name: String,
        description: String,
        category: String,
        royalty_address: Address,
    ) -> Result<u64, ContractError> {
        owner.require_auth();

        // Prevent duplicate channel per owner (one channel per address)
        if env.storage().persistent().has(&DataKey::ChannelByOwner(owner.clone())) {
            return Err(ContractError::AlreadyExists);
        }

        let channel_id = Self::next_id(&env, DataKey::ChannelCount);

        let channel = Channel {
            id: channel_id,
            owner: owner.clone(),
            name,
            description,
            category,
            subscriber_count: 0,
            total_views: 0,
            content_count: 0,
            created_at: env.ledger().timestamp(),
            status: ChannelStatus::Active,
            royalty_address,
        };

        env.storage().persistent().set(&DataKey::Channel(channel_id), &channel);
        env.storage().persistent().set(&DataKey::ChannelByOwner(owner), &channel_id);
        env.storage().persistent().set(&DataKey::ChannelSubscribers(channel_id), &0u64);
        env.storage().persistent().set(&DataKey::ChannelTotalViews(channel_id), &0u64);

        env.events().publish((symbol_short!("ch_crt"), channel_id), channel_id);

        Ok(channel_id)
    }

    /// Update channel metadata. Only owner can update.
    pub fn update_channel(
        env: Env,
        owner: Address,
        channel_id: u64,
        name: String,
        description: String,
        category: String,
    ) -> Result<(), ContractError> {
        owner.require_auth();

        let mut channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)?;

        if channel.owner != owner {
            return Err(ContractError::Unauthorized);
        }
        if channel.status == ChannelStatus::Suspended {
            return Err(ContractError::ChannelSuspended);
        }

        channel.name = name;
        channel.description = description;
        channel.category = category;

        env.storage().persistent().set(&DataKey::Channel(channel_id), &channel);
        Ok(())
    }

    /// Get channel information.
    pub fn get_channel(env: Env, channel_id: u64) -> Result<Channel, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)
    }

    /// Get channel ID by owner address.
    pub fn get_channel_by_owner(env: Env, owner: Address) -> Result<u64, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ChannelByOwner(owner))
            .ok_or(ContractError::NotFound)
    }

    /// Subscribe to a channel. Returns updated subscriber count.
    pub fn subscribe(env: Env, user: Address, channel_id: u64) -> Result<u64, ContractError> {
        user.require_auth();

        let sub_key = DataKey::IsSubscribed(channel_id, user.clone());
        if env.storage().persistent().get::<DataKey, bool>(&sub_key).unwrap_or(false) {
            return Err(ContractError::AlreadyExists);
        }

        let mut channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)?;

        channel.subscriber_count += 1;
        env.storage().persistent().set(&DataKey::Channel(channel_id), &channel);
        env.storage().persistent().set(&sub_key, &true);

        env.events().publish((symbol_short!("subscrib"), channel_id), user);

        Ok(channel.subscriber_count)
    }

    /// Unsubscribe from a channel.
    pub fn unsubscribe(env: Env, user: Address, channel_id: u64) -> Result<u64, ContractError> {
        user.require_auth();

        let sub_key = DataKey::IsSubscribed(channel_id, user.clone());
        if !env.storage().persistent().get::<DataKey, bool>(&sub_key).unwrap_or(false) {
            return Err(ContractError::NotFound);
        }

        let mut channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)?;

        if channel.subscriber_count > 0 {
            channel.subscriber_count -= 1;
        }
        env.storage().persistent().set(&DataKey::Channel(channel_id), &channel);
        env.storage().persistent().remove(&sub_key);

        Ok(channel.subscriber_count)
    }

    /// Check if a user is subscribed to a channel.
    pub fn is_subscribed(env: Env, user: Address, channel_id: u64) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::IsSubscribed(channel_id, user))
            .unwrap_or(false)
    }

    // ============================================================
    //  CONTENT / COPYRIGHT MODULE
    // ============================================================

    /// Register new content and its copyright on-chain.
    pub fn register_content(
        env: Env,
        owner: Address,
        channel_id: u64,
        title: String,
        description: String,
        content_hash: String,
        content_type: String,
        license: LicenseType,
        royalty_bps: u32,         // max 10000 = 100%
    ) -> Result<u64, ContractError> {
        owner.require_auth();

        // Validate royalty
        if royalty_bps > 10_000 {
            return Err(ContractError::InvalidInput);
        }

        // Validate channel ownership
        let mut channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)?;

        if channel.owner != owner {
            return Err(ContractError::Unauthorized);
        }

        let content_id = Self::next_id(&env, DataKey::ContentCount);

        let content = Content {
            id: content_id,
            channel_id,
            owner: owner.clone(),
            title,
            description,
            content_hash,
            content_type,
            license,
            royalty_bps,
            copyright_status: CopyrightStatus::Registered,
            registered_at: env.ledger().timestamp(),
            view_count: 0,
        };

        env.storage().persistent().set(&DataKey::Content(content_id), &content);

        // Update channel content count
        channel.content_count += 1;
        env.storage().persistent().set(&DataKey::Channel(channel_id), &channel);

        // Init viewer stats for this content
        env.storage().persistent().set(&DataKey::ViewCount(content_id), &0u64);
        env.storage().persistent().set(&DataKey::UniqueViewers(content_id), &0u64);

        env.events().publish((symbol_short!("reg_ctt"), content_id), (content_id, channel_id));

        Ok(content_id)
    }

    /// Get content / copyright info.
    pub fn get_content(env: Env, content_id: u64) -> Result<Content, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)
    }

    /// Verify that a given hash matches registered content (copyright check).
    pub fn verify_content_hash(
        env: Env,
        content_id: u64,
        hash_to_check: String,
    ) -> Result<bool, ContractError> {
        let content: Content = env
            .storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)?;

        Ok(content.content_hash == hash_to_check
            && content.copyright_status == CopyrightStatus::Registered)
    }

    /// File a copyright dispute for a piece of content.
    pub fn dispute_content(
        env: Env,
        disputer: Address,
        content_id: u64,
    ) -> Result<(), ContractError> {
        disputer.require_auth();

        let mut content: Content = env
            .storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)?;

        if content.copyright_status != CopyrightStatus::Registered {
            return Err(ContractError::InvalidInput);
        }

        content.copyright_status = CopyrightStatus::Disputed;
        env.storage().persistent().set(&DataKey::Content(content_id), &content);
        env.storage()
            .persistent()
            .set(&DataKey::OwnershipClaim(content_id), &disputer);

        env.events().publish((symbol_short!("dispute"), content_id), content_id);

        Ok(())
    }

    /// Resolve a copyright dispute (admin/owner action).
    pub fn resolve_dispute(
        env: Env,
        admin: Address,
        content_id: u64,
        uphold_original: bool,
    ) -> Result<(), ContractError> {
        admin.require_auth();

        let mut content: Content = env
            .storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)?;

        if content.copyright_status != CopyrightStatus::Disputed {
            return Err(ContractError::InvalidInput);
        }

        content.copyright_status = if uphold_original {
            CopyrightStatus::Registered
        } else {
            CopyrightStatus::Revoked
        };

        env.storage().persistent().set(&DataKey::Content(content_id), &content);
        env.storage().persistent().remove(&DataKey::OwnershipClaim(content_id));

        Ok(())
    }

    /// Pay royalty to content owner via XLM.
    /// Caller pays `amount_stroops` to the content's registered royalty address.
    pub fn pay_royalty(
        env: Env,
        payer: Address,
        content_id: u64,
        xlm_token: Address,
        amount_stroops: i128,
    ) -> Result<(), ContractError> {
        payer.require_auth();

        let content: Content = env
            .storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)?;

        if content.copyright_status == CopyrightStatus::Revoked
            || content.copyright_status == CopyrightStatus::Disputed
        {
            return Err(ContractError::ContentDisputed);
        }

        // Compute royalty share
        let royalty_amount = amount_stroops * (content.royalty_bps as i128) / 10_000;

        let xlm_client = token::Client::new(&env, &xlm_token);
        let channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(content.channel_id))
            .ok_or(ContractError::NotFound)?;

        xlm_client.transfer(&payer, &channel.royalty_address, &royalty_amount);

        env.events().publish((symbol_short!("royalty"), content_id), royalty_amount);

        Ok(())
    }

    // ============================================================
    //  VIEWER STATS MODULE
    // ============================================================

    /// Record a view for a piece of content.
    /// Increments total views; also increments unique viewers if first-time viewer.
    pub fn record_view(
        env: Env,
        viewer: Address,
        content_id: u64,
    ) -> Result<u64, ContractError> {
        // No auth required — viewing is permissionless

        let mut content: Content = env
            .storage()
            .persistent()
            .get(&DataKey::Content(content_id))
            .ok_or(ContractError::NotFound)?;

        if content.copyright_status == CopyrightStatus::Revoked {
            return Err(ContractError::ContentDisputed);
        }

        // Increment total views
        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ViewCount(content_id))
            .unwrap_or(0);
        let new_total = total + 1;
        env.storage().persistent().set(&DataKey::ViewCount(content_id), &new_total);

        // Update content struct
        content.view_count = new_total;
        env.storage().persistent().set(&DataKey::Content(content_id), &content);

        // Check unique viewer
        let viewer_key = DataKey::ViewerRecord(content_id, viewer.clone());
        if !env.storage().temporary().has(&viewer_key) {
            let unique: u64 = env
                .storage()
                .persistent()
                .get(&DataKey::UniqueViewers(content_id))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::UniqueViewers(content_id), &(unique + 1));

            // Store as temporary (TTL = ~1 day in ledgers) to re-count returning viewers
            env.storage().temporary().set(&viewer_key, &true);
            env.storage().temporary().extend_ttl(&viewer_key, 17280, 17280);
        }

        // Update channel aggregate
        let ch_views: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ChannelTotalViews(content.channel_id))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::ChannelTotalViews(content.channel_id),
            &(ch_views + 1),
        );

        // Sync channel struct
        let mut channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(content.channel_id))
            .ok_or(ContractError::NotFound)?;
        channel.total_views = ch_views + 1;
        env.storage().persistent().set(&DataKey::Channel(content.channel_id), &channel);

        Ok(new_total)
    }

    /// Get viewer stats for a specific content.
    pub fn get_viewer_stats(env: Env, content_id: u64) -> Result<ViewerStats, ContractError> {
        if !env.storage().persistent().has(&DataKey::Content(content_id)) {
            return Err(ContractError::NotFound);
        }

        let total_views: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ViewCount(content_id))
            .unwrap_or(0);

        let unique_viewers: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::UniqueViewers(content_id))
            .unwrap_or(0);

        Ok(ViewerStats {
            content_id,
            total_views,
            unique_viewers,
            last_viewed_at: env.ledger().timestamp(),
        })
    }

    /// Get total views for an entire channel.
    pub fn get_channel_views(env: Env, channel_id: u64) -> Result<u64, ContractError> {
        if !env.storage().persistent().has(&DataKey::Channel(channel_id)) {
            return Err(ContractError::NotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::ChannelTotalViews(channel_id))
            .unwrap_or(0))
    }

    /// Get subscriber count for a channel.
    pub fn get_subscriber_count(env: Env, channel_id: u64) -> Result<u64, ContractError> {
        if !env.storage().persistent().has(&DataKey::Channel(channel_id)) {
            return Err(ContractError::NotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::ChannelSubscribers(channel_id))
            .unwrap_or(0))
    }

    // ============================================================
    //  JOB / PLAN MODULE
    // ============================================================

    /// Create a new scheduled job/plan for a channel.
    pub fn create_job(
        env: Env,
        creator: Address,
        channel_id: u64,
        title: String,
        description: String,
        job_type: JobType,
        scheduled_at: u64,
        deadline_at: u64,
        reward_xlm: i128,
    ) -> Result<u64, ContractError> {
        creator.require_auth();

        // Validate channel ownership
        let channel: Channel = env
            .storage()
            .persistent()
            .get(&DataKey::Channel(channel_id))
            .ok_or(ContractError::NotFound)?;

        if channel.owner != creator {
            return Err(ContractError::Unauthorized);
        }
        if scheduled_at >= deadline_at {
            return Err(ContractError::InvalidInput);
        }

        let job_id = Self::next_id(&env, DataKey::JobCount);

        let job = Job {
            id: job_id,
            channel_id,
            creator: creator.clone(),
            title,
            description,
            job_type,
            status: JobStatus::Pending,
            scheduled_at,
            deadline_at,
            created_at: env.ledger().timestamp(),
            updated_at: env.ledger().timestamp(),
            reward_xlm,
        };

        env.storage().persistent().set(&DataKey::Job(job_id), &job);

        env.events().publish((symbol_short!("job_new"), job_id), (job_id, channel_id));

        Ok(job_id)
    }

    /// Update job status (only channel owner can update).
    pub fn update_job_status(
        env: Env,
        caller: Address,
        job_id: u64,
        new_status: JobStatus,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let mut job: Job = env
            .storage()
            .persistent()
            .get(&DataKey::Job(job_id))
            .ok_or(ContractError::NotFound)?;

        if job.creator != caller {
            return Err(ContractError::Unauthorized);
        }

        // State machine: only valid transitions
        match (&job.status, &new_status) {
            (JobStatus::Pending,     JobStatus::InProgress)  => {}
            (JobStatus::Pending,     JobStatus::Cancelled)   => {}
            (JobStatus::InProgress,  JobStatus::Completed)   => {}
            (JobStatus::InProgress,  JobStatus::Failed)      => {}
            (JobStatus::InProgress,  JobStatus::Cancelled)   => {}
            _ => return Err(ContractError::InvalidInput),
        }

        job.status = new_status;
        job.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&DataKey::Job(job_id), &job);

        env.events().publish((symbol_short!("job_upd"), job_id), job_id);

        Ok(())
    }

    /// Get a job by ID.
    pub fn get_job(env: Env, job_id: u64) -> Result<Job, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Job(job_id))
            .ok_or(ContractError::NotFound)
    }

    /// Cancel a job (only if Pending or InProgress).
    pub fn cancel_job(env: Env, caller: Address, job_id: u64) -> Result<(), ContractError> {
        caller.require_auth();

        let mut job: Job = env
            .storage()
            .persistent()
            .get(&DataKey::Job(job_id))
            .ok_or(ContractError::NotFound)?;

        if job.creator != caller {
            return Err(ContractError::Unauthorized);
        }
        if job.status == JobStatus::Completed || job.status == JobStatus::Cancelled {
            return Err(ContractError::InvalidInput);
        }

        job.status = JobStatus::Cancelled;
        job.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
        Ok(())
    }

    /// Get total number of jobs created (useful for iteration).
    pub fn get_job_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::JobCount)
            .unwrap_or(0)
    }

    /// Get total number of channels.
    pub fn get_channel_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::ChannelCount)
            .unwrap_or(0)
    }

    /// Get total number of registered contents.
    pub fn get_content_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::ContentCount)
            .unwrap_or(0)
    }

    // ============================================================
    //  INTERNAL HELPERS
    // ============================================================

    /// Auto-increment counter and return new ID.
    fn next_id(env: &Env, key: DataKey) -> u64 {
        let current: u64 = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(0);
        let next = current + 1;
        env.storage().persistent().set(&key, &next);
        next
    }
}

// ============================================================
//  UNIT TESTS
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env, String};

    fn setup() -> (Env, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let owner = Address::generate(&env);
        (env, owner)
    }

    #[test]
    fn test_create_channel() {
        let (env, owner) = setup();
        let client = CreatorPlatformContractClient::new(&env, &env.register_contract(None, CreatorPlatformContract {}));

        let channel_id = client
            .create_channel(
                &owner,
                &String::from_str(&env, "My Music Channel"),
                &String::from_str(&env, "Original music and covers"),
                &String::from_str(&env, "music"),
                &owner,
            )
            .unwrap();

        assert_eq!(channel_id, 1);

        let channel = client.get_channel(&channel_id).unwrap();
        assert_eq!(channel.name, String::from_str(&env, "My Music Channel"));
        assert_eq!(channel.subscriber_count, 0);
    }

    #[test]
    fn test_subscribe_unsubscribe() {
        let (env, owner) = setup();
        let client = CreatorPlatformContractClient::new(&env, &env.register_contract(None, CreatorPlatformContract {}));
        let channel_id = client
            .create_channel(&owner, &String::from_str(&env, "Test"), &String::from_str(&env, "Desc"), &String::from_str(&env, "gaming"), &owner)
            .unwrap();

        let user = Address::generate(&env);
        let subs = client.subscribe(&user, &channel_id).unwrap();
        assert_eq!(subs, 1);

        let is_sub = client.is_subscribed(&user, &channel_id);
        assert!(is_sub);

        let subs_after = client.unsubscribe(&user, &channel_id).unwrap();
        assert_eq!(subs_after, 0);
    }

    #[test]
    fn test_register_content_and_view() {
        let (env, owner) = setup();
        let client = CreatorPlatformContractClient::new(&env, &env.register_contract(None, CreatorPlatformContract {}));
        let channel_id = client
            .create_channel(&owner, &String::from_str(&env, "Art"), &String::from_str(&env, "Art channel"), &String::from_str(&env, "art"), &owner)
            .unwrap();

        let content_id = client
            .register_content(
                &owner,
                &channel_id,
                &String::from_str(&env, "My First Song"),
                &String::from_str(&env, "Original composition"),
                &String::from_str(&env, "Qm1234567890abcdef"),
                &String::from_str(&env, "audio"),
                &LicenseType::AllRightsReserved,
                &500, // 5% royalty
            )
            .unwrap();

        assert_eq!(content_id, 1);

        let viewer = Address::generate(&env);
        let total_views = client.record_view(&viewer, &content_id).unwrap();
        assert_eq!(total_views, 1);

        let stats = client.get_viewer_stats(&content_id).unwrap();
        assert_eq!(stats.total_views, 1);
        assert_eq!(stats.unique_viewers, 1);
    }

    #[test]
    fn test_create_and_update_job() {
        let (env, owner) = setup();
        let client = CreatorPlatformContractClient::new(&env, &env.register_contract(None, CreatorPlatformContract {}));
        let channel_id = client
            .create_channel(&owner, &String::from_str(&env, "Game"), &String::from_str(&env, "Gaming"), &String::from_str(&env, "gaming"), &owner)
            .unwrap();

        let now = env.ledger().timestamp();
        let job_id = client
            .create_job(
                &owner,
                &channel_id,
                &String::from_str(&env, "Saturday Livestream"),
                &String::from_str(&env, "Weekly gaming stream"),
                &JobType::LiveStream,
                &(now + 3600),
                &(now + 7200),
                &0i128,
            )
            .unwrap();

        assert_eq!(job_id, 1);

        client.update_job_status(&owner, &job_id, &JobStatus::InProgress).unwrap();
        let job = client.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::InProgress);
    }

    #[test]
    fn test_copyright_dispute_flow() {
        let (env, owner) = setup();
        let client = CreatorPlatformContractClient::new(&env, &env.register_contract(None, CreatorPlatformContract {}));
        let channel_id = client
            .create_channel(&owner, &String::from_str(&env, "Vlog"), &String::from_str(&env, "My vlogs"), &String::from_str(&env, "vlog"), &owner)
            .unwrap();

        let content_id = client
            .register_content(
                &owner, &channel_id,
                &String::from_str(&env, "Vlog #1"),
                &String::from_str(&env, "Day in my life"),
                &String::from_str(&env, "QmABC123"),
                &String::from_str(&env, "video"),
                &LicenseType::CreativeCommons,
                &0,
            )
            .unwrap();

        let claimer = Address::generate(&env);
        client.dispute_content(&claimer, &content_id).unwrap();

        let content = client.get_content(&content_id).unwrap();
        assert_eq!(content.copyright_status, CopyrightStatus::Disputed);

        let admin = Address::generate(&env);
        client.resolve_dispute(&admin, &content_id, &true).unwrap();

        let content_after = client.get_content(&content_id).unwrap();
        assert_eq!(content_after.copyright_status, CopyrightStatus::Registered);
    }
}