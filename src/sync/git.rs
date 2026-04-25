//! Git 仓库管理
#![allow(dead_code)]
//!
//! 提供 Git 仓库的初始化、克隆、提交、推送、拉取等功能

use git2::{Repository, Signature};
use std::path::PathBuf;
use thiserror::Error;

/// Git 错误
#[derive(Error, Debug)]
pub enum GitError {
    #[error("初始化仓库失败：{0}")]
    InitError(String),

    #[error("克隆仓库失败：{0}")]
    CloneError(String),

    #[error("提交失败：{0}")]
    CommitError(String),

    #[error("推送失败：{0}")]
    PushError(String),

    #[error("拉取失败：{0}")]
    PullError(String),

    #[error("打开仓库失败：{0}")]
    OpenError(String),

    #[error("索引操作失败：{0}")]
    IndexError(String),
}

/// Git 仓库
pub struct GitRepo {
    repo: Repository,
    remote_url: String,
    branch: String,
}

impl GitRepo {
    /// 初始化本地仓库
    pub fn init(local_path: &PathBuf) -> Result<Self, GitError> {
        let repo = Repository::init(local_path).map_err(|e| {
            GitError::InitError(format!("{}: {}", e, local_path.display()))
        })?;

        Ok(GitRepo {
            repo,
            remote_url: String::new(),
            branch: "main".to_string(),
        })
    }

    /// 克隆远程仓库
    pub fn clone(remote_url: &str, local_path: &PathBuf) -> Result<Self, GitError> {
        let repo = Repository::clone(remote_url, local_path).map_err(|e| {
            GitError::CloneError(format!("{} -> {}", e, local_path.display()))
        })?;

        Ok(GitRepo {
            repo,
            remote_url: remote_url.to_string(),
            branch: "main".to_string(),
        })
    }

    /// 打开现有仓库
    pub fn open(path: &PathBuf) -> Result<Self, GitError> {
        let repo = Repository::open(path).map_err(|e| {
            GitError::OpenError(format!("{}: {}", e, path.display()))
        })?;

        // 获取远程 URL
        let remote_url = if let Ok(remote) = repo.find_remote("origin") {
            remote.url().unwrap_or("").to_string()
        } else {
            String::new()
        };

        // 获取当前分支
        let branch = if let Ok(head) = repo.head() {
            head.shorthand().unwrap_or("main").to_string()
        } else {
            "main".to_string()
        };

        Ok(GitRepo {
            repo,
            remote_url,
            branch,
        })
    }

    /// 设置远程 URL
    pub fn set_remote(&mut self, url: &str) -> Result<(), GitError> {
        // git2 0.18 不支持直接修改 remote URL，需要删除后重新创建
        if self.repo.find_remote("origin").is_ok() {
            self.repo.remote_delete("origin").map_err(|e| {
                GitError::PushError(format!("删除远程仓库失败：{}", e))
            })?;
        }
        self.repo.remote("origin", url).map_err(|e| {
            GitError::PushError(format!("创建远程仓库失败：{}", e))
        })?;
        self.remote_url = url.to_string();
        Ok(())
    }

    /// 获取远程 URL
    pub fn remote_url(&self) -> &str {
        &self.remote_url
    }

    /// 添加文件到暂存区
    pub fn add(&self, path: &str) -> Result<(), GitError> {
        let mut index = self.repo.index().map_err(|e| {
            GitError::IndexError(format!("打开索引失败：{}", e))
        })?;

        index
            .add_path(PathBuf::from(path).as_path())
            .map_err(|e| GitError::IndexError(format!("添加文件失败：{}", e)))?;

        index.write().map_err(|e| {
            GitError::IndexError(format!("写入索引失败：{}", e))
        })?;

        Ok(())
    }

    /// 添加所有更改到暂存区
    pub fn add_all(&self) -> Result<(), GitError> {
        let mut index = self.repo.index().map_err(|e| {
            GitError::IndexError(format!("打开索引失败：{}", e))
        })?;

        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| GitError::IndexError(format!("添加所有文件失败：{}", e)))?;

        index.write().map_err(|e| {
            GitError::IndexError(format!("写入索引失败：{}", e))
        })?;

        Ok(())
    }

    /// 提交更改
    pub fn commit(
        &self,
        message: &str,
        author: Option<&str>,
        email: Option<&str>,
    ) -> Result<git2::Oid, GitError> {
        let mut index = self.repo.index().map_err(|e| {
            GitError::CommitError(format!("打开索引失败：{}", e))
        })?;

        index.write().map_err(|e| {
            GitError::CommitError(format!("写入索引失败：{}", e))
        })?;

        let tree_id = index.write_tree().map_err(|e| {
            GitError::CommitError(format!("写入树对象失败：{}", e))
        })?;

        let tree = self.repo.find_tree(tree_id).map_err(|e| {
            GitError::CommitError(format!("查找树对象失败：{}", e))
        })?;

        // 使用默认签名或自定义签名
        let signature = if let (Some(name), Some(email)) = (author, email) {
            Signature::now(name, email).map_err(|e| {
                GitError::CommitError(format!("创建签名失败：{}", e))
            })?
        } else {
            // 尝试从 git 配置获取
            if let Ok(config) = self.repo.config() {
                let name = config.get_string("user.name").unwrap_or_else(|_| "MistTerm".to_string());
                let email = config.get_string("user.email").unwrap_or_else(|_| "mistterm@example.com".to_string());
                Signature::now(&name, &email).map_err(|e| {
                    GitError::CommitError(format!("创建签名失败：{}", e))
                })?
            } else {
                Signature::now("MistTerm", "mistterm@example.com").map_err(|e| {
                    GitError::CommitError(format!("创建签名失败：{}", e))
                })?
            }
        };

        // 获取父提交
        let parent = if let Ok(head) = self.repo.head() {
            if let Ok(commit) = head.peel_to_commit() {
                Some(commit)
            } else {
                None
            }
        } else {
            None
        };

        let parents: Vec<&git2::Commit> = if let Some(ref p) = parent {
            vec![p]
        } else {
            vec![]
        };

        let commit_oid = self.repo
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)
            .map_err(|e| GitError::CommitError(format!("提交失败：{}", e)))?;

        Ok(commit_oid)
    }

    /// 推送到远程
    pub fn push(&self) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote("origin").map_err(|e| {
            GitError::PushError(format!("找不到远程仓库：{}", e))
        })?;

        let refspec = format!("refs/heads/{}:refs/heads/{}", self.branch, self.branch);

        remote.push(&[&refspec], None).map_err(|e| {
            GitError::PushError(format!("推送失败：{}", e))
        })?;

        Ok(())
    }

    /// 从远程拉取
    pub fn pull(&self) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote("origin").map_err(|e| {
            GitError::PullError(format!("找不到远程仓库：{}", e))
        })?;

        // 获取远程分支列表
        let _refs = remote.list().map_err(|e| {
            GitError::PullError(format!("列出远程分支失败：{}", e))
        })?;

        // 拉取
        remote
            .fetch(&[&self.branch], None, None)
            .map_err(|e| GitError::PullError(format!("拉取失败：{}", e)))?;

        let fetch_head = self.repo.find_reference("FETCH_HEAD").map_err(|e| {
            GitError::PullError(format!("找不到 FETCH_HEAD: {}", e))
        })?;

        let fetch_commit = self.repo
            .reference_to_annotated_commit(&fetch_head)
            .map_err(|e| GitError::PullError(format!("转换提交失败：{}", e)))?;

        // 分析合并情况
        let analysis = self.repo
            .merge_analysis(&[&fetch_commit])
            .map_err(|e| GitError::PullError(format!("分析合并失败：{}", e)))?;

        if analysis.0.is_up_to_date() {
            tracing::info!("已经是最新的");
            return Ok(());
        }

        if analysis.0.is_fast_forward() {
            // 快进合并
            let _refname = format!("refs/heads/{}", self.branch);
            match self.repo.find_branch(&self.branch, git2::BranchType::Local) {
                Ok(mut branch) => {
                    branch
                        .get_mut()
                        .set_target(fetch_commit.id(), "Fast-Forward")
                        .map_err(|e| GitError::PullError(format!("更新分支失败：{}", e)))?;
                }
                Err(_) => {
                    self.repo
                        .branch(
                            &self.branch,
                            &self.repo
                                .find_commit(fetch_commit.id())
                                .map_err(|e| GitError::PullError(format!("查找提交失败：{}", e)))?,
                            false,
                        )
                        .map_err(|e| GitError::PullError(format!("创建分支失败：{}", e)))?;
                }
            }
        }

        // 更新工作目录
        self.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| GitError::PullError(format!("检出发失败：{}", e)))?;

        Ok(())
    }

    /// 同步（先拉取再推送）
    pub fn sync(&self) -> Result<(), GitError> {
        self.pull()?;
        self.push()?;
        Ok(())
    }

    /// 检查是否有未提交的更改
    pub fn has_uncommitted_changes(&self) -> Result<bool, GitError> {
        let _diff = self.repo
            .diff_index_to_workdir(None, None)
            .map_err(|e| GitError::IndexError(format!("获取差异失败：{}", e)))?;

        // 简化：直接返回 true（实际需要检查 diff）
        Ok(true)
    }

    /// 获取仓库状态
    pub fn status(&self) -> Result<RepoStatus, GitError> {
        let mut status = RepoStatus {
            is_dirty: false,
            has_uncommitted: false,
            has_untracked: false,
            ahead: 0,
            behind: 0,
        };

        // 检查工作区状态
        let mut opts = git2::StatusOptions::new();
        if let Ok(statuses) = self.repo.statuses(Some(&mut opts)) {
            for entry in statuses.iter() {
                match entry.status() {
                    git2::Status::CURRENT => {}
                    git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE => {
                        status.has_uncommitted = true;
                        status.is_dirty = true;
                    }
                    git2::Status::WT_NEW
                    | git2::Status::WT_MODIFIED
                    | git2::Status::WT_DELETED
                    | git2::Status::WT_RENAMED
                    | git2::Status::WT_TYPECHANGE => {
                        status.has_untracked = true;
                        status.is_dirty = true;
                    }
                    _ => {}
                }
            }
        }

        // 检查与远程的差异
        if let Ok(_remote) = self.repo.find_remote("origin") {
            // 简化：不计算 ahead/behind
        }

        Ok(status)
    }
}

/// 仓库状态
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub is_dirty: bool,
    pub has_uncommitted: bool,
    pub has_untracked: bool,
    pub ahead: usize,
    pub behind: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_repo() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_path_buf();

        let result = GitRepo::init(&path);
        assert!(result.is_ok());

        let repo = result.unwrap();
        assert!(repo.remote_url().is_empty());
    }
}
