//! SCP 直传：复用终端的 SSH 连接，在 shell 泵线程上分块执行。
//!
//! 每写完一块就把后续工作重新排到泵队列末尾，块与块之间泵照常处理 PTY 输入输出，
//! 大文件上传不会让终端停住；本地文件按块读取，不整份载入内存。

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use super::{SessionBlockingGuard, SshSessionHandle};
use crate::i18n::Locale;

/// 单个泵任务最多写入的字节数：越小终端响应越及时，越大调度开销越低。
pub const SCP_CHUNK_BYTES: usize = 64 * 1024;

pub type SessionJob = Box<dyn FnOnce(&ssh2::Session) + Send>;

/// 能把任务排到「独占 Session 的线程」上执行的队列（生产环境即 shell 泵）。
pub trait SessionJobQueue: Clone + Send + 'static {
    fn enqueue(&self, job: SessionJob) -> Result<(), String>;
}

impl SessionJobQueue for SshSessionHandle {
    fn enqueue(&self, job: SessionJob) -> Result<(), String> {
        self.enqueue_session_job(job)
    }
}

pub struct ScpUpload {
    local_path: PathBuf,
    file: File,
    size: u64,
    sent: u64,
    remote_path: String,
    channel: Option<ssh2::Channel>,
    buf: Vec<u8>,
    locale: Locale,
    result_tx: Sender<Result<String, String>>,
}

impl ScpUpload {
    /// 在调用线程打开本地文件（失败立即返回），再把第一块排进队列。
    /// 结果（成功时为远端路径）经 `result_tx` 回传；会话断开导致任务被丢弃时 `result_tx` 随之 drop。
    pub fn start<Q: SessionJobQueue>(
        queue: &Q,
        local_path: &Path,
        remote_path: String,
        locale: Locale,
        result_tx: Sender<Result<String, String>>,
    ) -> Result<(), String> {
        let read_err =
            |e: std::io::Error| format!("{} {}", locale.tr("Failed to read file:", "读取文件失败："), e);
        let file = File::open(local_path).map_err(read_err)?;
        let size = file.metadata().map_err(read_err)?.len();
        let upload = Self {
            local_path: local_path.to_path_buf(),
            file,
            size,
            sent: 0,
            remote_path,
            channel: None,
            buf: Vec::new(),
            locale,
            result_tx,
        };
        let next = queue.clone();
        queue.enqueue(Box::new(move |session| upload.run(session, &next)))
    }

    fn run<Q: SessionJobQueue>(mut self, session: &ssh2::Session, queue: &Q) {
        loop {
            match self.step(session) {
                Ok(true) => {
                    log::info!(
                        "SSH SCP upload finished: {} ({} bytes)",
                        self.local_path.display(),
                        self.size
                    );
                    let _ = self.result_tx.send(Ok(self.remote_path.clone()));
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    log::warn!("SSH SCP upload failed: {} {}", self.local_path.display(), e);
                    let _ = self.result_tx.send(Err(e));
                    return;
                }
            }
            let slot = Arc::new(Mutex::new(Some(self)));
            let job_slot = Arc::clone(&slot);
            let next = queue.clone();
            let queued = queue.enqueue(Box::new(move |session| {
                if let Some(upload) = take_slot(&job_slot) {
                    upload.run(session, &next);
                }
            }));
            if queued.is_ok() {
                return;
            }
            // 队列满被拒时任务未执行、状态仍在 slot 里：本轮直接续传，而不是让上传失败。
            match take_slot(&slot) {
                Some(upload) => self = upload,
                None => return,
            }
        }
    }

    /// 写一块；全部写完并关闭通道后返回 `Ok(true)`。
    fn step(&mut self, session: &ssh2::Session) -> Result<bool, String> {
        let _blocking = SessionBlockingGuard::new(session);
        let loc = self.locale;
        let fail = |en: &'static str, zh: &'static str, e: &dyn std::fmt::Display| {
            format!("{} {}", loc.tr(en, zh), e)
        };

        if self.channel.is_none() {
            log::info!(
                "Starting SSH SCP upload: {} ({} bytes)",
                self.local_path.display(),
                self.size
            );
            let channel = session
                .scp_send(Path::new(&self.remote_path), 0o644, self.size, None)
                .map_err(|e| fail("Failed to open SCP channel:", "创建 SCP 通道失败：", &e))?;
            self.channel = Some(channel);
        }
        let Some(channel) = self.channel.as_mut() else {
            return Err(loc.tr("SCP channel missing", "SCP 通道丢失").to_string());
        };

        if self.sent < self.size {
            let n = (self.size - self.sent).min(SCP_CHUNK_BYTES as u64) as usize;
            self.buf.resize(n, 0);
            self.file
                .read_exact(&mut self.buf)
                .map_err(|e| fail("Failed to read file:", "读取文件失败：", &e))?;
            channel
                .write_all(&self.buf)
                .map_err(|e| fail("SCP write failed:", "SCP 写入失败：", &e))?;
            self.sent += n as u64;
            if self.sent < self.size {
                return Ok(false);
            }
        }

        channel
            .send_eof()
            .map_err(|e| fail("SCP send_eof failed:", "SCP 发送 EOF 失败：", &e))?;
        channel
            .wait_eof()
            .map_err(|e| fail("SCP wait_eof failed:", "SCP 等待 EOF 失败：", &e))?;
        channel
            .close()
            .map_err(|e| fail("SCP close failed:", "SCP 关闭失败：", &e))?;
        channel
            .wait_close()
            .map_err(|e| fail("SCP wait_close failed:", "SCP 等待关闭失败：", &e))?;
        Ok(true)
    }
}

fn take_slot(slot: &Mutex<Option<ScpUpload>>) -> Option<ScpUpload> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::UiLanguage;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    /// 模拟 shell 泵：任务排队，由测试线程按顺序执行；`cap` 为队列容量。
    #[derive(Clone)]
    struct TestQueue {
        jobs: Arc<Mutex<VecDeque<SessionJob>>>,
        cap: usize,
    }

    impl TestQueue {
        fn new(cap: usize) -> Self {
            Self { jobs: Arc::default(), cap }
        }

        fn pop(&self) -> Option<SessionJob> {
            self.jobs.lock().unwrap().pop_front()
        }
    }

    impl SessionJobQueue for TestQueue {
        fn enqueue(&self, job: SessionJob) -> Result<(), String> {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.len() >= self.cap {
                return Err("queue full".into());
            }
            jobs.push_back(job);
            Ok(())
        }
    }

    fn locale() -> Locale {
        Locale::from(UiLanguage::En)
    }

    fn temp_file(len: usize) -> (tempfile::NamedTempFile, Vec<u8>) {
        let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&data).unwrap();
        f.flush().unwrap();
        (f, data)
    }

    fn remote_path(tag: &str) -> String {
        format!(
            "{}/mistterm_scp_upload_{}_{}",
            crate::test_support::ssh_local::ssh_remote_sftp_root(),
            tag,
            std::process::id()
        )
    }

    fn read_remote(session: &ssh2::Session, path: &str) -> Vec<u8> {
        session.set_blocking(true);
        let (mut ch, _) = session.scp_recv(Path::new(path)).unwrap();
        let mut out = Vec::new();
        ch.read_to_end(&mut out).unwrap();
        let _ = crate::test_support::ssh_local::exec_remote(session, &format!("rm -f {path}"));
        out
    }

    #[test]
    fn unreadable_local_file_fails_before_enqueue() {
        let queue = TestQueue::new(8);
        let (tx, _rx) = mpsc::channel();
        let err = ScpUpload::start(
            &queue,
            Path::new("/nonexistent/mistterm-upload-src"),
            "./x".into(),
            locale(),
            tx,
        )
        .unwrap_err();
        assert!(err.starts_with("Failed to read file:"), "{err}");
        assert!(queue.pop().is_none());
    }

    #[test]
    fn dropped_job_disconnects_result_channel() {
        let (file, _) = temp_file(10);
        let queue = TestQueue::new(8);
        let (tx, rx) = mpsc::channel();
        ScpUpload::start(&queue, file.path(), "./x".into(), locale(), tx).unwrap();
        drop(queue.pop());
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn upload_yields_between_chunks_and_lets_other_jobs_run() {
        let Some(session) = crate::test_support::ssh_local::skip_without_sshd() else {
            return;
        };
        let (file, data) = temp_file(SCP_CHUNK_BYTES * 3 + 123);
        let remote = remote_path("yield");
        let queue = TestQueue::new(8);
        let (tx, rx) = mpsc::channel();
        ScpUpload::start(&queue, file.path(), remote.clone(), locale(), tx).unwrap();

        // 泵里始终还有一条「PTY 工作」排在后面：上传每块之后都应轮到它。
        let events: Arc<Mutex<Vec<&str>>> = Arc::default();
        let pty_work = |events: &Arc<Mutex<Vec<&'static str>>>| -> SessionJob {
            let events = Arc::clone(events);
            Box::new(move |_| events.lock().unwrap().push("pty"))
        };
        queue.enqueue(pty_work(&events)).unwrap();
        let mut result = None;
        while let Some(job) = queue.pop() {
            let before = events.lock().unwrap().len();
            job(&session);
            let ran_pty = events.lock().unwrap().len() > before;
            if !ran_pty {
                events.lock().unwrap().push("chunk");
                assert!(!session.is_blocking(), "blocking mode restored after each chunk");
            }
            if result.is_none() {
                result = rx.try_recv().ok();
            }
            if ran_pty && result.is_none() {
                queue.enqueue(pty_work(&events)).unwrap();
            }
        }

        assert_eq!(result, Some(Ok(remote.clone())));
        assert_eq!(
            *events.lock().unwrap(),
            ["chunk", "pty", "chunk", "pty", "chunk", "pty", "chunk", "pty"],
            "3 full chunks + 1 tail chunk, each followed by other pump work"
        );
        assert_eq!(read_remote(&session, &remote), data);
    }

    #[test]
    fn full_queue_continues_inline_instead_of_failing() {
        let Some(session) = crate::test_support::ssh_local::skip_without_sshd() else {
            return;
        };
        let (file, data) = temp_file(SCP_CHUNK_BYTES * 2 + 7);
        let remote = remote_path("inline");
        let queue = TestQueue::new(1);
        let (tx, rx) = mpsc::channel();
        ScpUpload::start(&queue, file.path(), remote.clone(), locale(), tx).unwrap();
        let first = queue.pop().unwrap();
        queue.enqueue(Box::new(|_| {})).unwrap();
        first(&session);

        assert_eq!(rx.try_recv().unwrap(), Ok(remote.clone()));
        assert_eq!(read_remote(&session, &remote), data);
    }
}
