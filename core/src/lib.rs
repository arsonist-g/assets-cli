//! assets-core：唯一真相引擎。
//!
//! 变量名生成与校验、注册表读写、元数据与快照、清单产出、自检与安装。
//! CLI 与 GUI 都只调用这一层，两处落点（数据目录、注册表根）也只在 `paths` 里解析。

pub mod doctor;
pub mod error;
pub mod init;
pub mod manifest;
pub mod model;
pub mod naming;
pub mod ops;
pub mod paths;
pub mod registry;
pub mod snapshot;
pub mod store;

pub use error::{CoreError, Result};
pub use ops::Ctx;
