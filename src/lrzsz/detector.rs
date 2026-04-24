//! lrzsz 命令检测器
//!
//! 检测终端输出中的 rz/sz 命令，触发文件传输流程

use std::sync::mpsc::Sender;

/// lrzsz 事件
#[derive(Debug, Clone)]
pub enum LrzszEvent {
    /// 准备上传文件（检测到 rz 命令）
    UploadReady,
    /// 准备下载文件（检测到 sz 命令）
    DownloadReady(String), // 文件名
}

/// lrzsz 检测器
pub struct LrzszDetector {
    /// 缓冲区，用于累积输出
    buffer: String,
    /// 最大缓冲区大小（行数）
    max_buffer_lines: usize,
}

impl LrzszDetector {
    /// 创建新的检测器
    pub fn new() -> Self {
        LrzszDetector {
            buffer: String::new(),
            max_buffer_lines: 100,
        }
    }

    /// 检测 lrzsz 命令
    ///
    /// # Arguments
    /// * `output` - 终端输出文本
    ///
    /// # Returns
    /// 如果检测到 lrzsz 命令，返回对应的事件
    pub fn detect(&mut self, output: &str) -> Option<LrzszEvent> {
        // 累积输出到缓冲区
        self.buffer.push_str(output);

        // 限制缓冲区大小
        let lines: Vec<&str> = self.buffer.lines().collect();
        if lines.len() > self.max_buffer_lines {
            // 保留最后 N 行
            self.buffer = lines
                .iter()
                .skip(lines.len() - self.max_buffer_lines)
                .copied()
                .collect::<Vec<&str>>()
                .join("\n");
        }

        // 检测 rz 命令（上传）
        if self.detect_rz(output) {
            return Some(LrzszEvent::UploadReady);
        }

        // 检测 sz 命令（下载）
        if let Some(filename) = self.detect_sz(output) {
            return Some(LrzszEvent::DownloadReady(filename));
        }

        None
    }

    /// 检测 rz 命令
    fn detect_rz(&self, output: &str) -> bool {
        let output_lower = output.to_lowercase();

        // 常见的 rz 命令触发提示
        let patterns = [
            "rz ready",
            "rj",
            "zmodem",
            "rz ",
            "rz -",
            "sending file",
            "transferring",
        ];

        patterns.iter().any(|p| output_lower.contains(p))
    }

    /// 检测 sz 命令，返回文件名
    fn detect_sz(&self, output: &str) -> Option<String> {
        // 匹配 "sz filename" 格式
        if output.starts_with("sz ") {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }

        // 匹配 "Sending filename" 格式
        if output.starts_with("Sending ") {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }

        // 匹配 "transferring filename" 格式
        let output_lower = output.to_lowercase();
        if output_lower.starts_with("transferring ") {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }

        None
    }

    /// 重置检测器
    pub fn reset(&mut self) {
        self.buffer.clear();
    }

    /// 获取当前缓冲区内容（用于调试）
    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }
}

impl Default for LrzszDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rz() {
        let mut detector = LrzszDetector::new();

        assert!(detector.detect("rz ready").is_some());
        assert!(detector.detect("Zmodem ready").is_some());
        assert!(detector.detect("rz -y").is_some());
    }

    #[test]
    fn test_detect_sz() {
        let mut detector = LrzszDetector::new();

        if let Some(LrzszEvent::DownloadReady(filename)) = detector.detect("sz test.txt") {
            assert_eq!(filename, "test.txt");
        } else {
            panic!("Expected DownloadReady event");
        }

        if let Some(LrzszEvent::DownloadReady(filename)) = detector.detect("Sending file.txt") {
            assert_eq!(filename, "file.txt");
        } else {
            panic!("Expected DownloadReady event");
        }
    }

    #[test]
    fn test_no_false_positive() {
        let mut detector = LrzszDetector::new();

        // 这些不应该触发 lrzsz
        assert!(detector.detect("hello world").is_none());
        assert!(detector.detect("ls -la").is_none());
        assert!(detector.detect("echo test").is_none());
    }
}
