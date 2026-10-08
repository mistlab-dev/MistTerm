//! 系统文件选择框。
//!
//! 桌面版（默认开启 `file-dialogs`）直接用 `rfd`；在 Linux 上它依赖 GTK3。
//! 只编命令行 `mist` 的静态版本（musl，见 `docs/release/CLI_STATIC.md`）时会关掉这个
//! feature，这里换成一个永远返回 `None` 的同名替身，界面代码不用到处加条件编译。

#[cfg(feature = "file-dialogs")]
pub use rfd::FileDialog;

#[cfg(not(feature = "file-dialogs"))]
pub use stub::FileDialog;

#[cfg(not(feature = "file-dialogs"))]
mod stub {
    use std::path::{Path, PathBuf};

    /// 没有文件选择框的构建：所有选择都当作用户取消。
    #[derive(Debug, Default, Clone)]
    pub struct FileDialog;

    impl FileDialog {
        pub fn new() -> Self {
            Self
        }
        pub fn set_title(self, _title: impl Into<String>) -> Self {
            self
        }
        pub fn set_file_name(self, _name: impl Into<String>) -> Self {
            self
        }
        pub fn set_directory<P: AsRef<Path>>(self, _path: P) -> Self {
            self
        }
        pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            None
        }
        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            None
        }
        pub fn pick_folder(self) -> Option<PathBuf> {
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            None
        }
    }
}
