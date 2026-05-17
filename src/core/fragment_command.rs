//! 命令片段展开与校验（无 UI）
//!
//! 将 Rhai、`<占位符>`、会话字段替换集中在此，供弹窗预览与发送前校验共用。

use std::collections::HashMap;

use crate::core::fragment::{
    expand_command_template, expand_fragment_command_stages, FragmentStats,
};
use crate::core::fragment_expr::{expand_rhai_blocks, merge_rhai_context};
use crate::core::session::SessionConfig;

/// 变量填写弹窗中的命令预览
pub fn build_fragment_command_preview(
    fragment: &FragmentStats,
    session: Option<&SessionConfig>,
    values: &HashMap<String, String>,
) -> String {
    expand_fragment_command_stages(&fragment.command, session, values).unwrap_or_else(|_| {
        let after = fragment.apply_variables(values);
        let ctx = merge_rhai_context(session, values);
        expand_rhai_blocks(&after, &ctx)
            .map(|rh| expand_command_template(&rh, session, values))
            .unwrap_or_else(|_| expand_command_template(&after, session, values))
    })
}

/// 发送前最终展开（含 Rhai 块内 `<user>` 等）
pub fn finalize_fragment_command_text(
    text: &str,
    session: Option<&SessionConfig>,
    values: &HashMap<String, String>,
) -> Result<String, String> {
    expand_fragment_command_stages(text, session, values)
}
