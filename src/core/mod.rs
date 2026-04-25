//! 核心业务逻辑层

pub mod session;
mod connection;

pub use session::{SessionConfig, SessionManager};
