//! Mist 团队平台客户端（`api.mistlab.dev`）。

mod auth;
mod cache;
mod client;
mod models;
mod oauth;
mod service;
mod settings;
mod state;
mod sync_config;

pub use auth::{jwt_exp_unix, token_needs_refresh, TeamTokenStore};
pub use cache::TeamFragmentCache;
pub use client::{TeamApiError, TeamClient};
pub use oauth::OAuthProvider;
pub use models::{
    parse_tags_json, parse_variables_json, CreateTeamFragmentRequest, FragmentAnalyticsResponse,
    FragmentAnalyticsRow, TeamFragment, TeamMember, TeamMembersResponse, TeamMembership, TeamRole,
    TeamServer, TeamSyncEntry, TeamSyncResponse, TeamUser, TeamsListResponse, TokenResponse,
    UpdateTeamFragmentRequest,
};
pub use sync_config::{apply_sync_response, apply_vault_for_team, parse_vault_credential_path};
pub use service::{
    create_team_fragment_blocking, delete_team_fragment_blocking,
    do_sync, ensure_access_token, sync_fragments_blocking, update_team_fragment_blocking,
    TeamAsyncResult, TeamService,
};
pub use settings::{
    normalize_api_base, team_web_forgot_password_url, team_web_oauth_desktop_callback_url,
    team_web_register_url, TeamSettings, DEFAULT_TEAM_API_BASE, DEFAULT_TEAM_WEB_ORIGIN,
    OAUTH_LOCAL_PORT,
};
pub use state::TeamState;
