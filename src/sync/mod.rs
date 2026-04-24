//! Git 同步模块
//!
//! 支持通过 Git 仓库同步会话配置和命令片段

mod git;

pub use git::{GitRepo, GitError};
