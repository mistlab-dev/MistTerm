//! Fragment / 命令面板 / 市场目录 / 发送序列 / 粘贴历史 域状态（#301 P1b，纯搬家）。
//!
//! app.rs 该域原有 44 个散字段；本 struct 收编字段，方法跟域走由同 PR 后续提交继续。
//! 跨域编排仍留在 MistTermApp 薄包装；域内字段保持 crate 内可见。

use crate::core::fragment::{FragmentManager, FragmentStats, SortBy};
use crate::core::fragment_recommendations::FragmentCandidate;
use crate::core::fragment_shortcuts::FragmentShortcutStore;
use crate::core::paste_history::PasteHistory;
use crate::ui::app::{FragmentListScope, FragmentQuickSelector, FragmentVariableDialog, FragmentVarsCompletion};
use crate::ui::team_fragment_dialog::TeamFragmentEditorState;
use crate::ui::fragment_library::FragmentLibraryState;
use crate::ui::paste_history_overlay::PasteHistoryOverlay;
use crate::ui::team_fragment_dialog::TeamFragmentConflictState;
use crate::ui::team_fragment_extras_dialog::{FragmentSharesState, FragmentVersionsState};


pub(crate) struct FragmentState {
    pub(crate) fragment_manager: FragmentManager,
    pub(crate) fragment_shortcut_store: FragmentShortcutStore,
    pub(crate) send_sequences: crate::core::SendSequenceStore,
    pub(crate) send_sequence_dialog: crate::ui::send_sequence_dialog::SendSequenceDialog,
    pub(crate) show_fragments_dialog: bool,
    pub(crate) show_fragment_panel: bool, // 命令片段侧边栏,
    pub(crate) fragment_panel_slot_rect: Option<egui::Rect>,
    pub(crate) fragment_search_query: String,
    pub(crate) fragment_sort_by: SortBy,
    pub(crate) variable_dialog: FragmentVariableDialog,
    pub(crate) fragment_vars_completion: FragmentVarsCompletion,
    pub(crate) quick_selector: FragmentQuickSelector,
    pub(crate) command_palette: crate::ui::command_palette::CommandPalette,
    pub(crate) fragment_library: FragmentLibraryState,
    pub(crate) team_fragment_editor: TeamFragmentEditorState,
    pub(crate) team_fragment_conflict: Option<TeamFragmentConflictState>,
    pub(crate) team_fragment_versions: FragmentVersionsState,
    pub(crate) team_fragment_shares: FragmentSharesState,
    pub(crate) team_fragment_selected_id: Option<String>,
    pub(crate) fragment_list_scope: FragmentListScope,
    pub(crate) show_fragment_analytics_dialog: bool,
    pub(crate) pending_fragment_candidate: Option<FragmentCandidate>,
    pub(crate) fragment_analytics_snapshot: crate::core::FragmentAnalyticsDashboard,
    pub(crate) fragment_analytics_range: crate::core::FragmentAnalyticsTimeRange,
    pub(crate) fragment_usage_log: crate::core::FragmentUsageLog,
    pub(crate) fragment_recommendations: Vec<crate::core::FragmentRecommendation>,
    pub(crate) market_catalog: crate::core::MarketCatalogState,
    pub(crate) market_catalog_refresh_pending: bool,
    pub(crate) market_catalog_refresh_rx: Option<std::sync::mpsc::Receiver<crate::core::MarketCatalogState>>,
    pub(crate) market_catalog_query_fingerprint: (String, String),
    pub(crate) market_catalog_debounce_deadline: Option<std::time::Instant>,
    pub(crate) pending_fragment_id: Option<String>,
    pub(crate) pending_fragment_name: String,
    pub(crate) pending_fragment_command: String,
    pub(crate) pending_fragment_command_edit: String,
    pub(crate) pending_fragment_vars: Vec<(String, String)>,
    pub(crate) show_fragment_vars_dialog: bool,
    pub(crate) fragment_filter_category: String,
    pub(crate) fragment_filter_status: String,
    pub(crate) pending_fragment_insert: Option<(usize, Option<String>, String)>,
    pub(crate) fragment_insert_only: bool,
    pub(crate) paste_history: PasteHistory,
    pub(crate) paste_history_overlay: PasteHistoryOverlay,
    pub(crate) pending_suggested_snippet: Option<(usize, FragmentStats)>,
}

impl FragmentState {
    /// 从 app.rs `new()` 原初始化表达式搬家（保持启动语义不变）。
    pub(crate) fn new(market_cache: &crate::core::market::MarketFragmentCache) -> Self {
        Self {
            fragment_manager: FragmentManager::load(&FragmentManager::default_config_path())
                .unwrap_or_else(|_| {
                    FragmentManager::init_from_market_or_defaults(Some(market_cache))
                }),
            fragment_shortcut_store: FragmentShortcutStore::load(),
            send_sequences: crate::core::SendSequenceStore::load(),
            send_sequence_dialog: crate::ui::send_sequence_dialog::SendSequenceDialog::default(),
            show_fragments_dialog: false,
            show_fragment_panel: false,
            fragment_panel_slot_rect: None,
            fragment_search_query: String::new(),
            fragment_sort_by: SortBy::UsageCount,
            variable_dialog: FragmentVariableDialog::default(),
            fragment_vars_completion: FragmentVarsCompletion::default(),
            quick_selector: FragmentQuickSelector::default(),
            command_palette: crate::ui::command_palette::CommandPalette::default(),
            fragment_library: FragmentLibraryState::new(),
            team_fragment_editor: TeamFragmentEditorState::default(),
            team_fragment_conflict: None,
            team_fragment_versions: FragmentVersionsState::default(),
            team_fragment_shares: FragmentSharesState::default(),
            team_fragment_selected_id: None,
            fragment_list_scope: FragmentListScope::Personal,
            show_fragment_analytics_dialog: false,
            pending_fragment_candidate: None,
            fragment_analytics_snapshot: crate::core::FragmentAnalyticsDashboard::default(),
            fragment_analytics_range: crate::core::FragmentAnalyticsTimeRange::default(),
            fragment_usage_log: crate::core::FragmentUsageLog::load(),
            fragment_recommendations: Vec::new(),
            market_catalog: crate::core::MarketCatalogState::load(),
            market_catalog_refresh_pending: false,
            market_catalog_refresh_rx: None,
            market_catalog_query_fingerprint: (String::new(), String::new()),
            market_catalog_debounce_deadline: None,
            pending_fragment_id: None,
            pending_fragment_name: String::new(),
            pending_fragment_command: String::new(),
            pending_fragment_command_edit: String::new(),
            pending_fragment_vars: Vec::new(),
            show_fragment_vars_dialog: false,
            fragment_filter_category: "all".to_string(),
            fragment_filter_status: "all".to_string(),
            pending_fragment_insert: None,
            fragment_insert_only: false,
            paste_history: PasteHistory::new(),
            paste_history_overlay: PasteHistoryOverlay::default(),
            pending_suggested_snippet: None,
        }
    }
}
